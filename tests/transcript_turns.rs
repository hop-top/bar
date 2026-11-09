//! Turn timing and prompt-cache misses from synthetic transcripts.

use hop_bar::transcript::{self, parse_ts_ms, Turns};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-tturn-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn append(&self, lines: &[String]) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join("t.jsonl"))
            .unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }
    fn turns(&self) -> Turns {
        transcript::update(
            &self.0.join("state"),
            &self.0.join("t.jsonl"),
            u64::MAX,
            None,
        )
        .unwrap()
        .stats
        .turns
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `2026-01-02T03:00:00.000Z` plus `ms` (under 20 hours).
fn at(ms: i64) -> String {
    let (s, ms) = (ms / 1000, ms % 1000);
    format!(
        "2026-01-02T{:02}:{:02}:{:02}.{ms:03}Z",
        3 + s / 3600,
        s / 60 % 60,
        s % 60
    )
}

fn prompt(ms: i64) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":"do it"}}}}"#,
        at(ms)
    )
}

fn tool_result(ms: i64) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"ok"}}]}}}}"#,
        at(ms)
    )
}

/// An assistant entry with cache counts and the 5m/1h split.
fn reply(id: &str, ms: i64, out: u64, cr: u64, cc: u64, h1: bool) -> String {
    let (m5, h) = if h1 { (0, cc) } else { (cc, 0) };
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","role":"assistant","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":3,"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cr},"output_tokens":{out},"cache_creation":{{"ephemeral_5m_input_tokens":{m5},"ephemeral_1h_input_tokens":{h}}}}}}}}}"#,
        at(ms)
    )
}

fn asst(id: &str, ms: i64, out: u64) -> String {
    reply(id, ms, out, 0, 0, false)
}

#[test]
fn timestamp_parses_milliseconds() {
    assert_eq!(parse_ts_ms("1970-01-01T00:00:01.250Z"), Some(1_250));
    assert_eq!(parse_ts_ms("1970-01-01T00:00:01.5Z"), Some(1_500));
    assert_eq!(parse_ts_ms("1970-01-01T00:00:01.123456Z"), Some(1_123));
    assert_eq!(parse_ts_ms("1970-01-01T00:00:01Z"), Some(1_000));
    assert_eq!(transcript::parse_ts("1970-01-01T00:00:01.999Z"), Some(1));
}

/// Prompt at 0; request `a` (two blocks) until 3 s with 600 out; a tool
/// result at 4 s; request `b` at 5 s with 200 out. Then a second prompt
/// at 60 s answered by `c`: one block at 60.5 s, 100 out.
fn two_turns() -> Vec<String> {
    vec![
        prompt(0),
        asst("a", 1_500, 300),
        asst("a", 3_000, 600),
        tool_result(4_000),
        asst("b", 5_000, 200),
        prompt(60_000),
        asst("c", 60_500, 100),
    ]
}

#[test]
fn ttft_and_tps_per_turn_and_session() {
    let t = Tmp::new();
    t.append(&two_turns()[..5]);
    let s = t.turns();
    assert_eq!(s.ttft_last(), Some(1_500));
    // a: 600 over 3 s; b: 200 over 1 s since the tool result
    assert_eq!(s.tps_last(), Some(200.0));
    t.append(&two_turns()[5..]);
    let s = t.turns();
    assert_eq!(s.ttft_last(), Some(500));
    assert_eq!(s.ttft_avg(), Some(1_000));
    assert_eq!(s.tps_last(), Some(200.0));
    // 900 over 4.5 s
    assert_eq!(s.tps_avg(), Some(200.0));
}

#[test]
fn incremental_reads_match_one_pass() {
    let lines = two_turns();
    let whole = Tmp::new();
    whole.append(&lines);
    let step = Tmp::new();
    for l in &lines {
        step.append(std::slice::from_ref(l));
        step.turns();
    }
    assert_eq!(step.turns(), whole.turns());
}

#[test]
fn unanswered_prompt_keeps_the_last_turn() {
    let t = Tmp::new();
    t.append(&two_turns()[..5]);
    t.append(&[prompt(90_000)]);
    let s = t.turns();
    assert_eq!(s.ttft_last(), Some(1_500));
    assert_eq!(s.tps_last(), Some(200.0));
}

#[test]
fn tool_results_meta_and_sidechain_are_not_prompts() {
    let t = Tmp::new();
    let meta = format!(
        r#"{{"type":"user","isMeta":true,"timestamp":"{}","message":{{"role":"user","content":"context"}}}}"#,
        at(500)
    );
    let side = format!(
        r#"{{"type":"user","isSidechain":true,"timestamp":"{}","message":{{"role":"user","content":"sub"}}}}"#,
        at(800)
    );
    t.append(&[
        prompt(0),
        meta,
        side,
        tool_result(900),
        asst("a", 2_000, 10),
    ]);
    // measured from the prompt, not the later user entries
    assert_eq!(t.turns().ttft_last(), Some(2_000));
    let t = Tmp::new();
    t.append(&[tool_result(0), asst("a", 2_000, 10)]);
    assert_eq!(t.turns().ttft_last(), None);
}

