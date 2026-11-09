// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;

use hop_bar::cache::Cache;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::tokens::{self, compact};
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::style::strip_codes;
use hop_bar::transcript::{self, parse_ts, Stats};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-ts-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
    fn file(&self) -> PathBuf {
        self.0.join("t.jsonl")
    }
    fn append(&self, s: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.file())
            .unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

/// One synthetic assistant line.
fn line(id: &str, i: u64, cc: u64, cr: u64, o: u64, ts: &str) -> String {
    format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"{id}\",\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"hi\"}}],\"usage\":{{\"input_tokens\":{i},\"cache_creation_input_tokens\":{cc},\"cache_read_input_tokens\":{cr},\"output_tokens\":{o},\"iterations\":[{{\"input_tokens\":999}}]}}}}}}\n"
    )
}

const T0: &str = "2026-01-02T03:04:05.250Z";

fn upd(t: &Tmp, cap: u64) -> (Stats, bool) {
    let p = transcript::update(&t.state(), &t.file(), cap, None).unwrap();
    (p.stats, p.complete)
}

#[test]
fn timestamp_parses_to_unix_seconds() {
    assert_eq!(parse_ts("1970-01-01T00:00:00.000Z"), Some(0));
    assert_eq!(parse_ts("2026-01-02T03:04:05.250Z"), Some(1_767_323_045));
    assert_eq!(parse_ts("2024-02-29T00:00:00Z"), Some(1_709_164_800));
    assert_eq!(parse_ts("garbage"), None);
    assert_eq!(parse_ts(""), None);
}

#[test]
fn sums_usage_fields() {
    let t = Tmp::new();
    t.append(&line("a", 10, 20, 300, 40, T0));
    t.append(&line("b", 1, 2, 3, 4, T0));
    let (s, done) = upd(&t, 1 << 22);
    assert!(done);
    assert_eq!(
        (s.input, s.cache_creation, s.cache_read, s.output),
        (11, 22, 303, 44)
    );
    assert_eq!(s.last_ts, Some(1_767_323_045));
}

#[test]
fn repeated_message_id_counts_once() {
    let t = Tmp::new();
    t.append(&line("a", 10, 0, 0, 5, T0));
    t.append(&line("a", 10, 0, 0, 5, T0));
    t.append(&line("a", 10, 0, 0, 5, T0));
    t.append(&line("b", 1, 0, 0, 1, T0));
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!((s.input, s.output), (11, 6));
}

