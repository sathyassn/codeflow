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
    SUBSTITUTED, SUBSTITUTED_BARE,
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

/// Resolves a branch expression where the command would run (`None` for the
/// session's working directory) to a branch name, or says why it cannot.
pub type BranchLookup<'a> =
    Option<&'a dyn Fn(Option<&Retarget<'_>>, &str) -> Result<String, String>>;

/// Resolve a branch expression the way `git branch`, `git checkout -B` and
/// `git switch -C` read a branch name, by running
/// `git check-ref-format --branch` where the command would run (`target`,
/// relative to `cwd`). It expands `@{-N}` and `<branch>@{upstream}` to the
/// local branch they name; an expression git cannot expand is an error.
///
/// # Errors
///
/// When git cannot run or cannot expand the expression.
pub fn read_branch_name(
    cwd: &std::path::Path,
    target: Option<&Retarget<'_>>,
    name: &str,
) -> Result<String, String> {
    let mut cmd = crate::git::command();
    cmd.current_dir(cwd).stdin(std::process::Stdio::null());
    match target {
        Some(t) if t.git_dir => {
            cmd.arg(format!("--git-dir={}", t.path));
        }
        Some(t) => {
            cmd.arg("-C").arg(t.path);
        }
        None => {}
    }
    cmd.args(["check-ref-format", "--branch", name]);
    match cmd.output() {
        // OS text rule (issue 79): the name is judged against protected
        // branch globs. One that is not valid UTF-8 reads as the sentinel
        // every branch rule treats as protected, as the hook plane does.
        Ok(out) if out.status.success() => Ok(std::str::from_utf8(&out.stdout).map_or_else(
            |_| crate::hooks::policy::NON_UTF8_BRANCH.to_string(),
            |text| text.strip_suffix('\n').unwrap_or(text).to_string(),
        )),
        Ok(out) => Err(format!(
            "`git check-ref-format --branch` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("git could not run: {e}")),
    }
}

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
        // OS text rule (issue 79): the expansion is judged as command words,
        // so one that is not valid UTF-8 is unreadable, which the guard
        // treats as uncertainty and stops, never as a lossy spelling.
        Ok(out) if out.status.success() => match String::from_utf8(out.stdout) {
            Ok(text) => {
                AliasAnswer::Expansion(text.strip_suffix('\n').unwrap_or(&text).to_string())
            }
            Err(_) => AliasAnswer::Unreadable("the alias is not valid UTF-8".to_string()),
        },
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
    let branch = super::repo::current_branch(&repo).ok()?;
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
    let root = if let Some(dir) = repo
        .workdir()
        .filter(|_| crate::root_checkout::is_root_checkout(&repo))
    {
        let authority = super::landed_policy::load(dir).ok()?;
        RootCheckout::read(&repo, &authority.policy.git).ok()?
    } else {
        None
    };
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
/// sends `camelCase` (`toolName`, `toolInput`) with shell tool
/// `run_terminal_command`, and since 1.0.46 each field under both spellings;
/// [`HookPayload::parse`] folds the spellings into one.
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
        let mut value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| PayloadError::Malformed(e.to_string()))?;
        let Some(object) = value.as_object_mut() else {
            return Err(PayloadError::Malformed(format!(
                "a JSON {} where an object belongs",
                json_kind(&value)
            )));
        };
        // One field under two spellings: equal values are one field; values
        // that disagree name two calls, and reading either one could judge a
        // command other than the one the harness runs.
        for (snake, camel) in [("tool_name", "toolName"), ("tool_input", "toolInput")] {
            if let Some(camel_value) = object.remove(camel) {
                match object.get(snake) {
                    Some(snake_value) if *snake_value != camel_value => {
                        return Err(PayloadError::Malformed(format!(
                            "fields `{snake}` and `{camel}` disagree"
                        )));
                    }
                    Some(_) => {}
                    None => {
                        object.insert(snake.to_string(), camel_value);
                    }
                }
            }
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
    /// How to resolve a branch expression (`@{-1}`, `x@{upstream}`) to the
    /// branch git would change in the targeted repository (injected).
    /// `None` leaves every such expression unresolved, which refuses.
    pub branch_lookup: BranchLookup<'a>,
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
    let _facts = super::edit_guard::RepoFactsScope::enter();
    let mut report = Evaluation::default();
    let violations = &mut report.violations;
    // Chained checkout/switch dodges change the branch later segments run on.
    let mut branches = BranchTracker::new(ctx.current_branch);
    branches.line = command.to_string();
    let segments = expand_commands(command);
    // Where each segment sits (TSK-112): on a flat line, `Some(join)` for a
    // top-level command and `None` for one nested in a substitution or a
    // `bash -c` string; `None` for the whole line when it is not flat.
    let roles = flat_top_level(command).and_then(|top| map_top_level(&segments, &top));
    let mut shell = ShellState::new(roles.is_some());
    let line = LineFacts::read(&segments, roles.as_deref(), command);
    let run = run_dirs(&segments, cwd);
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

        // Text nested deeper than the reader follows may hide any command.
        if segment == NESTING_UNREAD {
            if ctx.policy.hook_integrity.is_active() {
                violations.push(hook_integrity_violation(
                    ctx.policy.hook_integrity,
                    format!(
                        "the command nests groups or substitutions more than {NESTING_LIMIT} levels deep, past what the guard reads"
                    ),
                ));
            }
            continue;
        }

        // Hook/policy integrity: writes or removes that would disarm or tamper
        // with the enforcement plane, evaluated on ANY command (not just git).
        let moved = line.moves_for(&shell, top_level, &tokens);
        if let Some(mut v) = integrity_write_in_dirs(
            &tokens,
            &redirect_writes(segment),
            ctx.policy.hook_integrity,
            cwd,
            &moved.cwd,
            command,
            &run,
        ) {
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
    // not a leading prefix, so the positional scan below would miss it. The
    // program is judged after its launchers (`command git config …`,
    // `env X=1 git config …`), so printed text naming git is not.
    let git_config = strip_launchers(tokens).is_some_and(|(program, args)| {
        basename(program) == "git" && args.iter().any(|t| t == "config")
    });
    if git_config {
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
        // `command`/`builtin`/`exec` just prefix another simple command —
        // skip them and their options and re-examine the word they wrap
        // (`command env VAR=…`, `command -p env VAR=…`).
        if is_prefix_launcher(t) {
            match skip_launcher_options(t, tokens, idx + 1) {
                Some(next) => {
                    idx = next;
                    continue;
                }
                None => break,
            }
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
        .cloned()
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
    redirects: &Redirects,
    level: PolicyLevel,
    cwd: &Path,
    dirs: &Cwd,
    line: &str,
    run: &RunDirs,
) -> Option<Violation> {
    if let Some(v) = dot_glob_option_violation(tokens, redirects, level, line) {
        return Some(v);
    }
    match dirs {
        Cwd::Paths(dirs) => dirs.iter().find_map(|dir| {
            integrity_write_violation(tokens, redirects, level, &cwd.join(dir), cwd, line)
        }),
        Cwd::Unknown(_) => integrity_write_in_run(tokens, redirects, level, run, cwd, line),
    }
}

/// A line that turns on a shell option making patterns match names that
/// start with `.` (Bash `dotglob` or `GLOBIGNORE`, zsh `globdots`), with a
/// command that is not proven to only read and has a pattern among its
/// words or write targets: the guard reads patterns with the default
/// options, so it refuses (TSK-216 round 16).
fn dot_glob_option_violation(
    tokens: &[String],
    redirects: &Redirects,
    level: PolicyLevel,
    line: &str,
) -> Option<Violation> {
    let folded: String = line
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_lowercase();
    let option = ["dotglob", "globignore", "globdots"]
        .into_iter()
        .find(|option| folded.contains(option))?;
    if read_only_program(tokens) {
        return None;
    }
    let patterned = tokens.iter().skip(1).any(|t| has_glob(t))
        || words_of(&redirects.targets).iter().any(|t| has_glob(t))
        || redirects.unread.iter().any(|t| has_glob(t));
    patterned.then(|| {
        hook_integrity_violation(
            level,
            format!(
                "the line turns on `{option}`, which lets a pattern match names that start with `.`, and the guard reads patterns with the default options"
            ),
        )
    })
}

/// Programs that can change the paths they are given, directly or through
/// the command they run.
const WRITING_PROGRAMS: &[&str] = &[
    "rm",
    "unlink",
    "rmdir",
    "mv",
    "cp",
    "ln",
    "tee",
    "truncate",
    "shred",
    "chmod",
    "chown",
    "chgrp",
    "chflags",
    "install",
    "dd",
    "rsync",
    "sed",
    "find",
    "xargs",
    "parallel",
    "trash",
    "trash-put",
];

/// A word as the guard shows it in a message: a command substitution the
/// tokenizer cut out reads as `$(...)`.
fn shown_word(word: &str) -> String {
    word.replace([git_target::SUBSTITUTED, SUBSTITUTED_BARE], "$(...)")
}

/// The words a list of arguments holds, each split at blanks and shell
/// operators as the file system reads them, so a `sed` script's `w FILE`
/// yields `FILE`.
fn words_of<'a>(args: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    args.into_iter()
        .flat_map(|arg| {
            let text = canonical_text(arg);
            line_words(&text).map(str::to_string).collect::<Vec<_>>()
        })
        .collect()
}

/// What a command does to paths when the guard cannot tell the directory
/// it runs in (TSK-216 round 6).
enum UnknownDirUse {
    /// It only reads: a `find` whose actions change nothing, an `xargs` or
    /// `parallel` running a read-only program, or a program that writes
    /// no paths it is given.
    Reads,
    /// It can write, and these words could name what it writes.
    Words(Vec<String>),
    /// It can write through text the guard cannot see from here.
    Uncertain(String),
}

/// Classify a command by what it changes before its words are read by
/// name, with the same readings the checks from a known directory use:
/// the `find` action checks, the read-only programs `xargs` and `parallel`
/// run, and the `sed` read clearance.
fn unknown_dir_use(name: &str, args: &[String], line: &str) -> UnknownDirUse {
    let line_words_all = || {
        let text = canonical_text(line);
        line_words(&text).map(str::to_string).collect::<Vec<_>>()
    };
    match name {
        "find" if !find_mutates(args) => UnknownDirUse::Reads,
        "find" => UnknownDirUse::Words(line_words_all()),
        "xargs" => match xargs_command(args) {
            Some(command) if !read_only_program(command) => UnknownDirUse::Words(line_words_all()),
            _ => UnknownDirUse::Reads,
        },
        "parallel" => match args.iter().position(|arg| !arg.starts_with('-')) {
            Some(start) if read_only_program(&args[start..]) => UnknownDirUse::Reads,
            _ => UnknownDirUse::Words(line_words_all()),
        },
        "sed" => sed_unknown_dir_use(args, line),
        _ if WRITING_PROGRAMS.contains(&name) => UnknownDirUse::Words(words_of(args)),
        _ => UnknownDirUse::Reads,
    }
}

/// A `sed` from an unknown directory: the files it only reads are cleared
/// as from anywhere else; its script, in-place files and option values are
/// read by name, a script from its input by the rest of the line, and a
/// relative script file it cannot see is uncertain.
fn sed_unknown_dir_use(args: &[String], line: &str) -> UnknownDirUse {
    let reads = sed_read_flags(args);
    let mut words = words_of(
        args.iter()
            .zip(&reads)
            .filter(|(_, r)| !**r)
            .map(|(a, _)| a),
    );
    for spec in SED_GRAMMARS {
        for file in parse_options(args, spec).values_of('f', "--file") {
            if matches!(file, "-" | "/dev/stdin") {
                let elsewhere = line_without_reads(line, args, &reads);
                words.extend(words_of(std::iter::once(&elsewhere)));
            } else if !Path::new(file).is_absolute() {
                return UnknownDirUse::Uncertain(format!(
                    "its script file `{file}` lies where the guard cannot read it"
                ));
            }
        }
    }
    UnknownDirUse::Words(words)
}

/// Which arguments of a `sed` are files it only reads, by position
/// (TSK-216 round 8): without `-i`, an operand both grammars read as an
/// input file. A redirection and its target (`<<< TEXT`, `< FILE`, a
/// heredoc operator) is never one: it is shell text, judged whole.
fn sed_read_flags(args: &[String]) -> Vec<bool> {
    let in_place = requests_in_place(args);
    let gnu = sed_operands_in(args, &SED_OPTIONS);
    let bsd = sed_operands_in(args, &BSD_SED_OPTIONS);
    let operand_in = |arg: &String, operands: &[&str]| {
        operands
            .iter()
            .any(|op| std::ptr::eq(op.as_ptr(), arg.as_ptr()) && op.len() == arg.len())
    };
    let mut redirected = vec![false; args.len()];
    for (at, arg) in args.iter().enumerate() {
        if let Some(len) = redirect_operator_len(arg) {
            redirected[at] = true;
            if len == arg.len() {
                if let Some(target) = redirected.get_mut(at + 1) {
                    *target = true;
                }
            }
        }
    }
    args.iter()
        .enumerate()
        .map(|(at, arg)| {
            !in_place && !redirected[at] && operand_in(arg, &gnu) && operand_in(arg, &bsd)
        })
        .collect()
}

/// The command line a `sed` script from its input can come from, without
/// the files this `sed` only reads (TSK-216 round 8). The line is judged
/// command by command: every other command, producers, heredoc bodies and
/// here-strings included, is kept whole. In the one command whose argument
/// list is this `sed`'s own, the words at the read operands' positions are
/// dropped; nothing is searched for by text. When no command, or more than
/// one, has that argument list, the position cannot be established and
/// the whole line is kept.
fn line_without_reads(line: &str, args: &[String], reads: &[bool]) -> String {
    let segments = expand_commands(line);
    let mut own = Vec::new();
    for (at, segment) in segments.iter().enumerate() {
        let mut tokens = shell_tokens(segment);
        strip_reserved_words(&mut tokens);
        let matches = strip_launchers(&tokens)
            .is_some_and(|(program, rest)| basename(program) == "sed" && rest == args);
        if matches {
            own.push((at, tokens));
        }
    }
    let [(own_at, tokens)] = own.as_slice() else {
        return line.to_string();
    };
    let start = tokens.len() - args.len();
    segments
        .iter()
        .enumerate()
        .map(|(at, segment)| {
            if at == *own_at {
                tokens
                    .iter()
                    .enumerate()
                    .filter(|(pos, _)| *pos < start || !reads[pos - start])
                    .map(|(_, token)| token.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                segment.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the guard cannot list every directory a command can run in, a
/// path or glob judged from the directories it knows proves nothing
/// (TSK-216 rounds 4 to 6). A command that changes files is refused when a
/// word that could name what it writes could name an enforcement path from
/// some directory (`cd "$d" && rm policy.json`), and so is a write
/// redirect whose target could (`cd "$d" && printf x > policy.json`,
/// TSK-216 rounds 10 and 11); a command proven to only read passes
/// (`cd "$d" && sed -n p policy.json`).
fn unknown_dir_name_violation(
    tokens: &[String],
    redirects: &Redirects,
    level: PolicyLevel,
    line: &str,
    why: &str,
) -> Option<Violation> {
    let why = shown_word(why);
    let redirected = words_of(&redirects.targets);
    if let Some((target, p)) = redirected
        .iter()
        .find_map(|w| word_could_name(w).map(|p| (w, p)))
    {
        return Some(hook_integrity_violation(
            level,
            format!(
                "a redirect runs where the guard cannot tell the directory ({why}), and `{}` could name `{p}` from there",
                shown_word(target)
            ),
        ));
    }
    let (program, args) = strip_launchers(tokens)?;
    let name = basename(program);
    let words = match unknown_dir_use(name, args, line) {
        UnknownDirUse::Reads => return None,
        UnknownDirUse::Uncertain(what) => {
            return Some(hook_integrity_violation(
                level,
                format!(
                    "`{name}` runs where the guard cannot tell the directory ({why}), and {what}"
                ),
            ))
        }
        UnknownDirUse::Words(words) => words,
    };
    let (word, p) = words
        .iter()
        .find_map(|w| word_could_name(w).map(|p| (w, p)))?;
    Some(hook_integrity_violation(
        level,
        format!(
            "`{name}` runs where the guard cannot tell the directory ({why}), and `{}` could name `{p}` from there",
            shown_word(word)
        ),
    ))
}

/// The integrity path a single argument token names, when any. The token is
/// normalized first so equivalent spellings match. An empty token names no
/// path: the commands read `''` as a missing file, never as the cwd.
fn token_integrity_path(token: &str, cwd: &Path, payload_cwd: &Path) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }
    // Each word brace expansion can make is judged (TSK-216 round 16).
    let Some(words) = word_readings(token) else {
        return Some(BRACE_UNREAD);
    };
    words
        .iter()
        .find_map(|word| token_integrity_path_spelled(word, cwd, payload_cwd))
}

/// [`token_integrity_path`] for one word after brace expansion.
fn token_integrity_path_spelled(
    token: &str,
    cwd: &Path,
    payload_cwd: &Path,
) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }
    // A directory the guard cannot resolve (`~-`, `~user`, a value filled
    // in at run time): what follows it is read by name, and a last part
    // filled in at run time counts inside an enforcement directory it
    // follows (`alias/$x` with `alias` linked to `.codeflow`).
    if let Some(rest) = unknown_tilde_rest(token) {
        return word_could_name(rest).or_else(|| rest.is_empty().then_some(BRACE_UNREAD_DIR));
    }
    if let Some(tail) = unresolved_tail(token) {
        if let Some(p) = word_could_name(tail) {
            return Some(p);
        }
        if tail.is_empty() {
            let cut = token
                .rfind(['$', '`', SUBSTITUTED, SUBSTITUTED_BARE])
                .unwrap_or(0);
            if let Some(slash) = token[..cut].rfind('/') {
                let dir = &token[..slash];
                if !dir.is_empty()
                    && unresolved_tail(dir).is_none()
                    && in_enforcement_dir(&integrity_shell_path(dir, cwd))
                {
                    return Some("repository enforcement files");
                }
            }
        }
    }
    token_integrity_path_literal(token, cwd, payload_cwd).or_else(|| {
        // A glob is expanded as the shell will, and each path it reaches is
        // judged through symbolic links (TSK-216 round 4).
        (has_glob(token) && glob_reach(token, cwd, payload_cwd).is_some())
            .then_some("repository enforcement files")
    })
}

/// Whether `dir`, resolved through symbolic links, is or lies in one of its
/// repository's enforcement directories (`.codeflow`, `.claude`, `.git`,
/// `.github`, `.codex`, `.grok`), where any entry may be an enforcement
/// file.
fn in_enforcement_dir(dir: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(dir) else {
        return false;
    };
    let Some(root) = git2::Repository::discover(&real)
        .ok()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf))
        .and_then(|root| std::fs::canonicalize(root).ok())
    else {
        return false;
    };
    real.strip_prefix(&root).is_ok_and(|inside| {
        inside.components().next().is_some_and(|first| {
            let name = first.as_os_str().to_string_lossy().to_lowercase();
            matches!(
                name.as_str(),
                ".codeflow" | ".claude" | ".git" | ".github" | ".codex" | ".grok"
            )
        })
    })
}

/// What a recursive change of a directory the guard cannot resolve is
/// reported as.
const BRACE_UNREAD_DIR: &str =
    "repository enforcement files (a directory the guard cannot resolve)";

/// The integrity path a file-system path (a glob expansion or a `find`
/// candidate) reaches. Text is read as the token it spells. A path that is not
/// valid UTF-8 has no honest token (its storage key holds a NUL and resolves
/// to nothing), so the file-system checks run on the exact path, links
/// followed (OS text rule, issue 79).
fn path_integrity(path: &Path, cwd: &Path, payload_cwd: &Path) -> Option<&'static str> {
    let shown = crate::portable_path::slashed(path);
    if crate::git::key_is_text(&shown) {
        return token_integrity_path_literal(&shown, cwd, payload_cwd);
    }
    let exact = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    if super::edit_guard::repository_authority_target(&exact, payload_cwd, true) {
        return Some(super::edit_guard::AUTHORITY_PATH);
    }
    match super::edit_guard::repository_enforcement_target(&exact, payload_cwd, true) {
        Ok(true) => Some("repository enforcement files"),
        Ok(false) => None,
        Err(_) => Some("repository enforcement paths (cannot read repository state)"),
    }
}

/// [`token_integrity_path`] for the token as written, without expanding a
/// glob in it.
fn token_integrity_path_literal(
    token: &str,
    cwd: &Path,
    payload_cwd: &Path,
) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }
    let path = integrity_shell_path(token, cwd);
    if super::edit_guard::repository_authority_target(&path, payload_cwd, true) {
        return Some(super::edit_guard::AUTHORITY_PATH);
    }
    match super::edit_guard::repository_enforcement_target(&path, payload_cwd, true) {
        Ok(true) => return Some("repository enforcement files"),
        Ok(false) => {}
        Err(_) => return Some("repository enforcement paths (cannot read repository state)"),
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
        integrity_target(&normalize_path(&crate::portable_path::slashed(target)))
    })
}

/// Whether a word holds pattern syntax some shell reads at run time: Bash's
/// `*`, `?` and `[`, and the forms the guard reads conservatively
/// ([`conservative_glob`]).
fn has_glob(word: &str) -> bool {
    word.contains(['*', '?', '[']) || conservative_glob(word)
}

/// Whether a word holds pattern syntax the guard reads conservatively,
/// since a harness may run zsh or turn on extended patterns (TSK-216 round
/// 17): `(` or `)` (extglob groups, zsh groups, alternation and glob
/// qualifiers, which can admit names that start with `.` and apply to
/// every component, as `*/policy.json(D)` does), `^` and `#` (zsh extended
/// globs), a zsh numeric range `<n-m>` (zsh reads any other `<` or `>` as a
/// redirection, which ends the word), and `**` (any depth). Such a word
/// matches every path below its longest literal directory prefix, at
/// every depth, names that start with `.` included ([`WordGlob`]). zsh's
/// exclusion `pat~other` matches only names `pat` matches, so a `~` after
/// the first character is read by the part before it instead
/// ([`word_readings`]); a Windows short name such as `RUNNER~1` stays a
/// plain name (TSK-216 round 18).
fn conservative_glob(word: &str) -> bool {
    word.contains(['(', ')', '^', '#']) || word.contains("**") || numeric_range(word)
}

/// Whether a word holds a zsh numeric range glob: `<`, optional digits,
/// `-`, optional digits, `>` (`<->`, `<1-9>`).
fn numeric_range(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    (0..chars.len()).any(|at| numeric_range_len(&chars[at..]).is_some())
}

