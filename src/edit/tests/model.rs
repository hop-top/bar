use super::editor;
use crate::edit::model::{builtin_text, Cursor, Editor};
use crate::presets;
use std::path::PathBuf;

const CONF: &str = "\
# my layout
set left \"#{model} │ #{ctx} │ #{repo}:#{branch}\"  # main row
set right \"#{account}\"
rule usage[type=5h] early pace final=1h bad=30%
";

fn at(ed: &mut Editor, row: usize, seg: usize) {
    ed.cur = Cursor { row, seg, ph: 0 };
}

#[test]
fn reads_segments_per_row() {
    let ed = editor(Some(CONF));
    assert_eq!(ed.segments(0), ["#{model}", "#{ctx}", "#{repo}:#{branch}"]);
    assert!(ed.segments(1).is_empty());
    assert_eq!(ed.segments(2), ["#{account}"]);
    // line 1 in use, plus one line to grow into
    assert_eq!(ed.rows_shown(), 6);
}

#[test]
fn move_segment_rewrites_only_that_value() {
    let mut ed = editor(Some(CONF));
    at(&mut ed, 0, 0);
    assert!(ed.move_segment(1));
    assert_eq!(ed.cur.seg, 1);
    assert_eq!(
        ed.text(),
        CONF.replace("#{model} │ #{ctx} │", "#{ctx} │ #{model} │")
    );
    assert!(!ed.move_segment(5));
    at(&mut ed, 0, 0);
    assert!(!ed.move_segment(-1));
}

#[test]
fn move_segment_to_another_row() {
    let mut ed = editor(Some(CONF));
    at(&mut ed, 0, 2);
    assert!(ed.move_segment_row(1)); // down to center: front
    assert_eq!(ed.segments(0), ["#{model}", "#{ctx}"]);
    assert_eq!(ed.segments(1), ["#{repo}:#{branch}"]);
    assert_eq!(
        ed.cur,
        Cursor {
            row: 1,
            seg: 0,
            ph: 0
        }
    );
    assert!(ed.text().ends_with("set center \"#{repo}:#{branch}\"\n"));
    assert!(ed.move_segment_row(1)); // into right, at the front
    assert_eq!(ed.segments(2), ["#{repo}:#{branch}", "#{account}"]);
}

#[test]
fn insert_and_delete() {
    let mut ed = editor(Some(CONF));
    at(&mut ed, 0, 0);
    assert!(ed.insert("cost"));
    assert_eq!(ed.segments(0)[1], "#{cost}");
    assert_eq!(ed.cur.seg, 1);
    assert!(ed.delete());
    assert_eq!(ed.text(), CONF);
    // into an empty row: a new line
    at(&mut ed, 4, 0);
    assert!(ed.insert("usage"));
    assert!(ed.text().ends_with("set center@2 \"#{usage}\"\n"));
    assert!(ed.delete());
    assert!(ed.text().ends_with("set center@2 \"\"\n"));
    assert!(!ed.delete());
}

#[test]
fn set_option_on_selected_placeholder() {
    let mut ed = editor(Some(CONF));
    at(&mut ed, 0, 2);
    ed.next_placeholder(); // branch
    assert_eq!(ed.selected_placeholder().unwrap().name, "branch");
    assert_eq!(ed.set_option("max", Some("20")), Ok(true));
    assert_eq!(ed.segments(0)[2], "#{repo}:#{branch[max=20]}");
    assert_eq!(ed.set_option("max", None), Ok(true));
    assert_eq!(ed.text(), CONF);
    assert!(ed.set_option("label", Some("a,b")).is_err());
    assert_eq!(ed.text(), CONF);
}

#[test]
fn set_style_appends_then_rewrites() {
    let mut ed = editor(Some(CONF));
    assert!(ed.set_style("skin", "powerline"));
    assert!(ed.text().ends_with("set skin powerline\n"));
    assert!(ed.set_style("skin", "capsule"));
    assert_eq!(ed.text(), format!("{CONF}set skin capsule\n"));
    assert!(ed.config().is_ok());
}

