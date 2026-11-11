//! `diff`: lines added and removed in the working tree, from a cached
//! background `git diff --numstat`; render never runs git. The value is
//! kept with a stamp of HEAD and the index, so a commit, `git add` or
//! checkout makes it stale at once; edits to files refresh after `ttl`.

use super::{gitst, work_dir, Ctx, Placeholder};
use crate::cache::{run_child_capped, Job};
use crate::gitfs::{self, Found, Repo};
use crate::options::Options;
use crate::style::fg;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// Values of `scope`: against HEAD, index against HEAD, worktree against
/// the index.
pub const SCOPES: &[&str] = &["both", "staged", "unstaged"];
/// Values accepted by `show`, in display order.
pub const SHOW: &[&str] = &["added", "removed"];
/// `git diff --numstat` output read per run; past it the totals are
/// lower bounds.
pub const MAX_NUMSTAT: usize = 8 * 1024 * 1024;

pub struct Diff;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub added: u64,
    pub removed: u64,
    /// Changed files git counts no lines for.
    pub binary: u64,
    /// Output hit `MAX_NUMSTAT`: the totals are lower bounds.
    pub cut: bool,
}

pub fn cache_key(top: &Path, scope: &str) -> String {
    format!("gitdiff:{}:{scope}", top.display())
}

/// What the value depends on besides file contents: HEAD's commit (`-`
/// unborn, `?` unknown to the files) and the index's file id, mtime and
/// size (`?` without one).
pub fn stamp(repo: &Repo) -> String {
    let head = match repo.head_commit() {
        Ok(Some(c)) => c.to_ascii_lowercase(),
        Ok(None) => "-".into(),
        Err(_) => "?".into(),
    };
    let index = repo.index_stamp().unwrap_or_else(|| "?".into());
    format!("{head}|{index}")
}

/// A stamp as [`stamp`] makes it: short, printable, no blanks.
pub fn valid_stamp(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// Totals of `git diff --numstat` output; binary files (`-\t-`) count
/// in `binary`, malformed lines are skipped.
pub fn parse_numstat(out: &str) -> Totals {
    let mut t = Totals::default();
    for line in out.lines() {
        let mut f = line.splitn(3, '\t');
        match (f.next(), f.next(), f.next()) {
            (Some("-"), Some("-"), Some(_)) => t.binary += 1,
            (Some(a), Some(r), Some(_)) => {
                if let (Ok(a), Ok(r)) = (a.parse::<u64>(), r.parse::<u64>()) {
                    t.added = t.added.saturating_add(a);
                    t.removed = t.removed.saturating_add(r);
                }
            }
            _ => {}
        }
    }
    t
}

/// The job's stored form: the stamp, then the totals.
pub fn encode(stamp: &str, t: &Totals) -> String {
    let cut = u8::from(t.cut);
    format!("{stamp}\n{} {} {} {cut}\n", t.added, t.removed, t.binary)
}

/// Totals of a stored value made under `stamp`.
pub fn decode(raw: &str, stamp: &str) -> Option<Totals> {
    let mut lines = raw.lines();
    if lines.next()? != stamp {
        return None;
    }
    let mut n = lines.next()?.split(' ').map(|n| n.parse::<u64>().ok());
    Some(Totals {
        added: n.next()??,
        removed: n.next()??,
        binary: n.next()??,
        cut: n.next()?? == 1,
    })
}

fn git(dir: &str) -> Command {
    let mut c = Command::new(crate::platform::program("git"));
    c.args(["-c", "core.fsmonitor=false"])
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0");
    c
}

/// One `git <cmd> --numstat` (`diff-index` or `diff-files`): plumbing,
/// which never writes refreshed stat info back to the index (porcelain
/// `git diff` does, even with optional locks off) and ignores diff config
/// (external drivers, textconv, colour). `-M` finds renames as `git diff`
/// does by default.
fn numstat(dir: &str, cmd: &[&str], deadline: Instant) -> Option<Totals> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return None;
    }
    let mut c = git(dir);
    c.args(cmd).args(["--numstat", "-M", "--"]);
    let out = run_child_capped(c, None, left, MAX_NUMSTAT)?;
    let mut t = parse_numstat(&out);
    t.cut = out.len() >= MAX_NUMSTAT;
    Some(t)
}

/// The empty tree's id in this repository's hash.
fn empty_tree(dir: &str, deadline: Instant) -> Option<String> {
    let mut c = git(dir);
    c.args(["hash-object", "-t", "tree", "--stdin"]);
    let left = deadline.saturating_duration_since(Instant::now());
    let id = run_child_capped(c, Some(Vec::new()), left, 128)?;
    let id = id.trim();
    gitfs::is_oid(id).then(|| id.to_string())
}

/// `git diff-index [--cached] <base>`: HEAD, or the empty tree on an
/// unborn branch.
fn against_base(dir: &str, cached: bool, stamp: &str, deadline: Instant) -> Option<Totals> {
    let index = |base: &str| {
        let mut cmd = vec!["diff-index"];
        if cached {
            cmd.push("--cached");
        }
        cmd.push(base);
        numstat(dir, &cmd, deadline)
    };
    let empty = || index(&empty_tree(dir, deadline)?);
    match stamp.split('|').next() {
        Some("-") => empty(),
        // a HEAD the files could not resolve may still be unborn
        Some("?") => index("HEAD").or_else(empty),
        _ => index("HEAD"),
    }
}

/// Body of the `git-diff` job.
pub(crate) fn run(dir: &str, scope: &str, stamp: &str, limit: Duration) -> Option<String> {
    let deadline = Instant::now() + limit;
    let t = match scope {
        "staged" => against_base(dir, true, stamp, deadline)?,
        "unstaged" => numstat(dir, &["diff-files"], deadline)?,
        _ => against_base(dir, false, stamp, deadline)?,
    };
    Some(encode(stamp, &t))
}

impl Placeholder for Diff {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let scope = match opts.get("scope") {
            Some(s) => *SCOPES.iter().find(|v| **v == s.trim())?,
            None => SCOPES[0],
        };
        let show = match opts.get("show") {
            Some(v) => gitst::parse_show(v, SHOW)?,
            None => SHOW.to_vec(),
        };
        let cache = ctx.cache?;
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let stamp = stamp(&repo);
        let top = crate::platform::canonicalize(&repo.top).unwrap_or(repo.top);
        let job = Job::GitDiff {
            dir: top.to_str()?.to_string(),
            scope: scope.to_string(),
            stamp: stamp.clone(),
        };
        let key = cache_key(&top, scope);
        let current = |v: &str| v.lines().next() == Some(stamp.as_str());
        let raw = cache.read_or_refresh_if(&key, gitst::ttl(opts), &job, current)?;
        // a value from before the last commit or `git add` is shown until
        // the refresh lands, as `dirty` shows its last status
        let t = raw.split_once('\n').and_then(|(s, _)| decode(&raw, s))?;
        let more = if t.cut { "+" } else { "" };
        let c = &ctx.config.colors;
        let mut parts = Vec::new();
        if show.contains(&"added") && t.added > 0 {
            parts.push(fg(c.good, &format!("+{}{more}", t.added)));
        }
        if show.contains(&"removed") && t.removed > 0 {
            parts.push(fg(c.bad, &format!("-{}{more}", t.removed)));
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}
