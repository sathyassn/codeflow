//! TSK-1312: a merge of the default branch into an integration line lands
//! as a sync of that line's epic. The route is decided from the history
//! alone: every commit the range adds beyond the line and the default
//! branch's trusted commit is a two-parent merge whose second parent is on
//! that commit and whose tree is the clean merge of its parents. A branch
//! name never claims it, and every other check still judges the range.

use std::path::Path;
use std::process::Output;

const LINE: &str = "integration/EPC-001-outcome";
const SYNC: &str = "chore/sync-main-into-epc-001";

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall deliver.\n";

fn task(id: &str, target: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n"
    )
}

/// A completed task, as main's landed work leaves it.
fn complete_task(id: &str) -> String {
    format!(
        "{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: 0000000000000000000000000000000000000000\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n    AC-2: verified | journey ran\n  journey: verified | CLI fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n",
        task(id, "main").replace("status: todo", "status: complete")
    )
}

const FRAGMENT: &str = "### Fixed\n\n<!-- codeflow:release-impact patch -->\n- **The fixture's landed fix.** It ships once.\n";

fn body(task_line: &str) -> String {
    format!(
        "## Summary\nThe line takes main's landed work.\n\n- one merge of main\n\n{task_line}\n\n## Changes\n- merge main into the line\n\n## Testing\n- focused test\n\n## Release impact\n- Breaking: no\n- Rationale: main's own entries, already reviewed there\n"
    )
}

fn stdout_of(dir: &Path, args: &[&str]) -> String {
    let out = super::git_command(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn commit(root: &Path, message: &str) {
    super::git(root, &["add", "-A"]);
    super::git(root, &["commit", "-qm", message]);
}

/// `main` holds the epic, a line task TSK-001 and a completed TSK-002; the
/// line forks and moves on; then main lands TSK-002's product change and
/// its release note, which the line does not have.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    super::git(root, &["init", "-q", "-b", "main"]);
    super::git(root, &["config", "user.email", "t@example.com"]);
    super::git(root, &["config", "user.name", "t"]);
    write(root, "project-management/epics/EPC-001.md", EPIC);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", LINE),
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &complete_task("TSK-002"),
    );
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
    );
    write(root, "src/lib.rs", "pub fn base() {}\n");
    write(root, "src/line.rs", "pub fn line() {}\n");
    commit(root, "chore: plan the work");
    super::git(root, &["branch", LINE]);
    super::git(root, &["switch", "-q", LINE]);
    write(root, "docs/line.md", "# The line's notes\n");
    commit(root, "docs: the line moves on");
    super::git(root, &["switch", "-q", "main"]);
    write(root, "src/lib.rs", "pub fn base() {}\npub fn fixed() {}\n");
    write(root, "changelog.d/TSK-002.md", FRAGMENT);
    commit(root, "fix: land the fixture fix");
    dir
}

/// The sync branch: the line with main merged in by git's own merge.
fn sync_branch(root: &Path) {
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
}

/// The hosted check of a pull request into the line, judged with the
/// default branch's commit as the authority, as the policy workflow passes it.
fn ci(root: &Path, task_line: &str, authority: Option<&str>) -> Output {
    let main = stdout_of(root, &["rev-parse", "main"]);
    let mut command = super::codeflow();
    command.args([
        "ci", "--base", LINE, "--head", "HEAD", "--branch", SYNC, "--into", LINE,
    ]);
    command.args(["--policy-from", authority.unwrap_or(&main)]);
    command
        .args(["--pr-body", &body(task_line)])
        .current_dir(root)
        .output()
        .expect("binary runs")
}

