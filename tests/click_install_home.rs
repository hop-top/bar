//! `bar click install` refuses to write without an absolute claude dir.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use render_fx::Tmp;

/// `bar click install --hooks-only` from a neutral cwd under `t`.
fn hooks_only_cli(t: &Tmp, home: Option<&str>, claude: Option<&str>) -> std::process::Output {
    let cwd = t.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut c = Command::cargo_bin("bar").unwrap();
    c.args(["click", "install", "--hooks-only"])
        .current_dir(&cwd)
        .env("BAR_STATE_DIR", t.path().join("state"))
        .env("BAR_APP_DIR", t.path().join("apps"))
        .env("XDG_CONFIG_HOME", t.path().join("cfg"))
        .env("XDG_STATE_HOME", t.path().join("st"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("HOME");
    if let Some(h) = home {
        c.env("HOME", h);
    }
    if let Some(d) = claude {
        c.env("CLAUDE_CONFIG_DIR", d);
    }
    c.output().unwrap()
}

#[test]
fn hooks_only_refuses_without_absolute_home() {
    for home in [None, Some(""), Some("rel/home")] {
        let t = Tmp::new();
        let o = hooks_only_cli(&t, home, None);
        assert_eq!(o.status.code(), Some(2), "{home:?}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("HOME"), "{home:?}: {err}");
        let cwd = t.path().join("cwd");
        assert!(!cwd.join(".claude").exists() && !cwd.join("rel").exists());
    }
}

#[test]
fn hooks_only_ignores_relative_claude_config_dir() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let o = hooks_only_cli(&t, home.to_str(), Some("relclaude"));
    assert_eq!(o.status.code(), Some(2));
    assert!(!t.path().join("cwd/relclaude").exists());
    assert!(!home.join(".claude").exists());
    // an absolute one is used
    let abs = t.path().join("claude");
    let o = hooks_only_cli(&t, home.to_str(), abs.to_str());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(abs.join("settings.json").is_file());
}