/// The length of the zsh numeric range glob `chars` starts with, when it
/// starts with one. zsh reads it as part of the word, never as a
/// redirection (TSK-216 round 17).
fn numeric_range_len(chars: &[char]) -> Option<usize> {
    if chars.first() != Some(&'<') {
        return None;
    }
    let digits = |from: usize| {
        chars[from..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let dash = 1 + digits(1);
    if chars.get(dash) != Some(&'-') {
        return None;
    }
    let close = dash + 1 + digits(dash + 1);
    (chars.get(close) == Some(&'>')).then_some(close + 1)
}

/// Every word the shell can make of `word` before pathname expansion: each
/// word of its brace expansion ([`brace_words`]), and for one with a `~`
/// after its first character, also the part before that `~`, since zsh's
/// exclusion `pat~other` matches only names `pat` matches (TSK-216 round
/// 18). `None` when the braces are too many to read.
fn word_readings(word: &str) -> Option<Vec<String>> {
    let mut words = brace_words(word)?;
    let excluded: Vec<String> = words
        .iter()
        .filter_map(|w| {
            w.char_indices()
                .skip(1)
                .find(|&(_, c)| c == '~')
                .map(|(at, _)| w[..at].to_string())
        })
        .filter(|w| !w.is_empty())
        .collect();
    words.extend(excluded);
    Some(words)
}

/// The most words one brace expansion yields before the guard stops
/// reading it and refuses instead.
const BRACE_WORD_LIMIT: usize = 64;

/// The most `{`, `}` and `,` characters a word may hold for the guard to
/// read its braces; each may have been quoted or escaped (see
/// [`brace_words`]).
const BRACE_CHAR_LIMIT: usize = 10;

/// What a word with braces that the guard does not read is reported as.
const BRACE_UNREAD: &str = "repository enforcement files (a brace expansion too large to read)";

/// Every word the shell's brace expansion can make of `word`, the word
/// itself included; `None` when there are too many to read, and the caller
/// refuses (TSK-216 round 16). The command reader removes quotes and
/// escapes before the guard sees a word, so any `{`, `}` or `,` may have
/// been literal: the words of every such reading are kept, which can only
/// add words. A comma list or a sequence (`{1..3}`, `{a..e..2}`) expands,
/// nested groups included; `${...}` is a parameter, not a brace group. A
/// sequence longer than [`BRACE_WORD_LIMIT`] becomes `*`, which matches at
/// least every word it yields.
fn brace_words(word: &str) -> Option<Vec<String>> {
    if !word.contains('{') {
        return Some(vec![word.to_string()]);
    }
    let marks: Vec<usize> = word
        .char_indices()
        .filter(|(_, c)| matches!(c, '{' | '}' | ','))
        .map(|(at, _)| at)
        .collect();
    if marks.len() > BRACE_CHAR_LIMIT {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for literal in 0..1_u32 << marks.len() {
        let reading: String = word
            .char_indices()
            .map(|(at, c)| match marks.iter().position(|&m| m == at) {
                Some(k) if literal & (1 << k) != 0 => literal_brace_char(c),
                _ => c,
            })
            .collect();
        for expanded in brace_expand(&reading)? {
            let expanded: String = expanded.chars().map(plain_brace_char).collect();
            if !out.contains(&expanded) {
                if out.len() >= BRACE_WORD_LIMIT * 4 {
                    return None;
                }
                out.push(expanded);
            }
        }
    }
    Some(out)
}

/// A stand-in for a quoted or escaped brace character, which brace
/// expansion passes through as text.
fn literal_brace_char(c: char) -> char {
    match c {
        '{' => '\u{E000}',
        '}' => '\u{E001}',
        _ => '\u{E002}',
    }
}

fn plain_brace_char(c: char) -> char {
    match c {
        '\u{E000}' => '{',
        '\u{E001}' => '}',
        '\u{E002}' => ',',
        c => c,
    }
}

/// Brace expansion of a word whose `{`, `}` and `,` all count, as Bash
/// reads them: the first group from the left expands, then each result in
/// turn. `None` past [`BRACE_WORD_LIMIT`] words.
fn brace_expand(word: &str) -> Option<Vec<String>> {
    fn into(word: &str, out: &mut Vec<String>) -> Option<()> {
        let Some((open, close, alternatives)) = brace_group(word) else {
            if out.len() >= BRACE_WORD_LIMIT {
                return None;
            }
            out.push(word.to_string());
            return Some(());
        };
        for alternative in alternatives {
            into(
                &[&word[..open], alternative.as_str(), &word[close + 1..]].concat(),
                out,
            )?;
        }
        Some(())
    }
    let mut out = Vec::new();
    into(word, &mut out)?;
    Some(out)
}

/// The first brace group in `word` from the left: where it opens and
/// closes, and its alternatives. A `{` whose group holds neither a
/// top-level comma nor a sequence is text, and so is `${...}`.
fn brace_group(word: &str) -> Option<(usize, usize, Vec<String>)> {
    let bytes = word.as_bytes();
    let matching = |open: usize| {
        let mut depth = 0_usize;
        for (at, b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(at);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'{' {
            at += 1;
            continue;
        }
        let close = matching(at);
        if at > 0 && bytes[at - 1] == b'$' {
            at = close.map_or(at + 1, |c| c + 1);
            continue;
        }
        if let Some(close) = close {
            let inner = &word[at + 1..close];
            let mut pieces = Vec::new();
            let (mut depth, mut start) = (0_usize, 0);
            for (k, b) in inner.bytes().enumerate() {
                match b {
                    b'{' => depth += 1,
                    b'}' => depth = depth.saturating_sub(1),
                    b',' if depth == 0 => {
                        pieces.push(inner[start..k].to_string());
                        start = k + 1;
                    }
                    _ => {}
                }
            }
            if !pieces.is_empty() {
                pieces.push(inner[start..].to_string());
                return Some((at, close, pieces));
            }
            if let Some(sequence) = brace_sequence(inner) {
                return Some((at, close, sequence));
            }
        }
        at += 1;
    }
    None
}

/// The words of a sequence expression (`1..5`, `05..10..2`, `a..e`), or
/// `None` when `inner` is not one. A sequence of more than
/// [`BRACE_WORD_LIMIT`] words is `*`.
fn brace_sequence(inner: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = inner.split("..").collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let step = match parts.get(2) {
        Some(step) => step.parse::<i64>().ok()?.unsigned_abs().max(1),
        None => 1,
    };
    let as_letter = |s: &str| {
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphabetic() => Some(u32::from(c)),
            _ => None,
        }
    };
    let (words, padded): (Vec<i64>, Option<usize>) = if let (Ok(from), Ok(to)) =
        (parts[0].parse::<i64>(), parts[1].parse::<i64>())
    {
        let zero = |s: &str| s.trim_start_matches(['-', '+']).starts_with('0') && s.len() > 1;
        let width = (zero(parts[0]) || zero(parts[1])).then(|| parts[0].len().max(parts[1].len()));
        (vec![from, to], width)
    } else {
        let from = as_letter(parts[0])?;
        let to = as_letter(parts[1])?;
        (vec![i64::from(from), i64::from(to)], None)
    };
    let (from, to) = (words[0], words[1]);
    let count = from.abs_diff(to) / step + 1;
    if count > BRACE_WORD_LIMIT as u64 {
        return Some(vec!["*".to_string()]);
    }
    let letters = padded.is_none() && parts[0].parse::<i64>().is_err();
    let mut out = Vec::new();
    let mut value = from;
    for _ in 0..count {
        out.push(if letters {
            u32::try_from(value)
                .ok()
                .and_then(char::from_u32)
                .map_or_else(String::new, String::from)
        } else if let Some(width) = padded {
            if value < 0 {
                format!("-{:0>width$}", value.unsigned_abs(), width = width - 1)
            } else {
                format!("{value:0>width$}")
            }
        } else {
            value.to_string()
        });
        let step = i64::try_from(step).unwrap_or(1);
        value += if to >= from { step } else { -step };
    }
    Some(out)
}

/// The part of a word after the last character the shell fills in at run
/// time (`$`, a backquote or a command substitution), when one is there:
/// the words after it are spelled on the line, and the guard reads them by
/// their names alone, as from a directory filled in at run time (TSK-216
/// round 16). `Some("")` when the filled-in part is the last component.
fn unresolved_tail(word: &str) -> Option<&str> {
    let cut = word.rfind(['$', '`', SUBSTITUTED, SUBSTITUTED_BARE])?;
    Some(word[cut..].split_once('/').map_or("", |(_, tail)| tail))
}

/// A tilde prefix the guard cannot resolve to a directory (`~-`, `~user`,
/// `~1`, `~-1`): the rest of the word after it, read by name. `~`, `~/`
/// and `~+` resolve and are not unknown.
fn unknown_tilde_rest(word: &str) -> Option<&str> {
    let rest = word.strip_prefix('~')?;
    let (head, tail) = rest.split_once('/').unwrap_or((rest, ""));
    (!head.is_empty() && head != "+").then_some(tail)
}

/// The most directory entries one glob expansion reads before it stops. It
/// bounds the guard's work: a glob over a larger tree is judged by the
/// directory it starts from instead (see [`glob_reach`]).
const GLOB_ENTRY_LIMIT: usize = 4096;

/// Why a glob expansion stopped before it read every entry.
#[derive(Debug, PartialEq, Eq)]
enum GlobStop {
    /// It would read more than [`GLOB_ENTRY_LIMIT`] entries.
    TooManyEntries,
}

/// One file-name component of a shell pattern, read so that it matches at
/// least every name the shell would (TSK-216 rounds 13 to 15): the guard's
/// patterns only ever over-approximate.
///
/// - A run of `*` is one `*`, and `?` is any one character.
/// - Outside a bracket expression, a backslash makes the next character
///   literal, as the shell reads it; a trailing backslash is literal.
/// - A bracket expression whose members are only ASCII letters, digits,
///   `.` and `_` keeps its members, which is exactly what the shell
///   matches (`.[ab]*`), when no backslash follows its `[` and no other
///   `]` follows its close. The command reader removes escapes before
///   this reader sees a word, so `[p\\]]` arrives as `[p]]`; a later `]`
///   may be where the shell closes the expression.
/// - Any other `[` with a `]` somewhere after it makes the whole component
///   match every name (`*`): a negation, range, class, equivalence class,
///   collating symbol, nested `[`, backslash, or a close that a class or
///   escape could move ([`bracket_end`]). That can only refuse more, and
///   only for unusual patterns.
/// - A `[` with no `]` after it, and a stray `]`, are literal characters,
///   so colour output such as `e[32mhello` stays literal.
/// - Extended pattern syntax some shells read (`(`, `|`, `^`, `#`, `~`)
///   makes the whole component match every name (TSK-216 round 16).
///
/// The pattern always compiles, so no text becomes an accidental
/// match-everything glob.
fn shell_pattern(text: &str) -> glob::Pattern {
    let chars: Vec<char> = text.chars().collect();
    let every_name = || glob::Pattern::new("*").unwrap_or_default();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                while chars.get(i + 1) == Some(&'*') {
                    i += 1;
                }
                out.push('*');
            }
            '\\' => match chars.get(i + 1) {
                Some(next) => {
                    out.push_str(&glob::Pattern::escape(&next.to_string()));
                    i += 1;
                }
                None => out.push_str(&glob::Pattern::escape("\\")),
            },
            '[' => {
                if !chars[i + 1..].contains(&']') {
                    out.push_str("[[]");
                    i += 1;
                    continue;
                }
                let end = bracket_end(&chars, i, false);
                let plain = end.filter(|&end| {
                    let members = &chars[i + 1..end];
                    bracket_end(&chars, i, true) == Some(end)
                        && !chars[i..].contains(&'\\')
                        && !chars[end + 1..].contains(&']')
                        && !members.is_empty()
                        && members
                            .iter()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
                });
                let Some(end) = plain else {
                    return every_name();
                };
                out.push('[');
                out.extend(&chars[i + 1..end]);
                out.push(']');
                i = end;
            }
            ']' => out.push_str("[]]"),
            // Extended pattern syntax (extglob groups, zsh alternation,
            // qualifiers, `^`, `#` and `~`) matches every name.
            '(' | ')' | '|' | '^' | '#' | '~' => return every_name(),
            c => out.push(c),
        }
        i += 1;
    }
    glob::Pattern::new(&out).unwrap_or_else(|_| every_name())
}

/// Where the bracket expression opened at `chars[start]` (a `[`) closes,
/// read the POSIX way: after an optional `!` or `^`, a `]` in first
/// position is a member; `[:name:]`, `[=x=]` and `[.x.]` are whole units,
/// so the `]` inside one never closes the expression; the expression
/// closes at the first `]` after that. With `escapes`, a backslash also
/// makes the next character a plain member, as Bash reads it; without,
/// it is a member itself, as POSIX reads it. `None` when it never closes.
fn bracket_end(chars: &[char], start: usize, escapes: bool) -> Option<usize> {
    let mut at = start + 1;
    if matches!(chars.get(at), Some('!' | '^')) {
        at += 1;
    }
    if chars.get(at) == Some(&']') {
        at += 1;
    }
    while at < chars.len() {
        match chars[at] {
            ']' => return Some(at),
            '\\' if escapes => at += 2,
            '[' if matches!(chars.get(at + 1), Some(':' | '=' | '.')) => {
                let kind = chars[at + 1];
                let close = (at + 2..chars.len().saturating_sub(1))
                    .find(|&k| chars[k] == kind && chars[k + 1] == ']');
                at = close.map_or(at + 1, |k| k + 2);
            }
            _ => at += 1,
        }
    }
    None
}

/// A shell word read as a path pattern from a directory (TSK-216 round
/// 18): the path it names, how many leading components come from that
/// directory or the home directory and so are never read as pattern syntax
/// (a Windows short name such as `RUNNER~1`, or a `(` in a folder name,
/// stays literal there), and whether the word's own text holds syntax read
/// conservatively ([`conservative_glob`]).
struct WordGlob {
    pattern: PathBuf,
    literal: usize,
    conservative: bool,
}

impl WordGlob {
    fn new(word: &str, cwd: &Path) -> Self {
        let pattern = integrity_shell_path(word, cwd);
        let own = if word == "~" || word == "~+" {
            ""
        } else if let Some(rest) = word.strip_prefix("~+/").or_else(|| word.strip_prefix("~/")) {
            rest
        } else if word.starts_with('$') {
            word.split_once('/').map_or("", |(_, rest)| rest)
        } else {
            word
        };
        let own_parts = Path::new(own)
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .count();
        Self {
            literal: pattern.components().count().saturating_sub(own_parts),
            pattern,
            conservative: conservative_glob(own),
        }
    }

    /// The literal directory the pattern starts from: its components before
    /// the first one of the word's own with pattern syntax.
    fn prefix(&self) -> PathBuf {
        self.pattern
            .components()
            .enumerate()
            .take_while(|(at, c)| *at < self.literal || !has_glob(&c.as_os_str().to_string_lossy()))
            .map(|(_, c)| c)
            .collect()
    }

    /// Expand the pattern over the file system, as the shell would before
    /// the command runs. A component of the word with pattern syntax
    /// ([`has_glob`]) is read by [`shell_pattern`], and a name starting with
    /// `.` matches only a component that starts with `.`; matching ignores
    /// case. A word read conservatively matches every path below
    /// [`Self::prefix`], at every depth, names that start with `.` and the
    /// entries of linked directories included, and each wild component then
    /// matches every name, so a `..` after one is followed too (TSK-216
    /// round 17). A name that is not UTF-8 cannot be matched as text, so it
    /// counts as a match of any wildcard component: the result
    /// over-approximates and never panics (TSK-216 round 4). Paths are
    /// joined as written, so a symbolic link in them is resolved by the
    /// caller's file-system-aware check.
    fn expand(&self) -> Result<Vec<PathBuf>, GlobStop> {
        let mut read = 0;
        if !self.conservative {
            return expand_components(&self.pattern, self.literal, false, &mut read);
        }
        let prefix = self.prefix();
        let mut found = every_path_below(&prefix, &mut read)?;
        if self
            .pattern
            .components()
            .skip(prefix.components().count())
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            found.extend(expand_components(
                &self.pattern,
                self.literal,
                true,
                &mut read,
            )?);
        }
        Ok(found)
    }
}

/// Every path at or below `dir`, at every depth: names that start with `.`
/// are included and linked directories are entered, each real directory
/// once.
fn every_path_below(dir: &Path, read: &mut usize) -> Result<Vec<PathBuf>, GlobStop> {
    let mut found = vec![dir.to_path_buf()];
    let mut pending = vec![dir.to_path_buf()];
    let mut entered = std::collections::HashSet::new();
    while let Some(dir) = pending.pop() {
        if let Ok(real) = std::fs::canonicalize(&dir) {
            if !entered.insert(real) {
                continue;
            }
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            *read += 1;
            if *read > GLOB_ENTRY_LIMIT {
                return Err(GlobStop::TooManyEntries);
            }
            let path = dir.join(entry.file_name());
            if path.is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    Ok(found)
}

/// [`WordGlob::expand`] one component at a time; the first `literal`
/// components are never wild. `**` is zero or more directories. With
/// `every_name`, each wild component matches every name, names that start
/// with `.` included.
fn expand_components(
    pattern: &Path,
    literal: usize,
    every_name: bool,
    read: &mut usize,
) -> Result<Vec<PathBuf>, GlobStop> {
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let mut current: Vec<PathBuf> = vec![PathBuf::new()];
    for (at, component) in pattern.components().enumerate() {
        let text = component.as_os_str().to_string_lossy();
        let wild = at >= literal
            && matches!(component, std::path::Component::Normal(_))
            && has_glob(&text);
        if !wild {
            for path in &mut current {
                path.push(component);
            }
            continue;
        }
        if text.len() > 1 && text.chars().all(|c| c == '*') {
            // `**`: zero or more directories, as zsh reads it by default
            // and Bash with `globstar` (TSK-216 round 16).
            let mut next = current.clone();
            let mut pending = current.clone();
            while let Some(dir) = pending.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    *read += 1;
                    if *read > GLOB_ENTRY_LIMIT {
                        return Err(GlobStop::TooManyEntries);
                    }
                    let name = entry.file_name();
                    if !every_name && name.to_str().is_some_and(|n| n.starts_with('.')) {
                        continue;
                    }
                    let path = dir.join(&name);
                    if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        pending.push(path.clone());
                    }
                    next.push(path);
                }
            }
            current = next;
            continue;
        }
        let matcher = shell_pattern(&text);
        let dotted = every_name || text.starts_with('.');
        let mut next = Vec::new();
        for dir in &current {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                *read += 1;
                if *read > GLOB_ENTRY_LIMIT {
                    return Err(GlobStop::TooManyEntries);
                }
                let name = entry.file_name();
                let keep = every_name
                    || match name.to_str() {
                        Some(text) => {
                            (dotted || !text.starts_with('.'))
                                && matcher.matches_with(text, options)
                        }
                        None => true,
                    };
                if keep {
                    next.push(dir.join(&name));
                }
            }
        }
        current = next;
    }
    current.retain(|path| std::fs::symlink_metadata(path).is_ok());
    Ok(current)
}

/// What a glob word reaches from `cwd`, judged by the file-system-aware
/// checks: the enforcement path or registered worktree one of its
/// expansions resolves to, symbolic links followed. When the expansion
/// stops, the directory it starts from decides: one that holds or lies in
/// enforcement files or a registered worktree counts as reached.
fn glob_reach(word: &str, cwd: &Path, payload_cwd: &Path) -> Option<String> {
    let glob = WordGlob::new(word, cwd);
    let reached = |path: &Path| {
        path_integrity(path, cwd, payload_cwd)
            .map(str::to_string)
            .or_else(|| {
                checkout_under(path, cwd, payload_cwd, None)
                    .map(|c| format!("the registered worktree {}", c.description()))
            })
    };
    match glob.expand() {
        Ok(paths) => paths
            .iter()
            .find_map(|path| reached(path))
            .map(|p| format!("{p} (through `{word}`)")),
        Err(GlobStop::TooManyEntries) => {
            let prefix = glob.prefix();
            let holds =
                match super::edit_guard::holds_enforcement_files(&prefix, cwd).and_then(|first| {
                    super::edit_guard::holds_enforcement_files(&prefix, payload_cwd)
                        .map(|second| first || second)
                }) {
                    Ok(holds) => holds,
                    Err(error) => return Some(format!("cannot read enforcement paths: {error}")),
                };
            (holds || reached(&prefix).is_some()).then(|| {
                format!(
                    "`{word}`, which reads more than {GLOB_ENTRY_LIMIT} entries under a directory that holds enforcement files"
                )
            })
        }
    }
}

/// The enforcement path a relative word could name from a directory the
/// guard cannot determine (TSK-216 round 5). The word is read by its names
/// alone: its components after the last `..`, read as patterns, end an
/// enforcement path (`policy.json`, `pol*`, `../.codeflow/pol*`), or lead
/// into an enforcement directory whose every entry counts
/// (`hooks/pre-commit` under `.git/hooks`). An absolute word, or one from
/// the home directory, does not depend on the directory and is judged as
/// written; an option's value after `=` is read as a word.
fn word_could_name(word: &str) -> Option<&'static str> {
    let value = if word.starts_with('-') {
        word.split_once('=')?.1
    } else {
        word
    };
    // Each word brace expansion can make is read (TSK-216 round 16).
    let Some(words) = word_readings(value) else {
        return Some(BRACE_UNREAD);
    };
    words.iter().find_map(|w| word_could_name_spelled(w))
}

/// [`word_could_name`] for one word after brace expansion. What follows a
/// part the shell fills in at run time, or a tilde prefix the guard cannot
/// resolve, is read by name too.
fn word_could_name_spelled(value: &str) -> Option<&'static str> {
    let value = unresolved_tail(value)
        .or_else(|| unknown_tilde_rest(value))
        .unwrap_or(value);
    let value = value.strip_prefix("~+/").unwrap_or(value);
    if value.is_empty() || value.starts_with(['/', '~']) {
        return None;
    }
    // A word read conservatively matches every path below its literal
    // prefix, and from an unknown directory that may be any path
    // (TSK-216 round 17).
    if conservative_glob(value) {
        return Some(EVERY_PATH_PATTERN);
    }
    let parts: Vec<&str> = value.split('/').collect();
    let after_parent = parts
        .iter()
        .rposition(|c| *c == "..")
        .map_or(0, |at| at + 1);
    let tail: Vec<&str> = parts[after_parent..]
        .iter()
        .copied()
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    if tail.is_empty() {
        return None;
    }
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let component_matches = |pattern: &str, name: &str| {
        if has_glob(pattern) {
            (pattern.starts_with('.') || !name.starts_with('.'))
                && shell_pattern(pattern).matches_with(name, options)
        } else {
            pattern.eq_ignore_ascii_case(name)
        }
    };
    ENFORCEMENT_TEXT
        .iter()
        .chain(ENFORCEMENT_DIRS)
        .copied()
        .find(|needle| {
            let names: Vec<&str> = needle.split('/').collect();
            let whole_dir = matches!(
                *needle,
                ".codex" | ".grok" | ".codeflow/git-hooks" | ".git/hooks" | ".git/refs/remotes"
            );
            // The word's first `k` components end the needle (the unknown
            // directory supplies the rest) or hold it whole; any further
            // component must lie inside a whole enforcement directory.
            (1..=tail.len()).any(|k| {
                let head = &tail[..k];
                let n = head.len().min(names.len());
                (k == tail.len() || whole_dir)
                    && head[head.len() - n..]
                        .iter()
                        .zip(&names[names.len() - n..])
                        .all(|(pattern, name)| component_matches(pattern, name))
            })
        })
}

/// What a word read conservatively ([`conservative_glob`]) could name
/// from a directory the guard cannot determine.
const EVERY_PATH_PATTERN: &str =
    "repository enforcement files (a pattern the guard reads as every path)";

/// The most directories [`run_dirs`] lists before it gives up and reports
/// the directory as unknown.
const RUN_DIR_LIMIT: usize = 64;

/// Every directory the commands of a script can run in (TSK-216 round 5).
struct RunDirs {
    /// The starting directory and each one a literal `cd`, `pushd` or
    /// `env -C` on the script can reach from a directory listed before it.
    dirs: Vec<PathBuf>,
    /// Why the list may miss one: a directory filled in at run time, a
    /// program that can move the shell untracked, a move that repeats, or
    /// a rotation that can reach a `pushd -n` directory.
    unknown: Option<String>,
}

/// The directories the commands in `segments` can run in, starting from
/// `start`. Each literal directory change is applied, in order, to every
/// directory listed before it, so the list holds wherever the shell can be
/// whatever runs, a pipeline member, a subshell or a shell body included.
/// `pushd -n` alone moves nothing. Bash resolves the directory it stacks
/// only when a rotation or `popd` reaches it, from wherever the shell is
/// then, so once one is stacked a rotation or `popd` makes the directory
/// unknown (TSK-216 round 10). It over-approximates: a command is judged
/// from each listed directory.
fn run_dirs(segments: &[String], start: &Path) -> RunDirs {
    let mut run = RunDirs {
        dirs: vec![start.to_path_buf()],
        unknown: None,
    };
    let mut stacked = false;
    let mut repeats = false;
    let mut moves = false;
    for segment in segments {
        let mut tokens = command_argv(segment);
        repeats |= tokens.first().is_some_and(|t| {
            matches!(
                t.as_str(),
                "for" | "while" | "until" | "select" | "function"
            ) || t.ends_with("()")
        });
        strip_reserved_words(&mut tokens);
        let Some((program, args)) = strip_launchers(&tokens) else {
            continue;
        };
        let name = basename(program);
        if unresolved_word(program) {
            run.unknown
                .get_or_insert_with(|| "a program filled in at run time".to_string());
            continue;
        }
        if matches!(name, "source" | "." | "eval") {
            run.unknown
                .get_or_insert_with(|| format!("`{name}`, which can move the shell"));
            continue;
        }
        let mut moves_to: Vec<&str> = launcher_effects(&tokens).0;
        match dir_move(name, args) {
            DirMove::To(target) => moves_to.push(target),
            DirMove::Stack => stacked = true,
            DirMove::Rotate => {
                moves = true;
                // Without a `pushd -n` entry the stack holds only
                // directories the shell has been in, which are listed.
                if stacked {
                    run.unknown.get_or_insert_with(|| {
                        "a directory `pushd -n` stacked, which a rotation or `popd` moves to"
                            .to_string()
                    });
                }
            }
            DirMove::Unresolved(operand) => {
                moves = true;
                run.unknown.get_or_insert_with(|| {
                    let shown = shown_word(operand);
                    if unresolved_word(operand) {
                        format!("`{shown}`, a directory filled in at run time")
                    } else {
                        format!("`{name} {shown}`, a directory the guard does not resolve")
                    }
                });
            }
            DirMove::Stay => {}
        }
        for target in moves_to {
            moves = true;
            for dir in reach_dirs(target, &mut run) {
                if !add_run_dir(&mut run, dir) {
                    return run;
                }
            }
        }
    }
    if repeats && moves {
        run.unknown
            .get_or_insert_with(|| "a directory change in a loop or function".to_string());
    }
    run
}

/// Add a directory to the list, unless it is there already. `false`, with
/// the directory marked unknown, once the list is full: it never grows past
/// [`RUN_DIR_LIMIT`] (TSK-216 round 6).
fn add_run_dir(run: &mut RunDirs, dir: PathBuf) -> bool {
    if run.dirs.contains(&dir) {
        return true;
    }
    if run.dirs.len() >= RUN_DIR_LIMIT {
        run.unknown = Some(format!(
            "more than {RUN_DIR_LIMIT} directories the line can move to"
        ));
        return false;
    }
    run.dirs.push(dir);
    true
}

