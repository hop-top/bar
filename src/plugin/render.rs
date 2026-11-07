//! Render input, output parsing and colouring.

use serde_json::{json, Map, Value};

use super::info::ttl_of;
use super::{cached_info, find, validate, ExtInfo, Found};
use crate::cache::{hash_hex, Job};
use crate::conf::Config;
use crate::options::Options;
use crate::pace::metric_score;
use crate::placeholders::Ctx;
use crate::session::{Session, Window};
use crate::style::{dim, fg, link, ramp, sanitize};

fn window_json(w: &Option<Window>) -> Value {
    w.as_ref().map_or(
        Value::Null,
        |w| json!({"used_percentage": w.used_percentage, "resets_at": w.resets_at}),
    )
}

fn session_json(s: &Session) -> Value {
    json!({
        "cwd": s.cwd,
        "model": s.model,
        "model_id": s.model_id,
        "effort": s.effort,
        "session_id": s.session_id,
        "context_used": s.context_used,
        "five_hour": window_json(&s.five_hour),
        "seven_day": window_json(&s.seven_day),
    })
}

/// What one placeholder fetches: cached per option set, fresh for `ttl`.
pub struct Plan {
    pub key: String,
    pub job: Job,
    pub ttl: u64,
}

/// `None` when the plugin is not a placeholder or the options are invalid.
/// `config` is the plugin's own `plugin <name>` line, minus the core-owned
/// `ttl`; nothing else from the config or the environment goes in.
pub fn plan(ctx: &Ctx, found: &Found, info: &ExtInfo, opts: &Options) -> Option<Plan> {
    if !info.is_placeholder() {
        return None;
    }
    let options = validate(&info.options, opts).ok()?;
    let mut config = Map::new();
    let mut ttl = info.ttl;
    if let Some(own) = ctx.config.plugins.get(&found.name) {
        for (k, v) in own.iter() {
            if k == "ttl" {
                ttl = ttl_of(&Value::from(v.as_str()));
            } else {
                config.insert(k.clone(), Value::from(v.as_str()));
            }
        }
    }
    let (options, config) = (Value::Object(options), Value::Object(config));
    let path = found.path.to_string_lossy().into_owned();
    // per working directory (plugins like tlc read the repo there), but not
    // per volatile session data, so the cache can hit
    let cwd = hash_hex(&ctx.session.cwd);
    let key = format!("plugin:{}:{path}:{cwd}:{options}:{config}", found.name);
    let input = json!({
        "options": options,
        "session": session_json(ctx.session),
        "columns": ctx.columns,
        "config": config,
    });
    Some(Plan {
        key,
        job: Job::PluginRender {
            path,
            input: input.to_string(),
        },
        ttl: ttl.max(ctx.config.refresh),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    /// Cents.
    pub remaining: i64,
    /// Cents.
    pub granted: i64,
    /// Unix seconds.
    pub expires_at: i64,
    /// Unix seconds; gives the pace period when present.
    pub granted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub text: String,
    pub label: Option<String>,
    pub style: Option<String>,
    pub link: Option<String>,
    pub metric: Option<Metric>,
    pub score: Option<f64>,
}

fn int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite())
            .map(|f| f.round() as i64)
    })
}

fn metric(v: &Value) -> Option<Metric> {
    Some(Metric {
        remaining: int(&v["remaining"])?,
        granted: int(&v["granted"])?,
        expires_at: int(&v["expires_at"])?,
        granted_at: int(&v["granted_at"]),
    })
}

/// `None` unless a JSON object with string `text`; optional fields of the
/// wrong type are ignored.
pub fn parse_output(raw: &str) -> Option<Output> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    Some(Output {
        text: v.get("text")?.as_str()?.to_string(),
        label: v["label"].as_str().map(String::from),
        style: v["style"].as_str().map(String::from),
        link: v["link"].as_str().map(String::from),
        metric: metric(&v["metric"]),
        score: v["score"].as_f64().filter(|f| !f.is_nan()),
    })
}

/// Sanitised, coloured, linked text, after a dim unlinked `label`; `None` when nothing visible is left.
/// `style` beats `score`, `score` beats `metric` + `rule <name>`.
pub fn paint(out: &Output, name: &str, config: &Config, now: i64) -> Option<String> {
    let text = sanitize(&out.text);
    if text.trim().is_empty() {
        return None;
    }
    let colors = &config.colors;
    let body = if let Some(style) = &out.style {
        match style.as_str() {
            "good" => fg(colors.good, &text),
            "warn" => fg(colors.warn, &text),
            "bad" => fg(colors.bad, &text),
            "dim" => dim(&text),
            _ => text,
        }
    } else if let Some(s) = out.score {
        fg(
            ramp(s.clamp(0.0, 100.0).round() as i32, &config.ramp),
            &text,
        )
    } else if let Some((m, rule)) = out
        .metric
        .as_ref()
        .zip(config.rules.iter().find(|r| r.target == name))
    {
        let secs = Some(m.expires_at.saturating_sub(now)).filter(|&s| s > 0);
        let period = m.granted_at.map_or(0, |g| m.expires_at.saturating_sub(g));
        let s = metric_score(m.remaining, m.granted, secs, period, rule);
        fg(ramp(s, &config.ramp), &text)
    } else {
        text
    };
    let body = match out.link.as_deref().filter(|l| !l.is_empty()) {
        Some(url) => link(url, &body),
        None => body,
    };
    let label = out.label.as_deref().map(sanitize).unwrap_or_default();
    Some(if label.trim().is_empty() {
        body
    } else {
        format!("{} {body}", dim(&label))
    })
}

/// Cached output for `found`; empty while cold, on failure or bad options.
pub fn render_found(ctx: &Ctx, found: &Found, opts: &Options) -> Option<String> {
    let cache = ctx.cache?;
    let info = cached_info(cache, found)?;
    let plan = plan(ctx, found, &info, opts)?;
    let raw = cache.read_or_refresh(&plan.key, plan.ttl, &plan.job)?;
    paint(&parse_output(&raw)?, &found.name, ctx.config, ctx.now)
}

/// `#{name}` for a plugin found in `ctx.plugin_dirs`.
pub fn render(ctx: &Ctx, name: &str, opts: &Options) -> Option<String> {
    render_found(ctx, &find(ctx.plugin_dirs, name)?, opts)
}
