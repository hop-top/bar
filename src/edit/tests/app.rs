use super::app;
use crate::edit::app::{App, Choice, Flow, InfoLoader, Mode};
use crate::edit::fuzzy;
use crate::edit::model::Editor;
use crate::plugin::parse_info;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const CONF: &str = "set left \"#{model} │ #{ctx}\"\nset right \"#{usage[type=5h]}\"\n";

fn press(a: &mut App, keys: &[KeyCode]) -> Flow {
    let mut f = Flow::Continue;
    for k in keys {
        f = a.on_key(KeyEvent::new(*k, KeyModifiers::NONE));
    }
    f
}

fn type_str(a: &mut App, s: &str) {
    for c in s.chars() {
        press(a, &[KeyCode::Char(c)]);
    }
}

#[test]
fn fuzzy_ranks_name_prefix_first() {
    let items: Vec<(String, String)> = ["cost", "cache", "cachettl", "ctx"]
        .iter()
        .map(|n| (n.to_string(), String::new()))
        .collect();
    let r = fuzzy::rank("ca", &items);
    assert_eq!(r[0], 1); // cache before cachettl (item order breaks the tie)
    assert_eq!(r[1], 2);
    assert!(fuzzy::rank("zz", &items).is_empty());
    assert!(fuzzy::score("ctl", "cachettl").is_some());
}

#[test]
fn picker_searches_descriptions_too() {
    let a = app(CONF);
    let names: Vec<String> = a.matches("pull req").into_iter().map(|m| m.0).collect();
    assert_eq!(names.first().map(String::as_str), Some("prs"));
}

#[test]
fn picker_inserts_after_selection() {
    let mut a = app(CONF);
    press(&mut a, &[KeyCode::Char('a')]);
    type_str(&mut a, "spendh");
    press(&mut a, &[KeyCode::Enter]);
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(a.ed.segments(0), ["#{model}", "#{spendhist}", "#{ctx}"]);
    // escape adds nothing
    press(
        &mut a,
        &[KeyCode::Char('a'), KeyCode::Char('x'), KeyCode::Esc],
    );
    assert_eq!(a.ed.segments(0).len(), 3);
}

#[test]
fn options_cycle_choices_and_type_values() {
    let mut a = app(CONF);
    // right group, the usage placeholder
    press(&mut a, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(a.mode, Mode::Options { sel: 0 });
    // row 0 is `type`: 5h -> 7d
    press(&mut a, &[KeyCode::Right]);
    assert_eq!(a.ed.segments(2), ["#{usage[type=7d]}"]);
    // `width` row: type a value
    let rows = a.option_rows();
    let w = rows.iter().position(|o| o.key == "width").unwrap();
    for _ in 0..w {
        press(&mut a, &[KeyCode::Down]);
    }
    press(&mut a, &[KeyCode::Enter]);
    type_str(&mut a, "x");
    press(&mut a, &[KeyCode::Enter]);
    assert!(a.msg.contains("whole number"), "{}", a.msg);
    press(&mut a, &[KeyCode::Backspace]);
    type_str(&mut a, "8");
    press(&mut a, &[KeyCode::Enter]);
    assert_eq!(a.ed.segments(2), ["#{usage[type=7d,width=8]}"]);
    // backspace unsets
    press(&mut a, &[KeyCode::Backspace]);
    assert_eq!(a.ed.segments(2), ["#{usage[type=7d]}"]);
    press(&mut a, &[KeyCode::Esc]);
    assert_eq!(a.mode, Mode::Normal);
}

#[test]
fn plugin_options_come_from_ext_info() {
    let load: InfoLoader = Box::new(|name| {
        (name == "weather").then(|| {
            parse_info(r#"{"name":"weather","version":"1","capabilities":["placeholder"],"options":{"units":{"type":"enum","enum":["c","f"],"default":"c"}}}"#)
        })?
    });
    let ed = Editor::new(
        super::unwritable(),
        Some("set left \"#{weather}\"\n".into()),
    );
    let mut a = App::new(ed, vec!["weather".into()], load);
    let rows = a.option_rows();
    assert_eq!(rows[0].key, "units");
    assert_eq!(rows[0].default.as_deref(), Some("c"));
    assert!(rows.iter().any(|o| o.key == "click"));
    press(&mut a, &[KeyCode::Enter, KeyCode::Right]);
    assert_eq!(a.ed.segments(0), ["#{weather[units=c]}"]);
    press(&mut a, &[KeyCode::Right]);
    assert_eq!(a.ed.segments(0), ["#{weather[units=f]}"]);
    press(&mut a, &[KeyCode::Right]);
    assert_eq!(a.ed.segments(0), ["#{weather}"]);
}

#[test]
fn choosers_apply_on_enter_only() {
    let mut a = app(CONF);
    press(&mut a, &[KeyCode::Char('s')]);
    assert_eq!(
        a.mode,
        Mode::Choose {
            what: Choice::Skin,
            sel: 0
        }
    );
    press(&mut a, &[KeyCode::Down]);
    assert_eq!(a.ed.text(), CONF);
    press(&mut a, &[KeyCode::Enter]);
    assert_eq!(a.ed.get("skin").as_deref(), Some("powerline"));
    press(&mut a, &[KeyCode::Char('c'), KeyCode::Down, KeyCode::Esc]);
    assert_eq!(a.ed.get("palette"), None);
    press(&mut a, &[KeyCode::Char('p'), KeyCode::Enter]);
    assert_eq!(a.ed.preset(), Some("minimal"));
}

#[test]
fn moves_undo_and_redo_by_key() {
    let mut a = app(CONF);
    press(&mut a, &[KeyCode::Char('L')]);
    assert_eq!(a.ed.segments(0), ["#{ctx}", "#{model}"]);
    press(&mut a, &[KeyCode::Char('u')]);
    assert_eq!(a.ed.text(), CONF);
    a.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(a.ed.segments(0), ["#{ctx}", "#{model}"]);
    a.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(a.ed.text(), CONF);
}

#[test]
fn quit_asks_only_with_unsaved_changes() {
    let mut a = app(CONF);
    assert_eq!(press(&mut a, &[KeyCode::Char('q')]), Flow::Exit);
    press(&mut a, &[KeyCode::Char('d')]);
    assert_eq!(press(&mut a, &[KeyCode::Char('q')]), Flow::Continue);
    assert_eq!(a.mode, Mode::ConfirmQuit);
    assert_eq!(press(&mut a, &[KeyCode::Esc]), Flow::Continue);
    assert_eq!(a.mode, Mode::Normal);
    press(&mut a, &[KeyCode::Char('q')]);
    assert_eq!(press(&mut a, &[KeyCode::Char('n')]), Flow::Exit);
}

#[test]
fn quit_with_save_that_fails_stays_open() {
    let mut a = app(CONF);
    press(&mut a, &[KeyCode::Char('d'), KeyCode::Char('q')]);
    // the test path cannot be written
    assert_eq!(press(&mut a, &[KeyCode::Char('y')]), Flow::Continue);
    assert!(a.msg.starts_with("save failed"), "{}", a.msg);
    assert!(a.ed.dirty());
}

#[test]
fn every_core_placeholder_has_a_description() {
    for n in crate::placeholders::names() {
        assert!(!crate::edit::schema::describe(n).is_empty(), "{n}");
    }
}
