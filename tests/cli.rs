mod common;

use assert_cmd::Command;
use predicates::prelude::*;

fn bar() -> Command {
    Command::cargo_bin("bar").unwrap()
}

/// `bar render` with state, PATH and cwd confined to a temp dir.
fn isolated_render() -> (Command, common::tmp::Dir) {
    let home = common::tmp::Dir::new(&format!(
        "bar-cli-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let (iso, cwd) = common::isolation(&home);
    let mut c = bar();
    c.arg("render")
        .envs(iso)
        .current_dir(cwd)
        .env("COLUMNS", "200");
    (c, home)
}

#[test]
fn render_exits_zero() {
    let (mut c, _home) = isolated_render();
    c.write_stdin("").assert().success();
}

#[test]
fn help_lists_render_and_no_demo_commands() {
    bar()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("render"))
        .stdout(predicate::str::contains("hello").not())
        .stdout(predicate::str::contains("list").not());
}

#[test]
fn version_reports_bar() {
    bar()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("bar "));
}

#[test]
fn render_shows_cwd_dirname() {
    let (mut c, home) = isolated_render();
    let dir = home.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    c.write_stdin(common::workspace_json(&dir))
        .assert()
        .success()
        .stdout(predicate::str::contains("proj"));
}

#[test]
fn render_and_refresh_flags_still_reach_clap() {
    for args in [
        &["render", "--help"][..],
        &["refresh", "--help"],
        &["refresh"],
    ] {
        let out = bar().args(args).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("Usage: bar"), "{args:?}: {text}");
    }
}

#[cfg(feature = "edit")]
#[test]
fn edit_refuses_without_a_terminal() {
    let home = common::tmp::Dir::new(&format!("bar-cli-edit-{}", std::process::id()));
    let (iso, cwd) = common::isolation(&home);
    bar()
        .arg("edit")
        .envs(iso)
        .current_dir(cwd)
        .write_stdin("")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("bar edit: needs a terminal"));
    assert!(!home.join("xdg/bar/bar.conf").exists());
}
