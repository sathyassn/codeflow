//! Database operations: schema, sync, integrity check.

use anyhow::{Context, Result};
use codeflow_core::DataStore;
use codeflow_core::store::SurrealStore;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let db_path = project_dir.join(".state").join("db").join("codeflow.db");

    let store = SurrealStore::open(&db_path)
        .await
        .context("opening database")?;

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

    #[test]
    fn test_db_command_exists() {
        // Compile-time verification that the module exists and run is async.
        let _: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>>>> =
            || Box::pin(super::run());
    }

    #[test]
    fn test_db_uses_data_store_trait() {
        // Compile-time verification that SurrealStore implements DataStore.
        fn _assert_data_store<T: DataStore>() {}
        fn _check() {
            _assert_data_store::<SurrealStore>();
        }
    }
}
