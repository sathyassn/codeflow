//! One shell lexer for the directory checks of TSK-397.
//!
//! The exec guard cannot see where a shell command leaves the shell, so it
//! must know, for every word on a line, whether the shell will read it as a
//! command, an assignment, a reserved word or an operand, and whether any
//! part of it is filled in at run time. Reading words after their quotes
//! are removed, or matching spellings in the raw text, let quoted command
//! words, ANSI-C quotes and new spellings of the working directory through
//! (TSK-397 Codex review round 8, security review round 4). This module
//! lexes a line once, with the shell's own lexical states: unquoted text,
//! single quotes, double quotes, ANSI-C `$'...'`, locale `$"..."`, command
//! substitution `$(...)`, arithmetic `$((...))`, backquotes, parameter
//! expansion `${...}`, the current-shell substitution `${ cmd; }` and
//! here-document bodies. Backslash-newline is removed where the shell
//! removes it. A construct it does not model in full (an unbalanced quote
//! or substitution, a here-document without its end line, legacy `$[...]`)
//! makes the line opaque, and every consumer then treats the directory as
//! unknown.

/// One word as the shell's lexer reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Token {
    /// The word with quotes and escapes removed and expansions kept as
    /// written; only meaningful for a word with no quoting or expansion.
    pub(crate) text: String,
    /// Any part of the word was quoted or escaped.
    pub(crate) quoted: bool,
    /// How many leading characters of `text` were plain: unquoted,
    /// unescaped and not an expansion.
    pub(crate) plain_prefix: usize,
    /// The names of plain parameter expansions in the word (`$NAME`,
    /// `${NAME}`).
    pub(crate) params: Vec<String>,
    /// Any other expansion: a command or arithmetic substitution, a
    /// backquote, a special parameter or a `${...}` with an operator.
    pub(crate) complex: bool,
    /// The word's tilde prefix (`~`, `~+`, `~user`), when it starts with
    /// an unquoted `~`.
    pub(crate) tilde: Option<String>,
    /// A redirection operator (`>`, `2>>`, `<<<`, `&>`).
    pub(crate) redirect: bool,
}

impl Token {
    /// Whether the whole word is plain text: no quoting and no expansion.
    pub(crate) fn plain(&self) -> bool {
        !self.quoted
            && !self.complex
            && self.params.is_empty()
            && self.plain_prefix == self.text.len()
    }

    /// The assigned name, when the word is an assignment the shell
    /// recognizes: an unquoted identifier (a letter or `_` first), then
    /// `=` or `+=`, all within the plain prefix.
    pub(crate) fn assignment(&self) -> Option<&str> {
        let plain = &self.text[..self.plain_prefix];
        let (name, _) = plain.split_once('=')?;
        let name = name.strip_suffix('+').unwrap_or(name);
        let mut chars = name.chars();
        let first = chars.next()?;
        (first.is_ascii_alphabetic() || first == '_')
            .then_some(())
            .filter(|()| chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .map(|()| name)
    }
}

/// A lexed line.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lexed {
    /// Simple commands in order, each a list of words.
    pub(crate) commands: Vec<Vec<Token>>,
    /// The line with every backslash-newline the shell removes removed.
    pub(crate) text: String,
    /// The lexer met a construct it does not model in full.
    pub(crate) opaque: bool,
    /// The line holds a current-shell substitution (`${ cmd; }`,
    /// `${| cmd; }`), anywhere: in a word, a quote, an assignment, a
    /// redirection or a here-document.
    pub(crate) current_shell: bool,
}

