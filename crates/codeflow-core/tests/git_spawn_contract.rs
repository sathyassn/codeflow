//! Every `git` process codeflow starts is built by `codeflow_core::git`
//! (TSK-141 AC-4, SPC-013 R-85 as amended), so a hook git fires during a
//! codeflow command runs that same binary. A spawn written as
//! `Command::new("git")`, or as `Command::new(program)` where `program`
//! may hold git, would dispatch its hooks by PATH again.
//!
//! The scan reads each production source as Rust tokens, as rustc does:
//! CRLF line ends are read as LF, comments are dropped wherever they fall,
//! a raw identifier (`r#new`) is the identifier, and string literals are
//! decoded by Rust's escape rules (`\x`, `\u{…}` with underscores, raw
//! strings, line continuations). A literal the scan cannot decode is
//! refused, never read as some other program. It enforces a supported
//! subset, in which every spawn is written `…Command::new(ARG)` or
//! `<…Command>::new(ARG)`, and judges each argument. A literal that names
//! git (`git`, `git.exe`, `/usr/bin/git` and the like) is refused. Any other
//! argument that is not one literal must be listed in [`DYNAMIC`] with the
//! reason it never holds git, so a new spawn built from a value has to be
//! looked at. Every other form that would hide a spawn from that reading
//! is refused outright: renaming `Command` on import, a type alias of it
//! (parenthesized or not), generic arguments after it (`Command::<>`),
//! the constructor taken as a value, and a macro that builds
//! `$name::new` or `<$name>::new` from a metavariable.
//!
//! Outside the subset, and so not seen: a procedural macro that assembles
//! the name, and a process API other than `Command` (no production source
//! uses one). A shell that runs git from its own script text, such as a
//! test target the runner starts through `sh -c`, is the user's command,
//! not one codeflow builds.

//! The fixture environment scan below checks raw source tokens, including
//! comments and strings, across Rust, JavaScript, Python and shell fixtures.
//! It cannot see dynamic keys constructed from fragments or environment
//! changes made by an external program. Clone checks cover literal arrays
//! passed to Command args and the Git helper names listed in the scanner.

use std::path::{Path, PathBuf};

