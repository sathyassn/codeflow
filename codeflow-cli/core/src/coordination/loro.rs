//! `LoroCoordinator` — Loro CRDT-based coordination backend.
//!
//! Uses a single `LoroDoc` with named containers:
//! - `claims` (`LoroMap`): file-level claims keyed by path
//! - `merge_queue` (`LoroList`): ordered merge queue (reserved)
//! - `kg_entities` (`LoroMap`): reserved for Epic C knowledge graph
//! - `kg_relationships` (`LoroMap`): reserved for Epic C
//! - `kg_metadata` (`LoroMap`): reserved for Epic C
//! - `extraction_queue` (`LoroList`): reserved for Epic C

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{ExportMode, LoroDoc, LoroValue, ValueOrContainer};

use crate::error::CoordinationError;
use crate::types::SessionId;

use super::{Claim, Coordinator, FencingToken};

/// Extract a JSON string from a `ValueOrContainer` if it's a string value.
fn extract_string(voc: &ValueOrContainer) -> Option<&str> {
    match voc {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.as_ref()),
        _ => None,
    }
}

/// Default claim TTL: 70 minutes (4200s).
///
/// Must exceed the worker timeout (default 3600s / 60 min) to prevent
/// "locked out of own file" scenarios. The sync daemon's PID liveness
/// detection is the primary crash cleanup mechanism; TTL is the fallback.
const DEFAULT_TTL_SECS: u64 = 4200;

/// Environment variable to override the claim TTL.
const TTL_ENV_VAR: &str = "CODEFLOW_CLAIM_TTL_SECS";

/// Container name for the claims map.
const CLAIMS_CONTAINER: &str = "claims";

/// Container name for the merge queue list.
const MERGE_QUEUE_CONTAINER: &str = "merge_queue";

/// Reserved Knowledge Graph container names (Decision #23).
const KG_ENTITIES_CONTAINER: &str = "kg_entities";
const KG_RELATIONSHIPS_CONTAINER: &str = "kg_relationships";
const KG_METADATA_CONTAINER: &str = "kg_metadata";
const EXTRACTION_QUEUE_CONTAINER: &str = "extraction_queue";

/// Read the configured claim TTL.
///
/// Priority: env var > parallel-work-config.json > hardcoded default (4200s).
fn configured_ttl_secs() -> u64 {
    // 1. Environment variable override (highest priority).
    if let Some(val) = std::env::var(TTL_ENV_VAR)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        return val;
    }

    // 2. Read from parallel-work-config.json if available.
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(config) = crate::autorun::config::load_config(&cwd) {
            return config.claims.ttl_secs;
        }
    }

    // 3. Hardcoded default.
    DEFAULT_TTL_SECS
}

/// Return the current Unix timestamp in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// CRDT-based coordinator backed by a Loro document.
pub struct LoroCoordinator {
    doc: LoroDoc,
    /// Next fencing token value (monotonically increasing within this instance).
    next_token: u64,
    /// Path where the Loro state is persisted.
    state_path: PathBuf,
    /// Configured claim TTL in seconds.
    ttl_secs: u64,
}

impl LoroCoordinator {
    /// Create a new `LoroCoordinator`, loading existing state from disk if available.
    ///
    /// The `state_path` should point to `.state/coordination/state.loro`.
    ///
    /// # Errors
    ///
    /// Returns an error if an existing state file cannot be read or decoded.
    pub fn new(state_path: impl Into<PathBuf>) -> Result<Self, CoordinationError> {
        let state_path = state_path.into();
        let ttl_secs = configured_ttl_secs();

        let (doc, next_token) = if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            if bytes.is_empty() {
                (Self::init_doc(), 1)
            } else {
                match LoroDoc::from_snapshot(&bytes) {
                    Ok(doc) => {
                        let next_token = Self::compute_next_token(&doc);
                        (doc, next_token)
                    }
                    Err(_) => {
                        // Legacy or corrupt file — start fresh.
                        (Self::init_doc(), 1)
                    }
                }
            }
        } else {
            (Self::init_doc(), 1)
        };

