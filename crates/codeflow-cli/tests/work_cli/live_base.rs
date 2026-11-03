//! TSK-1047: claim-time bases, fresh admission and advisory review drift.
use super::{cli, fixture, git, record, succeeds, text, with_bare_origin, write};
use std::path::Path;

const A: &str = "integration/a";
const B: &str = "integration/b";
const CHILD: &str = "task/TSK-002-work-tsk-002";

fn commit(root: &Path, name: &str) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", name]);
}

fn lines() -> tempfile::TempDir {
    let dir = fixture();
    write(
        dir.path(),
        "project-management/tasks/TSK-001.md",
        &record("TSK-001", "[]").replace(
            "integration_target: main",
            "integration_target: integration/b",
        ),
    );
    write(
        dir.path(),
        "project-management/tasks/TSK-002.md",
        &record("TSK-002", "[]").replace(
            "integration_target: main",
            "integration_target: integration/a",
        ),
    );
    commit(dir.path(), "docs: declare lines");
    git(dir.path(), &["branch", A]);
    git(dir.path(), &["branch", B]);
    dir
}

// The ledger is deliberately unrelated to the Git admission evidence.
fn ledger(root: &Path, state: &str) {
    let paths = codeflow_core::coord::store::Paths::under(&root.join(".git"));
    match state {
        "absent" => {
            let _ = std::fs::remove_dir_all(paths.dir);
        }
        "present" => {
            codeflow_core::coord::fixture::open_stream(&paths, "WS-001", "fixture").unwrap();
        }
        "damaged" => {
            std::fs::create_dir_all(&paths.dir).unwrap();
            std::fs::write(paths.events, "not JSON\n").unwrap();
        }
        _ => unreachable!(),
    }
}

fn advance(root: &Path, line: &str, path: &str) -> String {
    git(root, &["switch", "-q", line]);
    write(root, path, "needed work\n");
    commit(root, "feat: advance source");
    git(root, &["rev-parse", "HEAD"])
}

#[test]
fn hand_made_main_and_stale_line_bases_are_refused_without_ledger() {
    for state in ["present", "absent", "damaged"] {
        for base in ["main", A] {
            let dir = lines();
            let root = dir.path();
            git(root, &["switch", "-qc", CHILD, base]);
            let needed = advance(root, A, "needed.txt");
            git(root, &["switch", "-q", CHILD]);
            ledger(root, state);
            let out = cli(root, &["work", "start", "TSK-002"], None);
            assert!(!out.status.success(), "{state}/{base}: {}", text(&out));
            assert!(
                text(&out).contains(&needed) && text(&out).contains("codeflow work claim"),
                "{}",
                text(&out)
            );
        }
    }
}

#[test]
fn claims_keep_their_base_after_the_source_advances_in_every_ledger_state() {
    for state in ["present", "absent", "damaged"] {
        let dir = lines();
        let root = dir.path();
        let base = advance(root, A, "needed.txt");
        ledger(root, state);
        succeeds(&cli(root, &["work", "claim", "TSK-002"], None));
        assert_eq!(git(root, &["rev-parse", CHILD]), base);
        let metadata = git(
            root,
            &["cat-file", "-p", &format!("refs/codeflow/claims/{CHILD}")],
        );
        assert!(
            metadata.contains(&base) && metadata.contains(A),
            "{metadata}"
        );
        let tip = advance(root, A, "later.txt");
        git(root, &["switch", "-q", CHILD]);
        let out = cli(root, &["work", "start", "TSK-002"], None);
        succeeds(&out);
        assert!(
            text(&out).contains("advanced") && text(&out).contains(&tip),
            "{}",
            text(&out)
        );
    }
}

#[test]
fn main_tip_and_unclaimed_line_tip_are_valid_bases() {
    for state in ["present", "absent", "damaged"] {
        let dir = fixture();
        let root = dir.path();
        git(root, &["switch", "-qc", "task/TSK-001-manual", "main"]);
        ledger(root, state);
        succeeds(&cli(root, &["work", "start", "TSK-001"], None));
        let dir = lines();
        let root = dir.path();
        advance(root, A, "needed.txt");
        git(root, &["switch", "-qc", CHILD, A]);
        ledger(root, state);
        succeeds(&cli(root, &["work", "start", "TSK-002"], None));
    }
}

