//! `git-guard` — the `PreToolUse` shell hook (charter §3.3, §6.1 plane 2).
//!
//! Intercepts git operations the client-side git hooks can't reach:
//! force-push / push / delete against protected branches, hard reset on a
//! protected branch, checkout-and-commit dodges, raw merges on protected,
//! and AI attribution or emoji in `gh pr create` and `gh pr edit` bodies
//! (charter §6.4, AC #13). Every rule reads its level from `policy.json.git` — the guard
//! gives instant in-session feedback; CI + remote protection stay the hard
//! line (D19).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::security::pattern::is_path_targeted;

use super::git_target::{
    self, assignment, expand_word, flat_top_level, has_substitution, join_path, launcher_env,
    map_top_level, substitution_placeholder, Cwd, Join, ShellState, Val, GIT_LOCATION_VARS,
    SUBSTITUTED_BARE,
};
use super::policy::{GitPolicy, PolicyLevel};
use super::{standards, Violation, HUMAN_OVERRIDE_ENV, INTEGRATE_TOKEN_ENV};
use crate::root_checkout::RootCheckout;

/// Resolves a `gh pr merge <target>` argument (a PR number, URL, or branch;
/// empty means the current branch's PR) to its base branch name, when it can
/// be determined. Returns `None` on any doubt — an unreachable API, an
/// unrecognized argument — so the guard blocks conservatively. The CLI wires a
/// bounded `gh pr view` implementation; tests inject a stub.
pub type PrBaseLookup<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// A repository location a git op was moved to by `cd`, `-C`, `--git-dir` or
/// `GIT_DIR`: a path relative to the session cwd, or absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retarget<'s> {
    /// The path, relative to the session cwd or absolute.
    pub path: &'s str,
    /// `true` when the path names a git directory (`--git-dir`, `GIT_DIR`)
    /// rather than a directory inside a working tree.
    pub git_dir: bool,
}

/// What the resolver reads from a retargeted repository.
#[derive(Debug, Clone)]
pub struct TargetRepo {
    /// The branch checked out there (empty on a detached HEAD).
    pub branch: String,
    /// The repository's own git policy, or `None` when it is the session
    /// repository (same common git dir), whose policy is already in force. A
    /// repository with no policy file carries the defaults, which protect
    /// `main` and `master`.
    pub policy: Option<GitPolicy>,
    /// The repository's root checkout when the location is one, judged by
    /// its own policy; `None` in a linked worktree (TSK-165).
    pub root: Option<RootCheckout>,
}

/// Resolves a retargeted repository location to its branch and policy, so a
/// git op aimed at another repository is judged by *that* repository's
/// branch and rules, not the session's (charter §6.1; TSK-112). `None` means
/// the repository could not be read; the guard then keeps its earlier
/// reading, which includes the session branch and policy, and says so (SPC-013
/// planning resolution 13). The CLI wires a git2 reader; tests inject a stub.
pub type DirTargetLookup<'a> = Option<&'a dyn Fn(&Retarget<'_>) -> Option<TargetRepo>>;

/// An alias lookup for a git subcommand that is not a git builtin (TSK-112).
#[derive(Debug, Clone, Copy)]
pub struct AliasQuery<'s> {
    /// Where git would run: `None` for the session's working directory.
    pub target: Option<Retarget<'s>>,
    /// The command's own `-c <name>=<value>` settings, in order. They can
    /// define the alias or an `include.path` that does.
    pub config: &'s [String],
    /// The subcommand as written.
    pub name: &'s str,
}

/// What an alias lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasAnswer {
    /// No `alias.<name>` is set: git runs no alias.
    NotAlias,
    /// The alias value, as `git config --get` prints it.
    Expansion(String),
    /// The configuration could not be read; the reason.
    Unreadable(String),
}

/// Resolves a git alias the way git reads it: `git config --get
/// alias.<name>` in the target repository with the command's own `-c`
/// settings, so `include.path` and conditional includes apply. The CLI wires
/// [`read_alias`]; tests inject a stub. `None` resolves nothing, so any
/// subcommand that is not a builtin is unclassifiable.
pub type AliasLookup<'a> = Option<&'a dyn Fn(&AliasQuery<'_>) -> AliasAnswer>;

/// Read `alias.<name>` for the guard by running `git config --get` where the
/// command would run (`query.target`, relative to `cwd`), with the command's
/// own `-c` settings. Exit 1 means the key is not set; any other failure is
/// [`AliasAnswer::Unreadable`].
#[must_use]
pub fn read_alias(cwd: &std::path::Path, query: &AliasQuery<'_>) -> AliasAnswer {
    let mut cmd = crate::git::command();
    cmd.current_dir(cwd).stdin(std::process::Stdio::null());
    match query.target {
        Some(t) if t.git_dir => {
            cmd.arg(format!("--git-dir={}", t.path));
        }
        Some(t) => {
            cmd.arg("-C").arg(t.path);
        }
        None => {}
    }
    for setting in query.config {
        cmd.arg("-c").arg(setting);
    }
    cmd.args(["config", "--get", &format!("alias.{}", query.name)]);
    match cmd.output() {
        Ok(out) if out.status.success() => AliasAnswer::Expansion(
            String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_string(),
        ),
        Ok(out) if out.status.code() == Some(1) => AliasAnswer::NotAlias,
        Ok(out) => AliasAnswer::Unreadable(format!(
            "`git config` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => AliasAnswer::Unreadable(format!("git could not run: {e}")),
    }
}

/// Read a retargeted repository for the guard: its branch and, unless it
/// shares the session repository's common git dir (`session_common`), its own
/// effective git policy, which is the defaults when it has no policy file.
/// A relative `spec.path` is taken from `cwd`. A git-dir spec opens exactly
/// that git directory, as git does with `--git-dir`/`GIT_DIR`; any other
/// path is discovered upward, as `-C` and `cd` are. `None` when the path does
/// not exist or is not in a repository.
#[must_use]
pub fn read_target(
    cwd: &std::path::Path,
    session_common: Option<&std::path::Path>,
    spec: &Retarget<'_>,
) -> Option<TargetRepo> {
    let p = std::path::Path::new(spec.path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    if !abs.exists() {
        return None;
    }
    let repo = if spec.git_dir {
        git2::Repository::open_ext(
            &abs,
            git2::RepositoryOpenFlags::NO_SEARCH,
            std::iter::empty::<&std::ffi::OsStr>(),
        )
        .ok()?
    } else {
        let start = if abs.file_name().is_some_and(|n| n == ".git") {
            abs.parent()
                .map_or(abs.clone(), std::path::Path::to_path_buf)
        } else {
            abs
        };
        git2::Repository::discover(&start).ok()?
    };
    // Empty on a detached HEAD, which no branch rule protects.
    let branch = super::repo::current_branch(&repo);
    let same = session_common.is_some_and(|s| same_path(s, repo.commondir()))
        && git2::Repository::discover(cwd)
            .ok()
            .and_then(|session| session.workdir().map(Path::to_path_buf))
            .zip(repo.workdir())
            .is_some_and(|(session, target)| same_path(&session, target));
    let policy = if same {
        None
    } else {
        Some(
            super::landed_policy::load(repo.workdir().unwrap_or(repo.path()))
                .ok()?
                .policy
                .git,
        )
    };
    let root = repo
        .workdir()
        .filter(|_| crate::root_checkout::is_root_checkout(&repo))
        .and_then(|dir| {
            super::landed_policy::load(dir)
                .ok()
                .and_then(|authority| RootCheckout::read(&repo, &authority.policy.git))
        });
    Some(TargetRepo {
        branch,
        policy,
        root,
    })
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Parsed `PreToolUse` hook payload (the fields the guard reads).
///
/// Claude and Codex send `snake_case` (`tool_name`, `tool_input`). Grok Build
/// sends `camelCase` (`toolName`, `toolInput`) with shell tool `run_terminal_command`.
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    #[serde(default, alias = "toolName")]
    pub tool_name: String,
    #[serde(default, alias = "toolInput")]
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

/// Why a hook payload was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// The input is not a JSON object, or a field the guard reads has the
    /// wrong type: the harness entry that runs the guard did not pass the
    /// payload through unchanged. The payload ignores fields it does not
    /// read, so these are the only ways a payload fails.
    Malformed(String),
}

impl std::fmt::Display for PayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "not a JSON hook payload: {why}"),
        }
    }
}

impl HookPayload {
    /// Parse the hook JSON from stdin.
    ///
    /// # Errors
    ///
    /// [`PayloadError::Malformed`] when the input is not a JSON object or a
    /// field the guard reads has the wrong type; the message names the field.
    pub fn parse(json: &str) -> Result<Self, PayloadError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| PayloadError::Malformed(e.to_string()))?;
        if !value.is_object() {
            return Err(PayloadError::Malformed(format!(
                "a JSON {} where an object belongs",
                json_kind(&value)
            )));
        }
        if let Some((field, expected, found)) = wrong_field_type(&value) {
            return Err(PayloadError::Malformed(format!(
                "field `{field}` is a JSON {found} where {expected} belongs"
            )));
        }
        serde_json::from_value(value).map_err(|e| PayloadError::Malformed(e.to_string()))
    }

    /// The command to evaluate when this is a shell tool call.
    /// Claude exposes `PowerShell` as a distinct tool on native Windows; Codex
    /// currently sends the same command shape under its shell hook; Grok Build
    /// sends `run_terminal_command` (aliased from `Bash` in matchers).
    #[must_use]
    pub fn shell_command(&self) -> Option<&str> {
        if matches!(
            self.tool_name.as_str(),
            "Bash" | "PowerShell" | "run_terminal_command"
        ) {
            self.tool_input.command.as_deref()
        } else {
            None
        }
    }
}

/// The first field the guard reads whose value has the wrong type, as
/// (field, expected, found). `null` stands for an absent optional field.
fn wrong_field_type(
    value: &serde_json::Value,
) -> Option<(&'static str, &'static str, &'static str)> {
    use serde_json::Value;
    let field = |names: &[&str]| names.iter().find_map(|name| value.get(*name));
    if let Some(name) = field(&["tool_name", "toolName"]) {
        if !name.is_string() {
            return Some(("tool_name", "a string", json_kind(name)));
        }
    }
    if let Some(input) = field(&["tool_input", "toolInput"]) {
        match input {
            Value::Object(_) => {
                if let Some(command) = input.get("command") {
                    if !(command.is_string() || command.is_null()) {
                        return Some(("tool_input.command", "a string", json_kind(command)));
                    }
                }
            }
            other => return Some(("tool_input", "an object", json_kind(other))),
        }
    }
    if let Some(cwd) = value.get("cwd") {
        if !(cwd.is_string() || cwd.is_null()) {
            return Some(("cwd", "a string", json_kind(cwd)));
        }
    }
    None
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
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
    /// How to resolve a retargeted repository (`-C`/`--git-dir`/`GIT_DIR`/
    /// `cd`) to its branch and policy (injected). `None` disables cross-repo
    /// resolution and the guard evaluates against `current_branch` — the
    /// behavior in unit tests that do not exercise retargeting.
    pub dir_target_lookup: DirTargetLookup<'a>,
    /// How to resolve a subcommand that is not a git builtin to the alias it
    /// names (injected). `None` leaves every such subcommand unclassifiable.
    pub alias_lookup: AliasLookup<'a>,
    /// Read-only local-work proof for the command's actual repository.
    pub discard_lookup: DiscardLookup<'a>,
    /// The session's root checkout, when the command runs in one: a commit
    /// there off its root branch is judged by `git.root_checkout_commits`
    /// (TSK-165). `None` in a linked worktree.
    pub root_checkout: Option<&'a RootCheckout>,
}

/// The outcome of one guard run.
#[derive(Debug, Default)]
pub struct Evaluation {
    /// Rule violations; block-level ones deny the command.
    pub violations: Vec<Violation>,
    /// Disclosures that accompany the verdict, such as a git op whose target
    /// repository could not be resolved, each with the step that clears it.
    pub notes: Vec<crate::remedy::Finding>,
}

/// Evaluate a Bash command against the git policy.
///
/// Returns all violations found; the caller maps block-level violations to
/// exit 2 (deny) and warn-level to stderr advice.
#[must_use]
pub fn evaluate(command: &str, ctx: &GuardContext<'_>) -> Vec<Violation> {
    evaluate_report(command, ctx).violations
}

/// Evaluate a Bash command and return the violations with the notes that
/// disclose how the verdict was reached.
#[must_use]
pub fn evaluate_report(command: &str, ctx: &GuardContext<'_>) -> Evaluation {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    evaluate_report_at(command, ctx, &cwd)
}

/// Evaluate a command using the tool payload's working directory for paths.
#[must_use]
#[allow(clippy::too_many_lines)] // The ordered shell tracker and dispatch share one state transition.
pub fn evaluate_report_at(command: &str, ctx: &GuardContext<'_>, cwd: &Path) -> Evaluation {
    let mut report = Evaluation::default();
    let violations = &mut report.violations;
    // Chained checkout/switch dodges change the branch later segments run on.
    let mut branches = BranchTracker::new(ctx.current_branch);
    let segments = expand_commands(command);
    // Where each segment sits (TSK-112): on a flat line, `Some(join)` for a
    // top-level command and `None` for one nested in a substitution or a
    // `bash -c` string; `None` for the whole line when it is not flat.
    let roles = flat_top_level(command).and_then(|top| map_top_level(&segments, &top));
    let mut shell = ShellState::new(roles.is_some());
    let line = LineFacts::read(&segments, roles.as_deref(), command);
    let mut notes = Vec::new();

    // `expand_commands` unwraps the shell constructs an agent can hide a git
    // token behind — subshells `( )`, brace groups `{ }`, `bash -c '…'`,
    // `$(…)`/backticks, newlines, and backgrounding `&` — so a git/gh
    // invocation is evaluated wherever it sits, not only as a segment's first
    // word (the review's wrapper evasions).
    let mut earlier_mutation = command.contains("GIT_INDEX_FILE");
    for (idx, segment) in segments.iter().enumerate() {
        branches.discard_state_changed = earlier_mutation;
        earlier_mutation |= may_change_discard_state(segment);
        let top_level = match roles.as_ref().map(|r| r[idx]) {
            Some(Some(join)) => {
                shell.begin(join);
                branches.begin(join);
                true
            }
            _ => false,
        };
        let mut tokens = shell_tokens(segment);
        strip_reserved_words(&mut tokens);
        if tokens.is_empty() {
            continue;
        }

        // Hook/policy integrity: writes or removes that would disarm or tamper
        // with the enforcement plane, evaluated on ANY command (not just git).
        let moved = line.moves_for(&shell, top_level, &tokens);
        if let Some(mut v) =
            integrity_write_in_dirs(&tokens, ctx.policy.hook_integrity, cwd, &moved.cwd)
        {
            let authority = v.message.contains(super::edit_guard::AUTHORITY_PATH);
            if authority {
                v.level = PolicyLevel::Block;
                v.level_fixed = true;
            }
            if authority || ctx.policy.hook_integrity.is_active() {
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
            if top_level {
                if tokens[0] == "cd" || plain_pushd(segment) {
                    shell.cd(&dir);
                } else {
                    shell.observe(&tokens[0], &tokens[1..]);
                }
            }
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

        // Dispatch on the argument vector the program receives: unquoted
        // redirections are the shell's, not arguments.
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let launched = strip_launchers(&words);
        if top_level {
            // A redirection can fail, and the assignment with it.
            let redirected = words.len() != tokens.len();
            shell.assign(&tokens, launched.map(|(program, _)| program), redirected);
        }
        let Some((program, args)) = launched else {
            continue;
        };
        if top_level {
            shell.observe(program, args);
        }
        match program_kind(program) {
            ProgramKind::Git => {
                let moved = line.moves_for(&shell, top_level, &tokens);
                check_git(
                    args,
                    &mut branches,
                    &moved,
                    ctx,
                    violations,
                    &mut notes,
                    0,
                    cwd,
                );
            }
            ProgramKind::Gh => check_gh(args, ctx, violations),
            // A program named by a substitution could be git itself.
            ProgramKind::Other if has_substitution(program) => {
                violations.extend(unclassifiable_violation(
                    ctx.policy,
                    None,
                    &substitution_reason("its program name"),
                    &crate::remedy::GUARD_UNCLASSIFIABLE,
                ));
            }
            ProgramKind::Other => {}
        }
    }
    report.notes = notes;
    report
}

/// The argument vector of every simple command in `command`, read the way
/// git-guard reads them (TSK-136): subshells, groups, control-structure
/// bodies, `$(…)`, backticks, `bash -c`/`eval` strings and script heredoc
/// bodies unwrapped; quotes and escapes removed; redirections, leading
/// reserved words (`!`, `time`, `do`, …), assignments and the `env`,
/// `command`, `builtin` and `exec` launchers stripped. The first element is
/// the program as written, which may still hold a substitution.
pub(crate) fn simple_commands(command: &str) -> Vec<Vec<String>> {
    expand_commands(command)
        .iter()
        .filter_map(|segment| {
            let mut words = command_argv(segment);
            strip_reserved_words(&mut words);
            let (program, args) = strip_launchers(&words)?;
            let mut command = Vec::with_capacity(args.len() + 1);
            command.push(program.to_string());
            command.extend(args.iter().cloned());
            Some(command)
        })
        .collect()
}

/// Shell reserved words that can open a segment of a control structure
/// (`do git commit`, `then git push`) or prefix a command (`! git commit`,
/// `time git commit`). The command after them runs all the same.
const LEADING_RESERVED_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "time",
];

/// Drop the reserved words that open a segment, and `time`'s own options, so
/// the command they introduce is judged (TSK-112).
pub(crate) fn strip_reserved_words(words: &mut Vec<String>) {
    let mut at = 0;
    while let Some(word) = words.get(at) {
        let after_time = at > 0 && words[at - 1] == "time" && word.starts_with('-');
        if LEADING_RESERVED_WORDS.contains(&word.as_str()) || after_time {
            at += 1;
        } else {
            break;
        }
    }
    words.drain(..at);
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
    Violation::always_blocking(
        "git.override_token_laundering",
        format!("command sets the override token `{var}` in-session"),
        "override tokens are human-only; setting them in-session is bypass — a human runs the sanctioned path (PR merge / `codeflow integrate` / `CODEFLOW_HUMAN_OVERRIDE`) from their own terminal",
    )
}

fn hook_integrity_violation(level: PolicyLevel, message: String) -> Violation {
    Violation::new(
        "git.hook_integrity",
        level,
        message,
        crate::remedy::HOOK_INTEGRITY.remedy(),
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

/// Inspect assignments before launchers are stripped, including prior exports.
pub(super) fn transport_config_environment(command: &str) -> bool {
    expand_commands(command).iter().any(|segment| {
        let tokens = shell_tokens(segment);
        leading_env_assignments(&tokens).iter().any(|(name, _)| {
            name.starts_with("GIT_CONFIG") || matches!(*name, "HOME" | "XDG_CONFIG_HOME")
        })
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

// Only the directory form has cd semantics; stack indexes and `-n` do not.
fn plain_pushd(segment: &str) -> bool {
    let mut tokens = command_argv(segment);
    strip_reserved_words(&mut tokens);
    let Some((program, args)) = tokens.split_first() else {
        return false;
    };
    let args = if args.first().is_some_and(|arg| arg == "--") {
        &args[1..]
    } else {
        args
    };
    program == "pushd" && matches!(args, [dir] if !dir.starts_with(['+', '-']))
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

/// The hook shims and the integrity files: directory prefixes and exact files
/// whose mutation would disarm or falsify the enforcement plane.
const INTEGRITY_PREFIXES: &[&str] = &[".git/hooks", ".git/refs/remotes", ".codeflow/git-hooks"];
const INTEGRITY_FILES: &[&str] = &[
    ".codeflow/policy.json",
    ".codeflow/project.toml",
    ".git/config",
    ".git/packed-refs",
];

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

fn integrity_write_in_dirs(
    tokens: &[String],
    level: PolicyLevel,
    cwd: &Path,
    dirs: &Cwd,
) -> Option<Violation> {
    match dirs {
        Cwd::Paths(dirs) => dirs
            .iter()
            .find_map(|dir| integrity_write_violation(tokens, level, &cwd.join(dir), cwd)),
        Cwd::Unknown(_) => integrity_write_violation(tokens, level, cwd, cwd),
    }
}

/// The integrity path a single argument token names, when any. The token is
/// normalized first so equivalent spellings match.
fn token_integrity_path(token: &str, cwd: &Path, payload_cwd: &Path) -> Option<&'static str> {
    let path = integrity_shell_path(token, cwd);
    if super::edit_guard::repository_authority_target(&path, payload_cwd, true) {
        return Some(super::edit_guard::AUTHORITY_PATH);
    }
    if super::edit_guard::repository_enforcement_target(&path, payload_cwd, true) {
        return Some("repository enforcement files");
    }
    integrity_target(&normalize_path(token)).or_else(|| {
        let base = integrity_disk_case(payload_cwd);
        let path = integrity_disk_case(&cwd.join(token));
        if let Some(protected) = root_dot_pattern_target(&path) {
            return Some(protected);
        }
        // A derived absolute path is not a typed scratch-copy exemption.
        // Keep relative spellings (including `..`) relative to the tool cwd.
        let target = path
            .strip_prefix(&base)
            .ok()
            .or_else(|| Path::new(token).is_absolute().then_some(path.as_path()))?;
        integrity_target(&normalize_path(&target.to_string_lossy()))
    })
}

fn integrity_shell_path(token: &str, cwd: &Path) -> PathBuf {
    if let Some(relative) = token.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(relative);
        }
    }
    for name in ["HOME", "XDG_CONFIG_HOME", "GIT_CONFIG_GLOBAL"] {
        let relative = token
            .strip_prefix(&format!("${name}"))
            .or_else(|| token.strip_prefix(&format!("${{{name}}}")));
        if let Some(relative) = relative.filter(|s| s.is_empty() || s.starts_with('/')) {
            if let Some(value) = std::env::var_os(name) {
                return cwd.join(value).join(relative.trim_start_matches('/'));
            }
        }
    }
    cwd.join(token)
}

// Only explicit dot-patterns at a repository root can reach these hidden
// directories; ordinary root globs and patterns below other paths stay ordinary.
// The pattern's own directory decides, wherever the command runs from.
fn root_dot_pattern_target(path: &Path) -> Option<&'static str> {
    let pattern = path.file_name()?.to_str()?;
    if !pattern.starts_with('.') || !pattern.contains(['*', '?', '[', '{']) {
        return None;
    }
    let parent = path.parent()?.canonicalize().ok()?;
    let root = super::RepoInfo::discover(&parent)?
        .root
        .canonicalize()
        .ok()?;
    if parent != root {
        return None;
    }
    INTEGRITY_PREFIXES
        .iter()
        .chain(INTEGRITY_FILES.iter())
        .copied()
        .find(|protected| {
            protected
                .split_once('/')
                .is_some_and(|(root, _)| integrity_glob_matches(pattern, root))
        })
}

// Glob handles stars, questions and brackets. Expand comma braces from the
// innermost pair, without consulting the filesystem or executing the shell.
fn integrity_glob_matches(pattern: &str, name: &str) -> bool {
    if let Some((prefix, tail)) = pattern.rsplit_once('{') {
        if let Some((choices, suffix)) = tail.split_once('}') {
            if choices.contains(',') {
                return choices.split(',').any(|choice| {
                    integrity_glob_matches(&format!("{prefix}{choice}{suffix}"), name)
                });
            }
        }
    }
    glob::Pattern::new(pattern).is_ok_and(|glob| glob.matches(name))
}

fn integrity_disk_case(path: &Path) -> PathBuf {
    let mut real = PathBuf::new();
    for component in path.components() {
        real.push(component);
        if let Ok(metadata) = std::fs::symlink_metadata(&real) {
            super::edit_guard::normalize_case(&mut real, &metadata);
        }
    }
    real
}

fn integrity_target(path: &str) -> Option<&'static str> {
    // A final-component pattern can empty the directory that contains it.
    let target = Path::new(path);
    let path = if target
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(['*', '?', '[', '{']))
    {
        target.parent().and_then(Path::to_str).unwrap_or(path)
    } else {
        path
    };
    INTEGRITY_PREFIXES
        .iter()
        .chain(INTEGRITY_FILES.iter())
        .copied()
        .find(|protected| {
            is_path_targeted(path, protected)
                || Path::new(protected)
                    .ancestors()
                    .skip(1)
                    .filter_map(Path::to_str)
                    .filter(|ancestor| !ancestor.is_empty())
                    .any(|ancestor| {
                        // An ancestor must be the whole target, not a prefix of
                        // an ordinary file such as `.codeflow/notes.md`.
                        (path == ancestor || path.ends_with(&format!("/{ancestor}")))
                            && is_path_targeted(path, ancestor)
                    })
        })
}

/// The integrity path named by any argument in `args`.
fn arg_integrity_path(args: &[String], cwd: &Path, payload_cwd: &Path) -> Option<&'static str> {
    args.iter()
        .find_map(|a| token_integrity_path(a, cwd, payload_cwd))
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
fn redirect_integrity_path(
    tokens: &[String],
    cwd: &Path,
    payload_cwd: &Path,
) -> Option<&'static str> {
    let mut i = 0;
    while i < tokens.len() {
        match redirect_target(&tokens[i]) {
            Some(RedirectTarget::Attached(t)) => {
                if let Some(p) = token_integrity_path(t, cwd, payload_cwd) {
                    return Some(p);
                }
            }
            Some(RedirectTarget::Next) => {
                if let Some(p) = tokens
                    .get(i + 1)
                    .and_then(|n| token_integrity_path(n, cwd, payload_cwd))
                {
                    return Some(p);
                }
            }
            None => {}
        }
        i += 1;
    }
    None
}

fn rsync_dry_run(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--dry-run"
            || arg
                .strip_prefix('-')
                .is_some_and(|flags| !flags.starts_with('-') && flags.contains('n'))
    })
}

