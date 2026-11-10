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
    o.insert("show".into(), text("state,alias,tier"));
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
        aliases: vec!["rl".into()],
        web: Some(serde_json::json!({
            "category": "usage",
            "samples": [
                // OFF, the alias served by a frontier model
                { "when": { "compact": "true" },
                  "output": { "text": "RL off auto \u{2192} sonnet-5-5", "style": "dim" } },
                // OFF, a private alias with no frontier row
                { "when": { "label": "none" },
                  "output": { "text": "RL off private \u{2717}", "style": "bad" } },
                // ON, RouteLLM up
                { "output": { "text": "RL on route-auto auto \u{2601}", "style": "good" } }
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

/// The placeholder's options.
#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    pub state: bool,
    pub alias: bool,
    pub tier: bool,
    pub caps: bool,
    /// Draw the alias item (`label=alias`, and not `compact`).
    pub label: bool,
    pub compact: bool,
    pub stale: Stale,
    pub link: Option<String>,
    /// Hide unless the session's model is a `route-*` alias.
    pub routed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stale {
    Mark,
    Dim,
    Hide,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            state: true,
            alias: true,
            tier: true,
            caps: false,
            label: true,
            compact: false,
            stale: Stale::Mark,
            link: None,
            routed: false,
        }
    }
}

impl Opts {
    pub fn from_input(i: &RenderInput) -> Opts {
        let mut o = Opts::default();
        if let Some(list) = i.opt_str("show") {
            let has = |k: &str| list.split(',').any(|p| p.trim().eq_ignore_ascii_case(k));
            // a list naming none of the parts keeps the default
            if ["state", "alias", "tier", "caps"].iter().any(|k| has(k)) {
                (o.state, o.alias, o.tier, o.caps) =
                    (has("state"), has("alias"), has("tier"), has("caps"));
            }
        }
        o.label = i.opt_str("label") != Some("none");
        o.compact = i.opt_bool("compact").unwrap_or(false);
        o.stale = match i.opt_str("stale") {
            Some("dim") => Stale::Dim,
            Some("hide") => Stale::Hide,
            _ => Stale::Mark,
        };
        o.link = i
            .opt_str("link")
            .filter(|l| !l.is_empty())
            .map(String::from);
        o.routed = i.opt_bool("routed").unwrap_or(false);
        o
    }
}

/// `route-auto` for `route-auto` or `team/route-auto`: the alias a session
/// model id names, as the proxy knows it.
pub fn alias_of_model(model_id: &str) -> Option<String> {
    let m = model_id.rsplit('/').next().unwrap_or(model_id);
    (m.len() > 6 && m.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("route-")))
        .then(|| m.to_ascii_lowercase())
}

/// The model an OFF alias serves, without the vendor prefix:
/// `claude-sonnet-5-5` is drawn `sonnet-5-5`.
fn short_model(m: &str) -> &str {
    m.strip_prefix("claude-").unwrap_or(m)
}

/// Where a request of an alias goes in the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// RouteLLM, to this tier.
    Router(String),
    /// A frontier model, through the proxy's own alias row.
    Frontier(String),
    /// Nowhere: the alias has no row and RouteLLM is not serving it.
    Blocked,
    /// Routing is on and RouteLLM is not answering: where the proxy sends
    /// the request then is not known to the plugin.
    Down,
}

/// The destination of `alias` (the proxy's alias of the session's model),
/// from the cached provider entry; `None` when the alias is not one the
/// provider exposes or the state is not known.
pub fn destination(s: &Snapshot, alias: &str) -> Option<Dest> {
    let w = state_word(s);
    if !matches!(w, "on" | "off" | "down") {
        return None;
    }
    let a = s
        .cache
        .entry
        .as_ref()?
        .aliases
        .iter()
        .find(|a| a.alias.eq_ignore_ascii_case(alias))?;
    let row = s
        .cache
        .rows
        .iter()
        .find(|r| r.alias.eq_ignore_ascii_case(alias));
    Some(match (w, row) {
        ("on", _) => Dest::Router(a.tier.clone()),
        ("down", _) => Dest::Down,
        (_, Some(r)) => Dest::Frontier(r.model.clone()),
        _ => Dest::Blocked,
    })
}

fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

fn expand(tpl: &str, inst: &Instance, alias: &str, tier: &str) -> String {
    tpl.replace("{instance}", &escape(&instance_id(&inst.origin)))
        .replace("{alias}", &escape(alias))
        .replace("{tier}", &escape(tier))
}

