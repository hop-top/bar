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

// --- items ---

use hop_bar::render::{render_in, ColdRunner};
use hop_bar::style::{strip_codes, ITEM_BREAK};

fn items_json(n: usize) -> String {
    let items: Vec<_> = (0..n)
        .map(|i| json!({"label": format!("a{i}"), "text": format!("{}%", 10 + i), "style": "good"}))
        .collect();
    json!({"items": items}).to_string()
}

#[test]
fn parse_items_keeps_valid_ones_and_caps_at_16() {
    let o = plugin::parse_output(&items_json(20)).unwrap();
    assert_eq!(o.items.len(), 16);
    assert_eq!(o.items[0].text, "10%");
    let raw = r#"{"items":[{"text":"a"},{"nope":1},"x",{"text":5},{"text":"b","items":[{"text":"deep"}]}]}"#;
    let o = plugin::parse_output(raw).unwrap();
    let texts: Vec<&str> = o.items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(texts, ["a", "b"]);
    assert!(o.items[1].items.is_empty(), "items do not nest");
    assert_eq!(o.text, "");
    assert!(plugin::parse_output(r#"{"items":[]}"#).is_some());
    assert!(plugin::parse_output(r#"{"items":"x"}"#).is_none());
    assert!(plugin::parse_output(r#"{"label":"x"}"#).is_none());
    assert!(plugin::parse_output(r#"{"text":"t","items":[{"text":"i"}]}"#).is_some());
}

#[test]
fn items_are_painted_individually() {
    let fx = Fx::new();
    let c = cfg("");
    fx.put(
        "out.json",
        r#"{"items":[{"label":"x","text":"1","style":"good"},{"text":"2","style":"bad"},{"text":"","style":"bad"},{"text":"3","score":0}]}"#,
    );
    fx.prime("echo", &c, &[]);
    let shown = fx.show("echo", &c, &[]);
    let good = hop_bar::style::fg(c.colors.good, "1");
    let bad = hop_bar::style::fg(c.colors.bad, "2");
    assert!(shown.contains(&good), "{shown:?}");
    assert!(shown.contains(&bad), "{shown:?}");
    let plain = strip_codes(&shown);
    assert_eq!(
        plain,
        format!("x 1{ITEM_BREAK} 2{ITEM_BREAK} 3"),
        "empty item skipped"
    );
}

#[test]
fn items_join_by_one_space_and_wrap_only_between_items() {
    let fx = Fx::new();
    let c = cfg("set wrap on\nset left \"#{echo}\"\nset right \"#{echo}\"\n");
    fx.put(
        "out.json",
        r#"{"items":[{"text":"aaaa 11%"},{"text":"bbbb 22%"},{"text":"cccc 33%"}]}"#,
    );
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let draw = |cols| render_in(&s, &c, Some(cols), &ColdRunner, Some(&cache), &dirs);
    // wide: one row, a single space between items, no marker left
    let wide = draw(200);
    assert!(!wide.contains(ITEM_BREAK));
    let plain = strip_codes(&wide);
    assert_eq!(plain.lines().count(), 1, "{plain:?}");
    assert!(plain.contains("aaaa 11% bbbb 22% cccc 33%"), "{plain:?}");
    // narrow: rows break between items, never inside one
    let rows = strip_codes(&draw(24));
    assert_eq!(
        rows.lines().collect::<Vec<_>>(),
        [
            "aaaa 11% bbbb 22%",
            "cccc 33% │ aaaa 11%",
            "bbbb 22% cccc 33%"
        ],
        "{rows:?}"
    );
}

#[test]
fn wrap_off_keeps_items_on_the_rows_as_before() {
    let fx = Fx::new();
    let c = cfg("set left \"#{echo}\"\n");
    fx.put("out.json", r#"{"items":[{"text":"aaaa"},{"text":"bbbb"}]}"#);
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let out = render_in(&s, &c, Some(8), &ColdRunner, Some(&cache), &dirs);
    assert_eq!(out, "aaaa bbbb");
}

#[test]
fn items_survive_a_gradient() {
    let fx = Fx::new();
    let c = cfg("set wrap on\nset gradient ember\nset left \"#{echo}\"\n");
    fx.put(
        "out.json",
        r#"{"items":[{"text":"aaaa 11%"},{"text":"bbbb 22%"}]}"#,
    );
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let out = render_in(&s, &c, Some(14), &ColdRunner, Some(&cache), &dirs);
    let plain = strip_codes(&out);
    assert!(!plain.contains(ITEM_BREAK));
    assert_eq!(
        plain.lines().collect::<Vec<_>>(),
        ["aaaa 11%", "bbbb 22%"],
        "{plain:?}"
    );
}

// --- usage source ---

use hop_bar::placeholders::usage;
use hop_bar::session::{Session, Window};

fn plugin_usage_json(five: f64, seven: f64) -> String {
    json!({"text": "", "usage": {
        "five_hour": {"used_percentage": five, "resets_at": NOW + 3600},
        "seven_day": {"used_percentage": seven, "resets_at": NOW + 3 * DAY},
    }})
    .to_string()
}

fn no_limits() -> Session {
    Session {
        five_hour: None,
        seven_day: None,
        ..session()
    }
}

fn usage_text(fx: &Fx, s: &Session, c: &hop_bar::conf::Config, o: &[(&str, &str)]) -> String {
    let (cache, dirs) = (fx.cache(), fx.dirs());
    let cx = ctx(s, c, &cache, &dirs);
    strip_codes(&usage::render_with(&cx, &opts(o), &fx.state).unwrap_or_default())
}

#[test]
fn usage_from_the_source_plugin_when_claude_sends_none() {
    let fx = Fx::new();
    let c = cfg("set usage-source echo\n");
    fx.put("out.json", &plugin_usage_json(41.0, 12.5));
    let s = no_limits();
    fx.prime_in(&s, "echo", &c, &[]);
    assert_eq!(usage_text(&fx, &s, &c, &[("show", "pct")]), "59%");
    assert_eq!(
        usage_text(&fx, &s, &c, &[("type", "7d"), ("show", "time")]),
        "3d"
    );
    assert_eq!(
        usage_text(&fx, &s, &c, &[("type", "7d"), ("show", "pct")]),
        "88%"
    );
}

#[test]
fn claude_rate_limits_win_per_window() {
    let fx = Fx::new();
    let c = cfg("set usage-source echo\n");
    fx.put("out.json", &plugin_usage_json(41.0, 12.5));
    let s = Session {
        five_hour: Some(Window {
            used_percentage: 70.0,
            resets_at: NOW + 3600,
        }),
        seven_day: None,
        ..session()
    };
    fx.prime_in(&s, "echo", &c, &[]);
    assert_eq!(
        usage_text(&fx, &s, &c, &[("show", "pct")]),
        "30%",
        "Claude's 5h"
    );
    assert_eq!(
        usage_text(&fx, &s, &c, &[("type", "7d"), ("show", "pct")]),
        "88%",
        "source's 7d"
    );
}

#[test]
fn no_source_or_cold_cache_renders_nothing_and_never_spawns() {
    let fx = Fx::new();
    fx.put("out.json", &plugin_usage_json(41.0, 12.5));
    let s = no_limits();
    let plain = cfg("");
    assert_eq!(usage_text(&fx, &s, &plain, &[("show", "pct")]), "");
    let c = cfg("set usage-source echo\n");
    // nothing primed: cold, and the cache here cannot start a refresh
    assert_eq!(usage_text(&fx, &s, &c, &[("show", "pct")]), "");
    assert!(!fx.bin.join("stdin.json").exists(), "no synchronous run");
    let missing = cfg("set usage-source nosuch\n");
    assert_eq!(usage_text(&fx, &s, &missing, &[("show", "pct")]), "");
}

#[test]
fn source_reads_the_default_options_result() {
    let fx = Fx::new();
    let c = cfg("set usage-source echo\ndefault echo mode=a\n");
    fx.put("out.json", &plugin_usage_json(41.0, 12.5));
    let s = no_limits();
    // an entry for other options is not the one read
    fx.prime_in(&s, "echo", &c, &[("mode", "b")]);
    assert_eq!(usage_text(&fx, &s, &c, &[("show", "pct")]), "");
    fx.prime_in(&s, "echo", &c, &[("mode", "a")]);
    assert_eq!(usage_text(&fx, &s, &c, &[("show", "pct")]), "59%");
}

#[test]
fn plugin_windows_are_validated() {
    let o = plugin::parse_output(
        r#"{"text":"","usage":{"five_hour":{"used_percentage":150,"resets_at":5},
            "seven_day":{"used_percentage":"x","resets_at":5}}}"#,
    )
    .unwrap();
    let u = o.usage.unwrap();
    assert_eq!(u.five_hour.unwrap().used_percentage, 100.0, "clamped");
    assert!(u.seven_day.is_none());
    let none =
        plugin::parse_output(r#"{"text":"","usage":{"five_hour":{"resets_at":5}}}"#).unwrap();
    assert!(none.usage.is_none());
    let neg = plugin::parse_output(
        r#"{"text":"","usage":{"five_hour":{"used_percentage":-4,"resets_at":5.4}}}"#,
    )
    .unwrap();
    let w = neg.usage.unwrap().five_hour.unwrap();
    assert_eq!((w.used_percentage, w.resets_at), (0.0, 5));
}

#[test]
fn source_history_is_kept_apart() {
    let fx = Fx::new();
    let c = cfg("set usage-source echo\n");
    fx.put("out.json", &plugin_usage_json(41.0, 12.5));
    let s = no_limits();
    fx.prime_in(&s, "echo", &c, &[]);
    usage_text(&fx, &s, &c, &[("show", "spark")]);
    assert!(fx.state.join("hist-5h@echo").exists());
    assert!(
        !fx.state.join("hist-5h").exists(),
        "not mixed with Claude's"
    );
    let own = session();
    usage_text(&fx, &own, &c, &[("show", "spark")]);
    assert!(fx.state.join("hist-5h").exists());
    assert_eq!(hop_bar::history::step("7d@echo"), 6 * 3600);
}

#[test]
fn source_windows_feed_pace_and_projection() {
    let fx = Fx::new();
    let c = cfg("set usage-source echo\n");
    // 90% used with 4 of 5 hours to go: far ahead of pace
    fx.put(
        "out.json",
        &json!({"text":"","usage":{"five_hour":{"used_percentage":90.0,"resets_at":NOW + 4 * 3600}}}).to_string(),
    );
    let s = no_limits();
    fx.prime_in(&s, "echo", &c, &[]);
    let text = usage_text(&fx, &s, &c, &[("show", "pct,proj")]);
    assert!(text.starts_with("10%") && text.contains("p"), "{text:?}");
}

#[test]
fn usage_source_is_user_config_only() {
    for mode in ["safe", "full"] {
        let user = format!("set project-config {mode}\n");
        let l = hop_bar::conf::layers(Some(&user), |_| Some(Ok("set usage-source evil\n".into())));
        let cfg = l.config.unwrap();
        assert_eq!(cfg.usage_source, None, "{mode}");
        assert_eq!(l.dropped.len(), 1, "{mode}");
        assert!(
            l.dropped[0].message.contains("user config"),
            "{:?}",
            l.dropped
        );
    }
    let c = cfg("set usage-source cliproxyapi\n");
    assert_eq!(c.usage_source.as_deref(), Some("cliproxyapi"));
    assert_eq!(
        cfg("set usage-source cliproxyapi\nset usage-source off\n").usage_source,
        None
    );
    for bad in ["a b", "a/b", "", "x;y"] {
        assert!(
            hop_bar::conf::parse(&format!("set usage-source \"{bad}\"\n")).is_err(),
            "{bad}"
        );
    }
}

// --- account source ---

use hop_bar::placeholders::lookup;

fn account_text(fx: &Fx, c: &hop_bar::conf::Config) -> String {
    let (cache, dirs, s) = (fx.cache(), fx.dirs(), session());
    let cx = ctx(&s, c, &cache, &dirs);
    let out = lookup("account").unwrap().render(&cx, &opts(&[]));
    strip_codes(&out.unwrap_or_default())
}

#[test]
fn account_comes_from_the_source_plugin() {
    let fx = Fx::new();
    let claude = fx.root.join("claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(
        claude.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"local@example.test"}}"#,
    )
    .unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
    let src = "set account-source echo\naccount \"work-*\" Work 33\n";
    let c = cfg(src);
    // cold: today's behaviour
    assert_eq!(account_text(&fx, &c), "local@example.test");
    fx.put(
        "out.json",
        r#"{"text":"","account":{"id":"a1b2","label":"work-alice"}}"#,
    );
    fx.prime("echo", &c, &[]);
    // the plugin's label, run through the account rules (label and colour)
    assert_eq!(account_text(&fx, &c), "Work");
    let raw = lookup("account")
        .unwrap()
        .render(&ctx(&session(), &c, &fx.cache(), &fx.dirs()), &opts(&[]))
        .unwrap();
    assert!(raw.contains(&hop_bar::style::fg(33u8, "Work")), "{raw:?}");
    // without a matching rule the label shows as is
    let plain = cfg("set account-source echo\n");
    assert_eq!(account_text(&fx, &plain), "work-alice");
    // no source configured: signed-in account
    assert_eq!(account_text(&fx, &cfg("")), "local@example.test");
    // output without an account: signed-in account
    fx.put("out.json", r#"{"text":"x"}"#);
    fx.prime("echo", &c, &[]);
    assert_eq!(account_text(&fx, &c), "local@example.test");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
}

#[test]
fn account_output_is_validated() {
    let ok =
        plugin::parse_output(r#"{"text":"","account":{"id":" a1 ","label":"x\u001by\u0007"}}"#)
            .unwrap();
    let a = ok.account.unwrap();
    assert_eq!((a.id.as_str(), a.label.as_str()), ("a1", "xy"));
    for bad in [
        r#"{"id":"a"}"#,
        r#"{"label":"b"}"#,
        r#"{"id":"","label":"b"}"#,
        r#"{"id":"a","label":" \u001b "}"#,
        r#"{"id":1,"label":"b"}"#,
        r#""x""#,
    ] {
        let raw = format!(r#"{{"text":"","account":{bad}}}"#);
        assert!(
            plugin::parse_output(&raw).unwrap().account.is_none(),
            "{bad}"
        );
    }
}

#[test]
fn account_source_is_user_config_only() {
    for mode in ["safe", "full"] {
        let user = format!("set project-config {mode}\n");
        let l = hop_bar::conf::layers(Some(&user), |_| {
            Some(Ok("set account-source evil\n".into()))
        });
        assert_eq!(l.config.unwrap().account_source, None, "{mode}");
        assert!(
            l.dropped[0].message.contains("user config"),
            "{:?}",
            l.dropped
        );
    }
    assert_eq!(
        cfg("set account-source x\nset account-source off\n").account_source,
        None
    );
    assert!(hop_bar::conf::parse("set account-source \"a b\"\n").is_err());
}
