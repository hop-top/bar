//! `bar-tlc` with a fake `tlc` and a stand-in chooser, for the track
//! chooser's tests.
#![allow(dead_code)]

use crate::common;
use crate::osc_links::{links, strip};
use crate::plugin_fx::*;
use hop_bar::click::action::{self, NONCE_FILE, STATUS_FILE};
use hop_bar::click::hook::read_record;
use hop_bar::conf::Config;
use hop_bar::session::Session;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
pub const SID_B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";
pub const NONCE: &str = "0123456789abcdef0123456789abcdef";
pub const TRACK: &str = "\u{25c9}";
pub const ALL: &str = "\u{2630}";

pub struct T {
    pub fx: Fx,
    pub fake: PathBuf,
    pub proj: PathBuf,
    pub cfg: Config,
    /// The placeholder's options.
    pub opts: Vec<(&'static str, String)>,
}

/// `bar-tlc` behind a wrapper with a fake `tlc` and chooser first on
/// `PATH`. Tracks `alpha` (1 of 2 closed), `beta` (2 of 4), `gamma` (0 of
/// 1); the project 4 of 8 (their tasks and one closed without track). `T-1` is on `alpha`, `T-2` has no track. The
/// active tracks are `active`. `chooser: false` makes `uname` say
/// Linux, as on a system without one.
pub fn setup(active: &[&str], chooser: bool) -> T {
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
  "task list"*) echo '[{{"status":"DONE","track_id":"alpha"}},{{"status":"TODO","track_id":"alpha"}},{{"status":"DONE","track_id":"beta"}},{{"status":"DONE","track_id":"beta"}},{{"status":"TODO","track_id":"beta"}},{{"status":"TODO","track_id":"beta"}},{{"status":"TODO","track_id":"gamma"}},{{"status":"CANCELLED","track_id":null}}]' ;;
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
# as osascript: -e <script> -- <args>
[ "$1" = -e ] || exit 9
shift 2
[ "$1" = -- ] || exit 9
shift
printf '%s\n' "$@" > "$d/chooser.args"
n=$(cat "$d/choose" 2>/dev/null)
[ -n "$n" ] || exit 0
case $n in *[!0-9]*) printf '%s\n' "$n"; exit 0 ;; esac
shift 3
eval "printf '%s\n' \"\${{$n}}\""
"#
        ),
    );
    // a copy of the plugin whose chooser is the fake: the one constant
    // naming osascript, rewritten
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/bar-tlc");
    let src = std::fs::read_to_string(real).unwrap();
    let constant = "\nOSASCRIPT=/usr/bin/osascript\n";
    assert_eq!(src.matches(constant).count(), 1, "one osascript constant");
    assert_eq!(src.matches("/usr/bin/osascript").count(), 1, "named once");
    let copy = src.replace(constant, &format!("\nOSASCRIPT='{d}/chooser'\n"));
    assert!(!copy.contains("/usr/bin/osascript"));
    common::exe::write(&fake.join("bar-tlc"), copy);
    // the chooser exists only on macOS: `uname` says which
    let os = if chooser { "Darwin" } else { "Linux" };
    common::exe::write(&fake.join("uname"), format!("#!/bin/sh\necho {os}\n"));
    common::exe::write(
        &fx.bin.join("bar-tlc"),
        format!("#!/bin/sh\nPATH='{d}':$PATH\nexport PATH\nexec '{d}/bar-tlc' \"$@\"\n"),
    );
    let t = T {
        fake,
        proj: fx.root.join("proj"),
        cfg: cfg("action tlc tlc.pick\n"),
        opts: Vec::new(),
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
    pub fn active(&self, ids: &[&str]) {
        let v: Vec<Value> = ids
            .iter()
            .map(|i| json!({"id": i, "slug": format!("{i}-slug"), "title": format!("Track {i}"), "status": "active"}))
            .collect();
        std::fs::write(self.fake.join("active.json"), json!(v).to_string()).unwrap();
    }

    pub fn record(&self, sid: &str, task: &str) {
        let dir = self.fx.state.join("records/tlc");
        std::fs::create_dir_all(&dir).unwrap();
        let r = json!({"task": task, "cwd": self.proj, "at": 1});
        std::fs::write(dir.join(sid), r.to_string()).unwrap();
    }

    pub fn session(&self, sid: &str) -> Session {
        Session {
            session_id: sid.into(),
            cwd: self.proj.display().to_string(),
            ..session()
        }
    }

    /// Runs the render once, then draws it.
    pub fn show(&self, sid: &str) -> String {
        let s = self.session(sid);
        let o: Vec<(&str, &str)> = self.opts.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.fx.prime_in(&s, "tlc", &self.cfg, &o);
        self.fx.show_in(&s, "tlc", &self.cfg, &o)
    }

    /// A click on the mark: the handler runs the link around it.
    pub fn click_mark(&self, sid: &str) -> Result<(), String> {
        let shown = self.show(sid);
        let (url, mark) = links(&shown).first().cloned().expect("a linked mark");
        assert!(mark == TRACK || mark == ALL, "{shown:?}");
        action::run(&self.fx.state, &url, &self.cfg, &self.fx.dirs())
    }

    pub fn choose(&self, n: Option<usize>) {
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
    pub fn offered(&self) -> Option<Vec<String>> {
        let f = self.fake.join("chooser.args");
        let s = std::fs::read_to_string(&f).ok()?;
        std::fs::remove_file(f).unwrap();
        Some(s.lines().map(String::from).collect())
    }

    pub fn calls(&self) -> String {
        std::fs::read_to_string(self.fake.join("calls")).unwrap_or_default()
    }

    pub fn picked(&self, sid: &str) -> Option<String> {
        read_record(&self.fx.state, "tlc", sid)?["track"]
            .as_str()
            .map(String::from)
    }

    pub fn status(&self) -> String {
        std::fs::read_to_string(self.fx.state.join(STATUS_FILE)).unwrap_or_default()
    }
}

pub fn shown(t: &T, sid: &str) -> String {
    strip(&t.show(sid))
}

/// The action run directly, as the handler would, bounded by the
/// deadline helper.
pub fn act(t: &T, input: &Value) -> std::process::Output {
    let mut c = std::process::Command::new(t.fx.bin.join("bar-tlc"));
    c.args(["--action", "pick"])
        .env("BAR_STATE_DIR", &t.fx.state);
    common::deadline::run(&mut c, Some(input.to_string().as_bytes()))
}

