//! Install runs (TSK-1476): a run of the `CodeFlow` binary as `update` or
//! `init`, which writes the project's install files: harness settings,
//! hooks, policy and workflows. Only the operator runs them, so exec-guard
//! refuses such a run from an agent under `git.hook_integrity`, in every
//! folder and whichever program starts it.
//!
//! Each simple command is read as git-guard reads it
//! (`git_guard::simple_commands`), with the lexer's mark of whether the
//! shell passes each word as written. A word that names the binary is
//! judged with the words after it when it can be the program that runs: the
//! first word, a word after a launcher, supplier, package runner or package
//! manager, or any word of a program this reader does not know. A program
//! known never to run its operands (`cp`, `make` and the like) gives no
//! such word; `npm` is read that way only after one of its own commands
//! that never runs a program, written in full or as a documented alias.
//! `cargo run` and `cargo r` are judged by the words cargo passes to the
//! program it builds. A word the shell expands (a variable, a substitution,
//! a pattern, braces or a leading `=`) in a program position or as the
//! first operand refuses where the verb could follow, and one among the
//! words of a program this reader does not know refuses when its raw text,
//! with the words after it, names the binary with a verb. Only the exact
//! help and version shapes are allowed.
//!
//! A quoted argument holding a space is read again as a command line,
//! except for programs that only print, search or record the words they are
//! given. An assignment's value is read the same way when a command on the
//! line can read it: a prefix assignment's command, or any later command
//! that is not one of those programs. A line the reader cannot resolve, and
//! interpreter code, are judged on their raw text: the binary's name
//! followed, anywhere later, by a verb.
//!
//! A search or forge program is read as data only in the forms known to
//! run nothing: `ack`, `ag` and `rg` with each option on the program's
//! list, and `gh` with a command on gh's list. Any other option or gh
//! command, or a word the shell expands where an option can stand, leaves
//! the line judged on its raw text. A search program's option that runs a
//! program (`ack` and `ag` `--pager`, `rg --pre`, by any prefix of the
//! option's name) is judged as the program runs its value: a command line,
//! through this same reader, or one program given each searched path. A
//! pager, editor, browser or preprocessor variable assigned before a
//! program that runs it, or earlier on the line, is judged the same way,
//! and so is each pager, editor, browser and text converter a Git call on
//! the line runs. One check decides all of these: such a value is data
//! only when each program it names is a viewer known to run nothing it is
//! given (`less`, `cat`, an editor, a browser), with each option on that
//! viewer's own list and no operand but `-`. Any other program or option
//! leaves the line judged on its raw text, and so does a live variable
//! that gives such a program options, start-up commands or a settings
//! file.
//! An assignment anywhere on the line is live for the whole line, and any
//! program on it that can start another one (a shell, a Git alias,
//! `find -exec`) judges every such variable, since the program it starts
//! inherits them. No `unset` or new value removes a value; a new value
//! only adds one. A builtin that writes or declares names is read only in
//! its admitted form: each option on that builtin's list of letters known
//! to keep plain identifier semantics, and each name it writes a plain
//! identifier this reader does not track. Any other form (a pattern
//! target, a name reference, a subscript, a word the shell expands), a
//! tracked variable written without a `NAME=value` word this reader sees
//! (`read`, `printf -v`, `NAME+=`, `${NAME:=...}`, a loop variable, a
//! declaration of a bare name), a builtin this reader does not model, and
//! one that rebinds a program name or changes how the shell reads later
//! words leave the line judged on its raw text. These checks read what the current shell runs,
//! once zsh's precommand modifiers and the `builtin` and `command`
//! wrappers are removed, with the lexer's mark of each word kept; a wrapper
//! or program word the shell expands, or a wrapper this reader cannot
//! remove, leaves the line judged on its raw text.
//! The words `gh repo clone`, `gh repo fork` and `gh gist clone` pass to
//! `git` after `--` are read as quoted arguments are. An editor's own
//! command (`vim -c`, `emacs
//! --eval`, a `+` word for vi or less) is judged on its raw text, as
//! interpreter code is.
//!
//! A program an agent builds, renames or copies, and the verb's words
//! written to a file and run later, are not named on the line. They are a
//! named residual.

use super::headless::{basename, shell_command_string};
use super::launchers::{LAUNCHERS, PACKAGE_RUNNERS, SHELLS, SUPPLIERS};
use crate::hooks::git_guard::{
    builtin_words, command_argv_literal, expand_commands, is_shell_modifier, shell_blank,
    strip_launchers, strip_reserved_words,
};

/// The binary's program name.
const BINARY: &str = "codeflow";

/// The verbs that write the project's install files.
const VERBS: &[&str] = &["update", "init"];

/// The words after the binary's name that only print help or the version,
/// in their exact shapes. Any other word on the call, an option's value
/// included, makes it a run.
const PRINTS_ONLY: &[&[&str]] = &[
    &["update", "--help"],
    &["update", "-h"],
    &["init", "--help"],
    &["init", "-h"],
    &["help", "update"],
    &["help", "init"],
    &["help"],
    &["--help"],
    &["-h"],
    &["--version"],
    &["-V"],
];

/// Programs that print, search, compare or show the words they are given
/// and never run one, so the binary's name among their words is data.
const DATA_PROGRAMS: &[&str] = &[
    "[", "ack", "ag", "cat", "echo", "gh", "git", "grep", "head", "jq", "ls", "printf", "rg",
    "tail", "test", "type", "wc", "which",
];

/// The data programs that start no other program, so no command variable
/// they inherit runs.
const STARTS_NO_PROGRAM: &[&str] = &[
    "[", "cat", "echo", "grep", "head", "jq", "ls", "printf", "tail", "test", "type", "wc", "which",
];

/// How a search program runs the value of an option that runs a program.
#[derive(Clone, Copy)]
enum OptionValue {
    /// A command line, handed to a shell or a command pipe: `ack --pager`
    /// (Perl's pipe `open`) and `ag --pager` (`popen`).
    CommandLine,
    /// One program path, run with each searched path as its argument and
    /// the file's content on its input: `rg --pre`.
    ProgramPath,
}

/// Options of a [`DATA_PROGRAMS`] search program that run a program, with
/// how each runs its value. An option is matched by any prefix of its
/// name, since `ack` (Perl's `Getopt::Long`) and `ag` (`getopt_long`)
/// accept a unique abbreviation; the value is attached after `=` or is the
/// next word.
const RUNS_THROUGH_OPTION: &[(&str, OptionValue)] = &[
    ("--pager", OptionValue::CommandLine),
    ("--pre", OptionValue::ProgramPath),
];

/// Variables a [`DATA_PROGRAMS`] program runs as a command: a pager, an
/// editor or a browser. An assignment of one earlier on the line is read
/// when that program follows. Git runs the command variables the startup
/// check lists; its runner reader also reads them.
const COMMAND_VARIABLES: &[(&str, &[&str])] = &[
    ("ack", &["ACK_PAGER", "ACK_PAGER_COLOR", "PAGER"]),
    ("git", super::startup::COMMAND_VARIABLES),
    (
        "gh",
        &[
            "BROWSER",
            "EDITOR",
            "GH_BROWSER",
            "GH_EDITOR",
            "GH_PAGER",
            "GIT_EDITOR",
            "PAGER",
            "VISUAL",
        ],
    ),
];

/// The `gh` commands that pass the words after their `--` to the `git
/// clone` they run, as gh's source declares them: `repo clone <repository>
/// [<directory>] [-- <gitflags>...]`, `repo fork [<repository>] [--
/// <gitflags>...]` (used when it clones) and `gist clone <gist>
/// [<directory>] [-- <gitflags>...]`. Each is its two words joined by one
/// space.
const GH_GIT_COMMANDS: &[&str] = &["gist clone", "repo clone", "repo fork"];

/// Editors and a pager whose own command language runs a shell command
/// (`!` in vi and less, `shell-command` in emacs). Their command options'
/// values and their `+` words are judged on their raw text, as
/// interpreter code is.
const EDITORS: &[&str] = &[
    "emacs", "ex", "less", "nvim", "rview", "rvim", "vi", "view", "vim",
];

/// The options of [`EDITORS`] whose value is a command in the editor's
/// own language.
const EDITOR_COMMAND_OPTIONS: &[&str] = &["-c", "--cmd", "--eval", "-eval", "--execute"];

/// The binary's own verbs that record or read the words they are given and
/// never run one, so their free text is data.
const RECORD_VERBS: &[&str] = &["adr", "epic", "feedback", "recall", "spec", "task"];

/// Programs known never to run one of their operands as a program, so a
/// folder or file named like the binary among their words is data. A
/// quoted argument holding a space is still read as a command line, since
/// a value such as a `make` variable can be. `npm` is read this way only
/// after one of [`NPM_OWN_COMMANDS`]; after any other word it runs a
/// package's binary, as [`PACKAGE_MANAGERS`] do.
const NOT_LAUNCHERS: &[&str] = &[
    "chmod", "chown", "cmp", "cp", "cut", "diff", "du", "file", "ln", "make", "mkdir", "mv", "npm",
    "rm", "rmdir", "sort", "stat", "tar", "tee", "touch", "tr", "uniq",
];

/// Package and tool managers that run a package's or a tool's binary by
/// name with the words after it, with or without a subcommand such as
/// `run`, `exec`, `dlx` or `x` (`yarn <name>`, `pnpm run <name>`,
/// `bun x <name>`, `corepack <manager>`, `volta run <name>`, `uv run
/// <name>`, `uvx <name>`). Every word after one is read as a program
/// position, whatever subcommand comes first, so no alias or abbreviation
/// of a subcommand moves a word out of it. `pn` is pnpm's short name.
const PACKAGE_MANAGERS: &[&str] = &[
    "bun", "corepack", "deno", "pn", "pnpm", "uv", "uvx", "volta", "yarn",
];

/// npm's own commands, in their exact names and documented aliases, that
/// never run a program named on the command line: they install, list,
/// show, publish or configure packages. The names are npm's `commands` and
/// `aliases` (`lib/utils/cmd-list.js` in the npm/cli repository). npm also
/// accepts any unambiguous abbreviation of a name (`deref` there, through
/// `abbrev`), so `exe` runs `exec`; this list holds exact words only, and
/// any other word, an abbreviation included, leaves npm read as a package
/// runner. Left out: the commands that run a program, a package's binary
/// or a script (`exec`, `explore`, `init`, `run`, `start`, `stop`,
/// `restart`, `test`, `install-test`, `install-ci-test`), those that open
/// an editor, browser or viewer (`edit`, `config` and its alias `c`, whose
/// `edit` opens one, `docs`, `bugs`, `repo`, `help`, `fund`), and those
/// whose effect this reader has not established
/// (`approve-scripts`, `deny-scripts`, `install-scripts`, `patch`,
/// `stage`, `trust`, `help-search`).
const NPM_OWN_COMMANDS: &[&str] = &[
    // Commands.
    "access",
    "audit",
    "cache",
    "ci",
    "completion",
    "dedupe",
    "deprecate",
    "diff",
    "dist-tag",
    "doctor",
    "explain",
    "find-dupes",
    "get",
    "install",
    "link",
    "ll",
    "login",
    "logout",
    "ls",
    "org",
    "outdated",
    "owner",
    "pack",
    "ping",
    "pkg",
    "prefix",
    "profile",
    "prune",
    "publish",
    "query",
    "rebuild",
    "root",
    "sbom",
    "search",
    "set",
    "team",
    "token",
    "undeprecate",
    "uninstall",
    "unpublish",
    "update",
    "version",
    "view",
    "whoami",
    // Their aliases.
    "add",
    "author",
    "clean-install",
    "ddp",
    "dist-tags",
    "find",
    "i",
    "ic",
    "in",
    "info",
    "ins",
    "inst",
    "insta",
    "instal",
    "install-clean",
    "isnt",
    "isnta",
    "isntal",
    "isntall",
    "isntall-clean",
    "la",
    "list",
    "ln",
    "ogr",
    "r",
    "rb",
    "remove",
    "rm",
    "s",
    "se",
    "show",
    "u",
    "udpate",
    "un",
    "unlink",
    "up",
    "upgrade",
    "v",
    "verison",
    "why",
];

/// npm options before its command that take the next word as their value.
const NPM_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-w",
    "--cache",
    "--globalconfig",
    "--loglevel",
    "--prefix",
    "--registry",
    "--userconfig",
    "--workspace",
];

/// npm options before its command that take no value.
const NPM_FLAGS: &[&str] = &[
    "-d",
    "-f",
    "-g",
    "-q",
    "-s",
    "-ws",
    "-y",
    "--dry-run",
    "--force",
    "--global",
    "--json",
    "--no-audit",
    "--no-fund",
    "--offline",
    "--quiet",
    "--silent",
    "--verbose",
    "--workspaces",
    "--yes",
];

/// Shell builtins that set variables from their operands.
const DECLARES: &[&str] = &["declare", "export", "local", "readonly", "typeset"];

/// Words after which a supplier or package manager starts the command
/// that follows.
const STARTS_A_COMMAND: &[&str] = &[
    "-exec", "-execdir", "-ok", "-okdir", "exec", "explore", "x", "dlx",
];

/// Suppliers that add the words they read after the command they run.
const APPENDING: &[&str] = &["parallel", "xargs"];

/// cargo's options, before or after its subcommand, that take the next
/// word as their value.
const CARGO_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-F",
    "-Z",
    "-j",
    "-p",
    "--bench",
    "--bin",
    "--color",
    "--config",
    "--example",
    "--features",
    "--jobs",
    "--lockfile-path",
    "--manifest-path",
    "--message-format",
    "--package",
    "--profile",
    "--target",
    "--target-dir",
    "--test",
];

/// An install run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRun {
    /// The form matched, such as `codeflow update` or `cargo run`.
    pub form: &'static str,
    /// `false` when the line could not be parsed and its raw text named the
    /// binary with a verb.
    pub parsed: bool,
    /// `true` when the run was found inside a quoted argument or an
    /// assignment's value, which the guard read as a command line.
    pub in_argument: bool,
}

/// The first install run in `command`, if any. `posix` is false for a
/// shell that reads a backslash as a path separator, such as `PowerShell`,
/// where the command is also read with each backslash as a `/`.
#[must_use]
pub fn install_run(command: &str, posix: bool) -> Option<InstallRun> {
    find(command, 0).or_else(|| {
        (!posix && command.contains('\\'))
            .then(|| find(&command.replace('\\', "/"), 0))
            .flatten()
    })
}

/// One simple command, read in two views. The program view (`argv`,
/// `literal`, `values`) is the program that finally runs, after every
/// launcher, for the checks of what runs. The shell view (`shell`) is what
/// the current shell itself does, for every check of the variables it
/// holds. `words` is every word as written, for checks of a word's text.
struct Simple {
    /// The argument vector from the program on, after every launcher.
    argv: Vec<String>,
    /// Whether the shell passes each word of `argv` as written.
    literal: Vec<bool>,
    /// The assignments before the program, launchers' included.
    values: Vec<(String, String)>,
    words: Vec<String>,
    shell: Shell,
}

