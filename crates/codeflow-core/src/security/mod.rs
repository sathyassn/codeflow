//! Security scanner for Bash command validation.
//!
//! Imported from v1 (charter section 3.2) and trimmed: the v1 hook-pipeline
//! adapter, session resolution, and `PathFlow` awareness are gone. The scanner
//! is a pure library, and every part left here runs in production:
//!
//! - [`pattern`]: path/glob matching, used by the git-guard
//!   (`hooks/git_guard.rs`) and the scaffold state (`scaffold/state.rs`).
//! - [`git::is_on_protected_branch`]: used by `hooks/policy.rs` and
//!   `integrate.rs`.
//! - [`dangerous::DangerousModule`] and [`privilege::PrivilegeModule`]: run by
//!   the exec-guard (`hooks/exec_guard.rs`, ADR-0008), which builds the only
//!   [`CheckContext`].
//!
//! The unwired v1 modules (`git` command scanning, `path`, `fileops`,
//! `branch`, `tmp`, `network`) and the `SecurityChecker` orchestrator were
//! removed (TSK-137, ADR-0008 amendment of 2026-09-27); recover them from git
//! history if ADR-0008 ever wires one.

pub mod dangerous;
mod deletion;
pub mod git;
#[cfg(test)]
pub(crate) mod guard_forms;
pub mod headless;
pub mod pattern;
pub mod policy;
pub mod privilege;

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
}
