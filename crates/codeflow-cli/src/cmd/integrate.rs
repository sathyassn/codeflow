//! `codeflow integrate <branch> [--into <target>]` — the local sanctioned
//! path for landing work on a protected branch (charter §6.2).

use clap::Args;
use codeflow_core::integrate::integrate;

#[derive(Args)]
pub struct IntegrateArgs {
    /// Branch to integrate
    pub branch: String,

    /// Target branch to land on
    #[arg(long = "into", default_value = "main")]
    pub into: String,
}

pub fn run(args: &IntegrateArgs) -> i32 {
    let root = super::repo_root();

    match integrate(&root, &args.branch, &args.into) {
        Ok(outcome) => {
            println!("{outcome}");
            0
        }
        Err(e) => {
            eprintln!("codeflow integrate: {e}");
            eprintln!("nothing was merged into '{}'", args.into);
            1
        }
    }
}
