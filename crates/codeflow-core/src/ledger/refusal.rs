//! The refusal event (TSK-149, SPC-013 R-118 ledger row).
//!
//! When a git hook or a session guard stops an operation, it appends one
//! `refusal` event to `<state>/ledger/refusals/refusals.jsonl`, naming the
//! plane, the effective level and the blocking rules. The event carries no
//! command text, message, path or other operation content, so no secret a
//! command held can reach the ledger. A finding printed at `warn` stops
//! nothing and is never written.
//!
//! Each plane also writes one `refusal_recording_started` event the first
//! time it runs with this ledger type absent, so a reader can tell "no
//! refusal since recording began" from "never recorded". The ceremony
//! report reads both.

use std::collections::HashMap;
use std::path::Path;

use super::rebuild::rebuild_ledger_type;
use super::{files, Event, JsonlWriter, LedgerError, LedgerWriter};

/// Event type of one refused operation.
pub const REFUSAL: &str = "refusal";
/// Event type that marks when this clone began recording refusals.
pub const RECORDING_STARTED: &str = "refusal_recording_started";
/// The level a refused operation is written at.
pub const BLOCK: &str = "block";
/// How long a hook or guard waits for another writer's lock on this
/// ledger before it gives up on the record and keeps its verdict.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// Write the recording marker unless the refusals ledger already exists.
/// A second marker from a racing plane is harmless: readers take the
/// earliest.
///
/// # Errors
///
/// Returns the writer's error when the ledger cannot be written.
pub fn mark_recording(ledger_dir: &Path, timestamp: &str) -> Result<(), LedgerError> {
    let base = super::resolve_path_in(state_dir_of(ledger_dir), files::REFUSALS)?;
    if base.exists() {
        return Ok(());
    }
    append(ledger_dir, RECORDING_STARTED, timestamp, HashMap::new())
}

/// Append one refusal: the plane that stopped the operation and the rules
/// that blocked it, at [`BLOCK`]. Nothing else is written.
///
/// # Errors
///
/// Returns the writer's error when the ledger cannot be written.
pub fn record(
    ledger_dir: &Path,
    plane: &str,
    rules: &[&str],
    timestamp: &str,
) -> Result<(), LedgerError> {
    let mut data = HashMap::new();
    data.insert("plane".to_string(), serde_json::json!(plane));
    data.insert("level".to_string(), serde_json::json!(BLOCK));
    data.insert("rules".to_string(), serde_json::json!(rules));
    append(ledger_dir, REFUSAL, timestamp, data)
}

fn append(
    ledger_dir: &Path,
    event_type: &str,
    timestamp: &str,
    data: HashMap<String, serde_json::Value>,
) -> Result<(), LedgerError> {
    JsonlWriter::new(ledger_dir)?
        .with_lock_wait(LOCK_WAIT)
        .append_event(Event {
            event_type: event_type.to_string(),
            timestamp: timestamp.to_string(),
            session_id: None,
            worktree: None,
            data,
        })
}

/// `resolve_path_in` takes the state dir, the parent of `ledger/`.
fn state_dir_of(ledger_dir: &Path) -> &Path {
    ledger_dir.parent().unwrap_or(ledger_dir)
}

/// One recorded refusal, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// RFC 3339 UTC time of the refusal.
    pub timestamp: String,
    /// The plane that stopped the operation (`pre-push`, `git-guard`, ...).
    pub plane: String,
}

/// What the refusals ledger holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    /// When recording began: the earliest marker, or the earliest refusal
    /// when a marker is missing; `None` when nothing was ever recorded.
    pub since: Option<String>,
    /// Every refusal at [`BLOCK`], oldest first. An event at any other
    /// level is not a refusal and is left out.
    pub refusals: Vec<Recorded>,
}

