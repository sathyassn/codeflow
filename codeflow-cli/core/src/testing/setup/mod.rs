//! Setup wizard orchestrator for `codeflow test setup`.
//!
//! Provides the top-level setup functions that the CLI subcommand calls.
//! All writes to `test-config.json` go through the TSK-002 config-writer.

pub mod detect;
pub mod prompt;
pub mod wizard;

use std::path::Path;

use crate::testing::config::{self, TestConfig};
use crate::testing::doctor;

use prompt::PromptProvider;

/// Result of a setup operation.
#[derive(Debug)]
pub enum SetupResult {
    /// Config was written successfully.
    Written,
    /// User aborted.
    Aborted,
}

/// Run the interactive setup wizard.
///
/// # Errors
///
/// Returns `SetupError` on I/O failure, wizard error, or doctor validation failure.
pub fn run_interactive(
    project_dir: &Path,
    prompts: &dyn PromptProvider,
) -> Result<SetupResult, SetupError> {
    let config_path = config_path(project_dir);
    let existing = config_path.exists();

    let config = wizard::run_wizard(project_dir, prompts, existing).map_err(SetupError::Wizard)?;

    let config = match config {
        Some(c) => c,
        None => return Ok(SetupResult::Aborted),
    };

    // Run doctor before writing
    let checks = doctor::run_all_checks_on_config(&config, project_dir);
    let has_errors = checks.iter().any(|c| c.status == doctor::CheckStatus::Fail);
    if has_errors {
        eprintln!("Doctor found errors:");
        for check in &checks {
            if check.status == doctor::CheckStatus::Fail {
                eprintln!("  FAIL: {} — {}", check.name, check.message);
            }
        }
        eprintln!("Fix the issues above and re-run setup.");
        return Err(SetupError::DoctorFailed);
    }

    // Write via config-writer
    write_config(&config_path, &config)?;
    println!("Config written to {}", config_path.display());
    println!("Next: run `codeflow test --mode essential` to try it.");

    Ok(SetupResult::Written)
}

/// Run auto-detection and write config non-interactively.
///
/// # Errors
///
/// Returns `SetupError` on I/O or config-write failure.
pub fn run_auto(project_dir: &Path) -> Result<SetupResult, SetupError> {
    let config_path = config_path(project_dir);
    let detected = detect::detect_stacks(project_dir);

    let config = TestConfig {
        description: None,
        schema_ref: Some(".codeflow/schemas/test-config.schema.json".to_string()),
        schema_version: "1.0".to_string(),
        execution: config::ExecutionConfig::default(),
        defaults: config::DefaultsConfig::default(),
        targets: detected.into_iter().map(|d| d.config).collect(),
    };

    write_config(&config_path, &config)?;
    println!("Auto-detected config written to {}", config_path.display());

    Ok(SetupResult::Written)
}

/// Apply a named template.
///
/// # Errors
///
/// Returns `SetupError::ConfigExists` if config exists and `force` is false.
/// Returns `SetupError::TemplateNotFound` if the template name is invalid.
pub fn run_template(
    project_dir: &Path,
    template_name: &str,
    force: bool,
) -> Result<SetupResult, SetupError> {
    let config_path = config_path(project_dir);

    if config_path.exists() && !force {
        return Err(SetupError::ConfigExists(config_path));
    }

    let template_path = template_path(project_dir, template_name)?;
    if !template_path.exists() {
        return Err(SetupError::TemplateNotFound(template_name.to_string()));
    }

    // Read template, parse it, write via config-writer for consistency
    let content = std::fs::read_to_string(&template_path)?;
    let config: TestConfig =
        serde_json::from_str(&content).map_err(|e| SetupError::TemplateParse(e.to_string()))?;

    write_config(&config_path, &config)?;
    println!(
        "Template '{template_name}' written to {}",
        config_path.display()
    );

    Ok(SetupResult::Written)
}

