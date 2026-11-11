//! `--dry-run` on the writing subcommands: the plan as data (or text),
//! exit 0, and no file created, changed or removed anywhere under the
//! test's config, state, data and working dirs.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use render_fx::Tmp;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    /// The plan: stdout is one JSON document with the plan's fields.
    fn plan(&self, command: &str) -> Value {
        assert_eq!(self.code, 0, "{} {}", self.out, self.err);
        let v: Value =
            serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out));
        assert_eq!(v["dry_run"], true, "{v}");
        assert_eq!(v["command"], command, "{v}");
        for c in v["changes"].as_array().unwrap_or_else(|| panic!("{v}")) {
            assert!(
                ["create", "update", "remove"].contains(&c["action"].as_str().unwrap_or("")),
                "{c}"
            );
            assert!(c["path"].as_str().is_some_and(|p| !p.is_empty()), "{c}");
        }
        v
    }
}

/// `bar <args>` with every config, state and PATH location under `t`.
fn bar(t: &Tmp, args: &[&str], stdin: &str) -> Run {
    let (iso, cwd) = common::isolation(t.path());
    let o = Command::cargo_bin("bar")
        .unwrap()
        .envs(iso)
        .current_dir(cwd)
        .args(args)
        .write_stdin(stdin)
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// Every file, dir and symlink under `root`, with the files' content
/// and the links' targets (links are not followed).
fn tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            let ft = e.file_type().unwrap();
            if ft.is_symlink() {
                let to = std::fs::read_link(&p).unwrap();
                out.insert(rel, Some(to.to_string_lossy().into_owned().into_bytes()));
            } else if ft.is_dir() {
                out.insert(rel, None);
                todo.push(p);
            } else {
                out.insert(rel, Some(std::fs::read(&p).unwrap()));
            }
        }
    }
    out
}

/// Runs `args` and checks nothing under `t` changed.
fn dry(t: &Tmp, args: &[&str], stdin: &str) -> Run {
    common::isolation(t.path());
    let before = tree(t.path());
    let r = bar(t, args, stdin);
    let after = tree(t.path());
    let changed: Vec<_> = before
        .keys()
        .chain(after.keys())
        .filter(|k| before.get(*k) != after.get(*k))
        .collect();
    assert!(changed.is_empty(), "{args:?} wrote {changed:?}");
    r
}

fn only_change(v: &Value) -> &Value {
    let c = v["changes"].as_array().unwrap();
    assert_eq!(c.len(), 1, "{v}");
    &c[0]
}

fn conf(t: &Tmp) -> PathBuf {
    t.path().join("xdg/bar/bar.conf")
}

/// A plan path equals `p` component-wise (Windows prints `\`, the
/// expected paths are joined with `/`).
fn is_path(v: &Value, p: &Path) -> bool {
    v.as_str().is_some_and(|s| Path::new(s) == p)
}

#[test]
fn init_dry_run_plans_the_file() {
    let t = Tmp::new();
    let r = dry(&t, &["init", "--dry-run", "--format", "json"], "");
    let v = r.plan("init");
    let c = only_change(&v);
    assert_eq!(c["action"], "create", "{v}");
    assert!(is_path(&c["path"], &conf(&t)), "{v}");
    assert_eq!(v["preset"], hop_bar::presets::DEFAULT);
    // text: what it would do, and that nothing was written
    let r = dry(&t, &["init", "--dry-run"], "");
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out.lines().any(|l| l
            .strip_prefix("would create ")
            .is_some_and(|p| Path::new(p) == conf(&t))),
        "{}",
        r.out
    );
    assert!(r.out.contains("dry run: nothing written"), "{}", r.out);
}

#[test]
fn init_dry_run_reports_the_same_refusals() {
    let t = Tmp::new();
    assert_eq!(bar(&t, &["init", "--preset", "minimal"], "").code, 0);
    let r = dry(
        &t,
        &[
            "init",
            "--preset",
            "minimal",
            "--dry-run",
            "--format",
            "json",
        ],
        "",
    );
    assert_eq!(r.plan("init")["changes"], serde_json::json!([]));
    let r = dry(
        &t,
        &["init", "--preset", "full", "--dry-run", "--format", "json"],
        "",
    );
    assert_eq!(r.code, 4, "{}", r.out);
    assert_eq!(
        serde_json::from_str::<Value>(&r.out).unwrap()["code"],
        "CONFLICT"
    );
}

