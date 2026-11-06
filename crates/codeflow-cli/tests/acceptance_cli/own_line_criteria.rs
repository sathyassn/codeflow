//! A task on an integration line changes its own criteria while `main`
//! holds a diverged copy of its record (TSK-350, issues #380, #420, #454).
//! Within the bounded exception (a confirmed `--into` line that the task's
//! record declares at the line's tip and at the head, and that passes the
//! trusted-line binding), the line's tip and the range base are the
//! criteria points for the task's own change; `main` stays the policy and
//! commit-history authority, and every point still counts for whether the
//! task's completion landed elsewhere.
use super::*;

const OWN: &str = "task/TSK-002-work";

/// How `main` and the line came to hold the task's record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// The line was cut, then `main` and the line each amended the task.
    Diverged,
    /// `main` and the line each planned the task on its own.
    PlannedOnEach,
    /// `main` is an ancestor of the line, which amended the task.
    MainAncestor,
    /// `main` moved on with unrelated commits after the line amended it.
    MainUnrelated,
}

/// The task's record targeting [`LINE`] with AC-1 reading `ac1`.
fn own_record(ac1: &str) -> String {
    line_task("TSK-002", "todo", "Pending.\n").replace("the system shall work.", ac1)
}

struct Repo {
    dir: tempfile::TempDir,
    line: String,
    main: String,
}

/// TSK-001 on `main` and the line, TSK-002 shaped by `shape`, then `OWN`
/// cut from the line changing its own AC-1.
fn shaped(shape: Shape) -> Repo {
    let policy = ", \"test_gate_on_push\": \"block\"";
    let dir = if shape == Shape::PlannedOnEach {
        line_repo_with(&["TSK-001"], policy)
    } else {
        line_repo_with(&["TSK-001", "TSK-002"], policy)
    };
    let root = dir.path();
    let path = record_path("TSK-002");
    match shape {
        Shape::Diverged => {
            write(root, &path, &own_record("the system shall work on main."));
            commit(root, "docs: amend TSK-002 on main");
            amend_on_line(root, &own_record("the system shall work on the line."));
        }
        Shape::PlannedOnEach => {
            write(root, &path, &own_record("the system shall work on main."));
            commit(root, "docs: plan TSK-002 on main");
            amend_on_line(root, &own_record("the system shall work on the line."));
        }
        Shape::MainAncestor => {
            amend_on_line(root, &own_record("the system shall work on the line."));
        }
        Shape::MainUnrelated => {
            amend_on_line(root, &own_record("the system shall work on the line."));
            git(root, &["switch", "-q", "main"]);
            write(root, "unrelated.txt", "unrelated\n");
            commit(root, "chore: unrelated work on main");
        }
    }
    let line = git_out(root, &["rev-parse", LINE]);
    let main = git_out(root, &["rev-parse", "main"]);
    git(root, &["switch", "-q", "-c", OWN, LINE]);
    write(
        root,
        &path,
        &own_record("the system shall work on the branch."),
    );
    commit(root, "docs: TSK-002 changes its own AC-1");
    Repo { dir, line, main }
}

/// Land `record` as TSK-002's record on [`LINE`] through a planning
/// branch merged with a merge commit, so the line stays a verified line.
fn amend_on_line(root: &Path, record: &str) {
    git(root, &["switch", "-q", "-C", "plan/amend-tsk-002", LINE]);
    write(root, &record_path("TSK-002"), record);
    commit(root, "docs(plan): amend TSK-002 on the line");
    land(root, "plan/amend-tsk-002");
}

/// `codeflow ci` over the branch as the pre-push hook hands it over when
/// `hook` (main's tip for policy and commits), with `--into` the line when
/// `into`.
fn judged(repo: &Repo, hook: bool, into: bool) -> (i32, String) {
    let mut args = vec![
        "ci", "--base", &repo.line, "--head", "HEAD", "--branch", OWN,
    ];
    if hook {
        args.extend(["--policy-from", &repo.main, "--commits-from", &repo.main]);
    }
    let into_arg = format!("--into={LINE}");
    if into {
        args.push(&into_arg);
    }
    let out = codeflow()
        .args(&args)
        .current_dir(repo.dir.path())
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

/// AC-7: the issue #380 cases through `codeflow ci`, printed for the
/// record. Run with `--nocapture` to read them.
#[test]
#[ignore = "AC-7 investigation; prints each case's verdict"]
fn issue_380_cases_print_their_verdicts() {
    for shape in [
        Shape::Diverged,
        Shape::PlannedOnEach,
        Shape::MainAncestor,
        Shape::MainUnrelated,
    ] {
        let repo = shaped(shape);
        for (hook, into) in [(false, false), (false, true), (true, false), (true, true)] {
            let (code, text) = judged(&repo, hook, into);
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| {
                    line.contains("criteria_frozen")
                        || line.contains("criteria delta")
                        || line.contains("criteria authority")
                })
                .collect();
            println!("{shape:?} hook={hook} into={into}: exit {code}");
            for line in lines {
                println!("    {line}");
            }
        }
    }
}

