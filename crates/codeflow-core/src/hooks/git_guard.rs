//! `git-guard` — the `PreToolUse` shell hook (charter §3.3, §6.1 plane 2).
//!
//! Intercepts git operations the client-side git hooks can't reach:
//! force-push / push / delete against protected branches, hard reset on a
//! protected branch, checkout-and-commit dodges, raw merges on protected,
//! and AI attribution or emoji in `gh pr create` bodies (charter §6.4,
//! AC #13). Every rule reads its level from `policy.json.git` — the guard
//! gives instant in-session feedback; CI + remote protection stay the hard
//! line (D19).

use std::path::PathBuf;

use serde::Deserialize;

use crate::security::pattern::is_path_targeted;

use super::policy::{GitPolicy, PolicyLevel};
use super::{standards, Violation, HUMAN_OVERRIDE_ENV, INTEGRATE_TOKEN_ENV};

/// Resolves a `gh pr merge <target>` argument (a PR number, URL, or branch;
/// empty means the current branch's PR) to its base branch name, when it can
/// be determined. Returns `None` on any doubt — an unreachable API, an
/// unrecognized argument — so the guard blocks conservatively. The CLI wires a
/// bounded `gh pr view` implementation; tests inject a stub.
pub type PrBaseLookup<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// Resolves a directory (a `-C`/`--git-dir`/`GIT_DIR`/`cd` target, possibly
/// relative to the session cwd) to the branch checked out there, so a git op
/// retargeted at another repository is evaluated against *that* repo's branch,
/// not the session's (charter §6.1; the review's wrong-dir evasion). `None`
/// means the branch could not be read; the guard then falls back to the session
/// branch (documented residual — the git-hook plane in the target repo is the
/// backstop). The CLI wires a `.git/HEAD` reader; tests inject a stub.
pub type DirBranchLookup<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// Parsed Claude Code `PreToolUse` hook payload (the fields the guard reads).
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: ToolInput,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// `tool_input` for shell invocations.
#[derive(Debug, Default, Deserialize)]
pub struct ToolInput {
    #[serde(default)]
    pub command: Option<String>,
}

impl HookPayload {
    /// Parse the hook JSON from stdin.
    ///
    /// # Errors
    ///
    /// Returns the serde error message when the payload is not valid JSON.
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// The command to evaluate when this is a Bash or `PowerShell` tool call.
    /// Claude exposes `PowerShell` as a distinct tool on native Windows; Codex
    /// currently sends the same command shape under its shell hook.
    #[must_use]
    pub fn shell_command(&self) -> Option<&str> {
        if matches!(self.tool_name.as_str(), "Bash" | "PowerShell") {
            self.tool_input.command.as_deref()
        } else {
            None
        }
    }
}

/// Evaluation context for a guard run.
pub struct GuardContext<'a> {
    /// The `git` policy section in force.
    pub policy: &'a GitPolicy,
    /// Branch checked out where the command will run (empty when unknown).
    pub current_branch: &'a str,
    /// `true` when the `codeflow integrate` gate-context token is present —
    /// the sanctioned local merge path (charter §6.2). Note: `HUMAN_OVERRIDE`
    /// is deliberately NOT a field here — the Claude layer never honors it
    /// (ADR-0007); an agent does not get to claim humanity.
    pub integrate_token: bool,
    /// How to resolve a `gh pr merge` target to its base branch (injected).
    pub pr_base_lookup: PrBaseLookup<'a>,
    /// How to resolve a retargeted directory (`-C`/`--git-dir`/`GIT_DIR`/`cd`)
    /// to the branch checked out there (injected). `None` disables cross-repo
    /// resolution and the guard evaluates against `current_branch` — the
    /// behavior in unit tests that do not exercise retargeting.
    pub dir_branch_lookup: DirBranchLookup<'a>,
}

/// Evaluate a Bash command against the git policy.
///
/// Returns all violations found; the caller maps block-level violations to
/// exit 2 (deny) and warn-level to stderr advice.
#[must_use]
pub fn evaluate(command: &str, ctx: &GuardContext<'_>) -> Vec<Violation> {
    let mut violations = Vec::new();
    // Chained checkout/switch dodges change the branch later segments run on.
    let mut branch = ctx.current_branch.to_string();
    // A `cd <dir>` earlier in the chain retargets subsequent git ops.
    let mut cd_dir: Option<String> = None;

    // `expand_commands` unwraps the shell constructs an agent can hide a git
    // token behind — subshells `( )`, brace groups `{ }`, `bash -c '…'`,
    // `$(…)`/backticks, newlines, and backgrounding `&` — so a git/gh
    // invocation is evaluated wherever it sits, not only as a segment's first
    // word (the review's wrapper evasions).
    for segment in expand_commands(command) {
        let tokens = shell_tokens(&segment);
        if tokens.is_empty() {
            continue;
        }

        // Hook/policy integrity: writes or removes that would disarm or tamper
        // with the enforcement plane, evaluated on ANY command (not just git).
        if ctx.policy.hook_integrity.is_active() {
            if let Some(v) = integrity_write_violation(&tokens, ctx.policy.hook_integrity) {
                violations.push(v);
                continue;
            }
        }

        // Anti-laundering: setting an override token in-session is bypass, not
        // override — block it before evaluating whatever it prefixed.
        if let Some(var) = laundered_override(&tokens) {
            violations.push(laundering_violation(var));
            continue;
        }

        // Hook-skip env prefixes (`GIT_SKIP_HOOKS=`, `HUSKY=0`, …) disarm the
        // client hooks; block regardless of the branch (charter §6.1, ADR-0009).
        if ctx.policy.hook_integrity.is_active() {
            if let Some(var) = hook_skip_env(&tokens) {
                violations.push(hook_integrity_violation(
                    ctx.policy.hook_integrity,
                    format!("command sets the hook-skip env var `{var}`"),
                ));
                continue;
            }
        }

        // A `cd <dir>` (as its own simple command) retargets later git ops.
        if let Some(dir) = cd_target(&tokens) {
            cd_dir = Some(dir);
            continue;
        }

        // GIT_CONFIG_* env injection of a protected config key (parallel to the
        // hook-skip env vars) — `GIT_CONFIG_KEY_0=core.hooksPath …`.
        if ctx.policy.hook_integrity.is_active() && git_config_env_sets_hooks_path(&tokens) {
            violations.push(hook_integrity_violation(
                ctx.policy.hook_integrity,
                "command injects core.hooksPath via GIT_CONFIG_* env".to_string(),
            ));
            continue;
        }

        let Some((program, args)) = strip_launchers(&tokens) else {
            continue;
        };
        let git_dir_env = git_dir_env_prefix(&tokens);
        match program_kind(program) {
            ProgramKind::Git => {
                check_git(
                    args,
                    &mut branch,
                    cd_dir.as_deref(),
                    git_dir_env,
                    ctx,
                    &mut violations,
                );
            }
            ProgramKind::Gh => check_gh(args, ctx, &mut violations),
            ProgramKind::Other => {}
        }
    }
    violations
}

/// Which program a segment invokes, for dispatch. The basename is taken so a
/// path form (`/usr/bin/git`) is still recognized.
enum ProgramKind {
    Git,
    Gh,
    Other,
}

fn program_kind(program: &str) -> ProgramKind {
    match basename(program) {
        "git" => ProgramKind::Git,
        "gh" => ProgramKind::Gh,
        _ => ProgramKind::Other,
    }
}

/// The final path component of a token (`/usr/bin/git` -> `git`).
fn basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

/// The override env vars a human may set in their own terminal but an agent
/// must never set in-session (ADR-0007): the human override and the integrate
/// gate token.
const OVERRIDE_ENV_VARS: &[&str] = &[HUMAN_OVERRIDE_ENV, INTEGRATE_TOKEN_ENV];

/// Detect an in-session attempt to set an override token. Covers the env-prefix
/// assignment (`VAR=1 git …`); `export VAR[=…]`; the `env` family including a
/// path form (`env`, `/usr/bin/env`) and a `command`/`builtin` prefix
/// (`command env VAR=…`); the shell declaration builtins (`declare -x`,
/// `typeset`, `readonly`, `local`); and an override carried inside a
/// `git config alias.*` value (`git config alias.x '!VAR=1 git …'`). These are
/// floor-raises, not a solve — an env var is not authentication (ADR-0009);
/// a determined agent has other ways to set its own environment.
fn laundered_override(tokens: &[String]) -> Option<&'static str> {
    let assigns = |t: &str| {
        OVERRIDE_ENV_VARS
            .iter()
            .copied()
            .find(|v| is_assignment_of(t, v))
    };
    let names = |t: &str| OVERRIDE_ENV_VARS.iter().copied().find(|v| **v == *t);

    // An override var carried inside any token (e.g. a `git config alias.*`
    // value `!CODEFLOW_HUMAN_OVERRIDE=1 git …`) — the assignment is embedded,
    // not a leading prefix, so the positional scan below would miss it.
    if tokens.first().map(|t| basename(t)) == Some("git") && tokens.iter().any(|t| t == "config") {
        if let Some(var) = tokens.iter().find_map(|t| embedded_override_assignment(t)) {
            return Some(var);
        }
    }

    // Leading env-prefix assignments and the env/declare/command builtins.
    let mut idx = 0;
    while idx < tokens.len() {
        let t = tokens[idx].as_str();
        if let Some(var) = assigns(t) {
            return Some(var); // VAR=... as an env-prefix
        }
        if t.contains('=') && is_identifier(t.split_once('=').map_or("", |(n, _)| n)) {
            idx += 1; // some other harmless assignment prefix — keep scanning
            continue;
        }
        // `command`/`builtin` just prefix another simple command — skip and
        // re-examine the word they wrap (`command env VAR=…`).
        if t == "command" || t == "builtin" {
            idx += 1;
            continue;
        }
        // `env` (any path form), `export`, and the declaration builtins all
        // carry the assignment in their arguments.
        let word = basename(t);
        if word == "env" || t == "export" || is_declare_builtin(t) {
            for arg in &tokens[idx + 1..] {
                if let Some(var) = assigns(arg) {
                    return Some(var);
                }
                // `export VAR` (no value) still primes a later assignment.
                if (t == "export" || is_declare_builtin(t)) && names(arg).is_some() {
                    return names(arg);
                }
            }
        }
        break;
    }
    None
}

/// `true` for the shell declaration builtins that can set/export a variable.
fn is_declare_builtin(t: &str) -> bool {
    matches!(t, "declare" | "typeset" | "readonly" | "local")
}

/// If `token` contains `<OVERRIDE_VAR>=` anywhere (not only as a leading
/// prefix), return that variable — for override tokens smuggled inside a quoted
/// value such as a git alias body.
fn embedded_override_assignment(token: &str) -> Option<&'static str> {
    OVERRIDE_ENV_VARS
        .iter()
        .copied()
        .find(|v| token.contains(&format!("{v}=")))
}