// Find's traversal roots are separate from predicate operands and -exec data.
fn find_mutating_roots(args: &[String]) -> Option<&[String]> {
    let start = args
        .iter()
        .take_while(|arg| matches!(arg.as_str(), "-H" | "-L" | "-P"))
        .count();
    let rest = &args[start..];
    let end = rest
        .iter()
        .position(|arg| arg.starts_with('-') || matches!(arg.as_str(), "!" | "("))
        .unwrap_or(rest.len());
    let roots = &rest[..end];
    let mut at = end;
    while let Some(arg) = rest.get(at) {
        match arg.as_str() {
            "-delete" => return Some(roots),
            "-exec" | "-execdir" => {
                let tail = &rest[at + 1..];
                let end = tail
                    .iter()
                    .position(|arg| matches!(arg.as_str(), ";" | "+"))
                    .unwrap_or(tail.len());
                if strip_launchers(&tail[..end])
                    .is_some_and(|(program, _)| matches!(basename(program), "rm" | "chmod"))
                {
                    return Some(roots);
                }
                at += end + 1;
            }
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename" | "-regex"
            | "-iregex" | "-type" | "-xtype" | "-user" | "-group" | "-uid" | "-gid" | "-perm"
            | "-size" | "-links" | "-inum" | "-mtime" | "-mmin" | "-atime" | "-amin" | "-ctime"
            | "-cmin" | "-newer" | "-anewer" | "-cnewer" | "-newermt" | "-maxdepth"
            | "-mindepth" | "-printf" | "-fprint" | "-fprint0" | "-fls" => at += 1,
            "-fprintf" => at += 2,
            _ => {}
        }
        at += 1;
    }
    None
}

/// Block a Bash write/remove that would disarm or falsify the enforcement
/// plane: a redirect into, or a mutating command targeting, the hook shims
/// (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
/// (`.codeflow/policy.json`, `.codeflow/project.toml`). Reads (`cat`, a `cp`
/// *from* an integrity path) stay allowed.
fn integrity_write_violation(
    tokens: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
) -> Option<Violation> {
    if let Some(p) = redirect_integrity_path(tokens, cwd, payload_cwd) {
        return Some(hook_integrity_violation(
            level,
            format!("redirect would overwrite the integrity path `{p}`"),
        ));
    }
    let (program, args) = strip_launchers(tokens)?;
    let cmd = basename(program);

    let write_args = match cmd {
        "find" => find_mutating_roots(args),
        "rm" | "unlink" | "mv" | "tee" | "truncate" | "shred" | "chmod" | "chown" | "install" => {
            Some(args)
        }
        _ => None,
    };
    if let Some(p) = write_args.and_then(|paths| {
        if paths.is_empty() && cmd == "find" {
            token_integrity_path(".", cwd, payload_cwd)
        } else {
            arg_integrity_path(paths, cwd, payload_cwd)
        }
    }) {
        return Some(hook_integrity_violation(
            level,
            format!("`{cmd}` targets the integrity path `{p}`"),
        ));
    }
    if cmd == "dd" {
        if let Some(p) = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("of="))
            .find_map(|path| token_integrity_path(path, cwd, payload_cwd))
        {
            return Some(hook_integrity_violation(
                level,
                format!("`dd` writes the integrity path `{p}`"),
            ));
        }
    }
    if cmd == "sed" && requests_in_place(args) {
        if let Some(p) = arg_integrity_path(args, cwd, payload_cwd) {
            return Some(hook_integrity_violation(
                level,
                format!("`sed -i` edits the integrity path `{p}`"),
            ));
        }
    }
    if matches!(cmd, "cp" | "ln") || (cmd == "rsync" && !rsync_dry_run(args)) {
        // Only the destination is written. A copy or link from an integrity
        // path leaves that source in place.
        if let Some(dest) = args.iter().rev().find(|a| !a.starts_with('-')) {
            if let Some(p) = token_integrity_path(dest, cwd, payload_cwd) {
                return Some(hook_integrity_violation(
                    level,
                    format!("`{cmd}` writes the integrity path `{p}`"),
                ));
            }
        }
    }
    if cmd == "git" && matches!(args.first().map(String::as_str), Some("rm" | "mv")) {
        if let Some(p) = arg_integrity_path(&args[1..], cwd, payload_cwd) {
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
/// `$(…)`/backtick command substitution, `bash -c '…'`, and `eval '…'`. Text
/// the shell never executes stays out: comments and heredoc bodies (see
/// [`split_into_segments`]). Program
/// resolution goes through [`strip_launchers`], so an env/`command` prefix on
/// the wrapper (`env FOO=1 bash -c …`) and a clustered short flag (`bash -lc`)
/// are both handled. A floor-raise, not a solve — arbitrary interpreters
/// (`python3 -c`) and pipe-to-shell (`echo … | sh`) are the genuinely unbounded
/// tail and stay a documented residual (ADR-0009), backstopped by CI + remote.
pub(crate) fn expand_commands(command: &str) -> Vec<String> {
    let mut raw = Vec::new();
    split_into_segments(command, &mut raw, 0, false);

    let mut out = Vec::new();
    for seg in raw {
        let toks = command_argv(&seg);
        out.push(seg);
        let Some((prog, args)) = strip_launchers(&toks) else {
            continue;
        };
        let name = basename(prog);
        if is_shell(name) {
            // `-c`/`--command`, or a clustered short flag containing `c`
            // (`-lc`, `-ec`): the wrapped command is the following argument.
            if let Some(inner) = shell_c_argument(args) {
                split_into_segments(inner, &mut out, 1, false);
            }
        } else if name == "eval" {
            // `eval '<cmd>'` runs its (joined) arguments as a command.
            let joined = args.join(" ");
            split_into_segments(&joined, &mut out, 1, false);
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
///
/// Text the shell does not execute is not a segment: a comment (an unquoted
/// `#` that starts a word) and a heredoc body read as data. A body is data
/// only when every program in the pipeline that reads it is a known data
/// reader ([`reads_as_data`]: `cat > f <<EOF`, `git commit -F - <<EOF`), that
/// pipeline has no group or process substitution and does not continue past
/// the body, and the text it lands in cannot run (`code_context`: set for a
/// substitution whose output is a command, as in `bash -c "$(cat <<EOF …)"`,
/// or whose pipeline turns out to feed a non-reader). Any other body is read
/// as a script, the conservative default. A data body with an unquoted
/// delimiter still runs its `$(…)` and backtick substitutions. Inside
/// `(( … ))` and `$[ … ]` arithmetic, `<<` is a shift and `#` is not a
/// comment.
///
/// Residual (ADR-0009): a data body the same command later runs by another
/// route, such as a heredoc written to a file that a later segment executes,
/// or a data substitution stored in a variable that is later `eval`ed.
#[allow(clippy::too_many_lines)] // one character state machine
fn split_into_segments(command: &str, out: &mut Vec<String>, depth: usize, code_context: bool) {
    if depth > 8 {
        return; // bound pathological nesting
    }
    let chars: Vec<char> = command.chars().collect();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    // Open `((` arithmetic commands and `$[` arithmetic expansions.
    let mut arithmetic = 0usize;
    let mut bracket_arithmetic = 0usize;
    // Open `( … )` subshells and `{ … }` groups.
    let mut groups = 0usize;
    // Heredocs opened on the current line; their bodies follow its newline.
    let mut heredocs: Vec<Heredoc> = Vec::new();
    let mut line = Line::default();
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
            line.substitution(inner, &cur, code_context, out, depth);
            // The segment keeps a placeholder for the text the shell will
            // substitute, so a word built from it is never read as known.
            cur.push(substitution_placeholder(in_double));
            i = ni;
            continue;
        }
        if c == '`' {
            let (inner, ni) = capture_backtick(&chars, i + 1);
            line.substitution(inner, &cur, code_context, out, depth);
            cur.push(substitution_placeholder(in_double));
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
            '#' if arithmetic == 0 && bracket_arithmetic == 0 && starts_word(&chars, i) => {
                // A comment runs to the end of the line; the newline itself
                // still ends the segment and starts any heredoc bodies.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\n' => {
                // A line ending in `|` continues its pipeline past the
                // heredoc bodies, so their readers are not all known yet.
                let continues = ends_with_pipe(&chars, i);
                line.end_segment(out, &mut cur, continues);
                i += 1;
                if !heredocs.is_empty() {
                    let docs = std::mem::take(&mut heredocs);
                    i = consume_heredoc_bodies(
                        &chars,
                        i,
                        &docs,
                        &line,
                        continues,
                        code_context,
                        out,
                        depth,
                    );
                }
                line.finish(continues, out, depth);
            }
            '(' if chars.get(i + 1) == Some(&'(') => {
                arithmetic += 1;
                line.end_segment(out, &mut cur, false);
                i += 2;
            }
            ')' if arithmetic > 0 && chars.get(i + 1) == Some(&')') => {
                arithmetic -= 1;
                line.end_segment(out, &mut cur, false);
                i += 2;
            }
            '$' if chars.get(i + 1) == Some(&'[') => {
                bracket_arithmetic += 1;
                cur.push_str("$[");
                i += 2;
            }
            ']' if bracket_arithmetic > 0 => {
                bracket_arithmetic -= 1;
                cur.push(c);
                i += 1;
            }
            // A group or process substitution in command position belongs to
            // the pipeline it sits in (`cat <<EOF | { bash; }`, `>(sh)`).
            '(' => {
                let in_pipeline = cur.trim().is_empty() || cur.ends_with(['<', '>']);
                if in_pipeline {
                    line.grouped.push(line.pipeline);
                }
                line.end_segment(out, &mut cur, in_pipeline);
                groups += 1;
                i += 1;
            }
            ')' => {
                line.end_segment(out, &mut cur, false);
                groups = groups.saturating_sub(1);
                i += 1;
            }
            ';' => {
                line.end_segment(out, &mut cur, false);
                i += 1;
            }
            // `<<<` is a here-string: its word is ordinary text on this line.
            '<' if chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'<') => {
                cur.push_str("<<<");
                i += 3;
            }
            '<' if arithmetic == 0 && bracket_arithmetic == 0 && chars.get(i + 1) == Some(&'<') => {
                if let Some((mut doc, end)) = parse_heredoc_operator(&chars, i) {
                    doc.pipeline = line.pipeline;
                    doc.in_group = groups > 0;
                    heredocs.push(doc);
                    cur.extend(&chars[i..end]);
                    i = end;
                } else {
                    cur.push_str("<<");
                    i += 2;
                }
            }
            '{' if i + 1 >= chars.len() || chars[i + 1].is_whitespace() => {
                let in_pipeline = cur.trim().is_empty();
                if in_pipeline {
                    line.grouped.push(line.pipeline);
                }
                line.end_segment(out, &mut cur, in_pipeline);
                groups += 1;
                i += 1;
            }
            '}' if i == 0 || chars[i - 1].is_whitespace() => {
                line.end_segment(out, &mut cur, false);
                groups = groups.saturating_sub(1);
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
                line.end_segment(out, &mut cur, false);
                i += if chars.get(i + 1) == Some(&'&') { 2 } else { 1 };
            }
            // `>|` is the clobber-redirect operator, not a pipe — keep it.
            '|' if i > 0 && chars[i - 1] == '>' => {
                cur.push(c);
                i += 1;
            }
            '|' => {
                let or = chars.get(i + 1) == Some(&'|');
                line.end_segment(out, &mut cur, !or);
                i += if or { 2 } else { 1 };
            }
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    line.end_segment(out, &mut cur, false);
    line.finish(false, out, depth);
}

/// The top-level segments of the line being split, each tagged with the
/// pipeline it belongs to, so the programs reading a heredoc are known when
/// its body starts.
#[derive(Default)]
struct Line {
    pipeline: usize,
    segments: Vec<(usize, String)>,
    /// Pipelines holding a group or process substitution, whose readers the
    /// guard does not resolve.
    grouped: Vec<usize>,
    /// Heredoc-bearing substitutions split as data, with their pipeline, to
    /// recheck once the line shows everything that reads their output.
    data_substitutions: Vec<(usize, String)>,
}

impl Line {
    /// Split a `$(…)`/backtick substitution opened after `prefix`. Output that
    /// can run makes its heredocs scripts; otherwise it is split as data now
    /// and, if it holds a heredoc, rechecked by [`Line::finish`].
    fn substitution(
        &mut self,
        inner: String,
        prefix: &str,
        code_context: bool,
        out: &mut Vec<String>,
        depth: usize,
    ) {
        let code = code_context || substitution_output_runs(prefix);
        split_into_segments(&inner, out, depth + 1, code);
        if !code && inner.contains("<<") {
            self.data_substitutions.push((self.pipeline, inner));
        }
    }

    /// `true` when every program of `pipeline` reads data and none of it is
    /// unresolved: no group or process substitution, and, when `continues`,
    /// not the pipeline still open past the line.
    fn pipeline_reads_data(&self, pipeline: usize, continues: bool) -> bool {
        let unresolved =
            self.grouped.contains(&pipeline) || (continues && pipeline == self.pipeline);
        !unresolved
            && self
                .segments
                .iter()
                .filter(|(p, _)| *p == pipeline)
                .all(|(_, segment)| reads_as_data(segment))
    }

    /// Close the line: re-split as script every data substitution whose
    /// pipeline turned out to feed a program that is not a data reader
    /// (`echo "$(cat <<EOF …)" | bash`), then start the next line.
    fn finish(&mut self, continues: bool, out: &mut Vec<String>, depth: usize) {
        for (pipeline, inner) in std::mem::take(&mut self.data_substitutions) {
            if !self.pipeline_reads_data(pipeline, continues) {
                split_into_segments(&inner, out, depth + 1, true);
            }
        }
        self.segments.clear();
        self.grouped.clear();
    }

    /// End the current segment. `pipe` keeps the next segment in the same
    /// pipeline (`|`, or a line ending in `|`); any other boundary starts a
    /// new one.
    fn end_segment(&mut self, out: &mut Vec<String>, cur: &mut String, pipe: bool) {
        let text = std::mem::take(cur);
        let text = text.trim();
        if !text.is_empty() {
            out.push(text.to_string());
            self.segments.push((self.pipeline, text.to_string()));
        }
        if !pipe {
            self.pipeline += 1;
        }
    }
}

/// `true` when the unquoted text before the newline at `i` ends with a pipe
/// (`|`, not `||`), so the pipeline continues on a later line.
fn ends_with_pipe(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && matches!(chars[j - 1], ' ' | '\t') {
        j -= 1;
    }
    j > 0 && chars[j - 1] == '|' && (j < 2 || chars[j - 2] != '|')
}

/// Programs that read a heredoc body, or a substitution's output, as data and
/// never run it. Tools that can execute their input (`awk` `system()`, GNU
/// `sed e`, `perl`) are left out. `python`/`python3`/`node` run their input
/// in their own language, outside the guard's model: the documented
/// interpreter residual.
const HEREDOC_DATA_READERS: &[&str] = &[
    "cat", "tee", "echo", "printf", "grep", "sort", "head", "tail", "wc", "tr", "cut", "uniq",
    "diff", "jq", "base64", "python", "python3", "node",
];

/// `true` when `segment` reads its input as data: it only assigns, or runs one of
/// [`HEREDOC_DATA_READERS`], `git` in a subcommand that takes a message or
/// patch on stdin with no `-c`/`--config-env` global (either can make any
/// subcommand an alias that runs a shell), or `gh` in a subcommand that takes
/// a body. A body fed to anything else (a shell, `eval`, `ssh`, `sudo`,
/// `make`, a `$VAR` or a function) is read as a script.
fn reads_as_data(segment: &str) -> bool {
    let words = command_argv(segment);
    // A segment of only assignments runs no program and reads nothing. A
    // segment with no words at all is a redirect attached to what came before
    // it (`(bash) <<EOF`), whose reader is unknown.
    let Some((program, args)) = strip_launchers(&words) else {
        return !words.is_empty();
    };
    match basename(program) {
        "git" => {
            !args
                .iter()
                .any(|a| a == "-c" || a.starts_with("--config-env"))
                && git_subcommand(args).is_some_and(|(sub, _)| {
                    matches!(
                        sub,
                        "commit" | "tag" | "notes" | "hash-object" | "apply" | "am"
                    )
                })
        }
        "gh" => args
            .first()
            .is_some_and(|sub| matches!(sub.as_str(), "pr" | "issue" | "api" | "release" | "gist")),
        name => HEREDOC_DATA_READERS.contains(&name),
    }
}

/// `true` when the output of a substitution that opens after `prefix` (the
/// segment text so far) can run as a command: in command position
/// (`$(cat <<EOF …)`), or as an argument of a program that is not a data
/// reader (`bash -c "$(…)"`, `eval "$(…)"`, `sh <<< "$(…)"`). An assignment
/// (`x=$(…)`) or a reader (`gh pr create --body "$(…)"`) keeps it data.
fn substitution_output_runs(prefix: &str) -> bool {
    let argv = command_argv(prefix);
    match strip_launchers(&argv) {
        Some(_) => !reads_as_data(prefix),
        // No program: command position unless the words are all assignments.
        None => {
            argv.is_empty()
                || !argv.iter().all(|w| {
                    w.split_once('=')
                        .is_some_and(|(name, _)| is_identifier(name))
                })
        }
    }
}

/// `true` when the character at `i` begins a shell word, where an unquoted
/// `#` opens a comment. After a closing `)` or a backtick it continues the
/// word instead (`$(x)#y`), so neither counts.
pub(super) fn starts_word(chars: &[char], i: usize) -> bool {
    i == 0 || matches!(chars[i - 1], ' ' | '\t' | '\n' | ';' | '&' | '|' | '(')
}

/// A here-document opened on the current line, waiting for its body.
struct Heredoc {
    delimiter: String,
    /// Any quoting in the delimiter word (`'EOF'`, `"EOF"`, `\EOF`) makes the
    /// body literal; otherwise its `$(…)` and backticks still run.
    literal: bool,
    /// `<<-` strips leading tabs from the body and the terminator line.
    strip_tabs: bool,
    /// The pipeline of the line whose programs read the body.
    pipeline: usize,
    /// Opened inside a subshell or group, whose output may feed anything.
    in_group: bool,
}

/// Parse the `<<`/`<<-` operator at `start` and its delimiter word. Returns
/// the heredoc and the index just past the delimiter, or `None` when no
/// complete delimiter word follows or it holds an unquoted `$` or backtick,
/// whose expansion rules the guard does not model; the lines after it are then
/// read as commands.
fn parse_heredoc_operator(chars: &[char], start: usize) -> Option<(Heredoc, usize)> {
    let mut i = start + 2;
    let strip_tabs = chars.get(i) == Some(&'-');
    if strip_tabs {
        i += 1;
    }
    while matches!(chars.get(i), Some(' ' | '\t')) {
        i += 1;
    }
    let mut delimiter = String::new();
    let mut literal = false;
    let mut quote: Option<char> = None;
    while let Some(&c) = chars.get(i) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => delimiter.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    literal = true;
                }
                '\\' => {
                    literal = true;
                    if let Some(&next) = chars.get(i + 1) {
                        delimiter.push(next);
                        i += 1;
                    }
                }
                '$' | '`' => return None,
                c if c.is_whitespace() || ";&|<>()".contains(c) => break,
                c => delimiter.push(c),
            },
        }
        i += 1;
    }
    if quote.is_some() || (delimiter.is_empty() && !literal) {
        return None;
    }
    Some((
        Heredoc {
            delimiter,
            literal,
            strip_tabs,
            pipeline: 0,
            in_group: false,
        },
        i,
    ))
}

/// Consume the bodies of the heredocs opened on the line that just ended,
/// starting at `start` (just past its newline), and return where command
/// text resumes. A body is data only when its pipeline reads data
/// ([`Line::pipeline_reads_data`]), it is not inside a group, and the text is
/// not in a code context; otherwise it is split as a script. A heredoc whose
/// terminator never appears is left in place, so the rest is still read as
/// commands: the conservative reading.
#[allow(clippy::too_many_arguments)]
fn consume_heredoc_bodies(
    chars: &[char],
    start: usize,
    docs: &[Heredoc],
    line: &Line,
    continues: bool,
    code_context: bool,
    out: &mut Vec<String>,
    depth: usize,
) -> usize {
    let mut i = start;
    for doc in docs {
        let Some((body, next)) = heredoc_body(chars, i, doc) else {
            return i;
        };
        if code_context || doc.in_group || !line.pipeline_reads_data(doc.pipeline, continues) {
            split_into_segments(&body, out, depth + 1, false);
        } else if !doc.literal {
            body_substitutions(&body, out, depth + 1);
        }
        i = next;
    }
    i
}

/// The body lines of `doc` from `start` up to its terminator line, and the
/// index just past that line; `None` when the terminator never appears.
fn heredoc_body(chars: &[char], start: usize, doc: &Heredoc) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut i = start;
    while i < chars.len() {
        let end = chars[i..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(chars.len(), |p| i + p);
        let line: String = chars[i..end].iter().collect();
        let line = if doc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line.as_str()
        };
        if line == doc.delimiter {
            return Some((body, (end + 1).min(chars.len())));
        }
        body.push_str(line);
        body.push('\n');
        i = end + 1;
    }
    None
}

/// Split the `$(…)` and backtick substitutions of an unquoted heredoc body,
/// the only parts of it the shell runs. A backslash escapes `$` and `` ` ``.
fn body_substitutions(body: &str, out: &mut Vec<String>, depth: usize) {
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '$' if chars.get(i + 1) == Some(&'(') => {
                let (inner, next) = capture_balanced(&chars, i + 2);
                split_into_segments(&inner, out, depth, false);
                i = next;
            }
            '`' => {
                let (inner, next) = capture_backtick(&chars, i + 1);
                split_into_segments(&inner, out, depth, false);
                i = next;
            }
            _ => i += 1,
        }
    }
}

/// Capture up to the matching `)` for a `$(` opened before `start`, honoring
/// nesting. Returns the inner text and the index just past the `)`.
pub(super) fn capture_balanced(chars: &[char], start: usize) -> (String, usize) {
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
pub(super) fn capture_backtick(chars: &[char], start: usize) -> (String, usize) {
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

/// The branches git ops could find checked out at one point of the line.
#[derive(Clone)]
struct Checkouts {
    /// The session repository's candidates.
    session: Vec<String>,
    /// `(normalized dir, candidates)` for retargeted repositories; `None` is
    /// the branch the repository had when the line started.
    dirs: Vec<(String, Vec<Option<String>>)>,
    /// Branches a checkout the guard could not place may have left checked
    /// out in any repository.
    anywhere: Vec<String>,
}

fn add_unique<T: PartialEq>(into: &mut Vec<T>, items: impl IntoIterator<Item = T>) {
    for item in items {
        if !into.contains(&item) {
            into.push(item);
        }
    }
}

impl Checkouts {
    fn in_dir(&self, dir: &str) -> Vec<Option<String>> {
        self.dirs
            .iter()
            .find(|(d, _)| d == dir)
            .map_or_else(|| vec![None], |(_, branches)| branches.clone())
    }

    fn set_dir(&mut self, dir: &str, branches: Vec<Option<String>>) {
        self.dirs.retain(|(d, _)| d != dir);
        self.dirs.push((dir.to_string(), branches));
    }

    /// Every branch either state allows.
    fn union(&self, other: &Self) -> Self {
        let mut out = self.clone();
        add_unique(&mut out.session, other.session.iter().cloned());
        add_unique(&mut out.anywhere, other.anywhere.iter().cloned());
        let mut dirs: Vec<&str> = self.dirs.iter().map(|(d, _)| d.as_str()).collect();
        add_unique(&mut dirs, other.dirs.iter().map(|(d, _)| d.as_str()));
        for dir in dirs {
            let mut branches = self.in_dir(dir);
            add_unique(&mut branches, other.in_dir(dir));
            out.set_dir(dir, branches);
        }
        out
    }

    /// `target` checked out at `place`; `narrows` replaces what was there,
    /// otherwise it joins it.
    fn check_out(&mut self, place: Option<&str>, target: &str, narrows: bool) {
        match place {
            None if narrows => self.session = vec![target.to_string()],
            None => add_unique(&mut self.session, [target.to_string()]),
            Some(dir) => {
                let mut branches = if narrows {
                    Vec::new()
                } else {
                    self.in_dir(dir)
                };
                add_unique(&mut branches, [Some(target.to_string())]);
                self.set_dir(dir, branches);
            }
        }
    }
}

/// The branches each git op in a line could run on (TSK-112). A chained
/// `checkout`, `switch` or rebase of a named branch moves them for the ops
/// after it, in the session repository and separately in each retargeted
/// directory. A move can fail, so it replaces the earlier branch only for the
/// commands of its own `&&` list; after `;`, a newline, or anywhere the guard
/// does not model the line, the earlier branch stays a candidate, as a `cd`
/// does for the directory.
struct BranchTracker {
    /// Where the current command could find things.
    now: Checkouts,
    /// Everywhere the current and-list could leave things, at whichever
    /// member it stops.
    list: Checkouts,
    /// A command earlier in the line wrote git configuration or moved a
    /// branch (which `includeIf "onbranch:…"` reads), so an alias read from
    /// disk now may not be what git runs.
    config_changed: bool,
    /// Earlier shell steps may invalidate the disk snapshot for discard checks.
    discard_state_changed: bool,
}

impl BranchTracker {
    fn new(session: &str) -> Self {
        let start = Checkouts {
            session: vec![session.to_string()],
            dirs: Vec::new(),
            anywhere: Vec::new(),
        };
        Self {
            now: start.clone(),
            list: start,
            config_changed: false,
            discard_state_changed: false,
        }
    }

    /// Enter a top-level command joined by `join`.
    fn begin(&mut self, join: Join) {
        if join == Join::Seq {
            self.now = self.list.union(&self.now);
            self.list = self.now.clone();
        }
    }

    /// Record a branch move at `places` (`None` is the session repository).
    /// Only a move at one known place on a modeled top-level command
    /// narrows; a move at one of several places joins each of them.
    fn check_out(&mut self, places: &[Option<String>], target: &str, narrows: bool) {
        let narrows = narrows && places.len() == 1;
        for place in places {
            let dir = place.as_deref().map(normalize_path);
            self.now.check_out(dir.as_deref(), target, narrows);
            self.list.check_out(dir.as_deref(), target, false);
        }
    }

    /// A move the guard could not place: the branch may now be checked out
    /// anywhere.
    fn check_out_anywhere(&mut self, target: &str) {
        add_unique(&mut self.now.anywhere, [target.to_string()]);
        add_unique(&mut self.list.anywhere, [target.to_string()]);
    }

    fn session(&self) -> &[String] {
        &self.now.session
    }

    fn in_dir(&self, dir: &str) -> Vec<Option<String>> {
        self.now.in_dir(&normalize_path(dir))
    }

    fn anywhere(&self) -> &[String] {
        &self.now.anywhere
    }
}

/// What the whole line reveals about moves the tracker cannot follow.
struct LineFacts {
    /// Some segment can move a shell's directory (`cd`, `pushd`, `source`, …).
    any_mover: bool,
    /// On a flat line: some *nested* segment can, or sets a variable.
    nested_mover: bool,
    /// Variables the line mentions that change what git reads.
    mentions: Mentions,
}

/// What a line may change about where git reads its repository and its
/// configuration.
#[allow(clippy::struct_excessive_bools)] // Independent facts may all hold on the same command line.
struct Mentions {
    /// A git location variable (`GIT_DIR`, …).
    location_var: bool,
    /// Where git reads its configuration from (`GIT_CONFIG_*`, `HOME`,
    /// `XDG_CONFIG_HOME`) or a config file path, which the alias reader
    /// cannot see.
    config_env: bool,
    /// Transport cannot trust config supplied by the command being judged.
    transport_env: bool,
    /// A `git config` write whose order the guard does not model: anywhere
    /// on a line that is not flat, or nested in a substitution. Top-level
    /// writes on a flat line are followed in order.
    config_write: bool,
}

impl LineFacts {
    fn read(segments: &[String], roles: Option<&[Option<Join>]>, command: &str) -> Self {
        let mut facts = Self {
            any_mover: false,
            nested_mover: false,
            mentions: Mentions {
                location_var: GIT_LOCATION_VARS.iter().any(|v| command.contains(v)),
                config_env: mentions_config_env(command),
                transport_env: transport_config_environment(command),
                config_write: false,
            },
        };
        for (idx, segment) in segments.iter().enumerate() {
            let mut tokens = shell_tokens(segment);
            let mut words = command_argv(segment);
            // `! cd x`, `then cd x`: the command runs all the same.
            strip_reserved_words(&mut tokens);
            strip_reserved_words(&mut words);
            let program = strip_launchers(&words).map(|(program, _)| program);
            let mover = cd_target(&tokens).is_some()
                || program.is_some_and(|p| git_target::moves_directory(p) || has_substitution(p));
            let nested = roles.is_some_and(|r| r[idx].is_none());
            let unordered = roles.is_none() || nested;
            facts.mentions.config_write |= unordered
                && strip_launchers(&words).is_some_and(|(program, args)| {
                    matches!(program_kind(program), ProgramKind::Git)
                        && git_subcommand(args)
                            .is_some_and(|(sub, rest)| sub == "config" && !config_only_reads(rest))
                });
            facts.any_mover |= mover;
            let sets_var = program.is_none_or(|p| p == "export")
                && tokens.iter().any(|t| assignment(t).is_some());
            facts.nested_mover |= nested && (mover || sets_var);
        }
        facts
    }

    /// Where a git op can run and which variables it can see.
    fn moves_for<'r>(
        &self,
        shell: &'r ShellState,
        top_level: bool,
        tokens: &'r [String],
    ) -> Moves<'r> {
        let (cwd, tracked) = if !shell.flat() {
            let cwd = if self.any_mover {
                Cwd::Unknown("a directory change on a line the guard does not model".to_string())
            } else {
                Cwd::Paths(vec![String::new()])
            };
            (cwd, false)
        } else if !top_level && self.nested_mover {
            let why = "a directory or variable change inside a nested command".to_string();
            (Cwd::Unknown(why), false)
        } else {
            (shell.cwd.clone(), top_level)
        };
        Moves {
            cwd,
            vars: tracked.then_some(&shell.vars),
            // A location variable the line sets outside the op's own launcher
            // environment is only modeled at top level of a flat line.
            location_unknown: self.mentions.location_var && !tracked,
            config_unknown: self.mentions.config_env || self.mentions.config_write,
            transport_env: self.mentions.transport_env,
            narrows: tracked,
            tokens,
        }
    }
}

