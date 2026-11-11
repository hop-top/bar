// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::conf::Config;
use hop_bar::plugin::{
    self, cached_info, discover, find, info, parse_info, validate, Found, DEFAULT_TTL,
};
use hop_bar::render::{render_in, ColdRunner};
use hop_bar::session::Session;
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};
// --- discovery ---

#[test]
fn discovers_prefixed_executables() {
    let fx = Fx::new();
    std::fs::write(fx.bin.join("unrelated"), "#!/bin/sh\n").unwrap();
    let names: Vec<String> = discover(&fx.dirs()).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["echo", "plain", "slow", "slowinfo"]);
    assert_eq!(fx.found("echo").path, fx.bin.join("bar-echo"));
}

#[test]
fn first_on_path_wins() {
    let fx = Fx::new();
    let second = fx.root.join("bin2");
    std::fs::create_dir_all(&second).unwrap();
    install(&fx.bin.join("bar-echo"), &second.join("bar-echo"));
    install(&fx.bin.join("bar-echo"), &second.join("bar-only2"));
    let dirs = vec![second.clone(), fx.bin.clone()];
    let all = discover(&dirs);
    let echo: Vec<&Found> = all.iter().filter(|f| f.name == "echo").collect();
    assert_eq!(echo.len(), 1);
    assert_eq!(echo[0].path, second.join("bar-echo"));
    assert_eq!(find(&dirs, "echo").unwrap().path, second.join("bar-echo"));
    assert!(all.iter().any(|f| f.name == "only2"));
}

