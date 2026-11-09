// Unix fixtures (shell scripts, modes, POSIX paths)
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::Cache;
use hop_bar::placeholders::jj::cache_key;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SAMPLE: &str =
    "kp\0qxywon\x003f2a\x001b9c\0feat\0main\x1ffeat\0other\0empty conflict\0fix: the thing\0";

/// Non-colocated workspace `name`: a `.jj` dir and nothing else.
fn workspace(t: &Tmp, name: &str) -> PathBuf {
    let d = t.path().join(name);
    std::fs::create_dir_all(d.join(".jj")).unwrap();
    d
}

/// Fake `jj` in its own dir: logs its arguments, prints `out`, exits 0.
/// Returns the PATH holding only it, and the log.
fn fake_jj(t: &Tmp, out: &str) -> (String, PathBuf) {
    let bin = t.path().join("jj-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = t.path().join("jj.log");
    let body = t.path().join("jj.out");
    std::fs::write(&body, out).unwrap();
    let jj = bin.join("jj");
    std::fs::write(
        &jj,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\n/bin/cat '{}'\n",
            log.display(),
            body.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&jj, PermissionsExt::from_mode(0o755)).unwrap();
    (bin.display().to_string(), log)
}

fn seed(t: &Tmp, d: &Path, body: &str) {
    Cache::new(&t.path().join("state"))
        .store(&cache_key(d), body)
        .unwrap();
}

fn one(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wait_for(p: &Path) -> String {
    for _ in 0..200 {
        if let Ok(s) = std::fs::read_to_string(p) {
            if !s.is_empty() {
                return s;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    String::new()
}

#[test]
fn default_parts_from_cache_without_spawning() {
    let t = Tmp::new();
    let d = workspace(&t, "proj");
    seed(&t, &d, SAMPLE);
    let (path, log) = fake_jj(&t, SAMPLE);
    let o = run(&t)
        .conf("set left \"#{repo} #{jj}\"")
        .env("PATH", &path)
        .cwd(&d)
        .go();
    assert_eq!(one(&o.plain), "proj kpqxywon feat main conflict empty");
    assert!(!log.exists(), "render spawned jj");
}

#[test]
fn every_part_in_display_order() {
    let t = Tmp::new();
    let d = workspace(&t, "proj");
    seed(&t, &d, SAMPLE);
    let o = run(&t)
        .conf("set left \"#{jj[show=desc,workspace,commit,change,state,bookmarks]}\"")
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .go();
    assert_eq!(
        one(&o.plain),
        "kpqxywon 3f2a1b9c feat main other conflict empty fix: the thing"
    );
}

#[test]
fn max_truncates_the_description() {
    let t = Tmp::new();
    let d = workspace(&t, "proj");
    seed(&t, &d, SAMPLE);
    let o = run(&t)
        .conf("set left \"#{jj[show=desc,max=5]}\"")
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .go();
    assert_eq!(one(&o.plain), "fix:…");
}

#[test]
fn subdirectory_uses_workspace_root() {
    let t = Tmp::new();
    let d = workspace(&t, "proj");
    let sub = d.join("src/deep");
    std::fs::create_dir_all(&sub).unwrap();
    seed(&t, &d, SAMPLE);
    let o = run(&t)
        .conf("set left \"#{root[base=1]} #{jj[show=change]}\"")
        .env("PATH", "/nonexistent")
        .cwd(&sub)
        .go();
    assert_eq!(one(&o.plain), "proj kpqxywon");
}

#[test]
fn hidden_outside_jj_and_on_bad_output() {
    let t = Tmp::new();
    let plain = t.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let (path, log) = fake_jj(&t, SAMPLE);
    let o = run(&t)
        .conf("set left \"#{cwd[base=1]} #{jj}\"")
        .env("PATH", &path)
        .cwd(&plain)
        .go();
    assert_eq!(one(&o.plain), "plain");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!log.exists(), "jj ran outside a workspace");

    let d = workspace(&t, "proj");
    seed(&t, &d, "garbage\n");
    let o = run(&t)
        .conf("set left \"#{cwd[base=1]} #{jj}\"")
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .go();
    assert_eq!(one(&o.plain), "proj");
}

#[test]
fn cold_cache_refresh_runs_jj_without_snapshot() {
    let t = Tmp::new();
    let d = workspace(&t, "proj");
    let (path, log) = fake_jj(&t, SAMPLE);
    let conf = "set left \"#{jj[show=change,bookmarks]}\"";
    let o = run(&t).conf(conf).env("PATH", &path).cwd(&d).go();
    assert_eq!(one(&o.plain), "", "cold cache renders empty");
    let mut calls = wait_for(&log);
    // the fake writes one argument per line: wait for the last one
    for _ in 0..200 {
        if calls.lines().any(|l| l.starts_with("self.")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        calls = std::fs::read_to_string(&log).unwrap_or_default();
    }
    let argv: Vec<&str> = calls.lines().collect();
    assert_eq!(argv.first(), Some(&"--ignore-working-copy"), "{calls}");
    let root = d.display().to_string();
    for w in ["-R", root.as_str(), "log", "-r", "@", "--no-graph", "-T"] {
        assert!(argv.contains(&w), "{w}: {calls}");
    }
    // the stored output renders on the next call
    let file = Cache::new(&t.path().join("state")).path_for(&cache_key(&d));
    wait_for(&file);
    let o = run(&t).conf(conf).env("PATH", &path).cwd(&d).go();
    assert_eq!(one(&o.plain), "kpqxywon feat main");
}

#[test]
fn repo_and_root_name_a_jj_only_workspace_inside_a_git_tree() {
    let t = Tmp::new();
    let outer = repo(&t, "outer");
    let ws = outer.join("inner");
    std::fs::create_dir_all(ws.join(".jj")).unwrap();
    let o = run(&t)
        .conf("set left \"#{repo} #{root[base=1]} #{branch}\"")
        .env("PATH", "/nonexistent")
        .cwd(&ws)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    // branch stays git's: the outer tree's
    assert_eq!(one(&o.plain), "inner inner main");
}

#[test]
fn colocated_repo_keeps_git_output() {
    let t = Tmp::new();
    let d = repo(&t, "colo");
    git(
        &d,
        &["remote", "add", "origin", "https://github.com/o/colo.git"],
    );
    let conf = "set left \"#{repo} #{root[base=1]} #{branch}\"";
    let before = run(&t)
        .conf(conf)
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    std::fs::create_dir_all(d.join(".jj")).unwrap();
    let after = run(&t)
        .conf(conf)
        .env("PATH", "/nonexistent")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&before.plain), "colo colo main");
    assert_eq!(before.raw, after.raw);
}

#[test]
fn colocated_repo_through_a_symlink_keeps_git_output() {
    let t = Tmp::new();
    let d = repo(&t, "colo");
    git(
        &d,
        &["remote", "add", "origin", "https://github.com/o/colo.git"],
    );
    std::fs::create_dir_all(d.join(".jj")).unwrap();
    let link = t.path().join("via");
    std::os::unix::fs::symlink(&d, &link).unwrap();
    let o = run(&t)
        .conf("set left \"#{repo}\"")
        .env("PATH", "/nonexistent")
        .cwd(&link)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(one(&o.plain), "colo");
    assert!(o.raw.contains("https://github.com/o/colo"), "{:?}", o.raw);
}

#[test]
fn check_accepts_options_and_warns_without_jj() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{jj[show=change,desc,ttl=30,max=20]}\"\n",
    );
    let (iso, cwd) = common::isolation(t.path());
    let (path, _) = fake_jj(&t, SAMPLE);
    let check = |path: &str| {
        let o = assert_cmd::Command::cargo_bin("bar")
            .unwrap()
            .envs(iso.clone())
            .env("PATH", path)
            .current_dir(&cwd)
            .arg("check")
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
    };
    let with = check(&path);
    assert!(!with.contains("jj"), "{with}");
    let without = check("/nonexistent");
    assert!(without.contains("`jj` not on PATH"), "{without}");
}

#[test]
fn check_rejects_bad_show() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set left \"#{jj[show=branch]}\"\n");
    let (iso, cwd) = common::isolation(t.path());
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .arg("check")
        .assert()
        .failure();
}

