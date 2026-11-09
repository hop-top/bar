// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::dirs::{abbrev_home, shown};
use hop_bar::placeholders::{lookup, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::{self, Session};
use hop_bar::style::strip_codes;

fn run(name: &str, s: &Session, opts: &str) -> Option<String> {
    let cfg = Config::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: s,
        config: &cfg,
        now: 0,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    let mut o = Options::default();
    for kv in opts.split(',').filter(|k| !k.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap();
        o.set(k, v);
    }
    lookup(name)
        .unwrap_or_else(|| panic!("{name} unregistered"))
        .render(&ctx, &o)
        .map(|t| strip_codes(&t))
}

fn p(json: &str) -> Session {
    session::parse(json)
}

fn cost(usd: &str, ms: &str) -> Session {
    p(&format!(
        r#"{{"cost":{{"total_cost_usd":{usd},"total_duration_ms":{ms}}}}}"#
    ))
}

#[test]
fn cost_formats() {
    assert_eq!(run("cost", &cost("1.234", "1"), "").unwrap(), "$1.23");
    assert_eq!(
        run("cost", &cost("1.235", "1"), "fmt=usd").unwrap(),
        "$1.24"
    );
    assert_eq!(run("cost", &cost("0.004", "1"), "").unwrap(), "<$0.01");
    assert_eq!(run("cost", &cost("0.01", "1"), "").unwrap(), "$0.01");
    assert_eq!(run("cost", &cost("0", "1"), "").unwrap(), "$0.00");
    assert_eq!(run("cost", &cost("1234.5", "1"), "").unwrap(), "$1234.50");
}

#[test]
fn cost_absent_or_malformed_is_hidden() {
    assert_eq!(run("cost", &Session::default(), ""), None);
    assert_eq!(run("cost", &cost("-1", "1"), ""), None);
    assert_eq!(run("cost", &cost("\"x\"", "1"), ""), None);
    assert_eq!(run("cost", &cost("null", "1"), ""), None);
    assert_eq!(run("cost", &cost("1e999", "1"), ""), None);
}

#[test]
fn burn_per_hour() {
    // $2.05 in 30 min = $4.10/h
    assert_eq!(
        run("burn", &cost("2.05", "1800000"), "").unwrap(),
        "$4.10/h"
    );
    assert_eq!(
        run("burn", &cost("0.0001", "3600000"), "").unwrap(),
        "<$0.01/h"
    );
}

#[test]
fn burn_hidden_under_a_minute_or_bad_input() {
    assert_eq!(run("burn", &cost("5", "59999"), ""), None);
    assert!(run("burn", &cost("5", "60000"), "").is_some());
    assert_eq!(run("burn", &cost("5", "0"), ""), None);
    assert_eq!(run("burn", &cost("5", "-10"), ""), None);
    assert_eq!(run("burn", &cost("-5", "600000"), ""), None);
    assert_eq!(run("burn", &Session::default(), ""), None);
    assert_eq!(run("burn", &cost("5", "\"x\""), ""), None);
}

#[test]
fn duration_compact() {
    let d = |ms: &str| run("duration", &cost("0", ms), "");
    assert_eq!(d("45000").unwrap(), "45s");
    assert_eq!(d("0").unwrap(), "0s");
    assert_eq!(d("59999").unwrap(), "59s");
    assert_eq!(d("60000").unwrap(), "1m");
    assert_eq!(d("720000").unwrap(), "12m");
    assert_eq!(d("3900000").unwrap(), "1h05m");
    assert_eq!(d("3600000").unwrap(), "1h00m");
    assert_eq!(d("90000000").unwrap(), "25h00m");
    assert_eq!(d("1e300").unwrap(), "999h00m");
    assert_eq!(d("-5"), None);
    assert_eq!(d("1e999"), None);
    assert_eq!(d("\"x\""), None);
    assert_eq!(run("duration", &Session::default(), ""), None);
}

#[test]
fn lines_added_removed() {
    let l = |a: &str, r: &str| {
        run(
            "lines",
            &p(&format!(
                r#"{{"cost":{{"total_lines_added":{a},"total_lines_removed":{r}}}}}"#
            )),
            "",
        )
    };
    assert_eq!(l("120", "14").unwrap(), "+120 -14");
    assert_eq!(l("3", "0").unwrap(), "+3 -0");
    assert_eq!(l("0", "0"), None);
    assert_eq!(l("-4", "\"x\""), None);
    assert_eq!(run("lines", &Session::default(), ""), None);
}

#[test]
fn simple_text_fields() {
    let s = p(
        r#"{"version":"2.1.5","output_style":{"name":"Learning"},"vim":{"mode":"NORMAL"},
            "agent":{"name":"reviewer"},"thinking":{"enabled":true},"fast_mode":true}"#,
    );
    assert_eq!(run("version", &s, "").unwrap(), "2.1.5");
    assert_eq!(run("style", &s, "").unwrap(), "Learning");
    assert_eq!(run("vim", &s, "").unwrap(), "NORMAL");
    assert_eq!(run("agent", &s, "").unwrap(), "reviewer");
    assert_eq!(run("thinking", &s, "").unwrap(), "think");
    assert_eq!(run("fast", &s, "").unwrap(), "fast");
}

