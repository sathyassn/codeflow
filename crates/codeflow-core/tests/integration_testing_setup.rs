//! Integration tests for `codeflow test setup` and `codeflow test doctor`.
//!
//! These tests exercise the setup module through its public API,
//! verifying end-to-end flows including template application, auto-detection,
//! and doctor validation, against the real shipped scaffold templates.

use codeflow_core::testing::config;
use codeflow_core::testing::doctor;
use codeflow_core::testing::setup;

/// Template directory in this repo's scaffold assets
/// (`assets/base/testing/templates/`), which init ships to consumer projects.
fn assets_template_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/testing/templates")
}

fn config_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir.join(".codeflow").join("test-config.json")
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
