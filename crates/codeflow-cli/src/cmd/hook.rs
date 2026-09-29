//! `codeflow hook <git-guard|exec-guard|session-orient|prompt-reminder|session-summary|delegate-turn>` — the
//! Claude layer hooks, wired by the settings presets (charter §3.3).
//!
//! Exit-code contract:
//! - `git-guard`: 0 allow, 2 block (`PreToolUse` deny) with the violated rule
//!   and sanctioned path on stderr.
//! - `exec-guard`: 0 allow (or warn), 2 block — same `PreToolUse` shell
//!   contract, enforcing the `security` policy section (ADR-0008). Payload is
//!   parsed leniently so the same subcommand serves the Codex hooks engine.
//! - `session-orient`: the stable advisory entry, always 0, dispatched on the
//!   payload's `hook_event_name` (TSK-128). `SessionStart` (or no event
//!   named): the digest, plus the rule guidance block after a compaction, a
//!   resume or a fork. `UserPromptSubmit`: at most one rule line. Any other
//!   event: nothing. Harnesses wire both events to this one command, so an
//!   older binary that knows only this name still exits 0 on a prompt.
//! - `prompt-reminder`: the prompt line alone, always 0; a convenience for
//!   manual use, never wired into a harness.
//! - `session-summary`: always 0 — a failed summary must never fail the
//!   session (warn on stderr instead).
//! - `delegate-turn`: schema-v2 state mode handles the full lifecycle without
//!   tmux; legacy result mode preserves its existing terminal signal contract.

use std::io::{Read, Write as _};
use std::path::PathBuf;

use clap::{ArgGroup, Args};
use codeflow_core::hooks::{
    delegate_turn, exec_guard, git_guard, guidance, orient, policy::Policy, session_summary,
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
    /// `SessionStart` and `UserPromptSubmit`: the advisory entry, dispatched
    /// on the payload's event (the digest and guidance, or one rule line).
    SessionOrient,
    /// The one advisory rule line for a prompt payload; not wired into any
    /// harness (the wired entry is `session-orient`).
    PromptReminder,
    /// `SessionEnd`: append the session record to the ledger.
    SessionSummary,
    /// `SessionStart`, `UserPromptSubmit`, `Stop` and `StopFailure`: with
    /// `--state-dir`, advance the schema-v2 delegate lifecycle (ready, armed
    /// prompt or task-notice continuation, terminal result); with `--result`,
    /// the legacy one-shot mode that records and signals a `Stop` or
    /// `StopFailure`.
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
    /// Absolute owner-only result path for legacy one-shot `delegate-turn`.
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
        HookName::SessionOrient => session_orient(&stdin),
        HookName::PromptReminder => {
            prompt_reminder(&stdin);
            0
        }
        HookName::SessionSummary => session_summary(&stdin),
        HookName::DelegateTurn => delegate_turn(args, &stdin),
    }
}

/// The advisory entry: dispatch on the payload's event. Every path exits 0,
/// and output is written without panicking, so a closed stdout cannot turn
/// advice into a failed session or a refused prompt. The guards never pass
/// through here.
fn session_orient(stdin: &str) -> i32 {
    match guidance::payload_event(stdin) {
        guidance::HookEvent::SessionStart => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let root = super::project_root(&cwd);
            let digest = orient::generate(&root);
            let mut out = std::io::stdout();
            let _ = write!(out, "{digest}");
            // The digest's own off-switch also silences the guidance block.
            if !digest.is_empty() {
                let source = guidance::payload_source(stdin).unwrap_or_default();
                if let Some(block) = guidance::session_guidance(&root, &source) {
                    let _ = write!(out, "\n{block}");
                }
            }
            let _ = out.flush();
        }
        guidance::HookEvent::PromptSubmit => prompt_reminder(stdin),
        guidance::HookEvent::Other(_) => {}
    }
    0
}

/// Write the prompt's one rule line, if any; a write error is dropped.
fn prompt_reminder(stdin: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    if let Some(line) = guidance::prompt_reminder(&super::project_root(&cwd), stdin) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
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
            let finding = payload_finding("git-guard", &e);
            eprintln!("{}", finding.line("codeflow git-guard", "warning"));
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
    // The session's root checkout, when the command runs in one (TSK-165).
    let root_checkout = codeflow_core::root_checkout::RootCheckout::at(&root, &policy.git);

    let lookup = gh_pr_base;
    // Resolve a retargeted repository (`-C`/`--git-dir`/`GIT_DIR`/`cd`) to its
    // branch and policy, so a git op is judged by the repository it targets,
    // not the session's (charter §6.1; TSK-112).
    let session_common = codeflow_core::hooks::RepoInfo::discover(&root).map(|i| i.common_dir);
    let dir_cwd = cwd.clone();
    let dir_target = move |spec: &git_guard::Retarget<'_>| {
        git_guard::read_target(&dir_cwd, session_common.as_deref(), spec)
    };
    // Resolve a subcommand that is not a builtin through the alias it names,
    // as git reads it where the command runs (TSK-112).
    let alias_cwd = cwd.clone();
    let alias = move |query: &git_guard::AliasQuery<'_>| git_guard::read_alias(&alias_cwd, query);
    let ctx = git_guard::GuardContext {
        policy: &policy.git,
        current_branch: &branch,
        integrate_token: super::integrate_token_present(),
        pr_base_lookup: Some(&lookup),
        dir_target_lookup: Some(&dir_target),
        alias_lookup: Some(&alias),
        root_checkout: root_checkout.as_ref(),
    };
    let report = git_guard::evaluate_report(command, &ctx);
    super::render_outcome("git-guard", &report.violations, &report.notes, 2)
}

/// The finding for a guard input it could not read. Fail open with a
/// visible warning: an unread payload must not veto every shell call
/// (charter principle 8: legible, not silent).
fn payload_finding(guard: &str, error: &git_guard::PayloadError) -> codeflow_core::remedy::Finding {
    let remedy = codeflow_core::remedy::GUARD_PAYLOAD_MALFORMED
        .with(&[("guard", guard), ("path", HARNESS_HOOK_FILES)]);
    codeflow_core::remedy::Finding::new(
        format!("unreadable hook payload ({error}); allowing"),
        remedy,
    )
}

/// Where each harness wires the guards.
const HARNESS_HOOK_FILES: &str = "`.claude/settings.json`, `.codex/hooks.json` or `.grok/hooks/`";

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
            let finding = payload_finding("exec-guard", &e);
            eprintln!("{}", finding.line("codeflow exec-guard", "warning"));
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
        Ok(Some(path)) => {
            eprintln!("codeflow session-summary: recorded to {}", path.display());
            0
        }
        Ok(None) => {
            eprintln!("codeflow session-summary: not in a git repository, nothing to record");
            0
        }
        Err(e) => {
            let path = e.path.display().to_string();
            let finding = codeflow_core::remedy::Finding::new(
                format!(
                    "session ledger not written ({}); session unaffected",
                    e.cause
                ),
                codeflow_core::remedy::SESSION_SUMMARY_UNWRITTEN
                    .with(&[("repair", e.repair.words()), ("path", &path)]),
            );
            eprintln!("{}", finding.line("codeflow session-summary", "warning"));
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