/// Lex `line` with the shell's lexical states.
pub(crate) fn lex(line: &str) -> Lexed {
    let mut lexer = Lexer {
        chars: line.chars().collect(),
        at: 0,
        removed: Vec::new(),
        opaque: false,
        current_shell: false,
        heredocs: Vec::new(),
    };
    let commands = lexer.list(None);
    let text = lexer
        .chars
        .iter()
        .enumerate()
        .filter(|(at, _)| !lexer.removed.contains(at))
        .map(|(_, c)| c)
        .collect();
    Lexed {
        commands,
        text,
        opaque: lexer.opaque,
        current_shell: lexer.current_shell,
    }
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    /// Positions of backslash-newline pairs the shell removes.
    removed: Vec<usize>,
    opaque: bool,
    current_shell: bool,
    /// Here-documents waiting for their body: delimiter, whether the body
    /// expands, whether leading tabs are stripped.
    heredocs: Vec<(String, bool, bool)>,
}

impl Lexer {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Lex commands until `closer` (unquoted, at this level) or the end.
    #[allow(clippy::too_many_lines)] // One state machine over the shell's operators.
    fn list(&mut self, closer: Option<char>) -> Vec<Vec<Token>> {
        let mut commands = Vec::new();
        let mut words: Vec<Token> = Vec::new();
        let mut word: Option<Token> = None;
        let mut heredoc_next = None;
        let end_word = |word: &mut Option<Token>,
                        words: &mut Vec<Token>,
                        heredoc_next: &mut Option<bool>,
                        heredocs: &mut Vec<(String, bool, bool)>| {
            if let Some(done) = word.take() {
                if let Some(strip) = heredoc_next.take() {
                    heredocs.push((done.text.clone(), !done.quoted, strip));
                }
                words.push(done);
            }
        };
        loop {
            let Some(c) = self.peek(0) else {
                if closer.is_some() {
                    self.opaque = true;
                }
                break;
            };
            if Some(c) == closer {
                self.at += 1;
                break;
            }
            match c {
                ' ' | '\t' => {
                    self.at += 1;
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                }
                '\n' => {
                    self.at += 1;
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                    commands.push(std::mem::take(&mut words));
                    self.heredoc_bodies();
                }
                // `NAME=(...)`: an array assignment's elements belong to its
                // word.
                '(' if word
                    .as_ref()
                    .is_some_and(|w| w.text.ends_with('=') && w.assignment().is_some()) =>
                {
                    let token = word.get_or_insert_with(Token::default);
                    token.text.push('(');
                    self.at += 1;
                    loop {
                        match self.peek(0) {
                            None => {
                                self.opaque = true;
                                break;
                            }
                            Some(')') => {
                                token.text.push(')');
                                self.at += 1;
                                break;
                            }
                            Some(' ' | '\t' | '\n') => {
                                token.text.push(' ');
                                self.at += 1;
                            }
                            Some(_) => self.word_char(token),
                        }
                    }
                }
                ';' | '|' | '(' | ')' => {
                    self.at += 1;
                    if matches!(
                        (c, self.peek(0)),
                        (';', Some(';' | '&')) | ('|', Some('|' | '&'))
                    ) {
                        self.at += 1;
                    }
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                    commands.push(std::mem::take(&mut words));
                }
                '&' if self.peek(1) == Some('>') => {
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                    self.at += 1;
                    words.push(self.redirect());
                }
                '&' => {
                    self.at += 1;
                    if self.peek(0) == Some('&') {
                        self.at += 1;
                    }
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                    commands.push(std::mem::take(&mut words));
                }
                '<' | '>' => {
                    // A descriptor number written against the operator is
                    // part of it (`2>`), not a word.
                    if word.as_ref().is_some_and(|w| {
                        w.plain()
                            && !w.text.is_empty()
                            && w.text.chars().all(|c| c.is_ascii_digit())
                    }) {
                        word = None;
                    }
                    end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
                    let operator = self.redirect();
                    if operator.text == "<<" || operator.text == "<<-" {
                        heredoc_next = Some(operator.text == "<<-");
                    }
                    words.push(operator);
                }
                '#' if word.is_none() => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                _ => {
                    let token = word.get_or_insert_with(Token::default);
                    if token.text.is_empty() && !token.quoted && c == '~' {
                        let start = self.at;
                        let mut end = start;
                        while self.chars.get(end).is_some_and(|c| {
                            !matches!(
                                c,
                                '/' | ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
                            )
                        }) {
                            end += 1;
                        }
                        token.tilde = Some(self.chars[start..end].iter().collect());
                    }
                    self.word_char(token);
                }
            }
        }
        end_word(&mut word, &mut words, &mut heredoc_next, &mut self.heredocs);
        commands.push(words);
        if closer.is_none() && !self.heredocs.is_empty() {
            self.opaque = true;
        }
        commands.retain(|command| !command.is_empty());
        commands
    }

