use serde::{Deserialize, Serialize};

use crate::types::{AreaType, EpicStatus, WorkType};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub format_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub is_ongoing: bool,
    pub file_scope: Vec<String>,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_epic() -> Epic {
        Epic {
            id: "epic-01abc123".to_string(),
            format_id: "INF-EPC-022".to_string(),
            title: "Test Epic".to_string(),
            summary: Some("A test epic".to_string()),
            status: EpicStatus::Draft,
            area_type: AreaType::Inf,
            work_type: WorkType::Feat,
            domain: "infrastructure".to_string(),
            is_ongoing: false,
            file_scope: vec!["src/".to_string()],
            priority: "high".to_string(),
            pr_number: None,
            external_id: None,
            external_url: None,
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_epic_serialize_id_as_plain_string() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // serialize_record_id serializes as plain string (no SurrealDB wrapper)
        assert_eq!(parsed["id"], "epic-01abc123");
    }

    #[test]
    fn test_epic_serialize_status() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "draft");
    }

    #[test]
    fn test_epic_serialize_optional_summary() {
        let mut epic = make_epic();
        epic.summary = None;
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["summary"].is_null());
    }

    #[test]
    fn test_epic_serialize_area_type() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["area_type"], "INF");
    }

    #[test]
    fn test_epic_serialize_work_type() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["work_type"], "FEAT");
    }

    #[test]
    fn test_epic_serialize_pr_number_none() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["pr_number"].is_null());
    }

    #[test]
    fn test_epic_serialize_pr_number_some() {
        let mut epic = make_epic();
        epic.pr_number = Some(42);
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["pr_number"], 42);
    }

    #[test]
    fn test_epic_clone() {
        let epic = make_epic();
        let clone = epic.clone();
        assert_eq!(epic.id, clone.id);
        assert_eq!(epic.title, clone.title);
        assert_eq!(epic.status, clone.status);
    }

    #[test]
    fn test_epic_debug_contains_title() {
        let epic = make_epic();
        let debug = format!("{epic:?}");
        assert!(debug.contains("Test Epic"));
    }
}
