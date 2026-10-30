//! `codeflow remote protect` — apply branch protection to the remote
//! provider as rulesets this command owns (charter §6.1 plane 3, D7, D19;
//! TSK-1080).
//!
//! The remote is part of the authoritative enforcement perimeter. The
//! command writes three rulesets it owns by name, for a profile, a check
//! identity and a line pattern:
//!
//! - **A, "codeflow authority updates"**, on `~DEFAULT_BRANCH`: `update`, so
//!   only one repository role (the administrator under
//!   `solo-administrator`, the maintain role under `independent-review`)
//!   moves the default branch, merges included.
//! - **B, "codeflow authority checks"**, on `~DEFAULT_BRANCH`: a pull
//!   request with the profile's approvals, strict required checks each
//!   pinned to the identity that must produce it (the GitHub App given by
//!   `--check-app` for the `authority:` names, else GitHub Actions for the
//!   `trusted:` names), no force push, no deletion, no bypass.
//! - **C, "codeflow line checks"**, on the line pattern (`--lines`, default
//!   `integration/**`): a pull request, the `trusted:` names and the test
//!   verdicts pinned to GitHub Actions and strict, no force push, no
//!   deletion, no bypass, and no creation rule.
//!
//! Each other pattern in `git.protected_branches` gets its own ruleset,
//! `codeflow protect <pattern>`, as does the registry's data profile.
//!
//! Ownership is by exact name. A ruleset with any other name is never
//! written. Every write sends the whole body (`name`, `target`,
//! `enforcement`, `conditions`, `bypass_actors`, `rules`), because GitHub
//! does not document what an update does with omitted fields. An update
//! only adds: an approval count is never lowered, a rule is never removed,
//! a check's producer is never changed, and a bypass list is never
//! widened. B's and C's bypass lists are written empty, which closes a
//! recovery window an administrator opened on the host. A reduction is the
//! administrator's edit on the host; there is no command for it.
//!
//! B and C are refused while the default branch's policy workflow does not
//! declare the names they require, since a rule that requires a name
//! nothing produces blocks every pull request.
//!
//! Legible degradation is the contract here (charter principle 8, AC #4):
//! anything the provider plan cannot apply — the canonical case being a
//! private repo on a GitHub Free plan returning HTTP 403 "Upgrade to GitHub
//! Pro" — is detected and reported precisely, with a manual checklist, and
//! the command exits successfully with status `degraded` instead of failing.
//!
//! Providers are pluggable behind [`RemoteProvider`]; only GitHub (via the
//! `gh` CLI) is implemented. Other providers get the printed manual
//! checklist until an adapter is warranted (charter D18).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::hooks::policy::DEFAULT_REQUIRED_CHECKS;

mod fnmatch;

/// The GitHub App id of GitHub Actions. Ruleset status checks pin it, so a
/// required check passes only when GitHub Actions reported it, as the
/// shipped workflows do; a check without an integration id would accept
/// the same context from any source.
pub const GITHUB_ACTIONS_APP_ID: u64 = 15368;

/// Ruleset A's name: only one role updates the default branch.
pub const RULESET_AUTHORITY_UPDATES: &str = "codeflow authority updates";
/// Ruleset B's name: the default branch's pull request and checks.
pub const RULESET_AUTHORITY_CHECKS: &str = "codeflow authority checks";
/// Ruleset C's name: the integration lines' pull request and checks.
pub const RULESET_LINE_CHECKS: &str = "codeflow line checks";

/// The line pattern C targets when `--lines` is not given.
pub const DEFAULT_LINES: &str = "integration/**";

/// The policy workflow on the default branch, which declares the names B
/// and C require.
pub const POLICY_WORKFLOW: &str = ".github/workflows/codeflow-policy.yml";

/// The prefix of the policy verdict job names the policy workflow runs from
/// the default branch's file for every pull request.
pub const TRUSTED_PREFIX: &str = "trusted: ";

/// The prefix of the check runs the GitHub App creates in Mode A.
pub const AUTHORITY_PREFIX: &str = "authority: ";

/// The `trusted:` names B and C cannot do without; `trusted: release
/// impact` joins them where the workflow declares it.
const REQUIRED_TRUSTED: &[&str] = &["trusted: commit standards", "trusted: secret scan"];

/// The test verdicts of head code B requires, each where a workflow on the
/// default branch declares it; `codeflow gates` is required outright.
const DEFAULT_BRANCH_TEST_VERDICTS: &[&str] = &["codeflow gates", "security review", "windows"];

/// The test verdicts C requires, each where a workflow on the default
/// branch declares it; `codeflow gates` is required outright.
const LINE_TEST_VERDICTS: &[&str] = &["codeflow gates", "windows"];

/// The test verdict every pull request needs.
const REQUIRED_TEST_VERDICT: &str = "codeflow gates";

/// Rulesets read per page.
const RULESET_PAGE_SIZE: usize = 100;

/// The repository role ids GitHub uses for `RepositoryRole` bypass actors.
/// GitHub's REST reference does not list them; these are the values its
/// API is reported to use, and the hosted fixture confirms them (TSK-1080
/// Findings).
pub const ADMIN_ROLE_ID: u64 = 5;
/// The maintain role's id; see [`ADMIN_ROLE_ID`].
pub const MAINTAIN_ROLE_ID: u64 = 2;

/// Outcome status of a protect run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectStatus {
    /// Every rule applied.
    Applied,
    /// Some or all rules could not be applied; limitations and a manual
    /// checklist are reported. Not a failure.
    Degraded,
    /// Plan printed, nothing touched.
    DryRun,
}

impl ProtectStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Degraded => "degraded",
            Self::DryRun => "dry-run",
        }
    }
}

/// Report from a protect run — everything the user needs to see, in order.
#[derive(Debug, Clone)]
pub struct ProtectReport {
    pub status: ProtectStatus,
    /// What happened (or would happen), rule by rule.
    pub lines: Vec<String>,
    /// Precise descriptions of what could not be applied and why.
    pub limitations: Vec<String>,
    /// Manual steps covering everything in `limitations`.
    pub checklist: Vec<String>,
}

impl ProtectReport {
    /// Render the report for terminal output.
    #[must_use]
    pub fn render(&self) -> crate::git::TextBlock {
        // Lines quote remote rule names and API messages: each `writeln!`
        // is one line under the block rule (TSK-388).
        let mut out = crate::git::TextBlock::new();
        for line in &self.lines {
            let _ = writeln!(out, "{line}");
        }
        if !self.limitations.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "Not applied:");
            for l in &self.limitations {
                let _ = writeln!(out, "  - {l}");
            }
        }
        if !self.checklist.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "Manual checklist (GitHub: Settings -> Rules -> Rulesets):"
            );
            for c in &self.checklist {
                let _ = writeln!(out, "  {c}");
            }
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "status: {}", self.status.as_str());
        out
    }
}

// ---------------------------------------------------------------------------
// Profiles and the authority plan
// ---------------------------------------------------------------------------

/// Who reviews and who moves the default branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// One human administrator: zero approvals; only the administrator
    /// role updates the default branch.
    SoloAdministrator,
    /// A second human reviews: one approval, stale reviews dismissed, the
    /// last push approved by someone else; the maintain role merges.
    IndependentReview,
}

impl Profile {
    /// The profile names, the default first.
    pub const NAMES: &'static [&'static str] = &["solo-administrator", "independent-review"];

    /// Read a profile name.
    ///
    /// # Errors
    ///
    /// Returns an error naming the accepted names for any other text.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "solo-administrator" => Ok(Self::SoloAdministrator),
            "independent-review" => Ok(Self::IndependentReview),
            other => Err(format!(
                "unknown profile '{other}'; use solo-administrator or independent-review"
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SoloAdministrator => "solo-administrator",
            Self::IndependentReview => "independent-review",
        }
    }

    /// The repository role that alone updates the default branch, as its
    /// id and name.
    #[must_use]
    pub fn update_role(self) -> (u64, &'static str) {
        match self {
            Self::SoloAdministrator => (ADMIN_ROLE_ID, "administrator"),
            Self::IndependentReview => (MAINTAIN_ROLE_ID, "maintain"),
        }
    }

    /// The approvals B requires.
    #[must_use]
    pub fn approvals(self) -> u64 {
        match self {
            Self::SoloAdministrator => 0,
            Self::IndependentReview => 1,
        }
    }

    /// A's bypass actor for this profile.
    #[must_use]
    pub fn bypass_actor(self) -> Value {
        json!({
            "actor_id": self.update_role().0,
            "actor_type": "RepositoryRole",
            "bypass_mode": "always",
        })
    }
}

/// The profile, check identity and line pattern rulesets A, B and C are
/// written for.
#[derive(Debug, Clone)]
pub struct AuthorityPlan {
    pub profile: Profile,
    /// The GitHub App that creates the `authority:` checks (Mode A), or
    /// none for GitHub Actions and the `trusted:` names (Mode B).
    pub check_app: Option<u64>,
    /// The integration-line pattern C targets, below `refs/heads/`.
    pub lines: String,
}

impl AuthorityPlan {
    /// Build the plan, refusing a check app or line pattern that cannot be
    /// right.
    ///
    /// # Errors
    ///
    /// Returns an error for app id 0 or GitHub Actions' own id, and for an
    /// empty line pattern or one that is not a branch pattern.
    pub fn new(profile: Profile, check_app: Option<u64>, lines: &str) -> Result<Self, String> {
        match check_app {
            Some(0) => return Err("--check-app must be a GitHub App id, not 0".into()),
            Some(GITHUB_ACTIONS_APP_ID) => {
                return Err(format!(
                    "--check-app {GITHUB_ACTIONS_APP_ID} is GitHub Actions, which any workflow can use; give the id of the App that creates the authority checks, or leave --check-app out"
                ))
            }
            _ => {}
        }
        let lines = lines.trim();
        if lines.is_empty() {
            return Err("--lines must name a branch pattern such as integration/**".into());
        }
        if lines.starts_with("refs/") || lines.starts_with('~') {
            return Err(format!(
                "--lines '{lines}' must be a branch pattern below refs/heads/, such as integration/**"
            ));
        }
        fnmatch::matches(&format!("refs/heads/{lines}"), "refs/heads/x")
            .map_err(|why| format!("--lines '{lines}' {why}"))?;
        Ok(Self {
            profile,
            check_app,
            lines: lines.to_string(),
        })
    }

    /// The check identity in words.
    #[must_use]
    pub fn mode(&self) -> String {
        match self.check_app {
            Some(app) => format!("Mode A, the authority: names pinned to GitHub App {app}"),
            None => format!(
                "Mode B, the trusted: names pinned to GitHub Actions ({GITHUB_ACTIONS_APP_ID})"
            ),
        }
    }

