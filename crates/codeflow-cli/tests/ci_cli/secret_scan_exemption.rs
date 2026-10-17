//! Issue 139: an exemption reaches the trusted base before the integration
//! line whose false positive it covers. Only dependency readiness is waived.
use super::{codeflow, git};
use std::path::Path;
use std::process::Output;

const LINE: &str = "integration/EPC-001-example";
const BRANCH: &str = "fix/scan-exemption";
const CLASS: &str = "pull request class: secret-scan exemption";

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn task(id: &str, depends: &str, status: &str) -> String {
    format!("---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: {LINE}\ntitle: example\nstatus: {status}\nwork_type: fix\nspecs: []\ndepends_on: [{depends}]\ncreated: 2026-10-16\n---\n\n## Description\nA bounded repair.\n\n## Acceptance Criteria\n- AC-1 (journey) The command shall work in a fresh project.\n")
}

// Build synthetic history with libgit2. These tests exercise CI, not client
// hooks or human approval; no hook or protection configuration is changed.
fn commit(root: &Path, message: &str) {
    let repo = git2::Repository::open(root).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    let parent = repo.head().ok().map(|head| head.peel_to_commit().unwrap());
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        message,
        &tree,
        &parent.iter().collect::<Vec<_>>(),
    )
    .unwrap();
}

fn fixture(full: bool) -> tempfile::TempDir {
    fixture_with_record(full, &task("TSK-002", "TSK-001", "todo"))
}

fn fixture_with_record(full: bool, record: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    if full {
        let out = codeflow()
            .args(["init", "--yes", "--full"])
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", said(&out));
    }
    write(root, "project-management/epics/EPC-001.md", "---\nid: EPC-001\ntitle: example\nstatus: in_progress\nwork_type: feat\nspecs: []\ncreated: 2026-10-16\n---\n\n## Summary\nA bounded outcome.\n\n## Acceptance Criteria\n- AC-1 The command shall work.\n");
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "", "todo"),
    );
    write(root, "project-management/tasks/TSK-002.md", record);
    commit(root, "plan: anchor example work");
    git(root, &["switch", "-q", "-c", LINE]);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "", "complete"),
    );
    commit(root, "fix: complete predecessor on the line");
    git(root, &["switch", "-q", "-c", BRANCH, "main"]);
    // A fictional fingerprint, not a credential. The class judges paths;
    // the independent scan decides whether any finding is exempted.
    write(
        root,
        ".gitleaksignore",
        "0123456789012345678901234567890123456789:sample.txt:generic-api-key:1\n",
    );
    commit(root, "fix: exempt a reviewed false positive");
    dir
}

fn body(id: &str) -> String {
    format!("Task: {id}\n\n## Summary\nExempt a reviewed false positive before its integration line lands.\n\n- Keep the exemption in its own range.\n\n## Changes\n- Add the reviewed fingerprint.\n\n## Testing\nAt fixture HEAD, ran the fixture check:\n```text\nfixture check passed\n```\nCoverage: not measured in this fictional project.\nNew tests: none.\nNot tested: hosted scanning.\n\n## Reviews\nNone: independent review follows.\n\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Exempt a reviewed false positive.\n- Migration: none\n")
}

fn ci(root: &Path, branch: &str, body: &str) -> Output {
    codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            BRANCH,
            "--branch",
            branch,
            "--into",
            "main",
            "--pr-body",
            body,
        ])
        .current_dir(root)
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn exemption_accepts_epic_and_task_with_dependency_complete_only_on_line() {
    let dir = fixture(false);
    for id in ["EPC-001", "TSK-002"] {
        let out = ci(dir.path(), BRANCH, &body(id));
        assert!(out.status.success(), "{id}: {}", said(&out));
        assert!(said(&out).contains(CLASS), "{}", said(&out));
        assert!(!said(&out).contains("work.stable_planning_anchor"));
    }
}