/// `true` when `token` is `VAR=<anything>` for exactly `var`.
fn is_assignment_of(token: &str, var: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| name == var)
}

fn laundering_violation(var: &str) -> Violation {
    Violation::new(
        "git.override_token_laundering",
        PolicyLevel::Block,
        format!("command sets the override token `{var}` in-session"),
        "override tokens are human-only; setting them in-session is bypass — a human runs the sanctioned path (PR merge / `codeflow integrate` / `CODEFLOW_HUMAN_OVERRIDE`) from their own terminal".to_string(),
    )
}

fn hook_integrity_violation(level: PolicyLevel, message: String) -> Violation {
    Violation::new(
        "git.hook_integrity",
        level,
        message,
        "the enforcement hooks and their policy are not agent-editable — fix the cause a gate flags rather than disabling it; hooks and policy change through a human or `codeflow update` (ADR-0009)".to_string(),
    )
}

// ---------------------------------------------------------------------------
// hook / policy integrity (task: HOOK-PLANE SELF-DISARM)
// ---------------------------------------------------------------------------

/// `true` when `s` names the `core.hooksPath` config key. Git config
/// section+name are case-insensitive (`core.hookspath` sets the same key), so
/// the comparison is too — matching a spelling, not a case.
fn mentions_hooks_path(s: &str) -> bool {
    s.to_ascii_lowercase().contains("core.hookspath")
}

/// Collect the leading `VAR=val` assignments of a simple command — the env
/// prefixes plus those carried by `env`/`export`/`declare`-family/`command`.
fn leading_env_assignments(tokens: &[String]) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < tokens.len() {
        let t = tokens[idx].as_str();
        if let Some((n, v)) = t.split_once('=') {
            if is_identifier(n) {
                out.push((n, v));
                idx += 1;
                continue;
            }
        }
        break;
    }
    if let Some(t) = tokens.get(idx) {
        let word = basename(t);
        if word == "env" || t == "export" || is_declare_builtin(t) || t == "command" {
            for a in &tokens[idx + 1..] {
                if let Some((n, v)) = a.split_once('=') {
                    if is_identifier(n) {
                        out.push((n, v));
                    }
                }
            }
        }
    }
    out
}

/// Detect a `core.hooksPath` set through git's `GIT_CONFIG_*` env mechanism —
/// `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=…` or
/// the older `GIT_CONFIG_PARAMETERS='core.hooksPath=…'`. Parallel to the
/// hook-skip env vars: an env-set of a protected key disarms the plane.
fn git_config_env_sets_hooks_path(tokens: &[String]) -> bool {
    leading_env_assignments(tokens)
        .into_iter()
        .any(|(name, val)| {
            (name.starts_with("GIT_CONFIG_KEY_") || name == "GIT_CONFIG_PARAMETERS")
                && mentions_hooks_path(val)
        })
}

/// Env vars whose in-session assignment disables the client hooks. `HUSKY`
/// only disarms at `=0`; the rest disarm at any value.
const HOOK_SKIP_ENV_VARS: &[&str] = &[
    "GIT_SKIP_HOOKS",
    "SKIP_HOOKS",
    "PRE_COMMIT_ALLOW_NO_CONFIG",
    "GIT_HOOKS_PATH",
];

/// Detect a hook-skip env var set as a leading prefix or via the env/declare
/// builtins (`GIT_SKIP_HOOKS=1 git …`, `HUSKY=0 …`, `env HUSKY=0 …`).
fn hook_skip_env(tokens: &[String]) -> Option<&'static str> {
    let matches_var = |t: &str| -> Option<&'static str> {
        let (name, val) = t.split_once('=')?;
        if name == "HUSKY" && val == "0" {
            return Some("HUSKY");
        }
        HOOK_SKIP_ENV_VARS.iter().copied().find(|v| **v == *name)
    };

    let mut idx = 0;
    while idx < tokens.len() {
        let t = tokens[idx].as_str();
        if let Some(v) = matches_var(t) {
            return Some(v);
        }
        if t.contains('=') && is_identifier(t.split_once('=').map_or("", |(n, _)| n)) {
            idx += 1;
            continue;
        }
        if t == "command" || t == "builtin" {
            idx += 1;
            continue;
        }
        let word = basename(t);
        if word == "env" || t == "export" || is_declare_builtin(t) {
            for arg in &tokens[idx + 1..] {
                if let Some(v) = matches_var(arg) {
                    return Some(v);
                }
            }
        }
        break;
    }
    None
}

/// A `cd <dir>`/`pushd <dir>` simple command that retargets later git ops.
fn cd_target(tokens: &[String]) -> Option<String> {
    let first = tokens.first()?.as_str();
    if first != "cd" && first != "pushd" {
        return None;
    }
    tokens[1..]
        .iter()
        .find(|t| !t.starts_with('-'))
        .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|d| !d.is_empty())
}

/// A leading `GIT_DIR=<dir>` env-prefix assignment, when present.
fn git_dir_env_prefix(tokens: &[String]) -> Option<String> {
    for t in tokens {
        let (name, val) = t.split_once('=')?;
        if !is_identifier(name) {
            break;
        }
        if name == "GIT_DIR" {
            return Some(val.to_string());
        }
    }
    None
}

/// The hook shims and the integrity files: directory prefixes and exact files
/// whose mutation would disarm or falsify the enforcement plane.
const INTEGRITY_PREFIXES: &[&str] = &[".git/hooks", ".codeflow/git-hooks"];
const INTEGRITY_FILES: &[&str] = &[".codeflow/policy.json", ".codeflow/project.toml"];

/// Collapse the path spellings that name the same file — `//`, `/./`, a
/// trailing `/`, and a leading `./` — so a matcher compares the real path, not
/// a literal contiguous substring (the review's `.codeflow//policy.json` /
/// `.codeflow/./policy.json` / `.git//hooks` variants). `..` is deliberately
/// left unresolved (traversal is a separate, documented residual).
fn normalize_path(s: &str) -> String {
    let absolute = s.starts_with('/');
    let parts: Vec<&str> = s
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The integrity path a single argument token names, when any. The token is
/// normalized first so equivalent spellings match.
fn token_integrity_path(token: &str) -> Option<&'static str> {
    let norm = normalize_path(token);
    INTEGRITY_PREFIXES
        .iter()
        .chain(INTEGRITY_FILES.iter())
        .copied()
        .find(|p| is_path_targeted(&norm, p))
}

/// The integrity path named by any argument in `args`.
fn arg_integrity_path(args: &[String]) -> Option<&'static str> {
    args.iter().find_map(|a| token_integrity_path(a))
}

/// Where a write-redirect operator's target sits.
enum RedirectTarget<'a> {
    /// Attached to the operator (`>policy.json`).
    Attached(&'a str),
    /// The following token (`> policy.json`).
    Next,
}

/// If `token` is a write-redirect operator, classify where its target is.
/// Handles an optional leading file-descriptor number (`1>`, `2>>`), clobber
/// `>|`, append `>>`, and the both-streams forms (`&>file`, `>&file`) — a
/// *shape*, not an enumerated set. A `>&`/`&>` followed by a digit or `-`
/// (`2>&1`, `>&-`) duplicates or closes an fd and is not a file write; a `>&`
/// followed by a filename (csh/bash `>&file`) writes both streams to it.
fn redirect_target(token: &str) -> Option<RedirectTarget<'_>> {
    // `&>file` / `&>>file`: bash redirect of both stdout and stderr to a file.
    if let Some(rest) = token
        .strip_prefix("&>>")
        .or_else(|| token.strip_prefix("&>"))
    {
        return Some(classify_redirect_rest(rest));
    }
    let after_fd = token.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = after_fd
        .strip_prefix(">>")
        .or_else(|| after_fd.strip_prefix(">|"))
        .or_else(|| after_fd.strip_prefix('>'))?;
    if let Some(after_amp) = rest.strip_prefix('&') {
        // `>&1` / `>&-` duplicate or close an fd; `>&file` is a write.
        return match after_amp.chars().next() {
            Some(c) if c.is_ascii_digit() || c == '-' => None,
            None => Some(RedirectTarget::Next),
            Some(_) => Some(RedirectTarget::Attached(after_amp)),
        };
    }
    Some(classify_redirect_rest(rest))
}

/// A redirect operator's trailing text names its target inline (`>file`), or the
/// operator stands alone and the next token is the target (`> file`).
fn classify_redirect_rest(rest: &str) -> RedirectTarget<'_> {
    if rest.is_empty() {
        RedirectTarget::Next
    } else {
        RedirectTarget::Attached(rest)
    }
}

/// A write redirect (`>`, `>>`, `>|`, `1>`, `2>>`, …) whose target is an
/// integrity path, from the token stream — target attached (`>policy.json`) or
/// the next token (`> policy.json`).
fn redirect_integrity_path(tokens: &[String]) -> Option<&'static str> {
    let mut i = 0;
    while i < tokens.len() {
        match redirect_target(&tokens[i]) {
            Some(RedirectTarget::Attached(t)) => {
                if let Some(p) = token_integrity_path(t) {
                    return Some(p);
                }
            }
            Some(RedirectTarget::Next) => {
                if let Some(p) = tokens.get(i + 1).and_then(|n| token_integrity_path(n)) {
                    return Some(p);
                }
            }
            None => {}
        }
        i += 1;
    }
    None
}

