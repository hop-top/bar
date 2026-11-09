//! Turn timing and prompt-cache misses, folded from the same incremental
//! transcript scan as the token totals. Kept per transcript: the request
//! still being written, the last turn, and running sums; nothing per turn.
//!
//! Times come from entry `timestamp`s, which Claude Code stamps when it
//! writes an entry: a user entry when the prompt or tool result is
//! recorded, an assistant entry when a content block finishes.

use serde_json::{json, Value};

/// Prompt cache lifetimes, seconds.
pub const TTL_5M: i64 = 300;
pub const TTL_1H: i64 = 3600;
/// Smallest prompt the API caches; a smaller cached prefix losing its
/// reads is not counted as a miss.
const MIN_PREFIX: u64 = 1024;
/// Message ids longer than this are not used to tell requests apart.
const MAX_ID: usize = 256;

/// One assistant entry, as the timing and cache tracking need it.
pub(super) struct Req<'a> {
    pub id: &'a str,
    pub ts_ms: i64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub one_hour: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Turns {
    /// Prompt not yet answered.
    prompt_ms: Option<i64>,
    /// Latest user entry: when the next request was sent.
    start_ms: Option<i64>,
    /// The request being written: id, start, last entry, output tokens.
    open_id: String,
    open_start: Option<i64>,
    open_end: i64,
    open_out: u64,
    /// Last turn: time to first token, and finished requests.
    ttft_ms: Option<i64>,
    turn_out: u64,
    turn_ms: i64,
    /// Whole session: time to first token sum and count, finished requests.
    ttft_sum: i64,
    ttft_n: u64,
    out: u64,
    ms: i64,
    /// Cached prompt prefix of the last request (cache read plus write).
    prefix: u64,
    /// The last split cache write went to the 1-hour cache.
    one_hour: bool,
    /// A compaction since the last request: its prompt is new.
    compacted: bool,
    misses_ttl: u64,
    misses_inv: u64,
}

fn rate(out: u64, ms: i64) -> Option<f64> {
    (out > 0 && ms > 0).then(|| out as f64 * 1000.0 / ms as f64)
}

impl Turns {
    pub(super) fn prompt(&mut self, ts_ms: Option<i64>) {
        if let Some(t) = ts_ms {
            self.prompt_ms = Some(t);
            self.start_ms = Some(t);
        }
    }

    pub(super) fn tool_result(&mut self, ts_ms: Option<i64>) {
        if let Some(t) = ts_ms {
            self.start_ms = Some(t);
        }
    }

    /// The conversation was rewritten: the cached prefix no longer exists.
    pub(super) fn compaction(&mut self) {
        self.compacted = true;
        self.prefix = 0;
    }

    /// An assistant entry: either another content block of the open
    /// request, or the first entry of a new one.
    pub(super) fn assistant(&mut self, r: &Req) {
        if !r.id.is_empty() && r.id == self.open_id {
            self.open_end = self.open_end.max(r.ts_ms);
            self.open_out = r.output;
            return;
        }
        let prev_end = (self.open_end > 0).then_some(self.open_end);
        self.close();
        let start = self.start_ms.take();
        if let Some(p) = self.prompt_ms.take() {
            let ttft = r.ts_ms - p;
            self.ttft_ms = (ttft >= 0).then_some(ttft);
            if ttft >= 0 {
                self.ttft_sum = self.ttft_sum.saturating_add(ttft);
                self.ttft_n += 1;
            }
            self.turn_out = 0;
            self.turn_ms = 0;
        }
        self.check_cache(r, start, prev_end);
        self.open_id = if r.id.len() <= MAX_ID {
            r.id.to_string()
        } else {
            String::new()
        };
        self.open_start = start;
        self.open_end = r.ts_ms;
        self.open_out = r.output;
    }

    /// Output tokens and duration of the open request; zero without a
    /// known start.
    fn open(&self) -> (u64, i64) {
        match self.open_start {
            Some(s) if self.open_end > s => (self.open_out, self.open_end - s),
            _ => (0, 0),
        }
    }

    fn close(&mut self) {
        let (o, d) = self.open();
        if d > 0 {
            self.turn_out = self.turn_out.saturating_add(o);
            self.turn_ms = self.turn_ms.saturating_add(d);
            self.out = self.out.saturating_add(o);
            self.ms = self.ms.saturating_add(d);
        }
        self.open_start = None;
        self.open_out = 0;
    }

