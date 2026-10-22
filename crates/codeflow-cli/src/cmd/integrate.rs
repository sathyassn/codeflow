//! `codeflow integrate <branch> [--into <target>]` — the local sanctioned
//! path for landing work on a protected branch (charter §6.2).

use clap::Args;
use codeflow_core::integrate::{integrate_with_mode, IntegrateMode};

#[derive(Args)]
pub struct IntegrateArgs {
    /// Branch to integrate
    pub branch: String,

    /// Target branch to land on
    #[arg(long = "into", default_value = "main")]
    pub into: String,

    /// Preserve the source head with a no-fast-forward merge commit
    #[arg(long)]
    pub merge: bool,
}

pub fn run(args: &IntegrateArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };

    let mode = if args.merge {
        IntegrateMode::Merge
    } else {
        IntegrateMode::Rebase
    };
    match integrate_with_mode(&root, &args.branch, &args.into, mode) {
        Ok(outcome) => {
            println_block!(outcome.block());
            0
        }
        Err(e) => {
            eprintln!("codeflow integrate: {e}", e = crate::cmd::shown(&e));
            eprintln!("nothing was merged into '{}'", args.into);
            1
        }
    }
}
