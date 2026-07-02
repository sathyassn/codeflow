//! The v2 hook plane (charter §3.3; harness parity ADR-0008): the custom hooks.
//!
//! | Hook | Event | Module |
//! |---|---|---|
//! | `git-guard` | `PreToolUse` (Bash) | [`git_guard`] |
//! | `exec-guard` | `PreToolUse` (Bash) | [`exec_guard`] |
//! | `session-orient` | `SessionStart` | [`orient`] |
//! | `session-summary` | `SessionEnd` | [`session_summary`] |
//! | git-hook shims | pre-commit / commit-msg / pre-merge-commit / reference-transaction / pre-push | [`git_hook`] |
//!
//! The two `PreToolUse` guards are harness-agnostic by design: the payload
//! contract (`tool_name`, `tool_input.command`, `cwd`) is byte-compatible with
//! the Codex hooks engine, so the same `codeflow hook <guard>` binaries bind a
//! Codex session via `.codex/hooks.json` exactly as they bind Claude (ADR-0008).
//!
//! All enforcement levels are read from `.codeflow/policy.json` (charter D7:
//! policy in config, not code) via [`policy::Policy`]; nothing is hardcoded.
//! Every blocking message names the violated policy rule and the sanctioned
//! path (charter §6.2).

pub mod exec_guard;
pub mod git_guard;
pub mod git_hook;
pub mod orient;
pub mod policy;
pub mod repo;
pub mod scan;
pub mod session_summary;
pub mod standards;

pub use policy::{GitPolicy, Policy, PolicyLevel, SecuritySection};
pub use repo::RepoInfo;

/// Environment variable carrying the `codeflow integrate` gate-context token
/// (charter §6.2, D9). When set, the sanctioned local merge path is active
/// and protected-branch commit/push checks step aside. It is a discipline
/// aid, not a security boundary (charter §15) — CI + remote protection are
/// the hard line.
pub const INTEGRATE_TOKEN_ENV: &str = "CODEFLOW_INTEGRATE_TOKEN";

/// Environment variable a **human** exports to override the git-layer merge
/// guard — `CODEFLOW_HUMAN_OVERRIDE=1 git merge …` lets a person land a merge
/// on a protected branch from their own terminal (ADR-0007).
///
/// HUMAN-ONLY by contract: it is honored **only** by the git-client hook
/// plane (which cannot tell who invoked git), never by the Claude `git-guard`
/// layer — an agent does not get to claim humanity. The git-guard actively
/// blocks any in-session attempt to set this (or [`INTEGRATE_TOKEN_ENV`]):
/// setting an override token from inside a session is bypass, not override.
pub const HUMAN_OVERRIDE_ENV: &str = "CODEFLOW_HUMAN_OVERRIDE";

/// A single policy violation found by a hook plane.
#[derive(Debug, Clone)]
pub struct Violation {
    /// The violated policy rule, dotted-path into `policy.json`
    /// (e.g. `git.push_to_protected`).
    pub rule: String,
    /// Enforcement level the policy assigns this rule (block or warn).
    pub level: PolicyLevel,
    /// Human-readable explanation of what was attempted.
    pub message: String,
    /// The sanctioned path: what to do instead.
    pub remedy: String,
}

impl Violation {
    /// Build a violation for `rule` at `level`.
    #[must_use]
    pub fn new(rule: &str, level: PolicyLevel, message: String, remedy: String) -> Self {
        Self {
            rule: rule.to_string(),
            level,
            message,
            remedy,
        }
    }

    /// Render the violation for stderr, prefixed with the emitting plane
    /// (e.g. `git-guard`, `pre-commit`).
    #[must_use]
    pub fn render(&self, plane: &str) -> String {
        let verdict = match self.level {
            PolicyLevel::Block => "BLOCKED",
            PolicyLevel::Warn => "warning",
            PolicyLevel::Allow | PolicyLevel::Off => "note",
        };
        format!(
            "codeflow {plane}: {verdict} — policy rule {rule} ({level})\n  {msg}\n  sanctioned: {remedy}\n  policy file: .codeflow/policy.json",
            rule = self.rule,
            level = self.level,
            msg = self.message,
            remedy = self.remedy,
        )
    }
}

/// `true` when any violation in the slice is at block level.
#[must_use]
pub fn any_blocking(violations: &[Violation]) -> bool {
    violations.iter().any(|v| v.level == PolicyLevel::Block)
}

/// Current UTC time as an RFC 3339 string (second precision).
///
/// Hand-rolled from `SystemTime` so the hook plane needs no clock dependency;
/// uses the standard civil-from-days algorithm.
#[must_use]
pub fn rfc3339_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339_from_unix(secs)
}

/// Format a unix timestamp (seconds) as RFC 3339 UTC.
#[must_use]
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]
pub fn rfc3339_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil-from-days (Howard Hinnant's algorithm), epoch 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };

    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc3339_epoch() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_rfc3339_known_timestamp() {
        // 2026-06-12T00:00:00Z == 1781222400
        assert_eq!(rfc3339_from_unix(1_781_222_400), "2026-06-12T00:00:00Z");
    }

    #[test]
    fn test_rfc3339_leap_year_day() {
        // 2024-02-29T12:30:45Z == 1709209845
        assert_eq!(rfc3339_from_unix(1_709_209_845), "2024-02-29T12:30:45Z");
    }

    #[test]
    fn test_rfc3339_now_shape() {
        let now = rfc3339_utc_now();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z'));
        assert_eq!(&now[4..5], "-");
    }

    #[test]
    fn test_violation_render_names_rule_and_remedy() {
        let v = Violation::new(
            "git.push_to_protected",
            PolicyLevel::Block,
            "direct push to protected branch 'main'".into(),
            "open a PR or run `codeflow integrate`".into(),
        );
        let out = v.render("git-guard");
        assert!(out.contains("BLOCKED"));
        assert!(out.contains("git.push_to_protected"));
        assert!(out.contains("codeflow integrate"));
        assert!(out.contains(".codeflow/policy.json"));
    }

    #[test]
    fn test_violation_render_warn() {
        let v = Violation::new(
            "git.commit_format",
            PolicyLevel::Warn,
            "bad subject".into(),
            "use type(scope): description".into(),
        );
        assert!(v.render("commit-msg").contains("warning"));
    }

    #[test]
    fn test_any_blocking() {
        let warn = Violation::new("r", PolicyLevel::Warn, String::new(), String::new());
        let block = Violation::new("r", PolicyLevel::Block, String::new(), String::new());
        assert!(!any_blocking(std::slice::from_ref(&warn)));
        assert!(any_blocking(&[warn, block]));
        assert!(!any_blocking(&[]));
    }
}
