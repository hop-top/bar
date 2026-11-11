// `bar tray state`, the plugins part: grants, the declaration check,
// rows for plugins without a menu, the `--menu` stdin, parallel runs
// with the cap, the render cache left alone.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/tray_fx.rs"]
mod tray_fx;
#[allow(unused_imports)]
use plugin_fx::*;
use tray_fx::*;

use bar_tray::model::snapshot::{GroupKind, Snapshot};
use hop_bar::tray::SNAPSHOT_FILE;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[test]
fn an_action_without_a_grant_is_shown_disabled() {
    let t = T::new("tray echo.use\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    let (s, raw) = t.snapshot();
    let p = plugin(&s, "echo");
    assert!(p.groups[0].items[0].enabled, "use is granted");
    let reset = &p.groups[1].items[0];
    assert!(!reset.enabled);
    assert_eq!(
        reset.detail.as_deref(),
        Some("not enabled: tray echo.reset")
    );
    assert_eq!(reset.action.as_deref(), Some("reset"), "the action stays");
    assert!(raw["plugins"][0]["groups"][1]["items"][0]
        .get("tokens")
        .is_some());
    assert!(
        p.groups[2].items[0].action.is_none(),
        "info rows carry none"
    );
    // a plugin-wide line grants every action
    t.config("tray echo\n");
    let (s, _) = t.snapshot();
    assert!(plugin(&s, "echo").groups[1].items[0].enabled);
}

#[test]
fn undeclared_items_are_dropped() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put(
        "menu.json",
        &menu_of(json!([
            {"id": "pick", "kind": "radio", "items": [
                item("1", "ok"),
                {"id": "2", "label": "undeclared", "action": "next", "tokens": {"provider": "claude"}},
                {"id": "3", "label": "missing token", "action": "use", "tokens": {"account": "3"}}]},
            {"id": "gone", "kind": "buttons", "items": [
                {"id": "x", "label": "x", "action": "nope"}]}
        ])),
    );
    let (s, _) = t.snapshot();
    let p = plugin(&s, "echo");
    assert_eq!(p.groups.len(), 1);
    let ids: Vec<&str> = p.groups[0].items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["1"]);
}

#[test]
fn plugins_without_a_menu_get_one_info_row() {
    let t = T::new("tray echo\ntray plain\ntray nosuch\ntray Upper\n");
    // echo: no `menu` capability (the fixture's default --ext-info)
    t.fx.put("menu.json", &good_menu());
    let (s, _) = t.snapshot();
    let names: Vec<&str> = s.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["echo", "plain", "nosuch", "Upper"],
        "every `tray` line gets an entry, capitals and all"
    );
    let row = |s: &Snapshot, name: &str| {
        let p = plugin(s, name);
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].kind, GroupKind::Info);
        let i = &p.groups[0].items[0];
        assert!(!i.enabled && i.action.is_none());
        (i.id.clone(), i.label.clone())
    };
    assert_eq!(
        row(&s, "echo"),
        ("no-menu".into(), "echo has no menu".into())
    );
    assert_eq!(
        row(&s, "plain"),
        ("no-menu".into(), "plain has no menu".into())
    );
    assert_eq!(
        row(&s, "nosuch"),
        ("missing".into(), "bar-nosuch not found".into())
    );
    assert_eq!(
        row(&s, "Upper"),
        ("missing".into(), "bar-Upper not found".into())
    );
    // declares the capability but answers nothing
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu-exit", "3");
    let (s, _) = t.snapshot();
    assert_eq!(
        row(&s, "echo"),
        ("no-menu".into(), "echo gave no menu".into())
    );
}

#[test]
fn items_carry_the_run_limit_of_their_action() {
    let t = T::new("tray echo\n");
    t.fx.put(
        "ext.json",
        &menu_ext(json!({"actions": {
            "use": {"description": "Prefer", "tokens": ["account", "provider"]},
            "reset": {"description": "Reset", "tokens": [], "timeout": 30},
            "long": {"description": "Long", "tokens": [], "timeout": 500}
        }})),
    );
    t.fx.put(
        "menu.json",
        &menu_of(json!([
            {"id": "g", "kind": "buttons", "items": [
                item("1", "a"),
                {"id": "r", "label": "r", "action": "reset"},
                {"id": "l", "label": "l", "action": "long"}]},
            {"id": "info", "kind": "info", "items": [{"id": "i", "label": "i"}]}
        ])),
    );
    let (_, raw) = t.snapshot();
    let items = &raw["plugins"][0]["groups"][0]["items"];
    assert_eq!(items[0]["limit"], 10, "the default");
    assert_eq!(items[1]["limit"], 30, "the declared timeout");
    assert_eq!(items[2]["limit"], 120, "capped");
    assert!(
        raw["plugins"][0]["groups"][1]["items"][0]
            .get("limit")
            .is_none(),
        "info rows run nothing"
    );
}

#[test]
fn an_empty_menu_is_an_entry_without_groups() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", r#"{"groups":[]}"#);
    let (s, raw) = t.snapshot();
    assert!(plugin(&s, "echo").groups.is_empty());
    assert_eq!(raw["plugins"][0]["groups"], json!([]));
}

