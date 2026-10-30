//! Journey (TSK-338, sathyassn/codeflow#182): a repository whose default
//! branch already exists, as a new hosted repository starts with an
//! "Initial commit", adopts `CodeFlow` at the full tier in one task's pull
//! request. The task branch passes the installed pre-push hook, the task
//! completes on its own branch, and `codeflow ci` judges the pull request
//! into `main` clean, while `main` itself is never pushed to. Negative
//! controls: a default branch whose history once carried a policy that was
//! removed, and a clone too shallow to prove the policy never existed, are
//! still refused. The binary under test is the one Cargo built; the
//! installed `codeflow` on `PATH` is never used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

const BRANCH: &str = "task/TSK-001-adopt";
const TASK: &str = "project-management/tasks/TSK-001.md";
const ADR: &str = "docs/decisions/ADR-0001-stack-choice.md";
/// The release-scope refusal of an established target whose policy is
/// missing (SPC-013 R-120).
const UNDECIDED: &str = "is a release branch cannot be decided";

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("CODEFLOW_HOOK_BINARY", &exe)
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
        // A caller's agent session must not judge the fixture's own commits.
        .env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
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
    text(out)
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

/// `git push` through the installed hooks; returns (success, output).
fn push(root: &Path) -> (bool, String) {
    let out = with_env(&mut Command::new("git"))
        .args(["push", "-u", "origin", BRANCH])
        .current_dir(root)
        .output()
        .expect("git runs");
    (out.status.success(), text(&out))
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
    std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
}

/// A bare `origin` whose `main` holds an "Initial commit" with a README and
/// then whatever `seed` commits in the seeding clone.
fn origin_with(dir: &Path, seed: impl FnOnce(&Path)) -> PathBuf {
    git(dir, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let seeding = dir.join("seed");
    std::fs::create_dir(&seeding).unwrap();
    git(&seeding, &["init", "-q", "-b", "main"]);
    std::fs::write(seeding.join("README.md"), "# project\n").unwrap();
    commit(&seeding, "Initial commit");
    seed(&seeding);
    git(&seeding, &["push", "-q", "../origin.git", "main"]);
    dir.join("origin.git")
}

/// Clone `origin` (with `clone_args`), adopt `CodeFlow` at the full tier on a
/// task branch with its adoption task planned and started, and commit it.
fn adopt(dir: &Path, origin: &Path, clone_args: &[&str]) -> PathBuf {
    let root = dir.join("proj");
    let url = format!("file://{}", origin.display());
    let mut args = vec!["clone", "-q"];
    args.extend(clone_args);
    args.extend([url.as_str(), "proj"]);
    git(dir, &args);
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    assert_eq!(
        git(&root, &["config", "core.hooksPath"]),
        ".codeflow/git-hooks"
    );
    git(&root, &["switch", "-q", "-c", BRANCH]);
    let args = [
        "task",
        "new",
        "--standalone-reason",
        "adopts CodeFlow",
        "--into",
        "main",
        "adopt codeflow",
    ];
    ok(&codeflow(&root, &args), "task new");
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When adopted, the project shall run the gates (journey).\n",
    );
    edit(
        &root,
        TASK,
        "## Outcome\n\n",
        "## Outcome\n\nAfter this merges, the team can run the CodeFlow gates.\n\n",
    );
    // The scaffold's first ADR is a new record without a uid; the registry
    // admits it as its own refusal says (`codeflow ids admit`).
    let refused = text(&codeflow(&root, &["ids", "admit", ADR]));
    let uid = refused
        .split("`uid: ")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .unwrap_or_else(|| panic!("no uid offered:\n{refused}"))
        .to_string();
    edit(
        &root,
        ADR,
        "id: ADR-0001\n",
        &format!("id: ADR-0001\nuid: {uid}\n"),
    );
    ok(&codeflow(&root, &["ids", "admit", ADR]), "ids admit");
    commit(&root, "chore: adopt codeflow");
    ok(
        &codeflow(&root, &["work", "start", "TSK-001"]),
        "work start",
    );
    root
}

