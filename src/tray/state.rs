//! The tray snapshot, version 1: one JSON object with `generation` (an id
//! of the runnable items, which the tray echoes back when it runs one;
//! see [`generation`]), `at`, `last_action` (from
//! `action-status`), `sessions` (counts of the hook's markers), `views`
//! (the toggles, only with `set tray-views on`) and `plugins` (one entry
//! per `tray` line, its menu from `--menu`). Written to `tray-menu.json`
//! in the state dir, mode 0600, by a rename: the tray never reads a
//! half-written file. The render `cache/` dir is never touched.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::{plugins, SNAPSHOT_FILE};
use crate::click::{self, action::STATUS_FILE};
use crate::conf::Config;

/// The snapshot contract version this writes.
pub const VERSION: u64 = 1;
/// Most session markers counted.
const MAX_SESSIONS: usize = 256;

/// One `action-status` line, `<at> ok[: <note>]` or `<at> refused: <note>`,
/// as `{at, ok, note}`; `None` for anything else.
pub fn parse_status(line: &str) -> Option<Value> {
    let (at, rest) = line.trim().split_once(' ')?;
    let at: u64 = at.parse().ok()?;
    let (ok, rest) = match rest.strip_prefix("ok") {
        Some(r) => (true, r),
        None => (false, rest.strip_prefix("refused")?),
    };
    let note = match rest {
        "" if ok => "",
        r => r.strip_prefix(": ")?.trim(),
    };
    Some(json!({"at": at, "ok": ok, "note": note}))
}

/// The last action's outcome, from the state dir's `action-status`.
pub fn last_action(state: &Path) -> Option<Value> {
    let s = std::fs::read_to_string(state.join(STATUS_FILE)).ok()?;
    parse_status(s.lines().next()?)
}

/// `{waiting, done}`: how many session markers say each.
pub fn sessions(state: &Path) -> Value {
    let (mut waiting, mut done) = (0u64, 0u64);
    if let Ok(rd) = std::fs::read_dir(state.join("sessions")) {
        for e in rd.flatten().take(MAX_SESSIONS) {
            match std::fs::read_to_string(e.path()).as_deref().map(str::trim) {
                Ok("waiting") => waiting += 1,
                Ok("done") => done += 1,
                _ => {}
            }
        }
    }
    json!({"waiting": waiting, "done": done})
}

/// The view toggles the tray may flip with `bar toggle <key>`: the usage
/// views and the alternate text of every granted plugin; empty unless
/// the user set `tray-views on`.
pub fn views(state: &Path, config: &Config) -> Vec<Value> {
    if !config.tray_views {
        return Vec::new();
    }
    let view = |key: &str, label: String| json!({"key": key, "label": label, "on": click::is_toggled(state, key)});
    let mut out: Vec<Value> = ["5h", "7d"]
        .iter()
        .map(|k| view(k, format!("{k} usage")))
        .collect();
    for name in plugins::granted_names(config) {
        out.push(view(&name, format!("{name} alternate text")));
    }
    out
}

/// The snapshot id: `[0-9a-f]`, 32 characters, a hash of what a run
/// depends on, in order: each plugin's name, each group's id, each
/// item's id, action, token values, `enabled` and `confirm`. Two
/// snapshots whose runnable items agree share it, so a tray that holds
/// a menu is not refused because `bar waybar` or an idle poll rewrote
/// the file with the same items; one whose items moved (an account
/// gone, a token changed, a grant added) gets a new one. Labels,
/// details, `checked`, styles, the view toggles and the last action do
/// not count: a click means the same with them changed.
pub fn generation(plugins: &[Value]) -> String {
    let mut key = String::new();
    let field = |k: &mut String, v: &Value| {
        k.push_str(&v.to_string());
        k.push('\n');
    };
    for p in plugins {
        field(&mut key, &p["name"]);
        for g in p["groups"].as_array().into_iter().flatten() {
            field(&mut key, &g["id"]);
            for i in g["items"].as_array().into_iter().flatten() {
                field(&mut key, &i["id"]);
                field(&mut key, &i["action"]);
                let tokens: std::collections::BTreeMap<&str, &Value> = i["tokens"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect();
                for (k, v) in tokens {
                    key.push_str(k);
                    key.push('=');
                    field(&mut key, v);
                }
                field(&mut key, &i["enabled"]);
                field(&mut key, &i["confirm"]);
            }
        }
    }
    crate::cache::hash_hex(&key)
}

/// The snapshot for `now` (Unix seconds): file reads, then every granted
/// plugin's `--menu` in parallel.
pub fn snapshot(state: &Path, config: &Config, dirs: &[PathBuf], now: u64) -> Value {
    let plugins = plugins::menus(state, config, dirs);
    let mut v = json!({"version": VERSION, "generation": generation(&plugins), "at": now});
    if let Some(a) = last_action(state) {
        v["last_action"] = a;
    }
    v["sessions"] = sessions(state);
    v["views"] = Value::Array(views(state, config));
    v["plugins"] = Value::Array(plugins);
    v
}

/// Builds the snapshot and writes it to `<state>/tray-menu.json` (0600,
/// a sibling temp file renamed into place); returns it.
pub fn run(state: &Path, config: &Config, dirs: &[PathBuf]) -> Result<Value, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let v = snapshot(state, config, dirs, now);
    let path = state.join(SNAPSHOT_FILE);
    click::write_atomic(&path, &format!("{v}\n"))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(v)
}

/// One line for a terminal: where the snapshot went and what it holds.
pub fn summary(v: &Value, state: &Path) -> String {
    let plugins = v["plugins"].as_array().map_or(0, Vec::len);
    let items: usize = v["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["groups"].as_array().into_iter().flatten())
        .map(|g| g["items"].as_array().map_or(0, Vec::len))
        .sum();
    format!(
        "{}: {plugins} plugin(s), {items} item(s)",
        state.join(SNAPSHOT_FILE).display()
    )
}
