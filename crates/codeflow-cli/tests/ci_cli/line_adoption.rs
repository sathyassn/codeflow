#[path = "../support/line_adoption.rs"]
pub(super) mod fixture;
use fixture::{blocks, output, passes, Line, LINE};

/// TSK-248 AC-1 (issue 85): a landed adoption makes the line an epic line
/// whose class line prints the adoption, and the criteria findings stop;
/// without the entry, or with it added by a second direct commit, the line
/// is refused naming the commit.
#[test]
fn epic_line_adoption_ci_reports_landed_entry_and_preserves_controls() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    f.land_task();
    passes(&f.ci("main", LINE));
    let direct = f.direct();
    let unadopted = f.ci("main", LINE);
    blocks(&unadopted, &direct[..9]);
    assert!(output(&unadopted).contains("work.criteria_frozen"));
    assert!(output(&unadopted).contains("work.acceptance_binding"));
    f.git(&["branch", "unadopted"]);
    f.land_adoption(&direct);
    let result = f.ci("main", LINE);
    passes(&result);
    let text = output(&result);
    assert!(
        text.lines().any(|line| line.contains("pull request class:")
            && line.contains(&format!("adopts {}", &direct[..9]))
            && line.contains("Repair false positive")
            && line.contains("reviewed at https://example.test/review/1")),
        "{text}"
    );
    assert!(
        !text.contains("work.criteria_frozen") && text.contains("completion bound at"),
        "{text}"
    );
    f.git(&["switch", "-qc", "test/direct-adoption", "unadopted"]);
    f.adoption(&direct);
    let unlanded = f.commit("docs: unlanded adoption");
    blocks(&f.ci("main", LINE), &unlanded[..9]);
}

/// TSK-248 AC-2: an entry naming a commit outside the range is a note.
#[test]
fn epic_line_adoption_outside_range_is_a_note() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    let outside = f.git(&["rev-parse", "main"]);
    f.land_adoption(&outside);
    let result = f.ci("main", LINE);
    passes(&result);
    assert!(
        output(&result).contains("outside the range"),
        "{}",
        output(&result)
    );
    // The note carries its remedy (SPC-013 R-80).
    assert!(
        output(&result).contains(
            "outside the range, so it adopts nothing here\n  clear it: nothing to change"
        ),
        "{}",
        output(&result)
    );
}

/// TSK-248 AC-1: an entry counts only when it first arrived by a merge or
/// is already on the target.
#[test]
fn epic_line_adoption_requires_first_arrival_by_merge_unless_target_has_it() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    let direct = f.direct();
    f.adoption(&direct);
    let author = f.commit("docs: add adoption directly");
    f.git(&["switch", "-qc", "plan/adopt-author"]);
    let record = std::fs::read_to_string(f.root().join(fixture::EPIC_PATH)).unwrap()
        .replace("line_adoptions:\n", &format!("line_adoptions:\n  - commit: '{author}'\n    reason: Adopt the record change\n    review: https://example.test/review/3\n"));
    f.write(fixture::EPIC_PATH, &record);
    f.commit("docs: adopt the direct record change");
    f.git(&["switch", "-q", LINE]);
    f.merge("plan/adopt-author");
    // The later merge legitimizes its named commit, but cannot launder an
    // earlier entry first written directly on the line.
    blocks(&f.ci("main", LINE), &direct[..9]);
    f.git(&["switch", "-qc", "plan/target-adoption", "main"]);
    f.write(fixture::EPIC_PATH, &record);
    f.commit("docs: adopt both commits at target");
    passes(&f.ci("main", "plan/target-adoption"));
    f.git(&["switch", "-q", "main"]);
    f.merge("plan/target-adoption");
    f.git(&["push", "-q", "origin", "main"]);
    f.git(&["switch", "-q", LINE]);
    f.merge("main");
    passes(&f.ci("main", LINE));
}

/// TSK-433 AC-7 (issue #341): an `integration/` line that fails the
/// epic-line proof, here for an unadopted direct code commit, is judged
/// without a pull request body as frozen task work. `codeflow ci` says why
/// once, and the binding finding names the range head whose change it
/// counts, since the completion was made before it.
#[test]
fn an_unverified_line_names_its_downgrade_and_the_range_head() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    f.land_task();
    let direct = f.direct();
    let result = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ci", "--base", "main", "--head", "HEAD", "--branch", LINE])
        .output()
        .unwrap();
    let text = output(&result);
    assert_eq!(result.status.code(), Some(1), "{text}");
    let note = format!("'{LINE}' is not a verified epic line, so its completions bind at the range head with frozen criteria: ");
    assert_eq!(text.matches(&note).count(), 1, "{text}");
    assert!(
        text.contains("\n  clear it: land work on the line only by classified pull requests"),
        "{text}"
    );
    assert!(text.contains(&direct[..9]), "{text}");
    assert!(
        text.contains(&format!("the range head {direct} also changes src/lib.rs")),
        "{text}"
    );
    assert!(!text.contains("the completion also changes"), "{text}");
}

/// TSK-433 AC-7: an unverified line whose range carries only what a
/// planning amendment may, here a direct record change, is judged without a
/// pull request body as a planning amendment, never as frozen task work, so
/// the frozen-criteria note does not print.
#[test]
fn an_unverified_planning_only_line_prints_no_frozen_note() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path)).unwrap();
    f.write(
        path,
        &record.replace("AC-1 Work runs.", "AC-1 The planned work runs."),
    );
    f.commit("docs: reword the planned work");
    let result = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ci", "--base", "main", "--head", "HEAD", "--branch", LINE])
        .output()
        .unwrap();
    let text = output(&result);
    assert!(text.contains("acceptance binding: amendable"), "{text}");
    assert!(!text.contains("is not a verified epic line"), "{text}");
}
