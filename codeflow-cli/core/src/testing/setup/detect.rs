//! Stack-detection heuristics for `codeflow test setup --auto`.
//!
//! Scans the repository root for sentinel files and infers test targets
//! with best-guess commands per the design doc §15.3.

use std::collections::BTreeMap;
use std::path::Path;

use crate::testing::config::{
    CoverageConfig, CoverageFormat, CoverageRule, CoverageScope, ModeCommand, PatternMapEntry,
    ReportConfig, ReportFormat, RunnerType, StructuralConfig, TargetConfig,
};

/// A detected stack with its best-guess target configuration.
#[derive(Debug, Clone)]
pub struct DetectedTarget {
    pub config: TargetConfig,
}

/// Detect stacks from sentinel files in priority order.
///
/// Order: Cargo.toml -> package.json+vitest -> package.json+jest ->
///        go.mod -> pyproject.toml+pytest
#[must_use]
pub fn detect_stacks(repo_root: &Path) -> Vec<DetectedTarget> {
    let mut targets = Vec::new();

    // 1. Cargo.toml → rust-core target
    if detect_rust(repo_root) {
        targets.push(DetectedTarget {
            config: build_rust_target(),
        });
    }

    // 2. package.json + vitest → web target with vitest
    // 3. package.json + jest → web target with jest
    if let Some(node_target) = detect_node(repo_root) {
        targets.push(DetectedTarget {
            config: node_target,
        });
    }

    // 4. go.mod → go-service target
    if detect_go(repo_root) {
        targets.push(DetectedTarget {
            config: build_go_target(),
        });
    }

    // 5. pyproject.toml or setup.py + pytest → python target
    if detect_python(repo_root) {
        targets.push(DetectedTarget {
            config: build_python_target(),
        });
    }

    targets
}

fn detect_rust(repo_root: &Path) -> bool {
    let cargo_toml = repo_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return false;
    }
    // Check if it's a workspace or a single crate
    if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
        return content.contains("[package]") || content.contains("[workspace]");
    }
    true
}

fn detect_node(repo_root: &Path) -> Option<TargetConfig> {
    let pkg_json = repo_root.join("package.json");
    if !pkg_json.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&pkg_json).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;

    let has_dep = |name: &str| -> bool {
        for section in ["devDependencies", "dependencies"] {
            if let Some(deps) = parsed.get(section).and_then(|v| v.as_object()) {
                if deps.contains_key(name) {
                    return true;
                }
            }
        }
        false
    };

    // Vitest takes priority over jest
    if has_dep("vitest") {
        return Some(build_vitest_target());
    }
    if has_dep("jest") {
        return Some(build_jest_target());
    }

    None
}

fn detect_go(repo_root: &Path) -> bool {
    repo_root.join("go.mod").exists()
}

fn detect_python(repo_root: &Path) -> bool {
    // Check pyproject.toml
    if let Ok(content) = std::fs::read_to_string(repo_root.join("pyproject.toml")) {
        if content.contains("pytest") {
            return true;
        }
    }
    // Check setup.py
    if let Ok(content) = std::fs::read_to_string(repo_root.join("setup.py")) {
        if content.contains("pytest") {
            return true;
        }
    }
    false
}

