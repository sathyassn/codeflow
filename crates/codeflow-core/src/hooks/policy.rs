//! Typed view of the `policy.json` `git` section (charter §6.1).
//!
//! One config read by several enforcement planes (D7) — but not every plane
//! reads every field; this struct is the shared schema, not a promise that all
//! of it is live everywhere. The git client hooks read the protected-branch set
//! and its protection levels, the commit-format / attribution / emoji /
//! policy-character / branch-naming rules, `secret_scan` (pre-commit), and
//! `test_gate_on_push` (pre-push); the Claude `git-guard` reads the same plus `pr_merge_to_protected`
//! and `hook_integrity`; remote protection reads the protected-branch set and its
//! force-push / delete / push intents; and CI reads `dep_audit` and
//! `security_review` (the umbrella gate for the `security-review` job) and,
//! through `codeflow ci`, the commit and branch rules. Every field is a default
//! the user may flip per repo; missing or malformed files fall back to the strict
//! charter defaults so enforcement never silently disables itself. That fallback
//! is fail-safe, not consumer-friendly — a present-but-invalid file is caught
//! loudly by the [`policy_schema`](super::policy_schema) strict validator at the
//! commit-msg hook, `codeflow ci`, and `codeflow validate`, and the whole key
//! schema is renderable from the binary via `codeflow policy explain`.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::security::git::is_on_protected_branch;
use crate::security::SecurityPolicy;

/// Branch prefix for integration lines shared by policy and range checks.
pub const INTEGRATION_BRANCH_PREFIX: &str = "integration/";

/// Enforcement level for a policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyLevel {
    /// Violations stop the operation.
    Block,
    /// Violations are reported but the operation proceeds.
    Warn,
    /// The operation is explicitly permitted.
    Allow,
    /// The check is not run at all.
    Off,
}

impl PolicyLevel {
    /// `true` when the rule produces a violation (block or warn).
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Block | Self::Warn)
    }

    /// The lower of two levels, in the order block, warn, allow, off.
    #[must_use]
    pub fn lower(self, other: Self) -> Self {
        let rank = |level: Self| match level {
            Self::Block => 3,
            Self::Warn => 2,
            Self::Allow => 1,
            Self::Off => 0,
        };
        if rank(other) < rank(self) {
            other
        } else {
            self
        }
    }
}

impl fmt::Display for PolicyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Block => f.write_str("block"),
            Self::Warn => f.write_str("warn"),
            Self::Allow => f.write_str("allow"),
            Self::Off => f.write_str("off"),
        }
    }
}

/// One trusted automation profile (SPC-013 R-82): the bot actors or app ids
/// it trusts, the branch pattern its pull requests use, and content for the
/// PR sections its bot body omits (heading to content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationProfile {
    pub name: String,
    pub actors: Vec<String>,
    pub branch_pattern: String,
    #[serde(default)]
    pub sections: std::collections::BTreeMap<String, String>,
    /// The unit name the profile supplies as the pull request's `Task:`
    /// line when the bot body has none; a bot never names a task record.
    #[serde(default)]
    pub task: Option<String>,
}

impl AutomationProfile {
    /// `true` when `actor` is one of the profile's actors and `branch`
    /// matches its pattern. The actor `unknown` (a local run, a fork pull
    /// request, or no actor passed) never matches.
    #[must_use]
    pub fn matches(&self, actor: &str, branch: &str) -> bool {
        actor != UNKNOWN_ACTOR
            && !actor.is_empty()
            && self.actors.iter().any(|a| a == actor)
            && self.branch_matches(branch)
    }

    /// `true` when `branch` matches the profile's glob pattern.
    #[must_use]
    pub fn branch_matches(&self, branch: &str) -> bool {
        glob::Pattern::new(&self.branch_pattern).is_ok_and(|p| p.matches(branch))
    }
}

/// The actor `codeflow ci` reports when no trusted actor was passed.
pub const UNKNOWN_ACTOR: &str = "unknown";

/// The four states of a kept brownfield PR template (SPC-013 R-84).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MappingState {
    /// Found and proposed; no operator decision yet.
    Diagnosed,
    /// The operator accepted the heading mapping.
    Accepted,
    /// The operator refused it; the missing headings go into the template.
    Refused,
    /// The project sets its own section list in policy.
    Custom,
}

impl fmt::Display for MappingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Diagnosed => "diagnosed",
            Self::Accepted => "accepted",
            Self::Refused => "refused",
            Self::Custom => "custom",
        })
    }
}

/// `git.pr_section_mapping`: the state, the proposed or accepted heading
/// mapping (required heading to the template's heading), and the decision
/// date (`none` while diagnosed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrSectionMapping {
    pub state: MappingState,
    #[serde(default)]
    pub headings: std::collections::BTreeMap<String, String>,
    #[serde(default = "decided_none")]
    pub decided: String,
}

fn decided_none() -> String {
    "none".to_string()
}

