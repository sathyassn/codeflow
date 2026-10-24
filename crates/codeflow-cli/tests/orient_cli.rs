//! Integration drift through the command and `SessionStart` entry.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn git(root: &Path, args: &[&str], time: i64) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_DATE", format!("@{time} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{time} +0000"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn commit(root: &Path, label: &str, time: i64) -> String {
    git(root, &["add", "."], time);
    git(root, &["commit", "--allow-empty", "-qm", label], time);
    git(root, &["rev-parse", "HEAD"], time)
}
fn run(root: &Path, hook: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    if hook {
        command.args(["hook", "session-orient"]);
    } else {
        command.arg("orient");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"SessionStart","source":"startup"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn row<'a>(output: &'a str, line: &str) -> &'a str {
    output
        .lines()
        .find(|r| r.starts_with(&format!("line: integration/{line} ")))
        .unwrap_or_else(|| panic!("missing {line}: {output}"))
}
fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}
fn day(time: i64) -> String {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), time);
    commit(dir.path(), "date", time);
    git(dir.path(), &["show", "-s", "--format=%cs", "HEAD"], time)
}
fn init(root: &Path, time: i64) {
    git(root, &["init", "-q", "-b", "main"], time);
    git(root, &["config", "user.name", "Fixture"], time);
    git(
        root,
        &["config", "user.email", "fixture@example.invalid"],
        time,
    );
}

fn seed_remote_lines(root: &Path, today: i64) {
    let month = today - 30 * 86_400;
    init(root, month);
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"line_sync_interval":1}}"#,
    );
    for (id, status) in [(1, "in_progress"), (2, "planning"), (3, "complete")] {
        write(root, &format!("project-management/epics/EPC-{id:03}.md"), &format!("---\nid: EPC-{id:03}\ntitle: Outcome {id}\nstatus: {status}\nwork_type: feat\ncreated: 2026-01-01\n---\n"));
    }
    let base = commit(root, "base", month);
    git(root, &["checkout", "-qb", "side"], month);
    write(root, "side", "side");
    commit(root, "side", month);
    git(root, &["checkout", "main"], month);
    git(
        root,
        &["merge", "--no-ff", "-qm", "inherited merge", "side"],
        month,
    );
    let target = git(root, &["rev-parse", "HEAD"], month);
    git(root, &["branch", "integration/fresh", &target], today);
    git(
        root,
        &["checkout", "-qb", "integration/unknown", &base],
        month,
    );
    write(root, "own", "own");
    let divergent = commit(root, "own", month);
    git(
        root,
        &["branch", "integration/named-today", &divergent],
        today,
    );
    git(
        root,
        &["checkout", "-qb", "integration/synced", &divergent],
        today,
    );
    for (id, status, date) in [
        (1, "todo", day(today - 10 * 86_400)),
        (2, "complete", "2020-01-01".into()),
    ] {
        write(root, &format!("project-management/tasks/TSK-{id:03}.md"), &format!("---\nid: TSK-{id:03}\nepic_id: EPC-001\ntitle: Task {id}\nstatus: {status}\nwork_type: feat\nintegration_target: integration/synced\ncreated: {date}\n---\n"));
    }
    commit(root, "records", today - 3 * 86_400);
    git(
        root,
        &["merge", "--no-ff", "-qm", "sync", &target],
        today - 2 * 86_400,
    );
    git(
        root,
        &["checkout", "-qb", "integration/synced-today", &divergent],
        today,
    );
    git(
        root,
        &["merge", "--no-ff", "-qm", "sync old target today", &target],
        today,
    );
    git(root, &["checkout", "main"], today);
    commit(root, "target advances", today);
}

