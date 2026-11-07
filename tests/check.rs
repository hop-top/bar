mod common;

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// Temp HOME/XDG tree; PATH is a private `bin` plus the few tools plugins need.
struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let root = std::env::temp_dir().join(format!(
            "bar-check-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("xdg/bar")).unwrap();
        std::fs::create_dir_all(root.join("neutral")).unwrap();
        // the fixture plugins need these; linking only them keeps any
        // system `gh` off PATH
        std::fs::create_dir_all(root.join("sys")).unwrap();
        for tool in ["cat", "dirname"] {
            let real = ["/bin", "/usr/bin"]
                .iter()
                .map(|d| Path::new(d).join(tool))
                .find(|p| p.exists())
                .unwrap();
            std::os::unix::fs::symlink(real, root.join("sys").join(tool)).unwrap();
        }
        Fx { root }
    }

    fn conf(&self, body: &str) -> &Fx {
        std::fs::write(self.root.join("xdg/bar/bar.conf"), body).unwrap();
        self
    }

    fn exe(&self, name: &str, body: &str) -> &Fx {
        let p = self.root.join("bin").join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        self
    }

    fn plugin(&self, name: &str) -> &Fx {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins");
        self.exe(name, &std::fs::read_to_string(src.join(name)).unwrap())
    }

    fn path(&self) -> std::ffi::OsString {
        std::env::join_paths([self.root.join("bin"), self.root.join("sys")]).unwrap()
    }

    fn bar(&self, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("bar").unwrap();
        c.args(args)
            .current_dir(self.root.join("neutral"))
            .env_clear()
            .env("PATH", self.path())
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("xdg"))
            .env("BAR_STATE_DIR", self.root.join("state"))
            .env("XDG_STATE_HOME", self.root.join("xdg-state"));
        c
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn out(c: &mut Command) -> (i32, String) {
    let o = c.output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

#[test]
fn check_reports_line_numbers() {
    let fx = Fx::new();
    fx.conf("# comment\nset gap 3\nset wrap sideways\nbogus thing\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 2);
    assert!(stdout.contains("line 3"), "{stdout}");
    assert!(stdout.contains("invalid wrap"), "{stdout}");
    assert!(stdout.contains("line 4"), "{stdout}");
    assert!(stdout.contains("unknown directive"), "{stdout}");
}

#[test]
fn check_flags_missing_gh() {
    let fx = Fx::new();
    fx.conf("set left \"#{repo} #{prs}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 0, "a warning is not a config error: {stdout}");
    assert!(stdout.contains("gh"), "{stdout}");
    assert!(stdout.contains("prs"), "{stdout}");

    fx.exe("gh", "#!/bin/sh\nexit 0\n");
    let (_, stdout) = out(&mut fx.bar(&["check"]));
    assert!(!stdout.contains("`gh`"), "gh present: {stdout}");
}

#[test]
fn check_flags_unknown_placeholder_and_unknown_option() {
    let fx = Fx::new();
    fx.plugin("bar-echo");
    fx.conf(concat!(
        "set left \"#{nosuch} #{model[color=red]} #{echo[limit=2]}\"\n",
        "set center \"#{echo[bogus=1]} #{echo[limit=many]} #{usage[type=9z]}\"\n",
    ));
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 2, "{stdout}");
    assert!(stdout.contains("unknown placeholder `nosuch`"), "{stdout}");
    assert!(stdout.contains("unknown option `color`"), "{stdout}");
    assert!(stdout.contains("unknown option `bogus`"), "{stdout}");
    assert!(stdout.contains("invalid value `many`"), "{stdout}");
    assert!(stdout.contains("invalid value `9z`"), "{stdout}");
    assert!(!stdout.contains("`echo[limit=2]`"), "{stdout}");
    assert_eq!(stdout.matches("line 1").count(), 2, "{stdout}");
}

#[test]
fn default_for_absent_plugin_warns_but_layout_use_errors() {
    let fx = Fx::new();
    fx.conf("default nosuch limit=2\nset left \"#{model}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("line 1: warning:"), "{stdout}");
    assert!(stdout.contains("`nosuch`"), "{stdout}");
    assert!(!stdout.contains("error:"), "{stdout}");
    fx.conf("default nosuch limit=2\nset left \"#{nosuch}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 2, "{stdout}");
    assert!(
        stdout.contains("line 2: error: unknown placeholder `nosuch`"),
        "{stdout}"
    );
    assert!(stdout.contains("line 1: warning:"), "{stdout}");
}

#[test]
fn check_flags_multi_query_cost() {
    let fx = Fx::new();
    fx.exe("gh", "#!/bin/sh\n");
    let many = "(a|b|c)&(d|e|f)";
    fx.conf(&format!(
        "set left \"#{{prs[author={many}]}} #{{issues[assignee=@me]}}\"\n"
    ));
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("9 queries"), "{stdout}");
    assert!(stdout.contains("only the first 8"), "{stdout}");
    assert!(!stdout.contains("issues:"), "{stdout}");
}

#[test]
fn check_flags_invalid_expression() {
    let fx = Fx::new();
    fx.exe("gh", "#!/bin/sh\n");
    fx.conf("set left \"#{prs[author=(a|]} #{issues[limit=lots]}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 2, "{stdout}");
    assert!(stdout.contains("invalid expression"), "{stdout}");
    assert!(stdout.contains("`author`"), "{stdout}");
    assert!(stdout.contains("invalid value `lots`"), "{stdout}");
}

#[test]
fn check_exit_2_on_config_error() {
    let fx = Fx::new();
    fx.conf("set gap many\n");
    fx.bar(&["check"]).assert().code(2);
    fx.conf("set gap 4\nset left \"#{model}\"\n");
    fx.bar(&["check"]).assert().code(0);
    let (code, json) = out(&mut fx
        .conf("set gap many\n")
        .bar(&["check", "--format", "json"]));
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["issues"][0]["line"], 1);
    assert_eq!(v["issues"][0]["level"], "error");
}

#[test]
fn check_unreadable_config_has_no_line_number() {
    let fx = Fx::new();
    fx.conf("set gap 3\n");
    let p = fx.root.join("xdg/bar/bar.conf");
    std::fs::write(&p, [0xff, 0xfe, 0xfd]).unwrap();
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 2);
    assert!(stdout.contains("bar.conf"), "{stdout}");
    assert!(!stdout.contains("line 0"), "{stdout}");
}

#[test]
fn check_prints_resolved_config_path() {
    let fx = Fx::new();
    fx.conf("set gap 3\n");
    let want = fx.root.join("xdg/bar/bar.conf");
    fx.bar(&["check"])
        .assert()
        .success()
        .stdout(predicate::str::contains(want.display().to_string()));
    let (_, json) = out(&mut fx.bar(&["check", "--format", "json"]));
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["path"], want.display().to_string());
    std::fs::remove_file(&want).unwrap();
    fx.bar(&["check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("built-in"));
}

#[test]
fn preview_uses_only_sample_data() {
    let fx = Fx::new();
    let (iso, cwd) = common::isolation(&fx.root.join("iso"));
    // a real-looking account that preview must not read
    std::fs::write(
        fx.root.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"real@person.test"}}"#,
    )
    .unwrap();
    let mut c = fx.bar(&["preview"]);
    c.envs(iso)
        .current_dir(cwd)
        .env("ANTHROPIC_API_KEY", "sk-real")
        .env("COLUMNS", "200");
    let (code, stdout) = out(&mut c);
    assert_eq!(code, 0);
    assert!(stdout.contains("#128"), "{stdout}");
    assert!(stdout.contains("!42"), "{stdout}");
    assert!(!stdout.contains("real@person"), "{stdout}");
    assert!(!stdout.contains("api-key"), "{stdout}");
    assert!(
        !common::gh_log(&fx.root.join("iso")).exists(),
        "gh must not run"
    );
    assert!(!fx.root.join("state").exists(), "no state written");
}

#[test]
fn preview_final_shows_bad_on_unused() {
    let fx = Fx::new();
    let ramp_bad = "38;5;196m";
    let run = |state: &str| {
        let mut c = fx.bar(&["preview", "--state", state]);
        c.env("COLUMNS", "200");
        out(&mut c).1
    };
    assert!(run("final").contains(ramp_bad), "{}", run("final"));
    assert!(!run("early").contains(ramp_bad), "{}", run("early"));
    fx.bar(&["preview", "--state", "later"]).assert().code(2);
}

#[test]
fn check_flags_bad_usage_style_values() {
    let fx = Fx::new();
    fx.conf("set left \"#{usage[show=bar,pct,glyphs=dots,count=used,alt=pct,time]}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 0, "{stdout}");

    fx.conf("set left \"#{usage[show=eta,proj,time,alt=proj,eta,bar]}\"\n");
    let (code, stdout) = out(&mut fx.bar(&["check"]));
    assert_eq!(code, 0, "{stdout}");

    for bad in ["show=bar,zzz", "alt=time,zzz", "glyphs=zzz", "count=zzz"] {
        fx.conf(&format!("set left \"#{{usage[{bad}]}}\"\n"));
        let (code, stdout) = out(&mut fx.bar(&["check"]));
        assert_eq!(code, 2, "{bad}: {stdout}");
        assert!(stdout.contains("invalid value"), "{bad}: {stdout}");
    }
}
