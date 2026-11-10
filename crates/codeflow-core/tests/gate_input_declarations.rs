//! This repository's gate targets declare the inputs they read (TSK-353).
//!
//! The path-to-target matrix below runs representative paths through
//! `declared_selection` with the real `.codeflow/test-config.json` and states
//! the targets each selects, with their prerequisites. No run applies this
//! selection yet: every run executes every eligible target, because no local
//! run record earns selection credit (ADR-0089). The matrix proves what the
//! declarations would select once credit returns, and the `guards` rules
//! that `codeflow test guards` prints for the build loop.
//!
//! `contract-scans` discovers this suite, so it runs in the quick gate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use codeflow_core::testing::config::{load_test_config, TestConfig};
use codeflow_core::testing::declared::{declared_selection, guards, Change};
use codeflow_core::testing::gate::inputs_match;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn config() -> TestConfig {
    load_test_config(&repo_root().join(".codeflow/test-config.json")).expect("config loads")
}

fn changed(path: &str) -> Change {
    Change::Changed(path.into())
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

/// Targets that run whatever a change touches: no declared inputs, or a
/// class other than `content`.
const QUICK_ALWAYS: &[&str] = &["contract-scans", "gate-parity", "herdr-delivery"];
const ESSENTIAL_ALWAYS: &[&str] = &[
    "docs-portal",
    "docs-portal-deps",
    "docs-portal-windows",
    "docs-validation",
    "gate-parity",
    "herdr-delivery",
    "model-eval-kit",
    "present-web-build",
    "release-automation",
    "rust-workspace",
];

/// One matrix row: a change, and per mode the targets its declarations
/// select and the prerequisites those need. `None` means every eligible
/// target.
struct Row {
    case: &'static str,
    changes: Vec<Change>,
    quick: Option<(&'static [&'static str], &'static [&'static str])>,
    essential: Option<(&'static [&'static str], &'static [&'static str])>,
}

fn with(always: &[&str], extra: &[&str]) -> BTreeSet<String> {
    names(always).union(&names(extra)).cloned().collect()
}

fn eligible(config: &TestConfig, mode: &str) -> BTreeSet<String> {
    config
        .targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key(mode))
        .map(|t| t.name.clone())
        .collect()
}

#[allow(clippy::too_many_lines)] // one table: a row per AC-1 path class
fn matrix() -> Vec<Row> {
    let records: (&[&str], &[&str]) = (&["codeflow-bin"], &["rust-build"]);
    vec![
        Row {
            case: "task record",
            changes: vec![changed("project-management/tasks/TSK-353.md")],
            quick: Some((&[], &[])),
            essential: Some(records),
        },
        Row {
            case: "epic record",
            changes: vec![changed("project-management/epics/EPC-048.md")],
            quick: Some((&[], &[])),
            essential: Some(records),
        },
        Row {
            case: "docs page",
            changes: vec![changed("docs/cli.md")],
            quick: Some((&[], &[])),
            essential: Some(records),
        },
        Row {
            case: "managed docs page",
            changes: vec![changed("docs/capabilities.md")],
            quick: Some((&["repository-update-noop"], &[])),
            essential: Some(records),
        },
        Row {
            case: "changelog fragment",
            changes: vec![changed("changelog.d/TSK-1281.md")],
            quick: Some((&[], &[])),
            essential: Some((&[], &["codeflow-bin", "rust-build"])),
        },
        Row {
            case: "managed instruction at the root",
            changes: vec![changed("AGENTS.md")],
            quick: Some((&["repository-update-noop"], &[])),
            essential: Some(records),
        },
        Row {
            case: "managed instruction under assets",
            changes: vec![changed("assets/base/agents/skills/cf-develop/SKILL.md")],
            quick: None,
            essential: None,
        },
        Row {
            case: "hook source",
            changes: vec![changed("crates/codeflow-core/src/hooks/git_guard.rs")],
            quick: None,
            essential: None,
        },
        Row {
            case: "guard security table",
            changes: vec![changed("crates/codeflow-core/src/security/actions.json")],
            quick: None,
            essential: None,
        },
        Row {
            case: "policy",
            changes: vec![changed(".codeflow/policy.json")],
            quick: None,
            essential: None,
        },
        Row {
            case: "core Rust source",
            changes: vec![changed("crates/codeflow-core/src/lib.rs")],
            quick: None,
            essential: None,
        },
        Row {
            case: "CLI Rust source",
            changes: vec![changed("crates/codeflow-cli/src/main.rs")],
            quick: None,
            essential: None,
        },
        Row {
            case: "present Rust source",
            changes: vec![changed("crates/codeflow-present/src/lib.rs")],
            quick: None,
            essential: None,
        },
        Row {
            case: "workflow file",
            changes: vec![changed(".github/workflows/codeflow-ci.yml")],
            quick: None,
            essential: None,
        },
        Row {
            case: "skill trigger eval",
            changes: vec![changed("evals/skill-triggers/triggers.json")],
            quick: Some((&["skill-triggers"], &[])),
            essential: Some((&["skill-triggers"], &["codeflow-bin", "rust-build"])),
        },
        Row {
            case: "deleted path",
            changes: vec![Change::Deleted(
                "project-management/tasks/TSK-353.md".into(),
            )],
            quick: None,
            essential: None,
        },
        Row {
            case: "renamed path",
            changes: vec![Change::Renamed {
                from: "docs/cli.md".into(),
                to: "docs/command-line.md".into(),
            }],
            quick: None,
            essential: None,
        },
        Row {
            case: "unknown top-level path",
            changes: vec![changed("notes.txt")],
            quick: None,
            essential: None,
        },
    ]
}

