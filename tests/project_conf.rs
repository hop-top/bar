//! Project config: lookup, layering over the user config, and the
//! `project-config` policy.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::conf::{layers, parse_layers, project, ConfError, LayerError, ProjectMode};
use std::cell::Cell;

fn layered(user: Option<&str>, proj: &str) -> Result<hop_bar::conf::Config, Vec<LayerError>> {
    parse_layers(user, |_| Some(Ok(proj.to_string())))
}

/// The config and the project lines it dropped.
fn dropping(user: Option<&str>, proj: &str) -> (hop_bar::conf::Config, Vec<ConfError>) {
    let l = layers(user, |_| Some(Ok(proj.to_string())));
    (l.config.unwrap(), l.dropped)
}

fn messages(e: &[ConfError]) -> Vec<(usize, String)> {
    e.iter().map(|e| (e.line, e.message.clone())).collect()
}

#[test]
fn find_prefers_start_dir() {
    let t = Tmp::new();
    t.write(".git/HEAD", "ref: refs/heads/main\n");
    t.write(".bar.conf", "set gap 1\n");
    t.write("a/b/.bar.conf", "set gap 2\n");
    let start = t.path().join("a/b");
    assert_eq!(project::find(&start), Some(start.join(".bar.conf")));
}

#[test]
fn find_climbs_to_git_root_and_takes_nearest() {
    let t = Tmp::new();
    t.write(".git/HEAD", "ref: refs/heads/main\n");
    t.write(".bar.conf", "");
    t.write("a/.bar.conf", "");
    std::fs::create_dir_all(t.path().join("a/b/c")).unwrap();
    assert_eq!(
        project::find(&t.path().join("a/b/c")),
        Some(t.path().join("a/.bar.conf"))
    );
    std::fs::create_dir_all(t.path().join("z")).unwrap();
    assert_eq!(
        project::find(&t.path().join("z")),
        Some(t.path().join(".bar.conf"))
    );
}

#[test]
fn find_stops_at_git_root() {
    let t = Tmp::new();
    t.write(".bar.conf", "");
    // a worktree's `.git` is a file; it marks the root all the same
    t.write("repo/.git", "gitdir: /elsewhere\n");
    std::fs::create_dir_all(t.path().join("repo/src")).unwrap();
    assert_eq!(project::find(&t.path().join("repo/src")), None);
}

#[test]
fn find_outside_a_repo_looks_at_start_only() {
    let t = Tmp::new();
    t.write(".bar.conf", "");
    std::fs::create_dir_all(t.path().join("sub")).unwrap();
    assert_eq!(project::find(&t.path().join("sub")), None);
    assert_eq!(project::find(t.path()), Some(t.path().join(".bar.conf")));
    assert_eq!(project::find(std::path::Path::new("rel")), None);
}

#[test]
fn project_lines_apply_after_user_lines() {
    let user =
        "set left \"#{model}\"\nset gap 5\ndefault prs limit=3 status=open\nset color.good 10\n";
    let proj = "set left \"#{ctx}\"\ndefault prs limit=9\nset palette nord\n";
    let c = layered(Some(user), proj).unwrap();
    assert_eq!(c.left, "#{ctx}");
    assert_eq!(c.gap, 5);
    let d: Vec<_> = c.defaults["prs"].iter().cloned().collect();
    assert_eq!(
        d,
        [
            ("limit".to_string(), "9".to_string()),
            ("status".to_string(), "open".to_string())
        ]
    );
    // a colour the user set explicitly survives the project's palette
    assert_eq!(c.colors.good, 10);
    assert_eq!(c.palette, "nord");
}

#[test]
fn project_rule_and_account_replace_the_users() {
    let user = "rule usage[type=5h] early pace final=1h bad=30%\naccount \"*@a.io\" work 1\naccount \"*@b.io\" home 2\n";
    let proj = "rule usage[type=5h] early pace final=2h bad=10%\naccount \"*@a.io\" client 3\n";
    let c = layered(Some(user), proj).unwrap();
    assert_eq!(c.rules.len(), 1);
    assert_eq!(c.rules[0].final_secs, 7200);
    let acc: Vec<_> = c.accounts.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(acc, ["home", "client"]);
}

