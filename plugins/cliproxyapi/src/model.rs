//! Accounts and usage windows, and the parsers that turn what the proxy
//! reports into them. Every window is `{id, used percent, reset time,
//! period}`; percent is always *used* (remaining is derived at render).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Passive rate-limit snapshot of an account: lower-cased header name to
/// value.
pub type Signals = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Win {
    /// `5h`, `7d`, `wk` or `p<hours>h`.
    pub id: String,
    /// Percent used, 0 to 100.
    pub used: f64,
    /// Reset time, Unix seconds; 0 when unknown.
    pub resets_at: i64,
    /// Window length in seconds; 0 when unknown.
    pub period_s: i64,
}

/// One proxy account. Holds the label shown and the proxy's opaque
/// `auth_index`; never an email, path, token or account id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acct {
    pub key: String,
    pub provider: String,
    pub label: String,
    /// The email's local part, for `label=local`; empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub local: String,
    /// The proxy's routing priority (unset is 0); the highest available
    /// tier serves requests.
    #[serde(default)]
    pub priority: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default)]
    pub windows: Vec<Win>,
    /// When `windows` were read, Unix seconds.
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub unavailable: bool,
    /// The upstream refused this account's own token.
    #[serde(default)]
    pub upstream_auth: bool,
    /// No upstream call for this account before this time.
    #[serde(default)]
    pub retry_at: i64,
}

/// Window id for a period in seconds.
pub fn window_id(period_s: i64) -> String {
    match period_s {
        18_000 => "5h".into(),
        604_800 => "7d".into(),
        s if (6 * 86_400..=8 * 86_400).contains(&s) => "wk".into(),
        s => format!("p{}h", ((s + 1800) / 3600).max(1)),
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

/// Percent clamped to 0..=100.
fn pct(p: f64) -> f64 {
    p.clamp(0.0, 100.0)
}

/// Claude's windows from the passive `Anthropic-Ratelimit-Unified-*`
/// snapshot: utilisation is a 0 to 1 fraction used. A window the snapshot
/// lacks (the 5h one often is) is omitted, never shown as 0%.
pub fn claude_windows(s: &Signals) -> Vec<Win> {
    let mut out = Vec::new();
    for (id, period) in [("5h", 18_000), ("7d", 604_800)] {
        let get = |k: &str| s.get(&format!("anthropic-ratelimit-unified-{id}-{k}"));
        let Some(u) = get("utilization").and_then(|v| v.trim().parse::<f64>().ok()) else {
            continue;
        };
        if !u.is_finite() {
            continue;
        }
        let reset = get("reset")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map_or(0, |r| r as i64);
        out.push(Win {
            id: id.into(),
            used: pct(u * 100.0),
            resets_at: reset,
            period_s: period,
        });
    }
    out
}

fn codex_window(w: &Value, now: i64) -> Option<Win> {
    let used = num(w.get("used_percent")?)?;
    let period = w
        .get("limit_window_seconds")
        .and_then(num)
        .map_or(0, |p| p as i64);
    let resets = match w.get("reset_at").and_then(num) {
        Some(r) => r as i64,
        None => w
            .get("reset_after_seconds")
            .and_then(num)
            .map_or(0, |a| now + a as i64),
    };
    Some(Win {
        id: if period > 0 {
            window_id(period)
        } else {
            "wk".into()
        },
        used: pct(used),
        resets_at: resets,
        period_s: period,
    })
}

/// Codex windows and plan from a `wham/usage` body. Either window may be
/// null (a pro plan has no 5h one); a missing percent omits the window.
pub fn codex_windows(body: &Value, now: i64) -> (Vec<Win>, Option<String>) {
    let rl = &body["rate_limit"];
    let wins = ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|k| codex_window(&rl[*k], now))
        .collect();
    let plan = body["plan_type"].as_str().map(plan_name);
    (wins, plan)
}

/// Codex windows from the passive `X-Codex-*` snapshot (the fallback when
/// the upstream call fails).
pub fn codex_signal_windows(s: &Signals, now: i64) -> Vec<Win> {
    let mut out = Vec::new();
    for which in ["primary", "secondary"] {
        let get = |k: &str| s.get(&format!("x-codex-{which}-{k}"));
        let Some(used) = get("used-percent").and_then(|v| v.trim().parse::<f64>().ok()) else {
            continue;
        };
        let period = get("window-minutes")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map_or(0, |m| m * 60);
        let resets = get("reset-at")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .or_else(|| {
                get("reset-after-seconds")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .map(|a| now + a)
            })
            .unwrap_or(0);
        out.push(Win {
            id: if period > 0 {
                window_id(period)
            } else {
                "wk".into()
            },
            used: pct(used),
            resets_at: resets,
            period_s: period,
        });
    }
    out
}

/// A plan name safe to draw: letters, digits, `-` and `_`, 16 at most.
pub fn plan_name(p: &str) -> String {
    p.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(16)
        .collect()
}

/// The provider whose quota applies to Claude Code's model: `claude`,
/// `opus`, `sonnet`, `haiku` mean claude; `gpt`, `codex`, `o<digit>` mean
/// codex. A proxy prefix (`prefix/model`) is ignored. `None`: not a
/// provider this plugin reads.
pub fn provider_of_model(model_id: &str, model: &str) -> Option<&'static str> {
    for m in [model_id, model] {
        let m = m.rsplit('/').next().unwrap_or(m).to_ascii_lowercase();
        if m.is_empty() {
            continue;
        }
        let b = m.as_bytes();
        if ["claude", "opus", "sonnet", "haiku"]
            .iter()
            .any(|k| m.contains(k))
        {
            return Some("claude");
        }
        if m.contains("gpt")
            || m.contains("codex")
            || (b[0] == b'o' && b.get(1).is_some_and(u8::is_ascii_digit))
        {
            return Some("codex");
        }
    }
    None
}

