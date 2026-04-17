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
    if has_populated_config(&config_path) {
        return Err(SetupError::ConfigExists(config_path));
    }
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

    // Idempotence guard: never overwrite an existing populated config.
    // `codeflow init` may be re-run on a configured project; the minimal
    // scaffold must not clobber a real configuration (see INF-TSK-046-006
    // AC 5 — accidental invocation from misdirected cwd previously wiped
    // the canonical worktree config).
    if has_populated_config(&config_path) {
        return Ok(());
    }

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

/// Returns `true` if `path` exists and parses as a `TestConfig` with at least
/// one target. Used as a safety gate by `write_minimal_config` and `run_auto`
/// to prevent accidental overwrite of a real, populated configuration.
///
/// Parse or I/O failures are treated as "not populated" (fail-open): a
/// corrupted or unreadable file should be replaceable, and the callers'
/// own write step will fail with a clear error if the real issue is I/O.
fn has_populated_config(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(existing) = serde_json::from_str::<TestConfig>(&content) else {
        return false;
    };
    !existing.targets.is_empty()
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
    canonicalize_project_dir(project_dir)
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json")
}

/// Normalise a candidate project directory to avoid writing config into a
/// nested sub-directory when an ancestor is the real project root.
///
/// **Why this exists:** a test or CLI invocation with a cwd inside the project
/// tree (e.g., running `codeflow init` from `codeflow-cli/cli/`) previously
/// caused `write_minimal_config` to create a stale `.codeflow/config/testing/`
/// hierarchy in the sub-directory. Downstream tooling then fell back to that
/// subtree as the "project root", corrupting the generic testing engine's
/// config lookup and polluting the worktree with duplicate state.
///
/// Resolution rule: if any ancestor of `project_dir` already contains a
/// `.claude/` directory (the canonical codeflow project-root marker), prefer
/// the nearest such ancestor as the effective project root. Fresh projects
/// (no `.claude/` on any ancestor) retain the caller's `project_dir` as-is —
/// this is the documented `codeflow init` creation path.
///
/// Ancestor check is bounded by filesystem root, so the walk always terminates.
fn canonicalize_project_dir(project_dir: &Path) -> std::path::PathBuf {
    // Caller's own `.claude/` takes priority — this is a fresh or canonical root.
    if project_dir.join(".claude").is_dir() {
        return project_dir.to_path_buf();
    }

    // Walk ancestors until we find `.claude/` or run out of parents.
    let mut cursor = project_dir;
    while let Some(parent) = cursor.parent() {
        if parent.join(".claude").is_dir() {
            return parent.to_path_buf();
        }
        cursor = parent;
    }

    // No ancestor marker — caller is a fresh project being initialised.
    project_dir.to_path_buf()
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
    fn canonicalize_project_dir_uses_ancestor_with_claude_marker() {
        // Simulates the bug where a caller inside a sub-directory of the
        // project (e.g., cargo-invoked tools with cwd=`codeflow-cli/cli/`)
        // would write a stale `.codeflow/` subtree at the nested path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("codeflow-cli").join("cli");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        let canonical = canonicalize_project_dir(&nested);
        assert_eq!(
            canonical,
            root.to_path_buf(),
            "nested path must resolve to the ancestor project root"
        );

        // config_path built from the nested dir must write to root/.codeflow/
        let cfg = config_path(&nested);
        assert_eq!(
            cfg,
            root.join(".codeflow/config/testing/test-config.json"),
            "config_path must rebase onto the canonical project root"
        );
    }

    #[test]
    fn canonicalize_project_dir_preserves_caller_when_claude_is_on_self() {
        // If `project_dir` is itself the canonical root (has .claude/), we
        // must NOT walk up any further — otherwise `codeflow init` on an
        // existing project would incorrectly rebase onto a grandparent.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        let canonical = canonicalize_project_dir(root);
        assert_eq!(canonical, root.to_path_buf());
    }

    #[test]
    fn canonicalize_project_dir_fresh_project_has_no_ancestor() {
        // Fresh project during `codeflow init`: no `.claude/` anywhere.
        // The caller's directory is used as-is so init can bootstrap.
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonicalize_project_dir(dir.path());
        assert_eq!(canonical, dir.path().to_path_buf());
    }

    #[test]
    fn run_auto_rejects_nested_project_dir_when_ancestor_has_claude() {
        // End-to-end: run_auto called with a nested path writes config at
        // the canonical root, NOT at the nested path. Prevents the stale
        // `codeflow-cli/cli/.codeflow/` regeneration observed in INF-TSK-046-006.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("codeflow-cli").join("cli");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        let result = run_auto(&nested).unwrap();
        assert!(matches!(result, SetupResult::Written));

        let nested_cfg = nested.join(".codeflow/config/testing/test-config.json");
        let canonical_cfg = root.join(".codeflow/config/testing/test-config.json");
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

    /// Regression: `codeflow init` must not overwrite a project that already
    /// has a populated `test-config.json`. INF-TSK-046-006 AC 5 — a misdirected
    /// unit test invocation previously clobbered the canonical worktree config
    /// (7361-byte populated → 213-byte empty template). The guard is checked by
    /// inspecting `targets.len() > 0` in the existing file.
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
                runner: RunnerType::Cargo,
                modes,
                report: None,
                coverage: None,
            }],
        };
        let cfg_path = config_path(dir.path());
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

    /// `run_auto` must refuse to clobber a populated config — same invariant
    /// as `write_minimal_config`. Returns `SetupError::ConfigExists` so the
    /// caller can surface a clear error and, optionally, re-invoke with a
    /// template-style `--force` in future.
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
                runner: RunnerType::Custom,
                modes,
                report: None,
                coverage: None,
            }],
        };
        let cfg_path = config_path(dir.path());
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
