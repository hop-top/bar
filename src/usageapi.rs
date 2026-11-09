//! Opt-in usage API (`set usage-api on`): a background job asks
//! Anthropic's OAuth usage endpoint, with Claude Code's own sign-in, for
//! what the statusline JSON lacks: the per-model weekly windows and the
//! extra-usage (overage) spend. Only the derived numbers are cached.

pub mod creds;
pub mod sha256;

use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::cache::{Cache, Job};
use crate::secret::Secret;
use crate::session::Window;
use crate::tz::{self, Tz};

pub const URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Headers besides the token: the OAuth beta the endpoint requires.
pub const HEADERS: [&str; 2] = [
    "anthropic-beta: oauth-2025-04-20",
    "Accept: application/json",
];
/// A result stays fresh this long, seconds.
pub const TTL: u64 = 300;
/// Wait after a 429 without a usable `Retry-After`, seconds.
pub const RATE_LIMIT_WAIT: i64 = 600;
/// Longest wait a `Retry-After` can ask for, seconds.
const MAX_WAIT: i64 = 86_400;
/// Wait after the token is refused (401, 403), seconds.
const AUTH_WAIT: i64 = 900;
/// Wait after a keychain read timed out (an unanswered prompt), seconds.
const BLOCKED_WAIT: i64 = 3600;
/// Wait after the first failed fetch, doubling with each one after it.
const FAIL_WAIT: i64 = 300;
/// Longest wait after failures, seconds.
const MAX_FAIL_WAIT: i64 = 3600;

/// Wait after `fails` failed fetches in a row (at least 1).
pub fn fail_wait(fails: u32) -> i64 {
    let doublings = fails.saturating_sub(1).min(12);
    (FAIL_WAIT << doublings).min(MAX_FAIL_WAIT)
}
/// Older results are not shown, seconds.
pub const STALE: i64 = 3600;
/// Far larger than the endpoint's JSON.
const MAX_BYTES: usize = 256 * 1024;

/// A weekly window of one model family.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Lowercase name: `opus`, `sonnet`, or the endpoint's display name.
    pub name: String,
    pub used: f64,
    pub resets_at: i64,
}

/// Extra usage for the month; amounts in minor units of `currency`.
#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    pub enabled: bool,
    pub limit: i64,
    pub used: i64,
    /// Used percent of the limit.
    pub util: f64,
    pub currency: String,
}

/// What the cache holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Data {
    /// When the numbers were fetched, unix seconds; 0 for none.
    pub at: i64,
    pub models: Vec<Model>,
    pub extra: Option<Extra>,
    /// Local day number of `base`.
    pub day: i64,
    /// Extra usage used when the day began, as far as seen; `None` until
    /// a fetch on the day before was seen.
    pub base: Option<i64>,
    /// No request before this time (rate limit, refused token, failures).
    pub retry_at: i64,
    /// Failed fetches in a row (server errors, curl failures, bad JSON).
    pub fails: u32,
}

impl Data {
    pub fn to_json(&self) -> String {
        let models: Vec<Value> = self
            .models
            .iter()
            .map(|m| json!({"name": m.name, "used": m.used, "resets_at": m.resets_at}))
            .collect();
        let extra = self.extra.as_ref().map(|e| {
            json!({"enabled": e.enabled, "limit": e.limit, "used": e.used,
                   "util": e.util, "currency": e.currency})
        });
        json!({"v": 1, "at": self.at, "models": models, "extra": extra,
               "day": self.day, "base": self.base, "retry_at": self.retry_at,
               "fails": self.fails})
        .to_string()
    }

