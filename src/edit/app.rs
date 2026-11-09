//! Modes and key handling: turns key presses into editor operations. No
//! terminal here, so every key path is testable.

use super::fuzzy;
use super::model::Editor;
use super::schema::{self, Kind, Opt};
use crate::conf::Config;
use crate::palette;
use crate::placeholders;
use crate::plugin::ExtInfo;
use crate::presets;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

/// What a chooser picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Preset,
    Skin,
    Palette,
    Icons,
}

impl Choice {
    pub fn title(self) -> &'static str {
        match self {
            Choice::Preset => "preset",
            Choice::Skin => "skin",
            Choice::Palette => "palette",
            Choice::Icons => "icons",
        }
    }

    pub fn values(self) -> Vec<&'static str> {
        match self {
            Choice::Preset => presets::NAMES.to_vec(),
            Choice::Skin => vec!["plain", "powerline", "capsule"],
            Choice::Palette => palette::NAMES.to_vec(),
            Choice::Icons => vec!["none", "nerdfont", "emoji"],
        }
    }

    /// Applies `value` to `ed`.
    pub fn apply(self, ed: &mut Editor, value: &str) -> bool {
        match self {
            Choice::Preset => ed.apply_preset(value),
            _ => ed.set_style(self.title(), value),
        }
    }

    /// The value in effect now, if it is one of `values`.
    pub fn current(self, ed: &Editor) -> Option<String> {
        match self {
            Choice::Preset => ed.preset().map(str::to_string),
            Choice::Skin => Some(ed.get("skin").unwrap_or_else(|| "plain".into())),
            Choice::Palette => Some(ed.get("palette").unwrap_or_else(|| "default".into())),
            Choice::Icons => Some(ed.get("icons").unwrap_or_else(|| "none".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Placeholder picker: search text and the highlighted match.
    Picker {
        query: String,
        sel: usize,
    },
    /// Options of the selected placeholder; `sel` is the highlighted row.
    Options {
        sel: usize,
    },
    /// Typing a value for option `key`.
    Input {
        key: String,
        buf: String,
        sel: usize,
    },
    Choose {
        what: Choice,
        sel: usize,
    },
    /// Unsaved changes on quit: save, discard or go back.
    ConfirmQuit,
}

pub const STATES: [&str; 3] = crate::preview::STATES;

/// Looks up a plugin's `--ext-info`; `None` when it has none.
pub type InfoLoader = Box<dyn Fn(&str) -> Option<ExtInfo>>;

pub struct App {
    pub ed: Editor,
    pub mode: Mode,
    /// Index into `STATES` for the preview.
    pub state: usize,
    /// Last status message.
    pub msg: String,
    /// Plugin names on `PATH`.
    pub plugins: Vec<String>,
    infos: HashMap<String, Option<ExtInfo>>,
    load: InfoLoader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

impl App {
    pub fn new(ed: Editor, plugins: Vec<String>, load: InfoLoader) -> App {
        App {
            ed,
            mode: Mode::Normal,
            state: 0,
            msg: String::new(),
            plugins,
            infos: HashMap::new(),
            load,
        }
    }

    /// The `--ext-info` of every plugin `config` names, loaded once each;
    /// plugins without one are left out.
    pub fn plugin_infos(&mut self, config: &Config) -> Vec<ExtInfo> {
        let load = &self.load;
        crate::conf::project::user_plugins(config)
            .into_iter()
            .filter_map(|n| {
                let info = self.infos.entry(n).or_insert_with_key(|n| load(n));
                info.clone()
            })
            .collect()
    }

    /// Picker entries: core placeholders, then plugins, with descriptions.
    pub fn catalogue(&self) -> Vec<(String, String)> {
        let core = placeholders::names().map(|n| (n.to_string(), schema::describe(n).to_string()));
        let plugins = self.plugins.iter().map(|n| {
            let d = self.infos.get(n).and_then(|i| i.as_ref()).map_or_else(
                || "plugin".to_string(),
                |i| format!("plugin: {}", i.description),
            );
            (n.clone(), d)
        });
        core.chain(plugins).collect()
    }

    /// Picker matches for `query`, best first.
    pub fn matches(&self, query: &str) -> Vec<(String, String)> {
        let all = self.catalogue();
        fuzzy::rank(query, &all)
            .into_iter()
            .map(|i| all[i].clone())
            .collect()
    }

    /// The option rows of the selected placeholder: its schema, then any
    /// option it sets that the schema lacks.
    pub fn option_rows(&mut self) -> Vec<Opt> {
        let Some(ph) = self.ed.selected_placeholder() else {
            return Vec::new();
        };
        let mut rows = match schema::core(&ph.name) {
            Some(o) => o,
            None => {
                let load = &self.load;
                let info = self
                    .infos
                    .entry(ph.name.clone())
                    .or_insert_with(|| load(&ph.name));
                schema::plugin(info.as_ref())
            }
        };
        for (k, _) in ph.options.iter() {
            if !rows.iter().any(|o| &o.key == k) {
                rows.push(Opt {
                    key: k.clone(),
                    kind: Kind::Text,
                    default: None,
                });
            }
        }
        rows
    }

    pub fn on_key(&mut self, k: KeyEvent) -> Flow {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match self.mode.clone() {
            Mode::Normal => return self.normal(k.code, ctrl),
            Mode::Picker { query, sel } => self.picker(k.code, query, sel),
            Mode::Options { sel } => self.options(k.code, sel),
            Mode::Input { key, buf, sel } => self.input(k.code, key, buf, sel),
            Mode::Choose { what, sel } => self.choose(k.code, what, sel),
            Mode::ConfirmQuit => return self.confirm(k.code),
        }
        Flow::Continue
    }

    fn report(&mut self, done: bool, ok: &str, no: &str) {
        self.msg = if done { ok } else { no }.to_string();
    }

    fn normal(&mut self, code: KeyCode, ctrl: bool) -> Flow {
        let ed = &mut self.ed;
        match code {
            KeyCode::Char('s') if ctrl => self.save(),
            KeyCode::Char('z') if ctrl => {
                let d = ed.undo();
                self.report(d, "undone", "nothing to undo");
            }
            KeyCode::Char('y') | KeyCode::Char('r') if ctrl => {
                let d = ed.redo();
                self.report(d, "redone", "nothing to redo");
            }
            KeyCode::Char('c') if ctrl => return self.quit(),
            KeyCode::Up | KeyCode::Char('k') => ed.go(-1, 0),
            KeyCode::Down | KeyCode::Char('j') => ed.go(1, 0),
            KeyCode::Left | KeyCode::Char('h') => ed.go(0, -1),
            KeyCode::Right | KeyCode::Char('l') => ed.go(0, 1),
            KeyCode::Tab => ed.next_placeholder(),
            KeyCode::Char('H') => {
                let d = ed.move_segment(-1);
                self.report(d, "moved", "");
            }
            KeyCode::Char('L') => {
                let d = ed.move_segment(1);
                self.report(d, "moved", "");
            }
            KeyCode::Char('K') => {
                let d = ed.move_segment_row(-1);
                self.report(d, "moved", "");
            }
            KeyCode::Char('J') => {
                let d = ed.move_segment_row(1);
                self.report(d, "moved", "");
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Picker {
                    query: String::new(),
                    sel: 0,
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                let d = ed.delete();
                self.report(d, "segment deleted", "nothing to delete");
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                if ed.selected_placeholder().is_some() {
                    self.mode = Mode::Options { sel: 0 };
                } else {
                    self.msg = "no placeholder here; a adds one".into();
                }
            }
            KeyCode::Char('p') => self.open(Choice::Preset),
            KeyCode::Char('s') => self.open(Choice::Skin),
            KeyCode::Char('c') => self.open(Choice::Palette),
            KeyCode::Char('i') => self.open(Choice::Icons),
            KeyCode::Char('t') => self.state = (self.state + 1) % STATES.len(),
            KeyCode::Char('u') => {
                let d = ed.undo();
                self.report(d, "undone", "nothing to undo");
            }
            KeyCode::Char('U') => {
                let d = ed.redo();
                self.report(d, "redone", "nothing to redo");
            }
            KeyCode::Char('w') => self.save(),
            KeyCode::Char('q') | KeyCode::Esc => return self.quit(),
            _ => {}
        }
        Flow::Continue
    }

    fn open(&mut self, what: Choice) {
        let values = what.values();
        let sel = what
            .current(&self.ed)
            .and_then(|c| values.iter().position(|v| *v == c))
            .unwrap_or(0);
        self.mode = Mode::Choose { what, sel };
    }

    fn quit(&mut self) -> Flow {
        if self.ed.dirty() {
            self.mode = Mode::ConfirmQuit;
            Flow::Continue
        } else {
            Flow::Exit
        }
    }

    fn save(&mut self) {
        self.msg = match self.ed.save() {
            Ok((p, None)) => format!("saved {}", p.display()),
            Ok((p, Some(b))) => format!("saved {} (old file: {})", p.display(), b.display()),
            Err(e) => format!("save failed: {e}"),
        };
    }

    fn picker(&mut self, code: KeyCode, mut query: String, sel: usize) {
        let n = self.matches(&query).len();
        let sel = match code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Enter => {
                if let Some((name, _)) = self.matches(&query).get(sel) {
                    let name = name.clone();
                    let d = self.ed.insert(&name);
                    self.report(d, &format!("added #{{{name}}}"), "");
                }
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Up => sel.saturating_sub(1),
            KeyCode::Down => (sel + 1).min(n.saturating_sub(1)),
            KeyCode::Backspace => {
                query.pop();
                0
            }
            KeyCode::Char(c) => {
                query.push(c);
                0
            }
            _ => sel,
        };
        self.mode = Mode::Picker { query, sel };
    }

    fn options(&mut self, code: KeyCode, sel: usize) {
        let rows = self.option_rows();
        let Some(ph) = self.ed.selected_placeholder() else {
            self.mode = Mode::Normal;
            return;
        };
        let Some(row) = rows.get(sel).cloned() else {
            self.mode = Mode::Normal;
            return;
        };
        let current = ph.options.get(&row.key).map(str::to_string);
        let mut sel = sel;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(rows.len() - 1),
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                if let Kind::Choice(values) = &row.kind {
                    let fwd = matches!(code, KeyCode::Right | KeyCode::Char('l'));
                    let next = cycle(values, current.as_deref(), fwd);
                    self.apply_option(&row.key, next.as_deref());
                }
            }
            KeyCode::Backspace | KeyCode::Delete => self.apply_option(&row.key, None),
            KeyCode::Enter | KeyCode::Char('e') => {
                self.mode = Mode::Input {
                    key: row.key.clone(),
                    buf: current.unwrap_or_default(),
                    sel,
                };
                return;
            }
            _ => {}
        }
        self.mode = Mode::Options { sel };
    }

    fn apply_option(&mut self, key: &str, value: Option<&str>) {
        self.msg = match self.ed.set_option(key, value) {
            Ok(_) => match value {
                Some(v) => format!("{key}={v}"),
                None => format!("{key} unset"),
            },
            Err(e) => e,
        };
    }

    fn input(&mut self, code: KeyCode, key: String, mut buf: String, sel: usize) {
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Options { sel };
                return;
            }
            KeyCode::Enter => {
                let rows = self.option_rows();
                let opt = rows.iter().find(|o| o.key == key);
                if let Some(why) = opt
                    .filter(|_| !buf.is_empty())
                    .and_then(|o| o.invalid(&buf))
                {
                    self.msg = format!("{key}: {why}");
                } else {
                    self.apply_option(
                        &key,
                        Some(&buf).filter(|b| !b.is_empty()).map(|b| b.as_str()),
                    );
                    self.mode = Mode::Options { sel };
                    return;
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        }
        self.mode = Mode::Input { key, buf, sel };
    }

    fn choose(&mut self, code: KeyCode, what: Choice, sel: usize) {
        let values = what.values();
        let sel = match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Enter => {
                let d = what.apply(&mut self.ed, values[sel]);
                self.report(d, &format!("{} {}", what.title(), values[sel]), "no change");
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (sel + 1).min(values.len() - 1),
            _ => sel,
        };
        self.mode = Mode::Choose { what, sel };
    }

    fn confirm(&mut self, code: KeyCode) -> Flow {
        match code {
            KeyCode::Char('y') => {
                self.save();
                if !self.ed.dirty() {
                    return Flow::Exit;
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Char('n') => return Flow::Exit,
            KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
        Flow::Continue
    }
}

/// The value after (or before) `current` in `values`, with unset (`None`)
/// between the last and the first.
fn cycle(values: &[String], current: Option<&str>, fwd: bool) -> Option<String> {
    let n = values.len() as isize + 1; // slot 0 is unset
    let at = current
        .and_then(|c| values.iter().position(|v| v == c))
        .map_or(0, |i| i as isize + 1);
    let next = (at + if fwd { 1 } else { -1 }).rem_euclid(n);
    (next > 0).then(|| values[next as usize - 1].clone())
}
