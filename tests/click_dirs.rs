//! The `bar://` handler uses the dirs of the environment that installed it,
//! and a pointer from the renderer keeps the two in step.

// Unix fixtures (shell, symlinks, POSIX paths); the handler is macOS-only
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::click::{
    applescript, handler_env, handler_warnings, pointer, run_install, shell_command, EnvVar,
    HandlerDirs, InstallCfg, HANDLER_ENV,
};
use render_fx::Tmp;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const ODD: &str = "st ate \"dq\" it's $HOME `x` \\ end";

fn bar() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bar"))
}

fn env_of(pairs: &[(&str, &str)]) -> Vec<EnvVar> {
    handler_env(&|k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    })
}

#[test]
fn handler_env_covers_every_dir_variable() {
    for k in [
        "BAR_STATE_DIR",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "CLAUDE_CONFIG_DIR",
        "TMUX_TMPDIR",
    ] {
        assert!(HANDLER_ENV.contains(&k), "{k}");
    }
    let env = env_of(&[("XDG_STATE_HOME", "/s"), ("BAR_STATE_DIR", "")]);
    assert_eq!(env.len(), HANDLER_ENV.len());
    let get = |k: &str| env.iter().find(|(n, _)| n == k).unwrap().1.clone();
    assert_eq!(get("XDG_STATE_HOME").as_deref(), Some("/s"));
    // empty counts as unset, as bar's dir resolution treats it
    assert_eq!(get("BAR_STATE_DIR"), None);
    assert_eq!(get("CLAUDE_CONFIG_DIR"), None);
}

#[test]
fn shell_command_sets_and_unsets() {
    let env = env_of(&[("XDG_STATE_HOME", "/x y"), ("CLAUDE_CONFIG_DIR", "/c")]);
    let cmd = shell_command(Path::new("/opt/b ar/bar"), &env);
    assert!(cmd.starts_with("unset "), "{cmd}");
    assert!(cmd.contains("BAR_STATE_DIR"), "{cmd}");
    assert!(cmd.contains("XDG_STATE_HOME='/x y' "), "{cmd}");
    assert!(cmd.contains("CLAUDE_CONFIG_DIR='/c' "), "{cmd}");
    assert!(cmd.ends_with("'/opt/b ar/bar'"), "{cmd}");
}

#[test]
fn applescript_carries_the_env() {
    let env = env_of(&[("XDG_STATE_HOME", "/s \"q\"")]);
    let s = applescript(Path::new("/opt/bar"), &env);
    assert!(s.contains("XDG_STATE_HOME='/s \\\"q\\\"'"), "{s}");
    assert!(s.contains("do shell script theBar & \" toggle \""), "{s}");
    assert!(
        s.contains("do shell script theBar & \" click run \""),
        "{s}"
    );
}

/// `sh -c` with a nearly empty environment, as LaunchServices starts the
/// handler: only `HOME` and a minimal `PATH`, plus `outer`.
fn sh(cmd: &str, home: &Path, outer: &[(&str, &Path)]) -> std::process::Output {
    let mut c = std::process::Command::new("/bin/sh");
    c.args(["-c", cmd])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin");
    for (k, v) in outer {
        c.env(k, v);
    }
    c.output().unwrap()
}

#[test]
fn toggle_lands_in_the_baked_state_dir() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let state = t.path().join(ODD);
    let env = env_of(&[("BAR_STATE_DIR", state.to_str().unwrap())]);
    let cmd = format!("{} toggle 5h", shell_command(&bar(), &env));
    let o = sh(&cmd, &home, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(state.join("pct-5h").is_file());
}

#[test]
fn toggle_lands_in_the_baked_xdg_state_home() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let xdg = t.path().join(ODD);
    let env = env_of(&[("XDG_STATE_HOME", xdg.to_str().unwrap())]);
    let cmd = format!("{} toggle tlc", shell_command(&bar(), &env));
    let o = sh(&cmd, &home, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(xdg.join("bar/toggle-tlc").is_file());
}

#[test]
fn unset_at_install_stays_unset() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let stray = t.path().join("stray");
    let env = env_of(&[]);
    let cmd = format!("{} toggle 7d", shell_command(&bar(), &env));
    let o = sh(
        &cmd,
        &home,
        &[("XDG_STATE_HOME", &stray), ("BAR_STATE_DIR", &stray)],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!stray.exists(), "launch env leaked into the handler");
}

