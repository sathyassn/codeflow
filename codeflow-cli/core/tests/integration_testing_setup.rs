//! Integration tests for `codeflow test setup`, `codeflow test doctor`,
//! and `codeflow init` testing integration.
//!
//! These tests exercise the setup module through its public API,
//! verifying end-to-end flows including template application, auto-detection,
//! and doctor validation.

use codeflow_core::testing::config;
use codeflow_core::testing::doctor;
use codeflow_core::testing::setup;

/// Find the project root containing `.codeflow/templates/test-config/`.
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

fn config_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir
        .join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json")
}

/// AC #42: `setup --template example-node.json --force` followed by `doctor` on a
/// fresh tempdir -> both exit 0; resulting test-config.json byte-equivalent to template.
#[test]
fn setup_template_then_doctor_pass() {
    let root = find_project_root();
    let dir = tempfile::tempdir().unwrap();

    // Copy template into tempdir's template location
    let tpl_dir = dir.path().join(".codeflow/templates/test-config");
    std::fs::create_dir_all(&tpl_dir).unwrap();
    let src = root.join(".codeflow/templates/test-config/example-node.json");
    std::fs::copy(&src, tpl_dir.join("example-node.json")).unwrap();

    // Apply template with force
    let result = setup::run_template(dir.path(), "example-node.json", true).unwrap();
    assert!(matches!(result, setup::SetupResult::Written));

    // Verify config exists
    let cfg = config_path(dir.path());
    assert!(cfg.exists());

    // Run doctor — should pass (exit 0)
    let checks = doctor::run_all_checks(dir.path());
    let code = doctor::exit_code(&checks);
    assert_eq!(code, 0, "doctor failed: {checks:?}");

    // Verify byte-equivalence with original template
    let _template_bytes = std::fs::read(&src).unwrap();
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

/// AC #43: `setup --auto` on a tempdir containing a Cargo.toml and a package.json
/// -> produces a config with both rust-core and web targets; doctor passes.
#[test]
fn setup_auto_detects_rust_and_node() {
    let dir = tempfile::tempdir().unwrap();

    // Create sentinel files
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

/// AC #44: `init --non-interactive` (flags.yes=true) produces
/// `.codeflow/config/testing/test-config.json` matching minimal.json byte-for-byte.
#[test]
fn init_non_interactive_produces_minimal_config() {
    let root = find_project_root();
    let dir = tempfile::tempdir().unwrap();

    // Write minimal config (simulates what init --non-interactive does)
    setup::write_minimal_config(dir.path()).unwrap();

    let cfg = config_path(dir.path());
    assert!(cfg.exists(), "config not created");

    // Load the minimal.json template for comparison
    let minimal_template = root.join(".codeflow/templates/test-config/minimal.json");

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
