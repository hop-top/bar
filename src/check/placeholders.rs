//! Placeholder checks: layout, options, icons, GitHub and plugins.

use crate::budget;
use crate::expr;
use crate::format::{self, Token};
use crate::icons;
use crate::look;
use crate::options::Options;
use crate::placeholders::gh::{self, Kind};
use crate::placeholders::task;
use crate::plugin;

use super::options::val_ok;
use super::*;

impl Checker<'_> {
    pub(super) fn layout(&mut self, src: &str, line: usize) {
        match format::parse(src) {
            Err(e) => self.error(line, format!("invalid layout: {e}")),
            Ok(tokens) => {
                for t in tokens {
                    if let Token::Placeholder { name, options } = t {
                        self.placeholder(&name, &options, line, true);
                    }
                }
            }
        }
    }

    /// `used` is false for a `default` line: it only checks names and values.
    pub(super) fn placeholder(&mut self, name: &str, opts: &Options, line: usize, used: bool) {
        let in_row = task::lookup(name).is_some();
        if used && in_row && !self.row {
            self.error(line, format!("`{name}` works only in `set subagent`"));
            return;
        }
        if used && self.row && core_options(name).is_none() {
            let msg = format!("unknown placeholder `{name}` (`set subagent` draws no plugins)");
            self.error(line, msg);
            return;
        }
        match core_options(name) {
            Some(spec) => {
                let bad = self.core_values(name, spec, opts, line);
                if matches!(name, "prs" | "issues") {
                    self.gh_checks(name, opts, line, used, bad);
                }
                if name == "budget" && used && !bad {
                    self.budget_set(opts, line);
                }
                if used && !self.usage_api {
                    self.usage_api_off(name, opts, line);
                }
                if name == "jj" && used && self.jj_missing {
                    self.jj_missing = false;
                    let msg = "`jj` not on PATH: `jj` renders nothing".to_string();
                    self.add(Level::Warning, line, msg);
                }
            }
            None => self.plugin(name, opts, line, used),
        }
    }

    /// Reports bad options; true when there was any.
    fn core_values(
        &mut self,
        name: &str,
        spec: &[(&str, Val)],
        opts: &Options,
        line: usize,
    ) -> bool {
        let mut bad = false;
        for (k, v) in opts.iter() {
            if k == "icon" {
                bad |= self.icon(v, line);
                continue;
            }
            if look::KEYS.contains(&k.as_str()) {
                bad |= self.look(k, v, line);
                continue;
            }
            match spec.iter().find(|(n, _)| n == k) {
                None => {
                    bad = true;
                    self.error(line, format!("unknown option `{k}` for `{name}`"));
                }
                Some((_, kind)) if !v.contains('%') && !val_ok(*kind, v) => {
                    bad = true;
                    self.error(line, format!("invalid value `{v}` for `{k}`"));
                }
                _ => {}
            }
        }
        bad
    }

    /// Reports a bad `icon` value; true when there was one.
    fn icon(&mut self, v: &str, line: usize) -> bool {
        let bad = !v.contains('%') && !icons::valid(v, self.icon_width);
        if bad {
            let msg = format!(
                "invalid icon `{v}` (`none` or at most {} cells)",
                icons::MAX_CELLS
            );
            self.error(line, msg);
        }
        bad
    }

    /// Reports a bad `bg`, `fg`, `bold` or `gradient` value; true when
    /// there was one.
    fn look(&mut self, k: &str, v: &str, line: usize) -> bool {
        let msg = look::invalid(k, v).filter(|_| !v.contains('%'));
        let bad = msg.is_some();
        if let Some(m) = msg {
            self.error(line, m);
        }
        bad
    }

    /// Warns when `extra` or `usage[model=...]` is used while `set usage-api
    /// on` is not: they render nothing then.
    fn usage_api_off(&mut self, name: &str, opts: &Options, line: usize) {
        let merged = match self.defaults.get(name) {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let what = match name {
            "extra" => "`extra`",
            "usage" if merged.get("model").is_some_and(|m| !m.trim().is_empty()) => {
                "`usage[model=...]`"
            }
            _ => return,
        };
        let msg = format!("no `set usage-api on`: {what} renders nothing");
        self.add(Level::Warning, line, msg);
    }

    /// Warns when the budget a `budget` placeholder shows is not set.
    fn budget_set(&mut self, opts: &Options, line: usize) {
        let merged = match self.defaults.get("budget") {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let ty = merged.get("type").unwrap_or("session");
        if let Some(kind) = budget::Kind::parse(ty) {
            if self.budgets.get(kind).is_none() {
                let msg = format!("no `set budget-{ty}`: `budget[type={ty}]` renders nothing");
                self.add(Level::Warning, line, msg);
            }
        }
    }

    fn gh_checks(&mut self, name: &str, opts: &Options, line: usize, used: bool, bad: bool) {
        if !used {
            return;
        }
        if self.gh_missing && !self.gh_warned.replace(true) {
            self.add(
                Level::Warning,
                line,
                format!("`gh` not on PATH: `{name}` renders nothing"),
            );
        }
        if std::mem::take(&mut self.glab_missing) {
            self.add(
                Level::Warning,
                line,
                format!("`glab` not on PATH: `{name}` renders nothing on GitLab remotes"),
            );
        }
        let kind = if name == "prs" { Kind::Pr } else { Kind::Issue };
        let merged = match self.defaults.get(name) {
            Some(d) => opts.merged_over(d),
            None => opts.clone(),
        };
        let dynamic = merged.iter().any(|(_, v)| v.contains('%'));
        let mut expr_bad = false;
        for key in ["author", "assignee", "label"] {
            if let Some(v) = opts.get(key).filter(|v| !v.contains('%')) {
                if let Err(e) = expr::parse(v) {
                    expr_bad = true;
                    self.error(line, format!("invalid expression for `{key}`: {e}"));
                }
            }
        }
        if bad || expr_bad || dynamic {
            return;
        }
        match gh::plan(kind, "https://github.com/o/r", &merged) {
            Some(plan) => {
                if let Some(w) = plan.warning {
                    self.add(Level::Warning, line, w);
                }
            }
            None => self.error(line, format!("`{name}` options do not compile to queries")),
        }
    }

    fn plugin(&mut self, name: &str, opts: &Options, line: usize, used: bool) {
        let Some(found) = plugin::find(self.dirs, name) else {
            if used {
                self.error(line, format!("unknown placeholder `{name}`"));
            } else {
                // a `default` line may be shared by machines without the plugin
                let msg = format!("plugin `{name}` not on PATH");
                self.add(Level::Warning, line, msg);
            }
            return;
        };
        let info = plugin::info(&found);
        if !info.is_placeholder() {
            self.error(
                line,
                format!("plugin `{name}` does not declare the placeholder capability"),
            );
            return;
        }
        for (declared, opt) in [
            (info.declares_icon_option, "icon"),
            (info.declares_bg_option, "bg"),
            (info.declares_click_option, "click"),
        ] {
            if declared {
                let msg = format!("plugin `{name}` declares the reserved option `{opt}`; ignored");
                self.add(Level::Warning, line, msg);
            }
        }
        if let Some(opt) = info.declares_look_option {
            let msg = format!("plugin `{name}` declares the reserved option `{opt}`; ignored");
            self.add(Level::Warning, line, msg);
        }
        for n in &info.inputs.refused {
            let msg = format!(
                "plugin `{name}` declares the input `{n}`, which looks like a credential; refused"
            );
            self.add(Level::Warning, line, msg);
        }
        if used
            && info
                .capabilities
                .iter()
                .any(|c| c == plugin::check::CHECK_CAPABILITY)
            && self.checked.insert(name.to_string())
        {
            for w in plugin::check::run(&found) {
                self.add(Level::Warning, line, format!("plugin `{name}`: {w}"));
            }
        }
        for issue in plugin::web::issues(&info) {
            self.add(
                Level::Warning,
                line,
                format!("plugin `{name}`: web: {issue}"),
            );
        }
        let mut concrete = Options::default();
        for (k, v) in opts.iter().filter(|(_, v)| !v.contains('%')) {
            if k == "icon" {
                self.icon(v, line);
            } else if look::KEYS.contains(&k.as_str()) {
                self.look(k, v, line);
            } else if k == "click" {
                if !val_ok(Val::OneOf(&["toggle", "none"]), v) {
                    self.error(line, format!("invalid value `{v}` for `click`"));
                }
            } else {
                concrete.set(k, v);
            }
        }
        if let Err(e) = plugin::validate(&info.options, &concrete) {
            self.error(line, format!("{e} for `{name}`"));
        }
    }
}
