//! The `--menu` call: what a plugin receives on stdin and the menu it
//! prints for a desktop tray, with the cleaning bar applies to it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::proto::{sanitize, valid_token, AccountRule, MAX_ITEMS};

/// Capability a plugin declares to be asked for a menu.
pub const MENU_CAPABILITY: &str = "menu";
/// Most groups a menu may carry.
pub const MAX_GROUPS: usize = MAX_ITEMS;
/// Longest group or item id, in characters.
pub const MAX_ID: usize = 32;
/// Longest label or detail kept, in characters.
pub const MAX_LABEL: usize = 120;
/// Most tokens an item may carry.
pub const MAX_TOKENS: usize = 8;

/// What `--menu` receives on stdin. Unknown fields are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MenuInput {
    /// The user's `account` lines, when `inputs.accounts` is declared.
    #[serde(default)]
    pub accounts: Vec<AccountRule>,
    /// The placeholder's `default <name>` options, typed.
    #[serde(default)]
    pub options: Map<String, Value>,
    /// The plugin's named toggles that are on.
    #[serde(default)]
    pub toggles: Map<String, Value>,
}

/// How a group's items behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// One row of a group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// `[A-Za-z0-9_@:-]`, at most [`MAX_ID`]; unique in its group.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// `good`, `warn`, `bad` or `dim`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// A declared action; required outside `info` groups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The values the action takes: exactly its declared token names.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

/// A section of the menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuGroup {
    /// `[A-Za-z0-9_@:-]`, at most [`MAX_ID`]; unique in the menu.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub kind: MenuKind,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

/// `--menu` output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Menu {
    pub groups: Vec<MenuGroup>,
}

/// `[A-Za-z0-9_@:-]`, 1 to [`MAX_ID`] characters: an id bar can join
/// into `<plugin>.<group>.<item>`.
pub fn valid_menu_id(s: &str) -> bool {
    (1..=MAX_ID).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@:-".contains(&b))
}

/// `[a-z0-9_-]`, 1 to 32 characters.
pub fn valid_token_name(k: &str) -> bool {
    (1..=32).contains(&k.len())
        && k.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}

/// A style bar colours: `good`, `warn`, `bad` or `dim`.
pub fn valid_style(s: &str) -> bool {
    matches!(s, "good" | "warn" | "bad" | "dim")
}

/// Sanitised, trimmed, cut to [`MAX_LABEL`] characters.
fn clean(s: &str) -> String {
    sanitize(s).trim().chars().take(MAX_LABEL).collect()
}

impl MenuItem {
    /// A row with a label.
    pub fn new(id: &str, label: &str) -> MenuItem {
        MenuItem {
            id: id.into(),
            label: label.into(),
            enabled: true,
            ..Default::default()
        }
    }

