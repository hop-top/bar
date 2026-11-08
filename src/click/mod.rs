//! Clickable status line: usage toggles, slash commands typed into the
//! session, and the macOS `bar://` URL handler that opens them.

mod check;
pub mod hook;
mod install;
mod settings;
pub mod sys;
pub mod tlc;
mod url;

pub use check::{box_is_empty, check, run, Env, Pane, Refusal};
pub use install::{
    applescript, exec, info_plist_commands, install_plan, run_hooks_only, run_install, InstallCfg,
    InstallError, InstallPlan,
};
pub use settings::{hook_command, hook_command_for, plan_settings, SettingsPlan, HOOK_EVENTS};
pub use url::{link, parse, with_cmd_link, ClickUrl, Term, COMMANDS};

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
        .unwrap_or_else(|| hop_top_kit::xdg::raw_state_dir("bar"))
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
pub fn plugin_toggles(dir: &Path, plugin: &str) -> std::collections::BTreeMap<String, bool> {
    let mut out = std::collections::BTreeMap::new();
    let prefix = format!("toggle-{plugin}.");
    if !hook::valid_plugin_name(plugin) {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(key) = name.to_str().and_then(|n| n.strip_prefix(&prefix)) else {
            continue;
        };
        if hook::valid_plugin_name(key) {
            out.insert(key.to_string(), true);
        }
    }
    out
}

pub fn is_toggled(dir: &Path, key: &str) -> bool {
    toggle_file(dir, key).is_some_and(|f| f.exists())
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
    let f = toggle_file(dir, key).ok_or_else(|| {
        format!("unknown key `{key}` (expected 5h, 7d, a plugin name or <plugin>.<toggle>)")
    })?;
    if f.exists() {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
        return Ok(false);
    }
    write_private(&f, "").map_err(|e| e.to_string())?;
    Ok(true)
}