fn build_rust_target() -> TargetConfig {
    TargetConfig {
        name: "rust-core".to_string(),
        enabled: true,
        cwd: Some(".".to_string()),
        env: BTreeMap::new(),
        runner: RunnerType::Cargo,
        modes: BTreeMap::from([
            (
                "essential".to_string(),
                ModeCommand {
                    command: "cargo nextest run".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "cargo nextest run --profile full".to_string(),
                },
            ),
        ]),
        report: Some(ReportConfig {
            format: ReportFormat::Junit,
            path: "target/nextest/default/junit.xml".to_string(),
            derive_from: Some("junit".to_string()),
        }),
        coverage: Some(CoverageConfig {
            format: CoverageFormat::Lcov,
            path: "target/llvm-cov/lcov.info".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        }),
        ci_skip: None,
        ci_skip_reason: None,
        structural: None,
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

fn build_vitest_target() -> TargetConfig {
    TargetConfig {
        name: "web".to_string(),
        enabled: true,
        cwd: Some(".".to_string()),
        env: BTreeMap::new(),
        runner: RunnerType::Vitest,
        modes: BTreeMap::from([
            (
                "essential".to_string(),
                ModeCommand {
                    command: "pnpm test --run".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "pnpm test --run --coverage --reporter=vitest-ctrf-json-reporter"
                        .to_string(),
                },
            ),
        ]),
        report: Some(ReportConfig {
            format: ReportFormat::Ctrf,
            path: "ctrf/ctrf-report.json".to_string(),
            derive_from: None,
        }),
        coverage: Some(CoverageConfig {
            format: CoverageFormat::IstanbulSummary,
            path: "coverage/coverage-summary.json".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        }),
        ci_skip: None,
        ci_skip_reason: None,
        structural: Some(js_ts_structural()),
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

fn build_jest_target() -> TargetConfig {
    TargetConfig {
        name: "web".to_string(),
        enabled: true,
        cwd: Some(".".to_string()),
        env: BTreeMap::new(),
        runner: RunnerType::Jest,
        modes: BTreeMap::from([
            (
                "essential".to_string(),
                ModeCommand {
                    command: "jest --ci".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "jest --ci --coverage --reporters=jest-junit".to_string(),
                },
            ),
        ]),
        report: Some(ReportConfig {
            format: ReportFormat::Junit,
            path: "junit.xml".to_string(),
            derive_from: Some("junit".to_string()),
        }),
        coverage: Some(CoverageConfig {
            format: CoverageFormat::IstanbulSummary,
            path: "coverage/coverage-summary.json".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        }),
        ci_skip: None,
        ci_skip_reason: None,
        structural: Some(js_ts_structural()),
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

fn build_go_target() -> TargetConfig {
    TargetConfig {
        name: "go-service".to_string(),
        enabled: true,
        cwd: Some(".".to_string()),
        env: BTreeMap::new(),
        runner: RunnerType::Go,
        modes: BTreeMap::from([
            (
                "essential".to_string(),
                ModeCommand {
                    command: "go test ./...".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command:
                        "go test ./... -coverprofile=coverage.out -json | go-ctrf-json-reporter"
                            .to_string(),
                },
            ),
        ]),
        report: Some(ReportConfig {
            format: ReportFormat::Ctrf,
            path: "ctrf/ctrf-report.json".to_string(),
            derive_from: None,
        }),
        coverage: Some(CoverageConfig {
            format: CoverageFormat::GoCover,
            path: "coverage.out".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        }),
        ci_skip: None,
        ci_skip_reason: None,
        structural: Some(go_structural()),
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

fn build_python_target() -> TargetConfig {
    TargetConfig {
        name: "python".to_string(),
        enabled: true,
        cwd: Some(".".to_string()),
        env: BTreeMap::new(),
        runner: RunnerType::Pytest,
        modes: BTreeMap::from([
            (
                "essential".to_string(),
                ModeCommand {
                    command: "pytest -q".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "pytest --cov --cov-report=xml --junitxml=report.xml".to_string(),
                },
            ),
        ]),
        report: Some(ReportConfig {
            format: ReportFormat::Junit,
            path: "report.xml".to_string(),
            derive_from: Some("junit".to_string()),
        }),
        coverage: Some(CoverageConfig {
            format: CoverageFormat::Cobertura,
            path: "coverage.xml".to_string(),
            transform: None,
            rules: vec![CoverageRule {
                scope: CoverageScope::PerFile,
                include: vec![],
                exclude: vec![],
                minimum: 85,
            }],
            exceptions: vec![],
        }),
        ci_skip: None,
        ci_skip_reason: None,
        structural: Some(python_structural()),
        tags: Vec::new(),
        test_files: Vec::new(),
    }
}

// -------------------------------------------------------------------------
// Structural-check helpers per detected stack.
//
// Each helper returns a best-effort `StructuralConfig` that a majority of
// adopters can keep unchanged. Emitters can still replace the defaults
// post-setup. Rust is deliberately omitted (structural = None) because
// `cargo nextest` colocates tests under `#[cfg(test)] mod tests`, not in
// sibling test files — a 1:1 source↔test pattern map does not fit.
// -------------------------------------------------------------------------

fn js_ts_structural() -> StructuralConfig {
    StructuralConfig {
        source_glob: vec!["src/**/*.ts".to_string(), "src/**/*.tsx".to_string()],
        test_glob: vec![
            "src/**/*.test.ts".to_string(),
            "src/**/*.test.tsx".to_string(),
            "tests/**/*.test.ts".to_string(),
        ],
        pattern_map: vec![PatternMapEntry {
            source: r"^src/(.+)\.tsx?$".to_string(),
            test: "src/$1.test.ts".to_string(),
        }],
        exclusions: None,
    }
}

fn go_structural() -> StructuralConfig {
    StructuralConfig {
        source_glob: vec!["**/*.go".to_string()],
        test_glob: vec!["**/*_test.go".to_string()],
        pattern_map: vec![PatternMapEntry {
            source: r"^(.+)\.go$".to_string(),
            test: "$1_test.go".to_string(),
        }],
        exclusions: None,
    }
}

fn python_structural() -> StructuralConfig {
    // pytest convention: test files live under `tests/` as `test_<name>.py`.
    // Two ordered rules, first-match-wins:
    //   1. nested package: `src/pkg/mod.py` → `tests/pkg/test_mod.py`
    //   2. flat module:    `src/foo.py`     → `tests/test_foo.py`
    // Both templates produce paths whose filenames start with `test_`, so
    // they satisfy `test_glob`. Adopters can retune after setup.
    StructuralConfig {
        source_glob: vec!["src/**/*.py".to_string()],
        test_glob: vec!["tests/**/test_*.py".to_string()],
        pattern_map: vec![
            PatternMapEntry {
                source: r"^src/(.+)/([^/]+)\.py$".to_string(),
                test: "tests/$1/test_$2.py".to_string(),
            },
            PatternMapEntry {
                source: r"^src/([^/]+)\.py$".to_string(),
                test: "tests/test_$1.py".to_string(),
            },
        ],
        exclusions: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "rust-core");
        assert_eq!(targets[0].config.runner, RunnerType::Cargo);
    }

    #[test]
    fn detect_rust_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate-a\"]\n",
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "rust-core");
    }

    #[test]
    fn detect_node_vitest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1.0"}}"#,
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "web");
        assert_eq!(targets[0].config.runner, RunnerType::Vitest);
    }

    #[test]
    fn detect_node_jest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"jest": "^29.0"}}"#,
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "web");
        assert_eq!(targets[0].config.runner, RunnerType::Jest);
    }

    #[test]
    fn detect_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/foo\n").unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "go-service");
        assert_eq!(targets[0].config.runner, RunnerType::Go);
    }

    #[test]
    fn detect_python_pytest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\nminversion = \"6.0\"\n",
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "python");
        assert_eq!(targets[0].config.runner, RunnerType::Pytest);
    }

    #[test]
    fn detect_no_sentinels_empty() {
        let dir = tempfile::tempdir().unwrap();
        let targets = detect_stacks(dir.path());
        assert!(targets.is_empty());
    }

    #[test]
    fn detect_multiple_stacks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1.0"}}"#,
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].config.name, "rust-core");
        assert_eq!(targets[1].config.name, "web");
    }

    #[test]
    fn detect_python_setup_py() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("setup.py"),
            "setup(install_requires=['pytest'])\n",
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "python");
    }

    #[test]
    fn vitest_takes_priority_over_jest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1.0", "jest": "^29.0"}}"#,
        )
        .unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.runner, RunnerType::Vitest);
    }
}
