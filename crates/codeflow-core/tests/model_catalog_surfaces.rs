//! Schema-5 diagnostics and derived scans over fictional fixture trees.
use std::path::Path;

use codeflow_core::doctor::{self, Options, Status};
use codeflow_core::model_catalog::{scan, Catalog};
use serde_json::json;

#[path = "support/catalog_fixture.rs"]
mod fixture_data;

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";

fn write(root: &Path, path: &str, value: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value).unwrap();
}

fn opts(root: &Path) -> Options {
    Options {
        project_dir: root.to_string_lossy().into_owned(),
        codeflow_home: Some(root.join("personal")),
        qualification_dir: Some(root.join("personal/qualified-bindings")),
        look_path: Some(|_| panic!("schema 5 diagnostics must not probe")),
        exec_command: Some(|_, _| panic!("schema 5 diagnostics must not launch")),
        ..Options::default()
    }
}

#[test]
fn doctor_catalog_rows_and_advisory_warnings_without_launching() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = fixture_data::fixture();
    let mut next = catalog["lines"][0]["versions"][0].clone();
    next["id"] = json!("orchid-next");
    next["pinned_id"] = json!("orchid-next-pin");
    next["alias"] = json!("orchid-next-alias");
    next["selectors"]["claude-code"] = json!("orchid-next-pin");
    next["designations"] = json!([]);
    catalog["lines"][0]["versions"]
        .as_array_mut()
        .unwrap()
        .push(next);
    write(dir.path(), CATALOG, &catalog.to_string());
    write(
        dir.path(),
        "personal/model-canary.json",
        r#"{"schema_version":1,"observed_ids":{"orchid-one-pin":"drifted-id"}}"#,
    );
    let result = doctor::run_check("model-bindings", &opts(dir.path())).unwrap();
    assert!(result.status.is_warn(), "{:?}", result.status);
    for expected in [
        "illustrative: context-free, not a task's resolution",
        "newer version not yet designated or adopted",
        "designated version with no full-suite record",
        "canary identity drift: orchid-one-pin observed drifted-id",
        "doctor did not launch a model",
    ] {
        assert!(
            result.message.contains(expected),
            "missing {expected}: {}",
            result.message
        );
    }
    for host in ["claude-code", "codex-cli", "codex-app", "grok-cli"] {
        for duty in catalog["duties"].as_object().unwrap().keys() {
            assert_eq!(
                result
                    .message
                    .lines()
                    .filter(|line| line.starts_with(&format!("{host} {duty}:")))
                    .count(),
                1
            );
        }
    }
    // Schema 5 has no author facts here; a review gap is illustrative, not a
    // configuration error. Warnings remain non-failing.
    assert!(result.message.contains("claude-code unit-review: open"));
}

#[test]
fn doctor_catalog_overlay_and_project_violations_fail_closed() {
    let cases = [
        (CATALOG, r#"{"schema_version":5}"#),
        (
            "personal/model-catalog.local.json",
            r#"{"schema_version":1,"additions":[],"exclusions":["unknown"]}"#,
        ),
        (
            "personal/model-catalog.local.json",
            r#"{"schema_version":1,"additions":[],"exclusions":[],"duties":{}}"#,
        ),
        (
            ".codeflow/model-selection.json",
            r#"{"schema_version":2,"bindings":[]}"#,
        ),
        (
            ".codeflow/model-selection.json",
            r#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"missing"}]}"#,
        ),
        (
            "personal/model-canary.json",
            r#"{"schema_version":1,"observed_ids":{"a":""}}"#,
        ),
    ];
    for (path, content) in cases {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), CATALOG, &fixture_data::fixture().to_string());
        write(dir.path(), path, content);
        let result = doctor::run_check("model-bindings", &opts(dir.path())).unwrap();
        assert_eq!(result.status, Status::Fail, "{path}: {}", result.message);
    }
}

fn scan_catalog() -> Catalog {
    let mut catalog = fixture_data::fixture();
    let mut retired = catalog["lines"][3]["versions"][0].clone();
    retired["id"] = json!("quartz-old");
    retired["alias"] = json!("quartz-old-alias");
    retired["pinned_id"] = json!("quartz-old-pin");
    retired["selectors"] = json!({"codex-cli":"quartz-old-selector"});
    retired["lifecycle"] = json!("retired");
    catalog["lines"][3]["versions"]
        .as_array_mut()
        .unwrap()
        .push(retired);
    Catalog::parse(catalog.to_string().as_bytes()).unwrap()
}

