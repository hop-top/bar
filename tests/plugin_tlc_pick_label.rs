//! `bar-tlc`'s `pick-label`: each entry of the track chooser drawn by a
//! template, from the placeholder's options through the render's output to
//! the `pick` action.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;
#[path = "common/osc_links.rs"]
mod osc_links;
#[path = "common/tlc_pick_fx.rs"]
mod tlc_pick_fx;
use tlc_pick_fx::*;

use serde_json::{json, Value};

/// Three active tracks, the session on `alpha`, the scope on the whole
/// project: the next click on the mark opens the chooser.
fn ready(label: Option<&str>) -> T {
    let mut t = setup(&["alpha", "beta", "gamma"], true);
    if let Some(l) = label {
        t.opts.push(("pick-label", l.to_string()));
    }
    t.record(SID, "T-1");
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
    t
}

/// The entries the chooser was shown.
fn items(t: &T) -> Vec<String> {
    t.offered().expect("chooser shown")[3..].to_vec()
}

#[test]
fn the_default_shows_the_slug_and_the_closed_count() {
    let t = ready(None);
    t.choose(Some(2));
    t.click_mark(SID).unwrap();
    assert_eq!(
        items(&t),
        ["alpha-slug - 1/2", "beta-slug - 2/4", "gamma-slug - 0/1"]
    );
    assert_eq!(t.picked(SID).as_deref(), Some("beta"));
}

#[test]
fn the_placeholder_option_draws_the_entries() {
    let t = ready(Some("{n}: {title} [{status}] {done} of {total} {{{id}}}"));
    t.choose(None);
    t.click_mark(SID).unwrap();
    assert_eq!(
        items(&t),
        [
            "1: Track alpha [active] 1 of 2 {alpha}",
            "2: Track beta [active] 2 of 4 {beta}",
            "3: Track gamma [active] 0 of 1 {gamma}"
        ]
    );
    // an unknown name and lone braces stay as written
    let t = ready(Some("{slug} {nope} { }}"));
    t.choose(None);
    t.click_mark(SID).unwrap();
    assert_eq!(
        items(&t),
        [
            "alpha-slug {nope} { }",
            "beta-slug {nope} { }",
            "gamma-slug {nope} { }"
        ]
    );
}

/// The action run directly with the output a render made for `label`.
fn act_with(t: &T, label: &str, reply: &str) -> std::process::Output {
    std::fs::write(t.fake.join("choose"), reply).unwrap();
    let root = t.proj.display().to_string();
    let input = json!({"action": "pick", "tokens": {"session": SID, "to": "track"},
        "output": {"root": root, "pick_label": label}});
    act(t, &input)
}

#[test]
fn entries_that_draw_alike_are_numbered_and_map_back() {
    let t = setup(&["alpha", "beta"], true);
    let o = act_with(&t, "{status}", "2");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(items(&t), ["1. active", "2. active"]);
    assert_eq!(t.picked(SID).as_deref(), Some("beta"));
    // empty, or `false`: Cancel to the chooser, so numbered too
    act_with(&t, "  ", "");
    assert_eq!(items(&t), ["1.", "2."]);
    // an empty template is the default
    act_with(&t, "", "");
    assert_eq!(items(&t), ["alpha-slug - 1/2", "beta-slug - 2/4"]);
    act_with(&t, "false", "");
    assert_eq!(items(&t), ["1. false", "2. false"]);
    // the text before numbering was not offered
    let t = setup(&["alpha", "beta"], true);
    let o = act_with(&t, "{status}", "active");
    assert!(!o.status.success());
    assert_eq!(t.picked(SID), None);
}

#[test]
fn entries_are_sanitised_and_counted_with_one_list() {
    let t = setup(&["alpha", "beta"], true);
    let o = act_with(&t, "a\n{slug}\u{7}\"; do shell script \"x", "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // one line per argument: no newline got through
    assert_eq!(
        items(&t),
        [
            "a alpha-slug \"; do shell script \"x",
            "a beta-slug \"; do shell script \"x"
        ]
    );
    let o = act_with(&t, &"z".repeat(500), "");
    assert!(o.status.success());
    let i = items(&t);
    assert!(i.iter().all(|l| l.chars().count() == 120), "{i:?}");
    // the tasks are listed once per click, for every track at once
    let _ = std::fs::remove_file(t.fake.join("calls"));
    act_with(&t, "{total}", "");
    assert_eq!(t.calls().matches("task list").count(), 1, "{}", t.calls());
    assert!(!t.calls().contains("--track"), "{}", t.calls());
}

/// The render's output for `opts`, from the plugin itself.
fn render(t: &T, opts: Value) -> Value {
    let mut c = std::process::Command::new(t.fx.bin.join("bar-tlc"));
    c.env("BAR_STATE_DIR", &t.fx.state);
    let input = json!({"options": opts, "session": {"cwd": t.proj}});
    let o = common::deadline::run(&mut c, Some(input.to_string().as_bytes()));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn the_render_passes_on_a_template_that_is_not_the_default() {
    let t = setup(&["alpha", "beta"], true);
    assert!(render(&t, json!({})).get("pick_label").is_none());
    let d = json!({"pick-label": "{slug} - {done}/{total}"});
    assert!(render(&t, d).get("pick_label").is_none());
    let o = render(&t, json!({"pick-label": "{n}. {slug}"}));
    assert_eq!(o["pick_label"], "{n}. {slug}");
    let mut c = std::process::Command::new(t.fx.bin.join("bar-tlc"));
    let info = common::deadline::run(c.arg("--ext-info"), None);
    let info: Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(
        info["options"]["pick-label"],
        json!({"type": "string", "default": "{slug} - {done}/{total}"})
    );
}
