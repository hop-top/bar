//! `bar check`: lints the config file without rendering anything.

use crate::conf::{parse, parse_options, tokenize};
use crate::expr;
use crate::format::{self, Token};
use crate::icons;
use crate::options::Options;
use crate::placeholders::gh::{self, Kind};
use crate::plugin;
use crate::render::builtin_config;
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: Level,
    /// 1-based; 0 when the problem has no line (I/O, built-in layout).
    pub line: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct Report {
    /// The config file in use; `None` means the built-in layout.
    pub path: Option<PathBuf>,
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.level == Level::Error)
    }

    pub fn to_json(&self) -> Value {
        let issues: Vec<Value> = self
            .issues
            .iter()
            .map(|i| {
                json!({
                    "level": level_name(i.level),
                    "line": i.line,
                    "message": i.message,
                })
            })
            .collect();
        json!({
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "ok": !self.has_errors(),
            "issues": issues,
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = match &self.path {
            Some(p) => format!("config: {}\n", p.display()),
            None => "config: none found, using the built-in layout\n".to_string(),
        };
        for i in &self.issues {
            let at = if i.line == 0 {
                String::new()
            } else {
                format!("line {}: ", i.line)
            };
            out.push_str(&format!("{at}{}: {}\n", level_name(i.level), i.message));
        }
        let count = |l| self.issues.iter().filter(|i| i.level == l).count();
        let (e, w) = (count(Level::Error), count(Level::Warning));
        out.push_str(&if e + w == 0 {
            "ok\n".to_string()
        } else {
            format!("{e} error(s), {w} warning(s)\n")
        });
        out
    }
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Error => "error",
        Level::Warning => "warning",
    }
}

/// How an option value is checked.
#[derive(Clone, Copy)]
enum Val {
    Any,
    Int,
    OneOf(&'static [&'static str]),
}

const STATUS_PR: &[&str] = &["open", "closed", "merged"];
const STATUS_ISSUE: &[&str] = &["open", "closed"];

fn core_options(name: &str) -> Option<&'static [(&'static str, Val)]> {
    use Val::*;
    Some(match name {
        "account" | "sessions" | "spend" => &[],
        "model" | "effort" | "ctx" => &[("click", Any)],
        "repo" | "branch" => &[("max", Int), ("link", Any)],
        "usage" => &[
            ("type", OneOf(&["5h", "7d", "h", "d"])),
            ("view", OneOf(&["bar", "pct", "time"])),
            ("width", Int),
            ("click", OneOf(&["toggle", "none"])),
        ],
        "prs" => &[
            ("limit", Int),
            ("author", Any),
            ("status", OneOf(STATUS_PR)),
            ("link", Any),
        ],
        "issues" => &[
            ("limit", Int),
            ("assignee", Any),
            ("author", Any),
            ("label", Any),
            ("status", OneOf(STATUS_ISSUE)),
            ("link", Any),
        ],
        _ => return None,
    })
}

fn val_ok(v: Val, raw: &str) -> bool {
    match v {
        Val::Any => true,
        Val::Int => raw.trim().parse::<usize>().is_ok(),
        Val::OneOf(set) => set.contains(&raw),
    }
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

struct Checker<'a> {
    dirs: &'a [PathBuf],
    icon_width: usize,
    defaults: BTreeMap<String, Options>,
    gh_missing: bool,
    gh_warned: Cell<bool>,
    issues: Vec<Issue>,
}

