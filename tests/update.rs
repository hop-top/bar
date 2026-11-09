//! Opt-in update check: `#{update}`, `set update-check`, `bar update`.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use hop_bar::cache::{Cache, Job, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::update::Update;
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::update;
use render_fx::Tmp;
use std::time::{Duration, Instant};

fn go(cache: &Cache, cfg: &Config) -> Option<String> {
    let s = Session::default();
    let ctx = Ctx {
        session: &s,
        config: cfg,
        now: 0,
        runner: &ColdRunner,
        cache: Some(cache),
        columns: None,
        plugin_dirs: &[],
    };
    Update
        .render(&ctx, &Options::default())
        .map(|o| hop_bar::style::strip_codes(&o))
}

fn key() -> &'static str {
    update::cache_key(update::channel_is_prerelease(update::current()))
}

#[test]
fn off_by_default_and_touches_nothing() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    cache.store(key(), "v999.0.0").unwrap();
    assert!(!Config::default().update_check);
    assert_eq!(go(&cache, &Config::default()), None);
    // nothing locked or spawned for the check
    assert!(!cache.lock_path(key()).exists());
}

#[test]
fn shows_a_newer_release_only() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let cfg = parse("set update-check on\n").unwrap();
    assert!(cfg.update_check);
    cache.store(key(), "v999.0.0\n").unwrap();
    // a development build (0.0.0) is not compared with releases
    let want = (!dev()).then_some("↑ 999.0.0");
    assert_eq!(go(&cache, &cfg).as_deref(), want);
    cache.store(key(), update::current()).unwrap();
    assert_eq!(go(&cache, &cfg), None);
    // a failed check is stored empty
    cache.store(key(), "").unwrap();
    assert_eq!(go(&cache, &cfg), None);
}

#[test]
fn config_values_and_check() {
    assert!(!parse("set update-check off\n").unwrap().update_check);
    let e = parse("set update-check yes\n").unwrap_err();
    assert!(e[0].message.contains("update-check"), "{e:?}");
    let r = hop_bar::check::check_text("set left \"#{update}\"\n", None, &[]);
    assert!(!r.has_errors(), "{}", r.to_text());
    let r = hop_bar::check::check_text("set left \"#{update[x=1]}\"\n", None, &[]);
    assert!(r.has_errors());
}

#[test]
fn job_round_trips() {
    for prerelease in [false, true] {
        let req = Request {
            key: "update:stable".into(),
            job: Job::Update { prerelease },
            lock: "l".into(),
        };
        assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    }
}

fn bar(t: &Tmp, args: &[&str], stdin: &str) -> std::process::Output {
    let (envs, cwd) = common::isolation(t.path());
    Command::cargo_bin("bar")
        .unwrap()
        .args(args)
        .envs(envs)
        .current_dir(cwd)
        .write_stdin(stdin)
        .output()
        .unwrap()
}

#[test]
fn render_starts_a_background_check_that_records_a_failure() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set update-check on\nset left \"#{update}\"\n",
    );
    let o = bar(&t, &["render"], "{}");
    assert!(o.status.success());
    // the fake curl fails: the job stores an empty result
    let cache = Cache::new(&t.path().join("state"));
    let path = cache.path_for(key());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    assert!(!cache.fail_path(key()).exists());
}

#[test]
fn bar_update_reports() {
    let t = Tmp::new();
    let o = bar(&t, &["update"], "");
    assert!(o.status.success());
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.starts_with(&format!("bar {}\n", update::current())),
        "{out}"
    );
    assert!(out.contains("update check off"), "{out}");
    assert!(out.contains("upgrade: "), "{out}");
    // nothing fetched while off
    assert!(!Cache::new(&t.path().join("state")).path_for(key()).exists());

    t.write("xdg/bar/bar.conf", "set update-check on\n");
    Cache::new(&t.path().join("state"))
        .store(key(), "v999.0.0")
        .unwrap();
    let o = bar(&t, &["update"], "");
    let out = String::from_utf8_lossy(&o.stdout);
    if dev() {
        assert!(out.contains("development build"), "{out}");
        assert!(out.contains("release: v999.0.0"), "{out}");
    } else {
        assert!(out.contains("release: 999.0.0"), "{out}");
    }
}

/// Whether this is a development build, version `0.0.0`.
fn dev() -> bool {
    update::current() == "0.0.0"
}