/// Spawns whose program is not a literal: (file under `crates/`, the
/// argument with whitespace removed, how many, why it never holds git).
const DYNAMIC: &[(&str, &str, usize, &str)] = &[
    (
        "codeflow-present/src/responses.rs",
        "std::env::current_exe().unwrap()",
        1,
        "a test re-running its own test binary as the crashing child",
    ),
    (
        "codeflow-core/src/git/mod.rs",
        "program",
        1,
        "the git constructor itself",
    ),
    (
        "codeflow-core/src/remote.rs",
        "&self.gh",
        1,
        "the gh client",
    ),
    (
        "codeflow-core/src/remote.rs",
        "path",
        1,
        "a test's fake gh shim",
    ),
    (
        "codeflow-core/src/release_local.rs",
        "PYTHON",
        3,
        "the python3 interpreter",
    ),
    (
        "codeflow-core/src/testing/runner/mod.rs",
        "executable",
        1,
        "powershell.exe or pwsh",
    ),
    (
        "codeflow-core/src/testing/doctor/mod.rs",
        "cmd",
        1,
        "a fixed list of test-runner probes, none of them git",
    ),
    (
        "codeflow-core/src/scaffold/state.rs",
        "std::env::current_exe().unwrap()",
        1,
        "a test re-running its own binary",
    ),
    (
        "codeflow-core/src/testing/runner/mod.rs",
        "std::env::current_exe().unwrap()",
        1,
        "a Windows test re-running its own binary as the gate",
    ),
    (
        "codeflow-cli/src/cmd/present.rs",
        "executable",
        3,
        "this codeflow binary, as the service and its test canaries",
    ),
    (
        "codeflow-cli/src/cmd/push_set.rs",
        "exe",
        1,
        "this codeflow binary",
    ),
    (
        "codeflow-present/src/platform.rs",
        "executable",
        2,
        "the restricted constructor (browser and /bin/ps) and a test re-run",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// One Rust token, as far as the scan needs: comments and whitespace are
/// gone and a string literal holds its decoded value.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Punct(char),
    Str(String),
    /// A string literal Rust's escape rules do not decode, as written.
    Undecodable(String),
    /// A number or character literal, or a lifetime.
    Other(String),
}

impl Token {
    /// The token as written, for listing a dynamic argument.
    fn text(&self) -> String {
        match self {
            Token::Ident(text) | Token::Other(text) | Token::Undecodable(text) => text.clone(),
            Token::Punct(c) => c.to_string(),
            Token::Str(value) => format!("{value:?}"),
        }
    }

    fn is(&self, word: &str) -> bool {
        matches!(self, Token::Ident(text) if text == word)
    }

    fn punct(&self, c: char) -> bool {
        *self == Token::Punct(c)
    }
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The tokens of `text`.
fn tokens(text: &str) -> Vec<Token> {
    // rustc reads a CRLF line end as LF, inside literals too.
    let chars: Vec<char> = text.replace("\r\n", "\n").chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at = |n: usize| chars.get(i + n).copied();
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(1) == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(1) == Some('*') {
            let mut depth = 0_usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if let Some((value, next)) = raw_string(&chars, i) {
            out.push(Token::Str(value));
            i = next;
        } else if c == 'r'
            && at(1) == Some('#')
            && at(2).is_some_and(|n| ident_char(n) && !n.is_ascii_digit())
            && !(i > 0 && ident_char(chars[i - 1]))
        {
            // A raw identifier is the identifier.
            let start = i + 2;
            i = start;
            while i < chars.len() && ident_char(chars[i]) {
                i += 1;
            }
            out.push(Token::Ident(chars[start..i].iter().collect()));
        } else if c == '"' || (c == 'b' && at(1) == Some('"')) {
            let open = if c == 'b' { i + 1 } else { i };
            let (value, next) = string(&chars, open, c == 'b');
            out.push(match value {
                Some(value) => Token::Str(value),
                None => Token::Undecodable(chars[open..next.min(chars.len())].iter().collect()),
            });
            i = next;
        } else if c == '\'' {
            // A character literal, or a lifetime.
            let end = if at(1) == Some('\\') {
                (i + 2..chars.len()).find(|&j| chars[j] == '\'')
            } else if at(2) == Some('\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(end) = end {
                out.push(Token::Other(chars[i..=end].iter().collect()));
                i = end + 1;
            } else {
                out.push(Token::Punct('\''));
                i += 1;
            }
        } else if ident_char(c) {
            let start = i;
            while i < chars.len() && ident_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if c.is_ascii_digit() {
                out.push(Token::Other(word));
            } else {
                out.push(Token::Ident(word));
            }
        } else {
            out.push(Token::Punct(c));
            i += 1;
        }
    }
    out
}

/// A raw string (`r"…"`, `r#"…"#`, `br"…"`) at `chars[i]`: its value and
/// the index after it.
fn raw_string(chars: &[char], i: usize) -> Option<(String, usize)> {
    if i > 0 && ident_char(chars[i - 1]) {
        return None;
    }
    let r = match (chars.get(i), chars.get(i + 1)) {
        (Some('r'), _) => i,
        (Some('b'), Some('r')) => i + 1,
        _ => return None,
    };
    let hashes = chars[r + 1..].iter().take_while(|&&c| c == '#').count();
    let open = r + 1 + hashes;
    if chars.get(open) != Some(&'"') {
        return None;
    }
    let mut j = open + 1;
    while j < chars.len()
        && !(chars[j] == '"' && (1..=hashes).all(|h| chars.get(j + h) == Some(&'#')))
    {
        j += 1;
    }
    Some((
        chars[open + 1..j.min(chars.len())].iter().collect(),
        j + 1 + hashes,
    ))
}

/// A string literal opening at `chars[open]`, decoded by Rust's escape
/// rules, and the index after it; `None` for a value those rules do not
/// give (an unknown escape, a bad `\x` or `\u{…}`, no closing quote). A
/// byte string takes any `\x` byte and no `\u{…}`.
fn string(chars: &[char], open: usize, bytes: bool) -> (Option<String>, usize) {
    let mut value = String::new();
    let mut ok = true;
    let mut j = open + 1;
    while j < chars.len() && chars[j] != '"' {
        if chars[j] != '\\' {
            value.push(chars[j]);
            j += 1;
            continue;
        }
        let escaped = chars.get(j + 1).copied();
        j += 2;
        match escaped {
            Some('n') => value.push('\n'),
            Some('r') => value.push('\r'),
            Some('t') => value.push('\t'),
            Some('0') => value.push('\0'),
            Some(c @ ('\\' | '\'' | '"')) => value.push(c),
            Some('x') => {
                let hex: String = chars[j.min(chars.len())..(j + 2).min(chars.len())]
                    .iter()
                    .collect();
                match u8::from_str_radix(&hex, 16) {
                    Ok(byte) if hex.len() == 2 && (bytes || byte <= 0x7f) => {
                        value.push(char::from(byte));
                    }
                    _ => ok = false,
                }
                j += 2;
            }
            Some('u') if !bytes => {
                let close = (j..chars.len()).find(|&k| chars[k] == '}');
                let digits: Option<String> = close.and_then(|close| {
                    (chars.get(j) == Some(&'{')).then(|| chars[j + 1..close].iter().collect())
                });
                let decoded = digits.and_then(|digits: String| {
                    let hex: String = digits.chars().filter(|c| *c != '_').collect();
                    let well_formed = !digits.starts_with('_') && (1..=6).contains(&hex.len());
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .filter(|_| well_formed)
                        .and_then(char::from_u32)
                });
                match decoded {
                    Some(c) => value.push(c),
                    None => ok = false,
                }
                j = close.map_or(chars.len(), |close| close + 1);
            }
            // A line continuation drops the newline and the blanks after it.
            Some('\n') => {
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
            }
            _ => ok = false,
        }
    }
    if j >= chars.len() {
        ok = false;
    }
    (ok.then_some(value), j + 1)
}

/// Whether `toks[at..]` starts with `::new`.
fn at_new(toks: &[Token], at: usize) -> bool {
    matches!(toks.get(at..at + 3), Some([a, b, new]) if a.punct(':') && b.punct(':') && new.is("new"))
}

/// Each `Command::new(...)` or `<…Command>::new(...)` in `toks`: its
/// argument as written, with the literal's value when it is one string.
fn spawns(toks: &[Token]) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    for (k, tok) in toks.iter().enumerate() {
        if !tok.is("Command") {
            continue;
        }
        let new = if at_new(toks, k + 1) {
            k + 4
        } else if toks.get(k + 1).is_some_and(|t| t.punct('>')) && at_new(toks, k + 2) {
            k + 5
        } else {
            continue;
        };
        if !toks.get(new).is_some_and(|t| t.punct('(')) {
            continue;
        }
        let mut depth = 0_usize;
        let mut end = toks.len();
        for (j, t) in toks.iter().enumerate().skip(new) {
            if t.punct('(') {
                depth += 1;
            } else if t.punct(')') {
                depth -= 1;
                if depth == 0 {
                    end = j;
                    break;
                }
            }
        }
        let argument = &toks[new + 1..end];
        let text: String = argument.iter().map(Token::text).collect();
        let value = match argument {
            [Token::Str(value)] => Some(value.clone()),
            _ => None,
        };
        found.push((text, value));
    }
    found
}

/// The forms that would hide a spawn from [`spawns`].
fn hidden_spawns(file: &str, toks: &[Token]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, tok) in toks.iter().enumerate() {
        let next = |n: usize| toks.get(k + n);
        if tok.is("Command") && next(1).is_some_and(|t| t.is("as")) {
            out.push(format!(
                "{file}: renames Command on import, which hides its spawns"
            ));
        }
        let new = if at_new(toks, k + 1) {
            Some(k + 4)
        } else if next(1).is_some_and(|t| t.punct('>')) && at_new(toks, k + 2) {
            Some(k + 5)
        } else {
            None
        };
        if let Some(new) = new.filter(|_| tok.is("Command")) {
            if !toks.get(new).is_some_and(|t| t.punct('(')) {
                out.push(format!(
                    "{file}: takes Command::new as a value, which hides its spawns"
                ));
            }
        } else if tok.is("Command") {
            // Generic arguments between `Command` and `::new` hide it.
            let generic = |at: usize| matches!(toks.get(at..at + 3), Some([a, b, c]) if a.punct(':') && b.punct(':') && c.punct('<'));
            if generic(k + 1) || (next(1).is_some_and(|t| t.punct('>')) && generic(k + 2)) {
                out.push(format!(
                    "{file}: Command::<…> is outside what the scan reads"
                ));
            }
        }
        let metavariable_new =
            at_new(toks, k + 2) || (next(2).is_some_and(|t| t.punct('>')) && at_new(toks, k + 3));
        if tok.punct('$') && matches!(next(1), Some(Token::Ident(_))) && metavariable_new {
            out.push(format!(
                "{file}: a macro builds a constructor from a metavariable, which hides its spawns"
            ));
        }
        if let Token::Undecodable(text) = tok {
            out.push(format!(
                "{file}: the string literal {text} is not one the scan can decode"
            ));
        }
        if tok.is("type") && matches!(next(1), Some(Token::Ident(_))) {
            let Some(eq) = toks[k..].iter().position(|t| t.punct('=') || t.punct(';')) else {
                continue;
            };
            // Parentheses around a type change nothing.
            let rhs: Vec<&Token> = toks[k + eq + 1..]
                .iter()
                .take_while(|t| !t.punct(';'))
                .filter(|t| !t.punct('(') && !t.punct(')'))
                .collect();
            let path = rhs
                .iter()
                .all(|t| matches!(t, Token::Ident(_)) || t.punct(':'));
            if toks[k + eq].punct('=') && path && rhs.last().is_some_and(|t| t.is("Command")) {
                out.push(format!("{file}: aliases Command, which hides its spawns"));
            }
        }
    }
    out
}

