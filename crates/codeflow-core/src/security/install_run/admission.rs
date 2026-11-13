//! The closed subset of POSIX shell lines the install check admits as data
//! (TSK-1476). A line that names the binary with an install verb is judged
//! on its raw text unless every part of it is in this subset, so this list
//! decides what an agent may say about the verbs, never what it may run.
//!
//! A line is admitted only when all of these hold:
//!
//! - It reads with this module's own lexer: words of plain characters,
//!   single-quoted text, double-quoted text with no `$`, backtick,
//!   backslash or `!`, the separators `;`, newline, `&&`, `||` and `|`, and
//!   redirections of a listed form to a file or `/dev/null`. Any other
//!   character or form (an expansion, a pattern, a substitution, a subshell
//!   or brace group, a here-document or here-string, the background `&`, a
//!   comment, a redirection to any other device) ends the reading, and the
//!   line is not admitted.
//! - Every simple command has a program word, written plainly, that is one
//!   of the arms of [`command_admitted`], with its arguments read by that
//!   program's own grammar. No assignment, declaration, alias, function,
//!   `eval`, `source`, `exec`, `builtin`, `command` or other wrapper, and no
//!   supplier such as `xargs` or `find -exec`, is listed, so each of those
//!   leaves the line out.
//! - Every argument is inert in bash, zsh and sh: it is printed, searched
//!   for or read as a file name, and never evaluated, run or used to reach
//!   the network. So `printf` takes only a format of literal text and `%s`
//!   (zsh evaluates the argument of a numeric conversion as arithmetic,
//!   which can run a substitution), `rg` takes `--no-config` as its first
//!   argument (so no option can take it as a value and no configuration
//!   file adds `--pre`), the binary is admitted only by its bare name, and
//!   no `git` call is listed (a search can fetch a missing object through a
//!   transport its configuration names).
//!
//! There is no denylist in this path: a program, option or form is
//! admitted only by being listed here.

use super::{BINARY, PRINTS_ONLY};
use crate::security::prose::string_format;

/// One word after quote removal, with whether any of it was quoted.
struct Word {
    text: String,
    quoted: bool,
}

enum Token {
    Word(Word),
    /// `|` between two commands.
    Pipe,
    /// `;`, newline, `&&` or `||` after a command.
    End,
    /// `>`, `>>` or `<`, with an optional `1` or `2` before it: the next
    /// word is its target.
    Redirect,
    /// `>&1` or `>&2`, with an optional `1` or `2` before it: no target.
    Duplicate,
}

/// The characters a word may hold unquoted: none the shell expands or
/// reads as syntax.
fn plain(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '.' | '/' | ':' | ',' | '+' | '@' | '-' | '=' | '%')
}

/// Whether `line` is in the admitted subset.
#[must_use]
pub(super) fn admitted(line: &str) -> bool {
    let Some(tokens) = lex(line) else {
        return false;
    };
    let mut command: Vec<Word> = Vec::new();
    // Whether the next word is a redirection's target.
    let mut target = false;
    // Whether the current command holds a redirection.
    let mut redirected = false;
    // The separator before the current command: `Some(true)` after `;`,
    // a newline, `&&` or `||`, `Some(false)` after `|`.
    let mut separator = None;
    for token in tokens {
        match token {
            Token::Word(word) => {
                if target {
                    if !target_admitted(&word.text) {
                        return false;
                    }
                    target = false;
                } else {
                    command.push(word);
                }
            }
            Token::Redirect | Token::Duplicate => {
                if target {
                    return false;
                }
                target = matches!(token, Token::Redirect);
                redirected = true;
            }
            Token::Pipe | Token::End => {
                if target || !command_admitted(&command) {
                    return false;
                }
                command.clear();
                redirected = false;
                separator = Some(matches!(token, Token::End));
            }
        }
    }
    if target {
        return false;
    }
    if command.is_empty() {
        // Only a trailing `;` or newline may leave the last command empty;
        // a redirection with no program runs the shell's own reader.
        return separator == Some(true) && !redirected;
    }
    command_admitted(&command)
}

/// Whether a redirection may open `target`: a file, or `/dev/null`. bash
/// opens a network connection for a target under `/dev/tcp/` or
/// `/dev/udp/`, so no other path with a `dev` component is admitted.
fn target_admitted(target: &str) -> bool {
    target == "/dev/null" || !target.split('/').any(|part| part == "dev")
}

