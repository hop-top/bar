//! `gitop` (operation in progress) and `conflicts` (unmerged paths), read
//! from the repository files. An index over `gitfs::MAX_INDEX`, or one the
//! reader cannot parse, is counted from the cached `git status` instead.
//! A parsed count is kept with the index's stamp and reused until the
//! index changes.

use super::{gitst, work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found, Repo, Unsupported};
use crate::options::Options;
use crate::style::fg;
use std::path::Path;

pub struct GitOp;
pub struct Conflicts;

impl Placeholder for GitOp {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let (op, step) = repo.operation().ok()??;
        let show_step = !matches!(opts.get("step"), Some("false" | "no" | "off" | "0"));
        let text = match step {
            Some((at, of)) if show_step => format!("{} {at}/{of}", op.name()),
            _ => op.name().to_string(),
        };
        Some(fg(ctx.config.colors.warn, &text))
    }
}

pub fn conflicts_key(top: &Path) -> String {
    format!("conflicts:{}", top.display())
}

/// Unmerged paths, reusing the count stored for an unchanged index
/// (same file, mtime and size) instead of parsing it again.
fn conflicts(ctx: &Ctx, repo: &Repo) -> Result<usize, Unsupported> {
    let (Some(cache), Some(stamp)) = (ctx.cache, repo.index_stamp()) else {
        return repo.conflicts();
    };
    let key = conflicts_key(&repo.top);
    let stored = cache.read_fresh(&key, u64::MAX);
    if let Some((s, n)) = stored.as_deref().and_then(|v| v.rsplit_once(' ')) {
        if let (true, Ok(n)) = (s == stamp, n.parse()) {
            return Ok(n);
        }
    }
    let n = repo.conflicts()?;
    let _ = cache.store(&key, &format!("{stamp} {n}"));
    Ok(n)
}

impl Placeholder for Conflicts {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let n = match conflicts(ctx, &repo) {
            Ok(n) => n,
            Err(_) => gitst::status(ctx, opts)?.unmerged,
        };
        (n > 0).then(|| fg(ctx.config.colors.bad, &format!("✗{n}")))
    }
}
