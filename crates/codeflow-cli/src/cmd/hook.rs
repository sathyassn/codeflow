//! `codeflow hook <git-guard|exec-guard|session-orient|session-summary|delegate-turn>` — the
//! Claude layer hooks, wired by the settings presets (charter §3.3).
//!
//! Exit-code contract:
//! - `git-guard`: 0 allow, 2 block (`PreToolUse` deny) with the violated rule
//!   and sanctioned path on stderr.
//! - `exec-guard`: 0 allow (or warn), 2 block — same `PreToolUse` shell
//!   contract, enforcing the `security` policy section (ADR-0008). Payload is
//!   parsed leniently so the same subcommand serves the Codex hooks engine.
//! - `session-orient`: digest on stdout, always 0.
//! - `session-summary`: always 0 — a failed summary must never fail the
//!   session (warn on stderr instead).
//! - `delegate-turn`: schema-v2 state mode handles the full lifecycle without
//!   tmux; legacy result mode preserves its existing terminal signal contract.

use std::io::Read;
use std::path::PathBuf;

use clap::{ArgGroup, Args};
use codeflow_core::hooks::{
    delegate_turn, exec_guard, git_guard, orient, policy::Policy, session_summary,
};

// Large enough for the maximum decoded terminal message even when every byte
// uses JSON's longest escape, while still bounding allocation before parsing.
const MAX_SCHEMA_HOOK_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// Which Claude-layer hook to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum HookName {
    /// `PreToolUse` (Bash/PowerShell): enforce policy.json git rules in-session.
    GitGuard,
    /// `PreToolUse` (Bash/PowerShell): enforce policy.json `security` rules (dangerous
    /// commands, privilege escalation). Harness-agnostic — also serves Codex.
    ExecGuard,
    /// `SessionStart`: emit the orient digest to stdout.
    SessionOrient,
    /// `SessionEnd`: append the session record to the ledger.
    SessionSummary,
    /// `SessionStart` / `UserPromptSubmit` / `Stop` / `StopFailure`: track an
    /// interactive delegate lifecycle (--state-dir); legacy --result signals termination.
    DelegateTurn,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("delegate_output")
        .args(["result", "state_dir"])
        .multiple(false)
))]
pub struct HookArgs {
    /// Hook to run (reads the Claude Code hook payload from stdin).
    #[arg(value_enum)]
    pub name: HookName,
    /// Unique task id for `delegate-turn` (1-64 safe ASCII characters).
    #[arg(long, value_name = "ID")]
    pub run_id: Option<String>,
    /// Absolute owner-only result path for `delegate-turn`.
    #[arg(long, value_name = "FILE")]
    pub result: Option<PathBuf>,
    /// Absolute owner-only lifecycle directory for schema-v2 `delegate-turn`.
    #[arg(long, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,
}

/// Run the hook; returns the process exit code.
#[must_use]
pub fn run(args: &HookArgs) -> i32 {
    let stdin = if matches!(args.name, HookName::DelegateTurn) && args.state_dir.is_some() {
        match read_bounded_utf8(std::io::stdin(), MAX_SCHEMA_HOOK_INPUT_BYTES) {
            Ok(stdin) => stdin,
            Err(error) => {
                // A schema-v2 input that cannot be inspected must not advance
                // the harness or the lifecycle. Exit 2 is the fail-closed hook
                // outcome; terminal callers recover through the bounded waiter.
                eprintln!("codeflow delegate-turn: {error}");
                return 2;
            }
        }
    } else {
        let mut stdin = String::new();
        // Preserve the existing advisory behavior for every legacy hook.
        let _ = std::io::stdin().read_to_string(&mut stdin);
        stdin
    };

    match args.name {
        HookName::GitGuard => git_guard(&stdin),
        HookName::ExecGuard => exec_guard(&stdin),
        HookName::SessionOrient => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
            print!("{}", orient::generate(&super::project_root(&cwd)));
            0
        }
        HookName::SessionSummary => session_summary(&stdin),
        HookName::DelegateTurn => delegate_turn(args, &stdin),
    }
}

fn read_bounded_utf8(reader: impl Read, max_bytes: usize) -> Result<String, &'static str> {
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    reader
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read schema-v2 hook payload")?;
    if bytes.len() > max_bytes {
        return Err("schema-v2 hook payload exceeds the 32 MiB input limit");
    }
    String::from_utf8(bytes).map_err(|_| "schema-v2 hook payload is not valid UTF-8")
}

fn delegate_turn(args: &HookArgs, stdin: &str) -> i32 {
    let Some(run_id) = args.run_id.clone() else {
        eprintln!("codeflow delegate-turn: --run-id is required");
        return 1;
    };
    match (&args.result, &args.state_dir) {
        (Some(result), None) => {
            let config = delegate_turn::TurnConfig {
                run_id,
                result: result.clone(),
            };
            match delegate_turn::record_and_signal(&config, stdin) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("codeflow delegate-turn: {error}");
                    1
                }
            }
        }
        (None, Some(state_dir)) => {
            match codeflow_core::delegate::handle_hook(&run_id, state_dir, stdin) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("codeflow delegate-turn: {error}");
                    if codeflow_core::delegate::is_prompt_submission(stdin) {
                        2
                    } else {
                        1
                    }
                }
            }
        }
        _ => {
            eprintln!(
                "codeflow delegate-turn: --result is required unless --state-dir is used; \
                 exactly one must be provided"
            );
            1
        }
    }
}

