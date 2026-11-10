//! Which account to switch to: the strategies behind `use --next`, `pick`
//! and the `next` and `next-soon` actions.
//!
//! An account's *left* is the smallest share of quota left over its
//! windows: 100 minus the largest percent used. A window whose reset time
//! has passed counts as unused. An account with no window data is
//! *unread* and has no left. The window that limits an account (the one
//! with the least left) gives its *reset*: when that quota returns, if
//! the window says.
//!
//! Accounts of one provider fall into buckets, best first:
//!
//! 0. available, with quota left (left above 0),
//! 1. available but exhausted (left 0),
//! 2. available but unread,
//! 3. cooling down (the proxy marks them unavailable).
//!
//! Disabled accounts are not ranked. Only bucket 0 is ever chosen by
//! `next`; the others are listed by `pick` after it, so what is known
//! stays visible.
//!
//! Within bucket 0, `most-left` orders by left, highest first. For
//! `soonest-reset`, the accounts whose left is within [`TIE_BAND`] points
//! of the best come first, soonest reset first, so quota about to expire
//! is used before it is lost; the rest follow by left. Ties fall to the
//! sooner reset, then to the account's position.

use crate::model::Win;
use crate::switch::Entry;

/// Points of left that count as a tie for `soonest-reset`.
pub const TIE_BAND: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    MostLeft,
    SoonestReset,
}

impl Strategy {
    pub fn parse(s: &str) -> Option<Strategy> {
        match s {
            "most-left" => Some(Strategy::MostLeft),
            "soonest-reset" => Some(Strategy::SoonestReset),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Strategy::MostLeft => "most-left",
            Strategy::SoonestReset => "soonest-reset",
        }
    }
}

/// `(left, reset)` of one window at `now`.
fn window(w: &Win, now: i64) -> (f64, Option<i64>) {
    if w.resets_at > 0 && w.resets_at <= now {
        (100.0, None)
    } else {
        (
            (100.0 - w.used).clamp(0.0, 100.0),
            (w.resets_at > now).then_some(w.resets_at),
        )
    }
}

/// The account's left and the reset of its limiting window; `None` when
/// it has no windows.
pub fn standing(e: &Entry, now: i64) -> Option<(f64, Option<i64>)> {
    e.windows
        .iter()
        .map(|w| window(w, now))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked<'a> {
    pub entry: &'a Entry,
    pub left: Option<f64>,
    pub reset: Option<i64>,
    pub bucket: u8,
}

/// The account the proxy serves first: the only available one at the
/// highest priority of its provider; `None` for a tie or no account.
pub fn preferred<'a>(list: &'a [Entry], provider: &str) -> Option<&'a Entry> {
    let live: Vec<&Entry> = list
        .iter()
        .filter(|e| e.provider == provider && !e.disabled && !e.unavailable)
        .collect();
    let top = live.iter().map(|e| e.priority).max()?;
    let mut at_top = live.into_iter().filter(|e| e.priority == top);
    let one = at_top.next()?;
    at_top.next().is_none().then_some(one)
}

fn sooner(a: Option<i64>, b: Option<i64>) -> std::cmp::Ordering {
    a.unwrap_or(i64::MAX).cmp(&b.unwrap_or(i64::MAX))
}

fn more(a: f64, b: f64) -> std::cmp::Ordering {
    b.total_cmp(&a)
}

