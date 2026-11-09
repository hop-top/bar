// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::Cache;
use hop_bar::placeholders::gitref::tags_key;
use hop_bar::placeholders::gitst::cache_key;
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

fn write_commit(d: &Path, file: &str, body: &str) {
    std::fs::write(d.join(file), body).unwrap();
    git(d, &["add", file]);
    git(d, &["commit", "-q", "-m", body.trim()]);
}

fn head(d: &Path) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// Mid-merge repo: two conflicted paths, a stash entry, a packed
/// annotated tag at HEAD, a GitHub origin.
fn merging(t: &Tmp) -> PathBuf {
    let d = repo(t, "proj");
    git(
        &d,
        &["remote", "add", "origin", "git@github.com:o/proj.git"],
    );
    write_commit(&d, "a", "base\n");
    write_commit(&d, "b", "base\n");
    git(&d, &["checkout", "-q", "-b", "feat"]);
    write_commit(&d, "a", "feat\n");
    write_commit(&d, "b", "feat\n");
    git(&d, &["checkout", "-q", "main"]);
    write_commit(&d, "a", "main\n");
    write_commit(&d, "b", "main\n");
    git(&d, &["tag", "-a", "-m", "r", "v0.3.0"]);
    git(&d, &["pack-refs", "--all"]);
    std::fs::write(d.join("a"), "wip\n").unwrap();
    git(&d, &["stash", "-q"]);
    let st = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(&d)
        .args(["merge", "-q", "feat"])
        .output()
        .unwrap();
    assert!(!st.status.success());
    d
}

#[test]
fn git_state_from_files_without_spawning() {
    let t = Tmp::new();
    let d = merging(&t);
    let id = head(&d);
    let (path, log) = no_git(&t);
    let o = run(&t)
        .conf("set left \"#{sha} #{tag} #{stash} #{gitop} #{conflicts}\"")
        .env("PATH", &path)
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .unset("GIT_INDEX_FILE")
        .go();
    assert_eq!(one(&o.plain), format!("{} v0.3.0 $1 merge ✗2", &id[..7]));
    assert!(
        o.raw
            .contains(&format!("https://github.com/o/proj/commit/{id}")),
        "{:?}",
        o.raw
    );
    assert!(o.raw.contains("https://github.com/o/proj/tree/v0.3.0"));
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn options_len_link_and_step() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(
        &d,
        &["remote", "add", "origin", "https://github.com/o/proj"],
    );
    let id = head(&d);
    let o = run(&t)
        .conf("set left \"#{sha[len=12,link=no]}|#{sha[len=0]}|#{gitop}|#{stash}|#{conflicts}\"")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    // empty placeholders hide, separators with them
    assert_eq!(one(&o.plain), &id[..12]);
    assert!(!o.raw.contains("/commit/"), "{:?}", o.raw);

    // mid-rebase, step on and off
    write_commit(&d, "a", "base\n");
    git(&d, &["checkout", "-q", "-b", "feat"]);
    write_commit(&d, "a", "feat\n");
    write_commit(&d, "c", "more\n");
    git(&d, &["checkout", "-q", "main"]);
    write_commit(&d, "a", "main\n");
    git(&d, &["checkout", "-q", "feat"]);
    let st = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(&d)
        .args(["rebase", "main"])
        .output()
        .unwrap();
    assert!(!st.status.success());
    let o = run(&t)
        .conf("set left \"#{gitop}|#{gitop[step=off]}|#{conflicts}\"")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "rebase 1/2|rebase|✗1");
}

#[test]
fn loose_annotated_tag_from_cached_git_list() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(&d, &["tag", "-a", "-m", "r", "v1.0.0"]);
    let id = head(&d);
    let (path, log) = no_git(&t);
    let conf = "set left \"[#{tag}]\"";
    // first render: nothing known yet, nothing spawned in render
    let o = run(&t)
        .conf(conf)
        .env("PATH", &path)
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert!(!o.plain.contains("v1.0.0"), "{:?}", o.plain);
    let cache = Cache::new(&t.path().join("state"));
    cache
        .store(&tags_key(&d), &format!("{id}\ntag v1.0.0\ncommit lw\n"))
        .unwrap();
    let o = run(&t)
        .conf(conf)
        .env("PATH", &path)
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "[v1.0.0]");
    // a list made for another commit is not used
    cache
        .store(&tags_key(&d), &format!("{}\ntag old\n", "0".repeat(40)))
        .unwrap();
    let o = run(&t)
        .conf(conf)
        .env("PATH", &path)
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert!(!o.plain.contains("old"), "{:?}", o.plain);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let spawned = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned.lines().all(|l| l.starts_with("for-each-ref")),
        "{spawned:?}"
    );
}

