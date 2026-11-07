//! The models.dev price snapshot: embedded at build time, optionally
//! replaced by a newer copy in bar's data dir (`bar prices update`).
//!
//! File shape: `{"source", "fetched": "YYYY-MM-DD", "providers":
//! {provider: {model: [input, output, cache_write, cache_read,
//! context]}}}`; rates are USD per million tokens, `null` where the
//! provider lists none. A missing cache rate is priced as plain input.

use super::norm::candidates;
use super::Rates;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

const EMBEDDED: &str = include_str!("../../data/models.json");

#[derive(Debug)]
pub struct Snapshot {
    fetched: String,
    /// model id to `(provider, rates)` for every provider listing it.
    by_id: HashMap<String, Vec<(String, Rates)>>,
    len: usize,
}

impl Snapshot {
    pub fn parse(text: &str) -> Option<Snapshot> {
        let v: Value = serde_json::from_str(text).ok()?;
        let fetched = v.get("fetched")?.as_str()?.to_string();
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

    /// Rates for `model`. With a `provider` the match stays inside it.
    /// Without, a vendor named by the id's prefix is tried first, then
    /// every provider; providers that disagree on the price make the
    /// model unknown. The first spelling that matches anything decides.
    pub fn lookup(&self, model: &str, provider: Option<&str>) -> Option<Rates> {
        for c in candidates(model) {
            let Some(all) = self.by_id.get(&c.id) else {
                continue;
            };
            let hint = provider.or(c.vendor.as_deref());
            let of = |p: &str| all.iter().find(|(q, _)| q == p).map(|(_, r)| *r);
            if let Some(r) = hint.and_then(of) {
                return Some(r);
            }
            if provider.is_some() {
                continue;
            }
            let first = all[0].1;
            return all.iter().all(|(_, r)| *r == first).then_some(first);
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
}

/// The snapshot prices come from: the data-dir copy when it is dated
/// the same day as the embedded one or later, else the embedded one.
pub fn active() -> &'static Active {
    static A: OnceLock<Active> = OnceLock::new();
    A.get_or_init(|| {
        let path = user_path();
        let user = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| Snapshot::parse(&t))
            .filter(|s| s.fetched() >= embedded().fetched());
        match user {
            Some(s) => Active {
                snapshot: Box::leak(Box::new(s)),
                user_file: Some(path),
            },
            None => Active {
                snapshot: embedded(),
                user_file: None,
            },
        }
    })
}
