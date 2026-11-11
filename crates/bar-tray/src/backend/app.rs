//! The tray's behaviour, without a desktop: every [`Event`] a backend
//! reports becomes the [`Effect`]s it must carry out. The backend owns
//! the native menu, the threads and the icon; this owns the decisions:
//! one snapshot fetch at a time, one action in flight (through
//! [`State`]), the `bad` tint after a refusal, the poll on open, and the
//! idle poll: due on a [`Event::Tick`] once the interval has passed since
//! the last fetch ended, the interval being `--poll`, else the
//! snapshot's `poll`, else [`DEFAULT_POLL`].

use super::spawn::{self, deadline};
use crate::model::{Diff, Menu, Snapshot, State};
use std::collections::HashMap;
use std::ffi::OsString;
use std::time::{Duration, Instant};

/// How long the icon stays tinted after a refusal.
pub const BAD_WINDOW: Duration = Duration::from_secs(10);
/// The tooltip when nothing is wrong.
pub const TOOLTIP: &str = "bar";
/// Idle poll when neither `--poll` nor the snapshot says.
pub const DEFAULT_POLL: Duration = Duration::from_secs(30);

/// What a backend reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The menu is about to show (a click on the icon).
    Opened,
    /// Fetch now (the backend's first event).
    Poll,
    /// A second passed.
    Tick,
    /// A fetch ended.
    Snapshot(Result<Snapshot, String>),
    /// The user chose row `id`.
    Click(String),
    /// A spawned `bar` ended: `Err` carries its first stderr line.
    Finished(Result<(), String>),
}

/// The icon variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Normal,
    Bad,
}

/// What a backend must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Change the shown menu.
    Apply(Diff),
    /// Fetch a snapshot off the main thread, then report `Snapshot`.
    Fetch,
    /// Run `bar` with these arguments off the main thread, waiting at
    /// most this long, then report `Finished`.
    Spawn(Vec<OsString>, Duration),
    Tint(Tint),
    Tooltip(String),
    Quit,
}

#[derive(Debug)]
pub struct App {
    state: State,
    snapshot: Option<Snapshot>,
    /// Action limits by row id, from the last snapshot.
    limits: HashMap<String, u64>,
    fetching: bool,
    /// `--poll`: wins over the snapshot's.
    poll_flag: Option<Duration>,
    /// When the idle poll is due; `None` until the first fetch ends.
    next_poll: Option<Instant>,
    bad_until: Option<Instant>,
    /// Unix seconds now, for `Last: … ago`.
    clock: fn() -> u64,
}

impl App {
    /// Starts with an empty menu (`Quit` only) until the first snapshot.
    pub fn new() -> App {
        App::with_clock(unix_now)
    }

    pub fn with_clock(clock: fn() -> u64) -> App {
        App {
            state: State::new(Menu::build(&Snapshot::default(), clock())),
            snapshot: None,
            limits: HashMap::new(),
            fetching: false,
            poll_flag: None,
            next_poll: None,
            bad_until: None,
            clock,
        }
    }

    /// Polls every `flag` while idle, whatever the snapshot says; `None`
    /// follows the snapshot.
    pub fn poll(mut self, flag: Option<Duration>) -> App {
        self.poll_flag = flag;
        self
    }

    /// The idle poll interval now.
    fn interval(&self) -> Duration {
        self.poll_flag
            .or_else(|| {
                self.snapshot
                    .as_ref()
                    .and_then(|s| s.poll)
                    .map(Duration::from_secs)
            })
            .unwrap_or(DEFAULT_POLL)
    }

    /// The menu as it should be shown.
    pub fn view(&self) -> &Menu {
        self.state.view()
    }

    pub fn handle(&mut self, ev: Event, now: Instant) -> Vec<Effect> {
        match ev {
            Event::Opened | Event::Poll => self.fetch(),
            Event::Tick => self.tick(now),
            Event::Snapshot(r) => self.snapshot(r, now),
            Event::Click(id) => self.click(&id, now),
            Event::Finished(r) => self.finished(r, now),
        }
    }

