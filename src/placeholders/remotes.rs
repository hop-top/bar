//! `origin`, `upstream` and `fork`, read from the repository config and
//! refs. Render never spawns git: what the files cannot tell comes from
//! the cached background `git remote get-url` and `git status`.

use super::gitref::url_part;
use super::repo::{cached_remote_raw, web_url};
use super::{gitst, link_enabled, truncate, work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{accent, dim, fg, link, sanitize};

pub struct Origin;
pub struct Upstream;
pub struct Fork;

/// Values accepted by `show` of `origin` and `fork`, in display order.
pub const SHOW: &[&str] = &["host", "owner", "repo"];
const DEFAULT_SHOW: &[&str] = &["owner", "repo"];

/// `(host, owner, repo)` of a remote URL: from its web address, else
/// (a local path) the last two path components, no host.
pub fn parts(raw: &str) -> Option<(String, String, String)> {
    let (host, path) = match web_url(raw) {
        Some(u) => {
            let rest = u.strip_prefix("https://")?;
            let (h, p) = rest.split_once('/')?;
            (h.to_string(), p.to_string())
        }
        None => {
            let p = raw.trim().trim_end_matches(['/', '\\']);
            let p = p.strip_suffix(".git").unwrap_or(p);
            (String::new(), p.replace('\\', "/"))
        }
    };
    let path = path.trim_matches('/');
    let (owner, repo) = match path.rsplit_once('/') {
        Some((o, r)) if web_url(raw).is_some() => (o.to_string(), r.to_string()),
        Some((o, r)) => {
            let o = o.rsplit('/').next().unwrap_or(o);
            let o = if matches!(o, "." | "..") { "" } else { o };
            (o.to_string(), r.to_string())
        }
        None => (String::new(), path.to_string()),
    };
    (!repo.is_empty()).then_some((host, owner, repo))
}

/// The `show` parts of `raw`, joined with `/`; `None` when empty.
fn shown(raw: &str, opts: &Options) -> Option<String> {
    let show = match opts.get("show") {
        Some(v) => gitst::parse_show(v, SHOW)?,
        None => DEFAULT_SHOW.to_vec(),
    };
    let (host, owner, repo) = parts(raw)?;
    let text: Vec<&str> = SHOW
        .iter()
        .filter(|s| show.contains(s))
        .map(|s| match *s {
            "host" => host.as_str(),
            "owner" => owner.as_str(),
            _ => repo.as_str(),
        })
        .filter(|p| !p.is_empty())
        .collect();
    let text = sanitize(&text.join("/"));
    (!text.is_empty()).then_some(text)
}

/// A remote drawn as its `show` parts in `color`, linked to its web page.
fn draw(raw: &str, opts: &Options, color: u8) -> Option<String> {
    let text = fg(color, &truncate(shown(raw, opts)?, opts));
    Some(match web_url(raw) {
        Some(u) if link_enabled(opts) => link(&sanitize(&u), &text),
        _ => text,
    })
}

/// Two remote URLs naming the same repository: same web address (host
/// case ignored), else the same URL.
fn same_repo(a: &str, b: &str) -> bool {
    match (web_url(a), web_url(b)) {
        (Some(x), Some(y)) => {
            let split = |u: &str| {
                let (h, p) = u.strip_prefix("https://")?.split_once('/')?;
                Some((h.to_ascii_lowercase(), p.to_string()))
            };
            split(&x) == split(&y)
        }
        _ => a.trim() == b.trim(),
    }
}

/// A remote as `origin` and `fork` draw it, unlinked, for previews.
pub fn sample(raw: &str, opts: &Options) -> Option<String> {
    let mut opts = opts.clone();
    opts.set("link", "off");
    draw(raw, &opts, accent())
}

impl Placeholder for Origin {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        draw(&cached_remote_raw(ctx, "origin")?, opts, accent())
    }
}

impl Placeholder for Fork {
    /// The `upstream` remote, when it names another repository than
    /// `origin`.
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let parent = cached_remote_raw(ctx, "upstream")?;
        let origin = cached_remote_raw(ctx, "origin")?;
        if same_repo(&origin, &parent) {
            return None;
        }
        draw(&parent, opts, accent())
    }
}

impl Placeholder for Upstream {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let Found::Repo(repo) = gitfs::discover(&dir) else {
            return None;
        };
        let (name, url) = match repo.upstream() {
            Ok(u) => {
                let u = u?;
                let url = (u.remote != ".")
                    .then(|| cached_remote_raw(ctx, &u.remote))
                    .flatten()
                    .and_then(|r| web_url(&r))
                    .map(|w| format!("{}/tree/{}", sanitize(&w), url_part(&sanitize(&u.branch))));
                (u.name, url)
            }
            // config or refs the files cannot read: git's own name, no link
            Err(_) => (gitst::status(ctx, opts)?.upstream?, None),
        };
        let name = sanitize(&name);
        if name.is_empty() {
            return None;
        }
        let text = dim(&truncate(name, opts));
        Some(match url {
            Some(u) if link_enabled(opts) => link(&u, &text),
            _ => text,
        })
    }
}
