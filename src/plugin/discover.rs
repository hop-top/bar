//! `bar-<name>` executables on a `PATH`-style list of directories; on
//! Windows `bar-<name>` plus a runnable `PATHEXT` extension (`.exe`,
//! `.cmd`, ...).

use std::ffi::OsStr;
use std::path::PathBuf;

use super::PREFIX;
use crate::placeholders::lookup;
use crate::platform;

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

/// [`path_dirs`] plus the directory holding the running `bar` (symlinks
/// resolved), last: plugins shipped beside `bar` are found without being
/// on `PATH`, and a copy on `PATH` wins.
pub fn plugin_dirs(path: Option<&OsStr>) -> Vec<PathBuf> {
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    plugin_dirs_with(path, exe.as_deref())
}

/// As [`plugin_dirs`] for a given executable path (`None`: none added).
pub fn plugin_dirs_with(path: Option<&OsStr>, exe: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = path_dirs(path);
    if let Some(dir) = exe.and_then(|e| e.parent()).filter(|d| d.is_absolute()) {
        if !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && lookup(name).is_none()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Every plugin in `dirs`, sorted by name; per name the first executable
/// in `dirs` order wins, and within one dir the earliest `PATHEXT`
/// extension (Windows). Core names and non-executables are skipped.
pub fn discover(dirs: &[PathBuf]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut here: Vec<(usize, Found)> = Vec::new();
        for e in entries.flatten() {
            let file = e.file_name();
            let Some((stem, rank)) = file.to_str().and_then(platform::command_stem) else {
                continue;
            };
            let Some(name) = stem.strip_prefix(PREFIX) else {
                continue;
            };
            let path = dir.join(&file);
            if !valid_name(name)
                || path.to_str().is_none()
                || out.iter().any(|f| f.name == name)
                || !platform::is_executable(&path)
            {
                continue;
            }
            let found = Found {
                name: name.to_string(),
                path,
            };
            match here.iter_mut().find(|(_, f)| f.name == name) {
                Some(slot) if slot.0 > rank => *slot = (rank, found),
                Some(_) => {}
                None => here.push((rank, found)),
            }
        }
        out.extend(here.into_iter().map(|(_, f)| f));
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The plugin `discover` would pick for `name`, with one stat per dir
/// (per `PATHEXT` extension on Windows).
pub fn find(dirs: &[PathBuf], name: &str) -> Option<Found> {
    if !valid_name(name) {
        return None;
    }
    let file = format!("{PREFIX}{name}");
    dirs.iter()
        .filter_map(|d| platform::executable_in(d, &file))
        .find(|p| p.to_str().is_some())
        .map(|path| Found {
            name: name.to_string(),
            path,
        })
}
