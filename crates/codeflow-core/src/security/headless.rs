//! Headless peer runs (TSK-136): Claude Code, Codex or Grok run one-shot,
//! without their interactive session.
//!
//! Peer seats run interactively, never headless (cf-delegate): a one-shot
//! headless run has no verified native session, no task tools or guards, and
//! no recheckable thread. This classifier finds such a run in a shell command
//! so exec-guard can refuse it (the default since ADR-0075 D4) or warn.
//!
//! The headless forms come from each CLI's own help (Claude Code 2.1.283,
//! Codex CLI 0.157.1, Grok 1.0.41):
//! - `claude -p`/`--print`, and `claude ultrareview`, which prints a review;
//! - `codex exec` (alias `e`) and `codex review`, both non-interactive;
//! - `grok -p`/`--single <PROMPT>` (also `--single=…` and `-p…`),
//!   `--prompt-file`, `--prompt-json` and `grok agent`.
//!
//! Each simple command is read the way git-guard reads it
//! (`git_guard::simple_commands`: control-structure bodies, groups, substitutions,
//! `bash -c` and `eval` strings, quotes and escapes), then the launch
//! wrappers are unwrapped (`nice`, `timeout`, `nohup`, `sudo`, `xargs`,
//! `find -exec`, a nested shell `-c`, `grok wrap`, …), and the peer's own
//! arguments are parsed by role: an option's value and anything after `--`
//! are never read as the headless flag. A help or version flag in an option
//! position (`claude --help -p`, `codex exec --help`) is no run, since the
//! CLI prints and exits (TSK-141); the same word as a value, a prompt or
//! after `--` is data. A line whose commands cannot be
//! resolved (a here-string, a substitution or variable as the program, an
//! alias, a shell reading its script from stdin) is judged on its raw text:
//! a peer name followed by one of its headless markers is flagged. That can
//! over-flag an unusual line, which the default `block` level then refuses;
//! the message says the line was not fully parsed, and a project that needs
//! such a run sets `security.headless_peer_runs` to `warn`.
//!
//! The raw text leaves out what the line only writes as data only when
//! the whole line is data (TSK-223): every simple command is a data command
//! written by its bare name (`cat`, `echo`, `grep`, `git commit`, `gh pr`
//! and the others `data_command` names) or `cd`, joined only by `;`,
//! `&&`, `||` or newlines, with no assignment, no other command, no
//! path-qualified program, no pipe, background job, subshell or process
//! substitution, no substitution other than a `$(cat <<EOF …)` message,
//! and no file written on a line that runs `git` or `gh`.
//! Then the heredoc bodies and the arguments it writes are left out, so
//! `grep -c review <<< 'Codex review: approve'` is no run. Any other line
//! keeps the 3.0.0 raw judgement of its whole text. The data left out is
//! still judged as a script, read with shell quoting: a peer counts only in
//! command position, with its headless flag, or with its headless
//! subcommand as the first word after its options, and a command the
//! script cannot resolve sends its whole text to the raw matcher.

use crate::hooks::git_guard::{command_argv, shell_tokens, simple_commands};

/// A headless peer run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessRun {
    /// The peer CLI: `claude`, `codex` or `grok`.
    pub peer: &'static str,
    /// The headless form matched, such as `claude -p` or `codex exec`.
    pub form: &'static str,
    /// `false` when the line could not be parsed and its raw text named the
    /// peer with a headless marker.
    pub parsed: bool,
}

/// The first headless peer run in `command`, if any.
#[must_use]
pub fn headless_peer_run(command: &str) -> Option<HeadlessRun> {
    find(command, 0)
}

/// What one simple command is.
enum Found {
    Run(HeadlessRun),
    /// It runs something this classifier cannot resolve.
    Unresolved,
    Nothing,
}

fn find(command: &str, depth: usize) -> Option<HeadlessRun> {
    if depth > 4 {
        return raw_run(command);
    }
    let mut unresolved = command.contains("<<<");
    for argv in simple_commands(command) {
        match classify(&argv, depth) {
            Found::Run(run) => return Some(run),
            Found::Unresolved => unresolved = true,
            Found::Nothing => {}
        }
    }
    if unresolved {
        fallback_run(command)
    } else {
        None
    }
}

/// The raw-text judgement of a line that could not be resolved: the line
/// without the data it only writes, judged as [`raw_run`] always has, and
/// that data judged as a script the line may run later (TSK-223).
fn fallback_run(command: &str) -> Option<HeadlessRun> {
    let (code, data) = split_data(command);
    raw_run(&code).or_else(|| data.iter().find_map(|text| data_run(text, 0)))
}

/// Programs whose arguments, here-strings and heredoc bodies are data: they
/// never run what they read or are given. Tools that can run their input
/// or a program they name (`awk` `system()`, GNU `sed e`, `sort
/// --compress-program`, `rg --pre`, interpreters, shells) are left out.
const DATA_COMMANDS: &[&str] = &[
    "cat", "tee", "echo", "printf", "grep", "egrep", "fgrep", "head", "tail", "wc", "uniq", "tr",
    "cut", "diff", "jq", "base64", "true", ":",
];

/// Whether a simple command's words are a data command, written as the
/// program itself with no assignment or launcher before it: one of
/// [`DATA_COMMANDS`] (`printf -v` assigns, so it is not), `git commit`,
/// `tag` or `notes` with no global option but `-C <dir>` (a `-c` in any
/// spelling can make the subcommand an alias that runs a shell) and no
/// `-e`/`--edit` (which opens the editor), or `gh` in a subcommand that
/// takes a body. Returns whether its arguments are also judged as a
/// script: a message never is.
fn data_command(words: &[String]) -> Option<bool> {
    let (program, args) = words.split_first()?;
    match basename(program).as_str() {
        "git" => {
            let mut at = 0;
            while args.get(at).is_some_and(|a| a == "-C") {
                at += 2;
            }
            let sub = args.get(at)?;
            let edits = args[at..].iter().any(|a| {
                a == "--edit" || (a.starts_with('-') && !a.starts_with("--") && a.contains('e'))
            });
            (!edits && matches!(sub.as_str(), "commit" | "tag" | "notes")).then_some(false)
        }
        "gh" => args
            .first()
            .is_some_and(|sub| matches!(sub.as_str(), "pr" | "issue" | "release" | "gist"))
            .then_some(false),
        "printf" if args.iter().any(|a| a.starts_with("-v")) => None,
        name if DATA_COMMANDS.contains(&name) => Some(true),
        _ => None,
    }
}

/// A heredoc opened in the current simple command, waiting for its body.
struct PendingDoc {
    delimiter: String,
    strip_tabs: bool,
    /// An unquoted delimiter: the body's substitutions run.
    expands: bool,
    /// The simple command that reads it.
    owner: usize,
}

/// One simple command of the line, as [`split_data`] read it.
struct Segment {
    text: String,
    /// `Some(judge_args)` when it is a data command written by its bare
    /// name that feeds no pipe and runs no substitution of its own.
    data: Option<bool>,
    /// No words, a data command, or `cd` with no substitution.
    plain: bool,
    /// It writes a file: an output redirection, or `tee`, `uniq` or
    /// `base64`, which can name an output file.
    writes: bool,
    /// `git` or `gh`, which run hooks, a configured fsmonitor and other
    /// programs the repository's files name.
    runs_repository_programs: bool,
}

/// The line in order: text that always stays in the code, a simple
/// command, or a heredoc body with the command that reads it.
enum Piece {
    Code(String),
    Segment(usize),
    Body {
        owner: usize,
        text: String,
        /// Closed, and runs no substitution.
        quiet: bool,
    },
}

/// What [`split_data`] learns while it reads the line.
#[derive(Default)]
struct LineShape {
    segments: Vec<Segment>,
    pieces: Vec<Piece>,
    /// A pipe, a background job, a subshell or a process substitution:
    /// commands joined other than by `;`, `&&`, `||` or a newline.
    joined_otherwise: bool,
}

