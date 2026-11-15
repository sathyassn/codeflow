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
//! The reader admits only the spellings it parses exactly: a whole option
//! name with its value after `=`, glued on where the program's grammar
//! allows it, or in the next word, and short options alone or in clusters.
//! Any other spelling of an inventory option refuses by name: an
//! abbreviation or a case variant of a `--` name, or a glued name inside a
//! cluster. It scans every word up to `--` (option permutation) and reads a
//! value that names a program path as shell text, so a value may be read as
//! more than the program runs, never less. A Cargo `--config` value is
//! parsed as TOML and judged by its decoded key paths. A value in a file the
//! program reads (a configuration file, an environment variable) is outside
//! this reader.
//!
//! A program may run the carried command in another directory (an option
//! of [`RUN_DIRECTORIES`], or `find -execdir`). A literal directory the
//! guard can read moves the judgment there as well as where the line
//! starts; any other directory refuses under `git.hook_integrity`.
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
//! - `npm`: `-c`, `--call`
//! - `su`, `runuser`: `-c`, `--command`, `--session-command`
//! - `fd`, `fdfind`: `-x`, `-X`, `--exec`, `--exec-batch`
//! - `gh repo clone`, `gh repo fork`, `gh gist clone`: `--`
//! - `vim`, `vi`, `nvim`, `view`, `ex`, `vimdiff`, `gvim`, `mvim`, `rvim`, `rview`, `gview`, `evim`, `vimx`, `nview`, `nvi`: `-c`, `--cmd`, `+`
//! - `less`: `+`
//! - `emacs`: `--eval`, `--execute`, `--funcall`, `-eval`, `-execute`, `-funcall`, `-f`
//! - `emacsclient`: `-e`, `--eval`
//! - `cargo`: `--config`
//! - `wget`, `wget2`: `--use-askpass`
//! - `wget`, `wget2`: `-e`, `--execute`

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
    /// A Cargo configuration value, judged by its decoded key paths: a key
    /// naming a program refuses, an `env` key is judged as the assignment.
    CargoConfig,
    /// A wget startup command (`NAME = VALUE`): the askpass command's value
    /// is the program wget runs.
    Wgetrc,
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
    /// Long names with their dashes (`--pager`, `-TT`, `-eval`), matched
    /// whole, with the value after `=` or in the next word. Another
    /// spelling of a `--` name, an abbreviation or a case variant, refuses.
    pub(crate) long: &'static [&'static str],
    /// Real options of the program that are prefixes of a name above, which
    /// an exact match makes the program's own (`install --strip`).
    pub(crate) shadows: &'static [&'static str],
    /// Whether the value must be attached (`-Hbrowser`, `--html=browser`).
    pub(crate) attached: bool,
    /// Whether a one-dash long name takes its value glued on (`-TTcmd`);
    /// the name glued inside a cluster refuses.
    pub(crate) glued: bool,
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
        shadows: NONE,
        attached: false,
        glued: false,
        plus: false,
        runs,
    }
}

const TAR: &[&str] = &["tar", "gtar", "bsdtar"];
const SSH: &[&str] = &["ssh", "scp", "sftp"];
const WGET: &[&str] = &["wget", "wget2"];
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
        ..row(&["ack", "ag"], Runs::Line)
    },
    Carrier {
        short: &['P'],
        long: &["--pager"],
        ..row(&["man"], Runs::Line)
    },
    Carrier {
        short: &['H'],
        long: &["--html"],
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
        ..row(TAR, Runs::Line)
    },
    Carrier {
        long: &["--checkpoint-action"],
        shadows: &["--checkpoint"],
        ..row(TAR, Runs::Checkpoint)
    },
    Carrier {
        long: &["--compress-program"],
        ..row(&["sort"], Runs::Line)
    },
    Carrier {
        long: &["--filter"],
        ..row(&["split"], Runs::Line)
    },
    Carrier {
        long: &["--strip-program"],
        shadows: &["--strip"],
        ..row(&["install"], Runs::Line)
    },
    Carrier {
        long: &["-TT", "--unzip-command"],
        glued: true,
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
        ..row(&["npx"], Runs::Line)
    },
    // npm takes `-c` as `--call` before or after any subcommand, so the
    // value is read wherever it stands.
    Carrier {
        short: &['c'],
        long: &["--call"],
        ..row(&["npm"], Runs::Line)
    },
    Carrier {
        short: &['c'],
        long: &["--command", "--session-command"],
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
        ..row(&["emacs"], Runs::Language)
    },
    Carrier {
        short: &['e'],
        long: &["--eval"],
        ..row(&["emacsclient"], Runs::Language)
    },
    Carrier {
        long: &["--config"],
        ..row(&["cargo"], Runs::CargoConfig)
    },
    Carrier {
        long: &["--use-askpass"],
        ..row(WGET, Runs::Line)
    },
    Carrier {
        short: &['e'],
        long: &["--execute"],
        ..row(WGET, Runs::Wgetrc)
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
    /// A startup setting whose value sets the startup-command setting
    /// again, with the program and option (`wget -e`). The program the
    /// innermost setting names is not read, so the value refuses.
    Nested(String),
    /// A carried command the program runs in a directory the guard cannot
    /// establish, with the program and option that move it (`su -l`).
    Elsewhere(String),
}

