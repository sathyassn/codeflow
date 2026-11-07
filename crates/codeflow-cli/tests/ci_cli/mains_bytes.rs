//! TSK-1185 AC-11 to AC-16 (operator ruling 7): a line takes the default
//! branch's records by the default branch's bytes. The rule runs only when
//! the run verified the default tip: `--policy-from` names it, the run is
//! not the pre-push hand-off, and this run's own query of `origin` names the
//! default branch at exactly that commit. Each admit fixture is refused
//! without that verification, which is also how it was refused before the
//! rule existed.

use std::path::{Path, PathBuf};
use std::process::Output;

const LINE: &str = "integration/EPC-001-outcome";
const SYNC: &str = "chore/sync-main-into-epc-001";
const RECEIVER: &str = "task/TSK-009-receiver";

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall deliver.\n";

fn task(id: &str, target: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n"
    )
}

/// A completion main admitted whose review the line cannot re-prove: its
/// reviewed commit is not in any history, as an admin push could leave it.
fn admitted_completion(id: &str) -> String {
    format!(
        "{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: 1111111111111111111111111111111111111111\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n    AC-2: verified | journey ran\n  journey: verified | CLI fixture\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n",
        task(id, "main").replace("status: todo", "status: complete")
    )
}

const FRAGMENT: &str = "### Fixed\n\n<!-- codeflow:release-impact patch -->\n- **The fixture's landed fix.** It ships once.\n";

fn body(task_line: &str) -> String {
    format!(
        "## Summary\nThe line takes main's landed work.\n\n- one merge of main\n\n{task_line}\n\n## Changes\n- merge main into the line\n\n## Testing\n- focused test\n\n## Release impact\n- Breaking: no\n- Rationale: main's own entries, already reviewed there\n"
    )
}