    /// What A, B and C require, ruleset by ruleset, for the dry run and
    /// the manual checklist, without reading the host.
    fn intents(&self) -> Vec<(String, Vec<String>)> {
        let (role_id, role) = self.profile.update_role();
        let checks = match self.check_app {
            Some(app) => format!(
                "the authority: names the policy workflow creates, pinned to GitHub App {app}, and the declared test verdicts ({}), pinned to GitHub Actions",
                DEFAULT_BRANCH_TEST_VERDICTS.join(", ")
            ),
            None => format!(
                "the trusted: names the policy workflow declares and the declared test verdicts ({}), pinned to GitHub Actions",
                DEFAULT_BRANCH_TEST_VERDICTS.join(", ")
            ),
        };
        let approvals = match self.profile {
            Profile::SoloAdministrator => "a pull request, no approval required".to_string(),
            Profile::IndependentReview => {
                "a pull request with one approval, stale approvals dismissed and the last push approved by someone else".to_string()
            }
        };
        vec![
            (
                format!("{RULESET_AUTHORITY_UPDATES} on ~DEFAULT_BRANCH"),
                vec![format!(
                    "only the {role} role (RepositoryRole {role_id}) updates the branch, merges included"
                )],
            ),
            (
                format!("{RULESET_AUTHORITY_CHECKS} on ~DEFAULT_BRANCH"),
                vec![
                    approvals,
                    format!("strict status checks on an up-to-date branch: {checks}"),
                    "block force pushes".into(),
                    "block branch deletion".into(),
                    "no bypass".into(),
                ],
            ),
            (
                format!("{RULESET_LINE_CHECKS} on refs/heads/{}", self.lines),
                vec![
                    "a pull request, no approval required".into(),
                    format!(
                        "strict status checks on an up-to-date branch: the trusted: names the policy workflow declares and the declared test verdicts ({}), pinned to GitHub Actions",
                        LINE_TEST_VERDICTS.join(", ")
                    ),
                    "block force pushes".into(),
                    "block branch deletion".into(),
                    "no bypass".into(),
                    "creating a line stays allowed".into(),
                ],
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// Plan (from policy.json)
// ---------------------------------------------------------------------------

/// Protection intent for one branch pattern.
// The four flags are independent policy intents read straight from
// policy.json — a state machine here would be ceremony.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct BranchRule {
    /// Branch name or glob pattern (e.g. `main`, `release/*`).
    pub pattern: String,
    pub require_pr: bool,
    pub require_status_checks: bool,
    pub block_force_push: bool,
    pub block_deletion: bool,
    /// The registry data profile (SPC-013 R-6, R-22): it holds before the
    /// branch exists and never touches `main`.
    pub data_profile: bool,
    /// The status check names required, on an up-to-date branch, when
    /// [`require_status_checks`](BranchRule::require_status_checks) holds.
    pub required_checks: Vec<String>,
}

impl BranchRule {
    /// Whether the pattern is a glob rather than one branch name.
    #[must_use]
    pub fn is_glob(&self) -> bool {
        self.pattern.contains(['*', '?', '['])
    }

    /// The data profile for `codeflow/registry`: no deletion and no force
    /// push; no pull-request requirement and no status checks.
    #[must_use]
    pub fn registry_data_profile() -> BranchRule {
        BranchRule {
            pattern: crate::ids::REGISTRY_BRANCH.to_string(),
            require_pr: false,
            require_status_checks: false,
            block_force_push: true,
            block_deletion: true,
            data_profile: true,
            required_checks: Vec::new(),
        }
    }

    /// Whether the rule requires named status checks.
    #[must_use]
    pub fn requires_checks(&self) -> bool {
        self.require_status_checks && !self.required_checks.is_empty()
    }

    /// The name of the ruleset this command owns for the pattern.
    #[must_use]
    pub fn ruleset_name(&self) -> String {
        format!("codeflow protect {}", self.pattern)
    }

    fn intent_lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.data_profile {
            v.push("data profile: no pull request and no status checks".to_string());
        }
        if self.require_pr {
            v.push("require a pull request before merging".to_string());
        }
        if self.requires_checks() {
            v.push(format!(
                "require status checks to pass on a branch that is up to date with its base: {}",
                self.required_checks.join(", ")
            ));
        }
        if self.block_force_push {
            v.push("block force pushes".to_string());
        }
        if self.block_deletion {
            v.push("block branch deletion".to_string());
        }
        v
    }

    /// The ruleset rules the policy intends for the pattern.
    fn rules(&self) -> Vec<Value> {
        let mut rules = Vec::new();
        if self.require_pr {
            rules.push(pull_request_rule(0, false));
        }
        if self.requires_checks() {
            rules.push(status_checks_rule(
                self.required_checks
                    .iter()
                    .map(|name| (name.clone(), GITHUB_ACTIONS_APP_ID)),
            ));
        }
        if self.block_force_push {
            rules.push(json!({ "type": "non_fast_forward" }));
        }
        if self.block_deletion {
            rules.push(json!({ "type": "deletion" }));
        }
        rules
    }
}

/// The full protection plan derived from `policy.json`, with the authority
/// rulesets when a profile is given.
#[derive(Debug, Clone)]
pub struct ProtectionPlan {
    pub rules: Vec<BranchRule>,
    /// Rulesets A, B and C; `None` writes only the policy patterns.
    pub authority: Option<AuthorityPlan>,
}

impl ProtectionPlan {
    /// Whether the policy file at `path` is proven missing, so the plan
    /// uses the charter defaults; a dangling link or an unreadable ancestor
    /// is an error, never absence.
    ///
    /// # Errors
    ///
    /// Returns an error when the path's presence cannot be read.
    pub fn policy_absent(path: &Path) -> Result<bool, String> {
        crate::absence::proven_absent(path)
            .map_err(|error| format!("cannot read policy {}: {error}", path.display()))
    }

    /// Build the plan from a `policy.json` file. Reads
    /// `git.protected_branches` (falling back to a top-level
    /// `protected_branches`, then to `main`/`master`) and derives rule
    /// strictness from the corresponding `git.*` block/warn/allow values
    /// (a missing file or value defaults to the charter's strict baseline).
    ///
    /// # Errors
    ///
    /// Returns an error when the policy cannot be read or its protection fields are invalid.
    pub fn from_policy_file(path: &Path) -> Result<Self, String> {
        let unreadable = |error| format!("cannot read policy {}: {error}", path.display());
        let policy: serde_json::Value = if Self::policy_absent(path)? {
            serde_json::json!({})
        } else {
            let bytes = std::fs::read(path).map_err(unreadable)?;
            serde_json::from_slice(&bytes).map_err(|error| {
                format!(
                    "cannot parse policy {}: {}",
                    path.display(),
                    crate::scaffold::json_error(&error)
                )
            })?
        };
        if !policy.is_object() {
            return Err("policy must be an object".into());
        }
        let git = match policy.get("git") {
            Some(value) if !value.is_object() => return Err("policy.git must be an object".into()),
            value => value,
        };
        let branches = match git
            .and_then(|git| git.get("protected_branches"))
            .or_else(|| policy.get("protected_branches"))
        {
            None => vec!["main".to_string(), "master".to_string()],
            Some(value) => value
                .as_array()
                .ok_or("protected_branches must be an array")?
                .iter()
                .map(|branch| {
                    branch
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "protected_branches entries must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let blocks = |key: &str| -> Result<bool, String> {
            match git.and_then(|git| git.get(key)) {
                None => Ok(true),
                Some(value) => match value.as_str() {
                    Some("block") => Ok(true),
                    Some("warn" | "allow") => Ok(false),
                    _ => Err(format!("policy.git.{key} must be block, warn or allow")),
                },
            }
        };
        let force = blocks("force_push_protected")?;
        let delete = blocks("delete_protected")?;
        let push = blocks("push_to_protected")?;
        // Read as the schema reads it: absent gives the shipped CI job
        // names; a list that is not strings, is empty or holds a blank name
        // is refused, never replaced by the defaults.
        let required_checks: Vec<String> = match git.and_then(|git| git.get("required_checks")) {
            None => DEFAULT_REQUIRED_CHECKS
                .iter()
                .map(ToString::to_string)
                .collect(),
            Some(value) => {
                let names = value
                    .as_array()
                    .ok_or("policy.git.required_checks must be an array")?
                    .iter()
                    .map(|name| match name.as_str() {
                        Some(name) if !name.trim().is_empty() => Ok(name.to_string()),
                        _ => Err(
                            "policy.git.required_checks entries must be non-blank strings"
                                .to_string(),
                        ),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if names.is_empty() {
                    return Err("policy.git.required_checks must name at least one check".into());
                }
                names
            }
        };

        let rules = branches
            .into_iter()
            .map(|pattern| BranchRule {
                pattern,
                require_pr: push,
                require_status_checks: push,
                block_force_push: force,
                block_deletion: delete,
                data_profile: false,
                required_checks: required_checks.clone(),
            })
            .collect();
        Ok(Self {
            rules,
            authority: None,
        })
    }

    /// Add the registry data profile when the repository tracks durable
    /// work; the rules for every other branch are unchanged.
    #[must_use]
    pub fn with_registry_profile(mut self) -> Self {
        self.rules.push(BranchRule::registry_data_profile());
        self
    }

    /// Add rulesets A, B and C for `authority`.
    #[must_use]
    pub fn with_authority(mut self, authority: AuthorityPlan) -> Self {
        self.authority = Some(authority);
        self
    }

    /// The policy patterns that get a `codeflow protect <pattern>` ruleset:
    /// with the authority rulesets, the default branch and the line
    /// pattern are theirs.
    fn pattern_rules(&self, default_branch: Option<&str>) -> Vec<&BranchRule> {
        self.rules
            .iter()
            .filter(|rule| match &self.authority {
                None => true,
                Some(authority) => {
                    rule.pattern != authority.lines && Some(rule.pattern.as_str()) != default_branch
                }
            })
            .collect()
    }

    /// Render the dry-run report: the intended rules, nothing applied.
    #[must_use]
    pub fn dry_run_report(&self, provider: &str) -> ProtectReport {
        let mut lines = vec![format!(
            "dry-run: intended remote protection ({provider}), nothing applied"
        )];
        if let Some(authority) = &self.authority {
            lines.push(format!(
                "profile {}; {}",
                authority.profile.as_str(),
                authority.mode()
            ));
            for (ruleset, intents) in authority.intents() {
                lines.push(format!("ruleset {ruleset}:"));
                for intent in intents {
                    lines.push(format!("  - {intent}"));
                }
            }
            lines.push(format!(
                "a policy pattern that names the default branch gets no ruleset of its own: {RULESET_AUTHORITY_UPDATES} and {RULESET_AUTHORITY_CHECKS} protect it, and the dry run reads no host to know its name"
            ));
        }
        for rule in self.pattern_rules(None) {
            // The pattern is policy text: a fixed label starts the line.
            lines.push(format!("ruleset {}:", rule.ruleset_name()));
            for intent in rule.intent_lines() {
                lines.push(format!("  - {intent}"));
            }
        }
        lines.push(
            "only rulesets with these names are written; every other ruleset is left as it is, and an update never lowers an approval count, removes a rule or widens a bypass list"
                .to_string(),
        );
        ProtectReport {
            status: ProtectStatus::DryRun,
            lines,
            limitations: Vec::new(),
            checklist: Vec::new(),
        }
    }

    /// Manual checklist covering the whole plan.
    #[must_use]
    pub fn manual_checklist(&self) -> Vec<String> {
        let mut items = Vec::new();
        if let Some(authority) = &self.authority {
            for (ruleset, intents) in authority.intents() {
                for intent in intents {
                    items.push(format!("[ ] {ruleset}: {intent}"));
                }
            }
        }
        for rule in self.pattern_rules(None) {
            for intent in rule.intent_lines() {
                items.push(format!("[ ] {}: {intent}", rule.ruleset_name()));
            }
        }
        items
    }
}

/// A `pull_request` rule.
fn pull_request_rule(approvals: u64, independent: bool) -> Value {
    json!({
        "type": "pull_request",
        "parameters": {
            "required_approving_review_count": approvals,
            "dismiss_stale_reviews_on_push": independent,
            "require_code_owner_review": false,
            "require_last_push_approval": independent,
            "required_review_thread_resolution": false
        }
    })
}

/// A strict `required_status_checks` rule, each name pinned to its app.
fn status_checks_rule(checks: impl IntoIterator<Item = (String, u64)>) -> Value {
    json!({
        "type": "required_status_checks",
        "parameters": {
            "strict_required_status_checks_policy": true,
            "do_not_enforce_on_create": false,
            "required_status_checks": checks
                .into_iter()
                .map(|(context, app)| json!({ "context": context, "integration_id": app }))
                .collect::<Vec<_>>()
        }
    })
}

// ---------------------------------------------------------------------------
// What the default branch declares
// ---------------------------------------------------------------------------

/// The check names the workflows on the default branch declare.
#[derive(Debug, Clone, Default)]
pub struct Declared {
    /// Whether the policy workflow exists on the default branch.
    pub policy_workflow: bool,
    /// The policy workflow's text.
    pub policy_text: String,
    /// Every literal job name (or id, for a job without a name) of every
    /// workflow on the default branch.
    pub job_names: BTreeSet<String>,
    /// `(file, job id)` for each job that references an environment, with
    /// the environment as written.
    pub environments: Vec<(String, String, String)>,
}

impl Declared {
    /// The `trusted:` names the policy workflow declares as job names.
    #[must_use]
    pub fn trusted_names(&self) -> Vec<String> {
        workflow_job_names(&self.policy_text)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| name.starts_with(TRUSTED_PREFIX))
            .collect()
    }

    /// Whether a workflow on the default branch produces `name`: a literal
    /// job name, or an `authority:` name the policy workflow writes.
    #[must_use]
    pub fn produces(&self, name: &str) -> bool {
        self.job_names.contains(name)
            || (name.starts_with(AUTHORITY_PREFIX) && self.policy_text.contains(name))
    }
}

/// The literal job names of a workflow, a job without `name` counting by
/// its id; a name built from an expression is not a declaration.
///
/// # Errors
///
/// Returns an error when the text is not a YAML workflow.
pub fn workflow_job_names(text: &str) -> Result<BTreeSet<String>, String> {
    let jobs = workflow_jobs(text)?;
    Ok(jobs
        .iter()
        .filter_map(|(id, job)| match job.get("name") {
            None => Some(id.clone()),
            Some(serde_yaml::Value::String(name)) => Some(name.clone()),
            Some(_) => None,
        })
        .filter(|name| !name.contains("${{"))
        .collect())
}

/// `(job id, environment)` for each job of a workflow that references an
/// environment, as a string or a `name`; an expression stays as written.
///
/// # Errors
///
/// Returns an error when the text is not a YAML workflow.
pub fn workflow_environments(text: &str) -> Result<Vec<(String, String)>, String> {
    let jobs = workflow_jobs(text)?;
    Ok(jobs
        .iter()
        .filter_map(|(id, job)| {
            let environment = job.get("environment")?;
            let name = match environment {
                serde_yaml::Value::String(name) => name.clone(),
                serde_yaml::Value::Mapping(map) => match map.get("name") {
                    Some(serde_yaml::Value::String(name)) => name.clone(),
                    _ => "(unnamed)".to_string(),
                },
                _ => "(unreadable)".to_string(),
            };
            Some((id.clone(), name))
        })
        .collect())
}

fn workflow_jobs(text: &str) -> Result<Vec<(String, serde_yaml::Mapping)>, String> {
    let doc: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|error| format!("not YAML: {error}"))?;
    let Some(jobs) = doc.get("jobs") else {
        return Ok(Vec::new());
    };
    let jobs = jobs.as_mapping().ok_or("`jobs` is not a mapping")?;
    jobs.iter()
        .map(|(id, job)| {
            let id = id.as_str().ok_or("a job id is not a string")?.to_string();
            let job = job
                .as_mapping()
                .cloned()
                .ok_or_else(|| format!("job {id} is not a mapping"))?;
            Ok((id, job))
        })
        .collect()
}

/// A text for a URL query value: unreserved characters and `/` stay, every
/// other byte is percent-encoded.
fn query_value(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Read what the workflows on `branch` declare, through `gh` answers. A
/// missing workflow folder declares nothing; any other failure is an error,
/// never an empty declaration.
///
/// # Errors
///
/// Returns an error when the folder or a workflow cannot be read or parsed.
pub fn read_declared(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
) -> Result<Declared, String> {
    let reference = query_value(branch);
    let listing = match gh(&[
        "api",
        &format!("repos/{nwo}/contents/.github/workflows?ref={reference}"),
    ]) {
        Ok(text) => text,
        Err(error) if error.contains("404") || error.contains("Not Found") => {
            return Ok(Declared::default())
        }
        Err(error) => {
            return Err(format!(
                "the workflows on {branch} are not readable ({})",
                first_line(&error)
            ))
        }
    };
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&listing) else {
        return Err(format!(
            "the workflow folder on {branch} is not a JSON list"
        ));
    };
    let mut declared = Declared::default();
    for entry in &entries {
        let Some(path) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("file")
            || !Path::new(path).extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
            })
        {
            continue;
        }
        let text = gh(&[
            "api",
            "-H",
            "Accept: application/vnd.github.raw",
            &format!("repos/{nwo}/contents/{path}?ref={reference}"),
        ])
        .map_err(|error| {
            format!(
                "{path} on {branch} is not readable ({})",
                first_line(&error)
            )
        })?;
        let names =
            workflow_job_names(&text).map_err(|why| format!("{path} on {branch}: {why}"))?;
        for (job, environment) in
            workflow_environments(&text).map_err(|why| format!("{path} on {branch}: {why}"))?
        {
            declared
                .environments
                .push((path.to_string(), job, environment));
        }
        if path == POLICY_WORKFLOW {
            declared.policy_workflow = true;
            declared.policy_text.clone_from(&text);
        }
        declared.job_names.extend(names);
    }
    Ok(declared)
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text).trim()
}

/// The checks B and C require, from what the default branch declares, or
/// why the ruleset cannot be written.
/// The checks a ruleset requires, each with the app pinned to it, or why
/// the ruleset cannot be written.
type PlannedChecks = Result<Vec<(String, u64)>, String>;

fn planned_checks(
    authority: &AuthorityPlan,
    declared: &Declared,
) -> (PlannedChecks, PlannedChecks) {
    if !declared.policy_workflow {
        let why = format!(
            "{POLICY_WORKFLOW} is not on the default branch, so no workflow declares the trusted: names this ruleset requires"
        );
        return (Err(why.clone()), Err(why));
    }
    let trusted = declared.trusted_names();
    let missing: Vec<&str> = REQUIRED_TRUSTED
        .iter()
        .copied()
        .filter(|name| !trusted.iter().any(|t| t == name))
        .collect();
    let gates = declared.job_names.contains(REQUIRED_TEST_VERDICT);
    let mut why = Vec::new();
    if !missing.is_empty() {
        why.push(format!(
            "the policy workflow on the default branch does not declare {}",
            missing.join(", ")
        ));
    }
    if !gates {
        why.push(format!(
            "no workflow on the default branch declares {REQUIRED_TEST_VERDICT}"
        ));
    }
    if !why.is_empty() {
        let why = format!(
            "{}; a rule that requires a name nothing produces would block every pull request",
            why.join(" and ")
        );
        return (Err(why.clone()), Err(why));
    }
    let tests = |names: &[&str]| -> Vec<(String, u64)> {
        names
            .iter()
            .filter(|name| declared.job_names.contains(**name))
            .map(|name| ((*name).to_string(), GITHUB_ACTIONS_APP_ID))
            .collect()
    };
    let pinned_trusted: Vec<(String, u64)> = trusted
        .iter()
        .map(|name| (name.clone(), GITHUB_ACTIONS_APP_ID))
        .collect();
    let default_branch = match authority.check_app {
        None => Ok(pinned_trusted.clone()),
        Some(app) => {
            let names: Vec<String> = trusted
                .iter()
                .map(|name| format!("{AUTHORITY_PREFIX}{}", &name[TRUSTED_PREFIX.len()..]))
                .collect();
            let absent: Vec<&str> = names
                .iter()
                .filter(|name| !declared.policy_text.contains(name.as_str()))
                .map(String::as_str)
                .collect();
            if absent.is_empty() {
                Ok(names.into_iter().map(|name| (name, app)).collect())
            } else {
                Err(format!(
                    "the policy workflow on the default branch does not create {}, so --check-app {app} would require checks nothing produces",
                    absent.join(", ")
                ))
            }
        }
    }
    .map(|mut checks: Vec<(String, u64)>| {
        checks.extend(tests(DEFAULT_BRANCH_TEST_VERDICTS));
        checks
    });
    let mut line = pinned_trusted;
    line.extend(tests(LINE_TEST_VERDICTS));
    (default_branch, Ok(line))
}

// ---------------------------------------------------------------------------
// Owned rulesets: the intended body, and the update that only adds
// ---------------------------------------------------------------------------

/// How an owned ruleset's bypass list is written.
#[derive(Debug, Clone)]
enum BypassIntent {
    /// Exactly this list, on create and on update (B and C: empty).
    Exactly(Vec<Value>),
    /// This list on create; on update the live list is kept as it is, so
    /// it is never widened (A, and the policy patterns' rulesets).
    OnCreate(Vec<Value>),
}

/// One ruleset this command owns by name.
#[derive(Debug, Clone)]
struct Owned {
    name: String,
    /// What the report calls the target.
    target: String,
    include: Vec<String>,
    rules: Vec<Value>,
    bypass: BypassIntent,
    /// B and C never carry `update`.
    no_update_rule: bool,
}

impl Owned {
    fn create_body(&self) -> Value {
        let bypass = match &self.bypass {
            BypassIntent::Exactly(list) | BypassIntent::OnCreate(list) => list.clone(),
        };
        json!({
            "name": self.name,
            "target": "branch",
            "enforcement": "active",
            "conditions": { "ref_name": { "include": self.include, "exclude": [] } },
            "bypass_actors": bypass,
            "rules": self.rules,
        })
    }

