//! TSK-1285: a binary older than the repository says it is too old.
//!
//! The fixture is a scratch adopter whose pin (`scaffold_version` in
//! `.codeflow/project.toml`) is raised past the binary's own, with a policy
//! key, record or test-config field this binary cannot read. Each reader
//! gives one message naming both versions and the upgrade, and still refuses:
//! a non-zero exit, an unmoved `HEAD` or an unmoved destination. The binary
//! under test is the one Cargo built, first on `PATH`; `TSK1285_TEST_BINARY`
//! runs these with another binary, such as an installed release, for the
//! fail-before record.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

const BINARY: &str = env!("CARGO_PKG_VERSION");
const NEWER: &str = "99.0.0";
const BRANCH: &str = "fix/too-old";
const PROJECT: &str = ".codeflow/project.toml";
const POLICY: &str = ".codeflow/policy.json";
const TEST_CONFIG: &str = ".codeflow/test-config.json";
/// A pin that is present and cannot be compared. `codeflow ci` refuses a
/// state with no pin line at all, as its head configuration.
const NOT_A_VERSION: &str = "newest";
const TASK: &str = "project-management/tasks/TSK-001.md";
const MAY_BE_OLDER: &str = "may be older than this repository";
const INSTALLER: &str = "codeflow-cli-installer.sh";

fn binary() -> PathBuf {
    std::env::var_os("TSK1285_TEST_BINARY").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_codeflow")),
        PathBuf::from,
    )
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = binary();
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
        .env_remove("CODEFLOW_HOOK_BINARY")
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
    with_env(&mut Command::new(binary()))
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

fn git_out(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = git_out(root, args);
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}:\n{text}");
    std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
}

/// Set the pin, or remove the state's line for it when `pin` is `None`.
fn set_pin(root: &Path, pin: Option<&str>) {
    let path = root.join(PROJECT);
    let state = std::fs::read_to_string(&path).unwrap();
    let lines = state.lines().filter_map(|line| {
        if !line.starts_with("scaffold_version") {
            return Some(line.to_string());
        }
        pin.map(|pin| format!("scaffold_version = \"{pin}\""))
    });
    std::fs::write(&path, lines.collect::<Vec<_>>().join("\n") + "\n").unwrap();
}

/// A policy key no release of this binary knows.
fn write_future_policy(root: &Path) {
    std::fs::write(
        root.join(POLICY),
        r#"{"schema_version":1,"git":{"a_rule_from_a_later_release":"block"}}"#,
    )
    .unwrap();
}

/// A task record this binary reads as wrong, as the 2026-11-03 binary read
/// a criterion outcome added after it.
fn write_future_record(root: &Path) {
    edit(root, TASK, "status: todo", "status: a_later_status");
}

/// A fixture branch whose commit carries the record, and the base it left.
fn branch_with_future_record(root: &Path) -> String {
    let base = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "-c", BRANCH]);
    write_future_record(root);
    land(root, "fix: change the record");
    base
}

/// A project at the standard tier holding one task record, with `change`
/// committed on the default branch.
fn adopter(change: impl FnOnce(&Path)) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = codeflow(&root, &["init", "--yes", "--standard"]);
    assert!(init.status.success(), "init: {}", text(&init));
    let default = git(&root, &["branch", "--show-current"]);
    let task = codeflow(
        &root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "fixture",
            "--into",
            &default,
            "fixture task",
        ],
    );
    assert!(task.status.success(), "task new: {}", text(&task));
    change(&root);
    land(&root, "chore: set up the fixture");
    (dir, root)
}

/// Commit the tree on the checked-out branch, as a fixture sets a state
/// that already landed. The hooks are what the tests exercise afterwards.
fn land(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
}

/// A fixture branch with one more commit, and the base it left.
fn branch_with_change(root: &Path) -> String {
    let base = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "-c", BRANCH]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    land(root, "fix: change the notes");
    base
}

