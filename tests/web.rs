//! The pure entry points behind the web configurator.

use hop_bar::web;
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000;

fn session(extra: Value) -> String {
    let mut v = json!({
        "model": {"id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6"},
        "context_window": {"used_percentage": 42.0},
        "rate_limits": {
            "five_hour": {"used_percentage": 20.0, "resets_at": NOW + 2 * 3600},
            "seven_day": {"used_percentage": 10.0, "resets_at": NOW + 3 * 86_400},
        },
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v.to_string()
}

fn plain(s: &str) -> String {
    hop_bar::style::strip_codes(s)
}

#[test]
fn render_measures_time_from_the_given_now() {
    let conf = "set left \"#{usage[type=5h,view=time,show=time]}\"";
    let out = plain(&web::render(conf, &session(json!({})), 80, NOW, ""));
    assert!(out.contains("2h0m"), "{out:?}");
    let later = plain(&web::render(conf, &session(json!({})), 80, NOW + 3600, ""));
    assert!(later.contains("1h0m"), "{later:?}");
}

#[test]
fn render_reads_session_fields_from_the_json() {
    let conf = "set left \"#{model} #{ctx}\"";
    let out = plain(&web::render(conf, &session(json!({})), 80, NOW, ""));
    assert!(out.contains("Sonnet 4.6"), "{out:?}");
    assert!(out.contains("42%"), "{out:?}");
}

#[test]
fn render_answers_repo_data_with_samples() {
    let conf = "set left \"#{repo}:#{branch}\"";
    let out = plain(&web::render(conf, &session(json!({})), 80, NOW, ""));
    assert_eq!(out.trim(), "bar:main");
}

#[test]
fn render_is_the_config_error_line_for_a_bad_config() {
    let out = web::render("set skin bogus", &session(json!({})), 80, NOW, "");
    assert_eq!(out, hop_bar::render::CONFIG_ERROR);
}

#[test]
fn render_tolerates_malformed_session_json() {
    let out = plain(&web::render(
        "set left \"#{repo} #{model}\"",
        "{not json",
        80,
        NOW,
        "",
    ));
    assert_eq!(out.trim(), "bar");
}

#[test]
fn render_places_groups_in_the_given_columns() {
    let conf = "set left L\nset right R";
    let width = |cols| {
        plain(&web::render(conf, "{}", cols, NOW, ""))
            .chars()
            .count()
    };
    assert_eq!(width(50) - width(30), 20);
}

#[test]
fn inspect_reports_lines_and_settings() {
    let conf =
        "set skin powerline\nset palette nord\nset left \"#{model}\"\nset right@2 \"#{account}\"";
    let v = web::inspect(conf);
    assert_eq!(v["ok"], true);
    assert_eq!(v["lines"][0][0], "#{model}");
    assert_eq!(v["lines"][1][2], "#{account}");
    assert_eq!(v["set"]["skin"], "powerline");
    assert_eq!(v["set"]["palette"], "nord");
    assert_eq!(v["set"]["icons"], "none");
}

#[test]
fn inspect_lists_parse_errors() {
    let v = web::inspect("set skin bogus");
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["line"], 1);
}

#[test]
fn tokens_split_text_and_placeholders() {
    let v = web::tokens("#{usage[type=5h,show=bar,pct]} │ x");
    assert_eq!(v["ok"], true);
    assert_eq!(v["tokens"][0]["name"], "usage");
    assert_eq!(
        v["tokens"][0]["options"],
        json!([["type", "5h"], ["show", "bar,pct"]])
    );
    assert_eq!(v["tokens"][1]["text"], " │ x");
    assert_eq!(web::tokens("#{oops")["ok"], false);
}

#[test]
fn schema_lists_every_core_placeholder() {
    let s = web::schema();
    let names: Vec<&str> = s["placeholders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    let core: Vec<&str> = hop_bar::placeholders::names().collect();
    assert_eq!(names, core);
    assert_eq!(s["presets"], json!(hop_bar::presets::NAMES));
}

/// Every value the page offers is one bar accepts.
#[test]
fn schema_values_pass_check() {
    let s = web::schema();
    for set in s["settings"].as_array().unwrap() {
        for v in set["values"].as_array().unwrap() {
            let conf = format!(
                "set {} {}",
                set["key"].as_str().unwrap(),
                v.as_str().unwrap()
            );
            assert_eq!(web::inspect(&conf)["ok"], true, "{conf}");
        }
    }
    for p in s["placeholders"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap();
        for o in p["options"].as_array().unwrap() {
            let key = o["key"].as_str().unwrap();
            let sample = match o["kind"].as_str().unwrap() {
                "one" | "list" => o["values"][0].as_str().unwrap().to_string(),
                "int" | "pos" => "3".into(),
                _ => "x".into(),
            };
            let conf = format!("set left \"#{{{name}[{key}={sample}]}}\"");
            let errs: Vec<Value> = web::check(&conf, "")["issues"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| i["level"] == "error")
                .cloned()
                .collect();
            assert!(errs.is_empty(), "{conf}: {errs:?}");
        }
    }
}

#[test]
fn check_flags_unknown_options() {
    let v = web::check("set left \"#{usage[bogus=1]}\"", "");
    assert!(v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["level"] == "error"));
}

#[test]
fn preset_returns_the_shipped_text() {
    assert_eq!(web::preset("powerline"), hop_bar::presets::get("powerline"));
    assert_eq!(web::preset("nope"), None);
}

#[test]
fn check_leaves_out_path_warnings() {
    let v = web::check("set left \"#{prs}\"", "");
    assert_eq!(v, json!({"ok": true, "issues": []}));
}

#[test]
fn check_takes_unknown_names_for_plugins() {
    let v = web::check("set left \"#{myplugin}\"", "");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["issues"][0]["level"], "warning");
    assert!(v["issues"][0]["message"]
        .as_str()
        .unwrap()
        .contains("`myplugin`"));
}
