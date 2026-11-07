//! Token and cache totals from the session transcript (JSONL), read
//! incrementally. A per-transcript state file in the state dir holds the
//! byte offset consumed so far and the running totals, so a render reads
//! only what was appended since. A render reads at most `INLINE_CAP`
//! bytes; the rest of a big file is left to a background `bar refresh`.

use std::fs;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use crate::cache::{hash_hex, Cache, Job};
use crate::linescan::{has_usage_key, Mark};

/// Most bytes one render reads.
pub const INLINE_CAP: u64 = 1024 * 1024;
/// Seconds a finished background scan counts as fresh.
const SCAN_TTL: u64 = 2;
/// Larger state files are ignored (and rebuilt).
const MAX_STATE: u64 = 64 * 1024;
/// Message ids longer than this are not used for dedupe.
const MAX_ID: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
    /// Unix seconds of the last assistant message.
    pub last_ts: Option<i64>,
    /// Transcript mtime, unix seconds.
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub stats: Stats,
    /// The read reached the end of the file (nothing left to defer).
    pub complete: bool,
}

#[derive(Debug, Clone, Default)]
struct State {
    mark: Mark,
    stats: Stats,
    last_id: String,
}

impl State {
    fn to_json(&self) -> String {
        let (s, m) = (&self.stats, &self.mark);
        json!({
            "v": 1, "ino": m.ino, "size": m.size, "mtime_ns": m.mtime_ns,
            "offset": m.offset, "skip": m.skip, "in": s.input,
            "cc": s.cache_creation, "cr": s.cache_read, "out": s.output,
            "last_ts": s.last_ts, "last_id": self.last_id,
        })
        .to_string()
    }

    fn from_json(s: &str) -> Option<State> {
        let v: Value = serde_json::from_str(s).ok()?;
        (v["v"] == 1).then_some(())?;
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        Some(State {
            mark: Mark {
                ino: n("ino"),
                size: n("size"),
                mtime_ns: v["mtime_ns"].as_i64().unwrap_or(0),
                offset: n("offset"),
                skip: v["skip"].as_bool().unwrap_or(false),
            },
            stats: Stats {
                input: n("in"),
                cache_creation: n("cc"),
                cache_read: n("cr"),
                output: n("out"),
                last_ts: v["last_ts"].as_i64(),
                mtime: 0,
            },
            last_id: v["last_id"]
                .as_str()
                .filter(|i| i.len() <= MAX_ID)
                .unwrap_or_default()
                .into(),
        })
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` to unix seconds.
pub fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |a: usize, z: usize| -> Option<i64> {
        let part = b.get(a..z)?;
        part.iter().all(u8::is_ascii_digit).then_some(())?;
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    // days from civil (Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

/// Folds one transcript line into the totals; anything unexpected is
/// skipped.
fn apply(st: &mut State, line: &[u8]) {
    if !has_usage_key(line) {
        return;
    }
    let Ok(v) = serde_json::from_slice::<Value>(line) else {
        return;
    };
    let Some(u) = v.get("message").and_then(|m| m.get("usage")) else {
        return;
    };
    if !u.is_object() {
        return;
    }
    if let Some(ts) = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_ts)
    {
        st.stats.last_ts = Some(st.stats.last_ts.map_or(ts, |t| t.max(ts)));
    }
    // a message is written once per content block, usage repeated
    let id = v["message"]["id"].as_str().unwrap_or_default();
    if id.is_empty() || id.len() > MAX_ID {
        st.last_id.clear();
    } else {
        if id == st.last_id {
            return;
        }
        st.last_id = id.to_string();
    }
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let s = &mut st.stats;
    s.input = s.input.saturating_add(n("input_tokens"));
    s.cache_creation = s
        .cache_creation
        .saturating_add(n("cache_creation_input_tokens"));
    s.cache_read = s.cache_read.saturating_add(n("cache_read_input_tokens"));
    s.output = s.output.saturating_add(n("output_tokens"));
}

fn state_file(state_dir: &Path, path: &Path) -> std::path::PathBuf {
    let key = hash_hex(&path.to_string_lossy());
    state_dir.join("transcript").join(format!("{key}.json"))
}

fn read_state(file: &Path) -> Option<State> {
    let mut buf = String::new();
    fs::File::open(file)
        .ok()?
        .take(MAX_STATE + 1)
        .read_to_string(&mut buf)
        .ok()?;
    (buf.len() as u64 <= MAX_STATE).then_some(())?;
    State::from_json(&buf)
}

fn save(state_dir: &Path, file: &Path, st: &State) {
    let dir = state_dir.join("transcript");
    let ok = fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .and_then(|()| fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)))
        .is_ok();
    if ok {
        let _ = crate::click::write_atomic(file, &st.to_json());
    }
}

/// Brings the stored totals up to date, reading at most `cap` bytes (and
/// stopping at `deadline`). `None` when the transcript is not a readable
/// file. A changed inode, a smaller file, or an mtime that moved without
/// the size growing means the file was replaced: rescan from the start.
pub fn update(
    state_dir: &Path,
    path: &Path,
    cap: u64,
    deadline: Option<Instant>,
) -> Option<Progress> {
    let meta = fs::metadata(path).ok().filter(fs::Metadata::is_file)?;
    let size = meta.len();
    let sfile = state_file(state_dir, path);
    let mut st = read_state(&sfile).unwrap_or_default();
    let mut dirty = st.mark.replaced_by(&meta);
    if dirty {
        st = State {
            mark: Mark::fresh(&meta),
            ..State::default()
        };
    }
    let mut complete = true;
    if st.mark.pending(size) {
        let mut f = fs::File::open(path).ok()?;
        let mut mark = st.mark;
        complete = mark.scan(&mut f, size, cap, deadline, |l| apply(&mut st, l));
        st.mark = mark;
        dirty = true;
    }
    dirty |= st.mark.stamp(&meta);
    if dirty {
        save(state_dir, &sfile, &st);
    }
    let mut stats = st.stats;
    stats.mtime = meta.mtime();
    Some(Progress { stats, complete })
}

thread_local! {
    static MEMO: std::cell::RefCell<std::collections::HashMap<String, Option<Stats>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// How many times `load` did real work (not served from the per-process
/// memo); a test hook.
pub static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Totals for a render, computed once per process per transcript so every
/// placeholder shares one read. Reads up to `INLINE_CAP` new bytes; when
/// the file has more, asks the cache to finish it in the background and
/// returns what is known so far.
pub fn load(cache: &Cache, path: &str) -> Option<Stats> {
    if path.is_empty() {
        return None;
    }
    let key = format!("{}\0{path}", cache.state_dir().display());
    if let Some(hit) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = load_uncached(cache, path);
    MEMO.with(|m| m.borrow_mut().insert(key, out.clone()));
    out
}

fn load_uncached(cache: &Cache, path: &str) -> Option<Stats> {
    let p = update(cache.state_dir(), Path::new(path), INLINE_CAP, None)?;
    if !p.complete {
        let job = Job::Transcript {
            path: path.into(),
            state: cache.state_dir().to_string_lossy().into_owned(),
        };
        cache.read_or_refresh(&format!("transcript:{path}"), SCAN_TTL, &job);
    }
    Some(p.stats)
}
