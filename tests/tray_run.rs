// `bar tray run`: every check refuses on its own, the item runs with the
// snapshot's tokens, the snapshot is spent afterwards.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/tray_fx.rs"]
mod tray_fx;
#[allow(unused_imports)]
use plugin_fx::*;
use tray_fx::*;

use hop_bar::click::action::STATUS_FILE;
use hop_bar::tray::{run, SNAPSHOT_FILE};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BOB: &str = "echo.claude@8317.4";
const RESET: &str = "echo.more.reset";

/// A granted echo with the full menu, its snapshot written; returns the
/// snapshot's generation.
fn ready(conf: &str) -> (T, String) {
    let t = T::new(conf);
    t.fx.put("ext.json", &menu_ext(json!({})));
    t.fx.put("menu.json", &good_menu());
    let (s, _) = t.snapshot();
    (t, s.generation)
}

fn tray_run(t: &T, args: &[&str]) -> Output {
    let mut c = t.bar();
    c.args(["tray", "run"]).args(args);
    busy_retry(|| c.output())
}

fn ok(t: &T, args: &[&str]) -> String {
    let out = tray_run(t, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Exit 1, nothing ran; the first stderr line without its prefix.
fn refused(t: &T, args: &[&str]) -> String {
    let out = tray_run(t, args);
    assert_eq!(out.status.code(), Some(1), "{args:?}");
    assert!(!ran(t), "ran for {args:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    let line = err.lines().next().unwrap_or_default();
    line.strip_prefix("bar tray run: refused: ")
        .unwrap_or_else(|| panic!("{err:?}"))
        .to_string()
}

fn ran(t: &T) -> bool {
    t.fx.bin.join("action.json").exists()
}

fn status(t: &T) -> String {
    std::fs::read_to_string(t.state().join(STATUS_FILE)).unwrap_or_default()
}

fn mtime(t: &T) -> SystemTime {
    std::fs::metadata(t.state().join(SNAPSHOT_FILE))
        .unwrap()
        .modified()
        .unwrap()
}

fn set_mtime(t: &T, at: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(t.state().join(SNAPSHOT_FILE))
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// Rewrites the snapshot with `edit` applied, keeping the file private
/// and fresh.
fn edit(t: &T, edit: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_str(&t.file()).unwrap();
    edit(&mut v);
    std::fs::write(t.state().join(SNAPSHOT_FILE), v.to_string()).unwrap();
}

fn bob(v: &mut Value) -> &mut Value {
    &mut v["plugins"][0]["groups"][0]["items"][1]
}

// --- the run ---

#[test]
fn runs_the_item_with_the_snapshots_tokens() {
    let (t, g) = ready("tray echo\n");
    t.fx.put("action-out", "switched to bob\nmore\n");
    let out = ok(&t, &[BOB, "--generation", &g]);
    assert_eq!(out, "switched to bob\n", "the note goes to stdout");
    let got: Value = serde_json::from_str(&t.fx.file("action.json")).unwrap();
    assert_eq!(
        got,
        json!({"action": "use", "tokens": {"account": "4", "provider": "claude"}})
    );
    assert_eq!(t.fx.file("action.name").trim(), "use");
    assert_eq!(t.fx.file("action.state"), t.state().display().to_string());
    assert!(
        status(&t).trim_end().ends_with(" ok: switched to bob"),
        "{:?}",
        status(&t)
    );
    assert_eq!(mtime(&t), UNIX_EPOCH, "the snapshot is spent");
    let v: Value = serde_json::from_str(&t.file()).unwrap();
    assert_eq!(v["generation"], g, "but not changed");
}

#[test]
fn a_spent_snapshot_refuses_a_replay_until_the_next_state() {
    let (t, g) = ready("tray echo\n");
    ok(&t, &[BOB, "--generation", &g]);
    std::fs::remove_file(t.fx.bin.join("action.json")).unwrap();
    assert_eq!(
        refused(&t, &[BOB, "--generation", &g]),
        "menu is stale, reopen it"
    );
    assert_eq!(refused(&t, &[BOB]), "menu is stale, reopen it");
    let (s, _) = t.snapshot();
    // the items did not move, so the tray's generation still holds: the
    // spend alone stops a replay, the new snapshot allows a new click
    assert_eq!(s.generation, g);
    ok(&t, &[BOB, "--generation", &g]);
    assert!(ran(&t));
}

#[test]
fn a_failing_plugin_is_reported_and_the_snapshot_is_spent() {
    let (t, g) = ready("tray echo\n");
    t.fx.put("action-exit", "3");
    let out = tray_run(&t, &[BOB, "--generation", &g]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).lines().next(),
        Some("bar tray run: refused: echo.use failed")
    );
    assert!(
        status(&t).contains("refused: echo.use failed"),
        "{:?}",
        status(&t)
    );
    assert_eq!(mtime(&t), UNIX_EPOCH);
}

#[test]
fn a_refusal_is_recorded_and_leaves_the_snapshot_alone() {
    let (t, _) = ready("tray echo\n");
    let before = mtime(&t);
    assert_eq!(
        refused(&t, &[BOB, "--generation", "ffff"]),
        "menu changed, reopen it"
    );
    assert!(
        status(&t).contains("refused: menu changed, reopen it"),
        "{:?}",
        status(&t)
    );
    assert_eq!(mtime(&t), before);
}

// --- the id ---

#[test]
fn a_bad_id_is_refused_before_anything_is_read() {
    let (t, _) = ready("tray echo\n");
    std::fs::remove_file(t.state().join(SNAPSHOT_FILE)).unwrap();
    let long = format!("echo.g.{}", "i".repeat(96 - "echo.g.".len() + 1));
    for id in [
        "",
        "echo",
        "echo.claude@8317",
        "echo.claude@8317.4.x",
        "echo..4",
        "Echo.claude@8317.4",
        "echo.cl/aude.4",
        "echo.claude@8317.4;rm",
        "ech o.claude@8317.4",
        long.as_str(),
    ] {
        assert_eq!(refused(&t, &[id]), "bad item id", "{id:?}");
    }
    assert_eq!(
        run::parse_id("echo.claude@8317.4"),
        Some(("echo", "claude@8317", "4"))
    );
    // parts stay within their own limits (32 each): 96 binds the whole
    let max = format!("{}.{}.{}", "p".repeat(30), "g".repeat(32), "i".repeat(32));
    assert_eq!(max.len(), 96);
    assert!(run::parse_id(&max).is_some());
    assert_eq!(run::parse_id(&format!("p{max}")), None);
    assert_eq!(run::parse_id(&long), None);
}

// --- the snapshot ---

#[test]
fn no_snapshot_no_run() {
    let (t, _) = ready("tray echo\n");
    std::fs::remove_file(t.state().join(SNAPSHOT_FILE)).unwrap();
    assert_eq!(refused(&t, &[BOB]), "no menu, reopen it");
    std::fs::create_dir(t.state().join(SNAPSHOT_FILE)).unwrap();
    assert_eq!(refused(&t, &[BOB]), "menu file is not a file");
}

#[test]
fn a_snapshot_others_can_read_is_refused() {
    let (t, g) = ready("tray echo\n");
    let f = t.state().join(SNAPSHOT_FILE);
    for mode in [0o644, 0o640, 0o604, 0o660] {
        std::fs::set_permissions(&f, PermissionsExt::from_mode(mode)).unwrap();
        assert_eq!(
            refused(&t, &[BOB, "--generation", &g]),
            "menu file is not private",
            "{mode:o}"
        );
    }
    std::fs::set_permissions(&f, PermissionsExt::from_mode(0o600)).unwrap();
    ok(&t, &[BOB, "--generation", &g]);
}

#[test]
fn a_snapshot_owned_by_another_user_is_refused() {
    let (t, _) = ready("tray echo\n");
    let uid = std::fs::metadata(t.state().join(SNAPSHOT_FILE))
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap();
    let now = SystemTime::now();
    assert_eq!(
        run::load_as(t.state(), Some(uid + 1), now).unwrap_err(),
        "menu file is owned by another user"
    );
    assert!(run::load_as(t.state(), Some(uid), now).is_ok());
    assert!(
        run::load_as(t.state(), None, now).is_ok(),
        "unknown: not checked"
    );
}

#[test]
fn an_old_snapshot_is_refused() {
    let (t, g) = ready("tray echo\n");
    assert_eq!(run::MAX_AGE, Duration::from_secs(600));
    let written = mtime(&t);
    let at = |secs: i64| {
        if secs < 0 {
            written - Duration::from_secs(secs.unsigned_abs())
        } else {
            written + Duration::from_secs(secs as u64)
        }
    };
    let stale = Some("menu is stale, reopen it".to_string());
    assert_eq!(run::load_as(t.state(), None, at(601)).err(), stale);
    assert!(run::load_as(t.state(), None, at(599)).is_ok());
    assert_eq!(
        run::load_as(t.state(), None, at(-1)).err(),
        stale,
        "a file from the future is not trusted either"
    );
    set_mtime(&t, SystemTime::now() - Duration::from_secs(601));
    assert_eq!(
        refused(&t, &[BOB, "--generation", &g]),
        "menu is stale, reopen it"
    );
    set_mtime(&t, SystemTime::now() - Duration::from_secs(300));
    ok(&t, &[BOB, "--generation", &g]);
}

#[test]
fn a_snapshot_that_is_not_the_contract_is_refused() {
    let (t, g) = ready("tray echo\n");
    edit(&t, |v| v["version"] = json!(2));
    assert_eq!(
        refused(&t, &[BOB, "--generation", &g]),
        "menu is stale, reopen it"
    );
    edit(&t, |v| {
        v["version"] = json!(1);
        v["generation"] = json!(7);
    });
    assert_eq!(refused(&t, &[BOB]), "menu is stale, reopen it");
    std::fs::write(t.state().join(SNAPSHOT_FILE), "{not json").unwrap();
    assert_eq!(refused(&t, &[BOB]), "menu is stale, reopen it");
    t.snapshot();
    edit(&t, |v| v["pad"] = json!("x".repeat(4 << 20)));
    assert_eq!(refused(&t, &[BOB]), "menu is stale, reopen it", "too big");
}

#[test]
fn a_different_generation_is_refused_and_none_is_not_checked() {
    let (t, g) = ready("tray echo\n");
    for bad in ["ffff", &g.to_uppercase(), &format!("{g} "), ""] {
        assert_eq!(
            refused(&t, &[BOB, "--generation", bad]),
            "menu changed, reopen it",
            "{bad:?}"
        );
    }
    ok(&t, &[BOB]);
}

// --- the item ---

#[test]
fn an_item_not_in_the_menu_is_refused() {
    let (t, _) = ready("tray echo\n");
    for id in [
        "echo.claude@8317.9",
        "echo.nope.4",
        "other.claude@8317.4",
        "echo.more.4",
        "echo.4.claude@8317",
    ] {
        assert_eq!(refused(&t, &[id]), "item is not in the menu", "{id}");
    }
}

#[test]
fn a_disabled_item_is_refused() {
    let (t, _) = ready("tray echo.use\n");
    assert_eq!(refused(&t, &[RESET, "--confirmed"]), "item is disabled");
    edit(&t, |v| bob(v)["enabled"] = json!("yes"));
    assert_eq!(refused(&t, &[BOB]), "item is disabled");
    edit(&t, |v| bob(v)["enabled"] = json!(true));
    ok(&t, &[BOB]);
}

#[test]
fn an_item_without_an_action_is_refused() {
    let (t, _) = ready("tray echo\n");
    // an info row is never enabled; an enabled row must name an action
    assert_eq!(refused(&t, &["echo.info.auth"]), "item is disabled");
    edit(&t, |v| {
        v["plugins"][0]["groups"][2]["items"][0]["enabled"] = json!(true);
    });
    assert_eq!(refused(&t, &["echo.info.auth"]), "item has no action");
    edit(&t, |v| bob(v)["action"] = json!("Use"));
    assert_eq!(refused(&t, &[BOB]), "item has no action");
    edit(&t, |v| {
        bob(v)
            .as_object_mut()
            .unwrap()
            .remove("action")
            .map(|_| ())
            .unwrap()
    });
    assert_eq!(refused(&t, &[BOB]), "item has no action");
}

#[test]
fn bad_tokens_in_the_menu_are_refused() {
    let (t, _) = ready("tray echo\n");
    let bad: [(&str, Value); 6] = [
        ("shell", json!({"account": "4;rm", "provider": "claude"})),
        ("empty value", json!({"account": "", "provider": "claude"})),
        ("number", json!({"account": 4, "provider": "claude"})),
        ("name", json!({"Account": "4", "provider": "claude"})),
        ("list", json!(["4", "claude"])),
        ("missing", Value::Null),
    ];
    for (what, tokens) in bad {
        edit(&t, |v| bob(v)["tokens"] = tokens);
        assert_eq!(refused(&t, &[BOB]), "bad tokens in the menu", "{what}");
    }
    let many: serde_json::Map<String, Value> =
        (0..9).map(|i| (format!("t{i}"), json!("v"))).collect();
    edit(&t, |v| bob(v)["tokens"] = Value::Object(many));
    assert_eq!(refused(&t, &[BOB]), "bad tokens in the menu", "too many");
}

// --- the grant ---

#[test]
fn a_tray_line_must_cover_the_action() {
    let (t, _) = ready("tray echo\n");
    let no = "no `tray` line for echo.reset";
    for conf in [
        "",
        "tray echo.use\n",
        "tray other\n",
        "action echo echo.reset\n",
        "tray other.reset\n",
    ] {
        t.config(conf);
        assert_eq!(refused(&t, &[RESET, "--confirmed"]), no, "{conf:?}");
    }
    t.config("tray echo.reset\n");
    ok(&t, &[RESET, "--confirmed"]);
}

// --- the plugin ---

#[test]
fn the_plugin_must_be_found() {
    let (t, _) = ready("tray echo\n");
    std::fs::remove_file(t.fx.bin.join("bar-echo")).unwrap();
    assert_eq!(refused(&t, &[BOB]), "plugin not found");
}

#[test]
fn the_declaration_must_cover_the_action_and_its_tokens() {
    let (t, _) = ready("tray echo\n");
    t.fx.put(
        "ext.json",
        &menu_ext(json!({"actions": {"reset": {"tokens": []}}})),
    );
    assert_eq!(
        refused(&t, &[BOB]),
        "the plugin does not declare that action"
    );
    std::fs::remove_dir_all(t.state().join("cache")).unwrap();
    t.fx.put(
        "ext.json",
        &menu_ext(json!({"actions": {"use": {"tokens": ["account"]}}})),
    );
    assert_eq!(
        refused(&t, &[BOB]),
        "tokens do not match the action's declaration"
    );
    std::fs::remove_dir_all(t.state().join("cache")).unwrap();
    t.fx.put(
        "ext.json",
        &menu_ext(json!({"actions": {"use": {"tokens": ["account", "provider", "x"]}}})),
    );
    assert_eq!(
        refused(&t, &[BOB]),
        "tokens do not match the action's declaration"
    );
}

// --- confirm ---

#[test]
fn a_confirmed_action_needs_the_flag() {
    let (t, g) = ready("tray echo\n");
    assert_eq!(refused(&t, &[RESET]), "needs confirmation");
    assert_eq!(
        refused(&t, &[RESET, "--generation", &g]),
        "needs confirmation"
    );
    // the snapshot cannot waive what the declaration asks
    edit(&t, |v| {
        v["plugins"][0]["groups"][1]["items"][0]["confirm"] = json!(false);
    });
    assert_eq!(refused(&t, &[RESET]), "needs confirmation");
    ok(&t, &[RESET, "--confirmed"]);
    let got: Value = serde_json::from_str(&t.fx.file("action.json")).unwrap();
    assert_eq!(got, json!({"action": "reset", "tokens": {}}));
}

#[test]
fn a_snapshot_asking_for_confirmation_is_honoured_too() {
    let (t, _) = ready("tray echo\n");
    edit(&t, |v| bob(v)["confirm"] = json!(true));
    assert_eq!(refused(&t, &[BOB]), "needs confirmation");
    ok(&t, &[BOB, "--confirmed"]);
}

// --- the command line ---

#[test]
fn argv_carries_no_tokens() {
    let (t, g) = ready("tray echo\n");
    let out = tray_run(&t, &[BOB, "--generation", &g, "--account", "9"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!ran(&t));
    let out = tray_run(&t, &[BOB, "account=9"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!ran(&t));
}

#[test]
fn a_broken_config_is_a_usage_error() {
    let (t, g) = ready("tray echo\n");
    t.config("tray echo\nnonsense line here\n");
    let out = tray_run(&t, &[BOB, "--generation", &g]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("config error"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!ran(&t));
}
