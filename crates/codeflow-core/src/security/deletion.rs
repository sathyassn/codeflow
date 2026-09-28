//! Deletions composed from more than one word (TSK-141).
//!
//! The catastrophic floor in [`super::dangerous`] reads each `rm` on its own.
//! The same deletion reaches a protected target other ways: `find … -delete`
//! or a remover `find` runs, `xargs` or `parallel` fed a protected listing,
//! `rsync --delete` into a protected destination, a relative path after `cd`,
//! a variable, a home expression such as `${HOME:?}` or `~user`, a link to a
//! protected directory, or any of those inside a subshell, group, function
//! or control structure. This module reads the line as a small shell program
//! and judges every deletion in it by the target it reaches, with the
//! classification the `rm` check uses.
//!
//! **Sound for protected targets.** For every variable, positional
//! parameter and the working directory the reader keeps the set of values
//! it may hold (its feasible values). A subshell, a pipeline stage, `sh -c`
//! and a substitution run on a copy that never flows back. A branch, an
//! `&&` or `||` operand, a `case` arm and a loop body may or may not run, so
//! the state after them is the union of every path through them. A
//! `break`, `continue` or `return` leaves with the state it holds there,
//! that state joins the union too, and the commands after it in the same
//! list are not reached; nor is anything after `exit`. A function call
//! runs its body with the call's arguments and prefix assignments, and its
//! `local` names and arguments take back their values on return; an alias
//! is read both expanded and as written; `command`, `builtin` and `time`
//! before a builtin still run it in this shell. Only an assignment, `cd` or
//! `unset` that runs unconditionally in the same shell narrows a value. A
//! deletion is refused when any feasible value of its target is protected,
//! and the refusal says so when the value is one of several.
//!
//! **Taint: what the reader does not model exactly is unknown.** A
//! construct the reader does not model exactly makes every value it may
//! change unknown instead of guessing it. A sourced file, a name reference
//! (`declare -n`) or a command whose name the reader cannot resolve makes
//! every variable and the working directory unknown for the rest of the
//! line (a literal `cd` gives a known directory again); `eval` of text the
//! reader cannot resolve, a function call or nested script deeper than it
//! follows, and a loop it cannot settle do the same for what they may
//! change. A `read` with an option the reader does not model or from input
//! it cannot see, `mapfile` from such input, `getopts`, a case-converting
//! attribute (`declare -u`), a field split on an `IFS` it cannot resolve, a
//! parameter expansion it does not resolve (`${V/x/y}`, `${!V}`, substrings)
//! and a glob it cannot list on disk each give an unknown value. A
//! recursive deletion whose operand holds an unknown value, or whose
//! relative operand runs in an unknown working directory, is refused as
//! unproven: the refusal names the construct and asks for a literal project
//! path. A literal path in a known state is judged as before.
//!
//! **Precise where it is cheap.** Single quotes keep `$VAR` literal; a `for`
//! over a fixed list leaves its variable at the last word when the body
//! neither leaves early nor assigns it; `${VAR:?}`, `${VAR:-…}`,
//! `${VAR:=…}` (which also assigns) and `${VAR%…}` expand to their possible
//! values; an array holds every element it may have; `set --`, `shift`,
//! `read` and `printf -v` set what they set; words split on the `IFS` the
//! line sets; brace expansion reaches across expansions (`{build,$D}`) and
//! ranges (`{a..z}`); a substitution whose last command is `echo`, `printf`
//! or `pwd` is read after the commands before it; a glob is matched against
//! the disk and against the protected names; and a path that exists is also
//! judged where it really lands (the real path of its longest existing
//! prefix), so a link to `/` is `/`.
//!
//! **Residual (ADR-0009).** A value from the environment or the output of
//! a command the reader does not run (`$(git rev-parse …)`) is judged by
//! its spelling and passes; `read` is the exception above, since the
//! builtin exists to take such a value. The deletions inside a sourced
//! file, a program in another language, or a function a sourced file
//! defines are not seen.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::dangerous::{
    dangerous_rm_target, normalize_path, program_name, protected_names, recursive_rm_operands,
};

/// A protected deletion found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    /// The protected target, as the `rm` check names it.
    pub target: &'static str,
    /// Set when the target is one of several values the line may reach
    /// there, so the refusal can say why a line that may not reach it is
    /// refused.
    pub ambiguous: bool,
    /// Set when the target cannot be proven: the operand or the working
    /// directory depends on a construct the reader does not model exactly.
    pub unproven: Option<Unproven>,
}

/// Why a deletion's target cannot be proven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unproven {
    /// The construct the target depends on, as the refusal names it.
    pub reason: String,
    /// Set when the working directory, not the operand, depends on it.
    pub cwd: bool,
}

/// The protected deletion `command` performs, read from the process's
/// working directory.
pub(super) fn composed_deletion(command: &str) -> Option<Found> {
    let base = std::env::current_dir().ok();
    composed_deletion_in(command, base.as_deref())
}

/// The protected deletion `command` performs when it starts in `base`.
pub(super) fn composed_deletion_in(command: &str, base: Option<&Path>) -> Option<Found> {
    let mut reader = Reader {
        base,
        found: None,
        depth: 0,
        pipe_input: None,
        jumps: Vec::new(),
        expanding: Vec::new(),
        traps: Vec::new(),
    };
    let mut state = State::start();
    reader.script(command, &mut state);
    // A trap's action runs when the line ends, in the state it ends in.
    state.dead = false;
    for action in std::mem::take(&mut reader.traps) {
        let mut end = state.clone();
        reader.child_script(&action, &mut end);
    }
    reader.found
}

/// Values kept per variable and for the working directory; past this, the
/// values that are already protected are kept first.
const MAX_VALUES: usize = 64;
/// Concrete argument vectors judged per simple command.
const MAX_VARIANTS: usize = 64;
/// Nested scripts (`sh -c`, `eval`, substitutions) and function calls.
const MAX_DEPTH: usize = 6;
/// Passes over a loop body before its state is taken as settled.
const LOOP_PASSES: usize = 6;

// ---------------------------------------------------------------------------
// Words and tokens
// ---------------------------------------------------------------------------

/// One piece of a shell word.
#[derive(Debug, Clone, PartialEq)]
enum Part {
    /// Literal text, its quoting removed.
    Lit { text: String, quoted: bool },
    /// An unquoted leading `~` or `~user` (`+` and `-` name `$PWD` and
    /// `$OLDPWD`).
    Tilde(String),
    /// `$NAME` or `${NAME…}`.
    Param {
        name: String,
        op: Option<ParamOp>,
        quoted: bool,
    },
    /// `$(…)`, a backtick or a process substitution.
    Subst { script: String, quoted: bool },
    /// The elements of an array value, `NAME=( … )`.
    Array(Vec<Word>),
    /// Text the reader does not evaluate: kept as spelled when it cannot
    /// name a path (`$$`, `$?`, `$((…))`, `${#V}`), or unknown for the
    /// reason given.
    Opaque {
        text: String,
        quoted: bool,
        unknown: Option<&'static str>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum ParamOp {
    /// `:-` and `-`, or with `assign` `:=` and `=`: the word when the value
    /// is empty or unset, which `assign` also stores in the variable.
    Default { word: Vec<Part>, assign: bool },
    /// `:?` and `?`: the value, or the shell stops.
    Required,
    /// `:+` and `+`: the word when the value is set.
    Alternate(Vec<Part>),
    /// `%`, `%%`, `#` and `##` with a pattern.
    Strip {
        suffix: bool,
        longest: bool,
        pattern: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Word {
    parts: Vec<Part>,
    /// The word as written, for an alias expansion to re-read.
    src: String,
}

/// `NAME=value` and its forms: `NAME+=value` appends, and `NAME[i]=value`
/// sets one element of an array.
#[derive(Debug, Clone, PartialEq)]
struct Assign {
    name: String,
    append: bool,
    element: bool,
    value: Vec<Part>,
}

impl Word {
    /// The text of a word that is one unquoted literal, such as a reserved
    /// word or an operator-like argument.
    fn plain(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [Part::Lit {
                text,
                quoted: false,
            }] => Some(text),
            _ => None,
        }
    }

    fn push_lit(&mut self, ch: char, quoted: bool) {
        if let Some(Part::Lit { text, quoted: q }) = self.parts.last_mut() {
            if *q == quoted {
                text.push(ch);
                return;
            }
        }
        self.parts.push(Part::Lit {
            text: ch.to_string(),
            quoted,
        });
    }

    /// The assignment this word makes, when it starts with `NAME=`,
    /// `NAME+=` or `NAME[subscript]=`. A subscript may hold expansions, so
    /// its closing `]` can be in a later part.
    fn assignment(&self) -> Option<Assign> {
        let Some(Part::Lit {
            text,
            quoted: false,
        }) = self.parts.first()
        else {
            return None;
        };
        let name_len = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(text.len());
        let name = &text[..name_len];
        if !is_name(name) {
            return None;
        }
        let rest = &text[name_len..];
        let (element, index, after) = if let Some(subscript) = rest.strip_prefix('[') {
            let found = match subscript.find(']') {
                Some(end) => Some((0, &subscript[end + 1..])),
                None => self.parts.iter().enumerate().skip(1).find_map(|(k, part)| {
                    let Part::Lit {
                        text,
                        quoted: false,
                    } = part
                    else {
                        return None;
                    };
                    text.find(']').map(|end| (k, &text[end + 1..]))
                }),
            };
            let (index, after) = found?;
            (true, index, after)
        } else {
            (false, 0, rest)
        };
        let (append, value) = if let Some(value) = after.strip_prefix("+=") {
            (true, value)
        } else if let Some(value) = after.strip_prefix('=') {
            (false, value)
        } else {
            return None;
        };
        let mut parts = Vec::new();
        if !value.is_empty() {
            parts.push(Part::Lit {
                text: value.to_string(),
                quoted: false,
            });
        }
        parts.extend(self.parts[index + 1..].iter().cloned());
        Some(Assign {
            name: name.to_string(),
            append,
            element,
            value: parts,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Redir {
    op: String,
    target: Option<Word>,
    /// A heredoc's body.
    body: Option<String>,
    /// A heredoc whose delimiter is unquoted: its body is expanded.
    expand: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(Word),
    Op(&'static str),
    Redir(Redir),
}

/// A heredoc waiting for its body after the end of its line.
struct Pending {
    at: usize,
    delimiter: String,
    strip_tabs: bool,
    expand: bool,
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    /// Where the word being read started.
    start: usize,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            at: 0,
            start: 0,
        }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Split a script into words, operators and redirections.
    fn tokens(mut self) -> Vec<Tok> {
        let mut toks = Vec::new();
        let mut word: Option<Word> = None;
        let mut pending: Vec<Pending> = Vec::new();
        while let Some(ch) = self.peek(0) {
            match ch {
                ' ' | '\t' | '\r' => {
                    self.flush(&mut toks, &mut word);
                    self.at += 1;
                }
                '\\' if self.peek(1) == Some('\n') => self.at += 2,
                '\n' => {
                    self.flush(&mut toks, &mut word);
                    toks.push(Tok::Op("\n"));
                    self.at += 1;
                    for doc in pending.drain(..) {
                        let body = self.heredoc_body(&doc.delimiter, doc.strip_tabs);
                        if let Some(Tok::Redir(redir)) = toks.get_mut(doc.at) {
                            redir.body = Some(body);
                            redir.expand = doc.expand;
                        }
                    }
                }
                '#' if word.is_none() => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                ';' | '&' | '|' => {
                    self.flush(&mut toks, &mut word);
                    self.operator(ch, &mut toks);
                }
                '(' => self.paren(&mut toks, &mut word),
                ')' => {
                    self.flush(&mut toks, &mut word);
                    toks.push(Tok::Op(")"));
                    self.at += 1;
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    // A process substitution runs its command.
                    self.begin(&mut word);
                    let (inner, next) = balanced(&self.chars, self.at + 1, '(', ')');
                    self.at = next;
                    self.begin(&mut word).parts.push(Part::Subst {
                        script: inner,
                        quoted: false,
                    });
                }
                '<' | '>' => {
                    // A leading file descriptor number belongs to the operator.
                    if word
                        .as_ref()
                        .and_then(Word::plain)
                        .is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()))
                    {
                        word = None;
                    }
                    self.flush(&mut toks, &mut word);
                    self.redirection(&mut toks, &mut pending);
                }
                _ => {
                    self.begin(&mut word);
                    if let Some(w) = word.as_mut() {
                        self.word_char(w, true);
                    }
                }
            }
        }
        self.flush(&mut toks, &mut word);
        toks
    }

    /// Start a word at the cursor, unless one is being read.
    fn begin<'w>(&mut self, word: &'w mut Option<Word>) -> &'w mut Word {
        if word.is_none() {
            self.start = self.at;
        }
        word.get_or_insert_with(Word::default)
    }

    /// End the word being read, keeping its text as written.
    fn flush(&self, toks: &mut Vec<Tok>, word: &mut Option<Word>) {
        if let Some(mut w) = word.take() {
            let end = self.at.min(self.chars.len());
            w.src = self.chars[self.start.min(end)..end].iter().collect();
            toks.push(Tok::Word(w));
        }
    }

    /// `;`, `;;`, `&`, `&&`, `&>`, `|`, `||` and `|&` at the cursor.
    fn operator(&mut self, ch: char, toks: &mut Vec<Tok>) {
        let op = match (ch, self.peek(1), self.peek(2)) {
            (';', Some(';'), Some('&')) => ";;&",
            (';', Some(';'), _) => ";;",
            (';', Some('&'), _) => ";&",
            (';', ..) => ";",
            ('&', Some('>'), Some('>')) => "&>>",
            ('&', Some('>'), _) => "&>",
            ('&', Some('&'), _) => "&&",
            ('&', ..) => "&",
            ('|', Some('|'), _) => "||",
            ('|', Some('&'), _) => "|&",
            _ => "|",
        };
        self.at += op.len();
        match op {
            "&>" | "&>>" => {
                let target = self.redirect_target();
                toks.push(Tok::Redir(Redir {
                    op: op.to_string(),
                    target,
                    body: None,
                    expand: false,
                }));
            }
            "|&" => toks.push(Tok::Op("|")),
            _ => toks.push(Tok::Op(op)),
        }
    }

    /// `(`: an array value after `NAME=`, an arithmetic command, or an
    /// operator.
    fn paren(&mut self, toks: &mut Vec<Tok>, word: &mut Option<Word>) {
        let array = word.as_ref().is_some_and(|w| {
            matches!(w.parts.last(), Some(Part::Lit { text, quoted: false }) if text.ends_with('='))
        });
        if array {
            let (inner, next) = balanced(&self.chars, self.at, '(', ')');
            self.at = next;
            let elements = Lexer::new(&inner)
                .tokens()
                .into_iter()
                .filter_map(|tok| match tok {
                    Tok::Word(w) => Some(w),
                    _ => None,
                })
                .collect();
            self.begin(word).parts.push(Part::Array(elements));
            return;
        }
        if word.is_none() && self.peek(1) == Some('(') {
            // `(( … ))` arithmetic gives a number; a substitution in it runs.
            self.begin(word);
            let (inner, next) = balanced(&self.chars, self.at, '(', ')');
            self.at = next;
            if let Some(w) = word.as_mut() {
                w.parts.push(Part::Opaque {
                    text: format!("({inner})"),
                    quoted: false,
                    unknown: None,
                });
            }
            self.flush(toks, word);
            return;
        }
        self.flush(toks, word);
        toks.push(Tok::Op("("));
        self.at += 1;
    }

    /// Read one redirection operator and its target.
    fn redirection(&mut self, toks: &mut Vec<Tok>, pending: &mut Vec<Pending>) {
        const OPS: &[&str] = &["<<<", "<<-", "<<", "<>", "<&", ">>", ">&", ">|", "<", ">"];
        let rest: String = self.chars[self.at..].iter().take(3).collect();
        let op = OPS
            .iter()
            .find(|op| rest.starts_with(**op))
            .copied()
            .unwrap_or("<");
        self.at += op.len();
        if op == "<<" || op == "<<-" {
            let target = self.redirect_target();
            // A quoted delimiter keeps the body literal.
            let expand = target.as_ref().is_some_and(|w| {
                w.parts
                    .iter()
                    .all(|p| matches!(p, Part::Lit { quoted: false, .. }))
            });
            let delimiter = target.map(|w| word_text(&w)).unwrap_or_default();
            pending.push(Pending {
                at: toks.len(),
                delimiter,
                strip_tabs: op == "<<-",
                expand,
            });
            toks.push(Tok::Redir(Redir {
                op: op.to_string(),
                target: None,
                body: None,
                expand,
            }));
            return;
        }
        let target = self.redirect_target();
        toks.push(Tok::Redir(Redir {
            op: op.to_string(),
            target,
            body: None,
            expand: false,
        }));
    }

    fn redirect_target(&mut self) -> Option<Word> {
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.at += 1;
        }
        let mut word = Word::default();
        while let Some(ch) = self.peek(0) {
            if ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')' | '<' | '>') {
                break;
            }
            self.word_char(&mut word, true);
        }
        (!word.parts.is_empty()).then_some(word)
    }

