//! What the plugin shows and prints: the segment's state item, the
//! `--ext-info` declaration, the `status` and `tiers` listings and the
//! proxy-config snippet that `setup` prints. Nothing here carries a key,
//! an api-key entry, a header or an email.

use crate::cache::{Cache, Problem};
use crate::collect::{armed, Snapshot, Status, STALE_AFTER};
use crate::config::{instance_id, Instance};
use bar_plugin::proto::{ActionSpec, ExtInfo, Inputs, OptSpec, Output};
use std::collections::BTreeMap;

pub const NAME: &str = "routellm";

pub fn actions() -> BTreeMap<String, ActionSpec> {
    let a = |d: &str| ActionSpec {
        description: d.into(),
        tokens: vec!["instance".into(), "to".into()],
        confirm: false,
        timeout: 15,
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
    o.insert("show".into(), text("state,alias,tier"));
    o.insert("label".into(), OptSpec::choice(&["alias", "none"], "alias"));
    o.insert("compact".into(), OptSpec::bool());
    o.insert(
        "stale".into(),
        OptSpec::choice(&["mark", "dim", "hide"], "mark"),
    );
    o.insert("link".into(), OptSpec::string());
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
        aliases: vec!["rl".into()],
        web: Some(serde_json::json!({
            "category": "usage",
            "samples": [
                { "output": { "text": "RL on", "style": "good" } },
                { "output": { "text": "RL off", "style": "dim" } },
                { "output": { "text": "RL on \u{2717}", "style": "bad" } }
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

/// The segment: hidden when the instance is not drivable.
pub fn output(s: &Snapshot, inst: &Instance, now: i64, stale_opt: &str) -> Output {
    if s.cache.problem.is_some() || (s.status == Status::Off) {
        return Output::hidden();
    }
    let w = state_word(s);
    let (text, style) = match w {
        "on" => ("RL on", "good"),
        "off" => ("RL off", "dim"),
        "down" => ("RL on \u{2717}", "bad"),
        "key?" => ("RL key?", "bad"),
        _ => ("RL ?", "dim"),
    };
    let old = stale(&s.cache, now) && matches!(w, "on" | "off" | "down");
    if old && stale_opt == "hide" {
        return Output::hidden();
    }
    let mark = if old && stale_opt == "mark" { " ~" } else { "" };
    let style = if old && stale_opt == "dim" {
        "dim"
    } else {
        style
    };
    let mut o = Output::styled(&format!("{text}{mark}"), style);
    o.tokens
        .insert("instance".into(), instance_id(&inst.origin));
    // what a click would do: the opposite of the current state
    o.tokens
        .insert("to".into(), if w == "off" { "on" } else { "off" }.into());
    o
}

fn caps_text(t: &crate::cache::Tier) -> String {
    let mut v = Vec::new();
    if t.vision == Some(true) {
        v.push("vis".to_string());
    }
    if t.tools == Some(true) {
        v.push("tools".into());
    }
    if let Some(c) = t.context {
        v.push(if c >= 1000 {
            format!("{}k", c / 1000)
        } else {
            c.to_string()
        });
    }
    v.join(" ")
}

/// `status`: state, aliases, tiers.
pub fn status_lines(s: &Snapshot, inst: &Instance, now: i64) -> Vec<String> {
    let c = &s.cache;
    let mut out = Vec::new();
    out.push(match (&s.status, c.problem) {
        (Status::AuthStopped(st), _) => {
            format!(
                "state: unknown (management refused the key, {st}; no calls until the key changes)"
            )
        }
        (Status::NoKey, _) => "state: unknown (no key)".into(),
        (Status::Off, _) => "state: unknown (management API off)".into(),
        (_, Some(Problem::Unconfigured)) => {
            format!(
                "state: not configured (no `{}` provider entry; run `bar rl setup`)",
                inst.provider
            )
        }
        (_, Some(Problem::Mismatch)) => format!(
            "state: refused (the `{}` entry does not point at {})",
            inst.provider, inst.routellm
        ),
        _ => {
            let w = state_word(s);
            let age = if c.at > 0 {
                format!(" ({}s ago)", (now - c.at).max(0))
            } else {
                String::new()
            };
            format!(
                "state: {}{age}",
                if w == "down" {
                    "on, RouteLLM not answering"
                } else {
                    w
                }
            )
        }
    });
    if let Some(e) = &c.entry {
        out.push(format!(
            "provider: {} (priority {})",
            inst.provider, e.priority
        ));
        for a in &e.aliases {
            let off = c
                .rows
                .iter()
                .find(|r| r.alias == a.alias)
                .map_or("no row".to_string(), |r| r.model.clone());
            out.push(format!("alias: {} -> tier {}; off: {off}", a.alias, a.tier));
        }
        if !armed(c) {
            out.push("warning: some aliases have no claude row, so OFF fails for them; run `bar rl setup`".into());
        }
    }
    match c.up {
        Some(true) => out.push(format!("routellm: up ({})", inst.routellm)),
        Some(false) => out.push(format!("routellm: not answering ({})", inst.routellm)),
        None => {}
    }
    if let Some(l) = &c.last {
        out.push(format!("last change: {} -> {} at {}", l.from, l.to, l.at));
    }
    out
}

/// `tiers`: one line per tier, aliased ones first.
pub fn tier_lines(s: &Snapshot, inst: &Instance, all: bool) -> Vec<String> {
    let c = &s.cache;
    let mut out = Vec::new();
    for t in &c.tiers {
        let alias = c
            .entry
            .as_ref()
            .and_then(|e| e.aliases.iter().find(|a| a.tier == t.id))
            .map(|a| a.alias.clone());
        if alias.is_none() && (!all || t.kind != "tier") {
            continue;
        }
        let mark = if inst.local_tiers.iter().any(|l| l == &t.id) {
            "local"
        } else {
            "may leave this host"
        };
        out.push(format!(
            "{} {} {} [{}]",
            t.id,
            alias.as_deref().unwrap_or("-"),
            caps_text(t),
            mark
        ));
    }
    if out.is_empty() {
        out.push("no tiers known; run `bar rl status --probe` while routing is on".into());
    }
    out
}

/// The proxy-config snippet `setup` prints; nothing is written.
pub fn setup_snippet(inst: &Instance, c: &Cache) -> Vec<String> {
    let aliases: Vec<(String, String)> = match &c.entry {
        Some(e) if !e.aliases.is_empty() => e
            .aliases
            .iter()
            .map(|a| (a.alias.clone(), a.tier.clone()))
            .collect(),
        _ => ["auto", "vision", "private"]
            .iter()
            .map(|t| (format!("route-{t}"), t.to_string()))
            .collect(),
    };
    let mut o = vec![
        "# Add to the proxy's config (legacy layout). In a v8-layout file the same".into(),
        "# lists sit under api-keys.openai-compatibility and oauth.model-alias.".into(),
        "openai-compatibility:".into(),
        format!("  - name: {}", inst.provider),
        format!("    base-url: {}/v1", inst.routellm),
        format!("    priority: {}", inst.on_priority),
        // without it, a provider that failed is skipped for about a minute
        // after it recovers
        "    disable-cooling: true".into(),
        "    api-key-entries:".into(),
        "      - api-key: <the key RouteLLM expects, if any>".into(),
        "    models:".into(),
    ];
    for (alias, tier) in &aliases {
        o.push(format!("      - {{name: {tier}, alias: {alias}}}"));
    }
    o.push("oauth-model-alias:".into());
    o.push("  claude:".into());
    for (alias, tier) in &aliases {
        // route-private stays without a row: OFF then fails closed
        if tier == "private" {
            continue;
        }
        let model = inst
            .off_map
            .iter()
            .find(|(a, _)| a == alias)
            .map_or("claude-sonnet-5-5", |(_, m)| m.as_str());
        o.push(format!(
            "    - {{name: {model}, alias: {alias}, fork: true}}"
        ));
    }
    o.push("# `fork: true` keeps the original model id available to RouteLLM's own calls.".into());
    o.push("# No claude row for route-private: with routing off, that alias errors instead of leaving this host.".into());
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{Alias, Entry, Row};

    fn inst() -> Instance {
        crate::config::parse(
            "instance http://127.0.0.1:8318\n routellm http://127.0.0.1:6060\n",
            None,
        )
        .instances
        .remove(0)
    }

    fn snap(disabled: bool, up: Option<bool>, at: i64) -> Snapshot {
        Snapshot {
            status: Status::Ok,
            cache: Cache {
                entry: Some(Entry {
                    disabled,
                    priority: 1000,
                    aliases: vec![
                        Alias {
                            alias: "route-auto".into(),
                            tier: "auto".into(),
                        },
                        Alias {
                            alias: "route-private".into(),
                            tier: "private".into(),
                        },
                    ],
                }),
                rows: vec![Row {
                    alias: "route-auto".into(),
                    model: "claude-sonnet-5-5".into(),
                }],
                up,
                at,
                ..Default::default()
            },
        }
    }

    #[test]
    fn the_state_item_says_what_a_click_does() {
        let i = inst();
        let on = output(&snap(false, Some(true), 100), &i, 110, "mark");
        assert_eq!(
            (on.text.as_deref(), on.style.as_deref()),
            (Some("RL on"), Some("good"))
        );
        assert_eq!(on.tokens["to"], "off");
        let off = output(&snap(true, None, 100), &i, 110, "mark");
        assert_eq!(off.text.as_deref(), Some("RL off"));
        assert_eq!(off.tokens["to"], "on");
        let down = output(&snap(false, Some(false), 100), &i, 110, "mark");
        assert_eq!(
            (down.text.as_deref(), down.style.as_deref()),
            (Some("RL on \u{2717}"), Some("bad"))
        );
        let old = output(
            &snap(false, Some(true), 100),
            &i,
            100 + STALE_AFTER + 1,
            "mark",
        );
        assert_eq!(old.text.as_deref(), Some("RL on ~"));
        let hidden = output(&snap(false, Some(true), 100), &i, 1000, "hide");
        assert_eq!(hidden.text.as_deref(), Some(""));
        assert_eq!(on.tokens["instance"], instance_id(&i.origin));
    }

    #[test]
    fn an_undrivable_instance_is_hidden() {
        let mut s = snap(false, Some(true), 100);
        s.cache.problem = Some(Problem::Mismatch);
        assert_eq!(output(&s, &inst(), 101, "mark").text.as_deref(), Some(""));
        let s = Snapshot {
            status: Status::AuthStopped(401),
            cache: Cache::default(),
        };
        assert_eq!(
            output(&s, &inst(), 101, "mark").text.as_deref(),
            Some("RL key?")
        );
    }

    #[test]
    fn setup_prints_a_snippet_without_a_private_row_and_without_secrets() {
        let s = snap(false, Some(true), 1);
        let text = setup_snippet(&inst(), &s.cache).join("\n");
        assert!(text.contains("alias: route-auto, fork: true"), "{text}");
        assert!(text.contains("priority: 1000") && text.contains("disable-cooling: true"));
        assert!(!text.contains("route-private, fork"), "{text}");
        assert!(text.contains("name: private, alias: route-private"));
    }

    #[test]
    fn status_warns_when_the_fallback_is_not_armed() {
        let mut s = snap(false, Some(true), 1);
        s.cache.rows.clear();
        let t = status_lines(&s, &inst(), 2).join("\n");
        assert!(t.contains("no claude row"), "{t}");
        let t = status_lines(&snap(false, Some(true), 1), &inst(), 2).join("\n");
        assert!(!t.contains("warning"), "private needs no row: {t}");
    }
}
