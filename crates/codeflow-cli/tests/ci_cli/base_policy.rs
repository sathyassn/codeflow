//! The range is judged with the policy recorded at its base, as the hosted
//! job reads it from its base checkout (sathyassn/codeflow#22), and a base
//! git refuses to resolve is reported with git's own cause.
use super::{git, run_in};
use std::path::Path;
use std::process::Output;

const POLICY: &str = ".codeflow/policy.json";
const LOOSER: &str =
    r#"{"git": {"commit_body_max_bullets": 10, "commit_body_bullet_max_len": 100}}"#;
const FOUR_BULLETS: &str = "feat: add y\n\n- one\n- two\n- three\n- a fourth bullet\n";

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

/// `main` with `base_policy` (none when `None`) and a `feat/x` branch.
fn fixture(base_policy: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    if let Some(policy) = base_policy {
        write(root, POLICY, policy);
    }
    write(root, "base.txt", "base\n");
    commit(root, "chore: init");
    git(root, &["switch", "-q", "-c", "feat/x"]);
    dir
}

fn ci(root: &Path) -> Output {
    run_in(
        root,
        &[
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ],
    )
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_head_that_loosens_its_own_policy_gets_the_base_verdict() {
    let dir = fixture(Some("{}"));
    let root = dir.path();
    write(root, POLICY, LOOSER);
    commit(root, "chore: loosen the commit body rules");
    write(root, "y.txt", "y\n");
    commit(root, FOUR_BULLETS);
    let out = ci(root);
    let said = said(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("git.commit_body"), "{said}");
    assert!(
        said.contains("verifying against .codeflow/policy.json at base "),
        "{said}"
    );
    // A caller-chosen base is not presented as the hosted verdict.
    assert!(
        said.contains("only when that is the pull request's target tip"),
        "{said}"
    );
    assert!(
        said.contains("the working copy's .codeflow/policy.json differs from the one judging"),
        "{said}"
    );
}

/// sathyassn/codeflow#81: a change that lowers or removes a security level
/// is named for its reviewer; raising one or leaving it alone says nothing.
#[test]
fn a_head_that_lowers_a_security_level_is_named() {
    let dir = fixture(Some(
        r#"{"git": {"security_review": "block", "dep_audit": "warn"}}"#,
    ));
    let root = dir.path();
    write(root, POLICY, r#"{"git": {"security_review": "off"}}"#);
    commit(root, "chore: relax the security review");
    let text = said(&ci(root));
    assert!(
        text.contains("this change lowers git.security_review from block to off"),
        "{text}"
    );
    assert!(text.contains("this change removes git.dep_audit"), "{text}");

    let dir = fixture(Some(
        r#"{"git": {"security_review": "warn", "dep_audit": "warn"}}"#,
    ));
    let root = dir.path();
    write(
        root,
        POLICY,
        r#"{"git": {"security_review": "block", "dep_audit": "warn"}}"#,
    );
    commit(root, "chore: harden the security review");
    let text = said(&ci(root));
    assert!(!text.contains("this change lowers"), "{text}");
    assert!(!text.contains("this change removes"), "{text}");
}

#[test]
fn the_base_policy_also_lets_through_what_it_allows() {
    let dir = fixture(Some(LOOSER));
    let root = dir.path();
    write(root, POLICY, "{}");
    commit(root, "chore: tighten the commit body rules");
    write(root, "y.txt", "y\n");
    commit(root, FOUR_BULLETS);
    let out = ci(root);
    let said = said(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(!said.contains("git.commit_body"), "{said}");
}

#[test]
fn a_base_without_a_policy_keeps_the_working_copy() {
    // The adoption change itself: the target has no policy yet.
    let dir = fixture(None);
    let root = dir.path();
    write(root, POLICY, LOOSER);
    commit(root, "chore: adopt codeflow");
    write(root, "y.txt", "y\n");
    commit(root, FOUR_BULLETS);
    let out = ci(root);
    let said = said(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(!said.contains("git.commit_body"), "{said}");
    assert!(
        said.contains("codeflow ci: verifying against .codeflow/policy.json\n"),
        "{said}"
    );
}

#[test]
fn an_invalid_base_policy_verifies_nothing() {
    let dir = fixture(Some(r#"{"git": {"commit_body": "sometimes"}}"#));
    let root = dir.path();
    write(root, POLICY, "{}");
    commit(root, "chore: repair the policy");
    let out = ci(root);
    let said = said(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("judged with the policy at "), "{said}");
}

/// Point a replace ref for `main`'s commit at an object that does not
/// exist, so git stops on the base the way git 2.55 stops on a
/// `GIT_REPLACE_REF_BASE` without a trailing slash. Git refuses to write
/// such a ref, so the file is written directly.
fn break_the_base(root: &Path) {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(root)
        .output()
        .unwrap();
    let sha = String::from_utf8(out.stdout).unwrap();
    write(
        root,
        &format!(".git/refs/replace/{}", sha.trim()),
        "0123456789012345678901234567890123456789\n",
    );
}

#[test]
fn a_base_git_refuses_is_reported_with_git_s_cause() {
    let dir = fixture(Some("{}"));
    let root = dir.path();
    write(root, "y.txt", "y\n");
    commit(root, "feat: add y");
    break_the_base(root);
    let out = ci(root);
    let said = said(&out);
    assert_ne!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains(
            "could not resolve a base ref (tried: main): git refused it: fatal: replacement"
        ),
        "{said}"
    );
    assert!(!said.contains("fetch the base branch"), "{said}");
}

/// `--policy-from` names the target tip apart from the range's base, so a
/// range that starts at an older fork point is judged by the target's
/// current policy, as the pre-push hook passes it.
#[test]
fn policy_from_judges_with_the_target_tip_not_the_fork_point() {
    let dir = fixture(Some(LOOSER));
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    write(root, POLICY, "{}");
    commit(root, "chore: tighten the commit body rules");
    git(root, &["switch", "-q", "feat/x"]);
    write(root, "y.txt", "y\n");
    commit(root, FOUR_BULLETS);
    let fork = run_in(
        root,
        &[
            "ci", "--base", "main~1", "--head", "HEAD", "--branch", "feat/x",
        ],
    );
    assert_eq!(fork.status.code(), Some(0), "{}", said(&fork));
    let out = run_in(
        root,
        &[
            "ci",
            "--base",
            "main~1",
            "--head",
            "HEAD",
            "--branch",
            "feat/x",
            "--policy-from",
            "main",
        ],
    );
    let said = said(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("git.commit_body"), "{said}");
    assert!(said.contains("the commit --policy-from names"), "{said}");
}

/// `--commits-from` moves the commit checks off the base, so the banner
/// names both ranges and does not call the run the hosted verdict, which
/// checks commits from its base (review round five).
#[test]
fn commits_from_another_commit_is_never_called_the_hosted_verdict() {
    let dir = fixture(Some("{}"));
    let root = dir.path();
    write(root, "y.txt", "y\n");
    commit(root, FOUR_BULLETS);
    let out = run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "feat/x",
            "--commits-from",
            "HEAD",
        ],
    );
    let said = said(&out);
    assert!(said.contains("the commit checks run over"), "{said}");
    assert!(said.contains("the other checks over"), "{said}");
    assert!(said.contains("so this is not the hosted verdict"), "{said}");
    assert!(
        !said.contains("it is the hosted verdict only when"),
        "{said}"
    );
    // The same commit as the base keeps the one range and its wording.
    let same = run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "feat/x",
            "--commits-from",
            "main",
        ],
    );
    let same = self::said(&same);
    assert!(!same.contains("the commit checks run over"), "{same}");
    assert!(
        same.contains("it is the hosted verdict only when"),
        "{same}"
    );
}
