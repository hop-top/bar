// Plugin-declared actions: bindings, links, the click handler, `bar check`.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::click::action::{self, NONCE_FILE, STATUS_FILE};
use hop_bar::conf::{layers, parse, Config};
use hop_bar::plugin;
use hop_bar::style::strip_codes;
use serde_json::json;
use std::collections::BTreeMap;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn use_action() -> serde_json::Value {
    json!({"actions": {"use": {"description": "Prefer this account", "tokens": ["account", "provider"]}}})
}

/// Echo with the `use` action declared, a nonce installed, `out` as its
/// output, and the bindings in `conf`.
fn setup(out: &str, conf: &str) -> (Fx, Config) {
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(use_action()));
    fx.put("out.json", out);
    let f = fx.state.join(NONCE_FILE);
    std::fs::write(&f, NONCE).unwrap();
    std::fs::set_permissions(&f, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let c = cfg(conf);
    fx.prime("echo", &c, &[]);
    (fx, c)
}

fn links(shown: &str) -> Vec<String> {
    shown
        .split("\x1b]8;;")
        .skip(1)
        .filter_map(|r| r.split(['\x1b', '\x07']).next())
        .filter(|u| u.starts_with("bar://action/"))
        .map(String::from)
        .collect()
}

const ONE: &str = r#"{"text":"alice","tokens":{"account":"a1","provider":"claude"}}"#;

// --- config ---

#[test]
fn action_line_parses_and_replaces_per_placeholder() {
    let c = cfg("action echo echo.use\naction account cliproxyapi.use\naction echo echo.other\n");
    let got: Vec<_> = c
        .actions
        .iter()
        .map(|b| (b.placeholder.as_str(), b.plugin.as_str(), b.action.as_str()))
        .collect();
    assert_eq!(
        got,
        [("account", "cliproxyapi", "use"), ("echo", "echo", "other")]
    );
}

#[test]
fn bad_action_lines_are_errors() {
    for bad in [
        "action",
        "action echo",
        "action echo echo",
        "action echo echo.use extra",
        "action echo ech/o.use",
        "action echo echo.u.se",
        "action \"a b\" echo.use",
        "action echo .use",
        "action echo echo.",
    ] {
        assert!(parse(&format!("{bad}\n")).is_err(), "{bad}");
    }
}

#[test]
fn action_is_user_config_only() {
    for mode in ["safe", "full"] {
        let user = format!("set project-config {mode}\n");
        let l = layers(Some(&user), |_| Some(Ok("action echo echo.use\n".into())));
        assert!(l.config.unwrap().actions.is_empty(), "{mode}");
        assert!(
            l.dropped[0].message.contains("user config"),
            "{:?}",
            l.dropped
        );
    }
}

// --- ext-info and output ---

#[test]
fn ext_info_actions_are_parsed_and_bounded() {
    let info = plugin::parse_info(&echo_info(json!({"actions": {
        "use": {"description": "d\u{1b}", "tokens": ["provider", "account", "account", "Bad Name"], "confirm": true},
        "bad name": {"tokens": []},
        "none": {},
    }})))
    .unwrap();
    let u = &info.actions["use"];
    assert_eq!(u.tokens, ["account", "provider"]);
    assert!(u.confirm);
    assert!(!u.description.contains('\u{1b}'));
    assert!(info.actions.contains_key("none") && !info.actions.contains_key("bad name"));
    assert!(plugin::parse_info(&echo_info(json!({"actions": 5})))
        .unwrap()
        .actions
        .is_empty());
}

#[test]
fn output_tokens_keep_only_valid_ones() {
    let o = plugin::parse_output(
        r#"{"text":"x","tokens":{"account":"a1b2.c-d_e@f","provider":"claude","Bad":"x","n":"a b","len":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx","num":5}}"#,
    )
    .unwrap();
    let keys: Vec<&str> = o.tokens.keys().map(String::as_str).collect();
    assert_eq!(keys, ["account", "provider"]);
}

// --- links ---

