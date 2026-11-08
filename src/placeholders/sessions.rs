use super::{Ctx, Placeholder};
use crate::click::{self, hook};
use crate::options::Options;
use crate::style::{bold, fg};
use serde_json::Value;
use std::path::Path;

/// `>2 ?1 v3`: other live sessions working, waiting on you, done.
/// Liveness and busy come from Claude Code's session files; waiting and
/// done from the markers `bar click hook` writes.
pub struct Sessions;

impl Placeholder for Sessions {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts, &click::state_dir(), &click::claude_dir()?)
    }
}

struct Entry {
    pid: u64,
    sid: String,
    busy: bool,
}

fn entries(claude: &Path) -> Vec<Entry> {
    let Ok(dir) = std::fs::read_dir(claude.join("sessions")) else {
        return Vec::new();
    };
    dir.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let pid = match v.get("pid")? {
                Value::Number(n) => n.as_u64()?,
                Value::String(s) if s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok()?,
                _ => return None,
            };
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            Some(Entry {
                pid,
                sid: s("sessionId"),
                busy: s("status") == "busy",
            })
        })
        .collect()
}

/// Probes the process without signalling it or spawning anything.
fn alive(pid: u64) -> bool {
    crate::platform::pid_alive(pid)
}

pub fn render_with(ctx: &Ctx, _opts: &Options, state: &Path, claude: &Path) -> Option<String> {
    let all = entries(claude);
    let (mut busy, mut waiting, mut done) = (0, 0, 0);
    for e in all.iter().filter(|e| alive(e.pid)) {
        let mark = hook::mark(state, &e.sid);
        match mark.as_deref() {
            Some("waiting") => waiting += 1,
            _ if e.busy => busy += 1,
            Some("done") => done += 1,
            _ => {}
        }
    }
    let c = &ctx.config.colors;
    let mut parts = Vec::new();
    if busy > 0 {
        parts.push(fg(c.warn, &format!(">{busy}")));
    }
    if waiting > 0 {
        parts.push(bold(&fg(c.bad, &format!("?{waiting}"))));
    }
    if done > 0 {
        parts.push(fg(c.good, &format!("v{done}")));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}
