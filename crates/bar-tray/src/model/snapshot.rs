//! The tray snapshot: what `bar tray state --json` prints and writes to
//! `<state>/tray-menu.json`. The tray reads it, never writes it.
//!
//! # Contract (`version` 1)
//!
//! One JSON object. Unknown fields are ignored everywhere, so producers
//! may add fields without breaking older trays.
//!
//! ```json
//! {
//!   "version": 1,
//!   "generation": "7c1f0a9e",
//!   "at": 1762800000,
//!   "last_action": {"at": 1762799988, "ok": true, "note": "preferred alice"},
//!   "sessions": {"waiting": 1, "done": 2},
//!   "views": [{"key": "5h", "label": "5h usage", "on": true}],
//!   "plugins": [
//!     {"name": "cliproxyapi", "label": "CLIProxyAPI", "groups": [
//!       {"id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
//!         {"id": "3", "label": "alice", "detail": "5h 59% · 7d 71%", "checked": true,
//!          "enabled": true, "style": "good", "action": "use", "confirm": false,
//!          "tokens": {"account": "3", "provider": "claude"}}
//!       ]}
//!     ]}
//!   ]
//! }
//! ```
//!
//! | Field | Type | Rule |
//! |---|---|---|
//! | `version` | integer | must be `1`; anything else is refused |
//! | `generation` | string | id of this snapshot, `[A-Za-z0-9]` 1..=64; echoed back to `bar tray run --generation` |
//! | `at` | integer | Unix seconds when the snapshot was written |
//! | `last_action` | object or absent | `{at, ok, note}` from the last action: `at` Unix seconds, `ok` bool, `note` text (may be empty) |
//! | `sessions` | object or absent | `{waiting, done}` counts of Claude Code sessions |
//! | `views` | array | status-line view toggles the tray may flip with `bar toggle <key>`; empty unless the user set `tray-views on`. `key` `[a-z0-9._-]` 1..=32, `label`, `on` |
//! | `plugins` | array | at most 16, in menu order; `name` `[a-z0-9_-]` 1..=32, unique; `label` optional display name |
//! | `plugins[].groups` | array | at most 16 per plugin, in menu order; `id` unique within the plugin |
//! | `groups[].kind` | string | `radio` (one checked: a picker, shown as a submenu), `check` (toggles), `buttons` (one-shot actions), `info` (disabled rows). A group with another kind is dropped, not refused |
//! | `groups[].label` | string | submenu title for `radio`; informational otherwise |
//! | `groups[].items` | array | at most 16 per group, in menu order; more are dropped |
//! | `items[].id` | string | `[A-Za-z0-9_@:-]` 1..=32 (no `.`), unique within the group; same charset for group ids. The dispatch id is `<plugin>.<group>.<item>`, at most 96 characters |
//! | `items[].label` | string | shown as the row's text; control and bidi characters removed, at most 80 characters; empty after cleaning drops the item |
//! | `items[].detail` | string or absent | secondary text (shown after the label where the host allows), same cleaning |
//! | `items[].checked` | bool, default false | `radio` and `check` only |
//! | `items[].enabled` | bool, default true | `false` renders the row disabled; `bar tray state` sets it for actions without a `tray` grant |
//! | `items[].style` | string or absent | `good`, `warn`, `bad` or `dim`; anything else is ignored |
//! | `items[].action` | string or absent | `[a-z0-9_-]` 1..=32; the plugin action a click runs. Without one the item is not clickable |
//! | `items[].confirm` | bool, default false | the action asks for confirmation: first click arms the row, second click runs with `--confirmed` |
//! | `items[].limit` | integer or absent | seconds `bar tray run` lets the action run (its declared limit, capped); the tray gives the run 5 s more, then kills it. Absent or 0: the tray waits the longest limit `bar` allows |
//! | `items[].tokens` | object | the action's token values; read by `bar tray run`, never by the tray |
//!
//! Duplicate plugin names, group ids or item ids are refused: a click
//! could otherwise run the wrong item. Item and group fields mirror the
//! plugin `--menu` protocol; `bar tray state` adds `confirm` from the
//! plugin's declaration.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

/// The only snapshot version this tray reads.
pub const VERSION: u64 = 1;
/// Most plugins, groups per plugin and items per group.
pub const MAX_ITEMS: usize = 16;
/// Longest label or detail, in characters.
pub const MAX_LABEL: usize = 80;
/// Longest dispatch id `<plugin>.<group>.<item>`.
pub const MAX_ID: usize = 96;