#[test]
fn plugin_placeholder_links_its_text() {
    let (fx, c) = setup(ONE, "action echo echo.use\n");
    let shown = fx.show("echo", &c, &[]);
    let l = links(&shown);
    assert_eq!(l.len(), 1, "{shown:?}");
    let u = action::parse(&l[0]).unwrap();
    assert_eq!((u.plugin.as_str(), u.action.as_str()), ("echo", "use"));
    assert_eq!(u.tokens["account"], "a1");
    assert_eq!(u.tokens["provider"], "claude");
    assert_eq!(u.nonce, NONCE);
    assert_eq!(strip_codes(&shown), "alice");
}

#[test]
fn each_item_gets_its_own_tokens() {
    let out = r#"{"tokens":{"provider":"claude"},"items":[
        {"text":"a","tokens":{"account":"a1"}},
        {"text":"b","tokens":{"account":"b2","provider":"codex"}},
        {"text":"c"}]}"#;
    let (fx, c) = setup(out, "action echo echo.use\n");
    let l: Vec<_> = links(&fx.show("echo", &c, &[]))
        .iter()
        .map(|u| action::parse(u).unwrap().tokens)
        .collect();
    let want = |a: &str, p: &str| {
        BTreeMap::from([
            ("account".to_string(), a.to_string()),
            ("provider".to_string(), p.to_string()),
        ])
    };
    assert_eq!(
        l,
        [want("a1", "claude"), want("b2", "codex")],
        "item c has no account: unlinked"
    );
}

#[test]
fn no_link_without_what_it_needs() {
    // no binding
    let (fx, c) = setup(ONE, "");
    assert!(links(&fx.show("echo", &c, &[])).is_empty());
    // bound to another plugin's action
    let (fx, c) = setup(ONE, "action echo other.use\n");
    assert!(links(&fx.show("echo", &c, &[])).is_empty());
    // undeclared action
    let (fx, c) = setup(ONE, "action echo echo.nope\n");
    assert!(links(&fx.show("echo", &c, &[])).is_empty());
    // a token the action takes is missing
    let (fx, c) = setup(
        r#"{"text":"x","tokens":{"account":"a1"}}"#,
        "action echo echo.use\n",
    );
    assert!(links(&fx.show("echo", &c, &[])).is_empty());
    // the plugin's own link stays
    let (fx, c) = setup(
        r#"{"text":"x","link":"https://example.test/","tokens":{"account":"a1","provider":"claude"}}"#,
        "action echo echo.use\n",
    );
    let shown = fx.show("echo", &c, &[]);
    assert!(links(&shown).is_empty() && shown.contains("https://example.test/"));
    // click=none
    let (fx, c) = setup(ONE, "action echo echo.use\n");
    assert!(links(&fx.show("echo", &c, &[("click", "none")])).is_empty());
    // handler without action support (no nonce)
    std::fs::remove_file(fx.state.join(NONCE_FILE)).unwrap();
    assert!(links(&fx.show("echo", &c, &[])).is_empty());
}

#[test]
fn account_links_through_the_account_source() {
    let out = r#"{"text":"","account":{"id":"a1","label":"alice"},"tokens":{"account":"a1","provider":"claude"}}"#;
    let conf = "set account-source echo\naction account echo.use\n";
    let (fx, c) = setup(out, conf);
    let claude = fx.root.join("claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
    let (cache, dirs, s) = (fx.cache(), fx.dirs(), session());
    let draw = |c: &Config| {
        hop_bar::render::render_in(
            &s,
            c,
            Some(120),
            &hop_bar::render::ColdRunner,
            Some(&cache),
            &dirs,
        )
    };
    let shown = draw(&cfg(&format!("{conf}set left \"#{{account}}\"\n")));
    let l = links(&shown);
    assert_eq!(l.len(), 1, "{shown:?}");
    assert_eq!(action::parse(&l[0]).unwrap().tokens["account"], "a1");
    assert_eq!(strip_codes(&shown), "alice");
    // without the source's account there is nothing to act on
    fx.put(
        "out.json",
        r#"{"text":"","tokens":{"account":"a1","provider":"claude"}}"#,
    );
    fx.prime("echo", &c, &[]);
    let shown = draw(&cfg(&format!("{conf}set left \"#{{account}}\"\n")));
    assert!(links(&shown).is_empty(), "{shown:?}");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
}

#[test]
fn an_action_not_aimed_at_the_shown_account_links_without_the_sources_account() {
    let fx = Fx::new();
    fx.put(
        "ext.json",
        &echo_info(
            json!({"actions": {"next": {"description": "d", "tokens": ["instance", "provider"]}}}),
        ),
    );
    fx.put(
        "out.json",
        r#"{"text":"","tokens":{"instance":"0123456789abcdef","provider":"claude"}}"#,
    );
    std::fs::write(fx.state.join(NONCE_FILE), NONCE).unwrap();
    std::fs::set_permissions(
        fx.state.join(NONCE_FILE),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    let conf = "set account-source echo\naction account echo.next\n";
    let c = cfg(conf);
    fx.prime("echo", &c, &[]);
    let claude = fx.root.join("claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(
        claude.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"local@example.test"}}"#,
    )
    .unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
    let (cache, dirs, s) = (fx.cache(), fx.dirs(), session());
    let shown = hop_bar::render::render_in(
        &s,
        &cfg(&format!("{conf}set left \"#{{account}}\"\n")),
        Some(120),
        &hop_bar::render::ColdRunner,
        Some(&cache),
        &dirs,
    );
    let l = links(&shown);
    assert_eq!(l.len(), 1, "{shown:?}");
    assert_eq!(action::parse(&l[0]).unwrap().tokens["provider"], "claude");
    assert_eq!(strip_codes(&shown), "local@example.test");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
}

