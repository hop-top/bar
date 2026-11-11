//! Fixtures for `bar tray state`: a granted echo plugin with a menu, a
//! second plugin dir, a user config and a state dir; `bar` runs against
//! them and its output is parsed by the tray's own reader.
#![allow(dead_code)]

use super::plugin_fx::{busy_retry, echo_info, install, Fx};
use bar_tray::model::snapshot::{self, Snapshot};
use hop_bar::tray::SNAPSHOT_FILE;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Echo with the `menu` capability and the `use` (two tokens) and
/// `reset` (no tokens, confirm) actions declared.
pub fn menu_ext(extra: Value) -> String {
    let mut v: Value = serde_json::from_str(&echo_info(json!({
        "capabilities": ["placeholder", "menu"],
        "actions": {
            "use": {"description": "Prefer", "tokens": ["account", "provider"]},
            "reset": {"description": "Reset", "tokens": [], "confirm": true}
        }
    })))
    .unwrap();
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v.to_string()
}

pub fn item(id: &str, label: &str) -> Value {
    json!({"id": id, "label": label, "action": "use",
           "tokens": {"account": id, "provider": "claude"}})
}

pub fn menu_of(groups: Value) -> String {
    json!({ "groups": groups }).to_string()
}

/// A full menu: a radio picker, a confirm button, an info row.
pub fn good_menu() -> String {
    menu_of(json!([
        {"id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
            {"id": "3", "label": "alice", "detail": "5h 59%", "checked": true,
             "action": "use", "tokens": {"account": "3", "provider": "claude"}},
            item("4", "bob")]},
        {"id": "more", "kind": "buttons", "items": [
            {"id": "reset", "label": "Reset", "action": "reset"}]},
        {"id": "info", "kind": "info", "items": [
            {"id": "auth", "label": "management refused the key", "style": "bad"}]}
    ]))
}

/// Fixture plugins, a user config, a state dir; `bar` runs against them.
pub struct T {
    pub fx: Fx,
    /// A second plugin dir: plugins with their own control files.
    pub bin2: PathBuf,
    pub conf: PathBuf,
}

impl T {
    pub fn new(conf: &str) -> T {
        let fx = Fx::new();
        let bin2 = fx.root.join("bin2");
        std::fs::create_dir_all(&bin2).unwrap();
        let dir = fx.root.join("xdg").join("bar");
        std::fs::create_dir_all(&dir).unwrap();
        let t = T {
            fx,
            bin2,
            conf: dir.join("bar.conf"),
        };
        t.config(conf);
        t
    }

    pub fn config(&self, conf: &str) {
        std::fs::write(&self.conf, conf).unwrap();
    }

    /// A copy of the echo fixture as `bar-<name>` in the second dir, with
    /// `ext.json` and `menu.json` of its own.
    pub fn clone_echo(&self, name: &str, ext: &str, menu: &str) {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/bar-echo");
        install(&src, &self.bin2.join(format!("bar-{name}")));
        std::fs::write(self.bin2.join("ext.json"), ext).unwrap();
        std::fs::write(self.bin2.join("menu.json"), menu).unwrap();
    }

    pub fn state(&self) -> &Path {
        &self.fx.state
    }

    pub fn bar(&self) -> Command {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let dirs = [self.fx.bin.clone(), self.bin2.clone()]
            .into_iter()
            .chain(std::env::split_paths(&inherited));
        let mut c = Command::new(assert_cmd::cargo::cargo_bin("bar"));
        c.env("PATH", std::env::join_paths(dirs).unwrap())
            .env("BAR_STATE_DIR", self.state())
            .env("HOME", &self.fx.root)
            .env("XDG_CONFIG_HOME", self.fx.root.join("xdg"))
            .env("XDG_STATE_HOME", self.fx.root.join("xdg-state"))
            .env("CLAUDE_CONFIG_DIR", self.fx.root.join("claude"));
        c
    }

    pub fn run(&self) -> Output {
        let mut c = self.bar();
        c.args(["tray", "state", "--json"]);
        busy_retry(|| c.output())
    }

    /// `bar tray state --json`: exit 0, stdout parsed by the tray.
    pub fn snapshot(&self) -> (Snapshot, Value) {
        let out = self.run();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        let parsed = snapshot::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        (parsed, serde_json::from_str(&text).unwrap())
    }

    pub fn file(&self) -> String {
        std::fs::read_to_string(self.state().join(SNAPSHOT_FILE)).unwrap()
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn plugin<'a>(s: &'a Snapshot, name: &str) -> &'a snapshot::Plugin {
    s.plugins.iter().find(|p| p.name == name).unwrap()
}