#[test]
fn install_dry_run_plans_settings_without_consent() {
    let t = Tmp::new();
    for extra in [&[][..], &["--yes"]] {
        let mut args = vec!["install", "--project", "--dry-run", "--format", "json"];
        args.extend_from_slice(extra);
        let r = dry(&t, &args, "");
        let v = r.plan("install");
        let settings = t.path().join("neutral-cwd/.claude/settings.json");
        let c = v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| is_path(&c["path"], &settings))
            .unwrap_or_else(|| panic!("{v}"));
        assert_eq!(c["action"], "create", "{v}");
        assert!(v["diff"].as_str().unwrap().contains("statusLine"), "{v}");
    }
    // once installed there is nothing to change
    assert_eq!(bar(&t, &["install", "--project", "--yes"], "").code, 0);
    let r = dry(
        &t,
        &["install", "--project", "--dry-run", "--format", "json"],
        "",
    );
    assert_eq!(r.plan("install")["changes"], serde_json::json!([]));
    let r = dry(&t, &["install", "--project", "--dry-run"], "");
    assert!(r.out.contains("dry run: nothing written"), "{}", r.out);
}

#[test]
fn import_dry_run_plans_the_write_and_backup() {
    let t = Tmp::new();
    let args = [
        "config",
        "import",
        "-",
        "--replace",
        "--dry-run",
        "--format",
        "json",
    ];
    let r = dry(&t, &args, "set gap 4\n");
    let v = r.plan("config import");
    let c = only_change(&v);
    assert_eq!(c["action"], "create", "{v}");
    assert!(is_path(&c["path"], &conf(&t)), "{v}");
    assert!(v["diff"].as_str().unwrap().contains("+set gap 4"), "{v}");
    t.write("xdg/bar/bar.conf", "set gap 2\n");
    let r = dry(&t, &args, "set gap 4\n");
    let v = r.plan("config import");
    assert_eq!(only_change(&v)["action"], "update", "{v}");
    assert_eq!(only_change(&v)["backup"], true, "{v}");
    let r = dry(&t, &args, "set gap 2\n");
    assert_eq!(r.plan("config import")["changes"], serde_json::json!([]));
    // a broken file is refused as without --dry-run
    let r = dry(&t, &args, "set no-such-key 1\n");
    assert_eq!(r.code, 2, "{}", r.out);
}

#[test]
fn export_dry_run_plans_the_file() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\n");
    let out = t.path().join("neutral-cwd/out.conf");
    let o = out.display().to_string();
    let r = dry(
        &t,
        &["config", "export", &o, "--dry-run", "--format", "json"],
        "",
    );
    let v = r.plan("config export");
    assert_eq!(only_change(&v)["action"], "create", "{v}");
    assert_eq!(only_change(&v)["path"], o);
    std::fs::write(&out, "old\n").unwrap();
    let r = dry(
        &t,
        &["config", "export", &o, "--dry-run", "--format", "json"],
        "",
    );
    assert_eq!(r.code, 4, "{}", r.out);
    let r = dry(
        &t,
        &[
            "config",
            "export",
            &o,
            "--force",
            "--dry-run",
            "--format",
            "json",
        ],
        "",
    );
    assert_eq!(only_change(&r.plan("config export"))["action"], "update");
}

#[test]
fn toggle_dry_run_plans_the_view() {
    let t = Tmp::new();
    let r = dry(&t, &["toggle", "5h", "--dry-run", "--format", "json"], "");
    let v = r.plan("toggle");
    let c = only_change(&v);
    assert_eq!(c["action"], "create", "{v}");
    assert_eq!(c["current"], "bar", "{v}");
    assert_eq!(c["proposed"], "pct", "{v}");
    assert_eq!(v["key"], "5h");
    assert_eq!(v["view"], "pct");
    let r = dry(
        &t,
        &["toggle", "5h", "bar", "--dry-run", "--format", "json"],
        "",
    );
    assert_eq!(r.plan("toggle")["changes"], serde_json::json!([]));
    assert_eq!(bar(&t, &["toggle", "5h"], "").code, 0);
    let r = dry(
        &t,
        &["toggle", "5h", "pct", "--dry-run", "--format", "json"],
        "",
    );
    assert_eq!(r.plan("toggle")["changes"], serde_json::json!([]));
    let r = dry(&t, &["toggle", "5h", "--dry-run", "--format", "json"], "");
    let v = r.plan("toggle");
    assert_eq!(only_change(&v)["action"], "remove", "{v}");
    assert_eq!(v["view"], "bar");
    let r = dry(&t, &["toggle", "5h", "--dry-run"], "");
    assert!(r.out.contains("would remove"), "{}", r.out);
    assert!(r.out.contains("pct -> bar"), "{}", r.out);
    // an unknown key is refused as without --dry-run
    let r = dry(&t, &["toggle", "no-such-view", "--dry-run"], "");
    assert_eq!(r.code, 2, "{}", r.err);
}