fn out(root: &Path, args: &[&str]) -> String {
    let out = super::git_command(root)
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

fn land(root: &Path, branch: &str, changes: &[(&str, &str)], message: &str) {
    super::git(root, &["switch", "-q", "-c", branch, "main"]);
    for (path, content) in changes {
        write(root, path, content);
    }
    commit(root, message);
    super::git(root, &["switch", "-q", "main"]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", branch]);
}

/// A project and its `origin`, a bare repository whose HEAD is `main`.
struct Fixture {
    dir: tempfile::TempDir,
    origin: PathBuf,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// `main` holds the epic, line task TSK-001 and main task TSK-003, both
    /// `todo`; the line forks and moves on.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap().keep().join("origin.git");
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
            "project-management/tasks/TSK-003.md",
            &task("TSK-003", "main"),
        );
        write(
            root,
            ".codeflow/policy.json",
            "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
        );
        write(root, "src/lib.rs", "pub fn base() {}\n");
        commit(root, "chore: plan the work");
        super::git(root, &["branch", LINE]);
        super::git(root, &["switch", "-q", LINE]);
        write(root, "docs/line.md", "# The line's notes\n");
        commit(root, "docs: the line moves on");
        super::git(root, &["switch", "-q", "main"]);
        super::git(
            root,
            &[
                "init",
                "--bare",
                "-q",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        super::git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
        Self { dir, origin }
    }

    /// Main lands a fix and the completion of TSK-003 it admitted.
    fn main_completes(&self) {
        let root = self.root();
        write(root, "src/lib.rs", "pub fn base() {}\npub fn fixed() {}\n");
        write(root, "changelog.d/TSK-003.md", FRAGMENT);
        write(
            root,
            "project-management/tasks/TSK-003.md",
            &admitted_completion("TSK-003"),
        );
        commit(root, "fix: land TSK-003");
    }

    /// Record main's ids in the id registry, as a real repository holds it.
    fn seed_registry(&self) {
        let seeded = super::codeflow()
            .args(["ids", "seed"])
            .current_dir(self.root())
            .output()
            .unwrap();
        assert!(seeded.status.success(), "{}", text(&seeded));
    }

    /// Publish main, the line and any id registry to `origin`, and fetch
    /// them back.
    fn publish(&self) {
        let root = self.root();
        super::git(root, &["push", "-q", "origin", "main", LINE]);
        let registry = super::git_command(root)
            .args([
                "rev-parse",
                "--verify",
                "-q",
                "refs/heads/codeflow/registry",
            ])
            .output()
            .unwrap();
        if registry.status.success() {
            super::git(root, &["push", "-q", "origin", "codeflow/registry"]);
        }
        super::git(root, &["fetch", "-q", "origin"]);
    }

    fn sync(&self) {
        let root = self.root();
        super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
        super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    }

    fn main(&self) -> String {
        out(self.root(), &["rev-parse", "main"])
    }

    /// The hosted check, with the default branch's commit as the authority.
    fn hosted(&self, branch: &str, task_line: &str) -> Output {
        self.run(branch, task_line, &["--policy-from", &self.main()])
    }

    fn run(&self, branch: &str, task_line: &str, extra: &[&str]) -> Output {
        super::codeflow()
            .args(["ci", "--base", LINE, "--head", "HEAD", "--branch", branch])
            .args(["--into", LINE])
            .args(extra)
            .args(["--pr-body", &body(task_line)])
            .current_dir(self.root())
            .output()
            .expect("binary runs")
    }
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

fn refused(out: &Output, what: &str) {
    assert_eq!(out.status.code(), Some(1), "{what}: {}", text(out));
}

fn says(out: &Output, what: &str, needle: &str) {
    let said = text(out);
    assert!(
        said.contains(needle),
        "{what}: missing {needle:?} in {said}"
    );
}

const ON: &str = "main's bytes on for";
const OFF: &str = "main's bytes off for";
const TAKEN: &str = "so the line takes it without re-proof or freeze";

/// AC-11 on a sync: main admitted TSK-003's completion, which the line
/// cannot re-prove. The hosted job takes main's record; a run without the
/// verified tip still refuses it.
#[test]
fn tsk1185_a_sync_takes_a_completion_main_admitted() {
    let fx = Fixture::new();
    fx.main_completes();
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    passes(&hosted, "main's admitted completion on a sync");
    says(&hosted, "rule on", ON);
    says(
        &hosted,
        "record taken",
        "TSK-003 at project-management/tasks/TSK-003.md is the default branch's record",
    );
    let local = fx.run(SYNC, "Task: EPC-001", &[]);
    refused(&local, "no verified tip");
}

/// AC-11 on a reviewed hand-resolved task: the task branch merges main by
/// hand, so the range is no sync, and the matching record is still taken.
#[test]
fn tsk1185_a_hand_resolved_task_takes_a_record_main_admitted() {
    let fx = Fixture::new();
    let root = fx.root();
    write(
        root,
        "project-management/tasks/TSK-009.md",
        &task("TSK-009", LINE),
    );
    commit(root, "docs: plan the receiver");
    super::git(root, &["switch", "-q", LINE]);
    super::git(root, &["merge", "--no-ff", "-q", "--no-edit", "main"]);
    super::git(root, &["switch", "-q", "main"]);
    fx.main_completes();
    write(root, "docs/shared.md", "# Notes\n\nMain's note.\n");
    commit(root, "docs: main's note");
    fx.publish();
    super::git(root, &["switch", "-q", "-c", RECEIVER, LINE]);
    write(root, "docs/shared.md", "# Notes\n\nThe receiver's note.\n");
    commit(root, "docs: the receiver's note");
    // A hand resolution of the document both sides wrote, so the range is
    // no sync; the product file merges cleanly.
    let merged = super::git_command(root)
        .args(["merge", "--no-ff", "-q", "--no-edit", "main"])
        .output()
        .unwrap();
    assert!(!merged.status.success(), "the fixture conflicts");
    write(
        root,
        "docs/shared.md",
        "# Notes\n\nMain's note.\n\nThe receiver's note.\n",
    );
    super::git(root, &["add", "-A"]);
    super::git(root, &["commit", "-qm", "chore: merge main into the task"]);
    let hosted = fx.hosted(RECEIVER, "Task: TSK-009");
    passes(&hosted, "a hand-resolved task with main's record");
    says(
        &hosted,
        "record taken",
        "TSK-003 at project-management/tasks/TSK-003.md is the default branch's record",
    );
    refused(&fx.run(RECEIVER, "Task: TSK-009", &[]), "no verified tip");
}

/// AC-12: a record path main's bytes would take, beside a non-record path in
/// the same commit, does not make the range a sync.
#[test]
fn tsk1185_a_product_change_beside_main_s_record_is_still_judged() {
    let fx = Fixture::new();
    fx.main_completes();
    fx.publish();
    let root = fx.root();
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(
        root,
        &[
            "checkout",
            "main",
            "--",
            "project-management/tasks/TSK-003.md",
        ],
    );
    write(
        root,
        "src/lib.rs",
        "pub fn base() {}\npub fn slipped() {}\n",
    );
    commit(root, "docs: take main's record with a product change");
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    refused(&hosted, "product beside a record");
    let said = text(&hosted);
    assert!(
        !said.contains("pull request class: sync"),
        "the record does not make the range a sync: {said}"
    );
}

/// AC-12: the same blob with another mode is not main's tree entry.
#[cfg(unix)]
#[test]
fn tsk1185_a_mode_only_difference_keeps_the_full_checks() {
    let fx = Fixture::new();
    fx.main_completes();
    fx.publish();
    fx.sync();
    let root = fx.root();
    super::git(
        root,
        &[
            "update-index",
            "--chmod=+x",
            "project-management/tasks/TSK-003.md",
        ],
    );
    super::git(root, &["commit", "-qm", "docs: mark the record executable"]);
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    refused(&hosted, "mode-only difference");
    let said = text(&hosted);
    assert!(!said.contains(TAKEN), "nothing is taken: {said}");
}

/// AC-12, group H (the TSK-350 shape): the line edited a line-homed record's
/// criteria; a head that puts main's bytes back stays frozen.
#[test]
fn tsk1185_a_line_edit_of_a_line_homed_record_stays_frozen() {
    let fx = Fixture::new();
    let root = fx.root();
    super::git(root, &["switch", "-q", LINE]);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", LINE).replace("shall work.", "shall work well."),
    );
    commit(root, "docs: the line amends its task");
    super::git(root, &["switch", "-q", "main"]);
    fx.main_completes();
    fx.publish();
    super::git(root, &["switch", "-q", "-C", SYNC, LINE]);
    super::git(
        root,
        &[
            "checkout",
            "main",
            "--",
            "project-management/tasks/TSK-001.md",
        ],
    );
    commit(root, "docs: put main's criteria back");
    // The rule declines the record, so the run judges exactly as it does
    // without the verified tip: here as the epic's planning amendment, the
    // one route that may change a line task's criteria.
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    let local = fx.run(SYNC, "Task: EPC-001", &[]);
    assert_eq!(
        hosted.status.code(),
        local.status.code(),
        "the rule changes nothing: {}",
        text(&hosted)
    );
    let said = text(&hosted);
    assert!(!said.contains(TAKEN), "nothing is taken: {said}");
    says(
        &hosted,
        "line edit",
        "the line has edited it since its merge base with the default branch",
    );
}

/// AC-12, group B: main re-homes a line task in a commit that also carries
/// product, so no planning landing moved it, and the sync stays refused.
#[test]
fn tsk1185_a_re_home_carried_with_product_stays_refused() {
    let fx = Fixture::new();
    let root = fx.root();
    land(
        root,
        "feat/re-home",
        &[
            (
                "project-management/tasks/TSK-001.md",
                &task("TSK-001", "main"),
            ),
            ("src/lib.rs", "pub fn base() {}\npub fn moved() {}\n"),
        ],
        "feat: move the task with product",
    );
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    refused(&hosted, "re-home with product");
    says(&hosted, "home", "declare different homes for it");
}

/// AC-16: a planning landing on main's first-parent chain moved the line task
/// to main while it was `todo`; the line takes main's record.
#[test]
fn tsk1185_a_planned_move_on_main_s_chain_is_taken() {
    let fx = Fixture::new();
    let root = fx.root();
    land(
        root,
        "plan/move-home",
        &[(
            "project-management/tasks/TSK-001.md",
            &task("TSK-001", "main"),
        )],
        "docs: move the task to main",
    );
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    passes(&hosted, "planned move on main's chain");
    says(&hosted, "record taken", TAKEN);
    refused(&fx.run(SYNC, "Task: EPC-001", &[]), "no verified tip");
}

/// AC-12: another line moved the task, and the move reached main inside
/// that line's checkpoint merge, which carries product. Only the merge is on
/// main's first-parent chain, and it is no planning landing, so nothing on
/// main's chain moved the task.
#[test]
fn tsk1185_a_home_move_from_another_line_stays_refused() {
    let fx = Fixture::new();
    let root = fx.root();
    super::git(
        root,
        &["switch", "-q", "-c", "integration/EPC-002-other", "main"],
    );
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001", "main"),
    );
    commit(root, "docs: another line moves the task");
    write(root, "src/other.rs", "pub fn other_line() {}\n");
    commit(root, "feat: the other line's product");
    super::git(root, &["switch", "-q", "main"]);
    write(root, "docs/main.md", "# Main moves on\n");
    commit(root, "docs: main moves on");
    super::git(
        root,
        &[
            "merge",
            "--no-ff",
            "-q",
            "--no-edit",
            "integration/EPC-002-other",
        ],
    );
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    refused(&hosted, "move from another line");
    let said = text(&hosted);
    assert!(!said.contains(TAKEN), "nothing is taken: {said}");
}

/// AC-13: a record the line never held is taken when its entry equals
/// main's, and keeps the full checks when it differs.
#[test]
fn tsk1185_a_record_the_line_never_held_is_taken_only_by_main_s_bytes() {
    let fx = Fixture::new();
    let root = fx.root();
    write(
        root,
        "project-management/tasks/TSK-005.md",
        &admitted_completion("TSK-005"),
    );
    write(root, "changelog.d/TSK-005.md", FRAGMENT);
    commit(root, "fix: land TSK-005");
    fx.seed_registry();
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    passes(&hosted, "a new record main admitted");
    says(
        &hosted,
        "record taken",
        "TSK-005 at project-management/tasks/TSK-005.md is the default branch's record",
    );
    refused(&fx.run(SYNC, "Task: EPC-001", &[]), "no verified tip");

    write(
        root,
        "project-management/tasks/TSK-005.md",
        &admitted_completion("TSK-005").replace("Work.", "Changed work."),
    );
    commit(root, "docs: change the new record");
    let changed = fx.hosted(SYNC, "Task: EPC-001");
    refused(&changed, "a new record that differs from main");
}

/// AC-12: a record that cannot be read is never taken, even when the head
/// holds main's bytes for it.
#[test]
fn tsk1185_an_unreadable_record_is_never_taken() {
    let fx = Fixture::new();
    let root = fx.root();
    fx.main_completes();
    write(
        root,
        "project-management/tasks/TSK-006.md",
        "---\nid: TSK-006\nstatus: [unclosed\n---\n\n# TSK-006\n",
    );
    commit(root, "docs: an unreadable record");
    fx.seed_registry();
    fx.publish();
    fx.sync();
    let hosted = fx.hosted(SYNC, "Task: EPC-001");
    refused(&hosted, "an unreadable record");
    let said = text(&hosted);
    assert!(
        !said.contains(
            "TSK-006 at project-management/tasks/TSK-006.md is the default branch's record"
        ),
        "the unreadable record is not taken: {said}"
    );
}

/// AC-14: nothing the head supplies turns the rule on. A `--policy-from`
/// that is not origin's default tip, a local branch named main moved to the
/// head, and a record the head writes all leave the rule off.
#[test]
fn tsk1185_a_head_supplied_tip_does_not_turn_the_rule_on() {
    let fx = Fixture::new();
    fx.main_completes();
    fx.publish();
    fx.sync();
    let root = fx.root();
    let head = out(root, &["rev-parse", "HEAD"]);
    let forged = fx.run(SYNC, "Task: EPC-001", &["--policy-from", &head]);
    refused(&forged, "the head as the tip");
    says(&forged, "rule off", OFF);
    // A local ref and a remote-tracking ref the requester moves.
    super::git(root, &["update-ref", "refs/remotes/origin/main", &head]);
    super::git(root, &["branch", "-f", "main", &head]);
    let moved = fx.run(SYNC, "Task: EPC-001", &["--policy-from", &head]);
    refused(&moved, "moved local refs");
    says(&moved, "rule off", OFF);
}

/// AC-15: the pre-push hand-off keeps full proof, as does a destination
/// given on the command line, which `pushurl` or `insteadOf` can redirect.
#[test]
fn tsk1185_the_pre_push_path_keeps_full_proof() {
    let fx = Fixture::new();
    fx.main_completes();
    fx.publish();
    fx.sync();
    let main = fx.main();
    let origin = fx.origin.to_str().unwrap().to_string();
    let pushed = format!("refs/heads/{SYNC}");
    for extra in [
        // The argument set `push_set.rs` hands `codeflow ci`.
        vec![
            "--pushed-ref",
            pushed.as_str(),
            "--policy-from",
            main.as_str(),
            "--commits-from",
            main.as_str(),
            "--destination",
            origin.as_str(),
        ],
        vec![
            "--policy-from",
            main.as_str(),
            "--commits-from",
            main.as_str(),
        ],
        vec![
            "--policy-from",
            main.as_str(),
            "--destination",
            origin.as_str(),
        ],
        vec![
            "--policy-from",
            main.as_str(),
            "--pushed-ref",
            "refs/heads/chore/sync-main-into-epc-001",
        ],
    ] {
        let run = fx.run(SYNC, "Task: EPC-001", &extra);
        refused(&run, &format!("{extra:?}"));
        says(&run, "rule off", OFF);
    }
    // A destination redirected through insteadOf names another tip.
    let fake = tempfile::tempdir().unwrap().keep().join("fake.git");
    super::git(
        fx.root(),
        &["init", "--bare", "-q", "-b", "main", fake.to_str().unwrap()],
    );
    super::git(
        fx.root(),
        &["push", "-q", fake.to_str().unwrap(), "HEAD:main"],
    );
    super::git(
        fx.root(),
        &[
            "config",
            &format!("url.{}.insteadOf", fake.display()),
            &origin,
        ],
    );
    // The hook's hand-off over the redirected destination, naming the head
    // as the tip the fake destination advertises, keeps full proof.
    let head = out(fx.root(), &["rev-parse", "HEAD"]);
    let redirected = fx.run(
        SYNC,
        "Task: EPC-001",
        &[
            "--pushed-ref",
            &pushed,
            "--policy-from",
            &head,
            "--commits-from",
            &head,
            "--destination",
            &origin,
        ],
    );
    refused(&redirected, "redirected destination with a forged tip");
    says(&redirected, "rule off", OFF);
}
