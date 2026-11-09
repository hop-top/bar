//! The per-day, per-model token totals a render reads, how they are
//! priced, and the limits that keep them inside one cache value.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::prices::{self, Price};

/// Model key suffix for requests priced at a long-context tier; never
/// part of a model id kept (see `model_key`).
pub(super) const LONG_MARK: char = '#';
/// Longest model id kept, and distinct keys per day (overflow included).
const MAX_MODEL: usize = 128;
pub(super) const MAX_MODELS: usize = 64;
/// Bucket for model ids not kept and for a day's overflow; never priced.
pub const OTHER: &str = "other";
/// Largest summary handed to the cache, below its value cap.
const MAX_SUMMARY: usize = crate::cache::MAX_VALUE - 1024;

/// input, output, cache write, cache read
pub type Tokens = [u64; 4];

/// Tokens per model, under one day or one hour.
pub type Models = BTreeMap<String, Tokens>;

/// Length of a usage block, hours.
pub const BLOCK_HOURS: i64 = 5;

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub days: BTreeMap<String, Models>,
    /// The last day by UTC hour (unix seconds / 3600).
    pub hours: BTreeMap<i64, Models>,
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
            hours: match v.get("hours") {
                Some(h) => hours_from_json(h)?,
                None => BTreeMap::new(),
            },
        })
    }

    /// Dollars for the days whose date starts with `prefix` (`2026-11-07`
    /// for a day, `2026-11` for a month).
    pub fn spend(&self, prefix: &str, overrides: &[Price]) -> Spend {
        price(
            self.days
                .iter()
                .filter(|(d, _)| d.starts_with(prefix))
                .map(|(_, m)| m),
            overrides,
        )
    }

    /// Dollars for the hours from `hour` (unix seconds / 3600) on.
    pub fn spend_since(&self, hour: i64, overrides: &[Price]) -> Spend {
        price(self.hours.range(hour..).map(|(_, m)| m), overrides)
    }

    /// First hour of the usage block running at `now`: a block opens at
    /// the first active hour, lasts `BLOCK_HOURS`, and the next active hour
    /// after it opens the next. `None` when no block is running.
    pub fn block_start(&self, now: i64) -> Option<i64> {
        let mut start: Option<i64> = None;
        for (h, models) in &self.hours {
            if !models.values().any(|t| t.iter().any(|n| *n > 0)) {
                continue;
            }
            if start.is_none_or(|s| *h >= s + BLOCK_HOURS) {
                start = Some(*h);
            }
        }
        start.filter(|s| now < (s + BLOCK_HOURS) * 3600)
    }

    /// Some model with tokens on the days starting with `prefix` has
    /// neither a `set price` rule nor a built-in price, so only the
    /// models.dev snapshot can price it.
    pub fn needs_snapshot(&self, prefix: &str, overrides: &[Price]) -> bool {
        self.days
            .iter()
            .filter(|(d, _)| d.starts_with(prefix))
            .flat_map(|(_, models)| models)
            .filter(|(key, t)| key.as_str() != OTHER && t.iter().any(|n| *n > 0))
            .any(|(key, _)| {
                let model = key.split_once(LONG_MARK).map_or(key.as_str(), |(m, _)| m);
                prices::needs_snapshot(model, overrides)
            })
    }
}

/// Dollars for every model of every bucket in `buckets`.
fn price<'a>(buckets: impl Iterator<Item = &'a Models>, overrides: &[Price]) -> Spend {
    let mut usd = 0.0;
    let mut unknown = false;
    for (key, t) in buckets.flatten() {
        if key == OTHER {
            unknown |= t.iter().any(|n| *n > 0);
            continue;
        }
        let (model, long) = match key.split_once(LONG_MARK) {
            Some((m, _)) => (m, true),
            None => (key.as_str(), false),
        };
        match prices::lookup(model, overrides) {
            Some(r) => {
                let k = if long {
                    prices::long_mult(model, overrides)
                } else {
                    1
                };
                usd += t
                    .iter()
                    .zip(r)
                    .map(|(n, r)| *n as f64 * (r * k) as f64)
                    .sum::<f64>()
                    / 1e12;
            }
            None => unknown |= t.iter().any(|n| *n > 0),
        }
    }
    Spend { usd, unknown }
}

