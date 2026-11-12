//! TSK-1456 AC-9: every expansion the shell applies to the word in the
//! program position is read exactly by the guards or refused. Each row is a
//! hook payload string given to the built binary; nothing in it runs.
use super::{
    codeflow, git, guard_payload, init_repo, line_allows, run_with_stdin, write_agent_policy,
};
use std::path::Path;
use std::process::Output;

/// The policy of the measured fixture: the secret-read and outward rules
/// off, so a row is refused only by the rule it is about.
const MEASURED_POLICY: &str = r#"{"git":{"root_checkout_commits":"off","hook_integrity":"block"},"security":{"secret_reads":"off","outward_actions":"off","headless_peer_runs":"block"}}"#;

/// Run hook `name` on `command`. With `runners`, the hook process has a
/// pager and an editor set, as an agent's shell usually does, so the
/// guard has a program to read where Git would run one (`git diff`, `git
/// log`, `git commit`). The guard only reads these names; no pager or
/// editor runs.
fn hook(root: &Path, name: &str, command: &str, runners: bool) -> Output {
    let mut cmd = codeflow();
    cmd.args(["hook", name]).current_dir(root);
    if runners {
        cmd.env("PAGER", "less")
            .env("EDITOR", "nvim")
            .env("VISUAL", "nvim");
    }
    run_with_stdin(&mut cmd, &guard_payload(command, root))
}

/// Every row whose verdict is wrong, with the hook and what it printed.
fn check(root: &Path, rows: &[(&str, &str, Option<&str>)]) -> Vec<String> {
    check_with(root, rows, false)
}

/// [`check`], with a pager and an editor set when `runners` ([`hook`]).
fn check_with(root: &Path, rows: &[(&str, &str, Option<&str>)], runners: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, command, rule) in rows {
        let out = hook(root, name, command, runners);
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

/// Every row that is not refused by its rule with its reason in the
/// message, with the hook and what it printed.
fn check_reasons(root: &Path, rows: &[(&str, &str, &str, &str)]) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, command, rule, reason) in rows {
        let out = hook(root, name, command, false);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.code() != Some(2) || !stderr.contains(rule) || !stderr.contains(reason) {
            wrong.push(format!(
                "{name}: {command:?}: expected {rule} for {reason:?}, exited {:?}: {stderr}",
                out.status.code()
            ));
        }
    }
    wrong
}

/// The reason git-guard gives for a word in the program position it
/// cannot read.
const UNREAD_PROGRAM: &str =
    "a word in the program position that the guard cannot read decides which program runs";

/// The reason git-guard gives for a program named by a substitution.
const SUBSTITUTED_PROGRAM: &str =
    "a command substitution in the program name decides which program runs";

