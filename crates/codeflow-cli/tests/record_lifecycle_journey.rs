//! Journey (TSK-102 AC-11): on a fresh `codeflow init --full` project, every
//! record transition runs through the status verbs and through the same
//! change made by hand, and both get one verdict from `validate --since` and
//! `codeflow ci`. Partial and stacked pull requests, cancel and reopen are
//! part of the path. The binary under test is the one Cargo built; the
//! installed `codeflow` on `PATH` is never used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ok(out: &Output, what: &str) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{what} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

/// Errors `validate --docs --since base` reports, and its exit code.
fn verdict(root: &Path, base: &str) -> (Option<i32>, Vec<String>) {
    let out = codeflow(root, &["validate", "--docs", "--since", base]);
    let errors = String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|line| line.contains(": error: "))
        .map(str::to_owned)
        .collect();
    (out.status.code(), errors)
}

const TASK: &str = "project-management/tasks/TSK-001.md";
const SECOND: &str = "project-management/tasks/TSK-002.md";
const EPIC: &str = "project-management/epics/EPC-001.md";

/// Run a verb, then undo it and make the identical change by hand; both must
/// get the same clean verdict. The verb's change is kept and committed.
fn verb_then_hand(root: &Path, file: &str, args: &[&str], message: &str) -> String {
    let base = git(root, &["rev-parse", "HEAD"]);
    ok(&codeflow(root, args), &args.join(" "));
    let written = std::fs::read_to_string(root.join(file)).unwrap();
    let by_verb = verdict(root, &base);
    git(root, &["checkout", "--", file]);
    std::fs::write(root.join(file), &written).unwrap();
    let by_hand = verdict(root, &base);
    assert_eq!(
        by_verb, by_hand,
        "{args:?}: verb and hand edit share one verdict"
    );
    assert_eq!(by_hand.0, Some(0), "{args:?}: {:?}", by_hand.1);
    commit(root, message);
    written
}

