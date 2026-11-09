//! Work a background refresh performs.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cache::{run_child, run_command, COMMAND_TIMEOUT};
use crate::plugin;
use crate::tz::Tz;

/// Work a refresh performs. Serialised into the refresh process's
/// arguments, so it carries only kinds and options, never secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `sh -c cmd` (`cmd /C` on Windows); stdout on success. `timeout` in seconds.
    Command { cmd: String, timeout: u64 },
    /// `gh <args>` run directly, no shell; stdout on success.
    Gh { args: Vec<String> },
    /// `glab <args>` run directly, no shell; its JSON reduced to the
    /// fields `gh` gives. `kind` `mr` also runs `glab mr view` per listed
    /// MR in `repo` (an `https://` URL) for its pipeline.
    Glab {
        kind: String,
        repo: String,
        args: Vec<String>,
    },
    /// `git status --porcelain=v2 --branch --show-stash` in `dir`
    /// (absolute), no shell.
    GitStatus { dir: String },
    /// `git for-each-ref --points-at=<commit> refs/tags` in `dir`
    /// (absolute), no shell; `commit` is a full object id. Output: the
    /// commit, then one `<objecttype> <tag>` line per tag.
    GitTags { dir: String, commit: String },
    /// `jj log` of the working-copy commit of the workspace at `dir`
    /// (absolute), no shell, never snapshotting the working copy.
    Jj { dir: String },
    /// `<path> --ext-info`, no shell; stdout when it parses as plugin info.
    PluginInfo { path: String },
    /// `<path>` with `input` on stdin, no shell; stdout when it parses as
    /// plugin output.
    PluginRender { path: String, input: String },
    /// `tmux display-message -p '#S:#I' -t <pane>`, no shell; `pane` is
    /// `%` and digits.
    Tmux { pane: String },
    /// Finish scanning a transcript for token totals; progress is kept in
    /// the transcript state file.
    Transcript { path: String, state: String },
    /// Scan all transcripts under `dir` for daily spend totals; `utc`
    /// buckets by UTC date, else local.
    SpendHist {
        dir: String,
        state: String,
        utc: bool,
    },
    /// `bar prices update`: fetch models.dev into the data-dir snapshot.
    Prices,
    /// Newest bar release tag of a channel from GitHub (`set update-check on`).
    Update { prerelease: bool },
    /// Claude service status from status.claude.com (`set service-status on`);
    /// `strip` also reads the incident history.
    Status { strip: bool },
    /// Per-model weekly windows and extra usage from the OAuth usage
    /// endpoint (`set usage-api on`). `dir` is Claude Code's config dir,
    /// `hashed` whether `CLAUDE_CONFIG_DIR` names it (the keychain entry
    /// name depends on it), `prev` the cache file of the last result.
    /// The token is read by the job itself, never passed in.
    UsageApi {
        dir: String,
        hashed: bool,
        prev: String,
    },
}