/// Block a Bash write/remove that would disarm or falsify the enforcement
/// plane: a redirect into, or a mutating command targeting, the hook shims
/// (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
/// (`.codeflow/policy.json`, `.codeflow/project.toml`). Reads (`cat`, a `cp`
/// *from* an integrity path) stay allowed.
fn integrity_write_violation(tokens: &[String], level: PolicyLevel) -> Option<Violation> {
    if let Some(p) = redirect_integrity_path(tokens) {
        return Some(hook_integrity_violation(
            level,
            format!("redirect would overwrite the integrity path `{p}`"),
        ));
    }
    let (program, args) = strip_launchers(tokens)?;
    let cmd = basename(program);

    if matches!(
        cmd,
        "rm" | "unlink"
            | "mv"
            | "tee"
            | "dd"
            | "truncate"
            | "shred"
            | "chmod"
            | "chown"
            | "ln"
            | "install"
    ) {
        if let Some(p) = arg_integrity_path(args) {
            return Some(hook_integrity_violation(
                level,
                format!("`{cmd}` targets the integrity path `{p}`"),
            ));
        }
    }
    if cmd == "sed" && args.iter().any(|a| a == "-i" || a.starts_with("-i")) {
        if let Some(p) = arg_integrity_path(args) {
            return Some(hook_integrity_violation(
                level,
                format!("`sed -i` edits the integrity path `{p}`"),
            ));
        }
    }
    if cmd == "cp" {
        // Only a write matters: the integrity path as the destination (the last
        // non-flag argument). A `cp` *from* an integrity path is a read.
        if let Some(dest) = args.iter().rev().find(|a| !a.starts_with('-')) {
            if let Some(p) = token_integrity_path(dest) {
                return Some(hook_integrity_violation(
                    level,
                    format!("`cp` writes the integrity path `{p}`"),
                ));
            }
        }
    }
    if cmd == "git" && matches!(args.first().map(String::as_str), Some("rm" | "mv")) {
        if let Some(p) = arg_integrity_path(&args[1..]) {
            return Some(hook_integrity_violation(
                level,
                format!("`git {}` removes the integrity path `{p}`", args[0]),
            ));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// command expansion (task: TOKEN DETECTION HARDENING — wrapper evasions)
// ---------------------------------------------------------------------------

/// Expand a raw command into candidate simple-command strings, unwrapping the
/// shell constructs an agent can hide a `git`/`gh` token behind so it is not
/// only the first word of a `&&`/`||`/`;`/`|` segment that is inspected:
/// newlines, backgrounding `&`, subshells `( )`, brace groups `{ }`,
/// `$(…)`/backtick command substitution, `bash -c '…'`, and `eval '…'`. Program
/// resolution goes through [`strip_launchers`], so an env/`command` prefix on
/// the wrapper (`env FOO=1 bash -c …`) and a clustered short flag (`bash -lc`)
/// are both handled. A floor-raise, not a solve — arbitrary interpreters
/// (`python3 -c`) and pipe-to-shell (`echo … | sh`) are the genuinely unbounded
/// tail and stay a documented residual (ADR-0009), backstopped by CI + remote.
fn expand_commands(command: &str) -> Vec<String> {
    let mut raw = Vec::new();
    split_into_segments(command, &mut raw, 0);

    let mut out = Vec::new();
    for seg in raw {
        let toks = shell_tokens(&seg);
        out.push(seg);
        let Some((prog, args)) = strip_launchers(&toks) else {
            continue;
        };
        let name = basename(prog);
        if is_shell(name) {
            // `-c`/`--command`, or a clustered short flag containing `c`
            // (`-lc`, `-ec`): the wrapped command is the following argument.
            if let Some(inner) = shell_c_argument(args) {
                split_into_segments(inner, &mut out, 1);
            }
        } else if name == "eval" {
            // `eval '<cmd>'` runs its (joined) arguments as a command.
            let joined = args.join(" ");
            split_into_segments(&joined, &mut out, 1);
        }
    }
    out
}

/// The command-string argument of a shell invocation: the token after a `-c`,
/// `--command`, or a clustered short flag that contains `c` (`-lc`, `-ec`).
fn shell_c_argument(args: &[String]) -> Option<&String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let is_c_flag = a == "-c"
            || a == "--command"
            || (a.starts_with('-') && !a.starts_with("--") && a.len() > 1 && a.contains('c'));
        if is_c_flag {
            return args.get(i + 1);
        }
        i += 1;
    }
    None
}

/// Split a command into simple-command segments, recursing into `$(…)` and
/// backtick substitutions. Honors single/double quotes; treats unquoted
/// newlines, `;`, `|`, `&`, `(`, `)`, and whitespace-bounded `{`/`}` as
/// boundaries.
fn split_into_segments(command: &str, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return; // bound pathological nesting
    }
    let chars: Vec<char> = command.chars().collect();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_single {
            cur.push(c);
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        // Command substitution executes even inside double quotes.
        if c == '$' && chars.get(i + 1) == Some(&'(') {
            let (inner, ni) = capture_balanced(&chars, i + 2);
            split_into_segments(&inner, out, depth + 1);
            i = ni;
            continue;
        }
        if c == '`' {
            let (inner, ni) = capture_backtick(&chars, i + 1);
            split_into_segments(&inner, out, depth + 1);
            i = ni;
            continue;
        }
        if in_double {
            cur.push(c);
            if c == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                in_single = true;
                cur.push(c);
                i += 1;
            }
            '"' => {
                in_double = true;
                cur.push(c);
                i += 1;
            }
            '\\' => {
                cur.push(c);
                if i + 1 < chars.len() {
                    cur.push(chars[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '\n' | ';' | '(' | ')' => {
                push_segment(out, &mut cur);
                i += 1;
            }
            '{' if i + 1 >= chars.len() || chars[i + 1].is_whitespace() => {
                push_segment(out, &mut cur);
                i += 1;
            }
            '}' if i == 0 || chars[i - 1].is_whitespace() => {
                push_segment(out, &mut cur);
                i += 1;
            }
            // `&&` is a segment boundary; a `&` that is part of a redirect
            // operator (`>&`, `&>`, `2>&1`) is not — keep it with the segment so
            // redirect detection sees the whole `>&target`, not a bare `>`.
            '&' if chars.get(i + 1) != Some(&'&')
                && (cur.ends_with('>') || chars.get(i + 1) == Some(&'>')) =>
            {
                cur.push(c);
                i += 1;
            }
            '&' => {
                push_segment(out, &mut cur);
                i += if chars.get(i + 1) == Some(&'&') { 2 } else { 1 };
            }
            // `>|` is the clobber-redirect operator, not a pipe — keep it.
            '|' if i > 0 && chars[i - 1] == '>' => {
                cur.push(c);
                i += 1;
            }
            '|' => {
                push_segment(out, &mut cur);
                i += if chars.get(i + 1) == Some(&'|') { 2 } else { 1 };
            }
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    push_segment(out, &mut cur);
}

/// Push the accumulated segment (trimmed) unless it is blank.
fn push_segment(out: &mut Vec<String>, cur: &mut String) {
    let s = std::mem::take(cur);
    let t = s.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
}

/// Capture up to the matching `)` for a `$(` opened before `start`, honoring
/// nesting. Returns the inner text and the index just past the `)`.
fn capture_balanced(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 1;
    let mut s = String::new();
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (s, i + 1);
                }
            }
            _ => {}
        }
        s.push(chars[i]);
        i += 1;
    }
    (s, i)
}

/// Capture up to the closing backtick. Returns the inner text and the index
/// just past the backtick.
fn capture_backtick(chars: &[char], start: usize) -> (String, usize) {
    let mut s = String::new();
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '`' {
            return (s, i + 1);
        }
        s.push(chars[i]);
        i += 1;
    }
    (s, i)
}

const SANCTIONED: &str = "land work via PR (gh pr create → merge on green CI) or `codeflow integrate <branch> --into <target>`";

#[allow(clippy::too_many_lines)]
fn check_git(
    args: &[String],
    session_branch: &mut String,
    cd_dir: Option<&str>,
    git_dir_env: Option<String>,
    ctx: &GuardContext<'_>,
    out: &mut Vec<Violation>,
) {
    let policy = ctx.policy;

    // Global-flag pass, ahead of the subcommand: hook-path override (`git -c
    // core.hooksPath=…` or `git --config-env=core.hooksPath=<VAR>`) disarms
    // the client hooks (ADR-0009); `-C`/`--git-dir` retarget the op at
    // another repository.
    let (hooks_path_override, retarget_flag) = scan_git_globals(args);
    if hooks_path_override && policy.hook_integrity.is_active() {
        out.push(hook_integrity_violation(
            policy.hook_integrity,
            "a git global flag (`-c`/`--config-env` core.hooksPath=…) overrides the hook path for this command"
                .to_string(),
        ));
        return;
    }

    let Some((sub, rest)) = git_subcommand(args) else {
        return;
    };

    // Effective target: an explicit `--git-dir`/`-C`, else `GIT_DIR=`, else a
    // chained `cd`. When retargeted, evaluate against that repo's branch (via
    // the injected resolver) instead of the session branch — the review's
    // wrong-dir evasion. No resolver / unreadable dir falls back to the session
    // branch (documented residual; the target repo's git-hook plane backstops).
    let retarget_dir = retarget_flag
        .or(git_dir_env)
        .or_else(|| cd_dir.map(str::to_string));
    let retargeted = retarget_dir.is_some();
    let eval_branch: String = match (&retarget_dir, ctx.dir_branch_lookup) {
        (Some(dir), Some(resolver)) => resolver(dir).unwrap_or_else(|| session_branch.clone()),
        _ => session_branch.clone(),
    };
    let branch = eval_branch.as_str();

    match sub {
        "checkout" | "switch" => {
            // A checkout in a *retargeted* dir does not change the session's
            // branch, so only track it for the session case.
            if !retargeted {
                if let Some(target) = checkout_target(rest) {
                    *session_branch = target;
                }
            }
        }
        "commit" => {
            if policy.commit_to_protected.is_active()
                && policy.branch_is_protected(branch)
                && !ctx.integrate_token
            {
                out.push(Violation::new(
                    "git.commit_to_protected",
                    policy.commit_to_protected,
                    format!("`git {sub}` would create commits on protected branch '{branch}'"),
                    SANCTIONED.to_string(),
                ));
            }
            maybe_no_verify(sub, rest, branch, policy, out);
        }
        "merge" | "cherry-pick" => {
            // Merges and cherry-picks land commits on the target — governed by
            // `merge_to_protected` (ADR-0007). The integrate gate token is the
            // one sanctioned local path; the human override is NOT honored here
            // (the Claude layer never trusts a claim of humanity).
            if policy.merge_to_protected.is_active()
                && policy.branch_is_protected(branch)
                && !ctx.integrate_token
            {
                out.push(Violation::new(
                    "git.merge_to_protected",
                    policy.merge_to_protected,
                    format!("`git {sub}` would land a commit on protected branch '{branch}'"),
                    SANCTIONED.to_string(),
                ));
            }
            maybe_no_verify(sub, rest, branch, policy, out);
        }
        "rebase" => {
            if policy.hard_reset_protected.is_active() && policy.branch_is_protected(branch) {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git rebase` rewrites history on protected branch '{branch}'"),
                    "rebase feature branches in their own worktree; protected history is append-only".to_string(),
                ));
            }
        }
        "reset" => {
            if policy.hard_reset_protected.is_active()
                && policy.branch_is_protected(branch)
                && rest.iter().any(|t| t == "--hard")
            {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git reset --hard` on protected branch '{branch}'"),
                    "create a revert commit instead; protected history is append-only".to_string(),
                ));
            }
        }
        "branch" => {
            if rest
                .iter()
                .any(|t| t == "-D" || t == "-d" || t == "--delete")
                && policy.delete_protected.is_active()
            {
                for target in rest
                    .iter()
                    .filter(|t| !t.starts_with('-'))
                    .filter(|t| policy.branch_is_protected(t))
                {
                    out.push(Violation::new(
                        "git.delete_protected",
                        policy.delete_protected,
                        format!("`git branch` would delete protected branch '{target}'"),
                        "protected branches are never deleted; remove the entry from git.protected_branches first if truly intended".to_string(),
                    ));
                }
            }
        }
        "config" => {
            // Hook-path manipulation via config (`git config core.hooksPath …`
            // / `--unset core.hooksPath`) disarms the client hooks. A pure read
            // (`--get`) is harmless and stays allowed.
            if policy.hook_integrity.is_active() && config_writes_hooks_path(rest) {
                out.push(hook_integrity_violation(
                    policy.hook_integrity,
                    "`git config core.hooksPath` changes where git looks for hooks".to_string(),
                ));
            }
        }
        "update-ref" => check_update_ref(rest, ctx, out),
        "symbolic-ref" => check_symbolic_ref(rest, ctx, out),
        "fast-import" => {
            // A bulk history import can write any ref, including a protected
            // branch, and its stream is opaque to the guard — treat it
            // conservatively (charter §6.1: unknown ref-writers deny).
            if policy.local_ref_protection.is_active() && !ctx.integrate_token {
                out.push(Violation::new(
                    "git.local_ref_protection",
                    policy.local_ref_protection,
                    "`git fast-import` can rewrite any ref, including protected branches"
                        .to_string(),
                    SANCTIONED.to_string(),
                ));
            }
        }
        "push" => check_push(rest, branch, ctx, out),
        _ => {}
    }
}

