mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use std::process::Command as Std;
const MODEL_CONF: &str = "set left \"#{model}#{effort}\"";

#[test]
fn model_strips_claude_prefix() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Claude Opus 5.5"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus 5.5");
}

#[test]
fn effort_brackets_level() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Opus 5.5"},"effort":{"level":"medium"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus 5.5[medium]");
}

#[test]
fn effort_falls_back_to_settings_per_model_then_global() {
    let t = Tmp::new();
    t.write(
        ".claude/settings.json",
        r#"{"effortLevel":"high","modelSettings":{"m-a":{"effortLevel":"low"}}}"#,
    );
    let go = |id: &str| {
        run(&t)
            .conf(MODEL_CONF)
            .stdin(&format!(
                r#"{{"model":{{"id":"{id}","display_name":"M"}}}}"#
            ))
            .go()
            .plain
    };
    assert_eq!(go("m-a").trim(), "M[low]");
    assert_eq!(go("m-b").trim(), "M[high]");
}

#[test]
fn model_and_effort_link_separately() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Opus"},"effort":{"level":"high"}}"#)
        .go();
    // two independent styled spans, so a later task can link each on its own
    assert!(
        o.raw
            .contains("\x1b[38;5;250mOpus\x1b[39m\x1b[38;5;250m[high]\x1b[39m"),
        "{:?}",
        o.raw
    );
}

#[test]
fn ctx_on_reversed_ramp() {
    let t = Tmp::new();
    for (pct, color) in [
        (0, 46),
        (9, 46),
        (10, 82),
        (50, 226),
        (90, 202),
        (91, 196),
        (100, 196),
    ] {
        let o = run(&t)
            .conf("set left \"#{ctx}\"")
            .stdin(&format!(
                r#"{{"context_window":{{"used_percentage":{pct}}}}}"#
            ))
            .go();
        assert_eq!(o.plain.trim(), format!("ctx {pct}%"));
        assert!(
            o.raw.contains(&format!("\x1b[38;5;{color}m{pct}%")),
            "{pct}: {:?}",
            o.raw
        );
    }
}

#[test]
fn ctx_missing_omitted() {
    let t = Tmp::new();
    let o = run(&t).conf("set left \"#{ctx}\"").go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn segment_with_only_empty_placeholders_drops_separator() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{model} │ #{ctx} │ #{effort}\"")
        .stdin(r#"{"model":{"display_name":"Opus"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus");
}

#[test]
fn unknown_placeholder_omitted() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{model} │ #{nonesuch} │ #{prs}\"")
        .stdin(r#"{"model":{"display_name":"Opus"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus");
}

#[test]
fn invalid_config_prints_short_error_and_exits_zero() {
    let t = Tmp::new();
    let o = run(&t).conf("bogus directive\n").go();
    assert_eq!(o.raw, "bar: config error, run bar check\n");
}

#[test]
fn missing_config_uses_builtin_default() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("me@example.org"));
    let d = repo(&t, "proj");
    let o = run(&t)
        .cwd(&d)
        .stdin(
            &serde_json::json!({
                "workspace": {"current_dir": d},
                "model": {"display_name": "Claude Opus"},
                "context_window": {"used_percentage": 38}
            })
            .to_string(),
        )
        .go();
    assert!(o.plain.contains("Opus"), "{:?}", o.plain);
    assert!(o.plain.contains("ctx 38%"));
    assert!(o.plain.contains("proj:main"));
    assert!(o.plain.trim_end().ends_with("me@example.org"));
    assert_eq!(o.plain.trim().lines().count(), 1);
}

#[test]
fn ctx_huge_or_negative_used_never_panics() {
    let t = Tmp::new();
    for (used, color, shown) in [
        ("1e300", 196, "ctx"),
        ("-50", 46, "ctx"),
        ("-1e300", 46, "ctx"),
    ] {
        let o = run(&t)
            .conf("set left \"#{ctx}\"")
            .stdin(&format!(
                r#"{{"context_window":{{"used_percentage":{used}}}}}"#
            ))
            .go();
        assert!(o.plain.contains(shown), "{used}: {:?}", o.plain);
        assert!(
            o.raw.contains(&format!("\x1b[38;5;{color}m")),
            "{used}: {:?}",
            o.raw
        );
    }
}

#[test]
fn closed_stdout_exits_zero() {
    use std::io::Write;
    use std::process::Stdio;
    let t = Tmp::new();
    let h = t.path();
    let (iso, cwd) = common::isolation(h);
    let mut child = Std::new(assert_cmd::cargo::cargo_bin("bar"))
        .envs(iso)
        .current_dir(cwd)
        .arg("render")
        .env("COLUMNS", "200")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // read end gone before bar writes: the write hits a broken pipe
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(br#"{"model":{"display_name":"Opus"}}"#);
    drop(stdin);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn default_layout_in_github_repo_stays_in_temp_state() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(
        &d,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
    let out = run(&t).cwd(&d).go();
    assert!(out.plain.contains("r:main"), "{}", out.plain);
    // the cold prs/issues lists spawn detached refreshes; they must hit the
    // fake gh (once the remote URL is known: the harness's git environment
    // sends it to a background `git remote get-url` first)
    let log = common::gh_log(t.path());
    let start = std::time::Instant::now();
    while std::fs::read_to_string(&log).map_or(true, |l| !l.contains("pr list")) {
        assert!(start.elapsed().as_secs() < 10, "fake gh never called");
        std::thread::sleep(std::time::Duration::from_millis(100));
        run(&t).cwd(&d).go();
    }
    // the fake gh the process resolved first is the one that logged the call
    let first = std::env::split_paths(&out.path).next().unwrap();
    assert_eq!(first, t.path().join("fake-bin"));
    #[cfg(unix)]
    assert!(first.join("gh").is_file());
    #[cfg(windows)]
    assert!(first.join("gh.cmd").is_file());
    // everything the run created lives in what the harness and test made:
    // state, fake-bin, neutral cwd, the repo, the fake gh log
    let allowed = ["state", "fake-bin", "neutral-cwd", "proj", "fake-gh.log"];
    let stray: Vec<String> = std::fs::read_dir(t.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !allowed.contains(&n.as_str()))
        .collect();
    assert!(stray.is_empty(), "stray entries under temp HOME: {stray:?}");
    assert!(t.path().join("state/cache").is_dir());
}
