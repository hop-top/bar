//! `bar subagent`: Claude Code's `subagentStatusLine` command. Reads the
//! visible subagent rows (one JSON object on stdin, with a `tasks` array)
//! and writes one `{"id", "content"}` JSON line per row it draws.

use crate::cache::Cache;
use crate::conf::{Config, LoadResult};
use crate::expand::CommandRunner;
use crate::session::Session;
use serde_json::{json, Value};

/// Layout of a row when the config sets no `subagent`.
pub const DEFAULT_LAYOUT: &str = "#{task.status} #{task.name[max=24]} │ #{task.label[max=50]} │ #{model}#{effort} │ #{task.tokens} │ #{task.elapsed}";

/// Most rows drawn; Claude Code shows far fewer.
pub const MAX_TASKS: usize = 64;
/// Token samples kept per task (Claude Code sends up to 16).
pub const MAX_SAMPLES: usize = 16;
/// Longest task id echoed back; longer ones are skipped.
const MAX_ID: usize = 256;
/// Longest text field kept, in chars.
const MAX_TEXT: usize = 1024;

/// One entry of `tasks`: a subagent row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Task {
    pub id: String,
    /// `name`: what the subagent is addressed by, when it has one.
    pub name: String,
    /// `type`, such as `local_agent`.
    pub kind: String,
    /// `agentType`, such as `Explore`.
    pub agent_type: String,
    /// `status`: `running`, `completed`, `failed`, `killed`, ...
    pub status: String,
    pub description: String,
    /// `label`: progress summary, else the description.
    pub label: String,
    /// `startTime`, Unix milliseconds.
    pub start_ms: Option<f64>,
    /// `model`: the resolved model id.
    pub model: String,
    /// `effort`: a level, or a token budget as a number.
    pub effort: String,
    /// `contextWindowSize`.
    pub context_size: Option<u64>,
    /// `tokenCount`.
    pub tokens: Option<u64>,
    /// `tokenSamples`, oldest first, at most [`MAX_SAMPLES`].
    pub samples: Vec<u64>,
    pub cwd: String,
}

impl Task {
    pub fn running(&self) -> bool {
        self.status.is_empty() || self.status == "running"
    }
}

/// The `subagentStatusLine` input.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Payload {
    pub session_id: String,
    pub cwd: String,
    /// `columns`: the usable width of a row.
    pub columns: Option<usize>,
    pub tasks: Vec<Task>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .map(|s| s.chars().take(MAX_TEXT).collect())
        .unwrap_or_default()
}

fn count(v: &Value, key: &str) -> Option<u64> {
    let n = v.get(key)?.as_f64()?;
    (n.is_finite() && n >= 0.0).then_some(n as u64)
}

fn task(v: &Value) -> Option<Task> {
    let id = text(v, "id");
    if id.is_empty() || id.len() > MAX_ID {
        return None;
    }
    let effort = match v.get("effort") {
        Some(Value::String(s)) => s.chars().take(MAX_TEXT).collect(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let samples: Vec<u64> = v
        .get("tokenSamples")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_f64().filter(|n| n.is_finite() && *n >= 0.0))
                .map(|n| n as u64)
                .collect()
        })
        .unwrap_or_default();
    let skip = samples.len().saturating_sub(MAX_SAMPLES);
    Some(Task {
        id,
        name: text(v, "name"),
        kind: text(v, "type"),
        agent_type: text(v, "agentType"),
        status: text(v, "status"),
        description: text(v, "description"),
        label: text(v, "label"),
        start_ms: v
            .get("startTime")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0),
        model: text(v, "model"),
        effort,
        context_size: count(v, "contextWindowSize").filter(|n| *n >= 1),
        tokens: count(v, "tokenCount"),
        samples: samples[skip..].to_vec(),
        cwd: text(v, "cwd"),
    })
}

/// Lenient: malformed input or entries are dropped; unknown fields ignored.
pub fn parse(input: &str) -> Payload {
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return Payload::default();
    };
    let tasks = v
        .get("tasks")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(task).take(MAX_TASKS).collect())
        .unwrap_or_default();
    Payload {
        session_id: text(&v, "session_id"),
        cwd: text(&v, "cwd"),
        columns: count(&v, "columns")
            .filter(|n| *n >= 1)
            .map(|n| n.min(10_000) as usize),
        tasks,
    }
}

