//! Claim management on top of `LoroCoordinator`.
//!
//! Provides batch operations and query utilities for file-level claims.

use std::time::Duration;

use crate::error::CoordinationError;
use crate::types::SessionId;

use super::loro::LoroCoordinator;
use super::{Claim, Coordinator, FencingToken};

/// Backoff schedule for [`release_all_with_retry`]: 4 attempts total
/// with sleeps of 500ms / 1.5s / 4.5s between attempts.
///
/// INF-TSK-050-004 AC-02: claim release used to be best-effort
/// (`let _ = release_all(...)`) inside the worker shutdown path. When
/// the underlying CRDT write contended with the sync daemon (5s poll)
/// or another worker, the release silently failed and left stale
/// claims that blocked successor workers. The retry schedule covers
/// transient contention windows of up to ~6.5s — a slow filesystem
/// flush or sync-daemon read-modify-write cycle resolves before the
/// 4.5s third retry, so we don't ship stale claims to the next worker.
///
/// **Schedule rationale (pre-merge rework, INF-TSK-050-004 retry 2):**
/// The earlier "stay under 5s sync-daemon cadence" interpretation was
/// over-engineered: at most one missed sync poll per shutdown is
/// acceptable, and the literal AC text ("3 attempts, 500ms / 1.5s /
/// 4.5s") implies all three delays exist as part of the schedule. We
/// therefore have 3 inter-attempt sleeps and 4 attempts total.
///
/// `len(delays) + 1` attempts total: 4 attempts → 3 inter-attempt
/// sleeps (500ms, 1500ms, 4500ms). Worst-case wall-clock wait before
/// returning the final error is the sum of the three sleeps (~6.5s)
/// plus the four CRDT-write attempts themselves.
pub const RELEASE_ALL_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_millis(1_500),
    Duration::from_millis(4_500),
];

/// Result of a batch acquire: successfully acquired claims and conflicts.
pub type BatchAcquireResult = Result<
    (
        Vec<(String, FencingToken)>,
        Vec<(String, CoordinationError)>,
    ),
    CoordinationError,
>;

/// Batch-acquire claims on multiple file paths for a single session.
///
/// Returns a list of `(path, token)` pairs for successfully acquired claims.
/// If any path fails with `ClaimConflict`, that path is skipped and included
/// in the returned error list. Non-conflict errors are propagated immediately.
///
/// # Errors
///
/// Returns a `CoordinationError` if a non-conflict error is encountered.
pub fn acquire_batch(
    coordinator: &mut LoroCoordinator,
    paths: &[&str],
    session_id: &SessionId,
) -> BatchAcquireResult {
    let mut acquired = Vec::new();
    let mut conflicts = Vec::new();

    for &path in paths {
        match coordinator.acquire(path, session_id) {
            Ok(token) => acquired.push((path.to_string(), token)),
            Err(e @ CoordinationError::ClaimConflict { .. }) => {
                conflicts.push((path.to_string(), e));
            }
            Err(e) => return Err(e),
        }
    }

    Ok((acquired, conflicts))
}

/// Release all claims held by a session.
///
/// Scans the claims map and releases every claim owned by `session_id`.
/// Claims owned by other sessions are left untouched.
///
/// Returns the number of claims released.
///
/// # Errors
///
/// Returns a `CoordinationError` if any release operation fails.
pub fn release_all(
    coordinator: &mut LoroCoordinator,
    session_id: &SessionId,
) -> Result<usize, CoordinationError> {
    let claims_map = coordinator.doc().get_map("claims");
    let mut owned_paths: Vec<(String, FencingToken)> = Vec::new();

    claims_map.for_each(|key, value| {
        if let loro::ValueOrContainer::Value(loro::LoroValue::String(json_str)) = value {
            if let Ok(claim) = serde_json::from_str::<Claim>(&json_str) {
                if claim.owner == *session_id {
                    owned_paths.push((key.to_string(), claim.token));
                }
            }
        }
    });

    let count = owned_paths.len();
    for (path, token) in owned_paths {
        coordinator.release(&path, session_id, token)?;
    }

    // Proactively evict any expired claims from other sessions.
    let _ = compact_expired_claims(coordinator);

    Ok(count)
}

