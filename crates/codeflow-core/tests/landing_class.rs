//! The records exemption is proven from real Git trees and the line policy.

use codeflow_core::workgraph::landing_class::{classify_landing, LandingClass};
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> String {
    let output = codeflow_core::git::command()
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "test: fixture"]);
    git(root, &["rev-parse", "HEAD"])
}

fn fixture(policy: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    write(dir.path(), ".codeflow/policy.json", policy);
    write(dir.path(), "docs/guide.md", "Old guide\n");
    write(
        dir.path(),
        "src/source.rs",
        "A substantial source file copied as documentation.\nIt is still a source change.\n",
    );
    let base = commit(dir.path());
    (dir, base)
}

fn decision(root: &Path, base: &str, class: LandingClass) {
    let actual = classify_landing(root, base, "HEAD");
    assert_eq!(actual.class, class, "{}", actual.reason);
    assert_eq!(
        actual.class.gate_mode(),
        if class == LandingClass::Code {
            "full"
        } else {
            "quick"
        }
    );
}

#[test]
fn records_only_candidate_selects_quick_from_the_exact_line_tip() {
    let (dir, base) = fixture("{}");
    write(dir.path(), "docs/guide.md", "New guide\n");
    write(dir.path(), "project-management/tasks/TSK-001.md", "Task\n");
    write(dir.path(), "project-management/epics/EPC-001.md", "Epic\n");
    write(dir.path(), "docs/plan/current.md", "Plan\n");
    commit(dir.path());
    decision(dir.path(), &base, LandingClass::RecordsOnly);
    decision(dir.path(), "HEAD", LandingClass::Code);
    decision(dir.path(), "missing-ref", LandingClass::Code);
}

#[test]
fn product_config_instructions_and_undeclared_paths_keep_the_full_gate() {
    for path in [
        "src/source.rs",
        "docs/data.json",
        "docs/config.toml",
        "docs/config.yaml",
        "docs/AGENTS.md",
        "docs/SKILL.md",
        "docs/.hidden/guide.md",
        "project-management/templates/task.md",
        "README.md",
    ] {
        let (dir, base) = fixture("{}");
        write(dir.path(), path, "Changed\n");
        commit(dir.path());
        decision(dir.path(), &base, LandingClass::Code);
    }
}

#[test]
fn record_paths_are_configurable_but_cannot_admit_code_or_watched_files() {
    for (policy, path, class) in [
        (
            r#"{"git":{"record_paths":["notes/**/*.md"]}}"#,
            "notes/one.md",
            LandingClass::RecordsOnly,
        ),
        (
            r#"{"git":{"record_paths":[]}}"#,
            "docs/guide.md",
            LandingClass::Code,
        ),
        (
            r#"{"git":{"record_paths":["**"]}}"#,
            "docs/data.json",
            LandingClass::Code,
        ),
        (
            r#"{"git":{"record_paths":["**"],"product_paths":["notes/**"]}}"#,
            "notes/one.md",
            LandingClass::Code,
        ),
        (
            r#"{"git":{"breaking_watch_paths":["docs/guide.md"]}}"#,
            "docs/guide.md",
            LandingClass::Code,
        ),
        (
            r#"{"git":{"record_paths":["["]}}"#,
            "docs/guide.md",
            LandingClass::Code,
        ),
    ] {
        let (dir, base) = fixture(policy);
        write(dir.path(), path, "Changed\n");
        commit(dir.path());
        decision(dir.path(), &base, class);
    }
}

#[test]
fn a_candidate_cannot_expand_its_own_record_paths() {
    let (dir, base) = fixture(r#"{"git":{"record_paths":[]}}"#);
    write(
        dir.path(),
        ".codeflow/policy.json",
        r#"{"git":{"record_paths":["**"]}}"#,
    );
    write(dir.path(), "docs/guide.md", "Changed\n");
    commit(dir.path());
    decision(dir.path(), &base, LandingClass::Code);
}

#[test]
fn copies_and_renames_from_code_keep_the_full_gate() {
    for rename in [false, true] {
        let (dir, base) = fixture("{}");
        let root = dir.path();
        if rename {
            git(root, &["mv", "src/source.rs", "docs/copied.md"]);
        } else {
            std::fs::copy(root.join("src/source.rs"), root.join("docs/copied.md")).unwrap();
        }
        commit(root);
        decision(root, &base, LandingClass::Code);
    }
}

#[test]
fn record_rename_and_deletion_stay_records() {
    let (dir, base) = fixture("{}");
    git(dir.path(), &["mv", "docs/guide.md", "docs/renamed.md"]);
    commit(dir.path());
    decision(dir.path(), &base, LandingClass::RecordsOnly);
    git(dir.path(), &["rm", "docs/renamed.md"]);
    commit(dir.path());
    decision(dir.path(), &base, LandingClass::RecordsOnly);
}

#[test]
fn mode_symlink_and_submodule_changes_keep_the_full_gate() {
    for mode in ["100755", "120000", "160000"] {
        let (dir, base) = fixture("{}");
        let object = if mode == "160000" {
            base.clone()
        } else {
            git(dir.path(), &["rev-parse", "HEAD:docs/guide.md"])
        };
        git(
            dir.path(),
            &[
                "update-index",
                "--cacheinfo",
                &format!("{mode},{object},docs/guide.md"),
            ],
        );
        git(dir.path(), &["commit", "-qm", "test: change mode"]);
        decision(dir.path(), &base, LandingClass::Code);
    }
}

#[test]
fn a_diverged_candidate_cannot_use_its_merge_base_as_the_line_tip() {
    let (dir, base) = fixture("{}");
    write(dir.path(), "docs/guide.md", "Candidate\n");
    let head = commit(dir.path());
    git(dir.path(), &["checkout", "--detach", &base]);
    write(dir.path(), "line.txt", "New line\n");
    let line = commit(dir.path());
    assert_eq!(
        classify_landing(dir.path(), &line, &head).class,
        LandingClass::Code
    );
}
