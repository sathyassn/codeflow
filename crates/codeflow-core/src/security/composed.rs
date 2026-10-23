//! Layer 1 of the composed-text check (TSK-302, review rounds 10 and 11):
//! the text a shell builds on the same line, read closed-world.
//!
//! A line can compose a path from pieces the guard only sees apart: an
//! assignment and a later expansion, a `printf` format and its arguments,
//! an `echo` or zsh `print` that decodes escapes, an array joined by `IFS`.
//! This module reads the line as bash and as zsh read it and computes those
//! texts. It is closed-world: a value is resolved only when every input is
//! a literal parsed exactly through a construct modelled here, and the
//! result equals what that shell produces. Anything else that feeds such a
//! value is unresolved, named by its construct family, and the caller
//! refuses it as cannot-verify on a line where a shell or interpreter runs.
//! It never falls back to the raw spelling.
//!
//! What is modelled, per shell:
//!
//! - words: single and double quotes, backslash escapes, `$'...'` with the
//!   escapes both shells decode alike, adjacent pieces joined;
//! - parameters: `$name` and `${name}` of a value the line assigned, its
//!   arrays through `${a[*]}`, `${a[@]}` and a literal index (bash from 0,
//!   zsh from 1), the zsh join flag `${(j:sep:)a}`, and bash indirection
//!   `${!name}` to a value the line assigned; splitting and globbing of an
//!   unquoted expansion only where they cannot change the text;
//! - assignments: standalone `name=word` and `name+=word`, `name=(...)`, the
//!   operands of `export`, `declare`, `typeset` and `readonly` without
//!   options, zsh `local`, `printf -v`, each value resolved from what the
//!   line already holds; persistence through `;`, `&&`, `||`, pipelines,
//!   subshells, groups and `eval` of literal text;
//! - output: the `printf` builtin with `%s`, `%b`, `%q` of a plain word and
//!   `%%`, the format reused while arguments remain; the `echo` builtin and
//!   its `-n`/`-e`/`-E` option clusters; zsh `print` with `-r`, `-n`, `-l`;
//!   the escapes each decodes (octal and hex below 0x80, the C letters);
//! - nested code: `eval` of literal text, a `bash`, `zsh` or `sh` `-c` body
//!   and a heredoc or here-string a shell reads, with the variables it
//!   inherits and the positional parameters its parent passes;
//! - loops: a `for` variable takes each value its list gives, the line read
//!   once per value; a glob in the list stands for outside names;
//! - command substitution whose body runs only the modelled builtins, its
//!   trailing newlines removed, and arithmetic over decimal integers.
//!
//! Unresolved, by family: an assignment whose value holds a construct
//! outside that list, a declaration builtin with options, an array element
//! assignment, an assignment a compound command, a pipeline or a
//! short-circuit may or may not make, an `echo` option cluster that mixes
//! `-e` and `-E`, a `printf` conversion other than those above, an escape
//! outside the decoded set (`\u`, `\U`, `\c`, a byte above 0x7f or NUL), an
//! indirect expansion that does not name an assigned value, a parameter
//! operator on an assigned name, `read` from text the line spells, and a
//! construct whose meaning a change the line makes leaves unknowable: an
//! output builtin a function, alias or disable redefines, `echo` after an
//! option that changes its decoding, an expansion after one that changes
//! word expansion.
//!
//! Text from outside the line is unknown text (`$` in the judged value),
//! judged by the trailing-component rule as before: a variable the line
//! never assigns, a positional parameter it does not set, a name a glob or
//! `find` supplies, and the output of any program other than the modelled
//! builtins (operator boundary, 2026-10-15: the guard models shell grammar;
//! text a program produces at run time is the stated residual), with what
//! `read` takes from it. A program runs as itself whether named bare or by
//! a path; only `printf` and `echo` by path are read, in their external
//! form. A word that joins a substitution's output with text the line
//! spells is a composition and is judged; a glob argument stands for
//! outside names and is judged by its pattern, whole, as a glob argument
//! to any program is, so a glob an output builtin writes refuses when it
//! can match a protected path, in any field, since the separators around
//! a field that may vanish still separate it when judged; since zsh does
//! not run a command whose glob matches nothing, that command's output is
//! judged but not claimed. Code from outside the line (`source`, `eval` of
//! outside text) is that residual too: what it may change keeps its value
//! for judging, but an output built from such a value is not claimed exact.
//! The differential test `tests/shell_resolution_differential.rs` runs the
//! host's bash and zsh over a corpus and checks each resolved value byte
//! for byte.

use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod paths;

/// The shell grammar a line is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// GNU bash (3.2 and later) outside POSIX mode.
    Bash,
    /// zsh with its default options.
    Zsh,
    /// A POSIX `sh` (dash, bash in POSIX mode): only the common subset.
    Sh,
}

/// One piece of a resolved value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Text the line spells, after the shell's decoding.
    Text(String),
    /// The text of a variable the line never assigns.
    Open,
    /// The names an unquoted glob matches: unknown text, judged by its
    /// pattern.
    Glob(String),
    /// A separator beside a field that may expand to nothing: for
    /// exactness it may be absent, but it always separates when judged.
    Soft(String),
}

/// A value the line composes: literal text and unknown outside text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Value(Vec<Piece>);

impl Value {
    fn text(text: &str) -> Self {
        let mut value = Self::default();
        value.push_text(text);
        value
    }

    fn open() -> Self {
        Self(vec![Piece::Open])
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Piece::Text(last)) = self.0.last_mut() {
            last.push_str(text);
        } else {
            self.0.push(Piece::Text(text.to_string()));
        }
    }

    fn append(&mut self, other: &Self) {
        for piece in &other.0 {
            match piece {
                Piece::Text(text) => self.push_text(text),
                Piece::Open => {
                    if self.0.last() != Some(&Piece::Open) {
                        self.0.push(Piece::Open);
                    }
                }
                Piece::Glob(pattern) => self.0.push(Piece::Glob(pattern.clone())),
                Piece::Soft(sep) => self.0.push(Piece::Soft(sep.clone())),
            }
        }
    }

    /// The pieces, in order.
    #[must_use]
    pub fn pieces(&self) -> &[Piece] {
        &self.0
    }

    /// The text, when no piece is unknown.
    #[must_use]
    pub fn exact(&self) -> Option<String> {
        let mut out = String::new();
        for piece in &self.0 {
            match piece {
                Piece::Text(text) => out.push_str(text),
                Piece::Open | Piece::Glob(_) | Piece::Soft(_) => return None,
            }
        }
        Some(out)
    }

    /// The text the guard judges: an unknown piece is written `$`, which
    /// the trailing-component rule reads as a part filled in at run time;
    /// a glob is written as its pattern, which that rule matches against
    /// the protected paths, as it does a glob argument.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for piece in &self.0 {
            match piece {
                Piece::Text(text) | Piece::Glob(text) | Piece::Soft(text) => out.push_str(text),
                Piece::Open => out.push('$'),
            }
        }
        out
    }

    /// The value with each unknown piece empty: the reading when outside
    /// text or a program's output is empty, which joins the spelled
    /// pieces around it. Globs and separators stay.
    #[must_use]
    pub fn without_open(&self) -> Self {
        let mut out = Self::default();
        for piece in &self.0 {
            match piece {
                Piece::Text(text) => out.push_text(text),
                Piece::Open => {}
                other => out.0.push(other.clone()),
            }
        }
        out
    }

    /// Each glob in the value as the whole word it sits in, with the text
    /// around it up to a blank, quote or shell operator: the pattern the
    /// guard judges as it judges a glob argument.
    #[must_use]
    pub fn glob_words(&self) -> Vec<String> {
        let breaks = |c: char| c.is_ascii_whitespace() || "'\"`(){};,|&<>".contains(c);
        let mut words = Vec::new();
        for (at, piece) in self.0.iter().enumerate() {
            let Piece::Glob(pattern) = piece else {
                continue;
            };
            let mut left = String::new();
            for before in self.0[..at].iter().rev() {
                let text = match before {
                    Piece::Text(t) | Piece::Glob(t) | Piece::Soft(t) => t.clone(),
                    Piece::Open => "$".to_string(),
                };
                let kept: String = text.chars().rev().take_while(|c| !breaks(*c)).collect();
                let whole = kept.chars().count() == text.chars().count();
                left = kept.chars().rev().collect::<String>() + &left;
                if !whole {
                    break;
                }
            }
            let mut right = String::new();
            for after in &self.0[at + 1..] {
                let text = match after {
                    Piece::Text(t) | Piece::Glob(t) | Piece::Soft(t) => t.clone(),
                    Piece::Open => "$".to_string(),
                };
                let kept: String = text.chars().take_while(|c| !breaks(*c)).collect();
                let whole = kept.chars().count() == text.chars().count();
                right.push_str(&kept);
                if !whole {
                    break;
                }
            }
            for word in format!("{left}{pattern}{right}").split(breaks) {
                if !word.is_empty() && !words.iter().any(|w| w == word) {
                    words.push(word.to_string());
                }
            }
        }
        words
    }

    /// Whether `text` is one the value can stand for: each unknown piece
    /// matches any run of characters.
    #[must_use]
    pub fn matches(&self, text: &str) -> bool {
        fn at(pieces: &[Piece], text: &str) -> bool {
            match pieces.split_first() {
                None => text.is_empty(),
                Some((Piece::Text(head), rest)) => text
                    .strip_prefix(head.as_str())
                    .is_some_and(|tail| at(rest, tail)),
                Some((Piece::Soft(sep), rest)) => {
                    at(rest, text)
                        || text
                            .strip_prefix(sep.as_str())
                            .is_some_and(|tail| at(rest, tail))
                }
                Some((Piece::Open | Piece::Glob(_), rest)) => text
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(text.len()))
                    .any(|i| at(rest, &text[i..])),
            }
        }
        at(&self.0, text)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn has_open(&self) -> bool {
        self.0
            .iter()
            .any(|piece| matches!(piece, Piece::Open | Piece::Glob(_)))
    }

    fn any_text(&self, test: impl Fn(char) -> bool) -> bool {
        self.0.iter().any(|piece| match piece {
            Piece::Text(chunk) => chunk.chars().any(&test),
            Piece::Open | Piece::Glob(_) | Piece::Soft(_) => false,
        })
    }
}

/// What layer 1 reads from one construct on the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Composed {
    /// The text the construct builds.
    Resolved(Value),
    /// A construct the guard cannot resolve exactly, named by its family.
    Unresolved(&'static str),
}

/// Every composed text on the line, read as bash and as zsh read it: what
/// each `printf`, `echo` and `print` writes, each word or assignment value
/// built from a variable the line assigns or from a decoded `$'...'`, and
/// each heredoc body built that way. The caller judges these only where a
/// shell or interpreter runs.
pub(crate) fn composed_texts(line: &str) -> Vec<Composed> {
    let mut found = Vec::new();
    for dialect in [Dialect::Bash, Dialect::Zsh] {
        for event in events(line, dialect) {
            if !found.contains(&event.value) {
                found.push(event.value);
            }
        }
    }
    found
}

/// What the last top-level `printf`, `echo` or `print` on the line writes
/// in `dialect`, or `None` when the line has none the dialect models. For
/// the differential test.
#[doc(hidden)]
#[must_use]
pub fn final_output(line: &str, dialect: Dialect) -> Option<Composed> {
    events(line, dialect)
        .into_iter()
        .rev()
        .find(|event| event.output && event.depth == 0)
        .map(|event| {
            if event.exact {
                event.value
            } else {
                Composed::Unresolved("an output the guard judges but does not claim exact")
            }
        })
}

/// Every composed text on the line in one dialect.
#[doc(hidden)]
#[must_use]
pub fn texts_in(line: &str, dialect: Dialect) -> Vec<Composed> {
    events(line, dialect).into_iter().map(|e| e.value).collect()
}

fn events(line: &str, dialect: Dialect) -> Vec<Event> {
    let mut events = Vec::new();
    let mut run = new_run(dialect, State::default(), &mut events, 0, 0);
    run.run_text(line);
    events
}

// ---------------------------------------------------------------------------
// Lexing
// ---------------------------------------------------------------------------

/// One piece of a shell word, as written.
#[derive(Clone, Debug)]
enum Part {
    /// Literal text; `quoted` when quoting or a backslash protects it.
    Lit { text: String, quoted: bool },
    /// The body of `$'...'`, escapes undecoded.
    Ansi(String),
    /// A parameter expansion: the name or the text inside `${...}`.
    Param {
        body: String,
        braced: bool,
        quoted: bool,
        /// zsh reads a subscript or a modifier after this unbraced name.
        zsh_tail: bool,
    },
    /// Command or process substitution, with its body.
    Subst { body: String, quoted: bool },
    /// Process substitution: a path the shell makes up.
    ProcSubst(String),
    /// `$((...))` or `$[...]`, with its text.
    Arith { body: String, quoted: bool },
    /// A construct read differently by the shells, never resolved.
    Unmodelled(&'static str),
}

#[derive(Clone, Debug, Default)]
struct Word {
    parts: Vec<Part>,
    raw: String,
}

impl Word {
    /// The word's text when it is one plain unquoted literal.
    fn plain(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [Part::Lit {
                text,
                quoted: false,
            }] => Some(text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum Tok {
    Word(Word),
    /// `name=(...)` or `name+=(...)`.
    Array {
        name: String,
        append: bool,
        elements: Vec<Word>,
    },
    /// A redirection: its operator, its target word, or its heredoc.
    Redir {
        op: String,
        target: Option<Word>,
        heredoc: Option<usize>,
    },
    Op(&'static str),
    /// `(( ... ))`.
    ArithCmd(String),
}

#[derive(Clone, Debug)]
struct Heredoc {
    strip_tabs: bool,
    quoted: bool,
    delimiter: String,
    body: Word,
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    toks: Vec<Tok>,
    heredocs: Vec<Heredoc>,
    pending: Vec<usize>,
}

type Lexed = Result<(Vec<Tok>, Vec<Heredoc>), &'static str>;

const GRAMMAR: &str = "shell grammar the guard does not read";

fn lex(text: &str) -> Lexed {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        at: 0,
        toks: Vec::new(),
        heredocs: Vec::new(),
        pending: Vec::new(),
    };
    lexer.run()?;
    Ok((lexer.toks, lexer.heredocs))
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.at + offset).copied()
    }

    fn starts(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
    }

    fn run(&mut self) -> Result<(), &'static str> {
        loop {
            while matches!(self.peek(), Some(' ' | '\t')) || self.starts("\\\n") {
                self.at += if self.peek() == Some('\\') { 2 } else { 1 };
            }
            let Some(c) = self.peek() else {
                break;
            };
            match c {
                '#' => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                '\n' => {
                    self.at += 1;
                    self.toks.push(Tok::Op("\n"));
                    self.read_heredoc_bodies()?;
                }
                ';' | '&' | '|' | ')' => self.operator(),
                '(' => {
                    if self.peek_at(1) == Some('(') {
                        self.at += 2;
                        let body = self.balanced(2)?;
                        let body = body.strip_suffix(')').unwrap_or(&body).to_string();
                        self.toks.push(Tok::ArithCmd(body));
                    } else {
                        self.at += 1;
                        self.toks.push(Tok::Op("("));
                    }
                }
                '<' | '>' if self.peek_at(1) == Some('(') => {
                    self.at += 2;
                    let body = self.balanced(1)?;
                    let mut word = Word::default();
                    word.parts.push(Part::ProcSubst(body));
                    self.toks.push(Tok::Word(word));
                }
                '<' | '>' => self.redirection()?,
                d if d.is_ascii_digit() && self.fd_redirection() => self.redirection()?,
                _ => {
                    let tok = self.read_word()?;
                    self.toks.push(tok);
                }
            }
        }
        if self.pending.is_empty() {
            Ok(())
        } else {
            // A heredoc whose body never starts: the shell reads none.
            self.read_heredoc_bodies()
        }
    }

    fn operator(&mut self) {
        for op in [
            ";;&", ";;", ";&", ";|", "&&", "||", "|&", ";", "&", "|", ")",
        ] {
            if op == "&" && (self.starts("&>")) {
                continue;
            }
            if self.starts(op) {
                self.at += op.chars().count();
                self.toks.push(Tok::Op(op));
                return;
            }
        }
        // `&>` and `&>>` redirect.
        let _ = self.redirection();
    }

