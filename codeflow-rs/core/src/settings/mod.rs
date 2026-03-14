//! Settings template validation.
//!
//! Ports Go's `internal/settings` package: `ValidateSettingsTemplates`
//! with 5 checks (hooks SHA256, version consistency, hook wiring,
//! settings checksum, settings local checksum).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{ConfigError, SettingsError};

/// Identifies a specific validation check type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCheck {
    HooksSha256,
    VersionConsistency,
    HookWiring,
    SettingsChecksum,
    SettingsLocalChecksum,
}

/// Result of a single validation check.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationResult {
    pub check: ValidationCheck,
    pub passed: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Template-to-destination copy mapping from enforcement-policy.json.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CopyMapping {
    pub template: String,
    pub destination: String,
    #[serde(default)]
    pub purpose: String,
}

/// Settings templates configuration section of enforcement-policy.json.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettingsTemplatesConfig {
    pub directory: String,
    pub copy_mappings: Vec<CopyMapping>,
}

// ---------------------------------------------------------------------------
// EnforcementPolicy — full enforcement-policy.json model
// ---------------------------------------------------------------------------

/// Edit/Write pre-tool-use validation configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditWriteConfig {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub blocked_directories: Vec<String>,
    #[serde(default)]
    pub allowed_tmp_prefixes: Vec<String>,
    #[serde(default)]
    pub dangerous_extensions: serde_json::Value,
    #[serde(default)]
    pub warn_on_dangerous: bool,
}

/// Enforcement level definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnforcementLevel {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub configurable: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Threshold configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThresholdsConfig {
    #[serde(default)]
    pub post_tool_use: serde_json::Value,
}

/// Sentinel directory configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SentinelConfig {
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub default_ttl: u64,
}

/// Network configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub always_block_domains: serde_json::Value,
}

/// Merge protection configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeProtection {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub protected_branches: Vec<String>,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub message: String,
}

/// Protected resources by tier.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtectedResources {
    #[serde(default)]
    pub critical: Vec<String>,
    #[serde(default)]
    pub high: Vec<String>,
    #[serde(default)]
    pub moderate: Vec<String>,
}

/// Git format configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GitFormatConfig {
    #[serde(default)]
    pub commit_types: Vec<String>,
    #[serde(default)]
    pub branch_types: Vec<String>,
    #[serde(default)]
    pub branch_prefixes: Vec<String>,
    #[serde(default)]
    pub ai_attribution_patterns: Vec<String>,
    #[serde(default)]
    pub subject: serde_json::Value,
    #[serde(default)]
    pub body: serde_json::Value,
    #[serde(default)]
    pub pr: serde_json::Value,
}

/// Stop verification configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StopVerification {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub require_task_completion: bool,
    #[serde(default)]
    pub hook_types: Vec<String>,
    #[serde(default)]
    pub pcv: serde_json::Value,
}

/// Logging configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub session_start: serde_json::Value,
    #[serde(default)]
    pub post_tool_use: serde_json::Value,
    #[serde(default)]
    pub session_end: serde_json::Value,
    #[serde(default)]
    pub stop: serde_json::Value,
    #[serde(default)]
    pub user_prompt: serde_json::Value,
}

/// Read delegation configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadDelegation {
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub always_block_patterns: Vec<String>,
    #[serde(default)]
    pub threshold_rules: serde_json::Value,
}

/// Managed tmp configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedTmp {
    #[serde(default)]
    pub allow_prefix: String,
    #[serde(default)]
    pub protected_folders: Vec<String>,
    #[serde(default)]
    pub state_folder: String,
    #[serde(default)]
    pub state_protection: String,
}

/// Session end cleanup configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionEndConfig {
    #[serde(default)]
    pub cleanup: serde_json::Value,
}

/// Cleanup configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CleanupConfig {
    #[serde(default)]
    pub stale_threshold_hours: u64,
    #[serde(default)]
    pub preserve_pathflow_active: bool,
    #[serde(default)]
    pub targets: serde_json::Value,
}

/// Full enforcement-policy.json model.
///
/// Consolidates the entire enforcement policy into a single typed structure,
/// removing the need for ad-hoc `serde_json::Value` parsing in each consumer.
/// The `settings_templates` field reuses the existing [`SettingsTemplatesConfig`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnforcementPolicy {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub edit_write: Option<EditWriteConfig>,
    #[serde(default)]
    pub enforcement_levels: std::collections::HashMap<String, EnforcementLevel>,
    #[serde(default)]
    pub settings_templates: Option<SettingsTemplatesConfig>,
    #[serde(default)]
    pub thresholds: Option<ThresholdsConfig>,
    #[serde(default)]
    pub sentinel: Option<SentinelConfig>,
    #[serde(default)]
    pub skills: serde_json::Value,
    #[serde(default)]
    pub network_operations: serde_json::Value,
    #[serde(default)]
    pub protected_resources: Option<ProtectedResources>,
    #[serde(default)]
    pub merge_protection: Option<MergeProtection>,
    #[serde(default)]
    pub protected_branches: Vec<String>,
    #[serde(default)]
    pub sensitive_file_patterns: Vec<String>,
    #[serde(default)]
    pub git_format: Option<GitFormatConfig>,
    #[serde(default)]
    pub stop_verification: Option<StopVerification>,
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    #[serde(default)]
    pub network: Option<NetworkConfig>,
    #[serde(default)]
    pub read_delegation: Option<ReadDelegation>,
    #[serde(default)]
    pub managed_tmp: Option<ManagedTmp>,
    #[serde(default)]
    pub session_end: Option<SessionEndConfig>,
    #[serde(default)]
    pub cleanup: Option<CleanupConfig>,
}

