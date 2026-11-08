//! Value expansion: `%$VAR%` environment, `%command%` output. `@me` is
//! left for the provider to resolve.

use std::cell::RefCell;
use std::collections::HashMap;

pub trait CommandRunner {
    /// Trimmed-or-raw stdout; `None` when the command fails.
    fn run(&self, cmd: &str) -> Option<String>;
}

pub struct Expander<'a> {
    runner: &'a dyn CommandRunner,
    env: &'a dyn Fn(&str) -> Option<String>,
    memo: RefCell<HashMap<String, String>>,
}

fn process_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

impl<'a> Expander<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self::with_env(runner, &process_env)
    }

    pub fn with_env(
        runner: &'a dyn CommandRunner,
        env: &'a dyn Fn(&str) -> Option<String>,
    ) -> Self {
        Expander {
            runner,
            env,
            memo: RefCell::new(HashMap::new()),
        }
    }

    pub fn expand(&self, value: &str) -> String {
        let mut out = String::new();
        let mut rest = value;
        while let Some(i) = rest.find('%') {
            let after = &rest[i + 1..];
            match after.find('%') {
                Some(j) if j > 0 => {
                    out.push_str(&rest[..i]);
                    out.push_str(&self.resolve(&after[..j]));
                    rest = &after[j + 1..];
                }
                _ => {
                    // lone or empty `%%`: literal
                    out.push_str(&rest[..=i]);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn resolve(&self, inner: &str) -> String {
        if let Some(var) = inner.strip_prefix('$') {
            return (self.env)(var).unwrap_or_default();
        }
        if let Some(hit) = self.memo.borrow().get(inner) {
            return hit.clone();
        }
        let val = self
            .runner
            .run(inner)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        self.memo
            .borrow_mut()
            .insert(inner.to_string(), val.clone());
        val
    }
}

pub fn expand(value: &str, runner: &dyn CommandRunner) -> String {
    Expander::new(runner).expand(value)
}
