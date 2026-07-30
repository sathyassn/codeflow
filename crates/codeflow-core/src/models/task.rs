//! Task record model.
//!
//! v2 trim: autorun fields (`autorun_eligible`, `auto_commit`, `auto_merge`,
//! `raise_pr`, `target_branch`), coordination fields (`file_scope`,
//! `scope_policy`, `scope_root`, `assignee_id`), pathflow stage fields
//! (`stage`, `stage_status`, `stage_history`), and external task-tracker
//! mirroring fields are removed with their subsystems (charter §3.2, D22).

use serde::{Deserialize, Deserializer, Serialize};

use super::status::TaskStatus;

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    /// Stable record id. Canonical records use `TSK-NNN`.
    pub id: String,
    /// Compatibility alias for historical `id` + `format_id` records.
    ///
    /// New records set this to the same value as `id` and never persist the
    /// duplicate field.
    pub format_id: String,
    /// Parent epic id, or `None` for an explicitly justified standalone task.
    pub epic_id: Option<String>,
    /// Required when `epic_id` is absent.
    pub standalone_reason: Option<String>,
    /// Specifications consumed directly by this task. Epic-linked tasks also
    /// inherit specifications referenced by their epic.
    pub specs: Vec<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    /// Work intent label from the v2 branch-prefix set (e.g., `fix`).
    pub work_type: String,
    /// Trimmed from the scaffold template; defaults keep records that omit it
    /// parseable (else the store silently skips them and `status` undercounts).
    pub priority: String,
    pub estimate: Option<String>,
    /// Acceptance criteria — the input-clarity contract (charter §2.1). Lives
    /// in the body checklist now, not the frontmatter, so default when absent.
    pub acceptance: Vec<String>,
    /// Test references that verify this task.
    pub tests: Vec<String>,
    /// Direct predecessor task format ids from the settled task graph.
    ///
    /// New records use `depends_on`; deserialization accepts the historical
    /// `dependencies` spelling as an alias.
    pub depends_on: Vec<String>,
    /// Stable branch into which this task's planning record must be merged
    /// before implementation starts. Required for canonical `TSK-NNN`
    /// records; optional only while reading historical task records.
    pub integration_target: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Deserialization tolerates the canonical hand-authored planning frontmatter
/// (the pm-template/validator shape), mirroring `Epic`: `created` is accepted as
/// an alias of `created_at`, a missing `updated_at` falls back to the creation
/// timestamp, and template-trimmed fields default — otherwise a task scaffolded
/// from the template (or `codeflow task new`) fails to parse and the store
/// silently skips it, undercounting `codeflow status`. Partial store updates
/// operate on the generic YAML mapping so fields outside this typed view
/// remain intact.
impl<'de> Deserialize<'de> for Task {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            #[serde(default)]
            format_id: Option<String>,
            #[serde(default)]
            epic_id: Option<String>,
            #[serde(default)]
            standalone_reason: Option<String>,
            #[serde(default)]
            specs: Vec<String>,
            title: String,
            #[serde(default)]
            description: Option<String>,
            status: TaskStatus,
            work_type: String,
            #[serde(default)]
            priority: String,
            #[serde(default)]
            estimate: Option<String>,
            #[serde(default)]
            acceptance: Vec<String>,
            #[serde(default)]
            tests: Vec<String>,
            #[serde(default, alias = "dependencies")]
            depends_on: Vec<String>,
            #[serde(default)]
            integration_target: Option<String>,
            #[serde(default)]
            branch: Option<String>,
            #[serde(default)]
            pr_number: Option<i64>,
            #[serde(alias = "created")]
            created_at: String,
            #[serde(default)]
            updated_at: Option<String>,
            #[serde(default)]
            started_at: Option<String>,
            #[serde(default)]
            completed_at: Option<String>,
        }

        let w = Wire::deserialize(deserializer)?;
        let updated_at = w.updated_at.unwrap_or_else(|| w.created_at.clone());
        let format_id = w.format_id.unwrap_or_else(|| w.id.clone());
        Ok(Task {
            id: w.id,
            format_id,
            epic_id: w.epic_id,
            standalone_reason: w.standalone_reason,
            specs: w.specs,
            title: w.title,
            description: w.description,
            status: w.status,
            work_type: w.work_type,
            priority: w.priority,
            estimate: w.estimate,
            acceptance: w.acceptance,
            tests: w.tests,
            depends_on: w.depends_on,
            integration_target: w.integration_target,
            branch: w.branch,
            pr_number: w.pr_number,
            created_at: w.created_at,
            updated_at,
            started_at: w.started_at,
            completed_at: w.completed_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_parses_template_shape_with_created_alias() {
        // A task authored from task.md.tmpl uses `created:` (not created_at),
        // omits updated_at, and omits the trimmed fields. It must still parse —
        // else the store skips it and `codeflow status` undercounts. updated_at
        // falls back to created_at, mirroring Epic.
        let yaml = r"
id: task-01abc
format_id: TSK-001-002
epic_id: epic-01abc
title: Do the thing
status: todo
work_type: fix
created: 2026-07-05T00:00:00Z
";
        let task: Task = serde_yaml::from_str(yaml).expect("template-shape task must parse");
        assert_eq!(task.created_at, "2026-07-05T00:00:00Z");
        assert_eq!(
            task.updated_at, "2026-07-05T00:00:00Z",
            "updated_at falls back to created_at"
        );
        assert_eq!(task.priority, "");
        assert!(task.acceptance.is_empty());
        assert!(task.depends_on.is_empty());
    }

    fn make_task() -> Task {
        Task {
            id: "task-01abc123".to_string(),
            format_id: "TSK-001-002".to_string(),
            epic_id: Some("epic-01xyz".to_string()),
            standalone_reason: None,
            specs: Vec::new(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::Todo,
            work_type: "test".to_string(),
            priority: "high".to_string(),
            estimate: Some("M".to_string()),
            acceptance: vec!["All tests pass".to_string()],
            tests: vec!["crates/codeflow-core/src/".to_string()],
            depends_on: vec!["TSK-001-001".to_string()],
            integration_target: Some("main".to_string()),
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
        assert_eq!(back.depends_on, task.depends_on);
    }

    #[test]
    fn test_task_accepts_legacy_dependencies_alias() {
        let yaml = r"
id: task-01abc
format_id: TSK-001-002
epic_id: epic-01abc
title: Do the thing
status: todo
work_type: fix
dependencies: [TSK-001-001]
created: 2026-07-05T00:00:00Z
";
        let task: Task = serde_yaml::from_str(yaml).expect("legacy task must parse");
        assert_eq!(task.depends_on, ["TSK-001-001"]);
    }

    #[test]
    fn test_task_rejects_both_dependency_spellings() {
        let yaml = r"
id: task-01abc
format_id: TSK-001-002
epic_id: epic-01abc
title: Do the thing
status: todo
work_type: fix
depends_on: [TSK-001-001]
dependencies: [TSK-001-000]
created: 2026-07-05T00:00:00Z
";
        let error = serde_yaml::from_str::<Task>(yaml).expect_err("aliases must not conflict");
        assert!(error.to_string().contains("duplicate field"));
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
