use hop_bar::prices::{self, Price};
use hop_bar::spendhist::{self, Summary, Tz};
use std::fs;
use std::path::{Path, PathBuf};

struct Fx(PathBuf);

impl Fx {
    fn new(tag: &str) -> Fx {
        let p = std::env::temp_dir().join(format!("bar-sh-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("claude/projects/p1")).unwrap();
        fs::create_dir_all(p.join("claude/projects/p2")).unwrap();
        Fx(p)
    }
    fn claude(&self) -> PathBuf {
        self.0.join("claude")
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
    fn file(&self, rel: &str) -> PathBuf {
        self.claude().join("projects").join(rel)
    }
    fn append(&self, rel: &str, body: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.file(rel))
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    fn run(&self, now: i64) -> Summary {
        self.run_tz(Tz::Utc, now)
    }
    fn run_tz(&self, tz: Tz, now: i64) -> Summary {
        let out = spendhist::update(&self.state(), &self.claude(), tz, now, None).unwrap();
        Summary::parse(&out).unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(id: &str, ts: &str, model: &str, i: u64, o: u64, cw: u64, cr: u64) -> String {
    format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":{i},\"output_tokens\":{o},\"cache_creation_input_tokens\":{cw},\"cache_read_input_tokens\":{cr}}}}}}}\n"
    )
}

const OPUS: &str = "claude-opus-4-5-20251101";
// 2026-11-07T12:00:00Z
const NOW: i64 = 1_794_052_800;

fn dollars(s: &Summary, prefix: &str) -> f64 {
    s.spend(prefix, &[]).usd
}

fn tok(s: &Summary, day: &str, model: &str) -> [u64; 4] {
    s.days[day][model]
}

#[test]
fn dedupes_across_files_and_within_a_file() {
    let fx = Fx::new("dedupe");
    let a = line("m1", "2026-11-07T01:00:00.000Z", OPUS, 10, 20, 30, 40);
    fx.append("p1/a.jsonl", &(a.clone() + &a));
    fx.append(
        "p2/b.jsonl",
        &(a + &line("m2", "2026-11-07T02:00:00Z", OPUS, 1, 2, 3, 4)),
    );
    let s = fx.run(NOW);
    assert_eq!(tok(&s, "2026-11-07", OPUS), [11, 22, 33, 44]);
}

#[test]
fn append_is_incremental_and_not_double_counted() {
    let fx = Fx::new("incr");
    fx.append(
        "p1/a.jsonl",
        &line("m1", "2026-11-07T01:00:00Z", OPUS, 1, 1, 1, 1),
    );
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [1, 1, 1, 1]);
    // unchanged: same totals
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [1, 1, 1, 1]);
    fx.append(
        "p1/a.jsonl",
        &line("m2", "2026-11-07T03:00:00Z", OPUS, 2, 2, 2, 2),
    );
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [3, 3, 3, 3]);
    // a half-written last line is picked up once finished
    let l = line("m3", "2026-11-07T04:00:00Z", OPUS, 4, 4, 4, 4);
    let (head, tail) = l.split_at(40);
    fx.append("p1/a.jsonl", head);
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [3, 3, 3, 3]);
    fx.append("p1/a.jsonl", tail);
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [7, 7, 7, 7]);
}

#[test]
fn replaced_file_is_rescanned_without_double_counting_ids() {
    let fx = Fx::new("replace");
    let a = line("m1", "2026-11-07T01:00:00Z", OPUS, 5, 5, 5, 5);
    fx.append("p1/a.jsonl", &a);
    fx.run(NOW);
    fs::remove_file(fx.file("p1/a.jsonl")).unwrap();
    fx.append(
        "p1/a.jsonl",
        &(a + &line("m2", "2026-11-07T02:00:00Z", OPUS, 1, 1, 1, 1)),
    );
    assert_eq!(tok(&fx.run(NOW), "2026-11-07", OPUS), [6, 6, 6, 6]);
}

