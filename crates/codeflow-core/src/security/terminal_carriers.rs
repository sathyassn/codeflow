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
//! template it cannot read, a directory it cannot establish for a Git
//! command, and nesting past [`DEPTH_LIMIT`].
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
    let assigned = assigned_template_roots(command, cfg!(windows));
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
                assigned: assigned.clone(),
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
    /// The configuration folders the line assigns to the variables `hc`
    /// reads its template folders from ([`assigned_template_roots`]), or
    /// why the guard cannot read one.
    assigned: Result<Vec<PathBuf>, String>,
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
    if parsed.help {
        return;
    }
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
        directory: directory.clone(),
    });
    // The first word may name a template, whose command runs instead.
    match template_command(&carried_words[0], place.home.as_deref(), &place.assigned) {
        Ok(Some(command)) => out.runs.push(Run {
            form: "hc run <template>",
            line: command,
            directory,
        }),
        Ok(None) => {}
        Err(why) => out.unread("hc run <template>", why),
    }
}

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
        assigned: Ok(Vec::new()),
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

/// The variables whose folder `hc` reads templates under.
const TEMPLATE_VARIABLES: &[&str] = &["XDG_CONFIG_HOME", "APPDATA"];

/// The template folders `hc` reads (`hc templates --help`), under each
/// configuration home it may use: the guard's own environment, and each
/// folder the line assigns to one of [`TEMPLATE_VARIABLES`].
fn template_dirs(home: Option<&Path>, assigned: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(config).join("herdr-core/templates"));
    }
    if let Some(home) = home {
        dirs.push(home.join(".config/herdr-core/templates"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(appdata).join("herdr-core/templates"));
    }
    for root in assigned {
        let dir = root.join("herdr-core/templates");
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Every folder the line assigns to one of [`TEMPLATE_VARIABLES`], so a
/// carried `hc run` reads the templates its process would. Each simple
/// command the line runs, nested bodies included, is read as the words the
/// shell passes, after quote removal ([`template_assignments`]): an
/// assignment in front of a command or a launcher (`env`), through an
/// export builtin, or as a statement of its own, and `hc exec --env`, whose
/// assignments a carried line holds as prefixes. A word that only looks
/// like one, as data a command receives, is not read. A name matches the
/// way the host reads variable names, and every value is judged. A value
/// the guard cannot read as one absolute folder on this host, or a form
/// that sets one it cannot read, is an error, which refuses a template
/// lookup. Every command on the line counts, whether or not its
/// assignment reaches `hc`, the stricter reading.
fn assigned_template_roots(command: &str, windows: bool) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    // A here-document body is not one of the line's commands, yet the
    // shell expands an unquoted one, where a default may assign.
    if command.contains("<<") && names_template_variable(command) {
        return Err(TEMPLATE_UNREAD.to_string());
    }
    let line = crate::hooks::git_guard::join_continuations(command);
    let mut arithmetic = Arithmetic::default();
    if !arithmetic.scan(&line) {
        return Err(TEMPLATE_UNREAD.to_string());
    }
    let mut assigned = Vec::new();
    for segment in crate::hooks::git_guard::expand_commands(command) {
        let (mut words, mut expanding) = crate::hooks::git_guard::command_argv_expanding(&segment);
        let all = words.len();
        crate::hooks::git_guard::strip_reserved_words(&mut words);
        expanding.drain(..all - words.len());
        let tokens = crate::hooks::git_guard::shell_tokens_expanding(&segment);
        // A decoded word can hold arithmetic the line's text does not show.
        if !tokens.iter().all(|(text, _)| arithmetic.scan(text)) {
            return Err(TEMPLATE_UNREAD.to_string());
        }
        let found = template_assignments(&words, &expanding, &tokens, &mut arithmetic)?;
        assigned.push(found.into_iter().cloned().collect::<Vec<String>>());
    }
    if !arithmetic.literal() {
        return Err(TEMPLATE_UNREAD.to_string());
    }
    for word in assigned.iter().flatten() {
        let Some((name, value)) = word.split_once('=') else {
            continue;
        };
        let Some(variable) = template_variable(name) else {
            continue;
        };
        let unread = || {
            format!(
                "`{variable}` is assigned a value the guard cannot read as one absolute \
                 folder, so it cannot tell which templates `hc` reads"
            )
        };
        // An appended or indexed value joins text the guard cannot see.
        if destination_base(name) != name {
            return Err(unread());
        }
        if value.is_empty() {
            continue;
        }
        if value.contains(['$', '`']) || value.chars().any(char::is_control) {
            return Err(unread());
        }
        roots.push(literal_root(value, windows).ok_or_else(unread)?);
    }
    Ok(roots)
}

/// The variable an assignment destination sets: `NAME`, `NAME+` (append)
/// and `NAME[index]` (an element, which for index 0 is the variable's own
/// value) all set `NAME`.
fn destination_base(name: &str) -> &str {
    let name = name.strip_suffix('+').unwrap_or(name);
    name.split_once('[').map_or(name, |(base, _)| base)
}

/// Whether `name` names one of [`TEMPLATE_VARIABLES`], read as the host
/// reads variable names.
fn template_variable(name: &str) -> Option<&'static str> {
    let base = destination_base(name);
    TEMPLATE_VARIABLES
        .iter()
        .copied()
        .find(|variable| variable.eq_ignore_ascii_case(base))
}