/// `git` with the shipped pre-push shim and the built binary as its hook
/// binary.
fn hooked_git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
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

/// A real `git push <args>` through the installed hook.
fn push(root: &Path, args: &[&str]) -> (i32, String) {
    let out = hooked_git(root).arg("push").args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Give `repo` a bare destination holding `main` and the line, seed the
/// id registry and install the shipped pre-push shim.
fn with_destination(repo: &Repo) {
    let root = repo.dir.path();
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
    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    // Two copies planned separately cannot be seeded as one record; the
    // hook then judges without a seeded registry.
    if !seed.status.success() {
        println!(
            "    ids seed: {}",
            String::from_utf8_lossy(&seed.stderr)
                .lines()
                .next()
                .unwrap_or("")
        );
    }
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
}

/// AC-7: the issue #380 cases through a real pre-push, printed.
#[test]
#[ignore = "AC-7 investigation; prints each case's verdict"]
fn issue_380_cases_print_their_pre_push_verdicts() {
    for shape in [
        Shape::Diverged,
        Shape::PlannedOnEach,
        Shape::MainAncestor,
        Shape::MainUnrelated,
    ] {
        let repo = shaped(shape);
        with_destination(&repo);
        let (code, text) = push(repo.dir.path(), &["origin", OWN]);
        println!("{shape:?} pre-push: exit {code}");
        for line in text.lines().filter(|line| {
            line.contains("criteria_frozen")
                || line.contains("criteria delta")
                || line.contains("criteria authority")
                || line.contains("judges")
        }) {
            println!("    {line}");
        }
    }
}

/// `codeflow ci` with `args` from `root`.
fn ci_args(root: &Path, args: &[&str]) -> (i32, String) {
    let out = codeflow()
        .arg("ci")
        .args(args)
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

/// The note `codeflow ci` prints when the own-line exception applies.
const GRANTED: &str = "changes its own criteria on its bound line";

/// The note both runs give for the task's own change (AC-6).
const OWN_DELTA: &str = "TSK-002 criteria delta: AC-1 changed";

/// AC-6 (issue #380): the task's own criteria change on its bound line
/// passes the real pre-push, on the first push and on a replacing push,
/// for the diverged copies and the copies planned on each. Before TSK-350
/// both refused with `work.criteria_frozen` (AC-7 findings).
#[test]
fn the_own_change_on_its_line_passes_the_pre_push() {
    for shape in [Shape::Diverged, Shape::PlannedOnEach] {
        let repo = shaped(shape);
        with_destination(&repo);
        let root = repo.dir.path();
        let (code, text) = push(root, &["origin", OWN]);
        assert_eq!(code, 0, "{shape:?} first push: {text}");
        assert!(text.contains(OWN_DELTA), "{shape:?} first push: {text}");
        write(
            root,
            &record_path("TSK-002"),
            &own_record("the system shall work on the rebuilt branch."),
        );
        git(root, &["commit", "-q", "-a", "--amend", "--no-edit"]);
        let (code, text) = push(root, &["--force-with-lease", "origin", OWN]);
        assert_eq!(code, 0, "{shape:?} replacing push: {text}");
        assert!(text.contains(OWN_DELTA), "{shape:?} replacing push: {text}");
    }
}

/// AC-6: `codeflow ci` given the hook's flags and `--into` the line gives
/// the pre-push verdict once the destination confirms the line. With no
/// destination to confirm the line, or without an explicit `--into` (the
/// run then infers the record's declared target), the exception does not
/// apply and the range refuses as before (TSK-350 Description: "Any other
/// range, including one with no `--into`, keeps today's rule").
#[test]
fn ci_into_the_confirmed_line_gives_the_pre_push_verdict() {
    let repo = shaped(Shape::Diverged);
    let (code, text) = judged(&repo, true, true);
    assert_eq!(code, 1, "no destination: {text}");
    assert!(
        text.contains("work.criteria_frozen"),
        "no destination: {text}"
    );
    assert!(!text.contains(GRANTED), "no destination: {text}");
    with_destination(&repo);
    let (code, text) = judged(&repo, true, true);
    assert_eq!(code, 0, "with --into: {text}");
    assert!(text.contains(GRANTED), "with --into: {text}");
    assert!(text.contains(OWN_DELTA), "with --into: {text}");
    let (code, text) = judged(&repo, true, false);
    assert_eq!(code, 1, "without --into: {text}");
    assert!(
        text.contains("work.criteria_frozen"),
        "without --into: {text}"
    );
    assert!(!text.contains(GRANTED), "without --into: {text}");
}

/// AC-6 (review 02, finding 1): the confirmed line tip stays a criteria
/// point when the run's base is older than it and the head contains it.
/// The line adds AC-3 and the branch cut from it drops AC-3: both bases
/// give one verdict and report the removal, whose reason the pull request
/// body must give. Before the fix the older base judged against its own
/// record only and reported no removal. Keeping AC-3 reports none.
#[test]
fn an_older_base_keeps_the_confirmed_tip_as_a_criteria_point() {
    let repo = shaped(Shape::Diverged);
    let root = repo.dir.path();
    let older = repo.line.clone();
    let ac3 = "the command shall succeed.\n- AC-3 When asked, the system shall report.\n";
    let with_ac3 = |ac1: &str| own_record(ac1).replace("the command shall succeed.\n", ac3);
    amend_on_line(root, &with_ac3("the system shall work on the line."));
    let tip = git_out(root, &["rev-parse", LINE]);
    git(root, &["switch", "-q", "-C", OWN, LINE]);
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch."),
    );
    commit(root, "docs: TSK-002 drops AC-3");
    with_destination(&repo);
    let into = format!("--into={LINE}");
    let run = |base: &str| {
        ci_args(
            root,
            &[
                "--base",
                base,
                "--head",
                "HEAD",
                "--branch",
                OWN,
                "--policy-from",
                &repo.main,
                "--commits-from",
                &repo.main,
                &into,
            ],
        )
    };
    let removed = "TSK-002 removes AC-3";
    let at_tip = run(&tip);
    assert!(at_tip.1.contains(removed), "the line tip: {}", at_tip.1);
    let from_older = run(&older);
    assert!(
        from_older.1.contains(removed),
        "an older base: {}",
        from_older.1
    );
    assert_eq!(at_tip.0, from_older.0, "an older base: {}", from_older.1);
    write(
        root,
        &record_path("TSK-002"),
        &with_ac3("the system shall work on the branch."),
    );
    commit(root, "docs: TSK-002 keeps AC-3");
    let (code, text) = run(&older);
    assert_eq!(code, 0, "own change from an older base: {text}");
    assert!(
        text.contains(OWN_DELTA),
        "own change from an older base: {text}"
    );
    assert!(
        !text.contains(removed),
        "own change from an older base: {text}"
    );
}

/// AC-8 (review 01, finding 1): the binding reads the pinned candidate
/// authority and the confirmed line tip, never a local or remote-tracking
/// ref. `main`'s pinned tip cancels the epic while `origin/main` is stale
/// and still shows it active: the exception is refused, through
/// `codeflow ci --into` and a real pre-push. A destination that does not
/// hold the line confirms nothing.
#[test]
fn a_stale_ref_cannot_bind_a_cancelled_epics_line() {
    let repo = shaped(Shape::Diverged);
    with_destination(&repo);
    let root = repo.dir.path();
    git(root, &["switch", "-q", "main"]);
    write(
        root,
        "project-management/epics/EPC-001.md",
        &EPIC.replace("status: planning", "status: cancelled"),
    );
    commit(root, "docs(plan): cancel EPC-001 on main");
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "-q",
            "origin",
            "main",
        ],
    );
    let cancelled = git_out(root, &["rev-parse", "main"]);
    // Leave `origin/main` at the active epic, as a clone that has not
    // fetched would.
    git(
        root,
        &["update-ref", "refs/remotes/origin/main", &repo.main],
    );
    git(root, &["switch", "-q", OWN]);
    let (code, text) = ci_args(
        root,
        &[
            "--base",
            &repo.line,
            "--head",
            "HEAD",
            "--branch",
            OWN,
            "--policy-from",
            &cancelled,
            "--commits-from",
            &cancelled,
            &format!("--into={LINE}"),
        ],
    );
    assert_ne!(code, 0, "ci with a stale origin/main: {text}");
    assert!(
        !text.contains(GRANTED),
        "ci with a stale origin/main: {text}"
    );
    assert!(
        text.contains("cancelled"),
        "ci with a stale origin/main: {text}"
    );
    let (code, text) = push(root, &["origin", OWN]);
    assert_ne!(code, 0, "pre-push with a stale origin/main: {text}");

    // Another destination that holds only `main`.
    let other = root.join(".git/other.git");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            other.to_str().unwrap(),
        ],
    );
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "-q",
            other.to_str().unwrap(),
            &format!("{}:refs/heads/main", repo.main),
        ],
    );
    let (code, text) = ci_args(
        root,
        &[
            "--base",
            &repo.line,
            "--head",
            "HEAD",
            "--branch",
            OWN,
            "--policy-from",
            &repo.main,
            "--commits-from",
            &repo.main,
            "--destination",
            other.to_str().unwrap(),
            &format!("--into={LINE}"),
        ],
    );
    assert_ne!(code, 0, "a destination without the line: {text}");
    assert!(
        !text.contains(GRANTED),
        "a destination without the line: {text}"
    );
}

