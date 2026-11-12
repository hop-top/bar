//! `bar check` warnings for a `plugin` or `action` line that a later line
//! of the same file replaces.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::check::{check_layers, check_text, Level};
use std::path::PathBuf;

fn warnings(r: &hop_bar::check::Report) -> Vec<(usize, String)> {
    r.issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .map(|i| (i.line, i.message.clone()))
        .collect()
}

#[test]
fn a_second_plugin_line_for_the_same_name_warns() {
    let conf = "plugin claude-credits org.a=1\nset gap 2\nplugin claude-credits org.b=2\n";
    let w = warnings(&check_text(conf, None, &[]));
    assert_eq!(w.len(), 1, "{w:?}");
    let (line, msg) = &w[0];
    assert_eq!(*line, 3);
    assert!(msg.contains("`plugin claude-credits`"), "{msg}");
    assert!(msg.contains("line 1"), "{msg}");
    assert!(msg.contains("one line"), "{msg}");
}

#[test]
fn each_repeat_names_the_line_it_replaces() {
    let conf = "plugin p a=1\nplugin p b=2\nplugin p c=3\n";
    let w = warnings(&check_text(conf, None, &[]));
    let lines: Vec<_> = w
        .iter()
        .map(|(l, m)| (*l, m.contains(&format!("line {}", l - 1))))
        .collect();
    assert_eq!(lines, vec![(2, true), (3, true)], "{w:?}");
}

#[test]
fn distinct_plugins_do_not_warn() {
    let conf = "plugin p a=1\nplugin q a=1\n# plugin p b=2\n";
    assert!(warnings(&check_text(conf, None, &[])).is_empty());
}

#[test]
fn a_second_action_line_for_the_same_placeholder_warns() {
    let conf = "action echo echo.use\naction echo echo.other\naction foo foo.use\n";
    let w: Vec<_> = warnings(&check_text(conf, None, &[]))
        .into_iter()
        .filter(|(_, m)| m.contains("replaces"))
        .collect();
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].0, 2);
    assert!(w[0].1.contains("`action echo`"), "{}", w[0].1);
    assert!(w[0].1.contains("line 1"), "{}", w[0].1);
}

#[test]
fn a_project_plugin_line_over_the_users_is_layering_not_a_repeat() {
    let user = (
        PathBuf::from("bar.conf"),
        Ok("set project-config full\nplugin p a=1\n".to_string()),
    );
    let proj = (PathBuf::from(".bar.conf"), Ok("plugin p a=2\n".to_string()));
    let r = check_layers(Some(user), Some(proj), &[]);
    assert!(warnings(&r).is_empty(), "{:?}", r.issues);
}

#[test]
fn a_repeat_within_the_project_file_warns_there() {
    let user = (
        PathBuf::from("bar.conf"),
        Ok("set project-config full\n".to_string()),
    );
    let proj = (
        PathBuf::from(".bar.conf"),
        Ok("plugin p a=1\nplugin p a=2\n".to_string()),
    );
    let r = check_layers(Some(user), Some(proj), &[]);
    let w: Vec<_> = r
        .issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .collect();
    assert_eq!(w.len(), 1, "{:?}", r.issues);
    assert_eq!(w[0].line, 2);
    assert_eq!(w[0].layer, hop_bar::conf::Layer::Project);
}

#[test]
fn check_json_carries_the_warning() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "plugin p a=1\nplugin p b=2\n");
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
    let dup: Vec<_> = issues
        .iter()
        .filter(|i| i["message"].as_str().unwrap().contains("`plugin p`"))
        .collect();
    assert_eq!(dup.len(), 1, "{issues:?}");
    assert_eq!(dup[0]["level"], "warning");
    assert_eq!(dup[0]["line"], 2);
    assert!(dup[0]["file"].as_str().unwrap().ends_with("bar.conf"));
}