fn ci(root: &Path, base: &str) -> Output {
    codeflow(
        root,
        &["ci", "--base", base, "--head", "HEAD", "--branch", BRANCH],
    )
}

/// The one too-old message: both versions and the upgrade path, once.
#[track_caller]
fn assert_too_old(said: &str) {
    assert_eq!(said.matches("this binary is too old").count(), 1, "{said}");
    assert!(said.contains(&format!("it is CodeFlow {BINARY}")), "{said}");
    assert!(said.contains(&format!("needs {NEWER} or newer")), "{said}");
    assert!(said.contains(INSTALLER), "{said}");
}

// ---- Task and epic records ---------------------------------------------

/// AC-2 and AC-4: the record findings give way to the one message.
#[test]
fn ci_reports_records_it_cannot_read_as_a_too_old_binary() {
    let (_dir, root) = adopter(|root| set_pin(root, Some(NEWER)));
    let base = branch_with_future_record(&root);
    let out = ci(&root, &base);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_too_old(&said);
    assert!(said.contains("git.binary_too_old"), "{said}");
    assert!(!said.contains("lifecycle invariant work."), "{said}");
}

/// The pin is read at the authority: a range that raises its own pin keeps
/// every finding it earned.
#[test]
fn a_pin_raised_in_the_range_hides_no_record_finding() {
    let (_dir, root) = adopter(|_| {});
    let base = branch_with_future_record(&root);
    set_pin(&root, Some(NEWER));
    land(&root, "fix: raise the pin");
    let out = ci(&root, &base);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("lifecycle invariant work."), "{said}");
    assert!(!said.contains("too old"), "{said}");
}

/// AC-3: no marker at the authority keeps the findings and adds one line.
#[test]
fn ci_with_no_pin_keeps_the_record_findings_and_adds_one_line() {
    let (_dir, root) = adopter(|root| set_pin(root, Some(NOT_A_VERSION)));
    let base = branch_with_future_record(&root);
    let out = ci(&root, &base);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("lifecycle invariant work."), "{said}");
    assert_eq!(said.matches(MAY_BE_OLDER).count(), 1, "{said}");
    assert!(said.contains("codeflow --version"), "{said}");
}

/// A pin at the binary's own version changes nothing.
#[test]
fn ci_at_the_binarys_own_version_reports_the_records_as_today() {
    let (_dir, root) = adopter(|_| {});
    let base = branch_with_future_record(&root);
    let out = ci(&root, &base);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("lifecycle invariant work."), "{said}");
    assert!(!said.contains("too old"), "{said}");
    assert!(!said.contains(MAY_BE_OLDER), "{said}");
}

#[test]
fn validate_docs_reports_records_it_cannot_read_as_a_too_old_binary() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_record(root);
    });
    let out = codeflow(&root, &["validate", "--docs"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("validate --docs: error"), "{said}");
    assert!(!said.contains("workgraph integrity"), "{said}");
}

#[test]
fn validate_with_no_pin_keeps_the_record_errors_and_adds_one_line() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, None);
        write_future_record(root);
    });
    let out = codeflow(&root, &["validate", "--docs"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("validate --docs: error"), "{said}");
    assert_eq!(said.matches(MAY_BE_OLDER).count(), 1, "{said}");
}

// ---- Policy --------------------------------------------------------------

#[test]
fn ci_reports_a_policy_it_cannot_read_as_a_too_old_binary() {
    let (dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_policy(root);
    });
    let base = git(&root, &["rev-parse", "HEAD"]);
    let rules = dir.path().join("rules.txt");
    std::fs::write(&rules, "").unwrap();
    let out = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &base,
            "--head",
            "HEAD",
            "--branch",
            BRANCH,
            "--blocking-rules-out",
            rules.to_str().unwrap(),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("policy error"), "{said}");
    assert_eq!(
        std::fs::read_to_string(&rules).unwrap(),
        "git.binary_too_old\n"
    );
}