/// AC-1 (review 01, finding 2): the checkout's policy file cannot change
/// the criteria verdict. An uncommitted policy that drops the `task`
/// prefix leaves the identical invocation's criteria verdict as it was.
#[test]
fn the_checkout_policy_cannot_change_the_own_line_verdict() {
    let repo = shaped(Shape::Diverged);
    with_destination(&repo);
    let root = repo.dir.path();
    let policy = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    write(
        root,
        ".codeflow/policy.json",
        &policy.replacen(
            "\"product_paths\"",
            "\"branch_prefixes\": [\"fix\", \"plan\"], \"product_paths\"",
            1,
        ),
    );
    // The criteria verdict is the one this invocation gives with the
    // committed policy. The work-start record check still reads the
    // checkout's policy and refuses here; that read can only refuse, and
    // is outside the criteria path.
    let (_, text) = judged(&repo, true, true);
    assert!(
        text.contains(GRANTED),
        "a checkout policy without task/: {text}"
    );
    assert!(
        text.contains(OWN_DELTA),
        "a checkout policy without task/: {text}"
    );
    assert!(
        !text.contains("BLOCKED — lifecycle invariant work.criteria_frozen"),
        "a checkout policy without task/: {text}"
    );
}

/// AC-10 on a task branch: TSK-451's rule still refuses a task pull
/// request that edits another task's record, but with its own message, not
/// the competing-versions refusal AC-10 removes.
#[test]
fn a_task_branch_editing_another_record_meets_the_ownership_rule() {
    let repo = shaped(Shape::Diverged);
    let root = repo.dir.path();
    git(root, &["switch", "-q", "-c", "task/TSK-001-tidy", LINE]);
    write(root, "src/tidy.rs", "// tidy\n");
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the line.").replace("Work.\n", "Work, described.\n"),
    );
    commit(root, "fix: tidy TSK-002's description");
    let (code, text) = ci_args(
        root,
        &[
            "--base",
            &repo.line,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-tidy",
            "--policy-from",
            &repo.main,
            "--commits-from",
            &repo.main,
        ],
    );
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("TSK-002 changes its record"), "{text}");
    assert!(!text.contains("none of them is newer"), "{text}");
}

