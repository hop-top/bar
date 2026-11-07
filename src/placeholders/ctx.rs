use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{dim, fg, ramp};

/// `ctx 38%`: context used, on the reversed ramp (green at 0, red above 90).
pub struct CtxUsed;

impl Placeholder for CtxUsed {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let used = ctx.session.context_used?;
        if !used.is_finite() {
            return None;
        }
        let pct = used.clamp(0.0, 100.0).round() as i64;
        let score = (100 - pct * 100 / 91).clamp(0, 100) as i32;
        let color = ramp(score, &ctx.config.ramp);
        Some(format!("{} {}", dim("ctx"), fg(color, &format!("{pct}%"))))
    }
}
