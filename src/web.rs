//! Entry points of the web configurator (`web/`), built to wasm by
//! `web/wasm`: text in, text or JSON out. Nothing here reads a file,
//! starts a process or reads the clock; the caller passes `now`.
//! `manifest` is a `plugins.json` (`bar web plugins`), or empty.

use crate::check::{self, Level};
use crate::color::ColorDepth;
use crate::conf::{self, layout_key, tokenize, Config, Icons, Skin, SkinGlyphs, Wrap};
use crate::format::{self, Token};
use crate::options::Options;
use crate::plugin::{self, ExtInfo, OptType, RESERVED};
use crate::render::CONFIG_ERROR;
use crate::{describe, palette, placeholders, presets, preview, session};
use serde_json::{json, Value};

/// The status line `config_text` draws for the statusLine JSON
/// `session_json` in `columns` cells at `now` (Unix seconds): ANSI text,
/// rows joined by `\n`. Data the JSON does not carry (git, gh, transcript,
/// accounts) comes from bar's preview samples; plugins draw their manifest
/// sample, or their name. A config with errors draws the line `bar render`
/// prints for one.
pub fn render(
    config_text: &str,
    session_json: &str,
    columns: usize,
    now: i64,
    manifest: &str,
) -> String {
    match conf::parse(config_text) {
        Ok(c) => {
            let plugins = plugin::web::parse_manifest(manifest);
            preview::render_session(&session::parse(session_json), &c, columns, now, &plugins)
        }
        Err(_) => CONFIG_ERROR.to_string(),
    }
}

/// `bar check` on `config_text`: `{"ok": bool, "issues": [{"level",
/// "line", "message"}]}`. The browser has no `PATH`: warnings about `gh`
/// or a plugin missing from it are left out. A plugin in the manifest has
/// its options checked against its schema; any other name that is not a
/// core placeholder is a warning instead of an error.
pub fn check(config_text: &str, manifest: &str) -> Value {
    let plugins = plugin::web::parse_manifest(manifest);
    let listed = |name: &str| plugins.iter().any(|p| p.name == name);
    let report = check::check_text(config_text, None, &[]);
    let mut issues: Vec<(usize, &str, String)> = Vec::new();
    for i in &report.issues {
        if i.level == Level::Warning && i.message.contains("not on PATH") {
            continue;
        }
        let unknown = i
            .message
            .strip_prefix("unknown placeholder `")
            .and_then(|r| r.strip_suffix('`'))
            .filter(|_| i.level == Level::Error);
        match (unknown, i.level) {
            (Some(name), _) if listed(name) => {}
            (Some(name), _) => issues.push((
                i.line,
                "warning",
                format!("`{name}` is not a core placeholder or a plugin in the manifest; previewed as its name"),
            )),
            (None, Level::Error) => issues.push((i.line, "error", i.message.clone())),
            (None, Level::Warning) => issues.push((i.line, "warning", i.message.clone())),
        }
    }
    issues.extend(plugin_issues(config_text, &plugins));
    issues.sort_by_key(|i| i.0);
    let ok = !issues.iter().any(|i| i.1 == "error");
    let issues: Vec<Value> = issues
        .into_iter()
        .map(|(line, level, message)| json!({"level": level, "line": line, "message": message}))
        .collect();
    json!({"ok": ok, "issues": issues})
}

/// Options of manifest plugins used in layout lines, checked as a render
/// would (with `default` lines merged in).
fn plugin_issues<'a>(text: &str, plugins: &[ExtInfo]) -> Vec<(usize, &'a str, String)> {
    let defaults = conf::parse(text).map(|c| c.defaults).unwrap_or_default();
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let Ok(t) = tokenize(raw) else { continue };
        if !(t.len() == 3 && t[0] == "set" && matches!(layout_key(&t[1]), Some(Ok(_)))) {
            continue;
        }
        for tok in format::parse(&t[2]).unwrap_or_default() {
            let Token::Placeholder { name, options } = tok else {
                continue;
            };
            let Some(info) = plugins.iter().find(|p| p.name == name) else {
                continue;
            };
            let merged = match defaults.get(&name) {
                Some(d) => options.merged_over(d),
                None => options,
            };
            let mut own = Options::default();
            for (k, v) in merged
                .iter()
                .filter(|(k, _)| !RESERVED.contains(&k.as_str()))
            {
                own.set(k, v);
            }
            if let Err(e) = plugin::validate(&info.options, &own) {
                out.push((n + 1, "error", format!("{e} for `{name}`")));
            }
        }
    }
    out
}