/// The enabled accounts of `provider` in the order `strategy` recommends,
/// without `exclude` (an `auth_index`).
pub fn rank<'a>(
    list: &'a [Entry],
    provider: &str,
    strategy: Strategy,
    now: i64,
    exclude: Option<&str>,
) -> Vec<Ranked<'a>> {
    let mut rows: Vec<Ranked> = list
        .iter()
        .filter(|e| e.provider == provider && !e.disabled && exclude != Some(e.index.as_str()))
        .map(|e| {
            let st = standing(e, now);
            let bucket = match (e.unavailable, st) {
                (true, _) => 3,
                (false, None) => 2,
                (false, Some((l, _))) if l <= 0.0 => 1,
                (false, Some(_)) => 0,
            };
            Ranked {
                entry: e,
                left: st.map(|s| s.0),
                reset: st.and_then(|s| s.1),
                bucket,
            }
        })
        .collect();
    let best = rows
        .iter()
        .filter(|r| r.bucket == 0)
        .filter_map(|r| r.left)
        .fold(f64::NAN, f64::max);
    let in_band = |r: &Ranked| {
        strategy == Strategy::SoonestReset && r.left.is_some_and(|l| l >= best - TIE_BAND)
    };
    rows.sort_by(|a, b| {
        a.bucket.cmp(&b.bucket).then_with(|| {
            let (la, lb) = (a.left.unwrap_or(-1.0), b.left.unwrap_or(-1.0));
            let by_left_then_reset = more(la, lb).then(sooner(a.reset, b.reset));
            let by_reset_then_left = sooner(a.reset, b.reset).then(more(la, lb));
            let ord = match a.bucket {
                0 => match (in_band(a), in_band(b)) {
                    (true, true) => by_reset_then_left,
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => by_left_then_reset,
                },
                1 => sooner(a.reset, b.reset),
                3 => more(la, lb),
                _ => std::cmp::Ordering::Equal,
            };
            ord.then(a.entry.n.cmp(&b.entry.n))
                .then(a.entry.index.cmp(&b.entry.index))
        })
    });
    rows
}

