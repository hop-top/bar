//! Option `open` on `cwd`, `root` and `repo`: OSC 8 links to the
//! directory in the file manager or an editor.
mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

/// `p` percent-encoded the way links carry it (unreserved and `/` kept).
#[cfg(unix)]
fn enc(p: &std::path::Path) -> String {
    let mut out = String::new();
    for b in p.to_str().unwrap().bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn link_to(url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\")
}

#[cfg(unix)]
#[test]
fn cwd_links_in_each_editor_with_spaces_and_unicode() {
    let t = Tmp::new();
    let d = t.path().join("My Project/café 日本");
    std::fs::create_dir_all(&d).unwrap();
    let e = enc(&d);
    assert!(
        e.contains("My%20Project/caf%C3%A9%20%E6%97%A5%E6%9C%AC"),
        "{e}"
    );
    for (open, url) in [
        ("vscode", format!("vscode://file{e}/")),
        ("cursor", format!("cursor://file{e}/")),
        ("zed", format!("zed://file{e}/")),
    ] {
        let conf = format!("set left \"#{{cwd[base=1;open={open}]}}\"");
        let o = run(&t).conf(&conf).cwd(&d).go();
        assert_eq!(o.plain.trim(), "café 日本", "{open}");
        assert!(o.raw.contains(&link_to(&url)), "{open}: {:?}", o.raw);
        assert!(o.raw.contains("\x1b]8;;\x1b\\"), "{open}: link closed");
    }
}

#[cfg(unix)]
#[test]
fn files_links_name_this_host() {
    let t = Tmp::new();
    let d = t.path().join("a b");
    std::fs::create_dir_all(&d).unwrap();
    let o = run(&t).conf("set left \"#{cwd[open=files]}\"").cwd(&d).go();
    let at = o.raw.find("\x1b]8;;file://").expect("file link");
    let url = &o.raw[at + 5..];
    let url = &url[..url.find('\x1b').unwrap()];
    let tail = format!("{}/", enc(&d));
    assert!(url.ends_with(&tail), "{url}");
    let host = &url["file://".len()..url.len() - tail.len()];
    assert!(
        host.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)),
        "{host}"
    );
}

#[test]
fn without_open_the_output_is_unchanged() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let plain = run(&t)
        .conf("set left \"#{cwd} #{root[base=1]} #{repo}\"")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    let none = run(&t)
        .conf("set left \"#{cwd[open=none]} #{root[base=1;open=none]} #{repo[open=none]}\"")
        .cwd(&d)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(plain.plain.trim(), "~/proj proj proj");
    assert_eq!(plain.raw, none.raw);
    assert!(!plain.raw.contains("\x1b]8"), "{:?}", plain.raw);
}

#[cfg(unix)]
#[test]
fn abbreviated_paths_link_the_full_directory() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let sub = d.join("alpha/beta/gamma");
    std::fs::create_dir_all(&sub).unwrap();
    let o = run(&t)
        .conf("set left \"#{cwd[fish=1;depth=2;open=vscode]} #{root[fish=1;open=zed]}\"")
        .cwd(&sub)
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    let shown: Vec<&str> = o.plain.split_whitespace().collect();
    assert_eq!(shown[0], "…/b/gamma", "{:?}", o.raw);
    assert!(
        shown[1].ends_with("/proj") && shown[1].len() < d.to_str().unwrap().len(),
        "{:?}",
        o.raw
    );
    assert!(
        o.raw
            .contains(&link_to(&format!("vscode://file{}/", enc(&sub)))),
        "{:?}",
        o.raw
    );
    assert!(
        o.raw.contains(&link_to(&format!("zed://file{}/", enc(&d)))),
        "{:?}",
        o.raw
    );
}

#[cfg(unix)]
#[test]
fn root_links_the_top_level_from_a_subdirectory() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let sub = d.join("src/deep");
    std::fs::create_dir_all(&sub).unwrap();
    let o = run(&t)
        .conf("set left \"#{root[base=1;open=vscode]}\"")
        .cwd(&sub)
        // git config env makes the file reader step aside; root needs it
        .unset("GIT_CONFIG_NOSYSTEM")
        .go();
    assert_eq!(o.plain.trim(), "proj", "{:?}", o.raw);
    assert!(o
        .raw
        .contains(&link_to(&format!("vscode://file{}/", enc(&d)))));
}

