//! Capability identities and coverage through the public CLI.

use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Catalog fixture"]);
    git(
        dir.path(),
        &["config", "user.email", "catalog@example.test"],
    );
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/project.toml"),
        "schema_version = 1\ntier = 'standard'\nscaffold_version = '4.0.0'\nstack = 'rust'\nareas = ['engine']\npolicy_armed = false\ngit_hooks = 'unwired'\npermission_preset = 'default'\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "chore: seed"]);
    dir
}

fn enable_tracking(root: &Path) {
    let path = root.join(".codeflow/project.toml");
    let config = std::fs::read_to_string(&path)
        .unwrap()
        .replace("tier = 'standard'", "tier = 'full'");
    std::fs::write(path, config).unwrap();
}

fn new(root: &Path, name: &str) -> String {
    success(cli(
        root,
        &[
            "capability",
            "new",
            name,
            "--summary",
            "A catalog fixture capability",
            "--area",
            "engine",
            "--audience",
            "agent",
        ],
    ))
}

#[test]
fn local_then_registry_allocation_keeps_the_number_occupied() {
    let dir = repo();
    let root = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "-qu", "origin", "main"]);
    assert!(new(root, "local").contains("CAP-001"));
    assert!(git(
        root,
        &["ls-remote", "--heads", "origin", "codeflow/registry"]
    )
    .is_empty());
    let first = git(root, &["show", "codeflow/registry:ids/CAP/001.toml"]);
    assert!(first.contains("uid"));
    std::fs::remove_file(root.join("docs/capabilities/CAP-001-local.md")).unwrap();
    enable_tracking(root);
    assert!(new(root, "tracked").contains("CAP-002"));
    assert!(git(
        remote.path(),
        &["show", "codeflow/registry:ids/CAP/001.toml"]
    )
    .contains("uid"));
    assert!(git(
        remote.path(),
        &["show", "codeflow/registry:ids/CAP/002.toml"]
    )
    .contains("uid"));
}

#[test]
fn tracking_on_issues_on_the_authority_registry() {
    let dir = repo();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        dir.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir.path(), &["push", "-qu", "origin", "main"]);
    enable_tracking(dir.path());
    assert!(new(dir.path(), "tracked").contains("CAP-001"));
    assert!(git(
        remote.path(),
        &["show", "codeflow/registry:ids/CAP/001.toml"]
    )
    .contains("uid"));
}

#[test]
fn legacy_fence_reserves_the_high_water_mark() {
    let dir = repo();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities.md"),
        "# Capabilities\n\n```yaml\nid: CAP-017\nname: old\narea: engine\nstatus: planned\n```\n",
    )
    .unwrap();
    git(dir.path(), &["add", "docs"]);
    git(
        dir.path(),
        &["commit", "-qm", "docs: add legacy capability"],
    );
    assert!(new(dir.path(), "next").contains("CAP-018"));
}