/// Whether a program name runs git: its last path part, without an
/// extension, is `git`.
fn names_git(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = name.split('.').next().unwrap_or(name);
    stem.eq_ignore_ascii_case("git")
}

/// Why `text`, read as the file `file`, breaks the contract.
fn violations(file: &str, text: &str) -> Vec<String> {
    let toks = tokens(text);
    let mut out = hidden_spawns(file, &toks);
    let mut dynamic: Vec<(String, usize)> = Vec::new();
    for (argument, value) in spawns(&toks) {
        match value {
            Some(program) if names_git(&program) => out.push(format!(
                "{file}: Command::new({argument}) starts git; use codeflow_core::git"
            )),
            Some(_) => {}
            None => match dynamic.iter_mut().find(|(a, _)| *a == argument) {
                Some((_, n)) => *n += 1,
                None => dynamic.push((argument, 1)),
            },
        }
    }
    for (argument, count) in dynamic {
        let listed = DYNAMIC
            .iter()
            .find(|(f, a, _, _)| *f == file && *a == argument);
        match listed {
            Some((_, _, expected, _)) if *expected == count => {}
            Some((_, _, expected, _)) => out.push(format!(
                "{file}: Command::new({argument}) appears {count} times, listed {expected}"
            )),
            None => out.push(format!(
                "{file}: Command::new({argument}) builds a program from a value; \
                 spawn git through codeflow_core::git, or list it in DYNAMIC with \
                 the reason it never holds git"
            )),
        }
    }
    out
}

