//! Reference plugins driven directly: JSON on stdin, JSON on stdout, fakes
//! for `tlc`, `security` and `curl` first on `PATH`.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugins_fx.rs"]
mod plugins_fx;
#[allow(unused_imports)]
use plugins_fx::*;

use serde_json::json;

// ---- bar-tlc ----

#[test]
fn bar_tlc_ext_info_declares_placeholder() {
    let fx = Fx::new();
    let o = ext_info_output("bar-tlc");
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
fn bar_tlc_ext_info_declares_options_and_session_cache() {
    let o = ext_info_output("bar-tlc");
    let v = out_json(&o);
    assert_eq!(
        v["options"]["scope"],
        json!({"type":"enum","enum":["track","all"],"default":"track"})
    );
    assert_eq!(v["options"]["status"], json!({"type":"string"}));
    assert_eq!(
        v["options"]["show"],
        json!({"type":"enum","enum":["count","track"],"default":"count"})
    );
    assert_eq!(
        v["options"]["alt"],
        json!({"type":"enum","enum":["count","track"]})
    );
    assert_eq!(v["cache"], "session");
    for (k, d) in [
        ("label-track", "\u{25c9}"),
        ("label-all", "\u{2630}"),
        ("icon-track", "\u{f45d}"),
        ("icon-all", "\u{f45e}"),
    ] {
        assert_eq!(v["options"][k], json!({"type":"string","default":d}), "{k}");
    }
    assert_eq!(v["version"], "0.4.0");
    // every sample puts the scope toggle on the text, none on the mark
    for s in v["web"]["samples"].as_array().unwrap() {
        assert_eq!(s["output"]["text_toggle"], "scope", "{s}");
        assert!(s["output"].get("label_toggle").is_none(), "{s}");
    }
}

fn rec(task: &str, cwd: &std::path::Path) -> serde_json::Value {
    json!({"task": task, "cwd": cwd, "at": 1_800_000_000})
}

#[test]
fn bar_tlc_scope_all_counts_the_whole_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let r = rec("T-0001", &proj);
    let text = |o| out_json(&fx.tlc_rec(&deep, o, r.clone()))["text"].clone();
    assert_eq!(text(json!({})), "1\u{2215}4");
    assert_eq!(text(json!({"scope":"track"})), "1\u{2215}4");
    assert_eq!(text(json!({"scope":"all"})), "9\u{2215}9");
    let o = fx.tlc_rec(&deep, json!({"scope":"bogus"}), r.clone());
    assert!(!o.status.success() && o.stdout.is_empty());
}

fn mark(v: &serde_json::Value) -> (String, String, String) {
    let f = |k: &str| v[k].as_str().unwrap().to_string();
    (f("label"), f("icon"), f("text"))
}

#[test]
fn bar_tlc_marks_track_and_all_scope() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let r = rec("T-0001", &proj);
    let go = |o, rec: serde_json::Value| out_json(&fx.tlc_rec(&deep, o, rec));
    // track mode: track mark, track count
    let v = go(json!({}), r.clone());
    assert_eq!(
        mark(&v),
        ("\u{25c9}".into(), "\u{f45d}".into(), "1\u{2215}4".into())
    );
    // a click on the text flips the scope; the mark keeps the alt toggle
    assert_eq!(v["text_toggle"], "scope");
    assert!(v.get("label_toggle").is_none(), "{v}");
    // all mode and no record: all mark
    let v = go(json!({"scope":"all"}), r.clone());
    assert_eq!(
        mark(&v),
        ("\u{2630}".into(), "\u{f45e}".into(), "9\u{2215}9".into())
    );
    let v = go(json!({}), serde_json::Value::Null);
    assert_eq!(
        (v["label"].clone(), v["icon"].clone()),
        (json!("\u{2630}"), json!("\u{f45e}"))
    );
}

#[test]
fn bar_tlc_mark_options_override() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let r = rec("T-0001", &proj);
    let o = json!({"label-track":"T","label-all":"A","icon-track":"it","icon-all":"ia"});
    let v = out_json(&fx.tlc_rec(&deep, o.clone(), r.clone()));
    assert_eq!(
        (v["label"].clone(), v["icon"].clone()),
        (json!("T"), json!("it"))
    );
    let v = out_json(&fx.tlc_rec(
        &deep,
        json!({"scope":"all","label-track":"T","label-all":"A","icon-all":"ia"}),
        r.clone(),
    ));
    assert_eq!(
        (v["label"].clone(), v["icon"].clone()),
        (json!("A"), json!("ia"))
    );
    // an empty override means no mark
    let v = out_json(&fx.tlc_rec(&deep, json!({"label-track":""}), r));
    assert_eq!(v["label"], "");
}