/// The tokens of `line`, or `None` when it holds anything outside the
/// subset.
fn lex(line: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut in_word = false;
    let mut at = 0;
    let flush =
        |tokens: &mut Vec<Token>, word: &mut String, quoted: &mut bool, in_word: &mut bool| {
            if *in_word {
                tokens.push(Token::Word(Word {
                    text: std::mem::take(word),
                    quoted: *quoted,
                }));
            }
            *quoted = false;
            *in_word = false;
        };
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        match c {
            ' ' | '\t' => {
                flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
                at += 1;
            }
            '\n' | ';' => {
                if next == Some(';') && c == ';' {
                    return None;
                }
                flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
                tokens.push(Token::End);
                at += 1;
            }
            '&' => {
                if next != Some('&') {
                    return None;
                }
                flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
                tokens.push(Token::End);
                at += 2;
            }
            '|' => {
                flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
                match next {
                    Some('|') => {
                        tokens.push(Token::End);
                        at += 2;
                    }
                    Some('&') => return None,
                    _ => {
                        tokens.push(Token::Pipe);
                        at += 1;
                    }
                }
            }
            '>' | '<' => {
                // A `1` or `2` written right before the operator names the
                // descriptor, and any other number there names one outside
                // the subset; any other word before it ends there.
                if in_word && !quoted && word.bytes().all(|b| b.is_ascii_digit()) {
                    if word != "1" && word != "2" {
                        return None;
                    }
                    word.clear();
                    in_word = false;
                } else {
                    flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
                }
                let (token, length) = redirection(
                    c,
                    next,
                    chars.get(at + 2).copied(),
                    chars.get(at + 3).copied(),
                )?;
                tokens.push(token);
                at += length;
            }
            '\'' | '"' => {
                let (inside, length) = quote(&chars, at)?;
                word.extend(inside);
                quoted = true;
                in_word = true;
                at += length;
            }
            '=' if !in_word => return None,
            c if plain(c) => {
                word.push(c);
                in_word = true;
                at += 1;
            }
            _ => return None,
        }
    }
    flush(&mut tokens, &mut word, &mut quoted, &mut in_word);
    Some(tokens)
}

/// The text of the quote that opens at `at` and its length with both
/// quotes, or `None` when it is left open or a double quote holds a
/// character the shell reads there (`$`, a backtick, a backslash or `!`).
fn quote(chars: &[char], at: usize) -> Option<(&[char], usize)> {
    let mark = chars[at];
    let close = chars[at + 1..].iter().position(|&q| q == mark)?;
    let inside = &chars[at + 1..at + 1 + close];
    if mark == '"' && inside.iter().any(|&q| matches!(q, '$' | '`' | '\\' | '!')) {
        return None;
    }
    Some((inside, close + 2))
}

/// The redirection that starts with `c`, followed by `next`, `third` and
/// `fourth`, and its length: `>`, `>>`, `<`, or `>&1` and `>&2` ending
/// there. Any other form (`<<`, `<<<`, `<>`, `<&`, `>|`, `>&` to a file
/// or another descriptor, `<(` or `>(`) is outside the subset.
fn redirection(
    c: char,
    next: Option<char>,
    third: Option<char>,
    fourth: Option<char>,
) -> Option<(Token, usize)> {
    let ends = |after: Option<char>| {
        after.is_none_or(|after| matches!(after, ' ' | '\t' | '\n' | ';' | '|' | '&'))
    };
    match (c, next) {
        ('>', Some('>')) => Some((Token::Redirect, 2)),
        ('>', Some('&')) if matches!(third, Some('1' | '2')) && ends(fourth) => {
            Some((Token::Duplicate, 3))
        }
        ('>' | '<', Some('>' | '<' | '&' | '|' | '(')) => None,
        _ => Some((Token::Redirect, 1)),
    }
}

/// How a program's options are written: letters that take no value,
/// letters that take one (attached or the next word), long names of each
/// kind, long names whose value must be attached after `=`, and whether a
/// bare number (`-5`) is an option.
struct Options {
    flags: &'static str,
    values: &'static str,
    long_flags: &'static [&'static str],
    long_values: &'static [&'static str],
    attached: &'static [&'static str],
    numbers: bool,
}

