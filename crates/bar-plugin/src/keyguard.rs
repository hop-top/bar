//! Ban-safety shared by every plugin that talks to one proxy's management
//! API. The proxy bans a client address after a few failed attempts, so
//! all plugins and processes of one machine must act as one client: they
//! take one lock per management origin, send nothing with a key that was
//! refused until the key changes (or a day passes), never send an empty
//! key and never send when a refusal could not be remembered.
//!
//! State lives under `<state>/plugins/mgmt/`, one `<sha256 of the
//! origin>.json` (and `.lock`) per management origin: a random salt and
//! the salted digest of the refused key, never the key. A refusal that an
//! older `bar-cliproxyapi` cache holds for the same origin is honoured too.

use crate::fsio;
use crate::keysource;
use crate::secret::Secret;
use crate::sha256;
use crate::url::Origin;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A refused key is not retried for this long (or until it changes).
pub const AUTH_HOLD: i64 = 24 * 3600;
/// How long a process waits for another's call to finish.
pub const LOCK_WAIT: Duration = Duration::from_secs(8);
const MAX_BYTES: usize = 8 * 1024;
/// Refused keys remembered per origin: several plugins may use different
/// keys against one proxy, and each refusal must outlive the others'.
pub const MAX_FAILS: usize = 4;

/// A management 401 or 403 and the key it was for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthFail {
    pub status: u16,
    pub at: i64,
    /// Salted digest of the key that failed.
    pub fp: String,
}

/// What is remembered about one management origin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub v: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub salt: String,
    /// One refusal per refused key, oldest first, at most `MAX_FAILS`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fails: Vec<AuthFail>,
    /// The single refusal of the older record format (and of an older
    /// `bar-cliproxyapi` cache): read, folded into `fails` on the next write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_fail: Option<AuthFail>,
}

impl Record {
    /// The refusal in force for `key` at `now`, when there is one.
    pub fn refusal(&self, key: &Secret, hold: i64, now: i64) -> Option<u16> {
        if self.salt.is_empty() {
            return None;
        }
        let fp = keysource::fingerprint(key, &self.salt);
        self.fails
            .iter()
            .chain(self.auth_fail.as_ref())
            .find(|f| f.fp == fp && now - f.at < hold)
            .map(|f| f.status)
    }
}

/// Where a management origin's guard state is kept.
#[derive(Debug, Clone, Copy)]
pub struct Guard<'a> {
    pub state_dir: &'a Path,
    /// Any URL on the management origin (the management base).
    pub mgmt: &'a str,
}

fn origin_of(url: &str) -> String {
    Origin::parse(url).map_or_else(|| url.to_string(), |o| o.to_string())
}

/// `origin_of`, with `localhost` read as `127.0.0.1`: one proxy, one
/// failed-attempt counter, one memory.
fn guard_origin_of(url: &str) -> String {
    match Origin::parse(url) {
        Some(mut o) => {
            if o.host == "localhost" {
                o.host = "127.0.0.1".into();
            }
            o.to_string()
        }
        None => url.to_string(),
    }
}