/// Scan the git global flags that precede the subcommand for a hook-path
/// override (`-c core.hooksPath=…`, or `--config-env core.hooksPath=<VAR>` /
/// `--config-env=core.hooksPath=<VAR>` — the value comes from an env var, but
/// the override is the same) and a retarget (`-C <dir>` / `--git-dir <dir>` /
/// `--git-dir=<dir>`). Returns `(hook_path_override, retarget_dir)`.
fn scan_git_globals(args: &[String]) -> (bool, Option<String>) {
    let mut hooks_path = false;
    let mut retarget: Option<String> = None;
    let mut idx = 0;
    while idx < args.len() {
        let t = args[idx].as_str();
        match t {
            "-c" | "--config-env" => {
                if args.get(idx + 1).is_some_and(|v| mentions_hooks_path(v)) {
                    hooks_path = true;
                }
                idx += 2;
            }
            "-C" | "--git-dir" | "--work-tree" | "--namespace" => {
                if (t == "-C" || t == "--git-dir") && retarget.is_none() {
                    retarget = args.get(idx + 1).cloned();
                }
                idx += 2;
            }
            _ => {
                if let Some(v) = t.strip_prefix("--config-env=") {
                    if mentions_hooks_path(v) {
                        hooks_path = true;
                    }
                    idx += 1;
                } else if let Some(v) = t.strip_prefix("--git-dir=") {
                    if retarget.is_none() {
                        retarget = Some(v.to_string());
                    }
                    idx += 1;
                } else if t.starts_with('-') {
                    idx += 1;
                } else {
                    break; // the subcommand
                }
            }
        }
    }
    (hooks_path, retarget)
}

/// `true` when a `git config` invocation *writes* `core.hooksPath` (a set or an
/// `--unset`), as opposed to a pure `--get`/`--list` read.
fn config_writes_hooks_path(rest: &[String]) -> bool {
    if !rest.iter().any(|t| mentions_hooks_path(t)) {
        return false;
    }
    let is_read = rest.iter().any(|t| {
        matches!(
            t.as_str(),
            "--get" | "--get-all" | "--get-regexp" | "--get-urlmatch" | "-l" | "--list"
        )
    });
    !is_read
}

/// Guard `git update-ref` (ADR-0009). Two vectors: (1) a direct write to a
/// protected `refs/heads/*` — a local move outside the sanctioned path; (2) a
/// write to `refs/remotes/*/<protected>` — poisoning the remote-tracking ref
/// the reference-transaction plane consults as its sync oracle. Deletions of a
/// protected ref route to `delete_protected`.
fn check_update_ref(rest: &[String], ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    let deleting = rest.iter().any(|t| t == "-d" || t == "--delete");
    // `--stdin` carries the refs out-of-band; block conservatively.
    if rest.iter().any(|t| t == "--stdin") {
        if policy.local_ref_protection.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                "`git update-ref --stdin` updates refs from an opaque stream that may touch protected branches".to_string(),
                SANCTIONED.to_string(),
            ));
        }
        return;
    }
    let Some(refname) = update_ref_name(rest) else {
        return;
    };

    if let Some(branch) = protected_component_of_remote_ref(refname, policy) {
        if policy.local_ref_protection.is_active() {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                format!("`git update-ref` writes the remote-tracking ref for protected branch '{branch}' — the reference-transaction sync oracle must not be agent-set"),
                "let a real `git fetch`/`git pull` update refs/remotes; never set it by hand".to_string(),
            ));
        }
        return;
    }

    if let Some(branch) = refname.strip_prefix("refs/heads/") {
        if !policy.branch_is_protected(branch) {
            return;
        }
        if deleting {
            if policy.delete_protected.is_active() && !ctx.integrate_token {
                out.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("`git update-ref -d` deletes protected branch '{branch}'"),
                    "protected branches are never deleted; remove the entry from git.protected_branches first if truly intended".to_string(),
                ));
            }
        } else if policy.local_ref_protection.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                format!("`git update-ref` moves protected branch '{branch}' outside the sanctioned path"),
                SANCTIONED.to_string(),
            ));
        }
    }
}

/// The ref name argument of `git update-ref` — the first non-flag token, past
/// `-m <reason>` (which consumes a value) and the boolean flags.
fn update_ref_name(rest: &[String]) -> Option<&str> {
    let mut idx = 0;
    while idx < rest.len() {
        let t = rest[idx].as_str();
        if t == "-m" {
            idx += 2;
        } else if t.starts_with('-') {
            idx += 1;
        } else {
            return Some(t);
        }
    }
    None
}

/// When `refname` is `refs/remotes/<remote>/<branch>` and `<branch>` is
/// protected, return the branch; else `None`.
fn protected_component_of_remote_ref<'a>(refname: &'a str, policy: &GitPolicy) -> Option<&'a str> {
    let rest = refname.strip_prefix("refs/remotes/")?;
    // rest is `<remote>/<branch...>`; the branch is everything after the first
    // path component.
    let (_remote, branch) = rest.split_once('/')?;
    policy.branch_is_protected(branch).then_some(branch)
}

/// Guard `git symbolic-ref` writes (`git symbolic-ref <name> <target>`) that
/// point a ref at, or repoint, a protected branch. The read form (a single
/// argument) is allowed.
fn check_symbolic_ref(rest: &[String], ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    if !policy.local_ref_protection.is_active() || ctx.integrate_token {
        return;
    }
    let positionals: Vec<&str> = rest
        .iter()
        .filter(|t| !t.starts_with('-'))
        .map(String::as_str)
        .collect();
    // A write needs a value (2+ positionals). Flag when either the ref being
    // set or its target names a protected branch.
    if positionals.len() >= 2 {
        let touches_protected = positionals.iter().any(|p| {
            let name = p.strip_prefix("refs/heads/").unwrap_or(p);
            policy.branch_is_protected(name)
        });
        if touches_protected {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                "`git symbolic-ref` repoints a protected branch ref".to_string(),
                SANCTIONED.to_string(),
            ));
        }
    }
}

fn check_push(rest: &[String], branch: &str, ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    let push = parse_push(rest, branch);

    // `--no-verify` skips the pre-push hook; on a protected target that is a
    // gate bypass (an anti-bypass structural block, ADR-0007).
    if has_no_verify("push", rest) {
        let bulk_touches_protected =
            push.touches_all_branches && !policy.protected_branch_names().is_empty();
        if bulk_touches_protected {
            out.push(no_verify_violation("push", "all branches"));
        } else {
            for target in push.updates.iter().chain(push.deletions.iter()) {
                if policy.branch_is_protected(target) {
                    out.push(no_verify_violation("push", target));
                    break;
                }
            }
        }
    }

    // `--all` / `--mirror` / a wildcard refspec (`refs/heads/*`, `*:*`) update
    // — and `--mirror` also deletes — *every* branch on the remote, protected
    // ones included; an explicit target list can never be trusted to exclude
    // them (the review's bulk-push evasion). Treat it as touching all protected
    // branches (charter §6.1).
    if push.touches_all_branches {
        let protected = policy.protected_branch_names();
        if !protected.is_empty() {
            let names = protected.join(", ");
            if push.force && policy.force_push_protected.is_active() {
                out.push(Violation::new(
                    "git.force_push_protected",
                    policy.force_push_protected,
                    format!("bulk force-push (--all/--mirror/wildcard) reaches protected branches: {names}"),
                    SANCTIONED.to_string(),
                ));
            } else if policy.push_to_protected.is_active() && !ctx.integrate_token {
                out.push(Violation::new(
                    "git.push_to_protected",
                    policy.push_to_protected,
                    format!(
                        "bulk push (--all/--mirror/wildcard) reaches protected branches: {names}"
                    ),
                    SANCTIONED.to_string(),
                ));
            }
            if push.mirror && policy.delete_protected.is_active() {
                out.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("`git push --mirror` can delete protected branches to mirror local: {names}"),
                    "protected branches are never deleted remotely; --mirror is unsafe here".to_string(),
                ));
            }
        }
        return;
    }

    for target in &push.deletions {
        if policy.delete_protected.is_active() && policy.branch_is_protected(target) {
            out.push(Violation::new(
                "git.delete_protected",
                policy.delete_protected,
                format!("`git push` would delete protected branch '{target}' on the remote"),
                "protected branches are never deleted remotely; adjust git.protected_branches first if truly intended".to_string(),
            ));
        }
    }

    for target in &push.updates {
        let protected = policy.branch_is_protected(target);
        if push.force {
            if protected {
                if policy.force_push_protected.is_active() {
                    out.push(Violation::new(
                        "git.force_push_protected",
                        policy.force_push_protected,
                        format!("force-push to protected branch '{target}'"),
                        SANCTIONED.to_string(),
                    ));
                }
            } else if policy.force_push_unprotected.is_active() {
                // Default policy is allow (D8): rebasing feature branches is
                // normal. Only flips on if the user tightens the policy.
                out.push(Violation::new(
                    "git.force_push_unprotected",
                    policy.force_push_unprotected,
                    format!("force-push to branch '{target}'"),
                    "policy git.force_push_unprotected restricts force-pushes in this repo"
                        .to_string(),
                ));
            }
        } else if protected && policy.push_to_protected.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!("direct push to protected branch '{target}'"),
                SANCTIONED.to_string(),
            ));
        }
    }
}

fn check_gh(args: &[String], ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let plain: Vec<&str> = args.iter().map(String::as_str).collect();
    if plain.first() != Some(&"pr") {
        return;
    }
    match plain.get(1) {
        Some(&"create") => check_gh_pr_create(&plain[2..], ctx.policy, out),
        Some(&"merge") => check_gh_pr_merge(&plain[2..], ctx, out),
        _ => {}
    }
}

