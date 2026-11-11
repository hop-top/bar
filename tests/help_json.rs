//! `bar --help --format json`: the command tree as data, each command with
//! its side-effect class and idempotency.

use assert_cmd::Command;
use serde_json::Value;

fn help(args: &[&str]) -> (Value, String) {
    let out = Command::cargo_bin("bar")
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let v = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{args:?}: {e}: {stdout}"));
    (v, String::from_utf8_lossy(&out.stderr).to_string())
}

fn command<'a>(doc: &'a Value, path: &[&str]) -> &'a Value {
    doc["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == serde_json::json!(path))
        .unwrap_or_else(|| panic!("no command {path:?}"))
}

#[test]
fn help_json_lists_every_command_with_its_contract() {
    let (doc, stderr) = help(&["--help", "--format", "json"]);
    assert_eq!(stderr, "");
    assert_eq!(doc["tool"], "bar");
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
    let cmds = doc["commands"].as_array().unwrap();
    assert!(cmds.len() > 10, "{doc}");
    for c in cmds {
        let se = c["side_effect"].as_str().unwrap();
        assert!(
            [
                "read",
                "write",
                "write-local",
                "write-shared",
                "interactive"
            ]
            .contains(&se),
            "{c}"
        );
        let id = c["idempotent"].as_str().unwrap();
        assert!(["yes", "no", "conditional"].contains(&id), "{c}");
        assert!(c["short"].as_str().is_some_and(|s| !s.is_empty()), "{c}");
    }
}

#[test]
fn help_json_declares_known_commands() {
    let (doc, _) = help(&["--help", "--format", "json"]);
    let check = command(&doc, &["bar", "check"]);
    assert_eq!(check["side_effect"], "read");
    assert_eq!(check["idempotent"], "yes");
    let import = command(&doc, &["bar", "config", "import"]);
    assert_eq!(import["side_effect"], "write-local");
    let arg = &import["args"][0];
    assert_eq!(arg["name"], "file");
    assert_eq!(arg["required"], true);
    let flags = import["flags"].as_array().unwrap();
    assert!(flags.iter().any(|f| f["name"] == "replace"), "{import}");
    let toggle = command(&doc, &["bar", "toggle"]);
    assert_eq!(toggle["side_effect"], "write-local");
    assert_eq!(check["dry_run_supported"], false);
    let click = command(&doc, &["bar", "click", "install"]);
    assert_eq!(click["dry_run_supported"], true);
    assert_eq!(command(&doc, &["bar", "refresh"])["hidden"], true);
}

#[test]
fn help_json_toggle_takes_an_optional_value() {
    let (doc, _) = help(&["--help", "--format", "json"]);
    let toggle = command(&doc, &["bar", "toggle"]);
    // the set form is idempotent, the flip is not
    assert_eq!(toggle["idempotent"], "conditional");
    let args = toggle["args"].as_array().unwrap();
    assert_eq!(args[0]["name"], "key");
    assert_eq!(args[0]["required"], true);
    assert_eq!(args[1]["name"], "value");
    assert_eq!(args[1]["required"], false);
}

#[test]
fn help_json_flags_carry_choices_and_defaults() {
    let (doc, _) = help(&["--help", "--format", "json"]);
    let init = command(&doc, &["bar", "init"]);
    let preset = init["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "preset")
        .unwrap();
    assert!(preset["choices"].as_array().unwrap().len() > 1, "{preset}");
    assert!(preset["default"].is_string(), "{preset}");
    let install = command(&doc, &["bar", "install"]);
    let yes = install["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "yes")
        .unwrap();
    assert_eq!(yes["short"], "y");
    assert_eq!(yes["type"], "bool");
}

#[test]
fn help_json_of_a_subcommand_lists_its_commands() {
    let (doc, _) = help(&["config", "--help", "--format=json"]);
    let paths: Vec<&Value> = doc["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["path"])
        .collect();
    assert_eq!(
        paths,
        [
            &serde_json::json!(["bar", "config", "export"]),
            &serde_json::json!(["bar", "config", "import"])
        ]
    );
}

#[test]
fn help_yaml_is_data_too() {
    let out = Command::cargo_bin("bar")
        .unwrap()
        .args(["-h", "--format", "yaml"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("side_effect: read"), "{text}");
}

#[test]
fn plain_help_is_unchanged() {
    for args in [&["--help"][..], &["--help", "--format", "table"]] {
        let out = Command::cargo_bin("bar")
            .unwrap()
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("Fast, configurable"), "{args:?}: {text}");
    }
}
