use super::{Ctx, Placeholder};
use crate::budget::{paint_if_set, Kind};
use crate::options::Options;
use crate::style::{dim, fg};

/// `$1.23`; `<$0.01` for a positive amount under a cent.
pub(crate) fn usd(amount: f64) -> String {
    if amount > 0.0 && amount < 0.01 {
        "<$0.01".into()
    } else {
        format!("${amount:.2}")
    }
}

/// Session cost so far (`cost.total_cost_usd`).
pub struct Cost;
/// Cost per hour of session time; hidden for the first minute.
pub struct Burn;
/// Session wall time: `45s`, `12m`, `1h05m`.
pub struct Duration;
/// Time spent waiting on API responses, formatted like `duration`.
pub struct Api;
/// `+120 -14`: lines added and removed; hidden when both are 0.
pub struct Lines;

impl Placeholder for Cost {
    // `fmt=usd` is the only format; any other value renders as usd.
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let cost = ctx.session.cost_usd?;
        Some(paint_if_set(ctx.config, Kind::Session, cost, usd(cost)))
    }
}

/// Burn shows only past this much session time; earlier it is noise.
const MIN_BURN_MS: f64 = 60_000.0;

impl Placeholder for Burn {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let (cost, ms) = (ctx.session.cost_usd?, ctx.session.duration_ms?);
        if ms < MIN_BURN_MS {
            return None;
        }
        let per_hour = cost / (ms / 3_600_000.0);
        per_hour.is_finite().then(|| format!("{}/h", usd(per_hour)))
    }
}

pub(crate) fn compact(ms: f64) -> String {
    // Capped so a corrupt value cannot print absurd hours.
    let secs = (ms.min(999.0 * 3_600_000.0) / 1000.0) as u64;
    match (secs / 3600, secs / 60 % 60) {
        (0, 0) => format!("{secs}s"),
        (0, m) => format!("{m}m"),
        (h, m) => format!("{h}h{m:02}m"),
    }
}

impl Placeholder for Duration {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        Some(dim(&compact(ctx.session.duration_ms?)))
    }
}

impl Placeholder for Api {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        Some(dim(&compact(ctx.session.api_duration_ms?)))
    }
}

impl Placeholder for Lines {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let (a, r) = (ctx.session.lines_added, ctx.session.lines_removed);
        if a == 0 && r == 0 {
            return None;
        }
        let c = &ctx.config.colors;
        Some(format!(
            "{} {}",
            fg(c.good, &format!("+{a}")),
            fg(c.bad, &format!("-{r}"))
        ))
    }
}
