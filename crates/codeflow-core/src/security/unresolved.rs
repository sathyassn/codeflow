//! The closed rule of TSK-242: on a line that names a protected target, a
//! form the guard cannot resolve refuses.
//!
//! Fifteen review rounds found new spellings that carry a command past a
//! text guard: another launcher, another option form, an expansion used as
//! the command. The set of programs that can run a command is open, so a
//! list of them never closes. This rule is its complement. Where a line
//! names a shell startup file (exec-guard) or a user or system git
//! configuration write (git-guard), every program on it must be one the
//! guard reads: a data reader used as one, git or gh, a writer whose
//! targets the guards judge and that runs nothing of its own, a shell or
//! launcher whose command the reader unwraps, or a builtin. Anything else
//! refuses, naming the form ([`unresolved_form`]). Each form the guard does
//! read is judged by what it writes, as before.
//!
//! The rule needs the target on the line. A spelling that names none (a
//! path built by a program, a value read from the environment, an include
//! file in the workspace) is a stated residual, held by the harness sandbox
//! where one runs.

use super::git::{config_kind, ConfigKind};
use super::startup::{mentions, reader_program};
use crate::hooks::git_guard::{
    basename, command_argv, expands, gnu_prefixed, is_shell, launcher_commands, launcher_name,
    shell_c_argument, strip_launchers, strip_reserved_words, xargs_command, Expanded,
    NESTING_UNREAD,
};

/// Shell builtins that run no command of their own and that the data
/// readers do not list.
const BUILTINS: &[&str] = &[
    "set", "wait", "read", "return", "exit", "break", "continue", "shopt", "setopt", "unsetopt",
    "umask", "hash", "jobs", "let", "getopts", "dirs", "[[", "]]",
];

/// Programs that write or remove the files they name, whose targets the
/// guards judge, and that run no command unless an option asks for one
/// ([`writer_runs_nothing`]).
const WRITERS: &[&str] = &[
    "cp",
    "mv",
    "ln",
    "install",
    "ditto",
    "tee",
    "dd",
    "truncate",
    "touch",
    "mkdir",
    "mktemp",
    "rm",
    "rmdir",
    "unlink",
    "chmod",
    "chown",
    "chgrp",
    "shred",
    "rsync",
    "scp",
    "tar",
    "bsdtar",
    "gtar",
    "unzip",
    "zip",
    "curl",
    "wget",
    "patch",
    "trash",
    "trash-put",
];

/// Words in a long option of a writer that make it run a program
/// (`--rsh`, `--use-compress-program`, `--to-command`, `--strip-program`,
/// `--checkpoint-action=exec=`).
const RUNNING_OPTIONS: &[&str] = &[
    "command",
    "cmd",
    "exec",
    "program",
    "script",
    "rsh",
    "askpass",
    "checkpoint",
];