/// The directories a move to `target` reaches from each listed directory,
/// a glob expanded; a target filled in at run time reaches none and marks
/// the directory unknown. At most [`RUN_DIR_LIMIT`] are returned.
fn reach_dirs(target: &str, run: &mut RunDirs) -> Vec<PathBuf> {
    if unresolved_word(target) {
        run.unknown.get_or_insert_with(|| {
            format!(
                "`{}`, a directory filled in at run time",
                shown_word(target)
            )
        });
        return Vec::new();
    }
    let mut reached = Vec::new();
    for dir in &run.dirs {
        let path = if target == "~" {
            std::env::var_os("HOME").map_or_else(|| dir.clone(), PathBuf::from)
        } else {
            integrity_shell_path(target, dir)
        };
        if target.contains(['*', '?', '[']) {
            match WordGlob::new(target, dir).expand() {
                Ok(found) => reached.extend(found),
                Err(GlobStop::TooManyEntries) => {
                    run.unknown.get_or_insert_with(|| {
                        format!("`{target}`, a directory glob over too many entries")
                    });
                }
            }
        } else {
            reached.push(path);
        }
        if reached.len() > RUN_DIR_LIMIT {
            run.unknown = Some(format!(
                "more than {RUN_DIR_LIMIT} directories the line can move to"
            ));
            reached.truncate(RUN_DIR_LIMIT);
            break;
        }
    }
    reached
}

/// How a `cd`, `pushd`, `popd` or `chdir` moves the shell.
enum DirMove<'a> {
    /// To its operand, or home for a bare `cd`.
    To(&'a str),
    /// `pushd -n DIR`: DIR goes on the stack, unresolved, and the shell
    /// stays.
    Stack,
    /// A stack rotation (`pushd +1`, `pushd -1`, a bare `pushd`) or a
    /// `popd`: to a directory on the stack.
    Rotate,
    /// To an operand that is not a plain literal path ([`plain_dir`]): the
    /// directory becomes unknown (TSK-216 round 11).
    Unresolved(&'a str),
    /// Nowhere: other programs do not move the shell.
    Stay,
}

/// A `cd` or `pushd` operand the guard resolves as written: no stack
/// reference or other tilde form (`~1`, `~+1`, `~-`, `~user`) beyond `~`
/// and `~/...`, no bare `-`, nothing the shell fills in (`$`, a
/// substitution) and no pattern or brace expansion (TSK-216 round 11).
fn plain_dir(word: &str) -> bool {
    let tilde = word.starts_with('~') && word != "~" && !word.starts_with("~/");
    !(tilde || word == "-" || unresolved_word(word) || has_glob(word) || word.contains('{'))
}

fn dir_move<'a>(name: &str, args: &'a [String]) -> DirMove<'a> {
    if name == "popd" {
        return DirMove::Rotate;
    }
    if !matches!(name, "cd" | "pushd" | "chdir") {
        return DirMove::Stay;
    }
    let stack_index = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    let mut options = true;
    let mut no_change = false;
    for arg in args {
        let word = arg.as_str();
        if options && word == "--" {
            options = false;
            continue;
        }
        if name == "pushd" && word.strip_prefix('+').is_some_and(stack_index) {
            return DirMove::Rotate;
        }
        if options && word.len() > 1 && word.starts_with('-') {
            if name == "pushd" && stack_index(&word[1..]) {
                return DirMove::Rotate;
            }
            no_change |= name == "pushd" && word == "-n";
            continue;
        }
        return if no_change {
            DirMove::Stack
        } else if plain_dir(word) {
            DirMove::To(word)
        } else {
            DirMove::Unresolved(word)
        };
    }
    if name == "pushd" {
        DirMove::Rotate
    } else {
        DirMove::To("~")
    }
}

/// Judge a command from every directory it can run in. Where that list may
/// be incomplete, a command that changes files is also refused when one of
/// its words could name an enforcement path from any directory: expanding
/// it from a listed directory proves nothing (TSK-216 round 5).
fn integrity_write_in_run(
    tokens: &[String],
    redirects: &Redirects,
    level: PolicyLevel,
    run: &RunDirs,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    run.dirs
        .iter()
        .find_map(|dir| integrity_write_violation(tokens, redirects, level, dir, payload_cwd, line))
        .or_else(|| {
            let why = run.unknown.as_deref()?;
            unknown_dir_name_violation(tokens, redirects, level, line, why)
        })
}

fn integrity_shell_path(token: &str, cwd: &Path) -> PathBuf {
    if token == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    // `~+` is the directory the command runs in.
    if token == "~+" {
        return cwd.to_path_buf();
    }
    if let Some(relative) = token.strip_prefix("~+/") {
        return cwd.join(relative);
    }
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
    let pattern = path.file_name()?;
    let bytes = pattern.as_encoded_bytes();
    if !bytes.starts_with(b".") || !bytes.iter().any(|b| b"*?[{".contains(b)) {
        return None;
    }
    let parent = match path.parent()?.canonicalize() {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return Some("repository root pattern (cannot read directory)"),
    };
    let info = match super::RepoInfo::discover(&parent) {
        Ok(info) => info?,
        Err(_) => return Some("repository root pattern (cannot read repository)"),
    };
    let Ok(root) = info.root.canonicalize() else {
        return Some("repository root pattern (cannot read root)");
    };
    if parent != root {
        return None;
    }
    let Some(pattern) = pattern.to_str() else {
        // The root pattern cannot be classified as text: refuse it as
        // potentially reaching enforcement, never treat it as absent.
        return Some(".codeflow/");
    };
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
    shell_pattern(pattern).matches(name)
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

/// The targets the redirections of one shell command can write, read from
/// its text with quote provenance, so a quoted `">x"` is text (TSK-216
/// round 11). A redirection is a read only when it is provably `<`, `<<`,
/// `<<-`, `<<<` or a descriptor copy or close (`N>&M`, `>&N`, `N<&M`,
/// `N>&-`, `N<&-`). Every other operator, `>`, `>>`, `>|`, `&>`, `&>>`,
/// `<>`, `{name}>` or one attached mid-word (`x>file`) included, opens its
/// target for writing. A process substitution (`>(...)`, `<(...)`) is a
/// command, judged as one, not a redirection.
///
/// Line continuations are joined first, as the shell joins them (TSK-216
/// round 12). The reader does not read ANSI-C or locale quoting (`$'...'`,
/// `$"..."`): on a command that holds either and a `>`, every word is kept
/// in [`Redirects::unread`] and judged by name.
fn redirect_writes(segment: &str) -> Redirects {
    let joined = join_continuations(segment);
    if (joined.contains("$'") || joined.contains("$\"")) && joined.contains('>') {
        let unread = line_words(&joined.replace(['\'', '"', '$', '\\'], " "))
            .map(str::to_string)
            .collect();
        return Redirects {
            targets: Vec::new(),
            unread,
        };
    }
    Redirects {
        targets: redirect_targets(&joined),
        unread: Vec::new(),
    }
}

/// What the redirections of one command can write ([`redirect_writes`]).
#[derive(Default)]
struct Redirects {
    /// The targets of its write redirections, read as written.
    targets: Vec<String>,
    /// Every word of a command whose quoting the reader does not read:
    /// any of them may be a write target, so each is judged by name.
    unread: Vec<String>,
}

/// `text` with each backslash-newline outside single quotes removed, as the
/// shell removes line continuations before it reads a command.
fn join_continuations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut single, mut double) = (false, false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '\\' if !single => match chars.next() {
                Some('\n') => continue,
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                    continue;
                }
                None => {}
            },
            _ => {}
        }
        out.push(c);
    }
    out
}

fn redirect_targets(segment: &str) -> Vec<String> {
    let chars: Vec<char> = segment.chars().collect();
    let mut targets = Vec::new();
    let (mut single, mut double) = (false, false);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '\\' if !single => i += 1,
            '<' | '>' if !single && !double && chars.get(i) != Some(&'(') => {
                let (writes, after) = redirect_operator(&chars, i - 1);
                let (target, end) = redirect_word(&chars, after);
                // zsh reads a numeric range (`<1-9>`) as part of the
                // target word, and Bash as two more redirections, so the
                // text after the operator is read again for Bash's
                // reading too (TSK-216 round 17).
                i = if numeric_range(&target) { after } else { end };
                if writes(&target) {
                    targets.push(target);
                }
            }
            _ => {}
        }
    }
    targets
}

/// The redirection operator starting at `at`: whether it writes the target
/// that follows, and where that target starts.
fn redirect_operator(chars: &[char], at: usize) -> (fn(&str) -> bool, usize) {
    fn never(_: &str) -> bool {
        false
    }
    fn always(_: &str) -> bool {
        true
    }
    // `>&N`, `>&N-` and `>&-` copy, move or close a descriptor; `>&file`
    // writes the file.
    fn unless_copy(target: &str) -> bool {
        let fd = target.strip_suffix('-').unwrap_or(target);
        !(target == "-" || (!fd.is_empty() && fd.chars().all(|c| c.is_ascii_digit())))
    }
    let next = |n: usize| chars.get(at + n).copied();
    if chars[at] == '<' {
        return match (next(1), next(2)) {
            (Some('<'), Some('<' | '-')) => (never, at + 3),
            (Some('<' | '&'), _) => (never, at + 2),
            (Some('>'), _) => (always, at + 2),
            _ => (never, at + 1),
        };
    }
    match next(1) {
        // csh's `>>&file` appends both streams.
        Some('>' | '|') if next(2) == Some('&') => (always, at + 3),
        Some('>' | '|') => (always, at + 2),
        Some('&') => (unless_copy, at + 2),
        _ => (always, at + 1),
    }
}

/// The word after a redirection operator, quotes removed, and where it
/// ends: blanks before it are skipped, and it stops at an unquoted blank
/// or shell metacharacter.
fn redirect_word(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while chars.get(i).is_some_and(|c| matches!(c, ' ' | '\t')) {
        i += 1;
    }
    let mut word = String::new();
    let (mut single, mut double) = (false, false);
    // Parentheses in the word are part of it (`out(D)`, `(a|b)/x`), as
    // zsh reads them; a `>(` written together never reaches here (TSK-216
    // rounds 17 and 18).
    let mut parens = 0usize;
    while let Some(&c) = chars.get(i) {
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '\\' if !single => {
                i += 1;
                if let Some(&escaped) = chars.get(i) {
                    word.push(escaped);
                }
            }
            '(' if !single && !double => {
                parens += 1;
                word.push(c);
            }
            ')' if !single && !double && parens > 0 => {
                parens -= 1;
                word.push(c);
            }
            '<' if !single && !double && numeric_range_len(&chars[i..]).is_some() => {
                let len = numeric_range_len(&chars[i..]).unwrap_or(1);
                word.extend(&chars[i..i + len]);
                i += len - 1;
            }
            c if !single
                && !double
                && (shell_blank(c) || matches!(c, '<' | '>' | '|' | ';' | '&' | '(' | ')')) =>
            {
                break;
            }
            c => word.push(c),
        }
        i += 1;
    }
    (word, i)
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

/// Whether an `rm` removes directories recursively (`-r`, `-R`, inside a
/// cluster, or `--recursive`), reading options up to `--`.
fn rm_recursive(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| {
            arg == "--recursive"
                || arg
                    .strip_prefix('-')
                    .is_some_and(|flags| !flags.starts_with('-') && flags.contains(['r', 'R']))
        })
}

/// The operands of a command that takes options then paths: every
/// non-option word, and every word after `--`.
fn rm_operands(args: &[String]) -> Vec<&str> {
    let mut operands = Vec::new();
    let mut options = true;
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if !options || !arg.starts_with('-') || arg == "-" {
            operands.push(arg.as_str());
        }
    }
    operands
}

// ---------------------------------------------------------------------------
// the text floor (TSK-216 review round 2)
// ---------------------------------------------------------------------------

/// Enforcement paths as a command line spells them. A command that can
/// change files and names one of these anywhere in its text (a `sed`
/// script, a `find` action, a `sh -c` string, a producer piped into
/// `xargs`) is refused. This floor is new hardening over 3.0.0's
/// command-specific checks, which it must keep: the precise readings below
/// may clear a match they prove harmless, never remove it otherwise.
/// A path built at run time, which no argument spells, is past this floor;
/// the OS sandbox's write denies are the backstop there.
const ENFORCEMENT_TEXT: &[&str] = &[
    ".codeflow/policy.json",
    ".codeflow/project.toml",
    ".codeflow/git-hooks",
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".git/hooks",
    ".git/config",
    ".git/packed-refs",
    ".git/refs/remotes",
    ".github/workflows/codeflow-ci.yml",
    ".codex",
    ".grok",
];

/// Directories that hold enforcement paths, matched only when the text
/// names the directory itself (`.codeflow`, `.codeflow/`, `.codeflow/*`).
const ENFORCEMENT_DIRS: &[&str] = &[
    ".codeflow",
    ".claude",
    ".git",
    ".github/workflows",
    ".github",
];

/// The folders linked worktrees live in, matched when the text names the
/// folder or one entry of it (`.worktrees`, `.worktrees/<name>`).
const WORKTREE_DIRS: &[&str] = &[".claude/worktrees", ".worktrees"];

/// Programs that only read the paths they are given, so running them
/// through `find -exec` or `xargs` changes nothing. None of them has an
/// output-file or command-running mode: `sort -o`, `uniq IN OUT`, `xxd IN
/// OUT`, `file -C` and `rg --pre` write files or run commands, so those
/// programs are judged as writers (TSK-216 round 3).
const READ_ONLY_PROGRAMS: &[&str] = &[
    "echo",
    "printf",
    "grep",
    "egrep",
    "fgrep",
    "cat",
    "head",
    "tail",
    "wc",
    "ls",
    "stat",
    "test",
    "[",
    "true",
    "false",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "du",
    "diff",
    "cmp",
    "od",
    "hexdump",
    "strings",
    "jq",
    "md5",
    "md5sum",
    "shasum",
    "sha1sum",
    "sha256sum",
];

fn path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// The text after each place `needle` occurs in `text` as the start of a
/// path, not inside a longer name.
fn path_mentions<'t>(text: &'t str, needle: &'t str) -> impl Iterator<Item = &'t str> + 't {
    text.match_indices(needle).filter_map(move |(at, _)| {
        if text[..at].chars().next_back().is_some_and(path_char) {
            return None;
        }
        Some(&text[at + needle.len()..])
    })
}

/// Whether `after` ends a name: the end, or a character no name holds.
fn ends_name(after: &str) -> bool {
    after
        .chars()
        .next()
        .is_none_or(|c| !path_char(c) && c != '/')
}

/// Whether `after`, the text after a directory name, leaves it naming the
/// directory itself: nothing more, a trailing `/`, or a glob over it.
fn names_whole_dir(after: &str) -> bool {
    ends_name(after)
        || after
            .strip_prefix('/')
            .is_some_and(|rest| ends_name(rest) || rest.starts_with(['*', '?', '[', '{']))
}

/// Text as the file system reads the paths in it: quotes removed, `//`
/// and `/./` collapsed, and lower case, since a case-insensitive file
/// system (the macOS default) reads `.CODEFLOW` as `.codeflow`.
fn canonical_text(text: &str) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '"'))
        .collect::<String>()
        .to_lowercase();
    loop {
        let next = out.replace("//", "/").replace("/./", "/");
        if next == out {
            return out;
        }
        out = next;
    }
}

/// The enforcement path `text` names, when it names one, in any spelling
/// [`canonical_text`] makes equal.
fn enforcement_text(text: &str) -> Option<&'static str> {
    enforcement_text_spelled(text).or_else(|| {
        // Each word brace expansion can make of a word (TSK-216 round 16).
        line_words(text)
            .filter(|word| word.contains('{'))
            .find_map(|word| match brace_words(word) {
                None => Some(BRACE_UNREAD),
                Some(words) => words
                    .iter()
                    .filter(|w| w.as_str() != word)
                    .find_map(|w| enforcement_text_spelled(w)),
            })
    })
}

/// [`enforcement_text`] for text as written, without brace expansion.
fn enforcement_text_spelled(text: &str) -> Option<&'static str> {
    let text = canonical_text(text);
    let text = text.as_str();
    ENFORCEMENT_TEXT
        .iter()
        .copied()
        .find(|needle| {
            path_mentions(text, needle).any(|after| ends_name(after) || after.starts_with('/'))
        })
        .or_else(|| {
            ENFORCEMENT_DIRS
                .iter()
                .copied()
                .find(|needle| path_mentions(text, needle).any(names_whole_dir))
        })
}

/// The worktree folder `text` names, as itself or as one entry of it.
fn worktree_text(text: &str) -> Option<&'static str> {
    let text = canonical_text(text);
    let text = text.as_str();
    WORKTREE_DIRS.iter().copied().find(|needle| {
        path_mentions(text, needle).any(|after| {
            names_whole_dir(after)
                || after.strip_prefix('/').is_some_and(|rest| {
                    let name = rest.len() - rest.trim_start_matches(path_char).len();
                    name > 0 && names_whole_dir(&rest[name..])
                })
        })
    })
}

/// The enforcement path a command line names, as text or through a glob
/// word that reaches one from `cwd` once expanded and resolved
/// (`.codeflow/pol*`, `alias/pol*` through a symbolic link, `.*`).
fn line_names(line: &str, cwd: &Path, payload_cwd: &Path) -> Option<String> {
    if let Some(p) = enforcement_text(line) {
        return Some(p.to_string());
    }
    // Each word, as split at blanks and as the shell reads it (quotes and
    // escapes removed, `policy"".json`), and the value of an assignment
    // (`x=alias/policy.json`), after brace expansion: a glob is expanded,
    // and a plain path is resolved through symbolic links (TSK-216 round
    // 16).
    let tokens: Vec<String> = expand_commands(line)
        .iter()
        .flat_map(|segment| shell_tokens(segment))
        .collect();
    let found = line_words(line)
        .chain(tokens.iter().map(String::as_str))
        .flat_map(|word| {
            let value = word
                .split_once('=')
                .filter(|(name, _)| assignment_name(name))
                .map(|(_, value)| value);
            std::iter::once(word).chain(value)
        })
        .find_map(|word| {
            let Some(words) = word_readings(word) else {
                return Some(BRACE_UNREAD.to_string());
            };
            words.iter().find_map(|w| {
                if has_glob(w) {
                    glob_reach(w, cwd, payload_cwd)
                } else {
                    let path = integrity_shell_path(w, cwd);
                    if w.is_empty() {
                        return None;
                    }
                    match super::edit_guard::repository_enforcement_target(
                        &path,
                        payload_cwd,
                        false,
                    ) {
                        Ok(true) => Some(format!("repository enforcement files (through `{w}`)")),
                        Ok(false) => None,
                        Err(error) => {
                            Some(format!("cannot read repository enforcement paths: {error}"))
                        }
                    }
                }
            })
        });
    found
}

/// Whether `name` is a shell variable name, as on the left of `=` in an
/// assignment.
fn assignment_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Raw words split at blanks and shell operators for conservative name
/// detection. Only `shell_words` removes shell quote framing.
fn line_words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| {
        shell_blank(c) || matches!(c, '|' | ';' | '&' | '(' | ')' | '<' | '>' | '\0')
    })
    .filter(|w| !w.is_empty())
}

/// Whether the shell fills in part of `word` when the command runs: a
/// variable, or a command substitution the tokenizer cut out.
fn unresolved_word(word: &str) -> bool {
    word.contains(['$', '`']) || has_substitution(word)
}

/// The enforcement path the command line names, when one of `args` is
/// filled in by the shell: the value may come from that text
/// (`p=<path>; rm "$p"`, `"$(echo <path>)"`).
fn unresolved_names_enforcement<'a>(
    args: impl IntoIterator<Item = &'a String>,
    line: &str,
    cwd: &Path,
    payload_cwd: &Path,
) -> Option<String> {
    args.into_iter()
        .any(|arg| unresolved_word(arg))
        .then(|| line_names(line, cwd, payload_cwd))
        .flatten()
}

fn read_only_program(tokens: &[String]) -> bool {
    strip_launchers(tokens).is_some_and(|(program, _)| {
        let name = basename(program);
        READ_ONLY_PROGRAMS.contains(&name) && !is_shell(name)
    })
}

/// A variable the line does not set, read from the guard's own
/// environment: every other occurrence of the name on the line must be an
/// expansion of it.
fn environment_value(name: &str, line: &str) -> Option<std::ffi::OsString> {
    let bare = line.match_indices(name).any(|(at, _)| {
        let before = &line[..at];
        let after = &line[at + name.len()..];
        let word_start = !before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let word_end = !after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        word_start && word_end && !before.ends_with('$') && !before.ends_with("${")
    });
    if bare {
        return None;
    }
    std::env::var_os(name)
}

/// The paths a destructive command's target word names, or `None` when
/// the guard cannot resolve it: a command substitution, a variable the line
/// sets or the guard cannot read, or a brace expansion. A glob is expanded
/// against the file system.
fn resolve_targets(token: &str, cwd: &Path, line: &str) -> Option<Vec<PathBuf>> {
    if token.contains('`') || token.contains("$(") || has_substitution(token) {
        return None;
    }
    let mut text = String::new();
    let mut rest = token;
    while let Some(at) = rest.find('$') {
        text.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, used) = if let Some(braced) = after.strip_prefix('{') {
            let end = braced.find('}')?;
            (&braced[..end], end + 2)
        } else {
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            (&after[..end], end)
        };
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        text.push_str(environment_value(name, line)?.to_str()?);
        rest = &after[used..];
    }
    text.push_str(rest);
    if text.contains('{') && (text.contains(',') || text.contains("..")) {
        return None;
    }
    if text.contains(['*', '?', '[']) {
        return WordGlob::new(&text, cwd).expand().ok();
    }
    Some(vec![integrity_shell_path(&text, cwd)])
}

/// A recursive delete of a registered worktree, or of a directory holding
/// one, removes a live checkout with its uncommitted work and enforcement
/// files (TSK-216 review finding 1).
enum CheckoutReach {
    Path(PathBuf),
    Unreadable(String),
}

impl CheckoutReach {
    fn description(&self) -> String {
        match self {
            Self::Path(path) => path.display().to_string(),
            Self::Unreadable(why) => format!("cannot read registered worktrees: {why}"),
        }
    }
}

fn checkout_delete_violation(
    level: PolicyLevel,
    what: &str,
    checkout: &CheckoutReach,
) -> Violation {
    let message = match checkout {
        CheckoutReach::Path(path) => format!(
            "{what} would delete the registered worktree `{}` with its work and enforcement files",
            path.display()
        ),
        CheckoutReach::Unreadable(why) => {
            format!("{what}: cannot read registered worktrees: {why}")
        }
    };
    Violation::new(
        "git.hook_integrity",
        level,
        message,
        crate::remedy::WORKTREE_DELETE.remedy(),
    )
}

fn checkout_under(
    path: &Path,
    cwd: &Path,
    payload_cwd: &Path,
    except: Option<&Path>,
) -> Option<CheckoutReach> {
    for root in [cwd, payload_cwd] {
        match super::edit_guard::registered_checkout_under(path, root, except) {
            Ok(Some(path)) => return Some(CheckoutReach::Path(path)),
            Ok(None) => {}
            Err(error) => return Some(CheckoutReach::Unreadable(error)),
        }
    }
    None
}

/// Judge the targets of a command that deletes directories (TSK-216 review
/// round 2): a target that resolves to a registered worktree or a directory
/// holding one is refused, and so is one the guard cannot resolve, where
/// registered worktrees live under the checkout or the text names their
/// folder. `except` is a checkout the command never deletes, such as the
/// one `git clean` cleans.
fn worktree_delete_check(
    what: &str,
    targets: &[&str],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
    except: Option<&Path>,
) -> Option<Violation> {
    let held = match super::edit_guard::holds_registered_worktrees(cwd) {
        Ok(held) => held,
        Err(error) => {
            return Some(hook_integrity_violation(
                level,
                format!("cannot read registered worktrees: {error}"),
            ))
        }
    };
    for target in targets.iter().filter(|t| !t.is_empty()) {
        match resolve_targets(target, cwd, line) {
            Some(paths) => {
                if let Some(checkout) = paths
                    .iter()
                    .find_map(|path| checkout_under(path, cwd, payload_cwd, except))
                {
                    return Some(checkout_delete_violation(level, what, &checkout));
                }
            }
            None if held || worktree_text(target).is_some() => {
                return Some(Violation::new(
                    "git.hook_integrity",
                    level,
                    format!(
                        "{what} would delete `{target}`, which the guard cannot resolve to a path, where registered worktrees live; it is judged as deleting one"
                    ),
                    crate::remedy::WORKTREE_DELETE.remedy(),
                ));
            }
            None => {}
        }
    }
    None
}