    pub fn from_json(s: &str) -> Option<Data> {
        let v: Value = serde_json::from_str(s).ok()?;
        (v["v"] == 1).then_some(())?;
        let models = v["models"]
            .as_array()?
            .iter()
            .filter_map(|m| {
                Some(Model {
                    name: m["name"].as_str()?.into(),
                    used: m["used"].as_f64()?,
                    resets_at: m["resets_at"].as_i64()?,
                })
            })
            .collect();
        let extra = v["extra"].as_object().and_then(|e| {
            Some(Extra {
                enabled: e.get("enabled")?.as_bool()?,
                limit: e.get("limit")?.as_i64()?,
                used: e.get("used")?.as_i64()?,
                util: e.get("util")?.as_f64()?,
                currency: e.get("currency")?.as_str()?.into(),
            })
        });
        Some(Data {
            at: v["at"].as_i64().unwrap_or(0),
            models,
            extra,
            day: v["day"].as_i64().unwrap_or(0),
            base: v["base"].as_i64(),
            retry_at: v["retry_at"].as_i64().unwrap_or(0),
            fails: v["fails"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
        })
    }

    /// The weekly window of the model family `name` (case-insensitive,
    /// a substring of the endpoint's name), unless the numbers are stale.
    pub fn model(&self, name: &str, now: i64) -> Option<Window> {
        if now - self.at > STALE || name.is_empty() {
            return None;
        }
        let want = name.to_ascii_lowercase();
        let m = self.models.iter().find(|m| m.name.contains(&want))?;
        Some(Window {
            used_percentage: m.used,
            resets_at: m.resets_at,
        })
    }

    /// Extra usage and what of it was spent today (`None` while unknown),
    /// unless the numbers are stale.
    pub fn extra(&self, now: i64, tz: Tz) -> Option<(&Extra, Option<i64>)> {
        if now - self.at > STALE {
            return None;
        }
        let e = self.extra.as_ref()?;
        let today = self
            .base
            .filter(|_| self.day == tz::day_of(now, tz))
            .map(|b| (e.used - b).max(0));
        Some((e, today))
    }
}

/// `2026-11-09T12:00:00.123456+00:00`, `...Z`, or unix seconds.
fn timestamp(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    tz::rfc3339(v.as_str()?)
}

/// A model name as kept: lowercase letters, digits, spaces, `.` and `-`.
fn model_name(s: &str) -> Option<String> {
    let n: String = s
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " .-".contains(*c))
        .take(32)
        .collect();
    (!n.is_empty()).then_some(n)
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}

/// Per-model windows and extra usage from the endpoint's JSON. Model
/// windows come from `limits` items of kind `weekly_scoped` (named by
/// `scope.model.display_name`), else the `seven_day_opus` and
/// `seven_day_sonnet` buckets; a bucket without utilization or reset
/// time is left out.
pub fn parse(body: &[u8]) -> Option<(Vec<Model>, Option<Extra>)> {
    let v: Value = serde_json::from_slice(body).ok()?;
    v.is_object().then_some(())?;
    let mut models: Vec<Model> = Vec::new();
    for l in v["limits"].as_array().into_iter().flatten() {
        if l["kind"] != "weekly_scoped" {
            continue;
        }
        let Some(name) = l["scope"]["model"]["display_name"]
            .as_str()
            .and_then(model_name)
        else {
            continue;
        };
        let (Some(used), Some(resets_at)) = (finite(&l["percent"]), timestamp(&l["resets_at"]))
        else {
            continue;
        };
        if !models.iter().any(|m| m.name == name) {
            models.push(Model {
                name,
                used,
                resets_at,
            });
        }
    }
    for (key, name) in [("seven_day_opus", "opus"), ("seven_day_sonnet", "sonnet")] {
        let b = &v[key];
        let (Some(used), Some(resets_at)) = (finite(&b["utilization"]), timestamp(&b["resets_at"]))
        else {
            continue;
        };
        if !models.iter().any(|m| m.name.contains(name)) {
            models.push(Model {
                name: name.into(),
                used,
                resets_at,
            });
        }
    }
    let e = &v["extra_usage"];
    let extra = (|| {
        let limit = finite(&e["monthly_limit"])?.round() as i64;
        let used = finite(&e["used_credits"]).unwrap_or(0.0).round() as i64;
        let util = finite(&e["utilization"]).unwrap_or_else(|| {
            if limit > 0 {
                used as f64 * 100.0 / limit as f64
            } else {
                0.0
            }
        });
        let currency = e["currency"]
            .as_str()
            .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()))
            .unwrap_or("USD");
        Some(Extra {
            enabled: e["is_enabled"].as_bool().unwrap_or(false),
            limit,
            used,
            util,
            currency: currency.into(),
        })
    })();
    Some((models, extra))
}

