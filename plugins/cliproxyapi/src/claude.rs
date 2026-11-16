//! Claude accounts' usage from two sources: the proxy's passive
//! `Anthropic-Ratelimit-Unified-*` snapshot (the headers of the account's
//! last request through it) and Anthropic's OAuth usage endpoint, read
//! through the proxy's `api-call` with the account's own credential,
//! which the proxy puts in; the plugin never sees it.
//!
//! The passive snapshot carries no time. The plugin keeps the snapshot it
//! last saw: one unchanged since then is as old as when it first
//! appeared, a changed one is new. The newer of the snapshot and the
//! last read is shown, with its own capture time.

use crate::model::{Acct, Win};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A passive snapshot's windows and when they first appeared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub windows: Vec<Win>,
    pub at: i64,
}

/// The usage endpoint, compiled in: `api-call` sends the account's token
/// to any URL it is given, so no configuration may choose one.
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// The beta the endpoint requires of an OAuth token.
pub const BETA: &str = "oauth-2025-04-20";
/// A refused read (401, 403: a token that may serve requests yet not
/// read usage) is not retried for this long.
pub const REFUSED_HOLD: i64 = 3600;

/// The headers of the read: the proxy's token placeholder and the beta.
pub fn headers() -> [(&'static str, &'static str); 2] {
    [
        ("Authorization", "Bearer $TOKEN$"),
        ("anthropic-beta", BETA),
    ]
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

fn reset(v: &Value) -> i64 {
    match v {
        Value::String(s) => rfc3339(s).unwrap_or(0),
        v => num(v).map_or(0, |n| n as i64),
    }
    .max(0)
}

/// The windows of a usage reply: `five_hour` and `seven_day`, each
/// `{utilization: 0..100, resets_at}`. A `null` window is unused, its
/// reset unknown; a missing one is omitted. `None`: not a usage reply.
pub fn oauth_windows(body: &Value) -> Option<Vec<Win>> {
    let m = body.as_object()?;
    let mut out = Vec::new();
    for (k, id, period) in [("five_hour", "5h", 18_000), ("seven_day", "7d", 604_800)] {
        let (used, resets_at) = match m.get(k) {
            None => continue,
            Some(Value::Null) => (0.0, 0),
            Some(w @ Value::Object(_)) => match &w["utilization"] {
                Value::Null => (0.0, reset(&w["resets_at"])),
                u => match num(u) {
                    Some(u) => (u.clamp(0.0, 100.0), reset(&w["resets_at"])),
                    None => continue,
                },
            },
            Some(_) => continue,
        };
        out.push(Win {
            id: id.into(),
            used,
            resets_at,
            period_s: period,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// The passive snapshot `windows` as of `now`, dated: unchanged since
/// `held`, as old as `held`; otherwise new. `None` without a snapshot.
pub fn seen(windows: Vec<Win>, held: Option<&Seen>, now: i64) -> Option<Seen> {
    if windows.is_empty() {
        return None;
    }
    Some(match held {
        Some(h) if h.windows == windows => h.clone(),
        _ => Seen { windows, at: now },
    })
}

/// Folds the passive snapshot `windows` into `a`. With `oauth` it is
/// shown when no older than what `a` holds; without, it is always shown,
/// as current.
pub fn fold_passive(a: &mut Acct, windows: Vec<Win>, oauth: bool, now: i64) {
    let Some(s) = seen(windows, a.passive.as_ref(), now) else {
        return;
    };
    if !oauth {
        a.windows = s.windows.clone();
        a.at = now;
    } else if a.windows.is_empty() || s.at >= a.at {
        a.windows = s.windows.clone();
        a.at = s.at;
    }
    a.passive = Some(s);
}

/// The windows to rank a Claude account by, from the listing's passive
/// `windows` and what the cache holds (`held`, within its keep time):
/// the cache when its data is newer than this snapshot, else the
/// snapshot.
pub fn current(windows: Vec<Win>, held: Option<&Acct>, now: i64) -> Vec<Win> {
    let Some(h) = held.filter(|h| !h.windows.is_empty()) else {
        return windows;
    };
    match seen(windows, h.passive.as_ref(), now) {
        Some(s) if s.at >= h.at => s.windows,
        _ => h.windows.clone(),
    }
}

/// `(year, month, day)` to days since 1970-01-01 (Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// `2027-01-15T10:00:00.123456+00:00` or `...Z` to Unix seconds.
pub fn rfc3339(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let d = b.get(r)?;
        d.iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(d).ok()?.parse().ok())?
    };
    let sep = |i: usize, c: &[u8]| b.get(i).is_some_and(|x| c.contains(x));
    if !(sep(4, b"-") && sep(7, b"-") && sep(10, b"Tt ") && sep(13, b":") && sep(16, b":")) {
        return None;
    }
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    let off = match &b[i..] {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let o = n(i + 1..i + 3)? * 3600 + n(i + 4..i + 6)? * 60;
            if *sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn w(id: &str, used: f64, resets_at: i64) -> Win {
        Win {
            id: id.into(),
            used,
            resets_at,
            period_s: if id == "5h" { 18_000 } else { 604_800 },
        }
    }

    #[test]
    fn rfc3339_times() {
        assert_eq!(rfc3339("2027-01-15T08:00:00Z"), Some(1_800_000_000));
        assert_eq!(rfc3339("2027-01-15T10:00:00.5+02:00"), Some(1_800_000_000));
        assert_eq!(rfc3339("2027-01-15T07:30:00-00:30"), Some(1_800_000_000));
        assert_eq!(rfc3339("1970-01-01T00:00:00Z"), Some(0));
        for bad in [
            "",
            "2027-01-15",
            "2027-01-15T08:00:00",
            "2027-13-01T00:00:00Z",
            "x027-01-15T08:00:00Z",
        ] {
            assert_eq!(rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn usage_replies_are_read_tolerantly() {
        let b = json!({"five_hour":{"utilization":12.5,"resets_at":"2027-01-15T08:00:00Z"},
            "seven_day":{"utilization":"140","resets_at":null},"seven_day_opus":null});
        assert_eq!(
            oauth_windows(&b),
            Some(vec![w("5h", 12.5, 1_800_000_000), w("7d", 100.0, 0)])
        );
        let b = json!({"five_hour":null,"seven_day":{"utilization":null}});
        assert_eq!(
            oauth_windows(&b),
            Some(vec![w("5h", 0.0, 0), w("7d", 0.0, 0)])
        );
        let b = json!({"seven_day":{"utilization":3,"resets_at":1_800_000_000}});
        assert_eq!(oauth_windows(&b), Some(vec![w("7d", 3.0, 1_800_000_000)]));
        for bad in [
            json!(null),
            json!([]),
            json!({"error":"x"}),
            json!({"five_hour":"x"}),
            json!({"five_hour":{"utilization":"nope"}}),
        ] {
            assert_eq!(oauth_windows(&bad), None, "{bad}");
        }
    }

    #[test]
    fn an_unchanged_snapshot_keeps_its_first_sight() {
        let a = seen(vec![w("7d", 5.0, 9)], None, 100).unwrap();
        assert_eq!(a.at, 100);
        assert_eq!(seen(vec![w("7d", 5.0, 9)], Some(&a), 500).unwrap().at, 100);
        assert_eq!(seen(vec![w("7d", 6.0, 9)], Some(&a), 500).unwrap().at, 500);
        assert!(seen(vec![], Some(&a), 500).is_none());
    }

    #[test]
    fn ranking_takes_the_newer_of_the_headers_and_the_cache() {
        let old = vec![w("7d", 5.0, 9)];
        let read = vec![w("5h", 1.0, 0), w("7d", 40.0, 0)];
        let held = Acct {
            key: "ix".into(),
            provider: "claude".into(),
            label: "a".into(),
            color: None,
            priority: 0,
            plan: None,
            windows: read.clone(),
            at: 300,
            disabled: false,
            unavailable: false,
            upstream_auth: false,
            retry_at: 0,
            fails: 0,
            passive: Some(Seen {
                windows: old.clone(),
                at: 100,
            }),
        };
        // the same headers as at 100: the read at 300 is newer
        assert_eq!(current(old.clone(), Some(&held), 400), read);
        // changed headers: new traffic, newer than the read
        let new = vec![w("7d", 6.0, 9)];
        assert_eq!(current(new.clone(), Some(&held), 400), new);
        // no headers: the cache; no cache: the headers
        assert_eq!(current(vec![], Some(&held), 400), read);
        assert_eq!(current(old.clone(), None, 400), old);
    }
}
