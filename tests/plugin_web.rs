// Unix fixtures (shell scripts, modes); the `web` object of `--ext-info`.
#![cfg(unix)]

mod common;

use assert_cmd::Command;
use hop_bar::plugin::{parse_info, web, ExtInfo};
use hop_bar::style::strip_codes;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const NOW: i64 = 1_800_000_000;

fn tlc_like(web: Value) -> ExtInfo {
    parse_info(
        &json!({
            "name": "demo",
            "version": "1.0.0",
            "capabilities": ["placeholder"],
            "options": {
                "scope": {"type": "enum", "enum": ["track", "all"], "default": "track"},
                "verbose": {"type": "bool"},
                "limit": {"type": "int"},
                "who": {"type": "string"},
            },
            "web": web,
        })
        .to_string(),
    )
    .unwrap()
}

fn good_web() -> Value {
    json!({
        "description": "Demo tasks",
        "category": "tasks",
        "samples": [
            {"when": {"scope": "all"}, "output": {"text": "12/30", "label": "all"}},
            {"when": {"verbose": true, "limit": 3}, "output": {"text": "verbose"}},
            {"output": {"text": "7/9", "label": "track", "style": "good"}},
        ],
        "options": {
            "scope": {"label": "Scope", "help": "Which tasks", "choices": ["track"]},
            "who": {"placeholder": "@me", "choices": ["@me", "anyone"]},
        },
    })
}

