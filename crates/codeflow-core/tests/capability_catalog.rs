//! Catalog contract exercised through checkout and immutable Git tree inputs.
use codeflow_core::capability::{
    load_capability_catalog, load_capability_catalog_from_tree, validate_capability_path,
};
use std::path::Path;

const RECORD: &str = "docs/capabilities/CAP-101-catalog.md";
const VALID: &str = include_str!("fixtures/capability-catalog/valid.md");
const VOCAB: &str = "inventory: incomplete\ngroups: [{name: records, about: Records}]\ntags: [{name: rust, about: Rust}]\n";

#[test]
fn tree_catalog_bounds_refuse_before_reading_or_parsing_payloads() {
    use codeflow_core::capability::{
        CAPABILITY_MAX_FILE_BYTES, CAPABILITY_MAX_TREE_BYTES, CAPABILITY_MAX_TREE_FILES,
    };
    for (count, size) in [
        (CAPABILITY_MAX_TREE_FILES + 1, 1),
        (
            CAPABILITY_MAX_TREE_BYTES / CAPABILITY_MAX_FILE_BYTES + 1,
            CAPABILITY_MAX_FILE_BYTES,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let blob = repo.blob(&vec![b'x'; size]).unwrap();
        let mut catalog = repo.treebuilder(None).unwrap();
        for n in 1..=count {
            catalog
                .insert(format!("CAP-{n:03}-example.md"), blob, 0o100_644)
                .unwrap();
        }
        let mut docs = repo.treebuilder(None).unwrap();
        docs.insert("capabilities", catalog.write().unwrap(), 0o040_000)
            .unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        root.insert("docs", docs.write().unwrap(), 0o040_000)
            .unwrap();
        let tree = root.write().unwrap().to_string();
        let error = load_capability_catalog_from_tree(dir.path(), &tree)
            .unwrap_err()
            .to_string();
        assert!(error.contains("catalog exceeds"), "{error}");
    }
}

#[test]
fn tree_catalog_bounds_the_directory_walk_and_keeps_companion_assets() {
    let dir = fixture();
    write(dir.path(), "docs/capabilities/images/icon.svg", "<svg/>\n");
    assert_eq!(
        load_capability_catalog(dir.path()).unwrap(),
        load_capability_catalog_from_tree(dir.path(), &tree(dir.path())).unwrap()
    );

    let repo = git2::Repository::open(dir.path()).unwrap();
    let mut child = repo.treebuilder(None).unwrap().write().unwrap();
    // Small shared metadata can describe paths too expensive to expand.
    // The walk is iterative and stops before accumulating those path bytes.
    for _ in 0..1_100 {
        let mut parent = repo.treebuilder(None).unwrap();
        parent.insert("x".repeat(120), child, 0o040_000).unwrap();
        child = parent.write().unwrap();
    }
    let mut docs = repo.treebuilder(None).unwrap();
    docs.insert("capabilities", child, 0o040_000).unwrap();
    let mut root = repo.treebuilder(None).unwrap();
    root.insert("docs", docs.write().unwrap(), 0o040_000)
        .unwrap();
    let error = load_capability_catalog_from_tree(dir.path(), &root.write().unwrap().to_string())
        .unwrap_err()
        .to_string();
    assert!(error.contains("path inventory exceeds"), "{error}");
}

#[test]
fn record_fences_accept_only_ascii_trailing_whitespace() {
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        &VALID.replacen("---\n", "---\u{a0}\n", 1),
    );
    refuse_both(dir.path(), RECORD, "record requires YAML frontmatter");
    write(
        dir.path(),
        RECORD,
        &VALID.replacen("\n---\n", "\n---\u{a0}\n", 1),
    );
    refuse_both(dir.path(), RECORD, "unterminated YAML frontmatter");
    write(dir.path(), RECORD, &VALID.replace("---\n", "--- \t\r\n"));
    let revision = tree(dir.path());
    assert!(load_capability_catalog(dir.path()).is_ok());
    assert!(load_capability_catalog_from_tree(dir.path(), &revision).is_ok());
}

