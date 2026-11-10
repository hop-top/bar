//! Collection against a fake proxy and RouteLLM, synthetic data only.

mod common;
mod fake;

use bar_plugin::keyguard::{Guard, AUTH_HOLD};
use bar_plugin::secret::Secret;
use bar_routellm::cache::{self, Problem};
use bar_routellm::collect::{collect, Ctx, Snapshot, Status, MGMT_FRESH, OFF_HOLD};
use common::*;
use fake::{cpa, entry, routellm, status, Server, HEADER_SECRET, SECRET};
use serde_json::json;

fn run(e: &Env, key: &str, now: i64, want_tiers: bool) -> Snapshot {
    collect(&Ctx {
        inst: &e.inst,
        key: &Secret::new(key.as_bytes().to_vec()),
        state_dir: &e.dir,
        now,
        want_tiers,
        force: false,
    })
}

#[test]
fn reads_the_entry_the_aliases_the_rows_and_routellm() {
    let e = Env::new("ok", false, healthy(), 1000);
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::Ok);
    let en = s.cache.entry.unwrap();
    assert!(!en.disabled);
    assert_eq!(en.priority, 1000);
    assert_eq!(en.aliases.len(), 2);
    assert_eq!(s.cache.rows.len(), 1);
    assert_eq!(s.cache.up, Some(true));
    assert_eq!(s.cache.tiers.len(), 3);
    assert_eq!(s.cache.tiers[0].context, Some(200000));
    assert_eq!(s.cache.problem, None);
    // at most 3 proxy calls and 2 RouteLLM calls
    assert_eq!(
        e.proxy.server.paths(),
        [
            "/v0/management/openai-compatibility",
            "/v0/management/oauth-model-alias"
        ]
    );
    assert_eq!(e.rl.paths(), ["/health", "/v1/models"]);
    // every proxy request carried the key
    assert!(e
        .proxy
        .server
        .log
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.auth.eq_ignore_ascii_case(&format!("bearer {KEY}"))));
}

#[test]
fn nothing_secret_reaches_the_cache() {
    let e = Env::new("privacy", false, healthy(), 1000);
    run(&e, KEY, T0, false);
    let text = std::fs::read_to_string(e.cache_path()).unwrap();
    for secret in [
        SECRET,
        HEADER_SECRET,
        KEY,
        "api-key",
        "headers",
        "X-Secret",
        "ab12",
    ] {
        assert!(!text.contains(secret), "cache leaks {secret}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            (m(&e.cache_path()), m(e.cache_path().parent().unwrap())),
            (0o600, 0o700)
        );
    }
}

#[test]
fn routellm_is_not_asked_while_routing_is_off() {
    let e = Env::new("off", true, healthy(), 1000);
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::Ok);
    assert!(s.cache.entry.unwrap().disabled);
    assert_eq!(e.rl.count(), 0);
    // unless tiers are wanted
    run(&e, KEY, T0 + 100, true);
    assert_eq!(e.rl.paths(), ["/health", "/v1/models"]);
}

#[test]
fn routellm_down_is_recorded_without_backing_off_the_proxy() {
    let e = Env::new("down", false, routellm(false, &[]), 1000);
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::Ok);
    assert_eq!(s.cache.up, Some(false));
    assert_eq!(s.cache.retry_at, 0);
    assert_eq!(s.cache.fails, 0);
    assert!(s.cache.tiers.is_empty());
}

#[test]
fn the_proxy_is_read_at_most_every_thirty_seconds() {
    let e = Env::new("fresh", false, healthy(), 1000);
    run(&e, KEY, T0, false);
    let n = e.proxy.server.count();
    run(&e, KEY, T0 + MGMT_FRESH - 1, false);
    assert_eq!(e.proxy.server.count(), n);
    run(&e, KEY, T0 + MGMT_FRESH, false);
    assert_eq!(e.proxy.server.count(), n + 2);
}

#[test]
fn a_missing_entry_or_a_foreign_base_url_is_not_drivable() {
    let proxy = cpa(json!([]), KEY);
    let e = Env::with("none", proxy, healthy());
    assert_eq!(
        run(&e, KEY, T0, false).cache.problem,
        Some(Problem::Unconfigured)
    );
    let other = cpa(json!([entry(1, false, 1000)]), KEY);
    let e = Env::with("mismatch", other, healthy());
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.cache.problem, Some(Problem::Mismatch));
    // RouteLLM is not probed for an entry that points elsewhere
    assert_eq!(e.rl.count(), 0);
}

