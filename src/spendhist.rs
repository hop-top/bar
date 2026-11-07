//! Daily token totals per model over all Claude Code transcripts
//! (`<claude dir>/projects/*/*.jsonl`), read incrementally by a background
//! job. One state file keeps a `Mark` per transcript, the totals per local
//! day and the hashes of messages already counted (a resumed session
//! repeats earlier messages in a new file). A render only reads the small
//! summary the job hands to the cache; it never touches a transcript.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::cache::hash_hex;
use crate::linescan::{has_usage_key, Mark};
use crate::prices::{self, Price};
use crate::transcript::parse_ts;
pub use crate::tz::{day_of, day_string, Tz};

/// Days of history kept, today included.
pub const KEEP_DAYS: i64 = 62;
/// Days handed to a render: a month and some slack.
const SUMMARY_DAYS: i64 = 35;
/// Largest state file written.
pub const MAX_STATE: usize = 16 * 1024 * 1024;
const MAX_ID: usize = 256;

/// input, output, cache write, cache read
pub type Tokens = [u64; 4];

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub days: BTreeMap<String, BTreeMap<String, Tokens>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spend {
    pub usd: f64,
    /// Tokens of a model without a price were left out of `usd`.
    pub unknown: bool,
}

impl Summary {
    pub fn parse(s: &str) -> Option<Summary> {
        let v: Value = serde_json::from_str(s).ok()?;
        (v["v"] == 1).then_some(())?;
        Some(Summary {
            days: days_from_json(v.get("days")?)?,
        })
    }

    /// Dollars for the days whose date starts with `prefix` (`2026-11-07`
    /// for a day, `2026-11` for a month).
    pub fn spend(&self, prefix: &str, overrides: &[Price]) -> Spend {
        let mut usd = 0.0;
        let mut unknown = false;
        for (_, models) in self.days.iter().filter(|(d, _)| d.starts_with(prefix)) {
            for (model, t) in models {
                match prices::lookup(model, overrides) {
                    Some(r) => {
                        usd += t
                            .iter()
                            .zip(r)
                            .map(|(n, r)| *n as f64 * r as f64)
                            .sum::<f64>()
                            / 1e12;
                    }
                    None => unknown |= t.iter().any(|n| *n > 0),
                }
            }
        }
        Spend { usd, unknown }
    }
}

fn days_from_json(v: &Value) -> Option<BTreeMap<String, BTreeMap<String, Tokens>>> {
    let mut out = BTreeMap::new();
    for (day, models) in v.as_object()? {
        let mut m = BTreeMap::new();
        for (model, t) in models.as_object()? {
            let a = t.as_array()?;
            let n = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(0);
            m.insert(model.clone(), [n(0), n(1), n(2), n(3)]);
        }
        out.insert(day.clone(), m);
    }
    Some(out)
}

fn days_to_json(days: &BTreeMap<String, BTreeMap<String, Tokens>>, from: &str) -> Value {
    let mut o = Map::new();
    for (d, models) in days.iter().filter(|(d, _)| d.as_str() >= from) {
        let m: Map<String, Value> = models.iter().map(|(k, t)| (k.clone(), json!(t))).collect();
        o.insert(d.clone(), Value::Object(m));
    }
    Value::Object(o)
}

#[derive(Default)]
struct State {
    files: BTreeMap<String, Mark>,
    days: BTreeMap<String, BTreeMap<String, Tokens>>,
    /// Hashes of counted messages, by the day they were counted on.
    seen: BTreeMap<String, Vec<u64>>,
}

impl State {
    fn from_json(s: &str) -> Option<State> {
        let v: Value = serde_json::from_str(s).ok()?;
        (v["v"] == 1).then_some(())?;
        let mut st = State {
            days: days_from_json(v.get("days")?)?,
            ..State::default()
        };
        for (path, m) in v.get("files")?.as_object()? {
            let a = m.as_array()?;
            let n = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(0);
            st.files.insert(
                path.clone(),
                Mark {
                    ino: n(0),
                    size: n(1),
                    mtime_ns: a.get(2).and_then(Value::as_i64).unwrap_or(0),
                    offset: n(3),
                    skip: n(4) == 1,
                },
            );
        }
        for (day, ids) in v.get("seen")?.as_object()? {
            let ids = ids.as_array()?.iter().filter_map(Value::as_u64).collect();
            st.seen.insert(day.clone(), ids);
        }
        Some(st)
    }

