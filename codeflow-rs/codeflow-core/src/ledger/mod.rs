mod jsonl;
mod routing;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::LedgerError;

pub use jsonl::JsonlWriter;
pub use routing::route_event_type;

/// Canonical JSONL ledger file names.
pub mod files {
    pub const WORK_GRAPH: &str = "work-graph.jsonl";
    pub const MEMORY_EVENTS: &str = "memory-events.jsonl";
    pub const SESSIONS: &str = "sessions.jsonl";
    pub const CONFIG: &str = "config.jsonl";
    pub const PATHFLOW_EVENTS: &str = "pathflow-events.jsonl";

    /// Files synced to the database (excludes pathflow-events).
    pub const CANONICAL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];
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
    fn dir(&self) -> &std::path::Path;
}
