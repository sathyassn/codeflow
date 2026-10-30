//! TSK-397: the built exec guard applies the strict text rule by default;
//! `security.text_guard: relaxed` is honoured only from the protected
//! remote-tracking refs, and a retry flag keeps a relaxed call strict.
use super::{codeflow, git, init_repo, run_time_word_refusal, run_with_stdin, write_policy};
use std::path::Path;
use std::process::Output;

const RELAXED: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"},"security":{"text_guard":"relaxed"}}"#;
const DEFAULT: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;
/// An everyday line the strict rule refuses by its run-time word and the
/// line's rule allows.
const EVERYDAY: &str = r#"echo "$HOME""#;

fn exec_guard(root: &Path, command: &str, retry: bool) -> Output {
    let mut input = serde_json::json!({ "command": command });
    if retry {
        input["dangerouslyDisableSandbox"] = serde_json::json!(true);
    }
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": input,
        "cwd": root,
    });
    run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard", "--contract", "3"])
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

/// A checkout on `task/x` whose `origin/main` holds `published` and whose
/// own `HEAD` and working copy hold `local`.
fn published(root: &Path, published: &str) {
    init_repo(root, "main");
    write_policy(root, published);
    git(root, &["add", ".codeflow/policy.json"]);
    git(root, &["commit", "-m", "chore: policy"]);
    git(
        root,
        &["remote", "add", "origin", "https://example.invalid/r.git"],
    );
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(root, &["checkout", "-q", "-b", "task/x"]);
}

#[test]
fn the_strict_rule_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("keeps the default"),
        "{out:?}"
    );
}

#[test]
fn relaxed_on_the_published_default_branch_is_honoured() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), RELAXED);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    if cfg!(windows) {
        // Native Windows keeps the strict rule under relaxed (ADR-0088).
        assert!(run_time_word_refusal(&out), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("native Windows"),
            "{out:?}"
        );
    } else {
        assert!(out.status.success(), "{out:?}");
    }
}

#[test]
fn relaxed_in_the_worktree_or_on_a_branch_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    write_policy(dir.path(), RELAXED);
    assert!(run_time_word_refusal(&exec_guard(
        dir.path(),
        EVERYDAY,
        false
    )));
    git(dir.path(), &["add", ".codeflow/policy.json"]);
    git(dir.path(), &["commit", "-m", "chore: relax on a branch"]);
    assert!(run_time_word_refusal(&exec_guard(
        dir.path(),
        EVERYDAY,
        false
    )));
}

#[test]
fn relaxed_without_a_remote_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_policy(dir.path(), RELAXED);
    git(dir.path(), &["add", ".codeflow/policy.json"]);
    git(dir.path(), &["commit", "-m", "chore: relax at HEAD"]);
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not honoured"),
        "{out:?}"
    );
}

#[test]
fn relaxed_keeps_a_retry_strict() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), RELAXED);
    let out = exec_guard(dir.path(), EVERYDAY, true);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("dangerouslyDisableSandbox"),
        "{out:?}"
    );
}

/// `published` with `json` as the remote's policy, where `level` is
/// `git.hook_integrity`.
fn policy_at(level: &str, relaxed: bool) -> String {
    let security = if relaxed {
        r#","security":{"text_guard":"relaxed"}"#
    } else {
        ""
    };
    format!(
        r#"{{"git":{{"protected_branches":["main","master"],"root_checkout_commits":"off","hook_integrity":"{level}"}}{security}}}"#
    )
}

/// SEC01-1: the strict rule's own refusals block whatever
/// `git.hook_integrity` says, for the default rule and for a marked retry
/// under relaxed; relaxed without a retry follows the line's rule. Native
/// Windows keeps the strict rule in every case.
#[test]
fn strict_refusals_block_at_every_integrity_level() {
    for level in ["block", "warn", "off"] {
        for relaxed in [false, true] {
            for retry in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                published(dir.path(), &policy_at(level, relaxed));
                let out = exec_guard(dir.path(), EVERYDAY, retry);
                let strict = !relaxed || retry || cfg!(windows);
                let case = format!("{level} relaxed={relaxed} retry={retry}: {out:?}");
                if strict {
                    assert!(run_time_word_refusal(&out), "{case}");
                } else {
                    assert!(out.status.success(), "{case}");
                }
            }
        }
    }
}

/// SEC01-1: doctor says the strict rule's refusals always block, and names
/// the level the line's own findings take.
#[test]
fn doctor_reports_how_the_text_rule_enforces() {
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), &policy_at("warn", false));
    let out = codeflow()
        .args(["doctor", "--check", "sandbox"])
        .current_dir(dir.path())
        .env_remove("CLAUDE_PROJECT_DIR")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("text rule: strict"), "{text}");
    assert!(
        text.contains("the strict rule's own refusals always block")
            && text.contains("`git.hook_integrity: warn`"),
        "{text}"
    );
}

