#![cfg(unix)]
//! A session without a proxy (`ANTHROPIC_BASE_URL` unset): `#{account}`
//! shows the signed-in account and links no account action, even with one
//! instance configured, since switching the proxy's account would not
//! change the account this session uses.

#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use fake::{ok, Server};
use hop_bar::cache::{Cache, Job};
use hop_bar::click::action::NONCE_FILE;
use hop_bar::conf::parse;
use hop_bar::options::Options;
use hop_bar::placeholders::{lookup, Ctx};
use hop_bar::plugin;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");

fn run(path: &str, arg: Option<&str>, stdin: &str) -> String {
    let out = deadline::run(Command::new(path).args(arg), Some(stdin.as_bytes()));
    String::from_utf8(out.stdout).unwrap()
}

/// `#{account}` rendered with `action account cliproxyapi.<action>` bound,
/// through the user's account lines, and whether the plugin declares
/// `action` at all.
fn account_bound_to(action: &str) -> (String, bool) {
    let server = Server::start(|_| ok("{}"));
    let root = std::env::temp_dir().join(format!("cpa-plain-{}-{action}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (bin, state, claude) = (root.join("bin"), root.join("state"), root.join("claude"));
    for d in [&bin, &state, &claude] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(
        claude.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"me.synthetic@local.test"}}"#,
    )
    .unwrap();
    std::fs::copy(BIN, bin.join("bar-cliproxyapi")).unwrap();
    let key = root.join("key");
    std::fs::write(&key, "plain-key\n").unwrap();
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
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
    std::env::remove_var("ANTHROPIC_BASE_URL");
    std::fs::write(state.join(NONCE_FILE), "0123456789abcdef0123456789abcdef").unwrap();
    std::fs::set_permissions(
        state.join(NONCE_FILE),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let dirs = plugin::plugin_dirs_with(None, Some(&bin.join("bar")));
    let found = plugin::find(&dirs, "cliproxyapi").unwrap();
    // account lines exist: they must not get in the way of the plain render
    let cfg = parse(&format!(
        "set account-source cliproxyapi\naction account cliproxyapi.{action}\n\
         account \"*@local.test\" mine 33\n"
    ))
    .unwrap();
    let cache = Cache::with_exe(&state, "/nonexistent/bar".into());
    let (ikey, ijob) = plugin::info_job(&found);
    let Job::PluginInfo { path } = &ijob else {
        panic!()
    };
    let info = run(path, Some("--ext-info"), "");
    let declared = serde_json::from_str::<Value>(&info).unwrap()["actions"]
        .get(action)
        .is_some();
    cache.store(&ikey, &info).unwrap();
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
    let Job::PluginRender { path, input, .. } = &plan.job else {
        panic!()
    };
    // the input has no base URL to name an instance
    assert!(!input.contains("ANTHROPIC_BASE_URL"), "{input}");
    let out = run(path, None, input);
    cache.store(&plan.key, &out).unwrap();

    let raw = lookup("account")
        .unwrap()
        .render(&ctx, &Options::default())
        .unwrap();
    // the render path draws the link around the placeholder's value
    let raw = plugin::link_account(&ctx, raw);
    assert_eq!(
        hop_bar::style::strip_codes(&raw),
        "mine",
        "the signed-in account, through the user's lines: {raw:?}"
    );
    assert_eq!(server.count(), 0, "nothing was asked of the proxy");
    let _ = std::fs::remove_dir_all(&root);
    (raw, declared)
}

#[test]
fn account_links_no_action_in_a_plain_session() {
    for action in ["next", "pick"] {
        let (raw, _) = account_bound_to(action);
        assert!(
            !raw.contains("bar://action/"),
            "{action}: a session outside the proxy gets no account link: {raw:?}"
        );
    }
}
