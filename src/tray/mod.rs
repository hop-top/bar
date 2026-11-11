//! `bar tray`: what the desktop tray shows and runs. `state` builds the
//! snapshot the tray reads (the view toggles, the last action, the
//! session counts and the granted plugins' menus) and writes it to the
//! state dir; `run` runs one item of that snapshot. The tray never
//! computes any of it. `install`, `process` and `agent` set the tray up,
//! start, stop and report on it.

pub mod agent;
pub mod install;
pub mod linux;
mod plugins;
pub mod process;
pub mod run;
pub mod state;
pub mod waybar;
pub mod xdg;

use std::ffi::OsStr;
use std::path::PathBuf;

pub use plugins::{granted_names, valid_name, MAX_ID, MAX_LABEL};

/// The snapshot `bar tray state` writes in the state dir, mode 0600.
pub const SNAPSHOT_FILE: &str = "tray-menu.json";

/// Where the tray's subcommands look for plugins: `PATH`, the dir of
/// `bar` and the usual install dirs, since a desktop process starts
/// `bar` with a minimal `PATH`.
pub fn handler_plugin_dirs(path: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs = crate::plugin::plugin_dirs(path);
    for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let d = PathBuf::from(d);
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}
