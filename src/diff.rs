//! Line diff in unified format, for the changes `bar install` and `bar
//! config import` show before they write.

/// Most cells of the line table; a larger change shows as removed and
/// added whole.
const MAX_CELLS: usize = 1 << 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Del,
    Add,
}

/// Edit script from `a` to `b`: (op, index into `a` or `b`).
fn script(a: &[&str], b: &[&str]) -> Vec<(Op, usize)> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut out: Vec<(Op, usize)> = (0..pre).map(|i| (Op::Keep, i)).collect();
    let (n, m) = (ma.len(), mb.len());
    if (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        out.extend((0..n).map(|i| (Op::Del, pre + i)));
        out.extend((0..m).map(|j| (Op::Add, pre + j)));
    } else {
        // lcs[i][j]: longest common subsequence of ma[i..] and mb[j..]
        let w = m + 1;
        let mut lcs = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * w + j] = if ma[i] == mb[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && ma[i] == mb[j] {
                out.push((Op::Keep, pre + i));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                // removals before additions, as `diff -u` prints them
                out.push((Op::Del, pre + i));
                i += 1;
            } else {
                out.push((Op::Add, pre + j));
                j += 1;
            }
        }
    }
    out.extend((0..suf).map(|k| (Op::Keep, a.len() - suf + k)));
    out
}

/// `s` with what a terminal would act on instead of print made visible:
/// C0 controls but tab, and DEL, as `\x1b`; C1 controls and bidi
/// overrides as `\u{9b}`. A diff of a file shown before a write cannot
/// then hide or redraw lines.
pub fn visible(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push(c),
            '\0'..='\x1f' | '\x7f' => out.push_str(&format!("\\x{:02x}", c as u32)),
            '\u{80}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// Unified diff of `old` to `new` with three lines of context, headed
/// `--- from` / `+++ to`; empty when they have the same lines. Every line
/// and name is shown through [`visible`].
pub fn unified(old: &str, new: &str, from: &str, to: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = script(&a, &b);
    let changed: Vec<usize> = (0..ops.len()).filter(|&k| ops[k].0 != Op::Keep).collect();
    if changed.is_empty() {
        return String::new();
    }
    const CTX: usize = 3;
    let mut out = format!("--- {}\n+++ {}\n", visible(from), visible(to));
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CTX);
        let mut end = changed[k];
        while k < changed.len() && changed[k] <= end + 2 * CTX {
            end = changed[k];
            k += 1;
        }
        let end = (end + CTX + 1).min(ops.len());
        // 1-based start lines of the hunk in `a` and `b`
        let (mut la, mut lb) = (1, 1);
        for &(op, _) in &ops[..start] {
            match op {
                Op::Keep => {
                    la += 1;
                    lb += 1;
                }
                Op::Del => la += 1,
                Op::Add => lb += 1,
            }
        }
        let hunk = &ops[start..end];
        let na = hunk.iter().filter(|(op, _)| *op != Op::Add).count();
        let nb = hunk.iter().filter(|(op, _)| *op != Op::Del).count();
        let at = |l: usize, n: usize| if n == 0 { l - 1 } else { l };
        out.push_str(&format!(
            "@@ -{},{na} +{},{nb} @@\n",
            at(la, na),
            at(lb, nb)
        ));
        for &(op, i) in hunk {
            let (mark, line) = match op {
                Op::Keep => (' ', a[i]),
                Op::Del => ('-', a[i]),
                Op::Add => ('+', b[i]),
            };
            out.push(mark);
            out.push_str(&visible(line));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{unified, visible, MAX_CELLS};

    #[test]
    fn equal_texts_have_no_diff() {
        assert_eq!(unified("a\nb\n", "a\nb\n", "x", "y"), "");
    }

    #[test]
    fn one_change_with_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        assert_eq!(
            unified(old, new, "a", "b"),
            "--- a\n+++ b\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
        );
    }

    #[test]
    fn new_file_is_all_additions() {
        assert_eq!(
            unified("", "{\n}\n", "/dev/null", "s.json"),
            "--- /dev/null\n+++ s.json\n@@ -0,0 +1,2 @@\n+{\n+}\n"
        );
    }

    #[test]
    fn far_apart_changes_make_two_hunks() {
        let old: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let new: String = (1..=20)
            .map(|n| match n {
                2 => "two\n".to_string(),
                19 => "nineteen\n".to_string(),
                n => format!("{n}\n"),
            })
            .collect();
        let d = unified(&old, &new, "a", "b");
        assert_eq!(d.matches("@@ ").count(), 2, "{d}");
    }

    #[test]
    fn huge_change_shows_as_removed_and_added_whole() {
        let n = 3000;
        assert!((n + 1) * (n + 1) > MAX_CELLS);
        let old: String = (0..n).map(|i| format!("a{i}\n")).collect();
        let new: String = (0..n).map(|i| format!("b{i}\n")).collect();
        let d = unified(&format!("same\n{old}"), &format!("same\n{new}"), "a", "b");
        assert!(
            d.starts_with("--- a\n+++ b\n@@ -1,3001 +1,3001 @@\n same\n-a0\n"),
            "{}",
            &d[..80]
        );
        assert_eq!(d.matches("\n-").count(), n);
        assert_eq!(d.matches("\n+").count(), n + 1);
    }

    #[test]
    fn control_characters_are_shown_escaped() {
        let d = unified("a\n", "a\rset gap 1\x1b[2K\u{9b}x\tb\n", "f\x1b", "f");
        assert!(
            !d.contains('\r') && !d.contains('\x1b') && !d.contains('\u{9b}'),
            "{d:?}"
        );
        assert!(d.contains("+a\\x0dset gap 1\\x1b[2K\\u{9b}x\tb\n"), "{d}");
        assert!(d.starts_with("--- f\\x1b\n"), "{d}");
        assert_eq!(visible("plain ünï\tcode"), "plain ünï\tcode");
    }
}
