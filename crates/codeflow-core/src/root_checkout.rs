//! The root checkout and workspace mode (TSK-165).
//!
//! Task work happens in a linked worktree on a feature branch. The root
//! checkout, the repository's main working tree, stays on its root branch:
//! the repository's default branch, or the branch `git.root_branch` names.
//! An umbrella repository that holds `CodeFlow` projects inside it (workspace
//! mode) keeps its root checkout on [`WORKSPACE_ROOT_BRANCH`] by convention.
//!
//! This module holds the facts every surface reads (whether a path is the
//! root checkout, its branch, the resolved root branch, who is acting, the
//! nested repositories, where linked worktrees live) and every message text,
//! so git-guard, the git hooks, `doctor` and `init` say the same thing.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::git::GitName;
use crate::hooks::policy::{GitPolicy, PolicyLevel};
use crate::hooks::{Violation, HUMAN_OVERRIDE_ENV};

/// `CodeFlow`'s convention name for an umbrella workspace's root branch. Every
/// surface that sets up or describes workspace mode reads this constant.
pub const WORKSPACE_ROOT_BRANCH: &str = "integration/workspace";

/// The policy rule that judges a commit at the root checkout.
pub const COMMIT_RULE: &str = "git.root_checkout_commits";

/// The policy key that names the root branch.
pub const ROOT_BRANCH_KEY: &str = "git.root_branch";

/// The policy key that lists where linked worktrees may live.
pub const LOCATIONS_KEY: &str = "git.worktree_locations";

/// The doctor check for nested repositories an umbrella must ignore.
pub const NESTED_RULE: &str = "workspace.nested_repositories";

/// The route every message about task work at the root names.
pub const WORKTREE_ROUTE: &str = "git worktree add .worktrees/<slug> -b <branch>";

/// Environment variables the harnesses set in the shells their agents run.
/// A git hook that sees any of them treats the actor as an agent.
///
/// - `CLAUDECODE`: Claude Code, observed in its Bash tool (2.1.281).
/// - `CODEX_SESSION_ID`, `CODEX_THREAD_ID`, `CODEX_CI`: Codex CLI
///   (`openai/codex` `4fd5745`, `core/src/exec_env.rs`,
///   `core/src/unified_exec/process_manager.rs`).
/// - `GROK_SESSION_ID`: Grok Build (`xai-grok-build` `f0e3be1`,
///   `xai-grok-shell/src/session/agent_rebuild.rs`).
pub const AGENT_MARKERS: [&str; 5] = [
    "CLAUDECODE",
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_CI",
    "GROK_SESSION_ID",
];

/// The default `git.worktree_locations`: `CodeFlow`'s own folder and the
/// folders the harnesses manage.
///
/// - `.worktrees`: `CodeFlow`'s convention, inside the repository.
/// - `.claude/worktrees`: the Claude desktop app, inside the repository.
/// - `$CODEX_HOME/worktrees`: Codex (`~/.codex` when `CODEX_HOME` is unset;
///   `openai/codex` `4fd5745`, `worktree/src/settings.rs`).
/// - `$GROK_HOME/worktrees`, `$GROK_HOME/worktree_pool`: Grok Build (`~/.grok`
///   when `GROK_HOME` is unset; `xai-grok-build` `f0e3be1`,
///   `xai-fast-worktree/src/discovery.rs`).
pub const DEFAULT_WORKTREE_LOCATIONS: [&str; 5] = [
    ".worktrees",
    ".claude/worktrees",
    "$CODEX_HOME/worktrees",
    "$GROK_HOME/worktrees",
    "$GROK_HOME/worktree_pool",
];

/// A read of one environment variable; tests pass their own.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The process environment, for production callers.
#[must_use]
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

// ---------------------------------------------------------------------------
// Who is acting
// ---------------------------------------------------------------------------

/// Who a git hook believes is committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// A harness marker is set; the named variable is the first one found.
    Agent(&'static str),
    /// `CODEFLOW_HUMAN_OVERRIDE` is set to `1`, the only value the git layer
    /// honours: the actor is treated as a human even inside a marked shell
    /// (ADR-0007). git-guard never reads it.
    HumanOverride,
    /// No marker: the hook cannot tell, and treats the actor as a human.
    Unmarked,
}

/// Decide the actor from the environment.
#[must_use]
pub fn actor(env: EnvLookup<'_>) -> Actor {
    if env(HUMAN_OVERRIDE_ENV).is_some_and(|v| v == "1") {
        return Actor::HumanOverride;
    }
    AGENT_MARKERS
        .iter()
        .find(|name| env(name).is_some_and(|v| !v.trim().is_empty()))
        .map_or(Actor::Unmarked, |name| Actor::Agent(name))
}

/// The level a git hook applies: the policy's level for an agent, at most a
/// warning for anyone else, so a human at their own terminal is never
/// stopped by this rule.
#[must_use]
pub fn hook_level(level: PolicyLevel, actor: Actor) -> PolicyLevel {
    match actor {
        Actor::Agent(_) => level,
        Actor::HumanOverride | Actor::Unmarked => {
            if level.is_active() {
                PolicyLevel::Warn
            } else {
                level
            }
        }
    }
}

/// The sentence a hook adds to say how it judged the actor.
#[must_use]
pub fn actor_note(actor: Actor) -> String {
    match actor {
        Actor::Agent(marker) => {
            format!("{marker} is set, so this commit comes from an agent session")
        }
        Actor::HumanOverride => format!(
            "{HUMAN_OVERRIDE_ENV} is 1, so this commit is treated as a human's and proceeds"
        ),
        Actor::Unmarked => "no harness marker is set, so this commit is treated as a \
                            human's and proceeds"
            .to_string(),
    }
}

// ---------------------------------------------------------------------------
// Branch names and the root branch
// ---------------------------------------------------------------------------

/// Whether `name` is a valid branch name under `git check-ref-format
/// --branch`'s rules, checked without running git.
#[must_use]
pub fn is_valid_branch_name(name: &str) -> bool {
    if name.is_empty()
        || name == "@"
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
    {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == ' ' || "~^:?*[\\".contains(c))
    {
        return false;
    }
    name.split('/')
        // git's own rule is case-sensitive, so a plain suffix test is exact.
        .all(|part| !part.starts_with('.') && part.strip_suffix(".lock").is_none())
}

/// Where the resolved root branch came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootBranchSource {
    /// `git.root_branch` in the policy.
    Policy,
    /// The target of `refs/remotes/origin/HEAD`.
    OriginHead,
    /// The first `git.protected_branches` entry that exists as a local branch.
    ProtectedList,
    /// Nothing else applied: `main`.
    Fallback,
}

impl fmt::Display for RootBranchSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Policy => "set by git.root_branch",
            Self::OriginHead => "the default branch, from origin/HEAD",
            Self::ProtectedList => "the default branch, from git.protected_branches",
            Self::Fallback => "the default branch, assumed main",
        })
    }
}

/// The branch the root checkout holds, and where that answer came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootBranch {
    /// The branch name, exact; shown through its `Display`.
    pub name: GitName,
    pub source: RootBranchSource,
}

