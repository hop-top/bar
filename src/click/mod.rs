//! Clickable status line: usage toggles, slash commands typed into the
//! session, and the macOS `bar://` URL handler that opens them.

mod check;
pub mod hook;
mod install;
mod settings;
pub mod sys;
mod url;

pub use check::{box_is_empty, check, run, Env, Pane, Refusal};
pub use install::{
    applescript, exec, info_plist_commands, install_plan, run_hooks_only, run_install, InstallCfg,
    InstallPlan, UNSUPPORTED,
};
pub use settings::{hook_command, plan_settings, SettingsPlan, HOOK_EVENTS};
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
pub fn claude_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => Some(PathBuf::from(std::env::var_os("HOME")?).join(".claude")),
    }
}

pub fn is_toggled(dir: &Path, key: &str) -> bool {
    dir.join(format!("pct-{key}")).exists()
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
    o.write(true).create(true).truncate(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(body.as_bytes())
}

/// Writes a sibling temp file (0600) and renames it over `path`.
pub(crate) fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", crate::cache::nonce()));
    write_private(&tmp, body)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Flips `pct-<key>`; `Ok(true)` when the percentage view is now on.
pub fn toggle(dir: &Path, key: &str) -> Result<bool, String> {
    if !KEYS.contains(&key) {
        return Err(format!("unknown key `{key}` (expected 5h or 7d)"));
    }
    let f = dir.join(format!("pct-{key}"));
    if f.exists() {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
        return Ok(false);
    }
    write_private(&f, "").map_err(|e| e.to_string())?;
    Ok(true)
}
