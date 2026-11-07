mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use hop_bar::presets;
use render_fx::Tmp;
use serde_json::Value;

/// `bar <args>` with every config, state and PATH location under `t`.
fn bar(t: &Tmp, args: &[&str]) -> Command {
    let (iso, cwd) = common::isolation(t.path());
    let mut c = Command::cargo_bin("bar").unwrap();
    c.envs(iso)
        .current_dir(cwd)
        .env("COLUMNS", "120")
        .args(args);
    c
}

#[test]
fn preset_names() {
    assert_eq!(
        presets::NAMES,
        ["minimal", "full", "powerline", "capsule", "compact"]
    );
    for n in presets::NAMES {
        let on_disk =
            std::fs::read_to_string(format!("{}/presets/{n}.conf", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
        assert_eq!(presets::get(n), Some(on_disk.as_str()), "{n}");
    }
    assert_eq!(presets::get("nope"), None);
}

#[test]
fn every_preset_checks_clean() {
    for n in presets::NAMES {
        let t = Tmp::new();
        t.write("xdg/bar/bar.conf", presets::get(n).unwrap());
        let o = bar(&t, &["check", "--format", "json"]).assert().success();
        let v: Value = serde_json::from_slice(&o.get_output().stdout).unwrap();
        assert_eq!(v["issues"], Value::Array(vec![]), "{n}: {v}");
    }
}

#[test]
fn preview_renders_a_preset() {
    let t = Tmp::new();
    let o = bar(&t, &["preview", "--preset", "powerline"])
        .assert()
        .success();
    let out = String::from_utf8(o.get_output().stdout.clone()).unwrap();
    assert!(
        out.contains('\u{e0b0}') && out.contains("\x1b[48;5;"),
        "{out:?}"
    );
    let o = bar(&t, &["preview", "--preset", "capsule"])
        .assert()
        .success();
    let out = String::from_utf8(o.get_output().stdout.clone()).unwrap();
    assert!(
        out.contains('\u{e0b6}') && out.contains('\u{e0b4}'),
        "{out:?}"
    );
    bar(&t, &["preview", "--preset", "nope"]).assert().code(2);
}

#[test]
fn preview_preset_ignores_user_config() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{model}\"\nset skin capsule\n",
    );
    let o = bar(&t, &["preview", "--preset", "minimal"])
        .assert()
        .success();
    let out = String::from_utf8(o.get_output().stdout.clone()).unwrap();
    assert!(!out.contains('\u{e0b6}'), "{out:?}");
    assert!(render_fx::strip(&out).contains("ctx 38%"), "{out:?}");
}

#[test]
fn init_writes_default_preset_when_absent() {
    let t = Tmp::new();
    let o = bar(&t, &["init"]).assert().success();
    let path = t.path().join("xdg/bar/bar.conf");
    let out = String::from_utf8(o.get_output().stdout.clone()).unwrap();
    assert!(out.contains(&path.display().to_string()), "{out}");
    let body = std::fs::read_to_string(&path).unwrap();
    assert_eq!(body, presets::get("full").unwrap());
}

#[test]
fn init_named_preset_under_home_config() {
    let t = Tmp::new();
    bar(&t, &["init", "--preset", "capsule"])
        .env_remove("XDG_CONFIG_HOME")
        .assert()
        .success();
    let body = std::fs::read_to_string(t.path().join(".config/bar/bar.conf")).unwrap();
    assert_eq!(body, presets::get("capsule").unwrap());
}

#[test]
fn init_refuses_to_overwrite() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set left \"#{model}\"\n");
    let o = bar(&t, &["init", "--preset", "minimal"]).assert().code(2);
    let err = String::from_utf8(o.get_output().stderr.clone()).unwrap();
    assert!(err.contains("exists"), "{err}");
    let body = std::fs::read_to_string(t.path().join("xdg/bar/bar.conf")).unwrap();
    assert_eq!(body, "set left \"#{model}\"\n");
}

#[test]
fn init_refuses_when_another_search_path_has_a_config() {
    let t = Tmp::new();
    t.write(".config/bar/bar.conf", "set left \"#{model}\"\n");
    bar(&t, &["init"]).assert().code(2);
    assert!(!t.path().join("xdg/bar/bar.conf").exists());
}

#[test]
fn init_has_no_force() {
    let t = Tmp::new();
    bar(&t, &["init", "--force"]).assert().code(2);
    bar(&t, &["init", "--preset", "nope"]).assert().code(2);
    assert!(!t.path().join("xdg/bar/bar.conf").exists());
}

#[test]
fn check_validates_bg_option() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{ctx[bg=200]} │ #{model[bg=%$X%]}\"\n",
    );
    bar(&t, &["check"]).assert().success();
    t.write("xdg/bar/bar.conf", "set left \"#{ctx[bg=300]}\"\n");
    let o = bar(&t, &["check"]).assert().code(2);
    let out = String::from_utf8(o.get_output().stdout.clone()).unwrap();
    assert!(out.contains("invalid bg `300`"), "{out}");
}
