// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::cache::Cache;
use hop_bar::plugin::{parse_output, validate};
use hop_bar::render::{render_in, ColdRunner};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};
// --- options ---

#[test]
fn options_validated_against_schema() {
    let i = Fx::new().ext_info("echo");
    let ok = validate(
        &i.options,
        &opts(&[
            ("limit", "7"),
            ("mode", "b"),
            ("verbose", "true"),
            ("label", "x y"),
        ]),
    )
    .unwrap();
    assert_eq!(
        Value::Object(ok),
        json!({"limit": 7, "mode": "b", "verbose": true, "label": "x y"})
    );
    let defaults = validate(&i.options, &opts(&[])).unwrap();
    assert_eq!(Value::Object(defaults), json!({"limit": 3}));
    for bad in [
        ("limit", "seven"),
        ("mode", "c"),
        ("verbose", "maybe"),
        ("nope", "1"),
    ] {
        assert!(validate(&i.options, &opts(&[bad])).is_err(), "{bad:?}");
    }
    // invalid options omit the segment
    let fx = Fx::new();
    let c = cfg("");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[("mode", "zzz")]), "");
}

#[test]
fn render_gets_options_on_stdin() {
    let fx = Fx::new();
    let c = cfg("plugin echo org=acme keychain=entry-name\n");
    fx.prime("echo", &c, &[("mode", "a"), ("verbose", "no")]);
    let input: Value = serde_json::from_str(&fx.file("stdin.json")).unwrap();
    assert_eq!(
        input["options"],
        json!({"limit": 3, "mode": "a", "verbose": false})
    );
    assert_eq!(
        input["config"],
        json!({"org": "acme", "keychain": "entry-name"})
    );
    assert_eq!(input["columns"], json!(120));
    assert_eq!(input["session"]["cwd"], json!("/tmp/work"));
    assert_eq!(input["session"]["model"], json!("Opus"));
    assert_eq!(
        input["session"]["five_hour"]["resets_at"],
        json!(NOW + 3600)
    );
    let mut keys: Vec<&String> = input.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["columns", "config", "options", "session", "toggles"]);
    assert!(!fx.file("stdin.json").contains("PATH"));
    assert_eq!(
        fx.show("echo", &c, &[("mode", "a"), ("verbose", "no")]),
        "ok"
    );
}

// --- failures ---

#[test]
fn malformed_output_omits_segment() {
    let fx = Fx::new();
    let c = cfg("");
    fx.put("out.json", "not json at all");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "");
    // a later malformed run keeps the last good value
    fx.put("out.json", r#"{"text":"good"}"#);
    fx.prime("echo", &c, &[]);
    fx.put("out.json", r#"{"txt":"bad"}"#);
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "good");
    assert!(parse_output(r#"{"text":5}"#).is_none());
    assert!(parse_output("[]").is_none());
}

#[test]
fn nonzero_exit_omits_segment() {
    let fx = Fx::new();
    let c = cfg("");
    fx.put("exit", "3");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "");
}

#[test]
fn render_timeout_kills_plugin() {
    let fx = Fx::new();
    let found = fx.found("slow");
    let job = hop_bar::cache::Job::PluginRender {
        path: found.path.display().to_string(),
        input: "{}".into(),
        timeout: 3,
        accounts: false,
        email: None,
    };
    let t = Instant::now();
    assert!(job.execute(Duration::from_secs(20)).is_none());
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(2900), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    let pid = fx.file("pid").trim().to_string();
    assert!(!pid.is_empty());
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "plugin {pid} still running");
}

#[test]
fn slow_plugin_never_blocks_render() {
    let fx = Fx::new();
    fx.prime_info("slow");
    let cache = Cache::with_exe(&fx.state, PathBuf::from(env!("CARGO_BIN_EXE_bar")));
    let (s, c, dirs) = (
        session(),
        cfg("set left \"#{slow} #{slowinfo}\"\n"),
        fx.dirs(),
    );
    let t = Instant::now();
    let out = render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    let took = t.elapsed();
    assert_eq!(out, "");
    assert!(took < Duration::from_millis(1000), "{took:?}");
}

/// The options `echo` got from a full render of `conf`: the layout parsed,
/// `default` lines merged, values expanded, the job run in the background.
fn rendered_options(conf: &str) -> Value {
    let fx = Fx::new();
    fx.prime_info("echo");
    let cache = Cache::with_exe(&fx.state, PathBuf::from(env!("CARGO_BIN_EXE_bar")));
    let (s, c, dirs) = (session(), cfg(conf), fx.dirs());
    render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    let t = Instant::now();
    while !fx.bin.join("stdin.json").exists() {
        assert!(t.elapsed() < Duration::from_secs(30), "no render ran");
        std::thread::sleep(Duration::from_millis(50));
    }
    // the file is written as the plugin reads stdin: wait for a whole one
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(&fx.file("stdin.json")) {
            return v["options"].clone();
        }
        assert!(t.elapsed() < Duration::from_secs(30), "stdin.json incomplete");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn brace_templates_reach_the_plugin_untouched() {
    // `{name}` is not an expansion (`%cmd%` and `%$VAR%` are): inline and
    // from a `default` line alike
    let tpl = "{slug} - {done}/{total} {{x}} }";
    let o = rendered_options(&format!(
        "set left \"#{{echo[label={{slug}} - {{done}}/{{total}}]}}\"\n"
    ));
    assert_eq!(o["label"], "{slug} - {done}/{total}");
    let o = rendered_options(&format!("default echo \"label={tpl}\"\nset left \"#{{echo}}\"\n"));
    assert_eq!(o["label"], tpl);
}