/// What one simple command does in the current shell ([`current_shell`]).
enum Shell {
    /// Assignments alone, which set their variables in this shell.
    Assigns(Vec<(String, String)>),
    /// The words of the command this shell runs, from its program on, and
    /// whether the shell passes each one as written ([`written`]).
    Runs {
        words: Vec<String>,
        written: Vec<bool>,
    },
    /// Nothing this shell keeps: a `command -v` lookup or a bare wrapper.
    Nothing,
    /// A wrapper the view cannot remove: a `command` option it does not
    /// know, or a wrapper or program word the shell expands.
    Hidden,
}

/// Whether the shell passes a word as exactly the text written: the
/// lexer's literal mark, and no `$`, backtick or substitution left in it.
fn written(word: &str, literal: bool) -> bool {
    literal && !unresolved(word)
}

/// The simple commands of `command`, read as
/// `git_guard::simple_commands` reads them, with each word's literal mark
/// and the assignments the launcher strip drops.
fn simple(command: &str) -> Vec<Simple> {
    expand_commands(command)
        .iter()
        .map(|segment| {
            let (mut words, mut literal) = command_argv_literal(segment);
            let all = words.clone();
            let total = words.len();
            strip_reserved_words(&mut words);
            literal.drain(..total - words.len());
            let start = strip_launchers(&words)
                .map_or(words.len(), |(_, args)| words.len() - args.len() - 1);
            let values = words[..start]
                .iter()
                .filter_map(|word| assigned(word))
                .collect();
            Simple {
                argv: words[start..].to_vec(),
                literal: literal[start..].to_vec(),
                values,
                shell: current_shell(&words, &literal),
                words: all,
            }
        })
        .collect()
}

/// The shell view of one simple command's words, once its reserved words
/// are stripped: its leading assignments are removed, then zsh's
/// precommand modifiers (`noglob`, `nocorrect`, `-`) and the builtin
/// wrappers `builtin`, `command`, `command -p` and `command --`
/// ([`builtin_words`]), as often as they lead, since each runs the command
/// after it in this same shell. Any other launcher (`env`, `exec`, `nice`,
/// `timeout`) starts a new program, so its words stay in this view and the
/// command it runs changes nothing here. The lexer's literal marks go with
/// the words, and a removed wrapper word or a program word the shell
/// expands hides the command.
fn current_shell(words: &[String], literal: &[bool]) -> Shell {
    let start = words
        .iter()
        .position(|word| assigned(word).is_none() && !word.contains("+="))
        .unwrap_or(words.len());
    let command = &words[start..];
    if command.is_empty() {
        return Shell::Assigns(words.iter().filter_map(|word| assigned(word)).collect());
    }
    let mut rest = command;
    loop {
        let before = rest.len();
        while rest.first().is_some_and(|word| is_shell_modifier(word)) {
            rest = &rest[1..];
        }
        rest = builtin_words(rest);
        if rest.len() == before {
            break;
        }
    }
    let from = words.len() - rest.len();
    let marks: Vec<bool> = (start..words.len())
        .map(|at| written(&words[at], literal.get(at).copied().unwrap_or(false)))
        .collect();
    let (wrappers, marks) = marks.split_at(from - start);
    // The lexer marks any `[` as pattern syntax, but the test builtins'
    // names alone hold no pattern the shell can expand.
    let test = rest.first().is_some_and(|word| word == "[" || word == "[[");
    if wrappers.contains(&false) || (marks.first() == Some(&false) && !test) {
        return Shell::Hidden;
    }
    match rest.first() {
        None => Shell::Nothing,
        Some(program) if program == "command" => Shell::Hidden,
        Some(_) => Shell::Runs {
            words: rest.to_vec(),
            written: marks.to_vec(),
        },
    }
}

/// The name and value of a `NAME=value` word.
fn assigned(word: &str) -> Option<(String, String)> {
    let (name, value) = word.split_once('=')?;
    identifier(name).then(|| (name.to_string(), value.to_string()))
}

/// What one simple command is.
enum Found {
    Run(InstallRun),
    /// It runs something this reader cannot resolve.
    Unresolved,
    Nothing,
}

fn find(command: &str, depth: usize) -> Option<InstallRun> {
    match read(command, depth) {
        Found::Run(run) => Some(run),
        Found::Unresolved => raw_run(command),
        Found::Nothing => None,
    }
}

/// What `command` is, read as a command line: a run, a command this reader
/// cannot resolve, whose text is then judged raw, or nothing.
fn read(command: &str, depth: usize) -> Found {
    if depth > 4 {
        return Found::Unresolved;
    }
    let mut unresolved = command.contains("<<<");
    // Every value assigned anywhere on the line is live for every command
    // on it: the command list carries no scope or order of execution, and
    // a loop or function can run a later assignment first. No `unset` or
    // new value removes one; a new value only adds to them. Each program
    // that can read one judges every value. A command variable written
    // in a way this reader cannot see leaves the line judged on its raw
    // text ([`unseen_write`]). Every check of what a command does to these
    // variables reads its shell view ([`current_shell`]).
    let commands = simple(command);
    let live: Vec<(String, String)> = commands.iter().flat_map(line_values).collect();
    for command in commands {
        if unseen_write(&command) {
            unresolved = true;
        }
        let builtin = match &command.shell {
            Shell::Assigns(_) => continue,
            Shell::Runs { words, .. } => words.first().map(|program| basename(program)),
            Shell::Hidden => {
                unresolved = true;
                None
            }
            Shell::Nothing => None,
        };
        // A command with no program, such as `env NAME=value`, sets nothing
        // in this shell and runs nothing.
        let Some(program) = command.argv.first() else {
            continue;
        };
        let name = basename(program);
        let mut judge =
            |variable: &str, value: &str| match assignment_value(&name, variable, value, depth) {
                Found::Run(run) => Some(run),
                Found::Unresolved => {
                    unresolved = true;
                    None
                }
                Found::Nothing => None,
            };
        for (variable, value) in &command.values {
            if let Some(run) = judge(variable, value) {
                return Found::Run(run);
            }
        }
        // A declaration or an `unset` this shell runs starts no program.
        // Behind another launcher it runs as a program, which reads the
        // values as any program does.
        if builtin.is_some_and(|builtin| DECLARES.contains(&builtin.as_str()) || builtin == "unset")
        {
            continue;
        }
        // A data program reads only the variables it runs as a command or
        // reads as options; any other program may also run any value as a
        // command.
        let data = DATA_PROGRAMS.contains(&name.as_str());
        for (variable, value) in live.iter().filter(|(variable, _)| {
            !data || runs_variable(&name, variable) || reads_options(&name, variable)
        }) {
            if let Some(run) = judge(variable, value) {
                return Found::Run(run);
            }
        }
        match classify(&command.argv, &command.literal, depth) {
            Found::Run(run) => return Found::Run(run),
            Found::Unresolved => unresolved = true,
            Found::Nothing => {}
        }
    }
    if unresolved {
        Found::Unresolved
    } else {
        Found::Nothing
    }
}

/// Whether `program`, or a program it starts with its environment, may run
/// the value of `variable` as a command. A data program that starts no
/// program runs none. Any other program may start one that reads any
/// command variable: a search program's pager, a Git alias or runner, a
/// gh extension, a shell, `find -exec` or `xargs`. So every command
/// variable a data program lists ([`COMMAND_VARIABLES`]) or the startup
/// check lists counts for it.
fn runs_variable(program: &str, variable: &str) -> bool {
    !STARTS_NO_PROGRAM.contains(&program) && command_variable(variable)
}

/// Whether `variable` is a command variable ([`command_variables`]).
fn command_variable(variable: &str) -> bool {
    command_variables().any(|name| *name == variable)
}

/// What the words after a [`WRITERS`] builtin's options are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operands {
    /// `NAME` or `NAME=value` words it declares.
    Declarations,
    /// The names it writes.
    Names,
    /// Data, such as a format and its arguments; it writes only the name
    /// its name option gives.
    Data,
    /// An option string, the name it writes, then data (`getopts`).
    Getopts,
    /// None: an operand leaves the line judged on its raw text.
    Nothing,
}

/// The form in which a builtin that writes or declares names, or rebinds
/// a program name, is read: the option letters known to keep plain
/// identifier semantics in bash, zsh and POSIX sh, by what each takes, and
/// what its operands are. Any other letter, a `+` option of a
/// declaration, or a word the shell expands where an option can stand
/// leaves the line judged on its raw text.
struct Writer {
    builtins: &'static [&'static str],
    /// Letters that take no value.
    flags: &'static str,
    /// Letters that only list, admitted when no operand follows.
    listing: &'static str,
    /// Letters whose value is data in every shell that has the builtin.
    data: &'static str,
    /// Letters whose value one shell reads as data and another as a name
    /// (`read -p`, `-n` and `-t` in bash and zsh), so the value must also
    /// be a plain name or a number.
    either: &'static str,
    /// Letters whose value is a name the builtin writes.
    name: &'static str,
    /// The letter without which the builtin writes no name, if any.
    writes_with: Option<char>,
    operands: Operands,
}

/// The builtins that write or declare names, with their admitted forms
/// (the option table in the TSK-1476 report cites each manual).
const WRITERS: &[Writer] = &[
    Writer {
        builtins: &["declare", "typeset"],
        flags: "grx",
        listing: "p",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Declarations,
    },
    Writer {
        builtins: &["local"],
        flags: "rx",
        listing: "p",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Declarations,
    },
    Writer {
        builtins: &["export", "readonly"],
        flags: "",
        listing: "p",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Declarations,
    },
    Writer {
        builtins: &["read"],
        flags: "ers",
        listing: "",
        data: "du",
        either: "Ninpt",
        name: "a",
        writes_with: None,
        operands: Operands::Names,
    },
    Writer {
        builtins: &["mapfile", "readarray"],
        flags: "t",
        listing: "",
        data: "Odnsu",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Names,
    },
    Writer {
        builtins: &["vared"],
        flags: "ceh",
        listing: "",
        data: "Mfimprt",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Names,
    },
    Writer {
        builtins: &["sysread"],
        flags: "",
        listing: "",
        data: "iost",
        either: "",
        name: "c",
        writes_with: None,
        operands: Operands::Names,
    },
    Writer {
        builtins: &["getln"],
        flags: "",
        listing: "",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Names,
    },
    Writer {
        builtins: &["getopts"],
        flags: "",
        listing: "",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Getopts,
    },
    Writer {
        builtins: &["printf"],
        flags: "",
        listing: "",
        data: "",
        either: "",
        name: "v",
        writes_with: Some('v'),
        operands: Operands::Data,
    },
    Writer {
        builtins: &["print"],
        flags: "Nlnr",
        listing: "",
        data: "CXfux",
        either: "",
        name: "v",
        writes_with: Some('v'),
        operands: Operands::Data,
    },
    Writer {
        builtins: &["wait"],
        flags: "fn",
        listing: "",
        data: "",
        either: "",
        name: "p",
        writes_with: Some('p'),
        operands: Operands::Data,
    },
    Writer {
        builtins: &["strftime"],
        flags: "nqr",
        listing: "",
        data: "",
        either: "",
        name: "s",
        writes_with: Some('s'),
        operands: Operands::Data,
    },
    Writer {
        builtins: &["hash"],
        flags: "r",
        listing: "",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Nothing,
    },
    Writer {
        builtins: &["trap"],
        flags: "",
        listing: "lp",
        data: "",
        either: "",
        name: "",
        writes_with: None,
        operands: Operands::Nothing,
    },
];

/// Builtins this reader does not model, read in no form: they write
/// names by a pattern, a reference, an arithmetic expression or a module's
/// own rules, run code in this shell later or from text this reader does
/// not see, rebind a program or builtin name, or change how the shell
/// reads later words. Any of them leaves the line judged on its raw text.
const UNMODELED: &[&str] = &[
    ".",
    "alias",
    "autoload",
    "compgen",
    "disable",
    "emulate",
    "enable",
    "eval",
    "fc",
    "float",
    "fltexpr",
    "functions",
    "integer",
    "let",
    "nameref",
    "pcre_match",
    "private",
    "sched",
    "setopt",
    "shopt",
    "source",
    "syserror",
    "sysopen",
    "unsetopt",
    "zcurses",
    "zformat",
    "zgetattr",
    "zle",
    "zlistattr",
    "zmodload",
    "zparseopts",
    "zpty",
    "zregexparse",
    "zselect",
    "zsocket",
    "zstat",
    "zstyle",
    "zsystem",
    "ztcp",
    "ztie",
];

/// The `set` option letters that change only whether the shell stops,
/// traces, exports or expands, never which word sets a name.
const SET_FLAGS: &str = "Caefnuvx";

/// The `set -o` names this reader admits, those of [`SET_FLAGS`] and
/// `pipefail`.
const SET_OPTIONS: &[&str] = &[
    "allexport",
    "errexit",
    "noclobber",
    "noexec",
    "noglob",
    "nounset",
    "pipefail",
    "verbose",
    "xtrace",
];

/// Variables that give a search or forge program, or a viewer it runs
/// ([`VIEWERS`]), options, start-up commands, a settings file or a pager
/// of its own, which can name a program it runs. Live on the line for a
/// program that may start another, each leaves the line judged on its raw
/// text.
const OPTIONS_VARIABLES: &[&str] = &[
    "ACKRC",
    "ACK_OPTIONS",
    "BAT_CONFIG_PATH",
    "BAT_PAGER",
    "EXINIT",
    "GH_CONFIG_DIR",
    "GVIMINIT",
    "LESS",
    "LESSKEY",
    "LESSKEYIN",
    "LESSKEY_CONTENT",
    "MORE",
    "NVIM_APPNAME",
    "RIPGREP_CONFIG_PATH",
    "VIMINIT",
    "XDG_CONFIG_HOME",
];

/// The command variables a program hands its output, a file or an
/// address to read: pagers, editors, browsers and less's input
/// preprocessor. Their values are judged by [`fed_input`].
const FED_VARIABLES: &[&str] = &[
    "ACK_PAGER",
    "ACK_PAGER_COLOR",
    "BROWSER",
    "EDITOR",
    "FCEDIT",
    "GH_BROWSER",
    "GH_EDITOR",
    "GH_PAGER",
    "GIT_EDITOR",
    "GIT_PAGER",
    "GIT_SEQUENCE_EDITOR",
    "LESSCLOSE",
    "LESSOPEN",
    "MANPAGER",
    "PAGER",
    "VISUAL",
];

/// A program known to show, convert or open what it is given and run
/// nothing from it, with the options it is admitted with, each read from
/// its manual: flag letters, letters that take a value (attached or the
/// next word), long options matched exactly, and long options that take
/// a value (after `=` or the next word). Any other option, an operand
/// other than `-` (its input), or a `+` word is not admitted.
struct Viewer {
    names: &'static [&'static str],
    flags: &'static str,
    values: &'static str,
    long: &'static [&'static str],
    long_values: &'static [&'static str],
}

/// A [`Viewer`] admitted with no option.
const fn bare(names: &'static [&'static str]) -> Viewer {
    Viewer {
        names,
        flags: "",
        values: "",
        long: &[],
        long_values: &[],
    }
}