#[test]
fn skips_non_executable() {
    let fx = Fx::new();
    let first = fx.root.join("first");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::copy(fx.bin.join("bar-echo"), first.join("bar-echo")).unwrap();
    std::fs::set_permissions(
        first.join("bar-echo"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    std::fs::write(first.join("bar-dud"), "#!/bin/sh\n").unwrap();
    let dirs = vec![first.clone(), fx.bin.clone()];
    let all = discover(&dirs);
    assert!(!all.iter().any(|f| f.name == "dud"));
    assert_eq!(find(&dirs, "echo").unwrap().path, fx.bin.join("bar-echo"));
    assert!(find(&dirs, "dud").is_none());
}

#[test]
fn core_name_wins_over_plugin() {
    let fx = Fx::new();
    assert!(fx.bin.join("bar-account").is_file());
    assert!(!discover(&fx.dirs()).iter().any(|f| f.name == "account"));
    assert!(find(&fx.dirs(), "account").is_none());
    for core in [
        "account", "repo", "branch", "model", "effort", "ctx", "usage", "prs", "issues",
    ] {
        install(
            &fx.bin.join("bar-echo"),
            &fx.bin.join(format!("bar-{core}")),
        );
        assert!(find(&fx.dirs(), core).is_none(), "{core}");
    }
}

#[test]
fn tray_companion_is_not_a_plugin() {
    let fx = Fx::new();
    install(&fx.bin.join("bar-echo"), &fx.bin.join("bar-tray"));
    assert!(!discover(&fx.dirs()).iter().any(|f| f.name == "tray"));
    assert!(find(&fx.dirs(), "tray").is_none());
}

#[test]
fn cache_key_includes_cwd() {
    let fx = Fx::new();
    let c = cfg("");
    let a = session();
    let b = Session {
        cwd: "/tmp/other-repo".into(),
        ..session()
    };
    fx.prime_info("echo");
    let (cache, dirs) = (fx.cache(), fx.dirs());
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let key = |s: &Session| {
        plugin::plan(&ctx(s, &c, &cache, &dirs), &found, &inf, &opts(&[]))
            .unwrap()
            .key
    };
    assert_ne!(key(&a), key(&b));
    let same_cwd = Session {
        session_id: "s-2".into(),
        context_used: Some(50.0),
        ..session()
    };
    assert_eq!(key(&a), key(&same_cwd), "only cwd splits the cache");
    fx.put("out.json", r#"{"text":"A"}"#);
    fx.prime_in(&a, "echo", &c, &[]);
    fx.put("out.json", r#"{"text":"B"}"#);
    fx.prime_in(&b, "echo", &c, &[]);
    assert_eq!(fx.show_in(&a, "echo", &c, &[]), "A");
    assert_eq!(fx.show_in(&b, "echo", &c, &[]), "B");
}

#[test]
fn wide_and_invisible_text_measured_through_compose() {
    let fx = Fx::new();
    let c = cfg("set left \"#{echo}\"\n");
    fx.put(
        "out.json",
        r#"{"text":"\u6f22\u5b57\ud83d\ude42\u200b\u2028\u2029x\u0085"}"#,
    );
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let out = render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    let line = out.trim_end();
    // the zero-width space is a format character: dropped
    assert_eq!(line, "\u{6f22}\u{5b57}\u{1f642}x");
    assert!(!line
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}')));
    assert_eq!(hop_bar::style::visible_width(line), 7);
    assert_eq!(out.lines().count(), 1);
}

#[test]
fn relative_path_dirs_ignored() {
    let dirs = plugin::path_dirs(Some(std::ffi::OsStr::new("/usr/bin:.:rel/bin::/bin")));
    assert_eq!(dirs, [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
}

// --- ext-info ---

#[test]
fn ext_info_timeout_5s_falls_back() {
    let fx = Fx::new();
    let t = Instant::now();
    let i = info(&fx.found("slowinfo"));
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(4900), "{took:?}");
    assert!(took < Duration::from_secs(7), "{took:?}");
    assert_eq!(i.name, "slowinfo");
    assert_eq!(i.version, "");
    assert!(!i.is_placeholder());
    assert_eq!(i.ttl, DEFAULT_TTL);
}

#[test]
fn ext_info_reads_fields() {
    let fx = Fx::new();
    let i = fx.ext_info("echo");
    assert_eq!((i.name.as_str(), i.version.as_str()), ("echo", "1.0.0"));
    assert_eq!(i.description, "test plugin");
    assert!(i.is_placeholder());
    assert_eq!(i.options.len(), 4);
}

#[test]
fn ext_info_strings_sanitized() {
    let raw = r#"{"name":"x","version":"1","capabilities":["placeholder"],
      "options":{
        "k\u001b[31m":{"type":"string","default":"d\u001b]8;;\u0007"},
        "m":{"type":"enum","enum":["a\u001b[0m","\u202eb"],"default":"a\u001b[0m"}}}"#;
    let i = parse_info(raw).unwrap();
    let shown = i.to_json().to_string();
    assert!(
        !shown.contains("\\u001b") && !shown.contains('\x1b'),
        "{shown}"
    );
    assert!(
        !shown.contains('\u{202e}') && !shown.contains("\\u0007"),
        "{shown}"
    );
    assert!(i.options.contains_key("k[31m"));
    assert_eq!(i.options["m"].choices, ["a[0m", "b"]);
    assert_eq!(i.options["m"].default, Some(json!("a[0m")));
    assert_eq!(i.options["k[31m"].default, Some(json!("d]8;;")));
    // the sanitised choice is what the user types and what is accepted
    assert!(validate(&i.options, &opts(&[("m", "b")])).is_ok());
}

#[test]
fn ttl_defaults_60s() {
    let base = r#"{"name":"x","version":"1","capabilities":["placeholder"]"#;
    assert_eq!(DEFAULT_TTL, 60);
    assert_eq!(parse_info(&format!("{base}}}")).unwrap().ttl, 60);
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":300}}"#)).unwrap().ttl,
        300
    );
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":"5m"}}"#)).unwrap().ttl,
        300
    );
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":"junk"}}"#))
            .unwrap()
            .ttl,
        60
    );
    assert!(parse_info(r#"{"version":"1"}"#).is_none());
    assert!(parse_info("not json").is_none());
    // the plan uses it, with `plugin <name> ttl=` overriding
    let fx = Fx::new();
    fx.prime_info("echo");
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let c = Config::default();
    let p = plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap();
    assert_eq!(p.ttl, 60);
    let c = cfg("plugin echo ttl=5m\n");
    let p = plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap();
    assert_eq!(p.ttl, 300);
}

#[test]
fn ext_info_cached_not_per_render() {
    let fx = Fx::new();
    let c = cfg("");
    for _ in 0..3 {
        assert_eq!(fx.show("echo", &c, &[]), "");
    }
    assert_eq!(fx.info_calls(), 0, "cold render must not interrogate");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.info_calls(), 1);
    for _ in 0..3 {
        assert_eq!(fx.show("echo", &c, &[]), "ok");
    }
    assert_eq!(fx.info_calls(), 1);
}

#[test]
fn plugin_without_placeholder_capability_not_rendered() {
    let fx = Fx::new();
    fx.prime_info("plain");
    let c = Config::default();
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let found = fx.found("plain");
    let inf = cached_info(&cache, &found).unwrap();
    assert!(plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).is_none());
}

#[test]
fn row_placeholder_names_never_resolve_to_plugins() {
    // a `bar-task.name` on PATH: `.` is not a plugin name character, so
    // `#{task.name}` in the status line looks nothing up and draws nothing
    let fx = Fx::new();
    std::fs::copy(fx.bin.join("bar-echo"), fx.bin.join("bar-task.name")).unwrap();
    for n in hop_bar::placeholders::task::names() {
        assert!(find(&fx.dirs(), n).is_none(), "{n}");
    }
    let c = cfg("set left \"#{task.name}\"\n");
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let out = render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    assert_eq!(out, "");
}
