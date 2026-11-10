//! The two outputs that stand for the whole pool rather than one account:
//! `usage` (the provider's windows pooled by the `pool` option) and
//! `account` (the account the proxy prefers, only when that is certain).

use crate::model::{Acct, Win};
use bar_plugin::proto::{Account, Usage, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pool {
    /// Mean percent used, latest reset: the strictest pace.
    #[default]
    Mean,
    /// The one account in the top priority tier; `Mean` when there are
    /// several.
    Active,
    /// The most-used account, per window.
    Max,
}

impl Pool {
    pub fn parse(s: &str) -> Pool {
        match s {
            "active" => Pool::Active,
            "max" => Pool::Max,
            _ => Pool::Mean,
        }
    }
}

/// Can the proxy route a request to this account right now?
fn routable(a: &Acct) -> bool {
    !a.disabled && !a.unavailable
}

/// The only account of the highest priority tier among those the proxy
/// can route to; `None` when the tier holds several (the strategy decides
/// among them) or no account is routable.
pub fn sole_top<'a>(accts: &[&'a Acct]) -> Option<&'a Acct> {
    let best = accts
        .iter()
        .filter(|a| routable(a))
        .map(|a| a.priority)
        .max()?;
    let mut tier = accts.iter().filter(|a| routable(a) && a.priority == best);
    match (tier.next(), tier.next()) {
        (Some(a), None) => Some(*a),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum Which {
    Five,
    Seven,
}

/// The account's window of that length that has not yet reset.
fn window(a: &Acct, which: Which, now: i64) -> Option<&Win> {
    let live = |w: &&Win| w.resets_at > now;
    let find = |id: &str| a.windows.iter().filter(live).find(|w| w.id == id);
    match which {
        Which::Five => find("5h"),
        Which::Seven => find("7d").or_else(|| find("wk")),
    }
}

fn pooled(accts: &[&Acct], which: Which, pool: Pool, now: i64) -> Option<Window> {
    let wins: Vec<&Win> = accts
        .iter()
        .map(|a| window(a, which, now))
        .collect::<Option<_>>()?;
    let first = wins.first()?;
    let (used, reset) = match pool {
        Pool::Max => {
            let w = wins
                .iter()
                .fold(*first, |m, w| if w.used > m.used { w } else { m });
            (w.used, w.resets_at)
        }
        _ => (
            wins.iter().map(|w| w.used).sum::<f64>() / wins.len() as f64,
            wins.iter().map(|w| w.resets_at).max().unwrap_or(0),
        ),
    };
    Some(Window {
        used_percentage: used.clamp(0.0, 100.0),
        resets_at: reset,
    })
}

/// The provider's usage windows. Only accounts with data count, and a
/// window is pooled only when every one of them reports it (a Codex Pro
/// plan has no 5h window, so a pool holding one has none either). `accts`
/// are the provider's enabled accounts.
pub fn usage(accts: &[&Acct], pool: Pool, now: i64) -> Option<Usage> {
    let pool = match (pool, sole_top(accts)) {
        (Pool::Active, Some(top)) => return usage_of(&[top], Pool::Mean, now),
        (Pool::Active, None) => Pool::Mean,
        (p, _) => p,
    };
    let counted: Vec<&Acct> = accts
        .iter()
        .copied()
        .filter(|a| !a.upstream_auth && !a.unavailable && !a.windows.is_empty())
        .collect();
    usage_of(&counted, pool, now)
}

fn usage_of(accts: &[&Acct], pool: Pool, now: i64) -> Option<Usage> {
    let u = Usage {
        five_hour: pooled(accts, Which::Five, pool, now),
        seven_day: pooled(accts, Which::Seven, pool, now),
    };
    (u.five_hour.is_some() || u.seven_day.is_some()).then_some(u)
}

/// The account the proxy prefers, when that is certain: the only one in the
/// top priority tier. `label` is what to show for it.
pub fn account(accts: &[&Acct], label: impl Fn(&Acct) -> String) -> Option<Account> {
    sole_top(accts).map(|a| Account {
        id: a.key.clone(),
        label: label(a),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(key: &str, prio: i64, wins: &[(&str, f64, i64)]) -> Acct {
        Acct {
            key: key.into(),
            provider: "claude".into(),
            label: key.into(),
            priority: prio,
            plan: None,
            windows: wins
                .iter()
                .map(|(id, used, reset)| Win {
                    id: (*id).into(),
                    used: *used,
                    resets_at: *reset,
                    period_s: 0,
                })
                .collect(),
            at: 1,
            disabled: false,
            unavailable: false,
            upstream_auth: false,
            retry_at: 0,
            fails: 0,
        }
    }

    const NOW: i64 = 1000;

    #[test]
    fn mean_pools_the_used_and_takes_the_latest_reset() {
        let a = acct("a", 0, &[("5h", 20.0, 1100), ("7d", 10.0, 5000)]);
        let b = acct("b", 0, &[("5h", 60.0, 1300), ("7d", 30.0, 4000)]);
        let u = usage(&[&a, &b], Pool::Mean, NOW).unwrap();
        let f = u.five_hour.unwrap();
        assert_eq!((f.used_percentage, f.resets_at), (40.0, 1300));
        let s = u.seven_day.unwrap();
        assert_eq!((s.used_percentage, s.resets_at), (20.0, 5000));
    }

    #[test]
    fn a_window_not_every_account_reports_is_not_pooled() {
        let a = acct("a", 0, &[("5h", 20.0, 1100), ("7d", 10.0, 5000)]);
        let b = acct("b", 0, &[("7d", 30.0, 4000)]);
        let u = usage(&[&a, &b], Pool::Mean, NOW).unwrap();
        assert!(u.five_hour.is_none());
        assert!(u.seven_day.is_some());
        // an account with no data at all does not veto the others
        let idle = acct("c", 0, &[]);
        let u = usage(&[&a, &idle], Pool::Mean, NOW).unwrap();
        assert!(u.five_hour.is_some());
    }

    #[test]
    fn a_window_that_already_reset_is_not_pooled() {
        let a = acct("a", 0, &[("5h", 20.0, 900), ("7d", 10.0, 5000)]);
        let u = usage(&[&a], Pool::Mean, NOW).unwrap();
        assert!(u.five_hour.is_none() && u.seven_day.is_some());
        let none = acct("n", 0, &[("5h", 20.0, 900)]);
        assert!(usage(&[&none], Pool::Mean, NOW).is_none());
    }

    #[test]
    fn weekly_may_be_called_wk() {
        let a = acct("a", 0, &[("wk", 25.0, 5000)]);
        let u = usage(&[&a], Pool::Mean, NOW).unwrap();
        assert_eq!(u.seven_day.unwrap().used_percentage, 25.0);
    }

    #[test]
    fn max_takes_the_most_used_account_and_its_reset() {
        let a = acct("a", 0, &[("7d", 10.0, 5000)]);
        let b = acct("b", 0, &[("7d", 70.0, 4000)]);
        let s = usage(&[&a, &b], Pool::Max, NOW).unwrap().seven_day.unwrap();
        assert_eq!((s.used_percentage, s.resets_at), (70.0, 4000));
    }

    #[test]
    fn active_follows_the_top_tier_else_the_mean() {
        let a = acct("a", 0, &[("7d", 10.0, 5000)]);
        let b = acct("b", 3, &[("7d", 70.0, 4000)]);
        let s = usage(&[&a, &b], Pool::Active, NOW)
            .unwrap()
            .seven_day
            .unwrap();
        assert_eq!((s.used_percentage, s.resets_at), (70.0, 4000));
        // two in the top tier: no single active account
        let c = acct("c", 3, &[("7d", 30.0, 6000)]);
        let s = usage(&[&a, &b, &c], Pool::Active, NOW)
            .unwrap()
            .seven_day
            .unwrap();
        assert_eq!((s.used_percentage, s.resets_at), (110.0 / 3.0, 6000));
    }

    #[test]
    fn the_preferred_account_needs_a_sole_top_tier() {
        let a = acct("a", 0, &[]);
        let b = acct("b", 2, &[]);
        let label = |a: &Acct| a.label.clone();
        assert_eq!(account(&[&a, &b], label).unwrap().id, "b");
        assert_eq!(account(&[&a], label).unwrap().id, "a");
        let c = acct("c", 2, &[]);
        assert!(account(&[&a, &b, &c], label).is_none(), "tie");
        assert!(account(&[&a, &a], label).is_none(), "round-robin pool");
        // a cooling top account drops out of the tier
        let mut cool = acct("d", 9, &[]);
        cool.unavailable = true;
        assert_eq!(account(&[&a, &b, &cool], label).unwrap().id, "b");
        assert!(account(&[], label).is_none());
    }
}