/// Whether a destination is one the guard cannot read: its name holds an
/// expansion, so it may be any variable.
fn unread_destination(name: &str) -> bool {
    name.contains(['$', '`'])
}

/// The options and operands of a builtin's words: each option letter that
/// takes a value (from `valued`), with its value attached or in the next
/// word, and the operands after the options or `--`.
fn builtin_words<'w>(rest: &'w [String], valued: &str) -> (Vec<(char, &'w str)>, Vec<&'w str>) {
    let mut taken = Vec::new();
    let mut operands = Vec::new();
    let mut options = true;
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at += 1;
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && word.len() > 1 && word.starts_with('-') {
            for (offset, letter) in word[1..].char_indices() {
                if valued.contains(letter) {
                    let attached = &word[1 + offset + letter.len_utf8()..];
                    let value = if attached.is_empty() {
                        at += 1;
                        rest.get(at - 1).map_or("", String::as_str)
                    } else {
                        attached
                    };
                    taken.push((letter, value));
                    break;
                }
            }
            continue;
        }
        options = false;
        operands.push(word.as_str());
    }
    (taken, operands)
}

/// The refusal of a template lookup when a command on the line can set a
/// template folder variable in a form the guard cannot read.
const TEMPLATE_UNREAD: &str = "a command on the line can set a template folder variable in a \
     form the guard cannot read, so it cannot tell which templates `hc` reads";

/// The refusal of a template lookup when a command on the line writes a
/// variable with a value the guard cannot rule out ([`writes_variable`],
/// [`numeric_safe`]), with the forms that clear it.
const TEMPLATE_WRITE: &str = "a command on the line writes a shell variable with a value the \
     guard cannot read, so it cannot tell which templates `hc` reads; set the variable to a \
     literal number or an absolute path, or set it on another line";

/// Programs that set no shell variable, so their operands are data: plain
/// programs, test and no-op builtins, and the shells, launchers and
/// carriers that run their commands in a process of their own. Any other
/// command whose words name a template folder variable, or hold an
/// expansion, refuses a template lookup ([`template_assignments`]). The
/// test builtins evaluate the operands of their numeric comparisons, which
/// [`Arithmetic`] judges. None of these writes a variable through an
/// option; the builtins that do are refused by [`writes_variable`].
const NON_WRITERS: &[&str] = &[
    "echo",
    "test",
    "[",
    "[[",
    "true",
    "false",
    ":",
    "cd",
    "pushd",
    "popd",
    "pwd",
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "sort",
    "uniq",
    "diff",
    "grep",
    "sed",
    "awk",
    "find",
    "xargs",
    "rm",
    "cp",
    "mv",
    "mkdir",
    "touch",
    "tar",
    "sleep",
    "date",
    "git",
    "gh",
    "ssh",
    "claude",
    "codex",
    "nice",
    "nohup",
    "timeout",
    "sudo",
    "doas",
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "hc",
    "herdr-core",
    "herdr",
];

/// The positions just after each occurrence of `variable` in the
/// normalized `text` that starts a name (nothing of a name before it).
fn name_ends<'t>(text: &'t str, variable: &'t str) -> impl Iterator<Item = usize> + 't {
    text.match_indices(variable).filter_map(move |(at, _)| {
        let starts = text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        starts.then_some(at + variable.len())
    })
}