    /// The state as JSON, keeping the dedupe hashes of the newest days
    /// that fit in `max` bytes.
    fn to_json(&self, max: usize) -> String {
        let files: Map<String, Value> = self
            .files
            .iter()
            .map(|(p, m)| {
                (
                    p.clone(),
                    json!([m.ino, m.size, m.mtime_ns, m.offset, u8::from(m.skip)]),
                )
            })
            .collect();
        let head = json!({"v": 1, "files": files, "days": days_to_json(&self.days, "")});
        let mut budget = max.saturating_sub(head.to_string().len() + 32);
        let mut seen = Map::new();
        for (day, ids) in self.seen.iter().rev() {
            let cost = ids.len() * 21 + day.len() + 6;
            if cost > budget {
                break;
            }
            budget -= cost;
            seen.insert(day.clone(), json!(ids));
        }
        let mut v = head;
        v["seen"] = Value::Object(seen);
        v.to_string()
    }
}

fn read_state(file: &Path, max: usize) -> Option<State> {
    let mut buf = String::new();
    fs::File::open(file)
        .ok()?
        .take(max as u64 + 1)
        .read_to_string(&mut buf)
        .ok()?;
    (buf.len() <= max).then_some(())?;
    State::from_json(&buf)
}

fn save(file: &Path, body: &str) {
    let Some(dir) = file.parent() else { return };
    let ok = fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .and_then(|()| fs::set_permissions(dir, fs::Permissions::from_mode(0o700)))
        .is_ok();
    if ok {
        let _ = crate::click::write_atomic(file, body);
    }
}

/// Cache key of the summary for a claude dir and zone.
pub fn cache_key(claude_dir: &Path, tz: Tz) -> String {
    format!("spendhist:{}:{}", claude_dir.display(), tz.name())
}

/// `YYYY-MM-DD` of `ts` in `tz`.
pub fn date_of(ts: i64, tz: Tz) -> String {
    day_string(day_of(ts, tz))
}

/// `~/` and `$CLAUDE_CONFIG_DIR` aware location of Claude Code's data:
/// the configured dir, else the environment's, else `~/.claude`.
pub fn claude_dir(configured: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let env = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|d| !d.is_empty());
    let raw = configured.map(String::from).or(env);
    let p = match raw {
        Some(r) => match r.strip_prefix("~/") {
            Some(rest) => home?.join(rest),
            None => PathBuf::from(r),
        },
        None => home?.join(".claude"),
    };
    p.is_absolute().then_some(p)
}

/// Local-day lookups with a small memo: zone offsets change rarely, and
/// transcripts are in time order.
struct Days {
    tz: Tz,
    slot: i64,
    off: i64,
    last: Option<(i64, String)>,
}

impl Days {
    fn date(&mut self, ts: i64) -> String {
        let slot = ts.div_euclid(1800);
        if slot != self.slot {
            self.slot = slot;
            self.off = self.tz.offset(ts);
        }
        let day = (ts + self.off).div_euclid(86_400);
        match &self.last {
            Some((d, s)) if *d == day => s.clone(),
            _ => {
                let s = day_string(day);
                self.last = Some((day, s.clone()));
                s
            }
        }
    }
}

fn id_hash(id: &str, req: &str) -> u64 {
    hash_hex(&format!("{id}\0{req}"))[..12]
        .chars()
        .fold(0, |a, c| a << 4 | u64::from(c.to_digit(16).unwrap_or(0)))
}

struct Fold<'a> {
    st: &'a mut State,
    seen: HashSet<u64>,
    days: Days,
    min_date: String,
    dirty: bool,
}

