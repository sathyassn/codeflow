//! The text a line's shell and its printing programs produce (TSK-1476).
//!
//! A line outside the closed subset is judged on its raw text, which reads
//! a backslash both as dropped and as a break. The shell and a printing
//! program can still build a name the raw text does not hold.
//!
//! Where the line itself hands text it produces to something that
//! executes it ([`executors`]), that text must be settled: one printer of
//! plain literal text, a literal here-string or a literal here-document,
//! reaching the executor through a pipe, a redirection, a process
//! substitution or a command substitution. The text is judged as the
//! executor reads it, as a command line for a shell or a supplier and as
//! code for an interpreter. Any other feed refuses ([`UNSETTLED`]),
//! whatever it names, and so does a line whose grammar the reader does not
//! take apart (a group, a loop, a background job) when it executes such
//! text ([`executes_produced_text`]). A plain file is the same as the file
//! run directly (`cat file | sh` as `sh file`) on a line that writes no
//! file; a line that writes a file and also runs one refuses
//! ([`writes_a_file`], [`runs_a_file`]), whatever the names, unless it is
//! one simple command, whose writes are its own output ([`lone`]). Output a
//! program hands to a shell it starts itself (a Git pager), a file a
//! program writes in its own way (a build) and a program word from outside
//! the line are state the line does not show.
//!
//! A program the guard does not know to read its input only as data
//! ([`inert`]) gets the same closed form for text the line spells and hands
//! it by pipe, here-string, here-document or process substitution: settled
//! text is judged as a command line, anything else refuses. The output of
//! a program the line runs, fed to such a program, is the named residual.
//! A line the reader does not take apart into simple commands is read
//! token by token for the same feeds ([`unread`]).
//!
//! On every line these texts are also judged for the binary with a verb:
//!
//! 1. each simple command's words as the shell passes them, from the
//!    guard's shell lexer, a quoted word that holds a blank read again as a
//!    command line;
//! 2. what `printf`, `echo` and zsh `print` write, where that is settled
//!    by plain reading ([`printed`]): no backslash the shell consumes on
//!    the command, no `$'...'` word with an escape, no escape in a `printf`
//!    format but `\n`, no conversion but a bare `%s`, no more arguments
//!    than conversions, and no backslash in what `echo` or `print` without
//!    `-r` writes or in a value expanded into them. Output that is not
//!    settled is not judged here;
//! 3. the texts the guard's shell composer resolves for bash and zsh: a
//!    word built from a value the line assigns, a substitution over the
//!    builtins it models, arithmetic, `eval` and `sh -c` bodies.
//!
//! Text a line writes to a process started earlier that executes what it
//! reads (a named pipe, a descriptor, a background shell) is the named
//! residual: these checks on printed text are partial there.

use super::{find, raw_run, unresolved, InstallRun};
use crate::hooks::git_guard::{
    carried_commands, command_argv, command_argv_expanding, command_argv_literal, command_segments,
    expand_commands, expand_commands_read, is_shell, program_name, reads_input_as_data,
    reads_shell_stdin, redirect_writes, shell_blank, shell_c_argument, strip_launchers,
    strip_reserved_words, COMMAND_CARRIERS, READ_ONLY_PROGRAMS,
};
use crate::security::composed::{
    bindings, composed_texts, compound, expanding_heredoc_readers, input_of, simple_commands,
    Composed, Fed, Input, Simple, Source, Spelt,
};
use crate::security::headless::basename;
use crate::security::launchers::{LAUNCHERS, PACKAGE_RUNNERS, SUPPLIERS};
use crate::security::outward::{
    code_sources, interpreter_code_positions, interpreter_family, reads_stdin_code_words,
};

/// Programs whose output this reader judges.
const PRINTERS: &[&str] = &["printf", "echo", "print"];

/// How deep a printed text, a quoted word or a fed command line is read
/// again as a command line.
const DEPTH: usize = 3;

/// The form reported when a produced text names the binary with a verb.
const FORM: &str =
    "text the shell or a printing program produces, naming the binary with an install verb";

/// The form reported when text the line hands to an executor on the line
/// is not settled.
pub(crate) const UNSETTLED: &str =
    "text the line hands to a shell or interpreter that the guard cannot settle";

/// The first install run, or unsettled text handed to an executor, that
/// `command` produces.
pub(super) fn produced_run(command: &str) -> Option<InstallRun> {
    let form = produced(command, 0, false)?;
    Some(InstallRun {
        form,
        parsed: false,
        in_argument: false,
    })
}

/// The judgment of `command`, where `writes` is whether a line it runs
/// inside writes a file.
fn produced(command: &str, depth: usize, writes: bool) -> Option<&'static str> {
    let writes = writes || (writes_a_file(command) && !lone(command));
    if writes && runs_a_file(command) {
        return Some(UNSETTLED);
    }
    if let Some(form) = executors(command, depth, &Stdin::Unseen, writes) {
        return Some(form);
    }
    let texts = composed_texts(command);
    let escape = escapes(command, &texts);
    if let Some(form) = read(command, 0, escape) {
        return Some(form);
    }
    texts.iter().find_map(|text| match text {
        Composed::Resolved(value) => [value.render(), value.without_open().render()]
            .iter()
            .find_map(|rendered| judge(rendered, 0, escape)),
        Composed::Unresolved(_) => None,
    })
}

/// Whether a value expanded into `echo` or `print` may hold a backslash:
/// the composer resolves a value that holds one, or leaves a value
/// unresolved on a line that holds one.
fn escapes(command: &str, texts: &[Composed]) -> bool {
    texts.iter().any(|text| match text {
        Composed::Resolved(value) => {
            value.render().contains('\\') || value.without_open().render().contains('\\')
        }
        Composed::Unresolved(_) => command.contains('\\'),
    })
}