#[test]
fn undo_redo_restore_text_and_selection() {
    let mut ed = editor(Some(CONF));
    at(&mut ed, 0, 0);
    ed.move_segment(1);
    ed.set_style("palette", "nord");
    let after = ed.text();
    assert!(ed.dirty());
    assert!(ed.undo());
    assert!(ed.undo());
    assert_eq!(ed.text(), CONF);
    assert_eq!(ed.cur.seg, 0);
    assert!(!ed.dirty());
    assert!(!ed.undo());
    assert!(ed.redo());
    assert!(ed.redo());
    assert_eq!(ed.text(), after);
    assert!(!ed.redo());
    // a new edit drops the redo history
    ed.undo();
    ed.set_style("skin", "plain");
    assert!(!ed.redo());
}

#[test]
fn apply_preset_keeps_other_lines() {
    let mut ed = editor(Some(CONF));
    ed.set_style("skin", "capsule");
    assert!(ed.apply_preset("two-line"));
    let t = ed.text();
    assert!(t.starts_with("# my layout\n"));
    assert!(t.contains("  # main row"));
    assert!(t.contains("rule usage[type=5h] early pace"));
    assert!(!t.contains("skin"), "{t}");
    assert_eq!(ed.preset(), Some("two-line"));
    let two = editor(presets::get("two-line"));
    for r in 0..12 {
        assert_eq!(ed.segments(r), two.segments(r), "row {r}");
    }
    assert!(ed.apply_preset("powerline"));
    assert_eq!(ed.preset(), Some("powerline"));
    assert!(ed.segments(3).is_empty());
    assert_eq!(ed.get("palette").as_deref(), Some("nord"));
    assert!(ed.undo());
    assert_eq!(ed.preset(), Some("two-line"));
}

#[test]
fn no_file_starts_from_the_builtin_layout() {
    let ed = editor(None);
    assert!(!ed.exists && !ed.dirty());
    assert_eq!(ed.text(), builtin_text());
    let c = ed.config().unwrap();
    let b = crate::render::builtin_config();
    assert_eq!((c.left, c.center, c.right), (b.left, b.center, b.right));
}

#[test]
fn invalid_config_still_edits() {
    let mut ed = editor(Some("set skin bogus\nset left \"#{model}\"\n"));
    assert!(ed.config().is_err());
    assert!(ed.set_style("skin", "plain"));
    assert!(ed.config().is_ok());
}

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> TmpDir {
        let p = std::env::temp_dir().join(format!("bar-edit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TmpDir(p)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn save_writes_the_text_and_clears_dirty() {
    let t = TmpDir::new("save");
    let path = t.0.join("sub/bar.conf");
    let mut ed = Editor::new(path.clone(), None);
    ed.set_style("skin", "capsule");
    assert!(ed.dirty());
    ed.save().unwrap();
    assert!(!ed.dirty() && ed.exists);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ed.text());
    let left: Vec<_> = std::fs::read_dir(t.0.join("sub")).unwrap().collect();
    assert_eq!(left.len(), 1, "no temp file left behind");
}

#[cfg(unix)]
#[test]
fn save_keeps_mode_and_follows_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let t = TmpDir::new("link");
    let real = t.0.join("dotfiles.conf");
    std::fs::write(&real, CONF).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = t.0.join("bar.conf");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut ed = Editor::new(link.clone(), Some(CONF.into()));
    ed.set_style("skin", "plain");
    ed.save().unwrap();
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&real).unwrap(), ed.text());
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn first_save_backs_up_the_old_file_once() {
    let t = TmpDir::new("bak");
    let path = t.0.join("bar.conf");
    std::fs::write(&path, CONF).unwrap();
    let mut ed = Editor::new(path.clone(), Some(CONF.into()));
    ed.set_style("skin", "plain");
    let (_, bak) = ed.save().unwrap();
    let bak = bak.expect("first save backs up");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), CONF);
    ed.set_style("skin", "capsule");
    assert_eq!(ed.save().unwrap().1, None);
    let baks = std::fs::read_dir(&t.0)
        .unwrap()
        .filter(|e| {
            let n = e.as_ref().unwrap().file_name();
            n.to_string_lossy().starts_with("bar.conf.bak-")
        })
        .count();
    assert_eq!(baks, 1);
}
