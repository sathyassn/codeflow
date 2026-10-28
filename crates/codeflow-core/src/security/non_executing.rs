//! The programs whose words the TSK-397 directory check may leave unjudged.
//!
//! Where launcher traversal stops, at the start of a command or inside a
//! launcher, the program may run an interpreter with the words after it, so
//! those words are judged as its possible operands. The one exception is a
//! program on this allowlist, written literally, used with the options its
//! entry lists (operator decision, 2026-10-27; TSK-397 Codex review round
//! 14, security review round 9). Every entry names the options it may take:
//! any other option, an abbreviation of a long option, or an option whose
//! name is not literal makes the program not exempt. A program with an
//! option that runs a program, writes a named file or evaluates a word as
//! code (`rg --pre`, `sort --compress-program`, `printf -v`, `test -v`) is
//! also not exempt where a word may become an option at run time: a word
//! that starts with an expansion or a pattern, or arguments `xargs` appends.
//!
//! The allowlist judges the command text. A shell function or alias that
//! shadows one of these names, or a configuration file the program reads
//! (`RIPGREP_CONFIG_PATH`), is outside the text; the OS sandbox's denials
//! on protected paths are the backstop for them.

use super::shell_lex::Token;
use crate::hooks::git_guard::{has_glob, program_name};

/// One allowlisted program and the options it may take.
struct Entry {
    name: &'static str,
    /// Short options that take no value.
    flags: &'static str,
    /// Short options that take a value, attached or in the next word.
    valued: &'static str,
    /// Long options that take no value, or an optional one after `=`.
    long_flags: &'static [&'static str],
    /// Long options that take a value, after `=` or in the next word.
    long_valued: &'static [&'static str],
    /// Some option outside the lists runs a program, writes a named file or
    /// evaluates a word as code, so a word that may become an option at
    /// run time makes the program not exempt.
    risky: bool,
    /// `-NUM` is a count (`head -5`).
    numeric: bool,
    /// The most operands the program takes without writing a file
    /// (`uniq IN OUT` writes OUT); `None` for any number.
    operands: Option<usize>,
}

const GREP_LONG_FLAGS: &[&str] = &[
    "extended-regexp",
    "fixed-strings",
    "basic-regexp",
    "perl-regexp",
    "ignore-case",
    "no-ignore-case",
    "invert-match",
    "word-regexp",
    "line-regexp",
    "count",
    "files-with-matches",
    "files-without-match",
    "only-matching",
    "quiet",
    "silent",
    "no-messages",
    "byte-offset",
    "with-filename",
    "no-filename",
    "line-number",
    "initial-tab",
    "null",
    "null-data",
    "text",
    "recursive",
    "dereference-recursive",
    "color",
    "colour",
    "help",
    "version",
];

const GREP_LONG_VALUED: &[&str] = &[
    "regexp",
    "file",
    "max-count",
    "after-context",
    "before-context",
    "context",
    "include",
    "exclude",
    "exclude-dir",
    "exclude-from",
    "label",
    "binary-files",
    "devices",
    "directories",
];

const GREP: Entry = Entry {
    name: "grep",
    flags: "EFGPiyvwxclLoqsbHhnTZzaIrRU",
    valued: "efmABCdD",
    long_flags: GREP_LONG_FLAGS,
    long_valued: GREP_LONG_VALUED,
    risky: false,
    numeric: true,
    operands: None,
};

