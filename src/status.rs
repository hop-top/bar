//! Opt-in Claude service status (`set service-status on`): a background
//! job reads the public Statuspage API of status.claude.com and caches a
//! compact summary that `#{status}` renders. The render path never
//! touches the network.

use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// The status page, where `#{status}` links to.
pub const URL: &str = "https://status.claude.com";
const SUMMARY_URL: &str = "https://status.claude.com/api/v2/summary.json";
const INCIDENTS_URL: &str = "https://status.claude.com/api/v2/incidents.json";
/// Cached results stay fresh this long, seconds.
pub const TTL: u64 = 300;
/// Larger than both responses by a wide margin.
const MAX_BYTES: usize = 4 << 20;
/// The history strip covers this many hours at most.
pub const STRIP_HOURS: i64 = 48;
/// Incidents older than this are not kept, hours.
const KEEP_HOURS: i64 = 72;
/// Most incidents kept.
const MAX_INCIDENTS: usize = 40;
/// Incident titles are cut to this many characters.
const MAX_TITLE: usize = 200;

/// Cache key of the job; the history variant also reads the incidents.
pub fn cache_key(strip: bool) -> &'static str {
    if strip {
        "status:strip"
    } else {
        "status:summary"
    }
}

/// How bad: `Ord` follows severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Impact {
    None,
    Minor,
    Major,
    Critical,
}

impl Impact {
    fn parse(s: &str) -> Impact {
        match s {
            "minor" | "degraded_performance" => Impact::Minor,
            "major" | "partial_outage" => Impact::Major,
            "critical" | "major_outage" => Impact::Critical,
            // operational, maintenance and anything unknown
            _ => Impact::None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Impact::None => "none",
            Impact::Minor => "minor",
            Impact::Major => "major",
            Impact::Critical => "critical",
        }
    }

    /// The word shown for it.
    pub fn word(self) -> &'static str {
        match self {
            Impact::None => "ok",
            other => other.name(),
        }
    }
}

/// An incident as kept in the cache; `end` is 0 while it is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incident {
    pub title: String,
    pub impact: Impact,
    pub start: i64,
    pub end: i64,
    /// Affected components; empty means all.
    pub components: Vec<String>,
}

/// What the job cached.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// The page's own overall indicator.
    pub indicator: Option<Impact>,
    pub components: Vec<(String, Impact)>,
    pub incidents: Vec<Incident>,
}

fn matches(name: &str, filter: &[String]) -> bool {
    let n = name.to_lowercase();
    filter.iter().any(|f| n.contains(f.as_str()))
}

