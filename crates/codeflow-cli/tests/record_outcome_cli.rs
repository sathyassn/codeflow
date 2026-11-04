//! Records state their Outcome and epics their Deliverables (TSK-435),
//! through the real binary: `task new` and `epic new` write the sections;
//! `update` marks the records an older release made as present at
//! adoption; `validate --docs` refuses a record created after that without
//! an Outcome, warns for an open record present at adoption, and never
//! reads a closed one.

use std::path::Path;
use std::process::{Command, Output};

/// `command` as the fixture's adopter runs it: a fixed git identity, the
/// hooks `init` installs bound to the binary under test, and no agent
/// session marker, so the hooks judge a human's commit wherever the test
/// runs.
fn as_adopter(command: &mut Command) -> &mut Command {
    command
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    for name in codeflow_core::root_checkout::AGENT_MARKERS {
        command.env_remove(name);
    }
    command
}

/// Runs the binary with an isolated `CODEFLOW_HOME`, as `task new` commits
/// to the ID registry.
fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let home = tempfile::tempdir().expect("home tempdir");
    as_adopter(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home.path())
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("codeflow binary runs")
}

fn git(dir: &Path, args: &[&str]) {
    let home = tempfile::tempdir().expect("home tempdir");
    let output = as_adopter(&mut Command::new("git"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home.path())
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

/// Three records the previous release made: `codeflow 3.1.0 task new` wrote
/// each, and `codeflow 3.1.0 task status TSK-002 cancelled` closed the
/// second. They are kept byte for byte as that release wrote them.
const PREVIOUS_RELEASE: [(&str, &str); 3] = [
    ("TSK-001", include_str!("fixtures/outcome-3.1.0/TSK-001.md")),
    ("TSK-002", include_str!("fixtures/outcome-3.1.0/TSK-002.md")),
    ("TSK-003", include_str!("fixtures/outcome-3.1.0/TSK-003.md")),
];

fn head_commit(dir: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// How many `validate --docs` lines warn that `id` has no Outcome.
fn outcome_warnings(out: &str, id: &str) -> usize {
    out.lines()
        .filter(|line| {
            line.contains("warning")
                && line.contains(&format!("{id}.md"))
                && line.contains("`## Outcome`")
        })
        .count()
}

/// The `validate --docs` lines that refuse a record for its Outcome.
fn outcome_errors(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|line| line.contains("error:") && line.contains("outcome:"))
        .collect()
}

/// AC-2, AC-3, AC-6 (journey): an adopter holds three records the previous
/// release made, one closed and one made the morning of the upgrade and
/// not yet committed. `update` records exactly their uids and the `HEAD`
/// it ran at, once. A record `task new` makes afterwards is refused until
/// its author states its Outcome, validates once it does, and is refused
/// again when the line is removed; each older open record warns once and
/// the closed one says nothing.
#[test]
fn update_marks_previous_release_records_and_later_ones_need_an_outcome() {
    let dir = repo();
    let root = dir.path();
    let init = codeflow(root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", text(&init));
    // The previous release wrote no marker.
    let state_file = ".codeflow/project.toml";
    let mut state: toml::Table = toml::from_str(&read(root, state_file)).unwrap();
    assert!(state.remove("outcome_present_uids").is_some());
    state.remove("outcome_required_since");
    write(root, state_file, &toml::to_string(&state).unwrap());
    for (id, record) in &PREVIOUS_RELEASE[..2] {
        write(root, &format!("project-management/tasks/{id}.md"), record);
    }
    git(root, &["switch", "-q", "-c", "chore/adopt"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: adopt codeflow"]);
    let (id, record) = PREVIOUS_RELEASE[2];
    write(root, &format!("project-management/tasks/{id}.md"), record);
    let head = head_commit(root);

    let update = codeflow(root, &["update"]);
    assert!(update.status.success(), "{}", text(&update));
    let state: toml::Table = toml::from_str(&read(root, state_file)).unwrap();
    let listed: std::collections::BTreeSet<&str> = state["outcome_present_uids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|uid| uid.as_str().unwrap())
        .collect();
    let made: Vec<String> = PREVIOUS_RELEASE
        .iter()
        .map(|(_, record)| uid(record))
        .collect();
    assert_eq!(listed, made.iter().map(String::as_str).collect());
    assert_eq!(
        state["outcome_required_since"].as_str(),
        Some(head.as_str())
    );
    let marked = read(root, state_file);
    let again = codeflow(root, &["update"]);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(read(root, state_file), marked, "the marker is written once");

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
    let warned_once = |out: &str| {
        for (id, warnings) in [
            ("TSK-001", 1),
            ("TSK-002", 0),
            ("TSK-003", 1),
            ("TSK-004", 0),
        ] {
            assert_eq!(outcome_warnings(out, id), warnings, "{id}:\n{out}");
        }
    };

    let refused = codeflow(root, &["validate", "--docs"]);
    let out = text(&refused);
    assert!(!refused.status.success(), "{out}");
    let errors = outcome_errors(&out);
    assert_eq!(errors.len(), 1, "{out}");
    assert!(
        errors[0].contains("project-management/tasks/TSK-004.md")
            && errors[0].contains("`## Outcome`"),
        "{out}"
    );
    warned_once(&out);

    let record = read(root, "project-management/tasks/TSK-004.md");
    write(
        root,
        "project-management/tasks/TSK-004.md",
        &filled(&record),
    );
    let clean = codeflow(root, &["validate", "--docs"]);
    let out = text(&clean);
    assert!(clean.status.success(), "{out}");
    assert!(outcome_errors(&out).is_empty(), "{out}");
    warned_once(&out);

    write(root, "project-management/tasks/TSK-004.md", &record);
    let removed = codeflow(root, &["validate", "--docs"]);
    let out = text(&removed);
    assert!(!removed.status.success(), "{out}");
    let errors = outcome_errors(&out);
    assert_eq!(errors.len(), 1, "{out}");
    assert!(errors[0].contains("TSK-004.md"), "{out}");
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
    let clean = codeflow(root, &["validate", "--docs"]);
    let out = text(&clean);
    assert!(clean.status.success(), "{out}");
    assert!(
        !out.lines()
            .any(|line| line.contains("TSK-001.md") && line.contains("outcome")),
        "{out}"
    );
}
