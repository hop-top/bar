//! `bar config export` and `bar config import`.

use clap::{ArgMatches, Subcommand};
use hop_bar::confio::{self, Mode};
use hop_bar::{check, conf, plugin};
use hop_top_kit::output::EXIT_USAGE;
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

fn fail(cmd: &str, msg: &str, code: i32) -> ! {
    eprintln!("bar config {cmd}: {msg}");
    std::process::exit(code);
}

fn usage(msg: &str) -> ! {
    fail("import", msg, EXIT_USAGE)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(m: &ArgMatches) {
    match m.subcommand() {
        Some(("export", e)) => export(e),
        Some(("import", i)) => import(i),
        _ => {}
    }
}

fn export(m: &ArgMatches) {
    let fail = |msg: &str| -> ! { fail("export", msg, EXIT_USAGE) };
    let user = conf::find_path_default().map(|p| {
        let text = read(&p).unwrap_or_else(|e| fail(&e));
        (p, text)
    });
    let body = if m.get_flag("user") {
        match &user {
            Some((_, text)) => text.clone(),
            None => fail("no config file; the built-in layout is in use"),
        }
    } else {
        let mut project = None;
        let parsed = conf::parse_layers(user.as_ref().map(|(_, t)| t.as_str()), |_| {
            let p = conf::project::find(&std::env::current_dir().ok()?)?;
            let text = read(&p);
            if let Ok(t) = &text {
                project = Some((p, t.clone()));
            }
            Some(text)
        });
        if parsed.is_err() {
            fail("the config has errors; run bar check");
        }
        confio::effective(
            user.as_ref().map(|(p, t)| (p.as_path(), t.as_str())),
            project.as_ref().map(|(p, t)| (p.as_path(), t.as_str())),
        )
    };
    match m
        .get_one::<PathBuf>("file")
        .filter(|f| f.as_os_str() != "-")
    {
        None => {
            let mut so = std::io::stdout().lock();
            let _ = so.write_all(body.as_bytes());
        }
        Some(f) => {
            if f.exists() && !m.get_flag("force") {
                fail(&format!(
                    "{} exists; pass --force to overwrite",
                    f.display()
                ));
            }
            if let Err(e) = confio::write_config(f, &body) {
                fail(&e);
            }
            println!("wrote {}", f.display());
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
        &hop_top_kit::xdg::raw_config_dir("bar"),
    )
    .remove(0);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err("the home dir is unset or relative; set it or XDG_CONFIG_HOME".into())
    }
}

/// Lints `text` as a user config; prints the report and exits 2 on errors.
fn lint(text: &str, name: &str) {
    let dirs = plugin::path_dirs(std::env::var_os("PATH").as_deref());
    let r = check::check_text(text, Some(PathBuf::from(name)), &dirs);
    if r.has_errors() {
        eprint!("{}", r.to_text());
        fail(
            "import",
            &format!("{name} has errors; nothing written"),
            EXIT_USAGE,
        );
    }
}

fn import(m: &ArgMatches) {
    let file = m.get_one::<String>("file").map_or("-", String::as_str);
    let stdin = file == "-";
    let text = if stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| usage(&format!("stdin: {e}")));
        s
    } else {
        read(Path::new(file)).unwrap_or_else(|e| usage(&e))
    };
    let shown = if stdin { "stdin" } else { file };
    lint(&text, shown);
    let mode = if m.get_flag("replace") {
        Mode::Replace
    } else {
        Mode::Merge
    };
    let target = target().unwrap_or_else(|e| usage(&e));
    let old = match target.exists() {
        true => Some(read(&target).unwrap_or_else(|e| usage(&e))),
        false => None,
    };
    let new = confio::imported(old.as_deref(), &text, mode);
    if mode == Mode::Merge {
        lint(&new, &format!("{} after the merge", target.display()));
    }
    if old.as_deref() == Some(new.as_str()) {
        println!("{} already matches; nothing written", target.display());
        return;
    }
    let name = target.display().to_string();
    let from = if old.is_some() {
        name.as_str()
    } else {
        "/dev/null"
    };
    let diff = hop_bar::diff::unified(old.as_deref().unwrap_or(""), &new, from, &name);
    print!("{diff}");
    let _ = std::io::stdout().flush();
    if !m.get_flag("yes") {
        if stdin {
            usage("reading the config from stdin needs --yes");
        }
        if !std::io::stdin().is_terminal() {
            usage("stdin is not a terminal; pass --yes to write");
        }
        eprint!("write {name}? [y/N] ");
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            fail("import", "nothing written", 1);
        }
    }
    match confio::write_config(&target, &new) {
        Ok(Some(bak)) => println!("wrote {name} (old file: {})", bak.display()),
        Ok(None) => println!("wrote {name}"),
        Err(e) => fail("import", &e, 1),
    }
}
