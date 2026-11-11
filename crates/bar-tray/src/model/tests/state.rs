use super::*;
use std::time::{Duration, Instant};

const TOGGLE: &str = "routellm.rtellm@8317.toggle";
const SOON: &str = "cliproxyapi.next@8317.next-soon";
const ALICE: &str = "cliproxyapi.claude@8317.3";

fn state() -> (State, Instant) {
    let t0 = Instant::now();
    (State::new(menu()), t0)
}

fn label(s: &State, id: &str) -> String {
    s.view().find(id).unwrap().label.clone()
}

fn change_ids(d: &Diff) -> Vec<String> {
    match d {
        Diff::Update(c) => c.iter().map(|c| c.id.clone()).collect(),
        other => panic!("expected Update, got {other:?}"),
    }
}

#[test]
fn new_state_shows_the_spec() {
    let (s, _) = state();
    assert_eq!(*s.view(), menu());
    assert!(s.in_flight().is_none());
}

#[test]
fn plain_click_runs_and_marks_in_flight() {
    let (mut s, t0) = state();
    let (c, d) = s.click(TOGGLE, t0);
    assert_eq!(
        c,
        Click::Run {
            id: TOGGLE.into(),
            confirmed: false
        }
    );
    assert_eq!(change_ids(&d), [TOGGLE]);
    let row = s.view().find(TOGGLE).unwrap();
    assert_eq!((row.label.as_str(), row.enabled), ("Routing …", false));
    assert_eq!(s.in_flight(), Some(TOGGLE));
    // a second action is refused while the first runs; Quit still works
    assert_eq!(s.click(ALICE, t0), (Click::Busy, Diff::Same));
    assert_eq!(s.click(QUIT_ID, t0), (Click::Quit, Diff::Same));
    let d = s.finished(t0 + Duration::from_secs(2));
    assert_eq!(change_ids(&d), [TOGGLE]);
    assert_eq!(*s.view(), menu());
    assert!(s.in_flight().is_none());
    assert!(matches!(s.click(ALICE, t0).0, Click::Run { .. }));
}

#[test]
fn confirm_needs_two_clicks() {
    let (mut s, t0) = state();
    let (c, d) = s.click(SOON, t0);
    assert_eq!(c, Click::Armed(SOON.into()));
    assert_eq!(change_ids(&d), [SOON]);
    assert_eq!(label(&s, SOON), "Confirm: Use soonest reset");
    assert!(s.in_flight().is_none());
    let (c, d) = s.click(SOON, t0 + Duration::from_secs(3));
    assert_eq!(
        c,
        Click::Run {
            id: SOON.into(),
            confirmed: true
        }
    );
    assert_eq!(change_ids(&d), [SOON]);
    assert_eq!(label(&s, SOON), "Use soonest reset …");
    s.finished(t0 + Duration::from_secs(4));
    // the next click starts over
    assert_eq!(
        s.click(SOON, t0 + Duration::from_secs(5)).0,
        Click::Armed(SOON.into())
    );
}

#[test]
fn confirm_expires_after_the_window() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    assert_eq!(
        s.tick(t0 + CONFIRM_WINDOW - Duration::from_millis(1)),
        Diff::Same
    );
    let d = s.tick(t0 + CONFIRM_WINDOW);
    assert_eq!(change_ids(&d), [SOON]);
    assert_eq!(label(&s, SOON), "Use soonest reset");
    // a click after expiry arms again instead of running
    assert_eq!(
        s.click(SOON, t0 + CONFIRM_WINDOW).0,
        Click::Armed(SOON.into())
    );
}

#[test]
fn late_second_click_arms_again() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    let (c, d) = s.click(SOON, t0 + CONFIRM_WINDOW);
    assert_eq!(c, Click::Armed(SOON.into()));
    assert_eq!(
        d,
        Diff::Same,
        "armed before and after: the row reads the same"
    );
    assert_eq!(label(&s, SOON), "Confirm: Use soonest reset");
}

#[test]
fn clicking_elsewhere_disarms() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    let (c, d) = s.click(TOGGLE, t0 + Duration::from_secs(1));
    assert_eq!(
        c,
        Click::Run {
            id: TOGGLE.into(),
            confirmed: false
        }
    );
    let mut ids = change_ids(&d);
    ids.sort();
    assert_eq!(ids, [SOON, TOGGLE]);
    assert_eq!(label(&s, SOON), "Use soonest reset");
    s.finished(t0 + Duration::from_secs(2));
    // the confirm row needs a fresh first click
    assert_eq!(
        s.click(SOON, t0 + Duration::from_secs(3)).0,
        Click::Armed(SOON.into())
    );
}

