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
        for event in texts(line, dialect) {
            if !found.contains(&event.value) {
                found.push(event.value);
            }
        }
    }
    found
}

/// One word of a simple command, as the install reader reads it
/// (TSK-1476).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Spelt {
    /// Text the shell passes exactly as written.
    Plain(String),
    /// A word that is one command substitution, with its body.
    Output(String),
    /// An input process substitution (`<(...)`), with its body.
    Process(String),
    /// An output process substitution (`>(...)`), with its body, which
    /// reads what the command writes there.
    Sink(String),
    /// Any other word the shell fills in, as written.
    Open(String),
}

/// Where a redirection takes its text from, or writes it to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// A literal here-string, or a here-document the shell does not expand.
    Text(String),
    /// A plain literal path.
    Path(String),
    /// A process substitution, with its body.
    Process(String),
    /// A here-string or here-document the shell fills in: text the line
    /// spells that the reader does not settle.
    Spelled,
    /// A copy or close of a descriptor by its number (`2>&1`, `<&3`,
    /// `>&-`): no file is named.
    Copy,
    /// Anything else, such as a target the shell fills in.
    Open,
}

/// One redirection of a simple command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Redirect {
    /// The descriptor it sets.
    pub(crate) fd: u32,
    /// Whether the command reads from it.
    pub(crate) input: bool,
    /// Whether the command may write to it (`>`, `>>`, `>|`, `>&`, `&>`,
    /// `&>>`, `<>`).
    pub(crate) writes: bool,
    /// What it reads or writes.
    pub(crate) source: Source,
}

/// One simple command of a line, as the install reader reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Simple {
    /// Its words, in order.
    pub(crate) words: Vec<Spelt>,
    /// Its words as written, joined by a blank.
    pub(crate) raw: String,
    /// Whether `|` hands the previous command's output to this one.
    pub(crate) piped: bool,
    /// Its redirections, in order.
    pub(crate) redirects: Vec<Redirect>,
}

