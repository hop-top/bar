//! The Codex upstream phase of a run: each wanted account's usage read
//! through the proxy's `api-call`, in parallel batches, and what an
//! outcome does to the account.

use super::{store, CODEX_USAGE_URL, DEADLINE, PARALLEL, TTL_UPSTREAM};
use crate::cache::{self, Cache};
use crate::mgmt::{ApiReply, Client, MErr};
use crate::model::{self, Acct, Signals, Win};
use std::path::Path;

enum Up {
    Got(Vec<Win>, Option<String>),
    Rate(Option<u64>),
    UpstreamAuth,
    Failed,
    Mgmt(u16),
}

fn upstream(client: &Client, key: &str, now: i64) -> Up {
    let headers = [
        ("Authorization", "Bearer $TOKEN$"),
        ("Accept", "application/json"),
        ("User-Agent", "codex_cli_rs/0.0.0"),
    ];
    match client.api_call(key, "GET", CODEX_USAGE_URL, &headers) {
        Ok(ApiReply {
            status: 200..=299,
            body,
            ..
        }) => {
            let (w, plan) = model::codex_windows(&body, now);
            if w.is_empty() && body["rate_limit"].is_null() {
                Up::Failed
            } else {
                Up::Got(w, plan)
            }
        }
        Ok(ApiReply {
            status: 401 | 403, ..
        }) => Up::UpstreamAuth,
        Ok(ApiReply {
            status: 429,
            retry_after,
            ..
        }) => Up::Rate(retry_after),
        Ok(_) => Up::Failed,
        Err(MErr::Auth(s)) => Up::Mgmt(s),
        Err(_) => Up::Failed,
    }
}

/// Reads the accounts in `todo` upstream, `PARALLEL` at a time until the
/// deadline, saving after each batch: the indexes read, and the status of
/// a management refusal, which ends the phase.
pub(super) fn upstream_phase(
    client: &Client,
    list: &mut [(Acct, Signals)],
    todo: &[usize],
    now: i64,
    path: &Path,
    c: &mut Cache,
) -> (Vec<usize>, Option<u16>) {
    // the first call goes alone: a management refusal costs one attempt
    let mut batches: Vec<&[usize]> = Vec::new();
    if let Some((one, rest)) = todo.split_first().map(|(_, r)| (&todo[..1], r)) {
        batches.push(one);
        batches.extend(rest.chunks(PARALLEL));
    }
    let started = std::time::Instant::now();
    let mut done: Vec<usize> = Vec::new();
    let mut mgmt = None;
    for batch in batches {
        if started.elapsed().as_secs() >= DEADLINE {
            break;
        }
        for (i, up) in upstream_batch(client, list, batch, now) {
            done.push(i);
            if let Up::Mgmt(s) = up {
                mgmt = Some(s);
                continue;
            }
            let (a, sig) = &mut list[i];
            apply(a, sig, up, now);
        }
        if mgmt.is_some() {
            break;
        }
        store(path, c, list);
    }
    (done, mgmt)
}

/// One batch of upstream calls, in parallel; a thread that panics counts
/// as a failed call.
fn upstream_batch(
    client: &Client,
    list: &[(Acct, Signals)],
    batch: &[usize],
    now: i64,
) -> Vec<(usize, Up)> {
    let mut outcomes: Vec<(usize, Up)> = Vec::new();
    std::thread::scope(|s| {
        let hs: Vec<_> = batch
            .iter()
            .map(|&i| {
                let key = list[i].0.key.clone();
                (i, s.spawn(move || upstream(client, &key, now)))
            })
            .collect();
        for (i, h) in hs {
            outcomes.push((i, h.join().unwrap_or(Up::Failed)));
        }
    });
    outcomes
}

/// Codex accounts not read upstream this run: passive data when the
/// snapshot has any and no fresh upstream result is held.
pub(super) fn passive_codex(list: &mut [(Acct, Signals)], done: &[usize], now: i64) {
    for (i, (a, sig)) in list
        .iter_mut()
        .enumerate()
        .filter(|(_, (a, _))| a.provider == "codex")
    {
        if done.contains(&i) {
            continue;
        }
        let fresh = a.at > 0 && now - a.at < TTL_UPSTREAM && !a.windows.is_empty();
        let passive = model::codex_signal_windows(sig, now);
        if !fresh && !passive.is_empty() {
            a.windows = passive;
            a.at = now;
        }
    }
}

/// Folds one upstream outcome (not a management refusal) into `a`.
fn apply(a: &mut Acct, sig: &Signals, up: Up, now: i64) {
    let passive = model::codex_signal_windows(sig, now);
    match up {
        Up::Got(w, plan) => {
            a.windows = w;
            a.at = now;
            a.upstream_auth = false;
            a.retry_at = 0;
            a.fails = 0;
            if plan.is_some() {
                a.plan = plan;
            }
            return;
        }
        Up::Rate(r) => a.retry_at = now + r.unwrap_or(300).min(86_400) as i64,
        Up::UpstreamAuth => {
            a.upstream_auth = true;
            a.retry_at = now + 3600;
        }
        Up::Failed | Up::Mgmt(_) => {
            a.fails += 1;
            a.retry_at = now + cache::backoff(a.fails);
        }
    }
    if !passive.is_empty() {
        a.windows = passive;
        a.at = now;
    }
}