#[test]
fn project_over_builtin_layout_when_no_user_file() {
    let c = layered(None, "set right \"#{version}\"\n").unwrap();
    let builtin = hop_bar::render::builtin_config();
    assert_eq!(c.left, builtin.left);
    assert_eq!(c.right, "#{version}");
}

#[test]
fn off_never_asks_for_the_project_file() {
    let asked = Cell::new(false);
    let c = parse_layers(Some("set project-config off\n"), |_| {
        asked.set(true);
        Some(Ok("set gap 1\n".into()))
    })
    .unwrap();
    assert!(!asked.get());
    assert_eq!(c.gap, 3);
    assert_eq!(c.project_config, ProjectMode::Off);
}

#[test]
fn project_config_key_is_user_only() {
    let (c, e) = dropping(
        Some("set project-config safe\n"),
        "set project-config full\n",
    );
    assert_eq!(c.project_config, ProjectMode::Safe);
    assert_eq!(
        messages(&e),
        [(
            1,
            "`project-config` is read from the user config only".to_string()
        )]
    );
    let (c, e) = dropping(
        Some("set project-config full\n"),
        "set project-config off\n",
    );
    assert_eq!(c.project_config, ProjectMode::Full);
    assert_eq!(e[0].line, 1);
    assert!(hop_bar::conf::parse("set project-config maybe\n").is_err());
}

#[test]
fn safe_refuses_commands_env_plugin_lines_and_new_plugins() {
    let proj = "set left \"#{model[icon=%touch pwned%]}\"\n\
                default prs author=%$GITHUB_TOKEN%\n\
                plugin ci ttl=1m\n\
                set right \"#{ci}\"\n\
                set center \"#{ctx[icon=50%]} 100%\"\n";
    let (c, e) = dropping(Some("set left \"#{model}\"\n"), proj);
    let lines: Vec<_> = e.iter().map(|e| e.line).collect();
    assert_eq!(lines, [1, 2, 3, 4]);
    assert!(e[0].message.contains("set project-config full"));
    assert!(e[3].message.contains("plugin `ci`"));
    // the refused lines are dropped, the rest applies
    assert_eq!(c.left, "#{model}");
    assert_eq!(c.right, "");
    assert!(c.plugins.is_empty() && c.defaults.is_empty());
    assert_eq!(c.center, "#{ctx[icon=50%]} 100%");
}

#[test]
fn invalid_project_lines_are_dropped() {
    let (c, e) = dropping(
        Some("set gap 4\n"),
        "set gap\nset gap x\nset separator \"|\n\nset wrap on\n",
    );
    assert_eq!(c.gap, 4);
    assert_eq!(c.wrap, hop_bar::conf::Wrap::On);
    let lines: Vec<_> = e.iter().map(|e| e.line).collect();
    assert_eq!(lines, [1, 2, 3]);
}

#[test]
fn safe_refuses_turning_on_the_update_check() {
    let (c, e) = dropping(None, "set update-check on\n");
    assert!(!c.update_check);
    assert!(e[0].message.contains("set project-config full"));
    let c = layered(Some("set project-config full\n"), "set update-check on\n").unwrap();
    assert!(c.update_check);
}

#[test]
fn safe_refuses_turning_on_the_usage_api() {
    let (c, e) = dropping(None, "set usage-api on\n");
    assert!(!c.usage_api);
    assert!(e[0].message.contains("set project-config full"));
    let c = layered(Some("set project-config full\n"), "set usage-api on\n").unwrap();
    assert!(c.usage_api);
}

#[test]
fn safe_allows_plugins_the_user_config_names() {
    for user in [
        "set right \"#{ci}\"\n",
        "default ci limit=1\n",
        "plugin ci ttl=1m\n",
    ] {
        let c = layered(Some(user), "set left \"#{ci} #{model}\"\n").unwrap();
        assert_eq!(c.left, "#{ci} #{model}");
    }
}

#[test]
fn full_allows_everything_but_project_config() {
    let proj = "set left \"#{model[icon=%echo x%]}\"\nplugin ci ttl=1m\nset right \"#{ci}\"\n";
    let c = layered(Some("set project-config full\n"), proj).unwrap();
    assert_eq!(c.right, "#{ci}");
    assert!(c.plugins.contains_key("ci"));
}

