//! Imported completions keep their home binding (TSK-339, issues 181/218/240).
use super::*;

const LINE: &str = "integration/EPC-001-outcome";
const RECEIVER: &str = "task/TSK-002-receiver";

fn merge(root: &Path, branch: &str) {
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore: land reviewed work",
            branch,
        ],
    );
}

fn adopt_receiver(root: &Path, direct: &str) {
    git(root, &["switch", "-c", "plan/receiver-adoption"]);
    write(root, "project-management/epics/EPC-001.md", &EPIC.replacen("---\n", &format!("---\nline_adoptions:\n  - commit: {direct}\n    reason: Reviewed receiver change\n    review: https://example.test/receiver-review\n"), 1));
    commit(root, "docs: adopt reviewed receiving work");
    git(root, &["switch", LINE]);
    merge(root, "plan/receiver-adoption");
}

fn imported_twice() -> tempfile::TempDir {
    let dir = repo(
        &[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)],
        ",\"test_gate_on_push\":\"block\"",
    );
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    commit(root, "docs: target the receiving line");
    git(root, &["switch", "-c", LINE]);
    write(root, "src/receiver.rs", "pub fn reviewed_receiver() {}\n");
    let direct = commit(root, "feat: reviewed receiving work");
    adopt_receiver(root, &direct);
    git(root, &["switch", "-c", RECEIVER]);
    let reviewed = code_change(root, BRANCH, "pub fn source() {}\n");
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&reviewed));
    git(root, &["switch", "main"]);
    merge(root, BRANCH);
    let epic_review = head(root);
    write(
        root,
        "project-management/epics/EPC-002.md",
        &format!(
            "{}\n## Closeout\n\n{}",
            EPIC.replace("EPC-001", "EPC-002")
                .replace("status: planning", "status: complete"),
            block(
                &epic_review,
                &["AC-1: verified | source journey"],
                "verified | source journey",
                "none: fixture"
            )
        ),
    );
    commit(root, "docs: complete the source epic");
    git(root, &["switch", RECEIVER]);
    merge(root, "main");
    write(root, "src/lib.rs", "pub fn receiver_after_sync() {}\n");
    commit(root, "feat: receiver edits the shared file");
    git(root, &["switch", "main"]);
    write(root, ".codeflow/policy.json", "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"block\",\"secret_scan\":\"warn\"}}\n");
    commit(root, "chore: update target policy");
    write(
        root,
        &record_path("TSK-003"),
        &task("TSK-003", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    commit(root, "docs: plan target work");
    git(root, &["switch", RECEIVER]);
    merge(root, "main");
    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    dir
}

fn check(root: &Path, base: &str, branch: Option<&str>, body: bool) -> (i32, String) {
    if body {
        return ci_on(root, base, RECEIVER, "Task: TSK-002");
    }
    let mut command = codeflow();
    command.args(["ci", "--base", base, "--head", "HEAD"]);
    if let Some(branch) = branch {
        command.args(["--branch", branch]);
    }
    let out = command.current_dir(root).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn target_imported_twice_keeps_its_completion_and_added_records() {
    let dir = imported_twice();
    let root = dir.path();
    let mut failures = Vec::new();
    for base in [LINE.to_string(), git_out(root, &["rev-parse", LINE])] {
        for branch in [None, Some(RECEIVER), Some(LINE)] {
            let result = check(root, &base, branch, false);
            if result.0 != 0 {
                failures.push(result.1);
            }
        }
        let result = check(root, &base, Some(RECEIVER), true);
        if result.0 != 0 {
            failures.push(result.1);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn imported_record_changes_are_still_receiving_work() {
    for change in ["title", "acceptance", "home"] {
        let dir = imported_twice();
        let root = dir.path();
        let path = record_path("TSK-001");
        let original = std::fs::read_to_string(root.join(&path)).unwrap();
        let changed = match change {
            "title" => original.replace("work TSK-001", "changed TSK-001"),
            "acceptance" => original.replace("verified | cargo test", "verified | other evidence"),
            _ => original.replace(
                "integration_target: main",
                "integration_target: integration/EPC-009-forged",
            ),
        };
        assert_ne!(changed, original);
        write(root, &path, &changed);
        commit(root, "docs: change an imported record");
        assert_blocks(
            &check(root, LINE, Some(RECEIVER), false),
            change,
            &["work.acceptance_binding"],
        );
    }
}

#[test]
fn receiving_completion_still_requires_review_of_later_product_work() {
    let dir = imported_twice();
    let root = dir.path();
    let reviewed = head(root);
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "complete", OWN_JOURNEY, &valid_block(&reviewed)).replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    commit(root, "docs: complete the receiver");
    assert_passes(
        &check(root, LINE, Some(RECEIVER), true),
        "receiver completes after imports",
    );
    write(root, "src/lib.rs", "pub fn unreviewed() {}\n");
    commit(root, "feat: change after receiver review");
    let result = check(root, LINE, Some(RECEIVER), false);
    assert_blocks(
        &result,
        "receiver owns later work",
        &["TSK-002", "work.acceptance_binding"],
    );
    assert!(
        !result
            .1
            .lines()
            .any(|line| line.contains("(block)") && line.contains("TSK-001")),
        "{}",
        result.1
    );
}

#[test]
fn imported_history_overlays_refuse() {
    let dir = imported_twice();
    let root = dir.path();
    let tip = head(root);
    git(root, &["replace", &tip, LINE]);
    let result = check(root, LINE, Some(RECEIVER), false);
    assert_ne!(result.0, 0, "{}", result.1);
    assert!(
        result.1.contains("overlays") || result.1.contains("replace"),
        "{}",
        result.1
    );
}

#[allow(clippy::too_many_lines)] // Keep the source and receiving history together for the matrix.
fn imported_from_home(home: &str, reopen: bool, stack: bool) -> tempfile::TempDir {
    let dir = repo(
        &[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)],
        ",\"test_gate_on_push\":\"block\"",
    );
    let root = dir.path();
    let source_task = |status: &str, criteria: &str, closeout: &str| {
        let text = task("TSK-001", status, criteria, closeout).replace(
            "integration_target: main",
            &format!("integration_target: {home}"),
        );
        if home == "main" {
            text
        } else {
            text.replace("EPC-001", "EPC-003")
        }
    };
    write(
        root,
        &record_path("TSK-001"),
        &source_task("todo", OWN_JOURNEY, "Pending.\n"),
    );
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    if home != "main" {
        write(
            root,
            "project-management/epics/EPC-003.md",
            &EPIC.replace("EPC-001", "EPC-003"),
        );
    }
    commit(root, "docs: plan homes");
    if home != "main" {
        git(root, &["branch", home]);
    }
    git(root, &["switch", "-c", LINE]);
    write(root, "src/receiver.rs", "pub fn previously_reviewed() {}\n");
    let direct = commit(root, "feat: receiving line work");
    adopt_receiver(root, &direct);
    git(root, &["switch", "-c", RECEIVER]);
    git(root, &["switch", "-c", BRANCH, home]);
    write(root, "src/lib.rs", "pub fn source() {}\n");
    let mut reviewed = commit(root, "feat: source work");
    let mut criteria = OWN_JOURNEY.to_string();
    let mut archive = String::new();
    if reopen {
        let old = valid_block(&reviewed);
        write(
            root,
            &record_path("TSK-001"),
            &source_task("complete", &criteria, &old),
        );
        commit(root, "docs: complete first attempt");
        archive = old.replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: missing criterion\n",
        );
        criteria.push_str("- AC-3 The new requirement works.\n");
        write(
            root,
            &record_path("TSK-001"),
            &source_task("todo", &criteria, &archive),
        );
        commit(root, "docs: reopen before landing");
        write(root, "src/lib.rs", "pub fn revised_source() {}\n");
        reviewed = commit(root, "feat: meet the new criterion");
    }
    if stack {
        git(root, &["switch", "-c", "plan/home-advance", home]);
        write(root, "docs/source-note.md", "Home planning.\n");
        commit(root, "docs: advance home planning");
        git(root, &["switch", home]);
        merge(root, "plan/home-advance");
        git(root, &["switch", BRANCH]);
        merge(root, home);
    }
    let results = if reopen {
        vec![
            "AC-1: verified | unit",
            "AC-2: verified | journey",
            "AC-3: verified | unit",
        ]
    } else {
        vec!["AC-1: verified | unit", "AC-2: verified | journey"]
    };
    let closeout = format!(
        "{archive}{}",
        block(&reviewed, &results, "verified | journey", "none: fixture")
    );
    write(
        root,
        &record_path("TSK-001"),
        &source_task("complete", &criteria, &closeout),
    );
    commit(root, "docs: complete source work");
    git(root, &["switch", home]);
    merge(root, BRANCH);
    git(root, &["switch", RECEIVER]);
    merge(root, home);
    write(root, "src/lib.rs", "pub fn later_receiver_work() {}\n");
    commit(root, "feat: receiving work after import");
    dir
}

#[test]
fn main_and_sibling_imports_keep_home_stacking_and_reopened_criteria() {
    for home in ["main", "integration/EPC-003-source"] {
        for (reopen, stack) in [(false, false), (false, true), (true, false)] {
            let dir = imported_from_home(home, reopen, stack);
            let root = dir.path();
            for base in [LINE.to_string(), git_out(root, &["rev-parse", LINE])] {
                assert_passes(
                    &check(root, &base, Some(RECEIVER), false),
                    &format!("{home}: reopen={reopen}, stack={stack}"),
                );
                assert_passes(
                    &check(root, &base, Some(LINE), false),
                    "receiving line context",
                );
                assert_passes(
                    &check(root, &base, Some(RECEIVER), true),
                    "receiving task PR",
                );
            }
            publish_sources(root, home);
            assert_passes(
                &pre_push(root, RECEIVER, &"0".repeat(40)),
                "home import through pre-push",
            );
            complete_receiver_and_land(root);
        }
    }
}

#[test]
fn imported_redaction_keeps_the_original_source_binding() {
    let dir = imported_from_home("main", false, false);
    let root = dir.path();
    git(root, &["switch", "-c", "plan/source-redaction", "main"]);
    let old = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    let replacement = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        replacement.path(),
        old.replace("verified | unit", "verified | source check"),
    )
    .unwrap();
    let out = codeflow()
        .current_dir(root)
        .args([
            "task",
            "redact",
            "TSK-001",
            "--replacement",
            replacement.path().to_str().unwrap(),
            "--review",
            "https://example.test/redaction-review",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    commit(root, "docs: redact source evidence");
    git(root, &["switch", "main"]);
    merge(root, "plan/source-redaction");
    git(root, &["switch", RECEIVER]);
    merge(root, "main");
    assert_passes(
        &check(root, LINE, Some(RECEIVER), true),
        "imported source redaction",
    );
}

#[test]
fn source_with_an_unreviewed_landing_resolution_refuses() {
    let dir = imported_from_home("main", false, false);
    let root = dir.path();
    // Rebuild the home landing with an extra product edit, then import it
    // on a fresh receiving task branch so the record still matches exactly.
    git(root, &["switch", "main"]);
    let original = git_out(root, &["rev-parse", "main^1"]);
    git(root, &["reset", "--hard", &original]);
    git(root, &["merge", "--no-ff", "--no-commit", BRANCH]);
    write(root, "src/resolved.rs", "pub fn never_reviewed() {}\n");
    commit(root, "chore: land with extra resolution");
    git(root, &["switch", "-C", RECEIVER, LINE]);
    merge(root, "main");
    let result = check(root, LINE, Some(RECEIVER), false);
    assert_blocks(
        &result,
        "source resolution",
        &[
            "target history",
            "unreviewed merge resolution",
            "repair or reopen",
        ],
    );
}

#[test]
fn a_protected_target_reopen_and_criteria_amendment_are_not_receiver_changes() {
    let dir = imported_twice();
    let root = dir.path();
    git(root, &["switch", "main"]);
    let path = record_path("TSK-001");
    let old = std::fs::read_to_string(root.join(&path)).unwrap();
    write(
        root,
        &path,
        &old.replace("status: complete", "status: todo").replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: target repair\n",
        ),
    );
    commit(root, "docs: reopen on target");
    let changed = std::fs::read_to_string(root.join(&path))
        .unwrap()
        .replace("the system shall work.", "the revised system shall work.");
    write(root, &path, &changed);
    commit(root, "docs: amend criteria on target");
    git(root, &["switch", RECEIVER]);
    merge(root, "main");
    assert_passes(
        &check(root, LINE, Some(RECEIVER), true),
        "target reopening and amendment",
    );
}

fn publish_sources(root: &Path, home: &str) {
    let remote = root.join(".git/destination.git");
    git(
        root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "origin", "main", LINE]);
    if home != "main" {
        git(root, &["push", "origin", home]);
    }
}

fn pre_push(root: &Path, branch: &str, old: &str) -> (i32, String) {
    pre_push_remote(root, branch, old, "origin")
}

fn pre_push_remote(root: &Path, branch: &str, old: &str, name: &str) -> (i32, String) {
    use std::io::Write;
    use std::process::Stdio;
    let remote = root.join(".git/destination.git");
    let mut child = codeflow()
        .args(["git-hook", "pre-push", name, remote.to_str().unwrap()])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "refs/heads/{branch} {} refs/heads/{branch} {old}",
        head(root)
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    let result = (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    );
    eprintln!("pre-push fixture {branch}:\n{}", result.1);
    assert!(
        result.1.contains("`codeflow ci` judges"),
        "pre-push must execute CI: {}",
        result.1
    );
    result
}

fn complete_receiver_and_land(root: &Path) {
    let reviewed = head(root);
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "complete", OWN_JOURNEY, &valid_block(&reviewed)).replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    commit(root, "docs: complete receiving task");
    assert_passes(
        &check(root, LINE, Some(RECEIVER), true),
        "completed receiver PR",
    );
    git(root, &["switch", LINE]);
    let old = head(root);
    merge(root, RECEIVER);
    assert_passes(
        &pre_push(root, LINE, &old),
        "receiving task lands on the line",
    );
}

