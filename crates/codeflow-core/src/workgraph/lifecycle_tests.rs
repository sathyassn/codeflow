//! Lifecycle tests: every verb paired with the same change made by hand,
//! one failing fixture per missing element, the epic close and spec rules,
//! the migration baseline and the stale-word warnings (TSK-102).

use std::fs;
use std::path::Path;
use std::process::Command;

use super::*;
use crate::workgraph::status_verb::{set_status, StatusChange, VerbError};

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Self {
        let repo = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.root())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// `block` reviewed at the current head, as a completion must be
    /// (TSK-105).
    fn reviewed(&self, block: &str) -> String {
        block.replace(&"a".repeat(40), &self.git(&["rev-parse", "HEAD"]))
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.root().join(relative)).unwrap()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn judge(&self, base: &str) -> Verdict {
        judge_range(self.root(), base, None).unwrap()
    }

    /// Add a bare `origin` and return it (keep it alive for the test).
    fn origin(&self) -> tempfile::TempDir {
        let bare = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(bare.path())
            .status()
            .unwrap();
        assert!(status.success());
        self.git(&[
            "remote",
            "add",
            "origin",
            &bare.path().display().to_string(),
        ]);
        bare
    }

    fn push(&self, branch: &str) {
        self.git(&["push", "-q", "origin", branch]);
    }

    fn set_baselines(&self, commits: &[&str]) {
        let list = commits
            .iter()
            .map(|commit| format!("\"{commit}\""))
            .collect::<Vec<_>>()
            .join(", ");
        self.write(
            ".codeflow/project.toml",
            &format!("{BASELINE_KEY} = [{list}]\n"),
        );
    }

    fn set_baseline(&self, commit: &str) {
        self.write(
            ".codeflow/project.toml",
            &format!("{BASELINE_KEY} = \"{commit}\"\n"),
        );
    }
}

const TASK_PATH: &str = "project-management/tasks/TSK-001.md";
const EPIC_PATH: &str = "project-management/epics/EPC-001.md";

fn task(id: &str, status: &str, criteria: &str, closeout: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\ntitle: \"work\"\n\
status: {status}             # todo | blocked | in_progress | complete | cancelled\n\
work_type: feat\nspecs: []\ndepends_on: []\nintegration_target: main\n---\n\n\
# {id}\n\n## Description\n\nText.\n\n## Acceptance Criteria\n\n{criteria}\n\n\
## Closeout\n\n{closeout}\n"
    )
}

const CRITERIA: &str = "- AC-1 When x happens, the system shall y.\n\
- AC-2 When z happens, the system shall w (serves EPC-001 AC-1)";

fn epic(status: &str, specs: &str, criteria: &str) -> String {
    format!(
        "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: {status}\nwork_type: feat\nspecs: [{specs}]\n---\n\n\
# EPC-001\n\n## Summary\n\nText.\n\n## Acceptance Criteria\n\n{criteria}\n"
    )
}

/// A spec with an empty `open_questions` list unless `extra` sets it.
fn spec(id: &str, status: &str, extra: &str) -> String {
    let questions = if extra.contains("open_questions") {
        ""
    } else {
        "open_questions: []\n"
    };
    legacy_spec(id, status, &format!("{questions}{extra}"))
}

/// A spec as written before the `open_questions` field existed.
fn legacy_spec(id: &str, status: &str, extra: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: \"contract\"\nstatus: {status}\n{extra}---\n\n\
# {id}\n\n## Summary\n\nS.\n\n## Behavior\n\nB.\n\n## Open questions\n\nNone.\n"
    )
}

fn block(criteria: &[&str], journey: &str) -> String {
    let mut lines = vec![
        "acceptance:".to_string(),
        format!("  reviewed: {}", "a".repeat(40)),
        "  review: session:abc@sha256:00".to_string(),
        "  criteria:".to_string(),
    ];
    lines.extend(
        criteria
            .iter()
            .map(|id| format!("    {id}: verified | cargo test {id}")),
    );
    lines.push(format!("  journey: {journey}"));
    lines.push("  not_verified: none".to_string());
    lines.push("  follow_ups: none: nothing left".to_string());
    lines.push("  verdict: approved".to_string());
    lines.join("\n") + "\n"
}

fn fenced(inner: &str) -> String {
    format!("```yaml\n{inner}```")
}

/// A committed project with one open epic and one todo task.
fn project() -> (Repo, String) {
    let repo = Repo::new();
    repo.write(
        EPIC_PATH,
        &epic("planning", "", "- AC-1 When used, the system shall work."),
    );
    repo.write(TASK_PATH, &task("TSK-001", "todo", CRITERIA, "Pending."));
    let base = repo.commit("plan");
    (repo, base)
}

fn change(target: &str) -> StatusChange {
    StatusChange {
        target: target.to_string(),
        ..StatusChange::default()
    }
}

fn blocked_change() -> StatusChange {
    StatusChange {
        reason: Some("waiting for the registry".into()),
        owner: Some("primary".into()),
        revisit: Some("TSK-101 lands".into()),
        ..change("blocked")
    }
}

/// Run a verb, then make the identical change by hand from the same base,
/// and require one verdict for both.
fn verb_and_hand_agree(
    repo: &Repo,
    path: &str,
    verb: impl FnOnce() -> Result<(), VerbError>,
) -> String {
    let base = repo.commit("base");
    verb().unwrap();
    let written = repo.read(path);
    let by_verb = repo.judge(&base);
    repo.git(&["checkout", "--", path]);
    repo.write(path, &written);
    let by_hand = repo.judge(&base);
    assert_eq!(by_verb, by_hand, "a verb and a hand edit share the verdict");
    assert!(by_hand.is_clean(), "{by_hand:?}");
    written
}

// ---------------------------------------------------------------------------
// AC-1: verbs write only what the transition needs; hand edits share the verdict
// ---------------------------------------------------------------------------

#[test]
fn every_task_verb_matches_the_same_hand_edit() {
    let (repo, _) = project();
    let root = repo.root().to_path_buf();
    let blocked = verb_and_hand_agree(&repo, TASK_PATH, || {
        set_status(&root, RecordKind::Task, "TSK-001", &blocked_change()).map(drop)
    });
    assert!(blocked.contains(
        "## Blocker\n\n- reason: waiting for the registry\n- owner: primary\n- revisit: TSK-101 lands\n\n## Closeout"
    ));
    let unblocked = verb_and_hand_agree(&repo, TASK_PATH, || {
        set_status(&root, RecordKind::Task, "TSK-001", &change("todo")).map(drop)
    });
    assert!(!unblocked.contains("## Blocker"));
    assert_eq!(unblocked, task("TSK-001", "todo", CRITERIA, "Pending."));

    let completed = verb_and_hand_agree(&repo, TASK_PATH, || {
        let finish = StatusChange {
            acceptance: Some(
                repo.reviewed(&block(&["AC-1", "AC-2"], "none | no journey criterion")),
            ),
            ..change("complete")
        };
        set_status(&root, RecordKind::Task, "TSK-001", &finish).map(drop)
    });
    assert!(completed.contains("status: complete         # todo"));
    let reopened = verb_and_hand_agree(&repo, TASK_PATH, || {
        let reopen = StatusChange {
            reason: Some("regression found".into()),
            ..change("todo")
        };
        set_status(&root, RecordKind::Task, "TSK-001", &reopen).map(drop)
    });
    assert!(reopened.contains("acceptance_superseded:\n  reason: regression found\n  reviewed:"));
    let cancelled = verb_and_hand_agree(&repo, TASK_PATH, || {
        let cancel = StatusChange {
            reason: Some("replaced".into()),
            scope: Some("moved to TSK-009".into()),
            ..change("cancelled")
        };
        set_status(&root, RecordKind::Task, "TSK-001", &cancel).map(drop)
    });
    assert!(cancelled.contains("- cancelled: replaced\n- scope: moved to TSK-009\n"));
}

#[test]
fn the_verb_changes_only_status_and_the_needed_section() {
    let (repo, _) = project();
    let before = repo.read(TASK_PATH);
    set_status(repo.root(), RecordKind::Task, "TSK-001", &blocked_change()).unwrap();
    let after = repo.read(TASK_PATH);
    let expected = before
        .replace(
            "status: todo             # todo",
            "status: blocked          # todo",
        )
        .replace(
            "## Closeout",
            "## Blocker\n\n- reason: waiting for the registry\n- owner: primary\n- revisit: TSK-101 lands\n\n## Closeout",
        );
    assert_eq!(after, expected);
}

#[test]
fn a_verb_refuses_exactly_what_the_hand_edit_is_refused_for() {
    let (repo, base) = project();
    let refused = set_status(
        repo.root(),
        RecordKind::Task,
        "TSK-001",
        &change("complete"),
    );
    let Err(VerbError::Refused(verb_errors)) = refused else {
        panic!("expected a refusal, got {refused:?}");
    };
    assert_eq!(
        repo.read(TASK_PATH),
        task("TSK-001", "todo", CRITERIA, "Pending.")
    );
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", CRITERIA, "Pending."),
    );
    let by_hand = repo.judge(&base);
    assert_eq!(by_hand.errors, verb_errors);
    assert!(verb_errors[0].contains("a complete record needs an acceptance block"));
}

#[test]
fn no_verb_writes_in_progress_and_a_hand_edit_to_it_is_refused() {
    let (repo, base) = project();
    assert!(matches!(
        set_status(
            repo.root(),
            RecordKind::Task,
            "TSK-001",
            &change("in_progress")
        ),
        Err(VerbError::Vocabulary { .. })
    ));
    assert!(matches!(
        set_status(
            repo.root(),
            RecordKind::Spec,
            "SPC-001",
            &change("implemented")
        ),
        Err(VerbError::Vocabulary { .. })
    ));
    repo.write(
        TASK_PATH,
        &task("TSK-001", "in_progress", CRITERIA, "Pending."),
    );
    let verdict = repo.judge(&base);
    assert!(verdict
        .errors
        .iter()
        .any(|e| e.contains("in_progress is no longer written")));
}

// ---------------------------------------------------------------------------
// AC-2: one failing fixture per missing element, judged on the range
// ---------------------------------------------------------------------------