/// AC-8: the exception never reaches a range outside its scope. Each case
/// is the diverged shape with one thing changed, pushed for real.
#[test]
fn the_exception_stays_inside_its_scope() {
    // The branch's record no longer declares the line.
    let repo = shaped(Shape::Diverged);
    with_destination(&repo);
    let root = repo.dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch.").replace(
            &format!("integration_target: {LINE}"),
            "integration_target: main",
        ),
    );
    commit(root, "docs: retarget TSK-002 to main");
    let (code, text) = push(root, &["origin", OWN]);
    assert_ne!(code, 0, "retargeted to main: {text}");
    assert!(text.contains("work.criteria_frozen"), "retargeted: {text}");
    assert!(!text.contains(GRANTED), "retargeted: {text}");

    // The branch also changes another task's criteria.
    let repo = shaped(Shape::Diverged);
    with_destination(&repo);
    let root = repo.dir.path();
    write(
        root,
        &record_path("TSK-001"),
        &line_task("TSK-001", "todo", "Pending.\n")
            .replace("the system shall work.", "the system shall work less."),
    );
    commit(root, "docs: change TSK-001 too");
    let (code, text) = push(root, &["origin", OWN]);
    assert_ne!(code, 0, "another task's criteria: {text}");
    assert!(text.contains("TSK-001"), "another task's criteria: {text}");
}

/// AC-8: a completion of the task that `main` holds still counts, so the
/// own change refuses as before.
#[test]
fn a_completion_on_main_still_refuses_the_own_change() {
    let dir = line_repo_with(
        &["TSK-001", "TSK-002"],
        ", \"test_gate_on_push\": \"block\"",
    );
    let root = dir.path();
    git(root, &["switch", "-q", LINE]);
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the line."),
    );
    commit(root, "docs: amend TSK-002 on the line");
    git(root, &["switch", "-q", "main"]);
    write(root, "src/two.rs", "// two\n");
    let built = commit(root, "feat: TSK-002 built on main");
    write(
        root,
        &record_path("TSK-002"),
        &line_task("TSK-002", "complete", &valid_block(&built))
            .replace("the system shall work.", "the system shall work on main."),
    );
    commit(root, "docs: TSK-002 completed on main");
    let repo = Repo {
        line: git_out(root, &["rev-parse", LINE]),
        main: git_out(root, &["rev-parse", "main"]),
        dir,
    };
    let root = repo.dir.path();
    git(root, &["switch", "-q", "-c", OWN, LINE]);
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch."),
    );
    commit(root, "docs: TSK-002 changes its own AC-1");
    with_destination(&repo);
    let (code, text) = push(root, &["origin", OWN]);
    assert_ne!(code, 0, "completion on main: {text}");
    assert!(
        text.contains("work.criteria_frozen"),
        "completion on main: {text}"
    );
}