#[test]
fn append_reads_only_new_bytes() {
    let t = Tmp::new();
    t.append(&line("a", 10, 0, 0, 1, T0));
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!(s.input, 10);
    // clobber the consumed prefix in place, keeping its length: a rescan
    // would now see garbage, an incremental read does not look at it
    let first = std::fs::read(t.file()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(t.file())
        .unwrap();
    f.write_all(&vec![b'x'; first.len() - 1]).unwrap();
    drop(f);
    t.append(&line("b", 5, 0, 0, 2, T0));
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!((s.input, s.output), (15, 3));
}

#[test]
fn partial_trailing_line_waits_for_its_newline() {
    let t = Tmp::new();
    let l = line("a", 7, 0, 0, 1, T0);
    let (head, tail) = l.split_at(l.len() / 2);
    t.append(head);
    let (s, done) = upd(&t, 1 << 22);
    assert_eq!(s.input, 0);
    assert!(done);
    t.append(tail);
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!(s.input, 7);
}

#[test]
fn truncation_rescans() {
    let t = Tmp::new();
    t.append(&line("a", 100, 0, 0, 1, T0));
    t.append(&line("b", 100, 0, 0, 1, T0));
    assert_eq!(upd(&t, 1 << 22).0.input, 200);
    std::fs::write(t.file(), line("c", 3, 0, 0, 1, T0)).unwrap();
    assert_eq!(upd(&t, 1 << 22).0.input, 3);
}

#[test]
fn replaced_file_rescans() {
    let t = Tmp::new();
    t.append(&line("a", 100, 0, 0, 1, T0));
    assert_eq!(upd(&t, 1 << 22).0.input, 100);
    // new inode, larger than the old one
    let other = t.0.join("other");
    std::fs::write(
        &other,
        line("x", 1, 0, 0, 1, T0) + &line("y", 2, 0, 0, 1, T0) + &line("z", 4, 0, 0, 1, T0),
    )
    .unwrap();
    std::fs::rename(&other, t.file()).unwrap();
    assert_eq!(upd(&t, 1 << 22).0.input, 7);
}

#[test]
fn garbage_and_huge_lines_are_skipped() {
    let t = Tmp::new();
    t.append("not json at all\n");
    t.append("{\"message\":{\"usage\":{\"input_tokens\":\"x\"}}}\n");
    t.append("{\"message\":{\"usage\":{\"input_tokens\":\n");
    t.append(&"\u{0}\u{1}\u{ff}".repeat(10));
    t.append("\n");
    t.append(&format!("{}\n", "z".repeat(3 << 20)));
    t.append(&line("a", 9, 0, 0, 1, T0));
    t.append("\n\n   \n");
    let (s, done) = upd(&t, 1 << 22);
    assert!(done);
    assert_eq!((s.input, s.output), (9, 1));
}

#[test]
fn huge_line_with_usage_marker_is_skipped_without_panic() {
    let t = Tmp::new();
    let big = format!(
        "{{\"message\":{{\"usage\":{{\"input_tokens\":5}},\"x\":\"{}\"}}}}\n",
        "y".repeat(2 << 20)
    );
    t.append(&big);
    t.append(&line("a", 2, 0, 0, 1, T0));
    assert_eq!(upd(&t, 1 << 24).0.input, 2);
}

#[test]
fn cap_defers_and_resumes() {
    let t = Tmp::new();
    for i in 0..40 {
        t.append(&line(&format!("m{i}"), 1, 0, 0, 1, T0));
    }
    let size = std::fs::metadata(t.file()).unwrap().len();
    let cap = size / 4;
    let (s, done) = upd(&t, cap);
    assert!(!done);
    assert!(s.input > 0 && s.input < 40, "partial totals: {}", s.input);
    let mut n = 0;
    loop {
        let (s, done) = upd(&t, cap);
        n += 1;
        if done {
            assert_eq!(s.input, 40);
            break;
        }
        assert!(n < 10, "no progress");
    }
}

#[test]
fn state_file_is_private() {
    let t = Tmp::new();
    t.append(&line("a", 1, 0, 0, 1, T0));
    upd(&t, 1 << 22);
    let dir = t.state().join("transcript");
    let mut seen = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let e = e.unwrap();
        assert_eq!(e.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        seen += 1;
    }
    assert_eq!(seen, 1, "no temp files left");
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn missing_file_is_none() {
    let t = Tmp::new();
    assert!(transcript::update(&t.state(), &t.0.join("nope"), 1 << 20, None).is_none());
}

#[test]
fn compact_numbers() {
    assert_eq!(compact(0), "0");
    assert_eq!(compact(999), "999");
    assert_eq!(compact(1_000), "1k");
    assert_eq!(compact(12_345), "12.3k");
    assert_eq!(compact(80_000), "80k");
    assert_eq!(compact(999_949), "999.9k");
    assert_eq!(compact(999_950), "1M");
    assert_eq!(compact(1_234_567), "1.2M");
    assert_eq!(compact(2_500_000_000), "2.5B");
}

fn stats() -> Stats {
    Stats {
        input: 10_000,
        cache_creation: 2_300,
        cache_read: 1_200_000,
        output: 4_100,
        compactions: 0,
        skills: 0,
        ctx_first: 0,
        ctx_prev: 0,
        ctx_last: 0,
        ctx_reqs: 0,
        user_msgs: 0,
        assistant_msgs: 0,
        last_ts: Some(10_000),
        mtime: 9_000,
        partial: false,
        turns: Default::default(),
    }
}

fn ctx_run<R>(f: impl FnOnce(&Ctx) -> R) -> R {
    let cfg = Config::default();
    let runner = ColdRunner;
    let s = Session::default();
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: 10_000,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    f(&ctx)
}

fn opts(kv: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in kv {
        o.set(k, v);
    }
    o
}

fn tok(kv: &[(&str, &str)]) -> Option<String> {
    ctx_run(|c| tokens::tokens(c, &opts(kv), &stats())).map(|s| strip_codes(&s))
}

#[test]
fn tokens_formats_in_out_total() {
    assert_eq!(tok(&[]).unwrap(), "12.3k↑ 4.1k↓");
    assert_eq!(tok(&[("show", "in")]).unwrap(), "12.3k↑");
    assert_eq!(tok(&[("show", "out")]).unwrap(), "4.1k↓");
    assert_eq!(tok(&[("show", "total")]).unwrap(), "16.4k");
}

#[test]
fn tokens_empty_stats_omitted() {
    let z = Stats::default();
    assert_eq!(
        ctx_run(|c| tokens::tokens(c, &Options::default(), &z)),
        None
    );
}

fn cch(kv: &[(&str, &str)]) -> Option<String> {
    ctx_run(|c| tokens::cache(c, &opts(kv), &stats())).map(|s| strip_codes(&s))
}

#[test]
fn cache_formats_read_write_hit() {
    assert_eq!(cch(&[]).unwrap(), "r 1.2M w 2.3k");
    assert_eq!(cch(&[("show", "read")]).unwrap(), "r 1.2M");
    assert_eq!(cch(&[("show", "write")]).unwrap(), "w 2.3k");
    // 1_200_000 / (1_200_000 + 10_000 + 2_300)
    assert_eq!(cch(&[("show", "hit")]).unwrap(), "hit 99%");
    let z = Stats::default();
    assert_eq!(
        ctx_run(|c| tokens::cache(c, &opts(&[("show", "hit")]), &z)),
        None
    );
}

fn ttl_at(now: i64, last: Option<i64>, mtime: i64, kv: &[(&str, &str)]) -> Option<String> {
    let s = Stats {
        last_ts: last,
        mtime,
        ..stats()
    };
    ctx_run(|c| {
        let c2 = Ctx {
            now,
            ..clone_ctx(c)
        };
        tokens::cachettl(&c2, &opts(kv), &s)
    })
    .map(|s| strip_codes(&s))
}

fn clone_ctx<'a>(c: &Ctx<'a>) -> Ctx<'a> {
    Ctx {
        session: c.session,
        config: c.config,
        now: c.now,
        runner: c.runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    }
}