#[test]
fn view_toggle_is_in_flight_too() {
    let (mut s, t0) = state();
    let (c, d) = s.click("tray.view.7d", t0);
    assert_eq!(c, Click::ViewToggle("7d".into()));
    assert_eq!(change_ids(&d), ["tray.view.7d"]);
    assert_eq!(label(&s, "tray.view.7d"), "7d usage …");
    assert_eq!(s.click(TOGGLE, t0).0, Click::Busy);
    s.finished(t0);
    assert_eq!(label(&s, "tray.view.7d"), "7d usage");
}

#[test]
fn info_disabled_separator_unknown_are_ignored() {
    let (mut s, t0) = state();
    for id in [
        "cliproxyapi.info.auth",
        "routellm.rtellm@8317.toggle@8318",
        "tray.sep.0",
        LAST_ID,
        "nope",
        "",
    ] {
        assert_eq!(s.click(id, t0), (Click::Ignored, Diff::Same), "{id}");
    }
    // a submenu row itself is not clickable
    assert_eq!(
        s.click("cliproxyapi.claude@8317", t0),
        (Click::Ignored, Diff::Same)
    );
}

#[test]
fn replace_keeps_in_flight_and_valid_arming() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    s.click(TOGGLE, t0 + Duration::from_secs(1));
    // the toggle ran: a new snapshot shows it off; the confirm row is still armed? no: the
    // run click disarmed it. Arm again on the new spec, then replace with a changed one.
    let mut v = fixture_json();
    v["plugins"][1]["groups"][0]["items"][0]["checked"] = json!(false);
    let spec = Menu::build(&parse_value(&v).unwrap(), NOW);
    let d = s.replace(spec.clone(), t0 + Duration::from_secs(2));
    assert_eq!(change_ids(&d), [TOGGLE]);
    let row = s.view().find(TOGGLE).unwrap();
    assert_eq!(
        (row.checked, row.enabled, row.label.as_str()),
        (false, false, "Routing …")
    );
    assert_eq!(s.in_flight(), Some(TOGGLE));
    s.finished(t0 + Duration::from_secs(3));
    assert_eq!(*s.view(), spec);
}

#[test]
fn replace_drops_arming_when_the_row_no_longer_confirms() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    let mut v = fixture_json();
    v["plugins"][0]["groups"][1]["items"][1]["confirm"] = json!(false);
    let spec = Menu::build(&parse_value(&v).unwrap(), NOW);
    let d = s.replace(spec.clone(), t0 + Duration::from_secs(1));
    assert_eq!(change_ids(&d), [SOON]);
    assert_eq!(*s.view(), spec);
    assert_eq!(
        s.click(SOON, t0 + Duration::from_secs(2)).0,
        Click::Run {
            id: SOON.into(),
            confirmed: false
        }
    );
}

#[test]
fn replace_keeps_arming_when_the_row_still_confirms() {
    let (mut s, t0) = state();
    s.click(SOON, t0);
    let d = s.replace(menu(), t0 + Duration::from_secs(1));
    assert_eq!(d, Diff::Same);
    assert_eq!(label(&s, SOON), "Confirm: Use soonest reset");
    assert_eq!(
        s.click(SOON, t0 + Duration::from_secs(2)).0,
        Click::Run {
            id: SOON.into(),
            confirmed: true
        }
    );
}

#[test]
fn replace_with_other_shape_rebuilds() {
    let (mut s, t0) = state();
    let spec = Menu::build(
        &parse(r#"{"version":1,"generation":"b","at":0}"#).unwrap(),
        0,
    );
    assert_eq!(s.replace(spec.clone(), t0), Diff::Rebuild(spec));
}

#[test]
fn in_flight_row_gone_from_new_spec_keeps_the_guard() {
    let (mut s, t0) = state();
    s.click(TOGGLE, t0);
    let spec = Menu::build(
        &parse(r#"{"version":1,"generation":"b","at":0}"#).unwrap(),
        0,
    );
    s.replace(spec, t0);
    assert_eq!(s.in_flight(), Some(TOGGLE));
    assert_eq!(s.click(QUIT_ID, t0).0, Click::Quit);
    s.finished(t0);
    assert!(s.in_flight().is_none());
}
