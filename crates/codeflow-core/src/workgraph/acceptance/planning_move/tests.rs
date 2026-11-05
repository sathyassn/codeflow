//! TSK-1320: fixtures of a reviewed task whose epic a landed planning pull
//! request moved, and of every edit that must keep refusing.

use git2::{Oid, Repository};

use super::super::{
    bind_completion_with_amendment, BindingResult, Graph, Landing, RecordView, RunBases, Transport,
};
use super::{shape, target_changes, Scope};
use crate::workgraph::work_start::RecordKind;

const LINE: &str = "integration/EPC-001-line";
const TASK: &str = "project-management/tasks/TSK-001.md";
const BULLET: &str = "- Regrouped by the planning pull request: from EPC-001 to EPC-002.";

/// The task record as the plan wrote it, with `epic` and `status`.
fn record(epic: &str, status: &str) -> String {
    format!(
        "---\nid: TSK-001\nepic_id: {epic}              # EPC-NNN\nstatus: {status}             # todo | complete\nintegration_target: \"{LINE}\"\n---\n\n# TSK-001: work\n\n## Description\n\nScope line one.\nScope line two.\n\n## Deliverables\n\n- the work file\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Execution contract\n\n- Builder: anyone.\n- Edges: none.\n\n## Closeout\n\nPending.\n"
    )
}

/// The regroup's edit: the epic line and one added bullet.
fn moved(text: &str, to: &str) -> String {
    text.replacen("epic_id: EPC-001", &format!("epic_id: {to}"), 1)
        .replacen(
            "- Edges: none.\n",
            &format!("- Edges: none.\n{BULLET}\n"),
            1,
        )
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = crate::git::command()
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Names the fixture uses for branches and commit messages, so a test can
/// vary them (AC-3).
struct Names {
    task_branch: &'static str,
    planning_branch: &'static str,
    message: &'static str,
    date: &'static str,
}

const PLAIN: Names = Names {
    task_branch: "task/TSK-001-work",
    planning_branch: "plan/regroup",
    message: "docs(plan): move open tasks",
    date: "2026-11-03T11:00:00",
};

struct Fixture {
    dir: tempfile::TempDir,
    reviewed: Oid,
    names: Names,
}

impl Fixture {
    /// A line with three epics and the task, and a task branch whose
    /// reviewed commit changes the product.
    fn new(names: Names) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let f = Self {
            dir,
            reviewed: Oid::ZERO_SHA1,
            names,
        };
        let root = f.dir.path();
        git(root, &["init", "-q", "-b", LINE]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        f.write(
            "project-management/epics/EPC-001.md",
            "---\nid: EPC-001\nstatus: in_progress\n---\n\n# EPC-001\n",
        );
        f.write(
            "project-management/epics/EPC-002.md",
            &format!(
                "---\nid: EPC-002\nstatus: in_progress\nparent_line: {LINE}\n---\n\n# EPC-002\n"
            ),
        );
        f.write(
            "project-management/epics/EPC-003.md",
            "---\nid: EPC-003\nstatus: in_progress\nparent_line: integration/EPC-009-elsewhere\n---\n\n# EPC-003\n",
        );
        f.write(
            "project-management/epics/EPC-004.md",
            "---\nid: EPC-004\nstatus: in_progress\n---\n\n# EPC-004\n",
        );
        f.write(TASK, &record("EPC-001", "todo"));
        f.commit("docs(plan): plan the work");
        git(root, &["switch", "-qc", f.names.task_branch]);
        f.write("src/work.rs", "// reviewed work\n");
        let reviewed = f.commit("feat: the work");
        Self { reviewed, ..f }
    }

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap()
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.root().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn head(&self) -> Oid {
        Repository::open(self.root())
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
    }

    fn tip(&self, branch: &str) -> Oid {
        Repository::open(self.root())
            .unwrap()
            .revparse_single(branch)
            .unwrap()
            .id()
    }

    /// Commit everything with `message` and the fixture's author date.
    fn commit(&self, message: &str) -> Oid {
        let date = format!("--date={}", self.names.date);
        git(self.root(), &["add", "-A"]);
        git(self.root(), &["commit", "-qm", message, &date]);
        self.head()
    }

    /// Land `edit` of the task record on the line through a planning
    /// branch and a merge on the line's first-parent chain; the merge.
    fn land_planning(&self, edit: impl Fn(&str) -> String) -> Oid {
        let root = self.root();
        git(root, &["switch", "-qc", self.names.planning_branch, LINE]);
        self.write(TASK, &edit(&self.read(TASK)));
        self.commit(self.names.message);
        git(root, &["switch", "-q", LINE]);
        git(
            root,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                self.names.message,
                self.names.planning_branch,
            ],
        );
        let merge = self.head();
        git(root, &["switch", "-q", self.names.task_branch]);
        merge
    }

    /// Merge `what` into the task branch cleanly.
    fn merge_in(&self, what: &str) {
        git(
            self.root(),
            &["merge", "-q", "--no-ff", "-m", "merge the line", what],
        );
    }

    /// Complete the task on its branch against the reviewed commit.
    fn complete(&self) -> Oid {
        let block = format!(
            "```yaml\nacceptance:\n  reviewed: {}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | test passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```",
            self.reviewed
        );
        let done = self
            .read(TASK)
            .replace("status: todo ", "status: complete ")
            .replace("Pending.", &block);
        self.write(TASK, &done);
        self.commit("docs(records): complete TSK-001")
    }

    /// Bind the completion at the head as `codeflow ci` judges a pull
    /// request into the line at `tip`.
    fn bind_at(&self, tip: Oid) -> BindingResult {
        let repo = Repository::open(self.root()).unwrap();
        let task = RecordView::parse(RecordKind::Task, TASK, &self.read(TASK)).unwrap();
        let run = RunBases::new([tip]);
        bind_completion_with_amendment(
            &repo,
            &task,
            &Graph::default(),
            Landing::Commit(self.head()),
            Some(tip),
            Transport::TaskLanding,
            Some(tip),
            None,
            &run.stacking(),
            &run.criteria(),
        )
    }

    fn bind(&self) -> BindingResult {
        self.bind_at(self.tip(LINE))
    }
}