// --- the handler ---

/// Renders, takes the link, and returns what the handler needs.
fn linked(out: &str) -> (Fx, Config, String) {
    let (fx, c) = setup(out, "action echo echo.use\n");
    let url = links(&fx.show("echo", &c, &[])).remove(0);
    (fx, c, url)
}

fn run(fx: &Fx, c: &Config, url: &str) -> Result<(), String> {
    action::run(&fx.state, url, c, &fx.dirs())
}

fn ran(fx: &Fx) -> bool {
    fx.bin.join("action.json").exists()
}

#[test]
fn handler_runs_the_action_with_tokens_on_stdin() {
    let (fx, c, url) = linked(ONE);
    run(&fx, &c, &url).unwrap();
    let got: serde_json::Value = serde_json::from_str(&fx.file("action.json")).unwrap();
    assert_eq!(
        got,
        json!({"action": "use", "tokens": {"account": "a1", "provider": "claude"}})
    );
    assert_eq!(fx.file("action.name").trim(), "use");
    let status = std::fs::read_to_string(fx.state.join(STATUS_FILE)).unwrap();
    assert!(status.trim_end().ends_with(" ok"), "{status:?}");
    // the entry the link came from is stale now, value kept
    let entry = action::parse(&url).unwrap().entry;
    let path = fx.state.join("cache").join(&entry);
    let age = std::fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap()
        .elapsed()
        .unwrap();
    assert!(age.as_secs() > 86_400 * 365, "{age:?}");
    assert!(std::fs::read_to_string(path).unwrap().contains("alice"));
}

#[test]
fn the_action_child_gets_the_state_dir_the_link_was_drawn_in() {
    let (fx, c, url) = linked(ONE);
    run(&fx, &c, &url).unwrap();
    assert_eq!(fx.file("action.state"), fx.state.display().to_string());
}

#[test]
fn the_status_carries_the_first_line_the_action_printed() {
    let (fx, c, url) = linked(ONE);
    fx.put("action-out", "cancelled\u{1b}[31m\nsecond line\n");
    run(&fx, &c, &url).unwrap();
    let status = std::fs::read_to_string(fx.state.join(STATUS_FILE)).unwrap();
    assert!(
        status.trim_end().ends_with(" ok: cancelled[31m"),
        "{status:?}"
    );
    assert_eq!(status.lines().count(), 1);
    assert!(!status.contains('\u{1b}') && !status.contains("second"));
}

#[test]
fn an_action_may_declare_a_longer_limit_up_to_a_cap() {
    use std::time::Duration;
    assert_eq!(action::limit(0), Duration::from_secs(10));
    assert_eq!(action::limit(60), Duration::from_secs(60));
    assert_eq!(action::limit(9999), action::MAX_ACTION_TIMEOUT);
    assert_eq!(action::MAX_ACTION_TIMEOUT, Duration::from_secs(120));
    let info = plugin::parse_info(&echo_info(json!({"actions": {
        "a": {"timeout": 60}, "b": {"timeout": 9999}, "c": {}, "d": {"timeout": "x"},
    }})))
    .unwrap();
    let t = |k: &str| info.actions[k].timeout;
    assert_eq!((t("a"), t("b"), t("c"), t("d")), (60, 120, 0, 0));
}

