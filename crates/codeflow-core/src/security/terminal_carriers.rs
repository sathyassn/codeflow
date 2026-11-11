//! Commands run through the Herdr app's `hc` and the older `herdr` CLI
//! (TSK-1344).
//!
//! `hc run`, `hc exec` and `herdr pane run` start a command in another
//! process, outside the shell the guards read, and `hc` terminals run
//! outside the harness's Bash sandbox. A guard that read only the outer
//! words passed a command it refuses bare. This reader finds every such
//! carrier on a line, wherever the line runs it (after launchers, inside
//! `sh -c`, in a substitution), and returns the command it carries with the
//! directory it runs in. Each guard then judges that command as a command
//! of its own, by the rules it applies to the same command typed bare.
//!
//! The reader follows each CLI's own grammar: `hc` 0.1.18 (`hc --help` and
//! `hc <command> --help`), and the `herdr` usage lines for `pane run` and
//! `agent start`. A form it cannot read with certainty is refused and
//! named: an unknown subcommand or option, a word it cannot fill in, a
//! directory it cannot establish for a Git command, and nesting past
//! [`DEPTH_LIMIT`]. Every `hc run` is refused: it can start a template
//! instead of the words given, and which one cannot be verified from the
//! command. Its carried command is still judged, so a rule it breaks is
//! named too.
//!
//! Text typed into a terminal (`hc send`, `hc keys`, `herdr pane
//! send-text`, `herdr pane send-keys`, `herdr agent prompt`) is not judged:
//! the guard cannot know whether the receiving program reads it as a prompt
//! or runs it as a command, so a filter would be partial and is not offered
//! as protection. Their options are still read, and an unknown one refuses.
//! `docs/policy-reference.md` ("Terminal carriers the guards model") states
//! the forms and the floor that holds for typed text.

use std::cell::Cell;
use std::path::{Path, PathBuf};

/// The program names of the Herdr app's CLI.
pub(crate) const HC_PROGRAMS: &[&str] = &["hc", "herdr-core"];

/// The program name of the older Herdr CLI.
pub(crate) const HERDR_PROGRAMS: &[&str] = &["herdr"];

/// How deep carried commands are judged inside one another before the
/// guard refuses: a template whose command starts itself would not end.
pub(crate) const DEPTH_LIMIT: usize = 6;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Marks one level of carried-command judging until dropped.
pub(crate) struct Nested(());

impl Nested {
    /// Enter one level; `None` past [`DEPTH_LIMIT`].
    pub(crate) fn enter() -> Option<Self> {
        let depth = DEPTH.get();
        if depth >= DEPTH_LIMIT {
            return None;
        }
        DEPTH.set(depth + 1);
        Some(Self(()))
    }
}

impl Drop for Nested {
    fn drop(&mut self) {
        DEPTH.set(DEPTH.get().saturating_sub(1));
    }
}

/// A command a carrier runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    /// The form that carries it, such as `hc run` or `herdr pane run`.
    pub(crate) form: &'static str,
    /// The command, as shell text the guards read as if typed bare.
    pub(crate) line: String,
    /// The directory it runs in on this machine. `None` when the guard
    /// cannot establish it (another machine, a pane's shell, or a `cd` it
    /// cannot follow); the form is then unread and refuses, since a
    /// relative path in the command names files the guard cannot see.
    pub(crate) directory: Option<PathBuf>,
}

impl Run {
    /// The command as it would be typed bare where the line runs: a run in
    /// another directory is preceded by `cd` to it, so each guard follows
    /// the move as it does for the same line typed bare, and judges a Git
    /// command against the repository there. A run whose directory is
    /// unknown is also judged where the line runs, and its form refuses
    /// (`unknown_directories`).
    pub(crate) fn bare_line(&self, cwd: &Path) -> String {
        match &self.directory {
            Some(directory) if directory != cwd => {
                let path = crate::portable_path::slashed(directory);
                format!("cd '{}' && {}", path.replace('\'', "'\\''"), self.line)
            }
            _ => self.line.clone(),
        }
    }
}

/// A file `hc file write` or `hc file edit` changes on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Write {
    pub(crate) form: &'static str,
    pub(crate) path: String,
    /// Where a relative path starts.
    pub(crate) directory: PathBuf,
}

/// What a line runs and writes through the terminal carriers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Carried {
    pub(crate) runs: Vec<Run>,
    pub(crate) writes: Vec<Write>,
    /// Each form the guard cannot read with certainty, with why.
    pub(crate) unread: Vec<String>,
}

impl Carried {
    fn unread(&mut self, form: &str, why: impl std::fmt::Display) {
        self.unread.push(format!(
            "`{form}`: {why}; the guard cannot judge what it runs"
        ));
    }
}

/// Whether the line names a terminal carrier at all, a cheap test before
/// the full read, matching names the way the host finds programs.
fn mentions_carrier(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    HC_PROGRAMS
        .iter()
        .chain(HERDR_PROGRAMS)
        .any(|name| command.contains(name))
}

/// The name a program word runs as, matched the way the host finds the
/// program.
fn carrier_name(program: &str) -> String {
    let name = super::dangerous::program_name(program);
    match name.strip_suffix(".exe") {
        Some(stem) => stem.to_string(),
        None => name,
    }
}

/// Whether the program word names a terminal carrier (`hc`, `herdr`).
pub(crate) fn is_carrier_program(program: &str) -> bool {
    let name = carrier_name(program);
    HC_PROGRAMS.contains(&name.as_str()) || HERDR_PROGRAMS.contains(&name.as_str())
}

/// Every command the line runs through `hc` or `herdr`, with the directory
/// it runs in, every file `hc file` changes, and every carrier form the
/// guard cannot read. `cwd` is where the line runs.
pub(crate) fn carried(command: &str, cwd: &Path) -> Carried {
    let mut out = Carried::default();
    if !mentions_carrier(command) {
        return out;
    }
    let homes = std::iter::once(crate::portable_path::user_home())
        .chain(crate::hooks::git_guard::line_home::assigned().map(Some));
    for home in homes {
        for call in super::outward::unwrapped_commands(command, cwd, home.as_deref()) {
            let Some(program) = call.args.first() else {
                continue;
            };
            let name = carrier_name(program);
            let name = name.as_str();
            let words = Words {
                args: &call.args[1..],
                patterns: call
                    .expands
                    .as_deref()
                    .map(|p| p.get(1..).unwrap_or_default()),
            };
            let place = Place {
                cwd: call.cwd.clone(),
                home: home.clone(),
            };
            let before = (out.runs.len(), out.writes.len(), out.unread.len());
            if HC_PROGRAMS.contains(&name) {
                hc(&words, &place, &mut out);
            } else if HERDR_PROGRAMS.contains(&name) {
                herdr(&words, &mut out);
            } else {
                continue;
            }
            let carries = out.runs.len() > before.0 || out.writes.len() > before.1;
            if carries && call.supplier.is_some() {
                out.unread(
                    name,
                    "its words come from input the guard cannot see (`xargs`, `find -exec`)",
                );
            }
            let unfilled = call.unresolved.iter().any(|unknown| {
                !unknown.is_empty() && words.args.iter().any(|a| a.contains(unknown.as_str()))
            });
            if carries && unfilled {
                out.unread(name, "a word holds an expansion the guard cannot fill in");
            }
        }
    }
    out.runs
        .sort_by(|a, b| a.line.cmp(&b.line).then(a.directory.cmp(&b.directory)));
    out.runs.dedup();
    out.writes
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.directory.cmp(&b.directory)));
    out.writes.dedup();
    unknown_directories(&mut out);
    out.unread.sort();
    out.unread.dedup();
    out
}

