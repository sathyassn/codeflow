//! The records exemption is proven from real Git trees and the line policy.

use codeflow_core::workgraph::landing_class::{
    classify_landing, classify_landing_at_fetched_tip, LandingClass,
};
use std::path::Path;

/// A policy that declares task, epic and docs Markdown as records.
const DECLARED: &str = r#"{"git":{"record_paths":["project-management/tasks/**/*.md","project-management/epics/**/*.md","docs/**/*.md"]}}"#;

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
    let (dir, base) = fixture(DECLARED);
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
        let (dir, base) = fixture(DECLARED);
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
            r#"{"git":{"record_paths":["docs/**/*.md"],"breaking_watch_paths":["docs/guide.md"]}}"#,
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
        let (dir, base) = fixture(DECLARED);
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
    let (dir, base) = fixture(DECLARED);
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
        let (dir, base) = fixture(DECLARED);
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
    let (dir, base) = fixture(DECLARED);
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

#[test]
fn protected_and_release_targets_keep_the_full_gate() {
    use codeflow_core::workgraph::landing_class::classify_landing_into;
    for (policy, target, class, reason) in [
        (
            DECLARED,
            "integration/EPC-001-notes",
            LandingClass::RecordsOnly,
            "declared",
        ),
        (
            DECLARED,
            "integration/release-4.0",
            LandingClass::Code,
            "release pattern",
        ),
        (
            DECLARED,
            "main",
            LandingClass::Code,
            "not an integration line",
        ),
        (
            r#"{"git":{"record_paths":["docs/**/*.md"],"protected_branches":["main","integration/locked"]}}"#,
            "integration/locked",
            LandingClass::Code,
            "protected",
        ),
        (
            r#"{"git":{"record_paths":["docs/**/*.md"],"release_branch_pattern":"integration/stable-*"}}"#,
            "integration/stable-4",
            LandingClass::Code,
            "release pattern",
        ),
    ] {
        let (dir, base) = fixture(policy);
        write(dir.path(), "docs/guide.md", "Changed\n");
        commit(dir.path());
        let actual = classify_landing_into(dir.path(), &base, "HEAD", target);
        assert_eq!(actual.class, class, "{target}: {}", actual.reason);
        assert!(
            actual.reason.contains(reason),
            "{target}: {}",
            actual.reason
        );
    }
}

#[test]
fn only_links_on_record_paths_withhold_the_class() {
    for (link, class) in [
        ("CLAUDE.md", LandingClass::RecordsOnly),
        ("tools/vendor", LandingClass::RecordsOnly),
        ("docs/img/source", LandingClass::Code),
        ("project-management", LandingClass::Code),
    ] {
        let (dir, _) = fixture(DECLARED);
        let object = git(dir.path(), &["rev-parse", "HEAD:src/source.rs"]);
        git(
            dir.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{object},{link}"),
            ],
        );
        git(dir.path(), &["commit", "-qm", "test: add a link"]);
        let base = git(dir.path(), &["rev-parse", "HEAD"]);
        git(dir.path(), &["checkout", "-q", "--", "."]);
        write(dir.path(), "docs/guide.md", "Changed\n");
        git(dir.path(), &["add", "docs/guide.md"]);
        git(dir.path(), &["commit", "-qm", "test: change a record"]);
        decision(dir.path(), &base, class);
    }
}

/// Point the index entry `path` at `object` as a gitlink and commit it.
fn set_gitlink(root: &Path, path: &str, object: &str) {
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{object},{path}"),
        ],
    );
    git(root, &["commit", "-qm", "test: set a gitlink"]);
}

#[test]
fn any_submodule_change_keeps_the_full_gate_even_when_git_ignores_it() {
    for change in ["moved", "added", "removed", "unchanged"] {
        let (dir, first) = fixture(DECLARED);
        let root = dir.path();
        write(
            root,
            ".gitmodules",
            "[submodule \"vendor\"]\n\tpath = vendor\n\turl = ./vendor\n\tignore = all\n",
        );
        git(root, &["add", ".gitmodules"]);
        git(
            root,
            &["commit", "-qm", "test: ignore the vendor submodule"],
        );
        let second = git(root, &["rev-parse", "HEAD"]);
        if change != "added" {
            set_gitlink(root, "vendor", &first);
        }
        let base = git(root, &["rev-parse", "HEAD"]);
        match change {
            "moved" | "added" => set_gitlink(root, "vendor", &second),
            "removed" => {
                git(root, &["rm", "-q", "--cached", "vendor"]);
                git(root, &["commit", "-qm", "test: remove the gitlink"]);
            }
            _ => {}
        }
        write(root, "docs/guide.md", "Changed\n");
        git(root, &["add", "docs/guide.md"]);
        git(root, &["commit", "-qm", "test: change a record"]);
        let expected = if change == "unchanged" {
            LandingClass::RecordsOnly
        } else {
            LandingClass::Code
        };
        let actual = classify_landing(root, &base, "HEAD");
        assert_eq!(actual.class, expected, "{change}: {}", actual.reason);
    }
}

