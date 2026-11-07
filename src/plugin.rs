//! External placeholders: `bar-<name>` executables on `PATH`, interrogated
//! with `--ext-info` (kit ext-discover, extended) and rendered with JSON on
//! stdin. Both runs go through the cache; render never waits on a plugin.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::cache::{run_child, Cache, Job};
use crate::conf::{duration, Config};
use crate::options::Options;
use crate::pace::metric_score;
use crate::placeholders::Ctx;
use crate::session::{Session, Window};
use crate::style::{dim, fg, link, ramp, sanitize};

pub const PREFIX: &str = "bar-";
/// Core placeholder names; a plugin by the same name is never used.
pub const CORE: &[&str] = &[
    "account", "repo", "branch", "model", "effort", "ctx", "usage", "prs", "issues",
];
/// Capability a plugin must declare to be rendered.
pub const CAPABILITY: &str = "placeholder";
pub const INFO_TIMEOUT: Duration = Duration::from_secs(5);
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);
/// Render results stay fresh this long unless the plugin says otherwise.
pub const DEFAULT_TTL: u64 = 60;
/// `--ext-info` results stay fresh a day; the cache key carries the
/// binary's mtime, so an upgrade asks again at once.
pub const INFO_TTL: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Placeholder name: the file name without the prefix.
    pub name: String,
    pub path: PathBuf,
}

