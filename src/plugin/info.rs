//! `--ext-info` interrogation, its cache, and the options schema.

use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

use serde_json::{json, Map, Value};

use super::{Found, CAPABILITY, DEFAULT_TTL, INFO_TIMEOUT, INFO_TTL};
use crate::cache::{Cache, Job};
use crate::conf::duration;
use crate::options::Options;
use crate::style::sanitize;

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
    /// Default segment icon, used when an icon set is chosen.
    pub icon: Option<String>,
    /// The plugin declared an `icon` option; it is ignored, the name is
    /// reserved for the generic placeholder option.
    pub declares_icon_option: bool,
    /// As `declares_icon_option`, for the reserved `bg` option.
    pub declares_bg_option: bool,
    /// The reserved `fg`, `bold` or `gradient` option the plugin declared
    /// first, if any.
    pub declares_look_option: Option<&'static str>,
    /// As `declares_icon_option`, for the reserved `click` option.
    pub declares_click_option: bool,
    /// `"cache": "session"`: output depends on the session, so results are
    /// cached per session id and the session's record is sent.
    pub session_cache: bool,
    /// The optional `web` object as given: what the web configurator shows
    /// for the plugin (`plugin::web`).
    pub web: Option<Value>,
}

impl ExtInfo {
    /// What the host assumes when `--ext-info` fails: name from the file,
    /// empty version, no capabilities.
    pub fn fallback(found: &Found) -> ExtInfo {
        ExtInfo::named(&found.name)
    }

    /// Info with only a name: no version, capabilities or options.
    pub fn named(name: &str) -> ExtInfo {
        ExtInfo {
            name: name.to_string(),
            version: String::new(),
            description: String::new(),
            capabilities: Vec::new(),
            options: BTreeMap::new(),
            ttl: DEFAULT_TTL,
            icon: None,
            declares_icon_option: false,
            declares_bg_option: false,
            declares_look_option: None,
            declares_click_option: false,
            session_cache: false,
            web: None,
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
        let mut out = json!({
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "capabilities": self.capabilities,
            "options": options,
            "ttl": self.ttl,
        });
        if let Some(icon) = &self.icon {
            out["icon"] = json!(icon);
        }
        if self.session_cache {
            out["cache"] = json!("session");
        }
        if let Some(web) = &self.web {
            out["web"] = web.clone();
        }
        out
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
pub(super) fn typed(spec: &OptSpec, raw: &str) -> Option<Value> {
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
        .map(|a| a.iter().filter_map(clean_str).collect())
        .unwrap_or_default();
    let mut spec = OptSpec {
        kind,
        choices,
        default: None,
    };
    spec.default = v
        .get("default")
        .map(|d| clean_str(d).map_or_else(|| d.clone(), Value::from))
        .filter(|d| default_fits(&spec, d));
    Some(spec)
}

/// Seconds from a number or a duration string (`90s`, `5m`); else default.
pub(super) fn ttl_of(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| duration(s.trim()).ok()))
        .filter(|&t| t > 0)
        .unwrap_or(DEFAULT_TTL)
}

/// Placeholder options bar handles itself; never sent to a plugin.
pub const RESERVED: [&str; 6] = ["icon", "bg", "fg", "bold", "gradient", "click"];

/// `None` unless a JSON object with string `name` and `version`.
pub fn parse_info(raw: &str) -> Option<ExtInfo> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let options = v["options"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, s)| Some((sanitize(k), opt_spec(s)?)))
                .filter(|(k, _)| !RESERVED.contains(&k.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let declares_icon_option = v["options"].get("icon").is_some();
    let declares_bg_option = v["options"].get("bg").is_some();
    let declares_look_option = ["fg", "bold", "gradient"]
        .into_iter()
        .find(|k| v["options"].get(k).is_some());
    let declares_click_option = v["options"].get("click").is_some();
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
        icon: clean_str(&v["icon"]).filter(|i| !i.trim().is_empty()),
        declares_icon_option,
        declares_bg_option,
        declares_look_option,
        declares_click_option,
        session_cache: v["cache"] == "session",
        web: v.get("web").filter(|w| !w.is_null()).cloned(),
    })
}

/// Runs `--ext-info` now (5 s limit), falling back to synthesized info.
pub fn info(found: &Found) -> ExtInfo {
    info_job(found)
        .1
        .execute(INFO_TIMEOUT)
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

/// The cached interrogation when fresh, else `--ext-info` run now (5 s
/// limit) and its output stored for the next caller; synthesized info
/// when that fails. Never spawns a background job.
pub fn info_now(cache: &Cache, found: &Found) -> ExtInfo {
    let (key, job) = info_job(found);
    if let Some(i) = cache
        .read_fresh(&key, INFO_TTL)
        .and_then(|o| parse_info(&o))
    {
        return i;
    }
    match job.execute(INFO_TIMEOUT) {
        Some(raw) => match parse_info(&raw) {
            Some(i) => {
                let _ = cache.store(&key, &raw);
                i
            }
            None => ExtInfo::fallback(found),
        },
        None => ExtInfo::fallback(found),
    }
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
