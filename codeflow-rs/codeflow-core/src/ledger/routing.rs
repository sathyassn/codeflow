use super::files;
use crate::error::LedgerError;

/// Route an event type string to its canonical ledger file name.
///
/// Covers all 32 `LedgerEvent` variants plus Go-compatibility event types
/// (`session_progress`, `work_claimed`, `claim_created`, `claim_released`,
/// `claim_renewed`, `work_complete`, `pr_merged`, `memory_stored`, `blocker`).
///
/// # Errors
///
/// Returns `LedgerError::UnknownEventType` if the event type is not recognized.
pub fn route_event_type(event_type: &str) -> Result<&'static str, LedgerError> {
    match event_type {
        // sessions.jsonl
        "session_start" | "session_end" | "session_progress" | "work_claimed" | "claim_created"
        | "claim_released" | "claim_renewed" => Ok(files::SESSIONS),

        // work-graph.jsonl
        "epic_created"
        | "epic_status_changed"
        | "task_created"
        | "task_status_changed"
        | "task_updated"
        | "task_id_corrected"
        | "task_cancelled"
        | "begin_work"
        | "complete_work"
        | "work_complete"
        | "commit"
        | "pr_created"
        | "pr_merged"
        | "work_finding"
        | "void"
        | "work_cancelled"
        | "stale_work_cleanup" => Ok(files::WORK_GRAPH),

        // memory-events.jsonl
        "memory_store" | "memory_stored" | "milestone" | "progress" | "finding" | "decision"
        | "session_summary" | "memory_event" | "memory_milestone" | "blocker" => {
            Ok(files::MEMORY_EVENTS)
        }

        // config.jsonl
        "config_set" | "config_updated" => Ok(files::CONFIG),

        // pathflow-events.jsonl
        "phase_transition"
        | "stage_transition"
        | "session_register"
        | "session_metadata"
        | "pathflow_task_update" => Ok(files::PATHFLOW_EVENTS),

        _ => Err(LedgerError::UnknownEventType(event_type.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_events_route_to_sessions() {
        for event_type in &[
            "session_start",
            "session_end",
            "session_progress",
            "work_claimed",
            "claim_created",
            "claim_released",
            "claim_renewed",
        ] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::SESSIONS,
                "{event_type} should route to sessions.jsonl"
            );
        }
    }

    #[test]
    fn test_work_graph_events_route_to_work_graph() {
        for event_type in &[
            "epic_created",
            "epic_status_changed",
            "task_created",
            "task_status_changed",
            "task_updated",
            "task_id_corrected",
            "task_cancelled",
            "begin_work",
            "complete_work",
            "work_complete",
            "commit",
            "pr_created",
            "pr_merged",
            "work_finding",
            "void",
            "work_cancelled",
            "stale_work_cleanup",
        ] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::WORK_GRAPH,
                "{event_type} should route to work-graph.jsonl"
            );
        }
    }

    #[test]
    fn test_memory_events_route_to_memory_events() {
        for event_type in &[
            "memory_store",
            "memory_stored",
            "milestone",
            "progress",
            "finding",
            "decision",
            "session_summary",
            "memory_event",
            "memory_milestone",
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
    fn test_pathflow_events_route_to_pathflow() {
        for event_type in &[
            "phase_transition",
            "stage_transition",
            "session_register",
            "session_metadata",
            "pathflow_task_update",
        ] {
            assert_eq!(
                route_event_type(event_type).unwrap(),
                files::PATHFLOW_EVENTS,
                "{event_type} should route to pathflow-events.jsonl"
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
    fn test_all_ledger_event_variants_routable() {
        // All 32 LedgerEvent variant event_type strings must be routable.
        let all_variants = [
            "session_start",
            "session_end",
            "epic_created",
            "epic_status_changed",
            "task_created",
            "task_status_changed",
            "task_updated",
            "task_id_corrected",
            "task_cancelled",
            "begin_work",
            "complete_work",
            "commit",
            "pr_created",
            "work_finding",
            "void",
            "work_cancelled",
            "stale_work_cleanup",
            "memory_store",
            "milestone",
            "progress",
            "finding",
            "decision",
            "session_summary",
            "memory_event",
            "memory_milestone",
            "phase_transition",
            "stage_transition",
            "session_register",
            "session_metadata",
            "pathflow_task_update",
            "config_set",
            "config_updated",
        ];
        for event_type in &all_variants {
            assert!(
                route_event_type(event_type).is_ok(),
                "LedgerEvent variant '{event_type}' must be routable"
            );
        }
    }
}