fn models_from_json(v: &Value) -> Option<Models> {
    let mut m = BTreeMap::new();
    for (model, t) in v.as_object()? {
        let a = t.as_array()?;
        let n = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(0);
        m.insert(model.clone(), [n(0), n(1), n(2), n(3)]);
    }
    Some(m)
}

fn models_to_json(models: &Models) -> Value {
    Value::Object(models.iter().map(|(k, t)| (k.clone(), json!(t))).collect())
}

pub(super) fn hours_from_json(v: &Value) -> Option<BTreeMap<i64, Models>> {
    let mut out = BTreeMap::new();
    for (h, models) in v.as_object()? {
        out.insert(h.parse().ok()?, models_from_json(models)?);
    }
    Some(out)
}

pub(super) fn hours_to_json(hours: &BTreeMap<i64, Models>) -> Value {
    Value::Object(
        hours
            .iter()
            .map(|(h, m)| (h.to_string(), models_to_json(m)))
            .collect(),
    )
}

pub(super) fn days_from_json(v: &Value) -> Option<BTreeMap<String, Models>> {
    let mut out = BTreeMap::new();
    for (day, models) in v.as_object()? {
        out.insert(day.clone(), models_from_json(models)?);
    }
    Some(out)
}

pub(super) fn days_to_json(days: &BTreeMap<String, Models>, from: &str) -> Value {
    let mut o = Map::new();
    for (d, models) in days.iter().filter(|(d, _)| d.as_str() >= from) {
        o.insert(d.clone(), models_to_json(models));
    }
    Value::Object(o)
}

/// The summary from day `from`, with `hours`, at most `MAX_SUMMARY` bytes:
/// when too big, each day and hour keeps only its largest models and folds
/// the rest into `OTHER`.
pub(super) fn summary_json(
    days: &BTreeMap<String, Models>,
    hours: &BTreeMap<i64, Models>,
    from: &str,
) -> String {
    let mut keep = MAX_MODELS;
    loop {
        let folded: BTreeMap<String, Models> = days
            .iter()
            .filter(|(d, _)| d.as_str() >= from)
            .map(|(d, m)| (d.clone(), fold_models(m, keep)))
            .collect();
        let hours: BTreeMap<i64, Models> = hours
            .iter()
            .map(|(h, m)| (*h, fold_models(m, keep)))
            .collect();
        let out = json!({
            "v": 1,
            "days": days_to_json(&folded, from),
            "hours": hours_to_json(&hours),
        })
        .to_string();
        if out.len() <= MAX_SUMMARY || keep == 0 {
            return out;
        }
        keep /= 2;
    }
}

/// `models` with all but the `keep` largest (by total tokens) in `OTHER`.
fn fold_models(models: &BTreeMap<String, Tokens>, keep: usize) -> BTreeMap<String, Tokens> {
    let total = |t: &Tokens| t.iter().fold(0u64, |a, n| a.saturating_add(*n));
    let mut ranked: Vec<(&String, &Tokens)> = models.iter().filter(|(k, _)| *k != OTHER).collect();
    if ranked.len() <= keep {
        return models.clone();
    }
    ranked.sort_by_key(|(_, t)| std::cmp::Reverse(total(t)));
    let mut out = BTreeMap::new();
    let mut other = models.get(OTHER).copied().unwrap_or_default();
    for (i, (k, t)) in ranked.into_iter().enumerate() {
        if i < keep {
            out.insert(k.clone(), *t);
        } else {
            add(&mut other, t);
        }
    }
    out.insert(OTHER.to_string(), other);
    out
}

pub(super) fn add(slot: &mut Tokens, t: &Tokens) {
    for (s, n) in slot.iter_mut().zip(t) {
        *s = s.saturating_add(*n);
    }
}

/// The id kept for a model: `OTHER` unless it is short and made of the
/// characters model ids use, so it can never carry `LONG_MARK`.
pub(super) fn model_key(model: &str) -> &str {
    let ok = model.len() <= MAX_MODEL
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:/@+".contains(&b));
    if ok {
        model
    } else {
        OTHER
    }
}
