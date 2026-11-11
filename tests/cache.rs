// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;

use common::{wait_until, BG_WAIT};
use hop_bar::cache::{Cache, CachedRunner, Job, Request};
use hop_bar::expand::CommandRunner;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-cache-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(hop_bar::platform::canonicalize(&p).unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn cache(&self) -> Cache {
        Cache::with_exe(self.path(), bin())
    }
    fn lines(&self, rel: &str) -> usize {
        std::fs::read_to_string(self.0.join(rel))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bar"))
}

fn cmd_job(cmd: &str) -> Job {
    Job::Command {
        cmd: cmd.to_string(),
        timeout: 30,
    }
}

/// Appends a line to `<tmp>/<file>` each time the job runs.
fn counting(t: &Tmp, file: &str, then: &str) -> Job {
    cmd_job(&format!(
        "echo x >> '{}'; {then}",
        t.path().join(file).display()
    ))
}

fn age(path: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(secs))
        .unwrap();
}

fn lock_gone(c: &Cache, key: &str) -> bool {
    !c.lock_path(key).exists()
}

#[test]
fn fresh_value_spawns_nothing() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", "old").unwrap();
    let got = c.read_or_refresh("k", 3600, &counting(&t, "runs", "echo new"));
    assert_eq!(got.as_deref(), Some("old"));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(t.lines("runs"), 0);
    assert!(lock_gone(&c, "k"));
}

#[test]
fn stale_value_returned_and_refresh_spawned() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", "old").unwrap();
    age(&c.path_for("k"), 120);
    // the same job is polled with: if a loaded host lets the first attempt
    // hit the job timeout, the retry after the backoff must produce `new`;
    // the count line is last so only completed runs are counted
    let job = cmd_job(&format!(
        "echo new; echo x >> '{}'",
        t.path().join("runs").display()
    ));
    let got = c.read_or_refresh("k", 60, &job);
    assert_eq!(got.as_deref(), Some("old"));
    assert!(wait_until(BG_WAIT, || {
        c.read_or_refresh("k", 60, &job).as_deref() == Some("new\n")
    }));
    // not `== 1`: a starved first attempt may time out and be retried
    assert!(t.lines("runs") >= 1);
}

#[test]
fn failed_refresh_keeps_last_value() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", "old").unwrap();
    age(&c.path_for("k"), 120);
    let job = counting(&t, "runs", "exit 1");
    assert_eq!(c.read_or_refresh("k", 60, &job).as_deref(), Some("old"));
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 1 && lock_gone(&c, "k")));
    assert_eq!(std::fs::read_to_string(c.path_for("k")).unwrap(), "old");
}

#[test]
fn concurrent_readers_spawn_one_refresh() {
    let t = Tmp::new();
    let job = counting(&t, "runs", "sleep 1; echo v");
    std::thread::scope(|s| {
        for _ in 0..20 {
            s.spawn(|| {
                let c = t.cache();
                assert_eq!(c.read_or_refresh("k", 60, &job), None);
            });
        }
    });
    let c = t.cache();
    assert!(wait_until(BG_WAIT, || lock_gone(&c, "k") && c.path_for("k").exists()));
    assert_eq!(t.lines("runs"), 1);
}

#[test]
fn writes_are_atomic_and_0600() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", "one").unwrap();
    let p = c.path_for("k");
    let ino = std::fs::metadata(&p).unwrap().ino();
    c.store("k", "two").unwrap();
    let meta = std::fs::metadata(&p).unwrap();
    assert_ne!(
        meta.ino(),
        ino,
        "replaced by rename, not rewritten in place"
    );
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let dir = p.parent().unwrap();
    assert_eq!(dir, t.path().join("cache"));
    assert_eq!(
        std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
    let name = p.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        name.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.'),
        "{name}"
    );
}

#[test]
fn no_temp_files_left_after_failure() {
    let t = Tmp::new();
    let c = t.cache();
    let p = c.path_for("k");
    // a non-empty directory at the target makes the final rename fail
    std::fs::create_dir_all(p.join("x")).unwrap();
    assert!(c.store("k", "v").is_err());
    let names: Vec<_> = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![p.file_name().unwrap().to_owned()]);
}

