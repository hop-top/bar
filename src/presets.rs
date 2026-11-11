//! Example configs shipped in the binary, and `bar init`.

use std::io::Write;
use std::path::{Path, PathBuf};

pub const NAMES: [&str; 6] = [
    "minimal",
    "full",
    "powerline",
    "capsule",
    "compact",
    "two-line",
];

/// Preset `bar init` writes when none is named.
pub const DEFAULT: &str = "full";

pub fn get(name: &str) -> Option<&'static str> {
    Some(match name {
        "minimal" => include_str!("../presets/minimal.conf"),
        "full" => include_str!("../presets/full.conf"),
        "powerline" => include_str!("../presets/powerline.conf"),
        "capsule" => include_str!("../presets/capsule.conf"),
        "compact" => include_str!("../presets/compact.conf"),
        "two-line" => include_str!("../presets/two-line.conf"),
        _ => return None,
    })
}

/// How [`init`] ends the message of a refusal to overwrite a config.
pub const EXISTS: &str = "exists; not overwritten";

/// The variable the home directory comes from, for messages.
const HOME_VAR: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

/// What `bar init` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Init {
    /// Wrote the preset to this path.
    Wrote(PathBuf),
    /// The config at this path is the preset already; nothing written.
    Same(PathBuf),
}

/// Writes preset `name` to the first config search path. A config that
/// already holds exactly that preset is left as is ([`Init::Same`]); any
/// other existing config is refused, as is a relative path (home unset or
/// relative).
pub fn init(
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Result<Init, String> {
    let body = get(name).ok_or_else(|| format!("unknown preset `{name}`"))?;
    let path = crate::conf::search_paths(env, home, kit_dir).remove(0);
    if !path.is_absolute() {
        return Err(format!(
            "{HOME_VAR} is unset or relative; set it or XDG_CONFIG_HOME"
        ));
    }
    if let Some(p) = crate::conf::find_path(env, home, kit_dir) {
        if std::fs::read(&p).is_ok_and(|b| b == body.as_bytes()) {
            return Ok(Init::Same(p));
        }
        return Err(format!("{} {EXISTS}", p.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_new(&path, body)?;
    Ok(Init::Wrote(path))
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
        std::io::ErrorKind::AlreadyExists => format!("{} {EXISTS}", path.display()),
        _ => shown(e),
    })
}