#[test]
fn no_timestamps_no_timing() {
    let t = Tmp::new();
    t.append(&[
        r#"{"type":"user","message":{"role":"user","content":"hi"}}"#.into(),
        r#"{"type":"assistant","message":{"id":"a","usage":{"output_tokens":5}}}"#.into(),
    ]);
    let s = t.turns();
    assert_eq!(
        (s.ttft_last(), s.tps_last(), s.tps_avg()),
        (None, None, None)
    );
}

const MIN: i64 = 60_000;

#[test]
fn cold_cache_after_idle_is_a_ttl_miss() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 50_000, false),
        prompt(MIN),
        reply("b", MIN + 1_000, 5, 50_000, 1_000, false),
        prompt(12 * MIN),
        // the system prompt is still cached from elsewhere
        reply("c", 12 * MIN + 1_000, 5, 8_000, 44_000, false),
        prompt(12 * MIN + 5_000),
        reply("d", 12 * MIN + 6_000, 5, 0, 53_000, false),
    ]);
    let s = t.turns();
    assert_eq!(s.misses(), (1, 1));
    assert_eq!(s.prefix(), 53_000);
    assert_eq!(s.ttl(), 300);
}

#[test]
fn one_hour_cache_idle_under_an_hour_is_invalidation() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 50_000, true),
        prompt(20 * MIN),
        reply("b", 20 * MIN + 1_000, 5, 0, 50_500, true),
        prompt(90 * MIN),
        reply("c", 90 * MIN + 1_000, 5, 0, 51_000, true),
    ]);
    let s = t.turns();
    assert_eq!(s.misses(), (1, 1));
    assert_eq!(s.ttl(), 3600);
    assert!(s.one_hour());
}

#[test]
fn warm_growth_small_prefix_and_compaction_are_not_misses() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 900, false),
        // under the minimum cacheable prefix
        reply("b", 2_000, 5, 0, 950, false),
        reply("c", 3_000, 5, 0, 40_000, false),
        reply("d", 4_000, 5, 40_000, 2_000, false),
        r#"{"type":"system","subtype":"compact_boundary"}"#.into(),
        prompt(5_000),
        reply("e", 6_000, 5, 0, 9_000, false),
    ]);
    assert_eq!(t.turns().misses(), (0, 0));
}

#[test]
fn repeated_blocks_count_one_request() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 50_000, false),
        prompt(MIN),
        reply("b", MIN + 500, 5, 0, 50_000, false),
        reply("b", MIN + 900, 9, 0, 50_000, false),
        reply("b", MIN + 1_200, 9, 0, 50_000, false),
    ]);
    assert_eq!(t.turns().misses(), (0, 1));
}

#[test]
fn tree_reading_classifies_like_the_typed_one() {
    // a surrogate escape and a duplicate key both go through the tree
    let esc = format!(
        r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":"hi 😀"}}}}"#,
        at(0)
    );
    let dup = format!(
        r#"{{"type":"user","type":"user","timestamp":"{}","message":{{"content":[1,{{"type":"tool_result"}}]}}}}"#,
        at(1_000)
    );
    let t = Tmp::new();
    t.append(&[esc, dup, asst("a", 3_000, 20)]);
    let s = t.turns();
    assert_eq!(s.ttft_last(), Some(3_000));
    // the request started at the tool result
    assert_eq!(s.tps_last(), Some(10.0));
}

/// An assistant entry Claude Code writes itself (an API error, a usage
/// limit): `<synthetic>` model, all usage zero.
fn synthetic(id: &str, ms: i64, model: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","model":"{model}","role":"assistant","content":[{{"type":"text","text":"API Error"}}],"usage":{{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0,"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}}}}}}}}"#,
        at(ms)
    )
}

#[test]
fn synthetic_entries_are_not_requests() {
    for model in ["<synthetic>", "claude-opus-5-5"] {
        let t = Tmp::new();
        t.append(&[
            prompt(0),
            reply("a", 1_000, 5, 0, 50_000, false),
            prompt(MIN),
            synthetic("e", MIN + 500, model),
            prompt(2 * MIN),
            reply("b", 2 * MIN + 1_000, 5, 0, 50_000, false),
        ]);
        let s = t.turns();
        // the error answered nothing: timed from the next prompt
        assert_eq!(
            (s.ttft_last(), s.ttft_avg()),
            (Some(1_000), Some(1_000)),
            "{model}"
        );
        // the prefix survived the error, so b is a miss
        assert_eq!(s.misses(), (0, 1), "{model}");
        let st = transcript::update(&t.0.join("state"), &t.0.join("t.jsonl"), u64::MAX, None)
            .unwrap()
            .stats;
        assert_eq!(
            (st.ctx_reqs, st.ctx_prev, st.ctx_last, st.assistant_msgs),
            (2, 50_003, 50_003, 2),
            "{model}"
        );
    }
}

