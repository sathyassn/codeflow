---
title: "Trait Hierarchy and Module Map"
type: analysis
epic: INF-EPC-022
task: INF-TSK-022-002
status: complete
created: "2026-03-07"
author: cf-planning
---

# Trait Hierarchy and Module Map

## 1. Executive Summary

This document defines the three core traits (`DataStore`, `LedgerWriter`, `HookHandler`) for the Rust CLI redesign, maps Go packages to Rust modules with consolidation decisions, and produces the module dependency graph. All trait signatures are precise enough for downstream implementers (tasks 009, 010, 015) to code against directly.

The trait hierarchy follows the architecture diagram in `data-layer-protection.md` Section 6: `codeflow-core` (lib crate) defines traits, domain types, and error enums; `codeflow-cli` (bin crate) provides Clap dispatch.

## 2. Domain Type Enums

These enums are consumed by trait signatures and defined in `codeflow-core::types`. They replace Go's `map[string]bool` validation sets (go-audit.md Section 5.2) with compile-time type safety.

```rust
// codeflow-core/src/types/phase.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum Phase {
    Pf1Init,
    Pf2Context,
    Pf3Classify,
    Pf4Execute,
    Pf5Verify,
    Pf6Complete,
    Pf7End,
}

// codeflow-core/src/types/stage.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum WorkStage {
    WsDev,
    WsRev,
    WsQa,
    WsTest,
    WsPlan,
    WsDocs,
}

// codeflow-core/src/types/work.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AreaType {
    Inf, Pln, Doc, Tst, Sec, Frm, Prj, Aut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkType {
    Feat, Fix, Rfct, Cicd, Docs, Test, Plan, Spke, Htfx, Chor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo, InProgress, Done, Blocked, Cancelled, Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpicStatus {
    Todo, InProgress, Done, Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active, Ended, Crashed,
}
```

## 3. Domain Error Enums

Error types follow `thiserror` conventions per cf-rust-standards. Each domain area has its own error enum. The CLI binary uses `anyhow` for top-level error propagation.

```rust
// codeflow-core/src/error.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("record not found: {table}:{id}")]
    NotFound { table: String, id: String },

    #[error("duplicate record: {table}:{id}")]
    Duplicate { table: String, id: String },

    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(String),

    #[error("schema migration failed: {0}")]
    Migration(String),

    #[error("transaction failed: {0}")]
    Transaction(String),

    #[error("integrity check failed: {0}")]
    IntegrityCheck(String),

    #[error("surrealdb error: {0}")]
    Surreal(#[from] surrealdb::Error),
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("misrouted event: {event_type} belongs to {expected}, not {actual}")]
    MisroutedEvent {
        event_type: String,
        expected: String,
        actual: String,
    },

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("lock acquisition failed: {0}")]
    Lock(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("blocked: {reason}")]
    Blocked { reason: String },

    #[error("sentinel not found: {0}")]
    SentinelNotFound(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("no active session")]
    NoActiveSession,

    #[error("session already ended: {0}")]
    AlreadyEnded(String),

    #[error("invalid session id: {0}")]
    InvalidSessionId(String),

    #[error("database error")]
    Db(#[from] DbError),
}
```

## 4. Core Trait Definitions

### 4.1 DataStore Trait

Maps to: `codeflow-cli/internal/db/` (connection.go, queries.go, models.go, transactions.go)

The `DataStore` trait abstracts all database CRUD operations. The sole implementation is `SurrealStore` (embedded `surrealkv://`). The trait exists for testability (mock implementations in tests) and future extensibility (`ws+unix://` daemon mode in Epic C).

**Design decision: async trait methods with static dispatch.** SurrealDB operations are inherently async (`Surreal::query().await`). The trait uses `async fn` (stable in Rust 1.75+) with `Send + Sync` bounds. However, `async fn` in traits does NOT make returned futures `Send` automatically, and the trait is NOT object-safe (no `dyn DataStore`). Therefore, all consumers use **static dispatch** via generics (`impl DataStore` or `<S: DataStore>`), never `Box<dyn DataStore>`. This is correct because there is only one production impl (`SurrealStore`) -- dynamic dispatch would add overhead with zero benefit. The sole impl's futures are `Send` by construction (SurrealDB's `Surreal` is `Send + Sync`), so `tokio::spawn` works when the concrete type is known.

**Design decision: typed queries over raw SQL.** The Go codebase uses `QueryToJSON` and `QueryToMaps` which return `map[string]any` (go-audit.md Section 5.1). The Rust trait replaces these with typed methods that return domain structs, eliminating all untyped map accessor helpers (go-audit.md Section 3.2).

