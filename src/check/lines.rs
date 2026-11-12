//! Config line checks: sources, actions, max-rows, `set` keys.

use crate::conf::{layout_key, parse_options, tokenize};
use crate::plugin;
use std::path::Path;

use super::*;

impl Checker<'_> {
    /// `set usage-source` / `set account-source` naming `name`: a plugin
    /// placeholder, or `#{usage}` / `#{account}` get nothing from it.
    pub(super) fn source(&mut self, key: &str, name: &str) {
        if crate::placeholders::lookup(name).is_some() {
            self.error(
                0,
                format!("`set {key} {name}`: `{name}` is a core placeholder, not a plugin"),
            );
            return;
        }
        let Some(found) = plugin::find(self.dirs, name) else {
            let msg = format!("`set {key} {name}`: plugin `{name}` not on PATH");
            self.add(Level::Warning, 0, msg);
            return;
        };
        if !plugin::info(&found).is_placeholder() {
            let msg = format!(
                "`set {key} {name}`: plugin `{name}` does not declare the placeholder capability"
            );
            self.error(0, msg);
        }
    }

    /// An `action <placeholder> <plugin>.<action>` line.
    fn action(&mut self, placeholder: &str, target: &str, line: usize) {
        let Some((plug, act)) = target.split_once('.') else {
            return; // a parse error already
        };
        if crate::placeholders::lookup(placeholder).is_some() {
            if placeholder != "account" {
                let msg = format!("`action {placeholder}`: only `account` (core) and plugin placeholders take an action");
                self.error(line, msg);
            } else if self.account_source.as_deref() != Some(plug) {
                let msg = format!("`action account {target}` needs `set account-source {plug}`: the account's tokens come from that plugin");
                self.error(line, msg);
            }
        } else if placeholder != plug {
            let msg = format!("`action {placeholder} {target}`: a plugin placeholder can bind only its own actions (`action {placeholder} {placeholder}.<action>`)");
            self.error(line, msg);
        }
        let Some(found) = plugin::find(self.dirs, plug) else {
            let msg = format!("plugin `{plug}` not on PATH: `action {placeholder}` does nothing");
            self.add(Level::Warning, line, msg);
            return;
        };
        if !plugin::info(&found).actions.contains_key(act) {
            let msg = format!("plugin `{plug}` does not declare the action `{act}`");
            self.error(line, msg);
        }
    }

    /// A `tray <plugin>[.<action>]` line: warns when the grant cannot
    /// show anything.
    fn tray(&mut self, target: &str, line: usize) {
        let (plug, act) = match target.split_once('.') {
            Some((p, a)) => (p, Some(a)),
            None => (target, None),
        };
        let Some(found) = plugin::find(self.dirs, plug) else {
            let msg = format!("`tray {target}`: plugin `{plug}` not on PATH");
            self.add(Level::Warning, line, msg);
            return;
        };
        let info = plugin::info(&found);
        if !info.has_menu() {
            let msg =
                format!("`tray {target}`: plugin `{plug}` does not declare the `menu` capability");
            self.add(Level::Warning, line, msg);
        }
        if let Some(act) = act.filter(|a| !info.actions.contains_key(*a)) {
            let msg =
                format!("`tray {target}`: plugin `{plug}` does not declare the action `{act}`");
            self.add(Level::Warning, line, msg);
        }
    }

    /// A layout for line `n` past `set max-rows`: it prints only when fewer
    /// rows come before it.
    fn past_max_rows(&mut self, key: &str, n: usize, src: &str, line: usize) {
        if n > self.max_rows && !src.trim().is_empty() {
            let max = self.max_rows;
            self.add(
                Level::Warning,
                line,
                format!(
                    "`{key}` is line {n}, past max-rows {max}: rows after the first {max} are dropped, so it shows only when lines before it render empty (`set max-rows {n}` to keep it)"
                ),
            );
        }
    }

    /// Lints every line of `text`, a config file's contents.
    pub(super) fn lint(&mut self, text: &str) {
        let mut seen = BTreeMap::new();
        for (i, raw) in text.lines().enumerate() {
            let Ok(t) = tokenize(raw) else { continue };
            self.repeat(&mut seen, &t, i + 1);
            match t.first().map(String::as_str) {
                Some("set") if t.len() == 3 => self.set(&t[1], &t[2], i + 1),
                Some("action") if t.len() == 3 => self.action(&t[1], &t[2], i + 1),
                Some("tray") if t.len() == 2 && self.layer == Layer::User => {
                    self.tray(&t[1], i + 1)
                }
                Some("default") if t.len() >= 2 => {
                    if let Ok(o) = parse_options(&t[2..]) {
                        self.placeholder(&t[1], &o, i + 1, false);
                    }
                }
                _ => {}
            }
        }
    }

    /// A `plugin <name>` or `action <placeholder>` line after one for the
    /// same name in this file: the later line replaces the earlier whole.
    /// A project file's line over the user's is layering, not a repeat.
    fn repeat(&mut self, seen: &mut BTreeMap<(String, String), usize>, t: &[String], line: usize) {
        let applies = match t.first().map(String::as_str) {
            Some("plugin") => t.len() >= 2 && parse_options(&t[2..]).is_ok(),
            Some("action") => t.len() == 3 && t[2].contains('.'),
            _ => false,
        };
        if !applies {
            return;
        }
        let (dir, name) = (&t[0], &t[1]);
        let Some(prev) = seen.insert((dir.clone(), name.clone()), line) else {
            return;
        };
        let fix = if dir == "plugin" {
            format!("put all its options on one line: `plugin {name} k=v k2=v2 ...`")
        } else {
            "keep the one you want".to_string()
        };
        let msg = format!(
            "`{dir} {name}` again: this line replaces the one on line {prev} (only the last `{dir} {name}` line applies); {fix}"
        );
        self.add(Level::Warning, line, msg);
    }

    /// `set <key> <value>` on line `line`.
    fn set(&mut self, key: &str, value: &str, line: usize) {
        // a bad line number is already a parse error
        if let Some(Ok((_, n))) = layout_key(key) {
            self.layout(value, line);
            self.past_max_rows(key, n, value, line);
            return;
        }
        match key {
            "subagent-hide" => self.subagent_hide(value, line),
            "palette" => {
                let file = crate::palette::file::path_of(value).filter(|p| p.is_file());
                if let (Some(_), Some(file)) = (crate::palette::by_name(value), file) {
                    let msg = format!(
                        "`{value}` is a built-in palette, so {} is not read; rename the file to use it",
                        file.display()
                    );
                    self.add(Level::Warning, line, msg);
                }
            }
            "subagent" => {
                self.row = true;
                self.layout(value, line);
                self.row = false;
            }
            "claude-dir" if !Path::new(value).is_absolute() && !value.starts_with("~/") => {
                self.add(
                    Level::Warning,
                    line,
                    "claude-dir must be an absolute path (or start with ~/); it is ignored".into(),
                );
            }
            _ => {}
        }
    }

    /// `set subagent-hide a,b`: each status is one Claude Code documents.
    fn subagent_hide(&mut self, value: &str, line: usize) {
        for s in value.split(',').map(|s| s.trim().to_ascii_lowercase()) {
            if !s.is_empty() && !crate::subagent::STATUSES.contains(&s.as_str()) {
                let msg = format!(
                    "subagent-hide: `{s}` is not a status Claude Code documents ({})",
                    crate::subagent::STATUSES.join(", ")
                );
                self.add(Level::Warning, line, msg);
            }
        }
    }
}