#[test]
fn buckets_by_day_across_midnight_and_month_boundary_utc() {
    let fx = Fx::new("bucket");
    let l = |id, ts, n| line(id, ts, OPUS, n, 0, 0, 0);
    fx.append(
        "p1/a.jsonl",
        &(l("a", "2026-10-31T23:59:59Z", 1)
            + &l("b", "2026-11-01T00:00:00Z", 10)
            + &l("c", "2026-11-01T23:59:59.999Z", 100)
            + &l("d", "2026-11-02T00:00:00Z", 1000)),
    );
    let s = fx.run(NOW);
    assert_eq!(tok(&s, "2026-10-31", OPUS)[0], 1);
    assert_eq!(tok(&s, "2026-11-01", OPUS)[0], 110);
    assert_eq!(tok(&s, "2026-11-02", OPUS)[0], 1000);
    // month prefix sums November only
    let nov = s.spend("2026-11", &[]);
    let all = s.spend("2026-1", &[]);
    assert!(nov.usd > 0.0 && all.usd > nov.usd);
    // $5 per MTok input
    let want = 1110.0 * 5.0 / 1e6;
    assert!((nov.usd - want).abs() < 1e-12, "{}", nov.usd);
}

#[test]
fn cost_uses_all_four_rates() {
    let fx = Fx::new("cost");
    fx.append(
        "p1/a.jsonl",
        &line(
            "m",
            "2026-11-07T01:00:00Z",
            OPUS,
            1_000_000,
            1_000_000,
            1_000_000,
            1_000_000,
        ),
    );
    let s = fx.run(NOW);
    // opus 4.5: 5 + 25 + 6.25 + 0.5
    assert!((dollars(&s, "2026-11-07") - 36.75).abs() < 1e-9);
}

#[test]
fn price_override_beats_builtin_by_longest_prefix() {
    let fx = Fx::new("price");
    fx.append(
        "p1/a.jsonl",
        &line("m", "2026-11-07T01:00:00Z", OPUS, 1_000_000, 0, 0, 0),
    );
    let s = fx.run(NOW);
    let over = [Price::parse("claude-opus", "1", "2", "3", "4").unwrap()];
    // an override beats the built-in even with a shorter prefix
    assert!((s.spend("2026-11-07", &over).usd - 1.0).abs() < 1e-9);
    // longest override prefix wins among overrides
    let over = [
        Price::parse("claude-opus-4-5", "7", "2", "3", "4").unwrap(),
        Price::parse("claude-opus", "1", "2", "3", "4").unwrap(),
    ];
    assert!((s.spend("2026-11-07", &over).usd - 7.0).abs() < 1e-9);
    assert_eq!(
        prices::lookup("claude-opus-4-5-20251101", &[]).unwrap()[0],
        5_000_000
    );
    assert_eq!(
        prices::lookup("claude-opus-4-5-20251101", &over).unwrap()[0],
        7_000_000
    );
    assert!(Price::parse("x", "a", "2", "3", "4").is_none());
    assert!(Price::parse("x", "-1", "2", "3", "4").is_none());
    assert!(Price::parse("", "1", "2", "3", "4").is_none());
}

#[test]
fn unknown_model_counts_tokens_but_not_dollars_and_flags() {
    let fx = Fx::new("unk");
    fx.append(
        "p1/a.jsonl",
        &(line("m1", "2026-11-07T01:00:00Z", OPUS, 1_000_000, 0, 0, 0)
            + &line(
                "m2",
                "2026-11-07T01:00:01Z",
                "mystery-9",
                9_000_000,
                0,
                0,
                0,
            )
            + &line("m3", "2026-11-07T01:00:02Z", "<synthetic>", 0, 0, 0, 0)),
    );
    let s = fx.run(NOW);
    assert_eq!(tok(&s, "2026-11-07", "mystery-9")[0], 9_000_000);
    let sp = s.spend("2026-11-07", &[]);
    assert!(sp.unknown);
    assert!((sp.usd - 5.0).abs() < 1e-9);
    assert!(!s.days["2026-11-07"].contains_key("<synthetic>"));
    // a price rule for it makes it known
    let over = [Price::parse("mystery", "1", "0", "0", "0").unwrap()];
    let sp = s.spend("2026-11-07", &over);
    assert!(!sp.unknown);
    assert!((sp.usd - 14.0).abs() < 1e-9);
}

