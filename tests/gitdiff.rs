// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::gitfs::{discover, Found, Repo};
use hop_bar::placeholders::gitdiff::{
    cache_key, decode, encode, parse_numstat, stamp, valid_stamp, Totals,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn open(d: &Path) -> Repo {
    match discover(d) {
        Found::Repo(r) => r,
        other => panic!("{other:?}"),
    }
}

fn job(d: &Path, scope: &str) -> Totals {
    let s = stamp(&open(d));
    let out = Job::GitDiff {
        dir: d.to_string_lossy().into(),
        scope: scope.into(),
        stamp: s.clone(),
    }
    .execute(Duration::from_secs(20))
    .expect("diff output");
    decode(&out, &s).expect("decodes under its stamp")
}

fn lines(t: &Totals) -> (u64, u64, u64) {
    (t.added, t.removed, t.binary)
}

/// `a.txt` with three lines committed, then one line changed and staged,
/// and one more appended in the worktree.
fn mixed(t: &Tmp) -> PathBuf {
    let d = repo(t, "proj");
    std::fs::write(d.join("a.txt"), "1\n2\n3\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-q", "-m", "a"]);
    std::fs::write(d.join("a.txt"), "1\nTWO\n3\n").unwrap();
    git(&d, &["add", "a.txt"]);
    std::fs::write(d.join("a.txt"), "1\nTWO\n3\n4\n").unwrap();
    d
}

#[test]
fn numstat_totals_skip_binary_and_junk() {
    let t = parse_numstat("3\t1\ta.rs\n-\t-\timg.png\n10\t0\told => new\nx\ty\tz\nnope\n");
    assert_eq!(lines(&t), (13, 1, 1));
    assert!(!t.cut);
}

#[test]
fn encoded_value_belongs_to_its_stamp() {
    let t = Totals {
        added: 5,
        removed: 2,
        binary: 1,
        cut: true,
    };
    let raw = encode("abc|1:2:3", &t);
    assert_eq!(decode(&raw, "abc|1:2:3"), Some(t));
    assert_eq!(decode(&raw, "abc|1:2:4"), None);
    assert_eq!(decode("abc|1:2:3\n1 2\n", "abc|1:2:3"), None);
}

#[test]
fn scopes_on_a_real_repository() {
    let t = Tmp::new();
    let d = mixed(&t);
    assert_eq!(lines(&job(&d, "staged")), (1, 1, 0));
    assert_eq!(lines(&job(&d, "unstaged")), (1, 0, 0));
    assert_eq!(lines(&job(&d, "both")), (2, 1, 0));
}

#[test]
fn binary_files_count_no_lines() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    std::fs::write(d.join("b.bin"), b"\x00\x01\x02\n").unwrap();
    git(&d, &["add", "b.bin"]);
    assert_eq!(lines(&job(&d, "staged")), (0, 0, 1));
}

#[test]
fn a_pure_rename_adds_nothing() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    std::fs::write(d.join("old.txt"), "a\nb\nc\nd\n").unwrap();
    git(&d, &["add", "old.txt"]);
    git(&d, &["commit", "-q", "-m", "old"]);
    git(&d, &["mv", "old.txt", "new.txt"]);
    assert_eq!(lines(&job(&d, "both")), (0, 0, 0));
}

#[test]
fn both_on_an_unborn_branch_diffs_against_the_empty_tree() {
    let t = Tmp::new();
    let d = t.path().join("fresh");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "1\n2\n").unwrap();
    git(&d, &["add", "a.txt"]);
    std::fs::write(d.join("a.txt"), "1\n2\n3\n").unwrap();
    assert!(stamp(&open(&d)).starts_with("-|"));
    assert_eq!(lines(&job(&d, "both")), (3, 0, 0));
    assert_eq!(lines(&job(&d, "staged")), (2, 0, 0));
}

#[test]
fn reftable_head_unknown_to_the_files_still_diffs() {
    let t = Tmp::new();
    let d = t.path().join("rt");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main", "--ref-format=reftable"]);
    std::fs::write(d.join("a.txt"), "1\n").unwrap();
    git(&d, &["add", "a.txt"]);
    // unborn: HEAD fails, the empty tree is used
    assert!(stamp(&open(&d)).starts_with("?|"));
    assert_eq!(lines(&job(&d, "both")), (1, 0, 0));
    git(&d, &["commit", "-q", "-m", "a"]);
    std::fs::write(d.join("a.txt"), "2\n").unwrap();
    assert_eq!(lines(&job(&d, "both")), (1, 1, 0));
}

#[test]
fn linked_worktree_uses_its_own_index() {
    let t = Tmp::new();
    let d = mixed(&t);
    let wt = t.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "f"],
    );
    std::fs::write(wt.join("a.txt"), "1\n2\n3\n4\n5\n").unwrap();
    git(&wt, &["add", "a.txt"]);
    let (main, linked) = (stamp(&open(&d)), stamp(&open(&wt)));
    assert_ne!(main.split('|').nth(1), linked.split('|').nth(1));
    assert_eq!(lines(&job(&wt, "staged")), (2, 0, 0));
    assert_eq!(lines(&job(&d, "staged")), (1, 1, 0));
}

