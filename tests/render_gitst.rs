mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::Cache;
use hop_bar::placeholders::gitst::cache_key;

const MIXED: &str = "# branch.head main\n# branch.ab +2 -1\n1 M. N... 1 1 1 a b s\n1 .M N... 1 1 1 a b m\n1 .M N... 1 1 1 a b n\n1 .M N... 1 1 1 a b o\n? u\n";

/// Fresh cache entry for `d`; fake `git` that logs every call, PATH holding
/// nothing else, so a spawn from render would show.
fn seed(t: &Tmp, d: &std::path::Path, body: &str) -> std::path::PathBuf {
    let cache = Cache::new(&t.path().join("state"));
    cache.store(&cache_key(d), body).unwrap();
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
    log
}

fn only_path(t: &Tmp) -> String {
    t.path().join("git-bin").display().to_string()
}

#[test]
fn dirty_and_ahead_from_cache_without_spawning() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let log = seed(&t, &d, MIXED);
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty} #{ahead}\"")
        .env("PATH", &only_path(&t))
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "proj +1 ~3 ?1 ↑2 ↓1");
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn show_subset() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, MIXED);
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty[show=staged,untracked]}\"")
        .env("PATH", &only_path(&t))
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "proj +1 ?1");
}

#[test]
fn clean_and_even_are_hidden() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, "# branch.head main\n# branch.ab +0 -0\n");
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty} #{ahead}\"")
        .env("PATH", &only_path(&t))
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "proj");
}

#[test]
fn ahead_only_one_side_and_no_upstream() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    seed(&t, &d, "# branch.head main\n# branch.ab +0 -3\n");
    let run1 = |t: &Tmp| {
        run(t)
            .conf("set left \"#{repo} #{ahead}\"")
            .env("PATH", &only_path(t))
            .cwd(&d)
            .unset("GIT_CONFIG_NOSYSTEM")
            .go()
            .plain
    };
    assert_eq!(one(&run1(&t)), "proj ↓3");
    Cache::new(&t.path().join("state"))
        .store(&cache_key(&d), "# branch.head main\n")
        .unwrap();
    assert_eq!(one(&run1(&t)), "proj");
}

#[test]
fn cold_cache_renders_empty_outside_repo_too() {
    let t = Tmp::new();
    let d = t.path().join("plain");
    std::fs::create_dir_all(&d).unwrap();
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty} #{ahead} #{worktree}\"")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "plain");
}

#[test]
fn worktree_name_only_in_linked_worktree() {
    let t = Tmp::new();
    let d = repo(&t, "main");
    let wt = t.path().join("wt-one");
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "w"],
    );
    let conf = "set left \"#{branch} #{worktree}\"";
    let o = run(&t)
        .conf(conf)
        .env("PATH", "/nonexistent")
        .cwd(&wt)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "w wt-one");
    let o = run(&t)
        .conf(conf)
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "main");
}

#[test]
fn ttl_option_accepted_by_check() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{dirty[ttl=30,show=staged]}#{ahead[ttl=5]}#{worktree}\"\n",
    );
    let (iso, cwd) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .arg("check")
        .assert()
        .success();
    let out = String::from_utf8_lossy(&o.get_output().stdout).to_string();
    assert!(!out.contains("unknown"), "{out}");
}

#[test]
fn check_rejects_bad_show() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set left \"#{dirty[show=bogus]}\"\n");
    let (iso, cwd) = common::isolation(t.path());
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .arg("check")
        .assert()
        .failure();
}

fn one(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn stale_status_served_while_refresh_runs_git() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let log = seed(&t, &d, MIXED);
    let file = Cache::new(&t.path().join("state")).path_for(&cache_key(&d));
    let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(100))
        .unwrap();
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty}\"")
        .env("PATH", &only_path(&t))
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "proj +1 ~3 ?1");
    // the detached `bar refresh` runs the fake git
    let mut calls = String::new();
    for _ in 0..100 {
        calls = std::fs::read_to_string(&log).unwrap_or_default();
        if !calls.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        calls.contains("-c core.fsmonitor=false status --porcelain=v2 --branch"),
        "{calls:?}"
    );
}

#[test]
fn truncated_status_marks_lower_bound() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let mut body = String::from("# branch.head main\n");
    while body.len() < hop_bar::cache::MAX_VALUE {
        body.push_str("? some/untracked/file.txt\n");
    }
    seed(&t, &d, &body);
    let o = run(&t)
        .conf("set left \"#{repo} #{dirty}\"")
        .env("PATH", &only_path(&t))
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    let out = one(&o.plain);
    assert!(out.starts_with("proj ?") && out.ends_with('+'), "{out}");
}
