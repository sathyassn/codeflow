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

/// What the feature tables say about the project folder's run loading the
/// project's config. Two runs that differ only by folder show at most that
/// the table is consistent with loading; they cannot name the layer that
/// supplied a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLayer {
    /// Every feature the project sets to a value the run without it does
    /// not show appears with the project's value, and at least one does.
    ConsistentWithLoading { features: Vec<(String, bool)> },
    /// Every such feature appears without the project's value: the project
    /// config was not loaded.
    NotLoaded {
        feature: String,
        shown: bool,
        configured: bool,
    },
    /// No such feature, or they disagree (some with the project's value,
    /// some without), or one is missing from a run.
    Unverified { conflicting: bool },
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
        let mut agree = Vec::new();
        let mut disagree = Vec::new();
        let mut missing = false;
        let mut contradicted = false;
        for (feature, &value) in configured {
            // Every configured feature must appear in the project run with
            // the project's value; a missing row or another value there is
            // contrary evidence whatever the run without the project showed.
            let Some(&in_project) = project.get(feature) else {
                missing = true;
                continue;
            };
            let Some(&in_startup) = startup.get(feature) else {
                missing = true;
                continue;
            };
            if in_startup != value {
                // Only these features tell the two runs apart.
                if in_project == value {
                    agree.push((feature.clone(), value));
                } else {
                    disagree.push((feature.clone(), in_project, value));
                }
            } else if in_project != value {
                contradicted = true;
            }
        }
        let missing = missing || contradicted;
        // All evidence is weighed: one agreeing feature never outvotes a
        // disagreeing or missing one.
        Some(match (agree.is_empty(), disagree.first(), missing) {
            (false, None, false) => ProjectLayer::ConsistentWithLoading { features: agree },
            (true, Some((feature, shown, configured)), false) => ProjectLayer::NotLoaded {
                feature: feature.clone(),
                shown: *shown,
                configured: *configured,
            },
            (true, None, false) => ProjectLayer::Unverified { conflicting: false },
            _ => ProjectLayer::Unverified { conflicting: true },
        })
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
    let version =
        match super::run_captured_complete(program, &["--version"], timeout, Some(""), None) {
            Ok(run) if run.code == Some(0) => cli_version(&run.stdout)
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

/// `codex-cli MAJOR.MINOR.PATCH`, with an optional `-prerelease` of ASCII
/// letters, digits, dots and hyphens, as the only output of
/// `codex --version`. `None` for anything else.
fn cli_version(stdout: &str) -> Option<String> {
    let mut lines = stdout.lines().filter(|line| !line.trim().is_empty());
    let line = lines.next()?.trim();
    if lines.next().is_some() {
        return None;
    }
    let version = line.strip_prefix("codex-cli ")?;
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    let numeric = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
    };
    // SemVer 9: identifiers are non-empty ASCII alphanumerics and hyphens,
    // and a numeric identifier has no leading zero.
    let pre_ok = pre.is_none_or(|pre| {
        pre.split('.').all(|id| {
            !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && (!id.bytes().all(|b| b.is_ascii_digit()) || numeric(id))
        })
    });
    (parts.len() == 3 && parts.iter().all(|part| numeric(part)) && pre_ok).then(|| line.to_string())
}

/// The stages `codex features list` prints between a name and its state.
const FEATURE_STAGES: [&str; 5] = [
    "stable",
    "experimental",
    "deprecated",
    "removed",
    "under development",
];

/// The rows of a `codex features list` table: each line is a feature name,
/// a known stage and `true` or `false`, and each name appears once. `None`
/// when the output holds no row, a line of another shape, an unknown stage
/// or a repeated name.
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
        let phase = words.get(1..words.len() - 1)?.join(" ");
        if !FEATURE_STAGES.contains(&phase.as_str())
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
            || table.insert((*name).to_string(), value).is_some()
        {
            return None;
        }
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
    let run = match super::run_captured_complete(program, &args, timeout, Some(""), Some(cwd)) {
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
    fn versions_must_be_codex_cli_semver() {
        for good in [
            "codex-cli 0.164.0\n",
            "codex-cli 1.2.3-alpha.1\n",
            "codex-cli 1.2.3-0\n",
            "codex-cli 1.2.3-0a1\n",
        ] {
            assert!(cli_version(good).is_some(), "{good:?}");
        }
        for bad in [
            "codex-cli banana\n",
            "codex-cli 0.164\n",
            "codex-cli 01.2.3\n",
            "codex-cli 1.2.3-\n",
            "codex-cli 1.2.3-01\n",
            "codex-cli 1.2.3-alpha.007\n",
            "codex-cli 1.2.3 extra\n",
            "codex-cli 0.164.0\ncodex-cli 0.165.0\n",
            "something 0.164.0\n",
            "",
        ] {
            assert_eq!(cli_version(bad), None, "{bad:?}");
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
            "apps banana true\n",
            "apps true\n",
            "network_proxy  stable  false\nnetwork_proxy  stable  true\n",
        ] {
            assert_eq!(feature_table(odd), None, "{odd:?}");
        }
    }

    #[test]
    fn project_layer_weighs_every_distinguishing_feature() {
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
        let judge = |startup: CodexLoad, project: CodexLoad| {
            CodexBuilderProbe {
                version: Ok("codex-cli 9.9.9".to_string()),
                startup,
                project,
            }
            .project_layer(&configured)
        };
        let off = || accepted(&[("hooks", false), ("network_proxy", false)]);
        assert_eq!(
            judge(off(), accepted(&[("hooks", true), ("network_proxy", true)])),
            Some(ProjectLayer::ConsistentWithLoading {
                features: vec![
                    ("hooks".to_string(), true),
                    ("network_proxy".to_string(), true)
                ]
            })
        );
        assert_eq!(
            judge(
                off(),
                accepted(&[("hooks", false), ("network_proxy", false)])
            ),
            Some(ProjectLayer::NotLoaded {
                feature: "hooks".to_string(),
                shown: false,
                configured: true
            })
        );
        // The review's case: one feature agrees, another contradicts.
        assert_eq!(
            judge(
                off(),
                accepted(&[("hooks", false), ("network_proxy", true)])
            ),
            Some(ProjectLayer::Unverified { conflicting: true })
        );
        // A distinguishing feature missing from the project run.
        assert_eq!(
            judge(off(), accepted(&[("network_proxy", true)])),
            Some(ProjectLayer::Unverified { conflicting: true })
        );
        // The review's cases: a feature that does not tell the runs apart
        // still contradicts when the project run lacks the project's value.
        let hooks_on = || accepted(&[("hooks", true), ("network_proxy", false)]);
        assert_eq!(
            judge(
                hooks_on(),
                accepted(&[("hooks", false), ("network_proxy", true)])
            ),
            Some(ProjectLayer::Unverified { conflicting: true })
        );
        assert_eq!(
            judge(hooks_on(), accepted(&[("network_proxy", true)])),
            Some(ProjectLayer::Unverified { conflicting: true })
        );
        assert_eq!(
            judge(
                hooks_on(),
                accepted(&[("hooks", true), ("network_proxy", true)])
            ),
            Some(ProjectLayer::ConsistentWithLoading {
                features: vec![("network_proxy".to_string(), true)]
            })
        );
        // Nothing tells the runs apart.
        assert_eq!(
            judge(
                accepted(&[("hooks", true), ("network_proxy", true)]),
                accepted(&[("hooks", true), ("network_proxy", true)])
            ),
            Some(ProjectLayer::Unverified { conflicting: false })
        );
        assert_eq!(judge(off(), CodexLoad::Rejected("no".to_string())), None);
    }
}
