//! Ref resolution: loose refs, `packed-refs`, symbolic refs, tags and the
//! stash reflog.

use super::{read, Repo, Unsupported};
use std::fs;
use std::path::Path;

/// Symbolic refs followed before giving up, as git's own limit.
const MAX_DEPTH: usize = 5;
/// More loose tags than this are not walked; git itself is asked instead.
const MAX_LOOSE_TAGS: usize = 1000;

/// A full object id: 40 (SHA-1) or 64 (SHA-256) hex digits.
pub fn is_oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Refs kept per worktree (in the worktree's git dir); the rest live in
/// the common dir.
fn per_worktree(name: &str) -> bool {
    !name.starts_with("refs/")
        || ["refs/bisect/", "refs/worktree/", "refs/rewritten/"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// A ref name safe to join onto a directory.
fn valid_name(name: &str) -> bool {
    (name == "HEAD" || name.starts_with("refs/"))
        && !name
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
        && !name.contains('\\')
}

struct Packed {
    /// Tags under `refs/tags/` carry their `^` peeled line when annotated.
    peeled: bool,
    /// `(oid, name, peeled oid)`.
    refs: Vec<(String, String, Option<String>)>,
}

fn parse_packed(body: &str) -> Result<Packed, Unsupported> {
    let mut p = Packed {
        peeled: false,
        refs: Vec::new(),
    };
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("# pack-refs with:") {
            p.peeled = h
                .split_whitespace()
                .any(|t| t == "peeled" || t == "fully-peeled");
        } else if let Some(oid) = line.strip_prefix('^') {
            let last = p.refs.last_mut().ok_or(Unsupported)?;
            if !is_oid(oid) {
                return Err(Unsupported);
            }
            last.2 = Some(oid.to_string());
        } else if !line.is_empty() && !line.starts_with('#') {
            let (oid, name) = line.split_once(' ').ok_or(Unsupported)?;
            if !is_oid(oid) {
                return Err(Unsupported);
            }
            p.refs.push((oid.to_string(), name.to_string(), None));
        }
    }
    Ok(p)
}

/// Tags pointing at one commit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TagScan {
    /// `(name, annotated)`.
    pub at: Vec<(String, bool)>,
    /// Some loose tag could be an annotated tag of this commit: telling
    /// needs the object database, so `at` may be incomplete.
    pub unknown: bool,
}

/// The tag to show: annotated before lightweight, then the highest
/// version (digit runs compared as numbers).
pub fn best_tag(tags: &[(String, bool)]) -> Option<&str> {
    tags.iter()
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| natural(&a.0, &b.0))
                .then_with(|| a.0.cmp(&b.0))
        })
        .map(|t| t.0.as_str())
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
                let (na, nb) = (run(a), run(b));
                let strip = |s: &[u8]| {
                    let z = s.iter().take_while(|c| **c == b'0').count();
                    s[z..].to_vec()
                };
                let (da, db) = (strip(&a[..na]), strip(&b[..nb]));
                let o = da.len().cmp(&db.len()).then_with(|| da.cmp(&db));
                if o.is_ne() {
                    return o;
                }
                (a, b) = (&a[na..], &b[nb..]);
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                (a, b) = (&a[1..], &b[1..]);
            }
        }
    }
}

impl Repo {
    fn packed(&self) -> Result<Option<Packed>, Unsupported> {
        read(&self.common_dir.join("packed-refs"))?
            .map(|b| parse_packed(&b))
            .transpose()
    }

    /// Object id `name` points to; `None` when the ref does not exist.
    pub fn resolve(&self, name: &str) -> Result<Option<String>, Unsupported> {
        let mut name = name.to_string();
        for _ in 0..MAX_DEPTH {
            // reftable repositories keep this placeholder in HEAD
            if !valid_name(&name) || name == "refs/heads/.invalid" {
                return Err(Unsupported);
            }
            let base = if per_worktree(&name) {
                &self.git_dir
            } else {
                &self.common_dir
            };
            let Some(body) = read(&base.join(&name))? else {
                if per_worktree(&name) {
                    return Ok(None);
                }
                return Ok(self
                    .packed()?
                    .and_then(|p| p.refs.into_iter().find(|r| r.1 == name))
                    .map(|r| r.0));
            };
            let body = body.trim();
            match body.strip_prefix("ref:") {
                Some(target) => name = target.trim().to_string(),
                None if is_oid(body) => return Ok(Some(body.to_ascii_lowercase())),
                None => return Err(Unsupported),
            }
        }
        Err(Unsupported)
    }

    /// The commit HEAD points to; `None` on an unborn branch.
    pub fn head_commit(&self) -> Result<Option<String>, Unsupported> {
        self.resolve("HEAD")
    }

    /// Tags whose target, peeled, is `commit`.
    pub fn tags_at(&self, commit: &str) -> Result<TagScan, Unsupported> {
        let mut scan = TagScan::default();
        let mut loose = Vec::new();
        walk(&self.common_dir.join("refs/tags"), "", &mut loose)?;
        for (name, body) in &loose {
            let body = body.trim();
            if body == commit {
                scan.at.push((name.clone(), false));
            } else if is_oid(body) || body.starts_with("ref:") {
                scan.unknown = true;
            } else {
                return Err(Unsupported);
            }
        }
        if let Some(p) = self.packed()? {
            for (oid, name, peeled) in p.refs {
                let Some(short) = name.strip_prefix("refs/tags/") else {
                    continue;
                };
                if loose.iter().any(|(n, _)| n == short) {
                    continue;
                }
                match peeled {
                    Some(c) if c == commit => scan.at.push((short.to_string(), true)),
                    Some(_) => {}
                    None if oid == commit => scan.at.push((short.to_string(), false)),
                    None if !p.peeled => scan.unknown = true,
                    None => {}
                }
            }
        }
        Ok(scan)
    }

    /// Entries in the stash, as `git stash list` counts them.
    pub fn stash_count(&self) -> Result<usize, Unsupported> {
        if self.resolve("refs/stash")?.is_none() {
            return Ok(0);
        }
        Ok(match read(&self.common_dir.join("logs/refs/stash"))? {
            Some(log) => log.lines().filter(|l| !l.trim().is_empty()).count(),
            None => 1,
        })
    }
}

/// Loose refs under `dir`, as `(name relative to refs/tags, contents)`.
fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) -> Result<(), Unsupported> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Unsupported),
    };
    for e in entries {
        let e = e.map_err(|_| Unsupported)?;
        let name = e.file_name().into_string().map_err(|_| Unsupported)?;
        let full = format!("{prefix}{name}");
        let ty = e.file_type().map_err(|_| Unsupported)?;
        if ty.is_dir() {
            walk(&e.path(), &format!("{full}/"), out)?;
        } else if name.ends_with(".lock") {
            // a ref being written; git skips these too
        } else {
            if out.len() >= MAX_LOOSE_TAGS {
                return Err(Unsupported);
            }
            out.push((full, read(&e.path())?.ok_or(Unsupported)?));
        }
    }
    Ok(())
}
