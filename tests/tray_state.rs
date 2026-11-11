// `bar tray state`: the snapshot's shape against the tray's own parser,
// the state dir inputs, the 0600 atomic write, the time budget.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/tray_fx.rs"]
mod tray_fx;
#[allow(unused_imports)]
use plugin_fx::*;
use tray_fx::*;

use bar_tray::model::snapshot::{self, GroupKind};
use hop_bar::click::action::STATUS_FILE;
use hop_bar::tray::{self, state, SNAPSHOT_FILE};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

// --- shape ---

/// A granted echo with a full menu, a last action, three session
/// markers (one empty) and the 7d view toggled.
fn seeded() -> T {
    let t = T::new("tray echo\nset tray-views on\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    std::fs::write(
        t.state().join(STATUS_FILE),
        "1762799988 ok: preferred alice\n",
    )
    .unwrap();
    let sessions = t.state().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("s-1"), "waiting").unwrap();
    std::fs::write(sessions.join("s-2"), "done\n").unwrap();
    std::fs::write(sessions.join("s-3"), "done").unwrap();
    std::fs::write(sessions.join("s-4"), "").unwrap();
    std::fs::write(t.state().join("pct-7d"), "").unwrap();
    t
}

#[test]
fn the_snapshot_header_is_what_the_tray_reads() {
    let t = seeded();
    let (s, raw) = t.snapshot();
    assert_eq!(raw["version"], 1);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.generation.len(), 32);
    assert!(now() - s.at <= 5, "at {} now {}", s.at, now());
    let a = s.last_action.as_ref().unwrap();
    assert_eq!(
        (a.at, a.ok, a.note.as_str()),
        (1762799988, true, "preferred alice")
    );
    let c = s.sessions.unwrap();
    assert_eq!((c.waiting, c.done), (1, 2));
    let views: Vec<(&str, &str, bool)> = s
        .views
        .iter()
        .map(|v| (v.key.as_str(), v.label.as_str(), v.on))
        .collect();
    assert_eq!(
        views,
        [
            ("5h", "5h usage", false),
            ("7d", "7d usage", true),
            ("echo", "echo alternate text", false)
        ]
    );
    assert_eq!(t.file(), format!("{raw}\n"), "the file is what was printed");
}

#[test]
fn the_snapshot_carries_the_configured_poll() {
    let t = T::new("");
    assert_eq!(t.snapshot().0.poll, Some(30));
    t.config("set tray-poll 2m\n");
    let (s, raw) = t.snapshot();
    assert_eq!(raw["poll"], 120);
    assert_eq!(s.poll, Some(120));
}

#[test]
fn the_plugin_menus_are_what_the_tray_reads() {
    let t = seeded();
    let (s, raw) = t.snapshot();
    let p = plugin(&s, "echo");
    let kinds: Vec<(&str, GroupKind)> = p.groups.iter().map(|g| (g.id.as_str(), g.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("claude@8317", GroupKind::Radio),
            ("more", GroupKind::Buttons),
            ("info", GroupKind::Info)
        ]
    );
    assert_eq!(p.groups[0].label.as_deref(), Some("Claude · :8317"));
    let alice = &p.groups[0].items[0];
    assert_eq!(alice.label, "alice");
    assert_eq!(alice.detail.as_deref(), Some("5h 59%"));
    assert!(alice.checked && alice.enabled && !alice.confirm);
    assert_eq!(alice.action.as_deref(), Some("use"));
    let reset = &p.groups[1].items[0];
    assert!(reset.confirm, "confirm comes from the declaration");
    assert_eq!(raw["plugins"][0]["groups"][1]["items"][0]["confirm"], true);
    assert_eq!(p.groups[2].items[0].style, Some(snapshot::Style::Bad));
    // the tokens travel in the snapshot for `bar tray run`, the tray ignores them
    assert_eq!(
        raw["plugins"][0]["groups"][0]["items"][0]["tokens"],
        json!({"account": "3", "provider": "claude"})
    );
}

