// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::Cache;
use hop_bar::placeholders::gitst::cache_key;
use hop_bar::placeholders::repo::{remote_key, remote_key_named};
use std::path::{Path, PathBuf};

/// Fake `git` that logs every call, alone on PATH, so a spawn from render
/// would show.
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

/// Waits (at most 10 s) until no background refresh of `keys` is in
/// flight: its lock goes once its result is stored, so a value written
/// after this is not overwritten by a refresh the render started.
fn settled(cache: &Cache, keys: &[String]) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while keys.iter().any(|k| cache.lock_path(k).exists()) {
        assert!(std::time::Instant::now() < end, "refresh still running");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn one(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// A fork's clone: `origin` the fork, `upstream` its parent, `main`
/// tracking `origin/main`, branch `feature/very-long` tracking
/// `upstream/dev`.
fn fork(t: &Tmp) -> PathBuf {
    let d = repo(t, "proj");
    git(
        &d,
        &["remote", "add", "origin", "git@github.com:me/proj.git"],
    );
    git(
        &d,
        &["remote", "add", "upstream", "https://github.com/org/proj"],
    );
    git(&d, &["config", "branch.main.remote", "origin"]);
    git(&d, &["config", "branch.main.merge", "refs/heads/main"]);
    d
}

fn render(t: &Tmp, d: &Path, conf: &str, path: &str) -> Out {
    run(t)
        .conf(conf)
        .env("PATH", path)
        .cwd(d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go()
}

#[test]
fn origin_upstream_fork_from_files_without_spawning() {
    let t = Tmp::new();
    let d = fork(&t);
    let (path, log) = no_git(&t);
    let o = render(&t, &d, "set left \"#{origin}|#{upstream}|#{fork}\"", &path);
    assert_eq!(one(&o.plain), "me/proj|origin/main|org/proj");
    assert!(
        o.raw.contains("\x1b]8;;https://github.com/me/proj\x1b\\"),
        "{:?}",
        o.raw
    );
    assert!(
        o.raw.contains("https://github.com/me/proj/tree/main"),
        "{:?}",
        o.raw
    );
    assert!(
        o.raw.contains("\x1b]8;;https://github.com/org/proj\x1b\\"),
        "{:?}",
        o.raw
    );

    // upstream of another remote links to that remote
    git(&d, &["checkout", "-q", "-b", "feature/very-long"]);
    git(
        &d,
        &["config", "branch.feature/very-long.remote", "upstream"],
    );
    git(
        &d,
        &["config", "branch.feature/very-long.merge", "refs/heads/dev"],
    );
    let o = render(&t, &d, "set left \"#{upstream}\"", &path);
    assert_eq!(one(&o.plain), "upstream/dev");
    assert!(
        o.raw.contains("https://github.com/org/proj/tree/dev"),
        "{:?}",
        o.raw
    );

    let o = render(
        &t,
        &d,
        "set left \"#{origin[show=host,owner,repo;link=no]}|#{fork[show=repo]}|#{upstream[link=off;max=5]}\"",
        &path,
    );
    assert_eq!(one(&o.plain), "github.com/me/proj|proj|upst…");
    assert!(!o.raw.contains("me/proj\x1b\\"), "{:?}", o.raw);
    assert!(!o.raw.contains("/tree/"), "{:?}", o.raw);
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn not_a_fork_when_upstream_is_origin_or_missing() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let (path, _) = no_git(&t);
    let conf = "set left \"#{origin} #{fork} #{upstream}\"";
    assert_eq!(one(&render(&t, &d, conf, &path).plain), "");
    git(
        &d,
        &["remote", "add", "origin", "git@github.com:org/proj.git"],
    );
    assert_eq!(one(&render(&t, &d, conf, &path).plain), "org/proj");
    // same repository by another URL form, host case aside
    git(
        &d,
        &["remote", "add", "upstream", "https://GitHub.com/org/proj/"],
    );
    assert_eq!(one(&render(&t, &d, conf, &path).plain), "org/proj");
}

#[test]
fn owner_and_repo_case_does_not_make_a_fork() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let (path, _) = no_git(&t);
    git(
        &d,
        &[
            "remote",
            "add",
            "origin",
            "git@gitlab.example.com:Org/Proj.git",
        ],
    );
    git(
        &d,
        &[
            "remote",
            "add",
            "upstream",
            "https://gitlab.example.com/org/proj",
        ],
    );
    let conf = "set left \"#{fork}\"";
    assert_eq!(one(&render(&t, &d, conf, &path).plain), "");
    git(
        &d,
        &[
            "remote",
            "set-url",
            "upstream",
            "https://gitlab.example.com/other/proj",
        ],
    );
    assert_eq!(one(&render(&t, &d, conf, &path).plain), "other/proj");
}

#[test]
fn rewritten_config_reads_cached_git() {
    let t = Tmp::new();
    let d = fork(&t);
    let cfg = d.join(".git/config");
    let body = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(
        &cfg,
        format!("{body}[url \"https://mirror.io/\"]\n\tinsteadOf = gh:\n"),
    )
    .unwrap();
    let (path, log) = no_git(&t);
    let conf = "set left \"#{origin} #{fork} #{upstream}\"";
    // cold: no remote known yet; upstream still from the files
    let o = render(&t, &d, conf, &path);
    assert_eq!(one(&o.plain), "origin/main");
    let cache = Cache::new(&t.path().join("state"));
    // the cold render's refresh (git fails: upstream stored as absent)
    // must not land on top of the values stored below
    settled(&cache, &[remote_key(&d), remote_key_named(&d, "upstream")]);
    cache
        .store(&remote_key(&d), "https://mirror.io/me/proj.git\n")
        .unwrap();
    cache
        .store(
            &remote_key_named(&d, "upstream"),
            "https://mirror.io/org/proj\n",
        )
        .unwrap();
    let o = render(&t, &d, conf, &path);
    assert_eq!(one(&o.plain), "me/proj org/proj origin/main");
    assert!(
        o.raw.contains("https://mirror.io/me/proj/tree/main"),
        "{:?}",
        o.raw
    );
    // no upstream remote, as the background job stores it
    cache.store(&remote_key_named(&d, "upstream"), "").unwrap();
    assert_eq!(
        one(&render(&t, &d, conf, &path).plain),
        "me/proj origin/main"
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    let spawned = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned
            .lines()
            .all(|l| l == "remote get-url origin" || l == "remote get-url upstream"),
        "{spawned:?}"
    );
}

#[test]
fn upstream_from_cached_status_when_config_needs_git() {
    let t = Tmp::new();
    let d = fork(&t);
    let cfg = d.join(".git/config");
    let body = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, format!("{body}[include]\n\tpath = extra\n")).unwrap();
    let cache = Cache::new(&t.path().join("state"));
    cache
        .store(
            &cache_key(&d),
            "# branch.head main\n# branch.upstream origin/trunk\n",
        )
        .unwrap();
    let (path, log) = no_git(&t);
    let o = render(&t, &d, "set left \"#{upstream}\"", &path);
    assert_eq!(one(&o.plain), "origin/trunk");
    assert!(!o.raw.contains("\x1b]8;;http"), "{:?}", o.raw);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let spawned = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned.lines().all(|l| l.starts_with("-c core.fsmonitor")),
        "{spawned:?}"
    );
}