/// TSK-1306: a project that declares nothing keeps the full gate, whether its
/// policy omits the key or the line tip holds no policy at all.
#[test]
fn a_project_with_no_declaration_keeps_the_full_gate() {
    let (dir, base) = fixture("{}");
    write(dir.path(), "docs/guide.md", "Changed\n");
    write(dir.path(), "project-management/tasks/TSK-001.md", "Task\n");
    commit(dir.path());
    decision(dir.path(), &base, LandingClass::Code);
    let reason = classify_landing(dir.path(), &base, "HEAD").reason;
    assert!(reason.contains("declares no git.record_paths"), "{reason}");

    let no_policy = tempfile::tempdir().unwrap();
    git(no_policy.path(), &["init", "-q", "-b", "main"]);
    write(no_policy.path(), "docs/guide.md", "Old\n");
    let base = commit(no_policy.path());
    write(no_policy.path(), "docs/guide.md", "New\n");
    commit(no_policy.path());
    decision(no_policy.path(), &base, LandingClass::Code);
}

/// TSK-1306: a candidate that widens the list to cover its own code change is
/// refused, and the list is read from the line tip rather than the checkout.
#[test]
fn widening_the_list_never_covers_a_code_change_or_the_checkout() {
    let (dir, base) = fixture(r#"{"git":{"record_paths":["docs/**/*.md"]}}"#);
    write(
        dir.path(),
        ".codeflow/policy.json",
        r#"{"git":{"record_paths":["**"]}}"#,
    );
    write(dir.path(), "src/source.rs", "Changed code\n");
    commit(dir.path());
    let decided = classify_landing(dir.path(), &base, "HEAD");
    assert_eq!(decided.class, LandingClass::Code, "{}", decided.reason);

    // The tip declares nothing and only the uncommitted checkout widens: a
    // reader of the checkout would grant the class here.
    let (dir, base) = fixture("{}");
    write(dir.path(), "docs/guide.md", "Changed\n");
    commit(dir.path());
    write(
        dir.path(),
        ".codeflow/policy.json",
        r#"{"git":{"record_paths":["**"]}}"#,
    );
    decision(dir.path(), &base, LandingClass::Code);
}

/// A base handed to a run counts only when it is the line's fetched tip.
#[test]
fn a_supplied_base_counts_only_at_the_fetched_tip() {
    const LINE: &str = "integration/landing";
    let (dir, tip) = fixture(DECLARED);
    let root = dir.path();
    write(root, "src/source.rs", "Changed code\n");
    let code = commit(root);
    write(root, "docs/guide.md", "Changed\n");
    commit(root);
    let classify = |base: &str| classify_landing_at_fetched_tip(root, base, "HEAD", LINE);

    let no_tip = classify(&code);
    assert_eq!(no_tip.class, LandingClass::Code);
    assert!(
        no_tip.reason.contains("no fetched tip"),
        "{}",
        no_tip.reason
    );

    git(
        root,
        &["update-ref", &format!("refs/remotes/origin/{LINE}"), &tip],
    );
    // A base past the code commit would read records-only without the bind.
    let past = classify(&code);
    assert_eq!(past.class, LandingClass::Code);
    assert!(
        past.reason.contains("is not the fetched tip"),
        "{}",
        past.reason
    );
    // At the real tip the code commit is in the diff.
    let at_tip = classify(&tip);
    assert_eq!(at_tip.class, LandingClass::Code);
    assert!(at_tip.reason.contains("src/source.rs"), "{}", at_tip.reason);
    // A records-only candidate keeps the class at its tip.
    git(
        root,
        &["update-ref", &format!("refs/remotes/origin/{LINE}"), &code],
    );
    assert_eq!(classify(&code).class, LandingClass::RecordsOnly);
}