/// The simple commands `line` is made of, when it is a list of them joined
/// by `|`, `;`, `&&`, `||` or a newline. `None` for anything else: grammar
/// the composer does not read, a background job, a group, a subshell, a
/// loop, a conditional, a function or an array (TSK-1476).
pub(crate) fn simple_commands(line: &str) -> Option<Vec<Simple>> {
    const COMPOUND: &[&str] = &[
        "{", "}", "!", "for", "while", "until", "if", "then", "elif", "else", "fi", "do", "done",
        "case", "esac", "select", "function", "coproc", "repeat", "foreach", "[[", "time",
    ];
    let (tokens, docs) = lex(line).ok()?;
    let mut commands = Vec::new();
    let mut current = Simple {
        words: Vec::new(),
        raw: String::new(),
        piped: false,
        redirects: Vec::new(),
    };
    let mut raw = Vec::new();
    for tok in &tokens {
        match tok {
            Tok::Word(word) => {
                if current.words.is_empty() && COMPOUND.contains(&word.raw.as_str()) {
                    return None;
                }
                raw.push(word.raw.clone());
                current.words.push(spelt(word));
            }
            Tok::Redir {
                op,
                target,
                heredoc,
            } => current
                .redirects
                .push(redirect(op, target.as_ref(), *heredoc, &docs)?),
            Tok::Op(op @ ("|" | ";" | "&&" | "||" | "\n")) => {
                if !current.words.is_empty() || !current.redirects.is_empty() {
                    current.raw = std::mem::take(&mut raw).join(" ");
                    let piped = *op == "|";
                    commands.push(std::mem::replace(
                        &mut current,
                        Simple {
                            words: Vec::new(),
                            raw: String::new(),
                            piped,
                            redirects: Vec::new(),
                        },
                    ));
                } else if *op == "|" {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if !current.words.is_empty() || !current.redirects.is_empty() {
        current.raw = raw.join(" ");
        commands.push(current);
    } else if current.piped {
        return None;
    }
    Some(commands)
}

/// How the install reader reads one word.
fn spelt(word: &Word) -> Spelt {
    match word.parts.as_slice() {
        [Part::Subst { body, .. }] => return Spelt::Output(body.clone()),
        [Part::ProcSubst(body, false)] => return Spelt::Process(body.clone()),
        [Part::ProcSubst(body, true)] => return Spelt::Sink(body.clone()),
        _ => {}
    }
    let mut text = String::new();
    for (at, part) in word.parts.iter().enumerate() {
        match part {
            Part::Lit { text: lit, quoted } => {
                let expands = lit.contains(['*', '?', '[', ']', '{', '}', '~'])
                    || (at == 0 && lit.starts_with('='));
                if !quoted && expands {
                    return Spelt::Open(word.raw.clone());
                }
                text.push_str(lit);
            }
            Part::Ansi(body) if !body.contains('\\') => text.push_str(body),
            _ => return Spelt::Open(word.raw.clone()),
        }
    }
    Spelt::Plain(text)
}

/// How the install reader reads one redirection; `None` when its heredoc
/// is missing.
fn redirect(
    op: &str,
    target: Option<&Word>,
    heredoc: Option<usize>,
    docs: &[Heredoc],
) -> Option<Redirect> {
    let digits = op.bytes().take_while(u8::is_ascii_digit).count();
    let (fd, op) = op.split_at(digits);
    let input = op.starts_with('<');
    let fd = if fd.is_empty() {
        u32::from(!input)
    } else {
        fd.parse().unwrap_or(u32::MAX)
    };
    let source = if let Some(index) = heredoc {
        let doc = docs.get(index)?;
        match body_text(&doc.body) {
            Some(text) => Source::Text(text),
            None if doc.quoted => Source::Text(doc.body.raw.clone()),
            None => Source::Spelled,
        }
    } else {
        match target.map(spelt) {
            Some(Spelt::Process(body) | Spelt::Sink(body)) => Source::Process(body),
            Some(Spelt::Plain(text)) if op == "<<<" => Source::Text(text),
            Some(_) if op == "<<<" => Source::Spelled,
            Some(Spelt::Plain(text))
                if matches!(op, ">&" | "<&")
                    && (text == "-"
                        || (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))) =>
            {
                Source::Copy
            }
            Some(Spelt::Plain(text))
                if matches!(op, "<" | "<>" | ">" | ">>" | ">|" | "&>" | "&>>")
                    && !crate::hooks::git_guard::expands(&text) =>
            {
                Source::Path(text)
            }
            _ => Source::Open,
        }
    };
    let writes = !input || op == "<>";
    Some(Redirect {
        fd,
        input,
        writes,
        source,
    })
}

/// What a feed hands to the commands that read it, on a line read token by
/// token ([`compound`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fed {
    /// What one simple command writes to a pipe.
    Command(Simple),
    /// A literal here-string or here-document.
    Text(String),
    /// What a process substitution writes, with its body.
    Body(String),
    /// Text the line spells that this reading does not settle: a pipe from
    /// a group, loop or other compound command holding a printer, a
    /// here-string, a here-document or a substitution that holds one, or a
    /// here-string or here-document the shell fills in.
    Spelled,
    /// The output of programs the line runs, holding no text it spells.
    Output,
}

/// One place a line hands text to the commands that read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Feed {
    /// What it hands over.
    pub(crate) fed: Fed,
    /// Each simple command that may read it, as written.
    pub(crate) readers: Vec<String>,
}

/// A line read token by token ([`compound`]).
#[derive(Debug, Default)]
pub(crate) struct Compound {
    /// Each pipe, here-string, here-document and process substitution.
    pub(crate) feeds: Vec<Feed>,
    /// Each output process substitution written after a compound command:
    /// what the command writes, and the body (the one reader) that reads
    /// it.
    pub(crate) sinks: Vec<Feed>,
    /// Every simple command, at every depth of substitution.
    pub(crate) commands: Vec<Simple>,
    /// Whether a command on the line writes a file ([`writes_file`]), or a
    /// compound command on it has its output redirected to one.
    pub(crate) writes_file: bool,
    /// Whether the line writes text it spells to a file
    /// ([`writes_spelled_text`]).
    pub(crate) spelled_write: bool,
}

/// `line` read token by token, for grammar [`simple_commands`] does not
/// take apart: each pipe, here-string, here-document and process
/// substitution on any descriptor, with every simple command that may read
/// it, and each output process substitution set on a compound command
/// with what that command writes ([`Compound::sinks`]), through
/// groups, subshells, loops, conditionals and `case` branches and inside
/// every substitution (TSK-1476). A pipe's readers are every simple
/// command of the element after it; its text is that of the element before
/// it. `None` when the line, or text nested in it, is outside the grammar
/// this reading follows: a function, a coprocess, an unbalanced compound
/// command or text the lexer refuses.
pub(crate) fn compound(line: &str) -> Option<Compound> {
    let mut out = Compound::default();
    compound_into(line, 0, &mut out)?;
    Some(out)
}

/// One simple command of a line read token by token: its token span, as
/// read, and its words as written.
struct Unit {
    start: usize,
    end: usize,
    simple: Simple,
}

/// Separators that end an element of a pipeline or a list.
fn separates(tok: &Tok) -> bool {
    matches!(tok, Tok::Op("|" | "|&" | ";" | "&&" | "||" | "&" | "\n"))
}

#[allow(clippy::too_many_lines)] // one token walk
fn compound_into(line: &str, depth: usize, out: &mut Compound) -> Option<()> {
    if depth > 8 {
        return None;
    }
    let (tokens, docs) = lex(line).ok()?;
    let mut level = Vec::with_capacity(tokens.len());
    let mut closers: Vec<&str> = Vec::new();
    let mut units: Vec<Unit> = Vec::new();
    let mut current: Option<Unit> = None;
    // Redirections written after a compound command, by token.
    let mut attached: Vec<(usize, Redirect)> = Vec::new();
    let mut bodies = Vec::new();
    // Tokens of a `case` header or pattern, which end an element and are
    // never a pipe.
    let mut bound = vec![false; tokens.len()];
    // For each open `case`: whether a pattern is being read.
    let mut patterns: Vec<bool> = Vec::new();
    // The words of a `case` header still to read: its subject, then `in`.
    let mut header = 0u8;
    let finish = |current: &mut Option<Unit>, end: usize, units: &mut Vec<Unit>| {
        if let Some(mut unit) = current.take() {
            unit.end = end;
            units.push(unit);
        }
    };
    for (at, tok) in tokens.iter().enumerate() {
        if header > 0 || patterns.last() == Some(&true) {
            level.push(closers.len());
            bound[at] = true;
            let Tok::Word(word) = tok else {
                match tok {
                    Tok::Op(")") if header == 0 => {
                        if let Some(top) = patterns.last_mut() {
                            *top = false;
                        }
                    }
                    Tok::Op("|" | "(") if header == 0 => {}
                    _ => return None,
                }
                continue;
            };
            substitution_bodies(word, &mut bodies);
            match header {
                2 => header = 1,
                1 if word.plain() == Some("in") => {
                    header = 0;
                    patterns.push(true);
                }
                1 => return None,
                _ if word.plain() == Some("esac") => {
                    if closers.pop() != Some("esac") {
                        return None;
                    }
                    patterns.pop();
                }
                _ => {}
            }
            continue;
        }
        match tok {
            Tok::Word(word) => {
                substitution_bodies(word, &mut bodies);
                if current.is_none() {
                    match word.raw.as_str() {
                        "coproc" | "function" | "select" | "repeat" | "foreach" => return None,
                        "case" => {
                            level.push(closers.len());
                            closers.push("esac");
                            bound[at] = true;
                            header = 2;
                            continue;
                        }
                        "{" => {
                            level.push(closers.len());
                            closers.push("}");
                            continue;
                        }
                        closer @ ("}" | "fi" | "done" | "esac") => {
                            if closers.pop() != Some(closer) {
                                return None;
                            }
                            if closer == "esac" {
                                patterns.pop();
                            }
                            level.push(closers.len());
                            continue;
                        }
                        "if" => closers.push("fi"),
                        "while" | "until" | "for" => closers.push("done"),
                        _ => {}
                    }
                }
                level.push(closers.len());
                let unit = current.get_or_insert_with(|| Unit {
                    start: at,
                    end: at,
                    simple: Simple {
                        words: Vec::new(),
                        raw: String::new(),
                        piped: false,
                        redirects: Vec::new(),
                    },
                });
                if !unit.simple.raw.is_empty() {
                    unit.simple.raw.push(' ');
                }
                unit.simple.raw.push_str(&word.raw);
                unit.simple.words.push(spelt(word));
            }
            Tok::Redir {
                op,
                target,
                heredoc,
            } => {
                level.push(closers.len());
                if let Some(word) = target {
                    substitution_bodies(word, &mut bodies);
                }
                if let Some(doc) = heredoc.and_then(|index| docs.get(index)) {
                    substitution_bodies(&doc.body, &mut bodies);
                }
                let redirect = redirect(op, target.as_ref(), *heredoc, &docs)?;
                match current.as_mut() {
                    Some(unit) => unit.simple.redirects.push(redirect),
                    None => attached.push((at, redirect)),
                }
            }
            Tok::Op(op) => {
                finish(&mut current, at, &mut units);
                match *op {
                    "(" => {
                        level.push(closers.len());
                        closers.push(")");
                    }
                    ")" => {
                        if closers.pop() != Some(")") {
                            return None;
                        }
                        level.push(closers.len());
                    }
                    "|" | "|&" | ";" | "&&" | "||" | "&" | "\n" => level.push(closers.len()),
                    ";;" | ";&" | ";;&" | ";|" if closers.last() == Some(&"esac") => {
                        // The branch ends; the next pattern, or `esac`.
                        level.push(closers.len());
                        bound[at] = true;
                        if let Some(top) = patterns.last_mut() {
                            *top = true;
                        }
                    }
                    _ => return None,
                }
            }
            Tok::Array { name, elements, .. } => {
                for element in elements {
                    substitution_bodies(element, &mut bodies);
                }
                level.push(closers.len());
                let unit = current.get_or_insert_with(|| Unit {
                    start: at,
                    end: at,
                    simple: Simple {
                        words: Vec::new(),
                        raw: String::new(),
                        piped: false,
                        redirects: Vec::new(),
                    },
                });
                unit.simple.words.push(Spelt::Open(format!("{name}=()")));
            }
            Tok::ArithCmd(body) => {
                if body.contains("$(") || body.contains('`') {
                    return None;
                }
                finish(&mut current, at, &mut units);
                level.push(closers.len());
            }
        }
    }
    finish(&mut current, tokens.len(), &mut units);
    if !closers.is_empty() {
        return None;
    }
    // The span of the element of a pipeline or list around `at` at its own
    // level: back to `from` or forward from it.
    let element = |at: usize, back: bool| -> (usize, usize) {
        let own = level[at];
        let stops = |index: usize| {
            bound[index] || level[index] < own || (level[index] == own && separates(&tokens[index]))
        };
        if back {
            let mut start = at;
            while start > 0 && !stops(start - 1) {
                start -= 1;
            }
            (start, at)
        } else {
            let mut end = at + 1;
            while end < tokens.len() && !stops(end) {
                end += 1;
            }
            (at + 1, end)
        }
    };
    let within = |(start, end): (usize, usize)| {
        units
            .iter()
            .filter(move |unit| unit.start >= start && unit.end <= end)
    };
    let reads_pipe = |unit: &Unit| !unit.simple.redirects.iter().any(|r| r.input && r.fd == 0);
    // What the element `before` writes: one simple command's output, text
    // it spells when a command in it spells or a compound command in it
    // reads a here-string, a here-document or a process substitution, else
    // its programs' output.
    let text_of = |before: (usize, usize)| -> Fed {
        let producers: Vec<&Unit> = within(before).collect();
        let attached_spells = attached.iter().any(|(index, redirect)| {
            (before.0..before.1).contains(index)
                && redirect.input
                && fed_by(&redirect.source).is_some()
        });
        match producers.as_slice() {
            [unit] if unit.start == before.0 && unit.end == before.1 => {
                Fed::Command(unit.simple.clone())
            }
            units
                if attached_spells
                    || units.iter().any(|unit| spells_simple(&unit.simple, depth)) =>
            {
                Fed::Spelled
            }
            _ => Fed::Output,
        }
    };
    for (at, tok) in tokens.iter().enumerate() {
        if bound[at] || !matches!(tok, Tok::Op("|" | "|&")) {
            continue;
        }
        let fed = text_of(element(at, true));
        let after = element(at, false);
        let readers = within(after)
            .filter(|unit| reads_pipe(unit))
            .map(|unit| unit.simple.raw.clone())
            .collect();
        out.feeds.push(Feed { fed, readers });
    }
    for unit in &units {
        let simple = &unit.simple;
        let input = simple.redirects.iter().rfind(|r| r.input && r.fd == 0);
        if let Some(fed) = input.and_then(|redirect| fed_by(&redirect.source)) {
            out.feeds.push(Feed {
                fed,
                readers: vec![simple.raw.clone()],
            });
        }
        for word in simple.words.iter().skip(1) {
            if let Spelt::Process(body) = word {
                out.feeds.push(Feed {
                    fed: Fed::Body(body.clone()),
                    readers: vec![simple.raw.clone()],
                });
            }
        }
        out.commands.push(simple.clone());
    }
    for (at, redirect) in &attached {
        let element = element(*at, true);
        if !redirect.input {
            // An output process substitution reads what the compound
            // command writes.
            if let Source::Process(body) = &redirect.source {
                out.sinks.push(Feed {
                    fed: text_of(element),
                    readers: vec![body.clone()],
                });
            }
            continue;
        }
        // Every descriptor, not only standard input: a command inside may
        // read another one by its path or a copy.
        let Some(fed) = fed_by(&redirect.source) else {
            continue;
        };
        let readers = within(element)
            .map(|unit| unit.simple.raw.clone())
            .collect();
        out.feeds.push(Feed { fed, readers });
    }
    // Each write to a file, and whether what it writes is text the line
    // spells: what the command spells, a feed it reads, a `sed` script, or
    // code it runs that writes such text.
    for unit in &units {
        let simple = &unit.simple;
        if writes_file(simple) {
            out.writes_file = true;
            let reads_spelled = out
                .feeds
                .iter()
                .any(|feed| feed.readers.contains(&simple.raw) && feed_spells(&feed.fed, depth));
            if spells_with(simple, depth, word_spells)
                || reads_spelled
                || operand_writer(&simple.raw) == Some("sed")
            {
                out.spelled_write = true;
            }
        }
        if (spells_with(simple, depth, word_spells) && writes_through_sink(simple))
            || runs_writing_code(simple)
        {
            out.writes_file = true;
            out.spelled_write = true;
        }
    }
    // A compound command's output redirected to a file, or to an output
    // process substitution whose body writes one.
    for (at, redirect) in &attached {
        if redirect.input && !redirect.writes {
            continue;
        }
        let span = element(*at, true);
        let reads_spelled = within(span).any(|unit| {
            out.feeds.iter().any(|feed| {
                feed.readers.contains(&unit.simple.raw) && feed_spells(&feed.fed, depth)
            })
        });
        let spelled = reads_spelled
            || match text_of(span) {
                Fed::Output => false,
                Fed::Command(simple) => spells_with(&simple, depth, word_spells),
                _ => true,
            };
        if let Source::Process(body) = &redirect.source {
            if !redirect.input && spelled && body_writes(body) {
                out.writes_file = true;
                out.spelled_write = true;
            }
        } else if to_file(redirect) {
            out.writes_file = true;
            out.spelled_write |= spelled;
        }
    }
    for body in bodies {
        compound_into(&body, depth + 1, out)?;
    }
    Some(())
}

/// What an input redirection hands its command, where that is text the
/// line spells or a process: a file or a descriptor is not a feed here.
fn fed_by(source: &Source) -> Option<Fed> {
    match source {
        Source::Text(text) => Some(Fed::Text(text.clone())),
        Source::Spelled => Some(Fed::Spelled),
        Source::Process(body) => Some(Fed::Body(body.clone())),
        Source::Path(_) | Source::Copy | Source::Open => None,
    }
}

/// Whether `simple` writes text the line spells: its program is a printer,
/// `eval` or a shell given a `-c` value, it reads a here-string or
/// here-document, or a substitution in it does.
fn spells_simple(simple: &Simple, depth: usize) -> bool {
    spells_with(simple, depth, |raw| {
        raw.contains("$(") || raw.contains('`') || raw.contains("<(") || raw.contains(">(")
    })
}

/// [`spells_simple`], where `open` says whether a word the shell fills in
/// holds text the line spells.
fn spells_with(simple: &Simple, depth: usize, open: fn(&str) -> bool) -> bool {
    let program = program_of(simple);
    matches!(program.as_str(), "printf" | "echo" | "print")
        || runs_code(simple)
        || simple.redirects.iter().any(|redirect| {
            matches!(redirect.source, Source::Text(_) | Source::Spelled)
                || matches!(&redirect.source, Source::Process(body) if spells(body, depth + 1))
        })
        || simple.words.iter().any(|word| match word {
            // An output process substitution's body writes to the line's
            // output too.
            Spelt::Output(body) | Spelt::Process(body) | Spelt::Sink(body) => {
                spells(body, depth + 1)
            }
            Spelt::Open(raw) => open(raw),
            Spelt::Plain(_) => false,
        })
}

/// The last component of the program word of `simple`: empty when that
/// word is one the shell fills in or there is none.
fn program_of(simple: &Simple) -> String {
    let program = simple.words.iter().find_map(|word| match word {
        Spelt::Plain(text) if !LEADING_WORDS.contains(&text.as_str()) && !assignment(text) => {
            Some(text.rsplit('/').next().unwrap_or(text).to_string())
        }
        Spelt::Plain(_) => None,
        _ => Some(String::new()),
    });
    program.unwrap_or_default()
}

/// Whether `simple` runs code it is given as words: `eval`, or a shell
/// given a `-c` option.
fn runs_code(simple: &Simple) -> bool {
    let program = program_of(simple);
    program == "eval"
        || (crate::hooks::git_guard::is_shell(&program)
            && simple.words.iter().any(|word| matches!(word, Spelt::Plain(text) if text.starts_with('-') && text.contains('c'))))
}

/// Targets a redirection or a writing program can name that are not
/// files.
pub(crate) const NOT_FILES: &[&str] = &[
    "/dev/null",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/fd/1",
    "/dev/fd/2",
];

/// Whether `redirect` writes a file: an output redirection to a target
/// other than [`NOT_FILES`], a descriptor copy aside, or one the shell
/// fills in.
fn to_file(redirect: &Redirect) -> bool {
    redirect.writes
        && match &redirect.source {
            Source::Path(path) => !NOT_FILES.contains(&path.as_str()),
            Source::Open => true,
            _ => false,
        }
}

/// The program of the simple command `segment` when it writes a file named
/// by its words (TSK-1476): `tee` with a file operand, `dd` with `of=`, or
/// `sed` that may write (`-i`, or a script that may hold a `w` command).
pub(crate) fn operand_writer(segment: &str) -> Option<&'static str> {
    use crate::hooks::git_guard::{command_argv, program_name, strip_launchers};
    let mut words = command_argv(segment);
    crate::hooks::git_guard::strip_reserved_words(&mut words);
    let (program, args) = strip_launchers(&words)?;
    let file = |target: &str| !NOT_FILES.contains(&target);
    match program_name(program) {
        "tee" if args.iter().any(|arg| !arg.starts_with('-') && file(arg)) => Some("tee"),
        "dd" if args
            .iter()
            .any(|arg| arg.strip_prefix("of=").is_some_and(file)) =>
        {
            Some("dd")
        }
        "sed"
            if args.iter().any(|arg| {
                arg.starts_with("-i")
                    || arg.starts_with("--in-place")
                    || arg == "-I"
                    || (!arg.starts_with('-') && arg.contains(['w', 'W']))
            }) =>
        {
            Some("sed")
        }
        _ => None,
    }
}