fn complete(root: &Path, id: &str) {
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let path = format!("project-management/tasks/{id}.md");
    let content = std::fs::read_to_string(root.join(&path)).unwrap();
    write(root, &path, &format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | fixture\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n", content.replace("status: todo", "status: complete")));
    commit(root, "docs: record predecessor acceptance");
}

fn dependent(root: &Path) {
    git(root, &["switch", "-q", "main"]);
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &record("TSK-002", "[TSK-001]").replace(
            "integration_target: main",
            "integration_target: integration/a",
        ),
    );
    commit(root, "docs: declare dependency");
    git(root, &["switch", "-q", A]);
    git(root, &["merge", "-q", "--ff-only", "main"]);
    git(root, &["switch", "-q", B]);
    git(root, &["merge", "-q", "--ff-only", "main"]);
    write(root, "predecessor.txt", "implementation\n");
    commit(root, "feat: land predecessor");
    complete(root, "TSK-001");
}

#[test]
fn cross_line_claim_and_start_use_the_landed_source_without_the_ledger() {
    for state in ["present", "absent", "damaged"] {
        let dir = lines();
        let root = dir.path();
        dependent(root);
        let base = advance(root, B, "after-predecessor.txt");
        git(root, &["switch", "-q", "main"]);
        ledger(root, state);
        let out = cli(root, &["work", "claim", "TSK-002"], None);
        succeeds(&out);
        assert_eq!(git(root, &["rev-parse", CHILD]), base);
        let tip = advance(root, B, "after-claim.txt");
        git(root, &["switch", "-q", CHILD]);
        let out = cli(root, &["work", "start", "TSK-002"], None);
        succeeds(&out);
        assert!(text(&out).contains(&tip), "{}", text(&out));
    }
}

#[test]
fn two_unmerged_predecessor_sources_require_line_sync() {
    let dir = lines();
    let root = dir.path();
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[]").replace(
            "integration_target: main",
            "integration_target: integration/c",
        ),
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &record("TSK-002", "[TSK-001, TSK-003]").replace(
            "integration_target: main",
            "integration_target: integration/a",
        ),
    );
    commit(root, "docs: declare two sources");
    for line in [A, B] {
        git(root, &["switch", "-q", line]);
        git(root, &["merge", "-q", "--ff-only", "main"]);
    }
    git(root, &["branch", "integration/c", "main"]);
    for (line, id) in [(B, "TSK-001"), ("integration/c", "TSK-003")] {
        git(root, &["switch", "-q", line]);
        let path = format!("project-management/tasks/{id}.md");
        let s = std::fs::read_to_string(root.join(&path)).unwrap();
        write(root, &path, &s.replace("status: todo", "status: complete"));
        commit(root, "feat: land source");
    }
    git(root, &["switch", "-q", "main"]);
    let out = cli(root, &["work", "claim", "TSK-002"], None);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("line sync")
            && text(&out).contains(B)
            && text(&out).contains("integration/c"),
        "{}",
        text(&out)
    );
}

#[test]
fn unreachable_remotes_refuse_claim_and_start_with_last_fetch_time() {
    for level in ["block", "warn"] {
        for verb in ["claim", "start"] {
            let dir = fixture();
            let root = dir.path();
            write(
                root,
                ".codeflow/policy.json",
                &format!("{{\"git\":{{\"work_planning\":\"{level}\"}}}}"),
            );
            commit(root, "chore: configure planning severity");
            let origin = with_bare_origin(root);
            git(root, &["fetch", "-q", "origin"]);
            git(root, &["switch", "-qc", "task/TSK-001-manual"]);
            std::fs::remove_dir_all(origin.path()).unwrap();
            let out = cli(root, &["work", verb, "TSK-001"], None);
            assert!(!out.status.success(), "{}", text(&out));
            assert!(
                text(&out).contains("origin") && text(&out).contains("last fetch"),
                "{}",
                text(&out)
            );
        }
    }
}

#[test]
fn wrong_base_is_advisory_when_work_planning_is_warn() {
    let dir = lines();
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"work_planning":"warn"}}"#,
    );
    commit(root, "chore: warn on planning findings");
    git(root, &["switch", "-q", A]);
    git(root, &["merge", "-q", "--ff-only", "main"]);
    let needed = advance(root, A, "needed.txt");
    git(root, &["switch", "-qc", CHILD, "main"]);
    let out = cli(root, &["work", "start", "TSK-002"], None);
    succeeds(&out);
    assert!(
        text(&out).contains("warning") && text(&out).contains(&needed),
        "{}",
        text(&out)
    );
}

#[test]
fn ci_merge_remedy_keeps_shell_metacharacters_in_the_target_literal() {
    let dir = lines();
    let root = dir.path();
    let target = "integration/a$(id)";
    git(root, &["branch", target, A]);
    advance(root, target, "needed.txt");
    git(root, &["switch", "-qc", CHILD, A]);
    let out = cli(
        root,
        &[
            "ci", "--base", "main", "--head", "HEAD", "--branch", CHILD, "--into", target,
        ],
        None,
    );
    assert!(
        text(&out).contains("git merge -- 'integration/a$(id)'"),
        "{}",
        text(&out)
    );
}

