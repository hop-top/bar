//! The toggle against a fake proxy that behaves as the real v0 routes.

mod common;
mod fake;

use bar_plugin::secret::Secret;
use bar_routellm::toggle::{run, Req};
use common::*;
use fake::{routellm, status, Server};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

fn go(e: &Env, key: &str, to_on: bool, dry: bool, force: bool) -> Result<Vec<String>, String> {
    run(&Req {
        inst: &e.inst,
        key: &Secret::new(key.as_bytes().to_vec()),
        state_dir: &e.dir,
        now: T0,
        to_on,
        dry_run: dry,
        force,
    })
    .map_err(|r| r.0)
}

fn state(e: &Env) -> Value {
    e.proxy.entries.lock().unwrap()[0].clone()
}

#[test]
fn off_is_one_field_patch_then_a_verifying_read() {
    let e = Env::new("off", false, healthy(), 1000);
    let out = go(&e, KEY, false, false, false).unwrap();
    assert!(out.iter().any(|l| l == "routing off"), "{out:?}");
    let p = e.proxy.server.patches();
    assert_eq!(p.len(), 1);
    let body: Value = serde_json::from_str(&p[0].body).unwrap();
    assert_eq!(body, json!({"name":"routellm","value":{"disabled":true}}));
    assert_eq!(state(&e)["disabled"], true);
    // GET, PATCH, GET: nothing else, and RouteLLM never contacted for OFF
    assert_eq!(e.proxy.server.count(), 3);
    assert_eq!(e.rl.count(), 0);
    // the entry's other fields survive (the proxy's merge), secrets untouched
    assert_eq!(state(&e)["api-key-entries"][0]["api-key"], fake::SECRET);
    let c = bar_routellm::cache::load(&e.cache_path());
    assert_eq!(c.last.unwrap().to, "off");
}

#[test]
fn off_works_with_routellm_dead() {
    let e = Env::new("off-dead", false, routellm(false, &[]), 1000);
    go(&e, KEY, false, false, false).unwrap();
    assert_eq!(e.rl.count(), 0);
}

#[test]
fn on_is_gated_on_routellm_and_the_aliased_tiers() {
    let e = Env::new("gate-down", true, routellm(false, &[]), 1000);
    let err = go(&e, KEY, true, false, false).unwrap_err();
    assert!(err.contains("not answering"), "{err}");
    assert!(e.proxy.server.patches().is_empty());
    // --force goes past, and says so
    let out = go(&e, KEY, true, false, true).unwrap();
    assert!(out[0].starts_with("forced past"), "{out:?}");
    assert_eq!(state(&e)["disabled"], false);

    let e = Env::new("gate-tier", true, routellm(true, &["auto"]), 1000);
    let err = go(&e, KEY, true, false, false).unwrap_err();
    assert!(err.contains("`private`"), "{err}");
    assert!(e.proxy.server.patches().is_empty());
}

#[test]
fn on_sets_the_priority_only_when_it_is_lower() {
    let e = Env::new("prio-low", true, healthy(), 5);
    go(&e, KEY, true, false, false).unwrap();
    let b: Value = serde_json::from_str(&e.proxy.server.patches()[0].body).unwrap();
    assert_eq!(b["value"], json!({"disabled":false,"priority":1000}));
    assert_eq!(state(&e)["priority"], 1000);
    let e = Env::new("prio-ok", true, healthy(), 2000);
    go(&e, KEY, true, false, false).unwrap();
    let b: Value = serde_json::from_str(&e.proxy.server.patches()[0].body).unwrap();
    assert_eq!(b["value"], json!({"disabled":false}));
    assert_eq!(state(&e)["priority"], 2000);
}

#[test]
fn the_patch_never_carries_base_url_or_secrets() {
    for (on, dis) in [(true, true), (false, false)] {
        let e = Env::new(&format!("body{on}"), dis, healthy(), 5);
        go(&e, KEY, on, false, false).unwrap();
        let body = e.proxy.server.patches()[0].body.clone();
        assert!(!body.contains("base-url") && !body.contains("models") && !body.contains(KEY));
        // a base-url in a PATCH would delete the entry on the real proxy
        assert_eq!(e.proxy.entries.lock().unwrap().as_array().unwrap().len(), 1);
    }
}

