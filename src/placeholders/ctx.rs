use super::usage::glyphs::{self, level};
use super::{Ctx, Placeholder};
use crate::click::{self, Term};
use crate::compact;
use crate::options::Options;
use crate::style::{dim, fg, ramp};
use crate::transcript::{self, Stats};
use serde_json::Value;
use std::path::Path;

pub mod parts;

use parts::{parse_show, signed, Part};

/// `ctx 38%`: context used, on the reversed ramp (green at 0, red above
/// 90), plus optional bar, gauge, token counts, room before
/// auto-compaction, the cache split and growth per request. Clicking it
/// types `/compact`.
pub struct CtxUsed;

impl Placeholder for CtxUsed {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        let state = click::state_dir();
        let stats = || transcript::load(ctx.cache?, &ctx.session.transcript_path);
        let src = Sources {
            state: &state,
            stats: &stats,
            env: &|k| std::env::var(k).ok(),
            settings: &user_settings,
        };
        render_parts(ctx, opts, &src, label, || Term::current().clone())
    }
}

/// Everything besides the session the placeholder reads, so tests and
/// `bar preview` can stand in for it.
pub struct Sources<'a> {
    /// Holds the click handler marker.
    pub state: &'a Path,
    /// Transcript totals, for `growth` and `trend`.
    pub stats: &'a dyn Fn() -> Option<Stats>,
    /// An environment variable of the Claude Code process.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Claude Code's user settings file, parsed.
    pub settings: &'a dyn Fn() -> Option<Value>,
}

fn user_settings() -> Option<Value> {
    crate::settings::user().cloned()
}

/// `state` holds the handler marker; `term` is probed only for a link.
/// Reads no transcript, environment or settings.
pub fn render_with(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let src = Sources {
        state,
        stats: &|| None,
        env: &|_| None,
        settings: &|| None,
    };
    render_parts(ctx, opts, &src, true, term)
}

/// Ramp score of a used percent: 100 at 0%, 0 from 91%.
pub(crate) fn score(used: f64) -> i32 {
    let pct = used.clamp(0.0, 100.0).round() as i64;
    (100 - pct * 100 / 91).clamp(0, 100) as i32
}

fn width_opt(opts: &Options) -> i64 {
    opts.get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40)
}

pub fn render_parts(
    ctx: &Ctx,
    opts: &Options,
    src: &Sources,
    label: bool,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let s = ctx.session;
    let show = match opts.get("show").map(parse_show) {
        Some(list) if !list.is_empty() => list,
        _ => vec![Part::Pct],
    };
    let used = s.context_used.filter(|u| u.is_finite());
    let size = s.context_size;
    // tokens in the window: the last request's inputs, else the JSON
    // total, else derived from the percent
    let from_usage = s.context_usage.as_ref().map(|u| {
        u.input
            .saturating_add(u.cache_creation)
            .saturating_add(u.cache_read)
    });
    let tokens = from_usage.or(s.context_tokens).or_else(|| {
        let (u, n) = (used?, size?);
        Some((u.clamp(0.0, 100.0) * n as f64 / 100.0).round() as u64)
    });
    let of_compact = opts.get("of") == Some("compact");
    let threshold = if of_compact || show.contains(&Part::Left) {
        size.and_then(|w| {
            let cfg = ctx.config.compact_at;
            compact::threshold(w, &s.model_id, cfg, src.env, src.settings)
        })
    } else {
        None
    };
    // the scale the percent, bar, gauge and token count measure against
    let (pct, scale) = match (of_compact, threshold, tokens) {
        (true, Some(t), Some(n)) => (Some(n as f64 * 100.0 / t as f64), Some(t)),
        _ => (used, size),
    };
    let count_left = opts.get("count") == Some("left");
    let value = pct.map(|p| {
        let p = p.clamp(0.0, 100.0).round() as i64;
        if count_left {
            100 - p
        } else {
            p
        }
    });
    let color = pct.map(|p| ramp(score(p), &ctx.config.ramp));
    let gradient = opts.get("color") == Some("gradient");
    let stats = std::cell::OnceCell::new();
    let stats = || stats.get_or_init(|| (src.stats)()).clone();
    let out: Vec<String> = show
        .iter()
        .filter_map(|p| match p {
            Part::Pct => Some(fg(color?, &format!("{}%", value?))),
            Part::Gauge => Some(fg(color?, &level(value? as f64).to_string())),
            Part::Bar => {
                let (v, c, used) = (value?, color?, pct?);
                let glyph_set = opts.get("glyphs").unwrap_or("blocks");
                Some(glyphs::bar(glyph_set, width_opt(opts), v, &|i, n| {
                    if !gradient {
                        return c;
                    }
                    // good end first for used, bad end first for left;
                    // the last filled cell is the solid colour
                    let reach = |x: f64| x * (i as f64 + 1.0) / n as f64;
                    let at = if count_left {
                        100.0 - reach(100.0 - used)
                    } else {
                        reach(used)
                    };
                    ramp(score(at), &ctx.config.ramp)
                }))
            }
            Part::Tokens => {
                let (n, of) = (tokens?, scale?);
                let c = color.unwrap_or(ctx.config.muted.into());
                Some(format!(
                    "{}{}",
                    fg(c, &parts::num(n)),
                    dim(&format!("/{}", parts::num(of)))
                ))
            }
            Part::Left => {
                let (t, n) = (threshold?, tokens?);
                let c = ramp(score(n as f64 * 100.0 / t as f64), &ctx.config.ramp);
                let room = t.saturating_sub(n);
                Some(format!("{} {}", fg(c, &parts::num(room)), dim("left")))
            }
            Part::Split => parts::split(s.context_usage.as_ref()?),
            Part::Growth => {
                let st = stats()?;
                (st.ctx_reqs >= 2).then(|| dim(&signed(st.ctx_last as i64 - st.ctx_prev as i64)))
            }
            Part::Trend => {
                let st = stats()?;
                (st.ctx_reqs >= 2).then(|| {
                    let per = (st.ctx_last as i64 - st.ctx_first as i64) / (st.ctx_reqs as i64 - 1);
                    dim(&format!("{}/req", signed(per)))
                })
            }
        })
        .collect();
    if out.is_empty() {
        return None;
    }
    let value = click::with_cmd_link(
        out.join(" "),
        "compact",
        &s.session_id,
        opts,
        src.state,
        term,
    );
    Some(if label {
        format!("{} {value}", dim("ctx"))
    } else {
        value
    })
}
