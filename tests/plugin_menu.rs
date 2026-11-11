// The `menu` capability: `--menu` parsing under the output rules, the
// declaration check, the stdin shape and the run limit.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::plugin::menu::{self, MAX_GROUPS, MAX_LABEL, MAX_MENU_ID};
use hop_bar::plugin::{parse_info, parse_menu, MenuKind, MAX_ITEMS, MAX_TOKENS};
use serde_json::{json, Map, Value};
use std::time::Instant;

fn item(id: &str, label: &str) -> Value {
    json!({"id": id, "label": label, "action": "use",
           "tokens": {"account": id, "provider": "claude"}})
}

fn menu_of(groups: Value) -> String {
    json!({ "groups": groups }).to_string()
}

fn one(kind: &str, items: Value) -> String {
    menu_of(json!([{"id": "g", "kind": kind, "items": items}]))
}

// --- capability ---

#[test]
fn menu_capability_is_read_from_ext_info() {
    let with = parse_info(&echo_info(json!({"capabilities": ["placeholder", "menu"]}))).unwrap();
    assert!(with.has_menu());
    let without = parse_info(&echo_info(json!({}))).unwrap();
    assert!(!without.has_menu());
    assert_eq!(hop_bar::plugin::MENU_CAPABILITY, "menu");
}

// --- parse: accept ---

fn good_menu() -> String {
    menu_of(json!([
        {"id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
            {"id": "3", "label": "alice", "detail": "5h 59% ↻2h10m", "checked": true,
             "action": "use", "tokens": {"account": "3", "provider": "claude"}},
            {"id": "4", "label": "bob", "enabled": false,
             "action": "use", "tokens": {"account": "4", "provider": "claude"}}]},
        {"id": "next@8317", "kind": "buttons", "items": [
            {"id": "next", "label": "Use most quota left", "action": "next", "tokens": {"provider": "claude"}}]},
        {"id": "rtellm", "label": "Routing", "kind": "check", "items": [
            {"id": "toggle", "label": "Routing", "checked": true, "action": "toggle", "tokens": {"to": "off"}}]},
        {"id": "info", "kind": "info", "items": [
            {"id": "auth", "label": "management refused the key", "style": "bad"}]}
    ]))
}

#[test]
fn a_good_menu_parses_whole() {
    let m = parse_menu(&good_menu()).unwrap();
    assert_eq!(m.groups.len(), 4);
    let g = &m.groups[0];
    assert_eq!((g.id.as_str(), g.kind), ("claude@8317", MenuKind::Radio));
    assert_eq!(g.label.as_deref(), Some("Claude · :8317"));
    assert_eq!(g.items[0].detail.as_deref(), Some("5h 59% ↻2h10m"));
    assert!(g.items[0].checked && g.items[0].enabled);
    assert!(!g.items[1].checked && !g.items[1].enabled);
    assert_eq!(g.items[0].tokens["account"], "3");
    assert_eq!(m.groups[1].kind, MenuKind::Buttons);
    assert_eq!(m.groups[2].kind, MenuKind::Check);
    let info = &m.groups[3];
    assert_eq!(info.kind, MenuKind::Info);
    assert_eq!(info.items[0].style.as_deref(), Some("bad"));
    assert!(!info.items[0].enabled && info.items[0].action.is_none());
}

#[test]
fn a_menu_goes_back_out_in_the_protocol_shape() {
    let v = parse_menu(&good_menu()).unwrap().to_json();
    assert_eq!(v["groups"][0]["kind"], "radio");
    assert_eq!(v["groups"][0]["label"], "Claude · :8317");
    assert_eq!(v["groups"][0]["items"][0]["tokens"]["account"], "3");
    assert_eq!(v["groups"][0]["items"][0]["checked"], true);
    assert_eq!(v["groups"][0]["items"][1]["enabled"], false);
    assert!(v["groups"][0]["items"][0].get("enabled").is_none());
    assert_eq!(v["groups"][3]["items"][0]["style"], "bad");
    assert!(v["groups"][3]["items"][0].get("action").is_none());
}