fn messages(bound: &BindingResult) -> String {
    bound
        .findings
        .iter()
        .map(|finding| finding.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The record's refusal keeps its start, and the binding has no proof.
fn assert_refused(bound: &BindingResult, needles: &[&str]) {
    let text = messages(bound);
    assert!(bound.proof.is_none(), "{text}");
    assert!(!bound.findings.is_empty(), "accepted");
    for needle in needles {
        assert!(text.contains(needle), "{needle}: {text}");
    }
}

/// A regroup of the task into `to` with `extra` applied to the planning
/// commit, merged into the reviewed task branch, then completed.
fn regrouped(to: &str, extra: impl Fn(String) -> String) -> (Fixture, Oid, BindingResult) {
    let f = Fixture::new(PLAIN);
    let merge = f.land_planning(|text| extra(moved(text, to)));
    f.merge_in(LINE);
    f.complete();
    let bound = f.bind();
    (f, merge, bound)
}

/// AC-1: the regroup's shape (the edit on a planning branch, brought to
/// the line by a merge on its first-parent chain, then the line merged
/// cleanly into the task branch) keeps the completion. Before TSK-1320 it
/// refused with "the record changed outside its status and Closeout".
#[test]
fn a_landed_planning_move_keeps_the_reviewed_completion() {
    let (_f, _merge, bound) = regrouped("EPC-002", |text| text);
    assert!(bound.findings.is_empty(), "{}", messages(&bound));
    assert!(bound.proof.is_some());
}

/// AC-1: a move into the line's own epic lands on the same line.
#[test]
fn a_move_into_the_lines_own_epic_is_admitted() {
    // The plan first puts the task under EPC-002, before the review; the
    // landed move then takes it to EPC-001, the line's own epic.
    let f = Fixture::new(PLAIN);
    git(f.root(), &["switch", "-q", LINE]);
    f.write(
        TASK,
        &f.read(TASK)
            .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    f.commit("docs(plan): start under EPC-002");
    git(f.root(), &["switch", "-q", f.names.task_branch]);
    f.merge_in(LINE);
    f.write("src/work.rs", "// reviewed again\n");
    let reviewed = f.commit("feat: the work under EPC-002");
    let f = Fixture { reviewed, ..f };
    f.land_planning(|text| {
        text.replacen("epic_id: EPC-002", "epic_id: EPC-001", 1)
            .replacen(
                "- Edges: none.\n",
                &format!("- Edges: none.\n{BULLET}\n"),
                1,
            )
    });
    f.merge_in(LINE);
    f.complete();
    let bound = f.bind();
    assert!(bound.findings.is_empty(), "{}", messages(&bound));
}

/// AC-2: a planning commit that also changes the criteria refuses, and the
/// message names the commit and the part (AC-4).
#[test]
fn a_criteria_change_with_the_move_refuses_and_names_commit_and_part() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace("shall work.", "shall work less.")
    });
    assert_refused(
        &bound,
        &[
            "the record changed outside its status and Closeout",
            &format!("commit {merge} on the target's first-parent line changed the record"),
            "Acceptance Criteria section",
        ],
    );
}

