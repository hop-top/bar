//! Pace score: 0 (red) to 100 (green); may fall outside, `style::ramp` clamps.

use crate::conf::{Amount, Early, Rule};

/// Built-in rule for `usage[type=<key>]` when the config has none.
pub fn builtin_rule(key: &str) -> Rule {
    Rule {
        target: format!("usage[type={key}]"),
        early: Early::Thresholds {
            warn: Amount::Percent(20),
            bad: Amount::Percent(5),
        },
        final_secs: if key == "7d" { 86_400 } else { 3_600 },
        final_bad: Amount::Percent(30),
    }
}

fn percent(a: Amount) -> i64 {
    match a {
        Amount::Percent(p) => i64::from(p),
        Amount::Cents(_) => 0,
    }
}

/// `pct_left` and results are whole percents. Final stretch (`secs_left` at or
/// below the rule's): `100 - left * 100 / (bad + 1)`. Early: 0 at or below
/// `red`, 50 at `amber`, 100 from `top` up, linear between; both floors rise
/// with the time left. Without `secs_left` the floors stand alone.
pub fn score(pct_left: i64, secs_left: Option<i64>, period: i64, rule: &Rule) -> i32 {
    let (warn, bad) = match rule.early {
        Early::Thresholds { warn, bad } => (percent(warn), percent(bad)),
        Early::Pace => (20, 5),
    };
    let (mut amber, mut red, mut share) = (warn, bad, 0);
    if let Some(secs) = secs_left.filter(|_| period > 0) {
        let last = i64::try_from(rule.final_secs).unwrap_or(i64::MAX);
        if secs <= last {
            let fin = match rule.final_bad {
                Amount::Percent(p) => i64::from(p),
                Amount::Cents(_) => 30,
            };
            return (100 - pct_left * 100 / (fin + 1)) as i32;
        }
        share = 100 * secs / period;
        amber = amber.max(50 * secs / period);
        red = red.max(20 * secs / period);
    }
    let top = if share <= amber { amber + 1 } else { share };
    let v = if pct_left <= red {
        0
    } else if pct_left <= amber {
        (pct_left - red) * 50 / (amber - red)
    } else if pct_left < top {
        50 + (pct_left - amber) * 50 / (top - amber)
    } else {
        100
    };
    v as i32
}
