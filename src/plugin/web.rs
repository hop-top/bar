//! The optional `web` object of `--ext-info`: what the web configurator
//! shows for a plugin it cannot run (picker text, option hints, preview
//! samples in the render-output shape), its validation, and the
//! `plugins.json` manifest.

use serde_json::{json, Map, Value};

use super::info::{typed, OptType};
use super::render::{parse_output, Output};
use super::ExtInfo;

/// Option hint fields that hold a string.
const HINT_STRINGS: [&str; 3] = ["label", "help", "placeholder"];

/// Problems in `info.web`, one message each; none when it is absent.
/// Unknown fields are ignored, so the object can grow.
pub fn issues(info: &ExtInfo) -> Vec<String> {
    let Some(web) = &info.web else {
        return Vec::new();
    };
    let Some(web) = web.as_object() else {
        return vec!["must be an object".into()];
    };
    let mut out = Vec::new();
    for k in ["description", "category"] {
        if web.get(k).is_some_and(|v| !v.is_string()) {
            out.push(format!("`{k}` must be a string"));
        }
    }
    if let Some(samples) = web.get("samples") {
        match samples.as_array() {
            Some(list) => {
                for (i, s) in list.iter().enumerate() {
                    out.extend(
                        sample_issues(info, s)
                            .into_iter()
                            .map(|m| format!("samples[{i}]: {m}")),
                    );
                }
            }
            None => out.push("`samples` must be an array".into()),
        }
    }
    if let Some(opts) = web.get("options") {
        match opts.as_object() {
            Some(map) => {
                for (k, hint) in map {
                    out.extend(
                        hint_issues(info, k, hint)
                            .into_iter()
                            .map(|m| format!("options.{k}: {m}")),
                    );
                }
            }
            None => out.push("`options` must be an object".into()),
        }
    }
    out
}

fn sample_issues(info: &ExtInfo, s: &Value) -> Vec<String> {
    let Some(s) = s.as_object() else {
        return vec!["must be an object".into()];
    };
    let mut out = Vec::new();
    match s.get("output") {
        Some(o) if o.is_object() && parse_output(&o.to_string()).is_some() => {}
        _ => out.push("`output` must be a render output object with string `text`".into()),
    }
    match s.get("when") {
        None => {}
        Some(Value::Object(when)) => {
            for (k, v) in when {
                match info.options.get(k) {
                    None => out.push(format!("`when` names undeclared option `{k}`")),
                    Some(spec) if when_value(spec, v).is_none() => {
                        out.push(format!("`when.{k}` is not a valid value"));
                    }
                    _ => {}
                }
            }
        }
        Some(_) => out.push("`when` must be an object".into()),
    }
    out
}

fn hint_issues(info: &ExtInfo, key: &str, hint: &Value) -> Vec<String> {
    let Some(spec) = info.options.get(key) else {
        return vec!["not a declared option".into()];
    };
    let Some(hint) = hint.as_object() else {
        return vec!["must be an object".into()];
    };
    let mut out = Vec::new();
    for k in HINT_STRINGS {
        if hint.get(k).is_some_and(|v| !v.is_string()) {
            out.push(format!("`{k}` must be a string"));
        }
    }
    if let Some(choices) = hint.get("choices") {
        match choices.as_array() {
            Some(list) => {
                for c in list {
                    if !c.as_str().is_some_and(|c| typed(spec, c).is_some()) {
                        out.push(format!("choice {c} is not a valid value"));
                    }
                }
            }
            None => out.push("`choices` must be an array of strings".into()),
        }
    }
    out
}

/// A `when` value as the plugin would receive it: a string goes through the
/// option's type, a JSON value must already have it.
fn when_value(spec: &super::OptSpec, v: &Value) -> Option<Value> {
    match v {
        Value::String(s) => typed(spec, s),
        Value::Bool(_) if spec.kind == OptType::Bool => Some(v.clone()),
        Value::Number(_) if spec.kind == OptType::Int && v.is_i64() => Some(v.clone()),
        _ => None,
    }
}

/// The preview output for typed `options` (as `validate` returns them):
/// the first sample whose `when` matches them all, else a generic one,
/// the plugin name as text.
pub fn sample(info: &ExtInfo, options: &Map<String, Value>) -> Output {
    matching_sample(info, options).unwrap_or_else(|| generic(&info.name))
}

/// The first sample whose `when` matches typed `options`; `None` when the
/// plugin gives none that does.
pub fn matching_sample(info: &ExtInfo, options: &Map<String, Value>) -> Option<Output> {
    let samples = info
        .web
        .as_ref()
        .and_then(|w| w.get("samples"))
        .and_then(Value::as_array);
    let matches = |s: &Value| {
        s.get("when").and_then(Value::as_object).is_none_or(|when| {
            when.iter().all(|(k, v)| {
                let spec = info.options.get(k);
                let want = spec.and_then(|spec| when_value(spec, v));
                want.is_some() && options.get(k) == want.as_ref()
            })
        })
    };
    samples
        .into_iter()
        .flatten()
        .filter(|s| matches(s))
        .find_map(|s| parse_output(&s.get("output")?.to_string()))
}

fn generic(name: &str) -> Output {
    Output {
        text: name.to_string(),
        ..Output::default()
    }
}

/// `plugins.json`: the `--ext-info` of every placeholder plugin.
pub fn manifest(infos: &[ExtInfo]) -> Value {
    let plugins: Vec<Value> = infos
        .iter()
        .filter(|i| i.is_placeholder())
        .map(ExtInfo::to_json)
        .collect();
    json!({ "plugins": plugins })
}

/// The placeholder plugins of a manifest (`{"plugins": [...]}` or a bare
/// array); entries that do not parse as `--ext-info` are skipped.
pub fn parse_manifest(raw: &str) -> Vec<ExtInfo> {
    let Ok(v) = serde_json::from_str::<Value>(raw.trim()) else {
        return Vec::new();
    };
    let list = v.get("plugins").unwrap_or(&v);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| super::parse_info(&e.to_string()))
        .filter(ExtInfo::is_placeholder)
        .collect()
}
