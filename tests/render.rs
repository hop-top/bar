use assert_cmd::Command;
use hop_bar::placeholders::account::glob_match;
use hop_bar::placeholders::repo::remote_url;
use std::path::{Path, PathBuf};
use std::process::Command as Std;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-render-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p.canonicalize().unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, body: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

struct Out {
    raw: String,
    plain: String,
}

struct Run<'a> {
    home: &'a Tmp,
    conf: Option<&'a str>,
    env: Vec<(&'a str, String)>,
    stdin: String,
}

fn run(home: &Tmp) -> Run<'_> {
    Run {
        home,
        conf: None,
        env: vec![],
        stdin: "{}".into(),
    }
}

impl<'a> Run<'a> {
    fn conf(mut self, c: &'a str) -> Self {
        self.conf = Some(c);
        self
    }
    fn env(mut self, k: &'a str, v: &str) -> Self {
        self.env.push((k, v.into()));
        self
    }
    fn stdin(mut self, s: &str) -> Self {
        self.stdin = s.into();
        self
    }
    fn cwd(self, d: &Path) -> Self {
        let s = format!(r#"{{"workspace":{{"current_dir":"{}"}}}}"#, d.display());
        self.stdin(&s)
    }
    fn go(self) -> Out {
        let h = self.home.path();
        if let Some(c) = self.conf {
            self.home.write("xdg/bar/bar.conf", c);
        }
        let mut cmd = Command::cargo_bin("bar").unwrap();
        cmd.arg("render")
            .env("HOME", h)
            .env("XDG_CONFIG_HOME", h.join("xdg"))
            .env("COLUMNS", "200")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("ANTHROPIC_API_KEY");
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let o = cmd.write_stdin(self.stdin).assert().success();
        let raw = String::from_utf8(o.get_output().stdout.clone()).unwrap();
        Out {
            plain: strip(&raw),
            raw,
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Std::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn repo(t: &Tmp, name: &str) -> PathBuf {
    let d = t.path().join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "x"]);
    d
}

fn account_json(email: &str) -> String {
    format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#)
}

const ACCOUNT_CONF: &str = "set right \"#{account}\"\n\
account \"*innoprofits*\" innoprofits 141\n\
account \"*@ideacrafters.com\" ideacrafters 75\n";

#[test]
fn account_first_match_wins() {
    let t = Tmp::new();
    t.write(
        ".claude.json",
        &account_json("jad+innoprofits@ideacrafters.com"),
    );
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "innoprofits");
    assert!(o.raw.contains("38;5;141m"), "{:?}", o.raw);
}

#[test]
fn unknown_account_shows_email() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("someone@example.org"));
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "someone@example.org");
}

#[test]
fn account_reads_claude_config_dir() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("home@example.org"));
    t.write("cc/.claude.json", &account_json("cfg@ideacrafters.com"));
    let cc = t.path().join("cc");
    let o = run(&t)
        .conf(ACCOUNT_CONF)
        .env("CLAUDE_CONFIG_DIR", cc.to_str().unwrap())
        .go();
    assert_eq!(o.plain.trim(), "ideacrafters");
}

#[test]
fn account_api_key_marker() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(ACCOUNT_CONF)
        .env("ANTHROPIC_API_KEY", "sk-x")
        .go();
    assert_eq!(o.plain.trim(), "api-key");
    let o = run(&t).conf(ACCOUNT_CONF).go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn account_glob_star_forms() {
    assert!(glob_match("*", "anything"));
    assert!(glob_match("*", ""));
    assert!(glob_match("a*", "abc"));
    assert!(!glob_match("a*", "ba"));
    assert!(glob_match("*c", "abc"));
    assert!(!glob_match("*c", "abd"));
    assert!(glob_match("*b*", "abc"));
    assert!(glob_match("a*c*e", "abcde"));
    assert!(!glob_match("a*c*e", "abde"));
    assert!(glob_match("a@b.io", "a@b.io"));
    assert!(!glob_match("a@b.io", "a@b.ioo"));
    assert!(glob_match("**x", "x"));
}

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