#[test]
fn legacy_bounds_are_enforced_during_deserialization() {
    for (field, value, reason) in [
        ("name", "x".repeat(1025), "1024 characters"),
        (
            "epics",
            format!("[{}]", vec!["EPC-001"; 65].join(",")),
            "64 items",
        ),
        (
            "verified_by",
            format!("[{}]", vec!["proof"; 65].join(",")),
            "64 items",
        ),
    ] {
        let input = format!("```yaml\nid: CAP-201\n{field}: {value}\n```\n");
        let (_, issues) = codeflow_core::capability::parse_capabilities(&input);
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains(field) && issue.message.contains(reason)),
            "{field}: {issues:?}"
        );
    }
}

#[test]
fn vocabulary_bounds_abort_before_later_invalid_items() {
    let dir = fixture();
    write(dir.path(), "docs/capabilities/vocabulary.yaml", &format!("inventory: incomplete\ngroups:\n  - name: records\n    about: {}\n  - unexpected: value\ntags: []\n", "x".repeat(1025)));
    refuse_both(dir.path(), "vocabulary.yaml", "1024 characters");
    write(
        dir.path(),
        "docs/capabilities/vocabulary.yaml",
        &format!(
            "inventory: incomplete\ngroups: [{}{{unexpected: value}}]\n",
            "{name: records, about: Records},".repeat(65)
        ),
    );
    refuse_both(dir.path(), "vocabulary.yaml", "64 items");
}

#[test]
fn legacy_relationship_shapes_and_unknown_keys_are_preserved() {
    for (value, expected) in [
        ("EPC-001", vec!["EPC-001"]),
        ("[EPC-001]", vec!["EPC-001"]),
        ("null", vec![]),
    ] {
        let (entries, issues) = codeflow_core::capability::parse_capabilities(&format!(
            "```yaml\nid: CAP-201\nepics: {value}\nunknown_key: {{nested: ignored}}\n```\n"
        ));
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(entries[0].epics, expected);
    }
}

#[test]
fn alias_bomb_record_is_bounded() {
    assert_alias_bomb_bounded("alias_bomb_record_is_bounded", RECORD, "tags");
}

#[test]
fn alias_bomb_legacy_fence_is_bounded() {
    assert_alias_bomb_bounded(
        "alias_bomb_legacy_fence_is_bounded",
        "docs/capabilities.md",
        "epics",
    );
}

#[test]
fn alias_bomb_vocabulary_is_bounded() {
    assert_alias_bomb_bounded(
        "alias_bomb_vocabulary_is_bounded",
        "docs/capabilities/vocabulary.yaml",
        "groups",
    );
}

fn assert_alias_bomb_bounded(test_name: &str, file: &str, field: &str) {
    use std::time::{Duration, Instant};
    // Allow process startup, Git fixture writes and parallel-gate contention;
    // normal refusal is much faster, while a real expansion still times out.
    const ALIAS_BOMB_TIMEOUT: Duration = Duration::from_secs(10);
    const CHILD: &str = "CODEFLOW_CATALOG_ALIAS_BOMB_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name])
            .env("CODEFLOW_CATALOG_ALIAS_BOMB_CHILD", "1")
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "bounded alias-bomb subprocess failed");
                return;
            }
            if started.elapsed() > ALIAS_BOMB_TIMEOUT {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("alias bomb exceeded the {ALIAS_BOMB_TIMEOUT:?} subprocess limit");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let mut bomb = "&a [x,x,x,x,x,x,x,x,x,x]".to_string();
    for level in b'b'..=b'i' {
        let previous = char::from(level - 1);
        bomb = format!(
            "&{} [{bomb}{}]",
            char::from(level),
            format!(",*{previous}").repeat(9)
        );
    }
    let content = match file {
        "docs/capabilities.md" => format!("```yaml\nid: CAP-201\nepics: {bomb}\n```\n"),
        "docs/capabilities/vocabulary.yaml" => format!("inventory: incomplete\ngroups: {bomb}\n"),
        _ => format!("---\ntags: {bomb}\n---\n"),
    };
    assert!(content.len() < 1024);
    let dir = fixture();
    write(dir.path(), file, &content);
    let revision = tree(dir.path());
    let started = Instant::now();
    let error = load_capability_catalog(dir.path()).unwrap_err();
    assert!(error.message.contains(field), "{error}");
    assert!(started.elapsed() < ALIAS_BOMB_TIMEOUT);
    let started = Instant::now();
    let error = load_capability_catalog_from_tree(dir.path(), &revision).unwrap_err();
    assert!(error.message.contains(field), "{error}");
    assert!(started.elapsed() < ALIAS_BOMB_TIMEOUT);
}