fn load_at(path: &Path) -> Record {
    let Ok(f) = std::fs::File::open(path) else {
        return Record::default();
    };
    fsio::read_capped(f, 64 * 1024)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

impl Guard<'_> {
    pub fn file(&self) -> PathBuf {
        self.state_dir.join("plugins").join("mgmt").join(format!(
            "{}.json",
            sha256::hex(guard_origin_of(self.mgmt).as_bytes())
        ))
    }

    /// The cache `bar-cliproxyapi` kept before the guard was shared; read
    /// for its refusal only, so an upgrade costs no extra failed attempt.
    fn legacy_file(&self) -> PathBuf {
        self.state_dir
            .join("plugins")
            .join("cliproxyapi")
            .join(format!(
                "{}.json",
                sha256::hex(origin_of(self.mgmt).as_bytes())
            ))
    }

    pub fn lock_file(&self) -> PathBuf {
        self.file().with_extension("lock")
    }

    pub fn load(&self) -> Record {
        load_at(&self.file())
    }

    /// The refusal in force for `key`: the shared record's, else the
    /// older cache's.
    pub fn refusal(&self, key: &Secret, now: i64) -> Option<u16> {
        self.load()
            .refusal(key, AUTH_HOLD, now)
            .or_else(|| load_at(&self.legacy_file()).refusal(key, AUTH_HOLD, now))
    }

    /// Remembers that `key` was refused with `status`. `false` when it
    /// could not be written (the caller still stops).
    pub fn stop(&self, status: u16, key: &Secret, now: i64) -> bool {
        let mut r = self.load();
        if r.salt.is_empty() {
            r.salt = keysource::new_salt();
        }
        r.v = 1;
        let fp = keysource::fingerprint(key, &r.salt);
        r.fails.extend(r.auth_fail.take());
        r.fails.retain(|f| f.fp != fp);
        r.fails.push(AuthFail {
            status,
            at: now,
            fp,
        });
        let extra = r.fails.len().saturating_sub(MAX_FAILS);
        r.fails.drain(..extra);
        self.save(&r)
    }

    fn save(&self, r: &Record) -> bool {
        let path = self.file();
        let Ok(json) = serde_json::to_vec(r) else {
            return false;
        };
        if json.len() > MAX_BYTES {
            return false;
        }
        if let Some(dir) = path.parent() {
            if fsio::private_dir(dir).is_err() {
                return false;
            }
        }
        fsio::write_atomic_private(&path, &json).is_ok()
    }

    /// Can the guard's state be written? (`bar check` asks.)
    pub fn writable(&self) -> bool {
        let path = self.file();
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

    /// Takes the origin's lock, then `own` (a plugin's private lock file)
    /// when given. `None` when either cannot be had: another process is
    /// mid-call, or the state directory is unwritable. Nothing may be sent
    /// to the proxy then, since a refusal could not be remembered.
    pub fn lock(&self, own: Option<&Path>) -> Option<Held> {
        let lf = self.lock_file();
        fsio::private_dir(lf.parent()?).ok()?;
        let a = fsio::lock(&lf, LOCK_WAIT).ok().flatten()?;
        let b = match own {
            Some(p) => {
                fsio::private_dir(p.parent()?).ok()?;
                Some(fsio::lock(p, LOCK_WAIT).ok().flatten()?)
            }
            None => None,
        };
        Some(Held { _a: a, _b: b })
    }
}

/// The locks of one management call, released when dropped.
pub struct Held {
    _a: fsio::Lock,
    _b: Option<fsio::Lock>,
}

/// Backoff after `fails` consecutive failures: 60 s, doubling, 1 h at most.
pub fn backoff(fails: u32) -> i64 {
    (60i64 << fails.saturating_sub(1).min(6)).min(3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bar-keyguard-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn backoff_doubles_to_an_hour() {
        let v: Vec<i64> = (1..=8).map(backoff).collect();
        assert_eq!(v, [60, 120, 240, 480, 960, 1920, 3600, 3600]);
        assert_eq!(backoff(0), 60);
    }

    #[test]
    fn a_refusal_is_per_key_and_expires() {
        let d = tmp("refusal");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:1/v0/management",
        };
        let k1 = Secret::new(b"k1".to_vec());
        let k2 = Secret::new(b"k2".to_vec());
        assert_eq!(g.refusal(&k1, 0), None);
        assert!(g.stop(401, &k1, 10));
        assert_eq!(g.refusal(&k1, 11), Some(401));
        assert_eq!(g.refusal(&k2, 11), None);
        assert_eq!(g.refusal(&k1, 10 + AUTH_HOLD), None);
        let text = std::fs::read_to_string(g.file()).unwrap();
        assert!(!text.contains("k1"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plugins_on_one_origin_share_one_refusal() {
        let d = tmp("share");
        // v8 and v0 bases, any path: one origin, one record
        let a = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:7/v8/management",
        };
        let b = Guard {
            state_dir: &d,
            mgmt: "HTTP://127.0.0.1:7/v0/management/",
        };
        let c = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:8/v0/management",
        };
        let k = Secret::new(b"k".to_vec());
        a.stop(403, &k, 5);
        assert_eq!(b.refusal(&k, 6), Some(403));
        assert_eq!(c.refusal(&k, 6), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_older_cliproxyapi_refusal_is_honoured() {
        let d = tmp("legacy");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:9/v8/management",
        };
        let k = Secret::new(b"k".to_vec());
        let fp = keysource::fingerprint(&k, "salt");
        let legacy = g.legacy_file();
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(
            &legacy,
            serde_json::json!({"v":1,"salt":"salt","accounts":[],
                "auth_fail":{"status":401,"at":100,"fp":fp}})
            .to_string(),
        )
        .unwrap();
        assert_eq!(g.refusal(&k, 101), Some(401));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_keys_on_one_origin_keep_separate_refusals() {
        let d = tmp("two-keys");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:5/v0/management",
        };
        let (k1, k2) = (Secret::new(b"k1".to_vec()), Secret::new(b"k2".to_vec()));
        assert!(g.stop(401, &k1, 10));
        assert!(g.stop(403, &k2, 11));
        assert_eq!(g.refusal(&k1, 12), Some(401));
        assert_eq!(g.refusal(&k2, 12), Some(403));
        // the same key again replaces its record, adds none
        assert!(g.stop(401, &k2, 13));
        assert_eq!(g.load().fails.len(), 2);
        assert_eq!(g.refusal(&k2, 14), Some(401));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_refusal_list_is_small_and_drops_the_oldest() {
        let d = tmp("bounded");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:6/v0/management",
        };
        let keys: Vec<Secret> = (0..6).map(|i| Secret::new(vec![b'k', b'0' + i])).collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(g.stop(401, k, 10 + i as i64));
        }
        assert_eq!(g.load().fails.len(), MAX_FAILS);
        assert_eq!(g.refusal(&keys[0], 20), None);
        assert_eq!(g.refusal(&keys[1], 20), None);
        assert_eq!(g.refusal(&keys[5], 20), Some(401));
        assert_eq!(g.refusal(&keys[2], 20), Some(401));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_single_refusal_of_the_older_record_format_is_kept() {
        let d = tmp("old-format");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:4/v0/management",
        };
        let (k1, k2) = (Secret::new(b"k1".to_vec()), Secret::new(b"k2".to_vec()));
        let fp = keysource::fingerprint(&k1, "salt");
        std::fs::create_dir_all(g.file().parent().unwrap()).unwrap();
        std::fs::write(
            g.file(),
            serde_json::json!({"v":1,"salt":"salt","auth_fail":{"status":401,"at":5,"fp":fp}})
                .to_string(),
        )
        .unwrap();
        assert_eq!(g.refusal(&k1, 6), Some(401));
        assert!(g.stop(403, &k2, 7));
        assert_eq!(g.refusal(&k1, 8), Some(401));
        assert_eq!(g.refusal(&k2, 8), Some(403));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn localhost_and_127_0_0_1_are_one_origin() {
        let d = tmp("loopback");
        let a = Guard {
            state_dir: &d,
            mgmt: "http://localhost:7/v0/management",
        };
        let b = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:7/v0/management",
        };
        let c = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:8/v0/management",
        };
        assert_eq!(a.file(), b.file());
        assert_eq!(a.lock_file(), b.lock_file());
        assert_ne!(a.file(), c.file());
        let k = Secret::new(b"k".to_vec());
        a.stop(401, &k, 1);
        assert_eq!(b.refusal(&k, 2), Some(401));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_lock_is_exclusive_per_origin_and_needs_a_writable_dir() {
        let d = tmp("lock");
        let g = Guard {
            state_dir: &d,
            mgmt: "http://127.0.0.1:3/v0/management",
        };
        let held = g.lock(None).expect("first lock");
        // a second holder of the same origin cannot get it (try without the 8 s wait)
        assert!(fsio::lock(&g.lock_file(), Duration::from_millis(50))
            .unwrap()
            .is_none());
        drop(held);
        assert!(g.lock(None).is_some());
        // a state "directory" that is a file: no lock, so nothing is sent
        let f = d.join("file");
        std::fs::write(&f, "x").unwrap();
        let bad = Guard {
            state_dir: &f,
            mgmt: "http://127.0.0.1:3/v0/management",
        };
        assert!(bad.lock(None).is_none());
        assert!(!bad.writable());
        let _ = std::fs::remove_dir_all(&d);
    }
}
