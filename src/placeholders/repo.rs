use super::{link_enabled, truncate, work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{accent, bold, dim, fg, link, sanitize};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Repo;
pub struct Branch;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

/// origin URL (ssh, scp or https form) as `https://host/owner/repo`.
pub fn remote_url(cwd: &Path) -> Option<String> {
    let raw = match gitfs::discover(cwd) {
        Found::NotRepo => return None,
        Found::Repo(r) => match r.origin_url() {
            Ok(u) => u?,
            Err(_) => git(cwd, &["remote", "get-url", "origin"])?,
        },
        Found::Unsupported => git(cwd, &["remote", "get-url", "origin"])?,
    };
    let u = raw.trim();
    let u = u.strip_suffix(".git").unwrap_or(u);
    let u = u.strip_suffix('/').unwrap_or(u);
    let (host, path) = if let Some((scheme, rest)) = u.split_once("://") {
        if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_lowercase() || c == '+') {
            return None;
        }
        let (auth, path) = rest.split_once('/')?;
        let auth = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
        (auth.split(':').next()?, path)
    } else {
        let (before, path) = u.split_once(':')?;
        if before.contains('/') {
            return None;
        }
        (before.rsplit_once('@').map_or(before, |(_, h)| h), path)
    };
    (!host.is_empty() && !path.is_empty() && !host.contains('/') && !path.contains('@'))
        .then(|| format!("https://{host}/{path}"))
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
fn toplevel(dir: &Path) -> Option<PathBuf> {
    match gitfs::discover(dir) {
        Found::NotRepo => None,
        Found::Repo(r) => Some(r.top.canonicalize().unwrap_or(r.top)),
        Found::Unsupported => git(dir, &["rev-parse", "--show-toplevel"])
            .filter(|t| !t.is_empty())
            .map(PathBuf::from),
    }
}

/// Current branch name; `None` when detached or not a repository.
fn current_branch(dir: &Path) -> Option<String> {
    match gitfs::discover(dir) {
        Found::NotRepo => None,
        Found::Repo(r) => match r.branch() {
            Ok(b) => b,
            Err(_) => git(dir, &["branch", "--show-current"]),
        },
        Found::Unsupported => git(dir, &["branch", "--show-current"]),
    }
}

fn project_name(dir: &Path) -> String {
    if let Some(root) = hop_root(dir) {
        return basename(root);
    }
    if let Some(top) = toplevel(dir) {
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
                return basename(p);
            }
        }
        return basename(&top);
    }
    basename(dir)
}

impl Placeholder for Repo {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let url = remote_url(&dir);
        let name = match &url {
            Some(u) => u.rsplit('/').next().unwrap_or_default().to_string(),
            None => project_name(&dir),
        };
        let name = truncate(sanitize(&name), opts);
        if name.is_empty() {
            return None;
        }
        let text = bold(&fg(accent(), &name));
        Some(match url {
            Some(u) if link_enabled(opts) => link(&sanitize(&u), &text),
            _ => text,
        })
    }
}

impl Placeholder for Branch {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let branch = sanitize(&current_branch(&dir)?);
        if branch.is_empty() {
            return None;
        }
        let text = dim(&truncate(branch.clone(), opts));
        Some(match remote_url(&dir) {
            Some(u) if link_enabled(opts) => {
                link(&format!("{}/tree/{branch}", sanitize(&u)), &text)
            }
            _ => text,
        })
    }
}
