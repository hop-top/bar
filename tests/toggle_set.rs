//! `bar toggle <key> <value>` (set, not flip), the keys `bar toggle`
//! accepts from the command line, and `bar init` repeated.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]
mod common;

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const A: &str = "0a1b2c3d-0000-4000-8000-00000000000a";

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-tset-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(p.join("state")).unwrap();
        std::fs::create_dir_all(p.join("bin")).unwrap();
        Tmp(p)
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
    /// An executable `bar-<name>` in the test's PATH dir; never run here.
    fn plugin(&self, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let f = self.0.join("bin").join(format!("bar-{name}"));
        std::fs::write(&f, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn bar(&self, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("bar").unwrap();
        c.args(args)
            .env("BAR_STATE_DIR", self.state())
            .env("PATH", self.0.join("bin"))
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("xdg"))
            .current_dir(&self.0);
        c
    }
    fn files(&self, dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

#[test]
fn set_form_is_idempotent() {
    let t = Tmp::new();
    let f = t.state().join("pct-5h");
    for _ in 0..2 {
        t.bar(&["toggle", "5h", "pct"])
            .assert()
            .success()
            .stdout("5h: pct\n");
        assert!(f.exists());
    }
    for _ in 0..2 {
        t.bar(&["toggle", "5h", "bar"])
            .assert()
            .success()
            .stdout("5h: bar\n");
        assert!(!f.exists());
    }
    t.bar(&["toggle", "7d", "on"])
        .assert()
        .success()
        .stdout("7d: pct\n");
    t.bar(&["toggle", "7d", "on"])
        .assert()
        .success()
        .stdout("7d: pct\n");
    t.bar(&["toggle", "7d", "off"])
        .assert()
        .success()
        .stdout("7d: bar\n");
    assert!(!t.state().join("pct-7d").exists());
}

#[test]
fn set_form_takes_plugin_view_names() {
    let t = Tmp::new();
    t.plugin("tlc");
    for _ in 0..2 {
        t.bar(&["toggle", "tlc", "alt"])
            .assert()
            .success()
            .stdout("tlc: alt\n");
    }
    t.bar(&["toggle", "tlc", "show"])
        .assert()
        .success()
        .stdout("tlc: show\n");
    t.bar(&["toggle", "tlc.scope", "on"])
        .assert()
        .success()
        .stdout("tlc.scope: alt\n");
    assert!(t.state().join("toggle-tlc.scope").exists());
    assert!(!t.state().join("toggle-tlc").exists());
}

#[test]
fn set_form_rejects_a_value_of_another_kind() {
    let t = Tmp::new();
    t.plugin("tlc");
    for (key, value, valid) in [
        ("5h", "alt", "pct, bar, on or off"),
        ("tlc", "pct", "alt, show, on or off"),
        ("5h", "yes", "pct, bar, on or off"),
    ] {
        t.bar(&["toggle", key, value])
            .assert()
            .code(2)
            .stdout("")
            .stderr(predicate::str::contains(valid));
    }
    assert_eq!(t.files(&t.state()), 0);
}

#[test]
fn set_form_in_a_session_is_idempotent() {
    let t = Tmp::new();
    let target = format!("5h?sid={A}");
    let own = t.state().join("toggles").join(A).join("pct-5h");
    for _ in 0..2 {
        t.bar(&["toggle", &target, "on"])
            .assert()
            .success()
            .stdout("5h: pct\n");
        assert_eq!(std::fs::read_to_string(&own).unwrap(), "on");
    }
    t.bar(&["toggle", &target, "bar"]).assert().success();
    assert_eq!(std::fs::read_to_string(&own).unwrap(), "off");
    // the handler's flip form still flips
    t.bar(&["toggle", &target])
        .assert()
        .success()
        .stdout("5h: pct\n");
    t.bar(&["toggle", &target])
        .assert()
        .success()
        .stdout("5h: bar\n");
    assert!(!t.state().join("pct-5h").exists());
}

#[test]
fn unknown_key_is_rejected_naming_the_valid_ones() {
    let t = Tmp::new();
    t.plugin("tlc");
    t.plugin("cliproxyapi");
    for key in ["no-such-view", "bogus", "bogus.scope", "tlc.a.b", "5h.x"] {
        t.bar(&["toggle", key])
            .assert()
            .code(2)
            .stdout("")
            .stderr(predicate::str::contains(format!("unknown key `{key}`")))
            // plugins beside the `bar` binary are listed too
            .stderr(predicate::str::contains("valid keys: 5h, 7d, "))
            .stderr(predicate::str::contains(" cliproxyapi, "))
            .stderr(predicate::str::contains(" tlc, "))
            .stderr(predicate::str::contains(
                "or <plugin>.<toggle> for one of those plugins",
            ));
    }
    t.bar(&["toggle", "bogus", "on"]).assert().code(2);
    assert_eq!(t.files(&t.state()), 0);
}

#[test]
fn installed_plugin_keys_are_accepted() {
    let t = Tmp::new();
    t.plugin("tlc");
    t.bar(&["toggle", "tlc"])
        .assert()
        .success()
        .stdout("tlc: alt\n");
    t.bar(&["toggle", "tlc.scope"])
        .assert()
        .success()
        .stdout("tlc.scope: alt\n");
    assert!(t.state().join("toggle-tlc").exists());
    assert!(t.state().join("toggle-tlc.scope").exists());
}

#[test]
fn link_form_flips_without_a_plugin_lookup() {
    // links come from a render, whose PATH may hold plugins the
    // handler's does not; only the key's shape is checked
    let t = Tmp::new();
    let target = format!("tlc.scope?sid={A}");
    t.bar(&["toggle", &target])
        .assert()
        .success()
        .stdout("tlc.scope: alt\n");
    let bad = format!("a.b.c?sid={A}");
    t.bar(&["toggle", &bad]).assert().code(2);
}

// ── init ───────────────────────────────────────────────────────────────────

#[test]
fn init_same_preset_again_is_a_no_op_success() {
    let t = Tmp::new();
    t.bar(&["init", "--preset", "minimal"]).assert().success();
    let path = t.0.join("xdg/bar/bar.conf");
    let before = std::fs::read_to_string(&path).unwrap();
    let meta = std::fs::metadata(&path).unwrap().modified().unwrap();
    t.bar(&["init", "--preset", "minimal"])
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("exists; not overwritten"))
        .stderr(predicate::str::contains("already preset minimal"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), meta);
}

#[test]
fn init_other_preset_over_existing_still_refuses() {
    let t = Tmp::new();
    t.bar(&["init", "--preset", "minimal"]).assert().success();
    let path = t.0.join("xdg/bar/bar.conf");
    let before = std::fs::read_to_string(&path).unwrap();
    t.bar(&["init", "--preset", "full"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("exists; not overwritten"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn init_same_preset_over_an_edited_config_refuses() {
    let t = Tmp::new();
    t.bar(&["init", "--preset", "minimal"]).assert().success();
    let path = t.0.join("xdg/bar/bar.conf");
    let edited = format!(
        "{}set right \"#{{ctx}}\"\n",
        std::fs::read_to_string(&path).unwrap()
    );
    std::fs::write(&path, &edited).unwrap();
    t.bar(&["init", "--preset", "minimal"]).assert().code(4);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
}