/// The compiled handler, sent `open location` the way LaunchServices
/// delivers a click.
#[cfg(target_os = "macos")]
#[test]
fn compiled_handler_toggles_in_the_baked_dir() {
    if !Path::new("/usr/bin/osacompile").exists() {
        return;
    }
    let t = Tmp::new();
    let home = t.path().join("home");
    let state = t.path().join(ODD);
    let env = env_of(&[("XDG_STATE_HOME", state.to_str().unwrap())]);
    let src = t.path().join("h.applescript");
    let scpt = t.path().join("h.scpt");
    std::fs::write(&src, applescript(&bar(), &env)).unwrap();
    let ok = std::process::Command::new("/usr/bin/osacompile")
        .arg("-o")
        .arg(&scpt)
        .arg(&src)
        .status()
        .unwrap();
    assert!(ok.success());
    let o = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!(
            "set s to load script POSIX file \"{}\"",
            scpt.display()
        ))
        .arg("-e")
        .arg("tell s to open location \"bar://toggle/5h\"")
        .env_clear()
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(state.join("bar/pct-5h").is_file());
}

// ── Install record and drift warnings ──────────────────────────────────────

fn cfg(t: &Tmp, env: Vec<EnvVar>) -> InstallCfg {
    InstallCfg {
        bar: bar(),
        state_dir: t.path().join("state"),
        app: t.path().join("apps/bar-click.app"),
        settings: t.path().join("claude/settings.json"),
        env,
    }
}

fn installed(t: &Tmp, env: Vec<EnvVar>) -> InstallCfg {
    let c = cfg(t, env);
    let mut out = Vec::new();
    run_install(&c, true, false, &mut |_| Ok(()), &mut out).unwrap();
    c
}

#[test]
fn install_records_the_dirs_in_the_bundle() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[("BAR_STATE_DIR", "/s")]));
    let r = HandlerDirs::read(&c.app).unwrap();
    assert_eq!(r.state_dir, c.state_dir);
    assert_eq!(r.claude_dir, Some(t.path().join("claude")));
    assert!(!r.follows_pointer, "a set state var pins the handler");
    let c = installed(&t, env_of(&[]));
    assert!(HandlerDirs::read(&c.app).unwrap().follows_pointer);
}

#[test]
fn no_warning_when_dirs_match() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[("BAR_STATE_DIR", "/s")]));
    let w = handler_warnings(&c.app, &c.state_dir, Some(&t.path().join("claude")), None);
    assert!(w.is_empty(), "{w:?}");
    // no app: nothing to compare
    let w = handler_warnings(&t.path().join("none.app"), &c.state_dir, None, None);
    assert!(w.is_empty(), "{w:?}");
}

#[test]
fn warns_when_state_dir_differs() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[("BAR_STATE_DIR", "/s")]));
    let other = t.path().join("other");
    let w = handler_warnings(&c.app, &other, Some(&t.path().join("claude")), None);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("bar click install"), "{w:?}");
    assert!(w[0].contains(&other.display().to_string()), "{w:?}");
}

#[test]
fn warns_when_claude_dir_differs() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[("BAR_STATE_DIR", "/s")]));
    let w = handler_warnings(&c.app, &c.state_dir, Some(Path::new("/elsewhere")), None);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("/elsewhere") && w[0].contains("bar click install"));
}

#[test]
fn warns_when_the_renderer_uses_another_dir() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[("BAR_STATE_DIR", "/s")]));
    let render = t.path().join("render");
    let claude = t.path().join("claude");
    let w = handler_warnings(&c.app, &c.state_dir, Some(&claude), Some(&render));
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains(&render.display().to_string()), "{w:?}");
}

#[test]
fn a_following_handler_agrees_with_the_pointer() {
    let t = Tmp::new();
    let c = installed(&t, env_of(&[]));
    let render = t.path().join("render");
    let claude = t.path().join("claude");
    let w = handler_warnings(&c.app, &render, Some(&claude), Some(&render));
    assert!(w.is_empty(), "{w:?}");
}

#[test]
fn a_handler_without_a_record_warns() {
    let t = Tmp::new();
    let app = t.path().join("old.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    let w = handler_warnings(&app, t.path(), None, None);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("bar click install"));
}

// ── Pointer ────────────────────────────────────────────────────────────────

fn marked(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("click-installed"), "cmd").unwrap();
}

#[test]
fn sync_points_the_default_at_the_render_dir() {
    let t = Tmp::new();
    let default = t.path().join("default");
    let state = t.path().join("xdg/bar");
    let now = SystemTime::now();
    // no handler marker: nothing written
    assert!(!pointer::sync(&state, &default, now));
    assert!(!default.exists());
    marked(&state);
    assert!(pointer::sync(&state, &default, now));
    assert_eq!(pointer::read(&default), Some(state.clone()));
    // unchanged: no write
    assert!(!pointer::sync(&state, &default, now));
    // the follower picks it up, but only from the default dir
    assert_eq!(pointer::follow(default.clone(), &default), state);
    let pinned = t.path().join("pinned");
    assert_eq!(pointer::follow(pinned.clone(), &default), pinned);
}

