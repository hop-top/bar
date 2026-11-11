//! `bar waybar`: the tray snapshot as one waybar custom-module line,
//! `{text, tooltip, class, alt}`. It runs the same builder as
//! `bar tray state` (and writes the same `tray-menu.json`, so a picker
//! launched from a click reads the generation of what the bar showed),
//! never a second copy of the plugin calls; the plugins' 3 s caps apply.
//! Waybar renders `text` and `tooltip` as Pango markup, so every string
//! that comes from a plugin is escaped.

use serde_json::{json, Value};

/// Pango-safe text: `&`, `<` and `>` escaped.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// Rank of an item style for the module class: 0 none or dim, 1 good,
/// 2 warn, 3 bad.
fn rank(style: Option<&str>) -> u8 {
    match style {
        Some("good") => 1,
        Some("warn") => 2,
        Some("bad") => 3,
        _ => 0,
    }
}

/// The CSS class: the worst item style (`bad` over `warn` over `good`),
/// a failed last action counting as `bad`; `idle` when nothing has a style.
fn class(v: &Value) -> &'static str {
    let mut worst = 0;
    if v["last_action"]["ok"] == Value::Bool(false) {
        worst = 3;
    }
    for i in items(v) {
        worst = worst.max(rank(i["style"].as_str()));
    }
    ["idle", "good", "warn", "bad"][usize::from(worst)]
}

fn groups(v: &Value) -> impl Iterator<Item = &Value> {
    v["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["groups"].as_array().into_iter().flatten())
}

fn items(v: &Value) -> impl Iterator<Item = &Value> {
    groups(v).flat_map(|g| g["items"].as_array().into_iter().flatten())
}

/// The short bar text: the checked item of the first two picker groups,
/// then the waiting-session count; `bar` when there is nothing to say.
fn text(v: &Value) -> String {
    let mut parts: Vec<String> = groups(v)
        .filter(|g| g["kind"] == "radio")
        .filter_map(|g| {
            let all = g["items"].as_array()?;
            all.iter().find(|i| i["checked"] == Value::Bool(true))
        })
        .filter_map(|i| i["label"].as_str().map(escape))
        .take(2)
        .collect();
    let waiting = v["sessions"]["waiting"].as_u64().unwrap_or(0);
    if waiting > 0 {
        parts.push(format!("{waiting} waiting"));
    }
    if parts.is_empty() {
        "bar".to_string()
    } else {
        parts.join(" · ")
    }
}

fn mark(kind: &str, i: &Value) -> &'static str {
    let on = i["checked"] == Value::Bool(true);
    match (kind, on) {
        ("radio", true) => "●",
        ("radio", false) => "○",
        ("check", true) => "☑",
        ("check", false) => "☐",
        _ => "•",
    }
}

/// The multi-line tooltip: plugin headings, group labels, one line per item.
fn tooltip(v: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    for p in v["plugins"].as_array().into_iter().flatten() {
        let name = p["label"].as_str().or(p["name"].as_str()).unwrap_or("?");
        lines.push(format!("<b>{}</b>", escape(name)));
        for g in p["groups"].as_array().into_iter().flatten() {
            let kind = g["kind"].as_str().unwrap_or("");
            if let Some(l) = g["label"].as_str() {
                lines.push(escape(l));
            }
            for i in g["items"].as_array().into_iter().flatten() {
                let mut l = format!(
                    "  {} {}",
                    mark(kind, i),
                    escape(i["label"].as_str().unwrap_or(""))
                );
                if let Some(d) = i["detail"].as_str() {
                    l.push_str(&format!(" ({})", escape(d)));
                }
                lines.push(l);
            }
        }
    }
    if lines.is_empty() {
        lines.push("No tray plugins: add a tray line to the bar config".to_string());
    }
    if let Some(a) = v["last_action"].as_object() {
        let ok = if a["ok"] == Value::Bool(true) {
            "ok"
        } else {
            "failed"
        };
        let note = a["note"].as_str().filter(|n| !n.is_empty());
        lines.push(match note {
            Some(n) => format!("Last action {ok}: {}", escape(n)),
            None => format!("Last action {ok}"),
        });
    }
    lines.join("\n")
}

/// The module object for a snapshot.
pub fn module(v: &Value) -> Value {
    let class = class(v);
    json!({"text": text(v), "tooltip": tooltip(v), "class": class, "alt": class})
}
