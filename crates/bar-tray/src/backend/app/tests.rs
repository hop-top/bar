use super::*;
use crate::model::{self, Kind, QUIT_ID};

const FIXTURE: &str = include_str!("../../../tests/fixtures/snapshot.json");
const NOW: u64 = 1_762_800_000;

fn clock() -> u64 {
    NOW
}

fn app() -> App {
    App::with_clock(clock)
}

fn snapshot() -> Snapshot {
    model::parse(FIXTURE).unwrap()
}

/// An app that has its first snapshot.
fn loaded() -> (App, Instant) {
    let mut a = app();
    let t = Instant::now();
    assert_eq!(a.handle(Event::Poll, t), [Effect::Fetch]);
    let fx = a.handle(Event::Snapshot(Ok(snapshot())), t);
    assert!(matches!(fx[0], Effect::Apply(Diff::Rebuild(_))), "{fx:?}");
    assert_eq!(fx[1], Effect::Tooltip("bar".into()));
    (a, t)
}

fn is_update(e: &Effect, id: &str) -> bool {
    matches!(e, Effect::Apply(Diff::Update(c)) if c.iter().any(|c| c.id == id))
}

fn strs(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

#[test]
fn starts_with_quit_only() {
    let a = app();
    let rows: Vec<&str> = a.view().items.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(rows, [QUIT_ID]);
}

#[test]
fn one_fetch_at_a_time() {
    let mut a = app();
    let t = Instant::now();
    assert_eq!(a.handle(Event::Opened, t), [Effect::Fetch]);
    assert_eq!(a.handle(Event::Opened, t), []);
    assert_eq!(a.handle(Event::Poll, t), []);
    let _ = a.handle(Event::Snapshot(Err("x".into())), t);
    assert_eq!(a.handle(Event::Poll, t), [Effect::Fetch]);
}

#[test]
fn snapshot_builds_the_menu() {
    let (a, _) = loaded();
    assert!(a.view().find("cliproxyapi.claude@8317.4").is_some());
    assert_eq!(a.view().items.last().unwrap().id, QUIT_ID);
}

#[test]
fn failed_fetch_tints_until_the_next_snapshot() {
    let (mut a, t) = loaded();
    let _ = a.handle(Event::Poll, t);
    let fx = a.handle(Event::Snapshot(Err("bar tray state: boom".into())), t);
    assert_eq!(
        fx,
        [
            Effect::Tint(Tint::Bad),
            Effect::Tooltip("bar tray state: boom".into())
        ]
    );
    // the menu is kept
    assert!(a.view().find("cliproxyapi.claude@8317.4").is_some());
    // a minute later still bad (the tick only polls again)
    assert_eq!(
        a.handle(Event::Tick, t + Duration::from_secs(60)),
        [Effect::Fetch]
    );
    let _ = a.handle(Event::Poll, t);
    let fx = a.handle(Event::Snapshot(Ok(snapshot())), t + Duration::from_secs(61));
    // same menu again: nothing to apply, and the tint is cleared by a tick
    assert!(fx.is_empty(), "{fx:?}");
}

#[test]
fn click_runs_with_generation_and_limit() {
    let (mut a, t) = loaded();
    let fx = a.handle(Event::Click("cliproxyapi.claude@8317.4".into()), t);
    assert!(is_update(&fx[0], "cliproxyapi.claude@8317.4"), "{fx:?}");
    let Effect::Spawn(args, limit) = &fx[1] else {
        panic!("{fx:?}");
    };
    assert_eq!(
        strs(args),
        [
            "tray",
            "run",
            "cliproxyapi.claude@8317.4",
            "--generation",
            "7c1f0a9e3b2d4f60"
        ]
    );
    assert_eq!(*limit, deadline(None));
    // in flight: a second click is busy
    assert_eq!(
        a.handle(Event::Click("routellm.rtellm@8317.toggle".into()), t),
        []
    );
    let row = a.view().find("cliproxyapi.claude@8317.4").unwrap();
    assert!(!row.enabled);
    assert!(row.label.ends_with(" …"));
}

#[test]
fn declared_limit_sets_the_deadline() {
    let mut a = app();
    let t = Instant::now();
    let mut s = snapshot();
    s.plugins[1].groups[0].items[0].limit = Some(98);
    let _ = a.handle(Event::Poll, t);
    let _ = a.handle(Event::Snapshot(Ok(s)), t);
    let fx = a.handle(Event::Click("routellm.rtellm@8317.toggle".into()), t);
    assert!(
        matches!(&fx[1], Effect::Spawn(_, d) if *d == Duration::from_secs(103)),
        "{fx:?}"
    );
}

#[test]
fn confirm_arms_then_runs() {
    let (mut a, t) = loaded();
    let id = "cliproxyapi.next@8317.next-soon";
    let fx = a.handle(Event::Click(id.into()), t);
    assert_eq!(fx.len(), 1);
    assert!(is_update(&fx[0], id));
    assert_eq!(
        a.view().find(id).unwrap().label,
        "Confirm: Use soonest reset"
    );
    let fx = a.handle(Event::Click(id.into()), t + Duration::from_secs(3));
    let Effect::Spawn(args, _) = &fx[1] else {
        panic!("{fx:?}");
    };
    assert_eq!(strs(args)[..4], ["tray", "run", id, "--confirmed"]);
}

#[test]
fn confirm_expires_on_tick() {
    let (mut a, t) = loaded();
    let id = "cliproxyapi.next@8317.next-soon";
    let _ = a.handle(Event::Click(id.into()), t);
    let fx = a.handle(Event::Tick, t + model::CONFIRM_WINDOW);
    assert!(is_update(&fx[0], id), "{fx:?}");
    assert_eq!(a.view().find(id).unwrap().label, "Use soonest reset");
}

#[test]
fn view_toggle_and_quit() {
    let (mut a, t) = loaded();
    let fx = a.handle(Event::Click("tray.view.7d".into()), t);
    assert_eq!(fx[1], Effect::Spawn(spawn::toggle_args("7d"), spawn::QUICK));
    let _ = a.handle(Event::Finished(Ok(())), t);
    assert_eq!(a.handle(Event::Click(QUIT_ID.into()), t), [Effect::Quit]);
}

#[test]
fn finished_ok_refetches() {
    let (mut a, t) = loaded();
    let _ = a.handle(Event::Click("routellm.rtellm@8317.toggle".into()), t);
    let fx = a.handle(Event::Finished(Ok(())), t);
    assert!(is_update(&fx[0], "routellm.rtellm@8317.toggle"));
    assert_eq!(fx[1], Effect::Fetch);
    assert_eq!(fx.len(), 2);
    assert!(
        a.view()
            .find("routellm.rtellm@8317.toggle")
            .unwrap()
            .enabled
    );
}

#[test]
fn refusal_tints_for_ten_seconds() {
    let (mut a, t) = loaded();
    let _ = a.handle(Event::Click("routellm.rtellm@8317.toggle".into()), t);
    let fx = a.handle(
        Event::Finished(Err("refused: no tray line for routellm.toggle".into())),
        t,
    );
    assert_eq!(
        &fx[1..],
        [
            Effect::Tint(Tint::Bad),
            Effect::Tooltip("refused: no tray line for routellm.toggle".into()),
            Effect::Fetch
        ]
    );
    // the snapshot after it keeps the tint and its tooltip
    let fx = a.handle(Event::Snapshot(Ok(snapshot())), t + Duration::from_secs(1));
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Tooltip(_))),
        "{fx:?}"
    );
    assert_eq!(a.handle(Event::Tick, t + Duration::from_secs(9)), []);
    assert_eq!(
        a.handle(Event::Tick, t + BAD_WINDOW),
        [Effect::Tint(Tint::Normal), Effect::Tooltip("bar".into())]
    );
    let fx = a.handle(
        Event::Finished(Err(String::new())),
        t + Duration::from_secs(20),
    );
    assert!(fx.contains(&Effect::Tooltip("bar failed".into())), "{fx:?}");
}