#[test]
fn sync_from_the_default_dir_removes_the_pointer() {
    let t = Tmp::new();
    let default = t.path().join("default");
    let state = t.path().join("xdg/bar");
    marked(&state);
    let then = SystemTime::now();
    assert!(pointer::sync(&state, &default, then));
    let later = then + Duration::from_secs(3600);
    assert!(pointer::sync(&default, &default, later));
    assert_eq!(pointer::read(&default), None);
    assert_eq!(pointer::follow(default.clone(), &default), default);
}

#[test]
fn sync_throttles_a_pointer_changed_moments_ago() {
    let t = Tmp::new();
    let default = t.path().join("default");
    let (a, b) = (t.path().join("a"), t.path().join("b"));
    marked(&a);
    marked(&b);
    let now = SystemTime::now();
    assert!(pointer::sync(&a, &default, now));
    // two renderers with different dirs: no write per render
    assert!(!pointer::sync(&b, &default, now + Duration::from_secs(5)));
    assert_eq!(pointer::read(&default), Some(a.clone()));
    assert!(pointer::sync(&b, &default, now + Duration::from_secs(3600)));
    assert_eq!(pointer::read(&default), Some(b));
}

#[test]
fn a_pointer_to_a_dir_without_the_marker_is_ignored() {
    let t = Tmp::new();
    let default = t.path().join("default");
    let state = t.path().join("gone");
    marked(&state);
    assert!(pointer::sync(&state, &default, SystemTime::now()));
    std::fs::remove_file(state.join("click-installed")).unwrap();
    assert_eq!(pointer::read(&default), None);
    assert_eq!(pointer::follow(default.clone(), &default), default);
}

#[test]
fn relative_state_dir_is_never_pointed_at() {
    let t = Tmp::new();
    let default = t.path().join("default");
    assert!(!pointer::sync(
        Path::new("rel"),
        &default,
        SystemTime::now()
    ));
    assert!(!default.exists());
}

/// `bar toggle` with neither state variable set follows the pointer in
/// the platform default dir (macOS: the handler is macOS-only).
#[cfg(target_os = "macos")]
#[test]
fn toggle_follows_the_pointer() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let default = home.join("Library/Application Support/bar");
    let state = t.path().join("xdg/bar");
    marked(&state);
    assert!(pointer::sync(&state, &default, SystemTime::now()));
    let o = sh(&format!("'{}' toggle 5h", bar().display()), &home, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(state.join("pct-5h").is_file());
    assert!(!default.join("pct-5h").exists());
}

/// `bar render` keeps the pointer, so a click handler started without the
/// renderer's environment still finds its toggles.
#[cfg(target_os = "macos")]
#[test]
fn render_writes_the_pointer() {
    let t = Tmp::new();
    let home = t.path().join("home");
    let default = home.join("Library/Application Support/bar");
    let xdg = t.path().join("xdg");
    marked(&xdg.join("bar"));
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .arg("render")
        .env_clear()
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &xdg)
        .env("XDG_CONFIG_HOME", t.path().join("cfg"))
        .env("PATH", "/usr/bin:/bin")
        .write_stdin("{}")
        .output()
        .unwrap();
    assert!(o.status.success());
    assert_eq!(pointer::read(&default), Some(xdg.join("bar")));
}

/// `bar check` reports a handler pinned to another state dir.
#[cfg(target_os = "macos")]
#[test]
fn check_warns_about_a_handler_on_another_dir() {
    let t = Tmp::new();
    let apps = t.path().join("apps");
    let claude = t.path().join("claude");
    let c = InstallCfg {
        app: apps.join("bar-click.app"),
        settings: claude.join("settings.json"),
        ..cfg(&t, env_of(&[("BAR_STATE_DIR", "/s")]))
    };
    run_install(&c, true, false, &mut |_| Ok(()), &mut Vec::new()).unwrap();
    let check = |state: &Path| {
        let o = assert_cmd::Command::cargo_bin("bar")
            .unwrap()
            .args(["check", "--format", "table"])
            .current_dir(t.path())
            .env("HOME", t.path().join("home"))
            .env("XDG_CONFIG_HOME", t.path().join("cfg"))
            .env("BAR_APP_DIR", &apps)
            .env("CLAUDE_CONFIG_DIR", &claude)
            .env("BAR_STATE_DIR", state)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let same = check(&c.state_dir);
    assert!(!same.contains("bar:// handler"), "{same}");
    let other = t.path().join("other");
    let out = check(&other);
    assert!(
        out.contains("warning: the bar:// handler uses state dir"),
        "{out}"
    );
    assert!(out.contains("bar click install"), "{out}");
}
