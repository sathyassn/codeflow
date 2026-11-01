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
            git(root, &["switch", "-q", LINE]);
            write(
                root,
                &path,
                &own_record("the system shall work on the line."),
            );
            commit(root, "docs: amend TSK-002 on the line");
        }
        Shape::PlannedOnEach => {
            write(root, &path, &own_record("the system shall work on main."));
            commit(root, "docs: plan TSK-002 on main");
            git(root, &["switch", "-q", LINE]);
            write(
                root,
                &path,
                &own_record("the system shall work on the line."),
            );
            commit(root, "docs: plan TSK-002 on the line");
        }
        Shape::MainAncestor => {
            git(root, &["switch", "-q", LINE]);
            write(
                root,
                &path,
                &own_record("the system shall work on the line."),
            );
            commit(root, "docs: amend TSK-002 on the line");
        }
        Shape::MainUnrelated => {
            git(root, &["switch", "-q", LINE]);
            write(
                root,
                &path,
                &own_record("the system shall work on the line."),
            );
            commit(root, "docs: amend TSK-002 on the line");
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
