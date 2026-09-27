//! Integration tests for test setup and the test-config doctor checks.
//!
//! These tests exercise the setup module through its public API,
//! verifying end-to-end flows including template application, auto-detection,
//! and doctor validation, against the real shipped scaffold templates.

use codeflow_core::testing::config;
use codeflow_core::testing::doctor;
use codeflow_core::testing::gate::{self, GateOutcome};
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

#[cfg(unix)]
fn write_executable(path: &std::path::Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, content).unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn prepend_fixture_bin(project_dir: &std::path::Path) {
    let path = config_path(project_dir);
    let mut config = config::load_test_config(&path).unwrap();
    let inherited = std::env::var("PATH").unwrap_or_default();
    let fixture_path = format!("{}:{inherited}", project_dir.join("fixture-bin").display());
    for target in &mut config.targets {
        target.env.insert("PATH".to_string(), fixture_path.clone());
    }
    config::write_test_config(&path, &config).unwrap();
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

#[cfg(unix)]
#[test]
fn shipped_custom_template_executes_both_public_command_sets() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    setup::run_template(
        dir.path(),
        &assets_template_dir(),
        "hooks-escape-hatch.json",
        false,
    )
    .unwrap();
    let scripts = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let runner = scripts.join("run-tests.sh");
    std::fs::write(
        &runner,
        "#!/bin/sh\nset -eu\ncase \"$1\" in essential|full) ;; *) exit 9 ;; esac\nprintf '%s\\n' \"$1\" >> .executed-modes\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();

    for mode in ["essential", "full"] {
        let outcome = gate::run_gate(dir.path(), mode).unwrap();
        match outcome {
            GateOutcome::Completed {
                passed, results, ..
            } => {
                assert!(passed, "{mode} failed: {results:?}");
                assert_eq!(results.len(), 1);
            }
            GateOutcome::NoTargets { reason } => panic!("{mode} did not execute: {reason}"),
        }
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".executed-modes")).unwrap(),
        "essential\nfull\n"
    );
}

#[cfg(unix)]
#[test]
fn representative_stack_templates_execute_and_produce_declared_artifacts() {
    let fixtures = [
        (
            "example-go.json",
            "go",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> .observed-command\nprintf 'mode: set\\nsrc/app.go:1.1,2.1 1 1\\n' > coverage.out\n",
            "-coverprofile=coverage.out",
        ),
        (
            "example-rust.json",
            "cargo",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> .observed-command\nmkdir -p target/llvm-cov\nprintf 'SF:src/lib.rs\\nDA:1,1\\nend_of_record\\n' > target/llvm-cov/lcov.info\n",
            "llvm-cov nextest --workspace --lcov --output-path target/llvm-cov/lcov.info",
        ),
        (
            "example-python.json",
            "pytest",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> .observed-command\nprintf '<testsuite name=\"pytest\" tests=\"1\" failures=\"0\" errors=\"0\" skipped=\"0\" time=\"0\"><testcase name=\"ok\" time=\"0\"/></testsuite>\\n' > report.xml\nprintf '<coverage><class filename=\"src/app.py\"><line number=\"1\" hits=\"1\"/></class></coverage>\\n' > coverage.xml\n",
            "--cov-report=xml --junitxml=report.xml",
        ),
        (
            "example-node.json",
            "pnpm",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> .observed-command\nmkdir -p coverage ctrf\nprintf '{\"src/app.ts\":{\"lines\":{\"total\":1,\"covered\":1}}}\\n' > coverage/coverage-summary.json\nprintf '{\"results\":{\"tool\":{\"name\":\"vitest\"},\"summary\":{\"total\":1,\"passed\":1,\"failed\":0,\"skipped\":0},\"tests\":[{\"name\":\"ok\",\"status\":\"passed\",\"duration\":0}]}}\\n' > ctrf/ctrf-report.json\n",
            "--coverage --reporter=vitest-ctrf-json-reporter",
        ),
    ];

    for (template, executable, script, expected_args) in fixtures {
        let dir = tempfile::tempdir().unwrap();
        setup::run_template(dir.path(), &assets_template_dir(), template, false).unwrap();
        let bin = dir.path().join("fixture-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let command = bin.join(executable);
        write_executable(&command, script);
        prepend_fixture_bin(dir.path());

        let outcome = gate::run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                passed,
                coverage,
                results,
            } => {
                // The Rust and Go templates also carry a lint target, which
                // calls the same fixture binary; the log is appended to.
                assert!(passed, "{template}: {results:?} {coverage:?}");
                assert_eq!(coverage.len(), 1, "{template}");
                assert_eq!(coverage[0].thresholds_failed, 0, "{template}");
                assert!(!coverage[0].data_missing, "{template}");
            }
            GateOutcome::NoTargets { reason } => panic!("{template}: {reason}"),
        }
        let observed = std::fs::read_to_string(dir.path().join(".observed-command")).unwrap();
        assert!(observed.contains(expected_args), "{template}: {observed}");
    }
}

