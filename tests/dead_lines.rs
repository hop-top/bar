//! `bar check` warnings for a `rule` or `account` line that never applies
//! because an earlier line of the same file matches first.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::check::{check_layers, check_text, Level};
use std::path::PathBuf;

const RULE: &str = "early warn=20% bad=5% final=1h bad=30%";

fn warnings(r: &hop_bar::check::Report) -> Vec<(usize, String)> {
    r.issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .map(|i| (i.line, i.message.clone()))
        .collect()
}

fn dead(conf: &str) -> Vec<(usize, String)> {
    warnings(&check_text(conf, None, &[]))
        .into_iter()
        .filter(|(_, m)| m.contains("never applies"))
        .collect()
}

#[test]
fn a_second_rule_for_the_same_target_warns() {
    let conf = format!(
        "rule usage[type=5h] {RULE}\nset gap 2\nrule usage[type=5h] early pace final=2h bad=10%\n"
    );
    let w = dead(&conf);
    assert_eq!(w.len(), 1, "{w:?}");
    let (line, msg) = &w[0];
    assert_eq!(*line, 3);
    assert!(msg.contains("`rule usage[type=5h]`"), "{msg}");
    assert!(msg.contains("line 1"), "{msg}");
    assert!(msg.contains("first"), "{msg}");
    assert!(msg.contains("remove"), "{msg}");
}

#[test]
fn each_dead_rule_names_the_first_line() {
    let conf = format!("rule extra {RULE}\nrule extra {RULE}\nrule extra {RULE}\n");
    let w = dead(&conf);
    let lines: Vec<_> = w.iter().map(|(l, m)| (*l, m.contains("line 1"))).collect();
    assert_eq!(lines, vec![(2, true), (3, true)], "{w:?}");
}

#[test]
fn rules_for_different_targets_do_not_warn() {
    let conf = format!(
        "rule usage[type=5h] {RULE}\nrule usage[type=7d] {RULE}\nrule usage[type=7d;model=opus] {RULE}\nrule extra {RULE}\n# rule extra {RULE}\n"
    );
    assert!(dead(&conf).is_empty());
}

#[test]
fn an_invalid_earlier_rule_does_not_make_a_later_one_dead() {
    let conf = format!("rule extra early warn=nope\nrule extra {RULE}\n");
    let r = check_text(&conf, None, &[]);
    assert!(
        r.issues
            .iter()
            .any(|i| i.level == Level::Error && i.line == 1),
        "{:?}",
        r.issues
    );
    assert!(dead(&conf).is_empty());
}

#[test]
fn a_second_account_line_for_the_same_glob_warns() {
    let conf = "account \"*@work.test\" work 33\naccount \"*@work.test\" job 34\n";
    let w = dead(conf);
    assert_eq!(w.len(), 1, "{w:?}");
    let (line, msg) = &w[0];
    assert_eq!(*line, 2);
    assert!(msg.contains("`account \"*@work.test\"`"), "{msg}");
    assert!(msg.contains("line 1"), "{msg}");
}

#[test]
fn an_account_glob_covered_by_an_earlier_one_warns() {
    let conf =
        "account \"*\" any 33\naccount \"alice*\" alice 34\naccount \"*@work.test\" work 35\n";
    let w = dead(conf);
    let lines: Vec<_> = w.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, vec![2, 3], "{w:?}");
    assert!(w[0].1.contains("`account \"*\"`"), "{}", w[0].1);
    assert!(w[0].1.contains("line 1"), "{}", w[0].1);
    assert!(w[0].1.contains("above"), "{}", w[0].1);
}

#[test]
fn a_covered_glob_names_the_first_line_that_covers_it() {
    let conf = "account \"bob@*\" bob 33\naccount \"*@work.test\" work 34\naccount \"a*@work.test\" a 35\n";
    let w = dead(conf);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].0, 3);
    assert!(w[0].1.contains("line 2"), "{}", w[0].1);
}

