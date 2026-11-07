//! Parse the statusLine JSON Claude Code sends on stdin.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub session_id: String,
    pub context_used: Option<f64>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
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
        effort: str_at(&v, &["effort", "level"]).unwrap_or_default(),
        session_id: str_at(&v, &["session_id"]).unwrap_or_default(),
        context_used: v
            .get("context_window")
            .and_then(|c| c.get("used_percentage"))
            .and_then(Value::as_f64),
        five_hour: window(&v, "five_hour"),
        seven_day: window(&v, "seven_day"),
    }
}
