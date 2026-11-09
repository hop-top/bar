//! `bar preview`: the configured layout filled with built-in sample data.
//! Reads no session, account, git, gh or plugin; touches no state.

use crate::click::Term;
use crate::conf::{Config, LoadResult};
use crate::icons;
use crate::options::Options;
use crate::placeholders::ctx as ctxp;
use crate::placeholders::{
    dirs, gh, lookup, model, modes, niche, speed, spendhist, task, tokens, update, usage, Ctx,
};
use crate::plugin::{self, ExtInfo};
use crate::render::{builtin_config, render_sample};
use crate::session::{CtxUsage, Session, SpendLimit, Window};
use crate::style::{accent, bold, dim, fg};
use serde_json::{json, Value};
use std::path::Path;

pub const STATES: [&str; 3] = ["early", "burning", "final"];
const DEFAULT_COLUMNS: usize = 120;

/// State dir stand-in: no click handler, toggles or usage history.
const NO_STATE: &str = "/nonexistent/bar-preview";

/// Never probed: links need a click handler, and `NO_STATE` has none.
fn no_term() -> Term {
    Term {
        macos: false,
        tty: None,
    }
}

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
        project_dir: String::new(),
        prompt_cache: None,
        model: "Claude Opus 4.7".into(),
        model_id: "sample".into(),
        effort: "high".into(),
        session_id: String::new(),
        name: "fix-login-bug".into(),
        transcript_path: String::new(),
        context_used: Some(38.0),
        context_size: Some(200_000),
        context_tokens: Some(76_000),
        context_usage: Some(CtxUsage {
            input: 1_200,
            cache_creation: 4_800,
            cache_read: 70_000,
        }),
        five_hour: Some(win(p.five)),
        seven_day: Some(win(p.seven)),
        spend_limit: Some(SpendLimit {
            period: "monthly".into(),
            resets_at: now + 3 * 86_400 + 4 * 3600,
        }),
        cost_usd: Some(1.84),
        duration_ms: Some(27.0 * 60_000.0),
        api_duration_ms: Some(9.0 * 60_000.0),
        lines_added: 120,
        lines_removed: 14,
        version: "2.1.0".into(),
        output_style: "Explanatory".into(),
        vim_mode: "NORMAL".into(),
        agent: "reviewer".into(),
        thinking: true,
        fast_mode: true,
        task: None,
    }
}

/// `gh pr list` rows: #131 failing CI, #129 checks running, #128 green.
fn sample_prs() -> [Value; 3] {
    let pr = |n: u32, rollup: Value| {
        json!({
            "number": n,
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE",
            "statusCheckRollup": rollup,
        })
    };
    let run = |status: &str, conclusion: &str| json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion});
    [
        pr(
            131,
            json!([run("COMPLETED", "SUCCESS"), run("COMPLETED", "FAILURE")]),
        ),
        pr(
            129,
            json!([run("COMPLETED", "SUCCESS"), run("IN_PROGRESS", "")]),
        ),
        pr(128, json!([run("COMPLETED", "SUCCESS")])),
    ]
}

fn limit(opts: &Options) -> usize {
    opts.get("limit")
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(5)
}

fn sample(state: &str, name: &str, opts: &Options, ctx: &Ctx, label: bool) -> Option<String> {
    let c = &ctx.config.colors;
    let list = |items: Vec<String>| {
        let v: Vec<String> = items.into_iter().take(limit(opts)).collect();
        (!v.is_empty()).then(|| v.join(" "))
    };
    match name {
        "repo" => Some(bold(&fg(accent(), "bar"))),
        "branch" => Some(dim("main")),
        // a clean tree in the last stretch
        "dirty" if state == "final" => None,
        "dirty" => Some(format!(
            "{} {} {}",
            fg(c.good, "+2"),
            fg(c.warn, "~3"),
            dim("?1")
        )),
        "ahead" => Some(format!("{} {}", fg(c.good, "↑2"), fg(c.warn, "↓1"))),
        "prs" => list(
            sample_prs()
                .iter()
                .map(|pr| {
                    let (text, mark) = gh::pr_parts(ctx, opts, pr);
                    text + &mark
                })
                .collect(),
        ),
        "issues" => list(vec![fg(c.bad, "!57"), dim("!42")]),
        "account" => {
            let (label, color) = ctx
                .config
                .accounts
                .first()
                .map_or(("work", accent().into()), |a| (a.label.as_str(), a.color));
            Some(bold(&fg(color, label)))
        }
        "agents" if label => Some(format!("{} 2", dim("agents"))),
        "agents" => Some("2".into()),
        "usage" => usage::render_with(ctx, opts, Path::new(NO_STATE)),
        "spendhist" => {
            let period = opts.get("period").unwrap_or("both");
            Some(spendhist::fmt(period, 3.12, false, 84.10, false))
        }
        "tokens" => tokens::tokens(ctx, opts, &tokens::sample_stats(ctx.now)),
        "cache" => tokens::cache(ctx, opts, &tokens::sample_stats(ctx.now)),
        "cachettl" => tokens::cachettl(ctx, opts, &tokens::sample_stats(ctx.now)),
        "ttft" => speed::ttft(opts, &tokens::sample_stats(ctx.now).turns, label),
        "tps" => speed::tps(opts, &tokens::sample_stats(ctx.now).turns),
        "misses" => speed::misses(ctx, opts, &tokens::sample_stats(ctx.now).turns, label),
        "worktree" => Some(dim("bar-feat")),
        "sid" => Some(dim("a1b2c3d4")),
        "tmux" => Some(dim("work:1")),
        "update" => Some(update::fmt(ctx, "0.2.0")),
        "compactions" | "skills" | "msgs" => niche::sample(name, opts, label),
        "modes" => Some(dim(&modes::sample(opts).join(" "))),
        "sessions" => Some(format!(
            "{} {} {}",
            fg(c.warn, ">1"),
            bold(&fg(c.bad, "?1")),
            fg(c.good, "v2")
        )),
        "cwd" | "root" => dirs::shown("~/src/bar", opts, ""),
        // fields of the session, read without state or settings
        "model" => model::model_with(ctx, opts, Path::new(NO_STATE), no_term),
        "effort" if ctx.session.effort.is_empty() => None,
        "effort" => model::effort_with(ctx, opts, Path::new(NO_STATE), no_term),
        "ctx" => {
            let stats = || Some(tokens::sample_stats(ctx.now));
            let src = ctxp::Sources {
                state: Path::new(NO_STATE),
                stats: &stats,
                env: &|_| None,
                settings: &|| None,
            };
            ctxp::render_parts(ctx, opts, &src, label, no_term)
        }
        "spend" | "cost" | "burn" | "duration" | "lines" | "version" | "style" | "vim"
        | "agent" | "thinking" | "fast" | "name" | "api" => {
            lookup(name)?.render_labeled(ctx, opts, label)
        }
        _ => None,
    }
}

