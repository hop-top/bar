// Account rules handed to plugins (`inputs.accounts`): render key and job,
// click input, `bar <name>` environment, `label_color`, `account.color`.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::cache::Job;
use hop_bar::click::action::{self, NONCE_FILE};
use hop_bar::placeholders::lookup;
use hop_bar::plugin::{self, cached_info, parse_info};
use hop_bar::style::strip_codes;
use serde_json::json;

const RULES: &str =
    "account \"*@corp.test\" work 33\naccount \"*@home.test\" personal \"#112233\"\n";

fn declared() -> serde_json::Value {
    json!({"inputs": {"accounts": true}})
}

fn plan_for(fx: &Fx, conf: &str) -> plugin::Plan {
    let (cache, dirs, c) = (fx.cache(), fx.dirs(), cfg(conf));
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let s = session();
    plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap()
}

#[test]
fn accounts_input_is_declared_in_ext_info() {
    let on = parse_info(&echo_info(declared())).unwrap();
    assert!(on.inputs.accounts);
    assert!(!on.inputs.is_empty());
    assert!(!parse_info(&echo_info(json!({}))).unwrap().inputs.accounts);
    let off = parse_info(&echo_info(json!({"inputs": {"accounts": "yes"}}))).unwrap();
    assert!(!off.inputs.accounts, "a boolean only");
}

#[test]
fn rules_split_the_key_by_hash_and_never_travel_in_the_job() {
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(declared()));
    fx.prime_info("echo");
    let none = plan_for(&fx, "");
    let a = plan_for(&fx, RULES);
    let b = plan_for(&fx, "account \"*@corp.test\" job 33\n");
    assert_ne!(none.key, a.key);
    assert_ne!(a.key, b.key, "a changed line is a new entry");
    assert_eq!(a.key, plan_for(&fx, RULES).key);
    for p in [&a, &b] {
        assert!(!p.key.contains("corp.test"), "{}", p.key);
        assert!(!p.key.contains("home.test"), "{}", p.key);
        let Job::PluginRender {
            input, accounts, ..
        } = &p.job
        else {
            panic!("{:?}", p.job)
        };
        assert!(accounts, "the job loads the rules itself");
        assert!(!input.contains("corp.test") && !input.contains("accounts"));
        // what a refresh process gets in argv
        let json = format!("{:?}", p.job);
        assert!(!json.contains("corp.test"), "{json}");
    }
    let Job::PluginRender { accounts, .. } = none.job else {
        panic!()
    };
    assert!(!accounts, "no rules, nothing to load");
}

#[test]
fn a_plugin_that_did_not_declare_it_gets_nothing_and_keeps_its_key() {
    let fx = Fx::new();
    fx.prime_info("echo");
    let (a, b) = (plan_for(&fx, RULES), plan_for(&fx, ""));
    assert_eq!(a.key, b.key, "rules are not an input of this plugin");
    let Job::PluginRender { accounts, .. } = a.job else {
        panic!()
    };
    assert!(!accounts);
}

