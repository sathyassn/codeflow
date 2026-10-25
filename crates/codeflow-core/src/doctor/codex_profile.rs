//! Whether the installed Codex accepts the builder seat's launch overrides
//! (TSK-1003).
//!
//! Codex 0.163.0 and later load config twice for a TUI seat: once at startup
//! with no project layer (the user config plus the `-c` overrides, from
//! openai/codex#51872), and once for the session with the project's
//! `.codex/config.toml`. A launch that selects a profile defined only in the
//! project file stops in the first load, before the seat opens.
//!
//! The probe runs `codex features list` with the builder overrides, which
//! loads config and prints the feature table without a model, a session or
//! the sandbox. It runs once from an empty temporary folder, which stands in
//! for the startup load, and once from the project. An exit code of 0 shows
//! only that Codex accepted the command. Whether the second run loaded the
//! project config is decided from the feature table: a feature the project
//! sets to a value the run without it does not show. When no such feature
//! exists, project loading is reported unverified. No run shows the
//! sandbox boundary a seat enforces.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// The `-c` overrides a Codex builder seat launches with, in order. The
/// posture table in cf-model-orchestrator's `resources/routing/transport.md`
/// is their documented home; a test keeps the two in step. The description
/// override gives the startup load a profile named `cf-builder` that grants
/// nothing; the session load merges it into the project's table, whose
/// description is the same value.
pub const CODEX_BUILDER_OVERRIDES: [&str; 2] = [
    "permissions.cf-builder.description=\"CodeFlow builder seat\"",
    "default_permissions=\"cf-builder\"",
];

/// How long one Codex command may take before it counts as not run.
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// How one `codex features list` run with the builder overrides ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexLoad {
    /// Codex exited 0 and printed a feature table: each feature's state.
    Accepted(BTreeMap<String, bool>),
    /// Codex ran and refused the command, with what it printed.
    Rejected(String),
    /// Codex did not start, did not finish in time, or answered with
    /// output that is not a feature table, so it said nothing about the
    /// profile.
    NotRun(String),
}

/// Whether the run in the project folder loaded the project's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLayer {
    /// A feature the project sets showed the project's value only there.
    Loaded { feature: String, value: bool },
    /// The project sets a feature that the project run did not show with
    /// the project's value, so the project config was not loaded.
    NotLoaded {
        feature: String,
        shown: bool,
        configured: bool,
    },
    /// No feature the project sets differs from the run without it.
    Unverified,
}

/// What the installed Codex said about the builder overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexBuilderProbe {
    /// `codex --version`, first line, or why it gave no `codex-cli` version.
    pub version: Result<String, String>,
    /// The run without a project config.
    pub startup: CodexLoad,
    /// The run in the project folder.
    pub project: CodexLoad,
}

impl CodexBuilderProbe {
    /// Whether the project run loaded the project config, judged from the
    /// `[features]` booleans the project's `.codex/config.toml` sets. `None`
    /// unless both runs were accepted.
    #[must_use]
    pub fn project_layer(&self, configured: &BTreeMap<String, bool>) -> Option<ProjectLayer> {
        let (CodexLoad::Accepted(startup), CodexLoad::Accepted(project)) =
            (&self.startup, &self.project)
        else {
            return None;
        };
        let mut not_loaded = None;
        for (feature, &value) in configured {
            // Only a feature whose project value differs from what Codex
            // shows without the project can tell the two loads apart.
            if startup.get(feature).is_none_or(|&shown| shown == value) {
                continue;
            }
            match project.get(feature) {
                Some(&shown) if shown == value => {
                    return Some(ProjectLayer::Loaded {
                        feature: feature.clone(),
                        value,
                    });
                }
                Some(&shown) => {
                    not_loaded.get_or_insert(ProjectLayer::NotLoaded {
                        feature: feature.clone(),
                        shown,
                        configured: value,
                    });
                }
                None => {}
            }
        }
        Some(not_loaded.unwrap_or(ProjectLayer::Unverified))
    }
}

/// Run the probe with the `codex` at `codex`, from `root` (the project) and
/// from an empty temporary folder. Nothing is written to the project; the
/// temporary folder is removed afterwards.
#[must_use]
pub fn probe_codex_builder_profile(root: &Path, codex: &Path) -> CodexBuilderProbe {
    probe_with_timeout(root, codex, LOAD_TIMEOUT)
}

fn probe_with_timeout(root: &Path, codex: &Path, timeout: Duration) -> CodexBuilderProbe {
    let Some(program) = codex.to_str() else {
        let reason = "the codex path is not valid UTF-8".to_string();
        return CodexBuilderProbe {
            version: Err(reason.clone()),
            startup: CodexLoad::NotRun(reason.clone()),
            project: CodexLoad::NotRun(reason),
        };
    };
    let version = match super::run_captured(program, &["--version"], timeout, Some(""), None) {
        Ok(run) if run.code == Some(0) => run
            .stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| line.starts_with("codex-cli "))
            .map(str::to_string)
            .ok_or_else(|| "codex --version did not print a codex-cli version".to_string()),
        Ok(_) => Err("codex --version failed".to_string()),
        Err(reason) => Err(format!("codex --version did not run: {reason}")),
    };
    let startup = match tempfile::tempdir() {
        Ok(empty) => load(program, empty.path(), timeout),
        Err(error) => CodexLoad::NotRun(format!("no temporary folder: {error}")),
    };
    let project = load(program, root, timeout);
    CodexBuilderProbe {
        version,
        startup,
        project,
    }
}

