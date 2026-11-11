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
        for (i, raw) in text.lines().enumerate() {
            let Ok(t) = tokenize(raw) else { continue };
            match t.first().map(String::as_str) {
                Some("set") if t.len() == 3 => self.set(&t[1], &t[2], i + 1),
                Some("action") if t.len() == 3 => self.action(&t[1], &t[2], i + 1),
                Some("default") if t.len() >= 2 => {
                    if let Ok(o) = parse_options(&t[2..]) {
                        self.placeholder(&t[1], &o, i + 1, false);
                    }
                }
                _ => {}
            }
        }
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