#[test]
fn minimal_template_executes_as_an_intentional_no_target_setup() {
    let dir = tempfile::tempdir().unwrap();
    setup::run_template(dir.path(), &assets_template_dir(), "minimal.json", false).unwrap();
    let outcome = gate::run_gate(dir.path(), "full").unwrap();
    assert!(matches!(outcome, GateOutcome::NoTargets { .. }));
}

#[cfg(unix)]
#[test]
fn single_target_templates_execute_their_full_commands() {
    for (template, script, expected_args, expected_coverage) in [
        (
            "single-target-basic.json",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > .observed-command\n",
            "test -- --ci",
            false,
        ),
        (
            "single-target-with-coverage.json",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > .observed-command\nmkdir -p coverage\nprintf 'SF:src/app.ts\\nDA:1,1\\nend_of_record\\n' > coverage/lcov.info\n",
            "test -- --ci --coverage",
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup::run_template(dir.path(), &assets_template_dir(), template, false).unwrap();
        let bin = dir.path().join("fixture-bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_executable(&bin.join("npm"), script);
        prepend_fixture_bin(dir.path());

        let outcome = gate::run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                passed,
                coverage,
                results,
            } => {
                assert!(passed, "{template}: {results:?} {coverage:?}");
                assert_eq!(coverage.is_empty(), !expected_coverage, "{template}");
            }
            GateOutcome::NoTargets { reason } => panic!("{template}: {reason}"),
        }
        let observed = std::fs::read_to_string(dir.path().join(".observed-command")).unwrap();
        assert!(observed.contains(expected_args), "{template}: {observed}");
    }
}