/// Where the line has moved the directory a git op runs in.
#[allow(clippy::struct_excessive_bools)] // Orthogonal location, config, transport and ordering evidence.
struct Moves<'r> {
    /// The directories the op could run in.
    cwd: Cwd,
    /// Variables the op's words can expand from (`None`: none are known).
    vars: Option<&'r HashMap<String, Val>>,
    /// A git location variable may be set in a way the guard cannot scope.
    location_unknown: bool,
    /// The line may change where git reads its configuration from.
    config_unknown: bool,
    transport_env: bool,
    /// The op is a modeled top-level command, so a branch move it makes
    /// holds for the rest of its `&&` list.
    narrows: bool,
    /// The op's full token list (its leading assignments and launchers).
    tokens: &'r [String],
}

fn check_authority(
    args: &[String],
    moved: &Moves<'_>,
    cwd: &Path,
    violations: &mut Vec<Violation>,
) {
    if moved.transport_env
        && git_subcommand(args).is_some_and(|(sub, _)| matches!(sub, "fetch" | "pull" | "push"))
    {
        violations.push(Violation::always_blocking(
            "git.policy_authority",
            "command-local configuration can redirect the configured remote".into(),
            "use the configured remote without configuration environment overrides",
        ));
        return;
    }
    if let Ok(specs) = compose_targets(args, moved) {
        for spec in specs {
            let target = spec
                .as_ref()
                .map_or_else(|| cwd.to_path_buf(), |s| cwd.join(&s.path));
            if let Err(reason) = super::landed_policy::load(&target) {
                let recovery = git_subcommand(args)
                    .is_some_and(|(sub, rest)| sub == "fetch" && rest.len() <= 1)
                    && super::ref_authority::check(&target, args).is_none();
                if !recovery {
                    violations.push(Violation::always_blocking(
                        "git.policy_authority",
                        reason,
                        "restore the named remote-tracking policy authority",
                    ));
                }
            }
            if let Some(reason) = super::ref_authority::check(&target, args) {
                violations.push(Violation::always_blocking("git.policy_authority", reason, "the operator repairs the configured remote; agents use its ordinary fetch mapping"));
            }
        }
    }
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // Ordered checks share the parsed command and its exact cwd.
fn check_git(
    args: &[String],
    branches: &mut BranchTracker,
    moved: &Moves<'_>,
    ctx: &GuardContext<'_>,
    out: &mut Vec<Violation>,
    notes: &mut Vec<crate::remedy::Finding>,
    depth: usize,
    cwd: &Path,
) {
    check_authority(args, moved, cwd, out);
    let policy = ctx.policy;

    // Global-flag pass, ahead of the subcommand: hook-path override (`git -c
    // core.hooksPath=…` or `git --config-env=core.hooksPath=<VAR>`) disarms
    // the client hooks (ADR-0009).
    let (hooks_path_override, _) = scan_git_globals(args);
    if hooks_path_override && policy.hook_integrity.is_active() {
        out.push(hook_integrity_violation(
            policy.hook_integrity,
            "a git global flag (`-c`/`--config-env` core.hooksPath=…) overrides the hook path for this command"
                .to_string(),
        ));
        return;
    }

    // Classification uncertainty adds its own verdict; it never ends the
    // judgment, so every other applicable rule still runs (R4-1).
    let mut unclassified = unclassifiable_git(args);
    let Some((sub, rest)) = git_subcommand(args) else {
        if let Some(u) = &unclassified {
            out.extend(u.violation(ctx.policy, None));
        }
        return;
    };

    // A subcommand that is not a builtin may be an alias: judge what it
    // expands to, as if written literally. One the guard cannot read is
    // unclassifiable.
    if unclassified.is_none() && !GIT_BUILTINS.contains(&sub) {
        match expand_alias(args, sub, moved, ctx, depth, branches.config_changed) {
            Ok(expansions) => {
                for expanded in expansions {
                    check_git(&expanded, branches, moved, ctx, out, notes, depth + 1, cwd);
                    branches.discard_state_changed |= git_subcommand(&expanded)
                        .is_none_or(|(sub, rest)| !discard_readonly_git(sub, rest));
                }
            }
            Err(why) => {
                unclassified = Some(Unclassified::Alias(format!(
                    "`git {sub}` may be an alias the guard cannot resolve ({why})"
                )));
            }
        }
    }

    let judged = judge_target(args, branches, moved, ctx);
    if let Some(u) = &unclassified {
        let known = match u {
            Unclassified::Subcommand(_) | Unclassified::Alias(_) => None,
            Unclassified::Arguments(_) => Some(sub),
        };
        // A subcommand that acts on the branch it runs on can only reach a
        // protected branch through its target, which is judged below; its
        // unknown arguments matter only there. Any other unknown part can
        // name a protected ref from anywhere.
        let acts_on_current = known.is_some_and(|s| CURRENT_BRANCH_SUBCOMMANDS.contains(&s));
        let exposed = !acts_on_current
            || judged
                .cases
                .iter()
                .any(|(branch, rules, _)| rules.branch_is_protected(branch));
        if exposed {
            let strictest = judged
                .cases
                .iter()
                .filter_map(|(_, rules, _)| u.violation(rules, known))
                .max_by_key(|v| severity(v.level));
            out.extend(strictest);
        }
    }
    check_discard(
        args,
        sub,
        rest,
        moved,
        branches.discard_state_changed,
        ctx,
        out,
    );
    track_branch_move(
        sub,
        rest,
        &judged.track,
        branches,
        ctx.policy,
        moved.narrows,
    );
    if sub == "config" && !config_only_reads(rest) {
        branches.config_changed = true;
    }
    if matches!(sub, "checkout" | "switch") {
        return;
    }

    let mut found: Vec<Violation> = Vec::new();
    for (branch, rules, root) in &judged.cases {
        let view = GuardContext {
            policy: rules,
            current_branch: ctx.current_branch,
            integrate_token: ctx.integrate_token,
            pr_base_lookup: ctx.pr_base_lookup,
            dir_target_lookup: ctx.dir_target_lookup,
            alias_lookup: ctx.alias_lookup,
            discard_lookup: ctx.discard_lookup,
            root_checkout: ctx.root_checkout,
        };
        let mut case = Vec::new();
        judge_git_sub(sub, rest, branch, &view, &mut case);
        case.extend(root_checkout_case(sub, rest, branch, rules, root.as_ref()));
        for v in case {
            if !found
                .iter()
                .any(|f| f.rule == v.rule && f.message == v.message)
            {
                found.push(v);
            }
        }
    }
    if let Some(why) = judged.unresolved {
        notes.push(disclose_unresolved(sub, &why, &mut found));
    }
    if !found.is_empty() && ctx.dir_target_lookup.is_some() {
        if let Ok(specs) = compose_targets(args, moved) {
            for spec in specs.into_iter().flatten() {
                if let Ok(authority) = super::landed_policy::load(&cwd.join(spec.path)) {
                    for finding in &mut found {
                        finding.message.push_str("; target policy source: ");
                        finding.message.push_str(&authority.source);
                    }
                }
            }
        }
    }
    out.extend(found);
}

/// The root-checkout rule for one case (TSK-165): a commit-creating
/// subcommand run in a root checkout off its root branch. A rebase given a
/// `<branch>` checks that branch out first, so it is judged there.
fn root_checkout_case(
    sub: &str,
    rest: &[String],
    branch: &str,
    rules: &GitPolicy,
    root: Option<&RootCheckout>,
) -> Option<Violation> {
    let on = if sub == "rebase" {
        rebase_branch(rest).unwrap_or(branch)
    } else {
        branch
    };
    crate::root_checkout::guard_violation(sub, rest, on, root?, rules.root_checkout_commits)
}

/// Name an unresolved target on each finding judged as protected, and the
/// note that discloses it (TSK-112).
fn disclose_unresolved(sub: &str, why: &str, found: &mut [Violation]) -> crate::remedy::Finding {
    let note = format!(
        "target unresolved: {why}; the guard cannot prove it is not a protected branch, so it judged it as one"
    );
    for v in found.iter_mut() {
        v.message = format!("{} ({note})", v.message);
        v.remedy = crate::remedy::GUARD_UNRESOLVED.remedy();
    }
    crate::remedy::Finding::new(
        format!("`git {sub}`: {note}"),
        crate::remedy::GUARD_UNRESOLVED.remedy(),
    )
}

/// Follow a branch change the op makes for the rest of the line: a checkout
/// or switch, or a rebase given a `<branch>`, which checks it out first. A
/// move in a retargeted dir moves that dir's branch, not the session's; one
/// the guard cannot place may have moved any repository's; a substituted
/// checkout target is assumed to be protected.
fn track_branch_move(
    sub: &str,
    rest: &[String],
    track: &Track,
    branches: &mut BranchTracker,
    policy: &GitPolicy,
    narrows: bool,
) {
    let target = match sub {
        "checkout" | "switch" => checkout_target(rest).map(|target| {
            if has_substitution(&target) {
                assumed_protected_branch(policy).unwrap_or(target)
            } else {
                target
            }
        }),
        "rebase" => rebase_branch(rest).map(str::to_string),
        _ => None,
    };
    let Some(target) = target else {
        return;
    };
    // A conditional include can depend on the branch.
    branches.config_changed = true;
    match track {
        Track::Places(places) => branches.check_out(places, &target, narrows),
        Track::Unplaced => branches.check_out_anywhere(&target),
    }
}

/// Subcommands whose arguments the guard judges. A substitution in their
/// arguments can change what they do to a branch.
const JUDGED_SUBCOMMANDS: &[&str] = &[
    "commit",
    "merge",
    "cherry-pick",
    "rebase",
    "reset",
    "branch",
    "config",
    "update-ref",
    "symbolic-ref",
    "fast-import",
    "push",
    "checkout",
    "switch",
    "restore",
    "clean",
    "stash",
    "worktree",
];

/// Subcommands that act on the branch checked out where they run. Their
/// arguments cannot move them to another branch. `rebase` is not one: its
/// `<branch>` argument checks that branch out first.
const CURRENT_BRANCH_SUBCOMMANDS: &[&str] = &["commit", "merge", "cherry-pick", "reset"];

/// Which part of a git invocation the guard cannot classify.
enum Unclassified {
    /// A substitution in a global option or the subcommand itself: any git
    /// command may run.
    Subcommand(&'static str),
    /// A substitution in the arguments of a subcommand the guard judges.
    Arguments(&'static str),
    /// An alias the guard cannot resolve; the reason. Any git command may run.
    Alias(String),
}

impl Unclassified {
    /// The verdict under `policy`, for the subcommand `sub` when it is known.
    fn violation(&self, policy: &GitPolicy, sub: Option<&str>) -> Option<Violation> {
        match self {
            Self::Subcommand(place) | Self::Arguments(place) => unclassifiable_violation(
                policy,
                sub,
                &substitution_reason(place),
                &crate::remedy::GUARD_UNCLASSIFIABLE,
            ),
            Self::Alias(reason) => {
                unclassifiable_violation(policy, sub, reason, &crate::remedy::GUARD_ALIAS)
            }
        }
    }
}

/// Why a substitution in `place` makes a git command unclassifiable.
fn substitution_reason(place: &str) -> String {
    format!("a command substitution in {place} decides what this git command does")
}

/// Where a substitution makes a git invocation unclassifiable, if anywhere
/// (TSK-112, R3-1): in a global option or at or before the subcommand, where
/// it can change which command runs; or, for a subcommand the guard judges,
/// anywhere unquoted (word splitting can add arguments) and in any argument
/// other than a quoted message value (`-m`, `--message`). A read-only
/// subcommand's arguments cannot turn it into a mutation.
fn unclassifiable_git(args: &[String]) -> Option<Unclassified> {
    let mut idx = 0;
    let sub = loop {
        let t = args.get(idx)?;
        if has_substitution(t) {
            return Some(Unclassified::Subcommand(
                "a global option or the subcommand",
            ));
        }
        if GIT_GLOBAL_VALUE_FLAGS.contains(&t.as_str()) {
            if args.get(idx + 1).is_some_and(|v| has_substitution(v)) {
                return Some(Unclassified::Subcommand("a global option"));
            }
            idx += 2;
        } else if t.starts_with('-') {
            idx += 1;
        } else {
            break t.as_str();
        }
    };
    if !JUDGED_SUBCOMMANDS.contains(&sub) {
        return None;
    }
    let rest = &args[idx + 1..];
    if rest.iter().any(|a| a.contains(SUBSTITUTED_BARE)) {
        return Some(Unclassified::Arguments(
            "an unquoted position, where word splitting can add arguments",
        ));
    }
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        let takes_message = a == "-m"
            || a == "--message"
            || (a.starts_with('-') && !a.starts_with("--") && a.ends_with('m'));
        if takes_message {
            i += 2; // the message value may hold a quoted substitution
            continue;
        }
        let message_inline = a.starts_with("--message=") || (a.starts_with("-m") && a.len() > 2);
        if has_substitution(a) && !message_inline {
            return Some(Unclassified::Arguments(
                "an argument other than the message",
            ));
        }
        i += 1;
    }
    None
}

/// The rules an unknown part of a git invocation could break: every branch
/// rule when the subcommand is unknown (`None`), else the rules of `sub`.
fn exposed_rules(sub: Option<&str>, p: &GitPolicy) -> Vec<(&'static str, PolicyLevel)> {
    let all = [
        ("git.commit_to_protected", p.commit_to_protected),
        ("git.merge_to_protected", p.merge_to_protected),
        ("git.push_to_protected", p.push_to_protected),
        ("git.force_push_protected", p.force_push_protected),
        ("git.delete_protected", p.delete_protected),
        ("git.hard_reset_protected", p.hard_reset_protected),
        ("git.local_ref_protection", p.local_ref_protection),
        ("git.hook_integrity", p.hook_integrity),
        ("git.discard_uncommitted", p.discard_uncommitted),
    ];
    let names: &[&str] = match sub {
        None => return all.to_vec(),
        Some("commit") => &["git.commit_to_protected"],
        Some("merge" | "cherry-pick") => &["git.merge_to_protected"],
        Some("rebase") => &["git.hard_reset_protected"],
        Some("reset") => &["git.hard_reset_protected", "git.discard_uncommitted"],
        Some("push") => &[
            "git.push_to_protected",
            "git.force_push_protected",
            "git.delete_protected",
        ],
        Some("branch") => &[
            "git.delete_protected",
            "git.local_ref_protection",
            "git.discard_uncommitted",
        ],
        Some("config") => &["git.hook_integrity"],
        Some("checkout" | "switch") => &[
            "git.local_ref_protection",
            "git.commit_to_protected",
            "git.discard_uncommitted",
        ],
        Some("restore" | "clean" | "stash" | "worktree") => &["git.discard_uncommitted"],
        Some(_) => &["git.local_ref_protection", "git.delete_protected"],
    };
    all.into_iter()
        .filter(|(name, _)| names.contains(name))
        .collect()
}

/// Order policy levels by strictness.
fn severity(level: PolicyLevel) -> u8 {
    match level {
        PolicyLevel::Block => 2,
        PolicyLevel::Warn => 1,
        PolicyLevel::Allow | PolicyLevel::Off => 0,
    }
}

/// The verdict for a git invocation the guard cannot classify: the strictest
/// of the rules its unknown part could break, under `policy` (charter §6.1:
/// unknown ref-writers deny). `None` when every such rule is off or allows.
fn unclassifiable_violation(
    policy: &GitPolicy,
    sub: Option<&str>,
    reason: &str,
    remedy: &'static crate::remedy::Clearing,
) -> Option<Violation> {
    // The strictest level; on a tie, the first rule listed.
    let (rule, level) = exposed_rules(sub, policy)
        .into_iter()
        .filter(|(_, level)| level.is_active())
        .rev()
        .max_by_key(|(_, level)| severity(*level))?;
    Some(Violation::new(
        rule,
        level,
        format!(
            "command unresolved: {reason}, so the guard cannot prove it leaves protected branches alone"
        ),
        remedy.remedy(),
    ))
}

/// Git's builtin commands (`git --list-cmds=builtins`, Git 2.53). Git runs a
/// builtin even when an alias of the same name exists, so only another name
/// is looked up as an alias.
pub(crate) const GIT_BUILTINS: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "backfill",
    "bisect",
    "blame",
    "branch",
    "bugreport",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout--worker",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-graph",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "credential-cache",
    "credential-cache--daemon",
    "credential-store",
    "describe",
    "diagnose",
    "diff",
    "diff-files",
    "diff-index",
    "diff-pairs",
    "diff-tree",
    "difftool",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "fmt-merge-msg",
    "for-each-ref",
    "for-each-repo",
    "format-patch",
    "fsck",
    "fsck-objects",
    "fsmonitor--daemon",
    "gc",
    "get-tar-commit-id",
    "grep",
    "hash-object",
    "help",
    "hook",
    "index-pack",
    "init",
    "init-db",
    "interpret-trailers",
    "last-modified",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "maintenance",
    "merge",
    "merge-base",
    "merge-file",
    "merge-index",
    "merge-ours",
    "merge-recursive",
    "merge-recursive-ours",
    "merge-recursive-theirs",
    "merge-subtree",
    "merge-tree",
    "mktag",
    "mktree",
    "multi-pack-index",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-redundant",
    "pack-refs",
    "patch-id",
    "pickaxe",
    "prune",
    "prune-packed",
    "pull",
    "push",
    "range-diff",
    "read-tree",
    "rebase",
    "receive-pack",
    "reflog",
    "refs",
    "remote",
    "remote-ext",
    "remote-fd",
    "repack",
    "replace",
    "replay",
    "repo",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-pack",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "sparse-checkout",
    "stage",
    "stash",
    "status",
    "stripspace",
    "submodule--helper",
    "switch",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "update-server-info",
    "upload-archive",
    "upload-archive--writer",
    "upload-pack",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
    "write-tree",
];

/// How deep an alias may expand into further aliases before the guard stops.
const MAX_ALIAS_DEPTH: usize = 8;

/// Does the line touch where git reads its configuration from, or name a git
/// config file it could write? Any mention of a `GIT_CONFIG*` variable,
/// `XDG_CONFIG_HOME`, a `git/config` or `gitconfig` path, or `HOME` as a word
/// not read as `$HOME`, counts.
fn mentions_config_env(command: &str) -> bool {
    if ["GIT_CONFIG", "XDG_CONFIG_HOME", "git/config", "gitconfig"]
        .iter()
        .any(|marker| command.contains(marker))
    {
        return true;
    }
    let bytes = command.as_bytes();
    command.match_indices("HOME").any(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + 4).copied();
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        !before.is_some_and(|b| word(b) || b == b'$' || b == b'{') && !after.is_some_and(word)
    })
}

/// Expand `sub`, a subcommand that is not a builtin, through the alias it
/// names in every repository the op could target (TSK-112). Each result is
/// the op's argument vector with the alias replaced by its expansion, the way
/// git prepends it; a name that is no alias adds nothing. `Err` names why the
/// alias cannot be read: an unresolved target, configuration the reader
/// cannot see, a `!` shell alias, one that starts with an option, a chain
/// deeper than [`MAX_ALIAS_DEPTH`], or a failed lookup.
fn expand_alias(
    args: &[String],
    sub: &str,
    moved: &Moves<'_>,
    ctx: &GuardContext<'_>,
    depth: usize,
    config_changed: bool,
) -> Result<Vec<Vec<String>>, String> {
    if depth >= MAX_ALIAS_DEPTH {
        return Err("an alias chain deeper than the guard follows".to_string());
    }
    let lookup = ctx
        .alias_lookup
        .ok_or_else(|| "no alias reader".to_string())?;
    if moved.config_unknown {
        return Err("the line changes where git reads its configuration".to_string());
    }
    if config_changed {
        return Err(
            "an earlier command in the line writes git configuration or moves a branch".to_string(),
        );
    }
    let at = git_subcommand(args).map_or(args.len(), |(_, rest)| args.len() - rest.len() - 1);
    let mut config = Vec::new();
    let mut idx = 0;
    while idx < at {
        let t = args[idx].as_str();
        if t == "--config-env" || t.starts_with("--config-env=") {
            return Err("`--config-env` sets configuration from the environment".to_string());
        }
        if t == "-c" {
            config.extend(args.get(idx + 1).cloned());
            idx += 2;
        } else if GIT_GLOBAL_VALUE_FLAGS.contains(&t) {
            idx += 2;
        } else {
            idx += 1;
        }
    }
    let specs = compose_targets(args, moved)?;
    let mut expansions: Vec<Vec<String>> = Vec::new();
    for spec in &specs {
        let query = AliasQuery {
            target: spec.as_ref().map(|s| Retarget {
                path: &s.path,
                git_dir: s.git_dir,
            }),
            config: &config,
            name: sub,
        };
        let value = match lookup(&query) {
            AliasAnswer::NotAlias => continue,
            AliasAnswer::Unreadable(why) => return Err(why),
            AliasAnswer::Expansion(value) => value,
        };
        if value.trim_start().starts_with('!') {
            return Err("a `!` shell alias".to_string());
        }
        let words =
            split_alias(&value).ok_or_else(|| "an alias value git cannot split".to_string())?;
        match words.first() {
            None => return Err("an empty alias".to_string()),
            Some(first) if first.starts_with('-') => {
                return Err("an alias that starts with an option".to_string())
            }
            Some(_) => {}
        }
        let mut expanded = args[..at].to_vec();
        expanded.extend(words);
        expanded.extend_from_slice(&args[at + 1..]);
        if !expansions.contains(&expanded) {
            expansions.push(expanded);
        }
    }
    Ok(expansions)
}

/// Split an alias value into words as git's `split_cmdline` does: on
/// whitespace, with single and double quotes grouping and a backslash
/// escaping the next character outside single quotes. `None` for an
/// unterminated quote or a trailing backslash.
fn split_alias(value: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(c),
            (_, '\\') => {
                word.push(chars.next()?);
                in_word = true;
            }
            (Some(_), _) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, _) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// Where a branch move in the judged op happens.
enum Track {
    /// At each of these places (`None` is the session repository).
    Places(Vec<Option<String>>),
    /// Somewhere the guard could not resolve.
    Unplaced,
}

/// A branch a git op is judged on, the policy it is judged by, and the root
/// checkout it runs in, if it runs in one.
type Case<'p> = (String, Cow<'p, GitPolicy>, Option<RootCheckout>);

/// The cases a git op is judged against.
struct Judged<'p> {
    track: Track,
    cases: Vec<Case<'p>>,
    /// Set when the target could not be resolved; the cases then assume a
    /// protected branch.
    unresolved: Option<String>,
}

