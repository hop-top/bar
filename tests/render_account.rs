mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::placeholders::account::glob_match;
const ACCOUNT_CONF: &str = "set right \"#{account}\"\n\
account \"*acme*\" acme 141\n\
account \"*@widgets.example.org\" widgets 75\n";

#[test]
fn account_first_match_wins() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("jad+acme@example.org"));
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "acme");
    assert!(o.raw.contains("38;5;141m"), "{:?}", o.raw);
}

#[test]
fn unknown_account_shows_email() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("someone@example.org"));
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "someone@example.org");
}

#[test]
fn account_reads_claude_config_dir() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("home@example.org"));
    t.write("cc/.claude.json", &account_json("cfg@widgets.example.org"));
    let cc = t.path().join("cc");
    let o = run(&t)
        .conf(ACCOUNT_CONF)
        .env("CLAUDE_CONFIG_DIR", cc.to_str().unwrap())
        .go();
    assert_eq!(o.plain.trim(), "widgets");
}

#[test]
fn account_api_key_marker() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(ACCOUNT_CONF)
        .env("ANTHROPIC_API_KEY", "sk-x")
        .go();
    assert_eq!(o.plain.trim(), "api-key");
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn account_glob_star_forms() {
    assert!(glob_match("*", "anything"));
    assert!(glob_match("*", ""));
    assert!(glob_match("a*", "abc"));
    assert!(!glob_match("a*", "ba"));
    assert!(glob_match("*c", "abc"));
    assert!(!glob_match("*c", "abd"));
    assert!(glob_match("*b*", "abc"));
    assert!(glob_match("a*c*e", "abcde"));
    assert!(!glob_match("a*c*e", "abde"));
    assert!(glob_match("a@b.io", "a@b.io"));
    assert!(!glob_match("a@b.io", "a@b.ioo"));
    assert!(glob_match("**x", "x"));
}
