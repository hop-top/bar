//! Placeholders of a `bar subagent` row: the subagent Claude Code
//! describes in `tasks`. Outside a row they render nothing.

use super::{truncate, Ctx, Placeholder};
use crate::check::Val;
use crate::options::Options;
use crate::placeholders::usage::glyphs::level;
use crate::style::{dim, fg, ramp, sanitize};
use crate::subagent::Task;

/// `reviewer`: the name the subagent is addressed by, else its type.
pub struct Name;
/// `Explore`: the subagent type.
pub struct Type;
/// The task description Claude gave when it started the subagent.
pub struct Desc;
/// What the subagent is doing now, else the description.
pub struct Label;
/// `●` running, `✓` completed, `✗` failed or killed.
pub struct Status;
/// Time since the subagent started, while it runs.
pub struct Elapsed;
/// `41k`: the subagent's running token count.
pub struct Tokens;
/// `▁▃▅▇`: recent token counts.
pub struct Trend;

static REGISTRY: &[(&str, &(dyn Placeholder + Sync))] = &[
    ("task.name", &Name),
    ("task.type", &Type),
    ("task.desc", &Desc),
    ("task.label", &Label),
    ("task.status", &Status),
    ("task.elapsed", &Elapsed),
    ("task.tokens", &Tokens),
    ("task.trend", &Trend),
];

/// Names of the row placeholders, in registry order.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(n, _)| *n)
}

/// `None` for names that are not row placeholders.
pub fn lookup(name: &str) -> Option<&'static dyn Placeholder> {
    REGISTRY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p as &'static dyn Placeholder)
}

/// Options a row placeholder takes, for `bar check`.
pub fn options(name: &str) -> Option<&'static [(&'static str, Val)]> {
    use Val::*;
    Some(match name {
        "task.name" | "task.type" | "task.desc" | "task.label" => &[("max", Int)],
        "task.status" => &[("show", OneOf(&["mark", "text", "both"]))],
        "task.elapsed" | "task.tokens" => &[],
        "task.trend" => &[("width", Pos)],
        _ => return None,
    })
}

/// Default and help of option `key` of row placeholder `name`.
pub fn option_help(name: &str, key: &str) -> Option<(&'static str, &'static str)> {
    Some(match (name, key) {
        ("task.name" | "task.type" | "task.desc" | "task.label", "max") => {
            ("", "truncate to this many characters")
        }
        ("task.status", "show") => ("mark", "the mark, the status word, or both"),
        ("task.trend", "width") => ("8", "most recent samples drawn (1-16)"),
        _ => return None,
    })
}

/// One-line description of row placeholder `name`; empty for others.
pub fn describe(name: &str) -> &'static str {
    match name {
        "task.name" => "subagent name, else its type",
        "task.type" => "subagent type",
        "task.desc" => "task description",
        "task.label" => "what the subagent is doing now",
        "task.status" => "running, completed or failed",
        "task.elapsed" => "time since the subagent started",
        "task.tokens" => "subagent token count",
        "task.trend" => "recent subagent token counts",
        _ => "",
    }
}

fn task<'a>(ctx: &Ctx<'a>) -> Option<&'a Task> {
    ctx.session.task.as_ref()
}

/// Sanitized, truncated by `max`, hidden when empty.
fn text(s: &str, opts: &Options) -> Option<String> {
    let s = sanitize(s).trim().to_string();
    (!s.is_empty()).then(|| truncate(s, opts))
}

impl Placeholder for Name {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let t = task(ctx)?;
        let name = if t.name.trim().is_empty() {
            &t.agent_type
        } else {
            &t.name
        };
        text(name, opts).map(|n| fg(crate::style::accent(), &n))
    }
}

impl Placeholder for Type {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        text(&task(ctx)?.agent_type, opts).map(|s| dim(&s))
    }
}

impl Placeholder for Desc {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        text(&task(ctx)?.description, opts)
    }
}

impl Placeholder for Label {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let t = task(ctx)?;
        let label = if t.label.trim().is_empty() {
            &t.description
        } else {
            &t.label
        };
        text(label, opts)
    }
}

impl Placeholder for Status {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let t = task(ctx)?;
        let c = &ctx.config.colors;
        let (mark, color) = match t.status.as_str() {
            "" | "running" => ("●", crate::style::accent()),
            "completed" => ("✓", c.good),
            "failed" | "killed" => ("✗", c.bad),
            _ => ("○", ctx.config.muted),
        };
        let word = sanitize(if t.status.is_empty() {
            "running"
        } else {
            &t.status
        });
        Some(match opts.get("show") {
            Some("text") => fg(color, &word),
            Some("both") => format!("{} {}", fg(color, mark), dim(&word)),
            _ => fg(color, mark),
        })
    }
}

impl Placeholder for Elapsed {
    // Claude Code sends no end time, so a finished task has no duration.
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        let t = task(ctx)?;
        if !t.running() {
            return None;
        }
        let ms = (ctx.now as f64 * 1000.0 - t.start_ms?).max(0.0);
        Some(dim(&super::cost::compact(ms)))
    }
}

impl Placeholder for Tokens {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        let t = task(ctx)?;
        let n = t.tokens?;
        let text = super::tokens::compact(n);
        Some(match t.context_size {
            Some(size) => {
                let used = n as f64 * 100.0 / size as f64;
                fg(ramp(super::ctx::score(used), &ctx.config.ramp), &text)
            }
            None => dim(&text),
        })
    }
}

impl Placeholder for Trend {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let t = task(ctx)?;
        let width = opts
            .get("width")
            .and_then(|w| w.parse::<usize>().ok())
            .unwrap_or(8)
            .clamp(1, crate::subagent::MAX_SAMPLES);
        let s = &t.samples[t.samples.len().saturating_sub(width)..];
        if s.len() < 2 {
            return None;
        }
        // against the window when known, else the largest sample
        let scale = t
            .context_size
            .or_else(|| s.iter().copied().max())
            .filter(|n| *n > 0)?;
        let cells: String = s
            .iter()
            .map(|&n| level(n as f64 * 100.0 / scale as f64))
            .collect();
        Some(dim(&cells))
    }
}