fn acceptance(dir: &Path, reviewed: &str) -> String {
    let body = format!(
        "acceptance:\n  reviewed: {reviewed}\n  review: session:journey@sha256:00\n  criteria:\n    AC-1: verified | the adoption journey ran\n  journey: verified | crates/codeflow-cli/tests/first_adoption_journey.rs\n  not_verified: none\n  follow_ups: none: journey fixture\n  verdict: approved\n"
    );
    let path = dir.join("acceptance.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

/// AC-2 to AC-4: the adoption pushes, completes in its own branch and its
/// pull request into `main` is clean; `main` is never pushed to.
#[test]
fn a_first_adoption_pushes_and_completes_in_its_own_pull_request() {
    let dir = tempfile::tempdir().unwrap();
    let origin = origin_with(dir.path(), |_| {});
    let initial = git(&origin, &["rev-parse", "main"]);
    let root = adopt(dir.path(), &origin, &[]);

    // Ordinary pre-push: the task branch is judged and goes through.
    let (pushed, said) = push(&root);
    assert!(pushed, "the adoption push was refused:\n{said}");
    assert!(!said.contains(UNDECIDED), "{said}");

    // `task status complete` reads the same bootstrap scope: a refusal for
    // another reason names only that reason (issue 182's comment).
    let reviewed = git(&root, &["rev-parse", "HEAD"]);
    let block = acceptance(dir.path(), &reviewed);
    std::fs::write(root.join("notes.txt"), "draft\n").unwrap();
    let out = codeflow(
        &root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            &block,
        ],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("notes.txt"), "{}", text(&out));
    assert!(
        !text(&out).contains("cannot tell whether this branch is a release branch"),
        "{}",
        text(&out)
    );
    std::fs::remove_file(root.join("notes.txt")).unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "task",
                "status",
                "TSK-001",
                "complete",
                "--acceptance",
                &block,
            ],
        ),
        "task status complete",
    );
    commit(&root, "chore: complete the adoption task");
    let (pushed, said) = push(&root);
    assert!(pushed, "the completed adoption push was refused:\n{said}");

    // The pull request into `main`, as the hosted check judges it.
    let out = codeflow(
        &root,
        &[
            "ci",
            "--base",
            "origin/main",
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--into",
            "main",
            "--destination",
            origin.to_str().unwrap(),
        ],
    );
    let said = ok(&out, "the adoption pull request check");
    assert!(said.contains("acceptance"), "{said}");
    // The policy reached `origin` only on the task branch.
    assert_eq!(git(&origin, &["rev-parse", "main"]), initial);
}

/// AC-1 negative control: a default branch that once carried a policy and
/// dropped it is not an initial adoption.
#[test]
fn a_removed_default_branch_policy_still_refuses_the_adoption_push() {
    let dir = tempfile::tempdir().unwrap();
    let origin = origin_with(dir.path(), |seeding| {
        std::fs::create_dir_all(seeding.join(".codeflow")).unwrap();
        std::fs::write(
            seeding.join(".codeflow/policy.json"),
            "{\"schema_version\": 1}\n",
        )
        .unwrap();
        commit(seeding, "chore: an earlier adoption");
        std::fs::remove_file(seeding.join(".codeflow/policy.json")).unwrap();
        commit(seeding, "chore: drop the policy");
    });
    let root = adopt(dir.path(), &origin, &[]);
    let (pushed, said) = push(&root);
    assert!(
        !pushed,
        "a push over a removed policy went through:\n{said}"
    );
    assert!(
        said.contains(UNDECIDED) && said.contains("so the policy was removed"),
        "{said}"
    );
}

/// AC-1 negative control: a shallow clone cannot prove the default branch
/// never carried a policy, so the push is refused.
#[test]
fn an_unreadable_default_branch_history_still_refuses_the_adoption_push() {
    let dir = tempfile::tempdir().unwrap();
    let origin = origin_with(dir.path(), |seeding| {
        std::fs::write(seeding.join("NOTES.md"), "notes\n").unwrap();
        commit(seeding, "docs: notes");
    });
    // The adoption is planned in a full clone (the id registry needs its
    // history), then the clone is cut at the default branch's tip.
    let root = adopt(dir.path(), &origin, &[]);
    git(&root, &["fetch", "-q", "--depth", "1", "origin", "main"]);
    assert_eq!(
        git(&root, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    let (pushed, said) = push(&root);
    assert!(!pushed, "a push from a shallow clone went through:\n{said}");
    assert!(
        said.contains(UNDECIDED) && said.contains("cannot be proven"),
        "{said}"
    );
}
