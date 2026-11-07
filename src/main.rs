use clap::{Command, CommandFactory, Parser, Subcommand};
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
    cli_cmd.get_matches();
}