#[test]
fn cachettl_counts_down_from_last_assistant() {
    assert_eq!(ttl_at(1_000 + 48, Some(1_000), 0, &[]).unwrap(), "4m12s");
    assert_eq!(ttl_at(1_000, Some(1_000), 0, &[]).unwrap(), "5m00s");
    assert_eq!(ttl_at(1_000 + 299, Some(1_000), 0, &[]).unwrap(), "1s");
}

#[test]
fn cachettl_expiry_is_cold_and_muted() {
    assert_eq!(ttl_at(1_300, Some(1_000), 0, &[]).unwrap(), "cold");
    assert_eq!(ttl_at(9_999, Some(1_000), 0, &[]).unwrap(), "cold");
    let raw = ctx_run(|c| {
        let c2 = Ctx {
            now: 5_000,
            ..clone_ctx(c)
        };
        tokens::cachettl(
            &c2,
            &Options::default(),
            &Stats {
                last_ts: Some(1),
                ..stats()
            },
        )
    })
    .unwrap();
    assert_eq!(raw, "\x1b[38;5;250mcold\x1b[39m");
}

#[test]
fn cachettl_ttl_option_and_mtime_fallback() {
    assert_eq!(
        ttl_at(1_000 + 100, Some(1_000), 0, &[("ttl", "3600")]).unwrap(),
        "58m20s"
    );
    assert_eq!(ttl_at(1_100, None, 1_000, &[]).unwrap(), "3m20s");
    assert_eq!(
        ttl_at(1_100, Some(1_000), 0, &[("ttl", "junk")]).unwrap(),
        "3m20s"
    );
    assert_eq!(ttl_at(1_100, None, 0, &[]), None);
}

#[test]
fn cachettl_partial_scan_uses_newer_mtime() {
    // the first MiB only holds old messages; the file was written since
    let s = |partial| Stats {
        last_ts: Some(1_000),
        mtime: 1_250,
        partial,
        ..stats()
    };
    let at = |st: Stats| {
        ctx_run(|c| {
            let c2 = Ctx {
                now: 1_300,
                ..clone_ctx(c)
            };
            tokens::cachettl(&c2, &Options::default(), &st)
        })
        .map(|s| strip_codes(&s))
    };
    assert_eq!(at(s(true)).unwrap(), "4m10s");
    assert_eq!(at(s(false)).unwrap(), "cold");
}

