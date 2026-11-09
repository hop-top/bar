//! The renderer's state dir, published for the `bar://` handler: a
//! symlink `state-dir` in the platform default state dir, pointing at the
//! dir `bar render` uses when that is elsewhere (`XDG_STATE_HOME`,
//! `BAR_STATE_DIR`). `bar toggle` and `bar click run` follow it when
//! their own environment resolves to the default, so a handler started
//! without the shell's environment still finds the renderer's toggles.
//!
//! Cost on the render path: one `readlink` (or `lstat`) when the handler
//! is installed; a write only when the dir changed. Two renderers with
//! different dirs would fight over it, so a pointer changed in the last
//! minute is left alone.

use super::MARKER;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const LINK: &str = "state-dir";

/// A pointer younger than this is not replaced.
const SETTLE: Duration = Duration::from_secs(60);

/// The macOS default state dir (`~/Library/Application Support/bar`),
/// where the pointer lives; `None` without an absolute home.
pub fn default_dir() -> Option<PathBuf> {
    let home = crate::platform::home().filter(|h| h.is_absolute())?;
    Some(home.join("Library/Application Support/bar"))
}

/// The pointer's target, when it is absolute and still holds the handler
/// marker (an install happened there and renders use it).
pub fn read(default: &Path) -> Option<PathBuf> {
    let t = std::fs::read_link(default.join(LINK)).ok()?;
    (t.is_absolute() && t.join(MARKER).is_file()).then_some(t)
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
    if !state.is_absolute() || std::fs::read_link(&link).is_ok_and(|t| t == state) {
        return false;
    }
    if !state.join(MARKER).is_file() || !settled(&link, now) {
        return false;
    }
    replace(state, default, &link).is_ok()
}

#[cfg(unix)]
fn replace(state: &Path, default: &Path, link: &Path) -> std::io::Result<()> {
    if !default.is_dir() {
        crate::platform::private_dir(default)?;
    }
    let tmp = default.join(format!(".{LINK}.{}.tmp", crate::cache::nonce()));
    std::os::unix::fs::symlink(state, &tmp)?;
    std::fs::rename(&tmp, link).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The handler is macOS-only; no pointer elsewhere.
#[cfg(not(unix))]
fn replace(_: &Path, _: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}
