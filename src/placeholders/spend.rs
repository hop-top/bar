use super::money::{self, Budget};
use super::usage::time_left;
use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::dim;
use crate::tz::{civil, days_from_civil};

/// `3d4h`: time left on a spend limit, dim; with `show`, the limit's
/// money components. Omitted for an unknown period.
pub struct Spend;

impl Placeholder for Spend {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts)
    }
}

const DAY: i64 = 86_400;

/// Length of the limit's period, seconds, for the pace: a day, a week, or
/// the calendar month before `resets_at`. Without a period (Claude Code
/// sends it later than the percentage), the shortest of those that holds
/// the time left. `None` for a period bar does not know.
pub fn period_secs(period: Option<&str>, resets_at: i64, now: i64) -> Option<i64> {
    let month = || resets_at - month_before(resets_at);
    match period {
        Some("daily") => Some(DAY),
        Some("weekly") => Some(7 * DAY),
        Some("monthly") => Some(month()),
        Some(_) => None,
        None => {
            let left = resets_at - now;
            Some(if left <= DAY {
                DAY
            } else if left <= 7 * DAY {
                7 * DAY
            } else {
                month()
            })
        }
    }
}

/// The same UTC time one calendar month before `ts`, the day clamped to
/// the length of that month.
fn month_before(ts: i64) -> i64 {
    let days = ts.div_euclid(DAY);
    let (y, m, d) = civil(days);
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let len = days_from_civil(y, m, 1) - days_from_civil(py, pm, 1);
    let pd = i64::from(d).min(len) as u32;
    days_from_civil(py, pm, pd) * DAY + ts.rem_euclid(DAY)
}

pub fn render_with(ctx: &Ctx, opts: &Options) -> Option<String> {
    let l = ctx.session.spend_limit.as_ref()?;
    let secs = l.resets_at.saturating_sub(ctx.now);
    let period = period_secs(l.period.as_deref(), l.resets_at, ctx.now)?;
    if secs <= 0 {
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
        period,
        today: None,
    };
    money::render(ctx, opts, &b, "spend", "time")
}