/// `git clean` (git-clean(1)); long options take any unambiguous prefix
/// (`--for`).
const GIT_CLEAN_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('d', Arity::Flag),
        ('f', Arity::Flag),
        ('i', Arity::Flag),
        ('n', Arity::Flag),
        ('q', Arity::Flag),
        ('e', Arity::Value),
        ('x', Arity::Flag),
        ('X', Arity::Flag),
    ],
    long: &[
        ("--force", Arity::Flag),
        ("--interactive", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--exclude", Arity::Value),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git clean` with force given twice removes untracked directories that
/// are other repositories, which is what a linked worktree inside the
/// checkout is. A dry run deletes nothing.
fn git_clean_violation(
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let parsed = parse_options(args, &GIT_CLEAN_OPTIONS);
    let force = parsed
        .sequence
        .iter()
        .filter(|seen| matches!(seen, Seen::Short('f') | Seen::Long("--force", _)))
        .count();
    let dry_run = parsed.has_short(&['n']) || parsed.has_long("--dry-run");
    let mut paths = parsed.operands;
    if force < 2 || dry_run {
        return None;
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let own = super::edit_guard::checkout_root_of(cwd);
    worktree_delete_check(
        "`git clean -ff`",
        &paths,
        level,
        cwd,
        payload_cwd,
        line,
        own.as_deref(),
    )
}

/// A `sh -c` script with its positional parameters written in (`$0` to
/// `$9`, `${N}`, `$@`, `$*`), so `sh -c 'rm "$1"' _ <path>` is judged as
/// `rm <path>`.
fn bind_positional(script: &str, params: &[String]) -> String {
    let word = |value: &str| {
        if value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-:~+,@%=".contains(c))
        {
            value.to_string()
        } else {
            format!("'{}'", value.replace('\'', r"'\''"))
        }
    };
    let all = params
        .iter()
        .skip(1)
        .map(|p| word(p))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = script
        .replace("\"$@\"", &all)
        .replace("\"$*\"", &all)
        .replace("$@", &all)
        .replace("$*", &all);
    for n in (0..=9).rev() {
        let value = params.get(n).map_or(String::new(), |p| word(p));
        out = out
            .replace(&format!("${{{n}}}"), &value)
            .replace(&format!("${n}"), &value);
    }
    out
}

/// A command run for its paths by `find -exec` or `xargs`: judged as the
/// guard judges it written on its own, and a `sh -c` script inside it as
/// the commands of that script with its positional parameters bound.
fn wrapped_violation(
    tokens: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    if let Some((program, args)) = strip_launchers(tokens) {
        if is_shell(basename(program)) {
            if let Some(script) = shell_c_argument(args) {
                let params = args
                    .iter()
                    .position(|arg| std::ptr::eq(arg, script))
                    .map_or(&[][..], |at| &args[at + 1..]);
                let bound = bind_positional(script, params);
                // The body runs where the launchers in front of its shell
                // put it, and follows its own directory changes.
                let start = launcher_effects(tokens)
                    .0
                    .iter()
                    .fold(cwd.to_path_buf(), |dir, change| {
                        integrity_shell_path(change, &dir)
                    });
                let segments = expand_commands(&bound);
                let run = run_dirs(&segments, &start);
                return segments.iter().find_map(|segment| {
                    integrity_write_in_run(
                        &shell_tokens(segment),
                        &redirect_writes(segment),
                        level,
                        &run,
                        payload_cwd,
                        line,
                    )
                });
            }
        }
    }
    integrity_write_violation(tokens, &Redirects::default(), level, cwd, payload_cwd, line)
}

/// The arguments of `sed` that a grammar reads as input files.
fn sed_operands_in<'a>(args: &'a [String], spec: &OptionSpec) -> Vec<&'a str> {
    let parsed = parse_options(args, spec);
    let scripted = parsed.has_short(&['e', 'f'])
        || parsed.has_long("--expression")
        || parsed.has_long("--file");
    let skip = usize::from(!scripted);
    parsed.operands.into_iter().skip(skip).collect()
}

/// The largest `sed -f` script the guard reads; a larger one is refused.
const SED_SCRIPT_LIMIT: u64 = 1 << 24;

/// The text floor for `sed`: any argument that names an enforcement path
/// refuses, script and option values included, so a `w`, `W` or GNU `e`
/// command and a comment alike count. The one clearance is a plain read:
/// without `-i`, a name that both grammars read as an input file. A `-f`
/// script is read and judged the same way; one read from the input is
/// judged by the rest of the command line.
fn sed_text_violation(
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let reads = sed_read_flags(args);
    let unread = || {
        args.iter()
            .zip(&reads)
            .filter(|(_, r)| !**r)
            .map(|(a, _)| a)
    };
    if let Some((arg, p)) = unread().find_map(|arg| enforcement_text(arg).map(|p| (arg, p))) {
        return Some(hook_integrity_violation(
            level,
            format!("`sed` names the enforcement path `{p}` in `{arg}`, where its script or in-place edit can write it"),
        ));
    }
    if let Some(p) = unresolved_names_enforcement(unread(), line, cwd, payload_cwd) {
        return Some(hook_integrity_violation(
            level,
            format!("`sed` takes a word the shell fills in, and the command line names the enforcement path `{p}`"),
        ));
    }
    // The rest of the line, without the files this `sed` only reads.
    let elsewhere = line_without_reads(line, args, &reads);
    let line = elsewhere.as_str();
    for spec in SED_GRAMMARS {
        for file in parse_options(args, spec).values_of('f', "--file") {
            let why = if matches!(file, "-" | "/dev/stdin") {
                line_names(line, cwd, payload_cwd).map(|p| {
                    format!("reads its script from its input, and the command line names `{p}`")
                })
            } else {
                match read_sed_script(file, cwd) {
                    SedRead::Read(text) => enforcement_text(&text)
                        .map(|p| format!("runs the script file `{file}`, which names `{p}`")),
                    SedRead::Unreadable { file, why } => {
                        Some(format!("cannot read script `{file}`: {why}"))
                    }
                }
            };
            if let Some(why) = why {
                return Some(hook_integrity_violation(level, format!("`sed` {why}")));
            }
        }
    }
    None
}

/// The `-name` and `-iname` patterns a path must all match to reach a
/// `find` action, read from the expression before that action only, or
/// `None` when that part can select a path in a way the guard does not
/// model (`-o`, `!`, `-not`, `(`, path or regex tests), so every path may
/// reach it.
fn find_name_filter(args: &[String]) -> Option<Vec<(String, bool)>> {
    let mut names = Vec::new();
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        match arg.as_str() {
            "-o" | "-or" | "!" | "-not" | "(" | "-path" | "-ipath" | "-wholename"
            | "-iwholename" | "-regex" | "-iregex" | "-lname" | "-ilname" => return None,
            "-name" | "-iname" => {
                names.push((args.get(at + 1)?.clone(), arg == "-iname"));
                at += 1;
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                while args.get(at).is_some_and(|a| a != ";" && a != "+") {
                    at += 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    (!names.is_empty()).then_some(names)
}

fn find_name_matches(filter: Option<&[(String, bool)]>, path: &Path) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if path.file_name().is_some_and(|n| n == "*") {
        return true;
    }
    // OS text rule (issue 79): a candidate is a name read from disk, which
    // need not be valid UTF-8. A single-character wildcard consumes one byte
    // in the C locale but one replacement character in the lossy spelling, so
    // the lossy answer cannot rule such a name out: it stays a candidate
    // (protection kept) unless the pattern is literals and `*` only.
    let lossy = path.file_name().map(std::ffi::OsStr::to_string_lossy);
    let invalid = path.file_name().is_some_and(|name| name.to_str().is_none());
    let name = lossy.unwrap_or_else(|| path.to_string_lossy());
    filter.iter().all(|(pattern, insensitive)| {
        let options = glob::MatchOptions {
            case_sensitive: !insensitive,
            ..glob::MatchOptions::new()
        };
        if invalid && pattern.contains(['?', '[', '\\']) {
            return true;
        }
        shell_pattern(pattern).matches_with(&name, options)
    })
}

/// The `find` primaries that take the next word as their value, which is
/// never an action, however it is spelled (`-name -delete`).
const FIND_VALUE_PRIMARIES: &[&str] = &[
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
    "-iwholename",
    "-regex",
    "-iregex",
    "-lname",
    "-ilname",
    "-type",
    "-xtype",
    "-user",
    "-group",
    "-uid",
    "-gid",
    "-perm",
    "-size",
    "-links",
    "-inum",
    "-samefile",
    "-mtime",
    "-mmin",
    "-atime",
    "-amin",
    "-ctime",
    "-cmin",
    "-used",
    "-fstype",
    "-context",
    "-maxdepth",
    "-mindepth",
    "-printf",
    "-files0-from",
    "-regextype",
];

/// The actions of a `find` expression, read in order (TSK-216): the
/// primaries it runs, with the value of a primary that takes one skipped,
/// and the command of each `-exec`, `-execdir`, `-ok` or `-okdir`, whose
/// words are its own and never primaries.
fn find_expression(args: &[String]) -> (Vec<&str>, Vec<&[String]>) {
    let mut actions = Vec::new();
    let mut commands = Vec::new();
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let word = arg.as_str();
        match word {
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                let tail = &args[at + 1..];
                let len = tail
                    .iter()
                    .position(|a| matches!(a.as_str(), ";" | "+"))
                    .unwrap_or(tail.len());
                commands.push(&tail[..len]);
                at += len + 1;
            }
            "-fprintf" => {
                actions.push(word);
                at += 2;
            }
            "-fprint" | "-fprint0" | "-fls" => {
                actions.push(word);
                at += 1;
            }
            _ if FIND_VALUE_PRIMARIES.contains(&word) || word.starts_with("-newer") => at += 1,
            _ if word.starts_with('-') => actions.push(word),
            _ => {}
        }
        at += 1;
    }
    (actions, commands)
}

/// Whether a `find` expression changes files: a `-delete` or output-file
/// action, or a command that is not read-only. A word that is a primary's
/// value or a command's argument is never an action.
fn find_mutates(args: &[String]) -> bool {
    let (actions, commands) = find_expression(args);
    actions
        .iter()
        .any(|a| matches!(*a, "-delete" | "-fprint" | "-fprint0" | "-fprintf" | "-fls"))
        || commands
            .into_iter()
            .any(|command| !read_only_program(command))
}

/// Judge what a `find` does to the paths it visits. First the text floor:
/// a `find` that changes files and names an enforcement path anywhere is
/// refused, as is one that follows symbolic links (`-L`, `-follow`), reads
/// its starting points from a file that names one, or starts from a target
/// it cannot resolve where worktrees live. Then each action in expression
/// order: every path holding enforcement state or a registered worktree
/// that the starting points reach and the `-name` tests before the action
/// let through is put in place of `{}` and judged as a direct command; an
/// `-execdir` command runs from that path's own directory.
#[allow(clippy::too_many_lines)] // One pass over the find expression.
fn find_action_violation(
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let skip = args
        .iter()
        .take_while(|arg| matches!(arg.as_str(), "-H" | "-L" | "-P"))
        .count();
    let follows = args[..skip].iter().any(|a| a == "-L") || args.iter().any(|a| a == "-follow");
    let rest = &args[skip..];
    let end = rest
        .iter()
        .position(|arg| arg.starts_with('-') || matches!(arg.as_str(), "!" | "("))
        .unwrap_or(rest.len());
    let starts: Vec<&str> = if end == 0 {
        vec!["."]
    } else {
        rest[..end].iter().map(String::as_str).collect()
    };
    if find_mutates(rest) {
        if let Some(p) = rest
            .iter()
            .find_map(|a| enforcement_text(a))
            .map(str::to_string)
            .or_else(|| unresolved_names_enforcement(rest, line, cwd, payload_cwd))
        {
            return Some(hook_integrity_violation(
                level,
                format!("`find` names the enforcement path `{p}` in a command that changes files"),
            ));
        }
        if follows {
            return Some(hook_integrity_violation(
                level,
                "`find -L` follows symbolic links the guard does not map, in a command that changes files".to_string(),
            ));
        }
        if let Some(at) = rest.iter().position(|a| a == "-files0-from") {
            let text = match rest.get(at + 1).map(String::as_str) {
                Some("-") | None => {
                    return Some(hook_integrity_violation(
                        level,
                        "cannot read find starting points from stdin".to_string(),
                    ))
                }
                Some(file) => match std::fs::read_to_string(cwd.join(file)) {
                    Ok(text) => text,
                    Err(error) => {
                        return Some(hook_integrity_violation(
                            level,
                            format!("cannot read find starting points from {file}: {error}"),
                        ))
                    }
                },
            };
            if let Some(p) = enforcement_text(&text).or_else(|| worktree_text(&text)) {
                return Some(hook_integrity_violation(
                    level,
                    format!("`find -files0-from` reads starting points that name `{p}`, in a command that changes files"),
                ));
            }
        }
        if let Some(v) = worktree_delete_check(
            "`find`",
            &starts
                .iter()
                .copied()
                .filter(|start| resolve_targets(start, cwd, line).is_none())
                .collect::<Vec<_>>(),
            level,
            cwd,
            payload_cwd,
            line,
            None,
        ) {
            return Some(v);
        }
    }
    // The shell expands a glob starting point before `find` runs.
    let start_paths: Vec<PathBuf> = starts
        .iter()
        .flat_map(|start| word_readings(start).unwrap_or_else(|| vec![(*start).to_string()]))
        .flat_map(|start| {
            let glob = WordGlob::new(&start, cwd);
            if has_glob(&start) {
                glob.expand().unwrap_or_else(|_| vec![glob.prefix()])
            } else {
                vec![glob.pattern]
            }
        })
        .collect();
    let mut reachable = Vec::new();
    for start in &start_paths {
        match super::edit_guard::find_candidates(start, payload_cwd) {
            Ok(paths) => reachable.extend(paths),
            Err(error) => {
                return Some(hook_integrity_violation(
                    level,
                    format!("cannot read find targets: {error}"),
                ))
            }
        }
    }
    let mut at = end;
    while let Some(arg) = rest.get(at) {
        let filter = find_name_filter(&rest[..at]);
        let candidates = || {
            reachable
                .iter()
                .filter(|path| find_name_matches(filter.as_deref(), path))
        };
        match arg.as_str() {
            "-delete" => {
                for candidate in candidates() {
                    if let Some(checkout) = checkout_under(candidate, cwd, payload_cwd, None) {
                        return Some(checkout_delete_violation(
                            level,
                            "`find -delete`",
                            &checkout,
                        ));
                    }
                    let shown = crate::portable_path::slashed(candidate);
                    let reach = if crate::git::key_is_text(&shown) {
                        token_integrity_path(&shown, cwd, payload_cwd)
                    } else {
                        path_integrity(candidate, cwd, payload_cwd)
                    };
                    if let Some(p) = reach {
                        return Some(hook_integrity_violation(
                            level,
                            format!("`find -delete` would delete the integrity path `{p}`"),
                        ));
                    }
                }
            }
            "-fprint" | "-fprint0" | "-fprintf" | "-fls" => {
                if let Some(p) = rest
                    .get(at + 1)
                    .and_then(|file| token_integrity_path(file, cwd, payload_cwd))
                {
                    return Some(hook_integrity_violation(
                        level,
                        format!("`find {arg}` writes the integrity path `{p}`"),
                    ));
                }
                at += if arg == "-fprintf" { 2 } else { 1 };
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                let in_dir = matches!(arg.as_str(), "-execdir" | "-okdir");
                let tail = &rest[at + 1..];
                let len = tail
                    .iter()
                    .position(|arg| matches!(arg.as_str(), ";" | "+"))
                    .unwrap_or(tail.len());
                let command = &tail[..len];
                let literal: Vec<String> = command.iter().filter(|t| *t != "{}").cloned().collect();
                let mut dirs = vec![cwd.to_path_buf()];
                dirs.extend(start_paths.iter().cloned());
                if in_dir {
                    dirs.extend(candidates().filter_map(|c| c.parent().map(Path::to_path_buf)));
                }
                for dir in &dirs {
                    if let Some(v) = wrapped_violation(&literal, level, dir, payload_cwd, line) {
                        return Some(v);
                    }
                }
                if command.iter().any(|t| t.contains("{}")) {
                    for candidate in candidates() {
                        let (shown, dir) = match (in_dir, candidate.parent(), candidate.file_name())
                        {
                            (true, Some(parent), Some(name)) => (
                                format!("./{}", crate::portable_path::slashed(Path::new(name))),
                                parent.to_path_buf(),
                            ),
                            _ => (crate::portable_path::slashed(candidate), cwd.to_path_buf()),
                        };
                        let substituted: Vec<String> =
                            command.iter().map(|t| t.replace("{}", &shown)).collect();
                        if let Some(v) =
                            wrapped_violation(&substituted, level, &dir, payload_cwd, line)
                        {
                            return Some(v);
                        }
                    }
                }
                at += len + 1;
            }
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename" | "-regex"
            | "-iregex" | "-type" | "-xtype" | "-user" | "-group" | "-uid" | "-gid" | "-perm"
            | "-size" | "-links" | "-inum" | "-mtime" | "-mmin" | "-atime" | "-amin" | "-ctime"
            | "-cmin" | "-newer" | "-anewer" | "-cnewer" | "-newermt" | "-maxdepth"
            | "-mindepth" | "-printf" | "-files0-from" => at += 1,
            _ => {}
        }
        at += 1;
    }
    None
}

/// The command an `xargs` runs, GNU and BSD options read.
fn xargs_command(args: &[String]) -> Option<&[String]> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            at += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            if value.is_none()
                && [
                    "arg-file",
                    "delimiter",
                    "max-args",
                    "max-procs",
                    "max-chars",
                    "process-slot-var",
                ]
                .iter()
                .any(|full| full.starts_with(name) && name.len() >= 3)
            {
                at += 1;
            }
            at += 1;
            continue;
        }
        let cluster = &arg[1..];
        for (offset, letter) in cluster.char_indices() {
            let attached = &cluster[offset + letter.len_utf8()..];
            match letter {
                'I' | 'J' | 'a' | 'd' | 'E' | 'L' | 'n' | 'P' | 's' | 'R' | 'S' => {
                    if attached.is_empty() {
                        at += 1;
                    }
                    break;
                }
                'i' | 'e' | 'l' => break,
                _ => {}
            }
        }
        at += 1;
    }
    args.get(at..).filter(|c| !c.is_empty())
}

/// An `xargs` takes its paths from its input, which the guard cannot see.
/// Its command is judged as written, and one that changes files is refused
/// when the command line names an enforcement path or a worktree folder,
/// as a producer feeding it would (TSK-216 review round 2). With nothing
/// protected in sight it passes: `find build -print0 | xargs -0 rm`.
fn xargs_violation(
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let command = xargs_command(args)?;
    if read_only_program(command) {
        return None;
    }
    if let Some(v) = wrapped_violation(command, level, cwd, payload_cwd, line) {
        return Some(v);
    }
    let named =
        line_names(line, cwd, payload_cwd).or_else(|| worktree_text(line).map(str::to_string))?;
    Some(hook_integrity_violation(
        level,
        format!(
            "`xargs {}` changes the paths it reads from its input, and the command line names `{named}`; name the files on the command line",
            command.join(" ")
        ),
    ))
}

/// A recursive `rm`, or any `chmod`, `chown`, `chgrp` or `chflags`, whose
/// target holds enforcement files changes them from any checkout: `chmod
/// -R 000 .` in a linked worktree reaches its own policy and settings, and
/// `chmod 000 .` makes them unreadable without touching them (TSK-216
/// round 3). A non-recursive `rm` of a checkout root, such as `rm -f .`,
/// reaches nothing below it.
fn recursive_change_violation(
    cmd: &str,
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let recursive = match cmd {
        "rm" => rm_recursive(args),
        "chmod" | "chown" | "chgrp" | "chflags" => true,
        _ => false,
    };
    if !recursive {
        return None;
    }
    for target in rm_operands(args)
        .into_iter()
        .filter(|target| !target.is_empty())
    {
        let Some(paths) = resolve_targets(target, cwd, line) else {
            continue;
        };
        for path in &paths {
            for root in [cwd, payload_cwd] {
                match super::edit_guard::holds_enforcement_files(path, root) {
                    Ok(true) => return Some(hook_integrity_violation(level, format!("`{cmd}` would change `{target}`, which holds enforcement files, and what lies below it"))),
                    Ok(false) => {},
                    Err(error) => return Some(hook_integrity_violation(level, format!("cannot read recursive-change targets: {error}"))),
                }
            }
        }
    }
    None
}

/// GNU `parallel` runs a command over its input as `xargs` does; its
/// command is the first word that is not an option, and it is judged as
/// `xargs` judges its command.
fn parallel_violation(
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let start = args.iter().position(|arg| !arg.starts_with('-'))?;
    let command = &args[start..];
    if read_only_program(command) {
        return None;
    }
    if let Some(v) = wrapped_violation(command, level, cwd, payload_cwd, line) {
        return Some(v);
    }
    let named =
        line_names(line, cwd, payload_cwd).or_else(|| worktree_text(line).map(str::to_string))?;
    Some(hook_integrity_violation(
        level,
        format!(
            "`parallel {}` changes the paths it reads from its input, and the command line names `{named}`; name the files on the command line",
            command.join(" ")
        ),
    ))
}

/// The deletes of worktrees and the wrapped or scripted writes the text
/// floor judges (TSK-216): `rm -r`, `trash`, `git clean -ff`, `find`,
/// `xargs` and `sed`.
fn wrapper_write_violation(
    cmd: &str,
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let deleted = match cmd {
        "rm" if rm_recursive(args) => Some("`rm -r`"),
        "trash" | "trash-put" => Some("`trash`"),
        _ => None,
    };
    if let Some(what) = deleted {
        if let Some(v) = worktree_delete_check(
            what,
            &rm_operands(args),
            level,
            cwd,
            payload_cwd,
            line,
            None,
        ) {
            return Some(v);
        }
    }
    if cmd == "git" {
        // `git [global options] clean`, as a wrapped or plain command; a
        // top-level git line is also judged through its aliases and
        // retargets in `check_git`.
        if let Some(("clean", rest)) = git_subcommand(args) {
            let mut dir = cwd.to_path_buf();
            let globals = &args[..args.len() - rest.len() - 1];
            for pair in globals.windows(2) {
                if pair[0] == "-C" {
                    dir = integrity_shell_path(&pair[1], &dir);
                }
            }
            if let Some(v) = git_clean_violation(rest, level, &dir, payload_cwd, line) {
                return Some(v);
            }
        }
    }
    if let Some(v) = recursive_change_violation(cmd, args, level, cwd, payload_cwd, line) {
        return Some(v);
    }
    if cmd == "find" {
        if let Some(v) = find_action_violation(args, level, cwd, payload_cwd, line) {
            return Some(v);
        }
    }
    if cmd == "parallel" {
        if let Some(v) = parallel_violation(args, level, cwd, payload_cwd, line) {
            return Some(v);
        }
    }
    if cmd == "xargs" {
        if let Some(v) = xargs_violation(args, level, cwd, payload_cwd, line) {
            return Some(v);
        }
    }
    if cmd == "sed" {
        if let Some(v) = sed_text_violation(args, level, cwd, payload_cwd, line) {
            return Some(v);
        }
    }
    None
}

/// Block a Bash write/remove that would disarm or falsify the enforcement
/// plane: a redirect into, or a mutating command targeting, the hook shims
/// (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
/// (`.codeflow/policy.json`, `.codeflow/project.toml`). Reads (`cat`, a `cp`
/// *from* an integrity path) stay allowed. `redirects` are the targets the
/// command's redirections write ([`redirect_writes`]); a command run by
/// `xargs` or `find -exec` has none.
fn integrity_write_violation(
    tokens: &[String],
    redirects: &Redirects,
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    if let Some(p) = redirects
        .targets
        .iter()
        .chain(&redirects.unread)
        .find_map(|t| token_integrity_path(t, cwd, payload_cwd))
    {
        return Some(hook_integrity_violation(
            level,
            format!("redirect would overwrite the integrity path `{p}`"),
        ));
    }
    // A command whose quoting the redirection reader does not read: any
    // word that could name an enforcement path from some directory may be
    // what it writes (TSK-216 round 12).
    if let Some((word, p)) = redirects
        .unread
        .iter()
        .find_map(|w| word_could_name(w).map(|p| (w, p)))
    {
        return Some(hook_integrity_violation(
            level,
            format!(
                "a redirect on a command with `$'...'` or `$\"...\"` quoting, which the guard does not read, and `{}` could name `{p}`",
                shown_word(word)
            ),
        ));
    }
    let (program, args) = strip_launchers(tokens)?;
    let cmd = basename(program);
    // A launcher's own effects apply to the command it runs: `env -C DIR`
    // moves its directory, and a launcher the guard cannot read with
    // certainty is not dropped silently (TSK-216 round 4).
    let (dirs, uncertain) = launcher_effects(tokens);
    let launched_cwd = dirs.iter().fold(cwd.to_path_buf(), |dir, change| {
        integrity_shell_path(change, &dir)
    });
    let cwd = launched_cwd.as_path();
    // The direct checks 3.0.0 made run first, so a command they refuse keeps
    // their reading, the remote-tracking authority class included, which
    // stays blocked when hook integrity is relaxed. The new hardening only
    // adds refusals after them (TSK-216).
    direct_write_violation(cmd, args, level, cwd, payload_cwd, line)
        .or_else(|| {
            let why = uncertain.filter(|_| !read_only_program(tokens))?;
            Some(hook_integrity_violation(
                level,
                format!("the guard cannot read the launcher in front of `{cmd}` ({why}), so it cannot tell what that command changes"),
            ))
        })
        .or_else(|| wrapper_write_violation(cmd, args, level, cwd, payload_cwd, line))
}

