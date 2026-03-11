//! Date-rotated JSONL activity writer with flock-based file locking.
//!
//! Distinct from [`crate::ledger::JsonlWriter`] which handles Tier 0
//! `WorkGraph` events with schema validation. `ActivityWriter` handles
//! operational activity logs written to `.state/logs/sessions/`.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

/// Appends structured log entries to date-rotated JSONL files with flock-based
/// file locking.
pub struct ActivityWriter {
    /// Absolute log directory path.
    dir: PathBuf,
    /// Clock function for deterministic testing.
    now: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl ActivityWriter {
    /// Create an `ActivityWriter` for the given project and log directories.
    ///
    /// Resolves the log directory (relative paths joined with `project_dir`)
    /// and creates the directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the directory cannot be created.
    pub fn new(project_dir: &Path, log_dir: &str) -> Result<Self, std::io::Error> {
        let abs_dir = super::resolve_log_dir(project_dir, log_dir);
        std::fs::create_dir_all(&abs_dir)?;
        Ok(Self {
            dir: abs_dir,
            now: Box::new(Utc::now),
        })
    }

    /// Create an `ActivityWriter` with a custom clock (for testing).
    #[cfg(test)]
    pub fn with_clock(
        dir: PathBuf,
        clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static,
    ) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            now: Box::new(clock),
        })
    }

    /// Append a log record to a date-rotated JSONL file.
    ///
    /// The file name is `{log_type}-{YYYY-MM-DD}.jsonl`.
    /// Uses `fs2::FileExt::lock_exclusive` for flock-based locking.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` on serialization, lock, or write failure.
    pub fn append(
        &self,
        log_type: &str,
        record: &HashMap<String, serde_json::Value>,
    ) -> Result<(), std::io::Error> {
        let now = (self.now)();
        let date_str = now.format("%Y-%m-%d").to_string();
        let filename = format!("{log_type}-{date_str}.jsonl");
        let file_path = self.dir.join(&filename);
        let lock_path = self.dir.join(format!("{filename}.lock"));

        let mut data = serde_json::to_vec(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        data.push(b'\n');

        // Acquire exclusive lock.
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        fs2::FileExt::lock_exclusive(&lock_file)?;

        // Write data.
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        f.write_all(&data)?;

        // Release lock (explicit unlock, also released on drop).
        let _ = fs2::FileExt::unlock(&lock_file);

        Ok(())
    }

    /// Return the log directory path.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Return a formatted timestamp string matching shell hook convention.
    ///
    /// Format: `YYYY-MM-DDTHH:MM:SS.sssZ` (ms precision, literal Z).
    #[must_use]
    pub fn timestamp(&self) -> String {
        (self.now)().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_clock() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 10, 14, 30, 0).unwrap()
    }

    #[test]
    fn test_activity_writer_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        let writer = ActivityWriter::with_clock(log_dir.clone(), fixed_clock).unwrap();
        assert!(log_dir.exists());
        assert_eq!(writer.dir(), log_dir);
    }

    #[test]
    fn test_append_creates_dated_file() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::with_clock(dir.path().to_path_buf(), fixed_clock).unwrap();

        let mut record = HashMap::new();
        record.insert("event".into(), serde_json::json!("test_event"));
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));

        writer.append("test", &record).unwrap();

        let file_path = dir.path().join("test-2026-03-10.jsonl");
        assert!(file_path.exists(), "dated JSONL file should exist");

        let content = std::fs::read_to_string(file_path).unwrap();
        assert!(content.contains("test_event"));
    }

    #[test]
    fn test_append_multiple_records() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::with_clock(dir.path().to_path_buf(), fixed_clock).unwrap();

        for i in 0..3 {
            let mut record = HashMap::new();
            record.insert("n".into(), serde_json::json!(i));
            writer.append("multi", &record).unwrap();
        }

        let file_path = dir.path().join("multi-2026-03-10.jsonl");
        let content = std::fs::read_to_string(file_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_timestamp_format() {
        let writer =
            ActivityWriter::with_clock(tempfile::tempdir().unwrap().keep(), fixed_clock).unwrap();
        let ts = writer.timestamp();
        assert_eq!(ts, "2026-03-10T14:30:00.000Z");
    }

    #[test]
    fn test_lock_file_created() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::with_clock(dir.path().to_path_buf(), fixed_clock).unwrap();

        let mut record = HashMap::new();
        record.insert("event".into(), serde_json::json!("lock_test"));
        writer.append("lock", &record).unwrap();

        let lock_path = dir.path().join("lock-2026-03-10.jsonl.lock");
        assert!(lock_path.exists(), "lock file should exist");
    }

    #[test]
    fn test_different_log_types_different_files() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::with_clock(dir.path().to_path_buf(), fixed_clock).unwrap();

        let mut r1 = HashMap::new();
        r1.insert("type".into(), serde_json::json!("session"));
        writer.append("session", &r1).unwrap();

        let mut r2 = HashMap::new();
        r2.insert("type".into(), serde_json::json!("tool-use"));
        writer.append("tool-use", &r2).unwrap();

        assert!(dir.path().join("session-2026-03-10.jsonl").exists());
        assert!(dir.path().join("tool-use-2026-03-10.jsonl").exists());
    }

    #[test]
    fn test_each_line_is_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::with_clock(dir.path().to_path_buf(), fixed_clock).unwrap();

        for i in 0..3 {
            let mut record = HashMap::new();
            record.insert("n".into(), serde_json::json!(i));
            writer.append("json", &record).unwrap();
        }

        let content = std::fs::read_to_string(dir.path().join("json-2026-03-10.jsonl")).unwrap();
        for line in content.trim().lines() {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.is_object());
        }
    }
}