#[test]
fn typed_record_bounds_name_the_field_and_limit() {
    for (from, to, key, expected) in [
        (
            "tags: [rust]",
            format!("tags: [{}]", vec!["rust"; 65].join(",")),
            "tags",
            "list exceeds 64 items",
        ),
        (
            "summary: Read capability records from a checkout or tree.",
            format!("summary: {}", "x".repeat(1025)),
            "summary",
            "value exceeds 1024 characters",
        ),
        (
            "kind: functional",
            "kind: functional\nunknown_key: true".to_string(),
            "unknown_key",
            "unknown field `unknown_key`",
        ),
    ] {
        let dir = fixture();
        write(dir.path(), RECORD, &VALID.replace(from, &to));
        refuse_both(dir.path(), RECORD, expected);
        assert!(load_capability_catalog(dir.path())
            .unwrap_err()
            .message
            .contains(key));
    }
}

#[test]
fn typed_record_size_limit_is_256_kib() {
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        &format!("{VALID}{}", "x".repeat(262_145 - VALID.len())),
    );
    refuse_both(dir.path(), RECORD, "262144 bytes");
}

#[test]
fn typed_record_accepts_bounded_scalar_aliases() {
    let dir = fixture();
    write(
        dir.path(),
        RECORD,
        &VALID
            .replace("summary: ", "summary: &text ")
            .replace("kind: functional", "kind: functional\nmeasure: *text"),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn review_input_bytes_are_bounded_before_parsing() {
    let dir = fixture();
    let maximum = codeflow_core::capability::CAPABILITY_MAX_FILE_BYTES;
    write(
        dir.path(),
        RECORD,
        &format!("{VALID}{}", "x".repeat(maximum - VALID.len())),
    );
    let revision = tree(dir.path());
    assert!(load_capability_catalog(dir.path()).is_ok());
    assert!(load_capability_catalog_from_tree(dir.path(), &revision).is_ok());
    let oversized = format!("{VALID}{}", "x".repeat(262_144));
    write(dir.path(), RECORD, &oversized);
    refuse_both(dir.path(), RECORD, "bytes");
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    write(dir.path(), "docs/capabilities.md", &"x".repeat(262_145));
    refuse_both(dir.path(), "docs/capabilities.md", "bytes");
    write(dir.path(), "docs/capabilities.md", "");
    write(
        dir.path(),
        "docs/capabilities/vocabulary.yaml",
        &" ".repeat(262_145),
    );
    refuse_both(dir.path(), "vocabulary.yaml", "262144 bytes");
    let (_, issues) = codeflow_core::capability::parse_capabilities(&"x".repeat(262_145));
    assert!(issues.iter().any(|issue| issue.message.contains("bytes")));
}

#[test]
fn typed_yaml_preserves_scalar_text_and_legacy_anchors() {
    for value in [
        "\"&literal *literal\"",
        "'&literal *literal'",
        "|\n  &literal *literal",
        "plain text with an ' apostrophe and & literal",
        ">\n  *literal\n  &literal",
    ] {
        let dir = fixture();
        write(
            dir.path(),
            RECORD,
            &VALID.replace(
                "summary: Read capability records from a checkout or tree.",
                &format!("summary: {value}"),
            ),
        );
        assert!(load_capability_catalog(dir.path()).is_ok(), "{value}");
    }
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities/vocabulary.yaml",
        &VOCAB.replace("inventory: incomplete", "inventory: &inventory incomplete"),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
    let (_, issues) = codeflow_core::capability::parse_capabilities(
        "```yaml\nid: CAP-001\nname: &name catalog\narea: engine\nstatus: planned\n```\n",
    );
    assert!(issues.is_empty());
}

#[test]
fn empty_or_invalid_catalog_is_present_for_reference_checks() {
    let dir = fixture();
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    write(
        dir.path(),
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ncapabilities: [CAP-999]\n---\n",
    );
    for invalid in [false, true] {
        if invalid {
            write(dir.path(), RECORD, "---\nunknown: true\n---\n");
        }
        let report = codeflow_core::validate::docs::lint_docs(dir.path());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("CAP-999")),
            "{report:?}"
        );
        assert!(!format!("{:?}", report.notes).contains("capability checks skipped"));
    }
}