#[test]
fn unreadable_project_file_is_dropped_without_line() {
    let l = layers(Some("set gap 2\n"), |_| Some(Err("nope".into())));
    assert_eq!(l.config.unwrap().gap, 2);
    assert_eq!(messages(&l.dropped), [(0, "nope".to_string())]);
}

#[test]
fn user_errors_stay_errors() {
    let e: Vec<LayerError> = layers(Some("set gap x\n"), |_| Some(Ok("set gap\n".into())))
        .config
        .unwrap_err();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].layer, hop_bar::conf::Layer::User);
}

#[test]
fn render_applies_project_file_from_session_dir() {
    let t = Tmp::new();
    t.write("repo/.git/HEAD", "ref: refs/heads/main\n");
    t.write("repo/.bar.conf", "set left \"P #{model}\"\n");
    let sub = t.path().join("repo/src");
    std::fs::create_dir_all(&sub).unwrap();
    let json = serde_json::json!({
        "workspace": {"current_dir": sub},
        "model": {"display_name": "Opus"},
    })
    .to_string();
    let o = run(&t).conf("set left \"U #{model}\"\n").stdin(&json).go();
    assert_eq!(o.plain.trim(), "P Opus");
    let o = run(&t)
        .conf("set project-config off\nset left \"U #{model}\"\n")
        .stdin(&json)
        .go();
    assert_eq!(o.plain.trim(), "U Opus");
}

#[test]
fn render_refuses_a_project_command_under_safe() {
    let t = Tmp::new();
    let marker = t.path().join("ran");
    t.write(
        "proj/.bar.conf",
        &format!(
            "set left \"#{{model[icon=%touch {}%]}}\"\n",
            marker.display()
        ),
    );
    let json = serde_json::json!({
        "workspace": {"current_dir": t.path().join("proj")},
        "model": {"display_name": "Opus"},
    })
    .to_string();
    let o = run(&t).conf("set left \"#{model}\"\n").stdin(&json).go();
    // the refused line is dropped; the user's layout renders
    assert_eq!(o.plain.trim(), "Opus");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!marker.exists());
}

/// `bar check` run in `dir` with the hermetic env of `t`.
fn check_in(t: &Tmp, dir: &std::path::Path, args: &[&str]) -> (i32, String) {
    let (iso, _) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .arg("check")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8(o.stdout).unwrap(),
    )
}

#[test]
fn check_names_the_file_of_each_issue() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set left \"#{model}\"\nset gap x\n");
    t.write(
        "proj/.bar.conf",
        "set right \"#{ctx[nope=1]}\"\nplugin ci ttl=1m\n",
    );
    let dir = t.path().join("proj");
    let (code, out) = check_in(&t, &dir, &["--format", "table"]);
    assert_eq!(code, 2, "{out}");
    assert!(
        out.contains(&format!(
            "project: {} (safe)",
            dir.join(".bar.conf").display()
        )),
        "{out}"
    );
    assert!(
        out.contains("config line 2: error: invalid gap `x`"),
        "{out}"
    );
    assert!(
        out.contains("project line 1: error: unknown option `nope` for `ctx`"),
        "{out}"
    );
    assert!(
        out.contains("project line 2: warning: a `plugin` line in a project config"),
        "{out}"
    );

    let (_, json) = check_in(&t, &dir, &["--format", "json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["project"]["mode"], "safe");
    assert_eq!(v["project"]["applied"], true);
    let files: Vec<_> = v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["file"].as_str().unwrap().to_string())
        .collect();
    let user = t.path().join("xdg/bar/bar.conf").display().to_string();
    let proj = dir.join(".bar.conf").display().to_string();
    assert_eq!(files, [user, proj.clone(), proj]);
}

#[test]
fn check_reports_but_skips_project_file_under_off() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set project-config off\nset left \"#{model}\"\n",
    );
    t.write("proj/.bar.conf", "set gap x\n");
    let (code, out) = check_in(&t, &t.path().join("proj"), &["--format", "table"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("(not applied: project-config off)"), "{out}");
}

