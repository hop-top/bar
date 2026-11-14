//! `bar-tlc`'s track chooser through the core: with `action tlc tlc.pick`
//! bound, a click on the mark runs the `pick` action, which flips
//! `tlc.scope` and, on the way to track scope with two or more tracks to
//! offer, asks which track first.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;
#[path = "common/osc_links.rs"]
mod osc_links;
use osc_links::{links, strip};

use hop_bar::click::action::{self, NONCE_FILE, STATUS_FILE};
use hop_bar::click::hook::read_record;
use hop_bar::conf::Config;
use hop_bar::session::Session;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
const SID_B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const TRACK: &str = "\u{25c9}";
const ALL: &str = "\u{2630}";

struct T {
    fx: Fx,
    fake: PathBuf,
    proj: PathBuf,
    cfg: Config,
}

/// `bar-tlc` behind a wrapper with a fake `tlc` and chooser first on
/// `PATH`. Tracks `alpha` (1 of 2 closed), `beta` (2 of 4), `gamma` (0 of
/// 1); the project 3 of 3. `T-1` is on `alpha`, `T-2` has no track. The
/// active tracks are `active`. `chooser: false` leaves the chooser out
/// and makes `uname` say Linux, as on a system without one.
fn setup(active: &[&str], chooser: bool) -> T {
    let fx = Fx::new();
    let fake = fx.root.join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    let d = fake.display();
    common::exe::write(
        &fake.join("tlc"),
        format!(
            r#"#!/bin/sh
d='{d}'
echo "$*" >> "$d/calls"
case "$*" in
  "task show T-1 "*) echo '{{"task":{{"id":"T-1","track_id":"alpha"}}}}' ;;
  "task show T-2 "*) echo '{{"task":{{"id":"T-2","track_id":null}}}}' ;;
  "track show "*) set -- $*; echo "{{\"id\":\"$3\",\"slug\":\"$3-slug\"}}" ;;
  "track list --status active --format json") cat "$d/active.json" ;;
  *"--track alpha"*) echo '[{{"status":"DONE"}},{{"status":"TODO"}}]' ;;
  *"--track beta"*) echo '[{{"status":"DONE"}},{{"status":"DONE"}},{{"status":"TODO"}},{{"status":"TODO"}}]' ;;
  *"--track gamma"*) echo '[{{"status":"TODO"}}]' ;;
  "task list"*) echo '[{{"status":"DONE"}},{{"status":"DONE"}},{{"status":"CANCELLED"}}]' ;;
  *) exit 1 ;;
esac
"#
        ),
    );
    // args: title, prompt, default item, items; replies with item `choose`
    // (1-based), nothing (Cancel) without it
    common::exe::write(
        &fake.join("chooser"),
        format!(
            r#"#!/bin/sh
d='{d}'
printf '%s\n' "$@" > "$d/chooser.args"
n=$(cat "$d/choose" 2>/dev/null)
[ -n "$n" ] || exit 0
case $n in *[!0-9]*) printf '%s\n' "$n"; exit 0 ;; esac
shift 3
eval "printf '%s\n' \"\${{$n}}\""
"#
        ),
    );
    let env = if chooser {
        format!("BAR_TLC_CHOOSER='{d}/chooser'\nexport BAR_TLC_CHOOSER\n")
    } else {
        common::exe::write(&fake.join("uname"), "#!/bin/sh\necho Linux\n");
        "unset BAR_TLC_CHOOSER\n".to_string()
    };
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/bar-tlc");
    common::exe::write(
        &fx.bin.join("bar-tlc"),
        format!(
            "#!/bin/sh\nPATH='{d}':$PATH\nexport PATH\n{env}exec '{}' \"$@\"\n",
            real.display()
        ),
    );
    let t = T {
        fake,
        proj: fx.root.join("proj"),
        cfg: cfg("action tlc tlc.pick\n"),
        fx,
    };
    t.active(active);
    std::fs::create_dir_all(t.proj.join(".git")).unwrap();
    std::fs::create_dir_all(t.proj.join(".tlc")).unwrap();
    std::fs::write(t.proj.join(".tlc/config.yaml"), "").unwrap();
    std::fs::write(t.fx.state.join("click-installed"), "cmd").unwrap();
    let n = t.fx.state.join(NONCE_FILE);
    std::fs::write(&n, NONCE).unwrap();
    std::fs::set_permissions(&n, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    t
}