/// `claude-opus-5-5` to `Opus 5.5`, `claude-haiku-4-5-20251001` to
/// `Haiku 4.5`; an id of any other form as it is.
pub fn model_name(id: &str) -> String {
    let canon = crate::prices::norm::canonical(id);
    let Some(rest) = canon.strip_prefix("claude-") else {
        return id.to_string();
    };
    let mut it = rest.split('-');
    let family = it.next().unwrap_or_default();
    let version: Vec<&str> = it
        .take_while(|p| !p.is_empty() && p.len() <= 2 && p.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    let mut chars = family.chars();
    let (Some(first), false) = (chars.next(), version.is_empty()) else {
        return id.to_string();
    };
    format!(
        "{}{} {}",
        first.to_ascii_uppercase(),
        chars.as_str(),
        version.join(".")
    )
}

/// The session a row's placeholders read: the task's model, effort,
/// context and directory. No session id or transcript, so nothing links
/// to or reads the main conversation.
pub fn session(p: &Payload, t: &Task) -> Session {
    let cwd = if t.cwd.is_empty() { &p.cwd } else { &t.cwd };
    let used = match (t.tokens, t.context_size) {
        (Some(n), Some(size)) => Some(n as f64 * 100.0 / size as f64),
        _ => None,
    };
    Session {
        cwd: cwd.clone(),
        project_dir: p.cwd.clone(),
        model: if t.model.is_empty() {
            String::new()
        } else {
            model_name(&t.model)
        },
        model_id: t.model.clone(),
        effort: t.effort.clone(),
        context_used: used,
        context_size: t.context_size,
        context_tokens: t.tokens,
        task: Some(t.clone()),
        ..Session::default()
    }
}

/// `(id, content)` per task whose row renders non-empty, in input order.
pub fn rows(
    p: &Payload,
    config: &Config,
    now: i64,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
) -> Vec<(String, String)> {
    let layout = config.subagent.as_deref().unwrap_or(DEFAULT_LAYOUT);
    if layout.is_empty() {
        return Vec::new();
    }
    p.tasks
        .iter()
        .filter_map(|t| {
            let s = session(p, t);
            let row = crate::render::render_row(&s, config, layout, p.columns, now, runner, cache);
            (!row.is_empty()).then(|| (t.id.clone(), row))
        })
        .collect()
}

/// The JSON lines Claude Code reads: `{"id": ..., "content": ...}` each.
pub fn lines(rows: &[(String, String)]) -> String {
    rows.iter()
        .map(|(id, content)| format!("{}\n", json!({"id": id, "content": content})))
        .collect()
}

/// What `bar subagent` prints for `input` under `load`. A config error
/// or an empty layout prints nothing, so Claude Code keeps its own rows.
pub fn run(
    input: &str,
    load: impl FnOnce(&Payload) -> LoadResult,
    now: i64,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
) -> String {
    let p = parse(input);
    if p.tasks.is_empty() {
        return String::new();
    }
    let config = match load(&p) {
        LoadResult::Invalid(_) => return String::new(),
        LoadResult::Missing => crate::render::builtin_config(),
        LoadResult::Ok(c) => c,
    };
    lines(&rows(&p, &config, now, runner, cache))
}

/// Sample rows for `bar preview --subagent`: a running Explore agent, a
/// named reviewer and a finished task, started relative to `now`.
pub fn sample(now: i64) -> Payload {
    let ms = |ago: i64| Some(((now - ago) * 1000) as f64);
    let base = Task {
        kind: "local_agent".into(),
        status: "running".into(),
        model: "claude-sonnet-5".into(),
        context_size: Some(200_000),
        ..Task::default()
    };
    Payload {
        session_id: String::new(),
        cwd: String::new(),
        columns: None,
        tasks: vec![
            Task {
                id: "sample-1".into(),
                agent_type: "Explore".into(),
                description: "Find the config loader".into(),
                label: "Reading src/conf.rs".into(),
                start_ms: ms(95),
                tokens: Some(41_300),
                samples: vec![8_200, 19_000, 27_400, 35_900, 41_300],
                ..base.clone()
            },
            Task {
                id: "sample-2".into(),
                name: "reviewer".into(),
                agent_type: "code-reviewer".into(),
                description: "Review the install diff".into(),
                label: "Checking uninstall paths".into(),
                model: "claude-opus-5-5".into(),
                effort: "high".into(),
                start_ms: ms(7 * 60 + 12),
                tokens: Some(128_000),
                samples: vec![60_000, 88_000, 104_000, 121_000, 128_000],
                ..base.clone()
            },
            Task {
                id: "sample-3".into(),
                agent_type: "general-purpose".into(),
                status: "completed".into(),
                description: "Run the test suite".into(),
                label: "Run the test suite".into(),
                start_ms: ms(20 * 60),
                tokens: Some(18_700),
                ..base
            },
        ],
    }
}