#[test]
fn catalog_scan_catches_all_instruction_surfaces_and_exact_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = scan_catalog();
    for path in [
        ".agents/skills/example/SKILL.md",
        ".claude/skills/example/resources/guide.md",
        ".agents/skills/example/references/guide.md",
        "assets/base/AGENTS.md.tmpl",
        "assets/base/AGENTS.minimal.md.tmpl",
        "assets/base/CLAUDE.md.tmpl",
        "assets/base/agents/cf-reviewer.md",
        ".claude/workflows/example.js",
    ] {
        write(
            dir.path(),
            path,
            "Use `orchid-one-alias`, then quartz-old-selector.\n",
        );
    }
    let findings = scan::instruction_selectors(dir.path(), &catalog).unwrap();
    assert_eq!(findings.len(), 16);
    assert!(findings.iter().all(|f| f.line == 1));
    let clean = tempfile::tempdir().unwrap();
    write(clean.path(), ".agents/skills/example/SKILL.md", "Orchid-one-alias quartz-old-selector-extra pre-orchid-one-alias orchid-one-alias_suffix orchid-one-alias.2\n");
    assert!(scan::instruction_selectors(clean.path(), &catalog)
        .unwrap()
        .is_empty());
    write(
        clean.path(),
        ".agents/skills/example/SKILL.md",
        "Use orchid-one-alias.",
    );
    assert_eq!(
        scan::instruction_selectors(clean.path(), &catalog)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn catalog_scans_allow_exact_catalog_mirrors_and_evaluation_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = scan_catalog();
    for prefix in [
        "assets/base/agents",
        ".agents",
        ".claude",
        ".codeflow/.baseline/.agents",
        ".codeflow/.baseline/.claude",
    ] {
        for suffix in [
            "skills/cf-model-orchestrator/resources/current-ensemble.json",
            "skills/cf-evaluate-model/resources/fixtures.json",
            "skills/cf-evaluate-model/resources/fixtures/case.md",
        ] {
            write(
                dir.path(),
                &format!("{prefix}/{suffix}"),
                "orchid-one-alias quartz-old-selector",
            );
        }
    }
    assert!(scan::instruction_selectors(dir.path(), &catalog)
        .unwrap()
        .is_empty());
    assert!(scan::retired_selectors(dir.path(), &catalog)
        .unwrap()
        .is_empty());
    write(
        dir.path(),
        ".agents/skills/unrelated/resources/current-ensemble.json",
        "quartz-old-selector",
    );
    assert_eq!(
        scan::instruction_selectors(dir.path(), &catalog)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        scan::retired_selectors(dir.path(), &catalog).unwrap().len(),
        1
    );
}

#[test]
fn retired_scan_rejects_outside_history_and_does_not_hide_instruction_pins() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = scan_catalog();
    for path in [
        "docs/decisions/ADR-900.md",
        "docs/verification/fixture.md",
        "project-management/tasks/TSK-900.md",
        "CHANGELOG.md",
        "docs/plan/history.md",
    ] {
        write(dir.path(), path, "quartz-old-selector");
    }
    assert!(scan::retired_selectors(dir.path(), &catalog)
        .unwrap()
        .is_empty());
    write(
        dir.path(),
        "docs/guide.md",
        "a retired quartz-old-selector\nquartz-old-pin\n",
    );
    let findings = scan::retired_selectors(dir.path(), &catalog).unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[1].line, 2);
    write(dir.path(), "docs/decisions/SKILL.md", "orchid-one-pin");
    assert_eq!(
        scan::instruction_selectors(dir.path(), &catalog)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn catalog_instruction_scan_leaves_nonrouting_preservation_tests_alone() {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "crates/core/src/standards.rs",
        "crates/cli/src/cmd/git_hook.rs",
        "crates/cli/src/cmd/ci.rs",
        "crates/cli/tests/settings.rs",
    ] {
        write(dir.path(), path, "fictional preservation orchid-one-alias");
    }
    assert!(scan::instruction_selectors(dir.path(), &scan_catalog())
        .unwrap()
        .is_empty());
}