#[test]
fn with_accounts_adds_the_rules_to_the_input() {
    let rules = plugin::account_rules(&cfg(RULES));
    assert_eq!(
        rules,
        json!([
            {"glob": "*@corp.test", "label": "work", "color": "33"},
            {"glob": "*@home.test", "label": "personal", "color": "#112233"},
        ])
    );
    let v: serde_json::Value =
        serde_json::from_str(&plugin::with_accounts(r#"{"options":{}}"#, rules)).unwrap();
    assert_eq!(v["accounts"][0]["label"], "work");
    assert_eq!(v["options"], json!({}));
    let v: serde_json::Value =
        serde_json::from_str(&plugin::with_accounts(r#"{"a":1}"#, json!([]))).unwrap();
    assert!(v.get("accounts").is_none(), "empty rules: no key");
    assert_eq!(
        plugin::with_accounts("not json", json!([{"x":1}])),
        "not json"
    );
}

fn click_setup(ext: serde_json::Value, conf: &str) -> (Fx, hop_bar::conf::Config, String) {
    let fx = Fx::new();
    let mut info = json!({"actions": {"use": {"description": "d", "tokens": ["account"]}}});
    for (k, v) in ext.as_object().unwrap() {
        info[k] = v.clone();
    }
    fx.put("ext.json", &echo_info(info));
    fx.put("out.json", r#"{"text":"x","tokens":{"account":"a1"}}"#);
    let n = fx.state.join(NONCE_FILE);
    std::fs::write(&n, "0123456789abcdef0123456789abcdef").unwrap();
    std::fs::set_permissions(&n, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let c = cfg(&format!("action echo echo.use\n{conf}"));
    fx.prime("echo", &c, &[]);
    let shown = fx.show("echo", &c, &[]);
    let url = shown
        .split("\x1b]8;;")
        .skip(1)
        .filter_map(|r| r.split(['\x1b', '\x07']).next())
        .find(|u| u.starts_with("bar://action/"))
        .unwrap()
        .to_string();
    (fx, c, url)
}

#[test]
fn a_click_hands_the_rules_to_a_plugin_that_wants_them() {
    let (fx, c, url) = click_setup(declared(), RULES);
    action::run(&fx.state, &url, &c, &fx.dirs()).unwrap();
    let got: serde_json::Value = serde_json::from_str(&fx.file("action.json")).unwrap();
    assert_eq!(got["tokens"], json!({"account": "a1"}));
    assert_eq!(got["accounts"][0]["glob"], "*@corp.test");
    assert_eq!(got["accounts"][1]["color"], "#112233");
}

#[test]
fn a_click_hands_nothing_to_one_that_does_not() {
    let (fx, c, url) = click_setup(json!({}), RULES);
    action::run(&fx.state, &url, &c, &fx.dirs()).unwrap();
    let got: serde_json::Value = serde_json::from_str(&fx.file("action.json")).unwrap();
    assert!(got.get("accounts").is_none(), "{got}");
}

fn bar_cmd(home: &common::tmp::Dir, bin: &std::path::Path) -> assert_cmd::Command {
    let (iso, cwd) = common::isolation(home);
    let mut c = assert_cmd::Command::cargo_bin("bar").unwrap();
    c.envs(iso)
        .current_dir(cwd)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    c
}

fn script(dir: &std::path::Path, name: &str, accounts: bool) {
    let p = dir.join(name);
    let inputs = if accounts {
        r#","inputs":{"accounts":true}"#
    } else {
        ""
    };
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--ext-info\" ]; then\n  echo '{{\"name\":\"{n}\",\"version\":\"1\",\"capabilities\":[\"placeholder\"]{inputs}}}'\n  exit 0\nfi\nprintf '%s' \"$BAR_ACCOUNTS\"\n",
            n = name.trim_start_matches("bar-"),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

#[test]
fn bar_name_sets_the_environment_for_a_plugin_that_wants_the_rules() {
    let home = common::tmp::Dir::new(&format!("bar-pacc-{}", std::process::id()));
    let bin = home.path().join("plugins");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(home.path().join("xdg/bar")).unwrap();
    std::fs::write(home.path().join("xdg/bar/bar.conf"), RULES).unwrap();
    script(&bin, "bar-want", true);
    script(&bin, "bar-skip", false);
    let out = bar_cmd(&home, &bin).arg("want").output().unwrap();
    let got: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{out:?}"));
    assert_eq!(got[0]["glob"], "*@corp.test");
    assert_eq!(got[1]["label"], "personal");
    let out = bar_cmd(&home, &bin).arg("skip").output().unwrap();
    assert!(out.stdout.is_empty(), "{out:?}");
}

#[test]
fn label_color_draws_the_label_in_that_colour() {
    let c = cfg("");
    let raw = r##"{"items":[{"label":"work","label_color":"33","text":"1"},
        {"label":"home","label_color":"#112233","text":"2"},
        {"label":"bad","label_color":"nope","text":"3"},
        {"label":"none","text":"4"}]}"##;
    let shown = paint(raw, "x", &c);
    assert!(
        shown.contains(&hop_bar::style::fg(33u8, "work")),
        "{shown:?}"
    );
    assert!(
        shown.contains(&hop_bar::style::fg(
            hop_bar::color::Color::Rgb([0x11, 0x22, 0x33]),
            "home"
        )),
        "{shown:?}"
    );
    let dimmed = |l: &str| shown.contains(&hop_bar::style::dim(l));
    assert!(dimmed("bad") && dimmed("none"), "{shown:?}");
    assert!(!dimmed("work"));
}

#[test]
fn an_account_with_a_colour_is_drawn_as_is_without_the_rules() {
    let fx = Fx::new();
    let claude = fx.root.join("claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
    // a glob that would relabel `work` if the rules were applied again
    let c = cfg("set account-source echo\naccount \"w*\" other 99\n");
    fx.put(
        "out.json",
        r#"{"text":"","account":{"id":"a1","label":"work","color":"33"}}"#,
    );
    fx.prime("echo", &c, &[]);
    let (cache, dirs, s) = (fx.cache(), fx.dirs(), session());
    let raw = lookup("account")
        .unwrap()
        .render(&ctx(&s, &c, &cache, &dirs), &opts(&[]))
        .unwrap();
    assert_eq!(strip_codes(&raw), "work");
    assert!(raw.contains(&hop_bar::style::fg(33u8, "work")), "{raw:?}");
    // without a colour the rules still apply, as before
    fx.put(
        "out.json",
        r#"{"text":"","account":{"id":"a1","label":"work"}}"#,
    );
    fx.prime("echo", &c, &[]);
    let raw = lookup("account")
        .unwrap()
        .render(&ctx(&s, &c, &cache, &dirs), &opts(&[]))
        .unwrap();
    assert_eq!(strip_codes(&raw), "other");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
}

#[test]
fn the_refresh_process_adds_the_rules_from_the_config_to_stdin() {
    let home = common::tmp::Dir::new(&format!("bar-pref-{}", std::process::id()));
    std::fs::create_dir_all(home.path().join("xdg/bar")).unwrap();
    std::fs::write(home.path().join("xdg/bar/bar.conf"), RULES).unwrap();
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(declared()));
    let cwd = home.path().join("neutral-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let input = json!({"options": {}, "session": {"cwd": cwd}}).to_string();
    let req = |accounts: bool| {
        hop_bar::cache::Request {
            key: "k".into(),
            job: Job::PluginRender {
                path: fx.found("echo").path.display().to_string(),
                input: input.clone(),
                timeout: 5,
                accounts,
                email: None,
            },
            lock: String::new(),
        }
        .to_json()
    };
    assert!(!req(true).contains("corp.test"), "argv carries no rule");
    let run = |accounts: bool| {
        let (iso, cwd) = common::isolation(&home);
        assert_cmd::Command::cargo_bin("bar")
            .unwrap()
            .envs(iso)
            .current_dir(cwd)
            .args(["refresh", &req(accounts)])
            .assert()
            .success();
        serde_json::from_str::<serde_json::Value>(&fx.file("stdin.json")).unwrap()
    };
    let got = run(true);
    assert_eq!(got["accounts"][0]["glob"], "*@corp.test", "{got}");
    assert_eq!(got["accounts"][1]["color"], "#112233");
    assert!(run(false).get("accounts").is_none());
}
