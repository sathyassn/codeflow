//! Session guards and git hooks: each guard finding, and the printed
//! step taken through the same guard.
//!
//! A guard judges a command the agent proposes. Its step is either a change
//! to what the guard reads (the policy, a git alias), after which the same
//! command passes, or a different command, which the guard passes and the
//! fixture then runs to show it does what the refused one was for.

use std::io::Write as _;
use std::process::Stdio;

use super::*;

/// `codeflow hook <which>` on a Claude Code `PreToolUse` Bash payload for
/// `shell`, run from `root`.
fn guard(root: &Path, which: &str, shell: &str) -> String {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": shell},
        "cwd": root,
    })
    .to_string();
    hook_with_stdin(root, &["hook", which], payload.as_bytes())
}

/// Run `codeflow <args>` in `root` with `input` on stdin.
fn hook_with_stdin(root: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = command(exe().to_str().unwrap(), root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    text(&child.wait_with_output().unwrap())
}

/// The guard passes `shell` without a finding.
fn assert_passes(root: &Path, which: &str, shell: &str) {
    let out = guard(root, which, shell);
    assert!(
        !out.contains("BLOCKED") && !out.contains("warning") && !out.contains("note:"),
        "the guard still has a finding for `{shell}`:\n{out}"
    );
}

/// The block a guard prints for `bad`, checked against the row.
fn refused(root: &Path, which: &str, bad: &str, finding: &str, row: &str) -> String {
    let out = guard(root, which, bad);
    let printed = block(&out, finding).to_string();
    assert_prints_row(&printed, row);
    printed
}

fn head(root: &Path, rev: &str) -> String {
    String::from_utf8(run("git", root, &["rev-parse", rev]).stdout)
        .unwrap()
        .trim()
        .to_string()
}

/// A plain repository on `feat/x` with one commit of work, and a bare
/// destination `origin` holding `main` and `feat/x`.
fn with_origin() -> tempfile::TempDir {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    // Scratch clones and state live beside the work, outside its status.
    write(root, ".git/info/exclude", ".dest.git/\n.other/\n");
    let dest = root.join(".dest.git");
    git(root, &["init", "-q", "--bare", dest.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", dest.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main", "feat/x"]);
    git(root, &["fetch", "-q", "origin"]);
    dir
}

#[test]
fn clears_protected_branch() {
    let dir = with_origin();
    let root = dir.path();
    let printed = refused(
        root,
        "git-guard",
        "git push origin feat/x:main",
        "git.push_to_protected",
        "PROTECTED_BRANCH",
    );
    let step = printed_command(&printed, "PROTECTED_BRANCH", None);
    let landing = step
        .replace("<branch>", "feat/x")
        .replace("<target>", "main");
    assert_passes(root, "git-guard", &landing);
    run_printed(root, &landing, &[], &[]);
    assert_eq!(head(root, "main"), head(root, "feat/x"), "the work landed");
}

#[test]
fn clears_protected_delete() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    prove(
        "PROTECTED_DELETE",
        "git.delete_protected",
        || guard(root, "git-guard", "git branch -D main"),
        |printed| {
            assert!(printed.contains("git.protected_branches"), "{printed}");
            write(
                root,
                ".codeflow/policy.json",
                r#"{"git": {"protected_branches": ["master"]}}"#,
            );
        },
    );
    git(root, &["branch", "-D", "main"]);
}

#[test]
fn clears_protected_rewrite() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    commit(root, "mistake.txt", "feat: add a mistake");
    let mistake = head(root, "HEAD");
    let printed = refused(
        root,
        "git-guard",
        "git reset --hard HEAD~1",
        "git.hard_reset_protected",
        "PROTECTED_REWRITE",
    );
    let step = printed_command(&printed, "PROTECTED_REWRITE", None);
    // The revert goes on its own branch, which lands like any other work.
    let undo = format!("git switch -c fix/undo && {step} --no-edit HEAD");
    assert_passes(root, "git-guard", &undo);
    git(root, &["switch", "-q", "-c", "fix/undo"]);
    run_printed(root, &step, &[], &["--no-edit", "HEAD"]);
    assert_eq!(head(root, "main"), mistake, "main keeps its history");
    assert!(!root.join("mistake.txt").exists(), "the revert undid it");
}