#[test]
fn remote_lines_distinguish_sync_creation_and_inherited_history() {
    let source = tempfile::tempdir().unwrap();
    let clone = tempfile::tempdir().unwrap();
    let root = source.path();
    let today = now();
    let month = today - 30 * 86_400;
    seed_remote_lines(root, today);
    codeflow_fixture::clone(clone.path(), root, ".")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
    git(
        root,
        &["branch", "integration/advertised-later", "main"],
        today,
    );
    // Orient never fetches: a line created on the remote after the last
    // fetch appears only once the operator fetches.
    let before = run(clone.path(), false);
    assert_eq!(
        before.lines().filter(|l| l.starts_with("line:")).count(),
        5,
        "{before}"
    );
    assert!(!before.contains("integration/advertised-later"), "{before}");
    git(clone.path(), &["fetch", "-q", "origin"], today);
    let out = run(clone.path(), false);
    assert_eq!(
        out.lines().filter(|l| l.starts_with("line:")).count(),
        6,
        "{out}"
    );
    let synced = row(&out, "synced");
    assert!(
        synced.contains("1 behind")
            && synced.contains(&format!("last sync {}", day(today - 2 * 86_400)))
            && synced.contains("oldest unlanded task 10d")
            && synced.contains("sync overdue: codeflow line sync integration/synced"),
        "{synced}"
    );
    let recent = row(&out, "synced-today");
    assert!(
        recent.contains(&format!("last sync {}", day(today))) && !recent.contains("overdue"),
        "{recent}"
    );
    for name in ["unknown", "named-today"] {
        let r = row(&out, name);
        assert!(
            r.contains("creation unknown")
                && r.contains(&format!("oldest divergent commit {}", day(month)))
                && !r.contains("overdue"),
            "{r}"
        );
    }
    for name in ["fresh", "advertised-later"] {
        let r = row(&out, name);
        assert!(
            r.contains("fresh, no own commits")
                && !r.contains("last sync")
                && !r.contains("overdue"),
            "{r}"
        );
    }
    assert!(
        out.contains("open epic: EPC-001")
            && out.contains("open epic: EPC-002")
            && !out.contains("open epic: EPC-003"),
        "{out}"
    );
    let hook = run(clone.path(), true);
    assert_eq!(row(&hook, "synced"), synced);
    assert!(hook.contains("open epic: EPC-002"));
    write(root, ".codeflow/policy.json", "{}");
    commit(root, "disable interval", today);
    git(clone.path(), &["fetch", "-q", "origin"], today);
    assert!(!run(clone.path(), false).contains("sync overdue"));
}

#[test]
fn unreachable_remote_falls_back_to_local_creation_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let today = now();
    init(root, today);
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"line_sync_interval":1}}"#,
    );
    commit(root, "base", today - 30 * 86_400);
    git(
        root,
        &["checkout", "-qb", "integration/local"],
        today - 3 * 86_400,
    );
    commit(root, "own", today - 2 * 86_400);
    git(root, &["checkout", "main"], today);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            root.join("missing.git").to_str().unwrap(),
        ],
        today,
    );
    let out = run(root, false);
    let local = row(&out, "local");
    assert!(
        local.contains("(local)")
            && local.contains(&format!("creation {}", day(today - 3 * 86_400)))
            && local.contains("sync overdue"),
        "{out}"
    );
    git(
        root,
        &[
            "reflog",
            "expire",
            "--expire=all",
            "refs/heads/integration/local",
        ],
        today,
    );
    let out = run(root, false);
    assert!(
        row(&out, "local").contains("creation unknown") && !out.contains("sync overdue"),
        "{out}"
    );
}

#[test]
fn grafted_history_is_reported_without_failing_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root, now());
    let base = commit(root, "base", now());
    git(root, &["branch", "integration/line"], now());
    write(root, ".git/info/grafts", &format!("{base}\n"));
    let out = run(root, true);
    assert!(
        out.contains("integration lines unavailable:") && out.contains("overlays"),
        "{out}"
    );
    assert!(!out.contains("line: integration/line"));
}

#[test]
fn open_epics_count_each_claimed_task_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root, now());
    for id in 1..=2 {
        write(root, &format!("project-management/epics/EPC-{id:03}.md"), &format!("---\nid: EPC-{id:03}\ntitle: Outcome {id}\nstatus: in_progress\nwork_type: feat\ncreated: 2026-01-01\n---\n"));
    }
    for (id, epic) in [(1, 1), (2, 1), (3, 2)] {
        write(root, &format!("project-management/tasks/TSK-{id:03}.md"), &format!("---\nid: TSK-{id:03}\nepic_id: EPC-{epic:03}\ntitle: Task {id}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: main\ncreated: 2026-01-01\n---\n"));
    }
    commit(root, "records", now());
    for id in 1..=2 {
        git(root, &["branch", &format!("task/TSK-{id:03}-build")], now());
    }
    let out = run(root, false);
    assert!(
        out.contains("open epic: EPC-001 · Outcome 1 · 2 started tasks")
            && out.contains("open epic: EPC-002 · Outcome 2 · 0 started tasks"),
        "{out}"
    );
}