```rust
// codeflow-core/src/store.rs

use crate::error::DbError;
use crate::types::*;
use crate::models::*;

/// DataStore abstracts all persistent storage operations.
///
/// The sole production implementation is `SurrealStore` (surrealkv:// embedded).
/// Test implementations use `SurrealStore` backed by `Surreal<Mem>` (in-memory).
///
/// **Async fn Send constraint:** `async fn` in traits does not guarantee Send
/// futures in stable Rust. Callers MUST use static dispatch (`impl DataStore`
/// or `<S: DataStore>`) -- dynamic dispatch (`dyn DataStore`) is not supported.
/// The compiler verifies Send-ness at each call site through the concrete impl.
///
/// All methods take `&self` -- implementations handle interior mutability
/// via SurrealDB's internally reference-counted `Surreal` instance.
pub trait DataStore: Send + Sync {
    // -- Schema lifecycle --

    /// Apply the schema (idempotent DEFINE statements). Safe to call on every startup.
    async fn apply_schema(&self) -> Result<(), DbError>;

    /// Check database integrity (SurrealDB equivalent of PRAGMA integrity_check).
    async fn check_integrity(&self) -> Result<(), DbError>;

    // -- Session CRUD --

    async fn create_session(&self, session: &Session) -> Result<(), DbError>;
    async fn get_session(&self, id: &str) -> Result<Option<Session>, DbError>;
    async fn update_session(&self, id: &str, update: SessionUpdate) -> Result<(), DbError>;
    async fn list_sessions(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<Session>, DbError>;

    // -- Epic CRUD --

    async fn create_epic(&self, epic: &Epic) -> Result<(), DbError>;
    async fn get_epic(&self, id: &str) -> Result<Option<Epic>, DbError>;
    async fn get_epic_by_format_id(&self, format_id: &str) -> Result<Option<Epic>, DbError>;
    async fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), DbError>;
    async fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, DbError>;

    // -- Task CRUD --

    async fn create_task(&self, task: &Task) -> Result<(), DbError>;
    async fn get_task(&self, id: &str) -> Result<Option<Task>, DbError>;
    async fn get_task_by_format_id(&self, format_id: &str) -> Result<Option<Task>, DbError>;
    async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), DbError>;
    async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>, DbError>;

    // -- Active work --

    async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError>;
    async fn set_active_work(&self, work: &ActiveWork) -> Result<(), DbError>;
    async fn clear_active_work(&self, id: &str) -> Result<(), DbError>;

    // -- Memory events --

    async fn create_memory_event(&self, event: &MemoryEvent) -> Result<(), DbError>;
    async fn list_memory_events(
        &self,
        filter: MemoryEventFilter,
    ) -> Result<Vec<MemoryEvent>, DbError>;

    // -- Autorun --

    async fn create_autorun_session(
        &self,
        session: &AutorunSession,
    ) -> Result<(), DbError>;
    async fn get_autorun_session(
        &self,
        id: &str,
    ) -> Result<Option<AutorunSession>, DbError>;
    async fn update_autorun_session(
        &self,
        id: &str,
        update: AutorunSessionUpdate,
    ) -> Result<(), DbError>;

    async fn create_autorun_worker(
        &self,
        worker: &AutorunWorker,
    ) -> Result<(), DbError>;
    async fn update_autorun_worker(
        &self,
        id: &str,
        update: AutorunWorkerUpdate,
    ) -> Result<(), DbError>;

    async fn create_autorun_task_run(
        &self,
        run: &AutorunTaskRun,
    ) -> Result<(), DbError>;
    async fn update_autorun_task_run(
        &self,
        id: &str,
        update: AutorunTaskRunUpdate,
    ) -> Result<(), DbError>;

    // -- Generic query (for CLI `codeflow db query` pass-through) --

    /// Execute a read-only query and return results as JSON.
    /// The implementation validates that the query is read-only.
    async fn query_to_json(&self, query: &str) -> Result<serde_json::Value, DbError>;

    // -- Sync (rebuild from JSONL) --

    /// Rebuild database state from JSONL ledger events.
    /// Replaces Go's `db/sync.go` + `ledger/normalize.go` (merged, go-audit.md Section 3.1).
    async fn sync_from_events(
        &self,
        events: impl Iterator<Item = crate::ledger::Event> + Send,
    ) -> Result<SyncResult, DbError>;
}

// Note: Transaction support is intentionally NOT part of the DataStore trait.
// Generic transactions (`<F, R>`) make the trait non-object-safe, and async fn
// in traits does not support dynamic dispatch. Since transaction semantics are
// implementation-specific (SurrealDB transactions are the only impl),
// the `SurrealStore` impl provides its own `transaction()` method directly:
//
//   impl SurrealStore {
//       pub async fn transaction<F, R>(&self, f: F) -> Result<R, DbError>
//       where
//           F: FnOnce(&Surreal<Db>) -> Pin<Box<dyn Future<Output = Result<R, DbError>> + Send + '_>>
//               + Send + 'static,
//           R: Send + 'static;
//   }
//
// Callers that need transactions depend on `SurrealStore` directly (not `dyn DataStore`).
// Test code uses in-memory SurrealDB (`Surreal<Mem>`) which supports the same transaction API.

/// Result of a sync operation.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub events_processed: u64,
    pub sessions_upserted: u64,
    pub epics_upserted: u64,
    pub tasks_upserted: u64,
    pub memory_events_inserted: u64,
    pub errors_skipped: u64,
}
```