#[test]
fn the_real_pre_push_entry_accepts_two_target_imports_and_receiver_landing() {
    let dir = imported_twice();
    let root = dir.path();
    publish_sources(root, "main");
    assert_passes(
        &pre_push(root, RECEIVER, &"0".repeat(40)),
        "two imports through pre-push",
    );
    complete_receiver_and_land(root);
    git(root, &["switch", RECEIVER]);
    write(root, "src/lib.rs", "pub fn unreviewed_receiver() {}\n");
    commit(root, "feat: edit after review");
    assert_blocks(
        &pre_push(root, RECEIVER, &"0".repeat(40)),
        "pre-push retains receiver binding",
        &["TSK-002", "work.acceptance_binding"],
    );
}

#[test]
fn an_unavailable_import_home_never_becomes_receiving_authority() {
    let home = "integration/EPC-003-source";
    let dir = imported_from_home(home, false, false);
    let root = dir.path();
    git(root, &["branch", "-D", home]);
    for branch in [RECEIVER, LINE] {
        assert_blocks(
            &check(root, LINE, Some(branch), false),
            "missing home",
            &["home", "unavailable", "fetch"],
        );
    }
}

#[test]
fn an_import_preserves_an_already_reviewed_different_shared_file() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)], "");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    commit(root, "docs: plan receiving line");
    let source_review = code_change(root, BRANCH, "pub fn shared_source() {}\n");
    git(root, &["branch", LINE]);
    git(root, &["switch", "-c", RECEIVER]);
    write(root, "src/lib.rs", "pub fn shared_receiver() {}\n");
    let receiver_review = commit(root, "feat: receiver has a different shared file");
    write(
        root,
        &record_path("TSK-002"),
        &task(
            "TSK-002",
            "complete",
            OWN_JOURNEY,
            &valid_block(&receiver_review),
        )
        .replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    commit(root, "docs: complete receiving work");
    git(root, &["switch", LINE]);
    merge(root, RECEIVER);
    let base = head(root);
    git(root, &["switch", BRANCH]);
    complete(root, "TSK-001", OWN_JOURNEY, &valid_block(&source_review));
    git(root, &["switch", "main"]);
    merge(root, BRANCH);
    git(root, &["switch", LINE]);
    merge(root, "main");
    assert_passes(
        &check(root, &base, Some(LINE), false),
        "different reviewed shared file",
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        "pub fn shared_receiver() {}\n"
    );
}

