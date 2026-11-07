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
/// search path already holds a config. Returns the written path.
pub fn init(
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Result<PathBuf, String> {
    let body = get(name).ok_or_else(|| format!("unknown preset `{name}`"))?;
    if let Some(p) = crate::conf::find_path(env, home, kit_dir) {
        return Err(format!("{} exists; not overwritten", p.display()));
    }
    let path = crate::conf::search_paths(env, home, kit_dir).remove(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // create_new: a file appearing since the check is not overwritten either
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(body.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}
