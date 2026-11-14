//! `bar-tlc` through the core: a click on the count flips the scope
//! (`tlc.scope`), a click on the mark shows the track name (`tlc`).

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::click::toggle;
use hop_bar::session::Session;
use serde_json::json;
use std::path::{Path, PathBuf};

const SID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
const SID_B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";
const CLOSE: &str = "\x1b]8;;\x1b\\";

/// `bar-tlc` behind a wrapper that puts a fake `tlc` first on `PATH`:
/// track `alpha` (slug `alpha-slug`) holds 1 closed of 2, the project 3 of
/// 3; `T-1` is on `alpha`, `T-2` has no track. Returns the project root.
fn setup(fx: &Fx) -> PathBuf {
    let fake = fx.root.join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    common::exe::write(
        &fake.join("tlc"),
        r#"#!/bin/sh
case "$*" in
  "task show T-1 "*) echo '{"task":{"id":"T-1","track_id":"alpha"}}' ;;
  "task show T-2 "*) echo '{"task":{"id":"T-2","track_id":null}}' ;;
  "track show alpha "*) echo '{"id":"alpha","slug":"alpha-slug"}' ;;
  *"--track alpha"*) echo '[{"status":"DONE"},{"status":"TODO"}]' ;;
  "task list"*) echo '[{"status":"DONE"},{"status":"DONE"},{"status":"CANCELLED"}]' ;;
  *) exit 1 ;;
esac
"#,
    );
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/bar-tlc");
    common::exe::write(
        &fx.bin.join("bar-tlc"),
        format!(
            "#!/bin/sh\nPATH='{}':$PATH\nexport PATH\nexec '{}' \"$@\"\n",
            fake.display(),
            real.display()
        ),
    );
    let proj = fx.root.join("proj");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    std::fs::create_dir_all(proj.join(".tlc")).unwrap();
    std::fs::write(proj.join(".tlc/config.yaml"), "").unwrap();
    proj
}

fn record(fx: &Fx, sid: &str, task: &str, proj: &Path) {
    let dir = fx.state.join("records/tlc");
    std::fs::create_dir_all(&dir).unwrap();
    let r = json!({"task": task, "cwd": proj, "at": 1});
    std::fs::write(dir.join(sid), r.to_string()).unwrap();
}

fn in_session(sid: &str, proj: &Path) -> Session {
    Session {
        session_id: sid.into(),
        cwd: proj.display().to_string(),
        ..session()
    }
}

fn url(key: &str, sid: &str) -> String {
    format!("bar://toggle/{key}?sid={sid}")
}

/// Each `bar://` link in `s`: its URL and the plain text inside.
fn links(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("\x1b]8;;bar://") {
        let after = &rest[i + "\x1b]8;;".len()..];
        let end = after.find("\x1b\\").unwrap();
        let body = &after[end + 2..];
        let close = body.find(CLOSE).unwrap();
        out.push((after[..end].to_string(), strip(&body[..close])));
        rest = &body[close + CLOSE.len()..];
    }
    out
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            // OSC runs to ST (ESC \), SGR to its `m`
            let end = if it.next() == Some(']') { '\\' } else { 'm' };
            for d in it.by_ref() {
                if d == end {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What the handler does with a `bar://toggle/<key>` link.
fn click(fx: &Fx, arg: &str) {
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", arg])
        .env("BAR_STATE_DIR", &fx.state)
        .assert()
        .success();
}

fn show(fx: &Fx, s: &Session) -> String {
    show_with(fx, s, &[])
}

fn show_with(fx: &Fx, s: &Session, o: &[(&str, &str)]) -> String {
    let c = cfg("");
    fx.prime_in(s, "tlc", &c, o);
    fx.show_in(s, "tlc", &c, o)
}

#[test]
fn count_click_flips_scope_and_mark_click_shows_the_track() {
    let fx = Fx::new();
    let proj = setup(&fx);
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
    record(&fx, SID, "T-1", &proj);
    let s = in_session(SID, &proj);
    let shown = show(&fx, &s);
    assert_eq!(
        links(&shown),
        vec![
            (url("tlc", SID), "\u{25c9}".into()),
            (url("tlc.scope", SID), "1\u{2215}2".into())
        ],
        "{shown:?}"
    );
    // the count: track -> project
    click(&fx, &format!("tlc.scope?sid={SID}"));
    assert_eq!(strip(&show(&fx, &s)), "\u{2630} 3\u{2215}3");
    // and back
    click(&fx, &format!("tlc.scope?sid={SID}"));
    assert_eq!(strip(&show(&fx, &s)), "\u{25c9} 1\u{2215}2");
    // the mark: the track name, then the count again
    click(&fx, &format!("tlc?sid={SID}"));
    assert_eq!(strip(&show(&fx, &s)), "\u{25c9} alpha-slug");
    click(&fx, &format!("tlc?sid={SID}"));
    assert_eq!(strip(&show(&fx, &s)), "\u{25c9} 1\u{2215}2");
}

#[test]
fn count_click_without_a_session_track_keeps_the_project() {
    let fx = Fx::new();
    let proj = setup(&fx);
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
    record(&fx, SID, "T-2", &proj);
    let s = in_session(SID, &proj);
    let all = [("scope", "all")];
    assert_eq!(strip(&show_with(&fx, &s, &all)), "\u{2630} 3\u{2215}3");
    // project -> track wanted, but the session has none: project stays
    click(&fx, &format!("tlc.scope?sid={SID}"));
    let shown = show_with(&fx, &s, &all);
    assert_eq!(strip(&shown), "\u{2630} 3\u{2215}3");
    assert_eq!(links(&shown)[1].0, url("tlc.scope", SID));
}

#[test]
fn global_scope_toggle_still_flips_sessions_without_their_own() {
    let fx = Fx::new();
    let proj = setup(&fx);
    record(&fx, SID, "T-1", &proj);
    record(&fx, SID_B, "T-1", &proj);
    let (a, b) = (in_session(SID, &proj), in_session(SID_B, &proj));
    // A flips its own scope back and forth: its state wins from now on
    click(&fx, &format!("tlc.scope?sid={SID}"));
    click(&fx, &format!("tlc.scope?sid={SID}"));
    assert_eq!(toggle(&fx.state, "tlc.scope"), Ok(true));
    assert_eq!(strip(&show(&fx, &b)), "\u{2630} 3\u{2215}3");
    assert_eq!(strip(&show(&fx, &a)), "\u{25c9} 1\u{2215}2");
}
