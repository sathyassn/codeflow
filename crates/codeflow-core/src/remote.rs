//! `codeflow remote protect` — apply `policy.json` branch protection to the
//! remote provider (charter §6.1 plane 3, D7, D19).
//!
//! The remote is part of the authoritative enforcement perimeter: require
//! PR + status checks, block force-pushes and deletions, for every pattern
//! in `git.protected_branches`. Exact branch names use the branch-protection
//! API; glob patterns use repository rulesets (which support fnmatch).
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
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
}

impl BranchRule {
    /// Whether the pattern needs glob semantics (rulesets, not classic
    /// branch protection).
    #[must_use]
    pub fn is_glob(&self) -> bool {
        self.pattern.contains(['*', '?', '['])
    }

    fn intent_lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.require_pr {
            v.push("require a pull request before merging".to_string());
        }
        if self.require_status_checks {
            v.push("require status checks to pass before merging".to_string());
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

        let rules = branches
            .into_iter()
            .map(|pattern| BranchRule {
                pattern,
                require_pr: push,
                require_status_checks: push,
                block_force_push: force,
                block_deletion: delete,
            })
            .collect();
        Self { rules }
    }

    /// Render the dry-run report: the intended rules, nothing applied.
    #[must_use]
    pub fn dry_run_report(&self, provider: &str) -> ProtectReport {
        let mut lines = vec![format!(
            "dry-run: intended remote protection ({provider}), nothing applied"
        )];
        for rule in &self.rules {
            let mechanism = if rule.is_glob() {
                "ruleset (glob pattern)"
            } else {
                "branch protection"
            };
            lines.push(format!("{} [{mechanism}]:", rule.pattern));
            for intent in rule.intent_lines() {
                lines.push(format!("  - {intent}"));
            }
        }
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

/// The CI job contexts a codeflow-scaffolded repo exposes — the job `name:`
/// values in the shipped `codeflow-ci.yml`. Pinned as required status checks so
/// a PR cannot merge until these jobs actually run and pass; an empty `contexts`
/// array would require *nothing*, leaving "require status checks" toothless.
const REQUIRED_CI_CONTEXTS: &[&str] =
    &["codeflow gates", "secret scan", "security review", "commit standards"];

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
        let gh = which::which("gh").map_err(|e| {
            format!("gh CLI not found ({e}); install GitHub CLI or use --dry-run")
        })?;
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
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("gh spawn: {e}"))?;
        if let Some(data) = input {
            if let Some(mut stdin) = child.stdin.take() {
                // If gh exits before consuming all of --input (an early auth
                // failure, or a test shim that ignores stdin), the closed read
                // end surfaces as BrokenPipe. That is not itself the verdict —
                // let the child's exit status and stderr decide, rather than
                // racing the write against the child's exit.
                match stdin.write_all(data.as_bytes()) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                    Err(e) => return Err(format!("gh stdin: {e}")),
                }
            }
        }
        let output = child.wait_with_output().map_err(|e| format!("gh: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{stdout}{stderr}").trim().to_string())
        }
    }

    /// Identify the repo (`owner/name`, visibility) via `gh repo view`.
    fn repo_info(&self) -> Result<(String, bool), String> {
        let out = self.run_gh(
            &["repo", "view", "--json", "nameWithOwner,isPrivate"],
            None,
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("gh repo view parse: {e}"))?;
        let nwo = v
            .get("nameWithOwner")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "gh repo view: nameWithOwner missing".to_string())?
            .to_string();
        let private = v
            .get("isPrivate")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Ok((nwo, private))
    }

    fn branch_protection_body(rule: &BranchRule) -> String {
        serde_json::json!({
            "required_status_checks": if rule.require_status_checks {
                serde_json::json!({ "strict": true, "contexts": REQUIRED_CI_CONTEXTS })
            } else {
                serde_json::Value::Null
            },
            "enforce_admins": true,
            "required_pull_request_reviews": if rule.require_pr {
                serde_json::json!({ "required_approving_review_count": 0 })
            } else {
                serde_json::Value::Null
            },
            "restrictions": serde_json::Value::Null,
            "allow_force_pushes": !rule.block_force_push,
            "allow_deletions": !rule.block_deletion,
        })
        .to_string()
    }

    fn ruleset_body(rule: &BranchRule) -> String {
        let mut rules = Vec::new();
        if rule.require_pr {
            rules.push(serde_json::json!({
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
        if rule.block_force_push {
            rules.push(serde_json::json!({ "type": "non_fast_forward" }));
        }
        if rule.block_deletion {
            rules.push(serde_json::json!({ "type": "deletion" }));
        }
        serde_json::json!({
            "name": format!("codeflow protect {}", rule.pattern),
            "target": "branch",
            "enforcement": "active",
            "conditions": {
                "ref_name": {
                    "include": [format!("refs/heads/{}", rule.pattern)],
                    "exclude": []
                }
            },
            "rules": rules,
        })
        .to_string()
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

impl RemoteProvider for GithubProvider {
    fn name(&self) -> &'static str {
        "github"
    }

    fn apply(&self, plan: &ProtectionPlan) -> ProtectReport {
        let (nwo, private) = match self.repo_info() {
            Ok(info) => info,
            Err(e) => {
                return ProtectReport {
                    status: ProtectStatus::Degraded,
                    lines: vec![format!("could not identify the GitHub repo: {e}")],
                    limitations: vec![
                        "no rules were applied; the repo could not be resolved via gh"
                            .to_string(),
                    ],
                    checklist: plan.manual_checklist(),
                };
            }
        };

        let mut lines = vec![format!("repo: {nwo} ({})", if private { "private" } else { "public" })];
        let mut limitations = Vec::new();
        let mut failed_rules = Vec::new();

        for rule in &plan.rules {
            let result = if rule.is_glob() {
                self.run_gh(
                    &["api", "-X", "POST", &format!("repos/{nwo}/rulesets"), "--input", "-"],
                    Some(&Self::ruleset_body(rule)),
                )
            } else {
                self.run_gh(
                    &[
                        "api",
                        "-X",
                        "PUT",
                        &format!("repos/{nwo}/branches/{}/protection", rule.pattern),
                        "--input",
                        "-",
                    ],
                    Some(&Self::branch_protection_body(rule)),
                )
            };
            match result {
                Ok(_) => {
                    let mechanism = if rule.is_glob() { "ruleset" } else { "branch protection" };
                    lines.push(format!(
                        "applied {mechanism} for {}: {}",
                        rule.pattern,
                        rule.intent_lines().join(", ")
                    ));
                    if rule.require_status_checks {
                        lines.push(format!(
                            "  note: {} requires these CI status checks to pass before merge: {} \
                             (the shipped codeflow-ci.yml job names; adjust if yours differ)",
                            rule.pattern,
                            REQUIRED_CI_CONTEXTS.join(", ")
                        ));
                    }
                }
                Err(e) => {
                    limitations.push(Self::limitation_for(&rule.pattern, &e, private));
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
        path
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
        assert!(text.contains("release/* [ruleset (glob pattern)]:"), "got:\n{text}");
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
        assert_eq!(report.limitations.len(), 2);
        assert!(
            report.limitations[0].contains("Upgrade to GitHub Pro"),
            "precise plan limitation expected: {}",
            report.limitations[0]
        );
        assert!(report.limitations[0].contains("private"));
        // Checklist covers every failed rule.
        assert!(report.checklist.iter().any(|c| c.contains("[ ] main: require a pull request")));
        assert!(report.checklist.iter().any(|c| c.contains("[ ] release/*: block force pushes")));
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
        assert!(text.contains("applied branch protection for main"), "got:\n{text}");
        assert!(text.contains("applied ruleset for release/*"), "got:\n{text}");
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
    fn test_limitation_for_generic_error() {
        let msg = GithubProvider::limitation_for("main", "HTTP 502: oops", false);
        assert!(msg.contains("main: not applied"));
        assert!(msg.contains("HTTP 502"));
    }

    #[test]
    fn test_branch_protection_body_pins_real_ci_contexts() {
        // An empty `contexts` array requires no named check — "require status
        // checks" would be toothless. Pin the shipped codeflow-ci.yml job names.
        let rule = BranchRule {
            pattern: "main".into(),
            require_pr: true,
            require_status_checks: true,
            block_force_push: true,
            block_deletion: true,
        };
        let body: serde_json::Value =
            serde_json::from_str(&GithubProvider::branch_protection_body(&rule)).unwrap();
        let contexts = body["required_status_checks"]["contexts"]
            .as_array()
            .expect("contexts is an array");
        assert!(!contexts.is_empty(), "contexts must not be empty");
        for expected in ["codeflow gates", "secret scan", "security review", "commit standards"] {
            assert!(
                contexts.iter().any(|c| c.as_str() == Some(expected)),
                "missing required context {expected}"
            );
        }
    }
}
