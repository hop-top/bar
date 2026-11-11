//! State as data: `bar check --format json` names the state dir, the
//! toggles, the cache and the update check; `bar config export --format
//! json` gives the effective config.

// Unix fixtures (POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]
mod common;

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

const SID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";

struct Fx {
    root: common::tmp::Dir,
}

impl Fx {
    fn new(tag: &str) -> Fx {
        Fx {
            root: common::tmp::Dir::new(&format!(
                "bar-state-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            )),
        }
    }

    fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    fn conf(&self, text: &str) {
        let dir = self.root.join("xdg/bar");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bar.conf"), text).unwrap();
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let (iso, cwd) = common::isolation(&self.root);
        Command::cargo_bin("bar")
            .unwrap()
            .args(args)
            .envs(iso)
            .current_dir(cwd)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert_eq!(String::from_utf8_lossy(&out.stderr), "", "{args:?}");
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", String::from_utf8_lossy(&out.stdout)))
    }

    fn check_state(&self) -> Value {
        let v = self.json(&["check", "--format", "json"]);
        assert_eq!(v["ok"], true, "{v}");
        v["state"].clone()
    }
}

fn view<'a>(toggles: &'a Value, key: &str) -> &'a Value {
    &toggles
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["key"] == key)
        .unwrap_or_else(|| panic!("no toggle {key}: {toggles}"))["view"]
}

#[test]
fn check_reports_a_fresh_state() {
    let fx = Fx::new("fresh");
    let s = fx.check_state();
    assert_eq!(s["dir"], fx.state().display().to_string());
    assert_eq!(view(&s["toggles"], "5h"), "bar");
    assert_eq!(view(&s["toggles"], "7d"), "bar");
    assert_eq!(s["toggles"].as_array().unwrap().len(), 2, "{s}");
    assert_eq!(s["sessions"], serde_json::json!([]));
    assert_eq!(s["cache"]["entries"], 0);
    assert_eq!(s["cache"]["failing"], 0);
    let u = &s["update"];
    assert_eq!(u["check"], false);
    assert_eq!(u["status"], "off");
    assert_eq!(u["current"], env!("CARGO_PKG_VERSION"));
    assert_eq!(u["channel"], "stable");
    assert!(u["latest"].is_null(), "{u}");
    // reading the state writes nothing
    assert!(!fx.state().exists());
}

#[test]
fn check_reports_toggles() {
    let fx = Fx::new("toggles");
    for k in ["5h", "cliproxyapi"] {
        assert!(fx.run(&["toggle", k]).status.success());
    }
    assert!(fx
        .run(&["toggle", &format!("7d?sid={SID}")])
        .status
        .success());
    let s = fx.check_state();
    assert_eq!(view(&s["toggles"], "5h"), "pct");
    assert_eq!(view(&s["toggles"], "7d"), "bar");
    assert_eq!(view(&s["toggles"], "cliproxyapi"), "alt");
    let sessions = s["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{s}");
    assert_eq!(sessions[0]["session"], SID);
    assert_eq!(view(&sessions[0]["toggles"], "7d"), "pct");
}

fn store(state: &Path, key: &str, value: &str) {
    hop_bar::cache::Cache::new(state).store(key, value).unwrap();
}

#[test]
fn check_reports_the_update_check() {
    let fx = Fx::new("update");
    fx.conf("set update-check on\n");
    let u = &fx.check_state()["update"];
    assert_eq!(u["check"], true);
    assert_eq!(u["status"], "pending", "{u}");
    // check never starts the background check
    assert!(!fx.state().join("cache").exists());

    store(&fx.state(), "update:stable", "v9.9.9");
    store(&fx.state(), "some command", "x");
    let s = fx.check_state();
    let u = &s["update"];
    assert_eq!(u["status"], "ok", "{u}");
    assert_eq!(u["latest"], "v9.9.9");
    assert!(u["age_secs"].as_u64().is_some_and(|a| a < 600), "{u}");
    assert_eq!(u["stale"], false);
    assert_eq!(s["cache"]["entries"], 2);

    store(&fx.state(), "update:stable", "");
    assert_eq!(fx.check_state()["update"]["status"], "failed");
}

#[test]
fn check_keeps_its_fields() {
    let fx = Fx::new("fields");
    let v = fx.json(&["check", "--format", "json"]);
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["path", "project", "prices", "ok", "issues", "state"],
        "{v}"
    );
}

#[test]
fn export_json_is_the_effective_config_as_data() {
    let fx = Fx::new("export");
    let v = fx.json(&["config", "export", "--format", "json"]);
    assert!(v["user"].is_null(), "{v}");
    assert!(v["project"].is_null(), "{v}");
    let text = v["text"].as_str().unwrap();
    assert!(text.starts_with("# bar config: built-in layout\n"), "{v}");
    let lines = v["lines"].as_array().unwrap();
    assert!(lines
        .iter()
        .any(|l| l[0] == "set" && l[1] == "left" && l[2].is_string()));
    // comments are left out of `lines`
    assert!(lines.iter().all(|l| l[0] != "#"), "{v}");

    fx.conf("set left \"#{model}\"\n");
    let v = fx.json(&["config", "export", "--format", "json"]);
    let conf = fx.root.join("xdg/bar/bar.conf");
    assert_eq!(v["user"], conf.display().to_string());
    assert_eq!(v["lines"], serde_json::json!([["set", "left", "#{model}"]]));
    // the plain export is unchanged
    let out = fx.run(&["config", "export"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!(
            "# bar config: {}\nset left \"#{{model}}\"\n",
            conf.display()
        )
    );
}

#[test]
fn export_json_to_a_file_reports_the_write() {
    let fx = Fx::new("export-file");
    let f = fx.root.join("out.conf");
    let v = fx.json(&["config", "export", f.to_str().unwrap(), "--format", "json"]);
    assert_eq!(v["path"], f.display().to_string());
    assert_eq!(v["written"], true);
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(
        text.starts_with("# bar config: built-in layout\n"),
        "{text}"
    );
}
