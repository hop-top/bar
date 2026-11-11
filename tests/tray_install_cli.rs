//! `bar tray` from the command line, every dir in a temp dir: the dry
//! run and the read-only subcommands, which run nothing.

// Unix fixtures (POSIX paths)
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use render_fx::Tmp;
use serde_json::Value;

/// `bar tray <args>` with every dir under `t`.
fn cli(t: &Tmp, args: &[&str]) -> std::process::Output {
    let cwd = t.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .arg("tray")
        .args(args)
        .current_dir(&cwd)
        .env("HOME", t.path().join("home"))
        .env("BAR_STATE_DIR", t.path().join("state"))
        .env("CLAUDE_CONFIG_DIR", t.path().join("claude"))
        .env("XDG_CONFIG_HOME", t.path().join("cfg"))
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap()
}

#[test]
fn cli_dry_run_prints_the_plan_and_writes_nothing() {
    let t = Tmp::new();
    let o = cli(&t, &["install", "--dry-run"]);
    let out = String::from_utf8_lossy(&o.stdout);
    if cfg!(target_os = "macos") {
        assert!(o.status.success(), "{o:?}");
        assert!(out.contains("top.hop.bar-tray.plist:"), "{out}");
        assert!(out.contains("/bin/launchctl bootstrap gui/"), "{out}");
    } else {
        assert_eq!(o.status.code(), Some(2), "{o:?}");
    }
    for p in ["state", "home/Library", "claude"] {
        assert!(!t.path().join(p).exists(), "{p}");
    }
}

#[test]
fn cli_status_json_without_install() {
    let t = Tmp::new();
    let o = cli(&t, &["status", "--json"]);
    assert!(o.status.success(), "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["installed"], false);
    assert_eq!(v["running"], false);
    assert_eq!(v["autostart"], Value::Null);
    let o = cli(&t, &["stop"]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), "not running\n");
}
