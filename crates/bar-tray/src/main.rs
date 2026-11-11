//! `bar-tray <snapshot.json>`: reads a tray snapshot, refuses an invalid
//! one and prints the menu it would show. No backend yet: nothing is
//! spawned, nothing is drawn.

use bar_tray::model::{self, Kind, Menu, MenuItem};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: bar-tray <snapshot.json>");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bar-tray: {}: {e}", path.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    let snapshot = match model::parse(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bar-tray: invalid snapshot: {e}");
            return ExitCode::from(1);
        }
    };
    for w in &snapshot.warnings {
        eprintln!("bar-tray: warning: {w}");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    print!("{}", tree(&Menu::build(&snapshot, now)));
    ExitCode::SUCCESS
}

/// One line per row: `[x]`/`[ ]` for toggles and choices, `>` for a
/// submenu, `---` for a separator, then the label, detail and flags.
fn tree(m: &Menu) -> String {
    let mut out = String::new();
    for (row, depth) in m.flatten() {
        out.push_str(&"    ".repeat(depth));
        out.push_str(&line(row));
        out.push('\n');
    }
    out
}

fn line(row: &MenuItem) -> String {
    let mark = match row.kind {
        Kind::Separator => return "---".to_string(),
        Kind::Toggle | Kind::Choice if row.checked => "[x] ",
        Kind::Toggle | Kind::Choice => "[ ] ",
        Kind::Submenu(_) => "> ",
        Kind::Info | Kind::Action => "",
    };
    let mut s = format!("{mark}{}", row.label);
    if let Some(d) = &row.detail {
        s.push_str(&format!(" — {d}"));
    }
    let mut flags = Vec::new();
    if row.confirm {
        flags.push("confirm".to_string());
    }
    if let Some(st) = row.style {
        flags.push(format!("{st:?}").to_lowercase());
    }
    if !row.enabled && !matches!(row.kind, Kind::Info | Kind::Submenu(_)) {
        flags.push("disabled".to_string());
    }
    if !flags.is_empty() {
        s.push_str(&format!("  ({})", flags.join(", ")));
    }
    format!("{s}  #{}", row.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_lines() {
        let s = model::parse(include_str!("../tests/fixtures/snapshot.json")).unwrap();
        let t = tree(&Menu::build(&s, 1_762_800_000));
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "> Claude · :8317  #cliproxyapi.claude@8317");
        assert_eq!(
            lines[1],
            "    [x] alice — 5h 59% ↻2h10m · 7d 71%  (good)  #cliproxyapi.claude@8317.3"
        );
        assert_eq!(
            lines[5],
            "Use soonest reset  (confirm)  #cliproxyapi.next@8317.next-soon"
        );
        assert_eq!(
            lines[6],
            "management refused the key  (bad)  #cliproxyapi.info.auth"
        );
        assert_eq!(lines[7], "---");
        assert_eq!(lines[9], "[ ] Routing :8318 — not enabled: tray routellm.toggle  (disabled)  #routellm.rtellm@8317.toggle@8318");
        assert_eq!(lines[lines.len() - 1], "Quit  #tray.quit");
    }
}
