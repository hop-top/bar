use clap::{Command, CommandFactory, Parser, Subcommand};
use hop_bar::{conf, session};
use hop_top_kit::output::{register_output_flags, RegisterOutputFlagsOptions};

#[derive(Parser)]
#[command(
    name = "bar",
    version,
    about = "Configurable status line for Claude Code: tmux-style format, placeholders, plugins"
)]
struct Cli {
    #[command(subcommand)]
    cmd: SubCmd,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Print the status line.
    Render,
}

fn main() {
    let cli_cmd: Command = Cli::command();
    let (cli_cmd, _ctx) = register_output_flags(cli_cmd, RegisterOutputFlagsOptions::default());
    let matches = cli_cmd.get_matches();
    if matches.subcommand_name() == Some("render") {
        render();
    }
}

fn render() {
    let mut buf = Vec::new();
    // Non-UTF-8 or unreadable stdin degrades to an empty session.
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    let s = session::parse(&String::from_utf8_lossy(&buf));
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let out = hop_bar::render::run(&s, conf::load(), columns);
    if !out.is_empty() {
        println!("{out}");
    }
}
