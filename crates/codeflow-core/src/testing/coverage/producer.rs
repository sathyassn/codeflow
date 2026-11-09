//! Coverage producers: the tool a detected stack's full command runs to write
//! the artifact its `coverage` block names (TSK-1014).
//!
//! `codeflow test setup` writes a detected target's block from this table
//! once, when it creates the config; `codeflow update` never touches the
//! test config. `codeflow doctor` reads the same table to name a producer
//! that is not installed.

use crate::testing::config::{
    CoverageConfig, CoverageFormat, ModeCommand, RunnerType, TargetConfig,
};

/// How doctor tells whether a producer is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// The test runner writes coverage itself; nothing else to install.
    BuiltIn,
    /// A command that succeeds once the tool is installed.
    Command(&'static [&'static str]),
    /// A file, relative to the target's working directory, present once the
    /// package is installed.
    File(&'static str),
}

/// One stack's coverage producer.
#[derive(Debug)]
pub struct Producer {
    /// The runner of the detected target this producer serves.
    pub runner: RunnerType,
    /// The tool's name as its users know it.
    pub tool: &'static str,
    /// The full-mode command that runs the tests under the producer.
    pub full_command: &'static str,
    /// Text that shows a full-mode command runs this producer; every entry
    /// must appear.
    pub markers: &'static [&'static str],
    /// Format of the artifact the full command writes.
    pub format: CoverageFormat,
    /// Artifact path, relative to the target's working directory. Each sits
    /// where the stack's usual ignore file already excludes it, in a folder
    /// that exists when the producer writes: cargo-llvm-cov does not create
    /// its output's folder, but cargo creates `target/` for its build while
    /// `CARGO_TARGET_DIR` is unset.
    pub path: &'static str,
    /// How doctor checks that the tool is installed.
    pub probe: Probe,
    /// The command that installs the tool.
    pub install: &'static str,
}

/// The producers detection writes, one per detected stack.
pub const PRODUCERS: &[Producer] = &[
    Producer {
        runner: RunnerType::Cargo,
        tool: "cargo-llvm-cov",
        full_command: "cargo llvm-cov --workspace --lcov --output-path target/lcov.info",
        markers: &["cargo llvm-cov"],
        format: CoverageFormat::Lcov,
        path: "target/lcov.info",
        probe: Probe::Command(&["cargo", "llvm-cov", "--version"]),
        install: "cargo install cargo-llvm-cov",
    },
    Producer {
        runner: RunnerType::Vitest,
        tool: "@vitest/coverage-v8",
        full_command: "./node_modules/.bin/vitest run --coverage.enabled --coverage.provider=v8 --coverage.reporter=lcov",
        markers: &["vitest", "--coverage"],
        format: CoverageFormat::Lcov,
        path: "coverage/lcov.info",
        probe: Probe::File("node_modules/@vitest/coverage-v8/package.json"),
        install: "npm install --save-dev @vitest/coverage-v8",
    },
    Producer {
        runner: RunnerType::Jest,
        tool: "jest",
        full_command: "./node_modules/.bin/jest --ci --coverage --coverageReporters=lcov",
        markers: &["jest", "--coverage"],
        format: CoverageFormat::Lcov,
        path: "coverage/lcov.info",
        probe: Probe::BuiltIn,
        install: "",
    },
    Producer {
        runner: RunnerType::Go,
        tool: "go test",
        full_command: "go test ./... -coverprofile=coverage.out",
        markers: &["go test", "-coverprofile"],
        format: CoverageFormat::GoCover,
        path: "coverage.out",
        probe: Probe::BuiltIn,
        install: "",
    },
    Producer {
        runner: RunnerType::Pytest,
        tool: "coverage.py",
        full_command: "coverage run -m pytest && coverage xml -o coverage.xml",
        markers: &["coverage run"],
        format: CoverageFormat::Cobertura,
        path: "coverage.xml",
        probe: Probe::Command(&["coverage", "--version"]),
        install: "python -m pip install coverage",
    },
];

/// The producer detection writes for `runner`, if any.
#[must_use]
pub fn for_runner(runner: &RunnerType) -> Option<&'static Producer> {
    PRODUCERS.iter().find(|producer| &producer.runner == runner)
}

/// The producer a full-mode command runs, if it is one in the table.
#[must_use]
pub fn in_command(command: &str) -> Option<&'static Producer> {
    PRODUCERS.iter().find(|producer| {
        producer
            .markers
            .iter()
            .all(|marker| command.contains(marker))
    })
}

/// `target` with its stack's producer: the full-mode command runs it and a
/// coverage block names the artifact it writes. The block sets no rules, so
/// it only collects until the project writes rules, in the block or in
/// `defaults.coverage`. A runner with no producer is returned unchanged.
#[must_use]
pub fn with_producer(mut target: TargetConfig) -> TargetConfig {
    if let Some(producer) = for_runner(&target.runner) {
        target.modes.insert(
            "full".to_string(),
            ModeCommand {
                command: producer.full_command.to_string(),
            },
        );
        target.coverage = Some(CoverageConfig {
            format: producer.format.clone(),
            path: producer.path.to_string(),
            transform: None,
            rules: Vec::new(),
            exceptions: Vec::new(),
        });
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_detected_stack_has_one_producer_its_own_command_names() {
        for runner in [
            RunnerType::Cargo,
            RunnerType::Vitest,
            RunnerType::Jest,
            RunnerType::Go,
            RunnerType::Pytest,
        ] {
            let producer = for_runner(&runner).expect("a detected stack has a producer");
            assert_eq!(
                in_command(producer.full_command).map(|p| &p.runner),
                Some(&runner),
                "{} names its own producer",
                producer.full_command
            );
            if producer.probe != Probe::BuiltIn {
                assert!(
                    !producer.install.is_empty(),
                    "{} has an install",
                    producer.tool
                );
            }
        }
        assert!(for_runner(&RunnerType::Custom).is_none());
    }

    #[test]
    fn a_command_without_a_producer_names_none() {
        for command in [
            "cargo test --workspace",
            "pytest",
            "./node_modules/.bin/vitest run",
        ] {
            assert!(in_command(command).is_none(), "{command}");
        }
        assert_eq!(
            in_command("cargo llvm-cov nextest --workspace").map(|p| p.tool),
            Some("cargo-llvm-cov")
        );
    }
}
