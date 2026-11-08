//! Fake `gh` on PATH, a git repo and a cache for `prs` / `issues` tests.
#![allow(dead_code)]

use hop_bar::cache::{Cache, Request};
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::gh::{plan, Issues, Kind, Prs};
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static N: AtomicUsize = AtomicUsize::new(0);
static ENV: Mutex<()> = Mutex::new(());

const FAKE_GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_GH_LOG"
if [ -n "$FAKE_GH_FAIL" ]; then echo "API rate limit exceeded" >&2; exit 1; fi
q=""; prev=""
for a in "$@"; do [ "$prev" = "--search" ] && q=$a; prev=$a; done
k=$(printf %s "$q" | tr -c 'A-Za-z0-9' _)
f="$FAKE_GH_DIR/out-$k.json"
[ -f "$f" ] || f="$FAKE_GH_DIR/out.json"
cat "$f"
"#;

/// Temp dir, fake `gh` on PATH and a git repo; holds the env lock.
pub struct Fx {
    pub root: PathBuf,
    pub state: PathBuf,
    pub repo: PathBuf,
    old_path: Option<std::ffi::OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Fx {
    pub fn new(remote: Option<&str>) -> Fx {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "bar-gh-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let (bin, data, state, repo) = (
            root.join("bin"),
            root.join("data"),
            root.join("state"),
            root.join("repo"),
        );
        for d in [&bin, &data, &state, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        let root = root.canonicalize().unwrap();
        let (bin, data, state, repo) = (
            root.join("bin"),
            root.join("data"),
            root.join("state"),
            root.join("repo"),
        );
        let gh = bin.join("gh");
        std::fs::write(&gh, FAKE_GH).unwrap();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut std::fs::metadata(&gh).unwrap().permissions(),
            0o755,
        );
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        git(&repo, &["init", "-q"]);
        if let Some(r) = remote {
            git(&repo, &["remote", "add", "origin", r]);
        }
        let old_path = std::env::var_os("PATH");
        let mut path = bin.into_os_string();
        path.push(":");
        path.push(old_path.clone().unwrap_or_default());
        std::env::set_var("PATH", path);
        std::env::set_var("FAKE_GH_DIR", &data);
        std::env::set_var("FAKE_GH_LOG", root.join("gh.log"));
        std::env::remove_var("FAKE_GH_FAIL");
        Fx {
            root,
            state,
            repo,
            old_path,
            _lock: lock,
        }
    }

    pub fn github() -> Fx {
        Fx::new(Some("https://github.com/o/r.git"))
    }

    pub fn out(&self, rel: &str, body: &str) {
        std::fs::write(self.root.join("data").join(rel), body).unwrap();
    }

    pub fn fixture(&self, name: &str) {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        self.out("out.json", &std::fs::read_to_string(p.join(name)).unwrap());
    }

    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("gh.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    pub fn cache(&self) -> Cache {
        Cache::with_exe(&self.state, PathBuf::from("/nonexistent/bar"))
    }

    /// Runs every query's job now, as a finished background refresh would.
    pub fn prime(&self, kind: Kind, o: &[(&str, &str)]) {
        let cache = self.cache();
        for (key, job) in plan(kind, "https://github.com/o/r", &opts(o)).unwrap().jobs {
            cache.refresh(
                &Request {
                    key,
                    job,
                    lock: String::new(),
                },
                Duration::from_secs(10),
            );
        }
    }

    pub fn show(&self, kind: Kind, o: &[(&str, &str)]) -> String {
        self.show_with(&Config::default(), kind, o)
    }

    pub fn show_with(&self, cfg: &Config, kind: Kind, o: &[(&str, &str)]) -> String {
        let session = Session {
            cwd: self.repo.display().to_string(),
            ..Session::default()
        };
        let cache = self.cache();
        let ctx = Ctx {
            session: &session,
            config: cfg,
            now: 0,
            runner: &ColdRunner,
            cache: Some(&cache),
            columns: None,
            plugin_dirs: &[],
        };
        let p: &dyn Placeholder = match kind {
            Kind::Pr => &Prs,
            Kind::Issue => &Issues,
        };
        p.render(&ctx, &opts(o)).unwrap_or_default()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        match &self.old_path {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

pub fn opts(o: &[(&str, &str)]) -> Options {
    let mut out = Options::default();
    for (k, v) in o {
        out.set(k, v);
    }
    out
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