#[test]
fn the_job_never_rewrites_the_index() {
    let t = Tmp::new();
    let d = mixed(&t);
    std::fs::write(d.join("same.txt"), "s\n").unwrap();
    git(&d, &["add", "same.txt"]);
    git(&d, &["commit", "-q", "-m", "same"]);
    // unchanged content, new stat info: a plain `git diff` writes the
    // refreshed entry back to the index
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(d.join("same.txt"), "s\n").unwrap();
    let before = stamp(&open(&d));
    for scope in ["both", "staged", "unstaged"] {
        job(&d, scope);
    }
    assert_eq!(stamp(&open(&d)), before);
}

#[test]
fn stamp_follows_commit_and_index() {
    let t = Tmp::new();
    let d = mixed(&t);
    let s1 = stamp(&open(&d));
    git(&d, &["commit", "-q", "--allow-empty", "-m", "e"]);
    let s2 = stamp(&open(&d));
    assert_ne!(s1.split('|').next(), s2.split('|').next());
    std::thread::sleep(Duration::from_millis(20));
    git(&d, &["add", "a.txt"]);
    let s3 = stamp(&open(&d));
    assert_ne!(s2, s3);
    assert!([&s1, &s2, &s3].iter().all(|s| valid_stamp(s)));
}

#[test]
fn job_request_checks_scope_and_stamp() {
    let ok = Request {
        key: cache_key(Path::new("/r"), "both"),
        job: Job::GitDiff {
            dir: "/r".into(),
            scope: "staged".into(),
            stamp: "-|12:34:56".into(),
        },
        lock: "t".into(),
    };
    assert_eq!(Request::from_json(&ok.to_json()).unwrap(), ok);
    for (scope, stamp) in [
        ("all", "-|1"),
        ("both", "a b"),
        ("both", ""),
        ("both", "x\ny"),
    ] {
        let bad = Request {
            job: Job::GitDiff {
                dir: "/r".into(),
                scope: scope.into(),
                stamp: stamp.into(),
            },
            ..ok.clone()
        };
        assert!(
            Request::from_json(&bad.to_json()).is_err(),
            "{scope} {stamp:?}"
        );
    }
}

/// Fake `git` that logs every call, alone on PATH, so a spawn would show.
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

fn seed(t: &Tmp, d: &Path, scope: &str, stamp: &str, totals: &Totals) {
    let cache = Cache::new(&t.path().join("state"));
    let top = d.canonicalize().unwrap();
    cache
        .store(&cache_key(&top, scope), &encode(stamp, totals))
        .unwrap();
}

fn totals(added: u64, removed: u64, cut: bool) -> Totals {
    Totals {
        added,
        removed,
        binary: 0,
        cut,
    }
}

fn render(t: &Tmp, d: &Path, path: &str, left: &str) -> String {
    let conf = format!("set left \"{left}\"");
    let o = run(t)
        .conf(&conf)
        .env("PATH", path)
        .cwd(d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    one(&o.plain)
}

#[test]
fn renders_from_the_cache_without_spawning() {
    let t = Tmp::new();
    let d = mixed(&t);
    let s = stamp(&open(&d));
    seed(&t, &d, "both", &s, &totals(12, 3, false));
    seed(&t, &d, "staged", &s, &totals(1, 0, true));
    let (path, log) = no_git(&t);
    let out = render(
        &t,
        &d,
        &path,
        "#{repo} #{diff} #{diff[scope=staged]} #{diff[show=removed]}",
    );
    assert_eq!(out, "proj +12 -3 +1+ -3");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn zero_and_bad_options_render_nothing() {
    let t = Tmp::new();
    let d = mixed(&t);
    let s = stamp(&open(&d));
    seed(&t, &d, "both", &s, &totals(0, 0, false));
    seed(&t, &d, "unstaged", &s, &totals(0, 4, false));
    let (path, _) = no_git(&t);
    let out = render(
        &t,
        &d,
        &path,
        "#{repo} #{diff} #{diff[scope=nope]} #{diff[scope=unstaged,show=added]}",
    );
    assert_eq!(out, "proj");
}

#[test]
fn outside_a_repository_renders_nothing() {
    let t = Tmp::new();
    let d = t.path().join("plain");
    std::fs::create_dir_all(&d).unwrap();
    let (path, log) = no_git(&t);
    assert_eq!(render(&t, &d, &path, "#{cwd[base=1]} #{diff}"), "plain");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!log.exists());
}

/// After a `git add` the stored value no longer matches the index: it is
/// still shown, and a refresh starts at once instead of after the ttl.
#[test]
fn a_new_index_refreshes_before_the_ttl() {
    let t = Tmp::new();
    let d = mixed(&t);
    let old = stamp(&open(&d));
    seed(&t, &d, "both", &old, &totals(9, 9, false));
    std::thread::sleep(Duration::from_millis(20));
    git(&d, &["add", "a.txt"]);
    let now = stamp(&open(&d));
    assert_ne!(old, now);
    let (path, log) = no_git(&t);
    assert_eq!(render(&t, &d, &path, "#{diff[ttl=3600]}"), "+9 -9");
    let start = std::time::Instant::now();
    while !log.exists() && start.elapsed() < Duration::from_secs(150) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let calls = std::fs::read_to_string(&log).expect("refresh ran git");
    assert!(calls.contains("diff-index HEAD --numstat"), "{calls}");
}

fn one(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
