//! The programs that run a command given to them as an option value, and
//! the one reader both shell guards take those commands from (TSK-1490).
//!
//! git-guard reads each command as a nested body of the line
//! (`carried_commands`), so every check that walks the line's commands
//! judges it as written bare. exec-guard judges each one as a command of its
//! own (`option_violations`), so its raw-text checks see it too. A value in
//! a command language the guard does not read (an editor's commands, or a
//! Cargo key naming a program Cargo runs) refuses under `git.hook_integrity`
//! in both guards. Git's own option commands are the runner catalog's
//! (`hooks/git_guard/runners.rs`), and the shells, `env -S`, `eval`, `sed`,
//! `find -exec`, `xargs`, `parallel` and `watch` keep their own readers.
//!
//! The reader fails closed where a program's grammar is not modelled: it
//! scans every word up to `--` (option permutation), reads any prefix of a
//! long name for programs that accept abbreviations, and reads a value that
//! names a program path as shell text, so a value may be read as more than
//! the program runs, never less. A value in a file the program reads (a
//! configuration file, an environment variable) is outside this reader.
//!
//! The inventory, one line per row of [`OPTION_COMMANDS`], held equal to it
//! by a test (`security/launchers.rs`):
//!
//! - `ack`, `ag`: `--pager`
//! - `man`: `-P`, `--pager`
//! - `man`: `-H`, `--html`
//! - `rg`: `--pre`, `--hostname-bin`
//! - `ssh`, `scp`, `sftp`: `-o`
//! - `scp`, `sftp`: `-S`
//! - `sftp`: `-D`
//! - `rsync`: `-e`, `--rsh`
//! - `tar`, `gtar`, `bsdtar`: `-I`, `-F`, `--use-compress-program`, `--to-command`, `--info-script`, `--new-volume-script`, `--rsh-command`
//! - `tar`, `gtar`, `bsdtar`: `--checkpoint-action`
//! - `sort`: `--compress-program`
//! - `split`: `--filter`
//! - `install`: `--strip-program`
//! - `zip`: `-TT`, `--unzip-command`
//! - `flock`, `script`: `-c`, `--command`
//! - `npx`: `-c`, `--call`
//! - `npm exec`, `npm x`: `-c`, `--call`
//! - `su`, `runuser`: `-c`, `--command`, `--session-command`
//! - `fd`, `fdfind`: `-x`, `-X`, `--exec`, `--exec-batch`
//! - `gh repo clone`, `gh repo fork`, `gh gist clone`: `--`
//! - `vim`, `vi`, `nvim`, `view`, `ex`, `vimdiff`, `gvim`, `mvim`, `rvim`, `rview`, `gview`, `evim`, `vimx`, `nview`, `nvi`: `-c`, `--cmd`, `+`
//! - `less`: `+`
//! - `emacs`: `--eval`, `--execute`, `--funcall`, `-eval`, `-execute`, `-funcall`, `-f`
//! - `emacsclient`: `-e`, `--eval`
//! - `cargo`: `--config`

use crate::hooks::git_guard::{launcher_commands, program_name};

/// How a program runs an option's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Runs {
    /// A command line, or a program path read as one.
    Line,
    /// Command words, up to a `;` word or the end of the arguments.
    Words,
    /// An ssh option whose key names a command (`KEY=VALUE`, `KEY VALUE`).
    SshOption,
    /// A tar checkpoint action; `exec=COMMAND` runs a command line.
    Checkpoint,
    /// Every word after `--`, passed to `git clone`.
    CloneArguments,
    /// A command language the guard does not read: refused.
    Language,
    /// A Cargo configuration value: a key naming a program refuses.
    CargoConfig,
    /// Read by the program's own reader in git-guard
    /// ([`launcher_commands`]), which also reads its operand commands.
    OwnReader,
}

/// One row of the inventory: programs and the options whose value runs.
#[derive(Debug)]
pub(crate) struct Carrier {
    /// Program names, as [`program_name`] gives them.
    pub(crate) programs: &'static [&'static str],
    /// Words one of which must open the operands (`repo clone`); empty for
    /// none.
    pub(crate) subcommands: &'static [&'static [&'static str]],
    /// Short option letters, read in clusters (`-vo VALUE`, `-oVALUE`).
    pub(crate) short: &'static [char],
    /// Long names with their dashes (`--pager`, `-TT`, `-eval`). A name
    /// with one dash is matched whole, with its value attached by `=` or
    /// in the next word.
    pub(crate) long: &'static [&'static str],
    /// Whether any prefix of a `--` name names it (`getopt_long`, Perl's
    /// `Getopt::Long`); over-reading a prefix fails closed.
    pub(crate) prefixes: bool,
    /// Real options of the program that are prefixes of a name above, which
    /// an exact match makes the program's own (`install --strip`).
    pub(crate) shadows: &'static [&'static str],
    /// Whether the value must be attached (`-Hbrowser`, `--html=browser`).
    pub(crate) attached: bool,
    /// Whether a `+VALUE` word is the value (`vim +CMD`, `less +CMD`).
    pub(crate) plus: bool,
    /// How the program runs the value.
    pub(crate) runs: Runs,
}