/// Add a target to existing config via wizard.
///
/// # Errors
///
/// Returns `SetupError::NoConfigForAddTarget` if no config exists.
pub fn run_add_target(
    project_dir: &Path,
    prompts: &dyn PromptProvider,
) -> Result<SetupResult, SetupError> {
    let config_path = config_path(project_dir);

    if !config_path.exists() {
        return Err(SetupError::NoConfigForAddTarget);
    }

    let mut existing =
        config::load_test_config(&config_path).map_err(|e| SetupError::Config(e.to_string()))?;

    let target = wizard::run_add_target_wizard(project_dir, prompts).map_err(SetupError::Wizard)?;

    match target {
        Some(t) => {
            existing.targets.push(t);
            write_config(&config_path, &existing)?;
            println!("Target added to {}", config_path.display());
            Ok(SetupResult::Written)
        }
        None => Ok(SetupResult::Aborted),
    }
}

/// List available templates to stdout.
///
/// # Errors
///
/// Returns `SetupError::Io` on filesystem errors.
pub fn list_templates(project_dir: &Path) -> Result<(), SetupError> {
    let templates = get_template_list(project_dir)?;
    for (name, description) in &templates {
        println!("{name}  — {description}");
    }
    Ok(())
}

/// Get sorted list of (name, description) for all templates.
///
/// # Errors
///
/// Returns `SetupError::Io` on filesystem errors.
pub fn get_template_list(project_dir: &Path) -> Result<Vec<(String, String)>, SetupError> {
    let template_dir = project_dir
        .join(".codeflow")
        .join("templates")
        .join("test-config");
    if !template_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&template_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let description = read_template_description(&path);
            entries.push((name, description));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Write minimal.json template (for init --non-interactive).
///
/// # Errors
///
/// Returns `SetupError` on I/O or config-write failure.
pub fn write_minimal_config(project_dir: &Path) -> Result<(), SetupError> {
    let config_path = config_path(project_dir);

    // Ensure parent directories exist
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = TestConfig {
        description: None,
        schema_ref: Some(".codeflow/schemas/test-config.schema.json".to_string()),
        schema_version: "1.0".to_string(),
        execution: config::ExecutionConfig::default(),
        defaults: config::DefaultsConfig::default(),
        targets: Vec::new(),
    };

    write_config(&config_path, &config)?;
    Ok(())
}

fn read_template_description(path: &Path) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    parsed
        .get("_description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn config_path(project_dir: &Path) -> std::path::PathBuf {
    project_dir
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json")
}

fn template_path(project_dir: &Path, name: &str) -> Result<std::path::PathBuf, SetupError> {
    // Reject path separators and traversal sequences to prevent arbitrary file read.
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(SetupError::TemplateNotFound(name.to_string()));
    }
    Ok(project_dir
        .join(".codeflow")
        .join("templates")
        .join("test-config")
        .join(name))
}

fn write_config(path: &Path, config: &TestConfig) -> Result<(), SetupError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    config::write_test_config(path, config).map_err(|e| SetupError::Config(e.to_string()))
}