#[test]
fn each_missing_element_fails_the_pull_request_range() {
    let blocker = |fields: &str| format!("## Blocker\n\n{fields}\n\n## Closeout\n\nPending.");
    let cases: Vec<(&str, String, &str)> = vec![
        ("blocked", "Pending.".to_string(), "needs a `## Blocker`"),
        (
            "blocked",
            blocker("- owner: primary\n- revisit: TSK-101 lands"),
            "missing reason",
        ),
        (
            "blocked",
            blocker("- reason: wait\n- revisit: TSK-101 lands"),
            "missing owner",
        ),
        (
            "blocked",
            blocker("- reason: wait\n- owner: primary"),
            "missing revisit",
        ),
        (
            "cancelled",
            "- scope: moved to TSK-009".to_string(),
            "missing cancelled",
        ),
        (
            "cancelled",
            "- cancelled: replaced".to_string(),
            "missing scope",
        ),
        (
            "complete",
            "Pending.".to_string(),
            "needs an acceptance block",
        ),
        (
            "complete",
            fenced("acceptance:\n  reviewed: x\n"),
            "acceptance block line",
        ),
    ];
    for (status, closeout, needle) in cases {
        let (repo, base) = project();
        repo.git(&["switch", "-q", "-c", "task/TSK-001-work"]);
        let content = if closeout.starts_with("## Blocker") {
            task("TSK-001", status, CRITERIA, "Pending.")
                .replace("## Closeout\n\nPending.", &closeout)
        } else {
            task("TSK-001", status, CRITERIA, &closeout)
        };
        repo.write(TASK_PATH, &content);
        repo.commit("change status by hand");
        let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
        assert!(
            verdict.errors.iter().any(|error| error.contains(needle)),
            "{status}/{needle}: {verdict:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-3: reopen, partial and stacked pull requests
// ---------------------------------------------------------------------------

fn completed_project() -> (Repo, String) {
    let (repo, _) = project();
    let done = fenced(&block(&["AC-1", "AC-2"], "none | no journey criterion"));
    repo.write(TASK_PATH, &task("TSK-001", "complete", CRITERIA, &done));
    let base = repo.commit("complete");
    (repo, base)
}

#[test]
fn reopening_needs_a_reason_and_keeps_the_old_block_superseded() {
    let (repo, base) = completed_project();
    let refused = set_status(repo.root(), RecordKind::Task, "TSK-001", &change("todo"));
    assert!(matches!(&refused, Err(VerbError::Refused(errors)) if errors[0].contains("--reason")));

    let dropped = task("TSK-001", "todo", CRITERIA, "Pending.");
    repo.write(TASK_PATH, &dropped);
    let verdict = repo.judge(&base);
    assert!(
        verdict
            .errors
            .iter()
            .any(|e| e.contains("acceptance_superseded")),
        "{verdict:?}"
    );

    let kept_active = repo
        .read(TASK_PATH)
        .replace("Pending.", &fenced(&block(&["AC-1", "AC-2"], "none | n/a")));
    repo.write(TASK_PATH, &kept_active);
    assert!(!repo.judge(&base).is_clean());

    repo.git(&["checkout", "--", TASK_PATH]);
    let reopen = StatusChange {
        reason: Some("regression".into()),
        ..change("todo")
    };
    set_status(repo.root(), RecordKind::Task, "TSK-001", &reopen).unwrap();
    let reopened = RecordView::parse(RecordKind::Task, TASK_PATH, &repo.read(TASK_PATH)).unwrap();
    assert_eq!(reopened.status, "todo");
    let superseded = reopened.superseded_blocks();
    assert_eq!(superseded.len(), 1);
    let kept = superseded[0].parsed.clone().unwrap();
    assert_eq!(kept.reason.as_deref(), Some("regression"));
    assert_eq!(kept.criteria.len(), 2);
    assert!(repo.judge(&base).is_clean());
}

#[test]
fn partial_and_stacked_slices_leave_the_task_open_until_the_last() {
    let (repo, base) = project();
    repo.git(&["switch", "-q", "-c", "task/TSK-001-slice-one"]);
    repo.write("src/one.rs", "// slice one\n");
    repo.write(
        TASK_PATH,
        &task(
            "TSK-001",
            "todo",
            CRITERIA,
            "Slice one landed; the task stays open.",
        ),
    );
    let slice_one = repo.commit("feat: slice one");
    let first = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(first.is_clean(), "{first:?}");
    let record = RecordView::parse(RecordKind::Task, TASK_PATH, &repo.read(TASK_PATH)).unwrap();
    assert_eq!(record.status, "todo", "a partial PR leaves the task todo");

    repo.git(&["switch", "-q", "-c", "task/TSK-001-slice-two"]);
    repo.write("src/two.rs", "// slice two\n");
    repo.commit("feat: slice two");
    let stacked = judge_range(repo.root(), &slice_one, Some("HEAD")).unwrap();
    assert!(stacked.is_clean());

    let finish = StatusChange {
        acceptance: Some(repo.reviewed(&block(&["AC-1", "AC-2"], "none | no journey criterion"))),
        ..change("complete")
    };
    set_status(repo.root(), RecordKind::Task, "TSK-001", &finish).unwrap();
    repo.commit("chore: complete TSK-001");
    let last = judge_range(repo.root(), &slice_one, Some("HEAD")).unwrap();
    assert!(last.is_clean(), "{last:?}");
}

// ---------------------------------------------------------------------------
// AC-4: epic close
// ---------------------------------------------------------------------------

fn close_epic(repo: &Repo) -> Result<(), VerbError> {
    set_status(
        repo.root(),
        RecordKind::Epic,
        "EPC-001",
        &change("complete"),
    )
    .map(drop)
}

const EPIC_CRITERION: &str = "- AC-1 When used, the system shall work.";

/// Close the epic with its own acceptance block verifying AC-1.
fn close_epic_with_block(repo: &Repo) -> Result<(), VerbError> {
    let close = StatusChange {
        acceptance: Some(block(&["AC-1"], "none | n/a")),
        ..change("complete")
    };
    set_status(repo.root(), RecordKind::Epic, "EPC-001", &close).map(drop)
}

fn refusal(result: Result<(), VerbError>) -> String {
    match result {
        Err(VerbError::Refused(errors)) => errors.join("\n"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn epic_close_needs_every_task_terminal() {
    let (repo, _) = project();
    assert!(refusal(close_epic(&repo)).contains("every task terminal; open: TSK-001"));
}

#[test]
fn an_epic_criterion_served_only_by_cancelled_tasks_is_unverified() {
    let (repo, _) = project();
    let cancelled = task(
        "TSK-001",
        "cancelled",
        CRITERIA,
        "- cancelled: replaced\n- scope: dropped",
    );
    repo.write(TASK_PATH, &cancelled);
    repo.commit("cancel");
    assert!(refusal(close_epic(&repo)).contains("served only by cancelled tasks"));

    let done = fenced(&block(&["AC-1", "AC-2"], "none | n/a"));
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "complete", CRITERIA, &done),
    );
    repo.commit("a complete serving task");
    close_epic(&repo).unwrap();
}

#[test]
fn a_journey_criterion_is_verified_only_on_the_journey() {
    let repo = Repo::new();
    repo.write(
        EPIC_PATH,
        &epic(
            "planning",
            "",
            "- AC-1 When run on a fresh init, the system shall work (journey)",
        ),
    );
    let criteria = "- AC-1 When run, the system shall work (serves EPC-001 AC-1)";
    let unrun = fenced(&block(&["AC-1"], "none | not run"));
    repo.write(TASK_PATH, &task("TSK-001", "complete", criteria, &unrun));
    repo.commit("plan");
    assert!(refusal(close_epic(&repo)).contains("no complete serving task verified it"));
    let run = fenced(&block(&["AC-1"], "verified | fresh init"));
    repo.write(TASK_PATH, &task("TSK-001", "complete", criteria, &run));
    repo.commit("journey run");
    close_epic(&repo).unwrap();
}

#[test]
fn epic_close_counts_the_closing_epic_as_the_last_spec_consumer() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", ""),
    );
    repo.write(
        "project-management/specs/SPC-002.md",
        &spec("SPC-002", "draft", ""),
    );
    repo.write(EPIC_PATH, &epic("planning", "SPC-001", EPIC_CRITERION));
    repo.commit("plan");
    close_epic_with_block(&repo).unwrap();
    assert_eq!(
        spec_state(&Graph::from_worktree(repo.root()), "SPC-001"),
        Some(SpecState::Implemented)
    );

    repo.write(EPIC_PATH, &epic("planning", "SPC-002", EPIC_CRITERION));
    repo.commit("consume a draft spec");
    assert!(refusal(close_epic_with_block(&repo)).contains("SPC-002 would not be implemented"));

    let open_consumer = task(
        "TSK-001",
        "todo",
        "- AC-1 When x, the system shall y.",
        "Pending.",
    )
    .replace("specs: []", "specs: [SPC-003]")
    .replace(
        "epic_id: EPC-001\nstandalone_reason: null",
        "epic_id: null\nstandalone_reason: \"one PR\"",
    );
    repo.write(
        "project-management/specs/SPC-003.md",
        &spec("SPC-003", "approved", ""),
    );
    repo.write(EPIC_PATH, &epic("planning", "SPC-003", EPIC_CRITERION));
    repo.write(TASK_PATH, &open_consumer);
    repo.commit("a spec still consumed by an open task");
    close_epic_with_block(&repo).unwrap();
}

#[test]
fn epic_terminal_acts_follow_their_transitions() {
    let (repo, base) = project();
    let cancel = StatusChange {
        reason: Some("superseded by EPC-002".into()),
        scope: Some("moved to EPC-002".into()),
        ..change("cancelled")
    };
    set_status(repo.root(), RecordKind::Epic, "EPC-001", &cancel).unwrap();
    assert!(repo.judge(&base).is_clean());
    set_status(
        repo.root(),
        RecordKind::Epic,
        "EPC-001",
        &change("archived"),
    )
    .unwrap();
    let archived = repo.commit("archive");
    repo.write(
        EPIC_PATH,
        &repo
            .read(EPIC_PATH)
            .replace("status: archived", "status: planning"),
    );
    assert!(repo.judge(&archived).errors[0].contains("an archived epic is final"));
}

// ---------------------------------------------------------------------------
// AC-5: spec lifecycle and the derived implemented state
// ---------------------------------------------------------------------------

fn consumer(id: &str, status: &str, spec_id: &str) -> String {
    let closeout = match status {
        "complete" => fenced(&block(&["AC-1"], "none | n/a")),
        "cancelled" => "- cancelled: dropped\n- scope: none left".to_string(),
        _ => "Pending.".to_string(),
    };
    task(id, status, "- AC-1 When x, the system shall y.", &closeout)
        .replace("specs: []", &format!("specs: [{spec_id}]"))
        .replace(
            "epic_id: EPC-001\nstandalone_reason: null",
            "epic_id: null\nstandalone_reason: \"one PR\"",
        )
}

#[test]
fn spec_state_is_derived_from_its_consumers() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", ""),
    );
    let graph = || Graph::from_worktree(repo.root());
    assert_eq!(
        spec_state(&graph(), "SPC-001"),
        Some(SpecState::NoDeliveringConsumer)
    );
    assert!(validate_lifecycle(repo.root())
        .warnings
        .iter()
        .any(|w| w.contains("no delivering consumer")));

    repo.write(TASK_PATH, &consumer("TSK-001", "todo", "SPC-001"));
    assert_eq!(spec_state(&graph(), "SPC-001"), Some(SpecState::Open));
    repo.write(TASK_PATH, &consumer("TSK-001", "complete", "SPC-001"));
    assert_eq!(
        spec_state(&graph(), "SPC-001"),
        Some(SpecState::Implemented),
        "one final consumer completes the spec"
    );
    repo.write(
        "project-management/tasks/TSK-002.md",
        &consumer("TSK-002", "cancelled", "SPC-001"),
    );
    assert_eq!(
        spec_state(&graph(), "SPC-001"),
        Some(SpecState::Implemented),
        "mixed complete and cancelled consumers"
    );
    repo.write(TASK_PATH, &consumer("TSK-001", "cancelled", "SPC-001"));
    assert_eq!(
        spec_state(&graph(), "SPC-001"),
        Some(SpecState::NoDeliveringConsumer)
    );
}

