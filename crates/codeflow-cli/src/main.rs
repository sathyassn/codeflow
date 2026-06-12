//! codeflow — the AI-development discipline layer CLI.

mod cmd;

use clap::Parser;

#[derive(Parser)]
#[command(name = "codeflow", version, about = "AI-development discipline layer")]
struct Cli {
    #[command(subcommand)]
    command: Option<cmd::Command>,
}

fn main() {
    let cli = Cli::parse();
    let code = cli.command.map_or_else(
        || {
            println!("codeflow v2 — under construction; see docs/plan/v2/00-charter.md");
            0
        },
        cmd::run,
    );
    std::process::exit(code);
}