/// Whether `simple` writes a file: by an output redirection ([`to_file`])
/// or by naming it to a writing program ([`operand_writer`]).
fn writes_file(simple: &Simple) -> bool {
    simple.redirects.iter().any(to_file) || operand_writer(&simple.raw).is_some()
}

/// Whether a feed hands over text the line spells.
fn feed_spells(fed: &Fed, depth: usize) -> bool {
    match fed {
        Fed::Command(simple) => spells_simple(simple, depth),
        Fed::Text(_) | Fed::Spelled => true,
        Fed::Body(body) => spells(body, depth + 1),
        Fed::Output => false,
    }
}

/// Whether the command line `body`, reading what another command writes,
/// may write a file; `true` when it cannot be read.
fn body_writes(body: &str) -> bool {
    compound(body).is_none_or(|found| found.writes_file)
}

/// Whether `simple` writes text it spells to a file through an output
/// process substitution whose body writes a file.
fn writes_through_sink(simple: &Simple) -> bool {
    simple.redirects.iter().any(|redirect| {
        !redirect.input && matches!(&redirect.source, Source::Process(body) if body_writes(body))
    }) || simple
        .words
        .iter()
        .any(|word| matches!(word, Spelt::Sink(body) if body_writes(body)))
}

/// Whether `simple` runs or carries a command that writes text it spells
/// to a file: a command the guard reads out of it (`eval`, a shell's `-c`
/// code, a launcher's or a supplier's command) that does, or `eval` or a
/// shell's `-c` code that this reading cannot follow.
fn runs_writing_code(simple: &Simple) -> bool {
    let read = crate::hooks::git_guard::expand_commands_read(&simple.raw);
    read.segments
        .iter()
        .any(|segment| *segment != simple.raw && writes_spelled_text(segment))
        || (runs_code(simple)
            && (!read.unread_commands.is_empty()
                || simple
                    .words
                    .iter()
                    .any(|word| !matches!(word, Spelt::Plain(_)))))
}

/// Whether a command or process substitution anywhere in `line` writes a
/// file; `true` when the line, or a substitution in it, cannot be read.
pub(crate) fn substitutions_write(line: &str) -> bool {
    let Ok((tokens, docs)) = lex(line) else {
        return true;
    };
    let mut words: Vec<&Word> = Vec::new();
    for tok in &tokens {
        match tok {
            Tok::Word(word) => words.push(word),
            Tok::Redir {
                target, heredoc, ..
            } => {
                words.extend(target.iter());
                words.extend(
                    heredoc
                        .and_then(|index| docs.get(index))
                        .map(|doc| &doc.body),
                );
            }
            Tok::Array { elements, .. } => words.extend(elements.iter()),
            Tok::ArithCmd(body) => {
                if body.contains("$(") || body.contains('`') {
                    return true;
                }
            }
            Tok::Op(_) => {}
        }
    }
    let mut bodies = Vec::new();
    for word in words {
        let nested = word.parts.iter().any(|part| match part {
            Part::Param { body, .. } | Part::Arith { body, .. } => {
                body.contains("$(") || body.contains('`')
            }
            Part::Unmodelled(_) => true,
            _ => false,
        });
        if nested {
            return true;
        }
        substitution_bodies(word, &mut bodies);
    }
    bodies.iter().any(|body| body_writes(body))
}

/// Whether the command line `line` writes text it spells to a file, in any
/// command, group, loop or substitution on it (TSK-1476): a command that
/// writes a file ([`to_file`], [`operand_writer`]) and writes text the
/// line spells ([`spells_with`] with [`word_spells`]) or reads it from a
/// feed, `sed` that may write a file (its script is text the line spells),
/// a command that writes text it spells to an output process substitution
/// whose body writes a file, `eval` or a shell's `-c` code that does any of
/// these, and a compound command whose output goes to a file while a
/// command in it writes text the line spells. `true` when the line cannot
/// be read.
pub(crate) fn writes_spelled_text(line: &str) -> bool {
    compound(line).is_none_or(|found| found.spelled_write)
}

/// Whether the shell word `raw` holds text the line spells that this
/// reading does not settle: a substitution whose command writes text it
/// spells ([`spells`]), or a `$'...'` quote with an escape. `true` when
/// `raw` is not one word this reading follows.
pub(crate) fn word_spells(raw: &str) -> bool {
    let Ok((tokens, _)) = lex(raw) else {
        return true;
    };
    let [Tok::Word(word)] = tokens.as_slice() else {
        return true;
    };
    word.parts.iter().any(|part| match part {
        Part::Subst { body, .. } | Part::ProcSubst(body, _) => spells(body, 1),
        Part::Ansi(body) => body.contains('\\'),
        // A substitution or quote nested in `${...}` or arithmetic, or a
        // construct the shells read differently.
        Part::Param { body, .. } | Part::Arith { body, .. } => {
            body.contains("$(") || body.contains('`') || body.contains("$'")
        }
        Part::Unmodelled(_) => true,
        Part::Lit { .. } => false,
    })
}

/// Whether the command line `text` writes text it spells anywhere
/// ([`spells`]).
pub(crate) fn line_spells(text: &str) -> bool {
    spells(text, 0)
}

/// Whether the command line `text` writes text it spells
/// ([`spells_simple`]) anywhere; `true` when it cannot be read.
fn spells(text: &str, depth: usize) -> bool {
    if depth > 8 {
        return true;
    }
    let mut found = Compound::default();
    if compound_into(text, depth, &mut found).is_none() {
        return true;
    }
    found
        .commands
        .iter()
        .any(|simple| spells_simple(simple, depth))
        || found
            .feeds
            .iter()
            .any(|feed| matches!(feed.fed, Fed::Spelled | Fed::Text(_)))
}

/// Push the body of each substitution in `word` to `bodies`.
fn substitution_bodies(word: &Word, bodies: &mut Vec<String>) {
    for part in &word.parts {
        if let Part::Subst { body, .. } | Part::ProcSubst(body, _) = part {
            bodies.push(body.clone());
        }
    }
}