#[test]
fn clears_remote_tracking_ref() {
    let dir = with_origin();
    let root = dir.path();
    let printed = refused(
        root,
        "git-guard",
        "git update-ref refs/remotes/origin/main HEAD",
        "git.local_ref_protection",
        "REMOTE_TRACKING_REF",
    );
    let step = printed_command(&printed, "REMOTE_TRACKING_REF", None);
    let fetch = format!("{step} origin");
    assert_passes(root, "git-guard", &fetch);
    // The destination moves on; the fetch, not a hand-set ref, follows it.
    let dest = root.join(".dest.git");
    git(root, &["push", "-q", "origin", "feat/x:main"]);
    run_printed(root, &step, &[], &["-q", "origin"]);
    assert_eq!(
        head(root, "refs/remotes/origin/main"),
        String::from_utf8(run("git", &dest, &["rev-parse", "main"]).stdout)
            .unwrap()
            .trim()
    );
}

#[test]
fn clears_force_push() {
    let dir = with_origin();
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        r#"{"git": {"force_push_unprotected": "block"}}"#,
    );
    git(
        root,
        &["commit", "-q", "-am", "chore: restrict force pushes"],
    );
    git(root, &["push", "-q", "origin", "feat/x"]);
    // Someone else's commit reached the destination's feat/x.
    let other = root.join(".other");
    git(
        root,
        &[
            "clone",
            "-q",
            "-b",
            "feat/x",
            root.join(".dest.git").to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    commit(&other, "o.txt", "feat: add o");
    git(&other, &["push", "-q", "origin", "feat/x"]);
    commit(root, "y.txt", "feat: add y");
    let printed = refused(
        root,
        "git-guard",
        "git push --force origin feat/x",
        "git.force_push_unprotected",
        "FORCE_PUSH",
    );
    let step = printed_command(&printed, "FORCE_PUSH", None);
    let pull = format!("{step} origin feat/x");
    assert_passes(root, "git-guard", &pull);
    assert_passes(root, "git-guard", "git push origin feat/x");
    run_printed(root, &step, &[], &["-q", "origin", "feat/x"]);
    git(root, &["push", "-q", "origin", "feat/x"]);
    assert!(root.join("o.txt").exists(), "the other commit is kept");
}

#[test]
fn clears_pr_merge_protected() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    prove(
        "PR_MERGE_PROTECTED",
        "git.pr_merge_to_protected",
        || guard(root, "git-guard", "gh pr merge 1"),
        |printed| {
            assert!(printed.contains(".codeflow/policy.json"), "{printed}");
            // The explicit sanction the text names; a human merge needs none.
            write(
                root,
                ".codeflow/policy.json",
                r#"{"git": {"pr_merge_to_protected": "allow"}}"#,
            );
        },
    );
}

#[test]
fn clears_hook_integrity() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let printed = refused(
        &root,
        "git-guard",
        "rm .codeflow/policy.json",
        "git.hook_integrity",
        "HOOK_INTEGRITY",
    );
    let step = printed_command(&printed, "HOOK_INTEGRITY", None);
    assert_passes(&root, "git-guard", &step);
    run_printed(&root, &step, &[], &[]);
}