impl Carried {
    /// The refusal message for a value the guard does not judge as a
    /// command; `None` for a [`Carried::Command`].
    pub(crate) fn refusal(&self) -> Option<String> {
        match self {
            Self::Command(_) => None,
            Self::Unread(form) => Some(unread_message(form)),
            Self::Nested(form) => Some(format!(
                "`{form}` sets a startup setting whose value sets the startup-command setting again, so the program it finally runs is not read and cannot be judged"
            )),
            Self::Elsewhere(form) => Some(format!(
                "`{form}` runs the command it carries in a directory the guard cannot establish, so the command cannot be judged with that directory's configuration"
            )),
        }
    }

    /// The marker segment git-guard's command reader stands in for this
    /// refusal ([`refusal_marker`] reads it back); `None` for a
    /// [`Carried::Command`].
    pub(crate) fn marker(&self) -> Option<String> {
        match self {
            Self::Command(_) => None,
            Self::Unread(form) => Some(format!("{OPTION_UNREAD}{form}")),
            Self::Nested(form) => Some(format!("{OPTION_NESTED}{form}")),
            Self::Elsewhere(form) => Some(format!("{OPTION_ELSEWHERE}{form}")),
        }
    }
}

/// The start of the segment that stands for an option value the guard does
/// not read as a command; the program and option follow it as plain words
/// (TSK-1490). No shell runs it and the integrity check refuses it.
pub(crate) const OPTION_UNREAD: &str = "\u{1}codeflow: unread option value: ";

/// The start of the segment that stands for a startup setting nested in
/// another ([`Carried::Nested`]), read like [`OPTION_UNREAD`].
pub(crate) const OPTION_NESTED: &str = "\u{1}codeflow: nested startup setting: ";

/// The start of the segment that stands for a carried command run in a
/// directory the guard cannot establish ([`Carried::Elsewhere`]), read like
/// [`OPTION_UNREAD`].
pub(crate) const OPTION_ELSEWHERE: &str = "\u{1}codeflow: carried command elsewhere: ";

/// The refusal a marker segment from [`Carried::marker`] stands for.
pub(crate) fn refusal_marker(segment: &str) -> Option<Carried> {
    if let Some(form) = segment.strip_prefix(OPTION_UNREAD) {
        return Some(Carried::Unread(form.to_string()));
    }
    if let Some(form) = segment.strip_prefix(OPTION_ELSEWHERE) {
        return Some(Carried::Elsewhere(form.to_string()));
    }
    segment
        .strip_prefix(OPTION_NESTED)
        .map(|form| Carried::Nested(form.to_string()))
}

/// The refusal message for an [`Carried::Unread`] form.
pub(crate) fn unread_message(form: &str) -> String {
    format!(
        "`{form}` takes a value the guard does not read as a command: a command language, a setting that names a program, or a spelling of the option the guard does not parse, so it cannot be judged"
    )
}

/// The refusal both guards give a value they do not judge as a command,
/// under `git.hook_integrity` at `level` (TSK-1490 rulings D2 and D3);
/// `None` for a [`Carried::Command`].
pub(crate) fn unread_violation(
    carried: &Carried,
    level: crate::hooks::policy::PolicyLevel,
) -> Option<crate::hooks::Violation> {
    Some(crate::hooks::Violation::new(
        "git.hook_integrity",
        level,
        carried.refusal()?,
        crate::remedy::OPTION_LANGUAGE.remedy(),
    ))
}