/// Ages a dir past the 30 days after which other sessions' toggle dirs
/// are pruned (Unix: Windows cannot open a dir to set its time).
#[cfg(unix)]
fn age(dir: &Path) {
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 86_400);
    let f = std::fs::File::open(dir).unwrap();
    f.set_modified(long_ago).unwrap();
}

/// The `remove` changes of a plan, as paths.
fn removals(v: &Value) -> Vec<PathBuf> {
    let c = v["changes"].as_array().unwrap();
    c.iter()
        .filter(|c| c["action"] == "remove")
        .map(|c| PathBuf::from(c["path"].as_str().unwrap()))
        .collect()
}

#[cfg(unix)]
#[test]
fn session_toggle_dry_run_lists_the_pruned_dirs() {
    const A: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
    const B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";
    const C: &str = "0a1b2c3d-0000-4000-8000-00000000000c";
    let t = Tmp::new();
    let toggles = t.path().join("state/toggles");
    for s in [B, C] {
        assert_eq!(
            bar(&t, &["toggle", &format!("5h?sid={s}"), "pct"], "").code,
            0
        );
    }
    let (old, fresh) = (toggles.join(B), toggles.join(C));
    age(&old);
    let a = format!("5h?sid={A}");
    // the set form and the flip form both prune; a set that changes
    // nothing still does
    for args in [
        vec![a.as_str(), "pct"],
        vec![a.as_str()],
        vec![a.as_str(), "bar"],
    ] {
        let mut full = vec!["toggle"];
        full.extend(&args);
        full.extend(["--dry-run", "--format", "json"]);
        let v = dry(&t, &full, "").plan("toggle");
        assert_eq!(removals(&v), vec![old.clone()], "{args:?} {v}");
    }
    let r = dry(&t, &["toggle", &a, "pct", "--dry-run"], "");
    assert!(
        r.out.contains(&format!("would remove {}", old.display())),
        "{}",
        r.out
    );
    // the real run removes exactly what the plan listed
    let v = dry(
        &t,
        &["toggle", &a, "pct", "--dry-run", "--format", "json"],
        "",
    )
    .plan("toggle");
    let before = tree(t.path());
    assert_eq!(bar(&t, &["toggle", &a, "pct"], "").code, 0);
    let after = tree(t.path());
    let gone: Vec<PathBuf> = before
        .keys()
        .filter(|k| !after.contains_key(*k))
        .map(|k| t.path().join(k))
        .filter(|p| p.parent() == Some(toggles.as_path()))
        .collect();
    assert_eq!(gone, removals(&v));
    assert!(!old.exists() && fresh.exists());
    // nothing stale left: nothing to remove
    let v = dry(&t, &["toggle", &a, "--dry-run", "--format", "json"], "").plan("toggle");
    assert!(removals(&v).is_empty(), "{v}");
}

#[test]
fn prices_update_dry_run_fetches_and_writes_nothing() {
    let t = Tmp::new();
    // curl fails in the test env: a dry run that fetched would fail
    let r = dry(
        &t,
        &["prices", "update", "--dry-run", "--format", "json"],
        "",
    );
    let v = r.plan("prices update");
    let c = only_change(&v);
    assert_eq!(c["action"], "create", "{v}");
    assert!(c["path"].as_str().unwrap().ends_with("models.json"), "{v}");
    let out = t.path().join("neutral-cwd/prices.json");
    let o = out.display().to_string();
    let r = dry(
        &t,
        &[
            "prices",
            "update",
            "--out",
            &o,
            "--dry-run",
            "--format",
            "json",
        ],
        "",
    );
    assert_eq!(only_change(&r.plan("prices update"))["path"], o);
}

#[test]
fn web_plugins_dry_run_plans_the_output_file() {
    let t = Tmp::new();
    let out = t.path().join("neutral-cwd/plugins.json");
    let o = out.display().to_string();
    let args = ["web", "plugins", "-o", &o, "--dry-run", "--format", "json"];
    let r = dry(&t, &args, "");
    let v = r.plan("web plugins");
    assert_eq!(only_change(&v)["action"], "create", "{v}");
    assert_eq!(only_change(&v)["path"], o);
    std::fs::write(&out, "{}\n").unwrap();
    let r = dry(&t, &args, "");
    assert_eq!(r.code, 4, "{}", r.out);
}

