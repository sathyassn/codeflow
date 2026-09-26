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

fn spec(id: &str, status: &str, extra: &str) -> String {
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
            acceptance: Some(block(&["AC-1", "AC-2"], "none | no journey criterion")),
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
        acceptance: Some(block(&["AC-1", "AC-2"], "none | no journey criterion")),
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
    assert!(warnings.contains("approved spec whose consumers are all accepted"));

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
        acceptance: Some(block(&["AC-1", "AC-2"], "none | no journey criterion")),
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
