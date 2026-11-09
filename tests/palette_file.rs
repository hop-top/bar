//! User palette files: `palettes/<name>.conf` beside `bar.conf`, the file
//! format, how `set palette` finds them, and what `bar check` reports.

use hop_bar::check::{check_text, Level};
use hop_bar::color::Color;
use hop_bar::conf::{self, parse, LoadResult};
use hop_bar::palette::{self, file};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

const HARBOUR: &str = "# a user palette\n\
set muted \"#a0a0b0\"\n\
set good 114\n\
set warn \"#e0af68\"\n\
set bad red\n\
set backgrounds \"#202030 236 #303048\"\n";

/// Temp tree: `xdg/bar/bar.conf` and `xdg/bar/palettes/`.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Tree {
        let t = std::env::temp_dir().join(format!(
            "bar-palette-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&t);
        fs::create_dir_all(t.join("xdg/bar/palettes")).unwrap();
        Tree(t)
    }
    fn conf(&self, body: &str) -> &Tree {
        fs::write(self.0.join("xdg/bar/bar.conf"), body).unwrap();
        self
    }
    fn palette(&self, name: &str, body: &str) -> &Tree {
        fs::write(self.palettes().join(format!("{name}.conf")), body).unwrap();
        self
    }
    fn palettes(&self) -> PathBuf {
        self.0.join("xdg/bar/palettes")
    }
    fn load(&self, cwd: Option<&Path>) -> LoadResult {
        let xdg = self.0.join("xdg").to_string_lossy().into_owned();
        let env = move |k: &str| (k == "XDG_CONFIG_HOME").then(|| xdg.clone());
        conf::load_layered(&env, &self.0.join("home"), &self.0.join("kit"), cwd)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ok(r: LoadResult) -> conf::Config {
    match r {
        LoadResult::Ok(c) => c,
        LoadResult::Invalid(e) => panic!("{e:?}"),
        LoadResult::Missing => panic!("missing"),
    }
}

fn err(r: LoadResult) -> String {
    match r {
        LoadResult::Invalid(e) => e.iter().map(|e| e.message.clone()).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn parses_roles_and_derives_the_ramp() {
    let p = file::parse("harbour", HARBOUR).unwrap();
    assert_eq!(p.name, "harbour");
    assert_eq!(p.muted, Color::Rgb([0xa0, 0xa0, 0xb0]));
    assert_eq!((p.good, p.bad), (Color::Idx(114), Color::Idx(1)));
    assert_eq!(p.bgs.len(), 3);
    assert_eq!(p.bgs[1], Color::Idx(236));
    assert_eq!(p.ramp, palette::ramp(p.bad, p.warn, p.good));
    assert!(!p.ansi);
}

#[test]
fn explicit_ramp_is_kept() {
    let text = format!("{HARBOUR}set ramp \"1 2 3 4 5 6 7 8 9 10 11\"\n");
    let p = file::parse("harbour", &text).unwrap();
    assert_eq!(p.ramp, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].map(Color::Idx));
    let short = format!("{HARBOUR}set ramp \"1 2 3\"\n");
    let e = file::parse("harbour", &short).unwrap_err();
    assert!(e.contains("line 7: ramp needs exactly 11 colours"), "{e}");
}

#[test]
fn reports_unknown_keys_bad_colours_and_missing_roles() {
    let text = "set muted \"#zzzzzz\"\nset accent 75\nset good 300\ncolour bad 1\n";
    let e = file::parse("x", text).unwrap_err();
    assert!(e.contains("line 1: invalid colour `#zzzzzz`"), "{e}");
    assert!(e.contains("line 2: unknown palette key `accent`"), "{e}");
    assert!(e.contains("line 3: invalid colour `300`"), "{e}");
    assert!(e.contains("line 4: unknown directive `colour`"), "{e}");
    assert!(
        e.ends_with("missing muted, good, warn, bad, backgrounds"),
        "{e}"
    );
    let e = file::parse("x", "set muted 250\nset good 2\nset warn 3\nset bad 1\n").unwrap_err();
    assert_eq!(e, "missing backgrounds");
}

#[test]
fn unquoted_hex_gets_a_hint() {
    let text = HARBOUR.replace("\"#e0af68\"", "#e0af68");
    let e = file::parse("x", &text).unwrap_err();
    assert!(
        e.contains("line 4:") && e.contains("quote hex colours"),
        "{e}"
    );
}

#[test]
fn background_count_is_bounded() {
    let many = vec!["236"; file::MAX_BACKGROUNDS + 1].join(" ");
    let text = HARBOUR.replace("\"#202030 236 #303048\"", &format!("\"{many}\""));
    let e = file::parse("x", &text).unwrap_err();
    assert!(e.contains("backgrounds needs 1-16 colours"), "{e}");
}

#[test]
fn names_cannot_leave_the_directory() {
    for bad in ["", "../x", "a/b", ".hidden", "-x", "a b", "a.conf", "x\\y"] {
        assert!(!file::valid_name(bad), "{bad:?}");
    }
    for good in ["harbour", "my-theme_2", "A1"] {
        assert!(file::valid_name(good), "{good}");
    }
    let _d = palette::use_dir(Some(PathBuf::from("/tmp/p")));
    assert_eq!(file::path_of("../etc"), None);
    assert_eq!(file::path_of("ok"), Some(PathBuf::from("/tmp/p/ok.conf")));
}

#[test]
fn set_palette_reads_the_user_file() {
    let t = Tree::new();
    t.conf("set skin powerline\nset palette harbour\n")
        .palette("harbour", HARBOUR);
    let c = ok(t.load(None));
    let p = file::parse("harbour", HARBOUR).unwrap();
    assert_eq!(c.palette, p);
    assert_eq!((c.muted, c.colors.good), (p.muted, p.good));
    assert_eq!(c.bgs, p.bgs.to_vec());
    // the directory is only looked in while loading
    assert_eq!(palette::file::dir(), None);
}

#[test]
fn without_a_directory_only_built_ins_resolve() {
    let e = parse("set palette harbour").unwrap_err();
    assert!(
        e[0].message
            .starts_with("invalid palette `harbour` (default|nord|"),
        "{e:?}"
    );
}

#[test]
fn missing_and_broken_files_are_config_errors() {
    let t = Tree::new();
    t.conf("set palette harbour\n");
    let e = err(t.load(None));
    assert!(e.contains("invalid palette `harbour`"), "{e}");
    assert!(e.contains("palettes/harbour.conf"), "{e}");
    t.palette("harbour", "set muted 250\n");
    let e = err(t.load(None));
    assert!(e.contains("palette `harbour`"), "{e}");
    assert!(e.contains("missing good, warn, bad, backgrounds"), "{e}");
}

#[test]
fn built_in_names_win_over_files() {
    let t = Tree::new();
    t.conf("set palette nord\n").palette("nord", HARBOUR);
    let c = ok(t.load(None));
    assert_eq!(&c.palette, palette::by_name("nord").unwrap());
}

#[test]
fn a_project_config_may_pick_a_user_palette() {
    let t = Tree::new();
    t.conf("set gap 3\n").palette("harbour", HARBOUR);
    let repo = t.0.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join(".bar.conf"), "set palette harbour\n").unwrap();
    assert_eq!(ok(t.load(Some(&repo))).palette.name, "harbour");
    // a palette that does not resolve drops the project line only
    fs::write(
        repo.join(".bar.conf"),
        "set palette ../harbour\nset gap 5\n",
    )
    .unwrap();
    let c = ok(t.load(Some(&repo)));
    assert_eq!((c.palette.name.as_ref(), c.gap), ("default", 5));
}

#[test]
fn check_reports_the_file_problem_on_the_set_line() {
    let t = Tree::new();
    t.palette("harbour", "set muted 250\nset good nope\n");
    let _d = palette::use_dir(Some(t.palettes()));
    let r = check_text("set gap 3\nset palette harbour\n", None, &[]);
    let e = r
        .issues
        .iter()
        .find(|i| i.level == Level::Error)
        .expect("an error");
    assert_eq!(e.line, 2);
    assert!(
        e.message.contains("line 2: invalid colour `nope`"),
        "{}",
        e.message
    );
}

#[test]
fn check_warns_about_a_file_a_built_in_hides() {
    let t = Tree::new();
    t.palette("nord", HARBOUR);
    let _d = palette::use_dir(Some(t.palettes()));
    let r = check_text("set palette nord\n", None, &[]);
    assert!(!r.has_errors());
    let w = r
        .issues
        .iter()
        .find(|i| i.level == Level::Warning && i.message.contains("built-in palette"))
        .unwrap_or_else(|| panic!("{:?}", r.issues));
    assert_eq!(w.line, 1);
    assert!(w.message.contains("nord.conf"), "{}", w.message);
}

#[test]
fn palette_dir_sits_beside_the_config_in_use() {
    let t = Tree::new();
    let xdg = t.0.join("xdg").to_string_lossy().into_owned();
    let env = move |k: &str| (k == "XDG_CONFIG_HOME").then(|| xdg.clone());
    let (home, kit) = (t.0.join("home"), t.0.join("kit"));
    // no bar.conf: the first candidate's directory
    assert_eq!(conf::palette_dir(&env, &home, &kit), Some(t.palettes()));
    fs::create_dir_all(&kit).unwrap();
    fs::write(kit.join("bar.conf"), "").unwrap();
    assert_eq!(
        conf::palette_dir(&env, &home, &kit),
        Some(kit.join("palettes"))
    );
}
