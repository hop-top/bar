// The tray end to end, without a desktop: `bar tray state --json` feeds
// the tray's controller, a click becomes the `bar tray run` the tray's
// spawner runs for real, the fixture plugin gets the action and the next
// snapshot shows the outcome as the `Last:` row.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/tray_fx.rs"]
mod tray_fx;
#[allow(unused_imports)]
use plugin_fx::*;
use tray_fx::*;

use bar_tray::backend::app::{App, Effect, Event, Tint};
use bar_tray::backend::source::Source;
use bar_tray::backend::spawn;
use bar_tray::model::{Diff, Style, LAST_ID};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BOB: &str = "echo.claude@8317.4";
const RESET: &str = "echo.more.reset";

/// The tray's controller over the fixture `bar`, as a backend drives it.
struct Tray {
    t: T,
    app: App,
    bar: PathBuf,
}

impl Tray {
    /// A granted echo with the full menu, its first snapshot loaded.
    fn new(conf: &str) -> Tray {
        let t = T::new(conf);
        t.fx.put("ext.json", &menu_ext(json!({})));
        t.fx.put("menu.json", &good_menu());
        let bar = t.shim();
        let mut tray = Tray {
            t,
            app: App::new(),
            bar,
        };
        assert_eq!(
            tray.app.handle(Event::Poll, Instant::now()),
            [Effect::Fetch]
        );
        let fx = tray.fetch();
        assert!(matches!(fx[0], Effect::Apply(Diff::Rebuild(_))), "{fx:?}");
        tray
    }

    /// The `Fetch` effect carried out: `bar tray state --json` through
    /// the tray's own source, its result reported to the controller.
    fn fetch(&mut self) -> Vec<Effect> {
        let r = Source::Bar(self.bar.clone()).fetch();
        self.app.handle(Event::Snapshot(r), Instant::now())
    }

    fn generation(&self) -> String {
        let v: Value = serde_json::from_str(&self.t.file()).unwrap();
        v["generation"].as_str().unwrap().to_string()
    }

    fn click(&mut self, id: &str) -> Vec<Effect> {
        self.app.handle(Event::Click(id.into()), Instant::now())
    }

    /// The `Spawn` effect carried out: the tray's spawner runs `bar`.
    fn run(&self, args: &[OsString], limit: Duration) -> Result<(), String> {
        spawn::run(&self.bar, args, false, limit).map(|_| ())
    }

    /// The run ended: the controller refetches and the new menu shows.
    fn finished(&mut self, r: Result<(), String>) -> Vec<Effect> {
        let fx = self.app.handle(Event::Finished(r), Instant::now());
        assert!(fx.contains(&Effect::Fetch), "{fx:?}");
        let mut out = fx;
        out.extend(self.fetch());
        out
    }

    fn last_row(&self) -> (String, Option<Style>) {
        let row = self.app.view().find(LAST_ID).expect("a Last: row");
        (row.label.clone(), row.style)
    }

    fn action(&self) -> Option<Value> {
        let s = self.t.fx.file("action.json");
        (!s.is_empty()).then(|| serde_json::from_str(&s).unwrap())
    }

    fn forget_action(&self) {
        let _ = std::fs::remove_file(self.t.fx.bin.join("action.json"));
    }
}

fn spawn_of(fx: &[Effect]) -> (Vec<String>, Duration) {
    let Some(Effect::Spawn(args, limit)) = fx.iter().find(|e| matches!(e, Effect::Spawn(..)))
    else {
        panic!("no spawn in {fx:?}");
    };
    let args = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    (args, *limit)
}

fn os(args: &[String]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn a_click_runs_the_item_and_the_next_menu_shows_it() {
    let mut tray = Tray::new("tray echo\n");
    tray.t.fx.put("action-out", "switched to bob\n");
    let g = tray.generation();
    let fx = tray.click(BOB);
    let (args, limit) = spawn_of(&fx);
    assert_eq!(args, ["tray", "run", BOB, "--generation", &g]);
    // the declared limit (10 s for an action without a timeout) plus grace
    assert_eq!(limit, spawn::deadline(Some(10)));
    let row = tray.app.view().find(BOB).unwrap();
    assert!(!row.enabled && row.label.ends_with(" …"), "in flight");

    assert_eq!(tray.run(&os(&args), limit), Ok(()));
    assert_eq!(
        tray.action(),
        Some(json!({"action": "use", "tokens": {"account": "4", "provider": "claude"}}))
    );
    assert_eq!(tray.t.fx.file("action.name").trim(), "use");
    assert_eq!(
        tray.t.fx.file("action.state"),
        tray.t.state().display().to_string()
    );

    let fx = tray.finished(Ok(()));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Tint(_))), "{fx:?}");
    let (label, style) = tray.last_row();
    assert!(label.starts_with("Last: ok: switched to bob · "), "{label}");
    assert!(label.ends_with(" s ago"), "{label}");
    assert_eq!(style, Some(Style::Dim));
    assert!(tray.app.view().find(BOB).unwrap().enabled, "not in flight");
}

