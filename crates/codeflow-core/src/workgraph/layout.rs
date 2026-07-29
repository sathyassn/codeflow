//! Canonical and compatibility paths for project-management records.
//!
//! `CodeFlow` writes the flat layout. The nested epic layout is retained as
//! read-only compatibility for existing repositories; all readers use these
//! enumerators so status, validation, recall, and allocation do not drift.

use std::fs;
use std::path::{Path, PathBuf};

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
