//! Task record model.
//!
//! v2 trim: autorun fields (`autorun_eligible`, `auto_commit`, `auto_merge`,
//! `raise_pr`, `target_branch`), coordination fields (`file_scope`,
//! `scope_policy`, `scope_root`, `assignee_id`), pathflow stage fields
//! (`stage`, `stage_status`, `stage_history`), and external task-tracker
//! mirroring fields are removed with their subsystems (charter §3.2, D22).

use serde::{Deserialize, Serialize};

use super::status::TaskStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// ULID-based internal id (e.g., `task-01abc...`).
    pub id: String,
    /// Human-readable format id (e.g., `TSK-001-002`).
    pub format_id: String,
    /// Parent epic's ULID-based id.
    pub epic_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    /// Work intent label from the v2 branch-prefix set (e.g., `fix`).
    pub work_type: String,
    /// Trimmed from the scaffold template; defaults keep records that omit it
    /// parseable (else the store silently skips them and `status` undercounts).
    #[serde(default)]
    pub priority: String,
    pub estimate: Option<String>,
    /// Acceptance criteria — the input-clarity contract (charter §2.1). Lives
    /// in the body checklist now, not the frontmatter, so default when absent.
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// Test references that verify this task.
    #[serde(default)]
    pub tests: Vec<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: "task-01abc123".to_string(),
            format_id: "TSK-001-002".to_string(),
            epic_id: "epic-01xyz".to_string(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::Todo,
            work_type: "test".to_string(),
            priority: "high".to_string(),
            estimate: Some("M".to_string()),
            acceptance: vec!["All tests pass".to_string()],
            tests: vec!["crates/codeflow-core/src/".to_string()],
            branch: None,
            pr_number: None,
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
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
        assert_eq!(parsed["work_type"], "test");
    }

    #[test]
    fn test_task_serialize_optional_fields_null() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["branch"].is_null());
        assert!(parsed["started_at"].is_null());
        assert!(parsed["completed_at"].is_null());
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
    fn test_task_json_roundtrip() {
        let task = make_task();
        let json = serde_json::to_string(&task).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, task.id);
        assert_eq!(back.format_id, task.format_id);
        assert_eq!(back.epic_id, task.epic_id);
        assert_eq!(back.status, task.status);
        assert_eq!(back.acceptance, task.acceptance);
    }

    #[test]
    fn test_task_clone() {
        let task = make_task();
        let clone = task.clone();
        assert_eq!(task.id, clone.id);
        assert_eq!(task.format_id, clone.format_id);
        assert_eq!(task.status, clone.status);
    }

    /// A task authored from the trimmed template omits `priority`, `estimate`,
    /// `acceptance`, and `tests`; it must still parse (via serde defaults) so
    /// the store does not silently skip it and `status` undercount.
    #[test]
    fn test_task_parses_without_trimmed_fields() {
        let yaml = r"
id: task-01abc123
format_id: TSK-001-002
epic_id: epic-01xyz
title: Trimmed task
status: todo
work_type: feat
created_at: 2026-07-05T00:00:00Z
updated_at: 2026-07-05T00:00:00Z
";
        let task: Task = serde_yaml::from_str(yaml).expect("trimmed task must parse");
        assert_eq!(task.format_id, "TSK-001-002");
        assert_eq!(task.priority, "");
        assert_eq!(task.estimate, None);
        assert!(task.acceptance.is_empty());
        assert!(task.tests.is_empty());
    }
}
