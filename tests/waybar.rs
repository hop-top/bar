// `bar waybar`: the module JSON's shape, Pango escaping, class choice,
// no plugins, a broken plugin.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/tray_fx.rs"]
mod tray_fx;
#[allow(unused_imports)]
use plugin_fx::*;
use tray_fx::*;

use hop_bar::tray::waybar::{escape, module};
use hop_bar::tray::SNAPSHOT_FILE;
use serde_json::{json, Value};

fn snap(items: Value) -> Value {
    json!({"version": 1, "plugins": [{"name": "p", "groups": [
        {"id": "g", "label": "G", "kind": "radio", "items": items}]}]})
}

fn waybar(t: &T) -> Value {
    let mut c = t.bar();
    c.arg("waybar");
    let out = busy_retry(|| c.output());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "one line: {text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn escape_covers_pango_specials() {
    assert_eq!(escape("a<b>&c&amp;"), "a&lt;b&gt;&amp;c&amp;amp;");
}

#[test]
fn plugin_text_is_escaped_in_text_and_tooltip() {
    let m = module(&snap(json!([
        {"id": "1", "label": "<b>x</b> & y", "detail": "a<b", "checked": true}
    ])));
    assert_eq!(m["text"], "&lt;b&gt;x&lt;/b&gt; &amp; y");
    let tip = m["tooltip"].as_str().unwrap();
    assert!(
        tip.contains("&lt;b&gt;x&lt;/b&gt; &amp; y (a&lt;b)"),
        "{tip}"
    );
    assert!(!tip.contains("<b>x"), "{tip}");
}

#[test]
fn class_is_the_worst_style() {
    let class = |styles: &[&str]| {
        let items: Vec<Value> = styles
            .iter()
            .enumerate()
            .map(|(n, s)| json!({"id": n.to_string(), "label": "l", "style": s}))
            .collect();
        module(&snap(json!(items)))["class"].clone()
    };
    assert_eq!(class(&[]), "idle");
    assert_eq!(class(&["dim"]), "idle");
    assert_eq!(class(&["good"]), "good");
    assert_eq!(class(&["good", "warn", "dim"]), "warn");
    assert_eq!(class(&["warn", "bad", "good"]), "bad");
    let mut v = snap(json!([{"id": "1", "label": "l", "style": "good"}]));
    v["last_action"] = json!({"at": 1, "ok": false, "note": "boom"});
    let m = module(&v);
    assert_eq!(
        (m["class"].as_str(), m["alt"].as_str()),
        (Some("bad"), Some("bad"))
    );
    assert!(m["tooltip"]
        .as_str()
        .unwrap()
        .ends_with("Last action failed: boom"));
}

#[test]
fn text_is_the_checked_account_and_waiting_count() {
    let mut v = snap(json!([
        {"id": "1", "label": "alice", "checked": false},
        {"id": "2", "label": "bob", "checked": true}]));
    assert_eq!(module(&v)["text"], "bob");
    v["sessions"] = json!({"waiting": 2, "done": 0});
    assert_eq!(module(&v)["text"], "bob · 2 waiting");
}

#[test]
fn no_plugins_is_a_valid_idle_module() {
    let t = T::new("");
    let m = waybar(&t);
    assert_eq!(m["text"], "bar");
    assert_eq!(m["class"], "idle");
    assert!(m["tooltip"].as_str().unwrap().contains("tray line"));
}

#[test]
fn end_to_end_shape_and_shared_snapshot_file() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    let m = waybar(&t);
    let mut keys: Vec<&str> = m.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["alt", "class", "text", "tooltip"]);
    assert_eq!(m["text"], "alice");
    assert_eq!(m["class"], "bad", "the info row styled bad");
    let tip = m["tooltip"].as_str().unwrap();
    assert!(
        tip.contains("● alice (5h 59%)") && tip.contains("○ bob"),
        "{tip}"
    );
    assert!(tip.lines().count() >= 5);
    assert!(t.state().join(SNAPSHOT_FILE).exists());
}

#[test]
fn a_broken_plugin_shows_bad_and_does_not_fail() {
    let t = T::new("tray nosuch\n");
    let m = waybar(&t);
    assert_eq!(m["class"], "bad");
    assert!(m["tooltip"]
        .as_str()
        .unwrap()
        .contains("bar-nosuch not found"));
}