/// Whether `text` holds one of [`TEMPLATE_VARIABLES`] as a whole name, read
/// as the host reads variable names.
fn names_template_variable(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    TEMPLATE_VARIABLES.iter().any(|variable| {
        let variable = variable.to_ascii_lowercase();
        let whole = name_ends(&lower, &variable).any(|end| {
            !lower[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        whole
    })
}

/// Whether a word of a command the guard does not know may set one of
/// [`TEMPLATE_VARIABLES`]: the name itself, or the name followed by `=`,
/// `+=` or `[`.
fn may_set_template_variable(word: &str) -> bool {
    if template_variable(word).is_some_and(|_| destination_base(word) == word) {
        return true;
    }
    let lower = word.to_ascii_lowercase();
    TEMPLATE_VARIABLES.iter().any(|variable| {
        let variable = variable.to_ascii_lowercase();
        let sets = name_ends(&lower, &variable).any(|end| {
            let after = &lower[end..];
            after.starts_with('=') || after.starts_with("+=") || after.starts_with('[')
        });
        sets
    })
}

/// Whether an expanding word may assign one of [`TEMPLATE_VARIABLES`] as
/// it expands: a `${NAME=...}`, `${NAME:=...}` or `${NAME[...` form, or
/// arithmetic that names the variable.
fn expansion_assigns(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let arithmetic = lower.contains("$((") || lower.contains("$[");
    TEMPLATE_VARIABLES.iter().any(|variable| {
        let variable = variable.to_ascii_lowercase();
        ["=", ":=", "["]
            .iter()
            .any(|after| lower.contains(&format!("${{{variable}{after}")))
    }) || (arithmetic && names_template_variable(text))
}

/// The words of one simple command that assign a variable for the program
/// it runs or for the commands after it: each `=` word before the program
/// its launchers run ([`crate::hooks::git_guard::strip_launchers`]), each
/// operand an export builtin receives, and every word of a command that is
/// only assignments. `expanding` and `tokens` carry the shared reader's
/// mark for each word the shell fills in, `tokens` with redirections too.
///
/// The template lookup refuses when a command can set one of
/// [`TEMPLATE_VARIABLES`] in a way the guard cannot read: an assignment
/// whose destination holds an expansion; an expansion that assigns as it
/// expands; a name reference whose target is, or may be, one of them; and
/// any command outside [`NON_WRITERS`] and the builtins read here whose
/// words name one or hold an expansion. Names match whole.
///
/// It also refuses where a write may be evaluated as arithmetic, since the
/// guard cannot know whether a destination is numeric: any builtin that
/// writes a variable, whatever the destination ([`writes_variable`]), and
/// an assignment, as a statement, a command prefix or a declaration, whose
/// value is not a literal number or absolute path ([`numeric_safe`]).
fn template_assignments<'w>(
    words: &'w [String],
    expanding: &[bool],
    tokens: &[(String, bool)],
    arithmetic: &mut Arithmetic,
) -> Result<Vec<&'w String>, String> {
    let unread = || Err(TEMPLATE_UNREAD.to_string());
    if tokens
        .iter()
        .any(|(text, fills)| *fills && expansion_assigns(text))
    {
        return unread();
    }
    let at = match crate::hooks::git_guard::strip_launchers(words) {
        Some((_, args)) => words.len() - args.len() - 1,
        None => words.len(),
    };
    let mut found = Vec::new();
    // The assignments before the first other word are the shell's own; a
    // launcher's (`env A=B`) set only its program's environment.
    let mut shell = true;
    for word in &words[..at] {
        let Some((name, value)) = word.split_once('=') else {
            shell = false;
            continue;
        };
        if unread_destination(name) {
            return unread();
        }
        if shell && !numeric_safe(value) {
            return Err(TEMPLATE_WRITE.to_string());
        }
        arithmetic.assignment(word);
        found.push(word);
    }
    let Some(program) = words.get(at) else {
        return Ok(found);
    };
    let rest = &words[at + 1..];
    // A program word holding `=` is an assignment the launcher reading did
    // not take as one, so it is judged as a command the guard does not know.
    let name = if program.contains('=') {
        ""
    } else {
        crate::hooks::git_guard::launcher_name(program)
    };
    // Builtins that evaluate their operands as arithmetic.
    if matches!(
        name,
        "let" | "repeat" | "shift" | "return" | "exit" | "break" | "continue"
    ) {
        arithmetic.expressions.extend(rest.iter().cloned());
    }
    if matches!(name, "[[" | "[" | "test") {
        arithmetic.comparison(rest);
    }
    if !code_read(&words[at..], arithmetic) {
        return unread();
    }
    if writes_variable(name, rest) {
        return Err(TEMPLATE_WRITE.to_string());
    }
    match name {
        "export" | "declare" | "typeset" | "local" | "readonly" | "integer" | "float" => {
            found.extend(declared(name, rest, arithmetic)?);
        }
        "printf" => {}
        known if NON_WRITERS.contains(&known) => {}
        // Any other command, a builtin or function that may write a
        // variable among them, refuses when a word names one or the guard
        // cannot read what a word becomes.
        _ => {
            let words_unread = words[at..]
                .iter()
                .zip(&expanding[at..])
                .any(|(word, fills)| *fills || may_set_template_variable(word));
            if words_unread {
                return unread();
            }
            // It may write any word it receives as a variable, and the
            // shell evaluates the subscript of a name it sets. A word
            // holding `=` is read as an assignment, which an appending one
            // in program position is.
            for word in &words[at..] {
                arithmetic.assignment(word);
            }
        }
    }
    Ok(found)
}

/// Whether the builtin `name` writes a shell variable with its words
/// (`rest`), whatever the destination. The guard cannot know a
/// destination's attributes: a variable may already be numeric with no
/// declaration on the line (a shell's own numeric parameters, or an
/// attribute set earlier in the session), and a value
/// written to one is evaluated as arithmetic, which may read and assign
/// any name. So every such write refuses a template lookup: the readers
/// and loop variables, and the builtins that write through an option.
fn writes_variable(name: &str, rest: &[String]) -> bool {
    let flagged = |letters: &[char]| {
        rest.iter().any(|word| {
            word.len() > 1 && word.starts_with(['-', '+']) && word[1..].contains(letters)
        })
    };
    match name {
        "read" | "mapfile" | "readarray" | "getopts" | "for" | "select" | "vared" | "getln"
        | "zparseopts" | "zformat" | "zregexparse" | "sysread" | "zselect" | "ztcp" | "zsocket"
        | "zpty" | "zstyle" | "zle" | "compadd" | "private" => true,
        "printf" | "print" => flagged(&['v']),
        "wait" => flagged(&['p']),
        "set" => flagged(&['A']),
        "strftime" => flagged(&['s']),
        "stat" | "zstat" => flagged(&['A', 'H']),
        "compgen" => flagged(&['V']),
        _ => false,
    }
}

/// Whether an assigned `value` cannot reach arithmetic when its
/// destination is numeric: a literal number expression
/// ([`literal_expression`]), or a literal absolute path, which numeric
/// evaluation rejects at its leading `/` before it reads any name. Any
/// other value refuses a template lookup, for every destination, because
/// the destination may already be numeric ([`writes_variable`]). An
/// `hc run` line that sets another variable to a word, such as
/// `MODE=fast hc run peer`, refuses: an accepted over-refusal.
fn numeric_safe(value: &str) -> bool {
    let path = value.starts_with('/')
        && !value.contains(['$', '`', '~', '(', ')', '{', '}', '*', '?', '[', ']', '\\'])
        && !value.chars().any(char::is_control);
    path || literal_expression(value)
}

/// Whether the code a command (`words`, from its program on) has the
/// current shell run is read. An `eval` body, which the line reader also
/// splits into commands, holds arithmetic its words no longer mark, so it
/// is read as code. A trap or scheduled action runs at a time the guard
/// cannot place, and a sourced file or a callback runs code it cannot
/// see: those are not read.
fn code_read(words: &[String], arithmetic: &mut Arithmetic) -> bool {
    let code = crate::hooks::git_guard::builtin_words(words);
    let Some(program) = code.first() else {
        return true;
    };
    let rest = &code[1..];
    match crate::hooks::git_guard::launcher_name(program) {
        "eval" => arithmetic.scan(&rest.join(" ")),
        // Listing (`-p`, `-l`) or resetting (`-`) a trap sets no action.
        "trap" => matches!(
            rest.first().map(String::as_str),
            None | Some("-p" | "-l" | "-")
        ),
        "sched" => rest.iter().all(|word| word.starts_with('-')),
        "source" | "." => false,
        "mapfile" | "readarray" => !builtin_words(rest, "dnOsuCc")
            .0
            .iter()
            .any(|(letter, _)| *letter == 'C'),
        _ => true,
    }
}

/// The assignments a declaration builtin (`name`) makes with its words
/// (`rest`), for [`template_assignments`]. A name reference whose target
/// is not a plain name, or is or may be a template folder variable,
/// refuses, and so do a destination that holds an expansion and an
/// integer or float attribute.
fn declared<'w>(
    name: &str,
    rest: &'w [String],
    arithmetic: &mut Arithmetic,
) -> Result<Vec<&'w String>, String> {
    let unread = || Err(TEMPLATE_UNREAD.to_string());
    let flags: Vec<&String> = rest
        .iter()
        .take_while(|word| word.starts_with(['-', '+']) && *word != "--")
        .collect();
    // `export -n` removes the export mark; elsewhere `-n` makes a name
    // reference.
    let reference = name != "export"
        && name != "readonly"
        && flags
            .iter()
            .any(|flag| flag.starts_with('-') && flag.contains('n'));
    // Every value later written to an integer or float variable, by any
    // builtin and with no arithmetic syntax on the line, is evaluated as
    // arithmetic, so giving any variable such an attribute refuses.
    let integer = matches!(name, "integer" | "float")
        || flags
            .iter()
            .any(|flag| flag.starts_with('-') && flag.contains(['i', 'E', 'F']));
    if integer {
        return unread();
    }
    let mut found = Vec::new();
    for word in &rest[flags.len()..] {
        let (destination, target) = match word.split_once('=') {
            Some((destination, target)) => (destination, Some(target)),
            None => (word.as_str(), None),
        };
        if unread_destination(destination) {
            return unread();
        }
        arithmetic.assignment(word);
        if reference {
            // Only a reference to a plain name the guard can see is read:
            // a template folder variable as its target, or a subscript the
            // shell evaluates, refuses.
            let target_unread = target
                .is_none_or(|target| !plain_name(target) || template_variable(target).is_some());
            if target_unread || template_variable(destination).is_some() {
                return unread();
            }
            continue;
        }
        if let Some(value) = target {
            // A declared value is written as an assignment's is.
            if !numeric_safe(value) {
                return Err(TEMPLATE_WRITE.to_string());
            }
            found.push(word);
        }
    }
    Ok(found)
}

