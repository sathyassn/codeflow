//! Deterministic id allocation and file scaffolding for new epics and tasks.
//!
//! Epic/task format ids (`EPC-NNN`, `TSK-NNN-MMM`) are hand-picked today, so
//! two agents in independent worktrees can still propose the same number.
//! Within one checkout, this module allocates the next free id by scanning the
//! `project-management/` tree (max existing + 1, restarting per epic for
//! tasks) and creates the file exclusively. Parallel plans therefore serialize
//! work-item allocation or resolve the visible same-path merge conflict; the
//! allocator is not a distributed id service.
//!
//! Allocation scans *files*, not parsed records: an id present on disk is
//! reserved even when its frontmatter is malformed or template-shaped, so the
//! allocator never reissues a number the store's parser would silently skip.
//! `CodeFlow` writes the canonical flat layout. The historical nested layout
//! (`epics/EPC-001/EPC-001.md`,
//! `epics/EPC-001/tasks/TSK-001-001.md`) remains read-compatible.

use std::fs;
use std::path::{Path, PathBuf};

use crate::scaffold::template::TemplateContext;
use crate::workgraph::store::StoreError;

/// A newly scaffolded record: its allocated format id and the file written.
#[derive(Debug, Clone)]
pub struct NewRecord {
    /// The allocated human-readable id (`EPC-NNN` or `TSK-NNN-MMM`).
    pub format_id: String,
    /// The file that was created.
    pub path: PathBuf,
}

/// Parse the sequence number from an `EPC-NNN` id (`None` if malformed).
fn epic_seq(id: &str) -> Option<u32> {
    id.strip_prefix("EPC-").and_then(|n| n.parse().ok())
}

/// Parse `(epic_seq, task_seq)` from a `TSK-NNN-MMM` id (`None` if malformed).
fn task_seqs(id: &str) -> Option<(u32, u32)> {
    let rest = id.strip_prefix("TSK-")?;
    let (epic, task) = rest.split_once('-')?;
    Some((epic.parse().ok()?, task.parse().ok()?))
}

/// The file stem of a record entry: the directory name for a nested record
/// (`epics/EPC-001/`), or the file name without `.md` for a flat one. The
/// store names both after their format id, so the stem is the candidate id.
fn record_stem(path: &Path) -> Option<String> {
    if path.is_dir() {
        path.file_name()
    } else if path.extension().is_some_and(|e| e == "md") {
        path.file_stem()
    } else {
        None
    }
    .and_then(|n| n.to_str())
    .map(str::to_string)
}

/// Existing epic sequence numbers under `epics/` (flat files and nested dirs).
fn scan_epic_seqs(epics_dir: &Path) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(epics_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| record_stem(&e.path()))
        .filter_map(|s| epic_seq(&s))
        .collect()
}

/// Collect `(epic_seq, task_seq)` pairs from every `TSK-*.md` file in `dir`.
fn collect_task_seqs(dir: &Path, out: &mut Vec<(u32, u32)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            if let Some(pair) = record_stem(&path).as_deref().and_then(task_seqs) {
                out.push(pair);
            }
        }
    }
}

/// Existing `(epic_seq, task_seq)` pairs, from the canonical flat `tasks/` dir
/// and legacy nested `epics/EPC-NNN/tasks/` dirs (both reserve their ids).
fn scan_task_seqs(pm_root: &Path) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    collect_task_seqs(&pm_root.join("tasks"), &mut out);
    if let Ok(entries) = fs::read_dir(pm_root.join("epics")) {
        for entry in entries.filter_map(Result::ok) {
            let dir = entry.path();
            if dir.is_dir() {
                collect_task_seqs(&dir.join("tasks"), &mut out);
            }
        }
    }
    out
}

/// The next free epic id under `pm_root` (`EPC-NNN` = highest existing + 1,
/// `EPC-001` when none exist).
#[must_use]
pub fn next_epic_id(pm_root: &Path) -> String {
    let next = scan_epic_seqs(&pm_root.join("epics"))
        .into_iter()
        .max()
        .unwrap_or(0)
        + 1;
    format!("EPC-{next:03}")
}

/// The next free task id within `epic_id` (`TSK-NNN-MMM`, numbering scoped to
/// the epic). `None` when `epic_id` is not a well-formed `EPC-NNN`.
#[must_use]
pub fn next_task_id(pm_root: &Path, epic_id: &str) -> Option<String> {
    if !crate::workgraph::format_id::is_valid_epic_format_id(epic_id) {
        return None;
    }
    let en = epic_seq(epic_id)?;
    let next = scan_task_seqs(pm_root)
        .into_iter()
        .filter(|(e, _)| *e == en)
        .map(|(_, t)| t)
        .max()
        .unwrap_or(0)
        + 1;
    Some(format!("TSK-{en:03}-{next:03}"))
}

