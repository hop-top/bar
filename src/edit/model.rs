//! Editor state without a terminal: the document, the selected segment,
//! every edit, undo and redo, and saving.

use super::doc::{self, Doc};
use super::group::{self, Ph};
use crate::conf::{self, layout_key, ConfError, Config, MAX_LINES};
use crate::presets;
use crate::render::builtin_config;
use std::path::PathBuf;

pub const GROUPS: [&str; 3] = ["left", "center", "right"];
/// `separator` when the file sets none.
const DEFAULT_SEPARATOR: &str = "│";

/// Style keys a preset owns: applying one sets those it names and drops the
/// rest, so the preset looks as `bar preview --preset` shows it.
const PRESET_STYLE: [&str; 5] = ["skin", "skin-glyphs", "palette", "icons", "gap"];
/// One row per group of each line: `line * 3 + group`.
pub const ROWS: usize = MAX_LINES * 3;

/// The selection: a group row, a segment in it and a placeholder in that.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub seg: usize,
    pub ph: usize,
}

#[derive(Clone)]
pub struct Editor {
    doc: Doc,
    saved: String,
    undo: Vec<(Doc, Cursor)>,
    redo: Vec<(Doc, Cursor)>,
    pub path: PathBuf,
    /// False until the file exists on disk.
    pub exists: bool,
    /// The file as it was before this session is backed up (first save).
    backed_up: bool,
    pub cur: Cursor,
}

/// `left`, `center@2`, ...: the key that sets `row`.
pub fn row_key(row: usize) -> String {
    let (line, g) = (row / 3 + 1, GROUPS[row % 3]);
    if line == 1 {
        g.to_string()
    } else {
        format!("{g}@{line}")
    }
}

fn is_row(row: usize) -> impl Fn(&str) -> bool {
    move |k: &str| layout_key(k) == Some(Ok((GROUPS[row % 3], row / 3 + 1)))
}

fn is_key(key: &str) -> impl Fn(&str) -> bool + '_ {
    move |k: &str| k == key
}

/// The built-in layout as config lines, for a new file.
pub fn builtin_text() -> String {
    let c = builtin_config();
    let mut out = String::new();
    for (k, v) in [("left", c.left), ("center", c.center), ("right", c.right)] {
        out.push_str(&format!("set {k} {}\n", doc::quote(&v, false)));
    }
    out
}

impl Editor {
    /// `text` is the file's contents, `None` when it does not exist yet (the
    /// built-in layout is then the starting point).
    pub fn new(path: PathBuf, text: Option<String>) -> Editor {
        let exists = text.is_some();
        let text = text.unwrap_or_else(builtin_text);
        Editor {
            doc: Doc::new(&text),
            saved: text,
            undo: Vec::new(),
            redo: Vec::new(),
            path,
            exists,
            backed_up: false,
            cur: Cursor::default(),
        }
    }

    pub fn text(&self) -> String {
        self.doc.text()
    }

    pub fn dirty(&self) -> bool {
        self.doc.text() != self.saved
    }

    pub fn config(&self) -> Result<Config, Vec<ConfError>> {
        conf::parse(&self.doc.text())
    }

    pub fn separator(&self) -> String {
        self.doc
            .get(&is_key("separator"))
            .unwrap_or_else(|| DEFAULT_SEPARATOR.into())
    }

    /// Current value of a `set` key, if the file sets it.
    pub fn get(&self, key: &str) -> Option<String> {
        self.doc.get(&is_key(key))
    }

    pub fn segments(&self, row: usize) -> Vec<String> {
        let src = self.doc.get(&is_row(row)).unwrap_or_default();
        group::split(&src, &self.separator())
    }

    /// Rows worth showing: every line in use plus one empty line to grow into.
    pub fn rows_shown(&self) -> usize {
        let used = (0..ROWS)
            .filter(|&r| !self.segments(r).is_empty())
            .map(|r| r / 3 + 1)
            .max()
            .unwrap_or(0);
        (used + 1).min(MAX_LINES) * 3
    }

