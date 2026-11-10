//! The native account chooser of the `pick` action: macOS `choose from
//! list` through `/usr/bin/osascript`. The title, prompt and items travel
//! as arguments of a fixed script (`on run argv`), never inside its
//! source, so no label can alter what the script does.

use crate::switch::Refusal;
use std::process::{Command, Stdio};

/// The program that shows the list.
const OSASCRIPT: &str = "/usr/bin/osascript";

/// Longest label or item shown, in characters.
pub const MAX_ITEM: usize = 120;

/// Fixed script. argv: title, prompt, default item, then the items. Prints
/// the chosen item, or nothing when cancelled.
pub const SCRIPT: &str = r#"on run argv
	set theTitle to item 1 of argv
	set thePrompt to item 2 of argv
	set theDefault to item 3 of argv
	set theItems to items 4 thru -1 of argv
	set r to choose from list theItems with title theTitle with prompt thePrompt default items {theDefault} without multiple selections allowed and empty selection allowed
	if r is false then return ""
	return item 1 of r
end run"#;

/// Printable text only, at most `max` characters; anything else becomes
/// a space.
pub fn sanitize(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Whether this build can show a chooser.
pub fn available() -> bool {
    cfg!(target_os = "macos") || fake().is_some()
}

/// Tests only (`test-tools`): a program that stands in for `osascript`,
/// given the same arguments: title, prompt, default item, items.
#[cfg(feature = "test-tools")]
fn fake() -> Option<String> {
    std::env::var("BAR_CLIPROXYAPI_CHOOSER")
        .ok()
        .filter(|p| p.starts_with('/'))
}

#[cfg(not(feature = "test-tools"))]
fn fake() -> Option<String> {
    None
}

/// Shows `items` (default `items[default]`); the index chosen, `None` on
/// Cancel. A reply that is none of the items is refused.
pub fn choose(
    title: &str,
    prompt: &str,
    items: &[String],
    default: usize,
) -> Result<Option<usize>, Refusal> {
    let Some(first) = items.get(default) else {
        return Err(Refusal("nothing to choose from".into()));
    };
    let mut c = match fake() {
        Some(p) => Command::new(p),
        None if cfg!(target_os = "macos") => {
            let mut c = Command::new(OSASCRIPT);
            c.arg("-e").arg(SCRIPT).arg("--");
            c
        }
        None => return Err(Refusal("the account chooser is macOS only".into())),
    };
    c.arg(sanitize(title, MAX_ITEM))
        .arg(sanitize(prompt, MAX_ITEM))
        .arg(first)
        .args(items)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = c
        .output()
        .map_err(|e| Refusal(format!("could not show the chooser: {e}")))?;
    if !out.status.success() {
        return Err(Refusal("the chooser failed".into()));
    }
    let said = String::from_utf8_lossy(&out.stdout);
    let said = said.trim_end_matches(['\n', '\r']);
    if said.is_empty() || said == "false" {
        return Ok(None);
    }
    items
        .iter()
        .position(|i| i == said)
        .map(Some)
        .ok_or_else(|| Refusal("the chooser returned something that was not offered".into()))
}