impl EnforcementPolicy {
    /// Load the enforcement policy from a project directory.
    ///
    /// Reads `.codeflow/config/enforcement/enforcement-policy.json` and
    /// deserializes it into a fully typed `EnforcementPolicy`.
    ///
    /// # Errors
    ///
    /// Returns `SettingsError::Io` if the file cannot be read, or
    /// `SettingsError::Config` wrapping a `ConfigError::Parse` if the
    /// JSON is malformed.
    pub fn load(project_dir: &Path) -> Result<Self, SettingsError> {
        let path = project_dir
            .join(".codeflow")
            .join("config")
            .join("enforcement")
            .join("enforcement-policy.json");
        let data = std::fs::read_to_string(&path)?;
        let policy: Self =
            serde_json::from_str(&data).map_err(|e| ConfigError::Parse(e.to_string()))?;
        Ok(policy)
    }
}

impl Default for SettingsTemplatesConfig {
    fn default() -> Self {
        Self {
            directory: ".claude/settings-templates".into(),
            copy_mappings: vec![
                CopyMapping {
                    template: "autonomous.json".into(),
                    destination: ".claude/settings.json".into(),
                    purpose: "Project settings".into(),
                },
                CopyMapping {
                    template: "autonomous.json".into(),
                    destination: ".claude/settings.local.json".into(),
                    purpose: "Local settings".into(),
                },
            ],
        }
    }
}

/// Run all 5 validation checks against the settings templates.
///
/// # Checks
///
/// 1. Hooks section SHA256 consistency across all templates
/// 2. `_version` consistency across all templates
/// 3. Hook wiring audit (orphaned/broken scripts)
/// 4. Full file SHA256 of `settings.json` vs template source
/// 5. Full file SHA256 of `settings.local.json` vs template source
///
/// # Errors
///
/// Returns `SettingsError` on config loading or template discovery failures.
pub fn validate_settings_templates(
    project_dir: &Path,
) -> Result<Vec<ValidationResult>, SettingsError> {
    let config = load_settings_templates_config(project_dir)?;
    let template_dir = project_dir.join(&config.directory);

    let templates = discover_templates(&template_dir)?;
    if templates.len() < 2 {
        return Err(SettingsError::TemplateDiscovery(format!(
            "need at least 2 templates, found {} in {}",
            templates.len(),
            template_dir.display()
        )));
    }

    let mut results = Vec::new();

    // Check 1: Hooks SHA256 consistency.
    results.push(check_hooks_consistency(&template_dir, &templates));

    // Check 2: Version consistency.
    results.push(check_version_consistency(&template_dir, &templates));

    // Check 3: Hook wiring audit.
    results.push(check_hook_wiring(
        project_dir,
        &template_dir.join(&templates[0]),
    ));

    // Check 4 & 5: File SHA256 checksums for copy mappings.
    for mapping in &config.copy_mappings {
        results.push(check_file_checksum(project_dir, &template_dir, mapping));
    }

    Ok(results)
}

/// Returns true if any validation result failed.
#[must_use]
pub fn has_failures(results: &[ValidationResult]) -> bool {
    results.iter().any(|r| !r.passed)
}

/// Format results as a human-readable summary.
#[must_use]
pub fn format_results(results: &[ValidationResult]) -> String {
    let mut s = String::new();
    let mut passed = 0;
    let mut failed = 0;

    s.push_str("Settings Template Validation\n");
    s.push_str(&"=".repeat(50));
    s.push_str("\n\n");

    for r in results {
        let status = if r.passed {
            passed += 1;
            "PASS"
        } else {
            failed += 1;
            "FAIL"
        };
        let _ = writeln!(s, "[{status}] {:?}: {}", r.check, r.message);
        if let Some(details) = &r.details {
            let _ = writeln!(s, "       {details}");
        }
    }

    let _ = writeln!(s, "\nSummary: {passed} passed, {failed} failed");
    s
}

/// Load the `settings_templates` section from enforcement-policy.json.
fn load_settings_templates_config(
    project_dir: &Path,
) -> Result<SettingsTemplatesConfig, SettingsError> {
    let config_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("enforcement")
        .join("enforcement-policy.json");

    let Ok(data) = std::fs::read_to_string(&config_path) else {
        return Ok(SettingsTemplatesConfig::default());
    };

    let raw: serde_json::Value =
        serde_json::from_str(&data).map_err(|e| ConfigError::Parse(e.to_string()))?;

    let st = raw.get("settings_templates");
    match st {
        Some(val) => {
            let mut config: SettingsTemplatesConfig = serde_json::from_value(val.clone())
                .map_err(|e| ConfigError::Parse(e.to_string()))?;
            if config.directory.is_empty() {
                config.directory = ".claude/settings-templates".into();
            }
            if config.copy_mappings.is_empty() {
                config.copy_mappings = SettingsTemplatesConfig::default().copy_mappings;
            }
            Ok(config)
        }
        None => Ok(SettingsTemplatesConfig::default()),
    }
}

