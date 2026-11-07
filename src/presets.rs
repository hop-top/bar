//! Example configs shipped in the binary, and `bar init`.

use std::io::Write;
use std::path::{Path, PathBuf};

pub const NAMES: [&str; 5] = ["minimal", "full", "powerline", "capsule", "compact"];

/// Preset `bar init` writes when none is named.
pub const DEFAULT: &str = "full";

pub fn get(name: &str) -> Option<&'static str> {
    Some(match name {
        "minimal" => include_str!("../presets/minimal.conf"),
        "full" => include_str!("../presets/full.conf"),
        "powerline" => include_str!("../presets/powerline.conf"),
        "capsule" => include_str!("../presets/capsule.conf"),
        "compact" => include_str!("../presets/compact.conf"),
        _ => return None,
    })
}

/// Writes preset `name` to the first config search path; refuses when any
/// search path already holds a config, or when the path would be relative
/// (`HOME` unset or relative). Returns the written path.
pub fn init(
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Result<PathBuf, String> {
    let body = get(name).ok_or_else(|| format!("unknown preset `{name}`"))?;
    let path = crate::conf::search_paths(env, home, kit_dir).remove(0);
    if !path.is_absolute() {
        return Err("HOME is unset or relative; set it or XDG_CONFIG_HOME".into());
    }
    if let Some(p) = crate::conf::find_path(env, home, kit_dir) {
        return Err(format!("{} exists; not overwritten", p.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_new(&path, body)?;
    Ok(path)
}

/// Writes `body` to `path` all at once and never over an existing file:
/// a temp file in the same directory, hard-linked to `path` (which fails
/// when `path` exists), then removed.
pub fn write_new(path: &Path, body: &str) -> Result<(), String> {
    let shown = |e: std::io::Error| format!("{}: {e}", path.display());
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bar.conf"),
        std::process::id()
    ));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        std::fs::hard_link(&tmp, path)
    })();
    let _ = std::fs::remove_file(&tmp);
    written.map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("{} exists; not overwritten", path.display()),
        _ => shown(e),
    })
}
