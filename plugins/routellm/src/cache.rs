//! The plugin's cache for one instance: `<state>/plugins/routellm/<sha256
//! of the origin>.json`, 0600 in a 0700 directory, written atomically, at
//! most 64 KiB. It holds the toggle's state, alias and tier names, tier
//! capabilities, the last change and the failure timers; never a key, a
//! credential or header of the proxy's payload, an email or a prompt.
//! A refused key is remembered in the guard shared with other plugins.

use bar_plugin::fsio;
use bar_plugin::keyguard::{Guard, Held};
use bar_plugin::sha256;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 64 * 1024;

/// An alias of the provider and the RouteLLM tier behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alias {
    pub alias: String,
    pub tier: String,
}

/// The provider entry as the proxy holds it (secret fields never parsed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub disabled: bool,
    pub priority: i64,
    pub aliases: Vec<Alias>,
}

/// A `route-*` alias of the proxy's claude channel: what OFF serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub alias: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tier {
    pub id: String,
    pub kind: String,
    pub vision: Option<bool>,
    pub tools: Option<bool>,
    pub context: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub at: i64,
    pub from: String,
    pub to: String,
    pub version: String,
}

/// Why the entry cannot be driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Problem {
    /// The proxy has no entry of that name.
    Unconfigured,
    /// The entry's `base-url` is not the configured RouteLLM.
    Mismatch,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<Problem>,
    #[serde(default)]
    pub rows: Vec<Row>,
    /// When the proxy was last read.
    #[serde(default)]
    pub at: i64,
    /// RouteLLM answered `/health` (`None`: not asked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<bool>,
    #[serde(default)]
    pub up_at: i64,
    #[serde(default)]
    pub tiers: Vec<Tier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Change>,
    /// Management API off (404): no call before this time.
    #[serde(default)]
    pub off_until: i64,
    /// Transient failure: no call before this time.
    #[serde(default)]
    pub retry_at: i64,
    #[serde(default)]
    pub fails: u32,
}

pub fn file(state_dir: &Path, origin: &str) -> PathBuf {
    state_dir
        .join("plugins")
        .join("routellm")
        .join(format!("{}.json", sha256::hex(origin.as_bytes())))
}

/// The cache at `path`; empty when absent, unreadable, over the cap or
/// not ours.
pub fn load(path: &Path) -> Cache {
    let Ok(f) = std::fs::File::open(path) else {
        return Cache::default();
    };
    let Ok(raw) = fsio::read_capped(f, MAX_BYTES) else {
        return Cache::default();
    };
    serde_json::from_slice(&raw).unwrap_or_default()
}

pub fn save(path: &Path, c: &Cache) -> std::io::Result<()> {
    let mut c = c.clone();
    c.v = 1;
    let json = serde_json::to_vec(&c).map_err(std::io::Error::other)?;
    if json.len() > MAX_BYTES {
        return Err(std::io::Error::other("cache over 64 KiB"));
    }
    if let Some(dir) = path.parent() {
        fsio::private_dir(dir)?;
    }
    fsio::write_atomic_private(path, &json)
}

/// Saves `c`; when that is too big, without the tiers.
pub fn save_state(path: &Path, c: &Cache) -> bool {
    if save(path, c).is_ok() {
        return true;
    }
    let mut min = c.clone();
    min.tiers.clear();
    save(path, &min).is_ok()
}

pub fn lock_file(path: &Path) -> PathBuf {
    path.with_extension("lock")
}

/// The management origin's lock (shared with other plugins) and this
/// instance's. `None`: nothing may be sent.
pub fn lock(g: &Guard, path: &Path) -> Option<Held> {
    g.lock(Some(&lock_file(path)))
}

/// This instance's lock alone, for writing what was learned outside the
/// proxy's lock (RouteLLM's answers). `None` when it is not free within
/// `wait` or cannot be made.
pub fn lock_own(path: &Path, wait: std::time::Duration) -> Option<fsio::Lock> {
    let lf = lock_file(path);
    fsio::private_dir(lf.parent()?).ok()?;
    fsio::lock(&lf, wait).ok().flatten()
}

pub fn writable(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    if fsio::private_dir(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".probe.{}", std::process::id()));
    let ok = fsio::write_atomic_private(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_garbage() {
        let d = std::env::temp_dir().join(format!("rl-cache-{}", std::process::id()));
        let p = file(&d, "http://127.0.0.1:1");
        assert_eq!(load(&p), Cache::default());
        let c = Cache {
            fails: 2,
            up: Some(true),
            ..Default::default()
        };
        save(&p, &c).unwrap();
        assert_eq!(load(&p).fails, 2);
        std::fs::write(&p, "not json").unwrap();
        assert_eq!(load(&p), Cache::default());
        std::fs::write(&p, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert_eq!(load(&p), Cache::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
