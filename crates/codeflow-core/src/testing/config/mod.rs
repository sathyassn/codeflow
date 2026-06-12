//! Test configuration loader and writer.
//!
//! Reads `.codeflow/test-config.json` into `TestConfig`, validates against
//! the JSON schema, and provides deterministic serialization for config
//! mutation subcommands.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::testing::error::TestingError;

/// Supported schema versions.
const SUPPORTED_SCHEMA_VERSIONS: &[&str] = &["1.0", "1"];

/// Top-level test configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    /// Template description (preserved for round-trip fidelity).
    #[serde(
        rename = "_description",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,

    /// Path to JSON schema for editor validation.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,

    /// Schema version (required).
    pub schema_version: String,

    /// Execution settings.
    #[serde(default)]
    pub execution: ExecutionConfig,

    /// Default coverage rules applied when targets omit them.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// Test targets.
    #[serde(default)]
    pub targets: Vec<TargetConfig>,
}

/// Execution settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Run targets in parallel.
    #[serde(default)]
    pub parallel: bool,

    /// Stop on first target failure.
    #[serde(default)]
    pub fail_fast: bool,
}

/// Default settings applied to all targets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Default coverage rules.
    #[serde(default)]
    pub coverage: Vec<CoverageRule>,
}

/// A single test target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Unique target name.
    pub name: String,

    /// Whether this target is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Working directory for commands (relative to repo root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Environment variables merged with inherited env.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// Runner type.
    pub runner: RunnerType,

    /// Mode-specific commands.
    pub modes: BTreeMap<String, ModeCommand>,

    /// Report configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<ReportConfig>,

    /// Coverage configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageConfig>,

    /// When `true` and the process runs with `CI=true` in the environment, the
    /// generic engine skips this target entirely. Lets a target opt out of the
    /// CI wall-time budget while staying enabled for local dev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_skip: Option<bool>,

    /// Human-readable explanation of why `ci_skip` is set. Surfaces in the
    /// runner's stderr output and in any ledger audit trail so reviewers can
    /// see why a target did not run in CI. Expected when `ci_skip = Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_skip_reason: Option<String>,

    /// Optional structural integrity configuration. When present, the
    /// `codeflow test structural-check` subcommand validates bidirectional
    /// source↔test mapping for this target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural: Option<StructuralConfig>,

    /// Target-level tags applied to every test file in this target.
    /// Additional per-file tags are merged (union) from `test_files`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,

    /// Explicit per-file test entries with optional tag metadata.
    /// Used by runners that enumerate test files directly (e.g. shell-scripts).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_files: Vec<TestFileEntry>,
}

/// Priority tag for a test target or individual test file.
///
/// Variants are declared in descending priority order. The derived `Ord`
/// follows declaration order so `Critical < High < Medium < Low`, which lets
/// tools show or sort by priority without extra plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Critical,
    High,
    Medium,
    Low,
}

impl Tag {
    /// Parse a tag from a string (case-insensitive). Returns None for unknown tags.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Some(Self::Critical),
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    /// Lowercase string form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An explicit test file entry.
///
/// When a target declares `test_files`, the runner operates on those specific
/// paths in addition to (or instead of) the commands in `modes`. Each entry
/// may carry its own tag set which is merged with the target-level tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFileEntry {
    /// Path to the test file, relative to target `cwd` or repo root.
    pub path: String,
    /// Per-file tags (merged with target-level tags; final set is the union).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
}

/// Structural integrity configuration.
///
/// Enables bidirectional source↔test mapping validation.
/// All fields are optional — a target with no `structural` block skips the check.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuralConfig {
    /// Glob patterns identifying source files that must have a matching test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_glob: Vec<String>,
    /// Glob patterns identifying test files that must map back to a source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_glob: Vec<String>,
    /// Ordered regex rules mapping source paths to expected test paths.
    /// The first matching rule wins; capture groups `$1`..`$9` are substituted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_map: Vec<PatternMapEntry>,
    /// Optional exclusion rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusions: Option<StructuralExclusions>,
}

/// A single source→test mapping rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMapEntry {
    /// Regex matched against the source path.
    pub source: String,
    /// Replacement template producing the expected test path.
    pub test: String,
}

/// Structural check exclusion rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuralExclusions {
    /// Source files/globs that do not require a test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_test_required: Vec<ExclusionEntry>,
    /// Test files/globs permitted to have no matching source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphan_allowed: Vec<ExclusionEntry>,
}

