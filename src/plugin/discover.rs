//! `bar-<name>` executables on a `PATH`-style list of directories.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{CORE, PREFIX};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Placeholder name: the file name without the prefix.
    pub name: String,
    pub path: PathBuf,
}

/// Absolute entries of a `PATH`-style list, in order; relative entries
/// would make the plugin set depend on the working directory.
pub fn path_dirs(path: Option<&OsStr>) -> Vec<PathBuf> {
    path.map(|p| {
        std::env::split_paths(p)
            .filter(|d| d.is_absolute())
            .collect()
    })
    .unwrap_or_default()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !CORE.contains(&name)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Every plugin in `dirs`, sorted by name; per name the first executable
/// in `dirs` order wins. Core names and non-executables are skipped.
pub fn discover(dirs: &[PathBuf]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let file = e.file_name();
            let Some(name) = file.to_str().and_then(|f| f.strip_prefix(PREFIX)) else {
                continue;
            };
            let path = dir.join(&file);
            if valid_name(name)
                && path.to_str().is_some()
                && !out.iter().any(|f| f.name == name)
                && is_executable(&path)
            {
                out.push(Found {
                    name: name.to_string(),
                    path,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The plugin `discover` would pick for `name`, with one stat per dir.
pub fn find(dirs: &[PathBuf], name: &str) -> Option<Found> {
    if !valid_name(name) {
        return None;
    }
    dirs.iter()
        .map(|d| d.join(format!("{PREFIX}{name}")))
        .find(|p| p.to_str().is_some() && is_executable(p))
        .map(|path| Found {
            name: name.to_string(),
            path,
        })
}