    fn fd_redirection(&self) -> bool {
        let mut i = 0;
        while self.peek_at(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        matches!(self.peek_at(i), Some('<' | '>')) && self.peek_at(i + 1) != Some('(')
    }

    fn redirection(&mut self) -> Result<(), &'static str> {
        let mut op = String::new();
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            op.push(self.peek().unwrap_or_default());
            self.at += 1;
        }
        let ops = [
            "<<<", "<<-", "<<", "<>", "<&", "<", ">>", ">|", ">&", ">", "&>>", "&>",
        ];
        let Some(found) = ops.iter().find(|candidate| self.starts(candidate)) else {
            return Err(GRAMMAR);
        };
        self.at += found.len();
        op.push_str(found);
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.at += 1;
        }
        if *found == "<<" || *found == "<<-" {
            let start = self.at;
            let Tok::Word(_) = self.read_word()? else {
                return Err(GRAMMAR);
            };
            let raw: String = self.chars[start..self.at].iter().collect();
            if raw.is_empty() {
                return Err(GRAMMAR);
            }
            let quoted = raw.contains(['\'', '"', '\\']);
            let delimiter: String = raw
                .chars()
                .filter(|c| !matches!(c, '\'' | '"' | '\\'))
                .collect();
            self.heredocs.push(Heredoc {
                strip_tabs: *found == "<<-",
                quoted,
                delimiter,
                body: Word::default(),
            });
            let index = self.heredocs.len() - 1;
            self.pending.push(index);
            self.toks.push(Tok::Redir {
                op,
                target: None,
                heredoc: Some(index),
            });
            return Ok(());
        }
        match self.read_word()? {
            Tok::Word(word) if !word.raw.is_empty() => {
                self.toks.push(Tok::Redir {
                    op,
                    target: Some(word),
                    heredoc: None,
                });
                Ok(())
            }
            _ => Err(GRAMMAR),
        }
    }

    fn read_heredoc_bodies(&mut self) -> Result<(), &'static str> {
        for index in std::mem::take(&mut self.pending) {
            let (strip, delimiter, quoted) = {
                let doc = &self.heredocs[index];
                (doc.strip_tabs, doc.delimiter.clone(), doc.quoted)
            };
            let mut text = String::new();
            loop {
                if self.at >= self.chars.len() {
                    break;
                }
                let mut line = String::new();
                while let Some(c) = self.peek() {
                    self.at += 1;
                    if c == '\n' {
                        break;
                    }
                    line.push(c);
                }
                let line = if strip {
                    line.chars().skip_while(|c| *c == '\t').collect()
                } else {
                    line
                };
                if line == delimiter {
                    break;
                }
                text.push_str(&line);
                text.push('\n');
            }
            self.heredocs[index].body = if quoted {
                let mut word = Word::default();
                word.parts.push(Part::Lit {
                    text: text.clone(),
                    quoted: true,
                });
                word.raw = text;
                word
            } else {
                let mut inner = Lexer {
                    chars: text.chars().collect(),
                    at: 0,
                    toks: Vec::new(),
                    heredocs: Vec::new(),
                    pending: Vec::new(),
                };
                let mut word = Word::default();
                inner.double_quoted(&mut word.parts, true)?;
                word.raw = text;
                word
            };
        }
        Ok(())
    }

    /// Read to the parenthesis that closes `open` already-open ones,
    /// honouring quotes; the text inside, without the closers.
    fn balanced(&mut self, open: usize) -> Result<String, &'static str> {
        let mut depth = open;
        let mut body = String::new();
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.at += 1;
            match (quote, c) {
                (Some('\''), '\'') | (Some('"'), '"') => quote = None,
                (Some('"') | None, '\\') => {
                    body.push(c);
                    if let Some(next) = self.peek() {
                        body.push(next);
                        self.at += 1;
                    }
                    continue;
                }
                (None, '\'' | '"') => quote = Some(c),
                (None, '(') => depth += 1,
                (None, ')') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(body);
                    }
                }
                _ => {}
            }
            body.push(c);
        }
        Err(GRAMMAR)
    }

    fn read_word(&mut self) -> Result<Tok, &'static str> {
        let start = self.at;
        let mut parts: Vec<Part> = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | ')' => break,
                '(' => {
                    if let Some(tok) = self.array(&parts)? {
                        return Ok(tok);
                    }
                    // `name()`: a function definition, the name one word.
                    let named = matches!(parts.as_slice(), [Part::Lit { text, quoted: false }] if is_identifier(text) || text.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c)));
                    if named && self.peek_at(1) == Some(')') {
                        break;
                    }
                    return Err("a parenthesis inside a word");
                }
                '\'' => {
                    self.at += 1;
                    let mut text = String::new();
                    loop {
                        match self.peek() {
                            None => return Err(GRAMMAR),
                            Some('\'') => break,
                            Some(c) => text.push(c),
                        }
                        self.at += 1;
                    }
                    self.at += 1;
                    parts.push(Part::Lit { text, quoted: true });
                }
                '"' => {
                    self.at += 1;
                    self.double_quoted(&mut parts, false)?;
                }
                '\\' => {
                    self.at += 1;
                    match self.peek() {
                        Some('\n') => self.at += 1,
                        Some(next) => {
                            self.at += 1;
                            parts.push(Part::Lit {
                                text: next.to_string(),
                                quoted: true,
                            });
                        }
                        None => parts.push(Part::Lit {
                            text: "\\".into(),
                            quoted: false,
                        }),
                    }
                }
                '$' => self.dollar(&mut parts, false)?,
                '`' => {
                    self.at += 1;
                    let body = self.backtick(false)?;
                    parts.push(Part::Subst {
                        body,
                        quoted: false,
                    });
                }
                _ => {
                    self.at += 1;
                    push_lit(&mut parts, c, false);
                }
            }
        }
        let raw = self.chars[start..self.at].iter().collect();
        Ok(Tok::Word(Word { parts, raw }))
    }

    /// `name=(` or `name+=(`: the elements up to the closing `)`.
    fn array(&mut self, parts: &[Part]) -> Result<Option<Tok>, &'static str> {
        let [Part::Lit {
            text,
            quoted: false,
        }] = parts
        else {
            return Ok(None);
        };
        let (name, append) = if let Some(name) = text.strip_suffix("+=") {
            (name, true)
        } else if let Some(name) = text.strip_suffix('=') {
            (name, false)
        } else {
            return Ok(None);
        };
        if !is_identifier(name) {
            return Ok(None);
        }
        let name = name.to_string();
        self.at += 1;
        let mut elements = Vec::new();
        loop {
            while matches!(self.peek(), Some(' ' | '\t' | '\n')) {
                self.at += 1;
            }
            match self.peek() {
                None | Some('#') => return Err(GRAMMAR),
                Some(')') => {
                    self.at += 1;
                    break;
                }
                Some(_) => match self.read_word()? {
                    Tok::Word(word) if !word.raw.is_empty() => elements.push(word),
                    _ => return Err(GRAMMAR),
                },
            }
        }
        if self
            .peek()
            .is_some_and(|c| !matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | ')'))
        {
            return Err(GRAMMAR);
        }
        Ok(Some(Tok::Array {
            name,
            append,
            elements,
        }))
    }

    /// The inside of a double-quoted string, or of an unquoted heredoc body
    /// (`heredoc`: a `"` is text and runs to the end).
    fn double_quoted(&mut self, parts: &mut Vec<Part>, heredoc: bool) -> Result<(), &'static str> {
        let entry = parts.len();
        loop {
            let Some(c) = self.peek() else {
                return if heredoc { Ok(()) } else { Err(GRAMMAR) };
            };
            match c {
                '"' if !heredoc => {
                    self.at += 1;
                    // An empty "" still makes a word.
                    if parts.len() == entry {
                        parts.push(Part::Lit {
                            text: String::new(),
                            quoted: true,
                        });
                    }
                    return Ok(());
                }
                '\\' => {
                    self.at += 1;
                    match self.peek() {
                        Some('\n') => self.at += 1,
                        Some(next @ ('$' | '`' | '\\')) => {
                            self.at += 1;
                            push_lit(parts, next, true);
                        }
                        Some('"') if !heredoc => {
                            self.at += 1;
                            push_lit(parts, '"', true);
                        }
                        _ => push_lit(parts, '\\', true),
                    }
                }
                '$' => self.dollar(parts, true)?,
                '`' => {
                    self.at += 1;
                    let body = self.backtick(true)?;
                    parts.push(Part::Subst { body, quoted: true });
                }
                _ => {
                    self.at += 1;
                    push_lit(parts, c, true);
                }
            }
        }
    }

    fn backtick(&mut self, quoted: bool) -> Result<String, &'static str> {
        let mut body = String::new();
        loop {
            match self.peek() {
                None => return Err(GRAMMAR),
                Some('`') => {
                    self.at += 1;
                    return Ok(body);
                }
                Some('\\') => {
                    self.at += 1;
                    match self.peek() {
                        Some(next @ ('$' | '`' | '\\')) => body.push(next),
                        Some('"') if quoted => body.push('"'),
                        Some(next) => {
                            body.push('\\');
                            body.push(next);
                        }
                        None => return Err(GRAMMAR),
                    }
                    self.at += 1;
                }
                Some(c) => {
                    body.push(c);
                    self.at += 1;
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one dispatch over the `$` forms
    fn dollar(&mut self, parts: &mut Vec<Part>, quoted: bool) -> Result<(), &'static str> {
        self.at += 1;
        match self.peek() {
            Some('{') => {
                self.at += 1;
                let body = self.braced()?;
                parts.push(Part::Param {
                    body,
                    braced: true,
                    quoted,
                    zsh_tail: false,
                });
            }
            Some('(') => {
                self.at += 1;
                let body = self.balanced(1)?;
                if body.starts_with('(') {
                    parts.push(Part::Arith { body, quoted });
                } else {
                    parts.push(Part::Subst { body, quoted });
                }
            }
            Some('[') => {
                self.at += 1;
                let mut body = String::new();
                loop {
                    match self.peek() {
                        None => return Err(GRAMMAR),
                        Some(']') => break,
                        Some(c) => body.push(c),
                    }
                    self.at += 1;
                }
                self.at += 1;
                parts.push(Part::Arith { body, quoted });
            }
            Some('\'') if !quoted => {
                self.at += 1;
                let mut body = String::new();
                loop {
                    match self.peek() {
                        None => return Err(GRAMMAR),
                        Some('\'') => break,
                        Some('\\') => {
                            body.push('\\');
                            self.at += 1;
                            match self.peek() {
                                Some(next) => body.push(next),
                                None => return Err(GRAMMAR),
                            }
                        }
                        Some(c) => body.push(c),
                    }
                    self.at += 1;
                }
                self.at += 1;
                parts.push(Part::Ansi(body));
            }
            Some('"') if !quoted => {
                self.at += 1;
                let mut inner = Vec::new();
                self.double_quoted(&mut inner, false)?;
                parts.push(Part::Unmodelled("locale quoting"));
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let zsh_tail = match self.peek() {
                    Some('[') => true,
                    Some(':') => self
                        .peek_at(1)
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '&'),
                    _ => false,
                };
                parts.push(Part::Param {
                    body: name,
                    braced: false,
                    quoted,
                    zsh_tail,
                });
            }
            // zsh reads `$=a`, `$~a`, `$^a`, `$+a` and `$#a` as flags or a
            // length on `a`; read them as the braced form.
            Some(c @ ('=' | '~' | '^' | '+' | '#'))
                if self
                    .peek_at(1)
                    .is_some_and(|n| n.is_ascii_alphabetic() || n == '_') =>
            {
                self.at += 1;
                let mut body = c.to_string();
                while let Some(n) = self.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' {
                        body.push(n);
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                parts.push(Part::Param {
                    body,
                    braced: true,
                    quoted,
                    zsh_tail: false,
                });
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.at += 1;
                parts.push(Part::Param {
                    body: c.to_string(),
                    braced: false,
                    quoted,
                    zsh_tail: false,
                });
            }
            _ => push_lit(parts, '$', quoted),
        }
        Ok(())
    }

    /// The body of `${...}` up to its closing brace.
    fn braced(&mut self) -> Result<String, &'static str> {
        let mut depth = 1;
        let mut body = String::new();
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.at += 1;
            match (quote, c) {
                (Some('\''), '\'') | (Some('"'), '"') => quote = None,
                (Some('"') | None, '\\') => {
                    body.push(c);
                    if let Some(next) = self.peek() {
                        body.push(next);
                        self.at += 1;
                    }
                    continue;
                }
                (None, '\'' | '"') => quote = Some(c),
                (None, '{') => depth += 1,
                (None, '}') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(body);
                    }
                }
                _ => {}
            }
            body.push(c);
        }
        Err(GRAMMAR)
    }
}

/// One token of shell arithmetic.
#[derive(Clone, Copy)]
enum Arith {
    Num(i64),
    Op(char),
}

/// `expr := term (('+'|'-') term)*`, with checked integer arithmetic.
fn arith_expr(tokens: &[Arith], at: &mut usize) -> Option<i64> {
    let mut value = arith_term(tokens, at)?;
    while let Some(Arith::Op(op @ ('+' | '-'))) = tokens.get(*at) {
        *at += 1;
        let rhs = arith_term(tokens, at)?;
        value = if *op == '+' {
            value.checked_add(rhs)?
        } else {
            value.checked_sub(rhs)?
        };
    }
    Some(value)
}

/// `term := unary (('*'|'/'|'%') unary)*`; division truncates, as C does.
fn arith_term(tokens: &[Arith], at: &mut usize) -> Option<i64> {
    let mut value = arith_unary(tokens, at)?;
    while let Some(Arith::Op(op @ ('*' | '/' | '%'))) = tokens.get(*at) {
        *at += 1;
        let rhs = arith_unary(tokens, at)?;
        value = match op {
            '*' => value.checked_mul(rhs)?,
            '/' => value.checked_div(rhs)?,
            _ => value.checked_rem(rhs)?,
        };
    }
    Some(value)
}

fn arith_unary(tokens: &[Arith], at: &mut usize) -> Option<i64> {
    match tokens.get(*at)? {
        Arith::Op('-') => {
            *at += 1;
            arith_unary(tokens, at)?.checked_neg()
        }
        Arith::Op('+') => {
            *at += 1;
            arith_unary(tokens, at)
        }
        Arith::Op('(') => {
            *at += 1;
            let value = arith_expr(tokens, at)?;
            matches!(tokens.get(*at), Some(Arith::Op(')'))).then(|| *at += 1)?;
            Some(value)
        }
        Arith::Num(n) => {
            *at += 1;
            Some(*n)
        }
        Arith::Op(_) => None,
    }
}

/// Unquoted text holding a glob: it stands for the names it matches,
/// outside text, and is judged by its pattern. Text without a glob
/// construct stays text.
fn push_glob(value: &mut Value, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let globs = chars
        .iter()
        .enumerate()
        .any(|(at, c)| *c == '*' || *c == '?' || (*c == '[' && chars[at + 1..].contains(&']')));
    if globs {
        value.0.push(Piece::Glob(text.to_string()));
    } else {
        value.push_text(text);
    }
}

fn push_lit(parts: &mut Vec<Part>, c: char, quoted: bool) {
    if let Some(Part::Lit { text, quoted: q }) = parts.last_mut() {
        if *q == quoted {
            text.push(c);
            return;
        }
    }
    parts.push(Part::Lit {
        text: c.to_string(),
        quoted,
    });
}

// ---------------------------------------------------------------------------
// Escapes
// ---------------------------------------------------------------------------

/// Which escape grammar a text is decoded by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escapes {
    /// `$'...'`.
    Ansi,
    /// A `printf` format.
    Format,
    /// A `printf` `%b` argument, as `dialect` decodes it.
    PercentB(Dialect),
    /// `echo` with decoding on, in bash and zsh.
    Echo,
    /// zsh `print` without `-r`.
    Print,
}

const UNDECODED: &str = "an escape the guard does not decode";

/// Decode `text` by `escapes`. Only the escapes every supported version of
/// the shell decodes alike are modelled; any other is unresolved.
fn decode(text: &str, escapes: Escapes) -> Result<String, &'static str> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = chars.get(i) else {
            return Err(UNDECODED);
        };
        i += 1;
        let letter = match e {
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            'f' => Some('\u{c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\u{b}'),
            '\\' => Some('\\'),
            'e' | 'E' if escapes != Escapes::Echo => Some('\u{1b}'),
            '\'' | '"' if escapes == Escapes::Ansi => Some(e),
            _ => None,
        };
        if let Some(letter) = letter {
            out.push(letter);
            continue;
        }
        let (radix, digits) = match (e, escapes) {
            ('x', _) => (16, 2),
            ('0', Escapes::Echo | Escapes::PercentB(_)) => (8, 3),
            ('0'..='7', Escapes::Ansi | Escapes::Format | Escapes::Print)
            | ('1'..='7', Escapes::PercentB(Dialect::Bash)) => {
                i -= 1;
                (8, 3)
            }
            _ => return Err(UNDECODED),
        };
        let mut value: u32 = 0;
        let mut read = 0;
        while read < digits {
            let Some(d) = chars.get(i).and_then(|c| c.to_digit(radix)) else {
                break;
            };
            value = value * radix + d;
            read += 1;
            i += 1;
        }
        if radix == 16 && read == 0 {
            return Err(UNDECODED);
        }
        match value {
            0 => return Err("a NUL escape"),
            1..=0x7f => out.push(char::from_u32(value).ok_or(UNDECODED)?),
            _ => return Err("an escape above ASCII"),
        }
    }
    if escapes == Escapes::Ansi {
        // The shared reader gives the same text for the escapes allowed above.
        return Ok(crate::hooks::git_guard::ansi_c_text(&mut text.chars()));
    }
    Ok(out)
}