/// Marks unread each run whose directory the guard cannot establish: on
/// another machine (`--on`, with or without `--cwd`), in a `herdr` pane's
/// shell, or after a `cd` it cannot follow. A relative path in such a
/// command names files the guard cannot see, so judging the words where
/// the line runs would judge a different tree; the form refuses instead.
fn unknown_directories(out: &mut Carried) {
    let forms: Vec<&'static str> = out
        .runs
        .iter()
        .filter(|run| run.directory.is_none())
        .map(|run| run.form)
        .collect();
    for form in forms {
        out.unread(
            form,
            "the directory it runs in cannot be established (another machine, a terminal \
             pane, a `cd` the guard cannot follow, or a `--cwd` the guard cannot place on \
             this host)",
        );
    }
}

/// A carrier's words after its program name, with whether each is an
/// unquoted pattern the shell expands.
struct Words<'a> {
    args: &'a [String],
    patterns: Option<&'a [bool]>,
}

impl Words<'_> {
    fn pattern(&self, at: usize) -> bool {
        // A lost reading is taken as a pattern, which the guards judge
        // more strictly than a literal.
        self.patterns
            .is_none_or(|p| p.get(at).copied().unwrap_or(true))
    }
}

/// Where the carrier itself runs.
struct Place {
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
}

/// The options of one carrier form: flags, and options that take a value.
struct Grammar {
    flags: &'static [&'static str],
    valued: &'static [&'static str],
}

const JSON_HELP: &[&str] = &["--json", "-h", "--help"];

