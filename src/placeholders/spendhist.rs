//! `spendhist`: estimated API-equivalent spend for today and this month,
//! from the background-built aggregate of all transcripts.

use super::money::dollars;
use super::{Ctx, Placeholder};
use crate::budget::{paint_if_set, Kind};
use crate::cache::Job;
use crate::conf::Config;
use crate::options::Options;
use crate::prices::models;
use crate::spendhist::{cache_key, claude_dir, date_of, Spend, Summary, Tz};
use crate::style::dim;
use std::time::Duration;

/// The aggregate is rebuilt at most this often, seconds.
const TTL: u64 = 60;
/// Cache key of the background models.dev fetch.
pub const PRICES_KEY: &str = "prices:models.dev";
/// The models.dev snapshot is fetched at most this often, seconds.
pub const PRICES_TTL: u64 = 86_400;

pub struct SpendHist;

fn amount(usd: f64, approx: bool) -> String {
    format!("{}{}", dollars(usd), if approx { "~" } else { "" })
}

/// `today $3.12 · mon $84.10`; one bare amount for a single period. `~`
/// marks an amount that left out tokens of a model without a price.
pub fn fmt(period: &str, today: f64, today_approx: bool, month: f64, month_approx: bool) -> String {
    let spend = |usd, unknown| Spend { usd, unknown };
    fmt_with(
        &Config::default(),
        period,
        spend(today, today_approx),
        spend(month, month_approx),
    )
}

/// As `fmt`, each amount coloured against its budget when one is set.
pub fn fmt_with(cfg: &Config, period: &str, today: Spend, month: Spend) -> String {
    let t = paint_if_set(
        cfg,
        Kind::Today,
        today.usd,
        amount(today.usd, today.unknown),
    );
    let m = paint_if_set(
        cfg,
        Kind::Month,
        month.usd,
        amount(month.usd, month.unknown),
    );
    match period {
        "today" => t,
        "month" => m,
        _ => format!("{} {t} · {} {m}", dim("today"), dim("mon")),
    }
}

/// Zone of the `tz` option: `utc`, else local.
pub fn tz_opt(opts: &Options) -> Tz {
    if opts.get("tz") == Some("utc") {
        Tz::Utc
    } else {
        Tz::Local
    }
}

/// The background-built aggregate and today's date in `tz`; `None` until
/// the first scan is cached. Starts the scan, and the models.dev fetch when
/// a model this month needs it; never waits for either.
pub fn summary(ctx: &Ctx, tz: Tz) -> Option<(Summary, String)> {
    let cache = ctx.cache?;
    let dir = claude_dir(ctx.config.claude_dir.as_deref())?;
    let job = Job::SpendHist {
        dir: dir.to_string_lossy().into_owned(),
        state: cache.state_dir().to_string_lossy().into_owned(),
        utc: tz == Tz::Utc,
    };
    let stored = cache.read_or_refresh(&cache_key(&dir, tz), TTL, &job)?;
    let sum = Summary::parse(&stored)?;
    let day = date_of(ctx.now, tz);
    if sum.needs_snapshot(&day[..7], &ctx.config.prices)
        && models::stale(Duration::from_secs(PRICES_TTL))
    {
        // the price shows as unknown until a later render reads it
        cache.read_or_refresh(PRICES_KEY, PRICES_TTL, &Job::Prices);
    }
    Some((sum, day))
}

impl Placeholder for SpendHist {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let (sum, day) = summary(ctx, tz_opt(opts))?;
        let (t, m) = (
            sum.spend(&day, &ctx.config.prices),
            sum.spend(&day[..7], &ctx.config.prices),
        );
        let period = opts.get("period").unwrap_or("both");
        Some(fmt_with(ctx.config, period, t, m))
    }
}
