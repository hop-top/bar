// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

const CONF: &str = "set left \"#{modes}\"";

fn go(t: &Tmp, conf: &str) -> String {
    run(t).conf(conf).cwd(&t.path().join("proj")).go().plain
}

#[test]
fn modes_hidden_without_settings() {
    let t = Tmp::new();
    assert_eq!(go(&t, CONF).trim(), "");
}

#[test]
fn modes_lists_enabled_flags_in_fixed_order() {
    let t = Tmp::new();
    t.write(
        ".claude/settings.json",
        r#"{"remoteControlAtStartup":true,"voice":{"enabled":true,"mode":"tap"},"sandbox":{"enabled":true}}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "voice sandbox remote");
}

#[test]
fn modes_off_flags_are_hidden() {
    let t = Tmp::new();
    t.write(
        ".claude/settings.json",
        r#"{"voice":{"enabled":false},"sandbox":{"enabled":true},"remoteControlAtStartup":false}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "sandbox");
}

#[test]
fn modes_reads_the_legacy_voice_key_unless_the_object_sets_it() {
    let t = Tmp::new();
    t.write(".claude/settings.json", r#"{"voiceEnabled":true}"#);
    assert_eq!(go(&t, CONF).trim(), "voice");
    t.write(
        ".claude/settings.json",
        r#"{"voiceEnabled":true,"voice":{"enabled":false}}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "");
}

#[test]
fn modes_project_settings_override_user_and_local_overrides_project() {
    let t = Tmp::new();
    t.write(".claude/settings.json", r#"{"sandbox":{"enabled":false}}"#);
    t.write(
        "proj/.claude/settings.json",
        r#"{"sandbox":{"enabled":true}}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "sandbox");
    t.write(
        "proj/.claude/settings.local.json",
        r#"{"sandbox":{"enabled":false}}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "");
}

#[test]
fn modes_show_selects_flags() {
    let t = Tmp::new();
    t.write(
        ".claude/settings.json",
        r#"{"voice":{"enabled":true},"sandbox":{"enabled":true}}"#,
    );
    let conf = "set left \"#{modes[show=sandbox,remote]}\"";
    assert_eq!(go(&t, conf).trim(), "sandbox");
}

#[test]
fn modes_ignore_malformed_settings() {
    let t = Tmp::new();
    t.write(".claude/settings.json", "not json");
    t.write(
        "proj/.claude/settings.json",
        r#"{"voice":{"enabled":"yes"},"sandbox":[1]}"#,
    );
    assert_eq!(go(&t, CONF).trim(), "");
}