        Ok(Self {
            doc,
            next_token,
            state_path,
            ttl_secs,
        })
    }

    /// Create a `LoroCoordinator` from a raw snapshot byte slice.
    ///
    /// Used by [`crate::file_lock::locked_binary_rmw`] to load the coordinator
    /// under an exclusive sidecar lock, eliminating TOCTOU races between
    /// `new()` (read) and `persist()` (write).
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot cannot be decoded.
    pub fn from_bytes(
        bytes: &[u8],
        state_path: impl Into<PathBuf>,
    ) -> Result<Self, CoordinationError> {
        let state_path = state_path.into();
        let ttl_secs = configured_ttl_secs();

        let (doc, next_token) = if bytes.is_empty() {
            (Self::init_doc(), 1)
        } else {
            match LoroDoc::from_snapshot(bytes) {
                Ok(doc) => {
                    let next_token = Self::compute_next_token(&doc);
                    (doc, next_token)
                }
                Err(_) => {
                    // Legacy or corrupt data — start fresh.
                    (Self::init_doc(), 1)
                }
            }
        };

        Ok(Self {
            doc,
            next_token,
            state_path,
            ttl_secs,
        })
    }

    /// Export the coordinator state as a Loro snapshot byte vector.
    ///
    /// Used by [`crate::file_lock::locked_binary_rmw`] to serialize the state
    /// for atomic write-back under the sidecar lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot export fails.
    pub fn export_bytes(&self) -> Result<Vec<u8>, CoordinationError> {
        let snapshot = self.doc.export(ExportMode::Snapshot)?;
        Ok(snapshot)
    }

    /// Create a new `LoroCoordinator` with an empty document (no disk state).
    ///
    /// Useful for testing.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            doc: Self::init_doc(),
            next_token: 1,
            state_path: PathBuf::from("/dev/null"),
            ttl_secs: configured_ttl_secs(),
        }
    }

    /// Initialize a fresh `LoroDoc` with all named containers.
    fn init_doc() -> LoroDoc {
        let doc = LoroDoc::new();
        // Touch all containers to ensure they exist in the document.
        let _ = doc.get_map(CLAIMS_CONTAINER);
        let _ = doc.get_list(MERGE_QUEUE_CONTAINER);
        // Reserved KG containers (Decision #23)
        let _ = doc.get_map(KG_ENTITIES_CONTAINER);
        let _ = doc.get_map(KG_RELATIONSHIPS_CONTAINER);
        let _ = doc.get_map(KG_METADATA_CONTAINER);
        let _ = doc.get_list(EXTRACTION_QUEUE_CONTAINER);
        doc.commit();
        doc
    }

    /// Scan the claims map to find the highest fencing token in use.
    fn compute_next_token(doc: &LoroDoc) -> u64 {
        let claims = doc.get_map(CLAIMS_CONTAINER);
        let mut max_token: u64 = 0;
        claims.for_each(|_key, value| {
            if let Some(json_str) = extract_string(&value) {
                if let Ok(claim) = serde_json::from_str::<Claim>(json_str) {
                    let t = claim.token.value();
                    if t > max_token {
                        max_token = t;
                    }
                }
            }
        });
        max_token + 1
    }

    /// Allocate the next fencing token.
    fn allocate_token(&mut self) -> FencingToken {
        let token = FencingToken::new(self.next_token);
        self.next_token += 1;
        token
    }

    /// Return a reference to the underlying `LoroDoc`.
    #[must_use]
    pub fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    /// Return the state file path.
    #[must_use]
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Return the configured claim TTL in seconds.
    #[must_use]
    pub fn ttl_secs(&self) -> u64 {
        self.ttl_secs
    }

    /// Override the claim TTL (mainly useful for testing).
    pub fn set_ttl_secs(&mut self, secs: u64) {
        self.ttl_secs = secs;
    }

    /// Merge another `LoroCoordinator`'s document into this one.
    ///
    /// After merging, the fencing token counter is recomputed to be higher
    /// than any token in the merged state.
    ///
    /// # Errors
    ///
    /// Returns `CoordinationError::MergeFailure` if the Loro merge fails.
    pub fn merge(&mut self, other: &LoroCoordinator) -> Result<(), CoordinationError> {
        let snapshot = other.doc.export(ExportMode::Snapshot)?;
        self.doc.import(&snapshot)?;
        self.next_token = Self::compute_next_token(&self.doc);
        Ok(())
    }
}

