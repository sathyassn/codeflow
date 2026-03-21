//! FIFO merge queue for ordering PR merges across parallel sessions.
//!
//! Uses the `merge_queue` `LoroList` container in the shared `LoroDoc`.
//! Each entry is serialized as a JSON string and appended to the list.
//! Dequeue reads index 0 and deletes it (FIFO ordering).
//!
//! All operations on the `LoroDoc` MUST go through `locked_binary_rmw`
//! to prevent races when multiple sessions enqueue/dequeue simultaneously.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::coordination::loro::LoroCoordinator;
use crate::error::CoordinationError;
use crate::file_lock::locked_binary_rmw;
use crate::types::SessionId;

use loro::{LoroValue, ValueOrContainer};

/// Container name for the merge queue list (matches loro.rs constant).
const MERGE_QUEUE_CONTAINER: &str = "merge_queue";

/// A merge queue entry representing a session ready to merge its PR.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeQueueEntry {
    /// Session that completed work and is ready to merge.
    pub session_id: SessionId,
    /// Task ID being worked on.
    pub task_id: String,
    /// Branch name for the PR.
    pub branch: String,
    /// ISO 8601 timestamp of when the session became ready to merge.
    pub pr_ready_at: String,
}

/// Enqueue a session's PR merge request into the FIFO queue.
///
/// Appends the entry as a JSON string to the end of the `merge_queue` `LoroList`.
///
/// # Errors
///
/// Returns `CoordinationError::Serialization` if JSON serialization fails.
/// Returns `CoordinationError::Loro` if the LoroList operation fails.
pub fn enqueue(
    coordinator: &LoroCoordinator,
    entry: &MergeQueueEntry,
) -> Result<(), CoordinationError> {
    let queue = coordinator.doc().get_list(MERGE_QUEUE_CONTAINER);
    let json = serde_json::to_string(entry)?;
    queue.push(json)?;
    coordinator.doc().commit();
    Ok(())
}

/// Dequeue the front entry if it belongs to the requesting session.
///
/// Peeks at index 0 of the `merge_queue` `LoroList` and only removes it
/// if the entry's `session_id` matches the provided `session_id`. Returns
/// `None` if the queue is empty or the front entry belongs to a different
/// session (preventing cross-session dequeue in concurrent batches).
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` if the entry cannot be parsed.
/// Returns `CoordinationError::Loro` if the LoroList operation fails.
pub fn dequeue(
    coordinator: &LoroCoordinator,
    session_id: &SessionId,
) -> Result<Option<MergeQueueEntry>, CoordinationError> {
    let queue = coordinator.doc().get_list(MERGE_QUEUE_CONTAINER);
    let len = queue.len();
    if len == 0 {
        return Ok(None);
    }

    let entry = read_entry_at(&queue, 0)?;
    match &entry {
        Some(e) if e.session_id == *session_id => {
            queue.delete(0, 1)?;
            coordinator.doc().commit();
            Ok(entry)
        }
        _ => Ok(None),
    }
}

/// Peek at the front entry without removing it.
///
/// Returns `None` if the queue is empty.
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` if the entry cannot be parsed.
pub fn peek(coordinator: &LoroCoordinator) -> Result<Option<MergeQueueEntry>, CoordinationError> {
    let queue = coordinator.doc().get_list(MERGE_QUEUE_CONTAINER);
    if queue.is_empty() {
        return Ok(None);
    }
    read_entry_at(&queue, 0)
}

/// Get the queue position for a session (0-indexed).
///
/// Returns `None` if the session is not in the queue.
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` if any entry cannot be parsed.
pub fn position(
    coordinator: &LoroCoordinator,
    session_id: &SessionId,
) -> Result<Option<usize>, CoordinationError> {
    let queue = coordinator.doc().get_list(MERGE_QUEUE_CONTAINER);
    let len = queue.len();
    for i in 0..len {
        if let Some(entry) = read_entry_at(&queue, i)? {
            if entry.session_id == *session_id {
                return Ok(Some(i));
            }
        }
    }
    Ok(None)
}

/// Get the total number of entries in the queue.
#[must_use]
pub fn queue_len(coordinator: &LoroCoordinator) -> usize {
    coordinator.doc().get_list(MERGE_QUEUE_CONTAINER).len()
}

/// Read a queue entry at a specific index without removing it.
///
/// Returns `None` if the index is out of bounds.
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` if the entry cannot be parsed.
pub fn peek_at(
    coordinator: &LoroCoordinator,
    index: usize,
) -> Result<Option<MergeQueueEntry>, CoordinationError> {
    let queue = coordinator.doc().get_list(MERGE_QUEUE_CONTAINER);
    if index >= queue.len() {
        return Ok(None);
    }
    read_entry_at(&queue, index)
}