#### 4.1.1 Models Excluded from DataStore Trait

Two Go models (`User`, `ProjectConfig`) are intentionally excluded from the `DataStore` trait:

| Model | Go Source | Exclusion Rationale |
|-------|-----------|---------------------|
| `ProjectConfig` | `db/models.go:44` (8 fields) | Singleton configuration record. In Go, it has no dedicated CRUD methods on `DB` -- it is populated during schema init and read via raw SQL. In Rust, it is loaded once at startup via `config::load::<ProjectConfig>()` and held in memory. Not managed through CRUD operations. |
| `User` | `db/models.go:114` (11 fields) | User management is deferred to Epic C (daemon mode with multi-user support). In the current single-process embedded mode, the user record is created/updated during `sync_from_events()` as a side effect of session event processing. No explicit CRUD needed until Epic C introduces `ws+unix://` access with authentication. |

Both structs are still defined in `codeflow-core::models` (see Section 4.1.3) for use by `sync_from_events()` and config loading. They simply don't need trait-level CRUD abstraction.

#### 4.1.2 Filter and Update Types

These replace Go's raw SQL WHERE clauses with typed, composable filter structs.

```rust
// codeflow-core/src/models/filters.rs

#[derive(Debug, Default)]
pub struct SessionFilter {
    pub status: Option<SessionStatus>,
    pub limit: Option<u32>,
}

#[derive(Debug, Default)]
pub struct EpicFilter {
    pub status: Option<EpicStatus>,
    pub area_type: Option<AreaType>,
    pub work_type: Option<WorkType>,
    pub is_ongoing: Option<bool>,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub epic_id: Option<String>,
    pub status: Option<TaskStatus>,
    pub area_type: Option<AreaType>,
    pub work_type: Option<WorkType>,
    pub assignee_id: Option<String>,
    pub autorun_eligible: Option<bool>,
}

#[derive(Debug, Default)]
pub struct MemoryEventFilter {
    pub event_type: Option<String>,
    pub domain: Option<String>,
    pub work_id: Option<String>,
    pub limit: Option<u32>,
}
```

Update types use `Option<T>` for partial updates (only set fields that are `Some`):

```rust
// codeflow-core/src/models/updates.rs

#[derive(Debug, Default)]
pub struct SessionUpdate {
    pub status: Option<SessionStatus>,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub context_summary: Option<String>,
    pub work_ids: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct EpicUpdate {
    pub status: Option<EpicStatus>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub pr_number: Option<i64>,
}

#[derive(Debug, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub stage: Option<WorkStage>,
    pub stage_status: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunSessionUpdate {
    pub status: Option<String>,
    pub completed_tasks: Option<i32>,
    pub failed_tasks: Option<i32>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunWorkerUpdate {
    pub status: Option<String>,
    pub tmux_session: Option<String>,
    pub worktree_path: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunTaskRunUpdate {
    pub status: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
}
```

#### 4.1.3 Domain Model Structs

Replace Go `db/models.go` structs. `Option<T>` replaces `sql.NullString` / `sql.NullInt64` (go-audit.md Section 5.3).

```rust
// codeflow-core/src/models/mod.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: Option<String>,
    pub user_id: String,
    pub user_host: String,
    pub machine_fingerprint: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub status: SessionStatus,
    pub work_ids: Vec<String>,
    pub previous_session_id: Option<String>,
    pub context_summary: Option<String>,
    #[serde(default)]
    pub tool_stats: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub id: String,
    pub format_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub is_ongoing: bool,
    pub file_scope: Vec<String>,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub format_id: String,
    pub epic_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub origin: String,
    pub file_scope: Vec<String>,
    pub scope_policy: String,
    pub scope_root: Option<String>,
    pub estimate: Option<String>,
    pub priority: String,
    pub assignee_id: Option<String>,
    pub autorun_eligible: bool,
    pub auto_commit: bool,
    pub raise_pr: bool,
    pub auto_merge: bool,
    pub target_branch: Option<String>,
    pub acceptance: Vec<String>,
    pub tests: Vec<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub stage: Option<WorkStage>,
    pub stage_status: Option<String>,
    pub stage_history: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    pub id: String,
    pub task_id: Option<String>,
    pub topic: String,
    pub status: String,
    pub branch: Option<String>,
    pub scope: Vec<String>,
    pub deliverables: Vec<String>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub current_stage: Option<WorkStage>,
    pub team_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: String,
    pub event_type: String,
    pub domain: String,
    pub work_id: Option<String>,
    pub data: String,
    pub memory_type: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunSession {
    pub id: String,
    pub batch_file: String,
    pub batch_name: Option<String>,
    pub status: String,
    pub max_session_workers: i32,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunWorker {
    pub id: String,
    pub session_id: String,
    pub worker_num: i32,
    pub task_id: String,
    pub status: String,
    pub tmux_session: Option<String>,
    pub worktree_path: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunTaskRun {
    pub id: String,
    pub worker_id: String,
    pub task_id: String,
    pub session_id: String,
    pub status: String,
    pub branch_name: Option<String>,
    pub worktree_path: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
    pub created_at: String,
}

// -- Models excluded from DataStore trait (see Section 4.1.1) --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub git_remote_url: Option<String>,
    pub default_branch: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub git_username: Option<String>,
    pub role: String,
    pub last_host: Option<String>,
    pub last_machine_fingerprint: Option<String>,
    pub first_seen_at: String,
    pub last_active_at: Option<String>,
    #[serde(default)]
    pub preferences: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
```