/// ripgrep's and the silver searcher's search options. `--pre`,
/// `--pre-glob`, `--hostname-bin` and `-z`/`--search-zip` (which runs
/// decompression programs) are not here, nor is `ag --pager`.
const SEARCH_LONG_FLAGS: &[&str] = &[
    "files",
    "hidden",
    "no-hidden",
    "no-ignore",
    "no-ignore-vcs",
    "no-ignore-parent",
    "no-ignore-dot",
    "no-ignore-global",
    "no-ignore-exclude",
    "ignore-case",
    "smart-case",
    "case-sensitive",
    "fixed-strings",
    "word-regexp",
    "line-regexp",
    "invert-match",
    "count",
    "count-matches",
    "files-with-matches",
    "files-without-match",
    "line-number",
    "no-line-number",
    "with-filename",
    "no-filename",
    "only-matching",
    "quiet",
    "json",
    "vimgrep",
    "heading",
    "no-heading",
    "trim",
    "follow",
    "multiline",
    "multiline-dotall",
    "pcre2",
    "text",
    "null",
    "null-data",
    "unrestricted",
    "stats",
    "column",
    "no-column",
    "no-messages",
    "passthru",
    "byte-offset",
    "crlf",
    "one-file-system",
    "glob-case-insensitive",
    "no-config",
    "no-mmap",
    "mmap",
    "binary",
    "no-require-git",
    "sort-files",
    "literal",
    "nocolor",
    "color",
    "group",
    "nogroup",
    "filename",
    "nofilename",
    "numbers",
    "nonumbers",
    "help",
    "version",
];

const SEARCH_LONG_VALUED: &[&str] = &[
    "regexp",
    "file",
    "glob",
    "iglob",
    "type",
    "type-not",
    "type-add",
    "max-count",
    "max-depth",
    "max-filesize",
    "context",
    "after-context",
    "before-context",
    "colors",
    "sort",
    "sortr",
    "threads",
    "replace",
    "encoding",
    "ignore-file",
    "engine",
    "path-separator",
    "max-columns",
    "file-search-regex",
    "depth",
    "ignore",
    "ignore-dir",
];

const RG: Entry = Entry {
    name: "rg",
    flags: "iSsFxwvcnNHIoquLlUPa0bp.",
    valued: "efgtTmABCjMErd",
    long_flags: SEARCH_LONG_FLAGS,
    long_valued: SEARCH_LONG_VALUED,
    risky: true,
    numeric: false,
    operands: None,
};

const AG: Entry = Entry {
    name: "ag",
    flags: "aiQsSvwlLcfHUtuo0",
    valued: "ABCGgmp",
    long_flags: SEARCH_LONG_FLAGS,
    long_valued: SEARCH_LONG_VALUED,
    risky: true,
    numeric: false,
    operands: None,
};