#[test]
fn every_git_process_is_built_by_the_one_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        rust_files(&crates.join(krate).join("src"), &mut files);
    }
    assert!(
        files.contains(&crates.join("codeflow-core/src/git/mod.rs")),
        "the constructor moved"
    );
    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for file in &files {
        let name = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(file).unwrap();
        offenders.extend(violations(&name, &text));
        seen.extend(
            spawns(&tokens(&text))
                .into_iter()
                .filter(|(_, value)| value.is_none())
                .map(|(argument, _)| (name.clone(), argument)),
        );
    }
    for (file, argument, _, _) in DYNAMIC {
        if !seen.iter().any(|(f, a)| f == file && a == argument) {
            offenders.push(format!(
                "{file}: DYNAMIC lists Command::new({argument}), which is gone"
            ));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// The closing delimiter of a Rust token group.
fn group_end(toks: &[Token], start: usize) -> usize {
    let mut depth = 0_usize;
    for (index, token) in toks.iter().enumerate().skip(start) {
        if token.punct('[') || token.punct('(') || token.punct('{') {
            depth += 1;
        } else if token.punct(']') || token.punct(')') || token.punct('}') {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    panic!("unclosed Rust token group");
}

/// Comma-separated expressions, retaining nested calls/indexing as one argument.
fn arguments(toks: &[Token]) -> Vec<&[Token]> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < toks.len() {
        if toks[index].punct('[') || toks[index].punct('(') || toks[index].punct('{') {
            index = group_end(toks, index);
        } else if toks[index].punct(',') {
            args.push(&toks[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    if start < toks.len() {
        args.push(&toks[start..]);
    }
    args
}

fn literal(toks: &[Token]) -> Option<&str> {
    match toks {
        [Token::Str(value)] => Some(value),
        _ => None,
    }
}

/// The callee enclosing an array, ignoring completed nested expressions.
fn array_callee(toks: &[Token], start: usize) -> Option<&Token> {
    let mut stack = Vec::new();
    for (index, token) in toks[..start].iter().enumerate() {
        if token.punct('(') || token.punct('[') || token.punct('{') {
            stack.push(index);
        } else if token.punct(')') || token.punct(']') || token.punct('}') {
            stack.pop();
        }
    }
    let open = *stack.last()?;
    (toks[open].punct('(') && open > 0).then(|| &toks[open - 1])
}

fn clone_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--template"
            | "--reference"
            | "--reference-if-able"
            | "--separate-git-dir"
            | "-c"
            | "--config"
            | "-o"
            | "-b"
            | "--branch"
            | "--depth"
            | "--filter"
            | "--origin"
            | "-u"
            | "--upload-pack"
            | "-j"
            | "--jobs"
            | "--shallow-since"
            | "--shallow-exclude"
            | "--server-option"
            | "--bundle-uri"
    )
}

/// Literal arrays passed to Command args or this suite's Git helpers form the
/// supported subset. Split clone chains and bound clone arrays are refused.
fn unsafe_fixture_clones(_file: &str, source: &str) -> Vec<String> {
    let toks = tokens(source);
    let mut offenders = Vec::new();
    for (start, token) in toks.iter().enumerate() {
        if token.is("arg")
            && toks.get(start + 1).is_some_and(|t| t.punct('('))
            && matches!(toks.get(start + 2), Some(Token::Str(s)) if s == "clone")
        {
            offenders
                .push(".arg(\"clone\") hides its options; use one literal argument array".into());
        }
        if !token.punct('[') {
            continue;
        }
        let end = group_end(&toks, start);
        let args = arguments(&toks[start + 1..end]);
        if !args.iter().any(|arg| literal(arg) == Some("clone")) {
            continue;
        }
        let text: String = toks[start..=end].iter().map(Token::text).collect();
        let before = &toks[..start];
        let direct = matches!(before.last(), Some(t) if t.punct('('))
            || matches!(before, [.., separator, amp] if amp.punct('&')
                && (separator.punct('(') || separator.punct(',')));
        if !direct {
            let bound = before
                .iter()
                .rev()
                .take_while(|t| !t.punct(';') && !t.punct('{'))
                .any(|t| t.is("let"));
            if bound {
                offenders.push(format!("{text}: bound clone array hides its use"));
            }
            continue;
        }
        if !array_callee(&toks, start).is_some_and(|callee| {
            ["args", "git", "run_git", "git_ok", "run", "command"]
                .iter()
                .any(|name| callee.is(name))
        }) {
            continue;
        }
        let mut command = 0;
        while command + 1 < args.len() && matches!(literal(args[command]), Some("-C" | "-c")) {
            command += 2;
        }
        if args.get(command).and_then(|arg| literal(arg)) != Some("clone") {
            // A path or message named clone is not a clone invocation.
            // Only the supported command slot identifies the operation.
            // Unrelated argument arrays need no vocabulary allowance.
            continue;
        }
        let mut transport = false;
        let mut index = command + 1;
        while index < args.len() {
            match literal(args[index]) {
                Some("--") => break,
                Some("--no-local") => transport = true,
                Some("--local" | "-l") => transport = false,
                Some(option) if clone_option_takes_value(option) => {
                    index += 1;
                }
                _ => {}
            }
            index += 1;
        }
        if !transport {
            offenders.push(format!("{text}: fixture clone needs effective --no-local before --; local copies can race with repacking"));
        }
    }
    offenders
}

fn fixture_sources() -> (PathBuf, Vec<PathBuf>) {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let krate = entry.unwrap().path();
        for area in ["src", "tests", "examples"] {
            let dir = krate.join(area);
            if dir.is_dir() {
                rust_files(&dir, &mut files);
            }
        }
    }
    files.sort();
    (crates, files)
}

#[test]
fn fixture_clones_use_the_git_transport() {
    let (crates, files) = fixture_sources();
    let mut offenders = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap();
        let name = file
            .strip_prefix(&crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for finding in unsafe_fixture_clones(&name, &source) {
            offenders.push(format!("{name}: {finding}"));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[test]
fn clone_scan_checks_the_flags_on_each_argument_list() {
    for source in [
        r#"git(root, &["clone", "-q", "--bare", source, dest]);"#,
        r#"git(root, &["clone", "source", "dest"]);"#,
        r#"git(root, &["clone", "--no-hardlinks", source, dest]);"#,
        r#"command.args(["clone", path.to_str().unwrap()]).arg(dest);"#,
        r#"git(root, &[r"clone", "-q", "file:///repo", "dest"]);"#,
        r#"git(root, &["cl\x6fne", source]); // --no-local"#,
        r#"git(root, &["clone", paths[0], dest]); let flag = "--no-local";"#,
        r#"git(root, &["clone", path("--no-local"), dest]);"#,
        r#"cmd.arg("clone").arg(source);"#,
        r#"cmd.arg("clone").arg("--no-local").arg(source);"#,
        r#"let args = ["clone", source]; cmd.args(args);"#,
        r#"let args = &["clone", "--no-local", source]; cmd.args(args);"#,
        r#"cmd.args(["-C", root, "clone", source]);"#,
        r#"cmd.args(["clone", "--", "--no-local", source]);"#,
        r#"cmd.args(["clone", "--no-local", "--local", source]);"#,
        r#"cmd.args(["clone", "--no-local", "-l", source]);"#,
        r#"cmd.args(["-c", "key=value", "clone", source]);"#,
    ] {
        assert!(
            !unsafe_fixture_clones("probe.rs", source).is_empty(),
            "{source}"
        );
    }
    for source in [
        r#"display_words(&["clone", "help"]); git(root, &["add", "clone"]);"#,
        r#"["clone", "help"].iter();"#,
        r#"Commands { subcommands: &["clone", "help"] }"#,
        r#"cmd.args(["-c", "a=b", "-C", root, "clone", "--no-local", source]);"#,
        r#"cmd.args(["clone", "--template=--local", "--no-local", source]);"#,
        r#"git(root, &["clone", "--no-local", source, dest]);"#,
        r#"git(root, &["clone", "--no-hardlinks", "--no-local", source, dest]);"#,
        r#"command.args(["clone", "--no-local", "--depth", "1", url]);"#,
        "git(root, &[/* fixture */ \"clone\",\n r\"--no-local\", source]);",
        r#"cmd.args(["-C", root.to_str().unwrap(), "clone", "--no-local", source]);"#,
        r#"cmd.args(["clone", "--local", "--no-local", source]);"#,
        r#"cmd.args(["clone", "--no-local", "--", "--local", dest]);"#,
        r#"// git(root, &["clone", source]);"#,
        r#"let text = "git(root, &[\"clone\", source]);";"#,
    ] {
        assert!(
            unsafe_fixture_clones("probe.rs", source).is_empty(),
            "{source}"
        );
    }
    assert_eq!(
        unsafe_fixture_clones(
            "probe.rs",
            r#"git(root, &["clone", "--no-local", source, a]);
        git(root, &["clone", source, b]);"#
        )
        .len(),
        1
    );
}

/// Environment tokens are deliberately checked in comments and strings too.
/// Dynamic key construction from fragments and mutations by external programs
/// remain outside this source scan. Exact line bindings require review on moves.
#[derive(Debug)]
struct EnvironmentAllowance<'a> {
    file: &'a str,
    line: usize,
    text: &'a str,
    reason: &'a str,
}

fn environment_allowances() -> Vec<EnvironmentAllowance<'static>> {
    include_str!("fixtures/git_environment_allowlist.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.splitn(4, '\t').collect();
            assert_eq!(fields.len(), 4, "{line}");
            EnvironmentAllowance {
                file: fields[0],
                line: fields[1].parse().unwrap(),
                text: fields[2],
                reason: fields[3],
            }
        })
        .collect()
}

fn environment_sites(source: &str) -> Vec<(usize, &str)> {
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    let mut offsets = Vec::new();
    for token in [
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_KEY_",
        "GIT_CONFIG_VALUE_",
        "env_clear",
        "envs",
        "os.environ.clear",
    ] {
        offsets.extend(compact.match_indices(token).filter_map(|(offset, _)| {
            let prefix = &compact[..offset];
            let inside_identifier = prefix.ends_with(|c: char| c.is_alphanumeric() || c == '_');
            let suffix = &compact[offset + token.len()..];
            let followed_by_identifier =
                suffix.starts_with(|c: char| c.is_alphanumeric() || c == '_');
            (token != "envs" || (!inside_identifier && !followed_by_identifier)).then_some(offset)
        }));
    }
    for (offset, _) in source.match_indices("env") {
        let tail = &source[offset + 3..];
        if tail.starts_with(char::is_whitespace) && tail.trim_start().starts_with("-i") {
            offsets.push(
                source[..offset]
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .map(char::len_utf8)
                    .sum(),
            );
        }
    }
    for (offset, _) in compact.match_indices("env:{") {
        let object = &compact[offset + 5..];
        let end = object.find('}').unwrap_or(object.len());
        if !object[..end].contains("...process.env") {
            offsets.push(offset);
        }
    }
    let mut start = 0;
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let end = start
                + line
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .map(char::len_utf8)
                    .sum::<usize>();
            let marked = offsets
                .iter()
                .any(|offset| *offset >= start && *offset < end);
            start = end;
            marked.then_some((index + 1, line.trim()))
        })
        .collect()
}

fn environment_violations(
    file: &str,
    source: &str,
    allowed: &[EnvironmentAllowance<'_>],
) -> Vec<String> {
    let sites = environment_sites(source);
    let mut offenders = Vec::new();
    for (line, text) in &sites {
        if !allowed.iter().any(|entry| {
            entry.file == file
                && entry.line == *line
                && entry.text == *text
                && !entry.reason.is_empty()
        }) {
            offenders.push(format!(
                "{file}:{line}: {text}: unreviewed environment token"
            ));
        }
    }
    for entry in allowed.iter().filter(|entry| entry.file == file) {
        if !sites.contains(&(entry.line, entry.text)) {
            offenders.push(format!(
                "{file}:{}: {}: allowance moved, changed or disappeared",
                entry.line, entry.text
            ));
        }
    }
    offenders
}

fn script_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            script_files(&path, files);
        } else if path.extension().is_some_and(|ext| {
            ["mjs", "js", "py", "sh", "bash"]
                .iter()
                .any(|name| ext == *name)
        }) {
            files.push(path);
        }
    }
}

