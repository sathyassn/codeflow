//! The Outcome adoption marker is judged against the trusted target
//! (TSK-435): a change may keep or drop uids its target lists, never add
//! one, so a pull request cannot list its own record as present at
//! adoption to turn the Outcome refusal into a warning. The one exception
//! is the adoption write itself, where the target has no marker.

use std::path::Path;
use std::process::Command;

use codeflow_core::workgraph::lifecycle::judge_pull_request;

#[path = "support/git_isolation.rs"]
mod git_isolation;
use git_isolation::{isolate_git, own_repository};

const OLD: &str = "11111111-1111-4111-8111-111111111111";
const NEW: &str = "22222222-2222-4222-8222-222222222222";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn state(uids: Option<&[&str]>) -> String {
    let marker = uids.map_or_else(String::new, |uids| {
        let list = uids
            .iter()
            .map(|uid| format!("\"{uid}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("outcome_present_uids = [{list}]\n")
    });
    format!("schema_version = 1\ntier = \"full\"\n{marker}")
}

fn task(id: &str, uid: &str, outcome: bool) -> String {
    let outcome = if outcome {
        "## Outcome\n\nafter this merges, a reviewer can see the rule hold\n\n"
    } else {
        ""
    };
    format!(
        "---\nid: {id}\nuid: {uid}\nepic_id: null\nstandalone_reason: \"fixture\"\ntitle: \"A task\"\n\
         status: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: main\n\
         created: 2026-10-27\n---\n\n# {id}: A task\n\n## Description\n\nSome change in `src/lib.rs`.\n\n\
         {outcome}## Acceptance Criteria\n\n- AC-1 When run, it works.\n"
    )
}

/// A repository whose `main` holds TSK-001 (uid `OLD`) and `base_uids` as
/// the marker, with a task branch that adds TSK-002 (uid `NEW`) and writes
/// `head_uids` as the marker. Returns the marker errors of the judged range.
fn marker_errors(
    base_uids: Option<&[&str]>,
    head_uids: Option<&[&str]>,
    outcome: bool,
) -> Vec<String> {
    isolate_git();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    own_repository(root);
    write(root, ".codeflow/project.toml", &state(base_uids));
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", OLD, false),
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: seed"]);
    git(root, &["switch", "-q", "-c", "task/TSK-002-x"]);
    write(root, ".codeflow/project.toml", &state(head_uids));
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", NEW, outcome),
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "feat: add a task"]);
    judge_pull_request(root, "main", "HEAD")
        .unwrap()
        .errors
        .into_iter()
        .filter(|error| error.contains("outcome_present_uids"))
        .collect()
}

/// A pull request that adds its own record's uid to the target's marker is
/// refused, naming the uid.
#[test]
fn a_change_that_lists_its_own_record_is_refused() {
    let errors = marker_errors(Some(&[OLD]), Some(&[OLD, NEW]), false);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains(NEW), "{errors:?}");
}

/// A pull request that removes the target's marker is refused.
#[test]
fn a_change_that_removes_the_marker_is_refused() {
    let errors = marker_errors(Some(&[OLD]), None, true);
    assert_eq!(errors.len(), 1, "{errors:?}");
}

/// Control: an unchanged list, or one that drops a uid, passes.
#[test]
fn an_unchanged_or_shorter_list_passes() {
    assert!(marker_errors(Some(&[OLD]), Some(&[OLD]), true).is_empty());
    assert!(marker_errors(Some(&[OLD]), Some(&[]), true).is_empty());
}

/// The adoption write: a target with no marker admits the change that
/// writes one, whatever it lists.
#[test]
fn the_adoption_write_passes() {
    assert!(marker_errors(None, Some(&[OLD, NEW]), false).is_empty());
}

/// A repository on `main` holding `uids` as the marker, ready for commits.
fn seeded(uids: Option<&[&str]>) -> tempfile::TempDir {
    isolate_git();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    own_repository(root);
    write(root, ".codeflow/project.toml", &state(uids));
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: seed"]);
    dir
}

fn commit_state(root: &Path, text: &str, message: &str) {
    write(root, ".codeflow/project.toml", text);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

fn marker_errors_at_head(root: &Path) -> Vec<String> {
    codeflow_core::workgraph::lifecycle::outcome_marker_errors(root, "main", Some("HEAD")).unwrap()
}

/// A target whose marker parses as TOML but is no list refuses, also for
/// a head that leaves the marker as it was where it forked.
#[test]
fn a_malformed_target_marker_refuses_an_unchanged_head() {
    let dir = seeded(Some(&[OLD]));
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "task/TSK-002-x"]);
    write(root, "notes.txt", "a change\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "feat: add notes"]);
    git(root, &["switch", "-q", "main"]);
    commit_state(
        root,
        "schema_version = 1\ntier = \"full\"\noutcome_present_uids = \"broken\"\n",
        "chore: break the marker",
    );
    git(root, &["switch", "-q", "task/TSK-002-x"]);
    let errors = marker_errors_at_head(root);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("cannot be read"), "{errors:?}");
}

/// After a criss-cross merge the target and the head have two best fork
/// points, one listing `OLD` and one not. The target dropped `OLD`; a head
/// that keeps it matches one fork point only, so it is judged and refused.
#[test]
fn a_head_matching_one_of_several_fork_points_is_judged() {
    for order in [["a", "b"], ["b", "a"]] {
        let dir = seeded(Some(&[]));
        let root = dir.path();
        git(root, &["branch", "a"]);
        git(root, &["branch", "b"]);
        for side in order {
            git(root, &["switch", "-q", side]);
            if side == "a" {
                commit_state(root, &state(Some(&[OLD])), "chore: list a record");
            } else {
                write(root, "b.txt", "b\n");
                git(root, &["add", "-A"]);
                git(root, &["commit", "-q", "-m", "feat: add b"]);
            }
        }
        // The target merges `a` into `b` and keeps the shorter list.
        git(root, &["switch", "-q", "-C", "main", "b"]);
        git(
            root,
            &["merge", "-q", "--no-ff", "-m", "chore: merge a", "a"],
        );
        commit_state(root, &state(Some(&[])), "chore: drop the record");
        // The head merges `b` into `a` and keeps `a`'s list.
        git(root, &["switch", "-q", "-c", "task/TSK-002-x", "a"]);
        git(
            root,
            &["merge", "-q", "--no-ff", "-m", "chore: merge b", "b"],
        );
        let errors = marker_errors_at_head(root);
        assert_eq!(errors.len(), 1, "{order:?}: {errors:?}");
        assert!(errors[0].contains(OLD), "{order:?}: {errors:?}");
    }
}