fn git_guard(stdin: &str) -> i32 {
    let payload = match git_guard::HookPayload::parse(stdin) {
        Ok(p) => p,
        Err(e) => {
            // Fail open with a visible warning: a malformed payload must not
            // veto every shell call (charter principle 8 — legible, not silent).
            eprintln!("codeflow git-guard: warning: unreadable hook payload ({e}); allowing");
            return 0;
        }
    };
    let Some(command) = payload.shell_command() else {
        return 0; // not a supported shell tool call
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

    let lookup = gh_pr_base;
    // Resolve a retargeted directory (`-C`/`--git-dir`/`GIT_DIR`/`cd`) to the
    // branch checked out there, so a wrong-dir git op is judged against the
    // target repo, not the session (charter §6.1).
    let dir_cwd = cwd.clone();
    let dir_branch = move |dir: &str| resolve_dir_branch(&dir_cwd, dir);
    let ctx = git_guard::GuardContext {
        policy: &policy.git,
        current_branch: &branch,
        integrate_token: super::integrate_token_present(),
        pr_base_lookup: Some(&lookup),
        dir_branch_lookup: Some(&dir_branch),
    };
    let violations = git_guard::evaluate(command, &ctx);
    super::render_outcome("git-guard", &violations, &[], 2)
}

/// Resolve a `-C`/`--git-dir`/`GIT_DIR`/`cd` target `dir` (relative to the
/// session `cwd`) to the branch checked out there. `--git-dir` may point at a
/// `.git` directory; discovery from its parent handles that. `None` when the
/// branch cannot be read — the guard then falls back to the session branch.
fn resolve_dir_branch(cwd: &std::path::Path, dir: &str) -> Option<String> {
    let p = std::path::Path::new(dir);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let start = if abs.file_name().is_some_and(|n| n == ".git") {
        abs.parent()
            .map_or(abs.clone(), std::path::Path::to_path_buf)
    } else {
        abs
    };
    codeflow_core::hooks::RepoInfo::discover(&start)
        .map(|i| i.branch)
        .filter(|b| !b.is_empty())
}

/// `exec-guard` (`PreToolUse` Bash/PowerShell): run the dangerous/privilege security
/// modules against the command per the `security` policy section (ADR-0008).
///
/// Payload parsing reuses [`git_guard::HookPayload`], which is lenient by
/// construction (serde ignores extra harness fields and accepts Grok camelCase
/// aliases), so this one handler serves Claude, Codex, and Grok Build.
fn exec_guard(stdin: &str) -> i32 {
    let payload = match git_guard::HookPayload::parse(stdin) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("codeflow exec-guard: warning: unreadable hook payload ({e}); allowing");
            return 0;
        }
    };
    let Some(command) = payload.shell_command() else {
        return 0; // not a supported shell tool call
    };

    let cwd = payload
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into());
    let root = super::project_root(&cwd);
    // The `security` section is not touched by bootstrap grace (like
    // `secret_scan`, a dangerous command is never graced), so a plain load is
    // enough — no need for `load_effective`.
    let policy = Policy::load(&root);
    let violations = exec_guard::evaluate(command, &policy.security);
    super::render_outcome("exec-guard", &violations, &[], 2)
}

/// Resolve a `gh pr merge <arg>` target to its base branch via `gh pr view`
/// (empty `arg` = the current branch's PR). Bounded (5s) and best-effort:
/// any failure returns `None`, which the guard treats as doubt and blocks.
fn gh_pr_base(arg: &str) -> Option<String> {
    let arg = arg.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(gh_pr_base_blocking(&arg));
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or(None)
}

fn gh_pr_base_blocking(arg: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["pr", "view"]);
    if !arg.is_empty() {
        cmd.arg(arg);
    }
    cmd.args(["--json", "baseRefName", "-q", ".baseRefName"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let base = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!base.is_empty()).then_some(base)
}

fn session_summary(stdin: &str) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match session_summary::record(&super::project_root(&cwd), stdin) {
        Ok(path) => {
            eprintln!("codeflow session-summary: recorded to {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("codeflow session-summary: warning: {e} — session unaffected");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_bounded_utf8;
    use std::io::Cursor;

    #[test]
    fn schema_hook_input_is_bounded_and_requires_utf8() {
        assert_eq!(
            read_bounded_utf8(Cursor::new(b"hello"), 5).unwrap(),
            "hello"
        );
        assert!(read_bounded_utf8(Cursor::new(b"longer"), 5)
            .unwrap_err()
            .contains("input limit"));
        assert!(read_bounded_utf8(Cursor::new([0xff]), 5)
            .unwrap_err()
            .contains("UTF-8"));
    }
}
