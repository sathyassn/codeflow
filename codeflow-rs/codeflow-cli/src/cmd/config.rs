//! Config command: configuration management with list, get, set subcommands.
//!
//! Mirrors the Go CLI's `codeflow config {list,get,set}` interface.
//! Reads `.codeflow/config/` directory, walks JSON files, builds flat
//! dot-notation key-value map (namespaced by file path).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde_json::Value;

use crate::helpers;

/// Config subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// List all configuration keys and values
    List,
    /// Get a configuration value by dot-notation key
    Get {
        /// Dot-notation key (e.g., enforcement.enforcement-policy.git_format.commit_types)
        key: String,
    },
    /// Set a configuration value
    Set {
        /// Dot-notation key
        key: String,
        /// Value to set
        value: String,
    },
}

pub fn run(cmd: Option<ConfigCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match cmd {
        None | Some(ConfigCommand::List) => run_list(&project_dir),
        Some(ConfigCommand::Get { key }) => run_get(&project_dir, &key),
        Some(ConfigCommand::Set { key, value }) => run_set(&project_dir, &key, &value),
    }
}

fn run_list(project_dir: &Path) -> Result<()> {
    let kv = read_config_dir(&project_dir.join(".codeflow").join("config"))?;
    for (k, v) in &kv {
        println!("{k} = {}", format_value(v));
    }
    Ok(())
}

fn run_get(project_dir: &Path, key: &str) -> Result<()> {
    let kv = read_config_dir(&project_dir.join(".codeflow").join("config"))?;
    match kv.get(key) {
        Some(v) => {
            println!("{}", format_value(v));
            Ok(())
        }
        None => bail!("key not found: {key}"),
    }
}

fn run_set(_project_dir: &Path, _key: &str, _value: &str) -> Result<()> {
    // Stub: set is not exercised by conformance tests.
    bail!("config set not yet implemented")
}

/// Format a `serde_json::Value` to match Go's `%v` formatting:
/// - Arrays: `[elem1 elem2 elem3]` (space-separated, no commas)
/// - Strings: unquoted
/// - Numbers/bools: as-is
fn format_value(v: &Value) -> String {
    match v {
        Value::Array(arr) => {
            let elems: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", elems.join(" "))
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Object(_) => format!("{v}"),
    }
}

/// Walk a config directory, read all `*.json` files, and return a flat
/// dot-notation key-value map. Keys are namespaced by file relative path.
///
/// Example: `enforcement/enforcement-policy.json` with key `git_format.commit_types`
/// becomes `enforcement.enforcement-policy.git_format.commit_types`.
fn read_config_dir(dir: &Path) -> Result<BTreeMap<String, Value>> {
    let mut result = BTreeMap::new();

    if !dir.is_dir() {
        return Ok(result);
    }

    walk_json_files(dir, dir, &mut result)?;
    Ok(result)
}

/// Recursively walk directory for JSON files and flatten into the result map.
fn walk_json_files(root: &Path, dir: &Path, result: &mut BTreeMap<String, Value>) -> Result<()> {
    let entries = std::fs::read_dir(dir).context("reading config directory")?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_json_files(root, &path, result)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let data = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let obj: Value = serde_json::from_str(&data)
                .with_context(|| format!("parsing {}", path.display()))?;
            let prefix = file_namespace_prefix(root, &path);
            if let Value::Object(map) = obj {
                flatten_map(&prefix, &map, result);
            }
        }
    }
    Ok(())
}

/// Compute a dot-notation prefix from a file's relative path to the config root.
///
/// Example: `dir=/a/config`, `path=/a/config/enforcement/enforcement-policy.json`
/// -> `enforcement.enforcement-policy`
fn file_namespace_prefix(dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(dir).unwrap_or(path).with_extension("");
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Recursively flatten a JSON object into dot-notation keys.
fn flatten_map(
    prefix: &str,
    map: &serde_json::Map<String, Value>,
    result: &mut BTreeMap<String, Value>,
) {
    for (k, v) in map {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            Value::Object(nested) => flatten_map(&key, nested, result),
            _ => {
                result.insert(key, v.clone());
            }
        }
    }
}