// The listed programs are the arms of [`command_admitted`], each with its
// options below, read as [`operands`] reads them. `cat`, `head`, `tail`,
// `wc` and `grep` have no option, in their GNU or BSD form, that runs a
// program, opens a connection or evaluates an argument, so a word read
// with another arity is still inert; for `rg`, whose `--pre` runs a
// program, each listed option has the arity ripgrep 15.2.0 gives it. The
// sources are each program's manual, cited in the TSK-1476 report.

const CAT: Options = Options {
    flags: "AbeEnstTuvl",
    values: "",
    long_flags: &[
        "number",
        "number-nonblank",
        "squeeze-blank",
        "show-all",
        "show-ends",
        "show-tabs",
        "show-nonprinting",
    ],
    long_values: &[],
    attached: &[],
    numbers: false,
};

const HEAD: Options = Options {
    flags: "qv",
    values: "nc",
    long_flags: &["quiet", "silent", "verbose"],
    long_values: &["lines", "bytes"],
    attached: &[],
    numbers: true,
};

const TAIL: Options = Options {
    flags: "qvr",
    values: "nc",
    long_flags: &["quiet", "silent", "verbose"],
    long_values: &["lines", "bytes"],
    attached: &[],
    numbers: true,
};

const WC: Options = Options {
    flags: "clmwL",
    values: "",
    long_flags: &["bytes", "chars", "lines", "words", "max-line-length"],
    long_values: &[],
    attached: &[],
    numbers: false,
};

/// GNU grep and the BSD grep macOS ships. Left out: `-z`, `-Z`, `-J`,
/// `-X`, `-M` and the long decompress options, whose meaning differs
/// between the two.
const GREP: Options = Options {
    flags: "acEFGhHiIlLnoqrRsUvwxPV",
    values: "ABCDdefm",
    long_flags: &[
        "count",
        "extended-regexp",
        "fixed-strings",
        "basic-regexp",
        "perl-regexp",
        "no-filename",
        "with-filename",
        "ignore-case",
        "no-ignore-case",
        "files-with-matches",
        "files-without-match",
        "line-number",
        "only-matching",
        "quiet",
        "silent",
        "recursive",
        "dereference-recursive",
        "no-messages",
        "invert-match",
        "word-regexp",
        "line-regexp",
        "text",
        "binary",
        "null",
        "color",
        "colour",
    ],
    long_values: &[
        "after-context",
        "before-context",
        "context",
        "regexp",
        "file",
        "max-count",
        "include",
        "exclude",
        "exclude-dir",
        "include-dir",
        "label",
        "devices",
        "directories",
        "binary-files",
    ],
    attached: &["color", "colour"],
    numbers: false,
};

/// ripgrep after its first argument, `--no-config`: a short list whose
/// arity matches ripgrep 15.2.0's `rg --help`, so no word read here as a
/// value is an option to ripgrep and no option ripgrep reads is off this
/// list. Left out among others: `--pre`, `--pre-glob`, `-z` and
/// `--search-zip`, which run programs, and `--hostname-bin`, which runs
/// one.
const RG: Options = Options {
    flags: "iSsFwxnNlcvoHIuLaUq0.",
    values: "eftTgABCmd",
    long_flags: &[
        "no-config",
        "hidden",
        "no-ignore",
        "no-ignore-vcs",
        "no-ignore-parent",
        "ignore-case",
        "smart-case",
        "case-sensitive",
        "fixed-strings",
        "word-regexp",
        "line-regexp",
        "line-number",
        "no-line-number",
        "files-with-matches",
        "files-without-match",
        "count",
        "count-matches",
        "invert-match",
        "only-matching",
        "with-filename",
        "no-filename",
        "heading",
        "no-heading",
        "vimgrep",
        "json",
        "files",
        "follow",
        "multiline",
        "text",
        "null",
        "quiet",
        "column",
        "no-messages",
        "unrestricted",
    ],
    long_values: &[
        "glob",
        "iglob",
        "type",
        "type-not",
        "regexp",
        "file",
        "after-context",
        "before-context",
        "context",
        "max-count",
        "max-depth",
    ],
    attached: &[],
    numbers: false,
};

