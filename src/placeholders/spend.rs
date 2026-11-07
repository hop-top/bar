use super::usage::time_left;
use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::dim;

/// `3d4h`: time left on a monthly spend limit, dim; omitted otherwise.
pub struct Spend;

impl Placeholder for Spend {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts)
    }
}

pub fn render_with(ctx: &Ctx, _opts: &Options) -> Option<String> {
    let l = ctx.session.spend_limit.as_ref()?;
    let secs = l.resets_at.saturating_sub(ctx.now);
    (l.period == "monthly" && secs > 0).then(|| dim(&time_left(secs)))
}
