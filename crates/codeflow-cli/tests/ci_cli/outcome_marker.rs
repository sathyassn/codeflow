//! The Outcome adoption marker is judged in every tier (TSK-435): a pull
//! request may not add a uid to `outcome_present_uids` that its target
//! does not list, also in a minimal or standard project that keeps only
//! epics, or no records yet, where no other record rule runs. The target
//! is the range's base: a `--baseline-from` commit, which the pre-push
//! hook takes from the pushed branch's own earlier tip, never governs it.

use std::path::Path;

use super::{git, run_in};

const STATE: &str = ".codeflow/project.toml";

/// A `trunk` commit holding `codeflow init --<tier>` (whose marker lists
/// no record) and, when `epic` is set, one epic record listed in the
/// marker; then a `feat/x` branch.
fn adopted(dir: &Path, tier: &str, epic: bool) {
    scaffolded(dir, tier, |dir| {
        assert_eq!(listed(dir), Vec::<String>::new(), "init lists no record");
        if epic {
            add_epic(dir, "EPC-001", "11111111-1111-4111-8111-111111111111");
            list(dir, "11111111-1111-4111-8111-111111111111");
        }
    });
}

/// A `trunk` commit holding `codeflow init --<tier>` as `prepare` leaves
/// it; then a `feat/x` branch.
fn scaffolded(dir: &Path, tier: &str, prepare: impl FnOnce(&Path)) {
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
    prepare(dir);
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
fn keeping_or_shortening_the_list_passes_in_minimal_and_standard() {
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

/// `codeflow ci` with `--baseline-from`, as the pre-push hook runs it with
/// the pushed branch's own earlier tip.
fn ci_from(dir: &Path, earlier: &str) -> (Option<i32>, String) {
    let out = run_in(
        dir,
        &[
            "ci",
            "--base",
            "trunk",
            "--head",
            "HEAD",
            "--branch",
            "feat/x",
            "--baseline-from",
            earlier,
        ],
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), all)
}

fn head(dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The branch's own earlier tip never governs the marker: a uid it listed
/// earlier is still an addition against the target.
#[test]
fn an_earlier_tip_of_the_branch_cannot_launder_a_uid() {
    for tier in ["standard", "full"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        adopted(root, tier, false);
        let mine = "44444444-4444-4444-8444-444444444444";
        list(root, mine);
        commit_all(root, "chore: list a record");
        let earlier = head(root);
        std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
        commit_all(root, "feat: add notes");
        let (code, out) = ci_from(root, &earlier);
        assert_ne!(code, Some(0), "{tier}: {out}");
        assert!(
            out.contains("adds uids to outcome_present_uids") && out.contains(mine),
            "{tier}: {out}"
        );
    }
}

/// A target with no marker admits the adoption write, also when the branch
/// already wrote one at its earlier tip and now lists a record that landed
/// on the target meanwhile.
#[test]
fn an_adoption_write_may_grow_while_the_target_has_no_marker() {
    for tier in ["standard", "full"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // An older release wrote no marker.
        scaffolded(root, tier, |dir| {
            let mut table = state(dir);
            table.remove("outcome_present_uids");
            std::fs::write(dir.join(STATE), toml::to_string(&table).unwrap()).unwrap();
        });
        let mut table = state(root);
        table.insert(
            "outcome_present_uids".into(),
            toml::Value::Array(vec!["55555555-5555-4555-8555-555555555555".into()]),
        );
        std::fs::write(root.join(STATE), toml::to_string(&table).unwrap()).unwrap();
        commit_all(root, "chore: adopt the Outcome section");
        let earlier = head(root);
        list(root, "66666666-6666-4666-8666-666666666666");
        commit_all(root, "chore: list a record that landed meanwhile");
        let (code, out) = ci_from(root, &earlier);
        assert_eq!(code, Some(0), "{tier}: {out}");
    }
}

/// The caller's `--base` is a diff boundary, never the marker's authority:
/// with `--into` naming the target, a base at the head itself still judges
/// the head's list against the target's tip.
#[test]
fn a_base_at_the_head_does_not_judge_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    adopted(root, "standard", false);
    let mine = "77777777-7777-4777-8777-777777777777";
    list(root, mine);
    commit_all(root, "chore: list a record");
    let out = run_in(
        root,
        &[
            "ci", "--base", "HEAD", "--head", "HEAD", "--branch", "feat/x", "--into", "trunk",
        ],
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("adds uids to outcome_present_uids") && all.contains(mine),
        "{all}"
    );
}

/// Write the marker into `dir`'s project state file: `uids`, or no marker.
fn rewrite_marker(dir: &Path, uids: Option<&[&str]>) {
    let mut table = state(dir);
    match uids {
        Some(uids) => {
            table.insert(
                "outcome_present_uids".into(),
                toml::Value::Array(uids.iter().map(|uid| (*uid).into()).collect()),
            );
        }
        None => {
            table.remove("outcome_present_uids");
        }
    }
    std::fs::write(dir.join(STATE), toml::to_string(&table).unwrap()).unwrap();
}

/// Push `feat/x` from `dir` to a bare copy of `trunk` as `trunk` stands
/// now, through the hooks `init` installed, and return the push's exit
/// code and output.
fn push_to_trunk_copy(dir: &Path) -> (Option<i32>, String) {
    let remote = tempfile::tempdir().unwrap();
    let bare = remote.path().join("origin.git");
    let bare = bare.to_str().unwrap();
    codeflow_fixture::clone(dir, ".", bare)
        .bare()
        .branch("trunk")
        .run();
    // The copy stands for the target alone, so the pushed branch is new to it.
    git(Path::new(bare), &["update-ref", "-d", "refs/heads/feat/x"]);
    git(dir, &["remote", "add", "origin", bare]);
    git(dir, &["fetch", "-q", "origin"]);
    let out = super::git_command(dir)
        .args(["push", "origin", "feat/x"])
        .env_remove("CODEFLOW_PR_BODY")
        .output()
        .expect("git push runs");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), all)
}