/// Whether an epic with `epic_id` exists under `pm_root`.
#[must_use]
pub fn epic_exists(pm_root: &Path, epic_id: &str) -> bool {
    epic_seq(epic_id).is_some_and(|en| scan_epic_seqs(&pm_root.join("epics")).contains(&en))
}

/// Date portion (`YYYY-MM-DD`) of the current UTC timestamp.
fn today() -> String {
    super::now_rfc3339()[..10].to_string()
}

/// Render `template`, replacing the given `{{KEY}}` placeholders.
fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut ctx = TemplateContext::new();
    for (key, value) in values {
        ctx.set(*key, *value);
    }
    ctx.substitute(template)
}

/// Create `path` exclusively (never clobbers an existing file) and write it.
fn write_new(path: &Path, content: &str) -> Result<(), StoreError> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Allocate the next epic id under `pm_root` and scaffold it from `template`
/// (a `pm/epic.md.tmpl` body), filling `NNN`/`TITLE`/`DATE`.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if the `epics/` directory or the record file
/// cannot be written.
pub fn create_epic(pm_root: &Path, template: &str, title: &str) -> Result<NewRecord, StoreError> {
    let format_id = next_epic_id(pm_root);
    let nnn = format_id.strip_prefix("EPC-").unwrap_or(format_id.as_str());
    let date = today();
    let content = render(
        template,
        &[("NNN", nnn), ("TITLE", title), ("DATE", date.as_str())],
    );
    let dir = pm_root.join("epics");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{format_id}.md"));
    write_new(&path, &content)?;
    Ok(NewRecord { format_id, path })
}

