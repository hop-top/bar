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
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
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
fn each_run_is_a_new_generation() {
    let t = T::new("");
    let (a, _) = t.snapshot();
    let (b, _) = t.snapshot();
    assert_ne!(a.generation, b.generation);
    assert!(a.generation.bytes().all(|b| b.is_ascii_hexdigit()));
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

/// Best of `n` runs of `f`.
fn best(n: usize, mut f: impl FnMut() -> Duration) -> Duration {
    (0..n).map(|_| f()).min().unwrap()
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
    let files_only = best(5, time);
    assert!(files_only < budget, "{files_only:?} without plugins");
    // two fast plugins: what they cost themselves (interrogation and
    // menu, which bar runs one after the other per plugin, plugins side
    // by side) is taken out of the measure
    t.config("set tray-views on\ntray echo\ntray quick\n");
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    t.clone_echo("quick", &menu_ext(json!({})), &good_menu());
    let plugin_cost = best(5, || {
        let start = Instant::now();
        for arg in ["--ext-info", "--menu"] {
            let mut c = Command::new(t.fx.bin.join("bar-echo"));
            c.arg(arg).stdin(std::process::Stdio::null());
            assert!(busy_retry(|| c.output()).status.success());
        }
        start.elapsed()
    });
    time();
    let with_plugins = best(5, time);
    let own = with_plugins.saturating_sub(plugin_cost);
    assert!(
        own < budget,
        "{own:?} of bar's own ({with_plugins:?} in all, plugins {plugin_cost:?})"
    );
}

// --- lib seams ---

#[test]
fn granted_names_keep_order_and_drop_what_the_tray_refuses() {
    let c = cfg("tray b.use\ntray a\ntray b\ntray Bad\ntray a.reset\n");
    assert_eq!(tray::granted_names(&c), ["b", "a"]);
    assert!(tray::valid_name("a-b_1"));
    for bad in ["", "A", "a.b", "a b"] {
        assert!(!tray::valid_name(bad), "{bad:?}");
    }
    assert!(!tray::valid_name(&"a".repeat(33)));
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