/// Enqueue under a file lock (for disk-backed state).
///
/// Uses `locked_binary_rmw` to ensure exclusive access to state.loro
/// during the enqueue operation.
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` on lock or operation failure.
pub fn locked_enqueue(state_path: &Path, entry: &MergeQueueEntry) -> Result<(), CoordinationError> {
    let entry_clone = entry.clone();
    locked_binary_rmw(
        state_path,
        LoroCoordinator::in_memory,
        |bytes| {
            LoroCoordinator::from_bytes(bytes, state_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| enqueue(coord, &entry_clone).map_err(|e| format!("enqueue: {e}")),
    )
    .map_err(|e| CoordinationError::MergeQueue(format!("locked enqueue: {e}")))
}

/// Dequeue under a file lock (for disk-backed state).
///
/// Only dequeues the front entry if it belongs to the given `session_id`.
///
/// # Errors
///
/// Returns `CoordinationError::MergeQueue` on lock or operation failure.
pub fn locked_dequeue(
    state_path: &Path,
    session_id: &SessionId,
) -> Result<Option<MergeQueueEntry>, CoordinationError> {
    let mut result = None;
    let sid = session_id.clone();
    locked_binary_rmw(
        state_path,
        LoroCoordinator::in_memory,
        |bytes| {
            LoroCoordinator::from_bytes(bytes, state_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            result = dequeue(coord, &sid).map_err(|e| format!("dequeue: {e}"))?;
            Ok(())
        },
    )
    .map_err(|e| CoordinationError::MergeQueue(format!("locked dequeue: {e}")))?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a `MergeQueueEntry` from a LoroList at the given index.
fn read_entry_at(
    queue: &loro::LoroList,
    index: usize,
) -> Result<Option<MergeQueueEntry>, CoordinationError> {
    let Some(voc) = queue.get(index) else {
        return Ok(None);
    };
    match voc {
        ValueOrContainer::Value(LoroValue::String(s)) => {
            let entry: MergeQueueEntry = serde_json::from_str(&s).map_err(|e| {
                CoordinationError::MergeQueue(format!("parse entry at {index}: {e}"))
            })?;
            Ok(Some(entry))
        }
        _ => Err(CoordinationError::MergeQueue(format!(
            "unexpected value type at index {index}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    fn make_entry(sid: &str, task: &str, branch: &str, ts: &str) -> MergeQueueEntry {
        MergeQueueEntry {
            session_id: session(sid),
            task_id: task.to_string(),
            branch: branch.to_string(),
            pr_ready_at: ts.to_string(),
        }
    }

    #[test]
    fn test_enqueue_dequeue_fifo() {
        let coord = LoroCoordinator::in_memory();
        let e1 = make_entry("ses-001", "TSK-001", "feat/a", "2026-03-19T10:00:00Z");
        let e2 = make_entry("ses-002", "TSK-002", "feat/b", "2026-03-19T10:01:00Z");
        let e3 = make_entry("ses-003", "TSK-003", "feat/c", "2026-03-19T10:02:00Z");

        enqueue(&coord, &e1).unwrap();
        enqueue(&coord, &e2).unwrap();
        enqueue(&coord, &e3).unwrap();

        assert_eq!(queue_len(&coord), 3);

        // Dequeue in FIFO order (each session dequeues its own entry).
        let d1 = dequeue(&coord, &session("ses-001")).unwrap().unwrap();
        assert_eq!(d1.session_id.as_str(), "ses-001");
        let d2 = dequeue(&coord, &session("ses-002")).unwrap().unwrap();
        assert_eq!(d2.session_id.as_str(), "ses-002");
        let d3 = dequeue(&coord, &session("ses-003")).unwrap().unwrap();
        assert_eq!(d3.session_id.as_str(), "ses-003");

        assert_eq!(queue_len(&coord), 0);
    }

    #[test]
    fn test_peek_does_not_remove() {
        let coord = LoroCoordinator::in_memory();
        let e1 = make_entry("ses-001", "TSK-001", "feat/a", "2026-03-19T10:00:00Z");
        enqueue(&coord, &e1).unwrap();

        let peeked = peek(&coord).unwrap().unwrap();
        assert_eq!(peeked.session_id.as_str(), "ses-001");
        // Queue should still have 1 entry.
        assert_eq!(queue_len(&coord), 1);

        // Peek again — same result.
        let peeked2 = peek(&coord).unwrap().unwrap();
        assert_eq!(peeked2, peeked);
    }

    #[test]
    fn test_position_correct_index() {
        let coord = LoroCoordinator::in_memory();
        enqueue(&coord, &make_entry("ses-001", "T1", "b1", "ts1")).unwrap();
        enqueue(&coord, &make_entry("ses-002", "T2", "b2", "ts2")).unwrap();
        enqueue(&coord, &make_entry("ses-003", "T3", "b3", "ts3")).unwrap();

        assert_eq!(position(&coord, &session("ses-001")).unwrap(), Some(0));
        assert_eq!(position(&coord, &session("ses-002")).unwrap(), Some(1));
        assert_eq!(position(&coord, &session("ses-003")).unwrap(), Some(2));
    }

    #[test]
    fn test_position_not_in_queue() {
        let coord = LoroCoordinator::in_memory();
        enqueue(&coord, &make_entry("ses-001", "T1", "b1", "ts1")).unwrap();

        assert_eq!(position(&coord, &session("ses-999")).unwrap(), None);
    }

    #[test]
    fn test_dequeue_empty_queue() {
        let coord = LoroCoordinator::in_memory();
        let result = dequeue(&coord, &session("ses-any")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_peek_empty_queue() {
        let coord = LoroCoordinator::in_memory();
        let result = peek(&coord).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_queue_entry_serde_roundtrip() {
        let entry = make_entry("ses-001", "TSK-001", "feat/test", "2026-03-19T10:00:00Z");
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: MergeQueueEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_queue_len_tracks_enqueue_dequeue() {
        let coord = LoroCoordinator::in_memory();
        assert_eq!(queue_len(&coord), 0);

        enqueue(&coord, &make_entry("ses-001", "T1", "b1", "ts1")).unwrap();
        assert_eq!(queue_len(&coord), 1);

        enqueue(&coord, &make_entry("ses-002", "T2", "b2", "ts2")).unwrap();
        assert_eq!(queue_len(&coord), 2);

        dequeue(&coord, &session("ses-001")).unwrap();
        assert_eq!(queue_len(&coord), 1);
    }

    #[test]
    fn test_concurrent_enqueue_deterministic() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        // Three threads enqueue into separate LoroCoordinators, then merge.
        let barrier = Arc::new(Barrier::new(3));

        let handles: Vec<_> = (0..3)
            .map(|i| {
                let b = Arc::clone(&barrier);
                thread::spawn(move || {
                    let coord = LoroCoordinator::in_memory();
                    coord
                        .doc()
                        .set_peer_id(u64::try_from(i + 100).unwrap())
                        .unwrap();
                    b.wait();
                    let entry = MergeQueueEntry {
                        session_id: SessionId::new_unchecked(format!("ses-{i:03}")),
                        task_id: format!("TSK-{i:03}"),
                        branch: format!("feat/{i}"),
                        pr_ready_at: format!("2026-03-19T10:{i:02}:00Z"),
                    };
                    enqueue(&coord, &entry).unwrap();
                    coord
                })
            })
            .collect();

        let coordinators: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Merge all into a single coordinator.
        let mut merged = LoroCoordinator::in_memory();
        merged.doc().set_peer_id(999).unwrap();
        for coord in &coordinators {
            merged.merge(coord).unwrap();
        }

        // All 3 entries should be present after merge.
        assert_eq!(queue_len(&merged), 3);

        // Verify all sessions are in the queue using peek+position.
        let mut found_sessions = Vec::new();
        for i in 0..3 {
            let entry = peek_at(&merged, i).unwrap().unwrap();
            found_sessions.push(entry.session_id.as_str().to_string());
        }
        found_sessions.sort();
        assert_eq!(found_sessions, vec!["ses-000", "ses-001", "ses-002"]);
    }

    #[test]
    fn test_locked_enqueue_dequeue() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");

        let e1 = make_entry("ses-001", "TSK-001", "feat/a", "2026-03-19T10:00:00Z");
        let e2 = make_entry("ses-002", "TSK-002", "feat/b", "2026-03-19T10:01:00Z");

        locked_enqueue(&state_path, &e1).unwrap();
        locked_enqueue(&state_path, &e2).unwrap();

        let d1 = locked_dequeue(&state_path, &session("ses-001"))
            .unwrap()
            .unwrap();
        assert_eq!(d1.session_id.as_str(), "ses-001");

        let d2 = locked_dequeue(&state_path, &session("ses-002"))
            .unwrap()
            .unwrap();
        assert_eq!(d2.session_id.as_str(), "ses-002");

        let d3 = locked_dequeue(&state_path, &session("ses-any")).unwrap();
        assert!(d3.is_none());
    }

    #[test]
    fn test_locked_dequeue_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");
        let result = locked_dequeue(&state_path, &session("ses-any")).unwrap();
        assert!(result.is_none());
    }

    // -- Session-verified dequeue tests --

    #[test]
    fn test_dequeue_mismatch_returns_none_without_removing() {
        let coord = LoroCoordinator::in_memory();
        let e1 = make_entry("ses-owner", "TSK-001", "feat/a", "2026-03-19T10:00:00Z");
        enqueue(&coord, &e1).unwrap();

        // A different session tries to dequeue -- should get None.
        let result = dequeue(&coord, &session("ses-other")).unwrap();
        assert!(result.is_none(), "mismatched session_id should return None");

        // Entry should still be in the queue.
        assert_eq!(
            queue_len(&coord),
            1,
            "entry must not be removed on mismatch"
        );
        let peeked = peek(&coord).unwrap().unwrap();
        assert_eq!(peeked.session_id.as_str(), "ses-owner");
    }

    #[test]
    fn test_dequeue_match_returns_and_removes() {
        let coord = LoroCoordinator::in_memory();
        let e1 = make_entry("ses-owner", "TSK-001", "feat/a", "2026-03-19T10:00:00Z");
        enqueue(&coord, &e1).unwrap();

        // The owning session dequeues -- should succeed.
        let result = dequeue(&coord, &session("ses-owner")).unwrap().unwrap();
        assert_eq!(result.session_id.as_str(), "ses-owner");
        assert_eq!(
            queue_len(&coord),
            0,
            "entry must be removed after matching dequeue"
        );
    }

    #[test]
    fn test_concurrent_dequeue_different_sessions_non_owner_gets_none() {
        let coord = LoroCoordinator::in_memory();
        enqueue(&coord, &make_entry("ses-A", "TSK-A", "feat/a", "ts1")).unwrap();
        enqueue(&coord, &make_entry("ses-B", "TSK-B", "feat/b", "ts2")).unwrap();

        // ses-B tries to dequeue first -- front is ses-A, so mismatch.
        let result_b = dequeue(&coord, &session("ses-B")).unwrap();
        assert!(result_b.is_none(), "ses-B should not dequeue ses-A's entry");
        assert_eq!(queue_len(&coord), 2, "no entries removed");

        // ses-A dequeues successfully.
        let result_a = dequeue(&coord, &session("ses-A")).unwrap().unwrap();
        assert_eq!(result_a.session_id.as_str(), "ses-A");
        assert_eq!(queue_len(&coord), 1);

        // Now ses-B is at the front and can dequeue.
        let result_b2 = dequeue(&coord, &session("ses-B")).unwrap().unwrap();
        assert_eq!(result_b2.session_id.as_str(), "ses-B");
        assert_eq!(queue_len(&coord), 0);
    }

    // -- Integration test scenarios --

    #[test]
    fn test_scenario_a_two_interactive_sessions() {
        // Two interactive sessions register worktrees and enqueue.
        let coord = LoroCoordinator::in_memory();
        let e1 = make_entry(
            "ses-interactive-1",
            "TSK-A1",
            "feat/a",
            "2026-03-19T10:00:00Z",
        );
        let e2 = make_entry(
            "ses-interactive-2",
            "TSK-A2",
            "feat/b",
            "2026-03-19T10:05:00Z",
        );

        enqueue(&coord, &e1).unwrap();
        enqueue(&coord, &e2).unwrap();

        // Both are in queue; first to enqueue is first to merge.
        assert_eq!(
            position(&coord, &session("ses-interactive-1")).unwrap(),
            Some(0)
        );
        assert_eq!(
            position(&coord, &session("ses-interactive-2")).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn test_scenario_b_autorun_batch_3_workers() {
        // Autorun batch: 3 workers enqueue in order of completion.
        let coord = LoroCoordinator::in_memory();
        // Worker 2 finishes first, then worker 0, then worker 1.
        enqueue(
            &coord,
            &make_entry("ses-worker-2", "TSK-B2", "feat/w2", "2026-03-19T10:00:00Z"),
        )
        .unwrap();
        enqueue(
            &coord,
            &make_entry("ses-worker-0", "TSK-B0", "feat/w0", "2026-03-19T10:01:00Z"),
        )
        .unwrap();
        enqueue(
            &coord,
            &make_entry("ses-worker-1", "TSK-B1", "feat/w1", "2026-03-19T10:02:00Z"),
        )
        .unwrap();

        // Merge order should be: worker-2, worker-0, worker-1.
        let d1 = dequeue(&coord, &session("ses-worker-2")).unwrap().unwrap();
        assert_eq!(d1.session_id.as_str(), "ses-worker-2");
        let d2 = dequeue(&coord, &session("ses-worker-0")).unwrap().unwrap();
        assert_eq!(d2.session_id.as_str(), "ses-worker-0");
        let d3 = dequeue(&coord, &session("ses-worker-1")).unwrap().unwrap();
        assert_eq!(d3.session_id.as_str(), "ses-worker-1");
    }

    #[test]
    fn test_scenario_c_interactive_plus_autorun() {
        // Interactive session + autorun worker coexist.
        let coord = LoroCoordinator::in_memory();
        enqueue(
            &coord,
            &make_entry(
                "ses-interactive",
                "TSK-C1",
                "feat/i",
                "2026-03-19T10:00:00Z",
            ),
        )
        .unwrap();
        enqueue(
            &coord,
            &make_entry("ses-autorun-w1", "TSK-C2", "feat/a", "2026-03-19T10:01:00Z"),
        )
        .unwrap();

        assert_eq!(queue_len(&coord), 2);
        assert_eq!(
            position(&coord, &session("ses-interactive")).unwrap(),
            Some(0)
        );
        assert_eq!(
            position(&coord, &session("ses-autorun-w1")).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn test_scenario_d_fifo_tiebreaking_via_timestamp_and_ulid() {
        // Two sessions complete near-simultaneously. FIFO ordering via
        // enqueue order (LoroList preserves per-peer insertion order).
        // After CRDT merge, pr_ready_at provides total ordering.
        let mut c1 = LoroCoordinator::in_memory();
        c1.doc().set_peer_id(1).unwrap();
        let c2 = LoroCoordinator::in_memory();
        c2.doc().set_peer_id(2).unwrap();

        // Session with earlier timestamp enqueues on peer 1.
        enqueue(
            &c1,
            &make_entry(
                "ses-01jq7aaaa000000000000001",
                "TSK-D1",
                "feat/d1",
                "2026-03-19T10:00:00.000Z",
            ),
        )
        .unwrap();

        // Session with later timestamp enqueues on peer 2.
        enqueue(
            &c2,
            &make_entry(
                "ses-01jq7aaaa000000000000002",
                "TSK-D2",
                "feat/d2",
                "2026-03-19T10:00:00.001Z",
            ),
        )
        .unwrap();

        // Merge both.
        c1.merge(&c2).unwrap();

        // Both entries should be in the queue.
        assert_eq!(queue_len(&c1), 2);

        // Verify both entries are present using peek_at (dequeue requires matching session_id).
        let mut sids = Vec::new();
        for i in 0..2 {
            let entry = peek_at(&c1, i).unwrap().unwrap();
            sids.push(entry.session_id.as_str().to_string());
        }
        sids.sort();
        assert_eq!(
            sids,
            vec![
                "ses-01jq7aaaa000000000000001",
                "ses-01jq7aaaa000000000000002",
            ]
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::Config as ProptestConfig;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Verify that concurrent enqueue operations are commutative:
        /// merging coordinators in any order produces the same final queue
        /// entries (regardless of ordering within the list).
        #[test]
        fn concurrent_enqueue_commutative(
            num_entries_a in 1..5_usize,
            num_entries_b in 1..5_usize,
        ) {
            let coord_a = LoroCoordinator::in_memory();
            coord_a.doc().set_peer_id(100).unwrap();
            let coord_b = LoroCoordinator::in_memory();
            coord_b.doc().set_peer_id(200).unwrap();

            // Enqueue entries on coordinator A.
            for i in 0..num_entries_a {
                let entry = MergeQueueEntry {
                    session_id: SessionId::new_unchecked(format!("ses-a-{i:03}")),
                    task_id: format!("TSK-A-{i:03}"),
                    branch: format!("feat/a-{i}"),
                    pr_ready_at: format!("2026-03-19T10:{i:02}:00Z"),
                };
                enqueue(&coord_a, &entry).unwrap();
            }

            // Enqueue entries on coordinator B.
            for i in 0..num_entries_b {
                let entry = MergeQueueEntry {
                    session_id: SessionId::new_unchecked(format!("ses-b-{i:03}")),
                    task_id: format!("TSK-B-{i:03}"),
                    branch: format!("feat/b-{i}"),
                    pr_ready_at: format!("2026-03-19T11:{i:02}:00Z"),
                };
                enqueue(&coord_b, &entry).unwrap();
            }

            // Export snapshots.
            let snap_a = coord_a.export_bytes().unwrap();
            let snap_b = coord_b.export_bytes().unwrap();

            // Merge A into B.
            let merged_ab = LoroCoordinator::in_memory();
            merged_ab.doc().set_peer_id(300).unwrap();
            merged_ab.doc().import(&snap_a).unwrap();
            merged_ab.doc().import(&snap_b).unwrap();

            // Merge B into A (opposite order).
            let merged_ba = LoroCoordinator::in_memory();
            merged_ba.doc().set_peer_id(400).unwrap();
            merged_ba.doc().import(&snap_b).unwrap();
            merged_ba.doc().import(&snap_a).unwrap();

            // Both merges should have the same number of entries.
            let len_ab = queue_len(&merged_ab);
            let len_ba = queue_len(&merged_ba);
            prop_assert_eq!(len_ab, len_ba, "queue lengths must match");
            prop_assert_eq!(
                len_ab,
                num_entries_a + num_entries_b,
                "total entries must equal sum of inputs"
            );

            // Extract all session IDs from both merged queues and sort.
            let mut sids_ab = Vec::new();
            let queue_ab = merged_ab.doc().get_list(MERGE_QUEUE_CONTAINER);
            for i in 0..len_ab {
                if let Some(entry) = read_entry_at(&queue_ab, i).unwrap() {
                    sids_ab.push(entry.session_id.as_str().to_string());
                }
            }
            sids_ab.sort();

            let mut sids_ba = Vec::new();
            let queue_ba = merged_ba.doc().get_list(MERGE_QUEUE_CONTAINER);
            for i in 0..len_ba {
                if let Some(entry) = read_entry_at(&queue_ba, i).unwrap() {
                    sids_ba.push(entry.session_id.as_str().to_string());
                }
            }
            sids_ba.sort();

            prop_assert_eq!(
                sids_ab,
                sids_ba,
                "merge must be commutative: same entries regardless of order"
            );
        }
    }
}
