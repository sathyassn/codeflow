//! Recorded criterion transfers through the Cargo-built status and validation commands.

use super::*;

const TRANSFER_EPIC_PATH: &str = "project-management/epics/EPC-001.md";
const TRANSFER_TARGET_PATH: &str = "project-management/epics/EPC-002.md";
const TRANSFER_CRITERION: &str = "- AC-1 When used, the system shall work.";
const TRANSFER_NOTE: &str = "\n  - Amended 2026-11-03: transferred to EPC-002 AC-1.";

fn transfer_epic(id: &str, status: &str, criterion: &str) -> String {
    format!("---\nid: {id}\ntitle: Transfer\nstatus: {status}\nwork_type: fix\nspecs: []\n---\n\n# {id}\n\n## Outcome\n\nAn adopter can retain proof.\n\n## Deliverables\n\n- proof: src/lib.rs\n\n## Acceptance Criteria\n\n{criterion}\n")
}

fn transfer_plan(case: &str) -> (tempfile::TempDir, String) {
    let dir = repo(&[("TSK-001", "- AC-1 Work (serves EPC-001 AC-1)\n")], "");
    let root = dir.path();
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic("EPC-001", "planning", TRANSFER_CRITERION),
    );
    if case == "preexisting" {
        write(
            root,
            TRANSFER_TARGET_PATH,
            &transfer_epic("EPC-002", "planning", TRANSFER_CRITERION),
        );
    }
    commit(root, "docs: original criterion");
    let source = match case {
        "after-release" => "- AC-1 When used, the system shall work. (after release)",
        "journey" => "- AC-1 (journey) When used, the system shall work.",
        _ => TRANSFER_CRITERION,
    };
    // Establish an after-release obligation before the moving amendment too.
    if matches!(case, "after-release" | "journey") {
        write(
            root,
            TRANSFER_EPIC_PATH,
            &transfer_epic("EPC-001", "planning", source),
        );
        commit(root, "docs: after release obligation");
    }
    let note = match case {
        "self" => TRANSFER_NOTE.replace("EPC-002", "EPC-001"),
        "unreadable" => TRANSFER_NOTE.replace("transferred to", "transferred toward"),
        "unrecorded" => String::new(),
        _ => TRANSFER_NOTE.to_string(),
    };
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic("EPC-001", "planning", &format!("{source}{note}")),
    );
    let target = if case == "different" {
        "- AC-1 A different obligation."
    } else {
        source
    };
    let status = match case {
        "cancelled" => "cancelled",
        "archived" => "archived",
        "complete" => "complete",
        _ => "planning",
    };
    if case != "missing" {
        write(
            root,
            TRANSFER_TARGET_PATH,
            &transfer_epic("EPC-002", status, target),
        );
    }
    let path = record_path("TSK-001");
    let content = std::fs::read_to_string(root.join(&path)).unwrap();
    write(
        root,
        &path,
        &content.replacen(
            "epic_id: EPC-001",
            if case == "membership" {
                "epic_id: EPC-003"
            } else {
                "epic_id: EPC-002"
            },
            1,
        ),
    );
    if case == "nonplanning" {
        write(root, "src/lib.rs", "pub fn altered() {}\n");
    }
    let amendment = commit(root, "docs: transfer obligation");
    (dir, amendment)
}

fn close_transfer(root: &Path, epic: &str, result: &str) -> (bool, String) {
    close_transfer_criterion(root, epic, "AC-1", result)
}

fn close_transfer_criterion(
    root: &Path,
    epic: &str,
    criterion: &str,
    result: &str,
) -> (bool, String) {
    close_transfer_reviewed(root, epic, criterion, result, &head(root))
}