/// The programs a pager, editor, browser or preprocessor value may name
/// ([`fed_input`]), each with its admitted options. A program whose ways
/// of running code cannot be listed from its manual is left out, so a
/// value naming it leaves the line judged on its raw text: Emacs and
/// `emacsclient` (Lisp from `--script`, `-l`, `--eval`, `-batch` and an
/// alternate editor), Kakoune (`-e` commands), text browsers whose
/// settings run programs (`lynx`, `links`, `w3m`), `delta` (which runs
/// the pager its Git settings name), and pagers whose options are not
/// listed here (`moar`, `most`, `ov`, `ydiff`).
const VIEWERS: &[Viewer] = &[
    // POSIX `:`, `true` and `false` run nothing and take no option.
    bare(&[":", "true", "false"]),
    // cat(1), GNU coreutils and BSD: numbering, marking and squeezing.
    Viewer {
        names: &["cat"],
        flags: "AbeElnstTuv",
        values: "",
        long: &[
            "--number",
            "--number-nonblank",
            "--show-all",
            "--show-ends",
            "--show-nonprinting",
            "--show-tabs",
            "--squeeze-blank",
        ],
        long_values: &[],
    },
    // echo(1) and bash's echo: `-n`, `-e` and `-E` escape handling.
    Viewer {
        names: &["echo"],
        flags: "eEn",
        values: "",
        long: &[],
        long_values: &[],
    },
    // head(1) and tail(1), GNU coreutils and BSD: a count of lines or
    // bytes, headers, and tail's follow and reverse modes.
    Viewer {
        names: &["head"],
        flags: "qv",
        values: "cn",
        long: &["--quiet", "--silent", "--verbose"],
        long_values: &["--bytes", "--lines"],
    },
    Viewer {
        names: &["tail"],
        flags: "fFqrv",
        values: "cn",
        long: &["--follow", "--quiet", "--silent", "--verbose"],
        long_values: &["--bytes", "--lines"],
    },
    // Decompressors that write to standard output: no option.
    bare(&["bzcat", "gzcat", "xzcat", "zcat", "zstdcat"]),
    // less(1) OPTIONS: display, search, prompt and exit options, and
    // `-L` (no input preprocessor). Not admitted: `-k` and the lesskey
    // options (key bindings, which can run a shell), `-o` and `-O` (log
    // files), `-t` and `-T` (tags), `-p`, `-P`, `-D` and `+` (commands
    // run at start).
    Viewer {
        names: &["less"],
        flags: "aAcdeEfFgGiIJKLmMnNqQrRsSuUwWX",
        values: "bhjxyz",
        long: &[
            "--chop-long-lines",
            "--clear-screen",
            "--HILITE-UNREAD",
            "--hilite-unread",
            "--IGNORE-CASE",
            "--ignore-case",
            "--LINE-NUMBERS",
            "--line-numbers",
            "--LONG-PROMPT",
            "--long-prompt",
            "--mouse",
            "--no-init",
            "--no-lessopen",
            "--QUIET",
            "--quiet",
            "--QUIT-AT-EOF",
            "--quit-at-eof",
            "--quit-if-one-screen",
            "--quit-on-intr",
            "--RAW-CONTROL-CHARS",
            "--raw-control-chars",
            "--SILENT",
            "--silent",
            "--squeeze-blank-lines",
            "--status-column",
            "--use-color",
        ],
        long_values: &[
            "--buffers",
            "--jump-target",
            "--max-back-scroll",
            "--max-forw-scroll",
            "--tabs",
            "--window",
        ],
    },
    // more(1), util-linux, and macOS's less in its more mode: display
    // options shared by both. Not admitted: `+` (a start line or search).
    Viewer {
        names: &["more"],
        flags: "cdflpsu",
        values: "",
        long: &[],
        long_values: &[],
    },
    // bat(1): plain style, numbers, shown characters, paging mode,
    // color, style, language, theme, tabs and wrapping. Not admitted:
    // `--pager` (a command it runs) or any other option.
    Viewer {
        names: &["bat", "batcat"],
        flags: "AnpP",
        values: "l",
        long: &["--number", "--plain", "--show-all"],
        long_values: &[
            "--color",
            "--decorations",
            "--language",
            "--paging",
            "--style",
            "--tabs",
            "--theme",
            "--wrap",
        ],
    },
    // Git diff filters: diff-so-fancy's `--patch` (its README, for
    // `interactive.diffFilter`) and diff-highlight with no option.
    Viewer {
        names: &["diff-so-fancy"],
        flags: "",
        values: "",
        long: &["--patch"],
        long_values: &[],
    },
    bare(&["diff-highlight"]),
    // vim(1) OPTIONS, also nvim(1) and vi: read-only, no-modify,
    // no-write, no-swap, binary, restricted, diff and foreground modes,
    // and window layout. Not admitted: `-c`, `--cmd`, `-S`, `-s`, `-u`,
    // `-U`, `-e`, `-E`, `-i`, `-w`, `-W`, `-y` and `+` (commands, scripts,
    // settings files and Ex mode, which reads commands from its input).
    Viewer {
        names: &["nvim", "vi", "view", "vim"],
        flags: "bdfmMnoOpRZ",
        values: "",
        long: &["--clean", "--noplugin", "--not-a-term"],
        long_values: &[],
    },
    // Editors that open a window, given a file by an editor variable:
    // wait for it to close, or choose a window (`code --help`, `subl
    // --help`, `zed --help`, `mate -h`, `gedit --help`, `kate --help`).
    Viewer {
        names: &["code", "code-insiders", "codium", "cursor"],
        flags: "nrw",
        values: "",
        long: &["--new-window", "--reuse-window", "--wait"],
        long_values: &[],
    },
    Viewer {
        names: &["subl"],
        flags: "nw",
        values: "",
        long: &["--new-window", "--wait"],
        long_values: &[],
    },
    Viewer {
        names: &["zed"],
        flags: "nw",
        values: "",
        long: &["--new", "--wait"],
        long_values: &[],
    },
    Viewer {
        names: &["mate"],
        flags: "w",
        values: "",
        long: &["--wait"],
        long_values: &[],
    },
    Viewer {
        names: &["gedit"],
        flags: "sw",
        values: "",
        long: &["--new-window", "--standalone", "--wait"],
        long_values: &[],
    },
    Viewer {
        names: &["kate"],
        flags: "bn",
        values: "",
        long: &["--block", "--new"],
        long_values: &[],
    },
    // Terminal editors given a file: no option.
    bare(&["hx", "micro", "nano", "pico"]),
    // Browsers given an address and openers that hand it to the default
    // program: no option (`open -a` names any application).
    bare(&[
        "chromium",
        "firefox",
        "google-chrome",
        "open",
        "wslview",
        "xdg-open",
    ]),
    // pdftotext(1) as a search preprocessor: no option.
    bare(&["pdftotext"]),
];

/// One search program's options known to run nothing: its flag letters,
/// its letters that take a value, and its long option names. Any other
/// option, except one of [`RUNS_THROUGH_OPTION`], leaves the line judged
/// on its raw text. Long names match exactly, so an abbreviation `ack` or
/// `ag` would accept is read as unknown.
struct SearchOptions {
    program: &'static str,
    flags: &'static str,
    values: &'static str,
    long: &'static [&'static str],
}

/// The options of `ack`, `ag` and `rg` known to run nothing, from each
/// program's manual (`ack --help`, `ag --help`, `rg --help`). Left out:
/// options that read a settings file or run a program, and those whose
/// effect this reader has not established.
const SEARCH_OPTIONS: &[SearchOptions] = &[
    SearchOptions {
        program: "ack",
        flags: "1HILQRScfhiklnorsvwx",
        values: "ABCgm",
        long: &[
            "after-context",
            "before-context",
            "break",
            "color",
            "colour",
            "column",
            "context",
            "count",
            "create-ackrc",
            "dump",
            "files-from",
            "files-with-matches",
            "files-without-matches",
            "filter",
            "flush",
            "follow",
            "group",
            "heading",
            "help",
            "help-types",
            "ignore-case",
            "ignore-dir",
            "ignore-directory",
            "ignore-file",
            "invert-match",
            "literal",
            "match",
            "max-count",
            "no-filename",
            "no-recurse",
            "nobreak",
            "nocolor",
            "nocolour",
            "nocolumn",
            "noenv",
            "nofilter",
            "nofollow",
            "nogroup",
            "noheading",
            "noignore-dir",
            "noignore-directory",
            "nopager",
            "nosmart-case",
            "passthru",
            "print0",
            "recurse",
            "show-types",
            "smart-case",
            "sort-files",
            "type",
            "version",
            "with-filename",
            "word-regexp",
        ],
    },
    SearchOptions {
        program: "ag",
        flags: "0FHLQSUacfilnostuvwz",
        values: "ABCGgmp",
        long: &[
            "ackmate",
            "after",
            "all-text",
            "all-types",
            "before",
            "break",
            "case-sensitive",
            "color",
            "column",
            "context",
            "count",
            "depth",
            "file-search-regex",
            "filename",
            "files-with-matches",
            "files-without-matches",
            "fixed-strings",
            "follow",
            "group",
            "heading",
            "help",
            "hidden",
            "ignore",
            "ignore-case",
            "ignore-dir",
            "invert-match",
            "list-file-types",
            "literal",
            "max-count",
            "nobreak",
            "nocolor",
            "nofilename",
            "nogroup",
            "noheading",
            "nonumbers",
            "nopager",
            "norecurse",
            "null",
            "numbers",
            "one-device",
            "only-matching",
            "path-to-ignore",
            "print0",
            "print-long-lines",
            "recurse",
            "search-binary",
            "search-zip",
            "silent",
            "skip-vcs-ignores",
            "smart-case",
            "stats",
            "unrestricted",
            "version",
            "vimgrep",
            "width",
            "word-regexp",
            "workers",
        ],
    },
    SearchOptions {
        program: "rg",
        flags: ".0FHILNPSUVabchilnopqsuvwx",
        values: "ABCEMTdefgjmrt",
        long: &[
            "after-context",
            "auto-hybrid-regex",
            "before-context",
            "binary",
            "block-buffered",
            "byte-offset",
            "case-sensitive",
            "color",
            "colors",
            "column",
            "context",
            "context-separator",
            "count",
            "count-matches",
            "crlf",
            "debug",
            "dfa-size-limit",
            "encoding",
            "engine",
            "field-context-separator",
            "field-match-separator",
            "file",
            "files",
            "files-with-matches",
            "files-without-match",
            "fixed-strings",
            "follow",
            "glob",
            "glob-case-insensitive",
            "heading",
            "help",
            "hidden",
            "hyperlink-format",
            "iglob",
            "ignore-case",
            "ignore-file",
            "ignore-file-case-insensitive",
            "include-zero",
            "invert-match",
            "json",
            "line-buffered",
            "line-number",
            "line-regexp",
            "max-columns",
            "max-columns-preview",
            "max-count",
            "max-depth",
            "max-filesize",
            "mmap",
            "multiline",
            "multiline-dotall",
            "no-config",
            "no-context-separator",
            "no-filename",
            "no-heading",
            "no-ignore",
            "no-ignore-dot",
            "no-ignore-exclude",
            "no-ignore-files",
            "no-ignore-global",
            "no-ignore-messages",
            "no-ignore-parent",
            "no-ignore-vcs",
            "no-line-number",
            "no-messages",
            "no-mmap",
            "no-pcre2-unicode",
            "no-require-git",
            "no-unicode",
            "null",
            "null-data",
            "one-file-system",
            "only-matching",
            "passthru",
            "path-separator",
            "pcre2",
            "pcre2-version",
            "pre-glob",
            "pretty",
            "quiet",
            "regex-size-limit",
            "regexp",
            "replace",
            "smart-case",
            "sort",
            "sort-files",
            "sortr",
            "stats",
            "stop-on-nonmatch",
            "text",
            "threads",
            "trace",
            "trim",
            "type",
            "type-add",
            "type-clear",
            "type-list",
            "type-not",
            "unrestricted",
            "version",
            "vimgrep",
            "with-filename",
            "word-regexp",
        ],
    },
];

/// The `gh` commands known to read, show or open data and run no program
/// but a pager, editor or browser, whose variables are judged: one word
/// (`api`) or two (`pr list`). Any other first words (an alias, an
/// extension, a command that changes gh's settings or checks out code)
/// leave the line judged on its raw text.
const GH_DATA_COMMANDS: &[&str] = &[
    "api",
    "auth status",
    "browse",
    "cache list",
    "gist clone",
    "gist list",
    "gist view",
    "help",
    "issue close",
    "issue comment",
    "issue create",
    "issue edit",
    "issue list",
    "issue reopen",
    "issue status",
    "issue view",
    "label list",
    "pr checks",
    "pr close",
    "pr comment",
    "pr create",
    "pr diff",
    "pr edit",
    "pr list",
    "pr ready",
    "pr reopen",
    "pr review",
    "pr status",
    "pr view",
    "release list",
    "release view",
    "repo clone",
    "repo fork",
    "repo list",
    "repo view",
    "run list",
    "run view",
    "run watch",
    "search code",
    "search commits",
    "search issues",
    "search prs",
    "search repos",
    "status",
    "workflow list",
    "workflow view",
];

/// The command variables a data program lists ([`COMMAND_VARIABLES`]) and
/// the startup check lists.
fn command_variables() -> impl Iterator<Item = &'static &'static str> {
    COMMAND_VARIABLES
        .iter()
        .flat_map(|(_, names)| names.iter())
        .chain(super::startup::COMMAND_VARIABLES)
}

/// Whether `name` is a variable this reader tracks: a command variable
/// ([`command_variables`]) or an options variable ([`OPTIONS_VARIABLES`]).
fn tracked(name: &str) -> bool {
    command_variables().any(|variable| *variable == name) || OPTIONS_VARIABLES.contains(&name)
}

/// Whether `program`, which may start another program, reads `variable`
/// as options or a settings file ([`OPTIONS_VARIABLES`]).
fn reads_options(program: &str, variable: &str) -> bool {
    !STARTS_NO_PROGRAM.contains(&program) && OPTIONS_VARIABLES.contains(&variable)
}

/// The values `command` assigns, each live for the whole line: assignments
/// alone, the assignments before its program (which a function or a
/// special builtin keeps), and the `NAME=value` operands written as they
/// stand of a declaration this shell runs.
fn line_values(command: &Simple) -> Vec<(String, String)> {
    let mut values = command.values.clone();
    match &command.shell {
        Shell::Assigns(assigns) => values.extend(assigns.iter().cloned()),
        Shell::Runs { words, written } if DECLARES.contains(&basename(&words[0]).as_str()) => {
            values.extend(
                words
                    .iter()
                    .zip(written)
                    .skip(1)
                    .filter(|(_, written)| **written)
                    .filter_map(|(word, _)| assigned(word)),
            );
        }
        _ => {}
    }
    values
}

