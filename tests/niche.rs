// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::placeholders::tmux::cache_key;
use hop_bar::transcript;
use std::time::Duration;

fn sid_stdin(sid: &str) -> String {
    format!("{{\"session_id\":\"{sid}\"}}")
}

#[test]
fn sid_defaults_to_eight_chars() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{sid}\"")
        .stdin(&sid_stdin("abcdef01-2345-6789"))
        .go();
    assert_eq!(o.plain.trim(), "abcdef01");
}

#[test]
fn sid_len_option_and_short_ids() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{sid[len=4]}|#{sid[len=99]}\"")
        .stdin(&sid_stdin("abcdef01"))
        .go();
    assert_eq!(o.plain.trim(), "abcd|abcdef01");
}

#[test]
fn sid_hidden_when_absent_or_zero() {
    let t = Tmp::new();
    let o = run(&t).conf("set left \"#{sid}\"").go();
    assert_eq!(o.plain.trim(), "");
    let o = run(&t)
        .conf("set left \"#{sid[len=0]}\"")
        .stdin(&sid_stdin("abc"))
        .go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn sid_is_sanitized() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{sid[len=20]}\"")
        .stdin(r#"{"session_id":"ab\u001b[31mcd\u0007"}"#)
        .go();
    // only the muted wrapper's own escapes remain
    assert_eq!(o.raw.matches('\x1b').count(), 2, "{:?}", o.raw);
    assert_eq!(o.plain.trim(), "ab[31mcd");
    assert!(!o.raw.contains('\u{7}'));
}

fn boundary() -> &'static str {
    "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"content\":\"Conversation compacted\",\"compactMetadata\":{\"trigger\":\"auto\"},\"timestamp\":\"2026-01-02T03:04:05.250Z\"}\n"
}

fn asst(id: &str, blocks: &str, side: bool) -> String {
    format!(
        "{{\"type\":\"assistant\",\"isSidechain\":{side},\"timestamp\":\"2026-01-02T03:04:05.250Z\",\"message\":{{\"id\":\"{id}\",\"role\":\"assistant\",\"content\":[{blocks}],\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
    )
}

fn skill(id: &str, name: &str) -> String {
    asst(
        id,
        &format!("{{\"type\":\"tool_use\",\"id\":\"t{id}\",\"name\":\"Skill\",\"input\":{{\"skill\":\"{name}\"}}}}"),
        false,
    )
}

fn tstdin(t: &Tmp) -> String {
    format!(
        "{{\"transcript_path\":\"{}\"}}",
        t.path().join("t.jsonl").display()
    )
}

#[test]
fn compactions_counts_boundaries_only() {
    let t = Tmp::new();
    // a message that merely quotes the marker, and a user line, do not count
    let quote = asst(
        "q",
        "{\"type\":\"text\",\"text\":\"\\\"subtype\\\":\\\"compact_boundary\\\"\"}",
        false,
    );
    let user = "{\"type\":\"user\",\"subtype\":\"compact_boundary\"}\n";
    t.write(
        "t.jsonl",
        &(boundary().to_string() + &quote + user + boundary()),
    );
    let o = run(&t)
        .conf("set left \"#{compactions}\"")
        .stdin(&tstdin(&t))
        .go();
    assert_eq!(o.plain.trim(), "compact 2");
}

#[test]
fn compactions_skip_sidechain_boundaries() {
    let t = Tmp::new();
    let side = boundary().replace("{\"type\"", "{\"isSidechain\":true,\"type\"");
    t.write("t.jsonl", &(boundary().to_string() + &side));
    let o = run(&t)
        .conf("set left \"#{compactions}\"")
        .stdin(&tstdin(&t))
        .go();
    assert_eq!(o.plain.trim(), "compact 1");
}

