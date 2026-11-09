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
