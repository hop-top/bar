//! Odd line shapes: the transcript reader counts exactly what reading each
//! line as a full JSON tree would, whichever way it parses.
mod common;

use hop_bar::transcript::{self, Stats};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-tshape-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

/// Totals after reading `lines` (each gets a trailing newline).
fn read(lines: &[&[u8]]) -> Stats {
    let t = Tmp::new();
    let file = t.0.join("t.jsonl");
    let mut f = std::fs::File::create(&file).unwrap();
    for l in lines {
        f.write_all(l).unwrap();
        f.write_all(b"\n").unwrap();
    }
    drop(f);
    transcript::update(&t.0.join("state"), &file, u64::MAX, None)
        .unwrap()
        .stats
}

/// A plain line that always counts: 100 input, 10 output.
const OK: &[u8] =
    br#"{"type":"assistant","message":{"id":"ok","usage":{"input_tokens":100,"output_tokens":10}}}"#;

#[test]
fn usage_as_array_is_not_an_object() {
    let s = read(&[br#"{"message":{"id":"a","usage":[5,6,7,8]}}"#, OK]);
    assert_eq!((s.input, s.output), (100, 10));
}

#[test]
fn message_as_array_is_not_an_object() {
    let s = read(&[
        br#"{"message":["a",{"input_tokens":5},null],"usage":null}"#,
        OK,
    ]);
    assert_eq!(s.input, 100);
}

#[test]
fn duplicate_key_takes_the_last_value() {
    let s = read(&[br#"{"message":{"id":"a","usage":{"input_tokens":1,"input_tokens":7}}}"#]);
    assert_eq!(s.input, 7);
}

#[test]
fn non_integer_count_reads_as_zero() {
    let s = read(&[
        br#"{"message":{"id":"a","usage":{"input_tokens":1.5,"output_tokens":3}}}"#,
        br#"{"message":{"id":"b","usage":{"input_tokens":-2,"output_tokens":"4"}}}"#,
        br#"{"message":{"id":"c","usage":{"input_tokens":null,"output_tokens":5}}}"#,
    ]);
    assert_eq!((s.input, s.output), (0, 8));
}

#[test]
fn non_string_id_is_not_deduped() {
    let l: &[u8] = br#"{"message":{"id":7,"usage":{"input_tokens":2}}}"#;
    assert_eq!(read(&[l, l]).input, 4);
}

#[test]
fn escaped_id_still_dedupes() {
    let l: &[u8] = br#"{"message":{"id":"a\"b\u00e9","usage":{"input_tokens":2}}}"#;
    assert_eq!(read(&[l, l]).input, 2);
}

#[test]
fn non_string_timestamp_is_ignored() {
    let s = read(&[br#"{"timestamp":5,"message":{"id":"a","usage":{"input_tokens":2}}}"#]);
    assert_eq!((s.input, s.last_ts), (2, None));
}

#[test]
fn invalid_utf8_in_a_skipped_string_drops_the_line() {
    let s = read(&[
        b"{\"message\":{\"id\":\"a\",\"usage\":{\"input_tokens\":4}},\"x\":\"\xff\"}",
        OK,
    ]);
    assert_eq!(s.input, 100);
}

#[test]
fn lone_surrogate_escape_drops_the_line() {
    // a JSON tree rejects it, so the line counts for nothing
    let s = read(&[
        br#"{"message":{"id":"a","usage":{"input_tokens":4}},"x":"\ud800"}"#,
        OK,
    ]);
    assert_eq!(s.input, 100);
    // a valid pair is fine
    let s = read(&[br#"{"message":{"id":"a","usage":{"input_tokens":4}},"x":"\ud83d\ude00"}"#]);
    assert_eq!(s.input, 4);
}

#[test]
fn skill_word_with_string_content_still_counts_usage() {
    let s = read(&[
        br#"{"type":"assistant","message":{"id":"s","content":"use the Skill","usage":{"input_tokens":2}}}"#,
    ]);
    assert_eq!((s.input, s.skills), (2, 0));
}

#[test]
fn non_object_blocks_are_passed_over() {
    let s = read(&[
        br#"{"type":"assistant","message":{"id":"s","content":[1,"Skill",{"type":"tool_use","name":"Skill"}],"usage":{"input_tokens":2}}}"#,
    ]);
    assert_eq!((s.input, s.skills), (2, 1));
}

#[test]
fn non_bool_sidechain_is_main_chain() {
    let s = read(&[
        br#"{"type":"system","subtype":"compact_boundary","isSidechain":"true"}"#,
        br#"{"type":"system","subtype":"compact_boundary","isSidechain":true}"#,
        br#"{"type":"assistant","isSidechain":1,"message":{"content":[{"type":"tool_use","name":"Skill"}]}}"#,
    ]);
    assert_eq!((s.compactions, s.skills), (1, 1));
}

#[test]
fn unknown_fields_and_nested_usage_are_ignored() {
    let s = read(&[
        br#"{"x":{"type":"system"},"message":{"model":"m","id":"a","content":[{"type":"text","text":"\"usage\": {}"}],"usage":{"input_tokens":3,"cache_creation":{"ephemeral_5m_input_tokens":9},"server_tool_use":{"web_search_requests":0}}}}"#,
    ]);
    assert_eq!(s.input, 3);
}

#[test]
fn line_as_array_is_not_an_entry() {
    let s = read(&[
        br#"["assistant",null,null,null,{"id":"a","usage":{"input_tokens":5},"content":null}]"#,
        OK,
    ]);
    assert_eq!(s.input, 100);
}

#[test]
fn lines_across_read_chunks_are_joined() {
    // a usage line starting 20 bytes before each 64 KiB multiple up to
    // 1 MiB, so lines straddle the reader's chunk boundaries
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut len = 0;
    for k in 1..=16 {
        let pad = k * 65_536 - 20 - len - 1 - 8;
        lines.push(format!("{{\"x\":\"{}\"}}", "f".repeat(pad)).into_bytes());
        let u = format!("{{\"message\":{{\"id\":\"m{k}\",\"usage\":{{\"input_tokens\":1}}}}}}");
        len = k * 65_536 - 20 + u.len() + 1;
        lines.push(u.into_bytes());
    }
    let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
    assert_eq!(read(&refs).input, 16);
}