#[test]
fn the_menu_run_gets_options_toggles_accounts_and_the_state_dir() {
    let t = T::new("tray echo\ndefault echo limit=7 icon=x\naccount \"*@example.test\" work 33\n");
    t.fx.put("ext.json", &menu_ext(json!({"inputs": {"accounts": true}})));
    t.fx.put("menu.json", &good_menu());
    std::fs::write(t.state().join("toggle-echo.scope"), "").unwrap();
    std::fs::write(t.state().join("toggle-echo"), "").unwrap();
    t.snapshot();
    let sent: Value = serde_json::from_str(&t.fx.file("menu-stdin.json")).unwrap();
    assert_eq!(sent["options"], json!({"limit": 7}));
    assert_eq!(sent["toggles"], json!({"scope": true}));
    assert_eq!(
        sent["accounts"],
        json!([{"glob": "*@example.test", "label": "work", "color": "33"}])
    );
    assert_eq!(t.fx.file("menu.state"), t.state().display().to_string());
    // an invalid value falls back to the schema defaults; no account
    // rules, no `accounts`
    t.config("tray echo\ndefault echo limit=lots\n");
    t.snapshot();
    let sent: Value = serde_json::from_str(&t.fx.file("menu-stdin.json")).unwrap();
    assert_eq!(sent["options"], json!({"limit": 3}));
    assert!(sent.get("accounts").is_none());
}

#[test]
fn text_is_cut_and_overlong_ids_dropped() {
    let name = "e".repeat(31);
    let t = T::new(&format!("tray echo\ntray {name}\n"));
    let long = "x".repeat(100);
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put(
        "menu.json",
        &menu_of(
            json!([{"id": "g", "label": long, "kind": "buttons", "items": [
            {"id": "1", "label": long, "detail": long, "action": "reset"}]}]),
        ),
    );
    let gid = "g".repeat(32);
    t.clone_echo(
        &name,
        &menu_ext(json!({})),
        &menu_of(json!([{"id": gid, "kind": "buttons", "items": [
            {"id": "i".repeat(32), "label": "too long", "action": "reset"},
            {"id": "i".repeat(31), "label": "fits", "action": "reset"}]}])),
    );
    let (s, raw) = t.snapshot();
    let p = plugin(&s, "echo");
    assert_eq!(
        raw["plugins"][0]["groups"][0]["label"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        80
    );
    let i = &raw["plugins"][0]["groups"][0]["items"][0];
    assert_eq!(i["label"].as_str().unwrap().chars().count(), 80);
    assert_eq!(i["detail"].as_str().unwrap().chars().count(), 80);
    assert_eq!(p.groups[0].items[0].label.len(), 80);
    let p = plugin(&s, &name);
    let ids: Vec<&str> = p.groups[0].items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["i".repeat(31).as_str()]);
}

#[test]
fn menus_run_in_parallel() {
    let t = T::new("tray echo\ntray lag\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    t.fx.put("menu-sleep", "2");
    t.clone_echo("lag", &menu_ext(json!({})), &good_menu());
    std::fs::write(t.bin2.join("menu-sleep"), "2").unwrap();
    let start = Instant::now();
    let (s, _) = t.snapshot();
    let took = start.elapsed();
    assert!(took >= Duration::from_secs(2), "{took:?}");
    assert!(took < hop_bar::plugin::MENU_TIMEOUT * 2, "{took:?}");
    assert_eq!(plugin(&s, "echo").groups.len(), 3);
    assert_eq!(plugin(&s, "lag").groups.len(), 3);
    // each plugin writes its stdin before sleeping: run one after the
    // other, the second would start 2 s after the first
    let started = |dir: &std::path::Path| {
        std::fs::metadata(dir.join("menu-stdin.json"))
            .unwrap()
            .modified()
            .unwrap()
    };
    let (a, b) = (started(&t.fx.bin), started(&t.bin2));
    let apart = a.duration_since(b).unwrap_or_else(|e| e.duration());
    assert!(
        apart < Duration::from_secs(1),
        "{apart:?} apart: not in parallel"
    );
}

#[test]
fn a_plugin_past_the_cap_is_given_up_on() {
    let t = T::new("tray echo\ntray lag\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    t.clone_echo("lag", &menu_ext(json!({})), &good_menu());
    std::fs::write(t.bin2.join("menu-sleep"), "20").unwrap();
    let start = Instant::now();
    let (s, _) = t.snapshot();
    let took = start.elapsed();
    assert!(took >= hop_bar::plugin::MENU_TIMEOUT, "{took:?}");
    assert!(took < hop_bar::plugin::MENU_TIMEOUT * 3, "{took:?}");
    assert_eq!(
        plugin(&s, "echo").groups.len(),
        3,
        "the others still answer"
    );
    let slow = plugin(&s, "lag");
    assert_eq!(slow.groups[0].items[0].label, "lag gave no menu");
}

#[test]
fn the_render_cache_is_never_touched() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    t.snapshot();
    t.snapshot();
    assert!(!t.state().join("cache").exists());
    let names: Vec<String> = std::fs::read_dir(t.state())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [SNAPSHOT_FILE]);
}

#[test]
fn a_plugin_named_with_capitals_gets_its_menu_and_view() {
    let name = format!("MyProxy{}", "x".repeat(57));
    assert_eq!(name.len(), 64);
    let t = T::new(&format!("tray {name}\nset tray-views on\n"));
    t.clone_echo(&name, &menu_ext(json!({})), &good_menu());
    let (s, _) = t.snapshot();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let p = plugin(&s, &name);
    assert_eq!(p.groups.len(), 3);
    assert!(p.groups[0].items[0].enabled);
    assert!(s.views.iter().any(|v| v.key == name), "{:?}", s.views);
}