#[test]
fn fixture_processes_preserve_the_maintenance_environment() {
    let (crates, mut files) = fixture_sources();
    let root = crates.parent().unwrap();
    for area in ["docs-portal/tests", "scripts", "tests"] {
        script_files(&root.join(area), &mut files);
    }
    let allowed = environment_allowances();
    let mut offenders = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for file in files {
        let name = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&file).unwrap();
        offenders.extend(environment_violations(&name, &source, &allowed));
        seen.insert(name);
    }
    for entry in &allowed {
        assert!(
            seen.contains(entry.file),
            "allowance names missing file {}",
            entry.file
        );
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[test]
fn environment_scan_refuses_unreviewed_changes() {
    for source in [
        "cmd.env_clear();",
        "Command::env_clear(&mut cmd);",
        r#"cmd.envs([("GIT_CONFIG_COUNT", "0")]);"#,
        r#"let key = "GIT_CONFIG_COUNT"; cmd.env_remove(key);"#,
        r#"spawnSync("git", args, { env: { GIT_CONFIG_COUNT: "0" } });"#,
        r#"os.environ["GIT_CONFIG_COUNT"] = "0""#,
        "os.environ.clear()",
        "env -i git commit",
        "cmd.envs(map);",
        "cmd.envs /* comment */ (map);",
        "Command::envs(&mut cmd, map);",
        "cmd.envs\n(map);",
        "spawn('git', [], { env:\n {} });",
        "// cmd.env_clear();",
    ] {
        assert!(
            !environment_violations("probe", source, &[]).is_empty(),
            "{source}"
        );
    }
    let original = r#"cmd.env("GIT_CONFIG_COUNT", inherited);"#;
    let allowance = [EnvironmentAllowance {
        file: "probe",
        line: 1,
        text: original,
        reason: "restores Cargo settings",
    }];
    assert!(environment_violations("probe", original, &allowance).is_empty());
    for source in [
        original.replace("inherited", "\"0\""),
        format!("\n{original}"),
        format!("{original}\n{original}"),
    ] {
        assert!(
            !environment_violations("probe", &source, &allowance).is_empty(),
            "{source}"
        );
    }
    for source in [
        r#"cmd.env("GIT_CONFIG_GLOBAL", config);"#,
        "cmd.env(\"PATH\", bin);",
        "spawn('git', [], { env: { ...process.env, PATH: bin } });",
    ] {
        assert!(environment_sites(source).is_empty(), "{source}");
    }
}

#[test]
fn environment_allowances_bind_the_actual_restoration_value() {
    let file = "crates/codeflow-cli/tests/ci_pin_platforms.rs";
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let source = std::fs::read_to_string(root.join(file)).unwrap();
    let allowed = environment_allowances();
    assert!(environment_violations(file, &source, &allowed).is_empty());
    let changed = source.replace("std::env::var(\"GIT_CONFIG_COUNT\").unwrap()", "\"0\"");
    assert_ne!(changed, source);
    let findings = environment_violations(file, &changed, &allowed);
    assert!(!findings.is_empty(), "restoring zero must be refused");
    assert!(findings.iter().all(|finding| finding.starts_with(file)));
}

#[test]
fn clone_options_consume_their_values() {
    for option in [
        "--template",
        "--reference",
        "--separate-git-dir",
        "-c",
        "--config",
        "-o",
        "-b",
        "--branch",
        "--depth",
        "--filter",
        "--origin",
        "-u",
        "--upload-pack",
        "-j",
        "--jobs",
        "--shallow-since",
        "--shallow-exclude",
        "--server-option",
        "--bundle-uri",
    ] {
        let source = format!(r#"cmd.args(["clone", "{option}", "--no-local", source]);"#);
        assert!(
            !unsafe_fixture_clones("probe", &source).is_empty(),
            "{source}"
        );
        let source =
            format!(r#"cmd.args(["clone", "{option}", "--local", "--no-local", source]);"#);
        assert!(
            unsafe_fixture_clones("probe", &source).is_empty(),
            "{source}"
        );
    }
}

#[test]
fn the_scan_refuses_each_git_spawn_its_subset_can_hide() {
    let refused = [
        r#"let c = Command::new("git");"#,
        "let c = std::process::Command\n    ::new(\n        \"git\"\n    );",
        r#"let c = Command::new("/usr/bin/git");"#,
        r#"let c = Command::new("git.exe");"#,
        r#"let c = Command::new(r"C:\Program Files\Git\cmd\git.exe");"#,
        "let c = Command::new(r#\"GIT\"#);",
        "const GIT: &str = \"git\"; let c = Command::new(GIT);",
        "let program = pick(); let c = Command::new(program);",
        "let c = match Command::new(program) {};",
        "use std::process::Command as Spawn;",
        // Round 2 (Codex F8): comments between tokens, escaped literals,
        // an alias, a qualified type, a macro and the constructor as a
        // value.
        "fn make() -> std::process::Command { std::process::Command /* spawn */ ::new(\"git\") }",
        r#"fn make() -> std::process::Command { std::process::Command::new("\x67it") }"#,
        r#"fn make() -> std::process::Command { std::process::Command::new("\u{67}it") }"#,
        "use std::process::Command /* builder */ as Spawn; fn make() -> Spawn { Spawn::new(\"git\") }",
        "type Spawn = std::process::Command; fn make() -> Spawn { Spawn::new(\"git\") }",
        r#"fn make() -> std::process::Command { <std::process::Command>::new("git") }"#,
        "macro_rules! launch { ($c:ident, $p:expr) => { $c::new($p) }; } \
         fn make() -> Command { launch!(Command, \"git\") }",
        r#"let make = Command::new; let c = make("git");"#,
        "let c = Command::new(\"gi\\\n    t\");",
        // Round 3 (Codex F8): escapes and spellings Rust accepts inside the
        // subset, each compiled by Codex to a Command whose program is git.
        "fn make() -> std::process::Command { std::process::Command::new(\"\\u{0_067}it\") }",
        "fn make() -> std::process::Command { std::process::Command::new(\"\\u{6_7}\\u{6_9}\\u{7_4}\") }",
        "fn make() -> std::process::Command { std::process::Command::new(\"gi\\\r\n    t\") }",
        "fn make() -> std::process::Command { std::process::Command::r#new(\"git\") }",
        "fn make() -> std::process::Command { <std::process::Command>::r#new(\"git\") }",
        "type Spawn = (std::process::Command); fn make() -> Spawn { Spawn::new(\"git\") }",
        "macro_rules! launch { ($c:ty, $p:expr) => { <$c>::new($p) }; } \
         fn make() -> std::process::Command { launch!(std::process::Command, \"git\") }",
        "fn make() -> r#Command { r#Command::new(\"git\") }",
        "fn make() -> Command { Command::<>::new(\"git\") }",
        // A literal the scan cannot decode is refused, not read as another
        // program.
        r#"let c = Command::new("\q");"#,
        r#"let c = Command::new("\u{d800}");"#,
        r#"let c = Command::new("\u{_67}it");"#,
    ];
    for source in refused {
        assert!(
            !violations("probe.rs", source).is_empty(),
            "the scan missed: {source}"
        );
    }
    let passed = [
        r#"let c = Command::new("gh");"#,
        r#"let c = Command::new("gitleaks");"#,
        r#"// Command::new("git")"#,
        r#"/* Command::new("git") */"#,
        r#"let s = "Command::new(\"git\")";"#,
        r#"let s = r"Command::new(program)";"#,
        r#"let c = MyCommand::new("git");"#,
        r#"let c = match Command::new("sh").status() {};"#,
        "let q = '\"'; let c = Command::new(\"sh\");",
        r#"let c = Command::new("gh") /* not git */;"#,
        "use std::process::{Command, Stdio}; fn f(c: &mut Command) -> Option<Command> { None }",
        "type Modes = BTreeMap<String, ModeCommand>;",
        r#"let s = "type X = Command; Command::new(\"git\")";"#,
        r#"let c = Command::new("my tool");"#,
        r#"let c = Command::new("\u{6_7}h");"#,
        "let c = Command::new(\"g\\\r\n    h\");",
        "type Pair = (Command, Command);",
        "fn f(c: &mut r#Command) {}",
    ];
    for source in passed {
        assert!(
            violations("probe.rs", source).is_empty(),
            "the scan refused: {source}: {:?}",
            violations("probe.rs", source)
        );
    }
}