#[test]
fn check_without_project_file_keeps_plain_line_labels() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap x\n");
    let (code, out) = check_in(&t, &t.path().join("xdg"), &["--format", "table"]);
    assert_eq!(code, 2);
    assert!(out.contains("\nline 1: error: invalid gap `x`"), "{out}");
    assert!(!out.contains("project"), "{out}");
}

#[test]
fn check_warns_about_dropped_project_lines_and_passes() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set left \"#{model}\"\n");
    t.write("proj/.bar.conf", "set gap\nplugin ci ttl=1m\nset gap 2\n");
    let (code, out) = check_in(&t, &t.path().join("proj"), &["--format", "table"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("project line 1: warning: "), "{out}");
    assert!(out.contains("project line 2: warning: "), "{out}");
    assert!(out.contains("the line is ignored"), "{out}");
}

#[test]
fn safe_refuses_setting_claude_dir() {
    let (c, e) = dropping(None, "set claude-dir /tmp/elsewhere\n");
    assert_eq!(c.claude_dir, None);
    assert!(e[0].message.contains("set project-config full"), "{e:?}");
    let c = layered(
        Some("set project-config full\n"),
        "set claude-dir /tmp/elsewhere\n",
    )
    .unwrap();
    assert_eq!(c.claude_dir.as_deref(), Some("/tmp/elsewhere"));
}

#[test]
fn read_refuses_oversized_project_files() {
    let t = Tmp::new();
    t.write(".bar.conf", &"# pad\n".repeat(12_000));
    let e = project::read(&t.path().join(".bar.conf")).unwrap_err();
    assert!(e.contains("64 KiB"), "{e}");
    t.write(".bar.conf", "set gap 1\n");
    assert_eq!(
        project::read(&t.path().join(".bar.conf")).unwrap(),
        "set gap 1\n"
    );
}

#[cfg(unix)]
#[test]
fn read_refuses_a_symlink_out_of_the_repository() {
    let t = Tmp::new();
    t.write("secret.conf", "set gap 1\n");
    t.write("repo/.git/HEAD", "ref: refs/heads/main\n");
    t.write("repo/shared.conf", "set gap 2\n");
    let inside = t.path().join("repo/sub");
    std::fs::create_dir_all(&inside).unwrap();
    let link = inside.join(".bar.conf");
    std::os::unix::fs::symlink(t.path().join("secret.conf"), &link).unwrap();
    let e = project::read(&link).unwrap_err();
    assert!(e.contains("outside the repository"), "{e}");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("../shared.conf", &link).unwrap();
    assert_eq!(project::read(&link).unwrap(), "set gap 2\n");
}

#[test]
fn render_ignores_an_oversized_project_file() {
    let t = Tmp::new();
    t.write(
        "proj/.bar.conf",
        &format!("set left \"P #{{model}}\"\n{}", "# pad\n".repeat(12_000)),
    );
    let json = serde_json::json!({
        "workspace": {"current_dir": t.path().join("proj")},
        "model": {"display_name": "Opus"},
    })
    .to_string();
    let o = run(&t).conf("set left \"U #{model}\"\n").stdin(&json).go();
    assert_eq!(o.plain.trim(), "U Opus");
}

#[cfg(unix)]
#[test]
fn read_refuses_a_file_someone_else_owns() {
    use std::os::unix::fs::MetadataExt;
    let t = Tmp::new();
    t.write(".bar.conf", "set gap 1\n");
    let f = t.path().join(".bar.conf");
    let mine = std::fs::metadata(&f).unwrap().uid();
    let e = project::read_as(&f, Some(mine + 1)).unwrap_err();
    assert!(e.contains("owned by another user"), "{e}");
    assert!(project::read_as(&f, Some(mine)).is_ok());
}

#[test]
fn safe_refuses_turning_on_the_service_status() {
    let (c, e) = dropping(None, "set service-status on\n");
    assert!(!c.service_status);
    assert!(e[0].message.contains("set project-config full"));
    let c = layered(Some("set project-config full\n"), "set service-status on\n").unwrap();
    assert!(c.service_status);
    // the user config may
    let (c, e) = dropping(Some("set service-status on\n"), "");
    assert!(c.service_status && e.is_empty());
}
