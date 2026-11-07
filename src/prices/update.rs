//! `bar prices update`: fetch models.dev, keep only what pricing needs,
//! write the result atomically. The one place bar touches the network;
//! it shells out to `curl` with a fixed argv rather than adding an HTTP
//! client dependency.

use super::models::user_path;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const URL: &str = "https://models.dev/api.json";
/// Larger than the real feed by a wide margin; a runaway body stops here.
const MAX_BYTES: &str = "67108864";

/// Fetches, trims and writes to `out` (default: the data-dir file).
/// Returns the path and the number of models kept.
pub fn run(out: Option<&Path>) -> Result<(PathBuf, usize), String> {
    let body = fetch()?;
    let days = crate::tz::day_string(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64 / 86_400),
    );
    let (text, n) = trim(&body, &days)?;
    let path = out.map_or_else(user_path, Path::to_path_buf);
    write_atomic(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, n))
}

fn fetch() -> Result<Vec<u8>, String> {
    let o = Command::new("curl")
        .args(["-fsS", "--max-time", "60", "--proto", "=https"])
        .args(["--proto-redir", "=https", "--max-filesize", MAX_BYTES, URL])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        return Err(format!("curl failed ({}): {}", o.status, err.trim()));
    }
    Ok(o.stdout)
}

/// The trimmed snapshot text for a models.dev body, and its model
/// count. Models without an input and output price are dropped.
pub fn trim(body: &[u8], fetched: &str) -> Result<(String, usize), String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("not JSON: {e}"))?;
    let all = v.as_object().ok_or("unexpected models.dev shape")?;
    let mut providers: BTreeMap<&str, BTreeMap<&str, String>> = BTreeMap::new();
    for (p, pv) in all {
        let Some(models) = pv.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (id, m) in models {
            let cost = &m["cost"];
            let num = |k: &str| cost.get(k).filter(|n| n.is_number());
            let (Some(i), Some(o)) = (num("input"), num("output")) else {
                continue;
            };
            let opt = |n: Option<&Value>| n.map_or("null".to_string(), Value::to_string);
            let ctx = m["limit"].get("context").filter(|n| n.is_u64());
            let row = format!(
                "[{i},{o},{},{},{}]",
                opt(num("cache_write")),
                opt(num("cache_read")),
                opt(ctx)
            );
            providers.entry(p).or_default().insert(id, row);
        }
    }
    let n: usize = providers.values().map(BTreeMap::len).sum();
    if n == 0 {
        return Err("no priced models in the response".into());
    }
    let key = |s: &str| Value::from(s).to_string();
    let blocks: Vec<String> = providers
        .iter()
        .map(|(p, ms)| {
            let rows: Vec<String> = ms
                .iter()
                .map(|(id, r)| format!("{}:{r}", key(id)))
                .collect();
            format!("{}:{{\n{}\n}}", key(p), rows.join(",\n"))
        })
        .collect();
    let text = format!(
        "{{\"source\":{},\"fetched\":{},\"providers\":{{\n{}\n}}}}\n",
        key(URL),
        key(fetched),
        blocks.join(",\n")
    );
    Ok((text, n))
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".models.json.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let res = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}
