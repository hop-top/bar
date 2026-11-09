//! GitLab remotes for `prs` and `issues`: `glab` lists, reduced in the
//! background job to the JSON shape `gh` gives, so rendering is shared.

use super::gh::{Kind, Plan, DEFAULT_LIMIT, MAX_QUERIES};
use crate::cache::{run_child_capped, Job};
use crate::expr;
use crate::options::Options;
use serde_json::{json, Value};
use std::process::Command;
use std::time::{Duration, Instant};

/// Always a GitLab host, whatever `gitlab-hosts` says.
pub const GITLAB_COM: &str = "gitlab.com";
/// Most bytes of `glab` output read; reduced to a few hundred per item.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// GitLab's largest page.
const MAX_PER_PAGE: usize = 100;
/// Merge requests per list that get a `glab mr view` for their pipeline.
pub const MAX_VIEWS: usize = 10;

/// `set gitlab-hosts`: comma-separated host names, lowercased; empty
/// entries are skipped, so `""` clears the list.
pub fn parse_hosts(val: &str) -> Result<Vec<String>, String> {
    let mut hosts = Vec::new();
    for h in val.split(',').map(str::trim).filter(|h| !h.is_empty()) {
        let ok = h.len() <= 253
            && !h.starts_with(['-', '.'])
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        if !ok {
            return Err(format!(
                "invalid gitlab-hosts `{h}` (comma-separated host names)"
            ));
        }
        let h = h.to_ascii_lowercase();
        if !hosts.contains(&h) {
            hosts.push(h);
        }
    }
    Ok(hosts)
}

/// `https://host[:port]/group/.../project` of a remote URL on gitlab.com
/// or one of `hosts`; other hosts give `None`.
pub fn gitlab_repo(url: &str, hosts: &[String]) -> Option<String> {
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    let host = host.to_ascii_lowercase();
    let (name, port) = host.split_once(':').unwrap_or((&host, "0"));
    if !port.bytes().all(|b| b.is_ascii_digit())
        || (name != GITLAB_COM && !hosts.iter().any(|h| h == name))
    {
        return None;
    }
    let ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['-', '.'])
            && !s.contains(|c: char| c.is_whitespace() || c.is_control() || "?#%\\".contains(c))
    };
    (path.split('/').count() >= 2 && path.split('/').all(ok))
        .then(|| format!("https://{host}/{path}"))
}

/// Flags for one DNF conjunct of `key`. `None`: not expressible with
/// `glab` flags; `Some(None)`: matches nothing (two different authors).
fn conjunct_flags(kind: Kind, key: &str, lits: &[(bool, String)]) -> Option<Option<Vec<String>>> {
    let (pos, neg): (Vec<_>, Vec<_>) = lits.iter().partition(|(n, _)| !n);
    if key == "label" {
        let mut f: Vec<String> = pos.iter().map(|(_, v)| format!("--label={v}")).collect();
        f.extend(neg.iter().map(|(_, v)| format!("--not-label={v}")));
        return Some(Some(f));
    }
    // `glab mr list` has no `--not-author`; issue filters take one value each
    if neg.len() > usize::from(kind == Kind::Issue) {
        return None;
    }
    if pos.len() > 1 {
        return Some(None);
    }
    let mut f: Vec<String> = pos.iter().map(|(_, v)| format!("--{key}={v}")).collect();
    f.extend(neg.iter().map(|(_, v)| format!("--not-{key}={v}")));
    Some(Some(f))
}

/// Flag sets, one per query; `[[]]` when no filter is set. `None` when an
/// option does not parse or has no `glab` form.
fn filters(kind: Kind, opts: &Options) -> Option<Vec<Vec<String>>> {
    let mut acc: Vec<Vec<String>> = vec![Vec::new()];
    for key in kind.search_keys() {
        let Some(v) = opts.get(key).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        let mut sets = Vec::new();
        for lits in expr::to_terms(&expr::parse(v).ok()?) {
            if let Some(f) = conjunct_flags(kind, key, &lits)? {
                sets.push(f);
            }
        }
        if acc.len().saturating_mul(sets.len()) > expr::MAX_CONJUNCTS {
            return None;
        }
        acc = acc
            .iter()
            .flat_map(|a| sets.iter().map(move |b| [a.clone(), b.clone()].concat()))
            .collect();
    }
    Some(acc)
}