/// The plugins of a manifest for the picker: `[{"name", "description",
/// "category", "icon", "options": [{"key", "kind", "values", "label",
/// "help", "placeholder"}]}]`, `kind` as in `schema` (`bool` is `one` of
/// `true`/`false`; a string option's `values` are its suggested `choices`).
pub fn plugins(manifest: &str) -> Value {
    let list: Vec<Value> = plugin::web::parse_manifest(manifest)
        .iter()
        .map(|p| {
            let web = p.web.as_ref().filter(|w| w.is_object());
            let field = |k: &str| web.and_then(|w| w.get(k)).and_then(Value::as_str);
            let options: Vec<Value> = p
                .options
                .iter()
                .map(|(key, spec)| {
                    let hint = web.and_then(|w| w["options"].get(key));
                    let hint_str = |k: &str| hint.and_then(|h| h.get(k)).and_then(Value::as_str);
                    let choices: Vec<&str> = hint
                        .and_then(|h| h["choices"].as_array())
                        .map(|a| a.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    let (kind, values): (&str, Vec<&str>) = match spec.kind {
                        OptType::Enum => ("one", spec.choices.iter().map(String::as_str).collect()),
                        OptType::Bool => ("one", vec!["true", "false"]),
                        OptType::Int => ("int", choices),
                        OptType::String => ("any", choices),
                    };
                    json!({
                        "key": key,
                        "kind": kind,
                        "values": values,
                        "label": hint_str("label"),
                        "help": hint_str("help"),
                        "placeholder": hint_str("placeholder"),
                        "default": spec.default,
                    })
                })
                .collect();
            json!({
                "name": p.name,
                "description": field("description").unwrap_or(&p.description),
                "category": field("category"),
                "icon": p.icon,
                "options": options,
            })
        })
        .collect();
    Value::Array(list)
}

/// The layout and look `config_text` sets: `{"ok": true, "lines":
/// [[left, center, right], ...], "set": {key: value}}`, or `{"ok": false,
/// "errors": [{"line", "message"}]}`.
pub fn inspect(config_text: &str) -> Value {
    match conf::parse(config_text) {
        Ok(c) => json!({
            "ok": true,
            "lines": c.lines(),
            "max_lines": conf::MAX_LINES,
            "set": settings(&c),
        }),
        Err(errs) => {
            let errors: Vec<Value> = errs
                .iter()
                .map(|e| json!({"line": e.line, "message": e.message}))
                .collect();
            json!({"ok": false, "errors": errors})
        }
    }
}

fn settings(c: &Config) -> Value {
    json!({
        "skin": match c.skin {
            Skin::Plain => "plain",
            Skin::Powerline => "powerline",
            Skin::Capsule => "capsule",
        },
        "skin-glyphs": match c.skin_glyphs {
            SkinGlyphs::Nerd => "nerd",
            SkinGlyphs::Ascii => "ascii",
        },
        "palette": c.palette.name,
        "icons": match c.icons {
            Icons::None => "none",
            Icons::Nerdfont => "nerdfont",
            Icons::Emoji => "emoji",
        },
        "icon-width": c.icon_width.to_string(),
        "wrap": match c.wrap {
            Wrap::Off => "off",
            Wrap::On => "on",
        },
        "separator": c.separator,
        "gap": c.gap.to_string(),
        "color-depth": c.color_depth.name(),
        "gradient": if c.gradient.is_empty() { "off" } else { &c.gradient },
    })
}

/// One group's source split into text and placeholders:
/// `{"ok": true, "tokens": [{"text"} | {"name", "options": [[k, v]]}]}`,
/// or `{"ok": false, "error"}`.
pub fn tokens(src: &str) -> Value {
    match format::parse(src) {
        Ok(tokens) => {
            let tokens: Vec<Value> = tokens
                .iter()
                .map(|t| match t {
                    Token::Text(s) => json!({"text": s}),
                    Token::Placeholder { name, options } => {
                        let opts: Vec<[&str; 2]> = options
                            .iter()
                            .map(|(k, v)| [k.as_str(), v.as_str()])
                            .collect();
                        json!({"name": name, "options": opts})
                    }
                })
                .collect();
            json!({"ok": true, "tokens": tokens})
        }
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// What the page offers: core placeholders with their description and
/// options (as `bar check` validates them, with default and help), the
/// options on every placeholder, presets, and the `set` keys with a fixed
/// set of values.
pub fn schema() -> Value {
    let placeholders: Vec<Value> = placeholders::names()
        .map(|name| {
            json!({
                "name": name,
                "description": describe::describe(name),
                "options": check::option_schema(name),
            })
        })
        .collect();
    let gradients: Vec<&str> = std::iter::once("off")
        .chain(crate::gradient::preset_names())
        .collect();
    json!({
        "placeholders": placeholders,
        "generic": crate::look::schema(),
        "presets": presets::NAMES,
        "settings": [
            {"key": "skin", "values": ["plain", "powerline", "capsule"]},
            {"key": "skin-glyphs", "values": ["nerd", "ascii"]},
            {"key": "palette", "values": palette::NAMES},
            {"key": "icons", "values": ["none", "nerdfont", "emoji"]},
            {"key": "icon-width", "values": ["1", "2"]},
            {"key": "wrap", "values": ["off", "on"]},
            {"key": "color-depth", "values": ColorDepth::NAMES},
            {"key": "gradient", "values": gradients},
        ],
    })
}

/// A shipped preset's text.
pub fn preset(name: &str) -> Option<&'static str> {
    presets::get(name)
}