/// Decide which repository a git op targets (TSK-112).
///
/// When every directory the shell could run the op in resolves to a readable
/// repository, the op is judged against each of them, by that repository's
/// branch and its own policy, and blocks if any of them is protected.
/// Otherwise the target is unresolved: the op is judged as if it ran on a
/// protected branch under the session policy, so a mutation blocks, and the
/// verdict says why and how to make the target resolvable.
fn judge_target<'p>(
    args: &[String],
    branches: &BranchTracker,
    moved: &Moves<'_>,
    ctx: &GuardContext<'p>,
) -> Judged<'p> {
    let resolved = compose_targets(args, moved).and_then(|specs| {
        let mut cases: Vec<Case<'p>> = Vec::new();
        for spec in &specs {
            match spec {
                None => cases.extend(branches.session().iter().map(|b| {
                    (
                        b.clone(),
                        Cow::Borrowed(ctx.policy),
                        ctx.root_checkout.cloned(),
                    )
                })),
                Some(spec) => cases.extend(
                    resolve_target(spec, branches, ctx)
                        .ok_or_else(|| format!("no readable repository at `{}`", spec.path))?,
                ),
            }
        }
        // A branch an unplaced checkout moved may be checked out here too.
        let moved: Vec<_> = cases
            .iter()
            .flat_map(|(_, rules, root)| {
                branches
                    .anywhere()
                    .iter()
                    .map(move |b| (b.clone(), rules.clone(), root.clone()))
            })
            .collect();
        cases.extend(moved);
        let places = specs
            .iter()
            .map(|spec| spec.as_ref().map(|s| s.path.clone()))
            .collect();
        Ok((Track::Places(places), cases))
    });
    match resolved {
        Ok((track, cases)) => Judged {
            track,
            cases,
            unresolved: None,
        },
        Err(why) => {
            let branch = assumed_protected_branch(ctx.policy)
                .or_else(|| branches.session().first().cloned())
                .unwrap_or_default();
            Judged {
                track: Track::Unplaced,
                cases: vec![(branch, Cow::Borrowed(ctx.policy), None)],
                unresolved: Some(why),
            }
        }
    }
}

/// A branch name the policy protects, to judge an unresolved target by.
fn assumed_protected_branch(policy: &GitPolicy) -> Option<String> {
    policy
        .protected_branches
        .iter()
        .map(|p| p.replace(['*', '?'], "x"))
        .find(|b| policy.branch_is_protected(b))
}

/// An owned [`Retarget`].
#[derive(PartialEq, Eq)]
struct TargetSpec {
    path: String,
    git_dir: bool,
}

/// Compose every repository location a git op could target: each directory
/// the shell could be in, then each `-C` in order, then the git dir
/// (`--git-dir`, else a `GIT_DIR` in the op's launcher environment), which git
/// reads relative to the `-C` directory. `None` in the result is the session's
/// own working directory. `Err` names what could not be resolved.
fn compose_targets(args: &[String], moved: &Moves<'_>) -> Result<Vec<Option<TargetSpec>>, String> {
    let no_vars = HashMap::new();
    let vars = moved.vars.unwrap_or(&no_vars);
    if moved.location_unknown {
        return Err(
            "a git location variable (`GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`) set where the guard cannot scope it".to_string(),
        );
    }
    if let Some(name) = GIT_LOCATION_VARS.iter().find(|v| vars.contains_key(**v)) {
        return Err(format!("`{name}` set earlier in the line"));
    }
    let mut git_dir: Option<String> = None;
    for (name, value) in launcher_env(moved.tokens)? {
        match name.as_str() {
            "GIT_DIR" => git_dir = Some(expand_word(&value, vars)?),
            "GIT_COMMON_DIR" | "GIT_WORK_TREE" => {
                return Err(format!("`{name}` in the command's environment"))
            }
            _ => {}
        }
    }
    let mut dash_c: Vec<String> = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        let t = args[idx].as_str();
        let value = || {
            args.get(idx + 1)
                .ok_or_else(|| format!("`{t}` without a value"))
        };
        match t {
            "-C" => {
                dash_c.push(expand_word(value()?, vars)?);
                idx += 2;
            }
            "--git-dir" => {
                git_dir = Some(expand_word(value()?, vars)?);
                idx += 2;
            }
            "-c" | "--config-env" | "--work-tree" | "--namespace" => idx += 2,
            _ => {
                if let Some(v) = t.strip_prefix("--git-dir=") {
                    git_dir = Some(expand_word(v, vars)?);
                } else if !t.starts_with('-') {
                    break; // the subcommand
                }
                idx += 1;
            }
        }
    }
    // A base the -C chain or git dir does not make irrelevant must be known.
    let bases: Vec<Option<String>> = match &moved.cwd {
        Cwd::Paths(paths) => paths.iter().cloned().map(Some).collect(),
        Cwd::Unknown(_) => vec![None],
    };
    let mut specs: Vec<Option<TargetSpec>> = Vec::new();
    for base in bases {
        let mut dir = base;
        for next in &dash_c {
            dir = match dir {
                Some(d) => Some(join_path(&d, next)),
                None if next.starts_with('/') => Some(next.clone()),
                None => None,
            };
        }
        let spec = match (&git_dir, dir) {
            (Some(g), Some(d)) => Some(TargetSpec {
                path: join_path(&d, g),
                git_dir: true,
            }),
            (Some(g), None) if g.starts_with('/') => Some(TargetSpec {
                path: g.clone(),
                git_dir: true,
            }),
            (None, Some(d)) if d.is_empty() => None,
            (None, Some(d)) => Some(TargetSpec {
                path: d,
                git_dir: false,
            }),
            (_, None) => {
                return Err(match &moved.cwd {
                    Cwd::Unknown(why) => why.clone(),
                    Cwd::Paths(_) => "the working directory".to_string(),
                })
            }
        };
        if !specs.contains(&spec) {
            specs.push(spec);
        }
    }
    Ok(specs)
}

fn lookup(spec: &Retarget<'_>, ctx: &GuardContext<'_>) -> Option<TargetRepo> {
    ctx.dir_target_lookup.and_then(|resolver| resolver(spec))
}

/// The branch and policy pairs a resolved target is judged by: each branch
/// an earlier move in the line may have left there, where the branch the
/// resolver read stands for no move, under the target's own policy (the
/// session's when it is the session repository). `None` when the resolver
/// cannot read it.
fn resolve_target<'p>(
    spec: &TargetSpec,
    branches: &BranchTracker,
    ctx: &GuardContext<'p>,
) -> Option<Vec<Case<'p>>> {
    let repo = lookup(
        &Retarget {
            path: &spec.path,
            git_dir: spec.git_dir,
        },
        ctx,
    )?;
    let rules = repo.policy.map_or(Cow::Borrowed(ctx.policy), Cow::Owned);
    Some(
        branches
            .in_dir(&spec.path)
            .into_iter()
            .map(|b| {
                (
                    b.unwrap_or_else(|| repo.branch.clone()),
                    rules.clone(),
                    repo.root.clone(),
                )
            })
            .collect(),
    )
}

/// Judge one git subcommand on `branch` under `ctx.policy`.
#[allow(clippy::too_many_lines)]
fn judge_git_sub(
    sub: &str,
    rest: &[String],
    branch: &str,
    ctx: &GuardContext<'_>,
    out: &mut Vec<Violation>,
) {
    let policy = ctx.policy;
    match sub {
        "commit" => {
            if policy.commit_to_protected.is_active()
                && policy.branch_is_protected(branch)
                && !ctx.integrate_token
            {
                out.push(Violation::new(
                    "git.commit_to_protected",
                    policy.commit_to_protected,
                    format!("`git {sub}` would create commits on protected branch '{branch}'"),
                    crate::remedy::PROTECTED_BRANCH.remedy(),
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
                    crate::remedy::PROTECTED_BRANCH.remedy(),
                ));
            }
            maybe_no_verify(sub, rest, branch, policy, out);
        }
        "rebase" => {
            // `git rebase <upstream> <branch>` checks `<branch>` out first and
            // rewrites it, not the branch the command starts on.
            let branch = rebase_branch(rest).unwrap_or(branch);
            if policy.hard_reset_protected.is_active() && policy.branch_is_protected(branch) {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git rebase` rewrites history on protected branch '{branch}'"),
                    crate::remedy::PROTECTED_REWRITE.remedy(),
                ));
            }
        }
        "reset" => {
            if policy.hard_reset_protected.is_active()
                && policy.branch_is_protected(branch)
                && parse_options(rest, &GIT_RESET_OPTIONS).has_long("--hard")
            {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git reset --hard` on protected branch '{branch}'"),
                    crate::remedy::PROTECTED_REWRITE.remedy(),
                ));
            }
        }
        "branch" => {
            if requests_branch_delete(rest) && policy.delete_protected.is_active() {
                for target in branch_operands(rest)
                    .into_iter()
                    .filter(|t| policy.branch_is_protected(t))
                {
                    out.push(Violation::new(
                        "git.delete_protected",
                        policy.delete_protected,
                        format!("`git branch` would delete protected branch '{target}'"),
                        crate::remedy::PROTECTED_DELETE.remedy(),
                    ));
                }
            }
        }
        "config" => {
            // Hook-path manipulation via config (`git config core.hooksPath …`
            // / `--unset core.hooksPath`) disarms the client hooks. Every read
            // (`git config core.hooksPath`, `--get`, `--list`) stays allowed.
            if policy.hook_integrity.is_active() && config_writes_hooks_path(rest) {
                out.push(hook_integrity_violation(
                    policy.hook_integrity,
                    "`git config` would write core.hooksPath, which changes where git looks for hooks".to_string(),
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
                    crate::remedy::PROTECTED_BRANCH.remedy(),
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

/// `true` when a `git config` invocation can write `core.hooksPath`: a set,
/// add, replace or unset whose operands name the key (a value naming it
/// counts too, so an alias body that rewrites the hook path is caught), a
/// remove or rename of the `core` section, or an interactive edit. Reads —
/// `--get*`, `--list`, the `get`/`list` subcommands, and a bare
/// `git config core.hooksPath` — are allowed whatever scope, file or display
/// options accompany them.
fn config_writes_hooks_path(rest: &[String]) -> bool {
    let parsed = parse_options(rest, &GIT_CONFIG_OPTIONS);
    let any_long = |names: &[&str]| names.iter().any(|name| parsed.has_long(name));
    // Git 2.46+ names the mode as a subcommand word; a config key always
    // contains a dot, so a key never reads as one.
    let subcommand = parsed.operands.first().copied().filter(|word| {
        matches!(
            *word,
            "get" | "set" | "unset" | "list" | "edit" | "rename-section" | "remove-section"
        )
    });
    let operands = &parsed.operands[usize::from(subcommand.is_some())..];
    let names_hooks_path = operands.iter().any(|o| mentions_hooks_path(o));

    if parsed.has_short(&['e']) || any_long(&["--edit"]) || subcommand == Some("edit") {
        return true;
    }
    if any_long(&["--rename-section", "--remove-section"])
        || matches!(subcommand, Some("rename-section" | "remove-section"))
    {
        return names_hooks_path || operands.iter().any(|o| o.eq_ignore_ascii_case("core"));
    }
    let read_mode = parsed.has_short(&['l'])
        || any_long(&[
            "--get",
            "--get-all",
            "--get-regexp",
            "--get-urlmatch",
            "--get-color",
            "--get-colorbool",
            "--list",
        ])
        || matches!(subcommand, Some("get" | "list"));
    let key_write = any_long(&[
        "--add",
        "--append",
        "--replace-all",
        "--unset",
        "--unset-all",
    ]) || matches!(subcommand, Some("set" | "unset"))
        || (!read_mode && operands.len() >= 2);
    key_write && names_hooks_path
}

/// Does this `git config` invocation only read? A get, list or single-name
/// query does; anything that sets, unsets, edits or renames, and any form
/// the guard does not recognize, counts as a write.
fn config_only_reads(rest: &[String]) -> bool {
    let parsed = parse_options(rest, &GIT_CONFIG_OPTIONS);
    let any_long = |names: &[&str]| names.iter().any(|name| parsed.has_long(name));
    let subcommand = parsed.operands.first().copied().filter(|word| {
        matches!(
            *word,
            "get" | "set" | "unset" | "list" | "edit" | "rename-section" | "remove-section"
        )
    });
    let writes = parsed.has_short(&['e'])
        || any_long(&[
            "--edit",
            "--rename-section",
            "--remove-section",
            "--add",
            "--append",
            "--replace-all",
            "--unset",
            "--unset-all",
        ])
        || matches!(
            subcommand,
            Some("set" | "unset" | "edit" | "rename-section" | "remove-section")
        );
    if writes {
        return false;
    }
    let read_mode = parsed.has_short(&['l'])
        || any_long(&[
            "--get",
            "--get-all",
            "--get-regexp",
            "--get-urlmatch",
            "--get-color",
            "--get-colorbool",
            "--list",
        ])
        || matches!(subcommand, Some("get" | "list"));
    read_mode || (subcommand.is_none() && parsed.operands.len() <= 1)
}

/// Guard `git update-ref` (ADR-0009). Two vectors: (1) a direct write to a
/// protected `refs/heads/*` — a local move outside the sanctioned path; (2) a
/// write to `refs/remotes/*/<protected>` — poisoning the remote-tracking ref
/// the reference-transaction plane consults as its sync oracle. Deletions of a
/// protected ref route to `delete_protected`.
fn check_update_ref(rest: &[String], ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    // One parse feeds every question here, so an option value is never read as
    // a flag: in `git update-ref -m --stdin <ref> <new>` the `--stdin` is the
    // reason text, not stdin mode.
    let parsed = parse_options(rest, &GIT_UPDATE_REF_OPTIONS);
    let deleting = parsed.has_short(&['d']) || parsed.has_long("--delete");
    // `--stdin` carries the refs out-of-band; block conservatively.
    if parsed.has_long("--stdin") {
        if policy.local_ref_protection.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                "`git update-ref --stdin` updates refs from an opaque stream that may touch protected branches".to_string(),
                crate::remedy::PROTECTED_BRANCH.remedy(),
            ));
        }
        return;
    }
    let Some(refname) = parsed.operands.first().copied() else {
        return;
    };

    // The fetched registry is what issue and CI verify against (R-10, R-11);
    // only a real, non-forced fetch may move it.
    if refname
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, branch)| branch == crate::ids::REGISTRY_BRANCH)
    {
        out.push(registry_violation(format!(
            "`git update-ref` writes the fetched registry `{refname}`; only a non-forced fetch moves it (R-11)"
        )));
        return;
    }

    if let Some(branch) = protected_component_of_remote_ref(refname, policy) {
        if policy.local_ref_protection.is_active() {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                format!("`git update-ref` writes the remote-tracking ref for protected branch '{branch}' — the reference-transaction sync oracle must not be agent-set"),
                crate::remedy::REMOTE_TRACKING_REF.remedy(),
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
                    crate::remedy::PROTECTED_DELETE.remedy(),
                ));
            }
        } else if policy.local_ref_protection.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.local_ref_protection",
                policy.local_ref_protection,
                format!("`git update-ref` moves protected branch '{branch}' outside the sanctioned path"),
                crate::remedy::PROTECTED_BRANCH.remedy(),
            ));
        }
    }
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
                crate::remedy::PROTECTED_BRANCH.remedy(),
            ));
        }
    }
}

/// The registry rule (SPC-013 R-8): `codeflow/registry` only grows. It
/// always blocks, whatever the policy levels say.
fn registry_violation(message: String) -> Violation {
    Violation::always_blocking(
        "registry.append_only",
        message,
        "issue ids with `codeflow task|epic|spec new`; a maintainer repairs damage with `codeflow ids restore <id>...`",
    )
}

/// Refuse a push that could remove or rewrite registry history: a delete, a
/// force (including `--mirror` and a forced wildcard or `--all`), or a push
/// that skips the pre-push range check with `--no-verify`.
fn check_registry_push(push: &PushIntent, rest: &[String], out: &mut Vec<Violation>) {
    let registry = crate::ids::REGISTRY_BRANCH;
    let named = |list: &[String]| list.iter().any(|target| target == registry);
    if named(&push.deletions) || (push.mirror && push.touches_all_branches) {
        out.push(registry_violation(format!(
            "`git push` could delete `{registry}`; the registry only grows (R-8)"
        )));
    }
    let reaches = named(&push.updates) || push.touches_all_branches;
    if reaches && push.force {
        out.push(registry_violation(format!(
            "force push reaches `{registry}`; the registry only grows (R-8)"
        )));
    }
    if named(&push.updates) && has_no_verify("push", rest) {
        out.push(registry_violation(format!(
            "`--no-verify` skips the pre-push range check of `{registry}` (R-8)"
        )));
    }
}

fn check_push(rest: &[String], branch: &str, ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    let push = parse_push(rest, branch);
    check_registry_push(&push, rest, out);

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
                    crate::remedy::PROTECTED_BRANCH.remedy(),
                ));
            } else if policy.push_to_protected.is_active() && !ctx.integrate_token {
                out.push(Violation::new(
                    "git.push_to_protected",
                    policy.push_to_protected,
                    format!(
                        "bulk push (--all/--mirror/wildcard) reaches protected branches: {names}"
                    ),
                    crate::remedy::PROTECTED_BRANCH.remedy(),
                ));
            }
            if push.mirror && policy.delete_protected.is_active() {
                out.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("`git push --mirror` can delete protected branches to mirror local: {names}"),
                    crate::remedy::PROTECTED_DELETE.remedy(),
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
                crate::remedy::PROTECTED_DELETE.remedy(),
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
                        crate::remedy::PROTECTED_BRANCH.remedy(),
                    ));
                }
            } else if policy.force_push_unprotected.is_active() {
                // Default policy is allow (D8): rebasing feature branches is
                // normal. Only flips on if the user tightens the policy.
                out.push(Violation::new(
                    "git.force_push_unprotected",
                    policy.force_push_unprotected,
                    format!("force-push to branch '{target}'"),
                    crate::remedy::FORCE_PUSH.remedy(),
                ));
            }
        } else if protected && policy.push_to_protected.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!("direct push to protected branch '{target}'"),
                crate::remedy::PROTECTED_BRANCH.remedy(),
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
        // `gh pr edit` takes the same body flags, so an edited body is
        // scanned like a new one (ADR-0067).
        Some(&"create" | &"edit") => check_gh_pr_body(&plain[2..], ctx.policy, out),
        Some(&"merge") => check_gh_pr_merge(&plain[2..], ctx, out),
        _ => {}
    }
}

/// Scan a `gh pr create` or `gh pr edit` body for AI attribution / emoji
/// (charter §6.4) and policy characters (ADR-0067).
///
/// Both the inline `--body`/`-b` value and the content of a `--body-file`/`-F`
/// file are scanned. Fail-open (matching the guard's doctrine): a missing or
/// unreadable body file passes rather than blocking. A stdin body (`-F -`) is
/// out of scope — its content is not available to the guard, so it is not read.
fn check_gh_pr_body(rest: &[&str], policy: &GitPolicy, out: &mut Vec<Violation>) {
    let inline = flag_value(rest, &["--body", "-b"]);
    let from_file = flag_value(rest, &["--body-file", "-F"])
        .filter(|path| *path != "-")
        .and_then(|path| std::fs::read_to_string(path).ok());
    for body in inline.into_iter().chain(from_file.as_deref()) {
        scan_pr_body(body, policy, out);
    }
}

