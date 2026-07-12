//! Security scanner for Bash command validation.
//!
//! Imported from v1 (charter section 3.2) and trimmed: the v1 hook-pipeline
//! adapter, session resolution, and `PathFlow` awareness are gone. The scanner
//! is a pure library.
//!
//! ## What is wired to a production plane — and what is not
//!
//! Only these parts run in production today:
//!
//! - [`pattern`] — path/glob matching, used by the git-guard
//!   (`hooks/git_guard.rs`) and the scaffold state (`scaffold/state.rs`).
//! - [`git::is_on_protected_branch`] — used by `hooks/policy.rs` and
//!   `integrate.rs`.
//! - [`dangerous::DangerousModule`] and [`privilege::PrivilegeModule`] — run by
//!   the exec-guard (`hooks/exec_guard.rs`, ADR-0008), which builds the only
//!   production [`CheckContext`].
//!
//! The [`SecurityChecker`] orchestrator, [`default_modules`], and the other six
//! `SecurityModule` impls ([`git::GitModule`], [`path`], [`fileops`],
//! [`branch`], [`tmp`], [`network`]) are currently UNWIRED — no production
//! plane invokes them. The live git protections are an independent
//! implementation in `hooks/git_guard.rs`; do not mistake `GitModule` for
//! active enforcement. They stay here, unit-tested, for the planned
//! exec-guard extension (ADR-0008 wires the modules incrementally).
//!
//! When orchestrated, [`SecurityChecker`] runs 8 modules in priority order
//! (critical first):
//! 1. `dangerous` - Destructive commands (rm -rf /, fork bombs)
//! 2. `privilege` - Privilege escalation (sudo, su, doas)
//! 3. `git` - Git hook bypass, force push, protected branches
//! 4. `path` - Protected path operations
//! 5. `fileops` - Indirect file operations, glob bypass
//! 6. `branch` - File writes on protected branches
//! 7. `tmp` - Managed tmp folder protection
//! 8. `network` - Network operations without sandbox bypass

pub mod branch;
pub mod dangerous;
pub mod fileops;
pub mod git;
pub mod network;
pub mod path;
pub mod pattern;
pub mod policy;
pub mod privilege;
pub mod tmp;

pub use policy::SecurityPolicy;

/// Result of a security module check.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// `true` if the command is permitted.
    pub allow: bool,
    /// Security violation category (e.g., "Dangerous Command").
    pub category: String,
    /// Human-readable explanation.
    pub reason: String,
    /// The specific pattern that triggered the block.
    pub pattern: String,
    /// Name of the module that produced this verdict.
    pub module: String,
}

impl Verdict {
    /// Format the block message for stderr output.
    #[must_use]
    pub fn message(&self) -> String {
        if self.allow {
            return String::new();
        }
        format!(
            "BLOCKED: Security violation detected\n\n\
             Category: {}\n\
             Reason: {}\n\
             Pattern: {}\n\n\
             This is a security restriction enforced by CodeFlow.\n",
            self.category, self.reason, self.pattern
        )
    }
}

/// Context for security module checks.
pub struct CheckContext<'a> {
    /// The bash command being checked.
    pub command: &'a str,
    /// `true` if `dangerouslyDisableSandbox` is set.
    pub sandbox_bypass: bool,
    /// Current git branch name (empty when unknown).
    pub current_branch: &'a str,
    /// Security policy configuration.
    pub policy: &'a SecurityPolicy,
}

/// Trait implemented by each security enforcement module.
pub trait SecurityModule: Send + Sync {
    /// Module name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Evaluate the command against the module's rules.
    ///
    /// Returns `None` if the module has no opinion (allow), or a `Verdict`
    /// with `allow=false` to block.
    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict>;
}

/// Create a blocking verdict.
#[must_use]
pub fn block(category: &str, reason: &str, pattern: &str) -> Verdict {
    Verdict {
        allow: false,
        category: category.into(),
        reason: reason.into(),
        pattern: pattern.into(),
        module: String::new(),
    }
}

/// Return the default set of enforcement modules in execution order.
#[must_use]
pub fn default_modules() -> Vec<Box<dyn SecurityModule>> {
    vec![
        Box::new(dangerous::DangerousModule),
        Box::new(privilege::PrivilegeModule),
        Box::new(git::GitModule),
        Box::new(path::PathModule),
        Box::new(fileops::FileOpsModule),
        Box::new(branch::BranchModule),
        Box::new(tmp::TmpModule),
        Box::new(network::NetworkModule),
    ]
}

