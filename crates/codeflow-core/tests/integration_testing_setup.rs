//! Integration tests for `codeflow test setup` and `codeflow test doctor`.
//!
//! These tests exercise the setup module through its public API,
//! verifying end-to-end flows including template application, auto-detection,
//! and doctor validation, against the real shipped scaffold templates.

use codeflow_core::testing::config;
use codeflow_core::testing::doctor;
use codeflow_core::testing::setup;

/// Template directory in this repo's scaffold assets. Release binaries embed
/// these files for test setup; init does not scaffold the templates themselves.
fn assets_template_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/testing/templates")
}

fn config_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir.join(".codeflow").join("test-config.json")
}

fn schema_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir
        .join(".codeflow")
        .join("test-config.schema.json")
}

fn assert_shipped_schema(project_dir: &std::path::Path) {
    let config_file = config_path(project_dir);
    let config = config::load_test_config(&config_file).expect("config loads");
    let reference = config
        .schema_ref
        .as_deref()
        .expect("schema reference present");
    assert_eq!(reference, "test-config.schema.json");
    let resolved = config_file
        .parent()
        .expect("config has parent")
        .join(reference);
    assert_eq!(resolved, schema_path(project_dir));

    let installed = std::fs::read(&resolved).expect("schema installed");
    let shipped = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/testing/test-config.schema.json"),
    )
    .expect("shipped schema readable");
    assert_eq!(
        installed, shipped,
        "installed schema drifted from the asset"
    );
}

/// `setup --template example-node.json --force` followed by `doctor` on a
/// fresh tempdir -> both exit 0; resulting test-config.json byte-equivalent
/// to the template after a round-trip through the config-writer.
#[test]
fn setup_template_then_doctor_pass() {
    let dir = tempfile::tempdir().unwrap();
    let template_dir = assets_template_dir();

    // Apply template with force
    let result = setup::run_template(dir.path(), &template_dir, "example-node.json", true).unwrap();
    assert!(matches!(result, setup::SetupResult::Written));

    // Verify config exists
    let cfg = config_path(dir.path());
    assert!(cfg.exists());
    assert_shipped_schema(dir.path());

    // Run doctor — should pass (exit 0)
    let checks = doctor::run_all_checks(dir.path());
    let code = doctor::exit_code(&checks);
    assert_eq!(code, 0, "doctor failed: {checks:?}");

    // Verify byte-equivalence with the canonical form of the template
    let src = template_dir.join("example-node.json");
    let config_bytes = std::fs::read(&cfg).unwrap();

    // Load and re-write via config-writer to get canonical form of the template
    let loaded = config::load_test_config(&src).unwrap();
    let roundtrip_dir = tempfile::tempdir().unwrap();
    let roundtrip_path = roundtrip_dir.path().join("roundtrip.json");
    config::write_test_config(&roundtrip_path, &loaded).unwrap();
    let roundtrip_bytes = std::fs::read(&roundtrip_path).unwrap();

    assert_eq!(
        config_bytes, roundtrip_bytes,
        "config not byte-equivalent to template after round-trip"
    );
}

/// `setup --auto` on a tempdir containing a Cargo.toml and a package.json
/// -> produces a config with both rust-core and web targets; doctor passes.
#[test]
fn setup_auto_detects_rust_and_node() {
    let dir = tempfile::tempdir().unwrap();

    // Create stack marker files
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"my-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"my-project","devDependencies":{"vitest":"^1.0"}}"#,
    )
    .unwrap();

    // Run auto-detection
    let result = setup::run_auto(dir.path()).unwrap();
    assert!(matches!(result, setup::SetupResult::Written));

    // Load config and verify targets
    let cfg = config_path(dir.path());
    let config = config::load_test_config(&cfg).unwrap();
    assert_shipped_schema(dir.path());
    assert_eq!(
        config.targets.len(),
        2,
        "expected 2 targets, got {}",
        config.targets.len()
    );

    let names: Vec<&str> = config.targets.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"rust-core"), "missing rust-core target");
    assert!(names.contains(&"web"), "missing web target");

    // Doctor should pass
    let checks = doctor::run_all_checks(dir.path());
    let code = doctor::exit_code(&checks);
    assert_eq!(code, 0, "doctor failed: {checks:?}");
}

/// `init --non-interactive` (flags.yes=true) produces
/// `.codeflow/test-config.json` equivalent to the minimal.json template.
#[test]
fn init_non_interactive_produces_minimal_config() {
    let dir = tempfile::tempdir().unwrap();

    // Write minimal config (simulates what init --non-interactive does)
    setup::write_minimal_config(dir.path()).unwrap();

    let cfg = config_path(dir.path());
    assert!(cfg.exists(), "config not created");
    assert_shipped_schema(dir.path());

    // Load the minimal.json template for comparison
    let minimal_template = assets_template_dir().join("minimal.json");

    // Both should load to equivalent configs
    let written_config = config::load_test_config(&cfg).unwrap();
    let template_config = config::load_test_config(&minimal_template).unwrap();

    assert_eq!(
        written_config.schema_version,
        template_config.schema_version
    );
    assert_eq!(written_config.targets.len(), template_config.targets.len());
    assert!(written_config.targets.is_empty());

    // Verify the written config round-trips correctly
    let roundtrip_dir = tempfile::tempdir().unwrap();
    let roundtrip_path = roundtrip_dir.path().join("roundtrip.json");
    config::write_test_config(&roundtrip_path, &written_config).unwrap();
    let written_bytes = std::fs::read(&cfg).unwrap();
    let roundtrip_bytes = std::fs::read(&roundtrip_path).unwrap();
    assert_eq!(
        written_bytes, roundtrip_bytes,
        "minimal config not byte-stable on round-trip"
    );
}

#[test]
fn setup_repairs_a_missing_schema_without_replacing_a_populated_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = r#"{
  "$schema": "test-config.schema.json",
  "schema_version": "1.0",
  "targets": [{
    "name": "existing",
    "runner": "custom",
    "modes": {"full": {"command": "true"}}
  }]
}
"#;
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(config_path(dir.path()), config).unwrap();

    let error = setup::run_auto(dir.path()).unwrap_err();
    assert!(matches!(error, setup::SetupError::ConfigExists(_)));
    assert_eq!(
        std::fs::read_to_string(config_path(dir.path())).unwrap(),
        config,
        "populated config must remain byte-identical"
    );
    assert_shipped_schema(dir.path());

    std::fs::write(schema_path(dir.path()), "stale schema\n").unwrap();
    assert!(matches!(
        setup::run_auto(dir.path()).unwrap_err(),
        setup::SetupError::ConfigExists(_)
    ));
    assert_shipped_schema(dir.path());
}

#[cfg(unix)]
#[test]
fn setup_refuses_a_codeflow_directory_symlink() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), project.path().join(".codeflow")).unwrap();

    let error = setup::run_auto(project.path()).unwrap_err().to_string();
    assert!(error.contains("refusing to follow a symlink"), "{error}");
    assert!(!outside.path().join("test-config.json").exists());
    assert!(!outside.path().join("test-config.schema.json").exists());
}