impl Coordinator for LoroCoordinator {
    fn acquire(
        &mut self,
        path: &str,
        session_id: &SessionId,
    ) -> Result<FencingToken, CoordinationError> {
        let claims = self.doc.get_map(CLAIMS_CONTAINER);
        let now = now_secs();

        // Check for existing non-expired claim by a different session.
        if let Some(ref voc) = claims.get(path) {
            if let Some(json_str) = extract_string(voc) {
                if let Ok(existing) = serde_json::from_str::<Claim>(json_str) {
                    let expired = now.saturating_sub(existing.acquired_at) >= existing.ttl_secs;
                    if !expired && existing.owner != *session_id {
                        return Err(CoordinationError::ClaimConflict {
                            path: path.to_string(),
                            owner: existing.owner,
                        });
                    }
                }
            }
        }

        let token = self.allocate_token();
        let claim = Claim {
            owner: session_id.clone(),
            token,
            ttl_secs: self.ttl_secs,
            acquired_at: now,
            task_id: String::new(),
            worktree_id: None,
        };
        let json = serde_json::to_string(&claim)?;
        claims.insert(path, json)?;
        self.doc.commit();
        Ok(token)
    }

    fn release(
        &mut self,
        path: &str,
        session_id: &SessionId,
        token: FencingToken,
    ) -> Result<(), CoordinationError> {
        let claims = self.doc.get_map(CLAIMS_CONTAINER);

        if let Some(ref voc) = claims.get(path) {
            if let Some(json_str) = extract_string(voc) {
                if let Ok(existing) = serde_json::from_str::<Claim>(json_str) {
                    if existing.owner != *session_id {
                        // Not our claim — no-op per trait contract.
                        return Ok(());
                    }
                    if existing.token != token {
                        return Err(CoordinationError::TokenMismatch {
                            expected: existing.token.value(),
                            found: token.value(),
                        });
                    }
                    claims.delete(path)?;
                    self.doc.commit();
                }
            }
        }
        Ok(())
    }

    fn check(&self, path: &str) -> Option<Claim> {
        let claims = self.doc.get_map(CLAIMS_CONTAINER);
        let now = now_secs();

        if let Some(ref voc) = claims.get(path) {
            if let Some(json_str) = extract_string(voc) {
                if let Ok(claim) = serde_json::from_str::<Claim>(json_str) {
                    let expired = now.saturating_sub(claim.acquired_at) >= claim.ttl_secs;
                    if !expired {
                        return Some(claim);
                    }
                }
            }
        }
        None
    }

