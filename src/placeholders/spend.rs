use super::money::{self, Budget};
use super::usage::time_left;
use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::dim;

/// `3d4h`: time left on a monthly spend limit, dim; with `show`, the
/// limit's money components. Omitted otherwise.
pub struct Spend;

impl Placeholder for Spend {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts)
    }
}

/// Pace period of a monthly limit, seconds.
const MONTH: i64 = 30 * 86_400;

pub fn render_with(ctx: &Ctx, opts: &Options) -> Option<String> {
    let l = ctx.session.spend_limit.as_ref()?;
    let secs = l.resets_at.saturating_sub(ctx.now);
    if l.period != "monthly" || secs <= 0 {
        return None;
    }
    let show = opts.get("show").map(str::trim).unwrap_or_default();
    if show.is_empty() || show == "time" {
        return Some(dim(&time_left(secs)));
    }
    let cents = |usd: f64| (usd * 100.0).round() as i64;
    let limit = cents(l.limit_usd?);
    let used = l.used_usd.map(cents);
    let util = l.used_percentage.or_else(|| {
        used.filter(|_| limit > 0)
            .map(|u| u as f64 * 100.0 / limit as f64)
    })?;
    let b = Budget {
        used: used.unwrap_or_else(|| (util * limit as f64 / 100.0).round() as i64),
        limit,
        util,
        currency: "USD".into(),
        resets_at: l.resets_at,
        period: MONTH,
        today: None,
    };
    money::render(ctx, opts, &b, "spend", "time")
}
