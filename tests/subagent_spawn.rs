//! A `bar subagent` row runs no process: no terminal probe (`ps`) for
//! command links, no `git` when the git files are not readable.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::click::Term;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::{ctx as ctxp, model, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::subagent::{self, Payload, Task};
use render_fx::Tmp;

fn row_session(cwd: &str) -> Session {
    let p = Payload {
        session_id: "0123abcd-0000-4000-8000-000000000000".into(),
        cwd: cwd.into(),
        ..Payload::default()
    };
    let t = Task {
        id: "t".into(),
        model: "claude-opus-5-5".into(),
        effort: "high".into(),
        context_size: Some(200_000),
        tokens: Some(1000),
        ..Task::default()
    };
    subagent::session(&p, &t)
}

fn no_probe() -> Term {
    panic!("terminal probed in a subagent row");
}

#[test]
fn command_links_never_probe_the_terminal_in_a_row() {
    let t = Tmp::new();
    // click handler installed for commands
    std::fs::write(t.path().join("click-installed"), "cmd").unwrap();
    let s = row_session("");
    let config = Config::default();
    let ctx = Ctx {
        session: &s,
        config: &config,
        now: 0,
        runner: &ColdRunner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    let o = Options::default();
    assert!(model::model_with(&ctx, &o, t.path(), no_probe).is_some());
    assert!(model::effort_with(&ctx, &o, t.path(), no_probe).is_some());
    assert!(ctxp::render_with(&ctx, &o, t.path(), no_probe).is_some());
}

/// `bar subagent` with `#{repo}`, `#{branch}`, `#{prs}` in the row and a
/// repository the git file reader does not support (`GIT_CONFIG_NOSYSTEM`
/// set): a logging `git` on PATH is never run.
#[cfg(unix)]
#[test]
fn git_is_never_run_in_the_foreground_for_a_row() {
    use std::os::unix::fs::PermissionsExt;
    let t = Tmp::new();
    let (iso, cwd) = common::isolation(t.path());
    let log = t.path().join("fake-git.log");
    let git = t.path().join("fake-bin/git");
    std::fs::write(
        &git,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\necho main\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    t.write("repo/.git/HEAD", "ref: refs/heads/main\n");
    t.write(
        "repo/.git/config",
        "[remote \"origin\"]\n\turl = https://github.com/o/r\n",
    );
    t.write(
        "xdg/bar/bar.conf",
        "set subagent \"#{task.name} #{repo} #{branch} #{prs}\"\n",
    );
    let repo = t.path().join("repo");
    let input = serde_json::json!({
        "cwd": repo, "columns": 100,
        "tasks": [{"id": "a", "agentType": "Explore", "status": "running", "cwd": repo}],
    });
    let mut c = assert_cmd::Command::cargo_bin("bar").unwrap();
    c.arg("subagent").envs(iso).current_dir(cwd);
    let out = c.write_stdin(input.to_string()).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(text.contains("Explore"), "{text}");
    assert!(
        !log.exists(),
        "git ran: {:?}",
        std::fs::read_to_string(&log)
    );
}
