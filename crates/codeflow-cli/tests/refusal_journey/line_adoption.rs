#[path = "../support/line_adoption.rs"]
mod fixture;
use fixture::{blocks, output, passes, Line, LINE};

/// TSK-248 AC-4 (issue 85), the reporter's scenario: a line with one direct
/// commit is refused, a planning pull request adds the adoption and lands on
/// the line by merge, and the line's pull request to main then passes.
#[test]
fn epic_line_adoption_journey_repairs_a_direct_commit_by_planning_pr() {
    let f = Line::new();
    let direct = f.direct();
    blocks(&f.ci("main", LINE), &direct[..9]);
    f.git(&["switch", "-qc", "plan/adopt"]);
    f.adoption(&direct);
    f.commit("docs: adopt reviewed repair");
    let planning = f.ci(LINE, "plan/adopt");
    passes(&planning);
    assert!(output(&planning).contains("class: planning-only"));
    f.git(&["switch", "-q", LINE]);
    f.merge("plan/adopt");
    passes(&f.push(&direct));
    let landed = f.ci("main", LINE);
    passes(&landed);
    assert!(output(&landed).contains(&format!("adopts {}", &direct[..9])));
}

/// TSK-248 AC-3 remedy, driven: the refused unpushed commit is kept on a
/// task branch, `git reset --keep` clears the line, and the merge passes.
#[test]
fn epic_line_adoption_unpushed_repair_keeps_work_on_a_task_branch() {
    let f = Line::new();
    let direct = f.direct();
    blocks(&f.push(fixture::ZERO), "git reset --keep main");
    f.git(&["branch", "task/TSK-001-repair"]);
    f.git(&["reset", "--keep", "main"]);
    assert_eq!(f.git(&["rev-parse", "task/TSK-001-repair"]), direct);
    f.merge("task/TSK-001-repair");
    passes(&f.push(fixture::ZERO));
    passes(&f.ci("main", LINE));
    assert_eq!(
        std::fs::read_to_string(f.root().join("src/lib.rs")).unwrap(),
        "pub fn direct() {}\n"
    );
}
