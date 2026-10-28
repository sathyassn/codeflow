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
#[allow(clippy::struct_excessive_bools)] // Each flag is one lexical fact a check reads.
pub(crate) struct Token {
    /// The word with quotes and escapes removed and expansions kept as
    /// written; only meaningful for a word with no quoting or expansion.
    pub(crate) text: String,
    /// Any part of the word was quoted or escaped.
    pub(crate) quoted: bool,
    /// How many leading bytes of `text` were plain: unquoted, unescaped and
    /// not an expansion. A byte offset, so slicing `text` by it is safe for
    /// any input (TSK-397 Codex review round 9).
    pub(crate) plain_prefix: usize,
    /// The names of plain parameter expansions in the word (`$NAME`,
    /// `${NAME}`).
    pub(crate) params: Vec<String>,
    /// Any other expansion: a command or arithmetic substitution, a
    /// backquote, a special parameter or a `${...}` with an operator.
    pub(crate) complex: bool,
    /// The strongest unquoted tilde prefix in the word (`~`, `~+`,
    /// `~user`), wherever it appears: the shell expands one at the start, and
    /// after `=` or `:` in an assignment value or an assignment-shaped
    /// argument (TSK-397 security review SR5-1).
    pub(crate) tilde: Option<String>,
    /// Any part of the word was ANSI-C quoted (`$'...'`).
    pub(crate) ansi: bool,
    /// A redirection operator (`>`, `2>>`, `<<<`, `&>`).
    pub(crate) redirect: bool,
    /// The word runs straight into a `(`: a zsh or ksh pattern group
    /// (`pyth(o)n3`, `@(a|b)`) or a function name, never plain text.
    pub(crate) grouped: bool,
}

impl Token {
    /// Whether the whole word is plain text: no quoting and no expansion.
    pub(crate) fn plain(&self) -> bool {
        !self.quoted
            && !self.complex
            && self.params.is_empty()
            && self.plain_prefix == self.text.len()
    }

    /// Whether the shell passes the word as exactly its text: plain, with
    /// no pattern character any shell may expand (git-guard's `has_glob`:
    /// `*`, `?`, `[`, zsh's extended `#`, `^`, `(`, `)`, `**` and numeric
    /// ranges, C14-2), no zsh `~` exclusion or tilde, no brace expansion
    /// and no zsh `=cmd` (C13-2; a lone `=` is text).
    pub(crate) fn literal(&self) -> bool {
        self.plain()
            && !self.grouped
            && self.tilde.is_none()
            && !crate::hooks::git_guard::has_glob(&self.text)
            && !self.text.contains(['{', '~'])
            && (!self.text.starts_with('=') || self.text == "=")
    }

