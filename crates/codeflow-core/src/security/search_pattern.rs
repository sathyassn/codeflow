//! Which words of a search tool's call are its search patterns (TSK-380,
//! issue 242).
//!
//! A search pattern is text the tool matches; it never opens, writes or
//! runs it. Both shell guards judged a word of a program they do not read
//! as a path it may write, so a pattern spelled like a path was read as a
//! write: exec-guard's startup check refused `rg '/CHANGELOG.md$'` as an
//! unresolved write, and git-guard's wrapper rules read a pattern that
//! spells or globs an enforcement path (`xargs rg '.claude/settings.json'`)
//! as the line naming that path. This reader names the pattern words of
//! `rg`, `ag`, `grep`, `egrep`, `fgrep` and `git grep`, called directly or
//! run by `xargs`, GNU `parallel` given `-q` or `find -exec`, `-execdir`,
//! `-ok` or `-okdir`. A direct `grep` is a reader whose words neither guard
//! judges, and a direct `git grep` is a git read the startup check passes
//! whole.
//!
//! A pattern word is every `-e` or `--regexp` value, and the first operand
//! when no `-e`, `--regexp`, `-f` or `--file` supplies the pattern and no
//! option drops it (`rg --files`, `ag -g`). `ag` has no `-e` or `--regexp`.
//! Every other word keeps its judgment: a path operand, a `-f` pattern
//! file, `rg --pre` and every other option value.
//!
//! The reader knows each tool's options and which take a value. An option
//! it does not know, a long option abbreviated included, could take the
//! next word as its value and shift which word is the first operand, so it
//! names no pattern word at all and the call is judged as before.
//!
//! ripgrep also reads options from the file `RIPGREP_CONFIG_PATH` names,
//! and one there may supply the pattern, which turns the first operand
//! into a searched path. When the line sets that variable, the first
//! operand is not a pattern word unless `--no-config` is given. A value
//! set outside the line is not visible to the guard; a configuration that
//! supplies both a pattern and a `--pre` program is past the guard, as
//! that program's own writes are, and the OS sandbox is the backstop.
//!
//! Review round 1 narrowed which words count. A word is a pattern word
//! only when the shell passes it as one argument with the text written:
//! quoted, or free of unquoted glob, brace and `$` or backtick expansion.
//! An unquoted `~/.*rc` in the pattern position expands into several
//! words, the first the pattern and the rest searched paths, so it is
//! judged as before. GNU `parallel` joins its command into a shell line
//! unless `-q` or `--quote` is given, so without one no word of its command
//! is a pattern word. The line naming `RIPGREP_CONFIG_PATH` anywhere,
//! through an assignment, `export`, `declare`, `typeset`, `read` or
//! `printf -v`, keeps the `rg` first operand judged.
//!
//! [`preprocessor`] reads the other half of the round: `--pre` or
//! `--pre-glob` makes ripgrep run a program on every file it searches, so
//! the guards judge each root it searches as a recursive writer's target.
//!
//! A pattern word is only left out of the judgment of what a command
//! writes. Its text still counts where a rule reads what the line names,
//! such as the closed rule of TSK-242: search output carries text that
//! matches the pattern, which a later command on the line may run.

use super::startup::git_subcommand;
use crate::hooks::git_guard::{
    basename, command_argv, expand_commands, parallel_template, xargs_command,
};

/// One search tool's option grammar.
struct Grammar {
    /// Short options that take no value.
    flags: &'static str,
    /// Short options that take a value, attached or as the next word.
    valued: &'static str,
    /// Whether a digit is a short option that takes no value (`grep -3`).
    digits: bool,
    /// Long options that take no value.
    long_flags: &'static [&'static str],
    /// Long options that take a value, after `=` or as the next word.
    long_valued: &'static [&'static str],
    /// Long options whose value, when given, follows `=` (`--color=WHEN`).
    long_optional: &'static [&'static str],
    /// The short and long option whose value is a pattern.
    pattern: Option<(char, &'static str)>,
    /// The short and long option whose value is a file of patterns.
    pattern_file: Option<(char, &'static str)>,
    /// Options after which the call takes no pattern operand.
    no_pattern: &'static [&'static str],
}

