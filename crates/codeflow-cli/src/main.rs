//! codeflow — the AI-development discipline layer CLI.

mod cmd;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "codeflow", version, about = "AI-development discipline layer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Search project memory: ledger, session summaries, ADRs, epics, capabilities
    Recall(cmd::recall::RecallArgs),
    /// Remote provider operations (branch protection)
    Remote(cmd::remote::RemoteArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cmd::touch_registry_best_effort();
    match cli.command {
        Some(Command::Recall(args)) => cmd::recall::run(&args),
        Some(Command::Remote(args)) => cmd::remote::run(&args),
        None => {
            println!("codeflow v2 — under construction; see docs/plan/v2/00-charter.md");
            Ok(())
        }
    }
}