/// Scan a `gh pr create` body for AI attribution / emoji (charter §6.4).
///
/// Both the inline `--body`/`-b` value and the content of a `--body-file`/`-F`
/// file are scanned. Fail-open (matching the guard's doctrine): a missing or
/// unreadable body file passes rather than blocking. A stdin body (`-F -`) is
/// out of scope — its content is not available to the guard, so it is not read.
fn check_gh_pr_create(rest: &[&str], policy: &GitPolicy, out: &mut Vec<Violation>) {
    let inline = flag_value(rest, &["--body", "-b"]);
    let from_file = flag_value(rest, &["--body-file", "-F"])
        .filter(|path| *path != "-")
        .and_then(|path| std::fs::read_to_string(path).ok());
    for body in inline.into_iter().chain(from_file.as_deref()) {
        scan_pr_body(body, policy, out);
    }
}

/// Flag AI-attribution and emoji violations in a single PR-body string.
fn scan_pr_body(body: &str, policy: &GitPolicy, out: &mut Vec<Violation>) {
    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(body) {
            out.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("PR body contains AI attribution ({which})"),
                "remove the attribution — project policy forbids AI attribution in commits and PR bodies (charter §6.4)".to_string(),
            ));
        }
    }
    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(body) {
            out.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("PR body contains emoji ('{c}')"),
                "remove emoji from the PR body (charter §6.4)".to_string(),
            ));
        }
    }
}

/// Guard `gh pr merge` (ADR-0007): block `--delete-branch` outright (it can
/// corrupt a worktree-checked-out root into `core.bare`), and block merges
/// into a protected base unless the base is provably unprotected.
fn check_gh_pr_merge(rest: &[&str], ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;

    // `--delete-branch` moves off the merged branch; when that branch is
    // checked out in a worktree, git has flipped the root repo to core.bare
    // (reproduced twice). Block regardless of the base branch.
    if rest.contains(&"--delete-branch") {
        out.push(Violation::new(
            "git.pr_merge_delete_branch",
            PolicyLevel::Block,
            "`gh pr merge --delete-branch` can flip the root repo to core.bare when the merged branch is checked out in a worktree".to_string(),
            "merge plain (no --delete-branch), then delete the branch from the repo root separately (`git branch -d <branch>` / `git push origin --delete <branch>`)".to_string(),
        ));
        return;
    }

    if !policy.pr_merge_to_protected.is_active() {
        return;
    }

    // Cheap, bounded base introspection: allow ONLY when the base is provably
    // not protected; block on a protected base or any doubt (no lookup, an
    // unreachable API, an unrecognized target).
    let target = pr_merge_target(rest).unwrap_or_default();
    let base = ctx.pr_base_lookup.and_then(|lookup| lookup(target));
    let provably_unprotected = base
        .as_deref()
        .is_some_and(|b| !policy.branch_is_protected(b));
    if !provably_unprotected {
        out.push(Violation::new(
            "git.pr_merge_to_protected",
            policy.pr_merge_to_protected,
            "`gh pr merge` into a base that is protected or could not be confirmed unprotected".to_string(),
            "PR merges into protected branches are performed by a human (GitHub UI / their own terminal) or explicitly sanctioned — policy git.pr_merge_to_protected".to_string(),
        ));
    }
}

/// Flags of `gh pr merge` that consume the following token.
const GH_PR_MERGE_VALUE_FLAGS: &[&str] = &[
    "--body",
    "-b",
    "--body-file",
    "-F",
    "--subject",
    "-t",
    "--match-head-commit",
    "--author-email",
];

/// The `gh pr merge [<number|url|branch>]` positional, when present. Empty /
/// absent means "the current branch's PR" — the lookup handles that.
fn pr_merge_target<'a>(rest: &[&'a str]) -> Option<&'a str> {
    let mut idx = 0;
    while idx < rest.len() {
        let t = rest[idx];
        if GH_PR_MERGE_VALUE_FLAGS.contains(&t) {
            idx += 2;
        } else if t.starts_with('-') {
            idx += 1;
        } else {
            return Some(t);
        }
    }
    None
}

/// `true` when `args` skip the client hooks with `--no-verify` (or `-n` on a
/// `git commit`, where `-n` is that flag; on push/merge `-n` means something
/// else, so only the long form counts there).
fn has_no_verify(sub: &str, args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--no-verify" || (sub == "commit" && a == "-n"))
}

fn maybe_no_verify(
    sub: &str,
    args: &[String],
    branch: &str,
    policy: &GitPolicy,
    out: &mut Vec<Violation>,
) {
    if has_no_verify(sub, args) && policy.branch_is_protected(branch) {
        out.push(no_verify_violation(sub, branch));
    }
}

fn no_verify_violation(sub: &str, branch: &str) -> Violation {
    Violation::new(
        "git.no_verify_bypass",
        PolicyLevel::Block,
        format!("`git {sub} --no-verify` skips the client hooks on protected branch '{branch}'"),
        "do not bypass the hooks with --no-verify — fix the cause the gate flags, or land via the sanctioned path (PR / `codeflow integrate`)".to_string(),
    )
}

// ---------------------------------------------------------------------------
// command parsing helpers
// ---------------------------------------------------------------------------

/// Tokenize a shell command segment, honoring single/double quotes and
/// backslash escapes (outside single quotes).
#[must_use]
pub fn shell_tokens(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut started = false;
    let mut chars = segment.chars();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                started = true;
            }
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    started = true;
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if started {
                    tokens.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(cur);
    }
    tokens
}

/// Skip leading `VAR=value` assignments; return `(program, args)`.
/// Resolve the *effective* program of a simple command, stripping the command
/// launchers an agent can hide it behind so the program is judged by what
/// actually runs, not the surface word: leading `VAR=val` assignments, the
/// `command`/`builtin` prefixes, and the `env` family (any path form, e.g.
/// `/usr/bin/env`, plus its leading `VAR=val` args). Returns the effective
/// program token and its arguments. This is a *general* normalization — the
/// same one the launderer scan uses — so it covers `env FOO=1 bash -c …`,
/// `command git …`, `/usr/bin/env git …`, etc., not an enumerated list.
fn strip_launchers(tokens: &[String]) -> Option<(&str, &[String])> {
    let mut idx = 0;
    loop {
        // Skip leading VAR=val assignments.
        while idx < tokens.len() {
            let t = tokens[idx].as_str();
            if t.split_once('=')
                .is_some_and(|(name, _)| !name.is_empty() && is_identifier(name))
            {
                idx += 1;
            } else {
                break;
            }
        }
        let t = tokens.get(idx)?.as_str();
        if t == "command" || t == "builtin" || t == "exec" {
            idx += 1;
            continue;
        }
        if basename(t) == "env" {
            idx += 1;
            // `env [-i] [-u NAME] [VAR=val]... command` — skip its own options
            // and assignments up to the wrapped command.
            while idx < tokens.len() {
                let a = tokens[idx].as_str();
                if a == "-u" {
                    idx += 2; // -u NAME
                } else if a.starts_with('-')
                    || a.split_once('=')
                        .is_some_and(|(name, _)| !name.is_empty() && is_identifier(name))
                {
                    idx += 1; // an env option or a VAR=val pair
                } else {
                    break;
                }
            }
            continue;
        }
        return Some((t, &tokens[idx + 1..]));
    }
}

/// `true` when `name` is a POSIX shell whose `-c` argument is a command string.
fn is_shell(name: &str) -> bool {
    matches!(name, "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash")
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Global git flags that consume the following token.
const GIT_GLOBAL_VALUE_FLAGS: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

/// Find the git subcommand, skipping global flags (`git -C path commit …`).
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut idx = 0;
    while idx < args.len() {
        let t = &args[idx];
        if GIT_GLOBAL_VALUE_FLAGS.contains(&t.as_str()) {
            idx += 2;
        } else if t.starts_with('-') {
            idx += 1;
        } else {
            return Some((t.as_str(), &args[idx + 1..]));
        }
    }
    None
}

/// The branch a `git checkout`/`git switch` lands on, when determinable.
fn checkout_target(rest: &[String]) -> Option<String> {
    let mut iter = rest.iter().peekable();
    while let Some(t) = iter.next() {
        match t.as_str() {
            "-b" | "-B" | "-c" | "-C" => return iter.next().cloned(),
            "--detach" | "--" => return None,
            s if s.starts_with('-') => {}
            s => return Some(s.to_string()),
        }
    }
    None
}

/// Parsed `git push` intent.
struct PushIntent {
    force: bool,
    /// Remote branch names being updated.
    updates: Vec<String>,
    /// Remote branch names being deleted.
    deletions: Vec<String>,
    /// `--all`, `--mirror`, or a wildcard refspec (`refs/heads/*`, `*:*`): the
    /// push reaches every branch on the remote, so any protected branch is in
    /// scope regardless of the explicit target list.
    touches_all_branches: bool,
    /// `--mirror`: also *deletes* remote branches absent locally.
    mirror: bool,
}

/// Push flags that consume the following token.
const PUSH_VALUE_FLAGS: &[&str] = &["-o", "--push-option", "--receive-pack", "--exec", "--repo"];

fn parse_push(rest: &[String], current_branch: &str) -> PushIntent {
    let mut force = false;
    let mut delete_mode = false;
    let mut all = false;
    let mut mirror = false;
    let mut positional: Vec<&str> = Vec::new();

    let mut idx = 0;
    while idx < rest.len() {
        let t = rest[idx].as_str();
        if PUSH_VALUE_FLAGS.contains(&t) {
            idx += 2;
            continue;
        }
        if t == "--force" || t.starts_with("--force-with-lease") {
            force = true;
        } else if t == "--delete" {
            delete_mode = true;
        } else if t == "--all" || t == "--branches" {
            all = true;
        } else if t == "--mirror" {
            mirror = true;
            force = true; // a mirror force-updates refs to match local
        } else if t.starts_with("--") {
            // other long flag, no value consumed
        } else if let Some(cluster) = t.strip_prefix('-') {
            if !cluster.is_empty() && cluster.chars().all(char::is_alphanumeric) {
                if cluster.contains('f') {
                    force = true;
                }
                if cluster.contains('d') {
                    delete_mode = true;
                }
            }
        } else {
            positional.push(t);
        }
        idx += 1;
    }

    let mut updates = Vec::new();
    let mut deletions = Vec::new();

    // positional[0] is the remote; the rest are refspecs.
    let refspecs = if positional.len() > 1 {
        &positional[1..]
    } else {
        &[][..]
    };

    // A wildcard refspec (`refs/heads/*:refs/heads/*`, `*:*`, `+refs/heads/*`)
    // fans out across branches just like `--all`/`--mirror`.
    let wildcard_refspec = refspecs.iter().any(|s| s.contains('*'));
    let touches_all_branches = all || mirror || wildcard_refspec;

    if refspecs.is_empty() {
        // `git push` / `git push origin`: updates the current branch.
        if !current_branch.is_empty() {
            let list = if delete_mode {
                &mut deletions
            } else {
                &mut updates
            };
            list.push(current_branch.to_string());
        }
    } else {
        for spec in refspecs {
            let spec = spec.strip_prefix('+').map_or(*spec, |s| {
                force = true;
                s
            });
            if delete_mode {
                deletions.push(normalize_ref(spec, current_branch));
                continue;
            }
            if let Some((src, dst)) = spec.split_once(':') {
                if src.is_empty() {
                    deletions.push(normalize_ref(dst, current_branch));
                } else {
                    updates.push(normalize_ref(dst, current_branch));
                }
            } else {
                updates.push(normalize_ref(spec, current_branch));
            }
        }
    }

    PushIntent {
        force,
        updates,
        deletions,
        touches_all_branches,
        mirror,
    }
}

