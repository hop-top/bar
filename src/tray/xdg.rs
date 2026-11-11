//! Linux autostart files: an XDG autostart entry, or a systemd user unit,
//! and the tray icon in the user's hicolor theme. Contents and paths only;
//! `linux` writes and removes them.

use std::path::{Path, PathBuf};

/// The user unit's name and file name.
pub const UNIT: &str = "bar-tray.service";
/// The autostart entry's file name.
pub const DESKTOP_FILE: &str = "bar-tray.desktop";
/// The icon the tray names (`IconName`); symbolic, so hosts recolour it.
pub const ICON_NAME: &str = "bar-tray-symbolic";

/// The tray glyph (three bars, as the fallback pixmap draws it) as a
/// symbolic SVG: GTK hosts repaint its shapes in the panel's foreground,
/// KDE hosts through the `ColorScheme-Text` class.
pub const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 32 32">
  <style id="current-color-scheme">.ColorScheme-Text { color: #232629; }</style>
  <g class="ColorScheme-Text" fill="currentColor">
    <rect x="6" y="7" width="20" height="5"/>
    <rect x="6" y="14" width="14" height="5"/>
    <rect x="6" y="21" width="17" height="5"/>
  </g>
</svg>
"##;

/// Where the files go: the XDG config and data homes, and the desktop
/// the session runs (`XDG_CURRENT_DESKTOP`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub desktop: Option<String>,
}

impl Dirs {
    /// `XDG_CONFIG_HOME` and `XDG_DATA_HOME` when absolute (the spec
    /// ignores relative ones), else `~/.config` and `~/.local/share`;
    /// `None` when a dir has neither.
    pub fn from_env(home: Option<&Path>, get: &dyn Fn(&str) -> Option<String>) -> Option<Dirs> {
        let home = home.filter(|h| h.is_absolute());
        let base = |var: &str, rel: &str| {
            get(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.map(|h| h.join(rel)))
        };
        Some(Dirs {
            config: base("XDG_CONFIG_HOME", ".config")?,
            data: base("XDG_DATA_HOME", ".local/share")?,
            desktop: get("XDG_CURRENT_DESKTOP").filter(|d| !d.is_empty()),
        })
    }

    pub fn desktop_entry(&self) -> PathBuf {
        self.config.join("autostart").join(DESKTOP_FILE)
    }

    pub fn unit(&self) -> PathBuf {
        self.config.join("systemd/user").join(UNIT)
    }

    pub fn icon(&self) -> PathBuf {
        self.data
            .join("icons/hicolor/scalable/apps")
            .join(format!("{ICON_NAME}.svg"))
    }

    /// The installed autostart file: the unit, else the entry.
    pub fn autostart(&self) -> Option<PathBuf> {
        [self.unit(), self.desktop_entry()]
            .into_iter()
            .find(|p| p.is_file())
    }

    /// Whether the session is GNOME Shell (`ubuntu:GNOME`, `GNOME`), which
    /// shows tray icons only with the AppIndicator extension.
    pub fn gnome(&self) -> bool {
        self.desktop
            .as_deref()
            .is_some_and(|d| d.split(':').any(|n| n.eq_ignore_ascii_case("GNOME")))
    }
}

/// What install prints on GNOME.
pub const GNOME_HINT: &str = "GNOME shows tray icons only with the AppIndicator extension: \
install gnome-shell-extension-appindicator and enable it \
(gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com); Ubuntu ships it enabled";

fn one_line(s: &str) -> Result<&str, String> {
    if s.contains(['\n', '\r']) {
        Err(format!(
            "a line break cannot go in an autostart file: {s:?}"
        ))
    } else {
        Ok(s)
    }
}

/// One `Exec` argument (desktop entry spec): quoted when it holds a
/// reserved character, with `"`, `` ` ``, `$` and `\` escaped inside the
/// quotes; `%` doubled (field codes); then every `\` doubled again, the
/// string escape applied before the quoting rule.
fn exec_arg(a: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
    ];
    let a = a.replace('%', "%%");
    let quoted = if a.is_empty() || a.contains(RESERVED) {
        let mut q = String::from("\"");
        for c in a.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    } else {
        a
    };
    quoted.replace('\\', "\\\\")
}