/// Reserved words that open a simple command of a compound one.
const LEADING_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "time",
];

/// Whether `word` is a `NAME=value` assignment.
fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

// ---------------------------------------------------------------------------
// Text the shell evaluates as arithmetic (TSK-1476)
// ---------------------------------------------------------------------------

/// The reason given when text the shell evaluates as arithmetic cannot be
/// placed.
const UNPLACED: &str = "text the shell evaluates as arithmetic that the guard cannot place";

/// How a word the shell evaluates is read: as an arithmetic expression, or
/// as a variable name whose subscript is one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Expression,
    Name,
}

/// Builtins of bash and zsh that evaluate none of their words as
/// arithmetic, by the bash manual (SHELL BUILTIN COMMANDS) and
/// zshbuiltins(1). Code the line hands `eval`, `source` or `trap` is judged
/// by the rules on code. Every other builtin of the two shells and of the
/// modules zsh ships ([`EVALUATING`]) has each word read as arithmetic.
const NOT_EVALUATING: &[&str] = &[
    "echo", "cd", "chdir", "pushd", "popd", "dirs", "pwd", "true", "false", ":", "source", ".",
    "eval", "type", "which", "whence", "where", "hash", "rehash", "unhash", "jobs", "fg", "bg",
    "disown", "kill", "times", "history", "fc", "r", "help", "trap", "shopt", "setopt", "unsetopt",
    "emulate", "ulimit", "limit", "unlimit", "umask", "suspend", "bind", "bindkey", "complete",
    "compgen", "compopt", "compctl", "zle", "ttyctl", "logout", "bye",
];

/// Builtins that run code the line hands them as a string or a file, now
/// or later: what it assigns is not a word on the line.
const CODE_RUNNERS: &[&str] = &["eval", "source", ".", "trap", "fc", "r", "emulate", "sched"];

/// Variables the shell sets to text a command on the line gives it: the
/// last argument, a match, a line `read` takes, an option argument, the
/// command text, a function's or file's name, the directory stack, and
/// the tables of zsh's parameter and mapfile modules (bash(1) Shell
/// Variables, zshparam(1) PARAMETERS SET BY THE SHELL, zshmodules(1)). The
/// line controls their text without a word that assigns them. `PWD` and
/// `OLDPWD` hold an absolute path, whose leading `/` ends arithmetic with
/// an error before anything after it is read.
const SHELL_SET: &[&str] = &[
    "_",
    "REPLY",
    "reply",
    "MAPFILE",
    "BASH_REMATCH",
    "MATCH",
    "match",
    "OPTARG",
    "BASH_COMMAND",
    "BASH_ARGV",
    "BASH_ARGV0",
    "BASH_EXECUTION_STRING",
    "BASH_SOURCE",
    "BASH_ALIASES",
    "BASH_CMDS",
    "FUNCNAME",
    "COPROC",
    "COMP_LINE",
    "COMP_WORDS",
    "READLINE_LINE",
    "DIRSTACK",
    "dirstack",
    "argv",
    "ARGV0",
    "ZSH_ARGZERO",
    "ZSH_EXECUTION_STRING",
    "ZSH_SCRIPT",
    "zsh_scheduled_events",
    "funcstack",
    "functrace",
    "funcfiletrace",
    "funcsourcetrace",
    "options",
    "commands",
    "functions",
    "dis_functions",
    "functions_source",
    "dis_functions_source",
    "builtins",
    "dis_builtins",
    "reswords",
    "dis_reswords",
    "patchars",
    "dis_patchars",
    "aliases",
    "dis_aliases",
    "galiases",
    "dis_galiases",
    "saliases",
    "dis_saliases",
    "parameters",
    "modules",
    "history",
    "historywords",
    "jobdirs",
    "jobtexts",
    "jobstates",
    "nameddirs",
    "userdirs",
    "usergroups",
    "mapfile",
];

/// zsh's arrays tied to a scalar (zshparam(1)): each reads the other's
/// value.
const TIED: &[(&str, &str)] = &[
    ("path", "PATH"),
    ("fpath", "FPATH"),
    ("cdpath", "CDPATH"),
    ("manpath", "MANPATH"),
    ("mailpath", "MAILPATH"),
    ("module_path", "MODULE_PATH"),
    ("psvar", "PSVAR"),
    ("watch", "WATCH"),
    ("zsh_eval_context", "ZSH_EVAL_CONTEXT"),
];

/// Variables whose value the shell expands as a prompt (bash(1) PROMPTING,
/// zshmisc(1) EXPANSION OF PROMPT SEQUENCES): `PS4` before each traced
/// command, `PS3` for `select`, the others in an interactive shell; and
/// `PROMPT_COMMAND`, which bash runs as a command.
const PROMPTS: &[&str] = &[
    "PS0",
    "PS1",
    "PS2",
    "PS3",
    "PS4",
    "PROMPT",
    "PROMPT2",
    "PROMPT3",
    "PROMPT4",
    "RPROMPT",
    "RPROMPT2",
    "RPS1",
    "RPS2",
    "SPROMPT",
    "PROMPT_COMMAND",
];

/// Builtins that define a function, an alias or a math function, or load
/// or turn on builtins: on a line that runs one, any command may be one of
/// these, so every word is read as arithmetic.
const DEFINERS: &[&str] = &[
    "alias",
    "unalias",
    "enable",
    "disable",
    "zmodload",
    "autoload",
    "functions",
    "unfunction",
    "function",
];

/// Words in front of a command that run it as a builtin or change how it
/// runs.
const PRECOMMANDS: &[&str] = &[
    "builtin",
    "command",
    "noglob",
    "nocorrect",
    "-",
    "exec",
    "time",
];

/// Declaration builtins: an operand's subscript is arithmetic, and with
/// the integer or float attribute (or a name reference) its value is too.
const DECLARATION: &[&str] = &[
    "declare", "typeset", "local", "export", "readonly", "private", "nameref", "integer", "float",
];

/// Builtins whose operands name a variable, which may be an array element
/// with an arithmetic subscript.
const NAMING: &[&str] = &[
    "read",
    "mapfile",
    "readarray",
    "getopts",
    "unset",
    "vared",
    "zparseopts",
    "zstat",
    "strftime",
    "sysread",
    "sysopen",
    "zselect",
    "zsocket",
    "ztcp",
    "zpty",
    "zformat",
    "zregexparse",
    "pcre_match",
    "zgetattr",
    "zlistattr",
];

/// The other builtins of bash, zsh and the modules zsh ships: each word is
/// read as arithmetic (`shift`, `return`, `exit`, `break`, `continue`,
/// `let` and the rest).
const EVALUATING: &[&str] = &[
    "let",
    "shift",
    "return",
    "exit",
    "break",
    "continue",
    "wait",
    "set",
    "printf",
    "print",
    "test",
    "[",
    "[[",
    "caller",
    "getln",
    "pushln",
    "sched",
    "zcompile",
    "zprof",
    "zftp",
    "zstyle",
    "clone",
    "cap",
    "getcap",
    "setcap",
    "echotc",
    "echoti",
    "syswrite",
    "sysseek",
    "zsystem",
    "syserror",
    "zcurses",
    "ztie",
    "zuntie",
    "zgdbmpath",
    "zsetattr",
    "zdelattr",
    "pcre_compile",
    "pcre_study",
    "compadd",
    "compset",
    "compcall",
    "comparguments",
    "compdescribe",
    "compfiles",
    "compgroups",
    "compquote",
    "comptags",
    "comptry",
    "compvalues",
];

/// The `[[` operators whose operands are arithmetic expressions.
const ARITH_OPERATORS: &[&str] = &["-eq", "-ne", "-lt", "-le", "-gt", "-ge"];

/// What text the shell evaluates as arithmetic on a line holds.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // independent facts about one line
struct Evaluated {
    /// The command lines the shell runs from such text: for each command
    /// substitution in arithmetic, one that hands its output to a shell;
    /// for one in a prompt, its body.
    feeds: Vec<String>,
    /// Variables such text reads.
    refs: Vec<String>,
    /// The variables the first walk finds read in a context it places,
    /// before every word is read as arithmetic.
    certain: Vec<String>,
    /// The positions in [`Self::refs`] of variables read only because a
    /// program the shell fills in may be a builtin that evaluates them.
    guessed: Vec<usize>,
    /// For each word that assigns a variable literal text with no `$` or
    /// backquote, or arithmetic expansions (a `name=value` word, a loop
    /// word, an array element), the name and that literal text.
    written: Vec<(String, String)>,
    /// Variables the line assigns in any other way.
    unwritten: Vec<String>,
    /// The line may assign a variable in a way no word on it shows: code
    /// it hands `eval`, `source`, `trap` or `mapfile -C`, a definer other
    /// than a function the line spells, a loop over several names or the
    /// positional parameters, a program or a variable name the shell fills
    /// in.
    hidden: bool,
    /// The line defines a function, an alias or a builtin.
    defines: bool,
    /// The line gives a variable the integer, float or reference attribute.
    integer: bool,
    /// The line sets the positional parameters.
    positional: bool,
    /// Every word is read as arithmetic (set on the second walk when the
    /// line defines a command or an attribute).
    every: bool,
}

