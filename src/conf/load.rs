//! Where the user config lives, and loading it with the project config.

use super::{parse_layers, project, ConfError, LoadResult};
use std::path::{Path, PathBuf};

/// Candidate paths, first existing wins.
pub fn search_paths(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Vec<PathBuf> {
    let mut v = Vec::new();
    // the XDG spec says relative values are to be ignored
    if let Some(x) = env("XDG_CONFIG_HOME").filter(|s| Path::new(s).is_absolute()) {
        v.push(PathBuf::from(x).join("bar/bar.conf"));
    }
    v.push(home.join(".config/bar/bar.conf"));
    v.push(kit_dir.join("bar.conf"));
    v
}

/// First existing candidate, or `None`.
pub fn find_path(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Option<PathBuf> {
    search_paths(env, home, kit_dir)
        .into_iter()
        .find(|p| p.is_file())
}

/// Where user palette files live: `palettes/` beside the `bar.conf` in
/// use, else beside the first candidate path (`None` when that one is
/// relative).
pub fn palette_dir(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Option<PathBuf> {
    let conf = find_path(env, home, kit_dir)
        .or_else(|| search_paths(env, home, kit_dir).into_iter().next())?;
    let dir = conf.parent()?.join("palettes");
    dir.is_absolute().then_some(dir)
}

/// [`palette_dir`] for the process environment.
pub fn palette_dir_default() -> Option<PathBuf> {
    let home = crate::platform::home().unwrap_or_default();
    palette_dir(
        &|k| std::env::var(k).ok(),
        &home,
        &crate::platform::dirs::config(),
    )
}

/// Seam for tests: explicit env lookup, home, and kit config dir; the user
/// config only.
pub fn load_from(env: &dyn Fn(&str) -> Option<String>, home: &Path, kit_dir: &Path) -> LoadResult {
    load_layered(env, home, kit_dir, None)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The user config with the project config found from `cwd` over it. The
/// project file is only looked for when `project-config` is not `off`.
/// `set palette` finds user palettes in [`palette_dir`].
pub fn load_layered(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
    cwd: Option<&Path>,
) -> LoadResult {
    let _palettes = crate::palette::use_dir(palette_dir(env, home, kit_dir));
    let user = match find_path(env, home, kit_dir).map(|p| read(&p)) {
        None => None,
        Some(Ok(text)) => Some(text),
        Some(Err(message)) => return LoadResult::Invalid(vec![ConfError { line: 0, message }]),
    };
    let mut found = false;
    let parsed = parse_layers(user.as_deref(), |_| {
        let path = project::find(cwd?)?;
        found = true;
        Some(project::read(&path))
    });
    match parsed {
        Err(e) => LoadResult::Invalid(e.into_iter().map(|e| e.error).collect()),
        Ok(_) if user.is_none() && !found => LoadResult::Missing,
        Ok(c) => LoadResult::Ok(c),
    }
}

/// Process environment, the home dir and the kit config dir for `bar`.
pub fn find_path_default() -> Option<PathBuf> {
    let home = crate::platform::home().unwrap_or_default();
    find_path(
        &|k| std::env::var(k).ok(),
        &home,
        &crate::platform::dirs::config(),
    )
}

/// The config for a session in `cwd` (`None`: user config only).
pub fn load(cwd: Option<&Path>) -> LoadResult {
    let home = crate::platform::home().unwrap_or_default();
    load_layered(
        &|k| std::env::var(k).ok(),
        &home,
        &crate::platform::dirs::config(),
        cwd,
    )
}