/// ripgrep's options (`rg -h`, version 15). A `--no-` form of a flag is
/// a flag.
const RG: Grammar = Grammar {
    flags: "zsFivxUPSawL.ubhnN0opqHIclV",
    valued: "efEmjgdtTABCMr",
    digits: false,
    long_flags: &[
        "search-zip",
        "case-sensitive",
        "crlf",
        "fixed-strings",
        "ignore-case",
        "invert-match",
        "line-regexp",
        "mmap",
        "multiline",
        "multiline-dotall",
        "unicode",
        "null-data",
        "pcre2",
        "smart-case",
        "stop-on-nonmatch",
        "text",
        "word-regexp",
        "auto-hybrid-regex",
        "pcre2-unicode",
        "binary",
        "follow",
        "glob-case-insensitive",
        "hidden",
        "ignore-file-case-insensitive",
        "ignore",
        "ignore-dot",
        "ignore-exclude",
        "ignore-files",
        "ignore-global",
        "ignore-parent",
        "ignore-vcs",
        "require-git",
        "one-file-system",
        "unrestricted",
        "block-buffered",
        "byte-offset",
        "column",
        "heading",
        "help",
        "include-zero",
        "line-buffered",
        "line-number",
        "max-columns-preview",
        "null",
        "only-matching",
        "passthru",
        "passthrough",
        "pretty",
        "quiet",
        "trim",
        "vimgrep",
        "with-filename",
        "filename",
        "sort-files",
        "count",
        "count-matches",
        "files-with-matches",
        "files-without-match",
        "json",
        "debug",
        "ignore-messages",
        "messages",
        "stats",
        "trace",
        "files",
        "no-config",
        "pcre2-version",
        "type-list",
        "version",
        "no-encoding",
        "no-context-separator",
    ],
    long_valued: &[
        "regexp",
        "file",
        "pre",
        "pre-glob",
        "dfa-size-limit",
        "encoding",
        "engine",
        "max-count",
        "regex-size-limit",
        "threads",
        "glob",
        "iglob",
        "ignore-file",
        "max-depth",
        "max-filesize",
        "type",
        "type-not",
        "type-add",
        "type-clear",
        "after-context",
        "before-context",
        "color",
        "colors",
        "context",
        "context-separator",
        "field-context-separator",
        "field-match-separator",
        "hostname-bin",
        "hyperlink-format",
        "max-columns",
        "path-separator",
        "replace",
        "sort",
        "sortr",
        "generate",
    ],
    long_optional: &[],
    pattern: Some(('e', "regexp")),
    pattern_file: Some(('f', "file")),
    no_pattern: &[
        "--files",
        "--type-list",
        "--help",
        "-h",
        "--version",
        "-V",
        "--pcre2-version",
        "--generate",
    ],
};

/// The silver searcher's options (`ag` 2.2). It has no `-e`; `-f` is
/// `--follow`, and `-g` lists file names matching its value, so the call
/// then takes no search pattern.
const AG: Grammar = Grammar {
    flags: "acDFfHhiLlnoQRrSsVvtuUwz0",
    valued: "ABCGgmpW",
    digits: false,
    long_flags: &[
        "ackmate",
        "affinity",
        "all-text",
        "all-types",
        "break",
        "case-sensitive",
        "color",
        "color-win-ansi",
        "column",
        "count",
        "debug",
        "filename",
        "files-with-matches",
        "files-without-matches",
        "fixed-strings",
        "follow",
        "group",
        "heading",
        "help",
        "hidden",
        "ignore-case",
        "invert-match",
        "line-numbers",
        "list-file-types",
        "literal",
        "mmap",
        "multiline",
        "numbers",
        "one-device",
        "only-matching",
        "parallel",
        "passthrough",
        "passthru",
        "print-all-files",
        "print-long-lines",
        "print0",
        "null",
        "recurse",
        "search-binary",
        "search-zip",
        "silent",
        "skip-vcs-ignores",
        "smart-case",
        "stats",
        "stats-only",
        "unrestricted",
        "version",
        "vimgrep",
        "word-regexp",
        "noaffinity",
        "nobreak",
        "nocolor",
        "nofilename",
        "nofollow",
        "nogroup",
        "noheading",
        "nomultiline",
        "nonumbers",
        "nommap",
        "norecurse",
        "no-affinity",
        "no-break",
        "no-color",
        "no-filename",
        "no-follow",
        "no-group",
        "no-heading",
        "no-multiline",
        "no-numbers",
        "no-mmap",
        "no-recurse",
    ],
    long_valued: &[
        "ackmate-dir-filter",
        "after",
        "before",
        "context",
        "color-line-number",
        "color-match",
        "color-path",
        "depth",
        "file-search-regex",
        "filename-pattern",
        "ignore",
        "ignore-dir",
        "max-count",
        "pager",
        "path-to-ignore",
        "width",
        "workers",
    ],
    long_optional: &[],
    pattern: None,
    pattern_file: None,
    no_pattern: &[
        "-g",
        "--filename-pattern",
        "--list-file-types",
        "--help",
        "-h",
        "--version",
        "-V",
    ],
};