impl LineShape {
    /// Close the simple command in `span`. Its words are read as written,
    /// reserved words included, so `if`, `while`, `{` or `fi` is no plain
    /// command.
    fn flush(
        &mut self,
        span: &mut String,
        runs: &mut bool,
        writes: &mut bool,
        docs: &mut [PendingDoc],
        piped: bool,
    ) {
        let words = command_argv(span);
        let program = words.first().map(|word| basename(word));
        let bare = words
            .first()
            .is_none_or(|program| !program.contains(['/', '\\']));
        let data = data_command(&words).filter(|_| bare && !piped && !*runs);
        let plain = words.is_empty()
            || data.is_some()
            || (words.first().is_some_and(|w| w == "cd") && !*runs && !piped);
        let owner = self.segments.len();
        for doc in docs.iter_mut().filter(|doc| doc.owner == usize::MAX) {
            doc.owner = owner;
        }
        self.segments.push(Segment {
            text: std::mem::take(span),
            data,
            plain,
            writes: *writes
                || program
                    .as_deref()
                    .is_some_and(|name| matches!(name, "tee" | "uniq" | "base64")),
            runs_repository_programs: program
                .as_deref()
                .is_some_and(|name| matches!(name, "git" | "gh")),
        });
        self.pieces.push(Piece::Segment(owner));
        *runs = false;
        *writes = false;
    }

    /// Whether the line is data only: every simple command is plain, they
    /// are joined only by `;`, `&&`, `||` or newlines, and no file is
    /// written on a line that runs `git` or `gh` (a written hook or
    /// configuration would run). Only then is its data left out; any other
    /// line keeps its whole raw text.
    fn data_only(&self) -> bool {
        let writes = self.segments.iter().any(|segment| segment.writes);
        let repository = self
            .segments
            .iter()
            .any(|segment| segment.runs_repository_programs);
        let exposed = self.joined_otherwise || (writes && repository);
        !exposed && self.segments.iter().all(|segment| segment.plain)
    }
}

/// Split `command` into the text that can run and the data it only
/// writes. Data is left out only when the line is data only
/// ([`LineShape::data_only`]): then each data command's text, and its
/// heredoc bodies unless an unquoted body runs a substitution, is data. The data returned is each judged
/// piece: a body, and a data command's arguments, joined and one by one,
/// unless they are a message. Quotes, `$(…)` and backticks are tracked so
/// a separator inside them splits nothing; anything this reader cannot
/// place stays in the code.
#[allow(clippy::too_many_lines)] // one character state machine
fn split_data(command: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = command.chars().collect();
    let mut shape = LineShape::default();
    let mut span = String::new();
    // Whether the span runs a substitution this reader does not clear.
    let mut runs = false;
    // Whether the span redirects output to a file.
    let mut writes = false;
    let mut docs: Vec<PendingDoc> = Vec::new();
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(open) = quote {
            if c == '`' && open == '"' {
                i = backtick(&chars, i, &mut span, &mut runs);
                continue;
            }
            span.push(c);
            if c == '\\' && open == '"' {
                if let Some(&next) = chars.get(i + 1) {
                    span.push(next);
                    i += 2;
                    continue;
                }
            }
            if c == open {
                quote = None;
            } else if open == '"' && c == '$' && chars.get(i + 1) == Some(&'(') {
                i = substitution(&chars, i, &mut span, &mut runs);
                continue;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => {
                span.push(c);
                if let Some(&next) = chars.get(i + 1) {
                    span.push(next);
                    i += 1;
                }
            }
            '`' => {
                i = backtick(&chars, i, &mut span, &mut runs);
                continue;
            }
            '\'' | '"' => {
                quote = Some(c);
                span.push(c);
            }
            '$' if chars.get(i + 1) == Some(&'(') => {
                i = substitution(&chars, i, &mut span, &mut runs);
                continue;
            }
            '<' if chars.get(i + 1) == Some(&'<')
                && chars.get(i + 2) != Some(&'<')
                && (i == 0 || chars[i - 1] != '<') =>
            {
                let (doc, end) = heredoc_operator(&chars, i);
                span.extend(&chars[i..end]);
                docs.extend(doc);
                i = end;
                continue;
            }
            '<' | '>' if chars.get(i + 1) == Some(&'(') => {
                // A process substitution runs its text, beside the command.
                runs = true;
                shape.joined_otherwise = true;
                span.push(c);
            }
            '>' => {
                writes = true;
                span.push(c);
            }
            '&' if matches!(chars.get(i + 1), Some('>')) => span.push(c),
            '&' if i > 0 && matches!(chars[i - 1], '>' | '<') => span.push(c),
            ';' | '&' | '|' | '(' | ')' | '\n' => {
                let prev = i.checked_sub(1).map(|at| chars[at]);
                let next = chars.get(i + 1).copied();
                let piped = c == '|' && next != Some('|') && prev != Some('|');
                let background = c == '&' && next != Some('&') && !matches!(prev, Some('&' | '|'));
                if piped || background || matches!(c, '(' | ')') {
                    shape.joined_otherwise = true;
                }
                shape.flush(&mut span, &mut runs, &mut writes, &mut docs, piped);
                shape.pieces.push(Piece::Code(c.to_string()));
                if c == '\n' && !docs.is_empty() {
                    i = heredoc_bodies(&chars, i + 1, &mut docs, &mut shape.pieces);
                    continue;
                }
            }
            _ => span.push(c),
        }
        i += 1;
    }
    shape.flush(&mut span, &mut runs, &mut writes, &mut docs, false);
    // A heredoc with no body line yet: its operator stays in the code.
    let data_only = shape.data_only();
    let removed: Vec<bool> = shape
        .segments
        .iter()
        .map(|segment| data_only && segment.data.is_some())
        .collect();
    let mut code = String::new();
    let mut data: Vec<String> = Vec::new();
    for piece in shape.pieces {
        match piece {
            Piece::Code(text) => code.push_str(&text),
            Piece::Segment(at) => {
                let segment = &shape.segments[at];
                if !removed[at] {
                    code.push_str(&segment.text);
                    continue;
                }
                if segment.data == Some(true) {
                    // Every word, a here-string's included, as written.
                    let tokens = shell_tokens(&segment.text);
                    data.push(tokens.join(" "));
                    data.extend(tokens);
                }
                code.push(' ');
            }
            Piece::Body { owner, text, quiet } => {
                if removed[owner] && quiet {
                    data.push(text);
                } else {
                    code.push_str(&text);
                }
            }
        }
    }
    (code, data)
}

/// Consume the backtick substitution at `start` into `span`, returning
/// the index after its closing backtick. It runs.
fn backtick(chars: &[char], start: usize, span: &mut String, runs: &mut bool) -> usize {
    *runs = true;
    span.push('`');
    let mut i = start + 1;
    while i < chars.len() {
        let c = chars[i];
        span.push(c);
        if c == '\\' {
            if let Some(&next) = chars.get(i + 1) {
                span.push(next);
                i += 2;
                continue;
            }
        }
        i += 1;
        if c == '`' {
            return i;
        }
    }
    chars.len()
}