/// The options read up to the first operand: their values by name, the
/// index of the first operand, and whether help was asked for.
struct Options {
    values: Vec<(&'static str, String)>,
    operands: usize,
    help: bool,
}

impl Options {
    fn last(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.values
            .iter()
            .filter(move |(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Read options from `at` until the first operand or `--`. `interleaved`
/// keeps reading options after operands (clap's default for commands with
/// a fixed number of operands); the operand index is then the first one.
fn options(
    args: &[String],
    at: usize,
    grammar: &Grammar,
    interleaved: bool,
) -> Result<Options, String> {
    let mut out = Options {
        values: Vec::new(),
        operands: args.len(),
        help: false,
    };
    let mut first_operand = None;
    let mut index = at;
    while let Some(word) = args.get(index) {
        if word == "--" {
            first_operand.get_or_insert(index + 1);
            break;
        }
        if !word.starts_with('-') || word == "-" {
            first_operand.get_or_insert(index);
            if !interleaved {
                break;
            }
            index += 1;
            continue;
        }
        let (name, attached) = match word.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (word.as_str(), None),
        };
        if matches!(name, "-h" | "--help") && attached.is_none() {
            out.help = true;
            index += 1;
            continue;
        }
        if let Some(flag) = grammar.flags.iter().chain(JSON_HELP).find(|f| **f == name) {
            if attached.is_some() {
                return Err(format!("the flag `{flag}` takes no value"));
            }
            out.values.push((flag, String::new()));
            index += 1;
            continue;
        }
        let Some(option) = grammar.valued.iter().find(|f| **f == name) else {
            return Err(format!("the option `{name}` is not one the guard reads"));
        };
        let value = if let Some(value) = attached {
            value
        } else {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("the option `{option}` has no value"))?
        };
        out.values.push((option, value));
        index += 1;
    }
    out.operands = first_operand.unwrap_or(args.len());
    Ok(out)
}

/// `hc` and its subcommands.
#[allow(clippy::too_many_lines)] // One dispatch over the closed list of `hc` commands.
fn hc(words: &Words<'_>, place: &Place, out: &mut Carried) {
    const NONE: Grammar = Grammar {
        flags: &[],
        valued: &[],
    };
    let args = words.args;
    let Some(first) = args.first().map(String::as_str) else {
        // `hc` alone picks a terminal to attach to.
        return;
    };
    // Top-level flags print and exit.
    if matches!(
        first,
        "--skill" | "--help" | "-h" | "--version" | "-V" | "--licenses"
    ) {
        return;
    }
    let simple = |grammar: &Grammar, form: &'static str, out: &mut Carried| {
        if let Err(why) = options(args, 1, grammar, true) {
            out.unread(form, why);
        }
    };
    match first {
        "run" => hc_run(words, place, out),
        "exec" => hc_exec(words, place, out),
        "file" => hc_file(args, place, out),
        "machine" => hc_machine(args, place, out),
        "send" => simple(
            &Grammar {
                flags: &["--force", "--wait"],
                valued: &["--if", "--timeout"],
            },
            "hc send",
            out,
        ),
        "wait" => simple(
            &Grammar {
                flags: &[],
                valued: &["--children", "--output", "--timeout"],
            },
            "hc wait",
            out,
        ),
        "attach" => simple(
            &Grammar {
                flags: &["--observe"],
                valued: &["--history"],
            },
            "hc attach",
            out,
        ),
        "ls" => simple(
            &Grammar {
                flags: &["--all"],
                valued: &["--label"],
            },
            "hc ls",
            out,
        ),
        "read" => simple(
            &Grammar {
                flags: &[],
                valued: &["--lines"],
            },
            "hc read",
            out,
        ),
        "interrupt" => simple(
            &Grammar {
                flags: &[],
                valued: &["--timeout"],
            },
            "hc interrupt",
            out,
        ),
        "keep" => simple(
            &Grammar {
                flags: &["--off"],
                valued: &[],
            },
            "hc keep",
            out,
        ),
        "events" => simple(
            &Grammar {
                flags: &[],
                valued: &["--terminal", "--children-of", "--label"],
            },
            "hc events",
            out,
        ),
        "update" => {
            let grammar = Grammar {
                flags: &["--force"],
                valued: &["--from", "--version"],
            };
            match options(args, 1, &grammar, true) {
                // `--from` reads a local file.
                Ok(parsed) => {
                    if let Some(file) = parsed.last("--from") {
                        read_path("hc update --from", file, place.cwd.clone(), out);
                    }
                }
                Err(why) => out.unread("hc update", why),
            }
        }
        "connect" => simple(
            &Grammar {
                flags: &[],
                valued: &["--session"],
            },
            "hc connect",
            out,
        ),
        "keys" => simple(&NONE, "hc keys", out),
        "get" => simple(&NONE, "hc get", out),
        "kill" => simple(&NONE, "hc kill", out),
        "rm" => simple(&NONE, "hc rm", out),
        "label" => simple(&NONE, "hc label", out),
        "put" => match options(args, 1, &NONE, true) {
            // `hc put <terminal> <file>` reads a local file and sends it.
            Ok(parsed) => match operands(args, parsed.operands, &NONE).as_slice() {
                _ if parsed.help => {}
                [_, file] => read_path("hc put", file, place.cwd.clone(), out),
                _ => out.unread("hc put", "it does not name one terminal and one file"),
            },
            Err(why) => out.unread("hc put", why),
        },
        "templates" => simple(&NONE, "hc templates", out),
        "schema" => simple(&NONE, "hc schema", out),
        "help" => {}
        "server" => match args.get(1).map(String::as_str) {
            Some("start" | "stop" | "status" | "restart") => {
                if let Err(why) = options(args, 2, &NONE, true) {
                    out.unread("hc server", why);
                }
            }
            Some("help" | "-h" | "--help" | "--json") | None => {}
            Some(other) => out.unread("hc server", format!("the subcommand `{other}` is unknown")),
        },
        other if other.starts_with('-') => {
            out.unread(
                "hc",
                format!("the option `{other}` is not one the guard reads"),
            );
        }
        other => out.unread("hc", format!("the subcommand `{other}` is unknown")),
    }
}

/// `hc run [OPTIONS] [--] [<program> <args>... | <line> | <template> [task]]`.
fn hc_run(words: &Words<'_>, place: &Place, out: &mut Carried) {
    const FORM: &str = "hc run";
    let args = words.args;
    let grammar = Grammar {
        flags: &[
            "--bg",
            "--no-parent",
            "--no-notify",
            "--outlive-parent",
            "--die-with-parent",
            "--rm",
            "--keep",
        ],
        valued: &[
            "--name",
            "--cwd",
            "--on",
            "--label",
            "--parent",
            "--timeout",
        ],
    };
    let parsed = match options(args, 1, &grammar, false) {
        Ok(parsed) => parsed,
        Err(why) => return out.unread(FORM, why),
    };
    // `hc run` may start a template, a file whose command runs instead of
    // the words given, and which template it starts depends on state the
    // guard cannot establish from the command. No form of `hc run` is known
    // to skip that dispatch, so every one refuses, a help word included:
    // the option grammar is not known well enough to tell a help flag from
    // an operand or an option's value. The carried command is still judged
    // below, so a rule it breaks is named too.
    out.unread(FORM, HC_RUN_UNVERIFIED);
    let carried_words = &args[parsed.operands..];
    if carried_words.is_empty() {
        // The machine's own interactive shell.
        return;
    }
    let directory = match run_directory(&parsed, place) {
        Ok(directory) => directory,
        Err(why) => return out.unread(FORM, why),
    };
    // One word is a shell line; several are a program and its arguments.
    let line = if carried_words.len() == 1 {
        carried_words[0].clone()
    } else {
        match quote(words, parsed.operands) {
            Ok(line) => line,
            Err(why) => return out.unread(FORM, why),
        }
    };
    out.runs.push(Run {
        form: FORM,
        line,
        directory,
    });
}

/// Why every `hc run` refuses, with what works instead.
const HC_RUN_UNVERIFIED: &str = "`hc run` can start a template instead of the words given, and \
     which one cannot be verified from the command; start the program outside a guarded \
     session, or through the host's own terminal";

/// `hc exec [OPTIONS] <ARGV>...`: run as given, with no shell.
fn hc_exec(words: &Words<'_>, place: &Place, out: &mut Carried) {
    const FORM: &str = "hc exec";
    let args = words.args;
    let grammar = Grammar {
        flags: &["-i", "--stdin"],
        valued: &["--on", "--cwd", "--env", "--timeout"],
    };
    let parsed = match options(args, 1, &grammar, false) {
        Ok(parsed) => parsed,
        Err(why) => return out.unread(FORM, why),
    };
    if parsed.help || parsed.operands >= args.len() {
        return;
    }
    let directory = match run_directory(&parsed, place) {
        Ok(directory) => directory,
        Err(why) => return out.unread(FORM, why),
    };
    let mut line = String::new();
    for assignment in parsed.all("--env") {
        let Some((name, value)) = assignment.split_once('=') else {
            return out.unread(FORM, format!("`--env {assignment}` is not key=value"));
        };
        if !is_identifier(name) || value.chars().any(char::is_control) {
            return out.unread(
                FORM,
                format!("`--env {assignment}` is not one the guard reads"),
            );
        }
        line.push_str(name);
        line.push('=');
        line.push_str(&double_quoted(value));
        line.push(' ');
    }
    match quote(words, parsed.operands) {
        Ok(command) => line.push_str(&command),
        Err(why) => return out.unread(FORM, why),
    }
    out.runs.push(Run {
        form: FORM,
        line,
        directory,
    });
}

/// `hc file read|write|edit [OPTIONS] <PATH>`.
fn hc_file(args: &[String], place: &Place, out: &mut Carried) {
    let (form, grammar) = match args.get(1).map(String::as_str) {
        Some("read") => (
            "hc file read",
            Grammar {
                flags: &[],
                valued: &["--on", "--cwd", "--offset", "--limit"],
            },
        ),
        Some("write") => (
            "hc file write",
            Grammar {
                flags: &[],
                valued: &["--on", "--cwd"],
            },
        ),
        Some("edit") => (
            "hc file edit",
            Grammar {
                flags: &[],
                valued: &["--on", "--cwd", "--old", "--new"],
            },
        ),
        Some("help" | "-h" | "--help" | "--json") | None => return,
        Some(other) => {
            return out.unread("hc file", format!("the subcommand `{other}` is unknown"))
        }
    };
    let parsed = match options(args, 2, &grammar, true) {
        Ok(parsed) => parsed,
        Err(why) => return out.unread(form, why),
    };
    if parsed.help {
        return;
    }
    let paths: Vec<&String> = operands(args, parsed.operands, &grammar);
    let [path] = paths.as_slice() else {
        return out.unread(form, "it does not name exactly one file");
    };
    if form == "hc file read" {
        // A read is judged by the secret-read rules as `cat` of the path in
        // its directory; a file on another machine (`--on`) has no known
        // directory, so the form refuses (`unknown_directories`).
        return match run_directory(&parsed, place) {
            Ok(directory) => read_path(form, path, directory, out),
            Err(why) => out.unread(form, why),
        };
    }
    if parsed.last("--on").is_some() {
        return out.unread(
            form,
            "the file is on another machine, where no path rule reaches",
        );
    }
    let directory = match run_directory(&parsed, place) {
        Ok(Some(directory)) => directory,
        Ok(None) => return out.unread(form, "the directory its path starts from is unknown"),
        Err(why) => return out.unread(form, why),
    };
    if path.contains(['$', '`']) || path.chars().any(char::is_control) {
        return out.unread(form, "its path holds an expansion the guard cannot fill in");
    }
    out.writes.push(Write {
        form,
        path: (*path).clone(),
        directory,
    });
}

/// `hc machine add --via <COMMAND>` runs that command here to reach the
/// machine; the other machine commands only manage the list.
fn hc_machine(args: &[String], place: &Place, out: &mut Carried) {
    match args.get(1).map(String::as_str) {
        Some("add") => {
            let grammar = Grammar {
                flags: &["--temp", "--install"],
                valued: &["--via", "--name", "--session"],
            };
            match options(args, 2, &grammar, true) {
                Ok(parsed) => {
                    if let Some(via) = parsed.last("--via") {
                        out.runs.push(Run {
                            form: "hc machine add --via",
                            line: via.to_string(),
                            directory: place.cwd.clone(),
                        });
                    }
                }
                Err(why) => out.unread("hc machine add", why),
            }
        }
        Some("ls" | "update" | "login" | "rename" | "rm") => {
            let grammar = Grammar {
                flags: &[],
                valued: &[],
            };
            if let Err(why) = options(args, 2, &grammar, true) {
                out.unread("hc machine", why);
            }
        }
        Some("help" | "-h" | "--help" | "--json") | None => {}
        Some(other) => out.unread("hc machine", format!("the subcommand `{other}` is unknown")),
    }
}

/// A local file a carrier reads, judged as `cat` of that path, so the
/// secret-read rules apply as they do to a bare read.
fn read_path(form: &'static str, path: &str, directory: Option<PathBuf>, out: &mut Carried) {
    if path.chars().any(char::is_control) {
        return out.unread(
            form,
            "its path holds a substitution the guard cannot fill in",
        );
    }
    out.runs.push(Run {
        form,
        line: format!("cat -- {}", shell_word(path, true)),
        directory,
    });
}

/// Whether the carrier reader reads the call `args` (program first) in
/// full: a known `hc` subcommand with every option known, whose carried
/// commands all run in a directory the guard can establish. The guards'
/// other readers then leave the outer words to it, so its carried command
/// is judged once, in its own directory (TSK-1344). A form with an unknown
/// directory (`--on`, a `herdr` pane) is not fully read, and refuses.
pub(crate) fn fully_read(args: &[String]) -> bool {
    let Some(program) = args.first() else {
        return false;
    };
    let name = carrier_name(program);
    let name = name.as_str();
    let words = Words {
        args: &args[1..],
        patterns: None,
    };
    let place = Place {
        cwd: Some(PathBuf::from("/")),
        home: crate::portable_path::user_home(),
    };
    let mut out = Carried::default();
    if HC_PROGRAMS.contains(&name) {
        hc(&words, &place, &mut out);
    } else if HERDR_PROGRAMS.contains(&name)
        && matches!(
            (
                args.get(1).map(String::as_str),
                args.get(2).map(String::as_str)
            ),
            (Some("pane"), Some("run")) | (Some("agent"), Some("start"))
        )
    {
        herdr(&words, &mut out);
    } else {
        return false;
    }
    unknown_directories(&mut out);
    out.unread.is_empty()
}

/// The operands among `args[from..]`, skipping options and their values.
fn operands<'a>(args: &'a [String], from: usize, grammar: &Grammar) -> Vec<&'a String> {
    let mut found = Vec::new();
    let mut index = from;
    while let Some(word) = args.get(index) {
        if word == "--" {
            found.extend(&args[index + 1..]);
            break;
        }
        if word.starts_with('-') && word != "-" {
            let valued = grammar.valued.contains(&word.as_str());
            index += 1 + usize::from(valued);
            continue;
        }
        found.push(word);
        index += 1;
    }
    found
}