#[test]
fn already_in_the_target_state_sends_nothing() {
    let e = Env::new("noop", true, healthy(), 1000);
    assert_eq!(go(&e, KEY, false, false, false).unwrap(), ["already off"]);
    assert!(e.proxy.server.patches().is_empty());
    let e = Env::new("noop-on", false, healthy(), 1000);
    assert_eq!(go(&e, KEY, true, false, false).unwrap(), ["already on"]);
    assert!(e.proxy.server.patches().is_empty());
    assert_eq!(e.rl.count(), 0, "no gate for a no-op");
}

#[test]
fn dry_run_sends_no_change() {
    let e = Env::new("dry", false, healthy(), 1000);
    let out = go(&e, KEY, false, true, false).unwrap();
    assert!(out.last().unwrap().starts_with("dry run: PATCH"), "{out:?}");
    assert!(e.proxy.server.patches().is_empty());
    assert_eq!(state(&e)["disabled"], false);
}

#[test]
fn a_change_that_did_not_land_is_reported() {
    let e = Env::new("nolanding", false, healthy(), 1000);
    e.proxy.apply.store(false, Ordering::SeqCst);
    let err = go(&e, KEY, false, false, false).unwrap_err();
    assert!(err.contains("still reports"), "{err}");
    assert_eq!(e.proxy.server.patches().len(), 1, "no retry loop");
}

#[test]
fn an_entry_that_is_missing_or_points_elsewhere_is_not_touched() {
    let proxy = fake::cpa(json!([]), KEY);
    let e = Env::with("missing", proxy, healthy());
    assert!(go(&e, KEY, false, false, false)
        .unwrap_err()
        .contains("no `routellm`"));
    assert!(e.proxy.server.patches().is_empty());
    let proxy = fake::cpa(json!([fake::entry(1, false, 1000)]), KEY);
    let e = Env::with("elsewhere", proxy, healthy());
    assert!(go(&e, KEY, false, false, false)
        .unwrap_err()
        .contains("does not point"));
    assert!(e.proxy.server.patches().is_empty());
}

#[test]
fn a_refused_key_costs_one_attempt_across_every_call() {
    for code in [401u16, 403] {
        let s = Server::start(move |_| status(code));
        let e = Env::with(
            &format!("ban{code}"),
            fake::Cpa {
                server: s,
                entries: Default::default(),
                apply: Default::default(),
            },
            healthy(),
        );
        for _ in 0..4 {
            assert!(go(&e, KEY, false, false, false).is_err());
        }
        assert_eq!(e.proxy.server.count(), 1);
        // the collector honours the same memory
        let s = bar_routellm::collect::collect(&bar_routellm::collect::Ctx {
            inst: &e.inst,
            key: &Secret::new(KEY.as_bytes().to_vec()),
            state_dir: &e.dir,
            now: T0 + 1,
            want_tiers: false,
            force: false,
        });
        assert_eq!(s.status, bar_routellm::collect::Status::AuthStopped(code));
        assert_eq!(e.proxy.server.count(), 1);
    }
}

#[test]
fn an_empty_key_is_never_sent() {
    let e = Env::new("emptykey", false, healthy(), 1000);
    assert_eq!(go(&e, "", false, false, false).unwrap_err(), "no key");
    assert_eq!(e.proxy.server.count(), 0);
}

#[test]
fn concurrent_toggles_serialize_on_the_lock() {
    let e = Env::new("conc", false, healthy(), 1000);
    let held = bar_plugin::keyguard::Guard {
        state_dir: &e.dir,
        mgmt: &e.inst.management,
    }
    .lock(None)
    .unwrap();
    std::thread::scope(|s| {
        let h = s.spawn(|| go(&e, KEY, false, false, false));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(e.proxy.server.count(), 0, "waits for the holder");
        drop(held);
        assert!(h.join().unwrap().is_ok());
    });
    assert_eq!(e.proxy.server.patches().len(), 1);
}

#[test]
fn nothing_is_sent_without_a_writable_state_directory() {
    let mut e = Env::new("ro", false, healthy(), 1000);
    let f = e.dir.join("f");
    std::fs::write(&f, "x").unwrap();
    e.dir = f;
    assert!(go(&e, KEY, false, false, false).is_err());
    assert_eq!(e.proxy.server.count(), 0);
}