### 4.2 LedgerWriter Trait

Maps to: `codeflow-cli/internal/ledger/` (writer.go, routing.go, event.go)

The `LedgerWriter` trait abstracts append-only JSONL event logging. The sole implementation is `JsonlWriter` (flock + serde append). The trait exists for testability (in-memory writer for tests).

**Design decision: synchronous trait.** JSONL appends are local file I/O with flock -- not network I/O. The Go implementation is synchronous. Keeping the Rust trait synchronous avoids unnecessary async overhead for a single file write. If needed, callers can wrap in `spawn_blocking`.

**Design decision: single writer, no bypass.** Go's session package bypasses the ledger writer with `writeJSONLEvent()` (go-audit.md Section 3.10). In Rust, all JSONL writes go through `LedgerWriter`, eliminating the bypass.

```rust
// codeflow-core/src/ledger.rs

use crate::error::LedgerError;

/// Canonical JSONL ledger file names.
/// Single source of truth -- replaces duplicate constants in Go's
/// ledger/routing.go and db/sync.go (go-audit.md Section 3.4).
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
///
/// Serialized as a flat JSON object: top-level fields (event, timestamp,
/// session_id) merge with the `data` map. Replaces Go's custom
/// MarshalJSON/UnmarshalJSON (go-audit.md Section 5.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Canonical event type (e.g., "session_start", "task_created").
    #[serde(rename = "event")]
    pub event_type: String,

    /// RFC 3339 timestamp. Auto-generated by the writer if empty.
    pub timestamp: String,

    /// Session that produced this event (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Event-specific key-value pairs, flattened into the top-level object.
    #[serde(flatten)]
    pub data: std::collections::HashMap<String, serde_json::Value>,
}

/// LedgerWriter abstracts append-only JSONL event logging.
///
/// The sole production implementation is `JsonlWriter` (flock + serde append).
/// Test implementations use an in-memory Vec<Event> collector.
///
/// All methods are synchronous -- JSONL appends are local file I/O.
pub trait LedgerWriter: Send + Sync {
    /// Validate the event schema, route to the correct file, and append atomically.
    ///
    /// If `event.timestamp` is empty, a UTC RFC 3339 timestamp is auto-generated.
    /// File locking (flock) ensures concurrent writers do not produce partial writes.
    fn append_event(&self, event: Event) -> Result<(), LedgerError>;

    /// Validate and append to a specific file (bypasses routing).
    /// Returns `LedgerError::MisroutedEvent` if the event type does not belong
    /// to the target file.
    fn append_event_to_file(
        &self,
        target_file: &str,
        event: Event,
    ) -> Result<(), LedgerError>;

    /// Route an event type to its canonical ledger file.
    fn route_event(&self, event_type: &str) -> Result<String, LedgerError>;

    /// Return the ledger directory path.
    /// Uses `&Path` instead of `&str` to support non-UTF-8 filesystem paths.
    fn dir(&self) -> &std::path::Path;
}
```

### 4.3 HookHandler Trait

Maps to: `codeflow-cli/cmd/codeflow/hooks.go` (dispatch) + `codeflow-cli/internal/hooks/` (11 sub-packages)

The `HookHandler` trait unifies all hook enforcement modules behind a single interface. This replaces 8 separate `Verdict` structs and 9 `hookInput` structs (go-audit.md Sections 3.7, 3.8) with unified types.

**Design decision: synchronous trait.** Hook handlers perform local file checks (sentinel files, enforcement policy) and stdin parsing. No network I/O. Keeping synchronous matches Go's approach and avoids async overhead in the hot path (hooks run on every tool call).

**Design decision: HookVerdict enum over struct.** Go uses `Verdict` structs with an `Allow bool` field. Rust idiom uses an enum where the block variant carries the reason. This makes the "forgot to check Allow" bug impossible -- the caller must pattern-match.