#[test]
fn invalid_facets_do_not_reserve_an_id() {
    let dir = repo();
    let output = cli(
        dir.path(),
        &[
            "capability",
            "new",
            "invalid",
            "--summary",
            "Invalid area",
            "--area",
            "missing",
            "--audience",
            "agent",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("area"));
    assert!(new(dir.path(), "valid").contains("CAP-001"));
}

fn assert_folder_coverage(args: &[&str]) {
    let dir = repo();
    new(dir.path(), "folder-only");
    let text = success(cli(dir.path(), args));
    assert!(
        text.contains("inventory: incomplete (1 capabilities)"),
        "{text}"
    );
    assert!(text.contains("evidence declarations: 0/0"), "{text}");
    assert!(text.contains("1 unassessed"), "{text}");
    assert!(!text.contains("capabilities.md absent"), "{text}");
}

#[test]
fn folder_only_status_reports_coverage() {
    assert_folder_coverage(&["status"]);
}
#[test]
fn folder_only_orient_reports_coverage() {
    assert_folder_coverage(&["orient"]);
}
#[test]
fn folder_only_doctor_reports_coverage() {
    assert_folder_coverage(&["doctor", "--check", "capabilities"]);
}

#[test]
fn init_reports_coverage() {
    let dir = repo();
    let text = success(cli(dir.path(), &["init", "--standard", "--yes"]));
    assert!(
        text.contains("inventory: incomplete (0 capabilities)"),
        "{text}"
    );
    assert!(text.contains("evidence declarations:"), "{text}");
}

#[test]
fn update_reports_coverage() {
    let dir = repo();
    success(cli(dir.path(), &["init", "--standard", "--yes"]));
    let text = success(cli(dir.path(), &["update"]));
    assert!(
        text.contains("inventory: incomplete (0 capabilities)"),
        "{text}"
    );
    assert!(text.contains("evidence declarations:"), "{text}");
}

#[test]
fn complete_inventory_does_not_imply_evidence_coverage() {
    let dir = repo();
    new(dir.path(), "unassessed");
    let file = dir.path().join("docs/capabilities/CAP-001-unassessed.md");
    let record = std::fs::read_to_string(&file).unwrap().replace(
        "status: planned",
        "status: shipped\nstage: unassessed\nverified_by: ['gap: test: not assessed']",
    );
    std::fs::write(&file, &record).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities/vocabulary.yaml"),
        "inventory: complete\n",
    )
    .unwrap();
    let text = success(cli(dir.path(), &["orient"]));
    assert!(
        text.contains("inventory: complete (1 capabilities)"),
        "{text}"
    );
    assert!(text.contains("evidence declarations: 0/1"), "{text}");
    assert!(text.contains("1 unassessed"), "{text}");
    let assessed = record
        .replace("stage: unassessed", "stage: experimental")
        .replace("gap: test: not assessed", "test: tests/catalog_search.rs");
    std::fs::write(&file, &assessed).unwrap();
    let text = success(cli(dir.path(), &["orient"]));
    assert!(text.contains("evidence declarations: 1/1"), "{text}");
    assert!(text.contains("0 unassessed"), "{text}");
    let partial = assessed.replace(
        "['test: tests/catalog_search.rs']",
        "['test: tests/catalog_search.rs', 'gap: test: recovery untested']",
    );
    std::fs::write(&file, partial).unwrap();
    let text = success(cli(dir.path(), &["orient"]));
    assert!(text.contains("evidence declarations: 0/1"), "{text}");
}

#[test]
fn empty_inventory_is_never_complete() {
    let dir = repo();
    std::fs::create_dir_all(dir.path().join("docs/capabilities")).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities/vocabulary.yaml"),
        "inventory: complete\n",
    )
    .unwrap();
    let text = success(cli(dir.path(), &["orient"]));
    assert!(
        text.contains("inventory: incomplete (0 capabilities)"),
        "{text}"
    );
}

#[test]
fn capability_on_another_branch_reserves_its_number() {
    let dir = repo();
    git(dir.path(), &["switch", "-qc", "feat/catalog"]);
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities.md"),
        "```yaml\nid: CAP-040\nname: another\nstatus: planned\n```\n",
    )
    .unwrap();
    git(dir.path(), &["add", "docs"]);
    git(dir.path(), &["commit", "-qm", "docs: another catalog"]);
    git(dir.path(), &["switch", "-q", "main"]);
    assert!(new(dir.path(), "next").contains("CAP-041"));
}

#[test]
fn invalid_catalog_on_a_side_branch_names_the_ref_and_revision() {
    let dir = repo();
    git(dir.path(), &["switch", "-qc", "feat/bad-catalog"]);
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities.md"),
        "```yaml\nid: CAP-040\nname: invalid\nstatus: unknown\n```\n",
    )
    .unwrap();
    git(dir.path(), &["add", "docs"]);
    git(dir.path(), &["commit", "-qm", "docs: invalid side catalog"]);
    let sha = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "-q", "main"]);
    let output = cli(
        dir.path(),
        &[
            "capability",
            "new",
            "next",
            "--summary",
            "Next capability",
            "--area",
            "engine",
            "--audience",
            "agent",
        ],
    );
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("refs/heads/feat/bad-catalog"), "{error}");
    assert!(error.contains(&sha[..8]), "{error}");
    assert!(error.contains("docs/capabilities.md"), "{error}");
}

