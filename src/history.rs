//! Usage history behind the sparkline: a few samples of used percent per
//! window, in a small private file of the state directory.

use crate::click::write_atomic;
use std::path::{Path, PathBuf};

/// Samples kept per window; the oldest drop first.
pub const CAP: usize = 64;

/// Seconds between samples: window / 30, so a 30-cell spark spans the
/// window. 5h is 10 minutes, 7d (and a model's `7d-<model>`) 6 hours.
pub fn step(key: &str) -> i64 {
    if key == "7d" || key.starts_with("7d-") {
        6 * 3600
    } else {
        600
    }
}

fn file(state: &Path, key: &str) -> PathBuf {
    state.join(format!("hist-{key}"))
}

/// `resets_at` on the first line, then `ts used` per sample.
fn load(path: &Path) -> Option<(i64, Vec<(i64, f64)>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let resets = lines.next()?.trim().parse().ok()?;
    let mut out = Vec::new();
    for line in lines {
        let (t, u) = line.split_once(' ')?;
        let u: f64 = u.parse().ok()?;
        if !u.is_finite() {
            return None;
        }
        out.push((t.parse().ok()?, u));
    }
    Some((resets, out))
}

/// Appends `used` when a step has passed since the last sample and returns
/// the used percents of the current window, oldest first. A changed
/// `resets_at` or a clock that went backwards starts a new series; an
/// unreadable file counts as empty and a failed write is ignored.
pub fn sample(state: &Path, key: &str, resets_at: i64, now: i64, used: f64) -> Vec<f64> {
    let path = file(state, key);
    let mut series = match load(&path) {
        Some((r, s)) if r == resets_at && s.last().is_none_or(|&(t, _)| t <= now) => s,
        _ => Vec::new(),
    };
    let due = series.last().is_none_or(|&(t, _)| now - t >= step(key));
    if due && used.is_finite() {
        series.push((now, (used * 10.0).round() / 10.0));
        if series.len() > CAP {
            series.drain(..series.len() - CAP);
        }
        let mut body = format!("{resets_at}\n");
        for (t, u) in &series {
            body.push_str(&format!("{t} {u:.1}\n"));
        }
        let _ = write_atomic(&path, &body);
    }
    series.into_iter().map(|(_, u)| u).collect()
}
