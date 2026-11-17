//! Git transport and environment checks for workspace fixtures.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Values forced by the repository's Cargo configuration, independent of
/// agent identity and CI event. A runtime manifest path does not establish
/// that Cargo loaded the configuration from its starting directory.
const CARGO_ENVIRONMENT_PROBE: [(&str, &str); 6] = [
    ("GIT_CONFIG_COUNT", "2"),
    ("GIT_CONFIG_KEY_0", "maintenance.auto"),
    ("GIT_CONFIG_VALUE_0", "false"),
    ("GIT_CONFIG_KEY_1", "gc.auto"),
    ("GIT_CONFIG_VALUE_1", "0"),
    ("GIT_CONFIG_PARAMETERS", ""),
];

/// Whether the lookup contains the values Cargo forces for fixture Git.
#[must_use]
pub fn cargo_environment_applied(lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    CARGO_ENVIRONMENT_PROBE
        .iter()
        .all(|(key, expected)| lookup(key).is_some_and(|value| value == *expected))
}

/// Refuse a fixture that depends on Cargo's forced environment when Cargo
/// was started elsewhere, before a host's Git settings or CI event affect it.
///
/// # Panics
/// Panics with the cause and the command to use when the table was skipped.
pub fn require_cargo_environment() {
    require_environment(cargo_environment_applied(|key| std::env::var_os(key)));
}

fn require_environment(applied: bool) {
    assert!(
        applied,
        ".cargo/config.toml [env] was not applied to this test process; \
         run `cargo test` from the repository root or a worktree root. \
         Starting elsewhere with --manifest-path does not load that table."
    );
}

/// A clone with only the options used by workspace fixtures.
#[must_use]
pub struct Clone {
    cwd: PathBuf,
    source: OsString,
    destination: OsString,
    options: Vec<OptionValue>,
    environment: Vec<(OsString, OsString)>,
}

enum OptionValue {
    Bare,
    Branch(String),
    Depth(u32),
    Blobless,
    NoCheckout,
}

/// Prepare a transport clone, including when `source` is a filesystem path.
/// The builder never exposes a command that callers could append options to.
pub fn clone(cwd: &Path, source: impl AsRef<OsStr>, destination: impl AsRef<OsStr>) -> Clone {
    Clone {
        cwd: cwd.to_path_buf(),
        source: source.as_ref().to_os_string(),
        destination: destination.as_ref().to_os_string(),
        options: Vec::new(),
        environment: Vec::new(),
    }
}

impl Clone {
    /// Create a bare repository.
    pub fn bare(mut self) -> Self {
        self.options.push(OptionValue::Bare);
        self
    }

    /// Select a branch.
    pub fn branch(mut self, name: &str) -> Self {
        self.options.push(OptionValue::Branch(name.to_owned()));
        self
    }

    /// Limit the history depth.
    pub fn depth(mut self, depth: u32) -> Self {
        self.options.push(OptionValue::Depth(depth));
        self
    }

    /// Omit blobs until requested.
    ///
    /// # Panics
    /// Panics for filters other than the fixture contract's `blob:none`.
    pub fn filter(mut self, filter: &str) -> Self {
        assert_eq!(filter, "blob:none", "unsupported fixture filter");
        self.options.push(OptionValue::Blobless);
        self
    }

    /// Leave the worktree unpopulated.
    pub fn no_checkout(mut self) -> Self {
        self.options.push(OptionValue::NoCheckout);
        self
    }

    /// Pass a fixture environment variable to Git.
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.environment
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// Execute the clone and return its captured output.
    ///
    /// # Panics
    /// Panics if the Git process cannot start.
    #[must_use]
    pub fn output(self) -> Output {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(self.cwd)
            .args(["clone", "--no-local", "--quiet"]);
        // A fixture's repository operands must not inherit the caller's index.
        command
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        for option in self.options {
            match option {
                OptionValue::Bare => {
                    command.arg("--bare");
                }
                OptionValue::Branch(branch) => {
                    command.arg("-b").arg(branch);
                }
                OptionValue::Depth(depth) => {
                    command.arg("--depth").arg(depth.to_string());
                }
                OptionValue::Blobless => {
                    command.arg("--filter=blob:none");
                }
                OptionValue::NoCheckout => {
                    command.arg("--no-checkout");
                }
            }
        }
        command
            .arg("--")
            .arg(self.source)
            .arg(self.destination)
            .envs(self.environment);
        command.output().expect("fixture git runs")
    }

    /// Execute the clone and return its trimmed standard output.
    ///
    /// # Panics
    /// Panics with Git's stderr if it cannot start or fails.
    // Most callers need the completed checkout and success assertion, not stdout.
    #[allow(clippy::must_use_candidate)]
    pub fn run(self) -> String {
        let output = self.output();
        assert!(
            output.status.success(),
            "fixture clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

#[cfg(test)]
mod environment_tests {
    use super::{cargo_environment_applied, require_environment, CARGO_ENVIRONMENT_PROBE};

    #[test]
    fn forced_values_establish_that_the_cargo_table_was_applied() {
        let config = include_str!("../../../.cargo/config.toml");
        for (key, value) in CARGO_ENVIRONMENT_PROBE {
            assert!(config.contains(&format!("{key} = {{ value = \"{value}\", force = true }}")));
        }
        assert!(cargo_environment_applied(|key| {
            CARGO_ENVIRONMENT_PROBE
                .iter()
                .find_map(|(name, value)| (*name == key).then(|| (*value).into()))
        }));
        for missing in CARGO_ENVIRONMENT_PROBE.map(|(key, _)| key) {
            assert!(!cargo_environment_applied(|key| {
                CARGO_ENVIRONMENT_PROBE.iter().find_map(|(name, value)| {
                    (*name == key && key != missing).then(|| (*value).into())
                })
            }));
            assert!(!cargo_environment_applied(|key| {
                CARGO_ENVIRONMENT_PROBE.iter().find_map(|(name, value)| {
                    (*name == key).then(|| {
                        if key == missing {
                            "host-value".into()
                        } else {
                            (*value).into()
                        }
                    })
                })
            }));
        }
        assert!(!cargo_environment_applied(|key| {
            (key == "CARGO_MANIFEST_DIR").then(|| "/a/manifest/path".into())
        }));
    }

    #[test]
    #[should_panic(expected = "run `cargo test` from the repository root or a worktree root")]
    fn a_missing_table_refuses_with_the_cause_and_fix() {
        require_environment(false);
    }
}
