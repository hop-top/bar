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
use hop_bar::conf::{self, parse};
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
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

/// The plugin beside a fake `bar`, its config pointing at the fake proxy
/// and RouteLLM, and the core's state and cache. The plugin and bar run
/// in this process's environment, so one test drives it all.
struct Compat {
    e: Env,
    root: PathBuf,
    state: PathBuf,
    dirs: Vec<PathBuf>,
    found: plugin::Found,
    cfg: conf::Config,
    cache: Cache,
}

fn setup() -> Compat {
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
    std::env::set_var("BAR_ROUTELLM_CONFIG", &conf);
    std::env::set_var("BAR_STATE_DIR", &state);
    std::env::set_var("ANTHROPIC_BASE_URL", e.proxy.server.origin());
    private(&state.join(NONCE_FILE), "0123456789abcdef0123456789abcdef");

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    let found = plugin::find(&dirs, "routellm").expect("discovered beside bar");
    let cfg = parse("action routellm routellm.toggle\n").unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!("info job")
    };
    cache
        .store(&ikey, &run(path, Some("--ext-info"), ""))
        .unwrap();
    Compat {
        e,
        root,
        state,
        dirs,
        found,
        cfg,
        cache,
    }
}

/// `--ext-info` as the core parses it: the declared inputs, options,
/// actions and web samples pass the core's checks.
fn ext_info_checks(found: &plugin::Found) {
    let info = plugin::parse_info(&run(&found.path, Some("--ext-info"), "")).expect("ext-info");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.timeout, 8);
    assert!(info.is_placeholder());
    assert!(info.inputs.env.iter().any(|n| n == "ANTHROPIC_BASE_URL"));
    assert!(info.inputs.session.iter().any(|s| s == "model_id"));
    assert_eq!(plugin::web::issues(&info), Vec::<String>::new());
    for o in [
        "show", "label", "compact", "stale", "link", "routed", "base-url",
    ] {
        assert!(info.options.contains_key(o), "{o}");
    }
    for a in ["toggle", "on", "off"] {
        assert!(info.actions.contains_key(a), "{a}");
    }
}

fn session(model: &str) -> Session {
    Session {
        model_id: model.into(),
        model: model.into(),
        cwd: "/tmp/work".into(),
        ..Session::default()
    }
}

fn ctx<'a>(c: &'a Compat, session: &'a Session) -> Ctx<'a> {
    Ctx {
        session,
        config: &c.cfg,
        now: NOW,
        runner: &ColdRunner,
        cache: Some(&c.cache),
        columns: Some(120),
        plugin_dirs: &c.dirs,
        memo: Default::default(),
    }
}

/// The plugin run on the core's plan for `model`, its output parsed by
/// the core and cached for a render.
fn render(c: &Compat, model: &str, o: &Options) -> (String, plugin::Output) {
    let session = session(model);
    let ctx = ctx(c, &session);
    let inf = plugin::cached_info(&c.cache, &c.found).expect("cached info");
    let plan = plugin::plan(&ctx, &c.found, &inf, o).expect("plan");
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!("render job")
    };
    // the core hands the declared environment and the model to the plugin
    assert!(input.contains(&c.e.proxy.server.origin()), "{input}");
    assert!(input.contains(model), "{input}");
    let text = run(path, None, input);
    let out = plugin::parse_output(&text).expect("render output");
    c.cache.store(&plan.key, &text).unwrap();
    (text, out)
}

/// The segment as the core draws it for `model`, codes stripped (icons
/// off: the dim label stands in for the icon).
fn draw(c: &Compat, model: &str, o: &Options) -> String {
    render(c, model, o);
    let session = session(model);
    hop_bar::style::strip_codes(
        &plugin::render(&ctx(c, &session), "routellm", o).unwrap_or_default(),
    )
}

#[test]
fn core_parses_the_output_draws_it_and_runs_the_click() {
    let c = setup();
    ext_info_checks(&c.found);
    let warns = plugin::check::parse_warnings(&run(&c.found.path, Some("--check"), ""));
    assert!(warns.iter().all(|w| !w.is_empty()), "{warns:?}");

    let mut opts = Options::default();
    opts.set("show", "state,alias,tier,marker,caps");
    let (text, out) = render(&c, "route-auto", &opts);
    let words: Vec<&str> = out.items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(
        words,
        ["on", "route-auto[auto]", "\u{2601}", "vis tools 200k"],
        "{text}"
    );
    assert_eq!(out.items[0].style.as_deref(), Some("good"));
    assert_eq!(out.items[0].tokens["to"], "off");

    // drawn through the core's painter, with the toggle as a link
    let session = session("route-auto");
    let shown = plugin::render(&ctx(&c, &session), "routellm", &opts).unwrap_or_default();
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

    // the default segment, drawn by the core
    let d = Options::default();
    for (model, want) in [
        ("route-auto", "RL route-auto[auto]"),
        ("route-private", "RL route-private[private]"),
        // not on an alias: the state word alone
        ("claude-opus-4", "RL on"),
    ] {
        assert_eq!(draw(&c, model, &d), want);
    }

    // the click runs the real `bar-routellm --action toggle`
    action::run(&c.state, &url, &c.cfg, &c.dirs).unwrap();
    assert_eq!(c.e.proxy.entries.lock().unwrap()[0]["disabled"], true);
    let status = std::fs::read_to_string(c.state.join(STATUS_FILE)).unwrap();
    assert!(status.contains("ok:"), "{status:?}");
    for (model, want) in [
        // after the click: tier and the full model id
        ("route-auto", "RL auto[claude-sonnet-5-5]"),
        // no row for the private alias: refused
        ("route-private", "RL private[\u{2717}]"),
        ("claude-opus-4", "RL off"),
    ] {
        assert_eq!(draw(&c, model, &d), want);
    }
    let _ = std::fs::remove_dir_all(&c.root);
}