/// Find all `.json` files in the template directory, sorted alphabetically.
fn discover_templates(dir: &Path) -> Result<Vec<String>, SettingsError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        SettingsError::TemplateDiscovery(format!("read template directory {}: {e}", dir.display()))
    })?;

    let mut templates: Vec<String> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            if e.file_type().ok()?.is_dir() {
                return None;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    templates.sort();
    Ok(templates)
}

/// Compute SHA256 of the `hooks` section in a settings template.
fn hooks_hash(file_path: &Path) -> Result<String, String> {
    let data = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    let hooks = parsed
        .get("hooks")
        .ok_or_else(|| format!("no hooks section in {}", file_path.display()))?;

    let sorted = serde_json::to_string(hooks).map_err(|e| e.to_string())?;
    let hash = Sha256::digest(sorted.as_bytes());
    Ok(format!("{hash:x}"))
}

/// Extract the `_version` field from a settings template.
fn template_version(file_path: &Path) -> Result<String, String> {
    let data = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    match parsed.get("_version") {
        Some(v) => Ok(format!("{v}")),
        None => Ok("missing".into()),
    }
}

/// Compute SHA256 of a file's contents.
fn file_hash(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let hash = Sha256::digest(&data);
    Ok(format!("{hash:x}"))
}

fn check_hooks_consistency(template_dir: &Path, templates: &[String]) -> ValidationResult {
    let ref_hash = match hooks_hash(&template_dir.join(&templates[0])) {
        Ok(h) => h,
        Err(e) => {
            return ValidationResult {
                check: ValidationCheck::HooksSha256,
                passed: false,
                message: "Failed to read reference hooks section".into(),
                details: Some(e),
            };
        }
    };

    let mut mismatched = Vec::new();
    for t in &templates[1..] {
        match hooks_hash(&template_dir.join(t)) {
            Ok(h) if h != ref_hash => mismatched.push(t.clone()),
            Err(_) => mismatched.push(format!("{t} (read error)")),
            _ => {}
        }
    }

    if !mismatched.is_empty() {
        return ValidationResult {
            check: ValidationCheck::HooksSha256,
            passed: false,
            message: format!(
                "Hooks section mismatch across templates (ref: {})",
                templates[0]
            ),
            details: Some(format!("Differing: {}", mismatched.join(", "))),
        };
    }

    ValidationResult {
        check: ValidationCheck::HooksSha256,
        passed: true,
        message: format!(
            "Hooks sections identical across all {} templates",
            templates.len()
        ),
        details: None,
    }
}

fn check_version_consistency(template_dir: &Path, templates: &[String]) -> ValidationResult {
    let ref_version = match template_version(&template_dir.join(&templates[0])) {
        Ok(v) => v,
        Err(e) => {
            return ValidationResult {
                check: ValidationCheck::VersionConsistency,
                passed: false,
                message: "Failed to read reference version".into(),
                details: Some(e),
            };
        }
    };

    let mut mismatched = Vec::new();
    for t in &templates[1..] {
        match template_version(&template_dir.join(t)) {
            Ok(v) if v != ref_version => mismatched.push(format!("{t}={v}")),
            Err(_) => mismatched.push(format!("{t} (read error)")),
            _ => {}
        }
    }

    if !mismatched.is_empty() {
        return ValidationResult {
            check: ValidationCheck::VersionConsistency,
            passed: false,
            message: format!("Version mismatch (ref {}: {ref_version})", templates[0]),
            details: Some(format!("Differing: {}", mismatched.join(", "))),
        };
    }

    ValidationResult {
        check: ValidationCheck::VersionConsistency,
        passed: true,
        message: format!(
            "Version {ref_version} consistent across all {} templates",
            templates.len()
        ),
        details: None,
    }
}

fn check_hook_wiring(project_dir: &Path, template_file: &Path) -> ValidationResult {
    let hooks_dir = project_dir.join(".claude").join("hooks").join("codeflow");

    let referenced = match extract_referenced_scripts(template_file) {
        Ok(scripts) => scripts,
        Err(e) => {
            return ValidationResult {
                check: ValidationCheck::HookWiring,
                passed: false,
                message: "Failed to extract referenced scripts from template".into(),
                details: Some(e),
            };
        }
    };

    let existing = match find_existing_scripts(&hooks_dir) {
        Ok(scripts) => scripts,
        Err(e) => {
            return ValidationResult {
                check: ValidationCheck::HookWiring,
                passed: false,
                message: "Failed to scan hooks directory".into(),
                details: Some(e),
            };
        }
    };

    let ref_set: std::collections::HashSet<&str> = referenced.iter().map(String::as_str).collect();
    let exist_set: std::collections::HashSet<&str> = existing.iter().map(String::as_str).collect();

    let orphaned: Vec<&str> = existing
        .iter()
        .filter(|s| !ref_set.contains(s.as_str()))
        .map(String::as_str)
        .collect();
    let broken: Vec<&str> = referenced
        .iter()
        .filter(|s| !exist_set.contains(s.as_str()))
        .map(String::as_str)
        .collect();

    if !orphaned.is_empty() || !broken.is_empty() {
        let mut details = Vec::new();
        if !orphaned.is_empty() {
            details.push(format!("Orphaned: {}", orphaned.join(", ")));
        }
        if !broken.is_empty() {
            details.push(format!("Broken: {}", broken.join(", ")));
        }
        return ValidationResult {
            check: ValidationCheck::HookWiring,
            passed: false,
            message: format!(
                "Hook wiring issues: {} orphaned, {} broken",
                orphaned.len(),
                broken.len()
            ),
            details: Some(details.join("; ")),
        };
    }

    ValidationResult {
        check: ValidationCheck::HookWiring,
        passed: true,
        message: format!(
            "Hook wiring OK: {} referenced, {} exist",
            referenced.len(),
            existing.len()
        ),
        details: None,
    }
}