impl T {
    fn active(&self, ids: &[&str]) {
        let v: Vec<Value> = ids
            .iter()
            .map(|i| json!({"id": i, "slug": format!("{i}-slug"), "title": format!("Track {i}"), "status": "active"}))
            .collect();
        std::fs::write(self.fake.join("active.json"), json!(v).to_string()).unwrap();
    }

    fn record(&self, sid: &str, task: &str) {
        let dir = self.fx.state.join("records/tlc");
        std::fs::create_dir_all(&dir).unwrap();
        let r = json!({"task": task, "cwd": self.proj, "at": 1});
        std::fs::write(dir.join(sid), r.to_string()).unwrap();
    }

    fn session(&self, sid: &str) -> Session {
        Session {
            session_id: sid.into(),
            cwd: self.proj.display().to_string(),
            ..session()
        }
    }

    /// Runs the render once, then draws it.
    fn show(&self, sid: &str) -> String {
        let s = self.session(sid);
        self.fx.prime_in(&s, "tlc", &self.cfg, &[]);
        self.fx.show_in(&s, "tlc", &self.cfg, &[])
    }

    /// A click on the mark: the handler runs the link around it.
    fn click_mark(&self, sid: &str) -> Result<(), String> {
        let shown = self.show(sid);
        let (url, mark) = links(&shown).first().cloned().expect("a linked mark");
        assert!(mark == TRACK || mark == ALL, "{shown:?}");
        action::run(&self.fx.state, &url, &self.cfg, &self.fx.dirs())
    }

    fn choose(&self, n: Option<usize>) {
        let f = self.fake.join("choose");
        match n {
            Some(n) => std::fs::write(f, n.to_string()).unwrap(),
            None => {
                let _ = std::fs::remove_file(f);
            }
        }
    }

    /// What the chooser was shown (title, prompt, default, items), and
    /// forgets it.
    fn offered(&self) -> Option<Vec<String>> {
        let f = self.fake.join("chooser.args");
        let s = std::fs::read_to_string(&f).ok()?;
        std::fs::remove_file(f).unwrap();
        Some(s.lines().map(String::from).collect())
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.fake.join("calls")).unwrap_or_default()
    }

    fn picked(&self, sid: &str) -> Option<String> {
        read_record(&self.fx.state, "tlc", sid)?["track"]
            .as_str()
            .map(String::from)
    }

    fn status(&self) -> String {
        std::fs::read_to_string(self.fx.state.join(STATUS_FILE)).unwrap_or_default()
    }
}

fn shown(t: &T, sid: &str) -> String {
    strip(&t.show(sid))
}

#[test]
fn the_mark_runs_pick_and_the_count_keeps_its_toggle() {
    let t = setup(&["alpha", "beta", "gamma"], true);
    t.record(SID, "T-1");
    let l = links(&t.show(SID));
    assert_eq!(l.len(), 2, "{l:?}");
    let u = action::parse(&l[0].0).unwrap();
    assert_eq!((u.plugin.as_str(), u.action.as_str()), ("tlc", "pick"));
    assert_eq!(u.tokens["session"], SID);
    assert_eq!(u.tokens["to"], "all");
    assert_eq!(l[0].1, TRACK);
    assert_eq!(
        l[1],
        (format!("bar://toggle/tlc?sid={SID}"), "1\u{2215}2".into())
    );
}

