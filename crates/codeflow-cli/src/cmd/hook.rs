//! `codeflow hook <git-guard|exec-guard|session-orient|session-summary>` — the
//! Claude layer hooks, wired by the settings presets (charter §3.3).
//!
//! Exit-code contract:
//! - `git-guard`: 0 allow, 2 block (`PreToolUse` deny) with the violated rule
//!   and sanctioned path on stderr.
//! - `exec-guard`: 0 allow (or warn), 2 block — same `PreToolUse` (Bash)
//!   contract, enforcing the `security` policy section (ADR-0008). Payload is
//!   parsed leniently so the same subcommand serves the Codex hooks engine.
//! - `session-orient`: digest on stdout, always 0.
//! - `session-summary`: always 0 — a failed summary must never fail the
//!   session (warn on stderr instead).

use std::io::Read;

use clap::Args;
use codeflow_core::hooks::{exec_guard, git_guard, orient, policy::Policy, session_summary};

/// Which Claude-layer hook to run.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum HookName {
    /// `PreToolUse` (Bash): enforce policy.json git rules in-session.
    GitGuard,
    /// `PreToolUse` (Bash): enforce policy.json `security` rules (dangerous
    /// commands, privilege escalation). Harness-agnostic — also serves Codex.
    ExecGuard,
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
        HookName::ExecGuard => exec_guard(&stdin),
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
    let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let start = if abs.file_name().is_some_and(|n| n == ".git") {
        abs.parent().map_or(abs.clone(), std::path::Path::to_path_buf)
    } else {
        abs
    };
    codeflow_core::hooks::RepoInfo::discover(&start)
        .map(|i| i.branch)
        .filter(|b| !b.is_empty())
}

/// `exec-guard` (`PreToolUse` Bash): run the dangerous/privilege security
/// modules against the command per the `security` policy section (ADR-0008).
///
/// Payload parsing reuses [`git_guard::HookPayload`], which is lenient by
/// construction (serde ignores the extra `turn_id`/`model`/`permission_mode`
/// fields and the nullable `transcript_path` a Codex payload carries), so this
/// one handler serves both the Claude and Codex hooks engines unchanged.
fn exec_guard(stdin: &str) -> i32 {
    let payload = match git_guard::HookPayload::parse(stdin) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("codeflow exec-guard: warning: unreadable hook payload ({e}); allowing");
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
