mod common;

use assert_cmd::Command;
use hop_bar::gitfs::{discover, Found, Repo, Unsupported};
use hop_bar::placeholders::repo::remote_url;
use std::path::{Path, PathBuf};
use std::process::Command as Std;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static ISOLATE: Once = Once::new();

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        // the reader consults global git config; keep the machine's out
        ISOLATE.call_once(|| {
            std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
            std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
        });
        let p = std::env::temp_dir().join(format!(
            "bar-gitfs-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
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

fn git_cmd(dir: &Path, args: &[&str]) -> Std {
    let mut c = Std::new("git");
    c.args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(dir)
        .args(args)
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    c
}

fn git(dir: &Path, args: &[&str]) {
    let st = git_cmd(dir, args).status().unwrap();
    assert!(st.success(), "git {args:?}");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd(dir, args).output().unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn repo(t: &Tmp, name: &str) -> PathBuf {
    let d = t.path().join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "x"]);
    d
}

fn open(d: &Path) -> Repo {
    match discover(d) {
        Found::Repo(r) => r,
        other => panic!("expected repo at {d:?}, got {other:?}"),
    }
}

#[test]
fn branch_from_head() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["checkout", "-q", "-b", "feat/x"]);
    let sub = d.join("a/b");
    std::fs::create_dir_all(&sub).unwrap();
    let r = open(&sub);
    assert_eq!(r.top, d);
    assert_eq!(r.branch(), Ok(Some("feat/x".into())));
    assert_eq!(git_out(&d, &["branch", "--show-current"]), "feat/x");
}

#[test]
fn detached_head_no_branch() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["checkout", "-q", "--detach"]);
    assert_eq!(open(&d).branch(), Ok(None));
}

#[test]
fn remote_from_config() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    assert_eq!(open(&d).origin_url(), Ok(None));
    git(&d, &["remote", "add", "origin", "git@github.com:o/r.git"]);
    assert_eq!(
        open(&d).origin_url(),
        Ok(Some(git_out(&d, &["remote", "get-url", "origin"])))
    );
    // other remotes are not origin
    git(&d, &["remote", "remove", "origin"]);
    git(&d, &["remote", "add", "up", "https://h/o/r"]);
    assert_eq!(open(&d).origin_url(), Ok(None));
}

#[test]
fn quoted_and_commented_config() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let cfg = d.join(".git/config");
    let mut body = std::fs::read_to_string(&cfg).unwrap();
    body.push_str(
        "[branch \"main\"]\n\turl = nope\n\
         [remote \"up\"]\n\turl = https://wrong/x\n\
         # a comment\n[Remote \"origin\"] ; trailing\n\
         \t; another\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
         \tURL = \"https://h.io/o/r.git\" # why\n\turl = https://second/x\n",
    );
    std::fs::write(&cfg, body).unwrap();
    let want = git_out(&d, &["remote", "get-url", "origin"]);
    assert_eq!(want, "https://h.io/o/r.git");
    assert_eq!(open(&d).origin_url(), Ok(Some(want)));
}

#[test]
fn linked_worktree_uses_commondir() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["remote", "add", "origin", "https://h.io/o/r.git"]);
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "feat"]);
    let wt = d.join(".wt/x");
    let r = open(&wt);
    assert_eq!(r.top, wt);
    assert_eq!(r.branch(), Ok(Some("feat".into())));
    assert_eq!(r.origin_url(), Ok(Some("https://h.io/o/r.git".into())));
    // the main checkout keeps its own HEAD
    assert_eq!(open(&d).branch(), Ok(Some("main".into())));
}

#[test]
fn gitdir_file_relative_path() {
    let t = Tmp::new();
    let d = t.path().join("wt");
    std::fs::create_dir_all(&d).unwrap();
    git(
        t.path(),
        &[
            "init",
            "-q",
            "-b",
            "dev",
            "--separate-git-dir",
            "sep.git",
            "wt",
        ],
    );
    git(&d, &["remote", "add", "origin", "https://h.io/o/r"]);
    std::fs::write(d.join(".git"), "gitdir: ../sep.git\n").unwrap();
    let r = open(&d);
    assert_eq!(r.branch(), Ok(Some("dev".into())));
    assert_eq!(r.origin_url(), Ok(Some("https://h.io/o/r".into())));
}

#[test]
fn hop_layout_hub_name_unchanged() {
    let t = Tmp::new();
    let seed = repo(&t, "seed");
    // bare hub as hop lays it out: hub/.bare plus worktrees under hub/hops
    let hub = t.path().join("hub");
    std::fs::create_dir_all(&hub).unwrap();
    git(t.path(), &["clone", "-q", "--bare", "seed", "hub/.bare"]);
    std::fs::write(hub.join("hop.json"), "{}").unwrap();
    let wt = hub.join("hops/feat/x");
    git(
        &hub.join(".bare"),
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
    );
    drop(seed);

    let home = t.path().join("home");
    let (iso, cwd) = common::isolation(&home);
    let out = Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .arg("render")
        .env("COLUMNS", "200")
        .current_dir(cwd)
        .write_stdin(format!(
            r#"{{"workspace":{{"current_dir":"{}"}}}}"#,
            wt.display()
        ))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("hub"), "{text:?}");
    assert!(!text.contains("x:"), "{text:?}");
}

#[test]
fn insteadof_falls_back_to_git() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["config", "url.https://mirror.io/.insteadOf", "gh:"]);
    git(&d, &["remote", "add", "origin", "gh:o/r.git"]);
    assert_eq!(open(&d).origin_url(), Err(Unsupported));
    assert_eq!(
        git_out(&d, &["remote", "get-url", "origin"]),
        "https://mirror.io/o/r.git"
    );
    assert_eq!(remote_url(&d).as_deref(), Some("https://mirror.io/o/r"));
}

#[test]
fn no_repo_none() {
    let t = Tmp::new();
    let d = t.path().join("plain");
    std::fs::create_dir_all(&d).unwrap();
    assert!(matches!(discover(&d), Found::NotRepo));
    assert_eq!(remote_url(&d), None);
}

#[test]
fn render_spawns_no_git_on_plain_repo() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(
        &d,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
    let home = t.path().join("home");
    let (mut iso, cwd) = common::isolation(&home);
    let log = t.path().join("git-calls.log");
    let fake = t.path().join("fake-git");
    std::fs::create_dir_all(&fake).unwrap();
    let g = fake.join("git");
    std::fs::write(
        &g,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&g, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    for (k, v) in iso.iter_mut() {
        if *k == "PATH" {
            let mut p = fake.clone().into_os_string();
            p.push(":");
            p.push(&*v);
            *v = p;
        }
    }
    let out = Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .arg("render")
        .env("COLUMNS", "200")
        .current_dir(cwd)
        .write_stdin(format!(
            r#"{{"workspace":{{"current_dir":"{}"}}}}"#,
            d.display()
        ))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("main") && text.contains("github.com/o/r"),
        "{text:?}"
    );
    // background refreshes of prs/issues run detached; give them time
    std::thread::sleep(std::time::Duration::from_millis(800));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(calls, "", "git was spawned");
}
