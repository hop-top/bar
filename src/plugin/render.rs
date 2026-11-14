//! Render input, output parsing and colouring.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::info::ttl_of;
use super::{cached_info, find, validate, ExtInfo, Found, RESERVED};
use crate::cache::{hash_hex, Job};
use crate::click::{self, hook::read_record};
use crate::color::Color;
use crate::conf::Config;
use crate::icons::{self, Pick};
use crate::options::Options;
use crate::pace::metric_score;
use crate::placeholders::Ctx;
use crate::session::{Session, Window};
use crate::style::{dim, fg, link, ramp, sanitize, ITEM_BREAK};

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

/// The user's `account "<glob>" <label> <color>` lines as plugins get
/// them (`inputs.accounts`): `[{"glob","label","color"}]`, in order, the
/// colour as written in a config. The globs can hold email addresses:
/// they travel in a plugin's stdin or environment only.
pub fn account_rules(config: &Config) -> Value {
    Value::Array(
        config
            .accounts
            .iter()
            .map(|r| json!({"glob": r.glob, "label": r.label, "color": r.color.to_string()}))
            .collect(),
    )
}

/// The user's account rules for a process that has no render context
/// (the refresh job, a click, `bar <name>`): the user config, with the
/// project one over it when `cwd` names a directory.
pub fn load_account_rules(cwd: Option<&Path>) -> Value {
    match crate::conf::load(cwd) {
        crate::conf::LoadResult::Ok(c) => account_rules(&c),
        _ => Value::Array(Vec::new()),
    }
}

/// `input` with `accounts` set when `rules` has any.
pub fn with_accounts(input: &str, rules: Value) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(input) else {
        return input.to_string();
    };
    if rules.as_array().is_some_and(|a| !a.is_empty()) {
        v["accounts"] = rules;
    }
    v.to_string()
}

