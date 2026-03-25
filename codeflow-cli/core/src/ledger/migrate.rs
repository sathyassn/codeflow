//! One-time migration from flat JSONL layout to subdirectory layout.
//!
//! Flat layout (pre-migration):
//! ```text
//! .state/ledger/
//! ├── work-graph.jsonl
//! ├── sessions.jsonl
//! └── ...
//! ```
//!
//! Subdirectory layout (post-migration):
//! ```text
//! .state/ledger/
//! ├── work-graph/
//! │   └── work-graph.jsonl
//! ├── sessions/
//! │   └── sessions.jsonl
//! └── ...
//! ```

use std::fs;
use std::path::Path;

use crate::error::LedgerError;

use super::MigrationResult;
use super::files;

/// Migrate from flat JSONL layout to subdirectory layout.
///
/// For each canonical type, moves `{type}.jsonl` into `{type}/{type}.jsonl`.
/// Idempotent: skips types that are already migrated (subdirectory exists
/// with base file). Creates empty base files for types that have no flat file.
///
/// # Errors
///
/// Returns `LedgerError::Io` if filesystem operations fail. Partial migration
/// is safe -- next invocation resumes from where it left off.
pub fn migrate_flat_to_subdirs(ledger_dir: &Path) -> Result<MigrationResult, LedgerError> {
    if !ledger_dir.exists() {
        fs::create_dir_all(ledger_dir)?;
        return Ok(MigrationResult {
            migrated_count: 0,
            already_migrated: true,
        });
    }

    // Detect flat layout: any `{type}.jsonl` file in the ledger root.
    let has_flat_files = files::ALL
        .iter()
        .any(|type_name| ledger_dir.join(format!("{type_name}.jsonl")).exists());

    // Check if subdirectory layout already exists.
    let has_subdirs = files::ALL
        .iter()
        .any(|type_name| ledger_dir.join(type_name).is_dir());

    if !has_flat_files && has_subdirs {
        return Ok(MigrationResult {
            migrated_count: 0,
            already_migrated: true,
        });
    }

    // If neither flat files nor subdirs exist, create empty subdirs.
    if !has_flat_files && !has_subdirs {
        for type_name in files::ALL {
            let subdir = ledger_dir.join(type_name);
            fs::create_dir_all(&subdir)?;
        }
        return Ok(MigrationResult {
            migrated_count: 0,
            already_migrated: true,
        });
    }

    let mut migrated_count = 0;

    for type_name in files::ALL {
        let flat_file = ledger_dir.join(format!("{type_name}.jsonl"));
        let subdir = ledger_dir.join(type_name);
        let base_file = subdir.join(format!("{type_name}.jsonl"));

        // Skip if already migrated (subdir exists with base file).
        if subdir.is_dir() && base_file.exists() {
            continue;
        }

        // Create subdirectory.
        fs::create_dir_all(&subdir)?;

        if flat_file.exists() {
            // Move flat file to subdirectory as base file.
            fs::rename(&flat_file, &base_file)?;
            migrated_count += 1;

            // Also move lock file if it exists.
            let flat_lock = ledger_dir.join(format!("{type_name}.jsonl.lock"));
            if flat_lock.exists() {
                let base_lock = subdir.join(format!("{type_name}.jsonl.lock"));
                let _ = fs::rename(&flat_lock, &base_lock);
            }
        }
        // If flat file doesn't exist but subdir was just created, leave it empty.
        // Base file will be created on first write.
    }

    Ok(MigrationResult {
        migrated_count,
        already_migrated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();

        let result = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(result.already_migrated);
        assert_eq!(result.migrated_count, 0);

        // Subdirectories should be created.
        for type_name in files::ALL {
            assert!(ledger_dir.join(type_name).is_dir());
        }
    }

    #[test]
    fn test_migrate_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();

        // Create flat files.
        fs::write(
            ledger_dir.join("work-graph.jsonl"),
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        fs::write(
            ledger_dir.join("sessions.jsonl"),
            r#"{"event":"session_start","timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        fs::write(ledger_dir.join("sessions.jsonl.lock"), "").unwrap();

        let result = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(!result.already_migrated);
        assert_eq!(result.migrated_count, 2);

        // Flat files should be moved.
        assert!(!ledger_dir.join("work-graph.jsonl").exists());
        assert!(!ledger_dir.join("sessions.jsonl").exists());
        assert!(!ledger_dir.join("sessions.jsonl.lock").exists());

        // Subdirectory files should exist.
        assert!(ledger_dir.join("work-graph/work-graph.jsonl").exists());
        assert!(ledger_dir.join("sessions/sessions.jsonl").exists());
        assert!(ledger_dir.join("sessions/sessions.jsonl.lock").exists());

        // Content should be preserved.
        let content = fs::read_to_string(ledger_dir.join("work-graph/work-graph.jsonl")).unwrap();
        assert!(content.contains("task_created"));
    }

    #[test]
    fn test_migrate_already_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");

        // Create subdirectory layout.
        for type_name in files::ALL {
            let subdir = ledger_dir.join(type_name);
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join(format!("{type_name}.jsonl")), "").unwrap();
        }

        let result = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(result.already_migrated);
        assert_eq!(result.migrated_count, 0);
    }

    #[test]
    fn test_migrate_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();

        fs::write(ledger_dir.join("sessions.jsonl"), "line1\n").unwrap();

        let r1 = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert_eq!(r1.migrated_count, 1);

        let r2 = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(r2.already_migrated);
        assert_eq!(r2.migrated_count, 0);

        // Content still intact.
        let content = fs::read_to_string(ledger_dir.join("sessions/sessions.jsonl")).unwrap();
        assert_eq!(content, "line1\n");
    }

    #[test]
    fn test_migrate_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("does-not-exist");

        let result = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(result.already_migrated);
    }

    #[test]
    fn test_migrate_partial_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();

        // Only some flat files exist.
        fs::write(ledger_dir.join("config.jsonl"), "data\n").unwrap();

        let result = migrate_flat_to_subdirs(&ledger_dir).unwrap();
        assert!(!result.already_migrated);
        assert_eq!(result.migrated_count, 1);

        assert!(ledger_dir.join("config/config.jsonl").exists());
        // Other subdirs should be created even without flat files.
        for type_name in files::ALL {
            assert!(ledger_dir.join(type_name).is_dir());
        }
    }
}
