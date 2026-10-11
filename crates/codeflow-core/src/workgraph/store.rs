//! Record persistence for the workgraph.
//!
//! v2 replaces the v1 `SurrealDB` `DataStore`. Git-tracked markdown is the
//! shared authority; JSONL is local operational evidence and databases are at
//! most rebuildable caches.
//! `RecordStore` is the minimal epic/task persistence surface the workgraph
//! needs; `MarkdownStore` is the production implementation, persisting each
//! record as a markdown file with YAML frontmatter.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::models::{Epic, EpicFilter, EpicUpdate, Task, TaskFilter, TaskUpdate};

/// Errors from record persistence.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml error in {path}: {message}")]
    Yaml { path: String, message: String },

    #[error("record not found: {0}")]
    NotFound(String),

    #[error("invalid work record: {0}")]
    Invalid(String),

    #[error("{0}")]
    Registry(#[from] crate::ids::IdsError),
}

/// Minimal persistence surface for workgraph records.
///
/// All methods are synchronous — v2 records are local files.
pub trait RecordStore {
    /// Persist a new epic.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on write failure.
    fn create_epic(&self, epic: &Epic) -> Result<(), StoreError>;

    /// Fetch an epic by its stable id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn get_epic(&self, id: &str) -> Result<Option<Epic>, StoreError>;

    /// Fetch an epic by its human-readable format id (e.g. `EPC-001`).
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn get_epic_by_format_id(&self, format_id: &str) -> Result<Option<Epic>, StoreError>;

    /// Apply a partial update to an epic identified by stable id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if the epic does not exist.
    fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), StoreError>;

    /// List epics matching the filter, ordered by format id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, StoreError>;

    /// Persist a new task.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on write failure.
    fn create_task(&self, task: &Task) -> Result<(), StoreError>;

    /// Fetch a task by its stable id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn get_task(&self, id: &str) -> Result<Option<Task>, StoreError>;

    /// Fetch a task by its human-readable format id (e.g. `TSK-001-002`).
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn get_task_by_format_id(&self, format_id: &str) -> Result<Option<Task>, StoreError>;

    /// Apply a partial update to a task identified by stable id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::NotFound` if the task does not exist.
    fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), StoreError>;

    /// List tasks matching the filter, ordered by format id.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` on read failure.
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>, StoreError>;
}

/// Markdown-file-backed `RecordStore` (the v2 production implementation).
///
/// Layout under the store root (typically `project-management/`):
///
/// ```text
/// <root>/
/// ├── epics/EPC-001.md      ← YAML frontmatter = the Epic record
/// ├── specs/SPC-001.md      ← YAML frontmatter = the Spec record
/// └── tasks/TSK-001.md      ← YAML frontmatter = the Task record
/// ```
///
/// The frontmatter is the machine-readable record; the markdown body below
/// it belongs to humans and agents and is preserved verbatim across updates.
pub struct MarkdownStore {
    root: PathBuf,
}