#[test]
fn overlapping_globs_that_do_not_cover_do_not_warn() {
    // each later glob matches something no earlier one does
    let conf = concat!(
        "account \"alice*\" a 33\n",
        "account \"*\" any 34\n",
        "account \"*a\" end 35\n",
        "account \"*@work.test\" w 36\n",
        "account \"*@Work.test\" W 37\n",
        "account \"a*b\" ab 38\n",
        "account \"a*\" a2 39\n",
    );
    let w = dead(conf);
    // only the lines after `*` (line 2) are dead
    let lines: Vec<_> = w.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, vec![3, 4, 5, 6, 7], "{w:?}");
    assert!(w.iter().all(|(_, m)| m.contains("line 2")), "{w:?}");
}

#[test]
fn globs_without_a_covering_earlier_line_do_not_warn() {
    let conf = concat!(
        "account \"alice*\" a 33\n",
        "account \"*a\" end 35\n",
        "account \"*@work.test\" w 36\n",
        "account \"*@Work.test\" W 37\n",
        "account \"a*b\" ab 38\n",
        "account \"*\" any 34\n",
    );
    assert!(dead(conf).is_empty(), "{:?}", dead(conf));
}

#[test]
fn a_question_mark_is_literal_in_a_glob() {
    // bar matches `?` literally: `a?c` does not cover `abc`
    let conf = "account \"a?c\" q 33\naccount \"abc\" abc 34\n";
    assert!(dead(conf).is_empty());
}

#[test]
fn an_earlier_line_with_an_empty_label_does_not_make_a_later_one_dead() {
    // a plugin matching the lines itself skips a line with nothing to show
    let conf = "account \"*\" \"\" 33\naccount \"alice*\" alice 34\n";
    let r = check_text(conf, None, &[]);
    assert!(
        r.issues.iter().all(|i| i.level != Level::Error),
        "{:?}",
        r.issues
    );
    assert!(dead(conf).is_empty());
}

#[test]
fn a_project_rule_or_account_over_the_users_is_layering_not_dead() {
    let user = (
        PathBuf::from("bar.conf"),
        Ok(format!(
            "set project-config full\nrule extra {RULE}\naccount \"*\" any 33\n"
        )),
    );
    let proj = (
        PathBuf::from(".bar.conf"),
        Ok("rule extra early pace final=2h bad=10%\naccount \"*\" all 34\n".to_string()),
    );
    let r = check_layers(Some(user), Some(proj), &[]);
    assert!(warnings(&r).is_empty(), "{:?}", r.issues);
}

#[test]
fn a_dead_line_within_the_project_file_warns_there() {
    let user = (
        PathBuf::from("bar.conf"),
        Ok("set project-config full\n".to_string()),
    );
    let proj = (
        PathBuf::from(".bar.conf"),
        Ok(format!(
            "rule extra {RULE}\nrule extra {RULE}\naccount \"*\" a 33\naccount \"b*\" b 34\n"
        )),
    );
    let r = check_layers(Some(user), Some(proj), &[]);
    let w: Vec<_> = r
        .issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .map(|i| (i.line, i.layer))
        .collect();
    let p = hop_bar::conf::Layer::Project;
    assert_eq!(w, vec![(2, p), (4, p)], "{:?}", r.issues);
}

#[test]
fn check_json_carries_the_warning() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "account \"*\" any 33\naccount \"me@x.test\" me 34\n",
    );
    t.write("work/.keep", "");
    let (iso, _) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .args(["check", "--format", "json"])
        .current_dir(t.path().join("work"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "a warning is not an error");
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["ok"], true);
    let issues = v["issues"].as_array().unwrap();
    let dead: Vec<_> = issues
        .iter()
        .filter(|i| i["message"].as_str().unwrap().contains("never applies"))
        .collect();
    assert_eq!(dead.len(), 1, "{issues:?}");
    assert_eq!(dead[0]["level"], "warning");
    assert_eq!(dead[0]["line"], 2);
    assert!(dead[0]["file"].as_str().unwrap().ends_with("bar.conf"));
}
