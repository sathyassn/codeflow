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
//! `rg`, `ag`, `grep`, `egrep` and `fgrep`, called directly or run by
//! `xargs`, GNU `parallel` or `find -exec`, `-execdir`, `-ok` or `-okdir`.
//! A direct `grep` is a reader whose words neither guard judges, and
//! `git grep` is a git read the startup check passes whole.
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
//! A pattern word is only left out of the judgment of what a command
//! writes. Its text still counts where a rule reads what the line names,
//! such as the closed rule of TSK-242: search output carries text that
//! matches the pattern, which a later command on the line may run.

use crate::hooks::git_guard::{basename, xargs_command};

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

/// The grammar of a search tool, by program name.
fn grammar(program: &str) -> Option<&'static Grammar> {
    match program {
        "rg" => Some(&RG),
        "ag" => Some(&AG),
        "grep" | "egrep" | "fgrep" => Some(&GREP),
        _ => None,
    }
}

/// The indices into `args` of the words `program` takes as search
/// patterns, or that a search tool it runs takes: the command of an
/// `xargs` or a GNU `parallel`, and each `find -exec`, `-execdir`, `-ok` or
/// `-okdir` command. Empty when no search tool this reader knows runs, or
/// when its call holds an option the reader does not know. `environment`
/// is every `NAME=value` the command line sets, read for
/// `RIPGREP_CONFIG_PATH`.
pub(crate) fn pattern_words(
    program: &str,
    args: &[String],
    environment: &[(String, String)],
) -> Vec<usize> {
    let run = |start: usize, command: &[String]| -> Vec<usize> {
        command
            .split_first()
            .map_or_else(Vec::new, |(program, rest)| {
                own_pattern_words(basename(program), rest, environment)
                    .into_iter()
                    .map(|at| start + 1 + at)
                    .collect()
            })
    };
    match program {
        "xargs" => xargs_command(args)
            .map_or_else(Vec::new, |command| run(args.len() - command.len(), command)),
        "parallel" => args
            .iter()
            .position(|arg| !arg.starts_with('-'))
            .map_or_else(Vec::new, |start| run(start, &args[start..])),
        "find" => find_commands(args)
            .into_iter()
            .flat_map(|(start, end)| run(start, &args[start..end]))
            .collect(),
        _ => own_pattern_words(program, args, environment),
    }
}

/// The words of `args` that are not [`pattern_words`], in order: what a
/// guard judges as paths the call may write.
pub(crate) fn other_words<'a>(
    program: &str,
    args: &'a [String],
    environment: &[(String, String)],
) -> Vec<&'a String> {
    let patterns = pattern_words(program, args, environment);
    args.iter()
        .enumerate()
        .filter(|(at, _)| !patterns.contains(at))
        .map(|(_, word)| word)
        .collect()
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

/// The pattern words of a search tool's own call.
fn own_pattern_words(
    program: &str,
    args: &[String],
    environment: &[(String, String)],
) -> Vec<usize> {
    let Some(grammar) = grammar(program) else {
        return Vec::new();
    };
    let Some(call) = read_call(grammar, args) else {
        return Vec::new();
    };
    let mut words = call.option_patterns;
    // ripgrep's configuration file may supply the pattern, which then
    // makes the first operand a searched path.
    let configured = program == "rg"
        && !call.no_config
        && environment
            .iter()
            .any(|(name, _)| name == "RIPGREP_CONFIG_PATH");
    if !call.supplied && !call.no_pattern && !configured {
        words.extend(call.first_operand);
    }
    words
}

/// What [`read_call`] finds in one call.
struct Call {
    /// The indices of the `-e` and `--regexp` values.
    option_patterns: Vec<usize>,
    /// The index of the first operand.
    first_operand: Option<usize>,
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
        first_operand: None,
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
            call.first_operand.get_or_insert(at);
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

    /// The pattern words of `line`, a program and its arguments.
    fn patterns(line: &str) -> Vec<String> {
        patterns_with(line, &[])
    }

    fn patterns_with(line: &str, environment: &[(String, String)]) -> Vec<String> {
        let all = words(line);
        let args = &all[1..];
        pattern_words(&all[0], args, environment)
            .into_iter()
            .map(|at| args[at].clone())
            .collect()
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
        assert_eq!(patterns("parallel rg /x$ ::: docs"), ["/x$"]);
        assert_eq!(
            patterns("find . -exec rg /x$ {} ; -execdir egrep -e /y {} +"),
            ["/x$", "/y"]
        );
        assert!(patterns("xargs sed /x$").is_empty());
        assert!(patterns("find . -name /x$ -delete").is_empty());
    }

    #[test]
    fn a_ripgrep_configuration_set_on_the_line_may_supply_the_pattern() {
        let set = [("RIPGREP_CONFIG_PATH".to_string(), "rc".to_string())];
        assert!(patterns_with("rg /x$ docs", &set).is_empty());
        assert_eq!(patterns_with("rg -e /x$ docs", &set), ["/x$"]);
        assert_eq!(patterns_with("rg --no-config /x$ docs", &set), ["/x$"]);
        assert_eq!(patterns_with("grep /x$ docs", &set), ["/x$"]);
    }
}