    fn persist(&self) -> Result<(), CoordinationError> {
        let snapshot = self.doc.export(ExportMode::Snapshot)?;

        // Atomic write: temp file + rename.
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = self.state_path.with_extension("loro.tmp");
        fs::write(&tmp_path, &snapshot)?;
        fs::rename(&tmp_path, &self.state_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_coordinator() -> LoroCoordinator {
        LoroCoordinator::in_memory()
    }

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    // -- Positive tests --

    #[test]
    fn acquire_and_check() {
        let mut coord = make_coordinator();
        let sid = session("ses-001");
        let token = coord.acquire("src/main.rs", &sid).unwrap();
        assert_eq!(token.value(), 1);

        let claim = coord.check("src/main.rs").unwrap();
        assert_eq!(claim.owner.as_str(), "ses-001");
        assert_eq!(claim.token.value(), 1);
    }

    #[test]
    fn acquire_and_release() {
        let mut coord = make_coordinator();
        let sid = session("ses-001");
        let token = coord.acquire("src/main.rs", &sid).unwrap();

        coord.release("src/main.rs", &sid, token).unwrap();
        assert!(coord.check("src/main.rs").is_none());
    }

    #[test]
    fn acquire_same_session_reacquires() {
        let mut coord = make_coordinator();
        let sid = session("ses-001");
        let t1 = coord.acquire("src/main.rs", &sid).unwrap();
        let t2 = coord.acquire("src/main.rs", &sid).unwrap();
        assert!(t2.value() > t1.value());
    }

    #[test]
    fn multiple_paths_independent() {
        let mut coord = make_coordinator();
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        let _t1 = coord.acquire("src/a.rs", &s1).unwrap();
        let _t2 = coord.acquire("src/b.rs", &s2).unwrap();

        assert!(coord.check("src/a.rs").is_some());
        assert!(coord.check("src/b.rs").is_some());
    }

    // -- Negative / error tests --

    #[test]
    fn acquire_conflict() {
        let mut coord = make_coordinator();
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        let _t = coord.acquire("src/main.rs", &s1).unwrap();

        let err = coord.acquire("src/main.rs", &s2).unwrap_err();
        assert!(
            err.to_string().contains("claim conflict"),
            "expected claim conflict, got: {err}",
        );
    }

    #[test]
    fn release_wrong_token() {
        let mut coord = make_coordinator();
        let sid = session("ses-001");
        let _t = coord.acquire("src/main.rs", &sid).unwrap();

        let wrong_token = FencingToken::new(999);
        let err = coord.release("src/main.rs", &sid, wrong_token).unwrap_err();
        assert!(
            err.to_string().contains("token mismatch"),
            "expected token mismatch, got: {err}",
        );
    }

    #[test]
    fn release_nonexistent_is_noop() {
        let mut coord = make_coordinator();
        let sid = session("ses-001");
        let token = FencingToken::new(1);
        // No claim exists — should succeed silently.
        coord.release("src/main.rs", &sid, token).unwrap();
    }

    #[test]
    fn release_wrong_session_is_noop() {
        let mut coord = make_coordinator();
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        let t = coord.acquire("src/main.rs", &s1).unwrap();

        // s2 does not own this claim — no-op.
        coord.release("src/main.rs", &s2, t).unwrap();
        // Claim still exists.
        assert!(coord.check("src/main.rs").is_some());
    }

    #[test]
    fn check_nonexistent_returns_none() {
        let coord = make_coordinator();
        assert!(coord.check("src/nonexistent.rs").is_none());
    }

    // -- TTL tests --

    #[test]
    fn expired_claim_allows_reacquire() {
        let mut coord = make_coordinator();
        // Override TTL to 0 to make claim immediately expired.
        coord.set_ttl_secs(0);
        let s1 = session("ses-001");
        let _t1 = coord.acquire("src/main.rs", &s1).unwrap();

        // Now another session should be able to acquire because TTL=0 means expired.
        let s2 = session("ses-002");
        let t2 = coord.acquire("src/main.rs", &s2).unwrap();
        assert!(t2.value() > 0);
    }

    #[test]
    fn check_expired_claim_returns_none() {
        let mut coord = make_coordinator();
        coord.set_ttl_secs(0);
        let sid = session("ses-001");
        let _t = coord.acquire("src/main.rs", &sid).unwrap();

        // TTL=0 means the claim is immediately expired.
        assert!(coord.check("src/main.rs").is_none());
    }

    // -- Persistence tests --

    #[test]
    fn persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");

        // Create and persist.
        {
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid = session("ses-001");
            let _t = coord.acquire("src/main.rs", &sid).unwrap();
            coord.persist().unwrap();
        }

        // Reload and verify.
        {
            let coord = LoroCoordinator::new(&state_path).unwrap();
            let claim = coord.check("src/main.rs").unwrap();
            assert_eq!(claim.owner.as_str(), "ses-001");
            assert_eq!(claim.token.value(), 1);
        }
    }

    #[test]
    fn persist_to_new_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("subdir").join("state.loro");

        let mut coord = LoroCoordinator::new(&state_path).unwrap();
        let sid = session("ses-001");
        let _t = coord.acquire("src/main.rs", &sid).unwrap();
        coord.persist().unwrap();

        assert!(state_path.exists());
    }