    fn fetch(&mut self) -> Vec<Effect> {
        if self.fetching {
            return Vec::new();
        }
        self.fetching = true;
        vec![Effect::Fetch]
    }

    fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = Vec::new();
        // the `Last: … ago` row ages with the clock
        if let Some(s) = &self.snapshot {
            let menu = Menu::build(s, (self.clock)());
            push_diff(&mut out, self.state.replace(menu, now));
        } else {
            push_diff(&mut out, self.state.tick(now));
        }
        if self.bad_until.is_some_and(|t| now >= t) {
            self.bad_until = None;
            out.push(Effect::Tint(Tint::Normal));
            out.push(Effect::Tooltip(TOOLTIP.into()));
        }
        if self.next_poll.is_some_and(|t| now >= t) {
            out.extend(self.fetch());
        }
        out
    }

    fn snapshot(&mut self, r: Result<Snapshot, String>, now: Instant) -> Vec<Effect> {
        self.fetching = false;
        let mut out = Vec::new();
        match r {
            Ok(s) => {
                for w in &s.warnings {
                    eprintln!("bar-tray: warning: {w}");
                }
                self.limits = limits(&s);
                let menu = Menu::build(&s, (self.clock)());
                self.snapshot = Some(s);
                push_diff(&mut out, self.state.replace(menu, now));
                if self.bad_until.is_none() {
                    out.push(Effect::Tooltip(TOOLTIP.into()));
                }
            }
            Err(e) => {
                eprintln!("bar-tray: {e}");
                // bad until a snapshot arrives; the menu keeps the last one
                self.bad_until = Some(now + Duration::from_secs(1 << 40));
                out.push(Effect::Tint(Tint::Bad));
                out.push(Effect::Tooltip(e));
            }
        }
        self.next_poll = Some(now + self.interval());
        out
    }

    fn click(&mut self, id: &str, now: Instant) -> Vec<Effect> {
        use crate::model::Click;
        let (click, diff) = self.state.click(id, now);
        let mut out = Vec::new();
        push_diff(&mut out, diff);
        match click {
            Click::Run { id, confirmed } => {
                let limit = self.limits.get(&id).copied();
                let generation = self.snapshot.as_ref().map_or("", |s| s.generation.as_str());
                let args = spawn::run_args(&id, confirmed, generation);
                out.push(Effect::Spawn(args, deadline(limit)));
            }
            Click::ViewToggle(key) => {
                out.push(Effect::Spawn(spawn::toggle_args(&key), spawn::QUICK));
            }
            Click::Quit => out.push(Effect::Quit),
            Click::Armed(_) | Click::Busy | Click::Ignored => {}
        }
        out
    }

    fn finished(&mut self, r: Result<(), String>, now: Instant) -> Vec<Effect> {
        let mut out = Vec::new();
        push_diff(&mut out, self.state.finished(now));
        if let Err(e) = r {
            self.bad_until = Some(now + BAD_WINDOW);
            out.push(Effect::Tint(Tint::Bad));
            out.push(Effect::Tooltip(if e.is_empty() {
                "bar failed".to_string()
            } else {
                e
            }));
        }
        // the outcome shows in the next snapshot's `Last:` row
        out.extend(self.fetch());
        out
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

fn push_diff(out: &mut Vec<Effect>, d: Diff) {
    if d != Diff::Same {
        out.push(Effect::Apply(d));
    }
}

/// Row id to declared limit, for rows that have one.
fn limits(s: &Snapshot) -> HashMap<String, u64> {
    let mut m = HashMap::new();
    for p in &s.plugins {
        for g in &p.groups {
            for it in &g.items {
                if let Some(l) = it.limit {
                    m.insert(format!("{}.{}.{}", p.name, g.id, it.id), l);
                }
            }
        }
    }
    m
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests;
