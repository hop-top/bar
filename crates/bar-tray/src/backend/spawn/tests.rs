use super::*;

fn strs(a: Vec<OsString>) -> Vec<String> {
    a.into_iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn argv() {
    assert_eq!(strs(state_args()), ["tray", "state", "--json"]);
    assert_eq!(strs(toggle_args("5h")), ["toggle", "5h"]);
    assert_eq!(strs(run_args("p.g.i", false, "")), ["tray", "run", "p.g.i"]);
    assert_eq!(
        strs(run_args("p.g.i", true, "ab12")),
        [
            "tray",
            "run",
            "p.g.i",
            "--confirmed",
            "--generation",
            "ab12"
        ]
    );
}

#[test]
fn id_stays_one_argument() {
    let a = run_args("a b; rm -rf /", false, "g");
    assert_eq!(a[2], "a b; rm -rf /");
    assert_eq!(a.len(), 5);
}

#[test]
fn bar_beside_tray() {
    let p = bar_path(Path::new("/opt/x/bar-tray"));
    assert_eq!(p.parent(), Some(Path::new("/opt/x")));
    assert_eq!(p.file_name().unwrap(), BAR_NAME);
}

#[test]
fn deadlines() {
    assert_eq!(deadline(Some(10)), Duration::from_secs(15));
    assert_eq!(deadline(Some(98)), Duration::from_secs(103));
    assert_eq!(deadline(Some(500)), MAX_LIMIT + GRACE);
    assert_eq!(deadline(Some(0)), MAX_LIMIT + GRACE);
    assert_eq!(deadline(None), MAX_LIMIT + GRACE);
}

#[test]
fn env_is_scrubbed() {
    let env = [
        ("PATH", "/usr/bin"),
        ("BAR_STATE_DIR", "/tmp/s"),
        ("XDG_STATE_HOME", ""),
        ("HOME", "/Users/x"),
        ("ANTHROPIC_API_KEY", "secret"),
    ]
    .map(|(k, v)| (OsString::from(k), OsString::from(v)));
    let kept: Vec<(String, String)> = scrub(env)
        .into_iter()
        .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
        .collect();
    assert_eq!(
        kept,
        [
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("BAR_STATE_DIR".to_string(), "/tmp/s".to_string())
        ]
    );
}

#[test]
fn notes() {
    assert_eq!(note("ok: routing off\nmore"), "ok: routing off");
    assert_eq!(note("  \x1b[1mbold\x07 \n"), "[1mbold");
    assert_eq!(note(&"x".repeat(100)).len(), 80);
    assert_eq!(note(""), "");
}

#[test]
fn locate_never_falls_back_to_path() {
    let dir = std::env::temp_dir().join(format!("bar-tray-locate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bar = dir.join(BAR_NAME);
    std::fs::write(&bar, "").unwrap();
    assert_eq!(beside_exe(Some(&dir.join("bar-tray"))).unwrap(), bar);
    // `PATH` is process-wide: `path_probe` runs in a child whose `PATH`
    // is only `dir`, where a `bar` is, and must still find none
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["backend::spawn::tests::path_probe", "--exact"])
        .env("PATH", &dir)
        .env("BAR_TRAY_PATH_PROBE", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    std::fs::remove_file(&bar).unwrap();
    let none = "no `bar` beside bar-tray; pass --bar <path>";
    assert_eq!(beside_exe(Some(&dir.join("bar-tray"))).unwrap_err(), none);
    assert_eq!(beside_exe(None).unwrap_err(), none);
}

/// The child of `locate_never_falls_back_to_path`; a no-op elsewhere.
#[test]
fn path_probe() {
    if std::env::var_os("BAR_TRAY_PATH_PROBE").is_none() {
        return;
    }
    let dir = PathBuf::from(std::env::var_os("PATH").unwrap());
    assert!(
        dir.join(BAR_NAME).is_file(),
        "the probe needs `bar` on PATH"
    );
    let e = locate(None).unwrap_err();
    assert_eq!(e, "no `bar` beside bar-tray; pass --bar <path>");
}

#[test]
fn missing_binary() {
    let e = run(
        Path::new("/nonexistent/bar"),
        &[],
        false,
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(e.starts_with("/nonexistent/bar: "), "{e}");
}

#[cfg(unix)]
mod unix {
    use super::*;
    const SH: &str = "/bin/sh";
    fn sh(script: &str) -> Vec<OsString> {
        vec!["-c".into(), script.into()]
    }

    #[test]
    fn captures_stdout() {
        let out = run(
            Path::new(SH),
            &sh("printf hi"),
            true,
            Duration::from_secs(5),
        );
        assert_eq!(out.unwrap(), "hi");
    }

    #[test]
    fn child_sees_scrubbed_env_and_no_stdin() {
        let out = run(
            Path::new(SH),
            &sh(r#"env | sort; read -r line && echo "stdin:$line"; exit 0"#),
            true,
            Duration::from_secs(5),
        )
        .unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert!(!lines.iter().any(|l| l.starts_with("HOME=")), "{lines:?}");
        assert!(!lines.iter().any(|l| l.starts_with("stdin:")), "{lines:?}");
        // the shell sets a few of its own (PWD, SHLVL, _, OLDPWD)
        let shell = ["PWD=", "SHLVL=", "_=", "OLDPWD="];
        let extra: Vec<&&str> = lines
            .iter()
            .filter(|l| !shell.iter().any(|s| l.starts_with(s)))
            .filter(|l| !HANDLER_ENV.iter().any(|k| l.starts_with(&format!("{k}="))))
            .collect();
        assert!(extra.is_empty(), "{extra:?}");
    }

    #[test]
    fn nonzero_exit_is_error() {
        let e = run(Path::new(SH), &sh("exit 3"), false, Duration::from_secs(5));
        assert_eq!(e.unwrap_err(), "exit 3");
    }

    #[test]
    fn stderr_is_the_failure_note() {
        let e = run(
            Path::new(SH),
            &sh("echo 'refused: needs confirmation' >&2; echo more >&2; exit 1"),
            false,
            Duration::from_secs(5),
        );
        assert_eq!(e.unwrap_err(), "refused: needs confirmation");
    }

    #[test]
    fn limit_kills() {
        let t = Instant::now();
        let e = run(
            Path::new(SH),
            &sh("sleep 30"),
            false,
            Duration::from_millis(100),
        );
        assert_eq!(e.unwrap_err(), "timed out");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn oversize_refused() {
        let s = format!("head -c {} /dev/zero", MAX_CAPTURE + 10);
        let e = run(Path::new(SH), &sh(&s), true, Duration::from_secs(10));
        assert_eq!(e.unwrap_err(), "output too large");
    }

    #[test]
    fn locate_explicit() {
        assert_eq!(locate(Some(Path::new(SH))).unwrap(), Path::new(SH));
        let e = locate(Some(Path::new("/nonexistent/bar"))).unwrap_err();
        assert_eq!(e, "/nonexistent/bar: not a file");
    }
}
