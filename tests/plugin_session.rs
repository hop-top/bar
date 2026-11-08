//! Session-scoped plugins (`"cache": "session"`): per-session cache keys,
//! the hook's record in `session.record`, and the generic `alt` toggle.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::click::toggle;
use hop_bar::session::Session;
use serde_json::{json, Value};

const SID_A: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
const SID_B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";

/// `bar-sess`: echoes the session id and the record's task; `bar-flat` is
/// the same without `"cache": "session"`. Both save their stdin.
fn plugins(fx: &Fx) {
    for (name, cache) in [("sess", r#","cache":"session""#), ("flat", "")] {
        let body = format!(
            r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--ext-info" ]; then
  echo '{{"name":"{name}","version":"1","capabilities":["placeholder"]{cache}}}'
  exit 0
fi
input=$(cat)
printf '%s' "$input" > "$here/{name}.stdin.json"
printf '%s' "$input" | jq -c '{{text: "\(.session.session_id[-1:])\(.session.record.task // "-")", alt: "ALT"}}'
"#
        );
        let p = fx.bin.join(format!("bar-{name}"));
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
}

fn in_session(sid: &str) -> Session {
    Session {
        session_id: sid.into(),
        ..session()
    }
}

fn record(fx: &Fx, name: &str, sid: &str, task: &str) {
    let dir = fx.state.join("records").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let r = json!({"task": task, "cwd": "/tmp/work", "at": 1});
    std::fs::write(dir.join(sid), r.to_string()).unwrap();
}

fn stdin(fx: &Fx, name: &str) -> Value {
    serde_json::from_str(&fx.file(&format!("{name}.stdin.json"))).unwrap()
}

#[test]
fn ext_info_reads_session_cache() {
    let fx = Fx::new();
    plugins(&fx);
    let s = fx.ext_info("sess");
    assert!(s.session_cache);
    assert_eq!(s.to_json()["cache"], "session");
    let f = fx.ext_info("flat");
    assert!(!f.session_cache);
    assert!(f.to_json().get("cache").is_none());
}

#[test]
fn session_plugin_is_cached_per_session_and_record() {
    let fx = Fx::new();
    plugins(&fx);
    let c = cfg("");
    let (a, b) = (in_session(SID_A), in_session(SID_B));
    record(&fx, "sess", SID_A, "T-0001");
    record(&fx, "sess", SID_B, "T-0002");
    fx.prime_in(&a, "sess", &c, &[]);
    assert_eq!(stdin(&fx, "sess")["session"]["record"]["task"], "T-0001");
    // same cwd, other session: its own entry, cold until primed
    assert_eq!(fx.show_in(&b, "sess", &c, &[]), "");
    fx.prime_in(&b, "sess", &c, &[]);
    assert_eq!(fx.show_in(&a, "sess", &c, &[]), "aT-0001");
    assert_eq!(fx.show_in(&b, "sess", &c, &[]), "bT-0002");
    // a new record is a new key, so it shows at the next refresh
    record(&fx, "sess", SID_A, "T-0003");
    assert_eq!(fx.show_in(&a, "sess", &c, &[]), "");
    fx.prime_in(&a, "sess", &c, &[]);
    assert_eq!(fx.show_in(&a, "sess", &c, &[]), "aT-0003");
    // no record: sent without one
    let none = in_session("0a1b2c3d-0000-4000-8000-00000000000c");
    fx.prime_in(&none, "sess", &c, &[]);
    assert!(stdin(&fx, "sess")["session"].get("record").is_none());
    assert_eq!(fx.show_in(&none, "sess", &c, &[]), "c-");
    // nor do two sessions without a record share an entry
    let other = in_session("0a1b2c3d-0000-4000-8000-00000000000d");
    assert_eq!(fx.show_in(&other, "sess", &c, &[]), "");
    fx.prime_in(&other, "sess", &c, &[]);
    assert_eq!(fx.show_in(&other, "sess", &c, &[]), "d-");
    assert_eq!(fx.show_in(&none, "sess", &c, &[]), "c-");
}

#[test]
fn other_plugins_share_one_entry_and_get_no_record() {
    let fx = Fx::new();
    plugins(&fx);
    let c = cfg("");
    record(&fx, "flat", SID_A, "T-0001");
    fx.prime_in(&in_session(SID_A), "flat", &c, &[]);
    assert!(stdin(&fx, "flat")["session"].get("record").is_none());
    assert_eq!(fx.show_in(&in_session(SID_A), "flat", &c, &[]), "a-");
    assert_eq!(fx.show_in(&in_session(SID_B), "flat", &c, &[]), "a-");
}

#[test]
fn alt_shows_while_toggled_and_links_with_handler() {
    let fx = Fx::new();
    plugins(&fx);
    let c = cfg("");
    let s = in_session(SID_A);
    fx.prime_in(&s, "sess", &c, &[]);
    assert_eq!(fx.show_in(&s, "sess", &c, &[]), "a-");
    assert_eq!(toggle(&fx.state, "sess"), Ok(true));
    assert_eq!(fx.show_in(&s, "sess", &c, &[]), "ALT");
    // the toggle state is per plugin
    fx.prime_in(&s, "flat", &c, &[]);
    assert_eq!(fx.show_in(&s, "flat", &c, &[]), "a-");
    assert_eq!(toggle(&fx.state, "sess"), Ok(false));
    assert_eq!(fx.show_in(&s, "sess", &c, &[]), "a-");

    // no handler: no link
    assert!(!fx.show_in(&s, "sess", &c, &[]).contains("\x1b]8;"));
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
    let linked = fx.show_in(&s, "sess", &c, &[]);
    assert!(
        linked.contains("\x1b]8;;bar://toggle/sess\x1b\\"),
        "{linked:?}"
    );
    // click is bar's own option: not sent, not rejected
    fx.prime_in(&s, "sess", &c, &[("click", "none")]);
    assert!(stdin(&fx, "sess")["options"].get("click").is_none());
    let plain = fx.show_in(&s, "sess", &c, &[("click", "none")]);
    assert_eq!(plain, "a-");
}

// ---- label toggle ----

/// `bar-mark`: a label with `label_toggle: scope`, text follows the toggle
/// the core passes in `toggles`; saves its stdin.
fn mark_plugin(fx: &Fx, toggle: &str) {
    let body = format!(
        r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--ext-info" ]; then
  echo '{{"name":"mark","version":"1","capabilities":["placeholder"]}}'
  exit 0
fi
input=$(cat)
printf '%s' "$input" > "$here/mark.stdin.json"
printf '%s' "$input" | jq -c '{{label: "L", icon: "I", label_toggle: "{toggle}", text: (if .toggles.scope then "all" else "trk" end)}}'
"#
    );
    let p = fx.bin.join("bar-mark");
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// Drops SGR sequences.
fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            for d in it.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn label_toggle_links_label_only_with_handler_and_click() {
    let fx = Fx::new();
    mark_plugin(&fx, "scope");
    let c = cfg("");
    let s = session();
    fx.prime_in(&s, "mark", &c, &[]);
    let plain = fx.show_in(&s, "mark", &c, &[]);
    assert!(!plain.contains("\x1b]8;"), "{plain:?}");
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
    let linked = fx.show_in(&s, "mark", &c, &[]);
    let open = "\x1b]8;;bar://toggle/mark.scope\x1b\\";
    assert!(linked.contains(open), "{linked:?}");
    // the link wraps the label, not the text
    let after = linked.split("\x1b]8;;\x1b\\").next().unwrap();
    assert!(after.contains("L") && !after.contains("trk"), "{linked:?}");
    assert!(linked.ends_with("trk"), "{linked:?}");
    // click=none drops it
    fx.prime_in(&s, "mark", &c, &[("click", "none")]);
    let off = fx.show_in(&s, "mark", &c, &[("click", "none")]);
    assert!(!off.contains("\x1b]8;"), "{off:?}");
}

#[test]
fn label_toggle_invalid_name_is_dropped() {
    let fx = Fx::new();
    mark_plugin(&fx, "a b/..");
    let c = cfg("");
    let s = session();
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
    fx.prime_in(&s, "mark", &c, &[]);
    let shown = fx.show_in(&s, "mark", &c, &[]);
    assert!(!shown.contains("\x1b]8;"), "{shown:?}");
}

#[test]
fn toggles_reach_stdin_split_the_cache_and_stay_per_plugin() {
    let fx = Fx::new();
    mark_plugin(&fx, "scope");
    plugins(&fx);
    let c = cfg("");
    let s = session();
    fx.prime_in(&s, "mark", &c, &[]);
    assert_eq!(stdin(&fx, "mark")["toggles"], json!({}));
    assert_eq!(plain_text(&fx.show_in(&s, "mark", &c, &[])), "L trk");
    assert_eq!(toggle(&fx.state, "mark.scope"), Ok(true));
    // another plugin's toggle and the text toggle do not count
    assert_eq!(toggle(&fx.state, "sess.scope"), Ok(true));
    assert_eq!(toggle(&fx.state, "mark"), Ok(true));
    fx.prime_in(&s, "mark", &c, &[]);
    assert_eq!(stdin(&fx, "mark")["toggles"], json!({"scope": true}));
    // new cache entry: shows without waiting for the ttl
    assert_eq!(plain_text(&fx.show_in(&s, "mark", &c, &[])), "L all");
    fx.prime_in(&s, "sess", &c, &[]);
    assert_eq!(stdin(&fx, "sess")["toggles"], json!({"scope": true}));
    assert_eq!(toggle(&fx.state, "mark.scope"), Ok(false));
    assert_eq!(plain_text(&fx.show_in(&s, "mark", &c, &[])), "L trk");
}
