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
//! **Sound for protected targets.** For every variable and for the working
//! directory the reader keeps the set of values it may hold (its feasible
//! values). A subshell, a pipeline stage, `sh -c` and a substitution run on
//! a copy that never flows back. A branch, an `&&` or `||` operand, a `case`
//! arm, a loop body and a function body may or may not run, so the state
//! after them is the union of every path through them. Only an assignment,
//! `cd` or `unset` that runs unconditionally in the same shell narrows a
//! value. A deletion is refused when any feasible value of its target is
//! protected, and the refusal says so when the value is one of several.
//!
//! **Precise where it is cheap.** Single quotes keep `$VAR` literal; a `for`
//! over a fixed list leaves its variable at the last word when the body
//! neither breaks nor assigns it; `${VAR:?}`, `${VAR:-…}` and `${VAR%…}`
//! expand to their possible values; `$(printf …)`, `$(echo …)` and `$(pwd)`
//! are read; and a path that exists is also judged where it really lands
//! (the real path of its longest existing prefix), so a link to `/` is `/`.
//!
//! **Residual (ADR-0009).** What the reader cannot see is judged by its
//! spelling and passes: a value from the environment or an unknown command,
//! a sourced file, an alias, and programs in another language.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::dangerous::{dangerous_rm_target, normalize_path, program_name, recursive_rm_operands};

/// A protected deletion found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    /// The protected target, as the `rm` check names it.
    pub target: &'static str,
    /// Set when the target is one of several values the line may reach
    /// there, so the refusal can say why a line that may not reach it is
    /// refused.
    pub ambiguous: bool,
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
        functions: BTreeMap::new(),
        pipe_input: None,
    };
    let mut state = State::start();
    reader.script(command, &mut state);
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
    /// Text the reader does not evaluate, kept as spelled.
    Opaque { text: String, quoted: bool },
}

#[derive(Debug, Clone, PartialEq)]
enum ParamOp {
    /// `:-`, `-`, `:=` and `=`: the word when the value is empty or unset.
    Default(Vec<Part>),
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

    /// `NAME=` as the start of this word, with the parts after `=`.
    fn assignment(&self) -> Option<(String, Vec<Part>)> {
        let Some(Part::Lit {
            text,
            quoted: false,
        }) = self.parts.first()
        else {
            return None;
        };
        let (name, rest) = text.split_once('=')?;
        if !is_name(name) {
            return None;
        }
        let mut value = Vec::new();
        if !rest.is_empty() {
            value.push(Part::Lit {
                text: rest.to_string(),
                quoted: false,
            });
        }
        value.extend(self.parts[1..].iter().cloned());
        Some((name.to_string(), value))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Redir {
    op: String,
    target: Option<Word>,
    /// A heredoc's body.
    body: Option<String>,
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
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            at: 0,
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
                    flush(&mut toks, &mut word);
                    self.at += 1;
                }
                '\\' if self.peek(1) == Some('\n') => self.at += 2,
                '\n' => {
                    flush(&mut toks, &mut word);
                    toks.push(Tok::Op("\n"));
                    self.at += 1;
                    for doc in pending.drain(..) {
                        let body = self.heredoc_body(&doc.delimiter, doc.strip_tabs);
                        if let Some(Tok::Redir(redir)) = toks.get_mut(doc.at) {
                            redir.body = Some(body);
                        }
                    }
                }
                '#' if word.is_none() => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                ';' | '&' | '|' => {
                    flush(&mut toks, &mut word);
                    self.operator(ch, &mut toks);
                }
                '(' => self.paren(&mut toks, &mut word),
                ')' => {
                    flush(&mut toks, &mut word);
                    toks.push(Tok::Op(")"));
                    self.at += 1;
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    // A process substitution runs its command.
                    let (inner, next) = balanced(&self.chars, self.at + 1, '(', ')');
                    self.at = next;
                    word.get_or_insert_with(Word::default)
                        .parts
                        .push(Part::Subst {
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
                    flush(&mut toks, &mut word);
                    self.redirection(&mut toks, &mut pending);
                }
                _ => {
                    let w = word.get_or_insert_with(Word::default);
                    self.word_char(w, true);
                }
            }
        }
        flush(&mut toks, &mut word);
        toks
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
        if array || (word.is_none() && self.peek(1) == Some('(')) {
            // Neither an array value nor `(( … ))` arithmetic is run.
            let (inner, next) = balanced(&self.chars, self.at, '(', ')');
            self.at = next;
            word.get_or_insert_with(Word::default)
                .parts
                .push(Part::Opaque {
                    text: format!("({inner})"),
                    quoted: false,
                });
            if !array {
                flush(toks, word);
            }
            return;
        }
        flush(toks, word);
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
            let delimiter = self
                .redirect_target()
                .map(|w| word_text(&w))
                .unwrap_or_default();
            pending.push(Pending {
                at: toks.len(),
                delimiter,
                strip_tabs: op == "<<-",
            });
            toks.push(Tok::Redir(Redir {
                op: op.to_string(),
                target: None,
                body: None,
            }));
            return;
        }
        let target = self.redirect_target();
        toks.push(Tok::Redir(Redir {
            op: op.to_string(),
            target,
            body: None,
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
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.at += 2;
                word.parts.push(Part::Opaque {
                    text: format!("${c}"),
                    quoted,
                });
            }
            _ => {
                word.push_lit('$', quoted);
                self.at += 1;
            }
        }
    }
}