/// Whether `command`, a line [`executors`] does not take apart, hands text
/// it produces to something on the line that executes it:
///
/// - a shell or interpreter that takes its code from a pipe, a
///   here-string, a process substitution or a descriptor, or from a `-c`
///   or `-e` value holding a substitution (`outward::code_sources`);
/// - a command word the guard cannot fill in, a substitution or a value;
/// - a shell or interpreter given a here-document the shell expands;
/// - in any simple command at any depth: `eval` with a word the shell
///   expands, a shell's `-c` value or an interpreter's code value the
///   shell expands, or a program that runs what it reads (a shell or
///   interpreter reading its input, a supplier running a command built
///   from it, or a launcher in front of one) with no input redirection
///   from a file or literal text.
fn executes_produced_text(command: &str) -> bool {
    let read = expand_commands_read(command);
    if !read.unread_commands.is_empty() || code_sources(command).0 {
        return true;
    }
    let heredoc = match expanding_heredoc_readers(command) {
        Some(readers) => readers
            .iter()
            .any(|reader| runs_its_input(&command_argv(reader))),
        None => command.contains("<<"),
    };
    let top = command_segments(command);
    heredoc
        || read.segments.iter().any(|segment| {
            // A command a launcher carries (`find -exec`, `xargs`) comes
            // back with every word in double quotes; its `$` was text
            // where the line was written, and the carrier's own words are
            // judged with their marks as written.
            let marks = !requoted(segment) || top.contains(segment);
            executes_in(segment, marks)
        })
}

/// Whether `segment` is written exactly as the guard writes a carried
/// command back: every word in double quotes.
fn requoted(segment: &str) -> bool {
    let words = command_argv(segment);
    let quoted: Vec<String> = words
        .iter()
        .map(|word| format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    !words.is_empty() && quoted.join(" ") == segment
}

/// Whether one simple command executes text the line produces. With
/// `marks`, a word in a program position that runs code from a value the shell
/// fills in counts: `eval` with such a word after it, a shell's `-c`
/// value or an interpreter's code value; a program that runs what it
/// reads counts with no input redirection from a file or literal text.
fn executes_in(segment: &str, marks: bool) -> bool {
    let (words, filled) = command_argv_expanding(segment);
    let Some((program, args)) = strip_launchers(&words) else {
        return false;
    };
    let first = words.len() - args.len() - 1;
    // After a launcher or a supplier, any later word may be the program
    // that runs.
    let carries = {
        let name = program_name(program);
        LAUNCHERS.contains(&name) || SUPPLIERS.contains(&name) || COMMAND_CARRIERS.contains(&name)
    };
    let last = if carries { words.len() } else { first + 1 };
    if marks {
        for (at, word) in words.iter().enumerate().take(last).skip(first) {
            let rest = &words[at + 1..];
            let filled_at = |offset: usize| filled.get(at + 1 + offset).copied().unwrap_or(true);
            let name = basename(word);
            let runs_filled = if name == "eval" {
                (0..rest.len()).any(filled_at)
            } else if is_shell(&name) {
                shell_c_argument(rest).is_some_and(|value| {
                    rest.iter()
                        .position(|arg| std::ptr::eq(arg, value))
                        .is_none_or(filled_at)
                })
            } else {
                interpreter_family(&name).is_some_and(|family| {
                    interpreter_code_positions(family, rest)
                        .into_iter()
                        .any(filled_at)
                })
            };
            if runs_filled {
                return true;
            }
        }
    }
    matches!(input_of(segment), Input::Inherited | Input::Produced) && runs_its_input(&words)
}

/// Whether the command `words` runs what it reads: a shell or interpreter
/// that reads its code from its input, a supplier that runs a command
/// built from it, or a launcher (`sudo` and the like) in front of either.
fn runs_its_input(words: &[String]) -> bool {
    let runs = |words: &[String]| reads_stdin_code_words(words) || supplies_a_run(words);
    if runs(words) {
        return true;
    }
    let Some((program, args)) = strip_launchers(words) else {
        return false;
    };
    LAUNCHERS.contains(&program_name(program)) && (0..args.len()).any(|at| runs(&args[at..]))
}

/// Whether the command `words` is a supplier that runs a command built
/// from what it reads: `parallel`, or `xargs` ([`xargs_runs_text`]).
fn supplies_a_run(words: &[String]) -> bool {
    let Some((program, args)) = strip_launchers(words) else {
        return false;
    };
    match program_name(program) {
        "parallel" => true,
        "xargs" => xargs_runs_text(args),
        _ => false,
    }
}

/// Whether `xargs`, given `args`, may execute the text it reads: an option
/// this reader does not know, a command whose program the text fills in or
/// the shell expands, or a command that runs code or another program (a
/// shell, `eval`, `source`, an interpreter, a launcher, a supplier or a
/// package runner). With no command it only prints.
fn xargs_runs_text(args: &[String]) -> bool {
    const FLAGS: &str = "0rtpx";
    const VALUED: &str = "ILnPsdEa";
    const OPTIONAL: &str = "iel";
    const LONG_FLAGS: &[&str] = &[
        "--null",
        "--no-run-if-empty",
        "--verbose",
        "--interactive",
        "--exit",
        "--open-tty",
        "--show-limits",
        "--replace",
        "--eof",
        "--max-lines",
    ];
    const LONG_VALUED: &[&str] = &[
        "--arg-file",
        "--delimiter",
        "--max-args",
        "--max-procs",
        "--max-chars",
        "--process-slot-var",
    ];
    let mut placeholder = String::from("{}");
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            at += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        at += 1;
        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long.split_once('=').unwrap_or((long, ""));
            let name = format!("--{name}");
            if name == "--replace" && !value.is_empty() {
                value.clone_into(&mut placeholder);
            }
            if LONG_VALUED.contains(&name.as_str()) {
                at += usize::from(!long.contains('='));
            } else if !LONG_FLAGS.contains(&name.as_str()) {
                return true;
            }
            continue;
        }
        for (offset, flag) in arg.char_indices().skip(1) {
            let rest = &arg[offset + flag.len_utf8()..];
            if FLAGS.contains(flag) {
                continue;
            }
            if VALUED.contains(flag) {
                let value = if rest.is_empty() {
                    at += 1;
                    args.get(at - 1).map_or("", String::as_str)
                } else {
                    rest
                };
                if flag == 'I' {
                    value.clone_into(&mut placeholder);
                }
            } else if OPTIONAL.contains(flag) {
                if flag == 'i' && !rest.is_empty() {
                    rest.clone_into(&mut placeholder);
                }
            } else {
                return true;
            }
            break;
        }
    }
    let command = args.get(at..).unwrap_or_default();
    let Some(first) = command.first() else {
        return false;
    };
    let Some((program, _)) = strip_launchers(command) else {
        return true;
    };
    let name = program_name(program);
    first.contains(placeholder.as_str())
        || program.contains(placeholder.as_str())
        || unresolved(first)
        || unresolved(program)
        || is_shell(name)
        || matches!(name, "eval" | "source" | ".")
        || interpreter_family(name).is_some()
        || LAUNCHERS.contains(&name)
        || SUPPLIERS.contains(&name)
        || PACKAGE_RUNNERS.contains(&name)
}

