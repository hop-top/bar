//! Clickable status line: usage toggles, slash commands typed into the
//! session, and the macOS `bar://` URL handler that opens them.

pub mod action;
mod check;
mod dirs;
pub mod hook;
mod install;
pub mod pointer;
pub(crate) mod settings;
pub mod sys;
pub mod tlc;
mod url;
mod views;

pub use check::{box_is_empty, check, run, Env, Pane, Refusal};
pub use dirs::{
    follows_pointer, handler_env, handler_warnings, sh_quote, shell_command, EnvVar, HandlerDirs,
    HANDLER_ENV,
};
pub use install::{
    applescript, exec, info_plist_commands, install_plan, run_hooks_only, run_install, InstallCfg,
    InstallError, InstallPlan,
};
pub use settings::{hook_command, hook_command_for, plan_settings, SettingsPlan, HOOK_EVENTS};
pub use url::{link, parse, with_cmd_link, ClickUrl, Term, COMMANDS};
pub use views::{check_key, cli as toggle_cli, parse_value, set_in, unknown_key, view_name};

use std::io::Write;
use std::path::{Path, PathBuf};

const KEYS: [&str; 2] = ["5h", "7d"];
pub(crate) const MARKER: &str = "click-installed";
/// Marker contents written by an install whose handler routes commands.
pub(crate) const MARKER_CMD: &str = "cmd";

/// `BAR_STATE_DIR` overrides the kit's state dir (tests, relocation).
pub fn state_dir() -> PathBuf {
    std::env::var_os("BAR_STATE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(crate::platform::dirs::state)
}

/// The state dir of `bar toggle` and `bar click run`, which the `bar://`
/// handler starts: `state_dir()`, except that on macOS the platform
/// default follows the renderer's pointer (see `pointer`).
pub fn handler_state_dir() -> PathBuf {
    let base = state_dir();
    if !cfg!(target_os = "macos") {
        return base;
    }
    match pointer::default_dir() {
        Some(d) => pointer::follow(base, &d),
        None => base,
    }
}

/// Claude Code's config dir: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
/// Absolute only: a relative `CLAUDE_CONFIG_DIR` or home (or none)
/// yields `None`, never a path under the working directory.
pub fn claude_dir() -> Option<PathBuf> {
    let abs = |p: PathBuf| p.is_absolute().then_some(p);
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => abs(PathBuf::from(d)),
        None => Some(abs(crate::platform::home()?)?.join(".claude")),
    }
}

/// Toggle state file: `pct-5h` / `pct-7d` for usage, `toggle-<name>` for
/// a plugin's text and `toggle-<name>.<key>` for one of its named toggles;
/// `None` for a key that is none of these.
fn toggle_file(dir: &Path, key: &str) -> Option<PathBuf> {
    if KEYS.contains(&key) {
        Some(dir.join(format!("pct-{key}")))
    } else {
        let ok = match key.split_once('.') {
            Some((name, sub)) => hook::valid_plugin_name(name) && hook::valid_plugin_name(sub),
            None => hook::valid_plugin_name(key),
        };
        ok.then(|| dir.join(format!("toggle-{key}")))
    }
}

/// Whether `key` is a valid toggle name for a plugin's label or icon.
pub fn valid_toggle_name(key: &str) -> bool {
    hook::valid_plugin_name(key)
}

/// The named toggles of `plugin` that are on, from `toggle-<plugin>.<key>`
/// files; off toggles have no file, so none is listed as `false`.
/// Session `sid`'s own entries override the global files.
pub fn plugin_toggles(
    dir: &Path,
    plugin: &str,
    sid: &str,
) -> std::collections::BTreeMap<String, bool> {
    let mut out = std::collections::BTreeMap::new();
    let prefix = format!("toggle-{plugin}.");
    if !hook::valid_plugin_name(plugin) {
        return out;
    }
    let keys = |d: &Path| -> Vec<(String, std::ffi::OsString)> {
        let Ok(rd) = std::fs::read_dir(d) else {
            return Vec::new();
        };
        rd.flatten()
            .filter_map(|e| {
                let name = e.file_name();
                let key = name.to_str()?.strip_prefix(&prefix)?;
                hook::valid_plugin_name(key).then(|| (key.to_string(), name.clone()))
            })
            .collect()
    };
    for (key, _) in keys(dir) {
        out.insert(key, true);
    }
    if let Some(own) = session_toggles_dir(dir, sid) {
        for (key, name) in keys(&own) {
            match session_state(dir, sid, &name) {
                Some(true) => {
                    out.insert(key, true);
                }
                Some(false) => {
                    out.remove(&key);
                }
                None => {}
            }
        }
    }
    out
}

/// The global (default) state of `key`, as `bar toggle <key>` sets it.
pub fn is_toggled(dir: &Path, key: &str) -> bool {
    toggle_file(dir, key).is_some_and(|f| f.exists())
}