/// Allocate the next task id within `epic_id` and scaffold it from `template`
/// (a `pm/task.md.tmpl` body), filling `NNN`/`MMM`/`TITLE`/`DATE`.
///
/// # Errors
///
/// Returns [`StoreError::NotFound`] if `epic_id` is not a well-formed
/// `EPC-NNN`, or [`StoreError::Io`] if the record file cannot be written.
pub fn create_task(
    pm_root: &Path,
    template: &str,
    epic_id: &str,
    title: &str,
) -> Result<NewRecord, StoreError> {
    let format_id = next_task_id(pm_root, epic_id)
        .ok_or_else(|| StoreError::NotFound(format!("epic:{epic_id}")))?;
    // format_id is `TSK-NNN-MMM`; split back out for the template placeholders.
    let (nnn, mmm) = format_id
        .strip_prefix("TSK-")
        .and_then(|r| r.split_once('-'))
        .unwrap_or(("", ""));
    let date = today();
    let content = render(
        template,
        &[
            ("NNN", nnn),
            ("MMM", mmm),
            ("TITLE", title),
            ("DATE", date.as_str()),
        ],
    );
    let dir = pm_root.join("tasks");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{format_id}.md"));
    write_new(&path, &content)?;
    Ok(NewRecord { format_id, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPIC_TMPL: &str = "---\nid: EPC-{{NNN}}\nformat_id: EPC-{{NNN}}\ntitle: {{TITLE}}\nstatus: draft\nwork_type: feat\ncreated: {{DATE}}\n---\n\n# EPC-{{NNN}} — {{TITLE}}\n";
    const TASK_TMPL: &str = "---\nid: TSK-{{NNN}}-{{MMM}}\nformat_id: TSK-{{NNN}}-{{MMM}}\nepic_id: EPC-{{NNN}}\ntitle: {{TITLE}}\nstatus: todo\nwork_type: feat\ncreated: {{DATE}}\n---\n\n# TSK-{{NNN}}-{{MMM}} — {{TITLE}}\n";

    fn pm() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("epics")).unwrap();
        fs::create_dir_all(dir.path().join("tasks")).unwrap();
        dir
    }

    /// Write a flat epic record file so it is discoverable by the scan.
    fn seed_epic(root: &Path, id: &str) {
        fs::write(
            root.join("epics").join(format!("{id}.md")),
            "---\nid: x\n---\n",
        )
        .unwrap();
    }

    /// Write a flat task record file so it is discoverable by the scan.
    fn seed_task(root: &Path, id: &str) {
        fs::write(
            root.join("tasks").join(format!("{id}.md")),
            "---\nid: x\n---\n",
        )
        .unwrap();
    }

    #[test]
    fn empty_project_starts_at_epic_001() {
        assert_eq!(next_epic_id(pm().path()), "EPC-001");
    }

    #[test]
    fn epic_allocation_picks_max_plus_one_not_count() {
        let dir = pm();
        // Non-contiguous ids prove it is max+1, not a count of files.
        seed_epic(dir.path(), "EPC-001");
        seed_epic(dir.path(), "EPC-005");
        assert_eq!(next_epic_id(dir.path()), "EPC-006");
    }

    #[test]
    fn epic_allocation_sees_nested_layout() {
        let dir = pm();
        // Dogfood layout: epics/EPC-003/EPC-003.md (dir named after its id).
        fs::create_dir_all(dir.path().join("epics/EPC-003")).unwrap();
        fs::write(
            dir.path().join("epics/EPC-003/EPC-003.md"),
            "---\nid: x\n---\n",
        )
        .unwrap();
        assert_eq!(next_epic_id(dir.path()), "EPC-004");
    }

    #[test]
    fn create_epic_allocates_renders_and_writes() {
        let dir = pm();
        seed_epic(dir.path(), "EPC-001");
        let rec = create_epic(dir.path(), EPIC_TMPL, "Ship it").unwrap();

        assert_eq!(rec.format_id, "EPC-002");
        assert_eq!(rec.path, dir.path().join("epics/EPC-002.md"));
        let body = fs::read_to_string(&rec.path).unwrap();
        assert!(body.contains("format_id: EPC-002"));
        assert!(body.contains("title: Ship it"));
        assert!(body.contains("# EPC-002 — Ship it"));
        // No placeholder survives.
        assert!(!body.contains("{{"));
        // A second allocation now sees the file just written.
        assert_eq!(next_epic_id(dir.path()), "EPC-003");
    }

    #[test]
    fn empty_epic_starts_tasks_at_001() {
        let dir = pm();
        seed_epic(dir.path(), "EPC-007");
        assert_eq!(next_task_id(dir.path(), "EPC-007").unwrap(), "TSK-007-001");
    }

    #[test]
    fn task_numbering_is_scoped_to_its_epic() {
        let dir = pm();
        seed_task(dir.path(), "TSK-007-001");
        seed_task(dir.path(), "TSK-007-002");
        // A different epic's tasks must not raise EPC-007's next number.
        seed_task(dir.path(), "TSK-008-001");
        seed_task(dir.path(), "TSK-008-002");
        seed_task(dir.path(), "TSK-008-003");

        assert_eq!(next_task_id(dir.path(), "EPC-007").unwrap(), "TSK-007-003");
        assert_eq!(next_task_id(dir.path(), "EPC-008").unwrap(), "TSK-008-004");
        // An epic with no tasks yet starts at 001.
        assert_eq!(next_task_id(dir.path(), "EPC-009").unwrap(), "TSK-009-001");
    }

    #[test]
    fn task_allocation_sees_nested_epic_tasks() {
        let dir = pm();
        // Tasks nested under their epic dir must still reserve their numbers.
        fs::create_dir_all(dir.path().join("epics/EPC-004/tasks")).unwrap();
        fs::write(
            dir.path().join("epics/EPC-004/tasks/TSK-004-001.md"),
            "---\nid: x\n---\n",
        )
        .unwrap();
        assert_eq!(next_task_id(dir.path(), "EPC-004").unwrap(), "TSK-004-002");
    }

    #[test]
    fn create_task_allocates_renders_and_writes() {
        let dir = pm();
        seed_epic(dir.path(), "EPC-007");
        seed_task(dir.path(), "TSK-007-001");
        let rec = create_task(dir.path(), TASK_TMPL, "EPC-007", "Do the thing").unwrap();

        assert_eq!(rec.format_id, "TSK-007-002");
        assert_eq!(rec.path, dir.path().join("tasks/TSK-007-002.md"));
        let body = fs::read_to_string(&rec.path).unwrap();
        assert!(body.contains("format_id: TSK-007-002"));
        assert!(body.contains("epic_id: EPC-007"));
        assert!(body.contains("title: Do the thing"));
        assert!(!body.contains("{{"));
        // The just-written task is now seen by the next allocation.
        assert_eq!(next_task_id(dir.path(), "EPC-007").unwrap(), "TSK-007-003");
    }

    #[test]
    fn next_task_id_rejects_malformed_epic() {
        assert!(next_task_id(pm().path(), "EPC-1").is_none());
        assert!(next_task_id(pm().path(), "nonsense").is_none());
    }

    #[test]
    fn create_task_errors_on_malformed_epic() {
        let err = create_task(pm().path(), TASK_TMPL, "bogus", "t").unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn epic_exists_reflects_disk() {
        let dir = pm();
        seed_epic(dir.path(), "EPC-002");
        assert!(epic_exists(dir.path(), "EPC-002"));
        assert!(!epic_exists(dir.path(), "EPC-001"));
        assert!(!epic_exists(dir.path(), "malformed"));
    }
}
