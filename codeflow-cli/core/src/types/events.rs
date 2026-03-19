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
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    StageTransition {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionRegister {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionMetadata {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    PathflowTaskUpdate {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
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

    #[test]
    fn test_memory_event_alias() {
        // "memory_event" alias deserializes to MemoryEvent variant
        let json = r#"{"event":"memory_event","timestamp":"2026-03-07T00:00:00Z","key":"value"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "memory_event");
    }

    #[test]
    fn test_memory_milestone_alias() {
        let json =
            r#"{"event":"memory_milestone","timestamp":"2026-03-07T00:00:00Z","key":"value"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "memory_milestone");
    }

    fn assert_variants_roundtrip(variants: &[(&str, serde_json::Value)]) {
        for (event_type, extra_fields) in variants {
            let mut obj = serde_json::json!({"event": event_type});
            if let Some(extra) = extra_fields.as_object() {
                obj.as_object_mut().unwrap().extend(extra.clone());
            }
            let json = serde_json::to_string(&obj).unwrap();
            let event: LedgerEvent = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("failed to deserialize {event_type}: {e}"));
            assert_eq!(
                event.event_type(),
                *event_type,
                "event_type() mismatch for {event_type}"
            );
            let re_json = serde_json::to_string(&event).unwrap();
            let re_event: LedgerEvent = serde_json::from_str(&re_json)
                .unwrap_or_else(|e| panic!("roundtrip failed for {event_type}: {e}"));
            assert_eq!(event.event_type(), re_event.event_type());
        }
    }

    #[test]
    fn test_session_workgraph_variants_serde_roundtrip() {
        // Session, epic, task, and work-graph variants
        let ts = "2026-03-07T00:00:00Z";
        let with_sid = serde_json::json!({"session_id": "ses-1", "timestamp": ts});
        let no_sid = serde_json::json!({"timestamp": ts});
        assert_variants_roundtrip(&[
            ("session_start", with_sid.clone()),
            ("session_end", with_sid.clone()),
            ("epic_created", no_sid.clone()),
            ("epic_status_changed", no_sid.clone()),
            ("task_created", no_sid.clone()),
            ("task_status_changed", no_sid.clone()),
            ("task_updated", no_sid.clone()),
            ("task_id_corrected", no_sid.clone()),
            ("task_cancelled", no_sid.clone()),
            ("begin_work", with_sid.clone()),
            ("complete_work", no_sid.clone()),
            ("commit", no_sid.clone()),
            ("pr_created", no_sid.clone()),
            ("work_finding", no_sid.clone()),
            ("void", no_sid.clone()),
            ("work_cancelled", no_sid.clone()),
            ("stale_work_cleanup", no_sid.clone()),
        ]);
    }

    #[test]
    fn test_memory_pathflow_config_variants_serde_roundtrip() {
        // Memory, pathflow, and config variants
        let ts = "2026-03-07T00:00:00Z";
        let with_sid = serde_json::json!({"session_id": "ses-1", "timestamp": ts});
        let no_sid = serde_json::json!({"timestamp": ts});
        assert_variants_roundtrip(&[
            ("memory_store", no_sid.clone()),
            ("milestone", no_sid.clone()),
            ("progress", no_sid.clone()),
            ("finding", no_sid.clone()),
            ("decision", no_sid.clone()),
            ("session_summary", no_sid.clone()),
            ("memory_event", no_sid.clone()),
            ("memory_milestone", no_sid.clone()),
            ("phase_transition", with_sid.clone()),
            ("stage_transition", with_sid.clone()),
            ("session_register", with_sid.clone()),
            ("session_metadata", with_sid.clone()),
            ("pathflow_task_update", with_sid.clone()),
            ("config_set", no_sid.clone()),
            ("config_updated", no_sid.clone()),
        ]);
    }

    #[test]
    fn test_event_type_no_session_id_for_non_session_variants() {
        // Variants without session_id should serialize without the field
        let event = LedgerEvent::EpicCreated {
            timestamp: Some("2026-03-07T00:00:00Z".to_string()),
            data: serde_json::json!({}),
        };
        let json = serde_json::to_string(&event).unwrap();
        // EpicCreated has no session_id field in its variant
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["event"], "epic_created");
        // session_id should be absent (not null) since it's not a field of EpicCreated
        assert!(parsed.get("session_id").is_none() || parsed["session_id"].is_null());
    }

    #[test]
    fn test_ledger_event_snapshot_session_start() {
        let event = LedgerEvent::SessionStart {
            session_id: Some("ses-177137202131769e89b2d5688".to_string()),
            timestamp: Some("2026-03-07T12:00:00Z".to_string()),
            data: serde_json::json!({
                "interaction_mode": "interactive",
                "source": "startup"
            }),
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn test_ledger_event_snapshot_work_graph() {
        let event = LedgerEvent::TaskStatusChanged {
            timestamp: Some("2026-03-07T12:00:00Z".to_string()),
            data: serde_json::json!({
                "format_id": "INF-TSK-022-019",
                "old_status": "todo",
                "new_status": "in_progress"
            }),
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn test_ledger_event_phase_transition_with_worktree() {
        let json = r#"{"event":"phase_transition","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z","worktree":"/tmp/wt/abc","phase":"PF1-INIT","status":"entered"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "phase_transition");
        if let LedgerEvent::PhaseTransition {
            session_id,
            worktree,
            ..
        } = &event
        {
            assert_eq!(session_id.as_deref(), Some("ses-1"));
            assert_eq!(worktree.as_deref(), Some("/tmp/wt/abc"));
        } else {
            panic!("wrong variant");
        }

        // Roundtrip preserves worktree
        let serialized = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed["worktree"], "/tmp/wt/abc");
    }

    #[test]
    fn test_ledger_event_backward_compat_no_worktree() {
        // Old events without worktree field should deserialize with worktree = None
        let json = r#"{"event":"phase_transition","session_id":"ses-old","timestamp":"2026-03-07T00:00:00Z","phase":"PF2-CONTEXT"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type(), "phase_transition");
        if let LedgerEvent::PhaseTransition { worktree, .. } = &event {
            assert!(worktree.is_none());
        } else {
            panic!("wrong variant");
        }

        // worktree should be omitted when None (skip_serializing_if)
        let serialized = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert!(
            parsed.get("worktree").is_none(),
            "worktree should be absent when None, but got: {serialized}",
        );
    }

    #[test]
    fn test_ledger_event_stage_transition_with_worktree() {
        let json = r#"{"event":"stage_transition","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z","worktree":"/tmp/wt/xyz","stage":"WS-DEV"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        if let LedgerEvent::StageTransition { worktree, .. } = &event {
            assert_eq!(worktree.as_deref(), Some("/tmp/wt/xyz"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_ledger_event_session_register_with_worktree() {
        let json = r#"{"event":"session_register","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z","worktree":"/tmp/wt/reg"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        if let LedgerEvent::SessionRegister { worktree, .. } = &event {
            assert_eq!(worktree.as_deref(), Some("/tmp/wt/reg"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_ledger_event_pathflow_task_update_with_worktree() {
        let json = r#"{"event":"pathflow_task_update","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z","worktree":"/tmp/wt/task"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        if let LedgerEvent::PathflowTaskUpdate { worktree, .. } = &event {
            assert_eq!(worktree.as_deref(), Some("/tmp/wt/task"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_ledger_event_session_metadata_with_worktree() {
        let json = r#"{"event":"session_metadata","session_id":"ses-1","timestamp":"2026-03-07T00:00:00Z","worktree":"/tmp/wt/meta"}"#;
        let event: LedgerEvent = serde_json::from_str(json).unwrap();
        if let LedgerEvent::SessionMetadata { worktree, .. } = &event {
            assert_eq!(worktree.as_deref(), Some("/tmp/wt/meta"));
        } else {
            panic!("wrong variant");
        }
    }

    mod proptests {
        use proptest::prelude::*;

        use super::LedgerEvent;

        fn arb_optional_string() -> impl Strategy<Value = Option<String>> {
            prop_oneof![Just(None), any::<String>().prop_map(Some)]
        }

        fn arb_ledger_event() -> impl Strategy<Value = LedgerEvent> {
            prop_oneof![
                (arb_optional_string(), arb_optional_string()).prop_map(|(sid, ts)| {
                    LedgerEvent::SessionStart {
                        session_id: sid,
                        timestamp: ts,
                        data: serde_json::json!({"key": "value"}),
                    }
                }),
                arb_optional_string().prop_map(|ts| LedgerEvent::TaskCreated {
                    timestamp: ts,
                    data: serde_json::json!({"format_id": "INF-TSK-022-019"}),
                }),
                arb_optional_string().prop_map(|ts| LedgerEvent::EpicCreated {
                    timestamp: ts,
                    data: serde_json::json!({"area_type": "INF"}),
                }),
                arb_optional_string().prop_map(|ts| LedgerEvent::TaskStatusChanged {
                    timestamp: ts,
                    data: serde_json::json!({"old": "todo", "new": "in_progress"}),
                }),
                (arb_optional_string(), arb_optional_string()).prop_map(|(sid, ts)| {
                    LedgerEvent::PhaseTransition {
                        session_id: sid,
                        timestamp: ts,
                        worktree: None,
                        data: serde_json::json!({"from": "PF1-INIT", "to": "PF2-CONTEXT"}),
                    }
                }),
                arb_optional_string().prop_map(|ts| LedgerEvent::ConfigSet {
                    timestamp: ts,
                    data: serde_json::json!({"key": "mode", "value": "interactive"}),
                }),
            ]
        }

        proptest! {
            #[test]
            fn ledger_event_serde_roundtrip(event in arb_ledger_event()) {
                let json = serde_json::to_string(&event).unwrap();
                let reparsed: LedgerEvent = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(event.event_type(), reparsed.event_type());
            }
        }
    }
}
