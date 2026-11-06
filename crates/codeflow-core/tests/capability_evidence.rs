//! Every `verified_by` entry resolves to a test, a verification record or a
//! declared gap (TSK-362), read through `validate --docs` on a real checkout.
use codeflow_core::capability::{
    load_capability_catalog, load_capability_catalog_from_tree, parse_evidence, Evidence,
    TestIdentity, CAPABILITY_MAX_EVIDENCE_FILE_BYTES,
};
use codeflow_core::validate::docs::{lint_docs, DocsLintReport};
use std::path::Path;

const TEST_FILE: &str = "crates/app/tests/flow.rs";
const SOURCE_FILE: &str = "crates/app/src/lib.rs";
const VERIFICATION: &str = "docs/verification/native-run.md";
const RECORD: &str = "docs/capabilities/CAP-101-catalog.md";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn checkout() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git2::Repository::init(root).unwrap();
    write(root, ".codeflow/project.toml", "areas = ['engine']\n");
    write(root, TEST_FILE, "#[test]\nfn real_case() {}\n");
    write(
        root,
        SOURCE_FILE,
        "pub fn run() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn inline_case() {}\n}\n",
    );
    write(root, VERIFICATION, "# A native run\n");
    write(
        root,
        "web/app.test.mjs",
        "test(\"x\", () => {});\nfunction helper_named_case() {}\n",
    );
    dir
}

fn fence(root: &Path, entries: &[&str]) {
    let list: Vec<String> = entries.iter().map(|entry| format!("{entry:?}")).collect();
    write(
        root,
        "docs/capabilities.md",
        &format!(
            "```yaml\nid: CAP-001\nname: thing\narea: engine\nstatus: shipped\nverified_by: [{}]\n```\n",
            list.join(", ")
        ),
    );
}

fn record(root: &Path, stage: &str, entries: &[&str]) {
    let list: Vec<String> = entries.iter().map(|entry| format!("{entry:?}")).collect();
    write(
        root,
        RECORD,
        &format!(
            "---\nid: CAP-101\nuid: 3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55\nname: catalog\nsummary: Reads records.\narea: engine\nstatus: shipped\nstage: {stage}\nkind: functional\naudience: [internal]\nverified_by: [{}]\n---\n\nProse.\n",
            list.join(", ")
        ),
    );
}

fn evidence_issues(report: &DocsLintReport) -> Vec<String> {
    report
        .issues
        .iter()
        .map(ToString::to_string)
        .filter(|issue| issue.contains("verified_by"))
        .collect()
}

/// Refused in a fence and in a folder record, naming the capability and entry.
fn assert_refused(entry: &str, reason: &str) {
    let dir = checkout();
    fence(dir.path(), &[entry]);
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert_eq!(issues.len(), 1, "{entry}: {issues:?}");
    let shown = format!("{entry:?}");
    for wanted in ["CAP-001", shown.as_str(), reason] {
        assert!(issues[0].contains(wanted), "wanted {wanted}: {issues:?}");
    }

    let dir = checkout();
    record(dir.path(), "stable", &[entry]);
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert_eq!(issues.len(), 1, "{entry}: {issues:?}");
    for wanted in ["CAP-101", shown.as_str(), reason] {
        assert!(issues[0].contains(wanted), "wanted {wanted}: {issues:?}");
    }
}

fn assert_resolves(entry: &str) {
    let dir = checkout();
    fence(dir.path(), &[entry]);
    let report = lint_docs(dir.path());
    assert!(evidence_issues(&report).is_empty(), "{entry}: {report:?}");
    assert!(report.warnings.is_empty(), "{entry}: {report:?}");

    let dir = checkout();
    let stage = if entry.starts_with("gap:") {
        "unassessed"
    } else {
        "stable"
    };
    record(dir.path(), stage, &[entry]);
    let report = lint_docs(dir.path());
    assert!(evidence_issues(&report).is_empty(), "{entry}: {report:?}");
}

