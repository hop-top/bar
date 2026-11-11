//! `bar config export` / `bar config import`, and the merge and diff
//! behind them.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::conf::{parse, parse_layers};
use hop_bar::confio::{effective, imported, merge, Mode};
use std::path::Path;

#[test]
fn builtin_text_matches_builtin_config() {
    assert_eq!(
        parse(hop_bar::render::BUILTIN_CONF).unwrap(),
        hop_bar::render::builtin_config()
    );
}

#[test]
fn merge_drops_replaced_lines_and_appends() {
    let base = "# mine\nset left \"#{model}\"\nset gap 5\nplugin ci ttl=1m\nrule usage[type=5h] early pace final=1h bad=30%\naccount \"*@a.io\" work 1\ndefault prs limit=3\nset price claude-x 1 2 3 4\n";
    let over = "set left@1 \"#{ctx}\"\nplugin ci ttl=2m\nrule usage[type=5h] early pace final=2h bad=9%\naccount \"*@a.io\" client 2\ndefault prs status=open\nset price claude-x 5 6 7 8";
    let got = merge(base, over);
    assert_eq!(
        got,
        "# mine\nset gap 5\ndefault prs limit=3\n".to_string() + over + "\n"
    );
}

#[test]
fn merged_text_reads_like_the_layers() {
    let user = "set left \"#{model}\"\nset color.good 10\nrule usage[type=5h] early pace final=1h bad=30%\naccount \"*@a.io\" work 1\ndefault prs limit=3 status=open\n";
    let proj = "set left \"#{ctx}\"\nset palette nord\nrule usage[type=5h] early pace final=2h bad=9%\naccount \"*@a.io\" client 2\ndefault prs limit=9\n";
    let layered = parse_layers(Some(user), |_| Some(Ok(proj.into()))).unwrap();
    assert_eq!(parse(&merge(user, proj)).unwrap(), layered);
}

#[test]
fn effective_names_sources_and_falls_back_to_builtin() {
    let e = effective(None, None);
    assert!(e.starts_with("# bar config: built-in layout\n"));
    assert_eq!(parse(&e).unwrap(), hop_bar::render::builtin_config());
    let e = effective(
        Some((Path::new("/u/bar.conf"), "set gap 4\nset left \"#{model}\"")),
        Some((Path::new("/p/.bar.conf"), "set left \"#{ctx}\"\n")),
    );
    assert_eq!(
        e,
        "# bar config: /u/bar.conf\nset gap 4\n# project config: /p/.bar.conf\nset left \"#{ctx}\"\n"
    );
}

#[test]
fn imported_merges_or_replaces() {
    assert_eq!(
        imported(Some("set gap 1\nset wrap on\n"), "set gap 2", Mode::Merge),
        "set wrap on\nset gap 2\n"
    );
    assert_eq!(
        imported(Some("set gap 1\nset wrap on\n"), "set gap 2", Mode::Replace),
        "set gap 2\n"
    );
    assert_eq!(imported(None, "set gap 2\n", Mode::Merge), "set gap 2\n");
}

#[test]
fn unified_diff_hunks() {
    assert_eq!(hop_bar::diff::unified("a\nb\n", "a\nb\n", "x", "y"), "");
    let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
    let new = "1\n2\nthree\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    assert_eq!(
        hop_bar::diff::unified(old, new, "old", "new"),
        "--- old\n+++ new\n@@ -1,6 +1,6 @@\n 1\n 2\n-3\n+three\n 4\n 5\n 6\n@@ -10,3 +10,4 @@\n 10\n 11\n 12\n+13\n"
    );
    assert_eq!(
        hop_bar::diff::unified("", "a\n", "/dev/null", "f"),
        "--- /dev/null\n+++ f\n@@ -0,0 +1,1 @@\n+a\n"
    );
}