/// Brace lists, zsh's `=name` and a file substitution in the program
/// position: each is read as the shell reads it, or refused.
#[test]
fn exact_program_word_expansions_are_judged() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let rows = [
        ("git-guard", "{git,push} --force origin main", force),
        ("exec-guard", "{claude,-p} x", headless),
        ("exec-guard", "{claude,'-p'} x", headless),
        ("git-guard", "=git push --force origin main", force),
        ("exec-guard", "=claude -p x", headless),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let protected = "git.commit_to_protected";
    let rows = [
        (
            "git-guard",
            "<(echo git) push --force origin main",
            protected,
            UNREAD_PROGRAM,
        ),
        (
            "git-guard",
            ">(echo git) push --force origin main",
            protected,
            UNREAD_PROGRAM,
        ),
        ("git-guard", "X=1 <(echo git) x", protected, UNREAD_PROGRAM),
        (
            "git-guard",
            "=(echo git) push --force origin main",
            protected,
            UNREAD_PROGRAM,
        ),
    ];
    let wrong = check_reasons(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A word the shell reads literally because it is quoted or escaped is
/// read literally by the guards too: brace and pattern readings apply only
/// to text the shell reads unquoted. Main allows each row at both guards;
/// the real expansions are refused in the tests above.
#[test]
fn quoted_program_words_are_read_literally() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    let mut rows = Vec::new();
    for command in [
        "'{claude,-p}' x",
        "\\{claude,-p\\} x",
        "{claude','-p} x",
        "\"{git,push}\" --force origin main",
        "'gi*' push --force origin main",
        "gi\\* push --force origin main",
        "\"/usr/bin/gi?\" push --force origin main",
        "'/usr/bin/git(N)' push --force origin main",
        "'$G' push --force origin main",
        "'=git' push --force origin main",
    ] {
        rows.push(("git-guard", command, None));
        rows.push(("exec-guard", command, None));
    }
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The program word of an unread reading is judged as Git whatever its
/// first argument is, so an alias of the repository is read as literal
/// `git` reads it.
#[test]
fn unread_program_words_are_judged_through_git_aliases() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(
        dir.path(),
        &["config", "alias.ship", "push --force origin main"],
    );
    let force = "git.force_push_protected";
    let rows = [
        ("git-guard", "$G ship", force, "$G"),
        ("git-guard", "${G:-git} ship", force, "${G:-git}"),
        ("git-guard", "$G -C . ship", force, "$G"),
    ];
    let mut wrong = Vec::new();
    for (name, command, rule, word) in rows {
        let out = hook(dir.path(), name, command, false);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let way = format!("Write the program name literally in place of `{word}`");
        if out.status.code() != Some(2) || !stderr.contains(rule) || !stderr.contains(&way) {
            wrong.push(format!(
                "{name}: {command:?}: expected {rule} with the way through, exited {:?}: {stderr}",
                out.status.code()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // Literal `git` reads the alias the same way, and a word that is no
    // alias is allowed as `git` allows it.
    let rows = [
        ("git-guard", "git ship", Some(force)),
        ("git-guard", "$G status", None),
        ("git-guard", "$TOOL data/in.txt", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The expansions the shell applies only in some forms (`~+`, `~-` and
/// zsh's glob qualifiers) in the program position: each keeps main's
/// verdict, or is read under each guarded name and refused with the way
/// through.
#[test]
fn rarer_program_word_expansions_have_their_verdicts() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let integrity = Some("git.hook_integrity");
    // Main's verdicts: `~+` and `~-` name a directory the guard reads as
    // unknown, so main refuses a guarded program behind them.
    let rows = [
        ("git-guard", "~+/git push --force origin main", force),
        ("git-guard", "~-/git push --force origin main", force),
        ("exec-guard", "~+/git push --force origin main", integrity),
        ("exec-guard", "~+/claude -p x", headless),
        ("exec-guard", "~-/claude -p x", headless),
        ("git-guard", "~+/bin/tool data/in.txt", None),
        ("exec-guard", "~+/bin/tool data/in.txt", None),
        ("git-guard", "python3(N) x.py", None),
        ("exec-guard", "python3(N) x.py", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // A glob qualifier makes the word a pattern: read under each guarded
    // name, refused with the way through.
    let rows = [
        (
            "git-guard",
            "/usr/bin/git(N) push --force origin main",
            "git.force_push_protected",
            "/usr/bin/git(N)",
        ),
        (
            "exec-guard",
            "/usr/local/bin/claude(N) -p x",
            "security.headless_peer_runs",
            "/usr/local/bin/claude(N)",
        ),
    ];
    let mut wrong = Vec::new();
    for (name, command, rule, word) in rows {
        let out = hook(dir.path(), name, command, false);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let way = format!("Write the program name literally in place of `{word}`");
        if out.status.code() != Some(2) || !stderr.contains(rule) || !stderr.contains(&way) {
            wrong.push(format!(
                "{name}: {command:?}: expected {rule} with the way through, exited {:?}: {stderr}",
                out.status.code()
            ));
        }
    }
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
    write_agent_policy(dir.path(), MEASURED_POLICY);
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
    let wrong = check_with(dir.path(), &rows, true);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The new refusals the operator accepted for TSK-1456: a program word the
/// shell fills in at run time is judged as each program it could be, and
/// every such refusal says how to get through, by writing the program name
/// literally. The policy is the measured fixture's, on `main`.
#[test]
fn refused_readings_of_unread_program_words_give_the_way_through() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    let headless = "security.headless_peer_runs";
    let protected = "git.commit_to_protected";
    let integrity = "git.hook_integrity";
    let rows = [
        // Read under the claude, codex or grok name.
        ("exec-guard", "$MKDIR -p dir", headless, "$MKDIR"),
        ("exec-guard", "\"$CP\" -p a b", headless, "$CP"),
        ("exec-guard", "$X -p text", headless, "$X"),
        (
            "exec-guard",
            "$DOCKER run -p 8080:80 img",
            headless,
            "$DOCKER",
        ),
        ("exec-guard", "$SSH -p 2222 host", headless, "$SSH"),
        ("exec-guard", "$MYSQL -u root -p", headless, "$MYSQL"),
        ("exec-guard", "$NPM exec tsc", headless, "$NPM"),
        ("exec-guard", "$PNPM exec vitest", headless, "$PNPM"),
        ("exec-guard", "$DOCKER exec -it c sh", headless, "$DOCKER"),
        (
            "exec-guard",
            "$KUBECTL exec pod -- ls",
            headless,
            "$KUBECTL",
        ),
        ("exec-guard", "$TOOL review file", headless, "$TOOL"),
        ("exec-guard", "$AGENT agent", headless, "$AGENT"),
        ("exec-guard", "$X ultrareview", headless, "$X"),
        // Read as a launcher of a program named by a substitution.
        (
            "git-guard",
            "$PYTHON \"$(git rev-parse --show-toplevel)/scripts/x.py\"",
            protected,
            "$PYTHON",
        ),
        (
            "git-guard",
            "$EDITOR \"$(git rev-parse --show-toplevel)/notes.md\"",
            protected,
            "$EDITOR",
        ),
        // Read under the git name after a command that may move the shell.
        ("git-guard", "$X /tmp; $GIT diff", integrity, "$GIT"),
        ("git-guard", "$X /tmp && $GIT log", integrity, "$GIT"),
        ("git-guard", "$A $B diff", integrity, "$A"),
    ];
    let mut wrong = Vec::new();
    for (name, command, rule, word) in rows {
        let out = hook(dir.path(), name, command, true);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let way = format!("Write the program name literally in place of `{word}`");
        if out.status.code() != Some(2) || !stderr.contains(rule) || !stderr.contains(&way) {
            wrong.push(format!(
                "{name}: {command:?}: expected {rule} with the way through, exited {:?}: {stderr}",
                out.status.code()
            ));
        }
    }
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
    // Command substitution and arithmetic: refused as a program the guard
    // cannot name.
    let protected = "git.commit_to_protected";
    let rows = [
        (
            "git-guard",
            "$(echo git) push --force origin main",
            protected,
            SUBSTITUTED_PROGRAM,
        ),
        (
            "git-guard",
            "`echo git` push --force origin main",
            protected,
            SUBSTITUTED_PROGRAM,
        ),
        (
            "git-guard",
            "$((1)) push --force origin main",
            protected,
            SUBSTITUTED_PROGRAM,
        ),
        (
            "git-guard",
            "gi$((0))t push --force origin main",
            protected,
            SUBSTITUTED_PROGRAM,
        ),
    ];
    let wrong = check_reasons(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let rows = [
        // Tilde.
        ("git-guard", "~/bin/git push --force origin main", force),
        ("git-guard", "~nobody/git push --force origin main", force),
        // A parameter set on the line, with field splitting.
        ("git-guard", "G=git; $G push --force origin main", force),
        ("git-guard", "G='git push'; $G --force origin main", force),
        ("exec-guard", "$(echo claude) -p x", headless),
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