#[test]
fn the_file_is_private_and_renamed_into_place() {
    let t = T::new("");
    std::fs::write(t.state().join(SNAPSHOT_FILE), "old").unwrap();
    std::fs::set_permissions(
        t.state().join(SNAPSHOT_FILE),
        PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    let (_, raw) = t.snapshot();
    let f = t.state().join(SNAPSHOT_FILE);
    assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(t.file(), format!("{raw}\n"));
    let left: Vec<String> = std::fs::read_dir(t.state())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(raw["plugins"], json!([]));
    assert_eq!(raw["views"], json!([]), "views need tray-views on");
    assert!(raw.get("last_action").is_none());
}

#[test]
fn the_state_dir_is_created_when_missing() {
    let t = T::new("");
    let fresh = t.fx.root.join("fresh-state");
    let mut c = t.bar();
    let out = c
        .env("BAR_STATE_DIR", &fresh)
        .args(["tray", "state", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(fresh.join(SNAPSHOT_FILE).is_file());
}

#[test]
fn the_generation_follows_the_runnable_items() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    let (a, _) = t.snapshot();
    assert_eq!(a.generation.len(), 32);
    assert!(a.generation.bytes().all(|b| b.is_ascii_hexdigit()));
    // another run, a session marker and a last action: same items, same id
    std::fs::write(t.state().join(STATUS_FILE), "7 ok: x\n").unwrap();
    std::fs::create_dir_all(t.state().join("sessions")).unwrap();
    std::fs::write(t.state().join("sessions/s-1"), "waiting").unwrap();
    assert_eq!(t.snapshot().0.generation, a.generation);
    // text, the checked row and styles change: a click means the same
    let mut m: Value = serde_json::from_str(&good_menu()).unwrap();
    m["groups"][0]["items"][0]["label"] = json!("alice (renamed)");
    m["groups"][0]["items"][0]["detail"] = json!("5h 61%");
    m["groups"][0]["items"][0]["checked"] = json!(false);
    m["groups"][0]["items"][1]["checked"] = json!(true);
    m["groups"][2]["items"][0]["style"] = json!("good");
    t.fx.put("menu.json", &m.to_string());
    assert_eq!(t.snapshot().0.generation, a.generation);
    // a token value changes: a new id
    m["groups"][0]["items"][1]["tokens"]["account"] = json!("5");
    t.fx.put("menu.json", &m.to_string());
    let b = t.snapshot().0.generation;
    assert_ne!(b, a.generation);
    // an item goes: a new id
    m["groups"][1]["items"] = json!([]);
    t.fx.put("menu.json", &m.to_string());
    let c = t.snapshot().0.generation;
    assert_ne!(c, b);
    // a grant changes what is enabled: a new id
    t.fx.put("menu.json", &good_menu());
    t.config("tray echo.use\n");
    assert_ne!(t.snapshot().0.generation, a.generation);
    // the empty menu has an id of its own
    t.config("");
    let (e, _) = t.snapshot();
    assert_eq!(e.generation.len(), 32);
    assert_ne!(e.generation, a.generation);
}

#[test]
fn generation_is_a_pure_function_of_the_items() {
    use hop_bar::tray::state::generation;
    let a = json!([{"name": "p", "groups": [{"id": "g", "items": [
        {"id": "i", "action": "use", "tokens": {"b": "2", "a": "1"}, "enabled": true}]}]}]);
    let same = json!([{"name": "p", "label": "P", "groups": [{"id": "g", "label": "G", "items": [
        {"id": "i", "label": "x", "action": "use", "tokens": {"a": "1", "b": "2"}, "enabled": true,
         "checked": true, "style": "good", "limit": 10}]}]}]);
    let other_tokens = json!([{"name": "p", "groups": [{"id": "g", "items": [
        {"id": "i", "action": "use", "tokens": {"a": "1", "b": "3"}, "enabled": true}]}]}]);
    let other_action = json!([{"name": "p", "groups": [{"id": "g", "items": [
        {"id": "i", "action": "reset", "tokens": {"a": "1", "b": "2"}, "enabled": true}]}]}]);
    let confirm = json!([{"name": "p", "groups": [{"id": "g", "items": [
        {"id": "i", "action": "use", "tokens": {"a": "1", "b": "2"}, "enabled": true, "confirm": true}]}]}]);
    let g = |v: &Value| generation(v.as_array().unwrap());
    assert_eq!(g(&a), g(&same), "token order and cosmetics do not count");
    assert_ne!(g(&a), g(&other_tokens));
    assert_ne!(g(&a), g(&other_action));
    assert_ne!(g(&a), g(&confirm));
    assert_ne!(g(&a), generation(&[]));
    assert_eq!(generation(&[]), generation(&[]));
}

#[test]
fn without_json_a_summary_is_printed_and_the_file_still_written() {
    let t = T::new("tray echo\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    let mut c = t.bar();
    let out = c.args(["tray", "state"]).output().unwrap();
    assert!(out.status.success());
    let line = String::from_utf8(out.stdout).unwrap();
    assert!(
        line.contains("tray-menu.json: 1 plugin(s), 4 item(s)"),
        "{line}"
    );
    snapshot::parse(&t.file()).unwrap();
}

#[test]
fn a_broken_config_is_refused() {
    let t = T::new("tray a.b.c\n");
    let out = t.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("config error"));
    assert!(!t.state().join(SNAPSHOT_FILE).exists());
}

// --- status line ---

#[test]
fn action_status_lines_parse_strictly() {
    let p = |l: &str| state::parse_status(l);
    assert_eq!(
        p("1762799988 ok"),
        Some(json!({"at": 1762799988, "ok": true, "note": ""}))
    );
    assert_eq!(
        p("1762799988 ok: preferred alice\n"),
        Some(json!({"at": 1762799988, "ok": true, "note": "preferred alice"}))
    );
    assert_eq!(
        p("7 refused: no `action` line for echo.use"),
        Some(json!({"at": 7, "ok": false, "note": "no `action` line for echo.use"}))
    );
    for bad in [
        "",
        "ok",
        "x ok",
        "7 okay",
        "7 refused",
        "7 nope: x",
        "7 ok:x",
    ] {
        assert_eq!(p(bad), None, "{bad:?}");
    }
}

#[test]
fn a_refused_action_shows_as_not_ok() {
    let t = T::new("");
    std::fs::write(t.state().join(STATUS_FILE), "7 refused: plugin not found\n").unwrap();
    let (s, _) = t.snapshot();
    let a = s.last_action.unwrap();
    assert_eq!((a.ok, a.note.as_str()), (false, "plugin not found"));
    std::fs::write(t.state().join(STATUS_FILE), "garbage\n").unwrap();
    let (s, _) = t.snapshot();
    assert!(s.last_action.is_none());
}

// --- budget ---

/// Runs of each measure; the best counts. Enough that a busy moment on
/// the host (sibling tests run alongside) passes.
const RUNS: usize = 15;

/// Best of `n` runs of `f`.
fn best(n: usize, mut f: impl FnMut() -> Duration) -> Duration {
    (0..n).map(|_| f()).min().unwrap()
}

/// What a plugin costs bar on its own: `--ext-info`, then `--menu`.
fn interrogate(plugin: &Path) {
    for arg in ["--ext-info", "--menu"] {
        let mut c = Command::new(plugin);
        c.arg(arg).stdin(std::process::Stdio::null());
        assert!(busy_retry(|| c.output()).status.success());
    }
}

#[test]
fn a_warm_run_fits_the_budget() {
    let budget = Duration::from_millis(50);
    let t = T::new("set tray-views on\n");
    std::fs::write(t.state().join(STATUS_FILE), "7 ok: x\n").unwrap();
    std::fs::create_dir_all(t.state().join("sessions")).unwrap();
    std::fs::write(t.state().join("sessions/s-1"), "waiting").unwrap();
    let time = || {
        let start = Instant::now();
        assert!(t.run().status.success());
        start.elapsed()
    };
    time();
    let files_only = best(RUNS, time);
    assert!(files_only < budget, "{files_only:?} without plugins");
    // two fast plugins: what they cost themselves is taken out of the
    // measure, run as bar runs them (each interrogated then asked for
    // its menu, the two side by side), so the contention of two plugins
    // starting at once counts as theirs, not bar's
    t.config("set tray-views on\ntray echo\ntray quick\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    t.clone_echo("quick", &menu_ext(json!({})), &good_menu());
    let plugins = [t.fx.bin.join("bar-echo"), t.bin2.join("bar-quick")];
    let plugin_cost = || {
        let start = Instant::now();
        std::thread::scope(|s| {
            for p in &plugins {
                s.spawn(move || interrogate(p));
            }
        });
        start.elapsed()
    };
    time();
    // in turns, so a busy moment weighs on both measures alike
    let (mut with_plugins, mut cost) = (Duration::MAX, Duration::MAX);
    for _ in 0..RUNS {
        cost = cost.min(plugin_cost());
        with_plugins = with_plugins.min(time());
    }
    let own = with_plugins.saturating_sub(cost);
    assert!(
        own < budget,
        "{own:?} of bar's own ({with_plugins:?} in all, plugins {cost:?})"
    );
}

// --- lib seams ---

#[test]
fn granted_names_keep_order_and_drop_what_the_tray_refuses() {
    let c = cfg("tray b.use\ntray a\ntray b\ntray My-Plugin\ntray a.reset\n");
    assert_eq!(tray::granted_names(&c), ["b", "a", "My-Plugin"]);
    // the tray takes every name a `tray` line takes
    for ok in ["a-b_1", "A", "MyPlugin", &"a".repeat(64)] {
        assert!(tray::valid_name(ok), "{ok:?}");
        assert!(cfg(&format!("tray {ok}\n")).tray.len() == 1, "{ok:?}");
    }
    for bad in ["", "a.b", "a b", &"a".repeat(65)] {
        assert!(!tray::valid_name(bad), "{bad:?}");
    }
}

#[test]
fn views_are_empty_unless_asked_for() {
    let t = T::new("");
    assert!(state::views(t.state(), &cfg("tray echo\n")).is_empty());
    let v = state::views(t.state(), &cfg("set tray-views on\n"));
    assert_eq!(v.len(), 2);
}

#[test]
fn sessions_count_markers_only() {
    let t = T::new("");
    assert_eq!(state::sessions(t.state()), json!({"waiting": 0, "done": 0}));
    let d = t.state().join("sessions");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("a"), "waiting\n").unwrap();
    std::fs::write(d.join("b"), "waiting").unwrap();
    std::fs::write(d.join("c"), "done").unwrap();
    std::fs::write(d.join("d"), "later").unwrap();
    assert_eq!(state::sessions(t.state()), json!({"waiting": 2, "done": 1}));
}