    /// One character of a word, at the unquoted level.
    fn word_char(&mut self, token: &mut Token) {
        let c = self.chars[self.at];
        let plain = token.plain();
        match c {
            '\\' => match self.peek(1) {
                Some('\n') => {
                    self.removed.extend([self.at, self.at + 1]);
                    self.at += 2;
                }
                Some(next) => {
                    token.quoted = true;
                    token.text.push(next);
                    self.at += 2;
                }
                None => {
                    token.text.push(c);
                    self.at += 1;
                    if plain {
                        token.plain_prefix += 1;
                    }
                }
            },
            '\'' => {
                token.quoted = true;
                self.at += 1;
                self.single(token);
            }
            '"' => {
                token.quoted = true;
                self.at += 1;
                self.double(token, Some('"'));
            }
            '`' => {
                self.at += 1;
                self.backquote(token);
            }
            '$' => self.dollar(token, false),
            _ => {
                token.text.push(c);
                self.at += 1;
                if plain {
                    token.plain_prefix += 1;
                }
            }
        }
    }

    fn single(&mut self, token: &mut Token) {
        while let Some(c) = self.peek(0) {
            self.at += 1;
            if c == '\'' {
                return;
            }
            token.text.push(c);
        }
        self.opaque = true;
    }

    /// Double-quote rules until `end`, or to the end of the input for a
    /// here-document body.
    fn double(&mut self, token: &mut Token, end: Option<char>) {
        while let Some(c) = self.peek(0) {
            if Some(c) == end {
                self.at += 1;
                return;
            }
            match c {
                '\\' => match self.peek(1) {
                    Some('\n') => {
                        self.removed.extend([self.at, self.at + 1]);
                        self.at += 2;
                    }
                    Some(next @ ('$' | '`' | '"' | '\\')) => {
                        token.text.push(next);
                        self.at += 2;
                    }
                    _ => {
                        token.text.push(c);
                        self.at += 1;
                    }
                },
                '$' => self.dollar(token, true),
                '`' => {
                    self.at += 1;
                    self.backquote(token);
                }
                _ => {
                    token.text.push(c);
                    self.at += 1;
                }
            }
        }
        if end.is_some() {
            self.opaque = true;
        }
    }