/// Consume the `$(…)` at `start` into `span`, returning the index after
/// it. A `$(cat <<WORD …)` whose body cannot expand is a message, as
/// `git commit -m "$(cat <<'EOF' …)"` writes one; any other substitution
/// runs.
fn substitution(chars: &[char], start: usize, span: &mut String, runs: &mut bool) -> usize {
    if let Some(end) = cat_heredoc_substitution(chars, start) {
        span.extend(&chars[start..end]);
        return end;
    }
    *runs = true;
    span.push('$');
    let mut level = 0usize;
    let mut quote: Option<char> = None;
    let mut i = start + 1;
    while i < chars.len() {
        let c = chars[i];
        span.push(c);
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => level += 1,
            (None, ')') => {
                level = level.saturating_sub(1);
                if level == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

/// The end of a `$(cat <<WORD …)` substitution at `start` whose body
/// cannot expand (a quoted delimiter, or no `$` or backtick in the body),
/// or `None` for any other text.
fn cat_heredoc_substitution(chars: &[char], start: usize) -> Option<usize> {
    let text: String = chars[start..].iter().collect();
    let rest = text.strip_prefix("$(")?.trim_start();
    let rest = rest.strip_prefix("cat")?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix("<<")?;
    let rest = rest
        .strip_prefix('-')
        .unwrap_or(rest)
        .trim_start_matches([' ', '\t']);
    let open = rest.chars().next()?;
    let (delimiter, quoted, after) = if matches!(open, '\'' | '"') {
        let inner = &rest[1..];
        let end = inner.find(open)?;
        (&inner[..end], true, &inner[end + 1..])
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(rest.len());
        (&rest[..end], false, &rest[end..])
    };
    if delimiter.is_empty() {
        return None;
    }
    let (first, body) = after.split_once('\n')?;
    if !first.trim().is_empty() {
        return None;
    }
    let mut offset = text.len() - body.len();
    for line in body.split_inclusive('\n') {
        offset += line.len();
        if line.trim() == delimiter {
            let tail = &text[offset..];
            let close = tail.find(|c: char| !c.is_whitespace())?;
            if !tail[close..].starts_with(')') {
                return None;
            }
            let content = &text[text.len() - body.len()..offset];
            if !quoted && (content.contains("$(") || content.contains('`')) {
                return None;
            }
            let end_bytes = offset + close + 1;
            return Some(start + text[..end_bytes].chars().count());
        }
    }
    None
}

/// Parse the `<<`/`<<-` operator at `start` and its delimiter word: the
/// heredoc, unless no delimiter follows, and the index after the word.
fn heredoc_operator(chars: &[char], start: usize) -> (Option<PendingDoc>, usize) {
    let mut i = start + 2;
    let strip_tabs = chars.get(i) == Some(&'-');
    if strip_tabs {
        i += 1;
    }
    while chars.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
        i += 1;
    }
    let mut delimiter = String::new();
    let mut expands = true;
    while let Some(&c) = chars.get(i) {
        match c {
            '\'' | '"' => {
                expands = false;
                let close = chars[i + 1..].iter().position(|x| *x == c);
                let Some(close) = close else { break };
                delimiter.extend(&chars[i + 1..i + 1 + close]);
                i += close + 2;
            }
            '\\' => {
                expands = false;
                i += 1;
            }
            c if c.is_whitespace() || matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')') => break,
            c => {
                delimiter.push(c);
                i += 1;
            }
        }
    }
    let doc = (!delimiter.is_empty()).then_some(PendingDoc {
        delimiter,
        strip_tabs,
        expands,
        owner: usize::MAX,
    });
    (doc, i)
}

/// Consume the bodies of `pending`, in order, from `start`, into
/// `pieces`, each with the command that reads it. Returns the index after
/// the last body read.
fn heredoc_bodies(
    chars: &[char],
    start: usize,
    pending: &mut Vec<PendingDoc>,
    pieces: &mut Vec<Piece>,
) -> usize {
    let mut i = start;
    for doc in pending.drain(..) {
        let mut body = String::new();
        let mut closed = false;
        while i < chars.len() {
            let end = chars[i..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |at| i + at);
            let line: String = chars[i..end].iter().collect();
            i = (end + 1).min(chars.len());
            let read = if doc.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            if read == doc.delimiter {
                closed = true;
                break;
            }
            body.push_str(&line);
            body.push('\n');
        }
        let runs = doc.expands && (body.contains("$(") || body.contains('`'));
        pieces.push(Piece::Body {
            owner: doc.owner,
            text: body,
            quiet: closed && !runs,
        });
        pieces.push(Piece::Code("\n".to_string()));
    }
    i
}

/// Judge data as a script the line may run later. Each simple command is
/// read with shell quoting; a peer in command position counts only as
/// its CLI parses the words after it ([`raw_marker`]), and any other
/// command is classified as usual.
fn data_run(text: &str, depth: usize) -> Option<HeadlessRun> {
    if depth > 4 {
        return raw_run(text);
    }
    simple_commands(text).iter().find_map(|argv| {
        let unparsed = |run: HeadlessRun| HeadlessRun {
            parsed: false,
            ..run
        };
        let cli = match basename(argv.first()?).as_str() {
            "claude" => &CLAUDE,
            "codex" => &CODEX,
            "grok" => &GROK,
            _ => {
                return match classify(argv, depth + 1) {
                    Found::Run(run) => Some(unparsed(run)),
                    // A program the data cannot resolve: its whole text is
                    // read as the 3.0.0 raw matcher reads a line.
                    Found::Unresolved => raw_run(text),
                    Found::Nothing => None,
                };
            }
        };
        raw_marker(cli, &argv[1..]).map(|form| HeadlessRun {
            peer: cli.peer,
            form,
            parsed: false,
        })
    })
}

/// The headless form the words after a peer name start, read the way its
/// CLI parses them. The headless subcommand counts only as the first
/// positional word; a headless flag counts before or after it, as the CLIs
/// read flags anywhere. An option the grammar does not know may take the
/// next word as its value, unless that word is the headless subcommand.
/// Help is not honoured: data is judged conservatively.
fn raw_marker(cli: &Cli, words: &[String]) -> Option<&'static str> {
    let short_form = cli.headless_long.first().map_or("", |(_, form)| *form);
    let subcommand = |word: &str| {
        cli.headless_subcommands
            .iter()
            .find(|(sub, _)| *sub == word)
            .map(|(_, form)| *form)
    };
    let mut positional = false;
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        if word == "--" {
            return None;
        }
        // Whether this option takes the next word as its value.
        let mut value = false;
        let mut known = false;
        if word.starts_with("--") {
            let (flag, inline) = word
                .split_once('=')
                .map_or((word.as_str(), false), |(flag, _)| (flag, true));
            if let Some((_, form)) = cli.headless_long.iter().find(|(long, _)| *long == flag) {
                return Some(form);
            }
            known = inline || cli.long_value.contains(&flag);
            value = !inline && known;
        } else if let Some(cluster) = word.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (index, flag) in cluster.char_indices() {
                if cli.headless_short.contains(flag) {
                    return Some(short_form);
                }
                if cli.short_value.contains(flag) {
                    known = true;
                    value = cluster[index + flag.len_utf8()..].is_empty();
                    break;
                }
            }
            known = known
                || cluster
                    .chars()
                    .all(|flag| cli.short_optional.contains(flag));
        } else {
            if !positional {
                if let Some(form) = subcommand(word) {
                    return Some(form);
                }
            }
            positional = true;
            continue;
        }
        let next = words.get(at).filter(|next| !next.starts_with('-'));
        if value {
            at += 1;
        } else if let Some(next) = next.filter(|_| !known && !positional) {
            // An unknown option: its value, unless it is the subcommand.
            if let Some(form) = subcommand(next) {
                return Some(form);
            }
            at += 1;
        }
    }
    None
}