/// Orchestrates security enforcement modules.
pub struct SecurityChecker {
    modules: Vec<Box<dyn SecurityModule>>,
}

impl SecurityChecker {
    /// Create a checker with the default set of enforcement modules.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modules: default_modules(),
        }
    }

    /// Run all modules against the given context. Returns the first blocking
    /// verdict, or an allow verdict if all modules pass.
    #[must_use]
    pub fn check(&self, ctx: &CheckContext<'_>) -> Verdict {
        for m in &self.modules {
            if let Some(mut v) = m.check(ctx) {
                if !v.allow {
                    v.module = m.name().to_string();
                    return v;
                }
            }
        }
        Verdict {
            allow: true,
            category: String::new(),
            reason: String::new(),
            pattern: String::new(),
            module: String::new(),
        }
    }
}

impl Default for SecurityChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the current git branch (best-effort, returns empty on failure).
///
/// Reads `.git/HEAD` directly so it works without spawning git. Callers
/// building a [`CheckContext`] use this to populate `current_branch`.
#[must_use]
pub fn current_branch(project_dir: &std::path::Path) -> String {
    let head_path = project_dir.join(".git").join("HEAD");
    match std::fs::read_to_string(head_path) {
        Ok(content) => {
            let trimmed = content.trim();
            if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                branch.to_string()
            } else {
                String::new()
            }
        }
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_helper() {
        let v = block("Test Category", "test reason", "test pattern");
        assert!(!v.allow);
        assert_eq!(v.category, "Test Category");
        assert_eq!(v.reason, "test reason");
        assert_eq!(v.pattern, "test pattern");
    }

    #[test]
    fn test_verdict_message_allow() {
        let v = Verdict {
            allow: true,
            category: String::new(),
            reason: String::new(),
            pattern: String::new(),
            module: String::new(),
        };
        assert_eq!(v.message(), "");
    }

    #[test]
    fn test_verdict_message_block() {
        let v = block("Dangerous Command", "rm -rf /", "rm -rf");
        let msg = v.message();
        assert!(msg.contains("BLOCKED: Security violation detected"));
        assert!(msg.contains("Dangerous Command"));
        assert!(msg.contains("rm -rf /"));
    }

    #[test]
    fn test_default_modules_count() {
        let modules = default_modules();
        assert_eq!(modules.len(), 8);
    }

    #[test]
    fn test_default_modules_names() {
        let modules = default_modules();
        let names: Vec<&str> = modules.iter().map(|m| m.name()).collect();
        assert_eq!(
            names,
            vec![
                "dangerous-commands",
                "privilege-protection",
                "git-protection",
                "path-protection",
                "file-operations",
                "branch-file-protection",
                "tmp-protection",
                "network-protection",
            ]
        );
    }

    #[test]
    fn test_security_checker_allows_safe_command() {
        let policy = SecurityPolicy::defaults();
        let ctx = CheckContext {
            command: "ls -la",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(verdict.allow);
    }

    #[test]
    fn test_security_checker_blocks_rm_rf_root() {
        let policy = SecurityPolicy::defaults();
        let ctx = CheckContext {
            command: "rm -rf /",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(!verdict.allow);
        assert_eq!(verdict.category, "Dangerous Command");
    }

    #[test]
    fn test_security_checker_blocks_sudo() {
        let policy = SecurityPolicy::defaults();
        let ctx = CheckContext {
            command: "sudo rm -rf /tmp/test",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(!verdict.allow);
    }

    #[test]
    fn test_security_checker_records_module_name() {
        let policy = SecurityPolicy::defaults();
        let ctx = CheckContext {
            command: "git commit --no-verify -m 'x'",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        let verdict = SecurityChecker::new().check(&ctx);
        assert!(!verdict.allow);
        assert_eq!(verdict.module, "git-protection");
    }

    #[test]
    fn test_current_branch_no_git() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(current_branch(dir.path()), "");
    }

    #[test]
    fn test_current_branch_with_ref() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/feat/test\n").unwrap();
        assert_eq!(current_branch(dir.path()), "feat/test");
    }

    #[test]
    fn test_current_branch_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "abc123def456\n").unwrap();
        assert_eq!(current_branch(dir.path()), "");
    }
}
