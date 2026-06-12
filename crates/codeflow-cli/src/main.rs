//! codeflow — the AI-development discipline layer CLI.

use clap::Parser;

#[derive(Parser)]
#[command(name = "codeflow", version, about = "AI-development discipline layer")]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    println!("codeflow v2 — under construction; see docs/plan/v2/00-charter.md");
    Ok(())
}