#[test]
fn start_uses_the_planning_severity_from_the_policy_it_just_fetched() {
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"work_planning":"warn"}}"#,
    );
    commit(root, "chore: warn on planning findings");
    let old = git(root, &["rev-parse", "HEAD"]);
    let _origin = with_bare_origin(root);
    git(root, &["fetch", "-q", "origin"]);
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"work_planning":"block"}}"#,
    );
    commit(root, "chore: block planning findings");
    git(root, &["push", "-q", "origin", "main"]);
    git(root, &["update-ref", "refs/remotes/origin/main", &old]);
    git(root, &["switch", "-qc", "task/TSK-001-manual", &old]);
    let out = cli(root, &["work", "start", "TSK-001"], None);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("required base"), "{}", text(&out));
}

#[test]
fn ci_warns_until_the_current_target_tip_is_merged_and_reports_unknown_tips() {
    let dir = lines();
    let root = dir.path();
    git(root, &["switch", "-qc", CHILD, A]);
    write(root, "own.txt", "work\n");
    commit(root, "fix: own work");
    let tip = advance(root, A, "target.txt");
    git(root, &["switch", "-q", CHILD]);
    let args = [
        "ci", "--base", "main", "--head", "HEAD", "--branch", CHILD, "--into", A,
    ];
    let out = cli(root, &args, None);
    assert!(
        text(&out).contains("work.target_tip")
            && text(&out).contains(&tip)
            && text(&out).contains("merge"),
        "{}",
        text(&out)
    );
    assert!(text(&out).contains("added-lines"), "{}", text(&out));
    git(root, &["merge", "-q", "--no-edit", A]);
    let out = cli(root, &args, None);
    assert!(!text(&out).contains("work.target_tip"), "{}", text(&out));
    git(root, &["branch", "-d", A]);
    let out = cli(root, &args, None);
    assert!(text(&out).contains("unverified"), "{}", text(&out));
}

#[cfg(unix)]
#[test]
fn reviewed_pins_older_than_the_target_are_valid_until_the_head_moves() {
    for state in ["present", "absent", "damaged"] {
        let dir = fixture();
        let root = dir.path();
        let bin = tempfile::tempdir().unwrap();
        let branch = "task/TSK-001-work";
        let (_, pin) = super::reviewed_predecessor(root, branch);
        super::review_tool(bin.path(), branch, &pin, true);
        let target_tip = advance(root, "main", "target-later.txt");
        assert_ne!(target_tip, pin);
        ledger(root, state);
        let on = format!("TSK-001@{pin}");
        succeeds(&cli(
            root,
            &["work", "claim", "TSK-002", "--on", &on],
            Some(bin.path()),
        ));
        git(root, &["switch", "-q", CHILD]);
        succeeds(&cli(
            root,
            &["work", "start", "TSK-002", "--on", &on],
            Some(bin.path()),
        ));
        let moved = advance(root, branch, "moved.txt");
        super::review_tool(bin.path(), branch, &moved, true);
        git(root, &["switch", "-q", CHILD]);
        let out = cli(
            root,
            &["work", "start", "TSK-002", "--on", &on],
            Some(bin.path()),
        );
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains(&moved), "{}", text(&out));
        git(root, &["rebase", "--onto", &moved, &pin, CHILD]);
        let new_on = format!("TSK-001@{moved}");
        succeeds(&cli(
            root,
            &["work", "start", "TSK-002", "--on", &new_on],
            Some(bin.path()),
        ));
        let metadata = git(
            root,
            &["cat-file", "-p", &format!("refs/codeflow/claims/{CHILD}")],
        );
        assert!(
            metadata.contains(&pin),
            "the claim's original base changed: {metadata}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_landed_predecessor_no_longer_needs_its_deleted_review_branch() {
    for state in ["present", "absent", "damaged"] {
        let dir = fixture();
        let root = dir.path();
        let bin = tempfile::tempdir().unwrap();
        let branch = "task/TSK-001-work";
        let (_, pin) = super::reviewed_predecessor(root, branch);
        super::review_tool(bin.path(), branch, &pin, true);
        let on = format!("TSK-001@{pin}");
        ledger(root, state);
        succeeds(&cli(
            root,
            &["work", "claim", "TSK-002", "--on", &on],
            Some(bin.path()),
        ));
        git(root, &["merge", "-q", "--ff-only", branch]);
        git(root, &["branch", "-d", branch]);
        git(root, &["switch", "-q", CHILD]);
        succeeds(&cli(root, &["work", "start", "TSK-002"], None));
    }
}

#[test]
fn corrupt_claim_metadata_is_never_treated_as_an_unclaimed_branch() {
    let dir = lines();
    let root = dir.path();
    succeeds(&cli(root, &["work", "claim", "TSK-002"], None));
    git(root, &["switch", "-q", CHILD]);
    git(
        root,
        &[
            "update-ref",
            &format!("refs/codeflow/claims/{CHILD}"),
            "HEAD",
        ],
    );
    let out = cli(root, &["work", "start", "TSK-002"], None);
    assert!(!out.status.success());
    assert!(text(&out).contains("claim base"), "{}", text(&out));
}

#[test]
fn cross_line_claim_requires_the_task_plan_at_the_shared_anchor() {
    let dir = lines();
    let root = dir.path();
    // B branches before A receives the dependent task's planning record.
    git(root, &["switch", "-q", A]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[TSK-001]").replace(
            "integration_target: main",
            "integration_target: integration/a",
        ),
    );
    commit(root, "docs: plan dependent task");
    git(root, &["switch", "-q", B]);
    let path = "project-management/tasks/TSK-001.md";
    let s = std::fs::read_to_string(root.join(path)).unwrap();
    write(root, path, &s.replace("status: todo", "status: complete"));
    commit(root, "feat: land predecessor");
    git(root, &["switch", "-q", A]);
    let out = cli(root, &["work", "claim", "TSK-003"], None);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("planning anchor") && text(&out).contains("line sync"),
        "{}",
        text(&out)
    );
    assert!(git(root, &["branch", "--list", "task/TSK-003-*"]).is_empty());
}

