//! Workgraph command: workgraph operations (epic/task CRUD, queries).

use anyhow::{Context, Result};
use codeflow_core::JsonlWriter;
use codeflow_core::store::SurrealStore;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let db_path = project_dir.join(".state").join("db").join("codeflow.db");
    let ledger_dir = project_dir.join(".state").join("ledger");

    let store = SurrealStore::open(&db_path)
        .await
        .context("opening database")?;
    let ledger = JsonlWriter::new(&ledger_dir).context("opening ledger")?;

    let _service = codeflow_core::workgraph::WorkgraphService::new(store, ledger);

    println!("workgraph ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_workgraph_command_exists() {
        // Compile-time verification that run is async.
        let _: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>>>> =
            || Box::pin(super::run());
    }

    #[test]
    fn test_workgraph_service_type_check() {
        // Verify the generic types are compatible at compile time.
        fn _type_check(
            store: codeflow_core::store::SurrealStore,
            ledger: codeflow_core::JsonlWriter,
        ) {
            let _svc = codeflow_core::workgraph::WorkgraphService::new(store, ledger);
        }
    }
}
