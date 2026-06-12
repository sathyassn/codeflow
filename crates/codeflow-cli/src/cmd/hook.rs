//! `codeflow hook <git-guard|session-orient|session-summary>` — the Claude
//! layer hooks, wired by the settings presets (charter §3.3).
//!
//! Exit-code contract:
//! - `git-guard`: 0 allow, 2 block (`PreToolUse` deny) with the violated rule
//!   and sanctioned path on stderr.
//! - `session-orient`: digest on stdout, always 0.
//! - `session-summary`: always 0 — a failed summary must never fail the
//!   session (warn on stderr instead).

use std::io::Read;

use clap::Args;
use codeflow_core::hooks::{git_guard, orient, policy::Policy, session_summary};

/// Which Claude-layer hook to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum HookName {
    /// `PreToolUse` (Bash): enforce policy.json git rules in-session.
    GitGuard,
    /// `SessionStart`: emit the orient digest to stdout.
    SessionOrient,
    /// `SessionEnd`: append the session record to the ledger.
    SessionSummary,
}

#[derive(Debug, Args)]
pub struct HookArgs {
    /// Hook to run (reads the Claude Code hook payload from stdin).
    #[arg(value_enum)]
    pub name: HookName,
}

/// Run the hook; returns the process exit code.
#[must_use]
pub fn run(args: &HookArgs) -> i32 {
    let mut stdin = String::new();
    // Hooks always receive a JSON payload on stdin; an unreadable stream is
    // treated as empty (degrade legibly, never crash the session).
    let _ = std::io::stdin().read_to_string(&mut stdin);

    match args.name {
        HookName::GitGuard => git_guard(&stdin),
        HookName::SessionOrient => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
            print!("{}", orient::generate(&super::project_root(&cwd)));
            0
        }
        HookName::SessionSummary => session_summary(&stdin),
    }
}

fn git_guard(stdin: &str) -> i32 {
    let payload = match git_guard::HookPayload::parse(stdin) {
        Ok(p) => p,
        Err(e) => {
            // Fail open with a visible warning: a malformed payload must not
            // veto every Bash call (charter principle 8 — legible, not silent).
            eprintln!("codeflow git-guard: warning: unreadable hook payload ({e}); allowing");
            return 0;
        }
    };
    let Some(command) = payload.bash_command() else {
        return 0; // not a Bash tool call
    };

    let cwd = payload
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into());
    let root = super::project_root(&cwd);
    let (policy, _armed) = Policy::load_effective(&root);
    let branch = codeflow_core::hooks::RepoInfo::discover(&root)
        .map(|i| i.branch)
        .unwrap_or_default();

    let ctx = git_guard::GuardContext {
        policy: &policy.git,
        current_branch: &branch,
        integrate_token: super::integrate_token_present(),
    };
    let violations = git_guard::evaluate(command, &ctx);
    super::render_outcome("git-guard", &violations, &[], 2)
}

fn session_summary(stdin: &str) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match session_summary::record(&super::project_root(&cwd), stdin) {
        Ok(path) => {
            eprintln!(
                "codeflow session-summary: recorded to {}",
                path.display()
            );
            0
        }
        Err(e) => {
            eprintln!("codeflow session-summary: warning: {e} — session unaffected");
            0
        }
    }
}