#[test]
fn each_valid_kind_resolves() {
    for entry in [
        "test: crates/app/tests/flow.rs",
        "test: crates/app/tests/flow.rs::real_case",
        "test: crates/app/src/lib.rs::tests",
        "test: crates/app/src/lib.rs::inline_case",
        "test: crates/app/src/lib.rs::tests::inline_case",
        "test: web/app.test.mjs",
        "test: web/app.test.mjs::helper_named_case",
        "journey: crates/app/tests/flow.rs::real_case",
        "record: docs/verification/native-run.md",
        "gap: test: no test covers this yet",
        "gap: journey: nothing drives the whole path",
        "gap: hosted: the hosted run is not recorded",
        "gap: native: no real host has run this",
    ] {
        assert_resolves(entry);
    }
}

#[test]
fn a_fabricated_tag_is_refused() {
    assert_refused("test: crates/app/tests/fabricated.rs", "does not exist");
    assert_refused("journey: crates/app/tests/fabricated.rs", "does not exist");
    assert_refused("record: docs/verification/fabricated.md", "does not exist");
}

#[test]
fn a_renamed_test_is_refused() {
    let tag = "test: crates/app/tests/flow.rs::real_case";
    let dir = checkout();
    fence(dir.path(), &[tag]);
    assert!(evidence_issues(&lint_docs(dir.path())).is_empty());
    write(dir.path(), TEST_FILE, "#[test]\nfn renamed_case() {}\n");
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(
        issues[0].contains("has no test named real_case"),
        "{issues:?}"
    );

    std::fs::rename(
        dir.path().join(TEST_FILE),
        dir.path().join("crates/app/tests/moved.rs"),
    )
    .unwrap();
    fence(dir.path(), &["test: crates/app/tests/flow.rs"]);
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert!(issues[0].contains("does not exist"), "{issues:?}");
}

#[test]
fn a_source_file_is_refused_as_evidence() {
    assert_refused("test: crates/app/src/lib.rs", "is a source file");
    assert_refused("journey: crates/app/src/lib.rs", "is a source file");
    // A name in source must be a test, not any function.
    assert_refused("test: crates/app/src/lib.rs::run", "has no test named run");
}

#[test]
fn a_named_test_must_be_declared_not_merely_mentioned() {
    let dir = checkout();
    write(
        dir.path(),
        TEST_FILE,
        "// fn ghost() is planned\nconst NAME: &str = \"fn ghost\";\nfn ghost_helper() {}\n",
    );
    fence(dir.path(), &["test: crates/app/tests/flow.rs::ghost"]);
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert!(issues[0].contains("has no test named ghost"), "{issues:?}");
}

#[test]
fn malformed_entries_are_refused() {
    for (entry, reason) in [
        (
            "test: cargo test capability_catalog",
            "command or free text",
        ),
        ("test: /etc/passwd/tests/x.rs", "repository-relative"),
        ("test: ../outside/tests/x.rs", "repository-relative"),
        ("test: crates/app/tests/./flow.rs", "repository-relative"),
        ("test: crates\\app\\tests\\flow.rs", "repository-relative"),
        ("test: crates/app/tests/flow.rs::", "is not a test name"),
        ("record: crates/app/tests/flow.rs", "docs/verification/"),
        (
            "record: docs/verification/../../secrets.md",
            "repository-relative",
        ),
        ("record: docs/verification/", "repository-relative"),
        ("gap: tests pending", "gap: <proof>: <reason>"),
        ("gap: engine: tests pending", "gap: <proof>: <reason>"),
        ("gap: test: ", "gap: <proof>: <reason>"),
    ] {
        assert_refused(entry, reason);
    }
}

#[test]
fn folders_data_and_fixtures_are_not_evidence() {
    let dir = checkout();
    write(
        dir.path(),
        "crates/app/tests/fixtures/case.rs",
        "fn x() {}\n",
    );
    write(dir.path(), "docs/verification/empty.md", "");
    std::fs::create_dir_all(dir.path().join("docs/verification/folder")).unwrap();
    for (entry, reason) in [
        (
            "test: crates/app/tests/fixtures/case.rs",
            "is a source file",
        ),
        ("test: crates/app/tests", "not a regular file"),
        ("record: docs/verification/empty.md", "is empty"),
        ("record: docs/verification/folder", "not a regular file"),
    ] {
        fence(dir.path(), &[entry]);
        let issues = evidence_issues(&lint_docs(dir.path()));
        assert!(
            issues.first().is_some_and(|issue| issue.contains(reason)),
            "{entry}: {issues:?}"
        );
    }
}