#[test]
fn help_json_declares_dry_run() {
    let t = Tmp::new();
    let r = bar(&t, &["--help", "--format", "json"], "");
    let doc: Value = serde_json::from_str(&r.out).unwrap();
    for path in [
        &["bar", "init"][..],
        &["bar", "install"],
        &["bar", "toggle"],
        &["bar", "prices", "update"],
        &["bar", "config", "import"],
        &["bar", "config", "export"],
        &["bar", "web", "plugins"],
    ] {
        let c = doc["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == serde_json::json!(path))
            .unwrap_or_else(|| panic!("{path:?}"));
        assert_eq!(c["dry_run_supported"], true, "{path:?}");
    }
}

#[cfg(unix)]
#[test]
fn session_toggle_dry_run_refuses_a_symlinked_toggles_dir() {
    let t = Tmp::new();
    let elsewhere = t.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(t.path().join("state")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, t.path().join("state/toggles")).unwrap();
    let a = "5h?sid=0a1b2c3d-0000-4000-8000-00000000000a";
    let real = bar(&t, &["toggle", a, "pct"], "");
    assert_eq!(real.code, 2, "{}", real.err);
    let r = dry(&t, &["toggle", a, "pct", "--dry-run"], "");
    assert_eq!(r.code, real.code, "{} {}", r.out, r.err);
    assert!(r.err.contains("is not a directory"), "{}", r.err);
}

/// The files under `t` the run of `args` removed.
fn removed_by(t: &Tmp, args: &[&str], stdin: &str) -> Vec<PathBuf> {
    let before = tree(t.path());
    let r = bar(t, args, stdin);
    assert_eq!(r.code, 0, "{args:?} {} {}", r.out, r.err);
    let after = tree(t.path());
    let mut gone: Vec<PathBuf> = before
        .keys()
        .filter(|k| !after.contains_key(*k))
        .map(|k| t.path().join(k))
        .collect();
    gone.sort();
    gone
}

fn sorted(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    v.sort();
    v
}

#[test]
fn import_dry_run_lists_the_backups_rotated_out() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 2\n");
    // five backups kept: a sixth pushes the oldest out
    for i in 1..=5 {
        t.write(
            &format!("xdg/bar/bar.conf.bak-100000000{i}-000000000-1"),
            "old\n",
        );
    }
    let oldest = t.path().join("xdg/bar/bar.conf.bak-1000000001-000000000-1");
    let mut args = vec!["config", "import", "-", "--replace", "--yes"];
    let dry_args: Vec<&str> = args
        .iter()
        .copied()
        .chain(["--dry-run", "--format", "json"])
        .collect();
    let v = dry(&t, &dry_args, "set gap 4\n").plan("config import");
    let rm = removals(&v);
    assert_eq!(rm, vec![oldest], "{v}");
    let r = dry(&t, &dry_args[..dry_args.len() - 2], "set gap 4\n");
    assert!(r.out.contains("would remove"), "{}", r.out);
    args.push("--format");
    args.push("json");
    assert_eq!(removed_by(&t, &args, "set gap 4\n"), sorted(rm));
}

#[test]
fn export_dry_run_lists_the_backups_rotated_out() {
    let t = Tmp::new();
    t.write("xdg/bar/bar.conf", "set gap 4\n");
    t.write("neutral-cwd/out.conf", "old\n");
    for i in 1..=6 {
        t.write(
            &format!("neutral-cwd/out.conf.bak-100000000{i}-000000000-1"),
            "old\n",
        );
    }
    let o = t.path().join("neutral-cwd/out.conf").display().to_string();
    let args = ["config", "export", &o, "--force"];
    let dry_args = [&args[..], &["--dry-run", "--format", "json"]].concat();
    let rm = removals(&dry(&t, &dry_args, "").plan("config export"));
    assert_eq!(rm.len(), 2, "{rm:?}");
    assert_eq!(removed_by(&t, &args, ""), sorted(rm));
}

#[test]
fn install_dry_run_lists_the_backups_rotated_out() {
    let t = Tmp::new();
    let install: &[&str] = &["install", "--project", "--yes"];
    let uninstall: &[&str] = &["install", "--project", "--uninstall", "--yes"];
    // each write after the first backs the old settings up: five backups
    for args in [install, uninstall, install, uninstall, install, uninstall] {
        assert_eq!(bar(&t, args, "").code, 0, "{args:?}");
    }
    let dry_args = [install, &["--dry-run", "--format", "json"]].concat();
    let rm = removals(&dry(&t, &dry_args, "").plan("install"));
    assert_eq!(rm.len(), 1, "{rm:?}");
    assert_eq!(removed_by(&t, install, ""), sorted(rm));
}