/// The issue #454 shape: `main` holds an older copy of TSK-002 targeting
/// `main`, and the line holds an amended copy targeting the line; `main`
/// then moved on, so it is not an ancestor of the line. With `synced` the
/// line merged `main`'s copy before amending it (the PR 453 workaround);
/// without, the copies were made in parallel, as TSK-1036's were on this
/// repository's line. The branch completes the task and amends one of its
/// own criteria. Returns the `codeflow ci --into` and pre-push verdicts.
fn issue_454(synced: bool) -> ((i32, String), (i32, String)) {
    let dir = line_repo_with(
        &["TSK-001", "TSK-002"],
        ", \"test_gate_on_push\": \"block\"",
    );
    let root = dir.path();
    // `main`'s copy is a criteria version of its own, as #454's AC-1 to
    // AC-9 against the line's AC-1 to AC-10.
    let older = own_record("the system shall work.")
        .replace(
            &format!("integration_target: {LINE}"),
            "integration_target: main",
        )
        .replace("the command shall succeed.", "the command shall finish.");
    write(root, &record_path("TSK-002"), &older);
    commit(root, "docs: TSK-002 targets main");
    git(root, &["switch", "-q", LINE]);
    if synced {
        git(
            root,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "merge: main into the line",
                "main",
            ],
        );
    }
    let amended = |ac1: &str| {
        own_record(ac1).replace(
            "the command shall succeed.",
            "the command shall succeed twice.",
        )
    };
    write(
        root,
        &record_path("TSK-002"),
        &amended("the system shall work."),
    );
    commit(root, "docs: the line amends TSK-002");
    git(root, &["switch", "-q", "main"]);
    write(root, "unrelated.txt", "unrelated\n");
    commit(root, "chore: unrelated work on main");
    let repo = Repo {
        line: git_out(root, &["rev-parse", LINE]),
        main: git_out(root, &["rev-parse", "main"]),
        dir,
    };
    let root = repo.dir.path();
    git(root, &["switch", "-q", "-c", OWN, LINE]);
    write(root, "src/two.rs", "// two\n");
    write(
        root,
        &record_path("TSK-002"),
        &amended("the system shall work on the branch."),
    );
    let built = commit(root, "feat: TSK-002 amends its own AC-1");
    write(
        root,
        &record_path("TSK-002"),
        &amended("the system shall work on the branch.")
            .replace("status: todo", "status: complete")
            .replace("Pending.\n", &valid_block(&built)),
    );
    commit(root, "docs: TSK-002 is complete");
    with_destination(&repo);
    (judged(&repo, true, true), push(root, &["origin", OWN]))
}

/// AC-13 (issue #454): the pre-push and `codeflow ci --into` judge the
/// task's own change against the line's copy, whether or not the line
/// merged `main`'s older copy first. Before TSK-350 the parallel copies
/// refused with `work.criteria_frozen`.
#[test]
fn an_older_main_copy_the_line_amended_is_not_a_competitor() {
    for synced in [false, true] {
        let ((code, text), (pushed, pushed_text)) = issue_454(synced);
        assert_eq!(code, 0, "synced={synced} ci --into: {text}");
        assert_eq!(pushed, 0, "synced={synced} pre-push: {pushed_text}");
    }
}

/// AC-13: a range that changes the criteria of another task's record
/// against a competing copy the line does not supersede still refuses.
#[test]
fn a_competing_copy_the_line_does_not_supersede_still_refuses() {
    let repo = other_record_range(|text| text.replace("work on the line.", "work on a line."));
    let (code, text) = judged_other(&repo);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains(FROZEN_BLOCK), "{text}");
}

/// A branch with no task, cut from the line in the diverged shape, that
/// rewrites TSK-002's record with `edit` applied to the line's copy.
fn other_record_range(edit: impl Fn(String) -> String) -> Repo {
    let repo = shaped(Shape::Diverged);
    let root = repo.dir.path();
    git(root, &["switch", "-q", "-c", "fix/tidy-records", LINE]);
    write(root, "src/tidy.rs", "// tidy\n");
    write(
        root,
        &record_path("TSK-002"),
        &edit(own_record("the system shall work on the line.")),
    );
    commit(root, "fix: tidy the records");
    repo
}

/// `codeflow ci` over the no-task branch with the hook's flags.
fn judged_other(repo: &Repo) -> (i32, String) {
    ci_args(
        repo.dir.path(),
        &[
            "--base",
            &repo.line,
            "--head",
            "HEAD",
            "--branch",
            "fix/tidy-records",
            "--policy-from",
            &repo.main,
            "--commits-from",
            &repo.main,
        ],
    )
}

/// The refusal AC-10 to AC-12 are about.
const FROZEN_BLOCK: &str = "BLOCKED — lifecycle invariant work.criteria_frozen";

/// AC-10 (issue #420): a range that changes another task's record outside
/// its criteria, while `main` and the line hold diverged criteria for it,
/// is not refused with `work.criteria_frozen`.
#[test]
fn another_records_description_passes_beside_diverged_copies() {
    let repo = other_record_range(|text| text.replace("Work.\n", "Work, described.\n"));
    let (code, text) = judged_other(&repo);
    assert!(!text.contains(FROZEN_BLOCK), "{text}");
    assert_eq!(code, 0, "{text}");
    with_destination(&repo);
    let (code, text) = push(repo.dir.path(), &["origin", "fix/tidy-records"]);
    assert!(!text.contains("work.criteria_frozen"), "pre-push: {text}");
    assert_eq!(code, 0, "pre-push: {text}");
}

