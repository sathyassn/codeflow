//! The command a `git config` write gives a command-valued key (TSK-1002
//! Fable review 15). Operator decision 2026-10-29: Git operand commands are
//! judged as commands. Every command a Git call runs itself (a `-c`
//! setting, `rebase -x`, `bisect run`, `submodule foreach`, filter-branch
//! filters, difftool and mergetool commands, send-email hooks, transport
//! programs and an `ext::` remote) is read by git-guard's runner reader
//! with the directory Git runs it in, and both guards judge each one. A
//! `git config` write runs nothing when it is made, so the runner reader
//! has no body for it; the value it stores for a key not known to run
//! nothing is judged here as a command in the call's directory.

use super::outward::Unwrapped;

/// The command a configuration setting gives Git: `Some(text)` for every
/// key [`super::git::config_kind`] does not know to run nothing, the same
/// classification git-guard applies to user and system settings. A `!`
/// value (an alias, a credential helper, a submodule update) is a shell
/// command after its `!`.
fn command_key(key: &str, value: &str) -> Option<String> {
    match super::git::config_kind(key, value) {
        super::git::ConfigKind::Safe => None,
        _ => Some(value.strip_prefix('!').unwrap_or(value).to_string()),
    }
}

/// The commands a `git config` write in `call` stores, in either syntax
/// (`git config core.pager X`, `git config set core.pager X`). A read, an
/// edit or a section change writes no value.
///
/// # Errors
///
/// A written key the guard cannot read.
pub(crate) fn commands(call: &Unwrapped) -> Result<Vec<String>, String> {
    const VALUED: &[&str] = &[
        "-f",
        "--file",
        "--blob",
        "--type",
        "--default",
        "--comment",
        "--value",
    ];
    const READS: &[&str] = &[
        "get",
        "list",
        "unset",
        "edit",
        "rename-section",
        "remove-section",
        "get-color",
        "get-colorbool",
    ];
    let Some((program, rest)) = call.args.split_first() else {
        return Ok(Vec::new());
    };
    let Some(git) = crate::hooks::git_guard::git_arguments(program, rest) else {
        return Ok(Vec::new());
    };
    let Some(("config", tail)) = crate::hooks::git_guard::git_subcommand(&git) else {
        return Ok(Vec::new());
    };
    let mut operands = Vec::new();
    let mut at = 0;
    while at < tail.len() {
        let word = tail[at].as_str();
        if VALUED.contains(&word) {
            at += 2;
            continue;
        }
        if word == "--" {
            operands.extend(tail[at + 1..].iter().cloned());
            break;
        }
        if word.starts_with('-') && word != "-" {
            let mode = word.split_once('=').map_or(word, |(mode, _)| mode);
            if mode.starts_with("--get")
                || mode.starts_with("--unset")
                || matches!(
                    mode,
                    "-l" | "--list" | "-e" | "--edit" | "--rename-section" | "--remove-section"
                )
            {
                return Ok(Vec::new());
            }
            at += 1;
            continue;
        }
        operands.push(tail[at].clone());
        at += 1;
    }
    let operands: &[String] = match operands.first().map(String::as_str) {
        Some("set") => &operands[1..],
        Some(first) if READS.contains(&first) => return Ok(Vec::new()),
        _ => &operands,
    };
    let (Some(key), Some(value)) = (operands.first(), operands.get(1)) else {
        return Ok(Vec::new());
    };
    if crate::hooks::git_guard::unresolved_word(key) {
        return Err(format!("cannot read the key `git config` writes: `{key}`"));
    }
    Ok(command_key(key, value)
        // Only the shell's own separators make a command empty.
        .filter(|text| {
            !text
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        })
        .into_iter()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(line: &str) -> Unwrapped {
        Unwrapped {
            args: crate::hooks::git_guard::command_argv(line),
            input: None,
            stdin_bound: false,
            unresolved: Vec::new(),
            supplier: None,
            expands: None,
            cwd: Some(std::env::temp_dir()),
            links: Vec::new(),
        }
    }

    fn texts(line: &str) -> Vec<String> {
        commands(&call(line)).unwrap_or_else(|why| panic!("{line}: {why}"))
    }

    #[test]
    fn a_config_write_of_a_command_key_is_a_command() {
        let one = |text: &str| vec![text.to_string()];
        assert_eq!(texts("git config core.pager 'cat a'"), one("cat a"));
        assert_eq!(
            texts("git config set --local core.editor 'cat a'"),
            one("cat a")
        );
        assert_eq!(
            texts("git config --add filter.x.smudge 'cat a'"),
            one("cat a")
        );
        assert_eq!(texts("git config 'alias.y' '!cat a'"), one("cat a"));
    }

    #[test]
    fn reads_and_plain_values_are_no_command() {
        for line in [
            "git config core.pager",
            "git config --get core.pager",
            "git config get core.pager",
            "git config --unset core.pager",
            "git config user.name 'A Person'",
            "git log",
        ] {
            assert_eq!(texts(line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn an_unread_written_key_refuses() {
        assert!(commands(&call("git config \"$K\" 'cat a'")).is_err());
    }
}