#[test]
fn keeps_only_the_last_62_days() {
    let fx = Fx::new("prune");
    let day = 86_400;
    let old = "2026-08-01T00:00:00Z"; // 98 days before NOW
    let edge = "2026-09-06T00:00:00Z"; // 62 days before 2026-11-07
    fx.append(
        "p1/a.jsonl",
        &(line("o", old, OPUS, 1, 0, 0, 0)
            + &line("e", edge, OPUS, 2, 0, 0, 0)
            + &line("n", "2026-11-07T00:00:00Z", OPUS, 4, 0, 0, 0)),
    );
    let s = fx.run(NOW);
    assert!(!s.days.contains_key("2026-08-01"));
    assert!(s.days.len() <= 62);
    assert_eq!(tok(&s, "2026-11-07", OPUS)[0], 4);
    // later the edge day falls out too
    let s = fx.run(NOW + 2 * day);
    assert!(!s.days.contains_key("2026-09-06"));
}

#[test]
fn state_file_is_private_and_capped() {
    let fx = Fx::new("cap");
    let body: String = (0..400)
        .map(|i| line(&format!("id{i}"), "2026-11-06T01:00:00Z", OPUS, 1, 1, 1, 1))
        .collect();
    fx.append("p1/a.jsonl", &body);
    let cap = 4096;
    let out = spendhist::update_with(&fx.state(), &fx.claude(), Tz::Utc, NOW, None, cap).unwrap();
    // totals survive trimming of the dedupe set
    assert_eq!(
        tok(&Summary::parse(&out).unwrap(), "2026-11-06", OPUS)[0],
        400
    );
    let f = find_state(&fx.state());
    let meta = fs::metadata(&f).unwrap();
    assert!(meta.len() <= cap as u64, "{}", meta.len());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
}

fn find_state(state: &Path) -> PathBuf {
    let d = state.join("spendhist");
    fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap()
}

#[test]
fn summary_output_stays_under_the_cache_value_cap() {
    let fx = Fx::new("small");
    let mut body = String::new();
    for d in 0..60 {
        for m in 0..5 {
            let ts = format!(
                "2026-{:02}-{:02}T01:00:00Z",
                9 + (d + 6) / 30,
                1 + (d + 6) % 30
            );
            body += &line(
                &format!("x{d}-{m}"),
                &ts,
                &format!("claude-model-{m}"),
                1,
                1,
                1,
                1,
            );
        }
    }
    fx.append("p1/a.jsonl", &body);
    let out = spendhist::update(&fx.state(), &fx.claude(), Tz::Utc, NOW, None).unwrap();
    assert!(out.len() < 60 * 1024, "{}", out.len());
}

#[test]
fn missing_projects_dir_is_a_failure() {
    let fx = Fx::new("none");
    assert!(spendhist::update(&fx.state(), &fx.0.join("nope"), Tz::Utc, NOW, None).is_none());
}

#[test]
fn ignores_lines_without_usable_fields() {
    let fx = Fx::new("junk");
    fx.append(
        "p1/a.jsonl",
        "not json\n{\"usage\":1}\n{\"message\":{\"usage\":{\"input_tokens\":5},\"model\":\"claude-opus-4-5\"}}\n\
         {\"timestamp\":\"nope\",\"message\":{\"id\":\"z\",\"model\":\"claude-opus-4-5\",\"usage\":{\"input_tokens\":5}}}\n",
    );
    let s = fx.run(NOW);
    assert!(s.days.is_empty(), "{:?}", s.days);
}