/// `jj` from the inherited PATH; tests that need the real CLI skip
/// without it.
fn real_jj() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("jj"))
        .find(|p| p.is_file())
}

#[test]
fn real_jj_workspace_end_to_end() {
    let Some(jj) = real_jj() else {
        eprintln!("jj not installed: skipped");
        return;
    };
    let t = Tmp::new();
    let d = t.path().join("real");
    std::fs::create_dir_all(&d).unwrap();
    let cfg = t.path().join("jj-config.toml");
    std::fs::write(&cfg, "").unwrap();
    let jjc = |args: &[&str]| {
        let st = std::process::Command::new(&jj)
            .args(["--config", "git.colocate=false"])
            .args(args)
            .current_dir(&d)
            .env("HOME", t.path())
            .env("JJ_CONFIG", &cfg)
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(st.status.success(), "jj {args:?}: {st:?}");
    };
    jjc(&["git", "init"]);
    jjc(&["describe", "-m", "hello world\n\nbody"]);
    jjc(&["bookmark", "create", "feat", "-r", "@"]);
    let mut path = std::ffi::OsString::from(jj.parent().unwrap());
    path.push(":/usr/bin:/bin");
    let path = path.to_string_lossy().to_string();
    let conf = "set left \"#{jj[show=bookmarks,state,desc,workspace]} #{repo}\"";
    let render = || {
        run(&t)
            .conf(conf)
            .env("PATH", &path)
            .env("JJ_CONFIG", cfg.to_str().unwrap())
            .cwd(&d)
            .go()
    };
    render();
    let file = Cache::new(&t.path().join("state")).path_for(&cache_key(&d));
    let raw = wait_for(&file);
    assert!(!raw.is_empty(), "jj refresh stored nothing");
    let o = render();
    // a described change with no files: empty, default workspace hidden
    assert_eq!(one(&o.plain), "feat empty hello world real");
}

