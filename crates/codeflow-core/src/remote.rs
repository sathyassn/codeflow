//! `codeflow remote protect` — apply `policy.json` branch protection to the
//! remote provider (charter §6.1 plane 3, D7, D19).
//!
//! The remote is part of the authoritative enforcement perimeter: require
//! PR + status checks, block force-pushes and deletions, for every pattern
//! in `git.protected_branches`. Exact branch names use the branch-protection
//! API; glob patterns use repository rulesets (which support fnmatch).
//!
//! Required status checks are always strict: a pull request merges only on
//! checks that ran on a branch up to date with its base, so two pull
//! requests each green on an older base cannot both land untested together
//! (TSK-261). The check names come from `git.required_checks`, defaulting to
//! the shipped CI job names. The live rules are read before anything is
//! written: a repository ruleset that already targets the branch is updated
//! in place, never duplicated, and a classic protection PUT carries the
//! settings this command does not own.
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

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::hooks::policy::DEFAULT_REQUIRED_CHECKS;

/// The GitHub App id of GitHub Actions. Ruleset status checks pin it, so a
/// required check passes only when GitHub Actions reported it, as the
/// shipped workflows do; a check without an integration id would accept
/// the same context from any source.
pub const GITHUB_ACTIONS_APP_ID: u64 = 15368;

/// Rulesets read per page when looking for the one that targets a branch.
const RULESET_PAGE_SIZE: usize = 100;

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
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        if !self.limitations.is_empty() {
            out.push_str("\nNot applied:\n");
            for l in &self.limitations {
                out.push_str("  - ");
                out.push_str(l);
                out.push('\n');
            }
        }
        if !self.checklist.is_empty() {
            out.push_str("\nManual checklist (GitHub: Settings -> Branches / Rules):\n");
            for c in &self.checklist {
                out.push_str("  ");
                out.push_str(c);
                out.push('\n');
            }
        }
        let _ = write!(out, "\nstatus: {}\n", self.status.as_str());
        out
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
    /// The registry data profile (SPC-013 R-6, R-22): applied as a ruleset
    /// so it holds before the branch exists and never touches `main`.
    pub data_profile: bool,
    /// The status check names required, on an up-to-date branch, when
    /// [`require_status_checks`](BranchRule::require_status_checks) holds.
    pub required_checks: Vec<String>,
}

impl BranchRule {
    /// Whether the pattern needs glob semantics (rulesets, not classic
    /// branch protection).
    #[must_use]
    pub fn is_glob(&self) -> bool {
        self.pattern.contains(['*', '?', '['])
    }

    /// Whether the rule is applied as a ruleset rather than classic branch
    /// protection.
    #[must_use]
    pub fn uses_ruleset(&self) -> bool {
        self.is_glob() || self.data_profile
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
}

/// The full protection plan derived from `policy.json`.
#[derive(Debug, Clone)]
pub struct ProtectionPlan {
    pub rules: Vec<BranchRule>,
}

impl ProtectionPlan {
    /// Build the plan from a `policy.json` file. Reads
    /// `git.protected_branches` (falling back to a top-level
    /// `protected_branches`, then to `main`/`master`) and derives rule
    /// strictness from the corresponding `git.*` block/warn/allow values
    /// (missing values default to the charter's strict baseline).
    #[must_use]
    pub fn from_policy_file(path: &Path) -> Self {
        let policy: serde_json::Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        let git = policy.get("git").unwrap_or(&serde_json::Value::Null);

        let branches: Vec<String> = git
            .get("protected_branches")
            .or_else(|| policy.get("protected_branches"))
            .and_then(serde_json::Value::as_array)
            .map_or_else(
                || vec!["main".to_string(), "master".to_string()],
                |a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                },
            );

        let blocks = |key: &str| -> bool {
            git.get(key)
                .and_then(serde_json::Value::as_str)
                .is_none_or(|v| v == "block")
        };
        let force = blocks("force_push_protected");
        let delete = blocks("delete_protected");
        let push = blocks("push_to_protected");
        let named: Vec<String> = git
            .get("required_checks")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let required_checks = if named.is_empty() {
            DEFAULT_REQUIRED_CHECKS
                .iter()
                .map(ToString::to_string)
                .collect()
        } else {
            named
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
        Self { rules }
    }

    /// Add the registry data profile when the repository tracks durable
    /// work; the rules for every other branch are unchanged.
    #[must_use]
    pub fn with_registry_profile(mut self) -> Self {
        self.rules.push(BranchRule::registry_data_profile());
        self
    }

