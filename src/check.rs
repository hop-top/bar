//! `bar check`: lints the config file without rendering anything.

use crate::budget;
use crate::conf::{layers, layout_key, parse_options, project, tokenize, Layer};
use crate::expr;
use crate::format::{self, Token};
use crate::icons;
use crate::look;
use crate::options::Options;
use crate::placeholders::budget::{DEFAULT_SHOW as BUDGET_SHOW, SHOW_ITEMS as BUDGET_ITEMS};
use crate::placeholders::ctx::parts::SHOW_ITEMS as CTX_ITEMS;
use crate::placeholders::gh::{self, Kind};
use crate::placeholders::money::SHOW as MONEY_ITEMS;
use crate::placeholders::usage::glyphs::{GLYPH_SETS, SHOW_ITEMS};
use crate::placeholders::{gitdiff, gitst, jj, modes, open, remotes, speed, task};
use crate::plugin;
use crate::render::builtin_config;
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
    if let Some(spec) = task::options(name) {
        return Some(spec);
    }
    Some(match name {
        "account" | "sessions" | "burn" | "duration" | "api" | "lines" | "version" | "style"
        | "vim" | "agent" | "thinking" | "fast" | "update" => &[],
        "status" => &[
            ("show", List(&["state", "text", "strip"])),
            ("components", Any),
            ("ok", OneOf(&["0", "1"])),
            ("hours", Pos),
            ("max", Int),
            ("link", Any),
        ],
        "cost" => &[("fmt", OneOf(&["usd"]))],
        "spend" | "extra" => &[
            ("show", List(&MONEY_ITEMS)),
            ("count", OneOf(&["used", "left"])),
            ("glyphs", OneOf(&GLYPH_SETS)),
            ("width", Int),
        ],
        "cwd" | "root" => &[
            ("base", OneOf(&["0", "1"])),
            ("fish", Int),
            ("full", Int),
            ("depth", Int),
            ("open", OneOf(&open::VALUES)),
        ],
        "spendhist" => &[
            ("period", OneOf(&["today", "month", "both"])),
            ("tz", OneOf(&["local", "utc"])),
        ],
        "budget" => &[
            ("type", OneOf(&budget::Kind::NAMES)),
            ("show", List(&BUDGET_ITEMS)),
            ("count", OneOf(&["used", "left"])),
            ("glyphs", OneOf(&GLYPH_SETS)),
            ("width", Int),
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
        "repo" => &[("max", Int), ("link", Any), ("open", OneOf(&open::VALUES))],
        "branch" => &[("max", Int), ("link", Any), ("fish", Int), ("full", Int)],
        "origin" | "fork" => &[("show", List(remotes::SHOW)), ("max", Int), ("link", Any)],
        "upstream" => &[("max", Int), ("link", Any)],
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
            ("model", Any),
        ],
        "dirty" => &[("show", List(gitst::SHOW)), ("ttl", Pos)],
        "ahead" | "conflicts" => &[("ttl", Pos)],
        "diff" => &[
            ("scope", OneOf(gitdiff::SCOPES)),
            ("show", List(gitdiff::SHOW)),
            ("ttl", Pos),
        ],
        "age" => &[("ramp", Pos)],
        "sha" => &[("len", Int), ("link", Any)],
        "tag" => &[("max", Int), ("link", Any), ("ttl", Pos)],
        "gitop" => &[("step", Any)],
        "jj" => &[("show", List(jj::SHOW)), ("ttl", Pos), ("max", Int)],
        "name" => &[("max", Int)],
        "msgs" => &[("show", OneOf(&["user", "total"]))],
        "modes" => &[("show", List(modes::SHOW))],
        "worktree" | "stash" | "compactions" | "skills" => &[],
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

/// Default value (empty when there is none) and one-line help of option
/// `key` of core placeholder `name`, for the editors; `None` when it takes
/// no such option. Every option in `core_options` has an entry.
pub fn option_help(name: &str, key: &str) -> Option<(&'static str, &'static str)> {
    if let Some(help) = task::option_help(name, key) {
        return Some(help);
    }
    Some(match (name, key) {
        ("cost", "fmt") => ("usd", "number format; usd is the only one"),
        ("cwd" | "root", "base") => ("0", "1 shows the last path component only"),
        ("cwd" | "root" | "branch", "fish") => (
            "0",
            "cut each component but the last to this many cells; 0 keeps them whole",
        ),
        ("cwd" | "root" | "branch", "full") => ("1", "trailing components fish leaves whole"),
        ("cwd" | "root", "depth") => ("0", "show only the last this many components; 0 shows all"),
        ("cwd" | "root", "open") => (
            "none",
            "link the directory: files (file manager), vscode, cursor or zed",
        ),
        ("repo", "open") => (
            "none",
            "without a web address, link the directory: files, vscode, cursor or zed",
        ),
        ("spendhist", "period") => ("both", "today, this month, or both"),
        ("spendhist", "tz") => ("local", "bucket days by local or UTC date"),
        ("budget", "type") => (
            "session",
            "the session, today, this month or the 5-hour block",
        ),
        ("budget", "show") => (BUDGET_SHOW, "components, in order"),
        ("budget", "count") => ("used", "show what is used or what is left"),
        ("budget", "glyphs") => ("blocks", "glyph set of the bar"),
        ("budget", "width") => ("5", "bar cells (1-40)"),
        ("budget", "tz") => ("local", "bucket days by local or UTC date"),
        ("tokens", "show") => ("both", "input, output, their total, or both"),
        ("cache", "show") => ("both", "cache reads, writes, hit rate, or reads and writes"),
        ("cachettl", "ttl") => ("300", "seconds the cache lives; 3600 for the 1 h cache"),
        ("ttft" | "tps", "show") => ("last", "last turn, session average, or both"),
        ("misses", "show") => ("count,cause", "components: count, cause, last, cold"),
        ("agents", "names") => ("0", "1 appends the running subagents' types"),
        ("agents", "max") => ("", "truncate the names to this many characters"),
        ("model" | "effort" | "ctx", "click") => ("", "none turns the command link off"),
        ("ctx", "show") => ("pct", "components, in order"),
        ("ctx", "of") => (
            "window",
            "measure against the window or the compaction point",
        ),
        ("ctx", "count") => ("used", "show what is used or what is left"),
        ("ctx", "glyphs") => ("blocks", "glyph set of the bar"),
        ("ctx", "color") => ("solid", "one colour, or a gradient along the filled cells"),
        ("ctx", "width") => ("5", "bar cells (1-40)"),
        ("status", "show") => (
            "state",
            "components, in order: state, text (the active incident), strip (48 h history)",
        ),
        ("status", "components") => ("", "only these components, matched by name (`code|api`)"),
        ("status", "ok") => ("0", "1 shows a check mark when all is operational"),
        ("status", "hours") => ("6", "hours per cell of the strip"),
        ("status", "max") => ("", "truncate the incident text to this many characters"),
        ("status", "link") => ("", "false, no, off or 0 give plain text"),
        ("repo" | "branch" | "origin" | "upstream" | "fork", "max") => {
            ("", "truncate to this many characters")
        }
        ("repo" | "branch" | "origin" | "upstream" | "fork", "link") => {
            ("", "false, no, off or 0 give plain text")
        }
        ("origin" | "fork", "show") => ("owner,repo", "parts of the remote: host, owner, repo"),
        ("usage", "type") => ("5h", "the 5-hour or the weekly window"),
        ("usage", "view") => ("bar", "alias of show: bar, pct or time"),
        ("usage", "show") => ("time,bar", "components, in order"),
        ("usage", "alt") => ("", "components after a click; show, bar and pct swapped"),
        ("usage", "count") => ("left", "show what is left or what is used"),
        ("usage", "glyphs") => ("blocks", "glyph set of the bar"),
        ("usage", "color") => ("solid", "one colour, or a gradient along the filled cells"),
        ("usage", "width") => ("5", "bar cells (1-40) and spark samples"),
        ("usage", "click") => ("toggle", "none turns the toggle link off"),
        ("usage", "model") => ("", "a model's weekly window, e.g. opus (needs usage-api)"),
        ("spend", "show") => ("time", "components, in order"),
        ("extra", "show") => ("used,limit", "components, in order"),
        ("spend" | "extra", "count") => ("used", "pct, bar and gauge show used or left"),
        ("spend" | "extra", "glyphs") => ("blocks", "glyph set of the bar"),
        ("spend" | "extra", "width") => ("5", "bar cells (1-40)"),
        ("dirty", "show") => ("", "parts to show; all when unset"),
        ("dirty" | "ahead", "ttl") => ("10", "seconds before git status is refreshed"),
        ("conflicts", "ttl") => (
            "10",
            "seconds before git status is refreshed, for an index too large to read",
        ),
        ("diff", "scope") => ("both", "both (against HEAD), staged or unstaged"),
        ("diff", "show") => ("added,removed", "parts to show"),
        ("diff", "ttl") => ("10", "seconds before git diff is refreshed"),
        ("age", "ramp") => ("", "hours at which the colour reaches the bad end"),
        ("sha", "len") => ("7", "characters of the commit id; 0 hides it"),
        ("sha" | "tag", "link") => ("", "false, no, off or 0 give plain text"),
        ("tag", "max") => ("", "truncate to this many characters"),
        ("tag", "ttl") => (
            "10",
            "seconds before git lists the tags, when the files cannot tell",
        ),
        ("gitop", "step") => ("", "false, no, off or 0 leave out the rebase step"),
        ("jj", "show") => ("change,bookmarks,state", "parts to show, in display order"),
        ("jj", "ttl") => ("10", "seconds before jj log is refreshed"),
        ("jj", "max") => ("", "truncate the description to this many characters"),
        ("name", "max") => ("", "truncate to this many characters"),
        ("msgs", "show") => ("user", "user prompts, or the total with assistant messages"),
        ("modes", "show") => ("", "modes to show; all when unset"),
        ("sid", "len") => ("8", "characters of the session id; 0 hides it"),
        ("tmux", "max") => ("", "truncate session:window to this many characters"),
        ("prs" | "issues", "limit") => ("5", "most items shown; 0 hides the placeholder"),
        ("prs" | "issues", "author") => ("", "author expression, e.g. @me"),
        ("issues", "assignee") => ("", "assignee expression, e.g. @me"),
        ("issues", "label") => ("", "label expression, e.g. bug|p1"),
        ("prs" | "issues", "status") => ("open", "which items to list"),
        ("prs" | "issues", "link") => ("", "false, no, off or 0 give plain text"),
        ("prs", "checks") => ("fail", "CI states marked after the number"),
        _ => return None,
    })
}

/// A core placeholder's options as JSON: `[{"key", "kind", "values",
/// "default", "help"}]` (`default` empty when there is none),
/// `kind` one of `any`, `int`, `pos`, `one`, `list`; the options on every
/// placeholder (`look::schema`) are not listed. Empty for plugins.
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
            let (default, help) = option_help(name, key).unwrap_or(("", ""));
            json!({"key": key, "kind": kind, "values": values, "default": default, "help": help})
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
    /// `set max-rows` of the config as applied.
    max_rows: usize,
    defaults: BTreeMap<String, Options>,
    gh_missing: bool,
    gh_warned: Cell<bool>,
    budgets: budget::Budgets,
    /// `set usage-api on`.
    usage_api: bool,
    /// `set account-source`.
    account_source: Option<String>,
    /// Plugins whose `--check` already ran.
    checked: std::collections::BTreeSet<String>,
    /// `glab` is not on PATH while a GitLab remote is in play.
    glab_missing: bool,
    jj_missing: bool,
    issues: Vec<Issue>,
    /// File of the lines being linted.
    layer: Layer,
    /// Linting the `subagent` layout.
    row: bool,
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
        let in_row = task::lookup(name).is_some();
        if used && in_row && !self.row {
            self.error(line, format!("`{name}` works only in `set subagent`"));
            return;
        }
        if used && self.row && core_options(name).is_none() {
            let msg = format!("unknown placeholder `{name}` (`set subagent` draws no plugins)");
            self.error(line, msg);
            return;
        }
        match core_options(name) {
            Some(spec) => {
                let bad = self.core_values(name, spec, opts, line);
                if matches!(name, "prs" | "issues") {
                    self.gh_checks(name, opts, line, used, bad);
                }
                if name == "budget" && used && !bad {
                    self.budget_set(opts, line);
                }
                if used && !self.usage_api {
                    self.usage_api_off(name, opts, line);
                }
                if name == "jj" && used && self.jj_missing {
                    self.jj_missing = false;
                    let msg = "`jj` not on PATH: `jj` renders nothing".to_string();
                    self.add(Level::Warning, line, msg);
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
            if look::KEYS.contains(&k.as_str()) {
                bad |= self.look(k, v, line);
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

    /// Reports a bad `bg`, `fg`, `bold` or `gradient` value; true when
    /// there was one.
    fn look(&mut self, k: &str, v: &str, line: usize) -> bool {
        let msg = look::invalid(k, v).filter(|_| !v.contains('%'));
        let bad = msg.is_some();
        if let Some(m) = msg {
            self.error(line, m);
        }
        bad
    }

    /// Warns when `extra` or `usage[model=...]` is used while `set usage-api
    /// on` is not: they render nothing then.
    fn usage_api_off(&mut self, name: &str, opts: &Options, line: usize) {
        let merged = match self.defaults.get(name) {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let what = match name {
            "extra" => "`extra`",
            "usage" if merged.get("model").is_some_and(|m| !m.trim().is_empty()) => {
                "`usage[model=...]`"
            }
            _ => return,
        };
        let msg = format!("no `set usage-api on`: {what} renders nothing");
        self.add(Level::Warning, line, msg);
    }

    /// Warns when the budget a `budget` placeholder shows is not set.
    fn budget_set(&mut self, opts: &Options, line: usize) {
        let merged = match self.defaults.get("budget") {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let ty = merged.get("type").unwrap_or("session");
        if let Some(kind) = budget::Kind::parse(ty) {
            if self.budgets.get(kind).is_none() {
                let msg = format!("no `set budget-{ty}`: `budget[type={ty}]` renders nothing");
                self.add(Level::Warning, line, msg);
            }
        }
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
        if std::mem::take(&mut self.glab_missing) {
            self.add(
                Level::Warning,
                line,
                format!("`glab` not on PATH: `{name}` renders nothing on GitLab remotes"),
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
        if let Some(opt) = info.declares_look_option {
            let msg = format!("plugin `{name}` declares the reserved option `{opt}`; ignored");
            self.add(Level::Warning, line, msg);
        }
        for n in &info.inputs.refused {
            let msg = format!(
                "plugin `{name}` declares the input `{n}`, which looks like a credential; refused"
            );
            self.add(Level::Warning, line, msg);
        }
        if used
            && info
                .capabilities
                .iter()
                .any(|c| c == plugin::check::CHECK_CAPABILITY)
            && self.checked.insert(name.to_string())
        {
            for w in plugin::check::run(&found) {
                self.add(Level::Warning, line, format!("plugin `{name}`: {w}"));
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
            } else if look::KEYS.contains(&k.as_str()) {
                self.look(k, v, line);
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
    /// `set usage-source` / `set account-source` naming `name`: a plugin
    /// placeholder, or `#{usage}` / `#{account}` get nothing from it.
    fn source(&mut self, key: &str, name: &str) {
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
    #[allow(clippy::cognitive_complexity)] // over the threshold; split when next touched
    fn lint(&mut self, text: &str) {
        for (i, raw) in text.lines().enumerate() {
            let Ok(t) = tokenize(raw) else { continue };
            match t.first().map(String::as_str) {
                // a bad line number is already a parse error
                Some("set") if t.len() == 3 && matches!(layout_key(&t[1]), Some(Ok(_))) => {
                    self.layout(&t[2], i + 1);
                    if let Some(Ok((_, n))) = layout_key(&t[1]) {
                        self.past_max_rows(&t[1], n, &t[2], i + 1);
                    }
                }
                Some("set") if t.len() == 3 && t[1] == "subagent-hide" => {
                    for s in t[2].split(',').map(|s| s.trim().to_ascii_lowercase()) {
                        if !s.is_empty() && !crate::subagent::STATUSES.contains(&s.as_str()) {
                            let msg = format!(
                                "subagent-hide: `{s}` is not a status Claude Code documents ({})",
                                crate::subagent::STATUSES.join(", ")
                            );
                            self.add(Level::Warning, i + 1, msg);
                        }
                    }
                }
                Some("set") if t.len() == 3 && t[1] == "palette" => {
                    let file = crate::palette::file::path_of(&t[2]).filter(|p| p.is_file());
                    if let (Some(_), Some(file)) = (crate::palette::by_name(&t[2]), file) {
                        let msg = format!(
                            "`{}` is a built-in palette, so {} is not read; rename the file to use it",
                            t[2],
                            file.display()
                        );
                        self.add(Level::Warning, i + 1, msg);
                    }
                }
                Some("set") if t.len() == 3 && t[1] == "subagent" => {
                    self.row = true;
                    self.layout(&t[2], i + 1);
                    self.row = false;
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
        max_rows: crate::conf::DEFAULT_MAX_ROWS,
        defaults: BTreeMap::new(),
        gh_missing: !dirs
            .iter()
            .any(|d| crate::platform::executable_in(d, "gh").is_some()),
        gh_warned: Cell::new(false),
        budgets: Default::default(),
        usage_api: false,
        account_source: None,
        checked: Default::default(),
        glab_missing: false,
        jj_missing: !dirs
            .iter()
            .any(|d| crate::platform::executable_in(d, "jj").is_some()),
        issues: Vec::new(),
        layer: Layer::User,
        row: false,
    };
    let user_text = match user.map(|(_, t)| t) {
        Some(Err(e)) => {
            c.error(0, e);
            return Report {
                path,
                project: None,
                issues: c.issues,
                notes: Vec::new(),
            };
        }
        Some(Ok(t)) => Some(t),
        None => None,
    };
    let mut applied = None;
    let mut mode = project::ProjectMode::Off;
    let parsed = layers(user_text.as_deref(), |m| {
        mode = m;
        let text = project.as_ref()?.1.clone();
        applied = text.as_ref().ok().cloned();
        Some(text)
    });
    if let Ok(cfg) = &parsed.config {
        c.icon_width = cfg.icon_width;
        c.max_rows = cfg.max_rows;
        c.defaults = cfg.defaults.clone();
        c.budgets = cfg.budgets;
        c.usage_api = cfg.usage_api;
        c.account_source = cfg.account_source.clone();
        for (k, v) in [
            ("usage-source", &cfg.usage_source),
            ("account-source", &cfg.account_source),
        ] {
            if let Some(n) = v {
                c.source(k, n);
            }
        }
        let gitlab = !cfg.gitlab_hosts.is_empty() || cwd_on_gitlab(&cfg.gitlab_hosts);
        c.glab_missing = gitlab
            && !dirs
                .iter()
                .any(|d| crate::platform::executable_in(d, "glab").is_some());
    }
    for e in parsed.config.err().unwrap_or_default() {
        c.layer = e.layer;
        c.error(e.error.line, e.error.message);
    }
    // dropped project lines do not apply: warnings, not linted further
    c.layer = Layer::Project;
    for e in &parsed.dropped {
        let why = if e.line == 0 {
            format!("{}; the project config is ignored", e.message)
        } else {
            format!("{}; the line is ignored", e.message)
        };
        c.add(Level::Warning, e.line, why);
    }
    let applied = applied.map(|text| {
        let skip: Vec<usize> = parsed.dropped.iter().map(|e| e.line).collect();
        text.lines()
            .enumerate()
            .map(|(i, l)| if skip.contains(&(i + 1)) { "" } else { l })
            .collect::<Vec<_>>()
            .join("\n")
    });
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
        notes: Vec::new(),
    }
}

/// The working directory's `origin`, read from the repository files only,
/// is on gitlab.com or one of `hosts`.
fn cwd_on_gitlab(hosts: &[String]) -> bool {
    let Ok(dir) = std::env::current_dir() else {
        return false;
    };
    let crate::gitfs::Found::Repo(repo) = crate::gitfs::discover(&dir) else {
        return false;
    };
    repo.origin_url()
        .ok()
        .flatten()
        .and_then(|u| crate::placeholders::repo::web_url(&u))
        .is_some_and(|u| crate::placeholders::glab::gitlab_repo(&u, hosts).is_some())
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
