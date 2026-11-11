//! The `plugins` part of the snapshot: one entry per plugin named by a
//! `tray` line, in config order. Each plugin's `--menu` runs in its own
//! thread (3 s cap each, so a slow plugin delays the menu once and never
//! blocks the others); its items are kept only when the plugin declares
//! their action with those token names, and an item whose action has no
//! `tray` grant is shown disabled, with the line that would enable it.
//! A plugin that cannot answer gets one information row saying why.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::click;
use crate::conf::{Config, TrayGrant};
use crate::options::Options;
use crate::plugin::{self, menu, ExtInfo, Menu, RESERVED};

/// Longest label or detail the snapshot carries, in characters.
pub const MAX_LABEL: usize = 80;
/// Longest dispatch id `<plugin>.<group>.<item>`.
pub const MAX_ID: usize = 96;

/// Plugin names as the snapshot takes them: `[a-z0-9_-]`, 1 to 32.
pub fn valid_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The plugins with a `tray` line, in order, once each; a name the
/// snapshot would refuse is left out.
pub fn granted_names(config: &Config) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in &config.tray {
        if valid_name(&g.plugin) && !out.contains(&g.plugin) {
            out.push(g.plugin.clone());
        }
    }
    out
}

/// A `tray <plugin>` or `tray <plugin>.<action>` line covers the action.
pub(super) fn granted(grants: &[TrayGrant], plugin: &str, action: &str) -> bool {
    grants
        .iter()
        .any(|g| g.plugin == plugin && g.action.as_deref().is_none_or(|a| a == action))
}

/// Every granted plugin's entry, menus run in parallel.
pub fn menus(state: &Path, config: &Config, dirs: &[PathBuf]) -> Vec<Value> {
    let names = granted_names(config);
    std::thread::scope(|s| {
        let handles: Vec<_> = names
            .iter()
            .map(|n| s.spawn(move || entry(state, config, dirs, n)))
            .collect();
        handles
            .into_iter()
            .zip(&names)
            .map(|(h, n)| {
                h.join()
                    .unwrap_or_else(|_| info_only(n, "failed", &format!("{n}: menu failed"), "bad"))
            })
            .collect()
    })
}

/// An entry with one disabled row: what the tray shows for a plugin
/// without a menu.
fn info_only(name: &str, id: &str, label: &str, style: &str) -> Value {
    json!({"name": name, "groups": [{"id": "info", "kind": "info", "items": [
        {"id": id, "label": label, "enabled": false, "style": style}
    ]}]})
}

/// One plugin's entry: found, declares `menu`, answers `--menu`.
fn entry(state: &Path, config: &Config, dirs: &[PathBuf], name: &str) -> Value {
    let Some(found) = plugin::find(dirs, name) else {
        return info_only(name, "missing", &format!("bar-{name} not found"), "bad");
    };
    let info = plugin::info(&found);
    if !info.has_menu() {
        return info_only(name, "no-menu", &format!("{name} has no menu"), "dim");
    }
    let input = menu::menu_input(
        options(config, name, &info),
        toggles(state, name),
        accounts(config, &info),
    );
    let Some(m) = menu::run(&found, &input, state) else {
        return info_only(name, "no-menu", &format!("{name} gave no menu"), "warn");
    };
    let mut m = m.declared(&info);
    fit(&mut m, name, &config.tray);
    let mut v = m.to_json();
    json!({"name": name, "groups": v["groups"].take()})
}

/// The plugin's `default <name>` options, typed with the schema's
/// defaults filled in; the schema defaults alone when a value is invalid.
fn options(config: &Config, name: &str, info: &ExtInfo) -> Map<String, Value> {
    let mut opts = Options::default();
    if let Some(d) = config.defaults.get(name) {
        for (k, v) in d.iter().filter(|(k, _)| !RESERVED.contains(&k.as_str())) {
            opts.set(k, v);
        }
    }
    plugin::validate(&info.options, &opts)
        .or_else(|_| plugin::validate(&info.options, &Options::default()))
        .unwrap_or_default()
}

/// The plugin's named toggles that are on: the global defaults, since the
/// tray belongs to no session.
fn toggles(state: &Path, name: &str) -> Map<String, Value> {
    click::plugin_toggles(state, name, "")
        .into_iter()
        .map(|(k, v)| (k, Value::Bool(v)))
        .collect()
}

/// The user's `account` rules, for a plugin that declared it wants them.
fn accounts(config: &Config, info: &ExtInfo) -> Value {
    if info.inputs.accounts {
        plugin::account_rules(config)
    } else {
        json!([])
    }
}

fn cut(s: &mut String) {
    if s.chars().count() > MAX_LABEL {
        *s = s
            .chars()
            .take(MAX_LABEL)
            .collect::<String>()
            .trim()
            .to_string();
    }
}

/// Makes a declared menu fit the snapshot: dispatch ids within
/// [`MAX_ID`] (longer items are dropped), text within [`MAX_LABEL`], and
/// an action without a `tray` grant disabled with the line that grants it.
fn fit(m: &mut Menu, name: &str, grants: &[TrayGrant]) {
    for g in &mut m.groups {
        let prefix = name.len() + g.id.len() + 2;
        g.items.retain(|i| prefix + i.id.len() <= MAX_ID);
        if let Some(l) = &mut g.label {
            cut(l);
        }
        for i in &mut g.items {
            if let Some(a) = i.action.as_deref().filter(|a| !granted(grants, name, a)) {
                i.enabled = false;
                i.detail = Some(format!("not enabled: tray {name}.{a}"));
            }
            cut(&mut i.label);
            if let Some(d) = &mut i.detail {
                cut(d);
            }
        }
    }
    m.groups.retain(|g| !g.items.is_empty());
}