/// The repository's default branch: `origin/HEAD`, then the first protected
/// branch that exists locally, then `main`.
#[must_use]
pub fn default_branch(repo: &git2::Repository, policy: &GitPolicy) -> RootBranch {
    if let Some(name) = repo
        .find_reference("refs/remotes/origin/HEAD")
        .ok()
        // OS text rule (issue 79): the name is exact bytes, compared with the
        // checked-out branch byte for byte, so a root branch that is not valid
        // UTF-8 is still recognised and no other branch takes its place.
        .and_then(|r| crate::git::name::symbolic_target(&r))
        .and_then(|t| t.strip_prefix(b"refs/remotes/origin/"))
    {
        return RootBranch {
            name,
            source: RootBranchSource::OriginHead,
        };
    }
    for pattern in &policy.protected_branches {
        if is_valid_branch_name(pattern)
            && repo.find_branch(pattern, git2::BranchType::Local).is_ok()
        {
            return RootBranch {
                name: GitName::from_text(pattern),
                source: RootBranchSource::ProtectedList,
            };
        }
    }
    RootBranch {
        name: GitName::from_text("main"),
        source: RootBranchSource::Fallback,
    }
}

/// The root branch: `git.root_branch` when set, else the default branch.
#[must_use]
pub fn root_branch(repo: &git2::Repository, policy: &GitPolicy) -> RootBranch {
    let configured = policy.root_branch.as_str();
    if configured.is_empty() {
        default_branch(repo, policy)
    } else {
        RootBranch {
            name: GitName::from_text(configured),
            source: RootBranchSource::Policy,
        }
    }
}

// ---------------------------------------------------------------------------
// The root checkout's state
// ---------------------------------------------------------------------------

/// What the root checkout's HEAD holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// A branch, possibly unborn.
    Branch(GitName),
    /// A detached HEAD at the short commit id.
    Detached(String),
}

impl Head {
    fn describe(&self) -> String {
        match self {
            Self::Branch(name) => format!("'{name}'"),
            Self::Detached(id) => format!("a detached HEAD at {id}"),
        }
    }
}

/// Read HEAD, including an unborn branch.
#[must_use]
pub fn head(repo: &git2::Repository) -> Option<Head> {
    match repo.head() {
        Ok(reference) => {
            if reference.is_branch() {
                // OS text rule (issue 79): a branch name that is not valid
                // UTF-8 stays a branch, exact.
                Some(Head::Branch(crate::git::name::reference_shorthand(
                    &reference,
                )))
            } else {
                let id = reference.target()?.to_string();
                Some(Head::Detached(id.chars().take(9).collect()))
            }
        }
        Err(_) => crate::git::name::symbolic_target(&repo.find_reference("HEAD").ok()?)?
            .strip_prefix(b"refs/heads/")
            .map(Head::Branch),
    }
}

/// Whether `repo` is a repository's root checkout: its main working tree,
/// not a linked worktree and not bare.
#[must_use]
pub fn is_root_checkout(repo: &git2::Repository) -> bool {
    !repo.is_bare() && !repo.is_worktree()
}

/// The repository label messages use: the root checkout's path.
fn label(root: &Path) -> String {
    root.display().to_string()
}

/// A commit at the root checkout on a branch other than its root branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFinding {
    pub repo: String,
    pub head: Head,
    pub root_branch: RootBranch,
}

impl CommitFinding {
    /// The finding: the repository, the branch found and the root branch.
    #[must_use]
    pub fn message(&self) -> String {
        format!(
            "{COMMIT_RULE}: a commit at the root checkout of {} on {}; its root branch is '{}' ({})",
            self.repo,
            self.head.describe(),
            self.root_branch.name,
            self.root_branch.source
        )
    }

    /// The exact next step: the catalogued remedy.
    #[must_use]
    pub fn next_step(&self) -> crate::remedy::Remedy {
        let root = self.root_branch.name.to_string();
        crate::remedy::ROOT_CHECKOUT_COMMIT.with(&[("root", &root)])
    }
}

impl CommitFinding {
    /// The policy violation at `level` for `what` (the command or event
    /// that commits), with `note` (how a hook judged the actor) appended.
    #[must_use]
    pub fn violation(&self, what: &str, level: PolicyLevel, note: Option<&str>) -> Violation {
        let mut message = format!(
            "{what} at the root checkout of {} on {}; its root branch is '{}' ({})",
            self.repo,
            self.head.describe(),
            self.root_branch.name,
            self.root_branch.source
        );
        if let Some(note) = note {
            message.push_str("; ");
            message.push_str(note);
        }
        Violation::new(COMMIT_RULE, level, message, self.next_step())
    }
}

/// Subcommands that create commits in the working tree they run in.
pub const COMMIT_SUBCOMMANDS: [&str; 7] = [
    "commit",
    "merge",
    "cherry-pick",
    "revert",
    "am",
    "rebase",
    "pull",
];

/// A repository's root checkout, as git-guard reads it before judging the
/// commands of one line: where it is, its root branch and its HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootCheckout {
    pub repo: String,
    pub root_branch: RootBranch,
    pub head: Option<Head>,
}

impl RootCheckout {
    /// The facts for `repo` under `policy` (the root checkout's own), or
    /// `None` when `repo` is a linked worktree, bare, or has no working
    /// tree.
    #[must_use]
    pub fn read(repo: &git2::Repository, policy: &GitPolicy) -> Option<Self> {
        if !is_root_checkout(repo) {
            return None;
        }
        Some(Self {
            repo: label(&canonical(repo.workdir()?)),
            root_branch: root_branch(repo, policy),
            head: head(repo),
        })
    }

    /// The facts for the repository that holds `path`, when `path` is in
    /// its root checkout.
    #[must_use]
    pub fn at(path: &Path, policy: &GitPolicy) -> Option<Self> {
        Self::read(&git2::Repository::discover(path).ok()?, policy)
    }

    /// The finding for a commit made here on `branch`, where an empty name
    /// is a detached HEAD. `None` on the root branch.
    #[must_use]
    pub fn commit_on(&self, branch: &str) -> Option<CommitFinding> {
        // The hook plane's text for a branch that is not valid UTF-8 never
        // equals a real branch name, so it is never the root branch.
        self.commit_on_name(
            (!branch.is_empty())
                .then(|| GitName::from_text(branch))
                .as_ref(),
        )
    }

    /// The finding for a commit made here on `branch` (`None` is a detached
    /// HEAD), compared with the root branch byte for byte.
    #[must_use]
    pub fn commit_on_name(&self, branch: Option<&GitName>) -> Option<CommitFinding> {
        if branch == Some(&self.root_branch.name) {
            return None;
        }
        let head = if let Some(name) = branch {
            Head::Branch(name.clone())
        } else {
            match &self.head {
                Some(detached @ Head::Detached(_)) => detached.clone(),
                _ => Head::Detached("an unknown commit".to_string()),
            }
        };
        Some(CommitFinding {
            repo: self.repo.clone(),
            head,
            root_branch: self.root_branch.clone(),
        })
    }
}

/// The subcommands whose in-progress operation `--abort` or `--quit` ends.
const ENDABLE_SUBCOMMANDS: [&str; 5] = ["merge", "cherry-pick", "revert", "am", "rebase"];