    /// The full body that brings `existing` up to this ruleset, adding
    /// only: includes are joined and excludes dropped (both widen what it
    /// covers), it is made active, a missing rule is added, an approval
    /// count is raised and never lowered, a review flag is turned on and
    /// never off, checks become strict and gain the missing names, and a
    /// bypass list is replaced only where `bypass` says exactly. A change
    /// that would weaken a rule or change a check's producer is refused
    /// with why.
    fn update_body(&self, existing: &Value) -> Result<Value, String> {
        let mut include: Vec<String> = existing
            .pointer("/conditions/ref_name/include")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        for entry in &self.include {
            if !include.contains(entry) {
                include.push(entry.clone());
            }
        }
        let bypass = match &self.bypass {
            BypassIntent::Exactly(list) => list.clone(),
            BypassIntent::OnCreate(_) => existing
                .get("bypass_actors")
                .and_then(Value::as_array)
                .cloned()
                .ok_or(
                    "its bypass list is not readable, so it cannot be written without changing it",
                )?,
        };
        let mut rules: Vec<Value> = existing
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if self.no_update_rule
            && rules
                .iter()
                .any(|rule| rule.get("type").and_then(Value::as_str) == Some("update"))
        {
            return Err(
                "it carries an `update` rule, which would block every merge into its branches; remove it on the host, since remote protect never removes a rule"
                    .into(),
            );
        }
        for intended in &self.rules {
            let kind = intended.get("type").and_then(Value::as_str).unwrap_or("");
            match rules
                .iter_mut()
                .find(|rule| rule.get("type").and_then(Value::as_str) == Some(kind))
            {
                None => rules.push(intended.clone()),
                Some(current) => strengthen(current, intended)?,
            }
        }
        Ok(json!({
            "name": self.name,
            "target": "branch",
            "enforcement": "active",
            "conditions": { "ref_name": { "include": include, "exclude": [] } },
            "bypass_actors": bypass,
            "rules": rules,
        }))
    }
}

/// The five written fields of a live ruleset, to tell whether a write
/// would change anything.
fn written_fields(ruleset: &Value) -> Value {
    json!({
        "name": ruleset.get("name").cloned().unwrap_or(Value::Null),
        "target": ruleset.get("target").cloned().unwrap_or(Value::Null),
        "enforcement": ruleset.get("enforcement").cloned().unwrap_or(Value::Null),
        "conditions": { "ref_name": {
            "include": ruleset.pointer("/conditions/ref_name/include").cloned().unwrap_or(json!([])),
            "exclude": ruleset.pointer("/conditions/ref_name/exclude").cloned().unwrap_or(json!([])),
        } },
        "bypass_actors": ruleset.get("bypass_actors").cloned().unwrap_or(Value::Null),
        "rules": ruleset.get("rules").cloned().unwrap_or(json!([])),
    })
}

/// Raise `current`'s parameters to at least `intended`'s, in place.
fn strengthen(current: &mut Value, intended: &Value) -> Result<(), String> {
    let kind = intended
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(wanted) = intended.get("parameters").and_then(Value::as_object) else {
        return Ok(());
    };
    if !current.get("parameters").is_some_and(Value::is_object) {
        current["parameters"] = json!({});
    }
    let parameters = current["parameters"]
        .as_object_mut()
        .ok_or("rule parameters are not an object")?;
    for (key, value) in wanted {
        match (kind.as_str(), key.as_str()) {
            ("pull_request", "required_approving_review_count") => {
                let live = parameters.get(key).and_then(Value::as_u64).unwrap_or(0);
                let want = value.as_u64().unwrap_or(0);
                parameters.insert(key.clone(), json!(live.max(want)));
            }
            ("required_status_checks", "required_status_checks") => {
                let mut checks = parameters
                    .get(key)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for check in value.as_array().into_iter().flatten() {
                    let context = check.get("context").and_then(Value::as_str);
                    let app = check.get("integration_id").and_then(Value::as_u64);
                    match checks
                        .iter_mut()
                        .find(|c| c.get("context").and_then(Value::as_str) == context)
                    {
                        None => checks.push(check.clone()),
                        Some(live) => match live.get("integration_id").and_then(Value::as_u64) {
                            None => {
                                if let Some(app) = app {
                                    live["integration_id"] = json!(app);
                                }
                            }
                            Some(pinned) if Some(pinned) == app => {}
                            Some(pinned) => {
                                return Err(format!(
                                    "it pins `{}` to app {pinned}, not {}; remote protect never changes which app produces a check, so change it on the host",
                                    context.unwrap_or_default(),
                                    app.map_or_else(|| "any".to_string(), |a| a.to_string())
                                ))
                            }
                        },
                    }
                }
                parameters.insert(key.clone(), Value::Array(checks));
            }
            // The flags that only let more through stay off where the live
            // rule has them off.
            (_, "do_not_enforce_on_create" | "update_allows_fetch_and_merge") => {
                let live = parameters
                    .get(key)
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                parameters.insert(key.clone(), json!(live && value.as_bool().unwrap_or(false)));
            }
            // Every other flag only binds more when on.
            (_, _) if value.is_boolean() => {
                let live = parameters
                    .get(key)
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                parameters.insert(key.clone(), json!(live || value.as_bool().unwrap_or(false)));
            }
            _ => {
                parameters
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }
    Ok(())
}

/// The owned rulesets of a plan: A, B and C first, then the policy
/// patterns, each with its body or why it cannot be written.
fn owned_rulesets(
    plan: &ProtectionPlan,
    default_branch: Option<&str>,
    declared: Option<&Result<Declared, String>>,
) -> Vec<Result<Owned, (String, String)>> {
    let mut owned = Vec::new();
    if let Some(authority) = &plan.authority {
        let default = vec!["~DEFAULT_BRANCH".to_string()];
        owned.push(Ok(Owned {
            name: RULESET_AUTHORITY_UPDATES.into(),
            target: "~DEFAULT_BRANCH".into(),
            include: default.clone(),
            rules: vec![json!({
                "type": "update",
                "parameters": { "update_allows_fetch_and_merge": false }
            })],
            bypass: BypassIntent::OnCreate(vec![authority.profile.bypass_actor()]),
            no_update_rule: false,
        }));
        let (checks_b, checks_c) = match declared {
            Some(Ok(declared)) => planned_checks(authority, declared),
            Some(Err(why)) => (Err(why.clone()), Err(why.clone())),
            None => {
                let why = "the default branch is not known".to_string();
                (Err(why.clone()), Err(why))
            }
        };
        let independent = authority.profile == Profile::IndependentReview;
        owned.push(
            checks_b
                .map(|checks| Owned {
                    name: RULESET_AUTHORITY_CHECKS.into(),
                    target: "~DEFAULT_BRANCH".into(),
                    include: default,
                    rules: vec![
                        pull_request_rule(authority.profile.approvals(), independent),
                        status_checks_rule(checks),
                        json!({ "type": "non_fast_forward" }),
                        json!({ "type": "deletion" }),
                    ],
                    bypass: BypassIntent::Exactly(Vec::new()),
                    no_update_rule: true,
                })
                .map_err(|why| (RULESET_AUTHORITY_CHECKS.to_string(), why)),
        );
        let line = format!("refs/heads/{}", authority.lines);
        owned.push(
            checks_c
                .map(|checks| Owned {
                    name: RULESET_LINE_CHECKS.into(),
                    target: line.clone(),
                    include: vec![line.clone()],
                    rules: vec![
                        pull_request_rule(0, false),
                        status_checks_rule(checks),
                        json!({ "type": "non_fast_forward" }),
                        json!({ "type": "deletion" }),
                    ],
                    bypass: BypassIntent::Exactly(Vec::new()),
                    no_update_rule: true,
                })
                .map_err(|why| (RULESET_LINE_CHECKS.to_string(), why)),
        );
    }
    for rule in plan.pattern_rules(default_branch) {
        owned.push(Ok(Owned {
            name: rule.ruleset_name(),
            target: format!("refs/heads/{}", rule.pattern),
            include: vec![format!("refs/heads/{}", rule.pattern)],
            rules: rule.rules(),
            bypass: BypassIntent::OnCreate(Vec::new()),
            no_update_rule: false,
        }));
    }
    owned
}

/// One line per rule of a ruleset body, for the report.
fn describe(body: &Value) -> String {
    let mut parts = Vec::new();
    for rule in body
        .get("rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parameters = &rule["parameters"];
        match rule.get("type").and_then(Value::as_str).unwrap_or("?") {
            "update" => parts.push("only bypass actors update it".to_string()),
            "pull_request" => parts.push(format!(
                "a pull request with {} approval(s)",
                parameters["required_approving_review_count"]
                    .as_u64()
                    .unwrap_or(0)
            )),
            "required_status_checks" => parts.push(format!(
                "{} checks: {}",
                if parameters["strict_required_status_checks_policy"] == true {
                    "strict"
                } else {
                    "non-strict"
                },
                parameters["required_status_checks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|check| match check["integration_id"].as_u64() {
                        Some(app) =>
                            format!("{} (app {app})", check["context"].as_str().unwrap_or("?")),
                        None => check["context"].as_str().unwrap_or("?").to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            "non_fast_forward" => parts.push("no force push".into()),
            "deletion" => parts.push("no deletion".into()),
            other => parts.push(other.to_string()),
        }
    }
    let actors = body
        .get("bypass_actors")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    parts.push(if actors == 0 {
        "no bypass".to_string()
    } else {
        format!("{actors} bypass actor(s)")
    });
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// Provider abstraction
// ---------------------------------------------------------------------------

/// A remote hosting provider that can enforce branch protection.
pub trait RemoteProvider {
    /// Provider name (`github`, `gitlab`, ...).
    fn name(&self) -> &'static str;

    /// Apply the plan, reporting exactly what was and was not applied.
    fn apply(&self, plan: &ProtectionPlan) -> ProtectReport;
}

/// Resolve a provider by name.
///
/// `github` resolves to the gh-backed adapter. Any other name returns the
/// manual-checklist provider: the plan is printed as a checklist and the
/// run reports `degraded` — pluggable adapters can slot in later
/// (charter D18).
///
/// # Errors
///
/// Returns `Err(String)` when the GitHub adapter is requested but the `gh`
/// CLI cannot be found on PATH.
pub fn provider_for(name: &str, repo_dir: &Path) -> Result<Box<dyn RemoteProvider>, String> {
    match name {
        "github" => Ok(Box::new(GithubProvider::discover(repo_dir)?)),
        other => Ok(Box::new(ManualChecklistProvider {
            provider: other.to_string(),
        })),
    }
}

/// Fallback provider for hosts without an adapter: reports the manual
/// checklist and degraded status.
pub struct ManualChecklistProvider {
    provider: String,
}

impl RemoteProvider for ManualChecklistProvider {
    fn name(&self) -> &'static str {
        "manual"
    }

    fn apply(&self, plan: &ProtectionPlan) -> ProtectReport {
        ProtectReport {
            status: ProtectStatus::Degraded,
            lines: vec![format!(
                "no adapter for provider '{}' — apply the checklist manually",
                self.provider
            )],
            limitations: vec![format!(
                "provider '{}' is not implemented (only github); nothing was applied",
                self.provider
            )],
            checklist: plan.manual_checklist(),
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub adapter (gh CLI)
// ---------------------------------------------------------------------------

/// A `gh` call: arguments and optional stdin, answering stdout or the
/// combined output of a failure.
pub type GhTransport = Box<dyn Fn(&[&str], Option<&str>) -> Result<String, String>>;

/// GitHub adapter, shelling out to the `gh` CLI for auth and transport.
pub struct GithubProvider {
    transport: GhTransport,
}

impl GithubProvider {
    /// Locate `gh` on PATH and bind to `repo_dir`.
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` when `gh` is not installed/locatable.
    pub fn discover(repo_dir: &Path) -> Result<Self, String> {
        let gh = which::which("gh")
            .map_err(|e| format!("gh CLI not found ({e}); install GitHub CLI or use --dry-run"))?;
        Ok(Self::with_gh(gh, repo_dir))
    }

    /// Construct with an explicit `gh` binary (test seam for PATH shims).
    pub fn with_gh(gh: impl Into<PathBuf>, repo_dir: impl AsRef<Path>) -> Self {
        let gh = gh.into();
        let repo_dir = repo_dir.as_ref().to_path_buf();
        Self::with_transport(Box::new(move |args, input| {
            run_gh(&gh, &repo_dir, args, input)
        }))
    }

    /// Construct with a transport that answers `gh` calls (test seam for a
    /// recorded host).
    #[must_use]
    pub fn with_transport(transport: GhTransport) -> Self {
        Self { transport }
    }

    fn run_gh(&self, args: &[&str], input: Option<&str>) -> Result<String, String> {
        (self.transport)(args, input)
    }

    /// Identify the repo (`owner/name`, visibility, default branch) via
    /// `gh repo view`.
    fn repo_info(&self) -> Result<RepoInfo, String> {
        let out = self.run_gh(
            &[
                "repo",
                "view",
                "--json",
                "nameWithOwner,isPrivate,defaultBranchRef",
            ],
            None,
        )?;
        let v: Value =
            serde_json::from_str(&out).map_err(|e| format!("gh repo view parse: {e}"))?;
        let nwo = v
            .get("nameWithOwner")
            .and_then(Value::as_str)
            .ok_or_else(|| "gh repo view: nameWithOwner missing".to_string())?
            .to_string();
        let private = v.get("isPrivate").and_then(Value::as_bool).unwrap_or(false);
        let default_branch = v
            .pointer("/defaultBranchRef/name")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        Ok(RepoInfo {
            nwo,
            private,
            default_branch,
        })
    }

    /// The summaries of every ruleset of the repository, all pages. A page
    /// that is not a list was not read; taking it as empty would add a
    /// second ruleset beside one already there.
    fn ruleset_summaries(&self, nwo: &str) -> Result<Vec<Value>, String> {
        let mut found = Vec::new();
        for page in 1.. {
            let list = self.run_gh(
                &[
                    "api",
                    &format!("repos/{nwo}/rulesets?per_page={RULESET_PAGE_SIZE}&page={page}"),
                ],
                None,
            )?;
            let Ok(Value::Array(summaries)) = serde_json::from_str::<Value>(&list) else {
                return Err(format!("page {page} of the rulesets is not a JSON list"));
            };
            let short = summaries.len() < RULESET_PAGE_SIZE;
            found.extend(summaries);
            if short {
                break;
            }
        }
        Ok(found)
    }

    /// Write one owned ruleset: create it, or bring the one with its name
    /// up to it, or leave it when it already is. Returns the report line.
    fn apply_owned(&self, nwo: &str, summaries: &[Value], owned: &Owned) -> Result<String, String> {
        let named: Vec<&Value> = summaries
            .iter()
            .filter(|s| s.get("name").and_then(Value::as_str) == Some(owned.name.as_str()))
            .collect();
        let (method, path, body) = match named.as_slice() {
            [] => (
                "POST",
                format!("repos/{nwo}/rulesets"),
                owned.create_body(),
            ),
            [summary] => {
                let ours = summary.get("target").and_then(Value::as_str) == Some("branch")
                    && summary
                        .get("source_type")
                        .and_then(Value::as_str)
                        .is_none_or(|source| source == "Repository");
                let id = summary.get("id").and_then(Value::as_u64);
                let (true, Some(id)) = (ours, id) else {
                    return Err(format!(
                        "a ruleset named '{}' exists that is not a branch ruleset of this repository; rename or remove it on the host",
                        owned.name
                    ));
                };
                let live = self.run_gh(&["api", &format!("repos/{nwo}/rulesets/{id}")], None)?;
                let live: Value = serde_json::from_str(&live)
                    .map_err(|e| format!("ruleset {id} is not JSON ({e})"))?;
                let body = owned
                    .update_body(&live)
                    .map_err(|why| format!("ruleset '{}' ({id}) was not written: {why}", owned.name))?;
                if written_fields(&live) == body {
                    return Ok(format!(
                        "ruleset '{}' ({id}) on {} already as intended, nothing written: {}",
                        owned.name,
                        owned.target,
                        describe(&body)
                    ));
                }
                ("PUT", format!("repos/{nwo}/rulesets/{id}"), body)
            }
            _ => {
                return Err(format!(
                    "{} rulesets are named '{}', so remote protect cannot tell which it owns; keep one on the host",
                    named.len(),
                    owned.name
                ))
            }
        };
        let answer = self.run_gh(
            &["api", "-X", method, &path, "--input", "-"],
            Some(&body.to_string()),
        )?;
        // The host's answer is the ruleset as written: its bypass list is
        // checked as a field, since an update's handling of it is not
        // documented.
        let written: Value = serde_json::from_str(&answer).unwrap_or(Value::Null);
        let id = written.get("id").and_then(Value::as_u64);
        if written.get("bypass_actors") != body.get("bypass_actors") {
            return Err(format!(
                "ruleset '{}' was written, but the host answered with the bypass list {} where {} was sent; read it on the host",
                owned.name,
                written
                    .get("bypass_actors")
                    .map_or_else(|| "(none)".to_string(), Value::to_string),
                body["bypass_actors"]
            ));
        }
        Ok(format!(
            "{} ruleset '{}'{} on {}: {}",
            if method == "POST" {
                "created"
            } else {
                "updated"
            },
            owned.name,
            id.map_or_else(String::new, |id| format!(" ({id})")),
            owned.target,
            describe(&body)
        ))
    }

    /// Classify a failed gh call into a precise limitation message.
    fn limitation_for(pattern: &str, err: &str, private: bool) -> String {
        let lower = err.to_lowercase();
        if lower.contains("upgrade to github pro")
            || (lower.contains("403") && lower.contains("upgrade"))
        {
            let visibility = if private {
                "this repository is private"
            } else {
                "the plan does not include this feature"
            };
            format!(
                "{pattern}: GitHub plan limitation (HTTP 403 'Upgrade to GitHub Pro') — \
                 rulesets on private repositories require GitHub Pro and \
                 {visibility}. Options: upgrade the plan, make the repository public, \
                 or apply the rules manually via the checklist below."
            )
        } else {
            format!("{pattern}: not applied — {err}")
        }
    }
}

/// Run `gh` with args, optionally piping `input` to stdin. Returns stdout
/// on success, combined output as the error string on failure.
fn run_gh(
    gh: &Path,
    repo_dir: &Path,
    args: &[&str],
    input: Option<&str>,
) -> Result<String, String> {
    let mut cmd = Command::new(gh);
    cmd.args(args)
        .current_dir(repo_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Stdin is written on its own thread while stdout and stderr are
    // drained, so a large `gh` answer cannot deadlock a large `--input`.
    // A `gh` that exits before consuming all of `--input` (an early auth
    // failure, or a test shim that ignores stdin) is not itself the
    // verdict: its exit status and stderr decide.
    let output = match input {
        Some(data) => crate::git::output_with_input(&mut cmd, data.as_bytes())
            .map_err(|e| format!("gh: {e}"))?,
        None => cmd
            .spawn()
            .map_err(|e| format!("gh spawn: {e}"))?
            .wait_with_output()
            .map_err(|e| format!("gh: {e}"))?,
    };
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|error| format!("gh output is not valid UTF-8: {error}"))
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{stdout}{stderr}").trim().to_string())
    }
}

/// What `gh repo view` says about the repository.
struct RepoInfo {
    nwo: String,
    private: bool,
    default_branch: Option<String>,
}

/// How a full ruleset relates to a branch or pattern.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Targeting {
    Targets,
    Misses,
    /// A ref pattern it depends on could not be read, with why.
    Unexplained(String),
}

/// `'name' (id)` for a full ruleset.
pub(crate) fn ruleset_label(ruleset: &Value) -> String {
    let name = ruleset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed");
    let id = ruleset
        .get("id")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    format!("'{name}' ({id})")
}

/// Whether a full ruleset targets `pattern`: one of its includes matches
/// the branch and none of its excludes does. An entry matches when it is
/// `~ALL`, `~DEFAULT_BRANCH` with `pattern` the default branch, the branch
/// ref itself, or an fnmatch pattern the ref `refs/heads/<pattern>` fits,
/// matched as GitHub matches it ([`fnmatch`]). When the answer depends on
/// an entry that cannot be read, it is unexplained, never a miss.
pub(crate) fn ruleset_targets(
    ruleset: &Value,
    pattern: &str,
    default_branch: Option<&str>,
) -> Targeting {
    let names = |list: &str| -> Vec<&str> {
        ruleset
            .pointer(&format!("/conditions/ref_name/{list}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    let reference = format!("refs/heads/{pattern}");
    let entry_matches = |entry: &str| -> Result<bool, String> {
        match entry {
            "~ALL" => Ok(true),
            "~DEFAULT_BRANCH" => Ok(default_branch == Some(pattern)),
            entry if entry == reference => Ok(true),
            entry => fnmatch::matches(entry, &reference)
                .map_err(|why| format!("its ref pattern '{entry}' {why}")),
        }
    };
    // Any entry that matches settles the list; else an unreadable one
    // leaves it open.
    let any = |list: &str| -> Result<bool, String> {
        let mut unreadable = None;
        for entry in names(list) {
            match entry_matches(entry) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(why) => {
                    unreadable.get_or_insert(why);
                }
            }
        }
        unreadable.map_or(Ok(false), Err)
    };
    match (any("include"), any("exclude")) {
        (Ok(false), _) | (_, Ok(true)) => Targeting::Misses,
        (Ok(true), Ok(false)) => Targeting::Targets,
        (Err(why), _) | (_, Err(why)) => Targeting::Unexplained(why),
    }
}

impl RemoteProvider for GithubProvider {
    fn name(&self) -> &'static str {
        "github"
    }

    fn apply(&self, plan: &ProtectionPlan) -> ProtectReport {
        let repo = match self.repo_info() {
            Ok(info) => info,
            Err(e) => {
                return ProtectReport {
                    status: ProtectStatus::Degraded,
                    lines: vec![format!("could not identify the GitHub repo: {e}")],
                    limitations: vec![
                        "no rules were applied; the repo could not be resolved via gh".to_string(),
                    ],
                    checklist: plan.manual_checklist(),
                };
            }
        };

        let mut lines = vec![format!(
            "repo: {} ({})",
            repo.nwo,
            if repo.private { "private" } else { "public" }
        )];
        if let Some(authority) = &plan.authority {
            lines.push(format!(
                "profile {}; {}",
                authority.profile.as_str(),
                authority.mode()
            ));
        }
        let mut limitations = Vec::new();
        let mut checklist = Vec::new();
        let declared = match (&plan.authority, repo.default_branch.as_deref()) {
            (Some(_), Some(branch)) => {
                let gh = |args: &[&str]| self.run_gh(args, None);
                Some(read_declared(&gh, &repo.nwo, branch))
            }
            _ => None,
        };
        let owned = owned_rulesets(plan, repo.default_branch.as_deref(), declared.as_ref());
        let summaries = self.ruleset_summaries(&repo.nwo);
        for entry in owned {
            let (name, outcome, refused) = match (entry, &summaries) {
                (Err((name, why)), _) => (name, Err(why), true),
                (Ok(owned), Err(why)) => (owned.name.clone(), Err(why.clone()), false),
                (Ok(owned), Ok(summaries)) => {
                    let name = owned.name.clone();
                    let outcome = self.apply_owned(&repo.nwo, summaries, &owned);
                    (name, outcome, false)
                }
            };
            match outcome {
                Ok(line) => lines.push(line),
                Err(e) if refused => {
                    limitations.push(format!("ruleset '{name}' was not written: {e}"));
                    checklist.push(format!(
                        "[ ] {name}: once the cause above is fixed, run `codeflow remote protect` again"
                    ));
                }
                Err(e) => {
                    limitations.push(Self::limitation_for(&name, &e, repo.private));
                    checklist.extend(
                        plan.manual_checklist()
                            .into_iter()
                            .filter(|item| item.starts_with(&format!("[ ] {name}"))),
                    );
                }
            }
        }
        lines.push(
            "rulesets with other names were not read for writing and were left as they are"
                .to_string(),
        );

        ProtectReport {
            status: if limitations.is_empty() {
                ProtectStatus::Applied
            } else {
                ProtectStatus::Degraded
            },
            lines,
            limitations,
            checklist,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn r21_invalid_utf8_repository_identity_refuses_without_api_request() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("gh");
        std::fs::write(
            &shim,
            r#"#!/bin/sh
if [ "$1" = "repo" ]; then
  printf '{"nameWithOwner":"owner/caf'
  printf '\377'
  printf '","isPrivate":false}'
  exit 0
fi
: > api-called
printf '{}'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_until_executable(&shim);
        let policy = write_policy(dir.path(), r#"["main"]"#);
        let plan = ProtectionPlan::from_policy_file(&policy).unwrap();
        let report = GithubProvider::with_gh(&shim, dir.path()).apply(&plan);
        assert_eq!(report.status, ProtectStatus::Degraded);
        assert!(!dir.path().join("api-called").exists());
        assert!(report.lines.iter().any(|line| line.contains("UTF-8")));
    }

    #[cfg(unix)]
    #[test]
    fn r18_remote_dangling_policy_refuses() {
        for leaf in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join(".codeflow");
            let path = config.join("policy.json");
            if leaf {
                std::fs::create_dir(&config).unwrap();
            }
            std::os::unix::fs::symlink(
                dir.path().join("missing"),
                if leaf { &path } else { &config },
            )
            .unwrap();
            assert!(
                ProtectionPlan::from_policy_file(&path).is_err(),
                "leaf={leaf}"
            );
        }
    }

    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    fn write_policy(dir: &Path, branches: &str) -> PathBuf {
        let path = dir.join("policy.json");
        std::fs::write(
            &path,
            format!(r#"{{ "schema_version": 1, "git": {{ "protected_branches": {branches} }} }}"#),
        )
        .unwrap();
        path
    }

    #[cfg(unix)]
    fn write_shim(dir: &Path, api_behavior: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("gh");
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"repo\" ]; then\n\
               printf '%s' '{{\"nameWithOwner\":\"sathyassn/private-repo\",\"isPrivate\":true}}'\n\
               exit 0\n\
             fi\n\
             {api_behavior}\n"
        );
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        wait_until_executable(&path);
        path
    }

    /// A child forked by a concurrent test can briefly inherit the write
    /// descriptor of a freshly written shim, and Linux then refuses to run it
    /// (ETXTBSY). The provider would report that as an unresolvable repo, so
    /// wait until one run succeeds. Nobody writes the shim again, so every
    /// later run succeeds too.
    #[cfg(unix)]
    fn wait_until_executable(path: &Path) {
        for _ in 0..200 {
            match Command::new(path)
                .arg("repo")
                .stdout(Stdio::null())
                .status()
            {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("shim {} does not run: {e}", path.display()),
                Ok(_) => return,
            }
        }
        panic!("shim {} stayed busy for 2 s", path.display());
    }

    #[test]
    fn test_plan_from_policy_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path).unwrap();
        assert_eq!(plan.rules.len(), 2);
        assert_eq!(plan.rules[0].pattern, "main");
        assert!(!plan.rules[0].is_glob());
        assert!(plan.rules[1].is_glob());
        assert!(plan.rules[0].require_pr);
        assert!(plan.rules[0].block_force_push);
        assert!(plan.rules[0].block_deletion);
    }

    #[test]
    fn r17_plan_missing_policy_uses_strict_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let plan = ProtectionPlan::from_policy_file(&dir.path().join("nope.json")).unwrap();
        assert_eq!(
            plan.rules
                .iter()
                .map(|rule| rule.pattern.as_str())
                .collect::<Vec<_>>(),
            ["main", "master"]
        );
        for rule in plan.rules {
            assert!(
                rule.require_pr
                    && rule.require_status_checks
                    && rule.block_force_push
                    && rule.block_deletion
            );
        }
    }

    #[test]
    fn test_plan_respects_allow_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(
            &path,
            r#"{ "git": { "protected_branches": ["main"], "delete_protected": "warn" } }"#,
        )
        .unwrap();
        let plan = ProtectionPlan::from_policy_file(&path).unwrap();
        assert!(!plan.rules[0].block_deletion);
        assert!(plan.rules[0].block_force_push, "unset values stay strict");
    }

    fn solo() -> AuthorityPlan {
        AuthorityPlan::new(Profile::SoloAdministrator, None, DEFAULT_LINES).unwrap()
    }

    #[test]
    fn test_dry_run_report_prints_plan() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .with_authority(solo());
        let report = plan.dry_run_report("github");
        assert_eq!(report.status, ProtectStatus::DryRun);
        let text = report.render();
        for expected in [
            "profile solo-administrator; Mode B",
            "ruleset codeflow authority updates on ~DEFAULT_BRANCH:",
            "only the administrator role (RepositoryRole 5) updates the branch",
            "ruleset codeflow authority checks on ~DEFAULT_BRANCH:",
            "ruleset codeflow line checks on refs/heads/integration/**:",
            "creating a line stays allowed",
            "ruleset codeflow protect release/*:",
            "require a pull request before merging",
            "block force pushes",
            "block branch deletion",
            "status: dry-run",
        ] {
            assert!(text.contains(expected), "{expected}: {text:?}");
        }
    }

    #[test]
    fn test_manual_provider_degrades_with_checklist() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let plan = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .with_authority(solo());
        let provider = provider_for("gitlab", dir.path()).unwrap();
        let report = provider.apply(&plan);
        assert_eq!(report.status, ProtectStatus::Degraded);
        assert!(report
            .checklist
            .iter()
            .any(|c| c.starts_with("[ ] codeflow authority checks on ~DEFAULT_BRANCH:")));
        assert!(report
            .checklist
            .iter()
            .any(|c| c.starts_with("[ ] codeflow protect main:")));
        assert!(report.limitations[0].contains("gitlab"));
    }

    #[cfg(unix)]
    #[test]
    fn test_github_degraded_on_free_plan_403() {
        let dir = tempfile::tempdir().unwrap();
        let gh = write_shim(
            dir.path(),
            "echo 'HTTP 403: Upgrade to GitHub Pro or make this repository public to enable this feature. (https://docs.github.com/rest/repos/rules)' >&2\nexit 1",
        );
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path).unwrap();

        let provider = GithubProvider::with_gh(gh, dir.path());
        let report = provider.apply(&plan);

        assert_eq!(report.status, ProtectStatus::Degraded);
        assert_eq!(report.limitations.len(), 2, "{:?}", report.limitations);
        assert!(
            report.limitations[0].contains("Upgrade to GitHub Pro"),
            "precise plan limitation expected: {}",
            report.limitations[0]
        );
        assert!(report.limitations[0].contains("private"));
        // Checklist covers every failed rule.
        assert!(report
            .checklist
            .iter()
            .any(|c| c.contains("[ ] codeflow protect main: require a pull request")));
        assert!(report
            .checklist
            .iter()
            .any(|c| c.contains("[ ] codeflow protect release/*: block force pushes")));
        let text = report.render();
        assert!(text.contains("status: degraded"), "got:\n{text:?}");
    }

    #[cfg(unix)]
    #[test]
    fn test_github_applies_when_gh_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        // The rulesets list is a JSON array on GitHub; a write answers the
        // ruleset as written.
        let gh = write_shim(
            dir.path(),
            "case \"$*\" in *rulesets\\?*) printf '%s' '[]' ;; *\"-X\"*) cat ;; *) printf '%s' '{}' ;; esac\nexit 0",
        );
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path).unwrap();

        let provider = GithubProvider::with_gh(gh, dir.path());
        let report = provider.apply(&plan);

        let text = report.render();
        assert_eq!(report.status, ProtectStatus::Applied, "{text:?}");
        assert!(report.limitations.is_empty());
        assert!(report.checklist.is_empty());
        assert!(
            text.contains("created ruleset 'codeflow protect main' on refs/heads/main"),
            "got:\n{text:?}"
        );
        assert!(
            text.contains("created ruleset 'codeflow protect release/*'"),
            "got:\n{text:?}"
        );
        assert!(text.contains("status: applied"));
    }

    #[cfg(unix)]
    #[test]
    fn test_github_degrades_when_repo_unresolvable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let gh = dir.path().join("gh");
        std::fs::write(&gh, "#!/bin/sh\necho 'not a git repository' >&2\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&gh).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&gh, perms).unwrap();

        let path = write_policy(dir.path(), r#"["main"]"#);
        let plan = ProtectionPlan::from_policy_file(&path).unwrap();
        let report = GithubProvider::with_gh(gh, dir.path()).apply(&plan);
        assert_eq!(report.status, ProtectStatus::Degraded);
        assert!(!report.checklist.is_empty());
    }

    #[test]
    fn registry_data_profile_is_a_ruleset_that_leaves_main_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let before = ProtectionPlan::from_policy_file(&path).unwrap();
        let plan = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .with_registry_profile();
        assert_eq!(plan.rules.len(), before.rules.len() + 1);
        let main = &plan.rules[0];
        assert_eq!(main.pattern, "main");
        assert!(main.require_pr && !main.data_profile);
        let registry = plan.rules.last().unwrap();
        assert_eq!(registry.pattern, "codeflow/registry");
        assert!(registry.block_force_push && registry.block_deletion);
        assert!(!registry.require_pr && !registry.require_status_checks);
        let kinds: Vec<String> = registry
            .rules()
            .iter()
            .map(|rule| rule["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kinds, ["non_fast_forward", "deletion"]);
        let report = plan.dry_run_report("github").render();
        assert!(
            report.contains("ruleset codeflow protect codeflow/registry:"),
            "{report:?}"
        );
    }

    #[test]
    fn test_limitation_for_generic_error() {
        let msg = GithubProvider::limitation_for("main", "HTTP 502: oops", false);
        assert!(msg.contains("main: not applied"));
        assert!(msg.contains("HTTP 502"));
    }

    #[test]
    fn an_empty_check_list_emits_no_status_check_rule() {
        let rule = BranchRule {
            pattern: "main".into(),
            require_pr: true,
            require_status_checks: true,
            block_force_push: true,
            block_deletion: true,
            data_profile: false,
            required_checks: Vec::new(),
        };
        assert!(
            !rule
                .rules()
                .iter()
                .any(|r| r["type"] == "required_status_checks"),
            "{:?}",
            rule.rules()
        );
    }

    /// The plan reads `git.required_checks` as the schema does: a list the
    /// schema refuses is an error, never replaced by the shipped defaults.
    #[test]
    fn a_required_checks_list_the_schema_refuses_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        for list in [r#""build""#, "[1]", r#"["build", 2]"#, "[]", r#"[" "]"#] {
            std::fs::write(
                &path,
                format!(r#"{{ "git": {{ "protected_branches": ["main"], "required_checks": {list} }} }}"#),
            )
            .unwrap();
            assert!(ProtectionPlan::from_policy_file(&path).is_err(), "{list}");
        }
    }

    #[test]
    fn the_dry_run_names_strict_checks_from_the_policy_or_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let text = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .dry_run_report("github")
            .render();
        assert!(
            text.contains(
                "require status checks to pass on a branch that is up to date with its base: \
                 codeflow gates, secret scan, security review, commit standards"
            ),
            "{text:?}"
        );
        assert!(text.contains("never lowers an approval count"), "{text:?}");
        std::fs::write(
            &path,
            r#"{ "git": { "protected_branches": ["main"], "required_checks": ["build", "windows"] } }"#,
        )
        .unwrap();
        let text = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .dry_run_report("github")
            .render();
        assert!(
            text.contains("up to date with its base: build, windows"),
            "{text:?}"
        );
    }

    /// With the authority rulesets, a policy pattern naming the default
    /// branch or the line pattern gets no ruleset of its own.
    #[test]
    fn the_default_branch_and_the_lines_get_no_pattern_ruleset() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main", "master", "integration/**"]"#);
        let plan = ProtectionPlan::from_policy_file(&path)
            .unwrap()
            .with_authority(solo());
        let patterns: Vec<&str> = plan
            .pattern_rules(Some("main"))
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, ["master"]);
    }

    #[test]
    fn the_authority_plan_refuses_a_check_app_or_line_pattern_that_cannot_be_right() {
        for (app, lines, why) in [
            (Some(0), DEFAULT_LINES, "not 0"),
            (Some(GITHUB_ACTIONS_APP_ID), DEFAULT_LINES, "GitHub Actions"),
            (None, " ", "must name a branch pattern"),
            (None, "refs/heads/integration/**", "below refs/heads/"),
            (None, "~ALL", "below refs/heads/"),
            (None, "release/[ma", "release/[ma"),
        ] {
            let error = AuthorityPlan::new(Profile::SoloAdministrator, app, lines).unwrap_err();
            assert!(error.contains(why), "{app:?} {lines}: {error}");
        }
        let plan = AuthorityPlan::new(Profile::IndependentReview, Some(42), "release/**").unwrap();
        assert_eq!(plan.lines, "release/**");
        assert!(plan.mode().contains("GitHub App 42"), "{}", plan.mode());
        assert_eq!(
            Profile::parse("independent-review"),
            Ok(Profile::IndependentReview)
        );
        assert!(Profile::parse("solo")
            .unwrap_err()
            .contains("solo-administrator"));
    }

    #[test]
    fn workflow_job_names_are_literal_names_or_ids() {
        let text = r"
name: x
on: pull_request_target
jobs:
  verdict:
    name: 'trusted: commit standards'
    runs-on: ubuntu-24.04
  scan:
    name: 'trusted: secret scan'
  gates:
    runs-on: ubuntu-24.04
  matrix:
    name: windows tests (${{ matrix.partition }})
  authority:
    environment: codeflow-authority
  other:
    environment:
      name: codeflow-authority
";
        let names = workflow_job_names(text).unwrap();
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            [
                "authority",
                "gates",
                "other",
                "trusted: commit standards",
                "trusted: secret scan"
            ]
        );
        assert_eq!(
            workflow_environments(text).unwrap(),
            [
                ("authority".to_string(), "codeflow-authority".to_string()),
                ("other".to_string(), "codeflow-authority".to_string())
            ]
        );
        assert!(workflow_job_names("jobs: [").is_err());
        assert!(workflow_job_names("jobs: [1]").is_err());
    }

    #[test]
    fn a_ref_is_encoded_for_a_query() {
        assert_eq!(query_value("main"), "main");
        assert_eq!(query_value("release/1.0"), "release/1.0");
        assert_eq!(query_value("a&b c#"), "a%26b%20c%23");
    }

    /// A `gh` stand-in that serves a host's live rules from files and logs
    /// every write: `writes` holds `<METHOD> <path>` per line and `bodies`
    /// the matching request body. A write answers its body, as the host
    /// answers the ruleset as written.
    #[cfg(unix)]
    struct Host {
        dir: tempfile::TempDir,
        gh: PathBuf,
    }

    #[cfg(unix)]
    impl Host {
        fn new(rulesets: &str, by_id: &[(u64, &str)]) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path().display().to_string();
            std::fs::write(dir.path().join("rulesets.json"), rulesets).unwrap();
            for (id, body) in by_id {
                std::fs::write(dir.path().join(format!("ruleset-{id}.json")), body).unwrap();
            }
            let script = format!(
                r#"#!/bin/sh
d='{d}'
case "$*" in
  "repo view"*) printf '%s' '{{"nameWithOwner":"o/r","isPrivate":false,"defaultBranchRef":{{"name":"main"}}}}' ;;
  "repo") exit 0 ;;
  "api -X "*) printf '%s %s\n' "$3" "$4" >> "$d/writes"; body="$(cat)"; printf '%s\n' "$body" >> "$d/bodies"; printf '%s' "$body" ;;
  "api repos/o/r/rulesets?per_page=100"*)
    page="${{2##*&page=}}"
    case "$page" in "$2") page=1 ;; esac
    if [ "$page" = 1 ]; then cat "$d/rulesets.json";
    elif [ -f "$d/rulesets-$page.json" ]; then cat "$d/rulesets-$page.json";
    else printf '[]'; fi ;;
  "api repos/o/r/rulesets/"*) cat "$d/ruleset-${{2##*/}}.json" ;;
  *) echo "unexpected gh call: $*" >&2; exit 1 ;;
esac
"#
            );
            let gh = dir.path().join("gh");
            std::fs::write(&gh, script).unwrap();
            let mut perms = std::fs::metadata(&gh).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&gh, perms).unwrap();
            wait_until_executable(&gh);
            Self { dir, gh }
        }

        fn apply(&self, branches: &str) -> ProtectReport {
            let path = write_policy(self.dir.path(), branches);
            let plan = ProtectionPlan::from_policy_file(&path).unwrap();
            GithubProvider::with_gh(&self.gh, self.dir.path()).apply(&plan)
        }

        fn writes(&self) -> Vec<(String, serde_json::Value)> {
            let read = |name: &str| {
                std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
            };
            read("writes")
                .lines()
                .zip(read("bodies").lines())
                .map(|(call, body)| (call.to_string(), serde_json::from_str(body).unwrap()))
                .collect()
        }
    }

    /// The live shape of a ruleset that targets the default branch with six
    /// GitHub Actions checks and no strict policy (this repository's
    /// "main required checks" on 2026-10-08).
    const LIVE_RULESET: &str = r#"{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":false,"do_not_enforce_on_create":false,"required_status_checks":[{"context":"codeflow gates","integration_id":15368},{"context":"commit standards","integration_id":15368},{"context":"release impact","integration_id":15368},{"context":"secret scan","integration_id":15368},{"context":"security review","integration_id":15368},{"context":"windows","integration_id":15368}]}}],"bypass_actors":[]}"#;

    /// The line ruleset of this repository as recorded on 2026-10-29:
    /// non-strict checks, no force push, no deletion, no bypass.
    const LIVE_LINE_RULESET: &str = r#"{"id":25435656,"name":"integration lines","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["refs/heads/integration/**"]}},"rules":[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":false,"do_not_enforce_on_create":false,"required_status_checks":[{"context":"codeflow gates","integration_id":15368},{"context":"windows","integration_id":15368},{"context":"commit standards","integration_id":15368},{"context":"release impact","integration_id":15368}]}},{"type":"non_fast_forward"},{"type":"deletion"}],"bypass_actors":[]}"#;

    /// D-11 (TSK-1080 AC-2): a ruleset `remote protect` does not own is
    /// never written, even when it targets the default branch; its rules
    /// and bypass list stay the host's.
    #[cfg(unix)]
    #[test]
    fn a_foreign_ruleset_is_never_written() {
        let list = r#"[{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository"}]"#;
        let host = Host::new(list, &[(24_379_074, LIVE_RULESET)]);
        let report = host.apply(r#"["main"]"#);
        let writes = host.writes();
        assert!(
            writes
                .iter()
                .all(|(call, _)| !call.ends_with("rulesets/24379074")),
            "{writes:?}\n{}",
            report.render().as_str()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_glob_ruleset_requires_strict_checks_pinned_to_github_actions() {
        let host = Host::new("[]", &[]);
        let report = host.apply(r#"["release/*"]"#);
        assert_eq!(
            report.status,
            ProtectStatus::Applied,
            "{}",
            report.render().as_str()
        );
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "POST repos/o/r/rulesets");
        let body = &writes[0].1;
        assert_eq!(body["name"], "codeflow protect release/*");
        assert_eq!(body["bypass_actors"], json!([]));
        let rule = body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "required_status_checks")
            .cloned()
            .unwrap_or_else(|| panic!("no required_status_checks rule: {body}"));
        assert_eq!(
            rule["parameters"]["strict_required_status_checks_policy"],
            true
        );
        let checks: Vec<(String, u64)> = rule["parameters"]["required_status_checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["context"].as_str().unwrap().to_string(),
                    c["integration_id"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            checks,
            DEFAULT_REQUIRED_CHECKS
                .iter()
                .map(|name| (name.to_string(), 15368))
                .collect::<Vec<_>>()
        );
    }

    /// A list read that answers with a body that does not parse was not
    /// read: protect writes nothing rather than add a second ruleset.
    #[cfg(unix)]
    #[test]
    fn an_unparsable_live_read_writes_nothing() {
        let list = Host::new("<html>", &[]);
        let report = list.apply(r#"["release/*"]"#);
        assert_eq!(
            report.status,
            ProtectStatus::Degraded,
            "{}",
            report.render().as_str()
        );
        assert!(list.writes().is_empty(), "{:?}", list.writes());
    }

    #[cfg(unix)]
    #[test]
    fn an_owned_ruleset_on_a_later_page_is_updated_not_duplicated() {
        let filler: Vec<serde_json::Value> = (1..=RULESET_PAGE_SIZE as u64)
            .map(|id| {
                serde_json::json!({
                    "id": id, "name": format!("tags {id}"), "target": "tag",
                    "source_type": "Repository"
                })
            })
            .collect();
        let mut owned: Value = serde_json::from_str(LIVE_RULESET).unwrap();
        owned["id"] = json!(900);
        owned["name"] = json!("codeflow protect main");
        let host = Host::new(
            &serde_json::to_string(&filler).unwrap(),
            &[(900, &owned.to_string())],
        );
        std::fs::write(
            host.dir.path().join("rulesets-2.json"),
            r#"[{"id":900,"name":"codeflow protect main","target":"branch","source_type":"Repository"}]"#,
        )
        .unwrap();
        let report = host.apply(r#"["main"]"#);
        assert_eq!(
            report.status,
            ProtectStatus::Applied,
            "{}",
            report.render().as_str()
        );
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "PUT repos/o/r/rulesets/900");
        // The six live checks stay, now strict, and the policy's two
        // missing rules are added.
        let body = &writes[0].1;
        let kinds: Vec<&str> = body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "required_status_checks",
                "pull_request",
                "non_fast_forward",
                "deletion"
            ]
        );
        let checks = &body["rules"][0]["parameters"];
        assert_eq!(checks["strict_required_status_checks_policy"], true);
        assert_eq!(
            checks["required_status_checks"],
            owned["rules"][0]["parameters"]["required_status_checks"]
        );
        assert_eq!(body["bypass_actors"], json!([]));
        assert_eq!(body["enforcement"], "active");
        assert_eq!(
            body["conditions"]["ref_name"]["include"],
            json!(["~DEFAULT_BRANCH", "refs/heads/main"])
        );
    }

    #[test]
    fn ruleset_targets_matches_as_github_does() {
        let ruleset = |enforcement: &str, include: &[&str], exclude: &[&str]| {
            json!({
                "id": 1, "name": "r", "enforcement": enforcement,
                "conditions": { "ref_name": { "include": include, "exclude": exclude } }
            })
        };
        let main = Some("main");
        for (include, exclude, branch, expected) in [
            (&["~ALL"][..], &[][..], "main", Targeting::Targets),
            (&["~DEFAULT_BRANCH"], &[], "main", Targeting::Targets),
            (&["~DEFAULT_BRANCH"], &[], "dev", Targeting::Misses),
            (&["refs/heads/main"], &[], "main", Targeting::Targets),
            (&["refs/heads/ma*"], &[], "main", Targeting::Targets),
            (&["refs/heads/m?in"], &[], "main", Targeting::Targets),
            (&["refs/heads/release/*"], &[], "main", Targeting::Misses),
            (&["refs/heads/**"], &[], "release/x", Targeting::Misses),
            (&["refs/heads/**"], &[], "main", Targeting::Targets),
            (
                &["refs/heads/qa**/**/*"],
                &[],
                "qa/foo/bar",
                Targeting::Targets,
            ),
            (&["~ALL"], &["refs/heads/m*"], "main", Targeting::Misses),
            (&["~ALL"], &["~DEFAULT_BRANCH"], "main", Targeting::Misses),
            (&["~ALL"], &["refs/heads/dev"], "main", Targeting::Targets),
            (
                &["refs/heads/integration/**"],
                &[],
                "integration/**",
                Targeting::Targets,
            ),
        ] {
            assert_eq!(
                ruleset_targets(&ruleset("active", include, exclude), branch, main),
                expected,
                "{include:?} {exclude:?} {branch}"
            );
        }
        assert!(matches!(
            ruleset_targets(&ruleset("active", &["refs/heads/[^m]*"], &[]), "main", main),
            Targeting::Unexplained(why) if why.contains("[^m]*")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_gh_answer_larger_than_a_pipe_does_not_block_a_large_input() {
        // `cat` stands in for `gh`: it echoes its stdin as it reads.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let input = "x".repeat(4 * 1024 * 1024);
        let expected = input.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(run_gh(Path::new("cat"), &repo, &[], Some(&input)));
        });
        let out = receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("run_gh finished instead of deadlocking on the pipe")
            .unwrap();
        assert_eq!(out, expected);
    }

    // -----------------------------------------------------------------------
    // A recorded host in memory (TSK-1080): rulesets by id, the default
    // branch's workflows, and every write. It stands in for GitHub's REST
    // answers; the hosted fixture on a scratch repository is owed.
    // -----------------------------------------------------------------------

    /// The policy workflow TSK-336 lands, reduced to what B and C read.
    const POLICY_WITH_TRUSTED: &str = r#"
name: codeflow-policy
on: pull_request_target
jobs:
  commit-standards:
    name: 'trusted: commit standards'
    runs-on: ubuntu-24.04
  secret-scan:
    name: 'trusted: secret scan'
    runs-on: ubuntu-24.04
  release-impact:
    name: 'trusted: release impact'
    runs-on: ubuntu-24.04
  authority:
    environment: codeflow-authority
    runs-on: ubuntu-24.04
    steps:
      - run: "create 'authority: commit standards' 'authority: secret scan' 'authority: release impact'"
"#;

    /// Today's policy workflow: one job named `commit standards`.
    const POLICY_TODAY: &str = r"
name: codeflow-policy
on: pull_request_target
jobs:
  policy:
    name: commit standards
    runs-on: ubuntu-24.04
";

    const CI_WORKFLOW: &str = r"
name: codeflow-ci
on: pull_request
jobs:
  gates:
    name: codeflow gates
  security-review:
    name: security review
  windows-verdict:
    name: windows
";

    #[derive(Default)]
    struct Fake {
        rulesets: RefCell<BTreeMap<u64, Value>>,
        workflows: Vec<(String, String)>,
        writes: RefCell<Vec<(String, Value)>>,
        /// When set, a write answers this bypass list instead of the one
        /// it was sent.
        answer_bypass: Option<Value>,
    }

    impl Fake {
        fn new(workflows: &[(&str, &str)], rulesets: &[Value]) -> Rc<Self> {
            Rc::new(Self {
                rulesets: RefCell::new(
                    rulesets
                        .iter()
                        .map(|r| (r["id"].as_u64().unwrap(), r.clone()))
                        .collect(),
                ),
                workflows: workflows
                    .iter()
                    .map(|(p, t)| ((*p).to_string(), (*t).to_string()))
                    .collect(),
                ..Self::default()
            })
        }

        fn standard(rulesets: &[Value]) -> Rc<Self> {
            Self::new(
                &[
                    (POLICY_WORKFLOW, POLICY_WITH_TRUSTED),
                    (".github/workflows/codeflow-ci.yml", CI_WORKFLOW),
                ],
                rulesets,
            )
        }

        fn handle(&self, args: &[&str], input: Option<&str>) -> Result<String, String> {
            match args {
                ["repo", "view", ..] => Ok(
                    r#"{"nameWithOwner":"o/r","isPrivate":false,"defaultBranchRef":{"name":"main"}}"#
                        .into(),
                ),
                ["api", url] if url.starts_with("repos/o/r/rulesets?") => {
                    let list: Vec<Value> = self
                        .rulesets
                        .borrow()
                        .values()
                        .map(|r| {
                            json!({
                                "id": r["id"], "name": r["name"],
                                "target": r.get("target").cloned().unwrap_or(json!("branch")),
                                "source_type": r.get("source_type").cloned().unwrap_or(json!("Repository")),
                            })
                        })
                        .collect();
                    Ok(Value::Array(list).to_string())
                }
                ["api", url] if url.starts_with("repos/o/r/rulesets/") => {
                    let id: u64 = url["repos/o/r/rulesets/".len()..].parse().unwrap();
                    self.rulesets
                        .borrow()
                        .get(&id)
                        .map(Value::to_string)
                        .ok_or_else(|| "gh: Not Found (HTTP 404)".to_string())
                }
                ["api", "repos/o/r/contents/.github/workflows?ref=main"] => {
                    if self.workflows.is_empty() {
                        return Err("gh: Not Found (HTTP 404)".into());
                    }
                    Ok(Value::Array(
                        self.workflows
                            .iter()
                            .map(|(path, _)| json!({ "path": path, "type": "file" }))
                            .collect(),
                    )
                    .to_string())
                }
                ["api", "-H", "Accept: application/vnd.github.raw", url] => self
                    .workflows
                    .iter()
                    .find(|(path, _)| *url == format!("repos/o/r/contents/{path}?ref=main"))
                    .map(|(_, text)| text.clone())
                    .ok_or_else(|| format!("gh: Not Found (HTTP 404) {url}")),
                ["api", "-X", method, path, "--input", "-"] => {
                    let mut body: Value = serde_json::from_str(input.unwrap()).unwrap();
                    self.writes
                        .borrow_mut()
                        .push((format!("{method} {path}"), body.clone()));
                    let id = if *method == "POST" {
                        1000 + self.rulesets.borrow().len() as u64
                    } else {
                        path.rsplit('/').next().unwrap().parse().unwrap()
                    };
                    body["id"] = json!(id);
                    body["source_type"] = json!("Repository");
                    self.rulesets.borrow_mut().insert(id, body.clone());
                    if let Some(bypass) = &self.answer_bypass {
                        body["bypass_actors"] = bypass.clone();
                    }
                    Ok(body.to_string())
                }
                other => Err(format!("unexpected gh call {other:?}")),
            }
        }

        fn provider(self: &Rc<Self>) -> GithubProvider {
            let host = Rc::clone(self);
            GithubProvider::with_transport(Box::new(move |args, input| host.handle(args, input)))
        }

        fn writes(&self) -> Vec<(String, Value)> {
            self.writes.borrow().clone()
        }

        /// The write sent for the ruleset named `name`.
        fn body(&self, name: &str) -> Value {
            self.writes()
                .into_iter()
                .rev()
                .find(|(_, body)| body["name"] == name)
                .unwrap_or_else(|| panic!("no write for {name}: {:?}", self.writes()))
                .1
        }
    }

    fn apply_authority(host: &Rc<Fake>, authority: AuthorityPlan) -> ProtectReport {
        let plan = ProtectionPlan {
            rules: Vec::new(),
            authority: Some(authority),
        };
        host.provider().apply(&plan)
    }

    fn check_pins(body: &Value) -> Vec<(String, u64)> {
        body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "required_status_checks")
            .unwrap()["parameters"]["required_status_checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["context"].as_str().unwrap().to_string(),
                    c["integration_id"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    fn rule_of<'a>(body: &'a Value, kind: &str) -> &'a Value {
        body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == kind)
            .unwrap_or_else(|| panic!("no {kind} rule in {body}"))
    }

    fn pins(names: &[(&str, u64)]) -> Vec<(String, u64)> {
        names.iter().map(|(n, a)| ((*n).to_string(), *a)).collect()
    }

    /// AC-1: the bodies of A, B and C for each profile and check identity.
    #[test]
    fn the_three_rulesets_per_profile_and_mode() {
        for (profile, app) in [
            (Profile::SoloAdministrator, None),
            (Profile::SoloAdministrator, Some(4242)),
            (Profile::IndependentReview, None),
            (Profile::IndependentReview, Some(4242)),
        ] {
            let host = Fake::standard(&[]);
            let report = apply_authority(
                &host,
                AuthorityPlan::new(profile, app, DEFAULT_LINES).unwrap(),
            );
            let text = report.render();
            assert_eq!(report.status, ProtectStatus::Applied, "{text:?}");
            let case = format!("{profile:?} {app:?}");

            // Every write is a POST of the full body.
            for (call, body) in host.writes() {
                assert_eq!(call, "POST repos/o/r/rulesets", "{case}");
                for field in [
                    "name",
                    "target",
                    "enforcement",
                    "conditions",
                    "bypass_actors",
                    "rules",
                ] {
                    assert!(
                        body.get(field).is_some(),
                        "{case}: {field} missing in {body}"
                    );
                }
                assert_eq!(body["enforcement"], "active", "{case}");
                assert_eq!(body["target"], "branch", "{case}");
            }

            let a = host.body(RULESET_AUTHORITY_UPDATES);
            assert_eq!(
                a["conditions"]["ref_name"]["include"],
                json!(["~DEFAULT_BRANCH"])
            );
            assert_eq!(
                a["rules"],
                json!([{ "type": "update", "parameters": { "update_allows_fetch_and_merge": false } }]),
                "{case}"
            );
            let role = if profile == Profile::SoloAdministrator {
                5
            } else {
                2
            };
            assert_eq!(
                a["bypass_actors"],
                json!([{ "actor_id": role, "actor_type": "RepositoryRole", "bypass_mode": "always" }]),
                "{case}"
            );

            let b = host.body(RULESET_AUTHORITY_CHECKS);
            assert_eq!(
                b["conditions"]["ref_name"]["include"],
                json!(["~DEFAULT_BRANCH"])
            );
            assert_eq!(b["bypass_actors"], json!([]), "{case}");
            let pr = &rule_of(&b, "pull_request")["parameters"];
            let independent = profile == Profile::IndependentReview;
            assert_eq!(
                pr["required_approving_review_count"],
                u64::from(independent)
            );
            assert_eq!(pr["dismiss_stale_reviews_on_push"], independent);
            assert_eq!(pr["require_last_push_approval"], independent);
            assert_eq!(pr["require_code_owner_review"], false);
            let checks = &rule_of(&b, "required_status_checks")["parameters"];
            assert_eq!(checks["strict_required_status_checks_policy"], true);
            let policy = match app {
                Some(app) => pins(&[
                    ("authority: commit standards", app),
                    ("authority: release impact", app),
                    ("authority: secret scan", app),
                ]),
                None => pins(&[
                    ("trusted: commit standards", 15368),
                    ("trusted: release impact", 15368),
                    ("trusted: secret scan", 15368),
                ]),
            };
            let mut expected = policy;
            expected.extend(pins(&[
                ("codeflow gates", 15368),
                ("security review", 15368),
                ("windows", 15368),
            ]));
            assert_eq!(check_pins(&b), expected, "{case}");
            rule_of(&b, "non_fast_forward");
            rule_of(&b, "deletion");
            assert!(!b["rules"].to_string().contains("\"update\""), "{case}");

            let c = host.body(RULESET_LINE_CHECKS);
            assert_eq!(
                c["conditions"]["ref_name"]["include"],
                json!(["refs/heads/integration/**"])
            );
            assert_eq!(c["bypass_actors"], json!([]), "{case}");
            let pr = &rule_of(&c, "pull_request")["parameters"];
            assert_eq!(pr["required_approving_review_count"], 0);
            assert_eq!(
                rule_of(&c, "required_status_checks")["parameters"]
                    ["strict_required_status_checks_policy"],
                true
            );
            assert_eq!(
                check_pins(&c),
                pins(&[
                    ("trusted: commit standards", 15368),
                    ("trusted: release impact", 15368),
                    ("trusted: secret scan", 15368),
                    ("codeflow gates", 15368),
                    ("windows", 15368),
                ]),
                "{case}"
            );
            rule_of(&c, "non_fast_forward");
            rule_of(&c, "deletion");
            let kinds: Vec<&str> = c["rules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["type"].as_str().unwrap())
                .collect();
            assert!(
                !kinds.contains(&"update") && !kinds.contains(&"creation"),
                "{kinds:?}"
            );
        }
    }

    /// AC-1: B and C are refused while the default branch's policy
    /// workflow does not declare the names they require; A is still
    /// written.
    #[test]
    fn b_and_c_are_refused_until_the_policy_workflow_declares_their_names() {
        for (workflows, why) in [
            (
                vec![
                    (POLICY_WORKFLOW, POLICY_TODAY),
                    (".github/workflows/codeflow-ci.yml", CI_WORKFLOW),
                ],
                "does not declare trusted: commit standards, trusted: secret scan",
            ),
            (
                vec![(".github/workflows/codeflow-ci.yml", CI_WORKFLOW)],
                "is not on the default branch",
            ),
            (Vec::new(), "is not on the default branch"),
            (
                vec![(POLICY_WORKFLOW, POLICY_WITH_TRUSTED)],
                "no workflow on the default branch declares codeflow gates",
            ),
        ] {
            let host = Fake::new(&workflows, &[]);
            let report = apply_authority(&host, solo());
            let text = report.render();
            assert_eq!(report.status, ProtectStatus::Degraded, "{text:?}");
            let names: Vec<String> = host
                .writes()
                .iter()
                .map(|(_, body)| body["name"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(names, [RULESET_AUTHORITY_UPDATES], "{text:?}");
            for name in [RULESET_AUTHORITY_CHECKS, RULESET_LINE_CHECKS] {
                assert!(
                    report
                        .limitations
                        .iter()
                        .any(|l| l.contains(name) && l.contains(why)),
                    "{why}: {text:?}"
                );
            }
        }
        // Mode A needs the authority: names in the policy workflow too; C
        // does not use them and is written.
        let mut policy = POLICY_WITH_TRUSTED.to_string();
        policy = policy.replace("'authority: secret scan'", "");
        let host = Fake::new(
            &[
                (POLICY_WORKFLOW, policy.as_str()),
                (".github/workflows/codeflow-ci.yml", CI_WORKFLOW),
            ],
            &[],
        );
        let report = apply_authority(
            &host,
            AuthorityPlan::new(Profile::SoloAdministrator, Some(4242), DEFAULT_LINES).unwrap(),
        );
        let text = report.render();
        assert!(
            text.contains("does not create authority: secret scan"),
            "{text:?}"
        );
        let names: Vec<String> = host
            .writes()
            .iter()
            .map(|(_, body)| body["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, [RULESET_AUTHORITY_UPDATES, RULESET_LINE_CHECKS]);
    }

    fn foreign() -> Vec<Value> {
        vec![
            serde_json::from_str(LIVE_RULESET).unwrap(),
            serde_json::from_str(LIVE_LINE_RULESET).unwrap(),
        ]
    }

    /// AC-2 and AC-6: with foreign rulesets on both targets, the first run
    /// creates A, B and C beside them and never writes them; the second
    /// run writes nothing, and the foreign rulesets are as they were.
    #[test]
    fn foreign_rulesets_are_untouched_and_a_second_run_changes_nothing() {
        let host = Fake::standard(&foreign());
        let first = apply_authority(&host, solo());
        assert_eq!(first.status, ProtectStatus::Applied, "{:?}", first.render());
        let calls: Vec<String> = host.writes().into_iter().map(|(call, _)| call).collect();
        assert_eq!(calls, ["POST repos/o/r/rulesets"; 3]);
        let after_first = host.rulesets.borrow().clone();

        let second = apply_authority(&host, solo());
        let text = second.render();
        assert_eq!(second.status, ProtectStatus::Applied, "{text:?}");
        assert_eq!(host.writes().len(), 3, "the second run wrote: {text:?}");
        assert_eq!(
            text.matches("already as intended, nothing written").count(),
            3,
            "{text:?}"
        );
        assert_eq!(*host.rulesets.borrow(), after_first);
        for live in foreign() {
            let id = live["id"].as_u64().unwrap();
            assert_eq!(host.rulesets.borrow()[&id], live);
        }
    }

    /// A ruleset of B's name as an administrator left it after a recovery:
    /// two approvals, an extra rule and check, the administrator on its
    /// bypass list, `evaluate` enforcement and an exclude.
    fn stronger_b() -> Value {
        json!({
            "id": 77, "name": RULESET_AUTHORITY_CHECKS, "target": "branch",
            "source_type": "Repository", "enforcement": "evaluate",
            "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH", "refs/heads/release"], "exclude": ["refs/heads/main"] } },
            "bypass_actors": [{ "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }],
            "rules": [
                { "type": "pull_request", "parameters": {
                    "required_approving_review_count": 2, "dismiss_stale_reviews_on_push": true,
                    "require_code_owner_review": true, "require_last_push_approval": false,
                    "required_review_thread_resolution": true, "allowed_merge_methods": ["merge"] } },
                { "type": "required_linear_history" },
                { "type": "required_status_checks", "parameters": {
                    "strict_required_status_checks_policy": false, "do_not_enforce_on_create": true,
                    "required_status_checks": [
                        { "context": "lint", "integration_id": 15368 },
                        { "context": "trusted: secret scan" }
                    ] } }
            ]
        })
    }

    /// AC-2: an update of an owned ruleset keeps everything stronger,
    /// adds what is missing, and closes the recovery window by writing an
    /// empty bypass list.
    #[test]
    fn an_owned_ruleset_is_only_strengthened() {
        let host = Fake::standard(&[stronger_b()]);
        let report = apply_authority(&host, solo());
        assert_eq!(
            report.status,
            ProtectStatus::Applied,
            "{:?}",
            report.render()
        );
        let (call, b) = host
            .writes()
            .into_iter()
            .find(|(_, body)| body["name"] == RULESET_AUTHORITY_CHECKS)
            .unwrap();
        assert_eq!(call, "PUT repos/o/r/rulesets/77");
        assert_eq!(b["enforcement"], "active");
        assert_eq!(
            b["conditions"]["ref_name"],
            json!({ "include": ["~DEFAULT_BRANCH", "refs/heads/release"], "exclude": [] })
        );
        assert_eq!(b["bypass_actors"], json!([]), "the window is closed");
        let pr = &rule_of(&b, "pull_request")["parameters"];
        assert_eq!(pr["required_approving_review_count"], 2, "never lowered");
        assert_eq!(pr["require_code_owner_review"], true);
        assert_eq!(pr["required_review_thread_resolution"], true);
        assert_eq!(pr["dismiss_stale_reviews_on_push"], true);
        assert_eq!(pr["allowed_merge_methods"], json!(["merge"]));
        rule_of(&b, "required_linear_history");
        let checks = &rule_of(&b, "required_status_checks")["parameters"];
        assert_eq!(checks["strict_required_status_checks_policy"], true);
        assert_eq!(checks["do_not_enforce_on_create"], false);
        let pinned = check_pins(&b);
        assert_eq!(pinned[0], ("lint".to_string(), 15368), "kept first");
        assert_eq!(
            pinned[1],
            ("trusted: secret scan".to_string(), 15368),
            "an unpinned check is pinned"
        );
        assert_eq!(pinned.len(), 7, "{pinned:?}");
        rule_of(&b, "non_fast_forward");
        rule_of(&b, "deletion");
    }

    /// AC-2: A's bypass list is never widened: an existing list, even one
    /// that differs from the profile, is written back as it is.
    #[test]
    fn a_keeps_its_live_bypass_list() {
        for live in [
            json!([]),
            json!([
                { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" },
                { "actor_id": 9, "actor_type": "Team", "bypass_mode": "pull_request" }
            ]),
        ] {
            let a = json!({
                "id": 50, "name": RULESET_AUTHORITY_UPDATES, "target": "branch",
                "enforcement": "active",
                "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
                "bypass_actors": live,
                "rules": [{ "type": "update", "parameters": { "update_allows_fetch_and_merge": true } }]
            });
            let host = Fake::standard(&[a]);
            apply_authority(&host, solo());
            let written = host.body(RULESET_AUTHORITY_UPDATES);
            assert_eq!(written["bypass_actors"], live);
            assert_eq!(
                rule_of(&written, "update")["parameters"]["update_allows_fetch_and_merge"],
                false
            );
        }
        // Without a readable list it is not written at all.
        let a = json!({
            "id": 50, "name": RULESET_AUTHORITY_UPDATES, "target": "branch",
            "enforcement": "active", "rules": []
        });
        let host = Fake::standard(&[a]);
        let report = apply_authority(&host, solo());
        assert!(host
            .writes()
            .iter()
            .all(|(_, body)| body["name"] != RULESET_AUTHORITY_UPDATES));
        assert!(
            report.render().contains("bypass list is not readable"),
            "{:?}",
            report.render()
        );
    }

    /// AC-2: an owned ruleset is not written when the update would remove
    /// a rule or change a check's producer, nor when its name is ambiguous
    /// or taken by a ruleset that is not a branch ruleset.
    #[test]
    fn an_owned_ruleset_that_cannot_be_strengthened_is_not_written() {
        let mut with_update = stronger_b();
        with_update["rules"].as_array_mut().unwrap().push(
            json!({ "type": "update", "parameters": { "update_allows_fetch_and_merge": false } }),
        );
        let mut repinned = stronger_b();
        repinned["rules"][2]["parameters"]["required_status_checks"][0] =
            json!({ "context": "codeflow gates", "integration_id": 99 });
        let mut tag = stronger_b();
        tag["target"] = json!("tag");
        let mut twin = stronger_b();
        twin["id"] = json!(78);
        for (rulesets, why) in [
            (vec![with_update], "carries an `update` rule"),
            (vec![repinned], "pins `codeflow gates` to app 99"),
            (vec![tag], "not a branch ruleset"),
            (vec![stronger_b(), twin], "2 rulesets are named"),
        ] {
            let host = Fake::standard(&rulesets);
            let report = apply_authority(&host, solo());
            let text = report.render();
            assert_eq!(report.status, ProtectStatus::Degraded, "{text:?}");
            assert!(
                host.writes()
                    .iter()
                    .all(|(_, body)| body["name"] != RULESET_AUTHORITY_CHECKS),
                "{why}: {:?}",
                host.writes()
            );
            assert!(text.contains(why), "{why}: {text:?}");
        }
    }

    /// The bypass list the host answers after a write is checked as a
    /// field.
    #[test]
    fn a_bypass_list_the_host_answers_differently_is_reported() {
        let host = Rc::new(Fake {
            answer_bypass: Some(
                json!([{ "actor_id": 4, "actor_type": "RepositoryRole", "bypass_mode": "always" }]),
            ),
            ..Rc::try_unwrap(Fake::standard(&[])).ok().unwrap()
        });
        let report = apply_authority(&host, solo());
        let text = report.render();
        assert_eq!(report.status, ProtectStatus::Degraded, "{text:?}");
        assert!(
            text.contains("the host answered with the bypass list"),
            "{text:?}"
        );
    }

    /// A line pattern from `--lines` is C's target.
    #[test]
    fn c_targets_the_line_pattern() {
        let host = Fake::standard(&[]);
        apply_authority(
            &host,
            AuthorityPlan::new(Profile::SoloAdministrator, None, "release/**").unwrap(),
        );
        assert_eq!(
            host.body(RULESET_LINE_CHECKS)["conditions"]["ref_name"]["include"],
            json!(["refs/heads/release/**"])
        );
    }
}

#[cfg(test)]
mod r16_obtaining_regressions {

    #[test]
    fn r16_protection_rejects_wrong_field_types() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        for text in [
            r#"{"git":{"protected_branches":["main",7]}}"#,
            r#"{"git":{"push_to_protected":"unknown"}}"#,
            r#"{"git":[]}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(
                super::ProtectionPlan::from_policy_file(&path).is_err(),
                "{text}"
            );
        }
    }
}
