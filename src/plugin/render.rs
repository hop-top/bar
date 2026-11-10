//! Render input, output parsing and colouring.

use std::path::Path;

use serde_json::{json, Map, Value};

use super::info::ttl_of;
use super::{cached_info, find, validate, ExtInfo, Found};
use crate::cache::{hash_hex, Job};
use crate::click::{self, hook::read_record};
use crate::conf::Config;
use crate::icons::{self, Pick};
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

fn session_json(s: &Session, record: Option<Value>) -> Value {
    let mut v = json!({
        "cwd": s.cwd,
        "model": s.model,
        "model_id": s.model_id,
        "effort": s.effort,
        "session_id": s.session_id,
        "context_used": s.context_used,
        "five_hour": window_json(&s.five_hour),
        "seven_day": window_json(&s.seven_day),
    });
    if let Some(r) = record {
        v["record"] = r;
    }
    v
}

/// What one placeholder fetches: cached per option set, fresh for `ttl`.
pub struct Plan {
    pub key: String,
    pub job: Job,
    pub ttl: u64,
}

/// `None` when the plugin is not a placeholder or the options are invalid.
/// `config` is the plugin's own `plugin <name>` line, minus the core-owned
/// `ttl`; nothing else from the config or the environment goes in. The
/// reserved `click` option stays with bar.
pub fn plan(ctx: &Ctx, found: &Found, info: &ExtInfo, opts: &Options) -> Option<Plan> {
    if !info.is_placeholder() {
        return None;
    }
    let mut own = Options::default();
    for (k, v) in opts.iter().filter(|(k, _)| k.as_str() != "click") {
        own.set(k, v);
    }
    let options = validate(&info.options, &own).ok()?;
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
    // per volatile session data, so the cache can hit; a session-scoped
    // plugin also per session id and the record the hook keeps for it
    let cwd = hash_hex(&ctx.session.cwd);
    let mut scope = cwd;
    let mut record = None;
    // named label/icon toggles: part of the key, so a flip is a new entry
    let toggles = ctx.cache.map_or_else(Default::default, |c| {
        click::plugin_toggles(c.state_dir(), &found.name)
    });
    let toggles = json!(toggles);
    if info.session_cache {
        let sid = &ctx.session.session_id;
        record = ctx
            .cache
            .and_then(|c| read_record(c.state_dir(), &found.name, sid));
        let rec = record.as_ref().map_or(String::new(), |r| r.to_string());
        scope = format!("{scope}:{}:{}", hash_hex(sid), hash_hex(&rec));
    }
    // declared inputs: part of the key, so two base URLs or models in one
    // directory do not share an entry
    let session = session_json(ctx.session, record);
    let env = info.inputs.env_values(&|n| std::env::var(n).ok());
    let declared = if info.inputs.is_empty() {
        String::new()
    } else {
        let picked: Map<String, Value> = info
            .inputs
            .session
            .iter()
            .map(|k| (k.clone(), session[k.as_str()].clone()))
            .collect();
        format!(":{}", json!({"env": env, "session": picked}))
    };
    let key = format!(
        "plugin:{}:{path}:{scope}:{options}:{config}:{toggles}{declared}",
        found.name
    );
    let mut input = json!({
        "options": options,
        "session": session,
        "columns": ctx.columns,
        "config": config,
        "toggles": toggles,
    });
    if !info.inputs.env.is_empty() {
        input["env"] = Value::Object(env);
    }
    Some(Plan {
        key,
        job: Job::PluginRender {
            path,
            input: input.to_string(),
            timeout: info.timeout,
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
    /// Shown instead of `text` while the placeholder's toggle is on.
    pub alt: Option<String>,
    pub label: Option<String>,
    /// Segment icon; replaces `label` when an icon is shown.
    pub icon: Option<String>,
    /// Name of the toggle a click on the label or icon flips
    /// (`bar toggle <plugin>.<name>`).
    pub label_toggle: Option<String>,
    /// Set by `toggled`: the link around the drawn label or icon.
    pub label_link: Option<String>,
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
        alt: v["alt"]
            .as_str()
            .filter(|a| !sanitize(a).trim().is_empty())
            .map(String::from),
        label: v["label"].as_str().map(String::from),
        icon: v["icon"].as_str().map(String::from),
        label_toggle: v["label_toggle"]
            .as_str()
            .filter(|k| click::valid_toggle_name(k))
            .map(String::from),
        label_link: None,
        style: v["style"].as_str().map(String::from),
        link: v["link"].as_str().map(String::from),
        metric: metric(&v["metric"]),
        score: v["score"].as_f64().filter(|f| !f.is_nan()),
    })
}

/// Sanitised, coloured, linked text, after a dim unlinked `label`; `None` when nothing visible is left.
/// `style` beats `score`, `score` beats `metric` + `rule <name>`.
pub fn paint(out: &Output, name: &str, config: &Config, now: i64) -> Option<String> {
    paint_with(out, name, config, now, &Pick::Auto, None)
}

/// As `paint`; the icon (explicit `pick`, else the output's, else the
/// plugin's default `info_icon` when an icon set is chosen) replaces `label`.
pub fn paint_with(
    out: &Output,
    name: &str,
    config: &Config,
    now: i64,
    pick: &Pick,
    info_icon: Option<&str>,
) -> Option<String> {
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
    // a bad output icon falls back to the plugin's own, then to the label
    let glyph = icons::resolve(config, pick, out.icon.as_deref()).or_else(|| {
        (*pick == Pick::Auto)
            .then(|| icons::resolve(config, pick, info_icon))
            .flatten()
    });
    if let Some(glyph) = glyph {
        return Some(format!("{} {body}", marked(&dim(&glyph), out)));
    }
    let label = out.label.as_deref().map(sanitize).unwrap_or_default();
    Some(if label.trim().is_empty() {
        body
    } else {
        format!("{} {body}", marked(&dim(&label), out))
    })
}

/// The drawn label or icon, inside its toggle link when `toggled` set one.
fn marked(mark: &str, out: &Output) -> String {
    match out.label_link.as_deref() {
        Some(url) => link(url, mark),
        None => mark.to_string(),
    }
}

/// Cached output for `found`; empty while cold, on failure or bad options.
pub fn render_found(ctx: &Ctx, found: &Found, opts: &Options) -> Option<String> {
    render_found_with(ctx, found, opts, &Pick::Auto)
}

/// As `render_found`, with the placeholder's `icon=` choice.
pub fn render_found_with(ctx: &Ctx, found: &Found, opts: &Options, pick: &Pick) -> Option<String> {
    let cache = ctx.cache?;
    let info = cached_info(cache, found)?;
    let plan = plan(ctx, found, &info, opts)?;
    let raw = cache.read_or_refresh(&plan.key, plan.ttl, &plan.job)?;
    let out = toggled(parse_output(&raw)?, &found.name, opts, cache.state_dir());
    paint_with(
        &out,
        &found.name,
        ctx.config,
        ctx.now,
        pick,
        info.icon.as_deref(),
    )
}

/// An output with `alt` can be toggled (`bar toggle <name>`): while on,
/// `alt` replaces `text`. The text links to `bar://toggle/<name>` unless
/// the output has its own `link`, `click=none`, or no handler is installed.
/// A `label_toggle` links the drawn label or icon to
/// `bar://toggle/<name>.<toggle>` under the same conditions.
fn toggled(mut out: Output, name: &str, opts: &Options, state: &Path) -> Output {
    let linkable = opts.get("click") != Some("none") && click::handler_installed(state);
    if let Some(key) = out.label_toggle.as_deref().filter(|_| linkable) {
        out.label_link = Some(format!("bar://toggle/{name}.{key}"));
    }
    let Some(alt) = out.alt.take() else {
        return out;
    };
    if click::is_toggled(state, name) {
        out.text = alt;
    }
    if out.link.as_deref().is_none_or(str::is_empty) && linkable {
        out.link = Some(format!("bar://toggle/{name}"));
    }
    out
}

/// `#{name}` for a plugin found in `ctx.plugin_dirs`.
pub fn render(ctx: &Ctx, name: &str, opts: &Options) -> Option<String> {
    render_found(ctx, &find(ctx.plugin_dirs, name)?, opts)
}
