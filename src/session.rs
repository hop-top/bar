//! Parse the statusLine JSON Claude Code sends on stdin.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpendLimit {
    /// `daily`, `weekly` or `monthly`; absent until Claude Code has read
    /// the dollar fields.
    pub period: Option<String>,
    pub resets_at: i64,
    /// `used_percentage`, when present.
    pub used_percentage: Option<f64>,
    /// `used_usd`, when present (Claude Code v2.1.284 and later).
    pub used_usd: Option<f64>,
    /// `limit_usd`, when present.
    pub limit_usd: Option<f64>,
}

/// `context_window.current_usage`: token counts of the last API call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CtxUsage {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

/// `prompt_cache`: Claude Code's own prompt-cache statistics for the
/// main conversation (v2.1.251 and later; causes from v2.1.260).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptCache {
    /// `warm`: the cached prefix is within its lifetime.
    pub warm: Option<bool>,
    /// `ttl` (`"5m"`, `"1h"`), seconds.
    pub ttl: Option<i64>,
    /// `expires_at`, unix seconds; `None` while `null`.
    pub expires_at: Option<i64>,
    /// `misses`.
    pub misses: Option<u64>,
    /// `miss_causes`: diagnosed misses per cause; `None` when absent.
    pub miss_causes: Option<Vec<(String, u64)>>,
    /// `last_miss_cause.causes`; empty when `null` or absent.
    pub last_miss: Vec<String>,
    /// `recache_tokens_if_cold`; `None` while `null`.
    pub recache_if_cold: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub cwd: String,
    /// `workspace.project_dir`: where Claude Code was launched.
    pub project_dir: String,
    pub model: String,
    pub model_id: String,
    pub effort: String,
    pub session_id: String,
    /// `session_name`: the custom name or the generated title.
    pub name: String,
    pub transcript_path: String,
    pub context_used: Option<f64>,
    /// `context_window.context_window_size`; positive or `None`.
    pub context_size: Option<u64>,
    /// `context_window.total_input_tokens`.
    pub context_tokens: Option<u64>,
    /// `context_window.current_usage`; `None` while it is `null`.
    pub context_usage: Option<CtxUsage>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub spend_limit: Option<SpendLimit>,
    /// `cost.total_cost_usd`; finite and non-negative or `None`.
    pub cost_usd: Option<f64>,
    /// `cost.total_duration_ms`; finite and non-negative or `None`.
    pub duration_ms: Option<f64>,
    /// `cost.total_api_duration_ms`; finite and non-negative or `None`.
    pub api_duration_ms: Option<f64>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub version: String,
    pub output_style: String,
    pub vim_mode: String,
    pub agent: String,
    pub thinking: bool,
    pub fast_mode: bool,
    /// `prompt_cache`, when it is an object.
    pub prompt_cache: Option<PromptCache>,
    /// The subagent a `bar subagent` row describes; `None` for the
    /// status line.
    pub task: Option<crate::subagent::Task>,
}

fn str_at(v: &Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn window(v: &Value, key: &str) -> Option<Window> {
    let w = v.get("rate_limits")?.get(key)?;
    Some(Window {
        used_percentage: w.get("used_percentage")?.as_f64()?,
        resets_at: w.get("resets_at")?.as_i64()?,
    })
}

fn spend_limit(v: &Value) -> Option<SpendLimit> {
    let l = v.get("rate_limits")?.get("spend_limit")?;
    Some(SpendLimit {
        period: l.get("period").and_then(Value::as_str).map(str::to_owned),
        resets_at: l.get("resets_at")?.as_i64()?,
        used_percentage: num_at(l, &["used_percentage"]),
        used_usd: num_at(l, &["used_usd"]),
        limit_usd: num_at(l, &["limit_usd"]),
    })
}

/// A finite, non-negative number at `path`.
fn num_at(v: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))?
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

fn current_usage(v: &Value) -> Option<CtxUsage> {
    let u = v.get("context_window")?.get("current_usage")?;
    u.is_object().then_some(())?;
    let n = |k: &str| num_at(u, &[k]).map_or(0, |n| n as u64);
    Some(CtxUsage {
        input: n("input_tokens"),
        cache_creation: n("cache_creation_input_tokens"),
        cache_read: n("cache_read_input_tokens"),
    })
}

