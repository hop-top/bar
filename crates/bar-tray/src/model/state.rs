//! Clicks to runs. The state holds the menu the snapshot described
//! (`spec`) and the menu as shown (`view`): the spec plus an armed
//! confirmation (`Confirm: <label>` for [`CONFIRM_WINDOW`]) and the one
//! action in flight (`<label> …`, disabled). Every change returns the
//! [`Diff`] a backend applies to what it shows.

use super::{diff, Diff, Menu, QUIT_ID, VIEW_PREFIX};
use std::time::{Duration, Instant};

/// How long a first click on a `confirm` row waits for the second.
pub const CONFIRM_WINDOW: Duration = Duration::from_secs(10);

/// What a click asks the backend to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Spawn `bar tray run <id>` (`--confirmed` when set); the row is in
    /// flight until [`State::finished`].
    Run {
        id: String,
        confirmed: bool,
    },
    /// First click on a `confirm` row: the row now reads `Confirm: …`.
    Armed(String),
    /// Spawn `bar toggle <key>`; in flight until [`State::finished`].
    ViewToggle(String),
    Quit,
    /// Another action is in flight.
    Busy,
    /// Not a clickable row (info, separator, disabled, unknown).
    Ignored,
}

#[derive(Debug)]
pub struct State {
    spec: Menu,
    view: Menu,
    armed: Option<(String, Instant)>,
    in_flight: Option<String>,
}

impl State {
    pub fn new(spec: Menu) -> State {
        let mut s = State {
            spec,
            view: Menu::default(),
            armed: None,
            in_flight: None,
        };
        s.view = s.render();
        s
    }

    /// The menu as it should be shown.
    pub fn view(&self) -> &Menu {
        &self.view
    }

    /// The row whose action is running, if any.
    pub fn in_flight(&self) -> Option<&str> {
        self.in_flight.as_deref()
    }

    /// A new snapshot arrived. The armed row stays armed if it is still
    /// a `confirm` row; the action in flight stays in flight.
    pub fn replace(&mut self, spec: Menu, now: Instant) -> Diff {
        self.spec = spec;
        if let Some((id, _)) = &self.armed {
            if !self
                .spec
                .find(id)
                .is_some_and(|m| m.confirm && m.clickable())
            {
                self.armed = None;
            }
        }
        self.refresh(now)
    }

    /// The user clicked row `id`.
    pub fn click(&mut self, id: &str, now: Instant) -> (Click, Diff) {
        self.expire(now);
        let Some(item) = self.spec.find(id) else {
            return (Click::Ignored, Diff::Same);
        };
        if !item.clickable() {
            return (Click::Ignored, Diff::Same);
        }
        if id == QUIT_ID {
            return (Click::Quit, Diff::Same);
        }
        if self.in_flight.is_some() {
            return (Click::Busy, Diff::Same);
        }
        if let Some(key) = id.strip_prefix(VIEW_PREFIX) {
            let key = key.to_string();
            self.armed = None;
            self.in_flight = Some(id.to_string());
            return (Click::ViewToggle(key), self.refresh(now));
        }
        let confirmed = match (&self.armed, item.confirm) {
            (_, false) => false,
            (Some((armed, _)), true) if armed == id => true,
            (_, true) => {
                self.armed = Some((id.to_string(), now));
                return (Click::Armed(id.to_string()), self.refresh(now));
            }
        };
        self.armed = None;
        self.in_flight = Some(id.to_string());
        (
            Click::Run {
                id: id.to_string(),
                confirmed,
            },
            self.refresh(now),
        )
    }

    /// Time passed: an armed confirmation may have expired.
    pub fn tick(&mut self, now: Instant) -> Diff {
        self.refresh(now)
    }

    /// The action in flight ended (however it ended: the next snapshot
    /// carries the outcome).
    pub fn finished(&mut self, now: Instant) -> Diff {
        self.in_flight = None;
        self.refresh(now)
    }

    /// Drops an armed confirmation once its window closed: the one place
    /// that decides whether the second click still counts.
    fn expire(&mut self, now: Instant) {
        if self
            .armed
            .as_ref()
            .is_some_and(|(_, at)| now >= *at + CONFIRM_WINDOW)
        {
            self.armed = None;
        }
    }

    fn refresh(&mut self, now: Instant) -> Diff {
        self.expire(now);
        let next = self.render();
        let d = diff(&self.view, &next);
        self.view = next;
        d
    }

    fn render(&self) -> Menu {
        let mut m = self.spec.clone();
        if let Some(row) = self.armed.as_ref().and_then(|(id, _)| m.find_mut(id)) {
            row.label = format!("Confirm: {}", row.label);
        }
        if let Some(row) = self.in_flight.as_ref().and_then(|id| m.find_mut(id)) {
            row.label.push_str(" …");
            row.enabled = false;
        }
        m
    }
}
