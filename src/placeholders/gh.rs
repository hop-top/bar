//! `prs` and `issues`: `gh` (GitHub) or `glab` (GitLab, see `glab`) lists
//! cached per query, merged at render time. `gh` output is reduced in the
//! background job to what rendering reads (see [`run`]).

use super::repo::cached_remote;
use super::{link_enabled, Ctx, Placeholder};
use crate::cache::{run_child_capped, Cache, Job};
use crate::color::Color;
use crate::expr;
use crate::icons;
use crate::options::Options;
use crate::style::{dim, fg, link, sanitize};
use serde_json::{json, Map, Value};
use std::process::Command;
use std::time::Duration;

/// Lists stay fresh for two minutes.
pub const TTL: u64 = 120;
/// Queries per placeholder; more are dropped.
pub const MAX_QUERIES: usize = 8;
pub(crate) const DEFAULT_LIMIT: usize = 5;
const PR_FIELDS: &str = "number,url,reviewDecision,mergeable,statusCheckRollup";
const ISSUE_FIELDS: &str = "number,url,labels";
/// Most bytes of `gh` output read: a PR's `statusCheckRollup` lists every
/// check run, a few hundred bytes each; reduced to under 200 per item.
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

pub struct Prs;
pub struct Issues;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pr,
    Issue,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Pr => "pr",
            Kind::Issue => "issue",
        }
    }

    /// Option keys compiled into search terms.
    pub(crate) fn search_keys(self) -> &'static [&'static str] {
        match self {
            Kind::Pr => &["author"],
            Kind::Issue => &["assignee", "author", "label"],
        }
    }

    pub(crate) fn state(self, opts: &Options) -> &'static str {
        match (self, opts.get("status")) {
            (_, Some("closed")) => "closed",
            (Kind::Pr, Some("merged")) => "merged",
            _ => "open",
        }
    }
}

/// What to fetch for one placeholder: a cache key and job per query.
pub struct Plan {
    pub jobs: Vec<(String, Job)>,
    pub warning: Option<String>,
    pub limit: usize,
    /// A GitLab remote: MRs read `!N`, issues `#N`.
    pub gitlab: bool,
}

/// `owner/repo` of a github.com remote URL; other hosts give `None`.
pub fn github_slug(url: &str) -> Option<String> {
    let path = url.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let ok = |s: &str| !s.is_empty() && !s.starts_with('-') && !s.contains(char::is_whitespace);
    (parts.next().is_none() && ok(owner) && ok(repo)).then(|| format!("{owner}/{repo}"))
}

/// Shown by `bar check` when a placeholder expands past the query cap.
pub fn cap_warning(kind: Kind, count: usize) -> String {
    format!(
        "{}s: {count} queries, only the first {MAX_QUERIES} run",
        kind.name()
    )
}

/// Search strings from the options; `[""]` when none are set. `None` when
/// an option value does not parse.
fn searches(kind: Kind, opts: &Options) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for key in kind.search_keys() {
        if let Some(v) = opts.get(key).filter(|v| !v.trim().is_empty()) {
            parts.push(expr::to_queries(key, &expr::parse(v).ok()?));
        }
    }
    let mut q = expr::combine(&parts).ok()?;
    if q.is_empty() {
        q.push(String::new());
    }
    Some(q)
}

/// `None` when the options are invalid or the remote is not on GitHub.
pub fn plan(kind: Kind, remote: &str, opts: &Options) -> Option<Plan> {
    let slug = github_slug(remote)?;
    let limit = opts
        .get("limit")
        .map_or(Some(DEFAULT_LIMIT), |l| l.trim().parse().ok())?;
    let mut queries = searches(kind, opts)?;
    let warning = (queries.len() > MAX_QUERIES).then(|| cap_warning(kind, queries.len()));
    queries.truncate(MAX_QUERIES);
    let state = kind.state(opts);
    let fields = match kind {
        Kind::Pr => PR_FIELDS,
        Kind::Issue => ISSUE_FIELDS,
    };
    let jobs = queries
        .into_iter()
        .map(|q| {
            let key = format!("gh:{}:{slug}:{q}:{state}:{limit}", kind.name());
            let mut args: Vec<String> = [kind.name(), "list", "-R", &slug, "--state", state]
                .map(String::from)
                .into();
            args.extend(["--limit".into(), limit.to_string()]);
            if !q.is_empty() {
                args.extend(["--search".into(), q]);
            }
            args.extend(["--json".into(), fields.into()]);
            (key, Job::Gh { args })
        })
        .collect();
    Some(Plan {
        jobs,
        warning,
        limit,
        gitlab: false,
    })
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}

