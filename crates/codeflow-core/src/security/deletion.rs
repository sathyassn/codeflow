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
//! **Closed world: the reader allows only what it models.** The grammar
//! below is the allowlist. A construct outside it is never guessed at:
//! from the point it runs, every variable and the working directory are
//! unknown, and a later recursive deletion whose operand holds an unknown
//! value, or whose relative operand runs in an unknown directory, is
//! refused as unproven. The refusal names the construct and asks for a
//! literal project path; a literal `cd` gives a known directory again.
//!
//! - *Structure:* simple commands with prefix assignments and
//!   redirections (heredocs and here-strings included); lists, `&&`, `||`,
//!   `!` and pipelines; `( … )` and `{ … }`; `if`, `while`, `until`, zsh's
//!   `repeat`; `for NAME [in …]`, `for ((…))` and `select`; `case`;
//!   function definitions and zsh's anonymous functions; `coproc`, whose
//!   command runs in a child shell; `[[ … ]]` and `(( … ))`.
//! - *Words:* the three quotings, `$'…'` with its escapes decoded; `~`,
//!   `~user`, `~+`, `~-`, `~N` and zsh's `~NAME`; `$NAME`, `${NAME}`,
//!   `${NAME[i]}` and the `:-`, `:=`, `:?`, `:+`, `%` and `#` forms;
//!   `$((…))` and `$[…]`; `$(…)`, backticks and process substitution;
//!   arrays; brace expansion; globs, `**/` included.
//! - *Command position:* a name spelled literally, or through a variable
//!   whose every value the reader knows (each value is judged); a function
//!   or alias the line defines; a builtin in `MODELLED_BUILTINS`, in the
//!   forms [`Reader::builtin`] reads, or in `INERT_BUILTINS`; the prefix
//!   words `command`, `builtin`, `exec`, `time`, `noglob`, `nocorrect` and
//!   `-`; the wrappers the `rm` check unwraps (`env`, `sudo`, `nice`,
//!   `nohup`, `timeout` and the like); any other program, which runs in a
//!   child process.
//! - *Code between commands:* `trap` with a literal action and zsh's hook
//!   functions (`TRAPDEBUG`, `chpwd`, `precmd`, …). Once one is set, its
//!   action may run at every command boundary after, so the state after
//!   each command also joins the state after the action, to a fixpoint.
//!   A reset of that signal (`trap - SIG`, `trap '' SIG`, a new action) or
//!   a removal of that function (`unfunction`, `unset -f`, a new body)
//!   takes the action away from there on, where it runs on every path,
//!   outside any function call, and names what the action was set under.
//!
//! Everything else taints, among it: a builtin in `UNMODELLED_BUILTINS`
//! (`enable`, `emulate`, `fc`, `zmodload`, `autoload`, `integer`) or a
//! modelled one with an option the reader does not model (`set -k`,
//! `shopt -s dotglob`, `setopt autocd`); a command name holding a value the
//! reader does not know; a sourced file; `eval` of text it cannot resolve;
//! a name reference (`declare -n`); a trap action it cannot read; an
//! assignment to a name that changes how the shell runs
//! (`SPECIAL_WRITES`: `CDPATH`, `BASH_ENV`, `PWD`, `argv`, the hook
//! arrays); an integer name given a non-number; arithmetic it cannot
//! resolve; `cd` with an option or operand form it does not model; and
//! text outside the grammar (a reserved word out of place, a construct or
//! quote left open, zsh's `foreach`, short `if … { … }` or `{ … } always`),
//! which the parser keeps as an unknown node while still judging its best
//! reading. A function call or nested script deeper than the reader
//! follows, and a loop it cannot settle, taint what they may change. Some
//! constructs give an unknown value rather than an unknown state: `read`
//! with an option it does not model or from input it cannot see,
//! `mapfile`, `getopts`, a case-converting attribute (`declare -u`), a
//! field split on an unknown `IFS`, a parameter expansion it does not
//! resolve (`${V/x/y}`, `${!V}`, zsh's flags and modifiers), a variable the
//! shell sets from what the line does (`SPECIAL_READS`: `BASH_REMATCH`,
//! `DIRSTACK`, `funcstack`), the output of a function or of a builtin it
//! does not print for, and a glob it cannot list on disk. The tests
//! `every_node_kind_is_modelled_or_taints` and
//! `every_word_part_is_modelled_or_taints` match every node and word kind
//! without a wildcard arm, so a new kind does not compile until it is
//! given a sample that shows it is modelled or taints.
//!
//! **A command that may fail is read both ways.** `cd` enters only a
//! directory the reader sees exists; otherwise it may fail, and the line
//! continues in the old directory on the failure branch of `&&`, `||`,
//! `if` and `!`.
//!
//! **Precise where it is cheap.** Single quotes keep `$VAR` literal; a `for`
//! over a fixed list leaves its variable at the last word when the body
//! neither leaves early nor assigns it; `${VAR:?}`, `${VAR:-…}`,
//! `${VAR:=…}` (which also assigns) and `${VAR%…}` expand to their possible
//! values; an array holds every element it may have; `set --`, `shift`,
//! `read` and `printf -v` set what they set; words split on the `IFS` the
//! line sets; brace expansion reaches across expansions (`{build,$D}`) and
//! ranges (`{a..z}`); a substitution's output is what its `echo`,
//! `print`, `printf`, `pwd` or heredoc `cat` commands print, in order; a
//! glob is matched against the disk and against the protected names; and a
//! path that exists is also judged where it really lands (the real path of
//! its longest existing prefix), so a link to `/` is `/`.
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
    read_deletion(command, base, cfg!(windows))
}