impl Job {
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Job::Command { cmd, timeout } => {
                json!({"kind": "command", "cmd": cmd, "timeout": timeout})
            }
            Job::Gh { args } => json!({"kind": "gh", "args": args}),
            Job::Glab { kind, repo, args } => {
                json!({"kind": "glab", "glab": kind, "repo": repo, "args": args})
            }
            Job::GitStatus { dir } => json!({"kind": "git-status", "dir": dir}),
            Job::GitTags { dir, commit } => {
                json!({"kind": "git-tags", "dir": dir, "commit": commit})
            }
            Job::Jj { dir } => json!({"kind": "jj", "dir": dir}),
            Job::PluginInfo { path } => json!({"kind": "plugin-info", "path": path}),
            Job::PluginRender { path, input } => {
                json!({"kind": "plugin-render", "path": path, "input": input})
            }
            Job::Tmux { pane } => json!({"kind": "tmux", "pane": pane}),
            Job::Transcript { path, state } => {
                json!({"kind": "transcript", "path": path, "state": state})
            }
            Job::SpendHist { dir, state, utc } => {
                json!({"kind": "spendhist", "dir": dir, "state": state, "utc": utc})
            }
            Job::Prices => json!({"kind": "prices"}),
            Job::Update { prerelease } => json!({"kind": "update", "prerelease": prerelease}),
            Job::Status { strip } => json!({"kind": "status", "strip": strip}),
            Job::UsageApi { dir, hashed, prev } => {
                json!({"kind": "usage-api", "dir": dir, "hashed": hashed, "prev": prev})
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
                args: str_args(v, "gh")?,
            }),
            Some("glab") => Ok(Job::Glab {
                kind: v["glab"]
                    .as_str()
                    .filter(|k| matches!(*k, "mr" | "issue"))
                    .ok_or("glab job without mr or issue")?
                    .into(),
                repo: v["repo"]
                    .as_str()
                    .filter(|r| r.starts_with("https://"))
                    .ok_or("glab job without https repo")?
                    .into(),
                args: str_args(v, "glab")?,
            }),
            Some("git-status") => Ok(Job::GitStatus {
                dir: abs_path(v, "dir")?,
            }),
            Some("git-tags") => Ok(Job::GitTags {
                dir: abs_path(v, "dir")?,
                commit: v["commit"]
                    .as_str()
                    .filter(|c| crate::gitfs::is_oid(c))
                    .ok_or("git-tags job without a commit id")?
                    .into(),
            }),
            Some("jj") => Ok(Job::Jj {
                dir: abs_path(v, "dir")?,
            }),
            Some("plugin-info") => Ok(Job::PluginInfo {
                path: abs_path(v, "path")?,
            }),
            Some("plugin-render") => Ok(Job::PluginRender {
                path: abs_path(v, "path")?,
                input: v["input"]
                    .as_str()
                    .ok_or("plugin job without input")?
                    .into(),
            }),
            Some("tmux") => Ok(Job::Tmux {
                pane: v["pane"]
                    .as_str()
                    .filter(|p| crate::placeholders::tmux::valid_pane(p))
                    .ok_or("tmux job without a valid pane")?
                    .into(),
            }),
            Some("transcript") => Ok(Job::Transcript {
                path: abs_path(v, "path")?,
                state: abs_path(v, "state")?,
            }),
            Some("spendhist") => Ok(Job::SpendHist {
                dir: abs_path(v, "dir")?,
                state: abs_path(v, "state")?,
                utc: v["utc"].as_bool().unwrap_or(false),
            }),
            Some("prices") => Ok(Job::Prices),
            Some("update") => Ok(Job::Update {
                prerelease: v["prerelease"].as_bool().unwrap_or(false),
            }),
            Some("status") => Ok(Job::Status {
                strip: v["strip"].as_bool().unwrap_or(false),
            }),
            Some("usage-api") => Ok(Job::UsageApi {
                dir: abs_path(v, "dir")?,
                hashed: v["hashed"].as_bool().unwrap_or(false),
                prev: abs_path(v, "prev")?,
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
                let mut c = Command::new(crate::platform::program("gh"));
                c.args(args);
                run_child(c, None, limit)
            }
            Job::Glab { kind, repo, args } => {
                crate::placeholders::glab::run(kind, repo, args, limit)
            }
            Job::GitStatus { dir } => {
                let mut c = Command::new(crate::platform::program("git"));
                c.args(["-c", "core.fsmonitor=false"])
                    .args(["status", "--porcelain=v2", "--branch", "--show-stash"])
                    .current_dir(dir)
                    .env("GIT_OPTIONAL_LOCKS", "0");
                run_child(c, None, limit)
            }
            Job::GitTags { dir, commit } => {
                let mut c = Command::new(crate::platform::program("git"));
                c.arg("for-each-ref")
                    .arg(format!("--points-at={commit}"))
                    .arg("--format=%(objecttype) %(refname:strip=2)")
                    .arg("refs/tags")
                    .current_dir(dir)
                    .env("GIT_OPTIONAL_LOCKS", "0");
                run_child(c, None, limit).map(|out| format!("{commit}\n{out}"))
            }
            Job::Jj { dir } => {
                let mut c = Command::new(crate::platform::program("jj"));
                c.args(crate::placeholders::jj::args(dir)).current_dir(dir);
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
            Job::Tmux { pane } => {
                let mut c = Command::new(crate::platform::program("tmux"));
                c.args(["display-message", "-p", "#S:#I", "-t", pane]);
                run_child(c, None, limit.min(Duration::from_secs(2)))
                    .filter(|o| !o.trim().is_empty())
            }
            Job::Transcript { path, state } => crate::transcript::update(
                Path::new(state),
                Path::new(path),
                u64::MAX,
                Some(Instant::now() + limit),
            )
            .map(|_| "ok".to_string()),
            Job::SpendHist { dir, state, utc } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                crate::spendhist::update(
                    Path::new(state),
                    Path::new(dir),
                    if *utc { Tz::Utc } else { Tz::Local },
                    now,
                    Some(Instant::now() + limit),
                )
            }
            Job::Prices => crate::prices::update::run_within(None, limit.as_secs())
                .ok()
                .map(|(_, n)| n.to_string()),
            Job::Update { prerelease } => Some(crate::update::check(*prerelease, limit)),
            Job::Status { strip } => crate::status::fetch(*strip, limit),
            Job::UsageApi { dir, hashed, prev } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                crate::usageapi::fetch(Path::new(dir), *hashed, Path::new(prev), now, limit)
            }
        }
    }
}

/// String array `args` of a `gh` or `glab` job.
fn str_args(v: &Value, tool: &str) -> Result<Vec<String>, String> {
    v["args"]
        .as_array()
        .ok_or_else(|| format!("{tool} job without args"))?
        .iter()
        .map(|a| {
            a.as_str()
                .map(String::from)
                .ok_or_else(|| format!("{tool} arg not a string"))
        })
        .collect()
}

/// Plugins run by absolute path only, never looked up on PATH.
/// Field `key` of job `v`, required to be an absolute path.
fn abs_path(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .filter(|p| Path::new(p).is_absolute())
        .map(String::from)
        .ok_or_else(|| {
            let kind = v["kind"].as_str().unwrap_or("job");
            format!("{kind} job without absolute {key}")
        })
}
