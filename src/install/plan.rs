//! The change `bar install` (or `--uninstall`) makes to a settings file,
//! worked out before anything is written.

use super::{record_path, render_command, subagent_command, Env, Opts};
use crate::click::settings::{self, add_hooks, command_for, load, remove_hooks};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The settings keys install writes: the key, the `bar` arguments its
/// command runs, and what it is called in messages.
struct Key {
    name: &'static str,
    args: &'static str,
    what: &'static str,
    flag: &'static str,
}

const STATUS: Key = Key {
    name: "statusLine",
    args: "render",
    what: "status line",
    flag: "",
};

const SUBAGENT: Key = Key {
    name: "subagentStatusLine",
    args: "subagent",
    what: "subagent status line",
    flag: " --subagent",
};

fn runs_bar(v: Option<&Value>, key: &Key) -> bool {
    v.and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| settings::runs_bar(c, key.args))
}

/// The record's settings values, by key; empty when there is none.
fn read_record(path: &Path) -> serde_json::Map<String, Value> {
    let v: Option<Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let mut m = match v {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    m.retain(|k, _| k == STATUS.name || k == SUBAGENT.name);
    m
}

/// What to do with the record of the replaced status lines.
pub(super) enum Record {
    Keep,
    /// Write these keys (`statusLine`, `subagentStatusLine`).
    Write(serde_json::Map<String, Value>),
    Remove,
}

pub(super) struct Plan {
    pub(super) path: PathBuf,
    pub(super) text: Option<String>,
    pub(super) after: Value,
    pub(super) record: Record,
    pub(super) notes: Vec<String>,
}

impl Plan {
    pub(super) fn changed(&self) -> bool {
        let before: Value = self
            .text
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_else(|| json!({}));
        before != self.after
    }

    /// Whether writing also changes the file's layout (one-line JSON,
    /// arrays kept on one line, ...), which the diff then leaves out.
    pub(super) fn reformats(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|t| !settings::same_layout(t))
    }

    /// The diff of the file as written; for a file whose layout changes,
    /// of its content in the written layout, so only real changes show.
    pub(super) fn diff(&self) -> String {
        let shown = self.path.display().to_string();
        let old_name = if self.text.is_some() {
            &shown
        } else {
            "/dev/null"
        };
        let orig = self.text.as_deref();
        let before = match orig {
            Some(t) if self.reformats() => serde_json::from_str::<Value>(t)
                .map(|v| settings::render_like(&v, orig))
                .unwrap_or_else(|_| t.to_string()),
            Some(t) => t.to_string(),
            None => String::new(),
        };
        crate::diff::unified(
            &before,
            &settings::render_like(&self.after, orig),
            old_name,
            &shown,
        )
    }
}

/// Sets `key` in `obj` to run `bar <key.args>` (other fields of the
/// current value kept); a current value that does not run bar goes into
/// `kept`.
fn set_line(
    obj: &mut serde_json::Map<String, Value>,
    key: &Key,
    command: String,
    kept: &mut serde_json::Map<String, Value>,
) {
    let cur = obj.get(key.name).cloned();
    if let Some(c) = cur.as_ref().filter(|c| !runs_bar(Some(c), key)) {
        kept.insert(key.name.into(), c.clone());
    }
    let mut line = match cur {
        Some(Value::Object(m)) => Value::Object(m),
        _ => json!({}),
    };
    line["type"] = json!("command");
    line["command"] = json!(command);
    obj.insert(key.name.into(), line);
}

/// A binary run through `npx` (or `npm exec`): under a `_npx` directory
/// of npm's cache.
fn in_npx_cache(bar: &Path) -> bool {
    bar.components().any(|c| c.as_os_str() == "_npx")
}

pub(super) fn plan_install(env: &Env, o: &Opts) -> Result<Plan, String> {
    let l = load(&env.settings)?;
    let shown = l.path.display().to_string();
    let rec = record_path(&env.state_dir, &l.path);
    let mut root = l.root.clone();
    let obj = root.as_object_mut().ok_or("not a JSON object")?;
    let bar = env.bar.display().to_string();
    let mut kept = serde_json::Map::new();
    if !o.subagent_only {
        set_line(obj, &STATUS, render_command(&bar, env.windows), &mut kept);
        if let Some(n) = o.refresh {
            obj[STATUS.name]["refreshInterval"] = json!(n);
        }
    }
    if o.subagent || o.subagent_only {
        set_line(
            obj,
            &SUBAGENT,
            subagent_command(&bar, env.windows),
            &mut kept,
        );
    }
    let mut notes = Vec::new();
    if in_npx_cache(&env.bar) {
        notes.push(format!(
            "warning: {bar} is in the npx cache, which npm may clear; \
             install it with npm install -g @hop-top/bar and run bar install again"
        ));
    }
    for key in [&STATUS, &SUBAGENT] {
        if kept.contains_key(key.name) {
            notes.push(format!(
                "the current {} is kept in {}; bar install --uninstall{} puts it back",
                key.what,
                rec.display(),
                key.flag
            ));
        }
    }
    let record = if kept.is_empty() {
        Record::Keep
    } else {
        let mut all = read_record(&rec);
        all.extend(kept);
        Record::Write(all)
    };
    if o.hooks {
        add_hooks(
            &mut root,
            &command_for(&bar, "click hook", env.windows),
            &shown,
        )?;
    }
    Ok(Plan {
        path: l.path,
        text: l.text,
        after: root,
        record,
        notes,
    })
}

pub(super) fn plan_uninstall(env: &Env, o: &Opts) -> Result<Plan, String> {
    let l = load(&env.settings)?;
    let rec = record_path(&env.state_dir, &l.path);
    let mut root = l.root.clone();
    let obj = root.as_object_mut().ok_or("not a JSON object")?;
    let mut notes = Vec::new();
    let mut saved = read_record(&rec);
    let mut touched = false;
    let keys = [
        (&STATUS, !o.subagent_only, !o.subagent_only),
        (&SUBAGENT, o.subagent || o.subagent_only, true),
    ];
    for (key, on, remind) in keys {
        let bars = runs_bar(obj.get(key.name), key);
        if !on {
            if bars && remind {
                notes.push(format!(
                    "{} still runs bar; bar install --uninstall{} removes it",
                    key.name, key.flag
                ));
            }
            continue;
        }
        if bars {
            touched = true;
            match saved.remove(key.name) {
                Some(prev) => {
                    obj.insert(key.name.into(), prev);
                    notes.push(format!("restores the {} bar replaced", key.what));
                }
                None => {
                    obj.shift_remove(key.name);
                }
            }
        } else if obj.contains_key(key.name) {
            notes.push(format!("{} does not run bar; left as is", key.name));
        }
    }
    let record = match (touched, saved.is_empty()) {
        (false, _) => Record::Keep,
        (true, true) => Record::Remove,
        (true, false) => Record::Write(saved),
    };
    if o.hooks {
        remove_hooks(&mut root);
    }
    Ok(Plan {
        path: l.path,
        text: l.text,
        after: root,
        record,
        notes,
    })
}