/// Flag AI-attribution, emoji and policy-character violations in a single
/// PR-body string.
fn scan_pr_body(body: &str, policy: &GitPolicy, out: &mut Vec<Violation>) {
    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(body) {
            out.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("PR body contains AI attribution ({which})"),
                crate::remedy::PR_AI_ATTRIBUTION.remedy(),
            ));
        }
    }
    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(body) {
            out.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("PR body contains emoji ('{c}')"),
                crate::remedy::PR_EMOJI.remedy(),
            ));
        }
    }
    if let Some(v) = standards::pr_body_policy_character(policy, body) {
        out.push(v);
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
    if gh_merge_deletes_branch(rest) {
        out.push(Violation::always_blocking(
            "git.pr_merge_delete_branch",
            "`gh pr merge --delete-branch` can flip the root repo to core.bare when the merged branch is checked out in a worktree".to_string(),
            "merge plain (no --delete-branch), then delete the branch from the repo root separately (`git branch -d <branch>` / `git push origin --delete <branch>`)",
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
            "`gh pr merge` into a base that is protected or could not be confirmed unprotected"
                .to_string(),
            crate::remedy::PR_MERGE_PROTECTED.remedy(),
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
/// How an option consumes its value.
#[derive(Clone, Copy)]
enum Arity {
    /// A boolean flag, taking nothing.
    Flag,
    /// A value is required: attached (`-mmsg`, `--message=msg`) or the next
    /// token, whatever that token looks like.
    Value,
    /// A value only when attached (`-uno`, `-i.bak`, `--color=always`); a
    /// separate token is an operand, not the value.
    AttachedValue,
}

/// Git's own end-of-options terminator (parse-options, Git 2.24), accepted
/// wherever a bare `--` is and with the same effect.
const END_OF_OPTIONS: &str = "--end-of-options";

/// One command's option grammar. The long list is the single source for both
/// arity and abbreviation, so a new option is added in one place. Anything not
/// listed is a boolean flag, which is the safe default: an unlisted option
/// never swallows an operand.
struct OptionSpec {
    short: &'static [(char, Arity)],
    long: &'static [(&'static str, Arity)],
    /// The command parses with Git's parse-options: it accepts any unambiguous
    /// prefix of a long option and `--end-of-options` as a terminator. `sed`
    /// and the `gh` commands (pflag) do neither: for them only the written
    /// long name counts.
    git_style: bool,
}

impl OptionSpec {
    fn short_arity(&self, letter: char) -> Arity {
        self.short
            .iter()
            .find(|(candidate, _)| *candidate == letter)
            .map_or(Arity::Flag, |(_, arity)| *arity)
    }

    /// The canonical long option a written name stands for: itself when known,
    /// otherwise, for a Git command, the one known option it is an unambiguous
    /// prefix of. An ambiguous or unknown prefix resolves to nothing, which
    /// reads as a boolean flag; Git rejects an ambiguous prefix outright.
    fn resolve_long(&self, written: &str) -> Option<&'static str> {
        if let Some((exact, _)) = self.long.iter().find(|(name, _)| *name == written) {
            return Some(exact);
        }
        if !self.git_style {
            return None;
        }
        let mut candidates = self
            .long
            .iter()
            .filter(|(name, _)| name.starts_with(written));
        let (first, _) = candidates.next()?;
        candidates.next().is_none().then_some(*first)
    }

    fn long_arity(&self, name: &str) -> Arity {
        self.long
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map_or(Arity::Flag, |(_, arity)| *arity)
    }
}

/// `git branch` (git-branch(1)). `-u`/`--set-upstream-to` name an upstream and
/// the filter, sort and format options take a value; `-t`/`--track` and the
/// display options take one only when attached. `-m`, `-M`, `-c` and `-C` are
/// rename and copy actions whose branch names are operands, not values.
const GIT_BRANCH_OPTIONS: OptionSpec = OptionSpec {
    short: &[('u', Arity::Value), ('t', Arity::AttachedValue)],
    long: &[
        ("--set-upstream-to", Arity::Value),
        ("--contains", Arity::Value),
        ("--no-contains", Arity::Value),
        ("--merged", Arity::Value),
        ("--no-merged", Arity::Value),
        ("--points-at", Arity::Value),
        ("--sort", Arity::Value),
        ("--format", Arity::Value),
        ("--track", Arity::AttachedValue),
        ("--color", Arity::AttachedValue),
        ("--abbrev", Arity::AttachedValue),
        ("--column", Arity::AttachedValue),
        ("--delete", Arity::Flag),
        ("--force", Arity::Flag),
        ("--move", Arity::Flag),
        ("--copy", Arity::Flag),
        ("--list", Arity::Flag),
        ("--all", Arity::Flag),
        ("--remotes", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--create-reflog", Arity::Flag),
        ("--unset-upstream", Arity::Flag),
        ("--edit-description", Arity::Flag),
        ("--ignore-case", Arity::Flag),
        ("--show-current", Arity::Flag),
        ("--omit-empty", Arity::Flag),
        ("--recurse-submodules", Arity::Flag),
        ("--no-color", Arity::Flag),
        ("--no-column", Arity::Flag),
        ("--no-abbrev", Arity::Flag),
        ("--no-track", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git commit` (git-commit(1)). `-m`/`--message`, `-F`, `-c`, `-C`, `-t` and
/// the metadata options take a value, so their content is never read as
/// options: `git commit --message -n` is a message, not a hook bypass.
/// `-u`/`--untracked-files` and `-S`/`--gpg-sign` take an attached mode only.
const GIT_COMMIT_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('m', Arity::Value),
        ('F', Arity::Value),
        ('c', Arity::Value),
        ('C', Arity::Value),
        ('t', Arity::Value),
        ('u', Arity::AttachedValue),
        ('S', Arity::AttachedValue),
    ],
    long: &[
        ("--message", Arity::Value),
        ("--file", Arity::Value),
        ("--reedit-message", Arity::Value),
        ("--reuse-message", Arity::Value),
        ("--template", Arity::Value),
        ("--fixup", Arity::Value),
        ("--squash", Arity::Value),
        ("--author", Arity::Value),
        ("--date", Arity::Value),
        ("--cleanup", Arity::Value),
        ("--trailer", Arity::Value),
        ("--pathspec-from-file", Arity::Value),
        ("--untracked-files", Arity::AttachedValue),
        ("--gpg-sign", Arity::AttachedValue),
        ("--no-verify", Arity::Flag),
        ("--verify", Arity::Flag),
        ("--all", Arity::Flag),
        ("--patch", Arity::Flag),
        ("--amend", Arity::Flag),
        ("--allow-empty", Arity::Flag),
        ("--allow-empty-message", Arity::Flag),
        ("--reset-author", Arity::Flag),
        ("--short", Arity::Flag),
        ("--branch", Arity::Flag),
        ("--no-branch", Arity::Flag),
        ("--porcelain", Arity::Flag),
        ("--long", Arity::Flag),
        ("--null", Arity::Flag),
        ("--signoff", Arity::Flag),
        ("--no-signoff", Arity::Flag),
        ("--edit", Arity::Flag),
        ("--no-edit", Arity::Flag),
        ("--no-post-rewrite", Arity::Flag),
        ("--include", Arity::Flag),
        ("--only", Arity::Flag),
        ("--pathspec-file-nul", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--status", Arity::Flag),
        ("--no-status", Arity::Flag),
        ("--no-gpg-sign", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git config` (git-config(1)): the legacy mode options and those of the
/// `get`/`set`/`unset`/`list`/`edit` subcommands. The file, blob, type,
/// default, comment and pattern options take a value, so it is never read as
/// a key; the mode options are what [`config_writes_hooks_path`] reads.
const GIT_CONFIG_OPTIONS: OptionSpec = OptionSpec {
    short: &[('f', Arity::Value)],
    long: &[
        ("--file", Arity::Value),
        ("--blob", Arity::Value),
        ("--type", Arity::Value),
        ("--default", Arity::Value),
        ("--comment", Arity::Value),
        ("--value", Arity::Value),
        ("--url", Arity::Value),
        ("--get", Arity::Flag),
        ("--get-all", Arity::Flag),
        ("--get-regexp", Arity::Flag),
        ("--get-urlmatch", Arity::Flag),
        ("--get-color", Arity::Flag),
        ("--get-colorbool", Arity::Flag),
        ("--list", Arity::Flag),
        ("--add", Arity::Flag),
        ("--append", Arity::Flag),
        ("--replace-all", Arity::Flag),
        ("--unset", Arity::Flag),
        ("--unset-all", Arity::Flag),
        ("--rename-section", Arity::Flag),
        ("--remove-section", Arity::Flag),
        ("--edit", Arity::Flag),
        ("--global", Arity::Flag),
        ("--system", Arity::Flag),
        ("--local", Arity::Flag),
        ("--worktree", Arity::Flag),
        ("--all", Arity::Flag),
        ("--regexp", Arity::Flag),
        ("--fixed-value", Arity::Flag),
        ("--bool", Arity::Flag),
        ("--int", Arity::Flag),
        ("--bool-or-int", Arity::Flag),
        ("--path", Arity::Flag),
        ("--expiry-date", Arity::Flag),
        ("--no-type", Arity::Flag),
        ("--null", Arity::Flag),
        ("--name-only", Arity::Flag),
        ("--show-origin", Arity::Flag),
        ("--show-scope", Arity::Flag),
        ("--includes", Arity::Flag),
        ("--no-includes", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git update-ref` (git-update-ref(1)): only `-m <reason>` takes a value.
const GIT_UPDATE_REF_OPTIONS: OptionSpec = OptionSpec {
    short: &[('m', Arity::Value)],
    long: &[
        ("--stdin", Arity::Flag),
        ("--no-deref", Arity::Flag),
        ("--create-reflog", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

pub(super) fn ref_plumbing_tampers(sub: &str, args: &[String]) -> bool {
    let parsed = parse_options(args, &GIT_UPDATE_REF_OPTIONS);
    let writing = sub == "update-ref"
        || parsed.has_short(&['d'])
        || args.iter().any(|arg| arg == "--delete")
        || parsed.operands.len() >= 2;
    writing
        && (parsed.has_long("--stdin")
            || parsed
                .operands
                .iter()
                .any(|name| name.starts_with("refs/remotes/")))
}

/// `git reset` (git-reset(1)): the mode options are booleans, and only the
/// pathspec sources take a value.
const GIT_RESET_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[
        ("--hard", Arity::Flag),
        ("--soft", Arity::Flag),
        ("--mixed", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--keep", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--no-refresh", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--pathspec-from-file", Arity::Value),
        ("--pathspec-file-nul", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git rebase` (git-rebase(1)). The options that take a value, so none of
/// them is read as the `<upstream>` or `<branch>` operand.
const GIT_REBASE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('s', Arity::Value),
        ('X', Arity::Value),
        ('x', Arity::Value),
        ('C', Arity::Value),
        ('S', Arity::AttachedValue),
        ('r', Arity::Flag),
    ],
    long: &[
        ("--onto", Arity::Value),
        ("--strategy", Arity::Value),
        ("--strategy-option", Arity::Value),
        ("--exec", Arity::Value),
        ("--whitespace", Arity::Value),
        ("--empty", Arity::Value),
        ("--trailer", Arity::Value),
        ("--gpg-sign", Arity::AttachedValue),
        ("--rebase-merges", Arity::AttachedValue),
        ("--root", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// The `<branch>` a `git rebase` checks out and rewrites, when one is given:
/// the second operand, or the first with `--root`.
fn rebase_branch(rest: &[String]) -> Option<&str> {
    let parsed = parse_options(rest, &GIT_REBASE_OPTIONS);
    let at = usize::from(!parsed.has_long("--root"));
    parsed.operands.get(at).copied()
}

/// `git push` (git-push(1)). The destructive options are what the guard reads,
/// so they resolve through the same abbreviation rule as every other command.
const GIT_PUSH_OPTIONS: OptionSpec = OptionSpec {
    short: &[('o', Arity::Value)],
    long: &[
        ("--force", Arity::Flag),
        ("--force-with-lease", Arity::AttachedValue),
        ("--force-if-includes", Arity::Flag),
        ("--no-force-if-includes", Arity::Flag),
        ("--delete", Arity::Flag),
        ("--mirror", Arity::Flag),
        ("--all", Arity::Flag),
        ("--branches", Arity::Flag),
        ("--tags", Arity::Flag),
        ("--follow-tags", Arity::Flag),
        ("--prune", Arity::Flag),
        ("--atomic", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--porcelain", Arity::Flag),
        ("--set-upstream", Arity::Flag),
        ("--no-verify", Arity::Flag),
        ("--verify", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--thin", Arity::Flag),
        ("--no-thin", Arity::Flag),
        ("--ipv4", Arity::Flag),
        ("--ipv6", Arity::Flag),
        ("--signed", Arity::AttachedValue),
        ("--no-signed", Arity::Flag),
        ("--repo", Arity::Value),
        ("--receive-pack", Arity::Value),
        ("--exec", Arity::Value),
        ("--push-option", Arity::Value),
        ("--recurse-submodules", Arity::Value),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `gh pr merge` (pflag). No prefix abbreviation, so `--del` is not
/// `--delete-branch`; boolean shorthands cluster (`-ds`, `-sd`), and a boolean
/// long option takes an attached `=true`/`=false` without consuming the next
/// token. Only the flags that carry a value are listed as such.
const GH_PR_MERGE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('b', Arity::Value),
        ('F', Arity::Value),
        ('t', Arity::Value),
    ],
    long: &[
        ("--body", Arity::Value),
        ("--body-file", Arity::Value),
        ("--subject", Arity::Value),
        ("--match-head-commit", Arity::Value),
        ("--author-email", Arity::Value),
        ("--delete-branch", Arity::Flag),
        ("--admin", Arity::Flag),
        ("--auto", Arity::Flag),
        ("--disable-auto", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--rebase", Arity::Flag),
        ("--squash", Arity::Flag),
    ],
    git_style: false,
};

/// `sed`: `-e` a script, `-f` a script file, `-l` a line length; `-i` takes
/// the GNU backup suffix only when attached (`-i.bak`), so a separate token
/// stays an operand.
const SED_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('e', Arity::Value),
        ('f', Arity::Value),
        ('l', Arity::Value),
        ('i', Arity::AttachedValue),
    ],
    long: &[
        ("--expression", Arity::Value),
        ("--file", Arity::Value),
        ("--line-length", Arity::Value),
        ("--in-place", Arity::AttachedValue),
    ],
    git_style: false,
};

/// For commands where only the presence of a long flag matters and no option
/// arity is modelled.
const PLAIN_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[],
    git_style: false,
};

/// One option as it was met on the command line, in encounter order.
#[derive(Clone, Copy)]
enum Seen<'a> {
    Short(char),
    Long(&'a str, Option<&'a str>),
}

/// The options and operands of one command line. A long option keeps the
/// value written onto it, so `--delete-branch=false` is distinguishable from
/// `--delete-branch`; `sequence` keeps every option in the order it was
/// written, for flags whose last assignment wins.
struct ParsedOptions<'a> {
    short: Vec<char>,
    long: Vec<(&'a str, Option<&'a str>)>,
    sequence: Vec<Seen<'a>>,
    operands: Vec<&'a str>,
}

impl ParsedOptions<'_> {
    fn has_short(&self, letters: &[char]) -> bool {
        self.short.iter().any(|letter| letters.contains(letter))
    }

    fn has_long(&self, name: &str) -> bool {
        self.long.iter().any(|(written, _)| *written == name)
    }
}

/// Walk the arguments once, in order, the way the command's own parser does.
/// An option that requires a value takes it first, attached or from the next
/// token, before anything later is interpreted, so a value that happens to
/// read as `--` or `--no-verify` is the value it is. Only a `--` reached as an
/// option ends the options; every token after it is an operand, including one
/// that begins with `-`.
fn parse_options<'a, S: AsRef<str>>(args: &'a [S], spec: &OptionSpec) -> ParsedOptions<'a> {
    let mut parsed = ParsedOptions {
        short: Vec::new(),
        long: Vec::new(),
        sequence: Vec::new(),
        operands: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let token = args[index].as_ref();
        index += 1;
        if let Some(body) = token.strip_prefix("--") {
            if body.is_empty() {
                parsed
                    .operands
                    .extend(args[index..].iter().map(AsRef::as_ref));
                return parsed;
            }
            let (name, attached) = match body.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (body, None),
            };
            let written = &token[..name.len() + 2];
            let canonical = spec.resolve_long(written);
            if canonical == Some(END_OF_OPTIONS) {
                parsed
                    .operands
                    .extend(args[index..].iter().map(AsRef::as_ref));
                return parsed;
            }
            let name = canonical.unwrap_or(written);
            parsed.long.push((name, attached));
            parsed.sequence.push(Seen::Long(name, attached));
            if attached.is_none()
                && canonical.is_some_and(|n| matches!(spec.long_arity(n), Arity::Value))
            {
                index += 1;
            }
            continue;
        }
        let Some(cluster) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            parsed.operands.push(token);
            continue;
        };
        for (offset, letter) in cluster.char_indices() {
            parsed.short.push(letter);
            parsed.sequence.push(Seen::Short(letter));
            match spec.short_arity(letter) {
                Arity::Flag => {}
                Arity::AttachedValue => break,
                Arity::Value => {
                    if cluster[offset + letter.len_utf8()..].is_empty() {
                        index += 1;
                    }
                    break;
                }
            }
        }
    }
    parsed
}

/// Does this `git branch` invocation ask to delete a branch? The delete flag
/// is recognized standalone, inside a cluster, and as the long form.
fn requests_branch_delete(args: &[String]) -> bool {
    let parsed = parse_options(args, &GIT_BRANCH_OPTIONS);
    parsed.has_short(&['d', 'D']) || parsed.has_long("--delete")
}

/// The branch names a `git branch` invocation operates on: the operands, with
/// option values consumed and everything after `--` kept, however it is
/// spelled.
fn branch_operands(args: &[String]) -> Vec<&str> {
    parse_options(args, &GIT_BRANCH_OPTIONS).operands
}

/// Does this `gh pr merge` invocation ask to delete the merged branch? The
/// flag is read as `-d` anywhere, including inside a cluster of boolean
/// shorthands (`-ds`, `-sd`), as the written long name, and with an attached
/// `=<bool>`. pflag assigns the flag each time it is written, so the last
/// assignment in argument order wins: `--delete-branch=false -sd` deletes and
/// `-sd --delete-branch=false` does not. A value that is neither true nor
/// false reads as a request, which fails toward the block.
fn gh_merge_deletes_branch(args: &[&str]) -> bool {
    parse_options(args, &GH_PR_MERGE_OPTIONS)
        .sequence
        .iter()
        .fold(false, |current, seen| match seen {
            Seen::Short('d') | Seen::Long("--delete-branch", None) => true,
            Seen::Long("--delete-branch", Some(value)) => {
                !matches!(value.to_ascii_lowercase().as_str(), "0" | "f" | "false")
            }
            _ => current,
        })
}

/// Does this `sed` invocation edit its input in place?
fn requests_in_place(args: &[String]) -> bool {
    let parsed = parse_options(args, &SED_OPTIONS);
    parsed.has_short(&['i']) || parsed.has_long("--in-place")
}

fn has_no_verify(sub: &str, args: &[String]) -> bool {
    let commit = sub == "commit";
    let parsed = parse_options(
        args,
        if commit {
            &GIT_COMMIT_OPTIONS
        } else {
            &PLAIN_OPTIONS
        },
    );
    parsed.has_long("--no-verify") || (commit && parsed.has_short(&['n']))
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
    Violation::always_blocking(
        "git.no_verify_bypass",
        format!("`git {sub} --no-verify` skips the client hooks on protected branch '{branch}'"),
        "do not bypass the hooks with --no-verify — fix the cause the gate flags, or land via the sanctioned path (PR / `codeflow integrate`)",
    )
}

// ---------------------------------------------------------------------------
// command parsing helpers
// ---------------------------------------------------------------------------

/// Tokenize a shell command segment, honoring single/double quotes and
/// backslash escapes (outside single quotes).
#[must_use]
pub fn shell_tokens(segment: &str) -> Vec<String> {
    shell_words(segment).into_iter().map(|w| w.text).collect()
}

/// One shell word after quote removal, with the length of its leading run of
/// unquoted, unescaped characters: a redirection operator only counts when it
/// was written there (`2>/dev/null` redirects, `'2>/dev/null'` is text).
struct ShellWord {
    text: String,
    unquoted_prefix: usize,
}

fn shell_words(segment: &str) -> Vec<ShellWord> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut started = false;
    let mut prefix_open = true;
    let mut unquoted_prefix = 0;
    let mut chars = segment.chars();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                started = true;
                prefix_open = false;
            }
            '"' if !in_single => {
                in_double = !in_double;
                started = true;
                prefix_open = false;
            }
            '\\' if !in_single => {
                // A backslash-newline is a line continuation: both vanish.
                if let Some(next) = chars.next().filter(|next| *next != '\n') {
                    cur.push(next);
                    started = true;
                    prefix_open = false;
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if started {
                    words.push(ShellWord {
                        text: std::mem::take(&mut cur),
                        unquoted_prefix,
                    });
                    started = false;
                }
                prefix_open = true;
                unquoted_prefix = 0;
            }
            c => {
                cur.push(c);
                started = true;
                if prefix_open && !in_single && !in_double {
                    unquoted_prefix += c.len_utf8();
                }
            }
        }
    }
    if started {
        words.push(ShellWord {
            text: cur,
            unquoted_prefix,
        });
    }
    words
}

/// The argument vector the program receives: the segment's words without its
/// unquoted redirections (`2>&1`, `> out`, `<<EOF`, `<<< text`), which the
/// shell removes before the program runs. The integrity checks keep reading
/// the full token list, where a redirect is the evidence.
pub(crate) fn command_argv(segment: &str) -> Vec<String> {
    let words = shell_words(segment);
    let mut argv = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let word = &words[i];
        i += 1;
        match redirect_operator_len(&word.text) {
            Some(len) if len <= word.unquoted_prefix => {
                if len == word.text.len() {
                    i += 1; // a bare operator: its target is the next word
                }
            }
            _ => argv.push(word.text.clone()),
        }
    }
    argv
}

