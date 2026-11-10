//! bar's config and state directories, resolved as bar resolves them: the
//! XDG variable when absolute, else the platform default (macOS
//! `~/Library/Application Support`, Windows `%LOCALAPPDATA%`, other Unix
//! `~/.config` and `~/.local/state`).

use std::path::PathBuf;

fn abs_env(name: &str) -> Option<PathBuf> {
    let v = std::env::var_os(name)?;
    let p = PathBuf::from(v);
    p.is_absolute().then_some(p)
}

/// The home directory (`USERPROFILE` on Windows, else `HOME`) when set
/// and absolute.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    abs_env(var)
}

fn platform_home(unix: &[&str]) -> PathBuf {
    let home = home().unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else if cfg!(windows) {
        abs_env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"))
    } else {
        unix.iter().fold(home, |p, s| p.join(s))
    }
}

/// `$XDG_CONFIG_HOME/<tool>` or the platform default.
pub fn config(tool: &str) -> PathBuf {
    abs_env("XDG_CONFIG_HOME")
        .unwrap_or_else(|| platform_home(&[".config"]))
        .join(tool)
}

/// bar's state directory: `BAR_STATE_DIR` (set by bar for its background
/// jobs) when absolute, else `$XDG_STATE_HOME/bar` or the platform default.
pub fn bar_state() -> PathBuf {
    abs_env("BAR_STATE_DIR").unwrap_or_else(|| {
        abs_env("XDG_STATE_HOME")
            .unwrap_or_else(|| platform_home(&[".local", "state"]))
            .join("bar")
    })
}