/// Whether `text` is a plain variable name, with no subscript or expansion.
fn plain_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The comparison operators of `[[`, `test` and `[` whose operands the
/// shell evaluates as arithmetic.
const ARITHMETIC_TESTS: &[&str] = &["-eq", "-ne", "-lt", "-le", "-gt", "-ge"];

/// What the shell evaluates as arithmetic on one line, where an assignment
/// may hide: each expression it evaluates, read from the shared reader's
/// words and from the line's text. Any name read in arithmetic is
/// evaluated in turn when its value is an expression, an inherited value
/// included, so the line is read only when every expression holds literal
/// numbers and operators alone ([`Self::literal`]). A line that gives any
/// variable an integer or float attribute refuses before this
/// ([`declared`]).
#[derive(Default)]
struct Arithmetic {
    /// Each expression the shell evaluates.
    expressions: Vec<String>,
}

impl Arithmetic {
    /// Read the arithmetic contexts in `text`, a line or one decoded word:
    /// `((`, `$((` and `$[` bodies, and the subscripts, offsets and
    /// indirect names of parameter expansions. Quotes are not read, so
    /// quoted data that holds such a form is read as code, the stricter
    /// reading. `false` when a context does not close, or a parameter
    /// names its target indirectly or takes a form the reader does not
    /// parse.
    fn scan(&mut self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let mut at = 0;
        while at < chars.len() {
            let next = chars.get(at + 1).copied();
            match (chars[at], next) {
                ('(', Some('(')) => {
                    let Some((body, end)) = closing(&chars, at + 2, '(', ')', true) else {
                        return false;
                    };
                    self.expressions.push(body);
                    at = end;
                }
                ('$', Some('[')) => {
                    let Some((body, end)) = closing(&chars, at + 2, '[', ']', false) else {
                        return false;
                    };
                    self.expressions.push(body);
                    at = end;
                }
                ('$', Some('{')) => match self.parameter(&chars, at + 2) {
                    Some(end) => at = end,
                    None => return false,
                },
                // zsh reads `$name[i]` as an element.
                ('$', Some(c)) if c.is_ascii_alphabetic() || c == '_' => {
                    let end = name_end(&chars, at + 1);
                    if chars.get(end) == Some(&'[') {
                        let Some((body, after)) = closing(&chars, end + 1, '[', ']', false) else {
                            return false;
                        };
                        self.expressions.push(body);
                        at = after;
                    } else {
                        at = end;
                    }
                }
                _ => at += 1,
            }
        }
        true
    }

