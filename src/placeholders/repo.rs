use super::abbrev::Abbrev;
use super::{link_enabled, truncate, work_dir, Ctx, Placeholder};
use crate::cache::Job;
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{accent, bold, dim, fg, link, sanitize};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Repo;
pub struct Branch;

/// `git` in `dir`, unless `spawn` is false (a `bar subagent` row, which
/// never waits on a process): then nothing, as for an unknown value.
pub(super) fn git_if(spawn: bool, dir: &Path, args: &[&str]) -> Option<String> {
    spawn.then(|| git(dir, args))?
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut c = Command::new(crate::platform::program("git"));
    let out = crate::platform::hide_window(&mut c)
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string()
    })
}

/// origin URL (ssh, scp or https form) as `https://host/owner/repo`.
/// Spawns `git` when the files cannot tell; render paths that must not
/// use `cached_remote` instead.
pub fn remote_url(cwd: &Path) -> Option<String> {
    remote_url_with(cwd, true)
}

/// [`remote_url`]; `spawn` false never runs `git` (a repository the file
/// reader does not support has no URL then).
pub fn remote_url_with(cwd: &Path, spawn: bool) -> Option<String> {
    let get = || git_if(spawn, cwd, &["remote", "get-url", "origin"]);
    let raw = match gitfs::discover(cwd) {
        Found::NotRepo => return None,
        Found::Repo(r) => match r.origin_url() {
            Ok(u) => u?,
            Err(_) => get()?,
        },
        Found::Unsupported => get()?,
    };
    web_url(&raw)
}

/// Remote URL stays fresh for a minute.
pub const REMOTE_TTL: u64 = 60;

pub fn remote_key(top: &Path) -> String {
    format!("gitremote:{}", top.display())
}

/// Cache key of the URL of remote `name`; `origin` is [`remote_key`].
pub fn remote_key_named(top: &Path, name: &str) -> String {
    match name {
        "origin" => remote_key(top),
        _ => format!("gitremote:{name}:{}", top.display()),
    }
}

/// As `remote_url`, never spawning: when the files cannot tell, the
/// cached result of a background `git remote get-url origin`, nothing
/// until it is in. Computed once per render.
pub(crate) fn cached_remote(ctx: &Ctx) -> Option<String> {
    ctx.memo
        .remote
        .get_or_init(|| web_url(&cached_remote_raw(ctx, "origin")?))
        .clone()
}

/// URL of remote `name` as configured (not yet a web address), never
/// spawning: from the files, else the cached background `git remote
/// get-url <name>`. `None` when there is no such remote, or until the
/// background result is in.
pub(crate) fn cached_remote_raw(ctx: &Ctx, name: &str) -> Option<String> {
    if !gitfs::valid_remote(name) {
        return None;
    }
    let dir = work_dir(ctx);
    let top = match gitfs::discover(&dir) {
        Found::NotRepo => return None,
        Found::Repo(r) => match r.remote_url(name) {
            Ok(u) => return u,
            Err(_) => r.top,
        },
        Found::Unsupported => dir,
    };
    let top = crate::platform::canonicalize(&top).unwrap_or(top);
    let job = Job::GitRemote {
        dir: top.to_str()?.to_string(),
        name: name.to_string(),
    };
    let raw = ctx
        .cache?
        .read_or_refresh(&remote_key_named(&top, name), REMOTE_TTL, &job)?;
    let raw = raw.trim();
    (!raw.is_empty()).then(|| raw.to_string())
}

/// A remote URL as the web address of the repository; an https port other
/// than 443 is kept, ssh ports are not.
pub fn web_url(raw: &str) -> Option<String> {
    let u = raw.trim();
    let u = u.strip_suffix(".git").unwrap_or(u);
    let u = u.strip_suffix('/').unwrap_or(u);
    let (host, port, path) = if let Some((scheme, rest)) = u.split_once("://") {
        if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_lowercase() || c == '+') {
            return None;
        }
        let (auth, path) = rest.split_once('/')?;
        let auth = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
        let (host, port) = auth.split_once(':').unwrap_or((auth, ""));
        let keep = scheme == "https"
            && port != "443"
            && !port.is_empty()
            && port.bytes().all(|b| b.is_ascii_digit());
        (host, if keep { port } else { "" }, path)
    } else {
        let (before, path) = u.split_once(':')?;
        if before.contains('/') {
            return None;
        }
        (before.rsplit_once('@').map_or(before, |(_, h)| h), "", path)
    };
    if host.is_empty() || path.is_empty() || host.contains('/') || path.contains('@') {
        return None;
    }
    Some(match port {
        "" => format!("https://{host}/{path}"),
        p => format!("https://{host}:{p}/{path}"),
    })
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Bare-repo hop hub: `hop.json` plus `hops/`; worktrees live under it.
fn hop_root(dir: &Path) -> Option<&Path> {
    dir.ancestors()
        .filter(|d| d.parent().is_some())
        .find(|d| d.join("hop.json").is_file() && d.join("hops").is_dir())
}