/// For each command substitution in text the shell evaluates as
/// arithmetic on `line`, a command line that hands its output to a shell
/// (`body | sh`): the shell runs the substitution and evaluates its output.
/// The contexts, from the bash manual (ARITHMETIC EVALUATION and the
/// sections it names) and zshmisc(1), zshparam(1), zshexpn(1) and
/// zshbuiltins(1): `((...))` and the arithmetic `for`, `$((...))` and
/// `$[...]`, `let`, the `[[` operators `-eq`, `-ne`, `-lt`, `-le`, `-gt`
/// and `-ge`, the operand of `-v` in `[[`, `test` and `[`, an array
/// subscript in an expansion, an assignment or a variable name a builtin
/// is given (`read`, `printf -v`, `unset` and the rest), a value assigned
/// with the integer or float attribute, a substring offset and length,
/// zsh's `repeat` count, the numeric arguments of zsh's `printf`, the
/// counts of zsh's `shift`, `return`, `exit`, `break` and `continue`, and
/// an expansion that reads a variable's value as a name or expands it
/// again (`${!name}`, zsh flags, bash `@P`). The `test` and `[` comparison
/// operators take integers in both shells and are not evaluated. Text the
/// shell expands as a prompt, a value assigned to a prompt variable
/// ([`PROMPTS`]) or an argument of zsh's `print -P`, has each substitution
/// run as a command line. `Err` when such text cannot be placed: a word
/// that joins written substitution text with an expansion, a variable it
/// reads that the line binds to text from outside the line or to text
/// with a substitution, that the line assigns in a way no word shows or
/// that the shell sets from the line's text, positional parameters the
/// line sets, or text the lexer cannot read ([`Evaluated::settle`]).
pub(crate) fn arithmetic_feeds(line: &str) -> Result<Vec<String>, &'static str> {
    let mut found = Evaluated::default();
    found.line(line, 0)?;
    found.certain = found
        .refs
        .iter()
        .enumerate()
        .filter(|(at, _)| !found.guessed.contains(at))
        .map(|(_, name)| name.clone())
        .collect();
    if found.defines || found.integer {
        // The second walk reads every word; what the first found about
        // the line's assignments stands.
        let first = found;
        found = Evaluated {
            every: true,
            ..Evaluated::default()
        };
        found.line(line, 0)?;
        found.defines |= first.defines;
        found.integer |= first.integer;
        found.hidden |= first.hidden;
        found.certain = first.certain;
        for fed in &first.feeds {
            found.runs(fed);
        }
        found.written.extend(first.written);
        found.unwritten.extend(first.unwritten);
    }
    found.settle(line)?;
    Ok(found.feeds)
}

impl Evaluated {
    fn line(&mut self, line: &str, depth: usize) -> Result<(), &'static str> {
        if depth > 8 {
            return Err(UNPLACED);
        }
        let Ok((toks, docs)) = lex(line) else {
            // With no expansion on it, nothing on the line can run text.
            return if line.contains('$') || line.contains('`') {
                Err(UNPLACED)
            } else {
                Ok(())
            };
        };
        // Commands end at an operator, except inside `[[ ... ]]`.
        let mut start = 0;
        let mut in_test = false;
        for at in 0..=toks.len() {
            match toks.get(at) {
                Some(Tok::Word(word)) => match word.plain() {
                    Some("[[") => in_test = true,
                    Some("]]") => in_test = false,
                    _ => {}
                },
                Some(Tok::Op(_)) if !in_test => {}
                Some(_) => continue,
                None => {}
            }
            if matches!(toks.get(at), Some(Tok::Op(_)) | None) {
                let segment = &toks[start..at];
                // `name()` starts a function definition.
                if segment.len() == 1
                    && matches!(toks.get(at), Some(Tok::Op("(")))
                    && matches!(toks.get(at + 1), Some(Tok::Op(")")))
                {
                    self.defines = true;
                }
                self.command(segment, &docs, depth)?;
                start = at + 1;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // one dispatch over the builtins
    fn command(
        &mut self,
        toks: &[Tok],
        docs: &[Heredoc],
        depth: usize,
    ) -> Result<(), &'static str> {
        let mut words: Vec<&Word> = Vec::new();
        for tok in toks {
            match tok {
                Tok::Word(word) => {
                    self.word(word, depth)?;
                    words.push(word);
                }
                Tok::Array { name, elements, .. } => {
                    self.record(name, Some(String::new()));
                    for element in elements {
                        self.record(name, settled_text(element, true));
                        self.word(element, depth)?;
                        if element.raw.starts_with('[') || self.every {
                            self.evaluated(element, Reading::Expression, depth)?;
                        }
                    }
                }
                Tok::Redir {
                    target, heredoc, ..
                } => {
                    if let Some(target) = target {
                        self.word(target, depth)?;
                    }
                    if let Some(doc) = heredoc.and_then(|index| docs.get(index)) {
                        self.word(&doc.body, depth)?;
                    }
                }
                Tok::ArithCmd(body) => self.text(body, depth)?,
                Tok::Op(_) => {}
            }
        }
        // A prompt variable's value, wherever a word assigns one: the
        // shell, or a shell the line starts, expands it as a prompt.
        for word in &words {
            if let Some(name) = assigned_name(&word.raw) {
                if PROMPTS.contains(&name) {
                    self.prompted(word, Some(name), depth)?;
                }
            }
        }
        let mut at = 0;
        // Reserved words, assignments and a `repeat` count.
        while let Some(word) = words.get(at) {
            let plain = word.plain().unwrap_or("");
            if LEADING_WORDS.contains(&plain) || matches!(plain, "{" | "}" | "coproc") {
                at += 1;
            } else if plain == "repeat" {
                if let Some(count) = words.get(at + 1) {
                    self.evaluated(count, Reading::Expression, depth)?;
                }
                at += 2;
            } else if let Some(name) = assigned_name(&word.raw) {
                self.assigns_word(name, word);
                if name.contains('[') || self.every {
                    self.evaluated(word, Reading::Expression, depth)?;
                }
                at += 1;
            } else {
                break;
            }
        }
        let rest = &words[at.min(words.len())..];
        if let [keyword, after @ ..] = rest {
            if matches!(keyword.plain(), Some("for" | "select" | "foreach")) {
                match after {
                    // `for name in words`: each word, as the loop gives it.
                    [name, word, values @ ..]
                        if keyword.plain() != Some("foreach") && word.plain() == Some("in") =>
                    {
                        self.record(&name.raw, Some(String::new()));
                        for value in values {
                            self.record(&name.raw, settled_text(value, true));
                        }
                    }
                    // The positional parameters, several names, or zsh's
                    // forms with the words in parentheses.
                    _ => self.hidden = true,
                }
            }
        }
        if self.every {
            return self.all(rest, Reading::Expression, depth);
        }
        let mut at = 0;
        loop {
            let Some(word) = rest.get(at) else {
                return Ok(());
            };
            let Some(name) = word.plain() else {
                // A program the shell fills in may be any builtin: its
                // words may be arithmetic, and it may assign a variable.
                self.hidden = true;
                let start = self.refs.len();
                self.all(rest, Reading::Expression, depth)?;
                self.guessed.extend(start..self.refs.len());
                return Ok(());
            };
            if !PRECOMMANDS.contains(&name) {
                break;
            }
            at += 1;
            while let Some(option) = rest.get(at).and_then(|word| word.plain()) {
                if !option.starts_with('-') || option == "-" || name == "builtin" {
                    break;
                }
                if name == "command" && (option.contains('v') || option.contains('V')) {
                    return Ok(());
                }
                at += if name == "exec" && option == "-a" {
                    2
                } else {
                    1
                };
            }
        }
        let name = rest[at].plain().unwrap_or("");
        let args = &rest[at + 1..];
        self.hidden |= CODE_RUNNERS.contains(&name);
        // `mapfile -C` runs a callback the line hands it as a string.
        self.hidden |= matches!(name, "mapfile" | "readarray")
            && args.iter().any(|word| {
                word.plain()
                    .is_some_and(|option| option.starts_with('-') && option.contains('C'))
            });
        match name {
            _ if DEFINERS.contains(&name) => {
                self.defines = true;
                // A function's body is on the line; an alias's text, a
                // loaded function or a builtin turned on is not.
                self.hidden |= name != "function";
            }
            _ if NOT_EVALUATING.contains(&name) => {}
            "printf" => self.printf(args, depth)?,
            "print" => self.print(args, depth)?,
            "[[" => {
                for (index, word) in args.iter().enumerate() {
                    let operator = word.plain().unwrap_or("");
                    if ARITH_OPERATORS.contains(&operator) {
                        if let Some(before) = index.checked_sub(1).and_then(|i| args.get(i)) {
                            self.evaluated(before, Reading::Expression, depth)?;
                        }
                    }
                    if ARITH_OPERATORS.contains(&operator) || operator == "-v" {
                        if let Some(after) = args.get(index + 1) {
                            let reading = if operator == "-v" {
                                Reading::Name
                            } else {
                                Reading::Expression
                            };
                            self.evaluated(after, reading, depth)?;
                        }
                    }
                }
            }
            "test" | "[" => self.after_option(args, "-v", depth)?,
            "wait" => self.after_option(args, "-p", depth)?,
            "set" => {
                for (index, word) in args.iter().enumerate() {
                    match word.plain() {
                        Some("-A" | "+A") => {
                            if let Some(name) = args.get(index + 1) {
                                self.evaluated(name, Reading::Name, depth)?;
                            }
                        }
                        Some(option) if option.starts_with(['-', '+']) && option != "--" => {}
                        _ => self.positional = true,
                    }
                }
            }
            _ if DECLARATION.contains(&name) => {
                let mut attribute = matches!(name, "integer" | "float" | "nameref");
                for word in args {
                    match word.plain() {
                        Some(option) if option.starts_with(['-', '+']) && option.len() > 1 => {
                            // zsh's `-T` ties a scalar to an array.
                            attribute |= option.contains(['i', 'E', 'F', 'n', 'T']);
                        }
                        _ if attribute => {
                            if let Some(name) = assigned_name(&word.raw) {
                                self.assigns_word(name, word);
                            }
                            self.evaluated(word, Reading::Expression, depth)?;
                        }
                        // Without the attribute only a subscript is
                        // arithmetic; a value is not.
                        _ => match assigned_name(&word.raw) {
                            Some(name) => {
                                self.assigns_word(name, word);
                                if name.contains('[') {
                                    self.evaluated(word, Reading::Expression, depth)?;
                                }
                            }
                            // A bare name keeps its value or is emptied.
                            None if word.plain().is_some_and(is_identifier) => {
                                self.record(&word.raw, Some(String::new()));
                            }
                            None => self.evaluated(word, Reading::Name, depth)?,
                        },
                    }
                }
                self.integer |= attribute;
            }
            _ if NAMING.contains(&name) => self.all(args, Reading::Name, depth)?,
            _ if EVALUATING.contains(&name) => self.all(args, Reading::Expression, depth)?,
            _ => {}
        }
        Ok(())
    }

    fn all(&mut self, words: &[&Word], reading: Reading, depth: usize) -> Result<(), &'static str> {
        for word in words {
            self.evaluated(word, reading, depth)?;
        }
        Ok(())
    }

