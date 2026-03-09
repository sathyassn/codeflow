// Async fn in traits is a deliberate design choice: DataStore uses static dispatch
// only (no dyn DataStore), so the Send bound concern does not apply.
#![allow(async_fn_in_trait)]

pub mod config;
pub mod error;
pub mod hooks;
pub mod ledger;
pub mod models;
pub mod session;
pub mod store;
pub mod types;

// Re-export commonly used items at crate root.
pub use error::{ConfigError, DbError, HookError, LedgerError, SessionError, WorktreeError};
pub use hooks::{HookEvent, HookHandler, HookInput, HookOutput};
pub use ledger::{Event, JsonlWriter, LedgerWriter};
pub use store::{DataStore, SyncResult};
pub use types::{
    ActiveWorkStatus, AreaType, AutorunSessionStatus, AutorunTaskRunStatus, AutorunWorkerStatus,
    BranchName, DecisionTier, DomainType, EpicId, EpicStatus, FormatId, LedgerEvent,
    ParseEnumError, ParseIdError, ParseSentinelError, Phase, PipelineType, Sentinel, SessionId,
    SessionStatus, TaskId, TaskStatus, WorkId, WorkStage, WorkType,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_enum_variants() {
        let phases = [
            Phase::Pf1Init,
            Phase::Pf2Context,
            Phase::Pf3Classify,
            Phase::Pf4Execute,
            Phase::Pf5Verify,
            Phase::Pf6Complete,
            Phase::Pf7End,
        ];
        assert_eq!(phases.len(), 7);
    }

    #[test]
    fn test_work_stage_enum_variants() {
        let stages = [
            WorkStage::WsDev,
            WorkStage::WsRev,
            WorkStage::WsQa,
            WorkStage::WsTest,
            WorkStage::WsPlan,
            WorkStage::WsDocs,
        ];
        assert_eq!(stages.len(), 6);
    }

    #[test]
    fn test_area_type_enum_variants() {
        let areas = [
            AreaType::Inf,
            AreaType::Pln,
            AreaType::Doc,
            AreaType::Tst,
            AreaType::Sec,
            AreaType::Frm,
            AreaType::Prj,
            AreaType::Aut,
        ];
        assert_eq!(areas.len(), 8);
    }

    #[test]
    fn test_task_status_serde_roundtrip() {
        let status = TaskStatus::InProgress;
        let json = serde_json::to_string(&status).expect("serialize");
        assert_eq!(json, "\"in_progress\"");
        let parsed: TaskStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_task_status_complete_serde() {
        let status = TaskStatus::Complete;
        let json = serde_json::to_string(&status).expect("serialize");
        assert_eq!(json, "\"complete\"");
        let parsed: TaskStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_session_status_serde_roundtrip() {
        let status = SessionStatus::Active;
        let json = serde_json::to_string(&status).expect("serialize");
        assert_eq!(json, "\"active\"");
        let parsed: SessionStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_ledger_file_constants() {
        assert_eq!(ledger::files::WORK_GRAPH, "work-graph.jsonl");
        assert_eq!(ledger::files::SESSIONS, "sessions.jsonl");
        assert_eq!(ledger::files::CANONICAL.len(), 4);
    }

    #[test]
    fn test_hook_event_serde_roundtrip() {
        let event = HookEvent::PreToolUse;
        let json = serde_json::to_string(&event).expect("serialize");
        assert_eq!(json, "\"pre_tool_use\"");
        let parsed: HookEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, event);
    }

    #[test]
    fn test_db_error_display() {
        let err = DbError::NotFound {
            table: "sessions".to_string(),
            id: "ses-123".to_string(),
        };
        assert_eq!(err.to_string(), "record not found: sessions:ses-123");
    }

    #[test]
    fn test_ledger_error_display() {
        let err = LedgerError::UnknownEventType("bad_event".to_string());
        assert_eq!(err.to_string(), "unknown event type: bad_event");
    }

    #[test]
    fn test_hook_error_display() {
        let err = HookError::SentinelNotFound("pf-3".to_string());
        assert_eq!(err.to_string(), "sentinel not found: pf-3");
    }

    #[test]
    fn test_session_error_display() {
        let err = SessionError::NoActiveSession;
        assert_eq!(err.to_string(), "no active session");
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::NotFound("pathflow-config.json".to_string());
        assert_eq!(
            err.to_string(),
            "config file not found: pathflow-config.json"
        );
    }

    #[test]
    fn test_worktree_error_display() {
        let err = WorktreeError::NotFound("/tmp/worktree".to_string());
        assert_eq!(err.to_string(), "worktree not found: /tmp/worktree");
    }

    #[test]
    fn test_ledger_event_serialization() {
        let event = Event {
            event_type: "session_start".to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: Some("ses-001".to_string()),
            data: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"event\":\"session_start\""));
        assert!(json.contains("\"session_id\":\"ses-001\""));
    }

    #[test]
    fn test_hook_output_variants() {
        let allow = HookOutput::Allow;
        assert!(matches!(allow, HookOutput::Allow));

        let block = HookOutput::Block {
            reason: "missing sentinel".to_string(),
            category: Some(hooks::BlockCategory::Gate),
        };
        assert!(matches!(block, HookOutput::Block { .. }));

        let warn = HookOutput::Warn {
            message: "deprecated usage".to_string(),
        };
        assert!(matches!(warn, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_new_types_accessible_from_crate_root() {
        // Verify all new types are accessible from the crate root
        let _sid = SessionId::new_unchecked("ses-1");
        let _tid = TaskId::new_unchecked("task-1");
        let _eid = EpicId::new_unchecked("epic-1");
        let _wid = WorkId::new_unchecked("work-1");
        let _bn = BranchName::new_unchecked("feat/test");
        let _fid = FormatId::new_unchecked("INF-TSK-001");
        let _sentinel = Sentinel::PathflowPf1;
        let _pipeline = PipelineType::DevRevQa;
        let _tier = DecisionTier::Tier1;
        let _domain = DomainType::Genl;
        let _epic = EpicStatus::Draft;
    }

    #[test]
    fn test_phase_transition_validation() {
        assert!(Phase::Pf1Init.can_transition_to(Phase::Pf2Context));
        assert!(!Phase::Pf1Init.can_transition_to(Phase::Pf7End));
    }

    #[test]
    fn test_sentinel_from_phase() {
        let sentinel = Sentinel::from(Phase::Pf3Classify);
        assert_eq!(sentinel.to_string(), "pathflow-pf-3");
    }

    #[test]
    fn test_sentinel_from_stage() {
        let sentinel = Sentinel::from(WorkStage::WsDev);
        assert_eq!(sentinel.to_string(), "pathflow-ws-dev");
    }

    #[test]
    fn test_epic_status_schema_aligned() {
        // All 6 schema values must be parseable
        for val in &[
            "draft",
            "planning",
            "in_progress",
            "blocked",
            "complete",
            "archived",
        ] {
            let parsed: EpicStatus =
                serde_json::from_str(&format!("\"{val}\"")).unwrap_or_else(|_| {
                    panic!("EpicStatus should deserialize from '{val}'");
                });
            let json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, format!("\"{val}\""));
        }
    }

    #[test]
    fn test_task_status_schema_aligned() {
        // All 5 schema values must be parseable
        for val in &["todo", "blocked", "in_progress", "complete", "cancelled"] {
            let parsed: TaskStatus =
                serde_json::from_str(&format!("\"{val}\"")).unwrap_or_else(|_| {
                    panic!("TaskStatus should deserialize from '{val}'");
                });
            let json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, format!("\"{val}\""));
        }
    }
}
