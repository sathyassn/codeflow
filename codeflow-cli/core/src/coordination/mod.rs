//! CRDT-based coordination for parallel worktree sessions.
//!
//! This module provides claim management and merge coordination using Loro CRDTs.
//! Claims are keyed by exact file path (relative to project root) and resolved
//! via last-writer-wins semantics on the underlying `LoroMap`.

pub mod claims;
pub mod loro;
pub mod sync;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::CoordinationError;
use crate::types::SessionId;

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

/// Identifies a git worktree instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorktreeId(String);

impl WorktreeId {
    /// Create a new `WorktreeId`.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the inner string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorktreeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Monotonically increasing token that prevents stale operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FencingToken(u64);

impl FencingToken {
    /// Create a new `FencingToken`.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the inner `u64` value.
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }

    /// Return the next token value (incremented by one).
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for FencingToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies a peer in the CRDT network.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PeerId(String);

impl PeerId {
    /// Create a new `PeerId`.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the inner string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Claim data
// ---------------------------------------------------------------------------

/// A file-level claim held by a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    /// Session that owns this claim.
    pub owner: SessionId,
    /// Fencing token at time of acquisition.
    pub token: FencingToken,
    /// TTL in seconds.
    pub ttl_secs: u64,
    /// Unix timestamp (seconds) when the claim was acquired.
    pub acquired_at: u64,
}

// ---------------------------------------------------------------------------
// Coordinator trait
// ---------------------------------------------------------------------------

/// Trait for coordination backends (CRDT-based or otherwise).
///
/// All operations are synchronous because the underlying Loro operations are
/// CPU-bound in-memory manipulations, not async I/O.
pub trait Coordinator: Send + Sync {
    /// Acquire a claim on a file path for the given session.
    ///
    /// Returns the `FencingToken` assigned to this claim.
    ///
    /// # Errors
    ///
    /// Returns `CoordinationError::ClaimConflict` if another session already
    /// holds a non-expired claim on the same path.
    fn acquire(
        &mut self,
        path: &str,
        session_id: &SessionId,
    ) -> Result<FencingToken, CoordinationError>;

    /// Release a claim on a file path.
    ///
    /// No-op if the path has no active claim or the session does not own it.
    ///
    /// # Errors
    ///
    /// Returns `CoordinationError::TokenMismatch` if the provided token does
    /// not match the current claim's token.
    fn release(
        &mut self,
        path: &str,
        session_id: &SessionId,
        token: FencingToken,
    ) -> Result<(), CoordinationError>;

    /// Check whether a claim exists for the given path and return it if active.
    fn check(&self, path: &str) -> Option<Claim>;

    /// Persist the current state to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the atomic write fails.
    fn persist(&self) -> Result<(), CoordinationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_id_display() {
        let id = WorktreeId::new("wt-feat-branch");
        assert_eq!(id.to_string(), "wt-feat-branch");
        assert_eq!(id.as_str(), "wt-feat-branch");
    }

    #[test]
    fn fencing_token_arithmetic() {
        let t = FencingToken::new(0);
        assert_eq!(t.value(), 0);
        let t2 = t.next();
        assert_eq!(t2.value(), 1);
        assert_eq!(t2.to_string(), "1");
    }

    #[test]
    fn peer_id_display() {
        let p = PeerId::new("peer-abc");
        assert_eq!(p.to_string(), "peer-abc");
        assert_eq!(p.as_str(), "peer-abc");
    }

    #[test]
    fn claim_serde_roundtrip() {
        let claim = Claim {
            owner: SessionId::new_unchecked("ses-123"),
            token: FencingToken::new(42),
            ttl_secs: 300,
            acquired_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&claim).unwrap();
        let parsed: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.owner.as_str(), "ses-123");
        assert_eq!(parsed.token.value(), 42);
        assert_eq!(parsed.ttl_secs, 300);
        assert_eq!(parsed.acquired_at, 1_700_000_000);
    }

    #[test]
    fn worktree_id_serde_roundtrip() {
        let id = WorktreeId::new("wt-1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"wt-1\"");
        let parsed: WorktreeId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn fencing_token_serde_roundtrip() {
        let t = FencingToken::new(99);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "99");
        let parsed: FencingToken = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn peer_id_serde_roundtrip() {
        let p = PeerId::new("peer-xyz");
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"peer-xyz\"");
        let parsed: PeerId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, p);
    }
}