#[test]
fn status_omits_coverage_when_the_catalog_is_absent() {
    let dir = repo();
    let text = success(cli(dir.path(), &["status"]));
    assert!(!text.contains("capability coverage:"), "{text}");
}

#[test]
fn retarget_repairs_a_two_clone_capability_collision() {
    let first = repo();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q", "-b", "main"]);
    git(
        first.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(first.path(), &["push", "-qu", "origin", "main"]);
    let second = tempfile::tempdir().unwrap();
    codeflow_fixture::clone(second.path(), remote.path(), ".").run();
    git(
        second.path(),
        &["config", "user.name", "Second catalog fixture"],
    );
    git(
        second.path(),
        &["config", "user.email", "second@example.test"],
    );
    assert!(new(first.path(), "first").contains("CAP-001"));
    assert!(new(second.path(), "second").contains("CAP-001"));
    let before = codeflow_core::capability::load_capability_catalog(second.path()).unwrap();
    let uid = before.records[0].uid.clone().unwrap();
    std::fs::write(
        second.path().join("docs/links.md"),
        "See [CAP-001](docs/capabilities/CAP-001-second.md).\n",
    )
    .unwrap();
    git(second.path(), &["add", "docs/links.md"]);
    success(cli(first.path(), &["ids", "sync"]));
    let collision = cli(second.path(), &["ids", "sync"]);
    assert!(!collision.status.success());
    let error = String::from_utf8(collision.stderr).unwrap();
    assert!(error.contains("ids retarget CAP-001"), "{error}");
    let text = success(cli(second.path(), &["ids", "retarget", "CAP-001"]));
    assert!(text.contains("CAP-001  ->  CAP-002"), "{text}");
    let after = codeflow_core::capability::load_capability_catalog(second.path()).unwrap();
    assert_eq!(after.records.len(), 1);
    assert_eq!(after.records[0].entry.id, "CAP-002");
    assert_eq!(after.records[0].uid.as_deref(), Some(uid.as_str()));
    assert_eq!(after.records[0].former_ids, ["CAP-001"]);
    assert!(!second
        .path()
        .join("docs/capabilities/CAP-001-second.md")
        .exists());
    assert_eq!(
        std::fs::read_to_string(second.path().join("docs/links.md")).unwrap(),
        "See [CAP-002](docs/capabilities/CAP-002-second.md).\n"
    );
    let bound = git(
        remote.path(),
        &["show", "codeflow/registry:ids/CAP/002.toml"],
    );
    assert_eq!(
        codeflow_core::ids::capability_uid_from_blob(2, bound.as_bytes()).unwrap(),
        uid
    );
}

#[test]
fn retarget_refuses_a_landed_capability() {
    let dir = repo();
    new(dir.path(), "landed");
    git(dir.path(), &["add", "docs"]);
    git(dir.path(), &["commit", "-qm", "feat: land capability"]);
    let output = cli(dir.path(), &["ids", "retarget", "CAP-001"]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("has landed on main"), "{error}");
}

#[test]
fn retarget_refuses_a_capability_already_bound_to_its_uid() {
    let dir = repo();
    new(dir.path(), "bound");
    let output = cli(dir.path(), &["ids", "retarget", "CAP-001"]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("already binds CAP-001 to this record"),
        "{error}"
    );
}

#[test]
fn retarget_legacy_capability_requires_migration() {
    let dir = repo();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/capabilities.md"),
        "```yaml\nid: CAP-017\nname: legacy\nstatus: building\n```\n",
    )
    .unwrap();
    let output = cli(dir.path(), &["ids", "retarget", "CAP-017"]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("migrate the legacy fence to a folder record"),
        "{error}"
    );
    assert!(!error.contains("backfill"), "{error}");
}
