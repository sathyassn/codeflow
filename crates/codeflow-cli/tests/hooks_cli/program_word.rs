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

/// Run hook `name` on `command` with `env` set in the hook process, the
/// session's own environment.
fn hook_env(root: &Path, name: &str, command: &str, env: &[(&str, &str)]) -> Output {
    let mut cmd = codeflow();
    cmd.args(["hook", name]).current_dir(root);
    for (key, value) in env {
        cmd.env(key, value);
    }
    run_with_stdin(&mut cmd, &guard_payload(command, root))
}

/// The policy rules a hook's message names, sorted, each once.
fn refusal_rules(out: &Output) -> Vec<String> {
    let mut rules: Vec<String> = String::from_utf8_lossy(&out.stderr)
        .split("policy rule ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect();
    rules.sort();
    rules.dedup();
    rules
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

/// [`check`], with `env` set in each hook process ([`hook_env`]).
fn check_env(
    root: &Path,
    rows: &[(&str, &str, Option<&str>)],
    env: &[(&str, &str)],
) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, command, rule) in rows {
        let out = hook_env(root, name, command, env);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let right = match rule {
            Some(rule) => out.status.code() == Some(2) && stderr.contains(rule),
            None if *name == "exec-guard" => line_allows(&out),
            None => out.status.code() == Some(0),
        };
        if !right {
            wrong.push(format!(
                "{name} {env:?}: {command:?}: expected {rule:?}, exited {:?}: {stderr}",
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
        ("git-guard", "$G --git-dir .git ship", force, "$G"),
        ("git-guard", "gi* ship", force, "gi*"),
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
    // Literal `git` reads the alias the same way. A first word that is no
    // alias and no builtin keeps main's verdict, as does a quoted `$`.
    let rows = [
        ("git-guard", "git ship", Some(force)),
        ("git-guard", "$G status", None),
        ("git-guard", "$TOOL data/in.txt", None),
        ("git-guard", "$TOOL notes.md", None),
        ("git-guard", "'$G' ship", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// An unread program word whose first argument names an alias of the
/// repository is read by the Git runner reader as literal `git` reads it,
/// so a program the alias makes Git run is judged at exec-guard. A word
/// whose braces or pattern the runner reader cannot read is refused with
/// the way through. The configured program is a string the guard reads;
/// Git never runs here.
#[test]
fn unread_program_words_read_the_runners_of_a_git_alias() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(dir.path(), &["config", "alias.inspect", "diff --ext-diff"]);
    git(dir.path(), &["config", "diff.external", "claude -p x"]);
    std::fs::write(dir.path().join("base.txt"), "base\nchanged\n").unwrap();
    let headless = "security.headless_peer_runs";
    let integrity = "git.hook_integrity";
    let rows = [
        ("exec-guard", "git inspect", Some(headless)),
        ("exec-guard", "$TOOL inspect", Some(headless)),
        ("exec-guard", "\"$TOOL\" inspect", Some(headless)),
        ("exec-guard", "${TOOL:-git} inspect", Some(integrity)),
        ("exec-guard", "gi* inspect", Some(integrity)),
        // A quoted `$` and a first word that is no alias keep main's
        // verdict.
        ("exec-guard", "'$TOOL' inspect", None),
        ("exec-guard", "$TOOL notes.md", None),
    ];
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let mut wrong = Vec::new();
    for word in ["${TOOL:-git}", "gi*"] {
        let out = hook(dir.path(), "exec-guard", &format!("{word} inspect"), false);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.contains(&format!(
            "Write the program name literally in place of `{word}`"
        )) {
            wrong.push(format!("{word}: no way through: {stderr}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// An alias is read in the repository the command runs in after its
/// launchers and assignments, for literal `git` and for an unread program
/// word alike: only the second repository's alias runs a configured
/// program, and the starting repository has no alias of that name, then
/// one that only shows status. At exec-guard each command is refused as
/// literal `git` is: by the configured program's rule, or by
/// `git.hook_integrity` behind an `env` option; at git-guard the unread
/// word is refused wherever literal `git` is. The configured program is a
/// string the guard reads; Git never runs here.
#[test]
fn unread_program_words_read_the_alias_where_the_command_runs() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    init_repo(&other, "task/x");
    write_agent_policy(&other, MEASURED_POLICY);
    git(&other, &["config", "alias.inspect", "diff --ext-diff"]);
    git(
        &other,
        &["config", "alias.ship", "push --force origin main"],
    );
    git(&other, &["config", "diff.external", "claude -p x"]);
    std::fs::write(other.join("base.txt"), "base\nchanged\n").unwrap();
    let mut wrong = moved_alias_rows(dir.path(), false);
    git(dir.path(), &["config", "alias.inspect", "status"]);
    wrong.extend(moved_alias_rows(dir.path(), true));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The rows of [`unread_program_words_read_the_alias_where_the_command_runs`]
/// in `root`, whose own alias of the same name only shows status when
/// `aliased`: every row whose verdict is wrong.
fn moved_alias_rows(root: &Path, aliased: bool) -> Vec<String> {
    let headless = Some("security.headless_peer_runs");
    let integrity = Some("git.hook_integrity");
    // An `env` option in front of Git keeps main's `git.hook_integrity`
    // refusal at exec-guard.
    let moved = [
        ("env -C other {} inspect", integrity),
        ("env --chdir=other {} inspect", integrity),
        ("env --chdir other {} inspect", integrity),
        ("nice env -C other -- {} inspect", headless),
        (
            "env GIT_DIR=other/.git GIT_WORK_TREE=other {} inspect",
            headless,
        ),
        (
            "nice env GIT_DIR=other/.git GIT_WORK_TREE=other {} inspect",
            headless,
        ),
        (
            "GIT_DIR=other/.git GIT_WORK_TREE=other {} inspect",
            headless,
        ),
        // Launchers that set or clear variables, and env's GNU spelling,
        // are read as they act.
        (
            "arch -e GIT_DIR=other/.git -e GIT_WORK_TREE=other {} inspect",
            headless,
        ),
        (
            "exec -c env GIT_DIR=other/.git GIT_WORK_TREE=other {} inspect",
            headless,
        ),
        (
            "genv GIT_DIR=other/.git GIT_WORK_TREE=other {} inspect",
            headless,
        ),
    ];
    // Each command as literal `git` and with the program word filled in
    // at run time, unquoted and in double quotes.
    let mut rows = Vec::new();
    let mut wrong = Vec::new();
    for (shape, rule) in moved {
        let literal = hook(root, "git-guard", &shape.replace("{}", "git"), false);
        for word in ["git", "$TOOL", "\"$TOOL\""] {
            let command = shape.replace("{}", word);
            let out = hook(root, "git-guard", &command, false);
            if literal.status.code() == Some(2) && out.status.code() != Some(2) {
                wrong.push(format!(
                    "git-guard: {command:?}: literal git is refused, exited {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            rows.push(("exec-guard", command, rule));
            // The second repository's alias that force-pushes its
            // protected branch is refused on its own, for literal `git`
            // and the unread word alike.
            let ship = shape.replace("{}", word).replace(" inspect", " ship");
            let out = hook(root, "git-guard", &ship, false);
            if out.status.code() != Some(2) || refusal_rules(&out).is_empty() {
                wrong.push(format!(
                    "git-guard: {ship:?}: a force push is not refused, exited {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    // Main's refusal of an `env` option in front of literal Git holds; an
    // unread word behind one is refused so only where an alias is found.
    rows.push(("exec-guard", "env -i git inspect".to_string(), integrity));
    rows.push((
        "exec-guard",
        "env -i $TOOL inspect".to_string(),
        aliased.then_some("git.hook_integrity"),
    ));
    rows.push((
        "exec-guard",
        "env -C other git status".to_string(),
        integrity,
    ));
    // A word that names no alias where the command runs, and no Git
    // reading: main's verdicts.
    let mut allowed = vec![
        "$TOOL inspect",
        "env -C other $TOOL notes.md",
        "env -C other ls",
    ];
    if aliased {
        allowed.push("git inspect");
    }
    for command in allowed {
        rows.push(("exec-guard", command.to_string(), None));
    }
    let rows: Vec<(&str, &str, Option<&str>)> = rows
        .iter()
        .map(|(name, command, rule)| (*name, command.as_str(), *rule))
        .collect();
    wrong.extend(check(root, &rows));
    wrong
        .into_iter()
        .map(|row| format!("starting alias {aliased}: {row}"))
        .collect()
}

/// An alias the command defines for itself through a variable, which the
/// command assigns in front of Git, is read with the value Git sees: the
/// command's own, whatever the session holds under that name. A variable
/// the guard cannot read is refused for literal `git` and an unread word
/// alike. The configured program is a string the guard reads; Git never
/// runs here.
#[test]
fn unread_program_words_read_an_alias_from_the_command_environment() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(dir.path(), &["config", "diff.external", "claude -p x"]);
    std::fs::write(dir.path().join("base.txt"), "base\nchanged\n").unwrap();
    let headless = Some("security.headless_peer_runs");
    let integrity = Some("git.hook_integrity");
    let mut rows = Vec::new();
    for word in ["git", "$TOOL", "\"$TOOL\""] {
        rows.push((
            format!(
                "K18_ALIAS='diff --ext-diff' {word} --config-env=alias.inspect=K18_ALIAS inspect"
            ),
            headless,
        ));
        rows.push((
            format!("{word} --config-env=alias.inspect=K18_UNSET inspect"),
            integrity,
        ));
    }
    rows.push(("$TOOL inspect".to_string(), None));
    rows.push(("K18_ALIAS=x $TOOL build".to_string(), None));
    let mut wrong = Vec::new();
    // The session without the variable, then holding a different alias
    // under the same name.
    for env in [&[][..], &[("K18_ALIAS", "status")][..]] {
        for (command, rule) in &rows {
            let out = hook_env(dir.path(), "exec-guard", command, env);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let right = match rule {
                Some(rule) => out.status.code() == Some(2) && stderr.contains(rule),
                None => line_allows(&out),
            };
            if !right {
                wrong.push(format!(
                    "exec-guard {env:?}: {command:?}: expected {rule:?}, exited {:?}: {stderr}",
                    out.status.code()
                ));
            }
        }
        // git-guard refuses each aliased row on its own, as it refuses
        // literal `git`, and allows the rows that name no alias. A row
        // whose alias force-pushes the protected branch is refused too.
        let mut judged = rows.clone();
        for word in ["git", "$TOOL", "\"$TOOL\""] {
            judged.push((
                format!(
                    "K18_ALIAS='push --force origin main' {word} --config-env=alias.ship=K18_ALIAS ship"
                ),
                integrity,
            ));
        }
        for (command, rule) in &judged {
            let literal = hook_env(
                dir.path(),
                "git-guard",
                &command.replace("\"$TOOL\"", "git").replace("$TOOL", "git"),
                env,
            );
            let out = hook_env(dir.path(), "git-guard", command, env);
            let refused = out.status.code() == Some(2);
            let right = if rule.is_none() {
                !refused
            } else {
                refused && literal.status.code() == Some(2)
            };
            if !right {
                wrong.push(format!(
                    "git-guard {env:?}: {command:?}: literal exited {:?}, exited {:?}: {}",
                    literal.status.code(),
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// git-guard judges the alias Git runs in the environment the command's
/// launchers and settings give it, so each row is refused for the force
/// push its alias runs, whatever alias the session holds. The command
/// strings are only read; Git never runs them.
#[test]
fn launchers_that_remove_variables_judge_the_alias_git_runs() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(
        dir.path(),
        &["config", "alias.ship", "push --force origin main"],
    );
    git(dir.path(), &["config", "diff.external", "claude -p x"]);
    std::fs::write(dir.path().join("base.txt"), "base\nchanged\n").unwrap();
    let force = Some("git.force_push_protected");
    let headless = Some("security.headless_peer_runs");
    let integrity = Some("git.hook_integrity");
    let unresolved = Some("git.commit_to_protected");
    let refused = Some("BLOCKED");
    let removed = [
        ("exec -c git ship", force),
        ("exec -c \"$G\" ship", force),
        ("git -c alias.ship='push --force origin main' ship", force),
        // Refused before any alias is read, as `main` refuses them: an
        // `env` option in front of Git, and a `-c` value the guard reads
        // as an assignment in front of an unread word.
        ("env -i git ship", integrity),
        ("env -i $G ship", integrity),
        ("env -u GIT_CONFIG_COUNT git ship", integrity),
        ("env -u GIT_CONFIG_COUNT $G ship", integrity),
        (
            "$G -c alias.ship='push --force origin main' ship",
            unresolved,
        ),
        // Git takes `-c` only with its setting as the next word and stops
        // at this one before it reads any alias; the guard refuses it.
        ("git -calias.ship='push --force origin main' ship", refused),
        ("$G -calias.ship='push --force origin main' ship", refused),
    ];
    let mut wrong = Vec::new();
    // The session holds no alias of that name, then one that runs a
    // configured program, then one that only shows status.
    for session in [None, Some("diff --ext-diff"), Some("status")] {
        let mut env = vec![("G", "git")];
        if let Some(value) = session {
            env.extend([
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_CONFIG_KEY_0", "alias.ship"),
                ("GIT_CONFIG_VALUE_0", value),
            ]);
        }
        let mut rows: Vec<(&str, &str, Option<&str>)> = removed
            .iter()
            .map(|(command, rule)| ("git-guard", *command, *rule))
            .collect();
        // Without a launcher, Git runs the session's alias.
        let (git_guard, exec_guard) = match session {
            None => (force, None),
            Some("status") => (None, None),
            Some(_) => (None, headless),
        };
        for command in ["git ship", "$G ship"] {
            rows.push(("git-guard", command, git_guard));
            rows.push(("exec-guard", command, exec_guard));
        }
        wrong.extend(check_env(dir.path(), &rows, &env));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A program-search assignment in front of Git (`PATH`, `GIT_EXEC_PATH`)
/// that extends or keeps a value the session holds is read as the Git
/// runner reader reads it, with the session's value filled in, so a
/// harmless alias keeps `main`'s verdict at both guards. A value the guard
/// cannot fill in stays refused, as `main` refuses it.
#[test]
fn program_search_assignments_keep_the_alias_reading() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(dir.path(), &["config", "alias.inspect", "status"]);
    let integrity = Some("git.hook_integrity");
    let both = [
        // Controls: the plain alias call, and the prefix on a builtin.
        ("git inspect", None),
        ("PATH=\"$PATH\" git status", None),
        ("PATH=\"$PATH\" git inspect", None),
        ("PATH=\"$PATH:$HOME/bin\" git inspect", None),
        ("PATH=/usr/bin:/bin git inspect", None),
        ("GIT_EXEC_PATH=\"$HOME/libexec\" git inspect", None),
        ("PATH=\"$PATH\" $G inspect", None),
        ("G=git; PATH=\"$PATH\" $G inspect", None),
        // A value the guard cannot fill in is refused, as on `main`.
        ("PATH=\"$K18_UNSET\" git inspect", integrity),
        ("GIT_EXEC_PATH=\"$K18_UNSET\" git inspect", integrity),
        ("PATH=\"$(pwd)\" git inspect", integrity),
    ];
    let mut rows: Vec<(&str, &str, Option<&str>)> = Vec::new();
    for (command, rule) in both {
        rows.push(("git-guard", command, rule));
        rows.push(("exec-guard", command, rule));
    }
    // `main` refuses an assignment behind `env` at exec-guard by the text
    // rule, and allows it at git-guard.
    rows.push(("git-guard", "env PATH=\"$PATH\" git inspect", None));
    rows.push((
        "exec-guard",
        "env PATH=\"$PATH\" git inspect",
        Some("security.text_guard"),
    ));
    // With the search path filled in, git-guard judges the alias Git runs.
    git(
        dir.path(),
        &["config", "alias.ship", "push --force origin main"],
    );
    let force = Some("git.force_push_protected");
    for command in [
        "git ship",
        "PATH=\"$PATH\" git ship",
        "GIT_EXEC_PATH=\"$HOME/libexec\" git ship",
        "PATH=\"$PATH\" nice git ship",
        "PATH=\"$PATH\" $G ship",
    ] {
        rows.push(("git-guard", command, force));
    }
    let wrong = check_env(dir.path(), &rows, &[("G", "git")]);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The variable naming the file `git config` reads is removed from every
/// configuration reading, so a launcher that removes it from the session
/// leaves an ordinary command's verdict as literal `git` has it.
#[test]
fn removing_the_config_command_file_keeps_ordinary_verdicts() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    let file = dir.path().join("k18-config");
    let env = [("GIT_CONFIG", file.to_str().unwrap())];
    let mut rows = Vec::new();
    for command in ["git status", "exec -c git status", "exec -c $TOOL build"] {
        rows.push(("exec-guard", command, None));
        rows.push(("git-guard", command, None));
    }
    let wrong = check_env(dir.path(), &rows, &env);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A launcher whose options the guard can read keeps an ordinary line
/// allowed: an option's operand is never read as an option, a launcher
/// that sets, unsets or clears variables is applied, and a directory
/// change is followed into a repository with no alias of that name.
#[test]
fn launcher_options_in_front_of_a_program_word_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let commands = [
        "exec -a -c git status",
        "exec -acname git status",
        "exec -a -c $TOOL build",
        "exec -c git status",
        "exec -c $TOOL build",
        "arch -e LANG=C git status",
        "arch -e LANG=C $TOOL build",
        "env -i $TOOL build",
        "env -u FOO $TOOL build",
        "env -C sub $TOOL build",
        "env -C sub $TOOL notes.md",
    ];
    let mut rows = Vec::new();
    for command in commands {
        rows.push(("exec-guard", command, None));
        rows.push(("git-guard", command, None));
    }
    let wrong = check(dir.path(), &rows);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Read as Git, a command whose global options Git rejects or only
/// answers runs no alias, so an ordinary line keeps its verdict even where
/// the repository has an alias of its first word. The configured program
/// is a string the guard reads; Git never runs here.
#[test]
fn unread_program_words_git_would_reject_run_no_alias() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "task/x");
    write_agent_policy(dir.path(), MEASURED_POLICY);
    git(dir.path(), &["config", "alias.inspect", "diff --ext-diff"]);
    git(dir.path(), &["config", "diff.external", "claude -p x"]);
    std::fs::write(dir.path().join("base.txt"), "base\nchanged\n").unwrap();
    let commands = [
        "$TOOL -c 'echo hi' inspect",
        "$TOOL -o out inspect",
        "$TOOL -h inspect",
        "$TOOL -c x.cfg build",
    ];
    let mut rows = Vec::new();
    for command in commands {
        rows.push(("exec-guard", command, None));
        rows.push(("git-guard", command, None));
    }
    // A setting Git takes keeps the alias reading.
    rows.push((
        "exec-guard",
        "$TOOL -c x.cfg inspect",
        Some("security.headless_peer_runs"),
    ));
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
/// shell fills in at run time is judged under the programs the guards
/// protect, and
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