/// Errors from setup operations.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("wizard error: {0}")]
    Wizard(#[from] wizard::WizardError),

    #[error("config error: {0}")]
    Config(String),

    #[error("template parse error: {0}")]
    TemplateParse(String),

    #[error("doctor validation failed")]
    DoctorFailed,

    #[error("Config already exists at {0}. Use --force to overwrite.")]
    ConfigExists(std::path::PathBuf),

    #[error("Template not found. Run `codeflow test setup --list` to see available templates.")]
    TemplateNotFound(String),

    #[error("No config exists. Run `codeflow test setup` first.")]
    NoConfigForAddTarget,

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::setup::prompt::ScriptedPromptProvider;

    #[test]
    fn config_path_is_correct() {
        let path = config_path(Path::new("/repo"));
        assert_eq!(
            path,
            std::path::PathBuf::from("/repo/.codeflow/config/testing/test-config.json")
        );
    }

    #[test]
    fn template_path_is_correct() {
        let path = template_path(Path::new("/repo"), "minimal.json").unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/repo/.codeflow/templates/test-config/minimal.json")
        );
    }

    #[test]
    fn run_auto_creates_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        let result = run_auto(dir.path()).unwrap();
        assert!(matches!(result, SetupResult::Written));
        assert!(config_path(dir.path()).exists());
    }

    #[test]
    fn run_auto_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_auto(dir.path()).unwrap();
        assert!(matches!(result, SetupResult::Written));
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn run_template_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        // Create a config
        write_minimal_config(dir.path()).unwrap();
        // Create template dir + file
        let tpl_dir = dir.path().join(".codeflow/templates/test-config");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        std::fs::write(
            tpl_dir.join("minimal.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();

        let result = run_template(dir.path(), "minimal.json", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Config already exists"), "got: {err}");
    }

    #[test]
    fn run_template_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(dir.path(), "nonexistent.json", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(dir.path(), "../../etc/passwd", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(dir.path(), "/etc/passwd", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_backslash_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(dir.path(), "..\\..\\etc\\passwd", false);
        assert!(result.is_err());
    }

    #[test]
    fn template_path_rejects_dot_dot() {
        let result = template_path(Path::new("/repo"), "../secret.json");
        assert!(result.is_err());
    }

    #[test]
    fn add_target_no_config_errors() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = ScriptedPromptProvider::new(vec![]);
        let result = run_add_target(dir.path(), &prompts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No config exists"), "got: {err}");
    }

    #[test]
    fn write_minimal_config_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_config(dir.path()).unwrap();
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert_eq!(config.schema_version, "1.0");
        assert!(config.targets.is_empty());
    }

    #[test]
    fn list_templates_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let list = get_template_list(dir.path()).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn interactive_abort_returns_aborted() {
        let dir = tempfile::tempdir().unwrap();
        // Write existing config
        write_minimal_config(dir.path()).unwrap();
        let prompts = ScriptedPromptProvider::new(vec!["a"]);
        let result = run_interactive(dir.path(), &prompts).unwrap();
        assert!(matches!(result, SetupResult::Aborted));
    }

    /// Find the project root for template round-trip tests.
    fn find_project_root() -> std::path::PathBuf {
        let mut dir = std::env::current_dir().unwrap();
        loop {
            if dir
                .join(".codeflow")
                .join("templates")
                .join("test-config")
                .is_dir()
            {
                return dir;
            }
            assert!(
                dir.pop(),
                "cannot find project root with .codeflow/templates/test-config"
            );
        }
    }

    /// Helper: byte-equal round-trip test for a single template.
    fn assert_template_roundtrip(template_name: &str) {
        let root = find_project_root();
        let template_path = root
            .join(".codeflow/templates/test-config")
            .join(template_name);

        let original_bytes = std::fs::read(&template_path).unwrap_or_else(|e| {
            panic!("cannot read template {template_name}: {e}");
        });

        // Load via config loader
        let loaded = config::load_test_config(&template_path).unwrap_or_else(|e| {
            panic!("cannot load template {template_name}: {e}");
        });

        // Write via config-writer to a tempfile
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join(template_name);
        config::write_test_config(&out_path, &loaded).unwrap();

        let written_bytes = std::fs::read(&out_path).unwrap();

        assert_eq!(
            original_bytes,
            written_bytes,
            "byte-equal round-trip failed for {template_name}.\n\
             Original ({} bytes):\n{}\n\nWritten ({} bytes):\n{}",
            original_bytes.len(),
            String::from_utf8_lossy(&original_bytes),
            written_bytes.len(),
            String::from_utf8_lossy(&written_bytes),
        );
    }

    #[test]
    fn template_roundtrip_minimal() {
        assert_template_roundtrip("minimal.json");
    }

    #[test]
    fn template_roundtrip_single_target_basic() {
        assert_template_roundtrip("single-target-basic.json");
    }

    #[test]
    fn template_roundtrip_single_target_with_coverage() {
        assert_template_roundtrip("single-target-with-coverage.json");
    }

    #[test]
    fn template_roundtrip_monorepo_multi_target() {
        assert_template_roundtrip("monorepo-multi-target.json");
    }

    #[test]
    fn template_roundtrip_hooks_escape_hatch() {
        assert_template_roundtrip("hooks-escape-hatch.json");
    }

    #[test]
    fn template_roundtrip_example_node() {
        assert_template_roundtrip("example-node.json");
    }

    #[test]
    fn template_roundtrip_example_python() {
        assert_template_roundtrip("example-python.json");
    }

    #[test]
    fn template_roundtrip_example_go() {
        assert_template_roundtrip("example-go.json");
    }

    #[test]
    fn template_roundtrip_example_rust() {
        assert_template_roundtrip("example-rust.json");
    }

    // ── Coverage gap tests ───────────────────────────────────────────────

    #[test]
    fn run_template_success_from_real_templates() {
        let root = find_project_root();
        let dir = tempfile::tempdir().unwrap();
        // Copy a template into the tempdir's template location
        let tpl_dir = dir.path().join(".codeflow/templates/test-config");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        let src = root.join(".codeflow/templates/test-config/minimal.json");
        std::fs::copy(&src, tpl_dir.join("minimal.json")).unwrap();

        let result = run_template(dir.path(), "minimal.json", false).unwrap();
        assert!(matches!(result, SetupResult::Written));
        assert!(config_path(dir.path()).exists());
    }

    #[test]
    fn run_template_force_overwrites() {
        let root = find_project_root();
        let dir = tempfile::tempdir().unwrap();
        // Create existing config
        write_minimal_config(dir.path()).unwrap();
        // Copy template
        let tpl_dir = dir.path().join(".codeflow/templates/test-config");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        let src = root.join(".codeflow/templates/test-config/single-target-basic.json");
        std::fs::copy(&src, tpl_dir.join("single-target-basic.json")).unwrap();

        let result = run_template(dir.path(), "single-target-basic.json", true).unwrap();
        assert!(matches!(result, SetupResult::Written));
        // Verify config now has a target (overwritten from minimal)
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert_eq!(config.targets.len(), 1);
    }

    #[test]
    fn run_add_target_success() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_config(dir.path()).unwrap();
        // Provide: name, runner, cwd, essential cmd, full cmd
        let prompts = ScriptedPromptProvider::new(vec![
            "my-target",
            "cargo",
            ".",
            "cargo test",
            "cargo test --all",
        ]);
        let result = run_add_target(dir.path(), &prompts).unwrap();
        assert!(matches!(result, SetupResult::Written));
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "my-target");
    }

    #[test]
    fn list_templates_with_real_templates() {
        let root = find_project_root();
        // list_templates prints to stdout; just verify no error
        let result = list_templates(&root);
        assert!(result.is_ok());
    }

    #[test]
    fn get_template_list_with_real_templates() {
        let root = find_project_root();
        let list = get_template_list(&root).unwrap();
        assert!(
            list.len() >= 9,
            "expected >=9 templates, got {}",
            list.len()
        );
        // Verify sorted
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "templates not sorted");
        // Verify all have descriptions
        for (name, desc) in &list {
            assert!(!desc.is_empty(), "template {name} has empty description");
        }
    }

    #[test]
    fn interactive_success_no_existing_no_stacks() {
        let dir = tempfile::tempdir().unwrap();
        // No existing config, no sentinel files -> wizard creates empty config
        let prompts = ScriptedPromptProvider::new(vec![]);
        let result = run_interactive(dir.path(), &prompts).unwrap();
        assert!(matches!(result, SetupResult::Written));
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn interactive_success_with_detected_stack() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        // "y" to confirm rust-core target, then 4 empty strings to accept defaults
        // (essential mode, full mode, report path, coverage path)
        let prompts = ScriptedPromptProvider::new(vec!["y", "", "", "", ""]);
        let result = run_interactive(dir.path(), &prompts).unwrap();
        assert!(matches!(result, SetupResult::Written));
        let config = config::load_test_config(&config_path(dir.path())).unwrap();
        assert_eq!(config.targets.len(), 1);
    }

    #[test]
    fn interactive_replace_existing() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_config(dir.path()).unwrap();
        // "r" to replace, no stacks detected -> empty config
        let prompts = ScriptedPromptProvider::new(vec!["r"]);
        let result = run_interactive(dir.path(), &prompts).unwrap();
        assert!(matches!(result, SetupResult::Written));
    }
}