/// The first value on the simple command `words` the guard does not judge
/// as a command.
pub(crate) fn first_unread(words: &[String]) -> Option<Carried> {
    let (program, args) = crate::hooks::git_guard::strip_launchers(words)?;
    option_values(program, args)
        .into_iter()
        .find(|carried| !matches!(carried, Carried::Command(_)))
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
    let rows: Vec<&Carrier> = OPTION_COMMANDS
        .iter()
        .filter(|carrier| carrier.programs.contains(&name))
        .collect();
    let mut out = unparsed_spellings(&rows, name, args);
    if let Some(option) = crate::hooks::git_guard::found_path_directory(name, args) {
        out.push(Carried::Elsewhere(format!("{name} {option}")));
    }
    // The commands git-guard's own launcher reader finds, for its rows and
    // for a program whose options move where it runs them (`parallel`).
    let own_reader = rows.iter().any(|carrier| carrier.runs == Runs::OwnReader);
    let moves = RUN_DIRECTORIES
        .iter()
        .any(|row| row.programs.contains(&name));
    let bodies = if own_reader || moves {
        launcher_commands(name, args)
    } else {
        Vec::new()
    };
    let launched = !bodies.is_empty();
    if own && own_reader {
        out.extend(bodies.into_iter().map(Carried::Command));
    }
    for carrier in rows {
        if carrier.runs == Runs::OwnReader {
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
    let carries = launched || out.iter().any(|c| matches!(c, Carried::Command(_)));
    if !carries {
        return out;
    }
    match run_directories(name, args) {
        Ok(dirs) => {
            let mut placed = Vec::new();
            for carried in out {
                if let Carried::Command(body) = &carried {
                    for body in relocated(&dirs, body) {
                        let command = Carried::Command(body);
                        if !placed.contains(&command) {
                            placed.push(command);
                        }
                    }
                } else if !placed.contains(&carried) {
                    placed.push(carried);
                }
            }
            placed
        }
        Err(form) => {
            let mut kept: Vec<Carried> = out
                .into_iter()
                .filter(|c| !matches!(c, Carried::Command(_)))
                .collect();
            let away = Carried::Elsewhere(form);
            if !kept.contains(&away) {
                kept.push(away);
            }
            kept
        }
    }
}

/// The options that make a program run the command it carries in another
/// directory (TSK-1490 AC-7). A directory value that is a literal path is
/// read in order, each relative to the last; the command is judged in the
/// starting directory and after each change, a superset of where it runs.
/// A login, a workspace, a value filled in at run time or a spelling the
/// reader does not parse leaves the directory unknown, and the command
/// refuses ([`Carried::Elsewhere`]).
struct RunDirectory {
    programs: &'static [&'static str],
    /// Options whose value is the directory.
    short: &'static [char],
    long: &'static [&'static str],
    /// Options and words that move the command where the guard cannot
    /// follow: a login's home, a workspace.
    away_short: &'static [char],
    away_long: &'static [&'static str],
    away_words: &'static [&'static str],
}

const RUN_DIRECTORIES: &[RunDirectory] = &[
    RunDirectory {
        programs: &["fd", "fdfind"],
        short: &['C'],
        long: &["--base-directory"],
        away_short: &[],
        away_long: &[],
        away_words: &[],
    },
    RunDirectory {
        programs: TAR,
        short: &['C'],
        long: &["--directory"],
        away_short: &[],
        away_long: &[],
        away_words: &[],
    },
    RunDirectory {
        programs: &["npm", "npx"],
        short: &['C'],
        long: &["--prefix"],
        away_short: &['w'],
        away_long: &["--workspace", "--workspaces"],
        away_words: &["-ws"],
    },
    RunDirectory {
        programs: &["su", "runuser"],
        short: &[],
        long: &[],
        away_short: &['l'],
        away_long: &["--login"],
        away_words: &["-"],
    },
    RunDirectory {
        programs: &["parallel"],
        short: &[],
        long: &["--wd", "--workdir"],
        away_short: &[],
        away_long: &[],
        away_words: &[],
    },
];

