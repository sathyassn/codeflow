//! Canonical and compatibility paths for project-management records.
//!
//! `CodeFlow` writes the flat layout. The nested epic layout is retained as
//! read-only compatibility for existing repositories; all readers use these
//! enumerators so status, validation, recall, and allocation do not drift.

use std::fs;
use std::path::{Path, PathBuf};

use super::is_valid_task_format_id;

/// Bounded inventory for hostile-input readers; existing store/lint callers keep
/// their historical behavior. Paths follow exactly the layouts enumerated below.
#[derive(Default)]
pub(crate) struct RecordFiles {
    pub tasks: Vec<PathBuf>,
    pub epics: Vec<PathBuf>,
    pub specs: Vec<PathBuf>,
}

#[derive(Debug)]
pub(crate) enum InventoryError {
    LimitExceeded,
    Unreadable,
}

pub(crate) fn bounded_record_files(
    pm_root: &Path,
    maximum_entries: usize,
) -> Result<RecordFiles, InventoryError> {
    let mut remaining = maximum_entries;
    let mut files = RecordFiles::default();
    bounded_markdown_files(&pm_root.join("tasks"), &mut remaining, &mut files.tasks)?;
    bounded_markdown_files(&pm_root.join("specs"), &mut remaining, &mut files.specs)?;
    bounded_entries(
        &pm_root.join("epics"),
        &mut remaining,
        |entry, remaining| {
            let kind = entry.file_type().map_err(|_| InventoryError::Unreadable)?;
            let path = entry.path();
            if kind.is_file() && path.extension().is_some_and(|extension| extension == "md") {
                files.epics.push(path);
            } else if kind.is_dir() {
                let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                    return Ok(());
                };
                let nested = path.join(format!("{name}.md"));
                if let Ok(metadata) = fs::symlink_metadata(&nested) {
                    charge_entry(remaining)?;
                    if metadata.file_type().is_file() {
                        files.epics.push(nested);
                    }
                }
                bounded_markdown_files(&path.join("tasks"), remaining, &mut files.tasks)?;
            }
            Ok(())
        },
    )?;
    files.tasks.sort();
    files.epics.sort();
    files.specs.sort();
    Ok(files)
}

/// Look only in the flat and historical nested task homes when deciding whether
/// an uninitialized or lower-tier repository already carries `CodeFlow` work.
/// Unlike a complete inventory, this never enters the spec directory or reads
/// epic record contents; the bounded epics-directory walk only locates legacy
/// nested task homes. Task contents are deliberately not parsed here: malformed
/// TSK-shaped records must reach the normal workgraph validator.
pub(crate) fn has_task_record_path(
    pm_root: &Path,
    maximum_entries: usize,
) -> Result<bool, InventoryError> {
    // The parent must also be a real directory: checking only child paths
    // would follow a symlinked project-management directory outside the repo.
    if checked_directory_entries(pm_root)?.is_none() {
        return Ok(false);
    }
    let mut remaining = maximum_entries;
    if task_dir_has_record(&pm_root.join("tasks"), &mut remaining)? {
        return Ok(true);
    }

    let epics = pm_root.join("epics");
    let Some(entries) = checked_directory_entries(&epics)? else {
        return Ok(false);
    };
    for entry in entries {
        charge_entry(&mut remaining)?;
        let entry = entry.map_err(|_| InventoryError::Unreadable)?;
        if entry
            .file_type()
            .map_err(|_| InventoryError::Unreadable)?
            .is_dir()
            && task_dir_has_record(&entry.path().join("tasks"), &mut remaining)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn task_dir_has_record(dir: &Path, remaining: &mut usize) -> Result<bool, InventoryError> {
    let Some(entries) = checked_directory_entries(dir)? else {
        return Ok(false);
    };
    for entry in entries {
        charge_entry(remaining)?;
        let entry = entry.map_err(|_| InventoryError::Unreadable)?;
        if !entry
            .file_type()
            .map_err(|_| InventoryError::Unreadable)?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "md")
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(is_valid_task_format_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn checked_directory_entries(dir: &Path) -> Result<Option<fs::ReadDir>, InventoryError> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(InventoryError::Unreadable),
        Ok(_) => {}
    }
    fs::read_dir(dir)
        .map(Some)
        .map_err(|_| InventoryError::Unreadable)
}

fn charge_entry(remaining: &mut usize) -> Result<(), InventoryError> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or(InventoryError::LimitExceeded)?;
    Ok(())
}

fn bounded_entries(
    dir: &Path,
    remaining: &mut usize,
    mut visit: impl FnMut(fs::DirEntry, &mut usize) -> Result<(), InventoryError>,
) -> Result<(), InventoryError> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(InventoryError::Unreadable),
        Ok(_) => {}
    }
    for entry in fs::read_dir(dir).map_err(|_| InventoryError::Unreadable)? {
        charge_entry(remaining)?;
        visit(entry.map_err(|_| InventoryError::Unreadable)?, remaining)?;
    }
    Ok(())
}

