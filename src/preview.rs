//! `bar preview`: the configured layout filled with built-in sample data.
//! Reads no session, account, git, gh or plugin; touches no state.

use crate::conf::{Config, LoadResult};
use crate::options::Options;
use crate::placeholders::{lookup, usage, Ctx};
use crate::render::{builtin_config, render_sample};
use crate::session::{Session, Window};
use crate::style::{bold, dim, fg};
use std::path::Path;

pub const STATES: [&str; 3] = ["early", "burning", "final"];
const DEFAULT_COLUMNS: usize = 120;

/// Seconds to reset and percent used, per window.
struct Pace {
    five: (i64, f64),
    seven: (i64, f64),
}

fn pace(state: &str) -> Pace {
    const H: i64 = 3600;
    const D: i64 = 86_400;
    match state {
        "burning" => Pace {
            five: (3 * H, 75.0),
            seven: (5 * D, 70.0),
        },
        // last stretch with budget left over: the "unused" case
        "final" => Pace {
            five: (25 * 60, 50.0),
            seven: (20 * H, 30.0),
        },
        _ => Pace {
            five: (4 * H + 40 * 60, 10.0),
            seven: (6 * D, 12.0),
        },
    }
}

fn session(state: &str, now: i64) -> Session {
    let p = pace(state);
    let win = |(secs, used): (i64, f64)| Window {
        used_percentage: used,
        resets_at: now + secs,
    };
    Session {
        cwd: String::new(),
        model: "Claude Opus 4.7".into(),
        model_id: "sample".into(),
        effort: "high".into(),
        session_id: String::new(),
        context_used: Some(38.0),
        five_hour: Some(win(p.five)),
        seven_day: Some(win(p.seven)),
        spend_limit: None,
    }
}

fn limit(opts: &Options) -> usize {
    opts.get("limit")
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(5)
}

fn sample(name: &str, opts: &Options, ctx: &Ctx, label: bool) -> Option<String> {
    let c = &ctx.config.colors;
    let list = |items: Vec<String>| {
        let v: Vec<String> = items.into_iter().take(limit(opts)).collect();
        (!v.is_empty()).then(|| v.join(" "))
    };
    match name {
        "repo" => Some(bold(&fg(75, "bar"))),
        "branch" => Some(dim("main")),
        "dirty" => Some(format!(
            "{} {} {}",
            fg(c.good, "+2"),
            fg(c.warn, "~3"),
            dim("?1")
        )),
        "ahead" => Some(format!("{} {}", fg(c.good, "↑2"), fg(c.warn, "↓1"))),
        "prs" => list(vec![
            fg(c.bad, "#131"),
            fg(c.warn, "#129"),
            fg(c.good, "#128"),
        ]),
        "issues" => list(vec![fg(c.bad, "!57"), dim("!42")]),
        "account" => {
            let (label, color) = ctx
                .config
                .accounts
                .first()
                .map_or(("work", 75), |a| (a.label.as_str(), a.color));
            Some(bold(&fg(color, label)))
        }
        "usage" => usage::render_with(ctx, opts, Path::new("/nonexistent/bar-preview")),
        "model" | "effort" | "ctx" => lookup(name)?.render_labeled(ctx, opts, label),
        _ => None,
    }
}

/// The preview line(s) for `state`; the user's layout when the config
/// is valid, else the built-in one.
pub fn render(state: &str, load: LoadResult, columns: Option<usize>, now: i64) -> String {
    let config: Config = match load {
        LoadResult::Ok(c) => c,
        _ => builtin_config(),
    };
    render_sample(
        &session(state, now),
        &config,
        Some(columns.unwrap_or(DEFAULT_COLUMNS)),
        &sample,
    )
}