#[test]
fn handler_reports_a_failing_plugin() {
    let (fx, c, url) = linked(ONE);
    fx.put("action-exit", "3");
    let e = run(&fx, &c, &url).unwrap_err();
    assert!(e.contains("echo.use failed"), "{e}");
    let status = std::fs::read_to_string(fx.state.join(STATUS_FILE)).unwrap();
    assert!(status.contains("refused"), "{status:?}");
}

fn swap(url: &str, from: &str, to: &str) -> String {
    assert!(url.contains(from), "{url} lacks {from}");
    url.replace(from, to)
}

#[test]
fn handler_refuses_forged_or_stale_links() {
    let (fx, c, url) = linked(ONE);
    let entry = action::parse(&url).unwrap().entry;
    let bad = [
        // another nonce, or none
        swap(&url, NONCE, "ffffffffffffffffffffffffffffffff"),
        url.rsplit_once("&n=").unwrap().0.to_string(),
        // token values the cached output does not hold
        swap(&url, "account=a1", "account=zz"),
        swap(&url, "provider=claude", "provider=codex"),
        // a token the action does not declare, or one missing
        swap(&url, "account=a1&", "account=a1&extra=1&"),
        swap(&url, "account=a1&", ""),
        // an entry that does not exist
        swap(&url, &entry, "00000000000000000000000000000000"),
        // not a plugin or action this link may name
        swap(&url, "echo.use", "echo.nope"),
        swap(&url, "echo.use", "other.use"),
        swap(&url, "bar://action/", "bar://cmd/"),
        swap(&url, "bar://action/", "bar://action//"),
        format!("{url}&n={NONCE}"),
        format!("{url}%0a"),
        format!("{url};rm"),
        swap(&url, "?", "?x=&"),
        "bar://action/echo.use".to_string(),
        String::new(),
    ];
    for u in &bad {
        assert!(run(&fx, &c, u).is_err(), "accepted {u}");
        assert!(!ran(&fx), "ran for {u}");
    }
    // the good one still works afterwards
    run(&fx, &c, &url).unwrap();
    assert!(ran(&fx));
}

#[test]
fn handler_needs_the_user_binding_and_the_declaration() {
    let (fx, _, url) = linked(ONE);
    // no `action` line
    assert!(run(&fx, &cfg(""), &url)
        .unwrap_err()
        .contains("no `action` line"));
    // a line for another action
    assert!(run(&fx, &cfg("action echo echo.other\n"), &url).is_err());
    assert!(!ran(&fx));
    // the plugin stops declaring it
    fx.put("ext.json", &echo_info(json!({})));
    std::fs::remove_dir_all(fx.state.join("cache")).ok();
    let c = cfg("action echo echo.use\n");
    let e = run(&fx, &c, &url).unwrap_err();
    assert!(e.contains("does not declare") || e.contains("gone"), "{e}");
    assert!(!ran(&fx));
}

#[test]
fn handler_fails_closed_without_a_nonce() {
    let (fx, c, url) = linked(ONE);
    std::fs::remove_file(fx.state.join(NONCE_FILE)).unwrap();
    assert!(run(&fx, &c, &url).is_err());
    std::fs::write(fx.state.join(NONCE_FILE), "short").unwrap();
    assert!(run(&fx, &c, &url).is_err());
    assert!(!ran(&fx));
}