/// The `git` section of `.codeflow/policy.json` (charter §6.1, verbatim
/// schema). Defaults carry the v1 strictness decision (D16).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitPolicy {
    /// Protected branch names or glob patterns (e.g. `release/*`),
    /// read by ALL enforcement planes.
    pub protected_branches: Vec<String>,
    pub commit_to_protected: PolicyLevel,
    pub push_to_protected: PolicyLevel,
    pub force_push_protected: PolicyLevel,
    pub force_push_unprotected: PolicyLevel,
    pub delete_protected: PolicyLevel,
    pub hard_reset_protected: PolicyLevel,
    /// A merge commit landing on a protected branch (the git layer's
    /// `pre-merge-commit` stage and git-guard's `git merge`/`git cherry-pick`
    /// interception). Agents land protected merges only via PR or
    /// `codeflow integrate`; a human may override the git layer with
    /// [`HUMAN_OVERRIDE_ENV`](super::HUMAN_OVERRIDE_ENV).
    pub merge_to_protected: PolicyLevel,
    /// A `gh pr merge` whose base branch is protected (git-guard only). A
    /// human merges via the GitHub UI or their own terminal; agents do not.
    pub pr_merge_to_protected: PolicyLevel,
    /// Any local update of a protected branch ref that did not come from the
    /// remote — the harness-agnostic backstop enforced by git's
    /// `reference-transaction` hook (ADR-0007). Catches what classic client
    /// hooks miss: fast-forward merges, `reset --hard`, and `branch -D` on a
    /// protected branch. A move to (or behind) the remote-tracking head is a
    /// legitimate sync and is allowed; the integrate token and human override
    /// pass. Deletion is governed by `delete_protected`.
    pub local_ref_protection: PolicyLevel,
    /// Integrity of the enforcement plane itself (git-guard only, ADR-0009):
    /// attempts to disarm or tamper with the hooks and their policy — hook-path
    /// manipulation (`git config core.hooksPath`, `git -c core.hooksPath=…`,
    /// `git --config-env=core.hooksPath=<VAR>`, `--unset core.hooksPath`,
    /// `GIT_CONFIG_*` env injection), hook-skip env prefixes (`GIT_SKIP_HOOKS=`,
    /// `HUSKY=0`), and Bash writes/removes targeting the hook shims
    /// (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
    /// (`.codeflow/policy.json`, `.codeflow/project.toml`). A floor-raise for
    /// honest agents, not a boundary: the Write tool bypasses it and remote+CI
    /// stay the hard line (charter D19). Suspended only in the
    /// pre-first-commit bootstrap window.
    pub hook_integrity: PolicyLevel,
    /// The branch the root checkout holds: the repository's main working
    /// tree, where no task work happens. Empty (the default) means the
    /// repository's default branch. Change it only for an umbrella
    /// repository whose root is a working checkout; the convention is
    /// [`WORKSPACE_ROOT_BRANCH`](crate::root_checkout::WORKSPACE_ROOT_BRANCH),
    /// which `codeflow init --workspace` sets.
    pub root_branch: String,
    /// A commit at the root checkout on any branch other than
    /// [`root_branch`](GitPolicy::root_branch). Level; default `block`.
    /// git-guard applies the level to agents; the git hooks apply it when a
    /// harness marker is set and warn otherwise, so a human at their own
    /// terminal is never stopped. Suspended in the bootstrap window.
    pub root_checkout_commits: PolicyLevel,
    /// Folders where linked worktrees may live: a path relative to the root
    /// checkout, an absolute path, `~/...`, or `$CODEX_HOME/...` or
    /// `$GROK_HOME/...`. The default covers `.worktrees` and the folders the
    /// harnesses manage; `doctor` reports a linked worktree outside them.
    pub worktree_locations: Vec<String>,
    /// Additional remotes whose branches count as advisory work claims.
    /// Origin and the target's fetch remote always count. Empty by default.
    pub claim_remotes: Vec<String>,
    pub commit_format: PolicyLevel,
    pub commit_types: Vec<String>,
    /// Max length of the commit *description* — the text after `type(scope): `.
    /// Integer characters; default 50 (the restored v1 subject budget, ADR-0020).
    /// Enforced under `commit_format`.
    pub commit_desc_max_len: u32,
    /// Max length of the whole subject line (git's 72-column wrap). Integer
    /// characters; default 72 (ADR-0020). Enforced under `commit_format`.
    pub commit_subject_max_len: u32,
    /// Whether the commit-body shape is enforced (ADR-0020). Level
    /// (off/warn/allow/block); default `block`. When active, the body is only
    /// `- ` bullets (≤ `commit_body_max_bullets`, each ≤ `commit_body_bullet_max_len`
    /// chars), blank lines, an optional `BREAKING CHANGE:` footer, and any opted-in
    /// footer trailers; a prose paragraph, numbered list, or story blocks.
    /// Auto-generated subjects (merge/revert/fixup/squash) are exempt.
    pub commit_body: PolicyLevel,
    /// Max number of `- ` bullets allowed in a commit body. Integer; default 3
    /// (ADR-0020).
    pub commit_body_max_bullets: u32,
    /// Max length of a single commit-body bullet line, including the `- ` marker.
    /// Integer characters; default 72 (ADR-0020).
    pub commit_body_bullet_max_len: u32,
    /// Extra footer-trailer tokens ALLOWED (optional) in the body beyond the
    /// always-allowed `BREAKING CHANGE:` footer (ADR-0020 footer/ticket
    /// amendment). Default empty — the strict baseline is bullets plus
    /// `BREAKING CHANGE:` only, so an agent cannot fill a default-allowed trailer
    /// slot with noise; a project opts a trailer in deliberately (e.g.
    /// `Signed-off-by` for DCO). A footer line `<Token>: <value>` (or
    /// `<Token> #<value>` for an issue ref) whose `<Token>` is here — matched
    /// exactly, case-sensitively — is a sanctioned footer, not a blocked prose
    /// line. Enforced under `commit_body`.
    pub commit_footer_tokens: Vec<String>,
    /// Footer-trailer tokens that MUST appear in every non-exempt commit (e.g.
    /// `["Signed-off-by"]` for DCO). Default empty. A required token is implicitly
    /// allowed (no need to also list it in
    /// [`commit_footer_tokens`](GitPolicy::commit_footer_tokens)); a commit
    /// missing one is blocked under `commit_body`.
    pub commit_required_footers: Vec<String>,
    /// Ticket-reference footer tokens the project recognizes (e.g.
    /// `["Refs", "Closes"]`). Default empty = the ticket feature is off and a
    /// `Refs:` line is NOT auto-allowed (it blocks unless added to
    /// [`commit_footer_tokens`](GitPolicy::commit_footer_tokens)). Non-empty =
    /// ticket trailers with these tokens
    /// are allowed in the footer; whether one is *required* is
    /// [`commit_ticket_required`](GitPolicy::commit_ticket_required).
    pub commit_ticket_keys: Vec<String>,
    /// Whether a matching ticket-reference trailer is REQUIRED (ADR-0020). Default
    /// `off` — ticket trailers are allowed-but-optional (a present one passes, an
    /// absent one is fine). `warn`/`block` require at least one matching trailer;
    /// a commit without one warns or blocks. Only meaningful when
    /// [`commit_ticket_keys`](GitPolicy::commit_ticket_keys) is non-empty.
    /// Merge/revert/fixup/squash exempt.
    pub commit_ticket_required: PolicyLevel,
    /// Regex a ticket trailer's value must match. String; default empty = no
    /// format check. When set, a present ticket trailer whose value does not match
    /// is a malformed reference and blocks; when a ticket is required, the present
    /// one must match. At enforcement time an unparseable pattern degrades to no
    /// format check (fail-open, matching the glob convention) — which is why the
    /// strict validator ([`policy_schema`](super::policy_schema)) rejects it
    /// loudly first.
    pub commit_ticket_pattern: String,
    /// Path globs (the `glob` crate's syntax) naming the repo's declared
    /// contract surfaces (ADR-0020). When non-empty, a commit-msg WARN fires if a
    /// staged file matches one of these globs and the message carries no breaking
    /// marker (`type!:` or a `BREAKING CHANGE:` footer) — a nudge to confirm the
    /// change is not breaking, never a block (breaking-ness is semantic and
    /// unprovable). Shipped default empty: consumers declare their own surfaces.
    pub breaking_watch_paths: Vec<String>,
    pub ai_attribution: PolicyLevel,
    pub commit_emoji: PolicyLevel,
    /// The policy characters of ADR-0067, U+2013 (en dash) and U+2014 (em
    /// dash), in new text: commit subjects and bodies (commit-msg hook), PR
    /// bodies, and lines a `codeflow ci` range adds under the written-content
    /// trees. Level (off/warn/allow/block); default `warn`, a writing
    /// guideline that review and evaluation judge (this repository sets
    /// `block`). Existing bytes are grandfathered: `codeflow ci` judges added
    /// lines, never the tree, and skips a file whose bytes are exactly the
    /// whole-file managed asset the running binary ships for that path.
    pub policy_characters: PolicyLevel,
    /// Structure of the PR/MR body — the section check `codeflow ci` runs when
    /// a PR body is provided (`--pr-body`, `--pr-body-file`, or
    /// `CODEFLOW_PR_BODY`). Level (off/warn/allow/block); default `block`.
    /// Governs both section lists below; the always-warn template-remnant
    /// scan rides along. Missing bodies fail on PR events; local/push runs
    /// without a body skip.
    pub pr_sections: PolicyLevel,
    /// Generic release declaration syntax, consistency and breaking commit floor.
    /// Defaults to warn; off/allow disables this independent check.
    pub pr_release_impact: PolicyLevel,
    /// The shape of the PR body's Summary section, which `codeflow ci`
    /// checks on a provided body: one prose paragraph that anchors the
    /// reader, then a list or table, then at most one closing paragraph
    /// (ADR-0071, note of 2026-10-03). Level (off/warn/allow/block); default
    /// `block`, independent of `pr_sections`. It judges block shape, never a
    /// word or sentence count. Runs at most at `warn` while a kept PR
    /// template is diagnosed, and a trusted automation profile skips it.
    pub pr_summary: PolicyLevel,
    /// Minimum impact level for incompatibility (major by default).
    /// Pre-1.0 projects can explicitly choose minor.
    pub pr_breaking_level: String,
    /// Markdown headings every PR body must carry (matched case-insensitively
    /// at `##`/`###` depth). A present-but-empty section — nothing but HTML
    /// comments and bare `-` bullets before the next heading — counts as
    /// missing. The built-in default (no policy file, or a file without this
    /// key) stays `["Summary", "Changes"]` so a binary upgrade alone never
    /// starts blocking; the scaffolded `policy.json` adds Reviews and Release
    /// impact. Enforced under `pr_sections`; updates preserve existing lists.
    pub pr_required_sections: Vec<String>,
    /// Headings required ONLY when the commit range touches non-docs files
    /// (docs-only = every changed path is `*.md`, `*.txt`, `LICENSE*`,
    /// `docs/**`, or a `.github` template; anything else — or an unresolvable
    /// range — counts as code). Default `["Testing"]`. Enforced under
    /// `pr_sections`.
    pub pr_code_sections: Vec<String>,
    /// The level of the adjustable work-record rules of SPC-013 R-80: the
    /// acceptance block bound to the reviewed commit and the journey
    /// criterion for adopter-facing ranges. Accepts `block` or `warn` only;
    /// `off` does not exist (R-81). Default `block`. The transition rules
    /// (Blocker, cancellation, acceptance block on completion) always block
    /// and are not governed by this key. Read it through
    /// [`GitPolicy::work_records_level`].
    pub work_records: PolicyLevel,
    /// The level of the planning checks `work start` and `codeflow ci` run
    /// once per task, on any work prefix (TSK-133): the visible workgraph is
    /// valid, the branch's task has a record, and that record is anchored on
    /// its target. Accepts `block` or `warn`; default `block`. Read it
    /// through [`GitPolicy::work_planning_level`].
    pub work_planning: PolicyLevel,
    /// Globs naming the project's product code (SPC-013 R-70, R-114). A
    /// range touching one needs a journey criterion. Every PR names its
    /// task or epic. `codeflow init` writes the default for the detected
    /// stack and `codeflow update` adds it once, keeping any value the
    /// project set. Absent (`null`), the binary assumes the stack default
    /// recorded in `.codeflow/project.toml`.
    pub product_paths: Option<Vec<String>>,
    /// Retired compatibility key, accepted and ignored (R-70).
    pub direct_changes: String,
    /// Trusted automation profiles (SPC-013 R-82). A profile applies in
    /// `codeflow ci` only when the actor the workflow passes and the head
    /// branch both match, and only as read from the target side of the
    /// range. It skips branch naming, the commit message shape rules and the
    /// PR Summary shape, and supplies content for the PR sections its bot
    /// omits; it never changes another level. Default empty.
    pub automation_profiles: Vec<AutomationProfile>,
    /// The decision about a kept brownfield PR template (SPC-013 R-84,
    /// R-115). Absent by default; `init` and `update` write `diagnosed`
    /// when a kept template's headings do not match the required sections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_section_mapping: Option<PrSectionMapping>,
    /// The glob naming release branches (SPC-013 R-120). Read only from the
    /// policy at the default target's tip at the destination, never from a
    /// local ref or the range being judged; absent there, the built-in
    /// `integration/release-*` applies. A pattern that could match the
    /// default target or an epic line (`integration/EPC-*`) is refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_branch_pattern: Option<String>,
    pub branch_naming: PolicyLevel,
    pub branch_prefixes: Vec<String>,
    pub secret_scan: PolicyLevel,
    /// Unresolved conflict markers on the lines a change adds to a text
    /// file (TSK-170): the pre-commit hook judges the staged diff and
    /// `codeflow ci` the range. Level (off/warn/allow/block); default
    /// `block`. A file that must hold markers sets `conflict-marker-size`
    /// for its path in `.gitattributes`. Suspended by bootstrap grace.
    pub conflict_markers: PolicyLevel,
    pub test_gate_on_push: PolicyLevel,
    pub security_review: PolicyLevel,
    pub dep_audit: PolicyLevel,
    /// Discarding locally unique uncommitted work (`checkout -- .`, `reset
    /// --hard`, `clean`, `stash drop`), judged by git-guard (TSK-172).
    /// Default `block`.
    pub discard_uncommitted: PolicyLevel,
    /// Regenerable directories `git clean` may remove without refusal
    /// (TSK-172).
    pub clean_regenerable: Vec<String>,
}

