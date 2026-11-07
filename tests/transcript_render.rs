mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use std::time::{Duration, Instant};

const CONF: &str = "set left \"#{tokens}|#{cache}|#{cachettl}\"";

fn line(id: usize, ts: &str) -> String {
    format!(
        "{{\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"m{id}\",\"usage\":{{\"input_tokens\":1,\"cache_creation_input_tokens\":2,\"cache_read_input_tokens\":1000,\"output_tokens\":3}}}}}}\n"
    )
}

fn stdin(t: &Tmp) -> String {
    format!(
        "{{\"transcript_path\":\"{}\"}}",
        t.path().join("t.jsonl").display()
    )
}

#[test]
fn renders_totals_from_transcript_and_cold_cache() {
    let t = Tmp::new();
    t.write(
        "t.jsonl",
        &(line(1, "2020-01-01T00:00:00.000Z") + &line(2, "2020-01-01T00:00:01.000Z")),
    );
    let o = run(&t).conf(CONF).stdin(&stdin(&t)).go();
    assert_eq!(o.plain.trim(), "6↑ 6↓|r 2k w 4|cold");
}

#[test]
fn missing_transcript_renders_nothing() {
    let t = Tmp::new();
    let o = run(&t).conf(CONF).stdin("{}").go();
    assert_eq!(o.plain.trim(), "");
    let o = run(&t).conf(CONF).stdin(&stdin(&t)).go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn big_transcript_defers_then_finishes_in_background() {
    let t = Tmp::new();
    let n = 30_000;
    let body: String = (0..n)
        .map(|i| line(i, "2020-01-01T00:00:00.000Z"))
        .collect();
    assert!(body.len() as u64 > 4 * 1024 * 1024);
    t.write("t.jsonl", &body);
    let s = stdin(&t);
    let conf = "set left \"#{tokens[show=in]}\"";
    let first = run(&t).conf(conf).stdin(&s).go().plain.trim().to_string();
    let full = format!("{}↑", hop_bar::placeholders::tokens::compact(3 * n as u64));
    assert_ne!(first, full, "first render must be partial");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let out = run(&t).conf(conf).stdin(&s).go().plain.trim().to_string();
        if out == full {
            break;
        }
        assert!(Instant::now() < deadline, "never finished: {out}");
        std::thread::sleep(Duration::from_millis(100));
    }
}
