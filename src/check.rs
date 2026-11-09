//! `bar check`: lints the config file without rendering anything.

use crate::conf::{layout_key, parse_layers, parse_options, project, tokenize, Layer};
use crate::expr;
use crate::format::{self, Token};
use crate::icons;
use crate::options::Options;
use crate::placeholders::ctx::parts::SHOW_ITEMS as CTX_ITEMS;
use crate::placeholders::gh::{self, Kind};
use crate::placeholders::usage::glyphs::{GLYPH_SETS, SHOW_ITEMS};
use crate::placeholders::{gitst, modes, speed};
use crate::plugin;
use crate::render::builtin_config;
use crate::skin;
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod report;
pub use report::{Issue, Level, ProjectInfo, Report};

/// How an option value is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Val {
    Any,
    Int,
    /// Integer of at least 1.
    Pos,
    OneOf(&'static [&'static str]),
    /// Comma list whose items all come from the set.
    List(&'static [&'static str]),
}

const STATUS_PR: &[&str] = &["open", "closed", "merged"];
const STATUS_ISSUE: &[&str] = &["open", "closed"];

/// Options a core placeholder takes and how each value is checked; `None`
/// for names that are not core placeholders.
pub fn core_options(name: &str) -> Option<&'static [(&'static str, Val)]> {
    use Val::*;
    Some(match name {
        "account" | "sessions" | "spend" | "burn" | "duration" | "api" | "lines" | "version"
        | "style" | "vim" | "agent" | "thinking" | "fast" | "update" => &[],
        "cost" => &[("fmt", OneOf(&["usd"]))],
        "cwd" | "root" => &[("base", OneOf(&["0", "1"]))],
        "spendhist" => &[
            ("period", OneOf(&["today", "month", "both"])),
            ("tz", OneOf(&["local", "utc"])),
        ],
        "tokens" => &[("show", OneOf(&["in", "out", "total", "both"]))],
        "cache" => &[("show", OneOf(&["read", "write", "hit", "both"]))],
        "cachettl" => &[("ttl", Pos)],
        "ttft" | "tps" => &[("show", OneOf(&["last", "avg", "both"]))],
        "misses" => &[("show", List(speed::SHOW))],
        "agents" => &[("names", OneOf(&["0", "1"])), ("max", Int)],
        "model" | "effort" => &[("click", Any)],
        "ctx" => &[
            ("click", Any),
            ("show", List(&CTX_ITEMS)),
            ("of", OneOf(&["window", "compact"])),
            ("count", OneOf(&["used", "left"])),
            ("glyphs", OneOf(&GLYPH_SETS)),
            ("color", OneOf(&["solid", "gradient"])),
            ("width", Int),
        ],
        "repo" | "branch" => &[("max", Int), ("link", Any)],
        "usage" => &[
            ("type", OneOf(&["5h", "7d", "h", "d"])),
            ("view", OneOf(&["bar", "pct", "time"])),
            ("show", List(&SHOW_ITEMS)),
            ("alt", List(&SHOW_ITEMS)),
            ("count", OneOf(&["left", "used"])),
            ("glyphs", OneOf(&GLYPH_SETS)),
            ("color", OneOf(&["solid", "gradient"])),
            ("width", Int),
            ("click", OneOf(&["toggle", "none"])),
        ],
        "dirty" => &[("show", List(gitst::SHOW)), ("ttl", Pos)],
        "ahead" => &[("ttl", Pos)],
        "name" => &[("max", Int)],
        "msgs" => &[("show", OneOf(&["user", "total"]))],
        "modes" => &[("show", List(modes::SHOW))],
        "worktree" | "compactions" | "skills" => &[],
        "sid" => &[("len", Int)],
        "tmux" => &[("max", Int)],
        "prs" => &[
            ("limit", Int),
            ("author", Any),
            ("status", OneOf(STATUS_PR)),
            ("link", Any),
            ("checks", List(gh::CHECK_MARKS)),
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

/// A core placeholder's options as JSON: `[{"key", "kind", "values"}]`,
/// `kind` one of `any`, `int`, `pos`, `one`, `list`; `icon` and `bg`
/// apply to every placeholder and are not listed. Empty for plugins.
pub fn option_schema(name: &str) -> Value {
    let opts: Vec<Value> = core_options(name)
        .unwrap_or(&[])
        .iter()
        .map(|(key, v)| {
            let (kind, values): (&str, &[&str]) = match v {
                Val::Any => ("any", &[]),
                Val::Int => ("int", &[]),
                Val::Pos => ("pos", &[]),
                Val::OneOf(set) => ("one", set),
                Val::List(set) => ("list", set),
            };
            json!({"key": key, "kind": kind, "values": values})
        })
        .collect();
    Value::Array(opts)
}

fn val_ok(v: Val, raw: &str) -> bool {
    match v {
        Val::Any => true,
        Val::Int => raw.trim().parse::<usize>().is_ok(),
        Val::Pos => raw.trim().parse::<usize>().is_ok_and(|n| n >= 1),
        Val::OneOf(set) => set.contains(&raw),
        Val::List(set) => raw.split(',').all(|i| set.contains(&i.trim())),
    }
}

struct Checker<'a> {
    dirs: &'a [PathBuf],
    icon_width: usize,
    defaults: BTreeMap<String, Options>,
    gh_missing: bool,
    gh_warned: Cell<bool>,
    issues: Vec<Issue>,
    /// File of the lines being linted.
    layer: Layer,
}

impl Checker<'_> {
    fn add(&mut self, level: Level, line: usize, message: String) {
        self.issues.push(Issue {
            level,
            line,
            message,
            layer: self.layer,
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
            if k == "bg" {
                bad |= self.bg(v, line);
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

    /// Reports a bad `bg` value; true when there was one.
    fn bg(&mut self, v: &str, line: usize) -> bool {
        let bad = !v.contains('%') && !skin::valid_bg(v);
        if bad {
            self.error(line, format!("invalid bg `{v}` (0-255)"));
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
        for (declared, opt) in [
            (info.declares_icon_option, "icon"),
            (info.declares_bg_option, "bg"),
            (info.declares_click_option, "click"),
        ] {
            if declared {
                let msg = format!("plugin `{name}` declares the reserved option `{opt}`; ignored");
                self.add(Level::Warning, line, msg);
            }
        }
        for issue in plugin::web::issues(&info) {
            self.add(
                Level::Warning,
                line,
                format!("plugin `{name}`: web: {issue}"),
            );
        }
        let mut concrete = Options::default();
        for (k, v) in opts.iter().filter(|(_, v)| !v.contains('%')) {
            if k == "icon" {
                self.icon(v, line);
            } else if k == "bg" {
                self.bg(v, line);
            } else if k == "click" {
                if !val_ok(Val::OneOf(&["toggle", "none"]), v) {
                    self.error(line, format!("invalid value `{v}` for `click`"));
                }
            } else {
                concrete.set(k, v);
            }
        }
        if let Err(e) = plugin::validate(&info.options, &concrete) {
            self.error(line, format!("{e} for `{name}`"));
        }
    }
}

impl Checker<'_> {
    /// Lints every line of `text`, a config file's contents.
    fn lint(&mut self, text: &str) {
        for (i, raw) in text.lines().enumerate() {
            let Ok(t) = tokenize(raw) else { continue };
            match t.first().map(String::as_str) {
                // a bad line number is already a parse error
                Some("set") if t.len() == 3 && matches!(layout_key(&t[1]), Some(Ok(_))) => {
                    self.layout(&t[2], i + 1);
                }
                Some("set") if t.len() == 3 && t[1] == "claude-dir" => {
                    if !Path::new(&t[2]).is_absolute() && !t[2].starts_with("~/") {
                        self.add(
                            Level::Warning,
                            i + 1,
                            "claude-dir must be an absolute path (or start with ~/); it is ignored"
                                .into(),
                        );
                    }
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
}

/// A config file and its contents, or why it could not be read.
pub type Source = (PathBuf, Result<String, String>);

/// Reads `path` for `check_layers`.
pub fn source(path: &Path) -> Source {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
    (path.to_path_buf(), text)
}

/// Lints the user config (`None`: the built-in layout) and the project
/// config over it, as `bar render` would apply them. An unreadable file
/// is one error without a line.
pub fn check_layers(user: Option<Source>, project: Option<Source>, dirs: &[PathBuf]) -> Report {
    let path = user.as_ref().map(|(p, _)| p.clone());
    let mut c = Checker {
        dirs,
        icon_width: 1,
        defaults: BTreeMap::new(),
        gh_missing: !dirs
            .iter()
            .any(|d| crate::platform::executable_in(d, "gh").is_some()),
        gh_warned: Cell::new(false),
        issues: Vec::new(),
        layer: Layer::User,
    };
    let user_text = match user.map(|(_, t)| t) {
        Some(Err(e)) => {
            c.error(0, e);
            return Report {
                path,
                project: None,
                issues: c.issues,
            };
        }
        Some(Ok(t)) => Some(t),
        None => None,
    };
    let mut applied = None;
    let mut mode = project::ProjectMode::Off;
    let parsed = parse_layers(user_text.as_deref(), |m| {
        mode = m;
        let text = project.as_ref()?.1.clone();
        applied = text.as_ref().ok().cloned();
        Some(text)
    });
    if let Ok(cfg) = &parsed {
        c.icon_width = cfg.icon_width;
        c.defaults = cfg.defaults.clone();
    }
    for e in parsed.err().unwrap_or_default() {
        c.layer = e.layer;
        c.error(e.error.line, e.error.message);
    }
    c.layer = Layer::User;
    match &user_text {
        Some(text) => c.lint(text),
        None => {
            let cfg = builtin_config();
            for src in [&cfg.left, &cfg.center, &cfg.right] {
                c.layout(src, 0);
            }
        }
    }
    if let Some(text) = &applied {
        c.layer = Layer::Project;
        c.lint(text);
    }
    c.issues
        .sort_by_key(|i| (i.layer == Layer::Project, i.line));
    Report {
        path,
        project: project.map(|(path, _)| ProjectInfo { path, mode }),
        issues: c.issues,
    }
}

/// Lints `text`, a user config file's contents, on its own.
pub fn check_text(text: &str, path: Option<PathBuf>, dirs: &[PathBuf]) -> Report {
    let p = path.unwrap_or_default();
    let mut r = check_layers(Some((p, Ok(text.to_string()))), None, dirs);
    r.path = r.path.filter(|p| !p.as_os_str().is_empty());
    r
}

/// Lints the built-in layout, used when no config file exists.
pub fn check_builtin(dirs: &[PathBuf]) -> Report {
    check_layers(None, None, dirs)
}

/// Lints the file at `path`; an unreadable file is one error without a line.
pub fn check_file(path: &Path, dirs: &[PathBuf]) -> Report {
    check_layers(Some(source(path)), None, dirs)
}
