#![cfg(unix)]
//! `--action use` with the `instance` token: two instances whose accounts
//! share an `auth_index`, picked from the menu. Fake servers, synthetic
//! accounts only.

mod fake;

use bar_cliproxyapi::switch::instance_id;
use fake::{pool, Server};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const KEY: &str = "instance-test-key";

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-inst-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Home(d)
    }
    fn file(&self, name: &str, text: &str) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        p
    }
    fn conf(&self, origins: &[String]) -> std::path::PathBuf {
        let key = self.file("key", &format!("{KEY}\n"));
        let text: String = origins
            .iter()
            .map(|o| format!("instance {o}\n key-file {}\n", key.display()))
            .collect();
        self.file("c.conf", &text)
    }
    fn run(&self, conf: &std::path::Path, args: &[&str], stdin: &str) -> Output {
        let mut ch = Command::new(BIN)
            .args(args)
            .env("BAR_CLIPROXYAPI_CONFIG", conf)
            .env("BAR_STATE_DIR", self.0.join("state"))
            .env_remove("ANTHROPIC_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        ch.wait_with_output().unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn prio(state: &Arc<Mutex<Value>>, name: &str) -> i64 {
    state
        .lock()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == format!("{name}.json"))
        .unwrap()["priority"]
        .as_i64()
        .unwrap_or(0)
}

/// Two accounts per instance; both instances list `ix-a` and `ix-b`.
fn fleet(who: &str) -> Value {
    json!([
        {"name":"a.json","auth_index":"ix-a","type":"claude","email":format!("{who}a@corp.test")},
        {"name":"b.json","auth_index":"ix-b","type":"claude","email":format!("{who}b@corp.test"),"priority":2},
    ])
}

struct Two {
    h: Home,
    conf: std::path::PathBuf,
    s: [Server; 2],
    st: [Arc<Mutex<Value>>; 2],
}

/// Two instances with colliding indexes, both listings cached.
fn two(name: &str) -> Two {
    let (s1, st1) = pool(fleet("p"), KEY);
    let (s2, st2) = pool(fleet("q"), KEY);
    let h = Home::new(name);
    let conf = h.conf(&[s1.origin(), s2.origin()]);
    for s in [&s1, &s2] {
        let input =
            json!({"session":{"model_id":"claude-x"},"env":{"ANTHROPIC_BASE_URL":s.origin()}});
        assert!(h.run(&conf, &[], &input.to_string()).status.success());
    }
    Two {
        h,
        conf,
        s: [s1, s2],
        st: [st1, st2],
    }
}

impl Two {
    fn patches(&self) -> [usize; 2] {
        [self.s[0].patches().len(), self.s[1].patches().len()]
    }
    fn act(&self, tokens: Value) -> Output {
        let a = json!({"action":"use","tokens":tokens}).to_string();
        self.h.run(&self.conf, &["--action", "use"], &a)
    }
    /// The `use` item for `index` in instance `n`'s menu group.
    fn menu_item(&self, n: usize, index: &str) -> Value {
        let o = self.h.run(&self.conf, &["--menu"], "{}");
        assert!(o.status.success(), "{}", err(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        let gid = format!("claude@{}", instance_id(&self.s[n].origin()));
        v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == gid)
            .unwrap_or_else(|| panic!("no group {gid} in {v}"))["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["tokens"]["account"] == index)
            .unwrap_or_else(|| panic!("no item {index} in {gid}"))
            .clone()
    }
}

#[test]
fn a_menu_pick_switches_only_its_own_instance() {
    let t = two("menu");
    for (n, want) in [(1, [0, 1]), (0, [1, 1])] {
        let item = t.menu_item(n, "ix-a");
        assert_eq!(item["action"], "use");
        assert_eq!(
            item["tokens"],
            json!({"account":"ix-a","instance":instance_id(&t.s[n].origin()),"provider":"claude"})
        );
        let o = t.act(item["tokens"].clone());
        assert!(o.status.success(), "{}", err(&o));
        assert_eq!(t.patches(), want);
        assert_eq!(prio(&t.st[n], "a"), 3);
    }
    // each change went to its own proxy only
    assert_eq!(prio(&t.st[0], "b"), 2);
    assert_eq!(prio(&t.st[1], "b"), 2);
}

#[test]
fn without_the_instance_token_a_shared_index_is_still_refused() {
    let t = two("omit");
    let o = t.act(json!({"account":"ix-a","provider":"claude"}));
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("several instances"), "{}", err(&o));
    assert_eq!(t.patches(), [0, 0]);
}

#[test]
fn without_the_instance_token_a_unique_index_finds_its_instance() {
    let (s1, st1) = pool(fleet("p"), KEY);
    let (s2, st2) = pool(
        json!([
            {"name":"z.json","auth_index":"ix-z","type":"claude","email":"z@corp.test"},
            {"name":"y.json","auth_index":"ix-y","type":"claude","email":"y@corp.test","priority":1},
        ]),
        KEY,
    );
    let h = Home::new("unique");
    let conf = h.conf(&[s1.origin(), s2.origin()]);
    for s in [&s1, &s2] {
        let input =
            json!({"session":{"model_id":"claude-x"},"env":{"ANTHROPIC_BASE_URL":s.origin()}});
        assert!(h.run(&conf, &[], &input.to_string()).status.success());
    }
    let a = json!({"action":"use","tokens":{"account":"ix-z","provider":"claude"}});
    let o = h.run(&conf, &["--action", "use"], &a.to_string());
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(prio(&st2, "z"), 2);
    assert_eq!(prio(&st1, "a"), 0);
    assert!(s1.patches().is_empty());
}

#[test]
fn a_bad_instance_token_is_refused_not_guessed() {
    let t = two("bad");
    let before = t.s[0].count() + t.s[1].count();
    for inst in ["0123456789abcdef", "x", "", "0123456789ABCDEFX"] {
        let o = t.act(json!({"account":"ix-a","instance":inst,"provider":"claude"}));
        assert_eq!(o.status.code(), Some(1), "{inst}");
    }
    // an instance that does not list the account is refused too
    let (s3, st3) = pool(
        json!([{"name":"z.json","auth_index":"ix-z","type":"claude","email":"z@corp.test"}]),
        KEY,
    );
    let conf = t.h.conf(&[t.s[0].origin(), t.s[1].origin(), s3.origin()]);
    let a = json!({"action":"use","tokens":{"account":"ix-a","instance":instance_id(&s3.origin()),"provider":"claude"}});
    let o = t.h.run(&conf, &["--action", "use"], &a.to_string());
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("no such account"), "{}", err(&o));
    assert!(s3.patches().is_empty());
    assert_eq!(prio(&st3, "z"), 0);
    assert_eq!(t.s[0].count() + t.s[1].count(), before);
}