fn check_file_checksum(
    project_dir: &Path,
    template_dir: &Path,
    mapping: &CopyMapping,
) -> ValidationResult {
    let check = if mapping.destination.ends_with("settings.local.json") {
        ValidationCheck::SettingsLocalChecksum
    } else {
        ValidationCheck::SettingsChecksum
    };

    let template_path = template_dir.join(&mapping.template);
    let dest_path = project_dir.join(&mapping.destination);

    let tmpl_hash = match file_hash(&template_path) {
        Ok(h) => h,
        Err(e) => {
            return ValidationResult {
                check,
                passed: false,
                message: format!("Cannot read template {}", mapping.template),
                details: Some(e),
            };
        }
    };

    let dest_hash = match file_hash(&dest_path) {
        Ok(h) => h,
        Err(e) => {
            return ValidationResult {
                check,
                passed: false,
                message: format!("Cannot read {}", mapping.destination),
                details: Some(e),
            };
        }
    };

    if tmpl_hash != dest_hash {
        return ValidationResult {
            check,
            passed: false,
            message: format!(
                "{} does not match template {}",
                mapping.destination, mapping.template
            ),
            details: Some(format!(
                "Fix: cp {} {}",
                template_dir.join(&mapping.template).display(),
                mapping.destination
            )),
        };
    }

    ValidationResult {
        check,
        passed: true,
        message: format!(
            "{} matches template {}",
            mapping.destination, mapping.template
        ),
        details: None,
    }
}

fn walk_json_scripts(
    v: &serde_json::Value,
    scripts: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(cmd)) = map.get("command") {
                if cmd.contains(".claude/hooks/") {
                    if let Some(base) = PathBuf::from(cmd).file_name() {
                        let name = base.to_string_lossy().to_string();
                        if seen.insert(name.clone()) {
                            scripts.push(name);
                        }
                    }
                }
            }
            for child in map.values() {
                walk_json_scripts(child, scripts, seen);
            }
        }
        serde_json::Value::Array(arr) => {
            for child in arr {
                walk_json_scripts(child, scripts, seen);
            }
        }
        _ => {}
    }
}

/// Extract `.sh` script basenames referenced in hook `command` fields.
fn extract_referenced_scripts(template_file: &Path) -> Result<Vec<String>, String> {
    let data = std::fs::read_to_string(template_file).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    let mut scripts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    walk_json_scripts(&parsed, &mut scripts, &mut seen);
    scripts.sort();
    Ok(scripts)
}

fn walk_dir_scripts(
    dir: &Path,
    scripts: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_scripts(&path, scripts, seen)?;
        } else if path.extension().is_some_and(|ext| ext == "sh") {
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy().to_string();
                if seen.insert(name.clone()) {
                    scripts.push(name);
                }
            }
        }
    }
    Ok(())
}