#[test]
fn indexed_executable_mode_is_refused_even_with_filemode_disabled() {
    let dir = fixture();
    tree(dir.path());
    let repo = git2::Repository::open(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.fileMode", false)
        .unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new(RECORD), 0).unwrap();
    entry.mode = 0o100_755;
    index.add(&entry).unwrap();
    index.write().unwrap();
    assert!(load_capability_catalog(dir.path())
        .unwrap_err()
        .message
        .contains("executable"));
}

#[test]
fn review_docs_collect_errors_and_keep_reference_checks() {
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &format!("{}{}", fence("CAP-201", "bad"), fence("CAP-202", "bad")),
    );
    write(
        dir.path(),
        "docs/capabilities/CAP-102-other.md",
        "---\nunknown: true\n---\n",
    );
    write(
        dir.path(),
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ncapabilities: [CAP-101, CAP-999]\n---\n",
    );
    let report = codeflow_core::validate::docs::lint_docs(dir.path());
    assert_eq!(
        report
            .issues
            .iter()
            .filter(|i| i.message.contains("status:"))
            .count(),
        2,
        "{report:?}"
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.file == Path::new("docs/capabilities/CAP-102-other.md")),
        "{report:?}"
    );
    assert!(
        report.issues.iter().any(|i| i.message.contains("CAP-999")),
        "{report:?}"
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.message.contains("CAP-101") && i.message.contains("not found")),
        "{report:?}"
    );
    assert!(
        !format!("{:?}", report.notes).contains("capability checks skipped"),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn review_checkout_modes_follow_git_and_only_apply_to_records() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-201", "planned"),
    );
    let repo = git2::Repository::open(dir.path()).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.fileMode", false)
        .unwrap();
    for path in [RECORD, "docs/capabilities.md", ".codeflow/project.toml"] {
        std::fs::set_permissions(
            dir.path().join(path),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    assert!(load_capability_catalog(dir.path()).is_ok());
    for path in ["docs/capabilities.md", ".codeflow/project.toml"] {
        let mut entry = index.get_path(Path::new(path), 0).unwrap();
        entry.mode = 0o100_755;
        index.add(&entry).unwrap();
    }
    let revision = index.write_tree().unwrap().to_string();
    assert!(load_capability_catalog_from_tree(dir.path(), &revision).is_ok());
    std::fs::remove_file(dir.path().join(RECORD)).unwrap();
    assert!(load_capability_catalog(dir.path()).is_ok());
}

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
    index.write().unwrap();
    index.write_tree().unwrap().to_string()
}