/// Release all claims for `session_id`, retrying transient failures with
/// exponential backoff per [`RELEASE_ALL_RETRY_DELAYS`].
///
/// INF-TSK-050-004 AC-02: the worker shutdown path used to call
/// `release_all` once and discard the `Result` (`let _ =`), which let
/// transient CRDT-write contention leak claims into the next worker's
/// startup. This helper retries up to 4 times total — initial attempt
/// plus three retries with sleeps of 500ms / 1.5s / 4.5s between them
/// (worst case ~6.5s of waiting before surfacing the final error).
///
/// `op` is invoked on each attempt and is expected to encapsulate the
/// caller's locking strategy (e.g. `locked_binary_rmw` wrapping
/// `release_all`). On the final failure, the last error is returned.
///
/// Returns `Ok(attempts)` where `attempts` is the 1-based attempt number
/// that succeeded (for telemetry / test assertions).
///
/// # Errors
///
/// Returns the last error from `op` if all attempts fail.
///
/// # Panics
///
/// Cannot panic in practice: the loop runs `RELEASE_ALL_RETRY_DELAYS.len() + 1`
/// times (>=1), so on the failure path `last_err` is always populated
/// before the `expect` is reached.
pub async fn release_all_with_retry<F, E>(mut op: F) -> Result<usize, E>
where
    F: FnMut() -> Result<(), E>,
    E: std::fmt::Display,
{
    let mut last_err: Option<E> = None;
    let total_attempts = RELEASE_ALL_RETRY_DELAYS.len() + 1;
    let mut attempt: usize = 0;
    while attempt < total_attempts {
        match op() {
            Ok(()) => return Ok(attempt + 1),
            Err(e) => {
                eprintln!(
                    "warn: release_all attempt {} of {total_attempts} failed: {e}",
                    attempt + 1
                );
                last_err = Some(e);
                // Sleep before next attempt, indexed by current
                // attempt number into the delay schedule.
                if attempt < RELEASE_ALL_RETRY_DELAYS.len() {
                    let delay = RELEASE_ALL_RETRY_DELAYS[attempt];
                    tokio::time::sleep(delay).await;
                }
            }
        }
        attempt += 1;
    }
    Err(last_err.expect("retry loop ran at least once"))
}

/// Validate that a session's expected fencing token matches the current token
/// for a claimed path.
///
/// Returns `Ok(())` if the token matches or no claim exists for the path.
/// Returns `Err(CoordinationError::TokenMismatch)` if the current claim has a
/// different token (indicating a superseding acquisition by another session).
///
/// # Errors
///
/// Returns `CoordinationError::TokenMismatch` if the stored token differs from
/// `expected_token`.
pub fn validate_token(
    coordinator: &LoroCoordinator,
    path: &str,
    session_id: &SessionId,
    expected_token: FencingToken,
) -> Result<(), CoordinationError> {
    match coordinator.check(path) {
        Some(claim) => {
            if claim.owner != *session_id && claim.token != expected_token {
                Err(CoordinationError::TokenMismatch {
                    expected: expected_token.value(),
                    found: claim.token.value(),
                })
            } else {
                Ok(())
            }
        }
        // No active claim — nothing to validate against.
        None => Ok(()),
    }
}