#[cfg(unix)]
#[test]
fn monorepo_template_executes_each_target_in_its_declared_cwd() {
    let dir = tempfile::tempdir().unwrap();
    setup::run_template(
        dir.path(),
        &assets_template_dir(),
        "monorepo-multi-target.json",
        false,
    )
    .unwrap();
    for cwd in ["services/api", "apps/web", "tools"] {
        std::fs::create_dir_all(dir.path().join(cwd)).unwrap();
    }
    let bin = dir.path().join("fixture-bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_executable(
        &bin.join("go"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > .observed-command\nprintf 'mode: set\\nsrc/app.go:1.1,2.1 1 1\\n' > coverage.out\n",
    );
    write_executable(
        &bin.join("pnpm"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > .observed-command\nmkdir -p coverage\nprintf '{\"src/app.ts\":{\"lines\":{\"total\":1,\"covered\":1}}}\\n' > coverage/coverage-summary.json\n",
    );
    write_executable(
        &bin.join("pytest"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > .observed-command\nprintf '<coverage><class filename=\"src/app.py\"><line number=\"1\" hits=\"1\"/></class></coverage>\\n' > coverage.xml\n",
    );
    prepend_fixture_bin(dir.path());

    let outcome = gate::run_gate(dir.path(), "full").unwrap();
    match outcome {
        GateOutcome::Completed {
            passed,
            coverage,
            results,
        } => {
            assert!(passed, "{results:?} {coverage:?}");
            assert_eq!(results.len(), 3);
            assert_eq!(coverage.len(), 3);
            assert!(coverage.iter().all(|report| !report.data_missing));
        }
        GateOutcome::NoTargets { reason } => panic!("{reason}"),
    }
    for (cwd, expected) in [
        ("services/api", "test ./... -coverprofile=coverage.out"),
        ("apps/web", "test --run --coverage"),
        ("tools", "--cov --cov-report=xml"),
    ] {
        let observed =
            std::fs::read_to_string(dir.path().join(cwd).join(".observed-command")).unwrap();
        assert!(observed.contains(expected), "{cwd}: {observed}");
    }
}

#[test]
fn shipped_templates_match_the_public_mode_and_artifact_contract() {
    for entry in std::fs::read_dir(assets_template_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let config = config::load_test_config(&path).unwrap();
        for target in &config.targets {
            assert!(target.modes.contains_key("essential"), "{}", path.display());
            assert!(target.modes.contains_key("full"), "{}", path.display());
            assert!(target.structural.is_none(), "{}", path.display());
        }
    }

    let go = config::load_test_config(&assets_template_dir().join("example-go.json")).unwrap();
    assert!(go.targets[0].report.is_none());
    assert!(!go.targets[0].modes["full"].command.contains('|'));
    assert!(go.targets[0].modes["full"].command.contains("coverage.out"));

    let rust = config::load_test_config(&assets_template_dir().join("example-rust.json")).unwrap();
    assert!(rust.targets[0].report.is_none());
    assert!(rust.targets[0].modes["full"]
        .command
        .contains("cargo llvm-cov nextest"));
    assert!(rust.targets[0].modes["full"]
        .command
        .contains("target/llvm-cov/lcov.info"));

    let python =
        config::load_test_config(&assets_template_dir().join("example-python.json")).unwrap();
    assert_eq!(
        python.targets[0]
            .report
            .as_ref()
            .and_then(|report| report.derive_from.as_deref()),
        None
    );
}

/// The push set (TSK-132): a template's `quick` mode lives only on a
/// format/lint target, never on the test suite, so the pre-push hook stays
/// under a minute and the suite belongs to the full gate.
#[test]
fn rust_and_go_templates_push_set_is_lint_only() {
    for (file, lint, suite) in [
        ("example-rust.json", "rust-lint", "rust-core"),
        ("example-go.json", "go-lint", "go-service"),
    ] {
        let config = config::load_test_config(&assets_template_dir().join(file)).unwrap();
        let quick: Vec<&str> = config
            .targets
            .iter()
            .filter(|t| t.modes.contains_key("quick"))
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(quick, vec![lint], "{file}");
        let suite_target = config.targets.iter().find(|t| t.name == suite).unwrap();
        assert!(!suite_target.modes.contains_key("quick"), "{file}");
    }
}

#[test]
fn monorepo_template_declares_explicit_package_targets() {
    let config =
        config::load_test_config(&assets_template_dir().join("monorepo-multi-target.json"))
            .unwrap();
    let targets: Vec<_> = config
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target.cwd.as_deref()))
        .collect();
    assert_eq!(
        targets,
        vec![
            ("api-go", Some("services/api")),
            ("web-node", Some("apps/web")),
            ("tools-python", Some("tools")),
        ]
    );
}

#[test]
fn shipped_schema_describes_the_runtime_ci_and_mode_contracts() {
    let schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/base/testing/test-config.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let target = &schema["$defs"]["target"]["properties"];
    assert_eq!(target["modes"]["minProperties"], 1);
    let modes = target["modes"]["description"].as_str().unwrap();
    assert!(modes.contains("full (CLI default)"));
    assert!(modes.contains("aliases quick to essential"));
    let ci_skip = target["ci_skip"]["description"].as_str().unwrap();
    assert!(ci_skip.contains("environment has CI set"));
    assert!(!ci_skip.contains("CODEFLOW_CI"));
    assert!(!ci_skip.contains("GITHUB_ACTIONS"));
}
