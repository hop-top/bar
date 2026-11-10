//! `--ext-info` interrogation, its cache, and the options schema.

use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

use serde_json::{json, Map, Value};

use super::{
    Found, CAPABILITY, DEFAULT_TTL, INFO_TIMEOUT, INFO_TTL, MAX_RENDER_TIMEOUT, RENDER_TIMEOUT,
};
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

/// An action a plugin declares: something a click can run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionSpec {
    pub description: String,
    /// Token names the action takes, sorted; the plugin's output supplies
    /// their values.
    pub tokens: Vec<String>,
    /// Asks for confirmation first. Declared and kept; bar's handler runs
    /// without a prompt, so a plugin that needs one asks itself.
    pub confirm: bool,
    /// Seconds the action may run, for one that waits on the user (a
    /// chooser); 0 is the default limit. Capped at
    /// [`crate::click::action::MAX_ACTION_TIMEOUT`] seconds.
    pub timeout: u64,
}

/// Most tokens an action can take.
pub const MAX_TOKENS: usize = 8;

fn aliases_of(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
        if super::valid_alias(a) && !out.iter().any(|o| o == a) && out.len() < super::MAX_ALIASES {
            out.push(a.to_string());
        }
    }
    out
}

fn actions_of(v: &Value) -> BTreeMap<String, ActionSpec> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| crate::click::valid_toggle_name(k))
                .map(|(k, a)| {
                    let mut tokens: Vec<String> = a["tokens"]
                        .as_array()
                        .map(|t| {
                            t.iter()
                                .filter_map(Value::as_str)
                                .filter(|n| crate::click::action::valid_token_name(n))
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default();
                    tokens.sort();
                    tokens.dedup();
                    tokens.truncate(MAX_TOKENS);
                    let spec = ActionSpec {
                        description: clean_str(&a["description"]).unwrap_or_default(),
                        tokens,
                        confirm: a["confirm"].as_bool().unwrap_or(false),
                        timeout: a["timeout"].as_u64().map_or(0, |t| {
                            t.min(crate::click::action::MAX_ACTION_TIMEOUT.as_secs())
                        }),
                    };
                    (k.clone(), spec)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Most environment variables a plugin can declare.
pub const MAX_ENV_INPUTS: usize = 8;
/// Longest environment value passed on, in bytes; a longer one counts as unset.
pub const MAX_ENV_VALUE: usize = 1024;
/// Session fields a plugin can declare as inputs: the stable ones. The
/// volatile ones (`context_used`, the rate-limit windows) would split the
/// cache on every render.
pub const SESSION_INPUTS: [&str; 5] = ["cwd", "model", "model_id", "effort", "session_id"];

/// What a plugin's output depends on besides its options (`inputs` in
/// `--ext-info`): each declared value is part of the cache key and is
/// sent in the render input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Environment variable names, `[A-Z0-9_]+`, sorted, at most 8.
    pub env: Vec<String>,
    /// Names from [`SESSION_INPUTS`], sorted.
    pub session: Vec<String>,
    /// Declared environment names bar refuses: `env_secret` ones.
    pub refused: Vec<String>,
}

/// Names that hold credentials: their values would land in the key, the
/// refresh arguments and the cache, so they cannot be declared.
pub fn env_secret(name: &str) -> bool {
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
        "PASS",
        "PRIVATE",
        "COOKIE",
        "BEARER",
        "JWT",
    ]
    .iter()
    .any(|w| name.contains(w))
}

fn env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

impl Inputs {
    fn parse(v: &Value) -> Inputs {
        let list = |k: &str| -> Vec<String> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut env: Vec<String> = list("env").into_iter().filter(|n| env_name(n)).collect();
        env.sort();
        env.dedup();
        let refused = env.iter().filter(|n| env_secret(n)).cloned().collect();
        env.retain(|n| !env_secret(n));
        env.truncate(MAX_ENV_INPUTS);
        let mut session: Vec<String> = list("session")
            .into_iter()
            .filter(|n| SESSION_INPUTS.contains(&n.as_str()))
            .collect();
        session.sort();
        session.dedup();
        Inputs {
            env,
            session,
            refused,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.session.is_empty()
    }

    fn to_json(&self) -> Option<Value> {
        (!self.is_empty()).then(|| json!({"env": self.env, "session": self.session}))
    }

    /// Values of the declared environment variables: only those set to
    /// valid UTF-8 within [`MAX_ENV_VALUE`] bytes.
    pub fn env_values(&self, get: &dyn Fn(&str) -> Option<String>) -> Map<String, Value> {
        self.env
            .iter()
            .filter_map(|n| {
                let v = get(n).filter(|v| v.len() <= MAX_ENV_VALUE)?;
                Some((n.clone(), Value::from(v)))
            })
            .collect()
    }
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
    /// Declared `actions`, by name.
    pub actions: BTreeMap<String, ActionSpec>,
    /// Other names `bar <name>` answers to (up to 4, `[a-z0-9-]`).
    pub aliases: Vec<String>,
    /// Declared `inputs`: part of the cache key, sent in the render input.
    pub inputs: Inputs,
    /// Seconds a background render may run: `timeout` clamped to 1..=15,
    /// else 3.
    pub timeout: u64,
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
            timeout: RENDER_TIMEOUT.as_secs(),
            inputs: Inputs::default(),
            actions: BTreeMap::new(),
            aliases: Vec::new(),
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
        if !self.actions.is_empty() {
            let a: Map<String, Value> = self
                .actions
                .iter()
                .map(|(k, s)| {
                    let mut v = json!({"description": s.description, "tokens": s.tokens, "confirm": s.confirm});
                    if s.timeout > 0 {
                        v["timeout"] = json!(s.timeout);
                    }
                    (k.clone(), v)
                })
                .collect();
            out["actions"] = Value::Object(a);
        }
        if !self.aliases.is_empty() {
            out["aliases"] = json!(self.aliases);
        }
        if let Some(i) = self.inputs.to_json() {
            out["inputs"] = i;
        }
        if self.timeout != RENDER_TIMEOUT.as_secs() {
            out["timeout"] = json!(format!("{}s", self.timeout));
        }
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

/// Render limit in seconds from `timeout` (a number or a duration string),
/// clamped to 1..=15; 3 when absent or unparsable.
pub(super) fn timeout_of(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| duration(s.trim()).ok()))
        .filter(|&t| t > 0)
        .map_or(RENDER_TIMEOUT.as_secs(), |t| {
            t.min(MAX_RENDER_TIMEOUT.as_secs())
        })
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
        timeout: timeout_of(&v["timeout"]),
        inputs: Inputs::parse(&v["inputs"]),
        actions: actions_of(&v["actions"]),
        aliases: aliases_of(&v["aliases"]),
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

#[cfg(test)]
mod secret_name_tests {
    use super::env_secret;

    #[test]
    fn credential_looking_names_are_refused() {
        for n in [
            "API_KEY",
            "GH_PASS",
            "SESSION_COOKIE",
            "JWT",
            "BEARER_X",
            "PRIVATE_DIR",
            "MY_AUTH",
        ] {
            assert!(env_secret(n), "{n}");
        }
        for n in ["ANTHROPIC_BASE_URL", "HOME", "LANG"] {
            assert!(!env_secret(n), "{n}");
        }
    }
}
