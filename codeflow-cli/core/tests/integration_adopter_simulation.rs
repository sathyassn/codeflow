//! Adopter simulation integration test — AC 35.
//!
//! Scaffolds fresh project trees in tempdirs for four stack cases
//! (Rust, Python, TypeScript+Vitest, empty), runs `setup::run_auto` on each,
//! and verifies the emitted `test-config.json`:
//!
//! * is valid under the canonical schema (load_test_config succeeds);
//! * contains the expected target(s) for that stack;
//! * includes the required top-level fields (schema_version, targets);
//! * round-trips losslessly (write → read → compare).
//!
//! Also covers the mixed-stack case (Rust + Python in one repo) which is a
//! common monorepo shape.

use std::fs;
use std::path::Path;

use codeflow_core::testing::config::{self, CoverageFormat, ReportFormat, RunnerType};
use codeflow_core::testing::setup;

fn canonical_config_path(root: &Path) -> std::path::PathBuf {
    root.join(".codeflow")
        .join("config")
        .join("testing")
        .join("test-config.json")
}

fn scaffold_rust(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"acme\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn hello() -> &'static str { \"hi\" }\n",
    )
    .unwrap();
}

fn scaffold_python(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"acme\"\nversion = \"0.1.0\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n",
    )
    .unwrap();
    fs::write(root.join("src/__init__.py"), "").unwrap();
}

fn scaffold_vitest(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "acme",
  "version": "0.1.0",
  "scripts": {"test": "vitest run"},
  "devDependencies": {"vitest": "^1.0.0"}
}"#,
    )
    .unwrap();
}

fn scaffold_empty(root: &Path) {
    // A deliberately empty project — no sentinel files.
    let _ = root;
}

#[test]
fn adopter_simulation_rust_stack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_rust(root);

    setup::run_auto(root).expect("run_auto should succeed on Rust stack");

    let cfg_path = canonical_config_path(root);
    assert!(cfg_path.exists(), "config must be written");
    let config = config::load_test_config(&cfg_path).expect("emitted config must load");
    assert_eq!(config.schema_version, "1.0");
    assert_eq!(config.targets.len(), 1);
    let target = &config.targets[0];
    assert_eq!(target.name, "rust-core");
    assert_eq!(target.runner, RunnerType::Cargo);
    assert!(target.modes.contains_key("full"));
    let report = target
        .report
        .as_ref()
        .expect("rust target must have report");
    assert_eq!(report.format, ReportFormat::Junit);
    let coverage = target
        .coverage
        .as_ref()
        .expect("rust target must have coverage");
    assert_eq!(coverage.format, CoverageFormat::Lcov);
}

#[test]
fn adopter_simulation_python_stack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_python(root);

    setup::run_auto(root).expect("run_auto should succeed on Python stack");

    let cfg_path = canonical_config_path(root);
    let config = config::load_test_config(&cfg_path).expect("emitted config must load");
    assert_eq!(config.schema_version, "1.0");
    assert_eq!(config.targets.len(), 1);
    let target = &config.targets[0];
    assert_eq!(target.name, "python");
    assert_eq!(target.runner, RunnerType::Pytest);
    let coverage = target
        .coverage
        .as_ref()
        .expect("python target must have coverage");
    assert_eq!(coverage.format, CoverageFormat::Cobertura);
}

#[test]
fn adopter_simulation_typescript_vitest_stack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_vitest(root);

    setup::run_auto(root).expect("run_auto should succeed on TypeScript+Vitest stack");

    let cfg_path = canonical_config_path(root);
    let config = config::load_test_config(&cfg_path).expect("emitted config must load");
    assert_eq!(config.targets.len(), 1);
    let target = &config.targets[0];
    assert_eq!(target.name, "web");
    assert_eq!(target.runner, RunnerType::Vitest);
}