#[test]
fn the_path_to_target_matrix_states_each_selection_and_its_prerequisites() {
    let config = config();
    for row in matrix() {
        for (mode, expected, always) in [
            ("quick", row.quick, QUICK_ALWAYS),
            ("essential", row.essential, ESSENTIAL_ALWAYS),
        ] {
            let selection = declared_selection(&config, mode, &row.changes);
            let got: BTreeSet<String> = selection.selected.keys().cloned().collect();
            match expected {
                None => {
                    assert!(
                        selection.every_target.is_some(),
                        "{}: {mode}: {selection:?}",
                        row.case
                    );
                    assert_eq!(got, eligible(&config, mode), "{}: {mode}", row.case);
                }
                Some((selected, prerequisites)) => {
                    assert_eq!(selection.every_target, None, "{}: {mode}", row.case);
                    assert_eq!(got, with(always, selected), "{}: {mode}", row.case);
                    assert_eq!(
                        selection.prerequisites,
                        names(prerequisites),
                        "{}: {mode}",
                        row.case
                    );
                }
            }
        }
    }
}

/// AC-3: a records-only change leaves out the Rust checks that do not read
/// records; the workspace suite stays, because its tests read every tracked
/// Markdown file and path; the provenance binary stays as the producer
/// `docs-validation` needs, with its prerequisite.
#[test]
fn records_and_docs_keep_their_readers_and_drop_the_rust_lints() {
    let config = config();
    for path in ["project-management/tasks/TSK-353.md", "docs/cli.md"] {
        let selection = declared_selection(&config, "essential", &[changed(path)]);
        for kept in [
            "docs-validation",
            "docs-portal",
            "rust-workspace",
            "codeflow-bin",
        ] {
            assert!(selection.selected.contains_key(kept), "{path}: {kept}");
        }
        assert!(selection.prerequisites.contains("rust-build"), "{path}");
        for dropped in ["rust-clippy", "rustdoc", "rust-format"] {
            assert!(!selection.closure().contains(dropped), "{path}: {dropped}");
        }
        let full = declared_selection(&config, "full", &[changed(path)]);
        assert!(
            full.selected.contains_key("docs-portal"),
            "{path}: full docs-portal"
        );
        let quick = declared_selection(&config, "quick", &[changed(path)]);
        assert!(
            !quick.closure().contains("rust-clippy"),
            "{path}: quick clippy"
        );
    }
}

