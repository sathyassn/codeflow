//! `codeflow git-hook <pre-commit|commit-msg|pre-push>` — the target the
//! `.git/hooks` shims exec (charter §6.1 plane 1). Exit 1 blocks the git
//! operation; warn-level findings are printed and let it proceed.

use std::io::Read;
use std::path::PathBuf;

use clap::Args;
use codeflow_core::hooks::{git_hook, policy::Policy};

/// Which git client hook stage to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum StageName {
    PreCommit,
    CommitMsg,
    PreMergeCommit,
    ReferenceTransaction,
    PrePush,
}

#[derive(Debug, Args)]
pub struct GitHookArgs {
    /// Hook stage (receives git's standard hook arguments).
    #[arg(value_enum)]
    pub stage: StageName,
    /// Arguments passed through by git (e.g. the commit-msg file path,
    /// the pre-push remote name and URL).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Run the stage; returns the process exit code.
#[must_use]
pub fn run(args: &GitHookArgs) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = super::project_root(&cwd);

    // reference-transaction fires on every ref update, including the hundreds
    // of remote-tracking refs a `git fetch` touches. Short-circuit before any
    // policy load or full stdin parse when it cannot apply (charter §6.1
    // performance note; ADR-0007).
    if let StageName::ReferenceTransaction = args.stage {
        return run_reference_transaction(&root, &args.args);
    }

    let (policy, _armed) = Policy::load_effective(&root);
    let token = super::integrate_token_present();

    let (plane, result) = match args.stage {
        StageName::PreCommit => ("pre-commit", git_hook::pre_commit(&root, &policy.git, token)),
        StageName::CommitMsg => ("commit-msg", commit_msg(&policy, &args.args)),
        StageName::PreMergeCommit => (
            "pre-merge-commit",
            git_hook::pre_merge_commit(
                &root,
                &policy.git,
                token,
                // Out-of-band human-authorization check-point (ADR-0009): today
                // `none` passes the env override through unchanged; a future
                // adapter would require its factor here.
                policy
                    .human_authorization
                    .authorizes_override(super::human_override_present()),
            ),
        ),
        StageName::ReferenceTransaction => unreachable!("handled above"),
        StageName::PrePush => {
            let mut stdin = String::new();
            let _ = std::io::stdin().read_to_string(&mut stdin);
            let refs = git_hook::parse_push_refs(&stdin);
            ("pre-push", git_hook::pre_push(&root, &policy.git, &refs, token))
        }
    };

    match result {
        Ok(report) => super::render_outcome(plane, &report.violations, &report.notes, 1),
        Err(e) => {
            // A hook that cannot evaluate must not block work invisibly:
            // report and pass (CI remains the hard line, charter D19).
            eprintln!("codeflow {plane}: warning: {e} — check skipped");
            0
        }
    }
}

/// Handle `git-hook reference-transaction <state>`. Exit 1 on the `prepared`
/// state cancels the transaction; every other path exits 0.
fn run_reference_transaction(root: &std::path::Path, args: &[String]) -> i32 {
    // Only the `prepared` phase can cancel a transaction; `committed`/`aborted`
    // are notifications. Bail before touching stdin or policy.
    if args.first().map(String::as_str) != Some("prepared") {
        return 0;
    }
    let mut stdin = String::new();
    if std::io::stdin().read_to_string(&mut stdin).is_err() {
        return 0;
    }
    // Fast path: no local-branch update in this transaction (e.g. a fetch that
    // only moved remote-tracking refs) — allow without loading policy.
    if !stdin.lines().any(git_hook::ref_line_touches_local_branch) {
        return 0;
    }

    let (policy, _armed) = Policy::load_effective(root);
    let token = super::integrate_token_present();
    // Out-of-band human-authorization check-point (ADR-0009): `none` is a no-op.
    let human = policy
        .human_authorization
        .authorizes_override(super::human_override_present());
    match git_hook::reference_transaction(root, &policy.git, &stdin, token, human) {
        Ok(report) => {
            super::render_outcome("reference-transaction", &report.violations, &report.notes, 1)
        }
        Err(e) => {
            eprintln!("codeflow reference-transaction: warning: {e} — check skipped");
            0
        }
    }
}

fn commit_msg(
    policy: &Policy,
    args: &[String],
) -> Result<git_hook::StageReport, codeflow_core::error::HookError> {
    // Git invokes commit-msg with the message file path relative to its own
    // cwd (the worktree top level), which is also our cwd — read it as-is.
    let msg_file = args.first().map(PathBuf::from).ok_or_else(|| {
        codeflow_core::error::HookError::Config(
            "commit-msg: missing message file argument".to_string(),
        )
    })?;
    let message = std::fs::read_to_string(&msg_file)?;
    Ok(git_hook::commit_msg(&policy.git, &message))
}
