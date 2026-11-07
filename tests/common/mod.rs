//! Hermetic environment for tests that run the `bar` binary: state in a
//! temp dir, a fake `gh` that fails without network, a non-git cwd; every
//! config, cache and credential location points into `root`.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n",
            gh_log(root).display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut path = bin.into_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    // `Command::envs` cannot unset, so the API key is emptied.
    let env = vec![
        ("PATH", path),
        ("BAR_STATE_DIR", root.join("state").into()),
        ("XDG_STATE_HOME", root.join("xdg-state").into()),
        ("HOME", root.into()),
        ("XDG_CONFIG_HOME", root.join("xdg").into()),
        ("XDG_CACHE_HOME", root.join("xdg-cache").into()),
        ("CLAUDE_CONFIG_DIR", root.join("claude").into()),
        ("ANTHROPIC_API_KEY", OsString::new()),
        ("GIT_CONFIG_GLOBAL", "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
    ];
    (env, cwd)
}
