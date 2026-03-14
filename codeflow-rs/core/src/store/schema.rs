use surrealdb::{Connection, Surreal};

use crate::error::DbError;

/// Raw `SurrealQL` schema loaded at compile time.
pub const SCHEMA_SQL: &str = include_str!("schema.surql");

/// Apply the idempotent schema to a `SurrealDB` instance.
///
/// All statements use `OVERWRITE`, making this safe to run on every startup.
///
/// # Errors
///
/// Returns `DbError::Surreal` if any schema statement fails.
pub async fn apply_schema<C: Connection>(db: &Surreal<C>) -> Result<(), DbError> {
    db.query(SCHEMA_SQL).await?.check()?;
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