/// Whether `command` may write a tracked variable ([`tracked`]) to a value
/// this reader does not see, or change what a later word means, which
/// leaves the line judged on its raw text. From the text of every word:
/// an append (`NAME+=`), a default expansion (`${NAME:=...}`), or the
/// variable of a `for` or `select` loop. From its shell view
/// ([`current_shell`]): a program word naming a tracked variable, a
/// builtin this reader does not model ([`UNMODELED`]), `set` outside its
/// admitted letters ([`set_admitted`]), and a builtin of [`WRITERS`]
/// outside its admitted form ([`admitted`]). Every other command writes
/// no tracked variable this reader does not see: a declaration's
/// `NAME=value` word is a value it sees ([`line_values`]).
fn unseen_write(command: &Simple) -> bool {
    let tracked_names = || {
        command_variables()
            .copied()
            .chain(OPTIONS_VARIABLES.iter().copied())
    };
    let words = &command.words;
    let in_text = words.iter().any(|word| {
        tracked_names().any(|name| {
            word.starts_with(&format!("{name}+="))
                || word.contains(&format!("${{{name}:="))
                || word.contains(&format!("${{{name}="))
        })
    }) || words.windows(2).any(|pair| {
        matches!(pair[0].as_str(), "for" | "select") && tracked_names().any(|name| pair[1] == name)
    });
    if in_text {
        return true;
    }
    let Shell::Runs { words, written } = &command.shell else {
        return false;
    };
    let program = basename(&words[0]);
    let (args, marks) = (&words[1..], &written[1..]);
    if tracked(&program) || UNMODELED.contains(&program.as_str()) {
        return true;
    }
    if program == "set" {
        return !set_admitted(args, marks);
    }
    WRITERS
        .iter()
        .find(|writer| writer.builtins.contains(&program.as_str()))
        .is_some_and(|writer| !admitted(writer, args, marks))
}

/// A [`WRITERS`] builtin's words, read by its option grammar.
struct Options<'a> {
    /// A letter outside the builtin's admitted letters.
    unknown: bool,
    /// A letter that only lists.
    listing: bool,
    /// The values of its name letters.
    names: Vec<&'a str>,
    /// The values of its letters that some shell reads as a name.
    either: Vec<&'a str>,
    /// The words after its options, with the shell's mark of each.
    operands: &'a [String],
    marks: &'a [bool],
}

/// `args` read by `writer`'s option grammar: options lead, each a `-`
/// word of letters whose value is the rest of the word or the next word,
/// up to `--` or the first other word. `None` when a word where an option
/// or its value stands is one the shell expands, which may be any option,
/// when a declaration carries a `+` option, or when a value is missing.
fn options<'a>(writer: &Writer, args: &'a [String], marks: &'a [bool]) -> Option<Options<'a>> {
    let written = |at: usize| marks.get(at).copied().unwrap_or(false);
    let mut read = Options {
        unknown: false,
        listing: false,
        names: Vec::new(),
        either: Vec::new(),
        operands: &[],
        marks: &[],
    };
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if !written(at) {
            return None;
        }
        if word == "--" {
            at += 1;
            break;
        }
        if writer.operands == Operands::Declarations && word.len() > 1 && word.starts_with('+') {
            return None;
        }
        let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) else {
            break;
        };
        at += 1;
        for (index, letter) in letters.char_indices() {
            let slot = if writer.name.contains(letter) {
                Some(&mut read.names)
            } else if writer.either.contains(letter) {
                Some(&mut read.either)
            } else if writer.data.contains(letter) {
                None
            } else {
                if writer.listing.contains(letter) {
                    read.listing = true;
                } else if !writer.flags.contains(letter) {
                    read.unknown = true;
                }
                continue;
            };
            let attached = &letters[index + letter.len_utf8()..];
            let value = if attached.is_empty() {
                let value = args.get(at).filter(|_| written(at))?;
                at += 1;
                value.as_str()
            } else {
                attached
            };
            if let Some(slot) = slot {
                slot.push(value);
            }
            break;
        }
    }
    read.operands = &args[at.min(args.len())..];
    read.marks = &marks[at.min(marks.len())..];
    Some(read)
}

/// Whether `word` is a plain identifier.
fn identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a [`WRITERS`] builtin given `args` is in its admitted form, so
/// it writes no tracked variable this reader does not see: every option
/// letter on its lists, a listing letter only with no operand, every name
/// it writes a plain identifier that is not tracked, a declaration's
/// operands each `NAME` or `NAME=value` with an identifier name, and every
/// word written as it stands, except the data after a data builtin's
/// options. A data builtin with no word holding its name letter writes no
/// name.
fn admitted(writer: &Writer, args: &[String], marks: &[bool]) -> bool {
    let Some(read) = options(writer, args, marks) else {
        return false;
    };
    if let Some(letter) = writer.writes_with {
        if !args
            .iter()
            .any(|word| word.starts_with('-') && word.contains(letter))
        {
            return true;
        }
    }
    if read.unknown || (read.listing && !read.operands.is_empty()) {
        return false;
    }
    let plain = |word: &str, number: bool| {
        (identifier(word)
            || (number && !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())))
            && !tracked(word)
    };
    if !read.names.iter().all(|word| plain(word, false))
        || !read.either.iter().all(|word| plain(word, true))
    {
        return false;
    }
    let all_written = !read.marks.contains(&false);
    match writer.operands {
        Operands::Declarations => {
            all_written
                && read
                    .operands
                    .iter()
                    .all(|word| assigned(word).is_some() || plain(word, false))
        }
        Operands::Names => all_written && read.operands.iter().all(|word| plain(word, false)),
        Operands::Data => true,
        Operands::Getopts => {
            all_written && read.operands.get(1).is_none_or(|word| plain(word, false))
        }
        Operands::Nothing => read.operands.is_empty(),
    }
}

/// Whether `set` given `args` is in its admitted form: options of
/// [`SET_FLAGS`] and `-o` or `+o` with a name of [`SET_OPTIONS`] or none,
/// each written as it stands, then the positional words, which are data.
/// Any other option, which may change which words set a name, leaves the
/// line judged on its raw text.
fn set_admitted(args: &[String], marks: &[bool]) -> bool {
    let written = |at: usize| marks.get(at).copied().unwrap_or(false);
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if !written(at) {
            return false;
        }
        if word == "--" || word == "-" {
            return true;
        }
        let Some(letters) = word.strip_prefix('-').or_else(|| word.strip_prefix('+')) else {
            return true;
        };
        at += 1;
        for (index, letter) in letters.char_indices() {
            if letter == 'o' {
                if index + 1 != letters.len() {
                    return false;
                }
                match args.get(at) {
                    None => return true,
                    Some(name) if written(at) && SET_OPTIONS.contains(&name.as_str()) => at += 1,
                    Some(_) => return false,
                }
            } else if !SET_FLAGS.contains(letter) {
                return false;
            }
        }
    }
    true
}

/// An assignment's value, for the command `program` that may read it. An
/// options variable it reads ([`reads_options`]) leaves the line judged on
/// its raw text. A value the program runs as a command is judged as an
/// option's command line is ([`command_line_value`]), so a command this
/// reader cannot resolve, such as a shell reading its input, leaves the
/// line judged on its raw text; a pager, editor, browser or preprocessor
/// value must also name an admitted viewer ([`fed_input`]). Any other
/// value is read for a run only.
fn assignment_value(program: &str, variable: &str, value: &str, depth: usize) -> Found {
    if reads_options(program, variable) {
        Found::Unresolved
    } else if runs_variable(program, variable) && FED_VARIABLES.contains(&variable) {
        fed_input(value, depth)
    } else if runs_variable(program, variable) {
        command_line_value(value, depth)
    } else {
        in_value(value, depth).map_or(Found::Nothing, Found::Run)
    }
}

/// The one judgement of a program another program hands its output, a
/// file or an address to read: a pager, an editor, a browser or a
/// preprocessor. The value is judged as [`command_line_value`] judges
/// it, and is unresolved unless it is empty or each of its commands is
/// a viewer written as it stands, with no assignment before it, that
/// [`viewer_admits`]. Its callers: [`assignment_value`] for
/// [`FED_VARIABLES`], [`runs_through_option`] for a search program's
/// pager, [`program_path_value`] for its preprocessor, and
/// [`unresolved_fed_command`] for the pagers, editors, browsers and
/// converters Git runs.
fn fed_input(value: &str, depth: usize) -> Found {
    match command_line_value(value, depth) {
        Found::Nothing if value.trim().is_empty() => Found::Nothing,
        Found::Nothing => {
            let commands = simple(value);
            if !commands.is_empty() && commands.iter().all(views_only) {
                Found::Nothing
            } else {
                Found::Unresolved
            }
        }
        found => found,
    }
}

/// Whether `command` is an admitted viewer run as written ([`fed_input`]).
fn views_only(command: &Simple) -> bool {
    command.values.is_empty()
        && matches!(&command.shell, Shell::Runs { written, .. } if !written.contains(&false))
        && viewer_admits(&command.argv)
}

/// Whether `argv` runs a [`VIEWERS`] program with only the options it is
/// admitted with and no operand other than `-`.
fn viewer_admits(argv: &[String]) -> bool {
    let Some(viewer) = argv.first().and_then(|program| {
        let name = basename(program);
        VIEWERS
            .iter()
            .find(|viewer| viewer.names.contains(&name.as_str()))
    }) else {
        return false;
    };
    let mut words = argv[1..].iter();
    while let Some(word) = words.next() {
        let admitted = if word == "-" {
            true
        } else if let Some(long) = word.strip_prefix("--") {
            let takes_value = |name: &str| {
                viewer
                    .long_values
                    .iter()
                    .any(|option| option.strip_prefix("--") == Some(name))
            };
            match long.split_once('=') {
                Some((name, _)) => takes_value(name),
                None if viewer.long.contains(&word.as_str()) => true,
                None => takes_value(long) && words.next().is_some(),
            }
        } else if let Some(letters) = word.strip_prefix('-') {
            short_options_admitted(viewer, letters, &mut words)
        } else {
            false
        };
        if !admitted {
            return false;
        }
    }
    true
}

/// Whether a viewer's cluster of option letters is admitted: each a flag,
/// up to a letter that takes a value, attached or the next word.
fn short_options_admitted<'a>(
    viewer: &Viewer,
    letters: &str,
    words: &mut impl Iterator<Item = &'a String>,
) -> bool {
    for (at, letter) in letters.char_indices() {
        if viewer.values.contains(letter) {
            return at + letter.len_utf8() < letters.len() || words.next().is_some();
        }
        if !viewer.flags.contains(letter) {
            return false;
        }
    }
    true
}

/// A run inside a quoted argument or an assignment's value, read as a
/// command line. A value whose first word holds an `=`, such as an
/// option's `exec=<command>`, is also read from after that `=`.
fn in_value(value: &str, depth: usize) -> Option<InstallRun> {
    find(value, depth + 1)
        .or_else(|| {
            let (head, tail) = value.split_once('=')?;
            if head.contains(shell_blank) {
                None
            } else {
                in_value(tail, depth + 1)
            }
        })
        .map(|run| InstallRun {
            in_argument: true,
            ..run
        })
}

/// Judge one simple command's argument vector, with whether the shell
/// passes each word as written.
fn classify(argv: &[String], literal: &[bool], depth: usize) -> Found {
    let Some(program) = argv.first() else {
        return Found::Nothing;
    };
    let written = |at: usize| literal.get(at).copied().unwrap_or(false) && !unresolved(&argv[at]);
    let name = basename(program);
    if DATA_PROGRAMS.contains(&name.as_str()) {
        if name == "gh" {
            if let Some(run) = gh_git_arguments(argv, depth) {
                return Found::Run(run);
            }
            if !gh_reads_only(argv, &written) {
                return Found::Unresolved;
            }
        }
        let found = runs_through_option(argv, &written, depth);
        let known = SEARCH_OPTIONS
            .iter()
            .find(|options| options.program == name);
        if matches!(found, Found::Nothing)
            && known.is_some_and(|options| !search_options_known(options, argv, &written))
        {
            return Found::Unresolved;
        }
        return found;
    }
    if EDITORS.contains(&name.as_str()) {
        if let Some(run) = editor_command(argv) {
            return Found::Run(run);
        }
    }
    if name == BINARY
        && written(0)
        && first_operand(&argv[1..]).is_some_and(|operand| RECORD_VERBS.contains(&operand.as_str()))
        && argv[1..]
            .iter()
            .position(|word| word != "--")
            .is_some_and(|at| written(at + 1))
    {
        return Found::Nothing;
    }
    let known = NOT_LAUNCHERS.contains(&name.as_str());
    let runner_from = package_manager_runs_from(argv, &written);
    let mut found = Found::Nothing;
    for (at, word) in argv.iter().enumerate() {
        let after = &argv[at + 1..];
        let before = &argv[..at];
        let in_program_position =
            at == 0 || starts_a_command(before) || runner_from.is_some_and(|from| at >= from);
        if in_program_position
            && (!written(at)
                || matches!(
                    basename(word).as_str(),
                    "alias" | "function" | "source" | "."
                )
                || (SHELLS.contains(&basename(word).as_str())
                    && shell_command_string(after).is_none()))
        {
            found = Found::Unresolved;
        }
        let may_run = in_program_position || !known;
        let open = |at: usize| is_verb(&argv[at]) || !written(at) || supplier_slot(&argv[at]);
        let run = match program_word(word, written(at)).as_str() {
            BINARY if may_run && runs_a_verb(at + 1, argv, &open, supplied(before)) => {
                Some(binary_form(after))
            }
            "cargo" if may_run && cargo_runs_a_verb(at + 1, argv, &open) => Some("cargo run"),
            _ if (word == "{}" || !written(at))
                && in_program_position
                && operand_at(at + 1, argv).is_some_and(|operand| is_verb(&argv[operand])) =>
            {
                Some("an unresolved program with an install verb")
            }
            _ => None,
        };
        if let Some(form) = run {
            return Found::Run(InstallRun {
                form,
                parsed: true,
                in_argument: false,
            });
        }
        // A word the shell expands where a program this reader does not
        // know may run it: judged with the words after it on their raw
        // text, as a literal name there would be.
        if may_run
            && !in_program_position
            && !written(at)
            && raw_run(&argv[at..].join(" ")).is_some()
        {
            return Found::Run(InstallRun {
                form: "an expanded word naming the binary with an install verb",
                parsed: true,
                in_argument: false,
            });
        }
        if at > 0 && word.contains(shell_blank) {
            if let Some(run) = in_value(word, depth) {
                return Found::Run(run);
            }
        }
    }
    found
}

/// A search program run with an option that runs a program
/// ([`RUNS_THROUGH_OPTION`]): the option's value is judged as the program
/// runs it, and refused when the shell expands it. A value this reader
/// cannot resolve, or one a shell reading its input runs, leaves the line
/// judged on its raw text. The program's other words are data.
fn runs_through_option(argv: &[String], written: &dyn Fn(usize) -> bool, depth: usize) -> Found {
    let mut found = Found::Nothing;
    for (at, word) in argv.iter().enumerate().skip(1) {
        let Some(name) = word.strip_prefix("--") else {
            continue;
        };
        let (name, attached) = match name.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (name, None),
        };
        if name.is_empty() {
            continue;
        }
        for &(option, runs) in RUNS_THROUGH_OPTION {
            if !option[2..].starts_with(name) {
                continue;
            }
            let (value, value_at) = match (attached, argv.get(at + 1)) {
                (Some(value), _) => (value, at),
                (None, Some(value)) => (value.as_str(), at + 1),
                (None, None) => continue,
            };
            if !written(value_at) {
                return Found::Run(InstallRun {
                    form: "codeflow with a supplied verb",
                    parsed: true,
                    in_argument: false,
                });
            }
            let judged = match runs {
                OptionValue::CommandLine => fed_input(value, depth),
                OptionValue::ProgramPath => {
                    program_path_value(value, argv, value_at + 1, written, depth)
                }
            };
            match judged {
                Found::Run(run) => return Found::Run(run),
                Found::Unresolved => found = Found::Unresolved,
                Found::Nothing => {}
            }
        }
    }
    found
}

