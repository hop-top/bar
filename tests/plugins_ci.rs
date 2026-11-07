//! bar-ci, bar-mem, bar-health driven directly with fakes first on `PATH`.
mod common;
#[path = "common/plugins_fx.rs"]
mod plugins_fx;
#[allow(unused_imports)]
use plugins_fx::*;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output};

fn ext_info(name: &str) -> Value {
    let o = Command::new(script(name))
        .arg("--ext-info")
        .output()
        .unwrap();
    assert!(o.status.success());
    out_json(&o)
}

fn assert_placeholder(v: &Value, name: &str) {
    assert_eq!(v["name"], name);
    assert!(v["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("placeholder")));
    assert!(v["ttl"].is_string());
}

// ---- bar-ci ----

fn ci_fakes(fx: &Fx, runs: &str, gh_code: i32) {
    exe(
        &fx.bin().join("gh"),
        &format!(
            "#!/bin/sh\necho \"$PWD|$*\" >> \"$FAKE_DIR/gh.log\"\ncat \"$FAKE_DIR/runs.json\"\nexit {gh_code}\n"
        ),
    );
    exe(
        &fx.bin().join("git"),
        "#!/bin/sh\necho \"$*\" >> \"$FAKE_DIR/git.log\"\n[ -f \"$FAKE_DIR/norepo\" ] && exit 128\nif [ \"$1 $2 $3\" = \"-C $PWD remote\" ] || [ \"$3\" = remote ]; then [ -f \"$FAKE_DIR/noremote\" ] || echo origin; exit 0; fi\ncat \"$FAKE_DIR/branch\"\n",
    );
    fx.put("runs.json", runs);
    fx.put("branch", "feat/x\n");
}

fn ci(fx: &Fx, options: Value) -> Output {
    let cwd: PathBuf = fx.cwd.clone();
    fx.run(
        "bar-ci",
        &json!({"options":options,"session":{"cwd":cwd},"columns":null,"config":{}}),
    )
}

fn run_json(status: &str, conclusion: &str) -> String {
    json!([{"status":status,"conclusion":conclusion,"url":"https://github.com/o/r/actions/runs/1","workflowName":"CI"}])
        .to_string()
}

#[test]
fn bar_ci_ext_info() {
    let v = ext_info("bar-ci");
    assert_placeholder(&v, "ci");
    assert_eq!(v["options"]["name"]["type"], "bool");
}

#[test]
fn bar_ci_states() {
    let fx = Fx::new();
    ci_fakes(&fx, &run_json("completed", "success"), 0);
    let v = out_json(&ci(&fx, json!({})));
    assert_eq!(v["text"], "\u{2713}");
    assert_eq!(v["score"], 100);
    assert_eq!(v["link"], "https://github.com/o/r/actions/runs/1");
    fx.put("runs.json", &run_json("completed", "failure"));
    let v = out_json(&ci(&fx, json!({})));
    assert_eq!(v["text"], "\u{2717}");
    assert_eq!(v["score"], 0);
    fx.put("runs.json", &run_json("in_progress", ""));
    let v = out_json(&ci(&fx, json!({})));
    assert_eq!(v["text"], "\u{2026}");
    assert_eq!(v["score"], 50);
}

#[test]
fn bar_ci_asks_gh_for_branch_in_cwd() {
    let fx = Fx::new();
    ci_fakes(&fx, &run_json("completed", "success"), 0);
    ci(&fx, json!({}));
    let log = fx.log("gh.log");
    assert!(log.contains("run list --branch feat/x --limit 1"), "{log}");
    assert!(
        log.contains("--json status,conclusion,url,workflowName"),
        "{log}"
    );
    assert!(log.contains("neutral-cwd"), "{log}");
}

#[test]
fn bar_ci_name_option_adds_workflow() {
    let fx = Fx::new();
    ci_fakes(&fx, &run_json("completed", "success"), 0);
    let v = out_json(&ci(&fx, json!({"name":true})));
    assert_eq!(v["text"], "\u{2713} CI");
    let v = out_json(&ci(&fx, json!({"name":false})));
    assert_eq!(v["text"], "\u{2713}");
}

#[test]
fn bar_ci_hidden_without_runs_repo_or_gh() {
    let fx = Fx::new();
    ci_fakes(&fx, "[]", 0);
    let o = ci(&fx, json!({}));
    assert!(o.status.success());
    assert_eq!(out_json(&o)["text"], "");
    ci_fakes(&fx, &run_json("completed", "success"), 0);
    fx.put("norepo", "");
    std::fs::remove_file(fx.fake.join("gh.log")).unwrap();
    let o = ci(&fx, json!({}));
    assert!(o.status.success());
    assert_eq!(out_json(&o)["text"], "");
    assert!(!fx.log("gh.log").contains("run list"));
    // no remote: hidden, gh never asked
    ci_fakes(&fx, &run_json("completed", "success"), 0);
    fx.put("noremote", "");
    let _ = std::fs::remove_file(fx.fake.join("gh.log"));
    let o = ci(&fx, json!({}));
    assert!(o.status.success());
    assert_eq!(out_json(&o)["text"], "");
    assert!(!fx.log("gh.log").contains("run list"));
    // no gh on PATH
    let mut fx = Fx::new();
    ci_fakes(&fx, "[]", 0);
    std::fs::remove_file(fx.bin().join("gh")).unwrap();
    let keep: Vec<_> = fx
        .env
        .iter()
        .filter(|(k, _)| *k != "PATH")
        .cloned()
        .collect();
    fx.env = keep;
    fx.env.push((
        "PATH",
        format!("{}:/usr/bin:/bin", fx.bin().display()).into(),
    ));
    if !std::path::Path::new("/usr/bin/gh").exists() {
        let o = ci(&fx, json!({}));
        assert!(o.status.success());
        assert_eq!(out_json(&o)["text"], "");
    }
}

#[test]
fn bar_ci_gh_failure_keeps_cache() {
    let fx = Fx::new();
    ci_fakes(&fx, "", 1);
    let o = ci(&fx, json!({}));
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
}

#[test]
fn bar_ci_sanitizes_and_drops_bad_link() {
    let fx = Fx::new();
    let runs = json!([{"status":"completed","conclusion":"success","url":"javascript:x","workflowName":"C\u{1b}[31mI\n"}]);
    ci_fakes(&fx, &runs.to_string(), 0);
    let v = out_json(&ci(&fx, json!({"name":true})));
    assert_eq!(v["text"], "\u{2713} CI");
    assert!(v.get("link").is_none() || v["link"].is_null());
}

// ---- bar-mem ----

fn mem(fx: &Fx) -> Output {
    fx.run(
        "bar-mem",
        &json!({"options":{},"session":{},"columns":null,"config":{}}),
    )
}

#[test]
fn bar_mem_ext_info() {
    assert_placeholder(&ext_info("bar-mem"), "mem");
}

#[test]
fn bar_mem_macos_memory_pressure() {
    let fx = Fx::new();
    exe(&fx.bin().join("uname"), "#!/bin/sh\necho Darwin\n");
    exe(
        &fx.bin().join("memory_pressure"),
        "#!/bin/sh\nprintf 'The system has 17179869184 (1048576 pages)\\nSystem-wide memory free percentage: 37%%\\n'\n",
    );
    let v = out_json(&mem(&fx));
    assert_eq!(v["text"], "mem 63%");
    assert_eq!(v["score"], 37);
}

#[test]
fn bar_mem_macos_vm_stat_fallback() {
    let fx = Fx::new();
    exe(&fx.bin().join("uname"), "#!/bin/sh\necho Darwin\n");
    exe(&fx.bin().join("memory_pressure"), "#!/bin/sh\nexit 1\n");
    exe(
        &fx.bin().join("vm_stat"),
        "#!/bin/sh\ncat <<'X'\nMach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                               100.\nPages active:                             300.\nPages inactive:                           200.\nPages speculative:                        100.\nPages wired down:                         300.\nPages occupied by compressor:             100.\nX\n",
    );
    // used = active+wired+compressor = 700 of 1100 total = 63.6, rounds to 64%
    let v = out_json(&mem(&fx));
    assert_eq!(v["text"], "mem 64%");
}

#[test]
fn bar_mem_linux_meminfo() {
    let fx = Fx::new();
    exe(&fx.bin().join("uname"), "#!/bin/sh\necho Linux\n");
    fx.put(
        "meminfo",
        "MemTotal:       1000 kB\nMemFree:          10 kB\nMemAvailable:    250 kB\n",
    );
    let mut fx = fx;
    let p = fx.fake.join("meminfo").display().to_string();
    fx.extra.push(("BAR_MEMINFO", p));
    let v = out_json(&mem(&fx));
    assert_eq!(v["text"], "mem 75%");
    assert_eq!(v["score"], 25);
}

#[test]
fn bar_mem_unreadable_fails_quietly() {
    let fx = Fx::new();
    exe(&fx.bin().join("uname"), "#!/bin/sh\necho Linux\n");
    let mut fx = fx;
    let p = fx.fake.join("missing").display().to_string();
    fx.extra.push(("BAR_MEMINFO", p));
    let o = mem(&fx);
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
}

// ---- bar-health ----

fn health_fake(fx: &Fx, code: &str, exit: i32) {
    exe(
        &fx.bin().join("curl"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"$FAKE_DIR/curl.log\"\ncat >> \"$FAKE_DIR/curl.stdin\"\nprintf '%s' '{code}'\nexit {exit}\n"
        ),
    );
}

fn health(fx: &Fx, options: Value) -> Output {
    fx.run(
        "bar-health",
        &json!({"options":options,"session":{},"columns":null,"config":{}}),
    )
}

#[test]
fn bar_health_ext_info() {
    let v = ext_info("bar-health");
    assert_placeholder(&v, "health");
    assert_eq!(v["options"]["url"]["type"], "string");
    assert_eq!(v["options"]["label"]["type"], "string");
}

#[test]
fn bar_health_up_and_down() {
    let fx = Fx::new();
    health_fake(&fx, "204", 0);
    let o = health(&fx, json!({"url":"https://example.com/h","label":"api"}));
    let v = out_json(&o);
    assert_eq!(v["text"], "\u{2713}");
    assert_eq!(v["label"], "api");
    assert_eq!(v["link"], "https://example.com/h");
    assert_eq!(v["style"], "good");
    let log = fx.log("curl.log");
    assert!(log.contains("--max-time 2"), "{log}");
    assert!(log.contains("-I"), "{log}");
    assert!(
        !log.contains("example.com"),
        "url must not be in argv: {log}"
    );
    assert!(fx.log("curl.stdin").contains("https://example.com/h"));
    health_fake(&fx, "503", 0);
    let v = out_json(&health(&fx, json!({"url":"https://example.com/h"})));
    assert_eq!(v["text"], "\u{2717}");
    assert_eq!(v["style"], "bad");
    health_fake(&fx, "000", 28);
    let v = out_json(&health(&fx, json!({"url":"http://example.com"})));
    assert_eq!(v["text"], "\u{2717}");
}

#[test]
fn bar_health_refuses_bad_urls() {
    let fx = Fx::new();
    health_fake(&fx, "200", 0);
    for u in [
        "file:///etc/passwd",
        "ftp://x",
        "javascript:alert(1)",
        "https://user:pw@example.com/",
        "https://exa mple.com",
        "https://example.com/\"x",
        "https://example.com/\nx",
        "",
        "-oops",
    ] {
        let o = health(&fx, json!({"url":u}));
        assert!(!o.status.success(), "{u:?}");
        assert!(o.stdout.is_empty(), "{u:?}");
    }
    let o = fx.run(
        "bar-health",
        &json!({"options":{},"session":{},"columns":null,"config":{}}),
    );
    assert!(!o.status.success());
    assert!(fx.log("curl.log").is_empty());
}

#[test]
fn bar_health_sanitizes_label() {
    let fx = Fx::new();
    health_fake(&fx, "200", 0);
    let v = out_json(&health(
        &fx,
        json!({"url":"https://example.com","label":"a\u{1b}[31mb\u{202e}c\u{1b}]8;;http://x\u{7}d"}),
    ));
    assert_eq!(v["label"], "abcd");
}