impl Default for GitPolicy {
    fn default() -> Self {
        Self {
            protected_branches: vec!["main".into(), "master".into()],
            commit_to_protected: PolicyLevel::Block,
            push_to_protected: PolicyLevel::Block,
            force_push_protected: PolicyLevel::Block,
            force_push_unprotected: PolicyLevel::Allow,
            delete_protected: PolicyLevel::Block,
            hard_reset_protected: PolicyLevel::Block,
            merge_to_protected: PolicyLevel::Block,
            pr_merge_to_protected: PolicyLevel::Block,
            local_ref_protection: PolicyLevel::Block,
            hook_integrity: PolicyLevel::Block,
            root_branch: String::new(),
            root_checkout_commits: PolicyLevel::Block,
            worktree_locations: crate::root_checkout::DEFAULT_WORKTREE_LOCATIONS
                .iter()
                .map(ToString::to_string)
                .collect(),
            claim_remotes: Vec::new(),
            commit_format: PolicyLevel::Block,
            commit_types: [
                "feat", "fix", "docs", "refactor", "test", "chore", "ci", "perf", "build", "revert",
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
            commit_desc_max_len: 50,
            commit_subject_max_len: 72,
            commit_body: PolicyLevel::Block,
            commit_body_max_bullets: 3,
            commit_body_bullet_max_len: 72,
            // Strict baseline: no extra footer tokens are allowed by default —
            // only `- ` bullets and the `BREAKING CHANGE:` footer. Every opt-in
            // list is empty and ticket enforcement is off; a project turns on
            // exactly what it needs (an agent fills every default-allowed slot).
            commit_footer_tokens: Vec::new(),
            commit_required_footers: Vec::new(),
            commit_ticket_keys: Vec::new(),
            commit_ticket_required: PolicyLevel::Off,
            commit_ticket_pattern: String::new(),
            // Empty by default: consumers declare their own contract surfaces.
            breaking_watch_paths: Vec::new(),
            ai_attribution: PolicyLevel::Block,
            commit_emoji: PolicyLevel::Block,
            policy_characters: PolicyLevel::Warn,
            pr_sections: PolicyLevel::Block,
            pr_release_impact: PolicyLevel::Warn,
            pr_summary: PolicyLevel::Block,
            pr_breaking_level: "major".into(),
            pr_required_sections: vec!["Summary".into(), "Changes".into()],
            pr_code_sections: vec!["Testing".into()],
            work_records: PolicyLevel::Block,
            work_planning: PolicyLevel::Block,
            product_paths: None,
            direct_changes: "allow".to_string(),
            automation_profiles: Vec::new(),
            pr_section_mapping: None,
            release_branch_pattern: None,
            branch_naming: PolicyLevel::Block,
            branch_prefixes: [
                "feat/",
                "fix/",
                "docs/",
                "refactor/",
                "test/",
                "chore/",
                "ci/",
                "hotfix/",
                "plan/",
                "task/",
                "spike/",
                "experiment/",
                INTEGRATION_BRANCH_PREFIX,
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
            secret_scan: PolicyLevel::Block,
            conflict_markers: PolicyLevel::Block,
            test_gate_on_push: PolicyLevel::Block,
            security_review: PolicyLevel::Warn,
            dep_audit: PolicyLevel::Warn,
            discard_uncommitted: PolicyLevel::Block,
            clean_regenerable: [
                "target/",
                "node_modules/",
                "dist/",
                "build/",
                ".venv/",
                "__pycache__/",
                "coverage/",
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
        }
    }
}

impl GitPolicy {
    /// The effective `work_records` level: `warn` when the policy says so,
    /// `block` otherwise. A value outside `block | warn` (the strict
    /// validator rejects it) never switches the rules off.
    #[must_use]
    pub fn work_records_level(&self) -> PolicyLevel {
        if self.work_records == PolicyLevel::Warn {
            PolicyLevel::Warn
        } else {
            PolicyLevel::Block
        }
    }

    /// The effective `work_planning` level: `warn` when the policy says so,
    /// `block` otherwise, so no value switches the planning checks off.
    #[must_use]
    pub fn work_planning_level(&self) -> PolicyLevel {
        if self.work_planning == PolicyLevel::Warn {
            PolicyLevel::Warn
        } else {
            PolicyLevel::Block
        }
    }

    /// `true` when `branch` matches the protected list (names and globs).
    /// Reuses the security plane's matcher so all planes agree (D7).
    #[must_use]
    pub fn branch_is_protected(&self, branch: &str) -> bool {
        let sec = SecurityPolicy {
            protected_branches: self.protected_branches.clone(),
            ..SecurityPolicy::defaults()
        };
        is_on_protected_branch(branch, &sec)
    }

    /// `true` when `branch` starts with one of the sanctioned prefixes
    /// (charter §6.4). Protected branches are exempt — they are governed by
    /// the protection rules, not naming.
    #[must_use]
    pub fn branch_name_ok(&self, branch: &str) -> bool {
        if self.branch_is_protected(branch) {
            return true;
        }
        self.branch_prefixes.iter().any(|p| branch.starts_with(p))
    }

    /// The configured protected branch entries (names and glob patterns), for
    /// display when a bulk operation is judged to reach all of them.
    #[must_use]
    pub fn protected_branch_names(&self) -> Vec<String> {
        self.protected_branches.clone()
    }

    /// The footer-trailer tokens the body-shape check recognizes as sanctioned
    /// (ADR-0020): the opt-in `commit_footer_tokens`, every `commit_required_footers`
    /// entry (a required token is implicitly allowed), and every `commit_ticket_keys`
    /// entry (a ticket trailer is allowed when the ticket feature is on).
    /// `BREAKING CHANGE:` is always allowed and handled separately. Empty by
    /// default → the strict baseline (bullets plus `BREAKING CHANGE:` only).
    #[must_use]
    pub fn allowed_footer_tokens(&self) -> Vec<String> {
        let mut tokens = self.commit_footer_tokens.clone();
        for extra in self
            .commit_required_footers
            .iter()
            .chain(&self.commit_ticket_keys)
        {
            if !tokens.contains(extra) {
                tokens.push(extra.clone());
            }
        }
        tokens
    }

    /// The first non-glob protected branch — used as the default integration
    /// base (e.g. for session-summary diff stats).
    #[must_use]
    pub fn default_base_branch(&self) -> Option<&str> {
        self.protected_branches
            .iter()
            .find(|b| !b.contains('*') && !b.contains('?'))
            .map(String::as_str)
    }
}

/// The `security` section of `.codeflow/policy.json` — the exec-guard posture
/// (ADR-0008, amended by ADR-0075). Separate from the `git` section:
/// `dangerous_commands` is accepted only as the explicit `block` marker and
/// remains enforced as a block despite a stale or hand-edited weaker value.
/// The keys after `headless_peer_runs` are read by the guard families of
/// TSK-172 to TSK-174; this unit ships their defaults (TSK-171).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecuritySection {
    /// Destructive commands the `dangerous` module catches (rm -rf on `/`, `~`,
    /// or system dirs; `dd` to a block device; `mkfs`; fork bombs; recursive
    /// chmod/chown on system paths). Pinned to `block`: never legitimate in a
    /// project, so no sanctioned path exists — the guard is the hard line.
    pub dangerous_commands: PolicyLevel,
    /// Privilege escalation the `privilege` module catches (Unix and Windows
    /// launchers, direct, chained or wrapped in a shell `-c` string or
    /// `eval`; `LD_PRELOAD` and a `PATH` through `/tmp`). Default
    /// `block` (operator decision D5 of 2026-09-28, ADR-0075): the presets
    /// deny the plain forms without a prompt and the guard refuses the
    /// wrapped ones; the operator runs privileged commands.
    pub privilege_escalation: PolicyLevel,
    /// A headless peer run (`claude -p`, `codex exec`, `grok -p`), TSK-136.
    /// Peer seats run interactively; a headless run has no verified native
    /// session or recheckable thread. Default `block` (D4, ADR-0075); a
    /// project can set `headless_peer_runs` to warn or off.
    pub headless_peer_runs: PolicyLevel,
    /// Findings in a script file a command runs (TSK-172). Default `warn`.
    pub script_bypass: PolicyLevel,
    /// Outward actions the text rules cannot spell: publishing, releases,
    /// tag pushes and account changes behind flags or wrappers (TSK-172).
    /// Default `block`.
    pub outward_actions: PolicyLevel,
    /// Interpreter-embedded forms of the refused actions (TSK-172). Default
    /// `block`.
    pub interpreter_scan: PolicyLevel,
    /// Shell reads of the secret stores (TSK-172). Default `block`.
    pub secret_reads: PolicyLevel,
    /// Commits and ref moves whose enforcement entries match no approved
    /// identity of the enforcement baseline (TSK-172). Default `block`.
    pub enforcement_baseline: PolicyLevel,
    /// A push that introduces a `.github/workflows/` change: `queue` refuses
    /// it and queues it for the operator, `allow` lets it through for an
    /// agent credential with workflow rights (D9, TSK-172 AC-23).
    pub workflow_pushes: String,
    /// Claude's unsandboxed retry: `allowlist` permits only an argument
    /// vector an entry of `sandbox_retry_allow` spells out, `block` refuses
    /// every retry (TSK-174).
    pub sandbox_retry: String,
    /// The argument-bound retry entries (TSK-174). The shipped entry is the
    /// Codex plugin companion; its qualification fields are filled by
    /// TSK-174.
    pub sandbox_retry_allow: Vec<serde_json::Value>,
}

/// The shipped retry entry: the Codex plugin companion, argument-bound. The
/// placeholders are replaced by the values TSK-174 records at qualification;
/// until then no command can match the entry, since its program path is not
/// absolute.
fn default_retry_allow() -> Vec<serde_json::Value> {
    let flags_review = serde_json::json!(["--wait", "--background", "--json"]);
    let values_review = serde_json::json!({
        "--base": "git-ref",
        "--scope": ["auto", "working-tree", "branch"],
        "--model": "word"
    });
    vec![serde_json::json!({
        "program": "node",
        "script": "~/.claude/plugins/cache/openai-codex/codex/1.0.6/scripts/codex-companion.mjs",
        "program_options": [],
        "cwd": "project",
        "refuse_if_env": ["NODE_OPTIONS", "NODE_PATH", "NODE_REPL_EXTERNAL_MODULE"],
        "subcommands": {
            "review": {"flags": flags_review, "values": values_review, "positionals": 0},
            "adversarial-review": {
                "flags": flags_review,
                "values": values_review,
                "positionals": "text"
            },
            "task": {
                "flags": ["--background", "--write", "--resume-last", "--resume", "--fresh", "--json"],
                "values": {
                    "--model": "word",
                    "--effort": ["none", "minimal", "low", "medium", "high", "xhigh"]
                },
                "positionals": "text"
            },
            "status": {
                "flags": ["--all", "--wait", "--json"],
                "values": {"--timeout-ms": "integer", "--poll-interval-ms": "integer"},
                "positionals": "job-id"
            },
            "result": {"flags": ["--json"], "values": {}, "positionals": "job-id"},
            "cancel": {"flags": ["--json"], "values": {}, "positionals": "job-id"}
        },
        "program_path": "<absolute path recorded at qualification, outside every sandbox-writable directory>",
        "script_digest": {
            "scripts/codex-companion.mjs": "<sha256 at qualification>",
            "scripts/lib/*.mjs": "<sha256 per file at qualification>"
        },
        "child_executables": {"codex": "<absolute path recorded at qualification>"},
        "refuse_writable_path_entries": true
    })]
}

impl Default for SecuritySection {
    fn default() -> Self {
        Self {
            dangerous_commands: PolicyLevel::Block,
            privilege_escalation: PolicyLevel::Block,
            headless_peer_runs: PolicyLevel::Block,
            script_bypass: PolicyLevel::Warn,
            outward_actions: PolicyLevel::Block,
            interpreter_scan: PolicyLevel::Block,
            secret_reads: PolicyLevel::Block,
            enforcement_baseline: PolicyLevel::Block,
            workflow_pushes: "queue".to_string(),
            sandbox_retry: "allowlist".to_string(),
            sandbox_retry_allow: default_retry_allow(),
        }
    }
}

/// The `guidance` section of `.codeflow/policy.json`: advisory rule
/// reminders the session hooks add to the agent's context (TSK-128). They
/// never block: an active level adds text, an inactive one adds nothing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct GuidanceSection {
    /// The one-line rule reminder the prompt-submit hook adds when a prompt
    /// asks for a duration, a status or a complex explanation. Default
    /// `warn`; `off` or `allow` turns it off. `block` is refused by
    /// validation and acts as `warn`: a reminder never stops a prompt.
    pub prompt_reminders: PolicyLevel,
}

impl Default for GuidanceSection {
    fn default() -> Self {
        Self {
            prompt_reminders: PolicyLevel::Warn,
        }
    }
}

/// Full `.codeflow/policy.json` shape (only the parts the hook plane reads).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub schema_version: u32,
    pub git: GitPolicy,
    pub security: SecuritySection,
    pub guidance: GuidanceSection,
}

