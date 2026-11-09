//! Parse the statusLine JSON Claude Code sends on stdin.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpendLimit {
    pub period: String,
    pub resets_at: i64,
}

/// `context_window.current_usage`: token counts of the last API call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CtxUsage {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub cwd: String,
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
        period: l.get("period")?.as_str()?.to_owned(),
        resets_at: l.get("resets_at")?.as_i64()?,
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
    }
}
