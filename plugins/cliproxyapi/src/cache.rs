//! The plugin's cache for one instance: `<state>/plugins/cliproxyapi/
//! <sha256 of the origin>.json`, 0600 in a 0700 directory, written
//! atomically, at most 64 KiB. It holds labels, `auth_index` values,
//! windows, and the failure state that keeps the plugin from hammering a
//! proxy; never an email, path, token or key (only a key's fingerprint).

use crate::model::Acct;
use bar_plugin::fsio;
use bar_plugin::sha256;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 64 * 1024;

/// A management 401 or 403 and the key it was for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthFail {
    pub status: u16,
    pub at: i64,
    /// SHA-256 of the key that failed.
    pub fp: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    pub v: u32,
    /// `v8` or `v0`: the management API the proxy answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_fail: Option<AuthFail>,
    /// Management API disabled (404): no call before this time.
    #[serde(default)]
    pub off_until: i64,
    /// Transient failure: no call before this time.
    #[serde(default)]
    pub retry_at: i64,
    #[serde(default)]
    pub fails: u32,
    #[serde(default)]
    pub accounts: Vec<Acct>,
}

pub fn file(state_dir: &Path, origin: &str) -> PathBuf {
    state_dir
        .join("plugins")
        .join("cliproxyapi")
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

/// Backoff after `fails` consecutive failures: 60 s, doubling, 1 h at most.
pub fn backoff(fails: u32) -> i64 {
    (60i64 << fails.saturating_sub(1).min(6)).min(3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_an_hour() {
        let v: Vec<i64> = (1..=8).map(backoff).collect();
        assert_eq!(v, [60, 120, 240, 480, 960, 1920, 3600, 3600]);
        assert_eq!(backoff(0), 60);
    }

    #[test]
    fn round_trip_and_garbage() {
        let d = std::env::temp_dir().join(format!("cpa-cache-{}", std::process::id()));
        let p = file(&d, "http://127.0.0.1:1");
        assert_eq!(load(&p), Cache::default());
        let c = Cache {
            api: Some("v8".into()),
            fails: 2,
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