#[test]
fn conflicts_fall_back_to_cached_status() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let cache = Cache::new(&t.path().join("state"));
    cache
        .store(
            &cache_key(&d),
            "# branch.head main\nu UU N... 1 1 1 1 a b c d\nu AA N... 1 1 1 1 a b c e\n",
        )
        .unwrap();
    let (path, log) = no_git(&t);
    let o = run(&t)
        .conf("set left \"#{conflicts}\"")
        .env("PATH", &path)
        .env("GIT_INDEX_FILE", d.join(".git/index").to_str().unwrap())
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "✗2");
    assert!(!log.exists(), "render spawned git");
}

#[test]
fn sha_from_cached_status_when_refs_are_too_large() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let id = head(&d);
    // HEAD's branch only in a packed-refs over the reader's cap
    git(&d, &["pack-refs", "--all"]);
    let packed = d.join(".git/packed-refs");
    let mut body = std::fs::read_to_string(&packed).unwrap();
    let filler: String = (0..20_000)
        .map(|i| format!("{} refs/tags/zz{i:06}\n", "0".repeat(40)))
        .collect();
    body.push_str(&filler);
    std::fs::write(&packed, body).unwrap();
    assert!(std::fs::metadata(&packed).unwrap().len() > 1024 * 1024);
    assert!(!d.join(".git/refs/heads/main").exists());
    let (path, log) = no_git(&t);
    let render = || {
        run(&t)
            .conf("set left \"[#{sha[link=no]}]\"")
            .env("PATH", &path)
            .cwd(&d)
            .unset("GIT_CONFIG_NOSYSTEM")
            .go()
    };
    // cold: nothing known, git only in the background refresh
    assert_eq!(one(&render().plain), "");
    Cache::new(&t.path().join("state"))
        .store(
            &cache_key(&d),
            &format!("# branch.oid {id}\n# branch.head main\n"),
        )
        .unwrap();
    assert_eq!(one(&render().plain), format!("[{}]", &id[..7]));
    std::thread::sleep(std::time::Duration::from_millis(200));
    let spawned = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!spawned.contains("rev-parse"), "{spawned:?}");
}

fn one(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

#[test]
fn stash_and_tag_of_reftable_repo_from_cached_git() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    std::fs::create_dir(d.join(".git/reftable")).unwrap();
    let id = head(&d);
    let (path, log) = no_git(&t);
    let conf = "set left \"[#{stash}|#{tag}]\"";
    let render = || {
        run(&t)
            .conf(conf)
            .env("PATH", &path)
            .cwd(&d)
            .unset("GIT_CONFIG_NOSYSTEM")
            .go()
    };
    // cold: the files cannot tell, so nothing (and never `$0`)
    let cold = render().plain;
    assert!(!cold.contains('$'), "{cold:?}");
    let cache = Cache::new(&t.path().join("state"));
    cache
        .store(
            &cache_key(&d),
            &format!("# branch.oid {id}\n# branch.head main\n# stash 2\n"),
        )
        .unwrap();
    cache
        .store(&tags_key(&d), &format!("{id}\ncommit v9\n"))
        .unwrap();
    assert_eq!(one(&render().plain), "[$2|v9]");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let spawned = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned
            .lines()
            .all(|l| l.contains("status") || l.starts_with("for-each-ref")),
        "{spawned:?}"
    );
}

#[test]
fn conflicts_reuse_the_count_while_the_index_is_unchanged() {
    let t = Tmp::new();
    let d = merging(&t);
    let conf = "set left \"[#{conflicts}]\"";
    let render = || {
        run(&t)
            .conf(conf)
            .cwd(&d)
            .unset("GIT_CONFIG_NOSYSTEM")
            .unset("GIT_INDEX_FILE")
            .go()
    };
    assert_eq!(one(&render().plain), "[✗2]");
    let cache = Cache::new(&t.path().join("state"));
    let key = hop_bar::placeholders::gitop::conflicts_key(&d);
    let stored = cache.read_fresh(&key, u64::MAX).expect("count stored");
    let (stamp, n) = stored.rsplit_once(' ').unwrap();
    assert_eq!(n, "2");
    // a count stored for this very index is used as is
    cache.store(&key, &format!("{stamp} 7")).unwrap();
    assert_eq!(one(&render().plain), "[✗7]");
    // a rewritten index is parsed again
    let idx = d.join(".git/index");
    let body = std::fs::read(&idx).unwrap();
    std::fs::remove_file(&idx).unwrap();
    std::fs::write(&idx, body).unwrap();
    assert_eq!(one(&render().plain), "[✗2]");
}