/// The command line with the variables in front through `env`, which
/// a desktop entry has no other way to set.
fn with_env(argv: &[String], env: &[(String, String)]) -> Vec<String> {
    if env.is_empty() {
        return argv.to_vec();
    }
    let mut v = vec!["/usr/bin/env".to_string()];
    v.extend(env.iter().map(|(k, val)| format!("{k}={val}")));
    v.extend_from_slice(argv);
    v
}

/// The autostart entry: `argv[0]` (the tray) is the `TryExec`, so a
/// session skips the entry once the binary is gone.
pub fn desktop_entry(argv: &[String], env: &[(String, String)]) -> Result<String, String> {
    let mut exec = Vec::new();
    for a in with_env(argv, env) {
        exec.push(exec_arg(one_line(&a)?));
    }
    let tray = one_line(argv.first().ok_or("no tray command")?)?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=bar tray\n\
         Comment=bar's toggles and plugin actions in the tray\n\
         Exec={}\n\
         TryExec={}\n\
         Icon={ICON_NAME}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec.join(" "),
        tray.replace('\\', "\\\\"),
    ))
}

/// A word of a unit file setting: `%` doubled (specifiers), and quoted
/// with `\` and `"` escaped when it holds whitespace, a quote or `\`.
fn unit_word(a: &str) -> String {
    let a = a.replace('%', "%%");
    if a.is_empty() || a.contains([' ', '\t', '"', '\'', '\\']) {
        format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        a
    }
}

/// The user unit: started with the graphical session and stopped with
/// it, restarted after a crash (not after `Quit`, which exits 0), giving
/// up after five quick failures. `$` is doubled in the command, where
/// systemd would expand variables; `Environment=` takes it as is.
pub fn unit(argv: &[String], env: &[(String, String)]) -> Result<String, String> {
    let tray = argv.first().ok_or("no tray command")?;
    if !Path::new(tray).is_absolute() {
        return Err(format!("the tray must be an absolute path: {tray}"));
    }
    let mut exec = Vec::new();
    for a in argv {
        exec.push(unit_word(one_line(a)?).replace('$', "$$"));
    }
    let mut s = String::from(
        "[Unit]\n\
         Description=bar tray: bar's toggles and plugin actions in the tray\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         StartLimitIntervalSec=60\n\
         StartLimitBurst=5\n\
         \n[Service]\n",
    );
    s.push_str(&format!("ExecStart={}\n", exec.join(" ")));
    for (k, v) in env {
        s.push_str(&format!(
            "Environment={}\n",
            unit_word(one_line(&format!("{k}={v}"))?)
        ));
    }
    s.push_str(
        "Restart=on-failure\n\
         RestartSec=5\n\
         \n[Install]\n\
         WantedBy=graphical-session.target\n",
    );
    Ok(s)
}