/// Classify one simple command's argument vector, unwrapping launchers.
fn classify(argv: &[String], depth: usize) -> Found {
    let mut argv = argv;
    for _ in 0..8 {
        let Some(program) = argv.first() else {
            return Found::Nothing;
        };
        // A variable or substitution names the program; git-guard's reader
        // marks a substitution with a control character.
        if program.contains(['$', '`']) || program.chars().any(char::is_control) {
            return Found::Unresolved;
        }
        let name = basename(program);
        let rest = &argv[1..];
        argv = match name.as_str() {
            "claude" | "codex" | "grok" => return peer_run(&name, rest, depth),
            "alias" | "function" | "source" | "." => return Found::Unresolved,
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" => {
                return match shell_command_string(rest) {
                    Some(inner) => find(inner, depth + 1).map_or(Found::Nothing, Found::Run),
                    // A script file, or a script read from stdin.
                    None => Found::Unresolved,
                };
            }
            "env" => match skip_env(rest) {
                Some(rest) => rest,
                None => return Found::Unresolved,
            },
            "command" | "builtin" | "exec" | "nohup" | "setsid" | "caffeinate" | "unbuffer"
            | "time" => skip_options(rest, &["-a"]),
            "nice" => skip_options(rest, &["-n", "--adjustment"]),
            "stdbuf" => skip_options(rest, &["-i", "-o", "-e"]),
            "timeout" => {
                let after = skip_options(rest, &["-s", "--signal", "-k", "--kill-after"]);
                after.get(1..).unwrap_or_default()
            }
            "sudo" | "doas" => skip_assignments(skip_options(
                rest,
                &[
                    "-u",
                    "--user",
                    "-g",
                    "--group",
                    "-C",
                    "--close-from",
                    "-D",
                    "--chdir",
                    "-h",
                    "--host",
                    "-p",
                    "--prompt",
                    "-r",
                    "--role",
                    "-t",
                    "--type",
                    "-T",
                    "--command-timeout",
                    "-U",
                    "--other-user",
                ],
            )),
            "xargs" => skip_options(
                rest,
                &[
                    "-I",
                    "-L",
                    "-n",
                    "-P",
                    "-s",
                    "-d",
                    "-E",
                    "-a",
                    "--arg-file",
                    "--delimiter",
                    "--eof",
                    "--max-lines",
                    "--max-rest",
                    "--max-procs",
                    "--max-chars",
                    "--replace",
                ],
            ),
            "find" => return find_exec(rest, depth),
            // A package runner starts the package's binary (TSK-141 AC-6).
            // pnpm 11 also answers to `pn` and `pnx`.
            "npx" | "bunx" | "pnpx" | "pnx" => return package_run(rest, depth),
            "pn" => return runner("pnpm", rest, depth),
            "npm" | "pnpm" | "yarn" | "bun" => return runner(&name, rest, depth),
            _ => return Found::Nothing,
        };
    }
    Found::Unresolved
}

/// Options package runners take without a value.
const RUNNER_FLAGS: &[&str] = &[
    "-y",
    "--yes",
    "--no",
    "-q",
    "--quiet",
    "-s",
    "--silent",
    "--bun",
    "--ignore-existing",
    "--prefer-offline",
    "--prefer-online",
    "--offline",
    "--no-install",
    "--workspaces",
    "--ws",
    "--include-workspace-root",
    "--legacy-peer-deps",
    "--foreground-scripts",
    "--verbose",
    "--stream",
];

/// Options package runners take with a value in the next word.
const RUNNER_VALUES: &[&str] = &[
    "--cache",
    "--cache-folder",
    "--workspace",
    "--prefix",
    "--userconfig",
    "--globalconfig",
    "--registry",
    "--node-options",
    "--script-shell",
    "--loglevel",
    "--cwd",
    "--dir",
    "-C",
    "--filter",
    "-F",
    "--reporter",
];

/// The runner's own help or version: it prints and exits.
fn runner_exits(arg: &str) -> bool {
    matches!(arg, "-h" | "--help" | "-v" | "--version")
}

/// Where the first word after a runner's options may be. An option the
/// grammar does not know may or may not take the next word, so both are
/// kept, though never an option word, which it does not take (npm reads
/// `--unknown --help` as help); a help or version option ends the run.
pub(crate) fn after_runner_options(args: &[String]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![0];
    let mut steps = 0;
    while let Some(at) = stack.pop() {
        steps += 1;
        if steps > 64 {
            break;
        }
        match args.get(at) {
            None => {}
            Some(arg) if arg == "--" => out.push(at + 1),
            Some(arg) if !arg.starts_with('-') || arg == "-" => out.push(at),
            Some(arg) if runner_exits(arg) => {}
            Some(arg) if arg.contains('=') || RUNNER_FLAGS.contains(&arg.as_str()) => {
                stack.push(at + 1);
            }
            Some(arg) if RUNNER_VALUES.contains(&arg.as_str()) => stack.push(at + 2),
            Some(_) => {
                stack.push(at + 1);
                if takes_next(args, at) {
                    stack.push(at + 2);
                }
            }
        }
    }
    out
}

/// `npm`, `pnpm`, `yarn` or `bun` with a subcommand that runs a package or
/// a local binary: `npm exec`/`npm x`, `pnpm dlx`/`yarn dlx`, `bun x`, and
/// `pnpm exec`/`yarn exec` or `pnpm BIN`/`yarn BIN` for an installed peer.
fn runner(name: &str, args: &[String], depth: usize) -> Found {
    let mut found = Found::Nothing;
    for at in after_runner_options(args) {
        let Some((sub, tail)) = args.get(at..).and_then(<[String]>::split_first) else {
            continue;
        };
        let next = match (name, sub.as_str()) {
            ("npm", "exec" | "x") | ("pnpm" | "yarn", "dlx") | ("bun", "x") => {
                package_run(tail, depth)
            }
            ("pnpm" | "yarn", "exec") => after_runner_options(tail)
                .into_iter()
                .map(|start| classify(tail.get(start..).unwrap_or_default(), depth + 1))
                .fold(Found::Nothing, strongest),
            ("pnpm" | "yarn", "claude" | "codex" | "grok") => classify(&args[at..], depth + 1),
            _ => Found::Nothing,
        };
        found = strongest(found, next);
        if matches!(found, Found::Run(_)) {
            break;
        }
    }
    found
}

/// Whether an option the grammar does not know may take the word after
/// it as its value: not when that word is an option itself.
fn takes_next(args: &[String], at: usize) -> bool {
    args.get(at + 1)
        .is_some_and(|next| !next.starts_with('-') || next == "-")
}

/// A run over an unresolved command over nothing.
fn strongest(a: Found, b: Found) -> Found {
    match (a, b) {
        (Found::Run(run), _) | (_, Found::Run(run)) => Found::Run(run),
        (Found::Unresolved, _) | (_, Found::Unresolved) => Found::Unresolved,
        _ => Found::Nothing,
    }
}

/// A package runner's command (`npx`, `bunx`, `npm exec`, `pnpm dlx`,
/// `yarn dlx`, `bun x`): after its options, the package's binary, or with
/// `--package` the command named after it; `-c` or `--call` runs a shell
/// string. An option the grammar does not know is read both with and
/// without a value, and the runner's help or version runs nothing.
fn package_run(args: &[String], depth: usize) -> Found {
    if depth > 4 {
        return Found::Unresolved;
    }
    let call = |script: &str| find(script, depth + 1).map_or(Found::Nothing, Found::Run);
    let mut found = Found::Nothing;
    let mut stack = vec![(0usize, false)];
    let mut steps = 0;
    while let Some((at, package_given)) = stack.pop() {
        steps += 1;
        if steps > 64 {
            return strongest(found, Found::Unresolved);
        }
        let Some(arg) = args.get(at) else {
            continue;
        };
        let next = if arg == "--" {
            launch(args, at + 1, package_given, depth)
        } else if !arg.starts_with('-') || arg == "-" {
            launch(args, at, package_given, depth)
        } else if let Some((option, value)) = arg.split_once('=') {
            match option {
                "-c" | "--call" => call(value),
                "-p" | "--package" => {
                    stack.push((at + 1, true));
                    continue;
                }
                _ => {
                    stack.push((at + 1, package_given));
                    continue;
                }
            }
        } else {
            match arg.as_str() {
                "-c" | "--call" => match args.get(at + 1) {
                    Some(script) => call(script),
                    None => continue,
                },
                "-p" | "--package" => {
                    stack.push((at + 2, true));
                    continue;
                }
                flag if runner_exits(flag) => continue,
                flag if RUNNER_FLAGS.contains(&flag) => {
                    stack.push((at + 1, package_given));
                    continue;
                }
                option if RUNNER_VALUES.contains(&option) => {
                    stack.push((at + 2, package_given));
                    continue;
                }
                _ => {
                    stack.push((at + 1, package_given));
                    if takes_next(args, at) {
                        stack.push((at + 2, package_given));
                    }
                    continue;
                }
            }
        };
        found = strongest(found, next);
        if matches!(found, Found::Run(_)) {
            return found;
        }
    }
    found
}