/// `config` filled from `session` where it has the data and from the
/// `early` samples elsewhere (git, gh, transcript, accounts), at `now`.
/// Plugins draw the sample their `plugins` entry gives for the options
/// (`plugin::web::sample`), painted as a real output; a plugin with no
/// entry draws its name. Pure: no clock, files, processes or plugin runs.
pub fn render_session(
    session: &Session,
    config: &Config,
    columns: usize,
    now: i64,
    plugins: &[ExtInfo],
) -> String {
    render_sample(
        session,
        config,
        Some(columns),
        now,
        &|name, opts, ctx, label| match lookup(name) {
            Some(_) => sample("early", name, opts, ctx, label),
            None => plugin_sample(name, opts, ctx, plugins, false),
        },
    )
}

/// A plugin's sample output through bar's plugin paint. Options are
/// checked as for a real render: a listed plugin with options it does not
/// accept draws nothing. With `stub`, a plugin with no matching sample
/// (or such options) draws its name, muted, instead.
fn plugin_sample(
    name: &str,
    opts: &Options,
    ctx: &Ctx,
    plugins: &[ExtInfo],
    stub: bool,
) -> Option<String> {
    let (pick, opts) = icons::split(opts);
    let mut own = Options::default();
    for (k, v) in opts.iter().filter(|(k, _)| k.as_str() != "click") {
        own.set(k, v);
    }
    let found = plugins.iter().find(|p| p.name == name);
    let typed = match found {
        Some(i) => plugin::validate(&i.options, &own).ok(),
        None => Some(Default::default()),
    };
    let info = found.cloned().unwrap_or_else(|| ExtInfo::named(name));
    let out = match typed {
        _ if stub => match typed.and_then(|t| plugin::web::matching_sample(&info, &t)) {
            Some(o) => o,
            None => return Some(dim(name)),
        },
        Some(t) => plugin::web::sample(&info, &t),
        None => return None,
    };
    plugin::paint_with(&out, name, ctx.config, ctx.now, &pick, info.icon.as_deref())
}

/// The preview line(s) for `state`; the user's layout when the config
/// is valid, else the built-in one.
pub fn render(state: &str, load: LoadResult, columns: Option<usize>, now: i64) -> String {
    let config: Config = match load {
        LoadResult::Ok(c) => c,
        _ => builtin_config(),
    };
    render_config(state, &config, columns, now, None)
}

/// The preview line(s) of `config`. `plugins: None` leaves plugin
/// placeholders empty, as `bar preview` does; with `Some`, a plugin draws
/// its matching `web` sample from that list, else its name, muted.
pub fn render_config(
    state: &str,
    config: &Config,
    columns: Option<usize>,
    now: i64,
    plugins: Option<&[ExtInfo]>,
) -> String {
    render_sample(
        &session(state, now),
        config,
        Some(columns.unwrap_or(DEFAULT_COLUMNS)),
        now,
        &|name, opts, ctx, label| match (lookup(name), plugins) {
            (None, Some(list)) => plugin_sample(name, opts, ctx, list, true),
            _ => sample(state, name, opts, ctx, label),
        },
    )
}

/// `bar preview --subagent`: the `subagent` layout of `load` drawn for
/// sample subagent rows, one per line. Row placeholders read the sample
/// tasks; the rest answer as for the status line preview.
pub fn subagent(load: LoadResult, columns: Option<usize>, now: i64) -> String {
    let config: Config = match load {
        LoadResult::Ok(c) => c,
        _ => builtin_config(),
    };
    let layout = config
        .subagent
        .as_deref()
        .unwrap_or(crate::subagent::DEFAULT_LAYOUT);
    let p = crate::subagent::sample(now);
    let answer = |name: &str, opts: &Options, ctx: &Ctx, label: bool| match task::lookup(name) {
        Some(t) => t.render_labeled(ctx, opts, label),
        None => sample("early", name, opts, ctx, label),
    };
    p.tasks
        .iter()
        .map(|t| {
            let s = crate::subagent::session(&p, t);
            crate::render::render_row_sample(&s, &config, layout, columns, now, &answer)
        })
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
