//! Setup wizard orchestrator for generating `.codeflow/test-config.json`.
//!
//! Provides the top-level setup functions.
//! All writes to `.codeflow/test-config.json` go through the config-writer
//! so output stays deterministic and byte-stable on round-trips.

pub mod detect;
pub mod prompt;
pub mod wizard;

use std::path::Path;

use crate::testing::config::{self, TestConfig};
use crate::testing::doctor;

use prompt::PromptProvider;

/// `$schema` reference written into generated configs. The schema ships next
/// to the config in the consumer repo (`.codeflow/test-config.schema.json`).
pub const SCHEMA_REF: &str = "test-config.schema.json";

const TEST_CONFIG_SCHEMA: &str =
    include_str!("../../../../../assets/base/testing/test-config.schema.json");

/// Result of a setup operation.
#[derive(Debug)]
pub enum SetupResult {
    /// Config was written successfully with at least one target.
    Written,
    /// Config was written, but auto-detection found no supported stack, so it
    /// has zero targets. Distinguished from [`SetupResult::Written`] so the
    /// caller reports the zero-detection outcome plainly instead of implying a
    /// stack was configured.
    WrittenNoTargets,
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
    let config_path = config_path(project_dir)?;
    let existing = config_path.exists();

    let config = wizard::run_wizard(project_dir, prompts, existing).map_err(SetupError::Wizard)?;

