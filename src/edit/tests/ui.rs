use super::app;
use crate::edit::app::App;
use crate::edit::ui::draw;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

const CONF: &str =
    "set left \"#{model} │ #{ctx} │ #{repo}:#{branch[max=9]}\"\nset right \"#{account}\"\n";

/// The screen as text rows, styles dropped.
fn screen(a: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(f, a)).unwrap();
    let buf = t.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn key(a: &mut App, k: KeyCode) {
    a.on_key(KeyEvent::new(k, KeyModifiers::NONE));
}

#[test]
fn layout_pane_snapshot() {
    let mut a = app(CONF);
    let s = screen(&mut a, 100, 20);
    assert!(s[0].starts_with(" bar edit "), "{}", s[0]);
    let top = s.iter().position(|l| l.starts_with("┌ layout ")).unwrap();
    // the layout pane: 60 of 100 columns
    let pane: Vec<String> = s[top..top + 8]
        .iter()
        .map(|l| l.chars().take(60).collect::<String>())
        .collect();
    let want = [
        "┌ layout ──────────────────────────────────────────────────┐",
        "│left      model │ ctx │ repo:branch…                      │",
        "│center    (empty)                                         │",
        "│right     account                                         │",
        "│left@2    (empty)                                         │",
        "│center@2  (empty)                                         │",
        "│right@2   (empty)                                         │",
        "│                                                          │",
    ];
    assert_eq!(pane, want);
    assert!(s.iter().any(|l| l
        .contains(" preview: early · preset custom · skin plain · palette default · icons none ")));
}

#[test]
fn preview_shows_the_renderer_output() {
    let mut a = app(CONF);
    let s = screen(&mut a, 100, 20);
    let preview = s
        .iter()
        .find(|l| l.contains("Opus 4.7"))
        .expect("preview row");
    assert!(preview.contains("ctx 38%"), "{preview}");
    assert!(preview.contains("bar:main"), "{preview}");
    assert!(s.iter().any(|l| l.contains(" preview: early ")));
    key(&mut a, KeyCode::Char('t'));
    let s = screen(&mut a, 100, 20);
    assert!(s.iter().any(|l| l.contains(" preview: burning ")));
}

#[test]
fn preview_lists_config_errors() {
    let mut a = app("set skin bogus\n");
    let s = screen(&mut a, 100, 20);
    assert!(
        s.iter().any(|l| l.contains("line 1: invalid skin `bogus`")),
        "{s:#?}"
    );
}

#[test]
fn options_and_picker_popups() {
    let mut a = app(CONF);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Enter);
    let s = screen(&mut a, 100, 20);
    assert!(s.iter().any(|l| l.contains("#{branch}")));
    assert!(s.iter().any(|l| l.contains("max       9")), "{s:#?}");
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char('a'));
    for c in "tok".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    let s = screen(&mut a, 100, 24);
    assert!(s.iter().any(|l| l.contains(" add placeholder ")));
    assert!(s.iter().any(|l| l.contains("> tok")));
    assert!(s
        .iter()
        .any(|l| l.contains("tokens") && l.contains("transcript token counts")));
}

#[test]
fn chooser_previews_the_highlighted_value() {
    let mut a = app(CONF);
    key(&mut a, KeyCode::Char('s'));
    key(&mut a, KeyCode::Down); // powerline, not applied yet
    let s = screen(&mut a, 100, 20);
    let preview = s.iter().find(|l| l.contains("Opus 4.7")).unwrap();
    assert!(preview.contains('\u{E0B0}'), "{preview}");
    assert_eq!(a.ed.get("skin"), None);
}

#[test]
fn quit_prompt_and_modified_marker() {
    let mut a = app(CONF);
    key(&mut a, KeyCode::Char('d'));
    key(&mut a, KeyCode::Char('q'));
    let s = screen(&mut a, 100, 20);
    assert!(s[0].contains("[modified]"));
    assert!(s.iter().any(|l| l.contains("save before quitting? y/n")));
}

#[test]
fn ansi_to_styled_spans() {
    use crate::edit::ansi::lines;
    use ratatui::style::{Color, Modifier};
    let l = lines("\x1b[38;5;75mA\x1b[39m\x1b[1mB\x1b[22m\x1b[48;5;24;31mC\x1b]8;;bar://x\x1b\\D\x1b]8;;\x1b\\\nE");
    assert_eq!(l.len(), 2);
    let s = &l[0].spans;
    assert_eq!(s[0].content, "A");
    assert_eq!(s[0].style.fg, Some(Color::Indexed(75)));
    assert_eq!(s[1].style.fg, None);
    assert!(s[1].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(s[2].content, "CD");
    assert_eq!(s[2].style.bg, Some(Color::Indexed(24)));
    assert_eq!(s[2].style.fg, Some(Color::Indexed(1)));
    assert_eq!(l[1].spans[0].content, "E");
}

#[test]
fn preview_draws_plugin_web_samples_else_the_name() {
    use crate::edit::app::InfoLoader;
    use crate::edit::model::Editor;
    let ci = r#"{"name":"ci","version":"1","capabilities":["placeholder"],"options":{"name":{"type":"bool"}},"web":{"samples":[{"when":{"name":"true"},"output":{"text":"ok build-and-test"}},{"output":{"text":"ok-mark"}}]}}"#;
    let load: InfoLoader =
        Box::new(move |name| (name == "ci").then(|| crate::plugin::parse_info(ci))?);
    let conf = "set left \"#{model} │ #{ci[name=true]} │ #{ci} │ #{weather}\"\n";
    let ed = Editor::new(super::unwritable(), Some(conf.into()));
    let mut a = App::new(ed, vec!["ci".into(), "weather".into()], load);
    let s = screen(&mut a, 120, 20);
    let preview = s
        .iter()
        .find(|l| l.contains("Opus 4.7"))
        .expect("preview row");
    assert!(preview.contains("ok build-and-test"), "{preview}");
    assert!(preview.contains("ok-mark"), "{preview}");
    assert!(preview.contains("weather"), "{preview}");
}