const ENTRIES: &[Entry] = &[
    GREP,
    Entry {
        name: "egrep",
        ..GREP
    },
    Entry {
        name: "fgrep",
        ..GREP
    },
    RG,
    AG,
    Entry {
        name: "ls",
        flags: "ABCFGHLOPRSTUWabcdefghiklmnopqrstuvwx1@%,",
        valued: "Iw",
        long_flags: &[
            "all",
            "almost-all",
            "author",
            "escape",
            "directory",
            "classify",
            "file-type",
            "human-readable",
            "si",
            "dereference",
            "dereference-command-line",
            "numeric-uid-gid",
            "literal",
            "hide-control-chars",
            "show-control-chars",
            "quote-name",
            "reverse",
            "recursive",
            "size",
            "group-directories-first",
            "no-group",
            "inode",
            "kibibytes",
            "context",
            "full-time",
            "zero",
            "color",
            "colour",
            "hyperlink",
            "help",
            "version",
        ],
        long_valued: &[
            "block-size",
            "format",
            "hide",
            "ignore",
            "indicator-style",
            "quoting-style",
            "sort",
            "time",
            "time-style",
            "tabsize",
            "width",
        ],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "cat",
        flags: "AbeEnstTuvl",
        valued: "",
        long_flags: &[
            "show-all",
            "number-nonblank",
            "show-ends",
            "number",
            "squeeze-blank",
            "show-tabs",
            "show-nonprinting",
            "help",
            "version",
        ],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "head",
        flags: "qvz",
        valued: "nc",
        long_flags: &[
            "quiet",
            "silent",
            "verbose",
            "zero-terminated",
            "help",
            "version",
        ],
        long_valued: &["lines", "bytes"],
        risky: false,
        numeric: true,
        operands: None,
    },
    Entry {
        name: "tail",
        flags: "fFqvrz",
        valued: "ncsb",
        long_flags: &[
            "follow",
            "retry",
            "quiet",
            "silent",
            "verbose",
            "zero-terminated",
            "help",
            "version",
        ],
        long_valued: &[
            "lines",
            "bytes",
            "sleep-interval",
            "pid",
            "max-unchanged-stats",
        ],
        risky: false,
        numeric: true,
        operands: None,
    },
    Entry {
        name: "wc",
        flags: "clmwL",
        valued: "",
        long_flags: &[
            "bytes",
            "chars",
            "lines",
            "words",
            "max-line-length",
            "help",
            "version",
        ],
        long_valued: &["files0-from", "total"],
        risky: false,
        numeric: false,
        operands: None,
    },
    // `-o`/`--output` writes a file, `-T`/`--temporary-directory` writes
    // into a directory, and `--compress-program` runs a program.
    Entry {
        name: "sort",
        flags: "bdfgiMhnRrcCsuVz",
        valued: "ktS",
        long_flags: &[
            "ignore-leading-blanks",
            "dictionary-order",
            "ignore-case",
            "general-numeric-sort",
            "ignore-nonprinting",
            "month-sort",
            "human-numeric-sort",
            "numeric-sort",
            "random-sort",
            "reverse",
            "version-sort",
            "check",
            "merge",
            "stable",
            "unique",
            "zero-terminated",
            "debug",
            "help",
            "version",
        ],
        long_valued: &[
            "key",
            "field-separator",
            "buffer-size",
            "parallel",
            "sort",
            "batch-size",
        ],
        risky: true,
        numeric: false,
        operands: None,
    },
    // A second operand is a file `uniq` writes.
    Entry {
        name: "uniq",
        flags: "cdDiuz",
        valued: "fsw",
        long_flags: &[
            "count",
            "repeated",
            "all-repeated",
            "ignore-case",
            "unique",
            "zero-terminated",
            "group",
            "help",
            "version",
        ],
        long_valued: &["skip-fields", "skip-chars", "check-chars"],
        risky: false,
        numeric: false,
        operands: Some(1),
    },
    Entry {
        name: "cut",
        flags: "nsz",
        valued: "bcdf",
        long_flags: &[
            "complement",
            "only-delimited",
            "zero-terminated",
            "help",
            "version",
        ],
        long_valued: &[
            "bytes",
            "characters",
            "delimiter",
            "fields",
            "output-delimiter",
        ],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "tr",
        flags: "cCdst",
        valued: "",
        long_flags: &[
            "complement",
            "delete",
            "squeeze-repeats",
            "truncate-set1",
            "help",
            "version",
        ],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "echo",
        flags: "neE",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    // Read by `printf_runs_nothing`: Bash and zsh `-v` and a `%n`
    // conversion assign a variable, whose subscript the shell evaluates.
    Entry {
        name: "printf",
        flags: "",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: true,
        numeric: false,
        operands: None,
    },
    // zsh's `stat` builtin assigns with `-A` and `-H`, which are not here.
    Entry {
        name: "stat",
        flags: "LFlnqrstxe",
        valued: "cf",
        long_flags: &["dereference", "file-system", "terse", "help", "version"],
        long_valued: &["format", "printf"],
        risky: false,
        numeric: false,
        operands: None,
    },
    // `-z`/`-Z` run decompression programs and `-C` writes a compiled
    // magic file.
    Entry {
        name: "file",
        flags: "bcdEhiIkLlNnprsSv0",
        valued: "efFmMP",
        long_flags: &[
            "brief",
            "checking-printout",
            "debug",
            "no-dereference",
            "dereference",
            "mime",
            "mime-type",
            "mime-encoding",
            "keep-going",
            "list",
            "no-pad",
            "no-buffer",
            "preserve-date",
            "raw",
            "special-files",
            "no-sandbox",
            "print0",
            "extension",
            "apple",
            "help",
            "version",
        ],
        long_valued: &[
            "exclude",
            "exclude-quiet",
            "files-from",
            "separator",
            "magic-file",
            "parameter",
        ],
        risky: true,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "du",
        flags: "0aAbcDhHklLmPsSxgt",
        valued: "BdIt",
        long_flags: &[
            "null",
            "all",
            "apparent-size",
            "bytes",
            "total",
            "dereference-args",
            "human-readable",
            "inodes",
            "dereference",
            "count-links",
            "no-dereference",
            "separate-dirs",
            "si",
            "summarize",
            "one-file-system",
            "time",
            "help",
            "version",
        ],
        long_valued: &[
            "block-size",
            "max-depth",
            "threshold",
            "time-style",
            "exclude",
            "exclude-from",
            "files0-from",
        ],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "df",
        flags: "aghHiklmnPT",
        valued: "BtTx",
        long_flags: &[
            "all",
            "human-readable",
            "si",
            "inodes",
            "local",
            "no-sync",
            "sync",
            "portability",
            "print-type",
            "total",
            "help",
            "version",
        ],
        long_valued: &["block-size", "output", "type", "exclude-type"],
        risky: false,
        numeric: false,
        operands: None,
    },
    // Read by `test_runs_nothing`: `-v` and `-R` name a variable, and the
    // numeric comparisons evaluate their operands as arithmetic, whose
    // subscripts may run commands.
    Entry {
        name: "test",
        flags: "",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: true,
        numeric: false,
        operands: None,
    },
    // The directory builtins: where they move the shell is the directory
    // reader's question (`dir_changes`); they run nothing (replay of the
    // round 14 corpus: `cd ~ && cd "$p"`, `cd / && pushd "$p"`). zsh's
    // `chdir` is an unknown program to Bash, so it is not here.
    Entry {
        name: "cd",
        flags: "LPe@qs",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: true,
        operands: None,
    },
    Entry {
        name: "pushd",
        flags: "nqsLP",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: true,
        operands: None,
    },
    Entry {
        name: "popd",
        flags: "nq",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: true,
        operands: None,
    },
    // `git log` with its reading options (replay: `git log --since="$D"
    // --oneline`). `--output` writes a file and `--ext-diff` and
    // `--textconv` run configured programs, so they are not here; nor is
    // any global option before `log` but `--no-pager` (`-c` sets
    // configuration that runs programs).
    Entry {
        name: "git log",
        flags: "pusMCBDz",
        valued: "nSGL",
        long_flags: &[
            "oneline",
            "stat",
            "shortstat",
            "numstat",
            "name-only",
            "name-status",
            "patch",
            "no-patch",
            "graph",
            "decorate",
            "no-decorate",
            "all",
            "branches",
            "tags",
            "remotes",
            "reverse",
            "first-parent",
            "no-merges",
            "merges",
            "follow",
            "abbrev-commit",
            "no-abbrev-commit",
            "full-history",
            "simplify-merges",
            "ancestry-path",
            "left-right",
            "cherry-pick",
            "cherry-mark",
            "boundary",
            "date-order",
            "author-date-order",
            "topo-order",
            "no-color",
            "color",
            "no-ext-diff",
            "no-textconv",
            "summary",
            "raw",
            "parents",
            "children",
            "source",
            "regexp-ignore-case",
            "extended-regexp",
            "fixed-strings",
            "perl-regexp",
            "all-match",
            "invert-grep",
            "show-signature",
            "log-size",
            "relative-date",
            "full-diff",
            "no-renames",
            "find-renames",
            "find-copies",
            "ignore-all-space",
            "ignore-space-change",
            "word-diff",
            "unified",
            "no-walk",
            "do-walk",
        ],
        long_valued: &[
            "since",
            "until",
            "after",
            "before",
            "author",
            "committer",
            "grep",
            "max-count",
            "skip",
            "format",
            "pretty",
            "date",
            "abbrev",
            "diff-filter",
            "min-parents",
            "max-parents",
            "encoding",
            "stat-width",
        ],
        risky: true,
        numeric: true,
        operands: None,
    },
    Entry {
        name: "true",
        flags: "",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "false",
        flags: "",
        valued: "",
        long_flags: &[],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "basename",
        flags: "az",
        valued: "s",
        long_flags: &["multiple", "zero", "help", "version"],
        long_valued: &["suffix"],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "dirname",
        flags: "z",
        valued: "",
        long_flags: &["zero", "help", "version"],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "realpath",
        flags: "eLmPqsz",
        valued: "",
        long_flags: &[
            "canonicalize-existing",
            "canonicalize-missing",
            "logical",
            "physical",
            "quiet",
            "strip",
            "no-symlinks",
            "zero",
            "help",
            "version",
        ],
        long_valued: &["relative-to", "relative-base"],
        risky: false,
        numeric: false,
        operands: None,
    },
    Entry {
        name: "readlink",
        flags: "femnqsvz",
        valued: "",
        long_flags: &[
            "canonicalize",
            "canonicalize-existing",
            "canonicalize-missing",
            "no-newline",
            "quiet",
            "silent",
            "verbose",
            "zero",
            "help",
            "version",
        ],
        long_valued: &[],
        risky: false,
        numeric: false,
        operands: None,
    },
];

/// The allowlisted names, for tests and for the launcher check.
#[cfg(test)]
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    ENTRIES.iter().map(|entry| entry.name)
}