/// GNU and BSD grep's options, as `grep`, `egrep` and `fgrep` take them.
/// `-NUM` sets the context. `-X` takes a value in GNU grep and none in
/// BSD grep, so it is not read.
const GREP: Grammar = Grammar {
    flags: "abcEFGHhIiJLlMnOoPpqRrSsTUuVvwxyZz",
    valued: "efmABCdD",
    digits: true,
    long_flags: &[
        "extended-regexp",
        "fixed-strings",
        "basic-regexp",
        "perl-regexp",
        "ignore-case",
        "no-ignore-case",
        "word-regexp",
        "line-regexp",
        "null-data",
        "no-messages",
        "invert-match",
        "version",
        "help",
        "byte-offset",
        "line-number",
        "no-line-number",
        "line-buffered",
        "with-filename",
        "no-filename",
        "only-matching",
        "quiet",
        "silent",
        "text",
        "recursive",
        "dereference-recursive",
        "files-without-match",
        "files-with-matches",
        "count",
        "initial-tab",
        "null",
        "unix-byte-offsets",
        "color",
        "colour",
        "bz2decompress",
        "decompress",
        "mmap",
        "no-group-separator",
    ],
    long_valued: &[
        "regexp",
        "file",
        "max-count",
        "label",
        "after-context",
        "before-context",
        "context",
        "directories",
        "devices",
        "include",
        "exclude",
        "exclude-from",
        "exclude-dir",
        "binary-files",
        "group-separator",
    ],
    long_optional: &["color", "colour"],
    pattern: Some(('e', "regexp")),
    pattern_file: Some(('f', "file")),
    no_pattern: &["--help", "--version", "-V"],
};

/// `git grep`'s options (git 2.4x `git grep -h`). It has no `--regexp`;
/// `-O` opens a pager and is not read.
const GIT_GREP: Grammar = Grammar {
    flags: "vaiIwrhHEGFPnlLzocpWq",
    valued: "efABCm",
    digits: true,
    long_flags: &[
        "cached",
        "index",
        "untracked",
        "exclude-standard",
        "recurse-submodules",
        "invert-match",
        "ignore-case",
        "word-regexp",
        "text",
        "textconv",
        "recursive",
        "full-name",
        "extended-regexp",
        "basic-regexp",
        "fixed-strings",
        "perl-regexp",
        "line-number",
        "column",
        "files-with-matches",
        "name-only",
        "files-without-match",
        "null",
        "only-matching",
        "count",
        "color",
        "break",
        "heading",
        "show-function",
        "function-context",
        "and",
        "or",
        "not",
        "all-match",
        "quiet",
    ],
    long_valued: &[
        "max-depth",
        "threads",
        "after-context",
        "before-context",
        "context",
        "max-count",
    ],
    long_optional: &["color"],
    pattern: Some(('e', "")),
    pattern_file: Some(('f', "")),
    no_pattern: &[],
};

/// The grammar of a search tool, by program name.
fn grammar(program: &str) -> Option<&'static Grammar> {
    match program {
        "rg" => Some(&RG),
        "ag" => Some(&AG),
        "grep" | "egrep" | "fgrep" => Some(&GREP),
        "git grep" => Some(&GIT_GREP),
        _ => None,
    }
}

/// The indices into `args` of the words `program` takes as search
/// patterns, or that a search tool it runs takes: the command of an
/// `xargs`, of a GNU `parallel` given `-q`, and of each `find -exec`,
/// `-execdir`, `-ok` or `-okdir`. Empty when no search tool this reader
/// knows runs, or when its call holds an option the reader does not know.
///
/// `literal` holds, for each word of `args`, whether the shell passes it
/// as one argument with the text written ([`command_argv_literal`]); any
/// other word is never a pattern word, since the shell may expand it into
/// several words, the first the pattern and the rest searched paths
/// (review round 1). `configured` says the line names
/// `RIPGREP_CONFIG_PATH` ([`names_ripgrep_config`]).
///
/// [`command_argv_literal`]: crate::hooks::git_guard::command_argv_literal
pub(crate) fn pattern_words(
    program: &str,
    args: &[String],
    literal: &[bool],
    configured: bool,
) -> Vec<usize> {
    let run = |start: usize, command: &[String]| -> Vec<usize> {
        command
            .split_first()
            .map_or_else(Vec::new, |(program, rest)| {
                own_pattern_words(basename(program), rest, configured)
                    .into_iter()
                    .map(|at| start + 1 + at)
                    .collect()
            })
    };
    let words = match program {
        "xargs" => xargs_command(args)
            .map_or_else(Vec::new, |command| run(args.len() - command.len(), command)),
        // GNU parallel joins its command into a shell line unless `-q`, so
        // a word quoted for the outer shell is expanded again.
        "parallel" => match parallel_command(args) {
            Some((start, end, true)) => run(start, &args[start..end]),
            _ => Vec::new(),
        },
        "find" => find_commands(args)
            .into_iter()
            .flat_map(|(start, end)| run(start, &args[start..end]))
            .collect(),
        _ => own_pattern_words(program, args, configured),
    };
    words
        .into_iter()
        .filter(|at| literal.get(*at).copied().unwrap_or(false))
        .collect()
}

