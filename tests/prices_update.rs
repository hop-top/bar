// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

use assert_cmd::Command;
use hop_bar::prices::models::Snapshot;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const API: &str = r#"{"openai":{"id":"openai","name":"OpenAI","models":{
"gpt-x":{"id":"gpt-x","name":"X","cost":{"input":1.5,"output":6,"cache_read":0.15,"reasoning":9},"limit":{"context":128000,"output":4096}},
"gpt-free":{"id":"gpt-free","cost":{"input":0,"output":0},"limit":{"context":8000}},
"gpt-nocost":{"id":"gpt-nocost","limit":{"context":8000}}}},
"anthropic":{"id":"anthropic","models":{"claude-z":{"id":"claude-z","cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75},"limit":{"context":200000}}}}}"#;

fn fx(name: &str, curl_body: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("bar-pu-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("curl");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" > \"{}/argv\"\n{curl_body}\n",
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
fn check_reports_embedded_then_user_file() {
    let (root, bin) = fx(
        "chk",
        &format!("cat \"{}/api.json\"", "$(dirname \"$0\")/.."),
    );
    let out = bar(&root, &bin).arg("check").output().unwrap();
    let t = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        t.contains("prices: models.dev snapshot 20") && t.contains("built in"),
        "{t}"
    );
    bar(&root, &bin)
        .args(["prices", "update"])
        .assert()
        .success();
    let out = bar(&root, &bin).arg("check").output().unwrap();
    let t = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(t.contains("models.json") && t.contains("user file"), "{t}");
    let j = bar(&root, &bin)
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert!(v["prices"]["user_file"]
        .as_str()
        .unwrap()
        .ends_with("models.json"));
    assert!(v["prices"]["snapshot"].as_str().unwrap().starts_with("20"));
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
        ("2001-01-01", true, "older"),
        ("zzz", true, "not a valid price file"),
        ("", false, "not a valid price file"),
    ] {
        let (root, bin) = fx("bad", "true");
        user_file(&root, fetched, valid);
        let t = String::from_utf8_lossy(&bar(&root, &bin).arg("check").output().unwrap().stdout)
            .into_owned();
        assert!(
            t.contains("built in") && t.contains("ignored") && t.contains(why),
            "{t}"
        );
        let j = bar(&root, &bin)
            .args(["check", "--format", "json"])
            .output()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
        assert!(v["prices"]["user_file"].is_null());
        assert!(v["prices"]["ignored"].as_str().unwrap().contains(why));
    }
}