    /// Runs `action` with `tokens`.
    pub fn action(mut self, action: &str, tokens: &[(&str, &str)]) -> MenuItem {
        self.action = Some(action.into());
        self.tokens = tokens
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    pub fn detail(mut self, detail: &str) -> MenuItem {
        self.detail = Some(detail.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> MenuItem {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self) -> MenuItem {
        self.enabled = false;
        self
    }

    pub fn style(mut self, style: &str) -> MenuItem {
        self.style = Some(style.into());
        self
    }

    /// Cleans the strings, drops invalid tokens; `None` when the id or the
    /// label is unusable, or an item outside `info` names no action.
    fn finish(mut self, kind: MenuKind) -> Option<MenuItem> {
        if !valid_menu_id(&self.id) {
            return None;
        }
        self.label = clean(&self.label);
        if self.label.is_empty() {
            return None;
        }
        self.detail = self
            .detail
            .take()
            .map(|d| clean(&d))
            .filter(|d| !d.is_empty());
        self.style = self.style.take().filter(|s| valid_style(s));
        if kind == MenuKind::Info {
            self.action = None;
            self.tokens.clear();
            self.checked = false;
            self.enabled = false;
            return Some(self);
        }
        self.action = self.action.take().filter(|a| valid_token_name(a));
        self.action.as_ref()?;
        self.tokens
            .retain(|k, v| valid_token_name(k) && valid_token(v));
        if self.tokens.len() > MAX_TOKENS {
            return None;
        }
        Some(self)
    }
}

impl MenuGroup {
    pub fn new(id: &str, kind: MenuKind) -> MenuGroup {
        MenuGroup {
            id: id.into(),
            label: None,
            kind,
            items: Vec::new(),
        }
    }

    pub fn label(mut self, label: &str) -> MenuGroup {
        self.label = Some(label.into());
        self
    }

    pub fn item(mut self, item: MenuItem) -> MenuGroup {
        self.items.push(item);
        self
    }

    /// Cleans the group and its items: surplus and duplicate items go, a
    /// `radio` keeps one checked item. `None` when the id is unusable or
    /// nothing is left.
    fn finish(mut self) -> Option<MenuGroup> {
        if !valid_menu_id(&self.id) {
            return None;
        }
        self.label = self
            .label
            .take()
            .map(|l| clean(&l))
            .filter(|l| !l.is_empty());
        let kind = self.kind;
        let mut seen: Vec<String> = Vec::new();
        let mut checked = false;
        self.items = std::mem::take(&mut self.items)
            .into_iter()
            .filter_map(|i| i.finish(kind))
            .filter(|i| {
                let new = !seen.contains(&i.id);
                if new {
                    seen.push(i.id.clone());
                }
                new
            })
            .take(MAX_ITEMS)
            .map(|mut i| {
                if kind == MenuKind::Radio && i.checked {
                    i.checked = !checked;
                    checked = true;
                }
                i
            })
            .collect();
        (!self.items.is_empty()).then_some(self)
    }
}

impl Menu {
    /// Sanitises every string, drops invalid ids, tokens and styles,
    /// duplicate ids, surplus groups and items, and empty groups: what bar
    /// keeps of the output.
    pub fn finish(mut self) -> Menu {
        let mut seen: Vec<String> = Vec::new();
        self.groups = std::mem::take(&mut self.groups)
            .into_iter()
            .filter_map(MenuGroup::finish)
            .filter(|g| {
                let new = !seen.contains(&g.id);
                if new {
                    seen.push(g.id.clone());
                }
                new
            })
            .take(MAX_GROUPS)
            .collect();
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"groups":[]}"#.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(id: &str, label: &str) -> MenuItem {
        MenuItem::new(id, label).action("use", &[("account", id), ("provider", "claude")])
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_menu_id("claude@8317"));
        assert!(valid_menu_id("a:b-c_d"));
        assert!(!valid_menu_id(""));
        assert!(!valid_menu_id("a.b"), "dots join the parts of a tray id");
        assert!(!valid_menu_id("a b"));
        assert!(!valid_menu_id(&"a".repeat(MAX_ID + 1)));
    }

    #[test]
    fn finish_keeps_what_bar_would() {
        let m = Menu {
            groups: vec![
                MenuGroup::new("claude@8317", MenuKind::Radio)
                    .label(" Claude\u{1b} · :8317 ")
                    .item(pick("1", "alice").checked(true).detail("5h 59%\t"))
                    .item(pick("2", "bob\u{200B}").checked(true).style("odd"))
                    .item(pick("2", "dup"))
                    .item(pick("3", " "))
                    .item(pick("a.b", "dotted"))
                    .item(MenuItem::new("4", "no action")),
                MenuGroup::new("info", MenuKind::Info)
                    .item(pick("auth", "refused").checked(true).style("bad")),
                MenuGroup::new("empty", MenuKind::Buttons),
                MenuGroup::new("bad id", MenuKind::Buttons).item(pick("x", "y")),
                MenuGroup::new("claude@8317", MenuKind::Check).item(pick("x", "y")),
            ],
        }
        .finish();
        assert_eq!(m.groups.len(), 2);
        let g = &m.groups[0];
        assert_eq!(g.label.as_deref(), Some("Claude · :8317"));
        let ids: Vec<&str> = g.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["1", "2"]);
        assert_eq!(g.items[0].detail.as_deref(), Some("5h 59%"));
        assert!(
            g.items[0].checked && !g.items[1].checked,
            "one checked in a radio"
        );
        assert_eq!(g.items[1].label, "bob");
        assert_eq!(g.items[1].style, None);
        let i = &m.groups[1].items[0];
        assert!(i.action.is_none() && i.tokens.is_empty() && !i.enabled && !i.checked);
        assert_eq!(i.style.as_deref(), Some("bad"));
    }

    #[test]
    fn finish_caps_and_cleans_tokens() {
        let mut i = pick("1", "a");
        i.tokens.insert("Bad Name".into(), "x".into());
        i.tokens.insert("to".into(), "bad value".into());
        let g = MenuGroup::new("g", MenuKind::Buttons)
            .item(i)
            .finish()
            .unwrap();
        assert_eq!(g.items[0].tokens.len(), 2);
        let mut many = pick("1", "a");
        for n in 0..9 {
            many.tokens.insert(format!("t{n}"), "v".into());
        }
        assert!(MenuGroup::new("g", MenuKind::Buttons)
            .item(many)
            .finish()
            .is_none());
        let mut g = MenuGroup::new("g", MenuKind::Buttons);
        for n in 0..20 {
            g = g.item(pick(&n.to_string(), "a"));
        }
        assert_eq!(g.finish().unwrap().items.len(), MAX_ITEMS);
        let m = Menu {
            groups: (0..20)
                .map(|n| MenuGroup::new(&n.to_string(), MenuKind::Buttons).item(pick("1", "a")))
                .collect(),
        }
        .finish();
        assert_eq!(m.groups.len(), MAX_GROUPS);
    }

    #[test]
    fn json_shape_matches_the_protocol() {
        let m = Menu {
            groups: vec![MenuGroup::new("rtellm@8317", MenuKind::Check)
                .label("Routing")
                .item(
                    MenuItem::new("toggle", "Routing")
                        .checked(true)
                        .action("toggle", &[("instance", "9f2c"), ("to", "off")]),
                )],
        };
        assert_eq!(
            m.to_json(),
            r#"{"groups":[{"id":"rtellm@8317","label":"Routing","kind":"check","items":[{"id":"toggle","label":"Routing","checked":true,"action":"toggle","tokens":{"instance":"9f2c","to":"off"}}]}]}"#
        );
        let back: Menu = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(back, m);
        assert!(back.groups[0].items[0].enabled, "enabled defaults to true");
    }

    #[test]
    fn input_ignores_unknown_fields() {
        let i: MenuInput = serde_json::from_str(
            r#"{"accounts":[{"glob":"*@x","label":"w","color":"33"}],"options":{"n":3},"toggles":{"scope":true},"extra":1}"#,
        )
        .unwrap();
        assert_eq!(i.accounts.len(), 1);
        assert_eq!(i.options["n"], 3);
        assert_eq!(i.toggles["scope"], true);
    }
}
