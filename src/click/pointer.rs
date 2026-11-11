//! The renderer's state dir, published for the `bar://` handler and the
//! tray: a symlink `state-dir` (a plain file holding the path on Windows,
//! where symlinks need a privilege) in the platform default state dir,
//! pointing at the dir `bar render` uses when that is elsewhere
//! (`XDG_STATE_HOME`, `BAR_STATE_DIR`). `bar toggle`, `bar click run` and
//! the tray follow it when their own environment resolves to the default,
//! so a process started without the shell's environment still finds the
//! renderer's toggles. Published only for a dir holding an install marker
//! (`click-installed` or `tray-installed`).
//!
//! Cost on the render path: one `readlink` (or `lstat`) per render; a
//! write only when the dir changed. Two renderers with different dirs
//! would fight over it, so a pointer changed in the last minute is left
//! alone.

use super::MARKER;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const LINK: &str = "state-dir";

/// Marker of an installed tray.
pub(crate) const TRAY_MARKER: &str = "tray-installed";

/// Whether `dir` holds an install marker.
fn marked(dir: &Path) -> bool {
    dir.join(MARKER).is_file() || dir.join(TRAY_MARKER).is_file()
}

/// A pointer younger than this is not replaced.
const SETTLE: Duration = Duration::from_secs(60);

/// The platform default state dir with the XDG override ignored, where
/// the pointer lives: `~/Library/Application Support/bar` on macOS,
/// `%LOCALAPPDATA%\bar` on Windows, `~/.local/state/bar` elsewhere;
/// `None` without an absolute home.
pub fn default_dir() -> Option<PathBuf> {
    let home = crate::platform::home().filter(|h| h.is_absolute())?;
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support/bar"))
    } else if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        Some(local.join("bar"))
    } else {
        Some(home.join(".local/state/bar"))
    }
}

/// The pointer at `link`: a symlink's target, else (a plain file, as on
/// Windows) its first line. A missing pointer costs one syscall.
fn target(link: &Path) -> Option<PathBuf> {
    match std::fs::read_link(link) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            let s = std::fs::read_to_string(link).ok()?;
            let line = s.lines().next()?.trim();
            (!line.is_empty()).then(|| PathBuf::from(line))
        }
    }
}

/// The pointer's target, when it is absolute and holds an install marker
/// (an install happened there and renders use it).
pub fn read(default: &Path) -> Option<PathBuf> {
    let t = target(&default.join(LINK))?;
    (t.is_absolute() && marked(&t)).then_some(t)
}

/// `base` itself, unless it is `default`: then the pointer's target, if
/// any.
pub fn follow(base: PathBuf, default: &Path) -> PathBuf {
    if base == default {
        read(default).unwrap_or(base)
    } else {
        base
    }
}

fn settled(link: &Path, now: SystemTime) -> bool {
    std::fs::symlink_metadata(link)
        .and_then(|m| m.modified())
        .map_or(true, |t| now.duration_since(t).is_ok_and(|d| d >= SETTLE))
}

/// Points `default/state-dir` at `state`, or removes it when `state` is
/// the default; `true` when it changed. Only for a `state` holding the
/// handler marker; errors leave things as they were.
pub fn sync(state: &Path, default: &Path, now: SystemTime) -> bool {
    let link = default.join(LINK);
    if state == default {
        let present = std::fs::symlink_metadata(&link).is_ok();
        return present && settled(&link, now) && std::fs::remove_file(&link).is_ok();
    }
    if !state.is_absolute() || target(&link).is_some_and(|t| t == state) {
        return false;
    }
    if !marked(state) || !settled(&link, now) {
        return false;
    }
    replace(state, default, &link).is_ok()
}

#[cfg(unix)]
fn publish(state: &Path, tmp: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(state, tmp)
}

/// A plain file holding the path: symlinks need a privilege on Windows.
#[cfg(not(unix))]
fn publish(state: &Path, tmp: &Path) -> std::io::Result<()> {
    std::fs::write(tmp, state.as_os_str().as_encoded_bytes())
}

fn replace(state: &Path, default: &Path, link: &Path) -> std::io::Result<()> {
    if !default.is_dir() {
        crate::platform::private_dir(default)?;
    }
    let tmp = default.join(format!(".{LINK}.{}.tmp", crate::cache::nonce()));
    publish(state, &tmp)?;
    std::fs::rename(&tmp, link).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
