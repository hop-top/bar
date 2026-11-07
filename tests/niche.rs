mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::placeholders::tmux::cache_key;
use hop_bar::transcript;
use std::time::{Duration, Instant};

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
    t.write("t.jsonl", &(boundary().to_string() + boundary()));
    let state = t.path().join("state");
    let p = transcript::update(&state, &t.path().join("t.jsonl"), u64::MAX, None).unwrap();
    assert_eq!(p.stats.compactions, 2);
    // downgrade the stored state to the earlier format
    let dir = state.join("transcript");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let s = std::fs::read_to_string(e.path()).unwrap();
        assert!(s.contains("\"v\":2"), "{s}");
        std::fs::write(e.path(), s.replace("\"v\":2", "\"v\":1")).unwrap();
    }
    let p = transcript::update(&state, &t.path().join("t.jsonl"), u64::MAX, None).unwrap();
    assert_eq!(p.stats.compactions, 2);
}

// tmux

const TMUX: &str = "/tmp/tmux-1000/default,123,0";

/// Fake `tmux` that logs its argv and prints `name`.
fn fake_tmux(t: &Tmp, name: &str, code: i32) -> std::path::PathBuf {
    let bin = t.path().join("tmux-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = t.path().join("tmux.log");
    let p = bin.join("tmux");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\necho '{name}'\nexit {code}\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

fn wait_for(t: &Tmp, want: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let o = with_tmux(t).go();
        if o.plain.trim() == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never showed {want}: {:?}",
            o.plain
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn tmux_cold_renders_empty_then_fills_from_background_job() {
    let t = Tmp::new();
    let log = fake_tmux(&t, "work:3", 0);
    assert_eq!(with_tmux(&t).go().plain.trim(), "");
    wait_for(&t, "work:3");
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
    for _ in 0..3 {
        with_tmux(&t).go();
        std::thread::sleep(Duration::from_millis(600));
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