/// `None` when the options are invalid for GitLab or the remote is not
/// a GitLab host.
pub fn plan(kind: Kind, remote: &str, hosts: &[String], opts: &Options) -> Option<Plan> {
    let repo = gitlab_repo(remote, hosts)?;
    let limit = opts
        .get("limit")
        .map_or(Some(DEFAULT_LIMIT), |l| l.trim().parse().ok())?;
    let mut sets = filters(kind, opts)?;
    let warning = (sets.len() > MAX_QUERIES).then(|| super::gh::cap_warning(kind, sets.len()));
    sets.truncate(MAX_QUERIES);
    let state = kind.state(opts);
    let sub = match kind {
        Kind::Pr => "mr",
        Kind::Issue => "issue",
    };
    let jobs = sets
        .into_iter()
        .map(|flags| {
            let key = format!(
                "glab:{}:{repo}:{}:{state}:{limit}",
                kind.name(),
                flags.join("\u{1f}")
            );
            let mut args: Vec<String> = [sub, "list", "-R", &repo].map(String::from).into();
            match state {
                "closed" => args.push("--closed".into()),
                "merged" => args.push("--merged".into()),
                _ => {}
            }
            let per_page = limit.clamp(1, MAX_PER_PAGE);
            args.extend(["--per-page".into(), per_page.to_string()]);
            args.extend(flags);
            args.extend(["--output".into(), "json".into()]);
            let job = Job::Glab {
                kind: sub.into(),
                repo: repo.clone(),
                args,
            };
            (key, job)
        })
        .collect();
    Some(Plan {
        jobs,
        warning,
        limit,
        gitlab: true,
    })
}

/// `glab` runs in bar's state dir, never the project, so nothing in the
/// project's directory reaches it.
fn glab(args: &[String], limit: Duration) -> Option<Value> {
    let mut c = Command::new(crate::platform::program("glab"));
    c.args(args).current_dir(crate::job::outside_project());
    let out = run_child_capped(c, None, limit, MAX_OUTPUT)?;
    serde_json::from_str(&out).ok()
}

/// Body of a `glab` job: the list, then for the first `MAX_VIEWS` merge
/// requests one `glab mr view` each for its pipeline, while time is left.
/// `None` when the list fails; a failed view only loses that MR's pipeline.
pub fn run(kind: &str, repo: &str, args: &[String], limit: Duration) -> Option<String> {
    let deadline = Instant::now() + limit;
    let Value::Array(items) = glab(args, limit)? else {
        return None;
    };
    let out: Vec<Value> = items
        .iter()
        .enumerate()
        .map(|(i, item)| match kind {
            "mr" => {
                let left = deadline.saturating_duration_since(Instant::now());
                let iid = item["iid"].as_u64();
                let view = iid.filter(|_| i < MAX_VIEWS && !left.is_zero());
                let view = view.and_then(|n| {
                    let a = ["mr", "view", &n.to_string(), "-R", repo, "--output", "json"];
                    glab(&a.map(String::from), left)
                });
                merge_request(item, view.as_ref())
            }
            _ => issue(item),
        })
        .collect();
    Some(Value::Array(out).to_string())
}

/// A pipeline status as a one-entry `statusCheckRollup`.
fn pipeline_rollup(status: &str) -> Value {
    let done = |c: &str| json!([{"status": "COMPLETED", "conclusion": c}]);
    match status {
        "success" => done("SUCCESS"),
        "skipped" => done("SKIPPED"),
        "failed" => done("FAILURE"),
        "canceled" | "canceling" => done("CANCELLED"),
        "created"
        | "waiting_for_resource"
        | "preparing"
        | "pending"
        | "running"
        | "scheduled"
        | "manual" => json!([{"status": "IN_PROGRESS"}]),
        _ => json!([]),
    }
}

/// A merge request (list entry, and its `glab mr view` when it ran) in
/// the fields `prs` reads from `gh`.
pub fn merge_request(item: &Value, view: Option<&Value>) -> Value {
    let src = view.filter(|v| v["iid"] == item["iid"]).unwrap_or(item);
    let status = src["detailed_merge_status"].as_str().unwrap_or_default();
    let review = match status {
        "not_approved" => "REVIEW_REQUIRED",
        "requested_changes" => "CHANGES_REQUESTED",
        _ => "",
    };
    let conflict = src["has_conflicts"].as_bool() == Some(true) || status == "conflict";
    let rollup = match src["head_pipeline"]["status"].as_str() {
        Some(s) => pipeline_rollup(s),
        None => match status {
            "ci_still_running" => pipeline_rollup("running"),
            "ci_must_pass" => pipeline_rollup("failed"),
            _ => json!([]),
        },
    };
    json!({
        "number": item["iid"],
        "url": item["web_url"],
        "isDraft": src["draft"].as_bool().unwrap_or(false),
        "reviewDecision": review,
        "mergeable": if conflict { "CONFLICTING" } else { "MERGEABLE" },
        "statusCheckRollup": rollup,
    })
}

/// An issue in the fields `issues` reads from `gh`.
pub fn issue(item: &Value) -> Value {
    let labels: Vec<Value> = item["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_str().or_else(|| l["name"].as_str()))
        .map(|name| json!({"name": name}))
        .collect();
    json!({"number": item["iid"], "url": item["web_url"], "labels": labels})
}
