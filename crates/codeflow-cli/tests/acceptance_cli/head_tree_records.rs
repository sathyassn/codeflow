//! `codeflow ci` judges task records from the commits under judgment, never
//! from the checkout it runs in (TSK-1257, issue #531). One command gives
//! one verdict from the task's own checkout, from a root checkout on
//! `main` that lacks the task's record and from another task's checkout.
use super::*;

const LINE: &str = "integration/EPC-001-line-work";
const LINE_TASK: &str = "task/TSK-002-extend-the-line";
const OTHER_TASK: &str = "task/TSK-001-other-work";
/// The criteria the task adds to its own record in its range.
const GROWN: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-3 When asked, the system shall explain itself.\n";

/// A record of TSK-002 that targets the line, with `criteria`.
fn line_record(criteria: &str) -> String {
    task("TSK-002", "todo", criteria, "Pending.\n").replace(
        "integration_target: main",
        &format!("integration_target: {LINE}"),
    )
}

struct Fixture {
    dir: tempfile::TempDir,
    /// Linked checkouts the fixture made, removed with it.
    checkouts: tempfile::TempDir,
    line_tip: String,
    task_head: String,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// A linked checkout of `rev` named `name`, detached so it shares no
    /// branch with the root.
    fn checkout(&self, name: &str, rev: &str) -> std::path::PathBuf {
        let path = self.checkouts.path().join(name);
        git(
            self.root(),
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                path.to_str().unwrap(),
                rev,
            ],
        );
        path
    }
}

/// `main` holds TSK-001 only. The line adds TSK-002, which `main` never
/// holds; the task branch, cut from the line, changes code and grows its
/// own criteria, which is a note for its own task. The root checkout is
/// left on the task branch.
fn fixture() -> Fixture {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-q", "-c", LINE]);
    write(root, &record_path("TSK-002"), &line_record(OWN_JOURNEY));
    let line_tip = commit(root, "docs(records): plan the line task");
    git(root, &["switch", "-q", "-c", LINE_TASK]);
    write(root, "src/lib.rs", "pub fn line_work() {}\n");
    write(root, &record_path("TSK-002"), &line_record(GROWN));
    let task_head = commit(root, "feat: extend the line");
    // Another task cut from `main`, whose checkout lacks TSK-002.
    git(root, &["branch", OTHER_TASK, "main"]);
    Fixture {
        dir,
        checkouts: tempfile::tempdir().unwrap(),
        line_tip,
        task_head,
    }
}