/// Every declared pattern names a tracked file: the evidence reader refuses
/// a pattern with no match, and a stale pattern would hide a missing input.
#[test]
fn every_declared_input_pattern_matches_a_tracked_file() {
    let root = repo_root();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git lists tracked files");
    assert!(output.status.success());
    let tracked: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    let config = config();
    let mut stale = Vec::new();
    for target in &config.targets {
        for pattern in &target.narrow {
            if !tracked
                .iter()
                .any(|path| inputs_match(std::slice::from_ref(pattern), path))
            {
                stale.push(format!("{}: {pattern}", target.name));
            }
        }
    }
    for rule in &config.guards {
        for pattern in &rule.paths {
            if !tracked
                .iter()
                .any(|path| inputs_match(std::slice::from_ref(pattern), path))
            {
                stale.push(format!("guards: {pattern}"));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "patterns with no tracked match: {stale:?}"
    );
}

/// Every guarding command names a test binary or script that exists.
#[test]
fn every_guarding_test_names_a_real_binary_or_script() {
    let root = repo_root();
    let mut missing = Vec::new();
    for rule in &config().guards {
        for test in &rule.tests {
            let words: Vec<&str> = test.split_whitespace().collect();
            if let [.., "-p", krate, "--test", name] = words.as_slice() {
                let tests = root.join("crates").join(krate).join("tests");
                if !tests.join(format!("{name}.rs")).is_file()
                    && !tests.join(name).join("main.rs").is_file()
                {
                    missing.push(test.clone());
                }
            } else if let ["python3", "-B", script] = words.as_slice() {
                if !root.join(script).is_file() {
                    missing.push(test.clone());
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "guarding tests that do not exist: {missing:?}"
    );
}

/// AC-9: PR 525's range changed managed skill assets and a record linking
/// into `docs/verification/`; quick ran neither `init_e2e` nor
/// `release_contract`, and hosted CI failed both. The guards name them.
#[test]
fn pr_525s_range_names_init_e2e_and_release_contract() {
    let changes = [
        changed("assets/base/agents/skills/cf-present/resources/tokens.md"),
        changed(".agents/skills/cf-present/resources/tokens.md"),
        changed(".codeflow/.baseline/.claude/skills/cf-present/resources/tokens.md"),
        changed("project-management/tasks/TSK-1162.md"),
        changed("docs/verification/evidence/tsk-1162/ink-swatches.html"),
        changed("crates/codeflow-present/web/src/theme.ts"),
    ];
    let report = guards(&config(), &changes);
    for test in [
        "cargo test -p codeflow-cli --test init_e2e",
        "cargo test -p codeflow-cli --test release_contract",
    ] {
        assert!(report.tests.iter().any(|t| t == test), "{test}: {report:?}");
    }
    assert!(report.guarded["project-management/tasks/TSK-1162.md"]
        .iter()
        .any(|t| t.ends_with("--test release_contract")));
    assert!(
        report.guarded["assets/base/agents/skills/cf-present/resources/tokens.md"]
            .iter()
            .any(|t| t.ends_with("--test init_e2e"))
    );
}

/// A docs page names the tests that read docs through the portal config,
/// with no `docs/` literal a source sweep would find.
#[test]
fn a_docs_page_names_the_portal_config_readers() {
    let report = guards(
        &config(),
        &[
            changed("docs/cli.md"),
            changed("docs-portal/portal.config.json"),
        ],
    );
    assert!(report.guarded["docs/cli.md"]
        .iter()
        .any(|t| t.ends_with("--test guide_altitude_budget")));
    let portal = &report.guarded["docs-portal/portal.config.json"];
    for test in ["guide_altitude_budget", "manifest_consistency"] {
        assert!(
            portal.iter().any(|t| t.ends_with(&format!("--test {test}"))),
            "{test}"
        );
    }
}

/// AC-10: a mapped path names its tests; an unmapped one is reported.
#[test]
fn an_unmapped_path_is_reported_as_unguarded() {
    let report = guards(
        &config(),
        &[changed("LICENSE-MIT"), changed(".codeflow/policy.json")],
    );
    assert_eq!(report.unguarded, ["LICENSE-MIT"]);
    assert!(report.guarded[".codeflow/policy.json"]
        .iter()
        .any(|t| t.ends_with("--test hooks_cli")));
}