#[test]
fn non_directory_work_records_are_reported_without_failing_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), now());
    write(dir.path(), "project-management", "not a directory");
    commit(dir.path(), "invalid work records", now());
    let output = run(dir.path(), false);
    assert!(output.contains("open epics unavailable:"), "{output}");
}

#[test]
fn orient_never_fetches_and_reads_lines_as_last_fetched() {
    let source = tempfile::tempdir().unwrap();
    let clone = tempfile::tempdir().unwrap();
    init(source.path(), now());
    commit(source.path(), "base", now());
    git(source.path(), &["branch", "integration/one"], now());
    git(source.path(), &["branch", "integration/two"], now());
    codeflow_fixture::clone(clone.path(), source.path(), ".")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
    let trace = clone.path().join("trace.json");
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(clone.path())
        .arg("orient")
        .env("GIT_TRACE2_EVENT", &trace)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        row(&stdout, "one").contains("(remote)") && row(&stdout, "two").contains("(remote)"),
        "{stdout}"
    );
    // No git process at all leaves no trace file, which is zero fetches too.
    let trace = std::fs::read_to_string(trace).unwrap_or_default();
    let count = trace
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| {
            v["event"] == "start"
                && v["argv"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg == "fetch"))
        })
        .count();
    assert_eq!(count, 0, "{trace}");
}

#[test]
fn every_line_is_shown_even_beyond_the_base_digest_guideline() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), now());
    commit(dir.path(), "base", now());
    for n in 0..32 {
        git(
            dir.path(),
            &["branch", &format!("integration/line-{n:02}")],
            now(),
        );
    }
    let out = run(dir.path(), true);
    assert_eq!(
        out.lines().filter(|l| l.starts_with("line:")).count(),
        32,
        "{out}"
    );
}

#[test]
fn copied_and_renamed_reflogs_do_not_prove_the_current_refs_creation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let today = now();
    let old = today - 30 * 86_400;
    init(root, old);
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git":{"line_sync_interval":1}}"#,
    );
    commit(root, "base", old);
    git(root, &["checkout", "-qb", "integration/original"], old);
    commit(root, "own", old);
    git(root, &["checkout", "main"], today);
    git(
        root,
        &["branch", "-c", "integration/original", "integration/copied"],
        today,
    );
    git(
        root,
        &[
            "branch",
            "-m",
            "integration/original",
            "integration/renamed",
        ],
        today,
    );
    let out = run(root, false);
    for name in ["copied", "renamed"] {
        let line = row(&out, name);
        assert!(
            line.contains("creation unknown") && !line.contains("overdue"),
            "{line}"
        );
    }
}

#[test]
fn an_undated_task_record_is_named_and_hides_no_line_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init(root, now());
    commit(root, "base", now());
    git(root, &["checkout", "-qb", "integration/dated"], now());
    for (id, created) in [
        (1, "created: 2026-01-01\n"),
        (2, ""),
        (3, "created: 2026-02-30\n"),
    ] {
        write(root, &format!("project-management/tasks/TSK-{id:03}.md"), &format!("---\nid: TSK-{id:03}\nepic_id: EPC-001\ntitle: Task {id}\nstatus: todo\nwork_type: feat\nintegration_target: integration/dated\n{created}---\n"));
    }
    commit(root, "records", now());
    git(root, &["checkout", "main"], now());
    git(root, &["branch", "integration/other"], now());
    let out = run(root, false);
    assert!(
        row(&out, "dated").contains("oldest unlanded task")
            && !row(&out, "dated").contains("oldest unlanded task none")
            && out.contains("line records: integration/dated skipped, missing or invalid created date: TSK-002, TSK-003")
            && row(&out, "other").contains("fresh, no own commits")
            && !out.contains("integration lines unavailable"),
        "{out}"
    );
}