/// Combined CI state of a PR's last commit, from `statusCheckRollup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    Pass,
    Fail,
    Pending,
    /// No checks reported, or no check data (older cache entries).
    None,
}

impl Checks {
    /// Name used by the `checks` option.
    pub fn name(self) -> &'static str {
        match self {
            Checks::Pass => "pass",
            Checks::Fail => "fail",
            Checks::Pending => "pending",
            Checks::None => "none",
        }
    }
}

/// Values of the `checks` option; `fail` is the default.
pub const CHECK_MARKS: &[&str] = &["fail", "pending", "pass", "none"];

/// A PR's check state: its reduced `checks` name when cached by [`run`],
/// else from its `statusCheckRollup` (GitLab, older cache entries).
pub fn checks(pr: &Value) -> Checks {
    match pr["checks"].as_str() {
        Some("pass") => Checks::Pass,
        Some("fail") => Checks::Fail,
        Some("pending") => Checks::Pending,
        Some(_) => Checks::None,
        None => rollup_checks(pr),
    }
}

/// Any failed check fails; else any unfinished one is pending; else pass.
/// Check runs carry `status`/`conclusion`, status contexts `state`.
fn rollup_checks(pr: &Value) -> Checks {
    let Some(list) = pr["statusCheckRollup"].as_array().filter(|a| !a.is_empty()) else {
        return Checks::None;
    };
    let outcome = |c: &Value| {
        c["conclusion"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| c["state"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let failed = list.iter().any(|c| {
        matches!(
            outcome(c).as_str(),
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "ERROR"
        )
    });
    let pending = list.iter().any(|c| {
        c["status"].as_str().is_some_and(|s| s != "COMPLETED")
            || matches!(str_of(c, "state"), "PENDING" | "EXPECTED")
    });
    if failed {
        Checks::Fail
    } else if pending {
        Checks::Pending
    } else {
        Checks::Pass
    }
}

/// Body of a `gh` job: `gh <args>` in bar's state dir, its JSON list
/// reduced per item to number, link and, for PRs, check state, review
/// decision, mergeability and draft flag; for issues, label names. `None`
/// when `gh` fails or its output does not parse (cut at `MAX_OUTPUT`
/// included), so the last value stays.
pub fn run(args: &[String], limit: Duration) -> Option<String> {
    let mut c = Command::new(crate::platform::program("gh"));
    c.args(args).current_dir(crate::job::outside_project());
    let out = run_child_capped(c, None, limit, MAX_OUTPUT)?;
    let Value::Array(items) = serde_json::from_str(&out).ok()? else {
        return None;
    };
    let pr = args.first().is_some_and(|a| a == "pr");
    let reduced: Vec<Value> = items
        .iter()
        .map(|i| if pr { compact_pr(i) } else { compact_issue(i) })
        .collect();
    Some(Value::Array(reduced).to_string())
}

/// The fields of a `gh` item that rendering reads, those it lacks left out.
fn pick(item: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter(|k| !item[**k].is_null())
        .map(|k| ((*k).to_string(), item[*k].clone()))
        .collect()
}

fn compact_pr(pr: &Value) -> Value {
    let mut m = pick(
        pr,
        &["number", "url", "reviewDecision", "mergeable", "isDraft"],
    );
    m.insert("checks".into(), rollup_checks(pr).name().into());
    Value::Object(m)
}

fn compact_issue(issue: &Value) -> Value {
    let mut m = pick(issue, &["number", "url"]);
    if let Some(labels) = issue["labels"].as_array() {
        let names = labels.iter().map(|l| json!({"name": str_of(l, "name")}));
        m.insert("labels".into(), names.collect());
    }
    Value::Object(m)
}

/// Port of the reference classification: red, amber, else green.
fn pr_color(ctx: &Ctx, pr: &Value, state: Checks) -> Color {
    let colors = &ctx.config.colors;
    if state == Checks::Fail
        || str_of(pr, "reviewDecision") == "CHANGES_REQUESTED"
        || str_of(pr, "mergeable") == "CONFLICTING"
    {
        colors.bad
    } else if state == Checks::Pending
        || str_of(pr, "reviewDecision") == "REVIEW_REQUIRED"
        || pr["isDraft"].as_bool() == Some(true)
    {
        colors.warn
    } else {
        colors.good
    }
}

/// Mark after a PR number for check states listed in option `checks`
/// (default `fail`); empty for unlisted states and `none`.
fn check_mark(ctx: &Ctx, opts: &Options, state: Checks) -> String {
    let wanted = opts.get("checks").unwrap_or("fail");
    if state == Checks::None || !wanted.split(',').any(|w| w.trim() == state.name()) {
        return String::new();
    }
    let colors = &ctx.config.colors;
    let color = match state {
        Checks::Fail => colors.bad,
        Checks::Pending => colors.warn,
        _ => colors.good,
    };
    fg(color, icons::check_mark(ctx.config.icons, state))
}

/// A PR's coloured `#N` and its check mark (empty when not shown).
pub fn pr_parts(ctx: &Ctx, opts: &Options, pr: &Value) -> (String, String) {
    numbered_parts(ctx, opts, pr, '#')
}

fn numbered_parts(ctx: &Ctx, opts: &Options, pr: &Value, sign: char) -> (String, String) {
    let n = pr["number"].as_u64().unwrap_or_default();
    let state = checks(pr);
    (
        fg(pr_color(ctx, pr, state), &format!("{sign}{n}")),
        check_mark(ctx, opts, state),
    )
}

fn has_bug_label(issue: &Value) -> bool {
    issue["labels"]
        .as_array()
        .is_some_and(|l| l.iter().any(|x| str_of(x, "name") == "bug"))
}

/// Cached results of every query, deduplicated by number, newest first.
fn merged(cache: &Cache, plan: &Plan) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    for (key, job) in &plan.jobs {
        let Some(raw) = cache.read_or_refresh(key, TTL, job) else {
            continue;
        };
        let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        for item in list {
            let n = item["number"].as_u64();
            if n.is_some() && !items.iter().any(|i| i["number"].as_u64() == n) {
                items.push(item);
            }
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i["number"].as_u64()));
    items.truncate(plan.limit);
    items
}

fn render_kind(kind: Kind, ctx: &Ctx, opts: &Options) -> Option<String> {
    let cache = ctx.cache?;
    let remote = cached_remote(ctx)?;
    let plan = plan(kind, &remote, opts)
        .or_else(|| super::glab::plan(kind, &remote, &ctx.config.gitlab_hosts, opts))?;
    let (pr_sign, issue_sign) = if plan.gitlab { ('!', '#') } else { ('#', '!') };
    if plan.limit == 0 {
        return None;
    }
    let links = link_enabled(opts);
    let parts: Vec<String> = merged(cache, &plan)
        .iter()
        .map(|item| {
            let n = item["number"].as_u64().unwrap_or_default();
            let (text, mark) = match kind {
                Kind::Pr => numbered_parts(ctx, opts, item, pr_sign),
                Kind::Issue if has_bug_label(item) => (
                    fg(ctx.config.colors.bad, &format!("{issue_sign}{n}")),
                    String::new(),
                ),
                Kind::Issue => (dim(&format!("{issue_sign}{n}")), String::new()),
            };
            let url = sanitize(str_of(item, "url"));
            let shown = if links && url.starts_with("https://") {
                link(&url, &text)
            } else {
                text
            };
            shown + &mark
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

impl Placeholder for Prs {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_kind(Kind::Pr, ctx, opts)
    }
}

impl Placeholder for Issues {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_kind(Kind::Issue, ctx, opts)
    }
}
