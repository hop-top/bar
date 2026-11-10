#![cfg(unix)]
//! The real binary, discovered beside `bar` and driven by bar's own plugin
//! code: what `--ext-info`, `--check` and a render print must be what the
//! core parses, so drift between the two fails here.

mod fake;

use fake::{ok, Server};
use hop_bar::cache::{Cache, Job};
use hop_bar::conf::{self, parse};
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use serde_json::json;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const NOW: i64 = 1_800_000_000;

fn run(path: impl AsRef<std::path::Path>, arg: Option<&str>, stdin: &str) -> String {
    let mut c = Command::new(path.as_ref());
    c.args(arg).stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = ch.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

/// The plugin beside a fake `bar`, its config pointing at a fake proxy
/// with two Claude accounts, and the core's state and cache. The plugin
/// and bar run in this process's environment, so one test drives it all.
struct Compat {
    server: Server,
    root: PathBuf,
    dirs: Vec<PathBuf>,
    found: plugin::Found,
    cfg: conf::Config,
    cache: Cache,
}

fn setup() -> Compat {
    let listing = json!({"files":[
        {"auth_index":"ix1","type":"claude","email":"a@corp.test",
         "priority":1,
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}},
        {"auth_index":"ix2","type":"claude","email":"b@corp.test",
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.95",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}}]});
    let server = Server::start(move |_| ok(listing.to_string()));

    let root = std::env::temp_dir().join(format!("cpa-compat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (bin, state) = (root.join("bin"), root.join("state"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    // discovered beside `bar`, not on PATH
    let plugin_path = bin.join("bar-cliproxyapi");
    std::fs::copy(BIN, &plugin_path).unwrap();
    let key = root.join("key");
    std::fs::write(&key, "compat-key\n").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let conf = root.join("cliproxyapi.conf");
    std::fs::write(
        &conf,
        format!(
            "instance {}\n key-file {}\n alias \"a@*\" me\n",
            server.origin(),
            key.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::env::set_var("BAR_CLIPROXYAPI_CONFIG", &conf);
    std::env::set_var("BAR_STATE_DIR", &state);
    std::env::set_var("ANTHROPIC_BASE_URL", server.origin());

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    assert_eq!(dirs, std::slice::from_ref(&bin));
    let found = plugin::find(&dirs, "cliproxyapi").expect("discovered beside bar");
    let cfg = parse("").unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!("info job")
    };
    cache
        .store(&ikey, &run(path, Some("--ext-info"), ""))
        .unwrap();
    Compat {
        server,
        root,
        dirs,
        found,
        cfg,
        cache,
    }
}

/// `--ext-info` as the core parses it: the declared inputs, capabilities,
/// options and web samples pass the core's validation.
fn ext_info_checks(found: &plugin::Found) {
    let info = plugin::parse_info(&run(&found.path, Some("--ext-info"), "")).expect("ext-info");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.timeout, 15);
    assert!(info.is_placeholder());
    assert!(info.inputs.env.iter().any(|n| n == "ANTHROPIC_BASE_URL"));
    for n in ["model_id", "model"] {
        assert!(info.inputs.session.iter().any(|s| s == n), "{n}");
    }
    assert!(info
        .capabilities
        .iter()
        .any(|c| c == plugin::check::CHECK_CAPABILITY));
    assert_eq!(plugin::web::issues(&info), Vec::<String>::new());
    for o in [
        "provider", "windows", "show", "count", "label", "sort", "accounts", "compact", "stale",
        "link", "pool",
    ] {
        assert!(info.options.contains_key(o), "{o}");
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

/// The plugin run on the core's plan, its output parsed by the core and
/// cached for a render.
fn render(c: &Compat, session: &Session, o: &Options) -> (String, plugin::Output) {
    let ctx = ctx(c, session);
    let inf = plugin::cached_info(&c.cache, &c.found).expect("cached info");
    let plan = plugin::plan(&ctx, &c.found, &inf, o).expect("plan");
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!("render job")
    };
    // the core hands the declared environment to the plugin
    assert!(input.contains(&c.server.origin()), "{input}");
    let text = run(path, None, input);
    let out = plugin::parse_output(&text).expect("render output");
    c.cache.store(&plan.key, &text).unwrap();
    (text, out)
}

/// One item per account, most used first, with the tokens an action
/// takes; the pool's usage (mean of 50 and 95, latest reset) and the
/// account in the higher priority tier.
fn output_checks(out: &plugin::Output, text: &str) {
    assert_eq!(out.items.len(), 2, "{text}");
    assert_eq!(out.items[0].label.as_deref(), Some("b"));
    assert!(out.items[0].text.starts_with("5%"), "{text}");
    assert_eq!(out.items[0].style.as_deref(), Some("bad"));
    assert_eq!(out.items[1].label.as_deref(), Some("me"));
    assert_eq!(out.items[1].score, Some(50.0));
    assert_eq!(out.items[1].tokens["account"], "ix1");
    assert_eq!(out.items[1].tokens["provider"], "claude");
    let w = out
        .usage
        .as_ref()
        .and_then(|u| u.seven_day.as_ref())
        .expect("usage");
    assert_eq!((w.used_percentage, w.resets_at), (72.5, 4_102_444_800));
    assert!(out.usage.as_ref().unwrap().five_hour.is_none());
    let a = out.account.as_ref().expect("account");
    assert_eq!((a.id.as_str(), a.label.as_str()), ("ix1", "me"));
}

#[test]
fn core_parses_what_the_plugin_prints_and_renders_it() {
    let c = setup();
    ext_info_checks(&c.found);

    // --check: the core reads what the plugin prints
    let warns = plugin::check::parse_warnings(&run(&c.found.path, Some("--check"), ""));
    assert!(warns.iter().all(|w| !w.is_empty()), "{warns:?}");

    // a render through the core's own input, cache and painter
    let session = Session {
        model_id: "claude-opus-4".into(),
        model: "Opus".into(),
        cwd: "/tmp/work".into(),
        ..Session::default()
    };
    let (text, out) = render(&c, &session, &Options::default());
    output_checks(&out, &text);
    let shown =
        plugin::render(&ctx(&c, &session), "cliproxyapi", &Options::default()).unwrap_or_default();
    assert!(
        hop_bar::style::strip_codes(&shown).contains("me 50%"),
        "{shown:?}"
    );

    // options typed by the core's schema reach the plugin and shape the output
    let mut opts = Options::default();
    for (k, v) in [
        ("windows", "7d,5h"),
        ("compact", "true"),
        ("count", "used"),
        ("pool", "max"),
    ] {
        opts.set(k, v);
    }
    let (_, out) = render(&c, &session, &opts);
    assert_eq!(out.items[1].text.split('\u{B7}').next(), Some("50%"));
    let _ = std::fs::remove_dir_all(&c.root);
}

#[test]
fn a_layout_passes_comma_lists_and_semicolon_options_to_the_plugin() {
    let raw = serde_json::to_string(&bar_cliproxyapi::render::ext_info()).unwrap();
    let info = plugin::parse_info(&raw).unwrap();
    let hop_bar::format::Token::Placeholder { options, .. } =
        hop_bar::format::parse("#{cliproxyapi[windows=5h,7d;count=used;show=label,pct]}")
            .unwrap()
            .pop()
            .unwrap()
    else {
        panic!()
    };
    let v = plugin::validate(&info.options, &options).unwrap();
    assert_eq!(v["windows"], "5h,7d");
    assert_eq!(v["count"], "used");
    assert_eq!(v["show"], "label,pct");
}