#[test]
fn a_source_task_edit_after_completion_cannot_hide_at_its_introduction() {
    let dir = imported_from_home("main", false, false);
    let root = dir.path();
    let original = git_out(root, &["rev-parse", "main^1"]);
    git(root, &["switch", BRANCH]);
    write(
        root,
        "src/after-review.rs",
        "pub fn unreviewed_source() {}\n",
    );
    commit(root, "feat: source change after completion");
    git(root, &["switch", "main"]);
    git(root, &["reset", "--hard", &original]);
    merge(root, BRANCH);
    git(root, &["switch", "-C", RECEIVER, LINE]);
    merge(root, "main");
    assert_blocks(
        &check(root, LINE, Some(RECEIVER), false),
        "unreviewed source tail",
        &["target history", "after-review.rs", "repair or reopen"],
    );
}

#[test]
fn independently_introduced_receiving_criteria_still_compete() {
    let dir = imported_from_home("main", true, false);
    let root = dir.path();
    git(root, &["switch", LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "todo",
            &format!("{OWN_JOURNEY}- AC-4 The receiving requirement works.\n"),
            "Pending.\n",
        ),
    );
    commit(root, "docs: independently amend receiving criteria");
    git(root, &["switch", RECEIVER]);
    let result = check(root, LINE, Some(RECEIVER), false);
    assert_ne!(result.0, 0, "{}", result.1);
    assert!(result.1.contains("criteria"), "{}", result.1);
}