/// The first form on the line the guard cannot resolve, as words for a
/// refusal; `None` when every command on it is read. `names_target` tells
/// whether a sourced file is the protected target itself, which the user's
/// own shell reads (`source ~/.zshrc`).
pub(crate) fn unresolved_form(
    expanded: &Expanded,
    names_target: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if let Some(word) = expanded.unread_commands.first() {
        return Some(format!(
            "`{}` runs as a command, an expansion the guard cannot read",
            shown(word)
        ));
    }
    expanded.segments.iter().find_map(|segment| {
        if segment == NESTING_UNREAD {
            return Some("it nests commands deeper than the guard reads".to_string());
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        segment_form(&words, names_target)
    })
}

/// The unresolved form of one simple command, or `None` when the guard
/// reads it.
fn segment_form(words: &[String], names_target: &dyn Fn(&str) -> bool) -> Option<String> {
    let launched = strip_launchers(words);
    let program_at = launched.map_or(words.len(), |(_, args)| words.len() - args.len() - 1);
    if let Some(prefixed) = words[..program_at].iter().find(|w| gnu_prefixed(w)) {
        return Some(format!(
            "`{prefixed}`, a GNU-prefixed launcher, runs the command after it"
        ));
    }
    // A search path set for the command picks which program its name runs.
    if let Some(path) = words[..program_at]
        .iter()
        .find(|w| w.starts_with("PATH=") || w.as_str() == "-P" || w.starts_with("--path"))
    {
        return Some(format!(
            "`{path}` picks which program the command's name runs"
        ));
    }
    // `env -S` and a bare assignment leave no program: the split string is
    // judged as its own commands.
    let (program, args) = launched?;
    // A command word that expands a name is read through its literal, or
    // listed among the unread commands.
    if expands(program) {
        return None;
    }
    let name = launcher_name(program).trim_end_matches(".exe");
    if is_shell(name) {
        if shell_c_argument(args).is_some() {
            return None;
        }
        let script = args.iter().find(|a| !a.starts_with(['-', '+']));
        return Some(match script {
            Some(file) => format!("`{name} {file}` runs a script the guard does not read"),
            None => format!("`{name}` reads the commands it runs from standard input"),
        });
    }
    match name {
        "eval" | "git" | "gh" => None,
        _ if BUILTINS.contains(&name) => None,
        "source" | "." => match args.first() {
            Some(file) if !expands(file) && names_target(file) => None,
            Some(file) => Some(format!(
                "`{name} {file}` runs a script the guard does not read"
            )),
            None => None,
        },
        "xargs" => xargs_command(args).and_then(|command| {
            (!command_reads(command)).then(|| {
                format!(
                    "`xargs` runs `{}` with arguments it reads from standard input",
                    command[0]
                )
            })
        }),
        "parallel" => Some(
            "`parallel` builds the commands it runs from a template and its inputs".to_string(),
        ),
        "find" => find_commands(args)
            .into_iter()
            .find(|command| !command_reads(command))
            .map(|command| {
                format!(
                    "`find -exec` runs `{}` on each path it finds",
                    command.first().map_or("", String::as_str)
                )
            }),
        "flock" | "script" | "watch" | "setsid" | "unbuffer" => launcher_commands(name, args)
            .is_empty()
            .then(|| format!("`{name}` runs a command the guard does not read")),
        _ if WRITERS.contains(&name) && writer_runs_nothing(name, args) => None,
        _ if reader_program(program, args) => None,
        _ => Some(format!(
            "`{program}` is a program the guard does not read, which can run a command of its own"
        )),
    }
}

/// Whether a command another program runs (`xargs`, `find -exec`) only
/// reads: a data reader used as one, under any launchers.
fn command_reads(command: &[String]) -> bool {
    strip_launchers(command).is_some_and(|(program, args)| {
        !expands(program) && !gnu_prefixed(program) && reader_program(program, args)
    })
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` and `-okdir`.
fn find_commands(args: &[String]) -> Vec<&[String]> {
    args.iter()
        .enumerate()
        .filter(|(_, word)| matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
        .map(|(at, _)| {
            let tail = &args[at + 1..];
            let end = tail
                .iter()
                .position(|w| w == ";" || w == "+")
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .collect()
}

/// Whether a writer runs no program of its own: no long option that names
/// one ([`RUNNING_OPTIONS`]), and none of rsync's `-e`, tar's `-I` or `-F`,
/// or scp's `-S`, `-o` or `-F`.
fn writer_runs_nothing(name: &str, args: &[String]) -> bool {
    let short = |letters: &[char]| {
        args.iter().any(|a| {
            a.strip_prefix('-')
                .filter(|c| !c.starts_with('-'))
                .is_some_and(|c| c.contains(letters))
        })
    };
    let long = args.iter().any(|a| {
        a.strip_prefix("--").is_some_and(|option| {
            let option = option.to_lowercase();
            RUNNING_OPTIONS.iter().any(|word| option.contains(word))
        })
    });
    !long
        && !match name {
            "rsync" => short(&['e']),
            "tar" | "bsdtar" | "gtar" => short(&['I', 'F']),
            "scp" => short(&['S', 'o', 'F']),
            _ => false,
        }
}

/// A word as a refusal shows it: a lifted substitution reads as `$(...)`.
fn shown(word: &str) -> String {
    word.replace(['\u{1}', '\u{2}'], "$(...)")
}

/// The user or system git configuration a line names, as words for a
/// refusal: a default path of the table (`~/.gitconfig`,
/// `~/.config/git/config`, `/etc/gitconfig` and the Homebrew and
/// `/usr/local` system files), a `gitconfig` in any `etc` directory, a
/// `git/config` below a configuration directory, a `GIT_CONFIG` variable,
/// or a `git config` write at `--global`, `--system`, `--file` or `--blob`
/// scope of a key not known to run nothing, or of a key the line does not
/// show. `None` when the line names none of them.
pub(crate) fn gitconfig_target(text: &str) -> Option<String> {
    let lower = text.replace('\\', "/").to_lowercase();
    let table = &super::actions::table().git_config_paths;
    let named = table
        .home
        .iter()
        .chain(&table.absolute)
        .map(|entry| entry.trim_end_matches('/').to_lowercase())
        .chain(["etc/gitconfig".to_string(), "git/config".to_string()])
        .find(|needle| mentions(&lower, needle));
    if let Some(needle) = named {
        return Some(format!("the git configuration file `{needle}`"));
    }
    if text.contains("GIT_CONFIG") {
        return Some("a `GIT_CONFIG` variable, which names the file git configures".to_string());
    }
    // Words as env, xargs or a shell may split them: at blanks, quotes,
    // operators, `=` and env's `\_`.
    let spaced = text.replace("\\_", " ");
    let words: Vec<&str> = spaced
        .split(|c: char| c.is_whitespace() || "'\";|&()<>`=".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    let has = |wanted: &[&str]| words.iter().any(|w| wanted.contains(w));
    if !(words.iter().any(|w| basename(w) == "git")
        && has(&["config"])
        && has(&["--global", "--system", "--file", "--blob"]))
    {
        return None;
    }
    let mut keys = words
        .iter()
        .enumerate()
        .filter(|(_, w)| key_shaped(w))
        .peekable();
    if keys.peek().is_none() {
        return Some(
            "a user or system `git config` write whose key the guard cannot see".to_string(),
        );
    }
    keys.find(|(at, key)| {
        let value = words.get(at + 1).copied().unwrap_or_default();
        config_kind(key, value) != ConfigKind::Safe
    })
    .map(|(_, key)| {
        format!("a user or system `git config` write of `{key}`, a key not known to run nothing")
    })
}

/// Whether `word` has the shape of a git configuration key:
/// `section.name` or `section.subsection.name`, with a section and a name
/// of letters, digits and `-` that start with a letter.
fn key_shaped(word: &str) -> bool {
    let Some((section, rest)) = word.split_once('.') else {
        return false;
    };
    let name = rest.rsplit('.').next().unwrap_or(rest);
    let plain = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    plain(section) && plain(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::git_guard::expand_commands_read;

    fn form(line: &str) -> Option<String> {
        unresolved_form(&expand_commands_read(line), &|w: &str| w.contains(".zshrc"))
    }

    #[test]
    fn every_form_the_guard_cannot_read_is_named() {
        for (line, expect) in [
            ("$CMD", "`$CMD` runs as a command"),
            ("eval \"$X\"", "`$X` runs as a command"),
            ("sh -c \"$CMD\"", "`$CMD` runs as a command"),
            ("${X:-sh} -c true", "runs as a command"),
            ("$(echo sh) -c true", "runs as a command"),
            ("gtimeout 5 true", "GNU-prefixed launcher"),
            ("gnice true", "GNU-prefixed launcher"),
            ("printf x | xargs git config", "`xargs` runs `git`"),
            ("xargs -I{} sh -c {}", "`xargs` runs `sh`"),
            ("parallel ' {1}' ::: x", "`parallel` builds"),
            ("find . -exec rm {} \\;", "`find -exec` runs `rm`"),
            ("tmux new-session -d true", "`tmux` is a program"),
            ("npm test", "`npm` is a program"),
            ("bash r.sh", "`bash r.sh` runs a script"),
            ("echo x | sh", "`sh` reads the commands"),
            ("source ./r.sh", "`source ./r.sh` runs a script"),
            ("rsync -e 'sh x' a b", "`rsync` is a program"),
            ("tar --to-command=sh -xf a.tar", "`tar` is a program"),
            ("script -q /dev/null", "`script` runs a command"),
            ("trap 'true' EXIT", "`trap` is a program"),
            ("PATH=/tmp/x cat ~/.zshrc", "`PATH=/tmp/x` picks"),
            ("env -P /tmp/x cat ~/.zshrc", "`-P` picks"),
        ] {
            let found = form(line);
            assert!(
                found.as_deref().is_some_and(|f| f.contains(expect)),
                "{line}: {found:?}"
            );
        }
    }

    #[test]
    fn forms_the_guard_reads_are_resolved() {
        for line in [
            // The narrow reading: an expansion outside the command word, in
            // a resolved read, is not an unresolved form (challenge finding 4).
            "grep \"$PAT\" ~/.zshrc",
            "sh -c 'grep \"$PAT\" ~/.zshrc'",
            "cat ~/.zshrc | sort",
            // A literal the line assigned before it is read as written.
            "CMD='git status'; $CMD",
            "CMD='git status'; sh -c \"$CMD\"",
            "CMD='cat x'; eval \"$CMD\"",
            "source ~/.zshrc",
            "git config --global user.name x",
            "cp ~/.zshrc backup.txt",
            "sed -i '' 's/a/b/' notes.txt",
            "find . -name x -exec grep -l y {} +",
            "printf '%s\\n' a | xargs ls",
            "xargs",
            "env -S 'git status'",
            "flock /tmp/l -c 'true'",
            "timeout 5 git status",
            "cd /tmp && ls",
            "export X=1; echo $X",
        ] {
            assert_eq!(form(line), None, "{line}");
        }
    }

    #[test]
    fn a_literal_assigned_after_its_use_stays_unread() {
        assert!(form("sh -c \"$CMD\"; CMD='git status'").is_some());
        assert!(form("CMD=a; CMD=b; $CMD").is_some());
        assert!(form("CMD=$OTHER; $CMD").is_some());
        // A prefix assignment lasts for its command only.
        assert!(form("CMD=true env; $CMD").is_some());
    }

    #[test]
    fn git_configuration_targets_are_named() {
        for line in [
            "cat ~/.gitconfig",
            "echo x >> \"$HOME/.config/git/config\"",
            "printf x > /opt/homebrew/etc/gitconfig",
            "printf x > /some/prefix/etc/gitconfig",
            "printf x > \"$XDG_CONFIG_HOME/git/config\"",
            "GIT_CONFIG_GLOBAL=/tmp/x git status",
            "env -S 'git\\_config\\_--global\\_alias.x\\_!id'",
            "printf '%s\\n' --global alias.x '!id' | xargs git config",
            "printf '%s\\n' --global | xargs git config",
            "tmux new-session -d 'git config --global core.pager x'",
            "git config --file=/tmp/x include.path y",
        ] {
            assert!(gitconfig_target(line).is_some(), "{line}");
        }
        for line in [
            "git status",
            "cat .git/config",
            "git config --global user.name x",
            "git config --global user.email a@b.c",
            "git config alias.x '!id'",
            "pip config --global set global.index-url x",
            "gtimeout 5 git status",
        ] {
            assert_eq!(gitconfig_target(line), None, "{line}");
        }
    }
}
