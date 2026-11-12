//! `rule` and `account` lines that never apply: both are first-match, so
//! a line an earlier line of the same file always matches before is dead.

use crate::placeholders::account::glob_covers;

use super::*;

/// The earlier `rule` and `account` lines of one file that apply.
#[derive(Default)]
pub(super) struct FirstMatch {
    /// Target of each valid `rule` line, with the first line holding it.
    rules: BTreeMap<String, usize>,
    /// Glob of each valid `account` line that can win a match, in order.
    accounts: Vec<(String, usize)>,
}

impl Checker<'_> {
    /// A `rule <target>` line after a valid one for the same target, or an
    /// `account "<glob>"` line whose glob an earlier line's covers: the
    /// earlier line matches everything the later one would, first. A
    /// project file's line over the user's is layering: each file has its
    /// own [`FirstMatch`].
    pub(super) fn dead(&mut self, seen: &mut FirstMatch, t: &[String], raw: &str, line: usize) {
        let head = t.first().map(String::as_str);
        if !matches!(head, Some("rule" | "account")) {
            return;
        }
        // a line that does not parse applies nowhere: an error already
        let Ok(cfg) = crate::conf::parse(raw) else {
            return;
        };
        if let Some(r) = cfg.rules.first() {
            let target = &r.target;
            if let Some(&prev) = seen.rules.get(target) {
                let msg = format!(
                    "`rule {target}` never applies: the one on line {prev} matches first (only the first `rule {target}` line applies); remove this line, or keep the one you want on line {prev}"
                );
                self.add(Level::Warning, line, msg);
            } else {
                seen.rules.insert(target.clone(), line);
            }
        }
        if let Some(a) = cfg.accounts.first() {
            let glob = &a.glob;
            if let Some((wide, prev)) = seen.accounts.iter().find(|(g, _)| glob_covers(g, glob)) {
                let (how, fix) = if wide == glob {
                    (
                        format!("the one on line {prev} matches first"),
                        format!("remove this line, or keep the one you want on line {prev}"),
                    )
                } else {
                    (
                        format!(
                            "`account \"{wide}\"` on line {prev} matches everything it does, first"
                        ),
                        format!("move it above line {prev} if it is meant to win, or remove it"),
                    )
                };
                let msg = format!(
                    "`account \"{glob}\"` never applies: {how} (the first matching `account` line applies); {fix}"
                );
                self.add(Level::Warning, line, msg);
            } else if !bar_plugin::proto::sanitize(&a.label).trim().is_empty() {
                // a plugin matching the lines skips one with nothing to show
                seen.accounts.push((glob.clone(), line));
            }
        }
    }
}
