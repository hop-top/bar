//! Temp repositories built with the `git` CLI, for the repository-file
//! reader tests.
#![allow(dead_code)]

use hop_bar::gitfs::{discover, Found, Repo};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

static N: AtomicUsize = AtomicUsize::new(0);
static ISOLATE: Once = Once::new();

pub struct Tmp(PathBuf);

impl Tmp {
    pub fn new() -> Tmp {
        // the reader consults global git config; keep the machine's out
        ISOLATE.call_once(|| std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null"));
        let p = crate::common::tmp::root().join(format!(
            "bar-gitstate-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

pub fn cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "core.editor=true", "-c", "commit.gpgsign=false"])
        .args(["-c", "tag.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_EDITOR", "true")
        .env_remove("GIT_INDEX_FILE");
    c
}

pub fn git(dir: &Path, args: &[&str]) {
    let st = cmd(dir, args).output().unwrap();
    assert!(st.status.success(), "git {args:?}: {st:?}");
}

/// Runs git expecting failure (a conflict).
pub fn git_fails(dir: &Path, args: &[&str]) {
    let st = cmd(dir, args).output().unwrap();
    assert!(!st.status.success(), "git {args:?} succeeded");
}

pub fn out(dir: &Path, args: &[&str]) -> String {
    let o = cmd(dir, args).output().unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

pub fn repo(t: &Tmp, name: &str) -> PathBuf {
    let d = t.path().join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "x"]);
    d
}

pub fn commit_file(d: &Path, file: &str, body: &str, msg: &str) {
    let p = d.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    git(d, &["add", "--", file]);
    git(d, &["commit", "-q", "-m", msg]);
}

pub fn open(d: &Path) -> Repo {
    match discover(d) {
        Found::Repo(r) => r,
        other => panic!("expected repo at {d:?}, got {other:?}"),
    }
}

/// Unmerged paths as `git status` reports them.
pub fn git_unmerged(d: &Path) -> usize {
    out(d, &["status", "--porcelain=v2"])
        .lines()
        .filter(|l| l.starts_with("u "))
        .count()
}

/// Repo whose branches `main` and `feat` both change `files`, so merging
/// or rebasing one onto the other conflicts on each. `feat` gets `extra`
/// more commits on top that touch other files.
pub fn diverged(t: &Tmp, files: &[&str], extra: usize) -> PathBuf {
    let d = repo(t, "p");
    for f in files {
        commit_file(&d, f, "base\n", "base");
    }
    git(&d, &["checkout", "-q", "-b", "feat"]);
    for f in files {
        commit_file(&d, f, "feat\n", "feat");
    }
    for i in 0..extra {
        commit_file(&d, &format!("extra{i}"), "x\n", "extra");
    }
    git(&d, &["checkout", "-q", "main"]);
    for f in files {
        commit_file(&d, f, "main\n", "main");
    }
    d
}
