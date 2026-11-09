//! `ttft`, `tps` and `misses` rendered from a synthetic transcript, and
//! `cachettl` following the detected cache lifetime.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::placeholders::speed::secs;

/// Unix seconds to `YYYY-MM-DDTHH:MM:SS.fffZ` (civil from days, Hinnant).
fn iso(ms: i64) -> String {
    let (secs, ms) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn prompt(ms: i64) -> String {
    format!(
        r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":"go"}}}}"#,
        iso(ms)
    ) + "\n"
}

fn reply(id: &str, ms: i64, out: u64, cr: u64, cc: u64, h1: bool) -> String {
    let (m5, h) = if h1 { (0, cc) } else { (cc, 0) };
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","usage":{{"input_tokens":1,"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cr},"output_tokens":{out},"cache_creation":{{"ephemeral_5m_input_tokens":{m5},"ephemeral_1h_input_tokens":{h}}}}}}}}}"#,
        iso(ms)
    ) + "\n"
}

fn stdin(t: &Tmp, model: &str) -> String {
    serde_json::json!({
        "transcript_path": t.path().join("t.jsonl"),
        "model": {"id": model},
    })
    .to_string()
}

/// Two turns: 1.5 s then 0.5 s to first token; 600 out over 3 s, then
/// 100 out over 0.5 s; the second request found the cache cold after
/// ten idle minutes.
fn session(h1: bool) -> String {
    let t0 = now_ms() - 30 * 60_000;
    let gap = 10 * 60_000;
    prompt(t0)
        + &reply("a", t0 + 1_500, 300, 0, 200_000, h1)
        + &reply("a", t0 + 3_000, 600, 0, 200_000, h1)
        + &prompt(t0 + gap)
        + &reply("b", t0 + gap + 500, 100, 0, 200_000, h1)
}

fn render(conf: &str, body: &str, model: &str) -> String {
    let t = Tmp::new();
    t.write("t.jsonl", body);
    run(&t)
        .conf(conf)
        .stdin(&stdin(&t, model))
        .go()
        .plain
        .trim()
        .to_string()
}

#[test]
fn ttft_and_tps_last_avg_both() {
    let conf = "set left \"#{ttft}|#{ttft[show=avg]}|#{ttft[show=both]}|#{tps}|#{tps[show=avg]}|#{tps[show=both]}\"";
    assert_eq!(
        render(conf, &session(false), "x"),
        "ttft 0.5s|ttft avg 1.0s|ttft 0.5s avg 1.0s|200 t/s|avg 200 t/s|200 avg 200 t/s"
    );
}

#[test]
fn misses_count_cause_and_cold_cost() {
    let conf = "set left \"#{misses}|#{misses[show=count]}|#{misses[show=cold]}\"";
    // 200k written again: (6.25 - 0.50) $/MTok on the 5-minute cache
    assert_eq!(
        render(conf, &session(false), "claude-opus-4-7"),
        "miss 1 ttl 1|miss 1|cold 200k +$1.15"
    );
    // the 1-hour cache writes at twice the input rate: (10 - 0.50) $/MTok;
    // ten idle minutes do not expire it
    assert_eq!(
        render(conf, &session(true), "claude-opus-4-7"),
        "miss 1 inv 1|miss 1|cold 200k +$1.90"
    );
    // an unpriced model shows only the tokens
    assert_eq!(
        render("set left \"#{misses[show=cold]}\"", &session(false), "x"),
        "cold 200k"
    );
}

#[test]
fn nothing_to_show_hides() {
    let t0 = now_ms();
    let warm = prompt(t0) + &reply("a", t0 + 1_000, 0, 0, 0, false);
    assert_eq!(
        render(
            "set left \"#{misses}|#{tps}|#{misses[show=cold]}\"",
            &warm,
            "x"
        ),
        ""
    );
    assert_eq!(render("set left \"#{ttft}\"", &warm, "x"), "ttft 1.0s");
}

#[test]
fn cachettl_follows_the_one_hour_cache() {
    let t0 = now_ms() - 10 * 60_000;
    let body = prompt(t0) + &reply("a", t0 + 500, 5, 0, 5_000, true);
    let out = render("set left \"#{cachettl}\"", &body, "x");
    assert!(out.starts_with("49m") || out.starts_with("50m"), "{out}");
    assert_eq!(
        render("set left \"#{cachettl[ttl=300]}\"", &body, "x"),
        "cold"
    );
    let body = prompt(t0) + &reply("a", t0 + 500, 5, 0, 5_000, false);
    assert_eq!(render("set left \"#{cachettl}\"", &body, "x"), "cold");
}

#[test]
fn seconds_format() {
    assert_eq!(secs(0), "0.0s");
    assert_eq!(secs(840), "0.8s");
    assert_eq!(secs(9_940), "9.9s");
    assert_eq!(secs(9_960), "10s");
    assert_eq!(secs(59_400), "59s");
    assert_eq!(secs(65_000), "1m05s");
    assert_eq!(secs(3_900_000), "1h05m");
    assert_eq!(secs(-5), "0.0s");
}
