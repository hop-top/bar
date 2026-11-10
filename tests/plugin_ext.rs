// Generic protocol extensions: render limit, discovery beside bar, declared
// inputs, items, usage windows, account, actions.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::cache::Job;
use hop_bar::plugin::{self, cached_info, discover, parse_info, plugin_dirs_with};
use serde_json::json;
use std::time::{Duration, Instant};

// --- render timeout ---

fn timeout_of(extra: serde_json::Value) -> u64 {
    parse_info(&echo_info(extra)).unwrap().timeout
}

#[test]
fn timeout_defaults_and_clamps() {
    assert_eq!(timeout_of(json!({})), 3);
    assert_eq!(timeout_of(json!({"timeout": "15s"})), 15);
    assert_eq!(timeout_of(json!({"timeout": "8s"})), 8);
    assert_eq!(timeout_of(json!({"timeout": 6})), 6);
    assert_eq!(timeout_of(json!({"timeout": "1m"})), 15, "clamped to 15 s");
    assert_eq!(
        timeout_of(json!({"timeout": "0s"})),
        3,
        "zero is the default"
    );
    assert_eq!(timeout_of(json!({"timeout": "soon"})), 3);
    assert_eq!(timeout_of(json!({"timeout": []})), 3);
}

#[test]
fn plan_carries_the_declared_timeout() {
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(json!({"timeout": "12s"})));
    fx.prime_info("echo");
    let (cache, dirs, c) = (fx.cache(), fx.dirs(), cfg(""));
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let s = session();
    let p = plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap();
    match p.job {
        Job::PluginRender { timeout, .. } => assert_eq!(timeout, 12),
        other => panic!("{other:?}"),
    }
}

