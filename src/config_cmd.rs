//! `bar config export` and `bar config import`.

use crate::cli_out::{self, with_fix};
use clap::{ArgMatches, Subcommand};
use hop_bar::confio::{self, Mode};
use hop_bar::{check, conf, plugin};
use hop_top_kit::output::CliError;
use serde_json::json;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Write the effective config (yours with the project config over it)
    /// to a file or stdout.
    Export {
        /// Output file; `-` or none for stdout.
        file: Option<PathBuf>,
        /// Your config file as it is, without the project config.
        #[arg(long)]
        user: bool,
        /// Overwrite FILE when it exists.
        #[arg(long)]
        force: bool,
    },
    /// Merge a config file into yours or replace yours with it, after a
    /// preview; the old file is backed up.
    Import {
        /// Config file to import; `-` for stdin.
        file: String,
        /// Lines of FILE replace the matching lines of yours; the rest of
        /// yours is kept.
        #[arg(long, required_unless_present = "replace", conflicts_with = "replace")]
        merge: bool,
        /// FILE becomes your config.
        #[arg(long)]
        replace: bool,
        /// Write without asking.
        #[arg(long, short)]
        yes: bool,
    },
}

/// Asking was not possible or the answer was not yes.
fn refused(msg: &str) -> CliError {
    with_fix(CliError::consent_refused(msg), "rerun with --yes to write")
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(m: &ArgMatches) {
    let _palettes = hop_bar::palette::use_dir(conf::palette_dir_default());
    match m.subcommand() {
        Some(("export", e)) => export(e),
        Some(("import", i)) => import(i),
        _ => {}
    }
}

fn export(m: &ArgMatches) {
    cli_out::check_format(m, "config export");
    let fail = |err: CliError| -> ! { cli_out::fail(m, "config export", err) };
    let user = conf::find_path_default().map(|p| {
        let text = read(&p).unwrap_or_else(|e| fail(CliError::generic(e)));
        (p, text)
    });
    let mut project = None;
    let body = if m.get_flag("user") {
        match &user {
            Some((_, text)) => text.clone(),
            None => fail(with_fix(
                CliError::not_found("no config file; the built-in layout is in use"),
                "`bar init` writes one",
            )),
        }
    } else {
        let parsed = conf::layers(user.as_ref().map(|(_, t)| t.as_str()), |_| {
            let p = conf::project::find(&std::env::current_dir().ok()?)?;
            let text = conf::project::read(&p);
            if let Ok(t) = &text {
                project = Some((p, t.clone()));
            }
            Some(text)
        });
        if parsed.config.is_err() {
            fail(with_fix(
                CliError::usage("the config has errors; run bar check"),
                "run bar check",
            ));
        }
        // what render leaves out of the project config is left out here too
        let skip: Vec<usize> = parsed.dropped.iter().map(|e| e.line).collect();
        if skip.contains(&0) {
            project = None;
        }
        project = project.map(|(p, t)| (p, confio::without_lines(&t, &skip)));
        confio::effective(
            user.as_ref().map(|(p, t)| (p.as_path(), t.as_str())),
            project.as_ref().map(|(p, t)| (p.as_path(), t.as_str())),
        )
    };
    let shown = |p: Option<&Path>| p.map(|p| p.display().to_string());
    match m
        .get_one::<PathBuf>("file")
        .filter(|f| f.as_os_str() != "-")
    {
        None => {
            let data = json!({
                "user": shown(user.as_ref().map(|(p, _)| p.as_path())),
                "project": shown(project.as_ref().map(|(p, _)| p.as_path())),
                "lines": confio::directives(&body),
                "text": body,
            });
            cli_out::emit(m, "config export", &body, &data);
        }
        Some(f) => {
            let old = std::fs::read_to_string(f).ok();
            if old.is_some() && !m.get_flag("force") {
                fail(CliError::conflict(format!(
                    "{} exists; pass --force to overwrite",
                    f.display()
                )));
            }
            if let Err(e) = confio::write_config(f, old.as_deref(), &body) {
                fail(CliError::generic(e));
            }
            let data = json!({"path": shown(Some(f)), "written": true});
            cli_out::emit(
                m,
                "config export",
                &format!("wrote {}\n", f.display()),
                &data,
            );
        }
    }
}

/// The user config file, or where `bar init` would write one.
fn target() -> Result<PathBuf, String> {
    if let Some(p) = conf::find_path_default() {
        return Ok(p);
    }
    let home = hop_bar::platform::home().unwrap_or_default();
    let p = conf::search_paths(
        &|k| std::env::var(k).ok(),
        &home,
        &hop_bar::platform::dirs::config(),
    )
    .remove(0);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err("the home dir is unset or relative; set it or XDG_CONFIG_HOME".into())
    }
}