/// What a command reads, as the install reader reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stdin {
    /// Nothing the line composes: the line's own input, or a plain file on
    /// a line that writes no file.
    Unseen,
    /// Text the line spells that the reader settles.
    Text(String),
    /// Text the line spells that the reader cannot settle, or a file on a
    /// line that writes one.
    Unsettled,
    /// The output of a program the line runs that the reader does not
    /// model, holding no text the line spells, or a descriptor: unsettled
    /// for a shell or interpreter, the named residual for a program the
    /// guard does not know.
    Output,
}

/// The judgment of every text `command` hands to something on it that
/// executes it, where `stdin` is what its commands read when nothing on
/// the line redirects or pipes their input and `writes` is whether the
/// line writes a file. A line the reader takes apart into simple commands
/// ([`simple_commands`]) is judged command by command ([`judge_simple`]);
/// any other line, or one nested past [`DEPTH`], is read token by token
/// ([`unread`]).
fn executors(command: &str, depth: usize, stdin: &Stdin, writes: bool) -> Option<&'static str> {
    let commands = if depth > DEPTH {
        None
    } else if requoted(command) {
        // A command a launcher carries (`find -exec`, `xargs`) comes back
        // with every word in double quotes; its `$` was text where the
        // line was written.
        Some(vec![Simple {
            words: command_argv(command)
                .into_iter()
                .map(Spelt::Plain)
                .collect(),
            raw: command.to_string(),
            piped: false,
            redirects: Vec::new(),
        }])
    } else {
        simple_commands(command)
    };
    let Some(commands) = commands else {
        return unread(command, depth, writes);
    };
    for at in 0..commands.len() {
        let input = if commands[at].piped && at > 0 {
            output_of(&commands, at - 1, stdin, writes)
        } else {
            stdin.clone()
        };
        if let Some(form) = judge_simple(command, &commands, at, &input, depth, writes) {
            return Some(form);
        }
    }
    None
}

/// The judgment of a line [`simple_commands`] does not take apart (a
/// group, a subshell, a loop, a conditional, a background job), or one
/// nested past [`DEPTH`]: [`UNSETTLED`] when it executes text it produces
/// at all ([`executes_produced_text`]); else each feed the line holds,
/// read token by token ([`compound`]), judged as for a simple command:
/// one simple command's output or literal text settled, text the line
/// spells in a compound command unsettled, and other programs' output the
/// named residual for a reader the guard does not know. Every command the
/// line carries (`sh -c`, `eval`, `xargs`) is judged again. Grammar the
/// token reading does not follow refuses when the line holds a pipe, a
/// here-string, a here-document or a process substitution.
fn unread(command: &str, depth: usize, writes: bool) -> Option<&'static str> {
    if executes_produced_text(command) {
        return Some(UNSETTLED);
    }
    let Some(found) = compound(command) else {
        let feeds = command.contains('|') || command.contains("<<") || command.contains("<(");
        return feeds.then_some(UNSETTLED);
    };
    let nested = depth <= DEPTH;
    for feed in &found.feeds {
        let readers: Vec<Vec<String>> = feed
            .readers
            .iter()
            .map(|raw| {
                let mut words = command_argv(raw);
                strip_reserved_words(&mut words);
                words
            })
            .collect();
        if readers.iter().all(|words| inert(words)) {
            continue;
        }
        let fed = match &feed.fed {
            Fed::Command(simple) => {
                output_of(std::slice::from_ref(simple), 0, &Stdin::Output, writes)
            }
            Fed::Text(text) => Stdin::Text(text.clone()),
            Fed::Body(body) if nested => {
                if let Some(form) = executors(body, depth + 1, &Stdin::Output, writes) {
                    return Some(form);
                }
                body_output(body, &Stdin::Output, writes)
            }
            Fed::Body(_) | Fed::Spelled => Stdin::Unsettled,
            Fed::Output => Stdin::Output,
        };
        let interpreter = readers.iter().any(|words| {
            strip_launchers(words)
                .is_some_and(|(program, _)| interpreter_family(&basename(program)).is_some())
        });
        let form = if readers.iter().any(|words| runs_its_input(words)) {
            judge_fed(&fed, interpreter, None, depth, writes)
        } else {
            judge_unknown(&fed, depth, writes)
        };
        if form.is_some() {
            return form;
        }
    }
    if !nested {
        return None;
    }
    found.commands.iter().find_map(|simple| {
        let mut words = command_argv(&simple.raw);
        strip_reserved_words(&mut words);
        carried_commands(&words)
            .iter()
            .find_map(|body| executors(body, depth + 1, &Stdin::Output, writes))
    })
}

