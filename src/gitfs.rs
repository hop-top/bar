//! Minimal read-only view of a git repository: branch and origin URL from
//! plain files, no process spawn. Anything it cannot read exactly the way
//! git would reports `Unsupported`; callers then ask git itself.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Larger files are not read; git itself is asked instead.
const MAX_FILE: u64 = 1024 * 1024;

/// The files cannot be interpreted faithfully without git.
#[derive(Debug, PartialEq, Eq)]
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
}

/// `Ok(None)` when the file does not exist (or is `/dev/null`, git's own
/// "no config" idiom). Anything but a small regular UTF-8 file is
/// `Unsupported`, so a FIFO or device never blocks the reader.
fn read(p: &Path) -> Result<Option<String>, Unsupported> {
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
    let mut body = String::new();
    let file = fs::File::open(p).map_err(|_| Unsupported)?;
    file.take(MAX_FILE + 1)
        .read_to_string(&mut body)
        .map_err(|_| Unsupported)?;
    if body.len() as u64 > MAX_FILE {
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
    })
}

impl Repo {
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
        for g in global_configs() {
            if read(&g)?.is_some_and(|body| rewrites_or_includes(&body)) {
                return Err(Unsupported);
            }
        }
        match read(&self.common_dir.join("config"))? {
            Some(body) => origin_from_config(&body),
            None => Ok(None),
        }
    }
}

fn global_configs() -> Vec<PathBuf> {
    let env = |k| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(g) = env("GIT_CONFIG_GLOBAL") {
        return vec![g.into(), "/etc/gitconfig".into()];
    }
    let home = env("HOME").map(PathBuf::from);
    let xdg = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".config")));
    let mut v = vec![PathBuf::from("/etc/gitconfig")];
    v.extend(home.map(|h| h.join(".gitconfig")));
    v.extend(xdg.map(|x| x.join("git/config")));
    v
}

/// Sections whose presence can change what `remote get-url` prints.
fn rewrites_or_includes(body: &str) -> bool {
    body.lines().any(|l| {
        let l = l.trim_start().to_ascii_lowercase();
        l.starts_with("[url") || l.starts_with("[include")
    })
}

/// First `url` of `[remote "origin"]`. Constructs this parser does not model
/// (includes, rewrites, old-style headers, continuations) are `Unsupported`.
fn origin_from_config(body: &str) -> Result<Option<String>, Unsupported> {
    let mut in_origin = false;
    for raw in body.lines() {
        let mut line = raw.trim();
        if raw.ends_with('\\') {
            return Err(Unsupported);
        }
        while let Some(rest) = line.strip_prefix('[') {
            let (head, tail) = rest.split_once(']').ok_or(Unsupported)?;
            let lower = head.to_ascii_lowercase();
            if lower.starts_with("include")
                || lower.starts_with("url")
                || lower.starts_with("remote.")
                || lower.starts_with("extensions")
            {
                return Err(Unsupported);
            }
            in_origin = head
                .split_once(char::is_whitespace)
                .is_some_and(|(s, sub)| {
                    s.eq_ignore_ascii_case("remote") && sub.trim() == "\"origin\""
                });
            line = tail.trim();
        }
        if !in_origin || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("url") {
            return value(val).map(Some).ok_or(Unsupported);
        }
    }
    Ok(None)
}

/// Config value: trimmed, quotes removed, `#`/`;` outside quotes start a
/// comment, `\\ \" \n \t \b` escapes.
fn value(v: &str) -> Option<String> {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = v.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => out.push(match chars.next()? {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{8}',
                c @ ('\\' | '"') => c,
                _ => return None,
            }),
            '#' | ';' if !quoted => break,
            c => out.push(c),
        }
    }
    (!quoted).then(|| out.trim_end().to_string())
}