#[test]
fn nonce_is_created_once_and_private() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    assert!(!action::handler_routes_actions(&fx.state));
    let a = action::ensure_nonce(&fx.state).unwrap();
    assert_eq!(a.len(), 32);
    assert_eq!(action::ensure_nonce(&fx.state).unwrap(), a, "kept");
    let mode = std::fs::metadata(fx.state.join(NONCE_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(action::handler_routes_actions(&fx.state));
}

// --- bar check ---

fn check(fx: &Fx, conf: &str) -> Vec<String> {
    hop_bar::check::check_text(conf, None, &fx.dirs())
        .issues
        .into_iter()
        .map(|i| format!("{:?}: {}", i.level, i.message))
        .collect()
}

#[test]
fn check_validates_action_lines() {
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(use_action()));
    let ok = check(&fx, "action echo echo.use\n");
    assert!(ok.iter().all(|m| !m.starts_with("Error")), "{ok:?}");
    let undeclared = check(&fx, "action echo echo.nope\n");
    assert!(
        undeclared
            .iter()
            .any(|m| m.contains("does not declare the action `nope`")),
        "{undeclared:?}"
    );
    let other = check(&fx, "action echo echo2.use\n");
    assert!(
        other.iter().any(|m| m.contains("only its own actions")),
        "{other:?}"
    );
    let core = check(&fx, "action repo echo.use\n");
    assert!(
        core.iter().any(|m| m.contains("only `account`")),
        "{core:?}"
    );
    let no_src = check(&fx, "action account echo.use\n");
    assert!(
        no_src.iter().any(|m| m.contains("set account-source echo")),
        "{no_src:?}"
    );
    let with_src = check(&fx, "set account-source echo\naction account echo.use\n");
    assert!(
        with_src.iter().all(|m| !m.starts_with("Error")),
        "{with_src:?}"
    );
    let missing = check(&fx, "action nosuch nosuch.use\n");
    assert!(
        missing.iter().any(|m| m.contains("not on PATH")),
        "{missing:?}"
    );
}

#[test]
fn check_runs_the_check_capability_only_when_declared() {
    let fx = Fx::new();
    fx.put(
        "check.json",
        r#"{"warnings":["instance file mode 0644","a\u001b]0;x\u0007b"]}"#,
    );
    let conf = "set left \"#{echo}\"\n";
    let none = check(&fx, conf);
    assert!(
        none.iter().all(|m| !m.contains("instance file")),
        "not declared: {none:?}"
    );
    fx.put(
        "ext.json",
        &echo_info(json!({"capabilities": ["placeholder", "check"]})),
    );
    let got = check(&fx, conf);
    assert!(
        got.iter()
            .any(|m| m == "Warning: plugin `echo`: instance file mode 0644"),
        "{got:?}"
    );
    assert!(
        got.iter()
            .all(|m| !m.contains('\u{1b}') && !m.contains('\u{7}')),
        "{got:?}"
    );
    // a bare array works too, and nothing printed is nothing reported
    fx.put("check.json", r#"["only one"]"#);
    assert!(check(&fx, conf)
        .iter()
        .any(|m| m.ends_with("plugin `echo`: only one")));
    fx.put("check.json", "not json");
    assert!(check(&fx, conf)
        .iter()
        .all(|m| !m.contains("plugin `echo`:")));
}

#[test]
fn check_warns_about_refused_inputs() {
    let fx = Fx::new();
    fx.put(
        "ext.json",
        &echo_info(json!({"inputs": {"env": ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"]}})),
    );
    let got = check(&fx, "set left \"#{echo}\"\n");
    assert!(
        got.iter()
            .any(|m| m.contains("`ANTHROPIC_AUTH_TOKEN`") && m.contains("credential")),
        "{got:?}"
    );
    assert!(
        got.iter().all(|m| !m.contains("ANTHROPIC_BASE_URL")),
        "{got:?}"
    );
}

// --- CLI ---

#[test]
fn bar_click_action_refuses_and_exits_1() {
    let home = common::tmp::Dir::new(&format!("bar-action-cli-{}", std::process::id()));
    let (iso, cwd) = common::isolation(&home);
    let url = format!("bar://action/echo.use?account=a1&k={NONCE}&n={NONCE}");
    assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "action", &url])
        .envs(iso)
        .current_dir(cwd)
        .assert()
        .code(1)
        .stderr(predicates::str::contains("refused"));
}

#[test]
fn a_nonce_file_others_can_read_is_not_a_nonce_and_is_replaced_private() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let f = fx.state.join(NONCE_FILE);
    std::fs::write(&f, NONCE).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(action::nonce(&fx.state), None);
    let n = action::ensure_nonce(&fx.state).unwrap();
    assert_ne!(n, NONCE);
    assert_eq!(action::nonce(&fx.state), Some(n));
    let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