fn close_transfer_reviewed(
    root: &Path,
    epic: &str,
    criterion: &str,
    result: &str,
    reviewed: &str,
) -> (bool, String) {
    let acceptance = format!("acceptance:\n  reviewed: {reviewed}\n  review: session:transfer@sha256:00\n  criteria:\n    {criterion}: {result}\n  journey: none | ordinary criterion\n  not_verified: none\n  follow_ups: none: transfer recorded\n  verdict: approved\n");
    // Outside the repository: the completion binding must see no unrelated edit.
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), acceptance).unwrap();
    let out = codeflow()
        .args(["epic", "status", epic, "complete", "--acceptance"])
        .arg(file.path())
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Issue #492: a frozen source tag remains on a task moved to another epic.
/// Before TSK-1158 the final close refuses: no complete serving task verified it.
#[test]
fn recorded_transfer_closes_source_through_the_built_binary() {
    let serving = "- AC-1 Work (serves EPC-001 AC-5)\n";
    let dir = repo(&[("TSK-001", serving)], "");
    let root = dir.path();
    let source = TRANSFER_CRITERION.replacen("AC-1", "AC-5", 1);
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic("EPC-001", "planning", &source),
    );
    commit(root, "docs: plan the original AC-5 obligation");
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic("EPC-001", "planning", &format!("{source}{TRANSFER_NOTE}")),
    );
    write(
        root,
        TRANSFER_TARGET_PATH,
        &transfer_epic("EPC-002", "planning", TRANSFER_CRITERION),
    );
    let path = record_path("TSK-001");
    let original = std::fs::read_to_string(root.join(&path)).unwrap();
    write(
        root,
        &path,
        &original.replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    let amendment = commit(root, "docs: transfer AC-5 to the new AC-1");
    let result =
        close_transfer_criterion(root, "EPC-001", "AC-5", &format!("waived | {amendment}"));
    assert!(result.0, "{}", result.1);
    commit(root, "docs: close the source through its waiver");

    // The task keeps its old source tag and its own AC-1 result. Neither
    // criterion number may be mistaken for the new epic's AC-1.
    let reviewed = head(root);
    let acceptance = format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: session:transfer@sha256:00\n  criteria:\n    AC-1: verified | task proof\n  journey: none | ordinary criterion\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n");
    write(
        root,
        &path,
        &task("TSK-001", "complete", serving, &acceptance).replacen(
            "epic_id: EPC-001",
            "epic_id: EPC-002",
            1,
        ),
    );
    commit(root, "docs: complete the moved serving task");
    let result = close_transfer(root, "EPC-002", "verified | own proof");
    assert!(result.0, "{}", result.1);
}

#[test]
fn recorded_transfer_cli_refuses_each_invalid_authority() {
    for (case, reason) in [
        ("verified", "waiver naming the commit"),
        ("nonplanning", "planning-only amendment"),
        ("missing", "destination epic is missing"),
        ("cancelled", "open destination"),
        ("archived", "open destination"),
        ("complete", "open destination"),
        ("self", "own epic"),
        ("membership", "not a moved serving task"),
        ("unreadable", "use `- Amended"),
        ("after-release", "after-release transfer"),
        ("preexisting", "existed before"),
        ("different", "preserve the obligation"),
        ("unrecorded", "no complete serving task"),
    ] {
        let (dir, amendment) = transfer_plan(case);
        let evidence = if case == "verified" {
            "verified | own proof".to_string()
        } else {
            format!("waived | {amendment}")
        };
        let result = close_transfer(dir.path(), "EPC-001", &evidence);
        assert!(
            !result.0 && result.1.contains(reason),
            "{case}: {}",
            result.1
        );
    }
}

#[test]
fn recorded_transfer_cli_refuses_another_amendment_and_a_commit_outside_review() {
    for outside_review in [false, true] {
        let (dir, amendment) = transfer_plan("valid");
        let root = dir.path();
        let content = std::fs::read_to_string(root.join(TRANSFER_EPIC_PATH)).unwrap();
        write(
            root,
            TRANSFER_EPIC_PATH,
            &format!("{content}  - Amended 2026-11-04: clarify the record.\n"),
        );
        let other = commit(root, "docs: later amendment");
        if outside_review {
            git(root, &["checkout", "--detach", &amendment]);
        }
        let result = close_transfer(root, "EPC-001", &format!("waived | {other}"));
        assert!(!result.0, "{}", result.1);
        assert!(
            result.1.contains(if outside_review {
                "does not contain"
            } else {
                "waiver naming the commit"
            }),
            "{}",
            result.1
        );
    }
}

