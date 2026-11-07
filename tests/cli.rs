use assert_cmd::Command;
use predicates::prelude::*;

fn bar() -> Command {
    Command::cargo_bin("bar").unwrap()
}

#[test]
fn render_exits_zero() {
    bar().arg("render").write_stdin("").assert().success();
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
fn render_prints_cwd_basename() {
    bar()
        .arg("render")
        .write_stdin(r#"{"workspace":{"current_dir":"/work/proj"}}"#)
        .assert()
        .success()
        .stdout("proj\n");
}