const NONE: &[&str] = &[];

const fn row(programs: &'static [&'static str], runs: Runs) -> Carrier {
    Carrier {
        programs,
        subcommands: &[],
        short: &[],
        long: NONE,
        prefixes: false,
        shadows: NONE,
        attached: false,
        plus: false,
        runs,
    }
}

const TAR: &[&str] = &["tar", "gtar", "bsdtar"];
const SSH: &[&str] = &["ssh", "scp", "sftp"];
const VIM: &[&str] = &[
    "vim", "vi", "nvim", "view", "ex", "vimdiff", "gvim", "mvim", "rvim", "rview", "gview", "evim",
    "vimx", "nview", "nvi",
];

/// The inventory: every program and option the guards read a command from.
/// `docs/policy-reference.md` describes the class; tests hold the module's
/// list equal to it and every row reached by both guards.
pub(crate) const OPTION_COMMANDS: &[Carrier] = &[
    Carrier {
        long: &["--pager"],
        prefixes: true,
        ..row(&["ack", "ag"], Runs::Line)
    },
    Carrier {
        short: &['P'],
        long: &["--pager"],
        prefixes: true,
        ..row(&["man"], Runs::Line)
    },
    Carrier {
        short: &['H'],
        long: &["--html"],
        prefixes: true,
        attached: true,
        ..row(&["man"], Runs::Line)
    },
    Carrier {
        long: &["--pre", "--hostname-bin"],
        ..row(&["rg"], Runs::Line)
    },
    Carrier {
        short: &['o'],
        ..row(SSH, Runs::SshOption)
    },
    Carrier {
        short: &['S'],
        ..row(&["scp", "sftp"], Runs::Line)
    },
    Carrier {
        short: &['D'],
        ..row(&["sftp"], Runs::Line)
    },
    Carrier {
        short: &['e'],
        long: &["--rsh"],
        prefixes: true,
        ..row(&["rsync"], Runs::Line)
    },
    Carrier {
        short: &['I', 'F'],
        long: &[
            "--use-compress-program",
            "--to-command",
            "--info-script",
            "--new-volume-script",
            "--rsh-command",
        ],
        prefixes: true,
        ..row(TAR, Runs::Line)
    },
    Carrier {
        long: &["--checkpoint-action"],
        prefixes: true,
        shadows: &["--checkpoint"],
        ..row(TAR, Runs::Checkpoint)
    },
    Carrier {
        long: &["--compress-program"],
        prefixes: true,
        ..row(&["sort"], Runs::Line)
    },
    Carrier {
        long: &["--filter"],
        prefixes: true,
        ..row(&["split"], Runs::Line)
    },
    Carrier {
        long: &["--strip-program"],
        prefixes: true,
        shadows: &["--strip"],
        ..row(&["install"], Runs::Line)
    },
    Carrier {
        long: &["-TT", "--unzip-command"],
        prefixes: true,
        ..row(&["zip"], Runs::Line)
    },
    Carrier {
        short: &['c'],
        long: &["--command"],
        ..row(&["flock", "script"], Runs::OwnReader)
    },
    Carrier {
        short: &['c'],
        long: &["--call"],
        prefixes: true,
        ..row(&["npx"], Runs::Line)
    },
    Carrier {
        subcommands: &[&["exec"], &["x"]],
        short: &['c'],
        long: &["--call"],
        prefixes: true,
        ..row(&["npm"], Runs::Line)
    },
    Carrier {
        short: &['c'],
        long: &["--command", "--session-command"],
        prefixes: true,
        ..row(&["su", "runuser"], Runs::Line)
    },
    Carrier {
        short: &['x', 'X'],
        long: &["--exec", "--exec-batch"],
        ..row(&["fd", "fdfind"], Runs::Words)
    },
    Carrier {
        subcommands: &[&["repo", "clone"], &["repo", "fork"], &["gist", "clone"]],
        ..row(&["gh"], Runs::CloneArguments)
    },
    Carrier {
        short: &['c'],
        long: &["--cmd"],
        plus: true,
        ..row(VIM, Runs::Language)
    },
    Carrier {
        plus: true,
        ..row(&["less"], Runs::Language)
    },
    Carrier {
        long: &[
            "--eval",
            "--execute",
            "--funcall",
            "-eval",
            "-execute",
            "-funcall",
            "-f",
        ],
        prefixes: true,
        ..row(&["emacs"], Runs::Language)
    },
    Carrier {
        short: &['e'],
        long: &["--eval"],
        prefixes: true,
        ..row(&["emacsclient"], Runs::Language)
    },
    Carrier {
        long: &["--config"],
        ..row(&["cargo"], Runs::CargoConfig)
    },
];