#[test]
fn two_active_tracks_open_the_chooser_on_the_way_to_track_scope() {
    let t = setup(&["alpha", "beta", "gamma"], true);
    t.record(SID, "T-1");
    assert_eq!(shown(&t, SID), format!("{TRACK} 1\u{2215}2"));
    // track -> all: no chooser
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    // all -> track: the session's track first and preselected, then the
    // other active ones
    t.choose(Some(2));
    t.click_mark(SID).unwrap();
    let o = t.offered().expect("chooser shown");
    assert_eq!(o[2], o[3], "preselected: {o:?}");
    assert!(o[3].contains("alpha-slug"), "{o:?}");
    assert!(
        o[4].contains("beta-slug") && o[5].contains("gamma-slug"),
        "{o:?}"
    );
    assert_eq!(o.len(), 6, "{o:?}");
    assert_eq!(t.picked(SID).as_deref(), Some("beta"));
    assert_eq!(shown(&t, SID), format!("{TRACK} 2\u{2215}4"));
    // the count still swaps to the name, of the picked track
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", &format!("tlc?sid={SID}")])
        .env("BAR_STATE_DIR", &t.fx.state)
        .assert()
        .success();
    assert_eq!(shown(&t, SID), format!("{TRACK} beta-slug"));
}

#[test]
fn scope_all_clears_the_pick_and_cancel_changes_nothing() {
    let t = setup(&["alpha", "beta"], true);
    t.record(SID, "T-2");
    // no session track: wanted track, two active, no pick yet: the project,
    // and the mark asks which track, keeping the scope wanted
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    let l = links(&t.show(SID));
    assert_eq!(action::parse(&l[0].0).unwrap().tokens["to"], "pick");
    t.choose(Some(2));
    t.click_mark(SID).unwrap();
    let o = t.offered().unwrap();
    assert!(
        o[3].contains("alpha-slug") && o[4].contains("beta-slug"),
        "{o:?}"
    );
    assert_eq!(shown(&t, SID), format!("{TRACK} 2\u{2215}4"));
    assert!(!hop_bar::click::is_toggled_in(
        &t.fx.state,
        "tlc.scope",
        SID
    ));
    // back to all: the pick is gone
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    assert_eq!(t.picked(SID), None);
    assert_eq!(read_record(&t.fx.state, "tlc", SID).unwrap()["task"], "T-2");
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    // to track again, cancelled: scope and record stay as they were
    t.choose(None);
    t.click_mark(SID).unwrap();
    assert!(t.offered().is_some());
    assert!(t.status().contains("ok: cancelled"), "{}", t.status());
    assert_eq!(t.picked(SID), None);
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    let l = links(&t.show(SID));
    assert_eq!(action::parse(&l[0].0).unwrap().tokens["to"], "track");
}

#[test]
fn one_track_to_offer_needs_no_chooser() {
    // one active track, no session track: that track is counted
    let t = setup(&["gamma"], true);
    t.record(SID, "T-2");
    assert_eq!(shown(&t, SID), format!("{TRACK} 0\u{2215}1"));
    t.click_mark(SID).unwrap();
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    assert_eq!(shown(&t, SID), format!("{TRACK} 0\u{2215}1"));
    // the session's track is the only active one
    let t = setup(&["alpha"], true);
    t.record(SID, "T-1");
    t.click_mark(SID).unwrap();
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    assert_eq!(shown(&t, SID), format!("{TRACK} 1\u{2215}2"));
}

#[test]
fn a_session_track_and_another_active_one_are_offered() {
    let t = setup(&["gamma"], true);
    t.record(SID, "T-1");
    t.click_mark(SID).unwrap();
    t.choose(Some(1));
    t.click_mark(SID).unwrap();
    let o = t.offered().expect("chooser shown");
    assert_eq!(o.len(), 5, "{o:?}");
    assert!(
        o[3].contains("alpha-slug") && o[4].contains("gamma-slug"),
        "{o:?}"
    );
    assert_eq!(shown(&t, SID), format!("{TRACK} 1\u{2215}2"));
}

#[test]
fn no_active_track_counts_the_project() {
    let t = setup(&[], true);
    t.record(SID, "T-2");
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    t.click_mark(SID).unwrap();
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
}

#[test]
fn the_render_lists_tracks_only_without_a_track_to_count() {
    let t = setup(&["alpha", "beta"], true);
    t.record(SID, "T-1");
    t.show(SID);
    assert!(!t.calls().contains("track list"), "{}", t.calls());
    t.click_mark(SID).unwrap(); // to all
    std::fs::remove_file(t.fake.join("calls")).unwrap();
    t.show(SID);
    assert!(!t.calls().contains("track list"), "{}", t.calls());
    // no session track: one list to find a single active track
    t.record(SID_B, "T-2");
    t.show(SID_B);
    assert_eq!(t.calls().matches("track list").count(), 1, "{}", t.calls());
}

