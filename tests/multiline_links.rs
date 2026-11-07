mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
use render_fx::*;

const OPEN: &str = "\x1b]8;;https://";
const CLOSE: &str = "\x1b]8;;\x1b\\";

#[test]
fn wrapped_rows_never_split_a_link() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(&d, &["remote", "add", "origin", "git@github.com:o/r.git"]);
    let conf = "set left \"aaaaaaa │ #{repo} │ #{branch}\"\nset center ccccccc\nset wrap on\n";
    for cols in ["12", "14", "20"] {
        let o = run(&t).conf(conf).env("COLUMNS", cols).cwd(&d).go();
        let rows: Vec<&str> = o.raw.trim_end_matches('\n').split('\n').collect();
        assert!(rows.len() > 1, "{cols}: {:?}", o.raw);
        let mut links = 0;
        for r in &rows {
            // every link opened on a row is closed on that row
            let opens = r.matches(OPEN).count();
            assert_eq!(opens, r.matches(CLOSE).count(), "{cols}: {r:?}");
            if let Some(at) = r.find(OPEN) {
                assert!(r[at..].contains(CLOSE), "{cols}: {r:?}");
            }
            links += opens;
        }
        assert_eq!(links, 2, "{cols}: {:?}", o.raw);
    }
}
