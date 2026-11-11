// The session's account handed to plugins (`inputs.account`): render key,
// input and job, and `bar-claude-credits` following it across accounts.
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[allow(unused_imports)]
use plugin_fx::*;

use hop_bar::cache::{Job, Request};
use hop_bar::plugin::{self, cached_info};
use hop_bar::style::strip_codes;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const RULES: &str = "account \"*@corp.test\" work 33\naccount \"*@home.test\" personal 34\n";
const PAIRS: &str = "plugin claude-credits org.work=org-w keychain.work=kc-w \
                     org.personal=org-p keychain.personal=kc-p";

/// Every test here points `CLAUDE_CONFIG_DIR` (and `PATH`) at fixtures.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Signs `email` in, as Claude Code's `.claude.json` records it; `None`
/// signs out.
fn sign_in(fx: &Fx, email: Option<&str>) {
    let dir = fx.root.join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(".claude.json");
    match email {
        Some(e) => {
            std::fs::write(&f, json!({"oauthAccount": {"emailAddress": e}}).to_string()).unwrap()
        }
        None => {
            let _ = std::fs::remove_file(&f);
        }
    }
    std::env::set_var("CLAUDE_CONFIG_DIR", &dir);
}

/// `bar-claude-credits` installed beside the fixtures, with fake
/// `security` and `curl` first on `PATH`: the balance in cents is the
/// file `<org>` in `fake`, and both log their arguments there.
fn credits_setup(fx: &Fx) -> std::path::PathBuf {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/bar-claude-credits");
    install(&script, &fx.bin.join("bar-claude-credits"));
    let fake = fx.root.join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    let exe = |name: &str, body: String| {
        let p = fake.join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    };
    let d = fake.display();
    exe(
        "security",
        format!("#!/bin/sh\necho \"$*\" >> '{d}/security.log'\necho synthetic-cookie\n"),
    );
    exe(
        "curl",
        format!(
            "#!/bin/sh\necho \"$*\" >> '{d}/curl.log'\ncat >/dev/null\n\
             for a; do url=$a; done\norg=${{url#*/organizations/}}\norg=${{org%%/*}}\n\
             printf '{{\"amount\":%s}}\\n200' \"$(cat '{d}/'\"$org\")\"\n"
        ),
    );
    for (org, cents) in [("org-w", "100"), ("org-p", "200"), ("org-f", "300")] {
        std::fs::write(fake.join(org), cents).unwrap();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(fake.clone()).chain(std::env::split_paths(&path));
    std::env::set_var("PATH", std::env::join_paths(dirs).unwrap());
    fake
}

fn restore_path(fake: &Path) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let rest: Vec<_> = std::env::split_paths(&path).filter(|p| p != fake).collect();
    std::env::set_var("PATH", std::env::join_paths(rest).unwrap());
}

fn credits_shown(fx: &Fx, c: &hop_bar::conf::Config) -> String {
    fx.prime("claude-credits", c, &[]);
    strip_codes(&fx.show("claude-credits", c, &[]))
}

#[test]
fn two_sessions_on_different_accounts_get_their_own_credits() {
    let _g = env_lock();
    let fx = Fx::new();
    let fake = credits_setup(&fx);
    let c = cfg(&format!("{RULES}{PAIRS} org=org-f keychain=kc-f\n"));
    for (email, want) in [
        ("alice@corp.test", "$1.00"),
        ("bob@home.test", "$2.00"),
        // no `account` line: the plain pair
        ("carol@else.test", "$3.00"),
        ("alice@corp.test", "$1.00"),
    ] {
        sign_in(&fx, Some(email));
        assert_eq!(credits_shown(&fx, &c), want, "{email}");
    }
    let sec = std::fs::read_to_string(fake.join("security.log")).unwrap();
    assert!(sec.contains("-s kc-w") && sec.contains("-s kc-p") && sec.contains("-s kc-f"));
    restore_path(&fake);
}

#[test]
fn an_account_without_a_pair_or_fallback_shows_nothing() {
    let _g = env_lock();
    let fx = Fx::new();
    let fake = credits_setup(&fx);
    let c = cfg(&format!("{RULES}{PAIRS}\n"));
    sign_in(&fx, Some("alice@corp.test"));
    assert_eq!(credits_shown(&fx, &c), "$1.00");
    for who in [Some("carol@else.test"), None] {
        sign_in(&fx, who);
        assert_eq!(credits_shown(&fx, &c), "", "{who:?}");
    }
    let curl = std::fs::read_to_string(fake.join("curl.log")).unwrap();
    assert_eq!(
        curl.matches("/organizations/").count(),
        1,
        "only alice's org asked: {curl}"
    );
    restore_path(&fake);
}