fn acceptance(root: &Path, criteria: &[&str], journey: &str) -> PathBuf {
    let mut lines = vec![
        "acceptance:".to_string(),
        format!("  reviewed: {}", git(root, &["rev-parse", "HEAD"])),
        "  review: session:journey@sha256:00".to_string(),
        "  criteria:".to_string(),
    ];
    for id in criteria {
        lines.push(format!("    {id}: verified | journey step {id}"));
    }
    lines.push(format!("  journey: {journey}"));
    lines.push("  not_verified: none".to_string());
    lines.push("  follow_ups: none: journey fixture".to_string());
    lines.push("  verdict: approved".to_string());
    let path = root.join("../acceptance.yaml");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

#[test]
#[allow(clippy::too_many_lines)] // One journey keeps every transition in the order a project lives it.
fn every_transition_runs_through_verbs_and_hand_edits_with_one_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let target = "integration/EPC-001-journey";
    git(&root, &["switch", "-q", "-c", target]);

    // Plan on a planning branch: an epic, a spec, two tasks.
    git(&root, &["switch", "-q", "-c", "plan/lifecycle"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    ok(
        &codeflow(&root, &["spec", "new", "--for", "EPC-001", "contract"]),
        "spec new",
    );
    for title in ["first", "second"] {
        let args = ["task", "new", "--epic", "EPC-001", "--into", target, title];
        ok(&codeflow(&root, &args), "task new");
    }
    let listed = "- AC-1\n";
    edit(
        &root,
        EPIC,
        listed,
        "- AC-1 When used, the system shall work.\n",
    );
    for file in [TASK, SECOND] {
        edit(
            &root,
            file,
            listed,
            "- AC-1 When run, the system shall work (serves EPC-001 AC-1)\n",
        );
    }
    let plan = commit(&root, "chore: plan the lifecycle journey");
    assert_eq!(verdict(&root, &plan).0, Some(0));

    // Spec: draft -> approved by the verb and by hand.
    verb_then_hand(
        &root,
        "project-management/specs/SPC-001.md",
        &["spec", "status", "SPC-001", "approved"],
        "chore: approve the contract",
    );

    // Block and unblock.
    let blocked = verb_then_hand(
        &root,
        TASK,
        &[
            "task",
            "status",
            "TSK-001",
            "blocked",
            "--reason",
            "wait",
            "--owner",
            "primary",
            "--revisit",
            "SPC-001 approved",
        ],
        "chore: block the first task",
    );
    assert!(blocked.contains("## Blocker\n\n- reason: wait\n"));
    verb_then_hand(
        &root,
        TASK,
        &["task", "status", "TSK-001", "todo"],
        "chore: unblock the first task",
    );

    // Negative controls: the hand edit and the verb refuse the same change.
    let planned = git(&root, &["rev-parse", "HEAD"]);
    edit(&root, TASK, "status: todo ", "status: blocked ");
    let (code, errors) = verdict(&root, &planned);
    assert_eq!(code, Some(1));
    assert!(
        errors.iter().any(|e| e.contains("needs a `## Blocker`")),
        "{errors:?}"
    );
    git(&root, &["checkout", "--", TASK]);
    edit(&root, TASK, "status: todo ", "status: complete ");
    let (code, hand) = verdict(&root, &planned);
    assert_eq!(code, Some(1));
    git(&root, &["checkout", "--", TASK]);
    let refused = codeflow(&root, &["task", "status", "TSK-001", "complete"]);
    assert_eq!(refused.status.code(), Some(1));
    let refusal = String::from_utf8_lossy(&refused.stderr);
    assert!(hand
        .iter()
        .all(|line| refusal.contains(line.split(": error: ").nth(1).unwrap())));
    let in_progress = codeflow(&root, &["task", "status", "TSK-001", "in_progress"]);
    assert_eq!(
        in_progress.status.code(),
        Some(2),
        "no verb writes in_progress"
    );

    // The plan lands on the integration line; task work starts from it.
    git(&root, &["switch", "-q", target]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/lifecycle",
            "-m",
            "chore: land the plan",
        ],
    );
    let before = git(&root, &["rev-parse", "HEAD"]);

    // A partial pull request, then a stacked slice that completes the task.
    git(&root, &["switch", "-q", "-c", "task/TSK-001-slice-one"]);
    let started = ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    assert!(started.contains("anchored"), "{started}");
    std::fs::write(root.join("one.txt"), "slice one\n").unwrap();
    let slice_one = commit(&root, "feat: add slice one");
    let partial = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &before,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-slice-one",
        ],
    );
    let partial_out = ok(&partial, "ci on the partial slice");
    assert!(partial_out.contains("work-records"), "{partial_out}");
    git(&root, &["switch", "-q", "-c", "task/TSK-001-slice-two"]);
    std::fs::write(root.join("two.txt"), "slice two\n").unwrap();
    commit(&root, "feat: add slice two");
    let block = acceptance(&root, &["AC-1"], "none | no journey criterion");
    let block = block.to_string_lossy().to_string();
    verb_then_hand(
        &root,
        TASK,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &block,
        ],
        "chore: complete the first task",
    );
    let stacked = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &slice_one,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-slice-two",
        ],
    );
    ok(&stacked, "ci on the stacked slice that completes the task");

    // Reopen, then complete again; cancel the second task.
    let reopened = verb_then_hand(
        &root,
        TASK,
        &[
            "task",
            "status",
            "TSK-001",
            "todo",
            "--reason",
            "regression",
        ],
        "chore: reopen the first task",
    );
    assert!(reopened.contains("acceptance_superseded:\n  reason: regression\n"));
    let block = acceptance(&root, &["AC-1"], "none | no journey criterion");
    let block = block.to_string_lossy().to_string();
    verb_then_hand(
        &root,
        TASK,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &block,
        ],
        "chore: complete the first task again",
    );
    verb_then_hand(
        &root,
        SECOND,
        &[
            "task",
            "status",
            "TSK-002",
            "cancelled",
            "--reason",
            "merged",
            "--scope",
            "folded into TSK-001",
        ],
        "chore: cancel the second task",
    );

    // Epic close counts the closing epic as the spec's last consumer, then
    // archives.
    let block = acceptance(&root, &["AC-1"], "none | no journey criterion");
    let block = block.to_string_lossy().to_string();
    verb_then_hand(
        &root,
        EPIC,
        &[
            "epic",
            "status",
            "EPC-001",
            "complete",
            "--acceptance",
            &block,
        ],
        "chore: close the epic",
    );
    let closed = git(&root, &["rev-parse", "HEAD"]);
    verb_then_hand(
        &root,
        EPIC,
        &["epic", "status", "EPC-001", "archived"],
        "chore: archive the epic",
    );

    // One transition per pull request, as CI judges it, and the whole tree.
    let archive = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &closed,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-slice-two",
        ],
    );
    ok(&archive, "ci on the archive step");
    let skipped = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &plan,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-slice-two",
        ],
    );
    let skipped_err = String::from_utf8_lossy(&skipped.stderr);
    assert!(
        skipped_err.contains("status draft -> archived is refused"),
        "a range that skips the close is refused: {skipped_err}"
    );
    let tree = ok(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(tree.contains("record(s) clean"));
}

