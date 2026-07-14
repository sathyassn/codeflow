//! Ledger compaction: merge completed session fragments into the base file.
//!
//! Only merges fragments from sessions that have a `session_end` event.
//! Active sessions (no `session_end`) and the current session are skipped.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::files;
use super::{CompactionResult, Event, LedgerError};

/// Compact a single ledger type by merging completed session fragments into the base.
///
/// Algorithm:
/// 1. Acquire `.compaction.lock` (flock exclusive)
/// 2. Identify completed sessions (has `session_end` AND not current)
/// 3. Read base file events
/// 4. Read all completed fragment files
/// 5. Merge: base + fragments, sort by timestamp (stable)
/// 6. Write base file atomically (tmp + rename)
/// 7. Delete merged fragment files
/// 8. Release lock
///
/// # Errors
///
/// Returns `LedgerError::Lock` if another compaction is in progress.
/// Returns `LedgerError::Io` on filesystem errors.
pub fn compact_ledger_type(
    ledger_dir: &Path,
    type_name: &str,
    current_session_id: Option<&str>,
) -> Result<CompactionResult, LedgerError> {
    let subdir = ledger_dir.join(type_name);
    if !subdir.is_dir() {
        return Ok(CompactionResult {
            type_name: type_name.to_string(),
            merged_count: 0,
            deleted_files: Vec::new(),
            skipped_active: 0,
        });
    }

    // Acquire compaction lock.
    let lock_path = subdir.join(".compaction.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;

    lock_file.try_lock_exclusive().map_err(|_| {
        LedgerError::Lock(format!("compaction already in progress for {type_name}"))
    })?;

    // Find compactable fragments.
    let completed_sessions = find_completed_sessions(ledger_dir);
    let fragments =
        compactable_fragments(&subdir, type_name, &completed_sessions, current_session_id);

    if fragments.is_empty() {
        drop(lock_file);
        return Ok(CompactionResult {
            type_name: type_name.to_string(),
            merged_count: 0,
            deleted_files: Vec::new(),
            skipped_active: count_active_fragments(
                &subdir,
                type_name,
                &completed_sessions,
                current_session_id,
            ),
        });
    }

    // Acquire the per-file writer lock for every file compaction will
    // read/replace/delete, so a concurrent append cannot be lost across the
    // read→rename→delete window. `None` = a write is in progress → defer.
    let base_path = subdir.join(format!("{type_name}.jsonl"));
    let Some(held_locks) = acquire_file_locks(&base_path, &fragments)? else {
        return Ok(CompactionResult {
            type_name: type_name.to_string(),
            merged_count: 0,
            deleted_files: Vec::new(),
            skipped_active: count_active_fragments(
                &subdir,
                type_name,
                &completed_sessions,
                current_session_id,
            ),
        });
    };

    // Read base + fragment events. Fail CLOSED: a read or parse error aborts
    // here, BEFORE the write/delete below, so a corrupt or partially written
    // source is never merged-and-deleted (permanent data loss).
    let mut all_events: Vec<Event> = Vec::new();
    if base_path.exists() {
        read_events_from_file(&base_path, &mut all_events)?;
    }
    for frag_path in &fragments {
        read_events_from_file(frag_path, &mut all_events)?;
    }

    // Sort by timestamp (stable preserves intra-file order).
    all_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    // Write merged base atomically.
    let tmp_path = subdir.join(format!("{type_name}.jsonl.tmp"));
    {
        let mut f = fs::File::create(&tmp_path)?;
        for event in &all_events {
            let mut line = serde_json::to_vec(event)?;
            line.push(b'\n');
            f.write_all(&line)?;
        }
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &base_path)?;

    // Delete merged fragments.
    let mut deleted_files = Vec::new();
    for frag_path in &fragments {
        if fs::remove_file(frag_path).is_ok() {
            deleted_files.push(frag_path.clone());
        }
        // Also remove lock file for the fragment.
        let frag_lock = frag_path.with_extension("jsonl.lock");
        let _ = fs::remove_file(&frag_lock);
    }

    let merged_count = deleted_files.len();

    // Release the per-file locks (held across the whole read→rename→delete
    // window) and then the compaction lock.
    drop(held_locks);
    drop(lock_file);

    Ok(CompactionResult {
        type_name: type_name.to_string(),
        merged_count,
        deleted_files,
        skipped_active: count_active_fragments(
            &subdir,
            type_name,
            &completed_sessions,
            current_session_id,
        ),
    })
}

/// Compact all ledger types.
///
/// # Errors
///
/// Returns errors from individual type compactions.
pub fn compact_all(
    ledger_dir: &Path,
    current_session_id: Option<&str>,
) -> Result<Vec<CompactionResult>, LedgerError> {
    let mut results = Vec::new();
    for type_name in files::ALL {
        results.push(compact_ledger_type(
            ledger_dir,
            type_name,
            current_session_id,
        )?);
    }
    Ok(results)
}

/// Find session IDs that have a `session_end` event in the sessions type.
fn find_completed_sessions(ledger_dir: &Path) -> std::collections::HashSet<String> {
    let mut completed = std::collections::HashSet::new();
    let sessions_dir = ledger_dir.join("sessions");
    if !sessions_dir.is_dir() {
        return completed;
    }

    // Check base file.
    let base = sessions_dir.join("sessions.jsonl");
    scan_for_session_ends(&base, &mut completed);

    // Check all fragment files.
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("sessions-ses-")
                && super::is_jsonl_file(name)
                && !super::is_lock_file(name)
            {
                scan_for_session_ends(&path, &mut completed);
            }
        }
    }

    completed
}