#[test]
fn recorded_transfer_cli_warns_only_for_dangling_or_unrecorded_tags() {
    for case in ["valid", "unrecorded", "dangling"] {
        let (dir, _) = transfer_plan(case);
        if case == "dangling" {
            let path = record_path("TSK-001");
            let content = std::fs::read_to_string(dir.path().join(&path)).unwrap();
            write(
                dir.path(),
                &path,
                &content.replace("EPC-001 AC-1)", "EPC-001 AC-99)"),
            );
        }
        let out = codeflow()
            .args(["validate", "--docs"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            said.contains("foreign epic without a recorded transfer"),
            case != "valid",
            "{case}: {said}"
        );
        if case != "valid" {
            assert!(said.contains("TSK-001 AC-1: (serves EPC-001"), "{said}");
        }
    }
}

#[test]
fn transfer_destination_cli_needs_complete_moved_tasks_and_the_journey() {
    for (complete, journey, native) in [
        (false, false, "none"),
        (true, false, "none"),
        (true, true, "none"),
        (true, true, "todo"),
        (true, true, "complete"),
    ] {
        let (dir, _) = transfer_plan("journey");
        let root = dir.path();
        if complete {
            let reviewed = head(root);
            let journey = if journey {
                "verified | fresh project"
            } else {
                "none | not exercised"
            };
            let acceptance = format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: session:transfer@sha256:00\n  criteria:\n    AC-1: verified | task proof\n  journey: {journey}\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n");
            write(
                root,
                &record_path("TSK-001"),
                &task(
                    "TSK-001",
                    "complete",
                    "- AC-1 Work (serves EPC-001 AC-1)\n",
                    &acceptance,
                )
                .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
            );
        }
        if native != "none" {
            write(
                root,
                &record_path("TSK-002"),
                &task(
                    "TSK-002",
                    native,
                    "- AC-1 Work (serves EPC-002 AC-1)\n",
                    "Pending.",
                )
                .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
            );
        }
        if complete || native != "none" {
            commit(root, "docs: serving task evidence");
        }
        let result = close_transfer(root, "EPC-002", "verified | own proof");
        assert_eq!(
            result.0,
            complete && journey && native == "none",
            "{complete}, {journey}, {native}: {}",
            result.1
        );
    }
}

#[test]
fn transfer_cli_source_waiver_keeps_the_destination_journey_obligation() {
    let (dir, amendment) = transfer_plan("journey");
    let result = close_transfer(dir.path(), "EPC-001", &format!("waived | {amendment}"));
    assert!(result.0, "{}", result.1);
}

#[test]
fn deleted_source_criterion_workaround_closes_but_warns_about_its_frozen_tag() {
    let (dir, _) = transfer_plan("unrecorded");
    let root = dir.path();
    let original = std::fs::read_to_string(root.join(TRANSFER_EPIC_PATH)).unwrap();
    write(
        root,
        TRANSFER_EPIC_PATH,
        &format!("{original}- AC-2 An unchanged ordinary obligation.\n"),
    );
    commit(root, "docs: retain another source obligation");
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic(
            "EPC-001",
            "planning",
            "- AC-2 An unchanged ordinary obligation.",
        ),
    );
    commit(root, "docs: delete source criterion workaround");
    let result =
        close_transfer_criterion(root, "EPC-001", "AC-2", "verified | unchanged obligation");
    assert!(result.0, "{}", result.1);
    let out = codeflow()
        .args(["validate", "--docs"])
        .current_dir(root)
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        said.contains("TSK-001 AC-1: (serves EPC-001 AC-1)"),
        "{said}"
    );
}

#[test]
fn transfer_cli_a_tick_or_absent_waiver_never_closes_the_source() {
    for ticked in [false, true] {
        let (dir, _) = transfer_plan("valid");
        let root = dir.path();
        if ticked {
            let content = std::fs::read_to_string(root.join(TRANSFER_EPIC_PATH)).unwrap();
            write(
                root,
                TRANSFER_EPIC_PATH,
                &content.replace("- AC-1", "- [x] AC-1"),
            );
            commit(root, "docs: tick transferred criterion");
        }
        let out = codeflow()
            .args(["epic", "status", "EPC-001", "complete"])
            .current_dir(root)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && said.contains("own waiver"),
            "{said}"
        );
    }
}

#[test]
fn cancelled_moved_tasks_cli_refuses_destination_own_block_proof() {
    let (dir, _) = transfer_plan("valid");
    let root = dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "cancelled",
            "- AC-1 Work (serves EPC-001 AC-1)\n",
            "- cancelled: work stopped\n- scope: retained in destination\n",
        )
        .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    commit(root, "docs: cancel moved task");
    let result = close_transfer(root, "EPC-002", "verified | own proof");
    assert!(
        !result.0 && result.1.contains("no complete moved serving task"),
        "{}",
        result.1
    );
}