impl Checker<'_> {
    fn add(&mut self, level: Level, line: usize, message: String) {
        self.issues.push(Issue {
            level,
            line,
            message,
        });
    }

    fn error(&mut self, line: usize, message: String) {
        self.add(Level::Error, line, message);
    }

    fn layout(&mut self, src: &str, line: usize) {
        match format::parse(src) {
            Err(e) => self.error(line, format!("invalid layout: {e}")),
            Ok(tokens) => {
                for t in tokens {
                    if let Token::Placeholder { name, options } = t {
                        self.placeholder(&name, &options, line, true);
                    }
                }
            }
        }
    }

    /// `used` is false for a `default` line: it only checks names and values.
    fn placeholder(&mut self, name: &str, opts: &Options, line: usize, used: bool) {
        match core_options(name) {
            Some(spec) => {
                let bad = self.core_values(name, spec, opts, line);
                if matches!(name, "prs" | "issues") {
                    self.gh_checks(name, opts, line, used, bad);
                }
            }
            None => self.plugin(name, opts, line, used),
        }
    }

    /// Reports bad options; true when there was any.
    fn core_values(
        &mut self,
        name: &str,
        spec: &[(&str, Val)],
        opts: &Options,
        line: usize,
    ) -> bool {
        let mut bad = false;
        for (k, v) in opts.iter() {
            if k == "icon" {
                bad |= self.icon(v, line);
                continue;
            }
            match spec.iter().find(|(n, _)| n == k) {
                None => {
                    bad = true;
                    self.error(line, format!("unknown option `{k}` for `{name}`"));
                }
                Some((_, kind)) if !v.contains('%') && !val_ok(*kind, v) => {
                    bad = true;
                    self.error(line, format!("invalid value `{v}` for `{k}`"));
                }
                _ => {}
            }
        }
        bad
    }

    /// Reports a bad `icon` value; true when there was one.
    fn icon(&mut self, v: &str, line: usize) -> bool {
        let bad = !v.contains('%') && !icons::valid(v, self.icon_width);
        if bad {
            let msg = format!(
                "invalid icon `{v}` (`none` or at most {} cells)",
                icons::MAX_CELLS
            );
            self.error(line, msg);
        }
        bad
    }

    fn gh_checks(&mut self, name: &str, opts: &Options, line: usize, used: bool, bad: bool) {
        if !used {
            return;
        }
        if self.gh_missing && !self.gh_warned.replace(true) {
            self.add(
                Level::Warning,
                line,
                format!("`gh` not on PATH: `{name}` renders nothing"),
            );
        }
        let kind = if name == "prs" { Kind::Pr } else { Kind::Issue };
        let merged = match self.defaults.get(name) {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let dynamic = merged.iter().any(|(_, v)| v.contains('%'));
        let mut expr_bad = false;
        for key in ["author", "assignee", "label"] {
            if let Some(v) = opts.get(key).filter(|v| !v.contains('%')) {
                if let Err(e) = expr::parse(v) {
                    expr_bad = true;
                    self.error(line, format!("invalid expression for `{key}`: {e}"));
                }
            }
        }
        if bad || expr_bad || dynamic {
            return;
        }
        match gh::plan(kind, "https://github.com/o/r", &merged) {
            Some(plan) => {
                if let Some(w) = plan.warning {
                    self.add(Level::Warning, line, w);
                }
            }
            None => self.error(line, format!("`{name}` options do not compile to queries")),
        }
    }

    fn plugin(&mut self, name: &str, opts: &Options, line: usize, used: bool) {
        let Some(found) = plugin::find(self.dirs, name) else {
            if used {
                self.error(line, format!("unknown placeholder `{name}`"));
            } else {
                // a `default` line may be shared by machines without the plugin
                let msg = format!("plugin `{name}` not on PATH");
                self.add(Level::Warning, line, msg);
            }
            return;
        };
        let info = plugin::info(&found);
        if !info.is_placeholder() {
            self.error(
                line,
                format!("plugin `{name}` does not declare the placeholder capability"),
            );
            return;
        }
        if info.declares_icon_option {
            let msg = format!("plugin `{name}` declares the reserved option `icon`; ignored");
            self.add(Level::Warning, line, msg);
        }
        let mut concrete = Options::default();
        for (k, v) in opts.iter().filter(|(_, v)| !v.contains('%')) {
            if k == "icon" {
                self.icon(v, line);
            } else {
                concrete.set(k, v);
            }
        }
        if let Err(e) = plugin::validate(&info.options, &concrete) {
            self.error(line, format!("{e} for `{name}`"));
        }
    }
}

/// Lints `text`, a config file's contents.
pub fn check_text(text: &str, path: Option<PathBuf>, dirs: &[PathBuf]) -> Report {
    let parsed = parse(text);
    let mut c = Checker {
        dirs,
        icon_width: parsed.as_ref().map_or(1, |cfg| cfg.icon_width),
        defaults: parsed
            .as_ref()
            .map(|cfg| cfg.defaults.clone())
            .unwrap_or_default(),
        gh_missing: !dirs.iter().any(|d| is_executable(&d.join("gh"))),
        gh_warned: Cell::new(false),
        issues: Vec::new(),
    };
    if let Err(errs) = &parsed {
        for e in errs {
            c.error(e.line, e.message.clone());
        }
    }
    for (i, raw) in text.lines().enumerate() {
        let Ok(t) = tokenize(raw) else { continue };
        match t.first().map(String::as_str) {
            Some("set") if t.len() == 3 && matches!(t[1].as_str(), "left" | "center" | "right") => {
                c.layout(&t[2], i + 1);
            }
            Some("default") if t.len() >= 2 => {
                if let Ok(o) = parse_options(&t[2..]) {
                    c.placeholder(&t[1], &o, i + 1, false);
                }
            }
            _ => {}
        }
    }
    c.issues.sort_by_key(|i| i.line);
    Report {
        path,
        issues: c.issues,
    }
}

/// Lints the built-in layout, used when no config file exists.
pub fn check_builtin(dirs: &[PathBuf]) -> Report {
    let cfg = builtin_config();
    let mut c = Checker {
        dirs,
        icon_width: 1,
        defaults: BTreeMap::new(),
        gh_missing: !dirs.iter().any(|d| is_executable(&d.join("gh"))),
        gh_warned: Cell::new(false),
        issues: Vec::new(),
    };
    for src in [&cfg.left, &cfg.center, &cfg.right] {
        c.layout(src, 0);
    }
    Report {
        path: None,
        issues: c.issues,
    }
}

/// Lints the file at `path`; an unreadable file is one error without a line.
pub fn check_file(path: &Path, dirs: &[PathBuf]) -> Report {
    match std::fs::read_to_string(path) {
        Ok(text) => check_text(&text, Some(path.to_path_buf()), dirs),
        Err(e) => Report {
            path: Some(path.to_path_buf()),
            issues: vec![Issue {
                level: Level::Error,
                line: 0,
                message: format!("{}: {e}", path.display()),
            }],
        },
    }
}