#[test]
fn the_account_source_names_the_account() {
    let _g = env_lock();
    let fx = Fx::new();
    let fake = credits_setup(&fx);
    // signed in as personal; the proxy says work
    sign_in(&fx, Some("bob@home.test"));
    let c = cfg(&format!("set account-source echo\n{RULES}{PAIRS}\n"));
    for (account, want) in [
        // a plugin that matched the rules itself: its label as is
        (json!({"id": "a1", "label": "work", "color": "33"}), "$1.00"),
        // one that did not: the rules, against its label
        (json!({"id": "a2", "label": "x@home.test"}), "$2.00"),
        (json!({"id": "a3", "label": "x@corp.test"}), "$1.00"),
    ] {
        fx.put(
            "out.json",
            &json!({"text": "", "account": account}).to_string(),
        );
        fx.prime("echo", &c, &[]);
        assert_eq!(credits_shown(&fx, &c), want, "{account}");
    }
    restore_path(&fake);
}

fn plan_echo(fx: &Fx, conf: &str) -> plugin::Plan {
    let (cache, dirs, c) = (fx.cache(), fx.dirs(), cfg(conf));
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let s = session();
    plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap()
}

fn input_of(p: &plugin::Plan) -> Value {
    let Job::PluginRender { input, .. } = &p.job else {
        panic!("{:?}", p.job)
    };
    serde_json::from_str(input).unwrap()
}

/// Runs the plan's job as a refresh process does and returns what the
/// plugin got on stdin.
fn run_job(fx: &Fx, p: &plugin::Plan) -> Value {
    let req = Request {
        key: p.key.clone(),
        job: p.job.clone(),
        lock: String::new(),
    };
    fx.cache().refresh(&req, Duration::from_secs(20));
    serde_json::from_str(&fx.file("stdin.json")).unwrap()
}

#[test]
fn the_account_splits_the_key_and_no_email_travels_in_the_request() {
    let _g = env_lock();
    let fx = Fx::new();
    fx.put("ext.json", &echo_info(json!({"inputs": {"account": true}})));
    fx.prime_info("echo");
    assert!(fx.ext_info("echo").inputs.account);
    sign_in(&fx, Some("alice@corp.test"));
    let a = plan_echo(&fx, RULES);
    sign_in(&fx, Some("bob@home.test"));
    let b = plan_echo(&fx, RULES);
    assert_ne!(a.key, b.key);
    assert_eq!(input_of(&b)["account"], json!({"label": "personal"}));
    for p in [&a, &b] {
        let req = Request {
            key: p.key.clone(),
            job: p.job.clone(),
            lock: String::new(),
        }
        .to_json();
        assert!(!req.contains(".test"), "argv carries no address: {req}");
    }
    // the job adds the address itself, for the account it was planned for
    assert_eq!(
        run_job(&fx, &b)["account"],
        json!({"label": "personal", "email": "bob@home.test"})
    );
    sign_in(&fx, Some("carol@else.test"));
    let c = plan_echo(&fx, RULES);
    assert_eq!(input_of(&c)["account"], json!({}), "no rule, no label");
    assert_eq!(
        run_job(&fx, &c)["account"],
        json!({"email": "carol@else.test"})
    );
    // switched between plan and run: no address rather than another's
    assert_eq!(run_job(&fx, &a)["account"], json!({"label": "work"}));
    sign_in(&fx, None);
    let none = plan_echo(&fx, RULES);
    assert!(input_of(&none).get("account").is_none());
    assert!(run_job(&fx, &none).get("account").is_none());
}

#[test]
fn a_plugin_that_did_not_declare_it_gets_no_account_and_keeps_its_key() {
    let _g = env_lock();
    let fx = Fx::new();
    fx.prime_info("echo");
    sign_in(&fx, Some("alice@corp.test"));
    let a = plan_echo(&fx, RULES);
    sign_in(&fx, Some("bob@home.test"));
    let b = plan_echo(&fx, RULES);
    assert_eq!(a.key, b.key);
    assert!(input_of(&a).get("account").is_none());
    assert!(run_job(&fx, &a).get("account").is_none());
}
