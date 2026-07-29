//! Record persistence for the workgraph.
//!
//! v2 replaces the v1 `SurrealDB` `DataStore` (charter D17: markdown + JSONL
//! are the source of truth; databases are at most rebuildable caches).
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

    /// Fetch an epic by its ULID-based id.
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

    /// Apply a partial update to an epic identified by ULID-based id.
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

    /// Fetch a task by its ULID-based id.
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

    /// Apply a partial update to a task identified by ULID-based id.
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
/// └── tasks/TSK-001-001.md  ← YAML frontmatter = the Task record
/// ```
///
/// The frontmatter is the machine-readable record; the markdown body below
/// it belongs to humans and agents and is preserved verbatim across updates.
pub struct MarkdownStore {
    root: PathBuf,
}

impl MarkdownStore {
    /// Create a store rooted at `root`, creating `epics/` and `tasks/`
    /// subdirectories if missing.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::Io` if the directories cannot be created.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(root.join("epics"))?;
        fs::create_dir_all(root.join("tasks"))?;
        Ok(Self { root })
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

    fn write_record<T: serde::Serialize>(
        path: &Path,
        record: &T,
        body: &str,
    ) -> Result<(), StoreError> {
        let yaml = serde_yaml::to_string(record).map_err(|e| StoreError::Yaml {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let content = format!("---\n{yaml}---\n{body}");
        fs::write(path, content)?;
        Ok(())
    }

    /// Update only the named frontmatter fields, preserving every other key.
    ///
    /// Planning records deliberately carry fields owned by documentation and
    /// orchestration rather than the typed workgraph model. Re-serializing the
    /// model would silently discard those fields, so partial record updates
    /// operate on the generic YAML mapping instead.
    fn update_record_fields(
        path: &Path,
        body: &str,
        fields: impl IntoIterator<Item = (&'static str, serde_yaml::Value)>,
    ) -> Result<(), StoreError> {
        let content = fs::read_to_string(path)?;
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
        Self::write_record(path, &frontmatter, body)
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

    fn scan<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<(PathBuf, T)>, StoreError> {
        let mut records = Vec::new();
        if !dir.is_dir() {
            return Ok(records);
        }
        for path in record_files(dir)? {
            match Self::read_record::<T>(&path) {
                Ok((record, _body)) => records.push((path, record)),
                Err(e) => eprintln!("warn: store: skipping {}: {e}", path.display()),
            }
        }
        Ok(records)
    }

    fn find_epic(&self, id: &str) -> Result<Option<(PathBuf, Epic, String)>, StoreError> {
        find_by(&self.epics_dir(), |e: &Epic| e.id == id)
    }

    fn find_task(&self, id: &str) -> Result<Option<(PathBuf, Task, String)>, StoreError> {
        find_by(&self.tasks_dir(), |t: &Task| t.id == id)
    }
}

/// Scan `dir` for the first record matching `pred`, returning its path,
/// record, and preserved body.
fn find_by<T: serde::de::DeserializeOwned>(
    dir: &Path,
    pred: impl Fn(&T) -> bool,
) -> Result<Option<(PathBuf, T, String)>, StoreError> {
    if !dir.is_dir() {
        return Ok(None);
    }
    for path in record_files(dir)? {
        match MarkdownStore::read_record::<T>(&path) {
            Ok((record, body)) => {
                if pred(&record) {
                    return Ok(Some((path, record, body)));
                }
            }
            Err(e) => eprintln!("warn: store: skipping {}: {e}", path.display()),
        }
    }
    Ok(None)
}

/// All record markdown files under `dir`, accepting BOTH layouts:
/// * flat — `<dir>/EPC-001.md`
/// * nested — `<dir>/EPC-001/EPC-001.md`, where a record gets its own
///   directory so related files (tasks, specs) can live alongside it.
///
/// In the nested case only the record file named for its directory is
/// returned; sibling files are ignored. Sorted for stable ordering.
///
/// # Errors
///
/// Returns `StoreError::Io` if `dir` cannot be read.
fn record_files(dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            paths.push(path);
        } else if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let nested = path.join(format!("{name}.md"));
                if nested.is_file() {
                    paths.push(nested);
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
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
        Self::write_record(&path, epic, "")
    }

    fn get_epic(&self, id: &str) -> Result<Option<Epic>, StoreError> {
        Ok(self.find_epic(id)?.map(|(_, epic, _)| epic))
    }

    fn get_epic_by_format_id(&self, format_id: &str) -> Result<Option<Epic>, StoreError> {
        Ok(
            find_by(&self.epics_dir(), |e: &Epic| e.format_id == format_id)?
                .map(|(_, epic, _)| epic),
        )
    }

    fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), StoreError> {
        let (path, _epic, body) = self
            .find_epic(id)?
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
        Self::update_record_fields(&path, &body, fields)
    }

    fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, StoreError> {
        let records = Self::scan::<Epic>(&self.epics_dir())?;
        Ok(records
            .into_iter()
            .map(|(_, epic)| epic)
            .filter(|e| filter.status.is_none_or(|s| e.status == s))
            .collect())
    }

    fn create_task(&self, task: &Task) -> Result<(), StoreError> {
        let path = self.tasks_dir().join(format!("{}.md", task.format_id));
        Self::write_record(&path, task, "")
    }

    fn get_task(&self, id: &str) -> Result<Option<Task>, StoreError> {
        Ok(self.find_task(id)?.map(|(_, task, _)| task))
    }

    fn get_task_by_format_id(&self, format_id: &str) -> Result<Option<Task>, StoreError> {
        Ok(
            find_by(&self.tasks_dir(), |t: &Task| t.format_id == format_id)?
                .map(|(_, task, _)| task),
        )
    }

    fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), StoreError> {
        let (path, _task, body) = self
            .find_task(id)?
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
        Self::update_record_fields(&path, &body, fields)
    }

    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>, StoreError> {
        let records = Self::scan::<Task>(&self.tasks_dir())?;
        Ok(records
            .into_iter()
            .map(|(_, task)| task)
            .filter(|t| filter.status.is_none_or(|s| t.status == s))
            .filter(|t| filter.epic_id.as_deref().is_none_or(|eid| t.epic_id == eid))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{EpicStatus, TaskStatus};

    use super::*;

    fn make_epic(id: &str, format_id: &str) -> Epic {
        Epic {
            id: id.to_string(),
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

    fn make_task(id: &str, format_id: &str, epic_id: &str) -> Task {
        Task {
            id: id.to_string(),
            format_id: format_id.to_string(),
            epic_id: epic_id.to_string(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::Todo,
            work_type: "feat".to_string(),
            priority: "high".to_string(),
            estimate: None,
            acceptance: vec!["it works".to_string()],
            tests: vec![],
            depends_on: vec![],
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

        let fetched = store.get_epic("epic-01a").unwrap().unwrap();
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
        assert_eq!(fetched.id, "epic-01a");
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
                "epic-01a",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    title: Some("New Title".to_string()),
                    pr_number: Some(7),
                    ..Default::default()
                },
            )
            .unwrap();

        let epic = store.get_epic("epic-01a").unwrap().unwrap();
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
                "epic-01a",
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
        assert_eq!(drafts[0].id, "epic-01a");
    }

    #[test]
    fn test_create_and_get_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let task = make_task("task-01a", "TSK-001-001", "epic-01a");
        store.create_task(&task).unwrap();

        assert!(dir.path().join("tasks/TSK-001-001.md").exists());

        let fetched = store.get_task("task-01a").unwrap().unwrap();
        assert_eq!(fetched.format_id, "TSK-001-001");
        assert_eq!(fetched.acceptance, vec!["it works".to_string()]);

        let by_fid = store.get_task_by_format_id("TSK-001-001").unwrap().unwrap();
        assert_eq!(by_fid.id, "task-01a");
    }

    #[test]
    fn test_update_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_task(&make_task("task-01a", "TSK-001-001", "epic-01a"))
            .unwrap();

        store
            .update_task(
                "task-01a",
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    branch: Some("feat/test-branch".to_string()),
                    started_at: Some("2026-06-11T01:00:00Z".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let task = store.get_task("task-01a").unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.branch.as_deref(), Some("feat/test-branch"));
        assert_eq!(task.started_at.as_deref(), Some("2026-06-11T01:00:00Z"));
    }

    #[test]
    fn test_list_tasks_filters() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        store
            .create_task(&make_task("task-01a", "TSK-001-001", "epic-01a"))
            .unwrap();
        let mut other = make_task("task-01b", "TSK-002-001", "epic-01b");
        other.status = TaskStatus::InProgress;
        store.create_task(&other).unwrap();

        let all = store.list_tasks(TaskFilter::default()).unwrap();
        assert_eq!(all.len(), 2);

        let by_epic = store
            .list_tasks(TaskFilter {
                epic_id: Some("epic-01a".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_epic.len(), 1);
        assert_eq!(by_epic[0].id, "task-01a");

        let in_progress = store
            .list_tasks(TaskFilter {
                status: Some(TaskStatus::InProgress),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(in_progress.len(), 1);
        assert_eq!(in_progress[0].id, "task-01b");
    }

    #[test]
    fn test_scan_finds_nested_epic_layout() {
        // Dogfood layout: epics/EPC-001/EPC-001.md (dir per epic so tasks/ and
        // specs live alongside). Discovery must find the record inside the dir.
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let nested_dir = store.root().join("epics/EPC-001");
        fs::create_dir_all(&nested_dir).unwrap();
        MarkdownStore::write_record(
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
        assert_eq!(by_fid.id, "epic-01a");
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
        MarkdownStore::write_record(
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
    fn test_update_nested_epic_rewrites_in_place() {
        // An update to a nested epic must rewrite the nested file, not create a
        // stray flat one — find_by returns the real path.
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();
        let nested = store.root().join("epics/EPC-001/EPC-001.md");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        MarkdownStore::write_record(&nested, &make_epic("epic-01a", "EPC-001"), "## keep\n")
            .unwrap();

        store
            .update_epic(
                "epic-01a",
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
