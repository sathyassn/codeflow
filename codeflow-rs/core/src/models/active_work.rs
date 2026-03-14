use serde::{Deserialize, Serialize};

use crate::types::{ActiveWorkStatus, WorkStage};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub task_id: Option<String>,
    pub topic: String,
    pub status: ActiveWorkStatus,
    pub branch: Option<String>,
    pub scope: Vec<String>,
    pub deliverables: Vec<String>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub current_stage: Option<WorkStage>,
    pub team_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_active_work() -> ActiveWork {
        ActiveWork {
            id: "work-01abc".to_string(),
            task_id: Some("INF-TSK-022-019".to_string()),
            topic: "Rust test hardening".to_string(),
            status: ActiveWorkStatus::InProgress,
            branch: Some("test/rust-hardening".to_string()),
            scope: vec!["codeflow-rs/core/src/".to_string()],
            deliverables: vec!["tests".to_string()],
            agent: Some("cf-quality-assurance".to_string()),
            session_id: Some("ses-123".to_string()),
            current_stage: Some(WorkStage::WsTest),
            team_name: Some("codeflow-team".to_string()),
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_active_work_serialize_id() {
        let aw = make_active_work();
        let json = serde_json::to_string(&aw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "work-01abc");
    }

    #[test]
    fn test_active_work_serialize_status() {
        let aw = make_active_work();
        let json = serde_json::to_string(&aw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // ActiveWorkStatus::InProgress should serialize correctly
        assert!(!parsed["status"].is_null());
    }

    #[test]
    fn test_active_work_serialize_optional_fields() {
        let mut aw = make_active_work();
        aw.task_id = None;
        aw.branch = None;
        aw.agent = None;
        aw.session_id = None;
        aw.current_stage = None;
        aw.team_name = None;
        let json = serde_json::to_string(&aw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["task_id"].is_null());
        assert!(parsed["branch"].is_null());
        assert!(parsed["agent"].is_null());
        assert!(parsed["session_id"].is_null());
        assert!(parsed["current_stage"].is_null());
        assert!(parsed["team_name"].is_null());
    }

    #[test]
    fn test_active_work_serialize_scope_and_deliverables() {
        let aw = make_active_work();
        let json = serde_json::to_string(&aw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let scope = parsed["scope"].as_array().unwrap();
        assert_eq!(scope.len(), 1);
        let deliverables = parsed["deliverables"].as_array().unwrap();
        assert_eq!(deliverables.len(), 1);
    }

    #[test]
    fn test_active_work_clone() {
        let aw = make_active_work();
        let clone = aw.clone();
        assert_eq!(aw.id, clone.id);
        assert_eq!(aw.topic, clone.topic);
    }
}