/// Scan a JSONL file for `session_end` events and record their session IDs.
fn scan_for_session_ends(path: &Path, completed: &mut std::collections::HashSet<String>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<Event>(trimmed) {
            if event.event_type == "session_end" {
                if let Some(sid) = &event.session_id {
                    completed.insert(sid.clone());
                }
            }
        }
    }
}

/// Find fragment files that can be compacted (their session is completed).
fn compactable_fragments(
    subdir: &Path,
    type_name: &str,
    completed_sessions: &std::collections::HashSet<String>,
    current_session_id: Option<&str>,
) -> Vec<PathBuf> {
    let mut fragments = Vec::new();
    let prefix = format!("{type_name}-");

    if let Ok(entries) = fs::read_dir(subdir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with(&prefix)
                || !super::is_jsonl_file(name)
                || super::is_lock_file(name)
            {
                continue;
            }

            // Extract session ID from filename: {type}-{session_id}.jsonl
            let sid = name
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".jsonl"));
            if let Some(sid) = sid {
                // Skip current session.
                if current_session_id == Some(sid) {
                    continue;
                }
                // Only compact if session is completed.
                if completed_sessions.contains(sid) {
                    fragments.push(path);
                }
            }
        }
    }

    fragments
}

/// Count fragments that were skipped (active sessions).
fn count_active_fragments(
    subdir: &Path,
    type_name: &str,
    completed_sessions: &std::collections::HashSet<String>,
    current_session_id: Option<&str>,
) -> usize {
    let prefix = format!("{type_name}-");
    let mut count = 0;

    if let Ok(entries) = fs::read_dir(subdir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_str().unwrap_or("").to_string();
            if !name.starts_with(&prefix)
                || !super::is_jsonl_file(&name)
                || super::is_lock_file(&name)
            {
                continue;
            }
            let sid = name
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".jsonl"));
            if let Some(sid) = sid {
                if current_session_id == Some(sid) || !completed_sessions.contains(sid) {
                    count += 1;
                }
            }
        }
    }

    count
}

/// Try to acquire the writer lock for the base file and every fragment (each
/// `{file}.jsonl.lock`, matching the append path). Returns `None` if any file is
/// currently being written (defer this round), otherwise the held lock handles
/// to keep for the whole read→rename→delete window.
///
/// `try_lock` never stalls a writer; the caller's `.compaction.lock` already
/// serializes compactions, and writers hold only one file lock at a time, so
/// acquiring several here in a stable order cannot deadlock.
fn acquire_file_locks(
    base_path: &Path,
    fragments: &[PathBuf],
) -> Result<Option<Vec<fs::File>>, LedgerError> {
    let mut targets: Vec<PathBuf> = vec![base_path.to_path_buf()];
    targets.extend(fragments.iter().cloned());
    targets.sort();
    let mut held = Vec::new();
    for target in &targets {
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(target.with_extension("jsonl.lock"))?;
        if lock.try_lock_exclusive().is_err() {
            return Ok(None);
        }
        held.push(lock);
    }
    Ok(Some(held))
}