/// `value` decoded by `escapes`, unknown pieces kept unknown; a backslash
/// next to an unknown piece cannot be read.
fn decode_value(value: &Value, escapes: Escapes) -> Result<Value, &'static str> {
    if value.has_open() {
        if value.any_text(|c| c == '\\') {
            return Err(UNDECODED);
        }
        return Ok(value.clone());
    }
    Ok(Value::text(&decode(&value.render(), escapes)?))
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Var {
    Scalar(Value),
    Array(Vec<Value>),
    /// A `for` variable: one of these values on each pass.
    Choices(Vec<Value>),
    Unknown(&'static str),
}

/// The positional parameters: `$0`, `$1` and on.
#[derive(Clone, Debug, Default)]
enum Positional {
    /// Given from outside the line: unknown text.
    #[default]
    Outside,
    /// Set by the line: `values[0]` is `$0`; `more` when a program may
    /// pass further ones.
    Set {
        values: Vec<Value>,
        more: bool,
    },
    Unknown(&'static str),
}

#[derive(Clone, Debug, Default)]
struct State {
    /// Known working directory for filesystem consumers, independent of PWD.
    directory: Option<std::path::PathBuf>,
    vars: BTreeMap<String, Var>,
    /// Names an assignment cannot change: `readonly`, or set by a trap
    /// that may run again.
    readonly: BTreeSet<String>,
    exported: BTreeSet<String>,
    /// Functions the line defines, with the names their bodies may set.
    functions: BTreeMap<String, Vec<String>>,
    /// Aliases the line defines: their text, or `None` when unreadable.
    aliases: BTreeMap<String, Option<String>>,
    /// Values outside code (a sourced file, `eval` of outside text) may
    /// have replaced: still judged, but not claimed exact.
    changed: BTreeSet<String>,
    /// An option change that makes `echo` and `print` decoding unknown.
    opt_echo: Option<&'static str>,
    /// An option change that makes word expansion unknown.
    opt_words: Option<&'static str>,
    /// A name reference whose target the guard cannot read: any variable
    /// may change.
    world: Option<&'static str>,
    positional: Positional,
}

struct Event {
    depth: usize,
    output: bool,
    /// Whether an output's value is claimed exact: not when a value the
    /// line may have changed fed it.
    exact: bool,
    value: Composed,
}

/// The and-or list being read in one frame.
#[derive(Clone, Debug, Default)]
struct List {
    /// Commands seen in the list so far.
    commands: usize,
    after_or: bool,
    /// Names assigned by commands that may not run.
    conditional: Vec<String>,
    /// Every name the list assigned.
    all: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Frame {
    list: List,
    /// A subshell: the state to restore when it closes.
    snapshot: Option<Box<State>>,
    /// Where the assignment log stood when a group opened.
    log_at: usize,
    /// The parent's list context when the frame opened.
    parent_first: bool,
    parent_uncertain: bool,
    /// The body of a function definition.
    function_body: bool,
}

#[allow(clippy::struct_excessive_bools)] // independent facts about the command being read
struct Run<'e> {
    dialect: Dialect,
    state: State,
    events: &'e mut Vec<Event>,
    depth: usize,
    nesting: usize,
    /// Open compound commands; `usize::MAX` once a construct runs to the
    /// end of the line.
    uncertain: usize,
    frames: Vec<Frame>,
    /// Every name assigned, in order.
    log: Vec<String>,
    /// The open loops: the `for` variable of each.
    loops: Vec<Option<String>>,
    /// What a command substitution's body writes, when this run reads one.
    capture: Option<Vec<Result<Value, &'static str>>>,
    /// Whether the command being read writes to the captured stdout.
    stdout_here: bool,
    /// Whether the command being read wrote a modelled output.
    wrote: bool,
    /// Whether the command being read was silent, or a special builtin.
    silent: bool,
    special: bool,
    /// Whether a value the line may have changed fed the command.
    approx: std::cell::Cell<bool>,
    /// Whether the command has a glob argument, so may not run.
    glob_arg: bool,
    /// Open alias expansions, whose outputs are not claimed exact.
    inexact: usize,
    /// The line feeds text it spells to a program's stdin.
    stdin_literal: bool,
    /// A function definition whose `{` body opens next.
    body_pending: bool,
}

/// A run over `events` in `state`.
fn new_run(
    dialect: Dialect,
    state: State,
    events: &mut Vec<Event>,
    depth: usize,
    nesting: usize,
) -> Run<'_> {
    Run {
        dialect,
        state,
        events,
        depth,
        nesting,
        uncertain: 0,
        frames: vec![Frame::default()],
        log: Vec::new(),
        loops: Vec::new(),
        capture: None,
        stdout_here: true,
        wrote: false,
        silent: false,
        special: false,
        approx: std::cell::Cell::new(false),
        glob_arg: false,
        inexact: 0,
        stdin_literal: false,
        body_pending: false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// An argument: split, globbed, removed when empty.
    Fields,
    /// A `for` list: a glob stands for the names it matches, text from
    /// outside the line.
    List,
    /// An assignment value or heredoc body: one string.
    Assign,
}

/// A word's value and whether it uses what the line itself composes.
struct Eval {
    fields: Result<Vec<Value>, &'static str>,
    online: bool,
    /// bash splits an unquoted outside value into fields the guard cannot
    /// count.
    loose: bool,
    /// An unquoted glob: zsh does not run the command when it matches
    /// nothing.
    globbed: bool,
}

/// One parameter's value.
enum Param {
    One(Value),
    Many(Vec<Value>),
}

struct ParamEval {
    value: Result<Param, &'static str>,
    online: bool,
}

impl ParamEval {
    fn one(value: Value, online: bool) -> Self {
        Self {
            value: Ok(Param::One(value)),
            online,
        }
    }

    fn open() -> Self {
        Self::one(Value::open(), false)
    }

    fn err(reason: &'static str, online: bool) -> Self {
        Self {
            value: Err(reason),
            online,
        }
    }
}

const MAY_ASSIGN: &str = "an assignment the line may or may not make";
const SPECIAL: &[&str] = &[
    "RANDOM",
    "SRANDOM",
    "SECONDS",
    "EPOCHSECONDS",
    "EPOCHREALTIME",
    "LINENO",
    "BASHPID",
    "HISTCMD",
    "UID",
    "EUID",
    "GID",
    "EGID",
    "PPID",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "OPTIND",
    "OPTARG",
    "REPLY",
    "reply",
    "COLUMNS",
    "LINES",
    "FUNCNAME",
    "GROUPS",
    "DIRSTACK",
    "dirstack",
    "PIPESTATUS",
    "pipestatus",
    "status",
    "BASHOPTS",
    "SHELLOPTS",
    "argv",
    "ARGC",
    "path",
    "PATH",
    "fpath",
    "FPATH",
    "cdpath",
    "CDPATH",
    "manpath",
    "MANPATH",
    "mailpath",
    "MAILPATH",
    "psvar",
    "PSVAR",
    "module_path",
    "MODULE_PATH",
    "watch",
    "WATCH",
    "USERNAME",
    "MATCH",
    "match",
    "MBEGIN",
    "MEND",
    "mbegin",
    "mend",
    "BASH_REMATCH",
    "MAPFILE",
    "_",
];
/// Builtins and shell commands that assign the names they are given.
const READERS: &[&str] = &[
    "read",
    "mapfile",
    "readarray",
    "getopts",
    "vared",
    "zparseopts",
    "wait",
    "zstyle",
    "zformat",
    "strftime",
    "zstat",
    "sysread",
    "zle",
    "compadd",
    "zregexparse",
    "pcre_match",
];
/// Builtins that write nothing to stdout.
const SILENT: &[&str] = &[
    ":",
    "true",
    "false",
    "test",
    "[",
    "wait",
    "exit",
    "return",
    "break",
    "continue",
    "unset",
    "shift",
    "let",
    "unalias",
    "read",
    "mapfile",
    "readarray",
    "getopts",
];
/// POSIX special builtins and declarations: an assignment in front of one
/// may outlast it.
const SPECIAL_BUILTINS: &[&str] = &[
    ":", ".", "source", "eval", "exec", "export", "readonly", "set", "shift", "trap", "unset",
    "return", "exit", "break", "continue", "times", "declare", "typeset", "local",
];
const DECLARERS: &[&str] = &[
    "export", "declare", "typeset", "local", "readonly", "integer", "float", "private",
];
const SHELLS: &[(&str, Dialect)] = &[
    ("bash", Dialect::Bash),
    ("zsh", Dialect::Zsh),
    ("sh", Dialect::Sh),
    ("dash", Dialect::Sh),
    ("ash", Dialect::Sh),
    ("ksh", Dialect::Sh),
    ("mksh", Dialect::Sh),
    ("yash", Dialect::Sh),
];
/// Programs that run the command after their own options, outside the
/// shell's builtins.
const LAUNCHERS: &[&str] = &[
    "env",
    "nice",
    "nohup",
    "timeout",
    "gtimeout",
    "stdbuf",
    "sudo",
    "doas",
    "caffeinate",
    "xcrun",
    "genv",
    "ionice",
    "chrt",
    "taskset",
    "setsid",
    "unbuffer",
    "exec",
];
/// Programs that add arguments of their own to the command they run.
const SUPPLIERS: &[&str] = &["xargs", "find", "parallel", "watch"];

/// The identifiers in `text`, in order.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|piece| is_identifier(piece))
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// The names `text` assigns as arithmetic: `x=`, `x+=`, `x++`, `++x`.
fn arith_assigned(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut names = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let before: String = chars[..start].iter().rev().take(2).collect();
            let mut j = i;
            while chars.get(j) == Some(&' ') {
                j += 1;
            }
            let after: String = chars[j.min(chars.len())..].iter().take(3).collect();
            let assigns = (after.starts_with('=') && !after.starts_with("=="))
                || after.starts_with("++")
                || after.starts_with("--")
                || ["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="]
                    .iter()
                    .any(|op| after.starts_with(op))
                || after.starts_with("<<=")
                || after.starts_with(">>=")
                || before == "++"
                || before == "--";
            if assigns {
                names.push(name);
            }
        } else {
            i += 1;
        }
    }
    names
}

/// The shape `name=`, `name+=` or `name[...]=` at the start of a word.
enum Shape {
    Scalar { name: String, append: bool },
    Element(String),
}

fn assignment_shape(word: &Word) -> Option<Shape> {
    let Some(Part::Lit {
        text,
        quoted: false,
    }) = word.parts.first()
    else {
        return None;
    };
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    let name = &text[..end];
    if !is_identifier(name) {
        return None;
    }
    let rest = &text[end..];
    if rest.starts_with("+=") {
        Some(Shape::Scalar {
            name: name.to_string(),
            append: true,
        })
    } else if rest.starts_with('=') {
        Some(Shape::Scalar {
            name: name.to_string(),
            append: false,
        })
    } else if rest.starts_with('[') && word.raw.contains("]=") {
        Some(Shape::Element(name.to_string()))
    } else {
        None
    }
}

/// The value parts of `name=value`: the first literal without its prefix.
fn value_parts(word: &Word) -> Word {
    let mut parts = word.parts.clone();
    if let Some(Part::Lit { text, .. }) = parts.first_mut() {
        let cut = text.find('=').map_or(0, |at| at + 1);
        *text = text[cut..].to_string();
    }
    Word {
        parts,
        raw: word.raw.clone(),
    }
}

/// A superset of the names a token run may assign, each marked when only
/// `read` and its kin assign it, from what they read.
fn assigned_names(toks: &[Tok]) -> Vec<(String, bool)> {
    const OUTSIDE: &[&str] = &["read", "mapfile", "readarray", "getopts"];
    let mut names = Vec::new();
    let mut taking: Option<bool> = None;
    let mut loop_var = false;
    for tok in toks {
        match tok {
            Tok::Op(_) => taking = None,
            Tok::Array { name, .. } => names.push((name.clone(), false)),
            Tok::ArithCmd(text) => {
                names.extend(arith_assigned(text).into_iter().map(|n| (n, false)));
            }
            Tok::Redir { .. } => {}
            Tok::Word(word) => {
                if let Some(Shape::Scalar { name, .. } | Shape::Element(name)) =
                    assignment_shape(word)
                {
                    names.push((name, false));
                }
                for part in &word.parts {
                    match part {
                        Part::Arith { body: text, .. } => {
                            names.extend(arith_assigned(text).into_iter().map(|n| (n, false)));
                        }
                        Part::Param {
                            body, braced: true, ..
                        } => names.extend(default_assigned(body).map(|n| (n, false))),
                        _ => {}
                    }
                }
                let text = word.plain().unwrap_or_default();
                if is_identifier(text) {
                    if loop_var {
                        names.push((text.to_string(), false));
                    } else if let Some(outside) = taking {
                        names.push((text.to_string(), outside));
                    }
                }
                loop_var = matches!(text, "for" | "select" | "foreach");
                if OUTSIDE.contains(&text) {
                    taking = Some(true);
                } else if READERS.contains(&text)
                    || DECLARERS.contains(&text)
                    || matches!(text, "unset" | "let" | "printf" | "print")
                {
                    taking = Some(false);
                }
            }
        }
    }
    names.extend(["REPLY", "reply", "OPTARG", "OPTIND", "MAPFILE"].map(|n| (n.to_string(), true)));
    names
}

/// The name `${x=...}` or `${x:=...}` assigns.
fn default_assigned(body: &str) -> Option<String> {
    let end = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(body.len());
    let rest = &body[end..];
    (end > 0 && (rest.starts_with('=') || rest.starts_with(":="))).then(|| body[..end].to_string())
}