#[test]
fn auth_failure_stops_all_calls_until_the_key_changes() {
    for code in [401u16, 403] {
        let s = Server::start(move |_| status(code));
        let e = Env::with(&format!("auth{code}"), fake::Cpa::bare(s), healthy());
        assert_eq!(run(&e, KEY, T0, false).status, Status::AuthStopped(code));
        assert_eq!(e.proxy.server.count(), 1, "one failed attempt");
        for dt in [1, 600, AUTH_HOLD - 1] {
            assert_eq!(
                run(&e, KEY, T0 + dt, false).status,
                Status::AuthStopped(code)
            );
        }
        assert_eq!(e.proxy.server.count(), 1);
        // a changed key is tried once
        run(&e, "another-key-BBBB", T0 + 10, false);
        assert_eq!(e.proxy.server.count(), 2);
        // after the hold, the same key again
        run(&e, "another-key-BBBB", T0 + 10 + AUTH_HOLD, false);
        assert_eq!(e.proxy.server.count(), 3);
        let g = std::fs::read_to_string(
            Guard {
                state_dir: &e.dir,
                mgmt: &e.inst.management,
            }
            .file(),
        )
        .unwrap();
        assert!(!g.contains(KEY) && !g.contains("another-key"));
    }
}

#[test]
fn an_empty_key_is_never_sent() {
    let e = Env::new("empty", false, healthy(), 1000);
    let s = run(&e, "", T0, false);
    assert_eq!(e.proxy.server.count(), 0);
    assert_eq!(s.status, Status::NoKey);
}

#[test]
fn a_refusal_recorded_for_the_proxy_by_another_plugin_is_honoured() {
    let e = Env::new("shared", false, healthy(), 1000);
    // what bar-cliproxyapi (v8 base, its own call) leaves
    let g = Guard {
        state_dir: &e.dir,
        mgmt: &format!("{}/v8/management", e.proxy.server.origin()),
    };
    g.stop(401, &Secret::new(KEY.as_bytes().to_vec()), T0);
    assert_eq!(run(&e, KEY, T0 + 1, false).status, Status::AuthStopped(401));
    assert_eq!(e.proxy.server.count(), 0);
}

#[test]
fn a_refusal_in_an_older_cliproxyapi_cache_is_honoured() {
    let e = Env::new("legacy", false, healthy(), 1000);
    let salt = "0123456789abcdef0123456789abcdef";
    let fp = bar_plugin::keysource::fingerprint(&Secret::new(KEY.as_bytes().to_vec()), salt);
    let legacy = e.dir.join("plugins").join("cliproxyapi").join(format!(
        "{}.json",
        bar_plugin::sha256::hex(e.proxy.server.origin().as_bytes())
    ));
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(
        &legacy,
        json!({"v":1,"salt":salt,"accounts":[],"auth_fail":{"status":403,"at":T0,"fp":fp}})
            .to_string(),
    )
    .unwrap();
    assert_eq!(run(&e, KEY, T0 + 5, false).status, Status::AuthStopped(403));
    assert_eq!(e.proxy.server.count(), 0);
}

#[test]
fn management_off_is_remembered_for_an_hour() {
    let s = Server::start(|_| status(404));
    let e = Env::with("404", fake::Cpa::bare(s), healthy());
    assert_eq!(run(&e, KEY, T0, false).status, Status::Off);
    assert_eq!(run(&e, KEY, T0 + OFF_HOLD - 1, false).status, Status::Off);
    assert_eq!(e.proxy.server.count(), 1);
    run(&e, KEY, T0 + OFF_HOLD, false);
    assert_eq!(e.proxy.server.count(), 2);
}

#[test]
fn transient_failures_back_off_and_serve_the_last_good_state() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let good = Arc::new(AtomicBool::new(true));
    let (g2, rl) = (good.clone(), healthy());
    let port = rl.port;
    let s = Server::start(move |r| {
        if !g2.load(Ordering::SeqCst) {
            return status(503);
        }
        match r.path.as_str() {
            "/v0/management/openai-compatibility" => {
                fake::ok(json!({"openai-compatibility":[entry(port,false,1000)]}).to_string())
            }
            _ => fake::ok(r#"{"oauth-model-alias":{}}"#),
        }
    });
    let e = Env::with("transient", fake::Cpa::bare(s), rl);
    assert_eq!(run(&e, KEY, T0, false).status, Status::Ok);
    good.store(false, Ordering::SeqCst);
    let s = run(&e, KEY, T0 + 100, false);
    assert_eq!(s.status, Status::Backoff);
    assert!(s.cache.entry.is_some(), "last good state is served");
    let n = e.proxy.server.count();
    assert_eq!(run(&e, KEY, T0 + 100 + 59, false).status, Status::Backoff);
    assert_eq!(e.proxy.server.count(), n);
    good.store(true, Ordering::SeqCst);
    assert_eq!(run(&e, KEY, T0 + 100 + 60, false).status, Status::Ok);
    assert_eq!(cache::load(&e.cache_path()).fails, 0);
}

#[test]
fn nothing_is_sent_when_the_state_directory_cannot_be_written() {
    let mut e = Env::new("readonly", false, healthy(), 1000);
    let f = e.dir.join("not-a-dir");
    std::fs::write(&f, "x").unwrap();
    e.dir = f;
    let s = run(&e, KEY, T0, false);
    assert_ne!(s.status, Status::Ok);
    assert_eq!(
        e.proxy.server.count(),
        0,
        "a refusal could not be remembered"
    );
}