/// Why a snapshot was refused: where and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub path: String,
    pub why: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.why)
    }
}

impl std::error::Error for Error {}

fn err<T>(path: impl Into<String>, why: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        path: path.into(),
        why: why.into(),
    })
}

/// A validated snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub generation: String,
    pub at: u64,
    pub last_action: Option<LastAction>,
    pub sessions: Option<Sessions>,
    pub views: Vec<View>,
    pub plugins: Vec<Plugin>,
    /// What was dropped while reading, one line each.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LastAction {
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct Sessions {
    #[serde(default)]
    pub waiting: u64,
    #[serde(default)]
    pub done: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    pub key: String,
    pub label: String,
    pub on: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plugin {
    pub name: String,
    pub label: Option<String>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Radio,
    Check,
    Buttons,
    Info,
}

impl GroupKind {
    fn parse(s: &str) -> Option<GroupKind> {
        match s {
            "radio" => Some(GroupKind::Radio),
            "check" => Some(GroupKind::Check),
            "buttons" => Some(GroupKind::Buttons),
            "info" => Some(GroupKind::Info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub label: Option<String>,
    pub kind: GroupKind,
    pub items: Vec<Item>,
}

/// Row colour hint, as the status line's `style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Good,
    Warn,
    Bad,
    Dim,
}

impl Style {
    pub fn parse(s: &str) -> Option<Style> {
        match s {
            "good" => Some(Style::Good),
            "warn" => Some(Style::Warn),
            "bad" => Some(Style::Bad),
            "dim" => Some(Style::Dim),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub checked: bool,
    pub enabled: bool,
    pub style: Option<Style>,
    pub action: Option<String>,
    pub confirm: bool,
    /// Seconds the action may run, when the producer knows.
    pub limit: Option<u64>,
}

// Raw shapes: everything optional, validated below.

#[derive(Deserialize)]
struct RawSnapshot {
    version: Option<u64>,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    at: u64,
    #[serde(default)]
    last_action: Option<LastAction>,
    #[serde(default)]
    sessions: Option<Sessions>,
    #[serde(default)]
    views: Vec<RawView>,
    #[serde(default)]
    plugins: Vec<RawPlugin>,
}

#[derive(Deserialize)]
struct RawView {
    #[serde(default)]
    key: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    on: bool,
}

#[derive(Deserialize)]
struct RawPlugin {
    #[serde(default)]
    name: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    groups: Vec<RawGroup>,
}

#[derive(Deserialize)]
struct RawGroup {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    items: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    checked: bool,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    limit: u64,
}

fn yes() -> bool {
    true
}

/// Text as a menu row shows it: no control or invisible-direction
/// characters, at most [`MAX_LABEL`] characters, trimmed.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            '\u{2028}' | '\u{2029}' | '\u{00AD}' | '\u{FEFF}' | '\u{061C}' => None,
            '\u{200B}' | '\u{200E}' | '\u{200F}' => None,
            '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' => None,
            c => Some(c),
        })
        .take(MAX_LABEL)
        .collect::<String>()
        .trim()
        .to_string()
}

fn optional(s: Option<String>) -> Option<String> {
    s.map(|t| clean(&t)).filter(|t| !t.is_empty())
}

/// A name of 1 to `max` bytes, every byte accepted by `ok`.
fn valid(s: &str, max: usize, ok: impl Fn(u8) -> bool) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(ok)
}

/// Plugin names and actions: `[a-z0-9_-]`.
pub fn valid_name(s: &str) -> bool {
    valid(s, 32, |b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
    })
}

/// Group and item ids: `[A-Za-z0-9_@:-]`, never `.`, so the dispatch id
/// `<plugin>.<group>.<item>` splits back into its parts.
pub fn valid_id(s: &str) -> bool {
    valid(s, 32, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'@' | b':')
    })
}

/// View keys: `[a-z0-9._-]`, as `bar toggle` takes them.
fn valid_key(s: &str) -> bool {
    valid(s, 32, |b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
    })
}