#[test]
fn a_standard_tier_project_without_records_sees_no_record_rule() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--standard"]),
        "init --standard",
    );
    let base = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", "feat/change"]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    commit(&root, "feat: add notes");
    let out = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &base,
            "--head",
            "HEAD",
            "--branch",
            "feat/change",
        ],
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(
        !text.contains("work-records") && !text.contains("work.records"),
        "{text}"
    );
    let validate = codeflow(&root, &["validate", "--docs"]);
    let text = String::from_utf8_lossy(&validate.stderr);
    assert!(validate.status.success(), "{text}");
    assert!(!text.contains("warning"), "{text}");
}

/// A branch forked before its target adopted `work_records_baseline` is
/// judged by the target's list in `codeflow ci` and `validate --since`
/// alike, so its own list cannot exempt its own new record (BL-R2-1).
#[test]
fn a_branch_forked_before_the_target_adopted_a_baseline_cannot_exempt_itself() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let target = "integration/EPC-001-line";
    git(&root, &["switch", "-q", "-c", target]);
    std::fs::write(root.join("notes.txt"), "seed\n").unwrap();
    let fork = commit(&root, "docs: seed the line");
    let config = root.join(".codeflow/project.toml");
    let state = std::fs::read_to_string(&config).unwrap();
    let with = |commit: &str| format!("{state}work_records_baseline = \"{commit}\"\n");
    std::fs::write(&config, with(&fork)).unwrap();
    commit(&root, "chore: adopt the records baseline");

    let branch = "fix/older-branch";
    git(&root, &["switch", "-q", "-c", branch, &fork]);
    std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
    std::fs::write(
        root.join(SECOND),
        "---\nid: TSK-002\nepic_id: null\nstandalone_reason: \"one change\"\n\
integration_target: main\ntitle: \"work\"\nstatus: complete\nwork_type: feat\n\
specs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# TSK-002: work\n\n\
## Description\n\nWork.\n\n## Acceptance Criteria\n\n- [x] AC-1 Checked.\n\n\
## Closeout\n\nNo block.\n",
    )
    .unwrap();
    let own = commit(&root, "docs: add a finished task");
    std::fs::write(&config, with(&own)).unwrap();
    commit(&root, "chore: exempt the task");

    let ci = codeflow(
        &root,
        &["ci", "--base", target, "--head", "HEAD", "--branch", branch],
    );
    let ci_err = String::from_utf8_lossy(&ci.stderr);
    assert!(!ci.status.success(), "{ci_err}");
    assert!(
        ci_err.contains("TSK-002.md: a complete record needs an acceptance block"),
        "{ci_err}"
    );
    assert!(!ci_err.contains("introduces"), "{ci_err}");
    let (code, errors) = verdict(&root, target);
    assert_ne!(code, Some(0), "{errors:?}");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("TSK-002.md: a complete record needs an acceptance block")),
        "{errors:?}"
    );
}

/// Whether a refusal names both routes of an approved spec (TSK-169 AC-3).
fn names_both_spec_routes(text: &str) -> bool {
    text.contains("approved never returns to draft")
        && text.contains("amend it in place in a planning change until it is implemented")
        && text.contains("once it is implemented, a changed contract is a new spec")
}

