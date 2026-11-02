//! Catalog contract exercised through checkout and immutable Git tree inputs.
use codeflow_core::capability::{
    load_capability_catalog, load_capability_catalog_from_tree, validate_capability_path,
};
use std::path::Path;

const RECORD: &str = "docs/capabilities/CAP-101-catalog.md";
const VALID: &str = include_str!("fixtures/capability-catalog/valid.md");
const VOCAB: &str = "inventory: incomplete\ngroups: [{name: records, about: Records}]\ntags: [{name: rust, about: Rust}]\n";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    write(dir.path(), ".codeflow/project.toml", "areas = ['engine']\n");
    write(dir.path(), "docs/capabilities/vocabulary.yaml", VOCAB);
    write(dir.path(), RECORD, VALID);
    dir
}

fn tree(root: &Path) -> String {
    let repo = git2::Repository::open(root).unwrap();
    let mut index = repo.index().unwrap();
    index.clear().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write_tree().unwrap().to_string()
}

fn refuse_both(root: &Path, file: &str, reason: &str) {
    let revision = tree(root);
    for result in [
        load_capability_catalog(root),
        load_capability_catalog_from_tree(root, &revision),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains(file), "{error}");
        assert!(error.contains(reason), "wanted {reason}: {error}");
    }
}

#[test]
fn schema_refusal_fixtures_name_the_file_and_key() {
    for (name, reason) in [
        ("unknown-key", "unknown"),
        ("non-numeric-id", "id"),
        ("malformed-yaml", "YAML"),
        ("duplicate-key", "duplicate"),
        ("oversized-value", "summary"),
        ("oversized-list", "tags"),
        ("oversized-keys", "frontmatter"),
        ("gap-stable", "non-gap"),
        ("unknown-area", "area"),
    ] {
        let dir = fixture();
        let content = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/capability-catalog/{name}.md")),
        )
        .unwrap();
        write(dir.path(), RECORD, &content);
        refuse_both(dir.path(), RECORD, reason);
    }
}

#[test]
fn checkout_and_pinned_tree_produce_equal_records_and_prose() {
    let dir = fixture();
    let revision = tree(dir.path());
    let checkout = load_capability_catalog(dir.path()).unwrap();
    assert_eq!(
        checkout,
        load_capability_catalog_from_tree(dir.path(), &revision).unwrap()
    );
    assert!(checkout.records[0].prose.contains("## Concept"));
    write(dir.path(), RECORD, "changed checkout");
    assert_eq!(
        checkout,
        load_capability_catalog_from_tree(dir.path(), &revision).unwrap()
    );
}

#[test]
fn identities_are_unique_across_files() {
    for (fixture_name, path, reason) in [
        (
            "duplicate-id",
            "docs/capabilities/CAP-101-other.md",
            "id: duplicate",
        ),
        (
            "duplicate-uid",
            "docs/capabilities/CAP-102-other.md",
            "uid: duplicate",
        ),
    ] {
        let dir = fixture();
        let content = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/capability-catalog/{fixture_name}.md"
        )))
        .unwrap();
        write(dir.path(), path, &content);
        refuse_both(dir.path(), RECORD, reason);
        assert!(load_capability_catalog(dir.path())
            .unwrap_err()
            .to_string()
            .contains(path));
    }
}

fn fence(id: &str, status: &str) -> String {
    format!("```yaml\nid: {id}\nname: catalog\narea: engine\nstatus: {status}\nverified_by: ['test: cargo test capability_catalog']\n```\n")
}

#[test]
fn legacy_and_folder_coexist_but_never_overlap_regardless_of_binding_or_payload() {
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-099", "shipped"),
    );
    assert_eq!(
        load_capability_catalog(dir.path()).unwrap().records.len(),
        2
    );
    assert_eq!(
        load_capability_catalog(dir.path()).unwrap(),
        load_capability_catalog_from_tree(dir.path(), &tree(dir.path())).unwrap()
    );
    for bound in [false, true] {
        for status in ["shipped", "building"] {
            write(
                dir.path(),
                "docs/capabilities.md",
                &fence("CAP-101", status),
            );
            if bound {
                write(
                    dir.path(),
                    "ids/CAP/101.toml",
                    "uid = '3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55'\n",
                );
            }
            refuse_both(dir.path(), "docs/capabilities.md", "duplicate CAP-101");
            assert!(load_capability_catalog(dir.path())
                .unwrap_err()
                .to_string()
                .contains(RECORD));
        }
    }
}

#[test]
fn each_revision_uses_its_own_areas_and_vocabulary() {
    let dir = fixture();
    let base = tree(dir.path());
    write(
        dir.path(),
        ".codeflow/project.toml",
        "areas = ['new-area']\n",
    );
    write(
        dir.path(),
        "docs/capabilities/vocabulary.yaml",
        &VOCAB.replace("records", "new-group"),
    );
    write(
        dir.path(),
        RECORD,
        &VALID
            .replace("area: engine", "area: new-area")
            .replace("group: records", "group: new-group"),
    );
    let head = tree(dir.path());
    assert_eq!(
        load_capability_catalog_from_tree(dir.path(), &base)
            .unwrap()
            .records[0]
            .entry
            .area,
        "engine"
    );
    assert_eq!(
        load_capability_catalog_from_tree(dir.path(), &head)
            .unwrap()
            .records[0]
            .entry
            .area,
        "new-area"
    );
    write(dir.path(), ".codeflow/project.toml", "areas = ['engine']\n");
    refuse_both(dir.path(), RECORD, "area");
    write(
        dir.path(),
        ".codeflow/project.toml",
        "areas = ['new-area']\n",
    );
    write(dir.path(), "docs/capabilities/vocabulary.yaml", VOCAB);
    refuse_both(dir.path(), RECORD, "group");
}