    let Some(config) = config else {
        return Ok(SetupResult::Aborted);
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
    let config_path = config_path(project_dir)?;
    write_schema_next_to(&config_path)?;
    if config_path.exists()
        && !config::load_test_config(&config_path)
            .map_err(|error| SetupError::Config(error.to_string()))?
            .targets
            .is_empty()
    {
        return Err(SetupError::ConfigExists(config_path));
    }
    let detected = detect::detect_stacks(project_dir)?;

    let config = TestConfig {
        description: None,
        schema_ref: Some(SCHEMA_REF.to_string()),
        schema_version: "1.0".to_string(),
        execution: config::ExecutionConfig::default(),
        defaults: config::DefaultsConfig::default(),
        targets: detected.into_iter().map(|d| d.config).collect(),
    };

    write_config(&config_path, &config)?;

    // Honesty: when detection found nothing, say so plainly rather than
    // implying a stack was configured. The empty config is still written (the
    // documented `init` contract — a fresh project starts from an empty config
    // it can extend), but the outcome is reported as zero-detection.
    if config.targets.is_empty() {
        println!(
            "No supported stack detected — wrote a config with no targets to {}. \
             Add a supported stack and rerun setup, or edit the config explicitly.",
            config_path.display()
        );
        return Ok(SetupResult::WrittenNoTargets);
    }

    println!("Auto-detected config written to {}", config_path.display());
    Ok(SetupResult::Written)
}

/// Apply a named template from `template_dir`.
///
/// The caller supplies the template directory — the CLI resolves it from the
/// embedded scaffold assets (`assets/base/testing/templates/` in this repo).
///
/// # Errors
///
/// Returns `SetupError::ConfigExists` if config exists and `force` is false.
/// Returns `SetupError::TemplateNotFound` if the template name is invalid.
pub fn run_template(
    project_dir: &Path,
    template_dir: &Path,
    template_name: &str,
    force: bool,
) -> Result<SetupResult, SetupError> {
    let template_path = template_path(template_dir, template_name)?;
    if !template_path.exists() {
        return Err(SetupError::TemplateNotFound(template_name.to_string()));
    }

    let content = std::fs::read_to_string(&template_path)?;
    run_template_content(project_dir, template_name, &content, force)
}

/// Apply a named template supplied as UTF-8 content.
///
/// This is the release-binary path: the CLI reads the template from its
/// embedded assets and passes the bytes here, while [`run_template`] remains a
/// filesystem adapter for tests and source-tree callers.
///
/// # Errors
///
/// Returns `SetupError::ConfigExists` if config exists and `force` is false.
/// Returns `SetupError::TemplateNotFound` for an unsafe template name.
/// Returns `SetupError::TemplateParse` when content is not a test config.
pub fn run_template_content(
    project_dir: &Path,
    template_name: &str,
    content: &str,
    force: bool,
) -> Result<SetupResult, SetupError> {
    validate_template_name(template_name)?;
    let config_path = config_path(project_dir)?;
    write_schema_next_to(&config_path)?;

    if config_path.exists() && !force {
        return Err(SetupError::ConfigExists(config_path));
    }

    let config: TestConfig =
        serde_json::from_str(content).map_err(|e| SetupError::TemplateParse(e.to_string()))?;

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
    let config_path = config_path(project_dir)?;
    write_schema_next_to(&config_path)?;

    if !config_path.exists() {
        return Err(SetupError::NoConfigForAddTarget);
    }

    let mut existing =
        config::load_test_config(&config_path).map_err(|e| SetupError::Config(e.to_string()))?;

    let target = wizard::run_add_target_wizard(project_dir, prompts).map_err(SetupError::Wizard)?;

    match target {
        Some(t) => {
            if existing
                .targets
                .iter()
                .any(|current| current.name == t.name)
            {
                return Err(SetupError::DuplicateTarget(t.name));
            }
            existing.targets.push(t);
            write_config(&config_path, &existing)?;
            println!("Target added to {}", config_path.display());
            Ok(SetupResult::Written)
        }
        None => Ok(SetupResult::Aborted),
    }
}

/// List available templates in `template_dir` to stdout.
///
/// # Errors
///
/// Returns `SetupError::Io` on filesystem errors.
pub fn list_templates(template_dir: &Path) -> Result<(), SetupError> {
    let templates = get_template_list(template_dir)?;
    for (name, description) in &templates {
        println!("{name}  — {description}");
    }
    Ok(())
}

/// Get sorted list of (name, description) for all templates in `template_dir`.
///
/// # Errors
///
/// Returns `SetupError::Io` on filesystem errors.
pub fn get_template_list(template_dir: &Path) -> Result<Vec<(String, String)>, SetupError> {
    if !template_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(template_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            // Shown to a person (OS text rule, issue 79).
            let name = crate::git::GitName::from_os_str(path.file_name().unwrap_or_default())
                .display()
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
    let config_path = config_path(project_dir)?;
    write_schema_next_to(&config_path)?;

    // Init is additive: any existing config, including one that currently
    // fails to parse, belongs to the project and must remain byte-identical.
    // `codeflow test`/doctor surface corruption; init never "repairs" it by
    // replacing project-owned test intent with an empty config.
    if config_path.exists() {
        return Ok(());
    }

    // Ensure parent directories exist
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = TestConfig {
        description: None,
        schema_ref: Some(SCHEMA_REF.to_string()),
        schema_version: "1.0".to_string(),
        execution: config::ExecutionConfig::default(),
        defaults: config::DefaultsConfig::default(),
        targets: Vec::new(),
    };

    write_config(&config_path, &config)?;
    Ok(())
}

fn read_template_description(path: &Path) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    template_description(&content)
}

/// Read the optional human-facing `_description` from template JSON.
/// Invalid input has no description; template application still returns its
/// more specific parse error when selected.
#[must_use]
pub fn template_description(content: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) else {
        return String::new();
    };
    parsed
        .get("_description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn config_path(project_dir: &Path) -> std::io::Result<std::path::PathBuf> {
    Ok(canonicalize_project_dir(project_dir)?.join(".codeflow/test-config.json"))
}

/// Normalise a candidate project directory to avoid writing config into a
/// nested sub-directory when an ancestor is the real project root.
///
/// **Why this exists:** a test or CLI invocation with a cwd inside the project
/// tree previously caused `write_minimal_config` to create a stale
/// `.codeflow/` hierarchy in the sub-directory. Downstream tooling then fell
/// back to that subtree as the "project root", corrupting the generic testing
/// engine's config lookup and polluting the worktree with duplicate state.
///
/// Resolution rule: if any ancestor of `project_dir` already contains an
/// unambiguous project-root marker, prefer the nearest such ancestor as the
/// effective project root. A bare `.codeflow/` or `.claude/` directory is not
/// enough: the user's home may contain both as application state. Fresh
/// projects retain the caller's `project_dir` as-is — this is the documented
/// `codeflow init` creation path.
///
/// Ancestor check is bounded by filesystem root, so the walk always terminates.
fn canonicalize_project_dir(project_dir: &Path) -> std::io::Result<std::path::PathBuf> {
    // Caller's own marker takes priority — this is a fresh or canonical root.
    if is_project_root(project_dir)? {
        return Ok(project_dir.to_path_buf());
    }

    // Walk ancestors until we find a marker or run out of parents.
    let mut cursor = project_dir;
    while let Some(parent) = cursor.parent() {
        if is_project_root(parent)? {
            return Ok(parent.to_path_buf());
        }
        cursor = parent;
    }

    // No ancestor marker — caller is a fresh project being initialised.
    Ok(project_dir.to_path_buf())
}

fn is_project_root(dir: &Path) -> std::io::Result<bool> {
    if !crate::absence::proven_absent(&dir.join(".git"))? {
        std::fs::metadata(dir.join(".git"))?;
        return Ok(true);
    }
    if crate::registry::is_initialized(dir)? {
        return Ok(true);
    }
    let is_kind = |path: &Path, directory: bool| -> std::io::Result<bool> {
        if crate::absence::proven_absent(path)? {
            return Ok(false);
        }
        let metadata = std::fs::metadata(path)?;
        Ok(if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        })
    };
    Ok(is_kind(&dir.join(".codeflow/test-config.json"), false)?
        || (is_kind(&dir.join(".claude"), true)? && is_kind(&dir.join("CLAUDE.md"), false)?))
}

