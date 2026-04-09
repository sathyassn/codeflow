//! Database operations: schema, sync, integrity check.

use anyhow::{Context, Result};
use codeflow_core::DataStore;
use codeflow_core::store::SurrealStore;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir).await
}

async fn run_with_dir(project_dir: &std::path::Path) -> Result<()> {
    let store = open_store(project_dir).await?;
    verify_database(&store).await
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
        let result = run().await;
        std::env::set_current_dir(orig).unwrap();
        assert!(result.is_ok(), "run() should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_db_run_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path()).await;
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
}