/// The words of `args` that are not [`pattern_words`], in order: what a
/// guard judges as paths the call may write.
pub(crate) fn other_words<'a>(
    program: &str,
    args: &'a [String],
    literal: &[bool],
    configured: bool,
) -> Vec<&'a String> {
    let patterns = pattern_words(program, args, literal, configured);
    args.iter()
        .enumerate()
        .filter(|(at, _)| !patterns.contains(at))
        .map(|(_, word)| word)
        .collect()
}

/// Search file operands and option values, using the same option arity and
/// pattern-text clearance as both writer guards. Unknown options clear no text.
pub(crate) fn path_values<'a>(
    program: &str,
    args: &'a [String],
    literal: &[bool],
    configured: bool,
) -> Option<Vec<(usize, &'a str, bool)>> {
    let grammar = grammar(program)?;
    let call = read_call(grammar, args)?;
    let patterns = pattern_words(program, args, literal, configured);
    let mut paths = Vec::new();
    for at in call.operands {
        if !patterns.contains(&at) {
            paths.push((at, args[at].as_str(), false));
        }
    }
    for (at, offset, glob) in call.option_values {
        if !patterns.contains(&at) {
            if let Some(value) = args.get(at).and_then(|word| word.get(offset..)) {
                paths.push((at, value, glob));
            }
        }
    }
    Some(paths)
}

/// The path operands a search tool walks, excluding its pattern text and
/// option values. `None` means its option grammar cannot prove the roots.
pub(crate) fn search_roots<'a>(
    program: &str,
    args: &'a [String],
    literal: &[bool],
    configured: bool,
) -> Option<Vec<&'a str>> {
    let call = read_call(grammar(program)?, args)?;
    let patterns = pattern_words(program, args, literal, configured);
    Some(
        call.operands
            .into_iter()
            .filter(|at| !patterns.contains(at))
            .map(|at| args[at].as_str())
            .collect(),
    )
}

/// Whether the command line names `RIPGREP_CONFIG_PATH` anywhere, as
/// written or as the shell decodes its words: an assignment, `export`,
/// `declare`, `typeset`, `read`, `printf -v` or any other use may set the
/// file ripgrep reads options from (review round 1). Reading the value
/// counts too; that only keeps the first operand judged.
pub(crate) fn names_ripgrep_config(line: &str) -> bool {
    const NAME: &str = "RIPGREP_CONFIG_PATH";
    line.contains(NAME)
        || expand_commands(line)
            .iter()
            .flat_map(|segment| command_argv(segment))
            .any(|word| word.contains(NAME))
}

/// How a call runs a ripgrep preprocessor, the program `--pre` names,
/// which ripgrep runs on every file it searches (review round 1, P-1).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Preprocessor {
    /// No `--pre` or `--pre-glob` is given to a ripgrep the call runs.
    None,
    /// The indices into `args` of the roots it searches; empty when it
    /// searches the working directory.
    Roots(Vec<usize>),
    /// It runs on files the call's input names (`xargs`, `parallel`),
    /// which the guard cannot see.
    FromInput,
    /// Its options cannot be read, so its roots are unknown.
    Unread,
}