```rust
// codeflow-core/src/hooks.rs

use crate::error::HookError;

/// Hook lifecycle events (maps to Claude Code hook events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    TaskCompleted,
    SessionStart,
    SessionEnd,
    Stop,
    UserPromptSubmit,
}

/// Typed input for hook handlers.
///
/// Replaces 9 separate hookInput/HookInput structs across Go hook packages
/// (go-audit.md Section 3.8). Each hook handler receives the same input struct
/// and extracts the fields it needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// The tool name from Claude Code (e.g., "Bash", "Edit", "Write", "Task").
    pub tool_name: String,

    /// The tool input payload as raw JSON.
    /// Each hook handler deserializes the subset of fields it needs.
    pub tool_input: serde_json::Value,

    /// The hook event type.
    pub event: HookEvent,

    /// Session ID (from environment or stdin).
    pub session_id: Option<String>,

    /// Project root directory.
    pub project_dir: Option<String>,
}

/// Result of a hook handler evaluation.
///
/// Replaces 8 separate Verdict structs (go-audit.md Section 3.7).
/// Uses an enum so the caller MUST pattern-match -- no "forgot to check Allow" bug.
#[derive(Debug, Clone)]
pub enum HookOutput {
    /// The operation is allowed to proceed.
    Allow,

    /// The operation is blocked. Exit code 2 in Claude Code hook protocol.
    Block {
        /// Human-readable reason displayed to the agent.
        reason: String,
        /// Optional classification of the block type.
        category: Option<BlockCategory>,
    },

    /// The operation is allowed but a warning is emitted to stderr.
    Warn {
        message: String,
    },
}

/// Classification of why a hook blocked an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockCategory {
    /// PathFlow gate check (missing sentinel).
    Gate,
    /// Security enforcement (dangerous command).
    Security,
    /// Team guard (team dissolution protection).
    TeamGuard,
    /// Edit/write scope enforcement.
    EditWriteScope,
    /// GitHub PR guard (protected branch).
    GhPrGuard,
    /// Protected resource access.
    ProtectedResource,
    /// WebFetch domain block.
    WebFetch,
}

/// HookHandler processes a single hook enforcement module.
///
/// Each Go hook sub-package (gate, sentinel, team, edit, ghpr, resource,
/// webfetch, security, logging, session, prompt) becomes a struct implementing
/// this trait.
///
/// The trait uses `fn handle(&self, input: HookInput) -> Result<HookOutput, HookError>`
/// as specified in the task acceptance criteria.
pub trait HookHandler: Send + Sync {
    /// Process the hook input and return a verdict.
    ///
    /// - `Ok(HookOutput::Allow)` -- operation proceeds
    /// - `Ok(HookOutput::Block { .. })` -- operation blocked (exit 2)
    /// - `Ok(HookOutput::Warn { .. })` -- operation proceeds with warning
    /// - `Err(HookError)` -- handler error, graceful degradation (allow through)
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError>;

    /// Human-readable name for this handler (for logging/diagnostics).
    fn name(&self) -> &str;

    /// Which hook events this handler responds to.
    fn events(&self) -> &[HookEvent];
}
```

#### 4.3.1 Hook Handler Implementations (Module Mapping)

Each Go hook sub-package becomes a struct implementing `HookHandler`:

| Go Package | Rust Module | Struct Name | Events |
|-----------|-------------|-------------|--------|
| `hooks/gate` | `codeflow_hooks::gate` | `GateChecker` | PreToolUse |
| `hooks/sentinel` | `codeflow_hooks::sentinel` | `SentinelWriter` | PostToolUse |
| `hooks/team` | `codeflow_hooks::team` | `TeamGuard` | PreToolUse |
| `hooks/edit` | `codeflow_hooks::edit` | `EditWriteGuard` | PreToolUse |
| `hooks/ghpr` | `codeflow_hooks::ghpr` | `GhPrGuard` | PreToolUse |
| `hooks/resource` | `codeflow_hooks::resource` | `ResourceGuard` | PreToolUse |
| `hooks/webfetch` | `codeflow_hooks::webfetch` | `WebFetchGuard` | PreToolUse |
| `hooks/security` | `codeflow_hooks::security` | `SecurityChecker` | PreToolUse |
| `hooks/logging` | `codeflow_hooks::logging` | `LoggingHandler` | PostToolUse, SessionStart, SessionEnd, Stop, UserPromptSubmit |
| `hooks/session` | `codeflow_hooks::session` | `SessionHandler` | SessionStart, SessionEnd |
| `hooks/prompt` | `codeflow_hooks::prompt` | `PromptHandler` | UserPromptSubmit |
| (new) | `codeflow_hooks::checkpoint` | `CheckpointHandler` | PostToolUse, TaskCompleted |

### 4.4 Deferred Traits (Epic A Placeholders)

These traits are documented here for cross-task coherence but are NOT defined in Epic 0. They are implemented in Epic A.