/// The literal directories `name`'s options move a carried command to, in
/// order, or the program and option that move it where the guard cannot
/// establish.
fn run_directories(name: &str, args: &[String]) -> Result<Vec<String>, String> {
    // A letter that takes the carried command as its value ends a cluster.
    let valued: Vec<char> = OPTION_COMMANDS
        .iter()
        .filter(|carrier| carrier.programs.contains(&name))
        .flat_map(|carrier| carrier.short.iter().copied())
        .collect();
    let mut dirs = Vec::new();
    for row in RUN_DIRECTORIES
        .iter()
        .filter(|row| row.programs.contains(&name))
    {
        let directory = |value: Option<&String>, option: &str| -> Result<String, String> {
            value
                .map(String::as_str)
                .filter(|value| literal_directory(value))
                .map(str::to_string)
                .ok_or_else(|| format!("{name} {option}"))
        };
        // GNU tar's old-style first word may hold the directory letter.
        if TAR.contains(&name)
            && args
                .first()
                .is_some_and(|first| !first.starts_with('-') && first.contains('C'))
        {
            return Err(format!("{name} C"));
        }
        let mut at = 0;
        while let Some(word) = args.get(at) {
            if word == "--" {
                break;
            }
            if row.away_words.contains(&word.as_str()) {
                return Err(format!("{name} {word}"));
            }
            if let Some(long) = word.strip_prefix("--").filter(|long| !long.is_empty()) {
                let (option, attached) = match long.split_once('=') {
                    Some((option, value)) => (format!("--{option}"), Some(value.to_string())),
                    None => (word.clone(), None),
                };
                if row.long.contains(&option.as_str()) {
                    let value = if attached.is_some() {
                        attached
                    } else {
                        at += 1;
                        args.get(at).cloned()
                    };
                    dirs.push(directory(value.as_ref(), &option)?);
                } else if row.away_long.contains(&option.as_str())
                    || row
                        .long
                        .iter()
                        .chain(row.away_long)
                        .any(|full| full.len() > option.len() && full.starts_with(&option))
                {
                    return Err(format!("{name} {option}"));
                }
            } else if let Some(cluster) = word.strip_prefix('-').filter(|c| !c.is_empty()) {
                for (offset, letter) in cluster.char_indices() {
                    if valued.contains(&letter) && !row.short.contains(&letter) {
                        break;
                    }
                    if row.away_short.contains(&letter) {
                        return Err(format!("{name} -{letter}"));
                    }
                    if row.short.contains(&letter) {
                        let rest = &cluster[offset + letter.len_utf8()..];
                        let value = if rest.is_empty() {
                            at += 1;
                            args.get(at).cloned()
                        } else {
                            Some(rest.trim_start_matches('=').to_string())
                        };
                        dirs.push(directory(value.as_ref(), &format!("-{letter}"))?);
                        break;
                    }
                }
            }
            at += 1;
        }
    }
    Ok(dirs)
}

/// Whether `value` names a directory the guard can read as written: not
/// empty, no option, no home or run-time expansion, no pattern, and none
/// of a program's own placeholders (`{}`, `...`).
fn literal_directory(value: &str) -> bool {
    !value.is_empty()
        && value != "..."
        && !value.starts_with(['-', '~'])
        && !value.contains(['$', '`', '*', '?', '[', '{', '}', '\n'])
}

/// `body` judged in the starting directory and after each change in
/// `dirs`, each relative to the last.
fn relocated(dirs: &[String], body: &str) -> Vec<String> {
    let mut out = vec![body.to_string()];
    let mut prefix = String::new();
    for dir in dirs {
        prefix.push_str("cd ");
        prefix.push_str(&literal(dir));
        prefix.push_str(" && ");
        out.push(format!("{prefix}{body}"));
    }
    out
}

/// [`relocated`] for the commands git-guard's own launcher reader finds
/// (`launcher_commands`): every body in each directory the program's
/// options move it to, or none where the directory is unknown, which
/// [`first_unread`] refuses.
pub(crate) fn relocated_bodies(program: &str, args: &[String], bodies: Vec<String>) -> Vec<String> {
    if bodies.is_empty() {
        return bodies;
    }
    match run_directories(program_name(program), args) {
        Ok(dirs) => {
            let mut out = Vec::new();
            for body in &bodies {
                for moved in relocated(&dirs, body) {
                    if !out.contains(&moved) {
                        out.push(moved);
                    }
                }
            }
            out
        }
        Err(_) => Vec::new(),
    }
}

