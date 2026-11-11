// `tray` grants, `set tray-views`, `set tray-poll`: parsing, project
// config refusal, `bar check`.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::conf::{layers, parse, TrayGrant};
use serde_json::json;

fn grant(plugin: &str, action: Option<&str>) -> TrayGrant {
    TrayGrant {
        plugin: plugin.into(),
        action: action.map(Into::into),
    }
}

#[test]
fn defaults_grant_nothing() {
    let c = parse("").unwrap();
    assert!(c.tray.is_empty());
    assert!(!c.tray_views);
    assert_eq!(c.tray_poll, 30);
}

#[test]
fn tray_lines_parse_in_order_without_duplicates() {
    let c = parse("tray cliproxy\ntray routellm.toggle\ntray cliproxy\n").unwrap();
    assert_eq!(
        c.tray,
        [grant("cliproxy", None), grant("routellm", Some("toggle"))]
    );
}

#[test]
fn bad_tray_lines_are_errors() {
    for bad in [
        "tray",
        "tray a b",
        "tray a.b.c",
        "tray .b",
        "tray a.",
        "tray \"a b\"",
        "tray a/b",
    ] {
        assert!(parse(&format!("{bad}\n")).is_err(), "{bad}");
    }
}

#[test]
fn tray_options_parse_and_validate() {
    let c = parse("set tray-views on\nset tray-poll 90s\n").unwrap();
    assert!(c.tray_views);
    assert_eq!(c.tray_poll, 90);
    assert_eq!(parse("set tray-poll 10s\n").unwrap().tray_poll, 10);
    assert_eq!(parse("set tray-poll 10m\n").unwrap().tray_poll, 600);
    assert!(!parse("set tray-views off\n").unwrap().tray_views);
    for bad in [
        "set tray-views yes",
        "set tray-poll 9s",
        "set tray-poll 11m",
        "set tray-poll 30",
        "set tray-poll fast",
    ] {
        assert!(parse(&format!("{bad}\n")).is_err(), "{bad}");
    }
}

#[test]
fn tray_is_user_config_only() {
    for mode in ["safe", "full"] {
        let user = format!("set project-config {mode}\n");
        let proj = "tray echo\nset tray-views on\nset tray-poll 15s\n";
        let l = layers(Some(&user), |_| Some(Ok(proj.into())));
        let c = l.config.unwrap();
        assert!(
            c.tray.is_empty() && !c.tray_views && c.tray_poll == 30,
            "{mode}"
        );
        assert_eq!(l.dropped.len(), 3, "{:?}", l.dropped);
        assert!(l.dropped.iter().all(|d| d.message.contains("user config")));
    }
}

#[test]
fn action_lines_do_not_grant_tray() {
    let c = parse("action echo echo.use\n").unwrap();
    assert_eq!(c.actions.len(), 1);
    assert!(c.tray.is_empty());
}

// --- bar check ---

fn check(fx: &Fx, conf: &str) -> Vec<String> {
    hop_bar::check::check_text(conf, None, &fx.dirs())
        .issues
        .into_iter()
        .map(|i| format!("{:?}: {}", i.level, i.message))
        .collect()
}

fn menu_info() -> String {
    echo_info(json!({
        "capabilities": ["placeholder", "menu"],
        "actions": {"use": {"tokens": ["account"]}},
    }))
}

#[test]
fn check_accepts_a_declared_menu_plugin_and_action() {
    let fx = Fx::new();
    fx.put("ext.json", &menu_info());
    let got = check(&fx, "tray echo\ntray echo.use\n");
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn check_warns_on_unknown_plugin() {
    let fx = Fx::new();
    let got = check(&fx, "tray nosuch\n");
    assert!(
        got.iter()
            .any(|m| m.starts_with("Warning") && m.contains("nosuch") && m.contains("not on PATH")),
        "{got:?}"
    );
}

#[test]
fn check_warns_without_menu_capability() {
    let fx = Fx::new();
    fx.put(
        "ext.json",
        &echo_info(json!({"actions": {"use": {"tokens": []}}})),
    );
    let got = check(&fx, "tray echo\n");
    assert!(
        got.iter()
            .any(|m| m.starts_with("Warning") && m.contains("`menu` capability")),
        "{got:?}"
    );
}

#[test]
fn check_warns_on_undeclared_action() {
    let fx = Fx::new();
    fx.put("ext.json", &menu_info());
    let got = check(&fx, "tray echo.nope\n");
    assert!(
        got.iter()
            .any(|m| m.starts_with("Warning") && m.contains("does not declare the action `nope`")),
        "{got:?}"
    );
}

#[test]
fn check_reports_bad_option_values() {
    let fx = Fx::new();
    let got = check(&fx, "set tray-poll 5s\nset tray-views maybe\n");
    assert!(got.iter().any(|m| m.contains("tray-poll")), "{got:?}");
    assert!(got.iter().any(|m| m.contains("tray-views")), "{got:?}");
}