/// One simple command's words as the shell passes them, where the reader
/// can tell.
struct Resolved {
    /// Each word: its text, a settled substitution's output, a stand-in
    /// path for a process substitution, or the word as written.
    words: Vec<String>,
    /// Whether each word is known.
    settled: Vec<bool>,
    /// A settled substitution's output, by word.
    texts: Vec<Option<String>>,
    /// Each process substitution's position and output.
    processes: Vec<(usize, Stdin)>,
}

/// The words of `simple`, where `input` is what it reads from a pipe or
/// the enclosing line, which its substitutions read too; `Err` with the
/// form when a substitution in them executes unsettled text itself.
fn resolve(
    simple: &Simple,
    input: &Stdin,
    depth: usize,
    writes: bool,
) -> Result<Resolved, &'static str> {
    let mut resolved = Resolved {
        words: Vec::new(),
        settled: Vec::new(),
        texts: Vec::new(),
        processes: Vec::new(),
    };
    for piece in &simple.words {
        let (word, settled, text) = match piece {
            Spelt::Plain(text) => (text.clone(), true, None),
            Spelt::Open(raw) => (raw.clone(), false, None),
            Spelt::Output(body) | Spelt::Process(body) => {
                if let Some(form) = executors(body, depth + 1, input, writes) {
                    return Err(form);
                }
                let output = body_output(body, input, writes);
                if matches!(piece, Spelt::Process(_)) {
                    resolved.processes.push((resolved.words.len(), output));
                    ("/dev/fd/63".to_string(), true, None)
                } else if let Stdin::Text(mut value) = output {
                    // The shell drops the trailing newlines.
                    while value.ends_with('\n') {
                        value.pop();
                    }
                    (value.clone(), true, Some(value))
                } else {
                    (format!("$({body})"), false, None)
                }
            }
        };
        resolved.words.push(word);
        resolved.settled.push(settled);
        resolved.texts.push(text);
    }
    Ok(resolved)
}

/// The judgment of the texts one simple command executes: a substitution
/// in a word that runs (its program, `eval`'s words, a shell's `-c` value,
/// an interpreter's code value), what it reads when it runs its input, a
/// descriptor or process substitution a shell or interpreter reads, text
/// the line spells that a program it does not know to read only data
/// receives ([`judge_unknown`]), and every command it carries, read again
/// with its input. `input` is what a pipe or the enclosing line hands it.
#[allow(clippy::too_many_lines)] // one command's routes, in order
fn judge_simple(
    line: &str,
    commands: &[Simple],
    at: usize,
    input: &Stdin,
    depth: usize,
    writes: bool,
) -> Option<&'static str> {
    let simple = &commands[at];
    let Resolved {
        words,
        settled,
        texts,
        processes,
    } = match resolve(simple, input, depth, writes) {
        Ok(resolved) => resolved,
        Err(form) => return Some(form),
    };
    let (program, _) = strip_launchers(&words)?;
    let program = basename(program);
    for (at, code) in running_positions(&words) {
        if !settled.get(at).copied().unwrap_or(false) {
            // A program word from outside the line (`$HOME/bin/tool`) is
            // state the line does not show; it runs only what it is fed,
            // and may be a shell.
            if code == Code::Program && !produced_program(&words[at], line) {
                return judge_fed(input, false, None, depth, writes);
            }
            return Some(UNSETTLED);
        }
        if let Some(text) = texts.get(at).cloned().flatten() {
            let form = match code {
                Code::Program => None,
                Code::Line => judge_line(&text, depth, writes),
                Code::Interpreter => raw_run(&text).map(|run| run.form),
            };
            if form.is_some() {
                return form;
            }
        }
    }
    // The first word in a program position that reads code from a file it
    // is given.
    let reads_code = program_span(&words).and_then(|(first, last)| {
        (first..last).find(|&at| {
            let name = basename(&words[at]);
            is_shell(&name)
                || interpreter_family(&name).is_some()
                || matches!(name.as_str(), "source" | ".")
        })
    });
    let interpreter = interpreter_family(&program).is_some();
    let mut fed = input.clone();
    for redirect in simple.redirects.iter().filter(|redirect| redirect.input) {
        let text = match &redirect.source {
            Source::Text(text) => Stdin::Text(text.clone()),
            Source::Path(path) if path == "/dev/null" => Stdin::Unseen,
            Source::Path(_) => file(writes),
            Source::Process(body) => {
                if let Some(form) = executors(body, depth + 1, input, writes) {
                    return Some(form);
                }
                body_output(body, input, writes)
            }
            Source::Spelled => Stdin::Unsettled,
            Source::Open => Stdin::Output,
        };
        if redirect.fd == 0 {
            fed = text;
        } else if reads_code.is_some() {
            if let Some(form) = judge_fed(&text, interpreter, None, depth, writes) {
                return Some(form);
            }
        }
    }
    for (at, output) in &processes {
        if reads_code.is_some_and(|first| first < *at) {
            if let Some(form) = judge_fed(output, interpreter, None, depth, writes) {
                return Some(form);
            }
        }
    }
    if runs_its_input(&words) {
        let supplier = matches!(program_name(&program), "xargs" | "parallel");
        let consumer = supplier.then_some(simple.raw.as_str());
        if let Some(form) = judge_fed(&fed, interpreter, consumer, depth, writes) {
            return Some(form);
        }
    } else if !inert(&words) {
        let form = std::iter::once(&fed)
            .chain(processes.iter().map(|(_, output)| output))
            .find_map(|text| judge_unknown(text, depth, writes));
        if form.is_some() {
            return form;
        }
    }
    carried_commands(&words)
        .iter()
        .find_map(|body| executors(body, depth + 1, &fed, writes))
}

/// What a plain file hands a command that reads it: nothing the line
/// composes on a line that writes no file, else unsettled text.
fn file(writes: bool) -> Stdin {
    if writes {
        Stdin::Unsettled
    } else {
        Stdin::Unseen
    }
}

