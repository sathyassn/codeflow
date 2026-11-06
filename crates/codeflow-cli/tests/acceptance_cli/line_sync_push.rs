//! A task branch can push its own hand-resolved sync of its target line
//! (TSK-1149). Pushing keeps the work durable; the whole-unit review of the
//! head, hand-resolved hunks included, is judged by `codeflow ci` at ready
//! and at landing. The push goes through the installed pre-push shim; every
//! push the relaxation does not cover keeps today's refusal.
use super::*;

const LINE: &str = "integration/EPC-001-line-work";
const RECEIVER: &str = "task/TSK-002-extend-the-line";
const TOOL: &str = "src/tool.txt";
const HAND_RESOLVED: &str = "task edit\nline edit\n";

/// `git` with the shipped pre-push shim installed and the built binary as
/// its hook binary.
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

/// A real `git push origin <refspec>` through the installed hook.
fn push(root: &Path, refspec: &str) -> (i32, String) {
    let out = hooked_git(root)
        .args(["push", "origin", refspec])
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

/// Push without hooks, to set up the destination.
fn seed_push(root: &Path, refs: &[&str]) {
    let mut args = vec!["-c", "core.hooksPath=/dev/null", "push", "-q", "origin"];
    args.extend_from_slice(refs);
    git(root, &args);
}

struct Fixture {
    dir: tempfile::TempDir,
    /// The line's tip that the task branch merged.
    line_tip: String,
}

/// TSK-002 targets the line and edits the tool; the line then edits the
/// same line of the tool, and the task branch merges the line, resolving
/// the conflict by hand. `extra` runs on the task branch before the push
/// hook is installed.
fn fixture() -> Fixture {
    let dir = repo(
        &[("TSK-001", OWN_JOURNEY), ("TSK-002", OWN_JOURNEY)],
        ", \"test_gate_on_push\": \"block\"",
    );
    let root = dir.path();
    write(
        root,
        &record_path("TSK-002"),
        &task("TSK-002", "todo", OWN_JOURNEY, "Pending.\n").replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        ),
    );
    write(root, TOOL, "tool line\n");
    commit(root, "docs: target the line");
    git(root, &["branch", LINE]);
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
    seed_push(root, &["main", LINE]);

    git(root, &["switch", "-q", "-c", RECEIVER, LINE]);
    write(root, TOOL, "task edit\n");
    commit(root, "feat: edit the tool");
    seed_push(root, &[RECEIVER]);

    git(root, &["switch", "-q", LINE]);
    write(root, TOOL, "line edit\n");
    let line_tip = commit(root, "feat: the line edits the tool");
    seed_push(root, &[LINE]);

    git(root, &["switch", "-q", RECEIVER]);
    let conflicted = codeflow_git(root, &["merge", "--no-ff", "--no-commit", LINE]);
    assert!(!conflicted, "the sync must conflict");
    write(root, TOOL, HAND_RESOLVED);
    commit(root, "chore(sync): merge the line");
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
    Fixture { dir, line_tip }
}

/// Run git and report whether it succeeded, for a merge that may stop.
fn codeflow_git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
        .status
        .success()
}

