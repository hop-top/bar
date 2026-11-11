//! Per-session toggles: a session's own state over the global default,
//! the `bar toggle <key>?sid=<id>` form the handler passes, and cleanup.

// Unix fixtures (POSIX paths, symlinks); Windows coverage: tests/windows.rs
#![cfg(unix)]
mod common;

use assert_cmd::Command;
use hop_bar::click::hook::run_hook;
use hop_bar::click::{
    is_toggled, is_toggled_in, plugin_toggles, session_toggles_dir, toggle, toggle_in, toggle_link,
    toggle_target,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const A: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
const B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-stoggle-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

#[test]
fn session_flip_is_its_own_and_leaves_the_default() {
    let t = Tmp::new();
    let d = t.path();
    for key in ["5h", "tlc", "tlc.scope"] {
        assert_eq!(toggle_in(d, key, Some(A)), Ok(true), "{key}");
        assert!(is_toggled_in(d, key, A));
        assert!(!is_toggled_in(d, key, B), "{key}");
        assert!(!is_toggled(d, key), "{key}");
        assert_eq!(toggle_in(d, key, Some(A)), Ok(false));
        assert!(!is_toggled_in(d, key, A));
    }
    assert_eq!(
        toggle_in(d, "a.b.c", Some(A)),
        Err("unknown key `a.b.c` (expected 5h, 7d, a plugin name or <plugin>.<toggle>)".into())
    );
}

#[test]
fn global_default_applies_until_a_session_flips() {
    let t = Tmp::new();
    let d = t.path();
    assert_eq!(toggle(d, "tlc.scope"), Ok(true));
    assert!(is_toggled_in(d, "tlc.scope", A) && is_toggled_in(d, "tlc.scope", B));
    // A turns it off for itself; B keeps following the default
    assert_eq!(toggle_in(d, "tlc.scope", Some(A)), Ok(false));
    assert!(!is_toggled_in(d, "tlc.scope", A));
    assert!(is_toggled_in(d, "tlc.scope", B));
    assert_eq!(plugin_toggles(d, "tlc", A).len(), 0);
    assert_eq!(plugin_toggles(d, "tlc", B).get("scope"), Some(&true));
    // the default changing again does not override A's own choice
    assert_eq!(toggle(d, "tlc.scope"), Ok(false));
    assert!(!is_toggled_in(d, "tlc.scope", B));
    assert_eq!(toggle_in(d, "tlc.scope", Some(B)), Ok(true));
    assert_eq!(plugin_toggles(d, "tlc", B).get("scope"), Some(&true));
    assert!(plugin_toggles(d, "other", B).is_empty());
    // no valid session: the default
    assert!(!is_toggled_in(d, "tlc.scope", "s-1"));
}

#[test]
fn links_carry_only_a_valid_session_id() {
    assert_eq!(toggle_link("5h", A), format!("bar://toggle/5h?sid={A}"));
    assert_eq!(toggle_link("tlc.scope", ""), "bar://toggle/tlc.scope");
    assert_eq!(toggle_link("tlc", "../x"), "bar://toggle/tlc");
}

#[test]
fn toggle_target_parses_strictly() {
    assert_eq!(toggle_target("tlc"), Ok(("tlc", None)));
    let arg = format!("tlc.scope?sid={A}");
    assert_eq!(toggle_target(&arg), Ok(("tlc.scope", Some(A))));
    for bad in [
        "tlc?sid=",
        "tlc?sid=../../etc",
        "tlc?sid=ABC",
        "tlc?x=1",
        &format!("tlc?sid={A}&sid={B}"),
        &format!("tlc?sid={A}/.."),
    ] {
        assert!(toggle_target(bad).is_err(), "{bad}");
    }
    assert!(session_toggles_dir(Path::new("/s"), "..").is_none());
}

#[test]
fn cli_flips_the_session_named_in_the_target() {
    let t = Tmp::new();
    let run = |arg: &str| {
        Command::cargo_bin("bar")
            .unwrap()
            .args(["toggle", arg])
            .env("BAR_STATE_DIR", t.path())
            .assert()
    };
    run(&format!("tlc.scope?sid={A}"))
        .success()
        .stdout("tlc.scope: alt\n");
    assert!(is_toggled_in(t.path(), "tlc.scope", A));
    assert!(!t.path().join("toggle-tlc.scope").exists());
    run("tlc.scope?sid=../../x").code(2);
    run("5h").success().stdout("5h: pct\n");
    assert!(t.path().join("pct-5h").exists());
    // A still has no entry for 5h, so it follows the default
    assert!(is_toggled_in(t.path(), "5h", A));
}

#[test]
fn symlinked_session_dir_is_refused() {
    let t = Tmp::new();
    let d = t.path();
    let elsewhere = d.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(d.join("toggles")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, d.join("toggles").join(A)).unwrap();
    assert!(toggle_in(d, "5h", Some(A)).is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn session_end_drops_that_sessions_toggles_only() {
    let t = Tmp::new();
    let d = t.path();
    toggle_in(d, "tlc.scope", Some(A)).unwrap();
    toggle_in(d, "tlc.scope", Some(B)).unwrap();
    run_hook(
        d,
        &format!(r#"{{"session_id":"{A}","hook_event_name":"SessionEnd"}}"#),
    );
    assert!(!session_toggles_dir(d, A).unwrap().exists());
    assert!(is_toggled_in(d, "tlc.scope", B));
}

#[test]
fn a_flip_prunes_stale_session_dirs() {
    let t = Tmp::new();
    let d = t.path();
    toggle_in(d, "5h", Some(B)).unwrap();
    let old = session_toggles_dir(d, B).unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 86_400);
    std::fs::File::open(&old)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    toggle_in(d, "5h", Some(A)).unwrap();
    assert!(!old.exists());
    assert!(session_toggles_dir(d, A).unwrap().exists());
}
