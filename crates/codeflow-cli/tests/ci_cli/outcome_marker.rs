//! The Outcome adoption marker is judged in every tier (TSK-435): a pull
//! request may not add a uid to `outcome_present_uids` that its target
//! does not list, also in a minimal or standard project that keeps only
//! epics, or no records yet, where no other record rule runs.

use std::path::Path;

use super::{git, run_in};

const STATE: &str = ".codeflow/project.toml";

/// A `trunk` commit holding `codeflow init --<tier>` (whose marker lists
/// no record) and, when `epic` is set, one epic record listed in the
/// marker; then a `feat/x` branch.
fn adopted(dir: &Path, tier: &str, epic: bool) {
    git(dir, &["init", "-b", "trunk"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "# sample\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "chore: init"]);
    let out = run_in(dir, &["init", &format!("--{tier}"), "--yes"]);
    assert!(
        out.status.success(),
        "init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(listed(dir), Vec::<String>::new(), "init lists no record");
    if epic {
        add_epic(dir, "EPC-001", "11111111-1111-4111-8111-111111111111");
        list(dir, "11111111-1111-4111-8111-111111111111");
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "chore: adopt codeflow"]);
    git(dir, &["checkout", "-q", "-b", "feat/x"]);
}

fn add_epic(dir: &Path, id: &str, uid: &str) {
    let epics = dir.join("project-management/epics");
    std::fs::create_dir_all(&epics).unwrap();
    std::fs::write(
        epics.join(format!("{id}.md")),
        format!(
            "---\nid: {id}\nuid: {uid}\ntitle: \"An epic\"\nstatus: active\ncreated: 2026-10-27\n---\n\n# {id}: An epic\n\n## Summary\n\nWork.\n"
        ),
    )
    .unwrap();
}

fn state(dir: &Path) -> toml::Table {
    toml::from_str(&std::fs::read_to_string(dir.join(STATE)).unwrap()).unwrap()
}

fn listed(dir: &Path) -> Vec<String> {
    state(dir)["outcome_present_uids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|uid| uid.as_str().unwrap().to_string())
        .collect()
}

/// Append `uid` to the marker.
fn list(dir: &Path, uid: &str) {
    let mut table = state(dir);
    table["outcome_present_uids"]
        .as_array_mut()
        .unwrap()
        .push(uid.into());
    std::fs::write(dir.join(STATE), toml::to_string(&table).unwrap()).unwrap();
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn ci(dir: &Path) -> (Option<i32>, String) {
    let out = run_in(
        dir,
        &[
            "ci", "--base", "trunk", "--head", "HEAD", "--branch", "feat/x",
        ],
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), all)
}

fn refused(dir: &Path, uid: &str) {
    let (code, out) = ci(dir);
    assert_ne!(code, Some(0), "{out}");
    assert!(
        out.contains("adds uids to outcome_present_uids") && out.contains(uid),
        "{out}"
    );
}

#[test]
fn a_new_epic_cannot_list_itself_in_a_project_that_keeps_only_epics() {
    for tier in ["minimal", "standard"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        adopted(root, tier, true);
        let mine = "22222222-2222-4222-8222-222222222222";
        add_epic(root, "EPC-002", mine);
        list(root, mine);
        commit_all(root, "docs: plan another epic");
        refused(root, mine);
    }
}

#[test]
fn a_change_cannot_add_a_uid_in_a_project_with_no_records() {
    for tier in ["minimal", "standard"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        adopted(root, tier, false);
        let mine = "33333333-3333-4333-8333-333333333333";
        list(root, mine);
        commit_all(root, "chore: list a record");
        refused(root, mine);
    }
}

#[test]
fn removing_the_marker_is_refused_in_a_project_with_no_records() {
    for tier in ["minimal", "standard"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        adopted(root, tier, false);
        let mut table = state(root);
        table.remove("outcome_present_uids");
        std::fs::write(root.join(STATE), toml::to_string(&table).unwrap()).unwrap();
        commit_all(root, "chore: drop the marker");
        let (code, out) = ci(root);
        assert_ne!(code, Some(0), "{out}");
        assert!(out.contains("removes outcome_present_uids"), "{out}");
    }
}

/// Control: a change that leaves the marker alone, or drops a listed uid,
/// passes, and the marker check is named among the checks that ran.
#[test]
fn keeping_or_shortening_the_list_passes_in_every_tier() {
    for tier in ["minimal", "standard"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        adopted(root, tier, true);
        std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
        commit_all(root, "feat: add notes");
        let (code, out) = ci(root);
        assert_eq!(code, Some(0), "{tier}: {out}");
        assert!(out.contains("outcome-marker"), "{tier}: {out}");

        let mut table = state(root);
        table["outcome_present_uids"] = toml::Value::Array(Vec::new());
        std::fs::write(root.join(STATE), toml::to_string(&table).unwrap()).unwrap();
        commit_all(root, "chore: drop a listed uid");
        let (code, out) = ci(root);
        assert_eq!(code, Some(0), "{tier}: {out}");
    }
}
