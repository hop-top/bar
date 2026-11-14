//! Background-refresh cache. Readers never wait: they return whatever is
//! stored and, when it is stale or missing, the lock winner spawns a
//! detached `bar refresh <request-json>` that runs the job and stores the
//! result.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::expand::CommandRunner;
pub use crate::job::Job;

/// `%command%` results stay fresh for an hour.
pub const COMMAND_TTL: u64 = 3600;
/// Per-command limit for `%command%`, in seconds.
pub const COMMAND_TIMEOUT: u64 = 3;
/// After a failed refresh, wait this long (or `ttl`, if shorter) before
/// trying again, in seconds.
pub const FAIL_BACKOFF: u64 = 60;
/// Most bytes of a stored value. Command output is cut to it (a plain
/// text consumer such as `gitst` sees the cut); [`Cache::store`] refuses a
/// longer value, so structured output reduced in its job is never stored
/// cut.
pub const MAX_VALUE: usize = 64 * 1024;
/// A lock older than this belongs to a crashed refresh.
const LOCK_EXPIRY: Duration = Duration::from_secs(30);
const DEFAULT_REFRESH_TIMEOUT: Duration = Duration::from_secs(20);

/// What `bar refresh` receives: where to store, what to run, and the
/// token in the lock it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub key: String,
    pub job: Job,
    pub lock: String,
}

impl Request {
    pub fn to_json(&self) -> String {
        json!({"key": self.key, "job": self.job.to_value(), "lock": self.lock}).to_string()
    }

    pub fn from_json(s: &str) -> Result<Request, String> {
        let v: Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
        Ok(Request {
            key: v["key"].as_str().ok_or("request without key")?.into(),
            job: Job::from_value(&v["job"])?,
            lock: v["lock"].as_str().unwrap_or_default().into(),
        })
    }
}

/// Whole-refresh limit from `BAR_REFRESH_TIMEOUT` (seconds), kept below
/// the lock expiry so a live refresh never looks crashed.
pub fn refresh_timeout(env: Option<&str>) -> Duration {
    env.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map_or(DEFAULT_REFRESH_TIMEOUT, Duration::from_secs)
        .clamp(Duration::from_secs(1), LOCK_EXPIRY - Duration::from_secs(1))
}

