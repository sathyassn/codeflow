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
}
