// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::placeholders::gitage::{cache_key, committer_time, decode, fmt, score};
use std::path::{Path, PathBuf};
use std::process::Command as Std;
use std::time::Duration;

fn head(d: &Path) -> String {
    let o = Std::new("git")
        .arg("-C")
        .arg(d)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn commit_at(d: &Path, when: &str) {
    let st = Std::new("git")
        .args(["-c", "user.name=a b <c>", "-c", "user.email=t@t", "-C"])
        .arg(d)
        .args(["commit", "-q", "--allow-empty", "-m", "dated"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_DATE", "@1000000000 +0000")
        .env("GIT_COMMITTER_DATE", when)
        .status()
        .unwrap();
    assert!(st.success());
}

#[test]
fn committer_line_read_from_the_header_only() {
    let body = "tree 4b82\nauthor A <a@b> 1 +0000\ncommitter Odd Name <x> 3 <y> 1700000000 -0500\n\ncommitter fake 5 +0000\n";
    assert_eq!(committer_time(body), Some(1_700_000_000));
    assert_eq!(committer_time("tree x\n\ncommitter a <b> 9 +0000\n"), None);
    assert_eq!(committer_time("committer a <b> soon +0000\n"), None);
}

#[test]
fn largest_whole_unit() {
    let cases = [
        (-5, "0s"),
        (0, "0s"),
        (59, "59s"),
        (60, "1m"),
        (3599, "59m"),
        (7200, "2h"),
        (86_399, "23h"),
        (3 * 86_400, "3d"),
        (29 * 86_400, "29d"),
        (95 * 86_400, "3mo"),
        (800 * 86_400, "2y"),
    ];
    for (secs, want) in cases {
        assert_eq!(fmt(secs), want, "{secs}");
    }
}

#[test]
fn ramp_score_runs_from_new_to_old() {
    assert_eq!(score(0, 24), 100);
    assert_eq!(score(12 * 3600, 24), 50);
    assert_eq!(score(24 * 3600, 24), 0);
    assert_eq!(score(400 * 3600, 24), 0);
    assert_eq!(score(-60, 24), 100);
    assert_eq!(score(i64::MAX, u64::MAX), 0);
    assert_eq!(score(3600, u64::MAX), 100);
}

#[test]
fn job_reads_committer_time_of_the_commit() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    commit_at(&d, "@1700000000 +0200");
    let id = head(&d);
    let out = Job::GitCommitTime {
        dir: d.to_string_lossy().into(),
        commit: id.clone(),
    }
    .execute(Duration::from_secs(10))
    .expect("commit time");
    assert_eq!(decode(&out, &id), Some(1_700_000_000));
    assert_eq!(decode(&out, &"0".repeat(40)), None);
}

#[test]
fn job_request_needs_a_full_commit_id() {
    let ok = Request {
        key: cache_key(Path::new("/r")),
        job: Job::GitCommitTime {
            dir: "/r".into(),
            commit: "a".repeat(40),
        },
        lock: "t".into(),
    };
    assert_eq!(Request::from_json(&ok.to_json()).unwrap(), ok);
    let bad = Request {
        job: Job::GitCommitTime {
            dir: "/r".into(),
            commit: "HEAD".into(),
        },
        ..ok
    };
    assert!(Request::from_json(&bad.to_json()).is_err());
}

fn no_git(t: &Tmp) -> (String, PathBuf) {
    let bin = t.path().join("git-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = t.path().join("git.log");
    let g = bin.join("git");
    std::fs::write(
        &g,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&g, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (bin.display().to_string(), log)
}

fn seed(t: &Tmp, d: &Path, commit: &str, ago: i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let cache = Cache::new(&t.path().join("state"));
    let top = d.canonicalize().unwrap();
    cache
        .store(&cache_key(&top), &format!("{commit}\n{}\n", now - ago))
        .unwrap();
}

fn render(t: &Tmp, d: &Path, path: &str, left: &str) -> (String, String) {
    let conf = format!("set left \"{left}\"");
    let o = run(t)
        .conf(&conf)
        .env("PATH", path)
        .cwd(d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    (
        o.plain.split_whitespace().collect::<Vec<_>>().join(" "),
        o.raw,
    )
}

#[test]
fn renders_from_the_cache_without_spawning() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, &head(&d), 2 * 3600 + 30);
    let (path, log) = no_git(&t);
    let (plain, _) = render(&t, &d, &path, "#{repo} #{age}");
    assert_eq!(plain, "proj 2h");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn ramp_colours_by_age() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, &head(&d), 3 * 86_400);
    let (path, _) = no_git(&t);
    let conf = "set ramp \"#ff0000 #000001 #000002 #000003 #000004 #000005 #000006 #000007 #000008 #000009 #00ff00\"\nset color-depth truecolor\nset left \"#{age[ramp=24]} #{age[ramp=1000000]}\"";
    let o = run(&t)
        .conf(conf)
        .env("PATH", &path)
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert!(o.raw.contains("38;2;255;0;0m3d"), "{:?}", o.raw);
    assert!(o.raw.contains("38;2;0;255;0m3d"), "{:?}", o.raw);
}

/// A value made for the previous HEAD is never shown; the new commit's
/// time is fetched at once.
#[test]
fn another_commit_s_time_is_not_shown() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, &head(&d), 60);
    commit_at(&d, "@1700000000 +0000");
    let (path, log) = no_git(&t);
    let (plain, _) = render(&t, &d, &path, "#{repo} #{age}");
    assert_eq!(plain, "proj");
    let start = std::time::Instant::now();
    while !log.exists() && start.elapsed() < Duration::from_secs(150) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let calls = std::fs::read_to_string(&log).expect("refresh ran git");
    assert!(
        calls.contains(&format!("cat-file commit {}", head(&d))),
        "{calls}"
    );
}

#[test]
fn background_refresh_fills_the_age_in_a_linked_worktree() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let wt = t.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "f"],
    );
    commit_at(&wt, "@1000000000 +0000");
    let path = std::env::var("PATH").unwrap();
    let mut plain = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(150) {
        plain = render(&t, &wt, &path, "#{worktree} #{age}").0;
        if plain != "wt" {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(plain.starts_with("wt ") && plain.ends_with('y'), "{plain}");
}

#[test]
fn preview_sample_follows_ramp() {
    let draw = |opts: &str| {
        let conf = format!("set left \"#{{age{opts}}}\"\n");
        hop_bar::web::render(&conf, "{}", 120, 0, "")
    };
    let (plain, new, old) = (draw(""), draw("[ramp=48]"), draw("[ramp=1]"));
    assert!(plain.contains("2h"), "{plain:?}");
    assert_ne!(plain, new, "ramp=48 coloured on the ramp");
    assert_ne!(new, old, "a 2h commit is past a 1h ramp");
}