/// A usage error of `bar config import`.
fn usage(m: &ArgMatches, msg: &str) -> ! {
    cli_out::fail(m, "config import", CliError::usage(msg))
}

/// Lints `text` as a user config; on errors exits with the report as the
/// cause.
fn lint(m: &ArgMatches, text: &str, name: &str) {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    let r = check::check_text(text, Some(PathBuf::from(name)), &dirs);
    if r.has_errors() {
        let mut err = CliError::usage(format!("{name} has errors; nothing written"));
        err.cause = r.to_text();
        cli_out::fail(m, "config import", with_fix(err, "fix the lines above"));
    }
}

fn import(m: &ArgMatches) {
    cli_out::check_format(m, "config import");
    let file = m.get_one::<String>("file").map_or("-", String::as_str);
    let stdin = file == "-";
    let text = if stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| usage(m, &format!("stdin: {e}")));
        s
    } else if !Path::new(file).exists() {
        let err = CliError::not_found(format!("{file}: no such file"));
        cli_out::fail(m, "config import", err)
    } else {
        read(Path::new(file)).unwrap_or_else(|e| usage(m, &e))
    };
    let shown = if stdin { "stdin" } else { file };
    lint(m, &text, shown);
    let mode = if m.get_flag("replace") {
        Mode::Replace
    } else {
        Mode::Merge
    };
    let target = target().unwrap_or_else(|e| usage(m, &e));
    let old = match target.exists() {
        true => Some(read(&target).unwrap_or_else(|e| usage(m, &e))),
        false => None,
    };
    let new = confio::imported(old.as_deref(), &text, mode);
    if mode == Mode::Merge {
        lint(m, &new, &format!("{} after the merge", target.display()));
    }
    let name = target.display().to_string();
    if old.as_deref() == Some(new.as_str()) {
        let text = format!("{name} already matches; nothing written\n");
        let data = json!({"path": name, "written": false, "backup": null});
        cli_out::emit(m, "config import", &text, &data);
        return;
    }
    let from = if old.is_some() {
        name.as_str()
    } else {
        "/dev/null"
    };
    let diff = hop_bar::diff::unified(old.as_deref().unwrap_or(""), &new, from, &name);
    let mut human = cli_out::human(m);
    let _ = write!(human, "{diff}");
    let _ = human.flush();
    warn_notable(shown, &text);
    if !m.get_flag("yes") {
        if let Err(e) = confirm(stdin, &name) {
            cli_out::fail(m, "config import", *e);
        }
    }
    match confio::write_config(&target, old.as_deref(), &new) {
        Ok(bak) => {
            let text = match &bak {
                Some(b) => format!("wrote {name} (old file: {})\n", b.display()),
                None => format!("wrote {name}\n"),
            };
            let backup = bak.map(|b| b.display().to_string());
            let data = json!({"path": name, "written": true, "backup": backup});
            cli_out::emit(m, "config import", &text, &data);
        }
        Err(e) => cli_out::fail(m, "config import", CliError::generic(e)),
    }
}

/// Points out the lines of `text` that reach outside the config.
fn warn_notable(shown: &str, text: &str) {
    let notable = confio::notable(text);
    if notable.is_empty() {
        return;
    }
    eprintln!("{shown} has lines that run commands or plugins, read the environment or set what project configs may do:");
    for (line, what) in &notable {
        eprintln!("  line {line}: {what}");
    }
}

/// Asks on the terminal before `name` is written; refuses unless the
/// answer is yes. The question needs a terminal on stdin, so a config
/// read from stdin cannot be confirmed.
fn confirm(stdin: bool, name: &str) -> Result<(), Box<CliError>> {
    if stdin {
        return Err(Box::new(refused(
            "reading the config from stdin needs --yes",
        )));
    }
    if !std::io::stdin().is_terminal() {
        return Err(Box::new(with_fix(
            CliError::consent_refused("stdin is not a terminal; pass --yes to write"),
            "pass --yes to write",
        )));
    }
    eprint!("write {name}? [y/N] ");
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        return Err(Box::new(refused("nothing written")));
    }
    Ok(())
}
