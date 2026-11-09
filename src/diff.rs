//! Line diff in unified format, for the changes `bar install` shows
//! before it writes. Settings files are small, so a plain LCS table is
//! enough.

/// Lines of context around each change.
const CONTEXT: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Del,
    Add,
}

/// Unified diff of `old` and `new` under `---`/`+++` headers named
/// `old_name` and `new_name`; empty when the texts are equal.
pub fn unified(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = script(&a, &b);
    if ops.iter().all(|(op, _, _)| *op == Op::Keep) {
        return String::new();
    }
    let mut out = format!("--- {old_name}\n+++ {new_name}\n");
    let mut i = 0;
    while i < ops.len() {
        if ops[i].0 == Op::Keep {
            i += 1;
            continue;
        }
        // a hunk: back up for context, run until CONTEXT*2 unchanged lines
        let start = i.saturating_sub(CONTEXT);
        let mut end = i;
        let mut keep_run = 0;
        while end < ops.len() && keep_run <= CONTEXT * 2 {
            keep_run = if ops[end].0 == Op::Keep {
                keep_run + 1
            } else {
                0
            };
            end += 1;
        }
        let end = end - keep_run.saturating_sub(CONTEXT);
        let hunk = &ops[start..end];
        let (a0, b0) = (hunk[0].1, hunk[0].2);
        let a_len = hunk.iter().filter(|o| o.0 != Op::Add).count();
        let b_len = hunk.iter().filter(|o| o.0 != Op::Del).count();
        let pos = |s: usize, n: usize| if n == 0 { s } else { s + 1 };
        out += &format!(
            "@@ -{},{a_len} +{},{b_len} @@\n",
            pos(a0, a_len),
            pos(b0, b_len)
        );
        for (op, ai, bi) in hunk {
            match op {
                Op::Keep => out += &format!(" {}\n", a[*ai]),
                Op::Del => out += &format!("-{}\n", a[*ai]),
                Op::Add => out += &format!("+{}\n", b[*bi]),
            }
        }
        i = end;
    }
    out
}

/// Edit script: each op with the line index into `a` and `b` it sits at.
fn script(a: &[&str], b: &[&str]) -> Vec<(Op, usize, usize)> {
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: LCS length of a[i..] and b[j..]
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut ops = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((Op::Keep, i, j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            ops.push((Op::Del, i, j));
            i += 1;
        } else {
            ops.push((Op::Add, i, j));
            j += 1;
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::unified;

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
}