fn bounded_markdown_files(
    dir: &Path,
    remaining: &mut usize,
    paths: &mut Vec<PathBuf>,
) -> Result<(), InventoryError> {
    bounded_entries(dir, remaining, |entry, _| {
        if entry
            .file_type()
            .map_err(|_| InventoryError::Unreadable)?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "md")
        {
            paths.push(entry.path());
        }
        Ok(())
    })
}

/// Canonical flat epic records plus legacy nested epic records.
pub(crate) fn epic_record_files(pm_root: &Path) -> Vec<PathBuf> {
    let epics = pm_root.join("epics");
    let mut paths = direct_markdown_files(&epics);
    let Some(entries) = real_directory_entries(&epics) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let nested = path.join(format!("{name}.md"));
        if fs::symlink_metadata(&nested).is_ok_and(|metadata| metadata.file_type().is_file()) {
            paths.push(nested);
        }
    }
    paths.sort();
    paths
}

/// Canonical flat task records plus tasks under legacy nested epics.
pub(crate) fn task_record_files(pm_root: &Path) -> Vec<PathBuf> {
    let mut paths = direct_markdown_files(&pm_root.join("tasks"));
    let epics = pm_root.join("epics");
    let Some(entries) = real_directory_entries(&epics) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            paths.extend(direct_markdown_files(&entry.path().join("tasks")));
        }
    }
    paths.sort();
    paths
}

/// Canonical flat spec records.
pub(crate) fn spec_record_files(pm_root: &Path) -> Vec<PathBuf> {
    direct_markdown_files(&pm_root.join("specs"))
}

fn direct_markdown_files(dir: &Path) -> Vec<PathBuf> {
    let Some(entries) = real_directory_entries(dir) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let path = entry.path();
            (file_type.is_file() && path.extension().is_some_and(|extension| extension == "md"))
                .then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Whether `project-management` or one of its kind folders is a symbolic
/// link or junction. Summaries use it to name a refused records folder
/// instead of treating an empty or dangling target as absent (issue 94).
pub(crate) fn records_folder_is_linked(pm_root: &Path) -> bool {
    std::iter::once(pm_root.to_path_buf())
        .chain(["epics", "specs", "tasks"].map(|kind| pm_root.join(kind)))
        .any(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()))
}

