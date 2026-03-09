//! Generic configuration loading for JSON config files.
//!
//! Provides a simple `load<T>` function for deserializing JSON configuration
//! files from `.codeflow/config/` directories (e.g., `pathflow-config.json`,
//! `enforcement-policy.json`).
//!
//! The full namespace/merge system is a CLI concern and lives in `codeflow-cli`.
//! This module handles single-file loading only.

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::ConfigError;

/// Load and deserialize a JSON configuration file.
///
/// # Errors
///
/// - `ConfigError::NotFound` if the file does not exist
/// - `ConfigError::Io` on read failure
/// - `ConfigError::Parse` on JSON deserialization failure
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound(path.display().to_string()));
    }

    let content = fs::read_to_string(path)?;

    serde_json::from_str(&content)
        .map_err(|e| ConfigError::Parse(format!("{path}: {e}", path = path.display())))
}

/// Load a JSON config file and return it as a generic `serde_json::Value`.
///
/// Useful when the schema is not known at compile time or for inspection.
///
/// # Errors
///
/// Same as [`load`].
pub fn load_value(path: &Path) -> Result<serde_json::Value, ConfigError> {
    load(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestConfig {
        name: String,
        value: i64,
        optional: Option<String>,
    }

    #[test]
    fn test_load_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-config.json");
        fs::write(
            &path,
            r#"{"name": "test", "value": 42, "optional": "present"}"#,
        )
        .unwrap();

        let config: TestConfig = load(&path).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.value, 42);
        assert_eq!(config.optional.as_deref(), Some("present"));
    }

    #[test]
    fn test_load_config_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let result: Result<TestConfig, _> = load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected NotFound: {err}"
        );
    }

    #[test]
    fn test_load_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{ not valid json }").unwrap();

        let result: Result<TestConfig, _> = load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("parse"),
            "expected Parse error: {err}"
        );
    }

    #[test]
    fn test_load_config_with_optional_field_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("minimal.json");
        fs::write(&path, r#"{"name": "minimal", "value": 1}"#).unwrap();

        let config: TestConfig = load(&path).unwrap();
        assert_eq!(config.name, "minimal");
        assert_eq!(config.value, 1);
        assert!(config.optional.is_none());
    }

    #[test]
    fn test_load_value_returns_generic_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generic.json");
        fs::write(&path, r#"{"key": "val", "nested": {"a": 1}}"#).unwrap();

        let value = load_value(&path).unwrap();
        assert_eq!(value["key"], "val");
        assert_eq!(value["nested"]["a"], 1);
    }

    #[test]
    fn test_load_value_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let result = load_value(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        fs::write(&path, "").unwrap();

        let result: Result<TestConfig, _> = load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_wrong_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong.json");
        fs::write(&path, r#"{"wrong_field": true}"#).unwrap();

        let result: Result<TestConfig, _> = load(&path);
        assert!(result.is_err());
    }
}