/// Cuts to `MAX_VALUE` bytes on a char boundary.
pub(crate) fn cap(mut s: String) -> String {
    if s.len() > MAX_VALUE {
        let mut end = MAX_VALUE;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

fn read_capped(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_VALUE as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(cap(String::from_utf8_lossy(&buf).into_owned()))
}

/// Unique per process and call: lock tokens and temp file names.
pub(crate) fn nonce() -> String {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{}.{}.{nanos}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn modified_age(path: &Path) -> Option<Duration> {
    fs::metadata(path).and_then(|m| m.modified()).ok().map(age)
}

/// FNV-1a, 128-bit: keys become fixed hex filenames.
pub(crate) fn hash_hex(key: &str) -> String {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in key.bytes() {
        h ^= u128::from(b);
        h = h.wrapping_mul(0x0000000001000000000000000000013b);
    }
    format!("{h:032x}")
}

#[derive(Clone)]
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

    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(hash_hex(key))
    }

    /// The stored value of the entry named `entry` (a [`hash_hex`] of its
    /// key, 32 lower-case hex digits), whatever its age.
    pub fn read_entry(&self, entry: &str) -> Option<String> {
        let ok = entry.len() == 32 && entry.bytes().all(|b| b.is_ascii_hexdigit());
        ok.then(|| read_capped(&self.dir.join(entry))).flatten()
    }

    /// Makes entry `entry` stale and forgets its failures: the next read
    /// returns the value and starts a refresh.
    pub fn age_entry(&self, entry: &str) {
        let ok = entry.len() == 32 && entry.bytes().all(|b| b.is_ascii_hexdigit());
        if !ok {
            return;
        }
        let _ = fs::remove_file(self.dir.join(format!("{entry}.fail")));
        if let Ok(f) = fs::OpenOptions::new()
            .write(true)
            .open(self.dir.join(entry))
        {
            let _ = f.set_modified(SystemTime::UNIX_EPOCH);
        }
    }

    pub fn fail_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.fail", hash_hex(key)))
    }

    pub fn lock_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.lock", hash_hex(key)))
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        crate::platform::private_dir(&self.dir)
    }

    /// Atomic: a private temp file in the cache dir, renamed into place.
    /// A value over `MAX_VALUE` is refused, the stored one kept.
    pub fn store(&self, key: &str, value: &str) -> std::io::Result<()> {
        if value.len() > MAX_VALUE {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "value over the cache size cap",
            ));
        }
        self.write_atomic(&self.path_for(key), value)
    }

    fn write_atomic(&self, dest: &Path, body: &str) -> std::io::Result<()> {
        self.ensure_dir()?;
        crate::click::write_atomic(dest, body)
    }

    /// A refresh failed less than `FAIL_BACKOFF` ago. The window ignores
    /// the ttl, so a short one (a plugin's 5 s floor, `ttl=1`) cannot turn
    /// a persistently failing job into a respawn every few seconds.
    fn backing_off(&self, key: &str) -> bool {
        let window = Duration::from_secs(FAIL_BACKOFF);
        modified_age(&self.fail_path(key)).is_some_and(|a| a < window)
    }

    /// The stored value when younger than `ttl` seconds; never spawns.
    pub fn read_fresh(&self, key: &str, ttl: u64) -> Option<String> {
        let path = self.path_for(key);
        let age = modified_age(&path)?;
        (age < Duration::from_secs(ttl))
            .then(|| read_capped(&path))
            .flatten()
    }

    /// The stored value, possibly stale; spawns a refresh when it is older
    /// than `ttl` seconds or missing. Never waits on the job.
    pub fn read_or_refresh(&self, key: &str, ttl: u64, job: &Job) -> Option<String> {
        self.read_or_refresh_if(key, ttl, job, |_| true)
    }

    /// As [`Cache::read_or_refresh`]; a value `current` rejects (made for
    /// another commit or index) is stale whatever its age.
    pub fn read_or_refresh_if(
        &self,
        key: &str,
        ttl: u64,
        job: &Job,
        current: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let path = self.path_for(key);
        let value = read_capped(&path);
        let fresh = value.as_deref().is_some_and(current)
            && modified_age(&path).is_some_and(|a| a < Duration::from_secs(ttl));
        if fresh || self.backing_off(key) {
            return value;
        }
        if let Some(token) = self.try_lock(key) {
            let req = Request {
                key: key.to_string(),
                job: job.clone(),
                lock: token,
            };
            if self.spawn(&req).is_err() {
                self.release(&req);
            }
        }
        value
    }

    /// The owner token for the one caller that created the lock.
    fn try_lock(&self, key: &str) -> Option<String> {
        self.ensure_dir().ok()?;
        let lock = self.lock_path(key);
        for _ in 0..2 {
            let mut o = fs::OpenOptions::new();
            crate::platform::private_file(o.write(true).create_new(true));
            match o.open(&lock) {
                Ok(mut f) => {
                    let token = nonce();
                    return f.write_all(token.as_bytes()).ok().map(|()| token);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    if !modified_age(&lock).is_some_and(|a| a > LOCK_EXPIRY) {
                        return None;
                    }
                    self.take_over(&lock)?;
                }
                Err(_) => return None,
            }
        }
        None
    }

    /// Moves an expired lock aside. Only one reader's rename succeeds; if
    /// the lock it moved was replaced by a live one in the meantime, it is
    /// put back (link fails rather than clobber a newer lock).
    fn take_over(&self, lock: &Path) -> Option<()> {
        let name = lock.file_name()?.to_string_lossy();
        let aside = self.dir.join(format!(".{name}.{}.stale", nonce()));
        fs::rename(lock, &aside).ok()?;
        let expired = modified_age(&aside).is_some_and(|a| a > LOCK_EXPIRY);
        if !expired {
            let _ = fs::hard_link(&aside, lock);
        }
        let _ = fs::remove_file(&aside);
        expired.then_some(())
    }

    /// Removes the lock only while it still holds this request's token.
    fn release(&self, req: &Request) {
        let lock = self.lock_path(&req.key);
        if fs::read_to_string(&lock).is_ok_and(|t| t == req.lock) {
            let _ = fs::remove_file(&lock);
        }
    }

    /// Detached: own process group, no stdio, not waited on. Runs in the
    /// state dir, not the render's working directory (often a cloned
    /// repository), so nothing it starts resolves against the project;
    /// only `%cmd%` keeps the render's directory, as the user's command.
    fn spawn(&self, req: &Request) -> std::io::Result<()> {
        let mut c = Command::new(&self.exe);
        if !matches!(req.job, Job::Command { .. }) {
            c.current_dir(outside_project(&self.state));
        }
        c.arg("refresh")
            .arg(req.to_json())
            .env("BAR_STATE_DIR", &self.state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::spawn_detached(c)
    }

    /// Body of `bar refresh`: run, store on success, release the lock.
    /// A failed job, or a value that cannot be stored (over `MAX_VALUE`),
    /// keeps the last value and starts the backoff window.
    pub fn refresh(&self, req: &Request, limit: Duration) {
        let fail = self.fail_path(&req.key);
        let stored = req
            .job
            .execute(limit)
            .is_some_and(|v| self.store(&req.key, &v).is_ok());
        if stored {
            let _ = fs::remove_file(&fail);
        } else {
            let _ = self.write_atomic(&fail, "");
        }
        self.release(req);
    }
}