/// `fresh` fetched at `now`, with the day's baseline carried over from
/// `prev`: the same day keeps its baseline; the first fetch of a day
/// starts it from the last value seen the day before; otherwise it is
/// unknown. A lower value than the baseline (a new month) restarts at 0.
pub fn merge(prev: Option<&Data>, mut fresh: Data, now: i64, tz: Tz) -> Data {
    let day = tz::day_of(now, tz);
    fresh.at = now;
    fresh.day = day;
    let used = fresh.extra.as_ref().map(|e| e.used);
    let prev_used = prev.and_then(|p| p.extra.as_ref().map(|e| e.used));
    fresh.base = match (prev, used) {
        (_, None) => None,
        (Some(p), Some(u)) if p.day == day => p.base.map(|b| if u < b { 0 } else { b }),
        (Some(p), Some(u)) if p.day == day - 1 => prev_used.map(|b| if u < b { 0 } else { b }),
        _ => None,
    };
    fresh
}

/// The curl config line carrying the token, in a buffer wiped on drop.
fn curl_config(token: &Secret) -> Secret {
    const HEAD: &[u8] = b"header = \"Authorization: Bearer ";
    const TAIL: &[u8] = b"\"\n";
    let mut c = Secret::with_capacity(HEAD.len() + token.as_bytes().len() + TAIL.len());
    c.push(HEAD);
    c.push(token.as_bytes());
    c.push(TAIL);
    c
}

/// Body of the background job: the new cache value, or `None` (no token)
/// to keep the old one and retry after the cache's 60 s backoff. `prev` is
/// the cache file of the last value. A 429, a refused token or a failed
/// fetch keeps the last numbers and sets `retry_at`; failures in a row
/// wait 5 minutes, doubling up to an hour.
pub fn fetch(dir: &Path, hashed: bool, prev: &Path, now: i64, limit: Duration) -> Option<String> {
    let old = std::fs::read_to_string(prev)
        .ok()
        .and_then(|s| Data::from_json(&s));
    if old.as_ref().is_some_and(|o| o.retry_at > now) {
        return None;
    }
    let wait = |secs: i64, failed: bool| {
        let mut d = old.clone().unwrap_or_default();
        d.retry_at = now + secs;
        if failed {
            d.fails = d.fails.saturating_add(1);
            d.retry_at = now + fail_wait(d.fails);
        }
        Some(d.to_json())
    };
    let failed = || wait(0, true);
    // the keychain read and the request share `limit`
    let end = Instant::now() + limit;
    let token = match creds::access_token(dir, hashed, now.saturating_mul(1000), limit) {
        Ok(t) => t,
        Err(creds::NoToken::Missing) => return None,
        Err(creds::NoToken::Blocked) => return wait(BLOCKED_WAIT, false),
    };
    let config = curl_config(&token);
    drop(token);
    let left = end.saturating_duration_since(Instant::now()).as_secs();
    if left == 0 {
        return None;
    }
    let r = crate::net::request(URL, &HEADERS, config.as_bytes(), left, MAX_BYTES);
    drop(config);
    let Ok(r) = r else {
        return failed();
    };
    match r.status {
        200 => {
            let Some((models, extra)) = parse(&r.body) else {
                return failed();
            };
            let fresh = Data {
                models,
                extra,
                ..Data::default()
            };
            Some(merge(old.as_ref(), fresh, now, Tz::Local).to_json())
        }
        429 => wait(
            r.retry_after
                .and_then(|s| i64::try_from(s).ok())
                .filter(|s| *s > 0)
                .map_or(RATE_LIMIT_WAIT, |s| s.min(MAX_WAIT)),
            false,
        ),
        401 | 403 => wait(AUTH_WAIT, false),
        _ => failed(),
    }
}

/// The Claude Code config dir and whether `CLAUDE_CONFIG_DIR` names it.
pub fn config_dir() -> Option<(String, bool)> {
    let hashed = std::env::var_os("CLAUDE_CONFIG_DIR").is_some_and(|v| !v.is_empty());
    let d = crate::click::claude_dir()?;
    Some((d.to_string_lossy().into_owned(), hashed))
}

/// Cache key of the result for one Claude Code config dir.
pub fn cache_key(dir: &str) -> String {
    format!("usage-api:{dir}")
}