fn real_directory_entries(dir: &Path) -> Option<fs::ReadDir> {
    let Ok(metadata) = fs::symlink_metadata(dir) else {
        return None;
    };
    if !metadata.file_type().is_dir() {
        return None;
    }
    fs::read_dir(dir).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_activation_probe_reads_only_supported_task_homes() {
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path();
        fs::create_dir_all(pm.join("tasks")).unwrap();
        fs::create_dir_all(pm.join("specs")).unwrap();
        fs::create_dir_all(pm.join("epics/EPC-002/tasks")).unwrap();
        fs::write(pm.join("tasks/notes.md"), "foreign note").unwrap();
        fs::write(pm.join("specs/SPC-001.md"), "unrelated spec").unwrap();
        assert!(!has_task_record_path(pm, 4).unwrap());

        fs::write(pm.join("epics/EPC-002/tasks/TSK-002-001.md"), "not YAML").unwrap();
        assert!(has_task_record_path(pm, 4).unwrap());
        fs::remove_file(pm.join("epics/EPC-002/tasks/TSK-002-001.md")).unwrap();
        fs::write(pm.join("tasks/TSK-001.md"), "not YAML").unwrap();
        // Directory order is platform-defined (NTFS lists notes.md first), so
        // budget both flat entries rather than depend on enumeration order.
        assert!(has_task_record_path(pm, 2).unwrap());
    }

    #[test]
    fn task_activation_probe_reports_bounded_uncertainty() {
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path();
        fs::create_dir_all(pm.join("tasks")).unwrap();
        fs::write(pm.join("tasks/notes.md"), "foreign note").unwrap();
        assert!(matches!(
            has_task_record_path(pm, 0),
            Err(InventoryError::LimitExceeded)
        ));
        assert!(!has_task_record_path(pm, 1).unwrap());

        // An unrelated spec directory is never part of this decision.
        fs::create_dir_all(pm.join("specs")).unwrap();
        for index in 0..10 {
            fs::write(pm.join(format!("specs/SPC-{index:03}.md")), "spec").unwrap();
        }
        assert!(!has_task_record_path(pm, 1).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn task_activation_probe_does_not_follow_task_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("TSK-001.md"), "external").unwrap();
        fs::create_dir_all(pm.join("epics/EPC-001")).unwrap();
        symlink(outside.path(), pm.join("tasks")).unwrap();
        symlink(outside.path(), pm.join("epics/EPC-001/tasks")).unwrap();
        assert!(!has_task_record_path(pm, 1).unwrap());

        fs::remove_file(pm.join("tasks")).unwrap();
        fs::create_dir_all(pm.join("tasks")).unwrap();
        symlink(
            outside.path().join("TSK-001.md"),
            pm.join("tasks/TSK-001.md"),
        )
        .unwrap();
        assert!(!has_task_record_path(pm, 2).unwrap());

        let linked_parent = tempfile::tempdir().unwrap();
        symlink(pm, linked_parent.path().join("project-management")).unwrap();
        assert!(
            !has_task_record_path(&linked_parent.path().join("project-management"), 2).unwrap()
        );
    }

    #[test]
    fn bounded_inventory_matches_existing_layouts_and_fails_before_truncating() {
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path();
        fs::create_dir_all(pm.join("tasks")).unwrap();
        fs::create_dir_all(pm.join("specs")).unwrap();
        fs::create_dir_all(pm.join("epics/EPC-002/tasks")).unwrap();
        for path in [
            "tasks/TSK-001.md",
            "specs/SPC-001.md",
            "epics/EPC-001.md",
            "epics/EPC-002/EPC-002.md",
            "epics/EPC-002/tasks/TSK-002-001.md",
        ] {
            fs::write(pm.join(path), "record").unwrap();
        }
        let files = bounded_record_files(pm, 6).unwrap();
        assert_eq!(files.tasks, task_record_files(pm));
        assert_eq!(files.epics, epic_record_files(pm));
        assert_eq!(files.specs, spec_record_files(pm));
        assert!(matches!(
            bounded_record_files(pm, 5),
            Err(InventoryError::LimitExceeded)
        ));
        fs::write(pm.join("tasks/not-a-record.txt"), "ignored but counted").unwrap();
        assert!(matches!(
            bounded_record_files(pm, 6),
            Err(InventoryError::LimitExceeded)
        ));
    }

    #[test]
    fn enumerates_flat_and_legacy_records_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path();
        fs::create_dir_all(pm.join("epics/EPC-002/tasks")).unwrap();
        fs::create_dir_all(pm.join("tasks")).unwrap();
        fs::create_dir_all(pm.join("specs")).unwrap();
        fs::write(pm.join("epics/EPC-001.md"), "").unwrap();
        fs::write(pm.join("epics/EPC-002/EPC-002.md"), "").unwrap();
        fs::write(pm.join("tasks/TSK-001-001.md"), "").unwrap();
        fs::write(pm.join("epics/EPC-002/tasks/TSK-002-001.md"), "").unwrap();
        fs::write(pm.join("specs/SPC-001.md"), "").unwrap();

        assert_eq!(epic_record_files(pm).len(), 2);
        assert_eq!(task_record_files(pm).len(), 2);
        assert_eq!(spec_record_files(pm).len(), 1);

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(pm.join("epics/EPC-001.md"), pm.join("epics/EPC-999.md"))
                .unwrap();
            std::os::unix::fs::symlink(pm.join("tasks"), pm.join("epics/EPC-998")).unwrap();
            assert_eq!(epic_record_files(pm).len(), 2);
            assert_eq!(task_record_files(pm).len(), 2);

            let linked_pm = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(pm.join("epics"), linked_pm.path().join("epics")).unwrap();
            assert!(epic_record_files(linked_pm.path()).is_empty());
            assert!(task_record_files(linked_pm.path()).is_empty());
        }
    }
}