/// Per-session toggle state: `<state>/toggles/<session id>/<file>`, the
/// file named as the global one, content `on` or `off`.
pub const SESSION_TOGGLES: &str = "toggles";

/// Where session `sid` keeps its own toggles; `None` for an invalid id.
pub fn session_toggles_dir(dir: &Path, sid: &str) -> Option<PathBuf> {
    url::valid_sid(sid).then(|| dir.join(SESSION_TOGGLES).join(sid))
}

/// Session `sid`'s own state of the toggle file `name`, when it has one.
fn session_state(dir: &Path, sid: &str, name: &std::ffi::OsStr) -> Option<bool> {
    let body = std::fs::read_to_string(session_toggles_dir(dir, sid)?.join(name)).ok()?;
    match body.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// `key` as session `sid` sees it: its own state when it flipped the
/// toggle, else the global one.
pub fn is_toggled_in(dir: &Path, key: &str, sid: &str) -> bool {
    let Some(f) = toggle_file(dir, key) else {
        return false;
    };
    f.file_name()
        .and_then(|n| session_state(dir, sid, n))
        .unwrap_or_else(|| f.exists())
}

/// The link that flips `key` in session `sid`: `bar://toggle/<key>?sid=<id>`,
/// or `bar://toggle/<key>` (the global toggle) without a valid id.
pub fn toggle_link(key: &str, sid: &str) -> String {
    if url::valid_sid(sid) {
        format!("bar://toggle/{key}?sid={sid}")
    } else {
        format!("bar://toggle/{key}")
    }
}

/// `bar toggle` argument: `<key>`, or `<key>?sid=<session id>` as the
/// handler passes the rest of a `bar://toggle/` link.
pub fn toggle_target(arg: &str) -> Result<(&str, Option<&str>), String> {
    let Some((key, query)) = arg.split_once('?') else {
        return Ok((arg, None));
    };
    match query.strip_prefix("sid=").filter(|s| url::valid_sid(s)) {
        Some(sid) => Ok((key, Some(sid))),
        None => Err(format!(
            "bad toggle target `{arg}` (expected <key>?sid=<session id>)"
        )),
    }
}

/// The install step writes this; links need a handler to open them.
pub fn handler_installed(dir: &Path) -> bool {
    dir.join(MARKER).exists()
}

/// The handler also routes `bar://cmd/` (the marker says `cmd`); a marker
/// from a toggle-only install keeps command links off.
pub fn cmd_handler_installed(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(MARKER)).is_ok_and(|s| s.trim() == MARKER_CMD)
}

pub(crate) fn write_private(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut o = std::fs::OpenOptions::new();
    crate::platform::private_file(o.write(true).create(true).truncate(true));
    o.open(path)?.write_all(body.as_bytes())
}

/// Writes a sibling temp file (0600) and renames it over `path`
/// (`MoveFileEx` with replace on Windows, also atomic on one volume).
pub(crate) fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", crate::cache::nonce()));
    write_private(&tmp, body)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Whether `key` toggles usage (`5h`, `7d`) rather than a plugin.
pub fn is_usage_key(key: &str) -> bool {
    KEYS.contains(&key)
}

/// Flips the toggle of `key`: `5h` or `7d` (usage), else a plugin name;
/// `Ok(true)` when the alternate view is now on.
pub fn toggle(dir: &Path, key: &str) -> Result<bool, String> {
    let on = !is_toggled(dir, key);
    set_in(dir, key, None, on)?;
    Ok(on)
}

/// Session toggle dirs untouched this long are pruned on the next flip.
pub const SESSION_TOGGLES_STALE_SECS: i64 = hook::RECORD_STALE_SECS;

/// Flips `key` for session `sid` only (see [`is_toggled_in`]), or the
/// global toggle without a session; `Ok(true)` when the alternate view is
/// now on in that session.
pub fn toggle_in(dir: &Path, key: &str, sid: Option<&str>) -> Result<bool, String> {
    let on = match sid {
        Some(sid) => !is_toggled_in(dir, key, sid),
        None => !is_toggled(dir, key),
    };
    set_in(dir, key, sid, on)?;
    Ok(on)
}

/// Drops other sessions' toggle dirs older than
/// [`SESSION_TOGGLES_STALE_SECS`]; bounded work.
fn prune_sessions(root: &Path, keep: &str) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let stale = std::time::Duration::from_secs(SESSION_TOGGLES_STALE_SECS as u64);
    for e in rd.flatten().take(256) {
        let (path, name) = (e.path(), e.file_name());
        let Some(sid) = name.to_str().filter(|n| *n != keep && url::valid_sid(n)) else {
            continue;
        };
        let old = std::fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.is_dir())
            .and_then(|m| m.modified().ok())
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= stale);
        if old {
            let _ = std::fs::remove_dir_all(root.join(sid));
        }
    }
}