/// A single exclusion entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionEntry {
    /// Glob pattern matched against the repo-relative path.
    pub pattern: String,
    /// Why this entry is excluded; surfaced in structural-check reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Supported test runners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerType {
    Cargo,
    Pytest,
    Jest,
    Vitest,
    Go,
    Mocha,
    Rspec,
    Phpunit,
    Custom,
}

/// A mode command (e.g., quick, essential, full).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeCommand {
    pub command: String,
}

/// Report configuration for a target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    /// Report format.
    pub format: ReportFormat,

    /// Path to the report file (relative to cwd).
    pub path: String,

    /// When format is CTRF but emitted format is `JUnit`, triggers internal conversion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derive_from: Option<String>,
}

/// Report format enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    Junit,
    Ctrf,
}

/// Coverage configuration for a target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageConfig {
    /// Coverage format.
    pub format: CoverageFormat,

    /// Path to the coverage artifact (relative to cwd).
    pub path: String,

    /// Optional transform command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,

    /// Ordered coverage rule list.
    #[serde(default)]
    pub rules: Vec<CoverageRule>,

    /// Per-file threshold exceptions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<CoverageException>,
}

/// Coverage format enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageFormat {
    Lcov,
    Cobertura,
    IstanbulSummary,
    GoCover,
}

/// A coverage rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageRule {
    /// Scope of evaluation.
    pub scope: CoverageScope,

    /// Include globs (default: `["**/*"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,

    /// Exclude globs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// Minimum threshold (0-100).
    pub minimum: u32,
}

/// Coverage scope types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageScope {
    PerFile,
    PerPackage,
    PerModule,
    ChangedFiles,
    Global,
}

/// A per-file coverage exception.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageException {
    /// File path (repo-relative).
    pub file: String,

    /// Lowered threshold for this file.
    pub threshold: u32,

    /// Justification.
    pub reason: String,

    /// Removal condition.
    pub remove_when: String,
}

fn default_true() -> bool {
    true
}

/// Load and validate test configuration.
///
/// # Errors
///
/// Returns `TestingError::ConfigNotFound` if file doesn't exist.
/// Returns `TestingError::MissingSchemaVersion` if `schema_version` is absent.
/// Returns `TestingError::UnsupportedSchemaVersion` for unknown versions.
/// Returns `TestingError::ConfigInvalid` for schema violations.
pub fn load_test_config(path: &Path) -> Result<TestConfig, TestingError> {
    if !path.exists() {
        return Err(TestingError::ConfigNotFound(path.to_path_buf()));
    }

    let content = std::fs::read_to_string(path)?;

    // First, check schema_version before full parse
    let raw: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| TestingError::ConfigInvalid {
            path: path.to_path_buf(),
            message: format!("{e}"),
        })?;

    // Check schema_version presence and value
    match raw.get("schema_version") {
        None => return Err(TestingError::MissingSchemaVersion),
        Some(v) => {
            let version = v.as_str().unwrap_or("");
            if !SUPPORTED_SCHEMA_VERSIONS.contains(&version) {
                return Err(TestingError::UnsupportedSchemaVersion {
                    version: version.to_string(),
                    supported: SUPPORTED_SCHEMA_VERSIONS.join(", "),
                });
            }
        }
    }

    // Check for unknown fields in targets
    if let Some(targets) = raw.get("targets").and_then(|t| t.as_array()) {
        let known_target_fields = [
            "name",
            "enabled",
            "cwd",
            "env",
            "runner",
            "modes",
            "report",
            "coverage",
            "ci_skip",
            "ci_skip_reason",
            "structural",
            "tags",
            "test_files",
        ];
        for (i, target) in targets.iter().enumerate() {
            if let Some(obj) = target.as_object() {
                for key in obj.keys() {
                    if !known_target_fields.contains(&key.as_str()) {
                        return Err(TestingError::ConfigInvalid {
                            path: path.to_path_buf(),
                            message: format!("targets[{i}].{key}: unknown field"),
                        });
                    }
                }
            }
        }
    }

    // Warn about unknown top-level fields
    let known_top_level = [
        "_description",
        "$schema",
        "schema_version",
        "execution",
        "defaults",
        "targets",
    ];
    if let Some(obj) = raw.as_object() {
        for key in obj.keys() {
            if !known_top_level.contains(&key.as_str()) {
                eprintln!("warning: unknown top-level field '{key}' in test config (ignored)");
            }
        }
    }

    // Full parse into typed struct
    let config: TestConfig =
        serde_json::from_value(raw).map_err(|e| TestingError::ConfigInvalid {
            path: path.to_path_buf(),
            message: format!("{e}"),
        })?;

    // Check for duplicate target names
    let mut seen_names = std::collections::HashSet::new();
    for target in &config.targets {
        if !seen_names.insert(&target.name) {
            return Err(TestingError::DuplicateTarget(target.name.clone()));
        }
    }

    Ok(config)
}