    /// The lines after the current one, up to the delimiter.
    fn heredoc_body(&mut self, delimiter: &str, strip_tabs: bool) -> String {
        let mut body = String::new();
        while self.at < self.chars.len() {
            let end = self.chars[self.at..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(self.chars.len(), |p| self.at + p);
            let line: String = self.chars[self.at..end].iter().collect();
            self.at = (end + 1).min(self.chars.len());
            let line = if strip_tabs {
                line.trim_start_matches('\t').to_string()
            } else {
                line
            };
            if line == delimiter {
                break;
            }
            body.push_str(&line);
            body.push('\n');
        }
        body
    }

    /// Consume one character (or one quoted or expanded unit) of a word.
    /// `split` is false inside `${…}` arguments, where blanks are text.
    fn word_char(&mut self, word: &mut Word, split: bool) {
        let Some(ch) = self.peek(0) else {
            return;
        };
        match ch {
            '\\' => {
                if let Some(next) = self.peek(1) {
                    word.push_lit(next, true);
                    self.at += 2;
                } else {
                    self.at += 1;
                }
            }
            '\'' => {
                self.at += 1;
                let mut text = String::new();
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    if c == '\'' {
                        break;
                    }
                    text.push(c);
                }
                word.parts.push(Part::Lit { text, quoted: true });
            }
            '"' => {
                self.at += 1;
                self.double_quoted(word);
            }
            '$' => self.dollar(word, false),
            '`' => {
                let script = self.backtick();
                word.parts.push(Part::Subst {
                    script,
                    quoted: false,
                });
            }
            '~' if split && Self::tilde_allowed(word) => {
                self.at += 1;
                let mut user = String::new();
                while let Some(c) = self.peek(0) {
                    if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+') {
                        user.push(c);
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                word.parts.push(Part::Tilde(user));
            }
            _ => {
                word.push_lit(ch, false);
                self.at += 1;
            }
        }
    }

    /// A tilde expands at the start of a word, and after `=` or `:` in an
    /// assignment.
    fn tilde_allowed(word: &Word) -> bool {
        match word.parts.as_slice() {
            [] => true,
            [Part::Lit {
                text,
                quoted: false,
            }] => text.split_once('=').is_some_and(|(name, rest)| {
                is_name(name) && (rest.is_empty() || rest.ends_with(':'))
            }),
            _ => false,
        }
    }

    fn double_quoted(&mut self, word: &mut Word) {
        // An empty "" still makes a word.
        word.parts.push(Part::Lit {
            text: String::new(),
            quoted: true,
        });
        while let Some(ch) = self.peek(0) {
            match ch {
                '"' => {
                    self.at += 1;
                    return;
                }
                '\\' if matches!(self.peek(1), Some('$' | '`' | '"' | '\\' | '\n')) => {
                    if let Some(next) = self.peek(1) {
                        if next != '\n' {
                            word.push_lit(next, true);
                        }
                    }
                    self.at += 2;
                }
                '$' => self.dollar(word, true),
                '`' => {
                    let script = self.backtick();
                    word.parts.push(Part::Subst {
                        script,
                        quoted: true,
                    });
                }
                _ => {
                    word.push_lit(ch, true);
                    self.at += 1;
                }
            }
        }
    }

    fn backtick(&mut self) -> String {
        self.at += 1;
        let mut script = String::new();
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '`' => break,
                '\\' if matches!(self.peek(0), Some('`' | '\\' | '$')) => {
                    if let Some(next) = self.peek(0) {
                        script.push(next);
                    }
                    self.at += 1;
                }
                _ => script.push(c),
            }
        }
        script
    }

    /// `$…` at the cursor.
    fn dollar(&mut self, word: &mut Word, quoted: bool) {
        match self.peek(1) {
            Some('(') if self.peek(2) == Some('(') => {
                let (inner, next) = balanced(&self.chars, self.at + 1, '(', ')');
                self.at = next;
                word.parts.push(Part::Opaque {
                    text: format!("$({inner})"),
                    quoted,
                    unknown: None,
                });
            }
            Some('(') => {
                let (script, next) = balanced(&self.chars, self.at + 1, '(', ')');
                self.at = next;
                word.parts.push(Part::Subst { script, quoted });
            }
            Some('{') => {
                let (inner, next) = balanced(&self.chars, self.at + 1, '{', '}');
                self.at = next;
                word.parts.push(parameter(&inner, quoted));
            }
            Some('\'') if !quoted => {
                self.at += 2;
                let mut text = String::new();
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    match c {
                        '\'' => break,
                        '\\' => {
                            let escaped = self.peek(0).unwrap_or('\\');
                            self.at += 1;
                            text.push(match escaped {
                                'n' => '\n',
                                't' => '\t',
                                '0' => '\0',
                                other => other,
                            });
                        }
                        _ => text.push(c),
                    }
                }
                word.parts.push(Part::Lit { text, quoted: true });
            }
            Some('"') if !quoted => {
                self.at += 2;
                self.double_quoted(word);
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                self.at += 1;
                let mut name = String::new();
                while let Some(c) = self.peek(0) {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                word.parts.push(Part::Param {
                    name,
                    op: None,
                    quoted,
                });
            }
            // A positional parameter, or `"$@"` and `"$*"`.
            Some(c) if c.is_ascii_digit() || c == '@' || c == '*' => {
                self.at += 2;
                word.parts.push(Part::Param {
                    name: c.to_string(),
                    op: None,
                    quoted,
                });
            }
            // `$#`, `$?`, `$$`, `$!` and `$-` never name a path.
            Some(c) if "#?$!-".contains(c) => {
                self.at += 2;
                word.parts.push(Part::Opaque {
                    text: format!("${c}"),
                    quoted,
                    unknown: None,
                });
            }
            _ => {
                word.push_lit('$', quoted);
                self.at += 1;
            }
        }
    }
}

/// The text between `chars[open_at]` (the opener) and its matching closer,
/// honouring quotes and escapes, and the index after the closer.
fn balanced(chars: &[char], open_at: usize, open: char, close: char) -> (String, usize) {
    let mut depth = 0usize;
    let mut i = open_at;
    let mut inner = String::new();
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' && q == '"' && i + 1 < chars.len() {
                inner.push(c);
                inner.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            inner.push(c);
            i += 1;
            continue;
        }
        match c {
            '\\' if i + 1 < chars.len() => {
                inner.push(c);
                inner.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '\'' | '"' => quote = Some(c),
            _ if c == open => {
                depth += 1;
                if depth == 1 {
                    i += 1;
                    continue;
                }
            }
            _ if c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (inner, i + 1);
                }
            }
            _ => {}
        }
        inner.push(c);
        i += 1;
    }
    (inner, i)
}

/// The inside of `${…}`.
fn parameter(inner: &str, quoted: bool) -> Part {
    let opaque = |unknown: Option<&'static str>| Part::Opaque {
        text: format!("${{{inner}}}"),
        quoted,
        unknown,
    };
    // `${#V}` is a length; `${!V}` and its forms name another variable.
    if inner.starts_with('#') {
        return opaque(None);
    }
    if inner.starts_with('!') {
        return opaque(Some(why::PARAM));
    }
    let name_len = if inner.starts_with(['@', '*']) {
        1
    } else {
        inner
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(inner.len())
    };
    let (name, mut rest) = inner.split_at(name_len);
    let positional = !name.is_empty() && name.chars().all(|c| c.is_ascii_digit());
    if !(is_name(name) || positional || matches!(name, "@" | "*")) {
        return opaque(Some(why::PARAM));
    }
    // An element of an array is read as the array: every element it holds.
    if is_name(name) {
        if let Some(subscript) = rest.strip_prefix('[') {
            let Some(end) = subscript.find(']') else {
                return opaque(Some(why::PARAM));
            };
            rest = &subscript[end + 1..];
        }
    }
    let word = |text: &str| {
        let mut lexer = Lexer::new(text);
        let mut w = Word::default();
        while lexer.peek(0).is_some() {
            lexer.word_char(&mut w, false);
        }
        if quoted {
            for part in &mut w.parts {
                if let Part::Lit { quoted: q, .. }
                | Part::Param { quoted: q, .. }
                | Part::Subst { quoted: q, .. }
                | Part::Opaque { quoted: q, .. } = part
                {
                    *q = true;
                }
            }
        }
        w.parts
    };
    let op = if rest.is_empty() {
        None
    } else if let Some((arg, assign)) = [(":-", false), ("-", false), (":=", true), ("=", true)]
        .iter()
        .find_map(|(op, assign)| rest.strip_prefix(op).map(|arg| (arg, *assign)))
    {
        Some(ParamOp::Default {
            word: word(arg),
            assign,
        })
    } else if rest.starts_with(":?") || rest.starts_with('?') {
        Some(ParamOp::Required)
    } else if let Some(arg) = [":+", "+"].iter().find_map(|op| rest.strip_prefix(op)) {
        Some(ParamOp::Alternate(word(arg)))
    } else if let Some((suffix, longest, pattern)) = [
        ("%%", true, true),
        ("%", true, false),
        ("##", false, true),
        ("#", false, false),
    ]
    .iter()
    .find_map(|(op, suffix, longest)| rest.strip_prefix(op).map(|p| (*suffix, *longest, p)))
    {
        if pattern.contains(['$', '`', '\\', '\'', '"', '[']) {
            return opaque(Some(why::PARAM));
        }
        Some(ParamOp::Strip {
            suffix,
            longest,
            pattern: pattern.to_string(),
        })
    } else {
        // Substitution, substrings and case changes are not resolved.
        return opaque(Some(why::PARAM));
    };
    Part::Param {
        name: name.to_string(),
        op,
        quoted,
    }
}

/// A word's literal text, for a heredoc delimiter.
fn word_text(word: &Word) -> String {
    word.parts
        .iter()
        .map(|part| match part {
            Part::Lit { text, .. } | Part::Opaque { text, .. } => text.clone(),
            Part::Tilde(user) => format!("~{user}"),
            Part::Param { name, .. } => format!("${name}"),
            Part::Subst { script, .. } => format!("$({script})"),
            Part::Array(words) => {
                let words: Vec<String> = words.iter().map(word_text).collect();
                format!("({})", words.join(" "))
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Simple(Vec<Word>, Vec<Redir>),
    /// Runs in a child shell: `( … )`, a background job.
    Sub(Box<Node>),
    Seq(Vec<Node>),
    /// The first runs; each later one may or may not.
    AndOr(Vec<Node>),
    /// Each stage runs in a child shell.
    Pipe(Vec<Node>),
    If(Vec<(Node, Node)>, Option<Box<Node>>),
    /// `while`/`until`: the condition runs at least once, the body any
    /// number of times.
    Loop(Box<Node>, Box<Node>),
    For(String, Option<Vec<Word>>, Box<Node>),
    Case(Vec<Node>),
    Func(String, Box<Node>),
    /// A compound command with redirections: its input feeds the commands
    /// inside.
    Redirected(Box<Node>, Vec<Redir>),
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn parse(script: &str) -> Node {
        let mut parser = Self {
            toks: Lexer::new(script).tokens(),
            at: 0,
        };
        let mut items = Vec::new();
        while parser.at < parser.toks.len() {
            let before = parser.at;
            items.push(parser.list(&[]));
            if parser.at == before {
                parser.at += 1; // an unmatched closer
            }
        }
        Node::Seq(items)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn at_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w.plain() == Some(word))
    }

    fn eat_word(&mut self, word: &str) {
        if self.at_word(word) {
            self.at += 1;
        }
    }

    fn skip_newlines(&mut self) {
        while self.at_op("\n") {
            self.at += 1;
        }
    }

    fn at_stop(&self, stops: &[&str]) -> bool {
        match self.peek() {
            None => true,
            Some(Tok::Op(op)) => matches!(*op, ")" | ";;" | ";&" | ";;&"),
            Some(Tok::Word(w)) => w.plain().is_some_and(|w| stops.contains(&w)),
            Some(Tok::Redir(_)) => false,
        }
    }

    fn list(&mut self, stops: &[&str]) -> Node {
        let mut items = Vec::new();
        loop {
            while self.at_op("\n") || self.at_op(";") {
                self.at += 1;
            }
            if self.at_stop(stops) {
                break;
            }
            let before = self.at;
            let node = self.and_or();
            if self.at_op("&") {
                self.at += 1;
                items.push(Node::Sub(Box::new(node)));
            } else {
                items.push(node);
            }
            if self.at == before {
                break;
            }
        }
        Node::Seq(items)
    }

    fn and_or(&mut self) -> Node {
        let mut items = vec![self.pipeline()];
        while self.at_op("&&") || self.at_op("||") {
            self.at += 1;
            self.skip_newlines();
            items.push(self.pipeline());
        }
        if items.len() == 1 {
            items.remove(0)
        } else {
            Node::AndOr(items)
        }
    }

    fn pipeline(&mut self) -> Node {
        self.eat_word("!");
        let mut stages = vec![self.command()];
        while self.at_op("|") {
            self.at += 1;
            self.skip_newlines();
            stages.push(self.command());
        }
        if stages.len() == 1 {
            stages.remove(0)
        } else {
            Node::Pipe(stages)
        }
    }

    fn command(&mut self) -> Node {
        let node = if self.at_op("(") {
            self.at += 1;
            let body = self.list(&[]);
            if self.at_op(")") {
                self.at += 1;
            }
            Node::Sub(Box::new(body))
        } else if self.at_word("{") {
            self.at += 1;
            let body = self.list(&["}"]);
            self.eat_word("}");
            body
        } else if self.at_word("if") {
            self.if_clause()
        } else if self.at_word("while") || self.at_word("until") {
            self.at += 1;
            let cond = self.list(&["do"]);
            self.eat_word("do");
            let body = self.list(&["done"]);
            self.eat_word("done");
            Node::Loop(Box::new(cond), Box::new(body))
        } else if self.at_word("for") || self.at_word("select") {
            self.for_clause()
        } else if self.at_word("case") {
            self.case_clause()
        } else if self.at_word("function") {
            self.at += 1;
            let name = match self.peek() {
                Some(Tok::Word(w)) => word_text(w),
                _ => String::new(),
            };
            self.at += 1;
            if self.at_op("(") {
                self.at += 1;
                if self.at_op(")") {
                    self.at += 1;
                }
            }
            self.skip_newlines();
            let body = self.command();
            Node::Func(name, Box::new(body))
        } else {
            return self.simple();
        };
        let mut redirs = Vec::new();
        while let Some(Tok::Redir(redir)) = self.peek() {
            redirs.push(redir.clone());
            self.at += 1;
        }
        if redirs.is_empty() {
            node
        } else {
            Node::Redirected(Box::new(node), redirs)
        }
    }

    fn simple(&mut self) -> Node {
        let mut words = Vec::new();
        let mut redirs = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::Word(w)) => {
                    words.push(w.clone());
                    self.at += 1;
                    if words.len() == 1
                        && self.at_op("(")
                        && matches!(self.toks.get(self.at + 1), Some(Tok::Op(")")))
                    {
                        self.at += 2;
                        self.skip_newlines();
                        let body = self.command();
                        return Node::Func(word_text(&words[0]), Box::new(body));
                    }
                }
                Some(Tok::Redir(r)) => {
                    redirs.push(r.clone());
                    self.at += 1;
                }
                _ => break,
            }
        }
        Node::Simple(words, redirs)
    }

    fn if_clause(&mut self) -> Node {
        self.at += 1;
        let mut arms = Vec::new();
        let cond = self.list(&["then"]);
        self.eat_word("then");
        let body = self.list(&["elif", "else", "fi"]);
        arms.push((cond, body));
        while self.at_word("elif") {
            self.at += 1;
            let cond = self.list(&["then"]);
            self.eat_word("then");
            let body = self.list(&["elif", "else", "fi"]);
            arms.push((cond, body));
        }
        let otherwise = if self.at_word("else") {
            self.at += 1;
            Some(Box::new(self.list(&["fi"])))
        } else {
            None
        };
        self.eat_word("fi");
        Node::If(arms, otherwise)
    }

    fn for_clause(&mut self) -> Node {
        self.at += 1;
        let name = match self.peek() {
            Some(Tok::Word(w)) => w.plain().map(str::to_string),
            _ => None,
        };
        self.at += 1;
        self.skip_newlines();
        let mut words = None;
        if self.at_word("in") {
            self.at += 1;
            let mut list = Vec::new();
            while let Some(Tok::Word(w)) = self.peek() {
                list.push(w.clone());
                self.at += 1;
            }
            words = Some(list);
        }
        while self.at_op(";") || self.at_op("\n") {
            self.at += 1;
        }
        self.eat_word("do");
        let body = self.list(&["done"]);
        self.eat_word("done");
        match name {
            Some(name) if is_name(&name) => Node::For(name, words, Box::new(body)),
            // `for (( … ))` and other forms: a loop over an unknown value.
            _ => Node::Loop(Box::new(Node::Seq(Vec::new())), Box::new(body)),
        }
    }

    fn case_clause(&mut self) -> Node {
        self.at += 1;
        self.at += 1; // the word
        self.skip_newlines();
        self.eat_word("in");
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_word("esac") {
                self.at += 1;
                break;
            }
            if self.peek().is_none() {
                break;
            }
            if self.at_op("(") {
                self.at += 1;
            }
            while self.peek().is_some() && !self.at_op(")") {
                self.at += 1;
            }
            if self.at_op(")") {
                self.at += 1;
            }
            arms.push(self.list(&["esac"]));
            if self.at_op(";;") || self.at_op(";&") || self.at_op(";;&") {
                self.at += 1;
            }
        }
        Node::Case(arms)
    }
}

// ---------------------------------------------------------------------------
// Values and taint
// ---------------------------------------------------------------------------

type Values = BTreeSet<String>;

/// Opens the reason inside a value the reader cannot prove.
const TAINT_OPEN: char = '\u{E001}';
/// Closes that reason.
const TAINT_CLOSE: char = '\u{E002}';
/// Separates the fields of `"$@"` held in one value.
const FIELD_BREAK: char = '\u{E000}';
/// Paths a glob may name that the reader checks before it gives up.
const MAX_GLOB: usize = 4096;

/// The constructs that make a value unknown, as a refusal names them.
mod why {
    pub const SOURCE: &str = "a sourced file";
    pub const NAMEREF: &str = "a name reference (`declare -n`)";
    pub const COMMAND: &str = "a command whose name the guard cannot resolve";
    pub const NAME: &str = "a variable name the guard cannot resolve";
    pub const EVAL: &str = "`eval` of text the guard cannot resolve";
    pub const ALIAS: &str = "an alias the guard cannot resolve";
    pub const DEPTH: &str = "a function call or nested script deeper than the guard follows";
    pub const LOOP: &str = "a loop the guard cannot settle";
    pub const MANY: &str = "more values than the guard follows";
    pub const READ_OPTION: &str = "a `read` option the guard does not model";
    pub const READ_INPUT: &str =
        "input the guard cannot see (`read` from a file or an unknown command)";
    pub const MAPFILE: &str = "`mapfile` input the guard cannot see";
    pub const GETOPTS: &str = "`getopts`";
    pub const CASE: &str = "a case-converting attribute (`declare -u`, `-l` or `-c`)";
    pub const DECLARE: &str = "a declaration option the guard does not model";
    pub const IFS: &str = "a field split on an IFS the guard cannot resolve";
    pub const PARAM: &str = "a parameter expansion the guard does not resolve";
    pub const GLOB: &str = "a glob the guard could not list on disk";
    pub const SHIFT: &str = "a `shift` count the guard cannot resolve";
    pub const PRINTF: &str = "a `printf -v` value the guard cannot resolve";
}