/// Whether the command `words`, reading text the line spells, cannot
/// execute it: only assignments, syntax words of a compound command, a
/// printer, a program on the guard's read-only list, a data reader
/// (`git_guard::reads_input_as_data`: the here-document data readers and
/// `git` and `gh` subcommands that take a message or a body), or `xargs`
/// building a command that is judged as a carried command.
fn inert(words: &[String]) -> bool {
    const SYNTAX: &[&str] = &["for", "in", "done", "fi", "{", "}"];
    let Some((program, _)) = strip_launchers(words) else {
        return true;
    };
    let name = program_name(program);
    SYNTAX.contains(&name)
        || PRINTERS.contains(&name)
        || READ_ONLY_PROGRAMS.contains(&name)
        || (name == "xargs" && !supplies_a_run(words))
        || reads_input_as_data(words)
}

/// The judgment of text a program the guard does not know to read only
/// data receives: as a command line when it is settled, [`UNSETTLED`] when
/// it is text the line spells that the reader cannot settle, and nothing
/// for a program's output or the line's own input (the named residual).
fn judge_unknown(fed: &Stdin, depth: usize, writes: bool) -> Option<&'static str> {
    match fed {
        Stdin::Unseen | Stdin::Output => None,
        Stdin::Unsettled => Some(UNSETTLED),
        Stdin::Text(text) => judge_line(text, depth, writes),
    }
}

/// Whether the program word `word`, which the shell fills in, may hold
/// text the line produces: a substitution or a `$'...'` word, or a name
/// the line binds to a value the composer does not resolve exactly. A name
/// the line resolves exactly is read with its value elsewhere; a name it
/// does not bind is state the line does not show.
fn produced_program(word: &str, line: &str) -> bool {
    if word.contains("$(") || word.contains("$'") || word.contains('`') || word.contains("<(") {
        return true;
    }
    let bound = bindings(line);
    expanded_names(word).iter().any(|name| {
        bound.iter().any(|(bound, value)| {
            bound == name && !matches!(value, Composed::Resolved(value) if value.exact().is_some())
        })
    })
}

/// The names `word` expands with `$NAME` or `${NAME...}`.
fn expanded_names(word: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = word;
    while let Some(at) = rest.find('$') {
        rest = &rest[at + 1..];
        let body = rest.strip_prefix('{').unwrap_or(rest);
        let name: String = body
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// How a word in a running position runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    /// The program the command runs.
    Program,
    /// A command line: `eval`'s words or a shell's `-c` value.
    Line,
    /// An interpreter's code value.
    Interpreter,
}

/// The positions in `words` whose text runs: the program, and for it or,
/// after a launcher or a supplier, any later word, `eval`'s words, a
/// shell's `-c` value and an interpreter's code value.
fn running_positions(words: &[String]) -> Vec<(usize, Code)> {
    let Some((first, last)) = program_span(words) else {
        return Vec::new();
    };
    let mut positions = vec![(first, Code::Program)];
    for at in first..last {
        let rest = &words[at + 1..];
        let name = basename(&words[at]);
        if name == "eval" {
            positions.extend((at + 1..words.len()).map(|at| (at, Code::Line)));
        } else if is_shell(&name) {
            if let Some(value) = shell_c_argument(rest) {
                match rest.iter().position(|arg| std::ptr::eq(arg, value)) {
                    Some(offset) => positions.push((at + 1 + offset, Code::Line)),
                    None => positions.extend((at + 1..words.len()).map(|at| (at, Code::Line))),
                }
            }
        } else if let Some(family) = interpreter_family(&name) {
            positions.extend(
                interpreter_code_positions(family, rest)
                    .into_iter()
                    .map(|offset| (at + 1 + offset, Code::Interpreter)),
            );
        }
    }
    positions
}

/// The span of `words` that may name the program that runs: the program
/// after its launchers, and after a launcher or a supplier every later
/// word.
fn program_span(words: &[String]) -> Option<(usize, usize)> {
    let (program, args) = strip_launchers(words)?;
    let first = words.len() - args.len() - 1;
    let name = program_name(program);
    let carries =
        LAUNCHERS.contains(&name) || SUPPLIERS.contains(&name) || COMMAND_CARRIERS.contains(&name);
    Some((first, if carries { words.len() } else { first + 1 }))
}

/// The judgment of text handed to an executor: [`UNSETTLED`] when it is
/// not settled or is a program's output, as code for an interpreter, and
/// as a command line for a shell or, after the supplier's own words
/// (`consumer`), for a supplier.
fn judge_fed(
    fed: &Stdin,
    interpreter: bool,
    consumer: Option<&str>,
    depth: usize,
    writes: bool,
) -> Option<&'static str> {
    match fed {
        Stdin::Unseen => None,
        Stdin::Unsettled | Stdin::Output => Some(UNSETTLED),
        Stdin::Text(text) if interpreter => raw_run(text).map(|run| run.form),
        Stdin::Text(text) => match consumer {
            Some(consumer) => judge_line(&format!("{consumer} {text}"), depth, writes),
            None => judge_line(text, depth, writes),
        },
    }
}

/// The judgment of a command line an executor runs: the closed subset,
/// the raw text, then everything this reader judges, read again, on a line
/// that writes a file when `writes`.
fn judge_line(line: &str, depth: usize, writes: bool) -> Option<&'static str> {
    find(line, 0)
        .or_else(|| raw_run(line))
        .map(|run| run.form)
        .or_else(|| produced(line, depth + 1, writes))
}

/// What a substitution's `body` writes, where `input` is what it reads:
/// the last command of one pipeline ([`output_of`]), else
/// [`Stdin::Unsettled`].
fn body_output(body: &str, input: &Stdin, writes: bool) -> Stdin {
    match simple_commands(body) {
        Some(commands) if !commands.is_empty() && commands[1..].iter().all(|c| c.piped) => {
            output_of(&commands, commands.len() - 1, input, writes)
        }
        _ => Stdin::Unsettled,
    }
}