/// How `program` with `args` runs a ripgrep preprocessor: directly, as the
/// command of `xargs` or `parallel`, or from a `find -exec` command, whose
/// roots are `find`'s starting points with the command's own roots.
pub(crate) fn preprocessor(
    program: &str,
    args: &[String],
    literal: &[bool],
    configured: bool,
) -> Preprocessor {
    let runs = |command: &[String]| {
        command.split_first().is_some_and(|(program, rest)| {
            basename(program) == "rg" && rest.iter().any(|a| names_preprocessor(a))
        })
    };
    match program {
        "xargs" => xargs_command(args)
            .filter(|command| runs(command))
            .map_or(Preprocessor::None, |_| Preprocessor::FromInput),
        "parallel" => match parallel_command(args) {
            Some((start, end, _)) if runs(&args[start..end]) => Preprocessor::FromInput,
            _ => Preprocessor::None,
        },
        "find" => {
            let mut roots = Vec::new();
            let mut found = false;
            for (start, end) in find_commands(args) {
                let command = &args[start..end];
                if !runs(command) {
                    continue;
                }
                found = true;
                let literal = literal.get(start + 1..end).unwrap_or_default();
                match own_roots(&command[1..], literal, configured) {
                    Some(own) => roots.extend(
                        own.into_iter()
                            .map(|at| start + 1 + at)
                            .filter(|at| args[*at] != "{}"),
                    ),
                    None => return Preprocessor::Unread,
                }
            }
            if !found {
                return Preprocessor::None;
            }
            let skip = args
                .iter()
                .take_while(|a| matches!(a.as_str(), "-H" | "-L" | "-P"))
                .count();
            roots.extend(
                (skip..args.len())
                    .take_while(|at| {
                        let a = args[*at].as_str();
                        !a.starts_with('-') && !matches!(a, "!" | "(")
                    })
                    .collect::<Vec<_>>(),
            );
            // Without a starting point `find` walks its working directory.
            if roots.is_empty() {
                Preprocessor::Roots(Vec::new())
            } else {
                Preprocessor::Roots(roots)
            }
        }
        "rg" if args.iter().any(|a| names_preprocessor(a)) => {
            own_roots(args, literal, configured).map_or(Preprocessor::Unread, Preprocessor::Roots)
        }
        _ => Preprocessor::None,
    }
}

/// Whether `arg` gives ripgrep's `--pre` or `--pre-glob`.
fn names_preprocessor(arg: &str) -> bool {
    ["--pre", "--pre-glob"].iter().any(|option| {
        arg == *option || arg.strip_prefix(option).is_some_and(|r| r.starts_with('='))
    })
}

/// The roots a ripgrep call searches: every operand that is not a pattern
/// word, or `None` when its options cannot be read.
fn own_roots(args: &[String], literal: &[bool], configured: bool) -> Option<Vec<usize>> {
    let call = read_call(&RG, args)?;
    let patterns: Vec<usize> = pattern_words("rg", args, literal, configured);
    Some(
        call.operands
            .into_iter()
            .filter(|at| !patterns.contains(at))
            .collect(),
    )
}

/// GNU `parallel`'s command, as the range of `args` it spans up to its
/// first input separator, and whether `-q` or `--quote` is given.
fn parallel_command(args: &[String]) -> Option<(usize, usize, bool)> {
    let (mut start, quoted) = parallel_template(args);
    if args.get(start).is_some_and(|a| a == "--") {
        start += 1;
    }
    let end = (start..args.len())
        .find(|at| matches!(args[*at].as_str(), ":::" | ":::+" | "::::" | "::::+"))
        .unwrap_or(args.len());
    (start < end).then_some((start, end, quoted))
}

/// The commands a `find` runs for its matches, each as the range of
/// `args` it spans: from the word after `-exec`, `-execdir`, `-ok` or
/// `-okdir` up to the `;` or the `{} +` that ends it.
fn find_commands(args: &[String]) -> Vec<(usize, usize)> {
    let mut commands = Vec::new();
    let mut at = 0;
    while at < args.len() {
        if matches!(args[at].as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            let start = at + 1;
            let mut end = start;
            while end < args.len()
                && args[end] != ";"
                && !(args[end] == "+" && end > start && args[end - 1] == "{}")
            {
                end += 1;
            }
            commands.push((start, end));
            at = end;
        }
        at += 1;
    }
    commands
}

/// The pattern words of a search tool's own call, `git grep` after git's
/// global options included.
fn own_pattern_words(program: &str, args: &[String], configured: bool) -> Vec<usize> {
    if program == "git" {
        return git_subcommand(args)
            .filter(|(sub, _)| *sub == "grep")
            .map_or_else(Vec::new, |(_, rest)| {
                let start = args.len() - rest.len();
                own_pattern_words("git grep", rest, configured)
                    .into_iter()
                    .map(|at| start + at)
                    .collect()
            });
    }
    let Some(grammar) = grammar(program) else {
        return Vec::new();
    };
    let Some(call) = read_call(grammar, args) else {
        return Vec::new();
    };
    let mut words = call.option_patterns;
    // ripgrep's configuration file may supply the pattern, which then
    // makes the first operand a searched path.
    let configured = program == "rg" && !call.no_config && configured;
    if !call.supplied && !call.no_pattern && !configured {
        words.extend(call.operands.first());
    }
    words
}

/// What [`read_call`] finds in one call.
struct Call {
    /// The indices of the `-e` and `--regexp` values.
    option_patterns: Vec<usize>,
    /// Option value location, byte offset in its word, and path-glob semantics.
    option_values: Vec<(usize, usize, bool)>,
    /// The indices of the operands, the first one first.
    operands: Vec<usize>,
    /// An option supplies the pattern (`-e`, `--regexp`, `-f`, `--file`).
    supplied: bool,
    /// An option drops the pattern operand (`rg --files`).
    no_pattern: bool,
    /// `--no-config` is given.
    no_config: bool,
}

