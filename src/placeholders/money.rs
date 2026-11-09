//! Components of a spending budget with a limit and a reset: shared by
//! `extra` (extra usage) and `spend` (a gateway spend limit).

use super::usage::glyphs::{bar, level};
use super::usage::{pct_left, time_left};
use super::Ctx;
use crate::conf::{Amount, Early, Rule};
use crate::options::Options;
use crate::pace::score;
use crate::style::{dim, fg, ramp};

/// Components `show` accepts.
pub const SHOW: [&str; 9] = [
    "used", "limit", "left", "today", "daily", "pct", "bar", "gauge", "time",
];

/// A budget at one instant; amounts in minor units (cents).
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub used: i64,
    pub limit: i64,
    /// Used percent of the limit.
    pub util: f64,
    pub currency: String,
    /// Unix seconds of the reset.
    pub resets_at: i64,
    /// Length of the budget period, seconds (the pace period).
    pub period: i64,
    /// Spent today; `None` when unknown.
    pub today: Option<i64>,
}

impl Budget {
    pub fn left(&self) -> i64 {
        (self.limit - self.used).max(0)
    }

    /// Days to the reset, today included: at least 1.
    pub fn days_left(&self, now: i64) -> i64 {
        let secs = (self.resets_at - now).max(1);
        (secs + 86_399) / 86_400
    }

    /// What is left spread over the days to the reset.
    pub fn daily(&self, now: i64) -> i64 {
        self.left() / self.days_left(now)
    }
}

/// `12.34` from minor units; `-0.50` below zero.
fn decimal(minor: i64) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let m = minor.unsigned_abs();
    format!("{sign}{}.{:02}", m / 100, m % 100)
}

/// `$1.23` from dollars, rounded to the cent.
pub fn dollars(usd: f64) -> String {
    format!("${usd:.2}")
}

/// `$1.23`; `<$0.01` for a positive amount under a cent.
pub fn usd(usd: f64) -> String {
    if usd > 0.0 && usd < 0.01 {
        "<$0.01".into()
    } else {
        dollars(usd)
    }
}

/// `$20`, or `$12.50` when the cents are not zero.
pub fn usd_whole(cents: u64) -> String {
    if cents.is_multiple_of(100) {
        format!("${}", cents / 100)
    } else {
        format!("${}", decimal(cents as i64))
    }
}

/// `$12.34`; other currencies `12.34 EUR`.
pub fn amount(minor: i64, currency: &str) -> String {
    let n = decimal(minor);
    if currency == "USD" {
        format!("${n}")
    } else {
        format!("{n} {currency}")
    }
}

/// Built-in pace rule: amber at 20% left, red at 5%, no final stretch
/// (money left at the reset is not lost the way unused usage is).
pub fn builtin_rule(target: &str) -> Rule {
    Rule {
        target: target.into(),
        early: Early::Thresholds {
            warn: Amount::Percent(20),
            bad: Amount::Percent(5),
        },
        final_secs: 0,
        final_bad: Amount::Percent(30),
    }
}

fn rule_for(ctx: &Ctx, target: &str) -> Rule {
    ctx.config
        .rules
        .iter()
        .find(|r| r.target == target)
        .cloned()
        .unwrap_or_else(|| builtin_rule(target))
}

/// The `show` components of `b`, space-separated; `limit` right after
/// `used` joins it with a slash (`$12.34/$50.00`). `rule` names the
/// target of a `rule` line for the colour. `None` past the reset.
pub fn render(
    ctx: &Ctx,
    opts: &Options,
    b: &Budget,
    target: &str,
    default: &str,
) -> Option<String> {
    let secs = b.resets_at.saturating_sub(ctx.now);
    if secs <= 0 || b.limit <= 0 {
        return None;
    }
    let left = pct_left(b.util);
    let rule = rule_for(ctx, target);
    let color = ramp(score(left, Some(secs), b.period, &rule), &ctx.config.ramp);
    let value = if opts.get("count") == Some("left") {
        left
    } else {
        100 - left
    };
    let width = opts
        .get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40);
    let raw = opts.get("show").filter(|s| !s.trim().is_empty());
    let mut show: Vec<&str> = Vec::new();
    for p in raw.unwrap_or(default).split(',').map(str::trim) {
        if SHOW.contains(&p) && !show.contains(&p) {
            show.push(p);
        }
    }
    let cur = b.currency.as_str();
    let mut out = String::new();
    let mut prev = "";
    for p in show {
        let text = match p {
            "used" => fg(color, &amount(b.used, cur)),
            "limit" => amount(b.limit, cur),
            "left" => format!("{} {}", fg(color, &amount(b.left(), cur)), dim("left")),
            "today" => match b.today {
                Some(t) => format!("{} {}", amount(t, cur), dim("today")),
                None => continue,
            },
            "daily" => format!("{}{}", amount(b.daily(ctx.now), cur), dim("/d")),
            "pct" => fg(color, &format!("{value}%")),
            "bar" => bar(
                opts.get("glyphs").unwrap_or("blocks"),
                width,
                value,
                &|_, _| color,
            ),
            "gauge" => fg(color, &level(value as f64).to_string()),
            _ => dim(&time_left(secs)),
        };
        if !out.is_empty() {
            out.push_str(if prev == "used" && p == "limit" {
                "/"
            } else {
                " "
            });
        }
        out.push_str(&text);
        prev = p;
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(amount(1234, "USD"), "$12.34");
        assert_eq!(amount(5, "USD"), "$0.05");
        assert_eq!(amount(5000, "EUR"), "50.00 EUR");
        assert_eq!(amount(-50, "USD"), "$-0.50");
        assert_eq!(amount(-1250, "USD"), "$-12.50");
        assert_eq!(
            (usd(0.004), usd(0.0), usd(3.126)),
            ("<$0.01".into(), "$0.00".into(), "$3.13".into())
        );
        assert_eq!(dollars(0.004), "$0.00");
        assert_eq!(
            (usd_whole(2000), usd_whole(1250), usd_whole(5)),
            ("$20".into(), "$12.50".into(), "$0.05".into())
        );
    }

    #[test]
    fn daily_budget_spreads_what_is_left() {
        let b = Budget {
            used: 2000,
            limit: 5000,
            util: 40.0,
            currency: "USD".into(),
            resets_at: 10 * 86_400,
            period: 30 * 86_400,
            today: None,
        };
        // 3000 left over 10 days
        assert_eq!(b.days_left(0), 10);
        assert_eq!(b.daily(0), 300);
        // part of a day counts as one
        assert_eq!(b.days_left(10 * 86_400 - 60), 1);
        assert_eq!(b.daily(10 * 86_400 - 60), 3000);
        let over = Budget { used: 6000, ..b };
        assert_eq!((over.left(), over.daily(0)), (0, 0));
    }
}
