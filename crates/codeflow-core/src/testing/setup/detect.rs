//! Stack-detection heuristics for the setup wizard.
//!
//! Scans the repository root for sentinel files and infers test targets
//! with best-guess commands per the design doc §15.3.

use std::collections::BTreeMap;
use std::path::Path;

use crate::testing::config::{ModeCommand, RunnerType, TargetConfig};

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
                    command: "cargo test --workspace --quiet".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "cargo test --workspace".to_string(),
                },
            ),
        ]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        timeout_seconds: None,
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
                    command: "./node_modules/.bin/vitest run".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "./node_modules/.bin/vitest run".to_string(),
                },
            ),
        ]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        timeout_seconds: None,
        structural: None,
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
                    command: "./node_modules/.bin/jest --ci".to_string(),
                },
            ),
            (
                "full".to_string(),
                ModeCommand {
                    command: "./node_modules/.bin/jest --ci".to_string(),
                },
            ),
        ]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        timeout_seconds: None,
        structural: None,
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
                    command: "go test ./...".to_string(),
                },
            ),
        ]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        timeout_seconds: None,
        structural: None,
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
                    command: "pytest".to_string(),
                },
            ),
        ]),
        report: None,
        coverage: None,
        ci_skip: None,
        ci_skip_reason: None,
        timeout_seconds: None,
        structural: None,
        tags: Vec::new(),
        test_files: Vec::new(),
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
        assert_eq!(
            targets[0].config.modes["essential"].command,
            "cargo test --workspace --quiet"
        );
        assert_eq!(
            targets[0].config.modes["full"].command,
            "cargo test --workspace"
        );
        assert!(targets[0].config.report.is_none());
        assert!(targets[0].config.coverage.is_none());
        assert!(targets[0].config.structural.is_none());
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
        assert_eq!(
            targets[0].config.modes["full"].command,
            "./node_modules/.bin/vitest run"
        );
        assert!(targets[0].config.report.is_none());
        assert!(targets[0].config.coverage.is_none());
        assert!(targets[0].config.structural.is_none());
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
        assert_eq!(
            targets[0].config.modes["full"].command,
            "./node_modules/.bin/jest --ci"
        );
        assert!(targets[0].config.report.is_none());
        assert!(targets[0].config.coverage.is_none());
        assert!(targets[0].config.structural.is_none());
    }

    #[test]
    fn detect_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/foo\n").unwrap();
        let targets = detect_stacks(dir.path());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].config.name, "go-service");
        assert_eq!(targets[0].config.runner, RunnerType::Go);
        assert_eq!(targets[0].config.modes["full"].command, "go test ./...");
        assert!(targets[0].config.report.is_none());
        assert!(targets[0].config.coverage.is_none());
        assert!(targets[0].config.structural.is_none());
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
        assert_eq!(targets[0].config.modes["full"].command, "pytest");
        assert!(targets[0].config.report.is_none());
        assert!(targets[0].config.coverage.is_none());
        assert!(targets[0].config.structural.is_none());
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
    fn detection_is_root_only_and_does_not_guess_monorepo_packages() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("apps/web");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("package.json"),
            r#"{"devDependencies":{"vitest":"^1"}}"#,
        )
        .unwrap();

        assert!(detect_stacks(dir.path()).is_empty());
        assert_eq!(detect_stacks(&nested).len(), 1);
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