/// Read a call by `grammar`, or `None` at an option it does not know.
fn read_call(grammar: &Grammar, args: &[String]) -> Option<Call> {
    let mut call = Call {
        option_patterns: Vec::new(),
        option_values: Vec::new(),
        operands: Vec::new(),
        supplied: false,
        no_pattern: false,
        no_config: false,
    };
    let mut options = true;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let a = arg.as_str();
        if options && a == "--" {
            options = false;
        } else if !options || a == "-" || !a.starts_with('-') {
            call.operands.push(at);
        } else if let Some(long) = a.strip_prefix("--") {
            at = read_long(grammar, args, at, long, &mut call)?;
        } else {
            at = read_short(grammar, args, at, &a[1..], &mut call)?;
        }
        at += 1;
    }
    Some(call)
}

/// Read the long option `--long` at `at`; the index of its last word.
fn read_long(
    grammar: &Grammar,
    args: &[String],
    at: usize,
    long: &str,
    call: &mut Call,
) -> Option<usize> {
    let (name, value) = long
        .split_once('=')
        .map_or((long, None), |(name, value)| (name, Some(value)));
    if grammar.no_pattern.contains(&format!("--{name}").as_str()) {
        call.no_pattern = true;
    }
    if name == "no-config" {
        call.no_config = true;
    }
    let flag = grammar.long_flags.contains(&name)
        || name
            .strip_prefix("no-")
            .is_some_and(|base| grammar.long_flags.contains(&base));
    if (value.is_none() && flag) || grammar.long_optional.contains(&name) {
        return Some(at);
    }
    if !grammar.long_valued.contains(&name) {
        return None;
    }
    let value_at = if value.is_some() { at } else { at + 1 };
    let offset = value.map_or(0, |value| args[at].len() - value.len());
    call.option_values.push((
        value_at,
        offset,
        matches!(
            name,
            "glob" | "iglob" | "include" | "exclude" | "exclude-dir" | "pre-glob"
        ),
    ));
    if grammar.pattern.is_some_and(|(_, long)| long == name) {
        call.supplied = true;
        if value_at < args.len() {
            call.option_patterns.push(value_at);
        }
    } else if grammar.pattern_file.is_some_and(|(_, long)| long == name) {
        call.supplied = true;
    }
    Some(value_at)
}