/// The segment: the state item, then, for a session on a `route-*` alias
/// the provider exposes, the alias, the tier (or where the alias goes while
/// routing is off) and the tier's capabilities. Hidden when the instance is
/// not drivable. Items and the top level carry the toggle's tokens, so a
/// bound `toggle` links every item; an item with a `link` keeps it.
pub fn output(s: &Snapshot, inst: &Instance, now: i64, o: &Opts, model_id: &str) -> Output {
    if s.cache.problem.is_some() || (s.status == Status::Off) {
        return Output::hidden();
    }
    let session_alias = alias_of_model(model_id);
    if o.routed && session_alias.is_none() {
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
    if old && o.stale == Stale::Hide {
        return Output::hidden();
    }
    let dim = old && o.stale == Stale::Dim;
    let mark = if old && o.stale == Stale::Mark {
        " ~"
    } else {
        ""
    };
    let state_style = if dim { "dim" } else { style };
    let tokens: BTreeMap<String, String> = BTreeMap::from([
        ("instance".into(), instance_id(&inst.origin)),
        // what a click would do: the opposite of the current state
        ("to".into(), if w == "off" { "on" } else { "off" }.into()),
    ]);
    let mut items = Vec::new();
    if o.state {
        let mut it = Output::styled(&format!("{text}{mark}"), state_style);
        it.tokens = tokens.clone();
        items.push(it);
    }
    let known = session_alias
        .as_deref()
        .and_then(|a| destination(s, a).map(|d| (a, d)));
    if let Some((alias, dest)) = known {
        let tier = s
            .cache
            .entry
            .as_ref()
            .and_then(|e| {
                e.aliases
                    .iter()
                    .find(|a| a.alias.eq_ignore_ascii_case(alias))
            })
            .map(|a| a.tier.clone())
            .unwrap_or_default();
        let link = |mut it: Output| {
            it.link = o.link.as_deref().map(|t| expand(t, inst, alias, &tier));
            if dim {
                it.style = Some("dim".into());
            }
            it
        };
        if o.alias && o.label && !o.compact {
            items.push(link(Output::text(alias)));
        }
        if o.tier {
            let mut it = match &dest {
                Dest::Router(t) => {
                    let marker = if inst.local_tiers.iter().any(|l| l == t) {
                        "\u{2302}"
                    } else {
                        "\u{2601}"
                    };
                    Output::text(&format!("{t} {marker}"))
                }
                Dest::Frontier(m) => Output::text(&format!("{tier} \u{2192} {}", short_model(m))),
                Dest::Blocked | Dest::Down => Output::styled(&format!("{tier} \u{2717}"), "bad"),
            };
            if it.style.is_none() && dim {
                it.style = Some("dim".into());
            }
            it.link = o.link.as_deref().map(|t| expand(t, inst, alias, &tier));
            items.push(it);
        }
        if o.caps {
            let t = s.cache.tiers.iter().find(|t| t.id == tier);
            if let Some(c) = t.map(caps_text).filter(|c| !c.is_empty()) {
                let mut it = link(Output::styled(&c, "dim"));
                it.style = Some("dim".into());
                items.push(it);
            }
        }
    }
    let mut out = Output {
        items,
        tokens,
        ..Default::default()
    };
    if out.items.is_empty() {
        // nothing selected to draw
        out = Output::hidden();
    }
    out
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

/// Which proxy-config layout `setup` prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `config-version: 8`: `api-keys.openai-compatibility`, `oauth.model-alias`, `keys`.
    V8,
    /// Flat file: `openai-compatibility`, `oauth-model-alias`, `api-key-entries`.
    Legacy,
}

impl Layout {
    pub fn parse(s: &str) -> Option<Layout> {
        match s {
            "v8" => Some(Layout::V8),
            "legacy" => Some(Layout::Legacy),
            _ => None,
        }
    }
}

/// The proxy-config snippet `setup` prints; nothing is written.
pub fn setup_snippet(inst: &Instance, c: &Cache, layout: Layout) -> Vec<String> {
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
    // (indent of the provider list, key-list field, claude rows' indent)
    let (mut o, keys_field) = match layout {
        Layout::V8 => (
            vec![
                "# Add to the proxy's config (v8 layout: `config-version: 8`). Merge each".into(),
                "# section into the file's existing one; do not repeat a top-level key.".into(),
                "oauth:".into(),
                "  model-alias:".into(),
                "    claude:".into(),
            ],
            "keys",
        ),
        Layout::Legacy => (
            vec![
                "# Add to the proxy's config (legacy layout, no `config-version: 8`). Merge each"
                    .into(),
                "# section into the file's existing one; do not repeat a top-level key.".into(),
                "oauth-model-alias:".into(),
                "  claude:".into(),
            ],
            "api-key-entries",
        ),
    };
    let (pad, row_pad) = match layout {
        Layout::V8 => ("    ", "      "),
        Layout::Legacy => ("  ", "    "),
    };
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
            "{row_pad}- {{name: {model}, alias: {alias}, fork: true}}"
        ));
    }
    if layout == Layout::V8 {
        o.push("api-keys:".into());
    }
    o.push(match layout {
        Layout::V8 => "  openai-compatibility:".into(),
        Layout::Legacy => "openai-compatibility:".into(),
    });
    let item = format!("{pad}- name: {}", inst.provider);
    let f = format!("{pad}  ");
    o.push(item);
    // routing stays OFF until `bar rl on`
    o.push(format!("{f}disabled: true"));
    o.push(format!("{f}base-url: {}/v1", inst.routellm));
    o.push(format!("{f}priority: {}", inst.on_priority));
    // without it, a provider that failed is skipped for about a minute
    // after it recovers
    o.push(format!("{f}disable-cooling: true"));
    o.push(format!("{f}{keys_field}:"));
    o.push(format!("{f}  - api-key: \"unused\""));
    o.push(format!("{f}models:"));
    for (alias, tier) in &aliases {
        o.push(format!("{f}  - {{name: {tier}, alias: {alias}}}"));
    }
    o.push("# `fork: true` keeps the original model id available to RouteLLM's own calls.".into());
    o.push("# No claude row for route-private: with routing off, that alias errors instead of leaving this host.".into());
    o.push(
        "# The api-key value is a placeholder; put the key RouteLLM expects, if it checks one."
            .into(),
    );
    o.push("# The provider is `disabled: true`: routing stays OFF until `bar rl on`.".into());
    if layout == Layout::V8 {
        o.push("# The plugin's first toggle rewrites a v8-layout file (comments are dropped); keep a backup.".into());
    }
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

    fn opts(s: &str) -> Opts {
        let o: serde_json::Map<String, serde_json::Value> = serde_json::from_str(s).unwrap();
        Opts::from_input(&RenderInput {
            options: o,
            ..Default::default()
        })
    }

    fn texts(o: &Output) -> Vec<(String, Option<String>)> {
        o.items
            .iter()
            .map(|i| (i.text.clone().unwrap(), i.style.clone()))
            .collect()
    }

    fn t(text: &str, style: Option<&str>) -> (String, Option<String>) {
        (text.to_string(), style.map(String::from))
    }

    #[test]
    fn the_state_item_says_what_a_click_does() {
        let i = inst();
        let go = |s: &Snapshot, now: i64, o: &Opts| output(s, &i, now, o, "claude-opus-4");
        let o = Opts::default();
        let on = go(&snap(false, Some(true), 100), 110, &o);
        assert_eq!(texts(&on), vec![t("RL on", Some("good"))]);
        assert_eq!(on.items[0].tokens["to"], "off");
        assert_eq!(on.items[0].tokens["instance"], instance_id(&i.origin));
        assert_eq!(on.tokens, on.items[0].tokens);
        let off = go(&snap(true, None, 100), 110, &o);
        assert_eq!(texts(&off), vec![t("RL off", Some("dim"))]);
        assert_eq!(off.items[0].tokens["to"], "on");
        let down = go(&snap(false, Some(false), 100), 110, &o);
        assert_eq!(texts(&down), vec![t("RL on \u{2717}", Some("bad"))]);
        let old = go(&snap(false, Some(true), 100), 100 + STALE_AFTER + 1, &o);
        assert_eq!(texts(&old), vec![t("RL on ~", Some("good"))]);
        let dim = go(
            &snap(false, Some(true), 100),
            100 + STALE_AFTER + 1,
            &opts(r#"{"stale":"dim"}"#),
        );
        assert_eq!(texts(&dim), vec![t("RL on", Some("dim"))]);
        let hidden = go(
            &snap(false, Some(true), 100),
            1000,
            &opts(r#"{"stale":"hide"}"#),
        );
        assert_eq!(hidden.text.as_deref(), Some(""));
    }

    #[test]
    fn a_session_model_names_its_alias() {
        assert_eq!(alias_of_model("route-auto").as_deref(), Some("route-auto"));
        assert_eq!(
            alias_of_model("team/Route-Vision").as_deref(),
            Some("route-vision")
        );
        for m in [
            "",
            "route-",
            "router-random-0.5",
            "claude-opus-4",
            "myroute-auto",
        ] {
            assert_eq!(alias_of_model(m), None, "{m}");
        }
    }

    #[test]
    fn the_alias_maps_to_its_tier_and_marker() {
        let i = inst();
        let on = snap(false, Some(true), 100);
        let go = |s: &Snapshot, model: &str, o: &str| output(s, &i, 110, &opts(o), model);
        // a cloud tier, and a tier declared local (private is by default)
        assert_eq!(
            texts(&go(&on, "route-auto", "{}")),
            vec![
                t("RL on", Some("good")),
                t("route-auto", None),
                t("auto \u{2601}", None)
            ]
        );
        assert_eq!(
            texts(&go(&on, "route-private", "{}"))[2],
            t("private \u{2302}", None)
        );
        // compact and `label=none` leave the alias item out
        for o in [r#"{"compact":true}"#, r#"{"label":"none"}"#] {
            assert_eq!(
                texts(&go(&on, "route-auto", o)),
                vec![t("RL on", Some("good")), t("auto \u{2601}", None)],
                "{o}"
            );
        }
        // an alias the provider does not expose, or a plain model: state only
        for m in ["route-hard", "claude-opus-4", ""] {
            assert_eq!(
                texts(&go(&on, m, "{}")),
                vec![t("RL on", Some("good"))],
                "{m}"
            );
        }
        // `routed` hides a session that is not on an alias
        assert_eq!(
            go(&on, "claude-opus-4", r#"{"routed":true}"#)
                .text
                .as_deref(),
            Some("")
        );
        assert_eq!(go(&on, "route-auto", r#"{"routed":true}"#).items.len(), 3);
    }

    #[test]
    fn off_and_down_show_where_the_alias_goes() {
        let i = inst();
        let go = |s: &Snapshot, model: &str, o: &str| texts(&output(s, &i, 110, &opts(o), model));
        let off = snap(true, None, 100);
        assert_eq!(
            go(&off, "route-auto", "{}"),
            vec![
                t("RL off", Some("dim")),
                t("route-auto", None),
                t("auto \u{2192} sonnet-5-5", None)
            ]
        );
        assert_eq!(
            go(&off, "route-auto", r#"{"show":"tier"}"#),
            vec![t("auto \u{2192} sonnet-5-5", None)]
        );
        // no row: the proxy refuses, and the item says so
        assert_eq!(
            go(&off, "route-private", "{}")[2],
            t("private \u{2717}", Some("bad"))
        );
        // routing on but RouteLLM down: no destination is claimed
        let down = snap(false, Some(false), 100);
        assert_eq!(
            go(&down, "route-auto", "{}")[2],
            t("auto \u{2717}", Some("bad"))
        );
        assert_eq!(
            go(&down, "route-private", "{}")[2],
            t("private \u{2717}", Some("bad"))
        );
    }

    #[test]
    fn caps_and_links_are_opt_in() {
        let i = inst();
        let mut s = snap(false, Some(true), 100);
        s.cache.tiers = vec![crate::cache::Tier {
            id: "auto".into(),
            kind: "tier".into(),
            vision: Some(true),
            tools: Some(true),
            context: Some(200_000),
        }];
        let o = output(&s, &i, 110, &opts(r#"{"show":"state,caps"}"#), "route-auto");
        assert_eq!(
            texts(&o),
            vec![t("RL on", Some("good")), t("vis tools 200k", Some("dim"))]
        );
        let o = output(
            &s,
            &i,
            110,
            &opts(r#"{"link":"x://{instance}/{alias}/{tier}?q={tier}%"}"#),
            "route-auto",
        );
        // the state item stays clickable for the toggle; the others link
        assert_eq!(o.items[0].link, None);
        assert_eq!(
            o.items[1].link.as_deref(),
            Some(format!("x://{}/route-auto/auto?q=auto%", instance_id(&i.origin)).as_str())
        );
        assert!(o.items[2].link.is_some());
        let none = output(&s, &i, 110, &opts("{}"), "route-auto");
        assert!(none.items.iter().all(|i| i.link.is_none()));
    }

    #[test]
    fn the_action_timeout_covers_the_worst_toggle_with_a_margin() {
        // two passes, each waiting for the proxy's lock and the instance's
        // (8 s each); the entry read twice, the change and three read-backs
        // (3 s each); RouteLLM's health and models (3 s each); the pauses
        let lock_waits = 2 * 2 * 8;
        let calls = (2 + 1 + 3) * 3;
        let probes = 2 * 3;
        let pauses = 2;
        let need = lock_waits + calls + probes + pauses;
        for (n, a) in actions() {
            assert!(
                a.timeout >= need + 10 && a.timeout <= 120,
                "{n}: {} for {need}",
                a.timeout
            );
        }
    }

    #[test]
    fn an_undrivable_instance_is_hidden() {
        let mut s = snap(false, Some(true), 100);
        s.cache.problem = Some(Problem::Mismatch);
        let o = Opts::default();
        assert_eq!(
            output(&s, &inst(), 101, &o, "route-auto").text.as_deref(),
            Some("")
        );
        let s = Snapshot {
            status: Status::AuthStopped(401),
            cache: Cache::default(),
        };
        let k = output(&s, &inst(), 101, &o, "route-auto");
        assert_eq!(texts(&k), vec![t("RL key?", Some("bad"))]);
    }

    #[test]
    fn setup_prints_a_snippet_without_a_private_row_and_without_secrets() {
        let s = snap(false, Some(true), 1);
        let text = setup_snippet(&inst(), &s.cache, Layout::Legacy).join("\n");
        assert!(text.contains("alias: route-auto, fork: true"), "{text}");
        assert!(text.contains("priority: 1000") && text.contains("disable-cooling: true"));
        assert!(!text.contains("route-private, fork"), "{text}");
        assert!(text.contains("name: private, alias: route-private"));
    }

    fn yaml(l: Layout) -> serde_norway::Value {
        let s = snap(false, Some(true), 1);
        serde_norway::from_str(&setup_snippet(&inst(), &s.cache, l).join("\n")).expect("valid YAML")
    }

    fn at<'a>(v: &'a serde_norway::Value, path: &[&str]) -> &'a serde_norway::Value {
        path.iter().fold(v, |v, k| &v[*k])
    }

    #[test]
    fn v8_snippet_is_a_complete_v8_block() {
        let v = yaml(Layout::V8);
        let p = &at(&v, &["api-keys", "openai-compatibility"])[0];
        assert_eq!(p["name"], "routellm");
        assert_eq!(p["disabled"], true);
        assert_eq!(p["priority"], 1000);
        assert_eq!(p["disable-cooling"], true);
        assert!(p["keys"][0]["api-key"].is_string(), "{p:?}");
        assert!(p["api-key-entries"].is_null());
        assert_eq!(p["models"][0]["alias"], "route-auto");
        let rows = at(&v, &["oauth", "model-alias", "claude"]);
        assert_eq!(rows[0]["alias"], "route-auto");
        assert_eq!(rows[0]["fork"], true);
        assert!(v["openai-compatibility"].is_null() && v["oauth-model-alias"].is_null());
    }

    #[test]
    fn legacy_snippet_is_a_complete_legacy_block() {
        let v = yaml(Layout::Legacy);
        let p = &v["openai-compatibility"][0];
        assert_eq!(p["name"], "routellm");
        assert_eq!(p["disabled"], true);
        assert_eq!(p["disable-cooling"], true);
        assert!(p["api-key-entries"][0]["api-key"].is_string(), "{p:?}");
        assert!(p["keys"].is_null());
        assert_eq!(v["oauth-model-alias"]["claude"][0]["alias"], "route-auto");
        assert!(v["api-keys"].is_null() && v["oauth"].is_null());
    }

    #[test]
    fn snippets_note_off_until_on_and_the_v8_rewrite() {
        let s = snap(false, Some(true), 1);
        let v8 = setup_snippet(&inst(), &s.cache, Layout::V8).join("\n");
        assert!(
            v8.contains("OFF until `bar rl on`") && v8.contains("keep a backup"),
            "{v8}"
        );
        let lg = setup_snippet(&inst(), &s.cache, Layout::Legacy).join("\n");
        assert!(
            lg.contains("OFF until `bar rl on`") && !lg.contains("backup"),
            "{lg}"
        );
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