    /// Render the dry-run report: the intended rules, nothing applied.
    #[must_use]
    pub fn dry_run_report(&self, provider: &str) -> ProtectReport {
        let mut lines = vec![format!(
            "dry-run: intended remote protection ({provider}), nothing applied"
        )];
        for rule in &self.rules {
            let mechanism = if rule.data_profile {
                "ruleset (data profile)"
            } else if rule.is_glob() {
                "ruleset (glob pattern)"
            } else {
                "branch protection"
            };
            lines.push(format!("{} [{mechanism}]:", rule.pattern));
            for intent in rule.intent_lines() {
                lines.push(format!("  - {intent}"));
            }
        }
        lines.push(
            "where a repository ruleset already targets a branch, that ruleset is updated in place and no second ruleset or branch protection is added"
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
        for rule in &self.rules {
            for intent in rule.intent_lines() {
                items.push(format!("[ ] {}: {intent}", rule.pattern));
            }
        }
        items
    }
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

/// GitHub adapter, shelling out to the `gh` CLI for auth and transport.
pub struct GithubProvider {
    gh: PathBuf,
    repo_dir: PathBuf,
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
        Self {
            gh: gh.into(),
            repo_dir: repo_dir.as_ref().to_path_buf(),
        }
    }

    /// Run `gh` with args, optionally piping `input` to stdin. Returns
    /// stdout on success, combined output as the error string on failure.
    fn run_gh(&self, args: &[&str], input: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new(&self.gh);
        cmd.args(args)
            .current_dir(&self.repo_dir)
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
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{stdout}{stderr}").trim().to_string())
        }
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

    /// Every repository ruleset that is active and already targets
    /// `pattern`, read in full (the list does not carry `enforcement`):
    /// see [`ruleset_targets`]. Empty when there is none. A ruleset that is
    /// `disabled` or only `evaluate`s enforces nothing, so it neither counts
    /// as the branch's ruleset nor stops the search.
    fn rulesets_for(
        &self,
        nwo: &str,
        pattern: &str,
        default_branch: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        // Page until a short page, so a repository with more rulesets than
        // one page holds is still searched in full.
        let mut found = Vec::new();
        let mut page = 1;
        loop {
            let list = self.run_gh(
                &[
                    "api",
                    &format!("repos/{nwo}/rulesets?per_page={RULESET_PAGE_SIZE}&page={page}"),
                ],
                None,
            )?;
            let list: Value = serde_json::from_str(&list).unwrap_or(Value::Null);
            let summaries = list.as_array().map_or(&[][..], Vec::as_slice);
            for summary in summaries {
                let ours = summary.get("target").and_then(Value::as_str) == Some("branch")
                    && summary
                        .get("source_type")
                        .and_then(Value::as_str)
                        .is_none_or(|source| source == "Repository");
                let Some(id) = summary.get("id").and_then(Value::as_u64).filter(|_| ours) else {
                    continue;
                };
                let full = self.run_gh(&["api", &format!("repos/{nwo}/rulesets/{id}")], None)?;
                let full: Value =
                    serde_json::from_str(&full).map_err(|e| format!("ruleset {id} parse: {e}"))?;
                if full.get("enforcement").and_then(Value::as_str) == Some("active")
                    && ruleset_targets(&full, pattern, default_branch)
                {
                    found.push(full);
                }
            }
            if summaries.len() < RULESET_PAGE_SIZE {
                return Ok(found);
            }
            page += 1;
        }
    }

    /// The live classic protection of `branch`, or `None` when the branch
    /// is not protected (HTTP 404).
    fn live_protection(&self, nwo: &str, branch: &str) -> Result<Option<Value>, String> {
        match self.run_gh(
            &["api", &format!("repos/{nwo}/branches/{branch}/protection")],
            None,
        ) {
            Ok(out) => Ok(Some(serde_json::from_str(&out).unwrap_or(Value::Null))),
            Err(e) if e.contains("404") || e.contains("Branch not protected") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The classic branch-protection PUT body. Required checks are strict.
    /// The PUT replaces the whole protection, so every setting this command
    /// does not own is carried from `live` (TSK-261): conversation
    /// resolution, linear history, lock, fork syncing, creation blocking,
    /// push restrictions, the review settings, and checks already required.
    fn branch_protection_body(rule: &BranchRule, live: Option<&Value>) -> String {
        let live = live.unwrap_or(&Value::Null);
        let mut body = json!({
            "required_status_checks": Self::classic_status_checks(rule, live),
            "enforce_admins": true,
            "required_pull_request_reviews": Self::classic_reviews(rule, live),
            "restrictions": Self::classic_restrictions(live),
            "allow_force_pushes": !rule.block_force_push,
            "allow_deletions": !rule.block_deletion,
        });
        for key in [
            "required_conversation_resolution",
            "required_linear_history",
            "lock_branch",
            "allow_fork_syncing",
            "block_creations",
        ] {
            if let Some(enabled) = live
                .pointer(&format!("/{key}/enabled"))
                .and_then(Value::as_bool)
            {
                body[key] = Value::Bool(enabled);
            }
        }
        body.to_string()
    }

    /// The classic `required_status_checks`: strict, with the rule's checks
    /// added to those the live protection already requires (and, where it
    /// pins checks to apps, pinned to GitHub Actions).
    fn classic_status_checks(rule: &BranchRule, live: &Value) -> Value {
        let live_checks = live.get("required_status_checks").filter(|v| v.is_object());
        let mut contexts: Vec<String> = live_checks
            .and_then(|checks| checks.get("contexts"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
        if !rule.requires_checks() {
            return live_checks.map_or(Value::Null, |checks| {
                json!({
                    "strict": checks.get("strict").and_then(Value::as_bool).unwrap_or(false),
                    "contexts": contexts,
                })
            });
        }
        for name in &rule.required_checks {
            if !contexts.contains(name) {
                contexts.push(name.clone());
            }
        }
        let mut body = json!({ "strict": true, "contexts": contexts });
        if let Some(checks) = live_checks
            .and_then(|checks| checks.get("checks"))
            .and_then(Value::as_array)
            .filter(|checks| !checks.is_empty())
        {
            let mut checks = checks.clone();
            for name in &rule.required_checks {
                if !checks
                    .iter()
                    .any(|c| c.get("context").and_then(Value::as_str) == Some(name))
                {
                    checks.push(json!({ "context": name, "app_id": GITHUB_ACTIONS_APP_ID }));
                }
            }
            body["checks"] = Value::Array(checks);
        }
        body
    }

    /// The classic review settings: the live ones when present, else no
    /// required approvals when the rule requires a pull request.
    fn classic_reviews(rule: &BranchRule, live: &Value) -> Value {
        match live
            .get("required_pull_request_reviews")
            .filter(|v| v.is_object())
        {
            Some(reviews) => {
                let mut kept = json!({});
                for key in [
                    "dismiss_stale_reviews",
                    "require_code_owner_reviews",
                    "require_last_push_approval",
                    "required_approving_review_count",
                ] {
                    if let Some(value) = reviews.get(key) {
                        kept[key] = value.clone();
                    }
                }
                // The live object lists people, teams and apps as objects;
                // the PUT takes their logins and slugs.
                for key in ["dismissal_restrictions", "bypass_pull_request_allowances"] {
                    if let Some(list) = reviews.get(key).filter(|v| v.is_object()) {
                        kept[key] = Self::principals(list);
                    }
                }
                kept
            }
            None if rule.require_pr => json!({ "required_approving_review_count": 0 }),
            None => Value::Null,
        }
    }

    /// The live push restrictions in the PUT form, or none.
    fn classic_restrictions(live: &Value) -> Value {
        live.get("restrictions")
            .filter(|v| v.is_object())
            .map_or(Value::Null, Self::principals)
    }

    /// A live `users`/`teams`/`apps` object in the PUT form: logins for
    /// users, slugs for teams and apps.
    fn principals(list: &Value) -> Value {
        let names = |key: &str, field: &str| -> Vec<Value> {
            list.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get(field).cloned())
                .collect()
        };
        json!({
            "users": names("users", "login"),
            "teams": names("teams", "slug"),
            "apps": names("apps", "slug"),
        })
    }

    /// The ruleset rules the plan intends for `rule`.
    fn ruleset_rules(rule: &BranchRule) -> Vec<Value> {
        let mut rules = Vec::new();
        if rule.require_pr {
            rules.push(json!({
                "type": "pull_request",
                "parameters": {
                    "required_approving_review_count": 0,
                    "dismiss_stale_reviews_on_push": false,
                    "require_code_owner_review": false,
                    "require_last_push_approval": false,
                    "required_review_thread_resolution": false
                }
            }));
        }
        if rule.requires_checks() {
            rules.push(json!({
                "type": "required_status_checks",
                "parameters": {
                    "strict_required_status_checks_policy": true,
                    "do_not_enforce_on_create": false,
                    "required_status_checks": rule
                        .required_checks
                        .iter()
                        .map(|name| json!({ "context": name, "integration_id": GITHUB_ACTIONS_APP_ID }))
                        .collect::<Vec<_>>()
                }
            }));
        }
        if rule.block_force_push {
            rules.push(json!({ "type": "non_fast_forward" }));
        }
        if rule.block_deletion {
            rules.push(json!({ "type": "deletion" }));
        }
        rules
    }

    fn ruleset_body(rule: &BranchRule) -> String {
        json!({
            "name": format!("codeflow protect {}", rule.pattern),
            "target": "branch",
            "enforcement": "active",
            "conditions": {
                "ref_name": {
                    "include": [format!("refs/heads/{}", rule.pattern)],
                    "exclude": []
                }
            },
            "rules": Self::ruleset_rules(rule),
        })
        .to_string()
    }

    /// The PUT body that brings an existing ruleset up to `rule`: its own
    /// rules, parameters, name, conditions and bypass list stay; a rule type
    /// it lacks is added; its required status checks become strict and gain
    /// any missing name, pinned to GitHub Actions.
    fn ruleset_update_body(existing: &Value, rule: &BranchRule) -> String {
        let mut rules: Vec<Value> = existing
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for intended in Self::ruleset_rules(rule) {
            let kind = intended
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(current) = rules
                .iter_mut()
                .find(|r| r.get("type").and_then(Value::as_str) == Some(kind))
            else {
                rules.push(intended);
                continue;
            };
            if kind != "required_status_checks" {
                continue;
            }
            let parameters = &mut current["parameters"];
            parameters["strict_required_status_checks_policy"] = Value::Bool(true);
            let mut checks = parameters
                .get("required_status_checks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for name in &rule.required_checks {
                if !checks
                    .iter()
                    .any(|c| c.get("context").and_then(Value::as_str) == Some(name))
                {
                    checks
                        .push(json!({ "context": name, "integration_id": GITHUB_ACTIONS_APP_ID }));
                }
            }
            parameters["required_status_checks"] = Value::Array(checks);
        }
        json!({ "rules": rules }).to_string()
    }

    /// Apply one rule: update the ruleset that already targets it, else
    /// create a ruleset (glob or data profile) or PUT classic protection.
    /// Returns the mechanism applied.
    fn apply_rule(&self, repo: &RepoInfo, rule: &BranchRule) -> Result<String, String> {
        let nwo = &repo.nwo;
        let existing = self.rulesets_for(nwo, &rule.pattern, repo.default_branch.as_deref())?;
        if !existing.is_empty() {
            let mut updated = Vec::new();
            for ruleset in &existing {
                let id = ruleset
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "ruleset without an id".to_string())?;
                let name = ruleset
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                self.run_gh(
                    &[
                        "api",
                        "-X",
                        "PUT",
                        &format!("repos/{nwo}/rulesets/{id}"),
                        "--input",
                        "-",
                    ],
                    Some(&Self::ruleset_update_body(ruleset, rule)),
                )?;
                updated.push(format!("'{name}' ({id})"));
            }
            return Ok(format!("updated ruleset {} in place", updated.join(", ")));
        }
        if rule.uses_ruleset() {
            self.run_gh(
                &[
                    "api",
                    "-X",
                    "POST",
                    &format!("repos/{nwo}/rulesets"),
                    "--input",
                    "-",
                ],
                Some(&Self::ruleset_body(rule)),
            )?;
            return Ok("applied ruleset".to_string());
        }
        let live = self.live_protection(nwo, &rule.pattern)?;
        self.run_gh(
            &[
                "api",
                "-X",
                "PUT",
                &format!("repos/{nwo}/branches/{}/protection", rule.pattern),
                "--input",
                "-",
            ],
            Some(&Self::branch_protection_body(rule, live.as_ref())),
        )?;
        Ok("applied branch protection".to_string())
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
                 branch protection on private repositories requires GitHub Pro and \
                 {visibility}. Options: upgrade the plan, make the repository public, \
                 or apply the rules manually via the checklist below."
            )
        } else {
            format!("{pattern}: not applied — {err}")
        }
    }
}

/// What `gh repo view` says about the repository.
struct RepoInfo {
    nwo: String,
    private: bool,
    default_branch: Option<String>,
}

/// Whether a full ruleset targets `pattern`: one of its includes matches
/// the branch and none of its excludes does. An entry matches when it is
/// `~ALL`, `~DEFAULT_BRANCH` with `pattern` the default branch, the branch
/// ref itself, or an fnmatch pattern the ref `refs/heads/<pattern>` fits
/// (`*` stops at `/`, as GitHub's pathname matching does).
fn ruleset_targets(ruleset: &Value, pattern: &str, default_branch: Option<&str>) -> bool {
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
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let matches = |entry: &&str| match *entry {
        "~ALL" => true,
        "~DEFAULT_BRANCH" => default_branch == Some(pattern),
        entry => {
            entry == reference
                || glob::Pattern::new(entry)
                    .is_ok_and(|fnmatch| fnmatch.matches_with(&reference, options))
        }
    };
    names("include").iter().any(matches) && !names("exclude").iter().any(matches)
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
        let mut limitations = Vec::new();
        let mut failed_rules = Vec::new();

        for rule in &plan.rules {
            match self.apply_rule(&repo, rule) {
                Ok(mechanism) => {
                    lines.push(format!(
                        "{mechanism} for {}: {}",
                        rule.pattern,
                        rule.intent_lines().join(", ")
                    ));
                    if rule.requires_checks() {
                        lines.push(format!(
                            "  note: {} requires these status checks to pass on a branch that is up to date with its base: {} \
                             (git.required_checks, by default the shipped codeflow-ci.yml job names)",
                            rule.pattern,
                            rule.required_checks.join(", ")
                        ));
                    }
                }
                Err(e) => {
                    limitations.push(Self::limitation_for(&rule.pattern, &e, repo.private));
                    failed_rules.push(rule.clone());
                }
            }
        }

        let checklist = if failed_rules.is_empty() {
            Vec::new()
        } else {
            ProtectionPlan {
                rules: failed_rules,
            }
            .manual_checklist()
        };

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
    use super::*;

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
        let plan = ProtectionPlan::from_policy_file(&path);
        assert_eq!(plan.rules.len(), 2);
        assert_eq!(plan.rules[0].pattern, "main");
        assert!(!plan.rules[0].is_glob());
        assert!(plan.rules[1].is_glob());
        assert!(plan.rules[0].require_pr);
        assert!(plan.rules[0].block_force_push);
        assert!(plan.rules[0].block_deletion);
    }