/// Whether `git <sub> <rest>` only ends an operation, which creates no
/// commit. git takes `--abort` and `--quit` alone, so any other argument
/// means they are something else, such as a message value.
fn ends_an_operation(sub: &str, rest: &[String]) -> bool {
    ENDABLE_SUBCOMMANDS.contains(&sub)
        && matches!(rest, [only] if only == "--abort" || only == "--quit")
}

/// git-guard's verdict for `git <sub> <rest>` run at this root checkout on
/// `branch` (empty: detached), at the policy's `level`. git-guard runs only
/// in an agent session, so it applies the level without reading the
/// environment. Ending an operation (`--abort`, `--quit`) creates no commit.
#[must_use]
pub fn guard_violation(
    sub: &str,
    rest: &[String],
    branch: &str,
    root: &RootCheckout,
    level: PolicyLevel,
) -> Option<Violation> {
    if !level.is_active() || !COMMIT_SUBCOMMANDS.contains(&sub) || ends_an_operation(sub, rest) {
        return None;
    }
    root.commit_on(branch)
        .map(|finding| finding.violation(&format!("`git {sub}` would commit"), level, None))
}

/// A git hook's verdict for a commit in the working tree at `path`: the
/// policy's level for an agent, a warning for anyone else (see [`actor`]).
#[must_use]
pub fn hook_violation(path: &Path, policy: &GitPolicy, env: EnvLookup<'_>) -> Option<Violation> {
    let level = policy.root_checkout_commits;
    if !level.is_active() {
        return None;
    }
    let finding = commit_finding(path, policy)?;
    let who = actor(env);
    Some(finding.violation("a commit", hook_level(level, who), Some(&actor_note(who))))
}

/// Judge a commit in the working tree at `path`: a finding when `path` is
/// a root checkout whose HEAD is not its root branch. `None` in a linked
/// worktree, on the root branch, or when `path` is not a repository.
#[must_use]
pub fn commit_finding(path: &Path, policy: &GitPolicy) -> Option<CommitFinding> {
    let repo = git2::Repository::discover(path).ok()?;
    let root = RootCheckout::read(&repo, policy)?;
    match root.head.clone()? {
        Head::Branch(branch) => root.commit_on_name(Some(&branch)),
        detached @ Head::Detached(_) => Some(CommitFinding {
            repo: root.repo,
            head: detached,
            root_branch: root.root_branch,
        }),
    }
}

// ---------------------------------------------------------------------------
// Findings for doctor and init
// ---------------------------------------------------------------------------

/// How serious a report line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
}

/// One report line: the rule, what was found and the exact next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub rule: &'static str,
    pub message: String,
    /// Empty for an information line that asks nothing.
    pub next_step: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule, self.message)?;
        if self.next_step.is_empty() {
            Ok(())
        } else {
            write!(f, ". Next: {}", self.next_step)
        }
    }
}

/// What a nested repository is, for the messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedKind {
    /// It carries `.codeflow/`.
    CodeflowProject,
    /// A plain git repository.
    GitRepository,
}

impl NestedKind {
    #[must_use]
    pub fn noun(self) -> &'static str {
        match self {
            Self::CodeflowProject => "nested CodeFlow project",
            Self::GitRepository => "nested git repository",
        }
    }
}

/// How a nested repository is kept out of the umbrella's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreState {
    /// A tracked `.gitignore` ignores it.
    Tracked,
    /// Only a source other clones do not share ignores it (the file named).
    LocalOnly(String),
    /// Nothing ignores it.
    NotIgnored,
}

/// A nested repository under the umbrella.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedRepo {
    /// Path relative to the umbrella's root, with `/` separators.
    pub path: String,
    pub kind: NestedKind,
    pub ignore: IgnoreState,
}

impl NestedRepo {
    /// The `.gitignore` line that ignores it, its path escaped so git reads
    /// each character literally.
    #[must_use]
    pub fn ignore_line(&self) -> String {
        format!("/{}/", escape_ignore_path(&self.path))
    }
}

/// `path` as literal `.gitignore` pattern text: the glob characters and the
/// backslash are escaped. The line ends in `/`, so git trims no space from
/// it, and it starts with `/`, so a leading `!` or `#` is literal already.
fn escape_ignore_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '*' | '?' | '[' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Run git in `root` with `input` on stdin; stdout on success. Stdin is
/// written while stdout is drained, so output larger than a pipe cannot
/// deadlock the exchange ([`crate::git::output_with_input`]).
fn git_stdin(root: &Path, args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    let mut command = crate::git::command();
    command.arg("-C").arg(root).args(args);
    let out = crate::git::output_with_input(&mut command, input).ok()?;
    // check-ignore exits 1 when nothing matched, which is still an answer.
    if out.status.success() || out.status.code() == Some(1) {
        Some(out.stdout)
    } else {
        None
    }
}

/// For each relative directory path, the ignore file that matches it, if
/// any, from one `git check-ignore -v -n` call.
fn ignore_sources(root: &Path, dirs: &[String]) -> Vec<Option<String>> {
    check_ignore(root, &[], dirs).unwrap_or_else(|| vec![None; dirs.len()])
}

/// A throwaway repository with an empty exclude file, removed on drop.
struct ScratchRepo(PathBuf);

impl ScratchRepo {
    fn new() -> Option<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("codeflow-ignore-{}-{nanos}", std::process::id()));
        let scratch = Self(dir);
        let made = crate::git::command()
            .args(["init", "--quiet"])
            .arg(&scratch.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?
            .success();
        made.then_some(scratch)
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Like [`ignore_sources`], but reading only the `.gitignore` files in the
/// working tree: the rules a clone shares once they are committed. git runs
/// against a scratch repository, so this repository's `.git/info/exclude`
/// and any global excludes file take no part. The scratch run matches case
/// as git does in this repository (`core.ignoreCase`, runtime overrides
/// included), not as a fresh repository on this file system would. Falls
/// back to [`ignore_sources`] when no scratch repository can be made.
fn tree_ignore_sources(root: &Path, dirs: &[String]) -> Vec<Option<String>> {
    let ignore_case = format!("core.ignoreCase={}", effective_ignore_case(root));
    let scratch = ScratchRepo::new();
    let git_dir = scratch
        .as_ref()
        .and_then(|s| s.0.join(".git").to_str().map(str::to_string));
    let work_tree = root.to_str();
    match (git_dir, work_tree) {
        (Some(git_dir), Some(work_tree)) => check_ignore(
            root,
            &[
                "--git-dir",
                &git_dir,
                "--work-tree",
                work_tree,
                "-c",
                "core.excludesFile=",
                "-c",
                &ignore_case,
            ],
            dirs,
        )
        .unwrap_or_else(|| ignore_sources(root, dirs)),
        _ => ignore_sources(root, dirs),
    }
}

/// `core.ignoreCase` as git itself resolves it in `root`, runtime overrides
/// (`git -c`, `GIT_CONFIG_COUNT`) included; git's default, false, when it
/// is unset or unreadable.
fn effective_ignore_case(root: &Path) -> bool {
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "--type=bool", "core.ignoreCase"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "true")
}

