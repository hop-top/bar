#![allow(dead_code, unused_imports)]
use crate::common;
use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as Std;
use std::sync::atomic::{AtomicUsize, Ordering};

pub static N: AtomicUsize = AtomicUsize::new(0);

pub struct Tmp(PathBuf);

impl Tmp {
    pub fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-render-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn write(&self, rel: &str, body: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

pub fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub struct Out {
    /// PATH the spawned `bar` received.
    pub path: std::ffi::OsString,
    pub raw: String,
    pub plain: String,
}

pub struct Run<'a> {
    pub home: &'a Tmp,
    pub conf: Option<&'a str>,
    pub env: Vec<(&'a str, String)>,
    pub unset: Vec<&'a str>,
    pub stdin: String,
}

pub fn run(home: &Tmp) -> Run<'_> {
    Run {
        home,
        conf: None,
        env: vec![],
        unset: vec![],
        stdin: "{}".into(),
    }
}

impl<'a> Run<'a> {
    pub fn conf(mut self, c: &'a str) -> Self {
        self.conf = Some(c);
        self
    }
    pub fn env(mut self, k: &'a str, v: &str) -> Self {
        self.env.push((k, v.into()));
        self
    }
    pub fn unset(mut self, k: &'a str) -> Self {
        self.unset.push(k);
        self
    }
    pub fn stdin(mut self, s: &str) -> Self {
        self.stdin = s.into();
        self
    }
    pub fn cwd(self, d: &Path) -> Self {
        self.stdin(&common::workspace_json(d))
    }
    pub fn go(self) -> Out {
        let h = self.home.path();
        if let Some(c) = self.conf {
            self.home.write("xdg/bar/bar.conf", c);
        }
        let (iso, cwd) = common::isolation(h);
        let path = iso.iter().find(|(k, _)| *k == "PATH").unwrap().1.clone();
        let mut cmd = Command::cargo_bin("bar").unwrap();
        cmd.envs(iso);
        // These tests seed `$HOME/.claude.json` and `$HOME/.claude/`.
        cmd.arg("render")
            .env("COLUMNS", "200")
            .env_remove("CLAUDE_CONFIG_DIR")
            .current_dir(&cwd);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        for k in &self.unset {
            cmd.env_remove(k);
        }
        let o = cmd.write_stdin(self.stdin).assert().success();
        let raw = String::from_utf8(o.get_output().stdout.clone()).unwrap();
        Out {
            plain: strip(&raw),
            path,
            raw,
        }
    }
}

pub fn git(dir: &Path, args: &[&str]) {
    let st = Std::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

pub fn repo(t: &Tmp, name: &str) -> PathBuf {
    let d = t.path().join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "x"]);
    d
}

pub fn account_json(email: &str) -> String {
    format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#)
}