/// AC-2: an acceptance block the planning commit lands refuses.
#[test]
fn an_acceptance_block_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace(
            "Pending.",
            "Pending.\n\n```yaml\nacceptance:\n  reviewed: 0000000000000000000000000000000000000000\n```",
        )
    });
    assert_refused(&bound, &[&merge.to_string(), "Closeout section changed"]);
}

/// AC-2: a status change in the planning commit refuses.
#[test]
fn a_status_change_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace("status: todo ", "status: blocked ")
    });
    assert_refused(&bound, &[&merge.to_string(), "front matter field `status`"]);
}

/// AC-2: a Deliverables change refuses.
#[test]
fn a_deliverables_change_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace("- the work file\n", "- the work file\n- another file\n")
    });
    assert_refused(&bound, &[&merge.to_string(), "Deliverables section"]);
}

/// AC-2: an `integration_target` change refuses.
#[test]
fn an_integration_target_change_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace(LINE, "integration/EPC-002-line")
    });
    assert_refused(
        &bound,
        &[
            &merge.to_string(),
            "front matter field `integration_target`",
        ],
    );
}

/// AC-2: a removed line outside status and Closeout refuses.
#[test]
fn a_removed_line_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| text.replace("Scope line two.\n", ""));
    assert_refused(
        &bound,
        &[
            &merge.to_string(),
            "`Scope line two.`",
            "removed or changed",
        ],
    );
}

/// AC-2: a changed line outside status and Closeout refuses.
#[test]
fn a_changed_line_with_the_move_refuses() {
    let (_f, merge, bound) = regrouped("EPC-002", |text| {
        text.replace("Scope line two.", "Scope line two, narrowed.")
    });
    assert_refused(&bound, &[&merge.to_string(), "Description section"]);
}

/// AC-2: an added line that changes how the record parses refuses.
#[test]
fn an_added_line_that_changes_the_parse_refuses() {
    for added in [
        "## Notes",
        "## Deliverables",
        "<!-- a note -->",
        "```text\nfenced\n```",
        "",
        "### Note",
    ] {
        let (_f, merge, bound) = regrouped("EPC-002", |text| {
            text.replace("Scope line one.\n", &format!("Scope line one.\n{added}\n"))
        });
        assert!(!bound.findings.is_empty(), "{added:?} was accepted");
        assert_refused(&bound, &[&merge.to_string()]);
    }
}