/// What one option value carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Carried {
    /// A command line, judged as the command written bare.
    Command(String),
    /// A value the guard cannot read, with the program and option that take
    /// it (`vim -c`), as plain words for the refusal.
    Unread(String),
}

/// The refusal message for an [`Carried::Unread`] form.
pub(crate) fn unread_message(form: &str) -> String {
    format!(
        "`{form}` takes a value that runs commands in a language the guard does not read, so it is not judged as a command"
    )
}

/// The refusal both guards give an [`Carried::Unread`] form, under
/// `git.hook_integrity` at `level` (TSK-1490 rulings D2 and D3).
pub(crate) fn unread_violation(
    form: &str,
    level: crate::hooks::policy::PolicyLevel,
) -> crate::hooks::Violation {
    crate::hooks::Violation::new(
        "git.hook_integrity",
        level,
        unread_message(form),
        crate::remedy::OPTION_LANGUAGE.remedy(),
    )
}

/// The first value on the simple command `words` the guard cannot read.
pub(crate) fn first_unread(words: &[String]) -> Option<String> {
    let (program, args) = crate::hooks::git_guard::strip_launchers(words)?;
    option_values(program, args)
        .into_iter()
        .find_map(|carried| match carried {
            Carried::Unread(form) => Some(form),
            Carried::Command(_) => None,
        })
}

/// Every command `program` runs from an option value in `args`, by the
/// inventory, rows read by their own reader included.
pub(crate) fn option_commands(program: &str, args: &[String]) -> Vec<Carried> {
    read(program, args, true)
}

/// [`option_commands`] without the rows git-guard's own launcher reader
/// reads ([`Runs::OwnReader`]), for that reader's caller.
pub(crate) fn option_values(program: &str, args: &[String]) -> Vec<Carried> {
    read(program, args, false)
}

fn read(program: &str, args: &[String], own: bool) -> Vec<Carried> {
    let name = program_name(program);
    let mut out = Vec::new();
    for carrier in OPTION_COMMANDS
        .iter()
        .filter(|carrier| carrier.programs.contains(&name))
    {
        if carrier.runs == Runs::OwnReader {
            if own {
                out.extend(
                    launcher_commands(name, args)
                        .into_iter()
                        .map(Carried::Command),
                );
            }
            continue;
        }
        if !carrier.subcommands.is_empty() && !opens_with_subcommand(carrier, args) {
            continue;
        }
        for carried in carrier_values(carrier, name, args) {
            if !out.contains(&carried) {
                out.push(carried);
            }
        }
    }
    out
}

/// Whether the operands open with one of the row's subcommands.
fn opens_with_subcommand(carrier: &Carrier, args: &[String]) -> bool {
    let operands: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|word| !word.starts_with('-'))
        .collect();
    carrier
        .subcommands
        .iter()
        .any(|words| operands.len() >= words.len() && operands[..words.len()] == words[..])
}

/// The values of one row's options in `args`, read by how the row runs.
fn carrier_values(carrier: &Carrier, name: &str, args: &[String]) -> Vec<Carried> {
    if carrier.runs == Runs::CloneArguments {
        // `git clone` with the words after `--`, then the repository and
        // directory operands the subcommand names before it.
        let Some(end) = args.iter().position(|word| word == "--") else {
            return Vec::new();
        };
        let flags = &args[end + 1..];
        if flags.is_empty() {
            return Vec::new();
        }
        let operands = args[..end]
            .iter()
            .filter(|word| !word.starts_with('-'))
            .skip(2);
        let mut line = "git clone".to_string();
        for word in flags.iter().chain(operands) {
            line.push(' ');
            line.push_str(&quoted(word));
        }
        return vec![Carried::Command(line)];
    }
    let mut out = Vec::new();
    let mut at = 0;
    if TAR.contains(&name) {
        at = old_tar_values(carrier, args, &mut out);
    }
    while let Some(word) = args.get(at).map(String::as_str) {
        at += 1;
        if word == "--" {
            break;
        }
        match option_value(carrier, word) {
            Some(Value::Attached(value)) => {
                at = carried_value(carrier, name, word, value, args, at, &mut out);
            }
            // The value is the next word, which the option consumes; with
            // no next word nothing runs.
            Some(Value::Next) => {
                if let Some(value) = args.get(at).cloned() {
                    at = carried_value(carrier, name, word, value, args, at + 1, &mut out);
                }
            }
            None => {}
        }
    }
    out
}

