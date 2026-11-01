//! TSK-1155 (issue #489): `codeflow ci` judges a landing candidate's head as
//! its epic line's next tip with the pre-push hook's `work.epic_line` rule,
//! so a direct commit on the candidate is refused before its gate.
use std::process::Output;

use super::line_adoption::fixture::{self, blocks, output, passes, Line, LINE};

/// A line published at its tip, with the local line branch checked out.
fn published() -> (Line, String) {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main"]);
    f.land_task();
    f.git(&["push", "-q", "origin", LINE]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    (f, tip)
}

/// Land a reviewed planning branch on the candidate by a no-ff merge.
fn land_plan(f: &Line, name: &str) {
    let candidate = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-qc", name, &candidate]);
    f.write(&format!("docs/{}.md", name.replace('/', "-")), "Planned.\n");
    f.commit("docs: plan more work");
    f.git(&["switch", "-q", LINE]);
    f.merge(name);
}

fn ci(f: &Line, args: &[&str]) -> Output {
    f.command(env!("CARGO_BIN_EXE_codeflow"))
        .arg("ci")
        .args(args)
        .output()
        .unwrap()
}

fn refused_by_rule(o: &Output, text: &str) {
    blocks(o, text);
    assert!(output(o).contains("work.epic_line"), "{}", output(o));
}

/// The run passes with a `work.epic_line` warning naming `text`.
fn warned_by_rule(o: &Output, text: &str) {
    passes(o);
    warns(o, text);
}

/// The output carries a `work.epic_line` warning naming `text`, whatever
/// other rules decide.
fn warns(o: &Output, text: &str) {
    let shown = output(o);
    assert!(
        !shown
            .lines()
            .any(|line| line.contains("BLOCKED") && line.contains("work.epic_line")),
        "{shown}"
    );
    assert!(
        shown
            .lines()
            .any(|line| line.contains("work.epic_line") && line.contains("warn")),
        "{shown}"
    );
    assert!(shown.contains(text), "missing {text}: {shown}");
}

/// AC-2, AC-3 and AC-7's shape: issue #489's four steps. A candidate of a
/// no-ff merge and one plain commit is refused by `codeflow ci --line-tip`
/// naming the commit, with the hook's message and remedy, and the hook
/// refuses the same push. The issue's own invocation, without the flag,
/// reports the same finding as a warning.
#[test]
fn a_candidate_with_a_direct_commit_is_refused_as_the_hook_refuses_it() {
    let (f, tip) = published();
    land_plan(&f, "plan/EPC-001-more");
    let direct = f.direct();
    let result = ci(&f, &["--base", &tip, "--head", "HEAD", "--line-tip", LINE]);
    refused_by_rule(&result, &direct[..9]);
    let text = output(&result);
    assert!(
        text.contains(&format!(
            "{LINE} would receive a commit made directly on the line ({})",
            &direct[..9]
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("`git reset --keep refs/remotes/origin/{LINE}`")),
        "{text}"
    );
    // The same candidate through the hook.
    blocks(&f.push(&tip), &direct[..9]);
    // The issue's invocation: the line is found from the base, and the
    // same finding prints as a warning that names the flag.
    let bare = ci(&f, &["--base", &tip, "--head", "HEAD"]);
    warned_by_rule(&bare, &direct[..9]);
    assert!(
        output(&bare).contains(&format!("pass --line-tip {LINE}")),
        "{}",
        output(&bare)
    );
}

/// AC-3: a candidate of merges only, and one whose direct commit a landed
/// adoption names, pass both forms.
#[test]
fn clean_candidates_pass_ci_and_the_hook() {
    let (f, tip) = published();
    land_plan(&f, "plan/EPC-001-more");
    let merges = ci(&f, &["--base", &tip, "--head", "HEAD"]);
    passes(&merges);
    assert!(
        output(&merges).contains("line tip: no unadopted direct commit"),
        "{}",
        output(&merges)
    );
    passes(&f.push(&tip));

    let (f, tip) = published();
    let direct = f.direct();
    f.git(&["push", "-q", "origin", LINE]);
    let adopted_tip = f.git(&["rev-parse", "HEAD"]);
    assert_ne!(tip, adopted_tip);
    f.land_adoption(&direct);
    // The whole range from the earlier tip holds the adopted commit.
    passes(&ci(
        &f,
        &["--base", &tip, "--head", "HEAD", "--line-tip", LINE],
    ));
    passes(&f.push(&adopted_tip));
}

/// AC-4 (a): a base placed after an unadopted direct commit does not hide
/// it when the line is named; the range always starts at the fetched tip.
/// A head not cut from the tip refuses.
#[test]
fn a_base_after_the_direct_commit_does_not_hide_it() {
    let (f, _) = published();
    let direct = f.direct();
    land_plan(&f, "plan/EPC-001-more");
    refused_by_rule(
        &ci(
            &f,
            &["--base", &direct, "--head", "HEAD", "--line-tip", LINE],
        ),
        &direct[..9],
    );
    f.git(&["switch", "-qc", "plan/EPC-001-elsewhere", "main"]);
    refused_by_rule(
        &ci(
            &f,
            &["--base", "main", "--head", "HEAD", "--line-tip", LINE],
        ),
        "not cut from the line's fetched tip",
    );
}

/// AC-4 (a): a line with no fetched tip (a local ref only), a line whose
/// records exist only in the head, and a base that two lines claim each
/// refuse with the cause.
#[test]
fn a_line_without_trusted_authority_refuses() {
    let (f, tip) = published();
    f.git(&["branch", "integration/EPC-001-local"]);
    refused_by_rule(
        &ci(
            &f,
            &[
                "--base",
                &tip,
                "--head",
                "HEAD",
                "--line-tip",
                "integration/EPC-001-local",
            ],
        ),
        "has no fetched tip",
    );

    // EPC-002's records exist only in the head, never at the line's tip.
    let other = "integration/EPC-002-new";
    f.git(&["switch", "-qc", other, "main"]);
    f.git(&["push", "-q", "origin", other]);
    let other_tip = f.git(&["rev-parse", "HEAD"]);
    let epic = std::fs::read_to_string(f.root().join(fixture::EPIC_PATH)).unwrap();
    f.write(
        "project-management/epics/EPC-002.md",
        &epic.replace("EPC-001", "EPC-002"),
    );
    let task =
        std::fs::read_to_string(f.root().join("project-management/tasks/TSK-001.md")).unwrap();
    f.write(
        "project-management/tasks/TSK-002.md",
        &task
            .replace("TSK-001", "TSK-002")
            .replace("EPC-001", "EPC-002")
            .replace(LINE, other),
    );
    f.commit("docs: bind a line in the head");
    refused_by_rule(
        &ci(
            &f,
            &["--base", &other_tip, "--head", "HEAD", "--line-tip", other],
        ),
        "has no epic record on 'origin/integration/EPC-002-new'",
    );

    // Two lines at the same fetched tip: the base names no single line, so
    // the run says so, as a warning since it was not given a line.
    f.git(&["switch", "-q", LINE]);
    f.git(&[
        "push",
        "-q",
        "origin",
        &format!("{tip}:refs/heads/integration/EPC-001-twin"),
    ]);
    f.git(&["fetch", "-q", "origin"]);
    warned_by_rule(
        &ci(&f, &["--base", &tip, "--head", "HEAD"]),
        "several epic lines",
    );
}

/// AC-4 (b): adoption entries that cannot be read refuse: here the target's
/// epic record, read for a head entry, is malformed.
#[test]
fn unreadable_adoption_entries_refuse() {
    let (f, tip) = published();
    let direct = f.direct();
    f.git(&["switch", "-q", "main"]);
    f.write(fixture::EPIC_PATH, "---\nid: EPC-001\n: [unclosed\n---\n");
    f.commit("docs: break the epic record");
    f.git(&["switch", "-q", LINE]);
    f.adoption(&direct);
    f.commit("docs: adopt in the head");
    let result = ci(&f, &["--base", &tip, "--head", "HEAD", "--line-tip", LINE]);
    assert_eq!(result.status.code(), Some(1), "{}", output(&result));
    assert!(
        output(&result).contains("work.epic_line"),
        "{}",
        output(&result)
    );
}

/// AC-4 (c): a malformed adoption entry in the head refuses even when the
/// candidate holds only merges.
#[test]
fn a_malformed_adoption_entry_refuses() {
    let (f, tip) = published();
    let name = "plan/EPC-001-adopt";
    f.git(&["switch", "-qc", name]);
    let epic = std::fs::read_to_string(f.root().join(fixture::EPIC_PATH)).unwrap();
    f.write(
        fixture::EPIC_PATH,
        &epic.replacen("---\n", "---\nline_adoptions:\n  - commit: abc\n", 1),
    );
    f.commit("docs: add a broken adoption");
    f.git(&["switch", "-q", LINE]);
    f.merge(name);
    // On the line's own branch the epic-line proof refuses first.
    let own = ci(&f, &["--base", &tip, "--head", "HEAD"]);
    assert_ne!(own.status.code(), Some(0), "{}", output(&own));
    assert!(
        output(&own).contains("line_adoptions[0].commit"),
        "{}",
        output(&own)
    );
    // Under any other name this check refuses it.
    refused_by_rule(
        &ci(
            &f,
            &[
                "--base",
                &tip,
                "--head",
                "HEAD",
                "--branch",
                name,
                "--line-tip",
                LINE,
            ],
        ),
        "line_adoptions cannot be read at the head",
    );
}

/// AC-4 (d): a head that deletes the epic's records still has its direct
/// commit judged, since tracking is on at the line's tip and the target.
#[test]
fn deleting_the_records_does_not_switch_the_check_off() {
    let (f, tip) = published();
    f.git(&["rm", "-q", "-r", "project-management"]);
    let direct = f.commit("chore: drop the records");
    refused_by_rule(
        &ci(&f, &["--base", &tip, "--head", "HEAD", "--line-tip", LINE]),
        &direct[..9],
    );
}

/// AC-4 (e): an adoption entry made only in the head's own commit has not
/// landed and adopts nothing.
#[test]
fn an_unlanded_adoption_in_the_head_adopts_nothing() {
    let (f, tip) = published();
    let direct = f.direct();
    f.adoption(&direct);
    f.commit("docs: adopt it directly");
    refused_by_rule(
        &ci(&f, &["--base", &tip, "--head", "HEAD", "--line-tip", LINE]),
        &direct[..9],
    );
}

/// AC-5: the head's branch name grants and removes nothing. A head named
/// like a task and offered as the line tip is refused, and without the flag
/// it gets the same warning a line-named head gets; the same head as a pull
/// request head (`--into`) lands by merge and is not a line tip. On `main`,
/// a head named like a line is not judged by this rule.
#[test]
fn the_head_name_grants_and_removes_nothing() {
    let (f, tip) = published();
    let direct = f.direct();
    let task = "task/TSK-001-work";
    refused_by_rule(
        &ci(
            &f,
            &[
                "--base",
                &tip,
                "--head",
                "HEAD",
                "--branch",
                task,
                "--line-tip",
                LINE,
            ],
        ),
        &direct[..9],
    );
    // Other rules judge a task-named head on their own; this rule warns.
    warns(
        &ci(&f, &["--base", &tip, "--head", "HEAD", "--branch", task]),
        &direct[..9],
    );
    warned_by_rule(
        &ci(&f, &["--base", &tip, "--head", "HEAD", "--branch", LINE]),
        &direct[..9],
    );
    let merged = ci(
        &f,
        &[
            "--base",
            &tip,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-work",
            "--into",
            LINE,
        ],
    );
    assert!(
        !output(&merged).contains("line tip:"),
        "{}",
        output(&merged)
    );
    assert!(
        !output(&merged).contains("would receive a commit made directly"),
        "{}",
        output(&merged)
    );

    // On main, a head named like a line: no line destination.
    f.git(&["switch", "-qc", "integration/EPC-001-onmain", "main"]);
    f.write("docs/note.md", "Note.\n");
    f.commit("docs: add a note");
    let main = f.git(&["rev-parse", "main"]);
    let on_main = ci(&f, &["--base", &main, "--head", "HEAD"]);
    let text = output(&on_main);
    assert!(text.contains("line tip: none"), "{text}");
    assert!(!text.contains("work.epic_line"), "{text}");
}
