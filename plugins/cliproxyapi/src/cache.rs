//! The plugin's cache for one instance: `<state>/plugins/cliproxyapi/
//! <sha256 of the origin>.json`, 0600 in a 0700 directory, written
//! atomically, at most 64 KiB. It holds labels, `auth_index` values,
//! windows, and the failure state that keeps the plugin from hammering a
//! proxy; never an email, path, token or key (only a salted, truncated
//! digest of a refused key, with its per-install salt).

use crate::model::Acct;
use bar_plugin::fsio;
use bar_plugin::keysource;
use bar_plugin::secret::Secret;
use bar_plugin::sha256;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 64 * 1024;

/// A management 401 or 403 and the key it was for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthFail {
    pub status: u16,
    pub at: i64,
    /// Salted digest of the key that failed.
    pub fp: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    pub v: u32,
    /// Random, made at the first refusal: salts the key digest.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub salt: String,
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

impl Cache {
    /// The digest of `key` under this cache's salt (made when missing).
    pub fn fp(&mut self, key: &Secret) -> String {
        if self.salt.is_empty() {
            self.salt = keysource::new_salt();
        }
        keysource::fingerprint(key, &self.salt)
    }

    /// The refusal in force for `key` at `now`, when there is one.
    pub fn refusal(&self, key: &Secret, hold: i64, now: i64) -> Option<u16> {
        let f = self.auth_fail.as_ref()?;
        (!self.salt.is_empty()
            && keysource::fingerprint(key, &self.salt) == f.fp
            && now - f.at < hold)
            .then_some(f.status)
    }
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

/// Saves `c`; when that fails (over the cap, no room) saves it without
/// its accounts, so the failure state is kept whatever else is lost.
/// `false` only when even that cannot be written.
pub fn save_state(path: &Path, c: &Cache) -> bool {
    if save(path, c).is_ok() {
        return true;
    }
    let mut min = c.clone();
    min.accounts.clear();
    save(path, &min).is_ok()
}

/// The lock file of an instance's cache.
pub fn lock_file(path: &Path) -> PathBuf {
    path.with_extension("lock")
}

/// How long a process waits for another's call to finish.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Takes the instance lock (its directory is made first). `None` when
/// it cannot be had: another process is mid-call, or the state directory
/// is unwritable. Nothing may be sent to the proxy then, since a refusal
/// could not be remembered.
pub fn lock(path: &Path) -> Option<fsio::Lock> {
    fsio::private_dir(path.parent()?).ok()?;
    fsio::lock(&lock_file(path), LOCK_WAIT).ok().flatten()
}

/// Can the cache be written? (`bar check` asks.)
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
    fn a_refusal_survives_a_cache_too_big_to_save() {
        let d = std::env::temp_dir().join(format!("cpa-cache-big-{}", std::process::id()));
        let p = file(&d, "http://127.0.0.1:2");
        let a = |i: usize| Acct {
            key: format!("ix-{i}"),
            provider: "claude".into(),
            label: "x".repeat(24),
            priority: 0,
            plan: None,
            windows: vec![],
            at: 0,
            disabled: false,
            unavailable: false,
            upstream_auth: false,
            retry_at: 0,
            fails: 0,
        };
        let mut c = Cache {
            accounts: (0..2000).map(a).collect(),
            ..Default::default()
        };
        let key = Secret::new(b"k1".to_vec());
        c.auth_fail = Some(AuthFail {
            status: 401,
            at: 5,
            fp: c.fp(&key),
        });
        assert!(save(&p, &c).is_err());
        assert!(save_state(&p, &c));
        let back = load(&p);
        assert!(back.accounts.is_empty());
        assert_eq!(back.refusal(&key, 100, 6), Some(401));
        assert_eq!(back.refusal(&Secret::new(b"k2".to_vec()), 100, 6), None);
        assert_eq!(back.refusal(&key, 100, 500), None);
        let _ = std::fs::remove_dir_all(&d);
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