#[test]
fn confirm_takes_two_clicks_and_the_second_passes_the_flag() {
    let mut tray = Tray::new("tray echo\n");
    let g = tray.generation();
    let fx = tray.click(RESET);
    assert!(!fx.iter().any(|e| matches!(e, Effect::Spawn(..))), "{fx:?}");
    assert_eq!(tray.app.view().find(RESET).unwrap().label, "Confirm: Reset");
    // what the first click would have run is refused by `bar` too
    let unconfirmed = spawn::run_args(RESET, false, &g);
    assert_eq!(
        tray.run(&unconfirmed, spawn::deadline(None)),
        Err("bar tray run: refused: needs confirmation".into())
    );
    assert_eq!(tray.action(), None);

    let fx = tray.click(RESET);
    let (args, limit) = spawn_of(&fx);
    assert_eq!(
        args,
        ["tray", "run", RESET, "--confirmed", "--generation", &g]
    );
    assert_eq!(tray.run(&os(&args), limit), Ok(()));
    assert_eq!(
        tray.action(),
        Some(json!({"action": "reset", "tokens": {}}))
    );
    tray.finished(Ok(()));
    let (label, style) = tray.last_row();
    assert!(label.starts_with("Last: ok · "), "{label}");
    assert_eq!(style, Some(Style::Dim));
}

#[test]
fn an_ungranted_item_is_disabled_and_refused_by_both_sides() {
    let mut tray = Tray::new("tray echo.use\n");
    let g = tray.generation();
    let row = tray.app.view().find(RESET).unwrap();
    assert!(!row.enabled);
    assert_eq!(row.detail.as_deref(), Some("not enabled: tray echo.reset"));
    // the controller ignores the click
    assert_eq!(tray.click(RESET), []);
    // a forged click is refused by `bar`, and the refusal shows
    let forged = spawn::run_args(RESET, true, &g);
    let r = tray.run(&forged, spawn::deadline(None));
    assert_eq!(r, Err("bar tray run: refused: item is disabled".into()));
    assert_eq!(tray.action(), None);
    let fx = tray.finished(r);
    assert_eq!(fx[0], Effect::Tint(Tint::Bad));
    assert_eq!(
        fx[1],
        Effect::Tooltip("bar tray run: refused: item is disabled".into())
    );
    let (label, style) = tray.last_row();
    assert!(
        label.starts_with("Last: refused: item is disabled · "),
        "{label}"
    );
    assert_eq!(style, Some(Style::Bad));
    // the tint clears after its window
    let later = Instant::now() + bar_tray::backend::app::BAD_WINDOW;
    let fx = tray.app.handle(Event::Tick, later);
    assert!(fx.contains(&Effect::Tint(Tint::Normal)), "{fx:?}");
}

#[test]
fn a_spent_snapshot_refuses_a_replay_and_a_fresh_one_allows_a_new_click() {
    let mut tray = Tray::new("tray echo\n");
    let g = tray.generation();
    let (args, limit) = spawn_of(&tray.click(BOB));
    assert_eq!(tray.run(&os(&args), limit), Ok(()));
    tray.forget_action();
    // the same command again, before any refetch: nothing runs
    assert_eq!(
        tray.run(&os(&args), limit),
        Err("bar tray run: refused: menu is stale, reopen it".into())
    );
    assert_eq!(tray.action(), None);
    // the refetch after the run: same items, same generation, new file
    tray.finished(Ok(()));
    assert_eq!(tray.generation(), g);
    let (again, limit) = spawn_of(&tray.click(BOB));
    assert_eq!(again, args);
    assert_eq!(tray.run(&os(&again), limit), Ok(()));
    assert!(tray.action().is_some(), "a new click runs");
}

#[test]
fn a_waybar_poll_between_open_and_click_keeps_the_click_valid() {
    let mut tray = Tray::new("tray echo\n");
    let g = tray.generation();
    let waybar = |tray: &Tray| {
        let mut c = tray.t.bar();
        c.arg("waybar");
        let out = busy_retry(|| c.output());
        assert!(out.status.success());
        let line: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(line["text"], "alice");
    };
    waybar(&tray);
    assert_eq!(tray.generation(), g, "the poll kept the generation");
    let (args, limit) = spawn_of(&tray.click(BOB));
    assert_eq!(tray.run(&os(&args), limit), Ok(()));
    tray.finished(Ok(()));

    // the items move under the open menu: bob's account is now 5
    let mut m: Value = serde_json::from_str(&good_menu()).unwrap();
    m["groups"][0]["items"][1]["tokens"]["account"] = json!("5");
    tray.t.fx.put("menu.json", &m.to_string());
    waybar(&tray);
    let moved = tray.generation();
    assert_ne!(moved, g);
    tray.forget_action();
    let (stale, limit) = spawn_of(&tray.click(BOB));
    assert_eq!(stale[4], g, "the tray still holds the old menu");
    let r = tray.run(&os(&stale), limit);
    assert_eq!(
        r,
        Err("bar tray run: refused: menu changed, reopen it".into())
    );
    assert_eq!(tray.action(), None);
    // the refetch brings the moved items; the next click runs with them
    tray.finished(r);
    let (label, _) = tray.last_row();
    assert!(
        label.starts_with("Last: refused: menu changed, reopen it · "),
        "{label}"
    );
    let (fresh, limit) = spawn_of(&tray.click(BOB));
    assert_eq!(fresh[4], moved);
    assert_eq!(tray.run(&os(&fresh), limit), Ok(()));
    assert_eq!(
        tray.action().unwrap()["tokens"],
        json!({"account": "5", "provider": "claude"})
    );
}
