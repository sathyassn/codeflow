//! Event-type to ledger-file routing.
//!
//! v2 trim: pathflow events (`phase_transition`, `stage_transition`,
//! `session_register`, `session_metadata`, `pathflow_task_update`),
//! coordination events (claims, merge queue), autorun events (batches,
//! workers), and v1 Go-compatibility aliases died with their subsystems.
//!
//! The work-graph event types (`epic_created`, `task_status_changed` and the
//! rest) had no producer and are retired (SPC-013 R-29): record status lives
//! in the Git-tracked records and is written by the status verbs, so no
//! ledger route accepts them. Existing `work-graph` ledger files stay
//! readable by `recall`.

use super::files;
use crate::ledger::LedgerError;

/// Route an event type string to its canonical ledger type name (directory name).
///
/// Returns the type name (e.g., `"work-graph"`) which maps to a subdirectory
/// under `<state_dir>/ledger/`. The caller resolves the full file path based
/// on whether a session ID is present.
///
/// # Errors
///
/// Returns `LedgerError::UnknownEventType` if the event type is not recognized.
pub fn route_event_type(event_type: &str) -> Result<&'static str, LedgerError> {
    match event_type {
        // sessions.jsonl
        "session_start" | "session_end" => Ok(files::SESSIONS),

        // memory-events.jsonl — recall's zero-ceremony corpus (charter §3.3)
        "session_summary" | "decision" | "finding" | "milestone" | "progress" | "blocker" => {
            Ok(files::MEMORY_EVENTS)
        }

        // config.jsonl
        "config_set" | "config_updated" => Ok(files::CONFIG),

        _ => Err(LedgerError::UnknownEventType(event_type.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_events_route_to_sessions() {
        for event_type in &["session_start", "session_end"] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::SESSIONS,
                "{event_type} should route to sessions.jsonl"
            );
        }
    }

    #[test]
    fn test_retired_work_graph_events_are_rejected() {
        for event_type in &[
            "epic_created",
            "epic_status_changed",
            "task_created",
            "task_status_changed",
            "task_updated",
            "begin_work",
            "complete_work",
            "commit",
            "pr_created",
            "pr_merged",
            "test_result_recorded",
        ] {
            assert!(
                route_event_type(event_type).is_err(),
                "retired work-graph event type '{event_type}' must not route"
            );
        }
    }

    #[test]
    fn test_memory_events_route_to_memory_events() {
        for event_type in &[
            "session_summary",
            "decision",
            "finding",
            "milestone",
            "progress",
            "blocker",
        ] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::MEMORY_EVENTS,
                "{event_type} should route to memory-events.jsonl"
            );
        }
    }

    #[test]
    fn test_config_events_route_to_config() {
        for event_type in &["config_set", "config_updated"] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::CONFIG,
                "{event_type} should route to config.jsonl"
            );
        }
    }

    #[test]
    fn test_dropped_v1_event_types_rejected() {
        // Pathflow, coordination, and autorun event types died with their
        // subsystems (charter §3.2) and must not route anywhere.
        for event_type in &[
            "phase_transition",
            "stage_transition",
            "session_register",
            "session_metadata",
            "pathflow_task_update",
            "claim_acquired",
            "claim_conflict",
            "coord_claim_released",
            "scope_expansion",
            "merge_conflict_detected",
            "merge_rebase_attempted",
            "batch_started",
            "batch_completed",
            "batch_aborted",
            "worker_started",
            "worker_completed",
            "worker_failed",
            "worker_timeout",
            "worker_blocked",
            "worker_cancelled",
        ] {
            assert!(
                route_event_type(event_type).is_err(),
                "dropped v1 event type '{event_type}' must not route"
            );
        }
    }

    #[test]
    fn test_unknown_event_type_returns_error() {
        let result = route_event_type("totally_unknown");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("totally_unknown"),
            "error should contain the unknown event type"
        );
    }

    #[test]
    fn test_empty_event_type_returns_error() {
        let result = route_event_type("");
        assert!(result.is_err());
    }

    #[test]
    fn test_all_v2_event_types_routable() {
        let all_variants = [
            "session_start",
            "session_end",
            "session_summary",
            "decision",
            "finding",
            "milestone",
            "progress",
            "blocker",
            "config_set",
            "config_updated",
        ];
        for event_type in &all_variants {
            assert!(
                route_event_type(event_type).is_ok(),
                "v2 event type '{event_type}' must be routable"
            );
        }
    }
}
