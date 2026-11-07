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
fn bar_tlc_ext_info_declares_scope_and_status() {
    let o = Command::new(script("bar-tlc"))
        .arg("--ext-info")
        .output()
        .unwrap();
    let v = out_json(&o);
    assert_eq!(
        v["options"]["scope"],
        json!({"type":"enum","enum":["track","all"],"default":"track"})
    );
    assert_eq!(v["options"]["status"], json!({"type":"string"}));
}

#[test]
fn bar_tlc_scope_all_counts_the_whole_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tracks.json", r#"[{"id":"alpha"}]"#);
    fx.put(
        "claimed.json",
        r#"[{"track_id":"alpha","updated_at":"2026-01-01T00:00:00Z"}]"#,
    );
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let track = |o| out_json(&fx.tlc_with(&deep, o))["text"].clone();
    assert_eq!(track(json!({})), "1\u{2215}4");
    assert_eq!(track(json!({"scope":"track"})), "1\u{2215}4");
    assert_eq!(track(json!({"scope":"all"})), "9\u{2215}9");
    let o = fx.tlc_with(&deep, json!({"scope":"bogus"}));
    assert!(!o.status.success() && o.stdout.is_empty());
}

#[test]
fn bar_tlc_status_filters_the_counted_set() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let listed = |fx: &Fx| -> Vec<String> {
        fx.log("tlc.log")
            .lines()
            .filter(|l| l.contains("task list") && !l.contains("IN_PROGRESS"))
            .map(String::from)
            .collect()
    };
    assert!(out_json(&fx.tlc(&deep))["text"].is_string());
    assert!(
        listed(&fx).iter().all(|l| !l.contains("--status")),
        "default: no filter"
    );
    let o = fx.tlc_with(&deep, json!({"status":"DONE"}));
    assert!(o.status.success());
    assert!(
        listed(&fx).iter().any(|l| l.contains("--status DONE")),
        "{:?}",
        listed(&fx)
    );
    for bad in ["-x", "a b", "$(id)"] {
        let o = fx.tlc_with(&deep, json!({"status":bad}));
        assert!(!o.status.success() && o.stdout.is_empty(), "{bad}");
    }
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
fn bar_tlc_enclosing_project_above_repo_root_is_not_used() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let org = fx.root.join("org");
    std::fs::create_dir_all(org.join(".tlc")).unwrap();
    std::fs::write(org.join(".tlc/config.yaml"), "").unwrap();
    let sub = org.join("repo/sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(org.join("repo/.git"), "gitdir: elsewhere").unwrap();
    for cwd in [org.join("repo"), sub] {
        let o = fx.tlc(&cwd);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(out_json(&o)["text"], "");
    }
    assert_eq!(fx.log("tlc.log"), "", "tlc must not run");
}

#[test]
fn bar_tlc_outside_a_repo_checks_cwd_only() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let proj = fx.root.join("loose");
    std::fs::create_dir_all(proj.join(".tlc")).unwrap();
    std::fs::write(proj.join(".tlc/config.yaml"), "").unwrap();
    let sub = proj.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let o = fx.tlc(&sub);
    assert!(o.status.success());
    assert_eq!(out_json(&o)["text"], "");
    assert_eq!(out_json(&fx.tlc(&proj))["text"], "9\u{2215}13");
}

#[test]
fn bar_tlc_no_tasks_hides_the_segment() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.put("tasks.json", "[]");
    let o = fx.tlc(&deep);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(out_json(&o)["text"], "");
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
    assert!(o.status.success());
    assert_eq!(out_json(&o)["text"], "");
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
        assert!(o.status.success(), "{cwd}");
        assert_eq!(out_json(&o)["text"], "", "{cwd}");
    }
}
