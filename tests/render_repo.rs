mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::placeholders::repo::remote_url;
#[test]
fn remote_url_forms() {
    let t = Tmp::new();
    let d = t.path().join("r");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q"]);
    let cases = [
        ("git@github.com:o/r.git", "https://github.com/o/r"),
        ("ssh://git@host.io:2222/o/r.git", "https://host.io/o/r"),
        (
            "https://tok:en@github.com/o/r.git",
            "https://github.com/o/r",
        ),
        ("https://github.com/o/r/", "https://github.com/o/r"),
        ("https://github.com/o/r.git", "https://github.com/o/r"),
        ("https://u:p@ss@host.io/o/r.git", "https://host.io/o/r"),
    ];
    for (raw, want) in cases {
        git(&d, &["config", "remote.origin.url", raw]);
        assert_eq!(remote_url(&d).as_deref(), Some(want), "{raw}");
    }
    git(&d, &["remote", "remove", "origin"]);
    assert_eq!(remote_url(&d), None);
}

#[test]
fn repo_and_branch_link_to_remote() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(&d, &["remote", "add", "origin", "git@github.com:o/r.git"]);
    let o = run(&t).conf("set left \"#{repo}:#{branch}\"").cwd(&d).go();
    assert_eq!(o.plain.trim(), "r:main");
    assert!(o.raw.contains("\x1b]8;;https://github.com/o/r\x1b\\"));
    assert!(o
        .raw
        .contains("\x1b]8;;https://github.com/o/r/tree/main\x1b\\"));
}

#[test]
fn no_remote_is_plain_text() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let o = run(&t).conf("set left \"#{repo}:#{branch}\"").cwd(&d).go();
    assert_eq!(o.plain.trim(), "proj:main");
    assert!(!o.raw.contains("\x1b]8"));
}

#[test]
fn not_a_repo_uses_dirname() {
    let t = Tmp::new();
    let d = t.path().join("plain");
    std::fs::create_dir_all(&d).unwrap();
    let o = run(&t).conf("set left \"#{repo}:#{branch}\"").cwd(&d).go();
    assert_eq!(o.plain.trim(), "plain");
}

#[test]
fn hop_layout_uses_hub_name() {
    let t = Tmp::new();
    t.write("hub/hop.json", "{}");
    let d = t.path().join("hub/hops/feat/x");
    std::fs::create_dir_all(&d).unwrap();
    let o = run(&t).conf("set left \"#{repo}\"").cwd(&d).go();
    assert_eq!(o.plain.trim(), "hub");
}

#[test]
fn linked_worktree_uses_parent_repo_name() {
    let t = Tmp::new();
    let d = repo(&t, "parent");
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "feat"]);
    let wt = d.join(".wt/x");
    let o = run(&t).conf("set left \"#{repo}:#{branch}\"").cwd(&wt).go();
    assert_eq!(o.plain.trim(), "parent:feat");
}

#[test]
fn detached_head_omits_branch() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    git(&d, &["checkout", "-q", "--detach"]);
    let o = run(&t).conf("set left \"#{repo}:#{branch}\"").cwd(&d).go();
    assert_eq!(o.plain.trim(), "proj");
}