#[test]
fn a_spec_is_superseded_only_by_a_revision_that_lists_it() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "draft", ""),
    );
    let base = repo.commit("draft");
    set_status(
        repo.root(),
        RecordKind::Spec,
        "SPC-001",
        &change("approved"),
    )
    .unwrap();
    assert!(repo.judge(&base).is_clean());
    let approved = repo.commit("approve");

    let by = StatusChange {
        by: Some("SPC-002".into()),
        ..change("superseded")
    };
    let missing = refusal(set_status(repo.root(), RecordKind::Spec, "SPC-001", &by).map(drop));
    assert!(
        missing.contains("superseded_by names missing spec SPC-002"),
        "{missing}"
    );

    repo.write(
        "project-management/specs/SPC-002.md",
        &spec("SPC-002", "draft", "supersedes: [SPC-001]\n"),
    );
    set_status(repo.root(), RecordKind::Spec, "SPC-001", &by).unwrap();
    let old = repo.read("project-management/specs/SPC-001.md");
    assert!(old.contains("status: superseded") && old.contains("superseded_by: SPC-002"));
    assert!(
        repo.judge(&approved).is_clean(),
        "{:?}",
        repo.judge(&approved)
    );

    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "draft", ""),
    );
    let back = repo.judge(&approved);
    assert!(back
        .errors
        .iter()
        .any(|e| e.contains("approved never returns to draft")));
}

#[test]
fn approving_a_spec_reads_open_questions_from_the_structured_field() {
    let repo = Repo::new();
    let open = "open_questions:\n  - \"Which channel is authoritative?\"\n";
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "draft", open),
    );
    let base = repo.commit("draft with an open question");
    let refused = refusal(
        set_status(
            repo.root(),
            RecordKind::Spec,
            "SPC-001",
            &change("approved"),
        )
        .map(drop),
    );
    assert!(
        refused.contains("still open: Which channel is authoritative?"),
        "{refused}"
    );

    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", "open_questions: not a list\n"),
    );
    assert!(repo
        .judge(&base)
        .errors
        .iter()
        .any(|e| e.contains("open_questions must be a list")));

    // The prose section is no longer parsed: a question mark there is
    // context, and an empty list is what approval reads.
    let prose = spec("SPC-001", "approved", "open_questions: []\n")
        .replace("None.\n", "Was this settled? Yes, see Decisions.\n");
    repo.write("project-management/specs/SPC-001.md", &prose);
    assert!(repo.judge(&base).is_clean(), "{:?}", repo.judge(&base));
}

/// TSK-135 review R3: a draft written before the field existed, whose prose
/// still asks a question, is never approved on a missing field; nor is one
/// whose list is null. Adding the list migrates it: the question stays open
/// until it is resolved and the list is empty. An approved spec without the
/// field stays readable.
#[test]
fn approving_needs_the_field_and_a_legacy_approval_stays_readable() {
    let path = "project-management/specs/SPC-001.md";
    let asking =
        |text: String| text.replace("None.\n", "Which recovery channel is authoritative?\n");
    for (what, spec_text, expected) in [
        (
            "missing",
            asking(legacy_spec("SPC-001", "draft", "")),
            "needs its `open_questions` frontmatter list",
        ),
        (
            "null",
            asking(legacy_spec("SPC-001", "draft", "open_questions: null\n")),
            "open_questions must be a list",
        ),
    ] {
        let repo = Repo::new();
        repo.write(path, &spec_text);
        repo.commit("a draft");
        let refused = refusal(
            set_status(
                repo.root(),
                RecordKind::Spec,
                "SPC-001",
                &change("approved"),
            )
            .map(drop),
        );
        assert!(refused.contains(expected), "{what}: {refused}");
        assert!(repo.read(path).contains("status: draft"), "{what}");
    }

    // The one-line migration: the unresolved question moves into the list
    // and blocks; once resolved, the empty list approves.
    let repo = Repo::new();
    repo.write(path, &asking(legacy_spec("SPC-001", "draft", "")));
    repo.commit("a legacy draft");
    let migrated = |questions: &str| asking(legacy_spec("SPC-001", "draft", questions));
    repo.write(
        path,
        &migrated("open_questions: [\"Which recovery channel is authoritative?\"]\n"),
    );
    repo.commit("migrate the open question");
    let refused = refusal(
        set_status(
            repo.root(),
            RecordKind::Spec,
            "SPC-001",
            &change("approved"),
        )
        .map(drop),
    );
    assert!(
        refused.contains("still open: Which recovery channel"),
        "{refused}"
    );
    repo.write(path, &migrated("open_questions: []\n"));
    repo.commit("resolve the question");
    set_status(
        repo.root(),
        RecordKind::Spec,
        "SPC-001",
        &change("approved"),
    )
    .unwrap();

    // Read compatibility: an approved spec without the field, unchanged or
    // edited without a status change, is not refused.
    let repo = Repo::new();
    repo.write(path, &legacy_spec("SPC-001", "approved", ""));
    let base = repo.commit("an approved legacy spec");
    assert!(validate_lifecycle(repo.root()).is_clean());
    repo.write(
        path,
        &legacy_spec("SPC-001", "approved", "").replace("B.\n", "B, clarified.\n"),
    );
    assert!(repo.judge(&base).is_clean(), "{:?}", repo.judge(&base));
}

#[test]
fn completing_against_a_superseded_spec_is_refused() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "superseded", "superseded_by: SPC-002\n"),
    );
    repo.write(
        "project-management/specs/SPC-002.md",
        &spec("SPC-002", "approved", "supersedes: [SPC-001]\n"),
    );
    repo.write(TASK_PATH, &consumer("TSK-001", "todo", "SPC-001"));
    repo.commit("plan");
    let finish = StatusChange {
        acceptance: Some(block(&["AC-1"], "none | n/a")),
        ..change("complete")
    };
    let refused = refusal(set_status(repo.root(), RecordKind::Task, "TSK-001", &finish).map(drop));
    assert!(refused.contains("cannot cite superseded spec SPC-001; move the task to SPC-002"));
}

#[test]
fn a_hand_written_implemented_on_a_new_spec_is_refused() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", ""),
    );
    let base = repo.commit("approved");
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "implemented", ""),
    );
    let range = repo.judge(&base);
    assert!(range
        .errors
        .iter()
        .any(|e| e.contains("implemented is derived")));
    assert!(validate_lifecycle(repo.root())
        .errors
        .iter()
        .any(|e| e.contains("refused as a written value")));
}

// ---------------------------------------------------------------------------
// AC-6: new records list criteria without checkboxes; legacy reads as before
// ---------------------------------------------------------------------------

#[test]
fn new_records_must_not_use_checkboxes_but_baseline_records_may() {
    let repo = Repo::new();
    let legacy = task("TSK-001", "todo", "- [ ] first\n- [x] second", "Pending.");
    repo.write(TASK_PATH, &legacy);
    let baseline = repo.commit("legacy");
    assert!(validate_lifecycle(repo.root())
        .errors
        .iter()
        .any(|e| e.contains("without a checkbox")));
    repo.set_baseline(&baseline);
    assert!(validate_lifecycle(repo.root()).is_clean());
    let malformed = task("TSK-002", "todo", "- AC-1 one\n- plain bullet", "Pending.");
    repo.write("project-management/tasks/TSK-002.md", &malformed);
    assert!(validate_lifecycle(repo.root())
        .errors
        .iter()
        .any(|e| e.contains("TSK-002.md") && e.contains("is not a criterion")));
}

// ---------------------------------------------------------------------------
// AC-7: migration baseline and transitions
// ---------------------------------------------------------------------------

/// A complete task written under the old rules, at the baseline.
fn baseline_project() -> (Repo, String) {
    let repo = Repo::new();
    repo.write(EPIC_PATH, &epic("in_progress", "", "- [x] AC-1 legacy"));
    repo.write(
        TASK_PATH,
        &task(
            "TSK-001",
            "complete",
            "- [x] first\n- [x] second",
            "Done long ago.",
        ),
    );
    let baseline = repo.commit("old rules");
    repo.set_baseline(&baseline);
    repo.commit("record the baseline");
    (repo, baseline)
}

