//! `--format` on the CLI subcommands: one document on stdout (the result,
//! or the error envelope), and exit codes by error class.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use render_fx::Tmp;
use serde_json::Value;

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }
    /// Stdout is one JSON document and stderr holds none.
    fn disciplined(&self) -> Value {
        let v = self.json();
        let err = self.err.trim();
        assert!(
            err.is_empty() || serde_json::from_str::<Value>(err).is_err(),
            "stderr is JSON: {err}"
        );
        v
    }
}

/// `bar <args>` with every config, state and PATH location under `t`.
fn bar(t: &Tmp, args: &[&str], stdin: &str) -> Run {
    let (iso, cwd) = common::isolation(t.path());
    let o = Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .args(args)
        .write_stdin(stdin)
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// The error envelope: kit's field names, the code and exit code agreeing.
fn envelope(r: &Run, code: &str, exit: i32) -> Value {
    assert_eq!(r.code, exit, "{} {}", r.out, r.err);
    let v = r.disciplined();
    assert_eq!(v["code"], code, "{v}");
    assert_eq!(v["exit_code"], exit, "{v}");
    assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()), "{v}");
    assert!(v["transience"].is_string(), "{v}");
    v
}

#[test]
fn unknown_command_is_a_usage_error_with_a_fix() {
    let t = Tmp::new();
    let r = bar(&t, &["frobnicate"], "");
    assert_eq!(r.code, 2);
    assert_eq!(r.out, "");
    assert!(
        r.err.contains("unrecognized command `frobnicate`"),
        "{}",
        r.err
    );
    assert!(r.err.contains("see `bar --help`"), "{}", r.err);
    for args in [
        &["frobnicate", "--format", "json"][..],
        &["frobnicate", "--format=json"],
        &["--format", "json", "frobnicate"],
    ] {
        let r = bar(&t, args, "");
        let v = envelope(&r, "USAGE", 2);
        assert!(
            v["suggested_fix"]
                .as_str()
                .is_some_and(|f| f.contains("bar --help")),
            "{args:?}: {v}"
        );
        assert!(v["message"].as_str().unwrap().contains("frobnicate"), "{v}");
    }
}

#[test]
fn a_clap_usage_error_follows_the_format() {
    let t = Tmp::new();
    let r = bar(&t, &["config", "import", "-"], "");
    assert_eq!(r.code, 2);
    assert!(
        r.err.contains("required arguments were not provided"),
        "{}",
        r.err
    );
    let r = bar(&t, &["config", "import", "-", "--format", "json"], "");
    let v = envelope(&r, "USAGE", 2);
    assert!(
        v["suggested_fix"].as_str().unwrap().contains("--help"),
        "{v}"
    );
}

#[test]
fn an_unknown_format_is_refused_before_anything_runs() {
    let t = Tmp::new();
    let r = bar(&t, &["init", "--preset", "minimal", "--format", "xml"], "");
    assert_eq!(r.code, 2);
    assert!(
        r.err.contains("valid: csv, json, table, text, yaml"),
        "{}",
        r.err
    );
    assert!(!t.path().join("xdg/bar/bar.conf").exists());
}

#[test]
fn init_reports_as_data_and_an_existing_config_is_a_conflict() {
    let t = Tmp::new();
    let r = bar(&t, &["init", "--preset", "minimal", "--format", "json"], "");
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert_eq!(v["preset"], "minimal");
    assert!(v["path"].as_str().unwrap().ends_with("bar.conf"), "{v}");

    let r = bar(&t, &["init", "--preset", "full"], "");
    assert_eq!(r.code, 4);
    assert!(r.err.contains("exists; not overwritten"), "{}", r.err);
    let r = bar(&t, &["init", "--preset", "full", "--format", "json"], "");
    let v = envelope(&r, "CONFLICT", 4);
    assert_eq!(v["transience"], "permanent");
}

#[test]
fn import_without_yes_is_consent_refused() {
    let t = Tmp::new();
    let conf = "set left \"#{model}\"\n";
    let r = bar(&t, &["config", "import", "-", "--replace"], conf);
    assert_eq!(r.code, 7);
    assert!(r.out.contains("+set left"), "{}", r.out);
    assert!(r.err.contains("needs --yes"), "{}", r.err);
    let r = bar(
        &t,
        &["config", "import", "-", "--replace", "--format", "json"],
        conf,
    );
    let v = envelope(&r, "CONSENT_REFUSED", 7);
    assert_eq!(v["transience"], "transient");
    assert!(
        v["suggested_fix"].as_str().unwrap().contains("--yes"),
        "{v}"
    );
    assert!(!t.path().join("xdg/bar/bar.conf").exists());
}