/// `published` with `RELAXED` committed on a local branch `relaxed` and as
/// the tag `relaxed-tag`; `origin/main` stays strict.
fn local_relaxed(root: &Path) -> String {
    published(root, DEFAULT);
    git(root, &["checkout", "-q", "-b", "relaxed"]);
    write_policy(root, RELAXED);
    git(root, &["add", ".codeflow/policy.json"]);
    git(root, &["commit", "-m", "chore: relax locally"]);
    git(root, &["tag", "relaxed-tag"]);
    git(root, &["checkout", "-q", "task/x"]);
    String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "relaxed"])
            .current_dir(root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

/// SEC01-2: a remote `HEAD` that points outside the remote's namespace, at
/// a local branch or a tag, does not select relaxed.
#[test]
fn a_remote_head_outside_the_remote_keeps_strict() {
    for target in ["refs/heads/relaxed", "refs/tags/relaxed-tag"] {
        let dir = tempfile::tempdir().unwrap();
        local_relaxed(dir.path());
        std::fs::write(
            dir.path().join(".git/refs/remotes/origin/HEAD"),
            format!("ref: {target}\n"),
        )
        .unwrap();
        let out = exec_guard(dir.path(), EVERYDAY, false);
        assert!(run_time_word_refusal(&out), "{target}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("outside refs/remotes/origin/"),
            "{target}: {out:?}"
        );
    }
}

/// SEC01-2: a loose remote-tracking ref file that is a symbolic link does
/// not select relaxed.
#[cfg(unix)]
#[test]
fn a_linked_ref_file_keeps_strict() {
    let dir = tempfile::tempdir().unwrap();
    let relaxed = local_relaxed(dir.path());
    let elsewhere = dir.path().join("forged-ref");
    std::fs::write(&elsewhere, format!("{relaxed}\n")).unwrap();
    let main = dir.path().join(".git/refs/remotes/origin/main");
    std::fs::remove_file(&main).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &main).unwrap();
    let out = exec_guard(dir.path(), EVERYDAY, false);
    assert!(run_time_word_refusal(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("symbolic link"),
        "{out:?}"
    );
}

/// SEC01-2 control: a genuine relaxed policy in a packed remote-tracking
/// ref, and read from a linked worktree, is honoured.
#[test]
fn relaxed_in_a_packed_ref_and_a_linked_worktree_is_honoured() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    published(&root, RELAXED);
    git(&root, &["pack-refs", "--all"]);
    assert!(!root.join(".git/refs/remotes/origin/main").exists());
    let linked = dir.path().join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/y",
            &linked.to_string_lossy(),
        ],
    );
    for at in [&root, &linked] {
        let out = exec_guard(at, EVERYDAY, false);
        if cfg!(windows) {
            assert!(run_time_word_refusal(&out), "{out:?}");
        } else {
            assert!(out.status.success(), "{}: {out:?}", at.display());
        }
    }
}

/// SEC01-4: a policy entry whose tree mode is a symbolic link is not read
/// as policy, even when its blob holds a relaxed document.
#[test]
fn a_link_mode_policy_entry_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    published(root, DEFAULT);
    git(root, &["checkout", "-q", "main"]);
    let blob = {
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .args(["hash-object", "-w", "--stdin"])
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(RELAXED.as_bytes())
            .unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    git(
        root,
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{blob},.codeflow/policy.json"),
        ],
    );
    git(root, &["commit", "-q", "-m", "chore: link-mode policy"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(root, &["checkout", "-q", "-f", "task/x"]);
    let out = exec_guard(root, EVERYDAY, false);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is not a regular file"),
        "{out:?}"
    );
}

#[cfg(unix)]
#[allow(dead_code)]
#[path = "../support/drain_pipe.rs"]
mod drain_pipe;
#[cfg(unix)]
use drain_pipe::drain_pipe;

/// [`exec_guard`], failing the test if the hook has not answered within a
/// deadline instead of waiting on it.
#[cfg(unix)]
fn exec_guard_within(root: &Path, command: &str) -> Output {
    use std::io::Write as _;
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": root,
    });
    let mut child = codeflow()
        .args(["hook", "exec-guard", "--contract", "3"])
        .current_dir(root)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let stdout = drain_pipe(child.stdout.take());
    let stderr = drain_pipe(child.stderr.take());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Output {
                status,
                stdout: stdout.join().unwrap(),
                stderr: stderr.join().unwrap(),
            };
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the hook did not answer within 20 seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// SEC01-3: a fifo as the working policy or as the repository's
/// configuration, and an oversized working policy, refuse promptly.
#[cfg(unix)]
#[test]
fn unreadable_settings_shapes_refuse_promptly() {
    let fifo = |path: &Path| {
        let _ = std::fs::remove_file(path);
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    };
    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    fifo(&dir.path().join(".codeflow/policy.json"));
    let out = exec_guard_within(dir.path(), "true");
    assert_eq!(out.status.code(), Some(2), "policy fifo: {out:?}");

    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    fifo(&dir.path().join(".git/config"));
    let out = exec_guard_within(dir.path(), "true");
    assert_eq!(out.status.code(), Some(2), "config fifo: {out:?}");

    let dir = tempfile::tempdir().unwrap();
    published(dir.path(), DEFAULT);
    let mut big = String::from(r#"{"git":{"protected_branches":["main"]},"pad":""#);
    big.push_str(&"x".repeat(5 * 1024 * 1024));
    big.push_str("\"}");
    write_policy(dir.path(), &big);
    let out = exec_guard_within(dir.path(), "true");
    assert_eq!(out.status.code(), Some(2), "oversized policy: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("larger than"),
        "{out:?}"
    );
}
