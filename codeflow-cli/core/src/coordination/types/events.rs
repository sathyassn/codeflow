//! Coordination event types for parallel execution tracking.
//!
//! These events are emitted to `coordination-events.jsonl` via `ledger/routing.rs`.
//! Full event type definitions and enforcement logic are owned by INF-TSK-023-027;
//! this module provides the minimal placeholder types needed for INF-TSK-023-024
//! wiring.

use serde::{Deserialize, Serialize};

use crate::types::SessionId;

/// A coordination event emitted during parallel execution.
///
/// Serialized as JSON and appended to `coordination-events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum CoordinationEvent {
    /// A file claim was successfully acquired.
    #[serde(rename = "claim_acquired")]
    ClaimAcquired {
        session_id: SessionId,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },

    /// A claim acquisition was blocked by a conflict.
    #[serde(rename = "claim_conflict")]
    ClaimConflict {
        session_id: SessionId,
        path: String,
        held_by: SessionId,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },

    /// A file claim was released.
    #[serde(rename = "coord_claim_released")]
    ClaimReleased {
        session_id: SessionId,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },

    /// A worker expanded its scope to claim an out-of-scope file.
    #[serde(rename = "scope_expansion")]
    ScopeExpansion {
        session_id: SessionId,
        path: String,
        original_scope: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },

    /// A merge conflict was detected before PR creation.
    #[serde(rename = "merge_conflict_detected")]
    MergeConflictDetected {
        session_id: SessionId,
        branch: String,
        target_branch: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },

    /// A merge rebase was attempted to resolve conflicts.
    #[serde(rename = "merge_rebase_attempted")]
    MergeRebaseAttempted {
        session_id: SessionId,
        branch: String,
        target_branch: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        timestamp: String,
    },
}

impl CoordinationEvent {
    /// Return the event type string for ledger routing.
    #[must_use]
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::ClaimAcquired { .. } => "claim_acquired",
            Self::ClaimConflict { .. } => "claim_conflict",
            Self::ClaimReleased { .. } => "coord_claim_released",
            Self::ScopeExpansion { .. } => "scope_expansion",
            Self::MergeConflictDetected { .. } => "merge_conflict_detected",
            Self::MergeRebaseAttempted { .. } => "merge_rebase_attempted",
        }
    }
}

/// An autorun lifecycle event emitted during batch execution.
///
/// Serialized as JSON and appended to `autorun-events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum AutorunEvent {
    /// A batch execution has started.
    #[serde(rename = "batch_started")]
    BatchStarted {
        session_id: String,
        batch_name: String,
        total_tasks: i32,
        timestamp: String,
    },

    /// A batch execution completed successfully.
    #[serde(rename = "batch_completed")]
    BatchCompleted {
        session_id: String,
        batch_name: String,
        completed_tasks: i32,
        failed_tasks: i32,
        skipped_tasks: i32,
        timestamp: String,
    },

    /// A batch execution was aborted.
    #[serde(rename = "batch_aborted")]
    BatchAborted {
        session_id: String,
        batch_name: String,
        reason: String,
        timestamp: String,
    },

    /// A worker started executing a task.
    #[serde(rename = "worker_started")]
    WorkerStarted {
        session_id: String,
        worker_id: String,
        task_id: String,
        timestamp: String,
    },

    /// A worker completed its task successfully.
    #[serde(rename = "worker_completed")]
    WorkerCompleted {
        session_id: String,
        worker_id: String,
        task_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pr_number: Option<i64>,
        timestamp: String,
    },

    /// A worker failed to complete its task.
    #[serde(rename = "worker_failed")]
    WorkerFailed {
        session_id: String,
        worker_id: String,
        task_id: String,
        error: String,
        timestamp: String,
    },

    /// A worker timed out.
    #[serde(rename = "worker_timeout")]
    WorkerTimeout {
        session_id: String,
        worker_id: String,
        task_id: String,
        timestamp: String,
    },

    /// A worker was blocked (e.g., by claim conflicts).
    #[serde(rename = "worker_blocked")]
    WorkerBlocked {
        session_id: String,
        worker_id: String,
        task_id: String,
        reason: String,
        timestamp: String,
    },

    /// A worker was cancelled (e.g., dependency failed).
    #[serde(rename = "worker_cancelled")]
    WorkerCancelled {
        session_id: String,
        worker_id: String,
        task_id: String,
        reason: String,
        timestamp: String,
    },
}

