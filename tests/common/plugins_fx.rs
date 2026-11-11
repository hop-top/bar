#![allow(dead_code, unused_imports)]
use crate::common;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

pub static N: AtomicUsize = AtomicUsize::new(0);
pub const DAY: i64 = 86_400;
pub const SENTINEL: &str = "sk-sentinel-cookie-7f3a9c";

pub struct Fx {
    pub root: PathBuf,
    pub fake: PathBuf,
    pub env: Vec<(&'static str, std::ffi::OsString)>,
    pub cwd: PathBuf,
    pub extra: Vec<(&'static str, String)>,
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.root);
    }
}

pub fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plugins")
        .join(name)
}

pub fn exe(path: &Path, body: &str) {
    crate::common::exe::write(path, body);
}

impl Fx {
    pub fn new() -> Fx {
        let root = crate::common::tmp::root().join(format!(
            "bar-plugins-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let (env, cwd) = common::isolation(&root);
        let fake = root.join("fake-data");
        std::fs::create_dir_all(&fake).unwrap();
        Fx {
            root,
            fake,
            env,
            cwd,
            extra: vec![],
        }
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("fake-bin")
    }

    pub fn put(&self, name: &str, text: &str) {
        std::fs::write(self.fake.join(name), text).unwrap();
    }

    pub fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.fake.join(name)).unwrap_or_default()
    }

    pub fn run(&self, plugin: &str, stdin: &Value) -> Output {
        let mut cmd = Command::new(script(plugin));
        cmd.env_clear()
            .envs(self.env.iter().map(|(k, v)| (*k, v.clone())))
            .env("FAKE_DIR", &self.fake)
            .envs(self.extra.iter().map(|(k, v)| (*k, v.as_str())))
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), stdin.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    // ---- tlc ----

    pub fn tlc_fake(&self) {
        exe(
            &self.bin().join("tlc"),
            r#"#!/bin/sh
echo "$PWD|$*" >> "$FAKE_DIR/tlc.log"
[ -d .tlc ] || mkdir -p .tlc
if [ -f "$FAKE_DIR/fail" ] && printf '%s' "$*" | grep -q -- "$(cat "$FAKE_DIR/fail")"; then exit 1; fi
case "$*" in
  "task show "*) [ -f "$FAKE_DIR/show_$3.json" ] || exit 3; cat "$FAKE_DIR/show_$3.json" ;;
  "track show "*) cat "$FAKE_DIR/track_$3.json" ;;
  "task list"*)
    track=""; prev=""
    for a in "$@"; do [ "$prev" = --track ] && track=$a; prev=$a; done
    cat "$FAKE_DIR/tasks${track:+_$track}.json" ;;
esac
"#,
        );
    }

    /// A task `id` on track `track` (`None`: no track) with that track's
    /// slug, as `tlc task show` / `tlc track show` print them.
    pub fn tlc_task(&self, id: &str, track: Option<&str>, status: &str) {
        let t = json!({"task": {"id": id, "track_id": track, "status": status}});
        self.put(&format!("show_{id}.json"), &t.to_string());
        if let Some(tr) = track {
            let slug = format!("{tr}-slug");
            self.put(
                &format!("track_{tr}.json"),
                &json!({"id": tr, "slug": slug}).to_string(),
            );
        }
    }

    /// Git work tree whose root holds `.tlc/`, plus a nested working dir;
    /// returns (root, nested).
    pub fn project(&self) -> (PathBuf, PathBuf) {
        let proj = self.root.join("proj");
        let deep = proj.join("sub/deep");
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        std::fs::create_dir_all(proj.join(".tlc")).unwrap();
        std::fs::write(proj.join(".tlc/config.yaml"), "").unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        (proj, deep)
    }

    pub fn tlc(&self, cwd: &Path) -> Output {
        self.tlc_with(cwd, json!({}))
    }

    pub fn tlc_with(&self, cwd: &Path, options: Value) -> Output {
        self.tlc_rec(cwd, options, Value::Null)
    }

    /// With `session.record` as the hook keeps it (`null`: none).
    pub fn tlc_rec(&self, cwd: &Path, options: Value, record: Value) -> Output {
        self.tlc_tog(cwd, options, record, json!({}))
    }

    /// With the plugin's on-toggles as the core sends them in `toggles`.
    pub fn tlc_tog(&self, cwd: &Path, options: Value, record: Value, toggles: Value) -> Output {
        let mut session = json!({"cwd": cwd, "session_id": "0a1b2c3d-0000-4000-8000-000000000001"});
        if !record.is_null() {
            session["record"] = record;
        }
        self.run(
            "bar-tlc",
            &json!({"options":options,"session":session,"columns":null,"config":{},"toggles":toggles}),
        )
    }

    // ---- credits ----

    pub fn credits_fakes(&self, body: &str, code: &str) {
        exe(
            &self.bin().join("security"),
            r#"#!/bin/sh
echo "$*" >> "$FAKE_DIR/security.log"
[ -f "$FAKE_DIR/cookie" ] || exit 44
cat "$FAKE_DIR/cookie"
"#,
        );
        exe(
            &self.bin().join("curl"),
            r#"#!/bin/sh
echo "$*" >> "$FAKE_DIR/curl.log"
cat >> "$FAKE_DIR/curl.stdin"
env >> "$FAKE_DIR/curl.env"
code=$(cat "$FAKE_DIR/code")
[ "$code" = fail ] && exit 28
cat "$FAKE_DIR/body.json"
printf '\n%s' "$code"
"#,
        );
        self.put("cookie", &format!("{SENTINEL}\n"));
        self.put("body.json", body);
        self.put("code", code);
    }

    pub fn credits(&self) -> Output {
        self.credits_with("org-1", "claude-console-session-x")
    }

    pub fn credits_with(&self, org: &str, keychain: &str) -> Output {
        self.credits_opts(json!({}), json!({"org":org,"keychain":keychain}))
    }

    pub fn credits_opts(&self, options: Value, config: Value) -> Output {
        self.run(
            "bar-claude-credits",
            &json!({"options":options,"session":{},"columns":null,"config":config}),
        )
    }
}

pub fn out_json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "bad json {e}: {:?} / stderr {:?}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

pub fn tasks(done: usize, skipped: usize, todo: usize, wip: usize) -> String {
    let mut v = vec![];
    for (n, s) in [
        (done, "DONE"),
        (skipped, "SKIPPED"),
        (todo, "TODO"),
        (wip, "IN_PROGRESS"),
    ] {
        for _ in 0..n {
            v.push(json!({"status": s}));
        }
    }
    Value::Array(v).to_string()
}
