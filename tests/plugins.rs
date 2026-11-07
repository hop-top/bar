//! Reference plugins driven directly: JSON on stdin, JSON on stdout, fakes
//! for `tlc`, `security` and `curl` first on `PATH`.
mod common;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const DAY: i64 = 86_400;
const SENTINEL: &str = "sk-sentinel-cookie-7f3a9c";

struct Fx {
    root: PathBuf,
    fake: PathBuf,
    env: Vec<(&'static str, std::ffi::OsString)>,
    cwd: PathBuf,
}

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plugins")
        .join(name)
}

fn exe(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

impl Fx {
    fn new() -> Fx {
        let root = std::env::temp_dir().join(format!(
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
        }
    }

    fn bin(&self) -> PathBuf {
        self.root.join("fake-bin")
    }

    fn put(&self, name: &str, text: &str) {
        std::fs::write(self.fake.join(name), text).unwrap();
    }

    fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.fake.join(name)).unwrap_or_default()
    }

    fn run(&self, plugin: &str, stdin: &Value) -> Output {
        let mut cmd = Command::new(script(plugin));
        cmd.env_clear()
            .envs(self.env.iter().map(|(k, v)| (*k, v.clone())))
            .env("HOME", &self.root)
            .env("FAKE_DIR", &self.fake)
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

    fn tlc_fake(&self) {
        exe(
            &self.bin().join("tlc"),
            r#"#!/bin/sh
echo "$PWD|$*" >> "$FAKE_DIR/tlc.log"
[ -d .tlc ] || mkdir -p .tlc
if [ -f "$FAKE_DIR/fail" ] && printf '%s' "$*" | grep -q -- "$(cat "$FAKE_DIR/fail")"; then exit 1; fi
case "$*" in
  "track list"*) cat "$FAKE_DIR/tracks.json" ;;
  "task list --status IN_PROGRESS"*) cat "$FAKE_DIR/claimed.json" ;;
  "task list"*)
    track=""; prev=""
    for a in "$@"; do [ "$prev" = --track ] && track=$a; prev=$a; done
    cat "$FAKE_DIR/tasks${track:+_$track}.json" ;;
esac
"#,
        );
        self.put("tracks.json", "[]");
        self.put("claimed.json", "[]");
    }

    /// Project root with `.tlc/` and a nested working dir; returns the latter.
    fn project(&self) -> (PathBuf, PathBuf) {
        let proj = self.root.join("proj");
        let deep = proj.join("sub/deep");
        std::fs::create_dir_all(proj.join(".tlc")).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        (proj, deep)
    }

    fn tlc(&self, cwd: &Path) -> Output {
        self.run(
            "bar-tlc",
            &json!({"options":{},"session":{"cwd":cwd},"columns":null,"config":{}}),
        )
    }

    // ---- credits ----

    fn credits_fakes(&self, body: &str, code: &str) {
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

    fn credits(&self) -> Output {
        self.run(
            "bar-claude-credits",
            &json!({"options":{},"session":{},"columns":null,
                    "config":{"org":"org-1","keychain":"claude-console-session-x"}}),
        )
    }
}

fn out_json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "bad json {e}: {:?} / stderr {:?}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

fn tasks(done: usize, skipped: usize, todo: usize, wip: usize) -> String {
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

// ---- bar-tlc ----

#[test]
fn bar_tlc_ext_info_declares_placeholder() {
    let fx = Fx::new();
    let o = Command::new(script("bar-tlc"))
        .arg("--ext-info")
        .output()
        .unwrap();
    assert!(o.status.success());
    let v = out_json(&o);
    assert_eq!(v["name"], "tlc");
    assert!(v["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("placeholder")));
    drop(fx);
}

#[test]
fn bar_tlc_closed_of_total() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let o = fx.tlc(&deep);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = out_json(&o);
    assert_eq!(v["text"], "9\u{2215}13");
    assert_eq!(v["score"], 69);
    assert!(v.get("style").is_none());
}

#[test]
fn bar_tlc_runs_from_project_root() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let at_root = out_json(&fx.tlc(&proj));
    let at_deep = out_json(&fx.tlc(&deep));
    assert_eq!(at_root, at_deep);
    assert!(!deep.join(".tlc").exists());
    assert!(!proj.join("sub").join(".tlc").exists());
    let root = std::fs::canonicalize(&proj).unwrap();
    for line in fx.log("tlc.log").lines() {
        let dir = std::fs::canonicalize(line.split('|').next().unwrap()).unwrap();
        assert_eq!(dir, root, "{line}");
    }
}

#[test]
fn bar_tlc_partial_failure_emits_nothing() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    for pat in ["track list", "IN_PROGRESS", "--archived"] {
        fx.put("fail", pat);
        let o = fx.tlc(&deep);
        assert!(!o.status.success(), "{pat}");
        assert!(
            o.stdout.is_empty(),
            "{pat}: {:?}",
            String::from_utf8_lossy(&o.stdout)
        );
    }
}

