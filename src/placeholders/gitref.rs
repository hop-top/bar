//! `sha`, `tag` and `stash`, read from the repository files. Render never
//! spawns git for them: `tag` hands what the files cannot tell (a loose
//! annotated tag) to a cached background `git for-each-ref`, and the
//! links use the cached remote URL when the config needs git.

use super::repo::{cached_remote, git_if, may_spawn};
use super::{gitst, link_enabled, truncate, work_dir, Ctx, Placeholder};
use crate::cache::Job;
use crate::gitfs::{self, best_tag, Found};
use crate::options::Options;
use crate::style::{dim, fg, link, sanitize};
use std::path::Path;

/// `sha` shows this many characters unless `len` says otherwise.
pub const SHA_LEN: usize = 7;

pub struct Sha;
pub struct Tag;
pub struct Stash;

/// HEAD's commit id. A repository whose refs the files cannot resolve
/// (a `packed-refs` over the reader's cap, reftable) reads it from the
/// cached `git status`; `git rev-parse` only when the repository itself
/// cannot be opened from the files.
fn head(ctx: &Ctx, dir: &Path) -> Option<String> {
    let id = match gitfs::discover(dir) {
        Found::NotRepo => return None,
        Found::Repo(r) => match r.head_commit() {
            Ok(c) => c?,
            Err(_) => gitst::status(ctx, &Options::default())?.oid?,
        },
        Found::Unsupported => git_if(
            may_spawn(ctx),
            dir,
            &["rev-parse", "--verify", "-q", "HEAD"],
        )?,
    };
    gitfs::is_oid(&id).then(|| id.to_ascii_lowercase())
}

/// `%`, `#` and `?` escaped, so a ref name stays one path in a URL.
pub(super) fn url_part(s: &str) -> String {
    s.replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F")
}

pub fn tags_key(top: &Path) -> String {
    format!("gittags:{}", top.display())
}

/// `(name, annotated)` from a `git-tags` job's output, when it was made for
/// `commit`.
pub fn parse_tags(out: &str, commit: &str) -> Option<Vec<(String, bool)>> {
    let mut lines = out.lines();
    if lines.next()? != commit {
        return None;
    }
    Some(
        lines
            .filter_map(|l| l.split_once(' '))
            .map(|(ty, name)| (name.to_string(), ty == "tag"))
            .collect(),
    )
}

impl Placeholder for Sha {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let id = head(ctx, &dir)?;
        let len = opts
            .get("len")
            .and_then(|l| l.trim().parse::<usize>().ok())
            .unwrap_or(SHA_LEN);
        if len == 0 {
            return None;
        }
        let text = dim(&id[..len.min(id.len())]);
        Some(match cached_remote(ctx) {
            Some(u) if link_enabled(opts) => link(&format!("{}/commit/{id}", sanitize(&u)), &text),
            _ => text,
        })
    }
}

impl Placeholder for Tag {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let Found::Repo(repo) = gitfs::discover(&dir) else {
            return None;
        };
        // refs the files cannot resolve: HEAD from the cached status,
        // tags from the cached git list
        let (commit, scan) = match repo.head_commit() {
            Ok(c) => {
                let c = c?;
                let scan = repo.tags_at(&c);
                (c, scan)
            }
            Err(e) => (gitst::status(ctx, opts)?.oid?, Err(e)),
        };
        let from_files = scan.as_ref().ok().map(|s| s.at.clone()).unwrap_or_default();
        let tags = match scan {
            Ok(s) if !s.unknown => s.at,
            _ => {
                let top = crate::platform::canonicalize(&repo.top).unwrap_or(repo.top.clone());
                let job = Job::GitTags {
                    dir: top.to_str()?.to_string(),
                    commit: commit.clone(),
                };
                ctx.cache
                    .and_then(|c| c.read_or_refresh(&tags_key(&top), gitst::ttl(opts), &job))
                    .and_then(|out| parse_tags(&out, &commit))
                    .unwrap_or(from_files)
            }
        };
        let name = sanitize(best_tag(&tags)?);
        if name.is_empty() {
            return None;
        }
        let text = fg(ctx.config.colors.good, &truncate(name.clone(), opts));
        Some(match cached_remote(ctx) {
            Some(u) if link_enabled(opts) => {
                link(&format!("{}/tree/{}", sanitize(&u), url_part(&name)), &text)
            }
            _ => text,
        })
    }
}

impl Placeholder for Stash {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let n = match repo.stash_count() {
            Ok(n) => n,
            Err(_) => gitst::status(ctx, opts)?.stash?,
        };
        (n > 0).then(|| dim(&format!("${n}")))
    }
}