/// Read the short option cluster `-cluster` at `at`; the index of its last
/// word.
fn read_short(
    grammar: &Grammar,
    args: &[String],
    at: usize,
    cluster: &str,
    call: &mut Call,
) -> Option<usize> {
    for (offset, letter) in cluster.char_indices() {
        if grammar.flags.contains(letter) || (grammar.digits && letter.is_ascii_digit()) {
            if grammar.no_pattern.contains(&format!("-{letter}").as_str()) {
                call.no_pattern = true;
            }
            continue;
        }
        if !grammar.valued.contains(letter) {
            return None;
        }
        if grammar.no_pattern.contains(&format!("-{letter}").as_str()) {
            call.no_pattern = true;
        }
        let attached = !cluster[offset + letter.len_utf8()..].is_empty();
        let value_at = if attached { at } else { at + 1 };
        let value_offset = if attached {
            1 + offset + letter.len_utf8()
        } else {
            0
        };
        call.option_values
            .push((value_at, value_offset, letter == 'g'));
        if grammar.pattern.is_some_and(|(short, _)| short == letter) {
            call.supplied = true;
            if value_at < args.len() {
                call.option_patterns.push(value_at);
            }
        } else if grammar
            .pattern_file
            .is_some_and(|(short, _)| short == letter)
        {
            call.supplied = true;
        }
        return Some(value_at);
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// The pattern words of `line`, a program and its arguments split at
    /// blanks, every word read as written: the grammar alone.
    fn patterns(line: &str) -> Vec<String> {
        patterns_with(line, false)
    }

    fn patterns_with(line: &str, configured: bool) -> Vec<String> {
        let all = words(line);
        let args = &all[1..];
        pattern_words(&all[0], args, &vec![true; args.len()], configured)
            .into_iter()
            .map(|at| args[at].clone())
            .collect()
    }

    /// The pattern words of `line` as the shell parses it.
    fn shell_patterns(line: &str) -> Vec<String> {
        let (all, literal) = crate::hooks::git_guard::command_argv_literal(line);
        let args = &all[1..];
        pattern_words(&all[0], args, &literal[1..], names_ripgrep_config(line))
            .into_iter()
            .map(|at| args[at].clone())
            .collect()
    }

    /// The preprocessor reading of `line` as the shell parses it, roots by
    /// their words.
    fn roots(line: &str) -> Result<Vec<String>, Preprocessor> {
        let (all, literal) = crate::hooks::git_guard::command_argv_literal(line);
        let args = &all[1..];
        match preprocessor(&all[0], args, &literal[1..], false) {
            Preprocessor::Roots(roots) => {
                Ok(roots.into_iter().map(|at| args[at].clone()).collect())
            }
            other => Err(other),
        }
    }

    #[test]
    fn tsk1002_search_paths_retain_attached_file_and_glob_values() {
        for (program, line, expected) in [
            (
                "rg",
                "-ugid_* token .",
                vec![(0, "id_*", true), (2, ".", false)],
            ),
            (
                "grep",
                "-nfid_rsa token",
                vec![(0, "id_rsa", false), (1, "token", false)],
            ),
            (
                "rg",
                "--iglob=*.PEM token .",
                vec![(0, "*.PEM", true), (2, ".", false)],
            ),
        ] {
            let args = words(line);
            let paths = path_values(program, &args, &vec![true; args.len()], false).unwrap();
            for path in expected {
                assert!(paths.contains(&path), "{line}: {paths:?}");
            }
        }
        let args = words("-m 1 id_rsa README.md");
        assert!(!path_values("rg", &args, &vec![true; args.len()], false)
            .unwrap()
            .iter()
            .any(|(_, value, _)| *value == "id_rsa"));
    }

    #[test]
    fn the_first_operand_is_the_pattern() {
        assert_eq!(patterns("rg /CHANGELOG.md$"), ["/CHANGELOG.md$"]);
        assert_eq!(patterns("rg -n -C 2 /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg -A2 -ttoml /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg --max-count 1 /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg --max-count=1 /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg --no-heading -- -x docs"), ["-x"]);
        assert_eq!(patterns("ag -C 2 ^/etc/.* docs"), ["^/etc/.*"]);
        assert_eq!(patterns("grep -rn /etc/hosts docs/"), ["/etc/hosts"]);
        assert_eq!(patterns("egrep -3 ^/etc/.* f"), ["^/etc/.*"]);
        assert_eq!(patterns("fgrep --include *.md $HOME/x ."), ["$HOME/x"]);
    }

    #[test]
    fn an_option_supplies_the_pattern() {
        assert_eq!(patterns("rg -e /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg -e/x$ docs"), ["-e/x$"]);
        assert_eq!(patterns("rg -ie /x$ docs"), ["/x$"]);
        assert_eq!(patterns("rg --regexp /x$ -e ^/y docs"), ["/x$", "^/y"]);
        assert_eq!(patterns("rg --regexp=/x$ docs"), ["--regexp=/x$"]);
        assert_eq!(patterns("grep -e /x$ -- -e"), ["/x$"]);
        // A pattern file makes every operand a path.
        assert!(patterns("rg -f pats /x$").is_empty());
        assert!(patterns("grep --file=pats /x$").is_empty());
        assert_eq!(patterns("rg -f pats -e /y /x$"), ["/y"]);
    }

    #[test]
    fn a_call_without_a_pattern_operand_names_none() {
        assert!(patterns("rg --files /x$").is_empty());
        assert!(patterns("rg --type-list /x$").is_empty());
        assert!(patterns("ag -g /x$ docs").is_empty());
        assert!(patterns("ag --filename-pattern=x /y$").is_empty());
    }

    #[test]
    fn an_unknown_option_names_no_pattern() {
        assert!(patterns("rg --frobnicate /x$ docs").is_empty());
        assert!(patterns("rg -K /x$ docs").is_empty());
        // An abbreviated long option is not read.
        assert!(patterns("grep --regex /x$ docs").is_empty());
        // ag has no `-e`.
        assert!(patterns("ag -e /x$ docs").is_empty());
        assert!(patterns("sed /x$ docs").is_empty());
    }

    #[test]
    fn option_values_stay_out_of_the_pattern() {
        assert_eq!(patterns("rg --pre ./x pat docs"), ["pat"]);
        assert_eq!(patterns("rg --pre=./x pat docs"), ["pat"]);
        assert_eq!(patterns("rg -j 4 pat docs"), ["pat"]);
        assert_eq!(patterns("ag --pager less pat docs"), ["pat"]);
        assert_eq!(patterns("grep -m1 -d skip pat docs"), ["pat"]);
    }

    #[test]
    fn a_search_tool_another_program_runs_has_its_pattern_read() {
        assert_eq!(patterns("xargs -0 grep -l /x$"), ["/x$"]);
        assert_eq!(patterns("xargs -I {} rg -e /x$ {}"), ["/x$"]);
        // GNU parallel runs its command through a shell unless `-q`.
        assert!(patterns("parallel rg /x$ ::: docs").is_empty());
        assert_eq!(patterns("parallel -q -j 4 rg /x$ ::: docs"), ["/x$"]);
        assert_eq!(
            patterns("parallel --jobs 4 --quote rg /x$ ::: docs"),
            ["/x$"]
        );
        assert_eq!(patterns("xargs git grep /x$"), ["/x$"]);
        assert_eq!(patterns("xargs git -C sub grep -n -e /x$"), ["/x$"]);
        assert_eq!(patterns("find docs -exec git grep /x$ {} +"), ["/x$"]);
        assert_eq!(
            patterns("find . -exec rg /x$ {} ; -execdir egrep -e /y {} +"),
            ["/x$", "/y"]
        );
        assert!(patterns("xargs sed /x$").is_empty());
        assert!(patterns("find . -name /x$ -delete").is_empty());
    }

    #[test]
    fn a_ripgrep_configuration_the_line_names_may_supply_the_pattern() {
        assert!(patterns_with("rg /x$ docs", true).is_empty());
        assert_eq!(patterns_with("rg -e /x$ docs", true), ["/x$"]);
        assert_eq!(patterns_with("rg --no-config /x$ docs", true), ["/x$"]);
        assert_eq!(patterns_with("grep /x$ docs", true), ["/x$"]);
        for line in [
            "RIPGREP_CONFIG_PATH=rc rg x",
            "read -r RIPGREP_CONFIG_PATH <<< rc; rg x",
            "printf -v RIPGREP_CONFIG_PATH rc; rg x",
            "declare -x RIPGREP_CONFIG_PATH; rg x",
            "typeset RIPGREP_CONFIG_PATH=rc; rg x",
            "export RIPGREP_CONF''IG_PATH=rc; rg x",
        ] {
            assert!(names_ripgrep_config(line), "{line}");
        }
        assert!(!names_ripgrep_config("rg x docs"));
    }

    #[test]
    fn a_word_the_shell_can_expand_is_never_a_pattern() {
        assert_eq!(
            shell_patterns("rg '/CHANGELOG.md$' docs"),
            ["/CHANGELOG.md$"]
        );
        assert_eq!(shell_patterns("rg \"$HOME/x\" docs"), ["$HOME/x"]);
        assert_eq!(shell_patterns("rg -e '^/etc/.*' docs"), ["^/etc/.*"]);
        assert_eq!(shell_patterns("rg 'a b'c docs"), ["a bc"]);
        for line in [
            "rg ~/.*rc",
            "rg $HOME/.*rc",
            "rg ~/.[bz]*",
            "rg ~/.?shrc",
            "rg ~/.{a,zsh}rc",
            "rg ~/.zshrc",
            "rg $P docs",
            "rg `echo x` docs",
            "rg $(echo x) docs",
            "rg -e x* docs",
            "rg =ls docs",
            "xargs rg ~/.*rc",
            "find . -exec rg ~/.?shrc {} +",
            "parallel -q rg ~/.*rc ::: x",
            "parallel rg '/x$' ::: x",
        ] {
            assert!(shell_patterns(line).is_empty(), "{line}");
        }
    }

    #[test]
    fn a_preprocessor_judges_every_root_it_searches() {
        assert_eq!(roots("rg x docs"), Err(Preprocessor::None));
        assert_eq!(roots("rg --pre tee x docs"), Ok(vec!["docs".to_string()]));
        assert_eq!(roots("rg --pre tee x"), Ok(vec![]));
        assert_eq!(
            roots("rg --hidden --pre=tee x ~ docs").map(|r| r.len()),
            Ok(2)
        );
        assert_eq!(
            roots("rg --pre-glob '*.pdf' x a"),
            Ok(vec!["a".to_string()])
        );
        // A word the shell expands is a root, not a pattern.
        assert_eq!(roots("rg --pre tee ~/.*rc").map(|r| r.len()), Ok(1));
        assert_eq!(roots("rg --files --pre tee a b").map(|r| r.len()), Ok(2));
        assert_eq!(
            roots("rg --frobnicate --pre tee x"),
            Err(Preprocessor::Unread)
        );
        assert_eq!(roots("xargs rg --pre tee x"), Err(Preprocessor::FromInput));
        assert_eq!(
            roots("parallel -j 2 rg --pre tee x ::: a"),
            Err(Preprocessor::FromInput)
        );
        assert_eq!(
            roots("find a b -exec rg --pre tee x {} +"),
            Ok(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(roots("find -exec rg --pre tee x {} +"), Ok(vec![]));
        assert_eq!(roots("find a -exec rg x {} +"), Err(Preprocessor::None));
    }
}