/// The issue's command, with no pull request body as the pre-push hook
/// runs it: the line's tip to the task head, as the task branch, with the
/// policy and commits judged from `main`.
fn judge_from(fixture: &Fixture, dir: &Path) -> (i32, String) {
    let out = journey_codeflow()
        .args([
            "ci",
            "--base",
            &fixture.line_tip,
            "--head",
            &fixture.task_head,
            "--branch",
            LINE_TASK,
            "--policy-from",
            "main",
            "--commits-from",
            &fixture.line_tip,
        ])
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The `work.criteria_frozen` lines of a run, for comparing verdicts.
fn frozen_lines(result: &(i32, String)) -> Vec<String> {
    result
        .1
        .lines()
        .filter(|line| line.contains("work.criteria_frozen"))
        .map(str::to_owned)
        .collect()
}

/// AC-1: the task's own checkout, a root checkout on `main` that lacks the
/// task's record, and another task's checkout all give the task's note.
#[test]
fn one_command_gives_one_criteria_verdict_from_any_checkout() {
    let fixture = fixture();
    let own = judge_from(&fixture, fixture.root());
    assert_passes(&own, "the task's own checkout");
    assert!(
        own.1.contains("TSK-002 criteria delta"),
        "the task's own checkout prints its delta: {}",
        own.1
    );
    let on_main = fixture.checkout("on-main", "main");
    assert!(!on_main.join(record_path("TSK-002")).exists());
    let other = fixture.checkout("other-task", OTHER_TASK);
    for (dir, what) in [
        (on_main.as_path(), "a checkout on main"),
        (other.as_path(), "another task's checkout"),
    ] {
        let run = judge_from(&fixture, dir);
        assert_passes(&run, what);
        assert_eq!(frozen_lines(&run), frozen_lines(&own), "{what}: {}", run.1);
    }
}

/// AC-2: an uncommitted edit of the task's record in the checkout, which
/// would give it other criteria and another target, changes nothing: the
/// head tree's record decides.
#[test]
fn the_head_tree_record_wins_over_the_checkout_copy() {
    let fixture = fixture();
    let clean = judge_from(&fixture, fixture.root());
    assert_passes(&clean, "the clean checkout");
    write(
        fixture.root(),
        &record_path("TSK-002"),
        &task("TSK-002", "todo", NO_JOURNEY, "Pending.\n"),
    );
    let edited = judge_from(&fixture, fixture.root());
    assert_passes(&edited, "the checkout with an edited record");
    assert_eq!(frozen_lines(&edited), frozen_lines(&clean), "{}", edited.1);
}

/// A task branch cut from `main` that changes code but never commits its
/// task's record; returns the head.
fn unrecorded_task(root: &Path) -> String {
    git(
        root,
        &["switch", "-q", "-c", "task/TSK-003-unplanned", "main"],
    );
    write(root, "src/lib.rs", "pub fn unplanned() {}\n");
    commit(root, "feat: start unplanned work")
}

/// `codeflow ci` from `main` to `head` as `branch`, with no body.
fn push_run(root: &Path, head: &str, branch: &str) -> (i32, String) {
    let out = codeflow()
        .args(["ci", "--base", "main", "--head", head, "--branch", branch])
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// AC-2: a record the head tree lacks refuses, naming the head commit,
/// whether or not the checkout holds one: a record only the checkout holds
/// is not trusted.
#[test]
fn a_record_missing_from_the_head_tree_refuses() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    let head = unrecorded_task(root);
    let missing = push_run(root, &head, "task/TSK-003-unplanned");
    // The checkout alone holds the record, as an uncommitted file.
    write(
        root,
        &record_path("TSK-003"),
        &task("TSK-003", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    let checkout_only = push_run(root, &head, "task/TSK-003-unplanned");
    // The rule first, then the message that names the head.
    for (run, what) in [
        (&checkout_only, "a record only the checkout holds"),
        (&missing, "no record anywhere"),
    ] {
        assert_blocks(run, what, &["work.task_record"]);
    }
    for (run, what) in [
        (&checkout_only, "a record only the checkout holds"),
        (&missing, "no record anywhere"),
    ] {
        assert_blocks(
            run,
            what,
            &["names no task record in the judged head", head.as_str()],
        );
    }
}

/// AC-2: a task record the head tree holds but that does not parse there
/// refuses, naming the commit and the record's path.
#[test]
fn an_unreadable_head_record_refuses_naming_commit_and_path() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "task/TSK-004-broken", "main"]);
    write(root, "src/lib.rs", "pub fn broken() {}\n");
    write(root, &record_path("TSK-004"), "---\nid: [TSK-004\n---\n");
    let head = commit(root, "feat: start broken work");
    // The checkout's copy parses; it is not read.
    write(
        root,
        &record_path("TSK-004"),
        &task("TSK-004", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    let run = push_run(root, &head, "task/TSK-004-broken");
    // Exit 2: the range cannot be verified, which refuses it.
    assert_eq!(run.0, 2, "an unparsable head record: {}", run.1);
    for needle in [
        "project-management/tasks/TSK-004.md",
        &format!("at commit {head}"),
    ] {
        assert!(run.1.contains(needle), "missing {needle:?} in {}", run.1);
    }
}

/// AC-2: a head whose task record object cannot be read refuses, naming the
/// commit and the record's path, though the checkout holds a good copy.
#[test]
fn an_unreadable_head_tree_refuses_naming_commit_and_path() {
    let dir = repo(&[("TSK-001", OWN_JOURNEY)], "");
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "task/TSK-005-lost", "main"]);
    write(root, "src/lib.rs", "pub fn lost() {}\n");
    write(
        root,
        &record_path("TSK-005"),
        &task("TSK-005", "todo", OWN_JOURNEY, "Pending.\n"),
    );
    let head = commit(root, "feat: start lost work");
    let blob = git_out(
        root,
        &["rev-parse", &format!("HEAD:{}", record_path("TSK-005"))],
    );
    std::fs::remove_file(root.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
    assert!(root.join(record_path("TSK-005")).exists());
    let run = push_run(root, &head, "task/TSK-005-lost");
    assert_ne!(run.0, 0, "an unreadable head record: {}", run.1);
    for needle in [
        "project-management/tasks/TSK-005.md",
        &format!("at commit {head}"),
    ] {
        assert!(run.1.contains(needle), "missing {needle:?} in {}", run.1);
    }
}

/// The check list a run reports on its last line.
fn checks_ran(result: &(i32, String)) -> String {
    result
        .1
        .lines()
        .rev()
        .find(|line| line.contains("check(s) passed") || line.contains("checks that ran"))
        .unwrap_or_default()
        .to_string()
}

/// AC-3 (tracking state): a checkout with no tracking state never switches
/// the record checks off for a range whose commits carry tracking.
#[test]
fn an_untracked_checkout_keeps_the_record_checks_on() {
    let fixture = fixture();
    let own = judge_from(&fixture, fixture.root());
    assert_passes(&own, "the task's own checkout");
    let root = fixture.root();
    // `switch --orphan` leaves an empty index and removes the tracked files.
    git(root, &["switch", "-q", "--orphan", "untracked"]);
    for path in ["project-management", ".codeflow", "src"] {
        assert!(!root.join(path).exists(), "{path} is left behind");
    }
    write(root, "README.md", "untracked\n");
    git(root, &["add", "README.md"]);
    commit(root, "chore: an untracked tree");
    git(root, &["switch", "-q", LINE_TASK]);
    let untracked = fixture.checkout("untracked", "untracked");
    let run = judge_from(&fixture, &untracked);
    for check in ["work-records", "id-registry", "work-start"] {
        assert!(
            checks_ran(&own).contains(check),
            "the own checkout runs {check}: {}",
            own.1
        );
        assert!(
            checks_ran(&run).contains(check),
            "the untracked checkout runs {check}: {}",
            run.1
        );
    }
}

/// The binary the hook runs: the built one, or `CODEFLOW_JOURNEY_BIN` to
/// replay the journey against another build, such as a release.
fn hook_binary() -> std::ffi::OsString {
    std::env::var_os("CODEFLOW_JOURNEY_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into())
}

/// [`codeflow`] with [`hook_binary`] as the program, so the journey can
/// replay against another build.
fn journey_codeflow() -> Command {
    let base = codeflow();
    let mut cmd = Command::new(hook_binary());
    for (key, value) in base.get_envs() {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    cmd
}

/// `git` in `root` with the shipped pre-push shim installed.
fn hooked_git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_HOOK_BINARY", hook_binary())
        .env("CODEFLOW_HOME", isolated_home())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID");
    cmd
}

/// The fixture with a destination that holds `main` and the line, the
/// registry seeded and the pre-push shim installed, and the root checkout
/// switched to `main`, which does not hold the line task's record.
fn hooked_fixture() -> Fixture {
    let fixture = fixture();
    let root = fixture.root();
    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let remote = root.join(".git/destination.git");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "-q",
            "origin",
            "main",
            LINE,
        ],
    );
    let hooks = root.join(".git/codeflow-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let shim = hooks.join("pre-push");
    std::fs::write(
        &shim,
        include_str!("../../../../assets/base/git-hooks/pre-push"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    git(root, &["switch", "-q", "main"]);
    fixture
}

/// AC-4: the issue's case through the real pre-push hook. A push of the
/// line task's branch from the root checkout on `main`, which lacks the
/// task's record, goes through as the same push from the task's checkout
/// does.
#[test]
fn a_line_task_pushed_from_a_root_checkout_on_main_passes_the_hook() {
    let fixture = hooked_fixture();
    let root = fixture.root();
    assert!(!root.join(record_path("TSK-002")).exists());
    let out = hooked_git(root)
        .args(["push", "origin", &format!("{LINE_TASK}:{LINE_TASK}")])
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "the push from main: {printed}");
    assert!(
        printed.contains("TSK-002 criteria delta"),
        "the hook judged the task's own criteria change: {printed}"
    );
    let pushed = git_out(root, &["ls-remote", "origin", LINE_TASK]);
    assert!(pushed.starts_with(&fixture.task_head), "{pushed}");
}