#[test]
fn a_named_lookup_over_the_read_bound_is_refused_and_a_whole_file_is_not_read() {
    let dir = checkout();
    let large = dir.path().join("crates/app/tests/large.rs");
    let file = std::fs::File::create(large).unwrap();
    file.set_len(CAPABILITY_MAX_EVIDENCE_FILE_BYTES + 1)
        .unwrap();
    fence(dir.path(), &["test: crates/app/tests/large.rs::case"]);
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert!(issues[0].contains("cannot be read"), "{issues:?}");
    fence(dir.path(), &["test: crates/app/tests/large.rs"]);
    assert!(evidence_issues(&lint_docs(dir.path())).is_empty());
}

#[test]
fn resolving_runs_nothing_an_entry_names() {
    let dir = checkout();
    let proof = dir.path().join("ran");
    write(
        dir.path(),
        "tests/test_probe.sh",
        &format!("#!/bin/sh\ntouch '{}'\n", proof.display()),
    );
    fence(dir.path(), &["test: tests/test_probe.sh"]);
    assert!(evidence_issues(&lint_docs(dir.path())).is_empty());
    assert!(!proof.exists());
}

#[test]
fn a_control_character_in_an_entry_is_refused_and_escaped() {
    let dir = checkout();
    write(
        dir.path(),
        "docs/capabilities.md",
        "```yaml\nid: CAP-001\nname: thing\narea: engine\nstatus: shipped\nverified_by: [\"test: crates/app/tests/\\x1b[2Jx.rs\"]\n```\n",
    );
    let issues = evidence_issues(&lint_docs(dir.path()));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("repository-relative"), "{issues:?}");
    assert!(!issues[0].contains('\u{1b}'), "{issues:?}");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_repository_is_refused() {
    let dir = checkout();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), "#[test]\nfn outside_case() {}\n").unwrap();
    let link = dir.path().join("crates/app/tests/link.rs");
    std::os::unix::fs::symlink(outside.path(), link).unwrap();
    for tag in [
        "test: crates/app/tests/link.rs",
        "test: crates/app/tests/link.rs::outside_case",
    ] {
        fence(dir.path(), &[tag]);
        assert_eq!(evidence_issues(&lint_docs(dir.path())).len(), 1, "{tag}");
    }
}

/// A sandbox can deny a read by the file's name. The index then proves a whole
/// file is tracked; nothing else is taken on trust.
#[cfg(unix)]
#[test]
fn a_denied_read_resolves_only_a_tracked_whole_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = checkout();
    let root = dir.path();
    let locked = root.join("crates/app/tests");
    let repo = git2::Repository::open(root).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(TEST_FILE)).unwrap();
    index.write().unwrap();
    write(root, "crates/app/tests/untracked.rs", "fn x() {}\n");
    std::os::unix::fs::symlink("flow.rs", locked.join("link.rs")).unwrap();
    index
        .add_path(Path::new("crates/app/tests/link.rs"))
        .unwrap();
    let mut gitlink = index.get_path(Path::new(TEST_FILE), 0).unwrap();
    gitlink.mode = 0o160_000;
    gitlink.path = b"crates/app/tests/module.rs".to_vec();
    gitlink.id = repo.blob(b"not read").unwrap();
    index.add(&gitlink).unwrap();
    index.write().unwrap();

    let check = |tag: &str| {
        fence(root, &[tag]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let issues = evidence_issues(&lint_docs(root));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        issues
    };
    // A process that ignores directory modes, such as root, cannot show this.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let enforced = std::fs::read_dir(&locked).is_err();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !enforced {
        return;
    }
    assert!(check("test: crates/app/tests/flow.rs").is_empty());
    assert_eq!(check("test: crates/app/tests/untracked.rs").len(), 1);
    assert_eq!(check("test: crates/app/tests/flow.rs::real_case").len(), 1);
    // A tracked link or gitlink is not a regular file, as an unlocked checkout
    // would also find.
    assert_eq!(check("test: crates/app/tests/link.rs").len(), 1);
    assert_eq!(check("test: crates/app/tests/module.rs").len(), 1);
}