/// Pre-push: the target adopted the marker after the branch forked. The
/// push's range starts at the unmarked fork point, but the branch's own
/// adoption write is judged against the target's tip and refused.
#[test]
fn a_push_cannot_list_its_record_after_the_target_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffolded(root, "standard", |dir| rewrite_marker(dir, None));
    git(root, &["checkout", "-q", "trunk"]);
    rewrite_marker(root, Some(&[]));
    commit_all(root, "chore: adopt the Outcome section");
    git(root, &["checkout", "-q", "feat/x"]);
    let mine = "88888888-8888-4888-8888-888888888888";
    rewrite_marker(root, Some(&[mine]));
    commit_all(root, "chore: list my record");
    let (code, out) = push_to_trunk_copy(root);
    assert_ne!(code, Some(0), "{out}");
    assert!(
        out.contains("adds uids to outcome_present_uids") && out.contains(mine),
        "{out}"
    );
}

/// Pre-push: the target removed an exemption after the branch forked. A
/// branch that still lists it while changing the list is refused, though
/// the fork point listed it.
#[test]
fn a_push_cannot_keep_an_exemption_the_target_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (kept, other) = (
        "99999999-9999-4999-8999-999999999999",
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    );
    scaffolded(root, "standard", |dir| {
        rewrite_marker(dir, Some(&[kept, other]));
    });
    git(root, &["checkout", "-q", "trunk"]);
    rewrite_marker(root, Some(&[other]));
    commit_all(root, "chore: drop an exemption");
    git(root, &["checkout", "-q", "feat/x"]);
    rewrite_marker(root, Some(&[kept]));
    commit_all(root, "chore: drop the other uid");
    let (code, out) = push_to_trunk_copy(root);
    assert_ne!(code, Some(0), "{out}");
    assert!(
        out.contains("adds uids to outcome_present_uids") && out.contains(kept),
        "{out}"
    );
}

/// Control: a branch that forked before the target adopted the marker and
/// leaves the file alone pushes cleanly; merging it keeps the target's list.
#[test]
fn a_push_that_leaves_the_marker_alone_passes_after_the_target_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scaffolded(root, "standard", |dir| rewrite_marker(dir, None));
    git(root, &["checkout", "-q", "trunk"]);
    rewrite_marker(root, Some(&[]));
    commit_all(root, "chore: adopt the Outcome section");
    git(root, &["checkout", "-q", "feat/x"]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    commit_all(root, "feat: add notes");
    let (code, out) = push_to_trunk_copy(root);
    assert_eq!(code, Some(0), "{out}");
}
