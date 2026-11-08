//! Windows behaviour: `PATHEXT` plugins, `cmd /C` values, detached
//! refreshes, process trees, home and zone lookups.
#![cfg(windows)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hop_bar::cache::{Cache, Job};
use hop_bar::plugin;

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-win-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
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

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

const HELLO_CMD: &str = "@echo off\r\n\
    if \"%1\"==\"--ext-info\" (\r\n\
    echo {\"name\":\"hello\",\"version\":\"1.0.0\"}\r\n\
    ) else (\r\n\
    echo {\"text\":\"hi\"}\r\n\
    )\r\n";

#[test]
fn cmd_plugin_is_discovered_without_its_extension() {
    let t = Tmp::new();
    std::fs::write(t.path().join("bar-hello.cmd"), HELLO_CMD).unwrap();
    std::fs::write(t.path().join("bar-skipped.ps1"), "'x'").unwrap();
    std::fs::write(t.path().join("bar-noext"), "x").unwrap();
    let dirs = vec![t.path().to_path_buf()];
    let found = plugin::discover(&dirs);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].name, "hello");
    assert_eq!(found[0].path, t.path().join("bar-hello.cmd"));
    assert_eq!(plugin::find(&dirs, "hello").unwrap().path, found[0].path);
    assert!(plugin::find(&dirs, "skipped").is_none());
}

#[test]
fn exe_beats_cmd_in_the_same_dir() {
    let t = Tmp::new();
    std::fs::write(t.path().join("bar-dup.cmd"), HELLO_CMD).unwrap();
    std::fs::copy(bin(), t.path().join("bar-dup.exe")).unwrap();
    let found = plugin::discover(&[t.path().to_path_buf()]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, t.path().join("bar-dup.exe"));
}

#[test]
fn cmd_plugin_runs_for_info_and_render() {
    let t = Tmp::new();
    let p = t.path().join("bar-hello.cmd");
    std::fs::write(&p, HELLO_CMD).unwrap();
    let path = p.to_string_lossy().into_owned();
    let info = Job::PluginInfo { path: path.clone() }.execute(Duration::from_secs(10));
    assert!(info.is_some_and(|o| o.contains("\"hello\"")));
    let out = Job::PluginRender {
        path,
        input: "{}".into(),
    }
    .execute(Duration::from_secs(10));
    assert!(out.is_some_and(|o| o.contains("\"hi\"")));
}

#[test]
fn command_values_run_through_cmd() {
    let run = |cmd: &str, secs| {
        Job::Command {
            cmd: cmd.into(),
            timeout: secs,
        }
        .execute(Duration::from_secs(secs))
    };
    assert_eq!(
        run("echo hello", 5).as_deref().map(str::trim),
        Some("hello")
    );
    assert_eq!(run("echo a& echo b", 5).map(|o| o.lines().count()), Some(2));
    assert_eq!(run("exit /b 1", 5), None);
}

#[test]
fn timed_out_command_is_killed_with_its_children() {
    let start = Instant::now();
    let out = Job::Command {
        cmd: "ping -n 30 127.0.0.1 >NUL & echo late".into(),
        timeout: 1,
    }
    .execute(Duration::from_secs(1));
    assert_eq!(out, None);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn detached_refresh_stores_the_value() {
    let t = Tmp::new();
    let cache = Cache::with_exe(&t.path().join("state"), bin());
    let job = Job::Command {
        cmd: "echo cached".into(),
        timeout: 5,
    };
    assert_eq!(cache.read_or_refresh("k", 60, &job), None);
    let stored = cache.path_for("k");
    assert!(wait_until(Duration::from_secs(15), || stored.exists()));
    assert_eq!(
        cache
            .read_or_refresh("k", 60, &job)
            .as_deref()
            .map(str::trim),
        Some("cached")
    );
    assert!(wait_until(Duration::from_secs(5), || !cache
        .lock_path("k")
        .exists()));
}

/// The refresh must not hold a copy of the pipe the caller reads: reading
/// the render's output to its end would otherwise wait for the slow job.
#[test]
fn render_output_ends_before_its_slow_refresh() {
    let t = Tmp::new();
    let cfg = t.path().join("cfg").join("bar");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("bar.conf"),
        "set left \"#{model[click=%ping -n 6 127.0.0.1 >NUL & echo 9%]}\"\n",
    )
    .unwrap();
    let run = || {
        let (iso, cwd) = common::isolation(t.path());
        let start = Instant::now();
        let out = Command::new(bin())
            .arg("render")
            .envs(iso)
            .env("XDG_CONFIG_HOME", t.path().join("cfg"))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success());
        start.elapsed()
    };
    run();
    let cache = Cache::with_exe(&t.path().join("state"), bin());
    assert!(
        cache
            .lock_path("expand:ping -n 6 127.0.0.1 >NUL & echo 9")
            .exists(),
        "render spawned the slow refresh"
    );
    let best = (0..3).map(|_| run()).min().unwrap();
    assert!(best < Duration::from_secs(2), "bar render took {best:?}");
}