    /// Read the parameter expansion whose name starts at `at`, just after
    /// `${`: the position to read on from, or `None` for an indirect name
    /// or flags that can name another variable.
    fn parameter(&mut self, chars: &[char], mut at: usize) -> Option<usize> {
        match chars.get(at) {
            Some('!') => {
                // Only listing names or keys is read; an indirect value
                // may name a subscript the shell evaluates.
                let end = name_end(chars, at + 1);
                let after: String = chars[end..].iter().take(4).collect();
                let listing = end > at + 1
                    && ["*}", "@}", "[@]}", "[*]}"]
                        .iter()
                        .any(|form| after.starts_with(form));
                return listing.then_some(end);
            }
            Some('(') => return None,
            // A length (`${#name}`), unless `#` is the parameter itself.
            Some('#') if chars.get(at + 1).is_some_and(|c| *c != '}' && *c != ':') => at += 1,
            _ => {}
        }
        // A name, a positional parameter or a special one; any other form
        // is not read.
        let end = match chars.get(at) {
            Some(c) if c.is_ascii_digit() => {
                at + chars[at..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count()
            }
            Some('@' | '*' | '#' | '?' | '-' | '$' | '!') => at + 1,
            _ => name_end(chars, at),
        };
        if end == at {
            return None;
        }
        at = end;
        if chars.get(at) == Some(&'[') {
            let (body, after) = closing(chars, at + 1, '[', ']', false)?;
            if body != "@" && body != "*" {
                self.expressions.push(body);
            }
            at = after;
        }
        // `${name=word}`, `${name:=word}` and `${name::=word}` assign the
        // word to the name, which may be numeric ([`writes_variable`]).
        let assigns = match chars.get(at) {
            Some('=') => true,
            Some(':') => {
                chars.get(at + 1) == Some(&'=') || chars[at + 1..].starts_with(&[':', '='])
            }
            _ => false,
        };
        if assigns {
            return None;
        }
        let modifier = matches!(chars.get(at + 1), Some('-' | '=' | '?' | '+'));
        if chars.get(at) == Some(&':') && !modifier {
            let (body, after) = closing(chars, at + 1, '{', '}', false)?;
            self.expressions
                .extend(body.split(':').map(ToString::to_string));
            return Some(after);
        }
        Some(at)
    }

    /// Record an assignment word, with the subscript of its destination
    /// and of each element of a compound value.
    fn assignment(&mut self, word: &str) {
        let (destination, value) = word.split_once('=').unwrap_or((word, ""));
        self.subscript(destination);
        if let Some(elements) = value.strip_prefix('(') {
            let chars: Vec<char> = elements.chars().collect();
            let mut at = 0;
            while let Some(open) = chars[at..].iter().position(|c| *c == '[') {
                let Some((body, after)) = closing(&chars, at + open + 1, '[', ']', false) else {
                    // An element that does not close is not read.
                    self.expressions.push("$".to_string());
                    break;
                };
                self.expressions.push(body);
                at = after;
            }
        }
    }

    /// Record the subscript of a variable name the shell sets or reads.
    fn subscript(&mut self, name: &str) {
        let Some(open) = name.find('[') else {
            return;
        };
        if !plain_name(&name[..open]) {
            return;
        }
        let chars: Vec<char> = name[open + 1..].chars().collect();
        let body =
            closing(&chars, 0, '[', ']', false).map_or_else(|| "$".to_string(), |(body, _)| body);
        self.expressions.push(body);
    }

    /// Record the operands of a test's numeric comparisons, which the
    /// shell evaluates even when quoted, and the subscript a `-v` names.
    fn comparison(&mut self, rest: &[String]) {
        for (at, word) in rest.iter().enumerate() {
            if ARITHMETIC_TESTS.contains(&word.as_str()) {
                let before = at.checked_sub(1).and_then(|before| rest.get(before));
                self.expressions.extend(before.cloned());
                self.expressions.extend(rest.get(at + 1).cloned());
            }
            if word == "-v" {
                if let Some(name) = rest.get(at + 1) {
                    self.subscript(name);
                }
            }
        }
    }

    /// Whether every expression the line evaluates holds only literal
    /// numbers and operators ([`literal_expression`]).
    fn literal(&self) -> bool {
        self.expressions
            .iter()
            .all(|expression| literal_expression(expression))
    }
}

/// The position after the name that starts at `at`, or `at` when none does.
fn name_end(chars: &[char], mut at: usize) -> usize {
    if chars
        .get(at)
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
    {
        while chars
            .get(at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            at += 1;
        }
    }
    at
}

/// The text from `at` to the `close` that balances it (a doubled one when
/// `doubled`, as `))` closes `((`), and the position after that close;
/// `None` when it does not close.
fn closing(
    chars: &[char],
    mut at: usize,
    open: char,
    close: char,
    doubled: bool,
) -> Option<(String, usize)> {
    let start = at;
    let mut depth = 0usize;
    while let Some(&c) = chars.get(at) {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth > 0 {
                depth -= 1;
            } else if !doubled {
                return Some((chars[start..at].iter().collect(), at + 1));
            } else if chars.get(at + 1) == Some(&close) {
                return Some((chars[start..at].iter().collect(), at + 2));
            } else {
                return None;
            }
        }
        at += 1;
    }
    None
}