/// Read the refusals ledger under `ledger_dir`.
///
/// # Errors
///
/// Returns the reader's error when the directory cannot be read.
pub fn read(ledger_dir: &Path) -> Result<Log, LedgerError> {
    let events = rebuild_ledger_type(ledger_dir, files::REFUSALS)?;
    let mut log = Log {
        since: events.first().map(|event| event.timestamp.clone()),
        refusals: Vec::new(),
    };
    for event in events {
        let text = |key: &str| {
            event
                .data
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        if event.event_type == REFUSAL && text("level").as_deref() == Some(BLOCK) {
            log.refusals.push(Recorded {
                plane: text("plane").unwrap_or_default(),
                timestamp: event.timestamp.clone(),
            });
        }
    }
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("codeflow").join("ledger");
        (dir, ledger)
    }

    #[test]
    fn a_refusal_names_the_plane_level_and_rules_and_nothing_else() {
        let (_dir, ledger) = ledger();
        record(
            &ledger,
            "pre-push",
            &["git.push_to_protected"],
            "2026-09-28T10:00:00Z",
        )
        .unwrap();
        let line = std::fs::read_to_string(ledger.join("refusals").join("refusals.jsonl")).unwrap();
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["event", "level", "plane", "rules", "timestamp"]);
        assert_eq!(value["event"], "refusal");
        assert_eq!(value["level"], "block");
        assert_eq!(value["rules"], serde_json::json!(["git.push_to_protected"]));
    }

    #[test]
    fn the_marker_is_written_once_and_dates_the_log() {
        let (_dir, ledger) = ledger();
        assert_eq!(read(&ledger).unwrap(), Log::default());
        mark_recording(&ledger, "2026-09-28T09:00:00Z").unwrap();
        mark_recording(&ledger, "2026-09-28T09:30:00Z").unwrap();
        record(
            &ledger,
            "git-guard",
            &["git.commit_to_protected"],
            "2026-09-28T10:00:00Z",
        )
        .unwrap();
        let log = read(&ledger).unwrap();
        assert_eq!(log.since.as_deref(), Some("2026-09-28T09:00:00Z"));
        assert_eq!(
            log.refusals,
            [Recorded {
                timestamp: "2026-09-28T10:00:00Z".to_string(),
                plane: "git-guard".to_string(),
            }]
        );
        let text = std::fs::read_to_string(ledger.join("refusals").join("refusals.jsonl")).unwrap();
        assert_eq!(text.matches(RECORDING_STARTED).count(), 1, "{text}");
    }

    #[test]
    fn a_held_lock_bounds_the_marker_and_the_refusal() {
        use fs2::FileExt;
        let (_dir, ledger) = ledger();
        let lock_path = ledger.join("refusals").join("refusals.jsonl.lock");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let holder = std::fs::File::create(&lock_path).unwrap();
        holder.lock_exclusive().unwrap();
        for attempt in [
            mark_recording(&ledger, "2026-09-28T09:00:00Z"),
            record(
                &ledger,
                "pre-push",
                &["git.push_to_protected"],
                "2026-09-28T10:00:00Z",
            ),
        ] {
            match attempt {
                Err(LedgerError::LockTimeout { path, waited }) => {
                    assert_eq!(path, lock_path);
                    assert_eq!(waited, LOCK_WAIT);
                }
                other => panic!("expected a lock timeout, got {other:?}"),
            }
        }
        holder.unlock().unwrap();
        record(
            &ledger,
            "pre-push",
            &["git.push_to_protected"],
            "2026-09-28T10:00:00Z",
        )
        .unwrap();
        assert_eq!(read(&ledger).unwrap().refusals.len(), 1);
    }

    #[test]
    fn an_event_at_warn_is_not_a_refusal() {
        let (_dir, ledger) = ledger();
        let mut data = HashMap::new();
        data.insert("plane".to_string(), serde_json::json!("pre-push"));
        data.insert("level".to_string(), serde_json::json!("warn"));
        append(&ledger, REFUSAL, "2026-09-28T10:00:00Z", data).unwrap();
        let log = read(&ledger).unwrap();
        assert!(log.refusals.is_empty(), "{log:?}");
        assert_eq!(log.since.as_deref(), Some("2026-09-28T10:00:00Z"));
    }
}