/// The package (or, with `--package`, the command) at `args[at]`, run with
/// the words after it.
fn launch(args: &[String], at: usize, package_given: bool, depth: usize) -> Found {
    let Some((spec, rest)) = args.get(at..).and_then(<[String]>::split_first) else {
        return Found::Nothing;
    };
    let program = if package_given {
        spec.clone()
    } else {
        package_bin(spec)
    };
    let mut command = vec![program];
    command.extend(rest.iter().cloned());
    classify(&command, depth + 1)
}

/// The binary a package spec runs: its name without scope or version,
/// with the peers' package names read as their commands.
pub(crate) fn package_bin(spec: &str) -> String {
    let unversioned = match spec.strip_prefix('@') {
        Some(scoped) => format!("@{}", scoped.split('@').next().unwrap_or(scoped)),
        None => spec.split('@').next().unwrap_or(spec).to_string(),
    };
    let name = unversioned.rsplit('/').next().unwrap_or(&unversioned);
    match name {
        "claude-code" => "claude".to_string(),
        "grok-cli" => "grok".to_string(),
        other => other.to_string(),
    }
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` or `-okdir`.
fn find_exec(args: &[String], depth: usize) -> Found {
    let mut rest = args;
    let mut found = Found::Nothing;
    while let Some(at) = rest
        .iter()
        .position(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
    {
        let body = &rest[at + 1..];
        let end = body
            .iter()
            .position(|a| a == ";" || a == "+")
            .unwrap_or(body.len());
        match classify(&body[..end], depth) {
            Found::Run(run) => return Found::Run(run),
            Found::Unresolved => found = Found::Unresolved,
            Found::Nothing => {}
        }
        rest = &body[end..];
    }
    found
}

/// Skip a wrapper's options (those in `with_value` take the next token) to
/// the command it runs.
pub(crate) fn skip_options<'a>(args: &'a [String], with_value: &[&str]) -> &'a [String] {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            return &args[at + 1..];
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
    args.get(at..).unwrap_or_default()
}

/// Skip `NAME=value` words.
pub(crate) fn skip_assignments(args: &[String]) -> &[String] {
    let at = args
        .iter()
        .position(|arg| !is_assignment(arg))
        .unwrap_or(args.len());
    &args[at..]
}

/// `env`'s options and assignments up to its command; `None` for `env -S`,
/// which splits a string this classifier does not model.
pub(crate) fn skip_env(args: &[String]) -> Option<&[String]> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "-S" || arg.starts_with("--split-string") || arg.starts_with("-S") {
            return None;
        }
        if arg == "-u" || arg == "--unset" || arg == "-C" || arg == "--chdir" {
            at += 2;
        } else if arg.starts_with('-') || is_assignment(arg) {
            at += 1;
        } else {
            break;
        }
    }
    Some(args.get(at..).unwrap_or_default())
}

/// The string after `-c` (or a cluster holding `c`, such as `-lc`).
pub(crate) fn shell_command_string(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|arg| {
        arg == "-c"
            || arg == "--command"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
                && arg.contains('c'))
    })?;
    args.get(at + 1).map(String::as_str)
}

/// How a peer CLI reads its arguments, from its `--help`.
struct Cli {
    peer: &'static str,
    /// Short flags that mean a headless run.
    headless_short: &'static str,
    /// Whether the headless short flag takes a value (`grok -p <PROMPT>`).
    headless_short_value: bool,
    /// Short flags that print help or the version and exit, as
    /// `--help` and `--version` do.
    help_short: &'static str,
    /// Long flags (boolean or taking a value) that mean a headless run, with
    /// the form reported.
    headless_long: &'static [(&'static str, &'static str)],
    /// Short options taking a required value.
    short_value: &'static str,
    /// Short options taking an optional value.
    short_optional: &'static str,
    /// Long options taking a required value.
    long_value: &'static [&'static str],
    /// Long options taking an optional value.
    long_optional: &'static [&'static str],
    /// Long options taking every following non-option word.
    long_variadic: &'static [&'static str],
    /// Subcommands that run headless, with the form reported.
    headless_subcommands: &'static [(&'static str, &'static str)],
    /// Subcommands that wrap another command (`grok wrap <command>`).
    wrap_subcommands: &'static [&'static str],
    /// Other subcommands: their arguments are not the peer's.
    subcommands: &'static [&'static str],
}

const CLAUDE: Cli = Cli {
    peer: "claude",
    headless_short: "p",
    headless_short_value: false,
    help_short: "hv",
    headless_long: &[("--print", "claude -p")],
    short_value: "n",
    short_optional: "drw",
    long_value: &[
        "--agent",
        "--agents",
        "--append-system-prompt",
        "--append-system-prompt-file",
        "--autocompact",
        "--client-data-url",
        "--debug-file",
        "--effort",
        "--environment",
        "--fallback-model",
        "--input-format",
        "--json-schema",
        "--max-budget-usd",
        "--model",
        "--name",
        "--output-format",
        "--permission-mode",
        "--permission-prompts",
        "--plugin-dir",
        "--plugin-url",
        "--remote-control-session-name-prefix",
        "--session-id",
        "--setting-sources",
        "--settings",
        "--system-prompt",
        "--system-prompt-file",
        "--system-prompt-snapshot",
    ],
    long_optional: &[
        "--cloud",
        "--debug",
        "--from-pr",
        "--prompt-suggestions",
        "--remote-control",
        "--resume",
        "--teleport",
        "--worktree",
    ],
    long_variadic: &[
        "--add-dir",
        "--allowedTools",
        "--allowed-tools",
        "--betas",
        "--disallowedTools",
        "--disallowed-tools",
        "--file",
        "--mcp-config",
        "--tools",
    ],
    headless_subcommands: &[("ultrareview", "claude ultrareview")],
    wrap_subcommands: &[],
    subcommands: &[
        "agents",
        "attach",
        "auth",
        "auto-mode",
        "doctor",
        "gateway",
        "import",
        "install",
        "logs",
        "mcp",
        "plugin",
        "plugins",
        "project",
        "respawn",
        "rm",
        "setup-token",
        "stop",
        "kill",
        "update",
        "upgrade",
    ],
};

const CODEX: Cli = Cli {
    peer: "codex",
    headless_short: "",
    headless_short_value: false,
    help_short: "hV",
    headless_long: &[],
    short_value: "cimpsCao",
    short_optional: "",
    long_value: &[
        "--config",
        "--enable",
        "--disable",
        "--remote",
        "--remote-auth-token-env",
        "--image",
        "--model",
        "--local-provider",
        "--profile",
        "--sandbox",
        "--cd",
        "--add-dir",
        "--ask-for-approval",
        // `codex exec` and `codex review` options.
        "--output-schema",
        "--color",
        "--output-last-message",
        "--thread-source",
        "--base",
        "--commit",
        "--title",
    ],
    long_optional: &[],
    long_variadic: &[],
    headless_subcommands: &[
        ("exec", "codex exec"),
        ("e", "codex exec"),
        ("review", "codex review"),
    ],
    wrap_subcommands: &[],
    subcommands: &[
        "agents",
        "login",
        "logout",
        "mcp",
        "plugin",
        "app-server",
        "remote-control",
        "app",
        "completion",
        "update",
        "doctor",
        "sandbox",
        "debug",
        "apply",
        "a",
        "resume",
        "queue",
        "archive",
        "delete",
        "migrate-rollouts",
        "unarchive",
        "fork",
        "cloud",
        "exec-server",
        "features",
        "help",
    ],
};

const GROK: Cli = Cli {
    peer: "grok",
    headless_short: "p",
    headless_short_value: true,
    help_short: "hv",
    headless_long: &[
        ("--single", "grok -p"),
        ("--prompt-file", "grok --prompt-file"),
        ("--prompt-json", "grok --prompt-json"),
    ],
    short_value: "ms",
    short_optional: "rw",
    long_value: &[
        "--single",
        "--prompt-file",
        "--prompt-json",
        "--agent",
        "--agents",
        "--allow",
        "--allowedTools",
        "--cwd",
        "--debug-file",
        "--deny",
        "--disallowedTools",
        "--disallowed-tools",
        "--json-schema",
        "--leader-socket",
        "--model",
        "--max-turns",
        "--output-format",
        "--permission-mode",
        "--reasoning-effort",
        "--effort",
        "--rules",
        "--session-id",
        "--sandbox",
        "--system-prompt-override",
        "--system-prompt",
        "--tools",
        "--worktree-ref",
        "--ref",
    ],
    long_optional: &["--resume", "--worktree"],
    long_variadic: &[],
    headless_subcommands: &[("agent", "grok agent")],
    wrap_subcommands: &["wrap"],
    subcommands: &[
        "clone",
        "completions",
        "cursor-worker",
        "dashboard",
        "doctor",
        "du",
        "disk-usage",
        "export",
        "help",
        "inspect",
        "leader",
        "login",
        "logout",
        "mcp",
        "memory",
        "models",
        "plugin",
        "sessions",
        "setup",
        "trace",
        "update",
        "usage",
        "version",
        "v",
        "worktree",
    ],
};

/// Long flags that print help or the version and exit, in every peer CLI.
const HELP_LONG: &[&str] = &["--help", "--version"];

/// Judge a peer's arguments by role (TSK-136, TSK-141 AC-3). A headless
/// flag or subcommand makes a run, unless the line also asks for help or
/// the version in an option position: each CLI's parser (commander for
/// Claude Code, clap for Codex and Grok) then prints and exits before any
/// session starts. An option's value, anything after `--`, and a prompt are
/// data, never help.
fn peer_run(name: &str, args: &[String], depth: usize) -> Found {
    let cli = match name {
        "claude" => &CLAUDE,
        "codex" => &CODEX,
        _ => &GROK,
    };
    let short_form = cli.headless_long.first().map_or("", |(_, form)| *form);
    let mut headless: Option<&'static str> = None;
    let mut help = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            break; // the rest is positional
        }
        if arg.starts_with("--") {
            let (flag, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
            if HELP_LONG.contains(&flag) && inline.is_none() {
                help = true;
                continue;
            }
            if let Some((_, form)) = cli.headless_long.iter().find(|(f, _)| *f == flag) {
                headless = headless.or(Some(form));
            }
            if inline.is_none() {
                at = after_long_value(cli, flag, args, at);
            }
            continue;
        }
        if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (index, flag) in cluster.char_indices() {
                let rest = &cluster[index + flag.len_utf8()..];
                if cli.help_short.contains(flag) {
                    help = true;
                    continue;
                }
                if cli.headless_short.contains(flag) {
                    headless = headless.or(Some(short_form));
                    if cli.headless_short_value {
                        if rest.is_empty() {
                            at += 1;
                        }
                        break;
                    }
                    continue;
                }
                if cli.short_value.contains(flag) {
                    if rest.is_empty() {
                        at += 1;
                    }
                    break;
                }
                if cli.short_optional.contains(flag) {
                    // An attached value could as well be more flags to
                    // another parser: judge it conservatively.
                    if rest.chars().any(|c| cli.headless_short.contains(c)) {
                        headless = headless.or(Some(short_form));
                    }
                    if rest.is_empty() && args.get(at).is_some_and(|n| !n.starts_with('-')) {
                        at += 1;
                    }
                    break;
                }
            }
            continue;
        }
        // A positional: a subcommand, or the prompt. Options after it are
        // still parsed, so a later `--help` still prints help.
        let word = arg.as_str();
        if headless.is_some() {
            continue;
        }
        if let Some((_, form)) = cli.headless_subcommands.iter().find(|(s, _)| *s == word) {
            headless = Some(form);
            continue;
        }
        if cli.wrap_subcommands.contains(&word) {
            return if help {
                Found::Nothing
            } else {
                classify(&args[at..], depth)
            };
        }
        if cli.subcommands.contains(&word) {
            return Found::Nothing;
        }
    }
    match headless {
        Some(form) if !help => Found::Run(HeadlessRun {
            peer: cli.peer,
            form,
            parsed: true,
        }),
        _ => Found::Nothing,
    }
}

/// Where the arguments resume after long option `flag`, whose value (if it
/// takes one) starts at `at`.
fn after_long_value(cli: &Cli, flag: &str, args: &[String], mut at: usize) -> usize {
    if cli.long_value.contains(&flag) {
        at += 1;
    } else if cli.long_optional.contains(&flag) {
        if args.get(at).is_some_and(|next| !next.starts_with('-')) {
            at += 1;
        }
    } else if cli.long_variadic.contains(&flag) {
        while args.get(at).is_some_and(|next| !next.starts_with('-')) {
            at += 1;
        }
    }
    at
}

/// The raw-text judgement for a line that could not be resolved: a peer
/// name followed, anywhere later, by one of its headless markers.
pub(crate) fn raw_run(command: &str) -> Option<HeadlessRun> {
    let plain: String = command
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"'))
        .collect();
    let words: Vec<&str> = plain
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '+' | '-')))
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().find_map(|(at, word)| {
        let name = basename(word);
        let later = &words[at + 1..];
        let (peer, form) = match name.as_str() {
            "claude" => later.iter().find_map(|w| {
                let short = w.strip_prefix('-').filter(|c| !c.starts_with('-'));
                (*w == "--print" || *w == "ultrareview" || short.is_some_and(|c| c.contains('p')))
                    .then_some(("claude", "claude -p"))
            })?,
            "codex" => later.iter().find_map(|w| {
                matches!(*w, "exec" | "e" | "review").then_some(("codex", "codex exec"))
            })?,
            "grok" => later.iter().find_map(|w| {
                (w.starts_with("-p")
                    || matches!(*w, "--single" | "--prompt-file" | "--prompt-json" | "agent"))
                .then_some(("grok", "grok -p"))
            })?,
            _ => return None,
        };
        Some(HeadlessRun {
            peer,
            form,
            parsed: false,
        })
    })
}

/// The program name: its last path component, without `.exe`, lower case.
fn basename(program: &str) -> String {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".exe")
        .map_or(name.clone(), str::to_string)
}

/// `NAME=value` before a program.
fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::super::guard_forms::{HELP_PAIRS, PACKAGE_RUNNER_PAIRS};
    use super::*;

    fn found(command: &str) -> Option<HeadlessRun> {
        headless_peer_run(command)
    }

    /// Every probe of the TSK-136 review round 1, with its expected verdict,
    /// and the forms each CLI's help documents.
    #[test]
    fn headless_runs_are_found_however_they_are_spelled() {
        for command in [
            "claude -p x",
            "codex exec x",
            "grok -p x",
            "'claude' -p x",
            "cl\"au\"de -p x",
            r"clau\de -p x",
            "$(printf claude) -p x",
            "/usr/local/bin/claude -p x",
            "alias peer='claude -p'\npeer x",
            "shopt -s expand_aliases\nalias peer='claude -p'\npeer x",
            "peer() { claude -p x; }; peer",
            "printf x | xargs claude -p",
            r"find . -maxdepth 0 -exec claude -p x \;",
            "timeout 5 claude -p x",
            "nice -n 1 claude -p x",
            "time claude -p x",
            "command claude -p x",
            "exec claude -p x",
            "bash -lc 'claude -p x'",
            "env bash -lc 'echo harmless; claude -p x'",
            "env bash -c 'echo harmless; claude -p x'",
            "bash <<< 'claude -p x'",
            "for x in one; do claude -p x; done",
            "while false; do claude -p x; done",
            "while true; do claude -p x; break; done",
            "if true; then claude -p x; fi",
            "(claude -p x)",
            "{ claude -p x; }",
            "claude -p <<< x",
            "claude -dp x",
            "grok -px",
            "grok --single x",
            "grok --single=x",
            "grok --prompt-file prompt.txt",
            "grok --prompt-json \"[]\"",
            "codex --model demo exec x",
            "codex --model=demo exec x",
            "codex -c model=\"demo\" exec x",
            // Beyond the probes: further wrappers and documented forms.
            "! claude -p x",
            "sudo -u me claude --print x",
            "nohup nice timeout -s KILL 5 codex e x &",
            "stdbuf -o0 grok agent",
            "grok wrap claude -p x",
            "codex review",
            "claude ultrareview",
            "claude --model opus 'hi' --print",
            "claude --tools Bash Edit -p x",
            "echo 'claude -p x' | bash",
            "CLAUDE=1 env -u X FOO=2 claude -cp x",
            "eval 'codex exec x'",
            "sh -c \"$(echo x); grok --single x\"",
        ] {
            assert!(found(command).is_some(), "{command}");
        }
    }

    #[test]
    fn other_commands_are_not_runs() {
        for command in [
            "claude --help",
            "codex login",
            "grok --version",
            "claude -- -p",
            "grok -- -p",
            "codex -- exec",
            "claude --system-prompt \"-p\"",
            "rg -p pattern",
            "cat -- -p",
            "git commit -m 'claude -p x'",
            "printf '%s' 'claude -p x'",
            // Beyond the probes.
            "codex --version",
            "codex",
            "codex resume --last",
            "codex -p review",
            "codex -i exec.png 'hi'",
            "claude",
            "claude --model opus --effort high",
            "claude -n p",
            "claude --resume abc 'x'",
            "claude mcp list -p",
            "grok -m p",
            "grok --rules '-p' 'fix it'",
            "grok models",
            "grep -rn \"codex exec\" docs",
            "echo 'claude -p'",
            "cargo test -p codeflow-core",
            "find . -name claude -print",
            "nice -n 5 cargo build",
        ] {
            assert_eq!(found(command), None, "{command}");
        }
    }

    #[test]
    fn a_help_or_version_invocation_is_not_a_run_and_its_data_twin_is() {
        for (help, twin) in HELP_PAIRS {
            assert_eq!(found(help), None, "{help}");
            assert!(found(twin).is_some(), "{twin}");
        }
    }

    #[test]
    fn unresolved_lines_say_they_were_not_parsed() {
        let run = found("alias peer='claude -p'\npeer x").unwrap();
        assert!(!run.parsed);
        assert_eq!(run.peer, "claude");
        let run = found("grok --single=x").unwrap();
        assert!(run.parsed);
        assert_eq!(run.form, "grok -p");
        assert_eq!(found("codex review").unwrap().form, "codex review");
        // An unresolved line without a peer marker is not a run.
        assert_eq!(found("$(printf ls) -la"), None);
        assert_eq!(found("bash <<< 'echo claude'"), None);
    }

    /// TSK-223 AC-3 (sathyassn/codeflow#52): text a line only writes as
    /// data, a brief in a heredoc, a commit message or other arguments of a
    /// data command, is no run on a line exec-guard cannot fully resolve.
    /// Each was refused before the change.
    #[test]
    fn text_that_only_names_a_peer_is_not_a_run() {
        for command in [
            "grep -c review <<< 'Codex review: approve'",
            "cat > brief.md <<< 'Codex adversarial seat: please review'",
            "jq -r .x <<< '{\"x\":\"Codex review\"}'",
            "cd docs && grep -c review <<< 'Codex review: approve'",
            "git commit -F - <<< 'docs: record the Codex review'",
            "gh pr comment 56 --body-file - <<< 'the Codex review approved'",
            "grep -q review <<< 'Codex review: approve' && git commit -m 'docs: record the Codex review'",
            "cat > brief.md <<'EOF'\nThe Codex review found two issues.\nEOF\ngrep -c Codex <<< 'Codex review: approve'",
        ] {
            assert_eq!(found(command), None, "{command}");
        }
    }

    /// TSK-223: a line with any command other than bare data commands and
    /// `cd` keeps the 3.0.0 raw judgement of its whole text, so text that
    /// only names a peer beside such a command is still refused.
    #[test]
    fn a_line_with_another_command_keeps_its_raw_text() {
        for command in [
            "$EDITOR notes.md; git commit -m 'docs: record the Codex review'",
            "$EDITOR notes.md; cat > brief.md <<EOF\nCodex adversarial seat: please review\nEOF",
            "$RUN; gh pr create --body 'the Codex review approved'",
            "D=$PWD; grep -c review <<< 'Codex review: approve'",
            "git add . && grep -c review <<< 'Codex review: approve'",
            "/bin/cat <<< 'Codex review: approve'",
            "grep -c review <<< 'Codex review' | wc -l",
            "(grep -c review <<< 'Codex review')",
            "if true; then grep -c review <<< 'Codex review'; fi",
        ] {
            assert!(found(command).is_some(), "{command}");
        }
    }

    /// TSK-223 AC-4: every headless run the 3.0.0 raw text refused on an
    /// unresolved line is still refused, the forms independent review
    /// probed included.
    #[test]
    fn unresolved_lines_that_run_a_peer_are_still_runs() {
        for command in [
            "CMD=codex; $CMD exec x",
            "CMD=\"codex exec\"; $CMD x",
            "printf -v CMD 'codex exec'; $CMD x",
            "read -r CMD <<< 'codex exec'; $CMD x",
            "CMD=codex; find . -maxdepth 0 -exec $CMD exec x \\;",
            "CMD=codex; npx --package @openai/codex $CMD exec x",
            "$SHELL -c 'codex exec x'",
            "$SUDO codex exec x",
            "$(printf claude) -p x",
            "$(echo codex exec) x",
            "alias peer='claude -p'",
            "shopt -s expand_aliases\nalias a=codex\nalias b=a\nb exec x",
            "shopt -s expand_aliases\nalias run='command '\nalias peer=codex\nrun peer exec x",
            "bash <<< \"codex exec x\"",
            "bash <<< 'claude \"hello; world\" --print'",
            "bash <<< 'grok \"hello|world\" -p'",
            "python3 <<< 'import os; os.system(\"codex exec x\")'",
            "echo 'codex exec x' | bash",
            "echo 'codex -c developer_instructions=\"hello world\" exec x' | bash",
            "echo 'codex exec x' |& $SHELL",
            "echo 'codex exec x' | { $SHELL; }",
            "echo 'codex exec x' | ( $SHELL )",
            "echo 'codex exec x' | env $SHELL",
            "echo 'codex exec x' | timeout 5 $SHELL",
            "cat > run.sh <<EOF\ncodex exec x\nEOF\nbash run.sh",
            "cat > run.sh <<'EOF'\nclaude --print hi\nEOF\n$SHELL run.sh",
            "echo 'codex exec x' > run.sh; bash run.sh",
            "cat <<< 'grok agent' > run.sh; source run.sh",
            "function peer { claude -p x; }; peer",
            // Round two of review: data a later command can run.
            "cat > run.sh <<'EOF'\nCMD=codex\n$CMD exec x\nEOF\nbash run.sh",
            "cat > run.sh <<-'EOF'\n\tCMD=codex\n\t$CMD exec x\n\tEOF\nsource run.sh",
            "cat > run.sh <<\\EOF\nshopt -s expand_aliases\nalias a=codex\nalias b=a\nb exec x\nEOF\nbash run.sh",
            "printf '%s %s\\n' codex exec > run.sh; bash run.sh",
            "echo codex > peer; CMD=$(cat peer); $CMD exec x",
            "read CMD < <(echo codex); $CMD exec x",
            "git commit -m 'fix: note' -m 'codex exec x'\ngit log -1 --format=%b | bash",
            "echo \"`CMD=codex; $CMD exec x`\"",
            "echo \"`printf '%s %s' codex exec | $SHELL`\"",
            "source /dev/null\nGIT_EDITOR='codex exec' git commit --allow-empty",
            "source /dev/null\ngit -ccore.editor='codex exec' commit --allow-empty",
            "source /dev/null\nGH_EDITOR='codex exec' gh gist edit 1",
            "nice printf '%s %s\\n' codex exec > run.sh; bash run.sh",
            "$X; cat > run.sh <<'EOF'\ncodex exec x\nEOF\n$EDITOR run.sh",
            "for i in 1 2; do bash run.sh; $X; echo 'codex exec x' > run.sh; done",
            "$SHELL run.sh & echo 'codex exec x' > run.sh",
            "trap 'bash run.sh' EXIT; $X; echo 'codex exec x' > run.sh",
            "f() { bash run.sh; }; $X; echo 'codex exec x' > run.sh; f",
            "PRELOAD=./x.so cat run.sh; $X; echo 'codex exec x' > run.sh",
            "$X; echo 'codex exec x' > run.sh; sort --compress-program=./run.sh big.txt",
            "$X; rg --pre 'codex exec' x",
            "alias cat='bash -s'; $X; cat <<'EOF'\ncodex exec x\nEOF",
            "source lib.sh; $X; git commit -m 'codex exec x'",
            "PATH=./bin:$PATH; $X; cat <<'EOF'\ncodex exec x\nEOF",
            "export BASH_ENV=run.sh; $X; echo 'codex exec x' > run.sh",
            "exec > >(bash); $X; echo 'codex exec x'",
            "$X; R=$(bash <<'true'\nx)\necho 'codex exec x'\ntrue\n)",
            // Round three of review: a reordering word behind a launcher,
            // a group or a reserved word.
            "command source fn.sh; echo codex exec do the work",
            "builtin source fn.sh; echo codex exec do the work",
            "command . fn.sh; echo codex exec do the work",
            "{ command source fn.sh; }; echo codex exec do the work",
            "if true; then command source fn.sh; fi; echo codex exec do the work",
            "if false; then true; else command source fn.sh; fi; echo codex exec do the work",
            "if false; then true; elif true; then command source fn.sh; fi; echo codex exec do the work",
            "$(true); command eval 'echo() { :; }'; echo codex exec do the work",
            "$(true); time command eval 'echo() { :; }'; echo codex exec do the work",
            "$EDITOR notes.md; command eval 'echo() { :; }'; echo codex exec do the work",
            "shopt -s expand_aliases\ncommand alias echo=eval\n$X\necho codex exec do the work",
            "nohup command source fn.sh; $X; echo codex exec do the work",
            "env source fn.sh; $X; echo codex exec do the work",
            "while source fn.sh; do $X; done; echo codex exec do the work",
            "! command source fn.sh; $X; echo codex exec do the work",
            "time -p builtin . fn.sh; $X; echo codex exec do the work",
            "command exec > >(bash); $X; echo codex exec do the work",
            "$EDITOR notes.md; noglob source fn.sh; echo codex exec do the work",
            "$EDITOR notes.md; nocorrect . fn.sh; echo codex exec do the work",
            "$EDITOR notes.md; - builtin source fn.sh; echo codex exec do the work",
            // Final review: an alias variable or a replaced program.
            "shopt -s expand_aliases\nprintf -v 'BASH_ALIASES[echo]' command\nX=true; $X\necho codex exec x",
            "shopt -s expand_aliases\nBASH_ALIASES[echo]=command\nX=true; $X\necho codex exec x",
            "shopt -s expand_aliases; printf -v 'BASH_ALIASES[echo]' command; X=true; $X; echo codex exec x",
            "declare -A BASH_ALIASES=([echo]=command); $X; echo codex exec x",
            "X=true; $X\nmkdir -p /tmp/review-bin\ncp /usr/bin/python3 /tmp/review-bin/cat\n/tmp/review-bin/cat <<'EOF'\nimport os\nos.system('codex exec x')\nEOF",
            "X=true; $X\nprintf -v PATH '/tmp/review-bin:%s' \"$PATH\"\ncat <<'EOF'\nimport os\nos.system('codex exec x')\nEOF",
            "PATH=/tmp/review-bin:$PATH; $X; cat <<'EOF'\nimport os\nos.system('codex exec x')\nEOF",
            "./cat <<< 'codex exec x'",
            "read -r X <<< 'codex exec x'; cat <<< 'x'",
            // A written hook or configuration that git or gh then runs.
            "cat > .git/hooks/pre-commit <<'EOF'\n#!/usr/bin/env python3\nimport os\nos.system('codex exec x')\nEOF\ngit commit -m x <<< ''",
            "cat >> .git/config <<'EOF'\n[core]\n\tfsmonitor = sh -c 'codex exec x'\nEOF\ngit commit -m x <<< ''",
            "tee .git/hooks/pre-push <<< 'import os; os.system(\"codex exec x\")' && gh pr create --body x",
            "export GIT_EDITOR='codex exec'; $X; git commit -e -m x",
        ] {
            assert!(found(command).is_some(), "{command}");
        }
    }

    /// TSK-223: data is judged as a script, with shell quoting: a peer in
    /// command position counts with its headless flag anywhere, or its
    /// headless subcommand as the first word after its options.
    #[test]
    fn data_is_judged_as_a_script_with_shell_quoting() {
        for (text, form) in [
            ("codex exec x", Some("codex exec")),
            ("codex --model demo exec x", Some("codex exec")),
            ("codex -c model=demo exec x", Some("codex exec")),
            ("codex -c 'k=\"a b\"' exec x", Some("codex exec")),
            ("codex --full-auto exec x", Some("codex exec")),
            ("claude \"hello; world\" --print", Some("claude -p")),
            ("claude -dp hi", Some("claude -p")),
            ("grok --single=x", Some("grok -p")),
            ("grok agent", Some("grok agent")),
            ("timeout 5 codex exec x", Some("codex exec")),
            ("Codex review approve", Some("codex review")),
            ("Codex adversarial seat: please review", None),
            ("The Codex review found two issues.", None),
            ("docs: record the Codex review", None),
            ("Claude, the review seat, approves", None),
            ("codex -- exec", None),
        ] {
            assert_eq!(data_run(text, 0).map(|run| run.form), form, "{text}");
        }
    }

    /// TSK-141 AC-6: a peer started through `npx`, `bunx`, `pnpm dlx` or
    /// `yarn dlx` is judged as its direct invocation, headless run and help
    /// alike.
    #[test]
    fn a_package_runner_is_judged_as_its_direct_invocation() {
        for (direct, runner) in PACKAGE_RUNNER_PAIRS {
            assert_eq!(
                headless_peer_run(runner),
                headless_peer_run(direct),
                "{runner} as {direct}"
            );
        }
        assert_eq!(package_bin("@anthropic-ai/claude-code@2.1.283"), "claude");
        assert_eq!(package_bin("@openai/codex"), "codex");
        assert_eq!(package_bin("grok@latest"), "grok");
        // A runner of anything else is no peer run.
        assert_eq!(headless_peer_run("npx prettier -p x"), None);
        assert_eq!(headless_peer_run("pnpm install -p"), None);
    }
}