#[test]
fn bar_tlc_scope_toggle_inverts_the_scope() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.tlc_task("T-0002", None, "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    let on = json!({"scope": true});
    let run = |o: serde_json::Value, t: &str, tog: &serde_json::Value| {
        let rec = if t.is_empty() {
            serde_json::Value::Null
        } else {
            rec(t, &proj)
        };
        let v = out_json(&fx.tlc_tog(&deep, o, rec, tog.clone()));
        (
            v["text"].as_str().unwrap().to_string(),
            v["label"].as_str().unwrap().to_string(),
        )
    };
    let (trk, all) = ("1\u{2215}4".to_string(), "9\u{2215}9".to_string());
    let (mt, ma) = ("\u{25c9}".to_string(), "\u{2630}".to_string());
    // off (or false): unchanged
    assert_eq!(
        run(json!({}), "T-0001", &json!({})),
        (trk.clone(), mt.clone())
    );
    assert_eq!(
        run(json!({}), "T-0001", &json!({"scope": false})),
        (trk.clone(), mt.clone())
    );
    // on: track -> all, all -> track
    assert_eq!(run(json!({}), "T-0001", &on), (all.clone(), ma.clone()));
    assert_eq!(run(json!({"scope":"all"}), "T-0001", &on), (trk, mt));
    // no session track (no record, or a task without track): stays all
    assert_eq!(
        run(json!({"scope":"all"}), "", &on),
        (all.clone(), ma.clone())
    );
    assert_eq!(
        run(json!({"scope":"all"}), "T-0002", &on),
        (all.clone(), ma.clone())
    );
    assert_eq!(run(json!({}), "", &on), (all.clone(), ma.clone()));
    assert_eq!(run(json!({}), "T-0002", &on), (all, ma));
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

fn tlc_cfg(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join(".tlc")).unwrap();
    std::fs::write(dir.join(".tlc/config.yaml"), "").unwrap();
}

fn tlc_worktree(wt: &std::path::Path, gitdir: &str) {
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {gitdir}\n")).unwrap();
}

fn tlc_text_at(fx: &Fx, cwd: &std::path::Path) -> serde_json::Value {
    let o = fx.tlc(cwd);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    out_json(&o)["text"].clone()
}

#[test]
fn bar_tlc_bare_repo_hops_worktree_finds_bare_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let org = fx.root.join("org");
    tlc_cfg(&org);
    let bare = org.join("repo");
    tlc_cfg(&bare);
    let wt = bare.join("hops/feat/a");
    tlc_worktree(&wt, &format!("{}/worktrees/a", bare.display()));
    std::fs::create_dir_all(bare.join("worktrees/a")).unwrap();
    assert_eq!(tlc_text_at(&fx, &wt.join("src")), "9\u{2215}13");
    let root = std::fs::canonicalize(&bare).unwrap();
    for line in fx.log("tlc.log").lines() {
        let dir = std::fs::canonicalize(line.split('|').next().unwrap()).unwrap();
        assert_eq!(dir, root, "{line}");
    }
}

#[test]
fn bar_tlc_worktree_outside_bare_dir_finds_bare_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let bare = fx.root.join("bare");
    tlc_cfg(&bare);
    std::fs::create_dir_all(bare.join("worktrees/w")).unwrap();
    let wt = fx.root.join("elsewhere/w");
    tlc_worktree(&wt, &format!("{}/worktrees/w", bare.display()));
    assert_eq!(tlc_text_at(&fx, &wt.join("src")), "9\u{2215}13");
}

#[test]
fn bar_tlc_linked_worktree_of_normal_repo_finds_main_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let main = fx.root.join("main");
    tlc_cfg(&main);
    std::fs::create_dir_all(main.join(".git/worktrees/w")).unwrap();
    let wt = fx.root.join("elsewhere/w");
    tlc_worktree(&wt, &format!("{}/.git/worktrees/w", main.display()));
    assert_eq!(tlc_text_at(&fx, &wt.join("src")), "9\u{2215}13");
}

#[test]
fn bar_tlc_worktree_commondir_file_is_honoured() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let main = fx.root.join("main");
    tlc_cfg(&main);
    let gd = main.join(".git/wt-meta/w");
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    let wt = fx.root.join("elsewhere/w");
    tlc_worktree(&wt, &gd.display().to_string());
    assert_eq!(tlc_text_at(&fx, &wt.join("src")), "9\u{2215}13");
}

#[test]
fn bar_tlc_worktree_never_climbs_above_repo_root() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    let org = fx.root.join("org");
    tlc_cfg(&org);
    let bare = org.join("repo");
    std::fs::create_dir_all(bare.join("worktrees/a")).unwrap();
    let wt = bare.join("hops/feat/a");
    tlc_worktree(&wt, &format!("{}/worktrees/a", bare.display()));
    assert_eq!(tlc_text_at(&fx, &wt.join("src")), "");
    assert_eq!(fx.log("tlc.log"), "");
}

