//! Work a background refresh performs.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cache::{run_child, run_command, COMMAND_TIMEOUT};
use crate::plugin;

/// Work a refresh performs. Serialised into the refresh process's
/// arguments, so it carries only kinds and options, never secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `sh -c cmd`; stdout on success. `timeout` in seconds.
    Command { cmd: String, timeout: u64 },
    /// `gh <args>` run directly, no shell; stdout on success.
    Gh { args: Vec<String> },
    /// `git status --porcelain=v2 --branch` in `dir` (absolute), no shell.
    GitStatus { dir: String },
    /// `<path> --ext-info`, no shell; stdout when it parses as plugin info.
    PluginInfo { path: String },
    /// `<path>` with `input` on stdin, no shell; stdout when it parses as
    /// plugin output.
    PluginRender { path: String, input: String },
    /// Finish scanning a transcript for token totals; progress is kept in
    /// the transcript state file.
    Transcript { path: String, state: String },
}

impl Job {
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Job::Command { cmd, timeout } => {
                json!({"kind": "command", "cmd": cmd, "timeout": timeout})
            }
            Job::Gh { args } => json!({"kind": "gh", "args": args}),
            Job::GitStatus { dir } => json!({"kind": "git-status", "dir": dir}),
            Job::PluginInfo { path } => json!({"kind": "plugin-info", "path": path}),
            Job::PluginRender { path, input } => {
                json!({"kind": "plugin-render", "path": path, "input": input})
            }
            Job::Transcript { path, state } => {
                json!({"kind": "transcript", "path": path, "state": state})
            }
        }
    }

    pub(crate) fn from_value(v: &Value) -> Result<Job, String> {
        match v["kind"].as_str() {
            Some("command") => Ok(Job::Command {
                cmd: v["cmd"].as_str().ok_or("command job without cmd")?.into(),
                timeout: v["timeout"].as_u64().unwrap_or(COMMAND_TIMEOUT),
            }),
            Some("gh") => Ok(Job::Gh {
                args: v["args"]
                    .as_array()
                    .ok_or("gh job without args")?
                    .iter()
                    .map(|a| a.as_str().map(String::from).ok_or("gh arg not a string"))
                    .collect::<Result<_, _>>()?,
            }),
            Some("git-status") => Ok(Job::GitStatus {
                dir: v["dir"]
                    .as_str()
                    .filter(|d| Path::new(d).is_absolute())
                    .ok_or("git-status job without absolute dir")?
                    .into(),
            }),
            Some("plugin-info") => Ok(Job::PluginInfo {
                path: plugin_path(v)?,
            }),
            Some("plugin-render") => Ok(Job::PluginRender {
                path: plugin_path(v)?,
                input: v["input"]
                    .as_str()
                    .ok_or("plugin job without input")?
                    .into(),
            }),
            Some("transcript") => Ok(Job::Transcript {
                path: plugin_path(v)?,
                state: v["state"]
                    .as_str()
                    .filter(|p| Path::new(p).is_absolute())
                    .ok_or("transcript job without absolute state dir")?
                    .into(),
            }),
            other => Err(format!("unknown job kind {other:?}")),
        }
    }

    /// `None` on failure or timeout; `limit` caps the job's own timeout.
    pub fn execute(&self, limit: Duration) -> Option<String> {
        match self {
            Job::Command { cmd, timeout } => {
                run_command(cmd, limit.min(Duration::from_secs(*timeout)))
            }
            Job::Gh { args } => {
                let mut c = Command::new("gh");
                c.args(args);
                run_child(c, None, limit)
            }
            Job::GitStatus { dir } => {
                let mut c = Command::new("git");
                c.args(["-c", "core.fsmonitor=false"])
                    .args(["status", "--porcelain=v2", "--branch"])
                    .current_dir(dir)
                    .env("GIT_OPTIONAL_LOCKS", "0");
                run_child(c, None, limit)
            }
            Job::PluginInfo { path } => {
                let mut c = Command::new(path);
                c.arg("--ext-info");
                run_child(c, None, limit.min(plugin::INFO_TIMEOUT))
                    .filter(|o| plugin::parse_info(o).is_some())
            }
            Job::PluginRender { path, input } => run_child(
                Command::new(path),
                Some(input.clone().into_bytes()),
                limit.min(plugin::RENDER_TIMEOUT),
            )
            .filter(|o| plugin::parse_output(o).is_some()),
            Job::Transcript { path, state } => crate::transcript::update(
                Path::new(state),
                Path::new(path),
                u64::MAX,
                Some(Instant::now() + limit),
            )
            .map(|_| "ok".to_string()),
        }
    }
}

/// Plugins run by absolute path only, never looked up on PATH.
fn plugin_path(v: &Value) -> Result<String, String> {
    v["path"]
        .as_str()
        .filter(|p| Path::new(p).is_absolute())
        .map(String::from)
        .ok_or_else(|| "plugin job without absolute path".to_string())
}