/// Remove all expired claim entries from the Loro CRDT document.
///
/// Iterates every claim in the coordinator's claims map and removes entries
/// where `now - claim.acquired_at >= claim.ttl_secs`, matching the lazy
/// expiry check in `LoroCoordinator::acquire` (`loro.rs:290`).
///
/// Returns the number of evicted entries.
///
/// **Concurrency:** This function mutates the `LoroCoordinator` in-place.
/// Callers MUST wrap it inside [`crate::file_lock::locked_binary_rmw`] when
/// operating on the shared `state.loro` file to hold the sidecar lock for
/// the duration of the read-modify-write cycle.
///
/// # Errors
///
/// Returns a `CoordinationError` if a claim entry cannot be deleted from
/// the underlying Loro map.
pub fn compact_expired_claims(
    coordinator: &mut LoroCoordinator,
) -> Result<usize, CoordinationError> {
    let claims_map = coordinator.doc().get_map("claims");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut expired_keys: Vec<String> = Vec::new();

    claims_map.for_each(|key, value| {
        if let loro::ValueOrContainer::Value(loro::LoroValue::String(json_str)) = value {
            if let Ok(claim) = serde_json::from_str::<Claim>(&json_str) {
                let expired = now.saturating_sub(claim.acquired_at) >= claim.ttl_secs;
                if expired {
                    expired_keys.push(key.to_string());
                }
            }
        }
    });

    let count = expired_keys.len();
    for key in &expired_keys {
        claims_map.delete(key)?;
    }
    if count > 0 {
        coordinator.doc().commit();
    }

    Ok(count)
}