/// AC-11 (issue #420): the same range still refuses when it changes the
/// record's criteria by one AC word, an amendment note inside an AC item
/// or a tag.
#[test]
fn another_records_criteria_change_still_refuses() {
    type Edit = fn(String) -> String;
    let edits: [(&str, Edit); 3] = [
        ("one AC word", |text| {
            text.replace("work on the line.", "work on the lines.")
        }),
        ("an amendment note inside an AC item", |text| {
            text.replace(
                "work on the line.\n",
                "work on the line.\n  Amended 2026-11-01: a note.\n",
            )
        }),
        ("a tag", |text| {
            text.replace("- AC-1 When run,", "- AC-1 (journey) When run,")
        }),
    ];
    for (what, edit) in edits {
        let repo = other_record_range(edit);
        let (code, text) = judged_other(&repo);
        assert_eq!(code, 1, "{what}: {text}");
        assert!(text.contains(FROZEN_BLOCK), "{what}: {text}");
    }
}

/// AC-12 (issue #420): the check is skipped only when the head's criteria
/// equal the range base's; a head that equals `main`'s copy but not the
/// base's is a criteria change and refuses.
#[test]
fn equal_to_mains_copy_is_still_a_change() {
    let repo = other_record_range(|_| own_record("the system shall work on main."));
    let (code, text) = judged_other(&repo);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains(FROZEN_BLOCK), "{text}");
}

/// The line-to-main range of `shape`, after the line merged `main` and
/// resolved TSK-002's record to `side` (`--ours` keeps the line's copy,
/// `--theirs` main's) when `side` is given, else with no merge yet.
fn line_to_main(shape: Shape, side: Option<&str>) -> (i32, String) {
    let repo = shaped(shape);
    let root = repo.dir.path();
    git(root, &["switch", "-q", LINE]);
    if let Some(side) = side {
        let merged = Command::new("git")
            .args([
                "merge",
                "--no-ff",
                "-m",
                "merge: main into the line",
                "main",
            ])
            .current_dir(root)
            .output()
            .unwrap();
        if !merged.status.success() {
            git(root, &["checkout", side, "--", &record_path("TSK-002")]);
            git(root, &["add", "-A"]);
            git(root, &["commit", "-q", "--no-edit"]);
        }
    }
    ci_args(
        root,
        &[
            "--base",
            "main",
            "--head",
            LINE,
            "--branch",
            LINE,
            "--pr-body",
            "## Summary\nThe line lands.\n\n- one change\n\nTask: EPC-001\n\n## Changes\n- one\n\n## Testing\n- test\n",
        ],
    )
}

/// AC-9: a line-to-main range that merges a task record whose criteria
/// `main` and the line each amended reports both versions and the one the
/// merge kept, as a note, whichever side the merge kept; a pending merge
/// that conflicts names the person who resolves it; a record only the
/// line amended reports nothing new.
#[test]
fn the_line_to_main_range_reports_competing_copies() {
    for side in ["--ours", "--theirs"] {
        let (_, text) = line_to_main(Shape::Diverged, Some(side));
        let note: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("work.criteria_copies"))
            .collect();
        assert_eq!(note.len(), 1, "{side}: {text}");
        assert!(note[0].contains("note:"), "{side}: {text}");
        assert!(note[0].contains("TSK-002"), "{side}: {text}");
        assert!(note[0].contains("and the merge "), "{side}: {text}");
    }
    let (_, text) = line_to_main(Shape::Diverged, None);
    assert!(text.contains("work.criteria_copies"), "pending: {text}");
    assert!(
        text.contains("the person who resolves it chooses"),
        "pending: {text}"
    );
    let (_, text) = line_to_main(Shape::MainAncestor, None);
    assert!(
        !text.contains("work.criteria_copies"),
        "only the line: {text}"
    );
}

/// TSK-002 planned only on the line, so `main`'s checkout lacks its
/// record, and `OWN` changing its own AC-1.
fn line_only() -> Repo {
    let dir = line_repo_with(&["TSK-001"], ", \"test_gate_on_push\": \"block\"");
    let root = dir.path();
    amend_on_line(root, &own_record("the system shall work on the line."));
    let repo = Repo {
        line: git_out(root, &["rev-parse", LINE]),
        main: git_out(root, &["rev-parse", "main"]),
        dir,
    };
    let root = repo.dir.path();
    git(root, &["switch", "-q", "-c", OWN, LINE]);
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch."),
    );
    commit(root, "docs: TSK-002 changes its own AC-1");
    repo
}

/// `codeflow ci` over `OWN`'s head with the hook's explicit inputs, run
/// from whatever the checkout is.
fn judged_head(repo: &Repo, head: &str) -> (i32, String) {
    ci_args(
        repo.dir.path(),
        &[
            "--base",
            &repo.line,
            "--head",
            head,
            "--branch",
            OWN,
            "--policy-from",
            &repo.main,
            "--commits-from",
            &repo.main,
            &format!("--into={LINE}"),
        ],
    )
}