#[test]
fn unassessed_gap_is_accepted_and_evidence_references_are_not_opened() {
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        include_str!("fixtures/capability-catalog/gap-unassessed.md"),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
    assert!(load_capability_catalog_from_tree(dir.path(), &tree(dir.path())).is_ok());
    write(
        dir.path(),
        RECORD,
        &VALID.replace(
            "test: cargo test capability_catalog",
            "record: docs/verification/outside-catalog.md",
        ),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn unsafe_paths_and_tree_modes_are_refused() {
    let cases: Vec<serde_yaml::Value> = serde_yaml::from_str(include_str!(
        "fixtures/capability-catalog/unsafe-files.yaml"
    ))
    .unwrap();
    for case in cases {
        let path = case["path"].as_str().unwrap();
        if ["absolute", "traversal", "nested"].contains(&case["case"].as_str().unwrap()) {
            assert!(validate_capability_path(path).is_err());
        }
    }
    let dir = fixture();
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    write(
        dir.path(),
        "docs/capabilities/nested/CAP-101-catalog.md",
        VALID,
    );
    refuse_both(dir.path(), "CAP-101-catalog.md", "directly");
}

#[cfg(unix)]
#[test]
fn symlink_and_executable_files_are_refused_in_both_inputs() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = fixture();
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    symlink("../../outside.md", dir.path().join(RECORD)).unwrap();
    refuse_both(dir.path(), RECORD, "");
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    write(dir.path(), RECORD, VALID);
    std::fs::set_permissions(
        dir.path().join(RECORD),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    refuse_both(dir.path(), RECORD, "executable");
}

#[test]
fn bounds_accept_exact_limits_and_count_unicode_characters() {
    let dir = fixture();
    let content = VALID
        .replace(
            "summary: Read capability records from a checkout or tree.",
            &format!("summary: {}", "é".repeat(1024)),
        )
        .replace(
            "tags: [rust]",
            &format!("tags: [{}]", vec!["rust"; 64].join(", ")),
        );
    write(dir.path(), RECORD, &content);
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn folder_records_participate_in_docs_graph_and_epic_reciprocity() {
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        &VALID.replace("kind: functional", "kind: functional\nepics: [EPC-001]"),
    );
    write(
        dir.path(),
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ncapabilities: []\n---\n",
    );
    let report = codeflow_core::validate::docs::lint_docs(dir.path());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.file == Path::new(RECORD)
                && issue.message.contains("does not link back")),
        "{report:?}"
    );
    write(
        dir.path(),
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ncapabilities: [CAP-101]\n---\n",
    );
    assert!(codeflow_core::validate::docs::lint_docs(dir.path()).is_clean());
}

#[test]
fn required_facets_and_conditional_fields_are_checked() {
    for (from, to, key) in [
        ("status: shipped", "status: ready", "status"),
        ("stage: stable", "stage: mature", "stage"),
        ("kind: functional", "kind: service", "kind"),
        ("kind: functional", "kind: non-functional", "quality"),
        (
            "kind: functional",
            "kind: non-functional\nquality: security",
            "measure",
        ),
        (
            "kind: functional",
            "kind: functional\nquality: magical",
            "quality",
        ),
        (
            "audience: [internal, agent]",
            "audience: [public]",
            "audience",
        ),
        ("audience: [internal, agent]", "audience: []", "audience"),
        ("stage: stable\n", "", "stage"),
        (
            "test: cargo test capability_catalog",
            "other: tests",
            "verified_by",
        ),
        (
            "test: cargo test capability_catalog",
            "test: ",
            "verified_by",
        ),
        ("tags: [rust]", "tags: [unknown]", "tags"),
        ("3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55", "not-a-uuid", "uid"),
    ] {
        let dir = fixture();
        write(dir.path(), RECORD, &VALID.replace(from, to));
        refuse_both(dir.path(), RECORD, key);
    }
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        &VALID.replace(
            "kind: functional",
            "kind: non-functional\nquality: security\nmeasure: Refuse every unsafe record file.",
        ),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn companion_pages_remain_prose_until_frontmatter_is_added() {
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-099", "shipped"),
    );
    write(
        dir.path(),
        "docs/capabilities/CAP-099-companion.md",
        "# Companion\n\nExisting prose.\n",
    );
    let checkout = load_capability_catalog(dir.path()).unwrap();
    assert_eq!(checkout.records.len(), 2);
    assert_eq!(
        checkout,
        load_capability_catalog_from_tree(dir.path(), &tree(dir.path())).unwrap()
    );
    write(dir.path(), RECORD, "# Missing frontmatter\n");
    refuse_both(dir.path(), RECORD, "frontmatter");
}

#[test]
fn vocabulary_refuses_unknown_duplicate_and_malformed_definitions() {
    for content in [
        "inventory: unknown\n", "inventory: incomplete\nextra: true\n",
        "inventory: incomplete\ngroups: [{name: Bad, about: bad}]\n",
        "inventory: incomplete\ngroups: [{name: records, about: one}, {name: records, about: two}]\n",
        "inventory: incomplete\ntags: [\n",
    ] {
        let dir = fixture();
        write(dir.path(), "docs/capabilities/vocabulary.yaml", content);
        refuse_both(dir.path(), "docs/capabilities/vocabulary.yaml", "");
    }
}

#[cfg(unix)]
#[test]
fn linked_catalog_parent_is_refused_without_reading_its_target() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::os::unix::fs::symlink("../../outside", dir.path().join("docs/capabilities")).unwrap();
    refuse_both(dir.path(), "docs/capabilities", "");
}