#[test]
fn an_unchanged_baseline_blob_is_exempt_and_a_spelling_edit_only_warns() {
    let (repo, baseline) = baseline_project();
    assert!(validate_lifecycle(repo.root()).is_clean());
    repo.write(
        TASK_PATH,
        &repo
            .read(TASK_PATH)
            .replace("Done long ago.", "Done long ago, spelling."),
    );
    let tree = validate_lifecycle(repo.root());
    assert!(tree.is_clean(), "{tree:?}");
    assert!(tree
        .warnings
        .iter()
        .any(|w| w.contains("needs an acceptance block")));
    let range = repo.judge(&baseline);
    assert!(range.is_clean() && !range.warnings.is_empty(), "{range:?}");
}

/// TSK-109: the backfill adds only the registered `uid` (R-25), so the
/// record keeps its baseline exception (R-3, R-83); any other edit on top
/// of the backfill still warns, and a changed status is still refused.
#[test]
fn a_backfilled_uid_keeps_the_baseline_exemption_and_nothing_else_does() {
    let (repo, baseline) = baseline_project();
    let backfilled = repo.read(TASK_PATH).replacen(
        "id: TSK-001\n",
        "id: TSK-001\nuid: 0e273f9f-5e55-4bf0-9b24-2698eeeca620\n",
        1,
    );
    assert_ne!(backfilled, repo.read(TASK_PATH), "the uid line was added");
    repo.write(TASK_PATH, &backfilled);
    let tree = validate_lifecycle(repo.root());
    assert!(tree.is_clean() && tree.warnings.is_empty(), "{tree:?}");
    let range = repo.judge(&baseline);
    assert!(range.is_clean() && range.warnings.is_empty(), "{range:?}");

    // A spelling edit on top of the backfill is an edit: it warns.
    repo.write(
        TASK_PATH,
        &backfilled.replace("Done long ago.", "Done long ago, spelling."),
    );
    let tree = validate_lifecycle(repo.root());
    assert!(
        tree.is_clean()
            && tree
                .warnings
                .iter()
                .any(|w| w.contains("needs an acceptance block")),
        "{tree:?}"
    );
    // A uid line in the body is not a backfill.
    repo.write(
        TASK_PATH,
        &format!("{}\nuid: in the body\n", repo.read(TASK_PATH)),
    );
    let body = repo
        .read(TASK_PATH)
        .replace("uid: 0e273f9f-5e55-4bf0-9b24-2698eeeca620\n", "");
    repo.write(
        TASK_PATH,
        &body.replace("Done long ago, spelling.", "Done long ago."),
    );
    assert!(validate_lifecycle(repo.root())
        .warnings
        .iter()
        .any(|w| w.contains("needs an acceptance block")));
    // A status change beside the uid is a transition, judged in full.
    repo.write(
        TASK_PATH,
        &backfilled.replace("status: complete", "status: cancelled"),
    );
    assert!(!validate_lifecycle(repo.root()).is_clean());
}

/// A task completed before the baseline whose evidence is gone takes the
/// historical form of R-101 in place of an acceptance block; nothing else
/// does, and a transition or criteria change still needs a block (R-83).
#[test]
fn a_pre_baseline_completion_accepts_the_historical_form_and_nothing_less() {
    const HISTORICAL: &str = "Done long ago.\n\n- acceptance: historical evidence unavailable; landed\n  by merge `dcbc3d837` (PR 466); reviews as recorded above.";
    let with_closeout = |repo: &Repo, closeout: &str| {
        let content = repo.read(TASK_PATH);
        let (head, _) = content.split_once("## Closeout").unwrap();
        repo.write(TASK_PATH, &format!("{head}## Closeout\n\n{closeout}\n"));
    };
    let (repo, baseline) = baseline_project();
    with_closeout(&repo, HISTORICAL);
    let tree = validate_lifecycle(repo.root());
    assert!(tree.is_clean() && tree.warnings.is_empty(), "{tree:?}");
    let range = repo.judge(&baseline);
    assert!(range.is_clean() && range.warnings.is_empty(), "{range:?}");

    for incomplete in [
        "Done long ago.\n\n- acceptance: historical evidence unavailable; PR 466.",
        "Done long ago.\n\n- acceptance: see merge `dcbc3d837`.",
        "Done long ago.\n\n- acceptance: historical evidence unavailable; merge `xyz`.",
        "Done long ago. historical evidence unavailable; merge `dcbc3d837`.",
    ] {
        with_closeout(&repo, incomplete);
        let tree = validate_lifecycle(repo.root());
        assert!(
            tree.is_clean()
                && tree
                    .warnings
                    .iter()
                    .any(|w| w.contains("- acceptance: historical evidence unavailable")),
            "{incomplete}: {tree:?}"
        );
    }

    // A criteria change is judged in full: the historical form is not enough.
    with_closeout(&repo, HISTORICAL);
    repo.write(
        TASK_PATH,
        &repo
            .read(TASK_PATH)
            .replace("- [x] second", "- [x] second, reworded"),
    );
    assert!(!validate_lifecycle(repo.root()).is_clean());
    assert!(!repo.judge(&baseline).is_clean());

    // A task completed after the baseline never takes the historical form.
    let (repo, _) = baseline_project();
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "complete", CRITERIA, HISTORICAL),
    );
    assert!(validate_lifecycle(repo.root())
        .errors
        .iter()
        .any(|e| e.contains("TSK-002") && e.contains("needs an acceptance block")));
}

/// Fable's F4 probe (TSK-109 review): a reopen and a re-completion after
/// the baseline, in one range or seen only at the tree, never take the
/// historical form; the range is judged per commit, so a middle commit that
/// reopens without its `- reopened:` line is still seen (R-83).
#[test]
fn a_reopen_and_recompletion_after_the_baseline_never_take_the_historical_form() {
    const REOPENED: &str = "Done long ago.\n\n- reopened: a gap was found";
    const RECOMPLETED: &str = "Done long ago.\n\n- reopened: a gap was found\n- acceptance: historical evidence unavailable; landed by merge `dcbc3d837`.";
    let criteria = "- [x] first\n- [x] second";
    let (repo, baseline) = baseline_project();
    repo.write(TASK_PATH, &task("TSK-001", "todo", criteria, REOPENED));
    let reopened = repo.commit("reopen");
    let reopen_verdict = repo.judge(&baseline);
    assert!(
        reopen_verdict.is_clean(),
        "reopen itself: {reopen_verdict:?}"
    );
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", criteria, RECOMPLETED),
    );
    let refused = |verdict: &Verdict| {
        verdict
            .errors
            .iter()
            .any(|e| e.contains("needs an acceptance block"))
    };
    assert!(refused(&repo.judge(&reopened)), "per-commit range");
    assert!(refused(&repo.judge(&baseline)), "endpoint range");
    assert!(refused(&validate_lifecycle(repo.root())), "tree");

    // No reopen line: only the per-commit walk of the range sees the reopen.
    let (repo, baseline) = baseline_project();
    let historical =
        "Done long ago.\n\n- acceptance: historical evidence unavailable; landed by merge `dcbc3d837`.";
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", criteria, "Done long ago."),
    );
    repo.commit("reopen by hand without a reason");
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", criteria, historical),
    );
    repo.commit("complete again on the historical form");
    assert!(
        refused(&repo.judge(&baseline)),
        "{:?}",
        repo.judge(&baseline)
    );
}

#[test]
fn reopening_changing_criteria_or_completing_again_applies_the_new_rules() {
    let (repo, _) = baseline_project();
    let changed_criteria = repo
        .read(TASK_PATH)
        .replace("- [x] second", "- [x] second, reworded");
    repo.write(TASK_PATH, &changed_criteria);
    assert!(validate_lifecycle(repo.root())
        .errors
        .iter()
        .any(|e| e.contains("needs an acceptance block")));

    let (repo, _) = baseline_project();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [x] first\n- [x] second", "Reopened."),
    );
    let reopened = repo.commit("reopen by hand");
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", "- [x] first\n- [x] second", "Again."),
    );
    let again = repo.judge(&reopened);
    assert!(
        again
            .errors
            .iter()
            .any(|e| e.contains("needs an acceptance block")),
        "{again:?}"
    );
}

#[test]
fn the_exception_binds_to_the_blob_and_relationship_errors_are_never_grandfathered() {
    let (repo, baseline) = baseline_project();
    // Same id, different bytes: not the exempt blob.
    repo.write(
        TASK_PATH,
        &repo
            .read(TASK_PATH)
            .replace("status: complete", "status: cancelled"),
    );
    assert!(!validate_lifecycle(repo.root()).is_clean());
    repo.git(&["checkout", "--", TASK_PATH]);

    // A supersession link that was already broken at the baseline still fails.
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", "superseded_by: SPC-009\n"),
    );
    let with_spec = repo.commit("broken link");
    repo.set_baseline(&with_spec);
    let tree = validate_lifecycle(repo.root());
    assert!(tree
        .errors
        .iter()
        .any(|e| e.contains("superseded_by names missing spec SPC-009")));
    assert_ne!(baseline, with_spec);
}

#[test]
fn a_baseline_missing_from_history_downgrades_to_warnings() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", CRITERIA, "No block."),
    );
    repo.commit("records");
    repo.set_baseline(&"b".repeat(40));
    let tree = validate_lifecycle(repo.root());
    assert!(tree.is_clean(), "{tree:?}");
    assert!(tree
        .warnings
        .iter()
        .any(|w| w.contains("not in this clone's history")));
    assert!(tree
        .warnings
        .iter()
        .any(|w| w.contains("needs an acceptance block")));
}

// ---------------------------------------------------------------------------
// AC-9: stale words warn
// ---------------------------------------------------------------------------