/// Whether an arithmetic expression holds only literal numbers and
/// operators. Any variable name, expansion or quote refuses: the shell
/// evaluates a name's value in turn, an inherited one included, and an
/// expansion's text after it fills it in. The numeric special parameters
/// `$?`, `$#`, `$$` and `$!` are numbers.
fn literal_expression(expression: &str) -> bool {
    let chars: Vec<char> = expression.chars().collect();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if c.is_ascii_digit() {
            // A number, with its base or hex digits (`16#ff`, `0x1f`).
            while chars
                .get(at)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | '_' | '@'))
            {
                at += 1;
            }
        } else if c == '$' {
            if !matches!(chars.get(at + 1), Some('?' | '#' | '$' | '!')) {
                return false;
            }
            at += 2;
        } else if " \t\n+-*/%<>=!&|^~?:,;()[]".contains(c) {
            at += 1;
        } else {
            return false;
        }
    }
    true
}

/// `value` as the absolute folder a process on this host reads it as.
/// `None` for a value the guard cannot place on this host.
fn literal_root(value: &str, windows: bool) -> Option<PathBuf> {
    if value.starts_with('~') {
        return None;
    }
    if windows {
        if let Some(native) = crate::portable_path::git_bash_drive_path(value) {
            return Some(native);
        }
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

/// The command of the template `name`, when one exists. A name that could
/// be a template whose file the guard cannot read, or whose `command` it
/// cannot parse, is an error, as is a template folder the line assigns in
/// a form the guard cannot read.
fn template_command(
    name: &str,
    home: Option<&Path>,
    assigned: &Result<Vec<PathBuf>, String>,
) -> Result<Option<String>, String> {
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Ok(None);
    }
    let assigned = assigned.as_ref().map_err(Clone::clone)?;
    let mut commands = Vec::new();
    for dir in template_dirs(home, assigned) {
        let file = dir.join(format!("{name}.md"));
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("the template `{name}` cannot be read: {error}")),
        };
        commands.push(
            frontmatter_command(&text).ok_or_else(|| {
                format!("the template `{name}` has no `command` the guard can read")
            })?,
        );
    }
    if commands.is_empty() {
        return Ok(None);
    }
    // More than one folder may hold the name; each command is judged.
    Ok(Some(commands.join("\n")))
}

