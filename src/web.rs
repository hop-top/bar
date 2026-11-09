//! Entry points of the web configurator (`web/`), built to wasm by
//! `web/crate`: text in, text or JSON out. Nothing here reads a file,
//! starts a process or reads the clock; the caller passes `now`.

use crate::check::{self, Level};
use crate::conf::{self, Config, Icons, Skin, SkinGlyphs, Wrap};
use crate::format::{self, Token};
use crate::render::CONFIG_ERROR;
use crate::{palette, placeholders, presets, preview, session};
use serde_json::{json, Value};

/// The status line `config_text` draws for the statusLine JSON
/// `session_json` in `columns` cells at `now` (Unix seconds): ANSI text,
/// rows joined by `\n`. Data the JSON does not carry (git, gh, transcript,
/// accounts) comes from bar's preview samples; plugins draw nothing. A
/// config with errors draws the line `bar render` prints for one.
pub fn render(config_text: &str, session_json: &str, columns: usize, now: i64) -> String {
    match conf::parse(config_text) {
        Ok(c) => preview::render_session(&session::parse(session_json), &c, columns, now),
        Err(_) => CONFIG_ERROR.to_string(),
    }
}

/// `bar check` on `config_text`: `{"ok": bool, "issues": [{"level",
/// "line", "message"}]}`. The browser has no `PATH`: warnings about `gh`
/// or a plugin missing from it are left out, and a name that is not a
/// core placeholder is a warning (a plugin, unchecked) instead of an error.
pub fn check(config_text: &str) -> Value {
    let report = check::check_text(config_text, None, &[]);
    let mut ok = true;
    let issues: Vec<Value> = report
        .issues
        .iter()
        .filter(|i| !(i.level == Level::Warning && i.message.contains("not on PATH")))
        .map(|i| {
            let plugin = i
                .message
                .strip_prefix("unknown placeholder ")
                .filter(|_| i.level == Level::Error);
            let (level, message) = match (plugin, i.level) {
                (Some(name), _) => (
                    "warning",
                    format!(
                        "{name} is not a core placeholder; a plugin draws it, not checked here"
                    ),
                ),
                (None, Level::Error) => ("error", i.message.clone()),
                (None, Level::Warning) => ("warning", i.message.clone()),
            };
            ok &= level != "error";
            json!({"level": level, "line": i.line, "message": message})
        })
        .collect();
    json!({"ok": ok, "issues": issues})
}

/// The layout and look `config_text` sets: `{"ok": true, "lines":
/// [[left, center, right], ...], "set": {key: value}}`, or `{"ok": false,
/// "errors": [{"line", "message"}]}`.
pub fn inspect(config_text: &str) -> Value {
    match conf::parse(config_text) {
        Ok(c) => json!({
            "ok": true,
            "lines": c.lines(),
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
        "palette": c.palette,
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

/// What the page offers: core placeholders with their options (as
/// `bar check` validates them), presets, and the `set` keys with a fixed
/// set of values.
pub fn schema() -> Value {
    let placeholders: Vec<Value> = placeholders::names()
        .map(|name| json!({"name": name, "options": check::option_schema(name)}))
        .collect();
    json!({
        "placeholders": placeholders,
        "presets": presets::NAMES,
        "settings": [
            {"key": "skin", "values": ["plain", "powerline", "capsule"]},
            {"key": "skin-glyphs", "values": ["nerd", "ascii"]},
            {"key": "palette", "values": palette::NAMES},
            {"key": "icons", "values": ["none", "nerdfont", "emoji"]},
            {"key": "icon-width", "values": ["1", "2"]},
            {"key": "wrap", "values": ["off", "on"]},
        ],
    })
}

/// A shipped preset's text.
pub fn preset(name: &str) -> Option<&'static str> {
    presets::get(name)
}