#[test]
fn refresh_timeout_kills_job() {
    let t = Tmp::new();
    let marker = t.path().join("finished");
    // far longer than the 1 s limit, so timing noise cannot blur the result
    // unique duration: lets pgrep find an orphaned grandchild
    let nap = format!(
        "8.{}{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    );
    let job = Job::Command {
        cmd: format!("sleep {nap}; touch '{}'; echo late", marker.display()),
        timeout: 60,
    };
    let req = Request {
        key: "k".into(),
        job,
        lock: "t".into(),
    };
    let start = Instant::now();
    let (iso, cwd) = common::isolation(t.path());
    let out = std::process::Command::new(bin())
        .arg("refresh")
        .arg(req.to_json())
        .envs(iso)
        .current_dir(cwd)
        .env("BAR_STATE_DIR", t.path())
        .env("BAR_REFRESH_TIMEOUT", "1")
        .output()
        .unwrap();
    let took = start.elapsed();
    assert!(out.status.success());
    // an orphaned `sleep` holding stdout would delay this past 8 s
    assert!(took < Duration::from_secs(6), "took {took:?}");
    std::thread::sleep(Duration::from_millis(200));
    let pattern = format!("sleep {nap}|{}", marker.display());
    let alive = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .unwrap();
    assert!(
        alive.stdout.is_empty(),
        "job processes survived: {}",
        String::from_utf8_lossy(&alive.stdout)
    );
    assert!(!marker.exists());
    let c = t.cache();
    assert!(!c.path_for("k").exists());
    assert!(lock_gone(&c, "k"));
}

#[test]
fn refresh_timeout_defaults_to_20s() {
    assert_eq!(
        hop_bar::cache::refresh_timeout(None),
        Duration::from_secs(20)
    );
    assert_eq!(
        hop_bar::cache::refresh_timeout(Some("1")),
        Duration::from_secs(1)
    );
    assert_eq!(
        hop_bar::cache::refresh_timeout(Some("junk")),
        Duration::from_secs(20)
    );
}

#[test]
fn job_json_has_no_secrets() {
    let secret = "s3cr3t-token-value";
    let req = Request {
        key: "expand:echo $BAR_TEST_SECRET_TOKEN".into(),
        job: cmd_job("echo $BAR_TEST_SECRET_TOKEN"),
        lock: "t".into(),
    };
    let json = req.to_json();
    assert!(!json.contains(secret), "{json}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut top: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    top.sort();
    assert_eq!(top, ["job", "key", "lock"]);
    let mut job: Vec<_> = v["job"].as_object().unwrap().keys().cloned().collect();
    job.sort();
    assert_eq!(job, ["cmd", "kind", "timeout"]);
    assert_eq!(Request::from_json(&json).unwrap(), req);
}

#[test]
fn cold_command_expansion_does_not_block() {
    let t = Tmp::new();
    let r = CachedRunner::new(t.cache());
    let start = Instant::now();
    assert_eq!(r.run("sleep 5; echo late"), None);
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[test]
fn command_result_cached_one_hour() {
    assert_eq!(hop_bar::cache::COMMAND_TTL, 3600);
    let t = Tmp::new();
    let c = t.cache();
    let r = CachedRunner::new(t.cache());
    let cmd = format!("echo x >> '{}'; echo hi", t.path().join("runs").display());
    let key = format!("expand:{cmd}");
    assert_eq!(r.run(&cmd), None);
    assert!(wait_until(BG_WAIT, || r.run(&cmd).as_deref() == Some("hi\n")));
    assert!(wait_until(BG_WAIT, || lock_gone(&c, &key)));
    // runs are measured from here: on a loaded host the first attempt may
    // hit the job timeout and be retried, which is not under test
    let before = t.lines("runs");
    age(&c.path_for(&key), 3500);
    assert_eq!(r.run(&cmd).as_deref(), Some("hi\n"));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(t.lines("runs"), before, "still fresh before an hour");
    age(&c.path_for(&key), 3700);
    assert_eq!(r.run(&cmd).as_deref(), Some("hi\n"));
    assert!(wait_until(BG_WAIT, || t.lines("runs") > before));
}

#[test]
fn render_returns_within_50ms_with_slow_job() {
    let t = Tmp::new();
    let cfg_dir = t.path().join("cfg/bar");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("bar.conf"),
        "set left \"#{model[click=%sleep 5 && echo 9%]}\"\n",
    )
    .unwrap();
    let run = || {
        let start = Instant::now();
        let (iso, cwd) = common::isolation(t.path());
        let out = std::process::Command::new(bin())
            .arg("render")
            .envs(iso)
            .current_dir(cwd)
            .env("XDG_CONFIG_HOME", t.path().join("cfg"))
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success());
        start.elapsed()
    };
    // warm the binary into the page cache first; then take the best of 3
    run();
    let state = Cache::with_exe(&t.path().join("state"), bin());
    assert!(
        state.lock_path("expand:sleep 5 && echo 9").exists(),
        "render went through the cache and spawned the slow refresh"
    );
    // Process start-up of a debug binary under a parallel test run is
    // noisy, so the binary only has to beat the 5 s job by a wide margin;
    // the 50 ms budget is held by render itself, measured in-process.
    let best = (0..3).map(|_| run()).min().unwrap();
    assert!(
        best < Duration::from_millis(1000),
        "bar render took {best:?}"
    );
    let cfg = hop_bar::conf::Config {
        left: "#{model[click=%sleep 5 && echo 8%]}".into(),
        ..hop_bar::conf::Config::default()
    };
    let session = hop_bar::session::parse("{}");
    let r = CachedRunner::new(Cache::with_exe(&t.path().join("state2"), bin()));
    let start = Instant::now();
    hop_bar::render::render(&session, &cfg, Some(120), &r);
    let took = start.elapsed();
    assert!(took < Duration::from_millis(50), "render took {took:?}");
    let state2 = Cache::with_exe(&t.path().join("state2"), bin());
    assert!(state2.lock_path("expand:sleep 5 && echo 8").exists());
}

#[test]
fn refresh_timeout_clamped() {
    let t = hop_bar::cache::refresh_timeout;
    assert_eq!(t(Some("0")), Duration::from_secs(20));
    assert_eq!(t(Some("29")), Duration::from_secs(29));
    assert_eq!(t(Some("30")), Duration::from_secs(29));
    assert_eq!(t(Some("3600")), Duration::from_secs(29));
}

#[test]
fn failing_job_backs_off() {
    let t = Tmp::new();
    let c = t.cache();
    let job = counting(&t, "runs", "exit 1");
    assert_eq!(c.read_or_refresh("k", 3600, &job), None);
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 1 && lock_gone(&c, "k")));
    for _ in 0..5 {
        assert_eq!(c.read_or_refresh("k", 3600, &job), None);
        std::thread::sleep(Duration::from_millis(60));
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(t.lines("runs"), 1, "backed off inside the window");
    let fail = c.fail_path("k");
    assert_eq!(
        std::fs::metadata(&fail).unwrap().permissions().mode() & 0o777,
        0o600
    );
    age(&fail, hop_bar::cache::FAIL_BACKOFF + 1);
    c.read_or_refresh("k", 3600, &job);
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 2));
}

