//! Reference plugins driven directly: JSON on stdin, JSON on stdout, fakes
//! for `tlc`, `security` and `curl` first on `PATH`.
mod common;
#[path = "common/plugins_fx.rs"]
mod plugins_fx;
#[allow(unused_imports)]
use plugins_fx::*;

use serde_json::json;
use std::process::Command;

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

#[test]
fn bar_tlc_skips_stray_empty_tlc_dir() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    std::fs::create_dir_all(proj.join("sub/.tlc")).unwrap();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let v = out_json(&fx.tlc(&deep));
    assert_eq!(v["text"], "9\u{2215}13");
    let root = std::fs::canonicalize(&proj).unwrap();
    for line in fx.log("tlc.log").lines() {
        let dir = std::fs::canonicalize(line.split('|').next().unwrap()).unwrap();
        assert_eq!(dir, root, "{line}");
    }
}

#[test]
fn bar_tlc_relative_or_dash_cwd_terminates() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.project();
    fx.put("tasks.json", &tasks(1, 0, 1, 0));
    for cwd in ["relative/dir", "-x", "."] {
        let o = fx.run(
            "bar-tlc",
            &json!({"options":{},"session":{"cwd":cwd},"columns":null,"config":{}}),
        );
        assert!(!o.status.success(), "{cwd}");
        assert!(o.stdout.is_empty(), "{cwd}");
    }
}