```rust
// Placeholder: codeflow-core/src/coordinator.rs (Epic A)
//
// pub trait Coordinator: Send + Sync {
//     /// Acquire a claim on a file scope pattern.
//     async fn acquire(&self, scope: &str, session_id: &str) -> Result<Claim, CoordinatorError>;
//     /// Release a claim.
//     async fn release(&self, claim_id: &str) -> Result<(), CoordinatorError>;
//     /// Merge remote CRDT state into local state.
//     async fn merge(&self, remote: &[u8]) -> Result<(), CoordinatorError>;
//     /// Export local CRDT state for transport.
//     async fn export(&self) -> Result<Vec<u8>, CoordinatorError>;
// }
// Sole impl: LoroCoordinator (loro crate)

// Placeholder: codeflow-core/src/transport.rs (Epic A)
//
// pub trait Transport: Send + Sync {
//     /// Send a delta to the remote peer.
//     async fn send_delta(&self, delta: &[u8]) -> Result<(), TransportError>;
//     /// Receive a delta from the remote peer.
//     async fn receive_delta(&self) -> Result<Vec<u8>, TransportError>;
// }
// Impls: FileTransport, GitRefTransport (git2 crate)
```

## 5. Module Dependency Graph

### 5.1 Crate Structure

Two crates as specified in data-layer-protection.md Section 6. The Rust workspace root is `codeflow-rs/` (coexists with the Go `codeflow-cli/` directory during migration, per go-audit.md Section 6.1):

```
codeflow-rs/                            # Rust workspace (coexists with codeflow-cli/ during migration)
  Cargo.toml              # [workspace] definition
  codeflow-core/          # Library crate
    src/
      lib.rs              # pub mod declarations, re-exports
      error.rs            # DbError, LedgerError, HookError, SessionError
      store.rs            # DataStore trait
      ledger.rs           # LedgerWriter trait, Event, file constants
      hooks.rs            # HookHandler trait, HookInput, HookOutput
      config.rs           # Config loading (generic load<T>)
      types/
        mod.rs            # pub use re-exports
        phase.rs          # Phase enum
        stage.rs          # WorkStage enum
        work.rs           # AreaType, WorkType, TaskStatus, EpicStatus, etc.
      models/
        mod.rs            # pub use re-exports
        session.rs        # Session struct
        epic.rs           # Epic struct
        task.rs           # Task struct
        active_work.rs    # ActiveWork struct
        memory.rs         # MemoryEvent struct
        autorun.rs        # AutorunSession, AutorunWorker, AutorunTaskRun
        user.rs           # User, ProjectConfig (excluded from DataStore, see 4.1.1)
        filters.rs        # SessionFilter, EpicFilter, TaskFilter, etc.
        updates.rs        # SessionUpdate, EpicUpdate, TaskUpdate, etc.
  codeflow-cli/           # Binary crate (note: different from Go's codeflow-cli/ directory)
    src/
      main.rs             # #[tokio::main], Clap dispatch
      cmd/
        mod.rs
        hooks.rs          # hooks subcommand dispatch
        pathflow.rs       # pathflow subcommands
        session.rs        # session subcommands
        db.rs             # db subcommands
        workgraph.rs      # workgraph subcommands
        validate.rs       # validate subcommands
        ... (remaining)
```

### 5.2 Dependency Graph (No Circular Dependencies)

```
codeflow-core (lib)                     codeflow-cli (bin)
  |                                       |
  +-- types/ (enums, no deps)             +-- cmd/ (Clap dispatch)
  |                                       |     |
  +-- error.rs (depends on: types)        |     +-- depends on: codeflow-core
  |                                       |
  +-- models/ (depends on: types)         +-- depends on: surrealdb (SurrealStore impl)
  |                                       +-- depends on: codeflow-core
  +-- store.rs (depends on: error, types, models)
  |
  +-- ledger.rs (depends on: error)
  |
  +-- hooks.rs (depends on: error)
  |
  +-- config.rs (depends on: error)
```

**Dependency direction is strictly downward:**

```
          types/
            |
         error.rs
          / | \
    models/ | config.rs
      |     |
  store.rs  ledger.rs  hooks.rs
```

No module imports from a module at the same level or above. No circular dependencies exist.

### 5.3 External Crate Dependencies

| Crate | Used By | Purpose |
|-------|---------|---------|
| `serde` + `serde_json` | types, models, ledger, hooks, config | Serialization (all domain types) |
| `thiserror` | error | Error enum derivation |
| `surrealdb` | codeflow-cli (SurrealStore impl) | Database operations |
| `clap` | codeflow-cli | CLI argument parsing |
| `tokio` | codeflow-cli | Async runtime for SurrealDB |
| `anyhow` | codeflow-cli | CLI-level error propagation |

Note: `surrealdb` is a dependency of `codeflow-cli` (the binary), NOT `codeflow-core` (the library). The `DataStore` trait in `codeflow-core` is database-agnostic. The `SurrealStore` implementation lives in the binary crate (or a future `codeflow-db` crate if the binary grows too large). The `futures` crate is NOT used -- transaction support lives on the `SurrealStore` impl directly and uses `std::pin::Pin<Box<dyn Future>>` instead.