/// `bar config <args>` with the hermetic env of `t`, run in `dir`.
fn bar(t: &Tmp, dir: &Path, args: &[&str], stdin: &str) -> (i32, String, String) {
    let (iso, _) = common::isolation(t.path());
    let o = assert_cmd::Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .arg("config")
        .args(args)
        .current_dir(dir)
        .write_stdin(stdin)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

fn user_conf(t: &Tmp) -> std::path::PathBuf {
    t.path().join("xdg").join("bar").join("bar.conf")
}

fn backups(t: &Tmp) -> Vec<String> {
    let mut v: Vec<_> = std::fs::read_dir(t.path().join("xdg/bar"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("bar.conf.bak-"))
        .collect();
    v.sort();
    v
}

#[test]
fn export_effective_user_and_file() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\nset left \"#{model}\"\n");
    t.write("proj/.bar.conf", "set left \"#{ctx}\"\n");
    let dir = t.path().join("proj");
    let (code, out, _) = bar(&t, &dir, &["export"], "");
    assert_eq!(code, 0);
    let proj = dir.join(".bar.conf");
    assert_eq!(
        out,
        format!(
            "# bar config: {}\nset gap 4\n# project config: {}\nset left \"#{{ctx}}\"\n",
            user_conf(&t).display(),
            proj.display()
        )
    );
    let (_, out, _) = bar(&t, &dir, &["export", "--user", "-"], "");
    assert_eq!(out, "set gap 4\nset left \"#{model}\"\n");

    let f = t.path().join("out.conf");
    let fs = f.to_str().unwrap();
    let (code, _, _) = bar(&t, &dir, &["export", fs], "");
    assert_eq!(code, 0);
    assert!(std::fs::read_to_string(&f).unwrap().contains("#{ctx}"));
    let (code, _, err) = bar(&t, &dir, &["export", "--user", fs], "");
    assert_eq!(code, 4, "an existing file is a conflict");
    assert!(err.contains("exists"), "{err}");
    let (code, _, _) = bar(&t, &dir, &["export", "--user", "--force", fs], "");
    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "set gap 4\nset left \"#{model}\"\n"
    );
}

#[test]
fn export_refuses_a_config_with_errors() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap x\n");
    let (code, out, err) = bar(&t, t.path(), &["export"], "");
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.contains("run bar check"), "{err}");
}