/// A value that depends on `reason`, a construct the reader does not model
/// exactly.
fn taint(reason: &str) -> String {
    format!("{TAINT_OPEN}{reason}{TAINT_CLOSE}")
}

/// The construct a value depends on, when the value is unproven.
fn unproven(value: &str) -> Option<&str> {
    let start = value.find(TAINT_OPEN)? + TAINT_OPEN.len_utf8();
    let len = value[start..].find(TAINT_CLOSE)?;
    Some(&value[start..start + len])
}

/// A value read from the environment or from output the reader does not
/// run, kept as spelled (`$NAME`, `$(…)`).
fn spelled(value: &str) -> bool {
    value.starts_with('$')
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The value a name had before a function call made it local, and whether
/// it is local on only some paths through the call.
#[derive(Debug, Clone, PartialEq)]
struct Saved {
    values: Values,
    maybe: bool,
}

/// What a point in the line may hold. A working directory of `""` is where
/// the line started (the project); a relative one is below it.
#[derive(Debug, Clone, PartialEq)]
struct State {
    vars: BTreeMap<String, Values>,
    cwd: Values,
    oldpwd: Values,
    /// Every directory the line may have been in, for `popd`.
    visited: Values,
    /// `$0`: the shell's name, or what `sh -c SCRIPT NAME` gives it.
    arg0: Values,
    /// The positional parameters, `$1` onwards.
    args: Vec<Values>,
    /// Each body a function name may have; `None` where it may be
    /// undefined.
    funcs: BTreeMap<String, Vec<Option<Node>>>,
    /// Each alias's text; `None` where the reader cannot know it.
    aliases: BTreeMap<String, Option<String>>,
    /// Names that hold an array, whose elements are one value set.
    arrays: BTreeSet<String>,
    readonly: BTreeSet<String>,
    /// Names with an attribute the reader does not model: every value
    /// they take is unknown.
    sticky: BTreeMap<String, &'static str>,
    /// Set by a construct that may change any variable and the working
    /// directory; from there on each of them is unknown.
    wild: Option<&'static str>,
    /// One entry per function call being read: the locals it saved.
    scopes: Vec<BTreeMap<String, Saved>>,
    /// The line cannot reach here (after `exit`, or a `break`, `continue`
    /// or `return` that leaves).
    dead: bool,
}

impl State {
    fn start() -> Self {
        let here: Values = [String::new()].into();
        Self {
            vars: BTreeMap::new(),
            cwd: here.clone(),
            oldpwd: ["$OLDPWD".to_string()].into(),
            visited: here,
            arg0: ["$0".to_string()].into(),
            args: Vec::new(),
            funcs: BTreeMap::new(),
            aliases: BTreeMap::new(),
            arrays: BTreeSet::new(),
            readonly: BTreeSet::new(),
            sticky: BTreeMap::new(),
            wild: None,
            scopes: Vec::new(),
            dead: false,
        }
    }

    /// The values `name` may hold; one read from the environment is kept
    /// as spelled, except `HOME`, which is the home directory, and `IFS`,
    /// which the shell sets itself.
    fn var(&self, name: &str) -> Values {
        if let Some(reason) = self.wild {
            return [taint(reason)].into();
        }
        if let Some(values) = self.vars.get(name) {
            return values.clone();
        }
        match name {
            "HOME" => ["~".to_string()].into(),
            "PWD" => self.cwd.iter().map(|c| display_dir(c)).collect(),
            "OLDPWD" => self.oldpwd.iter().map(|c| display_dir(c)).collect(),
            "IFS" => [" \t\n".to_string()].into(),
            _ => [format!("${name}")].into(),
        }
    }

    /// Assign `name`. A readonly name keeps its value (the assignment
    /// fails), and a name with an attribute the reader does not model
    /// takes an unknown one.
    fn set(&mut self, name: &str, values: Values) {
        let values = if let Some(reason) = self.sticky.get(name) {
            [taint(reason)].into()
        } else if self.readonly.contains(name) {
            self.var(name).union(&values).cloned().collect()
        } else {
            values
        };
        self.vars.insert(name.to_string(), bounded(values));
    }

    /// Change directory. A `cd` to a literal path gives a known directory
    /// again, even after a construct made it unknown.
    fn set_cwd(&mut self, cwd: Values) {
        let cwd = bounded(cwd);
        self.oldpwd = std::mem::replace(&mut self.cwd, cwd);
        self.visited = bounded(self.visited.union(&self.cwd).cloned().collect());
    }

    /// A construct that may change any variable and the working directory
    /// (a sourced file, a name reference, a command the reader cannot
    /// name): from here on each of them is unknown.
    fn go_wild(&mut self, reason: &'static str) {
        let reason = *self.wild.get_or_insert(reason);
        let unknown: Values = [taint(reason)].into();
        self.cwd.clone_from(&unknown);
        self.oldpwd.clone_from(&unknown);
        self.visited = unknown;
    }

    /// Make `name` local to the function call being read.
    fn declare_local(&mut self, name: &str) {
        let current = self.var(name);
        if let Some(scope) = self.scopes.last_mut() {
            scope.entry(name.to_string()).or_insert(Saved {
                values: current,
                maybe: false,
            });
        }
    }

    /// Return from a function call: its locals take back their values.
    fn leave_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        if self.dead {
            return;
        }
        for (name, saved) in scope {
            let values = if saved.maybe {
                saved.values.union(&self.var(&name)).cloned().collect()
            } else {
                saved.values
            };
            self.vars.insert(name, values);
        }
    }

    /// `unset NAME`. Unsetting a name a caller made local may reveal the
    /// value it saved, so that value stays feasible.
    fn unset(&mut self, name: &str) {
        if self.readonly.contains(name) {
            return;
        }
        self.sticky.remove(name);
        self.arrays.remove(name);
        if name == "IFS" && self.scopes.is_empty() {
            self.vars.remove(name);
            return;
        }
        let mut values: Values = [String::new()].into();
        for scope in &self.scopes {
            if let Some(saved) = scope.get(name) {
                values.extend(saved.values.iter().cloned());
            }
        }
        self.vars.insert(name.to_string(), bounded(values));
    }

    /// Every path through `self` or `other`.
    fn join(&mut self, other: &Self) {
        if other.dead {
            return;
        }
        if self.dead {
            *self = other.clone();
            return;
        }
        let names: BTreeSet<String> = self.vars.keys().chain(other.vars.keys()).cloned().collect();
        for name in names {
            let joined: Values = self.var(&name).union(&other.var(&name)).cloned().collect();
            self.vars.insert(name, bounded(joined));
        }
        self.cwd = bounded(self.cwd.union(&other.cwd).cloned().collect());
        self.oldpwd = bounded(self.oldpwd.union(&other.oldpwd).cloned().collect());
        self.visited = bounded(self.visited.union(&other.visited).cloned().collect());
        self.arg0 = bounded(self.arg0.union(&other.arg0).cloned().collect());
        let unset: Values = [String::new()].into();
        let len = self.args.len().max(other.args.len());
        self.args = (0..len)
            .map(|i| {
                let mine = self.args.get(i).unwrap_or(&unset);
                let theirs = other.args.get(i).unwrap_or(&unset);
                bounded(mine.union(theirs).cloned().collect())
            })
            .collect();
        let names: BTreeSet<String> = self
            .funcs
            .keys()
            .chain(other.funcs.keys())
            .cloned()
            .collect();
        for name in names {
            let mut bodies = self.funcs.get(&name).cloned().unwrap_or_else(|| vec![None]);
            for body in other
                .funcs
                .get(&name)
                .cloned()
                .unwrap_or_else(|| vec![None])
            {
                if !bodies.contains(&body) {
                    bodies.push(body);
                }
            }
            self.funcs.insert(name, bodies);
        }
        // An alias defined on one path only keeps its text: a command is
        // read both expanded and not. Two texts leave it unknown.
        for (name, text) in &other.aliases {
            match self.aliases.get(name) {
                Some(mine) if mine != text => {
                    self.aliases.insert(name.clone(), None);
                }
                Some(_) => {}
                None => {
                    self.aliases.insert(name.clone(), text.clone());
                }
            }
        }
        self.arrays.extend(other.arrays.iter().cloned());
        self.readonly.extend(other.readonly.iter().cloned());
        for (name, reason) in &other.sticky {
            self.sticky.entry(name.clone()).or_insert(reason);
        }
        self.wild = self.wild.or(other.wild);
        let levels = self.scopes.len().max(other.scopes.len());
        self.scopes.resize_with(levels, BTreeMap::new);
        for (level, theirs) in self.scopes.iter_mut().zip(
            other
                .scopes
                .iter()
                .cloned()
                .chain(std::iter::repeat_with(BTreeMap::new)),
        ) {
            for (name, saved) in level.iter_mut() {
                match theirs.get(name) {
                    Some(other) => {
                        saved.values.extend(other.values.iter().cloned());
                        saved.maybe |= other.maybe;
                    }
                    None => saved.maybe = true,
                }
            }
            for (name, saved) in theirs {
                level.entry(name).or_insert(Saved {
                    values: saved.values,
                    maybe: true,
                });
            }
        }
    }

    /// `declare -g` in a function sets the global value, which a caller's
    /// local of the same name hides until that caller returns.
    fn assign_global(&mut self, name: &str, values: Values) {
        let saved = self.scopes.iter_mut().find_map(|scope| scope.get_mut(name));
        match saved {
            Some(saved) => saved.values = values,
            None => self.set(name, values),
        }
    }

    /// Apply an assignment: an element or an append adds to what the name
    /// holds, and an array holds every element as one set.
    fn assign(&mut self, assign: &Assign, values: Values) {
        let array_value = matches!(assign.value.first(), Some(Part::Array(_)));
        let was_array = self.arrays.contains(&assign.name);
        if array_value || assign.element {
            self.arrays.insert(assign.name.clone());
        }
        let old = self.var(&assign.name);
        let appended = || -> Values {
            old.iter()
                .flat_map(|o| values.iter().map(move |v| format!("{o}{v}")))
                .collect()
        };
        let new: Values = if assign.append && !array_value {
            let mut new = appended();
            if assign.element || was_array {
                new.extend(old.iter().cloned());
            }
            new
        } else if assign.element || (assign.append && array_value) || (was_array && !array_value) {
            old.union(&values).cloned().collect()
        } else {
            values
        };
        self.set(&assign.name, new);
    }

    /// After a loop the reader could not settle: every value still
    /// changing between its last two passes is also unknown.
    fn widen(&mut self, before: &Self) {
        let names: Vec<String> = self.vars.keys().cloned().collect();
        for name in names {
            if self.var(&name) != before.var(&name) {
                if let Some(values) = self.vars.get_mut(&name) {
                    values.insert(taint(why::LOOP));
                }
            }
        }
        if self.cwd != before.cwd {
            self.cwd.insert(taint(why::LOOP));
        }
    }
}

/// Keep at most [`MAX_VALUES`], protected and unknown values first; a set
/// that had to drop values also holds an unknown one.
fn bounded(values: Values) -> Values {
    if values.len() <= MAX_VALUES {
        return values;
    }
    let (mut kept, rest): (Values, Values) = values
        .into_iter()
        .partition(|value| unproven(value).is_some() || dangerous_rm_target(value).is_some());
    kept.insert(taint(why::MANY));
    for value in rest {
        if kept.len() >= MAX_VALUES {
            break;
        }
        kept.insert(value);
    }
    kept
}

