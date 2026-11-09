//! Minimal read-only view of a git repository: branch, refs, remote URLs,
//! upstream, in-progress operation and unmerged index entries from plain files, no
//! process spawn. Anything it cannot read exactly the way git would
//! reports `Unsupported`; callers then ask git itself.

mod config;
mod index;
mod ops;
mod refs;

pub use index::{unmerged, MAX_INDEX};
pub use ops::{Op, Step};
pub use refs::{best_tag, is_oid, TagScan};

/// A remote name safe to pass to `git remote get-url`.
pub fn valid_remote(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

use std::cell::OnceCell;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Larger files are not read; git itself is asked instead.
const MAX_FILE: u64 = 1024 * 1024;

/// The files cannot be interpreted faithfully without git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported;

#[derive(Debug)]
pub enum Found {
    Repo(Repo),
    NotRepo,
    Unsupported,
}

#[derive(Debug)]
pub struct Repo {
    /// Directory holding the `.git` entry.
    pub top: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    /// Read on first use, then shared by every ref lookup.
    packed: OnceCell<Result<Option<refs::Packed>, Unsupported>>,
    reftable: OnceCell<Result<bool, Unsupported>>,
}

/// `Ok(None)` when the file does not exist (or is `/dev/null`, git's own
/// "no config" idiom). Anything but a small regular UTF-8 file is
/// `Unsupported`, so a FIFO or device never blocks the reader.
fn read(p: &Path) -> Result<Option<String>, Unsupported> {
    match read_bytes(p, MAX_FILE)? {
        Some(b) => String::from_utf8(b).map(Some).map_err(|_| Unsupported),
        None => Ok(None),
    }
}

/// As `read`, raw bytes, capped at `max`.
fn read_bytes(p: &Path, max: u64) -> Result<Option<Vec<u8>>, Unsupported> {
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Unsupported),
    };
    if !meta.is_file() {
        return if p == Path::new("/dev/null") {
            Ok(None)
        } else {
            Err(Unsupported)
        };
    }
    if meta.len() > max {
        return Err(Unsupported);
    }
    let mut body = Vec::with_capacity(meta.len() as usize);
    let file = fs::File::open(p).map_err(|_| Unsupported)?;
    file.take(max + 1)
        .read_to_end(&mut body)
        .map_err(|_| Unsupported)?;
    if body.len() as u64 > max {
        return Err(Unsupported);
    }
    Ok(Some(body))
}

fn looks_bare(d: &Path) -> bool {
    d.join("HEAD").is_file() && d.join("refs").is_dir() && d.join("objects").is_dir()
}

/// Find the repository containing `dir`, the way git walks upward.
pub fn discover(dir: &Path) -> Found {
    if [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some())
    {
        return Found::Unsupported;
    }
    for d in dir.ancestors() {
        let dot = d.join(".git");
        if dot.is_dir() {
            return open(d, dot);
        }
        if dot.is_file() {
            let Ok(Some(body)) = read(&dot) else {
                return Found::Unsupported;
            };
            let Some(target) = body.lines().next().and_then(|l| l.strip_prefix("gitdir:")) else {
                return Found::Unsupported;
            };
            return open(d, d.join(target.trim()));
        }
        if looks_bare(d) {
            return Found::Unsupported;
        }
    }
    Found::NotRepo
}

fn open(top: &Path, git_dir: PathBuf) -> Found {
    if !git_dir.join("HEAD").is_file() {
        return Found::Unsupported;
    }
    let common_dir = match read(&git_dir.join("commondir")) {
        Ok(Some(c)) => git_dir.join(c.trim()),
        Ok(None) => git_dir.clone(),
        Err(Unsupported) => return Found::Unsupported,
    };
    Found::Repo(Repo {
        top: top.to_path_buf(),
        git_dir,
        common_dir,
        packed: OnceCell::new(),
        reftable: OnceCell::new(),
    })
}

impl Repo {
    /// A linked worktree: its git dir is not the common dir (a submodule's
    /// `.git` file also points elsewhere, but has no `commondir`).
    pub fn linked(&self) -> bool {
        self.git_dir != self.common_dir
    }

