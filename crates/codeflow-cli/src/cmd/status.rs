//! `codeflow status [--capabilities] [--delivery]` — generated views, never
//! stored.

use clap::Args;
use codeflow_core::status::{collect_status, render_delivery, render_status};

#[derive(Args)]
pub struct StatusArgs {
    /// Show the full capability table (default: counts by status)
    #[arg(long)]
    pub capabilities: bool,

    /// Show the capability-delivery rollup: each capability's epics with their
    /// open/total task counts and next actionable tasks
    #[arg(long)]
    pub delivery: bool,
}

pub fn run(args: &StatusArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}");
            return 2;
        }
    };
    let view = collect_status(&root);
    print!("{}", render_status(&view, args.capabilities));
    if args.delivery {
        print!("{}", render_delivery(&view));
    }
    0
}