/// The directory a run starts in: `--cwd` from where `hc` runs, or that
/// directory. `Ok(None)` on another machine (`--on`) or after a `cd` the
/// guard cannot follow.
fn run_directory(parsed: &Options, place: &Place) -> Result<Option<PathBuf>, String> {
    if parsed.last("--on").is_some() {
        return Ok(None);
    }
    let Some(base) = place.cwd.clone() else {
        return Ok(None);
    };
    let Some(cwd) = parsed.last("--cwd") else {
        return Ok(Some(base));
    };
    cwd_path(cwd, &base, place.home.as_deref(), cfg!(windows))
}

/// The directory `--cwd` names from `base`, read as the host reads it.
/// `Ok(None)` where the guard cannot place it on this host, because it
/// depends on state the guard cannot see.
fn cwd_path(
    cwd: &str,
    base: &Path,
    home: Option<&Path>,
    windows: bool,
) -> Result<Option<PathBuf>, String> {
    if cwd.is_empty() || cwd.contains(['$', '`']) || cwd.chars().any(char::is_control) {
        return Err("its `--cwd` holds an expansion the guard cannot fill in".to_string());
    }
    let bytes = cwd.as_bytes();
    let placed_by_state = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && !matches!(bytes.get(2), Some(b'/' | b'\\'));
    let placed_by_root = cwd.starts_with('\\') && !cwd.starts_with("\\\\");
    if placed_by_state || placed_by_root {
        return Ok(None);
    }
    if windows && cwd.starts_with('/') {
        return Ok(crate::portable_path::git_bash_drive_path(cwd));
    }
    let path = if cwd == "~" {
        home.ok_or("its `--cwd` names a home the guard cannot read")?
            .to_path_buf()
    } else if let Some(rest) = cwd.strip_prefix("~/") {
        home.ok_or("its `--cwd` names a home the guard cannot read")?
            .join(rest)
    } else if cwd.starts_with('~') {
        return Err("its `--cwd` names another user's home".to_string());
    } else {
        base.join(cwd)
    };
    Ok(Some(path))
}

/// The carrier's words from `from` on as shell text that reads back as
/// the same words: a literal word double-quoted with its expansions left
/// live (the guards read `$X` there as they read it bare), and a pattern
/// word left unquoted so the guards expand it as the shell did.
fn quote(words: &Words<'_>, from: usize) -> Result<String, String> {
    let mut out = Vec::new();
    for (at, word) in words.args.iter().enumerate().skip(from) {
        if word.chars().any(char::is_control) {
            return Err("a word holds a substitution the guard cannot fill in".to_string());
        }
        out.push(shell_word(word, words.pattern(at)));
    }
    Ok(out.join(" "))
}

/// One word as shell text: a pattern word with its syntax live, any other
/// double-quoted with its expansions live. A leading `~` stays live either
/// way, since the shell that ran the carrier expanded an unquoted one; a
/// quoted one is judged as the home, the stricter reading.
fn shell_word(word: &str, pattern: bool) -> String {
    let (tilde, rest) = match word.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => ("~", rest),
        _ => ("", word),
    };
    if rest.is_empty() {
        return tilde.to_string();
    }
    if pattern {
        format!("{tilde}{}", escaped_pattern(rest))
    } else {
        format!("{tilde}{}", double_quoted(rest))
    }
}