/// One `git <prefix> check-ignore -v -n` call over `dirs`: for each, the
/// ignore file whose positive rule decides it. `None` when git fails.
fn check_ignore(root: &Path, prefix: &[&str], dirs: &[String]) -> Option<Vec<Option<String>>> {
    if dirs.is_empty() {
        return Some(Vec::new());
    }
    let mut input = Vec::new();
    for dir in dirs {
        input.extend_from_slice(dir.as_bytes());
        input.push(0);
    }
    let mut args = prefix.to_vec();
    args.extend(["check-ignore", "-v", "-n", "--no-index", "--stdin", "-z"]);
    let out = git_stdin(root, &args, &input)?;
    // Records of four NUL-terminated fields: source, line, pattern, path.
    let fields: Vec<&[u8]> = out.split(|b| *b == 0).collect();
    let mut by_path = std::collections::HashMap::new();
    for record in fields.chunks(4) {
        if record.len() < 4 {
            continue;
        }
        // OS text rule (issue 79): the paths asked about are valid text, so
        // an answer for a path that is not valid UTF-8 is not one of them and
        // is dropped, never read as a lookalike. A source that is not valid
        // text cannot be classified, so that directory reads as not ignored,
        // which only adds a warning.
        let (Ok(source), Ok(pattern), Ok(path)) = (
            std::str::from_utf8(record[0]),
            std::str::from_utf8(record[2]),
            std::str::from_utf8(record[3]),
        ) else {
            continue;
        };
        let source = source.to_string();
        let path = path.to_string();
        // A negated pattern (`!x`) matched but un-ignores the path.
        let ignored = !source.is_empty() && !pattern.starts_with('!');
        by_path.insert(path, ignored.then_some(source));
    }
    Some(
        dirs.iter()
            .map(|d| by_path.get(d).cloned().flatten())
            .collect(),
    )
}

/// Classify an ignore source: a tracked `.gitignore` in the tree, or a
/// source other clones do not share.
fn classify_source(repo: &git2::Repository, source: Option<&str>) -> IgnoreState {
    let Some(source) = source else {
        return IgnoreState::NotIgnored;
    };
    let relative = Path::new(source);
    let tracked = !relative.is_absolute()
        && !source.starts_with(".git/")
        && repo
            .index()
            .ok()
            .is_some_and(|index| index.get_path(relative, 0).is_some());
    if tracked {
        IgnoreState::Tracked
    } else {
        IgnoreState::LocalOnly(source.to_string())
    }
}

/// The `.git` entry of `dir`, when it holds one (a directory or a gitdir
/// file).
fn git_marker(dir: &Path) -> Option<PathBuf> {
    let marker = dir.join(".git");
    let meta = std::fs::symlink_metadata(&marker).ok()?;
    (meta.is_dir() || meta.is_file()).then_some(marker)
}

/// Whether the `.git` file at `marker` points into `common_dir/worktrees/`:
/// a linked worktree of this repository, not a nested repository.
fn is_own_worktree(marker: &Path, common_dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(marker) else {
        return false;
    };
    let Some(target) = crate::git::gitfile_dir(&bytes) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        marker.parent().unwrap_or(marker).join(target)
    };
    let worktrees = common_dir.join("worktrees");
    match (target.canonicalize(), worktrees.canonicalize()) {
        (Ok(t), Ok(w)) => t.starts_with(w),
        _ => false,
    }
}

/// The paths `.gitmodules` registers as submodules. Read from the file
/// itself: libgit2's submodule list also includes index gitlinks that
/// `.gitmodules` does not register.
fn submodule_paths(repo: &git2::Repository) -> BTreeSet<GitName> {
    let Some(file) = repo.workdir().map(|w| w.join(".gitmodules")) else {
        return BTreeSet::new();
    };
    if !file.is_file() {
        return BTreeSet::new();
    }
    let Ok(config) = git2::Config::open(&file) else {
        return BTreeSet::new();
    };
    let Ok(mut entries) = config.entries(Some(r"submodule\..*\.path")) else {
        return BTreeSet::new();
    };
    let mut paths = BTreeSet::new();
    while let Some(Ok(entry)) = entries.next() {
        // OS text rule (issue 79): a registered path is kept as its exact
        // bytes, so it is compared with an index path byte for byte.
        {
            let mut value = entry.value_bytes();
            while let Some(trimmed) = value.strip_suffix(b"/") {
                value = trimmed;
            }
            let value: Vec<u8> = value
                .iter()
                .map(|byte| if *byte == b'\\' { b'/' } else { *byte })
                .collect();
            paths.insert(GitName::from_vec(value));
        }
    }
    paths
}

/// Gitlinks in the index that `.gitmodules` does not register.
fn stray_gitlinks(repo: &git2::Repository, submodules: &BTreeSet<GitName>) -> Vec<String> {
    let Ok(index) = repo.index() else {
        return Vec::new();
    };
    index
        .iter()
        .filter(|e| e.mode == 0o160_000)
        .map(|e| GitName::from_bytes(&e.path))
        .filter(|p| !submodules.contains(p))
        .map(|p| p.display().to_string())
        .collect()
}

/// Find the nested repositories under the root checkout at `root`, by
/// git's own marker, at any depth. The walk skips `.git`, `.worktrees/`,
/// registered submodules, symbolic links and folders a tracked ignore rule covers,
/// and does not descend into a nested repository once found. Linked
/// worktrees of this repository are not nested repositories.
#[must_use]
pub fn nested_repositories(repo: &git2::Repository) -> Vec<NestedRepo> {
    let Some(root) = repo.workdir().map(Path::to_path_buf) else {
        return Vec::new();
    };
    let common_dir = repo.commondir().to_path_buf();
    let submodules = submodule_paths(repo);
    let mut found = Vec::new();
    let mut level = vec![String::new()];
    while !level.is_empty() {
        let mut candidates = Vec::new();
        for dir in &level {
            let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                // OS text rule (issue 79): a folder whose name is not valid
                // UTF-8 cannot be named in a path of text, and a lossy
                // spelling would read another folder, so it is not walked.
                // This only reports nested repositories (advice), so one under
                // such a name is not reported.
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            for name in names {
                if name == ".git" || (dir.is_empty() && name == ".worktrees") {
                    continue;
                }
                let rel = if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                };
                if !submodules.contains(&GitName::from_text(&rel)) {
                    candidates.push(rel);
                }
            }
        }
        let sources = ignore_sources(&root, &candidates);
        let mut next = Vec::new();
        for (rel, source) in candidates.into_iter().zip(sources) {
            let abs = root.join(&rel);
            let ignore = classify_source(repo, source.as_deref());
            if let Some(marker) = git_marker(&abs) {
                if marker.is_file() && is_own_worktree(&marker, &common_dir) {
                    continue;
                }
                let kind = if abs.join(".codeflow").is_dir() {
                    NestedKind::CodeflowProject
                } else {
                    NestedKind::GitRepository
                };
                found.push(NestedRepo {
                    ignore,
                    path: rel,
                    kind,
                });
            } else if ignore != IgnoreState::Tracked {
                // Only a shared rule may hide a folder: one ignored by a
                // local exclude, a global excludes file or an uncommitted
                // .gitignore is still searched.
                next.push(rel);
            }
        }
        level = next;
    }
    prefer_shared_rules(repo, &root, &mut found);
    found
}