#[test]
fn fish_branch_keeps_the_full_name_in_the_link() {
    let t = Tmp::new();
    let d = fork(&t);
    git(&d, &["checkout", "-q", "-b", "feature/sub/very-long"]);
    let (path, _) = no_git(&t);
    let o = render(
        &t,
        &d,
        "set left \"#{branch[fish=1]}|#{branch[fish=2;full=2]}|#{branch[fish=1;max=6]}\"",
        &path,
    );
    assert_eq!(one(&o.plain), "f/s/very-long|fe/sub/very-long|f/s/v…");
    assert!(
        o.raw
            .contains("https://github.com/me/proj/tree/feature/sub/very-long"),
        "{:?}",
        o.raw
    );
}

#[test]
fn fish_and_depth_paths() {
    let t = Tmp::new();
    let d = t.path().join("src/.hidden/機能/proj");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    let sub = d.join("docs/deep");
    std::fs::create_dir_all(&sub).unwrap();
    let (path, _) = no_git(&t);
    let o = run(&t)
        .conf(
            "set left \"#{cwd[fish=1]}|#{cwd[depth=2]}|#{root[fish=1]}|#{cwd[fish=1;depth=3]}|#{root[base=1;fish=1]}\"",
        )
        .env("PATH", &path)
        .stdin(&format!(
            r#"{{"cwd":"{0}","workspace":{{"current_dir":"{0}"}}}}"#,
            sub.display()
        ))
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(
        one(&o.plain),
        "~/s/.h/機/p/d/deep|…/docs/deep|~/s/.h/機/proj|…/p/d/deep|proj"
    );
}

#[test]
fn check_validates_new_options() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{origin[show=host,nope]} #{branch[fish=x]} #{cwd[depth=-1]} #{upstream[max=3]} #{fork}\"\n",
    );
    let (iso, cwd) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .arg("check")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text.contains("show"), "{text}");
    assert!(text.contains("fish"), "{text}");
    assert!(text.contains("depth"), "{text}");
    assert!(!text.contains("upstream"), "{text}");
    assert!(!text.contains("fork"), "{text}");
}