/// The policy that judges is the authority's, whatever the working copy says.
#[test]
fn ci_reports_an_authority_policy_it_cannot_read_as_a_too_old_binary() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_policy(root);
    });
    let base = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", BRANCH]);
    std::fs::write(root.join(POLICY), r#"{"schema_version":1}"#).unwrap();
    land(&root, "fix: drop the key");
    let out = ci(&root, &base);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("policy error"), "{said}");
    assert!(!said.contains("which is invalid"), "{said}");
}

#[test]
fn validate_reports_a_policy_it_cannot_read_as_a_too_old_binary() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_policy(root);
        write_future_record(root);
    });
    let out = codeflow(&root, &["validate", "--docs"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    // One message, though the policy and the records both failed.
    assert_too_old(&said);
    assert!(!said.contains("error: policy"), "{said}");
}

/// AC-2 and AC-4 on the commit path: the real hook refuses and `HEAD` stays.
#[test]
fn a_commit_under_a_policy_it_cannot_read_says_the_binary_is_too_old() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_policy(root);
    });
    let head = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", BRANCH]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    git(&root, &["add", "-A"]);
    let out = git_out(&root, &["commit", "-q", "-m", "fix: change the notes"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("a_rule_from_a_later_release"), "{said}");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}

/// The commit-msg stage reads the working copy's policy, which the landed
/// policy the pre-commit stage reads does not yet hold.
#[test]
fn a_commit_message_under_a_working_policy_it_cannot_read_says_too_old() {
    let (_dir, root) = adopter(|_| {});
    let head = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", BRANCH]);
    set_pin(&root, Some(NEWER));
    write_future_policy(&root);
    git(&root, &["add", "-A"]);
    let out = git_out(&root, &["commit", "-q", "-m", "fix: raise the pin"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("policy error"), "{said}");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}

/// AC-3 and AC-4 on the commit path.
#[test]
fn a_commit_with_no_pin_keeps_the_policy_errors_and_adds_one_line() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, None);
        write_future_policy(root);
    });
    let head = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "-c", BRANCH]);
    std::fs::write(root.join("notes.txt"), "a change\n").unwrap();
    git(&root, &["add", "-A"]);
    let out = git_out(&root, &["commit", "-q", "-m", "fix: change the notes"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("a_rule_from_a_later_release"), "{said}");
    assert_eq!(said.matches(MAY_BE_OLDER).count(), 1, "{said}");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}

// ---- Test config ---------------------------------------------------------

fn write_future_test_config(root: &Path) {
    std::fs::write(
        root.join(TEST_CONFIG),
        r#"{"schema_version":"1.0","targets":[{"name":"unit","enabled":true,"runner":"custom","a_field_from_a_later_release":true,"modes":{"quick":{"command":"true"}}}]}"#,
    )
    .unwrap();
}

#[test]
fn a_test_config_field_it_does_not_know_says_the_binary_is_too_old() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_test_config(root);
    });
    let out = codeflow(&root, &["test", "--mode", "quick"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("a_field_from_a_later_release"), "{said}");
}