#[test]
fn utc_and_local_dates() {
    assert_eq!(spendhist::date_of(NOW, Tz::Utc), "2026-11-07");
    assert_eq!(spendhist::date_of(0, Tz::Utc), "1970-01-01");
    assert_eq!(spendhist::date_of(951_782_400, Tz::Utc), "2000-02-29");
}

#[test]
fn model_ids_match_exact_version_or_date_suffix() {
    let five = |m: &str| prices::lookup(m, &[]).map(|r| r[0]);
    assert_eq!(five("claude-opus-5-5"), Some(4_000_000));
    assert_eq!(five("claude-opus-5-5-20260101"), Some(4_000_000));
    assert_eq!(five("claude-opus-5"), Some(5_000_000));
    assert_eq!(five("claude-opus-5-20260101"), Some(5_000_000));
    assert_eq!(five("claude-opus-5-6"), None);
    assert_eq!(five("claude-opus-5-6-20260101"), None);
    assert_eq!(five("claude-opus-4-20250514"), Some(15_000_000));
    assert_eq!(five("claude-opus-4-9"), None);
    assert_eq!(five("claude-opus-4-5-20251101"), Some(5_000_000));
    assert_eq!(five("claude-sonnet-5-2"), None);
}

#[test]
fn long_prompts_use_the_long_context_tier() {
    let fx = Fx::new("long");
    let h = "claude-haiku-5-5";
    fx.append(
        "p1/a.jsonl",
        &(line("s", "2026-11-07T01:00:00Z", h, 90_000, 0, 0, 0)
            // 150k prompt (input + cache read): long tier, 5x
            + &line("l", "2026-11-07T01:00:01Z", h, 50_000, 0, 0, 100_000)),
    );
    let s = fx.run(NOW);
    // short: 90k * 0.1/1M = 0.009, long 0.03; long: 50k*0.5/1M + 100k*0.05/1M = 0.025 + 0.005
    assert!(
        (s.spend("2026-11-07", &[]).usd - 0.039).abs() < 1e-9,
        "{:?} {}",
        s.days,
        s.spend("2026-11-07", &[]).usd
    );
    // exactly 100k is still the short tier
    let fx = Fx::new("long2");
    fx.append(
        "p1/a.jsonl",
        &line("e", "2026-11-07T01:00:00Z", h, 100_000, 0, 0, 0),
    );
    assert!((fx.run(NOW).spend("2026-11-07", &[]).usd - 0.01).abs() < 1e-9);
    // a model without a tier is unaffected
    let fx = Fx::new("long3");
    fx.append(
        "p1/a.jsonl",
        &line("o", "2026-11-07T01:00:00Z", OPUS, 200_000, 0, 0, 0),
    );
    assert!((fx.run(NOW).spend("2026-11-07", &[]).usd - 1.0).abs() < 1e-9);
}

#[test]
fn future_timestamps_count_as_today() {
    let fx = Fx::new("future");
    fx.append(
        "p1/a.jsonl",
        &line("f", "2030-01-01T00:00:00Z", OPUS, 7, 0, 0, 0),
    );
    let s = fx.run(NOW);
    assert_eq!(s.days.keys().collect::<Vec<_>>(), ["2026-11-07"]);
    assert_eq!(tok(&s, "2026-11-07", OPUS)[0], 7);
}

#[test]
fn many_tracked_files_still_fit_the_cap() {
    let fx = Fx::new("manyfiles");
    for i in 0..60 {
        fx.append(
            &format!("p1/f{i}.jsonl"),
            &line(&format!("m{i}"), "2026-11-06T01:00:00Z", OPUS, 1, 1, 1, 1),
        );
    }
    let cap = 3000;
    let out = spendhist::update_with(&fx.state(), &fx.claude(), Tz::Utc, NOW, None, cap).unwrap();
    assert_eq!(
        tok(&Summary::parse(&out).unwrap(), "2026-11-06", OPUS)[0],
        60
    );
    let len = fs::metadata(find_state(&fx.state())).unwrap().len();
    assert!(len <= cap as u64, "{len}");
}
