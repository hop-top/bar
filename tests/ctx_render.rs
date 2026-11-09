// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::transcript;

/// Assistant line whose request held `ctx` context tokens.
fn req(id: &str, ctx: u64, side: bool) -> String {
    format!(
        "{{\"type\":\"assistant\",\"isSidechain\":{side},\"timestamp\":\"2026-01-02T03:04:05.250Z\",\"message\":{{\"id\":\"{id}\",\"usage\":{{\"input_tokens\":10,\"cache_creation_input_tokens\":{},\"cache_read_input_tokens\":{},\"output_tokens\":5}}}}}}\n",
        ctx / 4,
        ctx - 10 - ctx / 4
    )
}

const BOUNDARY: &str = "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"content\":\"Conversation compacted\"}\n";

fn stats(t: &Tmp) -> transcript::Stats {
    let p = transcript::update(
        &t.path().join("state"),
        &t.path().join("t.jsonl"),
        u64::MAX,
        None,
    );
    p.unwrap().stats
}

fn append(t: &Tmp, s: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(t.path().join("t.jsonl"))
        .unwrap();
    f.write_all(s.as_bytes()).unwrap();
}

#[test]
fn context_sizes_per_main_request() {
    let t = Tmp::new();
    // a message repeated per content block counts once; sidechains are skipped
    append(
        &t,
        &(req("a", 40_000, false)
            + &req("a", 40_000, false)
            + &req("s", 900_000, true)
            + &req("b", 52_000, false)),
    );
    let s = stats(&t);
    assert_eq!(
        (s.ctx_first, s.ctx_prev, s.ctx_last, s.ctx_reqs),
        (40_000, 40_000, 52_000, 2)
    );
    // incremental: the saved state carries on
    append(&t, &req("c", 61_000, false));
    let s = stats(&t);
    assert_eq!(
        (s.ctx_first, s.ctx_prev, s.ctx_last, s.ctx_reqs),
        (40_000, 52_000, 61_000, 3)
    );
}

#[test]
fn compaction_restarts_the_context_series() {
    let t = Tmp::new();
    append(&t, &(req("a", 150_000, false) + &req("b", 180_000, false)));
    append(&t, BOUNDARY);
    let s = stats(&t);
    assert_eq!((s.ctx_reqs, s.compactions), (0, 1));
    append(&t, &req("c", 30_000, false));
    let s = stats(&t);
    assert_eq!(
        (s.ctx_first, s.ctx_prev, s.ctx_last, s.ctx_reqs),
        (30_000, 0, 30_000, 1)
    );
}

fn stdin(t: &Tmp, model: &str, size: u64, tokens: u64) -> String {
    serde_json::json!({
        "transcript_path": t.path().join("t.jsonl"),
        "model": {"id": model},
        "context_window": {
            "used_percentage": tokens * 100 / size,
            "context_window_size": size,
            "total_input_tokens": tokens,
        }
    })
    .to_string()
}

const VARS: [&str; 5] = [
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
    "DISABLE_AUTO_COMPACT",
    "DISABLE_COMPACT",
    "CLAUDE_CONFIG_DIR",
];

/// Unsets Claude Code's compaction variables except `keep`; `go` applies
/// unsets after sets.
fn clean<'a>(mut r: Run<'a>, keep: &str) -> Run<'a> {
    for v in VARS.into_iter().filter(|v| *v != keep) {
        r = r.unset(v);
    }
    r
}

#[test]
fn renders_growth_from_transcript() {
    let t = Tmp::new();
    append(
        &t,
        &(req("a", 40_000, false) + &req("b", 52_000, false) + &req("c", 61_000, false)),
    );
    let o = clean(run(&t), "")
        .conf("set left \"#{ctx[show=pct,growth,trend]}\"")
        .stdin(&stdin(&t, "claude-x", 200_000, 61_000))
        .go();
    assert_eq!(o.plain.trim(), "ctx 30% +9k +10.5k/req");
}

#[test]
fn compact_point_from_env_and_user_settings() {
    let t = Tmp::new();
    let conf = "set left \"#{ctx[show=left]}\"";
    let s = stdin(&t, "claude-x", 1_000_000, 400_000);
    let o = clean(run(&t), "").conf(conf).stdin(&s).go();
    assert_eq!(o.plain.trim(), "ctx 567k left");
    t.write(
        ".claude/settings.json",
        r#"{"modelSettings":{"claude-x":{"autoCompactWindow":500000}}}"#,
    );
    let o = clean(run(&t), "").conf(conf).stdin(&s).go();
    assert_eq!(o.plain.trim(), "ctx 100k left");
    let o = clean(run(&t), "CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        .env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "450k")
        .conf(conf)
        .stdin(&s)
        .go();
    assert_eq!(o.plain.trim(), "ctx 50k left");
    let o = clean(run(&t), "DISABLE_AUTO_COMPACT")
        .env("DISABLE_AUTO_COMPACT", "1")
        .conf(conf)
        .stdin(&s)
        .go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn preview_shows_context_parts() {
    let t = Tmp::new();
    t.write(
        "xdg/bar/bar.conf",
        "set left \"#{ctx[show=pct,tokens,left,split,growth]}\"\n",
    );
    let (iso, cwd) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .env_remove("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        .env("COLUMNS", "200")
        .current_dir(cwd)
        .arg("preview")
        .assert()
        .success();
    let out = strip(&String::from_utf8(o.get_output().stdout.clone()).unwrap());
    assert!(
        out.contains("ctx 38% 76k/200k 124k left r 70k w 4.8k new 1.2k +3.2k"),
        "{out}"
    );
}