impl Fold<'_> {
    fn line(&mut self, line: &[u8]) {
        if !has_usage_key(line) {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let msg = &v["message"];
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else {
            return;
        };
        let model = msg["model"].as_str().unwrap_or_default();
        let Some(ts) = v["timestamp"].as_str().and_then(parse_ts) else {
            return;
        };
        if model.is_empty() || model.starts_with('<') {
            return;
        }
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        let t = [
            n("input_tokens"),
            n("output_tokens"),
            n("cache_creation_input_tokens"),
            n("cache_read_input_tokens"),
        ];
        let date = self.days.date(ts);
        if t == [0; 4] || date < self.min_date {
            return;
        }
        let id = msg["id"]
            .as_str()
            .filter(|i| i.len() <= MAX_ID)
            .unwrap_or_default();
        if !id.is_empty() {
            let h = id_hash(id, v["requestId"].as_str().unwrap_or_default());
            if !self.seen.insert(h) {
                return;
            }
            self.st.seen.entry(date.clone()).or_default().push(h);
        }
        let slot = self
            .st
            .days
            .entry(date)
            .or_default()
            .entry(model.to_string())
            .or_default();
        for (s, add) in slot.iter_mut().zip(t) {
            *s = s.saturating_add(add);
        }
        self.dirty = true;
    }
}

/// Transcripts under `<claude dir>/projects/*/`, newest first, skipping
/// those untouched since before the window.
fn transcripts(dir: &Path, oldest_mtime: i64) -> Option<Vec<(String, fs::Metadata)>> {
    let mut out = Vec::new();
    for proj in fs::read_dir(dir.join("projects")).ok()?.flatten() {
        let Ok(files) = fs::read_dir(proj.path()) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            if let Some(m) = fs::metadata(&p).ok().filter(|m| m.is_file()) {
                if m.mtime() >= oldest_mtime {
                    out.push((p.to_string_lossy().into_owned(), m));
                }
            }
        }
    }
    out.sort_by_key(|(_, m)| std::cmp::Reverse(m.mtime()));
    Some(out)
}

/// As `update_with`, with the default state-size cap.
pub fn update(
    state_dir: &Path,
    claude_dir: &Path,
    tz: Tz,
    now: i64,
    deadline: Option<Instant>,
) -> Option<String> {
    update_with(state_dir, claude_dir, tz, now, deadline, MAX_STATE)
}

/// Brings the totals up to date and returns the summary JSON for a
/// render: the last `SUMMARY_DAYS` days. `None` when the claude dir has no
/// `projects`. Stops at `deadline`, keeping progress for the next run.
pub fn update_with(
    state_dir: &Path,
    claude_dir: &Path,
    tz: Tz,
    now: i64,
    deadline: Option<Instant>,
    max_state: usize,
) -> Option<String> {
    let today = day_of(now, tz);
    let min_date = day_string(today - (KEEP_DAYS - 1));
    let files = transcripts(claude_dir, now - (KEEP_DAYS + 1) * 86_400)?;
    let sfile = state_dir.join("spendhist").join(format!(
        "{}.json",
        hash_hex(&format!("{}\0{}", claude_dir.display(), tz.name()))
    ));
    let mut st = read_state(&sfile, max_state).unwrap_or_default();
    let before = st.files.len();
    st.files.retain(|p, _| files.iter().any(|(f, _)| f == p));
    let mut dirty = st.files.len() != before;
    let had_days = st.days.len() + st.seen.len();
    st.days.retain(|d, _| *d >= min_date);
    st.seen.retain(|d, _| *d >= min_date);
    dirty |= st.days.len() + st.seen.len() != had_days;
    let seen = st.seen.values().flatten().copied().collect();
    let mut fold = Fold {
        st: &mut st,
        seen,
        days: Days {
            tz,
            slot: i64::MIN,
            off: 0,
            last: None,
        },
        min_date,
        dirty: false,
    };
    let mut marks = Vec::new();
    for (path, meta) in &files {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        let mut mark = match fold.st.files.get(path) {
            Some(m) if !m.replaced_by(meta) => *m,
            _ => Mark::fresh(meta),
        };
        let mut changed = fold.st.files.get(path) != Some(&mark);
        if mark.pending(meta.len()) {
            if let Ok(mut f) = fs::File::open(path) {
                mark.scan(&mut f, meta.len(), u64::MAX, deadline, |l| fold.line(l));
                changed = true;
            }
        }
        changed |= mark.stamp(meta);
        if changed {
            marks.push((path.clone(), mark));
        }
    }
    dirty |= fold.dirty || !marks.is_empty();
    drop(fold);
    st.files.extend(marks);
    if dirty {
        save(&sfile, &st.to_json(max_state));
    }
    let from = day_string(today - (SUMMARY_DAYS - 1));
    Some(json!({"v": 1, "days": days_to_json(&st.days, &from)}).to_string())
}
