//! Temp dirs for tests, all under `<temp>/bar-tests`. A fixture removes
//! its dir when it drops ([`remove`]); a dir left behind (a test killed
//! mid-run, a detached `bar refresh` writing after the test ended) is
//! swept by the next test process once it is 10 minutes old.
//!
//! `BAR_TEST_KEEP=1` keeps the dir of a failing test: it is marked with a
//! `.keep` file, never swept, and its path goes to stderr.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// Age at which a dir left in the root is swept.
const STALE: Duration = Duration::from_secs(600);
/// Marks a dir kept for a failing test.
const KEEP: &str = ".keep";

/// `<temp>/bar-tests`, created, and swept of stale dirs once per process.
pub fn root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let r = std::env::temp_dir().join("bar-tests");
        std::fs::create_dir_all(&r).unwrap();
        sweep(&r, SystemTime::now());
        r
    })
    .clone()
}

/// Removes the dirs in `root` last modified over [`STALE`] before `now`,
/// except kept ones.
pub fn sweep(root: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > STALE));
        if old && !p.join(KEEP).exists() {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

/// Whether a failing test keeps its dir (`BAR_TEST_KEEP` set, not empty or `0`).
fn keep_failing() -> bool {
    std::env::var("BAR_TEST_KEEP").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// A test's temp dir at the end of the test: removed, or kept (marked,
/// path printed) when the test is failing and `BAR_TEST_KEEP` asks.
pub fn remove(dir: &Path) {
    if std::thread::panicking() && keep_failing() {
        let _ = std::fs::write(dir.join(KEEP), "");
        eprintln!("kept {}", dir.display());
        return;
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A temp dir removed on drop (see [`remove`]).
pub struct Dir(pub PathBuf);

impl Dir {
    /// `root()/<name>`, created (any old one removed first).
    pub fn new(name: &str) -> Dir {
        let p = root().join(name);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Dir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        remove(&self.0);
    }
}