    pub fn selected_segment(&self) -> Option<String> {
        self.segments(self.cur.row).get(self.cur.seg).cloned()
    }

    pub fn selected_placeholder(&self) -> Option<Ph> {
        group::placeholders(&self.selected_segment()?)
            .get(self.cur.ph)
            .cloned()
    }

    fn clamp(&mut self) {
        self.cur.row = self.cur.row.min(self.rows_shown() - 1);
        let n = self.segments(self.cur.row).len();
        self.cur.seg = self.cur.seg.min(n.saturating_sub(1));
        let p = self
            .selected_segment()
            .map_or(0, |s| group::placeholders(&s).len());
        self.cur.ph = self.cur.ph.min(p.saturating_sub(1));
    }

    /// Moves the selection by `rows` group rows and `segs` segments.
    pub fn go(&mut self, rows: isize, segs: isize) {
        let shown = self.rows_shown() as isize;
        if rows != 0 {
            self.cur.row = (self.cur.row as isize + rows).clamp(0, shown - 1) as usize;
            self.cur.ph = 0;
        }
        if segs != 0 {
            self.cur.seg = (self.cur.seg as isize + segs).max(0) as usize;
            self.cur.ph = 0;
        }
        self.clamp();
    }

    /// Next placeholder inside the selected segment, wrapping.
    pub fn next_placeholder(&mut self) {
        let n = self
            .selected_segment()
            .map_or(0, |s| group::placeholders(&s).len());
        if n > 0 {
            self.cur.ph = (self.cur.ph + 1) % n;
        }
    }

    /// Runs one edit as one undo step. False when the text did not change.
    fn edit(&mut self, f: impl FnOnce(&mut Doc) -> bool) -> bool {
        let before = (self.doc.clone(), self.cur);
        if !f(&mut self.doc) {
            return false;
        }
        self.undo.push(before);
        self.redo.clear();
        true
    }

    fn write_row(&mut self, row: usize, segs: &[String]) -> bool {
        let src = group::join(segs, &self.separator());
        let had = self.doc.get(&is_row(row)).is_some();
        if !had && src.is_empty() {
            return false;
        }
        let key = row_key(row);
        self.edit(|d| d.set(&is_row(row), &key, &src))
    }

    /// Swaps the selected segment with its neighbour `delta` places away.
    pub fn move_segment(&mut self, delta: isize) -> bool {
        let mut segs = self.segments(self.cur.row);
        let to = self.cur.seg as isize + delta;
        if segs.is_empty() || to < 0 || to as usize >= segs.len() {
            return false;
        }
        segs.swap(self.cur.seg, to as usize);
        let moved = self.write_row(self.cur.row, &segs);
        if moved {
            self.cur.seg = to as usize;
        }
        moved
    }

    /// Moves the selected segment to the end (down) or start (up) of the
    /// next or previous group row.
    pub fn move_segment_row(&mut self, delta: isize) -> bool {
        let to = self.cur.row as isize + delta;
        if to < 0 || to as usize >= ROWS {
            return false;
        }
        let to = to as usize;
        let mut from = self.segments(self.cur.row);
        if from.is_empty() {
            return false;
        }
        let seg = from.remove(self.cur.seg);
        let mut dest = self.segments(to);
        let at = if delta > 0 { 0 } else { dest.len() };
        dest.insert(at, seg);
        let sep = self.separator();
        let (src_from, src_to) = (group::join(&from, &sep), group::join(&dest, &sep));
        let (kf, kt) = (row_key(self.cur.row), row_key(to));
        let row = self.cur.row;
        let changed =
            self.edit(|d| d.set(&is_row(row), &kf, &src_from) | d.set(&is_row(to), &kt, &src_to));
        if changed {
            self.cur = Cursor {
                row: to,
                seg: at,
                ph: 0,
            };
        }
        changed
    }

    /// Adds `#{name}` as a new segment after the selected one.
    pub fn insert(&mut self, name: &str) -> bool {
        let mut segs = self.segments(self.cur.row);
        let at = if segs.is_empty() { 0 } else { self.cur.seg + 1 };
        segs.insert(at, group::write_placeholder(name, &Default::default()));
        let done = self.write_row(self.cur.row, &segs);
        if done {
            self.cur.seg = at;
            self.cur.ph = 0;
        }
        done
    }