/// Where [`Policy::load`] sources the effective policy from — for callers that
/// report which ruleset is actually enforced (e.g. `codeflow ci`). The loader
/// itself stays silently fail-safe; this only makes the fallback legible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySource {
    /// `.codeflow/policy.json` exists and parses — the project's own rules.
    ProjectFile,
    /// The file exists but does not parse — built-in charter defaults apply.
    MalformedFile,
    /// No policy file — built-in charter defaults apply.
    Absent,
}

impl Policy {
    /// Load policy from `<root>/.codeflow/policy.json`.
    ///
    /// Missing or malformed files return the strict charter defaults
    /// (fail-safe direction, same convention as [`SecurityPolicy::load`]).
    #[must_use]
    pub fn load(root: &Path) -> Self {
        Self::load_file(&root.join(".codeflow").join("policy.json"))
    }

    /// Report where [`Policy::load`] sources the policy for `root`, so callers
    /// can state honestly whether the project file or the built-in defaults
    /// are being enforced. Mirrors [`Policy::load_file`]'s fallback rules
    /// without changing them.
    #[must_use]
    pub fn source(root: &Path) -> PolicySource {
        match std::fs::read_to_string(root.join(".codeflow").join("policy.json")) {
            Ok(data) => {
                if serde_json::from_str::<Self>(&data).is_ok() {
                    PolicySource::ProjectFile
                } else {
                    PolicySource::MalformedFile
                }
            }
            Err(_) => PolicySource::Absent,
        }
    }

