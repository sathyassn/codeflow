//! Journeys (TSK-110 AC-4, SPC-013 R-104) with the installed CLI: the binary
//! Cargo built is first on `PATH`, so the git hooks the scaffold installs run
//! it as an adopter's would.
//!
//! - A standard-tier project without records meets no record rule; once it
//!   keeps a record, the rule judges that record.
//! - Repeated `codeflow update` changes nothing; a managed file removed out of
//!   band comes back on the next update, and the update after that is again a
//!   no-op.
//! - A project with no remote runs from plan to a green CI: allocate, land the
//!   plan, claim, start, build, complete and judge the PR, all offline. A
//!   task whose dependency is not complete cannot start.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const LINE: &str = "integration/EPC-001-offline";

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed:\n{}", text(out));
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn refused(out: &Output, what: &str, reason: &str) -> String {
    let all = text(out);
    assert!(!out.status.success(), "{what} passed:\n{all}");
    assert!(
        all.contains(reason),
        "{what} refused without {reason:?}:\n{all}"
    );
    all
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

fn project(tier: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(&codeflow(&root, &["init", "--yes", tier]), "init");
    (dir, root)
}

/// The scaffold's pull request template filled in for `task`.
fn body(root: &Path, task: &str) -> String {
    std::fs::read_to_string(root.join(".github/pull_request_template.md"))
        .unwrap()
        .replace("Task: `TSK-NNN | none: <reason>`", &format!("Task: {task}"))
        .replace(
            "Task: `TSK-NNN | EPC-NNN | EPC-001, EPC-002 | <unit name>`",
            &format!("Task: {task}"),
        )
        .replace(
            "## Summary\n",
            "## Summary\n\nBuild the task.\n\n- build the task\n",
        )
        .replace("\n-\n", "\n- Build the task.\n")
        .replace(
            "- Revision and command:",
            "- Revision and command: HEAD, cargo test",
        )
        .replace(
            "(paste the real test summary output here)",
            "Ran 1 test\nOK",
        )
        .replace("- Coverage:", "- Coverage: not measured for this journey")
        .replace("- New tests:", "- New tests: none")
        .replace("- Not tested:", "- Not tested: Windows")
}

fn ci(root: &Path, base: &str, branch: &str, body: Option<&str>) -> Output {
    let mut args = vec!["ci", "--base", base, "--head", "HEAD", "--branch", branch];
    if let Some(body) = body {
        args.extend(["--pr-body", body]);
    }
    codeflow(root, &args)
}

#[test]
fn a_standard_tier_project_meets_no_record_rule_until_it_keeps_records() {
    let (_dir, root) = project("--standard");
    let base = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", "feat/notes"]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    commit(&root, "feat: add notes");

    // Control: no records, so no record rule speaks.
    let passed = ok(&ci(&root, &base, "feat/notes", None), "ci without records");
    assert!(!passed.contains("work-records"), "{passed}");
    let validate = codeflow(&root, &["validate", "--docs"]);
    ok(&validate, "validate --docs without records");
    assert!(!text(&validate).contains("warning"), "{}", text(&validate));

    // Fault: the project now keeps a record, and a broken one. The record
    // rule judges it, so the absence above was the rule's scope, not a
    // silent rule.
    std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
    std::fs::write(
        root.join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\ntitle: \"Broken\"\nstatus: finished\n---\n\n# TSK-001: Broken\n",
    )
    .unwrap();
    commit(&root, "feat: keep a record");
    refused(
        &codeflow(&root, &["validate", "--docs"]),
        "validate --docs with a broken record",
        "TSK-001",
    );
}

/// The verb `codeflow update` reports for `path` (`unchanged`, `restored`, ...).
fn update_verb(out: &str, path: &str) -> String {
    out.lines()
        .map(str::split_whitespace)
        .find_map(|mut words| {
            let verb = words.next()?;
            (words.next() == Some(path)).then(|| verb.to_string())
        })
        .unwrap_or_else(|| panic!("update does not name {path}:\n{out}"))
}

#[test]
fn repeated_update_changes_nothing_and_restores_a_managed_file_changed_out_of_band() {
    let (_dir, root) = project("--full");
    let managed = ".codeflow/rules/writing.md";
    let original = std::fs::read(root.join(managed)).unwrap();
    let pristine = git(&root, &["status", "--porcelain"]);

    // Control: two updates in a row change no file.
    for run in 1..=2 {
        let out = ok(&codeflow(&root, &["update"]), "update");
        assert_eq!(update_verb(&out, managed), "unchanged", "update {run}");
        assert_eq!(
            git(&root, &["status", "--porcelain"]),
            pristine,
            "update {run} changed the tree"
        );
    }

    // Fault: a managed file removed out of band. The next update puts it
    // back as shipped, so the no-op above is not an update that does
    // nothing; the update after that is again a no-op.
    std::fs::remove_file(root.join(managed)).unwrap();
    let restored = ok(&codeflow(&root, &["update"]), "update after a removal");
    assert_ne!(update_verb(&restored, managed), "unchanged", "{restored}");
    assert_eq!(std::fs::read(root.join(managed)).unwrap(), original);
    assert_eq!(git(&root, &["status", "--porcelain"]), pristine);
    let again = ok(&codeflow(&root, &["update"]), "update after the restore");
    assert_eq!(update_verb(&again, managed), "unchanged", "{again}");
    assert_eq!(git(&root, &["status", "--porcelain"]), pristine);
}

#[test]
#[allow(clippy::too_many_lines)] // One journey from plan to a green CI, in order.
fn a_project_without_a_remote_runs_from_plan_to_a_green_ci_offline() {
    let (dir, root) = project("--full");
    assert_eq!(git(&root, &["remote"]), "", "the project has no remote");
    let default = git(&root, &["branch", "--show-current"]);
    git(&root, &["branch", LINE, &default]);
    ok(&codeflow(&root, &["ids", "seed"]), "ids seed");

    // Plan: an epic line with two tasks, the second depending on the first,
    // allocated from the local registry and landed on the line.
    git(&root, &["switch", "-q", "-c", "plan/offline", LINE]);
    ok(&codeflow(&root, &["epic", "new", "Offline"]), "epic new");
    for title in ["First", "Second"] {
        ok(
            &codeflow(
                &root,
                &["task", "new", "--epic", "EPC-001", "--into", LINE, title],
            ),
            "task new",
        );
    }
    let criterion = "- AC-1 When run, the system shall work.\n";
    edit(
        &root,
        "project-management/epics/EPC-001.md",
        "- AC-1\n",
        criterion,
    );
    for task in ["TSK-001", "TSK-002"] {
        edit(
            &root,
            &format!("project-management/tasks/{task}.md"),
            "- AC-1\n",
            &format!("{criterion}- AC-2 (journey) On a fresh project, the flow shall pass.\n"),
        );
    }
    edit(
        &root,
        "project-management/tasks/TSK-002.md",
        "depends_on: []",
        "depends_on: [TSK-001]",
    );
    commit(&root, "chore: plan the offline line");
    let plan = ci(&root, LINE, "plan/offline", None);
    ok(&plan, "ci on the planning branch");
    let plan = text(&plan);
    assert!(plan.contains("codeflow ci: clean"), "{plan}");
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/offline",
            "-m",
            "chore: land the plan",
        ],
    );

    // Read: one ready task and one waiting, from local refs only.
    let next = ok(&codeflow(&root, &["work", "next"]), "work next");
    assert!(next.contains("ready    TSK-001"), "{next}");
    assert!(next.contains("waiting  TSK-002"), "{next}");

    // Fault: the dependent task cannot start while TSK-001 is open.
    git(&root, &["switch", "-q", "-c", "task/TSK-002-second", LINE]);
    refused(
        &codeflow(&root, &["work", "start", "TSK-002"]),
        "work start of a task with an open dependency",
        "TSK-001",
    );
    git(&root, &["switch", "-q", LINE]);
    git(&root, &["branch", "-q", "-D", "task/TSK-002-second"]);

    // Control: claim (a local branch, no remote), start, build, complete and
    // a green CI verdict on the task's pull request.
    let claim = ok(
        &codeflow(&root, &["work", "claim", "TSK-001"]),
        "work claim",
    );
    let branch = git(
        &root,
        &[
            "branch",
            "--list",
            "task/TSK-001-*",
            "--format=%(refname:short)",
        ],
    );
    assert!(branch.starts_with("task/TSK-001-"), "{claim}");
    git(&root, &["switch", "-q", &branch]);
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn first() {}\n").unwrap();
    let reviewed = commit(&root, "feat: add the first function");
    let acceptance = dir.path().join("acceptance.yaml");
    std::fs::write(
        &acceptance,
        format!(
            "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | the journey ran\n    AC-2: verified | the journey ran\n  journey: verified | crates/codeflow-cli/tests/installed_cli_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
        ),
    )
    .unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                acceptance.to_str().unwrap(),
            ],
        ),
        "task status complete",
    );
    commit(&root, "chore: complete TSK-001");
    let body = body(&root, "TSK-001");
    ok(
        &ci(&root, LINE, &branch, Some(&body)),
        "ci on the completed task",
    );

    // Land it; the dependent task is now ready, still with no remote.
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            &branch,
            "-m",
            "chore: land TSK-001",
        ],
    );
    let next = ok(
        &codeflow(&root, &["work", "next"]),
        "work next after landing",
    );
    assert!(next.contains("ready    TSK-002"), "{next}");
    assert_eq!(git(&root, &["remote"]), "", "still no remote");
}
