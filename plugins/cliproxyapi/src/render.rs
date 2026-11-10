//! Turning a collection into render output, and the plugin's declaration
//! (`--ext-info`). `items` builds one output per account: the shape the
//! richer options and the account actions extend.

use crate::collect::{Snapshot, Status, STALE_AFTER};
use crate::model::Acct;
use bar_plugin::proto::{ExtInfo, Inputs, OptSpec, Output};

pub const NAME: &str = "cliproxyapi";

/// The `--ext-info` document.
pub fn ext_info() -> ExtInfo {
    let mut options = std::collections::BTreeMap::new();
    options.insert(
        "provider".into(),
        OptSpec::choice(&["auto", "claude", "codex"], "auto"),
    );
    options.insert("base-url".into(), OptSpec::string());
    options.insert("label".into(), OptSpec::choice(&["name", "index"], "name"));
    ExtInfo {
        name: NAME.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Per-account quota of a CLIProxyAPI proxy".into(),
        capabilities: vec!["discover".into(), "placeholder".into(), "check".into()],
        options,
        ttl: "60s".into(),
        icon: None,
        cache: None,
        inputs: Some(Inputs {
            env: vec!["ANTHROPIC_BASE_URL".into()],
            session: vec!["model_id".into(), "model".into()],
        }),
        timeout: Some("15s".into()),
        actions: Default::default(),
        web: Some(serde_json::json!({
            "category": "usage",
            "samples": [{ "output": { "label": "cpa", "text": "work 5h 41% \u{21BB}2h 7d 12% \u{21BB}3d" } }]
        })),
    }
}

/// `2d`, `3h`, `12m`, `<1m` for a number of seconds.
pub fn countdown(secs: i64) -> String {
    match secs {
        s if s < 60 => "<1m".into(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Style for the highest percent used among `a`'s windows.
fn style_of(a: &Acct) -> Option<&'static str> {
    let max = a.windows.iter().map(|w| w.used).fold(f64::NAN, f64::max);
    match max {
        m if m >= 90.0 => Some("bad"),
        m if m >= 70.0 => Some("warn"),
        _ => None,
    }
}

/// One output per account of `provider` (disabled accounts left out).
/// `index_labels` draws `#1`, `#2` instead of names.
pub fn items(s: &Snapshot, provider: &str, index_labels: bool, now: i64) -> Vec<Output> {
    s.accounts
        .iter()
        .filter(|a| a.provider == provider && !a.disabled)
        .enumerate()
        .map(|(i, a)| {
            let stale = a.at > 0 && now - a.at > STALE_AFTER;
            let label = if index_labels {
                format!("#{}", i + 1)
            } else {
                a.label.clone()
            };
            let mut parts: Vec<String> = a
                .windows
                .iter()
                .map(|w| {
                    let mut t = format!("{} {:.0}%", w.id, w.used);
                    if w.resets_at > now {
                        t += &format!(" \u{21BB}{}", countdown(w.resets_at - now));
                    }
                    t
                })
                .collect();
            if parts.is_empty() {
                parts.push("?".into());
            }
            let mut o = Output::text(&format!(
                "{label}{} {}",
                if stale { "~" } else { "" },
                parts.join(" ")
            ));
            o.style = if a.upstream_auth {
                Some("bad".into())
            } else if stale {
                Some("dim".into())
            } else {
                style_of(a).map(String::from)
            };
            o.tokens.insert("account".into(), a.key.clone());
            o.tokens.insert("provider".into(), a.provider.clone());
            o
        })
        .collect()
}

/// The output for one placeholder: the accounts joined as text.
pub fn output(s: &Snapshot, provider: &str, index_labels: bool, now: i64) -> Output {
    match s.status {
        Status::AuthStopped(_) | Status::NoKey => return Output::styled("key?", "bad"),
        Status::Off => return Output::hidden(),
        Status::Ok | Status::Backoff => {}
    }
    let its = items(s, provider, index_labels, now);
    if its.is_empty() {
        return Output::hidden();
    }
    let text = its
        .iter()
        .map(|i| i.text.clone().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" \u{2502} ");
    let mut o = Output::text(&text);
    // one style for the whole text: the worst account's
    o.style = ["bad", "warn", "dim"]
        .iter()
        .find(|st| its.iter().any(|i| i.style.as_deref() == Some(**st)))
        .map(|s| s.to_string());
    o.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Win;

    fn acct(label: &str, used: f64, at: i64) -> Acct {
        Acct {
            key: "idx1".into(),
            provider: "claude".into(),
            label: label.into(),
            plan: None,
            windows: vec![Win {
                id: "7d".into(),
                used,
                resets_at: 1000 + 90_000,
                period_s: 604_800,
            }],
            at,
            disabled: false,
            unavailable: false,
            upstream_auth: false,
            retry_at: 0,
        }
    }

    #[test]
    fn countdowns() {
        assert_eq!(countdown(30), "<1m");
        assert_eq!(countdown(150), "2m");
        assert_eq!(countdown(7200), "2h");
        assert_eq!(countdown(200_000), "2d");
    }

    #[test]
    fn text_marks_stale_and_picks_a_style() {
        let s = Snapshot {
            status: Status::Ok,
            accounts: vec![acct("work", 95.4, 1000), acct("home", 10.0, 0)],
        };
        let o = output(&s, "claude", false, 1000);
        assert_eq!(
            o.text.as_deref(),
            Some("work 7d 95% \u{21BB}1d \u{2502} home~ 7d 10% \u{21BB}1d")
        );
        assert_eq!(o.style.as_deref(), Some("bad"));
        let o = output(&s, "claude", true, 1000);
        assert!(o.text.unwrap().starts_with("#1 "));
        assert!(output(&s, "codex", false, 1000).text.as_deref() == Some(""));
        // never read (no signals yet): unknown, not stale
        let mut idle = acct("idle", 0.0, 0);
        idle.windows.clear();
        let s = Snapshot {
            status: Status::Ok,
            accounts: vec![idle],
        };
        assert_eq!(
            output(&s, "claude", false, 1000).text.as_deref(),
            Some("idle ?")
        );
    }

    #[test]
    fn failures_render_as_the_contract_says() {
        let s = Snapshot {
            status: Status::AuthStopped(401),
            accounts: vec![],
        };
        let o = output(&s, "claude", false, 0);
        assert_eq!(
            (o.text.as_deref(), o.style.as_deref()),
            (Some("key?"), Some("bad"))
        );
        let s = Snapshot {
            status: Status::Off,
            accounts: vec![acct("x", 1.0, 0)],
        };
        assert_eq!(output(&s, "claude", false, 0).text.as_deref(), Some(""));
    }

    #[test]
    fn items_carry_tokens_and_skip_disabled() {
        let mut off = acct("off", 1.0, 0);
        off.disabled = true;
        let s = Snapshot {
            status: Status::Ok,
            accounts: vec![acct("a", 1.0, 5), off],
        };
        let it = items(&s, "claude", false, 5);
        assert_eq!(it.len(), 1);
        assert_eq!(it[0].tokens["account"], "idx1");
        assert_eq!(it[0].tokens["provider"], "claude");
    }
}