/// Parse JSONL events from a file, appending to `events`.
///
/// Fails closed: a read error or a single unparseable line returns an error
/// rather than silently dropping events. The caller must abort compaction on
/// error so a corrupt or partially written source is never merged-and-deleted
/// (which would be permanent ledger data loss).
fn read_events_from_file(path: &Path, events: &mut Vec<Event>) -> Result<(), LedgerError> {
    let content = fs::read_to_string(path)?;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = serde_json::from_str::<Event>(trimmed).map_err(|e| {
            LedgerError::Corrupt(format!("{}:{}: {e}", path.display(), i + 1))
        })?;
        events.push(event);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_line(path: &Path, line: &str) {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(f, "{line}").unwrap();
    }

    fn setup_sessions_with_end(ledger_dir: &Path, sid: &str) {
        let sessions_dir = ledger_dir.join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let base = sessions_dir.join("sessions.jsonl");
        write_line(
            &base,
            &format!(
                r#"{{"event":"session_end","timestamp":"2026-01-01T01:00:00Z","session_id":"{sid}"}}"#
            ),
        );
    }

    #[test]
    fn test_compact_empty_type() {
        let dir = tempfile::tempdir().unwrap();
        let result = compact_ledger_type(dir.path(), "work-graph", None).unwrap();
        assert_eq!(result.merged_count, 0);
        assert_eq!(result.skipped_active, 0);
    }

    #[test]
    fn test_compact_merges_completed_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();

        // Create sessions/sessions.jsonl with session_end for ses-done.
        setup_sessions_with_end(ledger_dir, "ses-done");

        // Create work-graph with base + fragment.
        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();

        let base = wg_dir.join("work-graph.jsonl");
        write_line(
            &base,
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-old"}"#,
        );

        let frag = wg_dir.join("work-graph-ses-done.jsonl");
        write_line(
            &frag,
            r#"{"event":"task_status_changed","timestamp":"2026-01-01T00:30:00Z","session_id":"ses-done"}"#,
        );

        let result = compact_ledger_type(ledger_dir, "work-graph", None).unwrap();
        assert_eq!(result.merged_count, 1);
        assert_eq!(result.deleted_files.len(), 1);

        // Fragment should be deleted.
        assert!(!frag.exists());

        // Base should contain both events, sorted.
        let content = fs::read_to_string(&base).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("task_created"));
        assert!(lines[1].contains("task_status_changed"));
    }

    #[test]
    fn test_compact_skips_active_session() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();

        // No session_end for ses-active.
        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();

        let frag = wg_dir.join("work-graph-ses-active.jsonl");
        write_line(
            &frag,
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-active"}"#,
        );

        let result = compact_ledger_type(ledger_dir, "work-graph", None).unwrap();
        assert_eq!(result.merged_count, 0);
        assert_eq!(result.skipped_active, 1);

        // Fragment should still exist.
        assert!(frag.exists());
    }

    #[test]
    fn test_compact_skips_current_session() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();

        // Even though ses-current has session_end, skip because it's current.
        setup_sessions_with_end(ledger_dir, "ses-current");

        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();

        let frag = wg_dir.join("work-graph-ses-current.jsonl");
        write_line(
            &frag,
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-current"}"#,
        );

        let result = compact_ledger_type(ledger_dir, "work-graph", Some("ses-current")).unwrap();
        assert_eq!(result.merged_count, 0);
        assert_eq!(result.skipped_active, 1);
        assert!(frag.exists());
    }

    #[test]
    fn test_compact_all() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();

        setup_sessions_with_end(ledger_dir, "ses-done");

        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();
        write_line(
            &wg_dir.join("work-graph-ses-done.jsonl"),
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-done"}"#,
        );

        let results = compact_all(ledger_dir, None).unwrap();
        let total_merged: usize = results.iter().map(|r| r.merged_count).sum();
        assert!(total_merged >= 1);
    }

    #[test]
    fn test_compact_no_base_file() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();

        setup_sessions_with_end(ledger_dir, "ses-done");

        // Fragment but no base file.
        let cfg_dir = ledger_dir.join("config");
        fs::create_dir_all(&cfg_dir).unwrap();
        write_line(
            &cfg_dir.join("config-ses-done.jsonl"),
            r#"{"event":"config_set","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-done"}"#,
        );

        let result = compact_ledger_type(ledger_dir, "config", None).unwrap();
        assert_eq!(result.merged_count, 1);

        // Base should now exist with the merged event.
        let base = cfg_dir.join("config.jsonl");
        assert!(base.exists());
        let content = fs::read_to_string(&base).unwrap();
        assert!(content.contains("config_set"));
    }

    // codex round-3: a corrupt/partial line in a source must ABORT compaction —
    // never merge-and-delete it (that would be permanent ledger data loss).
    #[test]
    fn test_corrupt_fragment_aborts_without_data_loss() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();
        setup_sessions_with_end(ledger_dir, "ses-done");
        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();
        let base = wg_dir.join("work-graph.jsonl");
        write_line(
            &base,
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-old"}"#,
        );
        let frag = wg_dir.join("work-graph-ses-done.jsonl");
        write_line(
            &frag,
            r#"{"event":"task_status_changed","timestamp":"2026-01-01T00:30:00Z","session_id":"ses-done"}"#,
        );
        write_line(&frag, "{ this is not valid json");

        let err = compact_ledger_type(ledger_dir, "work-graph", None).unwrap_err();
        assert!(matches!(err, LedgerError::Corrupt(_)), "got {err:?}");
        assert!(frag.exists(), "corrupt fragment must not be deleted");
        assert_eq!(
            fs::read_to_string(&base).unwrap().lines().count(),
            1,
            "base must be untouched"
        );
    }

    // codex round-3: while a writer holds a file's lock, compaction defers rather
    // than reading a base it is about to overwrite (which would lose the append).
    #[test]
    fn test_write_in_progress_defers_compaction() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path();
        setup_sessions_with_end(ledger_dir, "ses-done");
        let wg_dir = ledger_dir.join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();
        let frag = wg_dir.join("work-graph-ses-done.jsonl");
        write_line(
            &frag,
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-done"}"#,
        );

        // A writer holds the fragment's lock.
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(frag.with_extension("jsonl.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();

        let result = compact_ledger_type(ledger_dir, "work-graph", None).unwrap();
        assert_eq!(result.merged_count, 0, "compaction defers under a held lock");
        assert!(frag.exists(), "fragment untouched while deferred");
        drop(lock);
    }
}
