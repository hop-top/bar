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
fn render_shows_cwd_dirname() {
    let home = std::env::temp_dir().join(format!("bar-cli-{}", std::process::id()));
    let dir = home.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    bar()
        .arg("render")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("COLUMNS", "200")
        .write_stdin(format!(
            r#"{{"workspace":{{"current_dir":"{}"}}}}"#,
            dir.display()
        ))
        .assert()
        .success()
        .stdout(predicate::str::contains("proj"));
    let _ = std::fs::remove_dir_all(&home);
}