impl Run<'_> {
    fn emit(&mut self, output: bool, value: Composed) {
        self.events.push(Event {
            depth: self.depth + usize::from(self.uncertain > 0),
            output,
            exact: true,
            value,
        });
    }

    fn emit_eval(&mut self, eval: &Eval) {
        if !eval.online {
            return;
        }
        match &eval.fields {
            Ok(fields) => {
                for field in fields {
                    self.emit(false, Composed::Resolved(field.clone()));
                }
            }
            Err(reason) => self.emit(false, Composed::Unresolved(reason)),
        }
    }

    fn run_text(&mut self, text: &str) {
        match lex(text) {
            Ok((toks, _)) if self.dialect == Dialect::Sh && toks.iter().any(bash_only) => {
                // A POSIX shell refuses the whole text: nothing in it runs.
                self.output(Err("bash or zsh grammar in a POSIX shell"));
            }
            Ok((toks, heredocs)) => self.run_tokens(&toks, &heredocs),
            Err(reason) => {
                if ["$", "`", "printf", "echo", "print", "<<", "eval"]
                    .iter()
                    .any(|t| text.contains(t))
                    || self.capture.is_some()
                {
                    self.output(Err(reason));
                }
            }
        }
    }

    /// A nested reading in its own state, events only: a substitution body
    /// (`captured`, its output becomes a value) or a child shell (its
    /// output is the line's).
    fn nested(&mut self, text: &str, state: State, dialect: Dialect, captured: bool) {
        if self.nesting >= 8 {
            self.emit(
                false,
                Composed::Unresolved("nesting deeper than the guard reads"),
            );
            return;
        }
        let depth = self.depth + usize::from(captured);
        let mut run = new_run(dialect, state, &mut *self.events, depth, self.nesting + 1);
        run.run_text(text);
    }

    fn frame(&mut self) -> &mut Frame {
        if self.frames.is_empty() {
            self.frames.push(Frame::default());
        }
        let last = self.frames.len() - 1;
        &mut self.frames[last]
    }

    fn set_unknown(&mut self, name: &str, reason: &'static str) {
        self.state
            .vars
            .insert(name.to_string(), Var::Unknown(reason));
        self.log.push(name.to_string());
    }

    fn mark_unknown(&mut self, names: Vec<String>, reason: &'static str) {
        for name in names {
            self.set_unknown(&name, reason);
        }
    }

    /// Store an assignment, as certain as the command that makes it.
    fn store(&mut self, name: &str, var: Var, certain: bool) {
        let var = if !certain {
            Var::Unknown(MAY_ASSIGN)
        } else if SPECIAL.contains(&name) && !(name == "PWD" && self.state.directory.is_some()) {
            Var::Unknown("an assignment to a special shell variable")
        } else if self.state.readonly.contains(name) {
            Var::Unknown("an assignment to a read-only variable")
        } else if self.dialect == Dialect::Sh && matches!(var, Var::Array(_)) {
            Var::Unknown("an array outside bash and zsh")
        } else {
            var
        };
        self.state.changed.remove(name);
        self.state.vars.insert(name.to_string(), var);
        self.log.push(name.to_string());
        let list = &mut self.frame().list;
        list.all.push(name.to_string());
        if list.commands > 1 {
            list.conditional.push(name.to_string());
        }
    }

    fn certain(&self, piped: bool) -> bool {
        let frame = self.frames.last();
        self.uncertain == 0
            && !piped
            && !frame.is_some_and(|f| f.list.after_or || f.parent_uncertain)
    }

    fn end_list(&mut self, background: bool) {
        let list = std::mem::take(&mut self.frame().list);
        let names = if background {
            list.all
        } else {
            list.conditional
        };
        self.mark_unknown(names, MAY_ASSIGN);
    }

    #[allow(clippy::too_many_lines)] // one walk over the command grammar
    fn run_tokens(&mut self, toks: &[Tok], heredocs: &[Heredoc]) {
        self.stdin_literal |= feeds_literal_stdin(toks);
        let mut i = 0;
        let mut prev_op = "\n";
        let mut case_patterns: Vec<bool> = Vec::new();
        while i < toks.len() {
            if case_patterns.last() == Some(&true) {
                // A case pattern runs to its `)`; `esac` ends the case.
                match &toks[i] {
                    Tok::Word(w) if w.plain() == Some("esac") => {
                        case_patterns.pop();
                        self.uncertain = self.uncertain.saturating_sub(1);
                    }
                    Tok::Op(")") => {
                        if let Some(top) = case_patterns.last_mut() {
                            *top = false;
                        }
                    }
                    Tok::Op(_) | Tok::Redir { .. } | Tok::Array { .. } | Tok::ArithCmd(_) => {}
                    Tok::Word(word) => {
                        let eval = self.word(word, Ctx::Fields);
                        self.emit_eval(&eval);
                    }
                }
                i += 1;
                continue;
            }
            match &toks[i] {
                Tok::Op(op) => {
                    match *op {
                        ";" | "\n" | "&" => self.end_list(*op == "&"),
                        "||" => {
                            let list = &mut self.frame().list;
                            list.after_or = true;
                            let names = std::mem::take(&mut list.conditional);
                            self.mark_unknown(names, MAY_ASSIGN);
                        }
                        "(" => {
                            let parent_first =
                                self.frames.last().is_some_and(|f| f.list.commands == 0);
                            let snapshot = Some(Box::new(self.state.clone()));
                            self.frame().list.commands += 1;
                            self.frames.push(Frame {
                                snapshot,
                                parent_first,
                                ..Frame::default()
                            });
                        }
                        ")" => {
                            if self.frames.len() > 1
                                && self.frames.last().is_some_and(|f| f.snapshot.is_some())
                            {
                                if let Some(frame) = self.frames.pop() {
                                    if let Some(state) = frame.snapshot {
                                        self.state = *state;
                                    }
                                }
                            } else {
                                self.emit(true, Composed::Unresolved(GRAMMAR));
                                return;
                            }
                        }
                        ";;" | ";&" | ";;&" | ";|" => {
                            self.end_list(false);
                            if let Some(top) = case_patterns.last_mut() {
                                *top = true;
                            }
                        }
                        _ => {}
                    }
                    prev_op = *op;
                    i += 1;
                    continue;
                }
                Tok::ArithCmd(text) => {
                    let names = arith_assigned(text);
                    self.mark_unknown(names, "an arithmetic assignment");
                    self.frame().list.commands += 1;
                    i += 1;
                    continue;
                }
                Tok::Word(word) => {
                    if let Some(next) = self.keyword(word, toks, i, heredocs, &mut case_patterns) {
                        i = next;
                        continue;
                    }
                }
                Tok::Array { .. } | Tok::Redir { .. } => {}
            }
            let start = i;
            while i < toks.len() && !matches!(toks[i], Tok::Op(_)) {
                i += 1;
            }
            let cmd = &toks[start..i];
            // `name ()`: a function definition.
            if let ([Tok::Word(name)], Some(Tok::Op("(")), Some(Tok::Op(")"))) =
                (cmd, toks.get(i), toks.get(i + 1))
            {
                let braced = toks.get(i + 2).and_then(tok_plain) == Some("{");
                self.define_function(name.plain().unwrap_or_default(), &toks[i..], braced);
                i += 2;
                continue;
            }
            let term = match toks.get(i) {
                Some(Tok::Op(op)) => *op,
                _ => "\n",
            };
            let piped = matches!(prev_op, "|" | "|&") || matches!(term, "|" | "|&");
            self.frame().list.commands += 1;
            self.each_choice(cmd, heredocs, &mut |run| {
                run.simple(cmd, heredocs, piped, term);
            });
            prev_op = "";
        }
        self.end_list(false);
    }

    /// A reserved word at command start: the index after what it consumes.
    #[allow(clippy::too_many_lines)] // one dispatch over the reserved words
    fn keyword(
        &mut self,
        word: &Word,
        toks: &[Tok],
        i: usize,
        heredocs: &[Heredoc],
        case_patterns: &mut Vec<bool>,
    ) -> Option<usize> {
        let text = word.plain()?;
        match text {
            "then" | "do" | "else" | "elif" | "!" => Some(i + 1),
            "time" => Some(if toks.get(i + 1).and_then(tok_plain) == Some("-p") {
                i + 2
            } else {
                i + 1
            }),
            "if" => {
                self.enter_compound(&toks[i..]);
                Some(i + 1)
            }
            "while" | "until" => {
                self.enter_compound(&toks[i..]);
                self.loops.push(None);
                Some(i + 1)
            }
            "fi" => {
                self.uncertain = self.uncertain.saturating_sub(1);
                Some(i + 1)
            }
            "done" => {
                self.uncertain = self.uncertain.saturating_sub(1);
                if let Some(Some(name)) = self.loops.pop() {
                    self.set_unknown(&name, "a loop variable after its loop");
                }
                Some(i + 1)
            }
            "for" | "select" => Some(self.loop_header(text, toks, i, heredocs)),
            "case" => {
                self.enter_compound(&toks[i..]);
                let mut j = i + 1;
                if let Some(subject @ Tok::Word(_)) = toks.get(j) {
                    self.judge_words(std::slice::from_ref(subject), heredocs, Ctx::Assign);
                    j += 1;
                }
                if toks.get(j).and_then(tok_plain) == Some("in") {
                    j += 1;
                }
                case_patterns.push(true);
                Some(j)
            }
            "esac" => {
                case_patterns.pop();
                self.uncertain = self.uncertain.saturating_sub(1);
                Some(i + 1)
            }
            "function" => {
                let name = toks
                    .get(i + 1)
                    .and_then(tok_plain)
                    .unwrap_or_default()
                    .to_string();
                let mut j = i + 2;
                if matches!(toks.get(j), Some(Tok::Op("(")))
                    && matches!(toks.get(j + 1), Some(Tok::Op(")")))
                {
                    j += 2;
                }
                let braced = toks.get(j).and_then(tok_plain) == Some("{");
                self.define_function(&name, &toks[i..], braced);
                Some(j)
            }
            "repeat" | "foreach" | "coproc" => {
                self.enter_compound(&toks[i..]);
                self.uncertain = usize::MAX;
                Some(i + 1)
            }
            "[[" => {
                let mut j = i + 1;
                while let Some(tok) = toks.get(j) {
                    j += 1;
                    if tok_plain(tok) == Some("]]") {
                        break;
                    }
                }
                let inside = &toks[i + 1..j.saturating_sub(1).max(i + 1)];
                self.judge_words(inside, heredocs, Ctx::Assign);
                self.frame().list.commands += 1;
                Some(j)
            }
            "{" => {
                let parent_first = self.frames.last().is_some_and(|f| f.list.commands == 0);
                let parent_uncertain = self
                    .frames
                    .last()
                    .is_some_and(|f| f.list.after_or || f.parent_uncertain);
                self.frame().list.commands += 1;
                let log_at = self.log.len();
                let function_body = std::mem::take(&mut self.body_pending);
                self.frames.push(Frame {
                    log_at,
                    parent_first,
                    parent_uncertain,
                    function_body,
                    ..Frame::default()
                });
                Some(i + 1)
            }
            "}" => {
                if self.frames.len() > 1 && self.frames.last().is_some_and(|f| f.snapshot.is_none())
                {
                    self.end_list(false);
                    if let Some(frame) = self.frames.pop() {
                        if frame.function_body {
                            self.uncertain = self.uncertain.saturating_sub(1);
                        }
                        let names: Vec<String> =
                            self.log[frame.log_at.min(self.log.len())..].to_vec();
                        let piped = matches!(toks.get(i + 1), Some(Tok::Op("|" | "|&" | "&")));
                        if piped || frame.parent_uncertain {
                            self.mark_unknown(names, MAY_ASSIGN);
                        } else if !frame.parent_first {
                            self.frame().list.conditional.extend(names);
                        }
                    }
                }
                Some(i + 1)
            }
            _ => None,
        }
    }

    /// Judge the words of `toks` once for each value a loop gives them.
    fn judge_words(&mut self, toks: &[Tok], heredocs: &[Heredoc], ctx: Ctx) {
        self.each_choice(toks, heredocs, &mut |run| {
            for tok in toks {
                if let Tok::Word(word) = tok {
                    run.word_side_effects(word);
                    let eval = run.word(word, ctx);
                    run.emit_eval(&eval);
                }
            }
        });
    }

    /// `for NAME in WORDS` and `select`: the variable takes each value
    /// the words give, a glob standing for outside names. The index after
    /// the header.
    fn loop_header(
        &mut self,
        keyword: &str,
        toks: &[Tok],
        i: usize,
        heredocs: &[Heredoc],
    ) -> usize {
        self.enter_compound(&toks[i..]);
        let mut j = i + 1;
        let name = toks
            .get(j)
            .and_then(tok_plain)
            .filter(|n| is_identifier(n))
            .map(str::to_string);
        if name.is_some() {
            j += 1;
        }
        let mut list: Option<Result<Vec<Value>, &'static str>> = None;
        if toks.get(j).and_then(tok_plain) == Some("in") {
            j += 1;
            let start = j;
            while let Some(tok) = toks.get(j) {
                if matches!(tok, Tok::Op(_)) || tok_plain(tok) == Some("do") {
                    break;
                }
                j += 1;
            }
            let header = &toks[start..j];
            let mut values = Vec::new();
            let mut failure = None;
            self.each_choice(header, heredocs, &mut |run| {
                for tok in header {
                    match tok {
                        Tok::Word(word) => {
                            run.word_side_effects(word);
                            let eval = run.word(word, Ctx::List);
                            run.emit_eval(&eval);
                            match eval.fields {
                                Ok(fields) => values.extend(fields),
                                Err(reason) => failure = failure.or(Some(reason)),
                            }
                        }
                        _ => failure = failure.or(Some(GRAMMAR)),
                    }
                }
            });
            list = Some(failure.map_or(Ok(values), Err));
        }
        if let Some(name) = &name {
            let var = match (keyword, list) {
                ("for", Some(Ok(values))) if values.len() <= 16 => Var::Choices(values),
                ("for", Some(Ok(_))) => {
                    Var::Unknown("a loop over more values than the guard reads")
                }
                (_, Some(Err(reason))) => Var::Unknown(reason),
                _ => Var::Unknown("a loop variable the guard cannot resolve"),
            };
            self.state.vars.insert(name.clone(), var);
            self.log.push(name.clone());
        }
        self.loops.push(name);
        j
    }

    /// The `for` variables a run of tokens reads.
    fn choice_names(&self, toks: &[Tok], heredocs: &[Heredoc]) -> Vec<String> {
        let names: Vec<&String> = self
            .state
            .vars
            .iter()
            .filter(|(_, var)| matches!(var, Var::Choices(_)))
            .map(|(name, _)| name)
            .collect();
        if names.is_empty() {
            return Vec::new();
        }
        let mut text = String::new();
        for tok in toks {
            match tok {
                Tok::Word(word) => text.push_str(&word.raw),
                Tok::Array { elements, .. } => {
                    for element in elements {
                        text.push_str(&element.raw);
                        text.push(' ');
                    }
                }
                Tok::Redir {
                    target, heredoc, ..
                } => {
                    if let Some(word) = target {
                        text.push_str(&word.raw);
                    }
                    if let Some(doc) = heredoc.and_then(|at| heredocs.get(at)) {
                        text.push_str(&doc.body.raw);
                    }
                }
                Tok::ArithCmd(body) => text.push_str(body),
                Tok::Op(_) => {}
            }
            text.push(' ');
        }
        names
            .into_iter()
            .filter(|name| {
                text.contains(&format!("${name}")) || text.contains(&format!("${{{name}"))
            })
            .cloned()
            .collect()
    }

    /// Run `f` once for each combination of the values the `for`
    /// variables `toks` reads take, each set as a plain value.
    fn each_choice(&mut self, toks: &[Tok], heredocs: &[Heredoc], f: &mut dyn FnMut(&mut Self)) {
        let names = self.choice_names(toks, heredocs);
        if names.is_empty() {
            f(self);
            return;
        }
        let lists: Vec<(String, Vec<Value>)> = names
            .into_iter()
            .map(|name| {
                let values = match self.state.vars.get(&name) {
                    Some(Var::Choices(values)) => values.clone(),
                    _ => Vec::new(),
                };
                (name, values)
            })
            .collect();
        let combos = lists
            .iter()
            .try_fold(1_usize, |n, (_, values)| n.checked_mul(values.len()))
            .unwrap_or(usize::MAX);
        if combos > 16 {
            self.emit(
                false,
                Composed::Unresolved("a loop over more values than the guard reads"),
            );
            for (name, _) in &lists {
                self.state.vars.insert(
                    name.clone(),
                    Var::Unknown("a loop over more values than the guard reads"),
                );
            }
            f(self);
        } else {
            for combo in 0..combos {
                let mut rest = combo;
                for (name, values) in &lists {
                    let pick = values[rest % values.len()].clone();
                    rest /= values.len();
                    self.state.vars.insert(name.clone(), Var::Scalar(pick));
                }
                f(self);
            }
        }
        for (name, values) in lists {
            self.state.vars.insert(name, Var::Choices(values));
        }
    }

    fn enter_compound(&mut self, rest: &[Tok]) {
        let names = assigned_names(rest);
        self.mark_assigned(&names, "a variable a compound command may set");
        self.uncertain = self.uncertain.saturating_add(1);
    }

    /// A function definition: its body is read now, as code that may run
    /// later; a call marks what it may set unknown. `braced` when a `{`
    /// body follows, which ends at its `}`.
    fn define_function(&mut self, name: &str, rest: &[Tok], braced: bool) {
        let names = assigned_names(rest);
        self.mark_assigned(&names, "a variable a function may set");
        self.state.functions.insert(
            name.to_string(),
            names.into_iter().map(|(n, _)| n).collect(),
        );
        if braced {
            self.uncertain = self.uncertain.saturating_add(1);
            self.body_pending = true;
        } else {
            self.uncertain = usize::MAX;
        }
    }
}

/// Whether the line gives a program's stdin text it spells: a heredoc, a
/// here-string, or a pipe from `printf`, `echo` or `print`.
fn feeds_literal_stdin(toks: &[Tok]) -> bool {
    let mut command_start = 0;
    for (at, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Redir { op, heredoc, .. } if op.ends_with("<<<") || heredoc.is_some() => {
                return true;
            }
            Tok::Op(op) => {
                if matches!(*op, "|" | "|&") {
                    let first = toks[command_start..at].iter().find_map(|t| match t {
                        Tok::Word(word) if assignment_shape(word).is_none() => word.plain(),
                        _ => None,
                    });
                    if first.is_some_and(|w| matches!(basename(w), "printf" | "echo" | "print")) {
                        return true;
                    }
                }
                command_start = at + 1;
            }
            _ => {}
        }
    }
    false
}

/// Grammar a POSIX `sh` does not parse.
fn bash_only(tok: &Tok) -> bool {
    match tok {
        Tok::Redir { op, .. } => op.ends_with("<<<") || op.starts_with('&'),
        Tok::Array { .. } | Tok::ArithCmd(_) => true,
        Tok::Op(op) => matches!(*op, "|&" | ";;&" | ";|"),
        Tok::Word(word) => matches!(
            word.plain(),
            Some("[[" | "function" | "select" | "repeat" | "foreach")
        ),
    }
}