/// The writes 3.0.0 judged directly: a mutating command's own path
/// arguments, `dd of=`, `sed -i` files and script writes, the destination
/// of `cp`, `ln` and `rsync`, and `git rm` or `git mv`.
fn direct_write_violation(
    cmd: &str,
    args: &[String],
    level: PolicyLevel,
    cwd: &Path,
    payload_cwd: &Path,
    line: &str,
) -> Option<Violation> {
    let write_args = match cmd {
        "find" => find_mutating_roots(args),
        "rm" | "unlink" | "mv" | "tee" | "truncate" | "shred" | "chmod" | "chown" | "install" => {
            Some(args)
        }
        _ => None,
    };
    if let Some(p) = write_args.and_then(|paths| {
        if paths.is_empty() && cmd == "find" {
            token_integrity_path(".", cwd, payload_cwd).map(str::to_string)
        } else {
            arg_integrity_path(paths, cwd, payload_cwd)
                .map(str::to_string)
                .or_else(|| unresolved_names_enforcement(paths, line, cwd, payload_cwd))
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
        if let Some(p) = sed_file_operands(args)
            .into_iter()
            .find_map(|file| token_integrity_path(file, cwd, payload_cwd))
        {
            return Some(hook_integrity_violation(
                level,
                format!("`sed -i` edits the integrity path `{p}`"),
            ));
        }
    }
    if cmd == "sed" {
        match sed_script_writes(args, cwd) {
            SedRead::Read(paths) => {
                if let Some(p) = paths
                    .iter()
                    .find_map(|path| token_integrity_path(path, cwd, payload_cwd))
                {
                    return Some(hook_integrity_violation(
                        level,
                        format!(
                            "a `sed` script `w` command or backup writes the integrity path `{p}`"
                        ),
                    ));
                }
            }
            SedRead::Unreadable { file, why } => {
                return Some(hook_integrity_violation(
                    level,
                    format!("cannot read `sed` script `{file}`: {why}"),
                ))
            }
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
            // It runs in the directory its launchers move to (`env -C DIR
            // sh -c ...`), which the body carries as its own leading `cd`
            // so every check that follows directories sees it (TSK-216
            // round 5).
            if let Some(inner) = shell_c_argument(args) {
                let mut moves = String::new();
                for dir in launcher_effects(&toks).0 {
                    moves.push_str("cd '");
                    moves.push_str(&dir.replace('\'', "'\\''"));
                    moves.push_str("' && ");
                }
                split_into_segments(&format!("{moves}{inner}"), &mut out, 1, false);
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
/// boundaries. Parentheses zsh reads as part of a word ([`paren_in_word`]:
/// `word(D)`, `@(a)`, `rm (a|b)/x`) stay in that word, and the text inside
/// them is judged as commands as well.
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
    if depth > NESTING_LIMIT {
        // Text nested deeper than the guard reads is never passed as read:
        // the marker segment refuses the line (TSK-216 round 19).
        if !out.iter().any(|s| s == NESTING_UNREAD) {
            out.push(NESTING_UNREAD.to_string());
        }
        return;
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
    // Open parentheses attached to a word with no blank before them: a
    // zsh glob qualifier or group, or an extglob group (`word(D)`,
    // `@(a)`), which is part of the word, never a subshell (TSK-216
    // round 17).
    let mut word_parens = 0usize;
    // The end of the word group last judged as commands: a group nested
    // inside it was judged by that call, so each character is read once per
    // nesting level, never once per enclosing group (TSK-216 round 19).
    let mut judged_until = 0usize;
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
            '#' if arithmetic == 0
                && bracket_arithmetic == 0
                && word_parens == 0
                && starts_word(&chars, i) =>
            {
                // A comment runs to the end of the line; the newline itself
                // still ends the segment and starts any heredoc bodies.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\n' => {
                word_parens = 0;
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
            '(' if paren_in_word(&cur) => {
                // zsh reads it as part of the word (a qualifier, group or
                // pattern), and Bash after a keyword as a subshell
                // (`if(rm x)`), so the text inside is also judged as
                // commands, and so is the code of a zsh `e` or `+`
                // qualifier (TSK-216 round 18).
                if i >= judged_until {
                    let (inner, end) = capture_word_group(&chars, i + 1);
                    split_into_segments(&inner, out, depth + 1, code_context);
                    for code in qualifier_code(&inner) {
                        split_into_segments(&code, out, depth + 1, code_context);
                    }
                    judged_until = end;
                }
                word_parens += 1;
                cur.push(c);
                i += 1;
            }
            ')' if word_parens > 0 => {
                word_parens -= 1;
                cur.push(c);
                i += 1;
            }
            // A group or process substitution in command position belongs to
            // the pipeline it sits in (`cat <<EOF | { bash; }`, `>(sh)`).
            '(' => {
                let in_pipeline =
                    cur.trim_matches(shell_blank).is_empty() || cur.ends_with(['<', '>']);
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
                word_parens = 0;
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
            '{' if i + 1 >= chars.len() || shell_blank(chars[i + 1]) => {
                let in_pipeline = cur.trim_matches(shell_blank).is_empty();
                if in_pipeline {
                    line.grouped.push(line.pipeline);
                }
                line.end_segment(out, &mut cur, in_pipeline);
                groups += 1;
                i += 1;
            }
            '}' if i == 0 || shell_blank(chars[i - 1]) => {
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
                word_parens = 0;
                line.end_segment(out, &mut cur, false);
                i += if chars.get(i + 1) == Some(&'&') { 2 } else { 1 };
            }
            // `>|` is the clobber-redirect operator, not a pipe — keep it.
            '|' if i > 0 && chars[i - 1] == '>' => {
                cur.push(c);
                i += 1;
            }
            '|' => {
                word_parens = 0;
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
        // Only the shell's blanks are trimmed: a command ends in a no-break
        // space when its last word does (OS text rule, issue 79).
        let text = text.trim_matches([' ', '\t', '\n']);
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

/// The deepest nesting of substitutions, shell bodies and word groups the
/// command reader follows; deeper text refuses the line ([`NESTING_UNREAD`]).
const NESTING_LIMIT: usize = 8;

/// The segment that stands for text nested deeper than [`NESTING_LIMIT`].
/// It is not a command any shell runs, and the integrity check refuses it.
pub(crate) const NESTING_UNREAD: &str = "\u{1}codeflow: nested deeper than the guard reads";

/// Words that leave the shell in command position, so a `(` after them
/// opens a subshell or a `case` pattern.
const COMMAND_POSITION_WORDS: &[&str] = &[
    "!", "{", "case", "coproc", "do", "elif", "else", "foreach", "function", "if", "in", "repeat",
    "select", "then", "time", "until", "while", "[[",
];

/// Whether a `(` that follows `cur` is read as part of a word: attached to
/// the word `cur` ends with (`word(D)`, `@(a)`), or in argument position
/// after a command word (`rm (a|b)/x`, a zsh pattern). zsh reads both as
/// pattern syntax. A `(` that opens the command, follows only keywords
/// (`if (`, `then (`) or starts a `case` line keeps the subshell reading,
/// and so does `<(` or `>(` written together, a process substitution; the
/// caller also judges the text inside a word's parentheses as commands
/// (TSK-216 rounds 17 and 18).
fn paren_in_word(cur: &str) -> bool {
    let attached = cur.chars().last().is_some_and(|c| !shell_blank(c));
    let trimmed = cur.trim_end_matches(shell_blank);
    let Some(last) = trimmed.chars().last() else {
        return false;
    };
    if matches!(last, '<' | '>' | '|' | '&') {
        return !attached;
    }
    let mut words = trimmed.split(shell_blank).filter(|word| !word.is_empty());
    let first = words.next().unwrap_or_default();
    first != "case"
        && !std::iter::once(first)
            .chain(words)
            .all(|w| COMMAND_POSITION_WORDS.contains(&w))
}

/// The text inside the parentheses opened before `start`, up to the
/// matching `)`, with quotes and escapes kept; and the index just past it.
fn capture_word_group(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 1;
    let mut text = String::new();
    let (mut single, mut double) = (false, false);
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' if !single => {
                text.push(c);
                if let Some(&next) = chars.get(i + 1) {
                    text.push(next);
                }
                i += 2;
                continue;
            }
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '(' if !single && !double => depth += 1,
            ')' if !single && !double => {
                depth -= 1;
                if depth == 0 {
                    return (text, i + 1);
                }
            }
            _ => {}
        }
        text.push(c);
        i += 1;
    }
    (text, i)
}

/// The shell code a zsh glob qualifier list runs: the string of an `e`
/// qualifier between its delimiters (`e:code:`, `e[code]`), and the
/// function or code an `+` qualifier names, read after quote removal.
/// Any `e` followed by a delimiter counts, so this can only read more
/// than zsh runs (TSK-216 round 18).
fn qualifier_code(group: &str) -> Vec<String> {
    let text: String = shell_tokens(group).join(" ");
    let chars: Vec<char> = text.chars().collect();
    let mut code = Vec::new();
    for (at, &c) in chars.iter().enumerate() {
        if c == 'e' {
            let Some(&open) = chars.get(at + 1) else {
                continue;
            };
            if open.is_alphanumeric() || shell_blank(open) {
                continue;
            }
            let close = match open {
                '(' => ')',
                '[' => ']',
                '{' => '}',
                '<' => '>',
                other => other,
            };
            if let Some(end) = chars[at + 2..].iter().position(|&d| d == close) {
                code.push(chars[at + 2..at + 2 + end].iter().collect());
            }
        } else if c == '+' {
            let rest: String = chars[at + 1..]
                .iter()
                .take_while(|&&d| !matches!(d, ',' | ')' | ':'))
                .collect();
            if !rest.trim_matches(shell_blank).is_empty() {
                code.push(rest);
            }
        }
    }
    code
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
                c if shell_blank(c) || ";&|<>()".contains(c) => break,
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
    /// A git command earlier in the line may have moved HEAD or changed an
    /// upstream, so a branch expression (`@{-1}`, `@{upstream}`) read from
    /// disk now may not be what git resolves when it runs.
    head_may_move: bool,
    /// The whole command line, for words the shell fills in.
    line: String,
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
            head_may_move: false,
            line: String::new(),
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
        && git_subcommand(args).is_some_and(|(sub, rest)| {
            matches!(sub, "fetch" | "pull" | "push")
                || super::ref_authority::remote_transport_args(sub, rest).is_some()
        })
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
        branches.head_may_move = true;
        return;
    };
    let head_moved_before = branches.head_may_move;
    branches.head_may_move |= !discard_readonly_git(sub, rest);

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

    // A double-force clean reached through global options, an alias or a
    // retarget deletes worktrees as a plain one does (TSK-216 round 3).
    if sub == "clean" && ctx.policy.hook_integrity.is_active() {
        if let Ok(specs) = compose_targets(args, moved) {
            for spec in specs {
                let dir = spec
                    .as_ref()
                    .map_or_else(|| cwd.to_path_buf(), |s| cwd.join(&s.path));
                if let Some(v) =
                    git_clean_violation(rest, ctx.policy.hook_integrity, &dir, cwd, &branches.line)
                {
                    out.push(v);
                    return;
                }
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
    // A plain checkout only moves HEAD; one that force-creates a branch
    // resets that branch and is judged below.
    if matches!(sub, "checkout" | "switch") && forced_branch_target(sub, rest).is_none() {
        return;
    }
    // A branch expression is judged as the branch git resolves it to in the
    // targeted repository; one the guard cannot resolve is refused (TSK-216).
    let resolved_rest;
    let rest = match resolve_forced_branch(sub, rest, args, moved, ctx, head_moved_before) {
        Ok(Some(resolved)) => {
            resolved_rest = resolved;
            resolved_rest.as_slice()
        }
        Ok(None) => rest,
        Err(why) => {
            if ctx.policy.local_ref_protection.is_active() && !ctx.integrate_token {
                out.push(Violation::new(
                    "git.local_ref_protection",
                    ctx.policy.local_ref_protection,
                    format!("`git {sub}` would force a branch the guard cannot identify: {why}; it is judged as a protected branch"),
                    crate::remedy::PROTECTED_BRANCH.remedy(),
                ));
            }
            rest
        }
    };

    let mut found: Vec<Violation> = Vec::new();
    for (branch, rules, root) in &judged.cases {
        let view = GuardContext {
            policy: rules,
            current_branch: ctx.current_branch,
            integrate_token: ctx.integrate_token,
            pr_base_lookup: ctx.pr_base_lookup,
            dir_target_lookup: ctx.dir_target_lookup,
            alias_lookup: ctx.alias_lookup,
            branch_lookup: ctx.branch_lookup,
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
        if value.starts_with('!') {
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
pub(crate) fn split_alias(value: &str) -> Option<Vec<String>> {
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
            // Git's sane_ctype blanks exclude vertical tab and form feed.
            (None, ' ' | '\t' | '\n' | '\r') => {
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
            } else if let Some(target) = forced_branch_target(sub, rest) {
                if policy.branch_is_protected(target) {
                    push_protected_move(sub, target, ctx, out);
                }
            }
        }
        "checkout" | "switch" | "worktree" => {
            if let Some(target) = forced_branch_target(sub, rest) {
                if policy.branch_is_protected(target) {
                    push_protected_move(sub, target, ctx, out);
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

/// The branch a forcing command sets to a new commit, as written: a `git
/// branch` forced create (`-f`/`--force`, whatever formatting flags such as
/// `-v` sit beside it) or forced rename or copy onto it (`-M`, `-C`, or
/// `-m`/`-c` with `--force`), a `git checkout -B <name>`, and a `git switch
/// -C|--force-create <name>`, long options abbreviated as git accepts them.
/// These rewrite the ref outside the sanctioned path, as `git update-ref
/// refs/heads/<protected>` does. A rename or copy without force fails in
/// git when the branch exists, so it can only create the branch. The
/// listing, upstream and description forms move no ref, and `-a`/`-r` with
/// a name make git refuse.
fn forced_branch_target<'a>(sub: &str, rest: &'a [String]) -> Option<&'a str> {
    match sub {
        "branch" => {
            if requests_branch_delete(rest) {
                return None;
            }
            let parsed = parse_options(rest, &GIT_BRANCH_OPTIONS);
            let force = parsed.has_short(&['f', 'M', 'C']) || parsed.has_long("--force");
            if !force {
                return None;
            }
            let operands = &parsed.operands;
            if parsed.has_short(&['m', 'M', 'c', 'C'])
                || parsed.has_long("--move")
                || parsed.has_long("--copy")
            {
                // `-M <new>` renames the current branch; `-M <old> <new>`
                // names both.
                return operands.get(1).or_else(|| operands.first()).copied();
            }
            let other_mode = parsed.has_short(&['u', 'l', 'a', 'r'])
                || [
                    "--set-upstream-to",
                    "--unset-upstream",
                    "--edit-description",
                    "--list",
                    "--all",
                    "--remotes",
                    "--show-current",
                    "--contains",
                    "--no-contains",
                    "--merged",
                    "--no-merged",
                    "--points-at",
                ]
                .iter()
                .any(|name| parsed.has_long(name));
            if other_mode {
                None
            } else {
                operands.first().copied()
            }
        }
        "checkout" => parse_options(rest, &GIT_CHECKOUT_OPTIONS)
            .values_of('B', "")
            .last()
            .copied(),
        "switch" => parse_options(rest, &GIT_SWITCH_OPTIONS)
            .values_of('C', "--force-create")
            .last()
            .copied(),
        "worktree" => {
            if rest.first().map(String::as_str) != Some("add") {
                return None;
            }
            parse_options(&rest[1..], &GIT_WORKTREE_ADD_OPTIONS)
                .values_of('B', "")
                .last()
                .copied()
        }
        _ => None,
    }
}

/// `git worktree add` (git-worktree(1)), for the branch `-B` resets.
const GIT_WORKTREE_ADD_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('f', Arity::Flag),
        ('d', Arity::Flag),
        ('q', Arity::Flag),
    ],
    long: &[
        ("--force", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--checkout", Arity::Flag),
        ("--no-checkout", Arity::Flag),
        ("--lock", Arity::Flag),
        ("--reason", Arity::Value),
        ("--orphan", Arity::Flag),
        ("--track", Arity::Flag),
        ("--no-track", Arity::Flag),
        ("--guess-remote", Arity::Flag),
        ("--no-guess-remote", Arity::Flag),
        ("--relative-paths", Arity::Flag),
        ("--no-relative-paths", Arity::Flag),
        ("--quiet", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git switch` (git-switch(1)), for the branch `-C`/`--force-create`
/// resets. Its long options take any unambiguous prefix (`--force-c`).
const GIT_SWITCH_OPTIONS: OptionSpec = OptionSpec {
    short: &[('c', Arity::Value), ('C', Arity::Value)],
    long: &[
        ("--create", Arity::Value),
        ("--force-create", Arity::Value),
        ("--orphan", Arity::Value),
        ("--conflict", Arity::Value),
        ("--track", Arity::AttachedValue),
        ("--no-track", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--guess", Arity::Flag),
        ("--no-guess", Arity::Flag),
        ("--force", Arity::Flag),
        ("--discard-changes", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--ignore-other-worktrees", Arity::Flag),
        ("--overwrite-ignore", Arity::Flag),
        ("--no-overwrite-ignore", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git checkout` (git-checkout(1)), for the branch `-B` resets.
const GIT_CHECKOUT_OPTIONS: OptionSpec = OptionSpec {
    short: &[('b', Arity::Value), ('B', Arity::Value)],
    long: &[
        ("--orphan", Arity::Value),
        ("--conflict", Arity::Value),
        ("--pathspec-from-file", Arity::Value),
        ("--track", Arity::AttachedValue),
        ("--no-track", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--guess", Arity::Flag),
        ("--no-guess", Arity::Flag),
        ("--force", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--patch", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--ours", Arity::Flag),
        ("--theirs", Arity::Flag),
        ("--overlay", Arity::Flag),
        ("--no-overlay", Arity::Flag),
        ("--ignore-skip-worktree-bits", Arity::Flag),
        ("--ignore-other-worktrees", Arity::Flag),
        ("--overwrite-ignore", Arity::Flag),
        ("--no-overwrite-ignore", Arity::Flag),
        ("--pathspec-file-nul", Arity::Flag),
        (END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// Whether git expands `name` before using it as a branch name: `@{-N}`,
/// `<branch>@{upstream}` and the other `@` forms, and `-` for the previous
/// branch. A plain name is the branch it names, and `refs/heads/main` or
/// `origin/main` create branches with those names, so they are not
/// rewritten.
fn needs_branch_resolution(name: &str) -> bool {
    name == "-" || name.contains('@')
}

/// Resolve the branch a forcing command names, when git would expand it,
/// in each repository the command targets. `Ok(None)` when nothing needs
/// resolving; `Ok(Some(rest))` with the expression replaced by the branch
/// git would change; `Err` when it cannot be resolved with certainty.
fn resolve_forced_branch(
    sub: &str,
    rest: &[String],
    args: &[String],
    moved: &Moves<'_>,
    ctx: &GuardContext<'_>,
    head_moved_before: bool,
) -> Result<Option<Vec<String>>, String> {
    let Some(name) = forced_branch_target(sub, rest).filter(|n| needs_branch_resolution(n)) else {
        return Ok(None);
    };
    // What `@{-1}` or `@{upstream}` names depends on what ran before it on
    // the line; the guard reads the repository as it is now (TSK-216).
    if head_moved_before {
        return Err(format!(
            "an earlier git command on this line may change what `{name}` names"
        ));
    }
    let lookup = ctx
        .branch_lookup
        .ok_or_else(|| format!("cannot resolve the branch expression `{name}`"))?;
    let specs = compose_targets(args, moved)?;
    let mut resolved: Option<String> = None;
    for spec in &specs {
        let target = spec.as_ref().map(|s| Retarget {
            path: &s.path,
            git_dir: s.git_dir,
        });
        let branch = lookup(target.as_ref(), name)
            .map_err(|why| format!("cannot resolve the branch expression `{name}`: {why}"))?;
        match &resolved {
            Some(other) if *other != branch => {
                return Err(format!(
                    "the branch expression `{name}` names different branches in the targeted repositories"
                ));
            }
            _ => resolved = Some(branch),
        }
    }
    let Some(branch) = resolved else {
        return Ok(None);
    };
    // Replace the expression inside the token that carries it, attached or
    // on its own.
    let at = name.as_ptr() as usize;
    Ok(Some(
        rest.iter()
            .map(|token| {
                let start = token.as_ptr() as usize;
                if (start..start + token.len()).contains(&at) {
                    let offset = at - start;
                    format!(
                        "{}{branch}{}",
                        &token[..offset],
                        &token[offset + name.len()..]
                    )
                } else {
                    token.clone()
                }
            })
            .collect(),
    ))
}

fn push_protected_move(sub: &str, target: &str, ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    if policy.local_ref_protection.is_active() && !ctx.integrate_token {
        out.push(Violation::new(
            "git.local_ref_protection",
            policy.local_ref_protection,
            format!("`git {sub}` would force protected branch '{target}' to a new commit outside the sanctioned path"),
            crate::remedy::PROTECTED_BRANCH.remedy(),
        ));
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
/// Both inline and file bodies are read before allowing publication.
fn check_gh_pr_body(rest: &[&str], policy: &GitPolicy, out: &mut Vec<Violation>) {
    if let Some(body) = flag_value(rest, &["--body", "-b"]) {
        scan_pr_body(body, policy, out);
    }
    if let Some(path) = flag_value(rest, &["--body-file", "-F"]) {
        let body = if path == "-" {
            Err("cannot read PR body from stdin".to_string())
        } else {
            std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read PR body {path}: {error}"))
        };
        match body {
            Ok(body) => scan_pr_body(&body, policy, out),
            Err(error) if policy.ai_attribution.is_active() || policy.commit_emoji.is_active() => {
                out.push(Violation::new(
                    "git.pr_body",
                    PolicyLevel::Block,
                    error,
                    crate::remedy::Remedy::sanctioned(
                        "use a readable UTF-8 body file or an inline body",
                    ),
                ));
            }
            Err(_) => {}
        }
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
    /// The command accepts any unambiguous prefix of a long option, as Git's
    /// parse-options and GNU `getopt_long` (GNU `sed`) do, and
    /// `--end-of-options` as a terminator when it lists it. BSD `sed` and
    /// the `gh` commands (pflag) do not: for them only the written long name
    /// counts.
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

/// GNU `sed`: `-e` a script, `-f` a script file, `-l` a line length; `-i`
/// takes the backup suffix only when attached (`-i.bak`), so a separate
/// token stays an operand. Its long options go through `getopt_long`, which
/// accepts any unambiguous prefix (`--in-pl`), so every GNU long option is
/// listed for the prefix to be judged against.
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
        ("--quiet", Arity::Flag),
        ("--silent", Arity::Flag),
        ("--debug", Arity::Flag),
        ("--follow-symlinks", Arity::Flag),
        ("--posix", Arity::Flag),
        ("--regexp-extended", Arity::Flag),
        ("--separate", Arity::Flag),
        ("--sandbox", Arity::Flag),
        ("--unbuffered", Arity::Flag),
        ("--null-data", Arity::Flag),
        ("--zero-terminated", Arity::Flag),
        ("--binary", Arity::Flag),
        ("--help", Arity::Flag),
        ("--version", Arity::Flag),
    ],
    git_style: true,
};

/// BSD `sed` (macOS): `-i` and `-I` edit in place and always take the
/// backup suffix, attached or as the next token, so `sed -i '' s/a/b/ file`
/// edits `file` with no backup. `-l` is a flag (line-buffered output), and
/// there are no long options.
const BSD_SED_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('e', Arity::Value),
        ('f', Arity::Value),
        ('i', Arity::Value),
        ('I', Arity::Value),
    ],
    long: &[],
    git_style: false,
};

/// The two `sed` grammars. The guard cannot tell which `sed` runs, so a
/// command line is read both ways and what either reading writes counts.
const SED_GRAMMARS: [&OptionSpec; 2] = [&SED_OPTIONS, &BSD_SED_OPTIONS];

/// Does this `sed` invocation edit its input in place, in either grammar?
fn requests_in_place(args: &[String]) -> bool {
    let gnu = parse_options(args, &SED_OPTIONS);
    let bsd = parse_options(args, &BSD_SED_OPTIONS);
    gnu.has_short(&['i']) || gnu.has_long("--in-place") || bsd.has_short(&['i', 'I'])
}

/// The files an in-place `sed` may write, in either grammar. The script
/// operand, the backup suffix and the option values are never files.
fn sed_file_operands(args: &[String]) -> Vec<&str> {
    let mut files = Vec::new();
    for spec in SED_GRAMMARS {
        for operand in sed_operands_in(args, spec) {
            if !files.contains(&operand) {
                files.push(operand);
            }
        }
    }
    files
}

/// The paths a `sed` script may write, whether or not it edits in place:
/// the destination of each `w` and `W` command and of the `w` flag of `s`,
/// which runs to the end of its line. Every argument is scanned, so a
/// script in `-e`, `--expression` or the script operand is covered, and so
/// is a readable `-f` script file. Each `w` yields the rest of its line,
/// and that text cut at `;` and `}`, as candidates; a candidate that is not
/// an enforcement path is harmless, so over-reading never refuses a
/// legitimate script. The backup an in-place edit writes is a candidate
/// too: the file name plus its suffix, or a GNU suffix with `*` replaced
/// by the file name (`-i'dir/*'`).
#[derive(Debug)]
enum SedRead<T> {
    Read(T),
    Unreadable { file: String, why: String },
}

fn read_sed_script(file: &str, cwd: &Path) -> SedRead<String> {
    use std::io::Read;
    let unreadable = |why: String| SedRead::Unreadable {
        file: file.into(),
        why,
    };
    let metadata = match std::fs::metadata(cwd.join(file)) {
        Ok(metadata) => metadata,
        Err(error) => return unreadable(error.to_string()),
    };
    if !metadata.is_file() {
        return unreadable("not a regular script file".into());
    }
    if metadata.len() > SED_SCRIPT_LIMIT {
        return unreadable(format!("exceeds {SED_SCRIPT_LIMIT} byte limit"));
    }
    let input = match std::fs::File::open(cwd.join(file)) {
        Ok(input) => input,
        Err(error) => return unreadable(error.to_string()),
    };
    match input.metadata() {
        Ok(metadata) if !metadata.is_file() => {
            return unreadable("script is no longer a regular file".into())
        }
        Ok(metadata) if metadata.len() > SED_SCRIPT_LIMIT => {
            return unreadable(format!("exceeds {SED_SCRIPT_LIMIT} byte limit"))
        }
        Ok(_) => {}
        Err(error) => return unreadable(error.to_string()),
    }
    let mut bytes = Vec::new();
    if let Err(error) = input.take(SED_SCRIPT_LIMIT + 1).read_to_end(&mut bytes) {
        return unreadable(error.to_string());
    }
    if bytes.len() as u64 > SED_SCRIPT_LIMIT {
        return unreadable(format!("exceeds {SED_SCRIPT_LIMIT} byte limit"));
    }
    match String::from_utf8(bytes) {
        Ok(text) => SedRead::Read(text),
        Err(error) => unreadable(format!("not valid UTF-8: {error}")),
    }
}

fn sed_script_writes(args: &[String], cwd: &Path) -> SedRead<Vec<String>> {
    let mut scripts: Vec<String> = args.to_vec();
    for spec in SED_GRAMMARS {
        let parsed = parse_options(args, spec);
        for file in parsed.values_of('f', "--file") {
            match read_sed_script(file, cwd) {
                SedRead::Read(text) => scripts.push(text),
                SedRead::Unreadable { file, why } => return SedRead::Unreadable { file, why },
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut add = |candidate: &str| {
        let candidate = candidate.trim_start_matches([' ', '\t']);
        if !candidate.is_empty() && !out.iter().any(|c| c == candidate) {
            out.push(candidate.to_string());
        }
    };
    for script in &scripts {
        for line in script.split('\n') {
            for (at, _) in line.match_indices(['w', 'W']) {
                let rest = &line[at + 1..];
                add(rest);
                add(rest.split(';').next().unwrap_or(rest));
                add(rest.split('}').next().unwrap_or(rest));
            }
        }
    }
    let gnu = parse_options(args, &SED_OPTIONS);
    let bsd = parse_options(args, &BSD_SED_OPTIONS);
    let mut suffixes = gnu.values_of('i', "--in-place");
    suffixes.extend(bsd.values_of('i', ""));
    suffixes.extend(bsd.values_of('I', ""));
    for suffix in suffixes.iter().filter(|s| !s.is_empty()) {
        for file in sed_file_operands(args) {
            if suffix.contains('*') {
                let name = Path::new(file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file);
                let backup = suffix.replace('*', name);
                add(&backup);
                if let Some(dir) = Path::new(file)
                    .parent()
                    .filter(|d| !d.as_os_str().is_empty())
                {
                    add(&crate::portable_path::slashed(&dir.join(&backup)));
                }
            } else {
                add(&format!("{file}{suffix}"));
            }
        }
    }
    SedRead::Read(out)
}

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
    /// The value each value-taking option was given, attached or from the
    /// next token, in encounter order.
    values: Vec<(Seen<'a>, &'a str)>,
}

impl<'a> ParsedOptions<'a> {
    fn has_short(&self, letters: &[char]) -> bool {
        self.short.iter().any(|letter| letters.contains(letter))
    }

    /// The values given to the short option `letter` or the long option
    /// `name` (canonical), in encounter order.
    fn values_of(&self, letter: char, name: &str) -> Vec<&'a str> {
        self.values
            .iter()
            .filter(|(seen, _)| match seen {
                Seen::Short(l) => *l == letter,
                Seen::Long(n, _) => *n == name,
            })
            .map(|(_, value)| *value)
            .collect()
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
        values: Vec::new(),
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
            if let Some(value) = attached {
                parsed.values.push((Seen::Long(name, attached), value));
            } else if canonical.is_some_and(|n| matches!(spec.long_arity(n), Arity::Value)) {
                if let Some(value) = args.get(index) {
                    parsed.values.push((Seen::Long(name, None), value.as_ref()));
                }
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
            let rest = &cluster[offset + letter.len_utf8()..];
            match spec.short_arity(letter) {
                Arity::Flag => {}
                Arity::AttachedValue => {
                    if !rest.is_empty() {
                        parsed.values.push((Seen::Short(letter), rest));
                    }
                    break;
                }
                Arity::Value => {
                    if rest.is_empty() {
                        if let Some(value) = args.get(index) {
                            parsed.values.push((Seen::Short(letter), value.as_ref()));
                        }
                        index += 1;
                    } else {
                        parsed.values.push((Seen::Short(letter), rest));
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
                // Inside double quotes a backslash escapes only `$`, a
                // backquote, `"` and `\`; before anything else it stays, so
                // a quoted Windows path such as "C:\Users\a" keeps its
                // separators, as the shell passes them.
                match chars.next() {
                    Some('\n') | None => {}
                    Some(next) => {
                        if in_double && !matches!(next, '$' | '`' | '"' | '\\') {
                            cur.push('\\');
                        }
                        cur.push(next);
                        started = true;
                        prefix_open = false;
                    }
                }
            }
            // The shell splits words at its blanks (space, tab, newline); any
            // other whitespace, a no-break space included, is part of the word
            // (OS text rule, issue 79), so a protected name that holds one is
            // still compared whole.
            ' ' | '\t' | '\n' if !in_single && !in_double => {
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
    let chars: Vec<char> = word.chars().take(64).collect();
    if numeric_range_len(&chars).is_some() {
        return None;
    }
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
        if is_prefix_launcher(t) {
            match skip_launcher_options(t, tokens, idx + 1) {
                Some(next) => {
                    idx = next;
                    continue;
                }
                // `command -v git` only looks the name up; nothing runs.
                None => return Some((t, &tokens[idx + 1..])),
            }
        }
        if basename(t) == "env" {
            idx += 1;
            // `env [-i] [-u NAME] [VAR=val]... command` — skip its own options
            // and assignments up to the wrapped command.
            while idx < tokens.len() {
                let a = tokens[idx].as_str();
                if a == "--" {
                    idx += 1;
                    break;
                }
                if matches!(
                    a,
                    "-u" | "--unset" | "-C" | "--chdir" | "-P" | "-a" | "--argv0"
                ) {
                    idx += 2; // the option and its value, `-u NAME`
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

/// Whether `word` is a `timeout` duration: a number with an optional
/// fraction and an `s`, `m`, `h` or `d` unit.
fn is_duration(word: &str) -> bool {
    let number = word.strip_suffix(['s', 'm', 'h', 'd']).unwrap_or(word);
    let mut parts = number.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
}

/// The effects of the launchers in front of a command that change how it
/// runs (TSK-216 round 4): each directory `env -C DIR` or `env
/// --chdir[=]DIR` moves to, in order, and the first launcher word the
/// guard cannot read with certainty: an `env` option it does not know, an
/// `env -C` without a directory, or a `timeout` without a duration. `env
/// -S`, which packs the command into one word, is a stated limit.
fn launcher_effects(tokens: &[String]) -> (Vec<&str>, Option<String>) {
    let assignment = |t: &str| {
        t.split_once('=')
            .is_some_and(|(name, _)| !name.is_empty() && is_identifier(name))
    };
    let mut dirs = Vec::new();
    let mut idx = 0;
    loop {
        while tokens.get(idx).is_some_and(|t| assignment(t)) {
            idx += 1;
        }
        let Some(t) = tokens.get(idx).map(String::as_str) else {
            return (dirs, None);
        };
        let name = basename(t);
        if is_prefix_launcher(t) {
            let Some(next) = skip_launcher_options(t, tokens, idx + 1) else {
                return (dirs, None);
            };
            if name == "timeout"
                && !tokens
                    .get(next.saturating_sub(1))
                    .is_some_and(|d| is_duration(d))
            {
                return (
                    dirs,
                    Some("`timeout` without a duration the guard can read".to_string()),
                );
            }
            idx = next;
            continue;
        }
        if name == "env" {
            match env_options(tokens, idx + 1, &mut dirs) {
                Ok(next) => idx = next,
                Err(why) => return (dirs, Some(why)),
            }
            continue;
        }
        return (dirs, None);
    }
}

/// Walk the options and assignments of an `env` launcher from `idx`,
/// pushing each directory `-C DIR`, `-CDIR` or `--chdir[=]DIR` moves to.
/// Returns the index of the command after them, or why the guard cannot
/// read them: an option it does not know, or `-C` without a directory.
fn env_options<'t>(
    tokens: &'t [String],
    mut idx: usize,
    dirs: &mut Vec<&'t str>,
) -> Result<usize, String> {
    while let Some(a) = tokens.get(idx).map(String::as_str) {
        if a == "--" {
            return Ok(idx + 1);
        }
        if matches!(a, "-C" | "--chdir") {
            let Some(dir) = tokens.get(idx + 1) else {
                return Err(format!("`env {a}` without a directory"));
            };
            dirs.push(dir.as_str());
            idx += 2;
            continue;
        }
        if let Some(dir) = a
            .strip_prefix("--chdir=")
            .or_else(|| a.strip_prefix("-C").filter(|d| !d.is_empty()))
        {
            dirs.push(dir);
            idx += 1;
            continue;
        }
        if matches!(a, "-u" | "--unset" | "-P" | "-a" | "--argv0") {
            idx += 2;
            continue;
        }
        if a.starts_with('-') {
            let known = matches!(
                a,
                "-" | "-i"
                    | "--ignore-environment"
                    | "-0"
                    | "--null"
                    | "-v"
                    | "--debug"
                    | "--list-signal-handling"
            ) || [
                "--unset=",
                "--argv0=",
                "--default-signal",
                "--ignore-signal",
                "--block-signal",
                "-u",
                "-P",
                "-a",
                "-S",
                "--split-string",
            ]
            .iter()
            .any(|prefix| a.starts_with(prefix));
            if !known {
                return Err(format!("`env {a}`, an option the guard does not read"));
            }
            idx += 1;
            continue;
        }
        let assignment = a
            .split_once('=')
            .is_some_and(|(name, _)| !name.is_empty() && is_identifier(name));
        if !assignment {
            break;
        }
        idx += 1;
    }
    Ok(idx)
}

/// `true` for the launchers that run the simple command after them: the
/// `command`, `builtin` and `exec` builtins, `nohup`, `time` in its
/// program form (`/usr/bin/time`, `command time`), and `nice`, `timeout`,
/// `stdbuf`, `ionice`, `caffeinate` and `xcrun`, any path form.
fn is_prefix_launcher(t: &str) -> bool {
    matches!(
        basename(t),
        "command"
            | "builtin"
            | "exec"
            | "nohup"
            | "time"
            | "nice"
            | "timeout"
            | "stdbuf"
            | "ionice"
            | "caffeinate"
            | "xcrun"
    )
}

/// Skip the options of a prefix launcher starting at `idx` and return the
/// index of the command it runs: `command -p`, `exec -c -l -a NAME`,
/// `time -p -o FILE`, and `--` for all. `None` when the options make
/// `command` only look the name up (`-v`, `-V`), so nothing after it runs.
/// Any other option is skipped, never read as "runs nothing": a launcher's
/// `--help` can be another option's value (`time --format --help git ...`),
/// so a non-executing form such as `nohup --help git push` is judged as the
/// command after it and may be refused, which fails closed.
fn skip_launcher_options(launcher: &str, tokens: &[String], mut idx: usize) -> Option<usize> {
    let launcher = basename(launcher);
    while let Some(a) = tokens.get(idx).map(String::as_str) {
        if a == "--" {
            // The options end; `timeout`'s duration still follows.
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a.len() < 2 {
            break;
        }
        if launcher == "command" && a.contains(['v', 'V']) {
            return None;
        }
        // `xcrun --find tool` and `xcrun -f tool` only print a path.
        if launcher == "xcrun" && matches!(a, "-f" | "--find") {
            return None;
        }
        idx += 1 + usize::from(takes_next_word(launcher, a));
    }
    // `timeout [options] DURATION command`.
    if launcher == "timeout" {
        idx += 1;
    }
    Some(idx)
}

/// `true` when the option `a` takes the next word as its value: `exec -a
/// NAME` (a name attached as in `-aNAME` is its own value), `time -o FILE`
/// or `--output FILE` (`--output=FILE` carries its own), `nice -n N`,
/// `timeout -s SIG` or `-k DURATION`, `stdbuf -o MODE`, `ionice -c CLASS`,
/// `caffeinate -t SECONDS` and `xcrun --sdk SDK`.
fn takes_next_word(launcher: &str, a: &str) -> bool {
    let long_valued: &[&str] = match launcher {
        "time" => &["--output", "--format"],
        "timeout" => &["--signal", "--kill-after"],
        "nice" => &["--adjustment"],
        "stdbuf" => &["--input", "--output", "--error"],
        "ionice" => &["--class", "--classdata", "--pid", "--pgid", "--uid"],
        "xcrun" => &["--sdk", "--toolchain"],
        _ => &[],
    };
    if long_valued.contains(&a) {
        return true;
    }
    let Some(flags) = a.strip_prefix('-').filter(|f| !f.starts_with('-')) else {
        return false;
    };
    let valued: &[char] = match launcher {
        "exec" => &['a'],
        "time" => &['o', 'f'],
        "nice" => &['n'],
        "timeout" => &['s', 'k'],
        "stdbuf" => &['i', 'o', 'e'],
        "ionice" => &['c', 'n', 'p', 'P', 'u'],
        "caffeinate" => &['t', 'w'],
        _ => return false,
    };
    flags.find(valued).is_some_and(|at| at + 1 == flags.len())
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
            branch_lookup: None,
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

/// Unquoted shell separators. Other whitespace belongs to the word.
pub(crate) fn shell_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

#[cfg(test)]
mod tests {
    #[test]
    fn r17_root_pattern_under_absent_directory_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        assert!(root_dot_pattern_target(&dir.path().join("absent/.*")).is_none());
        assert!(root_dot_pattern_target(&dir.path().join(".*")).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn r17_root_pattern_under_unreadable_directory_refuses() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("closed");
        std::fs::create_dir_all(parent.join("child")).unwrap();
        let original = std::fs::metadata(&parent).unwrap().permissions();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o0)).unwrap();
        let probe = parent.join("child").canonicalize();
        let target = root_dot_pattern_target(&parent.join("child/.*"));
        std::fs::set_permissions(&parent, original).unwrap();
        match probe {
            Ok(_) => eprintln!("EACCES directory probe unavailable under this test identity"),
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
                assert_eq!(
                    target,
                    Some("repository root pattern (cannot read directory)")
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn r15_extra_unreadable_root_pattern_is_unproven() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let path = dir
            .path()
            .join(std::ffi::OsString::from_vec(b".[\xff]*".to_vec()));
        assert!(root_dot_pattern_target(&path).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn r16_cd_keeps_a_quote_in_the_dequoted_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::fs::create_dir(root.join("safe'")).unwrap();
        std::os::unix::fs::symlink("../.git/config", root.join("safe'/link")).unwrap();
        let policy = GitPolicy::default();
        let report = evaluate_report_at(
            "cd \"safe'\"; printf x > link",
            &ctx(&policy, "task/local"),
            &root,
        );
        assert!(blocks(&report.violations), "{:?}", report.violations);
    }

    #[cfg(unix)]
    #[test]
    fn r16_sed_unreadable_script_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::os::unix::fs::symlink(".git/config", root.join("link")).unwrap();
        std::fs::write(root.join("script.sed"), b"# caf\xe9\nw link\n").unwrap();
        let policy = GitPolicy::default();
        let report = evaluate_report_at(
            "sed -f script.sed in.txt",
            &ctx(&policy, "task/local"),
            &root,
        );
        assert!(blocks(&report.violations), "{:?}", report.violations);
    }

    #[cfg(unix)]
    #[test]
    fn r16_sed_oversized_script_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::os::unix::fs::symlink(".git/config", root.join("link")).unwrap();
        let mut script = vec![b'#'; 65 * 1024];
        script.extend_from_slice(b"\nw link\n");
        std::fs::write(root.join("script.sed"), script).unwrap();
        let policy = GitPolicy::default();
        let report = evaluate_report_at(
            "sed -f script.sed in.txt",
            &ctx(&policy, "task/local"),
            &root,
        );
        assert!(blocks(&report.violations), "{:?}", report.violations);
    }

    #[test]
    fn r16_sed_shared_limit_refuses_even_without_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.sed");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(SED_SCRIPT_LIMIT + 1)
            .unwrap();
        assert!(matches!(
            read_sed_script("large.sed", dir.path()),
            SedRead::Unreadable { why, .. } if why.contains("limit")
        ));
        let policy = GitPolicy::default();
        let report = evaluate_report_at(
            "sed -f large.sed in.txt",
            &ctx(&policy, "task/local"),
            dir.path(),
        );
        assert!(blocks(&report.violations), "{:?}", report.violations);
    }

    #[test]
    fn r15_owned_sed_keeps_carriage_return_in_write_operand() {
        let SedRead::Read(paths) =
            sed_script_writes(&["w notes.md\r\nw other.md".into()], Path::new("."))
        else {
            panic!("inline script")
        };
        assert!(paths.contains(&"notes.md\r".to_string()), "{paths:?}");
        assert!(!paths.contains(&"notes.md".to_string()), "{paths:?}");
    }

    #[test]
    fn r15_environment_alias_is_refused_by_git_guard() {
        for command in [
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.ship GIT_CONFIG_VALUE_0='push origin release' git ship",
            "GIT_CONFIG_PARAMETERS=\"'alias.ship=push origin release'\" git ship",
        ] {
            let result = report(command, "task/work");
            assert!(blocks(&result.violations), "{command}: {:?}", result.violations);
            assert!(result.violations.iter().any(|v| v.message.contains("an alias the guard cannot resolve")), "{:?}", result.violations);
        }
    }

    #[test]
    fn alias_reader_removes_only_its_output_terminator() {
        let temp = tempfile::tempdir().unwrap();
        git2::Repository::init(temp.path()).unwrap();
        let value = "!printf value\r\n";
        let config = vec![format!("alias.x={value}")];
        assert_eq!(
            read_alias(
                temp.path(),
                &AliasQuery {
                    target: None,
                    config: &config,
                    name: "x"
                }
            ),
            AliasAnswer::Expansion(value.into())
        );
    }

    #[test]
    fn unicode_blanks_stay_in_shell_operands() {
        for blank in ['\u{a0}', '\u{2003}', '\u{202f}', '\r', '\u{b}', '\u{c}'] {
            let name = format!("release{blank}");
            assert_eq!(command_argv(&format!("git push origin {name}"))[3], name);
            let chars: Vec<_> = format!("{name} next").chars().collect();
            assert_eq!(redirect_word(&chars, 0).0, name);
            assert_eq!(
                line_words(&format!("echo {name}")).last(),
                Some(name.as_str())
            );
            let chars: Vec<_> = format!("<<{name}\n").chars().collect();
            assert_eq!(parse_heredoc_operator(&chars, 0).unwrap().0.delimiter, name);
            assert_eq!(
                expand_commands(&format!("echo {name}"))[0],
                format!("echo {name}")
            );
            assert!(paren_in_word(&format!("case{blank}")));
        }
        assert_eq!(
            qualifier_code("e\u{a0}git push origin main\u{a0}"),
            vec!["git push origin main"]
        );
        assert_eq!(qualifier_code("+\u{a0}"), vec!["\u{a0}"]);
        assert!(expand_commands("{\u{a0}echo ok")
            .iter()
            .any(|s| s.starts_with("{\u{a0}")));
        assert!(expand_commands("echo x\u{a0}} tail")
            .iter()
            .any(|s| s.contains("x\u{a0}}")));
        assert_eq!(
            match sed_script_writes(&["w file\u{a0}".into()], Path::new(".")) {
                SedRead::Read(paths) => paths,
                other @ SedRead::Unreadable { .. } => panic!("{other:?}"),
            },
            vec!["file\u{a0}"]
        );
        assert_eq!(
            split_alias("push origin release\u{a0}").unwrap()[2],
            "release\u{a0}"
        );
        assert_eq!(
            split_alias("push\torigin\nrelease").unwrap(),
            ["push", "origin", "release"]
        );
        assert_eq!(split_alias("version\u{c}").unwrap(), ["version\u{c}"]);
    }
    use super::super::policy::PolicyLevel;
    use super::*;

    /// Round seven on issue 79: a link whose name is not valid UTF-8 and that
    /// points at an enforcement file is followed on its exact path, not on a
    /// storage key that resolves to nothing.
    #[cfg(unix)]
    #[test]
    fn a_glob_reach_follows_a_link_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
        let odd = root.join(std::ffi::OsStr::from_bytes(b"alias-\xe9"));
        if std::os::unix::fs::symlink(".codeflow/policy.json", &odd).is_err() {
            return; // this volume refuses names that are not UTF-8
        }
        std::os::unix::fs::symlink(".codeflow/policy.json", root.join("alias-plain")).unwrap();
        assert!(
            glob_reach("alias-p*", &root, &root).is_some(),
            "the valid control"
        );
        std::fs::remove_file(root.join("alias-plain")).unwrap();
        assert!(glob_reach("alias-*", &root, &root).is_some());
    }

    /// Issue 79: `-name '??'` may match a name of two bytes that is not valid
    /// UTF-8 (one replacement character as text), so the candidate stays; a
    /// literal pattern is judged as before.
    #[cfg(unix)]
    #[test]
    fn a_find_name_filter_keeps_a_candidate_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt as _;
        let odd = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/repo/.git/\xe2\x82"));
        let wildcard = vec![("??".to_string(), false)];
        assert!(find_name_matches(Some(&wildcard), &odd));
        let literal = vec![("config".to_string(), false)];
        assert!(!find_name_matches(Some(&literal), &odd));
        let star = vec![("*".to_string(), false)];
        assert!(find_name_matches(Some(&star), &odd));
    }

    fn ctx<'a>(policy: &'a GitPolicy, branch: &'a str) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
            pr_base_lookup: None,
            dir_target_lookup: None,
            alias_lookup: None,
            branch_lookup: None,
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
            branch_lookup: None,
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
            branch_lookup: None,
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

    /// TSK-215: Grok Build 1.0.46 sends each field under both spellings
    /// (captured from a live session). Equal duplicates are one field; the
    /// guard reads the command instead of dropping the payload as unreadable
    /// and allowing it.
    #[test]
    fn test_payload_parse_grok_with_both_spellings() {
        let json = r#"{
            "hookEventName": "pre_tool_use",
            "cwd": "/repo",
            "toolName": "run_terminal_command",
            "toolInput": {"command": "git commit -m x", "description": "d"},
            "hook_event_name": "PreToolUse",
            "tool_name": "run_terminal_command",
            "tool_input": {"command": "git commit -m x", "description": "d"}
        }"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.shell_command(), Some("git commit -m x"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new("/repo")));
    }

    /// Spellings that disagree name two different calls; reading either one
    /// could judge a command other than the one the harness runs.
    #[test]
    fn test_payload_conflicting_spellings_are_malformed() {
        for json in [
            r#"{"toolName":"run_terminal_command","tool_name":"Write","tool_input":{"command":"git commit"}}"#,
            r#"{"tool_name":"Bash","toolInput":{"command":"git commit"},"tool_input":{"command":"git status"}}"#,
        ] {
            let err = HookPayload::parse(json).unwrap_err();
            assert!(err.to_string().contains("disagree"), "{json}: {err}");
        }
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
            branch_lookup: None,
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
            branch_lookup: None,
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

    /// Round sixteen on issue 79: a protected branch whose name ends in a
    /// no-break space is compared whole, so an unquoted push to it is blocked
    /// as the quoted one is, and an alias that runs it is judged the same.
    #[test]
    fn a_push_to_a_protected_name_with_a_no_break_space_is_blocked() {
        let p = GitPolicy {
            protected_branches: vec!["release\u{a0}".into()],
            ..GitPolicy::default()
        };
        for command in [
            "git push origin HEAD:release\u{a0}",
            "git push origin 'HEAD:release\u{a0}'",
        ] {
            let v = evaluate(command, &ctx(&p, "feat/x"));
            assert_eq!(
                v.first().map(|v| v.rule.as_str()),
                Some("git.push_to_protected"),
                "{command}"
            );
        }
        assert!(evaluate("git push origin release", &ctx(&p, "feat/x")).is_empty());
        let words = split_alias("push origin HEAD:release\u{a0}").unwrap();
        assert_eq!(words.last().map(String::as_str), Some("HEAD:release\u{a0}"));
    }

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
    fn r16_missing_sed_script_and_unreadable_pr_stdin_refuse() {
        let policy = default_policy();
        for command in [
            "sed -i -f /no/such/script.sed README.md",
            "gh pr create --body-file -",
        ] {
            assert!(
                blocks(&evaluate(command, &ctx(&policy, "feat/x"))),
                "{command}"
            );
        }
    }

    #[test]
    fn test_pr_body_file_missing_refuses() {
        // An unreadable publication body cannot be certified.
        let p = default_policy();
        let cmd = "gh pr create -t 'feat: x' --body-file '/no/such/body/file.md'";
        let violations = evaluate(cmd, &ctx(&p, "feat/x"));
        assert!(violations
            .iter()
            .any(|v| v.rule == "git.pr_body" && v.message.contains("cannot read")));
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

    #[test]
    fn a_double_quoted_backslash_escapes_only_what_the_shell_escapes() {
        // TSK-197: a quoted Windows path keeps its separators, as bash
        // passes them; an unquoted one loses them, as bash removes them.
        assert_eq!(
            shell_tokens(r#"git -C "C:\Users\a b\repo" status"#),
            vec!["git", "-C", r"C:\Users\a b\repo", "status"]
        );
        assert_eq!(
            shell_tokens(r"git -C C:\Users\a"),
            vec!["git", "-C", "C:Usersa"]
        );
        assert_eq!(
            shell_tokens(r#"echo "a\$b \"q\" c\\d \`e""#),
            vec!["echo", r#"a$b "q" c\d `e"#]
        );
        assert_eq!(shell_tokens("echo \"a\\\nb\""), vec!["echo", "ab"]);
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
            // BSD sed takes the backup suffix as the next token.
            "sed -i '' s/block/off/ .codeflow/policy.json",
            "sed -i .bak s/block/off/ .codeflow/policy.json",
            "sed -i '' -e s/block/off/ .codeflow/policy.json",
            "sed -i -e s/block/off/ .codeflow/policy.json",
            // GNU sed reads `''` as an empty script and the next token as a file.
            "sed -i '' .codeflow/policy.json",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    /// TSK-216 AC-1 (issue 23): an in-place `sed` is judged by the files it
    /// writes, never by its script, its backup suffix or an empty token.
    #[test]
    fn test_in_place_sed_judges_only_its_files() {
        let p = default_policy();
        assert_eq!(
            sed_file_operands(&["-i".into(), String::new(), "s/a/b/".into(), "f".into()]),
            ["s/a/b/", "f"],
        );
        assert_eq!(
            sed_file_operands(&["-i".into(), "-e".into(), "s/a/b/".into(), "f".into()]),
            ["f"],
        );
        for cmd in [
            "sed -i '' s/a/b/ README.md",
            "sed -i '' -e s/a/b/ README.md",
            "sed -i .bak s/a/b/ README.md",
            "sed -i -e s/.codeflow/x/ README.md",
            "rm -f '' README.md",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(!has_rule(&v, "git.hook_integrity"), "{cmd}: {v:?}");
        }
    }

    /// A `find` primary's value and an `-exec` command's argument are never
    /// actions, and `pushd -n` only stacks a directory; a later rotation or
    /// `popd` can reach it, resolved from wherever the shell is then, so the
    /// directory becomes unknown (PR 36 CI round, TSK-216 round 10).
    #[test]
    fn test_find_values_and_pushd_stack_are_read_as_written() {
        let words = |line: &str| shell_tokens(line)[1..].to_vec();
        assert!(!find_mutates(&words("find x -name -delete")));
        assert!(!find_mutates(&words("find x -exec echo -delete {} +")));
        assert!(find_mutates(&words("find x -name y -delete")));
        assert!(find_mutates(&words("find x -exec rm {} +")));
        let start = Path::new("/r");
        let run = run_dirs(&expand_commands("pushd -n sub && rm x"), start);
        assert_eq!(run.dirs, vec![PathBuf::from("/r")]);
        assert!(run.unknown.is_none(), "{:?}", run.unknown);
        for line in [
            "pushd -n sub && pushd +1 && rm x",
            "pushd -n sub; cd b; pushd; rm x",
            "pushd -n sub; cd b; pushd -1; rm x",
            "pushd -n sub; popd; rm x",
            "pushd -n sub; popd +1; rm x",
            "pushd -n sub | true; popd -n; rm x",
        ] {
            let run = run_dirs(&expand_commands(line), start);
            assert!(run.unknown.is_some(), "{line}: {:?}", run.dirs);
        }
        for line in ["pushd +1 && rm x", "pushd b && popd && rm x", "pushd; rm x"] {
            let run = run_dirs(&expand_commands(line), start);
            assert!(run.unknown.is_none(), "{line}: {:?}", run.unknown);
        }
        // From there a write redirect is read by its target's name.
        let judged = |cmd: &str| {
            let redirects = redirect_writes(cmd);
            unknown_dir_name_violation(
                &shell_tokens(cmd),
                &redirects,
                PolicyLevel::Block,
                cmd,
                "a stack",
            )
        };
        for cmd in [
            "printf x > policy.json",
            "echo x >>policy.json",
            "true &> pol*",
        ] {
            assert!(judged(cmd).is_some(), "{cmd}");
        }
        for cmd in [
            "printf x > notes.md",
            "make 2>/dev/null >&2",
            "cat policy.json",
        ] {
            assert!(judged(cmd).is_none(), "{cmd}");
        }
    }

    /// The files a `sed` only reads are dropped at their own positions in
    /// its argument list; every other command on the line, a heredoc body,
    /// a here-string or a producer, is kept whole (TSK-216 round 8).
    #[test]
    fn test_line_without_reads_drops_only_the_operands_position() {
        let judged = |line: &str| {
            let sed = expand_commands(line)
                .iter()
                .map(|segment| shell_tokens(segment))
                .find(|tokens| tokens.first().is_some_and(|p| p == "sed"))
                .expect("a sed command");
            let args = &sed[1..];
            line_without_reads(line, args, &sed_read_flags(args))
        };
        let escaped =
            judged("sed -f - .codeflow/policy\\.json <<'SED'\nw .codeflow/policy.json\nSED");
        assert!(escaped.contains("w .codeflow/policy.json"), "{escaped}");
        let produced = judged("printf '%s\\n' 'w policy.json' | sed -f - policy.json");
        assert_eq!(produced.matches("policy.json").count(), 1, "{produced}");
        let here = judged("sed -f - policy.json <<< 'w policy.json'");
        assert!(here.contains("w policy.json"), "{here}");
        for read in [
            "printf 'p\\n' | sed -f - .codeflow/policy.json",
            "printf 'p\\n' | sed -f - '.codeflow/policy.json'",
            "printf 'p\\n' | sed -f - .codeflow/policy\\.json",
        ] {
            let plain = judged(read);
            assert!(!plain.contains("policy.json"), "{read}: {plain}");
        }
        let body = judged("sed -f - a.txt <<'S'\nsed -f - a.txt\nS");
        assert!(body.contains("\nsed -f - a.txt"), "{body}");
        let twice = judged("sed -f - a.txt; sed -f - a.txt");
        assert_eq!(twice.matches("a.txt").count(), 2, "{twice}");
    }

    /// Many directory moves on one line stop at the limit as they are
    /// collected: the list never grows past it, the directory becomes
    /// unknown, and the work stays small (TSK-216 round 6).
    #[test]
    fn test_run_dirs_stop_at_the_limit_while_collecting() {
        let options: Vec<String> = (0..30).map(|n| format!("-C d{n}")).collect();
        let line = format!("env {} true", options.join(" "));
        let started = std::time::Instant::now();
        let run = run_dirs(&expand_commands(&line), Path::new("/r"));
        assert!(run.dirs.len() <= RUN_DIR_LIMIT, "{}", run.dirs.len());
        assert!(run.unknown.is_some());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    /// A refusal from a run-time directory names the substitution in
    /// words, never the tokenizer's placeholder.
    #[test]
    fn test_unknown_dir_refusal_shows_the_substitution() {
        let line = r#"cd "$(git rev-parse --show-toplevel)" && rm policy.json"#;
        let run = run_dirs(&expand_commands(line), Path::new("/r"));
        let why = run.unknown.expect("a run-time directory");
        let v = unknown_dir_name_violation(
            &shell_tokens("rm policy.json"),
            &Redirects::default(),
            PolicyLevel::Block,
            line,
            &why,
        )
        .expect("refused");
        assert!(v.message.contains("$(...)"), "{}", v.message);
        assert!(
            !v.message
                .contains([git_target::SUBSTITUTED, SUBSTITUTED_BARE]),
            "{}",
            v.message
        );
    }

    /// A redirection is a read only when it is provably one; every other
    /// operator writes its target, and quoted text is never an operator
    /// (TSK-216 round 11).
    #[test]
    fn test_redirect_writes_reads_operators_from_the_text() {
        for (segment, writes) in [
            ("printf x > a", vec!["a"]),
            (
                "printf x >a >>b >|c &>d &>>e",
                vec!["a", "b", "c", "d", "e"],
            ),
            ("printf x 1<>a", vec!["a"]),
            ("printf x <>a", vec!["a"]),
            (": {fd}>a", vec!["a"]),
            (": {fd}<>a", vec!["a"]),
            ("printf x>a", vec!["a"]),
            ("printf x 2> 'a b'", vec!["a b"]),
            ("printf x >\"$d\"/a", vec!["$d/a"]),
            ("printf x >&a", vec!["a"]),
            ("printf x >& a", vec!["a"]),
            ("printf x >a;", vec!["a"]),
            ("printf x >a|cat", vec!["a"]),
            // Line continuations are joined first (round 12).
            ("printf x > \\\n  a", vec!["a"]),
            ("printf x > p\\\nolicy.json", vec!["policy.json"]),
            ("printf x \\\n> a", vec!["a"]),
            ("printf 'x\\\n' > a", vec!["a"]),
        ] {
            let read = redirect_writes(segment);
            assert_eq!(read.targets, writes, "{segment}");
            assert!(read.unread.is_empty(), "{segment}");
        }
        // ANSI-C or locale quoting with a `>`: every word is judged by name.
        for segment in [
            "printf '%s\\n' $'it\\'s' > .codeflow/policy.json",
            "printf x > $'policy.json'",
            "printf x >$\"policy.json\"",
        ] {
            let read = redirect_writes(segment);
            assert!(read.targets.is_empty(), "{segment}");
            assert!(
                read.unread.iter().any(|w| word_could_name(w).is_some()),
                "{segment}: {:?}",
                read.unread
            );
        }
        let quoted_read = redirect_writes("printf '%s' $'a\\tb'");
        assert!(quoted_read.unread.is_empty() && quoted_read.targets.is_empty());
        for segment in [
            "cat < a",
            "cat <a",
            "cat 0<a",
            "cat << EOF",
            "cat <<-EOF",
            "cat <<< a",
            "make 2>&1",
            "make >&2",
            "make 3>&-",
            "make >&-",
            "make 3>&4-",
            "cat 3<&0",
            "cat <&-",
            "printf '%s' '>a'",
            "printf '%s' \">a\"",
            "printf '%s' \\>a",
            "printf x 'a>b'",
            "diff <(cat a) >(cat b)",
        ] {
            let read = redirect_writes(segment);
            assert!(
                read.targets.is_empty() && read.unread.is_empty(),
                "{segment}"
            );
        }
    }

    /// From a directory the guard cannot determine, a word is read by its
    /// names alone: one that ends an enforcement path, or leads into a
    /// whole enforcement directory, could name it; build output cannot.
    #[test]
    fn test_word_could_name_reads_names_alone() {
        for word in [
            "policy.json",
            "pol*",
            "../x/.git/hooks/pre-commit",
            "../.codeflow/policy.json",
            "config",
            "hooks/pre-commit",
            "*",
            "--file=settings.json",
        ] {
            assert!(word_could_name(word).is_some(), "{word}");
        }
        for word in [
            "a.o",
            "*.o",
            "build/a.o",
            "target/debug",
            ".git/index.lock",
            "config.toml",
            "-rf",
            "/abs/policy.json",
            "..",
        ] {
            assert!(word_could_name(word).is_none(), "{word}");
        }
    }

    /// The directories a script can run in: each literal move applied to
    /// every directory before it, across pipelines, subshells and launcher
    /// moves; a directory filled in at run time or a move in a loop leaves
    /// the list incomplete.
    #[test]
    fn test_run_dirs_follow_literal_moves() {
        let start = Path::new("/r");
        let run = run_dirs(&expand_commands("cd build && printf x | xargs rm"), start);
        assert_eq!(
            run.dirs,
            vec![PathBuf::from("/r"), PathBuf::from("/r/build")]
        );
        assert!(run.unknown.is_none());
        let run = run_dirs(&expand_commands("env -C sub sh -c 'cd a; rm x'"), start);
        assert!(
            run.dirs.contains(&PathBuf::from("/r/sub/a")),
            "{:?}",
            run.dirs
        );
        assert!(run.unknown.is_none());
        let run = run_dirs(&expand_commands("cd \"$(printf b)\" && rm x"), start);
        assert!(run.unknown.is_some());
        let run = run_dirs(&expand_commands("for i in 1 2; do cd a; done; rm x"), start);
        assert!(run.unknown.is_some());
        let run = run_dirs(&expand_commands("rm build/a.o"), start);
        assert_eq!(run.dirs, vec![PathBuf::from("/r")]);
        assert!(run.unknown.is_none());
        // Only a plain literal operand is followed (TSK-216 round 11).
        for line in [
            "cd ~1 && rm x",
            "cd ~+1 && rm x",
            "cd ~-1 && rm x",
            "cd ~+ && rm x",
            "cd ~- && rm x",
            "cd ~root && rm x",
            "cd - && rm x",
            "pushd ~2 && rm x",
            "cd .code* && rm x",
            "cd {a,b} && rm x",
            "cd \"$d\" && rm x",
        ] {
            let run = run_dirs(&expand_commands(line), start);
            assert!(run.unknown.is_some(), "{line}: {:?}", run.dirs);
        }
        for line in ["cd build && rm x", "cd ~/w && rm x", "cd -- build && rm x"] {
            let run = run_dirs(&expand_commands(line), start);
            assert!(run.unknown.is_none(), "{line}: {:?}", run.unknown);
        }
    }

    /// Nested word groups are read once per nesting level, so deep nesting
    /// costs linear time, and text nested past the limit refuses the line
    /// instead of passing unread (TSK-216 round 19).
    #[test]
    fn test_nested_word_groups_are_read_once_per_level() {
        let nested = |n: usize| format!("{}x{}", "@(".repeat(n), ")".repeat(n));
        let start = std::time::Instant::now();
        let segments = expand_commands(&format!("bash -O extglob -c 'echo {}'", nested(24)));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert!(segments.iter().any(|s| s == NESTING_UNREAD));
        let shallow = expand_commands(&format!("echo {}", nested(6)));
        assert!(!shallow.iter().any(|s| s == NESTING_UNREAD), "{shallow:?}");
        let policy = default_policy();
        let deep = format!("echo {}", nested(12));
        let report = evaluate_report_at(&deep, &ctx(&policy, "task/x"), Path::new("."));
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.hook_integrity"),
            "{deep}"
        );
    }

    /// Folder names above the word are never pattern syntax (TSK-216 round
    /// 18): a repository under a Windows short name such as `RUNNER~1`, or
    /// under `Program Files (x86)`, judges `build/*.o` by the build folder
    /// alone, while a word's own `(` and a
    /// zsh exclusion after the policy path still reach the policy file.
    #[test]
    fn test_folder_names_above_a_word_stay_literal() {
        for folder in ["RUNNER~1", "Program Files (x86)", "a#b^c"] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join(folder).join("repo");
            std::fs::create_dir_all(root.join("build")).unwrap();
            std::fs::create_dir_all(root.join(".codeflow")).unwrap();
            git2::Repository::init(&root).unwrap();
            std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
            std::fs::write(root.join("build/a.o"), "").unwrap();
            let glob = WordGlob::new("build/*.o", &root);
            assert_eq!(glob.expand().unwrap(), vec![root.join("build/a.o")]);
            assert_eq!(glob.prefix(), root.join("build"));
            assert_eq!(
                token_integrity_path("build/*.o", &root, &root),
                None,
                "{folder}"
            );
            // Typed out in full, the folder name is the word's own text: a
            // short name stays plain, while `(`, `#` or `^` there is read
            // as pattern syntax, since quotes are gone by then.
            let absolute = crate::portable_path::slashed(&root.join("build/a.o"));
            assert_eq!(
                token_integrity_path(&absolute, &root, &root).is_none(),
                folder == "RUNNER~1",
                "{folder}"
            );
            assert!(
                token_integrity_path(".codeflow/policy.json~x", &root, &root).is_some(),
                "{folder}"
            );
            assert!(token_integrity_path("(.codeflow|x)/policy.json", &root, &root).is_some());
        }
    }

    /// Glob expansion reads the file system as the shell does: a leading
    /// dot only matches a dotted pattern, and a glob that finds nothing
    /// expands to nothing (TSK-216 round 4).
    #[test]
    fn test_expand_glob_matches_like_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a1.o"), "").unwrap();
        std::fs::write(dir.path().join(".hidden.o"), "").unwrap();
        let found = WordGlob::new("*.o", dir.path()).expand().unwrap();
        assert_eq!(found, vec![dir.path().join("a1.o")]);
        assert_eq!(WordGlob::new(".*.o", dir.path()).expand().unwrap().len(), 1);
        assert!(WordGlob::new("none*", dir.path())
            .expand()
            .unwrap()
            .is_empty());
        // An unclosed bracket is a literal character, never a pattern that
        // matches every entry (TSK-216 round 13).
        assert!(WordGlob::new("e[32mhello", dir.path())
            .expand()
            .unwrap()
            .is_empty());
        std::fs::write(dir.path().join("x["), "").unwrap();
        assert_eq!(
            WordGlob::new("x[", dir.path()).expand().unwrap(),
            vec![dir.path().join("x[")]
        );
    }

    /// Patterns the shell-pattern tests read, and names to match them with.
    const SHELL_PATTERNS: &[&str] = &[
        "[[:alpha:]_]olicy.json",
        "[[:alpha:][:digit:]]olicy.json",
        "[[:alpha:]]olicy.json",
        "[[:digit:]]olicy.json",
        "[!p]olicy.json",
        "[^p]olicy.json",
        "[![:alpha:]]olicy.json",
        "[pq]olicy.json",
        "[p\\]]olicy.json",
        "[pa\\[:alpha:]olicy.json",
        "[ab]olicy.json",
        "[a-q]olicy.json",
        "[p._]olicy.json",
        ".[ab]*",
        ".[cg]*",
        "[]x]",
        "[]]olicy.json",
        "[!]]olicy.json",
        "[[=p=]]olicy.json",
        "[[.p.]]olicy.json",
        "[[:alpha:]",
        "[[:alp]olicy.json",
        "e[32mhello",
        "e[0mn",
        "x[",
        "x]",
        "pol*",
        "pol***",
        "a**b",
        "?olicy.json",
    ];
    const SHELL_NAMES: &[&str] = &[
        "policy.json",
        "xolicy.json",
        "_olicy.json",
        "1olicy.json",
        "]olicy.json",
        "e[32mhello",
        "e[0mn",
        "hello",
        "x[",
        "x]",
        "]",
        "a",
        "axyb",
        "[[:alpha:]",
        ":olicy.json",
        "aolicy.json",
        ".codeflow",
        ".git",
        ".a",
    ];

    /// The guard's reading of a shell pattern always compiles and only
    /// over-approximates: a plain member set keeps its members, any other
    /// bracket expression makes the whole component match every name, a
    /// backslash outside brackets makes the next character literal, and
    /// only a `[` that never closes is literal (TSK-216 rounds 13 to 15).
    #[test]
    fn test_shell_pattern_reads_brackets_as_the_shell_does() {
        for (pattern, name, matches) in [
            ("[[:alpha:]_]olicy.json", "policy.json", true),
            ("[[:alpha:][:digit:]]olicy.json", "policy.json", true),
            ("[[:alpha:]]olicy.json", "policy.json", true),
            ("[!p]olicy.json", "policy.json", true),
            ("[pq]olicy.json", "policy.json", true),
            ("[ab]olicy.json", "policy.json", false),
            ("[ab]olicy.json", "aolicy.json", true),
            ("[a-c]olicy.json", "policy.json", true),
            (".[ab]*", ".git", false),
            (".[cg]*", ".git", true),
            ("[]x]", "]", true),
            ("[]]olicy.json", "]olicy.json", true),
            ("[[=p=]]olicy.json", "policy.json", true),
            ("pol***", "policy.json", true),
            ("a**b", "axyb", true),
            ("?olicy.json", "policy.json", true),
            ("e[32mhello", "e[32mhello", true),
            ("e[32mhello", "policy.json", false),
            ("x[", "x[", true),
            ("x[", "xa", false),
            ("x]", "x]", true),
            ("[[:alpha:]_]olicy.json", "policy.jsonx", true),
            ("[p\\]]olicy.json", "policy.json", true),
            ("[p]]olicy.json", "policy.json", true),
            ("[pa[:alpha:]olicy.json", "policy.json", true),
            ("[ab][cd]", "policy.json", true),
            ("[ab]\\n", "settings.json", true),
            ("polic\\y.json", "policy.json", true),
            ("polic\\y.json", "polic\\y.json", false),
            ("pol\\*", "pol*", true),
            ("pol\\*", "policy.json", false),
            ("\\]", "]", true),
            ("x\\", "x\\", true),
        ] {
            assert_eq!(
                shell_pattern(pattern).matches(name),
                matches,
                "{pattern} {name}"
            );
        }
    }

    /// Whether `program` runs as a POSIX shell here: `-c 'echo ready'`
    /// prints `ready`. On Windows `bash` may be a launcher with no Linux
    /// behind it, which answers with an error instead (TSK-216 round 18).
    fn shell_ready(program: &str) -> bool {
        let mut command = if program == "zsh" {
            std::process::Command::new("zsh")
        } else {
            std::process::Command::new("bash")
        };
        command
            .args(["-c", "echo ready"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|out| out.status.success() && out.stdout == b"ready\n")
    }

    /// Bash's verdict on each `(pattern, name)` pair, from
    /// `[[ name == pattern ]]`; `None` when the platform has no working
    /// Bash ([`shell_ready`]).
    fn bash_pattern_verdicts(pairs: &[(&str, &str)]) -> Option<Vec<bool>> {
        if !shell_ready("bash") {
            return None;
        }
        let input = pairs
            .iter()
            .map(|(p, n)| [*p, "\t", *n, "\n"].concat())
            .collect::<String>();
        let script = "while IFS=$'\\t' read -r p n; do if [[ $n == $p ]]; then echo 1; else echo 0; fi; done";
        let mut child = std::process::Command::new("bash")
            .args(["-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            use std::io::Write as _;
            stdin.write_all(input.as_bytes())
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap().expect("bash read every pair");
        let verdicts: Vec<bool> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l == "1")
            .collect();
        assert_eq!(verdicts.len(), pairs.len(), "bash answered every pair");
        Some(verdicts)
    }

    /// Against the platform's Bash, where there is one: every name Bash's
    /// `[[ name == pattern ]]` matches, the guard's reading matches too.
    #[test]
    fn test_shell_pattern_never_matches_less_than_bash() {
        let pairs: Vec<(&str, &str)> = SHELL_PATTERNS
            .iter()
            .flat_map(|p| SHELL_NAMES.iter().map(move |n| (*p, *n)))
            .collect();
        let Some(verdicts) = bash_pattern_verdicts(&pairs) else {
            eprintln!("bash is not available; skipped");
            return;
        };
        let mut bash_matched = Vec::new();
        for ((pattern, name), bash) in pairs.iter().zip(verdicts) {
            if bash {
                bash_matched.push((*pattern, *name));
                assert!(
                    shell_pattern(pattern).matches(name),
                    "bash matches {name} with {pattern}; the guard must too"
                );
            }
        }
        // The comparison covers the review's patterns: Bash selects the
        // policy file with each of them.
        for pattern in [
            "[[:alpha:]_]olicy.json",
            "[[:alpha:][:digit:]]olicy.json",
            "[[:alpha:]]olicy.json",
            "[p\\]]olicy.json",
            "[pa\\[:alpha:]olicy.json",
        ] {
            assert!(
                bash_matched.contains(&(pattern, "policy.json")),
                "bash should match policy.json with {pattern}: {bash_matched:?}"
            );
        }
    }

    /// Random patterns over the characters that matter to bracket
    /// expressions, escapes and wildcards, against the platform's Bash
    /// (TSK-216 round 15): every pair Bash's `[[ name == pattern ]]`
    /// matches, the guard's reading matches too. The seed is fixed, so a
    /// failure names a pattern that reproduces.
    #[test]
    fn test_random_shell_patterns_never_match_less_than_bash() {
        const PIECES: &[&str] = &[
            "p",
            "o",
            "l",
            "i",
            "c",
            "y",
            ".",
            "j",
            "s",
            "n",
            "_",
            "a",
            "b",
            "[",
            "]",
            "\\",
            "!",
            "^",
            "-",
            ":",
            "*",
            "?",
            "x",
            "[:alpha:]",
            "[:digit:]",
            "[[:alpha:]",
            "olicy",
            ".json",
        ];
        const NAMES: &[&str] = &[
            "policy.json",
            ".codeflow",
            "hooks",
            "x",
            "]",
            "[",
            "\\",
            "p",
            "olicy.json",
            "]olicy.json",
            "-olicy.json",
            "!",
            "^",
            ":",
            "settings.json",
            ".git",
        ];
        let mut state: u64 = 0x2016_0215_7a3c_9e11;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut patterns = Vec::new();
        for _ in 0..3000 {
            let len = 1 + next() % 8;
            let pattern: String = (0..len)
                .map(|_| PIECES[usize::try_from(next() % PIECES.len() as u64).unwrap()])
                .collect();
            patterns.push(pattern);
        }
        let pairs: Vec<(&str, &str)> = patterns
            .iter()
            .flat_map(|p| NAMES.iter().map(move |n| (p.as_str(), *n)))
            .collect();
        let Some(verdicts) = bash_pattern_verdicts(&pairs) else {
            eprintln!("bash is not available; skipped");
            return;
        };
        let missed: Vec<String> = pairs
            .iter()
            .zip(verdicts)
            .filter(|((pattern, name), bash)| *bash && !shell_pattern(pattern).matches(name))
            .map(|((pattern, name), _)| format!("{pattern} matches {name}"))
            .collect();
        assert!(
            missed.is_empty(),
            "bash matched {} pairs the guard does not: {:?}",
            missed.len(),
            &missed[..missed.len().min(20)]
        );
    }

    /// The pieces the random command words are built from.
    #[cfg(unix)]
    const EXPANSION_PIECES: &[&str] = &[
        "alias",
        "/",
        "policy",
        "pol",
        "p",
        "olicy",
        "o",
        "l",
        ".json",
        "json",
        "policy.json",
        "olicy.json",
        "p*",
        "*.json",
        "*",
        "?",
        "**/",
        "[",
        "]",
        "[pq]",
        "[!x]",
        "[[:alpha:]]",
        "{",
        "}",
        ",",
        "..",
        "{p,x}",
        "{ol,uz}",
        "{y..y}",
        "{a..c}",
        "{policy,x}",
        "\\",
        "'",
        "\"",
        "x",
        ".code",
        "flow",
        "{flow,x}",
        "~+/",
        "-",
        "_",
        "a",
    ];

    /// A repository holding enforcement files, `alias -> .codeflow`, a
    /// hidden link to it as the only link under `build` and a numeric one
    /// under `out` (for zsh's qualifiers and ranges), and a few plain
    /// files, for comparing the guard with a shell's expansion.
    #[cfg(unix)]
    fn expansion_fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
        std::fs::write(root.join(".codeflow/project.toml"), "").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        for file in ["build/a.o", "x", "policy.txt", "polo.json", "a"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        std::os::unix::fs::symlink(".codeflow", root.join("alias")).unwrap();
        std::os::unix::fs::symlink("../alias", root.join("build/.review-hidden")).unwrap();
        std::fs::create_dir_all(root.join("out")).unwrap();
        std::os::unix::fs::symlink("../alias", root.join("out/7")).unwrap();
        (dir, root)
    }

    /// What Bash expands each word to from `root`, printed and never run:
    /// `printf '%s\n' WORD` per word. A word Bash cannot parse expands to
    /// nothing. `None` when the platform has no Bash.
    #[cfg(unix)]
    fn bash_expansions(root: &Path, words: &[String]) -> Option<Vec<Vec<String>>> {
        let script = "cd \"$1\" || exit 1; while IFS= read -r w; do eval \"printf '%s\\\\n' $w\" 2>/dev/null; printf '\\036\\n'; done";
        let mut command = std::process::Command::new("bash");
        command.args(["-c", script, "bash"]).arg(root);
        shell_expansions(command, words)
    }

    /// What zsh with no startup files expands each word to from `root`:
    /// `print -rl -- WORD` per word, as `zsh -f -c 'cd FIXTURE && print -rl
    /// -- WORD'` prints it, in one process with each word in its own
    /// subshell. With `extended`, `extendedglob` is set, so `^`, `#` and
    /// `~` are patterns too. `None` when the platform has no zsh.
    #[cfg(unix)]
    fn zsh_expansions(root: &Path, words: &[String], extended: bool) -> Option<Vec<Vec<String>>> {
        let script = "cd \"$1\" || exit 1; while IFS= read -r w; do ( eval \"print -rl -- $w\" ) 2>/dev/null; print -r -- $'\\036'; done";
        let mut command = std::process::Command::new("zsh");
        command.arg("-f");
        if extended {
            command.args(["-o", "extendedglob"]);
        }
        command.args(["-c", script, "zsh"]).arg(root);
        shell_expansions(command, words)
    }

    /// Feed `words` to a shell loop that prints each expansion and then a
    /// record separator (`\x1e`), and read one record per word.
    #[cfg(unix)]
    fn shell_expansions(
        mut command: std::process::Command,
        words: &[String],
    ) -> Option<Vec<Vec<String>>> {
        if !shell_ready(&command.get_program().to_string_lossy()) {
            return None;
        }
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take().unwrap();
        let input: String = words.iter().map(|w| [w.as_str(), "\n"].concat()).collect();
        let writer = std::thread::spawn(move || {
            use std::io::Write as _;
            stdin.write_all(input.as_bytes())
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap().expect("the shell read every word");
        let text = String::from_utf8_lossy(&out.stdout);
        let records: Vec<Vec<String>> = text
            .split("\u{1e}\n")
            .map(|record| record.lines().map(str::to_string).collect())
            .collect();
        assert_eq!(
            records.len(),
            words.len() + 1,
            "the shell answered every word"
        );
        Some(records)
    }

    /// Random words from a fixed seed: a start from `starts`, then one to
    /// four of `pieces`.
    #[cfg(unix)]
    fn random_words(seed: u64, count: usize, starts: &[&str], pieces: &[&str]) -> Vec<String> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state).unwrap()
        };
        (0..count)
            .map(|_| {
                let start = starts[next() % starts.len()];
                let len = 1 + next() % 4;
                let body: String = (0..len).map(|_| pieces[next() % pieces.len()]).collect();
                [start, body.as_str()].concat()
            })
            .collect()
    }

    /// The words whose expansion reaches an enforcement file (a path
    /// strictly inside `.codeflow`, links resolved), and every command the
    /// guard allows although one of them writes or removes what the word
    /// expands to: `rm WORD`, `printf x > WORD` and a producer feeding
    /// `xargs rm`.
    #[cfg(unix)]
    fn guard_misses(
        root: &Path,
        words: &[String],
        records: &[Vec<String>],
    ) -> (usize, Vec<String>) {
        let policy = default_policy();
        let refused = |command: &str| {
            evaluate_report_at(command, &ctx(&policy, "task/x"), root)
                .violations
                .iter()
                .any(|v| v.rule == "git.hook_integrity")
        };
        let inside = root.join(".codeflow");
        let mut reaching = 0;
        let mut missed = Vec::new();
        for (word, record) in words.iter().zip(records) {
            let reaches = record.iter().any(|path| {
                std::fs::canonicalize(root.join(path))
                    .is_ok_and(|real| real.starts_with(&inside) && real != inside)
            });
            if !reaches {
                continue;
            }
            reaching += 1;
            for command in [
                format!("rm {word}"),
                format!("printf x > {word}"),
                format!("printf '%s\\0' {word} | xargs -0 rm"),
            ] {
                if !refused(&command) {
                    missed.push(command);
                }
            }
        }
        (reaching, missed)
    }

    /// Whole command words, generated from a fixed seed, through the guard's
    /// own tokenizer and checks against Bash's real expansion (TSK-216 round
    /// 16). In a fixture repository holding `alias -> .codeflow` and a few
    /// plain files, Bash prints what each word expands to (brace, tilde and
    /// pathname expansion and quote removal; the alphabet has no `$`,
    /// backquote, blank or command separator, so nothing runs). Whenever an
    /// expansion resolves to an enforcement file, the guard must refuse both
    /// `rm WORD` and a producer feeding `xargs rm`. Skipped only without
    /// Bash.
    #[cfg(unix)]
    #[test]
    fn test_random_words_are_refused_whenever_bash_reaches_an_enforcement_file() {
        let (_dir, root) = expansion_fixture();
        let starts = ["alias/", ".codeflow/", "", "~+/alias/"];
        let words = random_words(0x2016_1600_5eed_0001, 3000, &starts, EXPANSION_PIECES);
        let Some(records) = bash_expansions(&root, &words) else {
            eprintln!("bash is not available; skipped");
            return;
        };
        let (reaching, missed) = guard_misses(&root, &words, &records);
        assert!(
            reaching >= 50,
            "the words reach enforcement files often enough: {reaching}"
        );
        assert!(
            missed.is_empty(),
            "bash reaches an enforcement file and the guard allows {} commands: {:?}",
            missed.len(),
            &missed[..missed.len().min(20)]
        );
    }

    /// The pieces the zsh words add to [`EXPANSION_PIECES`]: glob
    /// qualifiers, groups and alternation, numeric ranges and the
    /// extended-glob operators. A `|` appears only inside a group, so no
    /// word becomes a pipeline that runs something.
    #[cfg(unix)]
    const ZSH_PIECES: &[&str] = &[
        "(D)",
        "(.)",
        "(/)",
        "(N)",
        "(-.)",
        "(@)",
        "(#q)",
        "(",
        ")",
        "(p|x)",
        "(*/)#",
        "^",
        "#",
        "##",
        "~",
        "~x",
        "<1-9>",
        "<->",
        "*/",
        ".review-hidden",
        "7",
        "out/",
    ];

    /// The guard against zsh's real expansion, the way the Bash test above
    /// compares it with Bash (TSK-216 round 17): words from a fixed seed,
    /// built from the same safe alphabet plus zsh's glob qualifiers
    /// (`(D)` admits names that start with `.` in every component), groups,
    /// numeric ranges and extended-glob operators, are printed by `zsh -f`,
    /// once as zsh starts and once with `extendedglob`. Whenever an
    /// expansion resolves to an enforcement file, the guard must refuse the
    /// word as a direct target, a redirect target and a producer for
    /// `xargs rm`. Skipped only without zsh.
    #[cfg(unix)]
    #[test]
    fn test_random_words_are_refused_whenever_zsh_reaches_an_enforcement_file() {
        let (_dir, root) = expansion_fixture();
        let pieces: Vec<&str> = EXPANSION_PIECES.iter().chain(ZSH_PIECES).copied().collect();
        let starts = [
            "alias/",
            ".codeflow/",
            "",
            "build/",
            "build/*/",
            "out/",
            "~+/alias/",
        ];
        // The reviewers' round 17 and 18 words lead the random ones.
        let mut words: Vec<String> = [
            "build/*/policy.json(D)",
            "build/*(D)/policy.json",
            "out/<1-9>/policy.json",
            "out/(*/)#policy.json",
            "(alias|x)/policy.json",
            "alias/policy.json~x",
        ]
        .map(str::to_string)
        .to_vec();
        words.extend(random_words(0x2017_1700_5eed_0001, 3000, &starts, &pieces));
        let mut reaching = 0;
        let mut missed = Vec::new();
        for extended in [false, true] {
            let Some(records) = zsh_expansions(&root, &words, extended) else {
                eprintln!("zsh is not available; skipped");
                return;
            };
            let (reached, found) = guard_misses(&root, &words, &records);
            reaching += reached;
            missed.extend(found);
        }
        assert!(
            reaching >= 50,
            "the words reach enforcement files often enough: {reaching}"
        );
        assert!(
            missed.is_empty(),
            "zsh reaches an enforcement file and the guard allows {} commands: {:?}",
            missed.len(),
            &missed[..missed.len().min(20)]
        );
    }

    /// A glob over more entries than the guard reads stops, and is then
    /// judged by the directory it starts from: harmless there, it passes.
    #[test]
    fn test_glob_over_many_entries_is_judged_by_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let many = dir.path().join("many");
        std::fs::create_dir_all(&many).unwrap();
        for n in 0..=GLOB_ENTRY_LIMIT {
            std::fs::write(many.join(format!("f{n}")), "").unwrap();
        }
        assert_eq!(
            WordGlob::new("*", &many).expand(),
            Err(GlobStop::TooManyEntries)
        );
        assert!(glob_reach("many/*", dir.path(), dir.path()).is_none());
    }

    /// A file name that is not UTF-8 counts as a match of any wildcard
    /// component, so the guard gives a verdict instead of panicking.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_expand_glob_counts_a_non_utf8_name_as_a_match() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsStr::from_bytes(b"bad\xff.o");
        std::fs::write(dir.path().join(name), "").unwrap();
        let found = WordGlob::new("*.o", dir.path()).expand().unwrap();
        assert_eq!(found, vec![dir.path().join(name)]);
        let line = format!("printf x {}/* | xargs rm", dir.path().display());
        assert!(line_names(&line, dir.path(), dir.path()).is_none());
    }

    /// The text floor matches an enforcement path or worktree folder as a
    /// whole name, wherever it sits in an argument, and never inside a
    /// longer name or a child of a folder it only holds.
    #[test]
    fn test_text_floor_matches_whole_names() {
        let policy = [".codeflow", "policy.json"].join("/");
        for (text, named) in [
            (format!("w {policy}"), true),
            (format!("1W ./{policy};p"), true),
            (format!("x/{policy}"), true),
            ("rm -rf .codeflow".to_string(), true),
            ("rm -rf '.codeflow/'".to_string(), true),
            ("rm .codeflow/*".to_string(), true),
            ("sh -c 'rm .git/hooks/pre-commit'".to_string(), true),
            (format!("{policy}.bak"), false),
            ("docs/.codeflow/notes.md".to_string(), false),
            (".gitignore".to_string(), false),
            ("s/.codeflow/x/".to_string(), false),
            ("my.codeflow".to_string(), false),
        ] {
            assert_eq!(enforcement_text(&text).is_some(), named, "{text}");
        }
        for (text, named) in [
            (".worktrees", true),
            ("ls .worktrees | xargs rm", true),
            (".claude/worktrees/w/", true),
            (".worktrees/v*", true),
            (".worktrees/v/target", false),
            ("my.worktrees", false),
        ] {
            assert_eq!(worktree_text(text).is_some(), named, "{text}");
        }
    }

    /// Delete targets resolve through globs and variables the line does not
    /// set; a command substitution or a variable the line sets does not.
    #[test]
    fn test_delete_targets_resolve_only_what_the_guard_can_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a1")).unwrap();
        let cwd = dir.path();
        assert_eq!(
            resolve_targets("a*", cwd, "rm -rf a*").map(|p| p.len()),
            Some(1)
        );
        assert!(resolve_targets("$d", cwd, "d=x; rm -rf $d").is_none());
        assert!(resolve_targets("$(pwd)", cwd, "rm -rf $(pwd)").is_none());
        assert!(resolve_targets("{a,b}", cwd, "rm -rf {a,b}").is_none());
        // `PATH` is set wherever the tests run.
        assert!(resolve_targets("$PATH/x", cwd, "rm -rf $PATH/x").is_some());
        assert!(resolve_targets("${PATH}/x", cwd, "rm -rf ${PATH}/x").is_some());
        assert!(resolve_targets("$PATH/x", cwd, "PATH=/; rm -rf $PATH/x").is_none());
    }

    /// TSK-216 review findings 2 and 3, against the `sed` on this machine:
    /// every file a form changes or creates is among the paths the guard
    /// judges for it. Forms the local `sed` rejects are skipped, so BSD and
    /// GNU forms run where each is native.
    #[cfg(unix)]
    #[test]
    fn test_native_sed_writes_only_paths_the_guard_judges() {
        let forms: &[&[&str]] = &[
            &["-i", "", "s/a/b/", "f"],
            &["-i", "", "-e", "s/a/b/", "-l", "f"],
            &["-li", "", "s/a/b/", "f"],
            &["-I", "", "s/a/b/", "f"],
            &["-i", ".bak", "s/a/b/", "f"],
            &["-n", "w out", "f"],
            &["-e", "s/a/b/w out", "f"],
            &["-e", "1W out", "f"],
            &["-i", "", "-e", "w out", "f"],
            &["-i", "s/a/b/", "f"],
            &["-i.bak", "s/a/b/", "f"],
            &["--in-pl", "s/a/b/", "f"],
            &["--expression=w out", "f"],
        ];
        let snapshot = |dir: &Path| -> std::collections::BTreeMap<String, Vec<u8>> {
            std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        std::fs::read(e.path()).unwrap_or_default(),
                    )
                })
                .collect()
        };
        let mut ran: Vec<String> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for form in forms {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("f"), "a\n").unwrap();
            let before = snapshot(dir.path());
            let status = std::process::Command::new("sed")
                .args(*form)
                .current_dir(dir.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                skipped.push(format!("{form:?}"));
                continue;
            }
            ran.push(format!("{form:?}"));
            let args: Vec<String> = form.iter().map(ToString::to_string).collect();
            let SedRead::Read(mut judged) = sed_script_writes(&args, dir.path()) else {
                panic!("script fixture unreadable")
            };
            if requests_in_place(&args) {
                judged.extend(sed_file_operands(&args).into_iter().map(String::from));
            }
            for (name, bytes) in snapshot(dir.path()) {
                if before.get(&name) != Some(&bytes) {
                    assert!(
                        judged.contains(&name),
                        "{form:?} wrote {name}; judged {judged:?}"
                    );
                }
            }
        }
        // Which forms this machine's `sed` accepted: BSD and GNU each
        // reject the other's spellings, so the set differs by platform.
        eprintln!(
            "native sed ran {} of {} forms\nran: {}\nrejected by this sed: {}",
            ran.len(),
            forms.len(),
            ran.join(" "),
            skipped.join(" ")
        );
        assert!(
            ran.len() >= 6,
            "the local sed ran only {ran:?}; it rejected {skipped:?}"
        );
    }

    /// Read-only `sed` over an integrity path, including scripts and script
    /// files whose value carries an `i`. A whole-cluster scan would read the
    /// value as flags and block the read.
    #[test]
    fn test_integrity_stream_read_with_sed_allowed() {
        let p = default_policy();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("script.sed"), "p\n").unwrap();
        for cmd in [
            "sed -n 1,5p .codeflow/policy.json",
            "sed -e s/block/off/ .codeflow/policy.json",
            "sed -es/input/output/ .codeflow/policy.json",
            "sed -f script.sed .codeflow/policy.json",
            "sed -n -e p .codeflow/policy.json",
            "sed -Ef script.sed .codeflow/policy.json",
        ] {
            let report = evaluate_report_at(cmd, &ctx(&p, "feat/x"), dir.path());
            assert!(
                report.violations.is_empty(),
                "{cmd}: {:?}",
                report.violations
            );
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
        // GNU `sed` reads its long options with `getopt_long`, which takes
        // any unambiguous prefix (TSK-216 review); BSD `sed` has none.
        assert_eq!(SED_OPTIONS.resolve_long("--in-place"), Some("--in-place"));
        assert_eq!(SED_OPTIONS.resolve_long("--in-pl"), Some("--in-place"));
        assert_eq!(SED_OPTIONS.resolve_long("--s"), None);
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

    /// TSK-216 AC-4: a forced branch move rewrites a protected ref as
    /// `update-ref` does, so it is refused the same way; creating or moving
    /// a feature branch, and an unforced create of a new protected name,
    /// stay allowed.
    #[test]
    fn test_forced_branch_move_of_protected_blocked() {
        let p = default_policy();
        for cmd in [
            "git branch -f main HEAD~3",
            "git branch --force main HEAD~3",
            "git branch -f main",
            "git branch -qf main origin/feat",
            "git branch -M feat/x main",
            "git branch -m -f feat/x main",
            "git branch -C feat/x main",
            "git branch -c --force feat/x main",
            "git checkout -B main HEAD~3",
            "git checkout -Bmain",
            "git switch -C main HEAD~3",
            "git switch --force-create main HEAD~3",
            "git switch --force-create=main",
            // TSK-216 review: formatting flags, abbreviations, expressions.
            "git branch -fv main HEAD~1",
            "git branch -vf main HEAD~1",
            "git switch --force-c main HEAD~1",
            "git switch --force-cr=main",
            // No lookup in this context: an expression is unresolved and
            // judged as a protected branch.
            "git branch -f @{-1} HEAD~1",
            "git switch -C feat/x@{upstream} HEAD~1",
            "git checkout -B - HEAD~1",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.local_ref_protection"), "{cmd}: {v:?}");
        }
        let v = evaluate("git branch -M main", &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.local_ref_protection"), "{v:?}");
        for cmd in [
            "git branch -f feat/y HEAD~3",
            "git branch feat/y main",
            "git branch -m feat/x feat/y",
            "git branch -m feat/x main",
            "git branch -c feat/x main",
            "git branch -f -u origin/main main",
            "git branch --list -f main",
            "git branch -f refs/heads/main HEAD~1",
            "git branch -f origin/main HEAD~1",
            "git branch -v",
            "git switch --force-c feat/y HEAD~1",
            "git checkout -B feat/y main",
            "git checkout -b feat/y main",
            "git checkout main",
            "git switch -c feat/y main",
            "git switch -C feat/y main",
            "git switch main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(v.is_empty(), "{cmd}: {v:?}");
        }
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
            // A launcher before `git` does not hide the alias body.
            "command git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git push origin main'",
            "env GIT_TRACE=0 git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git push'",
            "/usr/bin/git config --global alias.x '!CODEFLOW_INTEGRATE_TOKEN=x git push'",
            // Launcher options do not hide the program either.
            "command -p git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git push'",
            "command -- git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git push'",
            "exec -a probe git config alias.x '!CODEFLOW_HUMAN_OVERRIDE=1 git push'",
            "command -p env CODEFLOW_HUMAN_OVERRIDE=1 git merge feat/y",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(
                has_rule(&v, "git.override_token_laundering"),
                "{cmd}: {v:?}"
            );
        }
    }

    #[test]
    fn test_launcher_options_do_not_hide_git() {
        let p = default_policy();
        for cmd in [
            "command -p git push origin main",
            "command -- git push origin main",
            "builtin -- git push origin main",
            "exec -a probe git push origin main",
            "exec -cl git push origin main",
            "exec -aprobea git push origin main",
            "exec -ca probe git push origin main",
            "nohup git push origin main",
            "/usr/bin/time git push origin main",
            "command time -p git push origin main",
            "/usr/bin/time -o out.txt git push origin main",
            // An option value that looks like help is still a value.
            "/usr/bin/time --format --help git push origin main",
            "/usr/bin/time --output --version git push origin main",
            "/usr/bin/time --output out.txt git push origin main",
            "/usr/bin/time --output=out.txt git push origin main",
            // Fails closed: a help option is skipped, not read as a no-op.
            "nohup --help git push origin main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert!(has_rule(&v, "git.push_to_protected"), "{cmd}: {v:?}");
        }
        // `command -v git` only looks git up, and a name attached to `-a`
        // is not followed by another: `echo` is the program here.
        assert!(evaluate("command -v git", &ctx(&p, "main")).is_empty());
        let echo = evaluate(
            "exec -aprobea echo git push origin main",
            &ctx(&p, "feat/x"),
        );
        assert!(echo.is_empty(), "{echo:?}");
        // The shell after an attached name still has its heredoc judged.
        let heredoc = "exec -aprobea bash -s cat <<'EOF'\nCODEFLOW_HUMAN_OVERRIDE=1 git push origin main\nEOF";
        let laundered =
            "/usr/bin/time --format --help env CODEFLOW_HUMAN_OVERRIDE=1 git push origin main";
        let v = evaluate(laundered, &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.override_token_laundering"), "{v:?}");
        let v = evaluate(heredoc, &ctx(&p, "feat/x"));
        assert!(has_rule(&v, "git.override_token_laundering"), "{v:?}");
    }

    #[test]
    fn test_laundering_extra_forms_do_not_flag_unrelated() {
        let p = default_policy();
        assert!(evaluate("/usr/bin/env FOO=1 git status", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("declare -x EDITOR=vim", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git config alias.st status", &ctx(&p, "feat/x")).is_empty());
        // Printed text that names git config is not a git invocation.
        assert!(evaluate(
            "printf '%s\\n' git config '!CODEFLOW_HUMAN_OVERRIDE=1'",
            &ctx(&p, "feat/x")
        )
        .is_empty());
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
            branch_lookup: None,
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
