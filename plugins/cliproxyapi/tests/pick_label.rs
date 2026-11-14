#![cfg(unix)]
//! The `pick-label` template of the account chooser, end to end: the
//! `pick` action with the template in the output bar hands it, against a
//! fake management server and a stand-in chooser. Synthetic accounts only.

#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use fake::{pool, Server};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const KEY: &str = "pick-label-test-key";

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-pl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Home(d)
    }
    fn file(&self, name: &str, text: &str, mode: u32) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }
    fn conf(&self, s: &Server) -> std::path::PathBuf {
        let key = self.file("key", &format!("{KEY}\n"), 0o600);
        let text = format!("instance {}\n key-file {}\n", s.origin(), key.display());
        self.file("c.conf", &text, 0o600)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A claude account `name` with `used5` percent of its 5h window used.
fn acct(name: &str, used5: u32) -> Value {
    json!({"name": format!("{name}.json"), "auth_index": format!("ix-{name}"),
        "type": "claude", "email": format!("{name}@corp.test"),
        "quota": {"signals": {
            "Anthropic-Ratelimit-Unified-5h-Utilization": format!("{}", used5 as f64 / 100.0),
            "Anthropic-Ratelimit-Unified-5h-Reset": "4102444800"}}})
}

/// Runs `pick` from an output carrying `label` (none: the default); the
/// chooser answers `reply` (`cancel`, `item N`, `raw TEXT`). The exit, and
/// the chooser's arguments: title, prompt, default, items.
fn pick(h: &Home, s: &Server, label: Option<&str>, reply: &str) -> (Output, Vec<String>) {
    let log = h.0.join("chooser.log");
    let prog = h.file(
        "chooser.sh",
        r#"#!/bin/sh
for a in "$@"; do printf '%s\n' "$a"; done > "$FAKE_LOG"
case "$FAKE_REPLY" in
  cancel) exit 0 ;;
  "item "*) n=${FAKE_REPLY#item }; shift 3; shift $((n-1)); printf '%s' "$1" ;;
  "raw "*) printf '%s' "${FAKE_REPLY#raw }" ;;
esac
"#,
        0o700,
    );
    let id = bar_cliproxyapi::switch::instance_id(&s.origin());
    let mut a = json!({"action":"pick","tokens":{"provider":"claude","instance":id}});
    if let Some(l) = label {
        a["output"] = json!({"text": "", "pick_label": l});
    }
    let mut c = Command::new(BIN);
    c.args(["--action", "pick"])
        .env("BAR_CLIPROXYAPI_CONFIG", h.conf(s))
        .env("BAR_STATE_DIR", h.0.join("state"))
        .env("BAR_CLIPROXYAPI_CHOOSER", &prog)
        .env("FAKE_LOG", &log)
        .env("FAKE_REPLY", reply)
        .env_remove("ANTHROPIC_BASE_URL");
    let o = deadline::run(&mut c, Some(a.to_string().as_bytes()));
    let argv = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    (o, argv)
}

fn prio(state: &Value, name: &str) -> i64 {
    state
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == format!("{name}.json"))
        .unwrap()["priority"]
        .as_i64()
        .unwrap_or(0)
}

#[test]
fn the_default_shows_the_label_and_the_5h_percent_used() {
    let (s, _) = pool(json!([acct("work", 42), acct("home", 7)]), KEY);
    let h = Home::new("default");
    let (o, argv) = pick(&h, &s, None, "cancel");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // most left first
    assert_eq!(argv[3..], ["home - 7%", "work - 42%"]);
    assert_eq!(argv[2], "home - 7%");
}

#[test]
fn a_template_from_the_output_draws_each_entry() {
    let (s, st) = pool(json!([acct("work", 42), acct("home", 7)]), KEY);
    let h = Home::new("custom");
    let tpl = "{n}: {label} ({left}% left, {provider}) {{100}}";
    let (o, argv) = pick(&h, &s, Some(tpl), "item 2");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        argv[3..],
        [
            "1: home (93% left, claude) {100}",
            "2: work (58% left, claude) {100}"
        ]
    );
    let files = st.lock().unwrap().clone();
    assert!(prio(&files, "work") > prio(&files, "home"), "{files}");
    // an unknown placeholder stays as written
    let (_, argv) = pick(&h, &s, Some("{label} {nope} { }"), "cancel");
    assert_eq!(argv[3..], ["home {nope} { }", "work {nope} { }"]);
}

#[test]
fn entries_that_draw_alike_are_numbered_and_map_back() {
    let (s, st) = pool(json!([acct("work", 42), acct("home", 7)]), KEY);
    let h = Home::new("dupes");
    // an empty entry (no account preferred yet), as one reading `false`,
    // would read as Cancel
    let (_, argv) = pick(&h, &s, Some("{current}"), "cancel");
    assert_eq!(argv[3..], ["1.", "2."]);
    let (_, argv) = pick(&h, &s, Some("false"), "cancel");
    assert_eq!(argv[3..], ["1. false", "2. false"]);
    let (o, argv) = pick(&h, &s, Some("{provider}"), "item 2");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(argv[3..], ["1. claude", "2. claude"]);
    // the second entry is work, the account switched to
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("work (claude)"),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let files = st.lock().unwrap().clone();
    assert!(prio(&files, "work") > prio(&files, "home"), "{files}");
    // the drawn text before numbering was not offered
    let n = s.patches().len();
    let (o, _) = pick(&h, &s, Some("{provider}"), "raw claude");
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("not offered"));
    assert_eq!(s.patches().len(), n);
}

#[test]
fn entries_are_sanitised_and_stay_arguments() {
    let (s, _) = pool(json!([acct("work", 42)]), KEY);
    let h = Home::new("sanitise");
    let tpl = "a\n{label}\u{7}\"; do shell script \"x";
    let (o, argv) = pick(&h, &s, Some(tpl), "cancel");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // one line per argument: no newline got through
    assert_eq!(argv.len(), 4, "{argv:?}");
    assert_eq!(argv[3], "a work \"; do shell script \"x");
    let long = "z".repeat(500);
    let (_, argv) = pick(&h, &s, Some(&long), "cancel");
    assert_eq!(argv[3].chars().count(), bar_cliproxyapi::chooser::MAX_ITEM);
}