/// The verdict lines a checkout could change: the exit code and the
/// criteria lines.
fn verdict(result: &(i32, String)) -> (i32, Vec<String>) {
    (
        result.0,
        result
            .1
            .lines()
            .filter(|line| line.contains("criteria") || line.contains("BLOCKED"))
            .map(str::to_owned)
            .collect(),
    )
}

/// The pull request body of a classed `TSK-002` run.
const CLASSED_BODY: &str = "## Summary\nTSK-002 changes its own AC-1.\n\n- one change\n\nTask: TSK-002\n\n## Changes\n- one\n\n## Testing\n- test\n";

/// [`judged_head`] as a classed task pull request, with [`CLASSED_BODY`].
fn judged_classed(repo: &Repo, head: &str) -> (i32, String) {
    ci_args(
        repo.dir.path(),
        &[
            "--base",
            &repo.line,
            "--head",
            head,
            "--branch",
            OWN,
            "--policy-from",
            &repo.main,
            "--commits-from",
            &repo.main,
            &format!("--into={LINE}"),
            "--pr-body",
            CLASSED_BODY,
        ],
    )
}

/// The lines of `text` that refuse the own change as frozen criteria.
fn frozen_refusals(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.contains("BLOCKED") && line.contains("criteria_frozen"))
        .collect()
}

/// AC-1 and AC-3 (journey, issue 221): the same pushed head, judged with
/// identical explicit inputs from `main` (which lacks the record), a
/// detached checkout and the task's own checkout, gets one verdict from
/// `codeflow ci` without a body, one from a classed task pull request run
/// with a body naming the task, and one from a pre-push dry run: the own
/// change passes each time.
#[test]
fn the_verdict_does_not_depend_on_the_checkout() {
    let repo = line_only();
    with_destination(&repo);
    let root = repo.dir.path();
    let head = git_out(root, &["rev-parse", OWN]);
    let mut verdicts = Vec::new();
    let mut classed_verdicts = Vec::new();
    for checkout in ["main", "--detach", OWN] {
        if checkout == "--detach" {
            git(root, &["switch", "-q", "--detach", &head]);
        } else {
            git(root, &["switch", "-q", checkout]);
        }
        let judged = judged_head(&repo, &head);
        assert_eq!(judged.0, 0, "ci from {checkout}: {}", judged.1);
        assert!(
            judged.1.contains(OWN_DELTA),
            "ci from {checkout}: {}",
            judged.1
        );
        verdicts.push(verdict(&judged));
        let classed = judged_classed(&repo, &head);
        assert!(
            classed.1.contains(OWN_DELTA),
            "classed ci from {checkout}: {}",
            classed.1
        );
        assert!(
            frozen_refusals(&classed.1).is_empty(),
            "classed ci from {checkout}: {}",
            classed.1
        );
        assert!(
            classed
                .1
                .contains("pull request class: tracked TSK-002 (from the Task: line)"),
            "classed ci from {checkout}: {}",
            classed.1
        );
        assert!(
            !classed
                .1
                .lines()
                .any(|line| line.contains("BLOCKED") && line.contains("work.classification")),
            "classed ci from {checkout}: {}",
            classed.1
        );
        classed_verdicts.push(verdict(&classed));
        let (code, text) = push(
            root,
            &["--dry-run", "origin", &format!("{head}:refs/heads/{OWN}")],
        );
        assert_eq!(code, 0, "pre-push dry run from {checkout}: {text}");
        assert!(
            text.contains(OWN_DELTA),
            "pre-push dry run from {checkout}: {text}"
        );
    }
    assert!(
        verdicts.windows(2).all(|pair| pair[0] == pair[1]),
        "{verdicts:#?}"
    );
    assert!(
        classed_verdicts.windows(2).all(|pair| pair[0] == pair[1]),
        "{classed_verdicts:#?}"
    );
}

