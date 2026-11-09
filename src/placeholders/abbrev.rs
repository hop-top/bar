//! Fish-style abbreviation of branch names and paths (`feat/very-long` →
//! `f/very-long`, `~/src/bar/docs` → `~/s/b/docs`) and keeping only the
//! last components of a path (`…/bar/docs`).

use crate::options::Options;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// `fish`, `full` and `depth` options, parsed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Abbrev {
    /// Cells each abbreviated component keeps; `0` abbreviates nothing.
    pub fish: usize,
    /// Trailing components left whole.
    pub full: usize,
    /// Trailing components shown; `0` shows all.
    pub depth: usize,
}

fn num(opts: &Options, key: &str) -> Option<usize> {
    opts.get(key).and_then(|v| v.trim().parse().ok())
}

impl Abbrev {
    pub fn from(opts: &Options) -> Abbrev {
        Abbrev {
            fish: num(opts, "fish").unwrap_or(0),
            full: num(opts, "full").unwrap_or(1),
            depth: num(opts, "depth").unwrap_or(0),
        }
    }

    /// `text` split at `sep` characters, shortened as configured. The
    /// separators are kept as written.
    pub fn apply(&self, text: &str, sep: impl Fn(char) -> bool) -> String {
        let mut parts: Vec<(&str, Option<char>)> = Vec::new();
        let mut start = 0;
        for (i, c) in text.char_indices() {
            if sep(c) {
                parts.push((&text[start..i], Some(c)));
                start = i + c.len_utf8();
            }
        }
        parts.push((&text[start..], None));
        let mut head = String::new();
        // only named components count: a leading `/` or a doubled one is not
        let named = parts.iter().filter(|(p, _)| !p.is_empty()).count();
        if self.depth > 0 && named > self.depth {
            let mut seen = 0;
            let cut = parts
                .iter()
                .position(|(p, _)| {
                    seen += usize::from(!p.is_empty());
                    seen > named - self.depth
                })
                .unwrap_or(0);
            let sep = parts[..cut]
                .iter()
                .rev()
                .find_map(|(_, s)| *s)
                .unwrap_or('/');
            head = format!("…{sep}");
            parts.drain(..cut);
        }
        let named = parts.iter().filter(|(p, _)| !p.is_empty()).count();
        let keep_from = named.saturating_sub(self.full);
        let mut out = head;
        let mut seen = 0;
        for (p, s) in parts {
            if !p.is_empty() {
                let whole = self.fish == 0 || seen >= keep_from;
                seen += 1;
                out.push_str(&if whole {
                    p.to_string()
                } else {
                    short(p, self.fish)
                });
            }
            out.extend(s);
        }
        out
    }
}

/// `part` cut to `cells` cells (at least one character), whole grapheme
/// clusters only; a leading `.` is kept on top, as fish does. `~` and a
/// drive (`C:`) stay as they are.
fn short(part: &str, cells: usize) -> String {
    if part == "~" || (part.len() == 2 && part.ends_with(':')) {
        return part.to_string();
    }
    let (dot, rest) = match part.strip_prefix('.') {
        Some(r) if !r.is_empty() => (".", r),
        _ => ("", part),
    };
    let mut out = String::from(dot);
    let mut used = 0;
    for g in rest.graphemes(true) {
        let w = g.width();
        if used > 0 && used + w > cells {
            break;
        }
        used += w;
        out.push_str(g);
        if used >= cells {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ab(fish: usize, full: usize, depth: usize) -> Abbrev {
        Abbrev { fish, full, depth }
    }
    fn slash(c: char) -> bool {
        c == '/'
    }

    #[test]
    fn fish_style() {
        assert_eq!(ab(1, 1, 0).apply("feature/very-long", slash), "f/very-long");
        assert_eq!(ab(1, 1, 0).apply("main", slash), "main");
        assert_eq!(
            ab(2, 1, 0).apply("~/src/.config/bar", slash),
            "~/sr/.co/bar"
        );
        assert_eq!(
            ab(1, 2, 0).apply("/usr/local/share/doc", slash),
            "/u/l/share/doc"
        );
        assert_eq!(ab(1, 0, 0).apply("a1/b2/c3", slash), "a/b/c");
        assert_eq!(ab(0, 1, 0).apply("feature/x", slash), "feature/x");
        assert_eq!(ab(1, 1, 0).apply("/", slash), "/");
        assert_eq!(ab(1, 1, 0).apply("a//b/", slash), "a//b/");
    }

    #[test]
    fn depth_keeps_last_components() {
        assert_eq!(ab(0, 1, 2).apply("/a/b/c/d", slash), "…/c/d");
        assert_eq!(ab(0, 1, 2).apply("~/c/d", slash), "…/c/d");
        assert_eq!(ab(0, 1, 3).apply("~/c/d", slash), "~/c/d");
        assert_eq!(ab(1, 1, 3).apply("~/src/bar/docs/x", slash), "…/b/d/x");
    }

    #[test]
    fn wide_and_combined_characters() {
        // two-cell characters: one kept even past the cell budget
        assert_eq!(ab(1, 1, 0).apply("機能/x", slash), "機/x");
        assert_eq!(ab(3, 1, 0).apply("機能追加/x", slash), "機/x");
        assert_eq!(ab(4, 1, 0).apply("機能追加/x", slash), "機能/x");
        // a ZWJ emoji and a combining accent stay whole
        assert_eq!(ab(1, 1, 0).apply("👩‍💻dev/x", slash), "👩‍💻/x");
        assert_eq!(ab(1, 1, 0).apply("e\u{301}te/x", slash), "e\u{301}/x");
    }

    #[test]
    fn windows_separators_and_drive() {
        let win = |c: char| c == '/' || c == '\\';
        assert_eq!(
            ab(1, 1, 0).apply("C:\\Users\\me\\src", win),
            "C:\\U\\m\\src"
        );
        assert_eq!(ab(0, 1, 2).apply("C:\\Users\\me\\src", win), "…\\me\\src");
    }
}
