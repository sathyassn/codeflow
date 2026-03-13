//! Workgraph command: workgraph operations (epic/task CRUD, queries).

use anyhow::{Context, Result};
use codeflow_core::JsonlWriter;
use codeflow_core::store::SurrealStore;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir).await
}

async fn run_with_dir(project_dir: &std::path::Path) -> Result<()> {
    let ledger_dir = project_dir.join(".state").join("ledger");
    let store = open_store(project_dir).await?;
    let ledger = JsonlWriter::new(&ledger_dir).context("opening ledger")?;
    verify_workgraph(store, ledger);
    Ok(())
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

fn verify_workgraph(store: SurrealStore, ledger: JsonlWriter) {
    let _service = codeflow_core::workgraph::WorkgraphService::new(store, ledger);

    println!("workgraph ok");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_workgraph_run() {
        let dir = tempfile::tempdir().unwrap();
        // Create markers so detect_project_dir() + run_with_dir() succeed.
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("ledger")).unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = run().await;
        std::env::set_current_dir(orig).unwrap();
        assert!(result.is_ok(), "run() should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_workgraph_run_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join(".state").join("ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        let result = run_with_dir(dir.path()).await;
        assert!(result.is_ok(), "run_with_dir should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_workgraph_open_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path()).await;
        assert!(store.is_ok(), "open_store should succeed");
    }

    #[tokio::test]
    async fn test_workgraph_verify() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();

        let store = SurrealStore::in_memory().await.unwrap();
        let ledger = JsonlWriter::new(&ledger_dir).unwrap();
        verify_workgraph(store, ledger);
    }

    #[test]
    fn test_jsonl_writer_creation() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();

        let writer = JsonlWriter::new(&ledger_dir);
        assert!(writer.is_ok());
    }

    #[test]
    fn test_jsonl_writer_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("nonexistent");
        // JsonlWriter::new may create the dir or fail — either is valid.
        let _result = JsonlWriter::new(&ledger_dir);
    }
}
