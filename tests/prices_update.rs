// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]
mod common;
use common::tmp::Dir;

use assert_cmd::Command;
use hop_bar::prices::models::Snapshot;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const API: &str = r#"{"openai":{"id":"openai","name":"OpenAI","models":{
"gpt-x":{"id":"gpt-x","name":"X","cost":{"input":1.5,"output":6,"cache_read":0.15,"reasoning":9},"limit":{"context":128000,"output":4096}},
"gpt-free":{"id":"gpt-free","cost":{"input":0,"output":0},"limit":{"context":8000}},
"gpt-nocost":{"id":"gpt-nocost","limit":{"context":8000}}}},
"anthropic":{"id":"anthropic","models":{"claude-z":{"id":"claude-z","cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75},"limit":{"context":200000}}}}}"#;

fn fx(name: &str, curl_body: &str) -> (Dir, PathBuf) {
    let root = Dir::new(&format!("bar-pu-{}-{name}", std::process::id()));
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("curl");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" >> \"{}/argv\"\n{curl_body}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("api.json"), API).unwrap();
    (root, bin)
}

fn bar(root: &Path, bin: &Path) -> Command {
    let mut c = Command::cargo_bin("bar").unwrap();
    c.env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("cfg"))
        .env("XDG_STATE_HOME", root.join("state"));
    c
}

#[test]
fn update_writes_trimmed_private_file() {
    let (root, bin) = fx(
        "ok",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    bar(&root, &bin)
        .args(["prices", "update"])
        .assert()
        .success();
    let path = root.join("data/bar/models.json");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let s = Snapshot::parse(&text).unwrap();
    assert_eq!(s.len(), 3, "free kept, no-cost dropped");
    assert!(s.lookup("gpt-nocost", None).is_none());
    assert_eq!(s.lookup("gpt-free", None), Some([0; 4]));
    assert!(!text.contains("reasoning") && !text.contains("\"name\""));
    let r = s.lookup("gpt-x", Some("openai")).unwrap();
    assert_eq!(r, [1_500_000, 6_000_000, 1_500_000, 150_000]);
    let argv = std::fs::read_to_string(root.join("argv")).unwrap();
    assert!(argv.contains("https://models.dev/api.json") && argv.contains("--max-time"));
    // no stray temp files
    assert_eq!(std::fs::read_dir(root.join("data/bar")).unwrap().count(), 1);
}

#[test]
fn update_failure_keeps_existing_file() {
    let (root, bin) = fx("fail", "echo nope; exit 22");
    let dir = root.join("data/bar");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("models.json"), "KEEP").unwrap();
    bar(&root, &bin)
        .args(["prices", "update"])
        .assert()
        .failure();
    assert_eq!(
        std::fs::read_to_string(dir.join("models.json")).unwrap(),
        "KEEP"
    );
}

#[test]
fn update_rejects_garbage_body() {
    let (root, bin) = fx("junk", "echo '<html>'");
    bar(&root, &bin)
        .args(["prices", "update"])
        .assert()
        .failure();
    assert!(!root.join("data/bar/models.json").exists());
}

#[test]
fn update_out_writes_given_file_sorted_and_stable() {
    let (root, bin) = fx(
        "out",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    let out = root.join("m.json");
    bar(&root, &bin)
        .args(["prices", "update", "--out"])
        .arg(&out)
        .assert()
        .success();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.find("\"anthropic\"").unwrap() < text.find("\"openai\"").unwrap());
    assert!(text.lines().count() > 3, "one model per line for diffs");
    assert!(!root.join("data/bar/models.json").exists());
}

#[test]
fn check_reports_not_fetched_then_user_file() {
    let (root, bin) = fx(
        "chk",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    let out = bar(&root, &bin).arg("check").output().unwrap();
    let t = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        t.contains("prices: models.dev not fetched") && t.contains("models.json"),
        "{t}"
    );
    let j = bar(&root, &bin)
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert!(v["prices"]["fetched"].is_null());
    assert!(v["prices"]["path"]
        .as_str()
        .unwrap()
        .ends_with("models.json"));
    bar(&root, &bin)
        .args(["prices", "update"])
        .assert()
        .success();
    let out = bar(&root, &bin).arg("check").output().unwrap();
    let t = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        t.contains("prices: models.dev snapshot 20") && t.contains("models.json"),
        "{t}"
    );
    let j = bar(&root, &bin)
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert!(v["prices"]["fetched"].as_str().unwrap().starts_with("20"));
}

fn user_file(root: &Path, fetched: &str, valid: bool) {
    let dir = root.join("data/bar");
    std::fs::create_dir_all(&dir).unwrap();
    let body = if valid {
        format!(r#"{{"fetched":"{fetched}","providers":{{}}}}"#)
    } else {
        "garbage".to_string()
    };
    std::fs::write(dir.join("models.json"), body).unwrap();
}

#[test]
fn check_ignores_bad_user_files_and_says_why() {
    for (fetched, valid, why) in [
        ("2099-01-01", true, "future"),
        ("zzz", true, "not a valid price file"),
        ("", false, "not a valid price file"),
    ] {
        let (root, bin) = fx("bad", "true");
        user_file(&root, fetched, valid);
        let t = String::from_utf8_lossy(&bar(&root, &bin).arg("check").output().unwrap().stdout)
            .into_owned();
        assert!(
            t.contains("ignored") && t.contains(why) && !t.contains("snapshot"),
            "{t}"
        );
        let j = bar(&root, &bin)
            .args(["check", "--format", "json"])
            .output()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
        assert!(v["prices"]["fetched"].is_null());
        assert!(v["prices"]["ignored"].as_str().unwrap().contains(why));
    }
}

#[test]
fn check_uses_an_old_user_file() {
    let (root, bin) = fx("old", "true");
    user_file(&root, "2001-01-01", true);
    let t = String::from_utf8_lossy(&bar(&root, &bin).arg("check").output().unwrap().stdout)
        .into_owned();
    assert!(t.contains("models.dev snapshot 2001-01-01"), "{t}");
}

const DAY: u64 = 86_400;

/// A transcript of one request today by `model`, a config showing
/// today's spend, and a `bar render` in that environment.
fn spend_fx<'a>(root: &'a Path, model: &str) -> impl Fn() -> String + 'a {
    let claude = root.join("claude");
    std::fs::create_dir_all(claude.join("projects/p")).unwrap();
    let ts = format!("{}T00:00:01Z", hop_bar::prices::models::today());
    std::fs::write(
        claude.join("projects/p/a.jsonl"),
        format!(
            "{{\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"m\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":1000000,\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
    let cfg = root.join("cfg/bar");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("bar.conf"),
        format!(
            "set claude-dir \"{}\"\nset left \"#{{spendhist[tz=utc;period=today]}}\"\n",
            claude.display()
        ),
    )
    .unwrap();
    let bin = root.join("bin");
    move || {
        let o = bar(root, &bin)
            .arg("render")
            .write_stdin("{}")
            .output()
            .unwrap();
        hop_bar::style::strip_codes(&String::from_utf8_lossy(&o.stdout))
            .trim()
            .to_string()
    }
}

/// A render asked for the background models.dev fetch: the request's
/// lock, result or failure marker is in the cache. Render creates the
/// lock itself, so no waiting on the job is needed to see it.
fn prices_requested(root: &Path) -> bool {
    let c = hop_bar::cache::Cache::with_exe(&root.join("state/bar"), "bar".into());
    let key = hop_bar::placeholders::spendhist::PRICES_KEY;
    [c.path_for(key), c.lock_path(key), c.fail_path(key)]
        .iter()
        .any(|p| p.exists())
}

/// Renders until `want(output)`, or panics after 20 s.
fn render_until(render: &dyn Fn() -> String, want: impl Fn(&str) -> bool) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let out = render();
        if want(&out) {
            return out;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never got there: {out:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn render_fetches_prices_for_an_unpriced_model_then_uses_them() {
    let (root, _) = fx(
        "jit",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    let render = spend_fx(&root, "gpt-x");
    // unknown until the background fetch lands, then priced from it
    let first = render_until(&render, |o| !o.is_empty());
    assert_eq!(first, "$0.00~");
    assert!(prices_requested(&root));
    assert_eq!(render_until(&render, |o| o == "$1.50"), "$1.50");
    let path = root.join("data/bar/models.json");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // fresh copy: further renders fetch nothing
    let calls = std::fs::read_to_string(root.join("argv")).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains("https://models.dev/api.json"));
    for _ in 0..3 {
        assert_eq!(render(), "$1.50");
    }
    assert_eq!(
        std::fs::read_to_string(root.join("argv"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn render_never_fetches_when_every_model_is_priced() {
    let (root, _) = fx(
        "priced",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    let render = spend_fx(&root, "claude-opus-4-5-20251101");
    assert_eq!(render_until(&render, |o| !o.is_empty()), "$5.00");
    for _ in 0..3 {
        render();
    }
    assert!(!prices_requested(&root));
    assert!(!root.join("argv").exists(), "curl ran");
    assert!(!root.join("data/bar/models.json").exists());
}

#[test]
fn render_refetches_a_stale_or_unusable_copy() {
    for (name, stale, valid) in [("stale", true, true), ("junk", false, false)] {
        let (root, _) = fx(
            name,
            &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
        );
        user_file(&root, "2001-01-01", valid);
        let path = root.join("data/bar/models.json");
        if stale {
            let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * DAY);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        let render = spend_fx(&root, "gpt-x");
        render_until(&render, |o| o == "$1.50");
        assert!(
            prices_requested(&root) && root.join("argv").exists(),
            "{name}"
        );
    }
}

#[test]
fn render_keeps_a_fresh_copy() {
    let (root, _) = fx(
        "fresh",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    user_file(&root, "2001-01-01", true);
    let render = spend_fx(&root, "gpt-x");
    assert_eq!(render_until(&render, |o| !o.is_empty()), "$0.00~");
    for _ in 0..3 {
        render();
    }
    assert!(!prices_requested(&root));
    assert!(!root.join("argv").exists(), "curl ran");
}
