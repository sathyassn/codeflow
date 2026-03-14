use serde::{Deserialize, Serialize};

use crate::types::SessionStatus;

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub project_id: Option<String>,
    pub user_id: String,
    pub user_host: String,
    pub machine_fingerprint: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub status: SessionStatus,
    pub work_ids: Vec<String>,
    pub previous_session_id: Option<String>,
    pub context_summary: Option<String>,
    #[serde(default)]
    pub tool_stats: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> Session {
        Session {
            id: "ses-177137202131769e89b2d5688".to_string(),
            project_id: None,
            user_id: "user-123".to_string(),
            user_host: "dev-machine".to_string(),
            machine_fingerprint: Some("fp-abc123".to_string()),
            started_at: "2026-03-07T00:00:00Z".to_string(),
            ended_at: None,
            duration_seconds: None,
            status: SessionStatus::Active,
            work_ids: vec![],
            previous_session_id: None,
            context_summary: None,
            tool_stats: serde_json::json!({}),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_session_serialize_id() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "ses-177137202131769e89b2d5688");
    }

    #[test]
    fn test_session_serialize_status() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "active");
    }

    #[test]
    fn test_session_serialize_optional_nulls() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["ended_at"].is_null());
        assert!(parsed["duration_seconds"].is_null());
        assert!(parsed["context_summary"].is_null());
        assert!(parsed["previous_session_id"].is_null());
    }

    #[test]
    fn test_session_serialize_with_ended_at() {
        let mut session = make_session();
        session.ended_at = Some("2026-03-07T01:00:00Z".to_string());
        session.duration_seconds = Some(3600);
        session.status = SessionStatus::Ended;
        let json = serde_json::to_string(&session).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["ended_at"], "2026-03-07T01:00:00Z");
        assert_eq!(parsed["duration_seconds"], 3600);
        assert_eq!(parsed["status"], "ended");
    }

    #[test]
    fn test_session_serialize_tool_stats_default() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["tool_stats"].is_object());
    }

    #[test]
    fn test_session_clone() {
        let session = make_session();
        let clone = session.clone();
        assert_eq!(session.id, clone.id);
        assert_eq!(session.status, clone.status);
    }
}
