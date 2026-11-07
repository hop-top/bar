//! `bar click hook`: Claude Code hook keeping a per-session waiting marker
//! (`<state>/sessions/<session id>`, content `waiting` or `done`) for `bar click run`. Fast and silent;
//! never fails the session.

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

/// Applies one hook event (stdin JSON). Bad input does nothing.
pub fn run_hook(state: &Path, input: &str) {
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return;
    };
    let sid = v.get("session_id").and_then(Value::as_str).unwrap_or("");
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