    fn backquote(&mut self, token: &mut Token) {
        token.complex = true;
        let mut inner = String::new();
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '`' => {
                    self.nested(&inner);
                    return;
                }
                '\\' => {
                    if let Some(next) = self.peek(0) {
                        self.at += 1;
                        if !matches!(next, '`' | '\\' | '$') {
                            inner.push('\\');
                        }
                        inner.push(next);
                    }
                }
                _ => inner.push(c),
            }
        }
        self.opaque = true;
    }

    /// Lex code the shell runs elsewhere (a backquote body) for the flags
    /// it can raise here.
    fn nested(&mut self, code: &str) {
        let inner = lex(code);
        self.opaque |= inner.opaque;
        self.current_shell |= inner.current_shell;
    }

    /// Remove backslash-newline pairs at the current position.
    fn skip_continuations(&mut self) {
        while self.peek(0) == Some('\\') && self.peek(1) == Some('\n') {
            self.removed.extend([self.at, self.at + 1]);
            self.at += 2;
        }
    }

    fn dollar(&mut self, token: &mut Token, in_double: bool) {
        self.at += 1;
        self.skip_continuations();
        match self.peek(0) {
            Some('\'') if !in_double => {
                // ANSI-C quoting: a backslash escapes any character,
                // including a quote.
                token.quoted = true;
                self.at += 1;
                while let Some(c) = self.peek(0) {
                    self.at += 1;
                    match c {
                        '\'' => return,
                        '\\' => {
                            if self.peek(0).is_some() {
                                self.at += 1;
                            }
                        }
                        _ => token.text.push(c),
                    }
                }
                self.opaque = true;
            }
            Some('"') if !in_double => {
                token.quoted = true;
                self.at += 1;
                self.double(token, Some('"'));
            }
            Some('(') => {
                token.complex = true;
                self.at += 1;
                if self.peek(0) == Some('(') {
                    self.at += 1;
                    self.arithmetic();
                } else {
                    self.list(Some(')'));
                }
            }
            Some('{') => {
                self.at += 1;
                self.skip_continuations();
                if self
                    .peek(0)
                    .is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '|'))
                {
                    self.current_shell = true;
                    token.complex = true;
                    self.list(Some('}'));
                } else {
                    self.parameter(token);
                }
            }
            Some('[') => {
                token.complex = true;
                self.opaque = true;
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(c) = self
                    .peek(0)
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    name.push(c);
                    self.at += 1;
                }
                token.text.push('$');
                token.text.push_str(&name);
                token.params.push(name);
            }
            Some(c)
                if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '$' | '!' | '-') =>
            {
                self.at += 1;
                token.complex = true;
                token.text.push('$');
                token.text.push(c);
            }
            _ => {
                token.text.push('$');
                if !in_double && !token.quoted && token.plain_prefix + 1 == token.text.len() {
                    token.plain_prefix += 1;
                }
            }
        }
    }

    /// `${...}` after its `{`: a plain name, or anything with an operator.
    fn parameter(&mut self, token: &mut Token) {
        let mut body = String::new();
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            match c {
                '}' if depth == 0 => {
                    self.at += 1;
                    // `NAME` or `NAME[*]`, `NAME[@]`, `NAME[0]`.
                    let (name, subscript) = body
                        .split_once('[')
                        .map_or((body.as_str(), None), |(n, s)| (n, Some(s)));
                    let mut chars = name.chars();
                    let plain = chars
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && subscript.is_none_or(|s| {
                            s.strip_suffix(']').is_some_and(|s| {
                                !s.is_empty()
                                    && s.chars()
                                        .all(|c| c.is_ascii_digit() || matches!(c, '*' | '@'))
                            })
                        });
                    if plain {
                        token.params.push(name.to_string());
                    } else {
                        token.complex = true;
                    }
                    token.text.push_str("${");
                    token.text.push_str(&body);
                    token.text.push('}');
                    return;
                }
                '}' => {
                    depth -= 1;
                    body.push(c);
                    self.at += 1;
                }
                '{' => {
                    depth += 1;
                    body.push(c);
                    self.at += 1;
                }
                '$' | '`' | '"' | '\'' | '\\' => {
                    // A nested expansion or quote inside the operator's
                    // word: lexed by its own rules, and complex.
                    token.complex = true;
                    let mut inner = Token::default();
                    match c {
                        '$' => self.dollar(&mut inner, true),
                        '`' => {
                            self.at += 1;
                            self.backquote(&mut inner);
                        }
                        '"' => {
                            self.at += 1;
                            self.double(&mut inner, Some('"'));
                        }
                        '\'' => {
                            self.at += 1;
                            self.single(&mut inner);
                        }
                        _ => {
                            self.at += 2;
                        }
                    }
                    body.push('?');
                }
                _ => {
                    body.push(c);
                    self.at += 1;
                }
            }
        }
        self.opaque = true;
    }

    fn arithmetic(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            match c {
                '(' => depth += 1,
                ')' if depth == 0 && self.peek(0) == Some(')') => {
                    self.at += 1;
                    return;
                }
                ')' => depth = depth.saturating_sub(1),
                '$' | '`' => {
                    self.at -= 1;
                    let mut inner = Token::default();
                    if c == '$' {
                        self.dollar(&mut inner, true);
                    } else {
                        self.at += 1;
                        self.backquote(&mut inner);
                    }
                }
                _ => {}
            }
        }
        self.opaque = true;
    }

    /// A redirection operator at `self.at`.
    fn redirect(&mut self) -> Token {
        let mut text = String::new();
        while let Some(c) = self
            .peek(0)
            .filter(|c| matches!(c, '<' | '>' | '&' | '|' | '-'))
        {
            // `-` only directly after `<<`.
            if c == '-' && text != "<<" {
                break;
            }
            text.push(c);
            self.at += 1;
        }
        Token {
            plain_prefix: text.len(),
            text,
            redirect: true,
            ..Token::default()
        }
    }

    /// The bodies of the here-documents opened on the line that just
    /// ended. An expanding body has its backslash-newlines removed and is
    /// then read by double-quote rules.
    fn heredoc_bodies(&mut self) {
        for (delimiter, expands, strip) in std::mem::take(&mut self.heredocs) {
            let mut found = false;
            let mut body = String::new();
            while self.at < self.chars.len() {
                let start = self.at;
                let mut end = start;
                while self.chars.get(end).is_some_and(|c| *c != '\n') {
                    end += 1;
                }
                let line: String = self.chars[start..end].iter().collect();
                self.at = (end + 1).min(self.chars.len());
                let trimmed = if strip {
                    line.trim_start_matches('\t')
                } else {
                    &line
                };
                if trimmed == delimiter {
                    found = true;
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if !found {
                self.opaque = true;
            }
            if expands {
                let mut inner = Lexer {
                    chars: body.replace("\\\n", "").chars().collect(),
                    at: 0,
                    removed: Vec::new(),
                    opaque: false,
                    current_shell: false,
                    heredocs: Vec::new(),
                };
                inner.double(&mut Token::default(), None);
                self.opaque |= inner.opaque;
                self.current_shell |= inner.current_shell;
            }
        }
    }
}

/// Shell reserved words and the `!` and `{`/`}` grouping words that open,
/// continue or close a compound command: a word after one of them is again
/// in command position.
const RESERVED: &[&str] = &[
    "!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "time",
    "[[", "]]",
];

/// Reserved words whose whole simple command is a header, not a command
/// that runs: `for NAME in ...`, `select`, `case WORD in`.
const HEADERS: &[&str] = &["for", "select", "case"];

/// What the shell runs on a lexed line, for the directory question.
#[derive(Debug, Default)]
pub(crate) struct Reading {
    /// Some command may move the shell somewhere the guard cannot follow:
    /// any program word that is not a fully unquoted, literal interpreter
    /// or modeled launcher, a function definition, or a hook assignment.
    pub(crate) moves: bool,
    /// The operands of each interpreter call, in order.
    pub(crate) operands: Vec<Token>,
    /// The first interpreter operand whose value may be derived from the
    /// working directory, judged with the assignments made before it.
    pub(crate) derived: Option<Token>,
    /// Names the line assigns a value not derived from the working
    /// directory: a plain assignment, a declaration (`export NAME=`), a
    /// `printf -v NAME`, or a loop variable, each with untainted values.
    pub(crate) assigned: Vec<String>,
}

impl Reading {
    /// Record an assignment to `name`, clean or tainted.
    fn record(&mut self, name: &str, tainted: bool) {
        self.assigned.retain(|known| known != name);
        if !tainted {
            self.assigned.push(name.to_string());
        }
    }

    /// Whether a value may be derived from the working directory: a
    /// command, arithmetic or special expansion, a parameter this line has
    /// not assigned cleanly (`PWD` and `OLDPWD` always count; `HOME` does
    /// not), or a tilde prefix other than `~`.
    pub(crate) fn tainted(&self, word: &Token) -> bool {
        word.complex
            || word.params.iter().any(|name| {
                matches!(name.as_str(), "PWD" | "OLDPWD")
                    || !(name == "HOME" || self.assigned.contains(name))
            })
            || word.tilde.as_deref().is_some_and(|prefix| prefix != "~")
    }
}

/// Read a lexed line's commands. `launcher` returns where a modeled
/// launcher's command starts within the words after it, or `None` when the
/// word is not one or its options are not known; `interpreter` says whether
/// a plain word names an interpreter; `hides` whether an interpreter's
/// operands hold a directory-change token.
pub(crate) fn read(
    lexed: &Lexed,
    launcher: &dyn Fn(&str, &[String]) -> Option<usize>,
    interpreter: &dyn Fn(&str) -> bool,
    hides: &dyn Fn(&[String]) -> bool,
    hook_variables: &[&str],
) -> Reading {
    let mut reading = Reading::default();
    for command in &lexed.commands {
        let mut at = 0;
        let mut header = false;
        while let Some(word) = command.get(at) {
            if word.redirect {
                at += 2;
                continue;
            }
            if word.plain() && HEADERS.contains(&word.text.as_str()) {
                header = true;
                if let Some(name) = command.get(at + 1).filter(|w| w.plain()) {
                    let tainted = command
                        .get(at + 2..)
                        .is_some_and(|values| values.iter().any(|w| reading.tainted(w)));
                    reading.record(&name.text, tainted);
                }
                break;
            }
            if word.plain() && RESERVED.contains(&word.text.as_str()) {
                at += 1;
                continue;
            }
            if let Some(name) = word.assignment() {
                if hook_variables.contains(&name) {
                    reading.moves = true;
                }
                let tainted = reading.tainted(word);
                reading.record(name, tainted);
                at += 1;
                continue;
            }
            break;
        }
        if header {
            continue;
        }
        let rest = &command[at.min(command.len())..];
        if rest.is_empty() {
            continue;
        }
        record_declarations(&mut reading, rest);
        match program_operands(rest, launcher, interpreter, 0) {
            Some(operands) => {
                let texts: Vec<String> = operands.iter().map(|w| w.text.clone()).collect();
                if hides(&texts) {
                    reading.moves = true;
                }
                for word in operands.iter().filter(|w| !w.redirect) {
                    if reading.derived.is_none() && reading.tainted(word) {
                        reading.derived = Some(word.clone());
                    }
                    reading.operands.push(word.clone());
                }
            }
            None => reading.moves = true,
        }
    }
    reading
}

/// Names a declaration builtin or `printf -v` assigns. Each is recorded
/// clean or tainted by its value; the command itself still counts as one
/// that may move the shell.
fn record_declarations(reading: &mut Reading, words: &[Token]) {
    let Some(program) = words.first().filter(|w| w.plain()) else {
        return;
    };
    match program.text.as_str() {
        "export" | "declare" | "typeset" | "local" | "readonly" => {
            for word in &words[1..] {
                if let Some(name) = word.assignment() {
                    let tainted = reading.tainted(word);
                    reading.record(name, tainted);
                }
            }
        }
        "printf" => {
            if let (Some(flag), Some(name)) = (words.get(1), words.get(2)) {
                if flag.plain() && flag.text == "-v" && name.plain() {
                    let tainted = words[3..].iter().any(|w| reading.tainted(w));
                    reading.record(&name.text, tainted);
                }
            }
        }
        _ => {}
    }
}

/// The operands of the interpreter a command runs from its program word,
/// through modeled launchers, or `None` when anything else runs.
fn program_operands<'a>(
    words: &'a [Token],
    launcher: &dyn Fn(&str, &[String]) -> Option<usize>,
    interpreter: &dyn Fn(&str) -> bool,
    depth: usize,
) -> Option<&'a [Token]> {
    let program = words.first()?;
    if depth > 8 || program.redirect || !program.plain() {
        return None;
    }
    let rest = &words[1..];
    if interpreter(&program.text) {
        return Some(rest);
    }
    let texts: Vec<String> = rest.iter().map(|w| w.text.clone()).collect();
    let from = launcher(&program.text, &texts)?;
    program_operands(rest.get(from..)?, launcher, interpreter, depth + 1)
}
