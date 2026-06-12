//! Epic record model.
//!
//! v2 trim: dual-purpose v1 fields tied to dead subsystems (area types,
//! pathflow domains, file-scope claims, external task-tracker mirroring)
//! are removed. Markdown + JSONL are the source of truth (charter D17).

use serde::{Deserialize, Serialize};

use super::status::EpicStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    /// ULID-based internal id (e.g., `epic-01abc...`).
    pub id: String,
    /// Human-readable format id (e.g., `EPC-001`).
    pub format_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    /// Work intent label from the v2 branch-prefix set (e.g., `feat`).
    pub work_type: String,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_epic() -> Epic {
        Epic {
            id: "epic-01abc123".to_string(),
            format_id: "EPC-001".to_string(),
            title: "Test Epic".to_string(),
            summary: Some("A test epic".to_string()),
            status: EpicStatus::Draft,
            work_type: "feat".to_string(),
            priority: "high".to_string(),
            pr_number: None,
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_epic_serialize_id_as_plain_string() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
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
    fn test_epic_json_roundtrip() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let back: Epic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, epic.id);
        assert_eq!(back.format_id, epic.format_id);
        assert_eq!(back.status, epic.status);
        assert_eq!(back.work_type, epic.work_type);
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