/// Case-insensitive glob with `*` and `?`.
pub fn glob(pat: &str, s: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => (0..=s.len()).any(|i| go(rest, &s[i..])),
            Some((b'?', rest)) => !s.is_empty() && go(rest, &s[1..]),
            Some((c, rest)) => s.first().is_some_and(|x| x == c) && go(rest, &s[1..]),
        }
    }
    go(
        pat.to_ascii_lowercase().as_bytes(),
        s.to_ascii_lowercase().as_bytes(),
    )
}

/// The label drawn for an account: the first alias whose glob matches the
/// email, else the part of the email before `@`, else `#<n>`. The email
/// itself is used here and nowhere stored.
pub fn label_for(email: &str, aliases: &[(String, String)], n: usize) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || "._-+".contains(*c))
            .take(24)
            .collect()
    };
    if let Some((_, l)) = aliases.iter().find(|(g, _)| glob(g, email)) {
        let l = bar_plugin::proto::sanitize(l);
        if !l.is_empty() {
            return l.chars().take(24).collect();
        }
    }
    let local = clean(email.split('@').next().unwrap_or(""));
    if local.is_empty() {
        format!("#{n}")
    } else {
        local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sig(pairs: &[(&str, &str)]) -> Signals {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn window_ids_follow_the_period() {
        assert_eq!(window_id(18_000), "5h");
        assert_eq!(window_id(604_800), "7d");
        assert_eq!(window_id(7 * 86_400 + 3600), "wk");
        assert_eq!(window_id(3 * 3600), "p3h");
        assert_eq!(window_id(10), "p1h");
    }

    #[test]
    fn claude_windows_come_from_signals() {
        let s = sig(&[
            ("anthropic-ratelimit-unified-7d-utilization", "0.125"),
            ("anthropic-ratelimit-unified-7d-reset", "1800400000"),
            ("anthropic-ratelimit-unified-status", "allowed"),
        ]);
        let w = claude_windows(&s);
        assert_eq!(w.len(), 1, "5h absent is omitted, not 0%");
        assert_eq!(
            (w[0].id.as_str(), w[0].used, w[0].resets_at),
            ("7d", 12.5, 1_800_400_000)
        );
        let s = sig(&[
            ("anthropic-ratelimit-unified-5h-utilization", "1.7"),
            ("anthropic-ratelimit-unified-7d-utilization", "nope"),
        ]);
        let w = claude_windows(&s);
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0].id.as_str(), w[0].used, w[0].resets_at),
            ("5h", 100.0, 0)
        );
        assert!(claude_windows(&Signals::new()).is_empty());
    }

    #[test]
    fn codex_windows_tolerate_nulls_and_strings() {
        let b = json!({"plan_type":"pro","rate_limit":{"allowed":true,
            "primary_window":{"used_percent":42,"limit_window_seconds":604800,"reset_at":1800000000,"reset_after_seconds":5},
            "secondary_window":null}});
        let (w, plan) = codex_windows(&b, 100);
        assert_eq!(plan.as_deref(), Some("pro"));
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0].id.as_str(), w[0].used, w[0].resets_at),
            ("7d", 42.0, 1_800_000_000)
        );
        let b = json!({"rate_limit":{"primary_window":{"used_percent":"7.5","limit_window_seconds":18000,"reset_after_seconds":60},
            "secondary_window":{"used_percent":null,"limit_window_seconds":604800}}});
        let (w, plan) = codex_windows(&b, 100);
        assert!(plan.is_none());
        assert_eq!(w.len(), 1, "no percent: omitted");
        assert_eq!((w[0].id.as_str(), w[0].resets_at), ("5h", 160));
        assert!(codex_windows(&json!({}), 0).0.is_empty());
        assert!(codex_windows(&json!({"rate_limit":null}), 0).0.is_empty());
    }

    #[test]
    fn codex_passive_signals() {
        let s = sig(&[
            ("x-codex-primary-used-percent", "30"),
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-reset-after-seconds", "90"),
            ("x-codex-secondary-used-percent", "10"),
            ("x-codex-secondary-window-minutes", "10080"),
            ("x-codex-secondary-reset-at", "1800000000"),
        ]);
        let w = codex_signal_windows(&s, 10);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].id.as_str(), w[0].resets_at), ("5h", 100));
        assert_eq!((w[1].id.as_str(), w[1].resets_at), ("7d", 1_800_000_000));
    }

    #[test]
    fn model_picks_the_provider() {
        assert_eq!(provider_of_model("claude-opus-4", ""), Some("claude"));
        assert_eq!(provider_of_model("", "Sonnet"), Some("claude"));
        assert_eq!(provider_of_model("team/gpt-5-codex", ""), Some("codex"));
        assert_eq!(provider_of_model("o3-mini", ""), Some("codex"));
        assert_eq!(provider_of_model("grok-4", ""), None);
        assert_eq!(provider_of_model("", ""), None);
        assert_eq!(provider_of_model("opus/grok", ""), None);
    }

    #[test]
    fn labels_never_need_the_email_stored() {
        let al = vec![("*@corp.test".to_string(), "work".to_string())];
        assert_eq!(label_for("Jo@Corp.test", &al, 1), "work");
        assert_eq!(label_for("jo.doe@other.test", &al, 1), "jo.doe");
        assert_eq!(label_for("", &al, 3), "#3");
        assert_eq!(label_for("a\u{1b}b@x", &[], 1), "ab");
        assert!(glob("a*c?", "ABCD") && !glob("a*c?", "abc"));
    }
}
