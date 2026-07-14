//! Append-only JSONL event ledger (charter §3.2, D17).
//!
//! Markdown + JSONL are the source of truth in v2; any index built over the
//! ledger is a rebuildable cache. Layout under a state directory:
//!
//! ```text
//! <state_dir>/ledger/
//! ├── work-graph/
//! │   ├── work-graph.jsonl              ← base (compacted history)
//! │   └── work-graph-ses-{id}.jsonl     ← session fragment
//! ├── sessions/
//! ├── memory-events/
//! └── config/
//! ```
//!
//! v2 trim: pathflow-events, coordination-events, and autorun-events ledger
//! types died with their subsystems (charter §3.2 "never imported").

pub mod compact;
mod jsonl;
pub mod rebuild;
mod routing;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use jsonl::JsonlWriter;
pub use routing::route_event_type;

/// Ledger operation errors.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("unknown ledger type: {0}")]
    UnknownType(String),

    #[error("misrouted event: {event_type} belongs to {expected}, not {actual}")]
    MisroutedEvent {
        event_type: String,
        expected: String,
        actual: String,
    },

    #[error("lock acquisition failed: {0}")]
    Lock(String),

    #[error("corrupt ledger data: {0}")]
    Corrupt(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Canonical JSONL ledger type names (directory names under `ledger/`).
///
/// Within each subdirectory, the base file is `{type}.jsonl` and session
/// fragments are `{type}-ses-{session_id}.jsonl`.
pub mod files {
    pub const WORK_GRAPH: &str = "work-graph";
    pub const MEMORY_EVENTS: &str = "memory-events";
    pub const SESSIONS: &str = "sessions";
    pub const CONFIG: &str = "config";

    /// All canonical ledger type names.
    pub const ALL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];
}

/// Resolve the canonical base file path for a ledger type under an explicit
/// state directory.
///
/// Returns `<state_dir>/ledger/<type_name>/<type_name>.jsonl`. This is the
/// single authoritative path resolver for ledger consumers; callers must use
/// it instead of constructing paths from string literals.
///
/// # Errors
///
/// Returns [`LedgerError::UnknownType`] when `type_name` is not listed in
/// [`files::ALL`].
pub fn resolve_path_in(state_dir: &Path, type_name: &str) -> Result<PathBuf, LedgerError> {
    if !files::ALL.contains(&type_name) {
        return Err(LedgerError::UnknownType(type_name.to_string()));
    }
    Ok(state_dir
        .join("ledger")
        .join(type_name)
        .join(format!("{type_name}.jsonl")))
}

/// Result of a compaction operation on a single ledger type.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The ledger type that was compacted (e.g., "work-graph").
    pub type_name: String,
    /// Number of fragment files merged into the base.
    pub merged_count: usize,
    /// Paths of fragment files that were deleted after merging.
    pub deleted_files: Vec<PathBuf>,
    /// Number of active sessions skipped (no `session_end` found).
    pub skipped_active: usize,
}

/// Check if a filename has a `.jsonl` extension (case-insensitive).
#[must_use]
pub fn is_jsonl_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
}

/// Check if a filename has a `.lock` extension (case-insensitive).
#[must_use]
pub fn is_lock_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
}

/// A single JSONL ledger event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Canonical event type (e.g., `session_start`, `task_created`).
    #[serde(rename = "event")]
    pub event_type: String,

    /// RFC 3339 timestamp.
    pub timestamp: String,

    /// Session that produced this event (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Worktree path that produced this event (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub worktree: Option<String>,

    /// Event-specific key-value pairs, flattened into the top-level object.
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

/// `LedgerWriter` abstracts append-only JSONL event logging.
///
/// All methods are synchronous -- JSONL appends are local file I/O.
pub trait LedgerWriter: Send + Sync {
    /// Validate the event schema, route to the correct file, and append atomically.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` on I/O failure, serialization error, or unknown event type.
    fn append_event(&self, event: Event) -> Result<(), LedgerError>;

    /// Validate and append to a specific file (bypasses routing).
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::MisroutedEvent` if the event type does not belong
    /// to the target file. Returns I/O or serialization errors on write failure.
    fn append_event_to_file(&self, target_file: &str, event: Event) -> Result<(), LedgerError>;

    /// Route an event type to its canonical ledger file.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::UnknownEventType` if the event type is not recognized.
    fn route_event(&self, event_type: &str) -> Result<String, LedgerError>;