/// AC-2: an epic that is missing at the target tip, declares no line, is
/// null, or declares another line refuses.
#[test]
fn a_move_to_an_epic_that_does_not_land_on_the_line_refuses() {
    for (to, why) in [
        ("EPC-099", "EPC-099 has no epic record"),
        ("EPC-004", "EPC-004 declares no parent_line"),
        ("EPC-003", "integration/EPC-009-elsewhere"),
        ("null", "epic_id is null or missing"),
    ] {
        let (_f, merge, bound) = regrouped(to, |text| text);
        assert_refused(&bound, &[&merge.to_string(), why]);
    }
}

/// AC-2: the same edit from a commit not on the line's first-parent chain
/// (the planning branch merged straight into the task branch) refuses.
#[test]
fn the_same_edit_from_an_unlanded_branch_refuses() {
    let f = Fixture::new(PLAIN);
    let root = f.root();
    git(root, &["switch", "-qc", "plan/unlanded", LINE]);
    f.write(TASK, &moved(&f.read(TASK), "EPC-002"));
    f.commit("docs(plan): move open tasks");
    git(root, &["switch", "-q", f.names.task_branch]);
    f.merge_in("plan/unlanded");
    f.complete();
    assert_refused(
        &f.bind(),
        &["not on the first-parent line of the target tip"],
    );
}

/// AC-2: a planning commit that reached the line only as a second parent,
/// never on its first-parent chain, carries no authority.
#[test]
fn the_same_edit_merged_from_the_lines_second_parent_refuses() {
    let f = Fixture::new(PLAIN);
    f.land_planning(|text| moved(text, "EPC-002"));
    // The planning branch's own commit is on the line, but only through
    // the landing merge's second parent.
    f.merge_in(PLAIN.planning_branch);
    f.complete();
    assert_refused(
        &f.bind(),
        &["not on the first-parent line of the target tip"],
    );
}

/// AC-2: a local commit made on the task's side of the line refuses: the
/// same edit committed straight onto a local branch that is not the run's
/// target tip.
#[test]
fn the_same_edit_from_a_local_only_commit_refuses() {
    let f = Fixture::new(PLAIN);
    let root = f.root();
    let tip = f.tip(LINE);
    git(root, &["switch", "-qc", "local", LINE]);
    f.write(TASK, &moved(&f.read(TASK), "EPC-002"));
    f.commit("docs(plan): move open tasks");
    git(root, &["switch", "-q", f.names.task_branch]);
    f.merge_in("local");
    f.complete();
    assert_refused(
        &f.bind_at(tip),
        &["not on the first-parent line of the target tip"],
    );
}

/// AC-2: the task branch's own commit making the same edit after the
/// review refuses.
#[test]
fn the_task_branchs_own_move_refuses() {
    let f = Fixture::new(PLAIN);
    f.write(TASK, &moved(&f.read(TASK), "EPC-002"));
    let own = f.commit("docs(plan): move open tasks");
    f.complete();
    assert_refused(&f.bind(), &[&format!("commit {own}")]);
}

/// AC-2: two commits on the line changing the record are not one planning
/// move, even when each looks like one.
#[test]
fn two_landed_changes_refuse() {
    let f = Fixture::new(PLAIN);
    let first = f.land_planning(|text| moved(text, "EPC-002"));
    git(f.root(), &["branch", "-qD", PLAIN.planning_branch]);
    let second = f.land_planning(|text| {
        text.replace("- Edges: none.\n", "- Edges: none.\n- A second note.\n")
    });
    f.merge_in(LINE);
    f.complete();
    assert_refused(
        &f.bind(),
        &[
            &first.to_string(),
            &second.to_string(),
            "only one landed planning move",
        ],
    );
}

