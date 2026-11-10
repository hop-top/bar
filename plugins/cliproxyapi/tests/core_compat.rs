#![cfg(unix)]
//! The real binary, discovered beside `bar` and driven by bar's own plugin
//! code: what `--ext-info`, `--check` and a render print must be what the
//! core parses, so drift between the two fails here.

mod fake;

use fake::{ok, Server};
use hop_bar::cache::{Cache, Job};
use hop_bar::conf::parse;
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use serde_json::json;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
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

#[test]
fn core_parses_what_the_plugin_prints_and_renders_it() {
    let listing = json!({"files":[
        {"auth_index":"ix1","type":"claude","email":"a@corp.test",
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
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
            "instance {}\n key-file {}\n alias \"*\" me\n",
            server.origin(),
            key.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).unwrap();
    // the plugin and bar run in this process's environment
    std::env::set_var("BAR_CLIPROXYAPI_CONFIG", &conf);
    std::env::set_var("BAR_STATE_DIR", &state);
    std::env::set_var("ANTHROPIC_BASE_URL", server.origin());

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    assert_eq!(dirs, std::slice::from_ref(&bin));
    let found = plugin::find(&dirs, "cliproxyapi").expect("discovered beside bar");

    // --ext-info
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

    // --check: the core reads what the plugin prints
    let warns = plugin::check::parse_warnings(&run(&found.path, Some("--check"), ""));
    assert!(warns.iter().all(|w| !w.is_empty()), "{warns:?}");

    // a render through the core's own input, cache and painter
    let cfg = parse("").unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!("info job")
    };
    cache
        .store(&ikey, &run(path, Some("--ext-info"), ""))
        .unwrap();
    let session = Session {
        model_id: "claude-opus-4".into(),
        model: "Opus".into(),
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
    let plan = plugin::plan(&ctx, &found, &inf, &Options::default()).expect("plan");
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!("render job")
    };
    // the core hands the declared environment to the plugin
    assert!(input.contains(&server.origin()), "{input}");
    let text = run(path, None, input);
    let out = plugin::parse_output(&text).expect("render output");
    assert!(out.text.starts_with("me 7d 50%"), "{text}");
    cache.store(&plan.key, &text).unwrap();
    let shown = plugin::render(&ctx, "cliproxyapi", &Options::default()).unwrap_or_default();
    assert!(
        hop_bar::style::strip_codes(&shown).contains("me 7d 50%"),
        "{shown:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
