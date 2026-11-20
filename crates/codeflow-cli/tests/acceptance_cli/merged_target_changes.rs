//! A late completion is judged by the pull request's own changes against
//! its target (TSK-1257 AC-5). A merge of the target line into the
//! completion branch brings in the line's files, which are not the pull
//! request's changes; a change the pull request's own commits make still
//! refuses.
use super::*;

/// What lands on the line after the task, so the completion branch syncs
/// it in: a changelog fragment and a shipped skill file, the two cases the
/// hosted runs on PR 628 and PR 627 refused.
const LINE_FILES: [(&str, &str); 2] = [
    ("changelog.d/TSK-009.md", "### Fixed\n\n- Another task.\n"),
    (
        ".agents/skills/cf-example/SKILL.md",
        "---\nname: cf-example\n---\n\nAn example skill.\n",
    ),
];

const COMPLETION: &str = "task/TSK-001-complete";

/// TSK-001 lands on the line; its completion branch completes the record,
/// the line then lands another change carrying `LINE_FILES`, and the
/// completion branch merges the line tip. `own` is a non-record file the
/// completion branch's own commit also writes, if any.
fn synced_completion(own: Option<&str>) -> tempfile::TempDir {
    let dir = line_repo(&["TSK-001"]);
    let root = dir.path();
    let reviewed = build(root, "task/TSK-001-work", "src/work.rs");
    land(root, "task/TSK-001-work");

    git(root, &["switch", "-C", COMPLETION, LINE]);
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "in_progress", &valid_block(&reviewed)),
    );
    let verb = status_complete(root, "TSK-001");
    assert_eq!(verb.0, 0, "the verb: {}", verb.1);
    if let Some(path) = own {
        write(root, path, "pub fn extra() {}\n");
    }
    commit(root, "docs(records): complete TSK-001");

    git(root, &["switch", "-C", "other/change", LINE]);
    for (path, text) in LINE_FILES {
        write(root, path, text);
    }
    commit(root, "docs: another task's fragment and skill");
    land(root, "other/change");

    git(root, &["switch", COMPLETION]);
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-m",
            "chore(sync): merge the line",
            LINE,
        ],
    );
    dir
}

#[test]
fn a_late_completion_that_merges_the_target_line_is_admitted() {
    let dir = synced_completion(None);
    let result = ci_on(dir.path(), LINE, COMPLETION, "Task: TSK-001");
    assert_passes(&result, "a records-only completion synced with the line");
    for (path, _) in LINE_FILES {
        assert!(
            !result.1.contains(&format!("also changes {path}")),
            "counts the line's {path} as the completion's: {}",
            result.1
        );
    }
}

#[test]
fn a_late_completion_whose_own_commit_changes_code_still_refuses() {
    let dir = synced_completion(Some("src/extra.rs"));
    let result = ci_on(dir.path(), LINE, COMPLETION, "Task: TSK-001");
    assert_blocks(
        &result,
        "a completion that changes code, synced with the line",
        &["work.acceptance_binding", "also changes src/extra.rs"],
    );
}
