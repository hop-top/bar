//! Claude Code's `settings.json`: reading it, the `bar click hook`
//! entries and the command strings bar writes into it.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Events the hook listens to; it filters the rest itself.
pub const HOOK_EVENTS: [&str; 12] = [
    "PermissionRequest",
    "Notification",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionDenied",
    "StopFailure",
    "Stop",
    "UserPromptSubmit",
    "SessionEnd",
    "SubagentStart",
    "SubagentStop",
];

/// Shell command Claude Code runs: `'<bar>' click hook`.
pub fn hook_command(bar: &Path) -> String {
    hook_command_for(&bar.display().to_string(), cfg!(windows))
}

/// As `hook_command`, for either platform.
pub fn hook_command_for(bar: &str, windows: bool) -> String {
    command_for(bar, "click hook", windows)
}

/// `'<bar>' <args>` as a Claude Code command (hook or status line).
/// Claude Code on Windows runs these through Git Bash, or PowerShell
/// without it; commands are written for both: backslashes become forward
/// slashes (Git Bash drops unquoted ones), and a path of plain characters
/// stays unquoted, which both shells run as is. Other paths are
/// single-quoted, which only Git Bash accepts.
pub fn command_for(bar: &str, args: &str, windows: bool) -> String {
    let p = if windows {
        bar.replace('\\', "/")
    } else {
        bar.to_string()
    };
    let plain = |c: char| c.is_ascii_alphanumeric() || "_-./:".contains(c);
    if windows && p.chars().all(plain) {
        return format!("{p} {args}");
    }
    format!("'{}' {args}", p.replace('\'', r"'\''"))
}

pub struct SettingsPlan {
    /// The file written: `settings.json`, or its target when it is a symlink.
    pub path: PathBuf,
    pub command: String,
    /// Events that gain a hook entry; empty means nothing to write.
    pub added: Vec<&'static str>,
    pub after: Value,
}

impl SettingsPlan {
    pub fn describe(&self) -> Vec<String> {
        let p = self.path.display();
        if self.added.is_empty() {
            return vec![format!("{p}: hooks already wired")];
        }
        let mut v = vec![format!("{p}: will change")];
        v.extend(
            self.added
                .iter()
                .map(|e| format!("  add {e} hook: {}", self.command)),
        );
        v
    }
}

fn has_command(entries: &[Value], command: &str) -> bool {
    entries.iter().any(|e| {
        e.get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hs| hs.iter().any(|h| h.get("command") == Some(&json!(command))))
    })
}

/// A settings file as read: its write target, raw text (`None` when
/// missing) and parsed root object.
pub(crate) struct Loaded {
    pub path: PathBuf,
    pub text: Option<String>,
    pub root: Value,
}

/// Reads `path` (missing counts as `{}`); a file that is not a JSON
/// object is an error, so a file we do not understand is never rewritten.
pub(crate) fn load(path: &Path) -> Result<Loaded, String> {
    let path = crate::rewrite::target(path)?;
    let shown = path.display();
    let (text, root) = match std::fs::read_to_string(&path) {
        Ok(s) => {
            let v = serde_json::from_str::<Value>(&s).map_err(|e| format!("{shown}: {e}"))?;
            (Some(s), v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, json!({})),
        Err(e) => return Err(format!("{shown}: {e}")),
    };
    if !root.is_object() {
        return Err(format!("{shown}: not a JSON object"));
    }
    Ok(Loaded { path, text, root })
}

/// Adds a `command` hook entry for every event lacking one; returns the
/// events that gained one. Anything not shaped as Claude Code expects is
/// an error.
pub(crate) fn add_hooks(
    root: &mut Value,
    command: &str,
    shown: &str,
) -> Result<Vec<&'static str>, String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("{shown}: not a JSON object"))?;
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("{shown}: `hooks` is not an object"))?;
    let mut added = Vec::new();
    for ev in HOOK_EVENTS {
        let list = hooks
            .entry(ev)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("{shown}: `hooks.{ev}` is not an array"))?;
        if !has_command(list, command) {
            list.push(json!({"hooks": [{"type": "command", "command": command}]}));
            added.push(ev);
        }
    }
    Ok(added)
}

/// Drops hook entries whose every hook runs `command`, then any event
/// list and `hooks` object this leaves empty; returns the events changed.
pub(crate) fn remove_hooks(root: &mut Value, command: &str) -> Vec<&'static str> {
    let mut removed = Vec::new();
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return removed;
    };
    let ours = |e: &Value| {
        e.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
            !hs.is_empty() && hs.iter().all(|h| h.get("command") == Some(&json!(command)))
        })
    };
    for ev in HOOK_EVENTS {
        let Some(list) = hooks.get_mut(ev).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = list.len();
        list.retain(|e| !ours(e));
        if list.len() != before {
            removed.push(ev);
            if list.is_empty() {
                hooks.shift_remove(ev);
            }
        }
    }
    if hooks.is_empty() && !removed.is_empty() {
        if let Some(o) = root.as_object_mut() {
            o.shift_remove("hooks");
        }
    }
    removed
}

/// Reads `path` (missing counts as `{}`) and adds a hook entry for every
/// event lacking one.
pub fn plan_settings(path: &Path, bar: &Path) -> Result<SettingsPlan, String> {
    let Loaded { path, mut root, .. } = load(path)?;
    let command = hook_command(bar);
    let added = add_hooks(&mut root, &command, &path.display().to_string())?;
    Ok(SettingsPlan {
        path,
        command,
        added,
        after: root,
    })
}

/// Where backups of settings files go: `backups/` in the state dir, never
/// beside the file, since `.claude/` may be committed and the file may
/// hold keys.
pub fn backup_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("backups")
}

/// Writes the plan; returns the backup path when an existing file changed.
pub(crate) fn apply_settings(
    plan: &SettingsPlan,
    state_dir: &Path,
) -> std::io::Result<Option<PathBuf>> {
    if plan.added.is_empty() {
        return Ok(None);
    }
    write_settings(&plan.path, &plan.after, state_dir)
}

/// Writes `after` over `path` (backup in [`backup_dir`], atomic, mode
/// kept); returns the backup when there was a file.
pub(crate) fn write_settings(
    path: &Path,
    after: &Value,
    state_dir: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let dir = backup_dir(state_dir);
    crate::rewrite::replace(path, &render(after), crate::rewrite::Backups::In(&dir))
}

/// The text written for `v`: pretty JSON and a trailing newline.
pub(crate) fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::hook_command_for;

    #[test]
    fn unix_paths_are_single_quoted() {
        assert_eq!(
            hook_command_for("/opt/my bar/bar", false),
            "'/opt/my bar/bar' click hook"
        );
        assert_eq!(
            hook_command_for("/o/it's/bar", false),
            r"'/o/it'\''s/bar' click hook"
        );
    }

    #[test]
    fn windows_paths_use_forward_slashes_and_stay_plain() {
        assert_eq!(
            hook_command_for(r"C:\Users\me\bin\bar.exe", true),
            "C:/Users/me/bin/bar.exe click hook"
        );
        assert_eq!(
            hook_command_for(r"C:\Users\Jane Doe\bar.exe", true),
            "'C:/Users/Jane Doe/bar.exe' click hook"
        );
    }
}
