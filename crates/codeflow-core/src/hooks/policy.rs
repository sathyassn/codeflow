//! Typed view of the `policy.json` `git` section (charter §6.1).
//!
//! Single source of truth for all four enforcement planes (D7): git client
//! hooks, the Claude `git-guard` hook, remote protection, and CI all read
//! these values. Every field is a default the user may flip per repo;
//! missing or malformed files fall back to the strict charter defaults so
//! enforcement never silently disables itself.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::security::SecurityPolicy;
use crate::security::git::is_on_protected_branch;

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
    pub commit_format: PolicyLevel,
    pub commit_types: Vec<String>,
    pub ai_attribution: PolicyLevel,
    pub commit_emoji: PolicyLevel,
    pub branch_naming: PolicyLevel,
    pub branch_prefixes: Vec<String>,
    pub secret_scan: PolicyLevel,
    pub test_gate_on_push: PolicyLevel,
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
            commit_format: PolicyLevel::Block,
            commit_types: [
                "feat", "fix", "docs", "refactor", "test", "chore", "ci", "perf", "build",
                "revert",
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
            ai_attribution: PolicyLevel::Block,
            commit_emoji: PolicyLevel::Block,
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
                "spike/",
                "experiment/",
                "integration/",
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
            secret_scan: PolicyLevel::Block,
            test_gate_on_push: PolicyLevel::Warn,
        }
    }
}

impl GitPolicy {
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

/// Full `.codeflow/policy.json` shape (only the parts the hook plane reads).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub schema_version: u32,
    pub git: GitPolicy,
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
    /// During bootstrap grace (`policy_armed = false` in
    /// `.codeflow/project.toml`, charter AC #1: the first hour has zero
    /// policy walls) every rule except `secret_scan` is suspended — secrets
    /// are never graced (charter §6.3, the `--minimal` floor). Returns the
    /// effective policy and whether the policy is armed.
    #[must_use]
    pub fn load_effective(root: &Path) -> (Self, bool) {
        let mut policy = Self::load(root);
        let armed = policy_armed(root);
        if !armed {
            policy.git.suspend_for_bootstrap();
        }
        (policy, armed)
    }
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
        self.commit_format = PolicyLevel::Off;
        self.ai_attribution = PolicyLevel::Off;
        self.commit_emoji = PolicyLevel::Off;
        self.branch_naming = PolicyLevel::Off;
        self.test_gate_on_push = PolicyLevel::Off;
    }
}

/// Read `policy_armed` from `.codeflow/project.toml`.
///
/// Absent file, absent key, or parse failure all mean **armed** — the
/// fail-safe direction. Init writes `policy_armed = false`, makes the
/// scaffold commit, then flips it to `true`.
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
        assert_eq!(g.ai_attribution, PolicyLevel::Block);
        assert_eq!(g.commit_emoji, PolicyLevel::Block);
        assert_eq!(g.branch_naming, PolicyLevel::Block);
        assert_eq!(g.branch_prefixes.len(), 12);
        assert_eq!(g.secret_scan, PolicyLevel::Block);
        assert_eq!(g.test_gate_on_push, PolicyLevel::Warn);
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
        assert_eq!(from_asset.git.branch_prefixes, defaults.branch_prefixes);
        assert_eq!(from_asset.git.test_gate_on_push, defaults.test_gate_on_push);
        assert_eq!(from_asset.git.merge_to_protected, defaults.merge_to_protected);
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
        // Secrets are never graced (charter §6.3).
        assert_eq!(policy.git.secret_scan, PolicyLevel::Block);
    }

    #[test]
    fn test_load_effective_armed_is_strict() {
        let dir = tempfile::tempdir().unwrap();
        let (policy, armed) = Policy::load_effective(dir.path());
        assert!(armed);
        assert_eq!(policy.git.commit_to_protected, PolicyLevel::Block);
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