fn age(t: SystemTime) -> Duration {
    SystemTime::now().duration_since(t).unwrap_or_default()
}

/// Working directory for what bar starts on its own (`bar refresh`, `gh`,
/// `glab`): the state dir, `/` before it exists; never the render's
/// directory, often a cloned repository, so nothing resolves against the
/// project.
pub(crate) fn outside_project(state: &Path) -> PathBuf {
    if state.is_dir() {
        state.to_path_buf()
    } else {
        PathBuf::from("/")
    }
}

/// Shell command stdout (`sh -c` on Unix, `cmd /C` on Windows) when it
/// exits 0 within `limit`.
pub(crate) fn run_command(cmd: &str, limit: Duration) -> Option<String> {
    run_child(crate::platform::shell(cmd), None, limit)
}

/// Stdout of `cmd` when it exits 0 within `limit`; otherwise killed with
/// its process group (its process tree on Windows). `input`, if any, is
/// written to its stdin, else stdin is closed.
pub(crate) fn run_child(cmd: Command, input: Option<Vec<u8>>, limit: Duration) -> Option<String> {
    match run_child_raw(cmd, input, limit) {
        Child::Out(b) => Some(cap(String::from_utf8_lossy(&b).into_owned())),
        _ => None,
    }
}

/// [`run_child`] reading up to `max` bytes of stdout instead of
/// `MAX_VALUE`, for output that is reduced before it is stored.
pub(crate) fn run_child_capped(
    cmd: Command,
    input: Option<Vec<u8>>,
    limit: Duration,
    max: usize,
) -> Option<String> {
    match run_child_raw_capped(cmd, input, limit, max) {
        Child::Out(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        _ => None,
    }
}

/// How a child run by `run_child_raw` ended.
pub(crate) enum Child {
    /// Exit 0: stdout, at most `MAX_VALUE` bytes (or the cap asked for).
    Out(Vec<u8>),
    /// It could not start or exited non-zero.
    Failed,
    /// Killed at the time limit.
    TimedOut,
}

/// As `run_child`, with the raw stdout and whether it timed out.
pub(crate) fn run_child_raw(cmd: Command, input: Option<Vec<u8>>, limit: Duration) -> Child {
    run_child_raw_capped(cmd, input, limit, MAX_VALUE)
}

fn run_child_raw_capped(
    mut cmd: Command,
    input: Option<Vec<u8>>,
    limit: Duration,
    max: usize,
) -> Child {
    let deadline = Instant::now() + limit;
    let mut child = match crate::platform::own_group(&mut cmd)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Child::Failed,
    };
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        // a plugin that never reads must not block us; EPIPE is ignored
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let Some(mut stdout) = child.stdout.take() else {
        return Child::Failed;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(max as u64).read_to_end(&mut buf);
        // keep draining so a chatty command does not block on a full pipe
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        let _ = tx.send(buf);
    });
    let status = bar_plugin::wait::wait_until(&mut child, deadline)
        .ok()
        .flatten();
    let out = status.and_then(|_| {
        rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    });
    if status.is_none() || out.is_none() {
        crate::platform::kill_tree(&mut child);
        let _ = child.wait();
        return Child::TimedOut;
    }
    match (status, out) {
        (Some(s), Some(b)) if s.success() => Child::Out(b),
        _ => Child::Failed,
    }
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