/// Strip `refs/heads/`, resolve `HEAD` to the current branch.
fn normalize_ref(name: &str, current_branch: &str) -> String {
    let name = name.strip_prefix("refs/heads/").unwrap_or(name);
    if name == "HEAD" {
        current_branch.to_string()
    } else {
        name.to_string()
    }
}

/// Extract the value of a flag (`--body x`, `--body=x`, `-b x`).
fn flag_value<'a>(args: &'a [&'a str], names: &[&str]) -> Option<&'a str> {
    let mut idx = 0;
    while idx < args.len() {
        let t = args[idx];
        for n in names {
            if t == *n {
                return args.get(idx + 1).copied();
            }
            if let Some(v) = t.strip_prefix(&format!("{n}=")) {
                return Some(v);
            }
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::policy::PolicyLevel;
    use super::*;

    fn ctx<'a>(policy: &'a GitPolicy, branch: &'a str) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
            pr_base_lookup: None,
            dir_branch_lookup: None,
        }
    }

    /// A guard context with an injected `gh pr merge` base-branch resolver.
    fn ctx_with_lookup<'a>(
        policy: &'a GitPolicy,
        branch: &'a str,
        lookup: &'a dyn Fn(&str) -> Option<String>,
    ) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
            pr_base_lookup: Some(lookup),
            dir_branch_lookup: None,
        }
    }

    /// A guard context with an injected retarget-dir → branch resolver.
    fn ctx_with_dir_branch<'a>(
        policy: &'a GitPolicy,
        branch: &'a str,
        resolver: &'a dyn Fn(&str) -> Option<String>,
    ) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
            pr_base_lookup: None,
            dir_branch_lookup: Some(resolver),
        }
    }

    fn default_policy() -> GitPolicy {
        GitPolicy::default()
    }

    fn release_policy() -> GitPolicy {
        GitPolicy {
            protected_branches: vec!["main".into(), "master".into(), "release/*".into()],
            ..GitPolicy::default()
        }
    }

    // -- payload parsing --

    #[test]
    fn test_payload_parse_bash() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"},"cwd":"/x"}"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), Some("git status"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new("/x")));
    }

    #[test]
    fn test_payload_parses_codex_shaped_extra_fields() {
        // The Codex hooks engine sends the same field names PLUS extras
        // (turn_id, model, permission_mode "dontAsk") and a nullable
        // transcript_path. The lenient payload must ignore the extras and parse
        // — this is what lets one `codeflow hook <guard>` serve both harnesses.
        let json = r#"{
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "session_id": "abc",
            "cwd": "/repo",
            "transcript_path": null,
            "turn_id": "t-1",
            "model": "gpt-5.5",
            "permission_mode": "dontAsk"
        }"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), Some("git status"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new("/repo")));
    }

    #[test]
    fn test_payload_parse_powershell() {
        let json =
            r#"{"tool_name":"PowerShell","tool_input":{"command":"git status"},"cwd":"C:\\repo"}"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), Some("git status"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new(r"C:\repo")));
    }

    #[test]
    fn test_payload_non_bash_tool_ignored() {
        let json = r#"{"tool_name":"Write","tool_input":{"file_path":"a"}}"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), None);
    }

    #[test]
    fn test_payload_malformed_json_is_error() {
        assert!(HookPayload::parse("{ nope").is_err());
    }

    // -- commit on protected --

    #[test]
    fn test_commit_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.commit_to_protected");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(v[0].remedy.contains("codeflow integrate"));
    }

    #[test]
    fn test_commit_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git commit -m 'feat: x'", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_commit_with_integrate_token_allowed() {
        let p = default_policy();
        let c = GuardContext {
            policy: &p,
            current_branch: "main",
            integrate_token: true,
            pr_base_lookup: None,
            dir_branch_lookup: None,
        };
        assert!(evaluate("git commit -m 'feat: x'", &c).is_empty());
    }

    #[test]
    fn test_merge_on_protected_blocked() {
        // Charter AC #5: raw `git merge` into main blocked — now on the
        // dedicated merge_to_protected key (ADR-0007).
        let p = default_policy();
        let v = evaluate("git merge feat/x", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.merge_to_protected");
    }

    #[test]
    fn test_merge_with_integrate_token_allowed() {
        let p = default_policy();
        let c = GuardContext {
            policy: &p,
            current_branch: "main",
            integrate_token: true,
            pr_base_lookup: None,
            dir_branch_lookup: None,
        };
        assert!(evaluate("git merge feat/x", &c).is_empty());
    }

    #[test]
    fn test_cherry_pick_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git cherry-pick abc123", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.merge_to_protected");
    }

    #[test]
    fn test_merge_key_independent_of_commit_key() {
        // merge_to_protected=off must not be overridden by commit_to_protected.
        let p = GitPolicy {
            merge_to_protected: PolicyLevel::Off,
            ..default_policy()
        };
        assert!(evaluate("git merge feat/x", &ctx(&p, "main")).is_empty());
        // ...but a plain commit on protected is still blocked by its own key.
        let v = evaluate("git commit -m 'x'", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_merge_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git merge main", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_checkout_dodge_detected() {
        let p = default_policy();
        let v = evaluate("git checkout main && git merge feat/x", &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.merge_to_protected");
    }

    #[test]
    fn test_switch_back_to_feature_not_flagged() {
        let p = default_policy();
        let v = evaluate(
            "git switch feat/y && git commit -m 'fix: y'",
            &ctx(&p, "main"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn test_checkout_new_branch_from_main_allowed() {
        let p = default_policy();
        let v = evaluate(
            "git checkout -b feat/z && git commit -m 'feat: z'",
            &ctx(&p, "main"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn test_commit_warn_level_warns() {
        let p = GitPolicy {
            commit_to_protected: PolicyLevel::Warn,
            ..default_policy()
        };
        let v = evaluate("git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_commit_check_off_silent() {
        let p = GitPolicy {
            commit_to_protected: PolicyLevel::Off,
            ..default_policy()
        };
        assert!(evaluate("git commit -m 'feat: x'", &ctx(&p, "main")).is_empty());
    }

    // -- push / force-push / delete --

    #[test]
    fn test_push_to_protected_explicit_refspec_blocked() {
        let p = default_policy();
        let v = evaluate("git push origin main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
        assert!(v[0].message.contains("'main'"));
    }

    #[test]
    fn test_push_head_to_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git push origin HEAD:main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_bare_push_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git push", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_push_feature_branch_allowed() {
        let p = default_policy();
        assert!(evaluate("git push origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git push -u origin feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_force_push_feature_branch_allowed() {
        // AC #4 / D8: force-push to a feature branch is normal.
        let p = default_policy();
        assert!(evaluate("git push --force origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git push -f origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate(
            "git push --force-with-lease origin feat/x",
            &ctx(&p, "feat/x")
        )
        .is_empty());
    }

    #[test]
    fn test_force_push_protected_blocked() {
        let p = default_policy();
        for cmd in [
            "git push --force origin main",
            "git push -f origin main",
            "git push --force-with-lease origin main",
            "git push origin +feat/x:main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert_eq!(v.len(), 1, "{cmd}");
            assert_eq!(v[0].rule, "git.force_push_protected", "{cmd}");
        }
    }

    #[test]
    fn test_delete_protected_via_push_blocked() {
        let p = default_policy();
        for cmd in [
            "git push origin --delete main",
            "git push origin :main",
            "git push -d origin main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert_eq!(v.len(), 1, "{cmd}");
            assert_eq!(v[0].rule, "git.delete_protected", "{cmd}");
        }
    }

    #[test]
    fn test_delete_feature_branch_allowed() {
        let p = default_policy();
        assert!(evaluate("git push origin --delete feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git branch -D feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_delete_protected_local_branch_blocked() {
        let p = default_policy();
        let v = evaluate("git branch -D main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.delete_protected");
    }

    // -- hard reset / rebase --

    #[test]
    fn test_hard_reset_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git reset --hard HEAD~3", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.hard_reset_protected");
    }

    #[test]
    fn test_hard_reset_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git reset --hard HEAD~3", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_soft_reset_on_protected_allowed() {
        let p = default_policy();
        assert!(evaluate("git reset --soft HEAD~1", &ctx(&p, "main")).is_empty());
    }

    #[test]
    fn test_rebase_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git rebase feat/x", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.hard_reset_protected");
    }

    // -- glob extension (AC #3) --

    #[test]
    fn test_release_glob_honored_by_guard() {
        let p = release_policy();
        let v = evaluate("git commit -m 'fix: x'", &ctx(&p, "release/2.0"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
        let v = evaluate("git push origin release/2.0", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
        let v = evaluate("git push -f origin release/2.0", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.force_push_protected");
        assert!(evaluate("git commit -m 'fix: x'", &ctx(&p, "feat/x")).is_empty());
    }

    // -- gh pr create body scan (AC #13) --

    #[test]
    fn test_pr_body_attribution_blocked() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'Done.\n\nGenerated with Claude Code'";
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.ai_attribution");
    }

    #[test]
    fn test_pr_body_coauthored_blocked() {
        let p = default_policy();
        let cmd =
            r#"gh pr create -t "feat: x" -b "ok Co-Authored-By: Claude <noreply@anthropic.com>""#;
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.ai_attribution");
    }

    #[test]
    fn test_pr_body_emoji_blocked() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'ship it \u{1F680}'";
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.commit_emoji");
    }

    #[test]
    fn test_pr_body_clean_allowed() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'Summary: adds the hook plane.'";
        assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_pr_body_scan_respects_policy_off() {
        let p = GitPolicy {
            ai_attribution: PolicyLevel::Off,
            commit_emoji: PolicyLevel::Off,
            ..default_policy()
        };
        let cmd = "gh pr create --body 'Generated with Bot \u{1F916}'";
        assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_pr_body_file_attribution_blocked() {
        let p = default_policy();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body.md");
        std::fs::write(&path, "Summary.\n\nGenerated with Claude Code").unwrap();
        let cmd = format!(
            "gh pr create --title 'feat: x' --body-file '{}'",
            path.display()
        );
        let v = evaluate(&cmd, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.ai_attribution");
    }

    #[test]
    fn test_pr_body_file_clean_allowed() {
        let p = default_policy();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body.md");
        std::fs::write(&path, "Summary: adds the hook plane.").unwrap();
        let cmd = format!("gh pr create -t 'feat: x' -F '{}'", path.display());
        assert!(evaluate(&cmd, &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_pr_body_file_missing_passes() {
        // Fail-open: an unreadable / missing body file must not block.
        let p = default_policy();
        let cmd = "gh pr create -t 'feat: x' --body-file '/no/such/body/file.md'";
        assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty());
    }

    // -- gh pr merge (ADR-0007) --

    #[test]
    fn test_gh_pr_merge_blocks_on_protected_base() {
        let p = default_policy();
        let lookup = |_: &str| Some("main".to_string());
        let v = evaluate(
            "gh pr merge 42 --squash",
            &ctx_with_lookup(&p, "feat/x", &lookup),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.pr_merge_to_protected");
        assert!(v[0].remedy.contains("performed by a human"));
    }

    #[test]
    fn test_gh_pr_merge_allows_proven_unprotected_base() {
        let p = default_policy();
        let lookup = |_: &str| Some("develop".to_string());
        assert!(evaluate(
            "gh pr merge 42 --squash",
            &ctx_with_lookup(&p, "feat/x", &lookup)
        )
        .is_empty());
    }

    #[test]
    fn test_gh_pr_merge_doubt_blocks() {
        // No lookup available, or the lookup fails → any doubt blocks.
        let p = default_policy();
        let v = evaluate("gh pr merge 42 --squash", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.pr_merge_to_protected");

        let lookup = |_: &str| None;
        let v = evaluate(
            "gh pr merge --merge",
            &ctx_with_lookup(&p, "feat/x", &lookup),
        );
        assert_eq!(v[0].rule, "git.pr_merge_to_protected");
    }

    #[test]
    fn test_gh_pr_merge_off_allows() {
        let p = GitPolicy {
            pr_merge_to_protected: PolicyLevel::Off,
            ..default_policy()
        };
        // Even with a protected base and no lookup, an off policy is silent.
        assert!(evaluate("gh pr merge 42 --squash", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_gh_pr_merge_delete_branch_blocked_regardless_of_base() {
        let p = default_policy();
        // Even a provably-unprotected base cannot rescue --delete-branch.
        let lookup = |_: &str| Some("develop".to_string());
        let v = evaluate(
            "gh pr merge 42 --squash --delete-branch",
            &ctx_with_lookup(&p, "feat/x", &lookup),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.pr_merge_delete_branch");
        assert!(v[0]
            .remedy
            .contains("delete the branch from the repo root separately"));
    }

    #[test]
    fn test_gh_pr_merge_delete_branch_blocked_even_when_policy_off() {
        // The --delete-branch guard is structural, not policy-gated.
        let p = GitPolicy {
            pr_merge_to_protected: PolicyLevel::Off,
            ..default_policy()
        };
        let v = evaluate("gh pr merge --delete-branch", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.pr_merge_delete_branch");
        assert_eq!(v[0].level, PolicyLevel::Block);
    }

    // -- anti-laundering of override tokens (ADR-0007) --

    #[test]
    fn test_laundering_env_prefix_blocked() {
        let p = default_policy();
        for cmd in [
            "CODEFLOW_HUMAN_OVERRIDE=1 git merge feat/x",
            "CODEFLOW_INTEGRATE_TOKEN=abc git merge feat/x",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(
                v.iter().any(|x| x.rule == "git.override_token_laundering"),
                "{cmd}: {v:?}"
            );
            assert!(v.iter().all(|x| x.level == PolicyLevel::Block));
        }
    }

    #[test]
    fn test_laundering_export_blocked() {
        let p = default_policy();
        let v = evaluate("export CODEFLOW_HUMAN_OVERRIDE=1", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.override_token_laundering");
        // Bare `export VAR` (priming) is also caught.
        let v = evaluate("export CODEFLOW_INTEGRATE_TOKEN", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.override_token_laundering");
    }

    #[test]
    fn test_laundering_env_command_blocked() {
        let p = default_policy();
        let v = evaluate(
            "env CODEFLOW_HUMAN_OVERRIDE=1 git merge feat/x",
            &ctx(&p, "feat/x"),
        );
        assert_eq!(v[0].rule, "git.override_token_laundering");
    }

    #[test]
    fn test_laundering_does_not_flag_unrelated_env() {
        let p = default_policy();
        assert!(evaluate("FOO=1 git status", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("export EDITOR=vim", &ctx(&p, "feat/x")).is_empty());
    }

    // -- --no-verify bypass (ADR-0007) --

    #[test]
    fn test_no_verify_bypass_on_protected_blocked() {
        let p = default_policy();
        for (cmd, branch) in [
            ("git commit --no-verify -m 'x'", "main"),
            ("git commit -n -m 'x'", "main"),
            ("git merge --no-verify feat/x", "main"),
            ("git push --no-verify origin main", "feat/x"),
        ] {
            let v = evaluate(cmd, &ctx(&p, branch));
            assert!(
                v.iter().any(|x| x.rule == "git.no_verify_bypass"),
                "{cmd}: {v:?}"
            );
        }
    }

    #[test]
    fn test_no_verify_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git commit --no-verify -m 'x'", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git push --no-verify origin feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_push_dry_run_n_not_treated_as_no_verify() {
        // `git push -n` is --dry-run, not --no-verify: must not be flagged.
        let p = default_policy();
        assert!(evaluate("git push -n origin feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    // -- safe commands --

    #[test]
    fn test_everyday_commands_untouched() {
        let p = default_policy();
        for cmd in [
            "git status",
            "git log --oneline -5",
            "git add -A",
            "git diff main...HEAD",
            "cargo test -p codeflow-core",
            "ls -la",
            "git fetch origin",
            "gh pr view 12",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "main")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn test_env_prefix_does_not_hide_git() {
        let p = default_policy();
        let v = evaluate("FOO=1 git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_git_with_global_flags() {
        let p = default_policy();
        let v = evaluate("git -C /repo commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    // -- tokenizer --

    #[test]
    fn test_shell_tokens_quotes() {
        assert_eq!(
            shell_tokens(r#"git commit -m "feat: a b" -n"#),
            vec!["git", "commit", "-m", "feat: a b", "-n"]
        );
        assert_eq!(
            shell_tokens("echo 'single quoted arg'"),
            vec!["echo", "single quoted arg"]
        );
    }

    #[test]
    fn test_shell_tokens_escapes() {
        assert_eq!(shell_tokens(r"echo a\ b"), vec!["echo", "a b"]);
    }

    fn has_rule(v: &[Violation], rule: &str) -> bool {
        v.iter().any(|x| x.rule == rule)
    }

    // -- hook/policy integrity (HOOK-PLANE SELF-DISARM) --

    #[test]
    fn test_hook_path_manipulation_blocked() {
        let p = default_policy();
        for cmd in [
            "git config core.hooksPath /tmp/evil",
            "git config --global core.hooksPath /tmp/evil",
            "git config --unset core.hooksPath",
            "git -c core.hooksPath=/dev/null commit -m x",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_config_env_hooks_path_blocked() {
        // `--config-env` sets the same key as `-c`, only sourcing the value
        // from an env var — the sibling bypass must block identically, in both
        // the `=`-joined and the space-separated form (case-insensitive key).
        let p = default_policy();
        for cmd in [
            "HOOKS=/dev/null git --config-env=core.hooksPath=HOOKS commit -m x",
            "HOOKS=/dev/null git --config-env core.hookspath=HOOKS commit -m x",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
        // A non-hooksPath --config-env is not an integrity concern.
        assert!(evaluate(
            "NAME=x git --config-env=user.name=NAME commit -m 'feat: x'",
            &ctx(&p, "feat/x")
        )
        .is_empty());
    }

    #[test]
    fn test_config_get_hooks_path_allowed() {
        // Reading the value is harmless.
        let p = default_policy();
        assert!(evaluate("git config --get core.hooksPath", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_hook_skip_env_prefixes_blocked_on_feature_branch() {
        // These disarm the client hooks regardless of branch — a feature branch
        // commit is normally fine, so the block proves it is the env var, not
        // the commit-to-protected rule, being caught.
        let p = default_policy();
        for cmd in [
            "GIT_SKIP_HOOKS=1 git commit -m x",
            "HUSKY=0 git commit -m x",
            "SKIP_HOOKS=1 git commit -m x",
            "PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit -m x",
            "GIT_HOOKS_PATH=/tmp git commit -m x",
            "env GIT_SKIP_HOOKS=1 git commit -m x",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_husky_nonzero_not_flagged() {
        // HUSKY disarms only at `=0`; `HUSKY=1` (or any non-zero) is a normal env
        // set that must NOT trip hook_integrity — guards the `val == "0"`
        // discrimination against a false positive.
        let p = default_policy();
        let v = evaluate("HUSKY=1 npm test", &ctx(&p, "feat/x"));
        assert!(!has_rule(&v, "git.hook_integrity"), "{v:?}");
    }

    #[test]
    fn test_integrity_file_writes_blocked() {
        let p = default_policy();
        for cmd in [
            "rm -f .git/hooks/pre-commit",
            "rm -rf .codeflow/git-hooks",
            "mv .codeflow/git-hooks /tmp/x",
            "chmod -x .git/hooks/pre-push",
            "echo bad > .codeflow/policy.json",
            "echo more >> .codeflow/project.toml",
            "tee .codeflow/policy.json",
            "cp /tmp/evil.json .codeflow/policy.json",
            "sed -i s/block/off/ .codeflow/policy.json",
            "git rm .codeflow/policy.json",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_integrity_reads_allowed() {
        let p = default_policy();
        for cmd in [
            "cat .codeflow/policy.json",
            "cp .codeflow/policy.json /tmp/backup.json",
            "git config --get core.hooksPath",
            "grep block .codeflow/policy.json",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn test_hook_integrity_off_is_silent() {
        let p = GitPolicy {
            hook_integrity: PolicyLevel::Off,
            ..default_policy()
        };
        assert!(evaluate("rm -rf .git/hooks", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git config core.hooksPath /x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_hook_integrity_warn_does_not_block() {
        // A project that relaxes hook_integrity to warn in policy.json gets warn,
        // not block, behavior.
        let p = GitPolicy {
            hook_integrity: PolicyLevel::Warn,
            ..default_policy()
        };
        let v = evaluate("rm -rf .git/hooks", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.hook_integrity");
        assert_eq!(v[0].level, PolicyLevel::Warn);
    }

    // -- wrapper evasions (TOKEN DETECTION HARDENING 4a) --

    #[test]
    fn test_wrapped_git_on_protected_still_caught() {
        // Session branch is main; each wrapper hides the commit from the naive
        // first-word check, but expansion re-exposes it.
        let p = default_policy();
        for cmd in [
            "(git commit -m x)",
            "{ git commit -m x; }",
            "bash -c 'git commit -m x'",
            "sh -c \"git commit -m x\"",
            "true\ngit commit -m x",
            "foo & git commit -m x",
            "$(git commit -m x)",
            "`git commit -m x`",
            "true; (git commit -m x)",
        ] {
            let v = evaluate(cmd, &ctx(&p, "main"));
            assert!(has_rule(&v, "git.commit_to_protected"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_chained_checkout_then_wrapped_commit() {
        // checkout main (tracked) then a wrapped commit — caught on main.
        let p = default_policy();
        let v = evaluate("git checkout main && (git commit -m x)", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_wrapper_does_not_false_positive_on_feature() {
        let p = default_policy();
        assert!(evaluate("(git commit -m x)", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("bash -c 'git commit -m x'", &ctx(&p, "feat/x")).is_empty());
    }

    // -- effective target resolution (4b) --

    #[test]
    fn test_retarget_dash_c_evaluated_against_target_branch() {
        // Session is feat/x; -C points at a repo the resolver says is on main.
        let p = default_policy();
        let resolver = |_dir: &str| Some("main".to_string());
        let v = evaluate(
            "git -C /root commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_retarget_cd_then_commit() {
        let p = default_policy();
        let resolver = |_dir: &str| Some("main".to_string());
        let v = evaluate(
            "cd /root && git commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_retarget_git_dir_env_prefix() {
        let p = default_policy();
        let resolver = |_dir: &str| Some("main".to_string());
        let v = evaluate(
            "GIT_DIR=/root/.git git commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_retarget_to_feature_dir_allowed() {
        // Resolver reports the target repo is on a feature branch — allowed.
        let p = default_policy();
        let resolver = |_dir: &str| Some("feat/y".to_string());
        assert!(evaluate(
            "git -C /other commit -m x",
            &ctx_with_dir_branch(&p, "main", &resolver)
        )
        .is_empty());
    }

    #[test]
    fn test_retarget_resolver_none_falls_back_to_session_branch() {
        // The resolver cannot read the target repo's branch (returns None); the
        // guard falls back to the session branch (documented residual — the
        // target repo's git-hook plane backstops). Session on a feature branch:
        // allowed; session on a protected branch: still blocked.
        let p = default_policy();
        let resolver = |_dir: &str| None;
        assert!(evaluate(
            "git -C /root commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver)
        )
        .is_empty());
        let v = evaluate(
            "git -C /root commit -m x",
            &ctx_with_dir_branch(&p, "main", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    // -- unknown ref-writers (4c) --

    #[test]
    fn test_update_ref_self_move_protected_blocked() {
        let p = default_policy();
        let v = evaluate(
            "git update-ref refs/heads/main deadbeef",
            &ctx(&p, "feat/x"),
        );
        assert!(has_rule(&v, "git.local_ref_protection"), "{v:?}");
    }

    #[test]
    fn test_update_ref_delete_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git update-ref -d refs/heads/main", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.delete_protected"), "{v:?}");
    }

    #[test]
    fn test_update_ref_feature_branch_allowed() {
        let p = default_policy();
        assert!(evaluate("git update-ref refs/heads/feat/x abc", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_symbolic_ref_and_fast_import_blocked() {
        let p = default_policy();
        let v = evaluate("git symbolic-ref HEAD refs/heads/main", &ctx(&p, "feat/x"));
        assert!(
            has_rule(&v, "git.local_ref_protection"),
            "symbolic-ref: {v:?}"
        );
        let v = evaluate("git fast-import", &ctx(&p, "feat/x"));
        assert!(
            has_rule(&v, "git.local_ref_protection"),
            "fast-import: {v:?}"
        );
    }

    // -- refs/remotes oracle poisoning (REFERENCE-TRANSACTION ORACLE, 2a) --

    #[test]
    fn test_update_ref_remote_tracking_protected_blocked() {
        let p = default_policy();
        for cmd in [
            "git update-ref refs/remotes/origin/main deadbeef",
            "git update-ref refs/remotes/upstream/master deadbeef",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.local_ref_protection"), "{cmd}: {v:?}");
            assert!(v[0].message.contains("remote-tracking"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_update_ref_remote_tracking_feature_allowed() {
        let p = default_policy();
        assert!(evaluate(
            "git update-ref refs/remotes/origin/feat/x abc",
            &ctx(&p, "feat/x")
        )
        .is_empty());
    }

    // -- bulk push (4d) --

    #[test]
    fn test_bulk_push_reaches_protected() {
        let p = default_policy();
        for cmd in [
            "git push --all origin",
            "git push origin refs/heads/*:refs/heads/*",
            "git push origin '*:*'",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.push_to_protected"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_mirror_push_flags_force_and_delete() {
        let p = default_policy();
        let v = evaluate("git push --mirror origin", &ctx(&p, "feat/x"));
        // --mirror force-updates and can delete protected branches.
        assert!(has_rule(&v, "git.force_push_protected"), "{v:?}");
        assert!(has_rule(&v, "git.delete_protected"), "{v:?}");
    }

    #[test]
    fn test_normal_feature_push_not_bulk() {
        let p = default_policy();
        assert!(evaluate("git push origin feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    // -- override-laundering extra forms (OVERRIDE-LAUNDERING, 5) --

    #[test]
    fn test_laundering_extra_forms_blocked() {
        let p = default_policy();
        for cmd in [
            "/usr/bin/env CODEFLOW_HUMAN_OVERRIDE=1 git merge feat/y",
            "command env CODEFLOW_HUMAN_OVERRIDE=1 git merge feat/y",
            "declare -x CODEFLOW_HUMAN_OVERRIDE=1",
            "typeset -x CODEFLOW_INTEGRATE_TOKEN=abc",
            "readonly CODEFLOW_HUMAN_OVERRIDE=1",
            "git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git merge'",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(
                has_rule(&v, "git.override_token_laundering"),
                "{cmd}: {v:?}"
            );
        }
    }

    #[test]
    fn test_laundering_extra_forms_do_not_flag_unrelated() {
        let p = default_policy();
        assert!(evaluate("/usr/bin/env FOO=1 git status", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("declare -x EDITOR=vim", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git config alias.st status", &ctx(&p, "feat/x")).is_empty());
    }

    // -- review round 2: generalized matchers (B1-B7) --

    #[test]
    fn test_b1_hooks_path_case_insensitive() {
        let p = default_policy();
        for cmd in [
            "git config core.hookspath /tmp/x",
            "git config CORE.HOOKSPATH /tmp/x",
            "git -c core.HooksPath=/dev/null commit -m x",
        ] {
            assert!(
                has_rule(&evaluate(cmd, &ctx(&p, "feat/x")), "git.hook_integrity"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_b2_integrity_path_normalized_spellings() {
        let p = default_policy();
        for cmd in [
            "rm -rf .codeflow//policy.json",
            "rm .codeflow/./policy.json",
            "rm -rf .git//hooks",
            "rm -rf .git/hooks/",
            "tee ./.codeflow/policy.json",
        ] {
            assert!(
                has_rule(&evaluate(cmd, &ctx(&p, "feat/x")), "git.hook_integrity"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_b3_redirect_fd_and_clobber_forms() {
        let p = default_policy();
        for cmd in [
            "echo x 1> .codeflow/policy.json",
            "echo x 2> .git/hooks/pre-commit",
            "echo x >| .codeflow/policy.json",
            "echo x 1>>.codeflow/project.toml",
            "echo x >|.codeflow/policy.json",
        ] {
            assert!(
                has_rule(&evaluate(cmd, &ctx(&p, "feat/x")), "git.hook_integrity"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_b3_fd_dup_and_normal_redirect_not_flagged() {
        let p = default_policy();
        // fd duplication/close is not a file write; a redirect to a normal file
        // is fine — including the `>&`/`&>` forms whose target is an fd, not a
        // filename (the N1 fix must not over-block these).
        for cmd in [
            "git status 2>&1",
            "echo hi > out.txt",
            "echo hi 2> /tmp/err.log",
            "echo hi >& out.txt",
            "make 1>&2",
            "exec 3>&-",
            "cargo test &> out.txt",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn test_n1_both_streams_write_to_integrity_path_blocked() {
        let p = default_policy();
        // csh/bash `>&file` and `&>file` write BOTH streams to `file`. When that
        // file is an integrity path it is a disarm write, not an fd dup — the
        // round-2 `>&`-exclusion (added for `2>&1`) had over-excluded these.
        for cmd in [
            "echo x >&.codeflow/policy.json",
            "echo x >& .codeflow/policy.json",
            "echo x &>.codeflow/policy.json",
            "echo x &>>.codeflow/project.toml",
            "echo x >>&.git/hooks/pre-commit",
        ] {
            assert!(
                has_rule(&evaluate(cmd, &ctx(&p, "feat/x")), "git.hook_integrity"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_b4_git_config_env_injection() {
        let p = default_policy();
        for cmd in [
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null git commit -m x",
            "GIT_CONFIG_PARAMETERS='core.hooksPath=/x' git commit -m x",
        ] {
            assert!(has_rule(&evaluate(cmd, &ctx(&p, "feat/x")), "git.hook_integrity"), "{cmd}");
        }
        // A non-hooksPath GIT_CONFIG_* injection is not an integrity concern.
        assert!(evaluate(
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=user.email GIT_CONFIG_VALUE_0=x git status",
            &ctx(&p, "feat/x")
        )
        .is_empty());
    }

    #[test]
    fn test_b6_env_prefixed_and_clustered_wrappers() {
        let p = default_policy();
        // Each hides a commit on main; strip_launchers (env/command/builtin/exec)
        // + clustered -c re-expose it.
        for cmd in [
            "bash -lc 'git commit -m x'",
            "env FOO=1 bash -c 'git commit -m x'",
            "env FOO=1 git commit -m x",
            "command git commit -m x",
            "exec git commit -m x",
            "builtin git commit -m x",
            "sh -ec 'git commit -m x'",
        ] {
            assert!(
                has_rule(&evaluate(cmd, &ctx(&p, "main")), "git.commit_to_protected"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn test_b7_eval_wrapper() {
        let p = default_policy();
        assert!(has_rule(
            &evaluate("eval 'git commit -m x'", &ctx(&p, "main")),
            "git.commit_to_protected"
        ));
        assert!(has_rule(
            &evaluate("eval git commit -m x", &ctx(&p, "main")),
            "git.commit_to_protected"
        ));
    }

    #[test]
    fn test_round2_no_false_positives() {
        let p = default_policy();
        // Normal operations that MUST stay allowed (reviewer's FP set).
        assert!(evaluate("git config user.email you@example.com", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git config --global user.name 'A B'", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("bash script.sh", &ctx(&p, "main")).is_empty());
        assert!(evaluate("bash -x script.sh", &ctx(&p, "main")).is_empty());
        assert!(evaluate("env NODE_ENV=test npm test", &ctx(&p, "main")).is_empty());
        // `git -C <subdir>` resolving to a feature branch stays allowed.
        let resolver = |_dir: &str| Some("feat/y".to_string());
        assert!(evaluate(
            "git -C sub status",
            &ctx_with_dir_branch(&p, "main", &resolver)
        )
        .is_empty());
    }
}