fn refuses_unpublished_home(case: &str) {
    let home = if case == "sibling" {
        "integration/EPC-003-source"
    } else {
        "main"
    };
    let dir = imported_from_home(home, false, false);
    let root = dir.path();
    publish_sources(root, "main");
    if case == "landing" {
        let planned = git_out(root, &["merge-base", LINE, "main"]);
        git(
            root,
            &[
                "--git-dir",
                ".git/destination.git",
                "update-ref",
                "refs/heads/main",
                &planned,
            ],
        );
        git(root, &["update-ref", "refs/remotes/origin/main", &planned]);
    } else if case != "sibling" {
        git(root, &["switch", "main"]);
        let old = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
        let changed = if case == "criteria" {
            old.replace("the system shall work", "the system may work")
        } else {
            old.replace("verified | unit", "verified | local evidence")
        };
        assert_ne!(old, changed);
        write(root, &record_path("TSK-001"), &changed);
        commit(root, "docs: change only the local home record");
        git(root, &["switch", RECEIVER]);
        merge(root, "main");
    }
    let ci = check(root, LINE, Some(RECEIVER), true);
    let push = pre_push(root, RECEIVER, &"0".repeat(40));
    for (name, result) in [("ci", ci), ("pre-push", push)] {
        assert_blocks(&result, &format!("{case}: {name}"), &[home, "fetch"]);
    }
}

