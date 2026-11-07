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

/// `None` unless a JSON object with string `name` and `version`.
pub fn parse_info(raw: &str) -> Option<ExtInfo> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let options = v["options"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, s)| Some((sanitize(k), opt_spec(s)?)))
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