#[test]
fn the_global_scope_toggle_never_opens_the_chooser() {
    let t = setup(&["alpha", "beta"], true);
    t.record(SID, "T-1");
    t.choose(Some(2));
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", "tlc.scope"])
        .env("BAR_STATE_DIR", &t.fx.state)
        .assert()
        .success();
    assert_eq!(shown(&t, SID), format!("{ALL} 3\u{2215}3"));
    assert_eq!(t.offered(), None);
    assert_eq!(t.picked(SID), None);
}

#[test]
fn without_a_chooser_the_mark_stays_a_toggle() {
    let t = setup(&["alpha", "beta"], false);
    assert!(t.fx.ext_info("tlc").actions.is_empty());
    t.record(SID, "T-1");
    let l = links(&t.show(SID));
    assert_eq!(
        l[0],
        (format!("bar://toggle/tlc.scope?sid={SID}"), TRACK.into())
    );
}

/// The action run directly, as the handler would, bounded by the
/// deadline helper.
fn act(t: &T, input: &Value) -> std::process::Output {
    let mut c = std::process::Command::new(t.fx.bin.join("bar-tlc"));
    c.args(["--action", "pick"])
        .env("BAR_STATE_DIR", &t.fx.state);
    common::deadline::run(&mut c, Some(input.to_string().as_bytes()))
}

#[test]
fn the_pick_action_refuses_bad_input() {
    let t = setup(&["alpha", "beta"], true);
    t.choose(Some(1));
    let root = t.proj.display().to_string();
    let ok = json!({"action": "pick", "tokens": {"session": SID, "to": "track"}, "output": {"root": root}});
    for bad in [
        json!({"action": "use", "tokens": {"session": SID, "to": "track"}, "output": {"root": root}}),
        json!({"action": "pick", "tokens": {"session": "../x", "to": "track"}, "output": {"root": root}}),
        json!({"action": "pick", "tokens": {"session": "NOT-A-SID", "to": "track"}, "output": {"root": root}}),
        json!({"action": "pick", "tokens": {"session": SID, "to": "both"}, "output": {"root": root}}),
        json!({"action": "pick", "tokens": {"session": SID, "to": "track"}, "output": {"root": "rel"}}),
        json!({"action": "pick", "tokens": {"session": SID, "to": "track"}, "output": {"root": "/nonexistent-tlc-root"}}),
        json!({"action": "pick", "tokens": {"session": SID, "to": "track"}}),
    ] {
        let o = act(&t, &bad);
        assert!(!o.status.success(), "accepted {bad}");
        assert_eq!(t.picked(SID), None, "{bad}");
    }
    // a reply that was not offered
    std::fs::write(t.fake.join("choose"), "9. evil-slug").unwrap();
    assert!(!act(&t, &ok).status.success());
    assert_eq!(t.picked(SID), None);
    t.choose(Some(1));
    let o = act(&t, &ok);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(t.picked(SID).as_deref(), Some("alpha"));
    // a planted symlink for the records dir is refused
    let t = setup(&["alpha", "beta"], true);
    t.choose(Some(1));
    std::fs::create_dir_all(t.fx.state.join("records")).unwrap();
    let elsewhere = t.fx.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, t.fx.state.join("records/tlc")).unwrap();
    let ok = json!({"action": "pick", "tokens": {"session": SID, "to": "track"}, "output": {"root": t.proj}});
    assert!(!act(&t, &ok).status.success());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    // and for the records dir above it
    let t = setup(&["alpha", "beta"], true);
    t.choose(Some(1));
    let elsewhere = t.fx.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, t.fx.state.join("records")).unwrap();
    let ok = json!({"action": "pick", "tokens": {"session": SID, "to": "track"}, "output": {"root": t.proj}});
    assert!(!act(&t, &ok).status.success());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}