/// AC-2: authority comes from the judged head and the policy the run
/// judges with, never from the checkout. Each negative changes TSK-001's
/// criteria on the line, which only TSK-001's own range may do.
#[test]
fn the_checkout_grants_no_own_task_authority() {
    let repo = line_only();
    let root = repo.dir.path();
    let changed = line_task("TSK-001", "todo", "Pending.\n")
        .replace("the system shall work.", "the system shall work less.");
    let refused = |result: &(i32, String), what: &str| {
        assert_ne!(result.0, 0, "{what}: {}", result.1);
        assert!(result.1.contains("BLOCKED"), "{what}: {}", result.1);
        assert!(
            !result
                .1
                .contains("work.criteria_frozen: TSK-001 criteria delta"),
            "{what} took TSK-001's own authority: {}",
            result.1
        );
    };

    // A record only the checkout holds: the branch names TSK-009, whose
    // record is an uncommitted file.
    git(root, &["switch", "-q", "-c", "task/TSK-009-work", LINE]);
    write(root, "src/nine.rs", "// nine\n");
    write(root, &record_path("TSK-001"), &changed);
    commit(root, "feat: change TSK-001 with the code");
    write(
        root,
        &record_path("TSK-009"),
        &line_task("TSK-009", "todo", "Pending.\n"),
    );
    let forged = |branch: &str| {
        ci_args(
            root,
            &[
                "--base",
                &repo.line,
                "--head",
                "HEAD",
                "--branch",
                branch,
                "--policy-from",
                &repo.main,
                "--commits-from",
                &repo.main,
            ],
        )
    };
    refused(&forged("task/TSK-009-work"), "a checkout-only record");
    std::fs::remove_file(root.join(record_path("TSK-009"))).unwrap();

    // A forged identity: the branch claims TSK-002 but changes TSK-001.
    git(root, &["branch", "-q", "task/TSK-002-forged"]);
    refused(&forged("task/TSK-002-forged"), "another task's change");

    // An invocation-local policy in the checkout that adds a prefix makes
    // no branch carry TSK-001.
    let policy = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    write(
        root,
        ".codeflow/policy.json",
        &policy.replacen(
            "\"product_paths\"",
            "\"branch_prefixes\": [\"own\", \"task\", \"fix\", \"plan\"], \"product_paths\"",
            1,
        ),
    );
    refused(&forged("own/TSK-001-work"), "an invocation-local prefix");
    git(root, &["checkout", "--", ".codeflow/policy.json"]);

    // An unreadable judged head.
    let unreadable = ci_args(
        root,
        &[
            "--base",
            &repo.line,
            "--head",
            "0123456789012345678901234567890123456789",
            "--branch",
            "task/TSK-001-work",
        ],
    );
    assert_ne!(unreadable.0, 0, "an unreadable head: {}", unreadable.1);
}

/// TSK-002 completed at C1 on `OWN`, pushed, then rebuilt at C2 with a
/// change to its own AC-1 and a fresh completion. With `copy`, C1 is
/// also pushed as another branch.
fn rebuilt(copy: bool) -> Repo {
    let repo = line_only();
    let root = repo.dir.path();
    git(root, &["reset", "-q", "--hard", &repo.line]);
    write(root, "src/two.rs", "// two\n");
    let built = commit(root, "feat: build TSK-002");
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the line.")
            .replace("status: todo", "status: complete")
            .replace("Pending.\n", &valid_block(&built)),
    );
    commit(root, "docs: TSK-002 is complete");
    with_destination(&repo);
    let (code, text) = push(root, &["origin", OWN]);
    assert_eq!(code, 0, "the first push: {text}");
    if copy {
        let (code, text) = push(
            root,
            &["origin", &format!("{OWN}:refs/heads/task/TSK-002-copy")],
        );
        assert_eq!(code, 0, "the copy: {text}");
    }
    git(root, &["reset", "-q", "--hard", &repo.line]);
    write(root, "src/two.rs", "// two, rebuilt\n");
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch."),
    );
    let rebuilt = commit(root, "feat: rebuild TSK-002");
    write(
        root,
        &record_path("TSK-002"),
        &own_record("the system shall work on the branch.")
            .replace("status: todo", "status: complete")
            .replace("Pending.\n", &valid_block(&rebuilt)),
    );
    commit(root, "docs: TSK-002 is complete again");
    repo
}

/// AC-4 (issue #304): the replacing push's own remote-tracking ref holds
/// the old completion; it does not make the landing unknown, so the own
/// change passes. A completion another branch holds still refuses.
#[test]
fn a_replacing_push_sets_aside_the_head_it_replaces() {
    let repo = rebuilt(false);
    let (code, text) = push(repo.dir.path(), &["--force-with-lease", "origin", OWN]);
    assert_eq!(code, 0, "the replacing push: {text}");
    let repo = rebuilt(true);
    let (code, text) = push(repo.dir.path(), &["--force-with-lease", "origin", OWN]);
    assert_ne!(code, 0, "the completion on another branch: {text}");
    assert!(text.contains("work.criteria_frozen"), "{text}");
}

/// AC-5 (issue #354 item 4): `codeflow ci` before the replacing push
/// judges the same way: the judged branch's own remote-tracking ref is set
/// aside, another branch's is not. The run names the line with `--into`, as
/// a bare run on a task branch whose line is fetched does (TSK-1155), and
/// no `--policy-from`, so the own-line exception plays no part.
#[test]
fn ci_before_the_replacing_push_sets_aside_the_same_head() {
    let into = format!("--into={LINE}");
    let ci = |repo: &Repo| {
        ci_args(
            repo.dir.path(),
            &[
                "--base", &repo.line, "--head", "HEAD", "--branch", OWN, &into,
            ],
        )
    };
    let repo = rebuilt(false);
    let (code, text) = ci(&repo);
    assert_eq!(code, 0, "before the push: {text}");
    let repo = rebuilt(true);
    let (code, text) = ci(&repo);
    assert_ne!(code, 0, "the completion on another branch: {text}");
    assert!(text.contains("work.criteria_frozen"), "{text}");
}
