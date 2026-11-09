use super::{Ctx, Placeholder};
use crate::click;
use crate::color::Color;
use crate::conf::Rule;
use crate::history;
use crate::options::Options;
use crate::pace::{builtin_rule, forecast, score};
use crate::session::Window;
use crate::style::{dim, fg, link, ramp};
use glyphs::{level, parse_list, swapped, view_alias, Part};
use std::path::Path;

pub mod glyphs;

/// `2h13m ███░░`: time to reset plus a pace-coloured bar, percentage or time.
pub struct Usage;

impl Placeholder for Usage {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts, &click::state_dir())
    }
}

/// `3d4h`, `3d`, `2h13m`, `1h0m`, `45m`, `0m`.
pub fn time_left(secs: i64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (1.., 0) => format!("{d}d"),
        (1.., _) => format!("{d}d{h}h"),
        (0, 1..) => format!("{h}h{m}m"),
        _ => format!("{m}m"),
    }
}

/// Percent left, rounded half up, clamped to 0..=100.
pub fn pct_left(used: f64) -> i64 {
    if !used.is_finite() {
        return 0;
    }
    (100.0 - used).clamp(0.0, 100.0).round() as i64
}

fn rule_for(ctx: &Ctx, key: &str) -> Rule {
    let target = format!("usage[type={key}]");
    ctx.config
        .rules
        .iter()
        .find(|r| r.target == target)
        .cloned()
        .unwrap_or_else(|| builtin_rule(key))
}

/// Key, window and period in seconds for the `type` option.
fn pick<'a>(ctx: &'a Ctx, ty: &str) -> Option<(&'static str, &'a Window, i64)> {
    match ty {
        "5h" | "h" => Some(("5h", ctx.session.five_hour.as_ref()?, 5 * 3600)),
        "7d" | "d" => Some(("7d", ctx.session.seven_day.as_ref()?, 7 * 86400)),
        _ => None,
    }
}

/// `state` holds the toggle files and the handler marker.
pub fn render_with(ctx: &Ctx, opts: &Options, state: &Path) -> Option<String> {
    let (key, win, period) = pick(ctx, opts.get("type").unwrap_or("5h"))?;
    let secs = win.resets_at.saturating_sub(ctx.now);
    if secs <= 0 {
        return None;
    }
    let left = pct_left(win.used_percentage);
    let rule = rule_for(ctx, key);
    let color = ramp(score(left, Some(secs), period, &rule), &ctx.config.ramp);
    let time = dim(&time_left(secs));
    let mut show = match opts.get("show") {
        Some(raw) if !parse_list(raw).is_empty() => parse_list(raw),
        _ => view_alias(opts.get("view").unwrap_or("bar")),
    };
    let alt = match opts.get("alt").map(parse_list) {
        Some(alt) if !alt.is_empty() => alt,
        _ => swapped(&show),
    };
    // a view that can be toggled to must keep recording
    let mut series = Vec::new();
    if show.iter().chain(&alt).any(|p| *p == Part::Spark) {
        series = history::sample(state, key, win.resets_at, ctx.now, win.used_percentage);
    }
    if click::is_toggled(state, key) {
        show = alt;
    }
    let used_count = opts.get("count") == Some("used");
    let value = if used_count { 100 - left } else { left };
    let linked =
        opts.get("click").unwrap_or("toggle") == "toggle" && click::handler_installed(state);
    let gradient = opts.get("color") == Some("gradient");
    let wrap = |body: String| {
        if linked {
            link(&format!("bar://toggle/{key}"), &body)
        } else {
            body
        }
    };
    let bad = ctx.config.colors.bad;
    let cast = forecast(win.used_percentage, secs, period);
    let parts: Vec<String> = show
        .iter()
        .filter_map(|p| {
            Some(match p {
                Part::Time => time.clone(),
                Part::Pct => wrap(fg(color, &format!("{value}%"))),
                Part::Bar => wrap(bar(opts, value, &|i, n| {
                    if !gradient {
                        return color;
                    }
                    // bad end up to the current score; the tip is the solid colour
                    let reach = |v: i64| v * (i as i64 + 1) / n as i64;
                    let at = if used_count {
                        100 - reach(100 - left)
                    } else {
                        reach(left)
                    };
                    ramp(score(at, Some(secs), period, &rule), &ctx.config.ramp)
                })),
                Part::Spark => {
                    if series.len() < 2 {
                        return None;
                    }
                    let n = width_opt(opts) as usize;
                    let tail = &series[series.len().saturating_sub(n)..];
                    // pure value ramp: no time-left or final-stretch input
                    spark(tail, gradient, color, |left| {
                        ramp(score(left, None, period, &rule), &ctx.config.ramp)
                    })
                }
                Part::Gauge => wrap(fg(color, &level(value as f64).to_string())),
                Part::Eta => {
                    let f = cast?;
                    if f.proj > 100 {
                        fg(bad, &format!("→{}", time_left(f.eta_secs)))
                    } else {
                        dim("→ok")
                    }
                }
                Part::Proj => {
                    let f = cast?;
                    let c = if f.proj > 100 {
                        bad
                    } else {
                        // 1 keeps ramp red for projections over budget only
                        let sc = score(100 - f.proj, None, period, &rule).max(1);
                        ramp(sc, &ctx.config.ramp)
                    };
                    fg(c, &format!("p{}%", f.proj))
                }
            })
        })
        .collect();
    Some(parts.join(" "))
}

fn width_opt(opts: &Options) -> i64 {
    opts.get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40)
}

/// Used percents as `▁..█`: one colour, or each cell by its own value.
fn spark(used: &[f64], gradient: bool, color: Color, by_left: impl Fn(i64) -> Color) -> String {
    if !gradient {
        let cells: String = used.iter().map(|&u| level(u)).collect();
        return fg(color, &cells);
    }
    used.iter()
        .map(|&u| fg(by_left(pct_left(u)), &level(u).to_string()))
        .collect()
}

/// Bar cells; `color(i, n)` is the colour of drawn cell `i` of `n`.
fn bar(opts: &Options, value: i64, color: &dyn Fn(usize, usize) -> Color) -> String {
    glyphs::bar(
        opts.get("glyphs").unwrap_or("blocks"),
        width_opt(opts),
        value,
        color,
    )
}