#[test]
fn real_jj_output_unaffected_by_log_word_wrap() {
    let Some(jj) = real_jj() else {
        eprintln!("jj not installed: skipped");
        return;
    };
    let t = Tmp::new();
    let d = t.path().join("real");
    std::fs::create_dir_all(&d).unwrap();
    let cfg = t.path().join("jj-config.toml");
    // wraps template output at the terminal width (`COLUMNS`, else 80)
    std::fs::write(&cfg, "[ui]\nlog-word-wrap = true\n").unwrap();
    let jjc = |args: &[&str]| {
        let st = std::process::Command::new(&jj)
            .args(["--config", "git.colocate=false"])
            .args(args)
            .current_dir(&d)
            .env("HOME", t.path())
            .env("JJ_CONFIG", &cfg)
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(st.status.success(), "jj {args:?}: {st:?}");
    };
    jjc(&["git", "init"]);
    jjc(&["describe", "-m", "short"]);
    let names: Vec<String> = (0..12)
        .map(|i| format!("long-bookmark-name-number-{i}"))
        .collect();
    for n in &names {
        jjc(&["bookmark", "create", n, "-r", "@"]);
    }
    let mut path = std::ffi::OsString::from(jj.parent().unwrap());
    path.push(":/usr/bin:/bin");
    let path = path.to_string_lossy().to_string();
    let render = || {
        run(&t)
            .conf("set left \"#{jj[show=bookmarks,state,desc]}\"")
            .env("PATH", &path)
            .env("JJ_CONFIG", cfg.to_str().unwrap())
            .cwd(&d)
            .go()
    };
    render();
    let file = Cache::new(&t.path().join("state")).path_for(&cache_key(&d));
    assert!(!wait_for(&file).is_empty(), "jj refresh stored nothing");
    // jj lists bookmarks by name
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        one(&render().plain),
        format!("{} empty short", sorted.join(" "))
    );
}

#[test]
fn real_jj_template_immune_to_aliases_and_spaced_names() {
    let Some(jj) = real_jj() else {
        eprintln!("jj not installed: skipped");
        return;
    };
    let t = Tmp::new();
    let d = t.path().join("real");
    std::fs::create_dir_all(&d).unwrap();
    let cfg = t.path().join("jj-config.toml");
    std::fs::write(&cfg, "").unwrap();
    let jjc = |args: &[&str]| {
        let o = std::process::Command::new(&jj)
            .args(["--config", "git.colocate=false"])
            .args(args)
            .current_dir(&d)
            .env("HOME", t.path())
            .env("JJ_CONFIG", &cfg)
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(o.status.success(), "jj {args:?}: {o:?}");
        String::from_utf8(o.stdout).unwrap()
    };
    jjc(&["git", "init"]);
    jjc(&["describe", "-m", "hello  world"]);
    jjc(&["bookmark", "create", "\"a b\"", "-r", "@"]);
    jjc(&["bookmark", "create", "feat", "-r", "@"]);
    // aliases named after the keywords the template reads
    std::fs::write(
        &cfg,
        concat!(
            "[template-aliases]\n",
            "change_id = '\"SHADOW\"'\n",
            "description = '\"SHADOW\"'\n",
            "empty = 'false'\n",
            "local_bookmarks = '\"\"'\n",
        ),
    )
    .unwrap();
    let dir = d.to_str().unwrap();
    let raw = jjc(&hop_bar::placeholders::jj::args(dir)
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>());
    let info = hop_bar::placeholders::jj::parse(&raw).expect("template output parses");
    assert!(
        info.change.0.chars().all(|c| c.is_ascii_lowercase()),
        "{info:?}"
    );
    assert_eq!(info.desc, "hello  world");
    assert!(info.empty);
    assert_eq!(info.bookmarks.len(), 2, "{info:?}");
    assert!(info.bookmarks.iter().any(|b| b.contains("a b")), "{info:?}");
}
