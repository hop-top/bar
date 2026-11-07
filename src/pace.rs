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
            return fit(100_i64.saturating_sub(pct_left.saturating_mul(100) / (fin + 1)));
        }
        share = secs.saturating_mul(100) / period;
        amber = amber.max(secs.saturating_mul(50) / period);
        red = red.max(secs.saturating_mul(20) / period);
    }
    let top = if share <= amber {
        amber.saturating_add(1)
    } else {
        share
    };
    let v = if pct_left <= red {
        0
    } else if pct_left <= amber {
        (pct_left - red).saturating_mul(50) / (amber - red)
    } else if pct_left < top {
        50_i64.saturating_add((pct_left - amber).saturating_mul(50) / (top - amber))
    } else {
        100
    };
    fit(v)
}

/// Straight-line projection of a window at its average burn so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forecast {
    /// Seconds until the budget runs out, from now. Only meaningful when
    /// `proj` is over 100.
    pub eta_secs: i64,
    /// Used percent at reset, rounded, 0..=999.
    pub proj: i64,
}

/// `None` while nothing is used or under 2% of the window has elapsed: the
/// rate is too noisy then.
pub fn forecast(used: f64, secs_left: i64, period: i64) -> Option<Forecast> {
    let elapsed = period - secs_left;
    if !used.is_finite() || used <= 0.0 || period <= 0 || elapsed * 50 < period {
        return None;
    }
    let eta = (100.0 - used).max(0.0) * elapsed as f64 / used;
    let proj = used * period as f64 / elapsed as f64;
    Some(Forecast {
        eta_secs: eta.min(i64::MAX as f64) as i64,
        proj: proj.clamp(0.0, 999.0).round() as i64,
    })
}

/// Scores outside -100..=100 carry no extra meaning; keeps the cast exact.
fn fit(v: i64) -> i32 {
    v.clamp(-100, 100) as i32
}

fn cents_i64(c: u64) -> i64 {
    i64::try_from(c).unwrap_or(i64::MAX)
}

/// Score for a plugin `metric`: `remaining` and `granted` in cents. Final
/// stretch: `100 - left * 100 / (bad + 1)` with `left` in cents for a `$`
/// bad, in percent of the grant for a `%` one, so unspent credit turns red
/// before it expires. Before that, `score` on the percent left of the grant,
/// `$` thresholds read as shares of it. No grant scores 100.
pub fn metric_score(
    remaining: i64,
    granted: i64,
    secs_left: Option<i64>,
    period: i64,
    rule: &Rule,
) -> i32 {
    let share = |cents: i64| {
        if granted > 0 {
            cents.max(0).saturating_mul(100) / granted
        } else {
            0
        }
    };
    let pct_left = share(remaining);
    let last = i64::try_from(rule.final_secs).unwrap_or(i64::MAX);
    if secs_left.is_some_and(|s| s <= last) {
        let (left, bad) = match rule.final_bad {
            Amount::Cents(c) => (remaining.max(0), cents_i64(c)),
            Amount::Percent(p) => (pct_left, i64::from(p)),
        };
        return fit(100_i64.saturating_sub(left.saturating_mul(100) / bad.saturating_add(1)));
    }
    if granted <= 0 {
        return 100;
    }
    let as_pct = |a: Amount| match a {
        Amount::Percent(p) => Amount::Percent(p),
        Amount::Cents(c) => Amount::Percent(u32::try_from(share(cents_i64(c))).unwrap_or(u32::MAX)),
    };
    let early = match rule.early {
        Early::Thresholds { warn, bad } => Early::Thresholds {
            warn: as_pct(warn),
            bad: as_pct(bad),
        },
        Early::Pace => Early::Pace,
    };
    let rule = Rule {
        early,
        ..rule.clone()
    };
    score(pct_left, secs_left, period, &rule)
}

#[cfg(test)]
mod forecast_tests {
    use super::forecast;

    const P: i64 = 5 * 3600;

    #[test]
    fn edges_yield_none() {
        assert!(forecast(f64::NAN, P / 2, P).is_none());
        assert!(forecast(-5.0, P / 2, P).is_none());
        assert!(forecast(0.0, P / 2, P).is_none());
        assert!(forecast(10.0, P + 1, P).is_none());
        assert!(forecast(10.0, P, P).is_none());
        assert!(forecast(10.0, P / 2, 0).is_none());
        assert!(forecast(10.0, P - 359, P).is_none());
        assert!(forecast(10.0, P - 360, P).is_some());
    }

    #[test]
    fn over_full_window_is_zero_eta_and_capped() {
        let f = forecast(150.0, P / 2, P).unwrap();
        assert_eq!((f.eta_secs, f.proj), (0, 300));
        assert_eq!(forecast(100.0, P / 2, P).unwrap().eta_secs, 0);
    }

    #[test]
    fn halfway_math() {
        let f = forecast(25.0, P / 2, P).unwrap();
        assert_eq!((f.eta_secs, f.proj), (P / 2 * 3, 50));
    }
}
