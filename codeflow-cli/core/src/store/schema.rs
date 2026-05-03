use surrealdb::{Connection, Surreal};

use crate::error::DbError;

/// Raw `SurrealQL` schema loaded at compile time.
pub const SCHEMA_SQL: &str = include_str!("schema.surql");

/// One-shot migration: back-fill `session_kind='autorun'` for existing
/// `interactive_session` rows whose `session_id` matches an `autorun_session`
/// id. INF-TSK-049-001 AC #8.
///
/// Idempotent: a second run matches zero rows because every targeted row is
/// already `'autorun'` after the first. Safe to call on every startup.
///
/// Uses `record::id(id)` to unwrap the `RecordId` to its string key part so
/// the set comparison against `interactive_session.session_id` (a plain
/// string) works.
const MIGRATION_BACKFILL_AUTORUN_KIND: &str = "\
UPDATE interactive_session SET session_kind = 'autorun' \
WHERE session_kind = 'interactive' \
  AND session_id IN (SELECT VALUE record::id(id) FROM autorun_session);";

/// One-shot migration: drop the legacy `pid` column from `interactive_session`
/// rows. INF-TSK-024-051 Phase 4-C.
///
/// The schema no longer defines `pid` on `interactive_session`, but SurrealDB
/// SCHEMAFULL does NOT auto-strip columns when a `DEFINE FIELD` is removed —
/// existing rows keep the (now-undefined) column until explicitly unset. This
/// `UNSET` migration scrubs the column from every row at startup.
///
/// Idempotent: rows that no longer have the field are no-ops.
const MIGRATION_DROP_INTERACTIVE_SESSION_PID: &str = "UPDATE interactive_session UNSET pid;";

/// Apply the idempotent schema to a `SurrealDB` instance.
///
/// All statements use `OVERWRITE`, making this safe to run on every startup.
/// After the schema, runs one-shot migrations that back-fill newly-added
/// columns on pre-existing rows (see [`MIGRATION_BACKFILL_AUTORUN_KIND`]) and
/// drop deprecated columns (see [`MIGRATION_DROP_INTERACTIVE_SESSION_PID`]).
///
/// # Errors
///
/// Returns `DbError::Surreal` if any schema statement fails.
pub async fn apply_schema<C: Connection>(db: &Surreal<C>) -> Result<(), DbError> {
    db.query(SCHEMA_SQL).await?.check()?;
    // Back-fill session_kind for rows created before the column existed.
    // Idempotent: rows already set to 'autorun' are not matched.
    let _ = db.query(MIGRATION_BACKFILL_AUTORUN_KIND).await?.check();
    // Drop the legacy `pid` column from any rows that still have it.
    // Idempotent: UNSET on a missing field is a no-op.
    let _ = db
        .query(MIGRATION_DROP_INTERACTIVE_SESSION_PID)
        .await?
        .check();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sql_is_nonempty() {
        assert!(!SCHEMA_SQL.is_empty());
    }

    #[test]
    fn schema_contains_core_tables() {
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE project_config"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE user"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE epic"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE task"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE session"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE active_work"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE memory_event"));
    }

    #[test]
    fn schema_contains_relation_tables() {
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE belongs_to TYPE RELATION"));
        assert!(SCHEMA_SQL.contains("DEFINE TABLE OVERWRITE depends_on TYPE RELATION"));
    }

    #[test]
    fn schema_contains_vector_index() {
        assert!(SCHEMA_SQL.contains("HNSW DIMENSION 384"));
    }

    #[test]
    fn schema_contains_autorun_new_fields() {
        // autorun_session new fields (INF-TSK-023-032)
        assert!(
            SCHEMA_SQL
                .contains("DEFINE FIELD OVERWRITE pid                 ON TABLE autorun_session"),
            "autorun_session missing pid field"
        );
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE skipped_tasks"),
            "autorun_session missing skipped_tasks field"
        );
        // INF-TSK-050-001 AC #1: abort_started_at field for the abort
        // timeout watchdog. Wave 2 fix: typed as option<string> (not
        // option<datetime>) so MERGE writes can pass RFC 3339 strings
        // without a SCHEMAFULL TYPE mismatch.
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE abort_started_at   ON TABLE autorun_session TYPE option<string>"),
            "abort_started_at should be option<string> on autorun_session"
        );
        // autorun_worker new fields
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE file_scope    ON TABLE autorun_worker"),
            "autorun_worker missing file_scope field"
        );
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE scope_policy  ON TABLE autorun_worker"),
            "autorun_worker missing scope_policy field"
        );
        // autorun_task_run new fields
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE blocked_reason"),
            "autorun_task_run missing blocked_reason field"
        );
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE claim_conflicts"),
            "autorun_task_run missing claim_conflicts field"
        );
        assert!(
            SCHEMA_SQL.contains("DEFINE FIELD OVERWRITE merge_conflicts"),
            "autorun_task_run missing merge_conflicts field"
        );
    }

    #[test]
    fn schema_does_not_define_interactive_session_pid() {
        // INF-TSK-024-051 Phase 4-C: the `pid` column on `interactive_session`
        // was wrong-by-construction for managed sessions (the value captured
        // pre-`exec` was the codeflow CLI PID, not the resulting Claude lead
        // PID). The canonical PID source is `pathflow-session-status.json::lead_pid`
        // read via `crate::session::liveness::is_session_alive`. If this
        // assertion ever fires it means a regression added the column back.
        assert!(
            !SCHEMA_SQL.contains("ON TABLE interactive_session TYPE int"),
            "interactive_session must not redefine the `pid` column \
             (INF-TSK-024-051): canonical source is \
             pathflow-session-status.json::lead_pid via \
             session::liveness::is_session_alive"
        );
    }

    #[test]
    fn schema_uses_overwrite() {
        // Every DEFINE statement should use OVERWRITE for idempotency.
        // Statements may span multiple lines, ending with `;`.
        let mut stmt = String::new();
        for line in SCHEMA_SQL.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("DEFINE ") {
                stmt.clear();
                stmt.push_str(trimmed);
            } else if !stmt.is_empty() {
                stmt.push(' ');
                stmt.push_str(trimmed);
            }
            if stmt.ends_with(';') && !stmt.is_empty() {
                assert!(stmt.contains("OVERWRITE"), "missing OVERWRITE in: {stmt}");
                stmt.clear();
            }
        }
    }
}