#[test]
fn stale_words_warn() {
    let repo = Repo::new();
    let done = fenced(&block(&["AC-1", "AC-2"], "none | n/a"));
    repo.write(
        EPIC_PATH,
        &epic("planning", "", "- AC-1 When used, the system shall work."),
    );
    repo.write(TASK_PATH, &task("TSK-001", "complete", CRITERIA, &done));
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "in_progress", CRITERIA, "Pending."),
    );
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", ""),
    );
    repo.write(
        "project-management/tasks/TSK-003.md",
        &consumer("TSK-003", "complete", "SPC-001"),
    );
    repo.commit("records");
    let warnings = validate_lifecycle(repo.root()).warnings.join("\n");
    assert!(
        warnings.contains("planning epic has complete tasks (TSK-001)"),
        "{warnings}"
    );
    assert!(warnings.contains("in_progress with no active branch carrying TSK-002"));
    assert!(
        !warnings.contains("SPC-001"),
        "an approved spec whose consumers are done is healthy: {warnings}"
    );

    repo.git(&["branch", "task/TSK-002-work"]);
    repo.git(&["switch", "-q", "task/TSK-002-work"]);
    repo.commit("work on the branch");
    repo.git(&["switch", "-q", "main"]);
    let warnings = validate_lifecycle(repo.root()).warnings.join("\n");
    assert!(
        !warnings.contains("TSK-002"),
        "an active branch clears the warning: {warnings}"
    );
}

// ---------------------------------------------------------------------------
// Mutation-driven edges of the transition rules
// ---------------------------------------------------------------------------

#[test]
fn a_second_reopen_must_supersede_the_new_block_too() {
    let (repo, _) = completed_project();
    let reopen = StatusChange {
        reason: Some("first regression".into()),
        ..change("todo")
    };
    set_status(repo.root(), RecordKind::Task, "TSK-001", &reopen).unwrap();
    let finish = StatusChange {
        acceptance: Some(repo.reviewed(&block(&["AC-1", "AC-2"], "none | no journey criterion"))),
        ..change("complete")
    };
    set_status(repo.root(), RecordKind::Task, "TSK-001", &finish).unwrap();
    let completed_again = repo.commit("complete again");
    // Dropping the new block keeps only the first superseded block: refused.
    let content = repo.read(TASK_PATH);
    let active = content.rfind("```yaml\nacceptance:\n").unwrap();
    let after_block = content[active..].split_once("```\n").unwrap().1;
    let dropped =
        format!("{}{after_block}", &content[..active]).replace("status: complete", "status: todo");
    repo.write(TASK_PATH, &dropped);
    let verdict = repo.judge(&completed_again);
    assert!(
        verdict
            .errors
            .iter()
            .any(|e| e.contains("acceptance_superseded")),
        "{verdict:?}\n{dropped}"
    );
}

#[test]
fn an_unreadable_baseline_treats_no_record_as_new() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy criterion", "Pending."),
    );
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "implemented", ""),
    );
    repo.commit("records");
    repo.set_baseline(&"c".repeat(40));
    let warnings = validate_lifecycle(repo.root()).warnings.join("\n");
    assert!(!warnings.contains("without a checkbox"), "{warnings}");
    assert!(
        !warnings.contains("refused as a written value"),
        "{warnings}"
    );
}

/// An approved spec whose consumers are all done is the healthy state:
/// `implemented` is derived and never written (R-32, R-51), so nothing
/// warns; the states that disagree with the consumers still do.
#[test]
fn an_approved_spec_with_done_consumers_is_healthy_and_real_mismatches_warn() {
    const SPEC_PATH: &str = "project-management/specs/SPC-001.md";
    let repo = Repo::new();
    repo.write(SPEC_PATH, &spec("SPC-001", "approved", ""));
    repo.write(TASK_PATH, &consumer("TSK-001", "complete", "SPC-001"));
    repo.write(
        "project-management/tasks/TSK-002.md",
        &consumer("TSK-002", "cancelled", "SPC-001"),
    );
    let base = repo.commit("approved spec, consumers done");
    assert_eq!(
        spec_state(&Graph::from_worktree(repo.root()), "SPC-001"),
        Some(SpecState::Implemented)
    );
    let healthy = validate_lifecycle(repo.root());
    assert!(
        healthy.is_clean() && healthy.warnings.is_empty(),
        "{healthy:?}"
    );

    // Every consumer cancelled: no delivering consumer still warns.
    repo.write(TASK_PATH, &consumer("TSK-001", "cancelled", "SPC-001"));
    assert!(validate_lifecycle(repo.root())
        .warnings
        .iter()
        .any(|w| w.contains("no delivering consumer")));

    // A written `implemented` with an open consumer is refused as ever.
    repo.write(TASK_PATH, &consumer("TSK-001", "todo", "SPC-001"));
    repo.write(SPEC_PATH, &spec("SPC-001", "implemented", ""));
    assert!(!repo.judge(&base).is_clean());
}

#[test]
fn a_spec_without_a_complete_consumer_is_not_implemented() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "approved", ""),
    );
    repo.write(EPIC_PATH, &epic("archived", "SPC-001", EPIC_CRITERION));
    repo.write(TASK_PATH, &consumer("TSK-001", "cancelled", "SPC-001"));
    assert_eq!(
        spec_state(&Graph::from_worktree(repo.root()), "SPC-001"),
        Some(SpecState::Open)
    );
}

#[test]
fn epic_close_refuses_a_spec_whose_other_consumer_is_terminal_and_incomplete() {
    let repo = Repo::new();
    repo.write(
        "project-management/specs/SPC-001.md",
        &spec("SPC-001", "draft", ""),
    );
    repo.write(EPIC_PATH, &epic("planning", "SPC-001", EPIC_CRITERION));
    repo.write(TASK_PATH, &consumer("TSK-001", "cancelled", "SPC-001"));
    repo.commit("plan");
    assert!(refusal(close_epic_with_block(&repo)).contains("no other open consumer"));
}

// ---------------------------------------------------------------------------
// Review round 1 regressions (R1 to R7), each built from the reviewer's probe
// ---------------------------------------------------------------------------

/// R1 and round 2: a block inside an HTML comment, an enclosing example
/// fence or a raw HTML block (`<pre>`, `<script>`) never completes a task;
/// the same block, visible, does, even after a code span holding `<!--`.
#[test]
fn hidden_or_example_acceptance_does_not_complete_a_task() {
    let visible = fenced(&block(&["AC-1", "AC-2"], "none | no journey criterion"));
    for (closeout, hidden) in [
        (format!("<!--\n{visible}\n-->"), true),
        (format!("````text\n{visible}\n````"), true),
        (format!("~~~\n{visible}\n~~~"), true),
        (format!("<pre>\n{visible}\n</pre>"), true),
        (
            format!("<script type=\"text/plain\">\n{visible}\n</script>"),
            true,
        ),
        (format!("Text <!-- note --> kept.\n\n{visible}"), false),
        (
            format!("The scanner handles `<!--` markers.\n\n{visible}"),
            false,
        ),
        (visible.clone(), false),
    ] {
        let (repo, base) = project();
        repo.write(TASK_PATH, &task("TSK-001", "complete", CRITERIA, &closeout));
        repo.commit("complete by hand");
        let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
        if hidden {
            assert!(
                verdict
                    .errors
                    .iter()
                    .any(|e| e.contains("needs an acceptance block")),
                "{closeout}: {verdict:?}"
            );
        } else {
            assert!(verdict.is_clean(), "{closeout}: {verdict:?}");
        }
    }
}

/// Round 3: text that reads `## Closeout` inside inline HTML is a paragraph,
/// not the Closeout heading, so the fence after it proves nothing.
#[test]
fn an_inline_tag_heading_is_not_the_closeout() {
    let visible = fenced(&block(&["AC-1", "AC-2"], "none | no journey criterion"));
    for (heading, clean) in [
        ("<span hidden>## Closeout</span>", false),
        ("## Closeout <!-- note -->", true),
        ("## Closeout", true),
    ] {
        let (repo, base) = project();
        let content =
            task("TSK-001", "complete", CRITERIA, &visible).replace("## Closeout", heading);
        repo.write(TASK_PATH, &content);
        repo.commit("complete by hand");
        let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
        assert_eq!(verdict.is_clean(), clean, "{heading}: {verdict:?}");
        if !clean {
            assert!(
                verdict
                    .errors
                    .iter()
                    .any(|e| e.contains("needs an acceptance block")),
                "{verdict:?}"
            );
        }
    }
}

/// R1: a Blocker section inside a comment does not satisfy a blocked task.
#[test]
fn a_commented_blocker_does_not_block_a_task() {
    let section = "## Blocker\n- reason: unavailable\n- owner: reviewer\n- revisit: later\n";
    for (wrapped, hidden) in [
        (format!("<!--\n{section}-->\n\n## Closeout"), true),
        (format!("<!-- guidance -->\n{section}\n## Closeout"), false),
    ] {
        let (repo, base) = project();
        let content =
            task("TSK-001", "blocked", CRITERIA, "Pending.").replace("## Closeout", &wrapped);
        repo.write(TASK_PATH, &content);
        let verdict = repo.judge(&base);
        assert_eq!(
            verdict
                .errors
                .iter()
                .any(|e| e.contains("needs a `## Blocker`")),
            hidden,
            "{content}: {verdict:?}"
        );
        assert_eq!(verdict.is_clean(), !hidden, "{verdict:?}");
    }
}

fn epic_with_closeout(status: &str, criteria: &str, closeout: &str) -> String {
    format!(
        "{}\n## Closeout\n\n{closeout}\n",
        epic(status, "", criteria)
    )
}

/// R2: the epic's own block passes the same structural rules as a task's.
#[test]
fn an_epic_closes_only_on_a_valid_own_block() {
    let invalid = block(&["AC-1"], "none | n/a")
        .replace(&"a".repeat(40), "invalid")
        .replace("verdict: approved", "verdict: rejected");
    let two = format!(
        "{}\n\n{}",
        fenced(&block(&["AC-1"], "none | n/a")),
        fenced(&block(&["AC-1"], "none | n/a"))
    );
    let cases = [
        (fenced(&invalid), Some("acceptance verdict is `rejected`")),
        (fenced(&invalid), Some("full commit sha")),
        (two, Some("one acceptance block per completion")),
        (fenced(&block(&["AC-1"], "none | n/a")), None),
    ];
    for (closeout, needle) in cases {
        let repo = Repo::new();
        repo.write(EPIC_PATH, &epic("planning", "", EPIC_CRITERION));
        let base = repo.commit("plan");
        repo.write(
            EPIC_PATH,
            &epic_with_closeout("complete", EPIC_CRITERION, &closeout),
        );
        let verdict = repo.judge(&base);
        match needle {
            Some(needle) => assert!(
                verdict.errors.iter().any(|e| e.contains(needle)),
                "{needle}: {verdict:?}"
            ),
            None => assert!(verdict.is_clean(), "{verdict:?}"),
        }
    }
}

