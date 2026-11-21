//! Journey (TSK-1154 AC-7, issue #488): on a fresh `codeflow init --full`
//! project with one integration line, published on a destination through
//! the installed hooks, the issue's four steps pass `codeflow ci` on the
//! landing candidate: the line tip holds TSK-001 todo; TSK-001's branch
//! rewrites its own AC-1 with a dated amendment note and completes; the
//! candidate is the fetched tip plus a no-ff merge of that head; `codeflow
//! ci --base <tip> --head <candidate>` passes. The AC-4 (a) variant, a
//! member that changes another task's criteria, is refused, naming it.

use std::path::Path;
use std::process::Output;

use super::sequence::{planned_project, task_block};
use super::{codeflow, commit, edit, fails, git, ok, push, text, LINE, TASK};

const OWN: &str = "- AC-1 When run, the system shall work.\n";
const AMENDED: &str = "- AC-1 When run, the system shall work quickly.\n  - Amended 2026-11-09: reworded after review.\n";

/// The destination: a bare repository holding the default branch, then the
/// line pushed through the installed hooks. Returns the line's fetched tip.
fn publish(dir: &Path, root: &Path, main: &str) -> String {
    let origin = dir.join("origin.git");
    git(dir, &["init", "-q", "--bare", "-b", main, "origin.git"]);
    git(
        &origin,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{main}:{main}"),
        ],
    );
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(root, &["fetch", "-q", "origin"]);
    // Git before 2.48 records no remote HEAD on fetch (ADR-0091).
    git(root, &["remote", "set-head", "origin", "-a"]);
    git(root, &["switch", "-q", LINE]);
    let (pushed, said) = push(root, &["origin", LINE]);
    assert!(pushed, "the line pushes:\n{said}");
    git(root, &["fetch", "-q", "origin"]);
    git(root, &["rev-parse", &format!("origin/{LINE}")])
}

/// `task`'s reviewed head from `tip`: its code and `change`, reviewed, then
/// completed with `codeflow task status`.
fn member(
    dir: &Path,
    root: &Path,
    tip: &str,
    (task, file): (&str, &str),
    change: impl Fn(&Path),
) -> String {
    git(
        root,
        &["switch", "-q", "-c", &format!("task/{task}-work"), tip],
    );
    ok(&codeflow(root, &["work", "start", task]), "work start");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join(file), "pub fn work() {}\n").unwrap();
    change(root);
    let reviewed = commit(root, &format!("feat: build {task}"));
    let block = task_block(dir, task, &reviewed, "verified | the journey ran");
    ok(
        &codeflow(
            root,
            &["task", "status", task, "complete", "--acceptance", &block],
        ),
        "task status complete",
    );
    commit(root, &format!("chore: complete {task}"))
}

/// The candidate `name`: the fetched line tip plus a no-ff merge of `head`,
/// judged as the issue runs it.
fn candidate(root: &Path, tip: &str, name: &str, head: &str) -> Output {
    git(root, &["switch", "-q", "-c", name, tip]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            head,
            "-m",
            "chore: land the member",
        ],
    );
    codeflow(root, &["ci", "--base", tip, "--head", "HEAD"])
}

#[test]
fn an_own_criteria_change_lands_through_the_candidate_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let main = planned_project(&root, true);
    let tip = publish(dir.path(), &root, &main);
    let version = ok(&codeflow(&root, &["--version"]), "version");
    let binary = format!("{} {version}", env!("CARGO_BIN_EXE_codeflow"));

    let head = member(
        dir.path(),
        &root,
        &tip,
        ("TSK-001", "src/first.rs"),
        |root| {
            edit(root, TASK, OWN, AMENDED);
        },
    );
    let landed = candidate(&root, &tip, "candidate/epc-001-c1", &head);
    eprintln!("AC-7 candidate run, binary {binary}{}", text(&landed));
    let said = ok(&landed, "the issue's candidate");
    assert!(said.contains("TSK-001 AC-1"), "{said}");
    assert!(
        said.contains(&format!("member {head:.9} (TSK-001)")),
        "{said}"
    );
    assert!(!said.contains("work.criteria_frozen (block)"), "{said}");

    let foreign = member(
        dir.path(),
        &root,
        &tip,
        ("TSK-002", "src/second.rs"),
        |root| {
            edit(
                root,
                TASK,
                OWN,
                "- AC-1 When run, the system shall work well.\n",
            );
        },
    );
    let refused = candidate(&root, &tip, "candidate/epc-001-c2", &foreign);
    eprintln!(
        "AC-7 AC-4 (a) variant run, binary {binary}{}",
        text(&refused)
    );
    fails(&refused, "the AC-4 (a) variant", "work.criteria_frozen");
    let said = text(&refused);
    assert!(said.contains("TSK-001"), "{said}");
    assert!(said.contains(&format!("member {foreign:.9}")), "{said}");
}