    /// Current branch; `None` when HEAD is detached.
    pub fn branch(&self) -> Result<Option<String>, Unsupported> {
        let head = read(&self.git_dir.join("HEAD"))?.ok_or(Unsupported)?;
        let head = head.trim();
        match head.strip_prefix("ref:") {
            Some(r) => match r.trim().strip_prefix("refs/heads/") {
                // reftable repositories keep this placeholder in HEAD
                Some(".invalid") | None => Err(Unsupported),
                Some(b) => Ok(Some(b.to_string())),
            },
            None if head.len() >= 40 && head.chars().all(|c| c.is_ascii_hexdigit()) => Ok(None),
            None => Err(Unsupported),
        }
    }

    /// `remote.origin.url` as git would report it.
    pub fn origin_url(&self) -> Result<Option<String>, Unsupported> {
        self.remote_url("origin")
    }

    /// First `url` of remote `name`, as `git remote get-url` prints it;
    /// `Unsupported` when URL rewrites could change it.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>, Unsupported> {
        self.with_config(true, |c| {
            if c.rewrites {
                return Err(Unsupported);
            }
            c.first("remote", name, "url")
        })
    }

    /// The current branch's upstream, as `git status` names it; `None`
    /// when HEAD is detached or the branch tracks nothing.
    pub fn upstream(&self) -> Result<Option<Upstream>, Unsupported> {
        let Some(branch) = self.branch()? else {
            return Ok(None);
        };
        let tracked = self.with_config(false, |c| {
            let remote = c.last("branch", &branch, "remote")?;
            let merge = c.first("branch", &branch, "merge")?;
            let (Some(remote), Some(merge)) = (remote, merge) else {
                return Ok(None);
            };
            if remote == "." {
                return Ok(Some((remote, merge.clone(), merge)));
            }
            let fetch = c.all("remote", &remote, "fetch")?;
            Ok(config::track(&fetch, &merge)?.map(|dst| (remote, merge, dst)))
        })?;
        let Some((remote, merge, dst)) = tracked else {
            return Ok(None);
        };
        let name = self.shorten(&dst)?;
        let branch = merge
            .strip_prefix("refs/heads/")
            .unwrap_or(&merge)
            .to_string();
        Ok(Some(Upstream {
            name,
            remote,
            branch,
        }))
    }

    /// `full` as `git status` prints it (`shorten_unambiguous_ref`): the
    /// prefix dropped unless the short name would also name another ref,
    /// which is left to git.
    fn shorten(&self, full: &str) -> Result<String, Unsupported> {
        let (short, earlier): (&str, &[&str]) = if let Some(s) = full.strip_prefix("refs/heads/") {
            (s, &["refs/", "refs/tags/"])
        } else if let Some(s) = full.strip_prefix("refs/remotes/") {
            (s, &["refs/", "refs/tags/", "refs/heads/"])
        } else if let Some(s) = full.strip_prefix("refs/tags/") {
            (s, &["refs/"])
        } else {
            return Err(Unsupported);
        };
        if short.is_empty() || self.git_dir.join(short).exists() {
            return Err(Unsupported);
        }
        for p in earlier {
            if self.resolve(&format!("{p}{short}"))?.is_some() {
                return Err(Unsupported);
            }
        }
        Ok(short.to_string())
    }

    /// `f` over the repository config, after the global configs are checked
    /// for anything that would change it (`urls`: URL rewrites too).
    fn with_config<T>(
        &self,
        urls: bool,
        f: impl FnOnce(&config::Config) -> Result<Option<T>, Unsupported>,
    ) -> Result<Option<T>, Unsupported> {
        for g in global_configs() {
            if read(&g)?.is_some_and(|body| global_overrides(&body, urls)) {
                return Err(Unsupported);
            }
        }
        match read(&self.common_dir.join("config"))? {
            Some(body) => f(&config::parse(&body)?),
            None => Ok(None),
        }
    }
}

