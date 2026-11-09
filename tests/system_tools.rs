//! Tools trusted with Claude Code's sign-in resolve to fixed system paths,
//! whatever `PATH` and the working directory hold. One test in its own
//! process: it changes `PATH`.

#[cfg(any(target_os = "macos", windows))]
#[test]
fn trusted_tools_ignore_path_and_the_working_directory() {
    let dir = std::env::temp_dir().join(format!("bar-systools-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["curl", "security", "curl.exe", "curl.cmd"] {
        std::fs::write(dir.join(name), "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(
            dir.join(name),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
    }
    let path = std::env::join_paths([dir.clone(), ".".into(), "".into()]).unwrap();
    std::env::set_var("PATH", &path);
    std::env::set_current_dir(&dir).unwrap();
    let curl = hop_bar::platform::system_path("curl");
    #[cfg(target_os = "macos")]
    {
        assert_eq!(curl, std::path::Path::new("/usr/bin/curl"));
        assert_eq!(
            hop_bar::platform::system_path("security"),
            std::path::Path::new("/usr/bin/security")
        );
    }
    #[cfg(windows)]
    {
        assert!(curl.is_absolute(), "{curl:?}");
        assert!(curl.ends_with("curl.exe"));
        assert!(
            curl.parent().unwrap().ends_with("System32")
                || curl.parent().unwrap().ends_with("system32")
        );
    }
    assert!(!curl.starts_with(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}