/// Whether the command `words` (its program first) runs nothing its words
/// name: a literal allowlisted program, by its bare name or a path in
/// `/bin` or `/usr/bin`, used with the options its entry lists.
/// `appended` says a launcher (`xargs`) adds arguments the text does not
/// show, which may be options.
pub(crate) fn runs_nothing(words: &[Token], appended: bool) -> bool {
    let Some((program, rest)) = words.split_first() else {
        return false;
    };
    if program.redirect {
        return false;
    }
    let args = arguments(rest);
    // `[` is `test` with a closing `]`.
    let (name, args) = if program.plain() && program.text == "[" {
        match args.split_last() {
            Some((close, inner)) if close.literal() && close.text == "]" => {
                ("test", inner.to_vec())
            }
            _ => return false,
        }
    } else if let Some(name) = program
        .literal()
        .then(|| system_name(&program.text))
        .flatten()
    {
        (name, args)
    } else {
        return false;
    };
    let (name, args) = match name {
        "git" => match git_subcommand(&args) {
            Some((sub, rest)) => (sub, rest),
            None => return false,
        },
        _ => (name, args),
    };
    let Some(entry) = ENTRIES.iter().find(|entry| entry.name == name) else {
        return false;
    };
    match name {
        "printf" => !appended && printf_runs_nothing(&args),
        "test" => !appended && test_runs_nothing(&args, false),
        _ => options_allowed(entry, &args, appended),
    }
}