    /// Whether the word holds a command substitution inside a possible
    /// array subscript (`'a[$(cmd)]'`), which Bash and zsh run when a
    /// builtin evaluates the subscript (`printf -v`, `test -v`, `read`,
    /// numeric comparisons), even from quoted text (TSK-397 Codex review
    /// round 14).
    pub(crate) fn subscript_code(&self) -> bool {
        self.text.split('[').skip(1).any(|after| {
            let inside = after.split(']').next().unwrap_or(after);
            inside.contains("$(") || inside.contains('`')
        })
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
    /// expands, whether leading tabs are stripped, and whether the
    /// delimiter word held an expansion or ANSI-C quoting, which the
    /// lexer does not model (SR5-3).
    heredocs: Vec<(String, bool, bool, bool)>,
}

impl Lexer {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Lex commands until `closer` (unquoted, at this level) or the end.
    #[allow(clippy::too_many_lines)] // One state machine over the shell's operators.
    fn list(&mut self, closer: Option<char>) -> Vec<Vec<Token>> {
        let mut commands: Vec<Vec<Token>> = Vec::new();
        let mut words: Vec<Token> = Vec::new();
        let mut word: Option<Token> = None;
        let mut heredoc_next = None;
        let end_word = |word: &mut Option<Token>,
                        words: &mut Vec<Token>,
                        heredoc_next: &mut Option<bool>,
                        heredocs: &mut Vec<(String, bool, bool, bool)>| {
            if let Some(done) = word.take() {
                if let Some(strip) = heredoc_next.take() {
                    let unmodeled = done.complex || !done.params.is_empty() || done.ansi;
                    heredocs.push((done.text.clone(), !done.quoted, strip, unmodeled));
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
                    if c == '(' {
                        if let Some(token) = word.as_mut() {
                            token.grouped = true;
                        }
                    }
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
                // A shell expands a tilde only at the start of a word, after
                // `=` or `:` in an assignment or a zsh argument, or where
                // brace or array syntax starts a new word; `HEAD~3` is text.
                if c == '~'
                    && token.text.chars().last().is_none_or(|p| {
                        matches!(p, '=' | ':' | '(' | '{' | ',' | ' ' | '\t' | '\n')
                    })
                {
                    let start = self.at;
                    let mut end = start;
                    while self.chars.get(end).is_some_and(|c| {
                        !matches!(
                            c,
                            '/' | ':' | ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
                        )
                    }) {
                        end += 1;
                    }
                    let prefix: String = self.chars[start..end].iter().collect();
                    if token.tilde.as_deref().is_none_or(|known| known == "~") {
                        token.tilde = Some(prefix);
                    }
                }
                token.text.push(c);
                self.at += 1;
                if plain {
                    token.plain_prefix += c.len_utf8();
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
                token.ansi = true;
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
                    self.parameter(token, in_double);
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
            // A `$` is literal only before a blank, the end of input or the
            // closing quote. Anything else (zsh `$=x`, `$^x`) may expand, so
            // it is complex (TSK-397 security review SR6-1).
            next => {
                token.text.push('$');
                let literal = match next {
                    None | Some(' ' | '\t' | '\n') => true,
                    Some('"') => in_double,
                    _ => false,
                };
                if !literal {
                    token.complex = true;
                } else if !in_double && !token.quoted && token.plain_prefix + 1 == token.text.len()
                {
                    token.plain_prefix += 1;
                }
            }
        }
    }

    /// `${...}` after its `{`: a plain name, or anything with an operator.
    fn parameter(&mut self, token: &mut Token, in_double: bool) {
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
                // Inside double quotes, whether `'` quotes depends on the
                // operator; the lexer does not model that (SR5-5).
                '\'' if in_double => {
                    self.opaque = true;
                    return;
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
                // A `)` closing `$((` alone means the shell reads `$( (`,
                // a nested subshell; and a quote or backslash inside
                // arithmetic is not modeled. Both are opaque (SR5-4).
                ')' if depth == 0 => {
                    self.opaque = true;
                    return;
                }
                '\'' | '"' | '\\' => {
                    self.opaque = true;
                    return;
                }
                ')' => depth -= 1,
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
        for (delimiter, expands, strip, unmodeled) in std::mem::take(&mut self.heredocs) {
            if unmodeled {
                self.opaque = true;
            }
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
    /// The words judged as interpreter operands, in order: an interpreter
    /// call's operands, and every word after the point where launcher
    /// traversal stopped on a command that may still run an interpreter.
    pub(crate) operands: Vec<Token>,
    /// The first operand whose value may be derived from the working
    /// directory.
    pub(crate) derived: Option<Token>,
}

impl Reading {
    /// Whether a value may be derived from the working directory. No
    /// variable is trusted, `HOME` included, and so neither is any tilde:
    /// a function the line runs may change `HOME` (TSK-397 Codex review
    /// rounds 9 and 10). The check applies only once the directory is
    /// unknown.
    pub(crate) fn tainted(word: &Token) -> bool {
        word.complex || !word.params.is_empty() || word.tilde.is_some()
    }
}

/// Where launcher traversal ends.
enum Program<'a> {
    /// A modeled interpreter runs with these operands.
    Interpreter(&'a [Token]),
    /// Traversal stopped at the first of these words: a word that is not
    /// literal, a launcher whose options the grammar does not know, or a
    /// program that is neither a launcher nor an interpreter.
    Stopped(&'a [Token]),
}

/// Read a lexed line's commands. `launcher` returns where a modeled
/// launcher's command starts within the words after it, or `None` when the
/// word is not one or its options are not known; `interpreter` says whether
/// a word names an interpreter; `hides` whether an interpreter's operands
/// hold a directory-change token.
pub(crate) fn read(
    lexed: &Lexed,
    launcher: &dyn Fn(&str, &[String]) -> Option<usize>,
    interpreter: &dyn Fn(&str) -> bool,
    hides: &dyn Fn(&[String]) -> bool,
    calls: &dyn Fn(&[String]) -> bool,
    hook_variables: &[&str],
) -> Reading {
    let callbacks = Callbacks {
        launcher,
        interpreter,
        hides,
        calls,
        hook_variables,
    };
    let shadowed = shadows(lexed, &callbacks);
    read_with(lexed, &callbacks, shadowed)
}

/// The checks [`read`] is given, passed on to an `eval` body.
struct Callbacks<'a> {
    launcher: &'a dyn Fn(&str, &[String]) -> Option<usize>,
    interpreter: &'a dyn Fn(&str) -> bool,
    hides: &'a dyn Fn(&[String]) -> bool,
    calls: &'a dyn Fn(&[String]) -> bool,
    hook_variables: &'a [&'a str],
}

/// Programs that give a name another meaning for the rest of the line: an
/// alias, a builtin turned off (`enable -n`, `disable`) so a program of the
/// same name runs, a hashed or autoloaded name, a function, or a file read
/// into the shell, which may define any of these.
const SHADOWING_PROGRAMS: &[&str] = &[
    "alias",
    "enable",
    "disable",
    "hash",
    "autoload",
    "function",
    "functions",
    "source",
    ".",
];

/// Builtins that assign a variable named by an operand, so `read PATH`
/// changes the command search path.
const ASSIGNING_PROGRAMS: &[&str] = &[
    "read",
    "printf",
    "mapfile",
    "readarray",
    "getln",
    "vared",
    "declare",
    "typeset",
    "export",
    "local",
    "readonly",
    "getopts",
    "print",
];

/// Whether a word sets the command search path (`PATH=`, `path+=`,
/// `path[1]=`), as an assignment or a declaration's operand.
fn sets_path(word: &Token) -> bool {
    ["PATH=", "PATH+=", "PATH[", "path=", "path+=", "path["]
        .iter()
        .any(|form| word.text.starts_with(form))
}

/// Whether the line may change what a program name runs: a command whose
/// program, reached through modeled launchers, is one of
/// [`SHADOWING_PROGRAMS`], a function definition, a program word the text
/// does not show, an `eval` whose text does any of these or is not fixed,
/// or a change to the command search path. Then no name on the line is
/// exempt by the allowlist.
fn shadows(lexed: &Lexed, callbacks: &Callbacks<'_>) -> bool {
    lexed.opaque
        || lexed.current_shell
        || lexed.commands.iter().any(|command| {
            command.iter().any(sets_path)
                || command_shadows(&command[program_start(command)..], callbacks)
        })
}

/// Whether one command, from its program word, may change what a name runs.
fn command_shadows(rest: &[Token], callbacks: &Callbacks<'_>) -> bool {
    let Program::Stopped(remaining) =
        program_operands(rest, callbacks.launcher, callbacks.interpreter, 0)
    else {
        return false;
    };
    let Some((program, words)) = remaining.split_first() else {
        return false;
    };
    if program.redirect {
        return false;
    }
    // `[` is the `test` builtin, not a pattern.
    if program.grouped || !(program.literal() || (program.plain() && program.text == "[")) {
        return true;
    }
    let name = crate::hooks::git_guard::launcher_name(&program.text);
    if SHADOWING_PROGRAMS.contains(&name) {
        return true;
    }
    if ASSIGNING_PROGRAMS.contains(&name)
        && words
            .iter()
            .any(|w| matches!(w.text.as_str(), "PATH" | "path"))
    {
        return true;
    }
    if name == "eval" {
        let joined: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let inner = lex(&joined.join(" "));
        return words
            .iter()
            .any(|w| w.complex || !w.params.is_empty() || w.ansi)
            || shadows(&inner, callbacks);
    }
    false
}

/// Where a command's program word starts: after redirections, reserved
/// words, assignments and a `for` or `select` header up to its `do`; the
/// end for a `case` header.
fn program_start(command: &[Token]) -> usize {
    let mut at = 0;
    while let Some(word) = command.get(at) {
        if word.redirect {
            at += 2;
        } else if word.plain() && matches!(word.text.as_str(), "for" | "select") {
            match command[at + 1..]
                .iter()
                .position(|w| w.plain() && w.text == "do")
            {
                Some(offset) => at += offset + 2,
                None => return command.len(),
            }
        } else if word.plain() && word.text == "case" {
            return command.len();
        } else if (word.plain() && RESERVED.contains(&word.text.as_str()))
            || word.assignment().is_some()
        {
            at += 1;
        } else {
            break;
        }
    }
    at.min(command.len())
}

/// A command substitution in a possible subscript runs wherever a builtin
/// evaluates it, quoted or not, so it is a hidden command and a value filled
/// in at run time (C14-1).
fn subscript_commands(command: &[Token], reading: &mut Reading) {
    for word in command.iter().filter(|w| w.subscript_code()) {
        reading.moves = true;
        reading.operands.push(Token {
            complex: true,
            ..word.clone()
        });
    }
}

/// A `[[ ... ]]` conditional, its words after `[[`, runs no program. It
/// evaluates a word as code only through `-v`, `-R` or a numeric
/// comparison, and then its words are judged.
fn conditional(words: &[Token], reading: &mut Reading) {
    let inner = match words.iter().rposition(|w| w.plain() && w.text == "]]") {
        Some(end) => &words[..end],
        None => words,
    };
    if !super::non_executing::conditional_runs_nothing(inner) {
        reading.moves = true;
        reading
            .operands
            .extend(inner.iter().filter(|w| !w.redirect).cloned());
    }
}

/// Traversal stopped at `remaining`, the tail of `command`, where a program
/// may run an interpreter with the words after it, at the start of the
/// command or inside a launcher. One rule holds at every stop (operator
/// decision, 2026-10-27): the remaining words are judged as possible
/// operands unless the stop is a literal program on the non-executing
/// allowlist used with the options its entry lists, on a line that changes
/// no program name's meaning. `xargs` adds arguments the text does not
/// show. A later word naming an interpreter, or an interpreter call the
/// command walk sees, also keeps them.
fn stopped(
    command: &[Token],
    remaining: &[Token],
    callbacks: &Callbacks<'_>,
    shadowed: bool,
    reading: &mut Reading,
) {
    reading.moves = true;
    let texts: Vec<String> = command.iter().map(|w| w.text.clone()).collect();
    let launchers = &command[..command.len() - remaining.len()];
    let appended = launchers
        .iter()
        .any(|w| crate::hooks::git_guard::launcher_name(&w.text) == "xargs");
    let after = remaining.iter().skip(1);
    if shadowed
        || !super::non_executing::runs_nothing(remaining, appended)
        || after.clone().any(|w| (callbacks.interpreter)(&w.text))
        || (callbacks.calls)(&texts)
    {
        reading
            .operands
            .extend(after.filter(|w| !w.redirect).cloned());
    }
    reading
        .operands
        .extend(eval_operands(remaining, callbacks, shadowed));
}

/// [`read`] with the callbacks gathered, and whether the line (or the line
/// an `eval` body came from) changes what a program name runs.
fn read_with(lexed: &Lexed, callbacks: &Callbacks<'_>, shadowed: bool) -> Reading {
    let Callbacks {
        launcher,
        interpreter,
        hides,
        hook_variables,
        ..
    } = *callbacks;
    let mut reading = Reading::default();
    for command in &lexed.commands {
        subscript_commands(command, &mut reading);
        let mut at = 0;
        let mut header = false;
        while let Some(word) = command.get(at) {
            if word.redirect {
                at += 2;
                continue;
            }
            if word.plain() && word.text == "[[" {
                conditional(&command[at + 1..], &mut reading);
                header = true;
                break;
            }
            if word.plain() && HEADERS.contains(&word.text.as_str()) {
                // A `for` or `select` header ends at `do`, and the words
                // after it are in command position again (SR6-2). Without
                // a `do` in this command the header runs nothing.
                let resume = (word.text != "case")
                    .then(|| {
                        command[at + 1..]
                            .iter()
                            .position(|w| w.plain() && w.text == "do")
                    })
                    .flatten();
                if let Some(offset) = resume {
                    at += offset + 2;
                    continue;
                }
                header = true;
                break;
            }
            if word.plain() && RESERVED.contains(&word.text.as_str()) {
                at += 1;
                // The `time` keyword takes bash's `-p` and `--` before the
                // command it times (C12-2).
                if word.text == "time" {
                    while command
                        .get(at)
                        .is_some_and(|w| w.plain() && matches!(w.text.as_str(), "-p" | "--"))
                    {
                        at += 1;
                    }
                }
                continue;
            }
            if let Some(name) = word.assignment() {
                if hook_variables.contains(&name) {
                    reading.moves = true;
                }
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
        match program_operands(rest, launcher, interpreter, 0) {
            Program::Interpreter(operands) => {
                let texts: Vec<String> = operands.iter().map(|w| w.text.clone()).collect();
                if hides(&texts) {
                    reading.moves = true;
                }
                reading
                    .operands
                    .extend(operands.iter().filter(|w| !w.redirect).cloned());
            }
            Program::Stopped(remaining) => {
                stopped(command, remaining, callbacks, shadowed, &mut reading);
            }
        }
    }
    reading.derived = reading
        .operands
        .iter()
        .find(|w| Reading::tainted(w))
        .cloned();
    reading
}

/// `eval` joins its words and parses them again, so text in single quotes
/// (`eval 'python3 r.py "$PWD/.."'`) is read as a line of its own (round
/// 13): the operands that line yields, and every word marked run-time when
/// the joined text is opaque. Empty for any other program.
fn eval_operands(words: &[Token], callbacks: &Callbacks<'_>, shadowed: bool) -> Vec<Token> {
    let Some((program, rest)) = words.split_first() else {
        return Vec::new();
    };
    if !program.literal() || crate::hooks::git_guard::launcher_name(&program.text) != "eval" {
        return Vec::new();
    }
    let joined: Vec<&str> = rest.iter().map(|w| w.text.as_str()).collect();
    let inner = lex(&joined.join(" "));
    let mut operands =
        read_with(&inner, callbacks, shadowed || shadows(&inner, callbacks)).operands;
    if inner.opaque || inner.current_shell {
        operands.extend(rest.iter().map(|w| Token {
            complex: true,
            ..w.clone()
        }));
    }
    operands
}

/// Follow a command from its program word through modeled launchers.
fn program_operands<'a>(
    words: &'a [Token],
    launcher: &dyn Fn(&str, &[String]) -> Option<usize>,
    interpreter: &dyn Fn(&str) -> bool,
    depth: usize,
) -> Program<'a> {
    let Some(program) = words.first() else {
        return Program::Interpreter(&[]);
    };
    if depth > 8 || program.redirect || !program.literal() {
        return Program::Stopped(words);
    }
    let rest = &words[1..];
    if interpreter(&program.text) {
        return Program::Interpreter(rest);
    }
    let texts: Vec<String> = rest.iter().map(|w| w.text.clone()).collect();
    // A launcher's options and their values must be literal, or the
    // argument count can shift and another word becomes the program
    // (C13-1); otherwise traversal stops at the launcher's own name.
    match launcher(&program.text, &texts)
        .filter(|&from| {
            rest.get(..from)
                .is_some_and(|options| options.iter().all(Token::literal))
        })
        .and_then(|from| rest.get(from..))
    {
        Some(inner) => program_operands(inner, launcher, interpreter, depth + 1),
        None => Program::Stopped(words),
    }
}

#[cfg(test)]
#[path = "shell_lex_tests.rs"]
mod tests;
