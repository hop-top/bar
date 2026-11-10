// `bar <name> <args>` runs the `bar-<name>` plugin.
#![cfg(unix)]

mod common;

use assert_cmd::Command;
use std::os::unix::fs::PermissionsExt;

fn script(dir: &std::path::Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn bar(home: &common::tmp::Dir, bin: &std::path::Path) -> Command {
    let (iso, cwd) = common::isolation(home);
    let mut c = Command::cargo_bin("bar").unwrap();
    c.envs(iso)
        .current_dir(cwd)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    c
}

fn setup(name: &str) -> (common::tmp::Dir, std::path::PathBuf) {
    let home = common::tmp::Dir::new(&format!("bar-pcmd-{}-{name}", std::process::id()));
    let bin = home.path().join("plugins");
    std::fs::create_dir_all(&bin).unwrap();
    (home, bin)
}

#[test]
fn a_plugin_name_runs_the_plugin_with_the_rest_and_its_exit_code() {
    let (home, bin) = setup("run");
    script(
        &bin,
        "bar-greet",
        r#"echo "args:$*"; echo oops >&2; exit 7"#,
    );
    let out = bar(&home, &bin)
        .args(["greet", "list", "--provider", "claude"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "args:list --provider claude\n"
    );
    assert_eq!(String::from_utf8_lossy(&out.stderr), "oops\n");
}

#[test]
fn stdin_reaches_the_plugin() {
    let (home, bin) = setup("stdin");
    script(&bin, "bar-cat", "cat");
    bar(&home, &bin)
        .arg("cat")
        .write_stdin("hello")
        .assert()
        .success()
        .stdout("hello");
}

#[test]
fn a_builtin_wins_over_a_plugin_of_the_same_name() {
    let (home, bin) = setup("collide");
    script(&bin, "bar-check", "echo PLUGIN; exit 9");
    script(&bin, "bar-plugin", "echo PLUGIN; exit 9");
    let out = bar(&home, &bin).arg("check").output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("PLUGIN"));
    assert_ne!(out.status.code(), Some(9));
    let out = bar(&home, &bin).args(["plugin", "list"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("PLUGIN"));
}

#[test]
fn an_unknown_name_is_a_usage_error() {
    let (home, bin) = setup("unknown");
    let out = bar(&home, &bin).arg("nosuchthing").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nosuchthing"));
}

/// A plugin whose `--ext-info` declares `aliases`, answering other calls
/// with its own name.
fn aliased(bin: &std::path::Path, name: &str, aliases: &str) {
    script(
        bin,
        &format!("bar-{name}"),
        &format!(
            r#"if [ "$1" = "--ext-info" ]; then
  echo '{{"name":"{name}","version":"1","capabilities":[],"aliases":{aliases}}}'; exit 0; fi
echo "{name}:$*""#
        ),
    );
}

#[test]
fn a_declared_alias_runs_its_plugin() {
    let (home, bin) = setup("alias");
    aliased(&bin, "cliproxyapi", r#"["cpa","proxy-x"]"#);
    let out = bar(&home, &bin)
        .args(["cpa", "list", "-x"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "cliproxyapi:list -x\n"
    );
    let out = bar(&home, &bin).args(["proxy-x"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "cliproxyapi:\n");
    // not declared: still unknown
    let out = bar(&home, &bin).arg("cpb").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_real_plugin_wins_over_an_alias() {
    let (home, bin) = setup("alias-real");
    aliased(&bin, "cliproxyapi", r#"["cpa"]"#);
    script(&bin, "bar-cpa", "echo REAL");
    let out = bar(&home, &bin).arg("cpa").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "REAL\n");
}

#[test]
fn a_builtin_wins_over_an_alias() {
    let (home, bin) = setup("alias-builtin");
    aliased(&bin, "thief", r#"["check","plugin"]"#);
    let out = bar(&home, &bin).arg("check").output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("thief:"));
    let out = bar(&home, &bin).args(["plugin", "list"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("thief:"));
}

#[test]
fn an_alias_two_plugins_declare_is_an_error_naming_both() {
    let (home, bin) = setup("alias-ambiguous");
    aliased(&bin, "one", r#"["dup"]"#);
    aliased(&bin, "two", r#"["dup"]"#);
    let out = bar(&home, &bin).arg("dup").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("one") && e.contains("two"), "{e}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains(":"));
}

#[test]
fn invalid_aliases_are_dropped_and_the_list_is_short() {
    use hop_bar::plugin::parse_info;
    let i = parse_info(
        r#"{"name":"p","version":"1","aliases":["ok","Bad","a b","","waytoolongaliasname123","ok","b","c","d","e"]}"#,
    )
    .unwrap();
    assert_eq!(i.aliases, ["ok", "b", "c", "d"]);
}