/// The allowlist name of a git command, `git` and its subcommand, and the
/// words after the subcommand. Only `--no-pager` may come before it.
fn git_subcommand<'a>(args: &[&'a Token]) -> Option<(&'static str, Vec<&'a Token>)> {
    let at = args
        .iter()
        .position(|w| !(w.literal() && w.text == "--no-pager"))?;
    let sub = args[at];
    if !sub.literal() {
        return None;
    }
    let name = ENTRIES
        .iter()
        .map(|entry| entry.name)
        .find(|name| name.strip_prefix("git ") == Some(sub.text.as_str()))?;
    Some((name, args[at + 1..].to_vec()))
}

/// Whether a `[[ ... ]]` conditional, its words between the brackets,
/// evaluates nothing as code: no `-v` or `-R`, and no numeric comparison
/// of a value the text does not show.
pub(crate) fn conditional_runs_nothing(words: &[Token]) -> bool {
    let args = arguments(words);
    test_runs_nothing(&args, true)
}

/// The words after the program, without redirections and their targets.
fn arguments(rest: &[Token]) -> Vec<&Token> {
    let mut args = Vec::new();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word.redirect {
            words.next();
        } else {
            args.push(word);
        }
    }
    args
}

/// The allowlist name of a literal program word: its bare name, or the
/// base name of a path in a system directory, GNU prefix removed.
fn system_name(text: &str) -> Option<&str> {
    let bare = !text.contains('/')
        || ["/bin/", "/usr/bin/"].iter().any(|dir| {
            text.strip_prefix(dir)
                .is_some_and(|name| !name.contains('/'))
        });
    bare.then(|| program_name(text))
}