/// TSK-169 on a fresh `init --full` project: `spec status <id> draft` is
/// refused with both routes and writes nothing, before and after the spec
/// ships. An approved spec is amended while its consumer is open; once the
/// consumer completes, the same amendment is refused by `validate --since`
/// and by `codeflow ci`, which the installed pre-push hook runs.
#[test]
#[allow(clippy::too_many_lines)] // One spec lived from approval to shipped.
fn an_approved_spec_is_amended_until_it_ships_and_frozen_after() {
    const SPEC: &str = "project-management/specs/SPC-001.md";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let target = "integration/EPC-001-contract";
    git(&root, &["switch", "-q", "-c", target]);
    git(&root, &["switch", "-q", "-c", "plan/contract"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    let args = [
        "task", "new", "--epic", "EPC-001", "--into", target, "first",
    ];
    ok(&codeflow(&root, &args), "task new");
    ok(
        &codeflow(&root, &["spec", "new", "--for", "TSK-001", "contract"]),
        "spec new",
    );
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When run, the system shall work (serves EPC-001 AC-1)\n",
    );
    edit(
        &root,
        SPEC,
        "## Summary\n",
        "## Summary\n\nThe limit is 10.\n",
    );
    commit(&root, "chore: plan the contract");
    ok(
        &codeflow(&root, &["spec", "status", "SPC-001", "approved"]),
        "spec status approved",
    );
    let approved = commit(&root, "chore: approve the contract");

    let draft_refused = |when: &str| {
        let before = std::fs::read_to_string(root.join(SPEC)).unwrap();
        let out = codeflow(&root, &["spec", "status", "SPC-001", "draft"]);
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        assert_eq!(out.status.code(), Some(1), "{when}: {stderr}");
        assert!(names_both_spec_routes(&stderr), "{when}: {stderr}");
        assert_eq!(
            std::fs::read_to_string(root.join(SPEC)).unwrap(),
            before,
            "{when}: nothing written"
        );
    };
    draft_refused("approved, consumer open");

    // Before it ships: amended in place in a planning change.
    edit(&root, SPEC, "The limit is 10.", "The limit is 20.");
    let (code, errors) = verdict(&root, &approved);
    assert_eq!(code, Some(0), "an open spec is amended: {errors:?}");
    commit(&root, "chore: amend the contract before it ships");

    // Ship it: the plan lands, the only consumer completes.
    git(&root, &["switch", "-q", target]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/contract",
            "-m",
            "chore: land the plan",
        ],
    );
    git(&root, &["switch", "-q", "-c", "task/TSK-001-first"]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::write(root.join("first.txt"), "first\n").unwrap();
    commit(&root, "feat: add the first slice");
    let block = acceptance(&root, &["AC-1"], "none | no journey criterion");
    let block = block.to_string_lossy().to_string();
    let args = [
        "task",
        "status",
        "TSK-001",
        "complete",
        "--acceptance",
        &block,
    ];
    ok(&codeflow(&root, &args), "task complete");
    let shipped = commit(&root, "chore: complete the first task");
    draft_refused("implemented");

    // After it ships: the same amendment is refused by both planes.
    git(&root, &["switch", "-q", "-c", "plan/amend-shipped"]);
    edit(&root, SPEC, "The limit is 20.", "The limit is 999.");
    let (code, errors) = verdict(&root, &shipped);
    assert_eq!(code, Some(1), "{errors:?}");
    let frozen = "SPC-001 is implemented, so its text is frozen";
    assert!(errors.iter().any(|e| e.contains(frozen)), "{errors:?}");
    commit(&root, "chore: amend the shipped contract");
    let ci = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &shipped,
            "--head",
            "HEAD",
            "--branch",
            "plan/amend-shipped",
        ],
    );
    let ci_err = String::from_utf8_lossy(&ci.stderr);
    assert!(!ci.status.success(), "{ci_err}");
    assert!(ci_err.contains(frozen), "{ci_err}");
}