#[test]
fn bar_tlc_partial_failure_emits_nothing() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    for pat in ["task show", "track show", "--archived"] {
        fx.put("fail", pat);
        let o = fx.tlc_rec(&deep, json!({}), rec("T-0001", &proj));
        assert!(!o.status.success(), "{pat}");
        assert!(
            o.stdout.is_empty(),
            "{pat}: {:?}",
            String::from_utf8_lossy(&o.stdout)
        );
    }
}

#[test]
fn bar_tlc_track_is_the_session_records_task() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.tlc_task("T-0002", Some("beta"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks_beta.json", &tasks(1, 0, 1, 0));
    fx.put("tasks.json", &tasks(9, 0, 0, 0));
    // two sessions in one directory, each with its own task
    let a = out_json(&fx.tlc_rec(&deep, json!({}), rec("T-0001", &deep)));
    let b = out_json(&fx.tlc_rec(&deep, json!({}), rec("T-0002", &proj)));
    assert_eq!(a["text"], "1\u{2215}4");
    assert_eq!(a["alt"], "alpha-slug");
    assert_eq!(b["text"], "1\u{2215}2");
    assert_eq!(b["alt"], "beta-slug");
    assert_eq!(b["score"], 50);
    assert!(fx.log("tlc.log").contains("--track beta"));
}

#[test]
fn bar_tlc_without_matching_record_counts_the_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.tlc_task("T-0003", None, "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 1, 0));
    let other = fx.root.join("other");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    tlc_cfg(&other);
    let cases = [
        ("no record", serde_json::Value::Null),
        ("other project", rec("T-0001", &other)),
        ("outside any project", rec("T-0001", &fx.root)),
        ("task without track", rec("T-0003", &deep)),
        ("task gone", rec("T-0404", &deep)),
        ("bad id", rec("-x", &deep)),
        ("relative cwd", json!({"task":"T-0001","cwd":"proj"})),
    ];
    for (what, r) in cases {
        let o = fx.tlc_rec(&deep, json!({}), r);
        assert!(o.status.success(), "{what}");
        let v = out_json(&o);
        assert_eq!(v["text"], "9\u{2215}10", "{what}");
        assert_eq!(v["alt"], "all", "{what}");
    }
    assert!(!fx.log("tlc.log").contains("--track"));
    assert!(!fx.log("tlc.log").contains("-x"));
}

#[test]
fn bar_tlc_completed_task_keeps_its_track() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "DONE");
    fx.put("tasks_alpha.json", &tasks(4, 0, 0, 0));
    fx.put("tasks.json", &tasks(9, 0, 1, 0));
    let v = out_json(&fx.tlc_rec(&deep, json!({}), rec("T-0001", &proj)));
    assert_eq!(v["text"], "4\u{2215}4");
    assert_eq!(v["alt"], "alpha-slug");
}

#[test]
fn bar_tlc_show_and_alt_pick_count_or_track() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (proj, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 1, 0));
    let r = rec("T-0001", &proj);
    let pair = |o| {
        let v = out_json(&fx.tlc_rec(&deep, o, r.clone()));
        (v["text"].clone(), v["alt"].clone())
    };
    assert_eq!(pair(json!({})), (json!("1\u{2215}4"), json!("alpha-slug")));
    assert_eq!(
        pair(json!({"show":"track"})),
        (json!("alpha-slug"), json!("1\u{2215}4"))
    );
    assert_eq!(
        pair(json!({"show":"count","alt":"count"})),
        (json!("1\u{2215}4"), json!("1\u{2215}4"))
    );
    let v = out_json(&fx.tlc_rec(&deep, json!({"show":"track"}), serde_json::Value::Null));
    assert_eq!(v["text"], "all");
    for bad in [json!({"show":"x"}), json!({"alt":"x"})] {
        let o = fx.tlc_rec(&deep, bad.clone(), r.clone());
        assert!(!o.status.success() && o.stdout.is_empty(), "{bad}");
    }
}

#[test]
fn bar_tlc_record_from_linked_worktree_matches_repo_project() {
    let fx = Fx::new();
    fx.tlc_fake();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(9, 0, 1, 0));
    let main = fx.root.join("repo");
    std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    tlc_cfg(&main);
    let wt = fx.root.join("wt");
    tlc_worktree(&wt, &format!("{}/.git/worktrees/wt", main.display()));
    let v = out_json(&fx.tlc_rec(&main, json!({}), rec("T-0001", &wt.join("src"))));
    assert_eq!(v["alt"], "alpha-slug");
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
