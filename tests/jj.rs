use hop_bar::cache::{Job, Request};
use hop_bar::placeholders::jj::{
    args, cache_key, discover, nearer_than, parse, Info, DEFAULT_SHOW, SHOW, TEMPLATE,
};
use std::path::{Path, PathBuf};

/// Output of `TEMPLATE` for a change with bookmarks on `@` and `@-`.
pub const SAMPLE: &str = "kp\0qxywon\x003f2a\x001b9c\0feat\0main\x1ffeat\0other\x1fdefault\0empty conflict\0fix: the thing\0";

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bar-jj-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn parses_every_field() {
    let i = parse(SAMPLE).unwrap();
    assert_eq!(i.change, ("kp".into(), "qxywon".into()));
    assert_eq!(i.commit, ("3f2a".into(), "1b9c".into()));
    assert_eq!(i.bookmarks, vec!["feat"]);
    assert_eq!(i.parent_bookmarks, vec!["main", "feat"]);
    assert_eq!(i.all_bookmarks(), vec!["feat", "main"]);
    assert!(i.empty && i.conflict && !i.divergent);
    assert_eq!(i.desc, "fix: the thing");
}

#[test]
fn empty_fields_parse() {
    let i = parse("ab\0cdefgh\x000123\x004567\0\0\x1f\0default\0\0\0").unwrap();
    assert!(i.all_bookmarks().is_empty());
    assert!(!i.empty && !i.conflict && !i.divergent);
    assert_eq!(i.desc, "");
    assert_eq!(i.workspace(Path::new("/r/proj")), None, "default is hidden");
}

#[test]
fn rejects_other_shapes() {
    assert_eq!(parse(""), None);
    assert_eq!(parse("Error: There is no jj repo in \".\"\n"), None);
    assert_eq!(parse("kp\0qx\x003f\x001b\0feat\0"), None, "cut short");
    assert_eq!(parse("\0qx\x003f\x001b\0\0\0\0\0\0"), None, "no id prefix");
    // the line-based shape of older bar versions
    assert_eq!(parse("kp\tqx\n3f\t1b\n\n\n\n\n\n"), None);
}

#[test]
fn whitespace_in_names_stays_in_one_name() {
    let i = parse("kp\0qx\x003f\x001b\0\"a b\"\x1fc\td\0x y\0my ws\0\0two  words \0").unwrap();
    assert_eq!(i.bookmarks, vec!["\"a b\"", "c\td"]);
    assert_eq!(i.parent_bookmarks, vec!["x y"]);
    assert_eq!(i.workspaces, vec!["my ws"]);
    assert_eq!(i.desc, "two  words ");
}

#[test]
fn workspace_prefers_the_one_named_after_the_root() {
    let i = parse(SAMPLE).unwrap();
    assert_eq!(i.workspace(Path::new("/w/default")), None);
    assert_eq!(i.workspace(Path::new("/w/elsewhere")), Some("other"));
    let i = Info {
        workspaces: vec!["a".into(), "b".into()],
        ..Info::default()
    };
    assert_eq!(i.workspace(Path::new("/w/b")), Some("b"));
}

#[test]
fn args_never_snapshot_and_log_only_at() {
    let a = args("/r");
    assert_eq!(a[0], "--ignore-working-copy");
    let joined = a.join(" ");
    assert!(joined.contains("-R /r log -r @ --no-graph -T"), "{joined}");
    assert_eq!(a.last().map(String::as_str), Some(TEMPLATE));
    assert!(a.contains(&"--color=never".to_string()));
}

#[test]
fn template_has_one_field_per_nul() {
    // 2 ids of 2 parts + 2 bookmark lists + workspaces + state + description
    assert_eq!(TEMPLATE.matches("\\0\"").count(), 9);
    assert!(!TEMPLATE.contains("\\n"), "no line separators");
    // keywords as methods of `self`: a template alias cannot shadow them
    for w in [
        "self.change_id()",
        "self.commit_id()",
        "self.local_bookmarks()",
        "self.parents()",
        "self.working_copies()",
        "self.empty()",
        "self.conflict()",
        "self.divergent()",
        "self.description()",
    ] {
        assert!(TEMPLATE.contains(w), "{w}");
    }
}

#[test]
fn show_sets() {
    for p in DEFAULT_SHOW {
        assert!(SHOW.contains(p));
    }
}

#[test]
fn discover_finds_nearest_jj_dir() {
    let d = tmp("disc");
    let sub = d.join("ws/a/b");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(discover(&sub), None);
    std::fs::create_dir_all(d.join("ws/.jj")).unwrap();
    assert_eq!(discover(&sub), Some(d.join("ws")));
    // a `.jj` file is not a workspace
    std::fs::write(d.join("ws/a/.jj"), "").unwrap();
    assert_eq!(discover(&sub), Some(d.join("ws")));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn nearer_than_git() {
    let d = tmp("near");
    let ws = d.join("outer/ws");
    std::fs::create_dir_all(ws.join(".jj")).unwrap();
    let outer = d.join("outer");
    assert_eq!(nearer_than(&ws, None), Some(ws.clone()));
    assert_eq!(nearer_than(&ws, Some(&outer)), Some(ws.clone()), "nested");
    assert_eq!(nearer_than(&ws, Some(&ws)), None, "colocated");
    assert_eq!(nearer_than(&ws.join("x"), Some(&ws.join("x"))), None);
    // reached through a symlink, compared to the canonical git top
    let link = d.join("link");
    std::os::unix::fs::symlink(&ws, &link).unwrap();
    assert_eq!(nearer_than(&link, Some(&ws)), None, "colocated via symlink");
    assert_eq!(nearer_than(&ws, Some(&link)), None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn job_roundtrips_and_needs_absolute_dir() {
    let req = Request {
        key: cache_key(Path::new("/r")),
        job: Job::Jj { dir: "/r".into() },
        lock: "t".into(),
    };
    assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    let rel = Request {
        job: Job::Jj { dir: "rel".into() },
        ..req
    };
    assert!(Request::from_json(&rel.to_json()).is_err());
}