impl MarkdownStore {
    /// Create a store rooted at `root`, creating `epics/`, `specs/`, and `tasks/`
    /// subdirectories if missing. The folders are created beneath the
    /// repository root without following a link (issue 94): a linked
    /// `project-management` or kind folder refuses the store.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` if the directories cannot be created or a
    /// component is a link.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let base = crate::workgraph::allocate::repository_root(&root);
        // Only tests need this branch; production roots are always project-management.
        if base == root.as_path() && !base.exists() {
            fs::create_dir_all(base)?;
        }
        let tree = crate::contained::Tree::open(base)?;
        for kind in ["epics", "specs", "tasks"] {
            tree.create_dir_all(&crate::contained::relative_to(base, &root.join(kind))?)?;
        }
        Ok(Self { root })
    }

    /// The repository root this store's writes are contained beneath, and
    /// `path` relative to it.
    fn contained(&self, path: &Path) -> Result<(crate::contained::Tree, String), StoreError> {
        let base = crate::workgraph::allocate::repository_root(&self.root);
        Ok((
            crate::contained::Tree::open(base)?,
            crate::contained::relative_to(base, path)?,
        ))
    }

    /// The store root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn epics_dir(&self) -> PathBuf {
        self.root.join("epics")
    }

    fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    /// Write a record, replacing any file there, through
    /// [`crate::contained`]: never through a link (issue 94), atomically,
    /// keeping its Unix permission bits. Creation and update share this
    /// writer; the store's create does not promise exclusivity.
    fn write_record<T: serde::Serialize>(
        &self,
        path: &Path,
        record: &T,
        body: &str,
    ) -> Result<(), StoreError> {
        let yaml = serde_yaml::to_string(record).map_err(|e| StoreError::Yaml {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let content = format!("---\n{yaml}---\n{body}");
        let (tree, relative) = self.contained(path)?;
        tree.write(&relative, content.as_bytes())?;
        Ok(())
    }

    /// Persist a canonical record with one on-disk identity. The typed models
    /// retain `format_id` only to read historical dual-identity records.
    fn write_canonical_record<T: serde::Serialize>(
        &self,
        path: &Path,
        record: &T,
        canonical_id: &str,
        body: &str,
    ) -> Result<(), StoreError> {
        let mut value = serde_yaml::to_value(record).map_err(|e| StoreError::Yaml {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let mapping = value.as_mapping_mut().ok_or_else(|| StoreError::Yaml {
            path: path.display().to_string(),
            message: "record did not serialize to a YAML mapping".to_string(),
        })?;
        mapping.remove(serde_yaml::Value::String("format_id".to_string()));
        mapping.insert(
            serde_yaml::Value::String("id".to_string()),
            serde_yaml::Value::String(canonical_id.to_string()),
        );
        self.write_record(path, &value, body)
    }

    /// Update only the named frontmatter fields, preserving every other key.
    ///
    /// Planning records deliberately carry fields owned by documentation and
    /// orchestration rather than the typed workgraph model. Re-serializing the
    /// model would silently discard those fields, so partial record updates
    /// operate on the generic YAML mapping instead.
    fn update_record_fields(
        &self,
        path: &Path,
        body: &str,
        fields: impl IntoIterator<Item = (&'static str, serde_yaml::Value)>,
    ) -> Result<(), StoreError> {
        let (tree, relative) = self.contained(path)?;
        let content = String::from_utf8(tree.read(&relative, u64::MAX)?)
            .map_err(|error| StoreError::Invalid(format!("{relative}: {error}")))?;
        let (yaml, _) = split_frontmatter(&content).ok_or_else(|| StoreError::Yaml {
            path: path.display().to_string(),
            message: "missing frontmatter delimiters".to_string(),
        })?;
        let mut frontmatter =
            serde_yaml::from_str::<serde_yaml::Mapping>(yaml).map_err(|e| StoreError::Yaml {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
        for (field, value) in fields {
            frontmatter.insert(serde_yaml::Value::String(field.to_string()), value);
        }
        self.write_record(path, &frontmatter, body)
    }

    fn yaml_value<T: serde::Serialize>(
        path: &Path,
        value: T,
    ) -> Result<serde_yaml::Value, StoreError> {
        serde_yaml::to_value(value).map_err(|e| StoreError::Yaml {
            path: path.display().to_string(),
            message: e.to_string(),
        })
    }

    fn read_record<T: serde::de::DeserializeOwned>(path: &Path) -> Result<(T, String), StoreError> {
        let content = fs::read_to_string(path)?;
        let (yaml, body) = split_frontmatter(&content).ok_or_else(|| StoreError::Yaml {
            path: path.display().to_string(),
            message: "missing frontmatter delimiters".to_string(),
        })?;
        let record = serde_yaml::from_str(yaml).map_err(|e| StoreError::Yaml {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok((record, body.to_string()))
    }

    fn scan<T: serde::de::DeserializeOwned>(paths: Vec<PathBuf>) -> Vec<(PathBuf, T)> {
        let mut records = Vec::new();
        for path in paths {
            match Self::read_record::<T>(&path) {
                Ok((record, _body)) => records.push((path, record)),
                Err(e) => eprintln!("warn: store: skipping {}: {e}", path.display()),
            }
        }
        records
    }

    fn find_epic(&self, id: &str) -> Option<(PathBuf, Epic, String)> {
        find_by(
            crate::workgraph::layout::epic_record_files(&self.root),
            |e: &Epic| e.id == id,
        )
    }

    fn find_task(&self, id: &str) -> Option<(PathBuf, Task, String)> {
        find_by(
            crate::workgraph::layout::task_record_files(&self.root),
            |t: &Task| t.id == id,
        )
    }
}

/// Scan record `paths` for the first match, returning its path, record, and
/// preserved body.
fn find_by<T: serde::de::DeserializeOwned>(
    paths: Vec<PathBuf>,
    pred: impl Fn(&T) -> bool,
) -> Option<(PathBuf, T, String)> {
    for path in paths {
        match MarkdownStore::read_record::<T>(&path) {
            Ok((record, body)) => {
                if pred(&record) {
                    return Some((path, record, body));
                }
            }
            Err(e) => eprintln!("warn: store: skipping {}: {e}", path.display()),
        }
    }
    None
}

/// Split markdown content into (frontmatter yaml, body).
///
/// The frontmatter must be delimited by `---` lines at the start of the
/// content. (Typed counterpart of `validate::parse_frontmatter`, which
/// parses to a generic map for field-level validation.)
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((yaml, body));
        }
        offset += line.len();
    }
    None
}

impl RecordStore for MarkdownStore {
    fn create_epic(&self, epic: &Epic) -> Result<(), StoreError> {
        let path = self.epics_dir().join(format!("{}.md", epic.format_id));
        self.write_canonical_record(&path, epic, &epic.format_id, "")
    }

    fn get_epic(&self, id: &str) -> Result<Option<Epic>, StoreError> {
        Ok(self.find_epic(id).map(|(_, epic, _)| epic))
    }

    fn get_epic_by_format_id(&self, format_id: &str) -> Result<Option<Epic>, StoreError> {
        Ok(find_by(
            crate::workgraph::layout::epic_record_files(&self.root),
            |e: &Epic| e.format_id == format_id,
        )
        .map(|(_, epic, _)| epic))
    }

    fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), StoreError> {
        let (path, _epic, body) = self
            .find_epic(id)
            .ok_or_else(|| StoreError::NotFound(format!("epic:{id}")))?;
        let mut fields = Vec::new();
        if let Some(status) = update.status {
            fields.push(("status", Self::yaml_value(&path, status)?));
        }
        if let Some(title) = update.title {
            fields.push(("title", Self::yaml_value(&path, title)?));
        }
        if let Some(summary) = update.summary {
            fields.push(("summary", Self::yaml_value(&path, summary)?));
        }
        if let Some(pr_number) = update.pr_number {
            fields.push(("pr_number", Self::yaml_value(&path, pr_number)?));
        }
        fields.push(("updated_at", Self::yaml_value(&path, super::now_rfc3339())?));
        self.update_record_fields(&path, &body, fields)
    }

    fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, StoreError> {
        let records = Self::scan::<Epic>(crate::workgraph::layout::epic_record_files(&self.root));
        Ok(records
            .into_iter()
            .map(|(_, epic)| epic)
            .filter(|e| filter.status.is_none_or(|s| e.status == s))
            .collect())
    }

    fn create_task(&self, task: &Task) -> Result<(), StoreError> {
        if crate::workgraph::is_canonical_task_format_id(&task.format_id) {
            let target = task
                .integration_target
                .as_deref()
                .filter(|target| !target.trim().is_empty())
                .ok_or_else(|| {
                    StoreError::Invalid(format!(
                        "{}: canonical task requires integration_target",
                        task.format_id
                    ))
                })?;
            if !crate::workgraph::is_stable_work_target(target) {
                return Err(StoreError::Invalid(format!(
                    "{}: integration_target '{target}' is not a stable non-task branch name",
                    task.format_id
                )));
            }
        }
        let path = self.tasks_dir().join(format!("{}.md", task.format_id));
        self.write_canonical_record(&path, task, &task.format_id, "")
    }

    fn get_task(&self, id: &str) -> Result<Option<Task>, StoreError> {
        Ok(self.find_task(id).map(|(_, task, _)| task))
    }

    fn get_task_by_format_id(&self, format_id: &str) -> Result<Option<Task>, StoreError> {
        Ok(find_by(
            crate::workgraph::layout::task_record_files(&self.root),
            |t: &Task| t.format_id == format_id,
        )
        .map(|(_, task, _)| task))
    }

    fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), StoreError> {
        let (path, _task, body) = self
            .find_task(id)
            .ok_or_else(|| StoreError::NotFound(format!("task:{id}")))?;
        let mut fields = Vec::new();
        if let Some(status) = update.status {
            fields.push(("status", Self::yaml_value(&path, status)?));
        }
        if let Some(branch) = update.branch {
            fields.push(("branch", Self::yaml_value(&path, branch)?));
        }
        if let Some(pr_number) = update.pr_number {
            fields.push(("pr_number", Self::yaml_value(&path, pr_number)?));
        }
        if let Some(started_at) = update.started_at {
            fields.push(("started_at", Self::yaml_value(&path, started_at)?));
        }
        if let Some(completed_at) = update.completed_at {
            fields.push(("completed_at", Self::yaml_value(&path, completed_at)?));
        }
        fields.push(("updated_at", Self::yaml_value(&path, super::now_rfc3339())?));
        self.update_record_fields(&path, &body, fields)
    }

    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>, StoreError> {
        let records = Self::scan::<Task>(crate::workgraph::layout::task_record_files(&self.root));
        Ok(records
            .into_iter()
            .map(|(_, task)| task)
            .filter(|t| filter.status.is_none_or(|s| t.status == s))
            .filter(|t| {
                filter
                    .epic_id
                    .as_deref()
                    .is_none_or(|eid| t.epic_id.as_deref() == Some(eid))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{EpicStatus, TaskStatus};

    use super::*;

    fn make_epic(_legacy_id: &str, format_id: &str) -> Epic {
        Epic {
            id: format_id.to_string(),
            format_id: format_id.to_string(),
            title: "Test Epic".to_string(),
            summary: Some("A test epic".to_string()),
            status: EpicStatus::Draft,
            work_type: "feat".to_string(),
            priority: "high".to_string(),
            pr_number: None,
            created_at: "2026-06-11T00:00:00Z".to_string(),
            updated_at: "2026-06-11T00:00:00Z".to_string(),
        }
    }

    fn make_task(_legacy_id: &str, format_id: &str, epic_id: &str) -> Task {
        Task {
            id: format_id.to_string(),
            format_id: format_id.to_string(),
            epic_id: Some(epic_id.to_string()),
            standalone_reason: None,
            specs: vec![],
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::Todo,
            work_type: "feat".to_string(),
            priority: "high".to_string(),
            estimate: None,
            acceptance: vec!["it works".to_string()],
            tests: vec![],
            depends_on: vec![],
            integration_target: Some("main".to_string()),
            branch: None,
            pr_number: None,
            created_at: "2026-06-11T00:00:00Z".to_string(),
            updated_at: "2026-06-11T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn test_split_frontmatter_valid() {
        let (yaml, body) = split_frontmatter("---\na: 1\n---\nbody here\n").unwrap();
        assert_eq!(yaml, "a: 1\n");
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn test_split_frontmatter_missing_delimiters() {
        assert!(split_frontmatter("no frontmatter").is_none());
        assert!(split_frontmatter("---\nunclosed: yes\n").is_none());
    }

    #[test]
    fn test_create_and_get_epic() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let epic = make_epic("epic-01a", "EPC-001");
        store.create_epic(&epic).unwrap();

        // File lives at epics/EPC-001.md.
        assert!(dir.path().join("epics/EPC-001.md").exists());

        let fetched = store.get_epic("EPC-001").unwrap().unwrap();
        assert_eq!(fetched.format_id, "EPC-001");
        assert_eq!(fetched.title, "Test Epic");
        assert_eq!(fetched.status, EpicStatus::Draft);
    }

    #[test]
    fn test_get_epic_by_format_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap();

        let fetched = store.get_epic_by_format_id("EPC-001").unwrap().unwrap();
        assert_eq!(fetched.id, "EPC-001");
        assert!(store.get_epic_by_format_id("EPC-999").unwrap().is_none());
    }

    #[test]
    fn test_get_epic_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        assert!(store.get_epic("epic-none").unwrap().is_none());
    }

    #[test]
    fn test_update_epic_fields_and_updated_at() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap();

        store
            .update_epic(
                "EPC-001",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    title: Some("New Title".to_string()),
                    pr_number: Some(7),
                    ..Default::default()
                },
            )
            .unwrap();

        let epic = store.get_epic("EPC-001").unwrap().unwrap();
        assert_eq!(epic.status, EpicStatus::InProgress);
        assert_eq!(epic.title, "New Title");
        assert_eq!(epic.pr_number, Some(7));
        assert_ne!(epic.updated_at, "2026-06-11T00:00:00Z");
    }

    #[test]
    fn test_update_missing_epic_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let err = store
            .update_epic("epic-none", EpicUpdate::default())
            .unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn test_update_preserves_markdown_body() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap();

        // A human (or template) adds body content below the frontmatter.
        let path = dir.path().join("epics/EPC-001.md");
        let content = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            format!("{content}\n## Summary\nHand-written notes.\n"),
        )
        .unwrap();

        store
            .update_epic(
                "EPC-001",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("## Summary"));
        assert!(after.contains("Hand-written notes."));
        assert!(after.contains("status: in_progress"));
    }

    #[test]
    fn test_update_epic_preserves_unmodeled_planning_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let path = dir.path().join("epics/EPC-001.md");
        fs::write(
            &path,
            r"---
id: epic-01a
format_id: EPC-001
title: Test Epic
status: draft
work_type: feat
capabilities: [CAP-001]
adrs: [ADR-0001]
specs: [SPC-001]
custom_review_contract:
  primary: claude
created: 2026-06-11
---
## Summary
Keep this body.
",
        )
        .unwrap();

        store
            .update_epic(
                "epic-01a",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let (yaml, body) = split_frontmatter(&after).unwrap();
        let frontmatter: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(frontmatter["status"], "in_progress");
        assert_eq!(frontmatter["capabilities"][0], "CAP-001");
        assert_eq!(frontmatter["adrs"][0], "ADR-0001");
        assert_eq!(frontmatter["specs"][0], "SPC-001");
        assert_eq!(frontmatter["custom_review_contract"]["primary"], "claude");
        assert_eq!(frontmatter["created"], "2026-06-11");
        assert!(
            frontmatter.get("created_at").is_none(),
            "a partial update must not rewrite the template's field shape"
        );
        assert!(body.contains("Keep this body."));
    }

    #[test]
    fn test_update_task_preserves_graph_and_unmodeled_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let path = dir.path().join("tasks/TSK-001-001.md");
        fs::write(
            &path,
            r"---
id: task-01a
format_id: TSK-001-001
epic_id: epic-01a
title: Test Task
status: todo
work_type: feat
depends_on: [TSK-001-000]
external_context: EXT-42
created: 2026-06-11
---
## Acceptance Criteria
- [ ] Preserve the graph.
",
        )
        .unwrap();

        store
            .update_task(
                "task-01a",
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    branch: Some("feat/test".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let (yaml, body) = split_frontmatter(&after).unwrap();
        let frontmatter: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(frontmatter["status"], "in_progress");
        assert_eq!(frontmatter["branch"], "feat/test");
        assert_eq!(frontmatter["depends_on"][0], "TSK-001-000");
        assert_eq!(frontmatter["external_context"], "EXT-42");
        assert_eq!(frontmatter["created"], "2026-06-11");
        assert!(body.contains("Preserve the graph."));
    }

    #[test]
    fn test_list_epics_filter_by_status() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap();
        let mut second = make_epic("epic-01b", "EPC-002");
        second.status = EpicStatus::InProgress;
        store.create_epic(&second).unwrap();

        let all = store.list_epics(EpicFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        // Ordered by format id.
        assert_eq!(all[0].format_id, "EPC-001");
        assert_eq!(all[1].format_id, "EPC-002");

        let drafts = store
            .list_epics(EpicFilter {
                status: Some(EpicStatus::Draft),
            })
            .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, "EPC-001");
    }

    #[test]
    fn test_create_and_get_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let task = make_task("task-01a", "TSK-001-001", "EPC-001");
        store.create_task(&task).unwrap();

        assert!(dir.path().join("tasks/TSK-001-001.md").exists());

        let fetched = store.get_task("TSK-001-001").unwrap().unwrap();
        assert_eq!(fetched.format_id, "TSK-001-001");
        assert_eq!(fetched.acceptance, vec!["it works".to_string()]);

        let by_fid = store.get_task_by_format_id("TSK-001-001").unwrap().unwrap();
        assert_eq!(by_fid.id, "TSK-001-001");
    }

    #[test]
    fn canonical_task_writer_requires_a_stable_integration_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();

        let mut missing = make_task("task-01a", "TSK-001", "EPC-001");
        missing.integration_target = None;
        assert!(matches!(
            store.create_task(&missing),
            Err(StoreError::Invalid(message)) if message.contains("requires integration_target")
        ));

        let mut unstable = make_task("task-01a", "TSK-001", "EPC-001");
        unstable.integration_target = Some("task/TSK-001-self".to_string());
        assert!(matches!(
            store.create_task(&unstable),
            Err(StoreError::Invalid(message)) if message.contains("not a stable")
        ));

        let valid = make_task("task-01a", "TSK-001", "EPC-001");
        store.create_task(&valid).unwrap();
        let content = fs::read_to_string(dir.path().join("tasks/TSK-001.md")).unwrap();
        assert!(content.contains("integration_target: main"));
    }

    #[test]
    fn test_update_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_task(&make_task("task-01a", "TSK-001-001", "EPC-001"))
            .unwrap();

        store
            .update_task(
                "TSK-001-001",
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    branch: Some("feat/test-branch".to_string()),
                    started_at: Some("2026-06-11T01:00:00Z".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let task = store.get_task("TSK-001-001").unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.branch.as_deref(), Some("feat/test-branch"));
        assert_eq!(task.started_at.as_deref(), Some("2026-06-11T01:00:00Z"));
    }

    #[test]
    fn test_list_tasks_filters() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_task(&make_task("task-01a", "TSK-001-001", "EPC-001"))
            .unwrap();
        let mut other = make_task("task-01b", "TSK-002-001", "EPC-002");
        other.status = TaskStatus::InProgress;
        store.create_task(&other).unwrap();

        let all = store.list_tasks(TaskFilter::default()).unwrap();
        assert_eq!(all.len(), 2);

        let by_epic = store
            .list_tasks(TaskFilter {
                epic_id: Some("EPC-001".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_epic.len(), 1);
        assert_eq!(by_epic[0].id, "TSK-001-001");

        let in_progress = store
            .list_tasks(TaskFilter {
                status: Some(TaskStatus::InProgress),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(in_progress.len(), 1);
        assert_eq!(in_progress[0].id, "TSK-002-001");
    }

    /// Issue 94: a linked `project-management` or kind folder refuses the
    /// store, and its writes never land where a link points.
    #[cfg(unix)]
    #[test]
    fn markdown_store_refuses_a_linked_project_management() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let pm = dir.path().join("project-management");
        std::os::unix::fs::symlink(outside.path(), &pm).unwrap();
        let error = MarkdownStore::new(&pm).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("project-management is a symbolic link"),
            "{error}"
        );
        assert_eq!(
            fs::read_dir(outside.path()).unwrap().count(),
            0,
            "no folder outside"
        );

        fs::remove_file(&pm).unwrap();
        let store = MarkdownStore::new(&pm).unwrap();
        fs::remove_dir(pm.join("epics")).unwrap();
        std::os::unix::fs::symlink(outside.path(), pm.join("epics")).unwrap();
        assert!(store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .is_err());
        assert_eq!(
            fs::read_dir(outside.path()).unwrap().count(),
            0,
            "no record outside"
        );
    }

    #[test]
    fn test_scan_finds_nested_epic_layout() {
        // Dogfood layout: epics/EPC-001/EPC-001.md (dir per epic so tasks/ and
        // specs live alongside). Discovery must find the record inside the dir.
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let nested_dir = store.root().join("epics/EPC-001");
        fs::create_dir_all(&nested_dir).unwrap();
        store
            .write_record(
                &nested_dir.join("EPC-001.md"),
                &make_epic("epic-01a", "EPC-001"),
                "## body kept\n",
            )
            .unwrap();
        // Sibling files inside the epic dir must NOT be treated as epics.
        fs::write(nested_dir.join("spec.md"), "---\nid: x\n---\n").unwrap();
        fs::create_dir_all(nested_dir.join("tasks")).unwrap();

        let epics = store.list_epics(EpicFilter::default()).unwrap();
        assert_eq!(epics.len(), 1, "nested epic must be discovered");
        assert_eq!(epics[0].format_id, "EPC-001");

        // find_by paths (get/update) must resolve the nested record too.
        let by_fid = store.get_epic_by_format_id("EPC-001").unwrap().unwrap();
        assert_eq!(by_fid.id, "EPC-001");
    }

    #[test]
    fn test_scan_finds_both_flat_and_nested_epics() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap(); // flat
        let nested_dir = store.root().join("epics/EPC-002");
        fs::create_dir_all(&nested_dir).unwrap();
        store
            .write_record(
                &nested_dir.join("EPC-002.md"),
                &make_epic("epic-01b", "EPC-002"),
                "",
            )
            .unwrap();

        let epics = store.list_epics(EpicFilter::default()).unwrap();
        assert_eq!(epics.len(), 2, "both layouts must coexist");
        assert_eq!(epics[0].format_id, "EPC-001");
        assert_eq!(epics[1].format_id, "EPC-002");
    }

    #[test]
    fn test_scan_finds_and_updates_legacy_nested_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let nested = store.root().join("epics/EPC-001/tasks/TSK-001-001.md");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        store
            .write_record(
                &nested,
                &make_task("task-01a", "TSK-001-001", "EPC-001"),
                "## keep\n",
            )
            .unwrap();

        assert_eq!(store.list_tasks(TaskFilter::default()).unwrap().len(), 1);
        store
            .update_task(
                "TSK-001-001",
                TaskUpdate {
                    status: Some(TaskStatus::Complete),
                    ..Default::default()
                },
            )
            .unwrap();

        let after = fs::read_to_string(&nested).unwrap();
        assert!(after.contains("status: complete"));
        assert!(after.contains("## keep"));
        assert!(!store.root().join("tasks/TSK-001-001.md").exists());
    }

    #[test]
    fn test_update_nested_epic_rewrites_in_place() {
        // An update to a nested epic must rewrite the nested file, not create a
        // stray flat one — find_by returns the real path.
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let nested = store.root().join("epics/EPC-001/EPC-001.md");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        store
            .write_record(&nested, &make_epic("epic-01a", "EPC-001"), "## keep\n")
            .unwrap();

        store
            .update_epic(
                "EPC-001",
                EpicUpdate {
                    status: Some(EpicStatus::Complete),
                    ..Default::default()
                },
            )
            .unwrap();

        assert!(
            nested.is_file(),
            "nested file must remain the record location"
        );
        assert!(
            !store.root().join("epics/EPC-001.md").exists(),
            "no stray flat file should be created"
        );
        let after = fs::read_to_string(&nested).unwrap();
        assert!(after.contains("status: complete"));
        assert!(after.contains("## keep"), "body preserved");
    }

    #[test]
    fn test_scan_skips_unparseable_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_epic(&make_epic("epic-01a", "EPC-001"))
            .unwrap();
        fs::write(
            dir.path().join("epics/notes.md"),
            "just notes, no frontmatter",
        )
        .unwrap();

        let all = store.list_epics(EpicFilter::default()).unwrap();
        assert_eq!(all.len(), 1, "unparseable file must be skipped, not fatal");
    }
}
