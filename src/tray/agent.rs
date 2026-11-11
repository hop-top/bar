//! macOS autostart: a per-user LaunchAgent that starts the tray at login,
//! loaded into the GUI domain with `launchctl`. No app bundle: the tray
//! sets its own activation policy, so a bare executable shows no Dock
//! icon.

use std::path::{Path, PathBuf};

/// The agent's label and the plist's file name.
pub const LABEL: &str = "top.hop.bar-tray";

const LAUNCHCTL: &str = "/bin/launchctl";

/// `~/Library/LaunchAgents`; `None` without an absolute home.
pub fn agents_dir(home: Option<&Path>) -> Option<PathBuf> {
    home.filter(|h| h.is_absolute())
        .map(|h| h.join("Library/LaunchAgents"))
}

pub fn plist_path(agents: &Path) -> PathBuf {
    agents.join(format!("{LABEL}.plist"))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn string(s: &str) -> String {
    format!("<string>{}</string>", xml(s))
}

/// The agent: `argv` as absolute `ProgramArguments`, the variables set at
/// install as `EnvironmentVariables` (unset ones are left out: launchd
/// starts agents with a minimal environment), started at load in the GUI
/// session only, restarted after a crash but not after `Quit` (exit 0).
pub fn plist(argv: &[String], env: &[(String, String)]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    s.push_str(&format!("\t<key>Label</key>\n\t{}\n", string(LABEL)));
    s.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    for a in argv {
        s.push_str(&format!("\t\t{}\n", string(a)));
    }
    s.push_str("\t</array>\n");
    if !env.is_empty() {
        s.push_str("\t<key>EnvironmentVariables</key>\n\t<dict>\n");
        for (k, v) in env {
            s.push_str(&format!("\t\t<key>{}</key>\n\t\t{}\n", xml(k), string(v)));
        }
        s.push_str("\t</dict>\n");
    }
    s.push_str(
        "\t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>\n\
         \t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         </dict>\n</plist>\n",
    );
    s
}

fn domain(uid: u32) -> String {
    format!("gui/{uid}")
}

fn service(uid: u32) -> String {
    format!("gui/{uid}/{LABEL}")
}

/// Loads the agent, which starts the tray (`RunAtLoad`).
pub fn bootstrap(uid: u32, plist: &Path) -> Vec<String> {
    vec![
        LAUNCHCTL.into(),
        "bootstrap".into(),
        domain(uid),
        plist.display().to_string(),
    ]
}

/// Unloads the agent, which stops the tray; fails when it is not loaded.
pub fn bootout(uid: u32) -> Vec<String> {
    vec![LAUNCHCTL.into(), "bootout".into(), service(uid)]
}

/// Starts a loaded agent whose tray has exited (`Quit`).
pub fn kickstart(uid: u32) -> Vec<String> {
    vec![LAUNCHCTL.into(), "kickstart".into(), service(uid)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_escapes_and_keeps_argv_order() {
        let p = plist(
            &["/a b/bar-tray".into(), "--bar".into(), "/x&y/<bar>".into()],
            &[("PATH".into(), "/bin:/usr/bin".into())],
        );
        let a = p.find("<string>/a b/bar-tray</string>").unwrap();
        let b = p.find("<string>--bar</string>").unwrap();
        let c = p.find("<string>/x&amp;y/&lt;bar&gt;</string>").unwrap();
        assert!(a < b && b < c, "{p}");
        assert!(p.contains("<key>PATH</key>\n\t\t<string>/bin:/usr/bin</string>"));
        assert!(p.contains("<key>Label</key>\n\t<string>top.hop.bar-tray</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(p.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
    }

    #[test]
    fn no_env_no_env_dict() {
        assert!(!plist(&["/t".into()], &[]).contains("EnvironmentVariables"));
    }

    #[test]
    fn commands_target_the_gui_domain() {
        assert_eq!(
            bootstrap(
                501,
                Path::new("/h/Library/LaunchAgents/top.hop.bar-tray.plist")
            ),
            [
                "/bin/launchctl",
                "bootstrap",
                "gui/501",
                "/h/Library/LaunchAgents/top.hop.bar-tray.plist"
            ]
        );
        assert_eq!(
            bootout(501),
            ["/bin/launchctl", "bootout", "gui/501/top.hop.bar-tray"]
        );
        assert_eq!(
            kickstart(501),
            ["/bin/launchctl", "kickstart", "gui/501/top.hop.bar-tray"]
        );
    }

    // LaunchAgents are macOS only; `/h` is no absolute home on Windows
    #[cfg(unix)]
    #[test]
    fn agents_live_under_an_absolute_home() {
        assert_eq!(agents_dir(Some(Path::new("rel"))), None);
        assert_eq!(
            agents_dir(Some(Path::new("/h"))),
            Some(PathBuf::from("/h/Library/LaunchAgents"))
        );
    }
}