/// Whether one simple command is a listed program in an admitted form.
fn command_admitted(words: &[Word]) -> bool {
    let Some((program, args)) = words.split_first() else {
        return false;
    };
    if program.quoted || program.text.contains('=') {
        return false;
    }
    let args: Vec<&str> = args.iter().map(|word| word.text.as_str()).collect();
    match program.text.as_str() {
        // bash, zsh and sh `echo` print their words; an escape they decode
        // yields text, never a conversion or an evaluation.
        "echo" => true,
        "printf" => printf_admitted(&args),
        "cat" => operands(&CAT, &args).is_some(),
        "head" => operands(&HEAD, &args).is_some(),
        "tail" => operands(&TAIL, &args).is_some(),
        "wc" => operands(&WC, &args).is_some(),
        "grep" => operands(&GREP, &args).is_some(),
        "rg" => rg_admitted(&args),
        // The binary by its bare name only, in the exact help and version
        // shapes; a path can name any file an agent wrote there.
        BINARY => PRINTS_ONLY
            .iter()
            .any(|shape| shape.len() == args.len() && shape.iter().zip(&args).all(|(a, b)| a == b)),
        _ => false,
    }
}

/// `printf` with a format, after an optional `--`, that holds only literal
/// text, `%s`, `%%` and the escapes `\n`, `\t` and `\\`
/// ([`string_format`]), so bash, zsh and sh print every other argument as
/// a string. Left out: a leading option (`-v` writes a variable), any
/// numeric conversion (zsh evaluates its argument as arithmetic, and a
/// subscript there runs a substitution), `%n`, `%b`, a width, precision or
/// position, and every other escape (zsh decodes an octal or Unicode
/// escape to `%` before it reads conversions).
fn printf_admitted(args: &[&str]) -> bool {
    let format = match args {
        ["--", format, ..] => format,
        [format, ..] if !format.starts_with('-') => format,
        _ => return false,
    };
    string_format(format)
}

/// `rg` whose first argument is `--no-config`, so ripgrep reads it as that
/// flag (no option before it can take it as a value) and reads no
/// configuration file, with each later option on [`RG`].
fn rg_admitted(args: &[&str]) -> bool {
    match args.split_first() {
        Some((&"--no-config", rest)) => operands(&RG, rest).is_some(),
        _ => false,
    }
}

