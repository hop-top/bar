//! `extra`: extra usage (overage) this month from the usage API
//! (`set usage-api on`): spent, limit, left, today, daily budget.

use super::money::{self, Budget};
use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::tz::{month_bounds, Tz};
use crate::usageapi::{self, Data};

pub struct Extra;

/// Default components.
pub const DEFAULT_SHOW: &str = "used,limit";

/// The month's budget from cached numbers at `now`: the period is the
/// local calendar month. `None` without extra usage, while it is turned
/// off for the account, or without a limit.
pub fn budget(data: &Data, now: i64) -> Option<Budget> {
    let (e, today) = data.extra(now, Tz::Local).filter(|(e, _)| e.enabled)?;
    let (start, end) = month_bounds(now, Tz::Local);
    Some(Budget {
        used: e.used,
        limit: e.limit,
        util: e.util,
        currency: e.currency.clone(),
        resets_at: end,
        period: end - start,
        today,
    })
}

pub fn render_budget(ctx: &Ctx, opts: &Options, b: &Budget) -> Option<String> {
    money::render(ctx, opts, b, "extra", DEFAULT_SHOW)
}

impl Placeholder for Extra {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        if !ctx.config.usage_api {
            return None;
        }
        let data = usageapi::load(ctx.cache?, ctx.now)?;
        render_budget(ctx, opts, &budget(&data, ctx.now)?)
    }
}