#[test]
fn clears_guard_unclassifiable() {
    let dir = with_origin();
    let root = dir.path();
    let printed = refused(
        root,
        "git-guard",
        "git push origin $(git branch --show-current)",
        "command unresolved",
        "GUARD_UNCLASSIFIABLE",
    );
    assert!(
        printed.contains("compute a value first and pass it as a literal"),
        "{printed}"
    );
    // The value, computed first, written literally.
    let branch = String::from_utf8(run("git", root, &["branch", "--show-current"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let literal = format!("git push origin {branch}");
    assert_passes(root, "git-guard", &literal);
    git(root, &["push", "-q", "origin", &branch]);
}

#[test]
fn clears_guard_unresolved() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    let printed = refused(
        root,
        "git-guard",
        "git -C $REPO status",
        "target unresolved",
        "GUARD_UNRESOLVED",
    );
    let step = printed_command(&printed, "GUARD_UNRESOLVED", None);
    let literal = step
        .replace("/path/to/repo", root.to_str().unwrap())
        .replace('…', "status");
    assert_passes(root, "git-guard", &literal);
    run_printed(root, &literal, &[], &[]);
}

#[test]
fn clears_guard_alias() {
    let dir = with_origin();
    let root = dir.path();
    git(root, &["config", "alias.sync", "!git push origin feat/x"]);
    prove(
        "GUARD_ALIAS",
        "may be an alias the guard cannot resolve",
        || guard(root, "git-guard", "git sync"),
        |printed| {
            let step = printed_command(printed, "GUARD_ALIAS", None);
            // A git-command alias the guard can read.
            run_printed(root, &step, &[], &["alias.sync", "push origin feat/x"]);
        },
    );
    git(root, &["sync", "-q"]);
}

#[test]
fn clears_headless_peer_run() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    let printed = refused(
        root,
        "exec-guard",
        "claude -p 'review the change'",
        "security.headless_peer_runs",
        "HEADLESS_PEER_RUN",
    );
    let step = printed_command(&printed, "HEADLESS_PEER_RUN", None);
    // The interactive lane starts with a delegate run of its own; the turn
    // itself runs in the peer's interactive CLI.
    // Its state lives outside any worktree.
    let outside = tempfile::tempdir().unwrap();
    let state = outside.path().join("run");
    let init = format!(
        "{step} init --run-id review-1 --state-dir {}",
        state.display()
    );
    assert_passes(root, "exec-guard", &init);
    run_printed(root, &init, &[], &[]);
    assert!(state.is_dir(), "the delegate run exists");
}

#[test]
fn clears_session_summary_unwritten() {
    // A file where the ledger needs a directory, and (the reviewer's round
    // 3 case) a directory where it writes its data file: each names the
    // path that failed and the repair its kind needs.
    for (blocker, is_dir, repair) in [
        (
            ".git/codeflow",
            false,
            "remove or rename the file that stands where the ledger needs a directory: ",
        ),
        (
            ".git/codeflow/ledger/sessions/sessions.jsonl",
            true,
            "remove the directory that stands where the ledger writes a file: ",
        ),
    ] {
        let dir = ci_repo(DEFAULTS);
        let root = dir.path();
        if is_dir {
            std::fs::create_dir_all(root.join(blocker)).unwrap();
        } else {
            std::fs::write(root.join(blocker), "not a directory\n").unwrap();
        }
        prove(
            "SESSION_SUMMARY_UNWRITTEN",
            "session ledger not written",
            || hook_with_stdin(root, &["hook", "session-summary"], b"{}"),
            |printed| {
                let named = printed
                    .split(repair)
                    .nth(1)
                    .and_then(|rest| rest.split("; the session ledger").next())
                    .unwrap_or_else(|| panic!("{blocker}: no repair named:\n{printed}"));
                let named = Path::new(named);
                assert!(named.ends_with(blocker), "{blocker}: {printed}");
                if is_dir {
                    std::fs::remove_dir(named).unwrap();
                } else {
                    std::fs::remove_file(named).unwrap();
                }
            },
        );
        let after = hook_with_stdin(root, &["hook", "session-summary"], b"{}");
        assert!(after.contains("recorded to"), "{blocker}: {after}");
    }
}

/// Run the `.claude/settings.json` entry that wires `hook <guard>` the way
/// Claude Code does: its command in a shell, the tool payload on stdin.
fn wired_guard(root: &Path, guard: &str, payload: &str) -> String {
    let settings: serde_json::Value =
        serde_json::from_str(&read(root, ".claude/settings.json")).unwrap();
    let wired = settings["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap().iter())
        .filter_map(|hook| hook["command"].as_str())
        .find(|command| command.contains(&format!("hook {guard}")))
        .unwrap_or_else(|| panic!("no {guard} entry"))
        .to_string();
    let mut child = command("sh", root)
        .args(["-c", &wired])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), payload.as_bytes()).unwrap();
    drop(child.stdin.take());
    text(&child.wait_with_output().unwrap())
}