#[test]
fn legacy_fence_without_any_typed_entry_warns_and_is_not_refused() {
    let dir = checkout();
    fence(dir.path(), &["free-form-tag", "another tag"]);
    let report = lint_docs(dir.path());
    assert!(evidence_issues(&report).is_empty(), "{report:?}");
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    assert!(report.warnings[0].text.contains("CAP-001"));
}

#[test]
fn a_folder_record_does_not_change_how_an_untyped_fence_is_read() {
    let dir = checkout();
    fence(dir.path(), &["free-form-tag"]);
    record(dir.path(), "stable", &["test: crates/app/tests/flow.rs"]);
    let report = lint_docs(dir.path());
    assert!(evidence_issues(&report).is_empty(), "{report:?}");
    assert_eq!(report.warnings.len(), 1, "{report:?}");
}

#[test]
fn once_a_fence_is_typed_a_free_form_tag_is_refused() {
    let dir = checkout();
    fence(
        dir.path(),
        &["test: crates/app/tests/flow.rs", "fabricated-tag"],
    );
    let report = lint_docs(dir.path());
    let issues = evidence_issues(&report);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("\"fabricated-tag\""), "{issues:?}");
    assert!(issues[0].contains("needs a kind"), "{issues:?}");
    assert!(report.warnings.is_empty(), "{report:?}");
}

#[test]
fn shipped_with_only_a_gap_must_be_unassessed_in_both_readers() {
    let dir = checkout();
    let revision = |root: &Path| {
        let repo = git2::Repository::open(root).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write_tree().unwrap().to_string()
    };
    record(dir.path(), "unassessed", &["gap: test: not written yet"]);
    assert!(load_capability_catalog(dir.path()).is_ok());
    let tree = revision(dir.path());
    assert!(load_capability_catalog_from_tree(dir.path(), &tree).is_ok());

    for stage in ["experimental", "beta", "stable"] {
        record(dir.path(), stage, &["gap: test: not written yet"]);
        let tree = revision(dir.path());
        for error in [
            load_capability_catalog(dir.path()).unwrap_err(),
            load_capability_catalog_from_tree(dir.path(), &tree).unwrap_err(),
        ] {
            assert!(error.to_string().contains("evidence"), "{stage}: {error}");
        }
    }
    // Real proof beside the gap lifts the requirement.
    record(
        dir.path(),
        "experimental",
        &["test: crates/app/tests/flow.rs", "gap: native: no real run"],
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn the_grammar_returns_the_typed_parts() {
    assert_eq!(
        parse_evidence("journey: a/tests/b.rs::c::d"),
        Ok(Evidence::Journey(TestIdentity {
            path: "a/tests/b.rs",
            name: Some("c::d"),
        }))
    );
    assert_eq!(
        parse_evidence("gap: native: no run: yet"),
        Ok(Evidence::Gap {
            proof: "native",
            reason: "no run: yet",
        })
    );
}

/// TSK-362 AC-2: this repository's own catalog resolves, each native or hosted
/// promise points at a record or is a declared gap, and CAP-008 discloses that
/// its tests stub the GitHub boundary.
#[test]
fn this_repositorys_capabilities_all_resolve() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let issues = evidence_issues(&lint_docs(&root));
    assert!(issues.is_empty(), "{issues:#?}");

    let catalog = load_capability_catalog(&root).unwrap();
    assert!(catalog.records.len() >= 17);
    for record in &catalog.records {
        for entry in &record.entry.verified_by {
            assert!(
                parse_evidence(entry).is_ok(),
                "{}: {entry}",
                record.entry.id
            );
        }
    }
    let cap008 = catalog
        .records
        .iter()
        .find(|record| record.entry.id == "CAP-008")
        .unwrap();
    assert!(
        cap008
            .entry
            .verified_by
            .iter()
            .any(|entry| entry.starts_with("gap: hosted:") && entry.contains("stub")),
        "{:?}",
        cap008.entry.verified_by
    );
}