const MODEL_CONF: &str = "set left \"#{model}#{effort}\"";

#[test]
fn model_strips_claude_prefix() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Claude Opus 5.5"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus 5.5");
}

#[test]
fn effort_brackets_level() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Opus 5.5"},"effort":{"level":"medium"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus 5.5[medium]");
}

#[test]
fn effort_falls_back_to_settings_per_model_then_global() {
    let t = Tmp::new();
    t.write(
        ".claude/settings.json",
        r#"{"effortLevel":"high","modelSettings":{"m-a":{"effortLevel":"low"}}}"#,
    );
    let go = |id: &str| {
        run(&t)
            .conf(MODEL_CONF)
            .stdin(&format!(
                r#"{{"model":{{"id":"{id}","display_name":"M"}}}}"#
            ))
            .go()
            .plain
    };
    assert_eq!(go("m-a").trim(), "M[low]");
    assert_eq!(go("m-b").trim(), "M[high]");
}

#[test]
fn model_and_effort_link_separately() {
    let t = Tmp::new();
    let o = run(&t)
        .conf(MODEL_CONF)
        .stdin(r#"{"model":{"display_name":"Opus"},"effort":{"level":"high"}}"#)
        .go();
    // two independent styled spans, so a later task can link each on its own
    assert!(
        o.raw.contains("\x1b[2mOpus\x1b[22m\x1b[2m[high]\x1b[22m"),
        "{:?}",
        o.raw
    );
}

#[test]
fn ctx_on_reversed_ramp() {
    let t = Tmp::new();
    for (pct, color) in [
        (0, 46),
        (9, 46),
        (10, 82),
        (50, 226),
        (90, 202),
        (91, 196),
        (100, 196),
    ] {
        let o = run(&t)
            .conf("set left \"#{ctx}\"")
            .stdin(&format!(
                r#"{{"context_window":{{"used_percentage":{pct}}}}}"#
            ))
            .go();
        assert_eq!(o.plain.trim(), format!("ctx {pct}%"));
        assert!(
            o.raw.contains(&format!("\x1b[38;5;{color}m{pct}%")),
            "{pct}: {:?}",
            o.raw
        );
    }
}

#[test]
fn ctx_missing_omitted() {
    let t = Tmp::new();
    let o = run(&t).conf("set left \"#{ctx}\"").go();
    assert_eq!(o.plain.trim(), "");
}

#[test]
fn segment_with_only_empty_placeholders_drops_separator() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{model} │ #{ctx} │ #{effort}\"")
        .stdin(r#"{"model":{"display_name":"Opus"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus");
}

#[test]
fn unknown_placeholder_omitted() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{model} │ #{nonesuch} │ #{prs}\"")
        .stdin(r#"{"model":{"display_name":"Opus"}}"#)
        .go();
    assert_eq!(o.plain.trim(), "Opus");
}

#[test]
fn invalid_config_prints_short_error_and_exits_zero() {
    let t = Tmp::new();
    let o = run(&t).conf("bogus directive\n").go();
    assert_eq!(o.raw, "bar: config error, run bar check\n");
}

#[test]
fn missing_config_uses_builtin_default() {
    let t = Tmp::new();
    t.write(".claude.json", &account_json("me@example.org"));
    let d = repo(&t, "proj");
    let o = run(&t)
        .cwd(&d)
        .stdin(&format!(
            r#"{{"workspace":{{"current_dir":"{}"}},"model":{{"display_name":"Claude Opus"}},"context_window":{{"used_percentage":38}}}}"#,
            d.display()
        ))
        .go();
    assert!(o.plain.contains("Opus"), "{:?}", o.plain);
    assert!(o.plain.contains("ctx 38%"));
    assert!(o.plain.contains("proj:main"));
    assert!(o.plain.trim_end().ends_with("me@example.org"));
    assert_eq!(o.plain.trim().lines().count(), 1);
}