    #[test]
    fn reload_preserves_token_counter() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");

        {
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid = session("ses-001");
            let _t1 = coord.acquire("src/a.rs", &sid).unwrap();
            let _t2 = coord.acquire("src/b.rs", &sid).unwrap();
            coord.persist().unwrap();
        }

        {
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid = session("ses-002");
            let t3 = coord.acquire("src/c.rs", &sid).unwrap();
            // Token should be > 2 (the highest in persisted state).
            assert!(t3.value() > 2, "expected token > 2, got {}", t3.value());
        }
    }

    // -- Reserved container tests --

    #[test]
    fn reserved_kg_containers_exist() {
        let coord = make_coordinator();
        let doc = coord.doc();
        // Verify all 6 containers exist in the doc by accessing them.
        let claims = doc.get_map(CLAIMS_CONTAINER);
        assert!(claims.is_empty());

        let _queue = doc.get_list(MERGE_QUEUE_CONTAINER);
        let _entities = doc.get_map(KG_ENTITIES_CONTAINER);
        let _rels = doc.get_map(KG_RELATIONSHIPS_CONTAINER);
        let _meta = doc.get_map(KG_METADATA_CONTAINER);
        let _extract = doc.get_list(EXTRACTION_QUEUE_CONTAINER);
    }

    // -- Merge tests --

    #[test]
    fn merge_two_coordinators() {
        let mut c1 = make_coordinator();
        let mut c2 = make_coordinator();

        let s1 = session("ses-001");
        let s2 = session("ses-002");

        c1.acquire("src/a.rs", &s1).unwrap();
        c2.acquire("src/b.rs", &s2).unwrap();

        c1.merge(&c2).unwrap();

        assert!(c1.check("src/a.rs").is_some());
        assert!(c1.check("src/b.rs").is_some());
    }

    // -- Concurrency tests --

    #[test]
    fn concurrent_two_threads_same_path() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        // Two threads will each create their own coordinator with different peer IDs,
        // acquire the same path, then we merge and verify deterministic outcome.
        let barrier = Arc::new(Barrier::new(2));

        let b1 = Arc::clone(&barrier);
        let h1 = thread::spawn(move || {
            let mut coord = LoroCoordinator::in_memory();
            coord.doc.set_peer_id(1).unwrap();
            let sid = SessionId::new_unchecked("ses-thread-1");
            b1.wait();
            coord.acquire("src/contested.rs", &sid).unwrap();
            coord
        });

        let b2 = Arc::clone(&barrier);
        let h2 = thread::spawn(move || {
            let mut coord = LoroCoordinator::in_memory();
            coord.doc.set_peer_id(2).unwrap();
            let sid = SessionId::new_unchecked("ses-thread-2");
            b2.wait();
            coord.acquire("src/contested.rs", &sid).unwrap();
            coord
        });

        let c1 = h1.join().unwrap();
        let c2 = h2.join().unwrap();

        // Merge c2 into c1 — last-writer-wins on the same key.
        let mut merged = c1;
        merged.merge(&c2).unwrap();

        // The merged result must have exactly one claim for the contested path.
        let claim = merged.check("src/contested.rs").unwrap();
        // Deterministic: one of the two sessions wins.
        assert!(
            claim.owner.as_str() == "ses-thread-1" || claim.owner.as_str() == "ses-thread-2",
            "unexpected winner: {}",
            claim.owner
        );

        // Verify the merge is deterministic by merging in the opposite direction.
        let mut merged2 = c2;
        merged2.merge(&merged).unwrap();
        let claim2 = merged2.check("src/contested.rs").unwrap();
        assert_eq!(
            claim.owner.as_str(),
            claim2.owner.as_str(),
            "merge order should not affect outcome"
        );
    }

    #[test]
    fn concurrent_two_threads_different_paths() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let barrier = Arc::new(Barrier::new(2));

        let b1 = Arc::clone(&barrier);
        let h1 = thread::spawn(move || {
            let mut coord = LoroCoordinator::in_memory();
            coord.doc.set_peer_id(10).unwrap();
            let sid = SessionId::new_unchecked("ses-a");
            b1.wait();
            coord.acquire("src/file_a.rs", &sid).unwrap();
            coord
        });

        let b2 = Arc::clone(&barrier);
        let h2 = thread::spawn(move || {
            let mut coord = LoroCoordinator::in_memory();
            coord.doc.set_peer_id(20).unwrap();
            let sid = SessionId::new_unchecked("ses-b");
            b2.wait();
            coord.acquire("src/file_b.rs", &sid).unwrap();
            coord
        });

        let c1 = h1.join().unwrap();
        let c2 = h2.join().unwrap();

        let mut merged = c1;
        merged.merge(&c2).unwrap();

        // Both claims should exist — no conflict on different keys.
        assert!(merged.check("src/file_a.rs").is_some());
        assert!(merged.check("src/file_b.rs").is_some());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::Config as ProptestConfig;

    /// Strategy to generate a claim operation: (path_index, session_index).
    fn claim_op_strategy() -> impl Strategy<Value = (usize, usize)> {
        (0..5_usize, 0..3_usize)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Verify that Loro CRDT merges are commutative: merging A into B then B into A
        /// produces the same final state regardless of merge order (256 random cases).
        #[test]
        fn merge_is_commutative(
            ops_a in proptest::collection::vec(claim_op_strategy(), 1..10),
            ops_b in proptest::collection::vec(claim_op_strategy(), 1..10),
        ) {
            let paths = ["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs"];
            let sessions = [
                SessionId::new_unchecked("ses-1"),
                SessionId::new_unchecked("ses-2"),
                SessionId::new_unchecked("ses-3"),
            ];

            // Create two coordinators with distinct peer IDs.
            let mut coord_a = LoroCoordinator::in_memory();
            coord_a.doc.set_peer_id(100).unwrap();
            let mut coord_b = LoroCoordinator::in_memory();
            coord_b.doc.set_peer_id(200).unwrap();

            // Apply operations to coordinator A.
            for (path_idx, session_idx) in &ops_a {
                let path = paths[*path_idx];
                let sid = &sessions[*session_idx];
                // Ignore conflicts — we're testing merge commutativity, not conflict logic.
                let _ = coord_a.acquire(path, sid);
            }

            // Apply operations to coordinator B.
            for (path_idx, session_idx) in &ops_b {
                let path = paths[*path_idx];
                let sid = &sessions[*session_idx];
                let _ = coord_b.acquire(path, sid);
            }

            // Export snapshots before merging (since merge consumes state).
            let snap_a = coord_a.doc.export(ExportMode::Snapshot).unwrap();
            let snap_b = coord_b.doc.export(ExportMode::Snapshot).unwrap();

            // Merge A into B.
            let merged_ab = LoroCoordinator::in_memory();
            merged_ab.doc.set_peer_id(300).unwrap();
            merged_ab.doc.import(&snap_a).unwrap();
            merged_ab.doc.import(&snap_b).unwrap();

            // Merge B into A (opposite order).
            let merged_ba = LoroCoordinator::in_memory();
            merged_ba.doc.set_peer_id(400).unwrap();
            merged_ba.doc.import(&snap_b).unwrap();
            merged_ba.doc.import(&snap_a).unwrap();

            // Both merges should produce identical state.
            let state_ab = merged_ab.doc.get_map(CLAIMS_CONTAINER).get_deep_value();
            let state_ba = merged_ba.doc.get_map(CLAIMS_CONTAINER).get_deep_value();

            prop_assert_eq!(state_ab, state_ba, "merge must be commutative");
        }
    }
}