/// Where an option word's value is.
enum Value {
    /// In the word itself (`-oX`, `--pre=X`, `+X`).
    Attached(String),
    /// In the next word.
    Next,
}

/// The value of one of the row's options in `word`, or `None` when `word`
/// is not one of them or the option takes only an attached value it lacks.
fn option_value(carrier: &Carrier, word: &str) -> Option<Value> {
    if carrier.plus && word.starts_with('+') {
        return Some(Value::Attached(word.trim_start_matches('+').to_string()));
    }
    let next = || (!carrier.attached).then_some(Value::Next);
    for long in carrier.long {
        if long_match(carrier, word, long) {
            return match word.split_once('=') {
                Some((_, value)) => Some(Value::Attached(value.to_string())),
                None => next(),
            };
        }
    }
    let cluster = word
        .strip_prefix('-')
        .filter(|c| !c.is_empty() && !c.starts_with('-'))?;
    let (offset, letter) = cluster
        .char_indices()
        .find(|(_, letter)| carrier.short.contains(letter))?;
    let attached = &cluster[offset + letter.len_utf8()..];
    if attached.is_empty() {
        next()
    } else {
        Some(Value::Attached(attached.to_string()))
    }
}

/// Whether `word` names the long option `long`: the whole name, a one-dash
/// name with `=VALUE`, or for `--` names a prefix where the row reads one
/// and no real option of the program shadows it.
fn long_match(carrier: &Carrier, word: &str, long: &str) -> bool {
    let name = word.split_once('=').map_or(word, |(name, _)| name);
    if name == long {
        return true;
    }
    if long.starts_with("--") && carrier.prefixes && name.starts_with("--") && name.len() > 2 {
        return long.starts_with(name) && !carrier.shadows.contains(&name);
    }
    false
}

/// Push what `value` carries by the row's kind; returns where reading goes
/// on (after the words a [`Runs::Words`] value takes).
fn carried_value(
    carrier: &Carrier,
    name: &str,
    word: &str,
    value: String,
    args: &[String],
    at: usize,
    out: &mut Vec<Carried>,
) -> usize {
    let mut push = |carried: Carried| {
        if !out.contains(&carried) {
            out.push(carried);
        }
    };
    match carrier.runs {
        Runs::Line => push(Carried::Command(value)),
        Runs::Words => {
            let tail = &args[at..];
            let end = tail.iter().position(|w| w == ";").unwrap_or(tail.len());
            let mut line = quoted(&value);
            for word in &tail[..end] {
                line.push(' ');
                line.push_str(&quoted(word));
            }
            push(Carried::Command(line));
            return at + end;
        }
        Runs::SshOption => {
            if let Some(command) = ssh_command(&value) {
                push(Carried::Command(command));
            }
        }
        Runs::Checkpoint => {
            if let Some(command) = value.strip_prefix("exec=") {
                push(Carried::Command(command.to_string()));
            }
        }
        Runs::Language => {
            if !inert_position(name, word, &value) {
                push(Carried::Unread(format!("{name} {}", shown_option(word))));
            }
        }
        Runs::CargoConfig => {
            if names_cargo_program(&value) {
                push(Carried::Unread(format!("{name} --config")));
            }
        }
        Runs::CloneArguments | Runs::OwnReader => {}
    }
    at
}

/// GNU tar's old-style first word (`tar cIf PROG x.tar d`): each letter
/// that takes a value takes the next word in turn. Returns where the
/// option scan starts.
fn old_tar_values(carrier: &Carrier, args: &[String], out: &mut Vec<Carried>) -> usize {
    const VALUED: &[char] = &[
        'b', 'C', 'f', 'F', 'g', 'H', 'I', 'K', 'L', 'N', 'T', 'V', 'X',
    ];
    let Some(first) = args.first().filter(|word| !word.starts_with('-')) else {
        return 0;
    };
    let mut next = 1;
    for letter in first.chars() {
        if !VALUED.contains(&letter) {
            continue;
        }
        if carrier.short.contains(&letter) {
            if let Some(value) = args.get(next) {
                let body = Carried::Command(value.clone());
                if !out.contains(&body) {
                    out.push(body);
                }
            }
        }
        next += 1;
    }
    next.min(args.len())
}