#[test]
fn init_writes_under_the_profile_dir() {
    let t = Tmp::new();
    let out = Command::new(bin())
        .arg("init")
        .env("USERPROFILE", t.path())
        .env("LOCALAPPDATA", t.path().join("local"))
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(t
        .path()
        .join(".config")
        .join("bar")
        .join("bar.conf")
        .is_file());
}

#[test]
fn click_run_is_refused_cleanly() {
    let out = Command::new(bin())
        .args(["click", "run", "bar://cmd/clear?tty=ttys001&sid=x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not supported on Windows"));
}

#[test]
fn click_install_without_hooks_only_is_unsupported() {
    let t = Tmp::new();
    let out = Command::new(bin())
        .args(["click", "install", "--dry-run"])
        .env("CLAUDE_CONFIG_DIR", t.path().join("claude"))
        .env("BAR_STATE_DIR", t.path().join("state"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--hooks-only"));
}

#[test]
fn hook_command_uses_forward_slashes() {
    let cmd = hop_bar::click::hook_command(Path::new(r"C:\Users\me\bin\bar.exe"));
    assert_eq!(cmd, "C:/Users/me/bin/bar.exe click hook");
}

#[test]
fn local_offset_is_whole_minutes_within_a_day() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    for ts in [now, 0, 1_700_000_000, 1_720_000_000] {
        let off = hop_bar::tz::local_offset(ts);
        assert_eq!(off % 60, 0, "{off}");
        assert!(off.abs() <= 14 * 3600, "{off}");
    }
}

#[test]
fn pid_alive_sees_this_process_and_not_an_exited_one() {
    assert!(hop_bar::platform::pid_alive(u64::from(std::process::id())));
    let mut c = Command::new("cmd.exe")
        .args(["/C", "exit"])
        .spawn()
        .unwrap();
    let pid = c.id();
    c.wait().unwrap();
    // the handle `c` still holds keeps the pid from being reused
    assert!(!hop_bar::platform::pid_alive(u64::from(pid)));
    assert!(!hop_bar::platform::pid_alive(0));
}

#[test]
fn home_abbreviation_handles_backslashes_and_case() {
    use hop_bar::placeholders::dirs::{abbrev_home, shown};
    assert_eq!(abbrev_home(r"C:\Users\me\w\x", r"C:\Users\me"), r"~\w\x");
    assert_eq!(abbrev_home(r"c:\users\ME\w", r"C:\Users\me\"), r"~\w");
    assert_eq!(abbrev_home(r"C:\Users\me", r"C:\Users\me"), "~");
    assert_eq!(
        abbrev_home(r"C:\Users\mex\w", r"C:\Users\me"),
        r"C:\Users\mex\w"
    );
    let mut opts = hop_bar::options::Options::default();
    opts.set("base", "1");
    let s = shown(r"C:\Users\me\w\proj\", &opts, r"C:\Users\me").unwrap();
    assert!(s.contains("proj") && !s.contains('\\'), "{s:?}");
}