/// How an allowed option reads.
enum Takes {
    Nothing,
    NextWord,
}

/// Whether every option in `args` is one `entry` lists, and its operands
/// are within the entry's limits. Arguments a launcher appends (`appended`)
/// may be options unless a `--` ended them.
fn options_allowed(entry: &Entry, args: &[&Token], appended: bool) -> bool {
    let mut operands = 0;
    let mut ended = false;
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if ended || !word.text.starts_with('-') || word.text == "-" {
            if entry.risky && !ended && may_be_option(word) {
                return false;
            }
            if entry.operands.is_some() && !word.literal() {
                return false;
            }
            operands += 1;
            continue;
        }
        if word.text == "--" && word.literal() {
            ended = true;
            continue;
        }
        match option(entry, word) {
            Some(Takes::NextWord) => at += 1,
            Some(Takes::Nothing) => {}
            None => return false,
        }
    }
    if entry.risky && appended && !ended {
        return false;
    }
    entry
        .operands
        .is_none_or(|max| operands <= max && !appended)
}

/// How `word`, which starts with `-`, reads as one of `entry`'s options;
/// `None` for an option the entry does not list or whose name is not
/// literal. Long options must be written in full: getopt accepts any
/// unambiguous abbreviation, so a prefix of a listed name is refused.
fn option(entry: &Entry, word: &Token) -> Option<Takes> {
    let text = word.text.as_str();
    let literal_end = word.plain_prefix;
    if has_glob(&text[..literal_end]) || text[..literal_end].contains(['{', '~']) {
        return None;
    }
    if let Some(long) = text.strip_prefix("--") {
        let (name, value) = match long.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (long, None),
        };
        if literal_end < 2 + name.len() {
            return None;
        }
        if entry.long_flags.contains(&name) {
            return Some(Takes::Nothing);
        }
        if entry.long_valued.contains(&name) {
            return Some(if value.is_some() {
                Takes::Nothing
            } else {
                Takes::NextWord
            });
        }
        return None;
    }
    for (offset, c) in text.char_indices().skip(1) {
        if offset >= literal_end {
            return None;
        }
        if entry.valued.contains(c) {
            return Some(if offset + c.len_utf8() == text.len() {
                Takes::NextWord
            } else {
                Takes::Nothing
            });
        }
        if !(entry.flags.contains(c) || (entry.numeric && c.is_ascii_digit())) {
            return None;
        }
    }
    Some(Takes::Nothing)
}

/// Whether `word` may be an option once the shell expands it: it starts
/// with `-`, or its first character is not known text (an expansion, a
/// pattern, an ANSI-C quote).
fn may_be_option(word: &Token) -> bool {
    if word.text.starts_with('-') {
        return true;
    }
    if word.literal() {
        return false;
    }
    if word.ansi {
        return true;
    }
    !word
        .text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || "/._~+,:@%".contains(c))
}

/// Whether the shell passes `word` as text the guard knows: no expansion,
/// pattern or ANSI-C quote. Quoted text counts.
fn fixed(word: &Token) -> bool {
    !word.complex
        && word.params.is_empty()
        && word.tilde.is_none()
        && !word.ansi
        && !has_glob(&word.text)
        && !word.text.contains(['{', '~'])
        && !(word.plain_prefix > 0 && word.text.starts_with('='))
}

