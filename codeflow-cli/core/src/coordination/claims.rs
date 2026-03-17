//! Claim management on top of `LoroCoordinator`.
//!
//! Provides batch operations and query utilities for file-level claims.

use crate::error::CoordinationError;
use crate::types::SessionId;

use super::loro::LoroCoordinator;
use super::{Claim, Coordinator, FencingToken};

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
}
