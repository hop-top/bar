//! Background-refresh cache. Readers never wait: they return whatever is
//! stored and, when it is stale or missing, the lock winner spawns a
//! detached `bar refresh <request-json>` that runs the job and stores the
//! result.

use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::expand::CommandRunner;

/// `%command%` results stay fresh for an hour.
pub const COMMAND_TTL: u64 = 3600;
/// Per-command limit for `%command%`, in seconds.
pub const COMMAND_TIMEOUT: u64 = 3;
/// A lock older than this belongs to a crashed refresh.
const LOCK_EXPIRY: Duration = Duration::from_secs(30);
const DEFAULT_REFRESH_TIMEOUT: Duration = Duration::from_secs(20);

/// Work a refresh performs. Serialised into the refresh process's
/// arguments, so it carries only kinds and options, never secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `sh -c cmd`; stdout on success. `timeout` in seconds.
    Command { cmd: String, timeout: u64 },
}

impl Job {
    fn to_value(&self) -> Value {
        match self {
            Job::Command { cmd, timeout } => {
                json!({"kind": "command", "cmd": cmd, "timeout": timeout})
            }
        }
    }

    fn from_value(v: &Value) -> Result<Job, String> {
        match v["kind"].as_str() {
            Some("command") => Ok(Job::Command {
                cmd: v["cmd"].as_str().ok_or("command job without cmd")?.into(),
                timeout: v["timeout"].as_u64().unwrap_or(COMMAND_TIMEOUT),
            }),
            other => Err(format!("unknown job kind {other:?}")),
        }
    }

    /// `None` on failure or timeout; `limit` caps the job's own timeout.
    pub fn execute(&self, limit: Duration) -> Option<String> {
        match self {
            Job::Command { cmd, timeout } => {
                run_command(cmd, limit.min(Duration::from_secs(*timeout)))
            }
        }
    }
}

/// What `bar refresh` receives: where to store, and what to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub key: String,
    pub job: Job,
}

impl Request {
    pub fn to_json(&self) -> String {
        json!({"key": self.key, "job": self.job.to_value()}).to_string()
    }

    pub fn from_json(s: &str) -> Result<Request, String> {
        let v: Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
        Ok(Request {
            key: v["key"].as_str().ok_or("request without key")?.into(),
            job: Job::from_value(&v["job"])?,
        })
    }
}

/// Whole-refresh limit from `BAR_REFRESH_TIMEOUT` (seconds).
pub fn refresh_timeout(env: Option<&str>) -> Duration {
    env.and_then(|v| v.trim().parse().ok())
        .map_or(DEFAULT_REFRESH_TIMEOUT, Duration::from_secs)
}

/// FNV-1a, 128-bit: keys become fixed hex filenames.
fn hash_hex(key: &str) -> String {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in key.bytes() {
        h ^= u128::from(b);
        h = h.wrapping_mul(0x0000000001000000000000000000013b);
    }
    format!("{h:032x}")
}

pub struct Cache {
    state: PathBuf,
    dir: PathBuf,
    exe: PathBuf,
}

impl Cache {
    /// `<state_dir>/cache`, refreshed by the running binary.
    pub fn new(state: &Path) -> Cache {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("bar"));
        Cache::with_exe(state, exe)
    }

    /// As `new`, spawning `exe refresh` instead of the current binary.
    pub fn with_exe(state: &Path, exe: PathBuf) -> Cache {
        Cache {
            state: state.to_path_buf(),
            dir: state.join("cache"),
            exe,
        }
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(hash_hex(key))
    }

    pub fn lock_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.lock", hash_hex(key)))
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
    }

    /// Atomic: a private temp file in the cache dir, renamed into place.
    pub fn store(&self, key: &str, value: &str) -> std::io::Result<()> {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        self.ensure_dir()?;
        let dest = self.path_for(key);
        let tmp = self.dir.join(format!(
            ".{}.{}.{}.tmp",
            hash_hex(key),
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let r = crate::click::write_private(&tmp, value).and_then(|()| fs::rename(&tmp, &dest));
        if r.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        r
    }

    /// The stored value, possibly stale; spawns a refresh when it is older
    /// than `ttl` seconds or missing. Never waits on the job.
    pub fn read_or_refresh(&self, key: &str, ttl: u64, job: &Job) -> Option<String> {
        let path = self.path_for(key);
        let value = fs::read_to_string(&path).ok();
        let fresh = value.is_some()
            && fs::metadata(&path)
                .and_then(|m| m.modified())
                .is_ok_and(|t| age(t) < Duration::from_secs(ttl));
        if !fresh && self.try_lock(key) {
            let req = Request {
                key: key.to_string(),
                job: job.clone(),
            };
            if self.spawn(&req).is_err() {
                let _ = fs::remove_file(self.lock_path(key));
            }
        }
        value
    }

    /// `true` for the one caller that created the lock.
    fn try_lock(&self, key: &str) -> bool {
        if self.ensure_dir().is_err() {
            return false;
        }
        let lock = self.lock_path(key);
        for _ in 0..2 {
            let mut o = fs::OpenOptions::new();
            o.write(true).create_new(true);
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            match o.open(&lock) {
                Ok(_) => return true,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&lock)
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| age(t) > LOCK_EXPIRY);
                    if !stale || fs::remove_file(&lock).is_err() {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        false
    }

    /// Detached: own process group, no stdio, not waited on.
    fn spawn(&self, req: &Request) -> std::io::Result<()> {
        Command::new(&self.exe)
            .arg("refresh")
            .arg(req.to_json())
            .env("BAR_STATE_DIR", &self.state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map(drop)
    }

    /// Body of `bar refresh`: run, store on success, release the lock.
    /// A failed job keeps the last value.
    pub fn refresh(&self, req: &Request, limit: Duration) {
        if let Some(v) = req.job.execute(limit) {
            let _ = self.store(&req.key, &v);
        }
        let _ = fs::remove_file(self.lock_path(&req.key));
    }
}

fn age(t: SystemTime) -> Duration {
    SystemTime::now().duration_since(t).unwrap_or_default()
}

/// Kills the whole group so grandchildren holding stdout die too.
fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// `sh -c cmd` stdout when it exits 0 within `limit`.
fn run_command(cmd: &str, limit: Duration) -> Option<String> {
    let deadline = Instant::now() + limit;
    let mut child = Command::new("sh")
        .args(["-c", cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let pid = child.id();
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => break None,
        }
    };
    let out = status.and_then(|_| {
        rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    });
    if status.is_none() || out.is_none() {
        kill_group(pid);
        let _ = child.wait();
        return None;
    }
    if !status.is_some_and(|s| s.success()) {
        return None;
    }
    out.map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The real `CommandRunner`: `%command%` via the cache, key
/// `expand:<cmd>`, fresh for an hour, 3 s per run.
pub struct CachedRunner {
    cache: Cache,
}

impl CachedRunner {
    pub fn new(cache: Cache) -> CachedRunner {
        CachedRunner { cache }
    }
}

impl CommandRunner for CachedRunner {
    fn run(&self, cmd: &str) -> Option<String> {
        let job = Job::Command {
            cmd: cmd.to_string(),
            timeout: COMMAND_TIMEOUT,
        };
        self.cache
            .read_or_refresh(&format!("expand:{cmd}"), COMMAND_TTL, &job)
    }
}