/// The words after `--` that a [`GH_GIT_COMMANDS`] command passes to the
/// `git clone` it runs: each one holding a blank, such as a `-c` setting or
/// an `--upload-pack` value, is read as a quoted argument is. The command
/// is the first two words of `argv` after the program that are not
/// options. Any other gh command's words after `--` are its data, such as
/// a search query.
fn gh_git_arguments(argv: &[String], depth: usize) -> Option<InstallRun> {
    let start = argv.iter().position(|word| word == "--")?;
    let mut words = argv[1..start].iter().filter(|word| !word.starts_with('-'));
    let command = format!("{} {}", words.next()?, words.next()?);
    if !GH_GIT_COMMANDS.contains(&command.as_str()) {
        return None;
    }
    argv[start + 1..]
        .iter()
        .filter(|word| word.contains(shell_blank))
        .find_map(|word| in_value(word, depth))
}

/// Whether a `gh` call is one of [`GH_DATA_COMMANDS`], read from its
/// first words that are not options before any `--`, and passes no word
/// the shell expands to the `git` a [`GH_GIT_COMMANDS`] command runs. A
/// call with no command only prints help or the version.
fn gh_reads_only(argv: &[String], written: &dyn Fn(usize) -> bool) -> bool {
    let end = argv
        .iter()
        .position(|word| word == "--")
        .unwrap_or(argv.len());
    let mut words = argv[1..end].iter().filter(|word| !word.starts_with('-'));
    let Some(first) = words.next() else {
        return true;
    };
    let command = match words.next() {
        Some(second) if !GH_DATA_COMMANDS.contains(&first.as_str()) => {
            format!("{first} {second}")
        }
        _ => first.clone(),
    };
    GH_DATA_COMMANDS.contains(&command.as_str())
        && (!GH_GIT_COMMANDS.contains(&command.as_str()) || (end..argv.len()).all(written))
}

/// Whether every option of a search program's call is one of `options`
/// or [`RUNS_THROUGH_OPTION`], up to `--`, and no word before `--` is one
/// the shell expands, which may be any option.
fn search_options_known(
    options: &SearchOptions,
    argv: &[String],
    written: &dyn Fn(usize) -> bool,
) -> bool {
    for (at, word) in argv.iter().enumerate().skip(1) {
        if word == "--" {
            return true;
        }
        if !written(at) {
            return false;
        }
        if let Some(long) = word.strip_prefix("--") {
            let long = long.split_once('=').map_or(long, |(name, _)| name);
            if !options.long.contains(&long)
                && !RUNS_THROUGH_OPTION
                    .iter()
                    .any(|(option, _)| option[2..].starts_with(long))
            {
                return false;
            }
        } else if let Some(letters) = word.strip_prefix('-') {
            for letter in letters.chars() {
                if options.values.contains(letter) {
                    break;
                }
                if !options.flags.contains(letter) {
                    return false;
                }
            }
        }
    }
    true
}

/// A command line an option's value holds, read with the recursive
/// reader. A value whose quotes are left open, or whose command this
/// reader cannot resolve (a shell reading its input, which for a pager is
/// the program's output), is unresolved.
fn command_line_value(value: &str, depth: usize) -> Found {
    if let Some(run) = in_value(value, depth) {
        return Found::Run(run);
    }
    if quotes_open(value) || matches!(read(value, depth + 1), Found::Unresolved) {
        Found::Unresolved
    } else {
        Found::Nothing
    }
}

/// A program path an option's value names, run with each word of `argv`
/// from `from` on that is not an option as its argument and the searched
/// file on its input, so a supplier there adds words of its own. A value
/// naming the binary refuses whatever it is given. The value is judged as
/// a preprocessor by [`fed_input`], so it must name an admitted viewer; a
/// value holding a blank names no program the search program can find and
/// fails closed there.
fn program_path_value(
    value: &str,
    argv: &[String],
    from: usize,
    written: &dyn Fn(usize) -> bool,
    depth: usize,
) -> Found {
    let first = value
        .split(shell_blank)
        .find(|part| !part.is_empty())
        .unwrap_or(value);
    if basename(first) == BINARY {
        return Found::Run(InstallRun {
            form: "codeflow with a supplied verb",
            parsed: true,
            in_argument: false,
        });
    }
    let mut found = fed_input(value, depth);
    if matches!(found, Found::Run(_)) {
        return found;
    }
    for (at, operand) in argv.iter().enumerate().skip(from) {
        if operand.starts_with('-') || operand.contains(shell_blank) {
            continue;
        }
        let words = [value.to_string(), operand.clone()];
        let marks = [true, written(at)];
        match classify(&words, &marks, depth + 1) {
            Found::Run(run) => return Found::Run(run),
            Found::Unresolved => found = Found::Unresolved,
            Found::Nothing => {}
        }
    }
    found
}

/// Whether `text` ends inside a quote or after a lone backslash, so a shell
/// could not read it as a command line.
fn quotes_open(text: &str) -> bool {
    let mut quote = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => {}
            (_, '\\') => {
                if chars.next().is_none() {
                    return true;
                }
            }
            (None, '\'' | '"') => quote = Some(c),
            _ => {}
        }
    }
    quote.is_some()
}

/// A command in an editor's own language ([`EDITORS`]) that names the
/// binary with a verb: an [`EDITOR_COMMAND_OPTIONS`] value, attached after
/// `=` or as the next word, or a `+` word, judged on its raw text.
fn editor_command(argv: &[String]) -> Option<InstallRun> {
    argv.iter().enumerate().skip(1).find_map(|(at, word)| {
        let command =
            if word.starts_with('+') || EDITOR_COMMAND_OPTIONS.contains(&argv[at - 1].as_str()) {
                Some(word.as_str())
            } else {
                word.split_once('=')
                    .filter(|(option, _)| EDITOR_COMMAND_OPTIONS.contains(option))
                    .map(|(_, value)| value)
            };
        command.and_then(raw_run).map(|run| InstallRun {
            in_argument: true,
            ..run
        })
    })
}

/// The name a program word runs as: its base name, with zsh's leading `=`
/// removed from a word the shell expands.
fn program_word(word: &str, written: bool) -> String {
    if written {
        basename(word)
    } else {
        basename(word.strip_prefix('=').unwrap_or(word))
    }
}

/// Whether the words of `argv` from `from` on, given to the binary, run a
/// verb. `supplied` is true when a supplier before it adds words of its own
/// after them. `open` marks a word that is a verb or may become one.
fn runs_a_verb(from: usize, argv: &[String], open: &dyn Fn(usize) -> bool, supplied: bool) -> bool {
    let start = operand_at(from, argv).unwrap_or(argv.len());
    let rest = &argv[start..];
    if !supplied
        && PRINTS_ONLY
            .iter()
            .any(|shape| shape.len() == rest.len() && shape.iter().zip(rest).all(|(a, b)| a == b))
    {
        return false;
    }
    match rest.first() {
        None => supplied,
        Some(_) if open(start) => true,
        Some(first) if first.starts_with('-') => (start + 1..argv.len()).any(open),
        Some(_) => supplied && (start..argv.len()).any(open),
    }
}

/// `cargo run` or `cargo r`, read from `from` in `argv`: true when a word
/// cargo passes to the program it builds is a verb or may become one. When
/// the program is the binary, by `--bin codeflow` or its package, the
/// passed words are judged as the binary's own. A subcommand the reader
/// cannot resolve is judged as `run`.
fn cargo_runs_a_verb(from: usize, argv: &[String], open: &dyn Fn(usize) -> bool) -> bool {
    let mut at = from;
    // The subcommand: the first word that is not an option, an option's
    // value or a toolchain.
    let subcommand = loop {
        let Some(word) = argv.get(at) else {
            return false;
        };
        if CARGO_VALUE_OPTIONS.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') || word.starts_with('+') {
            at += 1;
        } else {
            break at;
        }
    };
    let resolved = !open(subcommand) || is_verb(&argv[subcommand]);
    if resolved && !matches!(argv[subcommand].as_str(), "run" | "r") {
        return false;
    }
    let mut passed = Vec::new();
    let mut binary = false;
    let mut at = subcommand + 1;
    while let Some(word) = argv.get(at) {
        if word == "--" {
            passed.extend(at + 1..argv.len());
            break;
        }
        if CARGO_VALUE_OPTIONS.contains(&word.as_str()) {
            if let Some(value) = argv.get(at + 1) {
                binary |= matches!(word.as_str(), "--bin" | "-p" | "--package")
                    && matches!(value.as_str(), "codeflow" | "codeflow-cli");
            }
            at += 2;
            continue;
        }
        if word.starts_with('-') {
            binary |= matches!(
                word.as_str(),
                "--bin=codeflow" | "--package=codeflow-cli" | "-pcodeflow-cli"
            );
        } else {
            passed.push(at);
        }
        at += 1;
    }
    if !resolved {
        return passed.iter().any(|&at| open(at)) || (subcommand + 1..argv.len()).any(open);
    }
    if binary {
        let words: Vec<String> = passed.iter().map(|&at| argv[at].clone()).collect();
        let marks: Vec<bool> = passed.iter().map(|&at| open(at)).collect();
        let open_passed = |at: usize| marks[at];
        return runs_a_verb(0, &words, &open_passed, false);
    }
    passed.iter().any(|&at| open(at))
}

/// The raw-text form of [`cargo_runs_a_verb`] and the binary's run: the
/// verb anywhere among `words`.
fn names_a_verb(words: &[&str]) -> bool {
    words.iter().any(|word| is_verb(word))
}

/// The form reported for a run of the binary.
fn binary_form(after: &[String]) -> &'static str {
    match first_operand(after).map(String::as_str) {
        Some("update") => "codeflow update",
        Some("init") => "codeflow init",
        _ => "codeflow with an unresolved or supplied verb",
    }
}

/// The index of the first word of `argv` from `from` on after any leading
/// `--`.
fn operand_at(from: usize, argv: &[String]) -> Option<usize> {
    (from..argv.len()).find(|&at| argv[at] != "--")
}

/// The first word after any leading `--`.
fn first_operand(words: &[String]) -> Option<&String> {
    words.iter().find(|word| *word != "--")
}

fn is_verb(word: &str) -> bool {
    VERBS.contains(&word)
}

/// A word whose value the guard cannot read: a variable, a substitution,
/// which git-guard's reader marks with a control character, or a backtick.
fn unresolved(word: &str) -> bool {
    word.contains(['$', '`']) || word.chars().any(char::is_control)
}

/// A word a supplier fills in (`{}`) or a separator before its own words
/// (`:::`).
fn supplier_slot(word: &str) -> bool {
    word.contains("{}") || word.starts_with(":::")
}

/// Whether a supplier among `before` adds words after the command it runs.
fn supplied(before: &[String]) -> bool {
    before
        .iter()
        .any(|word| APPENDING.contains(&basename(word).as_str()))
}

/// The index from which every word of `argv` is in a program position
/// because a package manager in a program position before it runs a
/// binary by name: one of [`PACKAGE_MANAGERS`], or `npm` with any command
/// other than [`NPM_OWN_COMMANDS`].
fn package_manager_runs_from(argv: &[String], written: &dyn Fn(usize) -> bool) -> Option<usize> {
    (0..argv.len()).find_map(|at| {
        if at > 0 && !starts_a_command(&argv[..at]) {
            return None;
        }
        let name = basename(&argv[at]);
        let runs = PACKAGE_MANAGERS.contains(&name.as_str())
            || (name == "npm"
                && !npm_runs_its_own_command(&argv[at + 1..], &|word| written(at + 1 + word)));
        runs.then_some(at + 1)
    })
}

/// Whether npm, given `args`, runs one of [`NPM_OWN_COMMANDS`]: its command
/// is the first word after its options, as npm's option parser leaves it
/// (the first word of `remain`). Fail closed: an option this reader does
/// not know, a value that starts with `-`, or a word the shell expands
/// before the command means the command is not established.
fn npm_runs_its_own_command(args: &[String], written: &dyn Fn(usize) -> bool) -> bool {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if !written(at) {
            return false;
        }
        if word == "--" {
            return written(at + 1)
                && args
                    .get(at + 1)
                    .is_some_and(|command| NPM_OWN_COMMANDS.contains(&command.as_str()));
        }
        if NPM_VALUE_OPTIONS.contains(&word.as_str()) {
            match args.get(at + 1) {
                Some(value) if written(at + 1) && !value.starts_with('-') => at += 2,
                _ => return false,
            }
        } else if NPM_FLAGS.contains(&word.as_str())
            || (word.starts_with("--") && word.contains('='))
        {
            at += 1;
        } else {
            return !word.starts_with('-') && NPM_OWN_COMMANDS.contains(&word.as_str());
        }
    }
    false
}

/// Whether the word after `before` is in a program position: the first
/// word, or after a launcher, supplier or package runner.
fn starts_a_command(before: &[String]) -> bool {
    before.is_empty()
        || before.iter().any(|word| {
            let name = basename(word);
            LAUNCHERS.contains(&name.as_str())
                || SUPPLIERS.contains(&name.as_str())
                || PACKAGE_RUNNERS.contains(&name.as_str())
                || STARTS_A_COMMAND.contains(&word.as_str())
        })
}

/// Whether `command`, a program another program runs (such as a Git pager
/// or editor setting), is a command line this reader cannot resolve, such
/// as a shell reading its input. The line that starts it is then judged on
/// its raw text.
#[must_use]
pub(crate) fn unresolved_command(command: &str) -> bool {
    matches!(command_line_value(command, 0), Found::Unresolved)
}

/// Whether `command`, a program another program hands its output, a file
/// or an address to read (such as a Git pager, editor or text converter
/// setting), is not an admitted viewer ([`fed_input`]). The line that
/// starts it is then judged on its raw text.
#[must_use]
pub(crate) fn unresolved_fed_command(command: &str) -> bool {
    matches!(fed_input(command, 0), Found::Unresolved)
}