#[test]
fn adopter_simulation_mixed_rust_and_python() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_rust(root);
    scaffold_python(root);

    setup::run_auto(root).expect("run_auto should succeed on mixed stack");

    let cfg_path = canonical_config_path(root);
    let config = config::load_test_config(&cfg_path).expect("emitted config must load");
    let names: Vec<&str> = config.targets.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"rust-core"),
        "must include rust-core: {names:?}"
    );
    assert!(names.contains(&"python"), "must include python: {names:?}");
    assert_eq!(config.targets.len(), 2);
}

#[test]
fn adopter_simulation_empty_project_produces_empty_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_empty(root);

    setup::run_auto(root).expect("run_auto should succeed even with no detectable stack");

    let cfg_path = canonical_config_path(root);
    let config = config::load_test_config(&cfg_path).expect("emitted config must load");
    assert_eq!(config.schema_version, "1.0");
    assert!(
        config.targets.is_empty(),
        "empty project must emit empty targets array"
    );
}

/// AC 27: Python/TypeScript/Go stacks auto-emit a `structural` block so
/// adopters get working source↔test mapping out of the box. Rust is
/// intentionally omitted because `cargo nextest` colocates tests inside
/// source files (`#[cfg(test)] mod tests`) rather than in sibling files.
#[test]
fn adopter_simulation_python_emits_structural_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_python(root);
    setup::run_auto(root).unwrap();

    let config = config::load_test_config(&canonical_config_path(root)).unwrap();
    let target = &config.targets[0];
    let structural = target
        .structural
        .as_ref()
        .expect("python setup must emit structural block");
    assert!(!structural.source_glob.is_empty());
    assert!(!structural.test_glob.is_empty());
    assert!(!structural.pattern_map.is_empty());
}

#[test]
fn adopter_simulation_vitest_emits_structural_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_vitest(root);
    setup::run_auto(root).unwrap();

    let config = config::load_test_config(&canonical_config_path(root)).unwrap();
    let target = &config.targets[0];
    assert!(
        target.structural.is_some(),
        "vitest setup must emit structural block"
    );
}

#[test]
fn adopter_simulation_rust_stack_does_not_emit_structural_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_rust(root);
    setup::run_auto(root).unwrap();

    let config = config::load_test_config(&canonical_config_path(root)).unwrap();
    let target = &config.targets[0];
    assert!(
        target.structural.is_none(),
        "rust-core uses colocated #[cfg(test)] modules — structural-check not applicable"
    );
}

#[test]
fn adopter_simulation_empty_project_loads_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_empty(root);
    setup::run_auto(root).unwrap();

    let cfg_path = canonical_config_path(root);
    let original = config::load_test_config(&cfg_path).unwrap();
    // Write back to a second location and ensure it loads identically.
    let dst = root.join("second-config.json");
    config::write_test_config(&dst, &original).unwrap();
    let reloaded = config::load_test_config(&dst).unwrap();
    assert_eq!(reloaded.schema_version, original.schema_version);
    assert_eq!(reloaded.targets.len(), original.targets.len());
}

#[test]
fn adopter_simulation_run_auto_refuses_to_overwrite_populated_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_rust(root);

    // First run — succeeds.
    setup::run_auto(root).unwrap();
    let cfg_path = canonical_config_path(root);
    let before = fs::read(&cfg_path).unwrap();

    // Second run — must refuse, leaving the existing config intact.
    let err = setup::run_auto(root).expect_err("second run_auto should refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("ConfigExists") || msg.contains("exist"),
        "expected ConfigExists-style error, got: {msg}"
    );
    let after = fs::read(&cfg_path).unwrap();
    assert_eq!(before, after, "populated config must not be overwritten");
}

/// Equivalent to `codeflow test --mode full` on an empty-targets config:
/// the CLI prints "No test targets configured" and exits 0. Here we verify
/// the library-level signal (targets array is empty) that drives that
/// rendering.
#[test]
fn adopter_simulation_empty_targets_signal_no_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffold_empty(root);
    setup::run_auto(root).unwrap();

    let config = config::load_test_config(&canonical_config_path(root)).unwrap();
    let active: Vec<_> = config.targets.iter().filter(|t| t.enabled).collect();
    assert!(
        active.is_empty(),
        "empty adopter project must yield zero active targets so --mode full emits the friendly no-op message"
    );
}