/// R2: ticking a legacy journey checkbox is not journey evidence; an
/// ordinary legacy checkbox still reads as before.
#[test]
fn a_ticked_legacy_journey_checkbox_does_not_close_an_epic() {
    let ticked = "- [x] AC-1 Installed CLI works (journey)";
    let journey = fenced(&block(
        &["AC-1"],
        "verified | installed CLI on a fresh init",
    ));
    for (criteria, closeout, clean) in [
        (ticked, String::new(), false),
        (ticked, journey.clone(), true),
        (
            "- [x] AC-1 An ordinary legacy criterion",
            String::new(),
            true,
        ),
    ] {
        let repo = Repo::new();
        let before = criteria.replace("[x]", "[ ]");
        repo.write(EPIC_PATH, &epic("planning", "", &before));
        let baseline = repo.commit("legacy epic");
        repo.set_baseline(&baseline);
        let base = repo.commit("record the baseline");
        repo.write(
            EPIC_PATH,
            &epic_with_closeout("complete", criteria, &closeout),
        );
        let verdict = repo.judge(&base);
        assert_eq!(verdict.is_clean(), clean, "{criteria}: {verdict:?}");
        if !clean {
            assert!(
                verdict
                    .errors
                    .iter()
                    .any(|e| e.contains("verifying the journey")),
                "{verdict:?}"
            );
        }
    }
}

const SPC_1: &str = "project-management/specs/SPC-001.md";
const SPC_2: &str = "project-management/specs/SPC-002.md";

/// R3: a spec never supersedes itself, supersession never cycles, and the
/// successor is a new revision added by the same change.
#[test]
fn supersession_needs_a_distinct_new_successor() {
    // Self-supersession, as the probe wrote it.
    let repo = Repo::new();
    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    let base = repo.commit("approved");
    repo.write(
        SPC_1,
        &spec(
            "SPC-001",
            "superseded",
            "supersedes: [SPC-001]\nsuperseded_by: SPC-001\n",
        ),
    );
    let errors = repo.judge(&base).errors.join("\n");
    assert!(errors.contains("cannot supersede itself"), "{errors}");
    assert!(
        errors.contains("cannot be superseded by itself"),
        "{errors}"
    );

    // A cycle between two specs that both existed before.
    let repo = Repo::new();
    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    repo.write(SPC_2, &spec("SPC-002", "approved", ""));
    let base = repo.commit("approved");
    repo.write(
        SPC_1,
        &spec(
            "SPC-001",
            "superseded",
            "supersedes: [SPC-002]\nsuperseded_by: SPC-002\n",
        ),
    );
    repo.write(
        SPC_2,
        &spec(
            "SPC-002",
            "superseded",
            "supersedes: [SPC-001]\nsuperseded_by: SPC-001\n",
        ),
    );
    let errors = repo.judge(&base).errors.join("\n");
    assert!(errors.contains("supersession cannot cycle"), "{errors}");
    assert!(
        errors.contains("already existed before this change"),
        "{errors}"
    );

    // A successor that existed before the change, by verb and by hand.
    let repo = Repo::new();
    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    repo.write(SPC_2, &spec("SPC-002", "draft", ""));
    let base = repo.commit("both exist");
    repo.write(SPC_2, &spec("SPC-002", "draft", "supersedes: [SPC-001]\n"));
    repo.commit("list the old spec");
    let by = StatusChange {
        by: Some("SPC-002".into()),
        ..change("superseded")
    };
    let refused = refusal(set_status(repo.root(), RecordKind::Spec, "SPC-001", &by).map(drop));
    assert!(refused.contains("SPC-002 already existed"), "{refused}");
    repo.write(
        SPC_1,
        &spec("SPC-001", "superseded", "superseded_by: SPC-002\n"),
    );
    let errors = repo.judge(&base).errors.join("\n");
    assert!(errors.contains("SPC-002 already existed"), "{errors}");
}

/// R4: approval travels only in a planning-only change, for the verb, the
/// working-tree range and the committed pull request range.
#[test]
fn spec_approval_travels_only_in_a_planning_only_change() {
    let repo = Repo::new();
    repo.write(SPC_1, &spec("SPC-001", "draft", ""));
    let base = repo.commit("draft");
    repo.git(&["switch", "-q", "-c", "feat/product"]);
    repo.write("product.rs", "fn product() {}\n");
    let refused = refusal(
        set_status(
            repo.root(),
            RecordKind::Spec,
            "SPC-001",
            &change("approved"),
        )
        .map(drop),
    );
    assert!(refused.contains("planning-only change"), "{refused}");
    assert!(refused.contains("product.rs"), "{refused}");
    assert!(repo.read(SPC_1).contains("status: draft"));

    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    let by_hand = repo.judge(&base);
    assert!(
        by_hand.errors.iter().any(|e| e.contains("planning-only")),
        "{by_hand:?}"
    );
    repo.commit("approve in a product range");
    let range = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(
        range.errors.iter().any(|e| e.contains("planning-only")),
        "{range:?}"
    );

    // Control: records and plans only.
    let repo = Repo::new();
    repo.write(SPC_1, &spec("SPC-001", "draft", ""));
    let base = repo.commit("draft");
    repo.git(&["switch", "-q", "-c", "plan/approve-spc-001"]);
    repo.write("docs/plan/notes.md", "Approval notes.\n");
    set_status(
        repo.root(),
        RecordKind::Spec,
        "SPC-001",
        &change("approved"),
    )
    .unwrap();
    repo.commit("approve in a planning range");
    let range = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(range.is_clean(), "{range:?}");
}

/// R5: an unchanged complete task whose consumed spec is superseded in the
/// same range no longer verifies the epic criterion it serves.
#[test]
fn acceptance_citing_a_superseded_spec_cannot_close_an_epic() {
    let serving = "- AC-1 Work correctly (serves EPC-001 AC-1)";
    let done = fenced(&block(&["AC-1"], "none | n/a"));
    let completed =
        task("TSK-001", "complete", serving, &done).replace("specs: []", "specs: [SPC-001]");
    for supersede in [true, false] {
        let repo = Repo::new();
        repo.write(EPIC_PATH, &epic("planning", "", EPIC_CRITERION));
        repo.write(TASK_PATH, &completed);
        repo.write(SPC_1, &spec("SPC-001", "approved", ""));
        let baseline = repo.commit("delivered");
        repo.set_baseline(&baseline);
        let base = repo.commit("record the baseline");
        if supersede {
            repo.write(
                SPC_1,
                &spec("SPC-001", "superseded", "superseded_by: SPC-002\n"),
            );
            repo.write(SPC_2, &spec("SPC-002", "draft", "supersedes: [SPC-001]\n"));
        }
        repo.write(EPIC_PATH, &epic("complete", "", EPIC_CRITERION));
        repo.commit("close the epic");
        let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
        if supersede {
            assert!(
                verdict
                    .errors
                    .iter()
                    .any(|e| e.contains("cites a superseded spec")),
                "{verdict:?}"
            );
        } else {
            assert!(verdict.is_clean(), "{verdict:?}");
        }
    }
}

/// R6: a task completed before the migration (ticked criteria, an ordinary
/// Closeout, no block) reopens through the verb with a reason; no block is
/// invented, and the hand edit shares the verdict.
#[test]
fn a_task_completed_before_the_migration_reopens_with_a_reason() {
    let (repo, _) = baseline_project();
    let base = repo.commit("before reopening");
    let root = repo.root().to_path_buf();
    let reopened = verb_and_hand_agree(&repo, TASK_PATH, || {
        let reopen = StatusChange {
            reason: Some("new requirement".into()),
            ..change("todo")
        };
        set_status(&root, RecordKind::Task, "TSK-001", &reopen).map(drop)
    });
    assert!(reopened.contains("Done long ago.\n\n- reopened: new requirement\n"));
    assert!(!reopened.contains("acceptance"));

    repo.write(
        TASK_PATH,
        &task(
            "TSK-001",
            "todo",
            "- [x] first\n- [x] second",
            "Done long ago.",
        ),
    );
    let without_line = repo.judge(&base);
    assert!(
        without_line
            .errors
            .iter()
            .any(|e| e.contains("`- reopened: <reason>`")),
        "{without_line:?}"
    );
}

