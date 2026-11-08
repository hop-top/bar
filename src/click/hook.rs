//! `bar click hook`: Claude Code hook keeping a per-session waiting marker
//! (`<state>/sessions/<session id>`, content `waiting` or `done`) for `bar click run`,
//! the running-subagent registry and per-session plugin records. Fast and
//! silent; never fails the session.

use super::url::valid_sid;
use super::write_atomic;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn marker(state: &Path, sid: &str) -> Option<PathBuf> {
    valid_sid(sid).then(|| state.join("sessions").join(sid))
}

/// Marker content: `waiting` or `done`; `None` without a marker.
pub fn mark(state: &Path, sid: &str) -> Option<String> {
    let m = std::fs::read_to_string(marker(state, sid)?).ok()?;
    Some(m.trim().to_owned())
}

pub fn is_waiting(state: &Path, sid: &str) -> bool {
    mark(state, sid).as_deref() == Some("waiting")
}

enum Action {
    Set,
    Done,
    Clear,
    Keep,
}

fn action(v: &Value) -> Action {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let main = s("agent_id").is_empty();
    match s("hook_event_name") {
        "PermissionRequest" | "StopFailure" => Action::Set,
        "PreToolUse" if s("tool_name") == "AskUserQuestion" => Action::Set,
        "Notification"
            if matches!(
                s("notification_type"),
                "permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog"
            ) =>
        {
            Action::Set
        }
        "UserPromptSubmit" | "SessionEnd" => Action::Clear,
        // A subagent finishing says nothing about a prompt on the main session.
        "Stop" if main => Action::Done,
        "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" if main => Action::Clear,
        _ => Action::Keep,
    }
}

/// Running-subagent registry: `<state>/agents/<session id>/<agent id>`,
/// content the agent type. Entry names are validated, so no path escapes.
pub fn agents_dir(state: &Path, sid: &str) -> Option<PathBuf> {
    valid_sid(sid).then(|| state.join("agents").join(sid))
}

pub(crate) fn valid_agent_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Registry dir for writing: refused when it is a symlink, so a planted
/// link cannot redirect writes.
pub(crate) fn is_real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

/// Removes entries older than `stale` seconds and orphan temp files;
/// bounded work.
fn prune(dir: &Path, now: i64, stale: i64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten().take(256) {
        let old = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|d| now - (d.as_secs() as i64) >= stale);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Per-session plugin records: `<state>/records/<plugin>/<session id>`.
pub const RECORDS: &str = "records";
/// Records untouched this long are pruned on the next write.
pub const RECORD_STALE_SECS: i64 = 30 * 86_400;

/// Plugin names as discovery accepts them: `[A-Za-z0-9_-]+`.
pub(crate) fn valid_plugin_name(s: &str) -> bool {
    valid_agent_id(s)
}

/// Where the hook keeps `plugin`'s record for session `sid`.
pub fn record_path(state: &Path, plugin: &str, sid: &str) -> Option<PathBuf> {
    (valid_sid(sid) && valid_plugin_name(plugin))
        .then(|| state.join(RECORDS).join(plugin).join(sid))
}

/// The record as JSON; `None` when missing, oversized or not JSON.
pub fn read_record(state: &Path, plugin: &str, sid: &str) -> Option<Value> {
    use std::io::Read;
    let mut body = String::new();
    std::fs::File::open(record_path(state, plugin, sid)?)
        .ok()?
        .take(4096)
        .read_to_string(&mut body)
        .ok()?;
    serde_json::from_str::<Value>(&body)
        .ok()
        .filter(Value::is_object)
}

/// Bash output as the hook sees it: `tool_response` is an object with
/// `stdout` and `stderr`, or a plain string.
fn tool_output(v: &Value) -> String {
    let r = &v["tool_response"];
    match r.as_str() {
        Some(s) => s.to_owned(),
        None => ["stdout", "stderr"]
            .iter()
            .filter_map(|k| r[*k].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// `PostToolUse` of `Bash`: records the last tlc task the command touched.
/// Subagent tool calls carry the main session's id, so they count for it.
fn record_tlc(state: &Path, v: &Value, sid: &str) {
    let Some(path) = record_path(state, "tlc", sid) else {
        return;
    };
    let cwd = Path::new(v["cwd"].as_str().unwrap_or(""));
    let Some(cmd) = v["tool_input"]["command"].as_str() else {
        return;
    };
    if !cwd.is_absolute() || !cmd.contains("tlc") {
        return;
    }
    let Some(t) = super::tlc::last_touch(cmd, cwd, &tool_output(v)) else {
        return;
    };
    let dir = path.parent().unwrap_or(state);
    if std::fs::symlink_metadata(dir).is_ok() && !is_real_dir(dir) {
        return;
    }
    // the same task again keeps the record, so the plugin's cache entry
    // (keyed by the record) stays warm
    let same = read_record(state, "tlc", sid)
        .is_some_and(|r| r["task"] == t.task.as_str() && r["cwd"].as_str() == t.cwd.to_str());
    if same {
        return;
    }
    let now = unix_now();
    prune(dir, now, RECORD_STALE_SECS);
    let body = serde_json::json!({"task": t.task, "cwd": t.cwd, "at": now});
    let _ = write_atomic(&path, &body.to_string());
}

/// `SessionEnd`: drops the session's records of every plugin.
fn drop_records(state: &Path, sid: &str) {
    if !valid_sid(sid) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(state.join(RECORDS)) else {
        return;
    };
    for e in rd.flatten().take(64) {
        if is_real_dir(&e.path()) {
            let _ = std::fs::remove_file(e.path().join(sid));
        }
    }
}

fn track_agent(state: &Path, v: &Value, sid: &str) {
    let Some(dir) = agents_dir(state, sid) else {
        return;
    };
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let id = s("agent_id");
    if !valid_agent_id(id) {
        return;
    }
    match s("hook_event_name") {
        "SubagentStart" => {
            if std::fs::symlink_metadata(&dir).is_ok() && !is_real_dir(&dir) {
                return;
            }
            prune(&dir, unix_now(), crate::placeholders::agents::STALE_SECS);
            let kind: String = crate::style::sanitize(s("agent_type"))
                .chars()
                .take(64)
                .collect();
            let _ = write_atomic(&dir.join(id), &kind);
        }
        "SubagentStop" => {
            let _ = std::fs::remove_file(dir.join(id));
        }
        _ => {}
    }
}

/// Applies one hook event (stdin JSON). Bad input does nothing.
pub fn run_hook(state: &Path, input: &str) {
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return;
    };
    let sid = v.get("session_id").and_then(Value::as_str).unwrap_or("");
    match v.get("hook_event_name").and_then(Value::as_str) {
        Some("SubagentStart" | "SubagentStop") => return track_agent(state, &v, sid),
        Some("SessionEnd") => {
            if let Some(d) = agents_dir(state, sid).filter(|d| is_real_dir(d)) {
                let _ = std::fs::remove_dir_all(d);
            }
            drop_records(state, sid);
        }
        Some("PostToolUse") if v["tool_name"] == "Bash" => record_tlc(state, &v, sid),
        _ => {}
    }
    let Some(m) = marker(state, sid) else {
        return;
    };
    match action(&v) {
        Action::Set => {
            let _ = write_atomic(&m, "waiting");
        }
        Action::Done => {
            let _ = write_atomic(&m, "done");
        }
        Action::Clear => {
            let _ = std::fs::remove_file(&m);
        }
        Action::Keep => {}
    }
}