#[test]
fn job_timeout_bounds_the_run_and_survives_the_request_round_trip() {
    let fx = Fx::new();
    let job = Job::PluginRender {
        path: fx.found("slow").path.display().to_string(),
        input: "{}".into(),
        timeout: 1,
    };
    let req = hop_bar::cache::Request {
        key: "k".into(),
        job: job.clone(),
        lock: String::new(),
    };
    let back = hop_bar::cache::Request::from_json(&req.to_json()).unwrap();
    assert_eq!(back.job, job);
    let t = Instant::now();
    assert!(job.execute(Duration::from_secs(20)).is_none());
    assert!(
        t.elapsed() < Duration::from_millis(2500),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn job_without_timeout_field_keeps_three_seconds() {
    let raw =
        r#"{"key":"k","lock":"","job":{"kind":"plugin-render","path":"/x/bar-y","input":"{}"}}"#;
    match hop_bar::cache::Request::from_json(raw).unwrap().job {
        Job::PluginRender { timeout, .. } => assert_eq!(timeout, 3),
        other => panic!("{other:?}"),
    }
    let big = raw.replace(r#""input":"{}""#, r#""input":"{}","timeout":99"#);
    match hop_bar::cache::Request::from_json(&big).unwrap().job {
        Job::PluginRender { timeout, .. } => assert_eq!(timeout, 15),
        other => panic!("{other:?}"),
    }
}

// --- discovery beside the running executable ---

#[test]
fn exe_dir_is_scanned_after_path() {
    let fx = Fx::new();
    let exe_dir = fx.root.join("exe");
    std::fs::create_dir_all(&exe_dir).unwrap();
    install(&fx.bin.join("bar-echo"), &exe_dir.join("bar-echo"));
    install(&fx.bin.join("bar-echo"), &exe_dir.join("bar-beside"));
    let path = std::env::join_paths([&fx.bin]).unwrap();
    let dirs = plugin_dirs_with(Some(&path), Some(&exe_dir.join("bar")));
    assert_eq!(dirs, vec![fx.bin.clone(), exe_dir.clone()]);
    let all = discover(&dirs);
    assert!(all.iter().any(|f| f.name == "beside"), "found beside bar");
    let echo = all.iter().find(|f| f.name == "echo").unwrap();
    assert_eq!(echo.path, fx.bin.join("bar-echo"), "PATH copy wins");
    // core names still win beside bar too
    install(&fx.bin.join("bar-echo"), &exe_dir.join("bar-usage"));
    assert!(!discover(&dirs).iter().any(|f| f.name == "usage"));
}

#[test]
fn exe_dir_already_on_path_is_not_repeated_and_relative_is_ignored() {
    let fx = Fx::new();
    let path = std::env::join_paths([&fx.bin]).unwrap();
    let dirs = plugin_dirs_with(Some(&path), Some(&fx.bin.join("bar")));
    assert_eq!(dirs, vec![fx.bin.clone()]);
    assert_eq!(
        plugin_dirs_with(Some(&path), Some(std::path::Path::new("bar"))),
        vec![fx.bin.clone()]
    );
    assert_eq!(
        plugin_dirs_with(None, None),
        Vec::<std::path::PathBuf>::new()
    );
}

// --- declared inputs ---

fn plan_in(fx: &Fx, s: &hop_bar::session::Session) -> plugin::Plan {
    let (cache, dirs, c) = (fx.cache(), fx.dirs(), cfg(""));
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    plugin::plan(&ctx(s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap()
}

fn job_input(p: &plugin::Plan) -> serde_json::Value {
    match &p.job {
        Job::PluginRender { input, .. } => serde_json::from_str(input).unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn declared_env_splits_the_cache_and_is_sent() {
    let fx = Fx::new();
    fx.put(
        "ext.json",
        &echo_info(json!({"inputs": {"env": ["BAR_T_BASE_URL"]}})),
    );
    fx.prime_info("echo");
    let s = session();
    std::env::set_var("BAR_T_BASE_URL", "http://127.0.0.1:1");
    let a = plan_in(&fx, &s);
    std::env::set_var("BAR_T_BASE_URL", "http://127.0.0.1:2");
    let b = plan_in(&fx, &s);
    std::env::remove_var("BAR_T_BASE_URL");
    let none = plan_in(&fx, &s);
    assert_ne!(a.key, b.key);
    assert_ne!(a.key, none.key);
    assert_eq!(
        job_input(&a)["env"],
        json!({"BAR_T_BASE_URL": "http://127.0.0.1:1"})
    );
    assert_eq!(
        job_input(&b)["env"],
        json!({"BAR_T_BASE_URL": "http://127.0.0.1:2"})
    );
    assert_eq!(job_input(&none)["env"], json!({}), "unset is absent");
}

#[test]
fn declared_session_field_splits_the_cache() {
    let fx = Fx::new();
    fx.put(
        "ext.json",
        &echo_info(json!({"inputs": {"session": ["model_id", "context_used", "bogus"]}})),
    );
    fx.prime_info("echo");
    let a = hop_bar::session::Session {
        model_id: "m-a".into(),
        ..session()
    };
    let b = hop_bar::session::Session {
        model_id: "m-b".into(),
        ..session()
    };
    let busy = hop_bar::session::Session {
        context_used: Some(80.0),
        ..a.clone()
    };
    assert_ne!(plan_in(&fx, &a).key, plan_in(&fx, &b).key);
    assert_eq!(
        plan_in(&fx, &a).key,
        plan_in(&fx, &busy).key,
        "only stable fields can be declared"
    );
}

#[test]
fn undeclared_inputs_leave_key_and_input_alone() {
    let fx = Fx::new();
    fx.prime_info("echo");
    std::env::set_var("BAR_T_UNDECLARED", "x");
    let p = plan_in(&fx, &session());
    std::env::remove_var("BAR_T_UNDECLARED");
    assert!(job_input(&p).get("env").is_none());
    assert!(!p.key.contains("BAR_T_UNDECLARED"));
}

#[test]
fn inputs_are_bounded_and_secret_names_refused() {
    let names: Vec<String> = (0..12).map(|i| format!("BAR_T_V{i:02}")).collect();
    let mut env = names.clone();
    env.extend(
        [
            "ANTHROPIC_AUTH_TOKEN",
            "MY_API_KEY",
            "lower",
            "BAD-NAME",
            "",
        ]
        .map(String::from),
    );
    let info = parse_info(&echo_info(json!({"inputs": {"env": env}}))).unwrap();
    assert_eq!(info.inputs.env, names[..8], "at most 8, sorted");
    assert!(info
        .inputs
        .refused
        .contains(&"ANTHROPIC_AUTH_TOKEN".to_string()));
    assert!(info.inputs.refused.contains(&"MY_API_KEY".to_string()));
    let get = |n: &str| match n {
        "BAR_T_V00" => Some("x".repeat(1024)),
        "BAR_T_V01" => Some("x".repeat(1025)),
        "BAR_T_V02" => Some(String::new()),
        _ => None,
    };
    let vals = info.inputs.env_values(&get);
    assert_eq!(vals.len(), 2, "over 1 KiB counts as unset: {vals:?}");
    assert!(vals.contains_key("BAR_T_V00") && vals.contains_key("BAR_T_V02"));
}