/// The command an ssh `-o` value runs: its key, matched without case,
/// separated from the value by `=` or blanks, naming a command.
fn ssh_command(value: &str) -> Option<String> {
    const KEYS: &[&str] = &["proxycommand", "localcommand", "knownhostscommand"];
    let value = value.trim_start();
    let end = value
        .find(|c: char| c == '=' || c.is_whitespace())
        .unwrap_or(value.len());
    let key = value[..end].to_ascii_lowercase();
    if !KEYS.contains(&key.as_str()) {
        return None;
    }
    let rest = value[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    (!rest.is_empty() && !rest.eq_ignore_ascii_case("none")).then(|| rest.to_string())
}

/// The editor values that only move the cursor: `+` alone, a line number,
/// and for `less` the end, follow and start commands.
fn inert_position(name: &str, word: &str, value: &str) -> bool {
    if !word.starts_with('+') {
        return false;
    }
    value.is_empty()
        || value.chars().all(|c| c.is_ascii_digit())
        || (name == "less" && matches!(value, "G" | "g" | "F"))
}

/// Whether a Cargo `--config` value sets a key naming a program Cargo runs
/// (TSK-1490 ruling D3): a runner, a compiler wrapper, a linker, the
/// compiler, rustdoc, a browser or a credential provider. Any word of the
/// value counts, so a key in an inline table or a quoted segment is found;
/// a value naming no such key, a configuration file included, passes.
fn names_cargo_program(value: &str) -> bool {
    const KEYS: &[&str] = &[
        "runner",
        "linker",
        "rustc-wrapper",
        "rustc-workspace-wrapper",
        "rustc",
        "rustdoc",
        "browser",
        "credential-provider",
        "global-credential-providers",
    ];
    value.contains('=')
        && value
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .any(|word| KEYS.contains(&word))
}

/// The option as the refusal shows it: its name without a value.
fn shown_option(word: &str) -> String {
    if word.starts_with('+') {
        return "+".to_string();
    }
    let name = word.split_once('=').map_or(word, |(name, _)| name);
    if name.starts_with("--") {
        return name.to_string();
    }
    name.chars().take(2).collect()
}

/// A word as shell text that reads back as the same word. Double quotes
/// keep a `$` or a backtick live, so a later check still sees an expansion
/// the word may hold, as git-guard's own launcher reader does.
fn quoted(word: &str) -> String {
    format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    fn commands(line: &str) -> Vec<Carried> {
        let all = words(line);
        option_commands(&all[0], &all[1..])
    }

    #[test]
    fn option_spellings_read_one_value() {
        let x = || vec![Carried::Command("X".into())];
        for line in [
            "ack --pager=X",
            "ack --pag X",
            "man -PX",
            "man -aP X",
            "rsync -avzeX",
            "tar -cf a -I X",
            "tar --use=X",
            "zip -TT X",
            "zip --unzip-command=X",
            "sort --compress-prog=X",
            "su -lc X",
            "ssh -o ProxyCommand=X",
            "ssh -oLocalCommand=X",
            "tar --checkpoint-action=exec=X",
            "tar cIf X a.tar",
            "npm exec -c X",
            "/usr/bin/gsort --compress-program X",
        ] {
            assert_eq!(commands(line), x(), "{line}");
        }
        for line in [
            "ack -- --pager=X",
            "install --strip X",
            "tar --checkpoint X",
            "man -H",
            "man -H X",
            "npm install -c X",
            "ssh -o BatchMode=yes",
            "ssh -o ProxyCommand=none",
            "cargo --config build.jobs=2",
            "vim +12",
            "less +G",
        ] {
            assert!(commands(line).is_empty(), "{line}: {:?}", commands(line));
        }
        assert_eq!(
            commands("fd -x rm -f ; -e rs"),
            vec![Carried::Command("\"rm\" \"-f\"".into())]
        );
        assert_eq!(
            commands("gh repo clone o/r -- -u x"),
            vec![Carried::Command("git clone \"-u\" \"x\" \"o/r\"".into())]
        );
        for line in [
            "vim -c x",
            "less +!id",
            "emacs --ev x",
            "cargo --config target.x.runner=y",
        ] {
            assert!(
                matches!(commands(line).as_slice(), [Carried::Unread(_)]),
                "{line}"
            );
        }
    }
}
