//! The models.dev price snapshot: embedded at build time, optionally
//! replaced by a newer copy in bar's data dir (`bar prices update`).
//!
//! File shape: `{"source", "fetched": "YYYY-MM-DD", "providers":
//! {provider: {model: [input, output, cache_write, cache_read]}}}`;
//! rates are USD per million tokens, `null` where the
//! provider lists none. A missing cache rate is priced as plain input.

use super::norm::candidates;
use super::Rates;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

const EMBEDDED: &str = include_str!("../../data/models.json");

static PARSES: AtomicUsize = AtomicUsize::new(0);

/// How many snapshots this process has parsed; lets tests check that
/// lookups served by overrides or the built-in table never load one.
pub fn parses() -> usize {
    PARSES.load(Ordering::Relaxed)
}

/// `YYYY-MM-DD` with a plausible month and day.
fn date_ok(d: &str) -> bool {
    let b = d.as_bytes();
    let num = |r: std::ops::Range<usize>| d.get(r).and_then(|t| t.parse::<u32>().ok());
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && num(5..7).is_some_and(|m| (1..=12).contains(&m))
        && num(8..10).is_some_and(|d| (1..=31).contains(&d))
}

#[derive(Debug)]
pub struct Snapshot {
    fetched: String,
    /// model id to `(provider, rates)` for every provider listing it.
    by_id: HashMap<String, Vec<(String, Rates)>>,
    len: usize,
}

impl Snapshot {
    pub fn parse(text: &str) -> Option<Snapshot> {
        PARSES.fetch_add(1, Ordering::Relaxed);
        let v: Value = serde_json::from_str(text).ok()?;
        let fetched = v
            .get("fetched")?
            .as_str()
            .filter(|d| date_ok(d))?
            .to_string();
        let mut by_id: HashMap<String, Vec<(String, Rates)>> = HashMap::new();
        let mut len = 0;
        for (provider, models) in v.get("providers")?.as_object()? {
            for (id, e) in models.as_object()? {
                if let Some(r) = rates(e) {
                    by_id
                        .entry(id.clone())
                        .or_default()
                        .push((provider.clone(), r));
                    len += 1;
                }
            }
        }
        Some(Snapshot {
            fetched,
            by_id,
            len,
        })
    }

    /// Date the data was fetched, `YYYY-MM-DD`.
    pub fn fetched(&self) -> &str {
        &self.fetched
    }

    /// Number of provider/model entries.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Rates for `model`. A `provider`, or the vendor an id's prefix
    /// names, keeps the match inside that provider. With neither, every
    /// provider that lists the id counts; free (all-zero) listings are
    /// ignored when a priced one exists, and providers that still
    /// disagree make the model unknown. The first spelling that matches
    /// anything decides.
    pub fn lookup(&self, model: &str, provider: Option<&str>) -> Option<Rates> {
        for c in candidates(model) {
            let Some(all) = self.by_id.get(&c.id) else {
                continue;
            };
            if let Some(p) = provider.or(c.vendor.as_deref()) {
                match all.iter().find(|(q, _)| q == p) {
                    Some((_, r)) => return Some(*r),
                    None => continue,
                }
            }
            let priced: Vec<Rates> = all
                .iter()
                .map(|(_, r)| *r)
                .filter(|r| *r != [0; 4])
                .collect();
            let first = *priced.first().or(Some(&all[0].1))?;
            return if priced.is_empty() || priced.iter().all(|r| *r == first) {
                Some(first)
            } else {
                None
            };
        }
        None
    }
}

fn rates(e: &Value) -> Option<Rates> {
    let a = e.as_array()?;
    let n = |i: usize| -> Option<Option<u64>> {
        match a.get(i) {
            None | Some(Value::Null) => Some(None),
            Some(v) => {
                let f = v
                    .as_f64()
                    .filter(|f| f.is_finite() && (0.0..=1e6).contains(f))?;
                Some(Some((f * 1e6).round() as u64))
            }
        }
    };
    let (input, output) = (n(0)??, n(1)??);
    Some([
        input,
        output,
        n(2)?.unwrap_or(input),
        n(3)?.unwrap_or(input),
    ])
}

/// The snapshot built into the binary.
pub fn embedded() -> &'static Snapshot {
    static S: OnceLock<Snapshot> = OnceLock::new();
    S.get_or_init(|| {
        Snapshot::parse(EMBEDDED).unwrap_or(Snapshot {
            fetched: String::new(),
            by_id: HashMap::new(),
            len: 0,
        })
    })
}

/// `models.json` in bar's data dir (`XDG_DATA_HOME/bar`).
pub fn user_path() -> PathBuf {
    hop_top_kit::xdg::raw_data_dir("bar").join("models.json")
}

pub struct Active {
    pub snapshot: &'static Snapshot,
    /// The data-dir copy, when it is in use.
    pub user_file: Option<PathBuf>,
    /// Why a data-dir copy that exists was not used.
    pub ignored: Option<String>,
}

/// Today as `YYYY-MM-DD` (UTC).
pub fn today() -> String {
    crate::tz::day_string(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64 / 86_400),
    )
}

/// The snapshot prices come from: the data-dir copy when it is valid,
/// not dated in the future, and dated the same day as the embedded one
/// or later; else the embedded one.
pub fn active() -> &'static Active {
    static A: OnceLock<Active> = OnceLock::new();
    A.get_or_init(|| {
        let path = user_path();
        let mut ignored = None;
        if let Ok(text) = std::fs::read_to_string(&path) {
            match Snapshot::parse(&text) {
                None => ignored = Some("not a valid price file".to_string()),
                Some(s) if s.fetched() > today().as_str() => {
                    ignored = Some(format!("dated {} (in the future)", s.fetched()))
                }
                Some(s) if s.fetched() < embedded().fetched() => {
                    ignored = Some(format!("dated {} (older than built in)", s.fetched()))
                }
                Some(s) => {
                    return Active {
                        snapshot: Box::leak(Box::new(s)),
                        user_file: Some(path),
                        ignored: None,
                    }
                }
            }
        }
        Active {
            snapshot: embedded(),
            user_file: None,
            ignored,
        }
    })
}