#[test]
fn import_reports_as_data() {
    let t = Tmp::new();
    let conf = "set left \"#{model}\"\n";
    let args = [
        "config",
        "import",
        "-",
        "--replace",
        "--yes",
        "--format",
        "json",
    ];
    let r = bar(&t, &args, conf);
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert_eq!(v["written"], true, "{v}");
    assert!(v["path"].as_str().unwrap().ends_with("bar.conf"), "{v}");
    let r = bar(&t, &args, conf);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.disciplined()["written"], false);
    // a broken file: usage error, the report in the envelope
    let r = bar(&t, &args, "set no-such-key 1\n");
    let v = envelope(&r, "USAGE", 2);
    assert!(
        v["message"].as_str().unwrap().contains("nothing written"),
        "{v}"
    );
    assert!(v["cause"].as_str().unwrap().contains("line 1"), "{v}");
}

#[test]
fn import_of_a_missing_file_is_not_found() {
    let t = Tmp::new();
    let r = bar(&t, &["config", "import", "nope.conf", "--merge"], "");
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(r.err.contains("nope.conf"), "{}", r.err);
}

#[test]
fn export_reports_as_data() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\n");
    let r = bar(&t, &["config", "export", "--format", "json"], "");
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert!(v["config"].as_str().unwrap().contains("set gap 4"), "{v}");
    assert!(v["user"].as_str().unwrap().ends_with("bar.conf"), "{v}");
    assert_eq!(v["project"], Value::Null);
    // an existing output file without --force is a conflict
    t.write("out.conf", "x\n");
    let out = t.path().join("out.conf");
    let r = bar(&t, &["config", "export", out.to_str().unwrap()], "");
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("pass --force"), "{}", r.err);
}

#[test]
fn export_user_without_a_config_is_not_found() {
    let t = Tmp::new();
    let r = bar(&t, &["config", "export", "--user", "--format", "json"], "");
    envelope(&r, "NOT_FOUND", 3);
}

#[test]
fn install_preview_is_consent_refused_and_reports_as_data() {
    let t = Tmp::new();
    let r = bar(&t, &["install", "--project"], "");
    assert_eq!(r.code, 7, "{}", r.err);
    assert!(r.err.contains("rerun with --yes to write"), "{}", r.err);
    let r = bar(&t, &["install", "--local", "--format", "json"], "");
    let v = envelope(&r, "CONSENT_REFUSED", 7);
    assert!(
        v["suggested_fix"].as_str().unwrap().contains("--yes"),
        "{v}"
    );
    assert!(
        r.err.contains("statusLine"),
        "the diff goes to stderr: {}",
        r.err
    );

    let args = ["install", "--project", "--yes", "--format", "json"];
    let r = bar(&t, &args, "");
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert_eq!(v["written"], true, "{v}");
    assert!(
        v["settings"].as_str().unwrap().ends_with("settings.json"),
        "{v}"
    );
    let r = bar(&t, &args, "");
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.disciplined()["written"], false);
}

#[test]
fn toggle_reports_as_data() {
    let t = Tmp::new();
    let r = bar(&t, &["toggle", "7d", "--format", "json"], "");
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert_eq!(v["key"], "7d");
    assert_eq!(v["view"], "pct");
    let r = bar(&t, &["toggle", "7d?sid=../x", "--format", "json"], "");
    envelope(&r, "USAGE", 2);
}

#[test]
fn update_reports_as_data() {
    let t = Tmp::new();
    let r = bar(&t, &["update", "--format", "json"], "");
    assert_eq!(r.code, 0, "{}", r.err);
    let v = r.disciplined();
    assert_eq!(v["current"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["check"], false);
    assert_eq!(v["status"], "off");
    assert_eq!(v["age_secs"], Value::Null);
    assert_eq!(v["stale"], false);
    assert!(v["upgrade"].is_string(), "{v}");
}

#[test]
fn a_missing_plugin_is_not_found() {
    let t = Tmp::new();
    let r = bar(&t, &["plugin", "info", "no-such-plugin"], "");
    assert_eq!(r.code, 3);
    assert!(r.err.contains("no plugin `no-such-plugin`"), "{}", r.err);
    let r = bar(
        &t,
        &["plugin", "info", "no-such-plugin", "--format", "json"],
        "",
    );
    envelope(&r, "NOT_FOUND", 3);
}

#[test]
fn text_output_is_unchanged_by_default() {
    let t = Tmp::new();
    let r = bar(&t, &["init", "--preset", "minimal"], "");
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with("wrote "), "{}", r.out);
    assert!(r.out.ends_with("(preset minimal)\n"), "{}", r.out);
    let r = bar(&t, &["toggle", "5h"], "");
    assert_eq!(r.out, "5h: pct\n");
}