#[test]
fn unpublished_main_landing_is_not_import_authority() {
    refuses_unpublished_home("landing");
}

#[test]
fn local_only_sibling_is_not_import_authority() {
    refuses_unpublished_home("sibling");
}

#[test]
fn unpublished_home_evidence_edit_is_not_import_authority() {
    refuses_unpublished_home("evidence");
}

#[test]
fn unpublished_home_criteria_edit_is_not_import_authority() {
    refuses_unpublished_home("criteria");
}

fn unowned_line_merge(resolution: bool) {
    let dir = repo(
        &[("TSK-002", OWN_JOURNEY)],
        ",\"test_gate_on_push\":\"block\"",
    );
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    commit(root, "docs: plan receiver work");
    git(root, &["branch", LINE]);
    write(root, "docs/source-note.md", "Published source planning.\n");
    commit(root, "docs: advance source");
    publish_sources(root, "main");
    if !resolution {
        git(root, &["switch", "-c", "unreviewed-side"]);
        write(root, "src/unowned.rs", "pub fn unreviewed() {}\n");
        commit(root, "feat: unreviewed side work");
    }
    git(root, &["switch", LINE]);
    let base = head(root);
    if resolution {
        git(root, &["merge", "--no-ff", "--no-commit", "main"]);
        write(root, "src/unowned.rs", "pub fn unreviewed() {}\n");
        commit(root, "chore: resolve the sync");
    } else {
        merge(root, "unreviewed-side");
    }
    let ci = check(root, &base, Some(LINE), false);
    let push = pre_push(root, LINE, &base);
    for (name, result) in [("ci", ci), ("pre-push", push)] {
        assert_blocks(&result, name, &["receiving line", "src/unowned.rs"]);
    }
}

#[test]
fn a_sync_resolution_without_a_completion_stays_receiver_work() {
    unowned_line_merge(true);
}

#[test]
fn a_non_target_product_merge_without_a_completion_stays_receiver_work() {
    unowned_line_merge(false);
}