/// A cause name as shown: `a-z 0-9 _`, at most 40 bytes.
fn cause_name(s: &str) -> Option<String> {
    let ok = !s.is_empty()
        && s.len() <= 40
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    ok.then(|| s.to_string())
}

/// `"5m"`, `"1h"`, `"30s"` to seconds.
fn ttl_secs(s: &str) -> Option<i64> {
    let (n, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = n.parse().ok().filter(|n| *n > 0)?;
    n.checked_mul(match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return None,
    })
}

fn prompt_cache(v: &Value) -> Option<PromptCache> {
    let p = v.get("prompt_cache").filter(|p| p.is_object())?;
    let n = |k: &str| num_at(p, &[k]).map(|n| n as u64);
    Some(PromptCache {
        warm: p.get("warm").and_then(Value::as_bool),
        ttl: p.get("ttl").and_then(Value::as_str).and_then(ttl_secs),
        expires_at: p.get("expires_at").and_then(Value::as_i64),
        misses: n("misses"),
        miss_causes: p.get("miss_causes").and_then(Value::as_object).map(|m| {
            m.iter()
                .filter_map(|(k, c)| Some((cause_name(k)?, c.as_u64()?)))
                .collect()
        }),
        last_miss: p
            .get("last_miss_cause")
            .and_then(|c| c.get("causes"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|c| cause_name(c.as_str()?)).collect())
            .unwrap_or_default(),
        recache_if_cold: n("recache_tokens_if_cold"),
    })
}

fn count_at(v: &Value, path: &[&str]) -> u64 {
    num_at(v, path).map_or(0, |n| n as u64)
}

fn bool_at(v: &Value, path: &[&str]) -> bool {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Lenient: any malformed input yields defaults; unknown fields ignored.
pub fn parse(input: &str) -> Session {
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return Session::default();
    };
    Session {
        project_dir: str_at(&v, &["workspace", "project_dir"]).unwrap_or_default(),
        cwd: str_at(&v, &["workspace", "current_dir"])
            .or_else(|| str_at(&v, &["cwd"]))
            .unwrap_or_default(),
        model: str_at(&v, &["model", "display_name"])
            .or_else(|| str_at(&v, &["model", "id"]))
            .unwrap_or_default(),
        model_id: str_at(&v, &["model", "id"]).unwrap_or_default(),
        effort: str_at(&v, &["effort", "level"]).unwrap_or_default(),
        session_id: str_at(&v, &["session_id"]).unwrap_or_default(),
        name: str_at(&v, &["session_name"]).unwrap_or_default(),
        transcript_path: str_at(&v, &["transcript_path"]).unwrap_or_default(),
        context_used: v
            .get("context_window")
            .and_then(|c| c.get("used_percentage"))
            .and_then(Value::as_f64),
        context_size: num_at(&v, &["context_window", "context_window_size"])
            .filter(|n| *n >= 1.0)
            .map(|n| n as u64),
        context_tokens: num_at(&v, &["context_window", "total_input_tokens"]).map(|n| n as u64),
        context_usage: current_usage(&v),
        five_hour: window(&v, "five_hour"),
        seven_day: window(&v, "seven_day"),
        spend_limit: spend_limit(&v),
        cost_usd: num_at(&v, &["cost", "total_cost_usd"]),
        duration_ms: num_at(&v, &["cost", "total_duration_ms"]),
        api_duration_ms: num_at(&v, &["cost", "total_api_duration_ms"]),
        lines_added: count_at(&v, &["cost", "total_lines_added"]),
        lines_removed: count_at(&v, &["cost", "total_lines_removed"]),
        version: str_at(&v, &["version"]).unwrap_or_default(),
        output_style: str_at(&v, &["output_style", "name"]).unwrap_or_default(),
        vim_mode: str_at(&v, &["vim", "mode"]).unwrap_or_default(),
        agent: str_at(&v, &["agent", "name"]).unwrap_or_default(),
        thinking: bool_at(&v, &["thinking", "enabled"]),
        fast_mode: bool_at(&v, &["fast_mode"]),
        prompt_cache: prompt_cache(&v),
        task: None,
    }
}
