//! `bar-tlc`'s track chooser offers incomplete tracks only (closed below
//! total, so never an empty one), the session's too; the render's last
//! fallback counts the only incomplete active track.

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

/// The slugs the chooser was shown, from the default entries.
fn slugs(t: &T) -> Vec<String> {
    t.offered().expect("chooser shown")[3..]
        .iter()
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

#[test]
fn complete_and_empty_tracks_are_not_offered() {
    let t = setup(&["alpha", "delta", "epsilon", "beta"], true);
    t.record(SID, "T-1");
    t.click_mark(SID).unwrap(); // to all
    t.choose(Some(2));
    t.click_mark(SID).unwrap();
    assert_eq!(slugs(&t), ["alpha-slug", "beta-slug"]);
    assert_eq!(t.picked(SID).as_deref(), Some("beta"));
}

#[test]
fn a_complete_session_track_is_not_offered() {
    let t = setup(&["alpha", "delta", "beta"], true);
    t.record(SID, "T-3");
    // the segment still counts it
    assert_eq!(shown(&t, SID), format!("{TRACK} 2\u{2215}2"));
    t.click_mark(SID).unwrap(); // to all
    t.choose(None);
    t.click_mark(SID).unwrap();
    assert_eq!(slugs(&t), ["alpha-slug", "beta-slug"]);
    // with one incomplete track besides it, nothing to choose
    let t = setup(&["delta", "alpha"], true);
    t.record(SID, "T-3");
    t.click_mark(SID).unwrap();
    t.click_mark(SID).unwrap();
    assert_eq!(t.offered(), None);
}

#[test]
fn the_only_incomplete_active_track_is_counted() {
    let t = setup(&["delta", "alpha", "epsilon"], true);
    t.record(SID, "T-2");
    assert_eq!(shown(&t, SID), format!("{TRACK} 1\u{2215}2"));
    // one list for the counts, one for the track counted: no more
    let calls = t.calls();
    assert_eq!(calls.matches("task list").count(), 2, "{calls}");
    // two incomplete of three active: the project, and the mark asks
    let t = setup(&["delta", "alpha", "beta"], true);
    t.record(SID, "T-2");
    assert_eq!(shown(&t, SID), format!("{ALL} 6\u{2215}10"));
    let l = osc_links::links(&t.show(SID));
    let u = hop_bar::click::action::parse(&l[0].0).unwrap();
    assert_eq!(u.tokens["to"], "pick");
}

#[test]
fn the_normal_path_lists_no_tasks_per_track() {
    let t = setup(&["alpha", "beta"], true);
    t.record(SID, "T-1");
    t.show(SID);
    let calls = t.calls();
    assert_eq!(calls.matches("task list").count(), 1, "{calls}");
    assert!(calls.contains("--track alpha"), "{calls}");
    // no active track: no counts either
    let t = setup(&[], true);
    t.record(SID, "T-2");
    t.show(SID);
    let calls = t.calls();
    assert_eq!(calls.matches("task list").count(), 1, "{calls}");
}
