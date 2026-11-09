//! `budget`: spend against a `set budget-<kind>` limit.

use super::money::{usd, usd_whole};
use super::spendhist::{summary, tz_opt};
use super::usage::glyphs::{self, level};
use super::{Ctx, Placeholder};
use crate::budget::{standing, used_pct, Kind};
use crate::conf::Config;
use crate::options::Options;
use crate::spendhist::{Spend, BLOCK_HOURS};
use crate::style::dim;

pub struct Budget;

/// Components `show` accepts.
pub const SHOW_ITEMS: [&str; 7] = ["spent", "limit", "ratio", "pct", "left", "bar", "gauge"];
/// `show` when unset.
pub const DEFAULT_SHOW: &str = "ratio,bar";

impl Placeholder for Budget {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let kind = Kind::parse(opts.get("type").unwrap_or("session"))?;
        let limit = ctx.config.budgets.get(kind)?;
        Some(fmt(ctx.config, opts, spent(ctx, opts, kind)?, limit))
    }
}

/// Spend so far in the `kind` period; `None` when it is not known.
fn spent(ctx: &Ctx, opts: &Options, kind: Kind) -> Option<Spend> {
    if kind == Kind::Session {
        let usd = ctx.session.cost_usd?;
        return Some(Spend {
            usd,
            unknown: false,
        });
    }
    let (sum, day) = summary(ctx, tz_opt(opts))?;
    let prices = &ctx.config.prices;
    Some(match kind {
        Kind::Today => sum.spend(&day, prices),
        Kind::Month => sum.spend(&day[..7], prices),
        _ => match block_start(ctx)
            .map(|h| (h, false))
            .or_else(|| sum.block(ctx.now))
        {
            Some((hour, guess)) => {
                let mut s = sum.spend_since(hour, prices);
                s.unknown |= guess;
                s
            }
            None => Spend {
                usd: 0.0,
                unknown: false,
            },
        },
    })
}

/// First hour of the 5-hour window Claude Code reports, while it runs.
fn block_start(ctx: &Ctx) -> Option<i64> {
    let w = ctx.session.five_hour.as_ref()?;
    (w.resets_at > ctx.now).then(|| (w.resets_at - BLOCK_HOURS * 3600).div_euclid(3600))
}

fn width_opt(opts: &Options) -> i64 {
    opts.get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40)
}

/// Known `show` items in order, repeats dropped; the default when none.
fn show_list(opts: &Options) -> Vec<&str> {
    let mut out = Vec::new();
    for item in opts
        .get("show")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
    {
        if SHOW_ITEMS.contains(&item) && !out.contains(&item) {
            out.push(item);
        }
    }
    if out.is_empty() {
        return DEFAULT_SHOW.split(',').collect();
    }
    out
}

/// The `show` components for `spent` against `limit` cents.
pub fn fmt(cfg: &Config, opts: &Options, spent: Spend, limit: u64) -> String {
    let st = standing(spent.usd, limit, cfg);
    let used = used_pct(spent.usd, limit);
    let count_left = opts.get("count") == Some("left");
    let value = if count_left { 100.0 - used } else { used };
    let fill = value.clamp(0.0, 100.0).round() as i64;
    let tilde = if spent.unknown { "~" } else { "" };
    let amount = format!("{}{tilde}", usd(spent.usd));
    let left_cents = limit as f64 - (spent.usd * 100.0).round();
    let mut parts = Vec::new();
    for item in show_list(opts) {
        parts.push(match item {
            "spent" => st.paint(&amount),
            "limit" => dim(&usd_whole(limit)),
            "ratio" => format!(
                "{}{}",
                st.paint(&amount),
                dim(&format!("/{}", usd_whole(limit)))
            ),
            "pct" => st.paint(&format!("{}%", value.round() as i64)),
            "left" if st.over => format!(
                "{} {}",
                st.paint(&usd(-left_cents / 100.0)),
                st.paint("over")
            ),
            "left" => format!("{} {}", st.paint(&usd(left_cents / 100.0)), dim("left")),
            "bar" => glyphs::bar(
                opts.get("glyphs").unwrap_or("blocks"),
                width_opt(opts),
                fill,
                &|_, _| st.color,
            ),
            _ => st.paint(&level(fill as f64).to_string()),
        });
    }
    parts.join(" ")
}