/// `systemctl --user <args>`, found on `PATH` (its place differs between
/// distributions).
pub fn systemctl(args: &[&str]) -> Vec<String> {
    let mut v = vec![
        crate::platform::program("systemctl").display().to_string(),
        "--user".into(),
    ];
    v.extend(args.iter().map(|a| a.to_string()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn exec_args_follow_the_desktop_entry_quoting() {
        assert_eq!(exec_arg("/usr/bin/bar-tray"), "/usr/bin/bar-tray");
        assert_eq!(exec_arg("/a b/bar"), "\"/a b/bar\"");
        assert_eq!(exec_arg("100%"), "100%%");
        assert_eq!(exec_arg("a\"b"), "\"a\\\\\"b\"");
        assert_eq!(exec_arg("$HOME"), "\"\\\\$HOME\"");
        // a literal backslash in a quoted argument takes four
        assert_eq!(exec_arg("a\\b"), "\"a\\\\\\\\b\"");
        assert_eq!(exec_arg(""), "\"\"");
        assert_eq!(exec_arg("K=v"), "K=v");
    }

    #[test]
    fn desktop_entry_bakes_env_and_tryexec() {
        let d = desktop_entry(
            &s(&["/o/bar-tray", "--bar", "/o/bar", "--lock", "/s p/tray.lock"]),
            &[("PATH".into(), "/usr/bin:/bin".into())],
        )
        .unwrap();
        assert!(d.starts_with("[Desktop Entry]\nType=Application\n"), "{d}");
        assert!(
            d.contains(
                "\nExec=/usr/bin/env PATH=/usr/bin:/bin /o/bar-tray --bar /o/bar --lock \"/s p/tray.lock\"\n"
            ),
            "{d}"
        );
        assert!(d.contains("\nTryExec=/o/bar-tray\n"), "{d}");
        assert!(d.contains("\nIcon=bar-tray-symbolic\n"), "{d}");
        let bare = desktop_entry(&s(&["/o/bar-tray"]), &[]).unwrap();
        assert!(bare.contains("\nExec=/o/bar-tray\n"), "{bare}");
        assert!(desktop_entry(&s(&["/o/bar-tray", "a\nb"]), &[]).is_err());
    }

    // XDG autostart and systemd user units are Linux only; their `/`
    // paths are not absolute on Windows
    #[cfg(unix)]
    #[test]
    fn unit_quotes_words_and_escapes_specifiers() {
        let u = unit(
            &s(&[
                "/a b/bar-tray",
                "--bar",
                "/o/b$r",
                "--lock",
                "/s/100%/tray.lock",
            ]),
            &[
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("BAR_STATE_DIR".into(), "/s p/\"q\"".into()),
            ],
        )
        .unwrap();
        assert!(
            u.contains("\nExecStart=\"/a b/bar-tray\" --bar /o/b$$r --lock /s/100%%/tray.lock\n"),
            "{u}"
        );
        assert!(u.contains("\nEnvironment=PATH=/usr/bin:/bin\n"), "{u}");
        assert!(
            u.contains("\nEnvironment=\"BAR_STATE_DIR=/s p/\\\"q\\\"\"\n"),
            "{u}"
        );
        for line in [
            "Restart=on-failure",
            "WantedBy=graphical-session.target",
            "PartOf=graphical-session.target",
            "StartLimitBurst=5",
        ] {
            assert!(u.contains(&format!("\n{line}\n")), "{line}\n{u}");
        }
        assert!(unit(&s(&["bar-tray"]), &[]).is_err(), "relative tray");
        assert!(unit(&s(&["/t", "a\nb"]), &[]).is_err());
    }

    // XDG autostart and systemd user units are Linux only; their `/`
    // paths are not absolute on Windows
    #[cfg(unix)]
    #[test]
    fn dirs_follow_xdg_with_home_fallback() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        let d = Dirs::from_env(Some(Path::new("/h")), &env(&[])).unwrap();
        assert_eq!(
            d.desktop_entry(),
            Path::new("/h/.config/autostart/bar-tray.desktop")
        );
        assert_eq!(
            d.unit(),
            Path::new("/h/.config/systemd/user/bar-tray.service")
        );
        assert_eq!(
            d.icon(),
            Path::new("/h/.local/share/icons/hicolor/scalable/apps/bar-tray-symbolic.svg")
        );
        let d = Dirs::from_env(
            Some(Path::new("/h")),
            &env(&[
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_DATA_HOME", "rel"),
                ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
            ]),
        )
        .unwrap();
        assert_eq!(d.config, Path::new("/c"));
        assert_eq!(d.data, Path::new("/h/.local/share"), "relative ignored");
        assert!(d.gnome());
        assert!(Dirs::from_env(None, &env(&[("XDG_CONFIG_HOME", "/c")])).is_none());
        assert!(Dirs::from_env(Some(Path::new("rel")), &env(&[])).is_none());
        let kde = Dirs::from_env(
            Some(Path::new("/h")),
            &env(&[("XDG_CURRENT_DESKTOP", "KDE")]),
        );
        assert!(!kde.unwrap().gnome());
        let classic = Dirs {
            desktop: Some("GNOME-Classic:GNOME".into()),
            ..d
        };
        assert!(classic.gnome());
    }

    #[test]
    fn icon_names_what_the_tray_shows() {
        assert!(ICON_SVG.contains("ColorScheme-Text"));
        assert_eq!(ICON_SVG.matches("<rect ").count(), 3);
        assert_eq!(systemctl(&["start", UNIT])[1..], ["--user", "start", UNIT]);
    }
}