/// The operands of a call whose options are each on `options`, read as
/// getopt and ripgrep's parser read them: every word before `--` that
/// starts with `-` is an option, letters may be combined, and a letter or
/// long name that takes a value takes the rest of its word or the next
/// word, whatever that word holds. `None` when any option is not listed,
/// a long flag carries `=`, or a value is missing.
fn operands<'a>(options: &Options, args: &[&'a str]) -> Option<Vec<&'a str>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&word) = args.get(at) {
        at += 1;
        if word == "--" {
            found.extend_from_slice(&args[at..]);
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            match long.split_once('=') {
                Some((name, _)) => {
                    if !options.long_values.contains(&name) && !options.attached.contains(&name) {
                        return None;
                    }
                }
                None if options.long_flags.contains(&long) => {}
                None if options.long_values.contains(&long) => {
                    args.get(at)?;
                    at += 1;
                }
                None => return None,
            }
        } else if let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) {
            if options.numbers && letters.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            for (index, letter) in letters.char_indices() {
                if options.values.contains(letter) {
                    if index + letter.len_utf8() == letters.len() {
                        args.get(at)?;
                        at += 1;
                    }
                    break;
                }
                if !options.flags.contains(letter) {
                    return None;
                }
            }
        } else {
            found.push(word);
        }
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::admitted;

    #[test]
    fn the_listed_programs_are_admitted_in_their_listed_forms() {
        for line in [
            "grep -n 'codeflow update' notes.md",
            "grep -rn 'codeflow update' docs | head -n 5",
            "grep -e 'codeflow init' -A 2 notes.md",
            "rg --no-config 'codeflow init' docs",
            "rg --no-config -n -g '*.md' 'codeflow update' docs",
            "rg --no-config -e 'codeflow update' -i docs",
            "rg --no-config --regexp='codeflow init' -- docs",
            "echo 'run codeflow update'",
            "echo the operator runs codeflow update",
            "printf -- '%s\\n' 'codeflow update'",
            "printf '%s\\n' 'run codeflow update'",
            "printf '%s %s\\t100%%\\n' codeflow update",
            "printf '%s\\n' 'codeflow update' > /dev/null",
            "echo 'codeflow update' > /dev/null",
            "echo 'codeflow update' >> /dev/null",
            "grep -c 'codeflow init' notes.md 2>/dev/null",
            "grep -c 'codeflow init' notes.md 2> /dev/null | wc -l",
            "grep 'codeflow update' notes.md > /dev/null 2>&1",
            "grep 'codeflow update' notes.md 1>/dev/null 2>&1",
            "grep 'codeflow update' notes.md 2>/dev/null 1>&2",
            "grep -c 'codeflow update' < notes.md",
            "cat < notes.md; echo 'codeflow init'",
            "cat notes.md | grep -c 'codeflow update'",
            "tail -n 20 notes.md && echo 'codeflow init done'",
            "head -5 notes.md; echo \"codeflow update\"",
            "echo 'codeflow update'\n",
            "codeflow update --help",
            "codeflow help init",
        ] {
            assert!(admitted(line), "{line:?}");
        }
    }

    #[test]
    fn programs_options_and_forms_off_the_list_are_not_admitted() {
        for line in [
            // Programs not on the list.
            "ack 'codeflow update' docs",
            "gh search issues 'codeflow update'",
            "sed -n p 'codeflow update'",
            "jq '.x' plan.json",
            "/usr/bin/grep 'codeflow update' notes.md",
            "'grep' 'codeflow update' notes.md",
            "codeflow update",
            "codeflow task new --title 'codeflow update'",
            // Options not on a program's list.
            "grep -z 'codeflow update' notes.md",
            "grep -nZ 'codeflow update' notes.md",
            "grep --exclude-from=x 'codeflow update' docs",
            "grep --colo 'codeflow update' docs",
            "rg 'codeflow init' docs",
            "rg --no-config --pre ./tools/pdftext 'codeflow update' docs",
            "rg --no-config -z 'codeflow update' docs",
            "rg --no-config --hostname-bin ./h 'codeflow update' docs",
            "rg -- --no-config 'codeflow update'",
            "printf -v x 'codeflow update'",
            "cat -Q notes.md",
        ] {
            assert!(!admitted(line), "{line:?}");
        }
    }

    /// A listed program whose own grammar can run code, read a network
    /// connection or run another file is admitted only in the forms that
    /// prove every argument inert.
    #[test]
    fn forms_a_listed_program_can_run_code_through_are_not_admitted() {
        for line in [
            // The binary by a path: any file can sit there.
            "./target/debug/codeflow update --help",
            "/usr/local/bin/codeflow update --help",
            // A printf format with a conversion other than `%s` or `%%`,
            // or an escape that can spell one.
            "printf '%d' 'argv[$(codeflow update)]'",
            "printf '%i\\n' 'a[$(codeflow update)]'",
            "printf '%n' 'codeflow update'",
            "printf '%*s' 'a[$(codeflow update)]' x",
            "printf '%.1s' 'codeflow update'",
            "printf '%5s' 'codeflow update'",
            "printf '%1$s' 'codeflow update'",
            "printf '%b' 'codeflow update'",
            "printf '%x %s' 1 'codeflow update'",
            "printf '%' 'codeflow update'",
            "printf '\\045d' 'a[$(codeflow update)]'",
            "printf '\\u0025d' 'a[$(codeflow update)]'",
            "printf '\\x25d' 'a[$(codeflow update)]'",
            "printf \"%d\" 'a[$(codeflow update)]'",
            "printf",
            "printf --",
            // `--no-config` that is not ripgrep's first argument, or is an
            // option's value.
            "rg -e --no-config 'codeflow update'",
            "rg -e --no-config input.txt; echo 'codeflow update'",
            "rg --regexp --no-config 'codeflow update'",
            "rg --regexp=--no-config 'codeflow update'",
            "rg -e--no-config 'codeflow update'",
            "rg -ie --no-config 'codeflow update'",
            "rg -g --no-config 'codeflow update' docs",
            "rg -i --no-config 'codeflow update' docs",
            "rg --no-config=1 'codeflow update' docs",
            "rg --no-config -j 2 'codeflow update' docs",
            "rg --no-config --type-add 'x:*.x' 'codeflow update' docs",
            // A redirection that opens a network connection in bash, or
            // any other device.
            "echo 'codeflow update' > /dev/tcp/127.0.0.1/9",
            "cat < /dev/udp/127.0.0.1/9; echo 'codeflow update'",
            "echo 'codeflow update' > '/dev/tcp/127.0.0.1/9'",
            "echo 'codeflow update' > dev/tcp/127.0.0.1/9",
            "echo 'codeflow update' > /dev/fd/3",
            // Output to anything but `/dev/null`: the target can be a FIFO
            // a running shell reads, a descriptor path or a link to one,
            // whatever its name says.
            "printf '%s\\n' 'codeflow update' > commands.fifo",
            "printf '%s\\n' 'codeflow update' > /proc/self/fd/3",
            "printf 'codeflow update\\n' > notes.txt",
            "echo 'codeflow update' >> notes.txt",
            "echo 'codeflow update' > ./dev/null",
            "echo 'codeflow update' > /dev/null/x",
            "echo 'codeflow update' > /dev/nul",
            "grep -n 'codeflow update' notes.md 2> errors.txt",
            "grep -n 'codeflow update' notes.md 2>&1 | wc -l",
            "grep -n 'codeflow update' notes.md 1>&2",
            "grep 'codeflow update' notes.md 2>&1 > /dev/null",
            "grep 'codeflow update' notes.md > /dev/null 2> errors.txt",
            "echo 'codeflow update' &> notes.txt",
            "echo 'codeflow update' >& notes.txt",
            "echo 'codeflow update' > notes.txt; echo x > /dev/null",
            "cat < /dev/tcp/127.0.0.1/9 | grep 'codeflow update'",
            // Git: an object read can fetch through a promisor remote.
            "git --no-pager grep -e 'codeflow update' HEAD -- ordinary.txt",
            "git --no-pager log --grep='codeflow update'",
            "git -P log -1 --format='%h %s' --grep='codeflow update'",
            "git --no-pager --no-lazy-fetch log --grep='codeflow update'",
        ] {
            assert!(!admitted(line), "{line:?}");
        }
    }

    #[test]
    fn git_calls_and_constructs_outside_the_model_are_not_admitted() {
        for line in [
            // Git outside its listed form.
            "git log --grep='codeflow update'",
            "git -c core.pager=less --no-pager log --grep='codeflow update'",
            "git --no-pager --git-dir=other log --grep='codeflow update'",
            "git --no-pager log -p --grep='codeflow update'",
            "git --no-pager log --show-signature --grep='codeflow update'",
            "git --no-pager log --format='%G? codeflow update'",
            "git --no-pager log --format='%(describe) codeflow update'",
            "git --no-pager log --pretty=custom --grep='codeflow update'",
            "git --no-pager log --format 'x' --grep='codeflow update'",
            "git --no-pager grep -O 'codeflow update'",
            "git --no-pager grep --textconv 'codeflow update'",
            "git --no-pager show HEAD",
            "git --no-pager diff -- 'codeflow update'",
            // Constructs outside the model.
            "X=1 grep 'codeflow update' notes.md",
            "X=1; grep 'codeflow update' notes.md",
            "export X=1; grep 'codeflow update' notes.md",
            "alias grep=sh; grep 'codeflow update' notes.md",
            "grep() { sh; }; grep 'codeflow update' notes.md",
            "eval \"echo 'codeflow update'\"",
            "source ./x; echo 'codeflow update'",
            ". ./x; echo 'codeflow update'",
            "exec echo 'codeflow update'",
            "builtin echo 'codeflow update'",
            "command echo 'codeflow update'",
            "echo \"$(cat f) codeflow update\"",
            "echo `cat f` codeflow update",
            "echo $HOME codeflow update",
            "cat <(echo 'codeflow update')",
            "(echo 'codeflow update')",
            "{ echo 'codeflow update'; }",
            "cat <<'EOF'\ncodeflow update\nEOF",
            "cat <<< 'codeflow update'",
            "echo 'codeflow update' > \"$F\"",
            "echo 'codeflow update' >| notes.txt",
            "echo 'codeflow update' 3> notes.txt",
            "echo 'codeflow update' >&3",
            "echo 'codeflow update' <> notes.txt",
            "< notes.md",
            "grep 'codeflow update' *.md",
            "grep 'codeflow update' ~/notes.md",
            "grep 'codeflow update' docs/{a,b}.md",
            "echo =grep codeflow update",
            "echo notes.md | xargs grep 'codeflow update'",
            "find . -exec grep 'codeflow update' {} +",
            "echo 'codeflow update' & wait",
            "echo 'codeflow update' | sh",
            "echo 'codeflow update' |& cat",
            "echo ok # codeflow update",
            "echo \\codeflow update",
            "echo \"codeflow update\\\"",
            "echo 'codeflow update",
            "; echo 'codeflow update'",
            "echo 'codeflow update' |",
            "echo 'codeflow update' ;; echo x",
            "",
        ] {
            assert!(!admitted(line), "{line:?}");
        }
    }
}