#[test]
fn a_test_config_schema_it_does_not_know_says_the_binary_is_too_old() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        std::fs::write(
            root.join(TEST_CONFIG),
            r#"{"schema_version":"99.0","targets":[]}"#,
        )
        .unwrap();
    });
    let out = codeflow(&root, &["test", "--mode", "quick"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_too_old(&said);
}

#[test]
fn a_test_config_with_no_pin_keeps_its_error_and_adds_one_line() {
    let (_dir, root) = adopter(|root| {
        set_pin(root, None);
        write_future_test_config(root);
    });
    let out = codeflow(&root, &["test", "--mode", "quick"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("a_field_from_a_later_release"), "{said}");
    assert_eq!(said.matches(MAY_BE_OLDER).count(), 1, "{said}");
}

#[test]
fn a_test_config_error_at_the_binarys_own_version_stays_as_it_was() {
    let (_dir, root) = adopter(write_future_test_config);
    let out = codeflow(&root, &["test", "--mode", "quick"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("a_field_from_a_later_release"), "{said}");
    assert!(!said.contains("too old"), "{said}");
    assert!(!said.contains(MAY_BE_OLDER), "{said}");
}

// ---- The push, through the real pre-push hook ----------------------------

/// `root` with a bare destination holding the default branch as it stands.
fn bare_destination(dir: &Path, root: &Path) -> PathBuf {
    let origin = dir.join("origin.git");
    codeflow_fixture::clone(dir, root.to_str().unwrap(), "origin.git")
        .bare()
        .run();
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(root, &["fetch", "-q", "origin"]);
    // Git before 2.48 records no remote HEAD on fetch (ADR-0091).
    git(root, &["remote", "set-head", "origin", "-a"]);
    origin
}

/// [`bare_destination`] and a branch to push. The branch's commit is one a
/// binary that could read the policy made, so the push is what is judged.
fn destination(dir: &Path, root: &Path) -> PathBuf {
    let origin = bare_destination(dir, root);
    branch_with_change(root);
    origin
}

fn push(root: &Path) -> Output {
    git_out(root, &["push", "-u", "origin", BRANCH])
}

fn pushed(origin: &Path) -> bool {
    git_out(
        origin,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{BRANCH}"),
        ],
    )
    .status
    .success()
}

/// AC-5: the marker past the binary's own, with a key from that release,
/// gives the too-old message and the upgrade and moves nothing.
#[test]
fn a_push_to_a_newer_repository_refuses_with_the_too_old_message() {
    let (dir, root) = adopter(|root| {
        set_pin(root, Some(NEWER));
        write_future_policy(root);
    });
    let origin = destination(dir.path(), &root);
    let out = push(&root);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert_too_old(&said);
    assert!(!said.contains("fix the findings above"), "{said}");
    assert!(!said.contains("a_rule_from_a_later_release"), "{said}");
    assert!(!pushed(&origin), "{said}");
}

/// AC-5 as the 2026-11-03 push met it: the destination's policy is readable
/// and `codeflow ci` is what fails, on a record from the later release.
#[test]
fn a_push_of_records_it_cannot_read_refuses_with_the_too_old_message() {
    let (dir, root) = adopter(|root| set_pin(root, Some(NEWER)));
    let origin = bare_destination(dir.path(), &root);
    branch_with_future_record(&root);
    let out = push(&root);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert_too_old(&said);
    assert!(said.contains("install the newer release build"), "{said}");
    assert!(!said.contains("fix the findings above"), "{said}");
    assert!(!said.contains("lifecycle invariant work."), "{said}");
    assert!(!pushed(&origin), "{said}");
}

/// AC-5: the same push with the marker at the binary's own version passes.
#[test]
fn the_same_push_at_the_binarys_own_version_passes() {
    let (dir, root) = adopter(|_| {});
    let origin = destination(dir.path(), &root);
    let out = push(&root);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains("too old"), "{said}");
    assert!(pushed(&origin), "{said}");
}

/// A newer pin alone refuses nothing: the first pull request of the two-step
/// upgrade raises only the pin, and an older binary judges it.
#[test]
fn a_newer_pin_with_everything_readable_still_pushes() {
    let (dir, root) = adopter(|root| set_pin(root, Some(NEWER)));
    let origin = destination(dir.path(), &root);
    let out = push(&root);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(pushed(&origin), "{said}");
}

/// AC-3 and AC-4 on the push path.
#[test]
fn a_push_with_no_pin_keeps_the_refusal_and_adds_one_line() {
    let (dir, root) = adopter(|root| {
        set_pin(root, None);
        write_future_policy(root);
    });
    let origin = destination(dir.path(), &root);
    let out = push(&root);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("is not valid for this codeflow"), "{said}");
    assert!(said.contains(MAY_BE_OLDER), "{said}");
    assert!(!pushed(&origin), "{said}");
}