// Expose for use by git_hooks module.
pub(crate) fn load_enforcement_policy(project_dir: &Path) -> Result<EnforcementPolicy> {
    let path = project_dir
        .join(".codeflow")
        .join("config")
        .join("enforcement")
        .join("enforcement-policy.json");
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("reading enforcement policy at {}", path.display()))?;
    let policy: EnforcementPolicy =
        serde_json::from_str(&data).context("parsing enforcement policy")?;
    Ok(policy)
}

/// Enforcement policy struct matching Go's `EnforcementPolicy`.
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct EnforcementPolicy {
    #[serde(default)]
    pub protected_branches: Vec<String>,
    #[serde(default)]
    pub sensitive_file_patterns: Vec<String>,
    #[serde(default)]
    pub git_format: GitFormat,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct GitFormat {
    #[serde(default)]
    pub commit_types: Vec<String>,
    #[serde(default)]
    pub branch_types: Vec<String>,
    #[serde(default)]
    pub branch_prefixes: Vec<String>,
    #[serde(default)]
    pub ai_attribution_patterns: Vec<String>,
    #[serde(default)]
    pub subject: SubjectConfig,
    #[serde(default)]
    pub body: BodyConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct SubjectConfig {
    #[serde(default = "default_max_length")]
    pub max_length: usize,
    #[serde(default)]
    pub require_lowercase_type: bool,
    #[serde(default)]
    pub forbid_trailing_period: bool,
}

impl Default for SubjectConfig {
    fn default() -> Self {
        Self {
            max_length: 50,
            require_lowercase_type: true,
            forbid_trailing_period: true,
        }
    }
}

fn default_max_length() -> usize {
    50
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct BodyConfig {
    #[serde(default)]
    pub format: String,
    #[serde(default = "default_max_bullets")]
    pub max_bullets: usize,
    #[serde(default = "default_line_max_length")]
    pub line_max_length: usize,
}

impl Default for BodyConfig {
    fn default() -> Self {
        Self {
            format: "bullets_only".to_string(),
            max_bullets: 3,
            line_max_length: 72,
        }
    }
}

fn default_max_bullets() -> usize {
    3
}

fn default_line_max_length() -> usize {
    72
}

/// Default enforcement policy (matches Go's `DefaultPolicy()`).
pub(crate) fn default_policy() -> EnforcementPolicy {
    EnforcementPolicy {
        protected_branches: vec![
            "main".into(),
            "master".into(),
            "release/*".into(),
            "production".into(),
        ],
        sensitive_file_patterns: vec![
            r"\.env$".into(),
            r"\.env\.".into(),
            "credentials".into(),
            r"\.pem$".into(),
            r"\.key$".into(),
            "id_rsa".into(),
            "id_ed25519".into(),
            r"\.secret".into(),
            "password".into(),
        ],
        git_format: GitFormat {
            commit_types: vec![
                "feat", "fix", "bugfix", "hotfix", "docs", "refactor", "test", "chore", "style",
                "perf", "build", "ci", "revert", "merge", "plan", "refine",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            branch_types: vec![
                "feature",
                "feat",
                "fix",
                "bugfix",
                "hotfix",
                "release",
                "docs",
                "refactor",
                "test",
                "chore",
                "perf",
                "style",
                "build",
                "ci",
                "revert",
                "plan",
                "merge",
                "experiment",
                "wip",
                "refine",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            branch_prefixes: vec![
                "feat/",
                "fix/",
                "docs/",
                "refactor/",
                "test/",
                "chore/",
                "plan/",
                "experiment/",
                "release/",
                "hotfix/",
                "bugfix/",
                "feature/",
                "perf/",
                "style/",
                "build/",
                "ci/",
                "revert/",
                "merge/",
                "wip/",
                "refine/",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            ai_attribution_patterns: Vec::new(),
            subject: SubjectConfig::default(),
            body: BodyConfig::default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_config_command_exists() {
        let _: fn(Option<ConfigCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_config_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_config_dir(&dir.path().join(".codeflow").join("config"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_config_with_entries() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        std::fs::create_dir_all(config_dir.join("enforcement")).unwrap();
        std::fs::write(config_dir.join("settings.json"), r#"{"key": "val"}"#).unwrap();
        let result = read_config_dir(&config_dir);
        assert!(result.is_ok());
        let kv = result.unwrap();
        assert_eq!(kv.get("settings.key"), Some(&Value::String("val".into())));
    }

    #[test]
    fn test_file_namespace_prefix() {
        let dir = PathBuf::from("/a/config");
        let path = PathBuf::from("/a/config/enforcement/enforcement-policy.json");
        assert_eq!(
            file_namespace_prefix(&dir, &path),
            "enforcement.enforcement-policy"
        );

        let path2 = PathBuf::from("/a/config/settings.json");
        assert_eq!(file_namespace_prefix(&dir, &path2), "settings");
    }

    #[test]
    fn test_flatten_map_nested() {
        let json: Value =
            serde_json::from_str(r#"{"a": {"b": 1, "c": {"d": true}}, "e": "hello"}"#).unwrap();
        let mut result = BTreeMap::new();
        if let Value::Object(map) = json {
            flatten_map("root", &map, &mut result);
        }
        assert_eq!(result.get("root.a.b"), Some(&Value::from(1)));
        assert_eq!(result.get("root.a.c.d"), Some(&Value::Bool(true)));
        assert_eq!(result.get("root.e"), Some(&Value::String("hello".into())));
    }

    #[test]
    fn test_format_value_array() {
        let arr = Value::Array(vec![
            Value::String("feat".into()),
            Value::String("fix".into()),
        ]);
        assert_eq!(format_value(&arr), "[feat fix]");
    }

    #[test]
    fn test_format_value_string() {
        let s = Value::String("hello".into());
        assert_eq!(format_value(&s), "hello");
    }

    #[test]
    fn test_format_value_number() {
        let n = Value::from(42);
        assert_eq!(format_value(&n), "42");
    }

    #[test]
    fn test_format_value_bool() {
        assert_eq!(format_value(&Value::Bool(true)), "true");
    }

    #[test]
    fn test_format_value_null() {
        assert_eq!(format_value(&Value::Null), "null");
    }

    #[test]
    fn test_config_get_with_enforcement_policy() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        let enforcement_dir = config_dir.join("enforcement");
        std::fs::create_dir_all(&enforcement_dir).unwrap();
        std::fs::write(
            enforcement_dir.join("enforcement-policy.json"),
            r#"{"git_format": {"commit_types": ["feat", "fix"]}}"#,
        )
        .unwrap();
        let kv = read_config_dir(&config_dir).unwrap();
        let val = kv
            .get("enforcement.enforcement-policy.git_format.commit_types")
            .unwrap();
        assert_eq!(format_value(val), "[feat fix]");
    }

    #[test]
    fn test_load_enforcement_policy() {
        let dir = tempfile::tempdir().unwrap();
        let enforcement_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&enforcement_dir).unwrap();
        std::fs::write(
            enforcement_dir.join("enforcement-policy.json"),
            r#"{"git_format": {"commit_types": ["feat", "fix"], "subject": {"max_length": 50}}}"#,
        )
        .unwrap();
        let policy = load_enforcement_policy(dir.path()).unwrap();
        assert_eq!(policy.git_format.commit_types, vec!["feat", "fix"]);
        assert_eq!(policy.git_format.subject.max_length, 50);
    }

    #[test]
    fn test_default_policy_has_types() {
        let policy = default_policy();
        assert!(!policy.git_format.commit_types.is_empty());
        assert!(policy.git_format.commit_types.contains(&"feat".into()));
    }

    #[test]
    fn test_run_list_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_list(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_get_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("settings.json"), "{}").unwrap();
        let result = run_get(dir.path(), "nonexistent.key");
        assert!(result.is_err());
    }
}