/// List all active (non-expired) claims.
///
/// Returns `(path, claim)` pairs for all non-expired claims in the coordinator.
///
/// Uses a fallback of `0` for the current timestamp if the system clock is
/// before the Unix epoch (practically impossible on modern systems).
#[must_use]
pub fn list_active(coordinator: &LoroCoordinator) -> Vec<(String, Claim)> {
    let claims_map = coordinator.doc().get_map("claims");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut result = Vec::new();

    claims_map.for_each(|key, value| {
        if let loro::ValueOrContainer::Value(loro::LoroValue::String(json_str)) = value {
            if let Ok(claim) = serde_json::from_str::<Claim>(&json_str) {
                let expired = now.saturating_sub(claim.acquired_at) >= claim.ttl_secs;
                if !expired {
                    result.push((key.to_string(), claim));
                }
            }
        }
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    // -- Positive tests --

    #[test]
    fn batch_acquire_all_succeed() {
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        let paths = ["src/a.rs", "src/b.rs", "src/c.rs"];

        let (acquired, conflicts) = acquire_batch(&mut coord, &paths, &sid).unwrap();
        assert_eq!(acquired.len(), 3);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn batch_acquire_partial_conflict() {
        let mut coord = LoroCoordinator::in_memory();
        let s1 = session("ses-001");
        let s2 = session("ses-002");

        coord.acquire("src/b.rs", &s1).unwrap();

        let paths = ["src/a.rs", "src/b.rs", "src/c.rs"];
        let (acquired, conflicts) = acquire_batch(&mut coord, &paths, &s2).unwrap();
        assert_eq!(acquired.len(), 2);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "src/b.rs");
    }

    #[test]
    fn release_all_for_session() {
        let mut coord = LoroCoordinator::in_memory();
        let s1 = session("ses-001");
        let s2 = session("ses-002");

        coord.acquire("src/a.rs", &s1).unwrap();
        coord.acquire("src/b.rs", &s1).unwrap();
        coord.acquire("src/c.rs", &s2).unwrap();

        let count = release_all(&mut coord, &s1).unwrap();
        assert_eq!(count, 2);

        // s1's claims gone, s2's claim remains.
        assert!(coord.check("src/a.rs").is_none());
        assert!(coord.check("src/b.rs").is_none());
        assert!(coord.check("src/c.rs").is_some());
    }

    #[test]
    fn release_all_empty() {
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        let count = release_all(&mut coord, &sid).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn list_active_claims() {
        let mut coord = LoroCoordinator::in_memory();
        let s1 = session("ses-001");
        let s2 = session("ses-002");

        coord.acquire("src/a.rs", &s1).unwrap();
        coord.acquire("src/b.rs", &s2).unwrap();

        let active = list_active(&coord);
        assert_eq!(active.len(), 2);

        let paths: Vec<&str> = active.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"src/a.rs"));
        assert!(paths.contains(&"src/b.rs"));
    }

    // -- validate_token tests --

    #[test]
    fn validate_token_matches() {
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        let token = coord.acquire("src/a.rs", &sid).unwrap();

        // Same session, same token — should pass.
        assert!(validate_token(&coord, "src/a.rs", &sid, token).is_ok());
    }

    #[test]
    fn validate_token_mismatch_different_session() {
        let mut coord = LoroCoordinator::in_memory();
        let s1 = session("ses-001");
        let s2 = session("ses-002");

        // s1 acquires with TTL=0 so it immediately expires.
        coord.set_ttl_secs(0);
        let _t1 = coord.acquire("src/a.rs", &s1).unwrap();

        // s2 acquires (s1's claim is expired, so no conflict).
        // Use normal TTL so s2's claim is active for validation.
        coord.set_ttl_secs(300);
        let t2 = coord.acquire("src/a.rs", &s2).unwrap();

        // s1 tries to validate with its old token — mismatch.
        let stale_token = FencingToken::new(1);
        let err = validate_token(&coord, "src/a.rs", &s1, stale_token).unwrap_err();
        assert!(
            err.to_string().contains("token mismatch"),
            "expected token mismatch, got: {err}",
        );
        // Confirm the stored token is the newer one.
        assert_eq!(t2.value(), 2);
    }

    #[test]
    fn validate_token_no_claim_is_ok() {
        let coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        let token = FencingToken::new(42);

        // No claim exists — validation passes.
        assert!(validate_token(&coord, "src/nonexistent.rs", &sid, token).is_ok());
    }

    // -- Edge cases --

    #[test]
    fn list_active_excludes_expired() {
        let mut coord = LoroCoordinator::in_memory();
        coord.set_ttl_secs(0); // Immediate expiry.
        let sid = session("ses-001");
        coord.acquire("src/a.rs", &sid).unwrap();

        let active = list_active(&coord);
        assert!(active.is_empty());
    }

    #[test]
    fn batch_acquire_empty_paths() {
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        let paths: [&str; 0] = [];

        let (acquired, conflicts) = acquire_batch(&mut coord, &paths, &sid).unwrap();
        assert!(acquired.is_empty());
        assert!(conflicts.is_empty());
    }

    // -- compact_expired_claims tests --

    #[test]
    fn compact_removes_expired_claims() {
        let mut coord = LoroCoordinator::in_memory();
        // TTL=0 means claims expire immediately.
        coord.set_ttl_secs(0);
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        coord.acquire("src/a.rs", &s1).unwrap();
        coord.acquire("src/b.rs", &s2).unwrap();

        let evicted = compact_expired_claims(&mut coord).unwrap();
        assert_eq!(evicted, 2, "both expired claims should be evicted");

        // Verify the claims are actually gone from the CRDT map.
        let active = list_active(&coord);
        assert!(active.is_empty(), "no active claims should remain");
    }

    #[test]
    fn compact_retains_valid_claims() {
        let mut coord = LoroCoordinator::in_memory();
        // Use a large TTL so claims are not expired.
        coord.set_ttl_secs(99_999);
        let sid = session("ses-001");
        coord.acquire("src/a.rs", &sid).unwrap();
        coord.acquire("src/b.rs", &sid).unwrap();

        let evicted = compact_expired_claims(&mut coord).unwrap();
        assert_eq!(evicted, 0, "no claims should be evicted");

        let active = list_active(&coord);
        assert_eq!(active.len(), 2, "both claims should survive");
    }

    #[test]
    fn compact_on_empty_crdt_is_noop() {
        let mut coord = LoroCoordinator::in_memory();

        let evicted = compact_expired_claims(&mut coord).unwrap();
        assert_eq!(evicted, 0, "empty CRDT should be a no-op");
    }

    #[test]
    fn compact_mixed_expired_and_valid() {
        let mut coord = LoroCoordinator::in_memory();

        // First claim: TTL=0 (expired immediately).
        coord.set_ttl_secs(0);
        let s1 = session("ses-001");
        coord.acquire("src/expired.rs", &s1).unwrap();

        // Second claim: TTL=99999 (long-lived).
        coord.set_ttl_secs(99_999);
        let s2 = session("ses-002");
        coord.acquire("src/valid.rs", &s2).unwrap();

        let evicted = compact_expired_claims(&mut coord).unwrap();
        assert_eq!(evicted, 1, "only the expired claim should be evicted");

        // The valid claim should still be active.
        let active = list_active(&coord);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, "src/valid.rs");
    }

    // -- release_all_with_retry tests (INF-TSK-050-004 AC-02) --

    #[tokio::test]
    async fn release_retry_succeeds_on_first_attempt() {
        let attempts = std::cell::Cell::new(0);
        let result = release_all_with_retry::<_, &'static str>(|| {
            attempts.set(attempts.get() + 1);
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(result, 1);
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn release_retry_succeeds_on_third_attempt() {
        // Verifies the in-between behaviour: helper keeps retrying past
        // the first failure but stops as soon as op() succeeds, even
        // before exhausting the full schedule.
        let attempts = std::cell::Cell::new(0);
        let result = release_all_with_retry::<_, String>(|| {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 3 {
                Err(format!("transient error on attempt {}", attempts.get()))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 3, "should succeed on the 3rd attempt");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn release_retry_succeeds_on_fourth_attempt() {
        // Pre-merge AC-02 schedule rework: with 3 inter-attempt sleeps
        // there are 4 total attempts — verify the helper still keeps
        // retrying through the full schedule when failures persist.
        let attempts = std::cell::Cell::new(0);
        let result = release_all_with_retry::<_, String>(|| {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 4 {
                Err(format!("transient error on attempt {}", attempts.get()))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 4, "should succeed on the 4th attempt");
        assert_eq!(attempts.get(), 4);
    }

    #[tokio::test]
    async fn release_retry_returns_error_after_all_attempts_fail() {
        let attempts = std::cell::Cell::new(0);
        let err = release_all_with_retry::<_, String>(|| {
            attempts.set(attempts.get() + 1);
            Err(format!("attempt {}", attempts.get()))
        })
        .await
        .unwrap_err();
        assert_eq!(
            attempts.get(),
            RELEASE_ALL_RETRY_DELAYS.len() + 1,
            "should attempt {} times",
            RELEASE_ALL_RETRY_DELAYS.len() + 1
        );
        assert!(
            err.contains(&format!("attempt {}", attempts.get())),
            "final error should be from the last attempt, got: {err}",
        );
    }

    #[test]
    fn release_retry_delays_constant() {
        // INF-TSK-050-004 AC-02 (post-rework): literal AC text says
        // "3 attempts, 500ms / 1.5s / 4.5s". With 3 inter-attempt
        // sleeps, the natural reading is 4 attempts total. Anchor each
        // schedule slot so a regression that re-shortens the schedule
        // (e.g. back to the over-engineered 2-element [500ms, 1.5s])
        // produces a deliberate, visible diff.
        assert_eq!(RELEASE_ALL_RETRY_DELAYS.len(), 3);
        assert_eq!(RELEASE_ALL_RETRY_DELAYS[0].as_millis(), 500);
        assert_eq!(RELEASE_ALL_RETRY_DELAYS[1].as_millis(), 1_500);
        assert_eq!(RELEASE_ALL_RETRY_DELAYS[2].as_millis(), 4_500);
        // 4 attempts total = 1 + len(delays).
        assert_eq!(RELEASE_ALL_RETRY_DELAYS.len() + 1, 4);
    }

    #[test]
    fn release_all_also_compacts_expired() {
        let mut coord = LoroCoordinator::in_memory();

        // Create an expired claim from s1.
        coord.set_ttl_secs(0);
        let s1 = session("ses-001");
        coord.acquire("src/stale.rs", &s1).unwrap();

        // Create a valid claim from s2.
        coord.set_ttl_secs(99_999);
        let s2 = session("ses-002");
        coord.acquire("src/fresh.rs", &s2).unwrap();

        // release_all for s2 should release s2's claim AND compact s1's expired claim.
        let released = release_all(&mut coord, &s2).unwrap();
        assert_eq!(released, 1, "should release s2's claim");

        // The expired s1 claim should also be gone (compacted by release_all).
        let claims_map = coord.doc().get_map("claims");
        let mut remaining = 0;
        claims_map.for_each(|_key, _value| {
            remaining += 1;
        });
        assert_eq!(
            remaining, 0,
            "expired s1 claim should have been compacted by release_all"
        );
    }
}
