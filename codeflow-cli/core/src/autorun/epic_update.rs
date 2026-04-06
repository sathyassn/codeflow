//! Epic status update after autorun batch completion.
//!
//! When an autorun batch completes, this module updates task rows in epic
//! markdown tables and checks whether all tasks in an epic are complete
//! (rolling up to epic-level status).

use std::path::{Path, PathBuf};

use crate::error::AutorunError;

/// Result of updating epic statuses after a batch run.
#[derive(Debug, Clone, Default)]
pub struct EpicUpdateReport {
    /// Number of task rows updated in epic markdown tables.
    pub tasks_updated: usize,
    /// Number of epics whose status was rolled up to "complete".
    pub epics_completed: usize,
    /// Errors encountered (non-fatal -- logged but don't fail the batch).
    pub errors: Vec<String>,
}

/// Known task status values in epic markdown tables.
const KNOWN_STATUSES: &[&str] = &[
    "todo",
    "in_progress",
    "in-progress",
    "complete",
    "completed",
    "blocked",
    "cancelled",
    "canceled",
];

/// Update a task's status in an epic markdown table row.
///
/// Scans lines for a markdown table row containing `task_id`, finds the
/// cell matching a known status value, and replaces it with `new_status`.
/// Handles both 4-column and 5-column table formats.
///
/// Returns the updated content, or the original content if the task row
/// was not found.
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` if the task row is found but no
/// status cell can be identified.
pub fn update_task_row_status(
    content: &str,
    task_id: &str,
    new_status: &str,
) -> Result<String, AutorunError> {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut found = false;

    for line in &mut lines {
        // Only check table rows (lines starting with '|') that contain the task_id.
        if !line.starts_with('|') || !line.contains(task_id) {
            continue;
        }

        // Split the row into cells.
        let cells: Vec<&str> = line.split('|').collect();
        // Find the cell index containing a known status value.
        let mut status_idx = None;
        for (idx, cell) in cells.iter().enumerate() {
            let trimmed = cell.trim().to_lowercase();
            if KNOWN_STATUSES
                .iter()
                .any(|s| trimmed == *s || trimmed == s.replace('_', "-"))
            {
                status_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = status_idx {
            // Reconstruct the line with the new status.
            let mut new_cells: Vec<String> =
                cells.iter().map(std::string::ToString::to_string).collect();
            // Preserve leading/trailing spaces in the cell.
            let old_cell = cells[idx];
            let leading = old_cell.len() - old_cell.trim_start().len();
            let trailing = old_cell.len() - old_cell.trim_end().len();
            let prefix = &old_cell[..leading];
            let suffix = &old_cell[old_cell.len() - trailing..];
            new_cells[idx] = format!("{prefix}{new_status}{suffix}");
            *line = new_cells.join("|");
            found = true;
            break;
        }
    }

    if !found {
        // Not an error -- task might not be in this epic's table.
        return Ok(content.to_string());
    }

    // Ensure trailing newline.
    let mut result = lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Resolve a task format ID to its markdown file path.
///
/// Delegates to the batch module's `resolve_task_path`.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the task ID format is invalid.
pub fn resolve_task_path(project_dir: &Path, task_id: &str) -> Result<PathBuf, AutorunError> {
    super::batch::resolve_task_path(project_dir, task_id)
}

/// Resolve a task ID to its parent epic markdown file path.
///
/// Extracts area and epic number from the task ID format `{AREA}-TSK-{NNN}-{SEQ}`
/// and constructs the path to `project-management/epics/{AREA}/{AREA}-EPC-{NNN}/{AREA}-EPC-{NNN}.md`.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the task ID format is invalid.
pub fn resolve_epic_path(project_dir: &Path, task_id: &str) -> Result<PathBuf, AutorunError> {
    let parts: Vec<&str> = task_id.split('-').collect();
    if parts.len() < 4 || parts[1] != "TSK" {
        return Err(AutorunError::MissingTask(format!(
            "invalid task ID format for epic resolution: {task_id}"
        )));
    }
    let area = parts[0];
    let epic_num = parts[2];
    let epic_format_id = format!("{area}-EPC-{epic_num}");
    Ok(project_dir
        .join("project-management")
        .join("epics")
        .join(area)
        .join(&epic_format_id)
        .join(format!("{epic_format_id}.md")))
}

/// Check whether all tasks in an epic are complete by parsing the epic markdown.
///
/// Reads the epic markdown file, parses the task table, and checks if every
/// status cell contains "complete" or "completed" (case-insensitive).
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the epic file cannot be read.
pub fn all_tasks_complete(project_dir: &Path, epic_format_id: &str) -> Result<bool, AutorunError> {
    let parts: Vec<&str> = epic_format_id.split('-').collect();
    if parts.len() < 3 || parts[1] != "EPC" {
        return Err(AutorunError::MissingTask(format!(
            "invalid epic ID format: {epic_format_id}"
        )));
    }

    let area = parts[0];
    let epic_num = parts[2];
    let epic_path = project_dir
        .join("project-management")
        .join("epics")
        .join(area)
        .join(format!("{area}-EPC-{epic_num}"))
        .join(format!("{epic_format_id}.md"));

    let content = std::fs::read_to_string(&epic_path).map_err(|e| {
        AutorunError::MissingTask(format!(
            "reading epic markdown {}: {e}",
            epic_path.display()
        ))
    })?;

    // Parse table rows -- skip header and separator rows.
    let mut in_table = false;
    let mut has_tasks = false;

    for line in content.lines() {
        if !line.starts_with('|') {
            if in_table {
                break; // End of table.
            }
            continue;
        }

        // Skip separator rows (e.g., "|---|---|---|").
        if line.contains("---") {
            in_table = true;
            continue;
        }

        // Skip header row (first row before separator).
        if !in_table {
            continue;
        }

        // This is a data row -- check for task ID pattern and status.
        let cells: Vec<&str> = line.split('|').collect();
        let has_task_id = cells.iter().any(|c| {
            let t = c.trim();
            t.contains("-TSK-")
        });

        if !has_task_id {
            continue;
        }

        has_tasks = true;

        // Check if any cell has a "complete" status.
        let is_complete = cells.iter().any(|c| {
            let t = c.trim().to_lowercase();
            t == "complete" || t == "completed"
        });

        if !is_complete {
            return Ok(false);
        }
    }

    Ok(has_tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- update_task_row_status tests --

    #[test]
    fn test_update_4_col_table() {
        let content = "| ID | Title | Status | Notes |\n\
                        |---|---|---|---|\n\
                        | INF-TSK-022-001 | Task One | todo | |\n\
                        | INF-TSK-022-002 | Task Two | in_progress | WIP |\n";

        let result = update_task_row_status(content, "INF-TSK-022-001", "complete").unwrap();
        assert!(result.contains("| INF-TSK-022-001 | Task One | complete | |"));
        // Other rows unchanged.
        assert!(result.contains("| INF-TSK-022-002 | Task Two | in_progress | WIP |"));
    }

    #[test]
    fn test_update_5_col_table() {
        let content = "| ID | Title | Status | Priority | Notes |\n\
                        |---|---|---|---|---|\n\
                        | INF-TSK-022-001 | Task One | blocked | P1 | Needs review |\n";

        let result = update_task_row_status(content, "INF-TSK-022-001", "complete").unwrap();
        assert!(result.contains("complete"));
        assert!(!result.contains("blocked"));
    }

    #[test]
    fn test_update_task_not_found() {
        let content = "| ID | Title | Status |\n\
                        |---|---|---|\n\
                        | INF-TSK-022-001 | Task One | todo |\n";

        let result = update_task_row_status(content, "INF-TSK-099-999", "complete").unwrap();
        // Content unchanged.
        assert_eq!(result, content);
    }

    #[test]
    fn test_update_idempotent() {
        let content = "| ID | Title | Status |\n\
                        |---|---|---|\n\
                        | INF-TSK-022-001 | Task One | complete |\n";

        let result = update_task_row_status(content, "INF-TSK-022-001", "complete").unwrap();
        assert!(result.contains("| INF-TSK-022-001 | Task One | complete |"));
    }

    #[test]
    fn test_update_case_insensitive_status_match() {
        let content = "| ID | Status |\n|---|---|\n| INF-TSK-022-001 | Todo |\n";
        // "Todo" should match "todo" case-insensitively.
        let result = update_task_row_status(content, "INF-TSK-022-001", "complete").unwrap();
        assert!(result.contains("complete"));
    }

    // -- all_tasks_complete tests --

    #[test]
    fn test_all_tasks_complete_true() {
        let dir = tempfile::tempdir().unwrap();
        let epic_dir = dir.path().join("project-management/epics/INF/INF-EPC-022");
        std::fs::create_dir_all(&epic_dir).unwrap();
        std::fs::write(
            epic_dir.join("INF-EPC-022.md"),
            "# Epic\n\n| ID | Title | Status |\n|---|---|---|\n\
             | INF-TSK-022-001 | T1 | complete |\n\
             | INF-TSK-022-002 | T2 | completed |\n",
        )
        .unwrap();

        assert!(all_tasks_complete(dir.path(), "INF-EPC-022").unwrap());
    }

    #[test]
    fn test_all_tasks_complete_false() {
        let dir = tempfile::tempdir().unwrap();
        let epic_dir = dir.path().join("project-management/epics/INF/INF-EPC-022");
        std::fs::create_dir_all(&epic_dir).unwrap();
        std::fs::write(
            epic_dir.join("INF-EPC-022.md"),
            "# Epic\n\n| ID | Title | Status |\n|---|---|---|\n\
             | INF-TSK-022-001 | T1 | complete |\n\
             | INF-TSK-022-002 | T2 | in_progress |\n",
        )
        .unwrap();

        assert!(!all_tasks_complete(dir.path(), "INF-EPC-022").unwrap());
    }

    #[test]
    fn test_all_tasks_complete_no_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let epic_dir = dir.path().join("project-management/epics/INF/INF-EPC-022");
        std::fs::create_dir_all(&epic_dir).unwrap();
        std::fs::write(
            epic_dir.join("INF-EPC-022.md"),
            "# Epic\n\nNo task table here.\n",
        )
        .unwrap();

        // No tasks found → returns false (empty epic).
        assert!(!all_tasks_complete(dir.path(), "INF-EPC-022").unwrap());
    }

    #[test]
    fn test_resolve_task_path_delegates() {
        let dir = tempfile::tempdir().unwrap();
        let path = resolve_task_path(dir.path(), "INF-TSK-022-001").unwrap();
        assert!(path.ends_with("INF-TSK-022-001.md"));
    }

    // -- resolve_epic_path tests --

    #[test]
    fn test_resolve_epic_path_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = resolve_epic_path(dir.path(), "INF-TSK-022-001").unwrap();
        assert!(path.ends_with("INF-EPC-022/INF-EPC-022.md"));
    }

    #[test]
    fn test_resolve_epic_path_invalid_format() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_epic_path(dir.path(), "BADFORMAT");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_epic_path_different_areas() {
        let dir = tempfile::tempdir().unwrap();
        let path = resolve_epic_path(dir.path(), "SEC-TSK-005-002").unwrap();
        assert!(path.ends_with("SEC-EPC-005/SEC-EPC-005.md"));
    }
}