/// Classify each found repository by the working tree's own `.gitignore`
/// files first: a local rule on a parent folder hides from git the shared
/// rule that a clone would apply, so the shared one decides when it
/// exists, and the local rule only when no shared rule matches.
fn prefer_shared_rules(repo: &git2::Repository, root: &Path, found: &mut [NestedRepo]) {
    let pending: Vec<usize> = (0..found.len())
        .filter(|i| found[*i].ignore != IgnoreState::Tracked)
        .collect();
    if pending.is_empty() {
        return;
    }
    let paths: Vec<String> = pending.iter().map(|i| found[*i].path.clone()).collect();
    for (i, source) in pending.into_iter().zip(tree_ignore_sources(root, &paths)) {
        if source.is_some() {
            found[i].ignore = classify_source(repo, source.as_deref());
        }
    }
}

/// Doctor's findings for nested repositories.
#[must_use]
pub fn nested_findings(
    repo_label: &str,
    nested: &[NestedRepo],
    gitlinks: &[String],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for n in nested {
        match &n.ignore {
            IgnoreState::Tracked => {}
            IgnoreState::NotIgnored => out.push(Finding {
                severity: Severity::Warn,
                rule: NESTED_RULE,
                message: format!(
                    "{repo_label} contains the {} '{}', which no tracked .gitignore ignores, \
                     so git add . would embed it",
                    n.kind.noun(),
                    n.path
                ),
                next_step: format!(
                    "add the line {} to .gitignore (codeflow init --workspace adds one for \
                     every nested repository)",
                    n.ignore_line()
                ),
            }),
            IgnoreState::LocalOnly(source) => out.push(Finding {
                severity: Severity::Warn,
                rule: NESTED_RULE,
                message: format!(
                    "{repo_label} contains the {} '{}', which only {source} ignores; other \
                     clones do not share that file",
                    n.kind.noun(),
                    n.path
                ),
                next_step: format!("add the line {} to the tracked .gitignore", n.ignore_line()),
            }),
        }
    }
    for path in gitlinks {
        out.push(Finding {
            severity: Severity::Warn,
            rule: NESTED_RULE,
            message: format!(
                "{repo_label} tracks '{path}' as a gitlink that .gitmodules does not register"
            ),
            next_step: format!(
                "register it with git submodule add, or stop tracking it with git rm --cached \
                 {path} and add /{path}/ to .gitignore"
            ),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Worktree locations
// ---------------------------------------------------------------------------

/// Expand one `git.worktree_locations` entry to an absolute folder. A
/// relative entry is under the root checkout; `~/` is the home folder;
/// `$CODEX_HOME` and `$GROK_HOME` fall back to `~/.codex` and `~/.grok`.
/// `None` when the entry names a folder this machine cannot resolve.
#[must_use]
pub fn expand_location(entry: &str, root: &Path, env: EnvLookup<'_>) -> Option<PathBuf> {
    let home = || env("HOME").map(PathBuf::from);
    let under =
        |base: Option<PathBuf>, rest: &str| base.map(|b| b.join(rest.trim_start_matches('/')));
    if let Some(rest) = entry.strip_prefix("$CODEX_HOME") {
        let base = env("CODEX_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".codex")));
        return under(base, rest);
    }
    if let Some(rest) = entry.strip_prefix("$GROK_HOME") {
        let base = env("GROK_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".grok")));
        return under(base, rest);
    }
    if let Some(rest) = entry.strip_prefix("~/") {
        return under(home(), rest);
    }
    let path = Path::new(entry);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

/// Whether a `git.worktree_locations` entry has a form this module expands.
#[must_use]
pub fn is_valid_location(entry: &str) -> bool {
    if entry.trim().is_empty() {
        return false;
    }
    match entry.strip_prefix('$') {
        Some(rest) => {
            let var = rest.split('/').next().unwrap_or_default();
            var == "CODEX_HOME" || var == "GROK_HOME"
        }
        None => true,
    }
}

fn canonical(path: &Path) -> PathBuf {
    crate::portable_path::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Doctor's findings for linked worktrees: one outside every location, and
/// one inside the working tree that nothing ignores.
#[must_use]
pub fn worktree_findings(
    repo: &git2::Repository,
    policy: &GitPolicy,
    env: EnvLookup<'_>,
) -> Vec<Finding> {
    let Some(root) = repo.workdir().map(canonical) else {
        return Vec::new();
    };
    let repo_label = label(&root);
    let locations: Vec<PathBuf> = policy
        .worktree_locations
        .iter()
        .filter_map(|e| expand_location(e, &root, env))
        .map(|p| canonical(&p))
        .collect();
    let mut out = Vec::new();
    for worktree in crate::git::linked_worktrees(repo) {
        let path = canonical(&worktree.path);
        if !path.exists() {
            continue;
        }
        if !locations.iter().any(|loc| path.starts_with(loc)) {
            out.push(Finding {
                severity: Severity::Warn,
                rule: LOCATIONS_KEY,
                message: format!(
                    "linked worktree {} of {repo_label} is outside every {LOCATIONS_KEY} entry",
                    path.display()
                ),
                next_step: format!(
                    "move it with git worktree move {} .worktrees/<slug>, or add its folder \
                     to {LOCATIONS_KEY} in .codeflow/policy.json",
                    path.display()
                ),
            });
            continue;
        }
        if let Ok(rel) = path.strip_prefix(&root) {
            // OS text rule (issue 79): a folder name that is not valid UTF-8
            // cannot be asked about, so no rule is known to cover it and the
            // warning stays (it is shown with escapes).
            // The key swaps platform separators only (a backslash is a name
            // character on Unix), then a name that is not text is escaped.
            let key = crate::portable_path::slashed(rel);
            let ignored = crate::git::key_is_text(&key)
                && ignore_sources(&root, std::slice::from_ref(&key))
                    .into_iter()
                    .next()
                    .flatten()
                    .is_some();
            let rel = crate::git::display_key(&key);
            if !ignored {
                out.push(Finding {
                    severity: Severity::Warn,
                    rule: LOCATIONS_KEY,
                    message: format!(
                        "linked worktree '{rel}' sits inside the working tree of {repo_label} \
                         and no ignore rule covers it"
                    ),
                    next_step: format!(
                        "ignore its folder in .gitignore, as the managed block does for \
                         .worktrees/ (for example /{rel}/)"
                    ),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Doctor's root checkout report
// ---------------------------------------------------------------------------

/// Tracked files with uncommitted changes (staged or not) in the working
/// tree, relative paths as exact names (OS text rule, issue 79: a name that
/// is not valid UTF-8 is still work to keep, never dropped).
///
/// # Errors
/// Returns git's error when the status cannot be read, so a caller that
/// decides on it can refuse.
pub fn tracked_changes(repo: &git2::Repository) -> Result<Vec<GitName>, git2::Error> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(statuses
        .iter()
        .filter(|s| !s.status().is_empty() && !s.status().contains(git2::Status::IGNORED))
        .map(|s| GitName::from_bytes(s.path_bytes()))
        .collect())
}

/// Commits on the current branch that its upstream lacks, when it has one.
fn ahead_of_upstream(repo: &git2::Repository, branch: &GitName) -> Option<usize> {
    // Advice only: a name that is not valid UTF-8 has no upstream to count.
    let local = repo
        .find_branch(branch.rule_text().ok()?, git2::BranchType::Local)
        .ok()?;
    let upstream = local.upstream().ok()?;
    let (ahead, _) = repo
        .graph_ahead_behind(local.get().target()?, upstream.get().target()?)
        .ok()?;
    Some(ahead)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Doctor's `repo-integrity` lines for the root checkout: one line per
/// finding of [`doctor_report`], and whether any of them warns.
#[must_use]
pub fn doctor_lines(root: &Path, policy: &GitPolicy, env: EnvLookup<'_>) -> (Vec<String>, bool) {
    let findings = doctor_report(root, policy, env);
    let warns = findings.iter().any(|f| f.severity == Severity::Warn);
    (findings.iter().map(ToString::to_string).collect(), warns)
}

/// Everything doctor reports about the root checkout of the repository that
/// holds `root` (from a linked worktree too) and, in an umbrella, its
/// nested repositories and linked worktrees. The first line is always the
/// root branch (info); the rest are findings.
#[must_use]
pub fn doctor_report(root: &Path, policy: &GitPolicy, env: EnvLookup<'_>) -> Vec<Finding> {
    let Some(repo) = main_checkout(root) else {
        return Vec::new();
    };
    let Some(workdir) = repo.workdir().map(canonical) else {
        return Vec::new();
    };
    let repo_label = label(&workdir);
    let root_branch = root_branch(&repo, policy);
    let nested = nested_repositories(&repo);
    let submodules = submodule_paths(&repo);
    let gitlinks = stray_gitlinks(&repo, &submodules);
    let workspace = !policy.root_branch.is_empty() && !nested.is_empty();

    let mut out = vec![Finding {
        severity: Severity::Info,
        rule: ROOT_BRANCH_KEY,
        message: if workspace {
            format!(
                "workspace mode: the root checkout of {repo_label} stays on '{}' ({}); it holds \
                 {}",
                root_branch.name,
                root_branch.source,
                nested_summary(&nested)
            )
        } else {
            format!(
                "the root checkout of {repo_label} stays on '{}' ({})",
                root_branch.name, root_branch.source
            )
        },
        next_step: String::new(),
    }];

    out.extend(missing_branch_finding(&repo, &root_branch, &repo_label));
    out.extend(head_finding(&repo, policy, &root_branch, &repo_label));

    if policy.root_branch.is_empty() && !nested.is_empty() {
        out.push(workspace_hint_finding(&repo_label, &nested));
    }
    out.extend(nested_findings(&repo_label, &nested, &gitlinks));
    out.extend(worktree_findings(&repo, policy, env));
    out
}

/// The root checkout of the repository that holds `path`: the repository
/// itself, or the main working tree when `path` is in a linked worktree.
/// `None` for a bare repository or outside one.
fn main_checkout(path: &Path) -> Option<git2::Repository> {
    let repo = git2::Repository::discover(path).ok()?;
    if is_root_checkout(&repo) {
        return Some(repo);
    }
    if repo.is_worktree() {
        let main = git2::Repository::open(repo.commondir()).ok()?;
        return (is_root_checkout(&main) && main.workdir().is_some()).then_some(main);
    }
    None
}

/// The warning for a root branch the policy names but the repository lacks.
fn missing_branch_finding(
    repo: &git2::Repository,
    root_branch: &RootBranch,
    repo_label: &str,
) -> Option<Finding> {
    if root_branch.source != RootBranchSource::Policy
        || repo
            .find_branch(
                root_branch.name.rule_text().unwrap_or_default(),
                git2::BranchType::Local,
            )
            .is_ok()
    {
        return None;
    }
    Some(Finding {
        severity: Severity::Warn,
        rule: ROOT_BRANCH_KEY,
        message: format!(
            "{ROOT_BRANCH_KEY} names '{}', but {repo_label} has no local branch '{}'",
            root_branch.name, root_branch.name
        ),
        next_step: format!(
            "create it from the default branch (codeflow init --workspace does this for an \
             umbrella), or correct {ROOT_BRANCH_KEY} in .codeflow/policy.json"
        ),
    })
}

/// The finding for the root checkout's HEAD: off its root branch or
/// detached, or on a protected root branch while holding task edits.
fn head_finding(
    repo: &git2::Repository,
    policy: &GitPolicy,
    root_branch: &RootBranch,
    repo_label: &str,
) -> Option<Finding> {
    // Advice only: a status that cannot be read adds no change to the report.
    let changes = tracked_changes(repo).unwrap_or_default();
    match head(repo)? {
        Head::Branch(ref b) if *b == root_branch.name => {
            (policy.branch_is_protected_name(b) && !changes.is_empty()).then(|| Finding {
                severity: Severity::Warn,
                rule: ROOT_BRANCH_KEY,
                message: format!(
                    "the root checkout of {repo_label} is on its protected root branch '{b}' \
                         and holds uncommitted changes to {}; the root takes no task work",
                    plural(changes.len(), "tracked file", "tracked files")
                ),
                next_step: format!(
                    "move them to a worktree: git stash, then {WORKTREE_ROUTE}, then git \
                         stash pop inside the worktree"
                ),
            })
        }
        found => {
            let mut state = Vec::new();
            if !changes.is_empty() {
                state.push(format!(
                    "uncommitted changes to {}",
                    plural(changes.len(), "tracked file", "tracked files")
                ));
            }
            if let Head::Branch(b) = &found {
                if let Some(n) = ahead_of_upstream(repo, b).filter(|n| *n > 0) {
                    state.push(format!(
                        "{} its upstream lacks",
                        plural(n, "commit", "commits")
                    ));
                }
            }
            let (state, save_step) = if state.is_empty() {
                (String::new(), "")
            } else {
                (
                    format!(", with {}", state.join(" and ")),
                    "commit or stash that work, ",
                )
            };
            let continue_step = match &found {
                Head::Branch(b) => {
                    let pop = if save_step.is_empty() {
                        ""
                    } else {
                        ", and git stash pop inside it if you stashed"
                    };
                    format!(
                        ", then git worktree add .worktrees/<slug> {b} to continue it in a \
                         worktree{pop}"
                    )
                }
                Head::Detached(_) => String::new(),
            };
            Some(Finding {
                severity: Severity::Warn,
                rule: ROOT_BRANCH_KEY,
                message: format!(
                    "the root checkout of {repo_label} is on {}{state}; its root branch is '{}' \
                     ({})",
                    found.describe(),
                    root_branch.name,
                    root_branch.source
                ),
                next_step: format!(
                    "{save_step}run git switch {} at the root{continue_step}",
                    root_branch.name
                ),
            })
        }
    }
}

fn nested_summary(nested: &[NestedRepo]) -> String {
    let projects = nested
        .iter()
        .filter(|n| n.kind == NestedKind::CodeflowProject)
        .count();
    format!(
        "{} ({} with CodeFlow)",
        plural(nested.len(), "nested repository", "nested repositories"),
        projects
    )
}

fn nested_list(nested: &[NestedRepo]) -> String {
    nested
        .iter()
        .map(|n| n.path.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The recommendation for a repository that holds nested repositories but
/// names no root branch.
fn workspace_hint_finding(repo_label: &str, nested: &[NestedRepo]) -> Finding {
    Finding {
        severity: Severity::Info,
        rule: ROOT_BRANCH_KEY,
        message: format!(
            "{repo_label} contains {} ({}) and sets no {ROOT_BRANCH_KEY}, so it looks like a \
             workspace; nothing was switched",
            nested_summary(nested),
            nested_list(nested)
        ),
        next_step: format!(
            "for an umbrella workspace, run codeflow init --workspace (root branch \
             {WORKSPACE_ROOT_BRANCH}); a single project can ignore this"
        ),
    }
}

/// The hint plain `init` and `update` print in a folder that holds nested
/// repositories and names no root branch. They switch nothing.
#[must_use]
pub fn workspace_hint(root: &Path, policy: &GitPolicy) -> Option<Finding> {
    if !policy.root_branch.is_empty() {
        return None;
    }
    let repo = git2::Repository::open(root).ok()?;
    if !is_root_checkout(&repo) {
        return None;
    }
    let nested = nested_repositories(&repo);
    if nested.is_empty() {
        return None;
    }
    Some(workspace_hint_finding(
        &label(&canonical(repo.workdir()?)),
        &nested,
    ))
}

// ---------------------------------------------------------------------------
// codeflow init --workspace
// ---------------------------------------------------------------------------

/// Why `init --workspace` stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceError(pub Finding);

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for WorkspaceError {}

const INIT_RULE: &str = "codeflow init --workspace";

fn stop(message: String, next_step: String) -> WorkspaceError {
    WorkspaceError(Finding {
        severity: Severity::Warn,
        rule: INIT_RULE,
        message,
        next_step,
    })
}

fn git_run(root: &Path, args: &[&str]) -> Result<(), String> {
    let out = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// What the branch step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchStep {
    /// The root was already on the root branch.
    AlreadyOn(String),
    /// The branch existed and the root switched to it.
    Reused(String),
    /// The branch was created from the named default branch.
    Created { branch: String, from: String },
}

impl BranchStep {
    /// The root branch the step left the root checkout on.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::AlreadyOn(b) | Self::Reused(b) => b,
            Self::Created { branch, .. } => branch,
        }
    }
}

/// Put the root checkout at `root` on the workspace root branch: the
/// configured `git.root_branch`, else [`WORKSPACE_ROOT_BRANCH`]. Creates it
/// from the default branch when missing. Never switches over uncommitted
/// changes to tracked files.
///
/// # Errors
///
/// A [`WorkspaceError`] naming the reason and the next step: `root` is not
/// a repository's root checkout, it has no commit yet, tracked files have
/// uncommitted changes, or git refused the switch.
pub fn prepare_branch(root: &Path, policy: &GitPolicy) -> Result<BranchStep, WorkspaceError> {
    let repo = git2::Repository::open(root).map_err(|_| {
        stop(
            format!("{} is not the top of a git repository", root.display()),
            "run codeflow init --workspace at the umbrella repository's root checkout".to_string(),
        )
    })?;
    if !is_root_checkout(&repo) {
        return Err(stop(
            format!(
                "{} is a linked worktree, not the root checkout",
                root.display()
            ),
            "run codeflow init --workspace at the umbrella repository's root checkout".to_string(),
        ));
    }
    let repo_label = label(&canonical(root));
    let target = workspace_branch_name(policy);
    let current = head(&repo);
    if current == Some(Head::Branch(GitName::from_text(&target))) {
        return Ok(BranchStep::AlreadyOn(target));
    }
    if repo.head().is_err() {
        return Err(stop(
            format!("{repo_label} has no commit yet, so there is no default branch to create '{target}' from"),
            "make the first commit on the default branch, then rerun codeflow init --workspace".to_string(),
        ));
    }
    let changes = tracked_changes(&repo).map_err(|error| {
        stop(
            format!(
                "codeflow init --workspace cannot read the status of {repo_label} ({error}), so it cannot tell whether switching to '{target}' keeps uncommitted work"
            ),
            "fix the repository state, then rerun codeflow init --workspace".to_string(),
        )
    })?;
    if !changes.is_empty() {
        let found = current.map_or_else(|| "its current HEAD".to_string(), |h| h.describe());
        return Err(stop(
            format!(
                "codeflow init --workspace would switch the root checkout of {repo_label} from {found} to \
                 '{target}', but these tracked files have uncommitted changes: {}",
                changes
                    .iter()
                    .map(|name| name.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "commit or stash them, then rerun codeflow init --workspace".to_string(),
        ));
    }
    let exists = repo.find_branch(&target, git2::BranchType::Local).is_ok();
    let step = if exists {
        git_run(root, &["switch", "--quiet", &target])
            .map_err(|e| switch_failed(&repo_label, &target, &e))?;
        BranchStep::Reused(target)
    } else {
        let from = default_branch(&repo, policy).name;
        // OS text rule (issue 79): git cannot be given a default branch whose
        // name is not valid UTF-8 as text, so the new branch cannot start from
        // it. Say so instead of passing git a name that resolves to nothing.
        let Ok(from_text) = from.rule_text() else {
            return Err(stop(
                format!(
                    "the default branch of {repo_label} is not valid UTF-8, so the new branch \
                     cannot start from it"
                ),
                "give the root checkout a default branch whose name is valid UTF-8".to_string(),
            ));
        };
        let start = if repo.find_branch(from_text, git2::BranchType::Local).is_ok() {
            from_text.to_string()
        } else {
            format!("origin/{from_text}")
        };
        git_run(
            root,
            &["switch", "--quiet", "--no-track", "-c", &target, &start],
        )
        .map_err(|e| switch_failed(&repo_label, &target, &e))?;
        BranchStep::Created {
            branch: target,
            from: from.to_string(),
        }
    };
    Ok(step)
}

fn switch_failed(repo_label: &str, target: &str, reason: &str) -> WorkspaceError {
    stop(
        format!("git could not put the root checkout of {repo_label} on '{target}': {reason}"),
        "resolve the reason above, then rerun codeflow init --workspace".to_string(),
    )
}

/// The branch `init --workspace` puts the root on.
#[must_use]
pub fn workspace_branch_name(policy: &GitPolicy) -> String {
    let configured = policy.root_branch.as_str();
    if configured.is_empty() {
        WORKSPACE_ROOT_BRANCH.to_string()
    } else {
        configured.to_string()
    }
}

/// Write `git.root_branch` into the policy text, keeping its layout, and
/// adding the `git` object when the policy has none. `Ok(None)` when the
/// key already holds `branch`; an error names why the text cannot take the
/// key. The edit is kept only when it parses to the intended policy.
///
/// # Errors
///
/// When the text is not a JSON object, its `git` member is not an object,
/// or the edit cannot be made in place.
pub fn policy_with_root_branch(text: &str, branch: &str) -> Result<Option<String>, String> {
    use crate::scaffold::json_edit;
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("it is not valid JSON ({e})"))?;
    let value = serde_json::Value::String(branch.to_string());
    let json = value.to_string();
    let top = expected
        .as_object_mut()
        .ok_or("its top level is not a JSON object")?;
    let edited = match top.get_mut("git") {
        None => {
            top.insert(
                "git".to_string(),
                serde_json::json!({ "root_branch": branch }),
            );
            json_edit::insert_member(text, &[], "git", &format!("{{\"root_branch\": {json}}}"))
        }
        Some(serde_json::Value::Object(git)) => {
            let edit = match git.get("root_branch") {
                Some(current) if *current == value => return Ok(None),
                Some(_) => json_edit::replace_value(text, &["git", "root_branch"], &json),
                None => json_edit::insert_member(text, &["git"], "root_branch", &json),
            };
            git.insert("root_branch".to_string(), value);
            edit
        }
        Some(_) => return Err("its git member is not a JSON object".to_string()),
    };
    edited
        .and_then(|e| json_edit::verified(e, &expected))
        .map(Some)
        .ok_or_else(|| "the key could not be written in place".to_string())
}

/// Whether a rule other clones share keeps `n` out of the index: a tracked
/// `.gitignore`, or the root `.gitignore` this run writes for the user to
/// commit.
fn shared_ignore(n: &NestedRepo) -> bool {
    match &n.ignore {
        IgnoreState::Tracked => true,
        IgnoreState::LocalOnly(source) => source == ".gitignore",
        IgnoreState::NotIgnored => false,
    }
}

/// The `.gitignore` text with a line for each of `nested`, under one
/// comment. `None` when `nested` is empty.
#[must_use]
pub fn gitignore_with_nested(text: &str, nested: &[NestedRepo]) -> Option<String> {
    if nested.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(
        "# nested repositories, each its own git repository (codeflow init --workspace)\n",
    );
    for n in nested {
        out.push_str(&n.ignore_line());
        out.push('\n');
    }
    Some(out)
}

/// What `init --workspace` did, for its report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReport {
    pub branch: BranchStep,
    pub policy_changed: bool,
    pub ignored: Vec<NestedRepo>,
    pub already_ignored: Vec<NestedRepo>,
}

/// Finish `init --workspace` after the scaffold exists: set
/// `git.root_branch` and ignore every nested repository.
///
/// # Errors
///
/// A [`WorkspaceError`] when the policy or `.gitignore` cannot be read or
/// written.
pub fn finish(root: &Path, branch: BranchStep) -> Result<WorkspaceReport, WorkspaceError> {
    let name = branch.name().to_string();
    let policy_path = root.join(".codeflow").join("policy.json");
    let io = |what: &str, e: std::io::Error| {
        stop(
            format!("codeflow init --workspace could not {what}: {e}"),
            "fix the file's permissions, then rerun codeflow init --workspace".to_string(),
        )
    };
    let text =
        std::fs::read_to_string(&policy_path).map_err(|e| io("read .codeflow/policy.json", e))?;
    let edit = policy_with_root_branch(&text, &name).map_err(|why| {
        stop(
            format!(
                "codeflow init --workspace could not set {ROOT_BRANCH_KEY} in \
                 .codeflow/policy.json: {why}"
            ),
            format!(
                "set \"git\": {{ \"root_branch\": \"{name}\" }} in .codeflow/policy.json by \
                 hand, then rerun codeflow init --workspace"
            ),
        )
    })?;
    let policy_changed = match edit {
        Some(updated) => {
            std::fs::write(&policy_path, updated)
                .map_err(|e| io("write .codeflow/policy.json", e))?;
            true
        }
        None => false,
    };
    let repo = git2::Repository::open(root).map_err(|e| {
        stop(
            format!(
                "{} is no longer readable as a repository: {e}",
                root.display()
            ),
            "rerun codeflow init --workspace".to_string(),
        )
    })?;
    let (already_ignored, ignored): (Vec<_>, Vec<_>) = nested_repositories(&repo)
        .into_iter()
        .partition(shared_ignore);
    let gitignore = root.join(".gitignore");
    let current = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if let Some(updated) = gitignore_with_nested(&current, &ignored) {
        std::fs::write(&gitignore, updated).map_err(|e| io("write .gitignore", e))?;
        verify_ignored(root, &ignored)?;
    }
    Ok(WorkspaceReport {
        branch,
        policy_changed,
        ignored,
        already_ignored,
    })
}

/// Check through git that the root `.gitignore` now ignores each of
/// `nested`: a later rule, such as a negation in a deeper `.gitignore`, can
/// still un-ignore one.
fn verify_ignored(root: &Path, nested: &[NestedRepo]) -> Result<(), WorkspaceError> {
    let paths: Vec<String> = nested.iter().map(|n| n.path.clone()).collect();
    let still: Vec<&str> = paths
        .iter()
        .zip(tree_ignore_sources(root, &paths))
        .filter(|(_, source)| source.as_deref() != Some(".gitignore"))
        .map(|(path, _)| path.as_str())
        .collect();
    if still.is_empty() {
        return Ok(());
    }
    Err(stop(
        format!(
            "codeflow init --workspace added the nested repositories to .gitignore, but git \
             still does not ignore {} through it",
            still.join(", ")
        ),
        "find the rule that wins with `git check-ignore -v --no-index <path>`, remove it, then \
         rerun codeflow init --workspace"
            .to_string(),
    ))
}

impl fmt::Display for WorkspaceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "workspace mode:")?;
        match &self.branch {
            BranchStep::AlreadyOn(b) => writeln!(f, "  root checkout already on '{b}'")?,
            BranchStep::Reused(b) => writeln!(f, "  root checkout switched to the existing '{b}'")?,
            BranchStep::Created { branch, from } => {
                writeln!(
                    f,
                    "  created '{branch}' from '{from}' and switched the root checkout to it"
                )?;
            }
        }
        if self.policy_changed {
            writeln!(f, "  set {ROOT_BRANCH_KEY} in .codeflow/policy.json")?;
        } else {
            writeln!(f, "  {ROOT_BRANCH_KEY} already set")?;
        }
        for n in &self.ignored {
            writeln!(
                f,
                "  ignored the {} {} in .gitignore",
                n.kind.noun(),
                n.ignore_line()
            )?;
        }
        for n in &self.already_ignored {
            writeln!(
                f,
                "  the {} '{}' is already in .gitignore",
                n.kind.noun(),
                n.path
            )?;
        }
        writeln!(f, "next steps:")?;
        writeln!(
            f,
            "  - commit the files init wrote, including .gitignore and .codeflow/policy.json, on the \
             root branch"
        )?;
        writeln!(
            f,
            "  - bind the nested repositories with the nested-repository inventory once the \
             harness permissions work ships it"
        )?;
        writeln!(
            f,
            "  - main is the milestone checkpoint: at a milestone the operator moves it \
             forward with codeflow integrate {} --into main; agents never do",
            self.branch.name()
        )?;
        write!(
            f,
            "  - do larger or parallel work in a worktree: {WORKTREE_ROUTE}"
        )
    }
}

#[cfg(test)]
#[path = "root_checkout_tests.rs"]
mod tests;