fn template_path(template_dir: &Path, name: &str) -> Result<std::path::PathBuf, SetupError> {
    validate_template_name(name)?;
    Ok(template_dir.join(name))
}

fn validate_template_name(name: &str) -> Result<(), SetupError> {
    // Reject path separators and traversal sequences to prevent arbitrary file
    // reads through either the source-tree or embedded-asset adapter.
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(SetupError::TemplateNotFound(name.to_string()));
    }
    Ok(())
}

fn write_config(path: &Path, config: &TestConfig) -> Result<(), SetupError> {
    let path = guarded_generated_path(path, "test-config.json")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_schema_next_to(&path)?;
    config::write_test_config(&path, config).map_err(|e| SetupError::Config(e.to_string()))
}

fn write_schema_next_to(config_path: &Path) -> Result<(), SetupError> {
    let Some(parent) = config_path.parent() else {
        return Err(SetupError::Config(format!(
            "test config path has no parent: {}",
            config_path.display()
        )));
    };
    let schema_path = guarded_generated_path(config_path, "test-config.schema.json")?;
    std::fs::create_dir_all(parent)?;
    if matches!(
        std::fs::read_to_string(&schema_path),
        Ok(ref current) if current == TEST_CONFIG_SCHEMA
    ) {
        return Ok(());
    }
    crate::file_lock::atomic_write(&schema_path, TEST_CONFIG_SCHEMA.as_bytes())?;
    Ok(())
}