    /// Load policy from an explicit file path with the same fallback rules.
    #[must_use]
    pub fn load_file(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Load the policy as the enforcement planes should apply it.
    ///
    /// Bootstrap grace (`policy_armed = false` in `.codeflow/project.toml`,
    /// charter AC #1) suspends every rule except `secret_scan` — but ONLY for
    /// its real purpose, the very first scaffold commit. Grace is honored only
    /// while the repo has no commit yet (the pre-scaffold-commit window); once
    /// any commit exists the flag is treated as armed regardless of its value,
    /// so `policy_armed = false` is not a persistent, agent-flippable
    /// off-switch (ADR-0009). Secrets are never graced (charter §6.3). Returns
    /// the effective policy and whether the policy is armed.
    #[must_use]
    pub fn load_effective(root: &Path) -> (Self, bool) {
        let mut policy = Self::load(root);
        let armed = effective_armed(root);
        if !armed {
            policy.git.suspend_for_bootstrap();
        }
        (policy, armed)
    }
}

/// Whether the enforcement planes should treat the policy as armed.
///
/// `policy_armed = true` (the fail-safe default) is always armed. A disarmed
/// flag is honored only in the pre-first-commit bootstrap window: the moment a
/// scaffold (or any) commit exists, a disarmed flag is ignored and the policy
/// is armed. This confines the grace to its intended purpose — letting the
/// first scaffold commit through — and neutralizes a mid-session re-disarm
/// (ADR-0009). The complementary `git.hook_integrity` write-block stops an
/// agent flipping the flag via Bash; this makes the flip inert however it is
/// written (e.g. via a non-Bash editor the git-guard never sees).
#[must_use]
fn effective_armed(root: &Path) -> bool {
    policy_armed(root) || repo_has_commit(root)
}

/// `true` when the repository containing `root` has at least one commit.
/// A fresh repo with an unborn HEAD (the scaffold-commit window) has none;
/// a non-repository has none. Any error resolving HEAD is treated as "no
/// commit" — that only ever *widens* grace in the harmless no-history case,
/// never narrows enforcement on a real repo.
#[must_use]
fn repo_has_commit(root: &Path) -> bool {
    let Ok(repo) = git2::Repository::discover(root) else {
        return false;
    };
    // `head()` errs on an unborn HEAD (the pre-scaffold-commit window); a
    // resolved HEAD carries the tip commit's oid. `target()` yields a `Copy`
    // oid, so no borrow of `repo` escapes.
    repo.head().ok().and_then(|head| head.target()).is_some()
}

impl GitPolicy {
    /// Turn off every rule except the secret scan (bootstrap grace).
    pub fn suspend_for_bootstrap(&mut self) {
        self.commit_to_protected = PolicyLevel::Off;
        self.push_to_protected = PolicyLevel::Off;
        self.force_push_protected = PolicyLevel::Off;
        self.force_push_unprotected = PolicyLevel::Off;
        self.delete_protected = PolicyLevel::Off;
        self.hard_reset_protected = PolicyLevel::Off;
        self.merge_to_protected = PolicyLevel::Off;
        self.pr_merge_to_protected = PolicyLevel::Off;
        self.local_ref_protection = PolicyLevel::Off;
        self.hook_integrity = PolicyLevel::Off;
        self.root_checkout_commits = PolicyLevel::Off;
        self.commit_format = PolicyLevel::Off;
        self.commit_body = PolicyLevel::Off;
        // Neutralize the footer/ticket opt-ins so the scaffold commit is never
        // walled by a required trailer during the pre-first-commit grace window.
        self.commit_ticket_required = PolicyLevel::Off;
        self.commit_ticket_keys = Vec::new();
        self.commit_required_footers = Vec::new();
        self.ai_attribution = PolicyLevel::Off;
        self.commit_emoji = PolicyLevel::Off;
        self.policy_characters = PolicyLevel::Off;
        self.pr_sections = PolicyLevel::Off;
        self.pr_release_impact = PolicyLevel::Off;
        self.pr_summary = PolicyLevel::Off;
        self.branch_naming = PolicyLevel::Off;
        self.conflict_markers = PolicyLevel::Off;
        self.test_gate_on_push = PolicyLevel::Off;
        self.security_review = PolicyLevel::Off;
        self.dep_audit = PolicyLevel::Off;
    }
}

/// Read the raw `policy_armed` flag from `.codeflow/project.toml`.
///
/// Absent file, absent key, or parse failure all mean **armed** — the
/// fail-safe direction. Init writes `policy_armed = false`, makes the
/// scaffold commit, then flips it to `true`. Callers enforcing policy should
/// use `effective_armed` (via [`Policy::load_effective`]), which additionally
/// ignores a disarmed flag once the repo has a commit (ADR-0009); this raw
/// reader is the on-disk value only.
#[must_use]
pub fn policy_armed(root: &Path) -> bool {
    read_project_toml(root)
        .and_then(|v| v.get("policy_armed").and_then(toml::Value::as_bool))
        .unwrap_or(true)
}

/// Parse `.codeflow/project.toml` leniently (user-owned file; charter §4.3
/// class 3). Returns `None` when absent or unparseable.
#[must_use]
pub fn read_project_toml(root: &Path) -> Option<toml::Value> {
    let text = std::fs::read_to_string(root.join(".codeflow").join("project.toml")).ok()?;
    text.parse::<toml::Value>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_match_charter_6_1() {
        let g = GitPolicy::default();
        assert_eq!(g.protected_branches, vec!["main", "master"]);
        assert_eq!(g.commit_to_protected, PolicyLevel::Block);
        assert_eq!(g.push_to_protected, PolicyLevel::Block);
        assert_eq!(g.force_push_protected, PolicyLevel::Block);
        assert_eq!(g.force_push_unprotected, PolicyLevel::Allow);
        assert_eq!(g.delete_protected, PolicyLevel::Block);
        assert_eq!(g.hard_reset_protected, PolicyLevel::Block);
        assert_eq!(g.merge_to_protected, PolicyLevel::Block);
        assert_eq!(g.pr_merge_to_protected, PolicyLevel::Block);
        assert_eq!(g.local_ref_protection, PolicyLevel::Block);
        assert_eq!(g.commit_format, PolicyLevel::Block);
        assert_eq!(g.commit_types.len(), 10);
        // Restored v1 commit standard (ADR-0020): 50-char description, 72-char
        // subject line, and a block-level body-shape rule (≤3 bullets, ≤72 each).
        assert_eq!(g.commit_desc_max_len, 50);
        assert_eq!(g.commit_subject_max_len, 72);
        assert_eq!(g.commit_body, PolicyLevel::Block);
        assert_eq!(g.commit_body_max_bullets, 3);
        assert_eq!(g.commit_body_bullet_max_len, 72);
        // Footer/ticket amendment (ADR-0020): strict baseline — every footer
        // opt-in is empty and ticket enforcement is off. Only `- ` bullets and
        // the `BREAKING CHANGE:` footer are allowed until a project opts in.
        assert!(g.commit_footer_tokens.is_empty());
        assert!(g.commit_required_footers.is_empty());
        assert!(g.commit_ticket_keys.is_empty());
        assert_eq!(g.commit_ticket_required, PolicyLevel::Off);
        assert!(g.commit_ticket_pattern.is_empty());
        assert!(g.allowed_footer_tokens().is_empty());
        // The contract-surface tripwire ships empty — consumers declare their own.
        assert!(g.breaking_watch_paths.is_empty());
        assert_eq!(g.ai_attribution, PolicyLevel::Block);
        assert_eq!(g.commit_emoji, PolicyLevel::Block);
        // A writing guideline: it reports, and only a project that opts in
        // (CodeFlow itself) blocks on it.
        assert_eq!(g.policy_characters, PolicyLevel::Warn);
        // PR-body structure gate: the doctrine sections ship block-enforced.
        assert_eq!(g.pr_sections, PolicyLevel::Block);
        // The built-in default must not grow: a repository without an explicit
        // list would start blocking on a binary upgrade alone.
        assert_eq!(g.pr_required_sections, vec!["Summary", "Changes"]);
        assert_eq!(g.pr_release_impact, PolicyLevel::Warn);
        // The Summary shape blocks by default, by the operator's decision of
        // 2026-10-03 (ADR-0071 note); a project lowers it with this key.
        assert_eq!(g.pr_summary, PolicyLevel::Block);
        assert_eq!(g.work_records, PolicyLevel::Block);
        assert_eq!(g.pr_breaking_level, "major");
        assert_eq!(g.pr_code_sections, vec!["Testing"]);
        assert_eq!(g.branch_naming, PolicyLevel::Block);
        assert_eq!(g.branch_prefixes.len(), 13);
        assert_eq!(g.secret_scan, PolicyLevel::Block);
        assert_eq!(g.test_gate_on_push, PolicyLevel::Block);
        // Security / red-team gates bootstrap at `warn` (ADR-0016); they
        // harden to `block` in a later slice, as test_gate_on_push did once
        // its push set became fast (TSK-132).
        assert_eq!(g.security_review, PolicyLevel::Warn);
        assert_eq!(g.dep_audit, PolicyLevel::Warn);
    }

    #[test]
    fn test_defaults_match_shipped_scaffold_policy() {
        // The embedded scaffold asset and the in-code defaults must agree.
        let asset = include_str!("../../../../assets/base/policy.json");
        let from_asset: Policy = serde_json::from_str(asset).unwrap();
        let defaults = GitPolicy::default();
        assert_eq!(
            from_asset.git.protected_branches,
            defaults.protected_branches
        );
        assert_eq!(from_asset.git.commit_types, defaults.commit_types);
        assert_eq!(
            from_asset.git.commit_desc_max_len,
            defaults.commit_desc_max_len
        );
        assert_eq!(
            from_asset.git.commit_subject_max_len,
            defaults.commit_subject_max_len
        );
        assert_eq!(from_asset.git.commit_body, defaults.commit_body);
        assert_eq!(
            from_asset.git.commit_body_max_bullets,
            defaults.commit_body_max_bullets
        );
        assert_eq!(
            from_asset.git.commit_body_bullet_max_len,
            defaults.commit_body_bullet_max_len
        );
        assert_eq!(
            from_asset.git.commit_footer_tokens,
            defaults.commit_footer_tokens
        );
        assert_eq!(
            from_asset.git.commit_required_footers,
            defaults.commit_required_footers
        );
        assert_eq!(
            from_asset.git.commit_ticket_keys,
            defaults.commit_ticket_keys
        );
        assert_eq!(
            from_asset.git.commit_ticket_required,
            defaults.commit_ticket_required
        );
        assert_eq!(
            from_asset.git.commit_ticket_pattern,
            defaults.commit_ticket_pattern
        );
        assert_eq!(
            from_asset.git.breaking_watch_paths,
            defaults.breaking_watch_paths
        );
        assert_eq!(from_asset.git.pr_sections, defaults.pr_sections);
        assert_eq!(from_asset.git.pr_release_impact, defaults.pr_release_impact);
        assert_eq!(from_asset.git.pr_summary, defaults.pr_summary);
        assert_eq!(from_asset.git.pr_breaking_level, defaults.pr_breaking_level);
        // Release impact is conditional even in an older installed four-section
        // policy. WP2 removes it from the scaffold; the unconditional list is
        // the same with either scaffold version.
        assert_eq!(
            from_asset
                .git
                .pr_required_sections
                .iter()
                .filter(|section| section.as_str() != "Release impact")
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Summary", "Changes", "Reviews"]
        );
        assert_eq!(defaults.pr_required_sections, ["Summary", "Changes"]);
        assert_eq!(from_asset.git.pr_code_sections, defaults.pr_code_sections);
        assert_eq!(from_asset.git.branch_prefixes, defaults.branch_prefixes);
        assert_eq!(from_asset.git.test_gate_on_push, defaults.test_gate_on_push);
        assert_eq!(from_asset.git.security_review, defaults.security_review);
        assert_eq!(from_asset.git.dep_audit, defaults.dep_audit);
        assert_eq!(
            from_asset.git.merge_to_protected,
            defaults.merge_to_protected
        );
        assert_eq!(
            from_asset.git.pr_merge_to_protected,
            defaults.pr_merge_to_protected
        );
        assert_eq!(
            from_asset.git.local_ref_protection,
            defaults.local_ref_protection
        );
        assert_eq!(
            from_asset.git.force_push_unprotected,
            defaults.force_push_unprotected
        );
    }

    #[test]
    fn test_security_section_defaults() {
        // ADR-0075: agent sessions refuse instead of prompting, so the
        // operator's action families default to block (D4, D5).
        let s = SecuritySection::default();
        assert_eq!(s.dangerous_commands, PolicyLevel::Block);
        assert_eq!(s.privilege_escalation, PolicyLevel::Block);
        assert_eq!(s.headless_peer_runs, PolicyLevel::Block);
        assert_eq!(s.script_bypass, PolicyLevel::Warn);
        for level in [
            s.outward_actions,
            s.interpreter_scan,
            s.secret_reads,
            s.enforcement_baseline,
        ] {
            assert_eq!(level, PolicyLevel::Block);
        }
        assert_eq!(s.workflow_pushes, "queue");
        assert_eq!(s.sandbox_retry, "allowlist");
        assert_eq!(s.sandbox_retry_allow.len(), 1);
    }

    #[test]
    fn test_policy_default_carries_security_section() {
        let p = Policy::default();
        assert_eq!(p.security.dangerous_commands, PolicyLevel::Block);
        assert_eq!(p.security.privilege_escalation, PolicyLevel::Block);
    }

    #[test]
    fn test_security_section_missing_falls_back_to_defaults() {
        // Consumer policy.json predating ADR-0008 has no `security` key; the
        // struct-level serde default must fill it with the strict baseline.
        let p: Policy = serde_json::from_str(r#"{"schema_version":1,"git":{}}"#).unwrap();
        assert_eq!(p.security.dangerous_commands, PolicyLevel::Block);
        assert_eq!(p.security.privilege_escalation, PolicyLevel::Block);
    }

    #[test]
    fn test_shipped_asset_security_matches_defaults() {
        let asset = include_str!("../../../../assets/base/policy.json");
        let from_asset: Policy = serde_json::from_str(asset).unwrap();
        // Every shipped security value is the built-in default, so a project
        // without the section is judged exactly as a fresh install.
        assert_eq!(
            serde_json::to_value(&from_asset.security).unwrap(),
            serde_json::to_value(SecuritySection::default()).unwrap()
        );
        assert_eq!(from_asset.git.discard_uncommitted, PolicyLevel::Block);
        assert_eq!(
            from_asset.git.clean_regenerable,
            GitPolicy::default().clean_regenerable
        );
        assert!(
            asset.contains("\"headless_peer_runs\": \"block\""),
            "the shipped policy states the headless peer level"
        );
    }

    #[test]
    fn test_policy_level_parse_all() {
        for (s, l) in [
            ("\"block\"", PolicyLevel::Block),
            ("\"warn\"", PolicyLevel::Warn),
            ("\"allow\"", PolicyLevel::Allow),
            ("\"off\"", PolicyLevel::Off),
        ] {
            let parsed: PolicyLevel = serde_json::from_str(s).unwrap();
            assert_eq!(parsed, l);
        }
    }

    #[test]
    fn test_policy_level_is_active() {
        assert!(PolicyLevel::Block.is_active());
        assert!(PolicyLevel::Warn.is_active());
        assert!(!PolicyLevel::Allow.is_active());
        assert!(!PolicyLevel::Off.is_active());
    }

    #[test]
    fn test_load_missing_returns_strict_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = Policy::load(dir.path());
        assert_eq!(p.git.commit_to_protected, PolicyLevel::Block);
    }

    #[test]
    fn test_load_malformed_returns_strict_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("policy.json"), "{ nope").unwrap();
        let p = Policy::load(dir.path());
        assert_eq!(p.git.push_to_protected, PolicyLevel::Block);
    }

    #[test]
    fn test_source_reports_file_malformed_and_absent() {
        // Mirrors the load_file fallback rules so callers can report honestly
        // which ruleset (project file vs charter defaults) is enforced.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Policy::source(dir.path()), PolicySource::Absent);
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("policy.json"), "{ nope").unwrap();
        assert_eq!(Policy::source(dir.path()), PolicySource::MalformedFile);
        std::fs::write(cf.join("policy.json"), r#"{"schema_version":1}"#).unwrap();
        assert_eq!(Policy::source(dir.path()), PolicySource::ProjectFile);
    }

    #[test]
    fn test_load_partial_keeps_defaults_for_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(
            cf.join("policy.json"),
            r#"{"schema_version":1,"git":{"protected_branches":["main","release/*"],"commit_format":"warn"}}"#,
        )
        .unwrap();
        let p = Policy::load(dir.path());
        assert_eq!(p.git.protected_branches, vec!["main", "release/*"]);
        assert_eq!(p.git.commit_format, PolicyLevel::Warn);
        // Untouched keys keep charter defaults.
        assert_eq!(p.git.push_to_protected, PolicyLevel::Block);
        assert_eq!(p.git.commit_types.len(), 10);
    }

    #[test]
    fn test_glob_extension_release_star_is_protected() {
        // AC #3: adding release/* to protected_branches is honored with no
        // code change.
        let g = GitPolicy {
            protected_branches: vec!["main".into(), "release/*".into()],
            ..GitPolicy::default()
        };
        assert!(g.branch_is_protected("main"));
        assert!(g.branch_is_protected("release/v2.0"));
        assert!(!g.branch_is_protected("feat/release-notes"));
    }

    #[test]
    fn test_branch_name_ok() {
        let g = GitPolicy::default();
        assert!(g.branch_name_ok("feat/v2-hooks"));
        assert!(g.branch_name_ok("spike/idea"));
        assert!(!g.branch_name_ok("my-cool-branch"));
        // Protected branches are exempt from naming.
        assert!(g.branch_name_ok("main"));
    }

    #[test]
    fn test_default_prefixes_include_integration() {
        // Body-of-work epics land their tasks on an `integration/<epic>` branch
        // (see cf-method, "Managing a body of work"), so the prefix must be a
        // sanctioned default in every plane.
        let g = GitPolicy::default();
        assert!(g.branch_prefixes.iter().any(|p| p == "integration/"));
        assert!(g.branch_name_ok("integration/ep-12-new-flow"));
    }

    #[test]
    fn test_default_prefixes_include_durable_tasks() {
        let g = GitPolicy::default();
        assert!(g.branch_prefixes.iter().any(|p| p == "task/"));
        assert!(g.branch_name_ok("task/TSK-014-account-recovery"));
    }

    #[test]
    fn test_policy_armed_defaults_true() {
        let dir = tempfile::tempdir().unwrap();
        assert!(policy_armed(dir.path()));
    }

    #[test]
    fn test_policy_armed_false_during_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(
            cf.join("project.toml"),
            "schema_version = 1\npolicy_armed = false\n",
        )
        .unwrap();
        assert!(!policy_armed(dir.path()));
    }

    #[test]
    fn test_load_effective_grace_keeps_secret_scan() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("project.toml"), "policy_armed = false\n").unwrap();
        let (policy, armed) = Policy::load_effective(dir.path());
        assert!(!armed);
        assert_eq!(policy.git.commit_to_protected, PolicyLevel::Off);
        assert_eq!(policy.git.push_to_protected, PolicyLevel::Off);
        assert_eq!(policy.git.merge_to_protected, PolicyLevel::Off);
        assert_eq!(policy.git.pr_merge_to_protected, PolicyLevel::Off);
        assert_eq!(policy.git.local_ref_protection, PolicyLevel::Off);
        assert_eq!(policy.git.commit_format, PolicyLevel::Off);
        assert_eq!(policy.git.commit_body, PolicyLevel::Off);
        // The security / red-team gates are advisory (warn) defaults, so — like
        // test_gate_on_push — bootstrap grace suspends them (ADR-0016).
        assert_eq!(policy.git.security_review, PolicyLevel::Off);
        assert_eq!(policy.git.dep_audit, PolicyLevel::Off);
        // Secrets are never graced (charter §6.3).
        assert_eq!(policy.git.secret_scan, PolicyLevel::Block);
    }

    #[test]
    fn test_policy_with_the_removed_human_authorization_key_still_loads() {
        // TSK-137: the key is gone from the schema; an older file that still
        // carries it loads (serde ignores it) instead of failing every hook.
        let p: Policy = serde_json::from_str(
            r#"{"human_authorization":"none","git":{"commit_format":"warn"}}"#,
        )
        .unwrap();
        assert_eq!(p.git.commit_format, PolicyLevel::Warn);
    }

    #[test]
    fn test_load_effective_armed_is_strict() {
        let dir = tempfile::tempdir().unwrap();
        let (policy, armed) = Policy::load_effective(dir.path());
        assert!(armed);
        assert_eq!(policy.git.commit_to_protected, PolicyLevel::Block);
    }

    /// Make a real one-commit git repo at `dir` and write a project.toml with
    /// the given `policy_armed` value.
    fn committed_repo_with_armed(dir: &Path, armed: bool) {
        let run = |args: &[&str]| {
            let ok = crate::git::command()
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git runs")
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "chore: init"]);
        let cf = dir.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(
            cf.join("project.toml"),
            format!("schema_version = 1\npolicy_armed = {armed}\n"),
        )
        .unwrap();
    }

    #[test]
    fn test_load_effective_disarm_flag_is_inert_after_first_commit() {
        // ADR-0009: policy_armed=false is not a persistent off-switch. In a
        // repo that already has a (scaffold) commit, the disarmed flag is
        // ignored and rules stay armed.
        let dir = tempfile::tempdir().unwrap();
        committed_repo_with_armed(dir.path(), false);
        let (policy, armed) = Policy::load_effective(dir.path());
        assert!(
            armed,
            "a committed repo is armed despite policy_armed=false"
        );
        assert_eq!(policy.git.commit_to_protected, PolicyLevel::Block);
        assert_eq!(policy.git.hook_integrity, PolicyLevel::Block);
    }

    #[test]
    fn test_load_effective_grace_only_before_first_commit() {
        // Grace is still honored in the genuine pre-scaffold-commit window: a
        // repo with an unborn HEAD (no commit yet) and policy_armed=false is
        // graced, so the first scaffold commit is not walled.
        let dir = tempfile::tempdir().unwrap();
        crate::git::command()
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("project.toml"), "policy_armed = false\n").unwrap();
        let (policy, armed) = Policy::load_effective(dir.path());
        assert!(!armed, "pre-first-commit bootstrap window is graced");
        assert_eq!(policy.git.commit_to_protected, PolicyLevel::Off);
        // Secrets are never graced, even in the bootstrap window.
        assert_eq!(policy.git.secret_scan, PolicyLevel::Block);
    }

    #[test]
    fn test_hook_integrity_default_block_and_suspended_by_grace() {
        assert_eq!(GitPolicy::default().hook_integrity, PolicyLevel::Block);
        let mut g = GitPolicy::default();
        g.suspend_for_bootstrap();
        assert_eq!(g.hook_integrity, PolicyLevel::Off);
    }

    #[test]
    fn test_conflict_markers_default_block_and_suspended_by_grace() {
        // TSK-170 AC-5.
        assert_eq!(GitPolicy::default().conflict_markers, PolicyLevel::Block);
        let mut g = GitPolicy::default();
        g.suspend_for_bootstrap();
        assert_eq!(g.conflict_markers, PolicyLevel::Off);
        let shipped: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/base/policy.json")).unwrap();
        assert_eq!(shipped["git"]["conflict_markers"], "block");
    }

    #[test]
    fn test_default_base_branch_skips_globs() {
        let g = GitPolicy {
            protected_branches: vec!["release/*".into(), "main".into()],
            ..GitPolicy::default()
        };
        assert_eq!(g.default_base_branch(), Some("main"));
    }
}