fn user_text(ms: i64, content: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":{content}}}}}"#,
        at(ms)
    )
}

#[test]
fn interruptions_and_commands_are_not_prompts() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 0, false),
        user_text(
            MIN,
            r#"[{"type":"text","text":"[Request interrupted by user]"}]"#,
        ),
        user_text(MIN + 100, r#""[Request interrupted by user for tool use]""#),
        user_text(
            MIN + 200,
            r#""<command-name>/model</command-name>\n<command-message>model</command-message>""#,
        ),
        user_text(
            MIN + 300,
            r#""<local-command-stdout>Set model</local-command-stdout>""#,
        ),
        user_text(MIN + 400, r#""<command-message>review</command-message>""#),
        // a surrogate escape sends the line through the tree reading
        user_text(
            MIN + 500,
            r#"[{"type":"text","text":"<command-name>/x \ud83d\ude00"}]"#,
        ),
        reply("b", MIN + 2_000, 5, 0, 0, false),
        // typed text that merely mentions a marker later on is a prompt
        user_text(2 * MIN, r#""see <command-name> in the docs""#),
        reply("c", 2 * MIN + 700, 5, 0, 0, false),
    ]);
    let st = transcript::update(&t.0.join("state"), &t.0.join("t.jsonl"), u64::MAX, None)
        .unwrap()
        .stats;
    assert_eq!(st.user_msgs, 2);
    assert_eq!(st.turns.ttft_last(), Some(700));
    assert_eq!(st.turns.ttft_avg(), Some(850));
}

#[test]
fn compaction_clears_the_cached_prefix() {
    let t = Tmp::new();
    t.append(&[prompt(0), reply("a", 1_000, 5, 0, 50_000, false)]);
    assert_eq!(t.turns().prefix(), 50_000);
    t.append(&[r#"{"type":"system","subtype":"compact_boundary"}"#.into()]);
    // the old prefix is gone; nothing to re-cache until the next request
    assert_eq!(t.turns().prefix(), 0);
}

#[test]
fn miss_without_a_user_entry_is_not_labelled_ttl() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 50_000, false),
        // no user entry: when the request was sent is unknown, and the
        // block-end time includes generation
        reply("b", 10 * MIN, 5, 0, 50_000, false),
    ]);
    assert_eq!(t.turns().misses(), (0, 1));
}

fn stats(t: &Tmp) -> transcript::Stats {
    transcript::update(&t.0.join("state"), &t.0.join("t.jsonl"), u64::MAX, None)
        .unwrap()
        .stats
}

#[test]
fn tool_result_between_blocks_of_one_message() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 100, 0, 40_000, false),
        tool_result(1_500),
        // a later block of the same message: still request a
        reply("a", 2_000, 300, 0, 40_000, false),
        reply("b", 3_000, 100, 40_000, 500, false),
    ]);
    let st = stats(&t);
    assert_eq!((st.ctx_reqs, st.assistant_msgs), (2, 2));
    assert_eq!(st.cache_creation, 40_500);
    let s = st.turns;
    assert_eq!(s.ttft_last(), Some(1_000));
    assert_eq!(s.misses(), (0, 0));
    // a: 300 over 2 s; b: 100 over 1.5 s since the tool result
    assert_eq!(s.tps_last(), Some(400.0 / 3.5));
}

#[test]
fn compact_boundary_line_restarts_the_context_series() {
    let t = Tmp::new();
    t.append(&[
        prompt(0),
        reply("a", 1_000, 5, 0, 40_000, false),
        reply("b", 2_000, 5, 40_000, 9_000, false),
        format!(
            r#"{{"type":"system","subtype":"compact_boundary","content":"Conversation compacted","isSidechain":false,"timestamp":"{}","compactMetadata":{{"trigger":"auto","preTokens":49003}}}}"#,
            at(3_000)
        ),
        prompt(4_000),
        reply("c", 5_000, 5, 0, 12_000, false),
    ]);
    let st = stats(&t);
    assert_eq!(st.compactions, 1);
    assert_eq!(
        (st.ctx_reqs, st.ctx_first, st.ctx_last),
        (1, 12_003, 12_003)
    );
}

#[test]
fn mixed_5m_and_1h_write_expires_with_the_5m_part() {
    let t = Tmp::new();
    let mixed = format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"b","usage":{{"input_tokens":3,"cache_creation_input_tokens":3000,"cache_read_input_tokens":0,"output_tokens":5,"cache_creation":{{"ephemeral_5m_input_tokens":1000,"ephemeral_1h_input_tokens":2000}}}}}}}}"#,
        at(MIN)
    );
    t.append(&[prompt(0), reply("a", 1_000, 5, 0, 50_000, true)]);
    assert_eq!(t.turns().ttl(), 3600);
    t.append(&[mixed]);
    let s = t.turns();
    assert_eq!(s.ttl(), 300);
    assert!(!s.one_hour());
}
