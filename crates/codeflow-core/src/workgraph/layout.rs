//! Canonical and compatibility paths for project-management records.
//!
//! `CodeFlow` writes the flat layout. The nested epic layout is retained as
//! read-only compatibility for existing repositories; all readers use these
//! enumerators so status, validation, recall, and allocation do not drift.

use std::fs;
use std::path::{Path, PathBuf};

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
