//! Records state their Outcome and epics their Deliverables (TSK-435),
//! through the real binary: `task new` and `epic new` write the sections,
//! and `validate --docs` refuses a record created after the project
//! adopted the section without one, warns for an open record present at
//! adoption, and never reads a closed one.

use std::path::Path;
use std::process::{Command, Output};

/// Runs the binary with an isolated `CODEFLOW_HOME` and a fixed git
/// identity, as `task new` commits to the ID registry.
fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let home = tempfile::tempdir().expect("home tempdir");
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home.path())
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("codeflow binary runs")
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-m", "chore: initial commit"]);
    dir
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The `uid:` a record carries.
fn uid(record: &str) -> String {
    record
        .lines()
        .find_map(|line| line.strip_prefix("uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string()
}

/// Where each of `headings` starts in `record`, in that order.
fn in_order(record: &str, headings: &[&str]) {
    let at: Vec<Option<usize>> = headings
        .iter()
        .map(|heading| record.find(&format!("\n{heading}\n")))
        .collect();
    assert!(
        at.iter().all(Option::is_some) && at.windows(2).all(|pair| pair[0] < pair[1]),
        "{headings:?} in order:\n{record}"
    );
}

/// The marker listing `uids` as present at adoption.
fn mark(root: &Path, uids: &[&str]) {
    let mut state: toml::Table = toml::from_str(&read(root, ".codeflow/project.toml")).unwrap();
    let mut marker = toml::Table::new();
    marker.insert(
        "outcome_present_uids".into(),
        toml::Value::Array(uids.iter().map(|uid| toml::Value::from(*uid)).collect()),
    );
    state.extend(marker);
    write(
        root,
        ".codeflow/project.toml",
        &toml::to_string(&state).unwrap(),
    );
}

const OUTCOME_LINE: &str = "after this merges, an adopter can ship the thing";

/// `record` with its Outcome comment followed by `OUTCOME_LINE`.
fn filled(record: &str) -> String {
    record.replacen(
        "\n## Deliverables\n",
        &format!("\n{OUTCOME_LINE}\n\n## Deliverables\n"),
        1,
    )
}

/// AC-1: `task new` and `epic new` write `## Outcome` with its one-line
/// comment, and `epic new` also writes `## Deliverables`.
#[test]
fn new_records_carry_an_outcome_and_epics_their_deliverables() {
    let dir = repo();
    let root = dir.path();
    let epic = codeflow(root, &["epic", "new", "Ship the thing"]);
    assert!(epic.status.success(), "{}", text(&epic));
    let task = codeflow(root, &["task", "new", "--epic", "EPC-001", "Build it"]);
    assert!(task.status.success(), "{}", text(&task));

    let epic = read(root, "project-management/epics/EPC-001.md");
    in_order(
        &epic,
        &[
            "## Summary",
            "## Outcome",
            "## Deliverables",
            "## Acceptance Criteria",
        ],
    );
    let task = read(root, "project-management/tasks/TSK-001.md");
    in_order(
        &task,
        &[
            "## Description",
            "## Outcome",
            "## Deliverables",
            "## Acceptance Criteria",
        ],
    );
    for record in [&epic, &task] {
        assert!(
            record.contains("\"after this merges, <who> can <do what>\""),
            "{record}"
        );
    }
}

/// AC-2, AC-3, AC-6: with the marker listing the records present at
/// adoption, a record `task new` makes later is refused until it states its
/// Outcome, and refused again when the line is removed; the listed open
/// record warns once, also when it was made the morning of the upgrade;
/// the closed record says nothing.
#[test]
fn validate_refuses_a_later_record_and_warns_for_older_ones() {
    let dir = repo();
    let root = dir.path();
    let init = codeflow(root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", text(&init));
    for title in ["Older open", "Older closed", "Made this morning"] {
        let made = codeflow(
            root,
            &[
                "task",
                "new",
                "--standalone-reason",
                "test",
                "--into",
                "main",
                title,
            ],
        );
        assert!(made.status.success(), "{}", text(&made));
    }
    let closed = read(root, "project-management/tasks/TSK-002.md")
        .replace("status: todo ", "status: cancelled ")
        .replace(
            "## Closeout\n",
            "## Closeout\n\n- cancelled: superseded\n- scope: dropped\n",
        );
    write(root, "project-management/tasks/TSK-002.md", &closed);
    let present: Vec<String> = ["TSK-001", "TSK-002", "TSK-003"]
        .iter()
        .map(|id| uid(&read(root, &format!("project-management/tasks/{id}.md"))))
        .collect();
    mark(
        root,
        &present.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let later = codeflow(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "test",
            "--into",
            "main",
            "Later",
        ],
    );
    assert!(later.status.success(), "{}", text(&later));

    let refused = codeflow(root, &["validate", "--docs"]);
    let out = text(&refused);
    assert!(!refused.status.success(), "{out}");
    let errors: Vec<&str> = out
        .lines()
        .filter(|line| line.contains("outcome:") && !line.contains("warning"))
        .collect();
    assert_eq!(errors.len(), 1, "{out}");
    assert!(
        errors[0].contains("project-management/tasks/TSK-004.md")
            && errors[0].contains("`## Outcome`"),
        "{out}"
    );
    for (id, warnings) in [("TSK-001", 1), ("TSK-003", 1), ("TSK-002", 0)] {
        let count = out
            .lines()
            .filter(|line| {
                line.contains("warning")
                    && line.contains(&format!("{id}.md"))
                    && line.contains("`## Outcome`")
            })
            .count();
        assert_eq!(count, warnings, "{id}:\n{out}");
    }

    let record = read(root, "project-management/tasks/TSK-004.md");
    write(
        root,
        "project-management/tasks/TSK-004.md",
        &filled(&record),
    );
    let clean = codeflow(root, &["validate", "--docs"]);
    let out = text(&clean);
    assert!(
        !out.lines()
            .any(|line| line.contains("TSK-004.md") && line.contains("outcome")),
        "{out}"
    );

    write(root, "project-management/tasks/TSK-004.md", &record);
    let again = codeflow(root, &["validate", "--docs"]);
    let out = text(&again);
    assert!(!again.status.success(), "{out}");
    assert!(
        out.lines().any(|line| line.contains("TSK-004.md")
            && line.contains("`## Outcome`")
            && !line.contains("warning")),
        "{out}"
    );
}

/// AC-6: a fresh `init` project lists no record, so a record `task new`
/// makes there is refused until it states its Outcome.
#[test]
fn a_fresh_init_project_refuses_a_record_without_an_outcome() {
    let dir = repo();
    let root = dir.path();
    let init = codeflow(root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", text(&init));
    assert!(
        read(root, ".codeflow/project.toml").contains("outcome_present_uids = []"),
        "{}",
        read(root, ".codeflow/project.toml")
    );
    let made = codeflow(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "test",
            "--into",
            "main",
            "First",
        ],
    );
    assert!(made.status.success(), "{}", text(&made));
    let refused = codeflow(root, &["validate", "--docs"]);
    let out = text(&refused);
    assert!(!refused.status.success(), "{out}");
    assert!(
        out.lines()
            .any(|line| line.contains("TSK-001.md") && line.contains("`## Outcome`")),
        "{out}"
    );
    let record = read(root, "project-management/tasks/TSK-001.md");
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &filled(&record),
    );
    let out = text(&codeflow(root, &["validate", "--docs"]));
    assert!(
        !out.lines()
            .any(|line| line.contains("TSK-001.md") && line.contains("outcome")),
        "{out}"
    );
}
