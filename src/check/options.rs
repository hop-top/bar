//! Option names, help, schema and value checks of the core placeholders.

use crate::budget;
use crate::placeholders::budget::{DEFAULT_SHOW as BUDGET_SHOW, SHOW_ITEMS as BUDGET_ITEMS};
use crate::placeholders::ctx::parts::SHOW_ITEMS as CTX_ITEMS;
use crate::placeholders::gh;
use crate::placeholders::money::SHOW as MONEY_ITEMS;
use crate::placeholders::usage::glyphs::{GLYPH_SETS, SHOW_ITEMS};
use crate::placeholders::{gitdiff, gitst, jj, modes, open, remotes, speed, task};
use serde_json::{json, Value};

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

pub(super) fn val_ok(v: Val, raw: &str) -> bool {
    match v {
        Val::Any => true,
        Val::Int => raw.trim().parse::<usize>().is_ok(),
        Val::Pos => raw.trim().parse::<usize>().is_ok_and(|n| n >= 1),
        Val::OneOf(set) => set.contains(&raw),
        Val::List(set) => raw.split(',').all(|i| set.contains(&i.trim())),
    }
}