#[test]
fn deleted_home_imports_are_proven_at_the_published_default_landing() {
    let home = "integration/EPC-003-source";
    for (reopen, stack, keep_local) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let dir = imported_from_home(home, reopen, stack);
        let root = dir.path();
        git(root, &["switch", home]);
        let reviewed = head(root);
        git(root, &["switch", "-c", "plan/close-source"]);
        write(
            root,
            "project-management/epics/EPC-003.md",
            &format!(
                "{}\n## Closeout\n\n{}",
                EPIC.replace("EPC-001", "EPC-003")
                    .replace("status: planning", "status: complete"),
                block(
                    &reviewed,
                    &["AC-1: verified | source journey"],
                    "verified | source journey",
                    "none: fixture"
                )
            ),
        );
        commit(root, "docs: complete the source epic");
        git(root, &["switch", home]);
        merge(root, "plan/close-source");
        git(root, &["switch", "main"]);
        merge(root, home);
        publish_sources(root, home);
        git(root, &["push", "origin", "--delete", home]);
        if !keep_local {
            git(root, &["branch", "-D", home]);
        }
        git(root, &["switch", "-C", RECEIVER, LINE]);
        merge(root, "main");
        write(root, "src/lib.rs", "pub fn receiving_work() {}\n");
        commit(root, "feat: receiving work after the retired home sync");
        let mut failures = Vec::new();
        for branch in [None, Some(RECEIVER), Some(LINE)] {
            let result = check(root, LINE, branch, false);
            if result.0 != 0 {
                failures.push(result.1);
            }
        }
        for result in [
            check(root, LINE, Some(RECEIVER), true),
            pre_push(root, RECEIVER, &"0".repeat(40)),
        ] {
            if result.0 != 0 {
                failures.push(result.1);
            }
        }
        git(root, &["switch", LINE]);
        let old = head(root);
        merge(root, "main");
        for result in [
            check(root, &old, None, false),
            check(root, &old, Some(LINE), false),
            ci_on(root, &old, LINE, "Task: EPC-001"),
            pre_push(root, LINE, &old),
        ] {
            if result.0 != 0 {
                failures.push(result.1);
            }
        }
        assert!(
            failures.is_empty(),
            "reopen={reopen}, stack={stack}, keep_local={keep_local}: {}",
            failures.join("\n")
        );
        git(root, &["switch", RECEIVER]);
        complete_receiver_and_land(root);
    }
}

#[test]
fn imports_use_the_configured_upstream_remote() {
    let home = "integration/EPC-003-source";
    let dir = imported_from_home(home, false, false);
    let root = dir.path();
    publish_sources(root, home);
    for branch in ["main", LINE, home] {
        git(
            root,
            &[
                "branch",
                "--set-upstream-to",
                &format!("origin/{branch}"),
                branch,
            ],
        );
    }
    git(root, &["remote", "rename", "origin", "upstream"]);
    assert_passes(
        &check(root, LINE, Some(RECEIVER), true),
        "configured upstream import",
    );
    assert_passes(
        &pre_push_remote(root, RECEIVER, &"0".repeat(40), "upstream"),
        "configured upstream pre-push",
    );
}

#[test]
fn import_authority_refusals_name_the_import_rule() {
    let home = "integration/EPC-003-source";
    let dir = imported_from_home(home, false, false);
    let root = dir.path();
    publish_sources(root, "main");
    git(root, &["branch", "-D", home]);
    for result in [
        check(root, LINE, Some(RECEIVER), true),
        pre_push(root, RECEIVER, &"0".repeat(40)),
    ] {
        assert_blocks(
            &result,
            "missing import authority",
            &["work.import_authority", home, "unavailable", "fetch"],
        );
        assert!(
            !result
                .1
                .contains("cannot read the range to check acceptance"),
            "{}",
            result.1
        );
        assert!(!result.1.contains("a task PR may add only"), "{}", result.1);
    }
}