    /// The word after `option` (`test -v`, `wait -p`) names a variable.
    fn after_option(
        &mut self,
        args: &[&Word],
        option: &str,
        depth: usize,
    ) -> Result<(), &'static str> {
        for (index, word) in args.iter().enumerate() {
            if word.plain() == Some(option) {
                if let Some(name) = args.get(index + 1) {
                    self.evaluated(name, Reading::Name, depth)?;
                }
            }
        }
        Ok(())
    }

    /// `printf`: the `-v` name, and in zsh every argument after a format
    /// with a numeric conversion, or after a format the shell fills in.
    fn printf(&mut self, args: &[&Word], depth: usize) -> Result<(), &'static str> {
        let mut at = 0;
        while let Some(word) = args.get(at) {
            match word.plain() {
                Some("--") => {
                    at += 1;
                    break;
                }
                Some("-v") => {
                    if let Some(name) = args.get(at + 1) {
                        self.evaluated(name, Reading::Name, depth)?;
                    }
                    at += 2;
                }
                Some(option) if option.starts_with("-v") => {
                    self.evaluated(word, Reading::Name, depth)?;
                    at += 1;
                }
                _ => break,
            }
        }
        self.formatted(
            args.get(at).copied(),
            &args[(at + 1).min(args.len())..],
            depth,
        )
    }

    /// The arguments of a zsh `printf` format: arithmetic when the format
    /// has a numeric conversion or is not written out.
    fn formatted(
        &mut self,
        format: Option<&Word>,
        args: &[&Word],
        depth: usize,
    ) -> Result<(), &'static str> {
        let numeric = format
            .is_none_or(|format| literal_text(format).is_none_or(|text| numeric_conversion(&text)));
        if numeric {
            self.all(args, Reading::Expression, depth)?;
        }
        Ok(())
    }

    /// zsh `print`: the `-v` name, every argument with `-P` (prompt
    /// expansion, which runs substitutions with `PROMPT_SUBST`), the
    /// numeric values of `-C`, `-u`, `-x` and `-X`, and `-f` as `printf`.
    fn print(&mut self, args: &[&Word], depth: usize) -> Result<(), &'static str> {
        let mut at = 0;
        let mut prompt = false;
        let mut format: Option<&Word> = None;
        while let Some(word) = args.get(at) {
            let Some(option) = word.plain() else {
                break;
            };
            if option == "--" || option == "-" {
                at += 1;
                break;
            }
            if !option.starts_with(['-', '+']) || option.len() < 2 {
                break;
            }
            at += 1;
            for (index, flag) in option.char_indices().skip(1) {
                if flag == 'P' {
                    prompt = true;
                }
                if matches!(flag, 'f' | 'v' | 'C' | 'u' | 'x' | 'X') {
                    // The value is the rest of the word or the next word.
                    let value = if index + 1 < option.len() {
                        Some(*word)
                    } else {
                        at += 1;
                        args.get(at - 1).copied()
                    };
                    if let Some(value) = value {
                        if flag == 'f' {
                            format = Some(value);
                        } else {
                            let reading = if flag == 'v' {
                                Reading::Name
                            } else {
                                Reading::Expression
                            };
                            self.evaluated(value, reading, depth)?;
                        }
                    }
                    break;
                }
            }
        }
        let args = &args[at.min(args.len())..];
        if prompt {
            for word in args {
                self.prompted(word, None, depth)?;
            }
        } else if let Some(format) = format {
            self.formatted(Some(format), args, depth)?;
        }
        Ok(())
    }

    /// Any word on the line: each `$((...))`, a `${...}` subscript, offset
    /// or flag, and each substitution's body as a command line.
    fn word(&mut self, word: &Word, depth: usize) -> Result<(), &'static str> {
        let mut subscripted = false;
        for part in &word.parts {
            match part {
                Part::Arith { body, .. } => self.text(body, depth)?,
                Part::Param {
                    body, braced: true, ..
                } => self.braced(body, false, depth)?,
                Part::Param { zsh_tail, .. } => subscripted |= *zsh_tail,
                Part::Subst { body, .. } | Part::ProcSubst(body, _) => {
                    self.line(body, depth + 1)?;
                }
                Part::Lit { .. } | Part::Ansi(_) | Part::Unmodelled(_) => {}
            }
        }
        // zsh reads `$name[...]` as a subscript.
        if subscripted {
            self.evaluated(word, Reading::Expression, depth)?;
        }
        Ok(())
    }

    /// A word the shell evaluates: its written text is read as arithmetic
    /// (all of it, or a name's subscript), a substitution's output is fed to
    /// a shell, and a variable it expands is read.
    fn evaluated(
        &mut self,
        word: &Word,
        reading: Reading,
        depth: usize,
    ) -> Result<(), &'static str> {
        let mut written = String::new();
        let mut expands = false;
        for part in &word.parts {
            match part {
                Part::Lit { text, .. } => written.push_str(text),
                Part::Ansi(body) => written.push_str(&decode(body, Escapes::Ansi)?),
                Part::Param { body, braced, .. } => {
                    expands = true;
                    if *braced {
                        self.braced(body, true, depth)?;
                    } else {
                        self.refs.push(body.clone());
                    }
                }
                Part::Subst { body, .. } => {
                    expands = true;
                    self.feed(body);
                }
                Part::Arith { .. } | Part::ProcSubst(..) => expands = true,
                Part::Unmodelled(reason) => return Err(reason),
            }
        }
        if expands && (written.contains('$') || written.contains('`')) {
            return Err(UNPLACED);
        }
        match reading {
            Reading::Expression => self.text(&written, depth),
            Reading::Name => {
                // A name the shell fills in may be any variable.
                self.hidden |= expands;
                match (written.find('['), written.rfind(']')) {
                    _ if !expands && is_identifier(&written) => {
                        self.assigns(&written);
                        Ok(())
                    }
                    (Some(open), Some(close)) if open < close => {
                        self.assigns(&written[..open]);
                        self.text(&written[open + 1..close], depth)
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    /// Text the shell evaluates as arithmetic, as written: the variables it
    /// names, and inside it, read as between double quotes (a single quote
    /// is a character there), each substitution, expansion and nested
    /// arithmetic.
    fn text(&mut self, text: &str, depth: usize) -> Result<(), &'static str> {
        self.refs.extend(identifiers(text).map(str::to_string));
        if !text.contains('$') && !text.contains('`') {
            return Ok(());
        }
        let mut lexer = Lexer {
            chars: text.chars().collect(),
            at: 0,
            toks: Vec::new(),
            heredocs: Vec::new(),
            pending: Vec::new(),
        };
        let mut parts = Vec::new();
        lexer.double_quoted(&mut parts, true)?;
        for part in parts {
            match part {
                Part::Subst { body, .. } => {
                    self.feed(&body);
                    self.line(&body, depth + 1)?;
                }
                Part::Param {
                    body, braced: true, ..
                } => self.braced(&body, true, depth)?,
                Part::Param { body, .. } => self.refs.push(body),
                Part::Arith { body, .. } => self.text(&body, depth + 1)?,
                Part::Unmodelled(reason) => return Err(reason),
                Part::Lit { .. } | Part::Ansi(_) | Part::ProcSubst(..) => {}
            }
        }
        Ok(())
    }

    /// The body of `${...}`: a subscript, an offset and length, an
    /// indirection, zsh flags or bash `@P` are evaluated; with `value`, the
    /// variable's value is too (the expansion is itself in such text).
    fn braced(&mut self, body: &str, value: bool, depth: usize) -> Result<(), &'static str> {
        let mut rest = body;
        let mut reads = value;
        if let Some(flags) = rest.strip_prefix('(') {
            let Some(close) = flags.find(')') else {
                return Err(UNPLACED);
            };
            reads = true;
            rest = &flags[close + 1..];
        }
        if let Some(name) = rest.strip_prefix('!') {
            reads = true;
            rest = name;
        } else if rest.len() > 1 {
            if let Some(name) = rest.strip_prefix('#') {
                // A length is a number: the value is not read further.
                reads = reads && body.starts_with('(');
                rest = name;
            }
        }
        let name_len = if rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len())
        } else if rest.starts_with(|c: char| c.is_ascii_digit()) {
            rest.find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len())
        } else {
            rest.chars().next().map_or(0, char::len_utf8)
        };
        let name = &rest[..name_len];
        rest = &rest[name_len..];
        if let Some(inner) = rest.strip_prefix('[') {
            let mut level = 1;
            let close = inner.char_indices().find_map(|(index, c)| {
                match c {
                    '[' => level += 1,
                    ']' => level -= 1,
                    _ => {}
                }
                (level == 0).then_some(index)
            });
            let Some(close) = close else {
                return Err(UNPLACED);
            };
            self.text(&inner[..close], depth)?;
            rest = &inner[close + 1..];
        }
        if rest.starts_with("@P") {
            reads = true;
        }
        if reads {
            self.refs.push(name.to_string());
        }
        // `${name=word}`, `${name:=word}` and zsh's `${name::=word}`
        // assign the variable.
        if rest.trim_start_matches(':').starts_with('=') {
            self.assigns(name);
        }
        match rest.strip_prefix(':') {
            Some(offset) if !offset.starts_with(['-', '=', '?', '+']) => self.text(offset, depth),
            _ if rest.contains('$') || rest.contains('`') => {
                // An operator's word: what is nested in it.
                let mut lexer = Lexer {
                    chars: rest.chars().collect(),
                    at: 0,
                    toks: Vec::new(),
                    heredocs: Vec::new(),
                    pending: Vec::new(),
                };
                let mut parts = Vec::new();
                lexer.double_quoted(&mut parts, true)?;
                self.word(
                    &Word {
                        parts,
                        raw: rest.to_string(),
                    },
                    depth,
                )
            }
            _ => Ok(()),
        }
    }

    /// Record that the line assigns the variable `name` (a subscript
    /// dropped) a value no word on it shows.
    fn assigns(&mut self, name: &str) {
        self.record(name, None);
    }

    /// Record that the `name=value` word `word` assigns `name`: a value of
    /// literal text with no `$` or backquote, and of arithmetic expansions,
    /// which expand to numbers, is recorded with its literal text.
    fn assigns_word(&mut self, name: &str, word: &Word) {
        // A prompt variable's value is judged where it is assigned
        // ([`Self::prompted`]).
        if PROMPTS.contains(&name) {
            self.record(name, Some(String::new()));
            return;
        }
        let value = settled_text(word, false)
            .and_then(|text| text.split_once('=').map(|(_, value)| value.to_string()));
        self.record(name, value);
    }

    fn record(&mut self, name: &str, value: Option<String>) {
        let base = name.split_once('[').map_or(name, |(base, _)| base);
        if !is_identifier(base) {
            return;
        }
        match value {
            Some(value) => self.written.push((base.to_string(), value)),
            None => self.unwritten.push(base.to_string()),
        }
    }

    fn feed(&mut self, body: &str) {
        let fed = if body.contains([';', '&', '\n']) || body.contains("||") {
            format!("{{ {body}\n}} | sh")
        } else {
            format!("{body} | sh")
        };
        self.runs(&fed);
    }

    /// A command line the shell runs from such text.
    fn runs(&mut self, line: &str) {
        if !self.feeds.iter().any(|known| known == line) {
            self.feeds.push(line.to_string());
        }
    }

    /// A word the shell expands as a prompt: a value assigned to a prompt
    /// variable (`assigned` names it) or an argument of zsh's `print -P`.
    /// A variable it expands is read, a substitution's output is expanded
    /// again, and its written text is read as the shell expands a prompt
    /// ([`Self::expanded`]). `PROMPT_COMMAND`'s value is run as a command
    /// line.
    fn prompted(
        &mut self,
        word: &Word,
        assigned: Option<&str>,
        depth: usize,
    ) -> Result<(), &'static str> {
        let mut written = String::new();
        let mut expands = false;
        for part in &word.parts {
            match part {
                Part::Lit { text, .. } => written.push_str(text),
                Part::Ansi(body) => written.push_str(&decode(body, Escapes::Ansi)?),
                Part::Param { body, braced, .. } => {
                    expands = true;
                    if *braced {
                        self.braced(body, true, depth)?;
                    } else {
                        self.refs.push(body.clone());
                    }
                }
                Part::Subst { body, .. } => {
                    expands = true;
                    self.feed(body);
                }
                Part::Arith { .. } | Part::ProcSubst(..) => expands = true,
                Part::Unmodelled(reason) => return Err(reason),
            }
        }
        if expands && (written.contains('$') || written.contains('`')) {
            return Err(UNPLACED);
        }
        let value = match assigned {
            Some(_) => written.split_once('=').map_or("", |(_, value)| value),
            None => written.as_str(),
        };
        if assigned == Some("PROMPT_COMMAND") {
            if expands {
                return Err(UNPLACED);
            }
            self.runs(value);
            return self.line(value, depth + 1);
        }
        self.expanded(value, depth)
    }

    /// Text the shell expands once, as a prompt or between double quotes:
    /// each substitution's body is run as a command line, each arithmetic
    /// expansion is evaluated and each `${...}` subscript and offset.
    fn expanded(&mut self, text: &str, depth: usize) -> Result<(), &'static str> {
        if !text.contains('$') && !text.contains('`') {
            return Ok(());
        }
        let mut lexer = Lexer {
            chars: text.chars().collect(),
            at: 0,
            toks: Vec::new(),
            heredocs: Vec::new(),
            pending: Vec::new(),
        };
        let mut parts = Vec::new();
        lexer.double_quoted(&mut parts, true)?;
        for part in parts {
            match part {
                Part::Subst { body, .. } => {
                    self.runs(&body);
                    self.line(&body, depth + 1)?;
                }
                Part::Param {
                    body, braced: true, ..
                } => self.braced(&body, false, depth)?,
                Part::Arith { body, .. } => self.text(&body, depth + 1)?,
                Part::Unmodelled(reason) => return Err(reason),
                Part::Param { .. } | Part::Lit { .. } | Part::Ansi(_) | Part::ProcSubst(..) => {}
            }
        }
        Ok(())
    }

    /// Every variable such text reads, and every variable those values
    /// name, must hold a number, text the line writes with no substitution
    /// in it, or what it held before the line. Refused: a variable the
    /// shell sets from text on the line ([`SHELL_SET`]), a variable the
    /// line assigns other than by a word whose value this walk records
    /// ([`Self::unwritten`]), a variable read where the line may assign
    /// one unseen ([`Self::hidden`]), a prompt variable the line assigns
    /// unseen, and a value the composer resolves to outside or substituted
    /// text. Where the composer cannot resolve a value, the words this
    /// walk records settle it.
    fn settle(&self, line: &str) -> Result<(), &'static str> {
        let bound = bindings(line);
        if PROMPTS.iter().any(|name| {
            self.unwritten.iter().any(|known| known == name) || (self.hidden && line.contains(name))
        }) {
            return Err(UNPLACED);
        }
        if self.hidden && self.certain.iter().any(|name| !name.is_empty()) {
            return Err(UNPLACED);
        }
        let mut pending: Vec<String> = self.refs.clone();
        if self.integer {
            pending.extend(bound.iter().map(|(name, _)| name.clone()));
        }
        let mut seen: Vec<String> = Vec::new();
        while let Some(name) = pending.pop() {
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            if name.is_empty() {
                continue;
            }
            if name.chars().all(|c| c.is_ascii_digit()) || matches!(name.as_str(), "@" | "*") {
                if self.defines || self.positional || self.hidden {
                    return Err(UNPLACED);
                }
                continue;
            }
            if SHELL_SET.contains(&name.as_str()) || self.unwritten.contains(&name) {
                return Err(UNPLACED);
            }
            for (array, scalar) in TIED {
                if name == *array {
                    pending.push((*scalar).to_string());
                } else if name == *scalar {
                    pending.push((*array).to_string());
                }
            }
            for (_, value) in self.written.iter().filter(|(known, _)| *known == name) {
                pending.extend(identifiers(value).map(str::to_string));
            }
            for (_, value) in bound.iter().filter(|(bound, _)| *bound == name) {
                let value = match value {
                    Composed::Resolved(value) => value,
                    // Each assignment is a word this walk records.
                    Composed::Unresolved(_) if !self.hidden => continue,
                    Composed::Unresolved(_) => return Err(UNPLACED),
                };
                let text = value.render();
                if value.has_open() || text.contains('$') || text.contains('`') {
                    return Err(UNPLACED);
                }
                pending.extend(identifiers(&text).map(str::to_string));
            }
        }
        Ok(())
    }
}

