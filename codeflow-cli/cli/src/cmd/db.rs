//! Database operations: schema, sync, integrity check, exec, query.

use anyhow::{Context, Result};
use clap::Subcommand;
use codeflow_core::DataStore;
use codeflow_core::store::SurrealStore;

use crate::helpers;

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Verify database schema and integrity (default)
    Verify,
    /// Execute a SurrealQL write statement
    Exec {
        /// SurrealQL statement to execute
        #[arg(long)]
        query: String,
    },
    /// Execute a SurrealQL read query and return JSON
    Query {
        /// SurrealQL query to execute
        #[arg(long)]
        query: String,
    },
}

pub async fn run(command: Option<DbCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir, command).await
}

async fn run_with_dir(project_dir: &std::path::Path, command: Option<DbCommand>) -> Result<()> {
    let store = open_store(project_dir).await?;
    match command {
        None | Some(DbCommand::Verify) => verify_database(&store).await,
        Some(DbCommand::Exec { query }) => run_exec(&store, &query).await,
        Some(DbCommand::Query { query }) => run_query(&store, &query).await,
    }
}

/// Open the project's `SurrealDB` store.
///
/// In tests, returns an in-memory store to avoid the `surrealkv://`
/// SIGKILL under LLVM coverage instrumentation.
#[cfg(not(test))]
async fn open_store(project_dir: &std::path::Path) -> Result<SurrealStore> {
    let db_path = project_dir.join(".state").join("db").join("codeflow.db");
    SurrealStore::open(&db_path)
        .await
        .context("opening database")
}

#[cfg(test)]
async fn open_store(_project_dir: &std::path::Path) -> Result<SurrealStore> {
    SurrealStore::in_memory()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

async fn verify_database(store: &SurrealStore) -> Result<()> {
    store.apply_schema().await.context("applying schema")?;
    store
        .check_integrity()
        .await
        .context("checking integrity")?;

    println!("database ok");
    Ok(())
}

async fn run_exec(store: &SurrealStore, query: &str) -> Result<()> {
    let result = store
        .query_to_json(query)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("executing query")?;

    // Count affected rows when result is an array.
    let count = if let Some(arr) = result.as_array() {
        arr.len()
    } else {
        1
    };
    println!("{count} row(s) affected");
    Ok(())
}

async fn run_query(store: &SurrealStore, query: &str) -> Result<()> {
    let result = store
        .query_to_json(query)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("executing query")?;

    let json = serde_json::to_string_pretty(&result).context("serializing result")?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[tokio::test]
    #[serial(env_vars)]
    async fn test_db_run() {
        let dir = tempfile::tempdir().unwrap();
        // Create `.claude/` marker so detect_project_dir() finds this dir.
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = run(None).await;
        std::env::set_current_dir(orig).unwrap();
        assert!(result.is_ok(), "run() should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_run_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), None).await;
        assert!(result.is_ok(), "run_with_dir should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_open_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await;
        assert!(store.is_ok(), "open_store should succeed");
    }

    #[tokio::test]
    async fn test_db_verify_database() {
        let store = SurrealStore::in_memory().await.unwrap();
        let result = verify_database(&store).await;
        assert!(result.is_ok(), "verify_database should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_schema_apply_and_integrity() {
        let store = SurrealStore::in_memory().await.unwrap();
        store.apply_schema().await.unwrap();
        store.check_integrity().await.unwrap();
    }

    #[tokio::test]
    async fn test_db_double_schema_apply_idempotent() {
        let store = SurrealStore::in_memory().await.unwrap();
        store.apply_schema().await.unwrap();
        store.apply_schema().await.unwrap();
        store.check_integrity().await.unwrap();
    }

    #[tokio::test]
    async fn test_db_exec_runs_write_statement() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await.unwrap();
        store.apply_schema().await.unwrap();
        let result = run_exec(&store, "CREATE test_exec:1 SET name = 'hello'").await;
        assert!(result.is_ok(), "exec should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_query_returns_json() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await.unwrap();
        store.apply_schema().await.unwrap();
        // Insert a record then query it.
        let _ = run_exec(&store, "CREATE test_query:1 SET name = 'world'").await;
        let result = run_query(&store, "SELECT * FROM test_query").await;
        assert!(result.is_ok(), "query should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_exec_invalid_query_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await.unwrap();
        // Intentionally malformed query.
        let result = run_exec(&store, "INVALID SYNTAX HERE !!!").await;
        assert!(result.is_err(), "invalid query should fail");
    }

    #[tokio::test]
    async fn test_db_query_empty_result() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await.unwrap();
        store.apply_schema().await.unwrap();
        let result = run_query(&store, "SELECT * FROM nonexistent_table_xyz").await;
        assert!(result.is_ok(), "empty query should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_subcommand_verify() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), Some(DbCommand::Verify)).await;
        assert!(
            result.is_ok(),
            "verify subcommand should succeed: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_db_subcommand_exec() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(
            dir.path(),
            Some(DbCommand::Exec {
                query: "CREATE test_sub:1 SET val = 42".into(),
            }),
        )
        .await;
        assert!(result.is_ok(), "exec subcommand should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_subcommand_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await.unwrap();
        let _ = store
            .query_to_json("CREATE test_sub_q:1 SET val = 99")
            .await;
        let result = run_with_dir(
            dir.path(),
            Some(DbCommand::Query {
                query: "SELECT * FROM test_sub_q".into(),
            }),
        )
        .await;
        assert!(
            result.is_ok(),
            "query subcommand should succeed: {result:?}"
        );
    }
}