#[test]
fn instruction_scan_limits_workflows_to_managed_agent_examples() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = scan_catalog();
    for path in [
        ".github/workflows/canary.yml",
        "docs/workflows/example.md",
        "other/.claude/workflows/example.js",
        "assets/base/claude/workflows-extra/example.js",
    ] {
        write(dir.path(), path, "orchid-one-pin quartz-old-selector");
    }
    assert!(scan::instruction_selectors(dir.path(), &catalog)
        .unwrap()
        .is_empty());
    // The retirement rule is independent: CI remains subject to that scan.
    assert_eq!(
        scan::retired_selectors(dir.path(), &catalog).unwrap().len(),
        4
    );
    let paths = [
        ".claude/workflows/example.js",
        "assets/base/claude/workflows/example.js",
        ".codeflow/.baseline/.claude/workflows/example.js",
    ];
    for path in paths {
        write(dir.path(), path, "orchid-one-pin");
    }
    let findings = scan::instruction_selectors(dir.path(), &catalog).unwrap();
    assert_eq!(findings.len(), paths.len());
    assert!(findings
        .iter()
        .all(|finding| paths.iter().any(|path| finding.path == Path::new(path))));
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn real_tree_scans_find_no_catalog_selector_or_stray_retired_selector() {
    let root = repo_root();
    let catalog = codeflow_core::model_catalog::load_catalog(&root).unwrap();
    let instructions = scan::instruction_selectors(&root, &catalog).unwrap();
    assert!(instructions.is_empty(), "{instructions:#?}");
    let retired = scan::retired_selectors(&root, &catalog).unwrap();
    assert!(retired.is_empty(), "{retired:#?}");
}

#[test]
fn real_catalog_scans_fail_on_a_skill_selector_and_a_stray_retired_selector() {
    let catalog = codeflow_core::model_catalog::load_catalog(&repo_root()).unwrap();
    let versions = || catalog.lines.iter().flat_map(|line| &line.versions);
    let active = versions()
        .find(|v| v.lifecycle != codeflow_core::model_catalog::Lifecycle::Retired)
        .unwrap();
    let retired = versions()
        .find(|v| v.lifecycle == codeflow_core::model_catalog::Lifecycle::Retired)
        .unwrap();
    let selector = active.selectors.values().next().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".agents/skills/example/SKILL.md",
        &format!("Launch `{selector}`.\n"),
    );
    let found = scan::instruction_selectors(dir.path(), &catalog).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(&found[0].token, selector);
    // Allowed homes: the catalog mirrors and the history paths.
    write(dir.path(), CATALOG, &retired.pinned_id);
    write(dir.path(), "docs/decisions/ADR-900.md", &retired.pinned_id);
    assert!(scan::retired_selectors(dir.path(), &catalog)
        .unwrap()
        .is_empty());
    write(
        dir.path(),
        "docs/guide.md",
        &format!("use {}\n", retired.pinned_id),
    );
    let found = scan::retired_selectors(dir.path(), &catalog).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, Path::new("docs/guide.md"));
}

#[test]
fn doctor_reads_home_files_from_its_explicit_home_not_the_bindings_parent() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), CATALOG, &fixture_data::fixture().to_string());
    let home = dir.path().join("user-home");
    let bindings = dir.path().join("elsewhere/records/qualified-bindings");
    std::fs::create_dir_all(&bindings).unwrap();
    let canary = |drift: &str| {
        format!(r#"{{"schema_version":1,"observed_ids":{{"orchid-one-pin":"{drift}"}}}}"#)
    };
    write(&home, "model-canary.json", &canary("home-drift"));
    // A decoy where the old code derived home: the bindings directory's parent.
    write(
        bindings.parent().unwrap(),
        "model-canary.json",
        &canary("decoy-drift"),
    );
    let mut options = opts(dir.path());
    options.codeflow_home = Some(home);
    options.qualification_dir = Some(bindings.clone());
    let result = doctor::run_check("model-bindings", &options).unwrap();
    assert!(result.status.is_warn(), "{}", result.message);
    assert!(result
        .message
        .contains("canary identity drift: orchid-one-pin observed home-drift"));
    assert!(
        !result.message.contains("decoy-drift"),
        "{}",
        result.message
    );
    // Binding records still come from the independently located directory.
    write(&bindings, "broken.json", "{}");
    let result = doctor::run_check("model-bindings", &options).unwrap();
    assert_eq!(result.status, Status::Fail, "{}", result.message);
    assert!(result.message.contains("broken.json"), "{}", result.message);
}