#[test]
fn nothing_is_sent_while_another_process_holds_the_proxy() {
    let e = Env::new("locked", false, healthy(), 1000);
    let g = Guard {
        state_dir: &e.dir,
        mgmt: &e.inst.management,
    };
    let _held = g.lock(None).unwrap();
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::Backoff);
    assert_eq!(e.proxy.server.count(), 0);
}

fn forced(e: &Env, now: i64) -> Snapshot {
    collect(&Ctx {
        inst: &e.inst,
        key: &Secret::new(KEY.as_bytes().to_vec()),
        state_dir: &e.dir,
        now,
        want_tiers: true,
        force: true,
    })
}

#[test]
fn an_explicit_probe_goes_past_the_holds_and_clears_them() {
    let e = Env::new("force-holds", false, healthy(), 1000);
    for (off_until, retry_at) in [(T0 + 3000, 0), (0, T0 + 3000)] {
        let c = cache::Cache {
            off_until,
            retry_at,
            fails: 3,
            ..Default::default()
        };
        cache::save(&e.cache_path(), &c).unwrap();
        let n = e.proxy.server.count();
        // a render (no force) honours the hold
        assert_ne!(run(&e, KEY, T0, false).status, Status::Ok);
        assert_eq!(e.proxy.server.count(), n);
        // `status --probe` / `tiers` do not
        let s = forced(&e, T0);
        assert_eq!(s.status, Status::Ok);
        assert!(e.proxy.server.count() > n);
        let c = cache::load(&e.cache_path());
        assert_eq!((c.off_until, c.retry_at, c.fails), (0, 0, 0));
    }
}

#[test]
fn an_explicit_probe_still_honours_a_refused_key() {
    let e = Env::new("force-auth", false, healthy(), 1000);
    Guard {
        state_dir: &e.dir,
        mgmt: &e.inst.management,
    }
    .stop(401, &Secret::new(KEY.as_bytes().to_vec()), T0);
    assert_eq!(forced(&e, T0 + 1).status, Status::AuthStopped(401));
    assert_eq!(e.proxy.server.count(), 0);
}

#[test]
fn routellm_is_probed_with_no_lock_held() {
    let cell = Default::default();
    let (rl, seen) = rl_watching_locks(std::sync::Arc::clone(&cell));
    let e = Env::new("probe-unlocked", false, rl, 1000);
    *cell.lock().unwrap() = Some(lock_paths(&e));
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::Ok);
    assert_eq!(s.cache.up, Some(true));
    assert_eq!(*seen.lock().unwrap(), [false], "locks held while probing");
    // the probe's result is in the saved cache
    let c = cache::load(&e.cache_path());
    assert_eq!((c.up, c.up_at, c.tiers.len()), (Some(true), T0, 2));
    // a later probe with a fresh proxy read takes no management lock at all
    let _held = Guard {
        state_dir: &e.dir,
        mgmt: &e.inst.management,
    }
    .lock(None)
    .unwrap();
    let s = run(&e, KEY, T0 + 20, false);
    assert_eq!(s.status, Status::Ok);
    assert_eq!(s.cache.up_at, T0 + 20);
}

#[test]
fn a_refusal_that_cannot_be_remembered_holds_the_proxy_in_the_cache() {
    let cell: std::sync::Arc<std::sync::Mutex<Option<std::path::PathBuf>>> = Default::default();
    let c2 = cell.clone();
    let s = Server::start(move |_| {
        // the guard's record cannot be written from now on: a directory
        // sits where the file goes
        if let Some(f) = c2.lock().unwrap().clone() {
            let _ = std::fs::create_dir_all(f.join("x"));
        }
        status(401)
    });
    let e = Env::with("unrecorded", fake::Cpa::bare(s), healthy());
    let g = Guard {
        state_dir: &e.dir,
        mgmt: &e.inst.management,
    };
    *cell.lock().unwrap() = Some(g.file());
    let s = run(&e, KEY, T0, false);
    assert_eq!(s.status, Status::AuthStopped(401));
    assert!(g
        .refusal(&Secret::new(KEY.as_bytes().to_vec()), T0 + 1)
        .is_none());
    assert_eq!(e.proxy.server.count(), 1);
    // the plugin's own cache stops further calls
    assert_ne!(run(&e, KEY, T0 + 5, false).status, Status::Ok);
    assert_eq!(e.proxy.server.count(), 1, "the refused key is sent again");
}

#[test]
fn a_private_alias_with_a_claude_row_is_named() {
    use bar_routellm::cache::{Alias, Cache, Entry, Row};
    let al = |a: &str, t: &str| Alias {
        alias: a.into(),
        tier: t.into(),
    };
    let row = |a: &str| Row {
        alias: a.into(),
        model: "claude-x".into(),
    };
    let mut c = Cache {
        entry: Some(Entry {
            disabled: false,
            priority: 1,
            aliases: vec![al("route-auto", "auto"), al("route-private", "private")],
        }),
        rows: vec![row("route-auto")],
        ..Default::default()
    };
    assert!(bar_routellm::collect::private_with_row(&c).is_empty());
    c.rows.push(row("route-private"));
    assert_eq!(
        bar_routellm::collect::private_with_row(&c),
        ["route-private"]
    );
}
