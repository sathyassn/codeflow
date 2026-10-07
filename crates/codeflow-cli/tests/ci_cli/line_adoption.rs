#[path = "../support/line_adoption.rs"]
mod fixture;
use fixture::{blocks, output, passes, Line, LINE};

/// TSK-248 AC-1 (issue 85): a landed adoption makes the line an epic line
/// whose class line prints the adoption, and the criteria findings stop;
/// without the entry, or with it added by a second direct commit, the line
/// is refused naming the commit.
#[test]
fn epic_line_adoption_ci_reports_landed_entry_and_preserves_controls() {
    let f = Line::new();
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
    let outside = f.git(&["rev-parse", "main"]);
    f.land_adoption(&outside);
    let result = f.ci("main", LINE);
    passes(&result);
    assert!(
        output(&result).contains("outside the range"),
        "{}",
        output(&result)
    );
}

/// TSK-248 AC-1: an entry counts only when it first arrived by a merge or
/// is already on the target.
#[test]
fn epic_line_adoption_requires_first_arrival_by_merge_unless_target_has_it() {
    let f = Line::new();
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
    f.git(&["switch", "-q", LINE]);
    f.merge("main");
    passes(&f.ci("main", LINE));
}