/// R7: a record the range adds is new even when the migration baseline is
/// outside the clone's history, and the missing history is reported. A
/// real shallow clone and its full-history source agree.
#[test]
fn a_record_the_range_adds_is_new_in_a_shallow_clone() {
    let source = Repo::new();
    source.write(EPIC_PATH, &epic("planning", "", EPIC_CRITERION));
    let baseline = source.commit("records");
    source.set_baseline(&baseline);
    source.commit("record the baseline");
    let base = source.commit("establish the migration");
    source.write(
        TASK_PATH,
        &task(
            "TSK-001",
            "todo",
            "- [ ] AC-1 New criterion with a checkbox",
            "Pending.",
        ),
    );
    source.commit("add a task");

    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.root().display());
    let status = Command::new("git")
        .args(["clone", "--quiet", "--depth", "2", &url])
        .arg(clone.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(matches!(
        Baseline::load(clone.path()),
        Baseline::Unavailable(_)
    ));

    for (root, shallow) in [(clone.path(), true), (source.root(), false)] {
        let verdict = judge_range(root, &base, Some("HEAD")).unwrap();
        assert!(
            verdict
                .errors
                .iter()
                .any(|e| e.contains("without a checkbox")),
            "shallow={shallow}: {verdict:?}"
        );
        assert_eq!(
            verdict
                .warnings
                .iter()
                .any(|w| w.contains("not in this clone's history")),
            shallow,
            "{verdict:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// A range whose base predates the migration baseline (release rehearsal)
// ---------------------------------------------------------------------------

/// The one release pull request has a base older than the baseline. Records
/// the base lacks are judged from their baseline blob: unchanged ones stay
/// legacy, an edit after the baseline is a transition from the baseline
/// state, and a record absent from both is new and strict.
#[test]
fn a_base_older_than_the_baseline_judges_records_from_the_baseline() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "// before the records\n");
    let base = repo.commit("old main");
    repo.git(&["switch", "-q", "-c", "integration/EPC-001-line"]);
    repo.write(EPIC_PATH, &epic("in_progress", "", "- [x] AC-1 legacy"));
    let legacy = task("TSK-001", "complete", "- [x] first", "Done long ago.");
    repo.write(TASK_PATH, &legacy);
    repo.write(
        "project-management/tasks/TSK-004.md",
        &task("TSK-004", "todo", "- [ ] AC-1 checkbox", "Pending."),
    );
    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    let baseline = repo.commit("records under the old rules");
    repo.git(&["switch", "-q", "-c", "integration/release"]);
    repo.set_baseline(&baseline);
    repo.commit("record the baseline");

    // After the baseline: TSK-004 completes without a block, TSK-002 is new
    // with a checkbox, TSK-003 is new and valid, and product code changes.
    repo.write(
        "project-management/tasks/TSK-004.md",
        &task("TSK-004", "complete", "- [x] AC-1 checkbox", "Pending."),
    );
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "todo", "- [ ] AC-1 new checkbox", "Pending."),
    );
    repo.write(
        "project-management/tasks/TSK-003.md",
        &task(
            "TSK-003",
            "todo",
            "- AC-1 When x, the system shall y.",
            "Pending.",
        ),
    );
    repo.write("src/lib.rs", "// the release\n");
    repo.commit("work after the baseline");

    let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    let errors = verdict.errors.join("\n");
    assert!(
        errors.contains("TSK-002.md: new records list criteria"),
        "{errors}"
    );
    assert!(
        errors.contains("TSK-004.md: a complete record needs an acceptance block"),
        "{errors}"
    );
    assert!(
        !errors.contains("TSK-004.md: new records"),
        "an edit after the baseline is a transition, not a new record: {errors}"
    );
    for untouched in ["TSK-001", "SPC-001", "EPC-001", "TSK-003"] {
        assert!(!errors.contains(untouched), "{untouched}: {errors}");
    }
    assert_eq!(verdict.errors.len(), 2, "{errors}");

    // The same range with the baseline removed judges every record new.
    repo.write(".codeflow/project.toml", "");
    repo.commit("drop the baseline");
    let without = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(
        without.errors.iter().any(|e| e.contains("TSK-001.md")),
        "{without:?}"
    );
}

/// A spec approved at the baseline is no transition in a later code range,
/// so the range passes; a new approval beside code is still refused.
#[test]
fn a_spec_approved_at_the_baseline_passes_in_a_code_range() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "// old\n");
    let base = repo.commit("old main");
    repo.write(SPC_1, &spec("SPC-001", "draft", ""));
    repo.commit("draft");
    repo.write(SPC_1, &spec("SPC-001", "approved", ""));
    let baseline = repo.commit("approved in a planning change");
    repo.git(&["switch", "-q", "-c", "feat/code"]);
    repo.set_baseline(&baseline);
    repo.write("src/lib.rs", "// code\n");
    repo.commit("code and the baseline");
    let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(verdict.is_clean(), "{verdict:?}");

    // Control: an approval made after the baseline in the same code range.
    repo.write(
        "project-management/specs/SPC-002.md",
        &spec("SPC-002", "approved", ""),
    );
    repo.commit("approve a new spec beside code");
    let refused = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(
        refused
            .errors
            .iter()
            .any(|e| e.contains("SPC-002.md") && e.contains("planning-only")),
        "{refused:?}"
    );
}

/// A baseline that is not an ancestor of the judged commit is refused by
/// the range judge, the tree validator and the verbs.
#[test]
fn a_baseline_that_is_not_an_ancestor_is_refused() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    let base = repo.commit("records");
    repo.git(&["switch", "-q", "-c", "side"]);
    repo.write("elsewhere.txt", "x\n");
    let side = repo.commit("an unrelated commit");
    repo.git(&["switch", "-q", "main"]);
    repo.set_baseline(&side);
    repo.commit("point the baseline elsewhere");
    assert!(matches!(Baseline::load(repo.root()), Baseline::Refused(_)));
    let needle = "is not an ancestor of the commit being judged";
    let range = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(range.errors.iter().any(|e| e.contains(needle)), "{range:?}");
    let tree = validate_lifecycle(repo.root());
    assert!(tree.errors.iter().any(|e| e.contains(needle)), "{tree:?}");
    assert!(
        tree.errors.iter().any(|e| e.contains("without a checkbox")),
        "a refused baseline exempts nothing: {tree:?}"
    );
    let verb =
        refusal(set_status(repo.root(), RecordKind::Task, "TSK-001", &blocked_change()).map(drop));
    assert!(verb.contains(needle), "{verb}");

    // Control: the baseline at HEAD itself counts.
    let head = repo.git(&["rev-parse", "HEAD"]);
    repo.set_baseline(&head);
    assert!(matches!(
        Baseline::load(repo.root()),
        Baseline::Available { .. }
    ));
}

// ---------------------------------------------------------------------------
// One baseline per line of work, and changes to the baseline list
// ---------------------------------------------------------------------------

/// Two lines merged into one release, each with its own baseline: legacy
/// records of either line pass, a later edit is judged from its line's copy,
/// and a record in neither baseline is new and strict.
#[test]
fn two_merged_lines_are_each_judged_from_their_own_baseline() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "// old main\n");
    let base = repo.commit("old main");

    repo.git(&["switch", "-q", "-c", "integration/EPC-001-a"]);
    repo.write(EPIC_PATH, &epic("in_progress", "", "- [x] AC-1 legacy"));
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", "- [x] first", "Done on line a."),
    );
    repo.write(
        "project-management/tasks/TSK-004.md",
        &task("TSK-004", "todo", "- [ ] AC-1 checkbox", "Pending."),
    );
    let line_a = repo.commit("line a records");

    repo.git(&["switch", "-q", "-c", "integration/EPC-002-b", &base]);
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "complete", "- [x] first", "Done on line b.").replace(
            "epic_id: EPC-001\nstandalone_reason: null",
            "epic_id: null\nstandalone_reason: \"one PR\"",
        ),
    );
    let line_b = repo.commit("line b records");

    repo.git(&["switch", "-q", "-c", "integration/release", &base]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "merge line a",
        "integration/EPC-001-a",
    ]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "merge line b",
        "integration/EPC-002-b",
    ]);
    repo.set_baselines(&[&line_a, &line_b]);
    repo.write(
        "project-management/tasks/TSK-004.md",
        &task("TSK-004", "complete", "- [x] AC-1 checkbox", "Pending."),
    );
    repo.write(
        "project-management/tasks/TSK-003.md",
        &task("TSK-003", "todo", "- [ ] AC-1 new checkbox", "Pending."),
    );
    repo.write("src/lib.rs", "// the release\n");
    repo.commit("release work");

    assert!(matches!(
        Baseline::load(repo.root()),
        Baseline::Available { ref commits, .. } if commits.len() == 2
    ));
    let verdict = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    let errors = verdict.errors.join("\n");
    assert!(
        errors.contains("TSK-003.md: new records list criteria"),
        "{errors}"
    );
    assert!(
        errors.contains("TSK-004.md: a complete record needs an acceptance block"),
        "{errors}"
    );
    assert_eq!(verdict.errors.len(), 2, "{errors}");
    assert!(
        verdict
            .notices
            .iter()
            .any(|n| n.contains("introduces work_records_baseline")),
        "{verdict:?}"
    );

    // Control: with only line a's baseline, line b's legacy task is new.
    repo.set_baselines(&[&line_a]);
    repo.commit("only one baseline");
    let one = judge_range(repo.root(), &base, Some("HEAD")).unwrap();
    assert!(
        one.errors.iter().any(|e| e.contains("TSK-002.md")),
        "{one:?}"
    );
}

/// When the target already records a baseline, its list is the one that
/// exempts records: a change cannot exempt its own commit, whatever
/// branches that commit is pushed to, and the edit is only a notice until
/// it lands (BL-1).
#[test]
fn a_change_cannot_exempt_its_own_commit_once_the_target_has_a_baseline() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    let legacy = repo.commit("records");
    repo.set_baseline(&legacy);
    repo.commit("record the baseline");
    let _origin = repo.origin();
    repo.push("main");
    repo.git(&["switch", "-q", "-c", "integration/EPC-009-self"]);
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "complete", "- [x] first", "No block."),
    );
    let own = repo.commit("a record the change wants exempt");
    repo.set_baselines(&[&legacy, &own]);
    repo.commit("exempt it");
    repo.push("integration/EPC-009-self");
    // The same history under a sibling integration name.
    repo.git(&["push", "-q", "origin", "HEAD:integration/EPC-010-sibling"]);
    repo.git(&["fetch", "-q", "origin"]);

    let refused = |verdict: &Verdict| {
        verdict
            .errors
            .iter()
            .any(|e| e.contains("TSK-002.md") && e.contains("needs an acceptance block"))
    };
    let ci = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(refused(&ci), "{ci:?}");
    assert!(
        ci.notices
            .iter()
            .any(|n| n.contains(&format!("added {own}")) && n.contains("after it lands")),
        "{ci:?}"
    );
    assert!(!ci.errors.iter().any(|e| e.contains("TSK-001")), "{ci:?}");
    let since = judge_range(repo.root(), "main", None).unwrap();
    assert!(refused(&since), "{since:?}");

    // Once landed, the next change is judged by the new list.
    repo.git(&["switch", "-q", "main"]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "land",
        "integration/EPC-009-self",
    ]);
    repo.git(&["switch", "-q", "-c", "feat/after"]);
    repo.write("notes.txt", "later\n");
    repo.commit("later work");
    let later = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(later.is_clean() && later.notices.is_empty(), "{later:?}");
}