fn double_quoted(word: &str) -> String {
    format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A pattern word with every character outside the glob, brace and
/// expansion syntax escaped.
fn escaped_pattern(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if c.is_alphanumeric() || "*?[]{}!,-_./=:+@%^~$".contains(c) {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    if out.is_empty() {
        out.push_str("\"\"");
    }
    out
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `herdr pane run <pane> <command>` and `herdr agent start <name> --kind
/// KIND --pane ID [--timeout MS] [-- <agent-args>...]`, which run in a
/// pane whose directory the guard cannot see, and `herdr pane split`, whose
/// `--cwd` and `--env` set a pane's directory and environment. The other
/// `herdr` commands manage panes or type text and run nothing the guard
/// reads.
fn herdr(words: &Words<'_>, out: &mut Carried) {
    let args = words.args;
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("pane"), Some("run")) => {
            const FORM: &str = "herdr pane run";
            let rest = &args[2..];
            if rest.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
                return;
            }
            if let Some(option) = rest.iter().find(|a| a.starts_with('-') && *a != "-") {
                return out.unread(
                    FORM,
                    format!("the option `{option}` is not one the guard reads"),
                );
            }
            if rest.len() < 2 {
                return;
            }
            // The text is typed into the pane's shell, whose directory the
            // guard cannot see.
            out.runs.push(Run {
                form: FORM,
                line: rest[1..].join(" "),
                directory: None,
            });
        }
        (Some("agent"), Some("start")) => {
            const FORM: &str = "herdr agent start";
            let grammar = Grammar {
                flags: &[],
                valued: &["--kind", "--pane", "--timeout"],
            };
            let parsed = match options(args, 2, &grammar, true) {
                Ok(parsed) => parsed,
                Err(why) => return out.unread(FORM, why),
            };
            if parsed.help {
                return;
            }
            let Some(kind) = parsed.last("--kind") else {
                return out.unread(FORM, "it names no `--kind`");
            };
            if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return out.unread(
                    FORM,
                    format!("the kind `{kind}` is not one the guard reads"),
                );
            }
            let agent_args = args
                .iter()
                .position(|a| a == "--")
                .map_or(args.len(), |at| at + 1);
            let line = match quote(words, agent_args) {
                Ok(rest) if rest.is_empty() => kind.to_string(),
                Ok(rest) => format!("{kind} {rest}"),
                Err(why) => return out.unread(FORM, why),
            };
            out.runs.push(Run {
                form: FORM,
                line,
                directory: None,
            });
        }
        (Some("pane"), Some("split")) => {
            if let Some(option) = args[2..].iter().find(|a| {
                matches!(a.as_str(), "--cwd" | "--env")
                    || a.starts_with("--cwd=")
                    || a.starts_with("--env=")
            }) {
                out.unread(
                    "herdr pane split",
                    format!(
                        "`{option}` sets a pane's directory or environment, which later \
                         commands in that pane run under"
                    ),
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(command: &str) -> Carried {
        carried(command, Path::new("/work/repo"))
    }

    fn lines(command: &str) -> Vec<String> {
        let found = read(command);
        assert!(found.unread.is_empty(), "{command}: {:?}", found.unread);
        found.runs.into_iter().map(|run| run.line).collect()
    }

    /// The commands an `hc run` line carries, which are still judged, and a
    /// check that its only refusals are the one every `hc run` takes.
    fn run_lines(command: &str) -> Vec<String> {
        let found = read(command);
        assert!(
            !found.unread.is_empty() && found.unread.iter().all(|u| u.contains(HC_RUN_UNVERIFIED)),
            "{command}: {:?}",
            found.unread
        );
        found.runs.into_iter().map(|run| run.line).collect()
    }

    /// Whether `command`, or a command it carries, takes the refusal every
    /// `hc run` takes.
    fn hc_run_refused(command: &str) -> bool {
        let found = read(command);
        found.unread.iter().any(|u| u.contains(HC_RUN_UNVERIFIED))
            || found.runs.iter().any(|run| hc_run_refused(&run.line))
    }

    #[test]
    fn hc_run_carries_its_program_after_every_option_form() {
        for command in [
            "hc run -- git status",
            "hc run git status",
            "hc run --bg --name x -- git status",
            "hc run --bg --no-parent --no-notify --name x --cwd /tmp -- git status",
            "hc run --outlive-parent --rm --keep --timeout 5 --label a=b -- git status",
            "hc run --name=x --timeout=5 -- git status",
            "hc run --parent t1 --die-with-parent --json -- git status",
            "env A=B hc run -- git status",
            "/opt/bin/hc run -- git status",
            "nice -n 5 hc run -- git status",
            "sh -c 'hc run -- git status'",
            "echo $(hc run -- git status)",
        ] {
            assert_eq!(run_lines(command), vec!["\"git\" \"status\""], "{command}");
        }
        assert_eq!(run_lines("hc run 'git status'"), vec!["git status"]);
    }

    #[test]
    fn hc_run_directory_follows_cwd_and_on() {
        let found = read("hc run --cwd sub -- git status");
        assert_eq!(
            found.runs[0].directory,
            Some(PathBuf::from("/work/repo/sub"))
        );
        let found = read("hc run --cwd /elsewhere -- git status");
        assert_eq!(found.runs[0].directory, Some(PathBuf::from("/elsewhere")));
        // On another machine the directory is unknown, with or without
        // `--cwd`, so every carried command refuses (PR 706 review, F1).
        let words =
            |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_string).collect() };
        for line in [
            "hc run --on vps -- ls",
            "hc run --on vps -- git status",
            "hc run --on vps --cwd / -- rm -rf usr",
            "hc exec --on vps --cwd / -- rm -rf usr",
            "hc exec --on vps --cwd /root/.ssh -- tar -cf - .",
            "hc file read --on vps --cwd /root/.ssh config",
        ] {
            let found = read(line);
            assert_eq!(found.runs[0].directory, None, "{line}");
            assert!(
                found
                    .unread
                    .iter()
                    .any(|u| u.contains("cannot be established")),
                "{line}: {found:?}"
            );
            assert!(!fully_read(&words(line)), "{line}");
        }
        let found = read("hc run --cwd \"$D\" -- ls");
        assert!(!found.unread.is_empty());
    }

    #[test]
    fn hc_exec_carries_its_argv_and_environment() {
        assert_eq!(lines("hc exec git status"), vec!["\"git\" \"status\""]);
        assert_eq!(
            lines("hc exec --cwd /tmp --env A=B --timeout 5 -i git status"),
            vec!["A=\"B\" \"git\" \"status\""]
        );
        assert!(!read("hc exec --env 1=x ls").unread.is_empty());
    }

    #[test]
    fn a_pattern_word_stays_a_pattern() {
        assert_eq!(run_lines("hc run -- rm -rf *"), vec!["\"rm\" \"-rf\" *"]);
        assert_eq!(
            run_lines("hc run -- rm -rf '*'"),
            vec!["\"rm\" \"-rf\" \"*\""]
        );
    }

    #[test]
    fn unknown_forms_and_options_are_unread() {
        for command in [
            "hc frobnicate t1",
            "hc --frob",
            "hc run --frob -- ls",
            "hc exec --frob ls",
            "hc send --frob t1 hi",
            "hc keys --frob t1 enter",
            "hc file shred x",
            "hc server explode",
            "hc machine teleport",
            "hc run --name",
            "herdr pane run --frob p1 ls",
            "herdr agent start x --pane p1 -- -p hi",
            "herdr agent start x --kind claude --frob p1",
        ] {
            assert!(!read(command).unread.is_empty(), "{command}");
        }
    }

    #[test]
    fn read_only_and_typing_forms_carry_nothing() {
        for command in [
            "hc",
            "hc ls",
            "hc ls --all --label a=b",
            "hc get t1",
            "hc read --lines 5 t1",
            "hc --skill",
            "hc --help",
            "hc -V",
            "hc wait t1",
            "hc wait --children any",
            "hc kill t1",
            "hc rm t1",
            "hc attach --observe t1",
            "hc send t1 'rm -rf /'",
            "hc send --if idle t1 hello",
            "hc send --wait --timeout 5 t1 hello",
            "hc keys t1 down enter",
            "hc interrupt t1",
            "hc templates",
            "hc server status",
            "hc machine ls",
            "hc help run",
            "herdr pane send-text p1 'rm -rf /'",
            "herdr agent prompt a1 hello",
            "herdr pane list",
        ] {
            let found = read(command);
            assert_eq!(found, Carried::default(), "{command}");
        }
    }

    #[test]
    fn hc_file_writes_name_their_path() {
        let found = read("hc file write --cwd sub notes/x.txt");
        assert_eq!(
            found.writes,
            vec![Write {
                form: "hc file write",
                path: "notes/x.txt".to_string(),
                directory: PathBuf::from("/work/repo/sub"),
            }]
        );
        let found = read("hc file edit --old a --new b x.txt");
        assert_eq!(found.writes[0].path, "x.txt");
        assert!(!read("hc file write --on vps x.txt").unread.is_empty());
        assert!(!read("hc file write a b").unread.is_empty());
    }

    #[test]
    fn herdr_forms_carry_their_command() {
        // A pane's directory is unknown, so the carried command is read
        // and its form refuses (PR 706 review, F1).
        let found = read("herdr pane run w1:p1 'rm -rf usr'");
        assert_eq!(found.runs[0].line, "rm -rf usr");
        assert_eq!(found.runs[0].directory, None);
        assert!(!found.unread.is_empty());
        let found = read("herdr agent start x --kind claude --pane p1 -- -p hello");
        assert_eq!(found.runs[0].line, "claude \"-p\" \"hello\"");
        assert!(!found.unread.is_empty());
        assert!(!read("herdr agent start x --kind codex --pane p1")
            .unread
            .is_empty());
        for line in [
            "herdr pane split --current --direction right --cwd /",
            "herdr pane split --current --cwd=/",
            "herdr pane split --current --env A=1",
        ] {
            assert!(!read(line).unread.is_empty(), "{line}");
        }
        assert!(read("herdr pane split --current --direction right")
            .unread
            .is_empty());
    }

    #[test]
    fn file_reading_forms_are_judged_as_reads() {
        let found = read("hc file read --cwd sub notes.md");
        assert_eq!(found.runs[0].line, "cat -- notes.md");
        assert_eq!(
            found.runs[0].directory,
            Some(PathBuf::from("/work/repo/sub"))
        );
        // The reader fills in the home the shell expands.
        let home = lines("hc file read ~/.ssh/id_rsa");
        assert!(
            home[0].starts_with("cat -- ") && home[0].ends_with("/.ssh/id_rsa"),
            "{home:?}"
        );
        let remote = read("hc file read --on vps x");
        assert_eq!(remote.runs[0].directory, None);
        assert!(!remote.unread.is_empty());
        assert_eq!(lines("hc put t1 notes.md"), vec!["cat -- notes.md"]);
        assert_eq!(lines("hc update --from ./hc-new"), vec!["cat -- ./hc-new"]);
        assert!(!read("hc put t1").unread.is_empty());
    }

    #[test]
    fn fully_read_needs_a_known_form_with_known_options() {
        let words =
            |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_string).collect() };
        for line in ["hc exec --cwd /tmp ls", "hc ls", "hc"] {
            assert!(fully_read(&words(line)), "{line}");
        }
        // Every `hc run` refuses, so none is fully read.
        for line in ["hc run --bg --name x --cwd /tmp -- codex -c a=b", "hc run"] {
            assert!(!fully_read(&words(line)), "{line}");
        }
        // A pane's directory is unknown, so these are not fully read.
        for line in [
            "herdr pane run p1 ls",
            "herdr agent start x --kind codex --pane p1 -- -c a",
        ] {
            assert!(!fully_read(&words(line)), "{line}");
        }
        for line in [
            "hc run --frob -- ls",
            "hc frobnicate",
            "herdr pane send-text p1 x",
            "herdr agent start x --pane p1",
            "tar -c x",
        ] {
            assert!(!fully_read(&words(line)), "{line}");
        }
    }

    #[test]
    fn machine_add_via_carries_its_command() {
        assert_eq!(
            lines("hc machine add --name m --via 'ssh -p 2222 box'"),
            vec!["ssh -p 2222 box"]
        );
    }

    /// A wrapper that runs the command after its own words keeps the `hc`
    /// call it runs in view: `flock`, `script`, `watch`, `parallel` and
    /// `arch`, the wrappers git-guard reads that the shared traversal had
    /// no branch for (PR 706 round 9, seat C).
    #[test]
    fn wrappers_keep_the_hc_run_they_carry() {
        let mut missed = Vec::new();
        for command in [
            "flock /tmp/review.lock hc run peer",
            "flock -w 5 /tmp/review.lock hc run peer",
            "flock -n /tmp/review.lock -- hc run -- git status",
            "flock /tmp/review.lock -c 'hc run peer'",
            "flock /tmp/review.lock --command='hc run peer'",
            "script -c 'hc run peer' /dev/null",
            "script -q -c 'hc run peer' /dev/null",
            "script --command 'hc run peer' /dev/null",
            "script -q /dev/null hc run peer",
            "watch 'hc run peer'",
            "watch -n 5 hc run peer",
            "parallel hc run ::: peer",
            "arch -arm64 hc run peer",
            "flock /tmp/l flock /tmp/m script -c 'hc run peer' /dev/null",
            "env A=B flock /tmp/l hc run peer",
            "sh -c 'flock /tmp/l hc run peer'",
        ] {
            if !hc_run_refused(command) {
                missed.push(command);
            }
        }
        assert!(missed.is_empty(), "not refused: {missed:#?}");
        // The wrapper alone, and a wrapper around another command, are
        // unaffected.
        for command in [
            "flock /tmp/review.lock git status",
            "script -c 'git status' /dev/null",
            "watch -n 5 ls",
            "arch -arm64 ls",
            "flock /tmp/review.lock hc ls",
            "script -c 'hc exec --cwd /tmp ls' /dev/null",
        ] {
            assert!(!hc_run_refused(command), "refused: {command}");
        }
        // A carried `hc exec` is kept in view too, with its directory.
        let found = read("flock /tmp/review.lock hc exec --cwd sub git status");
        assert_eq!(
            found.runs[0].directory,
            Some(PathBuf::from("/work/repo/sub"))
        );
        let found = read("script -c 'hc exec --on vps ls' /dev/null");
        assert!(!found.unread.is_empty());
    }

    /// Each wrapper on the key 18 carrier list keeps the `hc run` it runs in
    /// view, one payload per wrapper.
    #[test]
    fn the_carrier_list_wrappers_keep_the_hc_run_they_carry() {
        let mut missed = Vec::new();
        for command in [
            "find . -maxdepth 0 -exec hc run peer \\;",
            "find . -maxdepth 0 -execdir hc run peer \\;",
            "find . -maxdepth 0 -ok hc run peer \\;",
            "echo x | xargs hc run peer",
            "env hc run peer",
            "env -i A=B hc run peer",
            "sudo hc run peer",
            "sudo -u root hc run peer",
            "doas hc run peer",
            "doas -u root hc run peer",
            "gsudo hc run peer",
            "pkexec hc run peer",
            "busybox hc run peer",
            "nice hc run peer",
            "nice -n 5 hc run peer",
            "nohup hc run peer",
            "time hc run peer",
            "timeout 5 hc run peer",
            "stdbuf -oL hc run peer",
            "setsid hc run peer",
            "setsid -f hc run peer",
            "chrt 1 hc run peer",
            "chrt -f 1 hc run peer",
            "ionice -c 3 hc run peer",
            "taskset 1 hc run peer",
            "taskset -c 0 hc run peer",
            "unbuffer hc run peer",
            "caffeinate hc run peer",
            "command hc run peer",
            "exec hc run peer",
            "xcrun hc run peer",
        ] {
            if !hc_run_refused(command) {
                missed.push(command);
            }
        }
        assert!(missed.is_empty(), "not refused: {missed:#?}");
        // Behind a wrapper whose directory the guard cannot read, a carried
        // `hc exec` has no known directory and refuses.
        for command in ["taskset 1 hc exec ls", "sudo hc exec ls"] {
            assert!(
                read(command)
                    .unread
                    .iter()
                    .any(|u| u.contains("cannot be established")),
                "{command}"
            );
        }
        // A wrapper around a command that is not a carrier is unaffected.
        for command in ["sudo ls", "taskset 1 ls", "chrt 1 ls", "doas ls"] {
            assert_eq!(read(command), Carried::default(), "{command}");
        }
    }

    #[test]
    fn supplied_words_are_unread() {
        assert!(!read("echo x | xargs hc run -- ls").unread.is_empty());
    }

    /// Which template `hc run` starts depends on state the guard cannot
    /// establish from the command, and no form is known to skip it, so
    /// every `hc run` refuses: the seat launch lines, plain and template
    /// forms, and every line an earlier reading let through.
    #[test]
    #[allow(clippy::too_many_lines)] // One table of payload lines.
    fn every_hc_run_refuses() {
        let codex = "codex --no-daemon --model gpt-6.1-sol -c 'model_reasoning_effort=\"medium\"' --ask-for-approval never";
        let grok = "grok --model grok-4.7 --reasoning-effort medium --permission-mode auto";
        let mut refusing: Vec<String> = Vec::new();
        for outlive in ["", "--outlive-parent "] {
            for seat in [codex, grok] {
                refusing.push(format!(
                    "hc run --bg --no-parent --no-notify {outlive}--name cf-x --cwd /work/repo -- {seat}"
                ));
            }
        }
        refusing.extend(
            [
                // Plain, program and template forms.
                "hc run",
                "hc run --bg",
                "hc run peer",
                "hc run --bg peer 'review x.rs'",
                "hc run -- git status",
                "hc run -- /usr/bin/git status",
                "hc run 'git status'",
                "herdr-core run -- ls",
                "/opt/bin/hc run -- ls",
                // Other commands on the line, earlier or later.
                "X=1 hc run peer",
                "X=/opt/x hc run peer",
                "cd x && hc run peer",
                "pushd x; hc run peer",
                "f; hc run peer",
                "hc run peer; echo x",
                "(hc run peer)",
                "echo | hc run peer",
                "export APPDATA=/x\nhc run peer",
                "declare -n X=APPDATA; X=/opt/alternate; hc run peer",
                // Lines an earlier reading of the template folder let through.
                "APPDATA=$X hc run peer",
                "APPDATA=relative/dir hc run peer",
                "AppData=~/x hc run peer",
                "hc exec --env 'AppData=$X' -- hc run peer",
                "hc exec --env AppData=rel -- hc run peer",
                "read APPDATA; hc run peer",
                "read -r -p prompt AppData; hc run peer",
                ": ${APPDATA:=rel}; hc run peer",
                ": \"${APPDATA:=rel}\"; hc run peer",
                ": ${APPDATA=rel}; hc run peer",
                "printf -v APPDATA %s rel; hc run peer",
                "mapfile -t APPDATA < list; hc run peer",
                "getopts ab APPDATA; hc run peer",
                "APPDATA+=/x; hc run peer",
                "APPDATA[0]=/x; hc run peer",
                "export \"$N\"=/x; hc run peer",
                "declare -n APPDATA=other; hc run peer",
                "let APPDATA=1; hc run peer",
                ": ${APP\\\nDATA:=rel}; hc run peer",
                "export APPDATA; declare -n ref=APPDATA; ref=/tmp/alternate; hc run peer",
                "declare -n ref; hc run peer",
                "getln APPDATA; hc run peer",
                "vared APPDATA; hc run peer",
                "(( APPDATA = 1 )); hc run peer",
                "frob \"$N\"; hc run peer",
                "eval \"$X\"; hc run peer",
                "source ./env.sh; hc run peer",
                "export APPDATA; [[ 'APPDATA=1' -eq 1 ]]; hc run peer",
                "export APPDATA; declare -n ref='arr[APPDATA=1]'; ref=x; hc run peer",
                "(( v )); hc run peer",
                "for (( ; v; )); do :; done; hc run peer",
                "echo $(( v )); hc run peer",
                "echo $[ v ]; hc run peer",
                "let v; hc run peer",
                "test v -eq 1; hc run peer",
                "[ v -lt 1 ]; hc run peer",
                "a[v]=1; hc run peer",
                "echo ${a[v]}; hc run peer",
                "unset 'a[v]'; hc run peer",
                "r='a[v]'; echo ${!r}; hc run peer",
                "printf -v 'a[v]' %s x; hc run peer",
                "read 'a[v]' < list; hc run peer",
                "declare -i n=v; hc run peer",
                "declare -i n; n+=v; hc run peer",
                "echo ${s:v}; hc run peer",
                "echo ${s:0:v}; hc run peer",
                "a=([v]=x); hc run peer",
                "i=0; (( i++ )); hc run peer",
                "i=0; read i; (( i++ )); hc run peer",
                "false && i=0; (( i++ )); hc run peer",
                "i=0 & (( i++ )); hc run peer",
                "i=0; read i < values; (( i )); hc run peer",
                "i=0; (( i < 3 )); hc run peer",
                "i=0; i[0]=v; (( i++ )); hc run peer",
                "i=0; declare -n r=i; r=v; (( i++ )); hc run peer",
                "set -- abc; echo ${1:v}; hc run peer",
                "echo ${@:v}; hc run peer",
                "eval '(( v ))'; hc run peer",
                "trap '(( v ))' DEBUG; hc run peer",
                "echo \"$(printf \"%s\" \"don't\")\"; (( v )); hc run peer",
                "echo $'\\''; (( v )); hc run peer",
                "true # don't\n(( v )); hc run peer",
                "cat <<EOF\ndon't\nEOF\n(( v )); hc run peer",
                "export APPDATA; integer n; read n < values; hc run peer",
                "declare -i n; mapfile n < values; hc run peer",
                "typeset -ix n; printf -v n %s x; hc run peer",
                "local -i n; getopts ab n; hc run peer",
                "float n; for n in x; do :; done; hc run peer",
                "typeset -E n; read n; hc run peer",
                "read SECONDS < values; hc run peer",
                "read n < values; hc run peer",
                "mapfile n < values; hc run peer",
                "printf -v n %s x; hc run peer",
                "print -v n x; hc run peer",
                "getopts ab n; hc run peer",
                "for n in x; do :; done; hc run peer",
                "select n in x; do :; done; hc run peer",
                "wait -p n; hc run peer",
                "set -A n x; hc run peer",
                "zparseopts -a n x; hc run peer",
                "compgen -V n -W x; hc run peer",
                ": ${SECONDS=v}; hc run peer",
                ": ${SECONDS:=v}; hc run peer",
                ": ${SECONDS::=v}; hc run peer",
                "SECONDS=v; hc run peer",
                "MODE=fast hc run peer",
                "export SECONDS=v; hc run peer",
                "FOO=$BAR hc run -- ls",
                "read MY_APPDATA; hc run -- ls",
            ]
            .map(str::to_string),
        );
        let passed: Vec<_> = refusing
            .iter()
            .filter(|command| !hc_run_refused(command))
            .collect();
        assert!(passed.is_empty(), "not refused: {passed:?}");
        // Help words refuse too: the option grammar cannot tell a help flag
        // from an operand or an option's value.
        for command in [
            "hc run --help",
            "hc run -h",
            "hc run --bg --help",
            "hc run -- peer --help",
            "hc run peer --help",
            "hc run --name --help peer",
        ] {
            assert!(hc_run_refused(command), "not refused: {command}");
        }
        // A line without `hc run` is unaffected.
        for command in [
            "git status",
            "echo hc run",
            "hc exec --cwd /tmp ls",
            "hc ls",
        ] {
            assert!(!hc_run_refused(command), "refused: {command}");
        }
        assert_eq!(read("git status"), Carried::default());
    }

    /// Each way the host finds a carrier program names the same carrier.
    #[test]
    fn carrier_names_match_as_the_host_finds_them() {
        let native = ["C:", "tools", "hc.exe"].join("\\");
        for command in [
            "HC run -- git status".to_string(),
            "HC.EXE run -- git status".to_string(),
            "Hc.Exe run -- git status".to_string(),
            "/opt/bin/HC.exe run -- git status".to_string(),
            "Herdr-Core.EXE run -- git status".to_string(),
            format!("'{native}' run -- git status"),
            format!("\"{native}\" run -- git status"),
        ] {
            assert_eq!(run_lines(&command), vec!["\"git\" \"status\""], "{command}");
        }
        let found = read("HERDR.exe pane run p1 ls");
        assert_eq!(found.runs[0].line, "ls");
        let words = |line: &str| -> Vec<String> { vec![line.to_string(), "ls".to_string()] };
        for program in ["HC.EXE", native.as_str()] {
            assert!(fully_read(&words(program)), "{program}");
        }
    }

    /// `--cwd` is read as the host reads it, and one the guard cannot
    /// place on the host is unknown.
    #[test]
    fn cwd_follows_the_host_paths() {
        let base = Path::new("/work/repo");
        let windows = |cwd: &str| cwd_path(cwd, base, None, true);
        assert_eq!(
            windows("/c/repo"),
            Ok(crate::portable_path::git_bash_drive_path("/c/repo"))
        );
        assert_eq!(windows("/workspace"), Ok(None));
        assert_eq!(windows("sub"), Ok(Some(base.join("sub"))));
        // On the other hosts they are ordinary paths.
        let posix = |cwd: &str| cwd_path(cwd, base, None, false);
        assert_eq!(posix("/c/repo"), Ok(Some(PathBuf::from("/c/repo"))));
        assert_eq!(posix("/workspace"), Ok(Some(PathBuf::from("/workspace"))));
    }

    /// A directory that depends on state the guard cannot see is unknown,
    /// and the form refuses.
    #[test]
    fn cwd_the_guard_cannot_place_is_unknown() {
        for cwd in ["C:foo", "c:", "\\foo"] {
            let found = read(&format!("hc run --cwd '{cwd}' -- true"));
            assert_eq!(found.runs[0].directory, None, "{cwd}");
            assert!(
                found
                    .unread
                    .iter()
                    .any(|u| u.contains("cannot be established")),
                "{cwd}: {found:?}"
            );
            let found = read(&format!("hc file write --cwd '{cwd}' x.txt"));
            assert!(found.writes.is_empty() && !found.unread.is_empty(), "{cwd}");
        }
    }

    #[test]
    fn the_policy_reference_lists_the_carrier_programs() {
        let listed = crate::security::launchers::policy_reference_list(
            "### Terminal carriers the guards model",
            "Programs",
        );
        let code: Vec<&str> = HC_PROGRAMS.iter().chain(HERDR_PROGRAMS).copied().collect();
        assert_eq!(listed, code);
    }

    #[test]
    fn nesting_stops_at_the_limit() {
        let guards: Vec<_> = (0..DEPTH_LIMIT).map(|_| Nested::enter()).collect();
        assert!(guards.iter().all(Option::is_some));
        assert!(Nested::enter().is_none());
        drop(guards);
        assert!(Nested::enter().is_some());
    }
}
