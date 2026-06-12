//! `codeflow status [--capabilities]` — generated views, never stored.

use clap::Args;
use codeflow_core::status::{collect_status, render_status};

#[derive(Args)]
pub struct StatusArgs {
    /// Show the full capability table (default: counts by status)
    #[arg(long)]
    pub capabilities: bool,
}

pub fn run(args: &StatusArgs) -> i32 {
    let root = super::repo_root();
    let view = collect_status(&root);
    print!("{}", render_status(&view, args.capabilities));
    0
}
