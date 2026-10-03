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

/// Model the operator's policy landing, rather than a session-local edit.
fn land_policy(root: &Path) {
    git(root, &["add", ".codeflow/policy.json"]);
    git(root, &["commit", "-qm", "chore: land policy decision"]);
    if !String::from_utf8(run("git", root, &["remote"]).stdout)
        .unwrap()
        .trim()
        .is_empty()
    {
        git(root, &["push", "-q", "origin", "HEAD:main"]);
        git(root, &["fetch", "-q", "origin"]);
    }
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
fn clears_root_checkout_commit() {
    // A commit at the root checkout on a feature branch (TSK-165): the
    // printed `git switch <root>` puts the root back on its root branch,
    // where the rule has no finding.
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    prove(
        "ROOT_CHECKOUT_COMMIT",
        "git.root_checkout_commits",
        || guard(root, "git-guard", "git commit -m \"feat: add x\""),
        |printed| {
            assert!(printed.contains("on 'feat/x'"), "{printed}");
            let step = printed_command(printed, "ROOT_CHECKOUT_COMMIT", None);
            assert_eq!(step, "git switch main");
            run_printed(root, &step, &[], &[]);
        },
    );
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
            land_policy(root);
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
    // The revert goes on its own branch in its own worktree, which lands
    // like any other work; the root checkout stays on main (TSK-165).
    git(
        root,
        &["worktree", "add", "-q", ".worktrees/undo", "-b", "fix/undo"],
    );
    let undo = format!(
        "git -C .worktrees/undo {} --no-edit HEAD",
        step.trim_start_matches("git ")
    );
    assert_passes(root, "git-guard", &undo);
    let worktree = root.join(".worktrees/undo");
    run_printed(&worktree, &step, &[], &["--no-edit", "HEAD"]);
    assert_eq!(head(root, "main"), mistake, "main keeps its history");
    assert!(
        !worktree.join("mistake.txt").exists(),
        "the revert undid it"
    );
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
        // The fixture pulls at its root checkout on feat/x; the
        // root-checkout rule (TSK-165) has its own proof, so it is off here.
        r#"{"git": {"force_push_unprotected": "block", "root_checkout_commits": "off"}}"#,
    );
    git(
        root,
        &["commit", "-q", "-am", "chore: restrict force pushes"],
    );
    git(root, &["push", "-q", "origin", "feat/x", "feat/x:main"]);
    git(root, &["fetch", "-q", "origin"]);
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
    let dir = with_origin();
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
            land_policy(root);
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
    // A bare word in a Bash command: on Windows with `/`, which Bash keeps.
    let literal = step
        .replace(
            "/path/to/repo",
            &codeflow_core::portable_path::slashed(root),
        )
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

// The interactive lane the remedy names starts with `codeflow delegate
// init`, which native Windows refuses by design (use WSL2), so the clearing
// step exists only on Unix.
#[cfg(unix)]
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

#[test]
fn clears_refusal_unrecorded() {
    // TSK-149: a directory where the refusals ledger writes its data file.
    // The guard still refuses; the warning names the path and its repair,
    // and after the repair the next refusal is recorded.
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    let blocker = ".git/codeflow/ledger/refusals/refusals.jsonl";
    std::fs::create_dir_all(root.join(blocker)).unwrap();
    let repair = "remove the directory that stands where the ledger writes a file: ";
    prove(
        "REFUSAL_UNRECORDED",
        "refusal not recorded",
        || guard(root, "git-guard", "git push origin main"),
        |printed| {
            let named = printed
                .split(repair)
                .nth(1)
                .and_then(|rest| rest.split("; the refusals ledger").next())
                .unwrap_or_else(|| panic!("no repair named:\n{printed}"));
            let named = Path::new(named);
            assert!(named.ends_with(blocker), "{printed}");
            std::fs::remove_dir(named).unwrap();
        },
    );
    assert!(
        read(root, blocker).contains(r#""event":"refusal""#),
        "the refusal after the repair is recorded"
    );
}

#[cfg(unix)]
#[test]
fn clears_refusal_unrecorded_behind_a_held_lock() {
    // TSK-149 review round 1: another process holds the ledger's lock past
    // the bounded wait. The guard still refuses; the warning names the lock
    // file, and once its holder ends the next refusal is recorded.
    use std::os::unix::io::AsRawFd;
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    let ledger = ".git/codeflow/ledger/refusals/refusals.jsonl";
    let lock = root.join(format!("{ledger}.lock"));
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let holder = std::fs::File::create(&lock).unwrap();
    // SAFETY: flock on a descriptor this test owns.
    assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX) }, 0);
    let holder = std::cell::RefCell::new(Some(holder));
    let repair = "end the process that holds the lock on the ledger file, or wait for it: ";
    prove(
        "REFUSAL_UNRECORDED",
        "refusal not recorded",
        || guard(root, "git-guard", "git push origin main"),
        |printed| {
            let named = printed
                .split(repair)
                .nth(1)
                .and_then(|rest| rest.split("; the refusals ledger").next())
                .unwrap_or_else(|| panic!("no repair named:\n{printed}"));
            assert!(
                Path::new(named).ends_with(format!("{ledger}.lock")),
                "{printed}"
            );
            // The holder ends.
            holder.borrow_mut().take();
        },
    );
    assert!(read(root, ledger).contains(r#""event":"refusal""#));
}

/// Run the `.claude/settings.json` entry that wires `hook <guard>` the way
/// Claude Code does: its command in a shell, the tool payload on stdin.
fn wired_command(root: &Path, guard: &str) -> String {
    let settings: serde_json::Value =
        serde_json::from_str(&read(root, ".claude/settings.json")).unwrap();
    settings["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap().iter())
        .filter_map(|hook| hook["command"].as_str())
        .find(|command| command.contains(&format!("hook {guard}")))
        .unwrap_or_else(|| panic!("no {guard} entry"))
        .to_string()
}

fn wired_guard(root: &Path, guard: &str, payload: &str) -> String {
    let wired = wired_command(root, guard);
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
    // payload through is a local repair, not a release to install. Round 4:
    // that holds for an entry that sends text that is not JSON and for one
    // that sends an object whose known field has the wrong type (a string
    // `tool_input`, a numeric `tool_name`, an array `command`).
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let payload = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let mangled = [
        "printf not-json",
        r#"printf '%s' '{"tool_name":"Bash","tool_input":"git status"}'"#,
        r#"printf '%s' '{"tool_name":5,"tool_input":{"command":"git status"}}'"#,
        r#"printf '%s' '{"tool_name":"Bash","tool_input":{"command":["git","status"]}}'"#,
    ];
    for guard in ["git-guard", "exec-guard"] {
        let settings = read(&root, ".claude/settings.json");
        let shipped = serde_json::to_string(&wired_command(&root, guard)).unwrap();
        assert!(settings.contains(&shipped), "{guard}");
        for sender in mangled {
            let entry =
                serde_json::to_string(&format!("{sender} | codeflow hook {guard}")).unwrap();
            write(
                &root,
                ".claude/settings.json",
                &settings.replace(&shipped, &entry),
            );
            prove(
                "GUARD_PAYLOAD_MALFORMED",
                "unreadable hook payload",
                || wired_guard(&root, guard, payload),
                |printed| {
                    assert!(
                        printed.contains(&format!("`codeflow hook {guard}`")),
                        "{sender}: {printed}"
                    );
                    assert!(printed.contains("`.claude/settings.json`"), "{printed}");
                    assert!(
                        !printed.contains("release"),
                        "a local repair only: {printed}"
                    );
                    // The entry passes the payload through unchanged again.
                    write(&root, ".claude/settings.json", &settings);
                },
            );
        }
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
    // A caller that hands the hook a stdin it cannot read: a directory, or
    // on Windows, which opens no directory as a file, a write-only handle.
    let finding = "could not read hook stdin";
    #[cfg(not(windows))]
    let unreadable = std::fs::File::open(&root).unwrap();
    #[cfg(windows)]
    let unreadable = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(root.parent().unwrap().join("stdin-write-only"))
        .unwrap();
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

#[test]
fn clears_discard_local_work() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    write(root, "seed.txt", "locally changed\n");
    prove(
        "DISCARD_LOCAL_WORK",
        "git.discard_uncommitted",
        || guard(root, "git-guard", "git reset --hard"),
        |printed| {
            let step = printed_command(printed, "DISCARD_LOCAL_WORK", None);
            assert_eq!(step, "git stash push");
            assert_passes(root, "git-guard", &step);
            run_printed(root, &step, &[], &[]);
        },
    );
    assert!(text(&run("git", root, &["stash", "show", "-p"])).contains("locally changed"));
}

#[test]
fn clears_push_without_follow_tags() {
    let dir = with_origin();
    let root = dir.path();
    git(root, &["config", "push.followTags", "true"]);
    let printed = refused(
        root,
        "exec-guard",
        "git push origin feat/x",
        "security.outward_actions",
        "PUSH_WITHOUT_FOLLOW_TAGS",
    );
    let step = printed_command(&printed, "PUSH_WITHOUT_FOLLOW_TAGS", None)
        .replace("<remote>", "origin")
        .replace("<branch>", "feat/x");
    assert_passes(root, "exec-guard", &step);
    run_printed(root, &step, &[], &[]);
    assert_eq!(head(root, "origin/feat/x"), head(root, "feat/x"));
}
