use hop_bar::spendhist::{self, Summary, Tz};
use std::fs;
use std::path::PathBuf;

struct Fx(PathBuf);

impl Fx {
    fn new(tag: &str) -> Fx {
        let p = std::env::temp_dir().join(format!("bar-shh-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("claude/projects/p1")).unwrap();
        Fx(p)
    }
    fn append(&self, body: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join("claude/projects/p1/a.jsonl"))
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    fn run(&self, now: i64) -> Summary {
        self.run_tz(Tz::Utc, now)
    }
    fn run_tz(&self, tz: Tz, now: i64) -> Summary {
        let out = spendhist::update(&self.0.join("state"), &self.0.join("claude"), tz, now, None)
            .unwrap();
        Summary::parse(&out).unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const OPUS: &str = "claude-opus-4-5-20251101";
// 2026-11-07T12:00:00Z
const NOW: i64 = 1_794_052_800;
const H: i64 = NOW / 3600;

fn line(id: &str, ts: &str, input: u64) -> String {
    format!(
        "{{\"timestamp\":\"{ts}\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{OPUS}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
    )
}

/// Dollars from hour `h` on; $5 per million input tokens.
fn since(s: &Summary, h: i64) -> f64 {
    s.spend_since(h, &[]).usd
}

#[test]
fn hours_are_utc_hours_whatever_the_day_zone() {
    let fx = Fx::new("utc");
    fx.append(
        &(line("a", "2026-11-07T09:59:59Z", 1_000_000)
            + &line("b", "2026-11-07T10:00:00Z", 2_000_000)
            + &line("c", "2026-11-07T11:30:00Z", 4_000_000)),
    );
    let s = fx.run_tz(Tz::Local, NOW);
    assert_eq!(s.hours[&(H - 3)][OPUS][0], 1_000_000);
    assert_eq!(s.hours[&(H - 2)][OPUS][0], 2_000_000);
    assert_eq!(s.hours[&(H - 1)][OPUS][0], 4_000_000);
    assert!(
        (since(&s, H - 2) - 30.0).abs() < 1e-9,
        "{}",
        since(&s, H - 2)
    );
    assert!((since(&s, H - 3) - 35.0).abs() < 1e-9);
    assert_eq!(since(&s, H), 0.0);
}

#[test]
fn hours_are_deduped_like_days() {
    let fx = Fx::new("dedupe");
    let a = line("a", "2026-11-07T11:00:00Z", 1_000_000);
    fx.append(&(a.clone() + &a));
    fx.run(NOW);
    fx.append(&a);
    let s = fx.run(NOW);
    assert_eq!(s.hours[&(H - 1)][OPUS][0], 1_000_000);
}

#[test]
fn only_the_last_day_of_hours_is_kept() {
    let fx = Fx::new("keep");
    fx.append(&(line("old", "2026-11-06T11:59:59Z", 1) + &line("edge", "2026-11-06T12:00:00Z", 2)));
    let s = fx.run(NOW);
    assert!(!s.hours.contains_key(&(H - 25)));
    assert_eq!(s.hours[&(H - 24)][OPUS][0], 2);
    // a later run drops hours that fell out of the window
    let s = fx.run(NOW + 3600);
    assert!(s.hours.is_empty(), "{:?}", s.hours);
    // the days are untouched
    assert_eq!(s.days["2026-11-06"][OPUS][0], 3);
}

#[test]
fn a_summary_without_hours_still_parses() {
    let s = Summary::parse(r#"{"v":1,"days":{}}"#).unwrap();
    assert!(s.hours.is_empty());
    assert_eq!(s.block_start(NOW), None);
}

fn with_hours(active: &[i64]) -> Summary {
    let hours: Vec<String> = active
        .iter()
        .map(|h| format!("\"{h}\":{{\"{OPUS}\":[1,0,0,0]}}"))
        .collect();
    Summary::parse(&format!(
        r#"{{"v":1,"days":{{}},"hours":{{{}}}}}"#,
        hours.join(",")
    ))
    .unwrap()
}

#[test]
fn block_starts_at_the_first_active_hour_after_a_gap() {
    // one run of activity: the block opened two hours ago
    assert_eq!(with_hours(&[H - 2, H]).block_start(NOW), Some(H - 2));
    // a block lasts five hours: activity six hours ago opened a block
    // that ended, the next active hour opened the current one
    assert_eq!(with_hours(&[H - 6, H - 1]).block_start(NOW), Some(H - 1));
    // back to back: a new block opens where the previous one ends
    let chain = with_hours(&[H - 9, H - 8, H - 6, H - 4, H - 3]);
    assert_eq!(chain.block_start(NOW), Some(H - 4));
    // nothing in the last five hours: no active block
    assert_eq!(with_hours(&[H - 5]).block_start(NOW), None);
    assert_eq!(with_hours(&[H - 4]).block_start(NOW), Some(H - 4));
    assert_eq!(with_hours(&[]).block_start(NOW), None);
}
