#![cfg(unix)]
//! The real binary, discovered beside `bar` and driven by bar's own plugin
//! code: what `--ext-info`, `--check` and a render print is what the core
//! parses, and a click on the state item, run by the core's handler,
//! switches the fake proxy.

mod common;
mod fake;

use common::{healthy, Env, KEY};
use hop_bar::cache::{Cache, Job};
use hop_bar::click::action::{self, NONCE_FILE, STATUS_FILE};
use hop_bar::conf::parse;
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-routellm");
const NOW: i64 = 1_800_000_000;

fn run(path: impl AsRef<std::path::Path>, arg: Option<&str>, stdin: &str) -> String {
    let mut ch = Command::new(path.as_ref())
        .args(arg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = ch.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn private(p: &std::path::Path, text: &str) {
    std::fs::write(p, text).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
#[allow(clippy::cognitive_complexity)] // over the threshold; split when next touched
fn core_parses_the_output_draws_it_and_runs_the_click() {
    let e = Env::new("compat", false, healthy(), 1000);
    let root = std::env::temp_dir().join(format!("rl-compat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (bin, state) = (root.join("bin"), root.join("state"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    // discovered beside `bar`, not on PATH
    std::fs::copy(BIN, bin.join("bar-routellm")).unwrap();
    private(&root.join("key"), &format!("{KEY}\n"));
    let conf = root.join("routellm.conf");
    private(
        &conf,
        &format!(
            "instance {}\n routellm {}\n key-file {}\n",
            e.proxy.server.origin(),
            e.rl.origin(),
            root.join("key").display()
        ),
    );
    // the plugin and bar run in this process's environment
    std::env::set_var("BAR_ROUTELLM_CONFIG", &conf);
    std::env::set_var("BAR_STATE_DIR", &state);
    std::env::set_var("ANTHROPIC_BASE_URL", e.proxy.server.origin());
    private(&state.join(NONCE_FILE), "0123456789abcdef0123456789abcdef");

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    let found = plugin::find(&dirs, "routellm").expect("discovered beside bar");
    let info = plugin::parse_info(&run(&found.path, Some("--ext-info"), "")).expect("ext-info");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.timeout, 8);
    assert!(info.is_placeholder());
    assert!(info.inputs.env.iter().any(|n| n == "ANTHROPIC_BASE_URL"));
    assert!(info.inputs.session.iter().any(|s| s == "model_id"));
    // the declared options, actions and web samples pass the core's checks
    assert_eq!(plugin::web::issues(&info), Vec::<String>::new());
    for o in [
        "show", "label", "compact", "stale", "link", "routed", "base-url",
    ] {
        assert!(info.options.contains_key(o), "{o}");
    }
    for a in ["toggle", "on", "off"] {
        assert!(info.actions.contains_key(a), "{a}");
    }
    let warns = plugin::check::parse_warnings(&run(&found.path, Some("--check"), ""));
    assert!(warns.iter().all(|w| !w.is_empty()), "{warns:?}");

    let cfg = parse("action routellm routellm.toggle\n").unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!("info job")
    };
    cache
        .store(&ikey, &run(path, Some("--ext-info"), ""))
        .unwrap();
    let session = Session {
        model_id: "route-auto".into(),
        model: "route-auto".into(),
        cwd: "/tmp/work".into(),
        ..Session::default()
    };
    let ctx = Ctx {
        session: &session,
        config: &cfg,
        now: NOW,
        runner: &ColdRunner,
        cache: Some(&cache),
        columns: Some(120),
        plugin_dirs: &dirs,
        memo: Default::default(),
    };
    let inf = plugin::cached_info(&cache, &found).expect("cached info");
    let mut opts = Options::default();
    opts.set("show", "state,alias,tier,marker,caps");
    let plan = plugin::plan(&ctx, &found, &inf, &opts).expect("plan");
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!("render job")
    };
    // the core hands the declared environment and the model to the plugin
    assert!(input.contains(&e.proxy.server.origin()), "{input}");
    assert!(input.contains("route-auto"), "{input}");
    let text = run(path, None, input);
    let out = plugin::parse_output(&text).expect("render output");
    let words: Vec<&str> = out.items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(
        words,
        ["on", "route-auto[auto]", "\u{2601}", "vis tools 200k"],
        "{text}"
    );
    assert_eq!(out.items[0].style.as_deref(), Some("good"));
    assert_eq!(out.items[0].tokens["to"], "off");
    cache.store(&plan.key, &text).unwrap();

    // drawn through the core's painter, with the toggle as a link
    let shown = {
        let mut o = Options::default();
        o.set("show", "state,alias,tier,marker,caps");
        plugin::render(&ctx, "routellm", &o).unwrap_or_default()
    };
    let plain = hop_bar::style::strip_codes(&shown);
    assert!(
        plain.contains("on") && plain.contains("route-auto[auto]") && plain.contains("\u{2601}"),
        "{shown:?}"
    );
    let url = shown
        .split("\x1b]8;;")
        .skip(1)
        .filter_map(|r| r.split(['\x1b', '\x07']).next())
        .find(|u| u.starts_with("bar://action/routellm.toggle?"))
        .unwrap_or_else(|| panic!("no action link in {shown:?}"))
        .to_string();

    // the default segment, drawn by the core (icons off: the dim label
    // stands in for the icon)
    let draw = |model: &str, o: &Options| -> String {
        let session = Session {
            model_id: model.into(),
            model: model.into(),
            cwd: "/tmp/work".into(),
            ..Session::default()
        };
        let ctx = Ctx {
            session: &session,
            config: &cfg,
            now: NOW,
            runner: &ColdRunner,
            cache: Some(&cache),
            columns: Some(120),
            plugin_dirs: &dirs,
            memo: Default::default(),
        };
        let plan = plugin::plan(&ctx, &found, &inf, o).expect("plan");
        let Job::PluginRender { path, input, .. } = &plan.job else {
            panic!("render job")
        };
        cache.store(&plan.key, &run(path, None, input)).unwrap();
        hop_bar::style::strip_codes(&plugin::render(&ctx, "routellm", o).unwrap_or_default())
    };
    let d = Options::default();
    assert_eq!(draw("route-auto", &d), "RL route-auto[auto]");
    assert_eq!(draw("route-private", &d), "RL route-private[private]");
    assert_eq!(draw("claude-opus-4", &d), "RL on");

    // the click runs the real `bar-routellm --action toggle`
    action::run(&state, &url, &cfg, &dirs).unwrap();
    assert_eq!(e.proxy.entries.lock().unwrap()[0]["disabled"], true);
    let status = std::fs::read_to_string(state.join(STATUS_FILE)).unwrap();
    assert!(status.contains("ok:"), "{status:?}");
    // after the click: tier and the full model id
    assert_eq!(draw("route-auto", &d), "RL auto[claude-sonnet-5-5]");
    // no row for the private alias: refused
    assert_eq!(draw("route-private", &d), "RL private[\u{2717}]");
    // not on an alias: the state word alone
    assert_eq!(draw("claude-opus-4", &d), "RL off");
    let _ = std::fs::remove_dir_all(&root);
}