#[test]
fn simple_fields_hidden_when_absent_default_or_malformed() {
    let none = Session::default();
    for n in ["version", "style", "vim", "agent", "thinking", "fast"] {
        assert_eq!(run(n, &none, ""), None, "{n}");
    }
    let s = p(r#"{"output_style":{"name":"default"}}"#);
    assert_eq!(run("style", &s, ""), None);
    let s = p(r#"{"output_style":{"name":"default\u001b"}}"#);
    assert_eq!(run("style", &s, ""), None);
    let s = p(
        r#"{"version":5,"output_style":"x","vim":[],"agent":{"name":null},
            "thinking":{"enabled":false},"fast_mode":"yes"}"#,
    );
    for n in ["version", "style", "vim", "agent", "thinking", "fast"] {
        assert_eq!(run(n, &s, ""), None, "{n}");
    }
}

#[test]
fn control_chars_are_sanitized() {
    let s = p(r#"{"agent":{"name":"a\u001b[31mb"}}"#);
    assert!(!run("agent", &s, "").unwrap().contains('\x1b'));
}

#[test]
fn home_abbreviation() {
    assert_eq!(abbrev_home("/Users/j/w/x", "/Users/j"), "~/w/x");
    assert_eq!(abbrev_home("/Users/j", "/Users/j"), "~");
    assert_eq!(abbrev_home("/Users/j/", "/Users/j"), "~/");
    assert_eq!(abbrev_home("/Users/jx/w", "/Users/j"), "/Users/jx/w");
    assert_eq!(abbrev_home("/tmp/x", "/Users/j"), "/tmp/x");
    assert_eq!(abbrev_home("/tmp/x", ""), "/tmp/x");
    assert_eq!(abbrev_home("/tmp/x", "/"), "/tmp/x");
}

#[test]
fn cwd_full_and_base() {
    let show = |path: &str, o: &str, home: &str| {
        let mut opts = Options::default();
        if let Some((k, v)) = o.split_once('=') {
            opts.set(k, v);
        }
        shown(path, &opts, home).map(|t| strip_codes(&t))
    };
    assert_eq!(show("/srv/app/api", "", "/h").unwrap(), "/srv/app/api");
    assert_eq!(show("/srv/app/api", "base=1", "/h").unwrap(), "api");
    assert_eq!(
        show("/srv/app/api", "base=0", "/h").unwrap(),
        "/srv/app/api"
    );
    assert_eq!(show("/h/code/x", "", "/h").unwrap(), "~/code/x");
    assert_eq!(show("/h/code/x", "base=1", "/h").unwrap(), "x");
    assert_eq!(show("/h", "base=1", "/h").unwrap(), "~");
    assert_eq!(show("/h/code/x", "", "/").unwrap(), "/h/code/x");
    let s = p(r#"{"workspace":{"current_dir":"/srv/app/api"}}"#);
    assert!(run("cwd", &s, "base=1").unwrap().ends_with("api"));
    assert_eq!(run("cwd", &Session::default(), ""), None);
    assert_eq!(run("cwd", &p(r#"{"cwd":5}"#), ""), None);
}

struct TempDir(std::path::PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn root_from_gitfs_and_hidden_outside_repo() {
    for k in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"] {
        std::env::remove_var(k);
    }
    let root = TempDir(std::env::temp_dir().join(format!("bar-root-{}", std::process::id())));
    let dir = &root.0;
    let sub = dir.join("repo/src/deep");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::write(dir.join("repo/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let json = format!(
        r#"{{"workspace":{{"current_dir":{:?}}}}}"#,
        sub.to_str().unwrap()
    );
    let s = p(&json);
    let full = run("root", &s, "").unwrap();
    assert!(full.ends_with("/repo"), "{full}");
    assert_eq!(run("root", &s, "base=1").unwrap(), "repo");
    let out = dir.join("plain");
    std::fs::create_dir_all(&out).unwrap();
    let s = p(&format!(r#"{{"cwd":{:?}}}"#, out.to_str().unwrap()));
    assert_eq!(run("root", &s, ""), None);
}

fn named(name: &str) -> Session {
    p(&format!(r#"{{"session_name":{name}}}"#))
}

#[test]
fn name_shows_session_name_truncated_and_sanitized() {
    assert_eq!(
        run("name", &named("\"fix-login\""), "").unwrap(),
        "fix-login"
    );
    assert_eq!(
        run("name", &named("\"fix-login-bug\""), "max=6").unwrap(),
        "fix-l…"
    );
    assert_eq!(
        run("name", &named("\"a\\u001b[31mb\""), "").unwrap(),
        "a[31mb"
    );
}

#[test]
fn name_hidden_when_absent_empty_or_mistyped() {
    assert_eq!(run("name", &Session::default(), ""), None);
    assert_eq!(run("name", &named("\"\""), ""), None);
    assert_eq!(run("name", &named("7"), ""), None);
}

#[test]
fn api_duration_formats_like_duration() {
    let a = |ms: &str| {
        run(
            "api",
            &p(&format!(r#"{{"cost":{{"total_api_duration_ms":{ms}}}}}"#)),
            "",
        )
    };
    assert_eq!(a("45000").unwrap(), "45s");
    assert_eq!(a("720000").unwrap(), "12m");
    assert_eq!(a("3900000").unwrap(), "1h05m");
    assert_eq!(a("-1"), None);
    assert_eq!(a("\"x\""), None);
    // the wall-time field is not the API time
    assert_eq!(run("api", &cost("0", "5000"), ""), None);
    assert_eq!(run("api", &Session::default(), ""), None);
}