/// The protected deletion `command` performs when it starts in `base`;
/// with `rooted_unplaced`, as on native Windows, a `/`-rooted path is
/// placed nowhere, so deleting below one is unproven.
fn read_deletion(command: &str, base: Option<&Path>, rooted_unplaced: bool) -> Option<Found> {
    let mut reader = Reader {
        base,
        rooted_unplaced,
        found: None,
        depth: 0,
        pipe_input: None,
        jumps: Vec::new(),
        expanding: Vec::new(),
        traps: Vec::new(),
        in_trap: false,
        status: None,
    };
    let mut state = State::start();
    reader.script(command, &mut state);
    // A trap's action runs when the line ends, in the state it ends in.
    state.dead = false;
    reader.in_trap = true;
    for action in reader.live_traps(&state) {
        let mut end = state.clone();
        reader.child_run(&action, &mut end);
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
    /// `$NAME` or `${NAME…}`; `index` holds the text of a subscript
    /// (`${NAME[i]}`), which is evaluated for its side effects and read as
    /// every element.
    Param {
        name: String,
        op: Option<ParamOp>,
        quoted: bool,
        index: Option<String>,
    },
    /// `$((…))` or `$[…]`: an arithmetic expression, which gives a number
    /// and may assign.
    Arith { text: String, quoted: bool },
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
/// sets one element of an array (`index` holds the subscript's parts).
#[derive(Debug, Clone, PartialEq)]
struct Assign {
    name: String,
    append: bool,
    element: bool,
    index: Vec<Part>,
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
        let mut subscript_parts = Vec::new();
        let (element, index, after) = if let Some(subscript) = rest.strip_prefix('[') {
            let found = if let Some(end) = subscript.find(']') {
                subscript_parts.push(Part::Lit {
                    text: subscript[..end].to_string(),
                    quoted: false,
                });
                Some((0, &subscript[end + 1..]))
            } else {
                subscript_parts.push(Part::Lit {
                    text: subscript.to_string(),
                    quoted: false,
                });
                self.parts.iter().enumerate().skip(1).find_map(|(k, part)| {
                    let Part::Lit {
                        text,
                        quoted: false,
                    } = part
                    else {
                        subscript_parts.push(part.clone());
                        return None;
                    };
                    if let Some(end) = text.find(']') {
                        subscript_parts.push(Part::Lit {
                            text: text[..end].to_string(),
                            quoted: false,
                        });
                        Some((k, &text[end + 1..]))
                    } else {
                        subscript_parts.push(part.clone());
                        None
                    }
                })
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
            index: subscript_parts,
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
    /// Set when a quote, substitution or expansion is never closed: the
    /// shell reads that text differently, or not at all.
    broken: bool,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            at: 0,
            start: 0,
            broken: false,
        }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Split a script into words, operators and redirections, and whether
    /// the text was cut off inside a quote or expansion.
    fn tokens(mut self) -> (Vec<Tok>, bool) {
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
                // zsh's `<N-M>` numeric glob, written against a word.
                '<' if word.is_some() && self.numeric_glob().is_some() => {
                    let end = self.numeric_glob().unwrap_or(self.at);
                    let text: String = self.chars[self.at..end].iter().collect();
                    self.at = end;
                    self.begin(&mut word).parts.push(Part::Opaque {
                        text,
                        quoted: false,
                        unknown: Some(why::GLOB),
                    });
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    // A process substitution runs its command.
                    self.begin(&mut word);
                    let (inner, next) = self.balanced(self.at + 1, '(', ')');
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
        (toks, self.broken)
    }

    /// The end of a `<N-M>` numeric glob at the cursor, if one is there.
    fn numeric_glob(&self) -> Option<usize> {
        let mut i = self.at + 1;
        while self.chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        if self.chars.get(i) != Some(&'-') {
            return None;
        }
        i += 1;
        while self.chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        (self.chars.get(i) == Some(&'>')).then_some(i + 1)
    }

    /// [`balanced`] from `open_at`, noting text that is never closed.
    fn balanced(&mut self, open_at: usize, open: char, close: char) -> (String, usize) {
        let (inner, next) = balanced(&self.chars, open_at, open, close);
        if next > self.chars.len() || self.chars.get(next.wrapping_sub(1)) != Some(&close) {
            self.broken = true;
        }
        (inner, next)
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
            let (inner, next) = self.balanced(self.at, '(', ')');
            self.at = next;
            let (elements, broken) = Lexer::new(&inner).tokens();
            self.broken |= broken;
            let elements = elements
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
            // `(( … ))`: an arithmetic command.
            self.begin(word);
            let (inner, next) = self.balanced(self.at, '(', ')');
            self.at = next;
            let text = inner
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .unwrap_or(&inner)
                .to_string();
            if let Some(w) = word.as_mut() {
                w.parts.push(Part::Arith {
                    text,
                    quoted: false,
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
            if crate::hooks::git_guard::shell_blank(ch)
                || matches!(ch, ';' | '&' | '|' | '(' | ')' | '<' | '>')
            {
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
                let mut closed = false;
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    if c == '\'' {
                        closed = true;
                        break;
                    }
                    text.push(c);
                }
                self.broken |= !closed;
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
            // zsh replaces `=NAME` at the start of a word with the path of
            // the command NAME.
            '=' if split
                && word.parts.is_empty()
                && self
                    .peek(1)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) =>
            {
                word.parts.push(Part::Opaque {
                    text: "=".to_string(),
                    quoted: false,
                    unknown: Some(why::PARAM),
                });
                self.at += 1;
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
        self.broken = true;
    }

    fn backtick(&mut self) -> String {
        self.at += 1;
        let mut script = String::new();
        let mut closed = false;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '`' => {
                    closed = true;
                    break;
                }
                '\\' if matches!(self.peek(0), Some('`' | '\\' | '$')) => {
                    if let Some(next) = self.peek(0) {
                        script.push(next);
                    }
                    self.at += 1;
                }
                _ => script.push(c),
            }
        }
        self.broken |= !closed;
        script
    }

    /// `$…` at the cursor, one arm per form.
    #[allow(clippy::too_many_lines)]
    fn dollar(&mut self, word: &mut Word, quoted: bool) {
        match self.peek(1) {
            Some('(') if self.peek(2) == Some('(') => {
                let (inner, next) = self.balanced(self.at + 1, '(', ')');
                self.at = next;
                let text = inner
                    .strip_prefix('(')
                    .and_then(|t| t.strip_suffix(')'))
                    .unwrap_or(&inner)
                    .to_string();
                word.parts.push(Part::Arith { text, quoted });
            }
            Some('(') => {
                let (script, next) = self.balanced(self.at + 1, '(', ')');
                self.at = next;
                word.parts.push(Part::Subst { script, quoted });
            }
            // bash's older `$[…]` arithmetic.
            Some('[') => {
                let (text, next) = self.balanced(self.at + 1, '[', ']');
                self.at = next;
                word.parts.push(Part::Arith { text, quoted });
            }
            Some('{') => {
                let (inner, next) = self.balanced(self.at + 1, '{', '}');
                self.at = next;
                word.parts.push(parameter(&inner, quoted));
            }
            Some('\'') if !quoted => {
                self.at += 2;
                let mut text = String::new();
                let mut closed = false;
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    match c {
                        '\'' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            let (decoded, used) = ansi_c_escape(&self.chars[self.at..]);
                            self.at += used;
                            text.push_str(&decoded);
                        }
                        _ => text.push(c),
                    }
                }
                self.broken |= !closed;
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
                // zsh reads `$NAME[…]` as a subscript and `$NAME:h` as a
                // modifier; bash reads them as text after the value.
                let zsh_suffix = match (self.peek(0), self.peek(1)) {
                    (Some('['), _) => true,
                    (Some(':'), Some(m)) => "aAcehlpPqQrsStuxfFwWg&".contains(m),
                    _ => false,
                };
                if zsh_suffix {
                    word.parts.push(Part::Opaque {
                        text: format!("${name}"),
                        quoted,
                        unknown: Some(why::PARAM),
                    });
                } else {
                    word.parts.push(Part::Param {
                        name,
                        op: None,
                        quoted,
                        index: None,
                    });
                }
            }
            // A positional parameter, or `"$@"` and `"$*"`.
            Some(c) if c.is_ascii_digit() || c == '@' || c == '*' => {
                self.at += 2;
                word.parts.push(Part::Param {
                    name: c.to_string(),
                    op: None,
                    quoted,
                    index: None,
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
            // zsh's `$=NAME`, `$~NAME` and `$^NAME` split, glob or
            // distribute the value; `$+NAME` tests it.
            Some(c @ ('=' | '~' | '^' | '+'))
                if self
                    .peek(2)
                    .is_some_and(|n| n.is_ascii_alphabetic() || n == '_' || n == '{') =>
            {
                self.at += 2;
                // A braced form keeps its text, so an assignment in it
                // (`${NAME:=…}`) still takes effect.
                let text = if self.peek(0) == Some('{') {
                    let (text, next) = self.balanced(self.at, '{', '}');
                    self.at = next;
                    format!("${{{text}}}")
                } else {
                    while self
                        .peek(0)
                        .is_some_and(|n| n.is_ascii_alphanumeric() || n == '_')
                    {
                        self.at += 1;
                    }
                    format!("${c}")
                };
                let unknown = (c != '+').then_some(why::PARAM);
                word.parts.push(Part::Opaque {
                    text,
                    quoted,
                    unknown,
                });
            }
            _ => {
                word.push_lit('$', quoted);
                self.at += 1;
            }
        }
    }
}

/// The character an ANSI-C escape (`$'\\…'`) after its backslash gives, as
/// bash decodes it, and how many characters it takes.
fn ansi_c_escape(chars: &[char]) -> (String, usize) {
    let Some(&c) = chars.first() else {
        return ("\\".to_string(), 0);
    };
    let digits = |radix: u32, max: usize, from: usize| -> (u32, usize) {
        let mut value = 0u32;
        let mut n = 0;
        while n < max {
            match chars.get(from + n).and_then(|d| d.to_digit(radix)) {
                Some(d) => {
                    value = value.saturating_mul(radix).saturating_add(d);
                    n += 1;
                }
                None => break,
            }
        }
        (value, n)
    };
    let code = |value: u32| char::from_u32(value).map(String::from).unwrap_or_default();
    match c {
        'a' => ("\u{7}".to_string(), 1),
        'b' => ("\u{8}".to_string(), 1),
        'e' | 'E' => ("\u{1b}".to_string(), 1),
        'f' => ("\u{c}".to_string(), 1),
        'n' => ("\n".to_string(), 1),
        'r' => ("\r".to_string(), 1),
        't' => ("\t".to_string(), 1),
        'v' => ("\u{b}".to_string(), 1),
        '\\' | '\'' | '"' | '?' => (c.to_string(), 1),
        '0'..='7' => {
            let (value, n) = digits(8, 3, 0);
            (code(value & 0xff), n)
        }
        'x' | 'u' | 'U' => {
            let max = match c {
                'x' => 2,
                'u' => 4,
                _ => 8,
            };
            match digits(16, max, 1) {
                (_, 0) => (format!("\\{c}"), 1),
                (value, n) => (code(value), n + 1),
            }
        }
        'c' => match chars.get(1) {
            Some(&k) => (code(u32::from(k) & 0x1f), 2),
            None => ("\\c".to_string(), 1),
        },
        _ => (format!("\\{c}"), 1),
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
    // Its subscript is kept, since evaluating it may assign or run.
    let mut index = None;
    if is_name(name) {
        if let Some(subscript) = rest.strip_prefix('[') {
            let chars = subscript_chars(subscript);
            let (inner, next) = balanced(&chars, 0, '[', ']');
            if next > chars.len() || chars.get(next.wrapping_sub(1)) != Some(&']') {
                return opaque(Some(why::PARAM));
            }
            let consumed: usize = chars[1..next].iter().map(|c| c.len_utf8()).sum();
            index = Some(inner);
            rest = &subscript[consumed..];
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
                | Part::Arith { quoted: q, .. }
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
        index,
    }
}

/// `[` and the text after it, for [`balanced`] to find the subscript's end.
fn subscript_chars(after_open: &str) -> Vec<char> {
    std::iter::once('[').chain(after_open.chars()).collect()
}

/// A word's literal text, for a heredoc delimiter.
fn word_text(word: &Word) -> String {
    word.parts
        .iter()
        .map(|part| match part {
            Part::Lit { text, .. } | Part::Opaque { text, .. } => text.clone(),
            Part::Tilde(user) => format!("~{user}"),
            Part::Param { name, .. } => format!("${name}"),
            Part::Arith { text, .. } => format!("$(({text}))"),
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
    /// The first runs; each later one may or may not: after `&&` (`true`
    /// in the second list) when the one before succeeded, after `||` when
    /// it failed.
    AndOr(Vec<Node>, Vec<bool>),
    /// `! PIPELINE`: its status is inverted.
    Not(Box<Node>),
    /// Each stage runs in a child shell.
    Pipe(Vec<Node>),
    If(Vec<(Node, Node)>, Option<Box<Node>>),
    /// `while`/`until` (and zsh's `repeat`): the condition runs at least
    /// once, the body any number of times.
    Loop(Box<Node>, Box<Node>),
    For(String, Option<Vec<Word>>, Box<Node>),
    /// `select NAME in …`: a loop whose variable takes any of the words,
    /// or none.
    Select(String, Option<Vec<Word>>, Box<Node>),
    /// `case WORD in PATTERN) …`: the subject and pattern words, which are
    /// expanded, and each arm.
    Case(Vec<Word>, Vec<Node>),
    Func(String, Box<Node>),
    /// zsh's anonymous function, `() { … } ARGS` or `function { … } ARGS`:
    /// called at once with its arguments.
    Anon(Box<Node>, Vec<Word>),
    /// `coproc [NAME] COMMAND`: the command runs in a child shell, and
    /// `NAME` (`COPROC` by default) holds its descriptors.
    Coproc(String, Box<Node>),
    /// `[[ … ]]`: its words are expanded, and an arithmetic comparison
    /// evaluates its operands.
    Cond(Vec<Word>),
    /// `(( … ))`: an arithmetic command.
    Arith(String),
    /// A compound command with redirections: its input feeds the commands
    /// inside.
    Redirected(Box<Node>, Vec<Redir>),
    /// Text the parser does not read as the shell does (a reserved word out
    /// of place, a construct left open, a quote never closed, zsh-only
    /// syntax): the state is unknown from here on, and the parser's best
    /// reading of the text is still judged.
    Unknown(&'static str, Box<Node>),
}

/// Words that begin a compound command.
const COMPOUND_STARTS: &[&str] = &["{", "if", "while", "until", "for", "select", "case", "[["];

/// Reserved words that close or continue a construct, or that only zsh
/// knows (`foreach … end`, `{ … } always { … }`): the parser never meets
/// one where a command starts unless the text is outside its grammar.
const OUT_OF_PLACE: &[&str] = &[
    "then", "fi", "do", "done", "esac", "elif", "else", "}", "end", "always", "foreach",
];

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn parse(script: &str) -> Node {
        let (toks, broken) = Lexer::new(script).tokens();
        let mut parser = Self { toks, at: 0 };
        let mut items = Vec::new();
        while parser.at < parser.toks.len() {
            let before = parser.at;
            items.push(parser.list(&[]));
            if parser.at == before {
                // An unmatched closer.
                parser.at += 1;
                items.push(Node::Unknown(why::SYNTAX, Box::new(Node::Seq(Vec::new()))));
            }
        }
        let node = Node::Seq(items);
        if broken {
            Node::Unknown(why::SYNTAX, Box::new(node))
        } else {
            node
        }
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

    /// Consume `word`, and whether it was there.
    fn expect_word(&mut self, word: &str) -> bool {
        let found = self.at_word(word);
        if found {
            self.at += 1;
        }
        found
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

    /// The text of a `(( … ))` word at the cursor.
    fn arith_word(&self) -> Option<String> {
        match self.peek() {
            Some(Tok::Word(w)) if w.src.starts_with("((") => match w.parts.as_slice() {
                [Part::Arith { text, .. }] => Some(text.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether the token at `at` begins a compound command.
    fn starts_compound(&self, at: usize) -> bool {
        match self.toks.get(at) {
            Some(Tok::Op(op)) => *op == "(",
            Some(Tok::Word(w)) => {
                w.plain().is_some_and(|w| COMPOUND_STARTS.contains(&w)) || w.src.starts_with("((")
            }
            _ => false,
        }
    }

    /// The words after the cursor, up to the next operator.
    fn words_until_op(&mut self) -> Vec<Word> {
        let mut words = Vec::new();
        while let Some(Tok::Word(w)) = self.peek() {
            words.push(w.clone());
            self.at += 1;
        }
        words
    }

    /// `node`, or the same marked unknown when its text did not close as
    /// the grammar requires.
    fn close(node: Node, ok: bool) -> Node {
        if ok {
            node
        } else {
            Node::Unknown(why::SYNTAX, Box::new(node))
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
        let mut ands = Vec::new();
        while self.at_op("&&") || self.at_op("||") {
            ands.push(self.at_op("&&"));
            self.at += 1;
            self.skip_newlines();
            items.push(self.pipeline());
        }
        if items.len() == 1 {
            items.remove(0)
        } else {
            Node::AndOr(items, ands)
        }
    }

    fn pipeline(&mut self) -> Node {
        // `time` (with `-p`) before a pipeline changes nothing; `!` inverts
        // its status.
        let mut negated = false;
        loop {
            if self.expect_word("!") {
                negated = !negated;
                continue;
            }
            if self.expect_word("time") {
                self.expect_word("-p");
                continue;
            }
            break;
        }
        let mut stages = vec![self.command()];
        while self.at_op("|") {
            self.at += 1;
            self.skip_newlines();
            stages.push(self.command());
        }
        let node = if stages.len() == 1 {
            stages.remove(0)
        } else {
            Node::Pipe(stages)
        };
        if negated {
            Node::Not(Box::new(node))
        } else {
            node
        }
    }

    #[allow(clippy::too_many_lines)]
    fn command(&mut self) -> Node {
        let node = if self.at_op("(") {
            self.at += 1;
            if self.at_op(")") {
                // zsh's anonymous function, `() { … } ARGS`.
                self.at += 1;
                self.skip_newlines();
                let body = self.command();
                let args = self.words_until_op();
                Node::Anon(Box::new(body), args)
            } else {
                let body = self.list(&[]);
                let closed = self.at_op(")");
                if closed {
                    self.at += 1;
                }
                Self::close(Node::Sub(Box::new(body)), closed)
            }
        } else if self.at_word("{") {
            self.at += 1;
            let body = self.list(&["}"]);
            let closed = self.expect_word("}");
            Self::close(body, closed)
        } else if self.at_word("if") {
            self.if_clause()
        } else if self.at_word("while") || self.at_word("until") {
            self.at += 1;
            let cond = self.list(&["do"]);
            let mut ok = self.expect_word("do");
            let body = self.list(&["done"]);
            ok &= self.expect_word("done");
            Self::close(Node::Loop(Box::new(cond), Box::new(body)), ok)
        } else if self.at_word("for") {
            self.for_clause(false)
        } else if self.at_word("select") {
            self.for_clause(true)
        } else if self.at_word("case") {
            self.case_clause()
        } else if self.at_word("function") {
            self.at += 1;
            if self.at_word("{") {
                // zsh's anonymous `function { … } ARGS`.
                let body = self.command();
                let args = self.words_until_op();
                Node::Anon(Box::new(body), args)
            } else {
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
            }
        } else if self.at_word("coproc") {
            self.at += 1;
            // `coproc NAME COMPOUND` names it; otherwise the rest is the
            // command.
            let named = match self.peek() {
                Some(Tok::Word(w)) if self.starts_compound(self.at + 1) => {
                    w.plain().filter(|n| is_name(n)).map(str::to_string)
                }
                _ => None,
            };
            if named.is_some() {
                self.at += 1;
            }
            let body = self.command();
            Node::Coproc(
                named.unwrap_or_else(|| "COPROC".to_string()),
                Box::new(body),
            )
        } else if self.at_word("repeat") {
            // zsh's `repeat N COMMAND` or `repeat N do …; done`.
            self.at += 1;
            let count: Vec<Word> = match self.peek() {
                Some(Tok::Word(w)) => {
                    let w = w.clone();
                    self.at += 1;
                    vec![w]
                }
                _ => Vec::new(),
            };
            self.skip_newlines();
            let (body, ok) = if self.expect_word("do") {
                let body = self.list(&["done"]);
                let ok = self.expect_word("done");
                (body, ok)
            } else {
                (self.command(), true)
            };
            Self::close(Node::Loop(Box::new(Node::Cond(count)), Box::new(body)), ok)
        } else if self.at_word("[[") {
            self.cond()
        } else if let Some(text) = self.arith_word() {
            self.at += 1;
            Node::Arith(text)
        } else if matches!(self.peek(), Some(Tok::Word(w)) if w.plain().is_some_and(|w| OUT_OF_PLACE.contains(&w)))
        {
            return Node::Unknown(why::SYNTAX, Box::new(self.simple()));
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
        let mut odd = false;
        loop {
            match self.peek() {
                Some(Tok::Word(w)) => {
                    // zsh closes a brace group at `}` in any position, and
                    // reads `((…))` as an argument as a glob.
                    let odd_word = !words.is_empty()
                        && (w.plain() == Some("}")
                            || (w.src.starts_with("((")
                                && matches!(w.parts.as_slice(), [Part::Arith { .. }])));
                    odd |= odd_word;
                    words.push(if odd_word && w.plain().is_none() {
                        Word {
                            parts: vec![Part::Opaque {
                                text: w.src.clone(),
                                quoted: false,
                                unknown: Some(why::SYNTAX),
                            }],
                            src: w.src.clone(),
                        }
                    } else {
                        w.clone()
                    });
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
        // A `(` after words is a glob qualifier or pattern to zsh and an
        // error to bash: the words it belongs to are unknown.
        if !words.is_empty() && self.at_op("(") {
            words.push(Word {
                parts: vec![Part::Opaque {
                    text: "(".to_string(),
                    quoted: false,
                    unknown: Some(why::SYNTAX),
                }],
                src: "(".to_string(),
            });
            odd = true;
        }
        Self::close(Node::Simple(words, redirs), !odd)
    }

    /// `[[ … ]]`: every token up to `]]` is a word of the test.
    fn cond(&mut self) -> Node {
        let start = self.at;
        self.at += 1;
        let mut words = Vec::new();
        let plain = |text: &str| Word {
            parts: vec![Part::Lit {
                text: text.to_string(),
                quoted: false,
            }],
            src: text.to_string(),
        };
        while let Some(tok) = self.peek().cloned() {
            self.at += 1;
            match tok {
                Tok::Word(w) if w.plain() == Some("]]") => return Node::Cond(words),
                Tok::Word(w) => words.push(w),
                Tok::Op(op) => words.push(plain(op)),
                Tok::Redir(r) => {
                    words.push(plain(&r.op));
                    words.extend(r.target);
                }
            }
        }
        // Never closed: read the rest as commands, marked unknown.
        self.at = start + 1;
        Node::Unknown(why::SYNTAX, Box::new(self.simple()))
    }

    fn if_clause(&mut self) -> Node {
        self.at += 1;
        let mut arms = Vec::new();
        let cond = self.list(&["then"]);
        let mut ok = self.expect_word("then");
        let body = self.list(&["elif", "else", "fi"]);
        arms.push((cond, body));
        while self.expect_word("elif") {
            let cond = self.list(&["then"]);
            ok &= self.expect_word("then");
            let body = self.list(&["elif", "else", "fi"]);
            arms.push((cond, body));
        }
        let otherwise = if self.expect_word("else") {
            Some(Box::new(self.list(&["fi"])))
        } else {
            None
        };
        ok &= self.expect_word("fi");
        Self::close(Node::If(arms, otherwise), ok)
    }

    fn for_clause(&mut self, select: bool) -> Node {
        self.at += 1;
        // `for (( INIT; COND; STEP ))`.
        if let Some(text) = self.arith_word().filter(|_| !select) {
            self.at += 1;
            while self.at_op(";") || self.at_op("\n") {
                self.at += 1;
            }
            let mut ok = self.expect_word("do");
            let body = self.list(&["done"]);
            ok &= self.expect_word("done");
            let parts: Vec<&str> = text.split(';').collect();
            let [init, cond, step] = parts.as_slice() else {
                return Node::Unknown(why::SYNTAX, Box::new(body));
            };
            let node = Node::Seq(vec![
                Node::Arith((*init).to_string()),
                Node::Loop(
                    Box::new(Node::Arith((*cond).to_string())),
                    Box::new(Node::Seq(vec![body, Node::Arith((*step).to_string())])),
                ),
            ]);
            return Self::close(node, ok);
        }
        let name = match self.peek() {
            Some(Tok::Word(w)) => w.plain().map(str::to_string),
            _ => None,
        };
        self.at += 1;
        self.skip_newlines();
        let mut words = None;
        if self.expect_word("in") {
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
        let mut ok = self.expect_word("do");
        let body = self.list(&["done"]);
        ok &= self.expect_word("done");
        match name {
            Some(name) if is_name(&name) => {
                let node = if select {
                    Node::Select(name, words, Box::new(body))
                } else {
                    Node::For(name, words, Box::new(body))
                };
                Self::close(node, ok)
            }
            _ => Node::Unknown(
                why::SYNTAX,
                Box::new(Node::Loop(Box::new(Node::Seq(Vec::new())), Box::new(body))),
            ),
        }
    }

    fn case_clause(&mut self) -> Node {
        self.at += 1;
        let mut words = Vec::new();
        let mut ok = true;
        match self.peek() {
            Some(Tok::Word(w)) => {
                words.push(w.clone());
                self.at += 1;
            }
            _ => ok = false,
        }
        self.skip_newlines();
        ok &= self.expect_word("in");
        let mut arms = Vec::new();
        let mut closed = false;
        loop {
            self.skip_newlines();
            if self.expect_word("esac") {
                closed = true;
                break;
            }
            if self.peek().is_none() {
                break;
            }
            if self.at_op("(") {
                self.at += 1;
            }
            // The patterns, separated by `|`, up to `)`.
            loop {
                match self.peek() {
                    Some(Tok::Word(w)) => {
                        words.push(w.clone());
                        self.at += 1;
                    }
                    Some(Tok::Op("|")) => self.at += 1,
                    Some(Tok::Op(")")) => {
                        self.at += 1;
                        break;
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            let before = self.at;
            arms.push(self.list(&["esac"]));
            if self.at_op(";;") || self.at_op(";&") || self.at_op(";;&") {
                self.at += 1;
            } else if self.at == before && !self.at_word("esac") {
                ok = false;
                break;
            }
        }
        Self::close(Node::Case(words, arms), ok && closed)
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
    pub const SYNTAX: &str = "shell syntax the guard does not parse exactly";
    pub const BUILTIN: &str = "a shell builtin the guard does not model";
    pub const OPTION: &str = "a shell option the guard does not model";
    pub const SPECIAL: &str = "a shell variable with a special meaning";
    pub const ARITH: &str = "arithmetic the guard cannot resolve";
    pub const CD: &str = "a `cd` form the guard does not model";
    pub const TRAP: &str = "a trap action the guard cannot resolve";
    pub const OUTPUT: &str = "the output of a shell command the guard does not resolve";
    pub const ROOTED: &str = "a `/`-rooted path, which names no fixed place on Windows: \
        the shell that runs it picks the root, and a junction can redirect any part of it";
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
    /// The reader's trap and hook actions (by index) that a reset or a
    /// removal took away on every path to here.
    retracted: BTreeSet<usize>,
    /// Functions made read-only, which `unset -f` and a new body leave.
    locked_funcs: BTreeSet<String>,
}

/// Variables whose value changes how the shell, or a shell it starts, reads
/// later commands in ways the reader does not follow: a startup file
/// (`BASH_ENV`, `ENV`, `ZDOTDIR`), `cd`'s search and logical path
/// (`CDPATH`, `PWD`, `OLDPWD`), the alias, hash and function tables,
/// options, globbing (`GLOBIGNORE`), trace expansion (`PS4`), the directory
/// stack, the positional parameters by another name (`argv`) and zsh's
/// hook arrays. Assigning one makes the state unknown.
const SPECIAL_WRITES: &[&str] = &[
    "BASH_ENV",
    "ENV",
    "ZDOTDIR",
    "BASH_ALIASES",
    "BASH_CMDS",
    "BASH_ARGV0",
    "BASH_ARGV",
    "BASH_ARGC",
    "BASH_COMPAT",
    "BASHOPTS",
    "SHELLOPTS",
    "POSIXLY_CORRECT",
    "CDPATH",
    "cdpath",
    "PWD",
    "OLDPWD",
    "GLOBIGNORE",
    "PS4",
    "PROMPT_COMMAND",
    "DIRSTACK",
    "dirstack",
    "argv",
    "NULLCMD",
    "READNULLCMD",
    "aliases",
    "galiases",
    "saliases",
    "dis_aliases",
    "dis_galiases",
    "dis_saliases",
    "functions",
    "functions_source",
    "dis_functions",
    "dis_functions_source",
    "builtins",
    "dis_builtins",
    "reswords",
    "dis_reswords",
    "options",
    "parameters",
    "commands",
    "nameddirs",
    "userdirs",
    "chpwd_functions",
    "precmd_functions",
    "preexec_functions",
    "periodic_functions",
    "zshexit_functions",
    "zshaddhistory_functions",
];

/// Variables the shell sets from what the line does, which the reader does
/// not follow (a regex match, the directory stack, the call stack, zsh's
/// tables): reading one gives an unknown value.
const SPECIAL_READS: &[&str] = &[
    "BASH_REMATCH",
    "DIRSTACK",
    "dirstack",
    "BASH_ARGV",
    "BASH_ARGC",
    "BASH_COMMAND",
    "BASH_SOURCE",
    "FUNCNAME",
    "BASH_LINENO",
    "BASH_ALIASES",
    "BASH_CMDS",
    "funcstack",
    "functrace",
    "funcfiletrace",
    "funcsourcetrace",
    "argv",
    "match",
    "MATCH",
    "mbegin",
    "mend",
    "MBEGIN",
    "MEND",
    "reply",
    "aliases",
    "galiases",
    "saliases",
    "functions",
    "functions_source",
    "builtins",
    "commands",
    "options",
    "parameters",
    "nameddirs",
    "userdirs",
    "history",
    "historywords",
    "jobtexts",
    "jobdirs",
    "jobstates",
    "modules",
    "reswords",
    "widgets",
    "keymaps",
    "patchars",
];

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
            retracted: BTreeSet::new(),
            locked_funcs: BTreeSet::new(),
        }
    }

    /// The values `name` may hold; one read from the environment is kept
    /// as spelled, except `HOME`, which is the home directory, and `IFS`,
    /// which the shell sets itself.
    fn var(&self, name: &str) -> Values {
        if let Some(reason) = self.wild {
            return [taint(reason)].into();
        }
        if SPECIAL_READS.contains(&name) {
            return [taint(why::SPECIAL)].into();
        }
        if matches!(name, "BASH_ARGV0" | "ZSH_ARGZERO") {
            return self.arg0.clone();
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
        if SPECIAL_WRITES.contains(&name) {
            self.go_wild(why::SPECIAL);
        }
        let values = if let Some(&reason) = self.sticky.get(name) {
            // An integer name evaluates what it is given as arithmetic,
            // which can assign and expand subscripts.
            if reason == why::ARITH && !values.iter().all(|v| integer_literal(v)) {
                self.go_wild(why::ARITH);
            }
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
        // Without `HOME`, bash reads `~` as the user's home directory.
        if name == "HOME" {
            values.insert("~".to_string());
        }
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
        self.retracted = self
            .retracted
            .intersection(&other.retracted)
            .copied()
            .collect();
        self.locked_funcs.extend(other.locked_funcs.iter().cloned());
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
        // An integer name adds what `+=` gives it; the value is unknown
        // anyway, so only what it is given is checked.
        let integer = self.sticky.get(&assign.name) == Some(&why::ARITH);
        let new: Values = if integer {
            values
        } else if assign.append && !array_value {
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

/// What a substitution prints, as far as the reader can tell.
#[derive(Debug, Clone, PartialEq)]
enum Output {
    /// Each text it may print; trailing newlines are dropped at the end.
    Known(Values),
    /// Output of a program the reader does not run, judged by its spelling
    /// (ADR-0009).
    External,
    /// Output of shell code the reader does not model.
    Unknown,
}

impl Output {
    fn empty() -> Self {
        Output::Known([String::new()].into())
    }

    fn is_empty(&self) -> bool {
        matches!(self, Output::Known(values) if values.iter().all(String::is_empty))
    }

    /// This output followed by `next`.
    fn then(self, next: Output) -> Output {
        match (self, next) {
            (Output::Known(a), Output::Known(b)) => {
                let joined: Values = a
                    .iter()
                    .flat_map(|x| b.iter().map(move |y| format!("{x}{y}")))
                    .collect();
                if joined.len() > MAX_VALUES {
                    Output::Unknown
                } else {
                    Output::Known(joined)
                }
            }
            (Output::External, other) | (other, Output::External)
                if other == Output::External || other.is_empty() =>
            {
                Output::External
            }
            _ => Output::Unknown,
        }
    }

    /// Either this output or `other`.
    fn or(self, other: Output) -> Output {
        match (self, other) {
            (Output::Known(mut a), Output::Known(b)) => {
                a.extend(b);
                Output::Known(a)
            }
            (Output::External, other) | (other, Output::External)
                if other == Output::External || other.is_empty() =>
            {
                Output::External
            }
            _ => Output::Unknown,
        }
    }
}

struct Reader<'a> {
    base: Option<&'a Path>,
    /// Set on native Windows: a `/`-rooted path names no fixed place.
    rooted_unplaced: bool,
    found: Option<Found>,
    depth: usize,
    /// What the pipeline stage being read receives on its input.
    pipe_input: Option<Fed>,
    jumps: Vec<Frame>,
    /// Aliases being expanded, which are not expanded again inside.
    expanding: Vec<String>,
    /// Trap actions and hook functions, each with what it was set under:
    /// each may run before any later command, and runs again when the line
    /// ends, unless the state has it retracted.
    traps: Vec<(Slot, Node)>,
    /// Set while a trap action is read; it runs no trap itself.
    in_trap: bool,
    /// Where the last simple command left the line when it succeeds and
    /// when it fails, when those differ (a `cd` that may fail).
    status: Option<Status>,
}

/// The working directory and `OLDPWD` after a `cd` that succeeds and
/// after one that fails; `None` where that cannot happen.
#[derive(Debug, Clone)]
struct Status {
    ok: Option<(Values, Values)>,
    fail: Option<(Values, Values)>,
}

impl Status {
    fn invert(&mut self) {
        std::mem::swap(&mut self.ok, &mut self.fail);
    }

    fn merge(&mut self, other: Status) {
        fn union(mine: &mut Option<(Values, Values)>, theirs: Option<(Values, Values)>) {
            match (mine.as_mut(), theirs) {
                (Some((cwd, oldpwd)), Some((more_cwd, more_oldpwd))) => {
                    cwd.extend(more_cwd);
                    oldpwd.extend(more_oldpwd);
                }
                (None, theirs) => *mine = theirs,
                (Some(_), None) => {}
            }
        }
        union(&mut self.ok, other.ok);
        union(&mut self.fail, other.fail);
    }
}

/// Whether the status of `node` is that of a simple command it ends with.
fn ends_in_simple(node: &Node) -> bool {
    match node {
        Node::Simple(..) => true,
        Node::Seq(items) => items.last().is_some_and(ends_in_simple),
        Node::Not(body) | Node::Redirected(body, _) => ends_in_simple(body),
        _ => false,
    }
}

/// A simple command as [`Reader::dispatch`] runs it: its words from the
/// command name on, prefix assignments, input, whether it has several
/// argument vectors, and how many of its words are literal text.
struct Call<'c> {
    words: &'c [Word],
    env: &'c [(String, Values)],
    input: Option<&'c Fed>,
    ambiguous: bool,
    literal: usize,
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
        let status = self.status.take();
        self.jumps.push(Frame {
            kind: FrameKind::Child,
            left: Vec::new(),
        });
        self.script(script, child);
        self.jumps.pop();
        self.status = status;
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
        // A child's `cd` is not this shell's status.
        let status = (kind == FrameKind::Child).then(|| self.status.take());
        self.jumps.push(Frame {
            kind,
            left: Vec::new(),
        });
        self.run(body, st);
        let frame = self.jumps.pop().expect("the frame pushed above");
        if let Some(status) = status {
            self.status = status;
        }
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

    /// Read `node`, one arm per node kind.
    #[allow(clippy::too_many_lines)]
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
            Node::AndOr(items, ands) => {
                self.run(&items[0], st);
                let (mut ok, mut fail) = self.outcome(&items[0], st);
                for (item, and) in items[1..].iter().zip(ands) {
                    let mut taken = if *and { ok.clone() } else { fail.clone() };
                    self.run(item, &mut taken);
                    let (taken_ok, taken_fail) = self.outcome(item, &taken);
                    if *and {
                        ok = taken_ok;
                        fail.join(&taken_fail);
                    } else {
                        ok.join(&taken_ok);
                        fail = taken_fail;
                    }
                }
                ok.join(&fail);
                *st = ok;
                self.status = None;
            }
            Node::Not(body) => {
                self.run(body, st);
                if let Some(status) = self.status.as_mut() {
                    status.invert();
                }
            }
            Node::Pipe(stages) => self.pipe(stages, st),
            Node::If(arms, otherwise) => {
                let mut cur = st.clone();
                let mut outs = Vec::new();
                for (cond, body) in arms {
                    self.run(cond, &mut cur);
                    let (ok, fail) = self.outcome(cond, &cur);
                    let mut taken = ok;
                    self.run(body, &mut taken);
                    outs.push(taken);
                    cur = fail;
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
            Node::For(name, words, body) => {
                self.for_loop(name, words.as_deref(), body, false, st);
            }
            Node::Select(name, words, body) => {
                self.for_loop(name, words.as_deref(), body, true, st);
            }
            Node::Case(words, arms) => {
                self.apply_traps(st);
                for word in words {
                    let _ = self.fields(word, st);
                }
                self.side_effects(words, st);
                let entry = st.clone();
                for arm in arms {
                    let mut taken = entry.clone();
                    self.run(arm, &mut taken);
                    st.join(&taken);
                }
            }
            Node::Func(name, body) => {
                st.funcs.insert(name.clone(), vec![Some((**body).clone())]);
                // A trap function or hook runs between later commands; a
                // new body replaces the one before.
                if is_hook(name) {
                    let slot = Slot::Function(name.clone());
                    self.retract(&slot, st);
                    self.register(slot, (**body).clone(), st);
                }
            }
            Node::Anon(body, words) => {
                self.apply_traps(st);
                let mut variants = self.argv_variants(words, st);
                self.side_effects(words, st);
                for argv in &mut variants {
                    argv.insert(0, "(anon)".to_string());
                }
                self.call(vec![Some((**body).clone())], &variants, &[], "(anon)", st);
            }
            Node::Coproc(name, body) => {
                self.apply_traps(st);
                let mut child = st.clone();
                self.frame(FrameKind::Child, body, &mut child);
                // The coprocess's descriptors and process ID are numbers.
                st.set(name, [format!("${{{name}}}")].into());
                st.set(&format!("{name}_PID"), [format!("${{{name}_PID}}")].into());
            }
            Node::Cond(words) => {
                self.apply_traps(st);
                self.condition(words, st);
            }
            Node::Arith(text) => {
                self.apply_traps(st);
                self.arith(text, st);
            }
            Node::Redirected(body, redirs) => {
                let fed = self.redirect_input(redirs, st);
                self.redirect_effects(redirs, st);
                let outer = self.pipe_input.clone();
                if fed.is_some() {
                    self.pipe_input = fed;
                }
                self.run(body, st);
                self.pipe_input = outer;
            }
            Node::Unknown(reason, inner) => {
                st.go_wild(reason);
                self.run(inner, st);
            }
            Node::Simple(words, redirs) => {
                self.apply_traps(st);
                self.status = None;
                self.simple(words, redirs, st);
            }
        }
    }

    /// The states after `node` when it succeeds and when it fails: they
    /// differ only after a `cd` that may fail, read as the last command of
    /// `node`.
    fn outcome(&mut self, node: &Node, st: &State) -> (State, State) {
        let status = self.status.take();
        match status.filter(|_| ends_in_simple(node)) {
            Some(status) => {
                let branch = |taken: Option<(Values, Values)>| {
                    let mut branch = st.clone();
                    match taken {
                        Some((cwd, oldpwd)) => {
                            branch.cwd = cwd;
                            branch.oldpwd = oldpwd;
                        }
                        None => branch.dead = true,
                    }
                    branch
                };
                (branch(status.ok), branch(status.fail))
            }
            None => (st.clone(), st.clone()),
        }
    }

    /// A trap action or hook may run before any command once it is set,
    /// any number of times: the state here is joined with every state the
    /// actions may leave, until it settles.
    fn apply_traps(&mut self, st: &mut State) {
        if self.in_trap || st.dead || self.done() || self.live_traps(st).is_empty() {
            return;
        }
        self.in_trap = true;
        let mut before = st.clone();
        let mut settled = false;
        for _ in 0..LOOP_PASSES {
            let mut next = st.clone();
            for action in &self.live_traps(st) {
                let mut ran = st.clone();
                self.frame(FrameKind::Child, action, &mut ran);
                next.join(&ran);
            }
            if next == *st {
                settled = true;
                break;
            }
            before = std::mem::replace(st, next);
        }
        if !settled {
            st.widen(&before);
        }
        self.in_trap = false;
    }

    /// The trap and hook actions `st` has not retracted.
    fn live_traps(&self, st: &State) -> Vec<Node> {
        self.traps
            .iter()
            .enumerate()
            .filter(|(at, _)| !st.retracted.contains(at))
            .map(|(_, (_, action))| action.clone())
            .collect()
    }

    /// Set `action` under `slot`: it is live from here on, even where an
    /// earlier reset took the same action away.
    fn register(&mut self, slot: Slot, action: Node, st: &mut State) {
        let known = self
            .traps
            .iter()
            .position(|(s, a)| *s == slot && *a == action);
        let at = known.unwrap_or_else(|| {
            self.traps.push((slot, action));
            self.traps.len() - 1
        });
        st.retracted.remove(&at);
    }

    /// Take every action set under `slot` away from here on. A path that
    /// skips the reset keeps the action, since states join by what every
    /// path retracted. Inside a function call nothing is taken away: zsh
    /// may restore the traps a function changes (`localtraps`) and runs a
    /// function's `EXIT` trap when it returns.
    fn retract(&self, slot: &Slot, st: &mut State) {
        if !st.scopes.is_empty() {
            return;
        }
        if let Slot::Function(name) = slot {
            if st.locked_funcs.contains(name) {
                return;
            }
        }
        for (at, (set, _)) in self.traps.iter().enumerate() {
            if set == slot {
                st.retracted.insert(at);
            }
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

    /// A `for` loop, or with `select` a `select` loop, whose variable may
    /// take any of the words or none, and `REPLY` whatever is typed.
    fn for_loop(
        &mut self,
        name: &str,
        words: Option<&[Word]>,
        body: &Node,
        select: bool,
        st: &mut State,
    ) {
        self.apply_traps(st);
        let (mut values, fixed) = match words {
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
                self.side_effects(words, st);
                (values, fixed.then_some(last).flatten())
            }
            // `for NAME` walks the positional parameters.
            None => match st.wild {
                Some(reason) => ([taint(reason)].into(), None),
                None => (st.args.iter().flatten().cloned().collect(), None),
            },
        };
        let fixed = if select {
            values.insert(String::new());
            None
        } else {
            fixed
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
            if select {
                pass.set("REPLY", [taint(why::READ_INPUT)].into());
            }
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
                all.append(&mut values);
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
            self.status = None;
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
        self.redirect_effects(redirs, st);
        if command.is_empty() {
            // Assignments alone take effect in order, each seeing the last.
            for word in assignments {
                if let Some(assign) = word.assignment() {
                    self.subscript_effects(&assign, st);
                    let values = self.value(&assign.value, st);
                    st.assign(&assign, values);
                }
            }
            self.side_effects(words, st);
            // What `$_` holds after an assignment alone is not followed.
            st.vars
                .insert("_".to_string(), [taint(why::SPECIAL)].into());
            return;
        }
        // zsh assigns a positional parameter written `1=…`.
        if command
            .first()
            .and_then(literal_text)
            .is_some_and(|text| positional_assignment(&text))
        {
            st.go_wild(why::SPECIAL);
            return;
        }
        // A prefix assignment may see the ones before it (bash) or not.
        let mut scratch = st.clone();
        let mut env = Vec::new();
        for word in assignments {
            if let Some(assign) = word.assignment() {
                self.subscript_effects(&assign, &mut scratch);
                let mut values = self.value(&assign.value, &scratch);
                values.extend(self.value(&assign.value, st));
                scratch.assign(&assign, values);
                env.push((assign.name.clone(), scratch.var(&assign.name)));
            }
        }
        let variants = self.argv_variants(command, st);
        self.side_effects(words, st);
        let literal = command
            .iter()
            .take_while(|w| literal_text(w).is_some())
            .count();
        let underscore = st.vars.get("_").cloned();
        let call = Call {
            words: command,
            env: &env,
            input: input.as_ref(),
            ambiguous: variants.len() > 1,
            literal,
        };
        self.dispatch(&call, &variants, st);
        // `$_` is the last argument of the command, or what a function
        // left in it.
        if !st.dead {
            let mut last: Values = variants.iter().filter_map(|a| a.last().cloned()).collect();
            if st.vars.get("_") != underscore.as_ref() {
                if let Some(inner) = st.vars.get("_") {
                    last.extend(inner.iter().cloned());
                }
            }
            st.vars.insert("_".to_string(), bounded(last));
        }
    }

    /// Run a command with its argument vectors. The words before its name
    /// that still run it in this shell (`builtin`, `command`) are read from
    /// each vector's values, so a variable naming one is followed too;
    /// each program it may be is read on its own and the states joined.
    fn dispatch(&mut self, call: &Call, variants: &[Vec<String>], st: &mut State) {
        type Group = (Option<(usize, bool)>, String, Vec<Vec<String>>);
        let mut groups: Vec<Group> = Vec::new();
        for argv in variants {
            let prefix = builtin_prefix(argv);
            let program = prefix
                .and_then(|(skip, _)| argv.get(skip).cloned())
                .unwrap_or_default();
            match groups
                .iter_mut()
                .find(|(p, k, _)| *p == prefix && *k == program)
            {
                Some((_, _, group)) => group.push(argv.clone()),
                None => groups.push((prefix, program, vec![argv.clone()])),
            }
        }
        if groups.len() <= 1 {
            if let Some((prefix, program, group)) = groups.pop() {
                self.dispatch_program(call, prefix, &program, &group, st);
            }
            return;
        }
        let entry = st.clone();
        let mut out: Option<State> = None;
        let mut statuses = Vec::new();
        for (prefix, program, group) in groups {
            let mut taken = entry.clone();
            self.dispatch_program(call, prefix, &program, &group, &mut taken);
            statuses.push(self.status.take());
            match &mut out {
                Some(out) => out.join(&taken),
                None => out = Some(taken),
            }
        }
        if let Some(out) = out {
            *st = out;
        }
        // Each program's status holds only if every one of them has one.
        if statuses.iter().all(Option::is_some) {
            let mut merged: Option<Status> = None;
            for status in statuses.into_iter().flatten() {
                match merged.as_mut() {
                    Some(known) => known.merge(status),
                    None => merged = Some(status),
                }
            }
            self.status = merged;
        }
    }

    /// Run one program: a function or a builtin changes this shell, and
    /// anything else runs in a child and is judged for what it deletes.
    fn dispatch_program(
        &mut self,
        call: &Call,
        prefix: Option<(usize, bool)>,
        program: &str,
        variants: &[Vec<String>],
        st: &mut State,
    ) {
        // `command -v` only prints.
        let Some((skip, functions)) = prefix else {
            return;
        };
        if program.is_empty() {
            return;
        }
        let argvs: Vec<Vec<String>> = variants
            .iter()
            .map(|argv| argv.get(skip..).unwrap_or_default().to_vec())
            .collect();
        // A command the reader cannot name may be any builtin: `cd`,
        // `eval`, `source`. What it deletes as a program is still judged.
        if unproven(program).is_some() || (spelled(program) && !program.contains('/')) {
            let child = st.clone();
            let fed = self.pipe_input.clone().or_else(|| call.input.cloned());
            for argv in &argvs {
                self.exec(argv, &child, fed.as_ref(), false, call.ambiguous);
            }
            st.go_wild(why::COMMAND);
            return;
        }
        let literal = skip < call.literal;
        if functions {
            if let Some(bodies) = st.funcs.get(program).cloned() {
                let defined: Vec<Option<Node>> =
                    bodies.iter().filter(|b| b.is_some()).cloned().collect();
                if !bodies.contains(&None) {
                    self.call(defined, &argvs, call.env, program, st);
                    return;
                }
                // Undefined on some path: read the builtin or program too.
                let mut other = st.clone();
                self.run_program(call, skip, program, &argvs, literal, &mut other);
                if defined.is_empty() {
                    *st = other;
                } else {
                    self.call(defined, &argvs, call.env, program, st);
                    st.join(&other);
                    self.status = None;
                }
                return;
            }
        }
        self.run_program(call, skip, program, &argvs, literal, st);
    }

    /// A builtin by what the table says of it, or a program in a child.
    fn run_program(
        &mut self,
        call: &Call,
        skip: usize,
        program: &str,
        argvs: &[Vec<String>],
        literal: bool,
        st: &mut State,
    ) {
        let Some(kind) = builtin_kind(program) else {
            let mut child = st.clone();
            for (name, values) in call.env {
                child.set(name, values.clone());
            }
            let fed = self.pipe_input.clone().or_else(|| call.input.cloned());
            for argv in argvs {
                self.exec(argv, &child, fed.as_ref(), false, call.ambiguous);
            }
            return;
        };
        // A builtin sees its prefix assignments, and they may persist (a
        // special builtin in POSIX mode).
        let before: Vec<(String, Values)> = call
            .env
            .iter()
            .map(|(name, _)| (name.clone(), st.var(name)))
            .collect();
        for (name, values) in call.env {
            st.set(name, values.clone());
        }
        match kind {
            // A declaration reached through an expansion parses its
            // arguments as text (`NAME=(…)` included), which the reader
            // does not follow.
            Builtin::Modelled if !literal && DECLARATIONS.contains(&program) => {
                st.go_wild(why::COMMAND);
            }
            Builtin::Modelled => {
                let words = if literal {
                    call.words.get(skip..).unwrap_or_default()
                } else {
                    &[]
                };
                let entry = st.clone();
                let mut out: Option<State> = None;
                for argv in argvs {
                    let mut taken = entry.clone();
                    self.builtin(program, words, argv, call.input, &mut taken);
                    match &mut out {
                        Some(out) => out.join(&taken),
                        None => out = Some(taken),
                    }
                }
                if let Some(out) = out {
                    *st = out;
                }
            }
            Builtin::Inert => {}
            Builtin::Unmodelled => st.go_wild(why::BUILTIN),
        }
        for (name, old) in before {
            if !st.dead {
                let mut both = old;
                both.extend(st.var(&name));
                st.set(&name, both);
            }
        }
    }

    /// Call a function: its body runs with the call's arguments and prefix
    /// assignments, and its locals and arguments are restored on return.
    /// zsh gives `$0` the function's name.
    fn call(
        &mut self,
        bodies: Vec<Option<Node>>,
        variants: &[Vec<String>],
        env: &[(String, Values)],
        name: &str,
        st: &mut State,
    ) {
        if self.depth > MAX_DEPTH {
            if st.wild.is_some() {
                return;
            }
            st.go_wild(why::DEPTH);
        }
        let caller_args = std::mem::replace(&mut st.args, positionals(variants, 1));
        let caller_zero = st.arg0.clone();
        st.arg0.insert(name.to_string());
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
            st.arg0 = caller_zero;
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
            "exit" | "bye" => st.dead = true,
            "cd" | "pushd" | "popd" | "chdir" => {
                let (cwd, may_fail) = self.change_dir(argv, st);
                let before = st.clone();
                st.set_cwd(cwd);
                // A `cd` that may fail leaves the directory as it was.
                let status = Status {
                    ok: Some((st.cwd.clone(), st.oldpwd.clone())),
                    fail: may_fail.then(|| (before.cwd.clone(), before.oldpwd.clone())),
                };
                match self.status.as_mut() {
                    Some(known) => known.merge(status),
                    None => self.status = Some(status),
                }
                if may_fail {
                    st.join(&before);
                }
            }
            "export" | "readonly" | "local" | "declare" | "typeset" => {
                self.declare(program, words, st);
            }
            "unset" | "unfunction" => {
                let mut mode = if program == "unfunction" { 'f' } else { ' ' };
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
                                self.retract(&Slot::Function(name.to_string()), st);
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
                let mut strip_delimiter = false;
                while let Some(arg) = argv.get(at) {
                    if !arg.starts_with('-') {
                        break;
                    }
                    modelled &= arg == "-t";
                    strip_delimiter |= arg == "-t";
                    at += 1;
                }
                let name = argv.get(at).map_or("MAPFILE", String::as_str);
                let values: Values = match fed {
                    Some(fed) if fed.complete && modelled => fed
                        .items
                        .iter()
                        .flat_map(|i| {
                            i.value
                                .split_inclusive('\n')
                                .map(|line| {
                                    if strip_delimiter {
                                        line.strip_suffix('\n').unwrap_or(line)
                                    } else {
                                        line
                                    }
                                })
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
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
            "set" => Self::set_builtin(argv, st),
            "shopt" => {
                let mut names = Vec::new();
                let mut changes = false;
                for arg in &argv[1..] {
                    match arg.strip_prefix('-') {
                        Some(flags) if !flags.is_empty() => changes |= flags.contains(['s', 'u']),
                        _ => names.push(arg.as_str()),
                    }
                }
                if changes && names.iter().any(|name| !inert_option(name)) {
                    st.go_wild(why::OPTION);
                }
            }
            "setopt" | "unsetopt" => {
                if argv[1..]
                    .iter()
                    .any(|arg| arg.starts_with(['-', '+']) || !inert_option(arg))
                {
                    st.go_wild(why::OPTION);
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
                    // zsh's global (`-g`) and suffix (`-s`) aliases expand
                    // outside the command position.
                    if arg.len() > 1 && arg.starts_with(['-', '+']) {
                        if arg.contains(['g', 's']) {
                            st.go_wild(why::ALIAS);
                        }
                        continue;
                    }
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
                if let (Some("-v"), Some(name)) = (argv.get(1).map(String::as_str), argv.get(2)) {
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
            // zsh's `print -v NAME` assigns what it would print.
            "print" => {
                if argv[1..]
                    .iter()
                    .take_while(|a| a.starts_with('-') && a.as_str() != "-" && a.as_str() != "--")
                    .any(|a| a.contains('v'))
                {
                    st.go_wild(why::BUILTIN);
                }
            }
            "trap" => self.trap(argv, st),
            "let" => {
                for arg in &argv[1..] {
                    self.arith_scan(arg, st, 0);
                }
            }
            "test" | "[" => {
                for (at, arg) in argv.iter().enumerate() {
                    if matches!(arg.as_str(), "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge") {
                        for operand in [at.wrapping_sub(1), at + 1] {
                            if let Some(operand) = argv.get(operand).filter(|_| operand > 0) {
                                self.arith_scan(operand, st, 0);
                            }
                        }
                    }
                }
            }
            // Listing forms print; the rest change what later commands run.
            "hash" => {
                if argv[1..].iter().any(|a| a != "-r") {
                    st.go_wild(why::BUILTIN);
                }
            }
            "functions" => {
                if argv.len() > 1 {
                    st.go_wild(why::BUILTIN);
                }
            }
            "dirs" => {
                if argv[1..].iter().any(|a| !a.starts_with('-')) {
                    st.go_wild(why::BUILTIN);
                }
            }
            "wait" | "jobs" => {
                let flag = if program == "wait" { 'p' } else { 'x' };
                if argv[1..]
                    .iter()
                    .any(|a| a.starts_with('-') && a.contains(flag))
                {
                    st.go_wild(why::BUILTIN);
                }
            }
            _ => {}
        }
    }

    /// `trap ACTION SIGNAL…`: the action may run before any later command,
    /// and runs again when the line ends. It replaces what each signal had,
    /// as a reset (`-`) or an ignore (`''`) does.
    fn trap(&mut self, argv: &[String], st: &mut State) {
        let operands = match argv.get(1).map(String::as_str) {
            Some("--") => &argv[2..],
            _ => &argv[1..],
        };
        match operands {
            // Listing and printing forms, and a lone operand, whose reading
            // differs between the shells.
            [] | [_] => {}
            [first, ..] if first.len() > 1 && first.starts_with('-') => {}
            [action, signals @ ..] => {
                let clears = action == "-" || action.is_empty();
                if !clears && unproven(action).is_some() {
                    st.go_wild(why::TRAP);
                    return;
                }
                if resets_each(signals) {
                    for signal in signals {
                        self.retract(&Slot::Signal(signal.clone()), st);
                    }
                }
                if clears {
                    return;
                }
                let action = Parser::parse(action);
                for signal in signals {
                    self.register(Slot::Signal(signal.clone()), action.clone(), st);
                }
            }
        }
    }

    /// `set`: its options, and the positional parameters after them.
    fn set_builtin(argv: &[String], st: &mut State) {
        let mut at = 1;
        while let Some(arg) = argv.get(at) {
            if arg == "--" || arg == "-" {
                st.args = positionals(&[argv.to_vec()], at + 1);
                return;
            }
            if arg.len() > 1 && arg.starts_with(['-', '+']) {
                let letters = &arg[1..];
                if letters
                    .chars()
                    .any(|c| c != 'o' && !INERT_SET_LETTERS.contains(c))
                {
                    st.go_wild(why::OPTION);
                }
                if letters.contains('o') {
                    // `-o NAME`; `-o` alone prints.
                    if let Some(name) = argv.get(at + 1) {
                        if !inert_option(name) {
                            st.go_wild(why::OPTION);
                        }
                    }
                    at += 2;
                } else {
                    at += 1;
                }
                continue;
            }
            st.args = positionals(&[argv.to_vec()], at);
            return;
        }
    }

    /// Where `cd`, `pushd`, `popd` or `chdir` with `argv` leaves the line
    /// when it succeeds, and whether it may fail: only a directory the
    /// reader sees exists is taken as entered. `-P` takes the real path.
    fn change_dir(&self, argv: &[String], st: &State) -> (Values, bool) {
        let unknown: Values = [taint(why::CD)].into();
        let program = program_name(&argv[0]);
        let mut physical = false;
        let mut operands: Vec<&str> = Vec::new();
        let mut only_operands = false;
        for arg in &argv[1..] {
            if !only_operands && arg == "--" {
                only_operands = true;
                continue;
            }
            if !only_operands && arg.len() > 1 && arg.starts_with('-') && !stack_entry(arg) {
                for flag in arg[1..].chars() {
                    match flag {
                        'L' | 'q' | 's' | 'e' | 'n' => {}
                        'P' => physical = true,
                        _ => return (unknown, true),
                    }
                }
                continue;
            }
            operands.push(arg);
        }
        if program == "popd" {
            return (st.visited.clone(), true);
        }
        let target = match operands.as_slice() {
            [] if program == "pushd" => return (st.oldpwd.clone(), true),
            [] => return (st.var("HOME"), true),
            ["-"] => return (st.oldpwd.clone(), true),
            [entry] if stack_entry(entry) => return (st.visited.clone(), true),
            [dir] => *dir,
            // zsh's `cd OLD NEW` substitutes in the current directory.
            _ => return (unknown, true),
        };
        let mut out = Values::new();
        let mut may_fail = false;
        for cwd in &st.cwd {
            let dir = resolve(cwd, target);
            if let Some(reason) = unproven(&dir) {
                out.insert(taint(reason));
                may_fail = true;
                continue;
            }
            may_fail |= !self
                .absolute(&dir)
                .is_some_and(|path| std::fs::metadata(path).is_ok_and(|m| m.is_dir()));
            out.insert(if physical {
                self.real_path(&dir).unwrap_or(dir)
            } else {
                dir
            });
        }
        (out, may_fail)
    }

    /// `export`, `readonly`, `local`, `declare` and `typeset`.
    fn declare(&mut self, program: &str, words: &[Word], st: &mut State) {
        let (at, flags) = declaration_flags(words);
        // Printing and functions change no variable. A function made
        // read-only cannot be removed or given a new body.
        if flags.contains(['f', 'F', 'p']) {
            if flags.contains('f') && (program == "readonly" || flags.contains('r')) {
                for word in &words[at..] {
                    let Some(name) = literal_text(word) else {
                        st.go_wild(why::NAME);
                        return;
                    };
                    st.locked_funcs.insert(name);
                }
            }
            return;
        }
        if flags.contains('n') {
            st.go_wild(why::NAMEREF);
            return;
        }
        // `-i` makes each assignment arithmetic.
        let unmodelled = flags.chars().any(|c| !"aAglrtuxc".contains(c));
        let case = flags.contains(['l', 'u', 'c']);
        let global = flags.contains('g');
        let in_function = !st.scopes.is_empty();
        let local = program == "local"
            || (matches!(program, "declare" | "typeset") && in_function && !global);
        let readonly = program == "readonly" || flags.contains('r');
        let array = flags.contains(['a', 'A']);
        let integer = flags.contains('i');
        for word in &words[at..] {
            let mut names = Vec::new();
            if let Some(assign) = word.assignment() {
                if local {
                    st.declare_local(&assign.name);
                }
                let values = self.value(&assign.value, st);
                if integer && !values.iter().all(|v| integer_literal(v)) {
                    st.go_wild(why::ARITH);
                }
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
                            if integer && !integer_literal(value) {
                                st.go_wild(why::ARITH);
                            }
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
                    let reason = if case {
                        why::CASE
                    } else if flags.contains('i') {
                        why::ARITH
                    } else {
                        why::DECLARE
                    };
                    st.sticky.insert(name.clone(), reason);
                    st.set(&name, Values::new());
                }
                if readonly {
                    st.readonly.insert(name);
                }
            }
        }
    }

    /// `${NAME:=word}` and `${NAME=word}` store the word they expand to,
    /// and arithmetic and subscripts evaluate.
    fn side_effects(&mut self, words: &[Word], st: &mut State) {
        for word in words {
            self.part_effects(&word.parts, st);
        }
    }

    fn part_effects(&mut self, parts: &[Part], st: &mut State) {
        for part in parts {
            match part {
                Part::Param {
                    name, op, index, ..
                } => {
                    if let Some(index) = index {
                        self.arith(index, st);
                    }
                    match op {
                        Some(op @ ParamOp::Default { word, assign }) => {
                            self.part_effects(word, st);
                            if *assign && is_name(name) {
                                let values = self.param_values(name, Some(op), st);
                                st.set(name, values);
                            }
                        }
                        Some(ParamOp::Alternate(word)) => self.part_effects(word, st),
                        _ => {}
                    }
                }
                Part::Arith { text, .. } => self.arith(text, st),
                Part::Array(words) => self.side_effects(words, st),
                // An expansion the reader does not resolve may still assign
                // inside (`${V/x/${D:=…}}`); `${!V=…}` assigns a name it
                // cannot see.
                Part::Opaque { text, .. } => {
                    if let Some(inner) = text.strip_prefix("${").and_then(|t| t.strip_suffix('}')) {
                        if inner.starts_with('!') && inner.contains('=') {
                            st.go_wild(why::NAME);
                        }
                        self.part_effects(&lex_word(inner).parts, st);
                    }
                }
                Part::Lit { .. } | Part::Tilde(_) | Part::Subst { .. } => {}
            }
        }
    }

    /// A redirection's target is expanded in this shell, and so is an
    /// unquoted heredoc's body.
    fn redirect_effects(&mut self, redirs: &[Redir], st: &mut State) {
        for redir in redirs {
            if let Some(target) = &redir.target {
                self.part_effects(&target.parts, st);
            }
            if let (Some(body), true) = (&redir.body, redir.expand) {
                self.part_effects(&heredoc_word(body).parts, st);
            }
        }
    }

    /// An element assignment's subscript is evaluated.
    fn subscript_effects(&mut self, assign: &Assign, st: &mut State) {
        if !assign.element {
            return;
        }
        let values = self.value(&assign.index, st);
        self.part_effects(&assign.index, st);
        for value in values {
            self.arith_scan(&value, st, 0);
        }
    }

    /// `[[ … ]]`: its words are expanded, and each operand of an arithmetic
    /// comparison is evaluated.
    fn condition(&mut self, words: &[Word], st: &mut State) {
        let values: Vec<Values> = words.iter().map(|w| self.value(&w.parts, st)).collect();
        self.side_effects(words, st);
        for (at, word) in words.iter().enumerate() {
            if matches!(
                word.plain(),
                Some("-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge")
            ) {
                for operand in [at.wrapping_sub(1), at + 1] {
                    for value in values.get(operand).cloned().unwrap_or_default() {
                        self.arith_scan(&value, st, 0);
                    }
                }
            }
        }
    }

    /// Evaluate arithmetic text: its expansions first, then what it
    /// assigns.
    fn arith(&mut self, text: &str, st: &mut State) {
        let word = lex_word(text);
        let values = self.value(&word.parts, st);
        self.part_effects(&word.parts, st);
        for value in values {
            self.arith_scan(&value, st, 0);
        }
    }

    /// What an expanded arithmetic expression changes: each name it
    /// assigns takes a number, and each name it reads is evaluated in turn
    /// when its value is itself an expression. A value that would expand
    /// again, or that the reader does not know, makes the state unknown.
    fn arith_scan(&mut self, expr: &str, st: &mut State, depth: usize) {
        if unproven(expr).is_some() || depth > 4 {
            st.go_wild(why::ARITH);
            return;
        }
        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '$' {
                // A value from the environment, kept as spelled.
                i += 1;
                while chars.get(i).is_some_and(|c| {
                    c.is_ascii_alphanumeric() || *c == '_' || *c == '{' || *c == '}'
                }) {
                    i += 1;
                }
            } else if c.is_ascii_digit() {
                while chars.get(i).is_some_and(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '_' | '#' | '.' | '@')
                }) {
                    i += 1;
                }
            } else if c.is_ascii_alphabetic() || c == '_' {
                let start = i;
                while chars
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                let mut j = i;
                while chars
                    .get(j)
                    .is_some_and(|c| crate::hooks::git_guard::shell_blank(*c))
                {
                    j += 1;
                }
                if chars.get(j) == Some(&'[') {
                    let (subscript, next) = balanced(&chars, j, '[', ']');
                    if depth > 0 && subscript.contains(['$', '`']) {
                        // A subscript in a value is expanded when it is
                        // evaluated.
                        self.arith(&subscript, st);
                        st.go_wild(why::ARITH);
                    } else {
                        self.arith_scan(&subscript, st, depth);
                    }
                    j = next;
                    while chars
                        .get(j)
                        .is_some_and(|c| crate::hooks::git_guard::shell_blank(*c))
                    {
                        j += 1;
                    }
                }
                let before: String = chars[..start]
                    .iter()
                    .rev()
                    .filter(|c| !crate::hooks::git_guard::shell_blank(**c))
                    .take(2)
                    .collect();
                let after = |k: usize| chars.get(j + k).copied();
                let assigns = before == "++"
                    || before == "--"
                    || match (after(0), after(1), after(2)) {
                        (Some('='), next, _) => next != Some('='),
                        (Some('+' | '-' | '*' | '/' | '%' | '&' | '^' | '|'), Some('='), _)
                        | (Some('+'), Some('+'), _)
                        | (Some('-'), Some('-'), _)
                        | (Some('<'), Some('<'), Some('='))
                        | (Some('>'), Some('>'), Some('='))
                        | (Some('*'), Some('*'), Some('=')) => true,
                        _ => false,
                    };
                self.arith_reference(&name, st, depth);
                if assigns {
                    // A number, read like `$((…))`.
                    st.set(&name, [format!("$(({name}))")].into());
                }
                i = j.max(i);
            } else {
                i += 1;
            }
        }
    }

    /// A name an arithmetic expression reads: a value that is itself an
    /// expression is evaluated too.
    fn arith_reference(&mut self, name: &str, st: &mut State, depth: usize) {
        for value in st.var(name) {
            let text = value.trim_matches(crate::hooks::git_guard::shell_blank);
            if text.is_empty() || spelled(text) || is_number(text) {
                continue;
            }
            if unproven(text).is_some() || text.contains('`') {
                st.go_wild(why::ARITH);
                return;
            }
            self.arith_scan(text, st, depth + 1);
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
            if self.rooted_unplaced && path.starts_with('/') {
                // Its spelling, or a name its glob may match, can still
                // be protected; where it lands cannot be proven.
                let named = dangerous_rm_target(&path).or_else(|| {
                    self.glob_paths(&path)?
                        .iter()
                        .find_map(|p| dangerous_rm_target(p))
                });
                if let Some(target) = named {
                    return Judged::Protected(target);
                }
                pending.get_or_insert_with(|| Unproven {
                    reason: why::ROOTED.to_string(),
                    cwd: false,
                });
                continue;
            }
            let landed = |path: &str| {
                let real = self.real_path_checked(path);
                let target = dangerous_rm_target(path).or_else(|| {
                    real.as_ref()
                        .ok()
                        .and_then(|p| p.as_deref())
                        .and_then(dangerous_rm_target)
                });
                (target, real.is_err())
            };
            let (target, unplaced) = landed(&path);
            if let Some(target) = target {
                return Judged::Protected(target);
            }
            if unplaced {
                pending.get_or_insert_with(|| Unproven {
                    reason: why::NAME.to_string(),
                    cwd: false,
                });
            }
            match self.glob_paths(&path) {
                Some(paths) => {
                    for candidate in &paths {
                        let (target, unplaced) = landed(candidate);
                        if let Some(target) = target {
                            return Judged::Protected(target);
                        }
                        if unplaced {
                            pending.get_or_insert_with(|| Unproven {
                                reason: why::NAME.to_string(),
                                cwd: false,
                            });
                        }
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
            // OS text rule (issue 79): the reader works on path text, so a
            // home folder that is not valid UTF-8 cannot be placed (unproven).
            let mut absolute = std::env::var_os("HOME")?;
            absolute.push(rest);
            Some(PathBuf::from(absolute))
        } else if path.starts_with('/') {
            Some(PathBuf::from(path))
        } else {
            Some(self.base?.join(path))
        }
    }

    /// The real path of `path`'s longest existing prefix with the rest
    /// appended; `None` for a path the reader cannot place.
    fn real_path(&self, path: &str) -> Option<String> {
        self.real_path_checked(path).ok().flatten()
    }

    /// Distinguish unreadable name bytes from ordinary unresolved syntax.
    /// Only the former must turn an otherwise ordinary deletion into an
    /// unproven one; e.g. an unset variable can still be an empty operand.
    fn real_path_checked(&self, path: &str) -> Result<Option<String>, ()> {
        if self.rooted_unplaced && path.starts_with('/') {
            return Ok(None);
        }
        let Some(absolute) = self.absolute(path) else {
            return Ok(None);
        };
        real_prefix_checked(&absolute)
    }

    /// Every path a glob in `path` may name: its matches on disk, and the
    /// protected names it could match where the disk does not show them.
    /// A final `*` is kept, since the protected classification reads
    /// `dir/*` itself, and a link the last component matches is left out,
    /// since the deletion removes the link; a trailing `/` follows it. Empty for a path without a glob or one the reader
    /// cannot place; `None` when a directory cannot be listed or there are
    /// too many matches.
    fn glob_paths(&self, path: &str) -> Option<Vec<String>> {
        if !path.contains(['*', '?', '[']) {
            return Some(Vec::new());
        }
        let Some(absolute) = self.absolute(path) else {
            // No concrete base/syntax to enumerate; no bytes were decoded.
            // Symbolic project-relative paths keep the spelling verdict.
            return Some(Vec::new());
        };
        // A base that is not valid UTF-8 cannot be read as text: unproven.
        let absolute = absolute.to_str()?.to_string();
        // `*/` names directories through links, so only a bare final `*`
        // is kept.
        let keep_last = !absolute.ends_with('/');
        let parts: Vec<&str> = absolute.split('/').filter(|p| !p.is_empty()).collect();
        let mut dirs = vec![String::new()];
        let mut deeper = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let globbed = part.contains(['*', '?', '[']);
            let mut next = Vec::new();
            // zsh (and bash `globstar`) read `**/` as any number of
            // directories, and `***/` through links too. Its match at no
            // depth and at one is read below; a deeper one can be protected
            // only under `/` or a protected directory, and then the whole
            // tree stands for it. `***/` may reach anything.
            if i + 1 < parts.len() && part.starts_with("**") && part.chars().all(|c| c == '*') {
                if part.len() > 2 {
                    return None;
                }
                for dir in &dirs {
                    let listed = if dir.is_empty() { "/" } else { dir.as_str() };
                    let real = real_prefix(Path::new(listed))?;
                    let real = real.trim_end_matches('/');
                    if real.is_empty() || dangerous_rm_target(&format!("{real}/deeper")).is_some() {
                        deeper.push(format!("{}/*", if real.is_empty() { "" } else { real }));
                    }
                    next.push(dir.clone());
                }
            }
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
                // A link matched by the last component is removed itself,
                // not followed.
                let last = i + 1 == parts.len() && keep_last;
                match std::fs::read_dir(listed) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if last && entry.file_type().is_ok_and(|t| t.is_symlink()) {
                                continue;
                            }
                            // OS text rule (issue 79): a name that is not valid
                            // UTF-8 cannot be placed as text. When the glob
                            // could match it, the deletion is unproven; else
                            // it is not an operand.
                            let name = entry.file_name();
                            match name.to_str() {
                                Some(text) => {
                                    names.insert(text.to_string());
                                }
                                None => {
                                    // A `?` or a `[..]` consumes one byte of
                                    // a name in the C locale, where the lossy
                                    // spelling holds one character for a
                                    // run of bytes, so the lossy match
                                    // cannot rule such a name out. With only
                                    // literals and `*`, it answers as the
                                    // exact bytes would.
                                    if part.contains(['?', '[', '\\'])
                                        || glob_match(part, &name.to_string_lossy())
                                    {
                                        return None;
                                    }
                                }
                            }
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
        dirs.extend(deeper);
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
        // zsh does not split an unquoted parameter at all.
        let mut modes = ifs_modes(st);
        if !modes.contains(&Ok(String::new())) {
            modes.push(Ok(String::new()));
        }
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
            Part::Arith { text, .. } => vec![(format!("$(({text}))"), Kind::Quoted)],
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
                // `~` is `$HOME`, which the line may set.
                "" => st
                    .var("HOME")
                    .into_iter()
                    .map(|home| (home, Kind::Quoted))
                    .collect(),
                // `~+N` and `~-N` name the directory stack; any user's home
                // is a home directory; and zsh reads `~NAME` as a named
                // directory, which a variable holding an absolute path
                // defines.
                _ if stack_entry(user) || user.chars().all(|c| c.is_ascii_digit()) => st
                    .visited
                    .iter()
                    .map(|c| (display_dir(c), Kind::Quoted))
                    .collect(),
                _ => {
                    let mut out = vec![("~".to_string(), Kind::Quoted)];
                    if is_name(user) && (st.wild.is_some() || st.vars.contains_key(user.as_str())) {
                        for value in st.var(user) {
                            if unproven(&value).is_some() || value.starts_with('/') {
                                out.push((value, Kind::Quoted));
                            }
                        }
                    }
                    out
                }
            },
            Part::Param {
                name, op, quoted, ..
            } => {
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
                    Output::Known(outputs) => outputs.into_iter().map(|v| (v, kind)).collect(),
                    Output::External => vec![(format!("$({script})"), Kind::Quoted)],
                    Output::Unknown => vec![(taint(why::OUTPUT), Kind::Quoted)],
                }
            }
            Part::Array(elements) => {
                let text: Vec<String> = elements.iter().map(word_text).collect();
                vec![(format!("({})", text.join(" ")), Kind::Quoted)]
            }
        }
    }

    /// Read a substitution in a child shell, judging what it deletes, and
    /// give what it prints: known when every command in it prints text
    /// the reader models (`echo`, `printf`, `print`, `pwd`, `cat` of a
    /// heredoc) or nothing, a program's own output when a program the
    /// reader does not run prints it (ADR-0009), and unknown otherwise.
    fn substitution(&mut self, script: &str, st: &State) -> Output {
        let mut child = st.clone();
        if self.depth > MAX_DEPTH {
            self.child_script(script, &mut child);
            return Output::Unknown;
        }
        let node = Parser::parse(script);
        let status = self.status.take();
        self.jumps.push(Frame {
            kind: FrameKind::Child,
            left: Vec::new(),
        });
        self.depth += 1;
        let out = self.output_of(&node, &mut child);
        self.depth -= 1;
        self.jumps.pop();
        self.status = status;
        match out {
            Output::Known(values) => Output::Known(
                values
                    .into_iter()
                    .map(|v| v.trim_end_matches('\n').to_string())
                    .collect(),
            ),
            other => other,
        }
    }

    /// Run `node` and give what it prints.
    fn output_of(&mut self, node: &Node, st: &mut State) -> Output {
        if st.dead || self.done() {
            return Output::empty();
        }
        match node {
            Node::Seq(items) => {
                let mut out = Output::empty();
                for item in items {
                    let next = self.output_of(item, st);
                    out = out.then(next);
                }
                out
            }
            Node::AndOr(items, _) => {
                let mut out = self.output_of(&items[0], st);
                for item in &items[1..] {
                    let mut taken = st.clone();
                    let next = self.output_of(item, &mut taken);
                    st.join(&taken);
                    out = out.then(next.or(Output::empty()));
                }
                out
            }
            Node::Sub(body) => {
                let mut child = st.clone();
                self.output_of(body, &mut child)
            }
            Node::Simple(words, redirs) => {
                let printed = self.printed(words, redirs, st);
                self.run(node, st);
                printed
            }
            Node::Pipe(stages) => {
                let out = match stages.last() {
                    Some(Node::Simple(words, redirs)) => self.printed(words, redirs, st),
                    Some(other) => self.silent_output(other, st),
                    None => Output::empty(),
                };
                self.run(node, st);
                out
            }
            Node::Func(..) | Node::Arith(_) | Node::Cond(_) | Node::Coproc(..) => {
                self.run(node, st);
                Output::empty()
            }
            _ => {
                let out = self.silent_output(node, st);
                self.run(node, st);
                out
            }
        }
    }

    /// What a compound command prints, by the commands inside it: nothing
    /// when none of them prints, a program's output when only programs do,
    /// and unknown otherwise.
    fn silent_output(&mut self, node: &Node, st: &State) -> Output {
        let mut simples = Vec::new();
        collect_simples(node, &mut simples);
        let mut out = Output::empty();
        for (words, redirs) in simples {
            match self.printed(&words, &redirs, st) {
                printed if printed.is_empty() => {}
                Output::External => out = out.then(Output::External),
                _ => return Output::Unknown,
            }
        }
        if matches!(node, Node::Unknown(..) | Node::Anon(..)) {
            return Output::Unknown;
        }
        out
    }

    /// What a simple command prints.
    fn printed(&mut self, words: &[Word], redirs: &[Redir], st: &State) -> Output {
        let assigned = words
            .iter()
            .take_while(|w| w.assignment().is_some())
            .count();
        let command = &words[assigned..];
        if command.is_empty() {
            return Output::empty();
        }
        // An alias is expanded before the command runs.
        if command[0]
            .plain()
            .is_some_and(|w| st.aliases.contains_key(w))
        {
            return Output::Unknown;
        }
        let mut out: Option<Output> = None;
        for argv in self.argv_variants(command, st) {
            let next = self.printed_argv(&argv, redirs, st);
            out = Some(match out {
                Some(out) => out.or(next),
                None => next,
            });
        }
        out.unwrap_or_else(Output::empty)
    }

    fn printed_argv(&mut self, argv: &[String], redirs: &[Redir], st: &State) -> Output {
        // `command -v` prints where a command is.
        let Some((skip, functions)) = builtin_prefix(argv) else {
            return Output::Unknown;
        };
        let Some(program) = argv.get(skip) else {
            return Output::empty();
        };
        if unproven(program).is_some() || (spelled(program) && !program.contains('/')) {
            return Output::Unknown;
        }
        if functions && st.funcs.contains_key(program) {
            return Output::Unknown;
        }
        let operands = &argv[skip + 1..];
        if builtin_kind(program).is_some() {
            return match program.as_str() {
                "echo" => Output::Known(echo_output(operands)),
                "print" => print_output(operands).map_or(Output::Unknown, Output::Known),
                "printf" if operands.first().map(String::as_str) != Some("-v") => {
                    printf(operands).map_or(Output::Unknown, |text| Output::Known([text].into()))
                }
                "pwd" => Output::Known(
                    st.cwd
                        .iter()
                        .map(|c| format!("{}\n", display_dir(c)))
                        .collect(),
                ),
                "cd" | "chdir" if operands.iter().any(|a| a == "-") => Output::Unknown,
                name if SILENT_BUILTINS.contains(&name) => Output::empty(),
                name if SILENT_WITH_ARGS.contains(&name)
                    && !operands.is_empty()
                    && !operands
                        .iter()
                        .any(|a| a.starts_with('-') && a.contains('p'))
                    && (operands.len() != 1 || !matches!(operands[0].as_str(), "-o" | "+o")) =>
                {
                    Output::empty()
                }
                _ => Output::Unknown,
            };
        }
        let Some(run) = unwrap(&argv[skip..]) else {
            return Output::Unknown;
        };
        let Some(first) = run.argv.first() else {
            return Output::empty();
        };
        match program_name(first).as_str() {
            "echo" => Output::Known(echo_output(&run.argv[1..])),
            "printf" => {
                printf(&run.argv[1..]).map_or(Output::Unknown, |text| Output::Known([text].into()))
            }
            "pwd" => Output::Known(
                st.cwd
                    .iter()
                    .map(|c| format!("{}\n", display_dir(c)))
                    .collect(),
            ),
            // `cat` with no file passes its heredoc or here-string on.
            "cat" if run.argv.len() == 1 => match self.redirect_input(redirs, st) {
                Some(fed) if fed.complete => {
                    Output::Known(fed.items.into_iter().map(|i| i.value).collect())
                }
                _ => Output::External,
            },
            _ => Output::External,
        }
    }

    /// Read `node` in a child shell of the line.
    fn child_run(&mut self, node: &Node, child: &mut State) {
        self.frame(FrameKind::Child, node, child);
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
                        for value in echo_output(&argv[1..]) {
                            out.push(Input {
                                value: value.trim_end_matches('\n').to_string(),
                                tree: false,
                            });
                        }
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

/// How the reader treats a command name that is a builtin of bash, zsh,
/// dash, ksh or mksh. Any other name is a program: it runs in a child
/// process and changes nothing in this shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Builtin {
    /// Read for what it does to the state (`cd`, `read`, `eval`, `trap`).
    Modelled,
    /// Changes nothing the reader follows (`echo`, `kill`, `umask`).
    Inert,
    /// Changes the shell in a way the reader does not follow: the state is
    /// unknown after it.
    Unmodelled,
}

/// Builtins the reader reads for their effect, some only in part (see
/// [`Reader::builtin`]: `set`, `shopt`, `setopt` and `alias` with an option
/// it does not model, `hash`, `functions` or `dirs` with arguments, `print
/// -v`, `wait -p` and `jobs -x` make the state unknown).
const MODELLED_BUILTINS: &[&str] = &[
    "break",
    "continue",
    "return",
    "exit",
    "bye",
    "cd",
    "pushd",
    "popd",
    "chdir",
    "export",
    "readonly",
    "local",
    "declare",
    "typeset",
    "unset",
    "unfunction",
    "read",
    "mapfile",
    "readarray",
    "getopts",
    "set",
    "shopt",
    "setopt",
    "unsetopt",
    "shift",
    "eval",
    "source",
    ".",
    "alias",
    "unalias",
    "printf",
    "print",
    "trap",
    "let",
    "test",
    "[",
    "hash",
    "functions",
    "dirs",
    "wait",
    "jobs",
];

/// Builtins that change nothing the reader follows: they print, test,
/// signal, or set limits, completion and key bindings.
const INERT_BUILTINS: &[&str] = &[
    ":",
    "true",
    "false",
    "echo",
    "pwd",
    "kill",
    "umask",
    "ulimit",
    "limit",
    "unlimit",
    "times",
    "type",
    "whence",
    "where",
    "which",
    "help",
    "caller",
    "bg",
    "fg",
    "disown",
    "suspend",
    "logout",
    "ttyctl",
    "echotc",
    "echoti",
    "log",
    "rehash",
    "zprof",
    "history",
    "complete",
    "compopt",
    "compctl",
    "compcall",
    "compadd",
    "compset",
    "comparguments",
    "compdescribe",
    "compfiles",
    "compgroups",
    "compquote",
    "comptags",
    "comptry",
    "compvalues",
    "bindkey",
    "zle",
    "clone",
    "pushln",
    "zcompile",
    "sleep",
    "cat",
    "realpath",
    "rename",
    "mknod",
    "getconf",
    "newgrp",
];

/// Builtins that change the shell in ways the reader does not follow:
/// they enable or load builtins, change options wholesale, re-run history,
/// autoload functions, assign through their own grammar, or declare in
/// forms the reader does not model.
const UNMODELLED_BUILTINS: &[&str] = &[
    "builtin",
    "enable",
    "disable",
    "emulate",
    "autoload",
    "zmodload",
    "sched",
    "vared",
    "getln",
    "zparseopts",
    "zformat",
    "zregexparse",
    "zstyle",
    "zselect",
    "zsocket",
    "ztcp",
    "zpty",
    "zftp",
    "zstat",
    "strftime",
    "sysopen",
    "sysread",
    "syswrite",
    "sysseek",
    "syserror",
    "zsystem",
    "zgetattr",
    "zsetattr",
    "zdelattr",
    "zlistattr",
    "zcurses",
    "ztie",
    "zuntie",
    "zgdbmpath",
    "pcre_compile",
    "pcre_match",
    "pcre_study",
    "example",
    "private",
    "nameref",
    "integer",
    "float",
    "fc",
    "r",
    "hist",
    "compgen",
    "bind",
    "enum",
    "global",
    "compound",
    "unhash",
    "cap",
    "getcap",
    "setcap",
];

/// Words before a command that still run it in this shell; `exec` runs it
/// in place of the shell, as a program.
#[cfg(test)]
const PREFIXES: &[&str] = &[
    "command",
    "builtin",
    "time",
    "noglob",
    "nocorrect",
    "-",
    "exec",
];

/// The declaration builtins, whose assignments the reader parses from the
/// words as written.
const DECLARATIONS: &[&str] = &["export", "readonly", "local", "declare", "typeset"];

/// Builtins that print nothing.
const SILENT_BUILTINS: &[&str] = &[
    ":",
    "true",
    "false",
    "cd",
    "chdir",
    "unset",
    "unfunction",
    "shift",
    "read",
    "mapfile",
    "readarray",
    "getopts",
    "let",
    "test",
    "[",
    "break",
    "continue",
    "return",
    "exit",
    "bye",
    "wait",
    "unalias",
    "logout",
];

/// Builtins that print nothing when given arguments (and list when not).
const SILENT_WITH_ARGS: &[&str] = &[
    "export", "readonly", "local", "declare", "typeset", "set", "alias", "trap", "hash", "umask",
    "shopt", "setopt", "unsetopt",
];

/// `set` option letters that mean an option the reader need not follow in
/// bash and in zsh alike.
const INERT_SET_LETTERS: &str = "euxvnamCfhtpBEH";

/// Shell options (bash `set -o` and `shopt`, zsh `setopt`), written in
/// lower case without `_` or `-`, that change nothing the reader follows
/// or only narrow what a command does.
const INERT_OPTIONS: &[&str] = &[
    "errexit",
    "nounset",
    "unset",
    "xtrace",
    "verbose",
    "noexec",
    "exec",
    "allexport",
    "monitor",
    "noclobber",
    "clobber",
    "pipefail",
    "notify",
    "hashall",
    "hashcmds",
    "braceexpand",
    "errtrace",
    "functrace",
    "history",
    "histexpand",
    "banghist",
    "ignoreeof",
    "interactivecomments",
    "emacs",
    "vi",
    "nolog",
    "onecmd",
    "privileged",
    "errreturn",
    "printexitvalue",
    "localoptions",
    "localtraps",
    "localloops",
    "nomatch",
    "nullglob",
    "cshnullglob",
    "failglob",
    "badpattern",
    "bgnice",
    "checkjobs",
    "checkrunningjobs",
    "hup",
    "huponexit",
    "multios",
    "aliases",
    "expandaliases",
    "glob",
    "beep",
    "correct",
    "correctall",
    "promptsubst",
    "promptbang",
    "promptcr",
    "promptsp",
    "promptpercent",
    "promptvars",
    "login",
    "loginshell",
    "zle",
    "singlelinezle",
    "mailwarning",
    "mailwarn",
    "nocasematch",
    "cdspell",
    "dirspell",
    "cdsilent",
    "pushdsilent",
    "pushdminus",
    "pushdignoredups",
    "autopushd",
    "checkwinsize",
    "histappend",
    "cmdhist",
    "lithist",
    "extquote",
    "sourcepath",
    "hostcomplete",
    "progcomp",
    "progcompalias",
    "completefullquote",
    "direxpand",
    "checkhash",
    "execfail",
    "gnuerrfmt",
    "inheriterrexit",
    "noemptycmdcompletion",
    "shiftverbose",
    "xpgecho",
    "globasciiranges",
    "globskipdots",
    "lastpipe",
    "rcs",
    "globalrcs",
    "warncreateglobal",
    "warnnestedvar",
    "typesetsilent",
    "sourcetrace",
    "evallineno",
    "debugbeforecmd",
    "continueonerror",
    "pathdirs",
    "pathscript",
    "hashdirs",
    "hashlistall",
    "octalzeroes",
    "cbases",
    "forcefloat",
    "multibyte",
    "rematchpcre",
    "shortloops",
    "shortrepeat",
    "shwordsplit",
    "equals",
    "numericglobsort",
    "bareglobqual",
    "autonamedirs",
    "functionargzero",
    "clobberempty",
    "appendcreate",
    "flowcontrol",
    "kshoptionprint",
    "ksharrays",
    "listtypes",
    "automenu",
    "autolist",
    "menucomplete",
    "alwaystoend",
    "completeinword",
    "globcomplete",
    "listambiguous",
    "listbeep",
    "listpacked",
    "listrowsfirst",
    "recexact",
    "histignoredups",
    "histignorespace",
    "histignorealldups",
    "histnostore",
    "histreduceblanks",
    "histsavenodups",
    "histverify",
    "histexpiredupsfirst",
    "histfindnodups",
    "histallowclobber",
    "histbeep",
    "histfcntllock",
    "histlexwords",
    "histnofunctions",
    "histsavebycopy",
    "histsubstpattern",
    "incappendhistory",
    "incappendhistorytime",
    "sharehistory",
    "appendhistory",
    "extendedhistory",
];

/// How the reader treats `name` in command position when it is a builtin.
fn builtin_kind(name: &str) -> Option<Builtin> {
    if MODELLED_BUILTINS.contains(&name) {
        Some(Builtin::Modelled)
    } else if INERT_BUILTINS.contains(&name) {
        Some(Builtin::Inert)
    } else if UNMODELLED_BUILTINS.contains(&name) {
        Some(Builtin::Unmodelled)
    } else {
        None
    }
}

/// Whether an option name changes nothing the reader follows, either way
/// it is set.
fn inert_option(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect();
    INERT_OPTIONS.contains(&name.as_str())
        || name
            .strip_prefix("no")
            .is_some_and(|rest| INERT_OPTIONS.contains(&rest))
}

/// What a trap action or hook function is set under: the signal a `trap`
/// names, as written, or the function's name. The two are kept apart, so
/// `trap - DEBUG` leaves a `TRAPDEBUG` function.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    Signal(String),
    Function(String),
}

/// Signals every shell the reader models knows, so a `trap` naming only
/// these handles each one.
const TRAP_SIGNALS: &[&str] = &[
    "EXIT", "DEBUG", "HUP", "INT", "QUIT", "TERM", "USR1", "USR2", "ALRM", "PIPE", "CHLD", "WINCH",
];

/// Whether a `trap` surely handles each of `signals`: each is resolved,
/// and either it is the only one or every one is a signal each shell
/// knows, since zsh stops at the first it does not.
fn resets_each(signals: &[String]) -> bool {
    signals
        .iter()
        .all(|signal| unproven(signal).is_none() && !spelled(signal))
        && (signals.len() == 1
            || signals
                .iter()
                .all(|signal| TRAP_SIGNALS.contains(&signal.as_str())))
}

/// A function the shell runs on its own between commands: a zsh trap
/// function (`TRAPEXIT`) or hook (`chpwd`), or a handler for a command
/// that is not found.
fn is_hook(name: &str) -> bool {
    name.strip_prefix("TRAP").is_some_and(|signal| {
        !signal.is_empty()
            && signal
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    }) || matches!(
        name,
        "chpwd"
            | "precmd"
            | "preexec"
            | "periodic"
            | "zshexit"
            | "zshaddhistory"
            | "command_not_found_handler"
            | "command_not_found_handle"
    )
}

/// The text of a word that is literal: no expansion, and no glob, brace
/// or zsh `=` expansion in its unquoted text.
fn literal_text(word: &Word) -> Option<String> {
    let mut text = String::new();
    for (at, part) in word.parts.iter().enumerate() {
        let Part::Lit { text: t, quoted } = part else {
            return None;
        };
        if !quoted && (t.contains(['*', '?', '[', '{']) || (at == 0 && t.starts_with('='))) {
            return None;
        }
        text.push_str(t);
    }
    Some(text)
}

/// zsh's assignment to a positional parameter, `1=…` or `2[…]=…`.
fn positional_assignment(text: &str) -> bool {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && text[digits..].starts_with(['=', '[', '+'])
}

/// A directory-stack entry, `+N` or `-N`.
fn stack_entry(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with(['+', '-']) && arg[1..].chars().all(|c| c.is_ascii_digit())
}

/// Whether an arithmetic value is a number, which evaluates to itself.
/// The attribute letters of a declaration, and where its operands start.
fn declaration_flags(words: &[Word]) -> (usize, String) {
    let mut at = 1;
    let mut flags = String::new();
    while let Some(text) = words.get(at).and_then(Word::plain) {
        if text == "--" {
            return (at + 1, flags);
        }
        match text.strip_prefix(['-', '+']) {
            Some(f) if !f.is_empty() => {
                flags.push_str(f);
                at += 1;
            }
            _ => break,
        }
    }
    (at, flags)
}

/// A value an integer variable takes without evaluating anything: a
/// number, or nothing (which reads as zero).
fn integer_literal(text: &str) -> bool {
    let text = text.trim_matches(crate::hooks::git_guard::shell_blank);
    text.is_empty() || is_number(text)
}

fn is_number(text: &str) -> bool {
    let text = text.trim_start_matches(['-', '+']);
    let (base, digits) = match text.split_once('#') {
        Some((base, digits)) => (base, digits),
        None => ("", text),
    };
    base.chars().all(|c| c.is_ascii_digit())
        && !digits.is_empty()
        && digits
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@'))
        && text.starts_with(|c: char| c.is_ascii_digit())
}

/// A word read from text as the inside of a double-quoted string or of
/// `${…}`: its expansions, without splitting.
fn lex_word(text: &str) -> Word {
    let mut lexer = Lexer::new(text);
    let mut word = Word::default();
    while lexer.peek(0).is_some() {
        lexer.word_char(&mut word, false);
    }
    word
}

/// The words before a command that still run it in this shell (`command`,
/// `builtin`, `time`, zsh's `noglob`, `nocorrect` and `-`), counted in its
/// argument vector, and whether a function may still be called; `None` for
/// `command -v`, which only prints. A `builtin` with an option (ksh's
/// `builtin -f`, which loads one) is the builtin itself.
fn builtin_prefix(argv: &[String]) -> Option<(usize, bool)> {
    let mut at = 0;
    let mut functions = true;
    loop {
        match argv.get(at).map(String::as_str) {
            Some("builtin") => {
                if argv
                    .get(at + 1)
                    .is_some_and(|a| a.len() > 1 && a.starts_with('-'))
                {
                    return Some((at, false));
                }
                at += 1;
                functions = false;
            }
            Some("command") => {
                at += 1;
                functions = false;
                while let Some(option) = argv.get(at).filter(|o| o.len() > 1 && o.starts_with('-'))
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
                if argv.get(at).map(String::as_str) == Some("-p") {
                    at += 1;
                }
            }
            Some("noglob" | "nocorrect" | "-") => at += 1,
            _ => return Some((at, functions)),
        }
    }
}

/// The fields `read` gives `count` names from `line`: each name a field,
/// the last the rest of the line (with and without one trailing
/// delimiter, which bash drops).
fn read_fields(line: &str, ifs: &str, count: usize) -> Vec<Values> {
    let space = |c: char| ifs.contains(c) && crate::hooks::git_guard::shell_blank(c);
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
                    Some(c)
                        if delim(c)
                            && !crate::hooks::git_guard::shell_blank(c)
                            && rest[end..].starts_with(c) =>
                    {
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
                .split([' ', '\t', '\n', '\r', '\u{b}', '\u{c}', '\0'])
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

/// `env`'s own options and assignments, up to the command it runs. A short
/// option's value may be attached (`-S'…'`, `-C/`) or in the next word,
/// and options may be clustered (`-iS…`); `-S` splits its string into the
/// command. An option `env` does not know makes it fail, running nothing.
fn unwrap_env(
    rest: &[String],
    env: &mut Vec<(String, String)>,
    cwd: &mut Option<String>,
) -> Vec<String> {
    let mut at = 0;
    let mut split: Option<Vec<String>> = None;
    while let Some(a) = rest.get(at) {
        at += 1;
        if a == "--" {
            break;
        }
        if let Some((name, value)) = a.split_once('=').filter(|(n, _)| is_name(n)) {
            env.push((name.to_string(), value.to_string()));
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (option, attached) = match long.split_once('=') {
                Some((option, value)) => (option, Some(value.to_string())),
                None => (long, None),
            };
            let mut value = || {
                attached.clone().or_else(|| {
                    let next = rest.get(at).cloned();
                    at += 1;
                    next
                })
            };
            match option {
                "unset" => {
                    value();
                }
                "chdir" => *cwd = value(),
                "split-string" => {
                    split = Some(split_string(&value().unwrap_or_default()));
                    break;
                }
                "ignore-environment"
                | "null"
                | "debug"
                | "list-signal-handling"
                | "block-signal"
                | "default-signal"
                | "ignore-signal" => {}
                _ => return Vec::new(),
            }
            continue;
        }
        let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty()) else {
            at -= 1;
            break;
        };
        let flags: Vec<char> = flags.chars().collect();
        for (i, flag) in flags.iter().enumerate() {
            match flag {
                'i' | '0' | 'v' => {}
                'u' | 'C' | 'S' | 'P' => {
                    let attached: String = flags[i + 1..].iter().collect();
                    let value = if attached.is_empty() {
                        let next = rest.get(at).cloned().unwrap_or_default();
                        at += 1;
                        next
                    } else {
                        attached
                    };
                    match flag {
                        'C' => *cwd = Some(value),
                        'S' => split = Some(split_string(&value)),
                        _ => {}
                    }
                    break;
                }
                _ => return Vec::new(),
            }
        }
        if split.is_some() {
            break;
        }
    }
    let tail = rest.get(at..).unwrap_or_default().iter().cloned();
    match split {
        Some(mut words) => {
            words.extend(tail);
            words
        }
        None => tail.collect(),
    }
}

/// `env -S`'s string split into words: blanks separate them, and single
/// and double quotes and backslashes quote.
fn split_string(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"') | None, '\\') => {
                if let Some(next) = chars.next() {
                    current.get_or_insert_with(String::new).push(match next {
                        'n' => '\n',
                        't' => '\t',
                        '_' => ' ',
                        other => other,
                    });
                }
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                current.get_or_insert_with(String::new);
            }
            (None, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}') => {
                if let Some(word) = current.take() {
                    words.push(word);
                }
            }
            (_, c) => current.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(current);
    words
}

/// `sudo`'s (or `doas`'s) options, up to the command it runs. A value may
/// be attached (`-D/`, `-uroot`) and flags clustered (`-nD /`); a login
/// shell (`-i`) starts in the target user's home, which is unknown here.
fn unwrap_privilege(rest: &[String], cwd: &mut Option<String>) -> Vec<String> {
    const WITH_VALUE: &str = "CDRTUghprtuc";
    let mut at = 0;
    while let Some(a) = rest.get(at) {
        at += 1;
        if a == "--" {
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (option, attached) = match long.split_once('=') {
                Some((option, value)) => (option, Some(value.to_string())),
                None => (long, None),
            };
            let takes = matches!(
                option,
                "chdir"
                    | "chroot"
                    | "user"
                    | "group"
                    | "close-from"
                    | "host"
                    | "prompt"
                    | "role"
                    | "type"
                    | "command-timeout"
                    | "other-user"
            );
            let value = if takes && attached.is_none() {
                let next = rest.get(at).cloned();
                at += 1;
                next
            } else {
                attached
            };
            match option {
                "chdir" => *cwd = value,
                "login" => *cwd = Some(taint(why::CD)),
                _ => {}
            }
            continue;
        }
        let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty()) else {
            at -= 1;
            break;
        };
        let flags: Vec<char> = flags.chars().collect();
        for (i, flag) in flags.iter().enumerate() {
            if *flag == 'i' {
                *cwd = Some(taint(why::CD));
            }
            if WITH_VALUE.contains(*flag) {
                let attached: String = flags[i + 1..].iter().collect();
                let value = if attached.is_empty() {
                    let next = rest.get(at).cloned().unwrap_or_default();
                    at += 1;
                    next
                } else {
                    attached
                };
                if *flag == 'D' {
                    *cwd = Some(value);
                }
                break;
            }
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

fn collect_simples(node: &Node, out: &mut Vec<(Vec<Word>, Vec<Redir>)>) {
    match node {
        Node::Simple(words, redirs) => out.push((words.clone(), redirs.clone())),
        Node::Sub(body)
        | Node::Not(body)
        | Node::Func(_, body)
        | Node::For(_, _, body)
        | Node::Select(_, _, body)
        | Node::Anon(body, _)
        | Node::Coproc(_, body)
        | Node::Unknown(_, body)
        | Node::Redirected(body, _) => {
            collect_simples(body, out);
        }
        Node::Seq(items) | Node::AndOr(items, _) | Node::Pipe(items) | Node::Case(_, items) => {
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
        Node::Cond(_) | Node::Arith(_) => {}
    }
}

/// Whether `node` holds anything whose effect the loop checks below
/// cannot see: text the parser does not read exactly, arithmetic or a
/// test (which may assign), a coprocess, or an anonymous function.
fn opaque_effects(node: &Node) -> bool {
    match node {
        Node::Unknown(..) | Node::Arith(_) | Node::Cond(_) | Node::Anon(..) => true,
        Node::Simple(..) | Node::Func(..) => false,
        Node::Sub(body)
        | Node::Not(body)
        | Node::For(_, _, body)
        | Node::Select(_, _, body)
        | Node::Coproc(_, body)
        | Node::Redirected(body, _) => opaque_effects(body),
        Node::Seq(items) | Node::AndOr(items, _) | Node::Pipe(items) | Node::Case(_, items) => {
            items.iter().any(opaque_effects)
        }
        Node::If(arms, otherwise) => {
            arms.iter()
                .any(|(cond, body)| opaque_effects(cond) || opaque_effects(body))
                || otherwise.as_deref().is_some_and(opaque_effects)
        }
        Node::Loop(cond, body) => opaque_effects(cond) || opaque_effects(body),
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
    opaque_effects(body)
        || simples.iter().any(|(words, _)| {
            words
                .iter()
                .any(|w| matches!(w.plain(), Some("break" | "return")))
                || command_word(words).is_some_and(|word| match literal_text(word) {
                    Some(name) => {
                        matches!(
                            name.as_str(),
                            "eval" | "source" | "." | "command" | "builtin"
                        ) || st.funcs.contains_key(&name)
                            || st.aliases.contains_key(&name)
                    }
                    None => true,
                })
        })
}

/// Whether a loop body may set `name`, or runs something the reader cannot
/// see into (`eval`, `source`, a function or alias, a command it cannot
/// name, arithmetic).
fn may_assign(body: &Node, name: &str, st: &State) -> bool {
    let mut simples = Vec::new();
    collect_simples(body, &mut simples);
    let mut nested = false;
    walk_for(body, &mut |var| nested |= var == name);
    nested
        || opaque_effects(body)
        || matches!(body, Node::Func(..))
        || simples.iter().any(|(words, _)| {
            command_word(words).is_some_and(|word| {
                literal_text(word)
                    .is_none_or(|w| st.funcs.contains_key(&w) || st.aliases.contains_key(&w))
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
                                    | "let"
                            )
                    })
            })
        })
}

/// Whether `${NAME:=…}` or `${NAME=…}` for `name`, or arithmetic or a
/// subscript that may assign, is among `parts`.
fn assigns_by_expansion(parts: &[Part], name: &str) -> bool {
    parts.iter().any(|part| match part {
        Part::Param {
            name: n, op, index, ..
        } => {
            index.is_some()
                || match op {
                    Some(ParamOp::Default { word, assign }) => {
                        (*assign && n == name) || assigns_by_expansion(word, name)
                    }
                    Some(ParamOp::Alternate(word)) => assigns_by_expansion(word, name),
                    _ => false,
                }
        }
        Part::Arith { .. } | Part::Opaque { .. } => true,
        Part::Array(words) => words.iter().any(|w| assigns_by_expansion(&w.parts, name)),
        Part::Lit { .. } | Part::Tilde(_) | Part::Subst { .. } => false,
    })
}

fn walk_for(node: &Node, visit: &mut impl FnMut(&str)) {
    match node {
        Node::For(var, _, body) | Node::Select(var, _, body) => {
            visit(var);
            walk_for(body, visit);
        }
        Node::Sub(body)
        | Node::Not(body)
        | Node::Func(_, body)
        | Node::Anon(body, _)
        | Node::Coproc(_, body)
        | Node::Unknown(_, body)
        | Node::Redirected(body, _) => walk_for(body, visit),
        Node::Seq(items) | Node::AndOr(items, _) | Node::Pipe(items) | Node::Case(_, items) => {
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
        Node::Simple(..) | Node::Cond(_) | Node::Arith(_) => {}
    }
}

/// `printf FORMAT ARGS` for formats of text, escapes, `%s`, `%b`, `%c`
/// and `%%`; `None` for any other directive.
fn printf(args: &[String]) -> Option<String> {
    let args = match args.first().map(String::as_str) {
        Some("--") => &args[1..],
        _ => args,
    };
    let (format, mut rest) = args.split_first()?;
    let format: Vec<char> = format.chars().collect();
    let mut out = String::new();
    loop {
        let mut used = false;
        let mut i = 0;
        while i < format.len() {
            match format[i] {
                '\\' => {
                    let (text, taken, stop) = unescape_one(&format[i + 1..], false);
                    out.push_str(&text);
                    if stop {
                        return Some(out);
                    }
                    i += 1 + taken;
                }
                '%' => {
                    let directive = format.get(i + 1).copied();
                    i += 2;
                    if directive == Some('%') {
                        out.push('%');
                        continue;
                    }
                    used = true;
                    let arg = rest.first().map_or("", String::as_str);
                    rest = rest.get(1..).unwrap_or_default();
                    match directive {
                        Some('s') => out.push_str(arg),
                        Some('c') => out.extend(arg.chars().next()),
                        Some('b') => {
                            let (text, stop) = unescape(arg, true);
                            out.push_str(&text);
                            if stop {
                                return Some(out);
                            }
                        }
                        _ => return None,
                    }
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        if !used || rest.is_empty() {
            break;
        }
    }
    Some(out)
}

/// What `echo` with `args` may print. `echo -e`, zsh's `echo` and a POSIX
/// `echo` read backslash escapes and bash's `echo` does not by default, so
/// both readings are kept.
fn echo_output(args: &[String]) -> Values {
    let mut newline = true;
    let mut at = 0;
    while let Some(flags) = args.get(at).and_then(|a| a.strip_prefix('-')) {
        if flags.is_empty() || !flags.chars().all(|c| matches!(c, 'n' | 'e' | 'E')) {
            break;
        }
        newline &= !flags.contains('n');
        at += 1;
    }
    let text = args[at..].join(" ");
    let end = if newline { "\n" } else { "" };
    let mut out: Values = [format!("{text}{end}")].into();
    if text.contains('\\') {
        let (decoded, stop) = unescape(&text, true);
        out.insert(if stop {
            decoded
        } else {
            format!("{decoded}{end}")
        });
    }
    out
}

/// What zsh's `print` with `args` prints, for its options `-r`, `-n`,
/// `-l` and `-N`; `None` for any other option.
fn print_output(args: &[String]) -> Option<Values> {
    let (mut raw, mut newline, mut separator) = (false, true, " ");
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "-" || arg == "--" {
            at += 1;
            break;
        }
        let Some(flags) = arg.strip_prefix('-').filter(|f| !f.is_empty()) else {
            break;
        };
        for flag in flags.chars() {
            match flag {
                'r' => raw = true,
                'n' => newline = false,
                'l' => separator = "\n",
                'N' => separator = "\0",
                _ => return None,
            }
        }
        at += 1;
    }
    let text = args[at..].join(separator);
    let end = if newline { "\n" } else { "" };
    let mut out: Values = [format!("{text}{end}")].into();
    if !raw && text.contains('\\') {
        let (decoded, stop) = unescape(&text, true);
        out.insert(if stop {
            decoded
        } else {
            format!("{decoded}{end}")
        });
    }
    Some(out)
}

/// `text` with its backslash escapes decoded, and whether `\c` stopped
/// the output. `echo` and `%b` write octal as `\0NNN`; a `printf` format
/// writes it as `\NNN`.
fn unescape(text: &str, echo: bool) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let (decoded, taken, stop) = unescape_one(&chars[i + 1..], echo);
            out.push_str(&decoded);
            if stop {
                return (out, true);
            }
            i += 1 + taken;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    (out, false)
}

/// One escape after its backslash: its text, how many characters it
/// takes, and whether it is `\c`, which ends the output.
fn unescape_one(chars: &[char], echo: bool) -> (String, usize, bool) {
    let Some(&c) = chars.first() else {
        return ("\\".to_string(), 0, false);
    };
    let digits = |radix: u32, max: usize, from: usize| -> (u32, usize) {
        let mut value = 0u32;
        let mut n = 0;
        while n < max {
            match chars.get(from + n).and_then(|d| d.to_digit(radix)) {
                Some(d) => {
                    value = value.saturating_mul(radix).saturating_add(d);
                    n += 1;
                }
                None => break,
            }
        }
        (value, n)
    };
    let code = |value: u32| char::from_u32(value).map(String::from).unwrap_or_default();
    match c {
        'a' => ("\u{7}".to_string(), 1, false),
        'b' => ("\u{8}".to_string(), 1, false),
        'c' => (String::new(), 1, true),
        'e' | 'E' => ("\u{1b}".to_string(), 1, false),
        'f' => ("\u{c}".to_string(), 1, false),
        'n' => ("\n".to_string(), 1, false),
        'r' => ("\r".to_string(), 1, false),
        't' => ("\t".to_string(), 1, false),
        'v' => ("\u{b}".to_string(), 1, false),
        '\\' => ("\\".to_string(), 1, false),
        '0' if echo => {
            let (value, n) = digits(8, 3, 1);
            (code(value & 0xff), 1 + n, false)
        }
        '0'..='7' if !echo => {
            let (value, n) = digits(8, 3, 0);
            (code(value & 0xff), n, false)
        }
        'x' | 'u' | 'U' => {
            let max = match c {
                'x' => 2,
                'u' => 4,
                _ => 8,
            };
            match digits(16, max, 1) {
                (_, 0) => (format!("\\{c}"), 1, false),
                (value, n) => (code(value), 1 + n, false),
            }
        }
        '"' | '\'' | '?' if !echo => (c.to_string(), 1, false),
        _ => (format!("\\{c}"), 1, false),
    }
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
    real_prefix_checked(path).ok().flatten()
}

fn real_prefix_checked(path: &Path) -> Result<Option<String>, ()> {
    // OS text rule (issue 79): a component that is not valid UTF-8 cannot be
    // placed as text, so the path is unplaced (unproven), never a lossy lookalike.
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(p) => parts.push(p.to_str().ok_or(())?.to_string()),
            std::path::Component::ParentDir => parts.push("..".to_string()),
            _ => {}
        }
    }
    let globbed = parts
        .iter()
        .position(|p| p.contains(['*', '?', '[']))
        .unwrap_or(parts.len());
    for existing in (0..=globbed).rev() {
        let prefix = format!("/{}", parts[..existing].join("/"));
        if std::fs::symlink_metadata(&prefix).is_err() {
            continue;
        }
        let Ok(real) = std::fs::canonicalize(&prefix) else {
            return Ok(None);
        };
        let mut joined = real.to_str().ok_or(())?.to_string();
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
        return Ok(Some(served));
    }
    Ok(None)
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
            // A locale may collate letters of either case into a range
            // (`[a-z]` holding `B`), so letters match it in either case.
            let end = p[i + 2];
            let lower = |x: char| x.to_ascii_lowercase();
            if (ch <= c && c <= end)
                || (ch.is_ascii_alphabetic()
                    && end.is_ascii_alphabetic()
                    && c.is_ascii_alphabetic()
                    && lower(ch) <= lower(c)
                    && lower(c) <= lower(end))
            {
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

// Several guard-form tables feed tests that build symlinks, which run on
// Unix only.
#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    #[cfg(unix)]
    #[test]
    fn r15_unreadable_deletion_base_remains_unproven() {
        use std::os::unix::ffi::OsStrExt;
        let base = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xff"));
        let result = super::composed_deletion_in("rm -rf ./ordinary", Some(base))
            .expect("unreadable base must refuse");
        assert!(result.unproven.is_some());
    }

    #[test]
    fn r15_owned_mapfile_preserves_record_bytes() {
        let mut reader = super::Reader {
            base: None,
            rooted_unplaced: false,
            found: None,
            depth: 0,
            pipe_input: None,
            jumps: Vec::new(),
            expanding: Vec::new(),
            traps: Vec::new(),
            in_trap: false,
            status: None,
        };
        let input = super::Fed {
            complete: true,
            items: vec![super::Input {
                value: "/etc\r\n/tmp\n".into(),
                tree: false,
            }],
        };
        #[cfg(unix)]
        {
            let mut expected = std::env::var_os("HOME").expect("home fixture");
            expected.push("//ordinary");
            assert_eq!(
                reader.absolute("~//ordinary"),
                Some(std::path::PathBuf::from(expected))
            );
        }
        for (options, expected) in [
            (vec!["mapfile", "-t", "a"], vec!["/etc\r", "/tmp"]),
            (vec!["mapfile", "a"], vec!["/etc\r\n", "/tmp\n"]),
        ] {
            let mut state = super::State::start();
            reader.builtin(
                "mapfile",
                &[],
                &options.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
                Some(&input),
                &mut state,
            );
            assert_eq!(
                state.var("a"),
                expected.into_iter().map(str::to_string).collect(),
                "{options:?}"
            );
        }
    }

    #[test]
    fn arithmetic_does_not_erase_unicode_between_name_and_assignment() {
        let mut reader = super::Reader {
            base: None,
            rooted_unplaced: false,
            found: None,
            depth: 0,
            pipe_input: None,
            jumps: Vec::new(),
            expanding: Vec::new(),
            traps: Vec::new(),
            in_trap: false,
            status: None,
        };
        for expression in ["x\u{a0}=1", "x[0]\u{a0}=1", "+\u{a0}+x"] {
            let mut state = super::State::start();
            state.set("x", ["0".into()].into());
            reader.arith_scan(expression, &mut state, 0);
            assert_eq!(state.var("x"), ["0".into()].into(), "{expression}");
        }
    }

    #[test]
    fn unicode_blanks_stay_in_deletion_inputs() {
        let mut lexer = super::Lexer::new("file\u{a0}tail");
        assert_eq!(
            lexer.redirect_target().unwrap().plain(),
            Some("file\u{a0}tail")
        );
        assert!(!super::integer_literal("\u{a0}1"));
        assert_eq!(
            super::split_string("rm file\u{a0}tail"),
            ["rm", "file\u{a0}tail"]
        );
        let fed = super::Fed {
            items: vec![super::Input {
                value: "file\u{a0}tail".into(),
                tree: false,
            }],
            complete: true,
        };
        assert_eq!(super::items(Some(&fed))[0].value, "file\u{a0}tail");
        let fields = super::read_fields("\u{a0}x\u{a0}y", "\u{a0}", 3);
        assert!(fields[0].contains(""));
        assert!(fields[1].contains("x"));
    }
    use super::super::dangerous::DangerousModule;
    use super::super::guard_forms::{
        Expect, COMPOSED_PAIRS, NESTINGS, PROJECT_DELETIONS, REVIEW_PROBES,
        REVIEW_ROUND_FOUR_PROBES, REVIEW_ROUND_THREE_PROBES, REVIEW_ROUND_TWO_PROBES,
        REVIEW_SYMLINK_PROBES,
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
        for command in super::super::guard_forms::ROOTED_DELETIONS {
            assert_eq!(refused(command).is_some(), cfg!(windows), "{command}");
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

    /// Native Windows places a `/`-rooted path nowhere, so a recursive
    /// delete below one is unproven: `C:\scratch` may be a junction to
    /// `C:\`, and `rm -r /scratch/Windows` would reach `C:\Windows`. A
    /// protected spelling or glob stays protected, and a deletion inside the
    /// project is judged as before. Read with the Windows rule on any host.
    #[test]
    fn a_rooted_deletion_is_unproven_where_rooted_paths_are_unplaced() {
        let read = |command: &str| super::read_deletion(command, None, true);
        // Each is refused; a temp path may already be refused as an
        // unresolved temp path where the host cannot place it (Windows).
        for command in super::super::guard_forms::ROOTED_DELETIONS {
            assert!(read(command).is_some(), "{command}");
        }
        for command in ["rm -r /scratch/Windows", "rm -rf /c/Users/me/app/target"] {
            let reason = read(command)
                .and_then(|found| found.unproven)
                .map(|u| u.reason);
            assert_eq!(reason.as_deref(), Some(super::why::ROOTED), "{command}");
        }
        for command in ["rm -rf /", "rm -rf /etc", "rm -rf /et?", "rm -rf /*"] {
            let found = read(command);
            assert!(
                found.as_ref().is_some_and(|f| f.unproven.is_none()),
                "{command}: {found:?}"
            );
        }
        for command in PROJECT_DELETIONS {
            assert!(read(command).is_none(), "{command}");
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

    /// Issue 79: in the C locale `?` matches one byte, so `??` matches the
    /// name `e2 82`, which reads as one replacement character as text. Such a
    /// name is never left out of the glob silently: the deletion is unproven.
    #[cfg(unix)]
    #[test]
    fn a_glob_that_may_match_a_name_that_is_not_utf8_is_unproven() {
        use std::os::unix::ffi::OsStrExt as _;
        let project = tempfile::tempdir().unwrap();
        let odd = project
            .path()
            .join(std::ffi::OsStr::from_bytes(b"\xe2\x82"));
        if std::fs::write(&odd, b"x").is_err() {
            return; // this volume refuses names that are not UTF-8
        }
        let found = composed_deletion_in("rm -rf ??", Some(project.path()));
        assert!(found.is_some_and(|found| found.unproven.is_some()));
        // A literal pattern no such name can match stays proven.
        std::fs::write(project.path().join("cafe"), b"x").unwrap();
        assert_eq!(
            composed_deletion_in("rm -rf cafe*", Some(project.path())),
            None
        );
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

    /// Read each review probe in a project holding `build`, `empty` and a
    /// `root-link` to `/`, and hold it to its expected verdict.
    #[cfg(unix)]
    fn hold_probes(probes: &[(&str, &str, Expect)]) {
        let project = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/", project.path().join("root-link")).unwrap();
        for dir in ["build", "empty"] {
            std::fs::create_dir(project.path().join(dir)).unwrap();
        }
        let mut wrong = Vec::new();
        for (case, command, expect) in probes {
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

    /// Codex round 2 and the taint rule: every probe keeps its verdict.
    #[cfg(unix)]
    #[test]
    fn every_round_two_probe_keeps_its_verdict() {
        hold_probes(REVIEW_ROUND_TWO_PROBES);
    }

    /// Codex round 3 and the closed-world reader's own probes: every probe
    /// keeps its verdict.
    #[cfg(unix)]
    #[test]
    fn every_round_three_probe_keeps_its_verdict() {
        hold_probes(REVIEW_ROUND_THREE_PROBES);
    }

    /// TSK-180: a trap reset or a removed hook takes its action away only
    /// where the reset surely runs and names what the action was set under.
    #[cfg(unix)]
    #[test]
    fn every_round_four_probe_keeps_its_verdict() {
        hold_probes(REVIEW_ROUND_FOUR_PROBES);
    }

    /// The kind of each node the parser builds, and the nodes inside it.
    /// The match has no wildcard arm: a new kind does not compile until it
    /// is named here, and `every_node_kind_is_modelled_or_taints` then
    /// needs a sample that shows it is read exactly or makes the state
    /// unknown.
    fn node_kind(node: &super::Node) -> (&'static str, Vec<&super::Node>) {
        use super::Node;
        match node {
            Node::Simple(..) => ("simple", Vec::new()),
            Node::Sub(inner) => ("subshell", vec![inner.as_ref()]),
            Node::Seq(list) => ("list", list.iter().collect()),
            Node::AndOr(list, _) => ("and-or", list.iter().collect()),
            Node::Not(inner) => ("not", vec![inner.as_ref()]),
            Node::Pipe(stages) => ("pipeline", stages.iter().collect()),
            Node::If(arms, other) => (
                "if",
                arms.iter()
                    .flat_map(|(test, body)| [test, body])
                    .chain(other.as_deref())
                    .collect(),
            ),
            Node::Loop(test, body) => ("loop", vec![test.as_ref(), body.as_ref()]),
            Node::For(_, _, body) => ("for", vec![body.as_ref()]),
            Node::Select(_, _, body) => ("select", vec![body.as_ref()]),
            Node::Case(_, arms) => ("case", arms.iter().collect()),
            Node::Func(_, body) => ("function", vec![body.as_ref()]),
            Node::Anon(body, _) => ("anonymous function", vec![body.as_ref()]),
            Node::Coproc(_, body) => ("coproc", vec![body.as_ref()]),
            Node::Cond(_) => ("conditional", Vec::new()),
            Node::Arith(_) => ("arithmetic", Vec::new()),
            Node::Redirected(inner, _) => ("redirected", vec![inner.as_ref()]),
            Node::Unknown(_, inner) => ("unknown", vec![inner.as_ref()]),
        }
    }

    fn node_kinds(node: &super::Node, kinds: &mut std::collections::BTreeSet<&'static str>) {
        let (kind, inner) = node_kind(node);
        kinds.insert(kind);
        for node in inner {
            node_kinds(node, kinds);
        }
    }

    const NODE_SAMPLES: &[(&str, &str, &str, Expect)] = &[
        ("simple", "simple", "cd /; rm -rf *", Expect::Protected),
        ("list", "list", "D=/; rm -rf \"$D\"", Expect::Protected),
        (
            "subshell",
            "subshell",
            "(cd /; rm -rf *)",
            Expect::Protected,
        ),
        (
            "subshell-isolated",
            "subshell",
            "(cd /); rm -rf build",
            Expect::Allowed,
        ),
        (
            "and-or",
            "and-or",
            "true && cd /; rm -rf *",
            Expect::Protected,
        ),
        ("not", "not", "! cd /; rm -rf *", Expect::Protected),
        (
            "pipeline",
            "pipeline",
            ": | { cd /; rm -rf *; }",
            Expect::Protected,
        ),
        (
            "pipeline-isolated",
            "pipeline",
            "cd / | :; rm -rf build",
            Expect::Allowed,
        ),
        (
            "if",
            "if",
            "if true; then cd /; fi; rm -rf *",
            Expect::Protected,
        ),
        (
            "loop",
            "loop",
            "while true; do cd /; break; done; rm -rf *",
            Expect::Protected,
        ),
        (
            "for",
            "for",
            "for d in /; do cd \"$d\"; done; rm -rf *",
            Expect::Protected,
        ),
        (
            "select",
            "select",
            "select d in /; do cd \"$d\"; break; done; rm -rf *",
            Expect::Protected,
        ),
        (
            "case",
            "case",
            "case x in x) cd / ;; esac; rm -rf *",
            Expect::Protected,
        ),
        (
            "function",
            "function",
            "f(){ cd /; }; f; rm -rf *",
            Expect::Protected,
        ),
        (
            "anonymous function",
            "anonymous function",
            "() { cd /; }; rm -rf *",
            Expect::Protected,
        ),
        (
            "coproc",
            "coproc",
            "coproc { rm -rf /; }",
            Expect::Protected,
        ),
        (
            "coproc-isolated",
            "coproc",
            "coproc { cd /; }; rm -rf build",
            Expect::Allowed,
        ),
        (
            "conditional",
            "conditional",
            "[[ ${D:=/} ]]; rm -rf \"$D\"",
            Expect::Protected,
        ),
        (
            "arithmetic",
            "arithmetic",
            "D=(/ build); i=1; (( i = 0 )); rm -rf \"${D[i]}\"",
            Expect::Protected,
        ),
        (
            "redirected",
            "redirected",
            "{ cd /; } >/dev/null; rm -rf *",
            Expect::Protected,
        ),
        (
            "unknown",
            "unknown",
            "{ D=build }; rm -rf \"$D\"",
            Expect::Unproven,
        ),
    ];

    /// The closed world, for structure: each node kind has a sample whose
    /// verdict shows the reader follows what the construct does (a `cd`
    /// or an assignment inside it reaches the deletion after it, a child
    /// shell's does not, a deletion inside it is seen), or, for text
    /// outside the grammar, that the state is unknown after it.
    #[cfg(unix)]
    #[test]
    fn every_node_kind_is_modelled_or_taints() {
        let mut seen = std::collections::BTreeSet::new();
        for (case, kind, command, _) in NODE_SAMPLES {
            let mut kinds = std::collections::BTreeSet::new();
            node_kinds(&super::Parser::parse(command), &mut kinds);
            assert!(kinds.contains(kind), "{case}: {command} builds {kinds:?}");
            seen.insert(*kind);
        }
        let all = [
            "simple",
            "list",
            "subshell",
            "and-or",
            "not",
            "pipeline",
            "if",
            "loop",
            "for",
            "select",
            "case",
            "function",
            "anonymous function",
            "coproc",
            "conditional",
            "arithmetic",
            "redirected",
            "unknown",
        ];
        for kind in all {
            assert!(seen.contains(kind), "no sample for the {kind} node");
        }
        let probes: Vec<(&str, &str, Expect)> = NODE_SAMPLES
            .iter()
            .map(|(case, _, command, expect)| (*case, *command, *expect))
            .collect();
        hold_probes(&probes);
    }

    /// The kind of each piece of a word. Like [`node_kind`], it has no
    /// wildcard arm.
    fn part_kind(part: &super::Part) -> &'static str {
        use super::Part;
        match part {
            Part::Lit { .. } => "literal",
            Part::Tilde(_) => "tilde",
            Part::Param { .. } => "parameter",
            Part::Arith { .. } => "arithmetic",
            Part::Subst { .. } => "substitution",
            Part::Array(_) => "array",
            Part::Opaque { .. } => "opaque",
        }
    }

    const PART_SAMPLES: &[(&str, &str, Expect)] = &[
        ("literal", "rm -rf /", Expect::Protected),
        ("tilde", "rm -rf ~", Expect::Protected),
        ("parameter", "D=/; rm -rf \"$D\"", Expect::Protected),
        (
            "arithmetic",
            "D=(/ build); i=1; : $((i=0)); rm -rf \"${D[i]}\"",
            Expect::Protected,
        ),
        (
            "substitution",
            "D=$(echo /); rm -rf \"$D\"",
            Expect::Protected,
        ),
        ("array", "D=(/); rm -rf \"${D[@]}\"", Expect::Protected),
        ("opaque", "N=D; rm -rf \"${!N}\"", Expect::Unproven),
    ];

    /// The closed world, for words: each kind of word piece has a sample
    /// word, and a line using it shows the reader resolves it exactly or
    /// refuses what depends on it as unproven.
    #[cfg(unix)]
    #[test]
    fn every_word_part_is_modelled_or_taints() {
        let mut seen = std::collections::BTreeSet::new();
        for (kind, command, _) in PART_SAMPLES {
            let mut simples = Vec::new();
            super::collect_simples(&super::Parser::parse(command), &mut simples);
            let kinds: Vec<&str> = simples
                .iter()
                .flat_map(|(words, _)| words.iter())
                .flat_map(|word| word.parts.iter().map(part_kind))
                .collect();
            assert!(kinds.contains(kind), "{command} holds {kinds:?}");
            seen.insert(*kind);
        }
        for kind in [
            "literal",
            "tilde",
            "parameter",
            "arithmetic",
            "substitution",
            "array",
            "opaque",
        ] {
            assert!(seen.contains(kind), "no sample for the {kind} part");
        }
        hold_probes(PART_SAMPLES);
    }

    const BASH_BUILTINS: &[&str] = &[
        ".",
        ":",
        "[",
        "alias",
        "bg",
        "bind",
        "break",
        "builtin",
        "caller",
        "cd",
        "command",
        "compgen",
        "complete",
        "compopt",
        "continue",
        "declare",
        "dirs",
        "disown",
        "echo",
        "enable",
        "eval",
        "exec",
        "exit",
        "export",
        "false",
        "fc",
        "fg",
        "getopts",
        "hash",
        "help",
        "history",
        "jobs",
        "kill",
        "let",
        "local",
        "logout",
        "mapfile",
        "popd",
        "printf",
        "pushd",
        "pwd",
        "read",
        "readarray",
        "readonly",
        "return",
        "set",
        "shift",
        "shopt",
        "source",
        "suspend",
        "test",
        "times",
        "trap",
        "true",
        "type",
        "typeset",
        "ulimit",
        "umask",
        "unalias",
        "unset",
        "wait",
    ];

    const DASH_BUILTINS: &[&str] = &[
        ".", ":", "[", "alias", "bg", "break", "cd", "chdir", "command", "continue", "echo",
        "eval", "exec", "exit", "export", "false", "fc", "fg", "getopts", "hash", "jobs", "kill",
        "local", "printf", "pwd", "read", "readonly", "return", "set", "shift", "test", "times",
        "trap", "true", "type", "ulimit", "umask", "unalias", "unset", "wait",
    ];

    const KSH_BUILTINS: &[&str] = &[
        ".",
        ":",
        "[",
        "alias",
        "autoload",
        "bg",
        "break",
        "builtin",
        "cd",
        "command",
        "compound",
        "continue",
        "disown",
        "echo",
        "enum",
        "eval",
        "exec",
        "exit",
        "export",
        "false",
        "fc",
        "fg",
        "float",
        "functions",
        "getconf",
        "getopts",
        "hist",
        "history",
        "integer",
        "jobs",
        "kill",
        "let",
        "nameref",
        "newgrp",
        "print",
        "printf",
        "pwd",
        "r",
        "read",
        "readonly",
        "return",
        "set",
        "shift",
        "sleep",
        "source",
        "suspend",
        "test",
        "times",
        "trap",
        "true",
        "type",
        "typeset",
        "ulimit",
        "umask",
        "unalias",
        "unset",
        "wait",
        "whence",
    ];

    const MKSH_BUILTINS: &[&str] = &[
        "bind", "cat", "global", "realpath", "rename", "mknod", "sleep", "suspend", "whence",
    ];

    const ZSH_BUILTINS: &[&str] = &[
        "-",
        ".",
        ":",
        "[",
        "alias",
        "autoload",
        "bg",
        "bindkey",
        "break",
        "builtin",
        "bye",
        "cap",
        "cd",
        "chdir",
        "clone",
        "command",
        "comparguments",
        "compcall",
        "compctl",
        "compdescribe",
        "compfiles",
        "compgroups",
        "compquote",
        "comptags",
        "comptry",
        "compvalues",
        "continue",
        "declare",
        "dirs",
        "disable",
        "disown",
        "echo",
        "echotc",
        "echoti",
        "emulate",
        "enable",
        "eval",
        "exec",
        "exit",
        "export",
        "false",
        "fc",
        "fg",
        "float",
        "functions",
        "getcap",
        "getln",
        "getopts",
        "hash",
        "history",
        "integer",
        "jobs",
        "kill",
        "let",
        "limit",
        "local",
        "logout",
        "noglob",
        "popd",
        "print",
        "printf",
        "pushd",
        "pushln",
        "pwd",
        "r",
        "read",
        "readonly",
        "rehash",
        "return",
        "sched",
        "set",
        "setcap",
        "setopt",
        "shift",
        "source",
        "suspend",
        "test",
        "times",
        "trap",
        "true",
        "ttyctl",
        "type",
        "typeset",
        "ulimit",
        "umask",
        "unalias",
        "unfunction",
        "unhash",
        "unlimit",
        "unset",
        "unsetopt",
        "vared",
        "wait",
        "whence",
        "where",
        "which",
        "zcompile",
        "zformat",
        "zftp",
        "zle",
        "zmodload",
        "zparseopts",
        "zprof",
        "zpty",
        "zregexparse",
        "zsocket",
        "zstyle",
        "ztcp",
    ];

    /// The closed world, for command names: every builtin of bash, dash,
    /// ksh93, mksh and zsh (from their manuals) is in exactly one of the
    /// reader's tables, or is a prefix word that runs the next one.
    #[test]
    fn every_shell_builtin_is_classified_once() {
        use super::{INERT_BUILTINS, MODELLED_BUILTINS, PREFIXES, UNMODELLED_BUILTINS};
        let tables = [MODELLED_BUILTINS, INERT_BUILTINS, UNMODELLED_BUILTINS];
        for (i, a) in tables.iter().enumerate() {
            for b in &tables[i + 1..] {
                let both: Vec<&&str> = a.iter().filter(|name| b.contains(name)).collect();
                assert!(both.is_empty(), "in two tables: {both:?}");
            }
        }
        let mut missing = Vec::new();
        for name in [
            BASH_BUILTINS,
            DASH_BUILTINS,
            KSH_BUILTINS,
            MKSH_BUILTINS,
            ZSH_BUILTINS,
        ]
        .concat()
        {
            let classified = tables.iter().any(|table| table.contains(&name));
            if !classified && !PREFIXES.contains(&name) && !missing.contains(&name) {
                missing.push(name);
            }
        }
        assert!(missing.is_empty(), "builtins in no table: {missing:?}");
    }

    /// Each builtin the reader does not model makes a later relative
    /// deletion unproven, and each name whose assignment changes how the
    /// shell runs does too.
    #[cfg(unix)]
    #[test]
    fn an_unmodelled_builtin_or_special_name_taints_what_follows() {
        use super::{OUT_OF_PLACE, SPECIAL_READS, SPECIAL_WRITES, UNMODELLED_BUILTINS};
        let mut probes: Vec<(String, String, Expect)> = Vec::new();
        // `builtin NAME` with no option runs NAME, and fails for a
        // non-builtin; its option forms are below.
        for name in UNMODELLED_BUILTINS
            .iter()
            .filter(|name| **name != "builtin")
        {
            probes.push((
                format!("builtin {name}"),
                format!("{name} x; rm -rf build"),
                Expect::Unproven,
            ));
        }
        for name in SPECIAL_WRITES {
            probes.push((
                format!("write {name}"),
                format!("{name}=x; rm -rf build"),
                Expect::Unproven,
            ));
        }
        for name in SPECIAL_READS {
            probes.push((
                format!("read {name}"),
                format!("rm -rf \"${name}\""),
                Expect::Unproven,
            ));
        }
        for word in OUT_OF_PLACE {
            probes.push((
                format!("reserved {word}"),
                format!("{word}; rm -rf build"),
                Expect::Unproven,
            ));
        }
        for command in [
            "$X; rm -rf build",
            "builtin -f ./x.so cd; rm -rf build",
            "trap \"$T\" EXIT; rm -rf build",
        ] {
            probes.push((command.to_string(), command.to_string(), Expect::Unproven));
        }
        let probes: Vec<(&str, &str, Expect)> = probes
            .iter()
            .map(|(case, command, expect)| (case.as_str(), command.as_str(), *expect))
            .collect();
        hold_probes(&probes);
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

    /// Issue 79: a path with a component that is not valid UTF-8 cannot be
    /// placed as text, so the reader does not place it (the deletion stays
    /// unproven) instead of reading a lossy lookalike.
    #[cfg(unix)]
    #[test]
    fn a_path_with_a_component_that_is_not_utf8_is_not_placed() {
        use std::os::unix::ffi::OsStrExt as _;
        let odd = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9/x"));
        assert!(super::real_prefix(odd).is_none());
        assert!(super::real_prefix(std::path::Path::new("/tmp/cafe/x")).is_some());
    }
}