fn tok_plain(tok: &Tok) -> Option<&str> {
    match tok {
        Tok::Word(word) => word.plain(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Words and parameters
// ---------------------------------------------------------------------------

impl Run<'_> {
    /// The current IFS: its characters, or `None` when it is unknown.
    fn ifs(&self) -> Option<String> {
        match self.state.vars.get("IFS") {
            None => Some(" \t\n".to_string()),
            Some(Var::Scalar(value)) => value.exact(),
            Some(_) => None,
        }
    }

    fn ifs_separator(&self) -> Result<String, &'static str> {
        self.ifs()
            .map(|ifs| ifs.chars().next().map(String::from).unwrap_or_default())
            .ok_or("an IFS the guard cannot resolve")
    }

    /// Run the substitutions and arithmetic a word holds, for their events
    /// and side effects.
    fn word_side_effects(&mut self, word: &Word) {
        for part in &word.parts {
            match part {
                Part::Subst { body, .. } | Part::ProcSubst(body) => {
                    let state = self.state.clone();
                    self.nested(body, state, self.dialect, true);
                }
                Part::Arith { body: text, .. } => {
                    let names = arith_assigned(text);
                    self.mark_unknown(names, "an arithmetic assignment");
                }
                Part::Param {
                    body, braced: true, ..
                } => {
                    if let Some(name) = default_assigned(body) {
                        self.set_unknown(&name, "a default assignment inside an expansion");
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one pass over a word's parts
    fn word(&mut self, word: &Word, ctx: Ctx) -> Eval {
        let mut online = false;
        let mut loose = false;
        let mut globbed = false;
        let mut err: Option<&'static str> = None;
        let mut fields: Vec<Value> = Vec::new();
        let mut cur = Value::default();
        let mut started = false;
        let unquoted: String = word
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Lit {
                    text,
                    quoted: false,
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let first_unquoted = match word.parts.first() {
            Some(Part::Lit {
                text,
                quoted: false,
            }) => text.chars().next(),
            _ => None,
        };
        let fields_ctx = ctx != Ctx::Assign;
        if fields_ctx {
            if unquoted.contains('{')
                && unquoted.contains('}')
                && (unquoted.contains(',') || unquoted.contains(".."))
            {
                err = Some("a brace expansion");
            } else if first_unquoted == Some('~') {
                err = Some("a tilde expansion");
            } else if first_unquoted == Some('=') && self.dialect == Dialect::Zsh {
                err = Some("a zsh equals expansion");
            }
        } else if first_unquoted == Some('~') || unquoted.contains(":~") {
            err = Some("a tilde expansion");
        }
        if let Some(reason) = self.state.opt_words {
            // An option may change quoting, globbing or any expansion.
            let expands = word.parts.iter().any(|p| !matches!(p, Part::Lit { .. }));
            if expands || word.raw.contains("''") || unquoted.contains(['^', '#', '~']) {
                err = err.or(Some(reason));
            }
        }
        let single = word.parts.len() == 1;
        for part in &word.parts {
            // One value and whether it is quoted, for the parts that give one.
            let (value, quoted) = match part {
                Part::Lit { text, quoted } => {
                    // A glob stands for the names it matches, outside text;
                    // as an argument, how many is unknown.
                    if fields_ctx && !*quoted && text.contains(['*', '?', '[']) {
                        loose |= ctx == Ctx::Fields;
                        globbed = true;
                        push_glob(&mut cur, text);
                    } else {
                        cur.push_text(text);
                    }
                    started |= *quoted || !text.is_empty();
                    continue;
                }
                Part::Ansi(raw) => {
                    online |= raw.contains('\\');
                    started = true;
                    if self.dialect == Dialect::Sh {
                        online = true;
                        err = err.or(Some("ANSI-C quoting outside bash and zsh"));
                        continue;
                    }
                    match decode(raw, Escapes::Ansi) {
                        Ok(text) => cur.push_text(&text),
                        Err(reason) => {
                            online = true;
                            err = err.or(Some(reason));
                        }
                    }
                    continue;
                }
                Part::Unmodelled(reason) => {
                    online = true;
                    err = err.or(Some(reason));
                    continue;
                }
                Part::Subst { body, quoted } => {
                    // The word joins the substitution's output with what
                    // the line spells around it: a composition, judged.
                    let (value, _) = self.capture(body);
                    online = true;
                    (value, *quoted)
                }
                Part::ProcSubst(_) => (Ok(Value::open()), false),
                Part::Arith { body, quoted } => {
                    let (value, on) = self.arith_value(body);
                    online |= on;
                    (value, *quoted)
                }
                Part::Param {
                    body,
                    braced,
                    quoted,
                    zsh_tail,
                } => {
                    let eval = self.param(body, *braced, *quoted, *zsh_tail);
                    online |= eval.online;
                    match eval.value {
                        Err(reason) => (Err(reason), *quoted),
                        Ok(Param::One(value)) => (Ok(value), *quoted),
                        Ok(Param::Many(values)) => {
                            if !fields_ctx {
                                err = err.or(Some("an array expansion in an assignment"));
                            } else if !*quoted {
                                if !single {
                                    err = err.or(Some("an unquoted array inside a word"));
                                    continue;
                                }
                                for value in values {
                                    match self.unquoted_ok(&value) {
                                        Ok(split) => loose |= split,
                                        Err(reason) => err = err.or(Some(reason)),
                                    }
                                    if !value.is_empty() {
                                        fields.push(value);
                                    }
                                }
                                return self.finish(err, fields, (online, loose, globbed), ctx);
                            } else if values.is_empty() {
                                if !single {
                                    err = err.or(Some("an empty array inside a word"));
                                }
                            } else {
                                let last = values.len() - 1;
                                for (at, value) in values.into_iter().enumerate() {
                                    cur.append(&value);
                                    if at < last {
                                        fields.push(std::mem::take(&mut cur));
                                    }
                                }
                                started = true;
                            }
                            continue;
                        }
                    }
                }
            };
            match value {
                Err(reason) => err = err.or(Some(reason)),
                Ok(value) => {
                    if quoted || !fields_ctx {
                        started |= quoted;
                    } else {
                        match self.unquoted_ok(&value) {
                            Ok(split) => loose |= split,
                            Err(reason) => err = err.or(Some(reason)),
                        }
                    }
                    started |= !value.is_empty();
                    cur.append(&value);
                }
            }
        }
        if err.is_none() && (!fields_ctx || started || !cur.is_empty()) {
            fields.push(cur);
        }
        self.finish(err, fields, (online, loose, globbed), ctx)
    }

    /// A word's result. In a `for` list a field bash may split stands for
    /// outside names as a whole.
    #[allow(clippy::unused_self)] // a method beside `word`
    fn finish(
        &self,
        err: Option<&'static str>,
        fields: Vec<Value>,
        (online, loose, globbed): (bool, bool, bool),
        ctx: Ctx,
    ) -> Eval {
        let fields = match err {
            Some(reason) => Err(reason),
            None if loose && ctx == Ctx::List => Ok(vec![Value::open()]),
            None => Ok(fields),
        };
        Eval {
            fields,
            online,
            loose,
            globbed,
        }
    }

    /// Whether an unquoted expansion's text reaches the command as written:
    /// bash splits it on IFS and globs it, zsh does neither. `Ok(true)`
    /// when bash may split text from outside the line into fields the
    /// guard cannot count.
    fn unquoted_ok(&self, value: &Value) -> Result<bool, &'static str> {
        if self.dialect == Dialect::Zsh {
            return Ok(false);
        }
        let Some(ifs) = self.ifs() else {
            return Err("an IFS the guard cannot resolve");
        };
        if value.any_text(|c| ifs.contains(c)) {
            return Err("an unquoted expansion the shell splits");
        }
        if value.any_text(|c| matches!(c, '*' | '?' | '[')) {
            return Err("an unquoted expansion the shell globs");
        }
        Ok(value.has_open())
    }

    /// The text a command substitution's body writes, trailing newlines
    /// removed: what the modelled builtins write, and outside text for any
    /// other program. Also whether it holds text the line spells.
    fn capture(&self, body: &str) -> (Result<Value, &'static str>, bool) {
        if self.nesting >= 8 {
            return (Err("nesting deeper than the guard reads"), true);
        }
        let mut scratch = Vec::new();
        let mut run = new_run(
            self.dialect,
            self.state.clone(),
            &mut scratch,
            self.depth + 1,
            self.nesting + 1,
        );
        run.capture = Some(Vec::new());
        run.run_text(body);
        let records = run.capture.take().unwrap_or_default();
        if run.approx.get() {
            self.approx.set(true);
        }
        let mut value = Value::default();
        for record in records {
            match record {
                Ok(piece) => value.append(&piece),
                Err(reason) => return (Err(reason), true),
            }
        }
        while let Some(Piece::Text(text)) = value.0.last_mut() {
            while text.ends_with('\n') {
                text.pop();
            }
            if text.is_empty() {
                value.0.pop();
            } else {
                break;
            }
        }
        let online = value.0.iter().any(|piece| matches!(piece, Piece::Text(_)));
        (Ok(value), online)
    }

    /// `$((...))` and `$[...]`: integer arithmetic over decimal literals
    /// and values the line assigns; a name the line never assigns makes
    /// it outside text. Also whether it reads a value the line assigns.
    fn arith_value(&self, text: &str) -> (Result<Value, &'static str>, bool) {
        const ARITH: &str = "an arithmetic expansion the guard cannot resolve";
        let chars: Vec<char> = text.chars().collect();
        let mut tokens: Vec<Arith> = Vec::new();
        let mut outside = false;
        let mut online = false;
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            if matches!(c, ' ' | '\t' | '\n') {
                i += 1;
            } else if c.is_ascii_digit() {
                let start = i;
                while chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
                let digits: String = chars[start..i].iter().collect();
                // A leading zero is octal in bash and decimal in zsh.
                match digits.parse::<i64>() {
                    Ok(n) if digits.len() == 1 || !digits.starts_with('0') => {
                        tokens.push(Arith::Num(n));
                    }
                    _ => return (Err(ARITH), online),
                }
            } else if c == '$' || c.is_ascii_alphabetic() || c == '_' {
                i += usize::from(c == '$');
                let braced = chars.get(i) == Some(&'{');
                i += usize::from(braced);
                let start = i;
                while chars
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if braced {
                    if chars.get(i) != Some(&'}') {
                        return (Err(ARITH), online);
                    }
                    i += 1;
                }
                if !is_identifier(&name) {
                    return (Err(ARITH), online);
                }
                match self.state.vars.get(&name) {
                    None => {
                        outside = true;
                        tokens.push(Arith::Num(0));
                    }
                    Some(Var::Scalar(value)) => {
                        online = true;
                        if self.state.changed.contains(&name) {
                            self.approx.set(true);
                        }
                        let number = value.exact().and_then(|t| {
                            let digits = t.strip_prefix('-').unwrap_or(&t);
                            let plain = !digits.is_empty()
                                && digits.chars().all(|c| c.is_ascii_digit())
                                && (digits.len() == 1 || !digits.starts_with('0'));
                            plain.then(|| t.parse::<i64>().ok()).flatten()
                        });
                        match number {
                            Some(n) => tokens.push(Arith::Num(n)),
                            None => return (Err(ARITH), true),
                        }
                    }
                    Some(_) => return (Err(ARITH), true),
                }
            } else if "+-*/%()".contains(c) {
                if c == '*' && chars.get(i + 1) == Some(&'*') {
                    return (Err(ARITH), online);
                }
                tokens.push(Arith::Op(c));
                i += 1;
            } else {
                return (Err(ARITH), online);
            }
        }
        let mut at = 0;
        let result = arith_expr(&tokens, &mut at).filter(|_| at == tokens.len());
        match result {
            _ if outside => (Ok(Value::open()), online),
            Some(n) => (Ok(Value::text(&n.to_string())), online),
            None => (Err(ARITH), online),
        }
    }

    fn param(&mut self, body: &str, braced: bool, quoted: bool, zsh_tail: bool) -> ParamEval {
        if !braced {
            if zsh_tail && self.dialect == Dialect::Zsh {
                return if self.state.vars.contains_key(body) {
                    ParamEval::err("a zsh modifier or subscript after a parameter", true)
                } else {
                    ParamEval::open()
                };
            }
            return self.plain(body, quoted);
        }
        if let Some(flags) = body.strip_prefix('(') {
            return self.zsh_flags(flags);
        }
        if let Some(rest) = body.strip_prefix('!') {
            return self.indirect(rest);
        }
        if let Some(rest) = body.strip_prefix('#') {
            if rest.is_empty() {
                return ParamEval::open();
            }
            let name = rest.split('[').next().unwrap_or(rest);
            return if self.state.vars.contains_key(name) {
                ParamEval::err("a length expansion", true)
            } else {
                ParamEval::open()
            };
        }
        if is_identifier(body)
            || (body.chars().count() == 1 && "@*#?$!-_".contains(body))
            || body.chars().all(|c| c.is_ascii_digit())
        {
            return self.plain(body, quoted);
        }
        if let Some((name, rest)) = body.split_once('[') {
            if let Some(subscript) = rest.strip_suffix(']') {
                if is_identifier(name) && !subscript.contains(['[', ']']) {
                    return self.subscript(name, subscript, quoted);
                }
            }
        }
        self.operator(body)
    }

    /// Note a read of a value outside code may have replaced.
    fn note_changed(&self, name: &str) {
        if self.state.changed.contains(name) {
            self.approx.set(true);
        }
    }

    fn plain(&self, name: &str, quoted: bool) -> ParamEval {
        if name == "_" {
            return ParamEval::err("the last-argument parameter", true);
        }
        let positional = name.chars().all(|c| c.is_ascii_digit()) || name == "@" || name == "*";
        if positional || name == "#" {
            return self.positional(name, quoted);
        }
        let Some(var) = self.state.vars.get(name) else {
            return ParamEval::open();
        };
        if let Some(reason) = self.state.world {
            return ParamEval::err(reason, true);
        }
        self.note_changed(name);
        match var {
            Var::Scalar(value) => ParamEval::one(value.clone(), true),
            Var::Array(values) => match self.dialect {
                Dialect::Bash => ParamEval::one(values.first().cloned().unwrap_or_default(), true),
                _ => ParamEval::err("an array used as a scalar", true),
            },
            Var::Choices(_) => ParamEval::err("a loop variable outside its loop", true),
            Var::Unknown(reason) => ParamEval::err(reason, true),
        }
    }

    fn positional(&self, name: &str, quoted: bool) -> ParamEval {
        match &self.state.positional {
            Positional::Unknown(reason) => ParamEval::err(reason, true),
            Positional::Outside if name == "@" || name == "*" => {
                ParamEval::err("the positional parameters", false)
            }
            Positional::Outside => ParamEval::open(),
            Positional::Set { values, more } => {
                let given = values.get(1..).unwrap_or_default();
                match name {
                    "#" if *more => ParamEval::open(),
                    "#" => ParamEval::one(Value::text(&given.len().to_string()), true),
                    "@" | "*" if *more => {
                        ParamEval::err("positional parameters a program supplies", true)
                    }
                    "*" if quoted => match self.ifs_separator() {
                        Ok(sep) => ParamEval::one(join(given, &sep, false), true),
                        Err(reason) => ParamEval::err(reason, true),
                    },
                    "@" | "*" => ParamEval {
                        value: Ok(Param::Many(given.to_vec())),
                        online: true,
                    },
                    digits => {
                        let index = digits.parse::<usize>().unwrap_or(usize::MAX);
                        let value = values.get(index).cloned().unwrap_or_else(|| {
                            if *more {
                                Value::open()
                            } else {
                                Value::default()
                            }
                        });
                        ParamEval::one(value, true)
                    }
                }
            }
        }
    }

    fn subscript(&self, name: &str, subscript: &str, quoted: bool) -> ParamEval {
        let var = self.state.vars.get(name);
        if var.is_some() {
            if let Some(reason) = self.state.world {
                return ParamEval::err(reason, true);
            }
            self.note_changed(name);
        }
        match (var, subscript) {
            (None, "*") if quoted => ParamEval::open(),
            (None, "@" | "*") => ParamEval::err("an array the line does not set", false),
            (None, _) => ParamEval::open(),
            (Some(Var::Unknown(reason)), _) => ParamEval::err(reason, true),
            (Some(Var::Scalar(_) | Var::Choices(_)), _) => {
                ParamEval::err("a subscript of a scalar", true)
            }
            (Some(Var::Array(values)), "*") if quoted => match self.ifs_separator() {
                Ok(sep) => {
                    let mut joined = Value::default();
                    for (at, value) in values.iter().enumerate() {
                        if at > 0 {
                            joined.push_text(&sep);
                        }
                        joined.append(value);
                    }
                    ParamEval::one(joined, true)
                }
                Err(reason) => ParamEval::err(reason, true),
            },
            (Some(Var::Array(values)), "@" | "*") => ParamEval {
                value: Ok(Param::Many(values.clone())),
                online: true,
            },
            (Some(Var::Array(values)), index) => {
                let Ok(index) = index.parse::<usize>() else {
                    return ParamEval::err("an array subscript the guard cannot resolve", true);
                };
                let index = match self.dialect {
                    Dialect::Bash => index,
                    Dialect::Zsh if index > 0 => index - 1,
                    _ => {
                        return ParamEval::err("an array subscript the guard cannot resolve", true)
                    }
                };
                ParamEval::one(values.get(index).cloned().unwrap_or_default(), true)
            }
        }
    }

    fn zsh_flags(&self, flags: &str) -> ParamEval {
        const FLAG: &str = "a zsh parameter flag the guard does not model";
        if self.dialect != Dialect::Zsh {
            return ParamEval::err("a zsh parameter flag outside zsh", true);
        }
        let mut chars = flags.chars();
        if chars.next() != Some('j') {
            return ParamEval::err(FLAG, true);
        }
        let Some(delim) = chars.next() else {
            return ParamEval::err(FLAG, true);
        };
        if delim.is_ascii_alphanumeric() || "([{<\\".contains(delim) {
            return ParamEval::err(FLAG, true);
        }
        let rest: String = chars.collect();
        let Some((sep, after)) = rest.split_once(delim) else {
            return ParamEval::err(FLAG, true);
        };
        let Some(name) = after.strip_prefix(')') else {
            return ParamEval::err(FLAG, true);
        };
        if sep.contains(['\\', '$']) || !is_identifier(name) {
            return ParamEval::err(FLAG, true);
        }
        if let Some(reason) = self.state.world {
            return ParamEval::err(reason, true);
        }
        self.note_changed(name);
        match self.state.vars.get(name) {
            Some(Var::Array(values)) => {
                let mut joined = Value::default();
                for (at, value) in values.iter().enumerate() {
                    if at > 0 {
                        joined.push_text(sep);
                    }
                    joined.append(value);
                }
                ParamEval::one(joined, true)
            }
            Some(Var::Unknown(reason)) => ParamEval::err(reason, true),
            Some(Var::Scalar(_) | Var::Choices(_)) => {
                ParamEval::err("a zsh join of a scalar", true)
            }
            None => ParamEval::open(),
        }
    }

    fn indirect(&self, rest: &str) -> ParamEval {
        const INDIRECT: &str = "an indirect expansion the guard cannot resolve";
        if self.dialect != Dialect::Bash || !is_identifier(rest) {
            return ParamEval::err(INDIRECT, true);
        }
        if let Some(reason) = self.state.world {
            return ParamEval::err(reason, true);
        }
        let Some(Var::Scalar(target)) = self.state.vars.get(rest) else {
            return ParamEval::err(INDIRECT, true);
        };
        let Some(target) = target.exact().filter(|t| is_identifier(t)) else {
            return ParamEval::err(INDIRECT, true);
        };
        self.note_changed(rest);
        self.note_changed(&target);
        match self.state.vars.get(&target) {
            Some(Var::Scalar(value)) => ParamEval::one(value.clone(), true),
            _ => ParamEval::err(INDIRECT, true),
        }
    }

    /// `${name<op>...}`: unresolved on a name the line assigns; on another
    /// name, unknown text when the operator only transforms it, and
    /// unresolved when a literal operand may become the value.
    fn operator(&self, body: &str) -> ParamEval {
        let end = body
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(body.len());
        let (name, op) = if end == 0 {
            body.split_at(body.chars().next().map_or(0, char::len_utf8))
        } else {
            body.split_at(end)
        };
        // Any name in the body the line assigns: zsh reads `${=a}`, `${~a}`
        // and `${^a}` as flags on `a`, and an operand may expand one.
        if self.state.vars.contains_key(name)
            || identifiers(body).any(|n| self.state.vars.contains_key(n))
        {
            return ParamEval::err("a parameter expansion operator", true);
        }
        let operand_at = op
            .find(|c: char| !matches!(c, ':' | '-' | '+' | '=' | '?'))
            .unwrap_or(op.len());
        let operand = &op[operand_at..];
        let mentions_line = operand.split('$').skip(1).any(|after| {
            let after = after.strip_prefix('{').unwrap_or(after);
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            self.state.vars.contains_key(&after[..end])
        });
        if mentions_line {
            return ParamEval::err("a parameter expansion operator", true);
        }
        if operand.contains(['$', '`']) {
            return ParamEval::err("a parameter expansion operator", false);
        }
        let defaulting = [":-", "-", ":=", "=", ":+", "+", ":?", "?"]
            .iter()
            .any(|o| op.starts_with(o));
        if defaulting && !operand.is_empty() {
            return ParamEval::err("a parameter default", false);
        }
        ParamEval::open()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// One argument of a command: its fields, or why they cannot be read.
struct Arg {
    fields: Result<Vec<Value>, &'static str>,
    /// bash may split it into fields the guard cannot count.
    loose: bool,
    globbed: bool,
}

impl Arg {
    /// The argument's text when it is one exact field.
    fn literal(&self) -> Option<String> {
        match &self.fields {
            Ok(fields) if fields.len() == 1 => fields[0].exact(),
            _ => None,
        }
    }
}

/// The fields of a run of arguments, flattened.
fn flatten(args: &[Arg]) -> Result<Vec<Value>, &'static str> {
    let mut out = Vec::new();
    for arg in args {
        out.extend(arg.fields.clone()?);
    }
    Ok(out)
}

impl Run<'_> {
    #[allow(clippy::too_many_lines)] // the simple-command grammar, in order
    fn simple(&mut self, cmd: &[Tok], heredocs: &[Heredoc], piped: bool, term: &str) {
        self.approx.set(false);
        let certain = self.certain(piped);
        let mut prefix: Vec<&Tok> = Vec::new();
        let mut operands: Vec<&Tok> = Vec::new();
        let mut redirs: Vec<&Tok> = Vec::new();
        for tok in cmd {
            match tok {
                Tok::Redir { .. } => redirs.push(tok),
                Tok::Array { .. } if operands.is_empty() => prefix.push(tok),
                Tok::Word(word) if operands.is_empty() && assignment_shape(word).is_some() => {
                    prefix.push(tok);
                }
                _ => operands.push(tok),
            }
        }
        for tok in cmd {
            match tok {
                Tok::Word(word)
                | Tok::Redir {
                    target: Some(word), ..
                } => self.word_side_effects(word),
                Tok::Array { elements, .. } => {
                    for element in elements {
                        self.word_side_effects(element);
                    }
                }
                _ => {}
            }
        }
        // Assignments apply left to right. In front of a command they last
        // for that command only: a later value and the command's own words
        // read them as unknown, and so does the rest of the line.
        let standalone = operands.is_empty();
        let mut assigned: Vec<(String, Var)> = Vec::new();
        let mut saved: Vec<(String, Option<Var>)> = Vec::new();
        for tok in &prefix {
            if let Some((name, var)) = self.assignment(tok) {
                if standalone {
                    self.store(&name, var.clone(), certain);
                } else {
                    saved.push((name.clone(), self.state.vars.get(&name).cloned()));
                    self.state
                        .vars
                        .insert(name.clone(), Var::Unknown("a variable set for one command"));
                }
                assigned.push((name, var));
            }
        }
        for (name, old) in saved.into_iter().rev() {
            match old {
                Some(var) => self.state.vars.insert(name, var),
                None => self.state.vars.remove(&name),
            };
        }
        let args: Vec<Arg> = operands
            .iter()
            .map(|tok| match tok {
                Tok::Word(word) => {
                    let eval = self.word(word, Ctx::Fields);
                    self.emit_eval(&eval);
                    Arg {
                        fields: eval.fields,
                        loose: eval.loose,
                        globbed: eval.globbed,
                    }
                }
                _ => Arg {
                    fields: Err("an array assignment as an argument"),
                    loose: false,
                    globbed: false,
                },
            })
            .collect();
        let mut stdin_code: Option<Value> = None;
        for tok in &redirs {
            if let Tok::Redir {
                op,
                target,
                heredoc,
            } = tok
            {
                let word = match (target, heredoc) {
                    (Some(word), _) => word.clone(),
                    (None, Some(index)) => match heredocs.get(*index) {
                        Some(doc) => doc.body.clone(),
                        None => continue,
                    },
                    _ => continue,
                };
                let here = op.ends_with("<<<") || heredoc.is_some();
                let eval = self.word(&word, if here { Ctx::Assign } else { Ctx::Fields });
                self.emit_eval(&eval);
                if here {
                    stdin_code = match eval.fields {
                        Ok(fields) if fields.len() == 1 => {
                            let mut value = fields[0].clone();
                            if op.ends_with("<<<") {
                                value.push_text("\n");
                            }
                            Some(value)
                        }
                        _ => None,
                    };
                }
            }
        }
        // What the command writes reaches a captured stdout unless a pipe,
        // a background `&` or a redirection takes it.
        let outer_here = self.stdout_here;
        self.stdout_here = outer_here
            && !matches!(term, "|" | "|&" | "&")
            && !redirs.iter().any(|tok| stdout_redirect(tok));
        self.wrote = false;
        self.silent = standalone;
        self.special = false;
        self.glob_arg = args.iter().any(|arg| arg.globbed);
        if !standalone {
            self.command(&operands, &args, &assigned, stdin_code.as_ref(), certain);
            // Assignments in front of a special builtin or a function may
            // outlast it; in front of anything else they end with it.
            if self.special {
                for (name, _) in &assigned {
                    self.set_unknown(name, "a variable set for one command");
                }
            }
        }
        if !self.wrote && !self.silent {
            self.record(Ok(Value::open()));
        }
        self.stdout_here = outer_here;
        if term == "&" && !self.silent {
            self.record(Ok(Value::open()));
        }
    }

    /// Add what a command writes to the captured stdout.
    fn record(&mut self, value: Result<Value, &'static str>) {
        if !self.stdout_here {
            return;
        }
        let uncertain = self.uncertain > 0;
        if let Some(capture) = &mut self.capture {
            capture.push(match value {
                // A command that may not run, or may run again.
                Ok(_) if uncertain => Ok(Value::open()),
                other => other,
            });
        }
    }

    /// Every value the line assigned may since have been replaced by code
    /// from outside it.
    fn changed_all(&mut self) {
        let names: Vec<String> = self.state.vars.keys().cloned().collect();
        self.state.changed.extend(names);
    }

    /// `name` takes text from outside the line, or text the line spells
    /// (`from_line`), which the guard does not read.
    fn outside(&mut self, name: &str, from_line: bool) {
        if from_line {
            self.set_unknown(name, "a variable read from text the line spells");
            return;
        }
        match self.state.vars.get(name) {
            None => {}
            Some(Var::Scalar(value)) if value.pieces() == [Piece::Open] => {
                self.state.vars.remove(name);
            }
            Some(_) => self.set_unknown(name, "a variable a read may replace"),
        }
    }

    /// Names a construct may assign: outside text for what `read` and its
    /// kin take, unknown for anything else.
    fn mark_assigned(&mut self, names: &[(String, bool)], reason: &'static str) {
        for (name, outside) in names {
            let only_outside = *outside && !names.iter().any(|(n, o)| n == name && !o);
            if only_outside {
                self.outside(name, self.stdin_literal);
            } else {
                self.set_unknown(name, reason);
            }
        }
    }

    /// An option change: what it may make unknown.
    fn options(&mut self, names: &[Option<String>]) {
        for name in names {
            let (echo, words) = name
                .as_deref()
                .map_or((true, true), |n| option_effect(self.dialect, n));
            if echo {
                self.state.opt_echo = Some("an option change that may change echo decoding");
            }
            if words {
                self.state.opt_words = Some("an option change that may change word expansion");
            }
        }
    }

    /// Read one prefix or operand assignment: its name and value.
    fn assignment(&mut self, tok: &Tok) -> Option<(String, Var)> {
        match tok {
            Tok::Array {
                name,
                append,
                elements,
            } => {
                let mut values = Vec::new();
                let mut failure = None;
                for element in elements {
                    let eval = self.word(element, Ctx::Fields);
                    self.emit_eval(&eval);
                    match eval.fields {
                        // How many elements a glob or a split makes is unknown.
                        Ok(_) if eval.loose => {
                            failure = failure.or(Some("an array of fields the guard cannot count"));
                        }
                        Ok(fields) => values.extend(fields),
                        Err(reason) => failure = failure.or(Some(reason)),
                    }
                }
                let var = match (failure, append) {
                    (Some(reason), _) => Var::Unknown(reason),
                    (None, false) => Var::Array(values),
                    (None, true) => match self.state.vars.get(name) {
                        Some(Var::Array(old)) => {
                            let mut all = old.clone();
                            all.extend(values);
                            Var::Array(all)
                        }
                        _ => Var::Unknown("an append to an array the guard cannot resolve"),
                    },
                };
                Some((name.clone(), var))
            }
            Tok::Word(word) => match assignment_shape(word)? {
                Shape::Element(name) => Some((name, Var::Unknown("an array element assignment"))),
                Shape::Scalar { name, append } => {
                    let eval = self.word(&value_parts(word), Ctx::Assign);
                    self.emit_eval(&eval);
                    let value = match eval.fields {
                        Ok(mut fields) if fields.len() == 1 => fields.remove(0),
                        Ok(_) => {
                            return Some((
                                name,
                                Var::Unknown("an assignment the guard cannot resolve"),
                            ))
                        }
                        Err(reason) => return Some((name, Var::Unknown(reason))),
                    };
                    if !append {
                        return Some((name, Var::Scalar(value)));
                    }
                    if self.dialect == Dialect::Sh {
                        return Some((name, Var::Unknown("an append outside bash and zsh")));
                    }
                    let var = match self.state.vars.get(&name) {
                        Some(Var::Scalar(old)) => {
                            let mut all = old.clone();
                            all.append(&value);
                            Var::Scalar(all)
                        }
                        None => {
                            let mut all = Value::open();
                            all.append(&value);
                            Var::Scalar(all)
                        }
                        Some(_) => Var::Unknown("an append the guard cannot resolve"),
                    };
                    Some((name, var))
                }
            },
            _ => None,
        }
    }

    /// Resolve the program a command runs and read what it composes.
    #[allow(clippy::too_many_lines)] // one dispatch over the modelled programs
    fn command(
        &mut self,
        operands: &[&Tok],
        args: &[Arg],
        prefix: &[(String, Var)],
        stdin_code: Option<&Value>,
        certain: bool,
    ) {
        if self.alias_call(operands) {
            return;
        }
        let mut at = 0;
        let mut bypass = false;
        while let Some(word) = args.get(at).and_then(Arg::literal) {
            match word.as_str() {
                "command" => {
                    at += 1;
                    bypass = true;
                    match args.get(at).and_then(Arg::literal).as_deref() {
                        Some("-p") => at += 1,
                        Some("-v" | "-V") => return,
                        _ => {}
                    }
                }
                "builtin" if self.dialect == Dialect::Sh => return,
                "builtin" => {
                    at += 1;
                    bypass = true;
                    if args.get(at).and_then(Arg::literal).as_deref() == Some("--") {
                        at += 1;
                    }
                }
                "noglob" | "nocorrect" | "-" | "!" => at += 1,
                "time" => {
                    at += 1;
                    if args.get(at).and_then(Arg::literal).as_deref() == Some("-p") {
                        at += 1;
                    }
                }
                _ => break,
            }
        }
        let Some(program) = args.get(at).and_then(Arg::literal) else {
            return;
        };
        let rest = &args[at + 1..];
        let operands = &operands[(at + 1).min(operands.len())..];
        let base = basename(&program).to_string();
        let builtin = !program.contains('/');
        if builtin && !bypass {
            if let Some(names) = self.state.functions.get(&program).cloned() {
                // A function the line defines, or a builtin it disables.
                self.mark_unknown(names, "a variable a function may set");
                self.special = true;
                if matches!(program.as_str(), "printf" | "echo" | "print") {
                    self.output(Err("a function that redefines a command the guard models"));
                }
                return;
            }
        }
        if let Some((_, dialect)) = SHELLS.iter().find(|(shell, _)| *shell == base) {
            self.child_shell(*dialect, rest, prefix, stdin_code, false);
            return;
        }
        if !builtin || LAUNCHERS.contains(&base.as_str()) || SUPPLIERS.contains(&base.as_str()) {
            self.external(&base, rest, prefix, stdin_code);
            return;
        }
        self.special = SPECIAL_BUILTINS.contains(&program.as_str());
        self.silent = SILENT.contains(&program.as_str())
            || (!rest.is_empty()
                && (DECLARERS.contains(&program.as_str())
                    || matches!(program.as_str(), "set" | "shopt" | "trap" | "alias")));
        match program.as_str() {
            "pwd" if rest.is_empty() && self.state.directory.is_some() => {
                let directory = crate::git::GitName::from_os_str(
                    self.state.directory.as_ref().unwrap().as_os_str(),
                );
                self.output(match directory.rule_text() {
                    Ok(text) => Ok(Value::text(&format!("{text}\n"))),
                    Err(_) => Err("working directory is not UTF-8"),
                });
            }
            "printf" => self.printf(rest, certain),
            "echo" => {
                let value = match self.state.opt_echo {
                    Some(reason) => Err(reason),
                    None => flatten(rest).and_then(|fields| self.echo(&fields, false)),
                };
                self.output(value);
            }
            "print" if self.dialect == Dialect::Zsh => self.print(rest),
            "eval" => self.eval(rest),
            "set" => self.set(rest),
            "shift" => {
                self.state.positional = match std::mem::take(&mut self.state.positional) {
                    Positional::Set { mut values, more }
                        if rest.is_empty() && !more && values.len() > 1 =>
                    {
                        values.remove(1);
                        Positional::Set { values, more }
                    }
                    _ => Positional::Unknown("a positional parameter the line shifts"),
                };
            }
            "unset" => {
                for arg in rest {
                    match arg.literal() {
                        Some(name) if is_identifier(&name) => {
                            self.set_unknown(&name, "a variable the line unsets");
                        }
                        Some(option) if option.starts_with('-') => {}
                        _ => self.changed_all(),
                    }
                }
            }
            "cd" | "pushd" | "popd" | "chdir" => {
                self.silent = program != "pushd"
                    && program != "popd"
                    && rest.first().and_then(Arg::literal).as_deref() != Some("-");
                for name in ["PWD", "OLDPWD", "DIRSTACK", "dirstack"] {
                    self.set_unknown(name, "a directory variable the line changes");
                }
            }
            "let" => {
                for arg in rest {
                    if let Some(text) = arg.literal() {
                        let names = arith_assigned(&text);
                        self.mark_unknown(names, "an arithmetic assignment");
                    } else {
                        self.changed_all();
                    }
                }
            }
            name if DECLARERS.contains(&name) => self.declare(name, operands, rest, certain),
            // Code from outside the line: what it sets is judged as it was.
            "source" | "." => self.changed_all(),
            "alias" => {
                for arg in rest {
                    match arg.literal() {
                        Some(word) => {
                            if let Some((name, text)) = word.split_once('=') {
                                self.state
                                    .aliases
                                    .insert(name.to_string(), Some(text.to_string()));
                            }
                        }
                        None => self.changed_all(),
                    }
                }
            }
            "unalias" => {
                for arg in rest {
                    if let Some(name) = arg.literal() {
                        self.state.aliases.remove(&name);
                    }
                }
            }
            "shopt" if self.dialect == Dialect::Bash => {
                let names: Vec<Option<String>> = rest
                    .iter()
                    .map(Arg::literal)
                    .filter(|w| !w.as_deref().is_some_and(|w| w.starts_with('-')))
                    .collect();
                self.options(&names);
            }
            "setopt" | "unsetopt" | "emulate" if self.dialect == Dialect::Zsh => {
                self.silent = true;
                let names: Vec<Option<String>> = if program == "emulate" {
                    vec![None]
                } else {
                    rest.iter().map(Arg::literal).collect()
                };
                self.options(&names);
            }
            "enable" | "disable" => {
                // A builtin turned off runs some other program.
                self.silent = true;
                for arg in rest {
                    match arg.literal() {
                        Some(name) if !name.starts_with('-') => {
                            self.state.functions.insert(name, Vec::new());
                        }
                        Some(_) => {}
                        None => {
                            for name in ["printf", "echo", "print"] {
                                self.state.functions.insert(name.to_string(), Vec::new());
                            }
                        }
                    }
                }
            }
            "trap" => self.trap(rest),
            "read" | "mapfile" | "readarray" | "getopts" => {
                self.silent = true;
                let from_line = stdin_code.is_some() || self.stdin_literal;
                for arg in rest {
                    match arg.literal() {
                        Some(name) if is_identifier(&name) => self.outside(&name, from_line),
                        Some(_) => {}
                        None => self.changed_all(),
                    }
                }
                for name in ["REPLY", "reply", "OPTARG", "OPTIND", "MAPFILE"] {
                    self.outside(name, from_line);
                }
            }
            name if READERS.contains(&name) => {
                for arg in rest {
                    match arg.literal() {
                        Some(name) if is_identifier(&name) => {
                            self.set_unknown(&name, "a variable a builtin reads into");
                        }
                        Some(_) => {}
                        None => self.changed_all(),
                    }
                }
            }
            _ => {}
        }
    }

    /// A command whose first word is an alias the line defines: its text,
    /// read as the shell expands it, outputs not claimed exact.
    fn alias_call(&mut self, operands: &[&Tok]) -> bool {
        let Some(Tok::Word(first)) = operands.first() else {
            return false;
        };
        let Some(text) = first
            .plain()
            .and_then(|n| self.state.aliases.get(n))
            .cloned()
        else {
            return false;
        };
        self.special = true;
        match text {
            Some(text) => {
                let rest: Vec<&str> = operands[1..]
                    .iter()
                    .filter_map(|tok| match tok {
                        Tok::Word(word) => Some(word.raw.as_str()),
                        _ => None,
                    })
                    .collect();
                self.inexact += 1;
                self.eval_text(&format!("{text} {}", rest.join(" ")));
                self.inexact -= 1;
                self.silent = true;
            }
            None => self.changed_all(),
        }
        true
    }

    /// zsh `print`; `-v NAME` assigns what it would write.
    fn print(&mut self, rest: &[Arg]) {
        let mut taking = false;
        for arg in rest {
            let Some(word) = arg.literal() else {
                if taking {
                    self.changed_all();
                }
                break;
            };
            if taking {
                self.set_unknown(&word, "a variable print assigns");
                break;
            }
            match word.strip_prefix('-') {
                Some(flags) if !flags.is_empty() && flags != "-" => taking = flags.contains('v'),
                _ => break,
            }
        }
        let value = match self.state.opt_echo {
            Some(reason) => Err(reason),
            None => flatten(rest).and_then(|fields| print_output(&fields)),
        };
        self.output(value);
    }

    /// `trap CODE SIGNAL...`: the code may run at any later point, so what
    /// it assigns stays unknown.
    fn trap(&mut self, rest: &[Arg]) {
        let Some(first) = rest.first() else {
            return;
        };
        if rest.len() < 2 {
            return;
        }
        match first.literal() {
            Some(code) => {
                let log_at = self.log.len();
                self.uncertain = self.uncertain.saturating_add(1);
                self.eval_text(&code);
                self.uncertain = self.uncertain.saturating_sub(1);
                let names: Vec<String> = self.log[log_at.min(self.log.len())..].to_vec();
                for name in names {
                    self.set_unknown(&name, "a variable a trap may set");
                    self.state.readonly.insert(name);
                }
                self.silent = true;
            }
            None => self.changed_all(),
        }
    }

    fn output(&mut self, value: Result<Value, &'static str>) {
        self.wrote = true;
        let value = match (self.state.world, value) {
            (Some(reason), _) | (None, Err(reason)) => Composed::Unresolved(reason),
            (None, Ok(value)) => Composed::Resolved(value),
        };
        // A glob that matches nothing stops zsh before the command runs:
        // the value is judged, but what reaches stdout is not claimed.
        if self.glob_arg {
            self.approx.set(true);
        }
        let exact = !self.approx.get() && self.inexact == 0;
        self.record(match &value {
            Composed::Resolved(v) => Ok(v.clone()),
            Composed::Unresolved(reason) => Err(*reason),
        });
        self.events.push(Event {
            depth: self.depth + usize::from(self.uncertain > 0),
            output: true,
            exact,
            value,
        });
    }

    fn printf(&mut self, rest: &[Arg], certain: bool) {
        let mut at = 0;
        let mut target: Option<String> = None;
        loop {
            match rest.get(at).and_then(Arg::literal).as_deref() {
                Some("-v") => {
                    target = rest.get(at + 1).and_then(Arg::literal);
                    if target.as_deref().is_none_or(|t| !is_identifier(t)) {
                        self.changed_all();
                        self.output(Err("a printf -v the guard cannot read"));
                        return;
                    }
                    at += 2;
                }
                Some(word) if word.starts_with("-v") && word.len() > 2 => {
                    target = Some(word[2..].to_string());
                    at += 1;
                }
                Some("--") => {
                    at += 1;
                    break;
                }
                _ => break,
            }
        }
        let operands = &rest[at.min(rest.len())..];
        let value = flatten(operands).and_then(|fields| {
            if operands.iter().any(|arg| arg.loose) {
                return Err("an unquoted expansion the shell may split");
            }
            let (format, values) = fields.split_first().ok_or("a printf without a format")?;
            if format
                .exact()
                .is_some_and(|f| f.starts_with('-') && f.len() > 1)
                && at == 0
            {
                return Err("a printf option the guard does not model");
            }
            if self.dialect == Dialect::Sh && target.is_some() {
                return Err("printf -v outside bash and zsh");
            }
            printf_output(self.dialect, false, format, values)
        });
        if let Some(name) = target {
            match &value {
                Ok(v) => self.store(&name, Var::Scalar(v.clone()), certain),
                Err(reason) => self.store(&name, Var::Unknown(reason), certain),
            }
        }
        self.output(value);
    }

    /// Bash, zsh and other `echo`s: `external` for one that is not the
    /// shell's builtin.
    fn echo(&self, fields: &[Value], external: bool) -> Result<Value, &'static str> {
        if external || self.dialect == Dialect::Sh {
            if fields.iter().any(|f| f.any_text(|c| c == '\\')) {
                return Err("an echo escape outside the bash and zsh builtins");
            }
            if fields
                .first()
                .and_then(|f| f.pieces().first())
                .is_some_and(|p| matches!(p, Piece::Text(t) if t.starts_with('-')))
            {
                return Err("an echo option outside the bash and zsh builtins");
            }
            return Ok(join(fields, " ", true));
        }
        let mut at = 0;
        let (mut newline, mut e, mut big_e) = (true, false, false);
        while let Some(field) = fields.get(at) {
            let Some(text) = field.exact() else {
                break;
            };
            if self.dialect == Dialect::Zsh && text == "-" {
                at += 1;
                break;
            }
            let Some(flags) = text.strip_prefix('-') else {
                break;
            };
            if flags.is_empty() || !flags.chars().all(|c| matches!(c, 'n' | 'e' | 'E')) {
                break;
            }
            newline &= !flags.contains('n');
            e |= flags.contains('e');
            big_e |= flags.contains('E');
            at += 1;
        }
        if e && big_e {
            return Err("an echo option cluster the guard does not model");
        }
        let decoding = match self.dialect {
            Dialect::Bash => e,
            _ => !big_e,
        };
        let mut decoded = Vec::new();
        for field in &fields[at..] {
            decoded.push(if decoding {
                decode_value(field, Escapes::Echo)?
            } else {
                field.clone()
            });
        }
        Ok(join(&decoded, " ", newline))
    }

    fn eval(&mut self, rest: &[Arg]) {
        self.silent = true;
        let fields = match flatten(rest) {
            Ok(fields) => fields,
            Err(reason) => {
                // Code the line composes in a way the guard cannot read.
                self.emit(false, Composed::Unresolved(reason));
                self.changed_all();
                return;
            }
        };
        match fields.iter().map(Value::exact).collect::<Option<Vec<_>>>() {
            Some(words) => self.eval_text(&words.join(" ")),
            // Code from outside the line, as `source` runs.
            None => self.changed_all(),
        }
    }

    /// Run `text` in this shell, as `eval` does: a group of its own.
    fn eval_text(&mut self, text: &str) {
        if self.nesting >= 8 {
            self.emit(
                false,
                Composed::Unresolved("nesting deeper than the guard reads"),
            );
            return;
        }
        let parent_first = self.frames.last().is_some_and(|f| f.list.commands <= 1);
        let parent_uncertain = !self.certain(false);
        let log_at = self.log.len();
        let frames = std::mem::replace(
            &mut self.frames,
            vec![Frame {
                log_at,
                parent_first,
                parent_uncertain,
                ..Frame::default()
            }],
        );
        let (here, wrote, silent, special) =
            (self.stdout_here, self.wrote, self.silent, self.special);
        self.nesting += 1;
        self.run_text(text);
        self.nesting -= 1;
        self.frames = frames;
        self.stdout_here = here;
        self.wrote = wrote;
        self.silent = silent;
        self.special = special;
        let names: Vec<String> = self.log[log_at.min(self.log.len())..].to_vec();
        if parent_uncertain {
            self.mark_unknown(names, MAY_ASSIGN);
        } else if !parent_first {
            self.frame().list.conditional.extend(names);
        }
    }

    fn set(&mut self, rest: &[Arg]) {
        const OPTIONS: &[&str] = &[
            "errexit",
            "nounset",
            "xtrace",
            "verbose",
            "noglob",
            "noclobber",
            "errtrace",
            "functrace",
            "notify",
            "hashall",
            "monitor",
        ];
        let mut at = 0;
        while let Some(arg) = rest.get(at) {
            let Some(word) = arg.literal() else {
                self.options(&[None]);
                self.state.positional = Positional::Unknown("a positional parameter the line sets");
                return;
            };
            if word == "--" || !(word.starts_with('-') || word.starts_with('+')) {
                let from = if word == "--" { at + 1 } else { at };
                self.state.positional = match flatten(&rest[from.min(rest.len())..]) {
                    Ok(values) => Positional::Set {
                        values: std::iter::once(Value::open()).chain(values).collect(),
                        more: false,
                    },
                    Err(reason) => Positional::Unknown(reason),
                };
                return;
            }
            if word == "-o" || word == "+o" {
                let name = rest.get(at + 1).and_then(Arg::literal);
                if !name.as_deref().is_some_and(|n| OPTIONS.contains(&n)) {
                    if self.dialect == Dialect::Sh {
                        // An option a POSIX shell lacks ends the whole line.
                        self.state.world = Some("a shell option a POSIX shell may refuse");
                        return;
                    }
                    self.options(&[name]);
                }
                at += 2;
                continue;
            }
            if word.len() < 2 || !word[1..].chars().all(|c| "euxvfCBhT".contains(c)) {
                self.options(&[None]);
            }
            at += 1;
        }
    }

    fn declare(&mut self, name: &str, operands: &[&Tok], rest: &[Arg], certain: bool) {
        const OPTIONED: &str = "a declaration builtin with options";
        let options = rest
            .iter()
            .take_while(|a| a.literal().is_some_and(|w| w.starts_with(['-', '+'])))
            .count();
        // A POSIX shell has only `export`, `readonly` and `local`.
        let foreign =
            self.dialect == Dialect::Sh && !matches!(name, "export" | "readonly" | "local");
        let option_text: String = rest[..options].iter().filter_map(Arg::literal).collect();
        // A name reference or tied variable writes another name.
        let reference = option_text.contains(['n', 'T']);
        for (at, tok) in operands.iter().enumerate() {
            if at < options {
                continue;
            }
            let operand_name = match tok {
                Tok::Array { name, .. } => Some(name.clone()),
                Tok::Word(word) => match assignment_shape(word) {
                    Some(Shape::Scalar { name, .. } | Shape::Element(name)) => Some(name),
                    None => rest
                        .get(at)
                        .and_then(Arg::literal)
                        .filter(|n| is_identifier(n)),
                },
                _ => None,
            };
            let Some(operand_name) = operand_name else {
                self.changed_all();
                continue;
            };
            let is_assignment = matches!(tok, Tok::Array { .. })
                || matches!(tok, Tok::Word(w) if assignment_shape(w).is_some());
            if options > 0 || foreign {
                if reference && is_assignment {
                    let target = self.assignment(tok).and_then(|(_, var)| match var {
                        Var::Scalar(value) => value.exact().filter(|t| is_identifier(t)),
                        _ => None,
                    });
                    match target {
                        Some(target) => self.set_unknown(&target, OPTIONED),
                        None => self.state.world = Some("a name reference the guard cannot read"),
                    }
                }
                self.set_unknown(&operand_name, OPTIONED);
                continue;
            }
            if !is_assignment {
                // `export NAME` and `readonly NAME` keep the value.
                if name == "export" {
                    self.state.exported.insert(operand_name);
                } else if name == "readonly" {
                    self.state.readonly.insert(operand_name);
                } else {
                    self.set_unknown(&operand_name, "a declaration without a value");
                }
                continue;
            }
            let Some((_, var)) = self.assignment(tok) else {
                continue;
            };
            // Bash 3.2 may split a declaration operand: one field only.
            let var = match var {
                Var::Scalar(value)
                    if self.dialect != Dialect::Zsh
                        && !matches!(self.unquoted_ok(&value), Ok(false))
                        && word_has_unquoted_param(tok) =>
                {
                    Var::Unknown("a declaration operand the shell may split")
                }
                other => other,
            };
            match name {
                "local" if self.dialect != Dialect::Zsh => {
                    self.set_unknown(&operand_name, "a local outside a function");
                }
                "integer" | "float" => self.set_unknown(&operand_name, OPTIONED),
                _ => {
                    self.store(&operand_name, var, certain);
                    if name == "export" {
                        self.state.exported.insert(operand_name.clone());
                    }
                    if name == "readonly" {
                        self.state.readonly.insert(operand_name);
                    }
                }
            }
        }
    }

    /// A shell started as a program: its `-c` body, or the code a heredoc
    /// or here-string gives it, read with what it inherits. `more` when a
    /// program may pass it further arguments.
    fn child_shell(
        &mut self,
        dialect: Dialect,
        rest: &[Arg],
        prefix: &[(String, Var)],
        stdin_code: Option<&Value>,
        more: bool,
    ) {
        let mut at = 0;
        let mut command = false;
        let mut dialect = dialect;
        while let Some(word) = rest.get(at).and_then(Arg::literal) {
            if word == "--posix" {
                dialect = Dialect::Sh;
            } else if ["-o", "+o", "-O", "+O", "--rcfile", "--init-file"].contains(&word.as_str()) {
                at += 1;
            } else if word.starts_with('-') && !word.starts_with("--") && word.contains('c') {
                command = true;
            } else if !word.starts_with(['-', '+']) {
                break;
            }
            at += 1;
        }
        let code = if command {
            match rest.get(at).map(|a| &a.fields) {
                Some(Ok(fields)) if fields.len() == 1 => fields[0].exact(),
                _ => None,
            }
        } else {
            stdin_code.and_then(Value::exact)
        };
        let Some(code) = code else {
            return;
        };
        let mut state = State {
            world: self.state.world,
            ..State::default()
        };
        for (name, var) in &self.state.vars {
            let inherited = match var {
                Var::Scalar(value) if self.state.exported.contains(name) => {
                    Var::Scalar(value.clone())
                }
                _ => Var::Unknown("a variable the parent shell may pass"),
            };
            state.vars.insert(name.clone(), inherited);
        }
        if dialect != Dialect::Zsh {
            state.vars.remove("IFS");
        }
        for (name, var) in prefix {
            state.vars.insert(name.clone(), var.clone());
        }
        // `sh -c CODE NAME ARG...`: `$0` is NAME, `$1` the first ARG.
        let given = if command {
            rest.get(at + 1..).unwrap_or_default()
        } else {
            &[]
        };
        state.positional = match flatten(given) {
            Ok(values) if values.is_empty() => Positional::Set {
                values: vec![Value::open()],
                more,
            },
            Ok(values) => Positional::Set { values, more },
            Err(reason) => Positional::Unknown(reason),
        };
        self.nested(&code, state, dialect, false);
    }

    /// A program run through a launcher, by path, or by a program that
    /// supplies arguments (`find -exec`, `xargs`): an `echo`, `printf` or
    /// shell it runs. A path `find` puts in place of `{}` is outside text.
    fn external(
        &mut self,
        base: &str,
        rest: &[Arg],
        prefix: &[(String, Var)],
        stdin_code: Option<&Value>,
    ) {
        let supplier = SUPPLIERS.contains(&base);
        let (name, after) = if LAUNCHERS.contains(&base) || supplier {
            // `find` runs what follows its `-exec`, `-execdir`, `-ok` or `-okdir`.
            let from = if base == "find" {
                rest.iter()
                    .position(|arg| {
                        arg.literal().is_some_and(|w| {
                            matches!(w.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir")
                        })
                    })
                    .map_or(rest.len(), |at| at + 1)
            } else {
                0
            };
            let found = rest.iter().enumerate().skip(from).find_map(|(at, arg)| {
                let word = arg.literal()?;
                let name = basename(&word).to_string();
                (["printf", "echo"].contains(&name.as_str())
                    || SHELLS.iter().any(|(s, _)| *s == name))
                .then_some((name, at + 1))
            });
            let Some(found) = found else {
                return;
            };
            found
        } else if ["printf", "echo"].contains(&base) || SHELLS.iter().any(|(s, _)| *s == base) {
            (base.to_string(), 0)
        } else {
            // Any other program named by a path runs as itself: what it
            // writes is outside text, as its bare name's would be.
            return;
        };
        let mut args: Vec<Arg> = Vec::new();
        let mut more = supplier && base != "find";
        for arg in &rest[after.min(rest.len())..] {
            match arg.literal().as_deref() {
                Some(";" | "+") if base == "find" => {
                    more |= arg.literal().as_deref() == Some("+");
                    break;
                }
                Some("{}") if base == "find" => args.push(Arg {
                    fields: Ok(vec![Value::open()]),
                    loose: false,
                    globbed: false,
                }),
                _ => args.push(Arg {
                    fields: arg.fields.clone(),
                    loose: arg.loose,
                    globbed: arg.globbed,
                }),
            }
        }
        if let Some((_, dialect)) = SHELLS.iter().find(|(shell, _)| *shell == name) {
            let mut env: Vec<(String, Var)> = prefix.to_vec();
            if base == "env" {
                for arg in &rest[..after.saturating_sub(1)] {
                    if let Some((n, v)) = arg.literal().as_deref().and_then(|w| w.split_once('=')) {
                        if is_identifier(n) {
                            env.push((n.to_string(), Var::Scalar(Value::text(v))));
                        }
                    }
                }
            }
            self.child_shell(*dialect, &args, &env, stdin_code, more);
            return;
        }
        if more {
            self.output(Err("a command another program supplies arguments to"));
            return;
        }
        let value = flatten(&args).and_then(|fields| match name.as_str() {
            "printf" => {
                let (format, values) = fields.split_first().ok_or("a printf without a format")?;
                printf_output(self.dialect, true, format, values)
            }
            "echo" => self.echo(&fields, true),
            _ => Err("a program the guard does not model"),
        });
        self.output(value);
    }
}

/// What changing option `name` may make unknown: `echo`/`print` decoding,
/// and word expansion. Options that touch neither are listed; any other
/// may touch both.
fn option_effect(dialect: Dialect, name: &str) -> (bool, bool) {
    const HARMLESS: &[&str] = &[
        "errexit",
        "errreturn",
        "nounset",
        "unset",
        "xtrace",
        "verbose",
        "pipefail",
        "nullglob",
        "nomatch",
        "globdots",
        "dotglob",
        "caseglob",
        "nocaseglob",
        "interactivecomments",
        "localoptions",
        "localtraps",
        "clobber",
        "noclobber",
        "notify",
        "monitor",
        "hup",
        "checkjobs",
        "beep",
        "promptsubst",
        "promptpercent",
        "autocd",
        "correct",
        "correctall",
        "warncreateglobal",
        "extglob",
        "failglob",
        "globstar",
        "nocasematch",
        "expandaliases",
        "lastpipe",
        "checkwinsize",
        "histappend",
        "cmdhist",
        "inheriterrexit",
        "sourcepath",
        "cdspell",
        "dirspell",
        "huponexit",
        "promptvars",
        "errtrace",
        "functrace",
        "hashall",
    ];
    const ECHO: &[&str] = &["xpgecho", "bsdecho"];
    let key: String = name
        .chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect();
    let known = |k: &str| HARMLESS.contains(&k) || ECHO.contains(&k);
    // zsh reads `noNAME` as NAME turned off.
    let key = match key.strip_prefix("no") {
        Some(rest) if dialect == Dialect::Zsh && !known(&key) && known(rest) => rest.to_string(),
        _ => key,
    };
    if HARMLESS.contains(&key.as_str()) {
        (false, false)
    } else if ECHO.contains(&key.as_str()) {
        (true, false)
    } else {
        (true, true)
    }
}

/// Whether a redirection sends the command's stdout somewhere else.
fn stdout_redirect(tok: &Tok) -> bool {
    let Tok::Redir { op, .. } = tok else {
        return false;
    };
    let digits = op.chars().take_while(char::is_ascii_digit).count();
    let (fd, rest) = op.split_at(digits);
    (fd.is_empty() || fd == "1") && (rest.starts_with('>') || rest.starts_with("&>"))
}

fn word_has_unquoted_param(tok: &Tok) -> bool {
    matches!(tok, Tok::Word(word) if word.parts.iter().any(|p| matches!(p, Part::Param { quoted: false, .. })))
}

fn join(fields: &[Value], sep: &str, newline: bool) -> Value {
    // A field of unknown text alone may expand to nothing: the separators
    // around it may be absent, but they still separate it when judged.
    let unknown = |field: &Value| {
        !field.0.is_empty()
            && field
                .0
                .iter()
                .all(|p| matches!(p, Piece::Open | Piece::Glob(_)))
    };
    let mut out = Value::default();
    for (at, field) in fields.iter().enumerate() {
        if at > 0 {
            if unknown(field) || unknown(&fields[at - 1]) {
                out.0.push(Piece::Soft(sep.to_string()));
            } else {
                out.push_text(sep);
            }
        }
        out.append(field);
    }
    if newline {
        out.push_text("\n");
    }
    out
}

/// zsh `print` with `-r`, `-n` and `-l`.
fn print_output(fields: &[Value]) -> Result<Value, &'static str> {
    let mut at = 0;
    let (mut raw, mut newline, mut lines) = (false, true, false);
    while let Some(text) = fields.get(at).and_then(Value::exact) {
        if text == "-" || text == "--" {
            at += 1;
            break;
        }
        let Some(flags) = text.strip_prefix('-') else {
            break;
        };
        if flags.is_empty() {
            break;
        }
        if !flags.chars().all(|c| matches!(c, 'r' | 'n' | 'l')) {
            return Err("a print option the guard does not model");
        }
        raw |= flags.contains('r');
        newline &= !flags.contains('n');
        lines |= flags.contains('l');
        at += 1;
    }
    let operands = &fields[at..];
    if operands.is_empty() && lines {
        return Err("a print option the guard does not model");
    }
    let mut decoded = Vec::new();
    for field in operands {
        decoded.push(if raw {
            field.clone()
        } else {
            decode_value(field, Escapes::Print)?
        });
    }
    Ok(join(&decoded, if lines { "\n" } else { " " }, newline))
}

/// What `printf` writes: the format applied to the arguments, reused while
/// arguments remain. `external` for a `printf` that is not the shell's
/// builtin, read only with `%s` and no escapes.
fn printf_output(
    dialect: Dialect,
    external: bool,
    format: &Value,
    values: &[Value],
) -> Result<Value, &'static str> {
    enum Seg {
        Text(String),
        Plain,
        Decode,
        Quote,
    }
    let format = format
        .exact()
        .ok_or("a printf format the guard cannot resolve")?;
    let plain_only = external || dialect == Dialect::Sh;
    let mut segs: Vec<Seg> = Vec::new();
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if plain_only && matches!(chars.peek(), Some('x' | 'e' | 'E')) {
                    return Err("a printf escape outside the bash and zsh builtins");
                }
                let mut escape = String::from('\\');
                // Take the longest escape the format grammar reads.
                if let Some(next) = chars.next() {
                    escape.push(next);
                    let more = if next == 'x' || ('0'..='7').contains(&next) {
                        2
                    } else {
                        0
                    };
                    for _ in 0..more {
                        match chars.peek() {
                            Some(d)
                                if (next == 'x' && d.is_ascii_hexdigit())
                                    || (next != 'x' && ('0'..='7').contains(d)) =>
                            {
                                escape.push(*d);
                                chars.next();
                            }
                            _ => break,
                        }
                    }
                }
                text.push_str(&decode(&escape, Escapes::Format)?);
            }
            '%' => match chars.next() {
                Some('%') => text.push('%'),
                Some('s') => {
                    segs.push(Seg::Text(std::mem::take(&mut text)));
                    segs.push(Seg::Plain);
                }
                Some('b') if !plain_only => {
                    segs.push(Seg::Text(std::mem::take(&mut text)));
                    segs.push(Seg::Decode);
                }
                Some('q') if !plain_only => {
                    segs.push(Seg::Text(std::mem::take(&mut text)));
                    segs.push(Seg::Quote);
                }
                _ => return Err("a printf conversion the guard cannot resolve"),
            },
            _ => text.push(c),
        }
    }
    segs.push(Seg::Text(text));
    let conversions = segs.iter().filter(|s| !matches!(s, Seg::Text(_))).count();
    let mut out = Value::default();
    let mut values = values.iter();
    loop {
        for seg in &segs {
            match seg {
                Seg::Text(text) => out.push_text(text),
                Seg::Plain => {
                    if let Some(value) = values.next() {
                        out.append(value);
                    }
                }
                Seg::Decode => {
                    if let Some(value) = values.next() {
                        out.append(&decode_value(value, Escapes::PercentB(dialect))?);
                    }
                }
                Seg::Quote => {
                    let value = values
                        .next()
                        .and_then(Value::exact)
                        .filter(|v| {
                            !v.is_empty()
                                && v.chars()
                                    .all(|c| c.is_ascii_alphanumeric() || "._-/@:+".contains(c))
                        })
                        .ok_or("a printf %q conversion the guard cannot resolve")?;
                    out.push_text(&value);
                }
            }
        }
        if conversions == 0 || values.len() == 0 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(line: &str, dialect: Dialect) -> Option<String> {
        match final_output(line, dialect) {
            Some(Composed::Resolved(value)) => value.exact(),
            _ => None,
        }
    }

    #[test]
    fn literal_compositions_resolve_per_dialect() {
        for d in [Dialect::Bash, Dialect::Zsh] {
            assert_eq!(
                out("a=1733; b=\"${a}02\"; printf '%s\\n' \"$b\"", d).as_deref(),
                Some("173302\n")
            );
            assert_eq!(
                out("f='%s%s\\n'; printf \"$f\" 1733 02", d).as_deref(),
                Some("173302\n")
            );
            assert_eq!(out("echo -ne '\\x31\\x37\\n'", d).as_deref(), Some("17\n"));
            assert_eq!(
                out("export a=notes.txt; printf '%s\\n' \"$a\"", d).as_deref(),
                Some("notes.txt\n")
            );
        }
        assert_eq!(
            out("echo '\\x31'", Dialect::Bash).as_deref(),
            Some("\\x31\n")
        );
        assert_eq!(out("echo '\\x31'", Dialect::Zsh).as_deref(), Some("1\n"));
        assert_eq!(
            out("n=v; v=173302; printf '%s\\n' \"${!n}\"", Dialect::Bash).as_deref(),
            Some("173302\n")
        );
    }

    #[test]
    fn unmodelled_constructs_are_unresolved() {
        for line in [
            "declare -x a=1; printf '%s\\n' \"$a\"",
            "arr=(a b); arr[0]=x; printf '%s\\n' \"${arr[0]}\"",
            "echo -eE '\\x31'",
            "printf '\\u0031\\n'",
            "a=$(printf x | tr x y); b=${a%y}; printf '%s\\n' \"$b\"",
            "false && a=1; printf '%s\\n' \"$a\"",
        ] {
            for d in [Dialect::Bash, Dialect::Zsh] {
                assert!(
                    matches!(final_output(line, d), Some(Composed::Unresolved(_))),
                    "{d:?}: {line}"
                );
            }
        }
        assert!(matches!(
            final_output("n=v; v=1; printf '%s\\n' \"${!n}\"", Dialect::Zsh),
            Some(Composed::Unresolved(_))
        ));
    }

    /// Grok round 12: a program named by a path runs as itself, so what
    /// it writes is outside text, never its arguments read as `echo`.
    #[test]
    fn a_path_program_writes_outside_text() {
        for d in [Dialect::Bash, Dialect::Zsh] {
            for line in [
                "printf '%s\\n' \"$(/usr/bin/basename /x/.claude)\"",
                "printf '%s\\n' \"$(/bin/cat /tmp/x)\"",
                "printf '%s\\n' \"$(nohup /usr/bin/tr x y)\"",
            ] {
                let Some(Composed::Resolved(value)) = final_output(line, d) else {
                    panic!("{d:?}: {line}");
                };
                assert_eq!(
                    value.pieces(),
                    &[Piece::Open, Piece::Text("\n".into())],
                    "{d:?}: {line}"
                );
            }
            assert_eq!(
                out("printf '%s\\n' \"$(/bin/echo 17)\"", d).as_deref(),
                Some("17\n")
            );
        }
    }

    #[test]
    fn an_offline_variable_stays_unknown_text() {
        let value = final_output("printf '%s/notes.txt\\n' \"$HOME\"", Dialect::Bash);
        let Some(Composed::Resolved(value)) = value else {
            panic!("{value:?}");
        };
        assert_eq!(value.render(), "$/notes.txt\n");
        assert!(value.matches("/home/x/notes.txt\n"));
        assert!(texts_in("python3 tool.py \"$ROOT/$NAME\"", Dialect::Bash).is_empty());
    }
}