/// Work tree root, as `git rev-parse --show-toplevel` reports it.
fn toplevel(dir: &Path, spawn: bool) -> Option<PathBuf> {
    match gitfs::discover(dir) {
        Found::NotRepo => None,
        Found::Repo(r) => Some(crate::platform::canonicalize(&r.top).unwrap_or(r.top)),
        Found::Unsupported => git_if(spawn, dir, &["rev-parse", "--show-toplevel"])
            .filter(|t| !t.is_empty())
            .map(PathBuf::from),
    }
}

/// Current branch name; `None` when detached or not a repository.
fn current_branch(dir: &Path, spawn: bool) -> Option<String> {
    match gitfs::discover(dir) {
        Found::NotRepo => None,
        Found::Repo(r) => match r.branch() {
            Ok(b) => b,
            Err(_) => git_if(spawn, dir, &["branch", "--show-current"]),
        },
        Found::Unsupported => git_if(spawn, dir, &["branch", "--show-current"]),
    }
}

/// Whether a placeholder may run `git` in the foreground: not in a
/// `bar subagent` row.
pub(crate) fn may_spawn(ctx: &Ctx) -> bool {
    ctx.session.task.is_none()
}

/// Root of a Jujutsu workspace not colocated with git (or nested inside
/// an outer git work tree); `repo` then names it and drops the link.
fn jj_only(dir: &Path, spawn: bool) -> Option<PathBuf> {
    // cheap check first: no `.jj` anywhere above, no git lookup
    super::jj::discover(dir)?;
    super::jj::nearer_than(dir, toplevel(dir, spawn).as_deref())
}

/// The directory `repo` names when there is no web address: a hop hub,
/// a Jujutsu-only workspace, the main repository of a linked worktree, the
/// work tree, else the session directory.
fn project_dir(dir: &Path, spawn: bool) -> PathBuf {
    if let Some(root) = hop_root(dir) {
        return root.to_path_buf();
    }
    if let Some(root) = jj_only(dir, spawn) {
        return root;
    }
    if let Some(top) = toplevel(dir, spawn) {
        if top.join(".git").is_file() {
            // linked worktree: nearest ancestor that is a repo (or bare repo)
            let parent = top
                .parent()
                .into_iter()
                .flat_map(Path::ancestors)
                .find(|d| {
                    d.parent().is_some()
                        && (d.join(".git").is_dir()
                            || (d.join("HEAD").is_file() && d.join("refs").is_dir()))
                });
            if let Some(p) = parent {
                return p.to_path_buf();
            }
        }
        return top;
    }
    dir.to_path_buf()
}

impl Placeholder for Repo {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let spawn = may_spawn(ctx);
        let url = match jj_only(&dir, spawn) {
            Some(_) => None,
            None => remote_url_with(&dir, spawn),
        };
        let (name, named) = match &url {
            Some(u) => (u.rsplit('/').next().unwrap_or_default().to_string(), None),
            None => {
                let d = project_dir(&dir, spawn);
                (basename(&d), Some(d))
            }
        };
        let name = truncate(sanitize(&name), opts);
        if name.is_empty() {
            return None;
        }
        let text = bold(&fg(accent(), &name));
        if !link_enabled(opts) {
            return Some(text);
        }
        Some(match (url, named.as_deref().and_then(Path::to_str)) {
            (Some(u), _) => link(&sanitize(&u), &text),
            (None, Some(d)) => super::open::wrap(text, d, opts),
            (None, None) => text,
        })
    }
}

/// A branch name as `branch` shows it: `fish`, then `max`.
pub fn branch_text(branch: &str, opts: &Options) -> String {
    let short = Abbrev {
        depth: 0,
        ..Abbrev::from(opts)
    }
    .apply(branch, |c| c == '/');
    truncate(short, opts)
}

impl Placeholder for Branch {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let spawn = may_spawn(ctx);
        let branch = sanitize(&current_branch(&dir, spawn)?);
        if branch.is_empty() {
            return None;
        }
        let text = dim(&branch_text(&branch, opts));
        Some(match remote_url_with(&dir, spawn) {
            Some(u) if link_enabled(opts) => {
                link(&format!("{}/tree/{branch}", sanitize(&u)), &text)
            }
            _ => text,
        })
    }
}
