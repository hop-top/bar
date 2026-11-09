//! `gitop` (operation in progress) and `conflicts` (unmerged paths), read
//! from the repository files. An index over `gitfs::MAX_INDEX`, or one the
//! reader cannot parse, is counted from the cached `git status` instead.

use super::{gitst, work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::fg;

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

impl Placeholder for Conflicts {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let Found::Repo(repo) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        let n = match repo.conflicts() {
            Ok(n) => n,
            Err(_) => gitst::status(ctx, opts)?.unmerged,
        };
        (n > 0).then(|| fg(ctx.config.colors.bad, &format!("✗{n}")))
    }
}
