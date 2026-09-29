//! Regression checks for retired checker contracts.
use std::path::Path;

#[test]
fn retired_direct_change_policy_is_accepted_and_diagnosed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        r#"{"schema_version":1,"git":{"direct_changes":"forbid"}}"#,
    )
    .unwrap();
    assert!(codeflow_core::hooks::policy_schema::validate_policy(dir.path()).is_ok());
    let notes = codeflow_core::hooks::policy_schema::deprecation_warnings(dir.path());
    assert!(notes
        .iter()
        .any(|n| n.to_string().contains("git.direct_changes")));
}

#[test]
fn retired_direct_change_floor_and_citations_are_gone() {
    fn visit(dir: &Path, needle: &str) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, needle);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "toml")
            ) {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains(needle),
                    "{} still cites retired rule",
                    path.display()
                );
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let retired = ["R-", "71"].concat();
    visit(&root.join("crates"), &retired);
    assert!(!codeflow_core::workgraph::classify::PATH_SETS_TOML.contains("[[direct_change_floor]]"));
}

#[test]
fn tracking_state_message_has_one_owner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codeflow-cli/src/cmd");
    let needle = ["cannot determine ", "durable-work tracking"].concat();
    let paths = [
        "ci.rs",
        "ci/classification.rs",
        "ci/work_records.rs",
        "ci/id_registry.rs",
        "ids.rs",
        "new.rs",
        "work.rs",
    ];
    let count: usize = paths
        .iter()
        .map(|p| {
            std::fs::read_to_string(root.join(p))
                .unwrap()
                .matches(&needle)
                .count()
        })
        .sum();
    assert_eq!(count, 0, "message belongs to the shared core helper");
}

#[test]
fn remote_only_predecessor_review_uses_its_remote_and_refuses_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["commit", "--allow-empty", "-qm", "base"]);
    run(&[
        "remote",
        "add",
        "origin",
        "https://github.com/other/project.git",
    ]);
    run(&[
        "remote",
        "add",
        "upstream",
        "https://github.com/owner/project.git",
    ]);
    run(&[
        "update-ref",
        "refs/remotes/upstream/task/TSK-001-work",
        "HEAD",
    ]);
    let resolve =
        || codeflow_core::workgraph::work_start::review_repository(dir.path(), "task/TSK-001-work");
    assert_eq!(resolve().unwrap(), "github.com/owner/project");
    run(&[
        "update-ref",
        "refs/remotes/origin/task/TSK-001-work",
        "HEAD",
    ]);
    assert!(resolve().is_err());
}

#[test]
fn planning_remedies_name_epic_and_standalone_routes() {
    for remedy in [
        &codeflow_core::remedy::WORK_START_MERGE_PLANNING,
        &codeflow_core::remedy::TASK_RECORD_MISSING,
    ] {
        assert!(remedy.text.contains("standalone"), "{}", remedy.text);
        assert!(remedy.text.contains("epic"), "{}", remedy.text);
    }
}
