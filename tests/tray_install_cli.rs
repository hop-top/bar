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
    } else if cfg!(target_os = "linux") {
        assert!(o.status.success(), "{o:?}");
        assert!(out.contains("bar-tray.desktop:\n[Desktop Entry]"), "{out}");
        assert!(!t.path().join("cfg").exists() && !t.path().join("home").exists());
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

/// Another install's autostart: install refuses to take it over, also as
/// a dry run, and `--force` does.
#[test]
fn cli_install_takes_another_installs_autostart_only_with_force() {
    let t = Tmp::new();
    let other = t.path().join("other");
    t.write("other/tray-installed", "");
    let argv: Vec<String> = ["/o/bar-tray", "--bar", "/o/bar", "--lock"]
        .iter()
        .map(|a| a.to_string())
        .chain([other.join("tray.lock").display().to_string()])
        .collect();
    let (path, body) = if cfg!(target_os = "macos") {
        let p = "home/Library/LaunchAgents/top.hop.bar-tray.plist";
        (p, hop_bar::tray::agent::plist(&argv, &[]))
    } else if cfg!(target_os = "linux") {
        let p = "cfg/autostart/bar-tray.desktop";
        (p, hop_bar::tray::xdg::desktop_entry(&argv, &[]).unwrap())
    } else {
        return;
    };
    t.write(path, &body);
    let o = cli(&t, &["install", "--dry-run"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(
        err.contains(&format!(
            "belongs to the tray install in {}",
            other.display()
        )),
        "{err}"
    );
    let o = cli(&t, &["install", "--dry-run", "--force"]);
    assert!(o.status.success(), "{o:?}");
    let o = cli(&t, &["install", "--no-autostart", "--force"]);
    assert_eq!(o.status.code(), Some(2), "--force is for autostart: {o:?}");
    assert_eq!(std::fs::read_to_string(t.path().join(path)).unwrap(), body);
}