/// The raw-text judgement for a line that could not be resolved, and for
/// interpreter code: the binary's name followed, anywhere later, by a
/// verb, or `cargo` followed by `run` or `r` and then a verb.
#[must_use]
pub(crate) fn raw_run(text: &str) -> Option<InstallRun> {
    let plain: String = text
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"'))
        .collect();
    let words: Vec<&str> = plain
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '+' | '-')))
        .filter(|w| !w.is_empty())
        .collect();
    let verb_after = |at: usize| names_a_verb(&words[at..]);
    words.iter().enumerate().find_map(|(at, word)| {
        let form = match basename(word).as_str() {
            BINARY if verb_after(at + 1) => "codeflow update or init",
            "cargo" => {
                let run = words[at + 1..]
                    .iter()
                    .position(|w| *w == "run" || *w == "r")?;
                verb_after(at + run + 2).then_some("cargo run")?
            }
            _ => return None,
        };
        Some(InstallRun {
            form,
            parsed: false,
            in_argument: false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(command: &str) -> bool {
        install_run(command, true).is_some()
    }

    #[test]
    fn a_run_of_either_verb_is_found_in_each_form() {
        for command in [
            "codeflow update",
            "codeflow init --yes",
            "./target/debug/codeflow update --force",
            "/usr/local/bin/codeflow update --pin 4.0.0",
            "CODEFLOW update",
            "codeflow.exe init",
            "nice -n 5 codeflow update",
            "sudo -u root codeflow update",
            "doas codeflow update",
            "chrt -o 0 codeflow update",
            "taskset -c 0 codeflow update",
            "watch -n 60 codeflow update",
            "watch 'codeflow update'",
            "parallel codeflow ::: update",
            "echo update | xargs codeflow",
            "echo update | xargs -I{} codeflow {}",
            "find . -name codeflow -exec {} update ;",
            "busybox env codeflow update",
            "gsudo codeflow update",
            "pkexec codeflow init",
            "npx codeflow update",
            "npm exec -- codeflow update",
            "pnpm exec codeflow update",
            "yarn exec codeflow update",
            "bun x codeflow update",
            "pn exec codeflow update",
            "sh -c 'codeflow update'",
            "eval 'codeflow init --yes'",
            "true; codeflow update",
            "cd /tmp && codeflow init --yes",
            "cargo run -- update",
            "cargo run update",
            "cargo r init --yes",
            "cargo +stable run --bin codeflow -- init",
            "cargo run -p codeflow-cli -- update --force",
            "B=./codeflow; $B update",
            "$(printf codeflow) update",
            "codeflow $V",
            "codeflow \"$@\"",
            "codeflow update --diff=--help",
            "codeflow update --help --force",
            "codeflow init -h --yes",
            "codeflow -- update",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// Words the shell expands without a `$` or a backtick: braces,
    /// patterns and a leading `=`, in the program position, as the first
    /// operand and among the words cargo passes on.
    #[test]
    fn words_the_shell_expands_are_read_as_unresolved() {
        for command in [
            "codeflow {update,--force}",
            "codeflow upd?te",
            "codeflow [u]pdate",
            "codeflow upd*",
            "{codeflow,} update",
            "{./target/debug/codeflow,update}",
            "./target/debug/codeflo? update",
            "=codeflow update",
            "sudo codeflow {update,--force}",
            "cargo run -- $V",
            "cargo run -- \"$@\"",
            "cargo run -- {update,--force}",
            "cargo run -- upd?te",
            "cargo r {init,--yes}",
            "cargo $SUB update",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A shell that reads its input behind a launcher the reader does not
    /// strip has the line's text judged; an assignment's value is read when
    /// a command can read it; a search program's option that runs a
    /// program is judged.
    #[test]
    fn hidden_runs_behind_launchers_values_and_options_are_found() {
        for command in [
            "printf 'codeflow update' | chrt -o 0 sh",
            "echo codeflow init | taskset -c 0 bash",
            "PAGER='codeflow update' man ls",
            "PAGER='codeflow init --yes'; man ls",
            "export PAGER='codeflow update'; man ls",
            "env PAGER='./target/debug/codeflow update' man ls",
            "NOTE='codeflow update'; $NOTE",
            "rg --pre ./target/debug/codeflow x update",
            "rg --pre=codeflow pattern init",
            "ack --pager='codeflow update' x",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A search program's option whose value is a command line it runs is
    /// judged as that command: behind each launcher, a path spelling, a
    /// shell or a supplier, with the value attached or as the next word,
    /// and under an abbreviated option name. A preprocessor value is one
    /// program, run with each searched path and the file on its input. A
    /// shell reading its input as the pager has the line's text judged.
    /// Ordinary pagers and preprocessors stay allowed.
    #[test]
    fn command_valued_options_are_judged_as_the_command_they_run() {
        for launcher in [
            "env",
            "nice",
            "command",
            "/usr/bin/env",
            "timeout 5",
            "sudo -u root",
            "busybox env",
            "env -i PATH=/usr/bin",
        ] {
            for option in [
                "ack --pager=",
                "ack --pager ",
                "ack --pag=",
                "ag --pager=",
                "ag --pager ",
                "ag --pag=",
            ] {
                for run in ["codeflow update", "./target/debug/codeflow init --yes"] {
                    let command = format!("{option}'{launcher} {run}' pattern README.md");
                    assert!(runs(&command), "{command}");
                }
            }
        }
        for command in [
            "ack --pager='sh -c \"codeflow update\"' pattern README.md",
            "ack --pager='env codefl?w update' pattern README.md",
            "ack --pager='xargs codeflow' pattern README.md",
            "ack --pager=sh -h 'codeflow update' notes.txt",
            "ack --pager='bash -s' -h 'codeflow init --yes' notes.txt",
            "ag --pager=sh 'codeflow update' notes.txt",
            "ack --pager='less -R \"x' 'codeflow update' notes.txt",
            "rg --pre xargs pattern codeflow",
            "rg --pre=parallel pattern codeflow",
            "rg --pre 'env codeflow update' pattern docs",
            "rg --pre='nice codeflow init' pattern docs",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "ack --pager=less pattern README.md",
            "ack --pager='less -R' pattern README.md",
            "ack --pager 'less -FRX' pattern README.md",
            "ack --pager='env LESS=-R less' pattern README.md",
            "ack --nopager 'codeflow update' docs",
            "ack --pager=less 'codeflow update' docs",
            "ag --pager=less pattern README.md",
            "ag --pager 'less -R' 'codeflow init' docs",
            "ag --pager=more 'codeflow init' docs",
            "rg --pre ./tools/pdftext pattern docs",
            "rg --pre=pdftotext 'codeflow update' docs",
            "rg --pre env pattern codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A pager, editor or browser variable assigned earlier on the line is
    /// read when a search or forge program that runs it follows; another
    /// variable stays data for them.
    #[test]
    fn command_variables_a_data_program_runs_are_judged() {
        for command in [
            "export ACK_PAGER='env codeflow update'; ack pattern README.md",
            "export ACK_PAGER_COLOR='nice codeflow update'; ack pattern README.md",
            "ACK_PAGER='env codeflow init'; ack pattern README.md",
            "export PAGER='env codeflow update'; ack --pager pattern README.md",
            "export GH_PAGER='env codeflow update'; gh pr list",
            "export GH_EDITOR='codeflow update'; gh pr create",
            "export BROWSER='nice codeflow init'; gh browse",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "export ACK_PAGER='less -R'; ack pattern README.md",
            "export GH_PAGER='less -R'; gh pr list",
            "NOTE='codeflow update'; ack -- \"$NOTE\" docs",
            "export NOTE='codeflow update'; gh pr list",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A pager, editor or browser variable naming a shell that reads its
    /// input gives that shell the program's output, so the line is judged
    /// on its raw text, as the same option is: set before the program or
    /// earlier on the line, for each variable a data program runs and for
    /// one a program the reader does not know runs. A variable no program
    /// runs, and an ordinary pager, stay data.
    #[test]
    fn a_command_variable_naming_a_stdin_shell_judges_the_line() {
        for (variables, line) in [
            (
                &["ACK_PAGER", "ACK_PAGER_COLOR", "PAGER"][..],
                "ack --nocolor -h 'codeflow update' notes.txt",
            ),
            (
                &[
                    "GH_PAGER",
                    "PAGER",
                    "GH_EDITOR",
                    "GIT_EDITOR",
                    "EDITOR",
                    "VISUAL",
                    "GH_BROWSER",
                    "BROWSER",
                ][..],
                "gh search issues 'codeflow update'",
            ),
            (&["PAGER", "MANPAGER"][..], "man -k 'codeflow init'"),
        ] {
            for variable in variables {
                for form in [
                    format!("{variable}=sh {line}"),
                    format!("{variable}='bash -s' {line}"),
                    format!("export {variable}=sh; {line}"),
                    format!("{variable}=zsh; {line}"),
                ] {
                    assert!(runs(&form), "{form}");
                }
            }
        }
        for command in [
            "ACK_PAGER=less ack 'codeflow update' docs",
            "export ACK_PAGER='less -R'; ack 'codeflow update' docs",
            "PAGER='less -R' gh search issues 'codeflow update'",
            "GH_PAGER=cat gh search issues 'codeflow update'",
            "NOTE=sh; ack 'codeflow update' docs",
            "NOTE=sh codeflow task new --title 'codeflow update docs'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A command variable set on the line stays live across every other
    /// command: the program that reads it, or one that can start such a
    /// program, judges it, whatever `unset` or new value the line holds.
    #[test]
    fn a_command_variable_stays_live_across_other_commands() {
        for (assignments, line) in [
            (
                &["ACK_PAGER", "ACK_PAGER_COLOR", "PAGER"][..],
                "ack --nocolor --nogroup -h 'codeflow update' notes.txt",
            ),
            (
                &[
                    "GH_PAGER",
                    "PAGER",
                    "GH_EDITOR",
                    "GIT_EDITOR",
                    "EDITOR",
                    "VISUAL",
                    "GH_BROWSER",
                    "BROWSER",
                ][..],
                "gh search issues 'codeflow update'",
            ),
            (
                &["GIT_PAGER", "PAGER"][..],
                "git log -1 --format='codeflow update'",
            ),
        ] {
            for variable in assignments {
                for form in [
                    format!("export {variable}=sh; true; {line}"),
                    format!("{variable}=sh; true; {line}"),
                    format!("export {variable}='bash -s' && cd docs && {line}"),
                    format!("export {variable}=sh; false || {line}"),
                    format!("export {variable}=sh; echo x | wc -l; {line}"),
                    format!("export {variable}=sh; (true); {line}"),
                    format!("export {variable}=sh; f() {{ true; }}; f; {line}"),
                    format!("export {variable}=sh; unset NOTE; {line}"),
                    format!("export {variable}=sh; export NOTE=1; {line}"),
                    format!("export {variable}=sh; sh -c \"{line}\""),
                    format!("export {variable}=sh; bash -c \"true; {line}\""),
                    format!("{variable}=sh sh -c \"{line}\""),
                    format!("eval 'export {variable}=sh'; {line}"),
                ] {
                    assert!(runs(&form), "{form}");
                }
            }
        }
        // A program that starts others passes the variable on.
        for command in [
            "export ACK_PAGER=sh; git -c alias.s='!ack -h \"codeflow update\" notes.txt' s",
            "export ACK_PAGER=sh; find . -name notes.txt -exec ack -h 'codeflow update' {} +",
            "export GH_PAGER=sh; echo 'codeflow update' | xargs gh search issues",
            "export ACK_PAGER=sh; unset ACK_PAGER; ack 'codeflow update' docs",
            "export ACK_PAGER=sh; unset -v ACK_PAGER; true; ack 'codeflow update' docs",
            "export GH_PAGER=sh; unset GH_PAGER; gh search issues 'codeflow update'",
            "export ACK_PAGER=sh; ACK_PAGER=less; true; ack 'codeflow update' docs",
            "export GH_PAGER=sh; export GH_PAGER='less -R'; true; gh search issues 'codeflow update'",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "export ACK_PAGER=less; true; ack 'codeflow update' docs",
            "export GH_PAGER='less -R'; cd docs && gh search issues 'codeflow update'",
            "export NOTE=sh; true; ack 'codeflow update' docs",
            "NOTE=sh; true; gh search issues 'codeflow update'",
            "export PAGER=less; true; git log -1 --format='codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// No `unset` or new value clears a command variable: in the same
    /// shell, a subshell, a pipeline, a conditional or loop body, a
    /// function body, a substitution, the background or another shell's
    /// body, the old value stays live.
    #[test]
    fn a_command_variable_is_never_cleared_on_the_line() {
        for (variable, line) in [
            (
                "ACK_PAGER",
                "ack --nocolor --nogroup -h 'codeflow update' notes.txt",
            ),
            ("GH_PAGER", "gh search issues 'codeflow update'"),
        ] {
            for update in [format!("unset {variable}"), format!("{variable}=less")] {
                for form in [
                    format!("export {variable}=sh; ({update}); {line}"),
                    format!("export {variable}=sh; {update} | true; {line}"),
                    format!("export {variable}=sh; true | {update}; {line}"),
                    format!("export {variable}=sh; false && {update}; {line}"),
                    format!("export {variable}=sh; true || {update}; {line}"),
                    format!("export {variable}=sh; {update} & {line}"),
                    format!("export {variable}=sh; if false; then {update}; fi; {line}"),
                    format!("export {variable}=sh; while false; do {update}; done; {line}"),
                    format!("export {variable}=sh; until true; do {update}; done; {line}"),
                    format!("export {variable}=sh; case x in y) {update};; esac; {line}"),
                    format!("export {variable}=sh; f() {{ {update}; }}; {line}"),
                    format!("export {variable}=sh; function f {{ {update}; }}; {line}"),
                    format!("export {variable}=sh; echo \"$({update})\"; {line}"),
                    format!("export {variable}=sh; echo `{update}`; {line}"),
                    format!("export {variable}=sh; sh -c '{update}'; {line}"),
                    format!("export {variable}=sh; bash -c \"{update}\"; {line}"),
                    format!("export {variable}=sh\n({update})\n{line}"),
                    format!("export {variable}=sh; x=$'a\\'b'; {update}; {line}"),
                    format!("export {variable}=sh; {update}; {line}"),
                    format!("export {variable}=sh; {{ {update}; }}; {line}"),
                    format!("export {variable}=sh\n{update}\n{line}"),
                    format!("export {variable}=sh; command {update}; {line}"),
                    format!("export {variable}=sh; command -p {update}; {line}"),
                ] {
                    assert!(runs(&form), "{form}");
                }
            }
        }
        for command in [
            "export ACK_PAGER=less; unset ACK_PAGER; ack 'codeflow update' docs",
            "export NOTE=sh; unset NOTE; gh search issues 'codeflow update'",
            "export GH_PAGER='less -R'; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A command variable set without a `NAME=value` word (a builtin that
    /// writes a name, an append, a default expansion, a loop variable, a
    /// name reference, a bare name a declaration gives) holds a value this
    /// reader does not see: the line is judged on its raw text.
    #[test]
    fn a_command_variable_the_reader_cannot_track_is_unresolved() {
        for (variable, line) in [
            ("ACK_PAGER", "ack -h 'codeflow update' notes.txt"),
            ("GH_PAGER", "gh search issues 'codeflow update'"),
        ] {
            for form in [
                format!("export {variable}=sh; readonly {variable}; unset {variable}; {line}"),
                format!("export {variable}=sh; declare -r {variable}; {variable}=less; {line}"),
                format!("export {variable}=sh; typeset -rx {variable}; unset {variable}; {line}"),
                format!("export {variable}=s; {variable}+=h; {line}"),
                format!("export {variable}=less; read {variable} < pager.txt; {line}"),
                format!("export {variable}=less; read -r -p x {variable} < pager.txt; {line}"),
                format!("export {variable}=less; printf -v {variable} sh; {line}"),
                format!("export {variable}=less; printf -v{variable} sh; {line}"),
                format!("export {variable}; : ${{{variable}:=sh}}; {line}"),
                format!("export {variable}; : \"${{{variable}=sh}}\"; {line}"),
                format!("export {variable}=less; declare -n r={variable}; r=sh; {line}"),
                format!("export {variable}=less; getopts a {variable}; {line}"),
                format!("export {variable}=less; mapfile -t {variable} < pager.txt; {line}"),
                format!("export {variable}=less; for {variable} in sh; do {line}; done"),
                format!("export {variable}=less; select {variable} in sh; do {line}; done"),
                format!("export {variable}=less; sleep 1 & wait -n -p {variable}; {line}"),
                format!("export {variable}; {line}"),
                format!("readonly NOTE=1; export {variable}=sh; unset {variable}; {line}"),
            ] {
                assert!(runs(&form), "{form}");
            }
        }
        for command in [
            "readonly NOTE=1; export ACK_PAGER=less; ack 'codeflow update' docs",
            "export ACK_PAGER=less; echo \"$ACK_PAGER\"; ack 'codeflow update' docs",
            "export GH_PAGER=less; read answer < notes.txt; gh search issues 'codeflow update'",
            "env -u GH_PAGER gh search issues 'codeflow update'",
            "echo GH_PAGER; gh search issues 'codeflow update'",
            "printf '%s' GH_PAGER; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// Every variable check reads one view of a command: behind the
    /// current-shell wrappers (`builtin`, `command`, `command -p`, `noglob`)
    /// a builtin acts on this shell, and behind any other launcher (`env`,
    /// `nice`, `timeout`) it does not run in this shell at all. A wrapper
    /// the view cannot remove judges the line on its raw text.
    #[test]
    fn variable_checks_read_through_current_shell_wrappers() {
        for (variable, line) in [
            ("ACK_PAGER", "ack -h 'codeflow update' notes.txt"),
            ("GH_PAGER", "gh search issues 'codeflow update'"),
        ] {
            for form in [
                format!("export {variable}=sh; builtin readonly {variable}; unset {variable}; {line}"),
                format!("export {variable}=sh; command readonly {variable}; unset {variable}; {line}"),
                format!("export {variable}=sh; command -p readonly {variable}; {variable}=less; {line}"),
                format!("export {variable}=sh; command -- declare -r {variable}; unset {variable}; {line}"),
                format!("export {variable}=sh; builtin command typeset -r {variable}; unset {variable}; {line}"),
                format!("export {variable}=sh; noglob readonly {variable}; unset {variable}; {line}"),
                format!("export {variable}=less; builtin read {variable} < pager.txt; {line}"),
                format!("export {variable}=less; command -p read {variable} < pager.txt; {line}"),
                format!("export {variable}=less; command printf -v {variable} sh; {line}"),
                format!("export {variable}=less; builtin mapfile -t {variable} < pager.txt; {line}"),
                format!("export {variable}=less; builtin declare -n r={variable}; r=sh; {line}"),
                format!("export {variable}=less; noglob read {variable} < pager.txt; {line}"),
                format!("export {variable}=less; print -v {variable} sh; {line}"),
                format!("export {variable}=less; vared {variable}; {line}"),
                format!("export {variable}=sh; env unset {variable}; {line}"),
                format!("export {variable}=sh; env {variable}=less; {line}"),
                format!("export {variable}=sh; nice export {variable}=less; {line}"),
                format!("export {variable}=sh; timeout 5 unset {variable}; {line}"),
                format!("export {variable}=sh; command -x unset {variable}; {line}"),
                format!("export {variable}=sh; builtin unset {variable}; {line}"),
                format!("export {variable}=sh; builtin export {variable}=less; {line}"),
                format!("export {variable}=sh; noglob unset {variable}; {line}"),
                format!("export {variable}=sh; - unset {variable}; {line}"),
                format!("export {variable}=sh; command builtin unset {variable}; {line}"),
                format!("export {variable}=sh; command unset {variable}; {line}"),
                format!("export {variable}=sh; command export {variable}=less; {line}"),
            ] {
                assert!(runs(&form), "{form}");
            }
        }
        for command in [
            "builtin echo hello; gh search issues 'codeflow update'",
            "command ls docs; gh search issues 'codeflow update'",
            "command -v gh; gh search issues 'codeflow update'",
            "export NOTE=1; builtin unset NOTE; gh search issues 'codeflow update'",
            "export NOTE=1; noglob unset NOTE; gh search issues 'codeflow update'",
            "command export GH_PAGER=less; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A word the shell expands (braces, a pattern, a variable) where a
    /// name-writing or declaring builtin, or a wrapper before one, takes a
    /// word may write any name, or turn an argument into the option that
    /// names one: the line is judged on its raw text. A word quoted as
    /// written is data. Values set anywhere on the line count for every
    /// reader on it, since a loop or function can run a later assignment
    /// first.
    #[test]
    fn expanded_operands_and_later_values_judge_the_line() {
        for (variable, line) in [
            (
                "ACK_PAGER",
                "ack --nocolor --nogroup -h 'codeflow update' notes.txt",
            ),
            ("GH_PAGER", "gh search issues 'codeflow update'"),
        ] {
            for form in [
                format!("export {variable}=less; printf -v {{{variable},sh}}; {line}"),
                format!(
                    "export {variable}=sh; readonly {{{variable},NOTE}}; unset {variable}; {line}"
                ),
                format!("export {variable}=less; printf {{-v,{variable}}} sh; {line}"),
                format!("export {variable}=less; read {{{variable},x}} < pager.txt; {line}"),
                format!("export {variable}=less; read {variable}* < pager.txt; {line}"),
                format!("export {variable}=less; read \"$name\" < pager.txt; {line}"),
                format!("export {variable}=less; mapfile -t {{{variable},x}} < pager.txt; {line}"),
                format!("export {variable}=less; getopts a {{{variable},x}}; {line}"),
                format!("export {variable}=less; wait -n -p {{{variable},x}}; {line}"),
                format!("export {variable}=less; vared {{{variable},x}}; {line}"),
                format!("export {variable}=less; declare {{{variable},x}}=sh; {line}"),
                format!("export {variable}=less; export {{{variable},x}}=sh; {line}"),
                format!("export {variable}=less; local -a \"$name\"; {line}"),
                format!("export {variable}=less; {{read,{variable}}} < pager.txt; {line}"),
                format!("export {variable}=less; builtin {{read,{variable}}} < pager.txt; {line}"),
                format!("export {variable}=less; command -$v read {variable} < pager.txt; {line}"),
                format!("for i in 1 2; do {line}; {variable}=sh; done"),
                format!("f() {{ {line}; }}; {variable}=sh; f"),
            ] {
                assert!(runs(&form), "{form}");
            }
        }
        for command in [
            "ACK_PAGER=less ack 'codeflow update' docs",
            "read line < notes.txt; gh search issues 'codeflow update'",
            "printf -v out '%s' x; gh search issues 'codeflow update'",
            "printf -v out '%s' '{GH_PAGER,sh}'; gh search issues 'codeflow update'",
            "echo '{ACK_PAGER,sh}'; ack 'codeflow update' docs",
            "ls *.md; gh search issues 'codeflow update'",
            "export GH_PAGER='less -R'; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A builtin that writes or declares names is read only in the forms
    /// known to name plain variables: each option on its admitted list and
    /// each name a plain identifier. Any other option (a pattern, a
    /// reference, a case or number format), a name with a subscript or
    /// pattern, and a builtin outside the list judge the line on its raw
    /// text, as do builtins that rebind a program name or change how the
    /// shell reads later words.
    #[test]
    fn name_writers_are_read_only_in_known_forms() {
        for (variable, pattern, line) in [
            (
                "ACK_PAGER",
                "ACK_*",
                "ack --nocolor --nogroup -h 'codeflow update' notes.txt",
            ),
            ("GH_PAGER", "GH_*", "gh search issues 'codeflow update'"),
        ] {
            for form in [
                format!("export {variable}=less; typeset -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; declare -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; export -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; readonly -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; local -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; typeset -gm '{pattern}=sh'; {line}"),
                format!("export {variable}=less; typeset -mg '{pattern}=sh'; {line}"),
                format!("export {variable}=less; typeset -m -- '{pattern}=sh'; {line}"),
                format!("export {variable}=less; typeset +m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; builtin typeset -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; command typeset -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; noglob typeset -m '{pattern}=sh'; {line}"),
                format!("export {variable}=less; typeset '{pattern}=sh'; {line}"),
                format!("export {variable}=less; export '{variable}[1]=sh'; {line}"),
                format!("export {variable}=x; read '{variable}[1]' < pager.txt; {line}"),
                format!("export {variable}=x; read -p '{variable}[1]' y < pager.txt; {line}"),
                format!("export {variable}=x; printf -v '{variable}[1]' sh; {line}"),
                format!("export {variable}=x; getopts a '{variable}[1]'; {line}"),
                format!("export {variable}=less; declare -l note=x; {line}"),
                format!("export {variable}=less; typeset -u note=x; {line}"),
                format!("export {variable}=less; typeset -T note notes; {line}"),
                format!("export {variable}=less; declare -p note=1; {line}"),
                format!("export {variable}=less; export -n note; {line}"),
                format!("export {variable}=less; local -n ref=note; {line}"),
                format!("export {variable}=less; read -A note < pager.txt; {line}"),
                format!("export {variable}=less; mapfile -C cb note < pager.txt; {line}"),
                format!("export {variable}=less; print -P -v note x; {line}"),
                format!("export {variable}=less; vared -A note; {line}"),
                format!("export {variable}=less; private {variable}=sh; {line}"),
                format!("export {variable}=less; integer note=1; {line}"),
                format!("export {variable}=less; let note=1; {line}"),
                format!("export {variable}=less; zparseopts -D -A {variable}; {line}"),
                format!("export {variable}=less; zstyle -s ctx style {variable}; {line}"),
                format!("export {variable}=less; zformat -f {variable} x; {line}"),
                format!("export {variable}=less; pcre_match -v {variable} x; {line}"),
                format!("export {variable}=less; trap '{variable}=sh' DEBUG; {line}"),
                format!("export {variable}=less; setopt globassign; {line}"),
                format!("export {variable}=less; shopt -s expand_aliases; {line}"),
                format!("export {variable}=less; set -k; {line} {variable}=sh"),
                format!("export {variable}=less; set -o keyword; {line}"),
                format!("export {variable}=less; set -A note x; {line}"),
            ] {
                assert!(runs(&form), "{form}");
            }
        }
        for command in [
            "hash -p /bin/sh gh; echo 'codeflow update' | gh",
            "hash gh=/bin/sh; echo 'codeflow update' | gh",
            "enable -f ./x.so echo; echo 'codeflow update'",
            "set -k; gh GH_PAGER=sh search issues 'codeflow update'",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "export GH_PAGER=less; gh search issues 'codeflow update'",
            "local x=1; gh search issues 'codeflow update'",
            "declare -r MAXV=3; gh search issues 'codeflow update'",
            "readonly VERSION=4; gh search issues 'codeflow update'",
            "typeset -gx NOTE=1; gh search issues 'codeflow update'",
            "export -p; gh search issues 'codeflow update'",
            "read line < notes.txt; gh search issues 'codeflow update'",
            "read -r -s line < notes.txt; gh search issues 'codeflow update'",
            "mapfile -t lines < notes.txt; gh search issues 'codeflow update'",
            "getopts ab opt; gh search issues 'codeflow update'",
            "printf -v out '%s' x; gh search issues 'codeflow update'",
            "printf '%s' \"$x\"; gh search issues 'codeflow update'",
            "print -r -- text; gh search issues 'codeflow update'",
            "set -euo pipefail; gh search issues 'codeflow update'",
            "set -- a b; gh search issues 'codeflow update'",
            "hash -r; gh search issues 'codeflow update'",
            "unset NOTE; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
        // An integer attribute makes the shell evaluate later values as
        // arithmetic, which writes other names: judged on the raw text.
        assert!(runs("typeset -i n=0; gh search issues 'codeflow update'"));
    }

    /// A search or forge program is read as data only in the forms known
    /// to run nothing: each option on the program's list and each `gh`
    /// command on gh's list. Any other option, a word the shell expands
    /// where an option can stand, or another gh command (an alias, an
    /// extension, a setting) judges the line on its raw text.
    #[test]
    fn data_programs_are_read_only_in_known_forms() {
        for command in [
            "gh alias set --shell u 'codeflow update'; gh u",
            "gh alias set u '!codeflow update'",
            "gh extension exec x 'codeflow update'",
            "gh config set pager sh; gh search issues 'codeflow update'",
            "gh u 'codeflow update'",
            "rg --hostname-bin=x 'codeflow update' docs",
            "rg -Q 'codeflow update' docs",
            "ack --ackrc=x.rc -h 'codeflow update' notes.txt",
            "ack --output=x 'codeflow update' docs",
            "ag --unknown 'codeflow update' docs",
            "o=--pager=sh; ack \"$o\" -h 'codeflow update' notes.txt",
            "rg \"$o\" 'codeflow update' notes.txt",
            "NOTE='codeflow update'; ack \"$NOTE\" docs",
            "gh repo clone owner/repo -- \"$flags\"; echo 'codeflow update'",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "gh search issues 'codeflow update'",
            "gh pr list --search 'codeflow update'",
            "gh issue create --title 'codeflow update' --body x",
            "gh issue comment 5 --body 'codeflow update'",
            "gh api search/issues -f q='codeflow update'",
            "gh search issues -- 'codeflow update'",
            "rg -n -i 'codeflow update' docs",
            "rg -g '*.md' -e 'codeflow update' docs",
            "rg -A3 --type md 'codeflow update' docs",
            "rg --no-config -- 'codeflow update' docs",
            "ack -- \"$NOTE\" docs; echo 'codeflow update'",
            "ack -i --nocolor 'codeflow update' docs",
            "ag -Q --nogroup 'codeflow update' docs",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A program a search or forge program runs with its output or a file
    /// (a pager, editor, browser or preprocessor value) is read as data
    /// only when it is a viewer known to run nothing it reads; any other
    /// program, such as an interpreter reading its input, judges the line
    /// on its raw text.
    #[test]
    fn programs_given_output_to_read_are_known_viewers() {
        for command in [
            "export GH_PAGER=perl; gh search issues 'codeflow update'",
            "GH_PAGER=python3 gh search issues 'codeflow update'",
            "export ACK_PAGER=ed; ack -h 'codeflow update' notes.txt",
            "ack --pager=perl -h 'codeflow update' notes.txt",
            "ag --pager=ruby 'codeflow update' notes.txt",
            "rg --pre=perl 'codeflow update' notes.txt",
            "export GH_EDITOR=node; gh issue create --title t --body 'codeflow update'",
            "export PAGER=perl; gh search issues 'codeflow update'",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "GH_PAGER=cat gh search issues 'codeflow update'",
            "export GH_PAGER='less -R'; gh search issues 'codeflow update'",
            "export ACK_PAGER='less -FRX'; ack 'codeflow update' docs",
            "ag --pager=more 'codeflow init' docs",
            "rg --pre=pdftotext 'codeflow update' docs",
            "export GH_EDITOR='code --wait'; gh issue create --title 'codeflow update'",
            "export BROWSER=echo; gh search issues 'codeflow update' --web",
            "export PAGER='less -s'; gh search issues 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A variable that gives a search or forge program, or a pager it
    /// runs, options or a settings file can make it run a program: live on
    /// the line, it judges the line on its raw text.
    #[test]
    fn options_variables_judge_the_line() {
        for command in [
            "export ACK_OPTIONS=--pager=sh; ack -h 'codeflow update' notes.txt",
            "ACK_OPTIONS='--pager=sh' ack -h 'codeflow update' notes.txt",
            "export ACKRC=x.rc; ack -h 'codeflow update' notes.txt",
            "export RIPGREP_CONFIG_PATH=rg.conf; rg 'codeflow update' docs",
            "export GH_CONFIG_DIR=cfg; gh search issues 'codeflow update'",
            "export XDG_CONFIG_HOME=cfg; gh search issues 'codeflow update'",
            "export LESS='+!sh'; gh search issues 'codeflow update'",
            "read ACK_OPTIONS < opts.txt; ack -h 'codeflow update' notes.txt",
            "export ACK_OPTIONS; ack -h 'codeflow update' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "export NOTE=1; ack 'codeflow update' docs",
            "RIPGREP_CONFIG_PATH=rg.conf grep 'codeflow update' notes.txt",
            "export ACK_OPTIONS=x; cat notes.txt; echo 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A program handed output or a file to read is admitted only as a
    /// viewer with options on that viewer's own list and no operand but
    /// `-`. Any other option or operand, a viewer whose ways of running
    /// code are not listed, or a variable that makes an admitted viewer run
    /// a program judges the line on its raw text.
    #[test]
    fn viewers_are_admitted_only_with_their_listed_options() {
        for command in [
            "export ACK_PAGER='emacs --script /dev/stdin'; ack -h 'codeflow update' notes.txt",
            "ACK_PAGER='emacs -l /dev/stdin' ack -h 'codeflow update' notes.txt",
            "ACK_PAGER=emacs ack -h 'codeflow update' notes.txt",
            "ACK_PAGER='vim -S /dev/stdin' ack 'codeflow update' notes.txt",
            "ACK_PAGER='vim -es' ack 'codeflow update' notes.txt",
            "ACK_PAGER='nvim -u /dev/stdin' ack 'codeflow update' notes.txt",
            "ACK_PAGER='vi -s /dev/stdin' ack 'codeflow update' notes.txt",
            "ack --pager='emacs --script /dev/stdin' 'codeflow update' notes.txt",
            "ag --pager 'vim -s /dev/stdin' 'codeflow update' notes.txt",
            "GH_PAGER='less -k keys' gh search issues 'codeflow update'",
            "GH_PAGER='less notes.txt' gh search issues 'codeflow update'",
            "GH_PAGER='bat --pager perl' gh search issues 'codeflow update'",
            "GH_PAGER=delta gh search issues 'codeflow update'",
            "GH_EDITOR='kak -e nop' gh search issues 'codeflow update'",
            "GH_EDITOR='emacsclient -a perl' gh search issues 'codeflow update'",
            "BROWSER='lynx -cmd_script=/dev/stdin' gh search issues 'codeflow update'",
            "BROWSER='open -a Terminal' gh search issues 'codeflow update'",
            "LESSOPEN='|perl %s' gh search issues 'codeflow update'",
            "BAT_PAGER=perl gh search issues 'codeflow update'",
            "VIMINIT='so /dev/stdin' gh search issues 'codeflow update'",
            "rg --pre='less -k keys' 'codeflow update' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "export ACK_PAGER='less -R'; ack 'codeflow update' notes.txt",
            "GH_PAGER='less -FRX' gh search issues 'codeflow update'",
            "GH_PAGER='less --tabs=4 -RFX' gh search issues 'codeflow update'",
            "GH_PAGER='diff-so-fancy | less -R' gh search issues 'codeflow update'",
            "GH_PAGER='bat -p --paging=always' gh search issues 'codeflow update'",
            "GH_PAGER='' gh search issues 'codeflow update'",
            "GH_EDITOR=vim gh search issues 'codeflow update'",
            "GH_EDITOR='vim -R -' gh search issues 'codeflow update'",
            "GH_EDITOR='code --wait' gh search issues 'codeflow update'",
            "GH_EDITOR='subl -w' gh search issues 'codeflow update'",
            "BROWSER=firefox gh search issues 'codeflow update'",
            "ag --pager='less -R' 'codeflow update' docs",
            "rg --pre=pdftotext 'codeflow update' docs",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// A program another program runs, such as a Git pager setting, is
    /// unresolved when it is a shell reading its input.
    #[test]
    fn a_stdin_shell_another_program_runs_is_unresolved() {
        for command in ["sh", "bash -s", "zsh", "'sh'", "env sh"] {
            assert!(unresolved_command(command), "{command}");
        }
        for command in ["less", "less -R", "cat", "delta --dark", ""] {
            assert!(!unresolved_command(command), "{command}");
        }
    }

    /// A program another program hands its output or a file to read, such
    /// as a Git pager, editor or text converter setting, is unresolved
    /// unless it is an admitted viewer with its listed options.
    #[test]
    fn a_program_handed_output_is_unresolved_unless_an_admitted_viewer() {
        for command in [
            "perl",
            "python3",
            "node",
            "ruby",
            "awk -f /dev/stdin",
            "sh",
            "emacs",
            "emacs --script /dev/stdin",
            "vim -S x.vim",
            "vim -es",
            "less -k keys",
            "less notes.txt",
            "less +!sh",
            "delta",
            "bat --pager perl",
            "LESSOPEN=x less",
            "$PAGER",
            "code --wait --install-extension x",
        ] {
            assert!(unresolved_fed_command(command), "{command}");
        }
        for command in [
            "",
            ":",
            "true",
            "cat",
            "less",
            "less -R",
            "less -FRX",
            "less -x4 -R",
            "less --tabs=4 -RFX",
            "diff-so-fancy | less --tabs=4 -RFX",
            "vim",
            "vim -R -",
            "nvim --clean",
            "code --wait",
            "subl -w",
            "bat -p --paging=always",
            "/usr/bin/firefox",
        ] {
            assert!(!unresolved_fed_command(command), "{command}");
        }
    }

    /// The words after `--` that `gh repo clone`, `gh repo fork` and `gh
    /// gist clone` pass to the `git clone` they run are read as a quoted
    /// argument is: a setting or option value holding a command line. The
    /// words after `--` of any other gh command are its data.
    #[test]
    fn words_gh_passes_to_git_are_read() {
        for command in [
            "gh repo clone owner/repo -- -c core.sshCommand='env codeflow update'",
            "gh repo clone owner/repo -- --upload-pack='codeflow update'",
            "gh repo fork owner/repo --clone -- -c core.fsmonitor='nice codeflow init'",
            "gh gist clone abc123 -- -c core.sshCommand='env codeflow update'",
            "gh repo clone -u upstream owner/repo dir -- -c core.sshCommand='codeflow init'",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "gh repo clone owner/repo -- --depth 1",
            "gh issue create --title 'codeflow update fails' --body 'run codeflow update'",
            "gh search issues -- 'codeflow update'",
            "gh search prs -- 'codeflow init --yes'",
            "gh search code -- 'codeflow update'",
            "gh issue list -- 'codeflow update'",
            "gh pr list --search x -- 'codeflow update'",
            "gh repo list -- 'codeflow update'",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// An editor's or pager's own command, given as an option value or a
    /// `+` word, runs a shell command through its command language, so it
    /// is judged on its text as interpreter code is.
    #[test]
    fn editor_commands_are_judged_on_their_text() {
        for command in [
            "less '+!codeflow update' notes.txt",
            "vim -c '!codeflow update' notes.txt",
            "vim '+!codeflow update' notes.txt",
            "nvim --cmd 'silent !codeflow update' notes.txt",
            "vi -c 'r !./target/debug/codeflow init' notes.txt",
            "emacs --eval '(shell-command \"codeflow update\")'",
            "vim --cmd='!codeflow update' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "less -R notes.txt",
            "less +10 notes.txt",
            "vim -c 'set nu' notes.txt",
            "vim +10 notes.txt",
            "emacs --eval '(setq x 1)' notes.txt",
            "vim docs/codeflow-update.md",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// Package managers that run a package's binary by name, a subcommand
    /// that runs a command, and a quoted option value that names a command
    /// after its own `=`.
    #[test]
    fn package_managers_and_option_values_that_run_the_binary_are_found() {
        for command in [
            "yarn codeflow update",
            "pnpm codeflow update",
            "yarn run codeflow update",
            "pnpm run codeflow init --yes",
            "npm explore pkg -- codeflow update",
            "tar -cf a.tar --checkpoint=1 '--checkpoint-action=exec=codeflow update' notes.txt",
            "tar -cf a.tar --checkpoint-action 'exec=codeflow init --yes' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// The word a package runner or manager runs is a program position, so
    /// a word the shell expands there is judged as one: with and without a
    /// subcommand, for each runner and manager this reader knows, and for
    /// each kind of expansion. A word the shell expands among the words of
    /// a program this reader does not know is judged on its raw text.
    #[test]
    fn expanded_words_a_package_runner_runs_are_found() {
        for runner in [
            "yarn",
            "yarn run",
            "yarn exec",
            "yarn dlx",
            "pnpm",
            "pnpm run",
            "pnpm exec",
            "pnpm dlx",
            "pn",
            "npm exec --",
            "npm x",
            "npm explore pkg --",
            "npx",
            "bunx",
            "bun",
            "bun x",
            "bun run",
            "pnpx",
            "pnx",
            "corepack yarn",
            "volta run",
            "deno run",
            "uv run",
            "uvx",
        ] {
            for word in [
                "{codeflow,update}",
                "{codeflow,} update",
                "codefl?w update",
                "codefl[o]w init --yes",
                "=codeflow update",
                "$B update",
            ] {
                let command = format!("{runner} {word}");
                assert!(runs(&command), "{command}");
            }
        }
        // A program this reader does not know: a word the shell expands
        // is judged on its raw text with the words after it.
        for command in [
            "mise exec -- {codeflow,update}",
            "mise exec -- {codeflow,} update",
            "mise exec -- =codeflow update",
            "tool --run {./target/debug/codeflow,init}",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// npm reads its operands as data only after one of its own commands
    /// that never runs a program, in its exact name or a documented alias:
    /// an abbreviation, a command that runs a script or a binary, an option
    /// this reader does not know before the command, or an expanded word
    /// there leaves npm read as a package runner.
    #[test]
    fn npm_runs_the_binary_after_any_word_but_its_own_commands() {
        for command in [
            "npm exe -- codeflow update",
            "npm exe codeflow update",
            "npm expl pkg -- codeflow update",
            "npm explo pkg -- codeflow update",
            "npm explor pkg -- codeflow update",
            "npm run codeflow update",
            "npm run-script codeflow update",
            "npm rum codeflow update",
            "npm urn codeflow init",
            "npm t -- codeflow update",
            "npm Exec codeflow update",
            "npm --prefix codeflow exe -- codeflow update",
            "npm -g exe codeflow update",
            "npm --registry update exe -- codeflow update",
            "npm --reg update exe -- codeflow update",
            "npm --prefix -x update codeflow update",
            "npm -- exe codeflow update",
            "npm $SUB codeflow update",
            "npm --prefix $D update codeflow update",
            "npm exe -- {codeflow,update}",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "npm --prefix codeflow update",
            "npm -C codeflow update",
            "npm --prefix=codeflow update",
            "npm --workspace codeflow update",
            "npm -g update codeflow",
            "npm update codeflow",
            "npm up codeflow",
            "npm i codeflow",
            "npm view codeflow init",
            "npm uninstall codeflow",
            "npm ls codeflow",
            "npm -- update codeflow",
            "npm init -y",
            "npm run build",
            "yarn add codeflow",
            "pnpm add -D codeflow",
            "pnpm install --frozen-lockfile",
            "cargo test {a,b}",
            "cp {a,b} codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    #[test]
    fn a_backslash_path_is_read_on_a_shell_that_takes_it_as_a_separator() {
        for command in [
            ".\\target\\debug\\codeflow.exe update",
            "C:\\Users\\me\\.cargo\\bin\\codeflow.exe init --yes",
        ] {
            assert!(install_run(command, false).is_some(), "{command}");
        }
        assert!(install_run(".\\target\\debug\\codeflow.exe doctor", false).is_none());
    }

    #[test]
    fn a_run_inside_a_quoted_argument_says_so() {
        let run = install_run("python3 -m tool \"codeflow update\"", true).unwrap();
        assert!(run.in_argument, "{run:?}");
        let run = install_run("codeflow update", true).unwrap();
        assert!(!run.in_argument, "{run:?}");
    }

    #[test]
    fn forms_that_write_nothing_and_other_programs_are_not_runs() {
        for command in [
            "codeflow update --help",
            "codeflow update -h",
            "codeflow init --help",
            "codeflow init -h",
            "codeflow help update",
            "codeflow help init",
            "codeflow --help",
            "codeflow --version",
            "codeflow -V",
            "codeflow",
            "codeflow doctor",
            "codeflow test --mode quick",
            "command -v codeflow",
            "which codeflow",
            "cargo run -- doctor",
            "cargo build",
            "cargo test update",
            "cargo update",
            "cargo init --vcs none",
            "npm init -y",
            "brew update",
            "rustup update",
            "echo the operator runs codeflow update",
            "printf 'codeflow update\\n' > notes.txt",
            "grep -rn 'codeflow update' docs",
            "git commit -m 'docs: the operator runs codeflow update'",
            "xargs codeflow doctor",
            "find . -name codeflow",
            "find . -name codeflow -print",
            "ls ./target/*/codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// Words that only name the verbs as data, which no agent's install
    /// run needs refused.
    #[test]
    fn data_that_names_the_verbs_is_not_a_run() {
        for command in [
            "codeflow task new --title \"the operator runs codeflow update\"",
            "codeflow feedback new \"codeflow init wrote a file twice\"",
            "codeflow epic new --title \"codeflow update reads the pin\"",
            "codeflow task new codeflow update",
            "[ \"$x\" = \"codeflow update\" ]",
            "test \"$x\" = \"codeflow init\"",
            "cargo nextest run update",
            "cargo +stable nextest run init",
            "cargo run --example init",
            "cargo run --example init -- --help",
            "cargo run -p codeflow-cli -- task update TSK-1",
            "cargo run --bin codeflow -- task status",
            "export NOTE=\"run codeflow update\"",
            "NOTE=\"run codeflow update\"; echo done",
            "npm --prefix codeflow update",
            "make -C codeflow init",
            "cp -r codeflow update",
            "ag 'codeflow init' docs",
            "ack 'codeflow update' docs",
            "jq '.steps[] | select(. == \"codeflow update\")' plan.json",
            "rg --pre ./tools/pdftext pattern docs",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    #[test]
    fn interpreter_code_and_unparsed_text_name_the_binary_and_a_verb() {
        for code in [
            "import subprocess; subprocess.run(['./codeflow', 'update'])",
            "require('child_process').execFileSync('codeflow', ['init'])",
            "system(\"codeflow\", \"update\")",
            "cargo run -- init",
        ] {
            assert!(raw_run(code).is_some(), "{code}");
        }
        for code in [
            "print('codeflow')",
            "d.update(x)",
            "cargo build && run tests",
        ] {
            assert!(raw_run(code).is_none(), "{code}");
        }
    }
}
