//! The step that clears a warning or a note (SPC-013 R-80, TSK-147).
//!
//! Every warning and note `CodeFlow` prints names an existing command, or an
//! edit to a named file, that clears it. A message carries a [`Remedy`],
//! and a [`Remedy`] is only made from an entry of the [`CATALOG`], whose
//! step the inventory test checks: a `codeflow` command must exist in
//! `codeflow --help`, a `git` command in `git help -a`, and an edit must
//! name a file path.

use std::fmt;
use std::ops::Deref;

/// What a remedy asks the reader to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Run a `codeflow` command, written from `codeflow` to its last
    /// subcommand, such as `codeflow task status`.
    Codeflow(&'static str),
    /// Run a `git` command, written `git <subcommand>`.
    Git(&'static str),
    /// Edit a named file, as a path from the project root. `{path}` stands
    /// for the file the message itself names.
    Edit(&'static str),
    /// Confirm by hand, only in a doctor note about a state doctor cannot
    /// read; its text says doctor cannot verify it (TSK-147 AC-1).
    Manual(&'static str),
}

impl Step {
    /// The words the remedy text must contain to name this step.
    #[must_use]
    pub fn named(&self) -> &'static str {
        match self {
            Self::Codeflow(command) | Self::Git(command) => command,
            Self::Edit(path) => path,
            Self::Manual(step) => step,
        }
    }
}

/// One catalogued way to clear a finding.
#[derive(Debug)]
pub struct Clearing {
    /// The catalogue name, for the inventory.
    pub name: &'static str,
    /// The step the text names.
    pub step: Step,
    /// The remedy text; `{key}` placeholders are filled by [`Clearing::with`].
    pub text: &'static str,
}

impl Clearing {
    /// The remedy, for a text without placeholders.
    #[must_use]
    pub fn remedy(&'static self) -> Remedy {
        debug_assert!(
            !self.text.contains('{'),
            "{} needs its placeholders",
            self.name
        );
        Remedy(self.text.to_string())
    }

    /// The remedy with each `{key}` replaced by its value.
    #[must_use]
    pub fn with(&'static self, fills: &[(&str, &str)]) -> Remedy {
        let mut text = self.text.to_string();
        for (key, value) in fills {
            text = text.replace(&format!("{{{key}}}"), value);
        }
        debug_assert!(
            !text.contains('{'),
            "{} left a placeholder: {text}",
            self.name
        );
        Remedy(text)
    }
}

/// A rendered remedy, made only from a [`Clearing`], or from the sanctioned
/// path of an always-blocking rule, which never prints as a warning.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Remedy(String);

impl Remedy {
    /// The sanctioned path of an always-blocking rule (R-80).
    pub(crate) fn sanctioned(text: &str) -> Self {
        Self(text.to_string())
    }
}

impl serde::Serialize for Remedy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl Deref for Remedy {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Remedy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A warning or note outside a policy violation, with the step that
/// clears it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// What was found.
    pub text: String,
    /// The step that clears it.
    pub remedy: Remedy,
}

impl Finding {
    /// A finding and its remedy.
    #[must_use]
    pub fn new(text: impl Into<String>, remedy: Remedy) -> Self {
        Self {
            text: text.into(),
            remedy,
        }
    }

    /// The same finding, its text prefixed with `prefix: `.
    #[must_use]
    pub fn prefixed(self, prefix: &str) -> Self {
        Self {
            text: format!("{prefix}: {}", self.text),
            remedy: self.remedy,
        }
    }

    /// The finding as printed by `plane` at `kind` (`warning`, `note` or
    /// `notice`), with the step that clears it on the next line.
    #[must_use]
    pub fn line(&self, plane: &str, kind: &str) -> String {
        // A finding's own text can span lines (a tool's error); they stay
        // indented under it, so the remedy line stays with its finding.
        format!(
            "{plane}: {kind}: {}\n  clear it: {}",
            self.text.trim_end().replace('\n', "\n  "),
            self.remedy
        )
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (clear it: {})", self.text, self.remedy)
    }
}

macro_rules! catalog {
    ($($(#[$doc:meta])* $name:ident = $step:expr, $text:expr;)*) => {
        $($(#[$doc])* pub static $name: Clearing = Clearing {
            name: stringify!($name),
            step: $step,
            text: $text,
        };)*
        /// Every catalogued clearing, for the inventory test.
        pub static CATALOG: &[&Clearing] = &[$(&$name),*];
    };
}

catalog! {
    /// Local changes or commits whose recovery has not been proved.
    DISCARD_LOCAL_WORK = Step::Git("git stash push"),
        "inspect `git status` and save tracked changes with `git stash push` or a commit; preview untracked removal with `git clean -nd`, keep or export unique files, and retain a branch or worktree until another live ref preserves its commits; restore a named file only when that is intended";

    // Records.

    /// A task whose written status disagrees with its branches.
    TASK_STATUS = Step::Codeflow("codeflow task status"),
        "run `codeflow task status {id} todo` (or the status it has reached), or push a branch `<prefix>/{id}-<slug>` that carries it";

    /// A planning epic with complete tasks.
    EPIC_STATUS = Step::Edit("{path}"),
        "set `status: in_progress` in {path}, or run `codeflow epic status {id} complete` once every task is done";
    /// A complete task whose acceptance cites a superseded spec.
    SUPERSEDED_CITATION = Step::Codeflow("codeflow task status"),
        "reopen it with `codeflow task status {id} todo --reason \"{spec} superseded\"`, recheck its acceptance against the successor, and complete it again";
    /// An approved spec that no task delivers.
    SPEC_NO_CONSUMER = Step::Codeflow("codeflow task new"),
        "plan a task that delivers it (`codeflow task new`, with {id} under `specs:`), or replace it with a new revision that lists `supersedes: [{id}]` and run `codeflow spec status {id} superseded --by <SPC-NNN>`";
    /// A spec whose written `implemented` disagrees with its consumers.
    SPEC_WRITTEN_IMPLEMENTED = Step::Edit("{path}"),
        "set `status: approved` in {path}; `implemented` is derived from the consumers and never written (R-32)";
    /// A clone without the work-records migration baseline.
    BASELINE_HISTORY = Step::Git("git fetch"),
        "fetch full history (`git fetch --unshallow`, or a CI checkout with fetch-depth 0) to enforce the rules in full";
    /// A record that still carries a second, historical id.
    DUAL_IDENTITY = Step::Edit("{path}"),
        "in {path}, set `id:` to the `format_id` value and delete the `format_id` line once nothing cites the old id; new records use one stable id";
    /// A standalone task delivered by several pull requests.
    STANDALONE_SPLIT = Step::Edit("{path}"),
        "give {path} an `epic_id`: work that takes several pull requests belongs to an epic (SPC-013 R-66)";
    /// A documentation layer the project does not have.
    DOCS_LAYER_ABSENT = Step::Codeflow("codeflow update"),
        "run `codeflow update`, which restores {path} at this project's tier";
    /// References into an absent epics layer.
    DOCS_EPICS_UNCHECKED = Step::Codeflow("codeflow epic new"),
        "create each epic named with `codeflow epic new`, or remove the references to it";
    /// References into an absent specs layer.
    DOCS_SPECS_UNCHECKED = Step::Codeflow("codeflow spec new"),
        "create each spec named with `codeflow spec new --for <id>`, or remove the references to it";
    /// References into an absent decisions layer.
    DOCS_ADRS_UNCHECKED = Step::Codeflow("codeflow adr new"),
        "create each decision named with `codeflow adr new`, or remove the references to it";
    /// References into an absent capability registry.
    DOCS_CAPABILITIES_UNCHECKED = Step::Edit("docs/capabilities.md"),
        "add each capability named to docs/capabilities.md, or remove the references to it";
    /// A task branch whose planning is not anchored on the target.
    WORK_START_RECONCILE = Step::Codeflow("codeflow work start"),
        "reconcile the target branch, then run `codeflow work start {id}`";
    /// A task whose planning record has not reached the target.
    WORK_START_MERGE_PLANNING = Step::Codeflow("codeflow work start"),
        "merge the validated planning record into '{target}', then run `codeflow work start {id}`";
    /// A task branch without a visible task record.
    TASK_RECORD_MISSING = Step::Codeflow("codeflow task new"),
        "create the durable task record with `codeflow task new` on a planning branch and merge it into the target before implementation";
    /// A workgraph that does not validate.
    WORKGRAPH_INVALID = Step::Codeflow("codeflow validate"),
        "repair the workgraph until `codeflow validate --docs` passes";
    /// A record added by hand, outside the id registry.
    ID_REGISTRY = Step::Codeflow("codeflow ids admit"),
        "issue ids with `codeflow task|epic|spec new`; a maintainer admits a hand-written record with `codeflow ids admit`";
    /// Records added in a checkout that has not fetched the id registry.
    ID_REGISTRY_UNFETCHED = Step::Git("git fetch"),
        "fetch the registry with its history (`git fetch origin codeflow/registry:refs/remotes/origin/codeflow/registry`), then rerun `codeflow ci`; a maintainer seeds a missing one once with `codeflow ids seed`";
    /// A record whose id the registry gives another record.
    ID_REGISTRY_RETARGET = Step::Codeflow("codeflow ids retarget"),
        "renumber this branch's record with `codeflow ids retarget <id>`, which moves it to a free number, then rerun `codeflow ci`";
    /// A record whose written uid the range changes.
    ID_REGISTRY_UID = Step::Git("git restore"),
        "restore the record's `uid` line as the target has it (`git restore --source <target> -- <path>` restores the whole record), since a uid is written once";
    /// An acceptance block bound to a commit other than the reviewed head.
    ACCEPTANCE_BINDING = Step::Codeflow("codeflow task status"),
        "review the pull request head, then record it: reopen the task (`codeflow task status <id> todo --reason \"review the head\"`) and complete it with the new review (`codeflow task status <id> complete --acceptance <file>`); a waiver names the planning amendment commit on the target ({note})";
    /// A release-line legacy criteria change, landed before the release
    /// rule and covered by its line's cutoff (SPC-013 R-120).
    RELEASE_LEGACY_CHANGE = Step::Edit(".codeflow/project.toml"),
        "an operator confirms the change against the review recorded for its landing, which the release report names; the cutoff in `release_rule_baseline` in .codeflow/project.toml is the 2.x to 3.0 transition record and is never edited, so the notice ends when the release lands";
    /// A task without a journey criterion for an adopter-facing range.
    JOURNEY_CRITERION = Step::Edit("{path}"),
        "add a `(journey)` criterion to {path} by a planning pull request, or serve the epic's journey criterion there with `(serves EPC-NNN AC-n)`";
    /// An older record that breaks a rule its baseline exempts.
    RECORD_BASELINE_EXEMPT = Step::Edit("{path}"),
        "an older record keeps its baseline exemption, so this only warns; fix it in {path}, since its next status change applies the rules in full";

    /// A range that introduces or edits the work-records baseline list.
    BASELINE_REVIEW = Step::Edit(".codeflow/project.toml"),
        "the reviewer approves each record listed; the notice ends once this change lands on its target, and removing an entry from `work_records_baseline` in .codeflow/project.toml applies the rules to that record now";
    /// A deprecated policy key.
    POLICY_DEPRECATED = Step::Codeflow("codeflow update"),
        "`codeflow update` removes it, or delete {key} from .codeflow/policy.json";
    /// A local target branch behind its upstream.
    TARGET_BEHIND_UPSTREAM = Step::Git("git fetch"),
        "fast-forward '{local}' with `git fetch . {upstream}:{local}` (or `git merge --ff-only {upstream}` while on it)";

    // Protected branches and the sanctioned landing path.

    /// Work that would land on a protected branch outside a pull request.
    PROTECTED_BRANCH = Step::Codeflow("codeflow integrate"),
        "land work via PR (gh pr create → merge on evidenced-green checks) or `codeflow integrate <branch> --into <target>`";
    /// Deleting a protected branch.
    PROTECTED_DELETE = Step::Edit(".codeflow/policy.json"),
        "protected branches are never deleted; if it is truly intended, first remove the entry from git.protected_branches in `.codeflow/policy.json`";
    /// Rewriting protected history.
    PROTECTED_REWRITE = Step::Git("git revert"),
        "undo it with `git revert` instead, and rebase feature branches in their own worktree; protected history is append-only";
    /// A remote-tracking ref set by hand.
    REMOTE_TRACKING_REF = Step::Git("git fetch"),
        "let a real `git fetch` or `git pull` update refs/remotes; never set it by hand";
    /// A force push to an unprotected branch the project restricts.
    FORCE_PUSH = Step::Git("git pull"),
        "push without force after `git pull --rebase`; the project restricts force-pushes with git.force_push_unprotected in `.codeflow/policy.json`";
    /// A `gh pr merge` into a protected base.
    PR_MERGE_PROTECTED = Step::Edit(".codeflow/policy.json"),
        "PR merges into protected branches are performed by a human (GitHub UI / their own terminal) or explicitly sanctioned by git.pr_merge_to_protected in `.codeflow/policy.json`";
    /// A branch name outside the prefixes.
    BRANCH_NAME = Step::Git("git branch"),
        "rename it with `git branch -m <prefix>/<kebab-name>`, with a sanctioned prefix: {prefixes}";
    /// A commit at the root checkout off its root branch (ADR-0074).
    ROOT_CHECKOUT_COMMIT = Step::Git("git switch"),
        "task work belongs in a linked worktree: put the root checkout back on its root branch with `git switch {root}`, then work and commit in a worktree (`git worktree add .worktrees/<slug> -b <branch>`, or `git worktree add .worktrees/<slug> <branch>` to continue an existing branch)";
    /// The hooks or their policy edited from a session.
    HOOK_INTEGRITY = Step::Codeflow("codeflow update"),
        "the enforcement hooks and their policy are not agent-editable: fix the cause a gate flags rather than disabling it; hooks and policy change through a human or `codeflow update` (ADR-0009)";

    // Commit messages, reworded with `git commit --amend`.

    /// A subject that is not `type(scope): description`.
    COMMIT_TYPE = Step::Git("git commit"),
        "reword it with `git commit --amend` as `type(scope): description`, with type one of: {types}";
    /// A subject over its length limits.
    COMMIT_LENGTH = Step::Git("git commit"),
        "reword it with `git commit --amend`: keep the description ≤ {description} chars and the whole subject line ≤ {subject} chars";
    /// No blank line after the subject.
    COMMIT_BLANK_LINE = Step::Git("git commit"),
        "reword it with `git commit --amend`: put one blank line between the subject and the body";
    /// A misspelt breaking-change marker.
    COMMIT_BREAKING_FOOTER = Step::Git("git commit"),
        "reword it with `git commit --amend`: signal a breaking change with `type!: description` or the exact footer `BREAKING CHANGE:` (uppercase)";
    /// A body outside the bullet form.
    COMMIT_BODY = Step::Git("git commit"),
        "reword it with `git commit --amend`: the body is `- ` bullets (max {bullets}, each ≤ {length} chars) and an optional `BREAKING CHANGE:` footer; blank lines are fine, prose paragraphs are not. Other trailers are allowed only when opted in via git.commit_footer_tokens";
    /// A required footer missing.
    COMMIT_REQUIRED_FOOTERS = Step::Git("git commit"),
        "reword it with `git commit --amend`: every commit must carry these footer trailers: {footers}";
    /// No ticket reference.
    COMMIT_TICKET = Step::Git("git commit"),
        "reword it with `git commit --amend` and add a ticket-reference footer trailer (key one of: {keys})";
    /// AI attribution in a commit message.
    COMMIT_AI_ATTRIBUTION = Step::Git("git commit"),
        "reword it with `git commit --amend` without it; project policy forbids AI attribution in commits and PR bodies (charter §6.4)";
    /// Emoji in a commit subject.
    COMMIT_EMOJI = Step::Git("git commit"),
        "reword it with `git commit --amend` without emoji in the subject (charter §6.4)";
    /// A policy character in a commit message.
    COMMIT_POLICY_CHARACTER = Step::Git("git commit"),
        "reword it with `git commit --amend`: use a comma, colon, semicolon, parentheses, or a full stop and a new sentence; a hyphen (-) inside a compound word; \"to\" in a range (ADR-0067)";
    /// A policy character on an added line of a file.
    FILE_POLICY_CHARACTER = Step::Edit("{path}"),
        "edit {path}: use a comma, colon, semicolon, parentheses, or a full stop and a new sentence; a hyphen (-) inside a compound word; \"to\" in a range (ADR-0067); existing lines are grandfathered, only this added line changes";

    /// An unresolved conflict marker on an added line of a file.
    CONFLICT_MARKER = Step::Edit("{path}"),
        "edit {path}: resolve the conflict and restage, or set conflict-marker-size for the path in .gitattributes to a length its markers do not have";

    /// A git without `check-attr --source`, which the conflict-marker row
    /// of `codeflow ci` needs.
    GIT_ATTR_SOURCE_UNSUPPORTED = Step::Codeflow("codeflow ci"),
        "install a Git release build of 2.40 or later, whose `git check-attr --source` the conflict-marker check needs, then rerun `codeflow ci`; fetching more history does not help";

    /// A commit on a declared contract surface.
    BREAKING_WATCH_PATH = Step::Codeflow("codeflow ci"),
        "if it is not breaking, state `Breaking: no` with a `Rationale` under Release impact in the pull request body, and `codeflow ci --pr-body-file <body.md>` reports nothing; if it is, mark the commit `type!:` with a `BREAKING CHANGE:` footer and the migration path";

    // Pull request bodies, checked by `codeflow ci`.

    /// A policy character in a PR body.
    PR_POLICY_CHARACTER = Step::Codeflow("codeflow ci"),
        "edit the pull request body: use a comma, colon, semicolon, parentheses, or a full stop and a new sentence; a hyphen (-) inside a compound word; \"to\" in a range (ADR-0067); `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// AI attribution in a PR body.
    PR_AI_ATTRIBUTION = Step::Codeflow("codeflow ci"),
        "remove the attribution from the pull request body (project policy forbids AI attribution in commits and PR bodies, charter §6.4); `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// Emoji in a PR body.
    PR_EMOJI = Step::Codeflow("codeflow ci"),
        "remove emoji from the pull request body (charter §6.4); `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A missing or empty PR body.
    PR_BODY_MISSING = Step::Codeflow("codeflow ci"),
        "write the pull request body from the PR template, then check it with `codeflow ci --pr-body-file <body.md>`";
    /// A required section heading twice.
    PR_SECTION_DUPLICATE = Step::Codeflow("codeflow ci"),
        "keep one authoritative section for each required heading in the pull request body; `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A required section without content.
    PR_SECTION_EMPTY = Step::Codeflow("codeflow ci"),
        "fill the section in (HTML comments and bare '-' bullets do not count as content); `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A required section missing.
    PR_SECTION_MISSING = Step::Codeflow("codeflow ci"),
        "add the section with real content (the shipped PR template carries the required structure); `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A PR template placeholder left in the body.
    PR_TEMPLATE_REMNANT = Step::Codeflow("codeflow ci"),
        "replace the placeholder with real content, or delete the line; `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A PR body over the presentation guidelines.
    PR_PRESENTATION = Step::Codeflow("codeflow ci"),
        "keep the body concise and link detailed evidence; retain necessary verification; `codeflow ci --pr-body-file <body.md>` checks the new text";
    /// A Release impact section that does not declare the release.
    PR_RELEASE_IMPACT = Step::Codeflow("codeflow ci"),
        "declare Impact, Breaking, Rationale and Migration under Release impact using the project's breaking level; `codeflow ci --pr-body-file <body.md>` checks the new text";

    // `codeflow ci` itself.

    /// A range whose base ref does not resolve.
    CI_BASE_UNRESOLVED = Step::Codeflow("codeflow ci"),
        "fetch the base branch (`git fetch`), or name the range: `codeflow ci --base <ref> --head <ref>`";
    /// A range git could not diff or list.
    CI_RANGE_UNREADABLE = Step::Git("git fetch"),
        "fetch the whole range (`git fetch --unshallow`, or a CI checkout with fetch-depth 0), then rerun `codeflow ci`";
    /// A run with no branch name to check.
    CI_BRANCH_UNRESOLVED = Step::Codeflow("codeflow ci"),
        "name the branch: `codeflow ci --branch <name>` (a detached checkout has none; CI platforms supply it)";
    /// A pull request whose body the platform did not supply.
    CI_BODY_UNSUPPLIED = Step::Codeflow("codeflow ci"),
        "give the body to the check: `codeflow ci --pr-body-file <body.md>`, or set CODEFLOW_PR_BODY in the pipeline";
    /// A codeflow build whose embedded scaffold manifest does not load.
    SCAFFOLD_MANIFEST_BROKEN = Step::Codeflow("codeflow doctor"),
        "this codeflow build is damaged: install a release build, then `codeflow doctor` reports its managed files again";

    // Secrets.

    /// A dotenv file staged.
    ENV_FILE_STAGED = Step::Edit(".gitignore"),
        "unstage it with `git restore --staged <file>` and keep env files out of git through `.gitignore`; commit a .env.example instead";
    /// A staged diff the scan could not read.
    SECRET_SCAN_INCOMPLETE = Step::Git("git commit"),
        "retry `git commit` once the staged diff can be scanned completely";
    /// A possible secret staged.
    SECRET_STAGED = Step::Git("git restore"),
        "remove the secret from the staged content, or unstage it with `git restore --staged <file>`; rotate it if it was ever committed";

    // The push set.

    /// A push-set target that failed.
    PUSH_SET_FAILED = Step::Codeflow("codeflow test"),
        "fix the failing target(s), or run `codeflow test --mode quick` to reproduce";
    /// A `codeflow ci` run in the push set that found problems.
    PUSH_SET_CI = Step::Codeflow("codeflow ci"),
        "fix the findings above, then push again; `codeflow ci {args}` reruns this check on the range as pushed";
    /// A `codeflow validate` run in the push set that found problems.
    PUSH_SET_VALIDATE = Step::Codeflow("codeflow validate"),
        "fix the findings above, then rerun `codeflow validate {args}`";
    /// A push set that could not start its checks.
    PUSH_SET_BY_HAND = Step::Codeflow("codeflow ci"),
        "run `codeflow ci` and `codeflow validate --docs` by hand";
    /// A pushed ref whose tree checks ran on another checkout.
    PUSH_TREE_UNCHECKED = Step::Codeflow("codeflow validate"),
        "push from a clean checkout of the pushed commit (commit or discard tracked edits first), or run `codeflow validate --docs` and `codeflow test --mode quick` on it by hand";
    /// A new branch whose `codeflow ci` range has no base.
    PUSH_RANGE_UNRESOLVED = Step::Git("git fetch"),
        "fetch the destination's protected branches (`git fetch <remote>`) so the range has a base, then push again";
    /// A destination that did not answer for its branches.
    PUSH_DESTINATION_SILENT = Step::Git("git ls-remote"),
        "make `git ls-remote <remote>` answer (network, credentials), then push again";
    /// A push that rewrites the destination's branch.
    PUSH_REWRITE = Step::Git("git pull"),
        "push without the rewrite (`git pull --rebase` onto the destination's branch first), or let CI check the whole range";
    /// A push set over its time budget, slowest at a test target.
    PUSH_OVER_BUDGET_TARGET = Step::Edit(".codeflow/test-config.json"),
        "remove the `modes.quick` of target '{target}' in .codeflow/test-config.json to move it to the full gate";
    /// A push set over its time budget, slowest at a built-in check.
    PUSH_OVER_BUDGET_BUILTIN = Step::Edit(".codeflow/policy.json"),
        "run the slowest step alone to see what makes it slow, or set `git.test_gate_on_push` to off in .codeflow/policy.json to drop the push set";
    /// A push set with no test configuration.
    PUSH_TARGETS_UNCONFIGURED = Step::Edit(".codeflow/test-config.json"),
        "create .codeflow/test-config.json (the /cf-stack skill adds a stack) with a target that has a `quick` mode";
    /// A test configuration with no quick target.
    PUSH_TARGETS_NONE = Step::Edit(".codeflow/test-config.json"),
        "give a target a `quick` mode in .codeflow/test-config.json";
    /// Pending id reservations not published by this push.
    IDS_PENDING_LOCAL = Step::Codeflow("codeflow ids sync"),
        "publish them to the authority with `codeflow ids sync`, or push to the authority remote";
    /// An id sync that could not run.
    IDS_SYNC_FAILED = Step::Codeflow("codeflow ids sync"),
        "rerun `codeflow ids sync` once the authority answers";
    /// A release preflight finding short of a broken tree.
    RELEASE_PREFLIGHT_NOTE = Step::Edit("CHANGELOG.md"),
        "resolve what the note names (most often the `CHANGELOG.md` entry), then rerun `python3 {script} preflight --branch {branch}`";
    /// A release preflight that could not run.
    RELEASE_PREFLIGHT_UNRUN = Step::Edit("scripts/release.py"),
        "make `python3 scripts/release.py preflight --branch {branch}` run (python3 on PATH, the script intact), then push again";
    /// A push that breaks the release tree.
    RELEASE_PREFLIGHT = Step::Edit("CHANGELOG.md"),
        "fix the release state the preflight names above (the `CHANGELOG.md` entry or the release files it lists), then rerun `python3 {script} preflight --branch {branch}`";
    /// A test configuration that does not load.
    TEST_CONFIG_REPAIR = Step::Edit(".codeflow/test-config.json"),
        "repair .codeflow/test-config.json, then run `codeflow test --mode quick`";

    // Doctor.

    /// Git hooks that do not run the codeflow shims.
    DOCTOR_HOOKS_PATH = Step::Git("git config"),
        "run `git config core.hooksPath {hooks}` so git runs the codeflow shims in each worktree";
    /// A project without its codeflow configuration.
    DOCTOR_INIT = Step::Codeflow("codeflow init"),
        "run `codeflow init` in the project root";
    /// A tool doctor looks for on PATH and does not find.
    DOCTOR_TOOL_MISSING = Step::Codeflow("codeflow doctor"),
        "install {tool} on PATH, then `codeflow doctor --check {check}` confirms it";
    /// Git hooks another hook manager owns, whose stages do not call the
    /// codeflow shims.
    DOCTOR_HOOK_MANAGER = Step::Edit("{path}"),
        "make each hook named in {path} executable, calling its codeflow shim on a live line (its pre-commit hook runs `.codeflow/git-hooks/pre-commit \"$@\"`, and so on for {hooks}); `codeflow doctor --check hooks` then stops warning, and a commit with a bad subject being refused confirms the calls run";
    /// Manager hooks that name every shim: reading cannot show they run it.
    DOCTOR_HOOK_WIRING_UNSEEN = Step::Git("git commit"),
        "confirm the hooks in {path} run the codeflow shims with a real git event: on a scratch branch, `git commit --allow-empty -m \"Bad subject.\"` must be refused under git.commit_format; `codeflow doctor` reads the hook files and cannot verify that they run";
    /// Hooks that another harness runs only once approved there.
    DOCTOR_HARNESS_APPROVAL = Step::Codeflow("codeflow doctor"),
        "{step} (an approval inside that harness), then `codeflow doctor --check {check}` confirms it";
    /// A network doctor cannot reach.
    DOCTOR_NETWORK = Step::Codeflow("codeflow doctor"),
        "restore network access to github.com, then `codeflow doctor --check network` confirms it";
    /// Delegation prerequisites missing on this machine: a tool, the Codex
    /// plugin in Claude Code, or an MCP inventory that does not answer.
    DOCTOR_DELEGATES = Step::Codeflow("codeflow doctor"),
        "install, enable or repair each missing piece named, then `codeflow doctor --check delegates` confirms it";
    /// A delegate CLI that is not signed in.
    DOCTOR_DELEGATES_SIGN_IN = Step::Codeflow("codeflow doctor"),
        "sign in with `codex login` (the operator's own account), then `codeflow doctor --check delegates` confirms it";
    /// A model binding whose harness or settings changed since it qualified.
    DOCTOR_REQUALIFY = Step::Codeflow("codeflow doctor"),
        "requalify each binding named with the /cf-evaluate-model skill, whose promotion needs a human's explicit approval, then `codeflow doctor --check model-bindings` confirms it";
    /// Harness trust doctor cannot read: the manual confirmation.
    DOCTOR_UNSEEN = Step::Manual("{step}"),
        "{step} (inside that harness); `codeflow doctor` cannot verify it";
    /// A model binding doctor cannot observe: the manual confirmation.
    DOCTOR_CANARY = Step::Manual("confirm each binding named with a native canary"),
        "confirm each binding named with a native canary (/cf-evaluate-model) when freshness matters; `codeflow doctor` cannot verify it";
    /// A policy or release setting that awaits a project decision.
    DOCTOR_POLICY_DECISION = Step::Edit(".codeflow/policy.json"),
        "record the decision in .codeflow/policy.json ({decision}), then `codeflow doctor --check adopter-fit` confirms it";
    /// A release backend that disagrees with the release tools found.
    DOCTOR_RELEASE_BACKEND = Step::Edit(".codeflow/project.toml"),
        "set `[release] backend` in .codeflow/project.toml to the one version authority this project uses";
    /// A CI workflow whose install step is still the placeholder.
    DOCTOR_CI_PLACEHOLDER = Step::Edit("{path}"),
        "replace the PLACEHOLDER install step in {path} with the release installer, so the test and validate gates run";
    /// A checkout that pins no codeflow version for CI to install.
    DOCTOR_CI_PIN_MISSING = Step::Edit(".codeflow/project.toml"),
        "set scaffold_version in .codeflow/project.toml to the codeflow version CI installs";
    /// A target branch that pins no codeflow version.
    DOCTOR_CI_PIN_TARGET = Step::Edit(".codeflow/project.toml"),
        "land a scaffold_version pin in .codeflow/project.toml on {target} first, in a pull request that changes only that line";
    /// A checkout that lowers the pin of the branch it lands on.
    DOCTOR_CI_PIN_LOWERED = Step::Edit(".codeflow/project.toml"),
        "set scaffold_version in .codeflow/project.toml back to {pin}, the version {target} pins";
    /// An upgrade that carries `codeflow update` before its raised pin lands.
    DOCTOR_CI_PIN_ORDER = Step::Edit(".codeflow/project.toml"),
        "upgrade in two pull requests, in order: first raise only scaffold_version in .codeflow/project.toml and land it; then run `codeflow update` on a new branch";
    /// A tracking setting that does not read.
    DOCTOR_TRACKING_UNKNOWN = Step::Edit(".codeflow/project.toml"),
        "repair .codeflow/project.toml so durable-work tracking reads as on or off";
    /// Id registry findings.
    DOCTOR_ID_REGISTRY = Step::Codeflow("codeflow ids check"),
        "resolve each finding named (`codeflow ids admit` registers an unplaced id, `codeflow ids retarget <id>` renumbers a collision), then `codeflow ids check` confirms";
    /// An id registry without host rules.
    DOCTOR_REGISTRY_UNPROTECTED = Step::Codeflow("codeflow remote protect"),
        "run `codeflow remote protect` to apply the host rules for codeflow/registry";
    /// Managed regions edited by hand.
    DOCTOR_MANAGED_DRIFT = Step::Codeflow("codeflow update"),
        "move the hand edits outside the codeflow markers (or accept losing them), then `codeflow update` rewrites the blocks";
    /// Project context still at its template placeholders.
    DOCTOR_CUSTOMIZATION = Step::Edit("{path}"),
        "replace the template placeholders in {path} with this project's context (the /cf-customize skill checks it against the project)";
    /// An instruction chain over Codex's limit.
    DOCTOR_INSTRUCTIONS = Step::Edit("AGENTS.md"),
        "move project detail out of the project section of AGENTS.md into files it points at";
    /// Reading sizes above their guidelines.
    DOCTOR_READING = Step::Edit("{path}"),
        "move detail in {path} behind a trigger (an index entry or a conditional read), never cutting a duty";
    /// The root checkout, nested repositories and linked worktrees
    /// (ADR-0074); each finding names its own next step.
    DOCTOR_ROOT_CHECKOUT = Step::Codeflow("codeflow doctor"),
        "take the next step each finding above names, then `codeflow doctor --check repo-integrity` confirms it";
    /// Test configuration health findings.
    DOCTOR_TEST_CONFIG = Step::Edit(".codeflow/test-config.json"),
        "fix the named checks in .codeflow/test-config.json; `codeflow doctor --check test-config` rechecks it";

    // Session guards.

    /// A git command the guard cannot classify.
    GUARD_UNCLASSIFIABLE = Step::Git("git commit"),
        "write the git command, its options and its targets literally; generated text belongs only in a quoted message (`git commit -m \"$(…)\"`), or compute a value first and pass it as a literal";
    /// A git command whose target repository the guard cannot resolve.
    GUARD_UNRESOLVED = Step::Git("git -C"),
        "name the repository with a literal path (`git -C /path/to/repo …`) or change to it first (`cd /path/to/repo && git …`), so the guard can read its branch and policy";
    /// A git alias the guard cannot resolve.
    GUARD_ALIAS = Step::Git("git config"),
        "write the git command the alias stands for, or make the alias readable: a git-command alias (not a `!` shell alias) set with `git config`, not through `--config-env` or configuration environment variables";
    /// A headless peer run.
    HEADLESS_PEER_RUN = Step::Codeflow("codeflow delegate"),
        "delegate through an interactive seat instead: Claude Code to Codex through the Codex plugin, Codex to Claude through `codeflow delegate` over the interactive `claude` CLI, or a named Herdr tab (cf-delegate); {enforcement} (policy security.headless_peer_runs)";
    /// A hook that could not evaluate and let the operation through.
    HOOK_UNEVALUATED = Step::Codeflow("codeflow doctor"),
        "fix the cause named above (a hook manager must pass git's arguments and stdin through to the codeflow shim), then rerun the git command; `codeflow doctor --check hooks` checks the hook wiring";
    /// A git hook that could not read its input from git.
    HOOK_STDIN_UNREAD = Step::Git("git push"),
        "rerun `git push` so git hands the hook its refs on stdin: a hook manager must pass its stdin through to the codeflow shim, and a branch whose name is not UTF-8 is pushed under a UTF-8 name (`git branch <new> <old>`, then push <new>); server-side CI stays authoritative meanwhile";
    /// A session guard input that is not a JSON hook payload, or whose
    /// known field has the wrong type: the harness entry that runs the guard
    /// does not pass the payload through unchanged.
    GUARD_PAYLOAD_MALFORMED = Step::Edit("{path}"),
        "make the hook entry that runs `codeflow hook {guard}` pass the harness payload on stdin unchanged, in {path}; the next tool call is then read";
    /// A session summary that could not be written.
    SESSION_SUMMARY_UNWRITTEN = Step::Edit("{path}"),
        "{repair}: {path}; the session ledger lives under git's common directory, and the next session end writes it";
    /// A refusal the hook or guard could not write to the refusals ledger.
    REFUSAL_UNRECORDED = Step::Edit("{path}"),
        "{repair}: {path}; the refusals ledger lives under git's common directory, and the next refusal writes it";
    /// The per-user project registry that could not be written.
    REGISTRY_UNWRITTEN = Step::Edit("{path}"),
        "repair or delete {path}, the per-user project registry; the next codeflow command writes it again";

    /// An action reserved for the operator, including a secret-store read.
    OUTWARD_ACTION = Step::Edit(".codeflow/policy.json"),
        "the operator runs this action personally in a separate terminal; prepare and inspect its inputs here, without retrying another spelling; operator-owned relief is configured in `.codeflow/policy.json`";

    /// A privilege escalation proposed from a session.
    PRIVILEGE_ESCALATION = Step::Edit(".codeflow/policy.json"),
        "privilege escalation needs applicable operator authority: an operator runs it outside the session; {enforcement}; the effective harness may show no permission prompt (security.privilege_escalation in `.codeflow/policy.json`)";
}
