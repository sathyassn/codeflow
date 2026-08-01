//! `codeflow portal setup` — explicit, offline portal-starter adoption.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use codeflow_core::scaffold::{self, AssetSource};

#[derive(Args)]
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
}

pub fn run(args: &PortalArgs, assets: &dyn AssetSource) -> i32 {
    let root = super::repo_root();
    let result = match &args.command {
        PortalCommand::Setup { path } => scaffold::portal::setup_portal(assets, &root, path),
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
