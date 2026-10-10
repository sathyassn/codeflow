//! `codeflow portal setup` — explicit, offline portal-starter adoption.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use codeflow_core::scaffold::{self, AssetSource};

#[derive(Args)]
#[command(
    about = "Adopt or reconcile the opt-in documentation portal",
    long_about = "Apply the utility presentation design system to durable repository docs\n\
for this project or any consuming project. Author source-in-place Markdown.\n\
Not a present session and not product UI. No Comment lifecycle."
)]
pub struct PortalArgs {
    #[command(subcommand)]
    command: PortalCommand,
}

#[derive(Subcommand)]
enum PortalCommand {
    /// Adopt or reconcile the documentation portal starter.
    Setup {
        /// Repository-relative portal workspace directory.
        #[arg(long, value_name = "DIR")]
        path: PathBuf,
    },
    /// Take project ownership, preserving runtime files and intentional deletions.
    Transfer {
        /// Confirm responsibility for future runtime reconciliation.
        #[arg(long, required = true)]
        confirm: bool,
    },
}

pub fn run(args: &PortalArgs, assets: &dyn AssetSource) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}");
            return 2;
        }
    };
    let result = match &args.command {
        PortalCommand::Setup { path } => scaffold::portal::setup_portal(assets, &root, path),
        PortalCommand::Transfer { confirm } => scaffold::portal::transfer_portal(&root, *confirm),
    };
    match result {
        Ok(report) => {
            print!("{report}");
            i32::from(report.has_conflicts()) * 2
        }
        Err(error) => {
            eprintln!("codeflow portal: {error}");
            1
        }
    }
}