#[test]
fn clears_guard_payload_malformed() {
    // TSK-147 round 3 F5: a hook entry that does not pass the harness
    // payload through (here it pipes other text into the guard) is a local
    // repair, not a release to install.
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let payload = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    for guard in ["git-guard", "exec-guard"] {
        let settings = read(&root, ".claude/settings.json");
        let shipped = format!("\"codeflow hook {guard}\"");
        assert!(settings.contains(&shipped), "{guard}");
        write(
            &root,
            ".claude/settings.json",
            &settings.replace(
                &shipped,
                &format!("\"printf not-json | codeflow hook {guard}\""),
            ),
        );
        prove(
            "GUARD_PAYLOAD_MALFORMED",
            "unreadable hook payload",
            || wired_guard(&root, guard, payload),
            |printed| {
                assert!(
                    printed.contains(&format!("`codeflow hook {guard}`")),
                    "{printed}"
                );
                assert!(printed.contains("`.claude/settings.json`"), "{printed}");
                // The entry passes the payload through unchanged again.
                write(&root, ".claude/settings.json", &settings);
            },
        );
    }
}

#[test]
fn clears_hook_unevaluated() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    write(&root, "x.txt", "x\n");
    git(&root, &["add", "x.txt"]);
    // A hook manager that calls the hook without git's message file.
    let finding = "cannot read the message file";
    let before = codeflow(&root, &["git-hook", "commit-msg", "MISSING_MSG"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "HOOK_UNEVALUATED");
    assert!(
        printed.contains("MISSING_MSG"),
        "the cause is named:\n{printed}"
    );
    // The git command, rerun, hands the hook its own message file.
    let after = commit_all(&root, "feat: add x");
    assert!(!after.contains(finding), "{after}");
    assert!(!after.contains("check skipped"), "{after}");
}

#[test]
fn clears_hook_stdin_unread() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    // A caller that hands the hook a stdin it cannot read: a directory.
    let finding = "could not read hook stdin";
    let unreadable = std::fs::File::open(&root).unwrap();
    let before = text(
        &command(exe().to_str().unwrap(), &root)
            .args(["git-hook", "pre-push", "origin", dest.to_str().unwrap()])
            .stdin(Stdio::from(unreadable))
            .output()
            .unwrap(),
    );
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "HOOK_STDIN_UNREAD");
    let step = printed_command(&printed, "HOOK_STDIN_UNREAD", None);
    let after = run_printed(&root, &step, &[], &["-q", "origin", "HEAD"]);
    assert!(!after.contains(finding), "{after}");
}

#[test]
#[cfg(unix)]
fn clears_hook_stdin_unread_for_a_branch_name_that_is_not_utf8() {
    // The second cause the row names. Git keeps such a name in packed-refs
    // (APFS refuses it as a loose ref file, and git then cannot rename or
    // delete it there), and pre-push, which reads its refs as UTF-8, prints
    // the finding; the ref-transaction hook stays fail-closed for it
    // (dffeb9dfc).
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    let old = b"refs/heads/caf\xe9";
    let mut packed = format!("{} ", head(&root, "HEAD")).into_bytes();
    packed.extend_from_slice(old);
    packed.push(b'\n');
    std::fs::write(root.join(".git/packed-refs"), &packed).unwrap();
    let git_raw = |args: &[&[u8]]| {
        text(
            &command("git", &root)
                .args(args.iter().map(|arg| OsStr::from_bytes(arg)))
                .output()
                .unwrap(),
        )
    };
    let finding = "could not read hook stdin";
    let before = git_raw(&[b"push", b"origin", old]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "HOOK_STDIN_UNREAD");
    let rename = printed
        .split('`')
        .skip(1)
        .step_by(2)
        .find(|span| span.starts_with("git branch <new> <old>"))
        .unwrap_or_else(|| panic!("no rename step printed:\n{printed}"));
    assert_eq!(rename, "git branch <new> <old>");
    // As printed: the work under a UTF-8 name, and that name pushed.
    let out = command("git", &root)
        .args([
            OsStr::new("branch"),
            OsStr::new("feat/cafe"),
            OsStr::from_bytes(old),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let after = run_printed(&root, "git push", &[], &["-q", "origin", "feat/cafe"]);
    assert!(!after.contains(finding), "{after}");
}