#[test]
fn compactions_hidden_at_zero_or_without_transcript() {
    let t = Tmp::new();
    t.write(
        "t.jsonl",
        &asst("a", "{\"type\":\"text\",\"text\":\"hi\"}", false),
    );
    let o = run(&t)
        .conf("set left \"#{compactions}\"")
        .stdin(&tstdin(&t))
        .go();
    assert_eq!(o.plain.trim(), "");
    let o = run(&t).conf("set left \"#{compactions}\"").go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn compactions_accumulate_incrementally() {
    let t = Tmp::new();
    t.write("t.jsonl", boundary());
    let conf = "set left \"#{compactions}\"";
    let s = tstdin(&t);
    assert_eq!(run(&t).conf(conf).stdin(&s).go().plain.trim(), "compact 1");
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(t.path().join("t.jsonl"))
        .unwrap();
    std::io::Write::write_all(&mut f, boundary().as_bytes()).unwrap();
    assert_eq!(run(&t).conf(conf).stdin(&s).go().plain.trim(), "compact 2");
}

#[test]
fn skills_counts_main_chain_skill_calls() {
    let t = Tmp::new();
    let other = asst(
        "o",
        "{\"type\":\"tool_use\",\"id\":\"x\",\"name\":\"Bash\",\"input\":{\"skill\":\"no\"}}",
        false,
    );
    let side = asst(
        "s",
        "{\"type\":\"tool_use\",\"id\":\"y\",\"name\":\"Skill\",\"input\":{\"skill\":\"sub\"}}",
        true,
    );
    t.write(
        "t.jsonl",
        &(skill("1", "a") + &other + &side + &skill("2", "b") + &skill("3", "a")),
    );
    let o = run(&t)
        .conf("set left \"#{skills}\"")
        .stdin(&tstdin(&t))
        .go();
    assert_eq!(o.plain.trim(), "skills 3");
}

#[test]
fn skills_hidden_at_zero() {
    let t = Tmp::new();
    t.write("t.jsonl", boundary());
    let o = run(&t)
        .conf("set left \"#{skills}\"")
        .stdin(&tstdin(&t))
        .go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn old_state_without_counts_is_rebuilt() {
    let t = Tmp::new();
    t.write(
        "t.jsonl",
        &(boundary().to_string() + boundary() + &user("\"hi\"", "")),
    );
    let state = t.path().join("state");
    let p = transcript::update(&state, &t.path().join("t.jsonl"), u64::MAX, None).unwrap();
    assert_eq!((p.stats.compactions, p.stats.user_msgs), (2, 1));
    // downgrade the stored state to the earlier format
    let dir = state.join("transcript");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let s = std::fs::read_to_string(e.path()).unwrap();
        assert!(s.contains("\"v\":3"), "{s}");
        // a v2 state has no message counts and would undercount
        let old = s
            .replace("\"v\":3", "\"v\":2")
            .replace("\"usr\":1", "\"usr\":0");
        std::fs::write(e.path(), old).unwrap();
    }
    let p = transcript::update(&state, &t.path().join("t.jsonl"), u64::MAX, None).unwrap();
    assert_eq!((p.stats.compactions, p.stats.user_msgs), (2, 1));
}

// tmux

const TMUX: &str = "/tmp/tmux-1000/default,123,0";

/// Fake `tmux` that logs its argv and prints `name`.
fn fake_tmux(t: &Tmp, name: &str, code: i32) -> std::path::PathBuf {
    let bin = t.path().join("tmux-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = t.path().join("tmux.log");
    let p = bin.join("tmux");
    common::exe::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\necho '{name}'\nexit {code}\n",
            log.display()
        ),
    );
    log
}