#[test]
fn bar_tlc_track_is_latest_updated_claim() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tracks.json", r#"[{"id":"alpha"},{"id":"beta"}]"#);
    fx.put(
        "claimed.json",
        r#"[{"track_id":"alpha","updated_at":"2026-01-01T00:00:00Z"},
            {"track_id":"beta","updated_at":"2026-03-01T00:00:00Z"}]"#,
    );
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks_beta.json", &tasks(1, 0, 1, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let v = out_json(&fx.tlc(&deep));
    assert_eq!(v["text"], "1\u{2215}2");
    assert_eq!(v["score"], 50);
    assert!(fx.log("tlc.log").contains("--track beta"));
}

#[test]
fn bar_tlc_no_tlc_dir_omitted() {
    let fx = Fx::new();
    fx.tlc_fake();
    let bare = fx.root.join("bare/sub");
    std::fs::create_dir_all(&bare).unwrap();
    let o = fx.tlc(&bare);
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
    assert!(!bare.join(".tlc").exists());
}

// ---- bar-claude-credits ----

fn iso(ts: i64) -> String {
    let (days, rem) = (ts.div_euclid(DAY), ts.rem_euclid(DAY));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn credits_body(exp: i64, granted_at: i64) -> String {
    json!({
        "amount": 14906,
        "amount_without_scoped_credits": 100,
        "promo_tranches": [
            {"remaining_amount_minor_units": 4807, "granted_amount_minor_units": 5000,
             "expires_at": iso(exp + 30 * DAY), "granted_at": iso(granted_at)},
            {"remaining_amount_minor_units": 10000, "granted_amount_minor_units": 20000,
             "expires_at": iso(exp), "granted_at": iso(granted_at)}
        ]
    })
    .to_string()
}

#[test]
fn credits_sum_promo_tranches() {
    let fx = Fx::new();
    let exp = now() + 14 * DAY + 3600;
    let granted_at = exp - 30 * DAY;
    fx.credits_fakes(&credits_body(exp, granted_at), "200");
    let o = fx.credits();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = out_json(&o);
    assert_eq!(v["text"], "14d $149.07");
    assert_eq!(
        v["metric"],
        json!({"remaining":14907,"granted":20000,"expires_at":exp,"granted_at":granted_at})
    );
    assert!(v.get("style").is_none());
    assert!(fx
        .log("curl.log")
        .contains("/api/organizations/org-1/prepaid/credits"));
    assert!(fx.log("security.log").contains("claude-console-session-x"));
}

#[test]
fn credits_401_shows_question_mark() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "401");
    let o = fx.credits();
    assert!(o.status.success());
    let v = out_json(&o);
    assert_eq!(v["text"], "?");
    assert_eq!(v["style"], "bad");
}

#[test]
fn credits_503_emits_nothing_so_cache_keeps_value() {
    let fx = Fx::new();
    for code in ["503", "fail"] {
        fx.credits_fakes("{}", code);
        let o = fx.credits();
        assert!(!o.status.success(), "{code}");
        assert!(o.stdout.is_empty(), "{code}");
    }
}

#[test]
fn credits_no_keychain_entry_omitted() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "200");
    std::fs::remove_file(fx.fake.join("cookie")).unwrap();
    let o = fx.credits();
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
    assert!(fx.log("curl.log").is_empty(), "curl must not run");
}

#[test]
fn cookie_never_in_argv() {
    let fx = Fx::new();
    let exp = now() + 14 * DAY + 3600;
    fx.credits_fakes(&credits_body(exp, exp - 30 * DAY), "200");
    let o = fx.credits();
    assert!(o.status.success());
    let argv = fx.log("curl.log");
    assert!(!argv.is_empty());
    assert!(!argv.contains(SENTINEL), "argv leaked: {argv}");
    assert!(!fx.log("security.log").contains(SENTINEL));
    assert!(!fx.log("curl.env").contains(SENTINEL), "env leaked");
    assert!(
        fx.log("curl.stdin").contains(SENTINEL),
        "cookie must arrive on stdin"
    );
    assert!(!String::from_utf8_lossy(&o.stdout).contains(SENTINEL));
}