/// The positional parameters a command's argument vectors give, from
/// `argv[from]`: each position holds every value it takes.
fn positionals(variants: &[Vec<String>], from: usize) -> Vec<Values> {
    let len = variants
        .iter()
        .map(|argv| argv.len().saturating_sub(from))
        .max()
        .unwrap_or(0);
    (0..len)
        .map(|i| {
            variants
                .iter()
                .map(|argv| argv.get(from + i).cloned().unwrap_or_default())
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Reading the program
// ---------------------------------------------------------------------------

/// A value a producer feeds `xargs`, `parallel` or `read`; `tree` when it
/// stands for a whole tree (a `find` start), so a remover given it deletes
/// recursively.
#[derive(Debug, Clone)]
struct Input {
    value: String,
    tree: bool,
}

/// What a command receives on its input, and whether that is all it may
/// receive (false for a file or a command the reader does not run).
#[derive(Debug, Clone)]
struct Fed {
    items: Vec<Input>,
    complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Loop,
    Function,
    /// A child shell: a jump inside it never leaves it.
    Child,
}

#[derive(Debug, Clone, Copy)]
enum Jump {
    /// `break` or `continue`, with the loops it leaves.
    Loop(usize),
    Return,
}

/// A loop, function call or child shell being read, with the states of
/// each jump that left it early.
struct Frame {
    kind: FrameKind,
    left: Vec<(Jump, State)>,
}

/// How a deletion's target reads.
enum Judged {
    Clear,
    Protected(&'static str),
    Unproven(Unproven),
}

struct Reader<'a> {
    base: Option<&'a Path>,
    found: Option<Found>,
    depth: usize,
    /// What the pipeline stage being read receives on its input.
    pipe_input: Option<Fed>,
    jumps: Vec<Frame>,
    /// Aliases being expanded, which are not expanded again inside.
    expanding: Vec<String>,
    /// Trap actions, read again when the line ends.
    traps: Vec<String>,
}

impl Reader<'_> {
    /// Whether a protected target is found; an unproven one keeps the
    /// reader going, since a protected one names the refusal better.
    fn done(&self) -> bool {
        self.found.as_ref().is_some_and(|f| f.unproven.is_none())
    }

    fn script(&mut self, script: &str, st: &mut State) {
        if self.done() {
            return;
        }
        // Past the depth it follows, the reader reads once more with every
        // value unknown, so a deletion there is still judged.
        if self.depth > MAX_DEPTH {
            if st.wild.is_some() {
                return;
            }
            st.go_wild(why::DEPTH);
        }
        self.depth += 1;
        let node = Parser::parse(script);
        self.run(&node, st);
        self.depth -= 1;
    }

    /// Read `script` in a child shell of the line.
    fn child_script(&mut self, script: &str, child: &mut State) {
        self.jumps.push(Frame {
            kind: FrameKind::Child,
            left: Vec::new(),
        });
        self.script(script, child);
        self.jumps.pop();
    }

    fn report(&mut self, judged: Judged, ambiguous: bool) {
        match judged {
            Judged::Protected(target) => {
                if !self.done() {
                    self.found = Some(Found {
                        target,
                        ambiguous,
                        unproven: None,
                    });
                }
            }
            Judged::Unproven(unproven) => {
                if self.found.is_none() {
                    self.found = Some(Found {
                        target: "unproven deletion target",
                        ambiguous: false,
                        unproven: Some(unproven),
                    });
                }
            }
            Judged::Clear => {}
        }
    }

    /// Judge each path in turn: a protected one is reported at once, an
    /// unproven one once none of them is protected.
    fn judge_paths<'p>(
        &mut self,
        paths: impl IntoIterator<Item = &'p str>,
        st: &State,
        ambiguous: bool,
    ) {
        let mut pending = None;
        for path in paths {
            match self.judge_path(path, st) {
                Judged::Protected(target) => {
                    self.report(Judged::Protected(target), ambiguous);
                    return;
                }
                Judged::Unproven(unproven) => {
                    pending.get_or_insert(unproven);
                }
                Judged::Clear => {}
            }
        }
        if let Some(unproven) = pending {
            self.report(Judged::Unproven(unproven), ambiguous);
        }
    }

    /// Read `body` from `st` as a loop pass, a function call or a child
    /// shell, then join in every state that left it early. A `break N`
    /// that leaves more loops, and a `return` from inside a loop, go on to
    /// the enclosing frame too.
    fn frame(&mut self, kind: FrameKind, body: &Node, st: &mut State) {
        self.jumps.push(Frame {
            kind,
            left: Vec::new(),
        });
        self.run(body, st);
        let frame = self.jumps.pop().expect("the frame pushed above");
        for (jump, early) in frame.left {
            st.join(&early);
            let outward = match (kind, jump) {
                (FrameKind::Loop, Jump::Loop(levels)) if levels > 1 => Some(Jump::Loop(levels - 1)),
                (FrameKind::Loop, Jump::Return) => Some(Jump::Return),
                _ => None,
            };
            if let (Some(jump), Some(outer)) = (outward, self.jumps.last_mut()) {
                outer.left.push((jump, early));
            }
        }
    }

    fn run(&mut self, node: &Node, st: &mut State) {
        if self.done() || st.dead {
            return;
        }
        match node {
            Node::Seq(items) => {
                for item in items {
                    self.run(item, st);
                }
            }
            Node::Sub(body) => {
                let mut child = st.clone();
                self.frame(FrameKind::Child, body, &mut child);
            }
            Node::AndOr(items) => {
                self.run(&items[0], st);
                for item in &items[1..] {
                    let mut taken = st.clone();
                    self.run(item, &mut taken);
                    st.join(&taken);
                }
            }
            Node::Pipe(stages) => self.pipe(stages, st),
            Node::If(arms, otherwise) => {
                let mut cur = st.clone();
                let mut outs = Vec::new();
                for (cond, body) in arms {
                    self.run(cond, &mut cur);
                    let mut taken = cur.clone();
                    self.run(body, &mut taken);
                    outs.push(taken);
                }
                if let Some(otherwise) = otherwise {
                    self.run(otherwise, &mut cur);
                }
                for out in outs {
                    cur.join(&out);
                }
                *st = cur;
            }
            Node::Loop(cond, body) => {
                let mut acc = st.clone();
                self.run(cond, &mut acc);
                let mut before = acc.clone();
                let mut settled = false;
                for _ in 0..LOOP_PASSES {
                    let mut pass = acc.clone();
                    self.frame(FrameKind::Loop, body, &mut pass);
                    self.run(cond, &mut pass);
                    let mut next = acc.clone();
                    next.join(&pass);
                    if next == acc {
                        settled = true;
                        break;
                    }
                    before = std::mem::replace(&mut acc, next);
                }
                if !settled {
                    acc.widen(&before);
                }
                *st = acc;
            }
            Node::For(name, words, body) => self.for_loop(name, words.as_deref(), body, st),
            Node::Case(arms) => {
                let entry = st.clone();
                for arm in arms {
                    let mut taken = entry.clone();
                    self.run(arm, &mut taken);
                    st.join(&taken);
                }
            }
            Node::Func(name, body) => {
                st.funcs.insert(name.clone(), vec![Some((**body).clone())]);
            }
            Node::Redirected(body, redirs) => {
                let fed = self.redirect_input(redirs, st);
                let outer = self.pipe_input.clone();
                if fed.is_some() {
                    self.pipe_input = fed;
                }
                self.run(body, st);
                self.pipe_input = outer;
            }
            Node::Simple(words, redirs) => self.simple(words, redirs, st),
        }
    }

    fn pipe(&mut self, stages: &[Node], st: &mut State) {
        let outer = self.pipe_input.take();
        let mut last = st.clone();
        for (at, stage) in stages.iter().enumerate() {
            self.pipe_input = if at == 0 {
                outer.clone()
            } else {
                Some(self.producer_values(&stages[at - 1], st))
            };
            let mut child = st.clone();
            self.frame(FrameKind::Child, stage, &mut child);
            last = child;
        }
        self.pipe_input = outer;
        // Under zsh (or bash `lastpipe`) the last stage runs in this shell.
        st.join(&last);
    }

    fn for_loop(&mut self, name: &str, words: Option<&[Word]>, body: &Node, st: &mut State) {
        let (values, fixed) = match words {
            Some(words) => {
                let mut values = Values::new();
                let mut last = None;
                let mut fixed = true;
                for word in words {
                    let fields = self.fields(word, st);
                    fixed &= fields.len() == 1 && fields[0].len() == 1;
                    for alternative in fields {
                        last = alternative.last().cloned();
                        values.extend(alternative);
                    }
                }
                (values, fixed.then_some(last).flatten())
            }
            // `for NAME` walks the positional parameters.
            None => match st.wild {
                Some(reason) => ([taint(reason)].into(), None),
                None => (st.args.iter().flatten().cloned().collect(), None),
            },
        };
        if values.is_empty() {
            return;
        }
        let mut acc = st.clone();
        let mut before = acc.clone();
        let mut settled = false;
        for _ in 0..LOOP_PASSES {
            let mut pass = acc.clone();
            pass.set(name, values.clone());
            self.frame(FrameKind::Loop, body, &mut pass);
            let mut next = acc.clone();
            next.join(&pass);
            if next == acc {
                settled = true;
                break;
            }
            before = std::mem::replace(&mut acc, next);
        }
        if !settled {
            acc.widen(&before);
        }
        // A fixed list the body never leaves early nor reassigns ends on its
        // last word; otherwise every word stays feasible.
        match fixed {
            Some(last) if !may_break(body, &acc) && !may_assign(body, name, &acc) => {
                acc.set(name, [last].into());
            }
            _ => {
                let mut all = acc.var(name);
                all.extend(values);
                acc.set(name, all);
            }
        }
        *st = acc;
    }

    /// A simple command, read expanded when its first word is an alias
    /// and also as written, since bash does not expand an alias defined on
    /// the same line.
    fn simple(&mut self, words: &[Word], redirs: &[Redir], st: &mut State) {
        let expanded = self.expand_alias(words, redirs, st);
        self.simple_plain(words, redirs, st);
        if let Some(expanded) = expanded {
            st.join(&expanded);
        }
    }

    fn expand_alias(&mut self, words: &[Word], redirs: &[Redir], st: &State) -> Option<State> {
        let assigned = words
            .iter()
            .take_while(|w| w.assignment().is_some())
            .count();
        let first = words.get(assigned)?.plain()?;
        let text = st.aliases.get(first)?.clone();
        if self.expanding.iter().any(|name| name == first) {
            return None;
        }
        let mut expanded = st.clone();
        match text {
            Some(text) => {
                let mut line: Vec<String> =
                    words[..assigned].iter().map(|w| w.src.clone()).collect();
                let mut next = assigned + 1;
                let mut text = text;
                // An alias ending in a blank expands the next word too.
                while text.ends_with([' ', '\t']) {
                    let Some(Some(more)) = words
                        .get(next)
                        .and_then(Word::plain)
                        .filter(|w| *w != first)
                        .and_then(|w| st.aliases.get(w))
                    else {
                        break;
                    };
                    line.push(std::mem::replace(&mut text, more.clone()));
                    next += 1;
                }
                line.push(text);
                line.extend(words[next..].iter().map(|w| w.src.clone()));
                let line = line.join(" ");
                self.expanding.push(first.to_string());
                let _ = self.redirect_input(redirs, &expanded);
                self.script(&line, &mut expanded);
                self.expanding.pop();
            }
            None => expanded.go_wild(why::ALIAS),
        }
        Some(expanded)
    }

    fn simple_plain(&mut self, words: &[Word], redirs: &[Redir], st: &mut State) {
        let assigned = words
            .iter()
            .take_while(|w| w.assignment().is_some())
            .count();
        let (assignments, command) = words.split_at(assigned);
        // Redirections and heredocs are read even with no command.
        let input = self.redirect_input(redirs, st);
        if command.is_empty() {
            // Assignments alone take effect in order, each seeing the last.
            for word in assignments {
                if let Some(assign) = word.assignment() {
                    let values = self.value(&assign.value, st);
                    st.assign(&assign, values);
                }
            }
            self.side_effects(words, st);
            return;
        }
        // A prefix assignment may see the ones before it (bash) or not.
        let mut scratch = st.clone();
        let mut env = Vec::new();
        for word in assignments {
            if let Some(assign) = word.assignment() {
                let mut values = self.value(&assign.value, &scratch);
                values.extend(self.value(&assign.value, st));
                scratch.assign(&assign, values);
                env.push((assign.name.clone(), scratch.var(&assign.name)));
            }
        }
        let variants = self.argv_variants(command, st);
        self.side_effects(words, st);
        self.dispatch(command, &variants, &env, input.as_ref(), st);
    }

    /// Run a command with its argument vectors. When its name takes
    /// several values, each program is read on its own and the states
    /// joined.
    fn dispatch(
        &mut self,
        command: &[Word],
        variants: &[Vec<String>],
        env: &[(String, Values)],
        input: Option<&Fed>,
        st: &mut State,
    ) {
        let ambiguous = variants.len() > 1;
        let mut groups: Vec<(&str, Vec<Vec<String>>)> = Vec::new();
        for argv in variants {
            let key = argv.first().map_or("", String::as_str);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, group)) => group.push(argv.clone()),
                None => groups.push((key, vec![argv.clone()])),
            }
        }
        if groups.len() <= 1 {
            self.dispatch_program(command, variants, env, input, ambiguous, st);
            return;
        }
        let entry = st.clone();
        let mut out: Option<State> = None;
        for (_, group) in groups {
            let mut taken = entry.clone();
            self.dispatch_program(command, &group, env, input, ambiguous, &mut taken);
            match &mut out {
                Some(out) => out.join(&taken),
                None => out = Some(taken),
            }
        }
        if let Some(out) = out {
            *st = out;
        }
    }

    /// Run one program: a function or a builtin changes this shell, and
    /// anything else runs in a child and is judged for what it deletes.
    fn dispatch_program(
        &mut self,
        command: &[Word],
        variants: &[Vec<String>],
        env: &[(String, Values)],
        input: Option<&Fed>,
        ambiguous: bool,
        st: &mut State,
    ) {
        // `command`, `builtin` and `time` before a builtin still run it in
        // this shell; `command -v` only prints.
        let Some((skip, functions)) = builtin_prefix(command) else {
            return;
        };
        let words = &command[skip..];
        let variants: Vec<Vec<String>> = variants
            .iter()
            .map(|argv| argv.get(skip..).unwrap_or_default().to_vec())
            .collect();
        let Some(program) = variants.first().and_then(|argv| argv.first()).cloned() else {
            return;
        };
        // A command the reader cannot name may be any builtin: `cd`,
        // `eval`, `source`.
        if unproven(&program).is_some() || (spelled(&program) && !program.contains('/')) {
            st.go_wild(why::COMMAND);
            return;
        }
        if functions {
            if let Some(bodies) = st.funcs.get(&program).cloned() {
                self.call(bodies, &variants, env, st);
                return;
            }
        }
        if is_builtin(&program, &variants) {
            // A builtin sees its prefix assignments, and they may persist
            // (a special builtin in POSIX mode).
            let before: Vec<(String, Values)> = env
                .iter()
                .map(|(name, _)| (name.clone(), st.var(name)))
                .collect();
            for (name, values) in env {
                st.set(name, values.clone());
            }
            let entry = st.clone();
            let mut out: Option<State> = None;
            for argv in &variants {
                let mut taken = entry.clone();
                self.builtin(&program, words, argv, input, &mut taken);
                match &mut out {
                    Some(out) => out.join(&taken),
                    None => out = Some(taken),
                }
            }
            if let Some(out) = out {
                *st = out;
            }
            for (name, old) in before {
                if !st.dead {
                    let mut both = old;
                    both.extend(st.var(&name));
                    st.set(&name, both);
                }
            }
            return;
        }
        let mut child = st.clone();
        for (name, values) in env {
            child.set(name, values.clone());
        }
        let fed = self.pipe_input.clone().or_else(|| input.cloned());
        for argv in &variants {
            self.exec(argv, &child, fed.as_ref(), false, ambiguous);
        }
    }

    /// Call a function: its body runs with the call's arguments and prefix
    /// assignments, and its locals and arguments are restored on return.
    fn call(
        &mut self,
        bodies: Vec<Option<Node>>,
        variants: &[Vec<String>],
        env: &[(String, Values)],
        st: &mut State,
    ) {
        if self.depth > MAX_DEPTH {
            if st.wild.is_some() {
                return;
            }
            st.go_wild(why::DEPTH);
        }
        let caller_args = std::mem::replace(&mut st.args, positionals(variants, 1));
        let mut scope = BTreeMap::new();
        for (name, _) in env {
            scope.insert(
                name.clone(),
                Saved {
                    values: st.var(name),
                    maybe: true,
                },
            );
        }
        st.scopes.push(scope);
        for (name, values) in env {
            st.set(name, values.clone());
        }
        let entry = st.clone();
        let mut out: Option<State> = None;
        self.depth += 1;
        for body in bodies {
            let mut taken = entry.clone();
            if let Some(body) = body {
                self.frame(FrameKind::Function, &body, &mut taken);
            }
            match &mut out {
                Some(out) => out.join(&taken),
                None => out = Some(taken),
            }
        }
        self.depth -= 1;
        if let Some(out) = out {
            *st = out;
        }
        st.leave_scope();
        if !st.dead {
            st.args = caller_args;
        }
    }

    /// A builtin that changes this shell, read for one argument vector.
    #[allow(clippy::too_many_lines)]
    fn builtin(
        &mut self,
        program: &str,
        words: &[Word],
        argv: &[String],
        input: Option<&Fed>,
        st: &mut State,
    ) {
        match program {
            "break" | "continue" => {
                let levels = argv
                    .get(1)
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                // A `break` in a function whose caller loops is left to the
                // shell's version: the reader reads on.
                if let Some(frame) = self.jumps.last_mut() {
                    if frame.kind == FrameKind::Loop {
                        frame.left.push((Jump::Loop(levels), st.clone()));
                        st.dead = true;
                    }
                }
            }
            "return" => {
                let in_function = self
                    .jumps
                    .iter()
                    .rev()
                    .take_while(|f| f.kind != FrameKind::Child)
                    .any(|f| f.kind == FrameKind::Function);
                if in_function {
                    if let Some(frame) = self.jumps.last_mut() {
                        frame.left.push((Jump::Return, st.clone()));
                    }
                    st.dead = true;
                }
            }
            "exit" => st.dead = true,
            "cd" | "pushd" | "popd" | "chdir" => {
                let cwd = cd(argv, st);
                st.set_cwd(cwd);
            }
            "export" | "readonly" | "local" | "declare" | "typeset" => {
                self.declare(program, words, st);
            }
            "unset" => {
                let mut mode = ' ';
                for arg in &argv[1..] {
                    match arg.as_str() {
                        "-f" => mode = 'f',
                        "-v" | "-n" => mode = 'v',
                        _ if arg.starts_with('-') => {}
                        name => {
                            if unproven(name).is_some() || spelled(name) {
                                st.go_wild(why::NAME);
                                return;
                            }
                            if mode != 'f' {
                                st.unset(name);
                            }
                            if mode == 'f' {
                                st.funcs.remove(name);
                            } else if mode == ' ' {
                                // `unset NAME` removes a function when no
                                // variable has the name.
                                if let Some(bodies) = st.funcs.get_mut(name) {
                                    if !bodies.contains(&None) {
                                        bodies.push(None);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "read" => self.read(argv, input, st),
            "mapfile" | "readarray" => {
                let fed = self.pipe_input.clone().or_else(|| input.cloned());
                let mut at = 1;
                let mut modelled = true;
                while let Some(arg) = argv.get(at) {
                    if !arg.starts_with('-') {
                        break;
                    }
                    modelled &= arg == "-t";
                    at += 1;
                }
                let name = argv.get(at).map_or("MAPFILE", String::as_str);
                let values: Values = match fed {
                    Some(fed) if fed.complete && modelled => fed
                        .items
                        .iter()
                        .flat_map(|i| i.value.lines().map(str::to_string).collect::<Vec<_>>())
                        .collect(),
                    _ => [taint(why::MAPFILE)].into(),
                };
                st.arrays.insert(name.to_string());
                st.set(name, values);
            }
            "getopts" => {
                for name in argv
                    .get(2)
                    .into_iter()
                    .map(String::as_str)
                    .chain(["OPTARG"])
                {
                    st.set(name, [taint(why::GETOPTS)].into());
                }
            }
            "set" => {
                let mut at = 1;
                while let Some(arg) = argv.get(at) {
                    if arg == "--" || arg == "-" {
                        st.args = positionals(&[argv.to_vec()], at + 1);
                        break;
                    }
                    if arg.len() > 1 && arg.starts_with(['-', '+']) {
                        // `-o NAME` takes the option's name.
                        at += if arg[1..].contains('o') { 2 } else { 1 };
                        continue;
                    }
                    st.args = positionals(&[argv.to_vec()], at);
                    break;
                }
            }
            "shift" => match argv.get(1).map_or(Some(1), |n| n.parse::<usize>().ok()) {
                Some(n) if n <= st.args.len() => {
                    st.args.drain(..n);
                }
                Some(_) => {}
                None => {
                    let mut all: Values = st.args.iter().flatten().cloned().collect();
                    all.insert(String::new());
                    all.insert(taint(why::SHIFT));
                    st.args = vec![all; st.args.len()];
                }
            },
            "eval" => {
                let text = argv[1..].join(" ");
                if unproven(&text).is_some() {
                    st.go_wild(why::EVAL);
                } else {
                    self.script(&text, st);
                }
            }
            "source" | "." => st.go_wild(why::SOURCE),
            "alias" => {
                for arg in &argv[1..] {
                    if let Some((name, text)) = arg.split_once('=') {
                        let text = unproven(text).is_none().then(|| text.to_string());
                        st.aliases.insert(name.to_string(), text);
                    }
                }
            }
            "unalias" => {
                for arg in &argv[1..] {
                    if arg == "-a" {
                        st.aliases.clear();
                    } else {
                        st.aliases.remove(arg);
                    }
                }
            }
            "printf" => {
                // `printf -v NAME FORMAT …` assigns instead of printing.
                if let Some(name) = argv.get(2) {
                    let values = match printf(&argv[3..]) {
                        Some(text) => [text].into(),
                        None => [taint(why::PRINTF)].into(),
                    };
                    if is_name(name) {
                        st.set(name, values);
                    } else {
                        st.go_wild(why::NAME);
                    }
                }
            }
            "trap" => {
                // `trap ACTION SIGNAL…`: the action runs later, so it is
                // read now and again when the line ends.
                if let [action, _, ..] = &argv[1..] {
                    if action != "-" && !action.starts_with('-') && !action.is_empty() {
                        let mut child = st.clone();
                        self.child_script(action, &mut child);
                        self.traps.push(action.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// `export`, `readonly`, `local`, `declare` and `typeset`.
    fn declare(&mut self, program: &str, words: &[Word], st: &mut State) {
        let mut at = 1;
        let mut flags = String::new();
        while let Some(text) = words.get(at).and_then(Word::plain) {
            if text == "--" {
                at += 1;
                break;
            }
            match text.strip_prefix(['-', '+']) {
                Some(f) if !f.is_empty() => {
                    flags.push_str(f);
                    at += 1;
                }
                _ => break,
            }
        }
        // Printing and functions change no variable.
        if flags.contains(['f', 'F', 'p']) {
            return;
        }
        if flags.contains('n') {
            st.go_wild(why::NAMEREF);
            return;
        }
        let unmodelled = flags.chars().any(|c| !"aAgilrtuxc".contains(c));
        let case = flags.contains(['l', 'u', 'c']);
        let global = flags.contains('g');
        let in_function = !st.scopes.is_empty();
        let local = program == "local"
            || (matches!(program, "declare" | "typeset") && in_function && !global);
        let readonly = program == "readonly" || flags.contains('r');
        let array = flags.contains(['a', 'A']);
        for word in &words[at..] {
            let mut names = Vec::new();
            if let Some(assign) = word.assignment() {
                if local {
                    st.declare_local(&assign.name);
                }
                let values = self.value(&assign.value, st);
                if program == "local" && !in_function {
                    // `local` outside a function fails in bash and assigns
                    // in zsh.
                    let mut both = st.var(&assign.name);
                    both.extend(values);
                    st.set(&assign.name, both);
                } else if global && in_function {
                    st.assign_global(&assign.name, values);
                } else {
                    st.assign(&assign, values);
                }
                names.push(assign.name);
            } else if let Some(name) = word.plain().filter(|n| is_name(n)) {
                if local {
                    st.declare_local(name);
                    st.set(name, [String::new()].into());
                }
                names.push(name.to_string());
            } else {
                // A quoted or computed `NAME=value`.
                let mut values: BTreeMap<String, Values> = BTreeMap::new();
                for field in self.fields(word, st).into_iter().flatten() {
                    match field.split_once('=') {
                        Some((name, value)) if is_name(name) => {
                            values
                                .entry(name.to_string())
                                .or_default()
                                .insert(value.to_string());
                        }
                        _ if is_name(&field) => names.push(field),
                        _ => {
                            st.go_wild(why::NAME);
                            return;
                        }
                    }
                }
                for (name, values) in values {
                    if local {
                        st.declare_local(&name);
                    }
                    st.set(&name, values);
                    names.push(name);
                }
            }
            for name in names {
                if array {
                    st.arrays.insert(name.clone());
                }
                if case || unmodelled {
                    let reason = if case { why::CASE } else { why::DECLARE };
                    st.sticky.insert(name.clone(), reason);
                    st.set(&name, Values::new());
                }
                if readonly {
                    st.readonly.insert(name);
                }
            }
        }
    }

    /// `${NAME:=word}` and `${NAME=word}` store the word they expand to.
    fn side_effects(&mut self, words: &[Word], st: &mut State) {
        for word in words {
            self.part_effects(&word.parts, st);
        }
    }

    fn part_effects(&mut self, parts: &[Part], st: &mut State) {
        for part in parts {
            match part {
                Part::Param {
                    name,
                    op: Some(op @ ParamOp::Default { word, assign }),
                    ..
                } => {
                    self.part_effects(word, st);
                    if *assign && is_name(name) {
                        let values = self.param_values(name, Some(op), st);
                        st.set(name, values);
                    }
                }
                Part::Param {
                    op: Some(ParamOp::Alternate(word)),
                    ..
                } => self.part_effects(word, st),
                Part::Array(words) => self.side_effects(words, st),
                _ => {}
            }
        }
    }

    /// `read [OPTIONS] NAME…`: each name takes a field of the first line of
    /// its input, split on `IFS`, and the last takes the rest.
    fn read(&mut self, argv: &[String], input: Option<&Fed>, st: &mut State) {
        let mut at = 1;
        let mut unmodelled = false;
        let mut raw = false;
        let mut array: Option<String> = None;
        while let Some(arg) = argv.get(at) {
            if arg == "--" {
                at += 1;
                break;
            }
            if !arg.starts_with('-') || arg == "-" {
                break;
            }
            let flags = &arg[1..];
            let mut next_taken = false;
            for (i, c) in flags.char_indices() {
                match c {
                    'r' => raw = true,
                    's' | 'e' => {}
                    'p' | 't' | 'i' | 'u' | 'd' | 'n' | 'N' | 'a' => {
                        let value = if i + 1 < flags.len() {
                            flags[i + 1..].to_string()
                        } else {
                            next_taken = true;
                            argv.get(at + 1).cloned().unwrap_or_default()
                        };
                        match c {
                            'a' => array = Some(value),
                            'u' | 'd' | 'n' | 'N' => unmodelled = true,
                            _ => {}
                        }
                        break;
                    }
                    _ => unmodelled = true,
                }
            }
            at += if next_taken { 2 } else { 1 };
        }
        let mut names: Vec<String> = argv.get(at..).unwrap_or_default().to_vec();
        if names.is_empty() && array.is_none() {
            names.push("REPLY".to_string());
        }
        if names.iter().chain(&array).any(|n| !is_name(n)) {
            st.go_wild(why::NAME);
            return;
        }
        let fed = self.pipe_input.clone().or_else(|| input.cloned());
        let modes = ifs_modes(st);
        let reason = if unmodelled {
            Some(why::READ_OPTION)
        } else if !fed.as_ref().is_some_and(|f| f.complete) {
            Some(why::READ_INPUT)
        } else if modes.iter().any(Result::is_err) {
            Some(why::IFS)
        } else {
            None
        };
        if let Some(reason) = reason {
            for name in names.iter().chain(&array) {
                st.set(name, [taint(reason)].into());
            }
            if let Some(array) = array {
                st.arrays.insert(array);
            }
            return;
        }
        let items = fed.map(|f| f.items).unwrap_or_default();
        let mut per_name: Vec<Values> = vec![Values::new(); names.len()];
        let mut elements = Values::new();
        for item in &items {
            let line = item.value.split('\n').next().unwrap_or_default();
            let mut lines = vec![line.to_string()];
            if !raw && line.contains('\\') {
                lines.push(line.replace('\\', ""));
            }
            for line in &lines {
                for ifs in modes.iter().filter_map(|m| m.as_ref().ok()) {
                    for (i, values) in read_fields(line, ifs, names.len()).into_iter().enumerate() {
                        per_name[i].extend(values);
                    }
                    elements.extend(split_ifs(line, ifs));
                }
            }
        }
        for (name, mut values) in names.iter().zip(per_name) {
            if values.is_empty() {
                values.insert(String::new());
            }
            st.set(name, values);
        }
        if let Some(array) = array {
            if elements.is_empty() {
                elements.insert(String::new());
            }
            st.arrays.insert(array.clone());
            st.set(&array, elements);
        }
    }

    /// Run one concrete command: unwrap its launchers and judge what it
    /// deletes. `forced` marks a remover given a whole tree.
    fn exec(
        &mut self,
        argv: &[String],
        st: &State,
        input: Option<&Fed>,
        forced: bool,
        ambiguous: bool,
    ) {
        if self.done() || self.depth > MAX_DEPTH {
            return;
        }
        let Some(run) = unwrap(argv) else {
            return;
        };
        let mut st = st.clone();
        for (name, value) in &run.env {
            st.set(name, [value.clone()].into());
        }
        if let Some(dir) = &run.cwd {
            let cwd = st.cwd.iter().map(|cwd| resolve(cwd, dir)).collect();
            st.set_cwd(cwd);
        }
        let argv = run.argv;
        let Some(first) = argv.first() else {
            return;
        };
        let ambiguous = ambiguous || st.cwd.len() > 1;
        match program_name(first).as_str() {
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" | "fish" | "mksh" => {
                // A new shell: its own arguments, no function calls.
                let mut child = st.clone();
                child.scopes.clear();
                if let Some((script, rest)) = shell_script(&argv[1..]) {
                    if let Some(name) = rest.first() {
                        child.arg0 = [name.clone()].into();
                    }
                    child.args = positionals(&[rest.to_vec()], 1);
                    let script = script.to_string();
                    self.depth += 1;
                    self.child_script(&script, &mut child);
                    self.depth -= 1;
                } else if let Some(input) = input {
                    let script: Vec<&str> = input.items.iter().map(|i| i.value.as_str()).collect();
                    child.args = Vec::new();
                    self.child_script(&script.join("\n"), &mut child);
                }
            }
            "eval" => {
                let mut child = st.clone();
                self.child_script(&argv[1..].join(" "), &mut child);
            }
            "rm" => self.judge_rm(&argv, &st, forced, ambiguous),
            "rmdir" | "unlink" | "shred" | "srm" if forced => {
                self.judge_rm(&argv, &st, true, ambiguous);
            }
            "find" => self.judge_find(&argv[1..], &st, ambiguous),
            "xargs" => self.judge_xargs(&argv[1..], &st, input, ambiguous),
            "parallel" => self.judge_parallel(&argv[1..], &st, input, ambiguous),
            "rsync" => self.judge_rsync(&argv[1..], &st, ambiguous),
            _ => {}
        }
    }

    fn judge_rm(&mut self, words: &[String], st: &State, forced: bool, ambiguous: bool) {
        let args = &words[1..];
        let operands: Vec<&str> = match recursive_rm_operands(args) {
            Some(operands) => operands,
            None if forced => args
                .iter()
                .filter(|a| !a.starts_with('-'))
                .map(String::as_str)
                .collect(),
            None => return,
        };
        self.judge_paths(operands, st, ambiguous);
    }

    /// `find`: a deletion over a protected start is refused whatever its
    /// tests (`-name` selects within the tree, it never proves the tree
    /// safe), and each command it runs is judged on its own with `{}` as
    /// each start.
    fn judge_find(&mut self, args: &[String], st: &State, ambiguous: bool) {
        let mut at = 0;
        while let Some(arg) = args.get(at) {
            match arg.as_str() {
                "-H" | "-L" | "-P" => at += 1,
                "-D" => at += 2,
                _ if arg.starts_with("-O") => at += 1,
                _ => break,
            }
        }
        let rest = args.get(at..).unwrap_or_default();
        let starts_end = rest
            .iter()
            .position(|a| a.starts_with('-') || matches!(a.as_str(), "(" | "!" | ","))
            .unwrap_or(rest.len());
        let (starts, expression) = rest.split_at(starts_end);
        let dot = [".".to_string()];
        let starts = if starts.is_empty() { &dot[..] } else { starts };
        let bodies = exec_bodies(expression);
        let deletes = expression.iter().any(|a| a == "-delete")
            || bodies.iter().any(|body| runs_remover(body, 0));
        if deletes {
            self.judge_paths(starts.iter().map(String::as_str), st, ambiguous);
            if self.done() {
                return;
            }
        }
        for body in bodies {
            for start in starts {
                let line: Vec<String> = body.iter().map(|w| w.replace("{}", start)).collect();
                self.exec(&line, st, None, false, ambiguous);
            }
        }
    }

    /// `xargs CMD`: the command with each input value appended, or put in
    /// place of its `-I` token, and the command alone.
    fn judge_xargs(&mut self, args: &[String], st: &State, input: Option<&Fed>, ambiguous: bool) {
        let (inner, replace, file_input) = after_xargs_options(args);
        if inner.is_empty() {
            return;
        }
        self.exec(inner, st, None, false, ambiguous);
        let inputs = if file_input { None } else { input };
        for item in items(inputs) {
            let line: Vec<String> = match &replace {
                Some(token) => inner
                    .iter()
                    .map(|w| w.replace(token.as_str(), &item.value))
                    .collect(),
                None => inner.iter().cloned().chain([item.value.clone()]).collect(),
            };
            self.exec(&line, st, None, item.tree, ambiguous);
        }
    }

    /// GNU `parallel CMD ::: ARGS`, or `parallel CMD` fed like `xargs`.
    fn judge_parallel(
        &mut self,
        args: &[String],
        st: &State,
        input: Option<&Fed>,
        ambiguous: bool,
    ) {
        const WITH_VALUE: &[&str] = &[
            "-j",
            "--jobs",
            "-S",
            "--sshlogin",
            "--joblog",
            "-a",
            "--arg-file",
            "--colsep",
            "-d",
            "--delimiter",
            "--results",
            "--tmpdir",
            "--workdir",
            "-I",
            "--timeout",
            "--delay",
        ];
        let mut at = 0;
        while let Some(arg) = args.get(at) {
            if arg == "--" {
                at += 1;
                break;
            }
            if !arg.starts_with('-') || arg.starts_with(":::") {
                break;
            }
            at += if WITH_VALUE.contains(&arg.as_str()) {
                2
            } else {
                1
            };
        }
        let rest = args.get(at..).unwrap_or_default();
        let split = rest
            .iter()
            .position(|a| a.starts_with(":::"))
            .unwrap_or(rest.len());
        let (command, sources) = rest.split_at(split);
        if command.is_empty() {
            return;
        }
        let mut entries: Vec<Input> = sources
            .iter()
            .filter(|a| !a.starts_with(":::"))
            .map(|value| Input {
                value: value.clone(),
                tree: false,
            })
            .collect();
        if sources.is_empty() {
            entries.extend(items(input));
        }
        self.exec(command, st, None, false, ambiguous);
        for item in entries {
            let line: Vec<String> = if command.iter().any(|w| w.contains("{}")) {
                command
                    .iter()
                    .map(|w| w.replace("{}", &item.value))
                    .collect()
            } else {
                command
                    .iter()
                    .cloned()
                    .chain([item.value.clone()])
                    .collect()
            };
            self.exec(&line, st, None, item.tree, ambiguous);
        }
    }

    /// `rsync --delete…` deletes in its destination, and
    /// `--remove-source-files` in its sources.
    fn judge_rsync(&mut self, args: &[String], st: &State, ambiguous: bool) {
        const WITH_VALUE: &[&str] = &[
            "-e",
            "--rsh",
            "-f",
            "--filter",
            "--exclude",
            "--include",
            "--exclude-from",
            "--include-from",
            "--files-from",
            "--password-file",
            "--log-file",
            "--partial-dir",
            "-T",
            "--temp-dir",
            "--backup-dir",
            "--suffix",
            "--chmod",
            "--chown",
            "--usermap",
            "--groupmap",
            "--timeout",
            "--contimeout",
            "-B",
            "--block-size",
            "--max-size",
            "--min-size",
            "--max-delete",
            "--bwlimit",
            "--compare-dest",
            "--copy-dest",
            "--link-dest",
            "--out-format",
            "--port",
            "--sockopts",
            "--iconv",
            "-M",
            "--remote-option",
        ];
        let mut deletes_destination = false;
        let mut removes_sources = false;
        let mut operands = Vec::new();
        let mut at = 0;
        let mut only_operands = false;
        while let Some(arg) = args.get(at) {
            at += 1;
            if only_operands || !arg.starts_with('-') || arg == "-" {
                operands.push(arg.as_str());
                continue;
            }
            if arg == "--" {
                only_operands = true;
            } else if arg.starts_with("--del") {
                deletes_destination = true;
            } else if arg == "--remove-source-files" {
                removes_sources = true;
            } else if WITH_VALUE.contains(&arg.as_str()) {
                at += 1;
            }
        }
        let Some((destination, sources)) = operands.split_last() else {
            return;
        };
        let local = |path: &str| {
            !path.starts_with("rsync://") && !path.split('/').next().unwrap_or("").contains(':')
        };
        let mut judged: Vec<&str> = Vec::new();
        if deletes_destination && operands.len() > 1 && local(destination) {
            judged.push(destination);
        }
        if removes_sources {
            judged.extend(sources.iter().copied().filter(|s| local(s)));
        }
        self.judge_paths(judged, st, ambiguous);
    }

    /// How `operand` reads from every working directory the line may be
    /// in: protected by its spelling, where it really lands, or where a
    /// glob in it may land; unproven when it holds an unknown value or is
    /// relative to an unknown directory.
    fn judge_path(&self, operand: &str, st: &State) -> Judged {
        if operand.is_empty() {
            return Judged::Clear;
        }
        if let Some(reason) = unproven(operand) {
            return Judged::Unproven(Unproven {
                reason: reason.to_string(),
                cwd: false,
            });
        }
        let relative = !operand.starts_with(['/', '~', '$']);
        let mut pending = None;
        for cwd in &st.cwd {
            if relative {
                if let Some(reason) = unproven(cwd) {
                    pending.get_or_insert_with(|| Unproven {
                        reason: reason.to_string(),
                        cwd: true,
                    });
                    continue;
                }
            }
            let path = resolve(cwd, operand);
            let landed = |path: &str| {
                dangerous_rm_target(path).or_else(|| {
                    self.real_path(path)
                        .and_then(|real| dangerous_rm_target(&real))
                })
            };
            if let Some(target) = landed(&path) {
                return Judged::Protected(target);
            }
            match self.glob_paths(&path) {
                Some(paths) => {
                    if let Some(target) = paths.iter().find_map(|p| landed(p)) {
                        return Judged::Protected(target);
                    }
                }
                None => {
                    pending.get_or_insert_with(|| Unproven {
                        reason: why::GLOB.to_string(),
                        cwd: false,
                    });
                }
            }
        }
        pending.map_or(Judged::Clear, Judged::Unproven)
    }

    /// `path` as an absolute path; `None` for one the reader cannot place.
    fn absolute(&self, path: &str) -> Option<PathBuf> {
        if path.contains(['$', '`']) {
            return None;
        }
        if let Some(rest) = path.strip_prefix('~') {
            if !(rest.is_empty() || rest.starts_with('/')) {
                return None;
            }
            let home = std::env::var_os("HOME")?;
            Some(PathBuf::from(format!("{}{rest}", home.to_string_lossy())))
        } else if path.starts_with('/') {
            Some(PathBuf::from(path))
        } else {
            Some(self.base?.join(path))
        }
    }

    /// The real path of `path`'s longest existing prefix with the rest
    /// appended; `None` for a path the reader cannot place.
    fn real_path(&self, path: &str) -> Option<String> {
        real_prefix(&self.absolute(path)?)
    }

    /// Every path a glob in `path` may name: its matches on disk, and the
    /// protected names it could match where the disk does not show them.
    /// A final `*` is kept, since the protected classification reads
    /// `dir/*` itself. Empty for a path without a glob or one the reader
    /// cannot place; `None` when a directory cannot be listed or there are
    /// too many matches.
    fn glob_paths(&self, path: &str) -> Option<Vec<String>> {
        if !path.contains(['*', '?', '[']) {
            return Some(Vec::new());
        }
        let Some(absolute) = self.absolute(path) else {
            return Some(Vec::new());
        };
        let absolute = absolute.to_string_lossy().into_owned();
        // `*/` names directories through links, so only a bare final `*`
        // is kept.
        let keep_last = !absolute.ends_with('/');
        let parts: Vec<&str> = absolute.split('/').filter(|p| !p.is_empty()).collect();
        let mut dirs = vec![String::new()];
        for (i, part) in parts.iter().enumerate() {
            let globbed = part.contains(['*', '?', '[']);
            let mut next = Vec::new();
            for dir in &dirs {
                if !globbed || (keep_last && i + 1 == parts.len() && *part == "*") {
                    next.push(format!("{dir}/{part}"));
                    continue;
                }
                let listed = if dir.is_empty() { "/" } else { dir.as_str() };
                let mut names: BTreeSet<String> = protected_names(listed)
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                match std::fs::read_dir(listed) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            names.insert(entry.file_name().to_string_lossy().into_owned());
                        }
                    }
                    Err(_) if !Path::new(listed).is_dir() => {}
                    Err(_) => return None,
                }
                next.extend(
                    names
                        .into_iter()
                        .filter(|name| glob_match(part, name))
                        .map(|name| format!("{dir}/{name}")),
                );
            }
            if next.len() > MAX_GLOB {
                return None;
            }
            dirs = next;
        }
        if !keep_last {
            for dir in &mut dirs {
                dir.push('/');
            }
        }
        Some(dirs)
    }

    // --- words ------------------------------------------------------------

    /// Concrete argument vectors for `words`: every combination while there
    /// are few, else each alternative of each word at least once.
    fn argv_variants(&mut self, words: &[Word], st: &State) -> Vec<Vec<String>> {
        let per_word: Vec<Vec<Vec<String>>> = words.iter().map(|w| self.fields(w, st)).collect();
        let product = per_word
            .iter()
            .map(Vec::len)
            .try_fold(1usize, |acc, n| acc.checked_mul(n.max(1)))
            .unwrap_or(usize::MAX);
        let count = if product <= MAX_VARIANTS {
            product
        } else {
            per_word
                .iter()
                .map(Vec::len)
                .max()
                .unwrap_or(1)
                .min(MAX_VARIANTS * 4)
        };
        let mut variants = Vec::with_capacity(count);
        for index in 0..count {
            let mut argv = Vec::new();
            let mut rest = index;
            for alternatives in &per_word {
                if alternatives.is_empty() {
                    continue;
                }
                let pick = if product <= MAX_VARIANTS {
                    let pick = rest % alternatives.len();
                    rest /= alternatives.len();
                    pick
                } else {
                    index % alternatives.len()
                };
                argv.extend(alternatives[pick].iter().cloned());
            }
            variants.push(argv);
        }
        variants
    }

    /// The alternatives for one word, each a list of fields after brace
    /// expansion and splitting on `IFS`.
    fn fields(&mut self, word: &Word, st: &State) -> Vec<Vec<String>> {
        let mut alternatives: Vec<Vec<(String, Kind)>> = vec![Vec::new()];
        let mut truncated = false;
        for part in &word.parts {
            let options = self.part_values(part, st);
            let mut next = Vec::new();
            'grow: for alternative in &alternatives {
                for option in &options {
                    if next.len() >= MAX_VALUES {
                        truncated = true;
                        break 'grow;
                    }
                    let mut extended = alternative.clone();
                    extended.push(option.clone());
                    next.push(extended);
                }
            }
            alternatives = next;
        }
        let modes = ifs_modes(st);
        let mut out = Vec::new();
        for pieces in alternatives {
            for braced in brace_expand(&pieces) {
                for mode in &modes {
                    let fields = split_fields(&braced, mode);
                    if !out.contains(&fields) {
                        out.push(fields);
                    }
                }
            }
        }
        if out.len() > MAX_VALUES {
            out.truncate(MAX_VALUES);
            truncated = true;
        }
        if truncated {
            out.push(vec![taint(why::MANY)]);
        }
        out
    }

    /// The values of an assignment's right-hand side: no splitting; an
    /// array holds each of its elements.
    fn value(&mut self, parts: &[Part], st: &State) -> Values {
        if let [Part::Array(elements)] = parts {
            let mut values = Values::new();
            for element in elements {
                // `[key]=value` gives the value.
                let element = element
                    .parts
                    .first()
                    .and_then(|part| match part {
                        Part::Lit {
                            text,
                            quoted: false,
                        } if text.starts_with('[') => text.find("]=").map(|end| (end, text)),
                        _ => None,
                    })
                    .map_or_else(
                        || element.clone(),
                        |(end, text)| {
                            let mut parts = vec![Part::Lit {
                                text: text[end + 2..].to_string(),
                                quoted: false,
                            }];
                            parts.extend(element.parts[1..].iter().cloned());
                            Word {
                                parts,
                                src: String::new(),
                            }
                        },
                    );
                values.extend(self.fields(&element, st).into_iter().flatten());
            }
            if values.is_empty() {
                values.insert(String::new());
            }
            return bounded(values);
        }
        let mut values: Vec<String> = vec![String::new()];
        let mut truncated = false;
        for part in parts {
            let options = self.part_values(part, st);
            let mut next = Vec::new();
            for value in &values {
                for (text, _) in &options {
                    next.push(format!("{value}{}", text.replace(FIELD_BREAK, " ")));
                }
            }
            if next.len() > MAX_VALUES {
                next.truncate(MAX_VALUES);
                truncated = true;
            }
            values = next;
        }
        if truncated {
            values.push(taint(why::MANY));
        }
        values.into_iter().collect()
    }

    /// Each value a part may take, with how the shell treats it.
    fn part_values(&mut self, part: &Part, st: &State) -> Vec<(String, Kind)> {
        match part {
            Part::Lit { text, quoted } => {
                let kind = if *quoted { Kind::Quoted } else { Kind::Bare };
                vec![(text.clone(), kind)]
            }
            Part::Opaque { text, unknown, .. } => {
                // A substitution inside arithmetic still runs.
                if text.contains("$(") || text.contains('`') {
                    let mut lexer = Lexer::new(text.trim_start_matches('$'));
                    let mut inner = Word::default();
                    while lexer.peek(0).is_some() {
                        lexer.word_char(&mut inner, false);
                    }
                    for part in &inner.parts {
                        if let Part::Subst { script, .. } = part {
                            let mut child = st.clone();
                            self.child_script(script, &mut child);
                        }
                    }
                }
                vec![(unknown.map_or_else(|| text.clone(), taint), Kind::Quoted)]
            }
            Part::Tilde(user) => match user.as_str() {
                "+" => st
                    .cwd
                    .iter()
                    .map(|c| (display_dir(c), Kind::Quoted))
                    .collect(),
                "-" => st
                    .oldpwd
                    .iter()
                    .map(|c| (display_dir(c), Kind::Quoted))
                    .collect(),
                // `~` is `$HOME`, which the line may set; any other user's
                // home is a home directory.
                "" => st
                    .var("HOME")
                    .into_iter()
                    .map(|home| (home, Kind::Quoted))
                    .collect(),
                _ => vec![("~".to_string(), Kind::Quoted)],
            },
            Part::Param { name, op, quoted } => {
                let kind = if *quoted { Kind::Quoted } else { Kind::Split };
                if name == "*" && *quoted {
                    // `"$*"` joins the arguments with the first `IFS`
                    // character.
                    let joined = self.param_values(name, op.as_ref(), st);
                    let mut out = Vec::new();
                    for ifs in st.var("IFS") {
                        let sep = if unproven(&ifs).is_some() || ifs.contains('$') {
                            taint(why::IFS)
                        } else {
                            ifs.chars().next().map(String::from).unwrap_or_default()
                        };
                        for value in &joined {
                            out.push((value.replace(FIELD_BREAK, &sep), kind));
                        }
                    }
                    return out;
                }
                let values = self.param_values(name, op.as_ref(), st);
                values.into_iter().map(|v| (v, kind)).collect()
            }
            Part::Subst { script, quoted } => {
                let kind = if *quoted { Kind::Quoted } else { Kind::Split };
                match self.substitution(script, st) {
                    Some(outputs) => outputs.into_iter().map(|v| (v, kind)).collect(),
                    None => vec![(format!("$({script})"), Kind::Quoted)],
                }
            }
            Part::Array(elements) => {
                let text: Vec<String> = elements.iter().map(word_text).collect();
                vec![(format!("({})", text.join(" ")), Kind::Quoted)]
            }
        }
    }

    /// Read a substitution in a child shell, judging what it deletes, and
    /// give what it prints when the reader can tell: its last command is
    /// `echo`, `printf` or `pwd`, read after the commands before it.
    fn substitution(&mut self, script: &str, st: &State) -> Option<Values> {
        let mut child = st.clone();
        if self.depth > MAX_DEPTH {
            self.child_script(script, &mut child);
            return None;
        }
        let mut items = Vec::new();
        flatten(Parser::parse(script), &mut items);
        // The last command, and whether it may not run (after `&&`).
        let (last, optional) = match items.pop() {
            Some(Node::Simple(words, redirs)) => ((words, redirs), false),
            Some(Node::AndOr(mut chain)) => match chain.pop() {
                Some(Node::Simple(words, redirs)) => {
                    items.push(if chain.len() == 1 {
                        chain.remove(0)
                    } else {
                        Node::AndOr(chain)
                    });
                    ((words, redirs), true)
                }
                Some(other) => {
                    chain.push(other);
                    items.push(Node::AndOr(chain));
                    self.child_run(&Node::Seq(items), &mut child);
                    return None;
                }
                None => return None,
            },
            Some(other) => {
                items.push(other);
                self.child_run(&Node::Seq(items), &mut child);
                return None;
            }
            None => return Some([String::new()].into()),
        };
        self.depth += 1;
        self.child_run(&Node::Seq(items), &mut child);
        let printed = if child.dead {
            None
        } else {
            self.printed(&last.0, &child)
        };
        self.child_run(&Node::Simple(last.0, last.1), &mut child);
        self.depth -= 1;
        printed.map(|mut out| {
            if optional {
                out.insert(String::new());
            }
            out
        })
    }

    /// Read `node` in a child shell of the line.
    fn child_run(&mut self, node: &Node, child: &mut State) {
        self.frame(FrameKind::Child, node, child);
    }

    /// What `echo`, `printf` or `pwd` with `words` prints; trailing
    /// newlines are dropped, as a substitution does.
    fn printed(&mut self, words: &[Word], st: &State) -> Option<Values> {
        let mut out = Values::new();
        for argv in self.argv_variants(words, st) {
            let run = unwrap(&argv)?;
            let argv = run.argv;
            match program_name(argv.first()?).as_str() {
                "echo" => {
                    let words: Vec<&str> = argv[1..]
                        .iter()
                        .skip_while(|a| matches!(a.as_str(), "-n" | "-e" | "-E"))
                        .map(String::as_str)
                        .collect();
                    out.insert(words.join(" "));
                }
                "printf" => {
                    out.insert(printf(&argv[1..])?);
                }
                "pwd" => out.extend(st.cwd.iter().map(|c| display_dir(c))),
                _ => return None,
            }
        }
        Some(
            out.into_iter()
                .map(|v| v.trim_end_matches('\n').to_string())
                .collect(),
        )
    }

    /// The values a positional parameter, `$@` or `$*` may take; the
    /// arguments of `$@` and `$*` are joined by [`FIELD_BREAK`], with each
    /// argument's values covered at least once.
    fn positional_values(name: &str, st: &State) -> Values {
        if let Some(reason) = st.wild {
            return [taint(reason)].into();
        }
        if name == "0" {
            return st.arg0.clone();
        }
        if let Ok(n) = name.parse::<usize>() {
            return st
                .args
                .get(n.saturating_sub(1))
                .cloned()
                .unwrap_or_else(|| [String::new()].into());
        }
        let columns: Vec<Vec<&String>> = st.args.iter().map(|v| v.iter().collect()).collect();
        let rows = columns.iter().map(Vec::len).max().unwrap_or(1).max(1);
        (0..rows)
            .map(|row| {
                let fields: Vec<&str> = columns
                    .iter()
                    .map(|column| column[row % column.len()].as_str())
                    .collect();
                fields.join(&FIELD_BREAK.to_string())
            })
            .collect()
    }

    fn param_values(&mut self, name: &str, op: Option<&ParamOp>, st: &State) -> Values {
        let values = if is_name(name) {
            st.var(name)
        } else {
            Self::positional_values(name, st)
        };
        let unknown = |v: &str| spelled(v) || unproven(v).is_some();
        match op {
            None => values,
            Some(ParamOp::Required) => values.into_iter().filter(|v| !v.is_empty()).collect(),
            Some(ParamOp::Default { word, .. }) => {
                let fallback = self.value(word, st);
                let mut out = Values::new();
                for v in values {
                    if v.is_empty() {
                        out.extend(fallback.iter().cloned());
                    } else {
                        if unknown(&v) {
                            out.extend(fallback.iter().cloned());
                        }
                        out.insert(v);
                    }
                }
                out
            }
            Some(ParamOp::Alternate(word)) => {
                let alternate = self.value(word, st);
                let mut out = Values::new();
                for v in values {
                    if v.is_empty() {
                        out.insert(String::new());
                    } else {
                        if unknown(&v) {
                            out.insert(String::new());
                        }
                        out.extend(alternate.iter().cloned());
                    }
                }
                out
            }
            Some(ParamOp::Strip {
                suffix,
                longest,
                pattern,
            }) => values
                .into_iter()
                .map(|v| {
                    if let Some(reason) = unproven(&v) {
                        taint(reason)
                    } else if spelled(&v) {
                        format!("${{{name}}}")
                    } else {
                        strip(&v, pattern, *suffix, *longest)
                    }
                })
                .collect(),
        }
    }

    /// Input a heredoc, here-string or file gives the command: all of it
    /// for a heredoc or here-string, none the reader can see for a file.
    fn redirect_input(&mut self, redirs: &[Redir], st: &State) -> Option<Fed> {
        let mut fed: Option<Fed> = None;
        for redir in redirs {
            let mut add = |items: Vec<Input>, complete: bool| {
                let entry = fed.get_or_insert(Fed {
                    items: Vec::new(),
                    complete: true,
                });
                entry.items.extend(items);
                entry.complete &= complete;
            };
            if let Some(body) = &redir.body {
                let values: Vec<String> = if redir.expand {
                    self.value(&heredoc_word(body).parts, st)
                        .into_iter()
                        .collect()
                } else {
                    vec![body.clone()]
                };
                add(
                    values
                        .into_iter()
                        .map(|value| Input { value, tree: false })
                        .collect(),
                    true,
                );
            } else if redir.op == "<<<" {
                let mut items = Vec::new();
                if let Some(target) = &redir.target {
                    for fields in self.fields(target, st) {
                        items.push(Input {
                            value: fields.join(" "),
                            tree: false,
                        });
                    }
                }
                add(items, true);
            } else if let Some(target) = &redir.target {
                // A redirection target can hold a substitution that runs.
                let _ = self.fields(target, st);
                if redir.op.starts_with('<') {
                    add(Vec::new(), false);
                }
            }
        }
        fed
    }

    /// What `node` writes to its output, as far as the reader can tell: a
    /// `find` lists its trees, `ls` its directories' entries, `echo` and
    /// `printf` their words; anything else makes the output incomplete.
    fn producer_values(&mut self, node: &Node, st: &State) -> Fed {
        let mut out = Vec::new();
        let mut complete = true;
        let mut simples = Vec::new();
        collect_simples(node, &mut simples);
        for (words, redirs) in simples {
            let variants = self.argv_variants(&words, st);
            for argv in variants {
                let Some(run) = unwrap(&argv) else {
                    continue;
                };
                let argv = run.argv;
                let Some(first) = argv.first() else {
                    continue;
                };
                let operands = argv[1..].iter().filter(|a| !a.starts_with('-'));
                match program_name(first).as_str() {
                    "ls" => {
                        let dirs: Vec<&String> = operands.collect();
                        if dirs.is_empty() {
                            out.push(Input {
                                value: "*".to_string(),
                                tree: false,
                            });
                        }
                        for dir in dirs {
                            out.push(Input {
                                value: format!("{}/*", dir.trim_end_matches('/')),
                                tree: false,
                            });
                        }
                    }
                    "find" => {
                        let starts: Vec<&String> = argv[1..]
                            .iter()
                            .skip_while(|a| matches!(a.as_str(), "-H" | "-L" | "-P"))
                            .take_while(|a| {
                                !a.starts_with('-') && !matches!(a.as_str(), "(" | "!" | ",")
                            })
                            .collect();
                        if starts.is_empty() {
                            out.push(Input {
                                value: ".".to_string(),
                                tree: true,
                            });
                        }
                        for start in starts {
                            out.push(Input {
                                value: start.clone(),
                                tree: true,
                            });
                        }
                    }
                    // A line of text: `xargs` splits it, `read` and a shell
                    // read it whole.
                    "echo" => {
                        let words: Vec<&str> = operands.map(String::as_str).collect();
                        out.push(Input {
                            value: words.join(" "),
                            tree: false,
                        });
                    }
                    "printf" => match printf(&argv[1..]) {
                        Some(value) => out.push(Input { value, tree: false }),
                        None => complete = false,
                    },
                    // `cat` with no file passes its heredoc or here-string on.
                    "cat" if argv.len() == 1 => match self.redirect_input(&redirs, st) {
                        Some(fed) => {
                            out.extend(fed.items);
                            complete &= fed.complete;
                        }
                        None => complete = false,
                    },
                    "true" | "false" | ":" => {}
                    _ => complete = false,
                }
            }
        }
        if out.len() > MAX_VALUES {
            out.truncate(MAX_VALUES);
            complete = false;
        }
        Fed {
            items: out,
            complete,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// How the shell treats a piece of a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Unquoted literal text: brace expansion applies.
    Bare,
    /// Quoted text, or an expansion inside quotes: taken as it is.
    Quoted,
    /// An unquoted expansion: split on `IFS`.
    Split,
}

/// How each `IFS` value `st` may hold splits a field: its characters, or
/// unknown.
fn ifs_modes(st: &State) -> Vec<Result<String, &'static str>> {
    st.var("IFS")
        .into_iter()
        .map(|ifs| {
            if unproven(&ifs).is_some() || ifs.contains('$') {
                Err(why::IFS)
            } else {
                Ok(ifs)
            }
        })
        .collect()
}

/// The builtins that change this shell, which the reader reads in it.
fn is_builtin(program: &str, variants: &[Vec<String>]) -> bool {
    match program {
        "printf" => variants
            .iter()
            .any(|argv| argv.get(1).is_some_and(|a| a == "-v")),
        _ => matches!(
            program,
            "break"
                | "continue"
                | "return"
                | "exit"
                | "cd"
                | "pushd"
                | "popd"
                | "chdir"
                | "export"
                | "readonly"
                | "local"
                | "declare"
                | "typeset"
                | "unset"
                | "read"
                | "mapfile"
                | "readarray"
                | "getopts"
                | "set"
                | "shift"
                | "eval"
                | "source"
                | "."
                | "alias"
                | "unalias"
                | "trap"
        ),
    }
}

/// The words before a command that still run it in this shell (`command`,
/// `builtin`, `time`, zsh's `noglob`), and whether a function may still be
/// called; `None` for `command -v`, which only prints.
fn builtin_prefix(words: &[Word]) -> Option<(usize, bool)> {
    let mut at = 0;
    let mut functions = true;
    loop {
        match words.get(at).and_then(Word::plain) {
            Some("builtin") => {
                at += 1;
                functions = false;
            }
            Some("command") => {
                at += 1;
                functions = false;
                while let Some(option) = words
                    .get(at)
                    .and_then(Word::plain)
                    .filter(|o| o.len() > 1 && o.starts_with('-'))
                {
                    at += 1;
                    if option == "--" {
                        break;
                    }
                    if option.contains(['v', 'V']) {
                        return None;
                    }
                }
            }
            Some("time") => {
                at += 1;
                if words.get(at).and_then(Word::plain) == Some("-p") {
                    at += 1;
                }
            }
            Some("noglob" | "nocorrect") => at += 1,
            _ => return Some((at, functions)),
        }
    }
}

/// The fields `read` gives `count` names from `line`: each name a field,
/// the last the rest of the line (with and without one trailing
/// delimiter, which bash drops).
fn read_fields(line: &str, ifs: &str, count: usize) -> Vec<Values> {
    let space = |c: char| ifs.contains(c) && c.is_whitespace();
    let delim = |c: char| ifs.contains(c);
    let mut rest = line.trim_matches(space);
    let mut out: Vec<Values> = Vec::new();
    while out.len() + 1 < count {
        match rest.find(delim) {
            None => {
                out.push([rest.to_string()].into());
                rest = "";
            }
            Some(end) => {
                out.push([rest[..end].to_string()].into());
                let after = rest[end..].trim_start_matches(space);
                let after = match after.chars().next() {
                    Some(c) if delim(c) && !c.is_whitespace() && rest[end..].starts_with(c) => {
                        &after[c.len_utf8()..]
                    }
                    _ => after,
                };
                rest = after.trim_start_matches(space);
            }
        }
    }
    let mut last: Values = [rest.to_string()].into();
    if let Some(c) = rest.chars().last().filter(|c| delim(*c)) {
        last.insert(rest[..rest.len() - c.len_utf8()].to_string());
    }
    out.push(last);
    out.truncate(count);
    while out.len() < count {
        out.push([String::new()].into());
    }
    out
}

/// `text` split on `ifs` into its non-empty fields.
fn split_ifs(text: &str, ifs: &str) -> Vec<String> {
    text.split(|c: char| ifs.contains(c))
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect()
}

/// A heredoc body read as the shell expands it: parameters and
/// substitutions, as inside double quotes.
fn heredoc_word(body: &str) -> Word {
    let mut lexer = Lexer::new(body);
    let mut word = Word::default();
    while let Some(ch) = lexer.peek(0) {
        match ch {
            '\\' if matches!(lexer.peek(1), Some('$' | '`' | '\\' | '\n')) => {
                if let Some(next) = lexer.peek(1).filter(|c| *c != '\n') {
                    word.push_lit(next, true);
                }
                lexer.at += 2;
            }
            '$' => lexer.dollar(&mut word, true),
            '`' => {
                let script = lexer.backtick();
                word.parts.push(Part::Subst {
                    script,
                    quoted: true,
                });
            }
            _ => {
                word.push_lit(ch, true);
                lexer.at += 1;
            }
        }
    }
    word
}

/// The items `xargs` or `parallel` reads from its input: each value split
/// on blanks and NUL.
fn items(fed: Option<&Fed>) -> Vec<Input> {
    let Some(fed) = fed else {
        return Vec::new();
    };
    fed.items
        .iter()
        .flat_map(|item| {
            item.value
                .split(|c: char| c.is_whitespace() || c == '\0')
                .filter(|v| !v.is_empty())
                .map(|value| Input {
                    value: value.to_string(),
                    tree: item.tree,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A command after its launchers, with the environment and working
/// directory they set.
struct Unwrapped {
    argv: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
}

/// Where `cd`, `pushd` or `popd` with `argv` may leave the line.
fn cd(argv: &[String], st: &State) -> Values {
    let program = program_name(&argv[0]);
    if program == "popd" {
        return st.visited.clone();
    }
    let target = argv[1..]
        .iter()
        .find(|a| !a.starts_with('-') || *a == "-")
        .cloned();
    match target.as_deref() {
        None if program == "pushd" => st.oldpwd.clone(),
        None => st.var("HOME"),
        Some("-") => st.oldpwd.clone(),
        Some(dir) => st.cwd.iter().map(|cwd| resolve(cwd, dir)).collect(),
    }
}

/// Remove launch wrappers (`env`, `command`, `sudo`, `nice`, `timeout`,
/// `nohup`, …); `None` when the command only prints (`command -v`).
fn unwrap(argv: &[String]) -> Option<Unwrapped> {
    let mut argv = argv.to_vec();
    let mut env = Vec::new();
    let mut cwd = None;
    for _ in 0..8 {
        while let Some((name, value)) = argv
            .first()
            .and_then(|w| w.split_once('='))
            .filter(|(n, _)| is_name(n))
        {
            env.push((name.to_string(), value.to_string()));
            argv.remove(0);
        }
        let Some(first) = argv.first() else {
            break;
        };
        let rest = &argv[1..];
        let next: Vec<String> = match program_name(first).as_str() {
            "env" => unwrap_env(rest, &mut env, &mut cwd),
            "command" => {
                if rest
                    .iter()
                    .take_while(|a| a.starts_with('-'))
                    .any(|a| a.contains(['v', 'V']))
                {
                    return None;
                }
                skip_options(rest, &[])
            }
            "builtin" | "nohup" | "unbuffer" | "busybox" | "setsid" => skip_options(rest, &[]),
            "exec" => skip_options(rest, &["-a"]),
            "time" => skip_options(rest, &["-f", "--format", "-o", "--output"]),
            "caffeinate" => skip_options(rest, &["-t", "-w"]),
            "nice" => skip_options(rest, &["-n", "--adjustment"]),
            "ionice" => skip_options(rest, &["-c", "--class", "-n", "--classdata"]),
            "stdbuf" => skip_options(rest, &["-i", "-o", "-e", "--input", "--output", "--error"]),
            "chrt" => {
                let after = skip_options(rest, &["-T", "-P", "-D"]);
                after.get(1..).unwrap_or_default().to_vec()
            }
            "taskset" => {
                let after = skip_options(rest, &[]);
                after.get(1..).unwrap_or_default().to_vec()
            }
            "timeout" => {
                let after = skip_options(rest, &["-s", "--signal", "-k", "--kill-after"]);
                after.get(1..).unwrap_or_default().to_vec()
            }
            "sudo" | "doas" | "pkexec" | "gsudo" => unwrap_privilege(rest, &mut cwd),
            _ => break,
        };
        argv = next;
    }
    Some(Unwrapped { argv, env, cwd })
}

/// `env`'s own options and assignments, up to the command it runs.
fn unwrap_env(
    rest: &[String],
    env: &mut Vec<(String, String)>,
    cwd: &mut Option<String>,
) -> Vec<String> {
    let mut at = 0;
    let mut split_string = None;
    while let Some(a) = rest.get(at) {
        if a == "--" {
            at += 1;
            break;
        }
        if let Some((name, value)) = a.split_once('=').filter(|(n, _)| is_name(n)) {
            env.push((name.to_string(), value.to_string()));
            at += 1;
        } else if matches!(a.as_str(), "-u" | "--unset") {
            at += 2;
        } else if matches!(a.as_str(), "-C" | "--chdir") {
            *cwd = rest.get(at + 1).cloned();
            at += 2;
        } else if let Some(dir) = a.strip_prefix("--chdir=") {
            *cwd = Some(dir.to_string());
            at += 1;
        } else if matches!(a.as_str(), "-S" | "--split-string") {
            let mut next: Vec<String> = rest
                .get(at + 1)
                .map(|s| s.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default();
            next.extend(rest.get(at + 2..).unwrap_or_default().iter().cloned());
            split_string = Some(next);
            break;
        } else if a.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    split_string.unwrap_or_else(|| rest.get(at..).unwrap_or_default().to_vec())
}

/// `sudo`'s (or `doas`'s) options, up to the command it runs.
fn unwrap_privilege(rest: &[String], cwd: &mut Option<String>) -> Vec<String> {
    let mut at = 0;
    while let Some(a) = rest.get(at) {
        if a == "--" {
            at += 1;
            break;
        }
        if !a.starts_with('-') {
            break;
        }
        if matches!(a.as_str(), "-D" | "--chdir") {
            *cwd = rest.get(at + 1).cloned();
            at += 2;
        } else if let Some(dir) = a.strip_prefix("--chdir=") {
            *cwd = Some(dir.to_string());
            at += 1;
        } else if matches!(
            a.as_str(),
            "-u" | "--user"
                | "-g"
                | "--group"
                | "-C"
                | "--close-from"
                | "-h"
                | "--host"
                | "-p"
                | "--prompt"
                | "-r"
                | "--role"
                | "-t"
                | "--type"
                | "-T"
                | "--command-timeout"
                | "-U"
                | "--other-user"
        ) {
            at += 2;
        } else {
            at += 1;
        }
    }
    rest.get(at..).unwrap_or_default().to_vec()
}

/// A wrapper's arguments after its options; those in `with_value` take the
/// next word (or carry it attached, as `-n5`).
fn skip_options(args: &[String], with_value: &[&str]) -> Vec<String> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            at += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        at += if with_value.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    args.get(at..).unwrap_or_default().to_vec()
}

/// The script a shell runs with `-c` (or a flag cluster holding `c`), and
/// the words after it: `$0`, then the positional parameters.
fn shell_script(args: &[String]) -> Option<(&str, &[String])> {
    let at = args.iter().position(|a| {
        a == "-c"
            || a == "--command"
            || (a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
    })?;
    let script = args[at + 1..]
        .iter()
        .position(|a| !a.starts_with('-') || a.as_str() == "-")?
        + at
        + 1;
    Some((args[script].as_str(), &args[script + 1..]))
}

/// The command `xargs` runs, its replacement token and whether it reads
/// its input from a file instead.
fn after_xargs_options(args: &[String]) -> (&[String], Option<String>, bool) {
    const WITH_VALUE: &[&str] = &[
        "-L",
        "-n",
        "-P",
        "-s",
        "-d",
        "-E",
        "--delimiter",
        "--eof",
        "--max-lines",
        "--max-args",
        "--max-procs",
        "--max-chars",
    ];
    let mut replace = None;
    let mut file_input = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            at += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        if arg == "-I" {
            replace = args.get(at + 1).cloned();
            at += 2;
        } else if let Some(token) = arg.strip_prefix("-I") {
            replace = Some(token.to_string());
            at += 1;
        } else if let Some(token) = arg.strip_prefix("--replace") {
            replace = Some(token.strip_prefix('=').unwrap_or("{}").to_string());
            at += 1;
        } else if arg == "-i" {
            replace = Some("{}".to_string());
            at += 1;
        } else if matches!(arg.as_str(), "-a" | "--arg-file") {
            file_input = true;
            at += 2;
        } else if arg.starts_with("--arg-file=") {
            file_input = true;
            at += 1;
        } else {
            at += if WITH_VALUE.contains(&arg.as_str()) {
                2
            } else {
                1
            };
        }
    }
    (args.get(at..).unwrap_or_default(), replace, file_input)
}

/// The commands a `find` expression runs with `-exec`, `-execdir`, `-ok`
/// or `-okdir`.
fn exec_bodies(expression: &[String]) -> Vec<Vec<String>> {
    let mut bodies = Vec::new();
    let mut at = 0;
    while at < expression.len() {
        if matches!(
            expression[at].as_str(),
            "-exec" | "-execdir" | "-ok" | "-okdir"
        ) {
            let body = &expression[at + 1..];
            let end = body
                .iter()
                .position(|a| a == ";" || a == "+")
                .unwrap_or(body.len());
            bodies.push(body[..end].to_vec());
            at += end + 2;
        } else {
            at += 1;
        }
    }
    bodies
}

/// Whether a command removes files: a remover, possibly behind launchers,
/// `xargs`, or a shell whose script runs one.
fn runs_remover(argv: &[String], depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Some(run) = unwrap(argv) else {
        return false;
    };
    let Some(first) = run.argv.first() else {
        return false;
    };
    match program_name(first).as_str() {
        "rm" | "rmdir" | "unlink" | "shred" | "srm" => true,
        "xargs" => runs_remover(after_xargs_options(&run.argv[1..]).0, depth + 1),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" | "fish" | "mksh" | "eval" => {
            let script = if program_name(first) == "eval" {
                run.argv[1..].join(" ")
            } else {
                shell_script(&run.argv[1..])
                    .map(|(script, _)| script.to_string())
                    .unwrap_or_default()
            };
            let mut simples = Vec::new();
            collect_simples(&Parser::parse(&script), &mut simples);
            simples.iter().any(|(words, _)| {
                let argv: Vec<String> = words.iter().map(word_text).collect();
                runs_remover(&argv, depth + 1)
            })
        }
        _ => false,
    }
}

/// The commands of a sequence, with nested sequences opened.
fn flatten(node: Node, out: &mut Vec<Node>) {
    match node {
        Node::Seq(items) => {
            for item in items {
                flatten(item, out);
            }
        }
        other => out.push(other),
    }
}

fn collect_simples(node: &Node, out: &mut Vec<(Vec<Word>, Vec<Redir>)>) {
    match node {
        Node::Simple(words, redirs) => out.push((words.clone(), redirs.clone())),
        Node::Sub(body)
        | Node::Func(_, body)
        | Node::For(_, _, body)
        | Node::Redirected(body, _) => {
            collect_simples(body, out);
        }
        Node::Seq(items) | Node::AndOr(items) | Node::Pipe(items) | Node::Case(items) => {
            for item in items {
                collect_simples(item, out);
            }
        }
        Node::If(arms, otherwise) => {
            for (cond, body) in arms {
                collect_simples(cond, out);
                collect_simples(body, out);
            }
            if let Some(otherwise) = otherwise {
                collect_simples(otherwise, out);
            }
        }
        Node::Loop(cond, body) => {
            collect_simples(cond, out);
            collect_simples(body, out);
        }
    }
}

/// The first word of a simple command after its assignments.
fn command_word(words: &[Word]) -> Option<&Word> {
    words.iter().find(|w| w.assignment().is_none())
}

/// Whether a loop body may leave early with `break` or `return`, or run
/// something the reader cannot see into for that: a function or alias
/// (its body may `break`), `eval`, `source`, or a command it cannot name.
fn may_break(body: &Node, st: &State) -> bool {
    let mut simples = Vec::new();
    collect_simples(body, &mut simples);
    simples.iter().any(|(words, _)| {
        words
            .iter()
            .any(|w| matches!(w.plain(), Some("break" | "return")))
            || command_word(words).is_some_and(|word| match word.plain() {
                Some(name) => {
                    matches!(name, "eval" | "source" | "." | "command" | "builtin")
                        || st.funcs.contains_key(name)
                        || st.aliases.contains_key(name)
                }
                None => true,
            })
    })
}

/// Whether a loop body may set `name`, or runs something the reader cannot
/// see into (`eval`, `source`, a function or alias, a command it cannot
/// name).
fn may_assign(body: &Node, name: &str, st: &State) -> bool {
    let mut simples = Vec::new();
    collect_simples(body, &mut simples);
    let mut nested = false;
    walk_for(body, &mut |var| nested |= var == name);
    nested
        || matches!(body, Node::Func(..))
        || simples.iter().any(|(words, _)| {
            command_word(words).is_some_and(|word| {
                word.plain()
                    .is_none_or(|w| st.funcs.contains_key(w) || st.aliases.contains_key(w))
            }) || words.iter().any(|w| {
                w.assignment().is_some_and(|a| a.name == name)
                    || assigns_by_expansion(&w.parts, name)
                    || w.plain().is_some_and(|t| {
                        t == name
                            || matches!(
                                t,
                                "eval"
                                    | "source"
                                    | "."
                                    | "read"
                                    | "unset"
                                    | "declare"
                                    | "export"
                                    | "local"
                                    | "typeset"
                                    | "readonly"
                                    | "mapfile"
                                    | "readarray"
                                    | "printf"
                                    | "getopts"
                            )
                    })
            })
        })
}

/// Whether `${NAME:=…}` or `${NAME=…}` for `name` is among `parts`.
fn assigns_by_expansion(parts: &[Part], name: &str) -> bool {
    parts.iter().any(|part| match part {
        Part::Param {
            name: n,
            op: Some(ParamOp::Default { word, assign }),
            ..
        } => (*assign && n == name) || assigns_by_expansion(word, name),
        Part::Param {
            op: Some(ParamOp::Alternate(word)),
            ..
        } => assigns_by_expansion(word, name),
        _ => false,
    })
}

fn walk_for(node: &Node, visit: &mut impl FnMut(&str)) {
    match node {
        Node::For(var, _, body) => {
            visit(var);
            walk_for(body, visit);
        }
        Node::Sub(body) | Node::Func(_, body) | Node::Redirected(body, _) => walk_for(body, visit),
        Node::Seq(items) | Node::AndOr(items) | Node::Pipe(items) | Node::Case(items) => {
            for item in items {
                walk_for(item, visit);
            }
        }
        Node::If(arms, otherwise) => {
            for (cond, body) in arms {
                walk_for(cond, visit);
                walk_for(body, visit);
            }
            if let Some(otherwise) = otherwise {
                walk_for(otherwise, visit);
            }
        }
        Node::Loop(cond, body) => {
            walk_for(cond, visit);
            walk_for(body, visit);
        }
        Node::Simple(..) => {}
    }
}

/// `printf FORMAT ARGS` for formats of plain text, `%s` and `%%`.
fn printf(args: &[String]) -> Option<String> {
    let (format, mut rest) = args.split_first()?;
    let mut out = String::new();
    loop {
        let mut chars = format.chars().peekable();
        let mut used = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('0') => out.push('\0'),
                    Some('\\') | None => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                },
                '%' => match chars.next() {
                    Some('%') => out.push('%'),
                    Some('s') => {
                        used = true;
                        if let Some((first, tail)) = rest.split_first() {
                            out.push_str(first);
                            rest = tail;
                        }
                    }
                    _ => return None,
                },
                _ => out.push(c),
            }
        }
        if !used || rest.is_empty() {
            break;
        }
    }
    Some(out)
}

/// A directory as the shell would print it: the project reads as `.`.
fn display_dir(cwd: &str) -> String {
    if cwd.is_empty() {
        ".".to_string()
    } else {
        cwd.to_string()
    }
}

/// `path` as the line reaches it from `cwd`: absolute, from `~`, or
/// relative to where the line started.
fn resolve(cwd: &str, path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            return from_home(rest);
        }
    }
    if path.starts_with('/') || path.starts_with('$') {
        return path.to_string();
    }
    if cwd.is_empty() {
        return path.to_string();
    }
    if let Some(rest) = cwd.strip_prefix('~') {
        return from_home(&format!("{rest}/{path}"));
    }
    if cwd.starts_with('/') {
        return normalize_path(&format!("{cwd}/{path}"));
    }
    if cwd.starts_with('$') {
        return format!("{cwd}/{path}");
    }
    relative_join(cwd, path)
}

/// Join two project-relative paths, keeping any leading `..`.
fn relative_join(base: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// A path below `~` with its `.` and `..` resolved; leaving the home
/// directory resolves from `HOME` instead.
fn from_home(rest: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
                    return normalize_path(&format!("{home}/{rest}"));
                }
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        "~".to_string()
    } else {
        format!("~/{}", parts.join("/"))
    }
}

/// The real path of `path`'s longest existing prefix, with the rest
/// appended; macOS's data-volume prefix is read as the path it serves.
fn real_prefix(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect();
    let globbed = parts
        .iter()
        .position(|p| p.contains(['*', '?', '[']))
        .unwrap_or(parts.len());
    for existing in (0..=globbed).rev() {
        let prefix = format!("/{}", parts[..existing].join("/"));
        if std::fs::symlink_metadata(&prefix).is_err() {
            continue;
        }
        let real = std::fs::canonicalize(&prefix).ok()?;
        let mut joined = real.to_string_lossy().into_owned();
        for part in &parts[existing..] {
            if !joined.ends_with('/') {
                joined.push('/');
            }
            joined.push_str(part);
        }
        let joined = normalize_path(&joined);
        let served = match joined.strip_prefix("/System/Volumes/Data") {
            Some("") => "/".to_string(),
            Some(rest) if rest.starts_with('/') => rest.to_string(),
            _ => joined,
        };
        return Some(served);
    }
    None
}

/// Brace expansion over a word's pieces: a `{a,b}` list or a `{x..y}`
/// range written in unquoted text, whose choices may hold expansions
/// (`{build,$D}`). The text a choice brings keeps its own treatment.
fn brace_expand(pieces: &[(String, Kind)]) -> Vec<Vec<(String, Kind)>> {
    if !pieces
        .iter()
        .any(|(text, kind)| *kind == Kind::Bare && text.contains('{'))
    {
        return vec![pieces.to_vec()];
    }
    let chars: Vec<(char, Kind)> = pieces
        .iter()
        .flat_map(|(text, kind)| text.chars().map(move |c| (c, *kind)))
        .collect();
    let mut out = Vec::new();
    expand_chars(chars, &mut out);
    out.iter().map(|chars| regroup(chars)).collect()
}

fn expand_chars(chars: Vec<(char, Kind)>, out: &mut Vec<Vec<(char, Kind)>>) {
    if out.len() >= MAX_VALUES {
        return;
    }
    let Some((open, close, choices)) = find_brace(&chars) else {
        out.push(chars);
        return;
    };
    for choice in choices {
        let mut next = chars[..open].to_vec();
        next.extend(choice);
        next.extend_from_slice(&chars[close + 1..]);
        expand_chars(next, out);
    }
}

/// The first brace expression in `chars`: its `{`, its `}` and its
/// choices.
#[allow(clippy::type_complexity)]
fn find_brace(chars: &[(char, Kind)]) -> Option<(usize, usize, Vec<Vec<(char, Kind)>>)> {
    let bare = |i: usize, c: char| chars[i] == (c, Kind::Bare);
    for open in 0..chars.len() {
        if !bare(open, '{') {
            continue;
        }
        let mut depth = 0usize;
        let mut commas = Vec::new();
        let mut close = None;
        for i in open..chars.len() {
            if bare(i, '{') {
                depth += 1;
            } else if bare(i, '}') {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            } else if depth == 1 && bare(i, ',') {
                commas.push(i);
            }
        }
        let Some(close) = close else {
            continue;
        };
        if !commas.is_empty() {
            let mut choices = Vec::new();
            let mut start = open + 1;
            for &end in commas.iter().chain([&close]) {
                choices.push(chars[start..end].to_vec());
                start = end + 1;
            }
            return Some((open, close, choices));
        }
        let inner = &chars[open + 1..close];
        if inner.iter().all(|(_, kind)| *kind == Kind::Bare) {
            let text: String = inner.iter().map(|(c, _)| c).collect();
            if let Some(values) = brace_range(&text) {
                let choices = values
                    .into_iter()
                    .map(|v| v.chars().map(|c| (c, Kind::Quoted)).collect())
                    .collect();
                return Some((open, close, choices));
            }
        }
    }
    None
}

/// The values of a `{x..y}` or `{x..y..step}` range of integers or single
/// characters; past [`MAX_VALUES`] the rest is unknown.
fn brace_range(text: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = text.split("..").collect();
    let (from, to, step) = match parts.as_slice() {
        [from, to] => (*from, *to, None),
        [from, to, step] => (*from, *to, Some(step.parse::<i64>().ok()?)),
        _ => return None,
    };
    let step = step.map_or(1, i64::unsigned_abs).max(1);
    let mut values = Vec::new();
    if let (Ok(a), Ok(b)) = (from.parse::<i64>(), to.parse::<i64>()) {
        let width = if from.starts_with('0') || to.starts_with('0') {
            from.len().max(to.len())
        } else {
            0
        };
        let mut n = a;
        loop {
            values.push(format!("{n:0width$}"));
            if n == b || values.len() > MAX_VALUES {
                break;
            }
            let next = if a <= b {
                n.saturating_add_unsigned(step).min(b)
            } else {
                n.saturating_sub_unsigned(step).max(b)
            };
            if (a <= b && next > b) || (a > b && next < b) || next == n {
                break;
            }
            n = next;
            if (a <= b && n > b) || (a > b && n < b) {
                break;
            }
        }
    } else {
        let (mut a, mut b) = (from.chars(), to.chars());
        let (Some(x), None, Some(y), None) = (a.next(), a.next(), b.next(), b.next()) else {
            return None;
        };
        let (x, y) = (u32::from(x), u32::from(y));
        let range: Vec<u32> = if x <= y {
            (x..=y).collect()
        } else {
            (y..=x).rev().collect()
        };
        for c in range.into_iter().step_by(usize::try_from(step).ok()?) {
            values.extend(char::from_u32(c).map(String::from));
            if values.len() > MAX_VALUES {
                break;
            }
        }
    }
    if values.len() > MAX_VALUES {
        values.truncate(MAX_VALUES);
        values.push(taint(why::MANY));
    }
    Some(values)
}

/// Characters back into pieces of one treatment each.
fn regroup(chars: &[(char, Kind)]) -> Vec<(String, Kind)> {
    let mut pieces: Vec<(String, Kind)> = Vec::new();
    for &(c, kind) in chars {
        match pieces.last_mut() {
            Some((text, k)) if *k == kind => text.push(c),
            _ => pieces.push((c.to_string(), kind)),
        }
    }
    pieces
}

/// Join an alternative's pieces into fields: an unquoted expansion splits
/// on the `IFS` characters (and is unknown where `IFS` is), an unquoted
/// expansion that is empty makes no field, and each argument of `"$@"`
/// is a field of its own.
fn split_fields(pieces: &[(String, Kind)], ifs: &Result<String, &'static str>) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut started = false;
    for (text, kind) in pieces {
        for (n, segment) in text.split(FIELD_BREAK).enumerate() {
            if n > 0 {
                if started {
                    fields.push(std::mem::take(&mut current));
                }
                started = *kind == Kind::Quoted;
            }
            match (kind, ifs) {
                (Kind::Split, Ok(chars)) => {
                    let mut first = true;
                    for piece in segment.split(|c: char| chars.contains(c)) {
                        if !first && started {
                            fields.push(std::mem::take(&mut current));
                            started = false;
                        }
                        first = false;
                        if !piece.is_empty() {
                            current.push_str(piece);
                            started = true;
                        }
                    }
                }
                (Kind::Split, Err(reason)) => {
                    if !segment.is_empty() {
                        current.push_str(segment);
                        current.push_str(&taint(reason));
                        started = true;
                    }
                }
                _ => {
                    current.push_str(segment);
                    started = true;
                }
            }
        }
    }
    if started {
        fields.push(current);
    }
    fields
}

/// `${VAR%pat}` and friends for patterns of text, `*` and `?`.
fn strip(value: &str, pattern: &str, suffix: bool, longest: bool) -> String {
    let len = value.len();
    let mut cuts: Vec<usize> = (0..=len).filter(|i| value.is_char_boundary(*i)).collect();
    if suffix {
        // Suffix: try starting points from the right for the shortest.
        if longest {
            cuts.sort_unstable();
        } else {
            cuts.sort_unstable_by(|a, b| b.cmp(a));
        }
        for at in cuts {
            if glob_match(pattern, &value[at..]) {
                return value[..at].to_string();
            }
        }
    } else {
        if longest {
            cuts.sort_unstable_by(|a, b| b.cmp(a));
        } else {
            cuts.sort_unstable();
        }
        for at in cuts {
            if glob_match(pattern, &value[..at]) {
                return value[at..].to_string();
            }
        }
    }
    value.to_string()
}

/// Whether a glob of `*`, `?` and bracket expressions matches `text`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        let step = match p.get(pi) {
            Some('*') => {
                star = Some(pi);
                mark = ti;
                pi += 1;
                continue;
            }
            Some('?') => Some(pi + 1),
            Some('[') => match bracket(&p, pi, t[ti]) {
                Some((true, next)) => Some(next),
                Some((false, _)) => None,
                None => (t[ti] == '[').then_some(pi + 1),
            },
            Some(&c) => (c == t[ti]).then_some(pi + 1),
            None => None,
        };
        match (step, star) {
            (Some(next), _) => {
                pi = next;
                ti += 1;
            }
            (None, Some(s)) => {
                pi = s + 1;
                mark += 1;
                ti = mark;
            }
            (None, None) => return false,
        }
    }
    while p.get(pi) == Some(&'*') {
        pi += 1;
    }
    pi == p.len()
}

/// The bracket expression at `p[at]`: whether `c` matches it and the index
/// after it; `None` when it is not one. A character class (`[[:alpha:]]`)
/// is taken to match any character.
fn bracket(p: &[char], at: usize, c: char) -> Option<(bool, usize)> {
    let mut i = at + 1;
    let negate = matches!(p.get(i), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while let Some(&ch) = p.get(i) {
        if ch == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if ch == '[' && p.get(i + 1) == Some(&':') {
            let close =
                (i + 2..p.len().saturating_sub(1)).find(|&k| p[k] == ':' && p[k + 1] == ']')?;
            matched = true;
            i = close + 2;
        } else if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
            if ch <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// A shell variable name.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::super::dangerous::DangerousModule;
    use super::super::guard_forms::{
        Expect, COMPOSED_PAIRS, NESTINGS, PROJECT_DELETIONS, REVIEW_PROBES,
        REVIEW_ROUND_TWO_PROBES, REVIEW_SYMLINK_PROBES,
    };
    use super::composed_deletion_in;
    use crate::security::{CheckContext, SecurityModule, SecurityPolicy, Verdict};

    fn verdict(command: &str) -> Option<Verdict> {
        let policy = SecurityPolicy::defaults();
        DangerousModule.check(&CheckContext {
            command,
            sandbox_bypass: false,
            current_branch: "",
            policy: &policy,
        })
    }

    fn refused(command: &str) -> Option<String> {
        verdict(command).map(|verdict| verdict.pattern)
    }

    #[test]
    fn each_composed_form_is_refused_as_its_rm_equivalent() {
        for (form, equivalent) in COMPOSED_PAIRS {
            let expected = refused(equivalent);
            assert!(expected.is_some(), "the equivalent {equivalent} is refused");
            assert_eq!(refused(form), expected, "{form} as {equivalent}");
        }
    }

    #[test]
    fn a_project_deletion_is_allowed() {
        for command in PROJECT_DELETIONS {
            assert_eq!(refused(command), None, "{command}");
        }
        // A form named as data is not run.
        for command in [
            "git commit -m 'cd / && rm -rf *'",
            "echo 'find / -delete'",
            "grep -rn 'ls / | xargs rm -rf' docs",
            "printf '%s' 'D=/; rm -rf $D'",
        ] {
            assert_eq!(refused(command), None, "{command}");
        }
    }

    /// The gap class is composition: every form in every nesting is
    /// refused, and every project deletion in every nesting is allowed.
    #[test]
    fn nesting_changes_no_verdict() {
        for nesting in NESTINGS {
            for (form, _) in COMPOSED_PAIRS {
                let nested = nesting.replace("{}", form);
                assert!(refused(&nested).is_some(), "{nested}");
            }
            for command in PROJECT_DELETIONS {
                let nested = nesting.replace("{}", command);
                assert_eq!(refused(&nested), None, "{nested}");
            }
        }
    }

    /// Codex round 1: every probe command keeps the verdict this task holds
    /// it to.
    #[test]
    fn every_round_one_probe_keeps_its_verdict() {
        let mut wrong = Vec::new();
        for (case, command, expected) in REVIEW_PROBES {
            if refused(command).is_some() != *expected {
                wrong.push(format!("{case}: {command}"));
            }
        }
        assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
    }

    /// A project entry that links to `/` is judged as `/`, by glob and
    /// after `cd`; the same names as plain directories stay allowed.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_protected_directory_is_judged_where_it_lands() {
        let project = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/", project.path().join("root-link")).unwrap();
        std::fs::create_dir(project.path().join("build")).unwrap();
        for (case, command) in REVIEW_SYMLINK_PROBES {
            let found = composed_deletion_in(command, Some(project.path()));
            assert_eq!(found.map(|f| f.target), Some("/"), "{case}: {command}");
            let plain = command.replace("root-link", "build");
            assert_eq!(
                composed_deletion_in(&plain, Some(project.path())),
                None,
                "{plain}"
            );
        }
        // A parent reached through `..` is judged where it lands too.
        let inner = project.path().join("build");
        let found = composed_deletion_in("rm -rf ../root-link/*", Some(&inner));
        assert_eq!(found.map(|f| f.target), Some("/"));
    }

    /// A refusal that rests on one of several feasible values says so
    /// (F5): the line may not reach the target, and the message names why.
    #[test]
    fn a_refusal_on_one_of_several_values_names_the_reason() {
        for command in [
            "D=/; if false; then D=build; fi; rm -rf $D",
            "for d in / build; do break; done; rm -rf $d",
            "true && cd /; rm -rf *",
        ] {
            let verdict = verdict(command).expect(command);
            assert!(
                verdict.reason.contains("one of several values"),
                "{command}: {}",
                verdict.reason
            );
        }
        let verdict = verdict("D=/; (D=build); rm -rf $D").unwrap();
        assert!(
            !verdict.reason.contains("one of several"),
            "{}",
            verdict.reason
        );
    }

    /// Codex round 2 and the taint rule: every probe, read in a project
    /// holding `build`, `empty` and a `root-link` to `/`, keeps its verdict.
    #[cfg(unix)]
    #[test]
    fn every_round_two_probe_keeps_its_verdict() {
        let project = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/", project.path().join("root-link")).unwrap();
        for dir in ["build", "empty"] {
            std::fs::create_dir(project.path().join(dir)).unwrap();
        }
        let mut wrong = Vec::new();
        for (case, command, expect) in REVIEW_ROUND_TWO_PROBES {
            let found = composed_deletion_in(command, Some(project.path()));
            let floor = refused(command).is_some();
            let held = match expect {
                Expect::Protected => found.as_ref().is_some_and(|f| f.unproven.is_none()),
                Expect::Unproven => found.as_ref().is_some_and(|f| f.unproven.is_some()),
                Expect::Allowed => found.is_none() && !floor,
                Expect::Floor => found.is_none() && floor,
            };
            if !held {
                wrong.push(format!(
                    "{case} ({expect:?}): {command} -> {found:?}, floor {floor}"
                ));
            }
        }
        assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
    }

    /// An unproven target is refused with what it depends on, and the
    /// refusal points to a literal project path.
    #[test]
    fn an_unproven_target_is_refused_with_its_reason() {
        for (command, names) in [
            ("source ./env.sh; rm -rf \"$D\"", "sourced file"),
            ("N=D; rm -rf \"${!N}\"", "parameter expansion"),
            ("$CMD /; rm -rf *", "working directory"),
        ] {
            let verdict = verdict(command).expect(command);
            assert!(
                verdict.reason.contains("cannot be proven")
                    && verdict.reason.contains(names)
                    && verdict.reason.contains("rm -rf ./build"),
                "{command}: {}",
                verdict.reason
            );
        }
    }
}
