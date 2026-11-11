use clap::{ArgMatches, Subcommand};
use serde_json::json;

use crate::{cli_out, dry_run};

#[derive(Subcommand)]
pub enum PricesCmd {
    /// Fetch models.dev prices into bar's data dir (spendhist also does, daily, for models without a built-in price).
    Update {
        /// Write this file instead of the data dir's models.json.
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
        /// Print what would be written, as data with --format; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(m: &ArgMatches) {
    if let Some(("update", u)) = m.subcommand() {
        let out = u.get_one::<std::path::PathBuf>("out").map(|p| p.as_path());
        if dry_run::on(u) {
            cli_out::check_format(u, "prices update");
            let target = hop_bar::prices::update::target(out);
            let extra = json!({"source": hop_bar::prices::update::URL});
            return dry_run::emit(u, "prices update", vec![dry_run::write(&target)], extra, "");
        }
        match hop_bar::prices::update::run(out) {
            Ok((path, n)) => println!("{n} models written to {}", path.display()),
            Err(e) => {
                eprintln!("bar prices update: {e}");
                std::process::exit(2);
            }
        }
    }
}