/// Write test config with deterministic output.
///
/// Uses `BTreeMap` for stable key ordering, 2-space indent, trailing newline.
/// Byte-equivalent round-trip for no-op mutations.
///
/// # Errors
///
/// Returns `TestingError::Json` on serialization failure, `TestingError::Io` on write failure.
pub fn write_test_config(path: &Path, config: &TestConfig) -> Result<(), TestingError> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, format!("{json}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("test-config.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_load_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "execution": {"parallel": false, "fail_fast": false},
            "targets": []
        }"#,
        );
        let config = load_test_config(&path).unwrap();
        assert_eq!(config.schema_version, "1.0");
        assert!(!config.execution.parallel);
        assert!(config.targets.is_empty());
    }

    #[test]
    fn test_load_missing_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), r#"{"targets": []}"#);
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::MissingSchemaVersion => {}
            other => panic!("expected MissingSchemaVersion, got: {other}"),
        }
    }

    #[test]
    fn test_load_unsupported_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), r#"{"schema_version": "2.0", "targets": []}"#);
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::UnsupportedSchemaVersion { version, supported } => {
                assert_eq!(version, "2.0");
                assert!(supported.contains("1.0"));
            }
            other => panic!("expected UnsupportedSchemaVersion, got: {other}"),
        }
    }

    #[test]
    fn test_load_unknown_top_level_field_warns_but_accepts() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [],
            "unknown_field": true
        }"#,
        );
        // Should succeed (unknown top-level = warn, accept)
        let config = load_test_config(&path).unwrap();
        assert_eq!(config.schema_version, "1.0");
    }

    #[test]
    fn test_load_unknown_field_in_target_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{"name": "t", "runner": "cargo", "modes": {}, "unknown_stuff": true}]
        }"#,
        );
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::ConfigInvalid { message, .. } => {
                assert!(message.contains("unknown field"), "got: {message}");
                assert!(message.contains("unknown_stuff"), "got: {message}");
            }
            other => panic!("expected ConfigInvalid, got: {other}"),
        }
    }

    #[test]
    fn test_load_file_not_found() {
        let err = load_test_config(Path::new("/nonexistent/test-config.json")).unwrap_err();
        match err {
            TestingError::ConfigNotFound(p) => {
                assert_eq!(p, std::path::PathBuf::from("/nonexistent/test-config.json"));
            }
            other => panic!("expected ConfigNotFound, got: {other}"),
        }
    }

    #[test]
    fn test_load_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "not json!");
        let err = load_test_config(&path).unwrap_err();
        assert!(matches!(err, TestingError::ConfigInvalid { .. }));
    }

    #[test]
    fn test_load_duplicate_target_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [
                {"name": "rust", "runner": "cargo", "modes": {}},
                {"name": "rust", "runner": "cargo", "modes": {}}
            ]
        }"#,
        );
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::DuplicateTarget(name) => assert_eq!(name, "rust"),
            other => panic!("expected DuplicateTarget, got: {other}"),
        }
    }

    #[test]
    fn test_load_config_with_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "rust-core",
                "runner": "cargo",
                "cwd": "codeflow-cli",
                "modes": {
                    "full": {"command": "cargo nextest run"}
                },
                "report": {
                    "format": "junit",
                    "path": "target/nextest/default/junit.xml"
                },
                "coverage": {
                    "format": "lcov",
                    "path": "target/llvm-cov/lcov.info",
                    "rules": [
                        {"scope": "changed_files", "minimum": 85}
                    ],
                    "exceptions": [{
                        "file": "core/src/autorun/worker.rs",
                        "threshold": 79,
                        "reason": "process spawning",
                        "remove_when": "mock harness"
                    }]
                }
            }]
        }"#,
        );
        let config = load_test_config(&path).unwrap();
        assert_eq!(config.targets.len(), 1);
        let t = &config.targets[0];
        assert_eq!(t.name, "rust-core");
        assert_eq!(t.runner, RunnerType::Cargo);
        assert_eq!(t.cwd.as_deref(), Some("codeflow-cli"));
        assert!(t.modes.contains_key("full"));
        assert!(t.report.is_some());
        assert!(t.coverage.is_some());
        let cov = t.coverage.as_ref().unwrap();
        assert_eq!(cov.format, CoverageFormat::Lcov);
        assert_eq!(cov.rules.len(), 1);
        assert_eq!(cov.rules[0].scope, CoverageScope::ChangedFiles);
        assert_eq!(cov.rules[0].minimum, 85);
        assert_eq!(cov.exceptions.len(), 1);
    }

    #[test]
    fn test_write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-config.json");

        let config = TestConfig {
            description: None,
            schema_ref: Some(".codeflow/test-config.schema.json".to_string()),
            schema_version: "1.0".to_string(),
            execution: ExecutionConfig::default(),
            defaults: DefaultsConfig::default(),
            targets: vec![TargetConfig {
                name: "test".to_string(),
                enabled: true,
                cwd: None,
                env: BTreeMap::new(),
                runner: RunnerType::Custom,
                modes: BTreeMap::from([(
                    "full".to_string(),
                    ModeCommand {
                        command: "echo test".to_string(),
                    },
                )]),
                report: None,
                coverage: None,
                ci_skip: None,
                ci_skip_reason: None,
                structural: None,
                tags: Vec::new(),
                test_files: Vec::new(),
            }],
        };

        write_test_config(&path, &config).unwrap();
        let loaded = load_test_config(&path).unwrap();
        assert_eq!(loaded.schema_version, "1.0");
        assert_eq!(loaded.targets.len(), 1);
        assert_eq!(loaded.targets[0].name, "test");
    }

    #[test]
    fn test_runner_type_serde() {
        for (json, expected) in [
            ("\"cargo\"", RunnerType::Cargo),
            ("\"pytest\"", RunnerType::Pytest),
            ("\"jest\"", RunnerType::Jest),
            ("\"vitest\"", RunnerType::Vitest),
            ("\"go\"", RunnerType::Go),
            ("\"mocha\"", RunnerType::Mocha),
            ("\"rspec\"", RunnerType::Rspec),
            ("\"phpunit\"", RunnerType::Phpunit),
            ("\"custom\"", RunnerType::Custom),
        ] {
            let parsed: RunnerType = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);
            let back = serde_json::to_string(&parsed).unwrap();
            assert_eq!(back, json);
        }
    }

    #[test]
    fn test_coverage_format_serde() {
        for (json, expected) in [
            ("\"lcov\"", CoverageFormat::Lcov),
            ("\"cobertura\"", CoverageFormat::Cobertura),
            ("\"istanbul-summary\"", CoverageFormat::IstanbulSummary),
            ("\"go-cover\"", CoverageFormat::GoCover),
        ] {
            let parsed: CoverageFormat = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);
            let back = serde_json::to_string(&parsed).unwrap();
            assert_eq!(back, json);
        }
    }

    #[test]
    fn test_coverage_scope_serde() {
        for (json, expected) in [
            ("\"per_file\"", CoverageScope::PerFile),
            ("\"per_package\"", CoverageScope::PerPackage),
            ("\"per_module\"", CoverageScope::PerModule),
            ("\"changed_files\"", CoverageScope::ChangedFiles),
            ("\"global\"", CoverageScope::Global),
        ] {
            let parsed: CoverageScope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);
            let back = serde_json::to_string(&parsed).unwrap();
            assert_eq!(back, json);
        }
    }

    #[test]
    fn test_empty_schema_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), r#"{"schema_version": "", "targets": []}"#);
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::UnsupportedSchemaVersion { version, .. } => {
                assert_eq!(version, "");
            }
            other => panic!("expected UnsupportedSchemaVersion, got: {other}"),
        }
    }

    #[test]
    fn test_tag_serde_roundtrip() {
        for (json, expected) in [
            ("\"critical\"", Tag::Critical),
            ("\"high\"", Tag::High),
            ("\"medium\"", Tag::Medium),
            ("\"low\"", Tag::Low),
        ] {
            let parsed: Tag = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);
            let back = serde_json::to_string(&parsed).unwrap();
            assert_eq!(back, json);
        }
    }

    #[test]
    fn test_tag_parse() {
        assert_eq!(Tag::parse("critical"), Some(Tag::Critical));
        assert_eq!(Tag::parse("CRITICAL"), Some(Tag::Critical));
        assert_eq!(Tag::parse("High"), Some(Tag::High));
        assert_eq!(Tag::parse("medium"), Some(Tag::Medium));
        assert_eq!(Tag::parse("low"), Some(Tag::Low));
        assert_eq!(Tag::parse("other"), None);
        assert_eq!(Tag::parse(""), None);
    }

    #[test]
    fn test_tag_display_and_as_str() {
        assert_eq!(Tag::Critical.as_str(), "critical");
        assert_eq!(format!("{}", Tag::High), "high");
        assert_eq!(format!("{}", Tag::Medium), "medium");
        assert_eq!(format!("{}", Tag::Low), "low");
    }

    #[test]
    fn test_load_config_with_target_level_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "shell",
                "runner": "custom",
                "modes": {"full": {"command": "bash -c true"}},
                "tags": ["critical", "high"]
            }]
        }"#,
        );
        let config = load_test_config(&path).unwrap();
        assert_eq!(config.targets[0].tags, vec![Tag::Critical, Tag::High]);
    }

    #[test]
    fn test_load_config_with_test_files_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "shell",
                "runner": "custom",
                "modes": {"full": {"command": "true"}},
                "test_files": [
                    {"path": "t/a.sh", "tags": ["critical"]},
                    {"path": "t/b.sh", "tags": ["medium", "low"]},
                    {"path": "t/c.sh"}
                ]
            }]
        }"#,
        );
        let config = load_test_config(&path).unwrap();
        let files = &config.targets[0].test_files;
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "t/a.sh");
        assert_eq!(files[0].tags, vec![Tag::Critical]);
        assert_eq!(files[1].tags, vec![Tag::Medium, Tag::Low]);
        assert!(files[2].tags.is_empty());
    }

    #[test]
    fn test_load_config_with_structural_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "shell",
                "runner": "custom",
                "modes": {"full": {"command": "true"}},
                "structural": {
                    "source_glob": ["scripts/**/*.sh"],
                    "test_glob": ["tests/scripts/**/test-*.sh"],
                    "pattern_map": [
                        {
                            "source": "^scripts/([^/]+)/([^/]+)\\.sh$",
                            "test": "tests/scripts/$1/test-$2.sh"
                        }
                    ],
                    "exclusions": {
                        "no_test_required": [{"pattern": "scripts/lib/*", "reason": "lib modules"}],
                        "orphan_allowed": [{"pattern": "fixtures/framework/*"}]
                    }
                }
            }]
        }"#,
        );
        let config = load_test_config(&path).unwrap();
        let s = config.targets[0].structural.as_ref().unwrap();
        assert_eq!(s.source_glob, vec!["scripts/**/*.sh"]);
        assert_eq!(s.test_glob, vec!["tests/scripts/**/test-*.sh"]);
        assert_eq!(s.pattern_map.len(), 1);
        let exclusions = s.exclusions.as_ref().unwrap();
        assert_eq!(exclusions.no_test_required.len(), 1);
        assert_eq!(exclusions.no_test_required[0].pattern, "scripts/lib/*");
        assert_eq!(
            exclusions.no_test_required[0].reason.as_deref(),
            Some("lib modules")
        );
        assert_eq!(exclusions.orphan_allowed.len(), 1);
        assert!(exclusions.orphan_allowed[0].reason.is_none());
    }

    #[test]
    fn test_load_config_with_invalid_tag_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "shell",
                "runner": "custom",
                "modes": {"full": {"command": "true"}},
                "tags": ["bogus"]
            }]
        }"#,
        );
        let err = load_test_config(&path).unwrap_err();
        assert!(matches!(err, TestingError::ConfigInvalid { .. }));
    }

    #[test]
    fn test_load_config_unknown_field_structural_still_rejected_if_misspelled() {
        // `structurall` is not a known field; ensure misspellings still rejected.
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
            "schema_version": "1.0",
            "targets": [{
                "name": "shell",
                "runner": "custom",
                "modes": {"full": {"command": "true"}},
                "structurall": {}
            }]
        }"#,
        );
        let err = load_test_config(&path).unwrap_err();
        match err {
            TestingError::ConfigInvalid { message, .. } => {
                assert!(message.contains("structurall"), "got: {message}");
            }
            other => panic!("expected ConfigInvalid, got: {other}"),
        }
    }
}