fn flush(toks: &mut Vec<Tok>, word: &mut Option<Word>) {
    if let Some(w) = word.take() {
        toks.push(Tok::Word(w));
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
    let opaque = || Part::Opaque {
        text: format!("${{{inner}}}"),
        quoted,
    };
    if inner.starts_with(['#', '!']) {
        return opaque();
    }
    let name_len = inner
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(inner.len());
    let (name, rest) = inner.split_at(name_len);
    if !is_name(name) {
        return opaque();
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
    } else if let Some(arg) = [":-", ":=", "-", "="]
        .iter()
        .find_map(|op| rest.strip_prefix(op))
    {
        Some(ParamOp::Default(word(arg)))
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
            return opaque();
        }
        Some(ParamOp::Strip {
            suffix,
            longest,
            pattern: pattern.to_string(),
        })
    } else {
        return opaque();
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
        while matches!(self.peek(), Some(Tok::Redir(_))) {
            self.at += 1;
        }
        node
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
// State
// ---------------------------------------------------------------------------

type Values = BTreeSet<String>;

/// What a point in the line may hold. A working directory of `""` is where
/// the line started (the project); a relative one is below it.
#[derive(Debug, Clone, PartialEq)]
struct State {
    vars: BTreeMap<String, Values>,
    cwd: Values,
    oldpwd: Values,
    /// Every directory the line may have been in, for `popd`.
    visited: Values,
}

impl State {
    fn start() -> Self {
        let here: Values = [String::new()].into();
        Self {
            vars: BTreeMap::new(),
            cwd: here.clone(),
            oldpwd: ["$OLDPWD".to_string()].into(),
            visited: here,
        }
    }

    /// The values `name` may hold; one read from the environment is kept
    /// as spelled, except `HOME`, which is the home directory.
    fn var(&self, name: &str) -> Values {
        if let Some(values) = self.vars.get(name) {
            return values.clone();
        }
        match name {
            "HOME" => ["~".to_string()].into(),
            "PWD" => self.cwd.clone(),
            "OLDPWD" => self.oldpwd.clone(),
            _ => [format!("${name}")].into(),
        }
    }

    fn set(&mut self, name: &str, values: Values) {
        self.vars.insert(name.to_string(), bounded(values));
    }

    fn set_cwd(&mut self, cwd: Values) {
        let cwd = bounded(cwd);
        self.oldpwd = std::mem::replace(&mut self.cwd, cwd);
        self.visited = bounded(self.visited.union(&self.cwd).cloned().collect());
    }

    /// Every path through `self` or `other`.
    fn join(&mut self, other: &Self) {
        let names: BTreeSet<String> = self.vars.keys().chain(other.vars.keys()).cloned().collect();
        for name in names {
            let joined: Values = self.var(&name).union(&other.var(&name)).cloned().collect();
            self.set(&name, joined);
        }
        self.cwd = bounded(self.cwd.union(&other.cwd).cloned().collect());
        self.oldpwd = bounded(self.oldpwd.union(&other.oldpwd).cloned().collect());
        self.visited = bounded(self.visited.union(&other.visited).cloned().collect());
    }
}

/// Keep at most [`MAX_VALUES`], protected values first.
fn bounded(values: Values) -> Values {
    if values.len() <= MAX_VALUES {
        return values;
    }
    let (mut kept, rest): (Values, Values) = values
        .into_iter()
        .partition(|value| dangerous_rm_target(value).is_some());
    for value in rest {
        if kept.len() >= MAX_VALUES {
            break;
        }
        kept.insert(value);
    }
    kept
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

struct Reader<'a> {
    base: Option<&'a Path>,
    found: Option<Found>,
    depth: usize,
    functions: BTreeMap<String, Node>,
    /// What the pipeline stage being read receives on its input.
    pipe_input: Option<Vec<Input>>,
}

impl Reader<'_> {
    fn script(&mut self, script: &str, st: &mut State) {
        if self.depth > MAX_DEPTH || self.found.is_some() {
            return;
        }
        self.depth += 1;
        let node = Parser::parse(script);
        self.run(&node, st);
        self.depth -= 1;
    }

    fn report(&mut self, target: &'static str, ambiguous: bool) {
        if self.found.is_none() {
            self.found = Some(Found { target, ambiguous });
        }
    }

    fn run(&mut self, node: &Node, st: &mut State) {
        if self.found.is_some() {
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
                self.run(body, &mut child);
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
                for _ in 0..LOOP_PASSES {
                    let mut pass = acc.clone();
                    self.run(body, &mut pass);
                    self.run(cond, &mut pass);
                    let mut next = acc.clone();
                    next.join(&pass);
                    if next == acc {
                        break;
                    }
                    acc = next;
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
                self.functions.insert(name.clone(), (**body).clone());
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
            self.run(stage, &mut child);
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
            None => (["$@".to_string()].into(), None),
        };
        let mut acc = st.clone();
        for _ in 0..LOOP_PASSES {
            let mut pass = acc.clone();
            pass.set(name, values.clone());
            self.run(body, &mut pass);
            let mut next = acc.clone();
            next.join(&pass);
            if next == acc {
                break;
            }
            acc = next;
        }
        // A fixed list the body never leaves early nor reassigns ends on its
        // last word; otherwise every word stays feasible.
        match fixed {
            Some(last) if !may_break(body) && !may_assign(body, name) => {
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

    fn simple(&mut self, words: &[Word], redirs: &[Redir], st: &mut State) {
        let assigned = words
            .iter()
            .take_while(|w| w.assignment().is_some())
            .count();
        let (assignments, command) = words.split_at(assigned);
        let env: Vec<(String, Values)> = assignments
            .iter()
            .filter_map(Word::assignment)
            .map(|(name, parts)| {
                let values = self.value(&parts, st);
                (name, values)
            })
            .collect();
        // Redirections and heredocs are read even with no command.
        let input = self.redirect_input(redirs, st);
        if command.is_empty() {
            for (name, values) in env {
                st.set(&name, values);
            }
            return;
        }
        let variants = self.argv_variants(command, st);
        let ambiguous = variants.len() > 1;
        if self.builtin(command, &variants, input.as_deref(), st) {
            return;
        }
        let mut child = st.clone();
        for (name, values) in &env {
            child.set(name, values.clone());
        }
        let fed = self.pipe_input.clone().or(input);
        for argv in &variants {
            self.exec(argv, &child, fed.as_deref(), false, ambiguous);
        }
    }

    /// A builtin that changes this shell (`cd`, an assignment builtin,
    /// `unset`, `read`, `eval`, a function call); `false` for any other
    /// command.
    fn builtin(
        &mut self,
        command: &[Word],
        variants: &[Vec<String>],
        input: Option<&[Input]>,
        st: &mut State,
    ) -> bool {
        let program = variants
            .first()
            .and_then(|argv| argv.first())
            .map(|p| program_name(p))
            .unwrap_or_default();
        match program.as_str() {
            "cd" | "pushd" | "popd" | "chdir" => {
                let mut cwd = Values::new();
                for argv in variants {
                    cwd.extend(cd(argv, st));
                }
                st.set_cwd(cwd);
                true
            }
            "export" | "readonly" | "local" | "declare" | "typeset" => {
                for word in &command[1..] {
                    if let Some((name, parts)) = word.assignment() {
                        let values = self.value(&parts, st);
                        st.set(&name, values);
                    }
                }
                true
            }
            "unset" => {
                for argv in variants {
                    for name in argv[1..].iter().filter(|a| !a.starts_with('-')) {
                        st.set(name, [String::new()].into());
                        self.functions.remove(name);
                    }
                }
                true
            }
            "read" | "mapfile" | "readarray" | "getopts" => {
                let fed = self
                    .pipe_input
                    .clone()
                    .or_else(|| input.map(<[Input]>::to_vec));
                for argv in variants {
                    for name in argv[1..].iter().filter(|a| is_name(a)) {
                        let values = match &fed {
                            Some(inputs) if !inputs.is_empty() => {
                                inputs.iter().map(|i| i.value.clone()).collect()
                            }
                            _ => [format!("${name}")].into(),
                        };
                        st.set(name, values);
                    }
                }
                true
            }
            "eval" => {
                let mut outs = Vec::new();
                for argv in variants {
                    let mut taken = st.clone();
                    self.script(&argv[1..].join(" "), &mut taken);
                    outs.push(taken);
                }
                if let Some(first) = outs.first().cloned() {
                    let mut joined = first;
                    for out in &outs[1..] {
                        joined.join(out);
                    }
                    *st = joined;
                }
                true
            }
            name if self.functions.contains_key(name) && self.depth <= MAX_DEPTH => {
                if let Some(body) = self.functions.get(name).cloned() {
                    self.depth += 1;
                    self.run(&body, st);
                    self.depth -= 1;
                }
                true
            }
            _ => false,
        }
    }

    /// Run one concrete command: unwrap its launchers and judge what it
    /// deletes. `forced` marks a remover given a whole tree.
    fn exec(
        &mut self,
        argv: &[String],
        st: &State,
        input: Option<&[Input]>,
        forced: bool,
        ambiguous: bool,
    ) {
        if self.found.is_some() || self.depth > MAX_DEPTH {
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
                let mut child = st.clone();
                if let Some(script) = shell_script(&argv[1..]) {
                    let script = script.to_string();
                    self.depth += 1;
                    self.script(&script, &mut child);
                    self.depth -= 1;
                } else if let Some(input) = input {
                    let script: Vec<&str> = input.iter().map(|i| i.value.as_str()).collect();
                    self.script(&script.join("\n"), &mut child);
                }
            }
            "eval" => {
                let mut child = st.clone();
                self.script(&argv[1..].join(" "), &mut child);
            }
            "rm" => self.judge_rm(&argv, &st, forced, ambiguous),
            "rmdir" | "unlink" | "shred" | "srm" if forced => {
                self.judge_rm(&argv, &st, true, ambiguous);
            }
            "find" => self.judge_find(&argv[1..], &st, ambiguous),
            "xargs" => self.judge_xargs(&argv[1..], &st, input, ambiguous),
            "parallel" => self.judge_parallel(&argv[1..], &st, input, ambiguous),
            "rsync" => self.judge_rsync(&argv[1..], &st, ambiguous),
            name if self.functions.contains_key(name) => {
                if let Some(body) = self.functions.get(name).cloned() {
                    let mut child = st.clone();
                    self.depth += 1;
                    self.run(&body, &mut child);
                    self.depth -= 1;
                }
            }
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
        for operand in operands {
            if let Some(target) = self.judge_path(operand, st) {
                self.report(target, ambiguous);
                return;
            }
        }
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
            for start in starts {
                if let Some(target) = self.judge_path(start, st) {
                    self.report(target, ambiguous);
                    return;
                }
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
    fn judge_xargs(
        &mut self,
        args: &[String],
        st: &State,
        input: Option<&[Input]>,
        ambiguous: bool,
    ) {
        let (inner, replace, file_input) = after_xargs_options(args);
        if inner.is_empty() {
            return;
        }
        self.exec(inner, st, None, false, ambiguous);
        let inputs = if file_input { None } else { input };
        for item in inputs.unwrap_or_default() {
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
        input: Option<&[Input]>,
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
        let mut items: Vec<Input> = sources
            .iter()
            .filter(|a| !a.starts_with(":::"))
            .map(|value| Input {
                value: value.clone(),
                tree: false,
            })
            .collect();
        if sources.is_empty() {
            items.extend(input.unwrap_or_default().iter().cloned());
        }
        self.exec(command, st, None, false, ambiguous);
        for item in items {
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
        for path in judged {
            if let Some(target) = self.judge_path(path, st) {
                self.report(target, ambiguous);
                return;
            }
        }
    }

    /// The protected target `operand` reaches from any working directory
    /// the line may be in: by its spelling, or where it really lands.
    fn judge_path(&self, operand: &str, st: &State) -> Option<&'static str> {
        st.cwd.iter().find_map(|cwd| {
            let path = resolve(cwd, operand);
            dangerous_rm_target(&path).or_else(|| {
                self.real_path(&path)
                    .and_then(|real| dangerous_rm_target(&real))
            })
        })
    }

    /// The real path of `path`'s longest existing prefix with the rest
    /// appended; `None` for a path the reader cannot place.
    fn real_path(&self, path: &str) -> Option<String> {
        if path.contains(['$', '`']) {
            return None;
        }
        let absolute: PathBuf = if let Some(rest) = path.strip_prefix('~') {
            let home = std::env::var_os("HOME")?;
            PathBuf::from(format!("{}{rest}", home.to_string_lossy()))
        } else if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.base?.join(path)
        };
        real_prefix(&absolute)
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

    /// The alternatives for one word, each a list of fields after
    /// splitting and brace expansion.
    fn fields(&mut self, word: &Word, st: &State) -> Vec<Vec<String>> {
        // Each alternative is a list of (text, splittable) pieces.
        let mut alternatives: Vec<Vec<(String, bool)>> = vec![Vec::new()];
        for part in &word.parts {
            let options = self.part_values(part, st);
            let mut next = Vec::new();
            for alternative in &alternatives {
                for option in &options {
                    let mut extended = alternative.clone();
                    extended.push(option.clone());
                    next.push(extended);
                    if next.len() >= MAX_VALUES {
                        break;
                    }
                }
            }
            alternatives = next;
        }
        let mut out = Vec::new();
        for pieces in alternatives {
            for braced in brace_expand(&pieces) {
                let fields = split_fields(&braced);
                if !out.contains(&fields) {
                    out.push(fields);
                }
            }
        }
        out.truncate(MAX_VALUES);
        out
    }

    /// The values of an assignment's right-hand side: no splitting.
    fn value(&mut self, parts: &[Part], st: &State) -> Values {
        let mut values: Vec<String> = vec![String::new()];
        for part in parts {
            let options = self.part_values(part, st);
            let mut next = Vec::new();
            for value in &values {
                for (text, _) in &options {
                    next.push(format!("{value}{text}"));
                }
            }
            next.truncate(MAX_VALUES);
            values = next;
        }
        values.into_iter().collect()
    }

    /// Each value a part may take, with whether the shell splits it.
    fn part_values(&mut self, part: &Part, st: &State) -> Vec<(String, bool)> {
        match part {
            Part::Lit { text, quoted } => vec![(text.clone(), !quoted && has_brace(text))],
            Part::Opaque { text, .. } => vec![(text.clone(), false)],
            Part::Tilde(user) => match user.as_str() {
                "+" => st.cwd.iter().map(|c| (display_dir(c), false)).collect(),
                "-" => st.oldpwd.iter().map(|c| (display_dir(c), false)).collect(),
                // Any user's home is a home directory.
                _ => vec![("~".to_string(), false)],
            },
            Part::Param { name, op, quoted } => {
                let values = self.param_values(name, op.as_ref(), st);
                values.into_iter().map(|v| (v, !quoted)).collect()
            }
            Part::Subst { script, quoted } => {
                // It runs: judge what it deletes, in a child shell.
                let mut child = st.clone();
                self.script(script, &mut child);
                match static_output(script, st) {
                    Some(outputs) => outputs.into_iter().map(|v| (v, !quoted)).collect(),
                    None => vec![(format!("$({script})"), false)],
                }
            }
        }
    }

    fn param_values(&mut self, name: &str, op: Option<&ParamOp>, st: &State) -> Values {
        let values = st.var(name);
        let unknown = |v: &str| v.starts_with('$');
        match op {
            None => values,
            Some(ParamOp::Required) => values.into_iter().filter(|v| !v.is_empty()).collect(),
            Some(ParamOp::Default(word)) => {
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
                    if unknown(&v) {
                        format!("${{{name}}}")
                    } else {
                        strip(&v, pattern, *suffix, *longest)
                    }
                })
                .collect(),
        }
    }

    /// Input a heredoc or here-string gives the command.
    fn redirect_input(&mut self, redirs: &[Redir], st: &State) -> Option<Vec<Input>> {
        let mut inputs = Vec::new();
        let mut any = false;
        for redir in redirs {
            if let Some(body) = &redir.body {
                any = true;
                inputs.push(Input {
                    value: body.clone(),
                    tree: false,
                });
            } else if redir.op == "<<<" {
                any = true;
                if let Some(target) = &redir.target {
                    for fields in self.fields(target, st) {
                        let text = fields.join(" ");
                        inputs.extend(text.split_whitespace().map(|v| Input {
                            value: v.to_string(),
                            tree: false,
                        }));
                    }
                }
            } else if let Some(target) = &redir.target {
                // A redirection target can hold a substitution that runs.
                let _ = self.fields(target, st);
            }
        }
        any.then_some(inputs)
    }

    /// What `node` writes to its output, as far as the reader can tell: a
    /// `find` lists its trees, `ls` its directories' entries, `echo` and
    /// `printf` their words.
    fn producer_values(&mut self, node: &Node, st: &State) -> Vec<Input> {
        let mut out = Vec::new();
        let mut simples = Vec::new();
        collect_simples(node, &mut simples);
        for (words, _) in simples {
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
                    "echo" => {
                        out.extend(operands.flat_map(|a| a.split_whitespace()).map(|v| Input {
                            value: v.to_string(),
                            tree: false,
                        }));
                    }
                    "printf" => {
                        if let Some(text) = printf(&argv[1..]) {
                            out.extend(
                                text.split(|c: char| c.is_whitespace() || c == '\0')
                                    .filter(|v| !v.is_empty())
                                    .map(|v| Input {
                                        value: v.to_string(),
                                        tree: false,
                                    }),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        out.truncate(MAX_VALUES);
        out
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        None => ["~".to_string()].into(),
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

/// The script a shell runs with `-c` (or a flag cluster holding `c`).
fn shell_script(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|a| {
        a == "-c"
            || a == "--command"
            || (a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
    })?;
    args[at + 1..]
        .iter()
        .find(|a| !a.starts_with('-') || a.as_str() == "-")
        .map(String::as_str)
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
                shell_script(&run.argv[1..]).unwrap_or_default().to_string()
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

fn collect_simples(node: &Node, out: &mut Vec<(Vec<Word>, Vec<Redir>)>) {
    match node {
        Node::Simple(words, redirs) => out.push((words.clone(), redirs.clone())),
        Node::Sub(body) | Node::Func(_, body) | Node::For(_, _, body) => collect_simples(body, out),
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

/// Whether a loop body may leave early with `break`.
fn may_break(body: &Node) -> bool {
    let mut simples = Vec::new();
    collect_simples(body, &mut simples);
    simples
        .iter()
        .any(|(words, _)| words.first().and_then(Word::plain) == Some("break"))
}

/// Whether a loop body may set `name`, or runs something the reader cannot
/// see into (`eval`, `source`, a function).
fn may_assign(body: &Node, name: &str) -> bool {
    let mut simples = Vec::new();
    collect_simples(body, &mut simples);
    let mut nested = false;
    walk_for(body, &mut |var| nested |= var == name);
    nested
        || simples.iter().any(|(words, _)| {
            words.iter().any(|w| {
                w.assignment().is_some_and(|(n, _)| n == name)
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
                            )
                    })
            })
        })
        || matches!(body, Node::Func(..))
}

fn walk_for(node: &Node, visit: &mut impl FnMut(&str)) {
    match node {
        Node::For(var, _, body) => {
            visit(var);
            walk_for(body, visit);
        }
        Node::Sub(body) | Node::Func(_, body) => walk_for(body, visit),
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

/// What a substitution prints, when it is `echo`, `printf` or `pwd` of
/// words the reader knows; trailing newlines are dropped, as the shell
/// does.
fn static_output(script: &str, st: &State) -> Option<Values> {
    let mut node = Parser::parse(script);
    let words = loop {
        match node {
            Node::Seq(mut items) if items.len() == 1 => node = items.remove(0),
            Node::Simple(words, _) => break words,
            _ => return None,
        }
    };
    let mut argv = Vec::new();
    for word in &words {
        let mut text = String::new();
        for part in &word.parts {
            match part {
                Part::Lit { text: t, .. } => text.push_str(t),
                Part::Tilde(_) => text.push('~'),
                _ => return None,
            }
        }
        argv.push(text);
    }
    let run = unwrap(&argv)?;
    let argv = run.argv;
    let output = match program_name(argv.first()?).as_str() {
        "echo" => {
            let words: Vec<&str> = argv[1..]
                .iter()
                .skip_while(|a| matches!(a.as_str(), "-n" | "-e" | "-E"))
                .map(String::as_str)
                .collect();
            words.join(" ")
        }
        "printf" => printf(&argv[1..])?,
        "pwd" => return Some(st.cwd.iter().map(|c| display_dir(c)).collect()),
        _ => return None,
    };
    Some([output.trim_end_matches('\n').to_string()].into())
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

fn has_brace(text: &str) -> bool {
    text.contains('{') && text.contains(',') && text.contains('}')
}

/// Brace expansion of the unquoted literal pieces (`/{etc,usr}`); each
/// result keeps its pieces.
fn brace_expand(pieces: &[(String, bool)]) -> Vec<Vec<(String, bool)>> {
    let mut out: Vec<Vec<(String, bool)>> = vec![Vec::new()];
    for (text, splittable) in pieces {
        let options: Vec<(String, bool)> = if *splittable && has_brace(text) {
            expand_braces(text)
                .into_iter()
                .map(|t| (t, false))
                .collect()
        } else {
            vec![(text.clone(), *splittable)]
        };
        let mut next = Vec::new();
        for prefix in &out {
            for option in &options {
                let mut extended = prefix.clone();
                extended.push(option.clone());
                next.push(extended);
            }
        }
        next.truncate(MAX_VALUES);
        out = next;
    }
    out
}

/// `a{b,c}d` to `abd` and `acd`, for one level of comma lists.
fn expand_braces(text: &str) -> Vec<String> {
    let Some(open) = text.find('{') else {
        return vec![text.to_string()];
    };
    let Some(close) = text[open..].find('}').map(|c| open + c) else {
        return vec![text.to_string()];
    };
    let inner = &text[open + 1..close];
    if !inner.contains(',') {
        return vec![text.to_string()];
    }
    let (head, tail) = (&text[..open], &text[close + 1..]);
    inner
        .split(',')
        .flat_map(|choice| expand_braces(&format!("{head}{choice}{tail}")))
        .take(MAX_VALUES)
        .collect()
}

/// Join an alternative's pieces into fields: splittable pieces break on
/// blanks, and an unquoted expansion that is empty makes no field.
fn split_fields(pieces: &[(String, bool)]) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut started = false;
    for (text, splittable) in pieces {
        if *splittable {
            let mut first = true;
            for piece in text.split(char::is_whitespace) {
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
        } else {
            current.push_str(text);
            started = true;
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

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
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
        COMPOSED_PAIRS, NESTINGS, PROJECT_DELETIONS, REVIEW_PROBES, REVIEW_SYMLINK_PROBES,
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
}