#[test]
fn backoff_floor_holds_for_short_ttl() {
    // a 5 s ttl (a plugin's floor) must not retry a failing job every 5 s
    let t = Tmp::new();
    let c = t.cache();
    let job = counting(&t, "runs", "exit 1");
    c.read_or_refresh("k", 5, &job);
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 1 && lock_gone(&c, "k")));
    age(&c.fail_path("k"), 6);
    c.read_or_refresh("k", 5, &job);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(t.lines("runs"), 1, "retried inside the 60 s floor");
    age(&c.fail_path("k"), hop_bar::cache::FAIL_BACKOFF + 1);
    c.read_or_refresh("k", 5, &job);
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 2));
}

#[test]
fn backoff_floor_holds_for_plugin_jobs() {
    let t = Tmp::new();
    let c = t.cache();
    let job = Job::PluginRender {
        path: "/nonexistent/bar-x".into(),
        input: "{}".into(),
        timeout: 3,
        accounts: false,
        email: None,
    };
    let fail = c.fail_path("p");
    std::fs::create_dir_all(fail.parent().unwrap()).unwrap();
    std::fs::write(&fail, "").unwrap();
    age(&fail, 6);
    c.read_or_refresh("p", 5, &job);
    let rerun = || {
        std::fs::metadata(&fail)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m.elapsed().unwrap_or_default() < Duration::from_secs(3))
    };
    assert!(
        !wait_until(Duration::from_secs(1), rerun),
        "retried inside the floor"
    );
}