fn refuse_both(root: &Path, file: &str, reason: &str) {
    let revision = tree(root);
    for result in [
        load_capability_catalog(root),
        load_capability_catalog_from_tree(root, &revision),
    ] {
        let error = result
            .map(|_| ())
            .expect_err("catalog must be refused")
            .to_string();
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
        ("oversized-list-item", "verified_by"),
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
    format!("```yaml\nid: {id}\nname: catalog\narea: engine\nstatus: {status}\nverified_by: ['test: crates/codeflow-core/tests/capability_catalog.rs']\n```\n")
}

#[test]
fn legacy_prose_is_bound_to_each_entry() {
    let dir = fixture();
    let legacy = format!(
        "# Catalog\n\n## First\n\n{}\nFirst contract.\n\n> ## Quoted heading\n\n### Details\n\nFirst details.\n\n# Another group\n\n## Second\n\n{}\nSecond contract.\n\n# Catalog schema\n\nGlobal guidance.\n",
        fence("CAP-098", "planned"),
        fence("CAP-099", "planned")
    );
    write(dir.path(), "docs/capabilities.md", &legacy);
    let catalog = load_capability_catalog(dir.path()).unwrap();
    let first = catalog
        .records
        .iter()
        .find(|r| r.entry.id == "CAP-098")
        .unwrap();
    let second = catalog
        .records
        .iter()
        .find(|r| r.entry.id == "CAP-099")
        .unwrap();
    assert!(first.prose.contains("First contract."));
    assert!(first.prose.contains("Quoted heading"));
    assert!(first.prose.contains("First details."));
    assert!(!first.prose.contains("Another group"));
    assert!(!first.prose.contains("Second"));
    assert!(second.prose.contains("Second contract."));
    assert!(!second.prose.contains("First"));
    assert!(!second.prose.contains("Global guidance"));
    assert!(!first.prose.contains("id: CAP-098"));
    assert_eq!(
        catalog,
        load_capability_catalog_from_tree(dir.path(), &tree(dir.path())).unwrap()
    );
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
        "areas = ['engine', 'new-area']\n",
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
        "areas = ['engine', 'new-area']\n",
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
            "test: crates/codeflow-core/tests/capability_catalog.rs",
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
            let error = validate_capability_path(path).unwrap_err().to_string();
            assert!(error.contains(path), "{error}");
        }
        if ["symlink", "executable"].contains(&case["case"].as_str().unwrap()) {
            let dir = fixture();
            let repo = git2::Repository::open(dir.path()).unwrap();
            let mut index = repo.index().unwrap();
            index
                .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            let mut entry = index.get_path(Path::new(path), 0).unwrap();
            let mode = case["mode"].as_u64().unwrap().to_string();
            entry.mode = u32::from_str_radix(&mode, 8).unwrap();
            let content = case["content"].as_str().unwrap();
            entry.id = repo
                .blob(if content == "valid.md" {
                    VALID.as_bytes()
                } else {
                    content.as_bytes()
                })
                .unwrap();
            index.add(&entry).unwrap();
            let revision = index.write_tree().unwrap().to_string();
            let error = load_capability_catalog_from_tree(dir.path(), &revision)
                .unwrap_err()
                .to_string();
            assert!(error.contains(path) && error.contains(&mode), "{error}");
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
            &format!("tags: [{}]", ["rust"; 64].join(", ")),
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
    write(
        dir.path(),
        "crates/codeflow-core/tests/capability_catalog.rs",
        "#[test]\nfn case() {}\n",
    );
    let report = codeflow_core::validate::docs::lint_docs(dir.path());
    assert!(report.is_clean(), "{report:?}");
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
            "test: crates/codeflow-core/tests/capability_catalog.rs",
            "other: tests",
            "verified_by",
        ),
        (
            "test: crates/codeflow-core/tests/capability_catalog.rs",
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

#[test]
fn legacy_validation_also_applies_to_pinned_trees() {
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-099", "unknown"),
    );
    refuse_both(dir.path(), "docs/capabilities.md", "status");
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-099", "shipped").replace(
            "['test: crates/codeflow-core/tests/capability_catalog.rs']",
            "[]",
        ),
    );
    refuse_both(dir.path(), "docs/capabilities.md", "verified_by");
}

#[test]
fn frontmatter_delimiter_whitespace_cannot_hide_a_layout_collision() {
    let dir = fixture();
    write(
        dir.path(),
        "docs/capabilities.md",
        &fence("CAP-101", "shipped"),
    );
    write(
        dir.path(),
        RECORD,
        &VALID.replacen("---\n", "---   \r\n", 1),
    );
    refuse_both(dir.path(), RECORD, "duplicate CAP-101");
}

#[test]
fn relationship_ids_are_syntax_checked_without_loading_their_targets() {
    let dir = fixture();
    for key in [
        "epics",
        "epic_id",
        "adrs",
        "specs",
        "capabilities",
        "depends_on",
        "related",
        "superseded_by",
    ] {
        write(
            dir.path(),
            RECORD,
            &VALID.replace(
                "kind: functional",
                &format!("kind: functional\n{key}: [not-an-id]"),
            ),
        );
        refuse_both(dir.path(), RECORD, key);
    }
    write(
        dir.path(),
        RECORD,
        &VALID.replace(
            "kind: functional",
            "kind: functional\nrelated: [TSK-999, CAP-999]\nepic_id: EPC-999",
        ),
    );
    assert!(load_capability_catalog(dir.path()).is_ok());
}

#[test]
fn vocabulary_lists_use_the_same_declared_bound() {
    use std::fmt::Write;
    let dir = fixture();
    let mut vocabulary = String::from("inventory: incomplete\ngroups: [{name: records, about: Records}]\ntags:\n  - {name: rust, about: Rust}\n");
    for n in 1..64 {
        writeln!(vocabulary, "  - {{name: tag-{n}, about: Tag}}").unwrap();
    }
    write(dir.path(), "docs/capabilities/vocabulary.yaml", &vocabulary);
    assert!(load_capability_catalog(dir.path()).is_ok());
    vocabulary.push_str("  - {name: overflow, about: Tag}\n");
    write(dir.path(), "docs/capabilities/vocabulary.yaml", &vocabulary);
    refuse_both(
        dir.path(),
        "docs/capabilities/vocabulary.yaml",
        "tags: list exceeds 64",
    );
}