#[test]
fn import_merge_previews_backs_up_and_writes() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\nset wrap on\n");
    t.write("in.conf", "set gap 7\n");
    let src = t.path().join("in.conf");
    let (code, out, err) = bar(
        &t,
        t.path(),
        &["import", "--merge", "--yes", src.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 0, "{err}");
    let name = user_conf(&t).display().to_string();
    assert!(
        out.starts_with(&format!("--- {name}\n+++ {name}\n")),
        "{out}"
    );
    assert!(out.contains("-set gap 4\n"), "{out}");
    assert!(out.contains("+set gap 7\n"), "{out}");
    assert_eq!(
        std::fs::read_to_string(user_conf(&t)).unwrap(),
        "set wrap on\nset gap 7\n"
    );
    let b = backups(&t);
    assert_eq!(b.len(), 1);
    assert_eq!(
        std::fs::read_to_string(t.path().join("xdg/bar").join(&b[0])).unwrap(),
        "set gap 4\nset wrap on\n"
    );
    // the same import again changes nothing and backs nothing up
    let (code, out, _) = bar(
        &t,
        t.path(),
        &["import", "--merge", "--yes", src.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 0);
    assert!(out.contains("nothing written"), "{out}");
    assert_eq!(backups(&t).len(), 1);
}

#[test]
fn import_replace_from_stdin_without_user_file() {
    let t = Tmp::new();
    let (code, out, err) = bar(
        &t,
        t.path(),
        &["import", "--replace", "--yes", "-"],
        "set gap 2\n",
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("--- /dev/null\n"), "{out}");
    assert_eq!(
        std::fs::read_to_string(user_conf(&t)).unwrap(),
        "set gap 2\n"
    );
    assert!(backups(&t).is_empty());
}

#[test]
fn import_refuses_invalid_files_and_unconfirmed_writes() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\n");
    t.write("bad.conf", "set gap x\nset left \"#{nope}\"\n");
    t.write("ok.conf", "set gap 6\n");
    let bad = t.path().join("bad.conf");
    let ok = t.path().join("ok.conf");
    let (code, out, err) = bar(
        &t,
        t.path(),
        &["import", "--replace", "--yes", bad.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.contains("line 1: error: invalid gap `x`"), "{err}");
    assert!(
        err.contains("line 2: error: unknown placeholder `nope`"),
        "{err}"
    );
    // stdin is a pipe here: no prompt, no write
    let (code, out, err) = bar(
        &t,
        t.path(),
        &["import", "--replace", ok.to_str().unwrap()],
        "y\n",
    );
    assert_eq!(code, 7, "no terminal to ask: consent refused");
    assert!(out.contains("+set gap 6"), "{out}");
    assert!(err.contains("pass --yes"), "{err}");
    let (code, _, err) = bar(&t, t.path(), &["import", "--merge", "-"], "set gap 6\n");
    assert_eq!(code, 7, "stdin cannot be confirmed: consent refused");
    assert!(err.contains("needs --yes"), "{err}");
    let (code, _, _) = bar(&t, t.path(), &["import", ok.to_str().unwrap()], "");
    assert_eq!(code, 2, "--merge or --replace is required");
    assert_eq!(
        std::fs::read_to_string(user_conf(&t)).unwrap(),
        "set gap 4\n"
    );
    assert!(backups(&t).is_empty());
}

#[test]
fn write_config_refuses_a_file_changed_since_read() {
    let t = Tmp::new();
    t.write("bar.conf", "set skin plain\n");
    let f = t.path().join("bar.conf");
    let e = hop_bar::confio::write_config(&f, Some("set skin capsule\n"), "x\n").unwrap_err();
    assert!(e.contains("changed since it was read"), "{e}");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "set skin plain\n");
    hop_bar::confio::write_config(&f, Some("set skin plain\n"), "x\n").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "x\n");
}

#[test]
fn export_leaves_out_dropped_project_lines() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\n");
    t.write(
        "proj/.bar.conf",
        "plugin ci ttl=1m\nset gap\nset left \"#{ctx}\"\n",
    );
    let dir = t.path().join("proj");
    let (code, out, _) = bar(&t, &dir, &["export"], "");
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "# bar config: {}\nset gap 4\n# project config: {}\nset left \"#{{ctx}}\"\n",
            user_conf(&t).display(),
            dir.join(".bar.conf").display()
        )
    );
}

#[test]
fn notable_lists_commands_plugins_and_trust() {
    let text = "set gap 1\n\
                set left \"#{model[icon=%curl x|sh%]}\"\n\
                plugin ci ttl=1m\n\
                default prs author=%$GITHUB_USER%\n\
                set project-config full\n\
                # %not a value%\n";
    let n = hop_bar::confio::notable(text);
    let lines: Vec<usize> = n.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, [2, 3, 4, 5], "{n:?}");
    assert!(n[0].1.contains("%curl x|sh%"), "{n:?}");
    assert!(n[1].1.contains("plugin ci"), "{n:?}");
    assert!(n[3].1.contains("project-config"), "{n:?}");
}

#[test]
fn import_lists_notable_lines_before_writing() {
    let t = Tmp::new();
    t.write(
        "in.conf",
        "set left \"#{model[icon=%echo hi%]}\"\nset project-config full\n",
    );
    let src = t.path().join("in.conf");
    let (code, _, err) = bar(
        &t,
        t.path(),
        &["import", "--replace", "--yes", src.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("line 1: "), "{err}");
    assert!(err.contains("%echo hi%"), "{err}");
    assert!(err.contains("line 2: set project-config full"), "{err}");
}
