//! `codeflow report ceremony`: the process cost over a window of merged
//! pull requests (TSK-149). History and refusals are read locally; review
//! rounds are the one host-backed read (SPC-013 R-103) and print `unknown`
//! when the host call fails.

use clap::{ArgGroup, Args, Subcommand};
use codeflow_core::ceremony::{self, Window};
use codeflow_core::hooks::policy::Policy;

#[derive(Debug, Args)]
pub struct ReportArgs {
    #[command(subcommand)]
    pub command: ReportCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReportCommand {
    /// Pull requests per logical change, review rounds per pull request
    /// (asked of the host; `unknown` when it cannot answer) and refusals
    /// hit by this clone's hooks and guards, over a window.
    #[command(group(ArgGroup::new("window").args(["prs", "since"]).required(true)))]
    Ceremony {
        /// Pull request numbers, first and last included: `568..644`.
        #[arg(long, value_name = "FIRST..LAST")]
        prs: Option<String>,
        /// Pull requests merged on this UTC date or later (`YYYY-MM-DD`).
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// With `--since`: merged on this UTC date or earlier.
        #[arg(long, value_name = "DATE", requires = "since")]
        until: Option<String>,
    },
}

#[must_use]
pub fn run(args: &ReportArgs) -> i32 {
    let ReportCommand::Ceremony { prs, since, until } = &args.command;
    let window = match (prs, since) {
        (Some(prs), _) => Window::numbers(prs),
        (None, Some(since)) => Window::dates(since, until.as_deref()),
        (None, None) => unreachable!("clap requires a window"),
    };
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            eprintln!("codeflow report ceremony: {error}");
            return 2;
        }
    };
    let root = super::repo_root();
    let prefixes = Policy::load(&root).git.branch_prefixes;
    match ceremony::build(&root, &prefixes, window, &ceremony::Gh) {
        Ok(report) => {
            print!("{report}");
            0
        }
        Err(error) => {
            eprintln!("codeflow report ceremony: {error}");
            1
        }
    }
}
