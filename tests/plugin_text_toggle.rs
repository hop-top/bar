//! `text_toggle`: the text links to a named toggle and the `alt` toggle
//! moves to the label or icon; outputs without it link as before.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::click::toggle;

const SID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";

/// `bar-tt`: prints `extra` merged into a label, text and alt; the text
/// follows the `scope` toggle the core passes in `toggles`.
fn tt_plugin(fx: &Fx, extra: &str) {
    let body = format!(
        r#"#!/bin/sh
if [ "$1" = "--ext-info" ]; then
  echo '{{"name":"tt","version":"1","capabilities":["placeholder"],"cache":"session"}}'
  exit 0
fi
cat | jq -c '{{label: "L", text: (if .toggles.scope then "all" else "trk" end), alt: "NAME"}} + {extra}'
"#
    );
    common::exe::write(&fx.bin.join("bar-tt"), body);
}

fn sess() -> hop_bar::session::Session {
    hop_bar::session::Session {
        session_id: SID.into(),
        ..session()
    }
}

const CLOSE: &str = "\x1b]8;;\x1b\\";

/// The URL of each `bar://` OSC 8 link in `s`, in order, and the plain text in it.
fn links(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("\x1b]8;;bar://") {
        let after = &rest[i + "\x1b]8;;".len()..];
        let end = after.find("\x1b\\").unwrap();
        let url = after[..end].to_string();
        let body_start = &after[end + 2..];
        let close = body_start.find(CLOSE).unwrap();
        out.push((url, strip(&body_start[..close])));
        rest = &body_start[close + CLOSE.len()..];
    }
    out
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            // OSC runs to ST (ESC \\), SGR to its `m`
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

fn click(fx: &Fx, key: &str) {
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", &format!("{key}?sid={SID}")])
        .env("BAR_STATE_DIR", &fx.state)
        .assert()
        .success();
}

fn installed(fx: &Fx) {
    std::fs::write(fx.state.join("click-installed"), "cmd").unwrap();
}

fn url(key: &str) -> String {
    format!("bar://toggle/{key}?sid={SID}")
}

#[test]
fn text_toggle_links_text_to_named_toggle_and_alt_to_mark() {
    let fx = Fx::new();
    tt_plugin(&fx, r#"{text_toggle: "scope"}"#);
    installed(&fx);
    let (c, s) = (cfg(""), sess());
    fx.prime_in(&s, "tt", &c, &[]);
    let shown = fx.show_in(&s, "tt", &c, &[]);
    assert_eq!(
        links(&shown),
        vec![(url("tt"), "L".into()), (url("tt.scope"), "trk".into())],
        "{shown:?}"
    );
    // a click on the text flips the named toggle: the plugin reruns
    click(&fx, "tt.scope");
    fx.prime_in(&s, "tt", &c, &[]);
    assert_eq!(strip(&fx.show_in(&s, "tt", &c, &[])), "L all");
    // a click on the mark flips the alt toggle: shown at once
    click(&fx, "tt");
    assert_eq!(strip(&fx.show_in(&s, "tt", &c, &[])), "L NAME");
    click(&fx, "tt");
    assert_eq!(strip(&fx.show_in(&s, "tt", &c, &[])), "L all");
}

#[test]
fn text_toggle_with_label_toggle_keeps_the_label_toggle_on_the_mark() {
    let fx = Fx::new();
    tt_plugin(&fx, r#"{text_toggle: "scope", label_toggle: "other"}"#);
    installed(&fx);
    let (c, s) = (cfg(""), sess());
    fx.prime_in(&s, "tt", &c, &[]);
    let shown = fx.show_in(&s, "tt", &c, &[]);
    assert_eq!(
        links(&shown),
        vec![
            (url("tt.other"), "L".into()),
            (url("tt.scope"), "trk".into())
        ],
        "{shown:?}"
    );
}

#[test]
fn text_toggle_yields_to_the_outputs_own_link() {
    let fx = Fx::new();
    tt_plugin(&fx, r#"{text_toggle: "scope", link: "https://x.test/"}"#);
    installed(&fx);
    let (c, s) = (cfg(""), sess());
    fx.prime_in(&s, "tt", &c, &[]);
    let shown = fx.show_in(&s, "tt", &c, &[]);
    assert_eq!(links(&shown), vec![(url("tt"), "L".into())], "{shown:?}");
    assert!(
        shown.contains("\x1b]8;;https://x.test/\x1b\\trk"),
        "{shown:?}"
    );
}

#[test]
fn text_toggle_needs_handler_and_click() {
    let fx = Fx::new();
    tt_plugin(&fx, r#"{text_toggle: "scope"}"#);
    let (c, s) = (cfg(""), sess());
    fx.prime_in(&s, "tt", &c, &[]);
    assert!(!fx.show_in(&s, "tt", &c, &[]).contains("\x1b]8;"));
    installed(&fx);
    fx.prime_in(&s, "tt", &c, &[("click", "none")]);
    let off = fx.show_in(&s, "tt", &c, &[("click", "none")]);
    assert!(!off.contains("\x1b]8;"), "{off:?}");
}

#[test]
fn without_text_toggle_the_links_are_unchanged() {
    let fx = Fx::new();
    let (c, s) = (cfg(""), sess());
    installed(&fx);
    // the pre-existing shape: label toggle on the mark, alt toggle on the text
    tt_plugin(&fx, r#"{label_toggle: "scope"}"#);
    fx.prime_in(&s, "tt", &c, &[]);
    let plain = fx.show_in(&s, "tt", &c, &[]);
    // bytes as rendered before `text_toggle` existed
    let before = format!(
        "\x1b]8;;{}\x1b\\\x1b[38;5;250mL\x1b[39m{CLOSE} \x1b]8;;{}\x1b\\trk{CLOSE}",
        url("tt.scope"),
        url("tt")
    );
    assert_eq!(plain, before);
    assert_eq!(
        links(&plain),
        vec![(url("tt.scope"), "L".into()), (url("tt"), "trk".into())]
    );
    // an invalid name is ignored: same bytes as no field at all
    tt_plugin(&fx, r#"{label_toggle: "scope", text_toggle: "a b/.."}"#);
    fx.prime_in(&s, "tt", &c, &[]);
    assert_eq!(fx.show_in(&s, "tt", &c, &[]), plain);
    // `bar toggle tt.scope` still reaches the plugin as `toggles.scope`
    assert_eq!(toggle(&fx.state, "tt.scope"), Ok(true));
    fx.prime_in(&s, "tt", &c, &[]);
    assert!(fx.show_in(&s, "tt", &c, &[]).contains("all"));
}

#[test]
fn text_toggle_without_alt_leaves_the_mark_unlinked() {
    let fx = Fx::new();
    tt_plugin(&fx, r#"{text_toggle: "scope", alt: null}"#);
    installed(&fx);
    let (c, s) = (cfg(""), sess());
    fx.prime_in(&s, "tt", &c, &[]);
    let shown = fx.show_in(&s, "tt", &c, &[]);
    assert_eq!(
        links(&shown),
        vec![(url("tt.scope"), "trk".into())],
        "{shown:?}"
    );
}