/// Byte length of the redirection operator a word starts with: an optional
/// fd number, then `>`, `>>`, `>|`, `>&`, `<`, `<<`, `<<-`, `<<<`, `<>` or
/// `<&`; or a leading `&>`/`&>>`. `None` when the word is not a redirection.
fn redirect_operator_len(word: &str) -> Option<usize> {
    if let Some(rest) = word.strip_prefix("&>") {
        return Some(if rest.starts_with('>') { 3 } else { 2 });
    }
    let fd = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    ["<<<", "<<-", ">>", ">|", ">&", "<<", "<>", "<&", ">", "<"]
        .iter()
        .find(|op| word[fd..].starts_with(**op))
        .map(|op| fd + op.len())
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
pub(crate) fn strip_launchers(tokens: &[String]) -> Option<(&str, &[String])> {
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
pub(super) fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
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

/// The branch a `git checkout`/`git switch` lands on, when determinable. A
/// branch followed by paths (`git checkout feat/x -- f`) restores files and
/// stays on the current branch.
fn checkout_target(rest: &[String]) -> Option<String> {
    let mut iter = rest.iter();
    while let Some(t) = iter.next() {
        match t.as_str() {
            "-b" | "-B" | "-c" | "-C" => return iter.next().cloned(),
            "--detach" | "--" => return None,
            s if s.starts_with('-') => {}
            s => {
                let paths_follow = iter.any(|t| t == "--" || !t.starts_with('-'));
                return (!paths_follow).then(|| s.to_string());
            }
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

fn parse_push(rest: &[String], current_branch: &str) -> PushIntent {
    let mut force = false;
    let mut delete_mode = false;
    let mut all = false;
    let mut mirror = false;
    let mut positional: Vec<&str> = Vec::new();

    // Read the options the way git does, so an abbreviated `--forc` or
    // `--del` is the option it stands for and a value is never read as a flag.
    let parsed = parse_options(rest, &GIT_PUSH_OPTIONS);
    for (name, _) in &parsed.long {
        match *name {
            "--force" | "--force-with-lease" => force = true,
            "--delete" => delete_mode = true,
            "--all" | "--branches" => all = true,
            "--mirror" => {
                mirror = true;
                force = true; // a mirror force-updates refs to match local
            }
            _ => {}
        }
    }
    if parsed.has_short(&['f']) {
        force = true;
    }
    if parsed.has_short(&['d']) {
        delete_mode = true;
    }
    positional.extend(parsed.operands.iter().copied());

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
mod registry_guard_tests {
    use super::*;

    fn verdict(command: &str) -> Vec<Violation> {
        let policy = GitPolicy::default();
        let ctx = GuardContext {
            policy: &policy,
            current_branch: "task/TSK-101-id-registry",
            integrate_token: false,
            pr_base_lookup: None,
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
        };
        evaluate(command, &ctx)
            .into_iter()
            .filter(|v| v.rule == "registry.append_only")
            .collect()
    }

    #[test]
    fn registry_deletion_force_and_no_verify_are_refused() {
        for command in [
            "git push origin --delete codeflow/registry",
            "git push origin :codeflow/registry",
            "git push origin :refs/heads/codeflow/registry",
            "git push --force origin codeflow/registry",
            "git push origin +codeflow/registry",
            "git push -f origin abc123:refs/heads/codeflow/registry",
            "git push --force-with-lease origin codeflow/registry",
            "git push --mirror origin",
            "git push --force --all origin",
            "git push --no-verify origin codeflow/registry",
            "git update-ref refs/remotes/origin/codeflow/registry abc123",
            "git update-ref -d refs/remotes/origin/codeflow/registry",
        ] {
            let found = verdict(command);
            assert!(!found.is_empty(), "{command} must be refused");
            assert!(
                found.iter().all(|v| v.level == PolicyLevel::Block),
                "{command}"
            );
        }
    }

    #[test]
    fn plain_registry_pushes_and_other_branches_pass() {
        for command in [
            "git push origin codeflow/registry",
            "git push origin abc123:refs/heads/codeflow/registry",
            "git push --force-with-lease origin task/TSK-101-id-registry",
            "git fetch origin refs/heads/codeflow/registry:refs/remotes/origin/codeflow/registry",
            "git update-ref refs/heads/codeflow/registry abc123",
        ] {
            assert!(
                verdict(command).is_empty(),
                "{command} must pass the registry rule"
            );
        }
    }
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
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
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
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
        }
    }

    /// A guard context with an injected retarget-dir → branch resolver.
    fn ctx_with_dir_branch<'a>(
        policy: &'a GitPolicy,
        branch: &'a str,
        resolver: &'a dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
    ) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
            pr_base_lookup: None,
            dir_target_lookup: Some(resolver),
            alias_lookup: Some(&fixture_alias),
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
        }
    }

    fn default_policy() -> GitPolicy {
        GitPolicy::default()
    }

    /// A resolver answer for a directory inside the session repository.
    #[allow(clippy::unnecessary_wraps)] // the resolver's return type
    fn same_repo(branch: &str) -> Option<TargetRepo> {
        Some(TargetRepo {
            branch: branch.to_string(),
            policy: None,
            root: None,
        })
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
    fn test_payload_parse_grok_camelcase_shell() {
        let json = r#"{
            "hookEventName": "pre_tool_use",
            "toolName": "run_terminal_command",
            "toolInput": {"command": "git status"},
            "cwd": "/repo"
        }"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), Some("git status"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new("/repo")));
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

    /// TSK-147 round 3 F5: input that is not a JSON object is a harness
    /// entry that does not pass the payload through, which a local edit
    /// clears. Round 4: so is a JSON object whose known field has the wrong
    /// type. The payload struct ignores unknown fields, so a wrong type on a
    /// field it reads is the only way an object fails, and nothing in it
    /// shows a newer harness schema rather than a mangled payload.
    #[test]
    fn a_payload_that_is_not_a_json_object_is_malformed() {
        for input in ["", "not json", "{ nope", "[]", "\"x\"", "42"] {
            assert!(
                matches!(HookPayload::parse(input), Err(PayloadError::Malformed(_))),
                "{input:?}"
            );
        }
    }

    #[test]
    fn a_known_field_with_the_wrong_type_is_malformed_and_named() {
        for (input, field) in [
            (r#"{"tool_name": 5}"#, "tool_name"),
            (r#"{"toolName": 5}"#, "tool_name"),
            (r#"{"tool_input": "git status"}"#, "tool_input"),
            (
                r#"{"tool_input": {"command": ["git"]}}"#,
                "tool_input.command",
            ),
            (r#"{"cwd": 7}"#, "cwd"),
        ] {
            let Err(PayloadError::Malformed(why)) = HookPayload::parse(input) else {
                panic!("{input:?} is not malformed");
            };
            assert!(why.contains(&format!("`{field}`")), "{input:?}: {why}");
        }
        // Fields this build does not read are ignored, not refused.
        assert!(HookPayload::parse(r#"{"tool_name":"Bash","extra":[1]}"#).is_ok());
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
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
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
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
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

    /// Git accepts the delete flag inside a short option cluster, so the guard
    /// reads the cluster's letters rather than the whole token.
    #[test]
    fn test_delete_protected_local_branch_blocked_in_a_short_cluster() {
        let p = default_policy();
        for cmd in [
            "git branch -Dq main",
            "git branch -qd main",
            "git branch -dq main",
            "git branch -Df main",
            "git branch --delete -q main",
            "git branch -q -d main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.delete_protected"), "{cmd}: {v:?}");
        }
    }

    /// Harmless `git branch` options aimed at a PROTECTED branch: if the
    /// cluster reader mistook an attached option value for flags, these would
    /// raise a delete violation. An unprotected target could not show that.
    #[test]
    fn test_harmless_branch_options_on_a_protected_branch_are_not_deletes() {
        let p = default_policy();
        for cmd in [
            "git branch -v main",
            "git branch -uorigin/dev main",
            "git branch -u origin/dev main",
            "git branch --set-upstream-to=origin/dev main",
            "git branch -m main main2",
            "git branch --contains main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(!has_rule(&v, "git.delete_protected"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_clustered_delete_of_an_unprotected_branch_is_allowed() {
        let p = default_policy();
        for cmd in [
            "git branch -Dq topic",
            "git branch -qd topic",
            "git branch -v",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn test_update_ref_clustered_delete_of_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git update-ref -dz refs/heads/main", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.delete_protected"), "{v:?}");
    }

    #[test]
    fn test_no_verify_inside_a_short_cluster_blocked() {
        let p = default_policy();
        for cmd in [
            "git commit -an -m wip",
            "git commit -anm wip",
            "git commit -n -m wip",
            "git commit -n",
            "git commit --no-verify",
        ] {
            let v = evaluate(cmd, &ctx(&p, "main"));
            assert!(has_rule(&v, "git.no_verify_bypass"), "{cmd}: {v:?}");
        }
    }

    /// Commit options that merely carry a value, on a PROTECTED branch where
    /// the no-verify rule can fire. Each must still raise the protected-commit
    /// rule, so the command is reaching the guard, and must not raise the
    /// bypass rule from an `n` inside an option value.
    #[test]
    fn test_commit_option_values_are_not_read_as_no_verify() {
        let p = default_policy();
        for cmd in [
            "git commit -mone",
            "git commit -m one -a",
            "git commit -am wip",
            "git commit -C HEAD",
            "git commit -uno",
            "git commit -tplan.txt",
            "git commit --message -n",
            "git commit --untracked-files=no -m wip",
        ] {
            let v = evaluate(cmd, &ctx(&p, "main"));
            assert!(has_rule(&v, "git.commit_to_protected"), "{cmd}: {v:?}");
            assert!(!has_rule(&v, "git.no_verify_bypass"), "{cmd}: {v:?}");
        }
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
    fn test_pr_body_policy_character_reported() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'Adds a hook \u{2014} and a test.'";
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.policy_characters");
        // The shipped default warns; a project that sets block is refused.
        assert_eq!(v[0].level, PolicyLevel::Warn);
        assert!(
            v[0].message.contains("em dash (U+2014)"),
            "{}",
            v[0].message
        );
    }

    // Grok review of the warn default, D2: a project that sets block (as
    // this repository does) still refuses a dashed PR body.
    #[test]
    fn test_pr_body_policy_character_blocked_when_policy_blocks() {
        let p = GitPolicy {
            policy_characters: PolicyLevel::Block,
            ..default_policy()
        };
        for cmd in [
            "gh pr create --title 'feat: x' --body 'Adds a hook \u{2014} and a test.'",
            "gh pr edit 12 --body 'Pages 1\u{2013}3.'",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert_eq!(v.len(), 1, "{cmd}");
            assert_eq!(v[0].rule, "git.policy_characters");
            assert_eq!(v[0].level, PolicyLevel::Block, "{cmd}");
        }
    }

    // Codex EPC-017 review, finding 5: an edited body is scanned too.
    #[test]
    fn test_pr_edit_body_policy_character_reported() {
        let p = default_policy();
        let inline = "gh pr edit 12 --body 'Adds a hook \u{2014} and a test.'";
        let v = evaluate(inline, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.policy_characters");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body.md");
        std::fs::write(&path, "Summary: pages 1\u{2013}3.").unwrap();
        let from_file = format!("gh pr edit -F '{}'", path.display());
        let v = evaluate(&from_file, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.policy_characters");
        let clean = "gh pr edit 12 --title 'feat: x' --body 'Summary: adds a test.'";
        assert!(evaluate(clean, &ctx(&p, "feat/x")).is_empty());
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
            policy_characters: PolicyLevel::Off,
            ..default_policy()
        };
        let cmd = "gh pr create --body 'Generated with Bot \u{1F916} \u{2013}'";
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

    /// `gh` parses with pflag: `-d` is the registered shorthand for
    /// `--delete-branch` and boolean shorthands cluster, so the guard reads the
    /// flag rather than the exact long spelling.
    #[test]
    fn test_gh_pr_merge_delete_branch_read_as_a_flag() {
        let p = default_policy();
        for cmd in [
            "gh pr merge 42 -d",
            "gh pr merge -d 42",
            "gh pr merge 42 --delete-branch",
            "gh pr merge 42 -ds",
            "gh pr merge 42 -sd",
            "gh pr merge 42 -dm",
            "gh pr merge 42 -rd",
            "gh pr merge 42 --delete-branch=true",
            "gh pr merge 42 --squash --delete-branch",
            "gh pr merge 42 --delete-branch=false -sd",
            "gh pr merge 42 --delete-branch=false -d",
            "gh pr merge 42 --delete-branch=false --delete-branch",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.pr_merge_delete_branch"), "{cmd}: {v:?}");
        }
    }

    /// pflag does not abbreviate, an explicit false is not a request, and a
    /// value carrying the letter is not the flag.
    #[test]
    fn test_gh_pr_merge_without_a_delete_request() {
        let p = default_policy();
        let lookup = |_: &str| Some("develop".to_string());
        for cmd in [
            "gh pr merge 42 --del",
            "gh pr merge 42 --delete-branch=false",
            "gh pr merge 42 -d --delete-branch=false",
            "gh pr merge 42 -sd --delete-branch=false",
            "gh pr merge 42 --squash",
            "gh pr merge 42 -s",
            "gh pr merge 42 -b done-deleting",
            "gh pr merge 42 --body done-deleting",
        ] {
            let v = evaluate(cmd, &ctx_with_lookup(&p, "feat/x", &lookup));
            assert!(!has_rule(&v, "git.pr_merge_delete_branch"), "{cmd}: {v:?}");
        }
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
        // Reading the value is harmless, whatever the mode, scope, source file
        // or display flags, and whatever the output is redirected to.
        let p = default_policy();
        for cmd in [
            "git config core.hooksPath",
            "git config core.hookspath",
            "git config --get core.hooksPath",
            "git config --get-all core.hooksPath",
            "git config --get-regexp hookspath",
            "git config -l",
            "git config --list --show-origin",
            "git config --show-origin core.hooksPath",
            "git config --show-scope --get core.hooksPath",
            "git config --local core.hooksPath",
            "git config --global --get core.hooksPath",
            "git config --worktree core.hooksPath",
            "git config --file .git/config core.hooksPath",
            "git config -f .git/config --get core.hooksPath",
            "git config --type=path core.hooksPath",
            "git config --type path core.hooksPath",
            "git config --default .githooks --get core.hooksPath",
            "git config get core.hooksPath",
            "git config get --show-origin core.hooksPath",
            "git config list",
            "git config core.hooksPath 2>/dev/null",
            "git config core.hooksPath 2>&1",
            "git config core.hooksPath > /tmp/hooks-path",
            "git config core.hooksPath || echo unset",
            "git -C /repo config core.hooksPath",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(!has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_config_hooks_path_writes_blocked() {
        // Every spelling that writes, unsets or can rewrite core.hooksPath, in
        // every scope, including a quoted value that only looks like a
        // redirect and an alias whose body rewrites the hook path.
        let p = default_policy();
        for cmd in [
            "git config core.hooksPath /tmp/evil",
            "git config core.hookspath /tmp/evil",
            "git config --local core.hooksPath /tmp/evil",
            "git config --global core.hooksPath /tmp/evil",
            "git config --system core.hooksPath /tmp/evil",
            "git config --worktree core.hooksPath /tmp/evil",
            "git config --file .git/config core.hooksPath /tmp/evil",
            "git config -f .git/config core.hooksPath /tmp/evil",
            "git config --type=path core.hooksPath /tmp/evil",
            "git config core.hooksPath /tmp/evil 2>/dev/null",
            "git config core.hooksPath '2>/dev/null'",
            "git config --add core.hooksPath /tmp/evil",
            "git config --replace-all core.hooksPath /tmp/evil",
            "git config --unset core.hooksPath",
            "git config --unset-all core.hooksPath",
            "git config --global --unset core.hooksPath",
            "git config --unset core.hooksPath 2>/dev/null",
            "git config set core.hooksPath /tmp/evil",
            "git config set --global core.hooksPath /tmp/evil",
            "git config unset core.hooksPath",
            "git config -e",
            "git config --edit",
            "git config --global --edit",
            "git config edit",
            "git config --remove-section core",
            "git config --rename-section core old",
            "git config remove-section core",
            "git config alias.x '!git config core.hooksPath /tmp/evil'",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_config_other_keys_not_hook_integrity() {
        // Writes to unrelated keys and sections are not an integrity concern.
        let p = default_policy();
        for cmd in [
            "git config user.name x",
            "git config --unset user.name",
            "git config --remove-section alias",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(!has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
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

    /// `sed` takes the in-place flag inside a cluster and with an attached
    /// backup suffix, so the guard reads the cluster's letters rather than the
    /// whole token.
    #[test]
    fn test_integrity_in_place_edit_blocked_in_a_short_cluster() {
        let p = default_policy();
        for cmd in [
            "sed -ni s/block/off/ .codeflow/policy.json",
            "sed -i.bak s/block/off/ .codeflow/policy.json",
            "sed -ni.bak s/block/off/ .codeflow/policy.json",
            "sed -Ei s/block/off/ .codeflow/policy.json",
            "sed --in-place s/block/off/ .codeflow/policy.json",
            "sed --in-place=.bak s/block/off/ .codeflow/policy.json",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    /// Read-only `sed` over an integrity path, including scripts and script
    /// files whose value carries an `i`. A whole-cluster scan would read the
    /// value as flags and block the read.
    #[test]
    fn test_integrity_stream_read_with_sed_allowed() {
        let p = default_policy();
        for cmd in [
            "sed -n 1,5p .codeflow/policy.json",
            "sed -e s/block/off/ .codeflow/policy.json",
            "sed -es/input/output/ .codeflow/policy.json",
            "sed -f script.sed .codeflow/policy.json",
            "sed -n -e p .codeflow/policy.json",
            "sed -Ef /tmp/script.sed .codeflow/policy.json",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty(), "{cmd}");
        }
    }

    /// The option reader itself, so a misread cluster or a swallowed operand
    /// is visible directly rather than only as a missing violation. Each row
    /// is a command line, its spec, the short letters that are really flags,
    /// and the operands left over.
    #[test]
    fn test_option_parsing_follows_each_command_arity() {
        let table: [(&str, &OptionSpec, &[char], &[&str]); 16] = [
            // git branch: delete clusters, and options whose value is attached
            // or separate.
            ("-Dq main", &GIT_BRANCH_OPTIONS, &['D', 'q'], &["main"]),
            ("-qd main", &GIT_BRANCH_OPTIONS, &['q', 'd'], &["main"]),
            ("-uorigin/dev main", &GIT_BRANCH_OPTIONS, &['u'], &["main"]),
            ("-u origin/dev main", &GIT_BRANCH_OPTIONS, &['u'], &["main"]),
            // -m and -M rename: the names are operands, not option values.
            (
                "-m main main2",
                &GIT_BRANCH_OPTIONS,
                &['m'],
                &["main", "main2"],
            ),
            (
                "-M main main2",
                &GIT_BRANCH_OPTIONS,
                &['M'],
                &["main", "main2"],
            ),
            // `--` ends the options and every later token is an operand, even
            // one that reads like an option.
            ("-d -- -weird", &GIT_BRANCH_OPTIONS, &['d'], &["-weird"]),
            ("-d -- main", &GIT_BRANCH_OPTIONS, &['d'], &["main"]),
            // git commit: values consumed before anything later is read.
            ("-an -m wip", &GIT_COMMIT_OPTIONS, &['a', 'n', 'm'], &[]),
            ("-mone", &GIT_COMMIT_OPTIONS, &['m'], &[]),
            ("-anm wip", &GIT_COMMIT_OPTIONS, &['a', 'n', 'm'], &[]),
            ("-uno", &GIT_COMMIT_OPTIONS, &['u'], &[]),
            ("-tplan.txt", &GIT_COMMIT_OPTIONS, &['t'], &[]),
            // git update-ref: the reason is a value, the ref is the operand.
            (
                "-dm reason refs/heads/main",
                &GIT_UPDATE_REF_OPTIONS,
                &['d', 'm'],
                &["refs/heads/main"],
            ),
            // sed: a script is a value, an attached suffix is not an operand.
            ("-es/input/output/ file", &SED_OPTIONS, &['e'], &["file"]),
            ("-ni.bak file", &SED_OPTIONS, &['n', 'i'], &["file"]),
        ];
        for (line, spec, flags, operands) in table {
            let args: Vec<String> = line.split_whitespace().map(str::to_string).collect();
            let parsed = parse_options(&args, spec);
            assert_eq!(parsed.short, flags.to_vec(), "flags of {line}");
            assert_eq!(parsed.operands, operands.to_vec(), "operands of {line}");
        }
    }

    /// A required value is taken before anything after it is interpreted, so
    /// a value that reads like `--` or like another option is the value.
    #[test]
    fn test_a_required_value_is_consumed_before_the_rest_is_read() {
        let p = default_policy();
        // `--` is the message here, so the later `-n` is the real bypass.
        let v = evaluate("git commit -m -- -n", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.no_verify_bypass"), "{v:?}");
        // Here `--no-verify` is the message text, not a bypass.
        let v = evaluate("git commit -m --no-verify", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
        assert!(!has_rule(&v, "git.no_verify_bypass"), "{v:?}");
        // The script file is named `--`, so `-i.bak` is still an edit.
        let v = evaluate("sed -f -- -i.bak .codeflow/policy.json", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.hook_integrity"), "{v:?}");
    }

    /// Git accepts `--end-of-options` wherever it accepts `--`, and it is read
    /// only when it arrives as an option: as the value of `-m` it is message
    /// text, so a later `-n` is still the real bypass.
    #[test]
    fn test_end_of_options_terminates_like_a_bare_dashdash() {
        let p = default_policy();
        for cmd in [
            "git branch -D --end-of-options -u main",
            "git branch --delete --end-of-options main",
            "git update-ref -d --end-of-options refs/heads/main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.delete_protected"), "{cmd}: {v:?}");
        }
        let v = evaluate("git commit -m --end-of-options -n", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.no_verify_bypass"), "{v:?}");
        let v = evaluate("git commit -m --end-of-options", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
        assert!(!has_rule(&v, "git.no_verify_bypass"), "{v:?}");
    }

    /// Git resolves any unambiguous prefix of a long option, so the guard has
    /// to resolve one too before it decides what a command does.
    #[test]
    fn test_abbreviated_long_options_resolve_like_git() {
        let p = default_policy();
        for cmd in [
            "git branch --del main",
            "git branch --forc --del main",
            "git branch -D --set-upstream-t origin/dev main",
            "git push --del origin main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.delete_protected"), "{cmd}: {v:?}");
        }
        let v = evaluate("git commit --no-ver", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.no_verify_bypass"), "{v:?}");
        let v = evaluate("git reset --har", &ctx(&p, "main"));
        assert!(has_rule(&v, "git.hard_reset_protected"), "{v:?}");
        // An ambiguous prefix is not an option: git rejects the command, and
        // the guard reads it as a plain flag rather than guessing.
        let v = evaluate("git commit --n -m wip", &ctx(&p, "main"));
        assert!(!has_rule(&v, "git.no_verify_bypass"), "{v:?}");
    }

    /// The resolver itself, including the commands that do not abbreviate.
    #[test]
    fn test_long_option_resolution_table() {
        assert_eq!(
            GIT_BRANCH_OPTIONS.resolve_long("--delete"),
            Some("--delete")
        );
        assert_eq!(GIT_BRANCH_OPTIONS.resolve_long("--del"), Some("--delete"));
        assert_eq!(GIT_BRANCH_OPTIONS.resolve_long("--forc"), Some("--force"));
        assert_eq!(
            GIT_BRANCH_OPTIONS.resolve_long("--set-upstream-t"),
            Some("--set-upstream-to")
        );
        assert_eq!(GIT_BRANCH_OPTIONS.resolve_long("--no-"), None);
        assert_eq!(GIT_BRANCH_OPTIONS.resolve_long("--nonsense"), None);
        assert_eq!(GIT_COMMIT_OPTIONS.resolve_long("--mess"), Some("--message"));
        assert_eq!(
            GIT_COMMIT_OPTIONS.resolve_long("--no-ver"),
            Some("--no-verify")
        );
        assert_eq!(GIT_COMMIT_OPTIONS.resolve_long("--n"), None);
        assert_eq!(GIT_PUSH_OPTIONS.resolve_long("--mirr"), Some("--mirror"));
        assert_eq!(GIT_PUSH_OPTIONS.resolve_long("--forc"), None);
        // `sed` is not parse-options: only the written name counts.
        assert_eq!(SED_OPTIONS.resolve_long("--in-place"), Some("--in-place"));
        assert_eq!(SED_OPTIONS.resolve_long("--in-pl"), None);
    }

    /// `--stdin` is stdin mode only when it arrives as an option; as the `-m`
    /// reason it is text, and the ref after it is the operand.
    #[test]
    fn test_update_ref_stdin_is_read_from_the_parsed_options() {
        let p = default_policy();
        let v = evaluate(
            "git update-ref -m --stdin refs/heads/topic HEAD",
            &ctx(&p, "feat/x"),
        );
        assert!(v.is_empty(), "{v:?}");
        let v = evaluate("git update-ref --stdin", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.local_ref_protection"), "{v:?}");
        let v = evaluate(
            "git update-ref -m --stdin refs/heads/main HEAD",
            &ctx(&p, "feat/x"),
        );
        assert!(has_rule(&v, "git.local_ref_protection"), "{v:?}");
    }

    /// Operands are read with option arity, so a protected name is found
    /// behind a consumed value and after an end-of-options marker.
    #[test]
    fn test_protected_operands_survive_option_values_and_end_of_options() {
        let p = default_policy();
        for cmd in [
            "git update-ref -dm reason refs/heads/main",
            "git update-ref -d -m reason refs/heads/main",
            "git branch -d -- main",
            "git branch -D -- main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.delete_protected"), "{cmd}: {v:?}");
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
        let resolver = |_: &Retarget<'_>| same_repo("main");
        let v = evaluate(
            "git -C /root commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_retarget_cd_then_commit() {
        let p = default_policy();
        let resolver = |_: &Retarget<'_>| same_repo("main");
        let v = evaluate(
            "cd /root && git commit -m x",
            &ctx_with_dir_branch(&p, "feat/x", &resolver),
        );
        assert!(has_rule(&v, "git.commit_to_protected"), "{v:?}");
    }

    #[test]
    fn test_retarget_git_dir_env_prefix() {
        let p = default_policy();
        let resolver = |_: &Retarget<'_>| same_repo("main");
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
        let resolver = |_: &Retarget<'_>| same_repo("feat/y");
        assert!(evaluate(
            "git -C /other commit -m x",
            &ctx_with_dir_branch(&p, "main", &resolver)
        )
        .is_empty());
    }

    #[test]
    fn test_retarget_resolver_none_blocks_as_unresolved() {
        // The resolver cannot read the target repository (TSK-112, T112-4):
        // the guard cannot prove the target is unprotected, so a commit
        // blocks whatever the session branch is, and says how to resolve it.
        let p = default_policy();
        let resolver = |_: &Retarget<'_>| None;
        for session in ["feat/x", "main"] {
            let v = evaluate(
                "git -C /root commit -m x",
                &ctx_with_dir_branch(&p, session, &resolver),
            );
            assert!(has_rule(&v, "git.commit_to_protected"), "{session}: {v:?}");
            assert!(v[0]
                .message
                .contains("target unresolved: no readable repository at `/root`"));
            assert!(v[0].remedy.contains("literal path"));
        }
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
    fn test_update_ref_remote_tracking_feature_refused() {
        let p = default_policy();
        assert!(has_rule(
            &evaluate(
                "git update-ref refs/remotes/origin/feat/x abc",
                &ctx(&p, "feat/x")
            ),
            "git.policy_authority"
        ));
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
        let resolver = |_: &Retarget<'_>| same_repo("feat/y");
        assert!(evaluate(
            "git -C sub status",
            &ctx_with_dir_branch(&p, "main", &resolver)
        )
        .is_empty());
    }

    // -- data is not a command: heredoc bodies, quoted arguments, comments --

    #[test]
    fn test_heredoc_body_is_data() {
        // Prose written through a heredoc names commands without running
        // them: quoted and unquoted delimiters, `<<-`, and bodies with
        // backticks, apostrophes and `;` that only look like shell syntax.
        let p = default_policy();
        for cmd in [
            "python3 - <<'EOF'\ntext = \"Pull requests into main are merged by the operator only; never run gh pr merge.\"\nEOF",
            "cat > AGENTS.md <<'EOF'\n- Never run `gh pr merge`; the operator merges.\n- Don't `git push --force origin main`.\nEOF",
            "cat > AGENTS.md <<\"EOF\"\ngh pr merge 12 --squash\nEOF",
            "cat > notes.md <<EOF\ngh pr merge 12 --squash; git commit -m x\nEOF",
            "cat <<-EOF > notes.md\n\tgh pr merge 12\n\tEOF",
            "cat <<EOF\ngh pr merge \\$(12) and \\`gh pr merge 13\\`\nEOF",
            "git commit -F - <<'EOF'\nfix: x\n\n- then gh pr merge after review\nEOF",
            "cat <<'A' <<'B'\ngh pr merge 1\nA\ngh pr merge 2\nB",
            "gh pr create --title t --body \"$(cat <<'EOF'\nMerge with `gh pr merge 12`; the operator does it.\nEOF\n)\"",
            "git commit -m \"$(cat <<'EOF'\nfix: x\n\n- never `gh pr merge 12`\nEOF\n)\"",
            "x=$(cat <<'EOF'\n`gh pr merge 12`\nEOF\n)",
            "cat > f <<'EOF' && git add f\nnever `gh pr merge 12`\nEOF",
            "cat <<'EOF' | tee -a f | grep -c x\n`gh pr merge 12`\nEOF",
            "gh pr create --title t --body-file - <<'EOF'\nnever `gh pr merge 12`\nEOF",
            "jq -n --arg b \"$(cat <<'EOF'\n`gh pr merge 12`\nEOF\n)\" '$b'",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(v.is_empty(), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_quoted_arguments_and_comments_are_data() {
        let p = default_policy();
        for cmd in [
            "echo 'run gh pr merge; then git push --force origin main'",
            "echo \"never run gh pr merge\"",
            "python3 -c \"print('gh pr merge 12; git commit -m x')\"",
            "printf '%s\\n' 'git commit -m x' '`gh pr merge 12`'",
            "ls # later: gh pr merge 12; git push --force origin main",
            "# don't run `gh pr merge`\nls",
            "ls;# gh pr merge 12",
        ] {
            let v = evaluate(cmd, &ctx(&p, "main"));
            assert!(v.is_empty(), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_real_invocations_around_data_still_caught() {
        // A real invocation in command position is caught wherever it sits:
        // chained, piped, substituted, wrapped, fed to a shell as a heredoc
        // script, substituted inside an unquoted heredoc body, after a
        // heredoc's terminator, or after a heredoc that never terminates.
        let p = default_policy();
        for cmd in [
            "ls && gh pr merge 12",
            "ls; gh pr merge 12",
            "echo y | gh pr merge 12",
            "echo $(gh pr merge 12)",
            "echo `gh pr merge 12`",
            "echo \"see `gh pr merge 12`\"",
            "bash -c \"gh pr merge 12\"",
            "bash <<'EOF'\ngh pr merge 12\nEOF",
            "sh -s <<EOF\nls\ngh pr merge 12\nEOF",
            "cat <<'EOF' | sh\ngh pr merge 12\nEOF",
            "cat <<EOF\n$(gh pr merge 12)\nEOF",
            "cat <<EOF\n`gh pr merge 12`\nEOF",
            "cat > f <<'EOF'\ntext\nEOF\ngh pr merge 12",
            "cat <<'EOF'\ngh pr merge 12",
            "cat <<$'EOF'\ngh pr merge 12\nEOF",
            "bash -c \"$(cat <<'EOF'\ngh pr merge 12\nEOF\n)\"",
            "eval \"$(cat <<'EOF'\ngh pr merge 12\nEOF\n)\"",
            "sh <<< \"$(cat <<'EOF'\ngh pr merge 12\nEOF\n)\"",
            "$(cat <<'EOF'\ngh pr merge 12\nEOF\n)",
            "cat <<'EOF' | \\\nbash\ngh pr merge 12\nEOF",
            "cat <<'EOF' |\ngh pr merge 12\nEOF\nbash",
            "$SHELL <<'EOF'\ngh pr merge 12\nEOF",
            "ssh host <<'EOF'\ngh pr merge 12\nEOF",
            "sudo -s <<'EOF'\ngh pr merge 12\nEOF",
            "make -f - <<'EOF'\nall:\n\tgh pr merge 12\nEOF",
            "x=$[1<<2]\ngh pr merge 12\n2]",
            "bash -c 2>/dev/null \"gh pr merge 12\"",
            "awk '{system($0)}' <<'EOF'\ngh pr merge 12\nEOF",
            "perl -ne 'system $_' <<'EOF'\ngh pr merge 12\nEOF",
            "sed e <<'EOF'\ngh pr merge 12\nEOF",
            "git -c alias.x='!sh' x <<'EOF'\ngh pr merge 12\nEOF",
            "cat <<'EOF' | git -c alias.x='!sh' x\ngh pr merge 12\nEOF",
            "cat <<'EOF' | { bash; }\ngh pr merge 12\nEOF",
            "cat <<'EOF' | (bash)\ngh pr merge 12\nEOF",
            "(cat <<'EOF'\ngh pr merge 12\nEOF\n) | bash",
            "cat <<'EOF' > >(sh)\ngh pr merge 12\nEOF",
            "echo \"$(cat <<'EOF'\ngh pr merge 12\nEOF\n)\" | bash",
            "printf '%s\\n' \"$(cat <<'EOF'\ngh pr merge 12\nEOF\n)\" | sh",
            "(bash) <<'EOF'\ngh pr merge 12\nEOF",
            "{ bash; } <<'EOF'\ngh pr merge 12\nEOF",
            "tee >(sh) <<'EOF'\ngh pr merge 12\nEOF",
            "(( x = 1 << 2 ))\ngh pr merge 12\n2",
            "echo $(true)#; gh pr merge 12",
            "(( 1 #)); gh pr merge 12",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.pr_merge_to_protected"), "{cmd}: {v:?}");
        }
        for cmd in [
            "cat > f <<'EOF'\ntext\nEOF\ngit commit -m x",
            "echo 'text' && git commit -m x",
            "echo ok # note\ngit commit -m x",
            "git \\\ncommit -m x",
            "bash -c \"$(cat <<'EOF'\ngit commit -m x\nEOF\n)\"",
            "git checkout feat/x -- f && git commit -m x",
            "git checkout feat/x f && git commit -m x",
            "(bash) <<'EOF'\ngit commit -m x\nEOF",
        ] {
            let v = evaluate(cmd, &ctx(&p, "main"));
            assert!(has_rule(&v, "git.commit_to_protected"), "{cmd}: {v:?}");
        }
    }

    // -- chained checkout in a retargeted directory --

    #[test]
    fn test_retargeted_switch_then_commit_allowed() {
        // The target repo is on main, but the chain first moves it to a new
        // feature branch; the commit lands there.
        let p = default_policy();
        let resolver = |_: &Retarget<'_>| same_repo("main");
        for cmd in [
            "cd /repo && git switch -c feat/x && git commit -m x",
            "cd /repo && git checkout -b feat/x && git commit -m x",
            "git -C /repo switch -c feat/x && git -C /repo commit -m x",
            "git -C /repo switch -c feat/x && git -C /repo/ commit -m x",
        ] {
            let v = evaluate(cmd, &ctx_with_dir_branch(&p, "main", &resolver));
            assert!(v.is_empty(), "{cmd}: {v:?}");
        }
    }

    #[test]
    fn test_retargeted_switch_does_not_leak() {
        // A switch in one directory says nothing about another directory or
        // the session, and a switch back to main is tracked too.
        let p = default_policy();
        let on_main = |_: &Retarget<'_>| same_repo("main");
        let on_feat = |_: &Retarget<'_>| same_repo("feat/y");
        for (cmd, resolver, session) in [
            (
                "git -C /a switch -c feat/x && git -C /b commit -m x",
                &on_main as &dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
                "feat/s",
            ),
            (
                "git -C /a switch -c feat/x && git commit -m x",
                &on_main as &dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
                "main",
            ),
            (
                "cd /repo && git switch main && git commit -m x",
                &on_feat as &dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
                "feat/s",
            ),
            (
                "cd /repo && git checkout feat/x -- f && git commit -m x",
                &on_main as &dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
                "feat/s",
            ),
            (
                "git -C /repo checkout feat/x -- f && git -C /repo commit -m x",
                &on_main as &dyn Fn(&Retarget<'_>) -> Option<TargetRepo>,
                "feat/s",
            ),
        ] {
            let v = evaluate(cmd, &ctx_with_dir_branch(&p, session, resolver));
            assert!(has_rule(&v, "git.commit_to_protected"), "{cmd}: {v:?}");
        }
    }

    // -- TSK-112: judge a git op by the repository it targets --

    /// A resolver over fixed repositories. `/scratch` and `/work/scratch` are
    /// other repositories on a feature branch with their own policy;
    /// `/scratch-main` has no policy file, so the defaults protect its `main`;
    /// `/release-repo` protects `release/*`; `/session/.git` is the session
    /// repository's git dir, on `main`.
    fn fixture_resolver(spec: &Retarget<'_>) -> Option<TargetRepo> {
        let other = |branch: &str, policy: GitPolicy| {
            Some(TargetRepo {
                branch: branch.to_string(),
                policy: Some(policy),
                root: None,
            })
        };
        match (spec.path, spec.git_dir) {
            ("/scratch" | "/work/scratch", false) => other("feat/x", GitPolicy::default()),
            ("/scratch-main" | "/aliased" | "/unreadable", false) => {
                other("main", GitPolicy::default())
            }
            ("/release-repo", false) => other("release/1.0", release_policy()),
            ("/session/.git", true) => same_repo("main"),
            _ => None,
        }
    }

    fn blocks(v: &[Violation]) -> bool {
        v.iter().any(|x| x.level == PolicyLevel::Block)
    }

    // R3-1: a substitution at or before the git subcommand, or in a global
    // option, decides which command runs; the command is unclassifiable and
    // blocks, and the message says how to rewrite it.
    #[test]
    fn test_tsk112_substitution_before_the_subcommand_blocks() {
        for (cmd, session) in [
            (
                "git $(printf '') commit --allow-empty -m \"fix: probe\"",
                "main",
            ),
            (
                "git `printf ''` commit --allow-empty -m \"fix: probe\"",
                "main",
            ),
            (
                "git $(printf -- '--no-pager') commit --allow-empty -m \"fix: probe\"",
                "main",
            ),
            (
                "git -C /scratch-main $(printf '') commit --allow-empty -m \"fix: probe\"",
                "feat/s",
            ),
            ("git \"$(printf commit)\" -m x", "feat/s"),
            ("git -c \"$(printf x=y)\" commit -m x", "feat/s"),
            ("$(printf git) push origin main", "feat/s"),
            ("command `printf git` push origin main", "feat/s"),
            ("git push origin \"$(printf main)\"", "feat/s"),
            ("git commit -m $(printf 'x --no-verify')", "main"),
        ] {
            let r = report(cmd, session);
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
            let v = r
                .violations
                .iter()
                .find(|v| {
                    v.message
                        .starts_with("command unresolved: a command substitution in")
                })
                .unwrap_or_else(|| panic!("{cmd}: {:?}", r.violations));
            assert_eq!(v.level, PolicyLevel::Block, "{cmd}");
            assert!(v.remedy.contains("literally"), "{}", v.remedy);
        }
        // Quoted message values and read-only subcommands stay classifiable.
        for cmd in [
            "git commit -m \"$(printf 'fix: x')\"",
            "git commit -am \"$(printf 'fix: x')\"",
            "git commit --message=\"$(printf 'fix: x')\"",
            "git merge --no-ff -m \"$(printf 'merge x')\" feat/y",
            "git log \"$(git merge-base a b)\"..HEAD",
            "git show $(git rev-parse HEAD)",
        ] {
            let r = report(cmd, "feat/s");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    // R3-1, property style: a substitution inserted at, or attached to, any
    // argv position up to and including the subcommand never yields an allow.
    #[test]
    fn test_tsk112_no_placeholder_before_the_subcommand_allows() {
        let bases: [(&str, usize); 5] = [
            ("git commit --allow-empty -m x", 1),
            ("git -C /scratch-main commit -m x", 3),
            ("git --no-pager -C /scratch push origin main", 4),
            ("git -c core.editor=true merge --no-ff feat/y", 3),
            ("git reset --hard HEAD~1", 1),
        ];
        let subs = [
            "$(printf '')",
            "`printf ''`",
            "\"$(printf '')\"",
            "$(printf -- --no-pager)",
        ];
        let mut cases = 0;
        for (base, sub_at) in bases {
            let words: Vec<&str> = base.split(' ').collect();
            for s in subs {
                for pos in 1..=sub_at + 1 {
                    let mut inserted = words.clone();
                    inserted.insert(pos, s);
                    let cmd = inserted.join(" ");
                    // Just after the subcommand, an unknown argument of a
                    // current-branch subcommand matters only on a protected
                    // target (R4-1); the main session covers that position.
                    let sessions: &[&str] = if pos > sub_at {
                        &["main"]
                    } else {
                        &["main", "feat/s"]
                    };
                    for &session in sessions {
                        let r = report(&cmd, session);
                        assert!(
                            blocks(&r.violations),
                            "{cmd} ({session}): {:?}",
                            r.violations
                        );
                        cases += 1;
                    }
                }
                for pos in 1..=sub_at {
                    for attached in [format!("{s}{}", words[pos]), format!("{}{s}", words[pos])] {
                        let mut joined = words.clone();
                        joined[pos] = &attached;
                        let cmd = joined.join(" ");
                        for session in ["main", "feat/s"] {
                            let r = report(&cmd, session);
                            assert!(
                                blocks(&r.violations),
                                "{cmd} ({session}): {:?}",
                                r.violations
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert!(cases > 200, "{cases}");
    }

    /// An alias reader over fixed configuration. The session defines `ci`
    /// (commit) and `st` (status); `/aliased` defines `x` (commit), `sh` (a
    /// `!` shell alias), `opt` (starts with an option), `loop` (itself) and
    /// `quoted` (split by quotes); `/unreadable` cannot be read. The command's
    /// own `-c alias.<name>=…` wins, and `-c include.path=/aliases.ini`
    /// defines `x` as commit.
    fn fixture_alias(q: &AliasQuery<'_>) -> AliasAnswer {
        let mut found: Option<&str> = match (q.target.map(|t| t.path), q.name) {
            (None, "ci") | (Some("/aliased"), "x") => Some("commit"),
            (None, "st") => Some("status --short"),
            (Some("/aliased"), "sh") => Some("!git commit"),
            (Some("/aliased"), "opt") => Some("-C /elsewhere commit"),
            (Some("/aliased"), "loop") => Some("loop"),
            (Some("/aliased"), "quoted") => Some("commit -m 'a b'"),
            (Some("/unreadable"), _) => {
                return AliasAnswer::Unreadable("fixture".to_string());
            }
            _ => None,
        };
        let key = format!("alias.{}", q.name);
        for setting in q.config {
            if let Some((k, v)) = setting.split_once('=') {
                if k == key {
                    found = Some(v);
                } else if k == "include.path" && v == "/aliases.ini" && q.name == "x" {
                    found = Some("commit");
                }
            }
        }
        found.map_or(AliasAnswer::NotAlias, |v| {
            AliasAnswer::Expansion(v.to_string())
        })
    }

    fn report(cmd: &str, session: &str) -> Evaluation {
        let p = default_policy();
        evaluate_report(cmd, &ctx_with_dir_branch(&p, session, &fixture_resolver))
    }

    // Aliases (TSK-112, primary ruling after round 4): a subcommand that is
    // not a builtin is judged by the alias it expands to; one the guard
    // cannot read is unclassifiable.
    #[test]
    fn test_tsk112_aliases_are_judged_by_their_expansion() {
        let has = |r: &Evaluation, rule: &str| r.violations.iter().any(|v| v.rule == rule);
        // Codex round 4 reproductions and their relatives, on main.
        for cmd in [
            "git -c alias.x=commit x --allow-empty -m \"$(printf x)\"",
            "git -c alias.x=commit x $(printf '') --allow-empty -m x",
            "git -c include.path=/aliases.ini x --allow-empty -m \"$(printf x)\"",
            "git -c include.path=/aliases.ini x $(printf '') --allow-empty -m x",
            "git ci --allow-empty -m x",
            "git -c alias.y=ci y -m x",
        ] {
            let r = report(cmd, "main");
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
            assert!(
                has(&r, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
        // The alias is read in the repository the op targets.
        let r = report("git -C /aliased x -m y", "feat/s");
        assert!(has(&r, "git.commit_to_protected"), "{:?}", r.violations);
        // The expansion is judged like a literal command.
        let r = report("git -c alias.x='commit --no-verify' x -m y", "main");
        assert!(has(&r, "git.no_verify_bypass"), "{:?}", r.violations);
        let r = report("git -c alias.p=push p origin main", "feat/s");
        assert!(has(&r, "git.push_to_protected"), "{:?}", r.violations);
        // Aliases the guard cannot read are unclassifiable, on any branch.
        for cmd in [
            "git -C /aliased sh",
            "git -C /aliased opt",
            "git -C /aliased loop",
            "git -C /unreadable x -m y",
            "git -c alias.x='!git commit' x -m y",
            "git --config-env=alias.x=V x -m y",
            "git --config-env alias.x=V x -m y",
            "GIT_CONFIG_GLOBAL=/tmp/g git x -m y",
            "export GIT_CONFIG_COUNT=1; git x -m y",
            "HOME=/tmp git x -m y",
            "unset HOME; git x -m y",
            "XDG_CONFIG_HOME=/tmp git x -m y",
            "git -C \"$UNSET\" x -m y",
        ] {
            let r = report(cmd, "feat/s");
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.message.contains("an alias the guard cannot resolve")),
                "{cmd}: {:?}",
                r.violations
            );
        }
        // Without an alias reader, a subcommand that is not a builtin blocks.
        let p = default_policy();
        assert!(blocks(&evaluate("git frobnicate", &ctx(&p, "feat/x"))));
        // Controls: a builtin wins over an alias of its name, as in git; an
        // alias to a read-only command, a name that is no alias, and a commit
        // alias on a feature branch pass.
        for (cmd, session) in [
            ("git -c alias.log=commit log", "main"),
            ("git st", "main"),
            ("git frobnicate", "main"),
            ("git ci -m x", "feat/s"),
            ("git -c alias.x=commit x -m y", "feat/s"),
            ("echo \"$HOME\" && git -c alias.x=status x", "main"),
            ("CODEFLOW_HOME=/tmp git -c alias.x=status x", "main"),
        ] {
            let r = report(cmd, session);
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    #[test]
    fn test_tsk112_alias_values_split_like_git() {
        assert_eq!(
            split_alias("commit -m 'a b'"),
            Some(vec!["commit".into(), "-m".into(), "a b".into()])
        );
        assert_eq!(
            split_alias(" a \"b c\"  d\\ e "),
            Some(vec!["a".into(), "b c".into(), "d e".into()])
        );
        assert_eq!(split_alias("a 'b"), None);
        assert_eq!(split_alias("a\\"), None);
        assert!(mentions_config_env("HOME=/x git y"));
        assert!(mentions_config_env("unset HOME"));
        assert!(!mentions_config_env("git -C \"$HOME/r\" y"));
        assert!(!mentions_config_env("git -C ${HOME}/r y"));
        assert!(!mentions_config_env("CODEFLOW_HOME=1 git y"));
    }

    // A rebase given a `<branch>` checks it out and rewrites it: that branch
    // is the one judged (primary ruling after round 4).
    #[test]
    fn test_tsk112_rebase_judges_its_branch_argument() {
        for cmd in [
            "git rebase feat/y main",
            "git rebase --onto feat/z feat/y main",
            "git rebase --onto=feat/z feat/y main",
            "git rebase --root main",
            "git rebase -s ours -X theirs feat/y main",
            "git rebase -x 'make test' --exec 'true' feat/y master",
            "git rebase -i --autosquash feat/y -- main",
        ] {
            let r = report(cmd, "feat/s");
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.rule == "git.hard_reset_protected" && v.level == PolicyLevel::Block),
                "{cmd}: {:?}",
                r.violations
            );
        }
        for (cmd, session) in [
            ("git rebase main", "feat/s"),
            ("git rebase --onto main feat/y", "feat/s"),
            ("git rebase main feat/s", "feat/s"),
            ("git rebase --continue", "feat/s"),
            ("git rebase -S feat/y", "feat/s"),
            ("git rebase feat/y feat/x", "main"),
        ] {
            let r = report(cmd, session);
            assert!(
                r.violations.is_empty(),
                "{cmd} ({session}): {:?}",
                r.violations
            );
        }
        assert!(blocks(&report("git rebase main", "main").violations));
        // The rebased branch stays checked out for the rest of the line.
        let mut p = default_policy();
        p.hard_reset_protected = PolicyLevel::Off;
        let c = ctx_with_dir_branch(&p, "feat/s", &fixture_resolver);
        let r = evaluate_report("git rebase feat/y main && git commit -m x", &c);
        assert!(blocks(&r.violations), "{:?}", r.violations);
        let c = ctx_with_dir_branch(&p, "main", &fixture_resolver);
        let r = evaluate_report("git rebase main feat/x && git commit -m x", &c);
        assert!(r.violations.is_empty(), "{:?}", r.violations);
    }

    // A git command inside a control structure is judged like any other
    // (found while pinning round 5; it predates TSK-112).
    #[test]
    fn test_tsk112_control_structure_bodies_are_judged() {
        for cmd in [
            "for i in 1 2; do git commit -m y; done",
            "if true; then git commit -m y; fi",
            "if false; then :; else git commit -m y; fi",
            "if false; then :; elif git commit -m y; then :; fi",
            "while true; do git commit -m y; done",
            "until false; do git commit -m y; done",
            "select x in a; do git commit -m y; done",
            "if git commit -m y; then :; fi",
            "! git commit -m y",
            "time git commit -m y",
            "time -p git commit -m y",
            "case x in a) git commit -m y;; esac",
            "for i in 1; do then_x=1; git commit -m y; done",
        ] {
            let r = report(cmd, "main");
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.rule == "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
        let r = report("while true; do git push origin main; done", "feat/s");
        assert!(blocks(&r.violations), "{:?}", r.violations);
        // A directory move inside a body leaves later targets unresolved.
        let r = report(
            "if true; then cd /scratch-main; fi; git commit -m y",
            "feat/s",
        );
        assert!(blocks(&r.violations), "{:?}", r.violations);
        for cmd in [
            "for i in 1 2; do git status; done",
            "if true; then git log; fi",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    // R5-1: a branch move can fail, so it narrows the branch only for the
    // rest of its `&&` list; after `;` or a newline the earlier branch stays
    // a candidate. The same holds in retargeted repositories and for a rebase
    // that comes from an alias.
    #[test]
    fn test_tsk112_branch_moves_narrow_only_across_and() {
        for cmd in [
            "git rebase does-not-exist feat/x; git commit --allow-empty -m x",
            "git rebase does-not-exist feat/x\ngit commit --allow-empty -m x",
            "git checkout does-not-exist; git commit -m x",
            "git checkout feat/y\ngit commit -m x",
            "git switch feat/y && git commit -m x; git commit -m y",
            "git -c alias.rb=rebase rb does-not-exist feat/x; git commit -m x",
            "(git checkout feat/y) && git commit -m x",
            "echo \"$(git checkout feat/y)\" && git commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.rule == "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
        for cmd in [
            "git -C /scratch-main rebase nope feat/x; git -C /scratch-main commit -m x",
            "git -C /scratch-main checkout feat/q\ngit -C /scratch-main commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
        }
        // Proven by `&&`: the op runs only after the move succeeded.
        for (cmd, session) in [
            (
                "git rebase does-not-exist feat/x && git commit --allow-empty -m x",
                "main",
            ),
            (
                "git rebase main feat/x && git commit --allow-empty -m x",
                "main",
            ),
            ("git checkout feat/y && git commit -m x", "main"),
            (
                "git -c alias.rb=rebase rb main feat/x && git commit -m x",
                "main",
            ),
            (
                "git -C /scratch-main checkout feat/q && git -C /scratch-main commit -m x",
                "feat/s",
            ),
        ] {
            let r = report(cmd, session);
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
        // A move at one of several places narrows none of them: here the
        // checkout may have run in the session if the first `cd` failed.
        let r = report(
            "cd /scratch-main; git checkout feat/q && cd /scratch-main && git commit -m x",
            "feat/s",
        );
        assert!(blocks(&r.violations), "{:?}", r.violations);
        // A checkout the guard cannot place may have moved any repository.
        let r = report(
            "git -C \"$UNSET\" checkout main && git -C /scratch commit -m x",
            "feat/s",
        );
        assert!(blocks(&r.violations), "{:?}", r.violations);
    }

    // R5-2: an alias is read from disk before the line runs, so after an
    // earlier config write (or a branch move, which conditional includes
    // read) a subcommand that is not a builtin is unclassifiable.
    #[test]
    fn test_tsk112_config_changes_earlier_in_the_line() {
        for cmd in [
            "git config alias.x commit; git x --allow-empty -m y",
            "git config include.path /aliases.ini; git x -m y",
            "git config set alias.x commit && git x -m y",
            "git config --add alias.x commit; git x -m y",
            "git -c alias.c=config c alias.x commit; git x -m y",
            "echo \"$(git config alias.x commit)\"; git x -m y",
            "(git config alias.x commit); git x -m y",
            "printf '[alias] x = commit' >> .git/config; git x -m y",
            "git config --unset alias.x; git x -m y",
            "git config --edit; git x -m y",
            "for i in 1 2; do git x -m y; git config alias.x commit; done",
            "git checkout feat/y && git st",
        ] {
            let r = report(cmd, "main");
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.message.contains("an alias the guard cannot resolve")),
                "{cmd}: {:?}",
                r.violations
            );
        }
        // Reads and queries change nothing; builtins need no alias.
        for cmd in [
            "git config --get alias.x; git st",
            "git config alias.x; git st",
            "git config --list; git st",
            "git config get user.name && git st",
            "git config alias.x commit; git status",
            "git st; git config alias.x commit",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    // R4-1: classification uncertainty never ends the judgment early. With
    // `commit_to_protected: block`, a commit on main blocks whatever level
    // `local_ref_protection` has, on both reproduced forms.
    #[test]
    fn test_tsk112_uncertainty_keeps_the_protected_commit_check() {
        let forms = [
            "git commit --allow-empty -m x --author=\"$(printf 'X <x@example.com>')\"",
            "git commit $(printf '') --allow-empty -m x",
        ];
        for local in [PolicyLevel::Off, PolicyLevel::Warn, PolicyLevel::Block] {
            let mut p = default_policy();
            p.commit_to_protected = PolicyLevel::Block;
            p.local_ref_protection = local;
            for cmd in forms
                .iter()
                .copied()
                .chain(["git commit --allow-empty -m x"])
            {
                let r = evaluate_report(cmd, &ctx_with_dir_branch(&p, "main", &fixture_resolver));
                assert!(
                    blocks(&r.violations),
                    "{cmd} ({local:?}): {:?}",
                    r.violations
                );
                assert!(
                    r.violations
                        .iter()
                        .any(|v| v.rule == "git.commit_to_protected"
                            && v.level == PolicyLevel::Block),
                    "{cmd} ({local:?}): {:?}",
                    r.violations
                );
            }
            // A warning never authorizes: with every exposed rule at warn,
            // the verdict still carries the warning.
            let mut w = p.clone();
            w.commit_to_protected = PolicyLevel::Warn;
            for cmd in forms {
                let r = evaluate_report(cmd, &ctx_with_dir_branch(&w, "main", &fixture_resolver));
                assert!(
                    r.violations.iter().any(|v| v.level == PolicyLevel::Warn),
                    "{cmd} ({local:?}): {:?}",
                    r.violations
                );
            }
            // On a feature branch the target is not protected, so unknown
            // commit arguments cannot reach a protected branch.
            for cmd in forms {
                let r = evaluate_report(cmd, &ctx_with_dir_branch(&p, "feat/s", &fixture_resolver));
                assert!(
                    r.violations.is_empty(),
                    "{cmd} ({local:?}): {:?}",
                    r.violations
                );
            }
        }
        // Default policy: both forms block on main.
        for cmd in forms {
            assert!(blocks(&report(cmd, "main").violations), "{cmd}");
        }
        // A warning from the uncertainty never ends the judgment: the
        // `--no-verify` check on a protected branch still blocks.
        let mut w = default_policy();
        w.commit_to_protected = PolicyLevel::Warn;
        w.local_ref_protection = PolicyLevel::Off;
        for cmd in [
            "git commit --no-verify --author=\"$(printf 'X <x@example.com>')\" -m x",
            "git commit $(printf '') --no-verify -m x",
        ] {
            let r = evaluate_report(cmd, &ctx_with_dir_branch(&w, "main", &fixture_resolver));
            assert!(
                r.violations
                    .iter()
                    .any(|v| v.rule == "git.no_verify_bypass" && v.level == PolicyLevel::Block),
                "{cmd}: {:?}",
                r.violations
            );
        }
        // The uncertainty is judged under each candidate target's policy,
        // and the strictest of them wins.
        let mut lax = default_policy();
        for level in [
            &mut lax.commit_to_protected,
            &mut lax.merge_to_protected,
            &mut lax.push_to_protected,
            &mut lax.force_push_protected,
            &mut lax.delete_protected,
            &mut lax.hard_reset_protected,
            &mut lax.local_ref_protection,
            &mut lax.hook_integrity,
        ] {
            *level = PolicyLevel::Warn;
        }
        for (cmd, session) in [
            ("git -C /scratch-main $(printf '') commit -m x", "feat/s"),
            ("cd /scratch-main; git $(printf '') commit -m x", "main"),
        ] {
            let r = evaluate_report(cmd, &ctx_with_dir_branch(&lax, session, &fixture_resolver));
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
        }
        // A rebase's `<branch>` argument checks that branch out first, so an
        // unknown rebase argument blocks from a feature branch too.
        for cmd in [
            "git rebase feat/y \"$(printf main)\"",
            "git rebase --onto x y $(printf main)",
        ] {
            assert!(blocks(&report(cmd, "feat/s").violations), "{cmd}");
        }
        // With no subcommand at all, the uncertainty still blocks.
        let r = report("git -c \"$(printf alias.x=commit)\"", "feat/s");
        assert!(blocks(&r.violations), "{:?}", r.violations);
    }

    // Regression: the path held in a variable used to fall back to the
    // session's `main` and block (3.0.0 probe: exit 2 on `R=…; git -C "$R"`).
    #[test]
    fn test_tsk112_variable_path_judged_by_its_target() {
        for cmd in [
            "R=/scratch; git -C \"$R\" commit -m x",
            "R=/scratch && git -C \"$R\" commit -m x",
            "export R=/scratch\ngit -C \"${R}\" commit -m x",
            "D=/work; S=$D/scratch; git -C \"$S\" commit -m x",
            "R=/scratch; cd \"$R\" && git commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
            assert!(r.notes.is_empty(), "{cmd}: {:?}", r.notes);
        }
    }

    // Regression: a relative `-C` after a `cd` was read from the session cwd
    // (3.0.0 probe: exit 2 on `cd <dir> && git -C scratchf commit`).
    #[test]
    fn test_tsk112_relative_dash_c_after_cd() {
        for cmd in [
            "cd /work && git -C scratch commit -m x",
            "cd / && cd work && git -C scratch commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    // Regression: the target's own policy applies, so a pattern only it
    // protects blocks (it used to be judged by the session's list only).
    #[test]
    fn test_tsk112_target_only_protected_pattern_blocks() {
        for cmd in [
            "git -C /release-repo commit -m x",
            "cd /release-repo && git commit -m x",
            "git -C /release-repo commit -m \"$(echo x)\"",
        ] {
            let r = report(cmd, "feat/s");
            assert!(has_rule(&r.violations, "git.commit_to_protected"), "{cmd}");
            assert!(
                r.violations[0].message.contains("'release/1.0'"),
                "{cmd}: {:?}",
                r.violations
            );
        }
    }

    // Regression (wrong allow): `-C` plus `--git-dir`/`GIT_DIR` commits into
    // the git dir, which here is the session repository on `main`; only the
    // `-C` directory used to be read.
    #[test]
    fn test_tsk112_dash_c_with_git_dir_judges_the_git_dir() {
        for cmd in [
            "git -C /scratch --git-dir=/session/.git commit -m x",
            "git -C /scratch --git-dir /session/.git commit -m x",
            "git --git-dir=/session/.git -C /scratch commit -m x",
            "GIT_DIR=/session/.git git -C /scratch commit -m x",
            "git -C /scratch --git-dir=/session/.git commit -m \"$(echo x)\"",
            "export GIT_DIR=/session/.git; git -C /scratch commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
    }

    // The stricter reading: a repository with no policy file keeps the
    // default protection of `main` and `master`.
    #[test]
    fn test_tsk112_repo_without_policy_keeps_default_protection() {
        for cmd in [
            "git -C /scratch-main commit -m x",
            "R=/scratch-main; git -C \"$R\" commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(has_rule(&r.violations, "git.commit_to_protected"), "{cmd}");
        }
    }

    // AC-3 (T112-4): an unresolved mutation target blocks, even from an
    // unprotected session, and the output names what could not be resolved
    // and how to make it resolvable. It never claims the target is safe.
    #[test]
    fn test_tsk112_unresolved_target_blocks_and_says_how_to_resolve() {
        for session in ["main", "feat/s"] {
            let r = report("git -C \"$DEST\" commit -m x", session);
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{session}"
            );
            let v = &r.violations[0];
            assert!(
                v.message.contains("target unresolved: `$DEST`"),
                "{}",
                v.message
            );
            assert!(
                v.message
                    .contains("the guard cannot prove it is not a protected branch"),
                "{}",
                v.message
            );
            assert!(v.remedy.contains("literal path") && v.remedy.contains("cd /path/to/repo &&"));
            assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
            assert!(r.notes[0]
                .text
                .starts_with("`git commit`: target unresolved: `$DEST`"));
            assert!(r.notes[0].remedy.contains("literal path"), "{}", r.notes[0]);
        }
        // A read of an unresolved target is not a mutation: allowed, noted.
        let r = report("git -C \"$DEST\" status", "feat/s");
        assert!(r.violations.is_empty(), "{:?}", r.violations);
        assert_eq!(r.notes.len(), 1);
    }

    // T112-3: a `cd` can fail, so after `;` the old directory stays a
    // candidate; `&&` proves the move.
    #[test]
    fn test_tsk112_failing_cd_keeps_the_old_directory() {
        for cmd in [
            "R=/scratch; cd \"$R\" > /absent/out; git commit -m x",
            "cd /scratch; git commit -m x",
            "cd /scratch && true; git commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(has_rule(&r.violations, "git.commit_to_protected"), "{cmd}");
            assert!(
                r.notes.is_empty(),
                "{cmd}: both directories resolve: {:?}",
                r.notes
            );
        }
        for cmd in [
            "R=/scratch; cd \"$R\" > /tmp/out && git commit -m x",
            "cd /scratch && git commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
        // From an unprotected session both candidates are unprotected.
        assert!(report("cd /scratch; git commit -m x", "feat/s")
            .violations
            .is_empty());
    }

    // T112-1: an escaped or quoted `$` is literal to the shell; the guard
    // never expands it.
    #[test]
    fn test_tsk112_escaped_dollar_is_not_expanded() {
        for cmd in [
            "R=/scratch; git -C \"\\$R\" commit -m x",
            "R=/scratch; git -C \\$R commit -m x",
            "R=\\$X; git -C \"$R\" commit -m x",
            "git -C $'/scratch' commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
    }

    // T112-2: launcher environment (`env`, `command env`) is modeled; an
    // unmodeled `env` option or location variable leaves the target
    // unresolved.
    #[test]
    fn test_tsk112_launcher_environment_is_modeled() {
        for cmd in [
            "R=/scratch; env GIT_DIR=/session/.git git -C \"$R\" commit -m x",
            "R=/scratch; command env GIT_DIR=/session/.git git -C \"$R\" commit -m x",
            "env -u GIT_DIR git -C /scratch commit -m x",
            "env GIT_COMMON_DIR=/session/.git git -C /scratch commit -m x",
            "GIT_WORK_TREE=/x git -C /scratch commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
        let r = report("env FOO=1 git -C /scratch commit -m x", "main");
        assert!(r.violations.is_empty(), "{:?}", r.violations);
    }

    // A git op nested in a substitution runs in a subshell whose own `cd`
    // the tracker does not follow: its target is unresolved.
    #[test]
    fn test_tsk112_nested_moves_are_unresolved() {
        for cmd in [
            "echo \"$(cd /scratch-main && git commit -m x)\"",
            "echo \"$(R=/scratch-main; git -C \"$R\" commit -m x)\"",
        ] {
            let r = report(cmd, "feat/s");
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
        // Nothing moves inside: a nested literal target still resolves.
        let r = report(
            "echo \"$(git -C /scratch log -1)\"; git -C /scratch commit -m x",
            "main",
        );
        assert!(r.violations.is_empty(), "{:?}", r.violations);
    }

    // R2-1: a location word or assignment built from a substitution is
    // unknown; the shell puts the command's output there.
    #[test]
    fn test_tsk112_substituted_locations_are_unresolved() {
        for cmd in [
            "R=/scratch$(printf /protected); git -C \"$R\" commit -m x",
            "R=/scratch`printf /protected`; git -C \"$R\" commit -m x",
            "git -C \"$(printf /scratch)\" commit -m x",
            "git -C /scratch --git-dir=\"$(printf /x)\" commit -m x",
            "cd \"$(printf /scratch)\" && git commit -m x",
            "git -C \"$(printf /scratch)\" commit -m x | cat",
            "$(printf cd) /scratch-main; git commit -m x",
        ] {
            let r = report(cmd, "feat/s");
            assert!(blocks(&r.violations), "{cmd}: {:?}", r.violations);
        }
        // A substitution in the message does not touch the location.
        for cmd in [
            "git -C /scratch commit -m \"$(printf 'fix: x')\"",
            "cd /scratch && git commit -m \"`printf x`\"",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
    }

    // R2-2: `&&` stays proven across a continuation newline or comment; a
    // newline after a complete command is still a sequence.
    #[test]
    fn test_tsk112_and_list_continues_across_newlines() {
        for cmd in [
            "cd /scratch &&\ngit commit -m x",
            "cd /scratch && # into the scratch repo\ngit commit -m x",
            "cd /scratch &&\n\n  git commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(r.violations.is_empty(), "{cmd}: {:?}", r.violations);
        }
        let r = report("cd /scratch\ngit commit -m x", "main");
        assert!(has_rule(&r.violations, "git.commit_to_protected"));
    }

    // Forms the tracker cannot follow stay unknown: `pushd -n` (it does
    // not move), a negated `cd`, and an assignment whose redirection can fail.
    #[test]
    fn test_tsk112_unmodeled_moves_stay_unknown() {
        for (cmd, session) in [
            ("pushd -n /scratch && git commit -m x", "main"),
            ("! cd /scratch-main && git commit -m x", "feat/s"),
            (
                "R=/scratch-main; export R=/scratch > /absent/x; git -C \"$R\" commit -m x",
                "feat/s",
            ),
            (
                "R=/scratch-main; R=/scratch 2>/absent/x; git -C \"$R\" commit -m x",
                "feat/s",
            ),
        ] {
            let r = report(cmd, session);
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
    }

    #[test]
    fn q4_plain_pushd_has_cd_semantics() {
        let allowed = report("pushd /scratch && git commit -m x", "main");
        assert!(allowed.violations.is_empty(), "{:?}", allowed.violations);
        let denied = report("pushd /scratch-main && git commit -m x", "feat/s");
        assert!(has_rule(&denied.violations, "git.commit_to_protected"));
    }

    // A commit message built by a heredoc substitution keeps the line flat.
    #[test]
    fn test_tsk112_heredoc_message_keeps_the_move_resolvable() {
        let cmd = "cd /scratch && git commit -m \"$(cat <<'EOF'\nfix: x (y)\nEOF\n)\"";
        let r = report(cmd, "main");
        assert!(r.violations.is_empty(), "{:?}", r.violations);
        let r = report("cd /scratch-main && git commit -m \"$(echo x)\"", "feat/s");
        assert!(has_rule(&r.violations, "git.commit_to_protected"));
    }

    // AC-3: moves the guard cannot be sure of never loosen the verdict: the
    // session repository (on `main`) stays a candidate.
    #[test]
    fn test_tsk112_uncertain_moves_never_loosen_the_verdict() {
        for cmd in [
            // A subshell's assignment does not reach the parent shell.
            "(R=/scratch); git -C \"$R\" commit -m x",
            // An assignment that runs only when an earlier command succeeds.
            "false && R=/scratch; git -C \"$R\" commit -m x",
            // A prefix assignment is not visible to its own expansion.
            "R=/scratch git -C \"$R\" commit -m x",
            // Single quotes keep `$R` literal.
            "R=/scratch; git -C '$R' commit -m x",
            "R=/scratch; unset R; git -C \"$R\" commit -m x",
            "R=/scratch; read R; git -C \"$R\" commit -m x",
            "R=~/scratch; git -C \"$R\" commit -m x",
            "test -d /x && cd /scratch; git commit -m x",
            "R=/scratch; source env.sh; git -C \"$R\" commit -m x",
            "cd /scratch && cd -; git commit -m x",
            "GIT_COMMON_DIR=/session/.git git -C /scratch commit -m x",
        ] {
            let r = report(cmd, "main");
            assert!(
                has_rule(&r.violations, "git.commit_to_protected"),
                "{cmd}: {:?}",
                r.violations
            );
        }
    }

    /// Run git in `dir` with the developer's configuration isolated.
    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn real_repo(dir: &std::path::Path, branch: &str) {
        run_git(dir, &["init", "-q", "-b", branch]);
        run_git(
            dir,
            &[
                "-c",
                "user.email=t@e",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "chore: init",
            ],
        );
    }

    #[test]
    fn test_tsk112_read_target_reads_branch_and_whose_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp.path().join("session");
        let owned = tmp.path().join("owned");
        let bare_policy = tmp.path().join("nopolicy");
        for (dir, branch) in [
            (&session, "main"),
            (&owned, "feat/x"),
            (&bare_policy, "main"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            real_repo(dir, branch);
        }
        std::fs::create_dir_all(session.join("sub")).unwrap();
        std::fs::create_dir_all(owned.join(".codeflow")).unwrap();
        std::fs::write(
            owned.join(".codeflow/policy.json"),
            r#"{"git":{"protected_branches":["trunk"]}}"#,
        )
        .unwrap();
        run_git(&owned, &["add", ".codeflow/policy.json"]);
        run_git(
            &owned,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "commit",
                "-qm",
                "chore: policy",
            ],
        );
        let common = session.join(".git");
        let read = |path: &str, git_dir: bool| {
            read_target(&session, Some(&common), &Retarget { path, git_dir })
        };

        // Inside the session repository: its branch, the session's policy.
        let inside = read("sub", false).expect("session subdir resolves");
        assert_eq!(inside.branch, "main");
        assert!(inside.policy.is_none());
        let git_dir = read(common.to_str().unwrap(), true).expect("session git dir");
        assert!(git_dir.policy.is_none());

        // Another repository with a policy file: its own rules.
        let other = read(owned.to_str().unwrap(), false).expect("other repo resolves");
        assert_eq!(other.branch, "feat/x");
        assert_eq!(
            other.policy.unwrap().protected_branches,
            vec!["trunk".to_string()]
        );

        // No policy file: the defaults, which protect main.
        let plain = read("../nopolicy", false).expect("relative path resolves");
        assert!(plain.policy.unwrap().branch_is_protected("main"));

        // Nothing there: unresolved.
        assert!(read("missing", false).is_none());
        assert!(read("$R", false).is_none());
    }
}

/// State required for a destructive command, at its composed repository target.
pub struct DiscardQuery<'a> {
    pub target: Option<Retarget<'a>>,
    pub intent: &'a super::git_discard::Intent,
}
/// An unreadable or absent state probe is uncertainty, never a clean result.
pub type DiscardLookup<'a> =
    Option<&'a dyn Fn(&DiscardQuery<'_>) -> Result<Option<String>, String>>;

/// Normalize the destructive forms with the same option parser as other rules.
#[allow(clippy::too_many_lines)] // One explicit arm per supported Git discard operation.
fn discard_intent(
    sub: &str,
    rest: &[String],
    moved: &Moves<'_>,
) -> Result<Option<super::git_discard::Intent>, String> {
    use super::git_discard::Intent;
    if !matches!(
        sub,
        "reset"
            | "restore"
            | "checkout"
            | "switch"
            | "clean"
            | "stash"
            | "branch"
            | "worktree"
            | "update-ref"
    ) {
        return Ok(None);
    }
    let no_vars = HashMap::new();
    let vars = moved.vars.unwrap_or(&no_vars);
    let mut operands_only = false;
    let rest: Vec<String> = rest
        .iter()
        .map(|s| {
            if s == "--" || s == END_OF_OPTIONS {
                operands_only = true;
            }
            if operands_only && matches!(sub, "branch" | "switch") {
                Ok(s.clone())
            } else if has_substitution(s) {
                Err("a generated argument can change what would be discarded".into())
            } else {
                expand_word(s, vars)
            }
        })
        .collect::<Result<_, String>>()?;
    let spec = match sub {
        "reset" => &GIT_RESET_OPTIONS,
        "branch" => &GIT_BRANCH_OPTIONS,
        "update-ref" => &GIT_UPDATE_REF_OPTIONS,
        "clean" => &DISCARD_CLEAN_OPTIONS,
        "restore" | "checkout" | "switch" => &DISCARD_RESTORE_OPTIONS,
        _ => &DISCARD_MISC_OPTIONS,
    };
    let parsed = parse_options(&rest, spec);
    if parsed.has_short(&['h']) || parsed.has_long("--help") {
        return Ok(None);
    }
    let paths = || parsed.operands.iter().map(|p| (*p).to_owned()).collect();
    match sub {
        "update-ref" => Ok(parsed.has_short(&['d']).then(|| {
            Intent::ForceDeleteBranches(
                parsed
                    .operands
                    .first()
                    .and_then(|name| name.strip_prefix("refs/heads/"))
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
            )
        })),
        "reset" => Ok(parsed.has_long("--hard").then_some(Intent::HardReset)),
        "restore" | "checkout" | "switch" => {
            if sub == "restore"
                && (parsed.has_long("--staged") || parsed.has_short(&['S']))
                && !(parsed.has_long("--worktree") || parsed.has_short(&['W']))
            {
                return Ok(None);
            }
            if parsed.has_long("--pathspec-from-file")
                || parsed.has_short(&['p'])
                || parsed.has_long("--patch")
            {
                return Err("pathspec input or interactive restore requires an explicit preview and named paths".into());
            }
            if matches!(sub, "checkout" | "switch")
                && (parsed.has_short(&['f'])
                    || parsed.has_long("--force")
                    || parsed.has_long("--discard-changes"))
            {
                return Ok(Some(Intent::HardReset));
            }
            if sub == "switch"
                || parsed.has_short(&['b', 'B', 'c', 'C'])
                || parsed.has_long("--orphan")
            {
                return Ok(None);
            }
            Ok(Some(
                if sub == "checkout" && !rest.iter().any(|p| p == "--" || p == END_OF_OPTIONS) {
                    Intent::CheckoutPaths(paths())
                } else {
                    Intent::RestorePaths(paths())
                },
            ))
        }
        "clean" => {
            if parsed.has_short(&['n']) || parsed.has_long("--dry-run") {
                return Ok(None);
            }
            // -X removes ignored output only, the accepted AC-2 control.
            if parsed.has_short(&['X']) {
                return Ok(None);
            }
            if parsed.has_short(&['i'])
                || parsed.has_long("--interactive")
                || parsed.has_short(&['e'])
                || parsed.has_long("--exclude")
            {
                return Err("interactive clean or exclusions need a separate dry-run and explicit named paths".into());
            }
            Ok(Some(Intent::CleanNonIgnored {
                paths: paths(),
                directories: parsed.has_short(&['d']),
            }))
        }
        "stash" => Ok(
            matches!(parsed.operands.first().copied(), Some("drop" | "clear"))
                .then_some(Intent::StashDiscard),
        ),
        "branch" => {
            if parsed.has_short(&['r']) || parsed.has_long("--remotes") {
                return Ok(None);
            }
            let force = parsed.has_short(&['D'])
                || ((parsed.has_short(&['d']) || parsed.has_long("--delete"))
                    && (parsed.has_short(&['f']) || parsed.has_long("--force")));
            if force
                && parsed
                    .operands
                    .iter()
                    .any(|s| has_substitution(s) || s.contains('$'))
            {
                return Err(
                    "cannot resolve the branch whose unique work would be discarded".into(),
                );
            }
            Ok(force.then(|| Intent::ForceDeleteBranches(paths())))
        }
        "worktree" => {
            if parsed.operands.first() != Some(&"remove")
                || !(parsed.has_short(&['f']) || parsed.has_long("--force"))
            {
                return Ok(None);
            }
            let path = parsed
                .operands
                .get(1)
                .ok_or("force removal has no resolved worktree path")?;
            Ok(Some(Intent::ForceRemoveWorktree((*path).into())))
        }
        _ => Ok(None),
    }
}

fn discard_violation(level: PolicyLevel, sub: &str, why: &str) -> Violation {
    Violation::new(
        "git.discard_uncommitted",
        level,
        format!("`git {sub}` cannot safely discard local work: {why}"),
        crate::remedy::DISCARD_LOCAL_WORK.remedy(),
    )
}

/// Evaluate every target, with that target's own policy. An unavailable lookup
/// refuses destructive commands; it never manufactures an empty repository.
fn check_discard(
    args: &[String],
    sub: &str,
    rest: &[String],
    moved: &Moves<'_>,
    prior_mutation: bool,
    ctx: &GuardContext<'_>,
    out: &mut Vec<Violation>,
) {
    let intent = discard_intent(sub, rest, moved);
    if matches!(intent, Ok(None)) {
        return;
    }
    let specs = match compose_targets(args, moved) {
        Ok(specs) => specs,
        Err(why) => {
            if ctx.policy.discard_uncommitted.is_active() {
                out.push(discard_violation(ctx.policy.discard_uncommitted, sub, &why));
            }
            return;
        }
    };
    for spec in &specs {
        let target = spec.as_ref().map(|s| Retarget {
            path: &s.path,
            git_dir: s.git_dir,
        });
        let mut unresolved = None;
        let rules = match target {
            None => Cow::Borrowed(ctx.policy),
            Some(target) => {
                if let Some(repo) = lookup(&target, ctx) {
                    repo.policy.map_or(Cow::Borrowed(ctx.policy), Cow::Owned)
                } else {
                    unresolved = Some("target repository or policy is unreadable".to_string());
                    Cow::Borrowed(ctx.policy)
                }
            }
        };
        let level = rules.discard_uncommitted;
        if !level.is_active() {
            continue;
        }
        let result = if let Some(why) = unresolved {
            Err(why)
        } else if prior_mutation {
            Err("an earlier command may change the tree, stash or refs; run the inspection after that command finishes".into())
        } else if moved.config_unknown
            || args
                .iter()
                .take(args.len().saturating_sub(rest.len() + 1))
                .any(|a| {
                    a == "--work-tree"
                        || a.starts_with("--work-tree=")
                        || a == "--bare"
                        || a.contains("core.worktree=")
                        || a.contains("core.bare=")
                        || a.contains("include.path=")
                        || a.contains("includeIf.")
                })
        {
            Err("explicit work-tree or bare repository overrides need an unambiguous git -C invocation".into())
        } else {
            match &intent {
                Err(why) => Err(why.clone()),
                Ok(Some(intent)) => ctx
                    .discard_lookup
                    .ok_or_else(|| "repository-state lookup is unavailable".to_string())
                    .and_then(|probe| probe(&DiscardQuery { target, intent })),
                Ok(None) => Ok(None),
            }
        };
        let why = match result {
            Ok(None) => continue,
            Ok(Some(why)) | Err(why) => why,
        };
        let violation = discard_violation(level, sub, &why);
        if !out.iter().any(|v| {
            v.rule == violation.rule && v.level == violation.level && v.message == violation.message
        }) {
            out.push(violation);
        }
    }
}

/// A pre-call snapshot cannot describe changes made by earlier commands in
/// this same shell call. Only obvious read-only operations preserve the proof.
fn may_change_discard_state(segment: &str) -> bool {
    let mut words = command_argv(segment);
    strip_reserved_words(&mut words);
    let mut tokens = shell_tokens(segment);
    strip_reserved_words(&mut tokens);
    if words.len() != tokens.len() {
        return true;
    }
    let Some((program, args)) = strip_launchers(&words) else {
        return false;
    };
    let name = basename(program);
    // expand_commands emits these bodies separately, in execution order.
    if (is_shell(name) && shell_c_argument(args).is_some()) || name == "eval" {
        return false;
    }
    if name == "git" {
        return git_subcommand(args).is_none_or(|(sub, rest)| !discard_readonly_git(sub, rest));
    }
    !matches!(
        name,
        "cd" | "pushd"
            | "popd"
            | "pwd"
            | "true"
            | "false"
            | ":"
            | "echo"
            | "printf"
            | "cat"
            | "grep"
            | "rg"
            | "ls"
    )
}
fn discard_readonly_git(sub: &str, rest: &[String]) -> bool {
    matches!(
        sub,
        "status"
            | "diff"
            | "log"
            | "show"
            | "rev-parse"
            | "ls-files"
            | "ls-tree"
            | "merge-base"
            | "for-each-ref"
            | "show-ref"
            | "cat-file"
            | "check-ignore"
            | "help"
            | "version"
    ) && !rest
        .iter()
        .any(|p| p == "--output" || p.starts_with("--output="))
}

const DISCARD_RESTORE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('s', Arity::Value),
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('c', Arity::Value),
        ('C', Arity::Value),
    ],
    long: &[
        ("--source", Arity::Value),
        ("--pathspec-from-file", Arity::Value),
        ("--pathspec-file-nul", Arity::Flag),
        ("--orphan", Arity::Value),
        ("--conflict", Arity::Value),
        ("--track", Arity::AttachedValue),
        ("--force", Arity::Flag),
        ("--discard-changes", Arity::Flag),
        ("--patch", Arity::Flag),
        ("--staged", Arity::Flag),
        ("--worktree", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--help", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};
const DISCARD_CLEAN_OPTIONS: OptionSpec = OptionSpec {
    short: &[('e', Arity::Value)],
    long: &[
        ("--exclude", Arity::Value),
        ("--force", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--interactive", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--help", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};
const DISCARD_MISC_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[
        ("--force", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--help", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

#[cfg(test)]
mod discard_integration_tests {
    use super::*;
    use std::path::Path;

    fn context<'a>(policy: &'a GitPolicy, probe: DiscardLookup<'a>) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: "task/local",
            integrate_token: false,
            pr_base_lookup: None,
            dir_target_lookup: None,
            alias_lookup: None,
            discard_lookup: probe,
            root_checkout: None,
        }
    }
    fn discard_only(command: &str, ctx: &GuardContext<'_>) -> Vec<Violation> {
        evaluate(command, ctx)
            .into_iter()
            .filter(|v| v.rule == "git.discard_uncommitted")
            .collect()
    }
    #[test]
    fn parser_recognizes_the_destructive_forms_and_preserves_controls() {
        let policy = GitPolicy::default();
        let dirty = |_: &DiscardQuery<'_>| Ok(Some("fixture local work".into()));
        let ctx = context(&policy, Some(&dirty));
        for command in [
            "git reset --hard",
            "git reset --har",
            "git checkout -- .",
            "git restore .",
            "git restore --source HEAD src",
            "git checkout -f task/other",
            "git switch --discard-changes task/other",
            "git stash drop",
            "git stash clear",
            "git clean -fd",
            "git clean -df",
            "git branch -D task/old",
            "git branch -df task/old",
            "git branch --delete --force task/old",
            "git worktree remove -f sibling",
            "git worktree remove --forc sibling",
            "bash -lc 'git reset --hard'",
            "command git reset --hard",
        ] {
            assert!(!discard_only(command, &ctx).is_empty(), "{command}");
        }
        for command in [
            "git reset --soft HEAD~1",
            "git clean -nd",
            "git clean --dry-run",
            "git clean -fdX target/",
            "git stash pop",
            "git stash list",
            "git worktree list",
            "git branch -r -D origin/task/old",
            "git restore --help",
        ] {
            assert!(discard_only(command, &ctx).is_empty(), "{command}");
        }
    }
    #[test]
    fn absent_or_unreadable_state_does_not_prove_safety() {
        let policy = GitPolicy::default();
        let ctx = context(&policy, None);
        assert!(discard_only("git reset --hard", &ctx)[0]
            .message
            .contains("lookup is unavailable"));
        let unreadable = |_: &DiscardQuery<'_>| Err("status failed".into());
        let ctx = context(&policy, Some(&unreadable));
        assert!(discard_only("git reset --hard", &ctx)[0]
            .message
            .contains("status failed"));
    }
    #[test]
    fn an_earlier_mutation_invalidates_clean_snapshot_proof() {
        let policy = GitPolicy::default();
        let clean = |_: &DiscardQuery<'_>| Ok(None);
        let ctx = context(&policy, Some(&clean));
        for command in [
            "printf changed > src/a && git reset --hard",
            "touch notes; git clean -fd",
            "git commit -m update && git branch -D task/old",
            "git stash push && git stash clear",
            "bash -lc 'touch notes; git clean -fd'",
        ] {
            let found = discard_only(command, &ctx);
            assert!(
                found.iter().any(|v| v.message.contains("earlier command")),
                "{command}: {found:?}"
            );
        }
        for command in [
            "git status && git reset --hard",
            "echo inspecting && git reset --hard",
            "git clean -fdX target/",
        ] {
            assert!(discard_only(command, &ctx).is_empty(), "{command}");
        }
    }
    #[test]
    fn discard_uncertainty_is_protected_on_feature_branches() {
        let policy = GitPolicy::default();
        let clean = |_: &DiscardQuery<'_>| Ok(None);
        let ctx = context(&policy, Some(&clean));
        for command in [
            "git reset \"$(printf -- --hard)\"",
            "git restore \"$UNKNOWN\"",
            "git clean --exclude=keep -fd",
            "git restore --pathspec-from-file=paths",
        ] {
            assert!(!discard_only(command, &ctx).is_empty(), "{command}");
        }
    }
    #[test]
    fn configured_relief_keeps_warn_and_off_semantics() {
        let mut policy = GitPolicy {
            discard_uncommitted: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let ctx = context(&policy, None);
        let found = discard_only("git reset --hard", &ctx);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, PolicyLevel::Warn);
        policy.discard_uncommitted = PolicyLevel::Off;
        assert!(discard_only("git reset --hard", &context(&policy, None)).is_empty());
    }
    // All mutations below are fixture setup in newly created temp repositories.
    fn fixture() -> (tempfile::TempDir, git2::Repository) {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.set_head("refs/heads/task/local").unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/a"), "tracked\n").unwrap();
        std::fs::write(temp.path().join(".gitignore"), "target/\n").unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "test: fixture",
            &repo.find_tree(oid).unwrap(),
            &[],
        )
        .unwrap();
        (temp, repo)
    }
    fn actual(cwd: &Path, command: &str) -> Vec<Violation> {
        let policy = GitPolicy::default();
        let common = git2::Repository::discover(cwd)
            .unwrap()
            .commondir()
            .to_path_buf();
        let target = |s: &Retarget<'_>| read_target(cwd, Some(&common), s);
        let probe =
            |q: &DiscardQuery<'_>| super::super::git_discard::inspect(cwd, q.target, q.intent);
        let alias = |q: &AliasQuery<'_>| read_alias(cwd, q);
        let mut ctx = context(&policy, Some(&probe));
        ctx.dir_target_lookup = Some(&target);
        ctx.alias_lookup = Some(&alias);
        discard_only(command, &ctx)
    }

    #[test]
    fn f12_branch_operands_after_separator_are_not_options() {
        let (temp, _repo) = fixture();
        for command in [r#"git branch -d -- "$name""#, r#"git switch -- "$BRANCH""#] {
            assert!(actual(temp.path(), command).is_empty(), "{command}");
        }
        for command in [
            r#"git branch -d "$name""#,
            r#"git switch "$BRANCH""#,
            r#"git branch -D -- "$name""#,
        ] {
            assert!(!actual(temp.path(), command).is_empty(), "{command}");
        }
    }

    #[test]
    fn f13_unstage_keeps_worktree_changes() {
        let (temp, _repo) = fixture();
        std::fs::write(temp.path().join("src/a"), "dirty").unwrap();
        assert!(actual(temp.path(), "git restore --staged .").is_empty());
        assert!(actual(temp.path(), "git restore -S .").is_empty());
        assert!(!actual(temp.path(), "git restore --staged --worktree .").is_empty());
    }

    #[test]
    fn real_dirty_clean_file_restore_and_alias_pairs() {
        let (temp, repo) = fixture();
        assert!(actual(temp.path(), "git reset --hard").is_empty());
        std::fs::write(temp.path().join("src/a"), "dirty\n").unwrap();
        for command in ["git reset --hard", "git checkout -- .", "git restore src"] {
            assert!(!actual(temp.path(), command).is_empty(), "{command}");
        }
        assert!(actual(temp.path(), "git restore src/a").is_empty());
        assert!(!actual(&temp.path().join("src"), "git restore .").is_empty());
        repo.config()
            .unwrap()
            .set_str("alias.wipe", "reset --hard")
            .unwrap();
        assert!(!actual(temp.path(), "git wipe").is_empty());
        std::fs::remove_dir_all(temp.path().join("src")).unwrap();
        assert!(!actual(temp.path(), "git restore src").is_empty());
    }
    #[test]
    fn retargets_use_actual_tree_and_foreign_policy() {
        let (first, _repo) = fixture();
        let (second, _other) = fixture();
        std::fs::write(second.path().join("src/a"), "dirty\n").unwrap();
        for command in [
            format!("git -C '{}' reset --hard", second.path().display()),
            format!("cd '{}' && git reset --hard", second.path().display()),
        ] {
            assert!(!actual(first.path(), &command).is_empty(), "{command}");
        }
        std::fs::create_dir(second.path().join(".codeflow")).unwrap();
        std::fs::write(
            second.path().join(".codeflow/policy.json"),
            r#"{"schema_version":1,"git":{"discard_uncommitted":"off"}}"#,
        )
        .unwrap();
        // Uncommitted local policy cannot relax the committed authority.
        assert!(!actual(
            first.path(),
            &format!("git -C '{}' reset --hard", second.path().display())
        )
        .is_empty());
    }
}
