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

/// The run is refused by `work.epic_line` naming `text`, and the remedy
/// names both routes for a run that found the line from its base.
fn refused_naming_routes(o: &Output, text: &str) {
    refused_by_rule(o, text);
    let shown = output(o);
    for route in [
        format!("pass --line-tip {LINE} on a landing candidate"),
        format!("--into {LINE}"),
        "CODEFLOW_PR_BODY".to_string(),
    ] {
        assert!(shown.contains(&route), "missing {route}: {shown}");
    }
}

const PR_BODY: &str = "## Summary\nLand work.\n\nTask: TSK-001\n";

/// AC-2, AC-3 and AC-7's shape: issue #489's four steps. A candidate of a
/// no-ff merge and one plain commit is refused by `codeflow ci --line-tip`
/// naming the commit, with the hook's message and remedy, and the hook
/// refuses the same push. The issue's own invocation, without the flag,
/// cannot tell a candidate from a pull request head, so it refuses too and
/// names both routes.
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
    // The issue's invocation: the line is found from the base.
    refused_naming_routes(&ci(&f, &["--base", &tip, "--head", "HEAD"]), &direct[..9]);
}

/// AC-3: a candidate of merges only, and one whose direct commit a landed
/// adoption names, pass both forms; the same candidate before the adoption
/// lands is refused by both, so the adoption is what changes the verdict.
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

    // The fetched tip stays before the direct commit, so every run below
    // judges a range that holds it.
    let (f, tip) = published();
    let direct = f.direct();
    let candidate = ["--base", tip.as_str(), "--head", "HEAD", "--line-tip", LINE];
    refused_by_rule(&ci(&f, &candidate), &direct[..9]);
    blocks(&f.push(&tip), &direct[..9]);
    f.land_adoption(&direct);
    assert_eq!(f.git(&["rev-parse", &format!("origin/{LINE}")]), tip);
    let adopted = ci(&f, &candidate);
    passes(&adopted);
    assert!(
        output(&adopted).contains("line tip: no unadopted direct commit"),
        "{}",
        output(&adopted)
    );
    passes(&f.push(&tip));
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
    // the run refuses and names both routes.
    f.git(&["switch", "-q", LINE]);
    f.git(&[
        "push",
        "-q",
        "origin",
        &format!("{tip}:refs/heads/integration/EPC-001-twin"),
    ]);
    f.git(&["fetch", "-q", "origin"]);
    let twin = ci(&f, &["--base", &tip, "--head", "HEAD"]);
    refused_by_rule(&twin, "several epic lines");
    assert!(
        output(&twin).contains("pass --line-tip <LINE> on a landing candidate"),
        "{}",
        output(&twin)
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
/// it is refused as a line-named head is; the same head as a pull request
/// head (`--into` or its body) lands by merge and is not a line tip. On
/// `main`, a head named like a line is not judged by this rule.
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
    for name in [task, LINE] {
        refused_naming_routes(
            &ci(&f, &["--base", &tip, "--head", "HEAD", "--branch", name]),
            &direct[..9],
        );
    }
    for context in [["--into", LINE], ["--pr-body", PR_BODY]] {
        let mut args = vec!["--base", tip.as_str(), "--head", "HEAD", "--branch", task];
        args.extend(context);
        let merged = ci(&f, &args);
        let text = output(&merged);
        assert!(!text.contains("line tip:"), "{text}");
        assert!(
            !text.contains("would receive a commit made directly"),
            "{text}"
        );
    }

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

/// AC-4 (b), review finding 2: tracking state that cannot be read at the
/// line's tip refuses even when the head and the target read as tracked, so
/// one readable revision never hides another's error.
#[test]
fn unreadable_tracking_at_the_tip_refuses() {
    let (f, _) = published();
    f.write(".codeflow/project.toml", "schema_version = [unclosed\n");
    f.commit("chore: break the project state");
    f.git(&["push", "-q", "origin", LINE]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    f.git(&["rm", "-q", ".codeflow/project.toml"]);
    f.commit("chore: drop the broken state");
    refused_by_rule(
        &ci(&f, &["--base", &tip, "--head", "HEAD", "--line-tip", LINE]),
        &format!("durable work tracking cannot be read at {}", &tip[..9]),
    );
}

/// A new line starts at the default target's tip. A change whose base is
/// that commit lands on the target, so it names no line and its own commit
/// is not refused; the landing route's `--line-tip` claim still judges the
/// same head as the line's next tip.
#[test]
fn a_new_line_at_the_default_tip_names_no_line() {
    let f = Line::new();
    f.git(&["push", "-q", "origin", "main", LINE]);
    let main = f.git(&["rev-parse", "main"]);
    assert_eq!(f.git(&["rev-parse", &format!("origin/{LINE}")]), main);
    f.git(&["switch", "-qc", "fix/TSK-001-main", "main"]);
    let own = f.direct();
    let into_main = ci(&f, &["--base", &main, "--head", "HEAD"]);
    let text = output(&into_main);
    assert!(
        text.contains(
            "line tip: none (the base is the default target's fetched tip 'origin/main')"
        ),
        "{text}"
    );
    assert!(!text.contains("work.epic_line"), "{text}");
    refused_by_rule(
        &ci(&f, &["--base", &main, "--head", "HEAD", "--line-tip", LINE]),
        &own[..9],
    );
}