fn guarded_generated_path(
    config_path: &Path,
    file_name: &str,
) -> Result<std::path::PathBuf, SetupError> {
    let Some(codeflow_dir) = config_path.parent() else {
        return Err(SetupError::Config(format!(
            "test config path has no parent: {}",
            config_path.display()
        )));
    };
    let Some(root) = codeflow_dir.parent() else {
        return Err(SetupError::Config(format!(
            "test config path has no project root: {}",
            config_path.display()
        )));
    };
    let generated_path = config_path.with_file_name(file_name);
    let relative = generated_path.strip_prefix(root).map_err(|_| {
        SetupError::Config(format!(
            "generated test path is outside the project root: {}",
            generated_path.display()
        ))
    })?;
    crate::scaffold::state::guard_beneath_root(root, relative)
        .map_err(|error| SetupError::Config(error.to_string()))
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

    #[error(
        "Template not found. Run `codeflow test setup --list-templates` to see available templates."
    )]
    TemplateNotFound(String),

    #[error("No config exists. Run `codeflow test setup` first.")]
    NoConfigForAddTarget,

    #[error("Target already exists: {0}")]
    DuplicateTarget(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::setup::prompt::ScriptedPromptProvider;

    /// Template directory in this repo's scaffold assets, used by tests that
    /// exercise template application against the real shipped templates.
    fn assets_template_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/testing/templates")
    }

    #[test]
    fn config_path_is_correct() {
        let path = config_path(Path::new("/repo")).unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/repo/.codeflow/test-config.json")
        );
    }

    #[test]
    fn canonicalize_project_dir_uses_ancestor_with_claude_contract() {
        // Simulates the bug where a caller inside a sub-directory of the
        // project (e.g., cargo-invoked tools with cwd=`crates/codeflow-cli/`)
        // would write a stale `.codeflow/` subtree at the nested path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("crates").join("codeflow-cli");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Project instructions\n").unwrap();

        let canonical = canonicalize_project_dir(&nested).unwrap();
        assert_eq!(
            canonical,
            root.to_path_buf(),
            "nested path must resolve to the ancestor project root"
        );

        // config_path built from the nested dir must write to root/.codeflow/
        let cfg = config_path(&nested).unwrap();
        assert_eq!(
            cfg,
            root.join(".codeflow/test-config.json"),
            "config_path must rebase onto the canonical project root"
        );
    }

    #[test]
    fn canonicalize_project_dir_uses_initialized_codeflow_ancestor() {
        // Every initialized tier has a policy file. A bare `.codeflow/` is
        // user-level state and must not qualify on its own.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("crates").join("codeflow-core");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}\n").unwrap();

        let canonical = canonicalize_project_dir(&nested).unwrap();
        assert_eq!(canonical, root.to_path_buf());
    }

    #[test]
    fn canonicalize_project_dir_preserves_caller_with_claude_contract() {
        // If `project_dir` is itself the canonical root, we must NOT walk up
        // any further — otherwise setup on an existing project could rebase
        // onto a grandparent.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Project instructions\n").unwrap();

        let canonical = canonicalize_project_dir(root).unwrap();
        assert_eq!(canonical, root.to_path_buf());
    }

    #[test]
    fn canonicalize_project_dir_fresh_project_has_no_ancestor() {
        // Fresh project during `codeflow init`: no project marker anywhere.
        // The caller's directory is used as-is so init can bootstrap.
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonicalize_project_dir(dir.path()).unwrap();
        assert_eq!(canonical, dir.path().to_path_buf());
    }

    #[test]
    fn canonicalize_project_dir_ignores_user_level_state_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/config.toml"),
            "[recall]\nlimit = 20\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/settings.json"), "{}\n").unwrap();
        let project = dir.path().join("AppData/Local/Temp/project");
        std::fs::create_dir_all(&project).unwrap();

        assert_eq!(canonicalize_project_dir(&project).unwrap(), project);
    }

    #[test]
    fn run_auto_rejects_nested_project_dir_when_ancestor_has_claude() {
        // End-to-end: run_auto called with a nested path writes config at
        // the canonical root, NOT at the nested path — prevents stale
        // duplicate `.codeflow/` subtrees inside the project.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("crates").join("codeflow-cli");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "# Project instructions\n").unwrap();

        let result = run_auto(&nested).unwrap();
        // No stack markers in the fixture → zero detection, but still written.
        assert!(matches!(
            result,
            SetupResult::Written | SetupResult::WrittenNoTargets
        ));

        let nested_cfg = nested.join(".codeflow/test-config.json");
        let canonical_cfg = root.join(".codeflow/test-config.json");
        assert!(
            !nested_cfg.exists(),
            "BUG: nested config must NOT be created at {}",
            nested_cfg.display()
        );
        assert!(
            canonical_cfg.exists(),
            "config must be created at canonical root {}",
            canonical_cfg.display()
        );
    }

    #[test]
    fn template_path_is_correct() {
        let path = template_path(Path::new("/templates"), "minimal.json").unwrap();
        assert_eq!(path, std::path::PathBuf::from("/templates/minimal.json"));
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
        assert!(config_path(dir.path()).unwrap().exists());
    }

    /// Zero-detection honesty: an empty repo still gets a config written (the
    /// `init` contract), but the outcome is reported as `WrittenNoTargets` — not
    /// a plain `Written` that would imply a stack was configured.
    #[test]
    fn run_auto_empty_repo_reports_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_auto(dir.path()).unwrap();
        assert!(
            matches!(result, SetupResult::WrittenNoTargets),
            "empty repo must report zero-detection, got {result:?}"
        );
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
        assert!(config.targets.is_empty());
    }

    #[test]
    fn run_template_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        // Create a config
        write_minimal_config(dir.path()).unwrap();
        // Create template dir + file
        let tpl_dir = dir.path().join("templates");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        std::fs::write(
            tpl_dir.join("minimal.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();

        let result = run_template(dir.path(), &tpl_dir, "minimal.json", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Config already exists"), "got: {err}");
    }

    #[test]
    fn run_template_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(
            dir.path(),
            &assets_template_dir(),
            "nonexistent.json",
            false,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(
            dir.path(),
            &assets_template_dir(),
            "../../etc/passwd",
            false,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(dir.path(), &assets_template_dir(), "/etc/passwd", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Template not found"), "got: {err}");
    }

    #[test]
    fn run_template_rejects_backslash_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template(
            dir.path(),
            &assets_template_dir(),
            "..\\..\\etc\\passwd",
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn template_path_rejects_dot_dot() {
        let result = template_path(Path::new("/templates"), "../secret.json");
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
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
        assert_eq!(config.schema_version, "1.0");
        assert!(config.targets.is_empty());
    }

    /// Regression: `codeflow init` must not overwrite a project that already
    /// has a populated `test-config.json` — a misdirected invocation previously
    /// clobbered a populated config with the empty minimal template. The guard
    /// is checked by inspecting `targets.len() > 0` in the existing file.
    #[test]
    fn write_minimal_config_is_idempotent_for_populated_config() {
        use crate::testing::config::{ModeCommand, RunnerType, TargetConfig, TestConfig};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();

        let mut modes = std::collections::BTreeMap::new();
        modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "cargo test".to_string(),
            },
        );
        let populated = TestConfig {
            description: Some("pre-existing config".to_string()),
            schema_ref: None,
            schema_version: "1.0".to_string(),
            execution: config::ExecutionConfig::default(),
            defaults: config::DefaultsConfig::default(),
            targets: vec![TargetConfig {
                name: "rust-core".to_string(),
                enabled: true,
                cwd: None,
                env: std::collections::BTreeMap::new(),
                shell: crate::testing::config::CommandShell::Auto,
                requires: Vec::new(),
                outputs: Vec::new(),
                narrow: Vec::new(),
                exclusive: false,
                runner: RunnerType::Cargo,
                modes,
                report: None,
                coverage: None,
                ci_skip: None,
                ci_skip_reason: None,
                timeout_seconds: None,
                structural: None,
                tags: Vec::new(),
                test_files: Vec::new(),
            }],
        };
        let cfg_path = config_path(dir.path()).unwrap();
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        config::write_test_config(&cfg_path, &populated).unwrap();
        let before = std::fs::read(&cfg_path).unwrap();

        write_minimal_config(dir.path()).expect("idempotent on populated");

        let after = std::fs::read(&cfg_path).unwrap();
        assert_eq!(
            before, after,
            "write_minimal_config must not overwrite a populated test-config.json"
        );
    }

    #[test]
    fn write_minimal_config_preserves_malformed_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = config_path(dir.path()).unwrap();
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let malformed = b"{ populated but malformed\n";
        std::fs::write(&cfg_path, malformed).unwrap();

        write_minimal_config(dir.path()).expect("init remains idempotent");

        assert_eq!(std::fs::read(&cfg_path).unwrap(), malformed);
    }

    /// `run_auto` must refuse to clobber a populated config — same invariant
    /// as `write_minimal_config`. Returns `SetupError::ConfigExists` so the
    /// caller can surface a clear idempotent result.
    #[test]
    fn run_auto_refuses_to_overwrite_populated_config() {
        use crate::testing::config::{ModeCommand, RunnerType, TargetConfig, TestConfig};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();

        let mut modes = std::collections::BTreeMap::new();
        modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "cargo test".to_string(),
            },
        );
        let populated = TestConfig {
            description: None,
            schema_ref: None,
            schema_version: "1.0".to_string(),
            execution: config::ExecutionConfig::default(),
            defaults: config::DefaultsConfig::default(),
            targets: vec![TargetConfig {
                name: "shell-scripts".to_string(),
                enabled: true,
                cwd: None,
                env: std::collections::BTreeMap::new(),
                shell: crate::testing::config::CommandShell::Auto,
                requires: Vec::new(),
                outputs: Vec::new(),
                narrow: Vec::new(),
                exclusive: false,
                runner: RunnerType::Custom,
                modes,
                report: None,
                coverage: None,
                ci_skip: None,
                ci_skip_reason: None,
                timeout_seconds: None,
                structural: None,
                tags: Vec::new(),
                test_files: Vec::new(),
            }],
        };
        let cfg_path = config_path(dir.path()).unwrap();
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        config::write_test_config(&cfg_path, &populated).unwrap();
        let before = std::fs::read(&cfg_path).unwrap();

        let err = run_auto(dir.path()).expect_err("populated config should block run_auto");
        assert!(
            matches!(err, SetupError::ConfigExists(_)),
            "expected ConfigExists, got {err:?}"
        );

        let after = std::fs::read(&cfg_path).unwrap();
        assert_eq!(
            before, after,
            "run_auto must not mutate a populated test-config.json"
        );
    }

    #[test]
    fn run_auto_rejects_and_preserves_malformed_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = config_path(dir.path()).unwrap();
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let malformed = b"{ populated but malformed\n";
        std::fs::write(&cfg_path, malformed).unwrap();

        let error = run_auto(dir.path()).expect_err("malformed config must block auto setup");
        assert!(matches!(error, SetupError::Config(_)), "got {error:?}");
        assert_eq!(std::fs::read(&cfg_path).unwrap(), malformed);
    }

    #[test]
    fn run_auto_populates_an_existing_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_config(dir.path()).unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .unwrap();

        assert!(matches!(
            run_auto(dir.path()).unwrap(),
            SetupResult::Written
        ));
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "rust-core");
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

    /// Helper: authored-content round-trip test for a single template.
    ///
    /// Git may materialize JSON fixtures with CRLF on Windows, while the JSON
    /// writer deliberately emits LF. Normalize only that platform newline
    /// convention; every other byte remains part of the contract.
    fn assert_template_roundtrip(template_name: &str) {
        let template_path = assets_template_dir().join(template_name);

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

        let normalize_crlf = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .expect("JSON templates and writer output must remain UTF-8")
                .replace("\r\n", "\n")
        };

        assert_eq!(
            normalize_crlf(&original_bytes),
            normalize_crlf(&written_bytes),
            "authored-content round-trip failed for {template_name}.\n\
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
        let dir = tempfile::tempdir().unwrap();

        let result =
            run_template(dir.path(), &assets_template_dir(), "minimal.json", false).unwrap();
        assert!(matches!(result, SetupResult::Written));
        assert!(config_path(dir.path()).unwrap().exists());
    }

    #[test]
    fn run_template_content_uses_the_same_safe_writer() {
        let dir = tempfile::tempdir().unwrap();
        let content = std::fs::read_to_string(assets_template_dir().join("minimal.json")).unwrap();
        let result = run_template_content(dir.path(), "minimal.json", &content, false).unwrap();
        assert!(matches!(result, SetupResult::Written));
        assert!(dir
            .path()
            .join(".codeflow/test-config.schema.json")
            .exists());

        let before = std::fs::read(config_path(dir.path()).unwrap()).unwrap();
        let error = run_template_content(dir.path(), "minimal.json", &content, false)
            .expect_err("implicit replacement must be rejected");
        assert!(matches!(error, SetupError::ConfigExists(_)));
        assert_eq!(
            std::fs::read(config_path(dir.path()).unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn run_template_content_rejects_unsafe_name_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_template_content(
            dir.path(),
            "../minimal.json",
            r#"{"schema_version":"1.0","targets":[]}"#,
            false,
        );
        assert!(matches!(result, Err(SetupError::TemplateNotFound(_))));
        assert!(!config_path(dir.path()).unwrap().exists());
    }

    #[test]
    fn run_template_force_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        // Create existing config
        write_minimal_config(dir.path()).unwrap();

        let result = run_template(
            dir.path(),
            &assets_template_dir(),
            "single-target-basic.json",
            true,
        )
        .unwrap();
        assert!(matches!(result, SetupResult::Written));
        // Verify config now has a target (overwritten from minimal)
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
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
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].name, "my-target");
    }

    #[test]
    fn run_add_target_rejects_duplicate_without_mutating_config() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_config(dir.path()).unwrap();
        let first = ScriptedPromptProvider::new(vec!["api", "custom", ".", "true", "true"]);
        run_add_target(dir.path(), &first).unwrap();
        let before = std::fs::read(config_path(dir.path()).unwrap()).unwrap();

        let duplicate = ScriptedPromptProvider::new(vec!["api", "custom", ".", "true", "true"]);
        let error = run_add_target(dir.path(), &duplicate).unwrap_err();
        assert!(matches!(error, SetupError::DuplicateTarget(name) if name == "api"));
        assert_eq!(
            std::fs::read(config_path(dir.path()).unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn list_templates_with_real_templates() {
        // list_templates prints to stdout; just verify no error
        let result = list_templates(&assets_template_dir());
        assert!(result.is_ok());
    }

    #[test]
    fn get_template_list_with_real_templates() {
        let list = get_template_list(&assets_template_dir()).unwrap();
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
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
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
        let config = config::load_test_config(&config_path(dir.path()).unwrap()).unwrap();
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

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_setup_refuses_unreadable_root_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(canonicalize_project_dir(dir.path()).unwrap(), dir.path());
        std::os::unix::fs::symlink("missing", dir.path().join(".codeflow")).unwrap();
        assert!(canonicalize_project_dir(dir.path()).is_err());
    }
}
