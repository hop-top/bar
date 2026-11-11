//! The optional `menu` capability: `bar-<name> --menu` prints the groups
//! and items a desktop tray shows for the plugin, each item naming a
//! declared action and its tokens. Parsed under the render-output rules:
//! sanitised text, checked ids, tokens and styles, capped counts, nothing
//! unknown kept.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{ExtInfo, Found, MAX_ITEMS, MAX_TOKENS, RENDER_TIMEOUT};
use crate::cache::run_child;
use crate::click;
use crate::style::sanitize;

/// Capability a plugin declares to be asked for a menu.
pub const MENU_CAPABILITY: &str = "menu";
/// Limit for one `--menu` run: a plugin answers from its cache.
pub const MENU_TIMEOUT: Duration = RENDER_TIMEOUT;
/// Most groups a menu may carry.
pub const MAX_GROUPS: usize = MAX_ITEMS;
/// Longest group or item id, in characters.
pub const MAX_MENU_ID: usize = 32;
/// Longest label or detail kept, in characters.
pub const MAX_LABEL: usize = 120;

/// How a group's items behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// A picker: one item checked.
    Radio,
    /// Toggles: each item checked or not.
    Check,
    /// One-shot actions.
    Buttons,
    /// Disabled rows: text only, no action.
    Info,
}

impl MenuKind {
    pub fn parse(s: &str) -> Option<MenuKind> {
        Some(match s {
            "radio" => MenuKind::Radio,
            "check" => MenuKind::Check,
            "buttons" => MenuKind::Buttons,
            "info" => MenuKind::Info,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MenuKind::Radio => "radio",
            MenuKind::Check => "check",
            MenuKind::Buttons => "buttons",
            MenuKind::Info => "info",
        }
    }
}

/// One row of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub checked: bool,
    pub enabled: bool,
    /// `good`, `warn`, `bad` or `dim`.
    pub style: Option<String>,
    /// A declared action; `None` only in an `info` group.
    pub action: Option<String>,
    pub tokens: BTreeMap<String, String>,
    /// The action asks for confirmation first; set by [`Menu::declared`]
    /// from the declaration, never by the plugin's output.
    pub confirm: bool,
}

/// A section of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuGroup {
    pub id: String,
    pub label: Option<String>,
    pub kind: MenuKind,
    pub items: Vec<MenuItem>,
}

/// A plugin's menu as bar keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Menu {
    pub groups: Vec<MenuGroup>,
}

impl ExtInfo {
    /// The plugin declared the `menu` capability.
    pub fn has_menu(&self) -> bool {
        self.capabilities.iter().any(|c| c == MENU_CAPABILITY)
    }
}

/// `[A-Za-z0-9_@:-]`, 1 to [`MAX_MENU_ID`] characters: no `.`, which
/// joins the parts of a tray id `<plugin>.<group>.<item>`.
pub fn valid_menu_id(s: &str) -> bool {
    (1..=MAX_MENU_ID).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@:-".contains(&b))
}

/// Sanitised, trimmed, cut to [`MAX_LABEL`]; `None` when nothing is left.
fn text(v: &Value) -> Option<String> {
    let s: String = sanitize(v.as_str()?)
        .trim()
        .chars()
        .take(MAX_LABEL)
        .collect();
    (!s.is_empty()).then_some(s)
}

fn id(v: &Value) -> Option<String> {
    v.as_str().filter(|s| valid_menu_id(s)).map(String::from)
}

fn style(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| matches!(*s, "good" | "warn" | "bad" | "dim"))
        .map(String::from)
}

/// `None` unless the object has a valid id and label, and names an action
/// outside `info` groups; an `info` row keeps neither action nor tokens
/// and is never enabled or checked.
fn item(v: &Value, kind: MenuKind) -> Option<MenuItem> {
    let v = v.as_object().map(|_| v)?;
    let info = kind == MenuKind::Info;
    let action = v["action"]
        .as_str()
        .filter(|a| click::action::valid_token_name(a))
        .map(String::from);
    if !info && action.is_none() {
        return None;
    }
    let tokens = super::render::tokens(&v["tokens"]);
    if v["tokens"]
        .as_object()
        .is_some_and(|o| o.len() > MAX_TOKENS)
    {
        return None;
    }
    Some(MenuItem {
        id: id(&v["id"])?,
        label: text(&v["label"])?,
        detail: text(&v["detail"]),
        checked: !info && v["checked"].as_bool().unwrap_or(false),
        enabled: !info && v["enabled"].as_bool().unwrap_or(true),
        style: style(&v["style"]),
        action: if info { None } else { action },
        tokens: if info { BTreeMap::new() } else { tokens },
        confirm: false,
    })
}