/// Reads and validates a snapshot.
pub fn parse(text: &str) -> Result<Snapshot, Error> {
    let raw: RawSnapshot = match serde_json::from_str::<Value>(text) {
        Err(e) => return err("$", format!("not JSON: {e}")),
        Ok(v) if !v.is_object() => return err("$", "not an object"),
        Ok(v) => serde_json::from_value(v).map_err(|e| Error {
            path: "$".into(),
            why: e.to_string(),
        })?,
    };
    match raw.version {
        Some(VERSION) => {}
        Some(v) => {
            return err(
                "version",
                format!("unsupported version {v}, expected {VERSION}"),
            )
        }
        None => return err("version", "missing"),
    }
    if !valid(&raw.generation, 64, |b| b.is_ascii_alphanumeric()) {
        return err("generation", "expected [A-Za-z0-9], 1 to 64");
    }
    let mut warnings = Vec::new();
    let views = raw
        .views
        .into_iter()
        .filter_map(|v| {
            let label = clean(&v.label);
            if valid_key(&v.key) && !label.is_empty() {
                Some(View {
                    key: v.key,
                    label,
                    on: v.on,
                })
            } else {
                warnings.push(format!("views: dropped {:?}", v.key));
                None
            }
        })
        .take(MAX_ITEMS)
        .collect();
    let mut names = BTreeSet::new();
    let mut plugins = Vec::new();
    for (i, p) in raw.plugins.into_iter().enumerate() {
        if i >= MAX_ITEMS {
            warnings.push(format!("plugins: more than {MAX_ITEMS}, rest dropped"));
            break;
        }
        plugins.push(plugin(p, &mut names, &mut warnings)?);
    }
    Ok(Snapshot {
        generation: raw.generation,
        at: raw.at,
        last_action: raw.last_action.map(|a| LastAction {
            note: clean(&a.note),
            ..a
        }),
        sessions: raw.sessions,
        views,
        plugins,
        warnings,
    })
}

fn plugin(
    p: RawPlugin,
    names: &mut BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Result<Plugin, Error> {
    let path = format!("plugins[{}]", p.name);
    if !valid_name(&p.name) {
        return err(path, "name: expected [a-z0-9_-], 1 to 32");
    }
    if !names.insert(p.name.clone()) {
        return err(path, "duplicate plugin name");
    }
    let mut ids = BTreeSet::new();
    let mut groups = Vec::new();
    for (i, g) in p.groups.into_iter().enumerate() {
        if i >= MAX_ITEMS {
            warnings.push(format!(
                "{path}: more than {MAX_ITEMS} groups, rest dropped"
            ));
            break;
        }
        let gpath = format!("{path}.{}", g.id);
        if !valid_id(&g.id) {
            return err(gpath, "id: expected [A-Za-z0-9_@:-], 1 to 32");
        }
        if !ids.insert(g.id.clone()) {
            return err(gpath, "duplicate group id");
        }
        let Some(kind) = GroupKind::parse(&g.kind) else {
            warnings.push(format!("{gpath}: unknown kind {:?}, dropped", g.kind));
            continue;
        };
        let items = items(g.items, &gpath, p.name.len() + g.id.len() + 2, warnings)?;
        groups.push(Group {
            id: g.id,
            label: optional(g.label),
            kind,
            items,
        });
    }
    Ok(Plugin {
        name: p.name,
        label: optional(p.label),
        groups,
    })
}

fn items(
    raw: Vec<RawItem>,
    gpath: &str,
    prefix: usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<Item>, Error> {
    let mut ids = BTreeSet::new();
    let mut out = Vec::new();
    for (i, it) in raw.into_iter().enumerate() {
        if i >= MAX_ITEMS {
            warnings.push(format!(
                "{gpath}: more than {MAX_ITEMS} items, rest dropped"
            ));
            break;
        }
        let path = format!("{gpath}.{}", it.id);
        if !valid_id(&it.id) {
            return err(path, "id: expected [A-Za-z0-9_@:-], 1 to 32");
        }
        if prefix + it.id.len() > MAX_ID {
            return err(path, format!("dispatch id longer than {MAX_ID}"));
        }
        if !ids.insert(it.id.clone()) {
            return err(path, "duplicate item id");
        }
        if let Some(a) = &it.action {
            if !valid_name(a) {
                return err(path, "action: expected [a-z0-9_-], 1 to 32");
            }
        }
        let label = clean(&it.label);
        if label.is_empty() {
            warnings.push(format!("{path}: empty label, dropped"));
            continue;
        }
        out.push(Item {
            id: it.id,
            label,
            detail: optional(it.detail),
            checked: it.checked,
            enabled: it.enabled,
            style: it.style.as_deref().and_then(Style::parse),
            action: it.action,
            confirm: it.confirm,
            limit: (it.limit > 0).then_some(it.limit),
        });
    }
    Ok(out)
}
