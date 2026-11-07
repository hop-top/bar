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
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        render_parts(ctx, opts, &click::state_dir(), label, || {
            Term::current().clone()
        })
    }
}

/// `state` holds the handler marker; `term` is probed only for a link.
pub fn render_with(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    render_parts(ctx, opts, state, true, term)
}

fn render_parts(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    label: bool,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let used = ctx.session.context_used?;
    if !used.is_finite() {
        return None;
    }
    let pct = used.clamp(0.0, 100.0).round() as i64;
    let score = (100 - pct * 100 / 91).clamp(0, 100) as i32;
    let color = ramp(score, &ctx.config.ramp);
    let value = fg(color, &format!("{pct}%"));
    let sid = &ctx.session.session_id;
    let value = click::with_cmd_link(value, "compact", sid, opts, state, term);
    Some(if label {
        format!("{} {value}", dim("ctx"))
    } else {
        value
    })
}
