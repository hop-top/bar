//! `age`: time since HEAD's commit, from a cached background `git
//! cat-file commit`; render never runs git. A commit's time never changes,
//! so the value is kept with the commit id and refreshed only when HEAD
//! moves.

use super::{gitst, work_dir, Ctx, Placeholder};
use crate::cache::{run_child_capped, Job};
use crate::color::Color;
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{dim, fg, ramp};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// A stored time is re-read this often even when HEAD stays put.
pub const TTL: u64 = 86_400;
/// Commit object bytes read; the committer line comes before the message.
const MAX_COMMIT: usize = 64 * 1024;

pub struct Age;

pub fn cache_key(top: &Path) -> String {
    format!("gitage:{}", top.display())
}

/// Committer time (Unix seconds) from a raw commit object.
pub fn committer_time(body: &str) -> Option<i64> {
    for line in body.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("committer ") {
            // `Name <email> <seconds> <zone>`; the name may hold anything
            let mut it = rest.rsplit(' ');
            let _zone = it.next()?;
            return it.next()?.parse().ok();
        }
    }
    None
}

/// Body of the `git-commit-time` job.
pub(crate) fn run(dir: &str, commit: &str, limit: Duration) -> Option<String> {
    let mut c = Command::new(crate::platform::program("git"));
    c.args(["cat-file", "commit", commit])
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0");
    let body = run_child_capped(c, None, limit, MAX_COMMIT)?;
    let t = committer_time(&body)?;
    Some(format!("{commit}\n{t}\n"))
}

/// The time of a stored value made for `commit`.
pub fn decode(raw: &str, commit: &str) -> Option<i64> {
    let mut lines = raw.lines();
    if lines.next()? != commit {
        return None;
    }
    lines.next()?.parse().ok()
}

/// `45s`, `12m`, `3h`, `5d`, `4mo`, `2y`: the largest whole unit.
pub fn fmt(secs: i64) -> String {
    let s = secs.max(0);
    let (m, h, d) = (s / 60, s / 3600, s / 86_400);
    match () {
        _ if m == 0 => format!("{s}s"),
        _ if h == 0 => format!("{m}m"),
        _ if d == 0 => format!("{h}h"),
        _ if d < 30 => format!("{d}d"),
        _ if d < 365 => format!("{}mo", d / 30),
        _ => format!("{}y", d / 365),
    }
}

/// Ramp score: 100 (good end) for a new commit, 0 at `hours` old.
pub fn score(secs: i64, hours: u64) -> i32 {
    let full = i64::try_from(hours.saturating_mul(3600)).unwrap_or(i64::MAX);
    if full == 0 {
        return 100;
    }
    let used = secs.clamp(0, full) as i128 * 100 / full as i128;
    (100 - used) as i32
}

/// `ramp=<hours>`: `Some(None)` without it, `None` when it is invalid.
fn ramp_hours(opts: &Options) -> Option<Option<u64>> {
    match opts.get("ramp") {
        Some(r) => Some(Some(r.trim().parse::<u64>().ok().filter(|h| *h >= 1)?)),
        None => Some(None),
    }
}

/// An age of `secs` as `age` draws it: muted, or on `ramp` over `hours`.
fn paint(secs: i64, hours: Option<u64>, ramp_colors: &[Color; 11]) -> String {
    let text = fmt(secs);
    match hours {
        Some(h) => fg(ramp(score(secs, h), ramp_colors), &text),
        None => dim(&text),
    }
}

/// A commit `secs` old, for previews.
pub fn sample(secs: i64, opts: &Options, ramp_colors: &[Color; 11]) -> Option<String> {
    Some(paint(secs, ramp_hours(opts)?, ramp_colors))
}

impl Placeholder for Age {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let hours = ramp_hours(opts)?;
        let cache = ctx.cache?;
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let commit = match repo.head_commit() {
            Ok(c) => c?,
            Err(_) => gitst::status(ctx, opts)?.oid?,
        }
        .to_ascii_lowercase();
        if !gitfs::is_oid(&commit) {
            return None;
        }
        let top = crate::platform::canonicalize(&repo.top).unwrap_or(repo.top);
        let job = Job::GitCommitTime {
            dir: top.to_str()?.to_string(),
            commit: commit.clone(),
        };
        let current = |v: &str| v.lines().next() == Some(commit.as_str());
        let raw = cache.read_or_refresh_if(&cache_key(&top), TTL, &job, current)?;
        // another commit's time would be wrong, not just old
        let secs = ctx.now - decode(&raw, &commit)?;
        Some(paint(secs, hours, &ctx.config.ramp))
    }
}