/// The rows of a `codex features list` table: each line is a feature name,
/// its stage words and `true` or `false`. `None` when the output holds no
/// row or a line of another shape.
fn feature_table(stdout: &str) -> Option<BTreeMap<String, bool>> {
    let mut table = BTreeMap::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let words: Vec<&str> = line.split_ascii_whitespace().collect();
        let (Some(name), Some(state)) = (words.first(), words.last()) else {
            return None;
        };
        let value = match *state {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        if words.len() < 3
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
        {
            return None;
        }
        table.insert((*name).to_string(), value);
    }
    (!table.is_empty()).then_some(table)
}

/// One `codex features list` run with the builder overrides, in `cwd`.
fn load(program: &str, cwd: &Path, timeout: Duration) -> CodexLoad {
    let mut args = vec!["features", "list"];
    for value in CODEX_BUILDER_OVERRIDES {
        args.push("-c");
        args.push(value);
    }
    let run = match super::run_captured(program, &args, timeout, Some(""), Some(cwd)) {
        Ok(run) => run,
        Err(reason) => return CodexLoad::NotRun(reason),
    };
    if run.code == Some(0) {
        return match feature_table(&run.stdout) {
            Some(table) => CodexLoad::Accepted(table),
            None => {
                CodexLoad::NotRun("codex exited 0 but did not print a feature table".to_string())
            }
        };
    }
    // Codex prints a config error as a headline and its causes on later
    // lines, so the non-empty lines are joined and bounded.
    let text = if run.stderr.trim().is_empty() {
        &run.stdout
    } else {
        &run.stderr
    };
    let reason: String = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
        .chars()
        .take(240)
        .collect();
    if reason.is_empty() {
        return CodexLoad::Rejected(match run.code {
            Some(code) => format!("exit code {code}"),
            None => "ended by a signal".to_string(),
        });
    }
    CodexLoad::Rejected(reason)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn script(dir: &Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("codex");
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_codex_that_hangs_is_reported_not_run() {
        let bin = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let codex = script(
            bin.path(),
            "if [ \"$1\" = --version ]; then echo 'codex-cli 9.9.9'; exit 0; fi\nsleep 30\n",
        );
        let probe = probe_with_timeout(project.path(), &codex, Duration::from_millis(300));
        assert_eq!(probe.version.as_deref(), Ok("codex-cli 9.9.9"));
        for load in [&probe.startup, &probe.project] {
            assert!(
                matches!(load, CodexLoad::NotRun(reason) if reason.contains("timed out")),
                "{load:?}"
            );
        }
    }

    #[test]
    fn feature_tables_accept_only_feature_rows() {
        assert_eq!(
            feature_table("apps  stable  true\nnetwork_proxy  under development  false\nguardianv2.thread_context  stable  true\n"),
            Some(BTreeMap::from([
                ("apps".to_string(), true),
                ("guardianv2.thread_context".to_string(), true),
                ("network_proxy".to_string(), false)
            ]))
        );
        for odd in [
            "",
            "\n\n",
            "ok\n",
            "apps stable maybe\n",
            "Error: x y true\n",
        ] {
            assert_eq!(feature_table(odd), None, "{odd:?}");
        }
    }

    #[test]
    fn project_layer_is_judged_from_a_distinguishing_feature() {
        let accepted = |pairs: &[(&str, bool)]| {
            CodexLoad::Accepted(
                pairs
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), *value))
                    .collect(),
            )
        };
        let configured = BTreeMap::from([
            ("hooks".to_string(), true),
            ("network_proxy".to_string(), true),
        ]);
        let probe = |project: CodexLoad| CodexBuilderProbe {
            version: Ok("codex-cli 9.9.9".to_string()),
            startup: accepted(&[("hooks", true), ("network_proxy", false)]),
            project,
        };
        assert_eq!(
            probe(accepted(&[("hooks", true), ("network_proxy", true)])).project_layer(&configured),
            Some(ProjectLayer::Loaded {
                feature: "network_proxy".to_string(),
                value: true
            })
        );
        assert_eq!(
            probe(accepted(&[("hooks", true), ("network_proxy", false)]))
                .project_layer(&configured),
            Some(ProjectLayer::NotLoaded {
                feature: "network_proxy".to_string(),
                shown: false,
                configured: true
            })
        );
        let same = CodexBuilderProbe {
            version: Ok("codex-cli 9.9.9".to_string()),
            startup: accepted(&[("hooks", true), ("network_proxy", true)]),
            project: accepted(&[("hooks", true), ("network_proxy", true)]),
        };
        assert_eq!(
            same.project_layer(&configured),
            Some(ProjectLayer::Unverified)
        );
        assert_eq!(
            probe(CodexLoad::Rejected("no".to_string())).project_layer(&configured),
            None
        );
    }
}