    /// Return the ledger directory path.
    fn dir(&self) -> &Path;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn make_event(event_type: &str, session_id: Option<&str>) -> Event {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::json!("value"));
        Event {
            event_type: event_type.to_string(),
            timestamp: "2026-03-07T12:00:00Z".to_string(),
            session_id: session_id.map(String::from),
            worktree: None,
            data,
        }
    }

    #[test]
    fn test_event_serde_roundtrip() {
        let event = make_event("task_created", Some("ses-123"));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["event"], "task_created");
        assert_eq!(parsed["timestamp"], "2026-03-07T12:00:00Z");
        assert_eq!(parsed["session_id"], "ses-123");
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_event_no_session_id_omits_field() {
        let event = make_event("epic_created", None);
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // skip_serializing_if = Option::is_none means session_id absent
        assert!(
            parsed.get("session_id").is_none(),
            "session_id should be absent when None, but got: {json}",
        );
    }

    #[test]
    fn test_event_rename_event_field() {
        // event_type field serializes as "event"
        let event = make_event("begin_work", None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"begin_work""#));
        assert!(!json.contains("event_type"));
    }

    #[test]
    fn test_event_flatten_data() {
        let mut data = HashMap::new();
        data.insert("format_id".to_string(), serde_json::json!("TSK-022-019"));
        data.insert("status".to_string(), serde_json::json!("todo"));
        let event = Event {
            event_type: "task_created".to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: None,
            worktree: None,
            data,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Flattened fields appear at top level
        assert_eq!(parsed["format_id"], "TSK-022-019");
        assert_eq!(parsed["status"], "todo");
        assert_eq!(parsed["event"], "task_created");
    }

    #[test]
    fn test_event_deserialize_from_jsonl_line() {
        let line = r#"{"event":"session_start","timestamp":"2026-03-07T00:00:00Z","session_id":"ses-abc","source":"startup"}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        assert_eq!(event.event_type, "session_start");
        assert_eq!(event.timestamp, "2026-03-07T00:00:00Z");
        assert_eq!(event.session_id.as_deref(), Some("ses-abc"));
        assert_eq!(event.data["source"], "startup");
    }

    #[test]
    fn test_ledger_files_constants() {
        assert_eq!(files::WORK_GRAPH, "work-graph");
        assert_eq!(files::MEMORY_EVENTS, "memory-events");
        assert_eq!(files::SESSIONS, "sessions");
        assert_eq!(files::CONFIG, "config");
        assert_eq!(files::ALL.len(), 4);
    }

    // -- resolve_path_in --

    #[test]
    fn test_resolve_path_in_known_work_graph() {
        let state_dir = PathBuf::from("/tmp/test-state");
        let p = resolve_path_in(&state_dir, files::WORK_GRAPH).unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/test-state/ledger/work-graph/work-graph.jsonl")
        );
    }

    #[test]
    fn test_resolve_path_in_all_canonical_types() {
        // Every entry in files::ALL must be accepted.
        let state_dir = PathBuf::from("/s");
        for ty in files::ALL {
            let p = resolve_path_in(&state_dir, ty).expect("every files::ALL entry must resolve");
            assert_eq!(p, PathBuf::from(format!("/s/ledger/{ty}/{ty}.jsonl")));
        }
    }

    #[test]
    fn test_resolve_path_in_unknown_type_returns_err() {
        let state_dir = PathBuf::from("/tmp/test-state");
        let err = resolve_path_in(&state_dir, "unknown-type").unwrap_err();
        match err {
            LedgerError::UnknownType(name) => assert_eq!(name, "unknown-type"),
            other => panic!("expected UnknownType, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_path_in_dropped_v1_types_rejected() {
        // The v1 pathflow/coordination/autorun ledger types must NOT resolve.
        let state_dir = PathBuf::from("/s");
        for ty in ["pathflow-events", "coordination-events", "autorun-events"] {
            assert!(
                resolve_path_in(&state_dir, ty).is_err(),
                "dropped v1 ledger type '{ty}' must not resolve"
            );
        }
    }

    #[test]
    fn test_event_serialization_with_worktree() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::json!("value"));
        let event = Event {
            event_type: "session_start".to_string(),
            timestamp: "2026-03-07T12:00:00Z".to_string(),
            session_id: Some("ses-123".to_string()),
            worktree: Some("/tmp/worktree/abc".to_string()),
            data,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["worktree"], "/tmp/worktree/abc");
        assert_eq!(parsed["session_id"], "ses-123");
    }

    #[test]
    fn test_event_deserialization_without_worktree() {
        // Events without a worktree field should deserialize with worktree = None
        let line = r#"{"event":"session_start","timestamp":"2026-03-07T00:00:00Z","session_id":"ses-old"}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        assert!(event.worktree.is_none());
        assert_eq!(event.event_type, "session_start");
        assert_eq!(event.session_id.as_deref(), Some("ses-old"));
    }

    #[test]
    fn test_event_worktree_skip_serializing_none() {
        let event = make_event("session_start", Some("ses-1"));
        // worktree is None in make_event
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // skip_serializing_if = Option::is_none means worktree is absent
        assert!(
            parsed.get("worktree").is_none(),
            "worktree should be absent when None, but got: {json}",
        );
    }

    #[test]
    fn test_is_jsonl_file() {
        assert!(is_jsonl_file("work-graph.jsonl"));
        assert!(is_jsonl_file("WORK-GRAPH.JSONL"));
        assert!(!is_jsonl_file("work-graph.jsonl.lock"));
        assert!(!is_jsonl_file("work-graph"));
    }

    #[test]
    fn test_is_lock_file() {
        assert!(is_lock_file("work-graph.jsonl.lock"));
        assert!(!is_lock_file("work-graph.jsonl"));
    }
}
