//! TSK-1456 AC-9: every expansion the shell applies to the word in the
//! program position is read exactly by the guards or refused. Each row is a
//! hook payload string given to the built binary; nothing in it runs.
use super::{codeflow, guard_payload, init_repo, line_allows, run_with_stdin, write_agent_policy};
use std::path::Path;
use std::process::Output;

fn hook(root: &Path, name: &str, command: &str, env: &[(&str, &str)]) -> Output {
    run_with_stdin(
        codeflow()
            .args(["hook", name])
            .envs(env.iter().copied())
            .current_dir(root),
        &guard_payload(command, root),
    )
}

/// Every row whose verdict is wrong, with the hook and what it printed.
fn check(root: &Path, rows: &[(&str, &str, Option<&str>)]) -> Vec<String> {
    check_with(root, rows, &[])
}

/// [`check`] with the hooks given `env`.
fn check_with(
    root: &Path,
    rows: &[(&str, &str, Option<&str>)],
    env: &[(&str, &str)],
) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, command, rule) in rows {
        let out = hook(root, name, command, env);
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

/// Brace lists, zsh's `=name` and a file substitution in the program
/// position: each is read as the shell reads it, or refused.
#[test]
fn exact_program_word_expansions_are_judged() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let refused = Some("policy rule git.");
    let rows = [
        ("git-guard", "{git,push} --force origin main", force),
        ("exec-guard", "{claude,-p} x", headless),
        ("git-guard", "=git push --force origin main", force),
        ("exec-guard", "=claude -p x", headless),
        ("git-guard", "<(echo git) push --force origin main", refused),
        ("git-guard", ">(echo git) push --force origin main", refused),
        ("git-guard", "X=1 <(echo git) x", refused),
        ("git-guard", "=(echo git) push --force origin main", refused),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A program word the guard cannot fill in (an environment value or a
/// pattern) is judged once under each guarded program name and as a
/// launcher of the words after it; any refusing reading refuses the line.
#[test]
fn unread_program_words_are_judged_under_each_guarded_name() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let rows = [
        ("git-guard", "$G push --force origin main", force),
        ("git-guard", "${G} push --force origin main", force),
        ("git-guard", "${G:-git} push --force origin main", force),
        ("git-guard", "$1 push --force origin main", force),
        ("exec-guard", "$C -p x", headless),
        ("git-guard", "gi* push --force origin main", force),
        ("git-guard", "/usr/bin/gi? push --force origin main", force),
        (
            "git-guard",
            "/usr/bin/g[i]t push --force origin main",
            force,
        ),
        ("exec-guard", "/usr/local/bin/clau* -p x", headless),
        // As a launcher, a shell or `eval`.
        ("git-guard", "$N -n 5 git push --force origin main", force),
        ("git-guard", "$S -c 'git push --force origin main'", force),
        ("git-guard", "$E 'git push --force origin main'", force),
        // New refusals listed for the operator: read under the claude name.
        ("exec-guard", "$MKDIR -p dir", headless),
        ("exec-guard", "\"$CP\" -p a b", headless),
        ("exec-guard", "$X -p text", headless),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Ordinary lines whose program word the shell expands keep main's
/// verdicts at both guards: git-guard allows each, and exec-guard allows
/// it or, for a program the shell fills in from the environment, refuses
/// it by the secret-read rule alone, as main does.
#[test]
fn ordinary_expanded_program_words_stay_allowed() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let unverified = Some("only policy rule security.secret_reads");
    let mut rows = Vec::new();
    for (command, exec) in [
        ("\"$HOME/bin/tool\" data/in.txt", None),
        ("$HOME/bin/tool data/in.txt", None),
        ("python{3..3} x.py", None),
        ("pyth?n3 x.py", None),
        ("=python3 x.py", None),
        ("${CARGO:-cargo} test", unverified),
        ("\"$EDITOR\" notes.md", unverified),
        ("$PYTHON x.py", unverified),
        ("$CC -o out main.c", unverified),
        ("'=git' x", None),
        ("git status", None),
    ] {
        rows.push(("git-guard", command, None));
        rows.push(("exec-guard", command, exec));
    }
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Ordinary lines where a reading of the program word meets another
/// reader keep main's verdicts: a git reading of a command that runs Git's
/// own programs (a pager, a diff driver, hooks), and the shell and `eval`
/// readings of a quoted argument. The policy is the measured fixture's:
/// main allows each row at both guards there. The scratch repository is a
/// root checkout on a task branch, so the root-checkout rule is off: under
/// it a git reading of `commit` is refused there, as `git commit` is.
#[test]
fn readings_of_ordinary_program_words_keep_main_verdicts() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(
        dir.path(),
        r#"{"git":{"root_checkout_commits":"off","hook_integrity":"block"},"security":{"secret_reads":"off","outward_actions":"off","headless_peer_runs":"block"}}"#,
    );
    let mut rows = Vec::new();
    for command in [
        "$GIT diff",
        "$GIT log --oneline",
        "$GIT commit -m msg",
        "$DOCKER commit c img",
        "$DOCKER tag a b",
        "$KUBECTL diff -f x.yaml",
        "$PYTHON -c 'print(1)'",
        "$NODE -e 'console.log(1)'",
        "$RUBY -e 'puts 1'",
        "$SHELL -c 'echo hi'",
    ] {
        rows.push(("git-guard", command, None));
        rows.push(("exec-guard", command, None));
    }
    // A pager or an editor makes Git run a program it chooses (`git diff`,
    // `git log`, `git commit`), which the guard reads from the directory
    // the command runs in.
    let wrong = check_with(
        dir.path(),
        &rows,
        &[("PAGER", "less"), ("EDITOR", "nvim"), ("VISUAL", "nvim")],
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The expansions main already reads or refuses in the program position
/// keep their verdicts.
#[test]
fn main_verdicts_hold_for_program_word_expansions() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let integrity = Some("git.hook_integrity");
    let refused = Some("policy rule git.");
    let rows = [
        // Tilde.
        ("git-guard", "~/bin/git push --force origin main", force),
        ("git-guard", "~nobody/git push --force origin main", force),
        // A parameter set on the line, with field splitting.
        ("git-guard", "G=git; $G push --force origin main", force),
        ("git-guard", "G='git push'; $G --force origin main", force),
        // Command substitution and arithmetic.
        ("git-guard", "$(echo git) push --force origin main", refused),
        ("git-guard", "`echo git` push --force origin main", refused),
        ("exec-guard", "$(echo claude) -p x", headless),
        ("git-guard", "$((1)) push --force origin main", refused),
        ("git-guard", "gi$((0))t push --force origin main", refused),
        // Quote removal.
        ("git-guard", "\"git\" push --force origin main", force),
        ("git-guard", "g'i't push --force origin main", force),
        ("git-guard", "\\git push --force origin main", force),
        ("git-guard", "$'git' push --force origin main", force),
        ("git-guard", "$\"git\" push --force origin main", force),
        ("exec-guard", "c'laude' -p x", headless),
        // Alias.
        (
            "git-guard",
            "alias g=git; g push --force origin main",
            integrity,
        ),
        (
            "exec-guard",
            "alias g=git; g push --force origin main",
            integrity,
        ),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