## 6. Go Package Consolidation Decisions

### 6.1 Consolidation Summary

| # | Consolidation | Go Source | Rust Target | Rationale | DRY Violation Fixed |
|---|--------------|-----------|-------------|-----------|---------------------|
| 1 | Normalization merge | `db/sync.go` + `ledger/normalize.go` | `DataStore::sync_from_events()` in codeflow-core, impl in codeflow-cli | Largest DRY violation: identical event normalization in two packages | go-audit.md 3.1 |
| 2 | Sentinel merge | `pathflow/sentinel.go` + `sentinel/sentinel.go` | `codeflow_hooks::sentinel` module | Two independent sentinel implementations with different API styles | go-audit.md 3.3 |
| 3 | Session + workstate merge | `session/` + `workstate/` | Models in `codeflow-core::models`, session logic in codeflow-cli | Both manage session runtime state in `.state/runtime/` | go-audit.md 4.4 |
| 4 | Verdict unification | 8 `Verdict` structs across hook packages | `HookOutput` enum in `codeflow-core::hooks` | Single enum replaces 8 structs | go-audit.md 3.7 |
| 5 | HookInput unification | 9 input structs across hook packages | `HookInput` struct in `codeflow-core::hooks` | Single struct replaces 9 duplicates | go-audit.md 3.8 |
| 6 | File constants merge | `ledger/routing.go` + `db/sync.go` constants | `codeflow-core::ledger::files` module | Eliminates crosscheck test hack | go-audit.md 3.4 |
| 7 | Config loading unification | 3 config loaders (githooks, cliutil, resource) | `codeflow-core::config::load::<T>()` | Generic deserialization function | go-audit.md 3.12 |

### 6.2 Decision Details

**Decision 1: Normalization merge into DataStore::sync_from_events()**

The Go codebase has identical normalization logic in `db/sync.go` and `ledger/normalize.go`. In Rust, event normalization is a `From<LegacyEvent>` trait implementation within the `sync_from_events` method. The `LedgerWriter` writes canonical events; the `DataStore` knows how to parse both legacy and canonical formats during rebuild. No duplication.

**Decision 2: Sentinel into hooks crate**

Go has `pathflow/sentinel.go` (package-level functions, hardcoded "pathflow" prefix) and `sentinel/sentinel.go` (Manager struct, configurable scope). Rust merges both into a single `SentinelManager` struct parameterized by scope, living in the hooks crate since sentinels are part of hook enforcement.

**Decision 3: Session + workstate merge**

Go separates `session/` (lifecycle: start/end) from `workstate/` (runtime: active-task.json). Both operate on `.state/runtime/`. In Rust, the data models (`Session`, `ActiveWork`) live in `codeflow-core::models`. The runtime file management (reading/writing `active-task.json`) lives alongside session management in the CLI binary. No separate crate needed -- the combined scope is small (~450 Go lines).

**Decision 4: Verdict -> HookOutput enum**

The Go pattern (`Allow bool` + optional fields) allows a bug where the caller forgets to check `Allow`. The Rust `HookOutput` enum makes this impossible: `Allow`, `Block { reason, category }`, and `Warn { message }` force exhaustive pattern matching.

**Decision 5: HookInput unification**

All 9 Go `hookInput` variants have the same two fields (`ToolName`, `ToolInput`). In Rust, a single `HookInput` struct adds `event`, `session_id`, and `project_dir` -- context that most handlers need and currently extract independently from the environment.

**Decision 6: File constants in ledger module**

Go has the same 4 JSONL filename constants in both `ledger/routing.go` and `db/sync.go`, with a crosscheck test to keep them in sync. Rust defines them once in `codeflow-core::ledger::files`. The crosscheck test is eliminated -- there is only one definition.

**Decision 7: Generic config loading**

Three Go packages independently implement `os.ReadFile() -> json.Unmarshal()`. Rust's `config::load::<T: DeserializeOwned>(path: &Path)` handles this generically for any deserializable type.

### 6.3 Split Decisions

| # | Split | Go Source | Rust Target | Rationale |
|---|-------|-----------|-------------|-----------|
| 1 | Hooks stay modular | `hooks/` (11 sub-packages) | `codeflow-cli` with per-handler modules | Each handler is independent; module boundaries are clean |
| 2 | Autorun stays separate | `autorun/` | Separate module in codeflow-cli | Independent orchestration logic with distinct dependencies |
| 3 | claim -> DataStore | `claim/` | `DataStore` methods (future Epic A `Coordinator`) | Claim logic is data operations; CRDT aspects deferred to Epic A |

### 6.4 Go-to-Rust Package Mapping (Complete)