#[cfg(unix)]
#[test]
fn repo_without_a_remote_links_the_directory_it_names() {
    let t = Tmp::new();
    let d = repo(&t, "parent");
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "feat"]);
    let o = run(&t)
        .conf("set left \"#{repo[open=zed]}\"")
        .cwd(&d.join(".wt/x"))
        .go();
    assert_eq!(o.plain.trim(), "parent");
    assert!(o.raw.contains(&link_to(&format!("zed://file{}/", enc(&d)))));

    t.write("hub/hop.json", "{}");
    let w = t.path().join("hub/hops/feat/x");
    std::fs::create_dir_all(&w).unwrap();
    let o = run(&t)
        .conf("set left \"#{repo[open=cursor]}\"")
        .cwd(&w)
        .go();
    assert_eq!(o.plain.trim(), "hub");
    let hub = t.path().join("hub");
    assert!(o
        .raw
        .contains(&link_to(&format!("cursor://file{}/", enc(&hub)))));
}

#[test]
fn repo_keeps_its_web_link_and_link_off_wins() {
    let t = Tmp::new();
    let d = repo(&t, "proj");
    let o = run(&t)
        .conf("set left \"#{repo[open=vscode;link=off]}\"")
        .cwd(&d)
        .go();
    assert_eq!(o.plain.trim(), "proj");
    assert!(!o.raw.contains("\x1b]8"), "{:?}", o.raw);

    git(&d, &["remote", "add", "origin", "git@github.com:o/r.git"]);
    let o = run(&t)
        .conf("set left \"#{repo[open=vscode]}\"")
        .cwd(&d)
        .go();
    assert!(o.raw.contains(&link_to("https://github.com/o/r")));
    assert!(!o.raw.contains("vscode://"));
}

#[cfg(unix)]
#[test]
fn subagent_rows_link_the_subagents_directory() {
    use hop_bar::{conf, render::ColdRunner, subagent};
    let t = Tmp::new();
    let wt = t.path().join("wt dir");
    std::fs::create_dir_all(&wt).unwrap();
    let input = format!(
        r#"{{"session_id":"s1","cwd":"/nonexistent/proj","columns":120,"tasks":[
        {{"id":"a1","type":"local_agent","status":"running","description":"d",
          "cwd":{}}}]}}"#,
        serde_json::to_string(wt.to_str().unwrap()).unwrap()
    );
    let p = subagent::parse(&input);
    let c = conf::parse("set subagent \"#{cwd[base=1;open=vscode]}\"\n").unwrap();
    let rows = subagent::rows(&p, &c, 0, &ColdRunner, None);
    assert_eq!(rows.len(), 1);
    assert!(rows[0]
        .1
        .contains(&link_to(&format!("vscode://file{}/", enc(&wt)))));
    assert!(!rows[0].1.contains("bar://"));
}

#[test]
fn check_validates_open() {
    let bad = hop_bar::check::check_text("set left \"#{cwd[open=finder]}\"\n", None, &[]);
    assert!(
        bad.issues.iter().any(|i| i.message.contains("open")),
        "{:?}",
        bad.issues
    );
    for ok in [
        "set left \"#{cwd[open=files]} #{root[open=zed]} #{repo[open=cursor]}\"\n",
        "set left \"#{cwd[open=vscode]} #{root[open=none]}\"\n",
    ] {
        let r = hop_bar::check::check_text(ok, None, &[]);
        assert!(
            !r.issues.iter().any(|i| i.message.contains("open")),
            "{ok}: {:?}",
            r.issues
        );
    }
    let r = hop_bar::check::check_text("set left \"#{branch[open=vscode]}\"\n", None, &[]);
    assert!(r.issues.iter().any(|i| i.message.contains("open")));
}

#[test]
fn web_preview_samples_carry_the_link() {
    let conf = "set left \"#{cwd[open=vscode]} #{root[base=1;open=files]} #{cwd}\"\n";
    let out = hop_bar::web::render(conf, "{}", 120, 0, "");
    assert!(
        out.contains(&link_to("vscode://file/home/you/src/bar/")),
        "{out:?}"
    );
    assert!(
        out.contains(&link_to("file:///home/you/src/bar/")),
        "{out:?}"
    );
    assert_eq!(out.matches("\x1b]8;;\x1b\\").count(), 2, "{out:?}");
}