// Change only the committed tree; CI must judge the head, not the checkout.
fn change_ignore_entry(root: &Path, mode: Option<i32>, content_change: bool) {
    let repo = git2::Repository::open(root).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.find_reference("refs/heads/main")
        .unwrap()
        .set_target(parent.id(), "anchor the existing ignore file")
        .unwrap();
    let tree = parent.tree().unwrap();
    let mut builder = repo.treebuilder(Some(&tree)).unwrap();
    if let Some(mode) = mode {
        let oid = if mode == 0o160_000 {
            parent.id()
        } else if content_change {
            repo.blob(b"another-reviewed-commit:sample.txt:generic-api-key:2\n")
                .unwrap()
        } else {
            tree.get_name(".gitleaksignore").unwrap().id()
        };
        builder.insert(".gitleaksignore", oid, mode).unwrap();
    } else {
        builder.remove(".gitleaksignore").unwrap();
    }
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let sig = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "fix: change the ignore entry",
        &tree,
        &[&parent],
    )
    .unwrap();
}

const TRACKED: &str = "pull request class: tracked TSK-002";

// An epic-named range of another shape is refused; a task-named one stays
// ordinary tracked work, as before the class existed.
fn refused_entry(mode: Option<i32>) {
    let dir = fixture(false);
    change_ignore_entry(dir.path(), mode, false);
    let out = ci(dir.path(), BRANCH, &body("EPC-001"));
    assert!(!out.status.success(), "{}", said(&out));
    assert!(!said(&out).contains(CLASS), "{}", said(&out));
    assert!(said(&out).contains("work.classification"), "{}", said(&out));
    tracked_entry(mode);
}

fn tracked_entry(mode: Option<i32>) {
    let ready = fixture_with_record(false, &task("TSK-002", "", "todo"));
    let unready = fixture(false);
    for (dir, passes) in [(&ready, true), (&unready, false)] {
        change_ignore_entry(dir.path(), mode, false);
        let out = ci(dir.path(), BRANCH, &body("TSK-002"));
        assert_eq!(out.status.success(), passes, "{}", said(&out));
        assert!(said(&out).contains(TRACKED), "{}", said(&out));
        assert!(!said(&out).contains(CLASS), "{}", said(&out));
        assert!(
            !said(&out).contains("work.classification"),
            "{}",
            said(&out)
        );
        assert_eq!(
            said(&out).contains("work.stable_planning_anchor"),
            !passes,
            "{}",
            said(&out)
        );
    }
}

#[test]
fn exemption_rejects_ignore_deletion() {
    refused_entry(None);
}

#[test]
fn exemption_rejects_ignore_symlink() {
    refused_entry(Some(0o120_000));
}

#[test]
fn exemption_rejects_ignore_gitlink() {
    refused_entry(Some(0o160_000));
}

#[test]
fn exemption_rejects_ignore_mode_change() {
    refused_entry(Some(0o100_755));
}

#[test]
fn exemption_rejects_nonregular_additions_at_the_head() {
    for mode in [0o120_000, 0o160_000] {
        let dir = fixture(false);
        let repo = git2::Repository::open(dir.path()).unwrap();
        let base = repo.refname_to_id("refs/heads/main").unwrap();
        change_ignore_entry(dir.path(), Some(mode), false);
        repo.find_reference("refs/heads/main")
            .unwrap()
            .set_target(base, "keep the ignore file absent on the base")
            .unwrap();
        let out = ci(dir.path(), BRANCH, &body("EPC-001"));
        assert!(!out.status.success(), "{}", said(&out));
        assert!(!said(&out).contains(CLASS), "{}", said(&out));
        assert!(said(&out).contains("regular file"), "{}", said(&out));
        let out = ci(dir.path(), BRANCH, &body("TSK-002"));
        assert!(said(&out).contains(TRACKED), "{}", said(&out));
        assert!(!said(&out).contains(CLASS), "{}", said(&out));
    }
}

#[test]
fn exemption_accepts_regular_file_modification() {
    let dir = fixture(false);
    change_ignore_entry(dir.path(), Some(0o100_644), true);
    for id in ["EPC-001", "TSK-002"] {
        let out = ci(dir.path(), BRANCH, &body(id));
        assert!(out.status.success(), "{}", said(&out));
        assert!(said(&out).contains(CLASS), "{}", said(&out));
    }
}

#[test]
fn exemption_accepts_own_task_branch_and_base_checkout() {
    let dir = fixture(false);
    git(dir.path(), &["switch", "-q", "main"]);
    let out = ci(dir.path(), "task/TSK-002-exemption", &body("TSK-002"));
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains(CLASS), "{}", said(&out));
}

