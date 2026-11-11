//! Platform-neutral menu model: a [`Snapshot`] becomes a [`Menu`], two
//! menus give a [`Diff`] a backend applies, and [`State`] turns clicks
//! into runs while tracking confirmation and the action in flight.

pub mod diff;
pub mod snapshot;
pub mod state;

pub use diff::{diff, Change, Diff};
pub use snapshot::{parse, Error, GroupKind, Snapshot, Style};
pub use state::{Click, State, CONFIRM_WINDOW};

/// Id of the tray's own `Quit` row.
pub const QUIT_ID: &str = "tray.quit";
/// Prefix of the status-line view toggles; the rest is the `bar toggle` key.
pub const VIEW_PREFIX: &str = "tray.view.";
/// Id of the last-action info row.
pub const LAST_ID: &str = "tray.last";
/// Id of the sessions info row.
pub const SESSIONS_ID: &str = "tray.sessions";

/// What a row is. `Submenu` rows carry their children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Disabled text row.
    Info,
    /// Check item that flips something.
    Toggle,
    /// One of a group's exclusive choices (radio).
    Choice,
    /// One-shot button.
    Action,
    Separator,
    Submenu(Vec<MenuItem>),
}

/// One row of the menu. `id` is unique across the whole tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: String,
    pub kind: Kind,
    pub label: String,
    pub detail: Option<String>,
    pub checked: bool,
    pub enabled: bool,
    pub style: Option<Style>,
    /// The action asks for confirmation before running.
    pub confirm: bool,
}

impl MenuItem {
    fn new(id: impl Into<String>, kind: Kind, label: impl Into<String>) -> MenuItem {
        MenuItem {
            id: id.into(),
            kind,
            label: label.into(),
            detail: None,
            checked: false,
            enabled: true,
            style: None,
            confirm: false,
        }
    }

    fn separator(n: usize) -> MenuItem {
        let mut m = MenuItem::new(format!("tray.sep.{n}"), Kind::Separator, "");
        m.enabled = false;
        m
    }

    /// The row runs something when clicked.
    pub fn clickable(&self) -> bool {
        self.enabled && matches!(self.kind, Kind::Toggle | Kind::Choice | Kind::Action)
    }
}

/// The whole menu, top level in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Menu {
    pub items: Vec<MenuItem>,
}

impl Menu {
    /// The menu a snapshot describes, at `now` (Unix seconds, for the
    /// age of the last action): one section per plugin, then the tray's
    /// own rows (last action, sessions, view toggles, `Quit`).
    pub fn build(s: &Snapshot, now: u64) -> Menu {
        let mut items = Vec::new();
        let mut seps = 0;
        for p in &s.plugins {
            let rows = plugin_rows(p);
            if rows.is_empty() {
                continue;
            }
            if !items.is_empty() {
                items.push(MenuItem::separator(seps));
                seps += 1;
            }
            items.extend(rows);
        }
        let mut tray = Vec::new();
        if let Some(a) = &s.last_action {
            let mut m = MenuItem::new(LAST_ID, Kind::Info, last_label(a, now));
            m.enabled = false;
            m.style = Some(if a.ok { Style::Dim } else { Style::Bad });
            tray.push(m);
        }
        if let Some(c) = &s.sessions {
            let label = format!("Sessions: {} waiting · {} done", c.waiting, c.done);
            let mut m = MenuItem::new(SESSIONS_ID, Kind::Info, label);
            m.enabled = false;
            m.style = Some(Style::Dim);
            tray.push(m);
        }
        if !tray.is_empty() {
            if !items.is_empty() {
                items.push(MenuItem::separator(seps));
                seps += 1;
            }
            items.append(&mut tray);
        }
        if !s.views.is_empty() {
            if !items.is_empty() {
                items.push(MenuItem::separator(seps));
                seps += 1;
            }
            for v in &s.views {
                let mut m =
                    MenuItem::new(format!("{VIEW_PREFIX}{}", v.key), Kind::Toggle, &v.label);
                m.checked = v.on;
                items.push(m);
            }
        }
        if !items.is_empty() {
            items.push(MenuItem::separator(seps));
        }
        items.push(MenuItem::new(QUIT_ID, Kind::Action, "Quit"));
        Menu { items }
    }

    /// The row with this id, at any depth.
    pub fn find(&self, id: &str) -> Option<&MenuItem> {
        self.flatten()
            .into_iter()
            .map(|(m, _)| m)
            .find(|m| m.id == id)
    }

    /// Every row in display order with its depth (0 = top level).
    pub fn flatten(&self) -> Vec<(&MenuItem, usize)> {
        let mut out = Vec::new();
        walk(&self.items, 0, &mut out);
        out
    }

    /// The row with this id, mutable, at any depth.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut MenuItem> {
        find_mut(&mut self.items, id)
    }
}

fn walk<'a>(items: &'a [MenuItem], depth: usize, out: &mut Vec<(&'a MenuItem, usize)>) {
    for m in items {
        out.push((m, depth));
        if let Kind::Submenu(children) = &m.kind {
            walk(children, depth + 1, out);
        }
    }
}

fn find_mut<'a>(items: &'a mut [MenuItem], id: &str) -> Option<&'a mut MenuItem> {
    for m in items {
        if m.id == id {
            return Some(m);
        }
        if let Kind::Submenu(children) = &mut m.kind {
            if let Some(found) = find_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// The rows of one plugin section.
fn plugin_rows(p: &snapshot::Plugin) -> Vec<MenuItem> {
    let mut rows = Vec::new();
    for g in &p.groups {
        let gid = format!("{}.{}", p.name, g.id);
        let children: Vec<MenuItem> = g.items.iter().map(|it| row(&gid, g.kind, it)).collect();
        match g.kind {
            GroupKind::Radio => {
                if children.is_empty() {
                    continue;
                }
                let label = g.label.clone().unwrap_or_else(|| g.id.clone());
                rows.push(MenuItem::new(gid, Kind::Submenu(children), label));
            }
            _ => rows.extend(children),
        }
    }
    rows
}

fn row(gid: &str, kind: GroupKind, it: &snapshot::Item) -> MenuItem {
    let kind = match kind {
        GroupKind::Radio => Kind::Choice,
        GroupKind::Check => Kind::Toggle,
        GroupKind::Buttons => Kind::Action,
        GroupKind::Info => Kind::Info,
    };
    let mut m = MenuItem::new(format!("{gid}.{}", it.id), kind, &it.label);
    m.detail = it.detail.clone();
    m.checked = it.checked && matches!(m.kind, Kind::Choice | Kind::Toggle);
    // info rows never run; a row without an action has nothing to run
    m.enabled = it.enabled && it.action.is_some() && m.kind != Kind::Info;
    m.style = it.style;
    m.confirm = it.confirm && it.action.is_some();
    m
}

/// `Last: ok: note · 12 s ago`.
fn last_label(a: &snapshot::LastAction, now: u64) -> String {
    let verdict = if a.ok { "ok" } else { "refused" };
    let what = if a.note.is_empty() {
        verdict.to_string()
    } else {
        format!("{verdict}: {}", a.note)
    };
    format!("Last: {what} · {}", age(now.saturating_sub(a.at)))
}

/// `12 s ago`, `3 m ago`, `2 h ago`, `5 d ago`.
pub fn age(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} m ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests;
