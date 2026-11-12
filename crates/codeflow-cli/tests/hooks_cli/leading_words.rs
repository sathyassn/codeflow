//! TSK-1456: the guards find the program a command line runs behind the
//! words in front of it. Each row is a hook payload string given to the
//! built binary; nothing in it runs.
use super::{codeflow, guard_payload, init_repo, line_allows, run_with_stdin, write_agent_policy};
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
            // Refused by this rule and by no other.
            Some(rule) if rule.starts_with("only ") => {
                out.status.code() == Some(2)
                    && stderr.matches("policy rule ").count() == 1
                    && stderr.contains(&rule["only ".len()..])
            }
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

/// `env` reads every operand before its command that holds `=` as an
/// assignment, whatever stands before the `=`, so the program after such
/// operands is judged. The secret-read rule is off, so each row is refused
/// by the headless rule alone.
#[test]
fn every_env_operand_before_the_command_is_read_as_env_reads_it() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(
        dir.path(),
        r#"{"git":{"root_checkout_commits":"off","hook_integrity":"block"},"security":{"secret_reads":"off","outward_actions":"off","headless_peer_runs":"block"}}"#,
    );
    let only_headless = Some("only policy rule security.headless_peer_runs");
    let rows = [
        ("exec-guard", "env '=x' claude -p x", only_headless),
        ("exec-guard", "env =x claude -p x", only_headless),
        ("exec-guard", "env = claude -p x", only_headless),
        ("exec-guard", "env -i '=x' codex exec y", only_headless),
        (
            "exec-guard",
            "sudo '=x' claude -p x",
            Some("policy rule security.headless_peer_runs"),
        ),
        (
            "git-guard",
            "env '=x' git push --force origin main",
            Some("git.force_push_protected"),
        ),
        // A shell word with nothing before its `=` is the program.
        ("exec-guard", "'=x' claude -p x", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A word the text reads as an assignment but the shell runs as the
/// program, because a quote or escape stands in its name or its `=`, is
/// judged as the program as well; the same word written plainly stays an
/// assignment.
#[test]
fn words_the_shell_runs_as_the_program_are_judged_as_the_program() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let rows = [
        (
            "git-guard",
            "'X=/usr/bin/git' push --force origin main",
            force,
        ),
        (
            "git-guard",
            "\"X\"=/usr/bin/git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "X\\=/usr/bin/git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "nohup 'X=/usr/bin/git' push --force origin main",
            force,
        ),
        (
            "git-guard",
            "X+=/usr/bin/git push --force origin main",
            force,
        ),
        ("exec-guard", "'X=/usr/local/bin/claude' -p x", headless),
        ("exec-guard", "X+=/usr/local/bin/claude -p x", headless),
        // A quote before the `=` makes the word no assignment even when it
        // quotes nothing.
        (
            "git-guard",
            "X''=/usr/bin/git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "X\"\"=/usr/bin/git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "X$''=/usr/bin/git push --force origin main",
            force,
        ),
        (
            "git-guard",
            "''X=/usr/bin/git push --force origin main",
            force,
        ),
        ("exec-guard", "X''=/usr/local/bin/claude -p x", headless),
        ("exec-guard", "X\"\"=/usr/local/bin/claude -p x", headless),
        ("exec-guard", "X$''=/usr/local/bin/claude -p x", headless),
        ("git-guard", "X='' git status", None),
        ("git-guard", "X=\"\" git status", None),
        ("exec-guard", "X='' git status", None),
        (
            "git-guard",
            "'X='$(echo /usr/bin/git) push --force origin main",
            Some("a command substitution in the program name"),
        ),
        // Controls.
        ("git-guard", "'X=1' git status", None),
        ("exec-guard", "'X=1' git status", None),
        ("git-guard", "X=/usr/bin/git git status", None),
        ("git-guard", "X+=1 git status", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A value the shell keeps in one word, blanks included, never hides the
/// program after it: each command is judged as the shell reads its words.
#[test]
fn values_the_shell_keeps_in_one_word_never_hide_the_program() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(
        dir.path(),
        r#"{"git":{"root_checkout_commits":"off","hook_integrity":"block"},"security":{"secret_reads":"off","outward_actions":"block","headless_peer_runs":"block"}}"#,
    );
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let rows = [
        ("git-guard", "X=$[ 1 ] git push --force origin main", force),
        ("git-guard", "X=${v: 1} git push --force origin main", force),
        (
            "git-guard",
            "X=$[ 1 ] git push --no-verify origin main",
            Some("git.no_verify_bypass"),
        ),
        (
            "git-guard",
            "X=${a[ 1 ]} bash -c 'git push --force origin main'",
            force,
        ),
        (
            "git-guard",
            "X=${v:- x} sh -c 'git push --force origin main'",
            force,
        ),
        (
            "git-guard",
            "env X=${v: 1} git push --force origin main",
            force,
        ),
        ("exec-guard", "X=$[ 1 ] claude -p x", headless),
        ("exec-guard", "X=${v+ x} codex exec x", headless),
        (
            "exec-guard",
            "X=${v: 1} gh release create v1",
            Some("security.outward_actions"),
        ),
        // Controls.
        ("git-guard", "X=${v: 1} git status", None),
        ("git-guard", "X=$[ 1 + 1 ] git status", None),
        ("exec-guard", "X=${v:-a b} ls -p", None),
        ("exec-guard", "echo ${v:- a b}", None),
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