/// Every word of `args`, up to `--`, that spells an option of `rows` in a
/// form the reader does not parse exactly: an abbreviation or a case
/// variant of a `--` name, or a glued one-dash name inside a cluster. Each
/// refuses by name, so a spelling the program may accept is never left
/// unjudged; a full real option that is a prefix of a name stays the
/// program's own (`install --strip`).
fn unparsed_spellings(rows: &[&Carrier], name: &str, args: &[String]) -> Vec<Carried> {
    let longs: Vec<&str> = rows
        .iter()
        .flat_map(|row| row.long.iter().copied())
        .collect();
    let shadows: Vec<&str> = rows
        .iter()
        .flat_map(|row| row.shadows.iter().copied())
        .collect();
    let glued: Vec<&str> = rows
        .iter()
        .filter(|row| row.glued)
        .flat_map(|row| row.long.iter().copied())
        .filter(|long| !long.starts_with("--"))
        .collect();
    let mut out = Vec::new();
    for word in args
        .iter()
        .map(String::as_str)
        .take_while(|word| *word != "--")
    {
        let spelled = word.split_once('=').map_or(word, |(spelled, _)| spelled);
        let form = if spelled.starts_with("--") && spelled.len() > 2 {
            let lower = spelled.to_ascii_lowercase();
            let unparsed = !longs.contains(&spelled)
                && !shadows.contains(&spelled)
                && longs
                    .iter()
                    .any(|long| long.starts_with("--") && long.starts_with(lower.as_str()));
            unparsed.then(|| spelled.to_string())
        } else {
            word.strip_prefix('-')
                .filter(|cluster| !cluster.starts_with('-'))
                .and_then(|cluster| {
                    glued
                        .iter()
                        .find(|long| cluster.find(&long[1..]).is_some_and(|at| at > 0))
                })
                .map(|long| (*long).to_string())
        };
        if let Some(form) = form {
            let carried = Carried::Unread(format!("{name} {form}"));
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
    let spelled = word.split_once('=').map_or(word, |(spelled, _)| spelled);
    for long in carrier.long {
        if spelled == *long {
            return match word.split_once('=') {
                Some((_, value)) => Some(Value::Attached(value.to_string())),
                None => next(),
            };
        }
        if carrier.glued && !long.starts_with("--") {
            if let Some(rest) = word.strip_prefix(long).filter(|rest| !rest.is_empty()) {
                return Some(Value::Attached(
                    rest.strip_prefix('=').unwrap_or(rest).to_string(),
                ));
            }
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
            for carried in cargo_config(&value) {
                push(carried);
            }
        }
        Runs::Wgetrc => match wgetrc_command(&value) {
            Startup::Program(command) => push(Carried::Command(command)),
            Startup::Nested => push(Carried::Nested(format!("{name} {}", shown_option(word)))),
            Startup::Other => {}
        },
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
    // ssh's own configuration separators: ASCII blanks and line ends only.
    const BLANKS: &[char] = &[' ', '\t', '\r', '\n'];
    let value = value.trim_start_matches(BLANKS);
    let end = value
        .find(|c: char| c == '=' || BLANKS.contains(&c))
        .unwrap_or(value.len());
    let key = value[..end].to_ascii_lowercase();
    if !KEYS.contains(&key.as_str()) {
        return None;
    }
    let rest = value[end..].trim_start_matches(BLANKS);
    let rest = rest
        .strip_prefix('=')
        .unwrap_or(rest)
        .trim_start_matches(BLANKS);
    // ssh compares the disabling word exactly, so only `none` is inert.
    (!rest.is_empty() && rest != "none").then(|| rest.to_string())
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

/// What a Cargo `--config` value carries (TSK-1490 ruling D3). A value
/// without `=` names a configuration file, outside this reader. Any other
/// value is parsed as TOML, its keys decoded, and judged by key path, never
/// by its values: a setting naming a program Cargo runs refuses, an `env`
/// setting is judged as the same assignment written bare before Cargo, and
/// every other setting passes. A value that does not parse refuses.
fn cargo_config(value: &str) -> Vec<Carried> {
    let refuse = || vec![Carried::Unread("cargo --config".to_string())];
    if !value.contains('=') {
        return Vec::new();
    }
    let Ok(table) = value.parse::<toml::Table>() else {
        return refuse();
    };
    let mut out = Vec::new();
    for (key, setting) in &table {
        let path = [cargo_segment(key)];
        if path[0] != "env" {
            if names_cargo_program(&path, setting) {
                return refuse();
            }
            continue;
        }
        let toml::Value::Table(variables) = setting else {
            return refuse();
        };
        for (variable, setting) in variables {
            let text = match setting {
                toml::Value::String(text) => Some(text),
                toml::Value::Table(fields) => match fields.get("value") {
                    Some(toml::Value::String(text)) => Some(text),
                    _ => None,
                },
                _ => None,
            };
            let Some(text) = text.filter(|_| shell_name(variable)) else {
                return refuse();
            };
            out.push(Carried::Command(format!(
                "{variable}={} cargo",
                literal(text)
            )));
        }
    }
    out
}

/// A Cargo key segment as compared here: lower case, `_` read as `-`, so a
/// spelling Cargo may fold together is judged as one.
fn cargo_segment(key: &str) -> String {
    key.to_ascii_lowercase().replace('_', "-")
}

/// Whether the setting at `path`, or one below it, names a program Cargo
/// runs: anything under `target` or `host` (runners, linkers, flags,
/// build script overrides), a Cargo command alias or a credential provider
/// alias, and the program and flag keys wherever they stand.
fn names_cargo_program(path: &[String], setting: &toml::Value) -> bool {
    const ROOTS: &[&str] = &["target", "host", "alias", "credential-alias"];
    const KEYS: &[&str] = &[
        "runner",
        "linker",
        "ar",
        "rustc",
        "rustdoc",
        "rustc-wrapper",
        "rustc-workspace-wrapper",
        "browser",
        "credential-provider",
        "global-credential-providers",
        "rustflags",
        "rustdocflags",
        "codegen-backend",
    ];
    if ROOTS.contains(&path[0].as_str())
        || path.iter().any(|segment| KEYS.contains(&segment.as_str()))
    {
        return true;
    }
    match setting {
        toml::Value::Table(table) => table.iter().any(|(key, below)| {
            let mut deeper = path.to_vec();
            deeper.push(cargo_segment(key));
            names_cargo_program(&deeper, below)
        }),
        toml::Value::Array(items) => items.iter().any(|below| names_cargo_program(path, below)),
        _ => false,
    }
}

/// Whether `name` is a shell variable name, so `name=value` is an
/// assignment.
fn shell_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What a wget startup command (`-e NAME=VALUE`) does here.
enum Startup {
    /// It names the program the askpass setting runs.
    Program(String),
    /// It sets the startup-command setting again, whose own value is a
    /// further startup command (wget2 reads it so); it refuses at any
    /// depth rather than being read to a bound.
    Nested,
    /// Another setting, or text without `=`, which runs nothing.
    Other,
}

/// What a wget startup command (`-e NAME=VALUE`) does: wget compares the
/// name without case and with `-` and `_` dropped, so any spelling of the
/// askpass or startup-command setting counts, a leading `--` included.
fn wgetrc_command(value: &str) -> Startup {
    // wget's own blanks: ASCII spaces, tabs and line ends.
    const BLANKS: &[char] = &[' ', '\t', '\r', '\n', '\u{b}', '\u{c}'];
    let Some((key, rest)) = value.split_once('=') else {
        return Startup::Other;
    };
    let key: String = key
        .chars()
        .filter(|c| !matches!(c, '-' | '_') && !BLANKS.contains(c))
        .collect::<String>()
        .to_ascii_lowercase();
    let rest = rest.trim_matches(BLANKS);
    match key.as_str() {
        "execute" => Startup::Nested,
        "useaskpass" if !rest.is_empty() => Startup::Program(rest.to_string()),
        _ => Startup::Other,
    }
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

/// Literal data as shell text that reads back as the same string with no
/// expansion: single quotes, each `'` written `'\''`. A decoded setting
/// value is data the program receives, never shell text.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
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

    // A literal directory moves the command there as well as where the
    // line starts; a login's home is a directory the reader cannot
    // establish.
    #[test]
    fn a_moved_command_is_read_where_it_runs() {
        assert_eq!(
            commands("npm --prefix /tmp exec -c X"),
            vec![
                Carried::Command("X".into()),
                Carried::Command("cd '/tmp' && X".into())
            ]
        );
        assert_eq!(
            commands("su -lc X"),
            vec![Carried::Elsewhere("su -l".into())]
        );
    }

    #[test]
    fn option_spellings_read_one_value() {
        let x = || vec![Carried::Command("X".into())];
        for line in [
            "ack --pager=X",
            "man -PX",
            "man -aP X",
            "rsync -avzeX",
            "tar -cf a -I X",
            "zip -TT X",
            "zip -TTX",
            "zip -TT=X",
            "zip --unzip-command=X",
            "su -c X",
            "ssh -o ProxyCommand=X",
            "ssh -oLocalCommand=X",
            "tar --checkpoint-action=exec=X",
            "tar cIf X a.tar",
            "npm exec -c X",
            "npm install -c X",
            "/usr/bin/gsort --compress-program X",
            "wget --use-askpass=X",
            "wget -e use_askpass=X",
            "wget -qe Use-AskPass=X",
        ] {
            assert_eq!(commands(line), x(), "{line}");
        }
        for line in [
            "ack -- --pager=X",
            "install --strip X",
            "tar --checkpoint X",
            "man -H",
            "man -H X",
            "npm install --cache X",
            "ssh -o BatchMode=yes",
            "ssh -o ProxyCommand=none",
            "cargo --config build.jobs=2",
            "cargo --config ci.toml",
            "cargo --config build.target-dir=\"/tmp/runner\"",
            "vim +12",
            "less +G",
            "wget -e robots=off",
            "wget -Orelease.tar.gz",
            "tar --checkpoint=10",
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
        // ssh separates its keys only by ASCII blanks and `=`, and disables
        // a command only with `none` exactly.
        assert_eq!(ssh_command("ProxyCommand\tX"), Some("X".into()));
        assert_eq!(ssh_command("ProxyCommand\u{a0}X"), None);
        assert_eq!(ssh_command("ProxyCommand NONE"), Some("NONE".into()));
        // A Cargo `env` setting is the same assignment written bare.
        assert_eq!(
            commands("cargo --config env.APP_MODE=\"runner\""),
            vec![Carried::Command("APP_MODE='runner' cargo".into())]
        );
        // Its value is literal data, so no expansion in it stays live.
        assert_eq!(
            commands("cargo --config env.A=\"$(x)`y`'z\""),
            vec![Carried::Command("A='$(x)`y`'\\''z' cargo".into())]
        );
        for line in [
            "vim -c x",
            "less +!id",
            "emacs --ev x",
            "cargo --config target.x.runner=y",
            "cargo --config doc.\"brow\\u0073er\"=\"id\"",
            "cargo --config credential-alias.local=[\"id\"]",
            "cargo --config build.RUSTC_WRAPPER=\"id\"",
            "cargo --config a=",
            "cargo --config env.\"A-B\"=\"x\"",
            "cargo --conf build.jobs=2",
            "ack --pag X",
            "tar --use=X",
            "sort --compress-prog=X",
            "script --comm X",
            "vim --cMd x",
            "zip -qTTX",
            "wget --use-ask=X",
        ] {
            assert!(
                matches!(commands(line).as_slice(), [Carried::Unread(_)]),
                "{line}: {:?}",
                commands(line)
            );
        }
    }

    // A startup setting nested in another refuses at any depth, by any
    // spelling of its name, and its marker reads back as the same refusal.
    #[test]
    fn a_nested_startup_setting_refuses_at_any_depth() {
        for line in [
            "wget -e execute=use_askpass=X",
            "wget2 --execute=--Execute=use-askpass=X",
            "wget2 -qe execute=execute=robots=off",
        ] {
            let got = commands(line);
            assert!(
                matches!(got.as_slice(), [Carried::Nested(_)]),
                "{line}: {got:?}"
            );
            let marker = got[0].marker().unwrap();
            assert_eq!(refusal_marker(&marker).as_ref(), Some(&got[0]), "{line}");
        }
    }
}
