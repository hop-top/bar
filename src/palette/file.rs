//! User palettes: `palettes/<name>.conf` in the directory of the `bar.conf`
//! in use, written in the same `set <key> <value>` form. A built-in name
//! always means the built-in palette; a file by that name is not read.

use super::{by_name, ramp, Palette, NAMES};
use crate::color::Color;
use crate::conf::{hex_comment, tokenize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Largest palette file read.
pub const MAX_BYTES: u64 = 16 * 1024;

/// Most backgrounds a palette lists.
pub const MAX_BACKGROUNDS: usize = 16;

/// Keys a palette file sets; all but `ramp` are required.
pub const KEYS: [&str; 6] = ["muted", "good", "warn", "bad", "backgrounds", "ramp"];

thread_local! {
    static DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Restores the previous palette directory when dropped.
pub struct DirGuard(Option<PathBuf>);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        DIR.with(|d| *d.borrow_mut() = prev);
    }
}

/// Looks for user palettes in `dir` on this thread until the guard drops;
/// `None`: built-in palettes only.
pub fn use_dir(dir: Option<PathBuf>) -> DirGuard {
    DirGuard(DIR.with(|d| d.replace(dir)))
}

/// The palette directory in effect on this thread.
pub fn dir() -> Option<PathBuf> {
    DIR.with(|d| d.borrow().clone())
}

/// A name a palette file may have: ASCII letters, digits, `-` and `_`,
/// starting with a letter or digit, at most 64 bytes, and not a Windows
/// device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`,
/// any case), on every platform, so a config means the same everywhere.
/// Nothing that could leave the palette directory.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=64).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
        && !windows_device(name)
}

fn windows_device(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), "con" | "prn" | "aux" | "nul")
        || matches!(
            n.as_bytes(),
            [b'c', b'o', b'm', b'1'..=b'9'] | [b'l', b'p', b't', b'1'..=b'9']
        )
}

/// Where the file for palette `name` would be, under the directory in effect.
pub fn path_of(name: &str) -> Option<PathBuf> {
    valid_name(name)
        .then(dir)
        .flatten()
        .map(|d| d.join(format!("{name}.conf")))
}

/// The palette a `set palette` value names: a built-in, else the user
/// file `<name>.conf` in the palette directory.
pub fn find(name: &str) -> Result<Palette, String> {
    if let Some(p) = by_name(name) {
        return Ok(p.clone());
    }
    let unknown = format!("invalid palette `{name}` ({})", NAMES.join("|"));
    let Some(path) = path_of(name) else {
        return Err(unknown);
    };
    match read(&path) {
        Ok(None) => Err(format!("{unknown}; no {}", path.display())),
        Ok(Some(text)) => {
            parse(name, &text).map_err(|e| format!("palette `{name}` ({}): {e}", path.display()))
        }
        Err(e) => Err(format!("palette `{name}`: {e}")),
    }
}

/// The file's text; `None` when there is no file.
fn read(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let shown = path.display();
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{shown}: {e}")),
    };
    let mut buf = Vec::new();
    f.take(MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{shown}: {e}"))?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(format!("{shown}: larger than 16 KiB"));
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| format!("{shown}: not UTF-8"))
}

fn colour(s: &str) -> Result<Color, String> {
    Color::parse(s).ok_or_else(|| format!("invalid colour `{s}` (0-255, #rrggbb or an ANSI name)"))
}

fn colours(s: &str) -> Result<Vec<Color>, String> {
    s.split_whitespace().map(colour).collect()
}

/// Parses a palette file's text. Every problem is reported, `; `-joined,
/// each with its 1-based line; missing keys last.
pub fn parse(name: &str, text: &str) -> Result<Palette, String> {
    let mut errs = Vec::new();
    let (mut muted, mut good, mut warn, mut bad) = (None, None, None, None);
    let (mut bgs, mut ramp_set): (Option<Vec<Color>>, Option<[Color; 11]>) = (None, None);
    for (i, raw) in text.lines().enumerate() {
        let r = tokenize(raw).and_then(|t| {
            let Some(first) = t.first() else {
                return Ok(());
            };
            if first != "set" {
                return Err(format!("unknown directive `{first}` (set)"));
            }
            let [_, key, val] = t.as_slice() else {
                return Err("usage: set <key> <value>; quote a list or a hex colour".into());
            };
            match key.as_str() {
                "muted" => muted = Some(colour(val)?),
                "good" => good = Some(colour(val)?),
                "warn" => warn = Some(colour(val)?),
                "bad" => bad = Some(colour(val)?),
                "backgrounds" => {
                    let v = colours(val)?;
                    if !(1..=MAX_BACKGROUNDS).contains(&v.len()) {
                        return Err(format!("backgrounds needs 1-{MAX_BACKGROUNDS} colours"));
                    }
                    bgs = Some(v);
                }
                "ramp" => {
                    ramp_set = Some(
                        colours(val)?
                            .try_into()
                            .map_err(|_| "ramp needs exactly 11 colours".to_string())?,
                    )
                }
                other => {
                    return Err(format!(
                        "unknown palette key `{other}` ({})",
                        KEYS.join("|")
                    ))
                }
            }
            Ok(())
        });
        if let Err(mut e) = r {
            if hex_comment(raw) {
                e.push_str("; quote hex colours: \"#rrggbb\"");
            }
            errs.push(format!("line {}: {e}", i + 1));
        }
    }
    let missing: Vec<&str> = [
        ("muted", muted.is_none()),
        ("good", good.is_none()),
        ("warn", warn.is_none()),
        ("bad", bad.is_none()),
        ("backgrounds", bgs.is_none()),
    ]
    .into_iter()
    .filter_map(|(k, m)| m.then_some(k))
    .collect();
    if !missing.is_empty() {
        errs.push(format!("missing {}", missing.join(", ")));
    }
    match (muted, good, warn, bad, bgs) {
        (Some(muted), Some(good), Some(warn), Some(bad), Some(bgs)) if errs.is_empty() => {
            Ok(Palette {
                name: Cow::Owned(name.to_string()),
                muted,
                good,
                warn,
                bad,
                ramp: ramp_set.unwrap_or(ramp(bad, warn, good)),
                bgs: Cow::Owned(bgs),
                ansi: false,
            })
        }
        _ => Err(errs.join("; ")),
    }
}
