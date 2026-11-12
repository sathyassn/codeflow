//! TSK-1456: the guards find the program a command line runs behind the
//! words in front of it. Each row is a hook payload string given to the
//! built binary; nothing in it runs.
use super::{codeflow, guard_payload, init_repo, line_allows, run_with_stdin};
use std::path::Path;
use std::process::Output;

fn hook(root: &Path, name: &str, command: &str) -> Output {
    run_with_stdin(
        codeflow().args(["hook", name]).current_dir(root),
        &guard_payload(command, root),
    )
}

/// Every row whose verdict is wrong, with the hook and what it printed.
fn check(root: &Path, rows: &[(&str, &str, Option<&str>)]) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, command, rule) in rows {
        let out = hook(root, name, command);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let right = match rule {
            Some(rule) => out.status.code() == Some(2) && stderr.contains(rule),
            None if *name == "exec-guard" => line_allows(&out),
            None => out.status.code() == Some(0),
        };
        if !right {
            let expected = rule.map_or("allow".to_string(), |rule| format!("refuse by {rule}"));
            wrong.push(format!(
                "{name}: {command:?}: expected {expected}, exited {:?}: {stderr}",
                out.status.code()
            ));
        }
    }
    wrong
}

/// A word in front of the program that a shell or `env` may read as an
/// assignment never hides the program: each command after it is judged.
#[test]
fn the_program_behind_unplaced_words_is_judged() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let rows = [
        ("git-guard", "X+=1 git push --force origin main", force),
        ("git-guard", "a[0]=1 git push --force origin main", force),
        (
            "git-guard",
            "env X[0]=1 git push --force origin main",
            force,
        ),
        ("git-guard", "env {X=1,git} push --force origin main", force),
        ("git-guard", "A=1 X+=1 git push --force origin main", force),
        ("exec-guard", "X+=1 claude -p x", headless),
        ("exec-guard", "a[0]=1 claude -p x", headless),
        ("exec-guard", "env X[0]=1 claude -p x", headless),
        ("exec-guard", "env {X=1,claude} -p x", headless),
        ("exec-guard", "sudo X+=1 claude -p x", headless),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Main's verdicts that the change keeps: refusals, readings and controls,
/// each measured on main before the change.
#[test]
fn main_verdicts_hold_for_leading_words() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let authority = Some("git.policy_authority");
    let integrity = Some("git.hook_integrity");
    let rows = [
        // Refusals main makes.
        ("exec-guard", "env -S 'A=1' claude -p x", headless),
        ("exec-guard", "env -S 'A=1 claude -p x'", headless),
        ("exec-guard", "genv -S 'A=1' claude -p x", headless),
        ("exec-guard", "genv -S 'A=1 claude -p x'", headless),
        (
            "git-guard",
            "env --split-string='-i A=1' git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "env -S 'A=1' git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "env -u FOO git push --force origin main",
            force,
        ),
        ("exec-guard", "env -u FOO claude -p x", headless),
        // Readings main makes.
        ("git-guard", "'=git' x", None),
        ("exec-guard", "'=git' x", None),
        ("git-guard", "env X[0]=1 git push", authority),
        ("git-guard", "env -i PATH=/bin git log", integrity),
        ("git-guard", "env -u FOO git status", integrity),
        ("exec-guard", "env -u FOO git status", integrity),
        ("git-guard", "env /tmp/a=b/git git status", integrity),
        // Controls.
        ("git-guard", "FOO=1 git status", None),
        ("git-guard", "A=b=c git status", None),
        ("git-guard", "X+=1 git status", None),
        ("git-guard", "a[0]=1 git status", None),
        ("git-guard", "/tmp/a=b/git push origin task/x", None),
        ("git-guard", "export FOO=1; git status", None),
        ("git-guard", "make CC=gcc", None),
        ("git-guard", "(( a == b )) && git status", None),
        ("exec-guard", "FOO=1 git status", None),
        ("exec-guard", "X+=1 git status", None),
        ("exec-guard", "env FOO=1 ls -p", None),
        ("exec-guard", "make CC=gcc", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