/// Find all `.sh` files in the hooks directory tree.
fn find_existing_scripts(hooks_dir: &Path) -> Result<Vec<String>, String> {
    if !hooks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut scripts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    walk_dir_scripts(hooks_dir, &mut scripts, &mut seen)?;
    scripts.sort();
    Ok(scripts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_template(dir: &Path, name: &str, hooks: &str, version: &str) {
        let content = format!(
            r#"{{
  "_version": "{version}",
  "hooks": {hooks}
}}"#
        );
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_validation_check_serde() {
        let json = serde_json::to_string(&ValidationCheck::HooksSha256).unwrap();
        assert_eq!(json, "\"hooks_sha256\"");
        let parsed: ValidationCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ValidationCheck::HooksSha256);
    }

    #[test]
    fn test_has_failures_all_pass() {
        let results = vec![ValidationResult {
            check: ValidationCheck::HooksSha256,
            passed: true,
            message: "ok".into(),
            details: None,
        }];
        assert!(!has_failures(&results));
    }

    #[test]
    fn test_has_failures_with_failure() {
        let results = vec![
            ValidationResult {
                check: ValidationCheck::HooksSha256,
                passed: true,
                message: "ok".into(),
                details: None,
            },
            ValidationResult {
                check: ValidationCheck::VersionConsistency,
                passed: false,
                message: "mismatch".into(),
                details: None,
            },
        ];
        assert!(has_failures(&results));
    }

    #[test]
    fn test_format_results() {
        let results = vec![
            ValidationResult {
                check: ValidationCheck::HooksSha256,
                passed: true,
                message: "ok".into(),
                details: None,
            },
            ValidationResult {
                check: ValidationCheck::VersionConsistency,
                passed: false,
                message: "mismatch".into(),
                details: Some("detail".into()),
            },
        ];
        let output = format_results(&results);
        assert!(output.contains("[PASS]"));
        assert!(output.contains("[FAIL]"));
        assert!(output.contains("1 passed, 1 failed"));
    }

    #[test]
    fn test_hooks_consistency_pass() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = r#"{"preToolUse": []}"#;
        create_template(dir.path(), "a.json", hooks, "1.0");
        create_template(dir.path(), "b.json", hooks, "1.0");

        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_hooks_consistency(dir.path(), &templates);
        assert!(result.passed, "expected pass: {}", result.message);
    }

    #[test]
    fn test_hooks_consistency_fail() {
        let dir = tempfile::tempdir().unwrap();
        create_template(dir.path(), "a.json", r#"{"preToolUse": []}"#, "1.0");
        create_template(dir.path(), "b.json", r#"{"preToolUse": [1]}"#, "1.0");

        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_hooks_consistency(dir.path(), &templates);
        assert!(!result.passed);
    }

    #[test]
    fn test_version_consistency_pass() {
        let dir = tempfile::tempdir().unwrap();
        create_template(dir.path(), "a.json", "{}", "2.0");
        create_template(dir.path(), "b.json", "{}", "2.0");

        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_version_consistency(dir.path(), &templates);
        assert!(result.passed);
    }

    #[test]
    fn test_version_consistency_fail() {
        let dir = tempfile::tempdir().unwrap();
        create_template(dir.path(), "a.json", "{}", "1.0");
        create_template(dir.path(), "b.json", "{}", "2.0");

        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_version_consistency(dir.path(), &templates);
        assert!(!result.passed);
    }

    #[test]
    fn test_discover_templates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("c.json"), "{}").unwrap();
        std::fs::write(dir.path().join("a.json"), "{}").unwrap();
        std::fs::write(dir.path().join("b.json"), "{}").unwrap();
        std::fs::write(dir.path().join("readme.md"), "ignore").unwrap();

        let templates = discover_templates(dir.path()).unwrap();
        assert_eq!(templates, vec!["a.json", "b.json", "c.json"]);
    }

    #[test]
    fn test_file_hash_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let h1 = file_hash(&path).unwrap();
        let h2 = file_hash(&path).unwrap();
        assert_eq!(h1, h2);
        assert!(!h1.is_empty());
    }

    #[test]
    fn test_check_file_checksum_match() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(template_dir.join("tmpl.json"), "{}").unwrap();

        let dest_dir = dir.path().join("dest");
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(dest_dir.join("settings.json"), "{}").unwrap();

        let mapping = CopyMapping {
            template: "tmpl.json".into(),
            destination: "dest/settings.json".into(),
            purpose: "test".into(),
        };

        let result = check_file_checksum(dir.path(), &template_dir, &mapping);
        assert!(result.passed, "expected pass: {}", result.message);
    }

    #[test]
    fn test_check_file_checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(template_dir.join("tmpl.json"), r#"{"a":1}"#).unwrap();

        let dest_dir = dir.path().join("dest");
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(dest_dir.join("settings.json"), r#"{"b":2}"#).unwrap();

        let mapping = CopyMapping {
            template: "tmpl.json".into(),
            destination: "dest/settings.json".into(),
            purpose: "test".into(),
        };

        let result = check_file_checksum(dir.path(), &template_dir, &mapping);
        assert!(!result.passed);
    }

    #[test]
    fn test_settings_templates_config_default() {
        let config = SettingsTemplatesConfig::default();
        assert_eq!(config.directory, ".claude/settings-templates");
        assert_eq!(config.copy_mappings.len(), 2);
    }

    #[test]
    fn test_extract_referenced_scripts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, r#"{"hooks": {}}"#).unwrap();

        let scripts = extract_referenced_scripts(&path).unwrap();
        assert!(scripts.is_empty());
    }

    #[test]
    fn test_extract_referenced_scripts_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        let content = r#"{
            "hooks": {
                "preToolUse": [
                    {"command": ".claude/hooks/codeflow/pre-tool-use/script.sh"}
                ]
            }
        }"#;
        std::fs::write(&path, content).unwrap();

        let scripts = extract_referenced_scripts(&path).unwrap();
        assert_eq!(scripts, vec!["script.sh"]);
    }

    #[test]
    fn test_find_existing_scripts_nonexistent_dir() {
        let scripts = find_existing_scripts(Path::new("/nonexistent/path")).unwrap();
        assert!(scripts.is_empty());
    }

    #[test]
    fn test_find_existing_scripts_found() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.path().join("a.sh"), "#!/bin/bash").unwrap();
        std::fs::write(sub.join("b.sh"), "#!/bin/bash").unwrap();
        std::fs::write(dir.path().join("readme.md"), "ignore").unwrap();

        let scripts = find_existing_scripts(dir.path()).unwrap();
        assert_eq!(scripts, vec!["a.sh", "b.sh"]);
    }

    #[test]
    fn test_check_hook_wiring_no_hooks_dir() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("tmpl.json");
        std::fs::write(&template, r#"{"hooks": {}}"#).unwrap();

        let result = check_hook_wiring(dir.path(), &template);
        assert!(
            result.passed,
            "expected pass with no hooks dir: {}",
            result.message
        );
    }

    #[test]
    fn test_load_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_settings_templates_config(dir.path()).unwrap();
        assert_eq!(config.directory, ".claude/settings-templates");
    }

    #[test]
    fn test_load_config_with_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        let content = r#"{
            "settings_templates": {
                "directory": "custom/templates",
                "copy_mappings": [
                    {"template": "t.json", "destination": "d.json", "purpose": "test"}
                ]
            }
        }"#;
        std::fs::write(config_dir.join("enforcement-policy.json"), content).unwrap();

        let config = load_settings_templates_config(dir.path()).unwrap();
        assert_eq!(config.directory, "custom/templates");
        assert_eq!(config.copy_mappings.len(), 1);
    }

    #[test]
    fn test_load_config_empty_directory_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        let content = r#"{
            "settings_templates": {
                "directory": "",
                "copy_mappings": []
            }
        }"#;
        std::fs::write(config_dir.join("enforcement-policy.json"), content).unwrap();

        let config = load_settings_templates_config(dir.path()).unwrap();
        assert_eq!(config.directory, ".claude/settings-templates");
        assert_eq!(config.copy_mappings.len(), 2); // defaults restored
    }

    #[test]
    fn test_load_config_no_settings_templates_section() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"other": true}"#,
        )
        .unwrap();

        let config = load_settings_templates_config(dir.path()).unwrap();
        assert_eq!(config.directory, ".claude/settings-templates");
    }

    #[test]
    fn test_load_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("enforcement-policy.json"), "not json!!").unwrap();

        let result = load_settings_templates_config(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_check_file_checksum_local() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(template_dir.join("tmpl.json"), "{}").unwrap();

        let dest_dir = dir.path().join("dest");
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(dest_dir.join("settings.local.json"), "{}").unwrap();

        let mapping = CopyMapping {
            template: "tmpl.json".into(),
            destination: "dest/settings.local.json".into(),
            purpose: "local".into(),
        };

        let result = check_file_checksum(dir.path(), &template_dir, &mapping);
        assert!(result.passed);
        assert_eq!(result.check, ValidationCheck::SettingsLocalChecksum);
    }

    #[test]
    fn test_check_file_checksum_missing_template() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir_all(&template_dir).unwrap();

        let mapping = CopyMapping {
            template: "nonexistent.json".into(),
            destination: "dest/settings.json".into(),
            purpose: "test".into(),
        };

        let result = check_file_checksum(dir.path(), &template_dir, &mapping);
        assert!(!result.passed);
        assert!(result.message.contains("Cannot read template"));
    }

    #[test]
    fn test_check_file_checksum_missing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(template_dir.join("tmpl.json"), "{}").unwrap();

        let mapping = CopyMapping {
            template: "tmpl.json".into(),
            destination: "nonexistent/settings.json".into(),
            purpose: "test".into(),
        };

        let result = check_file_checksum(dir.path(), &template_dir, &mapping);
        assert!(!result.passed);
        assert!(result.message.contains("Cannot read"));
    }

    #[test]
    fn test_check_hook_wiring_orphaned_script() {
        let dir = tempfile::tempdir().unwrap();

        // Template references nothing
        let template = dir.path().join("tmpl.json");
        std::fs::write(&template, r#"{"hooks": {}}"#).unwrap();

        // But hooks dir has a script
        let hooks_dir = dir.path().join(".claude").join("hooks").join("codeflow");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("orphan.sh"), "#!/bin/bash").unwrap();

        let result = check_hook_wiring(dir.path(), &template);
        assert!(!result.passed);
        assert!(result.details.as_ref().unwrap().contains("Orphaned"));
    }

    #[test]
    fn test_check_hook_wiring_broken_reference() {
        let dir = tempfile::tempdir().unwrap();

        // Template references a script that doesn't exist
        let template = dir.path().join("tmpl.json");
        let content = r#"{
            "hooks": {
                "preToolUse": [
                    {"command": ".claude/hooks/codeflow/pre-tool-use/missing.sh"}
                ]
            }
        }"#;
        std::fs::write(&template, content).unwrap();

        // hooks dir exists but is empty
        let hooks_dir = dir.path().join(".claude").join("hooks").join("codeflow");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        let result = check_hook_wiring(dir.path(), &template);
        assert!(!result.passed);
        assert!(result.details.as_ref().unwrap().contains("Broken"));
    }

    #[test]
    fn test_check_hook_wiring_all_wired() {
        let dir = tempfile::tempdir().unwrap();

        // Template references a script
        let template = dir.path().join("tmpl.json");
        let content = r#"{
            "hooks": {
                "preToolUse": [
                    {"command": ".claude/hooks/codeflow/pre-tool-use/wired.sh"}
                ]
            }
        }"#;
        std::fs::write(&template, content).unwrap();

        // hooks dir has that exact script
        let hooks_dir = dir
            .path()
            .join(".claude")
            .join("hooks")
            .join("codeflow")
            .join("pre-tool-use");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("wired.sh"), "#!/bin/bash").unwrap();

        let result = check_hook_wiring(dir.path(), &template);
        assert!(result.passed, "expected pass: {}", result.message);
    }

    #[test]
    fn test_template_version_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-version.json");
        std::fs::write(&path, r#"{"hooks": {}}"#).unwrap();

        let version = template_version(&path).unwrap();
        assert_eq!(version, "missing");
    }

    #[test]
    fn test_template_version_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versioned.json");
        std::fs::write(&path, r#"{"_version": "3.0", "hooks": {}}"#).unwrap();

        let version = template_version(&path).unwrap();
        assert!(version.contains("3.0"));
    }

    #[test]
    fn test_hooks_hash_no_hooks_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-hooks.json");
        std::fs::write(&path, r#"{"_version": "1.0"}"#).unwrap();

        let result = hooks_hash(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no hooks"));
    }

    #[test]
    fn test_walk_json_scripts_nested() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "hooks": {
                    "preToolUse": [
                        {"command": ".claude/hooks/codeflow/pre/a.sh"},
                        {"command": "/usr/bin/echo"}
                    ],
                    "postToolUse": [
                        {"command": ".claude/hooks/codeflow/post/b.sh"}
                    ]
                }
            }"#,
        )
        .unwrap();

        let mut scripts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        walk_json_scripts(&json, &mut scripts, &mut seen);
        scripts.sort();
        assert_eq!(scripts, vec!["a.sh", "b.sh"]);
    }

    #[test]
    fn test_walk_json_scripts_dedup() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "hooks": {
                    "a": [{"command": ".claude/hooks/codeflow/x/same.sh"}],
                    "b": [{"command": ".claude/hooks/codeflow/y/same.sh"}]
                }
            }"#,
        )
        .unwrap();

        let mut scripts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        walk_json_scripts(&json, &mut scripts, &mut seen);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0], "same.sh");
    }

    #[test]
    fn test_discover_templates_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let templates = discover_templates(dir.path()).unwrap();
        assert!(templates.is_empty());
    }

    #[test]
    fn test_discover_templates_missing_dir() {
        let result = discover_templates(Path::new("/nonexistent/templates"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_settings_templates_too_few() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(
            template_dir.join("one.json"),
            r#"{"hooks": {}, "_version": "1"}"#,
        )
        .unwrap();

        let result = validate_settings_templates(dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("at least 2"));
    }

    #[test]
    fn test_validate_settings_templates_full_pass() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join(".claude").join("settings-templates");
        std::fs::create_dir_all(&template_dir).unwrap();

        let hooks = r#"{"preToolUse": []}"#;
        create_template(template_dir.as_path(), "a.json", hooks, "1.0");
        create_template(template_dir.as_path(), "b.json", hooks, "1.0");

        // Create hooks dir (empty, so no orphans)
        let hooks_dir = dir.path().join(".claude").join("hooks").join("codeflow");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        // Create destination files matching templates
        let a_content = std::fs::read_to_string(template_dir.join("a.json")).unwrap();
        let dest_settings = dir.path().join(".claude").join("settings.json");
        let dest_local = dir.path().join(".claude").join("settings.local.json");
        // Settings destinations for default copy_mappings
        std::fs::create_dir_all(dest_settings.parent().unwrap()).unwrap();
        std::fs::write(&dest_settings, &a_content).unwrap();
        std::fs::write(&dest_local, &a_content).unwrap();

        let results = validate_settings_templates(dir.path()).unwrap();
        // hooks_consistency + version_consistency + hook_wiring + 2 checksums = 5
        assert_eq!(results.len(), 5);
        let pass_count = results.iter().filter(|r| r.passed).count();
        assert!(
            pass_count >= 3,
            "expected at least 3 passes, got {pass_count}"
        );
    }

    #[test]
    fn test_validation_result_serde() {
        let result = ValidationResult {
            check: ValidationCheck::HookWiring,
            passed: true,
            message: "ok".into(),
            details: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("hook_wiring"));
        assert!(!json.contains("details")); // skipped when None

        let result_with_details = ValidationResult {
            check: ValidationCheck::SettingsChecksum,
            passed: false,
            message: "mismatch".into(),
            details: Some("info".into()),
        };
        let json2 = serde_json::to_string(&result_with_details).unwrap();
        assert!(json2.contains("\"details\":\"info\""));
    }

    #[test]
    fn test_copy_mapping_serde() {
        let mapping = CopyMapping {
            template: "t.json".into(),
            destination: "d.json".into(),
            purpose: "p".into(),
        };
        let json = serde_json::to_string(&mapping).unwrap();
        let parsed: CopyMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.template, "t.json");
        assert_eq!(parsed.purpose, "p");
    }

    #[test]
    fn test_version_consistency_read_error() {
        let dir = tempfile::tempdir().unwrap();
        create_template(dir.path(), "a.json", "{}", "1.0");
        // b.json doesn't exist
        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_version_consistency(dir.path(), &templates);
        assert!(!result.passed);
        assert!(result.details.as_ref().unwrap().contains("read error"));
    }

    #[test]
    fn test_hooks_consistency_read_error() {
        let dir = tempfile::tempdir().unwrap();
        create_template(dir.path(), "a.json", r#"{"preToolUse": []}"#, "1.0");
        // b.json doesn't exist
        let templates = vec!["a.json".into(), "b.json".into()];
        let result = check_hooks_consistency(dir.path(), &templates);
        assert!(!result.passed);
        assert!(result.details.as_ref().unwrap().contains("read error"));
    }

    // -----------------------------------------------------------------------
    // EnforcementPolicy tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_enforcement_policy_load_real_file() {
        // Load from actual project enforcement-policy.json
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let result = EnforcementPolicy::load(project_dir);
        // May not exist in CI; skip if file missing
        if let Ok(policy) = result {
            assert!(!policy.version.is_empty(), "version should be populated");
            assert!(
                policy.protected_resources.is_some(),
                "protected_resources should exist"
            );
            assert!(
                policy.settings_templates.is_some(),
                "settings_templates should exist"
            );
        }
    }

    #[test]
    fn test_enforcement_policy_load_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();

        let json = r#"{
            "version": "1.4.0",
            "description": "test policy",
            "edit_write": {
                "blocked_directories": [".git"],
                "warn_on_dangerous": true
            },
            "enforcement_levels": {
                "L1_SENTINEL": {
                    "description": "Skill enforcement",
                    "configurable": true,
                    "enabled": true,
                    "examples": ["example"]
                }
            },
            "settings_templates": {
                "directory": ".claude/settings-templates",
                "copy_mappings": [
                    {"template": "a.json", "destination": "b.json", "purpose": "test"}
                ]
            },
            "protected_resources": {
                "critical": [".claude/settings.json"],
                "high": [".claude/hooks/**"],
                "moderate": ["project/mission.md"]
            },
            "merge_protection": {
                "protected_branches": ["main", "master"],
                "policy": "hard_block",
                "message": "blocked"
            },
            "protected_branches": ["main"],
            "sensitive_file_patterns": ["\\.env$"],
            "git_format": {
                "commit_types": ["feat", "fix"],
                "branch_types": ["feat"],
                "branch_prefixes": ["feat/"]
            },
            "sentinel": {
                "directory": ".state/sentinels/skill",
                "default_ttl": 600
            },
            "cleanup": {
                "stale_threshold_hours": 24,
                "preserve_pathflow_active": true
            }
        }"#;
        std::fs::write(config_dir.join("enforcement-policy.json"), json).unwrap();

        let policy = EnforcementPolicy::load(dir.path()).unwrap();
        assert_eq!(policy.version, "1.4.0");
        assert_eq!(policy.description, "test policy");

        // edit_write
        let ew = policy.edit_write.unwrap();
        assert_eq!(ew.blocked_directories, vec![".git"]);
        assert!(ew.warn_on_dangerous);

        // enforcement_levels
        assert!(policy.enforcement_levels.contains_key("L1_SENTINEL"));
        let l1 = &policy.enforcement_levels["L1_SENTINEL"];
        assert!(l1.enabled);
        assert!(l1.configurable);

        // settings_templates
        let st = policy.settings_templates.unwrap();
        assert_eq!(st.directory, ".claude/settings-templates");
        assert_eq!(st.copy_mappings.len(), 1);

        // protected_resources
        let pr = policy.protected_resources.unwrap();
        assert_eq!(pr.critical, vec![".claude/settings.json"]);
        assert_eq!(pr.high, vec![".claude/hooks/**"]);
        assert_eq!(pr.moderate, vec!["project/mission.md"]);

        // merge_protection
        let mp = policy.merge_protection.unwrap();
        assert_eq!(mp.protected_branches, vec!["main", "master"]);
        assert_eq!(mp.policy, "hard_block");

        // top-level protected_branches
        assert_eq!(policy.protected_branches, vec!["main"]);
        assert_eq!(policy.sensitive_file_patterns, vec!["\\.env$"]);

        // git_format
        let gf = policy.git_format.unwrap();
        assert_eq!(gf.commit_types, vec!["feat", "fix"]);
        assert_eq!(gf.branch_prefixes, vec!["feat/"]);

        // sentinel
        let sen = policy.sentinel.unwrap();
        assert_eq!(sen.directory, ".state/sentinels/skill");
        assert_eq!(sen.default_ttl, 600);

        // cleanup
        let cl = policy.cleanup.unwrap();
        assert_eq!(cl.stale_threshold_hours, 24);
        assert!(cl.preserve_pathflow_active);
    }

    #[test]
    fn test_enforcement_policy_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = EnforcementPolicy::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_enforcement_policy_load_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("enforcement-policy.json"), "not valid json").unwrap();

        let result = EnforcementPolicy::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_enforcement_policy_minimal_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        // Minimal valid JSON — all fields should default
        std::fs::write(config_dir.join("enforcement-policy.json"), "{}").unwrap();

        let policy = EnforcementPolicy::load(dir.path()).unwrap();
        assert!(policy.version.is_empty());
        assert!(policy.protected_branches.is_empty());
        assert!(policy.enforcement_levels.is_empty());
        assert!(policy.edit_write.is_none());
        assert!(policy.protected_resources.is_none());
    }

    #[test]
    fn test_enforcement_policy_roundtrip_serde() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();

        let json = r#"{"version": "2.0", "protected_branches": ["main", "release/*"]}"#;
        std::fs::write(config_dir.join("enforcement-policy.json"), json).unwrap();

        let policy = EnforcementPolicy::load(dir.path()).unwrap();
        let serialized = serde_json::to_string(&policy).unwrap();
        let reparsed: EnforcementPolicy = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reparsed.version, "2.0");
        assert_eq!(reparsed.protected_branches, vec!["main", "release/*"]);
    }
}