#[test]
fn update_marks_capped_read_partial() {
    let t = Tmp::new();
    t.append(&line("a", 1, 0, 0, 1, T0));
    t.append(&line("b", 1, 0, 0, 1, T0));
    let (s, complete) = upd(&t, 10);
    assert!(!complete && s.partial);
    let (s, complete) = upd(&t, 1 << 20);
    assert!(complete && !s.partial);
}

#[test]
fn cachettl_future_timestamp_clamps_to_full_ttl() {
    assert_eq!(ttl_at(500, Some(1_000), 0, &[]).unwrap(), "5m00s");
}

#[test]
fn load_without_cache_dir_use_defers_big_file_in_background() {
    // 3 MiB cap would be needed; use the public load with a tiny file
    let t = Tmp::new();
    t.append(&line("a", 4, 0, 0, 1, T0));
    let cache = Cache::with_exe(&t.state(), PathBuf::from(env!("CARGO_BIN_EXE_bar")));
    let s = transcript::load(&cache, t.file().to_str().unwrap()).unwrap();
    assert_eq!(s.input, 4);
    assert!(transcript::load(&cache, "/nonexistent/x.jsonl").is_none());
    let _ = Path::new("");
}

#[test]
fn huge_values_do_not_overflow() {
    assert_eq!(compact(u64::MAX), "18446744073.7B");
    let t = Tmp::new();
    let big = u64::MAX;
    t.append(&format!(
        "{{\"message\":{{\"id\":\"a\",\"usage\":{{\"input_tokens\":{big},\"output_tokens\":{big},\"cache_read_input_tokens\":{big}}}}}}}\n"
    ));
    t.append(&format!(
        "{{\"message\":{{\"id\":\"b\",\"usage\":{{\"input_tokens\":{big},\"cache_creation_input_tokens\":{big}}}}}}}\n"
    ));
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!(s.input, u64::MAX);
    assert!(!compact(s.input.saturating_add(s.cache_creation)).is_empty());
}

#[test]
fn far_future_mtime_does_not_overflow() {
    let t = Tmp::new();
    t.append(&line("a", 1, 0, 0, 1, T0));
    let ok = std::process::Command::new("touch")
        .args(["-t", "999912312359"])
        .arg(t.file())
        .status()
        .unwrap()
        .success();
    assert!(ok);
    assert_eq!(upd(&t, 1 << 22).0.input, 1);
}

#[test]
fn oversized_state_file_is_ignored_and_long_id_not_deduped() {
    let t = Tmp::new();
    t.append(&line("a", 1, 0, 0, 1, T0));
    upd(&t, 1 << 22);
    let f = std::fs::read_dir(t.state().join("transcript"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&f, " ".repeat(200_000)).unwrap();
    assert_eq!(upd(&t, 1 << 22).0.input, 1, "rebuilt from the file");
    let long = "i".repeat(1000);
    let t2 = Tmp::new();
    t2.append(&line(&long, 2, 0, 0, 1, T0));
    t2.append(&line(&long, 2, 0, 0, 1, T0));
    assert_eq!(upd(&t2, 1 << 22).0.input, 4);
    let st = std::fs::read_dir(t2.state().join("transcript"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(std::fs::metadata(st).unwrap().len() < 1000);
}

#[test]
fn spaced_json_is_read() {
    let t = Tmp::new();
    t.append(
        "{ \"message\" : { \"id\" : \"a\", \"usage\" : { \"input_tokens\" : 8, \"output_tokens\" : 2 } } }\n",
    );
    t.append("{\"text\":\"a \\\"usage\\\": {\"}\n");
    let (s, _) = upd(&t, 1 << 22);
    assert_eq!((s.input, s.output), (8, 2));
}

#[test]
fn job_carries_state_dir() {
    use hop_bar::cache::{Job, Request};
    let r = Request {
        key: "k".into(),
        job: Job::Transcript {
            path: "/a/t.jsonl".into(),
            state: "/s".into(),
        },
        lock: String::new(),
    };
    assert_eq!(Request::from_json(&r.to_json()).unwrap(), r);
    let bad = r.to_json().replace("\"/s\"", "\"rel\"");
    assert!(Request::from_json(&bad).is_err());
    let bad = r.to_json().replace("\"/a/t.jsonl\"", "\"t.jsonl\"");
    assert_eq!(
        Request::from_json(&bad).unwrap_err(),
        "transcript job without absolute path"
    );
}
