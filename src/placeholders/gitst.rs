//! `dirty`, `ahead` and `worktree`. The first two read the output of
//! `git status --porcelain=v2 --branch`, cached per repository root and
//! refreshed in the background; render never runs git. `worktree` comes
//! from the repository files.

use super::{work_dir, Ctx, Placeholder};
use crate::cache::{Cache, Job, MAX_VALUE};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{dim, fg, sanitize};
use std::path::Path;

/// Status stays fresh for ten seconds unless `ttl` says otherwise.
pub const TTL: u64 = 10;
/// Values accepted by `show`, in display order.
pub const SHOW: &[&str] = &["staged", "modified", "untracked"];

pub struct Dirty;
pub struct Ahead;
pub struct Worktree;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub staged: usize,
    /// Worktree changes and unmerged paths.
    pub modified: usize,
    pub untracked: usize,
    /// Unmerged paths (also counted in `modified`).
    pub unmerged: usize,
    /// `(ahead, behind)`; `None` without an upstream.
    pub ahead_behind: Option<(usize, usize)>,
    /// HEAD's commit id; `None` on an unborn branch.
    pub oid: Option<String>,
    /// Input hit the cache size cap: the counts are lower bounds.
    pub truncated: bool,
}

pub fn cache_key(top: &Path) -> String {
    format!("gitst:{}", top.display())
}

/// Counts from porcelain v2 text. A file both staged and modified counts
/// in both columns. Unrecognised lines are skipped.
pub fn parse(text: &str) -> Status {
    let mut s = Status {
        // the cache cuts on a char boundary, up to 3 bytes short of the cap
        truncated: text.len() + 4 > MAX_VALUE,
        ..Status::default()
    };
    for line in text.lines() {
        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            s.oid = gitfs::is_oid(oid).then(|| oid.to_ascii_lowercase());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let n = |t: Option<&str>, sign| {
                t.and_then(|t| t.strip_prefix(sign))
                    .and_then(|t| t.parse().ok())
            };
            let mut it = ab.split_whitespace();
            if let (Some(a), Some(b)) = (n(it.next(), '+'), n(it.next(), '-')) {
                s.ahead_behind = Some((a, b));
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            let mut xy = line.chars().skip(2);
            let (x, y) = (xy.next(), xy.next());
            s.staged += usize::from(x.is_some_and(|c| c != '.'));
            s.modified += usize::from(y.is_some_and(|c| c != '.'));
        } else if line.starts_with("u ") {
            s.modified += 1;
            s.unmerged += 1;
        } else if line.starts_with("? ") {
            s.untracked += 1;
        }
    }
    s
}

/// The `show` option as a subset of `set`; `None` when a part is unknown.
pub fn parse_show(raw: &str, set: &[&'static str]) -> Option<Vec<&'static str>> {
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        out.push(*set.iter().find(|s| **s == part)?);
    }
    Some(out)
}

pub(crate) fn ttl(opts: &Options) -> u64 {
    opts.get("ttl")
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map_or(TTL, |t| t.max(1))
}

/// Cached status of the session's repository; stale values are served
/// while a background refresh runs.
pub(crate) fn status(ctx: &Ctx, opts: &Options) -> Option<Status> {
    let cache: &Cache = ctx.cache?;
    let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
        return None;
    };
    let top = repo.top.canonicalize().unwrap_or(repo.top);
    let job = Job::GitStatus {
        dir: top.to_str()?.to_string(),
    };
    let raw = cache.read_or_refresh(&cache_key(&top), ttl(opts), &job)?;
    Some(parse(&raw))
}

impl Placeholder for Dirty {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let show = match opts.get("show") {
            Some(v) => parse_show(v, SHOW)?,
            None => SHOW.to_vec(),
        };
        let s = status(ctx, opts)?;
        let c = &ctx.config.colors;
        let more = if s.truncated { "+" } else { "" };
        let mut parts = Vec::new();
        for kind in SHOW.iter().filter(|k| show.contains(k)) {
            match *kind {
                "staged" if s.staged > 0 => parts.push(fg(c.good, &format!("+{}{more}", s.staged))),
                "modified" if s.modified > 0 => {
                    parts.push(fg(c.warn, &format!("~{}{more}", s.modified)))
                }
                "untracked" if s.untracked > 0 => {
                    parts.push(dim(&format!("?{}{more}", s.untracked)))
                }
                _ => {}
            }
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

impl Placeholder for Ahead {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let (a, b) = status(ctx, opts)?.ahead_behind?;
        let c = &ctx.config.colors;
        let mut parts = Vec::new();
        if a > 0 {
            parts.push(fg(c.good, &format!("↑{a}")));
        }
        if b > 0 {
            parts.push(fg(c.warn, &format!("↓{b}")));
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

impl Placeholder for Worktree {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        if !repo.linked() {
            return None;
        }
        let name = sanitize(&repo.top.file_name()?.to_string_lossy());
        (!name.is_empty()).then(|| dim(&name))
    }
}