/// The account `next` switches to: the best of bucket 0, leaving the
/// preferred one out.
pub fn next<'a>(
    list: &'a [Entry],
    provider: &str,
    strategy: Strategy,
    now: i64,
) -> Option<Ranked<'a>> {
    let skip = preferred(list, provider).map(|e| e.index.as_str());
    rank(list, provider, strategy, now, skip)
        .into_iter()
        .find(|r| r.bucket == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn w(id: &str, used: f64, resets_in: i64) -> Win {
        Win {
            id: id.into(),
            used,
            resets_at: if resets_in == 0 { 0 } else { NOW + resets_in },
            period_s: 0,
        }
    }

    fn acct(n: usize, prio: i64, wins: Vec<Win>) -> Entry {
        Entry {
            n,
            index: format!("ix{n}"),
            name: format!("a{n}.json"),
            provider: "claude".into(),
            label: format!("a{n}"),
            priority: prio,
            disabled: false,
            unavailable: false,
            signals: Default::default(),
            windows: wins,
        }
    }

    fn order(list: &[Entry], s: Strategy, ex: Option<&str>) -> Vec<String> {
        rank(list, "claude", s, NOW, ex)
            .into_iter()
            .map(|r| r.entry.label.clone())
            .collect()
    }

    #[test]
    fn left_is_the_tightest_window_and_a_passed_reset_is_unused() {
        let e = acct(1, 0, vec![w("5h", 20.0, 100), w("7d", 70.0, 5000)]);
        assert_eq!(standing(&e, NOW), Some((30.0, Some(NOW + 5000))));
        let e = acct(1, 0, vec![w("5h", 99.0, -10), w("7d", 40.0, 5000)]);
        assert_eq!(standing(&e, NOW), Some((60.0, Some(NOW + 5000))));
        assert_eq!(standing(&acct(1, 0, vec![]), NOW), None);
        let over = acct(1, 0, vec![w("5h", 130.0, 5)]);
        assert_eq!(standing(&over, NOW).unwrap().0, 0.0);
    }

    #[test]
    fn most_left_orders_by_left_then_sooner_reset_then_position() {
        let l = [
            acct(1, 0, vec![w("5h", 50.0, 100)]),
            acct(2, 0, vec![w("5h", 10.0, 900)]),
            acct(3, 0, vec![w("5h", 10.0, 300)]),
            acct(4, 0, vec![w("5h", 50.0, 100)]),
        ];
        assert_eq!(
            order(&l, Strategy::MostLeft, None),
            ["a3", "a2", "a1", "a4"]
        );
    }

    #[test]
    fn soonest_reset_prefers_expiring_quota_inside_the_tie_band() {
        let l = [
            acct(1, 0, vec![w("5h", 10.0, 9000)]), // 90 left, resets late
            acct(2, 0, vec![w("5h", 18.0, 100)]),  // 82 left, resets soon: in band
            acct(3, 0, vec![w("5h", 30.0, 10)]),   // 70 left: outside the band
        ];
        assert_eq!(order(&l, Strategy::SoonestReset, None), ["a2", "a1", "a3"]);
        assert_eq!(order(&l, Strategy::MostLeft, None), ["a1", "a2", "a3"]);
        // the band's edge is inclusive: exactly TIE_BAND below the best
        let l = [
            acct(1, 0, vec![w("5h", 10.0, 9000)]),
            acct(2, 0, vec![w("5h", 20.0, 100)]),
        ];
        assert_eq!(order(&l, Strategy::SoonestReset, None), ["a2", "a1"]);
    }

    #[test]
    fn exhausted_unread_and_cooling_accounts_come_last_in_that_order() {
        let mut cool = acct(5, 0, vec![w("5h", 5.0, 50)]);
        cool.unavailable = true;
        let l = [
            acct(1, 0, vec![]),                   // unread
            acct(2, 0, vec![w("5h", 100.0, 40)]), // exhausted
            cool,                                 // cooling
            acct(4, 0, vec![w("5h", 95.0, 10)]),  // 5 left
        ];
        for s in [Strategy::MostLeft, Strategy::SoonestReset] {
            assert_eq!(order(&l, s, None), ["a4", "a2", "a1", "a5"]);
        }
    }

    #[test]
    fn disabled_accounts_and_other_providers_are_not_ranked() {
        let mut off = acct(1, 0, vec![w("5h", 0.0, 1)]);
        off.disabled = true;
        let mut codex = acct(2, 0, vec![w("5h", 0.0, 1)]);
        codex.provider = "codex".into();
        let l = [off, codex, acct(3, 0, vec![w("5h", 50.0, 1)])];
        assert_eq!(order(&l, Strategy::MostLeft, None), ["a3"]);
    }

    #[test]
    fn next_skips_the_preferred_account_and_needs_data() {
        let l = [
            acct(1, 5, vec![w("5h", 0.0, 100)]), // preferred, most left
            acct(2, 0, vec![w("5h", 40.0, 100)]),
            acct(3, 0, vec![w("5h", 20.0, 100)]),
        ];
        assert_eq!(preferred(&l, "claude").unwrap().label, "a1");
        assert_eq!(
            next(&l, "claude", Strategy::MostLeft, NOW)
                .unwrap()
                .entry
                .label,
            "a3"
        );
        // a tie at the top: nobody is preferred, so the best overall wins
        let l = [
            acct(1, 1, vec![w("5h", 50.0, 100)]),
            acct(2, 1, vec![w("5h", 10.0, 100)]),
        ];
        assert!(preferred(&l, "claude").is_none());
        assert_eq!(
            next(&l, "claude", Strategy::MostLeft, NOW)
                .unwrap()
                .entry
                .label,
            "a2"
        );
        // nothing with quota left besides the preferred one
        let l = [
            acct(1, 5, vec![w("5h", 10.0, 100)]),
            acct(2, 0, vec![w("5h", 100.0, 100)]),
            acct(3, 0, vec![]),
        ];
        assert!(next(&l, "claude", Strategy::MostLeft, NOW).is_none());
        assert!(next(&[], "claude", Strategy::MostLeft, NOW).is_none());
    }

    #[test]
    fn strategies_parse() {
        assert_eq!(Strategy::parse("most-left"), Some(Strategy::MostLeft));
        assert_eq!(
            Strategy::parse("soonest-reset"),
            Some(Strategy::SoonestReset)
        );
        assert_eq!(Strategy::parse("x"), None);
    }
}