/// The literal text of `word` when it holds only literal text with no `$`
/// or backquote and arithmetic expansions, which expand to numbers. With
/// `fields` (a loop word or an array element), an unquoted glob or brace
/// character, which may make other words, also gives `None`.
fn settled_text(word: &Word, fields: bool) -> Option<String> {
    let mut literal = String::new();
    for part in &word.parts {
        match part {
            Part::Lit { text, quoted } => {
                if text.contains(['$', '`'])
                    || (fields && !quoted && text.contains(['*', '?', '[', '{']))
                {
                    return None;
                }
                literal.push_str(text);
            }
            Part::Arith { .. } => {}
            _ => return None,
        }
    }
    Some(literal)
}

/// The variable name a `name=`, `name+=` or `name[...]=` word assigns, as
/// written before the `=`.
fn assigned_name(raw: &str) -> Option<&str> {
    let (name, _) = raw.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let base = name.split_once('[').map_or(name, |(base, _)| base);
    (is_identifier(base) && (base == name || name.ends_with(']'))).then_some(name)
}

/// Whether a `printf` format has a conversion that reads a number.
fn numeric_conversion(format: &str) -> bool {
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        for spec in chars.by_ref() {
            match spec {
                '%' | 's' | 'b' | 'q' => break,
                '-' | '+' | ' ' | '#' | '0'..='9' | '.' | '\'' => {}
                // A `*` width reads a number too.
                _ => return true,
            }
        }
    }
    false
}

/// The commands on `line`, at every depth, that take a here-document the
/// shell expands (an unquoted delimiter and a body with an expansion), as
/// their words are written; `None` when the line, or text nested in it, is
/// outside the shell grammar the composer reads (TSK-1476).
pub(crate) fn expanding_heredoc_readers(line: &str) -> Option<Vec<String>> {
    let mut found = Vec::new();
    heredoc_readers(line, 0, &mut found)?;
    Some(found)
}

fn heredoc_readers(line: &str, depth: usize, found: &mut Vec<String>) -> Option<()> {
    if depth > 8 {
        return None;
    }
    let (tokens, docs) = lex(line).ok()?;
    let mut nested = Vec::new();
    let mut substitutions = |word: &Word| {
        for part in &word.parts {
            if let Part::Subst { body, .. } | Part::ProcSubst(body, _) = part {
                nested.push(body.clone());
            }
        }
    };
    let mut words = Vec::new();
    let mut expanding = false;
    for (at, tok) in tokens.iter().enumerate() {
        match tok {
            Tok::Word(word) => {
                substitutions(word);
                words.push(word.raw.clone());
            }
            Tok::Redir {
                heredoc: Some(index),
                ..
            } => {
                let doc = docs.get(*index)?;
                substitutions(&doc.body);
                expanding |= !doc.quoted && body_text(&doc.body).is_none();
            }
            Tok::Redir {
                target: Some(word), ..
            } => substitutions(word),
            _ => {}
        }
        let ends = matches!(tok, Tok::Op(_)) || at + 1 == tokens.len();
        if ends {
            if std::mem::take(&mut expanding) {
                found.push(words.join(" "));
            }
            words.clear();
        }
    }
    for body in nested {
        heredoc_readers(&body, depth + 1, found)?;
    }
    Some(())
}

