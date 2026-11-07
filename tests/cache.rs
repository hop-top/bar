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
        let p = std::env::temp_dir().join(format!(
            "bar-cache-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bar"))
}

fn cmd_job(cmd: &str) -> Job {
    Job::Command {
        cmd: cmd.to_string(),
        timeout: 3,
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

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
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
    let got = c.read_or_refresh("k", 60, &counting(&t, "runs", "echo new"));
    assert_eq!(got.as_deref(), Some("old"));
    assert!(wait_until(Duration::from_secs(5), || {
        c.read_or_refresh("k", 60, &cmd_job("true")).as_deref() == Some("new\n")
    }));
    assert_eq!(t.lines("runs"), 1);
}

#[test]
fn failed_refresh_keeps_last_value() {
    let t = Tmp::new();
    let c = t.cache();
    c.store("k", "old").unwrap();
    age(&c.path_for("k"), 120);
    let job = counting(&t, "runs", "exit 1");
    assert_eq!(c.read_or_refresh("k", 60, &job).as_deref(), Some("old"));
    assert!(wait_until(Duration::from_secs(5), || t.lines("runs") == 1
        && lock_gone(&c, "k")));
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
    assert!(wait_until(Duration::from_secs(6), || lock_gone(&c, "k")
        && c.path_for("k").exists()));
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
    let job = Job::Command {
        cmd: format!("sleep 3; touch '{}'; echo late", marker.display()),
        timeout: 60,
    };
    let req = Request {
        key: "k".into(),
        job,
    };
    let start = Instant::now();
    let out = std::process::Command::new(bin())
        .arg("refresh")
        .arg(req.to_json())
        .env("BAR_STATE_DIR", t.path())
        .env("BAR_REFRESH_TIMEOUT", "1")
        .output()
        .unwrap();
    let took = start.elapsed();
    assert!(out.status.success());
    assert!(took < Duration::from_millis(2500), "took {took:?}");
    std::thread::sleep(Duration::from_millis(3500) - took.min(Duration::from_secs(3)));
    assert!(!marker.exists(), "job's process group was not killed");
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
    std::env::set_var("BAR_TEST_SECRET_TOKEN", secret);
    let req = Request {
        key: "expand:echo $BAR_TEST_SECRET_TOKEN".into(),
        job: cmd_job("echo $BAR_TEST_SECRET_TOKEN"),
    };
    let json = req.to_json();
    assert!(!json.contains(secret), "{json}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut top: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    top.sort();
    assert_eq!(top, ["job", "key"]);
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
    assert!(wait_until(Duration::from_secs(5), || r
        .run(&cmd)
        .as_deref()
        == Some("hi\n")));
    assert!(wait_until(Duration::from_secs(2), || lock_gone(&c, &key)));
    age(&c.path_for(&key), 3500);
    assert_eq!(r.run(&cmd).as_deref(), Some("hi\n"));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(t.lines("runs"), 1, "still fresh before an hour");
    age(&c.path_for(&key), 3700);
    assert_eq!(r.run(&cmd).as_deref(), Some("hi\n"));
    assert!(wait_until(Duration::from_secs(5), || t.lines("runs") == 2));
}

#[test]
fn render_returns_within_50ms_with_slow_job() {
    let t = Tmp::new();
    let cfg_dir = t.path().join("cfg/bar");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("bar.conf"),
        "set left \"#{repo[max=%sleep 5; echo 9%]}\"\n",
    )
    .unwrap();
    let run = || {
        let start = Instant::now();
        let out = std::process::Command::new(bin())
            .arg("render")
            .env("HOME", t.path())
            .env("XDG_CONFIG_HOME", t.path().join("cfg"))
            .env("BAR_STATE_DIR", t.path().join("state"))
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
        state.lock_path("expand:sleep 5; echo 9").exists(),
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
        left: "#{repo[max=%sleep 5; echo 8%]}".into(),
        ..hop_bar::conf::Config::default()
    };
    let session = hop_bar::session::parse("{}");
    let r = CachedRunner::new(Cache::with_exe(&t.path().join("state2"), bin()));
    let start = Instant::now();
    hop_bar::render::render(&session, &cfg, Some(120), &r);
    let took = start.elapsed();
    assert!(took < Duration::from_millis(50), "render took {took:?}");
    let state2 = Cache::with_exe(&t.path().join("state2"), bin());
    assert!(state2.lock_path("expand:sleep 5; echo 8").exists());
}