    #[test]
    fn test_plan_missing_policy_defaults_to_main_master() {
        let dir = tempfile::tempdir().unwrap();
        let plan = ProtectionPlan::from_policy_file(&dir.path().join("nope.json"));
        let patterns: Vec<&str> = plan.rules.iter().map(|r| r.pattern.as_str()).collect();
        assert_eq!(patterns, vec!["main", "master"]);
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
        let plan = ProtectionPlan::from_policy_file(&path);
        assert!(!plan.rules[0].block_deletion);
        assert!(plan.rules[0].block_force_push, "unset values stay strict");
    }

    #[test]
    fn test_dry_run_report_prints_plan() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path);
        let report = plan.dry_run_report("github");
        assert_eq!(report.status, ProtectStatus::DryRun);
        let text = report.render();
        assert!(text.contains("main [branch protection]:"), "got:\n{text}");
        assert!(
            text.contains("release/* [ruleset (glob pattern)]:"),
            "got:\n{text}"
        );
        assert!(text.contains("require a pull request before merging"));
        assert!(text.contains("block force pushes"));
        assert!(text.contains("block branch deletion"));
        assert!(text.contains("status: dry-run"));
    }

    #[test]
    fn test_manual_provider_degrades_with_checklist() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let plan = ProtectionPlan::from_policy_file(&path);
        let provider = provider_for("gitlab", dir.path()).unwrap();
        let report = provider.apply(&plan);
        assert_eq!(report.status, ProtectStatus::Degraded);
        assert!(report.checklist.iter().any(|c| c.contains("[ ] main:")));
        assert!(report.limitations[0].contains("gitlab"));
    }

    #[cfg(unix)]
    #[test]
    fn test_github_degraded_on_free_plan_403() {
        let dir = tempfile::tempdir().unwrap();
        let gh = write_shim(
            dir.path(),
            "echo 'HTTP 403: Upgrade to GitHub Pro or make this repository public to enable this feature. (https://docs.github.com/rest/branches/branch-protection)' >&2\nexit 1",
        );
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path);

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
            .any(|c| c.contains("[ ] main: require a pull request")));
        assert!(report
            .checklist
            .iter()
            .any(|c| c.contains("[ ] release/*: block force pushes")));
        let text = report.render();
        assert!(text.contains("status: degraded"), "got:\n{text}");
    }

    #[cfg(unix)]
    #[test]
    fn test_github_applies_when_gh_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let gh = write_shim(dir.path(), "printf '%s' '{}'\nexit 0");
        let path = write_policy(dir.path(), r#"["main", "release/*"]"#);
        let plan = ProtectionPlan::from_policy_file(&path);

        let provider = GithubProvider::with_gh(gh, dir.path());
        let report = provider.apply(&plan);

        assert_eq!(report.status, ProtectStatus::Applied);
        assert!(report.limitations.is_empty());
        assert!(report.checklist.is_empty());
        let text = report.render();
        assert!(
            text.contains("applied branch protection for main"),
            "got:\n{text}"
        );
        assert!(
            text.contains("applied ruleset for release/*"),
            "got:\n{text}"
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
        let plan = ProtectionPlan::from_policy_file(&path);
        let report = GithubProvider::with_gh(gh, dir.path()).apply(&plan);
        assert_eq!(report.status, ProtectStatus::Degraded);
        assert!(!report.checklist.is_empty());
    }

    #[test]
    fn registry_data_profile_is_a_ruleset_that_leaves_main_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let before = ProtectionPlan::from_policy_file(&path);
        let plan = ProtectionPlan::from_policy_file(&path).with_registry_profile();
        assert_eq!(plan.rules.len(), before.rules.len() + 1);
        let main = &plan.rules[0];
        assert_eq!(main.pattern, "main");
        assert!(main.require_pr && !main.data_profile && !main.uses_ruleset());
        let registry = plan.rules.last().unwrap();
        assert_eq!(registry.pattern, "codeflow/registry");
        assert!(registry.uses_ruleset() && registry.block_force_push && registry.block_deletion);
        assert!(!registry.require_pr && !registry.require_status_checks);
        let body: serde_json::Value =
            serde_json::from_str(&GithubProvider::ruleset_body(registry)).unwrap();
        let kinds: Vec<&str> = body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["non_fast_forward", "deletion"]);
        assert_eq!(
            body["conditions"]["ref_name"]["include"][0],
            "refs/heads/codeflow/registry"
        );
        let report = plan.dry_run_report("github").render();
        assert!(
            report.contains("codeflow/registry [ruleset (data profile)]:"),
            "{report}"
        );
    }

    #[test]
    fn test_limitation_for_generic_error() {
        let msg = GithubProvider::limitation_for("main", "HTTP 502: oops", false);
        assert!(msg.contains("main: not applied"));
        assert!(msg.contains("HTTP 502"));
    }

    fn main_rule(required_checks: &[&str]) -> BranchRule {
        BranchRule {
            pattern: "main".into(),
            require_pr: true,
            require_status_checks: true,
            block_force_push: true,
            block_deletion: true,
            data_profile: false,
            required_checks: required_checks.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn test_branch_protection_body_pins_real_ci_contexts() {
        // An empty `contexts` array requires no named check: "require status
        // checks" would be toothless. Pin the shipped codeflow-ci.yml job names,
        // required on an up-to-date branch.
        let rule = main_rule(DEFAULT_REQUIRED_CHECKS);
        let body: serde_json::Value =
            serde_json::from_str(&GithubProvider::branch_protection_body(&rule, None)).unwrap();
        assert_eq!(body["required_status_checks"]["strict"], true);
        let contexts = body["required_status_checks"]["contexts"]
            .as_array()
            .expect("contexts is an array");
        assert!(!contexts.is_empty(), "contexts must not be empty");
        for expected in [
            "codeflow gates",
            "secret scan",
            "security review",
            "commit standards",
        ] {
            assert!(
                contexts.iter().any(|c| c.as_str() == Some(expected)),
                "missing required context {expected}"
            );
        }
    }

    #[test]
    fn an_empty_check_list_emits_no_status_check_rule() {
        let rule = main_rule(&[]);
        let classic: serde_json::Value =
            serde_json::from_str(&GithubProvider::branch_protection_body(&rule, None)).unwrap();
        assert!(classic["required_status_checks"].is_null(), "{classic}");
        let ruleset: serde_json::Value =
            serde_json::from_str(&GithubProvider::ruleset_body(&rule)).unwrap();
        assert!(
            !ruleset["rules"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["type"] == "required_status_checks"),
            "{ruleset}"
        );
    }

    #[test]
    fn the_dry_run_names_strict_checks_from_the_policy_or_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_policy(dir.path(), r#"["main"]"#);
        let text = ProtectionPlan::from_policy_file(&path)
            .dry_run_report("github")
            .render();
        assert!(
            text.contains(
                "require status checks to pass on a branch that is up to date with its base: \
                 codeflow gates, secret scan, security review, commit standards"
            ),
            "{text}"
        );
        assert!(text.contains("updated in place"), "{text}");
        std::fs::write(
            &path,
            r#"{ "git": { "protected_branches": ["main"], "required_checks": ["build", "windows"] } }"#,
        )
        .unwrap();
        let text = ProtectionPlan::from_policy_file(&path)
            .dry_run_report("github")
            .render();
        assert!(
            text.contains("up to date with its base: build, windows"),
            "{text}"
        );
    }

    /// A `gh` stand-in that serves a host's live rules from files and logs
    /// every write: `writes` holds `<METHOD> <path>` per line and `bodies`
    /// the matching request body.
    #[cfg(unix)]
    struct Host {
        dir: tempfile::TempDir,
        gh: PathBuf,
    }

    #[cfg(unix)]
    impl Host {
        fn new(rulesets: &str, by_id: &[(u64, &str)], protection: Option<&str>) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path().display().to_string();
            std::fs::write(dir.path().join("rulesets.json"), rulesets).unwrap();
            for (id, body) in by_id {
                std::fs::write(dir.path().join(format!("ruleset-{id}.json")), body).unwrap();
            }
            if let Some(body) = protection {
                std::fs::write(dir.path().join("protection.json"), body).unwrap();
            }
            let script = format!(
                r#"#!/bin/sh
d='{d}'
case "$*" in
  "repo view"*) printf '%s' '{{"nameWithOwner":"o/r","isPrivate":false,"defaultBranchRef":{{"name":"main"}}}}' ;;
  "repo") exit 0 ;;
  "api -X "*) printf '%s %s\n' "$3" "$4" >> "$d/writes"; cat >> "$d/bodies"; printf '\n' >> "$d/bodies"; printf '{{}}' ;;
  "api repos/o/r/rulesets?per_page=100"*)
    page="${{2##*&page=}}"
    case "$page" in "$2") page=1 ;; esac
    if [ "$page" = 1 ]; then cat "$d/rulesets.json";
    elif [ -f "$d/rulesets-$page.json" ]; then cat "$d/rulesets-$page.json";
    else printf '[]'; fi ;;
  "api repos/o/r/rulesets/"*) cat "$d/ruleset-${{2##*/}}.json" ;;
  "api repos/o/r/branches/"*"/protection")
    if [ -f "$d/protection.json" ]; then cat "$d/protection.json"; else echo 'gh: Branch not protected (HTTP 404)' >&2; exit 1; fi ;;
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
            let plan = ProtectionPlan::from_policy_file(&path);
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
    #[cfg(unix)]
    const LIVE_RULESET: &str = r#"{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":false,"do_not_enforce_on_create":false,"required_status_checks":[{"context":"codeflow gates","integration_id":15368},{"context":"commit standards","integration_id":15368},{"context":"release impact","integration_id":15368},{"context":"secret scan","integration_id":15368},{"context":"security review","integration_id":15368},{"context":"windows","integration_id":15368}]}}],"bypass_actors":[]}"#;

    /// The live classic protection of this repository's `main` on
    /// 2026-10-08: conversation resolution on, no required checks.
    #[cfg(unix)]
    const LIVE_PROTECTION: &str = r#"{"required_pull_request_reviews":{"dismiss_stale_reviews":false,"require_code_owner_reviews":false,"require_last_push_approval":false,"required_approving_review_count":0},"required_signatures":{"enabled":false},"enforce_admins":{"enabled":false},"required_linear_history":{"enabled":false},"allow_force_pushes":{"enabled":false},"allow_deletions":{"enabled":false},"block_creations":{"enabled":false},"required_conversation_resolution":{"enabled":true},"lock_branch":{"enabled":false},"allow_fork_syncing":{"enabled":false}}"#;

    #[cfg(unix)]
    #[test]
    fn a_glob_ruleset_requires_strict_checks_pinned_to_github_actions() {
        let host = Host::new("[]", &[], None);
        let report = host.apply(r#"["release/*"]"#);
        assert_eq!(report.status, ProtectStatus::Applied, "{}", report.render());
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "POST repos/o/r/rulesets");
        let rule = writes[0].1["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "required_status_checks")
            .cloned()
            .unwrap_or_else(|| panic!("no required_status_checks rule: {}", writes[0].1));
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

    #[cfg(unix)]
    #[test]
    fn classic_protection_is_strict_and_keeps_conversation_resolution() {
        let host = Host::new("[]", &[], Some(LIVE_PROTECTION));
        let report = host.apply(r#"["main"]"#);
        assert_eq!(report.status, ProtectStatus::Applied, "{}", report.render());
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "PUT repos/o/r/branches/main/protection");
        let body = &writes[0].1;
        assert_eq!(body["required_conversation_resolution"], true, "{body}");
        assert_eq!(body["required_status_checks"]["strict"], true, "{body}");
        assert_eq!(
            body["required_status_checks"]["contexts"],
            serde_json::json!(DEFAULT_REQUIRED_CHECKS)
        );
    }

    #[cfg(unix)]
    #[test]
    fn classic_protection_keeps_dismissal_restrictions_and_bypass_allowances() {
        let mut live: serde_json::Value = serde_json::from_str(LIVE_PROTECTION).unwrap();
        live["required_pull_request_reviews"]["dismissal_restrictions"] = serde_json::json!({
            "url": "https://api.github.com/x",
            "users": [{ "login": "alice", "id": 1 }],
            "teams": [{ "slug": "leads", "id": 2 }],
            "apps": [{ "slug": "triage-bot", "id": 3 }]
        });
        live["required_pull_request_reviews"]["bypass_pull_request_allowances"] = serde_json::json!({
            "users": [{ "login": "bob", "id": 4 }],
            "teams": [],
            "apps": [{ "slug": "release-bot", "id": 5 }]
        });
        let host = Host::new("[]", &[], Some(&live.to_string()));
        let report = host.apply(r#"["main"]"#);
        assert_eq!(report.status, ProtectStatus::Applied, "{}", report.render());
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        let reviews = &writes[0].1["required_pull_request_reviews"];
        assert_eq!(
            reviews["dismissal_restrictions"],
            serde_json::json!({ "users": ["alice"], "teams": ["leads"], "apps": ["triage-bot"] }),
            "{reviews}"
        );
        assert_eq!(
            reviews["bypass_pull_request_allowances"],
            serde_json::json!({ "users": ["bob"], "teams": [], "apps": ["release-bot"] }),
            "{reviews}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_ruleset_on_a_later_page_is_updated_not_duplicated() {
        let filler: Vec<serde_json::Value> = (1..=RULESET_PAGE_SIZE as u64)
            .map(|id| {
                serde_json::json!({
                    "id": id, "name": format!("tags {id}"), "target": "tag",
                    "source_type": "Repository"
                })
            })
            .collect();
        let host = Host::new(
            &serde_json::to_string(&filler).unwrap(),
            &[(24_379_074, LIVE_RULESET)],
            Some(LIVE_PROTECTION),
        );
        std::fs::write(
            host.dir.path().join("rulesets-2.json"),
            r#"[{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository"}]"#,
        )
        .unwrap();
        let report = host.apply(r#"["main"]"#);
        assert_eq!(report.status, ProtectStatus::Applied, "{}", report.render());
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "PUT repos/o/r/rulesets/24379074");
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_ruleset_is_updated_in_place_and_never_duplicated() {
        let list = r#"[{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository"},{"id":24379101,"name":"release tags","target":"tag","source_type":"Repository"}]"#;
        let host = Host::new(list, &[(24_379_074, LIVE_RULESET)], Some(LIVE_PROTECTION));
        let report = host.apply(r#"["main"]"#);
        let text = report.render();
        assert_eq!(report.status, ProtectStatus::Applied, "{text}");
        assert!(
            text.contains("updated ruleset 'main required checks' (24379074) in place for main"),
            "{text}"
        );
        // One write, to that ruleset: no second ruleset, no classic PUT.
        let writes = host.writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert_eq!(writes[0].0, "PUT repos/o/r/rulesets/24379074");
        let rules = writes[0].1["rules"].as_array().unwrap();
        let kinds: Vec<&str> = rules.iter().map(|r| r["type"].as_str().unwrap()).collect();
        assert_eq!(
            kinds,
            [
                "required_status_checks",
                "pull_request",
                "non_fast_forward",
                "deletion"
            ]
        );
        let checks = &rules[0]["parameters"];
        assert_eq!(checks["strict_required_status_checks_policy"], true);
        // The six live checks stay, with their integration id, and none is added.
        let live: serde_json::Value = serde_json::from_str(LIVE_RULESET).unwrap();
        assert_eq!(
            checks["required_status_checks"],
            live["rules"][0]["parameters"]["required_status_checks"]
        );
        assert!(writes[0].1.get("bypass_actors").is_none());
    }

    /// `LIVE_RULESET` with another id, enforcement and ref conditions.
    #[cfg(unix)]
    fn ruleset_with(id: u64, enforcement: &str, include: &[&str], exclude: &[&str]) -> String {
        let mut ruleset: serde_json::Value = serde_json::from_str(LIVE_RULESET).unwrap();
        ruleset["id"] = id.into();
        ruleset["name"] = format!("ruleset {id}").into();
        ruleset["enforcement"] = enforcement.into();
        ruleset["conditions"]["ref_name"] = serde_json::json!({
            "include": include, "exclude": exclude
        });
        ruleset.to_string()
    }

    /// Apply `main` against rulesets that are all branch rulesets of the
    /// repository; each is `(id, enforcement, include, exclude)`. Returns
    /// the write calls.
    #[cfg(unix)]
    fn writes_against(rulesets: &[(u64, &str, &[&str], &[&str])]) -> Vec<String> {
        let bodies: Vec<(u64, String)> = rulesets
            .iter()
            .map(|(id, enforcement, include, exclude)| {
                (*id, ruleset_with(*id, enforcement, include, exclude))
            })
            .collect();
        let list: Vec<serde_json::Value> = rulesets
            .iter()
            .map(|(id, ..)| {
                serde_json::json!({
                    "id": id, "name": format!("ruleset {id}"), "target": "branch",
                    "source_type": "Repository"
                })
            })
            .collect();
        let by_id: Vec<(u64, &str)> = bodies.iter().map(|(id, b)| (*id, b.as_str())).collect();
        let host = Host::new(&serde_json::to_string(&list).unwrap(), &by_id, None);
        let report = host.apply(r#"["main"]"#);
        assert_eq!(report.status, ProtectStatus::Applied, "{}", report.render());
        host.writes().into_iter().map(|(call, _)| call).collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_disabled_ruleset_listed_first_is_skipped_for_the_active_one() {
        let writes = writes_against(&[
            (1, "disabled", &["refs/heads/main"], &[]),
            (2, "evaluate", &["~DEFAULT_BRANCH"], &[]),
            (3, "active", &["~DEFAULT_BRANCH"], &[]),
        ]);
        assert_eq!(writes, ["PUT repos/o/r/rulesets/3"]);
    }

    #[cfg(unix)]
    #[test]
    fn every_active_ruleset_that_targets_the_branch_is_updated() {
        let writes = writes_against(&[
            (1, "active", &["refs/heads/main"], &[]),
            (2, "disabled", &["~ALL"], &[]),
            (3, "active", &["~ALL"], &[]),
        ]);
        assert_eq!(
            writes,
            ["PUT repos/o/r/rulesets/1", "PUT repos/o/r/rulesets/3"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_ruleset_that_includes_all_branches_is_updated_without_a_classic_put() {
        let writes = writes_against(&[(5, "active", &["~ALL"], &[])]);
        assert_eq!(writes, ["PUT repos/o/r/rulesets/5"]);
    }

    #[cfg(unix)]
    #[test]
    fn an_fnmatch_include_that_matches_the_branch_is_updated() {
        for include in ["refs/heads/ma*", "refs/heads/*", "refs/heads/m?in"] {
            let writes = writes_against(&[(6, "active", &[include], &[])]);
            assert_eq!(writes, ["PUT repos/o/r/rulesets/6"], "{include}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_fnmatch_include_that_does_not_match_the_branch_is_left_alone() {
        for include in ["refs/heads/release/*", "refs/heads/ma", "refs/tags/*"] {
            let writes = writes_against(&[(6, "active", &[include], &[])]);
            assert_eq!(
                writes,
                ["PUT repos/o/r/branches/main/protection"],
                "{include}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_exclude_that_removes_the_branch_leaves_the_ruleset_alone() {
        for exclude in ["refs/heads/main", "refs/heads/m*", "~DEFAULT_BRANCH"] {
            let writes = writes_against(&[(7, "active", &["~ALL"], &[exclude])]);
            assert_eq!(
                writes,
                ["PUT repos/o/r/branches/main/protection"],
                "{exclude}"
            );
        }
        // An exclude of another branch does not.
        let writes = writes_against(&[(7, "active", &["~ALL"], &["refs/heads/dev"])]);
        assert_eq!(writes, ["PUT repos/o/r/rulesets/7"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_gh_answer_larger_than_a_pipe_does_not_block_a_large_input() {
        // `cat` stands in for `gh`: it echoes its stdin as it reads.
        let dir = tempfile::tempdir().unwrap();
        let provider = GithubProvider::with_gh("cat", dir.path());
        let input = "x".repeat(4 * 1024 * 1024);
        let expected = input.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(provider.run_gh(&[], Some(&input)));
        });
        let out = receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("run_gh finished instead of deadlocking on the pipe")
            .unwrap();
        assert_eq!(out, expected);
    }
}