| Go Package | Lines | Rust Location | Notes |
|-----------|-------|---------------|-------|
| `db` | 2,257 | `codeflow-core` (traits, models), `codeflow-cli` (SurrealStore impl) | Trait in core, impl in cli |
| `ledger` | 792 | `codeflow-core::ledger` (trait, Event, routing), `codeflow-cli` (JsonlWriter impl) | Trait in core, impl in cli |
| `hooks/*` | 6,531 | `codeflow-cli::hooks::*` modules implementing `HookHandler` trait | Unified via trait |
| `pathflow` | 904 | `codeflow-core::types` (enums), `codeflow-cli::hooks::sentinel` + `::checkpoint` | Enums in core, enforcement in cli |
| `sentinel` | 170 | Merged into `codeflow-cli::hooks::sentinel` | Consolidated with pathflow/sentinel.go |
| `session` | 323 | `codeflow-core::models::Session`, `codeflow-cli` session commands | Model in core, commands in cli |
| `workstate` | 125 | Merged into `codeflow-core::models::ActiveWork` | Consolidated with session |
| `workgraph` | 1,082 | `codeflow-core::models` (domain types), `codeflow-cli` workgraph commands | Model in core, commands in cli |
| `claim` | 625 | Deferred to Epic A (`Coordinator` trait) | CRDT-based in Epic A |
| `autorun` | 983 | `codeflow-cli::autorun` module | Stays separate |
| `config` | 412 | `codeflow-core::config` | Generic load<T> |
| `cliutil` | 303 | Split: errors -> `codeflow-core::error`, config -> `codeflow-core::config`, logging -> `tracing` | Distributed to appropriate modules |
| `settings` | 716 | `codeflow-cli::settings` | CLI-specific |
| `githooks` | 1,217 | `codeflow-cli::githooks` module | Stays separate |
| `doctor` | 1,133 | `codeflow-cli::doctor` module | CLI-specific |
| `initialize` | 559 | `codeflow-cli::init` module | CLI-specific |
| `preflight` | 264 | `codeflow-cli::preflight` module | CLI-specific |
| `validate` | 700 | `codeflow-cli::validate` module | CLI-specific |
| `report` | 461 | `codeflow-cli::report` module | CLI-specific |
| `welcome` | 649 | `codeflow-cli::welcome` module | CLI-specific |
| `update` | 314 | `codeflow-cli::update` module | CLI-specific |
| `shadowtest` | 1,163 | `codeflow-cli::shadowtest` (behind `#[cfg(feature = "shadowtest")]`) | Dev-only |
| `worktree` | 546 | `codeflow-cli::worktree` module | CLI-specific |
| `idgen` | 164 | `ulid` crate dependency; format_id logic in `codeflow-core::types` | External crate |
| `testutil` | 117 | `#[cfg(test)]` helper modules per crate | Standard Rust pattern |
| `claude` | 2 | Removed (doc-only stub) | No content |
| `benchmark` | 0 | `criterion` benchmarks | Standard Rust pattern |
| `integration` | 0 | `tests/` directory at workspace root | Standard Rust pattern |
| `verification` | 0 | `tests/` directory | Standard Rust pattern |

## 7. Assumptions

| # | Assumption | Verified? | Evidence |
|---|-----------|-----------|----------|
| 1 | Two-crate structure (core lib + cli bin) | YES | data-layer-protection.md Section 6 diagram explicitly shows this |
| 2 | SurrealDB is the sole DataStore impl (no legacy store) | YES | data-layer-protection.md Section 6: "legacy store REMOVED" |
| 3 | Go db package has 10 model structs (8 with DataStore CRUD + 2 excluded) | YES | Read db/models.go: ActiveWork, Session, Epic, Task, MemoryEvent, AutorunSession, AutorunWorker, AutorunTaskRun (8 with CRUD). ProjectConfig and User defined as structs but excluded from trait (see Section 4.1.1). |
| 4 | 8 separate Verdict structs exist in hooks | YES | go-audit.md Section 3.7 lists all 8 with file:line references |
| 5 | 9 separate hookInput structs exist | YES | go-audit.md Section 3.8 lists all 9 with file:line references |
| 6 | Async is needed for DataStore (SurrealDB) | YES | cf-surrealdb-standards: all Surreal operations are async (.await) |
| 7 | Synchronous is appropriate for LedgerWriter | YES | ledger/writer.go uses syscall.Flock (blocking, local I/O only) |
| 8 | Synchronous is appropriate for HookHandler | YES | All hook handlers perform local file checks, no network I/O |
| 9 | Coordinator and Transport are deferred to Epic A | YES | data-layer-protection.md Section 6 and task description both state this |
| 10 | 11 existing hook sub-packages in Go + 1 new checkpoint handler = 12 total HookHandler implementations | YES | Glob found 11 existing: gate, sentinel, team, edit, ghpr, resource, webfetch, security, logging, session, prompt. Section 4.3.1 adds checkpoint (new, no Go equivalent). |