/// The session's account for `inputs.account`: what travels in the input
/// (the label, when a rule matched) and the email, which the job adds
/// itself. `None` when bar knows no account.
fn account_input(ctx: &Ctx) -> Option<(Value, Option<String>)> {
    let a = crate::placeholders::account::resolve(ctx)?;
    let v = a.label.map_or_else(|| json!({}), |l| json!({"label": l}));
    (a.email.is_some() || !v.as_object().is_some_and(Map::is_empty)).then_some((v, a.email))
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
    // named label/icon toggles as this session sees them: part of the key,
    // so a flip is a new entry, and sessions that see the same ones share it
    let toggles = ctx.cache.map_or_else(Default::default, |c| {
        click::plugin_toggles(c.state_dir(), &found.name, &ctx.session.session_id)
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
    // the rules take part in the key as a hash, never in clear: a changed
    // `account` line is a new entry
    let rules = (info.inputs.accounts && !ctx.config.accounts.is_empty())
        .then(|| hash_hex(&account_rules(ctx.config).to_string()));
    let declared = match &rules {
        Some(h) => format!("{declared}:accounts={h}"),
        None => declared,
    };
    let account = info.inputs.account.then(|| account_input(ctx)).flatten();
    let declared = match &account {
        Some((v, email)) => format!(
            "{declared}:account={}",
            hash_hex(&json!([v, email]).to_string())
        ),
        None => declared,
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
    if let Some((v, _)) = &account {
        input["account"] = v.clone();
    }
    Some(Plan {
        key,
        job: Job::PluginRender {
            path,
            input: input.to_string(),
            timeout: info.timeout,
            accounts: rules.is_some(),
            email: account.and_then(|(_, e)| e.map(|e| hash_hex(&e))),
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

/// Most items one output can carry.
pub const MAX_ITEMS: usize = 16;

/// Usage windows an output carries, shaped like the status line's
/// `rate_limits`: percent used 0..=100, reset time in Unix seconds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

/// The account an output names, for `set account-source`.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// Stable identifier.
    pub id: String,
    /// What `#{account}` shows; sanitised.
    pub label: String,
    /// Colour of `label`, set when the plugin matched the user's `account`
    /// rules itself: the label is then drawn as is, in this colour.
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Output {
    /// `tokens`: values an action takes (`click::action`).
    pub tokens: BTreeMap<String, String>,
    /// The cache entry this output was read from (set by the readers, not
    /// parsed); empty for a parsed string.
    pub entry: String,
    /// `account`, for `set account-source`.
    pub account: Option<Account>,
    /// `usage` windows, for `set usage-source`.
    pub usage: Option<Usage>,
    /// With `items`, the output's own `text` is not drawn.
    pub text: String,
    /// `items`: each drawn like an output of its own, joined by a space,
    /// with a wrap point between them. Items carry no items.
    pub items: Vec<Output>,
    /// Shown instead of `text` while the placeholder's toggle is on.
    pub alt: Option<String>,
    pub label: Option<String>,
    /// Colour of `label` (`label_color`), as an `account` line takes one.
    pub label_color: Option<Color>,
    /// Segment icon; replaces `label` when an icon is shown.
    pub icon: Option<String>,
    /// Name of the toggle a click on the label or icon flips
    /// (`bar toggle <plugin>.<name>`).
    pub label_toggle: Option<String>,
    /// Name of the toggle a click on the text flips instead of the `alt`
    /// toggle, which then moves to the label or icon.
    pub text_toggle: Option<String>,
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

fn window(v: &Value) -> Option<Window> {
    let used = v["used_percentage"].as_f64().filter(|u| u.is_finite())?;
    Some(Window {
        used_percentage: used.clamp(0.0, 100.0),
        resets_at: int(&v["resets_at"])?,
    })
}

/// `None` unless at least one window is valid.
fn usage(v: &Value) -> Option<Usage> {
    let u = Usage {
        five_hour: window(&v["five_hour"]),
        seven_day: window(&v["seven_day"]),
    };
    (u.five_hour.is_some() || u.seven_day.is_some()).then_some(u)
}

/// Valid tokens only: names `[a-z0-9_-]`, values `[A-Za-z0-9._@-]` of at
/// most 64 characters, at most 8.
pub(super) fn tokens(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| click::action::valid_token_name(k))
                .filter_map(|(k, x)| Some((k.clone(), x.as_str()?.to_string())))
                .filter(|(_, x)| click::action::valid_token_value(x))
                .take(super::MAX_TOKENS)
                .collect()
        })
        .unwrap_or_default()
}

impl Output {
    /// Whether `want` is among this output's token sets: the top-level
    /// one, or an item's (which falls back to the top-level tokens for
    /// those it lacks).
    pub fn offers_tokens(&self, want: &BTreeMap<String, String>) -> bool {
        let covers =
            |have: &BTreeMap<String, String>| want.iter().all(|(k, v)| have.get(k) == Some(v));
        covers(&self.tokens)
            || self.items.iter().any(|i| {
                let mut all = self.tokens.clone();
                all.extend(i.tokens.clone());
                covers(&all)
            })
    }
}

/// `None` unless `id` and `label` are strings and `label` shows something.
fn account(v: &Value) -> Option<Account> {
    let id = v["id"].as_str().map(str::trim).filter(|i| !i.is_empty())?;
    let label = sanitize(v["label"].as_str()?).trim().to_string();
    (!label.is_empty()).then(|| Account {
        id: id.chars().take(128).collect(),
        label,
        color: v["color"].as_str().and_then(Color::parse),
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

/// `None` unless a JSON object with string `text` or an `items` array;
/// optional fields of the wrong type are ignored, and an item without
/// string `text` is left out.
pub fn parse_output(raw: &str) -> Option<Output> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let items = v["items"].as_array();
    let mut out = match v.get("text").and_then(Value::as_str) {
        Some(_) => parse_one(&v)?,
        None if items.is_some() => parse_one(&{
            let mut w = v.clone();
            w["text"] = Value::from("");
            w
        })?,
        None => return None,
    };
    out.items = items
        .into_iter()
        .flatten()
        .filter_map(parse_one)
        .take(MAX_ITEMS)
        .collect();
    Some(out)
}

fn parse_one(v: &Value) -> Option<Output> {
    let v = v.as_object().map(|_| v)?;
    Some(Output {
        text: v.get("text")?.as_str()?.to_string(),
        items: Vec::new(),
        usage: usage(&v["usage"]),
        account: account(&v["account"]),
        tokens: tokens(&v["tokens"]),
        entry: String::new(),
        alt: v["alt"]
            .as_str()
            .filter(|a| !sanitize(a).trim().is_empty())
            .map(String::from),
        label: v["label"].as_str().map(String::from),
        label_color: v["label_color"].as_str().and_then(Color::parse),
        icon: v["icon"].as_str().map(String::from),
        label_toggle: v["label_toggle"]
            .as_str()
            .filter(|k| click::valid_toggle_name(k))
            .map(String::from),
        text_toggle: v["text_toggle"]
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
/// An output with `items` draws each item this way, joined by
/// [`ITEM_BREAK`] and a space; `None` when none shows.
pub fn paint_with(
    out: &Output,
    name: &str,
    config: &Config,
    now: i64,
    pick: &Pick,
    info_icon: Option<&str>,
) -> Option<String> {
    if out.items.is_empty() {
        return paint_one(out, name, config, now, pick, info_icon);
    }
    let drawn: Vec<String> = out
        .items
        .iter()
        .filter_map(|i| paint_one(i, name, config, now, pick, info_icon))
        .collect();
    (!drawn.is_empty()).then(|| drawn.join(&format!("{ITEM_BREAK} ")))
}

fn paint_one(
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
        let shown = match out.label_color {
            Some(c) => fg(c, &label),
            None => dim(&label),
        };
        format!("{} {body}", marked(&shown, out))
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
    let mut out = parse_output(&raw)?;
    let state = cache.state_dir();
    let sid = ctx.session.session_id.as_str();
    if opts.get("click") != Some("none") {
        link_actions(
            &mut out,
            ctx,
            &found.name,
            &info,
            &hash_hex(&plan.key),
            state,
        );
    }
    out.items = std::mem::take(&mut out.items)
        .into_iter()
        .map(|i| toggled(i, &found.name, sid, opts, state))
        .collect();
    let out = toggled(out, &found.name, sid, opts, state);
    paint_with(
        &out,
        &found.name,
        ctx.config,
        ctx.now,
        pick,
        info.icon.as_deref(),
    )
}

/// The tokens `spec` takes, from `tokens` (an output's own over the
/// top-level ones); `None` when one is missing.
fn action_tokens(
    spec: &super::ActionSpec,
    top: &BTreeMap<String, String>,
    own: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    spec.tokens
        .iter()
        .map(|n| Some((n.clone(), own.get(n).or_else(|| top.get(n))?.clone())))
        .collect()
}

/// Gives the output, or each of its items, the link of the action the
/// user bound to the placeholder (`action <name> <name>.<action>`), when
/// the handler routes actions and the output carries every token the
/// action takes. An output with a `link` of its own keeps it. An action
/// that fronts a toggle (`toggle` in its declaration) links the label or
/// icon instead, of an output whose `label_toggle` names that toggle.
fn link_actions(
    out: &mut Output,
    ctx: &Ctx,
    name: &str,
    info: &ExtInfo,
    entry: &str,
    state: &Path,
) {
    let bound = ctx
        .config
        .actions
        .iter()
        .find(|b| b.placeholder == name && b.plugin == name);
    let Some((b, spec)) = bound.and_then(|b| Some((b, info.actions.get(&b.action)?))) else {
        return;
    };
    let top = out.tokens.clone();
    let one = |o: &mut Output| {
        let fronts = spec.toggle.as_deref();
        if fronts.is_some_and(|t| o.label_toggle.as_deref() != Some(t))
            || (fronts.is_none() && o.link.as_deref().is_some_and(|l| !l.is_empty()))
        {
            return;
        }
        let Some(t) = action_tokens(spec, &top, &o.tokens) else {
            return;
        };
        let url = click::action::build(state, &b.plugin, &b.action, &t, entry);
        if fronts.is_some() {
            o.label_link = url;
        } else {
            o.link = url;
        }
    };
    if out.items.is_empty() {
        one(out);
    } else {
        out.items.iter_mut().for_each(one);
    }
}

/// `value` of `#{account}` inside the link of the action bound to it
/// (`action account <plugin>.<action>`), when the plugin `set
/// account-source` names carries the tokens the action takes and, for an
/// action that takes an `account` token, supplied the account shown.
pub fn link_account(ctx: &Ctx, value: String) -> String {
    let linked = || -> Option<String> {
        let b = ctx
            .config
            .actions
            .iter()
            .find(|b| b.placeholder == "account")?;
        let src = ctx
            .config
            .account_source
            .as_deref()
            .filter(|s| *s == b.plugin)?;
        let cache = ctx.cache?;
        let out = default_output(ctx, src)?;
        let found = find(ctx.plugin_dirs, src)?;
        let spec = cached_info(cache, &found)?.actions.get(&b.action)?.clone();
        // an action aimed at the account shown needs the plugin to have
        // named it; one that is not (switch to another account, say) is
        // linked even while the shown account is bar's own
        if out.account.is_none() && spec.tokens.iter().any(|t| t == "account") {
            return None;
        }
        let t = action_tokens(&spec, &out.tokens, &out.tokens)?;
        let url = click::action::build(cache.state_dir(), &b.plugin, &b.action, &t, &out.entry)?;
        Some(link(&url, &value))
    };
    linked().unwrap_or(value)
}

/// An output with `alt` can be toggled (`bar toggle <name>`): while on in
/// session `sid`, `alt` replaces `text`. The text links to
/// `bar://toggle/<name>?sid=<sid>` unless the output has its own `link`,
/// `click=none`, or no handler is installed. A `label_toggle` links the
/// drawn label or icon to `bar://toggle/<name>.<toggle>?sid=<sid>` under
/// the same conditions. A `text_toggle` links the text to
/// `bar://toggle/<name>.<toggle>` instead, and the `alt` toggle to the
/// label or icon unless a `label_toggle` holds it.
fn toggled(mut out: Output, name: &str, sid: &str, opts: &Options, state: &Path) -> Output {
    let linkable = opts.get("click") != Some("none") && click::handler_installed(state);
    let named = |key: &str| click::toggle_link(&format!("{name}.{key}"), sid);
    // an action fronting the toggle (`link_actions`) keeps the label
    if let Some(key) = out.label_toggle.as_deref().filter(|_| linkable) {
        out.label_link.get_or_insert_with(|| named(key));
    }
    let own_link = out.link.as_deref().is_some_and(|l| !l.is_empty());
    let text_toggle = out.text_toggle.take();
    if let Some(key) = text_toggle.as_deref().filter(|_| linkable && !own_link) {
        out.link = Some(named(key));
    }
    let Some(alt) = out.alt.take() else {
        return out;
    };
    if click::is_toggled_in(state, name, sid) {
        out.text = alt;
    }
    if !linkable {
        return out;
    }
    if text_toggle.is_none() {
        if !own_link {
            out.link = Some(click::toggle_link(name, sid));
        }
    } else if out.label_link.is_none() {
        out.label_link = Some(click::toggle_link(name, sid));
    }
    out
}

/// [`cached_output`], read once per render however many placeholders ask.
pub fn default_output(ctx: &Ctx, name: &str) -> Option<std::rc::Rc<Output>> {
    if let Some(hit) = ctx.memo.outputs.borrow().get(name) {
        return hit.clone();
    }
    let out = cached_output(ctx, name).map(std::rc::Rc::new);
    ctx.memo
        .outputs
        .borrow_mut()
        .insert(name.to_string(), out.clone());
    out
}

/// Seconds after which a plugin's `usage` and `account` are no longer
/// taken as current: the larger of this and five times its `ttl`.
pub const USAGE_ACCOUNT_MAX_AGE: u64 = 900;

/// The cached output of `name` rendered with its `default <name>` options
/// only, as `#{name}` bare would be: a plain cache read, which starts a
/// background refresh when stale and never waits. `None` while cold, when
/// the plugin is missing, not a placeholder, or its options are invalid.
pub fn cached_output(ctx: &Ctx, name: &str) -> Option<Output> {
    let cache = ctx.cache?;
    let found = find(ctx.plugin_dirs, name)?;
    let info = cached_info(cache, &found)?;
    let mut opts = Options::default();
    if let Some(d) = ctx.config.defaults.get(name) {
        let ex = crate::expand::Expander::new(ctx.runner);
        for (k, v) in d.iter().filter(|(k, _)| !RESERVED.contains(&k.as_str())) {
            opts.set(k, &ex.expand(v));
        }
    }
    let plan = plan(ctx, &found, &info, &opts)?;
    let mut out = parse_output(&cache.read_or_refresh(&plan.key, plan.ttl, &plan.job)?)?;
    out.entry = hash_hex(&plan.key);
    // a plugin that stopped refreshing (its backend is down) must not keep
    // feeding `usage` and `account` as if they were live
    let bound = Duration::from_secs((plan.ttl * 5).max(USAGE_ACCOUNT_MAX_AGE));
    if crate::cache::modified_age(&cache.path_for(&plan.key)).is_some_and(|a| a > bound) {
        out.usage = None;
        out.account = None;
    }
    Some(out)
}

/// `#{name}` for a plugin found in `ctx.plugin_dirs`.
pub fn render(ctx: &Ctx, name: &str, opts: &Options) -> Option<String> {
    render_found(ctx, &find(ctx.plugin_dirs, name)?, opts)
}
