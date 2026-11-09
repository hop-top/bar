//! Claude Code's settings files, read for `ctx` and `modes`. Reads are
//! capped, and the user file is parsed at most once per process.

use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

/// Larger settings files are not read.
pub const MAX_SETTINGS: u64 = 64 * 1024;

/// One settings file, parsed; `None` when missing, too large or not JSON.
pub fn read(path: &Path) -> Option<Value> {
    let mut buf = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_SETTINGS + 1)
        .read_to_string(&mut buf)
        .ok()?;
    (buf.len() as u64 <= MAX_SETTINGS).then_some(())?;
    serde_json::from_str(&buf).ok()
}

/// `settings.json` in the Claude config dir, read on first use.
pub fn user() -> Option<&'static Value> {
    static USER: OnceLock<Option<Value>> = OnceLock::new();
    USER.get_or_init(|| read(&crate::click::claude_dir()?.join("settings.json")))
        .as_ref()
}