impl AutorunEvent {
    /// Return the event type string for ledger routing.
    #[must_use]
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::BatchStarted { .. } => "batch_started",
            Self::BatchCompleted { .. } => "batch_completed",
            Self::BatchAborted { .. } => "batch_aborted",
            Self::WorkerStarted { .. } => "worker_started",
            Self::WorkerCompleted { .. } => "worker_completed",
            Self::WorkerFailed { .. } => "worker_failed",
            Self::WorkerTimeout { .. } => "worker_timeout",
            Self::WorkerBlocked { .. } => "worker_blocked",
            Self::WorkerCancelled { .. } => "worker_cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    #[test]
    fn claim_acquired_serde_roundtrip() {
        let event = CoordinationEvent::ClaimAcquired {
            session_id: session("ses-001"),
            path: "src/main.rs".to_string(),
            task_id: Some("TSK-001".to_string()),
            timestamp: "2026-03-21T10:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("claim_acquired"));
        assert!(json.contains("src/main.rs"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn claim_conflict_serde_roundtrip() {
        let event = CoordinationEvent::ClaimConflict {
            session_id: session("ses-002"),
            path: "src/lib.rs".to_string(),
            held_by: session("ses-001"),
            task_id: None,
            timestamp: "2026-03-21T10:01:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("claim_conflict"));
        assert!(json.contains("ses-001"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn claim_released_serde_roundtrip() {
        let event = CoordinationEvent::ClaimReleased {
            session_id: session("ses-001"),
            path: "src/main.rs".to_string(),
            task_id: Some("TSK-001".to_string()),
            timestamp: "2026-03-21T10:02:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("coord_claim_released"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn scope_expansion_serde_roundtrip() {
        let event = CoordinationEvent::ScopeExpansion {
            session_id: session("ses-001"),
            path: "src/extra.rs".to_string(),
            original_scope: vec!["src/main.rs".to_string()],
            task_id: Some("TSK-001".to_string()),
            timestamp: "2026-03-21T10:03:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("scope_expansion"));
        assert!(json.contains("src/extra.rs"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn event_type_strings_correct() {
        let acquired = CoordinationEvent::ClaimAcquired {
            session_id: session("ses-001"),
            path: "a.rs".to_string(),
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(acquired.event_type(), "claim_acquired");

        let conflict = CoordinationEvent::ClaimConflict {
            session_id: session("ses-002"),
            path: "a.rs".to_string(),
            held_by: session("ses-001"),
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(conflict.event_type(), "claim_conflict");

        let released = CoordinationEvent::ClaimReleased {
            session_id: session("ses-001"),
            path: "a.rs".to_string(),
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(released.event_type(), "coord_claim_released");

        let expansion = CoordinationEvent::ScopeExpansion {
            session_id: session("ses-001"),
            path: "b.rs".to_string(),
            original_scope: vec![],
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(expansion.event_type(), "scope_expansion");

        let merge_conflict = CoordinationEvent::MergeConflictDetected {
            session_id: session("ses-001"),
            branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(merge_conflict.event_type(), "merge_conflict_detected");

        let rebase = CoordinationEvent::MergeRebaseAttempted {
            session_id: session("ses-001"),
            branch: "feat/test".to_string(),
            target_branch: "main".to_string(),
            success: true,
            task_id: None,
            timestamp: "ts".to_string(),
        };
        assert_eq!(rebase.event_type(), "merge_rebase_attempted");
    }

    #[test]
    fn claim_acquired_without_task_id_omits_field() {
        let event = CoordinationEvent::ClaimAcquired {
            session_id: session("ses-001"),
            path: "a.rs".to_string(),
            task_id: None,
            timestamp: "ts".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            !json.contains("task_id"),
            "task_id should be omitted when None"
        );
    }

    #[test]
    fn merge_conflict_detected_serde_roundtrip() {
        let event = CoordinationEvent::MergeConflictDetected {
            session_id: session("ses-001"),
            branch: "feat/my-feature".to_string(),
            target_branch: "main".to_string(),
            task_id: Some("TSK-001".to_string()),
            timestamp: "2026-03-21T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("merge_conflict_detected"));
        assert!(json.contains("feat/my-feature"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_batch_started_serde_roundtrip() {
        let event = AutorunEvent::BatchStarted {
            session_id: "ses-001".to_string(),
            batch_name: "test-batch".to_string(),
            total_tasks: 5,
            timestamp: "2026-03-21T10:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("batch_started"));
        assert!(json.contains("test-batch"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_batch_completed_serde_roundtrip() {
        let event = AutorunEvent::BatchCompleted {
            session_id: "ses-001".to_string(),
            batch_name: "test-batch".to_string(),
            completed_tasks: 3,
            failed_tasks: 1,
            skipped_tasks: 1,
            timestamp: "2026-03-21T11:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("batch_completed"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_batch_aborted_serde_roundtrip() {
        let event = AutorunEvent::BatchAborted {
            session_id: "ses-001".to_string(),
            batch_name: "test-batch".to_string(),
            reason: "fatal error".to_string(),
            timestamp: "2026-03-21T11:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("batch_aborted"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_started_serde_roundtrip() {
        let event = AutorunEvent::WorkerStarted {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            timestamp: "2026-03-21T10:01:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_started"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_completed_serde_roundtrip() {
        let event = AutorunEvent::WorkerCompleted {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            pr_number: Some(42),
            timestamp: "2026-03-21T10:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_completed"));
        assert!(json.contains("42"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_completed_no_pr_omits_field() {
        let event = AutorunEvent::WorkerCompleted {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            pr_number: None,
            timestamp: "2026-03-21T10:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            !json.contains("pr_number"),
            "pr_number should be omitted when None"
        );
    }

    #[test]
    fn autorun_worker_failed_serde_roundtrip() {
        let event = AutorunEvent::WorkerFailed {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            error: "compilation failed".to_string(),
            timestamp: "2026-03-21T10:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_failed"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_timeout_serde_roundtrip() {
        let event = AutorunEvent::WorkerTimeout {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            timestamp: "2026-03-21T11:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_timeout"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_blocked_serde_roundtrip() {
        let event = AutorunEvent::WorkerBlocked {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-a".to_string(),
            task_id: "task-a".to_string(),
            reason: "claim conflict on src/main.rs".to_string(),
            timestamp: "2026-03-21T10:05:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_blocked"));
        assert!(json.contains("claim conflict"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_worker_cancelled_serde_roundtrip() {
        let event = AutorunEvent::WorkerCancelled {
            session_id: "ses-001".to_string(),
            worker_id: "arw-task-b".to_string(),
            task_id: "task-b".to_string(),
            reason: "dependency task-a failed".to_string(),
            timestamp: "2026-03-21T10:10:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("worker_cancelled"));
        let parsed: AutorunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn autorun_event_type_strings_correct() {
        let variants: Vec<(AutorunEvent, &str)> = vec![
            (
                AutorunEvent::BatchStarted {
                    session_id: "s".into(),
                    batch_name: "b".into(),
                    total_tasks: 0,
                    timestamp: "t".into(),
                },
                "batch_started",
            ),
            (
                AutorunEvent::BatchCompleted {
                    session_id: "s".into(),
                    batch_name: "b".into(),
                    completed_tasks: 0,
                    failed_tasks: 0,
                    skipped_tasks: 0,
                    timestamp: "t".into(),
                },
                "batch_completed",
            ),
            (
                AutorunEvent::BatchAborted {
                    session_id: "s".into(),
                    batch_name: "b".into(),
                    reason: "r".into(),
                    timestamp: "t".into(),
                },
                "batch_aborted",
            ),
            (
                AutorunEvent::WorkerStarted {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    timestamp: "ts".into(),
                },
                "worker_started",
            ),
            (
                AutorunEvent::WorkerCompleted {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    pr_number: None,
                    timestamp: "ts".into(),
                },
                "worker_completed",
            ),
            (
                AutorunEvent::WorkerFailed {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    error: "e".into(),
                    timestamp: "ts".into(),
                },
                "worker_failed",
            ),
            (
                AutorunEvent::WorkerTimeout {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    timestamp: "ts".into(),
                },
                "worker_timeout",
            ),
            (
                AutorunEvent::WorkerBlocked {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    reason: "r".into(),
                    timestamp: "ts".into(),
                },
                "worker_blocked",
            ),
            (
                AutorunEvent::WorkerCancelled {
                    session_id: "s".into(),
                    worker_id: "w".into(),
                    task_id: "t".into(),
                    reason: "r".into(),
                    timestamp: "ts".into(),
                },
                "worker_cancelled",
            ),
        ];

        for (event, expected_type) in variants {
            assert_eq!(
                event.event_type(),
                expected_type,
                "event_type() mismatch for {expected_type}"
            );
        }
    }

    #[test]
    fn merge_rebase_attempted_serde_roundtrip() {
        let event = CoordinationEvent::MergeRebaseAttempted {
            session_id: session("ses-001"),
            branch: "feat/my-feature".to_string(),
            target_branch: "main".to_string(),
            success: false,
            task_id: None,
            timestamp: "2026-03-21T12:01:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("merge_rebase_attempted"));
        assert!(json.contains("\"success\":false"));
        let parsed: CoordinationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }
}