/// The `command` value of a template's front matter: a plain, single- or
/// double-quoted scalar on one line. Any other form is unread.
fn frontmatter_command(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut found = None;
    for line in lines {
        if line.trim_end() == "---" {
            return found;
        }
        let Some(value) = line.strip_prefix("command:") else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        let value = value.trim();
        found = Some(if let Some(inner) = value.strip_prefix('\'') {
            inner.strip_suffix('\'')?.replace("''", "'")
        } else if let Some(inner) = value.strip_prefix('"') {
            let inner = inner.strip_suffix('"')?;
            let mut out = String::new();
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next()? {
                        c @ ('"' | '\\') => out.push(c),
                        _ => return None,
                    },
                    '"' => return None,
                    c => out.push(c),
                }
            }
            out
        } else {
            if value.is_empty()
                || value.starts_with(['|', '>', '[', '{', '&', '*', '!', '#'])
                || value.contains(" #")
            {
                return None;
            }
            value.to_string()
        });
    }
    None
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
            assert_eq!(lines(command), vec!["\"git\" \"status\""], "{command}");
        }
        assert_eq!(lines("hc run 'git status'"), vec!["git status"]);
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
        assert_eq!(lines("hc run -- rm -rf *"), vec!["\"rm\" \"-rf\" *"]);
        assert_eq!(lines("hc run -- rm -rf '*'"), vec!["\"rm\" \"-rf\" \"*\""]);
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
            "hc run",
            "hc run --bg",
            "hc run --help",
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
        for line in [
            "hc run --bg --name x --cwd /tmp -- codex -c a=b",
            "hc exec --cwd /tmp ls",
            "hc ls",
            "hc",
        ] {
            assert!(fully_read(&words(line)), "{line}");
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

    #[test]
    fn supplied_words_are_unread() {
        assert!(!read("echo x | xargs hc run -- ls").unread.is_empty());
    }

    #[test]
    fn template_commands_are_read_or_refused() {
        assert_eq!(
            frontmatter_command("---\nname: r\ncommand: claude --model x\n---\nbody"),
            Some("claude --model x".to_string())
        );
        assert_eq!(
            frontmatter_command("---\ncommand: 'a ''b'''\n---\n"),
            Some("a 'b'".to_string())
        );
        assert_eq!(
            frontmatter_command("---\ncommand: \"a \\\"b\\\"\"\n---\n"),
            Some("a \"b\"".to_string())
        );
        for text in [
            "no front matter",
            "---\nname: r\n---\n",
            "---\ncommand: |\n  claude\n---\n",
            "---\ncommand: [claude]\n---\n",
            "---\ncommand: \"a\\nb\"\n---\n",
            "---\ncommand: a\ncommand: b\n---\n",
            "---\ncommand: claude # x\n---\n",
        ] {
            assert_eq!(frontmatter_command(text), None, "{text}");
        }
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".config/herdr-core/templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("peer.md"), "---\ncommand: claude -p\n---\n").unwrap();
        std::fs::write(dir.join("bad.md"), "---\ncommand: |\n---\n").unwrap();
        let none = Ok(Vec::new());
        assert_eq!(
            template_command("peer", Some(temp.path()), &none),
            Ok(Some("claude -p".to_string()))
        );
        assert!(template_command("bad", Some(temp.path()), &none).is_err());
        assert_eq!(
            template_command("absent", Some(temp.path()), &none),
            Ok(None)
        );
        assert_eq!(
            template_command("../peer", Some(temp.path()), &none),
            Ok(None)
        );
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
            assert_eq!(lines(&command), vec!["\"git\" \"status\""], "{command}");
        }
        let found = read("HERDR.exe pane run p1 ls");
        assert_eq!(found.runs[0].line, "ls");
        let words = |line: &str| -> Vec<String> { vec![line.to_string(), "ls".to_string()] };
        for program in ["HC.EXE", native.as_str()] {
            assert!(fully_read(&words(program)), "{program}");
        }
    }

    /// The template folders `hc run` reads move with the variables the line
    /// gives it, including `hc exec --env`; an assignment the guard cannot
    /// read refuses.
    #[test]
    fn assigned_template_folders_are_read() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("herdr-core").join("templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("peer.md"), "---\ncommand: claude -p\n---\n").unwrap();
        // The root as a shell writes an absolute path, starting with `/`,
        // since only such a path value is read.
        let real = crate::portable_path::canonicalize(temp.path()).unwrap();
        let slashed = crate::portable_path::slashed(&real);
        let root = match slashed.split_once(":/") {
            Some((drive, rest)) if drive.len() == 1 => {
                format!("/{}/{rest}", drive.to_ascii_lowercase())
            }
            _ => slashed,
        };
        let template = |command: &str| -> Vec<String> {
            let found = read(command);
            assert!(found.unread.is_empty(), "{command}: {:?}", found.unread);
            found
                .runs
                .iter()
                .filter(|run| run.form == "hc run <template>")
                .map(|run| run.line.clone())
                .collect()
        };
        for name in [
            "APPDATA",
            "AppData",
            "appdata",
            "XDG_CONFIG_HOME",
            "Xdg_Config_Home",
        ] {
            assert_eq!(
                template(&format!("{name}='{root}' hc run peer")),
                vec!["claude -p"],
                "{name}"
            );
            assert_eq!(
                template(&format!("env {name}=\"{root}\" hc run peer")),
                vec!["claude -p"],
                "{name}"
            );
        }
        // Assignments are read from the words the shell passes, after
        // quote removal, wherever they reach the carrier.
        for command in [
            format!("env 'APPDATA'='{root}' hc run peer"),
            format!("env AP''PDATA=\"{root}\" hc run peer"),
            format!("export APPDATA='{root}'; hc run peer"),
            format!("APPDATA='{root}'; hc run peer"),
            format!("sh -c \"APPDATA='{root}' hc run peer\""),
            // Arithmetic over literal numbers, quoted data a command does
            // not evaluate, and a trap that is only listed or reset set
            // nothing.
            format!("(( 1 + 2 )); APPDATA='{root}' hc run peer"),
            format!("printf '%s' 'APPDATA=1'; APPDATA='{root}' hc run peer"),
            format!("trap -p DEBUG; APPDATA='{root}' hc run peer"),
            format!("trap - DEBUG; APPDATA='{root}' hc run peer"),
            // A literal number assigned to another variable cannot reach
            // arithmetic it does not already hold.
            format!("X=1 APPDATA='{root}' hc run peer"),
            format!("X=1; APPDATA='{root}' hc run peer"),
        ] {
            assert_eq!(template(&command), vec!["claude -p"], "{command}");
        }
        // `hc exec --env` gives the variable to the `hc run` it starts,
        // which is judged as that line typed bare.
        let outer = read(&format!("hc exec --env 'AppData={root}' -- hc run peer"));
        assert!(outer.unread.is_empty(), "{:?}", outer.unread);
        assert_eq!(template(&outer.runs[0].line), vec!["claude -p"]);
        let refusing = [
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
        ];
        let passed: Vec<_> = refusing
            .into_iter()
            .filter(|command| !template_refused(command))
            .collect();
        assert!(passed.is_empty(), "not refused: {passed:?}");
    }

    /// Whether `command` refuses its template lookup, read bare or as the
    /// line its first carrier runs.
    fn template_refused(command: &str) -> bool {
        let found = read(command);
        let inner = found
            .runs
            .first()
            .map(|run| read(&run.line))
            .unwrap_or_default();
        found
            .unread
            .iter()
            .chain(&inner.unread)
            .any(|u| u.contains("<template>"))
    }

    /// The shell evaluates arithmetic in many places, and a name read there
    /// is evaluated in turn, so any operand that is not a literal number
    /// refuses a template lookup.
    #[test]
    fn arithmetic_operands_are_read() {
        let refusing = [
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
            // Any variable name in arithmetic refuses, one the line sets to
            // a number included.
            "i=0; (( i++ )); hc run peer",
            "i=0; read i; (( i++ )); hc run peer",
            "false && i=0; (( i++ )); hc run peer",
            "i=0 & (( i++ )); hc run peer",
            "i=0; read i < values; (( i )); hc run peer",
            "i=0; (( i < 3 )); hc run peer",
            "i=0; i[0]=v; (( i++ )); hc run peer",
            "i=0; declare -n r=i; r=v; (( i++ )); hc run peer",
            // Positional and special parameters take offsets too.
            "set -- abc; echo ${1:v}; hc run peer",
            "echo ${@:v}; hc run peer",
            // Code the current shell runs from a quoted word.
            "eval '(( v ))'; hc run peer",
            "trap '(( v ))' DEBUG; hc run peer",
            // Quotes do not hide arithmetic: nested quotes, comments and
            // here-document text are read as code.
            "echo \"$(printf \"%s\" \"don't\")\"; (( v )); hc run peer",
            "echo $'\\''; (( v )); hc run peer",
            "true # don't\n(( v )); hc run peer",
            "cat <<EOF\ndon't\nEOF\n(( v )); hc run peer",
            // Arithmetic forms inside single-quoted data are read as code
            // too, an accepted over-refusal.
            "printf '%s' '(( v ))'; hc run peer",
            "echo '${s:v}'; hc run peer",
            // A value any builtin writes to an integer or float variable is
            // evaluated as arithmetic, with no arithmetic on the line.
            "export APPDATA; integer n; read n < values; hc run peer",
            "declare -i n; mapfile n < values; hc run peer",
            "typeset -ix n; printf -v n %s x; hc run peer",
            "local -i n; getopts ab n; hc run peer",
            "float n; for n in x; do :; done; hc run peer",
            "typeset -E n; read n; hc run peer",
            // A destination may be numeric with no declaration on the line,
            // as a shell's own numeric parameters are, so every builtin
            // write refuses, whatever the destination.
            "read SECONDS < values; hc run peer",
            "read n < values; hc run peer",
            "mapfile n < values; hc run peer",
            "printf -v n %s x; hc run peer",
            "print -v n x; hc run peer",
            "print -rv n x; hc run peer",
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
            // So does an assignment whose value is not a literal number or
            // absolute path, to any name: `MODE=fast hc run peer` refuses
            // too, an accepted over-refusal.
            "SECONDS=v; hc run peer",
            "X=v hc run peer",
            "MODE=fast hc run peer",
            "export SECONDS=v; hc run peer",
            "X=1/v hc run peer",
            "X=/v~ hc run peer",
            "FOO=$BAR hc run -- ls",
            "read MY_APPDATA; hc run -- ls",
            // Assigning default syntax is read in single quotes too, an
            // accepted over-refusal.
            "echo '${APPDATA:=relative}'; hc run -- ls",
        ];
        let passed: Vec<_> = refusing
            .into_iter()
            .filter(|command| !template_refused(command))
            .collect();
        assert!(passed.is_empty(), "not refused: {passed:?}");
        for control in [
            "X=1 hc run peer",
            "X=/opt/x hc run peer",
            "printf '%s' x; hc run peer",
        ] {
            assert!(!template_refused(control), "refused: {control}");
        }
        // The refusal names the forms that clear it.
        for command in ["MODE=fast hc run peer", "read n; hc run peer"] {
            let found = read(command);
            assert!(
                found
                    .unread
                    .iter()
                    .any(|u| u.contains("a literal number or an absolute path")),
                "{command}: {:?}",
                found.unread
            );
        }
        let data = "printf '%s' '(( v ))'; hc run -- ls";
        assert!(!read(data).unread.is_empty(), "not refused: {data}");
    }

    /// A word that only looks like an assignment, as data a carried
    /// command receives, sets no template folder and refuses nothing.
    #[test]
    fn assignment_shaped_data_is_not_an_assignment() {
        let commands = [
            "hc run -- echo APPDATA=relative",
            "hc run echo XDG_CONFIG_HOME=relative",
            "hc exec -- printf %s APPDATA=relative",
            "echo APPDATA=relative && hc run -- ls",
            // A variable's name as data, and an expansion in an assignment
            // a launcher passes to its program, set no template folder.
            "printf '%s' APPDATA; hc run -- ls",
            "env FOO=$BAR hc run -- ls",
            // A reference to another plain name is read.
            "declare -n ref=OTHER; ref=/x; hc run -- ls",
        ];
        let refused: Vec<_> = commands
            .into_iter()
            .filter(|command| !read(command).unread.is_empty())
            .collect();
        assert!(refused.is_empty(), "refused: {refused:?}");
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