/// Keeps the first of each id, at most `max`.
fn unique<T>(list: impl Iterator<Item = T>, key: impl Fn(&T) -> &str, max: usize) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for x in list {
        if out.len() < max && !out.iter().any(|o| key(o) == key(&x)) {
            out.push(x);
        }
    }
    out
}

/// `None` unless the object has a valid id, a known `kind` and at least
/// one usable item. A `radio` keeps its first checked item checked.
fn group(v: &Value) -> Option<MenuGroup> {
    let v = v.as_object().map(|_| v)?;
    let gid = id(&v["id"])?;
    let kind = MenuKind::parse(v["kind"].as_str()?)?;
    let items = v["items"].as_array()?.iter().filter_map(|i| item(i, kind));
    let mut items = unique(items, |i| &i.id, MAX_ITEMS);
    if kind == MenuKind::Radio {
        let mut seen = false;
        for i in items.iter_mut().filter(|i| i.checked) {
            i.checked = !seen;
            seen = true;
        }
    }
    (!items.is_empty()).then(|| MenuGroup {
        id: gid,
        label: text(&v["label"]),
        kind,
        items,
    })
}

/// `None` unless a JSON object with a `groups` array; groups and items
/// that break a rule are left out, so an empty menu is a valid answer.
pub fn parse_menu(raw: &str) -> Option<Menu> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let groups = v.get("groups")?.as_array()?.iter().filter_map(group);
    Some(Menu {
        groups: unique(groups, |g| &g.id, MAX_GROUPS),
    })
}

impl Menu {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Only the items whose action `info` declares with exactly the token
    /// names given, each marked with the declaration's `confirm`; a group
    /// left without items goes too. `info` rows carry no action and stay.
    pub fn declared(mut self, info: &ExtInfo) -> Menu {
        for g in &mut self.groups {
            g.items.retain_mut(|i| {
                let Some(a) = &i.action else {
                    return true;
                };
                let Some(spec) = info.actions.get(a) else {
                    return false;
                };
                let given: Vec<&String> = i.tokens.keys().collect();
                if given != spec.tokens.iter().collect::<Vec<_>>() {
                    return false;
                }
                i.confirm = spec.confirm;
                true
            });
        }
        self.groups.retain(|g| !g.items.is_empty());
        self
    }

    /// The protocol shape, as a snapshot carries it.
    pub fn to_json(&self) -> Value {
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let mut v = json!({"id": g.id, "kind": g.kind.as_str()});
                if let Some(l) = &g.label {
                    v["label"] = json!(l);
                }
                v["items"] = Value::Array(g.items.iter().map(item_json).collect());
                v
            })
            .collect();
        json!({ "groups": groups })
    }
}

fn item_json(i: &MenuItem) -> Value {
    let mut v = json!({"id": i.id, "label": i.label});
    if let Some(d) = &i.detail {
        v["detail"] = json!(d);
    }
    if i.checked {
        v["checked"] = json!(true);
    }
    if !i.enabled {
        v["enabled"] = json!(false);
    }
    if let Some(s) = &i.style {
        v["style"] = json!(s);
    }
    if let Some(a) = &i.action {
        v["action"] = json!(a);
        v["tokens"] = json!(i.tokens);
    }
    if i.confirm {
        v["confirm"] = json!(true);
    }
    v
}

/// The stdin of a `--menu` run: `options` (the placeholder's typed
/// defaults), `toggles` (the plugin's toggles that are on) and, when there
/// are any, `accounts` (the user's account rules).
pub fn menu_input(
    options: Map<String, Value>,
    toggles: Map<String, Value>,
    accounts: Value,
) -> String {
    let mut v = json!({ "options": options, "toggles": toggles });
    if accounts.as_array().is_some_and(|a| !a.is_empty()) {
        v["accounts"] = accounts;
    }
    v.to_string()
}

/// Runs `--menu` now (no shell, [`MENU_TIMEOUT`]) with `input` on stdin
/// and `BAR_STATE_DIR` set to `state`. `None` when the plugin fails,
/// times out or prints no menu.
pub fn run(found: &Found, input: &str, state: &std::path::Path) -> Option<Menu> {
    let mut c = std::process::Command::new(&found.path);
    c.arg("--menu").env("BAR_STATE_DIR", state);
    run_child(c, Some(input.as_bytes().to_vec()), MENU_TIMEOUT).and_then(|o| parse_menu(&o))
}
