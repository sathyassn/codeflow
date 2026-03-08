use serde::{Deserialize, Serialize};

/// Strongly-typed ledger event, discriminated by the `event` JSON field.
///
/// Each variant maps to an `"event"` discriminator in the JSONL ledger files.
/// Event-specific data is captured as `serde_json::Value` since schemas vary
/// per event type and must support legacy/evolving formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent {
    // -- sessions.jsonl --
    SessionStart {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionEnd {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    // -- work-graph.jsonl --
    EpicCreated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    EpicStatusChanged {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    TaskCreated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    TaskStatusChanged {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    TaskUpdated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    TaskIdCorrected {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    TaskCancelled {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    BeginWork {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    CompleteWork {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Commit {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    PrCreated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    WorkFinding {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Void {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    WorkCancelled {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    StaleWorkCleanup {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    // -- memory-events.jsonl --
    MemoryStore {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Milestone {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Progress {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Finding {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    Decision {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionSummary {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    #[serde(alias = "memory_event")]
    MemoryEvent {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    #[serde(alias = "memory_milestone")]
    MemoryMilestone {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    // -- pathflow-events.jsonl --
    PhaseTransition {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    StageTransition {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionRegister {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionMetadata {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    PathflowTaskUpdate {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    // -- config.jsonl --
    ConfigSet {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    ConfigUpdated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
}

impl LedgerEvent {
    /// Return the event discriminator string.
    #[must_use]
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SessionStart { .. } => "session_start",
            Self::SessionEnd { .. } => "session_end",
            Self::EpicCreated { .. } => "epic_created",
            Self::EpicStatusChanged { .. } => "epic_status_changed",
            Self::TaskCreated { .. } => "task_created",
            Self::TaskStatusChanged { .. } => "task_status_changed",
            Self::TaskUpdated { .. } => "task_updated",
            Self::TaskIdCorrected { .. } => "task_id_corrected",
            Self::TaskCancelled { .. } => "task_cancelled",
            Self::BeginWork { .. } => "begin_work",
            Self::CompleteWork { .. } => "complete_work",
            Self::Commit { .. } => "commit",
            Self::PrCreated { .. } => "pr_created",
            Self::WorkFinding { .. } => "work_finding",
            Self::Void { .. } => "void",
            Self::WorkCancelled { .. } => "work_cancelled",
            Self::StaleWorkCleanup { .. } => "stale_work_cleanup",
            Self::MemoryStore { .. } => "memory_store",
            Self::Milestone { .. } => "milestone",
            Self::Progress { .. } => "progress",
            Self::Finding { .. } => "finding",
            Self::Decision { .. } => "decision",
            Self::SessionSummary { .. } => "session_summary",
            Self::MemoryEvent { .. } => "memory_event",
            Self::MemoryMilestone { .. } => "memory_milestone",
            Self::PhaseTransition { .. } => "phase_transition",
            Self::StageTransition { .. } => "stage_transition",
            Self::SessionRegister { .. } => "session_register",
            Self::SessionMetadata { .. } => "session_metadata",
            Self::PathflowTaskUpdate { .. } => "pathflow_task_update",
            Self::ConfigSet { .. } => "config_set",
            Self::ConfigUpdated { .. } => "config_updated",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_session_start() {
        let json = r#"{"event":"session_start","session_id":"ses-123","timestamp":"2026-03-07T00:00:00Z","interaction_mode":"interactive"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "session_start");
        if let LedgerEvent::SessionStart { session_id, .. } = &event {
            assert_eq!(session_id.as_deref(), Some("ses-123"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_deserialize_session_end() {
        let json = r#"{"event":"session_end","session_id":"ses-123","timestamp":"2026-03-07T01:00:00Z","summary":"done"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "session_end");
    }

    #[test]
    fn test_deserialize_task_created() {
        let json = r#"{"event":"task_created","format_id":"INF-TSK-022-006","id":"task-01ABC","status":"todo","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "task_created");
    }

    #[test]
    fn test_deserialize_epic_created() {
        let json = r#"{"event":"epic_created","format_id":"INF-EPC-022","area_type":"INF","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "epic_created");
    }

    #[test]
    fn test_deserialize_begin_work() {
        let json = r#"{"event":"begin_work","work_id":"work-123","task_id":"INF-TSK-001","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "begin_work");
    }

    #[test]
    fn test_deserialize_memory_store() {
        let json = r#"{"event":"memory_store","event_type":"milestone","data":{"content":"test"},"timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "memory_store");
    }

    #[test]
    fn test_deserialize_phase_transition() {
        let json = r#"{"event":"phase_transition","session_id":"ses-1","from":"PF1-INIT","to":"PF2-CONTEXT","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "phase_transition");
    }

    #[test]
    fn test_deserialize_stage_transition() {
        let json = r#"{"event":"stage_transition","session_id":"ses-1","stage":"WS-DEV","status":"complete","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "stage_transition");
    }

    #[test]
    fn test_deserialize_commit() {
        let json = r#"{"event":"commit","hash":"abc123","message":"feat: something","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "commit");
    }

    #[test]
    fn test_deserialize_void() {
        let json = r#"{"event":"void","reason":"test","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "void");
    }

    #[test]
    fn test_serde_roundtrip() {
        let json = r#"{"event":"task_status_changed","format_id":"INF-TSK-001","old_status":"todo","new_status":"in_progress","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&event).unwrap();
        let reparsed: LedgerEvent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(event.event_type(), reparsed.event_type());
    }

    #[test]
    fn test_unknown_event_rejected() {
        let json = r#"{"event":"totally_unknown_event","timestamp":"2026-03-07T00:00:00Z"}"#;
        let result = serde_json::from_str::<LedgerEvent>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_config_set() {
        let json = r#"{"event":"config_set","key":"mode","value":"interactive","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "config_set");
    }

    #[test]
    fn test_deserialize_pr_created() {
        let json = r#"{"event":"pr_created","pr_number":42,"branch":"feat/test","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "pr_created");
    }

    #[test]
    fn test_deserialize_work_finding() {
        let json = r#"{"event":"work_finding","finding":"issue found","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "work_finding");
    }

    #[test]
    fn test_deserialize_session_metadata() {
        let json = r#"{"event":"session_metadata","session_id":"ses-1","key":"work_type","value":"PLAN","timestamp":"2026-03-07T00:00:00Z"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "session_metadata");
    }

    #[test]
    fn test_task_created_json_snapshot() {
        let event = LedgerEvent::TaskCreated {
            timestamp: Some("2026-03-07T12:00:00Z".to_string()),
            data: serde_json::json!({
                "format_id": "INF-TSK-022-007",
                "id": "task-01abc123def456",
                "epic_id": "epic-01xyz789",
                "title": "Set up Rust unit testing infrastructure",
                "status": "todo",
                "area_type": "INF",
                "work_type": "FEAT",
                "priority": "critical"
            }),
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn test_all_event_types_unique() {
        use std::collections::HashSet;
        let types = [
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
        let set: HashSet<_> = types.iter().collect();
        assert_eq!(set.len(), types.len(), "duplicate event type detected");
    }
}