/// `printf FORMAT ARGS`: exempt when the format is known text, is not an
/// option (`-v` assigns a variable) and holds no `%n` conversion, which
/// assigns one, and no escape that may spell `%`.
fn printf_runs_nothing(args: &[&Token]) -> bool {
    let args = match args.first() {
        Some(first) if first.literal() && first.text == "--" => &args[1..],
        _ => args,
    };
    let Some(format) = args.first() else {
        return false;
    };
    fixed(format)
        && !format.text.starts_with('-')
        && !assigns(&format.text)
        && !escapes_a_number(&format.text)
}

/// Whether a printf format holds a `%n` conversion, with any flags,
/// width, precision or length between.
fn assigns(format: &str) -> bool {
    let chars: Vec<char> = format.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] != '%' {
            at += 1;
            continue;
        }
        at += 1;
        if chars.get(at) == Some(&'%') {
            at += 1;
            continue;
        }
        while chars
            .get(at)
            .is_some_and(|c| "-+ #0'123456789.*$hlLqjzt".contains(*c))
        {
            at += 1;
        }
        if chars.get(at) == Some(&'n') {
            return true;
        }
    }
    false
}

/// Whether a format holds a numeric escape (`\045`, `\x25`, `%`),
/// which may spell `%` in some implementation.
fn escapes_a_number(format: &str) -> bool {
    format.split('\\').skip(1).any(|after| {
        after.starts_with(|c: char| c.is_ascii_digit() || matches!(c, 'x' | 'u' | 'U'))
    })
}

const UNARY_TESTS: &[&str] = &[
    "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-L", "-n", "-O", "-G", "-N", "-p", "-r",
    "-s", "-S", "-t", "-u", "-w", "-x", "-z", "-o",
];

const BINARY_TESTS: &[&str] = &[
    "=", "==", "!=", "<", ">", "-nt", "-ot", "-ef", "-a", "-o", "=~",
];

/// Numeric comparisons: Bash's `test`, `[` and `[[`, and zsh's `test` and
/// `[`, evaluate their operands as arithmetic, so a subscript in a value
/// runs its command substitution.
const NUMERIC_TESTS: &[&str] = &["-eq", "-ne", "-lt", "-le", "-gt", "-ge"];

/// `test`, `[` and `[[` evaluate nothing as code when no word is `-v` or
/// `-R` (whose operand names a variable with a subscript the shell
/// evaluates), every value a numeric comparison reads is known text, and
/// no value the text does not show can become an operator. In `[[`,
/// operators are fixed when the shell parses the line.
fn test_runs_nothing(args: &[&Token], conditional: bool) -> bool {
    if args.iter().any(|w| matches!(w.text.as_str(), "-v" | "-R")) {
        return false;
    }
    if args
        .iter()
        .any(|w| NUMERIC_TESTS.contains(&w.text.as_str()))
        && !args.iter().all(|w| fixed(w))
    {
        return false;
    }
    if conditional {
        return true;
    }
    let is = |w: &Token, set: &[&str]| w.literal() && set.contains(&w.text.as_str());
    let bang = |w: &Token| w.literal() && w.text == "!";
    let binary = |w: &Token| is(w, BINARY_TESTS) || is(w, NUMERIC_TESTS);
    // POSIX reads up to four arguments by their count, so a value in an
    // operand position stays an operand whatever it expands to.
    let known = match args {
        [] | [_] => true,
        [op, _] => is(op, UNARY_TESTS) || bang(op),
        [first, op, _] => binary(op) || (bang(first) && is(op, UNARY_TESTS)),
        [not, _, op, _] => bang(not) && binary(op),
        _ => false,
    };
    known || args.iter().all(|w| w.literal() || !may_be_option(w))
}

#[cfg(test)]
#[path = "non_executing_tests.rs"]
mod tests;
