use serde::{Deserialize, Serialize};

use crate::types::{AreaType, TaskStatus, WorkStage, WorkType};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

// Bool fields mirror the existing Go/SurrealDB schema exactly.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub format_id: String,
    pub epic_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub origin: String,
    pub file_scope: Vec<String>,
    pub scope_policy: String,
    pub scope_root: Option<String>,
    pub estimate: Option<String>,
    pub priority: String,
    pub assignee_id: Option<String>,
    pub autorun_eligible: bool,
    pub auto_commit: bool,
    pub raise_pr: bool,
    pub auto_merge: bool,
    pub target_branch: Option<String>,
    pub acceptance: Vec<String>,
    pub tests: Vec<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub stage: Option<WorkStage>,
    pub stage_status: Option<String>,
    pub stage_history: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: "task-01abc123".to_string(),
            format_id: "INF-TSK-022-019".to_string(),
            epic_id: "epic-01xyz".to_string(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::Todo,
            area_type: AreaType::Inf,
            work_type: WorkType::Test,
            domain: "GENL".to_string(),
            origin: "planned".to_string(),
            file_scope: vec!["codeflow-rs/".to_string()],
            scope_policy: "hard".to_string(),
            scope_root: None,
            estimate: Some("M".to_string()),
            priority: "high".to_string(),
            assignee_id: None,
            autorun_eligible: false,
            auto_commit: false,
            raise_pr: true,
            auto_merge: false,
            target_branch: None,
            acceptance: vec!["All tests pass".to_string()],
            tests: vec!["codeflow-rs/codeflow-core/src/".to_string()],
            branch: None,
            pr_number: None,
            external_id: None,
            external_url: None,
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
            stage: None,
            stage_status: None,
            stage_history: vec![],
        }
    }

    #[test]
    fn test_task_serialize_id_as_plain_string() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "task-01abc123");
    }

    #[test]
    fn test_task_serialize_status() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "todo");
    }

    #[test]
    fn test_task_serialize_work_type() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["work_type"], "TEST");
    }

    #[test]
    fn test_task_serialize_bool_fields() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["autorun_eligible"], false);
        assert_eq!(parsed["raise_pr"], true);
        assert_eq!(parsed["auto_merge"], false);
    }

    #[test]
    fn test_task_serialize_optional_fields_null() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["assignee_id"].is_null());
        assert!(parsed["branch"].is_null());
        assert!(parsed["started_at"].is_null());
        assert!(parsed["completed_at"].is_null());
        assert!(parsed["stage"].is_null());
        assert!(parsed["stage_status"].is_null());
    }

    #[test]
    fn test_task_serialize_stage_history_empty() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["stage_history"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_task_serialize_acceptance_criteria() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let acceptance = parsed["acceptance"].as_array().unwrap();
        assert_eq!(acceptance.len(), 1);
        assert_eq!(acceptance[0], "All tests pass");
    }

    #[test]
    fn test_task_clone() {
        let task = make_task();
        let clone = task.clone();
        assert_eq!(task.id, clone.id);
        assert_eq!(task.format_id, clone.format_id);
        assert_eq!(task.status, clone.status);
    }
}