fn with_tmux(t: &Tmp) -> Run<'_> {
    let path = format!(
        "{}:{}",
        t.path().join("tmux-bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    run(t)
        .conf("set left \"#{tmux}\"")
        .env("PATH", &path)
        .env("TMUX", TMUX)
        .env("TMUX_PANE", "%5")
}

/// Runs the background job, as `bar refresh` would, until it stores a
/// value: on a loaded host a run can pass the job's 2 s limit and store
/// nothing, which says nothing about the code under test.
fn refresh_now(t: &Tmp) {
    let (iso, cwd) = common::isolation(t.path());
    let key = cache_key(TMUX, "%5");
    let req = Request {
        key: key.clone(),
        job: Job::Tmux { pane: "%5".into() },
        lock: String::new(),
    };
    let path = format!(
        "{}:{}",
        t.path().join("tmux-bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let cache = Cache::new(&t.path().join("state"));
    let stored = || {
        assert_cmd::Command::cargo_bin("bar")
            .unwrap()
            .envs(iso.iter().cloned())
            .env("PATH", &path)
            .env("TMUX", TMUX)
            .current_dir(&cwd)
            .args(["refresh", &req.to_json()])
            .assert()
            .success();
        cache.read_fresh(&key, 3600).is_some()
    };
    assert!(common::wait_until(common::BG_WAIT, stored), "never stored");
}

#[test]
fn tmux_cold_renders_empty_then_fills_from_background_job() {
    let t = Tmp::new();
    let log = fake_tmux(&t, "work:3", 0);
    assert_eq!(with_tmux(&t).go().plain.trim(), "");
    refresh_now(&t);
    assert_eq!(with_tmux(&t).go().plain.trim(), "work:3");
    let argv = std::fs::read_to_string(log).unwrap();
    assert!(
        argv.lines().all(|l| l == "display-message -p #S:#I -t %5"),
        "{argv}"
    );
}

#[test]
fn tmux_warm_cache_renders_without_spawning() {
    let t = Tmp::new();
    let log = fake_tmux(&t, "other", 0);
    Cache::new(&t.path().join("state"))
        .store(&cache_key(TMUX, "%5"), "dev:2\n")
        .unwrap();
    assert_eq!(with_tmux(&t).go().plain.trim(), "dev:2");
    assert!(!log.exists(), "render spawned tmux");
}

#[test]
fn tmux_cached_value_is_sanitized() {
    let t = Tmp::new();
    fake_tmux(&t, "o", 0);
    Cache::new(&t.path().join("state"))
        .store(&cache_key(TMUX, "%5"), "a\u{1b}[31mb\u{7}:1")
        .unwrap();
    let o = with_tmux(&t).go();
    assert!(!o.raw.contains("\x1b[31m"), "{:?}", o.raw);
    assert!(!o.raw.contains('\u{7}'));
}

#[test]
fn tmux_hidden_outside_tmux_or_with_bad_pane() {
    let t = Tmp::new();
    let log = fake_tmux(&t, "work:3", 0);
    assert_eq!(with_tmux(&t).unset("TMUX").go().plain.trim(), "");
    assert_eq!(with_tmux(&t).unset("TMUX_PANE").go().plain.trim(), "");
    assert_eq!(with_tmux(&t).env("TMUX", "").go().plain.trim(), "");
    assert_eq!(
        with_tmux(&t).env("TMUX_PANE", "%5;touch").go().plain.trim(),
        ""
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(!log.exists(), "spawned tmux");
}

#[test]
fn tmux_failure_backs_off() {
    let t = Tmp::new();
    let log = fake_tmux(&t, "boom", 1);
    let cache = Cache::new(&t.path().join("state"));
    let key = cache_key(TMUX, "%5");
    with_tmux(&t).go();
    // the background refresh ran tmux, failed and let go of its lock
    let failed = || cache.fail_path(&key).exists() && !cache.lock_path(&key).exists();
    assert!(common::wait_until(common::BG_WAIT, failed), "no failed run");
    for _ in 0..2 {
        with_tmux(&t).go();
        // a render starting a refresh takes the lock before it returns
        assert!(
            !cache.lock_path(&key).exists(),
            "refreshed while backing off"
        );
    }
    let n = std::fs::read_to_string(log).unwrap().lines().count();
    assert_eq!(n, 1, "retried inside the backoff window");
}

#[test]
fn tmux_job_round_trips_and_rejects_bad_pane() {
    let r = Request {
        key: "k".into(),
        job: Job::Tmux { pane: "%12".into() },
        lock: String::new(),
    };
    assert_eq!(Request::from_json(&r.to_json()).unwrap(), r);
    let bad = r.to_json().replace("%12", "-t");
    assert!(Request::from_json(&bad).is_err());
}

fn user(content: &str, extra: &str) -> String {
    format!(
        "{{\"type\":\"user\"{extra},\"message\":{{\"role\":\"user\",\"content\":{content}}}}}\n"
    )
}

const TOOL_RESULT: &str = "[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"ok\"}]";

fn msgs_transcript() -> String {
    let text = asst("a1", "{\"type\":\"text\",\"text\":\"hi\"}", false);
    let split = asst("a2", "{\"type\":\"text\",\"text\":\"x\"}", false)
        + &asst(
            "a2",
            "{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\"}",
            false,
        );
    user("\"hello\"", "")
        + &text
        + &split
        + &user(TOOL_RESULT, "")
        + &user("[{\"type\":\"text\",\"text\":\"again\"}]", "")
        + &user("\"meta\"", ",\"isMeta\":true")
        + &user("\"sum\"", ",\"isCompactSummary\":true")
        + &user("\"side\"", ",\"isSidechain\":true")
        + &asst("s1", "{\"type\":\"text\",\"text\":\"s\"}", true)
}

#[test]
fn msgs_counts_user_prompts_only_by_default() {
    let t = Tmp::new();
    t.write("t.jsonl", &msgs_transcript());
    let o = run(&t).conf("set left \"#{msgs}\"").stdin(&tstdin(&t)).go();
    assert_eq!(o.plain.trim(), "msgs 2");
}

#[test]
fn msgs_counts_escaped_text_and_skips_odd_content() {
    let t = Tmp::new();
    let body = user("\"line one\\nline \\\"two\\\"\"", "")
        + &user("[{\"type\":\"text\",\"text\":\"tab\\there\"}]", "")
        + &user("{\"text\":\"obj\"}", "")
        + &user("42", "");
    t.write("t.jsonl", &body);
    let o = run(&t).conf("set left \"#{msgs}\"").stdin(&tstdin(&t)).go();
    assert_eq!(o.plain.trim(), "msgs 2");
}

#[test]
fn msgs_total_adds_main_chain_assistant_messages() {
    let t = Tmp::new();
    t.write("t.jsonl", &msgs_transcript());
    let o = run(&t)
        .conf("set left \"#{msgs[show=total]}\"")
        .stdin(&tstdin(&t))
        .go();
    // 2 prompts + a1 + a2 (one id over two lines)
    assert_eq!(o.plain.trim(), "msgs 4");
}

#[test]
fn msgs_ignores_quoted_marker_and_hides_at_zero() {
    let t = Tmp::new();
    let quote = asst(
        "q",
        "{\"type\":\"text\",\"text\":\"\\\"type\\\":\\\"user\\\"\"}",
        false,
    );
    t.write("t.jsonl", &quote);
    let o = run(&t).conf("set left \"#{msgs}\"").stdin(&tstdin(&t)).go();
    assert_eq!(o.plain.trim(), "");
    let o = run(&t).conf("set left \"#{msgs}\"").go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn msgs_accumulate_incrementally() {
    let t = Tmp::new();
    t.write("t.jsonl", &user("\"one\"", ""));
    let go = || {
        run(&t)
            .conf("set left \"#{msgs}\"")
            .stdin(&tstdin(&t))
            .go()
            .plain
    };
    assert_eq!(go().trim(), "msgs 1");
    let more = std::fs::read_to_string(t.path().join("t.jsonl")).unwrap() + &user("\"two\"", "");
    t.write("t.jsonl", &more);
    assert_eq!(go().trim(), "msgs 2");
}