fn local_ci(root: &Path, task_line: &str) -> Output {
    super::codeflow()
        .args([
            "ci",
            "--base",
            LINE,
            "--head",
            "HEAD",
            "--branch",
            SYNC,
            "--into",
            LINE,
            "--pr-body",
        ])
        .arg(body(task_line))
        .current_dir(root)
        .output()
        .expect("binary runs")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn passes(out: &Output, what: &str) {
    assert_eq!(out.status.code(), Some(0), "{what}: {}", text(out));
}

fn blocks(out: &Output, what: &str, needle: &str) {
    let said = text(out);
    assert_eq!(out.status.code(), Some(1), "{what}: {said}");
    assert!(
        said.contains(needle),
        "{what}: missing {needle:?} in {said}"
    );
}

/// AC-1 and AC-2: a clean sync whose net diff carries a release note. The
/// epic route used to refuse it as a planning amendment and the landed
/// task's route as a completed task; the epic route now admits it as a
/// sync, and the completed task's route is still refused.
#[test]
fn a_clean_sync_of_main_with_a_release_note_lands_as_a_sync() {
    let dir = fixture();
    let root = dir.path();
    sync_branch(root);

    let synced = ci(root, "Task: EPC-001", None);
    passes(&synced, "clean sync");
    let said = text(&synced);
    assert!(
        said.contains(&format!(
            "pull request class: sync of the default branch into {LINE} for EPC-001"
        )),
        "{said}"
    );

    // The landed task still names no new work.
    blocks(
        &ci(root, "Task: TSK-002", None),
        "completed task",
        "work.stable_planning_anchor",
    );
}

/// AC-3: the second parent must be on the default branch's trusted commit,
/// bound by its id, never by a ref name the requester can create.
#[test]
fn a_merge_of_anything_but_the_trusted_default_branch_is_refused() {
    let dir = fixture();
    let root = dir.path();
    // A branch that only looks like main: a local ref named after the
    // remote's main points at it.
    super::git(root, &["switch", "-q", "-c", "rogue", "main"]);
    write(
        root,
        "src/lib.rs",
        "pub fn base() {}\npub fn slipped() {}\n",
    );
    commit(root, "fix: an unreviewed change");
    let rogue = stdout_of(root, &["rev-parse", "HEAD"]);
    super::git(root, &["update-ref", "refs/remotes/origin/main", &rogue]);
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(
        root,
        &["merge", "--no-ff", "-q", "--no-edit", "origin/main"],
    );

    blocks(
        &ci(root, "Task: EPC-001", None),
        "second parent off main",
        "is not on the default branch's commit",
    );
    // A judgment whose authority is not named holds no trusted commit.
    blocks(
        &local_ci(root, "Task: EPC-001"),
        "no trusted commit",
        "no default branch commit to trust",
    );
}

/// AC-3: a non-merge commit in the range, whether after a merge or as the
/// whole range of a branch named like a sync, is refused naming it.
#[test]
fn a_range_with_a_commit_of_its_own_is_refused() {
    let dir = fixture();
    let root = dir.path();
    sync_branch(root);
    write(root, "src/line.rs", "pub fn line() {}\npub fn extra() {}\n");
    commit(root, "fix: one more change");
    let own = stdout_of(root, &["rev-parse", "HEAD"]);
    blocks(
        &ci(root, "Task: EPC-001", None),
        "commit after the merge",
        &format!("commit {} is not a merge", &own[..9]),
    );

    // The branch name alone claims nothing.
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    write(root, "changelog.d/TSK-001.md", FRAGMENT);
    commit(root, "docs: a release note");
    let by_name = ci(root, "Task: EPC-001", None);
    blocks(&by_name, "sync by name", "is not a merge");
    // AC-4: the refusal's remedy names the route and where it is written.
    assert!(
        text(&by_name).contains(
            "a sync of the default branch into that line (clean two-parent merges of the default branch's commit only, see docs/delivery.md)"
        ),
        "{}",
        text(&by_name)
    );
}

/// AC-3: a commit before the merge is caught as one after it is: the merge
/// proves nothing about the first parent it stands on.
#[test]
fn a_commit_hidden_under_the_merge_is_refused() {
    let dir = fixture();
    let root = dir.path();
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    write(
        root,
        "src/line.rs",
        "pub fn line() {}\npub fn hidden() {}\n",
    );
    commit(root, "fix: a change under the merge");
    let hidden = stdout_of(root, &["rev-parse", "HEAD"]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    blocks(
        &ci(root, "Task: EPC-001", None),
        "commit under the merge",
        &format!("commit {} is not a merge", &hidden[..9]),
    );
}

/// AC-3: the trust is the exact commit the run names. Main's newer commit is
/// refused while the authority is an older one, and an octopus merge is
/// refused though each of its parents is on the authority's history.
#[test]
fn only_the_named_commit_and_two_parents_count() {
    let dir = fixture();
    let root = dir.path();
    let older = stdout_of(root, &["rev-parse", "main~1"]);
    sync_branch(root);
    blocks(
        &ci(root, "Task: EPC-001", Some(&older)),
        "merge beyond the authority",
        "is not on the default branch's commit",
    );

    // Two heads neither of which holds the other, both on main's next commit.
    let fix = stdout_of(root, &["rev-parse", "main"]);
    super::git(root, &["switch", "-q", "-C", "side", &older]);
    write(root, "docs/side.md", "# A side note\n");
    commit(root, "docs: a side note");
    super::git(root, &["switch", "-q", "main"]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "side"]);
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(root, &["merge", "-q", "--no-edit", &fix, "side"]);
    blocks(
        &ci(root, "Task: EPC-001", None),
        "octopus merge",
        "is an octopus merge",
    );
}

/// AC-3: merging the line into an open sync is not a merge of the default
/// branch, so a line that moved means a new sync from its tip, as
/// docs/delivery.md says.
#[test]
fn a_merge_of_the_line_into_the_sync_is_refused() {
    let dir = fixture();
    let root = dir.path();
    sync_branch(root);
    super::git(root, &["switch", "-q", LINE]);
    write(root, "docs/line.md", "# The line's notes\n\nMore.\n");
    commit(root, "docs: the line moves again");
    let tip = stdout_of(root, &["rev-parse", "HEAD"]);
    super::git(root, &["switch", "-q", SYNC]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", LINE]);
    blocks(
        &ci(root, "Task: EPC-001", None),
        "line merged into the sync",
        &format!("takes {}, which is not on the default branch", &tip[..9]),
    );
}

/// The route belongs to the epic of the line the range targets: naming
/// another epic admits nothing, whatever the history proves.
#[test]
fn a_sync_naming_another_epic_is_refused() {
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    commit(root, "chore: another epic on main");
    sync_branch(root);
    let refused = ci(root, "Task: EPC-002", None);
    blocks(&refused, "another epic", "work.classification");
    assert!(
        !text(&refused).contains("pull request class: sync"),
        "{}",
        text(&refused)
    );
}

/// AC-2: every other refusal still applies. A clean sync that would change
/// the criteria, status or text of a task whose home is the line, or the
/// epic's criteria, is refused by the frozen criteria. Under the one-home
/// rule main never holds such a change; these fixtures force one.
#[test]
fn a_clean_sync_cannot_change_the_lines_records() {
    for (path, content, what) in [
        (
            "project-management/tasks/TSK-001.md",
            task("TSK-001", LINE).replace(
                "- AC-1 When run, the system shall work.",
                "- AC-1 When run, the system may work.",
            ),
            "a task's criteria",
        ),
        (
            "project-management/epics/EPC-001.md",
            EPIC.replace(
                "- AC-1 When run, the system shall deliver.",
                "- AC-1 When run, the system may deliver.",
            ),
            "the epic's criteria",
        ),
        (
            "project-management/tasks/TSK-001.md",
            task("TSK-001", LINE).replace("status: todo", "status: blocked"),
            "a task's status",
        ),
        (
            "project-management/tasks/TSK-001.md",
            task("TSK-001", LINE).replace("Work.", "Other work."),
            "a task's text",
        ),
    ] {
        let dir = fixture();
        let root = dir.path();
        write(root, path, &content);
        commit(root, "docs: main changes the criteria");
        sync_branch(root);
        let refused = ci(root, "Task: EPC-001", None);
        let said = text(&refused);
        assert!(
            said.contains("pull request class: sync"),
            "{what} is judged as a sync: {said}"
        );
        blocks(&refused, what, "work.criteria_frozen");
    }
}

/// AC-2: the freeze covers records whose home is the line, so another
/// epic's record whose home is main comes along with the sync when main
/// changes it.
#[test]
fn a_clean_sync_carries_another_epics_record() {
    let dir = fixture();
    let root = dir.path();
    let other = task("TSK-003", "main").replace("epic_id: EPC-001", "epic_id: EPC-002");
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    write(root, "project-management/tasks/TSK-003.md", &other);
    commit(root, "docs: plan another epic");
    // The line takes that plan, and main then rewords the other epic's task.
    super::git(root, &["switch", "-q", LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    super::git(root, &["switch", "-q", "main"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &other.replace("Work.", "Other work."),
    );
    write(root, "changelog.d/TSK-003.md", FRAGMENT);
    commit(root, "docs: reword the other epic's task");
    sync_branch(root);
    passes(&ci(root, "Task: EPC-001", None), "another epic's record");
}

/// AC-2 as amended 2026-11-05: a record's home is where it lands. TSK-005
/// belongs to the line's epic but targets main, so its home is main, and the
/// sync takes main's version of it.
#[test]
fn a_clean_sync_carries_a_main_homed_task_of_the_lines_epic() {
    let dir = fixture();
    let root = dir.path();
    let homed = task("TSK-005", "main");
    write(root, "project-management/tasks/TSK-005.md", &homed);
    commit(root, "docs: plan a task of the epic on main");
    super::git(root, &["switch", "-q", LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    super::git(root, &["switch", "-q", "main"]);
    write(
        root,
        "project-management/tasks/TSK-005.md",
        &homed.replace("Work.", "Other work."),
    );
    write(root, "changelog.d/TSK-005.md", FRAGMENT);
    commit(root, "docs: reword a task whose home is main");
    sync_branch(root);
    passes(
        &ci(root, "Task: EPC-001", None),
        "a main-homed task of the line's epic",
    );
}

/// AC-2 as amended 2026-11-05: a task of another epic that targets the line
/// has its home on the line, so a sync never changes it, even when main
/// holds a change to it against the one-home rule.
#[test]
fn a_clean_sync_cannot_change_another_epics_task_homed_on_the_line() {
    let dir = fixture();
    let root = dir.path();
    let homed = task("TSK-004", LINE).replace("epic_id: EPC-001", "epic_id: EPC-002");
    write(
        root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    write(root, "project-management/tasks/TSK-004.md", &homed);
    commit(root, "docs: plan another epic's task on the line");
    super::git(root, &["switch", "-q", LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    super::git(root, &["switch", "-q", "main"]);
    write(
        root,
        "project-management/tasks/TSK-004.md",
        &homed.replace("Work.", "Other work."),
    );
    write(root, "changelog.d/TSK-004.md", FRAGMENT);
    commit(root, "docs: main rewords a line-homed task");
    sync_branch(root);
    let refused = ci(root, "Task: EPC-001", None);
    let said = text(&refused);
    assert!(
        said.contains("pull request class: sync"),
        "judged as a sync: {said}"
    );
    blocks(&refused, "a line-homed task", "work.criteria_frozen");
    blocks(&refused, "a line-homed task", "whose home is the line");
}

/// AC-3: a merge whose tree is not the clean merge of its parents carries
/// a hunk of its own, which a sync cannot hold.
#[test]
fn a_merge_with_a_hunk_from_neither_parent_is_refused() {
    let dir = fixture();
    let root = dir.path();
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-commit", "main"]);
    write(
        root,
        "src/line.rs",
        "pub fn line() {}\npub fn inside() {}\n",
    );
    super::git(root, &["add", "-A"]);
    super::git(root, &["commit", "-qm", "Merge branch 'main'"]);
    blocks(
        &ci(root, "Task: EPC-001", None),
        "evil merge",
        "src/line.rs beyond the clean merge of its parents",
    );
}