#[test]
fn a_new_claim_replaces_metadata_left_by_a_deleted_claim_branch() {
    let dir = lines();
    let root = dir.path();
    succeeds(&cli(root, &["work", "claim", "TSK-002"], None));
    git(root, &["branch", "-d", CHILD]);
    let base = advance(root, A, "later.txt");
    succeeds(&cli(root, &["work", "claim", "TSK-002"], None));
    git(root, &["switch", "-q", CHILD]);
    succeeds(&cli(root, &["work", "start", "TSK-002"], None));
    let metadata = git(
        root,
        &["cat-file", "-p", &format!("refs/codeflow/claims/{CHILD}")],
    );
    assert!(metadata.contains(&base), "{metadata}");
}

#[test]
fn cross_line_adopter_journey_claims_starts_refuses_main_and_warns_until_merge() {
    let dir = lines();
    let root = dir.path();
    dependent(root);
    let base = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", A]);
    let out = cli(root, &["work", "claim", "TSK-002"], None);
    succeeds(&out);
    println!("journey claim: {}", text(&out));
    assert_eq!(git(root, &["rev-parse", CHILD]), base);
    git(root, &["switch", "-q", CHILD]);
    let out = cli(root, &["work", "start", "TSK-002"], None);
    succeeds(&out);
    println!("journey start: {}", text(&out));
    git(root, &["switch", "-qc", "task/TSK-002-wrong", "main"]);
    let out = cli(root, &["work", "start", "TSK-002"], None);
    assert!(!out.status.success());
    assert!(text(&out).contains("required base"), "{}", text(&out));
    println!("journey wrong base: {}", text(&out));
    let tip = advance(root, A, "target-advanced.txt");
    git(root, &["switch", "-q", CHILD]);
    write(root, "dependent.txt", "dependent work\n");
    commit(root, "fix: build dependent work");
    let args = [
        "ci", "--base", B, "--head", "HEAD", "--branch", CHILD, "--into", A,
    ];
    let out = cli(root, &args, None);
    assert!(
        text(&out).contains("work.target_tip") && text(&out).contains(&tip),
        "{}",
        text(&out)
    );
    println!(
        "journey before merge: target {tip} warning observed (CI exit {:?})",
        out.status.code()
    );
    git(root, &["merge", "-q", "--no-edit", A]);
    let out = cli(root, &args, None);
    assert!(!text(&out).contains("work.target_tip"), "{}", text(&out));
    println!(
        "journey after merge: target warning cleared (CI exit {:?})",
        out.status.code()
    );
}

#[test]
fn an_unrelated_source_advance_does_not_restore_a_reopened_predecessor() {
    let dir = lines();
    let root = dir.path();
    dependent(root);
    git(root, &["switch", "-q", A]);
    git(root, &["merge", "-q", "--ff-only", B]);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &record("TSK-001", "[]").replace(
            "integration_target: main",
            "integration_target: integration/b",
        ),
    );
    commit(root, "docs: reopen predecessor");
    advance(root, B, "unrelated.txt");
    git(root, &["switch", "-q", A]);
    let out = cli(root, &["work", "claim", "TSK-002"], None);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(git(root, &["branch", "--list", CHILD]).is_empty());
}
