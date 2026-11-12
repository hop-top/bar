//! bar-tlc when a helper it runs fails: it must stop, not loop.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugins_fx.rs"]
mod plugins_fx;
#[allow(unused_imports)]
use plugins_fx::*;

use serde_json::json;

/// `dirname` first on `PATH` failing with no output on its call number
/// `nth` (`0`: every call), as a substitution does when its pipe cannot be
/// made; other calls run the real one.
fn dirname_failing(fx: &Fx, nth: usize) {
    fx.put("nth", &nth.to_string());
    exe(
        &fx.bin().join("dirname"),
        r#"#!/bin/sh
n=$(($(cat "$FAKE_DIR/dirname.n" 2>/dev/null || echo 0) + 1))
echo "$n" > "$FAKE_DIR/dirname.n"
nth=$(cat "$FAKE_DIR/nth")
[ "$nth" = 0 ] || [ "$n" = "$nth" ] && exit 1
PATH=${PATH#*:} exec dirname "$@"
"#,
    );
    std::fs::remove_file(fx.fake.join("dirname.n")).ok();
}

#[test]
fn bar_tlc_failing_dirname_stops_and_emits_nothing() {
    let fx = Fx::new();
    fx.tlc_fake();
    let (_, deep) = fx.project();
    fx.tlc_task("T-0001", Some("alpha"), "IN_PROGRESS");
    fx.put("tasks_alpha.json", &tasks(1, 0, 3, 0));
    fx.put("tasks.json", &tasks(7, 2, 3, 1));
    // calls 1-2: git boundary and 3-4: project root for the session cwd
    // (each looped forever); 5-8: the same for the record's cwd
    for nth in 0..=8 {
        dirname_failing(&fx, nth);
        let session = json!({"cwd": deep, "record": {"task": "T-0001", "cwd": deep}});
        let o = fx.run_within(
            "bar-tlc",
            &json!({"options":{},"session":session,"columns":null,"config":{}}),
            20,
        );
        assert!(!o.status.success(), "call {nth}");
        assert!(
            o.stdout.is_empty(),
            "call {nth}: {:?}",
            String::from_utf8_lossy(&o.stdout)
        );
    }
    // the fake passes every other call through
    dirname_failing(&fx, 99);
    let session = json!({"cwd": deep, "record": {"task": "T-0001", "cwd": deep}});
    let o = fx.run_within(
        "bar-tlc",
        &json!({"options":{},"session":session,"columns":null,"config":{}}),
        20,
    );
    assert_eq!(out_json(&o)["text"], "1\u{2215}4");
}
