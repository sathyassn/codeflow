use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::routing::route_event_type;
use super::{Event, LedgerError, LedgerWriter};

/// JSONL-based `LedgerWriter` implementation.
///
/// Appends events as single-line JSON to the appropriate ledger file,
/// using `flock`-based exclusive file locking via the `fs2` crate for
/// concurrent-writer safety. Files are opened with `O_APPEND` for atomic
/// append semantics.
///
/// Each ledger type gets its own directory:
/// ```text
/// <ledger_dir>/
/// ├── work-graph/
/// │   ├── work-graph.jsonl                  ← base (compacted history)
/// │   └── work-graph-ses-{id}.jsonl         ← session fragment
/// ├── sessions/
/// │   ├── sessions.jsonl
/// │   └── sessions-ses-{id}.jsonl
/// └── ...
/// ```
///
/// When `session_id` is `Some`, writes go to the session fragment file.
/// When `session_id` is `None`, writes go to the base file.
pub struct JsonlWriter {
    ledger_dir: PathBuf,
    session_id: Option<String>,
}

impl JsonlWriter {
    /// Create a new `JsonlWriter` that writes to `ledger_dir`.
    ///
    /// Writes go to base files (`{type}/{type}.jsonl`). Use
    /// [`new_with_session`](Self::new_with_session) for session-scoped writes.
    ///
    /// The directory is created (including parents) if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::Io` if the directory cannot be created.
    pub fn new(ledger_dir: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        Self::new_with_session(ledger_dir, None)
    }