    pub fn delete(&mut self) -> bool {
        let mut segs = self.segments(self.cur.row);
        if segs.is_empty() {
            return false;
        }
        segs.remove(self.cur.seg);
        let done = self.write_row(self.cur.row, &segs);
        self.clamp();
        done
    }

    /// Sets option `key` of the selected placeholder, or removes it (`None`).
    pub fn set_option(&mut self, key: &str, value: Option<&str>) -> Result<bool, String> {
        if let Some(e) = value.and_then(|v| group::value_error(key, v)) {
            return Err(e);
        }
        let (Some(seg), Some(ph)) = (self.selected_segment(), self.selected_placeholder()) else {
            return Ok(false);
        };
        let opts = group::set_option(&ph.options, key, value);
        let mut segs = self.segments(self.cur.row);
        segs[self.cur.seg] = group::with_options(&seg, &ph, &opts);
        Ok(self.write_row(self.cur.row, &segs))
    }

    /// `set <key> <value>` (skin, palette, icons, ...).
    pub fn set_style(&mut self, key: &str, value: &str) -> bool {
        self.edit(|d| d.set(&is_key(key), key, value))
    }

    /// The preset whose layout and style the file matches, if any.
    pub fn preset(&self) -> Option<&'static str> {
        let mine = Editor::new(PathBuf::new(), Some(self.text()));
        presets::NAMES.iter().copied().find(|p| {
            let theirs = Editor::new(PathBuf::new(), presets::get(p).map(str::to_string));
            (0..ROWS).all(|r| mine.segments(r) == theirs.segments(r))
                && PRESET_STYLE.iter().all(|k| mine.get(k) == theirs.get(k))
        })
    }

    /// Takes preset `name`'s layout and style keys; every other line stays.
    pub fn apply_preset(&mut self, name: &str) -> bool {
        let Some(src) = presets::get(name) else {
            return false;
        };
        let theirs = Doc::new(src);
        // the preset's segments, rejoined with this file's separator
        let (from, to) = (
            theirs
                .get(&is_key("separator"))
                .unwrap_or_else(|| DEFAULT_SEPARATOR.into()),
            self.separator(),
        );
        let changed = self.edit(|d| {
            for row in 0..ROWS {
                match theirs.get(&is_row(row)) {
                    Some(v) => {
                        let v = group::join(&group::split(&v, &from), &to);
                        d.set(&is_row(row), &row_key(row), &v)
                    }
                    None => d.remove(&is_row(row)),
                };
            }
            for k in PRESET_STYLE {
                match theirs.get(&is_key(k)) {
                    Some(v) => d.set(&is_key(k), k, &v),
                    None => d.remove(&is_key(k)),
                };
            }
            true
        });
        self.clamp();
        changed
    }

    pub fn undo(&mut self) -> bool {
        let Some((doc, cur)) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push((std::mem::replace(&mut self.doc, doc), self.cur));
        self.cur = cur;
        self.clamp();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((doc, cur)) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push((std::mem::replace(&mut self.doc, doc), self.cur));
        self.cur = cur;
        self.clamp();
        true
    }

    /// Writes the text to the file through [`crate::rewrite`] (through a
    /// symlink to its target, atomic, permissions kept). The first save
    /// backs the old file up. Returns the file written and that backup.
    pub fn save(&mut self) -> Result<(PathBuf, Option<PathBuf>), String> {
        let text = self.doc.text();
        let target = crate::rewrite::target(&self.path)?;
        let shown = |e: std::io::Error| format!("{}: {e}", target.display());
        let bak = if self.backed_up {
            crate::rewrite::write(&target, &text).map_err(shown)?;
            None
        } else {
            crate::rewrite::replace(&target, &text, crate::rewrite::Backups::Beside)
                .map_err(shown)?
        };
        self.backed_up = true;
        self.saved = text;
        self.exists = true;
        Ok((target, bak))
    }
}