/// Absolute entries of a `PATH`-style list, in order; relative entries
/// would make the plugin set depend on the working directory.
pub fn path_dirs(path: Option<&OsStr>) -> Vec<PathBuf> {
    path.map(|p| {
        std::env::split_paths(p)
            .filter(|d| d.is_absolute())
            .collect()
    })
    .unwrap_or_default()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !CORE.contains(&name)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Every plugin in `dirs`, sorted by name; per name the first executable
/// in `dirs` order wins. Core names and non-executables are skipped.
pub fn discover(dirs: &[PathBuf]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let file = e.file_name();
            let Some(name) = file.to_str().and_then(|f| f.strip_prefix(PREFIX)) else {
                continue;
            };
            let path = dir.join(&file);
            if valid_name(name)
                && path.to_str().is_some()
                && !out.iter().any(|f| f.name == name)
                && is_executable(&path)
            {
                out.push(Found {
                    name: name.to_string(),
                    path,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The plugin `discover` would pick for `name`, with one stat per dir.
pub fn find(dirs: &[PathBuf], name: &str) -> Option<Found> {
    if !valid_name(name) {
        return None;
    }
    dirs.iter()
        .map(|d| d.join(format!("{PREFIX}{name}")))
        .find(|p| p.to_str().is_some() && is_executable(p))
        .map(|path| Found {
            name: name.to_string(),
            path,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptType {
    Int,
    String,
    Bool,
    Enum,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptSpec {
    pub kind: OptType,
    /// Allowed values for `Enum`.
    pub choices: Vec<String>,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub options: BTreeMap<String, OptSpec>,
    /// Seconds a render result stays fresh.
    pub ttl: u64,
}

impl ExtInfo {
    /// What the host assumes when `--ext-info` fails: name from the file,
    /// empty version, no capabilities.
    pub fn fallback(found: &Found) -> ExtInfo {
        ExtInfo {
            name: found.name.clone(),
            version: String::new(),
            description: String::new(),
            capabilities: Vec::new(),
            options: BTreeMap::new(),
            ttl: DEFAULT_TTL,
        }
    }

    pub fn is_placeholder(&self) -> bool {
        self.capabilities.iter().any(|c| c == CAPABILITY)
    }

    pub fn to_json(&self) -> Value {
        let options: Map<String, Value> = self
            .options
            .iter()
            .map(|(k, s)| {
                let kind = match s.kind {
                    OptType::Int => "int",
                    OptType::String => "string",
                    OptType::Bool => "bool",
                    OptType::Enum => "enum",
                };
                let mut o = json!({ "type": kind });
                if s.kind == OptType::Enum {
                    o["enum"] = json!(s.choices);
                }
                if let Some(d) = &s.default {
                    o["default"] = d.clone();
                }
                (k.clone(), o)
            })
            .collect();
        json!({
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "capabilities": self.capabilities,
            "options": options,
            "ttl": self.ttl,
        })
    }
}

fn clean_str(v: &Value) -> Option<String> {
    v.as_str().map(sanitize)
}

/// `"1"`, `"true"`, `"yes"`, `"on"` and their opposites.
fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// An option value as the plugin receives it: number, bool or string.
fn typed(spec: &OptSpec, raw: &str) -> Option<Value> {
    match spec.kind {
        OptType::Int => raw.trim().parse::<i64>().ok().map(Value::from),
        OptType::Bool => parse_bool(raw).map(Value::from),
        OptType::Enum => spec
            .choices
            .iter()
            .any(|c| c == raw)
            .then(|| Value::from(raw)),
        OptType::String => Some(Value::from(raw)),
    }
}

fn default_fits(spec: &OptSpec, v: &Value) -> bool {
    match spec.kind {
        OptType::Int => v.is_i64(),
        OptType::Bool => v.is_boolean(),
        OptType::String => v.is_string(),
        OptType::Enum => v
            .as_str()
            .is_some_and(|s| spec.choices.iter().any(|c| c == s)),
    }
}

/// One `options` entry; unknown types drop the option, so using it fails
/// validation, and a default of the wrong type is ignored.
fn opt_spec(v: &Value) -> Option<OptSpec> {
    let kind = match v["type"].as_str()? {
        "int" => OptType::Int,
        "string" => OptType::String,
        "bool" => OptType::Bool,
        "enum" => OptType::Enum,
        _ => return None,
    };
    let choices = v["enum"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut spec = OptSpec {
        kind,
        choices,
        default: None,
    };
    spec.default = v.get("default").filter(|d| default_fits(&spec, d)).cloned();
    Some(spec)
}

/// Seconds from a number or a duration string (`90s`, `5m`); else default.
fn ttl_of(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| duration(s.trim()).ok()))
        .filter(|&t| t > 0)
        .unwrap_or(DEFAULT_TTL)
}

/// `None` unless a JSON object with string `name` and `version`.
pub fn parse_info(raw: &str) -> Option<ExtInfo> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let options = v["options"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, s)| Some((k.clone(), opt_spec(s)?)))
                .collect()
        })
        .unwrap_or_default();
    Some(ExtInfo {
        name: clean_str(&v["name"])?,
        version: clean_str(&v["version"])?,
        description: clean_str(&v["description"]).unwrap_or_default(),
        capabilities: v["capabilities"]
            .as_array()
            .map(|a| a.iter().filter_map(clean_str).collect())
            .unwrap_or_default(),
        options,
        ttl: ttl_of(&v["ttl"]),
    })
}

/// Runs `--ext-info` now (5 s limit), falling back to synthesized info.
pub fn info(found: &Found) -> ExtInfo {
    let mut c = Command::new(&found.path);
    c.arg("--ext-info");
    run_child(c, None, INFO_TIMEOUT)
        .and_then(|o| parse_info(&o))
        .unwrap_or_else(|| ExtInfo::fallback(found))
}

/// Cache key and job for a plugin's interrogation.
pub fn info_job(found: &Found) -> (String, Job) {
    let stamp = std::fs::metadata(&found.path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let path = found.path.to_string_lossy().into_owned();
    (
        format!("plugin-info:{path}:{stamp}"),
        Job::PluginInfo { path },
    )
}

/// Cached interrogation; `None` while cold (a refresh is then started).
pub fn cached_info(cache: &Cache, found: &Found) -> Option<ExtInfo> {
    let (key, job) = info_job(found);
    parse_info(&cache.read_or_refresh(&key, INFO_TTL, &job)?)
}

/// Typed option values: every key must be in the schema and parse as its
/// type; schema defaults fill the missing ones.
pub fn validate(
    schema: &BTreeMap<String, OptSpec>,
    opts: &Options,
) -> Result<Map<String, Value>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in opts.iter() {
        let spec = schema
            .get(k)
            .ok_or_else(|| format!("unknown option `{k}`"))?;
        let val = typed(spec, v).ok_or_else(|| format!("invalid value `{v}` for `{k}`"))?;
        out.insert(k.clone(), val);
    }
    for (k, spec) in schema {
        if let (false, Some(d)) = (out.contains_key(k), &spec.default) {
            out.insert(k.clone(), d.clone());
        }
    }
    // sorted, so the cache key does not depend on option order
    Ok(out.into_iter().collect())
}

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
    // session data is left out of the key so the cache can hit
    let key = format!("plugin:{}:{path}:{options}:{config}", found.name);
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
        style: v["style"].as_str().map(String::from),
        link: v["link"].as_str().map(String::from),
        metric: metric(&v["metric"]),
        score: v["score"].as_f64().filter(|f| !f.is_nan()),
    })
}

/// Sanitised, coloured, linked text; `None` when nothing visible is left.
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
        let secs = Some(m.expires_at - now).filter(|&s| s > 0);
        let period = m.granted_at.map_or(0, |g| m.expires_at - g);
        let s = metric_score(m.remaining, m.granted, secs, period, rule);
        fg(ramp(s, &config.ramp), &text)
    } else {
        text
    };
    Some(match out.link.as_deref().filter(|l| !l.is_empty()) {
        Some(url) => link(url, &body),
        None => body,
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
