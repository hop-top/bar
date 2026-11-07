mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::conf::Config;
use hop_bar::pace::{metric_score, score};
use hop_bar::style::{fg, ramp};
use serde_json::{json, Value};
// --- output styling ---

#[test]
fn plugin_escape_codes_stripped() {
    let c = Config::default();
    let out = paint(
        r#"{"text":"\u001b[31mred\u001b[0m \u202eevil\u0007","link":"https://x.test/\u001b]8;;"}"#,
        "p",
        &c,
    );
    assert!(!out.contains('\x1b'), "{out:?}");
    assert!(!out.contains('\u{202e}') && !out.contains('\x07'));
    assert!(out.contains("red") && out.contains("evil"));
    let linked = paint(r#"{"text":"t","link":"https://x.test/a"}"#, "p", &c);
    assert_eq!(linked, "\x1b]8;;https://x.test/a\x1b\\t\x1b]8;;\x1b\\");
    assert_eq!(paint(r#"{"text":"\u001b\u0007"}"#, "p", &c), "");
}

#[test]
fn style_maps_to_palette() {
    let c = cfg("set color.good 10\nset color.warn 11\nset color.bad 12\n");
    assert_eq!(
        paint(r#"{"text":"a","style":"good"}"#, "p", &c),
        fg(10, "a")
    );
    assert_eq!(
        paint(r#"{"text":"a","style":"warn"}"#, "p", &c),
        fg(11, "a")
    );
    assert_eq!(paint(r#"{"text":"a","style":"bad"}"#, "p", &c), fg(12, "a"));
    assert_eq!(
        paint(r#"{"text":"a","style":"dim"}"#, "p", &c),
        hop_bar::style::dim("a")
    );
    assert_eq!(paint(r#"{"text":"a","style":"pink"}"#, "p", &c), "a");
}

#[test]
fn score_goes_on_ramp() {
    let c = Config::default();
    for s in [0, 37, 50, 100] {
        assert_eq!(
            paint(&format!(r#"{{"text":"a","score":{s}}}"#), "p", &c),
            fg(ramp(s, &c.ramp), "a")
        );
    }
}

#[test]
fn score_out_of_range_clamped() {
    let c = Config::default();
    let red = fg(c.ramp[0], "a");
    let green = fg(c.ramp[10], "a");
    assert_eq!(paint(r#"{"text":"a","score":-40}"#, "p", &c), red);
    assert_eq!(paint(r#"{"text":"a","score":250}"#, "p", &c), green);
    assert_eq!(paint(r#"{"text":"a","score":1e300}"#, "p", &c), green);
}

#[test]
fn style_beats_score() {
    let c = Config::default();
    assert_eq!(
        paint(r#"{"text":"a","style":"bad","score":100}"#, "p", &c),
        fg(c.colors.bad, "a")
    );
    let metric = format!(
        r#""metric":{{"remaining":0,"granted":100,"expires_at":{}}}"#,
        NOW + DAY
    );
    let c2 = cfg("rule p early pace final=3d bad=$50\n");
    assert_eq!(
        paint(&format!(r#"{{"text":"a","score":0,{metric}}}"#), "p", &c2),
        fg(c2.ramp[0], "a"),
        "score beats metric"
    );
}

fn credits_rule() -> hop_bar::conf::Rule {
    cfg("rule claude-credits early pace final=3d bad=$50\n").rules[0].clone()
}

#[test]
fn rule_applies_to_metric() {
    let rule = credits_rule();
    for (cents, want) in [(0, 100), (2500, 51), (5000, 1), (5001, 0)] {
        assert_eq!(
            metric_score(cents, 20_000, Some(2 * DAY), 0, &rule),
            want,
            "{cents}"
        );
    }
    // through paint: rule target equals the placeholder name
    let c = cfg("rule claude-credits early pace final=3d bad=$50\n");
    let out = format!(
        r#"{{"text":"$25","metric":{{"remaining":2500,"granted":20000,"expires_at":{}}}}}"#,
        NOW + 2 * DAY
    );
    assert_eq!(
        paint(&out, "claude-credits", &c),
        fg(ramp(51, &c.ramp), "$25")
    );
    assert_eq!(paint(&out, "other", &c), "$25", "rule for another name");
}

#[test]
fn rule_pace_against_grant() {
    let rule = credits_rule();
    let (secs, period) = (20 * DAY, 30 * DAY);
    for (cents, pct) in [(10_000, 100), (5_000, 50), (2_000, 20), (1_000, 10)] {
        assert_eq!(
            metric_score(cents, 10_000, Some(secs), period, &rule),
            score(pct, Some(secs), period, &rule),
            "{cents}"
        );
    }
    assert_eq!(metric_score(5_000, 10_000, Some(secs), period, &rule), 75);
    assert_eq!(metric_score(1_000, 10_000, Some(secs), period, &rule), 0);
    // dollar thresholds are read as shares of the grant
    let r = cfg("rule c early warn=$20 bad=$5 final=1d bad=$10\n").rules[0].clone();
    let pct = cfg("rule c early warn=20% bad=5% final=1d bad=30%\n").rules[0].clone();
    assert_eq!(
        metric_score(1_500, 10_000, None, 0, &r),
        score(15, None, 0, &pct)
    );
    // through paint, with granted_at giving the period
    let c = cfg("rule claude-credits early pace final=3d bad=$50\n");
    let out = format!(
        r#"{{"text":"x","metric":{{"remaining":5000,"granted":10000,"expires_at":{},"granted_at":{}}}}}"#,
        NOW + secs,
        NOW + secs - period
    );
    assert_eq!(
        paint(&out, "claude-credits", &c),
        fg(ramp(75, &c.ramp), "x")
    );
}

// --- CLI ---

fn cli(fx: &Fx) -> assert_cmd::Command {
    let (iso, cwd) = common::isolation(&fx.root);
    let mut c = assert_cmd::Command::cargo_bin("bar").unwrap();
    c.envs(iso)
        .current_dir(cwd)
        .env("PATH", format!("{}:/usr/bin:/bin", fx.bin.display()));
    c
}

#[test]
fn plugin_list_and_info_cli() {
    let fx = Fx::new();
    std::fs::remove_file(fx.bin.join("bar-slowinfo")).unwrap();
    let out = cli(&fx)
        .args(["plugin", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    let names: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["echo", "plain", "slow"]);
    let out = cli(&fx)
        .args(["plugin", "info", "echo", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["version"], json!("1.0.0"));
    assert_eq!(v["ttl"], json!(60));
    assert_eq!(v["options"]["mode"]["enum"], json!(["a", "b"]));
    cli(&fx)
        .args(["plugin", "info", "account"])
        .assert()
        .failure();
}

#[test]
fn extreme_metric_times_never_panic() {
    let c = cfg("rule claude-credits early pace final=3d bad=$50\n");
    let ints = [i64::MIN, -1, 0, 1, i64::MAX];
    for exp in ints {
        for granted_at in ints {
            let out = format!(
                r#"{{"text":"$25","metric":{{"remaining":2500,"granted":20000,"expires_at":{exp},"granted_at":{granted_at}}}}}"#
            );
            let _ = paint(&out, "claude-credits", &c);
        }
    }
}

#[test]
fn label_outside_link_and_dim() {
    let c = Config::default();
    let out = paint(
        r#"{"text":"3","label":"@\u001b\u0007","link":"https://x.test/a"}"#,
        "p",
        &c,
    );
    assert_eq!(
        out,
        format!(
            "{} \x1b]8;;https://x.test/a\x1b\\3\x1b]8;;\x1b\\",
            hop_bar::style::dim("@")
        )
    );
    assert_eq!(
        paint(r#"{"text":"3","label":""}"#, "p", &c),
        paint(r#"{"text":"3"}"#, "p", &c)
    );
    assert_eq!(paint(r#"{"text":"","label":"@"}"#, "p", &c), "");
}