/// Every managed-catalog duty resolution, keyed by host, author and trigger.
fn managed_resolutions(root: &Path) -> String {
    use codeflow_core::model_catalog::{load_catalog, CatalogInputs, ResolveRequest};
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    let catalog = load_catalog(root).unwrap();
    let inputs = CatalogInputs::load(catalog, root, None, None).unwrap();
    let triggers: [&[String]; 3] = [
        &[],
        &["operator instruction".to_owned()],
        &["cross-cutting architecture or security".to_owned()],
    ];
    let empty = BTreeMap::new();
    let mut out = String::new();
    for family in &inputs.catalog.families {
        for host in &family.harnesses {
            for author in [None, Some("claude"), Some("codex"), Some("grok")] {
                for facts in triggers {
                    for duty in inputs.catalog.duties.keys() {
                        let result = inputs
                            .catalog
                            .resolve(&ResolveRequest {
                                duty,
                                task: "TSK-900",
                                host_harness: host,
                                author_lineage: author,
                                exclusions: &[],
                                observed_ids: &empty,
                                trigger_facts: facts,
                                area: None,
                                requested_override: None,
                                operator_override: None,
                            })
                            .unwrap();
                        writeln!(
                            out,
                            "{host} {} {facts:?} {duty}: {}",
                            author.unwrap_or("none"),
                            serde_json::to_string(&result).unwrap()
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
    out
}

const MANAGED_BASELINE: &str =
    "tests/fixtures/model-catalog/managed-resolutions-before-schema-2.txt";

/// With no project file, or a schema 1 file, every managed duty resolves
/// exactly as the 3.0.0 engine did. The baseline was recorded from main at
/// 447455ca5, before the project design-authority block existed, so it
/// carries the current managed catalog.
#[test]
fn managed_resolutions_without_a_schema_two_block_match_the_prior_engine() {
    let baseline_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MANAGED_BASELINE);
    let dir = tempfile::tempdir().unwrap();
    let absent = managed_resolutions(dir.path());
    if !baseline_path.exists() && std::env::var_os("CODEFLOW_RECORD_BASELINE").is_some() {
        std::fs::create_dir_all(baseline_path.parent().unwrap()).unwrap();
        std::fs::write(&baseline_path, &absent).unwrap();
    }
    let baseline = std::fs::read_to_string(&baseline_path).unwrap();
    assert_eq!(
        absent, baseline,
        "absent project file drifted from the prior engine"
    );
    write(
        dir.path(),
        ".codeflow/model-selection.json",
        r#"{"schema_version":1,"bindings":[]}"#,
    );
    assert_eq!(
        managed_resolutions(dir.path()),
        baseline,
        "schema 1 project file drifted from the prior engine"
    );
}

/// Issue 43 (TSK-236): doctor prints the project's design authority and
/// standing reviews as the working tree states them, and an invalid block
/// fails the check, which blocks preflight.
#[test]
fn doctor_prints_design_authority_and_fails_an_invalid_block() {
    let block = json!({
        "schema_version": 2,
        "bindings": [],
        "design_authority": {
            "seat": "orchid-seat",
            "designated": {"date": "2026-10-04", "record": "owner instruction fixture"},
            "lines": {
                "orchid-main": {"role": "owner"},
                "orchid-support": {"role": "co-owner", "approves": ["phase-exit"]}
            }
        },
        "standing_reviews": [
            {"area": "design", "seat": "cinder-seat", "mode": "read-only", "per": "phase",
             "record": "owner instruction fixture"}
        ]
    });
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), CATALOG, &fixture_data::fixture().to_string());
    write(
        dir.path(),
        ".codeflow/model-selection.json",
        &block.to_string(),
    );
    let result = doctor::run_check("model-bindings", &opts(dir.path())).unwrap();
    assert_ne!(result.status, Status::Fail, "{}", result.message);
    for expected in [
        "project design authority: seat orchid-seat: orchid-main owner; orchid-support \
         co-owner, approves at phase-exit; designated 2026-10-04, owner instruction fixture; \
         applies to a task once committed on its integration target; a same-family design \
         approval is never the independent review",
        "project standing review: standing assignment (area design, seat cinder-seat, \
         read-only, per phase; owner instruction fixture)",
    ] {
        assert!(result.message.contains(expected), "{}", result.message);
    }
    let mut invalid = block.clone();
    invalid["design_authority"]["lines"]["quartz-main"] = json!({"role": "co-owner"});
    write(
        dir.path(),
        ".codeflow/model-selection.json",
        &invalid.to_string(),
    );
    let result = doctor::run_check("model-bindings", &opts(dir.path())).unwrap();
    assert_eq!(result.status, Status::Fail, "{}", result.message);
    assert!(
        result.message.contains("another family"),
        "{}",
        result.message
    );
}