/// AC-2: history the check cannot read refuses: a record blob the landed
/// move holds is missing from the object store.
#[test]
fn unreadable_history_refuses() {
    let (f, merge, _) = regrouped("EPC-002", |text| text);
    let repo = Repository::open(f.root()).unwrap();
    let blob = repo
        .find_commit(merge)
        .unwrap()
        .tree()
        .unwrap()
        .get_path(std::path::Path::new(TASK))
        .unwrap()
        .id();
    let hex = blob.to_string();
    std::fs::remove_file(repo.path().join("objects").join(&hex[..2]).join(&hex[2..])).unwrap();
    let bound = f.bind();
    assert!(bound.proof.is_none(), "{}", messages(&bound));
    assert!(!bound.findings.is_empty());
}

/// AC-2: a history overlay (a replace ref) refuses.
#[test]
fn a_history_overlay_refuses() {
    let (f, merge, _) = regrouped("EPC-002", |text| text);
    git(
        f.root(),
        &["replace", &merge.to_string(), &f.reviewed.to_string()],
    );
    let bound = f.bind();
    assert!(bound.proof.is_none(), "{}", messages(&bound));
    assert!(!bound.findings.is_empty());
}

/// A target tip whose first-parent line never reaches the reviewed
/// commit's history is an error, never an empty set of changes.
#[test]
fn a_target_line_outside_the_reviewed_history_is_an_error() {
    let f = Fixture::new(PLAIN);
    git(f.root(), &["switch", "-q", "--orphan", "elsewhere"]);
    f.write("other.txt", "other\n");
    let orphan = f.commit("other");
    let repo = Repository::open(f.root()).unwrap();
    let tips = RunBases::new([orphan]).stacking();
    let result = target_changes(&repo, TASK, f.reviewed, f.reviewed, &tips);
    assert!(
        result
            .as_ref()
            .is_err_and(|error| error.contains("never reaches")),
        "{:?}",
        result.map(|changes| changes.len())
    );
}

/// AC-3: branch names, the commit message and dates do not decide. The
/// same edit is admitted under names that mimic authority, and refused
/// when the line name holds it but the run's tip does not.
#[test]
fn names_messages_and_dates_do_not_decide() {
    for names in [
        Names {
            task_branch: "integration/EPC-001-line-copy",
            planning_branch: "task/TSK-001-work-reviewed",
            message: "chore: TSK-001 reviewed and approved by the operator",
            date: "2001-01-01T00:00:00",
        },
        Names {
            task_branch: "fix/anything",
            planning_branch: "main",
            message: "fix: unrelated",
            date: "2090-12-31T23:59:59",
        },
    ] {
        let f = Fixture::new(names);
        let before = f.tip(LINE);
        f.land_planning(|text| moved(text, "EPC-002"));
        f.merge_in(LINE);
        f.complete();
        let bound = f.bind();
        assert!(bound.findings.is_empty(), "{}", messages(&bound));
        // A run judged against the line's earlier tip: the branch named
        // like the line holds the move, but the run's tip does not.
        assert_refused(
            &f.bind_at(before),
            &["not on the first-parent line of the target tip"],
        );
    }
}

/// The shape reader: the admitted edit and its parts.
#[test]
fn the_shape_reads_the_move_and_its_added_lines() {
    let before = record("EPC-001", "todo");
    let after = moved(&before, "EPC-002");
    let found = shape(&before, &after, Scope::Whole).unwrap();
    assert_eq!(
        (found.from.as_str(), found.to.as_str(), found.added),
        ("EPC-001", "EPC-002", vec![BULLET.to_string()])
    );
    // Added lines alone, with no move, are no planning move.
    let noted = before.replace("- Edges: none.\n", &format!("- Edges: none.\n{BULLET}\n"));
    assert!(
        shape(&before, &noted, Scope::Whole).is_err_and(|why| why.contains("epic_id is unchanged"))
    );
    // A dropped front matter field is a change of the front matter.
    let dropped = after.replacen("status: todo             # todo | complete\n", "", 1);
    assert!(shape(&before, &dropped, Scope::Whole).is_err());
}
