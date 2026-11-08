//! Hermetic environment for tests that run the `bar` binary: state in a
//! temp dir, a fake `gh` and `curl` that fail without network, a non-git
//! cwd; every config, data, cache and credential location points into
//! `root`.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Status-line JSON whose `workspace.current_dir` is `dir`, escaped so
/// Windows paths survive.
pub fn workspace_json(dir: &Path) -> String {
    serde_json::json!({"workspace": {"current_dir": dir}}).to_string()
}

/// Where the fake `gh` logs each call.
pub fn gh_log(root: &Path) -> PathBuf {
    root.join("fake-gh.log")
}

/// Env pairs to apply to the command; creates the fake `gh` and cwd dirs
/// under `root`. Returns the neutral working directory too.
pub fn isolation(root: &Path) -> (Vec<(&'static str, OsString)>, PathBuf) {
    let bin = root.join("fake-bin");
    let cwd = root.join("neutral-cwd");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    fake_gh(&bin, &gh_log(root));
    fake_curl(&bin);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(bin).chain(std::env::split_paths(&inherited));
    let path = std::env::join_paths(dirs).unwrap();
    // `Command::envs` cannot unset, so the API key is emptied.
    let env = vec![
        ("PATH", path),
        ("BAR_STATE_DIR", root.join("state").into()),
        ("XDG_STATE_HOME", root.join("xdg-state").into()),
        ("HOME", root.into()),
        ("XDG_CONFIG_HOME", root.join("xdg").into()),
        ("XDG_CACHE_HOME", root.join("xdg-cache").into()),
        ("XDG_DATA_HOME", root.join("xdg-data").into()),
        ("CLAUDE_CONFIG_DIR", root.join("claude").into()),
        ("ANTHROPIC_API_KEY", OsString::new()),
        ("GIT_CONFIG_GLOBAL", NULL_DEVICE.into()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
    ];
    #[cfg(windows)]
    let env = {
        let mut env = env;
        env.extend([
            ("USERPROFILE", root.into()),
            ("LOCALAPPDATA", root.join("local-app-data").into()),
            ("APPDATA", root.join("app-data").into()),
        ]);
        env
    };
    (env, cwd)
}

#[cfg(unix)]
const NULL_DEVICE: &str = "/dev/null";
#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";

/// A `gh` that logs its arguments and fails.
#[cfg(unix)]
fn fake_gh(bin: &Path, log: &Path) {
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// A `gh.cmd` that logs its arguments and fails.
#[cfg(windows)]
fn fake_gh(bin: &Path, log: &Path) {
    std::fs::write(
        bin.join("gh.cmd"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nexit /b 1\r\n",
            log.display()
        ),
    )
    .unwrap();
}

/// A `curl` that fails: a render's background models.dev fetch never
/// reaches the network.
#[cfg(unix)]
fn fake_curl(bin: &Path) {
    let curl = bin.join("curl");
    std::fs::write(&curl, "#!/bin/sh\nexit 7\n").unwrap();
    std::fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

#[cfg(windows)]
fn fake_curl(bin: &Path) {
    std::fs::write(bin.join("curl.cmd"), "@echo off\r\nexit /b 7\r\n").unwrap();
}
