//! Wires `bar click hook` into Claude Code's `settings.json`: plan the
//! change, list it, then back up and write atomically.

use super::write_atomic;
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

/// As `hook_command`, for either platform. Claude Code on Windows runs
/// status line commands through Git Bash, or PowerShell without it; hook
/// commands are written for both: backslashes become forward slashes (Git
/// Bash drops unquoted ones), and a path of plain characters stays
/// unquoted, which both shells run as is. Other paths are single-quoted,
/// which only Git Bash accepts.
pub fn hook_command_for(bar: &str, windows: bool) -> String {
    let p = if windows {
        bar.replace('\\', "/")
    } else {
        bar.to_string()
    };
    let plain = |c: char| c.is_ascii_alphanumeric() || "_-./:".contains(c);
    if windows && p.chars().all(plain) {
        return format!("{p} click hook");
    }
    format!("'{}' click hook", p.replace('\'', r"'\''"))
}

pub struct SettingsPlan {
    /// The file written: `settings.json`, or its target when it is a symlink.
    pub path: PathBuf,
    /// The file existed (so it gets a backup).
    pub existed: bool,
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

/// Reads `path` (missing counts as `{}`) and adds a hook entry for every
/// event lacking one. Anything not shaped as Claude Code expects is an
/// error, so a file we do not understand is never rewritten.
pub fn plan_settings(path: &Path, bar: &Path) -> Result<SettingsPlan, String> {
    let path = &write_target(path)?;
    let shown = path.display();
    let (existed, mut root) = match std::fs::read_to_string(path) {
        Ok(s) => (
            true,
            serde_json::from_str::<Value>(&s).map_err(|e| format!("{shown}: {e}"))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, json!({})),
        Err(e) => return Err(format!("{shown}: {e}")),
    };
    let command = hook_command(bar);
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
        if !has_command(list, &command) {
            list.push(json!({"hooks": [{"type": "command", "command": command}]}));
            added.push(ev);
        }
    }
    Ok(SettingsPlan {
        path: path.to_path_buf(),
        existed,
        command,
        added,
        after: root,
    })
}

/// A symlinked `settings.json` is written through to its target, never
/// replaced; a link whose target is missing is refused.
fn write_target(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)
            .map_err(|e| format!("{}: symlink target unusable: {e}", path.display())),
        _ => Ok(path.to_path_buf()),
    }
}

/// Writes the plan; returns the backup path when an existing file changed.
pub(crate) fn apply_settings(plan: &SettingsPlan) -> std::io::Result<Option<PathBuf>> {
    if plan.added.is_empty() {
        return Ok(None);
    }
    let backup = if plan.existed {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let name = plan.path.file_name().unwrap_or_default().to_string_lossy();
        let b = plan.path.with_file_name(format!(
            "{name}.bak-{}-{:09}-{}",
            now.as_secs(),
            now.subsec_nanos(),
            std::process::id()
        ));
        std::fs::copy(&plan.path, &b)?;
        Some(b)
    } else {
        None
    };
    let mode = std::fs::metadata(&plan.path).map(|m| m.permissions()).ok();
    let body = serde_json::to_string_pretty(&plan.after)? + "\n";
    write_atomic(&plan.path, &body)?;
    if let Some(m) = mode {
        std::fs::set_permissions(&plan.path, m)?;
    }
    Ok(backup)
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