    /// Create a new `JsonlWriter` with an optional session ID.
    ///
    /// When `session_id` is `Some`, events are written to session-scoped
    /// fragment files (`{type}/{type}-ses-{id}.jsonl`). When `None`, events
    /// are written to the base file (`{type}/{type}.jsonl`).
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::Io` if the directory cannot be created.
    pub fn new_with_session(
        ledger_dir: impl Into<PathBuf>,
        session_id: Option<String>,
    ) -> Result<Self, LedgerError> {
        let ledger_dir = ledger_dir.into();
        fs::create_dir_all(&ledger_dir)?;
        Ok(Self {
            ledger_dir,
            session_id,
        })
    }

    /// Resolve the file path for a given ledger type name.
    ///
    /// In subdirectory layout:
    /// - Base: `{ledger_dir}/{type_name}/{type_name}.jsonl`
    /// - Session: `{ledger_dir}/{type_name}/{type_name}-ses-{id}.jsonl`
    fn session_scoped_path(&self, type_name: &str) -> PathBuf {
        let subdir = self.ledger_dir.join(type_name);
        match &self.session_id {
            Some(sid) => subdir.join(format!("{type_name}-{sid}.jsonl")),
            None => subdir.join(format!("{type_name}.jsonl")),
        }
    }

    /// Serialize the event to a single JSON line and append it to the
    /// appropriate file, holding an exclusive flock for the duration
    /// of the write.
    fn append_to_file(&self, type_name: &str, event: &Event) -> Result<(), LedgerError> {
        let file_path = self.session_scoped_path(type_name);

        // Ensure the subdirectory exists.
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let lock_path = file_path.with_extension("jsonl.lock");

        // Open (or create) the lock file.
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        // Acquire exclusive lock.
        lock_file.lock_exclusive().map_err(|e| {
            LedgerError::Lock(format!("acquiring lock on {}: {e}", lock_path.display()))
        })?;

        // Serialize event to a single JSON line.
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');

        // Open the data file with O_APPEND | O_CREATE | O_WRONLY.
        let mut data_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file_path)?;

        // Write the full line in a single call.
        data_file.write_all(&line)?;

        // Lock is released on drop of lock_file.
        drop(lock_file);

        Ok(())
    }
}

impl LedgerWriter for JsonlWriter {
    fn append_event(&self, event: Event) -> Result<(), LedgerError> {
        let target = route_event_type(&event.event_type)?;
        self.append_to_file(target, &event)
    }

    fn append_event_to_file(&self, target_file: &str, event: Event) -> Result<(), LedgerError> {
        // Verify routing: the event must belong to the specified file.
        let expected = route_event_type(&event.event_type)?;
        if expected != target_file {
            return Err(LedgerError::MisroutedEvent {
                event_type: event.event_type.clone(),
                expected: expected.to_string(),
                actual: target_file.to_string(),
            });
        }
        self.append_to_file(target_file, &event)
    }

    fn route_event(&self, event_type: &str) -> Result<String, LedgerError> {
        route_event_type(event_type).map(String::from)
    }

    fn dir(&self) -> &Path {
        &self.ledger_dir
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::thread;

    use super::*;
    use crate::ledger::files;

    /// Helper: create a simple Event for testing.
    fn make_event(event_type: &str) -> Event {
        Event {
            event_type: event_type.to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: Some("ses-test-001".to_string()),
            worktree: None,
            data: HashMap::new(),
        }
    }

    /// Helper: create an Event with extra data fields.
    fn make_event_with_data(event_type: &str, data: HashMap<String, serde_json::Value>) -> Event {
        Event {
            event_type: event_type.to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: Some("ses-test-002".to_string()),
            worktree: None,
            data,
        }
    }

    /// Helper: resolve base file path in subdirectory layout.
    fn base_path(dir: &std::path::Path, type_name: &str) -> std::path::PathBuf {
        dir.join(type_name).join(format!("{type_name}.jsonl"))
    }

    /// Helper: resolve session fragment path in subdirectory layout.
    fn session_path(dir: &std::path::Path, type_name: &str, sid: &str) -> std::path::PathBuf {
        dir.join(type_name).join(format!("{type_name}-{sid}.jsonl"))
    }

    #[test]
    fn test_append_event_writes_to_correct_file() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let event = make_event("session_start");
        writer.append_event(event).unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::SESSIONS)).unwrap();
        assert!(content.contains("\"event\":\"session_start\""));
        assert!(content.contains("\"session_id\":\"ses-test-001\""));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn test_append_event_to_file_correct_routing() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let event = make_event("task_created");
        writer
            .append_event_to_file(files::WORK_GRAPH, event)
            .unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::WORK_GRAPH)).unwrap();
        assert!(content.contains("\"event\":\"task_created\""));
    }

    #[test]
    fn test_append_event_to_file_misrouted_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        // session_start belongs to sessions, not work-graph
        let event = make_event("session_start");
        let result = writer.append_event_to_file(files::WORK_GRAPH, event);

        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("misrouted"), "expected MisroutedEvent: {msg}");
        assert!(msg.contains("session_start"));
        assert!(msg.contains(files::SESSIONS));
        assert!(msg.contains(files::WORK_GRAPH));
    }

    #[test]
    fn test_unknown_event_type_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let event = make_event("nonexistent_event");
        let result = writer.append_event(event);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nonexistent_event"),
            "error should reference the unknown type"
        );
    }

    #[test]
    fn test_empty_event_type_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let event = make_event("");
        let result = writer.append_event(event);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_locking_does_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        // Two sequential appends to the same file should not deadlock.
        writer.append_event(make_event("session_start")).unwrap();
        writer.append_event(make_event("session_end")).unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::SESSIONS)).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "both events should be appended");
        assert!(lines[0].contains("session_start"));
        assert!(lines[1].contains("session_end"));
    }

    #[test]
    fn test_routing_returns_correct_types() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        assert_eq!(
            writer.route_event("session_start").unwrap(),
            files::SESSIONS
        );
        assert_eq!(
            writer.route_event("task_created").unwrap(),
            files::WORK_GRAPH
        );
        assert_eq!(
            writer.route_event("decision").unwrap(),
            files::MEMORY_EVENTS
        );
        assert_eq!(writer.route_event("config_set").unwrap(), files::CONFIG);
    }

    #[test]
    fn test_serialization_format_is_single_line_json() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let mut data = HashMap::new();
        data.insert(
            "format_id".to_string(),
            serde_json::Value::String("TSK-001-001".to_string()),
        );
        let event = make_event_with_data("task_created", data);
        writer.append_event(event).unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::WORK_GRAPH)).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "should be exactly one line");

        // Verify the line is valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["event"], "task_created");
        assert_eq!(parsed["format_id"], "TSK-001-001");
        assert_eq!(parsed["timestamp"], "2026-03-07T00:00:00Z");
    }

    #[test]
    fn test_concurrent_append_safety() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let path = dir_path.clone();
                thread::spawn(move || {
                    let writer = JsonlWriter::new(&path).unwrap();
                    let mut data = HashMap::new();
                    data.insert("index".to_string(), serde_json::json!(i));
                    let event = Event {
                        event_type: "task_created".to_string(),
                        timestamp: format!("2026-03-07T00:00:{i:02}Z"),
                        session_id: None,
                        worktree: None,
                        data,
                    };
                    writer.append_event(event).unwrap();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let content = fs::read_to_string(base_path(&dir_path, files::WORK_GRAPH)).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 10, "all 10 concurrent writes should succeed");

        // Each line should be valid JSON.
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(parsed["event"], "task_created");
        }
    }

    #[test]
    fn test_multiple_event_types_to_different_files() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        writer.append_event(make_event("session_start")).unwrap();
        writer.append_event(make_event("task_created")).unwrap();
        writer.append_event(make_event("decision")).unwrap();
        writer.append_event(make_event("config_set")).unwrap();

        // Each subdirectory file should have exactly one event.
        assert_eq!(
            fs::read_to_string(base_path(dir.path(), files::SESSIONS))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(base_path(dir.path(), files::WORK_GRAPH))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(base_path(dir.path(), files::MEMORY_EVENTS))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(base_path(dir.path(), files::CONFIG))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn test_session_scoped_writes_to_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let writer =
            JsonlWriter::new_with_session(dir.path(), Some("ses-test-session".to_string()))
                .unwrap();

        let event = make_event("session_start");
        writer.append_event(event).unwrap();

        // Should write to sessions/sessions-ses-test-session.jsonl
        let fragment = session_path(dir.path(), files::SESSIONS, "ses-test-session");
        assert!(fragment.exists(), "fragment file should exist");
        let content = fs::read_to_string(&fragment).unwrap();
        assert!(content.contains("\"event\":\"session_start\""));

        // Base file should NOT exist (no writes to it).
        let base = base_path(dir.path(), files::SESSIONS);
        assert!(!base.exists(), "base file should not be created");
    }

    #[test]
    fn test_session_scoped_and_base_are_separate() {
        let dir = tempfile::tempdir().unwrap();

        // Write to base.
        let base_writer = JsonlWriter::new(dir.path()).unwrap();
        base_writer
            .append_event(make_event("task_created"))
            .unwrap();

        // Write to session fragment.
        let session_writer =
            JsonlWriter::new_with_session(dir.path(), Some("ses-abc".to_string())).unwrap();
        session_writer
            .append_event(make_event("task_created"))
            .unwrap();

        let base_content = fs::read_to_string(base_path(dir.path(), files::WORK_GRAPH)).unwrap();
        let frag_content =
            fs::read_to_string(session_path(dir.path(), files::WORK_GRAPH, "ses-abc")).unwrap();

        assert_eq!(base_content.lines().count(), 1);
        assert_eq!(frag_content.lines().count(), 1);
    }

    #[test]
    fn test_dir_returns_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();
        assert_eq!(writer.dir(), dir.path());
    }

    #[test]
    fn test_new_creates_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested").join("ledger");
        assert!(!nested.exists());

        let writer = JsonlWriter::new(&nested).unwrap();
        assert!(nested.exists());
        assert_eq!(writer.dir(), nested.as_path());
    }

    #[test]
    fn test_session_id_omitted_when_none() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let event = Event {
            event_type: "config_set".to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: None,
            worktree: None,
            data: HashMap::new(),
        };
        writer.append_event(event).unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::CONFIG)).unwrap();
        assert!(
            !content.contains("session_id"),
            "session_id should be omitted when None"
        );
    }

    #[test]
    fn test_data_fields_flattened_into_top_level() {
        let dir = tempfile::tempdir().unwrap();
        let writer = JsonlWriter::new(dir.path()).unwrap();

        let mut data = HashMap::new();
        data.insert(
            "old_status".to_string(),
            serde_json::Value::String("todo".to_string()),
        );
        data.insert(
            "new_status".to_string(),
            serde_json::Value::String("in_progress".to_string()),
        );

        let event = Event {
            event_type: "task_status_changed".to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: None,
            worktree: None,
            data,
        };
        writer.append_event(event).unwrap();

        let content = fs::read_to_string(base_path(dir.path(), files::WORK_GRAPH)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["old_status"], "todo");
        assert_eq!(parsed["new_status"], "in_progress");
    }

}