/// The text of `word` when the shell passes it exactly as written: only
/// literal parts, and no pattern, brace, tilde or leading `=` outside
/// quotes.
fn literal_text(word: &Word) -> Option<String> {
    let mut text = String::new();
    for (at, part) in word.parts.iter().enumerate() {
        let Part::Lit { text: lit, quoted } = part else {
            return None;
        };
        let expands =
            lit.contains(['*', '?', '[', ']', '{', '}', '~']) || (at == 0 && lit.starts_with('='));
        if !quoted && expands {
            return None;
        }
        text.push_str(lit);
    }
    Some(text)
}

/// The text of a here-document body with no expansion in it.
fn body_text(body: &Word) -> Option<String> {
    body.parts
        .iter()
        .map(|part| match part {
            Part::Lit { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Where a simple command takes its standard input from, by its last
/// input redirection (TSK-1476).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    /// No input redirection: a pipe, or what the line itself reads.
    Inherited,
    /// A file named by plain literal text.
    File,
    /// Text written on the line: a literal here-string, or a here-document
    /// whose body this text does not hold or the shell does not expand.
    Literal,
    /// Anything else: a process substitution, a descriptor, a target the
    /// shell expands, or a command the composer cannot read.
    Produced,
}

/// Where `segment`, one simple command, takes its standard input from.
pub(crate) fn input_of(segment: &str) -> Input {
    let Ok((tokens, docs)) = lex(segment) else {
        return Input::Produced;
    };
    let plain =
        |word: &Word| literal_text(word).is_some() && !crate::hooks::git_guard::expands(&word.raw);
    let mut input = Input::Inherited;
    for tok in &tokens {
        let Tok::Redir {
            op,
            target,
            heredoc,
        } = tok
        else {
            continue;
        };
        let digits = op.bytes().take_while(u8::is_ascii_digit).count();
        let (fd, op) = op.split_at(digits);
        if !fd.bytes().all(|byte| byte == b'0') || !op.starts_with('<') {
            continue;
        }
        input = match (op, target, heredoc) {
            (_, _, Some(index)) => match docs.get(*index) {
                Some(doc) if doc.quoted || body_text(&doc.body).is_some() => Input::Literal,
                _ => Input::Produced,
            },
            ("<<<", Some(word), _) if plain(word) => Input::Literal,
            ("<" | "<>", Some(word), _) if plain(word) => Input::File,
            _ => Input::Produced,
        };
    }
    input
}

/// What the last top-level `printf`, `echo` or `print` on the line writes
/// in `dialect`, or `None` when the line has none the dialect models. For
/// the differential test.
#[doc(hidden)]
#[must_use]
pub fn final_output(line: &str, dialect: Dialect) -> Option<Composed> {
    texts(line, dialect)
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
    texts(line, dialect).into_iter().map(|e| e.value).collect()
}

/// Every value the line binds to a variable, read as bash and as zsh read
/// it: a standalone or prefix assignment, a declaration operand, and every
/// other way the line stores a value (`printf -v`, `read`, a `for`
/// variable), with what each holds. A value the guard cannot resolve is
/// [`Composed::Unresolved`]; text from outside the line is an open piece
/// (TSK-1002 review 12: a program value is judged by what it holds).
pub(crate) fn bindings(line: &str) -> Vec<(String, Composed)> {
    let mut found = Vec::new();
    for dialect in [Dialect::Bash, Dialect::Zsh] {
        for event in events(line, dialect) {
            if let Some(name) = event.binding {
                let bound = (name, event.value);
                if !found.contains(&bound) {
                    found.push(bound);
                }
            }
        }
    }
    found
}

/// The text events, without the bindings [`bindings`] reports.
fn texts(line: &str, dialect: Dialect) -> Vec<Event> {
    events(line, dialect)
        .into_iter()
        .filter(|event| event.binding.is_none())
        .collect()
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
    /// Process substitution: a path the shell makes up, with its body and
    /// whether it is an output one (`>(...)`), whose body reads what the
    /// command writes there.
    ProcSubst(String, bool),
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
                sign @ ('<' | '>' | '=') if self.peek_at(1) == Some('(') => {
                    self.at += 2;
                    let body = self.balanced(1)?;
                    let mut word = Word::default();
                    word.parts.push(Part::ProcSubst(body, sign == '>'));
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
        if self.starts("<(") || self.starts(">(") {
            let output = self.starts(">(");
            self.at += 2;
            let body = self.balanced(1)?;
            let mut word = Word::default();
            word.parts.push(Part::ProcSubst(body, output));
            self.toks.push(Tok::Redir {
                op,
                target: Some(word),
                heredoc: None,
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
    let globs = chars.iter().enumerate().any(|(at, c)| {
        matches!(c, '*' | '?' | '^' | '#') || (*c == '[' && chars[at + 1..].contains(&']'))
    });
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Var {
    Scalar(Value),
    Array(Vec<Value>),
    /// A `for` variable: one of these values on each pass.
    Choices(Vec<Value>),
    Unknown(&'static str),
}

/// The positional parameters: `$0`, `$1` and on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// The variable a binding event assigns; `None` for a text event.
    binding: Option<String>,
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
pub(crate) const LAUNCHERS: &[&str] = &[
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
pub(crate) const SUPPLIERS: &[&str] = &["xargs", "find", "parallel", "watch"];

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
            binding: None,
        });
    }

    /// Report what `name` now holds, for [`bindings`].
    fn bind(&mut self, name: &str, var: &Var) {
        let values = match var {
            Var::Scalar(value) => vec![Composed::Resolved(value.clone())],
            Var::Array(values) | Var::Choices(values) if !values.is_empty() => values
                .iter()
                .map(|value| Composed::Resolved(value.clone()))
                .collect(),
            Var::Array(_) | Var::Choices(_) => vec![Composed::Resolved(Value::default())],
            Var::Unknown(reason) => vec![Composed::Unresolved(reason)],
        };
        for value in values {
            self.events.push(Event {
                depth: self.depth + usize::from(self.uncertain > 0),
                output: false,
                exact: true,
                value,
                binding: Some(name.to_string()),
            });
        }
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
        self.bind(name, &var);
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
                Part::Subst { body, .. } | Part::ProcSubst(body, _) => {
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
        // Builtin output emulation must apply the same leading tilde as the
        // operand reader. Keep the home prefix quoted and the tail's glob
        // syntax active. Unknown/named homes retain the unresolved reading.
        if let Some(tail) = word
            .plain()
            .and_then(|raw| raw.strip_prefix('~'))
            .filter(|tail| tail.is_empty() || tail.starts_with('/'))
        {
            if let Some(Var::Scalar(home)) = self.state.vars.get("HOME") {
                if let Some(home) = home.exact() {
                    let expanded = Word {
                        raw: word.raw.clone(),
                        parts: vec![
                            Part::Lit {
                                text: home,
                                quoted: true,
                            },
                            Part::Lit {
                                text: tail.into(),
                                quoted: false,
                            },
                        ],
                    };
                    return self.word(&expanded, ctx);
                }
            }
        }
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
                    // zsh reads `^` and `#` as glob operators too, as
                    // `has_glob` does for a plain word (TSK-1002 security
                    // review 03, H-A: an unquoted `$HOME/.codex/^`).
                    if fields_ctx && !*quoted && text.contains(['*', '?', '[', '^', '#']) {
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
                Part::ProcSubst(..) => (Ok(Value::open()), false),
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
        let mut value = match self.capture_stdout(body) {
            Ok(value) => value,
            Err(reason) => return (Err(reason), true),
        };
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

    /// Literal stdout bytes for pipeline and process-substitution consumers.
    /// Unlike command substitution, these retain trailing newlines.
    fn capture_stdout(&self, body: &str) -> Result<Value, &'static str> {
        if self.nesting >= 8 {
            return Err("nesting deeper than the guard reads");
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
            value.append(&record?);
        }
        Ok(value)
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
/// Whether a `pwd` prints the physical directory (`-P`) or the logical one
/// (no option, `-L`); the last of the two wins. `None` for any other word.
fn pwd_physical(args: &[Arg]) -> Option<bool> {
    let mut physical = false;
    for arg in args {
        let word = arg.literal()?;
        let letters = word
            .strip_prefix('-')
            .filter(|letters| !letters.is_empty())?;
        for letter in letters.chars() {
            match letter {
                'P' => physical = true,
                'L' => physical = false,
                _ => return None,
            }
        }
    }
    Some(physical)
}

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
                self.bind(&name, &var);
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
                name if name == "!" || crate::hooks::git_guard::is_shell_modifier(name) => at += 1,
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
            "pwd" if self.state.directory.is_some() && pwd_physical(rest).is_some() => {
                let logical = self.state.directory.clone().unwrap();
                // `pwd -P` prints the physical directory: links resolved,
                // as the kernel reads it (TSK-1002 review 06). A UNC or
                // namespace place is never resolved.
                let shown = if pwd_physical(rest) == Some(true) {
                    logical
                        .to_str()
                        .filter(|text| !crate::security::outward::remote_or_namespace_operand(text))
                        .and_then(|_| crate::portable_path::canonicalize(&logical).ok())
                } else {
                    Some(logical)
                };
                self.output(match shown {
                    Some(dir) => {
                        match crate::git::GitName::from_os_str(dir.as_os_str()).rule_text() {
                            Ok(text) => Ok(Value::text(&format!("{text}\n"))),
                            Err(_) => Err("working directory is not UTF-8"),
                        }
                    }
                    None => Err("cannot establish the physical working directory"),
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
                // This evaluator does not follow directory changes, so after
                // one its directory is unknown: a later `pwd` in the same
                // capture (`$(cd alias && pwd)`) cannot print the place it
                // started from (TSK-1002 review 08). The command-position
                // reader applies the main line's changes itself.
                self.state.directory = None;
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
                            for name in ["printf", "echo", "print", "true", "false", ":"] {
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
            binding: None,
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
            Some(words) => self.eval_text(&paths::eval_body(&words)),
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