/// What `commands[at]` writes to a pipe, where `stdin` is what it reads
/// with no redirection. Settled text comes from a printer of plain literal
/// text whose output is settled ([`printed`]), or `cat` given a literal
/// here-string or here-document; `cat` of plain files writes nothing the
/// line composes on a line that writes no file ([`file`]). A printer that
/// is not settled, `eval` or a shell given a `-c` value writes text the
/// line spells, unsettled. Any other program writes its own output
/// ([`Stdin::Output`]).
fn output_of(commands: &[Simple], at: usize, stdin: &Stdin, writes: bool) -> Stdin {
    let simple = &commands[at];
    let words: Option<Vec<String>> = simple
        .words
        .iter()
        .map(|piece| match piece {
            Spelt::Plain(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    let mut input = None;
    for redirect in &simple.redirects {
        match (redirect.input, redirect.fd) {
            (true, 0) => input = Some(&redirect.source),
            (false, 1) => return Stdin::Text(String::new()),
            _ => {}
        }
    }
    let Some(words) = words else {
        // A word the shell fills in: a printer, a shell or `cat` named on
        // the command may write text the line spells.
        let names: Vec<String> = simple
            .words
            .iter()
            .filter_map(|piece| match piece {
                Spelt::Plain(text) => Some(basename(text)),
                _ => None,
            })
            .collect();
        let spells = names.iter().any(|name| {
            PRINTERS.contains(&name.as_str()) || is_shell(name) || name == "eval" || name == "cat"
        });
        return if spells {
            Stdin::Unsettled
        } else {
            Stdin::Output
        };
    };
    let Some((program, args)) = strip_launchers(&words) else {
        return Stdin::Text(String::new());
    };
    let program = basename(program);
    if PRINTERS.contains(&program.as_str()) {
        if shell_escaped(&simple.raw, &words) || escaped_dollar_quote(&simple.raw) {
            return Stdin::Unsettled;
        }
        return printed(&program, args, false).map_or(Stdin::Unsettled, Stdin::Text);
    }
    if program == "eval" || (is_shell(&program) && shell_c_argument(args).is_some()) {
        return Stdin::Unsettled;
    }
    if program != "cat" {
        return Stdin::Output;
    }
    let options = args.iter().any(|word| word.starts_with('-') && word != "-");
    let read = if args.iter().any(|word| !word.starts_with('-')) {
        file(writes)
    } else {
        match input {
            Some(Source::Text(text)) => Stdin::Text(text.clone()),
            Some(Source::Path(_)) => file(writes),
            Some(Source::Process(body)) => body_output(body, stdin, writes),
            Some(Source::Spelled) => Stdin::Unsettled,
            Some(Source::Open) => Stdin::Output,
            None if simple.piped && at > 0 => output_of(commands, at - 1, stdin, writes),
            None => stdin.clone(),
        }
    };
    // An option (`-v`, `-n`) changes the text it passes on.
    match read {
        Stdin::Text(_) if options => Stdin::Unsettled,
        read => read,
    }
}

/// Targets a redirection or a writing program can name that are not
/// files.
const NOT_FILES: &[&str] = &[
    "/dev/null",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/fd/1",
    "/dev/fd/2",
];

/// Whether `command` writes a file with text it spells, in any command,
/// group, loop or substitution on it (TSK-1476): an output redirection to
/// a target other than [`NOT_FILES`] (descriptor copies are not writes),
/// `tee` with a file operand, `dd` with `of=`, or `sed` that may write
/// (`-i`, or a script that may hold a `w` command). The other writing
/// programs (`cp`, `mv`, `ln`, `install`, `rsync`) copy content from
/// files: one the line wrote is caught by the rule that wrote it, and
/// another one is state the line does not show.
fn writes_a_file(command: &str) -> bool {
    let read = expand_commands_read(command);
    std::iter::once(command.to_string())
        .chain(read.segments)
        .any(|text| {
            let redirects = redirect_writes(&text);
            !redirects.unread.is_empty()
                || redirects
                    .targets
                    .iter()
                    .any(|target| !NOT_FILES.contains(&target.as_str()))
                || writes_content(&text)
        })
}

/// Whether `command` is one simple command that holds no substitution and
/// carries no command. What such a command writes is its own output: the
/// program starts and its input opens before any of it is written, so a
/// file it runs or reads holds what was there before the line, plus at
/// most that output.
fn lone(command: &str) -> bool {
    let Some(commands) = simple_commands(command) else {
        return false;
    };
    let [simple] = commands.as_slice() else {
        return false;
    };
    let mut words = command_argv(&simple.raw);
    strip_reserved_words(&mut words);
    simple
        .words
        .iter()
        .all(|word| matches!(word, Spelt::Plain(_) | Spelt::Open(_)))
        && !simple
            .redirects
            .iter()
            .any(|redirect| matches!(redirect.source, Source::Process(_)))
        && carried_commands(&words).is_empty()
        && expand_commands_read(command).segments.len() <= 1
}

/// Whether the simple command `segment` is a program that writes a file
/// with content from the line ([`writes_a_file`]).
fn writes_content(segment: &str) -> bool {
    let mut words = command_argv(segment);
    strip_reserved_words(&mut words);
    let Some((program, args)) = strip_launchers(&words) else {
        return false;
    };
    let file = |target: &str| !NOT_FILES.contains(&target);
    match program_name(program) {
        "tee" => args.iter().any(|arg| !arg.starts_with('-') && file(arg)),
        "dd" => args
            .iter()
            .any(|arg| arg.strip_prefix("of=").is_some_and(file)),
        "sed" => args.iter().any(|arg| {
            arg.starts_with("-i")
                || arg.starts_with("--in-place")
                || arg == "-I"
                || (!arg.starts_with('-') && arg.contains(['w', 'W']))
        }),
        _ => false,
    }
}

/// Whether `command` runs a file, in any command it holds or carries
/// (TSK-1476): a program word that is a path (`./run.sh`, `dir/tool`), a
/// script operand given to a shell, `source`, `.` or an interpreter, or a
/// file given to a shell or interpreter as its input. A process
/// substitution or `/dev/stdin` is not a file here.
fn runs_a_file(command: &str) -> bool {
    let not_file = |arg: &str| {
        arg.starts_with('-')
            || arg.starts_with('+')
            || arg.starts_with("<(")
            || arg.starts_with("/dev/fd/")
            || arg == "/dev/stdin"
    };
    expand_commands_read(command)
        .segments
        .iter()
        .any(|segment| {
            let mut words = command_argv(segment);
            strip_reserved_words(&mut words);
            let Some((first, last)) = program_span(&words) else {
                return false;
            };
            let input_file = input_of(segment) == Input::File;
            (first..last).any(|at| {
                let word = &words[at];
                let rest = &words[at + 1..];
                let name = basename(word);
                let operand = rest.iter().find(|arg| !arg.starts_with(['-', '+']));
                let shell = is_shell(&name)
                    && shell_c_argument(rest).is_none()
                    && !reads_shell_stdin(&words[at..])
                    && operand.is_some_and(|arg| !not_file(arg));
                let sourced = matches!(name.as_str(), "source" | ".")
                    && rest.first().is_some_and(|arg| !not_file(arg));
                let interpreted = interpreter_family(&name).is_some_and(|family| {
                    interpreter_code_positions(family, rest).is_empty()
                        && rest.iter().any(|arg| !not_file(arg))
                });
                let fed_file =
                    input_file && (is_shell(&name) || interpreter_family(&name).is_some());
                (word.contains('/') && !not_file(word))
                    || shell
                    || sourced
                    || interpreted
                    || fed_file
            })
        })
}

/// The one judgment of a produced text: [`FORM`] when it names the binary
/// with a verb as written, else what reading it again as a command line
/// finds ([`read`]).
fn judge(text: &str, depth: usize, escape: bool) -> Option<&'static str> {
    if names(text) {
        return Some(FORM);
    }
    read(text, depth + 1, escape)
}

/// Whether `text`, read as the raw text is, names the binary with a verb.
fn names(text: &str) -> bool {
    raw_run(text).is_some()
}

/// [`FORM`] when a text `command` produces names the binary with a verb:
/// its words as the shell passes them, a quoted word with a blank read
/// again, and what each printing program on it writes where that is
/// settled. `escape` is whether a value the line expands into `echo` or
/// `print` may hold a backslash ([`escapes`]).
fn read(command: &str, depth: usize, escape: bool) -> Option<&'static str> {
    if depth > DEPTH {
        return None;
    }
    for segment in expand_commands(command) {
        let (words, _) = command_argv_literal(&segment);
        if names(&words.join(" ")) {
            return Some(FORM);
        }
        for word in words.iter().skip(1) {
            if word.contains(shell_blank) {
                if let Some(form) = read(word, depth + 1, escape) {
                    return Some(form);
                }
            }
        }
        if escaped_dollar_quote(&segment) || shell_escaped(&segment, &words) {
            continue;
        }
        for (at, word) in words.iter().enumerate() {
            let program = basename(word);
            if !PRINTERS.contains(&program.as_str()) {
                continue;
            }
            if let Some(text) = printed(&program, &words[at + 1..], escape) {
                if let Some(form) = judge(&text, depth, escape) {
                    return Some(form);
                }
            }
        }
    }
    None
}

/// Whether `segment` holds a `$'...'` word with an escape in it.
fn escaped_dollar_quote(segment: &str) -> bool {
    let mut rest = segment;
    while let Some(at) = rest.find("$'") {
        let body = &rest[at + 2..];
        match body.find(['\\', '\'']) {
            Some(end) if body[end..].starts_with('\\') => return true,
            Some(end) => rest = &body[end + 1..],
            None => return false,
        }
    }
    false
}

/// Whether the shell consumed a backslash in `segment`: the words the lexer
/// passes on hold fewer backslashes than the text as written, so an
/// unquoted escape, a `\\` or `\$` in double quotes, or a `$'...'` escape
/// was read there.
fn shell_escaped(segment: &str, words: &[String]) -> bool {
    let kept: usize = words.iter().map(|word| word.matches('\\').count()).sum();
    segment.matches('\\').count() > kept
}

/// What `program` writes from `args` when that is settled by plain
/// reading, else `None`. `values_unsettled` is whether a value the line
/// expands into `echo` or `print` may hold an escape the composer did not
/// resolve: the line holds a backslash and the composer leaves a value
/// unresolved (a value from outside the line is the named residual).
fn printed(program: &str, args: &[String], values_unsettled: bool) -> Option<String> {
    if program == "printf" {
        let mut args = args;
        if args.first().is_some_and(|word| word == "-v") {
            args = args.get(2..)?;
        }
        if args.first().is_some_and(|word| word == "--") {
            args = &args[1..];
        }
        let (format, rest) = args.split_first()?;
        if format.starts_with('-') || format.contains(['$', '`']) {
            return None;
        }
        let mut out = String::new();
        let mut used = 0;
        let mut chars = format.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if chars.next() != Some('n') {
                        return None;
                    }
                    out.push('\n');
                }
                '%' => {
                    if chars.next() != Some('s') {
                        return None;
                    }
                    out.push_str(rest.get(used).map_or("", String::as_str));
                    used += 1;
                }
                c => out.push(c),
            }
        }
        (rest.len() <= used).then_some(out)
    } else {
        let mut args = args;
        let mut raw = false;
        while let Some(first) = args.first() {
            let flags = first.strip_prefix('-').filter(|flags| !flags.is_empty());
            match flags {
                Some(flags) if program == "echo" && flags.chars().all(|c| "neE".contains(c)) => {}
                Some(flags) if program == "print" && flags.chars().all(|c| "nr".contains(c)) => {
                    raw |= flags.contains('r');
                }
                Some(_) if program == "print" => return None,
                _ => break,
            }
            args = &args[1..];
        }
        let escapes = args.iter().any(|word| word.contains('\\'));
        let expands = args.iter().any(|word| word.contains(['$', '`']));
        let unsettled = escapes || (expands && values_unsettled);
        (raw || !unsettled).then(|| args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split(' ').map(ToString::to_string).collect()
    }

    /// Plain literal output is written as the program writes it; anything
    /// else is not settled.
    #[test]
    fn only_plain_literal_output_is_settled() {
        for (program, args, values_unsettled, expected) in [
            ("printf", "%s notes", false, Some("notes")),
            ("printf", r"a%sb%s\n x", true, Some("axb\n")),
            ("printf", "-- %s x", false, Some("x")),
            ("printf", "-v p %s%s no tes", false, Some("notes")),
            ("printf", r"%s a\b", true, Some(r"a\b")),
            ("printf", "%s $x", true, Some("$x")),
            ("echo", "-n notes", false, Some("notes")),
            ("echo", "$HOME", false, Some("$HOME")),
            ("print", "-r notes", false, Some("notes")),
            ("print", r"-r a\x75b", true, Some(r"a\x75b")),
            ("printf", "%s a b", false, None),
            ("printf", "%5s a", false, None),
            ("printf", "%%", false, None),
            ("printf", "%b notes", false, None),
            ("printf", r"a\tb", true, None),
            ("printf", r"%s\\n x", true, None),
            ("printf", "$f no", false, None),
            ("printf", "-v", false, None),
            ("echo", r"a\nb", true, None),
            ("echo", "$x", true, None),
            ("print", "-P %n", false, None),
        ] {
            assert_eq!(
                printed(program, &words(args), values_unsettled),
                expected.map(ToString::to_string),
                "{program} {args}"
            );
        }
        assert!(printed("printf", &[], false).is_none());
    }

    /// A backslash the shell consumes is an escape on the command.
    #[test]
    fn a_backslash_the_shell_consumes_is_an_escape() {
        assert!(shell_escaped(r"printf %s\n x", &words(r"printf %sn x")));
        assert!(shell_escaped(r#"echo "a\\b""#, &words(r"echo a\b")));
        assert!(!shell_escaped(r"printf '%s\n' x", &words(r"printf %s\n x")));
    }

    /// A `$'...'` word is plain only without an escape.
    #[test]
    fn a_dollar_quote_with_an_escape_is_not_settled() {
        assert!(escaped_dollar_quote(r"echo $'a\tb'"));
        assert!(escaped_dollar_quote(r"echo $'a' $'\n'"));
        assert!(!escaped_dollar_quote("echo $'notes'"));
        assert!(!escaped_dollar_quote(r"echo 'a\tb' $x"));
    }

    /// `xargs` runs what it reads only where the text can reach a program
    /// position or code, or under an option this reader does not know.
    #[test]
    fn xargs_runs_its_input_only_into_a_program_or_code() {
        for args in [
            "sh -c",
            "-0 sh -c",
            "-I{} {} update",
            "-I X sh -c X",
            "env",
            "-n1 python3 -c",
            "--unknown wc",
            "-I {} bash -c {}",
            "sudo sh",
            "$cmd",
        ] {
            assert!(xargs_runs_text(&words(args)), "{args}");
        }
        for args in [
            "wc -l",
            "-0 grep foo",
            "-n 1 rm",
            "--null cat",
            "-I{} cp {} dir",
        ] {
            assert!(!xargs_runs_text(&words(args)), "{args}");
        }
        assert!(!xargs_runs_text(&[]));
    }

    /// Settled feeds through each route reach the executor as text; any
    /// other feed is unsettled.
    #[test]
    fn only_settled_text_reaches_an_executor() {
        for line in [
            r"printf 'echo hi\n' | sh",
            "bash <<< 'echo hi'",
            "python3 - <<'EOF'\nprint(1)\nEOF\n",
            "cat <<'E' | sh\nprintf hello\nE\n",
            "cd src && bash <<< 'echo hi'",
            r"printf a | sh; true",
            r"source <(printf a)",
            r"bash <(cat script.sh)",
            r"cat script.sh | bash",
            r#"python3 -c "$(printf '%s\n' 'print(1)')""#,
            r"python3 -c $'print(1)'",
            r"python3 /dev/fd/3 3<<< 'print(1)'",
            r"git -c core.pager=sh log -1",
            r"sh",
            r#""$HOME/bin/tool" data/in.txt"#,
            r"x=ls; $x",
        ] {
            assert_eq!(produced(line, 0, false), None, "{line}");
        }
        for line in [
            r"{ printf a; printf b; } | sh",
            r"printf a | tee x | sh",
            r#"printf "$x" | sh"#,
            r#"bash <<< "$x""#,
            "python3 - <<EOF\n$x\nEOF\n",
            r"printf a |& sh",
            r"printf a & sh",
            r"printf '%x' 1 | sh",
            r#"sh -c "$x""#,
            r"python3 <<< $'print(1\x32)'",
            r"sh <(base64 -d <<< eA==)",
            r"printf a > s.sh; cat s.sh | sh",
            r"sh <&3",
            r"curl -s https://example.com/x | sh",
            r"$(printf '%x' 1) update",
            r"x=$(curl -s https://example.com/x); $x",
        ] {
            assert_eq!(produced(line, 0, false), Some(UNSETTLED), "{line}");
        }
    }

    /// Standard input by the last input redirection.
    #[test]
    fn input_is_read_from_the_last_redirection() {
        assert_eq!(input_of("sh"), Input::Inherited);
        assert_eq!(input_of("sh < script.sh"), Input::File);
        assert_eq!(input_of("sh 2< script.sh"), Input::Inherited);
        assert_eq!(input_of("bash <<< 'echo hi'"), Input::Literal);
        assert_eq!(input_of(r#"bash <<< "$x""#), Input::Produced);
        assert_eq!(input_of("sh < <(printf a)"), Input::Produced);
        assert_eq!(input_of("sh <&3"), Input::Produced);
        assert_eq!(input_of(r#"sh < "$f""#), Input::Produced);
    }
}