/// `codeflow ci` for the pull request from the task branch into the line,
/// as the ready check and the hosted job run it.
fn ready(fixture: &Fixture) -> (i32, String) {
    let root = fixture.dir.path();
    let body = with_reviews(
        root,
        "HEAD",
        "## Summary\nThe tool edit, synced with the line.\n\n- one change\n\nTask: TSK-002\n\n## Changes\n- Edit the tool.\n\n## Testing\n- test\n",
    );
    let out = codeflow()
        .args([
            "ci",
            "--base",
            &fixture.line_tip,
            "--head",
            "HEAD",
            "--branch",
            RECEIVER,
            "--into",
            LINE,
            "--pr-body",
            &body,
        ])
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

/// `codeflow ci` with the pre-push hook's hand-off arguments for `branch`.
fn push_hand_off(fixture: &Fixture, branch: &str) -> (i32, String) {
    let root = fixture.dir.path();
    let remote = root.join(".git/destination.git");
    let url = remote.to_str().unwrap();
    let listed = git_out(
        root,
        &[
            "ls-remote",
            "--symref",
            url,
            "HEAD",
            "refs/heads/*",
            "refs/tags/*",
        ],
    );
    let mut child = codeflow()
        .args([
            "ci",
            "--base",
            &fixture.line_tip,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--run-level",
            "block",
            "--destination",
            url,
            "--advertisement-stdin",
            &format!("--into={LINE}"),
        ])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(
        &mut child.stdin.take().unwrap(),
        format!("{listed}\n").as_bytes(),
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn receiving(merge: &str) -> String {
    format!("receiving line `{LINE}` introduces {TOOL} through merge {merge}")
}

/// AC-1, AC-2: the task branch's push of its hand-resolved line sync goes
/// through the real hook, and the same head is still refused at ready
/// without a review of it.
#[test]
fn a_task_branch_pushes_its_hand_resolved_line_sync_and_ready_still_refuses() {
    let fixture = fixture();
    let root = fixture.dir.path();
    let merge = head(root);
    let pushed = push(root, RECEIVER);
    assert_eq!(pushed.0, 0, "the push must go through: {}", pushed.1);
    assert!(!pushed.1.contains(&receiving(&merge)), "{}", pushed.1);
    assert_eq!(
        git_out(root, &["rev-parse", &format!("origin/{RECEIVER}")]),
        merge,
        "the destination holds the synced head"
    );
    assert_blocks(
        &ready(&fixture),
        "ready without a review",
        &["work.acceptance_binding", &receiving(&merge)],
    );
}

/// AC-3: a merge of anything the line does not hold is not the branch's
/// own line sync, so the push keeps today's refusal.
#[test]
fn a_hand_resolved_merge_of_main_still_refuses_the_push() {
    let fixture = fixture();
    let root = fixture.dir.path();
    git(root, &["switch", "-q", "main"]);
    write(root, TOOL, "main edit\n");
    commit(root, "feat: main edits the tool");
    seed_push(root, &["main"]);
    git(root, &["switch", "-q", RECEIVER]);
    assert!(!codeflow_git(
        root,
        &["merge", "--no-ff", "--no-commit", "main"]
    ));
    write(root, TOOL, "task edit\nline edit\nmain edit\n");
    let merge = commit(root, "chore(sync): merge main");
    let pushed = push(root, RECEIVER);
    assert_blocks(
        &pushed,
        "a merge of main",
        &["work.acceptance_binding", &receiving(&merge)],
    );
}

/// AC-3: a branch that carries no task gets no relaxation in the hook's
/// hand-off, and neither does a protected name.
#[test]
fn a_non_task_or_protected_branch_keeps_the_refusal() {
    let fixture = fixture();
    let root = fixture.dir.path();
    let merge = head(root);
    assert_passes(&push_hand_off(&fixture, RECEIVER), "the task branch");
    for branch in ["feat/sync-the-line", "main"] {
        assert_blocks(
            &push_hand_off(&fixture, branch),
            branch,
            &["work.acceptance_binding", &receiving(&merge)],
        );
    }
}

/// AC-3: the line's own push of the same hand-resolved history keeps
/// judging every merge.
#[test]
fn a_push_of_the_line_itself_keeps_the_refusal() {
    let fixture = fixture();
    let root = fixture.dir.path();
    let merge = head(root);
    let pushed = push(root, &format!("{RECEIVER}:refs/heads/{LINE}"));
    assert_ne!(pushed.0, 0, "the line push must refuse: {}", pushed.1);
    assert_eq!(
        git_out(root, &["rev-parse", &format!("origin/{LINE}")]),
        fixture.line_tip,
        "the line stays where it was; refused merge {merge}"
    );
}