    /// A request that reads under half of the previous request's cached
    /// prefix and writes more than it reads found the cache cold. `sent`:
    /// the user entry that sent it; without one the idle time is unknown
    /// and the miss is not put down to the TTL.
    fn check_cache(&mut self, r: &Req, sent: Option<i64>, prev_end: Option<i64>) {
        let prev = self.prefix;
        if std::mem::take(&mut self.compacted) {
            // a compacted conversation starts a new prompt
        } else if prev >= MIN_PREFIX
            && r.cache_read.saturating_mul(2) < prev
            && r.cache_creation > r.cache_read
        {
            let idle = sent.zip(prev_end).map(|(s, e)| s - e);
            if idle.is_some_and(|i| i > self.ttl() * 1000) {
                self.misses_ttl += 1;
            } else {
                self.misses_inv += 1;
            }
        }
        if let Some(h) = r.one_hour {
            self.one_hour = h;
        }
        self.prefix = r.cache_read.saturating_add(r.cache_creation);
    }

    /// Time to first token of the last answered prompt, ms.
    pub fn ttft_last(&self) -> Option<i64> {
        self.ttft_ms
    }

    /// Mean time to first token over the session's answered prompts, ms.
    pub fn ttft_avg(&self) -> Option<i64> {
        (self.ttft_n > 0).then(|| self.ttft_sum / self.ttft_n as i64)
    }

    /// Output tokens per second over the last turn's requests.
    pub fn tps_last(&self) -> Option<f64> {
        let (o, d) = self.open();
        rate(
            self.turn_out.saturating_add(o),
            self.turn_ms.saturating_add(d),
        )
    }

    /// Output tokens per second over the session's requests.
    pub fn tps_avg(&self) -> Option<f64> {
        let (o, d) = self.open();
        rate(self.out.saturating_add(o), self.ms.saturating_add(d))
    }

    /// Cache misses after the cache outlived its TTL, and the others.
    pub fn misses(&self) -> (u64, u64) {
        (self.misses_ttl, self.misses_inv)
    }

    /// Prompt cache lifetime, seconds, from the last split cache write.
    pub fn ttl(&self) -> i64 {
        if self.one_hour {
            TTL_1H
        } else {
            TTL_5M
        }
    }

    /// The last split cache write went to the 1-hour cache.
    pub fn one_hour(&self) -> bool {
        self.one_hour
    }

    /// Cached prompt prefix of the last request, tokens: what a cold cache
    /// writes again on the next request.
    pub fn prefix(&self) -> u64 {
        self.prefix
    }

    /// Stand-in values for `bar preview`.
    pub fn sample() -> Turns {
        Turns {
            ttft_ms: Some(1_400),
            turn_out: 1_800,
            turn_ms: 42_000,
            ttft_sum: 9_000,
            ttft_n: 5,
            out: 4_100,
            ms: 108_000,
            prefix: 180_000,
            misses_ttl: 1,
            misses_inv: 1,
            ..Turns::default()
        }
    }

    pub(super) fn to_json(&self) -> Value {
        json!({
            "pm": self.prompt_ms, "sm": self.start_ms, "oid": self.open_id,
            "os": self.open_start, "oe": self.open_end, "oo": self.open_out,
            "tt": self.ttft_ms, "to": self.turn_out, "td": self.turn_ms,
            "ts": self.ttft_sum, "tn": self.ttft_n, "o": self.out, "d": self.ms,
            "px": self.prefix, "h1": self.one_hour, "cp": self.compacted,
            "mt": self.misses_ttl, "mi": self.misses_inv,
        })
    }

    pub(super) fn from_json(v: &Value) -> Turns {
        let u = |k: &str| v[k].as_u64().unwrap_or(0);
        let i = |k: &str| v[k].as_i64().unwrap_or(0);
        let o = |k: &str| v[k].as_i64();
        let b = |k: &str| v[k].as_bool().unwrap_or(false);
        Turns {
            prompt_ms: o("pm"),
            start_ms: o("sm"),
            open_id: v["oid"]
                .as_str()
                .filter(|s| s.len() <= MAX_ID)
                .unwrap_or_default()
                .into(),
            open_start: o("os"),
            open_end: i("oe"),
            open_out: u("oo"),
            ttft_ms: o("tt"),
            turn_out: u("to"),
            turn_ms: i("td"),
            ttft_sum: i("ts"),
            ttft_n: u("tn"),
            out: u("o"),
            ms: i("d"),
            prefix: u("px"),
            one_hour: b("h1"),
            compacted: b("cp"),
            misses_ttl: u("mt"),
            misses_inv: u("mi"),
        }
    }
}
