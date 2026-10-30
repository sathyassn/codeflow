//! Issues 83 and 136: an amendment registers its planning home in the index.
use super::{combined, git, planned_task, run_in};
use std::path::Path;

const TASK: &str = "project-management/tasks/TSK-001.md";
const BRANCH: &str = "plan/EPC-001-content-brief";
const BODY: &str = "Task: EPC-001\n\n## Summary\nRegister the planning home.\n\n- amend the criterion and index\n\n## Changes\n- register the brief\n\n## Reviews\nNone: independent review follows\n";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
}

fn fixture(indexes: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "user.name", "Test"]);
    write(
        root,
        ".codeflow/policy.json",
        &serde_json::json!({"git": {
            "product_paths": ["src/**", "docs/portal/**"],
            "document_indexes": indexes
        }})
        .to_string(),
    );
    write(root, "project-management/epics/EPC-001.md", "---\nid: EPC-001\ntitle: content brief\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-12\n---\n\n## Summary\nA content brief.\n\n## Acceptance Criteria\n- AC-1 When planned, the brief shall be registered.\n");
    write(
        root,
        TASK,
        &planned_task("TSK-001", "")
            .replace("epic_id: null", "epic_id: EPC-001")
            .replace("standalone_reason: bounded work", "standalone_reason: null"),
    );
    write(root, "DOC-INDEX.md", "# Documents\n");
    commit(root, "chore: plan the work");
    git(root, &["switch", "-qc", BRANCH]);
    let record = std::fs::read_to_string(root.join(TASK)).unwrap();
    write(
        root,
        TASK,
        &record.replace("shall work.", "shall register the brief."),
    );
    write(root, "docs/plan/content-brief/plan.md", "# Content brief\n");
    write(root, "docs/research/content-brief.md", "# Research\n");
    commit(root, "docs: amend the content brief");
    dir
}

fn ci(root: &Path) -> std::process::Output {
    run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--pr-body",
            BODY,
        ],
    )
}

#[test]
fn an_epic_amendment_can_register_its_planning_home_in_a_declared_index() {
    let dir = fixture(&["DOC-INDEX.md"]);
    let root = dir.path();
    // Issue 136 steps 1 to 3: amend criteria and add the planning packet.
    let before = ci(root);
    assert!(before.status.success(), "{}", combined(&before));
    // Step 4: the index row belongs in the same amendment.
    write(
        root,
        "DOC-INDEX.md",
        "# Documents\n\n- [Content brief](docs/plan/content-brief/plan.md)\n",
    );
    commit(root, "docs: register the content brief");
    let after = ci(root);
    let text = combined(&after);
    assert!(after.status.success(), "{text}");
    assert!(
        text.contains("planning-only amendment of EPC-001"),
        "{text}"
    );
    assert!(!text.contains("work.criteria_frozen"), "{text}");
    assert!(
        text.lines().any(
            |line| line.contains("instruction and doc files:") && line.contains("DOC-INDEX.md")
        ),
        "{text}"
    );
}

#[test]
fn an_excluded_path_names_the_admitted_set_and_the_declared_indexes() {
    for (indexes, path) in [
        (vec![], "DOC-INDEX.md"),
        (
            vec!["DOC-INDEX.md", "docs/portal/INDEX.md"],
            "docs/portal/INDEX.md",
        ),
    ] {
        let dir = fixture(&indexes);
        let root = dir.path();
        let before = ci(root);
        assert!(before.status.success(), "{}", combined(&before));
        write(root, path, "# Index\n\n- Content brief\n");
        commit(root, "docs: register the brief outside the admitted set");
        let output = ci(root);
        let text = combined(&output);
        assert_eq!(output.status.code(), Some(1), "{text}");
        for needle in [
            "work.classification",
            path,
            "admitted paths:",
            "project-management/** except templates",
            "docs/plan/**",
            "docs/**",
            "git.document_indexes",
            "AGENTS.md only outside its managed block",
            "first declare a document index",
        ] {
            assert!(text.contains(needle), "missing {needle:?}: {text}");
        }
        let declared = if indexes.is_empty() {
            "none declared".into()
        } else {
            indexes.join(", ")
        };
        assert!(
            text.contains(&format!("target policy ({declared})")),
            "{text}"
        );
    }
}

#[test]
fn an_amendment_cannot_declare_its_own_index() {
    let dir = fixture(&[]);
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"document_indexes":["DOC-INDEX.md"]}}"#,
    );
    write(root, "DOC-INDEX.md", "# Documents\n\n- Content brief\n");
    commit(root, "chore: declare an index in the amendment");
    let output = ci(root);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("none declared"), "{text}");
}