#[test]
fn unconfigured_remote_cannot_redirect_published_main_to_a_pushed_branch() {
    let mut failures = Vec::new();
    for remote in ["origin", "upstream"] {
        let dir = imported_from_home("main", false, false);
        let root = dir.path();
        let planned = git_out(root, &["merge-base", LINE, "main"]);
        git(root, &["switch", "main"]);
        git(root, &["reset", "--hard", &planned]);
        publish_sources(root, "main");
        if remote != "origin" {
            git(root, &["remote", "rename", "origin", remote]);
        }
        write(root, "src/forged.rs", "pub fn never_reviewed() {}\n");
        let reviewed = commit(root, "feat: forged direct work");
        write(
            root,
            &record_path("TSK-001"),
            &task("TSK-001", "complete", OWN_JOURNEY, &valid_block(&reviewed)),
        );
        commit(root, "docs: forged completion");
        git(root, &["push", remote, "main:refs/heads/docs/x/main"]);
        git(root, &["fetch", remote]);
        git(root, &["switch", "-C", RECEIVER, LINE]);
        merge(root, "main");
        git(root, &["config", "branch.main.remote", remote]);
        assert_blocks(
            &check(root, LINE, Some(RECEIVER), true),
            "published main control",
            &["main", "unpublished local history"],
        );
        git(
            root,
            &["config", "branch.main.remote", &format!("{remote}/docs/x")],
        );
        let reason = if remote == "origin" {
            "unpublished local history"
        } else {
            "unavailable"
        };
        for result in [
            check(root, LINE, Some(RECEIVER), true),
            pre_push_remote(root, RECEIVER, &"0".repeat(40), remote),
        ] {
            if result.0 == 0 || !result.1.contains(reason) || !result.1.contains("main") {
                failures.push(format!("{remote}: {}", result.1));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn retired_home_rejects_a_non_line_source_chain() {
    let home = "integration/EPC-003-source";
    let mut failures = Vec::new();
    for case in ["completion", "evidence", "criteria"] {
        let dir = imported_from_home(home, false, false);
        let root = dir.path();
        git(root, &["switch", "main"]);
        if case != "completion" {
            merge(root, home);
        }
        git(root, &["switch", "-c", "docs/side"]);
        if case == "completion" {
            write(root, "src/forged.rs", "pub fn never_reviewed() {}\n");
            let reviewed = commit(root, "feat: direct work on a side branch");
            write(
                root,
                &record_path("TSK-001"),
                &task("TSK-001", "complete", OWN_JOURNEY, &valid_block(&reviewed))
                    .replace(
                        "integration_target: main",
                        &format!("integration_target: {home}"),
                    )
                    .replace("EPC-001", "EPC-003"),
            );
        } else {
            let old = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
            let changed = if case == "evidence" {
                old.replace("verified | unit", "verified | rewritten evidence")
            } else {
                old.replace("the system shall work", "the system may work")
            };
            assert_ne!(old, changed);
            write(root, &record_path("TSK-001"), &changed);
        }
        commit(root, "docs: change the source record on a side branch");
        git(root, &["switch", "main"]);
        merge(root, "docs/side");
        publish_sources(root, home);
        git(root, &["push", "origin", "--delete", home]);
        git(root, &["branch", "-D", home]);
        git(root, &["switch", "-C", RECEIVER, LINE]);
        merge(root, "main");
        for result in [
            check(root, LINE, Some(RECEIVER), true),
            pre_push(root, RECEIVER, &"0".repeat(40)),
        ] {
            if result.0 == 0
                || !result.1.contains(home)
                || !result.1.contains("directly on the line")
            {
                failures.push(format!("{case}: {}", result.1));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// TSK-1004: two lines that share one planning merge.
const LINE_A: &str = "integration/EPC-001-first";
const LINE_B: &str = "integration/EPC-002-second";

/// The planning branch holding a task for each line and the second epic.
fn plan_two_lines(root: &Path) {
    git(root, &["switch", "-c", "plan/lines", "main"]);
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    for (id, epic, line) in [
        ("TSK-001", "EPC-001", LINE_A),
        ("TSK-002", "EPC-002", LINE_B),
    ] {
        write(
            root,
            &record_path(id),
            &task(id, "todo", OWN_JOURNEY, "Pending.\n")
                .replace(
                    "integration_target: main",
                    &format!("integration_target: {line}"),
                )
                .replace("epic_id: EPC-001", &format!("epic_id: {epic}")),
        );
    }
    commit(root, "docs: plan both lines");
    git(root, &["switch", "main"]);
}

/// Advance `line` by merging one branch that changes no record.
fn advance(root: &Path, line: &str, file: &str) {
    let branch = format!("docs/advance-{}", file.replace(['/', '.'], "-"));
    git(root, &["switch", "-c", &branch, line]);
    write(root, file, "Line notes.\n");
    commit(root, "docs: advance the line");
    git(root, &["switch", line]);
    merge(root, &branch);
}

/// Seed the id registry and publish it with `main` only, so both lines
/// are first pushes.
fn publish_main(root: &Path) {
    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let remote = root.join(".git/destination.git");
    git(
        root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "origin", "main", "codeflow/registry"]);
    git(root, &["fetch", "origin"]);
}

/// Both lines hold the planning merge P (`shared`) or each line has its
/// own planning merge (the control), then each line advances without
/// changing a planning record.
fn two_lines(shared: bool) -> tempfile::TempDir {
    let dir = repo(&[], ",\"test_gate_on_push\":\"block\"");
    let root = dir.path();
    plan_two_lines(root);
    if shared {
        git(root, &["switch", "-c", LINE_A]);
        merge(root, "plan/lines");
        git(root, &["branch", LINE_B]);
    } else {
        for line in [LINE_A, LINE_B] {
            git(root, &["switch", "-c", line, "main"]);
            git(
                root,
                &[
                    "merge",
                    "--no-ff",
                    "-m",
                    &format!("chore: plan {line}"),
                    "plan/lines",
                ],
            );
        }
    }
    advance(root, LINE_A, "docs/first.md");
    advance(root, LINE_B, "docs/second.md");
    publish_main(root);
    dir
}

/// Push each line first in turn, then the other, through the real
/// pre-push entry; returns every result that was refused.
fn push_both(shared: bool) -> Vec<String> {
    let mut refused = Vec::new();
    for order in [[LINE_A, LINE_B], [LINE_B, LINE_A]] {
        let dir = two_lines(shared);
        let root = dir.path();
        for line in order {
            git(root, &["switch", line]);
            let result = pre_push(root, line, &"0".repeat(40));
            if result.0 == 0 {
                git(root, &["push", "--no-verify", "origin", line]);
            } else {
                refused.push(format!("{order:?} {line}: {}", result.1));
            }
        }
    }
    refused
}

#[test]
fn lines_that_share_one_planning_merge_push_in_either_order() {
    let refused = push_both(true);
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

#[test]
fn lines_with_their_own_planning_merges_push_in_either_order() {
    let refused = push_both(false);
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// Line B lands a home-only version of TSK-002 (or a TSK-003 introduced
/// already complete) at a merge M, line A
/// branches from M, and both lines advance with that version unchanged.
fn home_only_version_shared_by_ancestry(case: &str) -> tempfile::TempDir {
    let dir = repo(&[], ",\"test_gate_on_push\":\"block\"");
    let root = dir.path();
    plan_two_lines(root);
    git(root, &["switch", "-c", LINE_B]);
    merge(root, "plan/lines");
    let record = |status: &str, criteria: &str, closeout: &str| {
        let id = if case == "born complete" {
            "TSK-003"
        } else {
            "TSK-002"
        };
        task(id, status, criteria, closeout)
            .replace(
                "integration_target: main",
                &format!("integration_target: {LINE_B}"),
            )
            .replace("epic_id: EPC-001", "epic_id: EPC-002")
    };
    git(root, &["switch", "-c", "task/TSK-002-home"]);
    match case {
        "completion" => {
            write(root, "src/lib.rs", "pub fn home_work() {}\n");
            let reviewed = commit(root, "feat: home work");
            write(
                root,
                &record_path("TSK-002"),
                &record("complete", OWN_JOURNEY, &valid_block(&reviewed)),
            );
        }
        "born complete" => {
            // A record introduced already complete is no planning version.
            write(root, "src/lib.rs", "pub fn home_work() {}\n");
            let reviewed = commit(root, "feat: home work");
            write(
                root,
                &record_path("TSK-003"),
                &record("complete", OWN_JOURNEY, &valid_block(&reviewed)),
            );
        }
        "criteria" => {
            write(
                root,
                &record_path("TSK-002"),
                &record(
                    "todo",
                    &OWN_JOURNEY.replace("the system shall work", "the system may work"),
                    "Pending.\n",
                ),
            );
        }
        _ => unreachable!("{case}"),
    }
    commit(root, "docs: change the home record");
    git(root, &["switch", LINE_B]);
    merge(root, "task/TSK-002-home");
    git(root, &["branch", LINE_A]);
    advance(root, LINE_A, "docs/first.md");
    advance(root, LINE_B, "docs/second.md");
    publish_main(root);
    git(root, &["switch", LINE_A]);
    dir
}

#[test]
fn shared_ancestry_never_exempts_an_unpublished_home_only_version() {
    let mut failures = Vec::new();
    for (case, id) in [
        ("completion", "TSK-002"),
        ("criteria", "TSK-002"),
        ("born complete", "TSK-003"),
    ] {
        let dir = home_only_version_shared_by_ancestry(case);
        let root = dir.path();
        let result = pre_push(root, LINE_A, &"0".repeat(40));
        let needles = [
            "work.import_authority",
            id,
            LINE_B,
            "exists only on unpublished local history",
        ];
        if result.0 == 0 || !needles.iter().all(|needle| result.1.contains(needle)) {
            failures.push(format!("{case}: {}", result.1));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
