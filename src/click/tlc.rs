//! Per-session tlc task record kept by `bar click hook`: the task a session
//! last claimed, set in progress (`update --status IN_PROGRESS`), completed or
//! created through the Bash tool, and the directory tlc ran in. Stored as
//! JSON `{"task","cwd","at"}` in `<state>/records/tlc/<session id>`, which
//! bar hands to the `tlc` plugin as `session.record`.

use std::path::{Path, PathBuf};

/// One tlc task action found in a shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touch {
    pub task: String,
    /// Where tlc ran: the hook's cwd, moved by `cd` and `-C`/`--chdir`.
    pub cwd: PathBuf,
}

/// Task ids and aliases (`T-0042`, `task_01…`); patterns and URIs are not
/// recorded.
pub(crate) fn valid_task(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Shell words grouped into simple commands: `&&`, `||`, `;`, `|`, `&`,
/// newlines and parentheses separate; quotes and backslashes are honoured,
/// `#` starts a comment. Substitutions and expansions stay literal.
pub fn commands(src: &str) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new()];
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = src.chars().peekable();
    let end_word = |out: &mut Vec<Vec<String>>, word: &mut String, in_word: &mut bool| {
        if *in_word {
            out.last_mut().unwrap().push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            word.push(chars.next().unwrap());
                        }
                        _ => word.push(q),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') | None => {}
                Some(n) => {
                    in_word = true;
                    word.push(n);
                }
            },
            '#' if !in_word => {
                for q in chars.by_ref() {
                    if q == '\n' {
                        break;
                    }
                }
                end_word(&mut out, &mut word, &mut in_word);
                out.push(Vec::new());
            }
            '&' | '|' | ';' | '\n' | '(' | ')' => {
                end_word(&mut out, &mut word, &mut in_word);
                if out.last().is_some_and(|c| !c.is_empty()) {
                    out.push(Vec::new());
                }
            }
            c if c.is_whitespace() => end_word(&mut out, &mut word, &mut in_word),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut out, &mut word, &mut in_word);
    out.retain(|c| !c.is_empty());
    out
}

fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// `cd <dir>` moves on from `dir`; a target that cannot be resolved here
/// (no argument, `-`, `~`, `$…`) makes the directory unknown.
fn cd(dir: Option<PathBuf>, arg: Option<&String>) -> Option<PathBuf> {
    let a = arg?;
    if a.is_empty() || a.starts_with(['-', '~', '$', '`']) {
        return None;
    }
    Some(dir?.join(a))
}

/// Task ids that `Created task <id>` lines announce, in order.
fn created(output: &str) -> impl Iterator<Item = String> + '_ {
    output.lines().filter_map(|l| {
        let id = l.trim_start().strip_prefix("Created task ")?;
        let id: String = id
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        valid_task(&id).then_some(id)
    })
}

/// Flags of `tlc task <verb>` that take no value.
fn bare_flag(f: &str) -> bool {
    f.contains('=') || matches!(f, "-h" | "--help" | "--no-prompt" | "--force" | "-f")
}

/// The first positional word after the verb.
fn first_arg(rest: &[String]) -> Option<&str> {
    let mut i = 0;
    while let Some(w) = rest.get(i) {
        if !w.starts_with('-') {
            return Some(w);
        }
        i += if bare_flag(w) { 1 } else { 2 };
    }
    None
}

fn sets_in_progress(rest: &[String]) -> bool {
    let wip = |v: &str| v.eq_ignore_ascii_case("IN_PROGRESS");
    rest.iter().enumerate().any(|(i, w)| {
        w.strip_prefix("--status=").is_some_and(wip)
            || (w == "--status" && rest.get(i + 1).is_some_and(|v| wip(v)))
    })
}

/// The last tlc task action in `command` (run in `cwd`), whose output is
/// `output`; `create` takes its id from the `Created task <id>` lines.
pub fn last_touch(command: &str, cwd: &Path, output: &str) -> Option<Touch> {
    let mut dir = Some(cwd.to_path_buf());
    let mut ids = created(output);
    let mut last = None;
    for words in commands(command) {
        let mut w = words.iter().skip_while(|w| is_assignment(w));
        let Some(prog) = w.next() else { continue };
        let rest: Vec<String> = w.cloned().collect();
        if prog == "cd" {
            dir = cd(dir, rest.first());
            continue;
        }
        if Path::new(prog).file_name().and_then(|n| n.to_str()) != Some("tlc") {
            continue;
        }
        // global flags before `task`: only -C/--chdir moves tlc
        let mut at = dir.clone();
        let mut i = 0;
        while let Some(t) = rest.get(i).filter(|t| t.starts_with('-')) {
            let target = match t.as_str() {
                "-C" | "--chdir" => {
                    i += 1;
                    rest.get(i).cloned()
                }
                _ => t.strip_prefix("--chdir=").map(String::from),
            };
            if let Some(d) = target {
                at = at.map(|a| a.join(d));
            }
            i += 1;
        }
        if rest.get(i).map(String::as_str) != Some("task") {
            continue;
        }
        let verb = rest.get(i + 1).map_or("", String::as_str);
        let args = rest.get(i + 2..).unwrap_or_default();
        let task = match verb {
            "create" => ids.next(),
            "claim" | "complete" => first_arg(args).map(String::from),
            "update" if sets_in_progress(args) => first_arg(args).map(String::from),
            _ => None,
        };
        if let (Some(task), Some(cwd)) = (task.filter(|t| valid_task(t)), at) {
            last = Some(Touch { task, cwd });
        }
    }
    last
}