/// Lower-cased terms of a component filter (`a|b`); empty for none.
pub fn filter_terms(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split('|')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

impl Snapshot {
    /// Parses a cached value; `None` for an empty or unreadable one.
    pub fn parse(raw: &str) -> Option<Snapshot> {
        let v: Value = serde_json::from_str(raw).ok()?;
        let comps = v["c"].as_array()?;
        Some(Snapshot {
            indicator: v["ind"].as_str().map(Impact::parse),
            components: comps
                .iter()
                .filter_map(|c| Some((c[0].as_str()?.to_string(), Impact::parse(c[1].as_str()?))))
                .collect(),
            incidents: v["i"]
                .as_array()?
                .iter()
                .filter_map(|i| {
                    Some(Incident {
                        title: i["n"].as_str()?.to_string(),
                        impact: Impact::parse(i["m"].as_str()?),
                        start: i["s"].as_i64()?,
                        end: i["e"].as_i64()?,
                        components: i["c"]
                            .as_array()?
                            .iter()
                            .filter_map(|c| c.as_str().map(String::from))
                            .collect(),
                    })
                })
                .collect(),
        })
    }

    /// Current level: the worst of the components matching `filter`, or
    /// the page's own indicator without a filter.
    pub fn level(&self, filter: &[String]) -> Impact {
        if filter.is_empty() {
            return self.indicator.unwrap_or_else(|| {
                self.components
                    .iter()
                    .map(|c| c.1)
                    .max()
                    .unwrap_or(Impact::None)
            });
        }
        self.components
            .iter()
            .filter(|(n, _)| matches(n, filter))
            .map(|c| c.1)
            .max()
            .unwrap_or(Impact::None)
    }

    fn concerns(i: &Incident, filter: &[String]) -> bool {
        filter.is_empty()
            || i.components.is_empty()
            || i.components.iter().any(|c| matches(c, filter))
    }

    /// The active incident to show: the worst, then the newest.
    pub fn active(&self, filter: &[String]) -> Option<&Incident> {
        self.incidents
            .iter()
            .filter(|i| i.end == 0 && i.impact > Impact::None && Self::concerns(i, filter))
            .max_by_key(|i| (i.impact, i.start))
    }

    /// One cell per `hours`, oldest first, covering the last
    /// `ceil(48 / hours)` blocks up to `now`; each is the worst impact of
    /// an incident overlapping it.
    pub fn strip(&self, filter: &[String], now: i64, hours: i64) -> Vec<Impact> {
        let hours = hours.clamp(1, STRIP_HOURS);
        let cells = (STRIP_HOURS + hours - 1) / hours;
        let size = hours * 3600;
        let first = now - cells * size;
        (0..cells)
            .map(|k| {
                let (a, b) = (first + k * size, first + (k + 1) * size);
                self.incidents
                    .iter()
                    .filter(|i| Self::concerns(i, filter))
                    .filter(|i| i.start < b && (i.end == 0 || i.end > a))
                    .map(|i| i.impact)
                    .max()
                    .unwrap_or(Impact::None)
            })
            .collect()
    }
}

fn ts(v: &Value) -> Option<i64> {
    crate::transcript::parse_ts(v.as_str()?)
}

fn incident(v: &Value) -> Option<Incident> {
    let impact = Impact::parse(v["impact"].as_str()?);
    let title: String = v["name"].as_str()?.chars().take(MAX_TITLE).collect();
    Some(Incident {
        title,
        impact,
        start: ts(&v["started_at"]).or_else(|| ts(&v["created_at"]))?,
        end: ts(&v["resolved_at"]).unwrap_or(0),
        components: v["components"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// The compact cached value from a `summary.json` body and, for the
/// history, an `incidents.json` body; `None` when either is not the
/// expected JSON.
pub fn compact(summary: &[u8], incidents: Option<&[u8]>, now: i64) -> Option<String> {
    let s: Value = serde_json::from_slice(summary).ok()?;
    let comps: Vec<Value> = s["components"]
        .as_array()?
        .iter()
        .filter(|c| c["group"] != Value::Bool(true))
        .filter_map(|c| Some(json!([c["name"].as_str()?, c["status"].as_str()?])))
        .collect();
    let list: Vec<Value> = match incidents {
        Some(b) => serde_json::from_slice::<Value>(b).ok()?["incidents"]
            .as_array()?
            .clone(),
        None => s["incidents"].as_array()?.clone(),
    };
    let keep_after = now - KEEP_HOURS * 3600;
    let kept: Vec<Value> = list
        .iter()
        .filter_map(incident)
        .filter(|i| i.impact > Impact::None && (i.end == 0 || i.end >= keep_after))
        .take(MAX_INCIDENTS)
        .map(|i| json!({"n": i.title, "m": i.impact.name(), "s": i.start, "e": i.end, "c": i.components}))
        .collect();
    Some(
        json!({
            "ind": s["status"]["indicator"].as_str().unwrap_or("none"),
            "c": comps,
            "i": kept,
        })
        .to_string(),
    )
}

/// Body of the background job; `None` on a network or format failure
/// (the cache then backs off).
pub fn fetch(strip: bool, limit: Duration) -> Option<String> {
    let end = Instant::now() + limit;
    let left = || {
        end.saturating_duration_since(Instant::now())
            .as_secs()
            .max(1)
    };
    let headers = ["Accept: application/json"];
    let summary = crate::net::get(SUMMARY_URL, &headers, left(), MAX_BYTES).ok()?;
    let incidents = if strip {
        Some(crate::net::get(INCIDENTS_URL, &headers, left(), MAX_BYTES).ok()?)
    } else {
        None
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    compact(&summary, incidents.as_deref(), now)
}
