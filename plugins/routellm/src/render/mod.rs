//! What the plugin shows and prints: the segment's state item, the
//! `--ext-info` declaration, the `status` and `tiers` listings and the
//! proxy-config snippet that `setup` prints. Nothing here carries a key,
//! an api-key entry, a header or an email.

use crate::cache::{Cache, Problem};
use crate::collect::{armed, Snapshot, Status, STALE_AFTER};
use crate::config::{instance_id, Instance};
use bar_plugin::proto::{ActionSpec, ExtInfo, Inputs, OptSpec, Output, RenderInput};
use std::collections::BTreeMap;

pub const NAME: &str = "routellm";

pub fn actions() -> BTreeMap<String, ActionSpec> {
    let a = |d: &str| ActionSpec {
        description: d.into(),
        tokens: vec!["instance".into(), "to".into()],
        confirm: false,
        // the toggle's worst case, with a margin (the protocol allows 120 s)
        timeout: crate::toggle::WORST_SECS + 20,
    };
    BTreeMap::from([
        ("toggle".to_string(), a("Switch routing on or off")),
        ("on".to_string(), a("Switch routing on")),
        ("off".to_string(), a("Switch routing off")),
    ])
}

pub fn ext_info() -> ExtInfo {
    let mut o = BTreeMap::new();
    let text = |default: &str| OptSpec {
        default: Some(default.into()),
        ..OptSpec::string()
    };
    o.insert("show".into(), text("alias,tier"));
    o.insert("label".into(), OptSpec::choice(&["alias", "none"], "alias"));
    o.insert("compact".into(), OptSpec::bool());
    o.insert(
        "stale".into(),
        OptSpec::choice(&["mark", "dim", "hide"], "mark"),
    );
    o.insert("link".into(), OptSpec::string());
    o.insert("routed".into(), OptSpec::bool());
    o.insert("base-url".into(), OptSpec::string());
    ExtInfo {
        name: NAME.into(),
        version: crate::VERSION.into(),
        description: "Routing state and toggle of a RouteLLM behind a CLIProxyAPI proxy".into(),
        capabilities: vec!["discover".into(), "placeholder".into(), "check".into()],
        options: o,
        ttl: "15s".into(),
        icon: None,
        // one cache for every session of the proxy
        cache: None,
        inputs: Some(Inputs {
            env: vec!["ANTHROPIC_BASE_URL".into()],
            session: vec!["model_id".into()],
            accounts: false,
        }),
        timeout: Some("8s".into()),
        actions: actions(),
        aliases: vec!["rtellm".into()],
        web: Some(serde_json::json!({
            "category": "usage",
            "samples": [
                // OFF, the alias served by a frontier model
                { "when": { "compact": "true" },
                  "output": { "icon": ICON, "label": LABEL,
                              "text": "auto[claude-sonnet-5-5]", "style": "dim" } },
                // OFF, a private alias with no frontier row
                { "when": { "label": "none" },
                  "output": { "icon": ICON, "label": LABEL,
                              "text": "private[\u{2717}]", "style": "bad" } },
                // ON, RouteLLM up
                { "output": { "icon": ICON, "label": LABEL,
                              "text": "route-auto[auto]", "style": "good" } }
            ]
        })),
    }
}

/// Is the data old enough to be drawn as stale?
pub fn stale(c: &Cache, now: i64) -> bool {
    c.at == 0 || now - c.at > STALE_AFTER
}

/// The one-word state: `on`, `off`, `down` (on, RouteLLM not answering),
/// `key?`, `?`.
pub fn state_word(s: &Snapshot) -> &'static str {
    match s.status {
        Status::AuthStopped(_) | Status::NoKey => "key?",
        Status::Off => "?",
        _ => match (&s.cache.entry, s.cache.problem) {
            (Some(e), None) if e.disabled => "off",
            (Some(_), None) if s.cache.up == Some(false) => "down",
            (Some(_), None) => "on",
            _ => "?",
        },
    }
}

mod item;
mod listing;
mod opts;
mod setup;
#[cfg(test)]
mod tests;

pub use item::{output, ICON, LABEL};
pub use listing::{status_lines, tier_lines};
pub use opts::{alias_of_model, destination, Dest, Opts, Stale};
pub use setup::{setup_snippet, Layout};