#[test]
fn exemption_keeps_start_gate_and_spike_scope_with_unready_dependencies() {
    let task = task("TSK-002", "TSK-001", "todo");
    for (record, expected) in [
        (
            task.replace("status: todo", "status: blocked")
                + "\n## Blocker\nWaiting for an operator decision.\n",
            "is blocked",
        ),
        (
            task.replace(
                "depends_on: [TSK-001]",
                "depends_on: [TSK-001]\nawaiting_selection: docs/choice.md",
            ),
            "awaiting selection",
        ),
        (
            task.replace("work_type: fix", "work_type: spike"),
            "a spike lands only findings",
        ),
    ] {
        let dir = fixture_with_record(false, &record);
        for branch in [BRANCH, "task/TSK-002-exemption"] {
            let out = ci(dir.path(), branch, &body("TSK-002"));
            assert!(!out.status.success(), "{}", said(&out));
            assert!(said(&out).contains(expected), "{}", said(&out));
        }
    }
}

#[test]
fn exemption_rejects_record_doc_and_hidden_neighbours() {
    for neighbour in [
        "project-management/tasks/TSK-002.md",
        "docs/guide.md",
        ".other",
    ] {
        let dir = fixture(false);
        let path = dir.path().join(neighbour);
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str("\nAn additional change.\n");
        write(dir.path(), neighbour, &text);
        commit(dir.path(), "docs: add another change");
        for id in ["EPC-001", "TSK-002"] {
            let out = ci(dir.path(), BRANCH, &body(id));
            assert!(!out.status.success(), "{neighbour}: {}", said(&out));
            assert!(!said(&out).contains(CLASS), "{}", said(&out));
        }
    }
}

#[test]
fn exemption_keeps_task_identity_and_body_checks() {
    let dir = fixture(false);
    for (branch, text, expected) in [
        (BRANCH, body("EPC-999"), "work.classification"),
        (BRANCH, body("TSK-999"), "work.stable_planning_anchor"),
        ("task/TSK-001-wrong", body("TSK-002"), "work.classification"),
        (
            BRANCH,
            body("TSK-002").replace("Task: TSK-002", ""),
            "work.classification",
        ),
        (
            BRANCH,
            body("TSK-002").replace("## Testing", "## Other"),
            "git.pr_sections",
        ),
        (BRANCH, body("EPC-001, EPC-002"), "work.classification"),
    ] {
        let out = ci(dir.path(), branch, &text);
        assert!(!out.status.success(), "{}", said(&out));
        assert!(said(&out).contains(expected), "{}", said(&out));
    }
}

#[test]
fn fresh_full_project_exemption_keeps_security_jobs_and_review_requirements() {
    let dir = fixture(true);
    let root = dir.path();
    let out = ci(root, BRANCH, &body("EPC-001"));
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains(CLASS), "{}", said(&out));
    println!("Fresh full-tier exemption journey:\n{}", said(&out));
    let no_review = ci(
        root,
        BRANCH,
        &body("EPC-001").replace("## Reviews", "## Other"),
    );
    assert!(!no_review.status.success(), "{}", said(&no_review));
    assert!(
        said(&no_review).contains("git.pr_sections"),
        "{}",
        said(&no_review)
    );
    let workflow: serde_yaml::Value = serde_yaml::from_slice(
        &std::fs::read(root.join(".github/workflows/codeflow-ci.yml")).unwrap(),
    )
    .unwrap();
    for job in ["secret-scan", "security-review"] {
        let job = &workflow["jobs"][job];
        assert!(
            job["if"].is_null(),
            "security job must not filter exemption PRs"
        );
        assert!(
            job["needs"].is_null(),
            "classification must not gate security jobs"
        );
        assert!(!job["steps"].as_sequence().unwrap().is_empty());
    }
    let scan = &workflow["jobs"]["secret-scan"]["steps"];
    let scan = scan
        .as_sequence()
        .unwrap()
        .iter()
        .find(|step| step["name"] == "gitleaks")
        .unwrap();
    assert!(scan["env"]["TRUSTED_SHA"]
        .as_str()
        .unwrap()
        .contains("pull_request.base.sha"));
    assert!(scan["run"]
        .as_str()
        .unwrap()
        .contains("ignore = blob(\".gitleaksignore\")"));
    let security = serde_yaml::to_string(&workflow["jobs"]["security-review"]).unwrap();
    assert!(security.contains("git.security_review"));
    assert!(security.contains("TRUSTED_SHA"));
}