/// The cached numbers; starts the background fetch when they are older
/// than `TTL`, unless a rate limit or refused token says to wait.
/// Never waits on the network.
pub fn load(cache: &Cache, now: i64) -> Option<Data> {
    let (dir, hashed) = config_dir()?;
    let key = cache_key(&dir);
    let stored = cache
        .read_fresh(&key, u64::MAX)
        .and_then(|s| Data::from_json(&s));
    if stored.as_ref().is_some_and(|d| d.retry_at > now) {
        return stored;
    }
    let job = Job::UsageApi {
        dir,
        hashed,
        prev: cache.path_for(&key).to_string_lossy().into_owned(),
    };
    cache
        .read_or_refresh(&key, TTL, &job)
        .and_then(|s| Data::from_json(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        let z = timestamp(&json!("2026-11-09T12:00:00Z")).unwrap();
        assert_eq!(
            timestamp(&json!("2026-11-09T12:00:00.123456+00:00")),
            Some(z)
        );
        assert_eq!(timestamp(&json!("2026-11-09T14:00:00+02:00")), Some(z));
        assert_eq!(timestamp(&json!("2026-11-09T10:30:00-01:30")), Some(z));
        assert_eq!(timestamp(&json!(z)), Some(z));
        assert_eq!(timestamp(&json!("2026-11-09T12:00:00 junk")), None);
        assert_eq!(timestamp(&json!(null)), None);
    }

    #[test]
    fn round_trip() {
        let d = Data {
            at: 5,
            models: vec![Model {
                name: "opus".into(),
                used: 12.5,
                resets_at: 99,
            }],
            extra: Some(Extra {
                enabled: true,
                limit: 5000,
                used: 1234,
                util: 24.68,
                currency: "USD".into(),
            }),
            day: 3,
            base: Some(1000),
            retry_at: 0,
            fails: 2,
        };
        assert_eq!(Data::from_json(&d.to_json()), Some(d));
        assert_eq!(Data::from_json("{}"), None);
    }

    fn extra(used: i64) -> Option<Extra> {
        Some(Extra {
            enabled: true,
            limit: 5000,
            used,
            util: 0.0,
            currency: "USD".into(),
        })
    }

    #[test]
    fn daily_baseline() {
        let day = 86_400;
        let fresh = |used| Data {
            extra: extra(used),
            ..Data::default()
        };
        // first sight: unknown
        let a = merge(None, fresh(100), 10 * day + 60, Tz::Utc);
        assert_eq!((a.day, a.base), (10, None));
        // same day: still unknown
        let b = merge(Some(&a), fresh(150), 10 * day + 120, Tz::Utc);
        assert_eq!(b.base, None);
        // next day: the last value of the day before
        let c = merge(Some(&b), fresh(180), 11 * day + 60, Tz::Utc);
        assert_eq!((c.day, c.base), (11, Some(150)));
        assert_eq!(c.extra(11 * day + 60, Tz::Utc).unwrap().1, Some(30));
        // same day keeps it
        let d = merge(Some(&c), fresh(400), 11 * day + 600, Tz::Utc);
        assert_eq!(d.base, Some(150));
        // lower than the baseline: a new month
        let e = merge(Some(&d), fresh(20), 11 * day + 900, Tz::Utc);
        assert_eq!(e.base, Some(0));
        // a gap of days: unknown again
        let f = merge(Some(&e), fresh(30), 14 * day, Tz::Utc);
        assert_eq!(f.base, None);
        // shown for its own day only
        let later = Data { at: 12 * day, ..d };
        assert_eq!(later.extra(12 * day, Tz::Utc).unwrap().1, None);
    }

    #[test]
    fn stale_numbers_are_hidden() {
        let d = Data {
            at: 1000,
            models: vec![Model {
                name: "claude opus".into(),
                used: 40.0,
                resets_at: 9000,
            }],
            extra: extra(10),
            ..Data::default()
        };
        assert_eq!(d.model("Opus", 1000).unwrap().used_percentage, 40.0);
        assert!(d.model("sonnet", 1000).is_none());
        assert!(d.model("opus", 1000 + STALE + 1).is_none());
        assert!(d.extra(1000 + STALE + 1, Tz::Utc).is_none());
    }
}