#[test]
fn cancelled_native_task_cli_keeps_complete_moved_task_proof() {
    let (dir, _) = transfer_plan("valid");
    let root = dir.path();
    let reviewed = head(root);
    let close = format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: session:transfer@sha256:00\n  criteria:\n    AC-1: verified | task proof\n  journey: none | ordinary criterion\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n");
    write(
        root,
        &record_path("TSK-001"),
        &task(
            "TSK-001",
            "complete",
            "- AC-1 Work (serves EPC-001 AC-1)\n",
            &close,
        )
        .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    write(
        root,
        &record_path("TSK-002"),
        &task(
            "TSK-002",
            "cancelled",
            "- AC-1 Native work (serves EPC-002 AC-1)\n",
            "- cancelled: replaced\n- scope: complete moved task proves it\n",
        )
        .replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    commit(root, "docs: cancel native task beside moved proof");
    let result = close_transfer(root, "EPC-002", "verified | own proof");
    assert!(result.0, "{}", result.1);
}

#[test]
fn cancelled_foreign_membership_cli_requires_cancellation_at_both_points() {
    for (before, now) in [
        ("cancelled", "cancelled"),
        ("todo", "cancelled"),
        ("todo", "todo"),
    ] {
        let (dir, amendment) = transfer_plan("valid");
        let root = dir.path();
        let foreign_path = record_path("TSK-002");
        let foreign = |status: &str| {
            task(
                "TSK-002",
                status,
                "- AC-1 Foreign work (serves EPC-001 AC-1)\n",
                if status == "cancelled" {
                    "- cancelled: withdrawn\n- scope: carried elsewhere\n"
                } else {
                    "Pending.\n"
                },
            )
            .replacen("epic_id: EPC-001", "epic_id: EPC-003", 1)
        };
        write(
            root,
            "project-management/epics/EPC-003.md",
            &transfer_epic("EPC-003", "planning", "- AC-1 Other work."),
        );
        write(root, &foreign_path, &foreign(before));
        let reviewed = commit(root, "docs: record foreign membership at review");
        if before != now {
            write(root, &foreign_path, &foreign(now));
            commit(root, "docs: cancel foreign task after review");
        }
        let result = close_transfer_reviewed(
            root,
            "EPC-001",
            "AC-1",
            &format!("waived | {amendment}"),
            &reviewed,
        );
        assert_eq!(
            result.0,
            before == "cancelled",
            "{before} -> {now}: {}",
            result.1
        );
        if before != "cancelled" {
            assert!(
                result.1.contains("not a moved serving task"),
                "{}",
                result.1
            );
            if now == "cancelled" {
                assert!(result.1.contains("cancelled after review"), "{}", result.1);
            }
        }
    }
}

/// A dated wording note on the open destination keeps the obligation, so it
/// never invalidates the transfer for either epic or for later validation.
#[test]
fn destination_wording_note_keeps_both_transfer_closes() {
    let serving = "- AC-1 Work (serves EPC-001 AC-1)\n";
    let dir = repo(&[("TSK-001", serving)], "");
    let root = dir.path();
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic("EPC-001", "planning", TRANSFER_CRITERION),
    );
    commit(root, "docs: plan the original obligation");
    write(
        root,
        TRANSFER_EPIC_PATH,
        &transfer_epic(
            "EPC-001",
            "planning",
            &format!("{TRANSFER_CRITERION}{TRANSFER_NOTE}"),
        ),
    );
    write(
        root,
        TRANSFER_TARGET_PATH,
        &transfer_epic("EPC-002", "planning", TRANSFER_CRITERION),
    );
    let path = record_path("TSK-001");
    let original = std::fs::read_to_string(root.join(&path)).unwrap();
    write(
        root,
        &path,
        &original.replacen("epic_id: EPC-001", "epic_id: EPC-002", 1),
    );
    let amendment = commit(root, "docs: transfer the obligation");
    let noted = format!("{TRANSFER_CRITERION}\n  - Amended 2026-11-04: wording clarified.");
    write(
        root,
        TRANSFER_TARGET_PATH,
        &transfer_epic("EPC-002", "planning", &noted),
    );
    commit(root, "docs: note the destination wording");
    let result = close_transfer(root, "EPC-001", &format!("waived | {amendment}"));
    assert!(result.0, "{}", result.1);
    commit(root, "docs: close the source through its waiver");

    let reviewed = head(root);
    let acceptance = format!("```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: session:transfer@sha256:00\n  criteria:\n    AC-1: verified | task proof\n  journey: none | ordinary criterion\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n");
    write(
        root,
        &path,
        &task("TSK-001", "complete", serving, &acceptance).replacen(
            "epic_id: EPC-001",
            "epic_id: EPC-002",
            1,
        ),
    );
    commit(root, "docs: complete the moved serving task");
    let result = close_transfer(root, "EPC-002", "verified | own proof");
    assert!(result.0, "{}", result.1);
    commit(root, "docs: close the destination");
    let out = codeflow()
        .args(["validate", "--docs"])
        .current_dir(root)
        .output()
        .unwrap();
    // The minimal fixture epics lack unrelated sections; only the re-judged
    // completed epics matter here.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!said.contains("is unverified"), "{said}");
    assert!(!said.contains("transfer"), "{said}");
}
