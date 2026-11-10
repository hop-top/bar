#![cfg(unix)]
//! A click, end to end: bar draws the placeholder as an action link from
//! the plugin's declaration and cached output, its click handler checks
//! the link and runs the real `bar-cliproxyapi --action use`, and the
//! proxy's priority changes.

mod fake;

use fake::pool;
use hop_bar::cache::{Cache, Job};
use hop_bar::click::action::{self, NONCE_FILE, STATUS_FILE};
use hop_bar::conf::parse;
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");

fn run(path: impl AsRef<std::path::Path>, arg: &str) -> String {
    let out = Command::new(path.as_ref())
        .arg(arg)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_click_switches_the_account_through_the_core_handler() {
    let files = json!([
        {"name":"a.json","auth_index":"ix-a","type":"claude","email":"alice@corp.test",
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.9",
           "Anthropic-Ratelimit-Unified-7d-Reset":"1800086400"}}},
        {"name":"b.json","auth_index":"ix-b","type":"claude","email":"bob@corp.test","priority":2,
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.1",
           "Anthropic-Ratelimit-Unified-7d-Reset":"1800086400"}}}
    ]);
    let (server, st) = pool(files, "e2e-key");
    let root = std::env::temp_dir().join(format!("cpa-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (bin, state) = (root.join("bin"), root.join("state"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let plugin_path = bin.join("bar-cliproxyapi");
    std::fs::copy(BIN, &plugin_path).unwrap();
    let key = root.join("key");
    std::fs::write(&key, "e2e-key\n").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let conf = root.join("cliproxyapi.conf");
    std::fs::write(
        &conf,
        format!(
            "instance {}\n key-file {}\n",
            server.origin(),
            key.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::env::set_var("BAR_CLIPROXYAPI_CONFIG", &conf);
    std::env::set_var("BAR_STATE_DIR", &state);
    std::env::set_var("ANTHROPIC_BASE_URL", server.origin());
    std::fs::write(state.join(NONCE_FILE), "0123456789abcdef0123456789abcdef").unwrap();
    std::fs::set_permissions(
        state.join(NONCE_FILE),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    let found = plugin::find(&dirs, "cliproxyapi").unwrap();
    let cfg = parse("action cliproxyapi cliproxyapi.use\n").unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!("info job")
    };
    cache.store(&ikey, &run(path, "--ext-info")).unwrap();
    let session = Session {
        model_id: "claude-opus-4".into(),
        ..Session::default()
    };
    let ctx = Ctx {
        session: &session,
        config: &cfg,
        now: 1_800_000_000,
        runner: &ColdRunner,
        cache: Some(&cache),
        columns: Some(120),
        plugin_dirs: &dirs,
        memo: Default::default(),
    };
    let inf = plugin::cached_info(&cache, &found).unwrap();
    let plan = plugin::plan(&ctx, &found, &inf, &Options::default()).unwrap();
    // the plugin's own render output (tokens included) is what bar stores;
    // its collector fills the snapshot the action checks
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!("render job")
    };
    let render = || {
        let mut ch = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut ch.stdin.take().unwrap(), input.as_bytes()).unwrap();
        let done = ch.wait_with_output().unwrap();
        assert!(done.status.success());
        String::from_utf8(done.stdout).unwrap()
    };
    // the first render starts the collection; the second reads its result
    render();
    let mut out = render();
    for _ in 0..50 {
        if out.contains("\"ix-a\"") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        out = render();
    }
    assert!(out.contains("\"ix-a\""), "tokens missing: {out}");
    cache.store(&plan.key, &out).unwrap();

    let shown = plugin::render(&ctx, "cliproxyapi", &Options::default()).unwrap_or_default();
    let url = shown
        .split("\x1b]8;;")
        .skip(1)
        .filter_map(|r| r.split(['\x1b', '\x07']).next())
        .find(|u| u.starts_with("bar://action/cliproxyapi.use?"))
        .unwrap_or_else(|| panic!("no action link in {shown:?}"))
        .to_string();

    action::run(&state, &url, &cfg, &dirs).unwrap();
    assert_eq!(st.lock().unwrap()[0]["priority"], 3);
    let status = std::fs::read_to_string(state.join(STATUS_FILE)).unwrap();
    assert!(status.trim_end().ends_with(" ok"), "{status:?}");
    let _ = std::fs::remove_dir_all(&root);
}