fn typed(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn ext_info_keeps_the_web_object() {
    let info = tlc_like(good_web());
    assert_eq!(info.web, Some(good_web()));
    assert_eq!(info.to_json()["web"], good_web());
    let plain = parse_info(r#"{"name":"x","version":"1"}"#).unwrap();
    assert_eq!(plain.web, None);
    assert!(plain.to_json().get("web").is_none());
}

#[test]
fn a_valid_web_object_has_no_issues() {
    assert!(web::issues(&tlc_like(good_web())).is_empty());
    let none = parse_info(r#"{"name":"x","version":"1"}"#).unwrap();
    assert!(web::issues(&none).is_empty());
}

#[test]
fn issues_name_each_problem() {
    let cases = [
        (json!("nope"), "must be an object"),
        (json!({"category": 3}), "`category` must be a string"),
        (json!({"samples": {}}), "`samples` must be an array"),
        (
            json!({"samples": [{"output": {"label": "x"}}]}),
            "samples[0]: `output`",
        ),
        (
            json!({"samples": [{"when": {"bogus": "1"}, "output": {"text": "x"}}]}),
            "undeclared option `bogus`",
        ),
        (
            json!({"samples": [{"when": {"scope": "none"}, "output": {"text": "x"}}]}),
            "`when.scope` is not a valid value",
        ),
        (
            json!({"options": {"bogus": {}}}),
            "options.bogus: not a declared option",
        ),
        (
            json!({"options": {"scope": {"choices": ["none"]}}}),
            "options.scope: choice \"none\" is not a valid value",
        ),
        (
            json!({"options": {"who": {"label": 1}}}),
            "options.who: `label` must be a string",
        ),
    ];
    for (w, want) in cases {
        let got = web::issues(&tlc_like(w.clone()));
        assert!(got.iter().any(|m| m.contains(want)), "{w}: {got:?}");
    }
}

#[test]
fn sample_picks_the_first_matching_when() {
    let info = tlc_like(good_web());
    let all = web::sample(&info, &typed(&[("scope", json!("all"))]));
    assert_eq!(all.text, "12/30");
    let track = web::sample(&info, &typed(&[("scope", json!("track"))]));
    assert_eq!(track.text, "7/9");
    let both = typed(&[("verbose", json!(true)), ("limit", json!(3))]);
    assert_eq!(web::sample(&info, &both).text, "verbose");
    let one = typed(&[("verbose", json!(true))]);
    assert_eq!(web::sample(&info, &one).text, "7/9");
}

#[test]
fn sample_without_web_is_the_plugin_name() {
    let info =
        parse_info(r#"{"name":"mem","version":"1","capabilities":["placeholder"]}"#).unwrap();
    assert_eq!(web::sample(&info, &Map::new()).text, "mem");
}

#[test]
fn manifest_lists_placeholder_plugins_and_parses_back() {
    let a = tlc_like(good_web());
    let b = parse_info(r#"{"name":"other","version":"1","capabilities":["discover"]}"#).unwrap();
    let m = web::manifest(&[a.clone(), b]);
    assert_eq!(m["plugins"].as_array().unwrap().len(), 1);
    let back = web::parse_manifest(&m.to_string());
    assert_eq!(back, vec![a.clone()]);
    let bare = Value::Array(vec![a.to_json()]).to_string();
    assert_eq!(web::parse_manifest(&bare).len(), 1);
    assert!(web::parse_manifest("not json").is_empty());
}

fn manifest() -> String {
    web::manifest(&[tlc_like(good_web())]).to_string()
}

#[test]
fn render_paints_a_plugin_sample() {
    let out = hop_bar::web::render("set left \"#{demo}\"", "{}", 80, NOW, &manifest());
    assert_eq!(strip_codes(&out).trim(), "track 7/9");
    // style good: the configured good colour
    assert!(out.contains("\x1b[38;5;114m7/9"), "{out:?}");
    let all = hop_bar::web::render(
        "set left \"#{demo[scope=all]}\"",
        "{}",
        80,
        NOW,
        &manifest(),
    );
    assert_eq!(strip_codes(&all).trim(), "all 12/30");
}

#[test]
fn render_draws_nothing_for_options_a_plugin_rejects() {
    let out = hop_bar::web::render(
        "set left \"#{demo[scope=bogus]}\"",
        "{}",
        80,
        NOW,
        &manifest(),
    );
    assert_eq!(strip_codes(&out).trim(), "");
}

#[test]
fn render_draws_the_name_for_an_unlisted_plugin() {
    let out = hop_bar::web::render("set left \"#{mystery[x=1]}\"", "{}", 80, NOW, "");
    assert_eq!(strip_codes(&out).trim(), "mystery");
}

#[test]
fn render_uses_the_icon_of_the_sample_when_icons_are_on() {
    let w = json!({"samples": [{"output": {"text": "7/9", "label": "track", "icon": "\u{f45d}"}}]});
    let m = web::manifest(&[tlc_like(w)]).to_string();
    let conf = "set icons nerdfont\nset left \"#{demo}\"";
    let out = strip_codes(&hop_bar::web::render(conf, "{}", 80, NOW, &m));
    assert_eq!(out.trim(), "\u{f45d} 7/9");
}

#[test]
fn web_check_validates_manifest_plugin_options() {
    let v = hop_bar::web::check("set left \"#{demo[scope=bogus]}\"", &manifest());
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["issues"][0]["line"], 1);
    let ok = hop_bar::web::check("set left \"#{demo[scope=all]}\"", &manifest());
    assert_eq!(ok, json!({"ok": true, "issues": []}));
}

#[test]
fn web_plugins_maps_options_and_hints() {
    let v = hop_bar::web::plugins(&manifest());
    let p = &v[0];
    assert_eq!(p["name"], "demo");
    assert_eq!(p["description"], "Demo tasks");
    assert_eq!(p["category"], "tasks");
    let opt = |k: &str| {
        p["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["key"] == k)
            .cloned()
            .unwrap()
    };
    assert_eq!(opt("scope")["kind"], "one");
    assert_eq!(opt("scope")["values"], json!(["track", "all"]));
    assert_eq!(opt("scope")["label"], "Scope");
    assert_eq!(opt("verbose")["values"], json!(["true", "false"]));
    assert_eq!(opt("who")["kind"], "any");
    assert_eq!(opt("who")["values"], json!(["@me", "anyone"]));
    assert_eq!(opt("who")["placeholder"], "@me");
}

// --- reference plugins and the command ---

fn root() -> PathBuf {
    let r = std::env::temp_dir().join(format!(
        "bar-plugin-web-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&r).unwrap();
    r
}

fn reference() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins")
}

#[test]
fn reference_plugins_declare_valid_web_objects() {
    for e in std::fs::read_dir(reference()).unwrap().flatten() {
        let out = std::process::Command::new(e.path())
            .arg("--ext-info")
            .output()
            .unwrap();
        let info = parse_info(&String::from_utf8_lossy(&out.stdout)).unwrap();
        assert!(info.web.is_some(), "{}", info.name);
        assert_eq!(web::issues(&info), Vec::<String>::new(), "{}", info.name);
    }
}

fn bar(root: &Path) -> Command {
    let (env, cwd) = common::isolation(root);
    let mut c = Command::cargo_bin("bar").unwrap();
    c.envs(env).current_dir(cwd);
    c
}

#[test]
fn web_plugins_writes_the_manifest_of_a_dir() {
    let r = root();
    let out = bar(&r)
        .args(["web", "plugins", "--dir"])
        .arg(reference())
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ci", "claude-credits", "health", "mem", "tlc"]);
    assert_eq!(v["plugins"][4]["web"]["category"], "tasks");

    let file = r.join("plugins.json");
    bar(&r)
        .args(["web", "plugins", "--dir"])
        .arg(reference())
        .arg("-o")
        .arg(&file)
        .assert()
        .success();
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written, v);
    // an existing file is kept without --force
    std::fs::write(&file, "mine\n").unwrap();
    let o = bar(&r)
        .args(["web", "plugins", "--dir"])
        .arg(reference())
        .arg("-o")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--force"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine\n");
    bar(&r)
        .args(["web", "plugins", "--force", "--dir"])
        .arg(reference())
        .arg("-o")
        .arg(&file)
        .assert()
        .success();
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written, v);
    let left: Vec<_> = std::fs::read_dir(&r)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("plugins.json") && n != "plugins.json")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn bar_check_warns_about_a_bad_web_object() {
    let r = root();
    let bin = r.join("plugins");
    std::fs::create_dir_all(&bin).unwrap();
    let p = bin.join("bar-demo");
    std::fs::write(
        &p,
        "#!/bin/sh\necho '{\"name\":\"demo\",\"version\":\"1\",\"capabilities\":[\"placeholder\"],\"web\":{\"samples\":[{\"output\":{}}]}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let report = hop_bar::check::check_text("set left \"#{demo}\"", None, &[bin]);
    let msgs: Vec<&str> = report.issues.iter().map(|i| i.message.as_str()).collect();
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("plugin `demo`: web: samples[0]: `output`")),
        "{msgs:?}"
    );
    assert!(!report.has_errors());
}