#[test]
fn success_clears_fail_marker() {
    let t = Tmp::new();
    let c = t.cache();
    c.read_or_refresh("k", 3600, &cmd_job("exit 1"));
    assert!(wait_until(BG_WAIT, || c.fail_path("k").exists() && lock_gone(&c, "k")));
    let req = Request {
        key: "k".into(),
        job: cmd_job("echo ok"),
        lock: "t".into(),
    };
    c.refresh(&req, Duration::from_secs(30));
    assert!(!c.fail_path("k").exists());
    assert_eq!(std::fs::read_to_string(c.path_for("k")).unwrap(), "ok\n");
}

#[test]
fn expired_lock_taken_over() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("seed", "").unwrap();
    std::fs::write(c.lock_path("k"), "crashed").unwrap();
    age(&c.lock_path("k"), 40);
    c.read_or_refresh("k", 60, &counting(&t, "runs", "echo v"));
    assert!(wait_until(BG_WAIT, || t.lines("runs") == 1 && lock_gone(&c, "k")));
    let leftovers: Vec<_> = std::fs::read_dir(t.path().join("cache"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("stale") || n.contains("lock"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn live_lock_is_not_taken_over() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("seed", "").unwrap();
    std::fs::write(c.lock_path("k"), "busy").unwrap();
    age(&c.lock_path("k"), 10);
    c.read_or_refresh("k", 60, &counting(&t, "runs", "echo v"));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(t.lines("runs"), 0);
    assert_eq!(std::fs::read_to_string(c.lock_path("k")).unwrap(), "busy");
}

#[test]
fn refresh_keeps_foreign_lock() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("seed", "").unwrap();
    std::fs::write(c.lock_path("k"), "someone-else").unwrap();
    let req = Request {
        key: "k".into(),
        job: cmd_job("echo v"),
        lock: "mine".into(),
    };
    c.refresh(&req, Duration::from_secs(30));
    assert_eq!(
        std::fs::read_to_string(c.lock_path("k")).unwrap(),
        "someone-else"
    );
}

#[test]
fn output_capped_at_64k() {
    let t = Tmp::new();
    let c = t.cache();
    let req = Request {
        key: "k".into(),
        job: cmd_job("head -c 200000 /dev/zero | tr '\\0' a"),
        lock: "t".into(),
    };
    c.refresh(&req, Duration::from_secs(30));
    let stored = std::fs::read_to_string(c.path_for("k")).unwrap();
    assert_eq!(stored.len(), hop_bar::cache::MAX_VALUE);
    assert_eq!(hop_bar::cache::MAX_VALUE, 64 * 1024);
}

#[test]
fn stored_value_read_capped() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", &"b".repeat(100_000)).unwrap();
    let got = c.read_or_refresh("k", 3600, &cmd_job("true")).unwrap();
    assert_eq!(got.len(), 64 * 1024);
}

/// Waits up to 10 s for `p` to hold a line.
fn wait_line(p: &Path) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Ok(s) = std::fs::read_to_string(p) {
            if s.ends_with('\n') {
                return s.trim().to_string();
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no line in {}", p.display());
}

#[test]
fn refresh_runs_outside_the_working_directory() {
    let t = Tmp::new();
    let exe = t.path().join("fake-bar");
    common::exe::write(
        &exe,
        format!(
            "#!/bin/sh\npwd -P > '{}/cwd.tmp' && mv '{0}/cwd.tmp' '{0}/cwd'\n",
            t.path().display()
        ),
    );
    let state = t.path().join("state");
    let cache = Cache::with_exe(&state, exe);
    cache.read_or_refresh("status:summary", 300, &Job::Status { strip: false });
    let cwd = wait_line(&t.path().join("cwd"));
    assert_eq!(Path::new(&cwd), state.canonicalize().unwrap());
    // `%cmd%` keeps the directory it was rendered in
    std::fs::remove_file(t.path().join("cwd")).unwrap();
    cache.read_or_refresh("cmd:x", 300, &cmd_job("true"));
    let cwd = wait_line(&t.path().join("cwd"));
    assert_eq!(
        Path::new(&cwd),
        std::env::current_dir().unwrap().canonicalize().unwrap()
    );
}