#[test]
fn ticks_age_the_last_action_row() {
    let (mut a, t) = loaded();
    assert_eq!(
        a.view().find(model::LAST_ID).unwrap().label,
        "Last: ok: preferred alice · 12 s ago"
    );
    // the clock is fixed, so a tick changes nothing
    assert_eq!(a.handle(Event::Tick, t + Duration::from_secs(1)), []);
}

#[test]
fn kinds_survive() {
    let (a, _) = loaded();
    let m = a.view();
    assert!(matches!(
        m.find("cliproxyapi.claude@8317").unwrap().kind,
        Kind::Submenu(_)
    ));
}

/// Whether `fx` asks for a fetch.
fn fetches(fx: &[Effect]) -> bool {
    fx.contains(&Effect::Fetch)
}

/// An app given `flag` (`--poll`) that has a snapshot asking for `poll`.
fn polled(flag: Option<u64>, poll: Option<u64>) -> (App, Instant) {
    let mut a = App::with_clock(clock).poll(flag.map(Duration::from_secs));
    let t = Instant::now();
    assert_eq!(a.handle(Event::Poll, t), [Effect::Fetch]);
    let mut s = snapshot();
    s.poll = poll;
    let _ = a.handle(Event::Snapshot(Ok(s)), t);
    (a, t)
}

#[test]
fn the_idle_poll_follows_the_snapshot() {
    let (mut a, t) = polled(None, Some(90));
    assert!(!fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(89))
    ));
    assert!(fetches(&a.handle(Event::Tick, t + Duration::from_secs(90))));
}

#[test]
fn the_idle_poll_defaults_to_30_s() {
    let (mut a, t) = polled(None, None);
    assert!(!fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(29))
    ));
    assert!(fetches(&a.handle(Event::Tick, t + DEFAULT_POLL)));
}

#[test]
fn an_explicit_poll_wins_over_the_snapshot() {
    let (mut a, t) = polled(Some(15), Some(90));
    assert!(!fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(14))
    ));
    assert!(fetches(&a.handle(Event::Tick, t + Duration::from_secs(15))));
}

#[test]
fn the_idle_poll_restarts_after_every_fetch() {
    let (mut a, t) = polled(None, Some(60));
    // a fetch on open resets the wait
    assert_eq!(
        a.handle(Event::Opened, t + Duration::from_secs(50)),
        [Effect::Fetch]
    );
    let _ = a.handle(
        Event::Snapshot(Err("x".into())),
        t + Duration::from_secs(50),
    );
    assert!(!fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(109))
    ));
    // a failed fetch keeps the last snapshot's interval
    assert!(fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(110))
    ));
    // one fetch at a time: ticks while it runs ask for nothing more
    assert!(!fetches(
        &a.handle(Event::Tick, t + Duration::from_secs(111))
    ));
}