#[test]
fn an_empty_menu_is_a_valid_answer() {
    assert!(parse_menu(r#"{"groups":[]}"#).unwrap().is_empty());
    assert!(parse_menu(r#"{"groups":[], "extra": 1}"#)
        .unwrap()
        .is_empty());
}

// --- parse: reject ---

#[test]
fn not_a_menu_is_none() {
    for raw in [
        "",
        "nope",
        "[]",
        "{}",
        r#"{"groups": 5}"#,
        r#"{"groups": {}}"#,
    ] {
        assert!(parse_menu(raw).is_none(), "{raw}");
    }
}

#[test]
fn a_group_needs_a_known_kind_an_id_and_items() {
    for groups in [
        json!([{"id": "g", "kind": "menu", "items": [item("1", "a")]}]),
        json!([{"id": "g", "items": [item("1", "a")]}]),
        json!([{"kind": "radio", "items": [item("1", "a")]}]),
        json!([{"id": "g.h", "kind": "radio", "items": [item("1", "a")]}]),
        json!([{"id": "g", "kind": "radio", "items": []}]),
        json!([{"id": "g", "kind": "radio"}]),
        json!([{"id": "g", "kind": "radio", "items": "x"}]),
        json!(["g"]),
    ] {
        let m = parse_menu(&menu_of(groups.clone())).unwrap();
        assert!(m.is_empty(), "{groups}");
    }
}

#[test]
fn an_item_needs_an_id_a_label_and_an_action() {
    let keep = |items: Value| parse_menu(&one("buttons", items)).unwrap().groups.len();
    assert_eq!(keep(json!([item("1", "a")])), 1);
    assert_eq!(keep(json!([{"label": "a", "action": "use"}])), 0);
    assert_eq!(
        keep(json!([{"id": "a b", "label": "a", "action": "use"}])),
        0
    );
    assert_eq!(
        keep(json!([{"id": "a.b", "label": "a", "action": "use"}])),
        0
    );
    assert_eq!(
        keep(json!([{"id": "a".repeat(MAX_MENU_ID + 1), "label": "a", "action": "use"}])),
        0
    );
    assert_eq!(keep(json!([{"id": "1", "action": "use"}])), 0);
    assert_eq!(
        keep(json!([{"id": "1", "label": " \u{200B} ", "action": "use"}])),
        0
    );
    assert_eq!(
        keep(json!([{"id": "1", "label": "a"}])),
        0,
        "no action outside info"
    );
    assert_eq!(keep(json!([{"id": "1", "label": "a", "action": "Use"}])), 0);
    assert_eq!(
        keep(json!([{"id": "1", "label": "a", "action": "use"}])),
        1,
        "no tokens is fine"
    );
    assert_eq!(keep(json!(["1"])), 0);
}

#[test]
fn info_rows_need_no_action_and_carry_none() {
    let m = parse_menu(&one(
        "info",
        json!([{"id": "auth", "label": "ok", "checked": true, "enabled": true,
                "action": "use", "tokens": {"account": "1"}}]),
    ))
    .unwrap();
    let i = &m.groups[0].items[0];
    assert!(i.action.is_none() && i.tokens.is_empty());
    assert!(!i.checked && !i.enabled);
}

#[test]
fn strings_are_sanitised_and_cut() {
    let long = "x".repeat(MAX_LABEL + 10);
    let m = parse_menu(&menu_of(json!([{
        "id": "g", "kind": "buttons", "label": " Pick\u{1b}[31m one\t",
        "items": [{"id": "1", "label": " al\u{202E}ice\n", "detail": long,
                   "action": "use", "style": "loud"}]
    }])))
    .unwrap();
    let g = &m.groups[0];
    assert_eq!(g.label.as_deref(), Some("Pick[31m one"));
    assert_eq!(g.items[0].label, "alice");
    assert_eq!(
        g.items[0].detail.as_ref().unwrap().chars().count(),
        MAX_LABEL
    );
    assert_eq!(g.items[0].style, None, "unknown styles are dropped");
    let m = parse_menu(&menu_of(
        json!([{"id": "g", "kind": "buttons", "label": "\u{200B}",
        "items": [{"id": "1", "label": "a", "detail": "  ", "action": "use"}]}]),
    ))
    .unwrap();
    assert_eq!(m.groups[0].label, None);
    assert_eq!(m.groups[0].items[0].detail, None);
}

#[test]
fn tokens_follow_the_output_rules() {
    let m = parse_menu(&one(
        "buttons",
        json!([{"id": "1", "label": "a", "action": "use",
                "tokens": {"account": "a1.b_c@d-e", "Bad": "x", "to": "bad value", "n": 5}}]),
    ))
    .unwrap();
    let t = &m.groups[0].items[0].tokens;
    assert_eq!(t.len(), 1);
    assert_eq!(t["account"], "a1.b_c@d-e");
    let mut many = Map::new();
    for n in 0..=MAX_TOKENS {
        many.insert(format!("t{n}"), json!("v"));
    }
    let m = parse_menu(&one(
        "buttons",
        json!([{"id": "1", "label": "a", "action": "use", "tokens": many}]),
    ))
    .unwrap();
    assert!(m.is_empty(), "more than {MAX_TOKENS} tokens drops the item");
}

#[test]
fn surplus_and_duplicates_are_dropped() {
    let items: Vec<Value> = (0..MAX_ITEMS + 4)
        .map(|n| item(&n.to_string(), "a"))
        .collect();
    let m = parse_menu(&one("buttons", json!(items))).unwrap();
    assert_eq!(m.groups[0].items.len(), MAX_ITEMS);
    let groups: Vec<Value> = (0..MAX_GROUPS + 4)
        .map(|n| json!({"id": n.to_string(), "kind": "buttons", "items": [item("1", "a")]}))
        .collect();
    let m = parse_menu(&menu_of(json!(groups))).unwrap();
    assert_eq!(m.groups.len(), MAX_GROUPS);
    let m = parse_menu(&menu_of(json!([
        {"id": "g", "kind": "buttons", "items": [item("1", "first"), item("1", "second")]},
        {"id": "g", "kind": "radio", "items": [item("2", "later")]}
    ])))
    .unwrap();
    assert_eq!(m.groups.len(), 1);
    assert_eq!(m.groups[0].kind, MenuKind::Buttons);
    assert_eq!(m.groups[0].items.len(), 1);
    assert_eq!(m.groups[0].items[0].label, "first");
}

#[test]
fn a_radio_keeps_one_checked_item() {
    let mut a = item("1", "a");
    a["checked"] = json!(true);
    let mut b = item("2", "b");
    b["checked"] = json!(true);
    let m = parse_menu(&one("radio", json!([a.clone(), b.clone()]))).unwrap();
    let checked: Vec<&str> = m.groups[0]
        .items
        .iter()
        .filter(|i| i.checked)
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(checked, ["1"]);
    let m = parse_menu(&one("check", json!([a, b]))).unwrap();
    assert_eq!(m.groups[0].items.iter().filter(|i| i.checked).count(), 2);
}

// --- declaration ---

fn declared_info() -> hop_bar::plugin::ExtInfo {
    parse_info(&echo_info(
        json!({"capabilities": ["placeholder", "menu"], "actions": {
            "use": {"description": "Prefer", "tokens": ["account", "provider"]},
            "reset": {"description": "Reset", "tokens": [], "confirm": true}
        }}),
    ))
    .unwrap()
}

#[test]
fn declared_keeps_only_items_whose_action_and_tokens_match() {
    let raw = menu_of(json!([
        {"id": "pick", "kind": "radio", "items": [
            item("1", "ok"),
            {"id": "2", "label": "undeclared", "action": "next", "tokens": {"provider": "claude"}},
            {"id": "3", "label": "missing token", "action": "use", "tokens": {"account": "3"}},
            {"id": "4", "label": "extra token", "action": "use",
             "tokens": {"account": "4", "provider": "claude", "to": "x"}},
            {"id": "5", "label": "other names", "action": "use",
             "tokens": {"acct": "5", "provider": "claude"}}]},
        {"id": "more", "kind": "buttons", "items": [
            {"id": "reset", "label": "Reset", "action": "reset"}]},
        {"id": "gone", "kind": "buttons", "items": [
            {"id": "x", "label": "x", "action": "nope"}]},
        {"id": "info", "kind": "info", "items": [{"id": "a", "label": "auth ok"}]}
    ]));
    let m = parse_menu(&raw).unwrap().declared(&declared_info());
    let ids: Vec<&str> = m.groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["pick", "more", "info"]);
    let pick: Vec<&str> = m.groups[0].items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(pick, ["1"]);
    assert!(!m.groups[0].items[0].confirm);
    assert!(
        m.groups[1].items[0].confirm,
        "confirm comes from the declaration"
    );
    assert_eq!(m.to_json()["groups"][1]["items"][0]["confirm"], true);
    assert!(m.to_json()["groups"][0]["items"][0]
        .get("confirm")
        .is_none());
}

// --- stdin ---

#[test]
fn menu_input_carries_options_toggles_and_accounts_when_any() {
    let mut o = Map::new();
    o.insert("provider".into(), json!("claude"));
    let mut t = Map::new();
    t.insert("scope".into(), json!(true));
    let v: Value =
        serde_json::from_str(&menu::menu_input(o.clone(), t.clone(), json!([]))).unwrap();
    assert_eq!(v["options"]["provider"], "claude");
    assert_eq!(v["toggles"]["scope"], true);
    assert!(v.get("accounts").is_none());
    let rules = json!([{"glob": "*@x", "label": "w", "color": "33"}]);
    let v: Value = serde_json::from_str(&menu::menu_input(o, t, rules.clone())).unwrap();
    assert_eq!(v["accounts"], rules);
}

// --- run ---

#[test]
fn run_feeds_stdin_sets_the_state_dir_and_parses_the_answer() {
    let fx = Fx::new();
    fx.put("menu.json", &one("buttons", json!([item("1", "a")])));
    let m = menu::run(
        &fx.found("echo"),
        r#"{"options":{"n":1},"toggles":{}}"#,
        &fx.state,
    )
    .unwrap();
    assert_eq!(m.groups[0].items[0].id, "1");
    let sent: Value = serde_json::from_str(&fx.file("menu-stdin.json")).unwrap();
    assert_eq!(sent["options"]["n"], 1);
    assert_eq!(fx.file("menu.state"), fx.state.display().to_string());
}

#[test]
fn run_is_none_on_failure_or_bad_output() {
    let fx = Fx::new();
    assert!(
        menu::run(&fx.found("echo"), "{}", &fx.state).is_none(),
        "no menu.json"
    );
    fx.put("menu.json", &one("buttons", json!([item("1", "a")])));
    fx.put("menu-exit", "3");
    assert!(menu::run(&fx.found("echo"), "{}", &fx.state).is_none());
    assert!(
        menu::run(&fx.found("plain"), "{}", &fx.state).is_none(),
        "no --menu at all"
    );
}

#[test]
fn run_gives_up_at_the_limit() {
    let fx = Fx::new();
    fx.put("menu.json", &one("buttons", json!([item("1", "a")])));
    fx.put("menu-sleep", "20");
    let t = Instant::now();
    assert!(menu::run(&fx.found("echo"), "{}", &fx.state).is_none());
    let took = t.elapsed();
    assert!(took >= hop_bar::plugin::MENU_TIMEOUT, "{took:?}");
    assert!(took < hop_bar::plugin::MENU_TIMEOUT * 3, "{took:?}");
}
