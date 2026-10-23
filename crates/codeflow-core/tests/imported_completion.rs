//! Source proof endpoints survive later receiving-line work (TSK-339).
use std::path::Path;
use std::process::Command;

use codeflow_core::workgraph::acceptance::imported_records;
use codeflow_core::workgraph::lifecycle::Graph;
use git2::{Oid, Repository};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn commit(root: &Path) -> Oid {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "fixture change"]);
    Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap()
}
const PATH: &str = "project-management/tasks/TSK-001.md";
const TASK: &str = "---\nid: TSK-001\nuid: source-identity\nepic_id: null\nstandalone_reason: Fixture\nstatus: todo\nintegration_target: main\n---\n\n## Acceptance Criteria\n\n- AC-1 (journey) Work succeeds.\n\n## Closeout\n\n";

#[test]
fn import_proof_names_home_landing_and_original_completion_tail() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.test"]);
    write(root, PATH, TASK);
    let base = commit(root);
    git(root, &["switch", "-qc", "task/TSK-001-source"]);
    write(root, "src/shared.rs", "reviewed source\n");
    let reviewed = commit(root);
    let completed = format!("{}```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: fixture review\n  criteria:\n    AC-1: verified | fixture\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n", TASK.replace("status: todo", "status: complete"));
    write(root, PATH, &completed);
    let completion = commit(root);
    git(root, &["switch", "main"]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-qm",
            "land source",
            "task/TSK-001-source",
        ],
    );
    let landed = Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap();
    git(
        root,
        &["switch", "-qc", "task/TSK-002-receiver", &base.to_string()],
    );
    write(root, "src/receiver.rs", "receiving work\n");
    let receiver_base = commit(root);
    git(root, &["merge", "--no-ff", "-qm", "import source", "main"]);
    write(root, "src/shared.rs", "later receiving work\n");
    let receiver = commit(root);
    let repo = Repository::open(root).unwrap();
    let graph = Graph::from_revision(&repo, "HEAD").unwrap();
    let imports = imported_records(&repo, receiver_base, receiver, &graph).unwrap();
    let source = &imports["TSK-001"];
    assert_eq!(source.home, "main");
    assert_eq!(source.landing, landed);
    let proof = source.completion.unwrap();
    assert_eq!(proof.reviewed, reviewed);
    assert_eq!(proof.completion_tail, completion);
    assert_ne!(proof.completion_tail, receiver);

    write(
        root,
        PATH,
        &completed.replace("fixture review", "changed review"),
    );
    let changed = commit(root);
    let graph = Graph::from_revision(&repo, "HEAD").unwrap();
    assert!(imported_records(&repo, receiver_base, changed, &graph)
        .unwrap()
        .is_empty());
}