/// Entries are full commit ids read as object ids: a ref, tag, expression or
/// abbreviation is refused by name, so a snapshot cannot move while the
/// config stays the same (BL-2).
#[test]
fn baseline_entries_must_be_full_commit_ids() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    let base = repo.commit("records");
    repo.git(&["tag", "v1"]);
    let short = base[..9].to_string();
    let upper = base.to_uppercase();
    for entry in ["HEAD", "HEAD~1", "v1", short.as_str(), upper.as_str()] {
        repo.set_baseline(entry);
        let needle = format!("entry `{entry}` is not a full 40-character");
        let tree = validate_lifecycle(repo.root());
        assert!(
            tree.errors.iter().any(|e| e.contains(&needle)),
            "{entry}: {tree:?}"
        );
        assert!(
            tree.errors.iter().any(|e| e.contains("without a checkbox")),
            "a refused entry exempts nothing: {tree:?}"
        );
        let range = judge_range(repo.root(), &base, None).unwrap();
        assert!(
            range.errors.iter().any(|e| e.contains(&needle)),
            "{entry}: {range:?}"
        );
    }

    // A moved tag changes nothing: it was never accepted.
    repo.write("notes.txt", "x\n");
    repo.commit("move on");
    repo.git(&["tag", "-f", "v1"]);
    repo.set_baseline("v1");
    assert!(matches!(Baseline::load(repo.root()), Baseline::Refused(_)));

    // Control: the full id is accepted.
    repo.set_baseline(&base);
    assert!(validate_lifecycle(repo.root()).is_clean());
}

/// A ref whose name is another commit's id does not redirect an entry: the
/// entry resolves to its own object (BL-2, short-sha shadow).
#[test]
fn a_ref_named_like_a_commit_id_does_not_shadow_the_entry() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    let legacy = repo.commit("records");
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] shadow", "Pending."),
    );
    let shadow = repo.commit("a different snapshot");
    repo.git(&["tag", &legacy[..7], &shadow]);
    repo.git(&["update-ref", &format!("refs/tags/{legacy}"), &shadow]);
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    repo.set_baseline(&legacy);
    let tree = validate_lifecycle(repo.root());
    assert!(tree.is_clean(), "the entry is the legacy commit: {tree:?}");
    repo.set_baseline(&legacy[..7]);
    assert!(matches!(Baseline::load(repo.root()), Baseline::Refused(_)));
}

/// List order carries no meaning: of ancestor-related snapshots holding a
/// record, only the latest counts, so reversing the list keeps a
/// `cancelled -> todo` refusal (BL-3).
#[test]
fn baseline_order_does_not_change_the_verdict() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "// old main\n");
    let base = repo.commit("old main");
    repo.git(&["switch", "-q", "-c", "integration/EPC-001-line"]);
    repo.write(EPIC_PATH, &epic("in_progress", "", "- [x] AC-1 legacy"));
    repo.write(TASK_PATH, &task("TSK-001", "todo", "- [ ] first", "Open."));
    let older = repo.commit("todo snapshot");
    repo.write(
        TASK_PATH,
        &task("TSK-001", "cancelled", "- [ ] first", "Dropped."),
    );
    let newer = repo.commit("cancelled snapshot");
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] first", "Revived with new prose."),
    );
    let refused = "status cancelled -> todo is refused";
    let mut trees = Vec::new();
    for list in [[&older, &newer], [&newer, &older]] {
        repo.set_baselines(&[list[0], list[1]]);
        let range = judge_range(repo.root(), &base, None).unwrap();
        assert!(
            range.errors.iter().any(|e| e.contains(refused)),
            "{list:?}: {range:?}"
        );
        trees.push(format!("{:?}", validate_lifecycle(repo.root())));
    }
    assert_eq!(trees[0], trees[1]);

    // A record equal to any listed copy stays legacy.
    repo.write(TASK_PATH, &task("TSK-001", "todo", "- [ ] first", "Open."));
    for list in [[&older, &newer], [&newer, &older]] {
        repo.set_baselines(&[list[0], list[1]]);
        let range = judge_range(repo.root(), &base, None).unwrap();
        assert!(range.is_clean(), "{list:?}: {range:?}");
    }
}

/// A change whose target has no baseline introduces the migration: the
/// head's list governs, and every entry is named for the human reviewer.
#[test]
fn a_change_that_introduces_the_baseline_is_judged_by_its_own_list() {
    let repo = Repo::new();
    repo.write("src/lib.rs", "// old main\n");
    repo.commit("old main");
    repo.git(&["switch", "-q", "-c", "integration/EPC-001-a"]);
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", "- [x] first", "Done on line a."),
    );
    let line_a = repo.commit("line a records");
    repo.git(&["switch", "-q", "-c", "integration/EPC-002-b", "main"]);
    repo.write(EPIC_PATH, &epic("in_progress", "", "- [x] AC-1 legacy"));
    let line_b = repo.commit("line b records");
    repo.git(&["switch", "-q", "-c", "release", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "a", "integration/EPC-001-a"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "b", "integration/EPC-002-b"]);
    repo.set_baselines(&[&line_b, &line_a]);
    repo.commit("introduce the baseline");

    let ci = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(ci.errors.is_empty(), "{ci:?}");
    let notice = ci
        .notices
        .iter()
        .find(|n| n.contains("introduces work_records_baseline"))
        .unwrap_or_else(|| panic!("{ci:?}"));
    assert!(
        notice.contains(&line_a) && notice.contains(&line_b),
        "{notice}"
    );

    // An entry outside the head's history is refused.
    repo.git(&["switch", "-q", "-c", "stray", "main"]);
    repo.write("elsewhere.txt", "x\n");
    let stray = repo.commit("not in the release");
    repo.git(&["switch", "-q", "release"]);
    repo.set_baselines(&[&line_a, &line_b, &stray]);
    repo.commit("add a stray entry");
    let ci = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(
        ci.errors
            .iter()
            .any(|e| e.contains(&stray) && e.contains("not an ancestor")),
        "{ci:?}"
    );
}

/// A branch forked before its target adopted a baseline is judged by the
/// target's list, not by the list-less merge-base, so its own list cannot
/// exempt its own record; a target list the branch cannot contain is refused
/// rather than replaced by the branch's list (BL-R2-1).
#[test]
fn a_branch_forked_before_the_target_adopted_a_baseline_is_judged_by_the_target() {
    let repo = Repo::new();
    repo.write("README.md", "# fixture\n");
    let fork = repo.commit("seed");
    repo.set_baseline(&fork);
    repo.commit("adopt the baseline on the target");
    repo.git(&["switch", "-q", "-c", "fix/older-branch", &fork]);
    repo.write(
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "complete", "- [x] AC-1 first", "No block."),
    );
    let own = repo.commit("a record the branch wants exempt");
    repo.set_baseline(&own);
    repo.commit("exempt it");

    let refused = |verdict: &Verdict| {
        verdict
            .errors
            .iter()
            .any(|e| e.contains("TSK-002.md") && e.contains("needs an acceptance block"))
    };
    let ci = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(refused(&ci), "{ci:?}");
    assert!(
        !ci.notices.iter().any(|n| n.contains("introduces")),
        "{ci:?}"
    );
    assert!(
        ci.notices
            .iter()
            .any(|n| n.contains(&format!("added {own}"))),
        "{ci:?}"
    );
    let since = judge_range(repo.root(), "main", None).unwrap();
    assert!(refused(&since), "{since:?}");

    // The target adopts a list naming a commit this branch lacks.
    repo.git(&["switch", "-q", "main"]);
    repo.write("later.txt", "later\n");
    let later = repo.commit("later target work");
    repo.set_baseline(&later);
    repo.commit("move the target's baseline");
    repo.git(&["switch", "-q", "fix/older-branch"]);
    let needle = format!("the target's {BASELINE_KEY} entry {later} is not an ancestor");
    let ci = judge_pull_request(repo.root(), "main", "HEAD").unwrap();
    assert!(ci.errors.iter().any(|e| e.contains(&needle)), "{ci:?}");
    assert!(refused(&ci), "{ci:?}");
    assert!(
        !ci.notices.iter().any(|n| n.contains("introduces")),
        "{ci:?}"
    );
}

/// The single-string form reads as a one-item list, and repeats collapse.
#[test]
fn the_single_string_baseline_still_works() {
    let repo = Repo::new();
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", "- [ ] legacy", "Pending."),
    );
    let commit = repo.commit("records");
    repo.set_baseline(&commit);
    assert_eq!(recorded_baseline(repo.root()), vec![commit.clone()]);
    assert!(matches!(
        Baseline::load(repo.root()),
        Baseline::Available { .. }
    ));
    assert!(validate_lifecycle(repo.root()).is_clean());
    repo.set_baselines(&[&commit, &commit]);
    assert_eq!(recorded_baseline(repo.root()), vec![commit]);
    assert!(validate_lifecycle(repo.root()).is_clean());
}

#[test]
fn recompletion_preserves_the_anchored_acceptance_even_with_equal_active_block() {
    let (repo, _) = project();
    let old = repo.reviewed(&block(&["AC-1", "AC-2"], "verified | epic journey"));
    repo.write(
        TASK_PATH,
        &task("TSK-001", "complete", CRITERIA, &fenced(&old)),
    );
    let base = repo.commit("complete the task");
    let archive = old.replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: regression\n",
    );
    repo.write(
        TASK_PATH,
        &task("TSK-001", "todo", CRITERIA, &fenced(&archive)),
    );
    repo.commit("reopen the task");
    for fault in ["valid", "dropped", "edited", "reason", "criteria"] {
        let superseded = old.replace(
            "acceptance:\n",
            "acceptance_superseded:\n  reason: regression\n",
        );
        let superseded = match fault {
            "dropped" => String::new(),
            "edited" => superseded.replace("cargo test AC-1", "invented evidence"),
            "reason" => superseded.replace("  reason: regression\n", ""),
            _ => superseded,
        };
        let criteria = if fault == "criteria" {
            CRITERIA.replace("shall y", "may y")
        } else {
            CRITERIA.into()
        };
        repo.write(
            TASK_PATH,
            &task(
                "TSK-001",
                "complete",
                &criteria,
                &format!("{}\n{}", fenced(&superseded), fenced(&old)),
            ),
        );
        let verdict = repo.judge(&base);
        if fault == "valid" {
            assert!(verdict.is_clean(), "{:?}", verdict.errors);
        } else {
            assert!(!verdict.is_clean(), "{fault} must refuse the recompletion");
        }
    }
}
