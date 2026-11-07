use super::{Ctx, Placeholder};
use crate::click::{self, Term};
use crate::options::Options;
use crate::style::{dim, fg, ramp};
use std::path::Path;

/// `ctx 38%`: context used, on the reversed ramp (green at 0, red above 90).
/// Clicking it types `/compact`.
pub struct CtxUsed;

impl Placeholder for CtxUsed {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts, &click::state_dir(), || Term::current().clone())
    }
}

/// `state` holds the handler marker; `term` is probed only for a link.
pub fn render_with(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let used = ctx.session.context_used?;
    if !used.is_finite() {
        return None;
    }
    let pct = used.clamp(0.0, 100.0).round() as i64;
    let score = (100 - pct * 100 / 91).clamp(0, 100) as i32;
    let color = ramp(score, &ctx.config.ramp);
    let body = format!("{} {}", dim("ctx"), fg(color, &format!("{pct}%")));
    let sid = &ctx.session.session_id;
    Some(click::with_cmd_link(
        body, "compact", sid, opts, state, term,
    ))
}
