//! `spendhist`: estimated API-equivalent spend for today and this month,
//! from the background-built aggregate of all transcripts.

use super::{Ctx, Placeholder};
use crate::cache::Job;
use crate::options::Options;
use crate::spendhist::{cache_key, claude_dir, date_of, Summary, Tz};
use crate::style::dim;

/// The aggregate is rebuilt at most this often, seconds.
const TTL: u64 = 60;

pub struct SpendHist;

fn amount(usd: f64, approx: bool) -> String {
    format!("${usd:.2}{}", if approx { "~" } else { "" })
}

/// `today $3.12 · mon $84.10`; one bare amount for a single period. `~`
/// marks an amount that left out tokens of a model without a price.
pub fn fmt(period: &str, today: f64, today_approx: bool, month: f64, month_approx: bool) -> String {
    let (t, m) = (amount(today, today_approx), amount(month, month_approx));
    match period {
        "today" => t,
        "month" => m,
        _ => format!("{} {t} · {} {m}", dim("today"), dim("mon")),
    }
}

impl Placeholder for SpendHist {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let cache = ctx.cache?;
        let dir = claude_dir(ctx.config.claude_dir.as_deref())?;
        let tz = if opts.get("tz") == Some("utc") {
            Tz::Utc
        } else {
            Tz::Local
        };
        let job = Job::SpendHist {
            dir: dir.to_string_lossy().into_owned(),
            state: cache.state_dir().to_string_lossy().into_owned(),
            utc: tz == Tz::Utc,
        };
        let stored = cache.read_or_refresh(&cache_key(&dir, tz), TTL, &job)?;
        let sum = Summary::parse(&stored)?;
        let day = date_of(ctx.now, tz);
        let (t, m) = (
            sum.spend(&day, &ctx.config.prices),
            sum.spend(&day[..7], &ctx.config.prices),
        );
        let period = opts.get("period").unwrap_or("both");
        Some(fmt(period, t.usd, t.unknown, m.usd, m.unknown))
    }
}