/// A branch's upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// As `git status` shows it: `origin/main`, or `main` for a local
    /// branch (remote `.`).
    pub name: String,
    /// `branch.<name>.remote`: a remote name, or `.`.
    pub remote: String,
    /// The branch on the remote, `refs/heads/` dropped.
    pub branch: String,
}

fn global_configs() -> Vec<PathBuf> {
    let env = |k| std::env::var_os(k).filter(|v| !v.is_empty());
    let mut v = system_configs();
    if let Some(g) = env("GIT_CONFIG_GLOBAL") {
        v.push(g.into());
        return v;
    }
    // git for Windows falls back to the profile dir without `HOME`
    let home = env("HOME")
        .or_else(|| env("USERPROFILE").filter(|_| cfg!(windows)))
        .map(PathBuf::from);
    let xdg = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".config")));
    v.extend(home.map(|h| h.join(".gitconfig")));
    v.extend(xdg.map(|x| x.join("git/config")));
    v
}

/// System-level config files git may read.
#[cfg(not(windows))]
fn system_configs() -> Vec<PathBuf> {
    vec![PathBuf::from("/etc/gitconfig")]
}

/// Git for Windows: `%PROGRAMDATA%\Git\config`, then `etc\gitconfig`
/// (and the older `mingw64\etc\gitconfig`) of each install whose
/// `git.exe` is on `PATH`.
#[cfg(windows)]
fn system_configs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PROGRAMDATA")
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(p).join("Git").join("config"))
        .into_iter()
        .collect();
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path).filter(|d| d.join("git.exe").is_file()) {
        if let Some(root) = dir.parent() {
            for c in [
                root.join("etc/gitconfig"),
                root.join("mingw64/etc/gitconfig"),
            ] {
                if !v.contains(&c) {
                    v.push(c);
                }
            }
        }
    }
    v
}

/// A global config that changes what the repository config alone says:
/// includes and `remote` entries, plus URL rewrites when reading a URL
/// (`urls`), else `branch` subsection entries. Parsed as the repository
/// config is (continued lines joined), so every header form counts; a
/// body the parser does not model counts too.
fn global_overrides(body: &str, urls: bool) -> bool {
    let body = config::join_continued(body);
    let Ok(cfg) = config::parse(&body) else {
        return true;
    };
    cfg.has_entries("remote", false)
        || if urls {
            cfg.rewrites
        } else {
            cfg.has_entries("branch", true)
        }
}

#[cfg(test)]
mod tests {
    use super::global_overrides;

    #[test]
    fn global_remote_and_branch_entries_in_any_header_form() {
        for body in [
            "[remote \"o\"]\n\turl = x\n",
            "[remote\t\"o\"]\n\turl = x\n",
            "[Remote \"o\"]\n\tpushurl = x\n",
            "[remote.origin]\n\turl = x\n",
            "[include]\n\tpath = x\n",
        ] {
            assert!(global_overrides(body, true), "urls: {body:?}");
            assert!(global_overrides(body, false), "branch: {body:?}");
        }
        for body in [
            "[branch \"main\"]\n\tremote = o\n",
            "[branch\t\"main\"]\n\tmerge = refs/heads/x\n",
            "[branch.main]\n\tremote = o\n",
        ] {
            assert!(global_overrides(body, false), "{body:?}");
        }
        assert!(global_overrides(
            "[url \"https://m/\"]\n\tinsteadOf = gh:\n",
            true
        ));
        assert!(!global_overrides(
            "[url \"https://m/\"]\n\tinsteadOf = gh:\n",
            false
        ));
        assert!(!global_overrides("[branch \"main\"]\n\tremote = o\n", true));
        let alias = "[alias]\n\tlg = log \\\n\t  --oneline\n";
        assert!(!global_overrides(alias, true), "a continued alias");
        assert!(global_overrides(
            &format!("{alias}[remote \"o\"]\n\turl = x\n"),
            true
        ));
        assert!(!global_overrides(
            "[user]\n\tname = me\n[core]\n\teditor = vi\n[branch]\n\tautosetupmerge = true\n",
            false
        ));
    }
}
