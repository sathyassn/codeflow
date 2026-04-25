//! Autorun session detection.
//!
//! Provides a single helper that determines whether the current process is
//! running as part of an autorun worker session. Security hooks that would
//! otherwise emit a user-facing permission prompt check this first and
//! short-circuit with an allow decision, since autorun sessions have no
//! human available to respond to prompts.
//!
//! ## Trust boundary
//!
//! The "is this an autorun worker?" question is the trust boundary that
//! decides whether security prompts get bypassed. A single env var check
//! is forgeable — a local attacker can `export AUTORUN_SESSION_ID=ses-anything`
//! and disable interactive permission prompts. This is the F1 finding from
//! the WS-SEC review.
//!
//! To make forgery substantially harder without adding new infrastructure,
//! the helper requires THREE conditions to all hold simultaneously:
//!
//! 1. `AUTORUN_SESSION_ID` is set and non-blank.
//! 2. `AUTORUN_BATCH_ID` is set and non-blank. The autorun orchestrator
//!    sets both vars together (see CLAUDE.md "Environment variables"
//!    table); a forger must know to set both.
//! 3. `AUTORUN_SESSION_ID` matches the orchestrator's ID format —
//!    validated via the existing `crate::session::is_valid_session_id`
//!    helper, which accepts the ULID format produced by
//!    `generate_session_id()` (and the legacy timestamp+hex format).
//!
//! All three must pass for the helper to return `true`. Any one failing
//! reverts to interactive enforcement (false). This is defense-in-depth,
//! not cryptographic — but it raises the bar from "guess one var" to
//! "know two var names AND produce a syntactically valid session ID."

use crate::session::is_valid_session_id;

/// Returns true if this process is running as part of an autorun worker
/// session, with all three trust-boundary conditions satisfied.
///
/// The caller (a security hook) uses this as a binary gate to skip
/// permission prompts that have no human to answer. Returning false in any
/// ambiguous case keeps interactive enforcement active — fail-secure.
///
/// ## Conditions (all must hold)
///
/// 1. `AUTORUN_SESSION_ID` env var present and non-blank
/// 2. `AUTORUN_BATCH_ID` env var present and non-blank
/// 3. `AUTORUN_SESSION_ID` value passes `is_valid_session_id` (matches
///    `ses-{26-char Crockford base32 ULID}` or the legacy
///    `ses-{13-digit timestamp}{12-char lowercase hex}` form)
///
/// Blank/whitespace values are treated as unset to prevent a stale empty
/// var inherited from a prior invocation from disabling enforcement.
#[must_use]
pub fn is_autorun_session() -> bool {
    let session_id = match read_non_blank_env("AUTORUN_SESSION_ID") {
        Some(v) => v,
        None => return false,
    };
    if read_non_blank_env("AUTORUN_BATCH_ID").is_none() {
        return false;
    }
    is_valid_session_id(&session_id)
}

/// Read an env var, returning `None` if unset, empty, or whitespace-only.
///
/// Mirrors the pattern used at `gh_pr_guard.rs:235-238` for consistency
/// with the existing autorun consumer.
fn read_non_blank_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid autorun environment (all three conditions hold) reports
    /// autorun mode. Uses the orchestrator's ULID format.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_true_when_all_three_valid() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "ses-01jq7abcdef0123456789abcde");
            std::env::set_var("AUTORUN_BATCH_ID", "batch-abc123");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting so a panic cannot
        // leak the env vars into subsequent tests.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(
            result,
            "valid AUTORUN_SESSION_ID + AUTORUN_BATCH_ID + ULID format must pass"
        );
    }

    /// Default test environment (no env vars set) reports interactive.
    /// `lib.rs::strip_codeflow_env` runs before any test thread spawns, so
    /// the harness guarantees a clean baseline.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_env_unset() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(!is_autorun_session());
    }

    /// A blank (whitespace-only) `AUTORUN_SESSION_ID` must be rejected.
    /// Prevents a stale empty var inherited from a prior autorun invocation
    /// from disabling enforcement in an interactive session.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_session_id_blank() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "   ");
            std::env::set_var("AUTORUN_BATCH_ID", "batch-abc123");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(!result, "blank AUTORUN_SESSION_ID must be treated as unset");
    }

    /// `AUTORUN_SESSION_ID` set to a valid ULID, but `AUTORUN_BATCH_ID`
    /// absent → reject. Forging just one var is the F1 attack chain;
    /// requiring both raises the bar.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_batch_id_unset() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "ses-01jq7abcdef0123456789abcde");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
        }
        assert!(
            !result,
            "missing AUTORUN_BATCH_ID must keep interactive enforcement active"
        );
    }

    /// `AUTORUN_BATCH_ID` set but blank → reject. Same rationale as the
    /// `AUTORUN_SESSION_ID` blank case: a stale empty var must not unlock
    /// the bypass.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_batch_id_blank() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "ses-01jq7abcdef0123456789abcde");
            std::env::set_var("AUTORUN_BATCH_ID", "   ");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(!result, "blank AUTORUN_BATCH_ID must be treated as unset");
    }

    /// Both env vars set, but `AUTORUN_SESSION_ID` value does not match
    /// the orchestrator's session-ID format → reject. This is the F1
    /// hardening case: an attacker who knows both var names but produces
    /// "ses-anything" cannot pass without also producing a syntactically
    /// valid ULID.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_session_id_format_invalid() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "ses-anything-attacker-set");
            std::env::set_var("AUTORUN_BATCH_ID", "batch-abc123");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(
            !result,
            "malformed AUTORUN_SESSION_ID must keep interactive enforcement active"
        );
    }

    /// Empty-prefix forgery attempt: `AUTORUN_SESSION_ID` lacks the `ses-`
    /// prefix entirely. `is_valid_session_id` rejects this, so we reject
    /// autorun mode.
    #[test]
    #[serial_test::serial(env_vars)]
    fn is_autorun_session_false_when_session_id_missing_prefix() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("AUTORUN_SESSION_ID", "01jq7validulidexample0aa");
            std::env::set_var("AUTORUN_BATCH_ID", "batch-abc123");
        }
        let result = is_autorun_session();
        // SAFETY: serialized; clean up BEFORE asserting.
        unsafe {
            std::env::remove_var("AUTORUN_SESSION_ID");
            std::env::remove_var("AUTORUN_BATCH_ID");
        }
        assert!(
            !result,
            "session ID without `ses-` prefix must fail format check"
        );
    }
}
