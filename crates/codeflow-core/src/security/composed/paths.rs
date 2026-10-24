//! Command-position shell values for filesystem consumers. The text lexer and
//! value evaluator are shared with the composed-text check; no shell is run.
use super::{
    lex, new_run, Ctx, Dialect, Heredoc, Part, Run, State, Tok, Value, Var, Word, GRAMMAR,
};
use crate::security::outward::lexical_join;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub(crate) text: String,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) unresolved: Vec<String>,
    /// Literal builtin output, captured without executing a program.
    stdout: Option<String>,
    /// Literal data a preceding pipeline producer supplies to this command.
    pub(crate) input_code: Option<String>,
    pub(crate) links: Vec<Link>,
}

#[derive(Clone, PartialEq, Eq)]
struct Position {
    state: State,
    stack: Vec<PathBuf>,
}

/// Resolve both supported shell readings. Pipeline tails may run in the caller
/// (zsh and Bash lastpipe), so the walk retains both the child and caller state.
pub(crate) fn commands(line: &str, cwd: &Path, home: Option<&Path>) -> Vec<Command> {
    read_commands(line, cwd, home, false)
}

pub(crate) fn writer_commands(line: &str, cwd: &Path, home: Option<&Path>) -> Vec<Command> {
    read_commands(line, cwd, home, true)
}

fn read_commands(line: &str, cwd: &Path, home: Option<&Path>, environment: bool) -> Vec<Command> {
    let Ok((tokens, docs)) = lex(line) else {
        return vec![Command {
            text: line.into(),
            cwd: None,
            unresolved: vec![line.into()],
            stdout: None,
            input_code: None,
            links: Vec::new(),
        }];
    };
    let mut result = Vec::new();
    for dialect in [Dialect::Bash, Dialect::Zsh] {
        let mut state = State {
            directory: Some(cwd.to_path_buf()),
            ..State::default()
        };
        if environment {
            for (name, value) in std::env::vars_os() {
                if let (Ok(name), Ok(value)) = (name.into_string(), value.into_string()) {
                    if !shell_managed(&name)
                        && !name.starts_with("__directory_stack_")
                        && line.contains(&name)
                    {
                        state.vars.insert(name, Var::Scalar(Value::text(&value)));
                    }
                }
            }
        }
        state.vars.insert("PWD".into(), path_value(cwd));
        if let Some(home) = home {
            state.vars.insert("HOME".into(), path_value(home));
        }
        if std::env::var_os("CDPATH").is_some_and(|value| !value.is_empty()) {
            state
                .vars
                .insert("CDPATH".into(), Var::Unknown("inherited CDPATH"));
        }
        walk(
            &tokens,
            &docs,
            vec![Position {
                state,
                stack: Vec::new(),
            }],
            dialect,
            &mut result,
            0,
            &mut Reparses::default(),
        );
    }
    // A command can be reached by several branches and by both dialects.
    // Judge each directory/text pair once, retaining every unresolved reason.
    let mut indices = HashMap::new();
    let mut unique: Vec<(usize, Command)> = Vec::new();
    for (position, command) in result.into_iter().enumerate() {
        let key = (command.cwd.clone(), command.text.clone());
        if let Some(&index) = indices.get(&key) {
            let (last_position, prior): &mut (usize, Command) = &mut unique[index];
            // Prospective checkout declarations take effect at their first
            // occurrence. Other repeated commands are judged at their last
            // occurrence, after every checkout declaration that preceded one.
            let words = pipeline_tail_words(&prior.text);
            let declares_checkout =
                crate::hooks::git_guard::strip_launchers(&words).is_some_and(|(program, args)| {
                    (program.rsplit('/').next() == Some("git")
                        && args.iter().any(|arg| arg == "worktree"))
                        || matches!(
                            crate::security::unresolved::writer_name(program),
                            "ln" | "link" | "cp" | "pax" | "cpio" | "rsync"
                        )
                });
            if !declares_checkout {
                *last_position = position;
            }
            for word in command.unresolved {
                if !prior.unresolved.contains(&word) {
                    prior.unresolved.push(word);
                }
            }
        } else {
            indices.insert(key, unique.len());
            unique.push((position, command));
        }
    }
    unique.sort_by_key(|(position, _)| *position);
    let mut links = Vec::new();
    unique
        .into_iter()
        .map(|(_, mut command)| {
            command.links.clone_from(&links);
            if let Some(dir) = &command.cwd {
                links.extend(created_links(
                    &command.text,
                    dir,
                    command.input_code.as_deref(),
                ));
            }
            command
        })
        .collect()
}

/// A possible link made earlier on the line. Sources and destinations use
/// command-position cwd; symbolic targets are relative to the link's parent.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Link {
    from: PathBuf,
    to: PathBuf,
}

fn pipeline_tail_words(text: &str) -> Vec<String> {
    if !text.contains('|') {
        return crate::hooks::git_guard::command_argv(text);
    }
    // Pipeline records retain their producer for code readers. Link makers
    // consume the tail's operands and the captured literal producer output.
    let tail = lex(text).ok().map(|(tokens, _)| {
        let start = tokens
            .iter()
            .rposition(|t| matches!(t, Tok::Op("|" | "|&")))
            .map_or(0, |at| at + 1);
        tokens[start..]
            .iter()
            .filter_map(|t| match t {
                Tok::Word(w) => Some(w.raw.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    });
    crate::hooks::git_guard::command_argv(tail.as_deref().unwrap_or(text))
}

fn created_links(text: &str, cwd: &Path, input: Option<&str>) -> Vec<Link> {
    let words = pipeline_tail_words(text);
    let Some((program, args)) = crate::hooks::git_guard::strip_launchers(&words) else {
        return Vec::new();
    };
    let name = crate::security::unresolved::writer_name(program);
    if matches!(name, "pax" | "cpio") {
        return archive_links(name, args, cwd, input);
    }
    if name == "rsync" {
        return rsync_links(args, cwd);
    }
    let symbolic = args.iter().any(|a| {
        a == "--symbolic-link" || (a.starts_with('-') && !a.starts_with("--") && a.contains('s'))
    });
    let hard_link = args
        .iter()
        .any(|a| a == "--link" || (a.starts_with('-') && !a.starts_with("--") && a.contains('l')));
    if !(matches!(name, "ln" | "link") || name == "cp" && (symbolic || hard_link)) {
        return Vec::new();
    }
    let Some(effects) = crate::security::unresolved::writer_effects(
        if name == "link" { "ln" } else { name },
        args,
        &[],
    ) else {
        return Vec::new();
    };
    let Some(copy) = effects.copy else {
        return Vec::new();
    };
    let Some(dest) = copy.dest else {
        return Vec::new();
    };
    let dest = lexical_join(cwd, Path::new(&dest));
    let mut links = Vec::new();
    for source in copy.sources {
        // The destination can be an existing directory, or one made earlier
        // on the line. Retain both spellings without consulting the hook's cwd.
        for from in [
            Some(dest.clone()),
            Path::new(&source).file_name().map(|base| dest.join(base)),
        ]
        .into_iter()
        .flatten()
        {
            let to = lexical_join(
                if symbolic {
                    from.parent().unwrap_or(cwd)
                } else {
                    cwd
                },
                Path::new(&source),
            );
            links.push(Link { from, to });
        }
    }
    links
}

/// Pass-through archive tools preserve the source's relative path below the
/// destination. Unknown file lists or pathname rewrites retain an unresolved
/// target, which is refused only if a later operation follows that mapping.
fn archive_links(name: &str, args: &[String], cwd: &Path, input: Option<&str>) -> Vec<Link> {
    let mut flags = String::new();
    let mut operands = Vec::new();
    let mut unknown = false;
    let mut literal = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if literal || !arg.starts_with('-') || arg == "-" {
            operands.push(arg.clone());
        } else if arg == "--" {
            literal = true;
        } else if arg == "--link" {
            flags.push('l');
        } else if arg == "--pass-through" {
            flags.push('p');
        } else if arg == "--null" {
            flags.push('0');
        } else if arg.starts_with("--") {
            if matches!(
                arg.as_str(),
                "--owner" | "--format" | "--file" | "--message"
            ) {
                at += 1;
            }
            // These do not change pathname placement or consume operands.
            if !matches!(
                arg.as_str(),
                "--make-directories"
                    | "--preserve-modification-time"
                    | "--unconditional"
                    | "--verbose"
                    | "--quiet"
            ) {
                unknown = true;
            }
        } else {
            for (offset, flag) in arg[1..].char_indices() {
                flags.push(flag);
                unknown |= (name == "pax" && flag == 'i') || (name == "cpio" && flag == 'r');
                let takes_value = if name == "pax" {
                    "bfopsxBEGTU"
                } else {
                    "CEFfHIORM"
                };
                if takes_value.contains(flag) {
                    // A pathname substitution can change every output name.
                    unknown |= name == "pax" && flag == 's';
                    if offset + 2 == arg.len() {
                        at += 1;
                    }
                    break;
                }
            }
        }
    }
    let pass = if name == "pax" {
        flags.contains('r') && flags.contains('w')
    } else {
        flags.contains('p')
    };
    if !pass || !flags.contains('l') {
        return Vec::new();
    }
    let Some(dest) = operands.pop() else {
        return Vec::new();
    };
    let dest = lexical_join(cwd, Path::new(&dest));
    if name == "cpio" || operands.is_empty() {
        if let Some(input) = input {
            operands = input
                .split(if flags.contains('0') { '\0' } else { '\n' })
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        } else {
            unknown = true;
        }
    }
    if unknown {
        return vec![Link {
            from: dest,
            to: cwd.join("$__unread_link_source"),
        }];
    }
    operands
        .into_iter()
        .map(|source| Link {
            from: lexical_join(&dest, Path::new(source.trim_start_matches('/'))),
            to: lexical_join(cwd, Path::new(&source)),
        })
        .collect()
}

fn rsync_links(args: &[String], cwd: &Path) -> Vec<Link> {
    let Some(effects) = crate::security::unresolved::writer_effects("rsync", args, &[]) else {
        return Vec::new();
    };
    let Some(dest) = effects.copy.and_then(|copy| copy.dest) else {
        return Vec::new();
    };
    let dest = lexical_join(cwd, Path::new(&dest));
    effects
        .read
        .values
        .into_iter()
        .filter(|(option, _)| option == "--link-dest")
        .map(|(_, source)| Link {
            from: dest.clone(),
            to: lexical_join(&dest, Path::new(&source)),
        })
        .collect()
}

/// All possible mapped spellings, retaining the original spelling at callers.
/// A fixed point bound prevents cyclic or repeated link declarations growing.
pub(crate) fn linked_paths(word: &str, cwd: &Path, links: &[Link]) -> Result<Vec<PathBuf>, String> {
    if links.is_empty() {
        return Ok(Vec::new());
    }
    let original = cwd.join(word);
    let original = crate::hooks::git_guard::prospective_path(&original)?;
    let mut paths = vec![original.clone()];
    for _ in 0..16 {
        let before = paths.len();
        for path in paths.clone() {
            for link in links {
                let from = crate::hooks::git_guard::prospective_path(&link.from)?;
                if let Ok(tail) = path.strip_prefix(&from) {
                    if link
                        .to
                        .to_str()
                        .is_none_or(crate::hooks::git_guard::unresolved_word)
                    {
                        return Err(
                            "cannot verify a link target created earlier on this line".into()
                        );
                    }
                    let target = lexical_join(&link.to, tail);
                    let target = crate::hooks::git_guard::prospective_path(&target)?;
                    if !paths.contains(&target) {
                        paths.push(target);
                    }
                }
            }
        }
        if paths.len() == before {
            return Ok(paths.into_iter().filter(|p| p != &original).collect());
        }
        if paths.len() > 64 {
            break;
        }
    }
    Err("cannot verify paths through too many same-line links".into())
}

/// Maximum distinct concrete shell-code readings per input line.
pub(crate) const MAX_SCRIPT_READINGS: usize = 16;

#[derive(Default)]
struct Reparses {
    entries: Vec<(Position, String, Position, Vec<Command>)>,
    count: usize,
}

/// These values are maintained by the executing shell, not its hook child.
pub(crate) fn shell_managed(name: &str) -> bool {
    matches!(
        name,
        "PWD"
            | "OLDPWD"
            | "DIRSTACK"
            | "dirstack"
            | "_"
            | "PPID"
            | "BASHPID"
            | "BASH_SOURCE"
            | "BASH_LINENO"
            | "BASH_COMMAND"
            | "BASH_SUBSHELL"
            | "BASH_ARGC"
            | "BASH_ARGV"
            | "FUNCNAME"
            | "LINENO"
            | "SHLVL"
            | "RANDOM"
            | "SRANDOM"
            | "SECONDS"
            | "EPOCHSECONDS"
            | "EPOCHREALTIME"
    )
}

/// Both shell readings accept --; zsh also accepts a lone -. Any other
/// leading option is unread instead of being mistaken for a command word.
pub(crate) fn eval_body(args: &[String]) -> String {
    let args = &args[args
        .iter()
        .take_while(|word| matches!(word.as_str(), "--" | "-"))
        .count()..];
    if args.first().is_some_and(|word| word.starts_with('-')) {
        "__unresolved_shell_code".into()
    } else {
        args.join(" ")
    }
}

/// Literal stdin code for the generic segment reader, which also owns Git
/// branch checks. The position-aware reader resolves variables separately.
pub(crate) fn literal_input_script(line: &str) -> Option<String> {
    let words = crate::hooks::git_guard::command_argv(line);
    let (name, args) = crate::hooks::git_guard::script_reader(&words)?;
    if crate::hooks::git_guard::shell_inline_code(name, args).is_some() {
        return None;
    }
    if let Some(script) = crate::security::outward::here_string(line) {
        return Some(crate::hooks::git_guard::launched_script(&words, &script));
    }
    let (tokens, _) = lex(line).ok()?;
    let mut events = Vec::new();
    let run = new_run(Dialect::Bash, State::default(), &mut events, 0, 0);
    tokens
        .iter()
        .find_map(|token| match token {
            Tok::Word(word)
            | Tok::Redir {
                target: Some(word), ..
            } if !matches!(token, Tok::Word(_)) || !stdin_option(args) => {
                word.parts.iter().find_map(|part| match part {
                    Part::ProcSubst(body) => {
                        run.approx.set(false);
                        let value = run.capture(body).0.ok().and_then(|v| v.exact());
                        (!run.approx.get()).then_some(value).flatten()
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .map(|script| crate::hooks::git_guard::launched_script(&words, &script))
}

pub(crate) fn literal_output(line: &str) -> Option<String> {
    let mut events = Vec::new();
    let run = new_run(Dialect::Bash, State::default(), &mut events, 0, 0);
    let value = run.capture(line).0.ok().and_then(|value| value.exact());
    (!run.approx.get()).then_some(value).flatten()
}

fn stdin_option(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('s'))
}

fn unread_code(reason: &str) -> Command {
    Command {
        text: "__unresolved_shell_code".into(),
        cwd: None,
        unresolved: vec![reason.into()],
        stdout: None,
        input_code: None,
        links: Vec::new(),
    }
}

fn plain(tok: &Tok) -> Option<&str> {
    match tok {
        Tok::Word(word) => word.plain(),
        Tok::Op(op) => Some(op),
        _ => None,
    }
}

const TOO_MANY_STATES: &str = "too many reachable shell states";

fn bound(states: &mut Vec<Position>) {
    // Once the bounded reading is fully unknown, concrete alternatives add
    // no information. Do not grow them again on the rest of an OR chain.
    if let Some(index) = states
        .iter()
        .position(|state| state.state.world == Some(TOO_MANY_STATES))
    {
        let unknown = states.swap_remove(index);
        *states = vec![unknown];
        return;
    }
    let mut unique = Vec::new();
    for state in states.drain(..) {
        if !unique.contains(&state) {
            unique.push(state);
        }
    }
    *states = unique;
    if states.len() > 64 {
        let mut state = State {
            world: Some(TOO_MANY_STATES),
            ..State::default()
        };
        state
            .vars
            .insert("PWD".into(), Var::Unknown("unknown working directory"));
        *states = vec![Position {
            state,
            stack: Vec::new(),
        }];
    }
}

#[allow(clippy::too_many_lines)] // Scope and separators are handled in one token walk.
fn walk(
    tokens: &[Tok],
    docs: &[Heredoc],
    mut states: Vec<Position>,
    dialect: Dialect,
    output: &mut Vec<Command>,
    depth: usize,
    reparses: &mut Reparses,
) -> Vec<Position> {
    if depth > 16 {
        for state in &mut states {
            state.state.directory = None;
            state.state.world = Some(GRAMMAR);
        }
        return states;
    }
    let mut at = 0;
    let mut list_start = states.clone();
    let mut pipeline: Option<Vec<Position>> = None;
    let mut alternatives = Vec::new();
    let mut failures = Vec::new();
    let mut producers: Vec<Command> = Vec::new();
    let mut compounds: Vec<Vec<String>> = Vec::new();
    while at < tokens.len() {
        let keyword = plain(&tokens[at]);
        if matches!(keyword, Some("if" | "for" | "while" | "until" | "case")) {
            compounds.push(
                super::assigned_names(&tokens[at..])
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect(),
            );
        } else if matches!(keyword, Some("else" | "elif" | "fi" | "done" | "esac")) {
            if let Some(names) = compounds.last() {
                for position in &mut states {
                    for name in names {
                        position.state.vars.insert(
                            name.clone(),
                            Var::Unknown("a compound command may assign this value"),
                        );
                    }
                }
            }
            if matches!(keyword, Some("fi" | "done" | "esac")) {
                compounds.pop();
            }
        }
        if (matches!(tokens.get(at), Some(Tok::Word(_)))
            && matches!(tokens.get(at + 1), Some(Tok::Op("(")))
            && matches!(tokens.get(at + 2), Some(Tok::Op(")"))))
            || matches!(
                plain(&tokens[at]),
                Some("if" | "for" | "while" | "until" | "case" | "function")
            )
        {
            let moves = tokens[at..]
                .iter()
                .any(|token| matches!(plain(token), Some("cd" | "chdir" | "pushd" | "popd")));
            for state in &mut states {
                state.state.directory = None;
                if moves {
                    for name in ["PWD", "OLDPWD"] {
                        state.state.vars.insert(
                            name.into(),
                            Var::Unknown("directory values may change in control flow"),
                        );
                    }
                    state.stack.clear();
                }
            }
        }
        let function = if plain(&tokens[at]) == Some("function") {
            tokens.get(at + 1).and_then(plain)
        } else if matches!(tokens.get(at + 1), Some(Tok::Op("(")))
            && matches!(tokens.get(at + 2), Some(Tok::Op(")")))
        {
            plain(&tokens[at])
        } else {
            None
        };
        if let Some(name) = function {
            for position in &mut states {
                let mut events = Vec::new();
                let mut run = new_run(dialect, position.state.clone(), &mut events, 0, 0);
                run.define_function(name, &tokens[at..], true);
                position.state = run.state;
            }
            if let Some(open) = tokens[at..].iter().position(|t| plain(t) == Some("{")) {
                let start = at + open + 1;
                let mut end = start;
                let mut nesting = 1;
                while end < tokens.len() {
                    match plain(&tokens[end]) {
                        Some("{") => nesting += 1,
                        Some("}") => nesting -= 1,
                        _ => {}
                    }
                    if nesting == 0 {
                        break;
                    }
                    end += 1;
                }
                let mut body_states = states.clone();
                for position in &mut body_states {
                    position.state.world = Some("function body executes with later shell values");
                }
                walk(
                    &tokens[start..end],
                    docs,
                    body_states,
                    dialect,
                    output,
                    depth + 1,
                    reparses,
                );
                at = end + usize::from(end < tokens.len());
                continue;
            }
        }
        if plain(&tokens[at]) == Some("for") {
            if let (Some(name), Some("in")) = (
                tokens.get(at + 1).and_then(plain),
                tokens.get(at + 2).and_then(plain),
            ) {
                let end = tokens[at..]
                    .iter()
                    .position(|t| matches!(t, Tok::Op(_)))
                    .map_or(tokens.len(), |n| at + n);
                let mut choices = Vec::new();
                for state in &states {
                    for token in &tokens[at + 3..end] {
                        if let Tok::Word(word) = token {
                            let mut events = Vec::new();
                            let mut run = new_run(dialect, state.state.clone(), &mut events, 0, 0);
                            if let Ok(values) = run.word(word, Ctx::Fields).fields {
                                for value in values {
                                    let mut choice = state.clone();
                                    choice
                                        .state
                                        .vars
                                        .insert(name.to_string(), Var::Scalar(value));
                                    choices.push(choice);
                                }
                            }
                        }
                    }
                }
                if !choices.is_empty() {
                    states = choices;
                    bound(&mut states);
                }
                at = end;
                continue;
            }
        }
        let before = states.clone();
        let first_output = output.len();
        if matches!(plain(&tokens[at]), Some("(" | "{")) {
            let open = plain(&tokens[at]).unwrap_or_default();
            let close = if open == "(" { ")" } else { "}" };
            let mut nesting = 1;
            let start = at + 1;
            at = start;
            while at < tokens.len() {
                if plain(&tokens[at]) == Some(open) {
                    nesting += 1;
                }
                if plain(&tokens[at]) == Some(close) {
                    nesting -= 1;
                }
                if nesting == 0 {
                    break;
                }
                at += 1;
            }
            let nested = walk(
                &tokens[start..at],
                docs,
                states.clone(),
                dialect,
                output,
                depth + 1,
                reparses,
            );
            if open == "{" {
                states = nested;
            }
            at += usize::from(at < tokens.len());
        } else if matches!(tokens[at], Tok::Op(_)) {
            at += 1;
            continue;
        } else {
            let start = at;
            while at < tokens.len()
                && !matches!(tokens[at], Tok::Op(_))
                && plain(&tokens[at]) != Some("}")
            {
                at += 1;
            }
            if start == at {
                at += 1;
                continue;
            }
            states = states
                .into_iter()
                .map(|state| {
                    simple(
                        &tokens[start..at],
                        docs,
                        state,
                        dialect,
                        output,
                        depth,
                        reparses,
                        !producers.is_empty(),
                    )
                })
                .collect();
        }
        if !producers.is_empty() {
            let consumers = output.split_off(first_output);
            for consumer in consumers {
                let before = output.len();
                for producer in producers.iter().filter(|producer| {
                    producer.cwd == consumer.cwd || producer.cwd.is_none() || consumer.cwd.is_none()
                }) {
                    let mut command = consumer.clone();
                    command.text = format!("{} | {}", producer.text, command.text);
                    command.input_code.clone_from(&producer.stdout);
                    if crate::security::outward::reads_stdin_code(&consumer.text) {
                        command
                            .unresolved
                            .extend(producer.unresolved.iter().cloned());
                    }
                    let words = crate::hooks::git_guard::command_argv(&consumer.text);
                    if crate::hooks::git_guard::reads_shell_stdin(&words) {
                        if let Some(script) = &producer.stdout {
                            for state in states.clone() {
                                let script =
                                    crate::hooks::git_guard::launched_script(&words, script);
                                let after =
                                    reparse(&script, state, dialect, output, depth, reparses);
                                if !states.contains(&after) {
                                    // A pipeline tail may run in this shell or
                                    // a child. Join differing results once;
                                    // later tails cannot multiply those forks.
                                    let mut joined = after;
                                    joined.state.directory = None;
                                    joined.state.world = Some(TOO_MANY_STATES);
                                    joined.stack.clear();
                                    set_pwd(&mut joined.state);
                                    states = vec![joined];
                                    break;
                                }
                            }
                            bound(&mut states);
                        }
                    }
                    output.push(command);
                }
                if output.len() == before {
                    let mut command = consumer;
                    command.unresolved.push("pipeline directory state".into());
                    output.push(command);
                }
            }
            producers.clear();
        }
        let term = tokens.get(at).and_then(plain).unwrap_or(";");
        match term {
            "|" | "|&" => {
                producers = output[first_output..].to_vec();
                if producers.len() > 64 {
                    producers = vec![Command {
                        text: String::new(),
                        cwd: None,
                        unresolved: vec!["pipeline input with too many reachable states".into()],
                        stdout: None,
                        input_code: None,
                        links: Vec::new(),
                    }];
                }
                pipeline.get_or_insert_with(|| before.clone());
                states = before;
            }
            "&" => {
                states.clone_from(&list_start);
                pipeline = None;
            }
            "||" => {
                // Only failure enters the next alternative. Preserve changed
                // variables as well as directories on the successful branch;
                // unchanged alternatives collapse instead of doubling state.
                if states != before {
                    alternatives.extend(states);
                    states = before;
                    states.append(&mut failures);
                    bound(&mut alternatives);
                }
            }
            "&&" => {
                failures.extend(before.clone());
                bound(&mut failures);
                states.append(&mut alternatives);
            }
            ";" | "\n" => {
                // A directory change may fail. On && only success reaches the
                // next command; at other joins retain its original state too.
                if states
                    .iter()
                    .zip(&before)
                    .any(|(a, b)| a.state.world.is_none() && a.state.directory != b.state.directory)
                {
                    states.extend(before);
                }
            }
            _ => {}
        }
        if !matches!(term, "|" | "|&") {
            if let Some(parent) = pipeline.take() {
                states.extend(parent);
            }
        }
        if matches!(term, ";" | "\n" | "&") {
            states.append(&mut alternatives);
            states.append(&mut failures);
        }
        bound(&mut states);
        if matches!(term, ";" | "\n" | "&") {
            list_start.clone_from(&states);
        }
        if matches!(tokens.get(at), Some(Tok::Op(_))) {
            at += 1;
        }
    }
    states
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn unresolved_word(word: &Word) -> String {
    if word.raw.is_empty() {
        "process substitution".into()
    } else {
        word.raw.clone()
    }
}

fn resolve(run: &mut Run<'_>, word: &Word, ctx: Ctx, unresolved: &mut Vec<String>) -> String {
    if run.state.world.is_some()
        && matches!(word.parts.first(), Some(Part::Lit { text, quoted: false }) if text.starts_with('~'))
    {
        unresolved.push(unresolved_word(word));
        return word.raw.clone();
    }
    if let Some(raw) = word.plain().filter(|raw| raw.starts_with('~')) {
        let (head, tail) = raw.split_once('/').unwrap_or((raw, ""));
        let name = match head {
            "~" => "HOME".to_string(),
            "~+" => "PWD".to_string(),
            "~-" => "OLDPWD".to_string(),
            _ => format!("__directory_stack_{}", &head[1..]),
        };
        if let Some(Var::Scalar(value)) = run.state.vars.get(&name) {
            if let Some(value) = value.exact() {
                return if tail.is_empty() {
                    quote(&value)
                } else {
                    format!("{}/{tail}", quote(&value))
                };
            }
        }
        unresolved.push(unresolved_word(word));
        return raw.into();
    }
    if matches!(word.parts.first(), Some(Part::Lit { text, quoted: false }) if text.starts_with('~'))
    {
        // Mixed quoted/expanded tails still have an active leading tilde.
        // Until that combination is modeled, it must not become a literal path.
        unresolved.push(unresolved_word(word));
        return word.raw.clone();
    }
    // Keep literal syntax and glob provenance; only expansions need rendering.
    if word
        .parts
        .iter()
        .all(|part| matches!(part, Part::Lit { .. }))
    {
        return word.raw.clone();
    }
    if run.state.world.is_some() {
        unresolved.push(unresolved_word(word));
        return word.raw.clone();
    }
    run.approx.set(false);
    let eval = run.word(word, ctx);
    if run.approx.get() {
        unresolved.push(unresolved_word(word));
        return word.raw.clone();
    }
    if let Ok(fields) = eval.fields {
        let values = fields
            .iter()
            .map(|value| {
                if let Some(exact) = value.exact() {
                    return Some(quote(&exact));
                }
                if value
                    .0
                    .iter()
                    .all(|p| matches!(p, super::Piece::Text(_) | super::Piece::Glob(_)))
                {
                    return Some(
                        value
                            .0
                            .iter()
                            .map(|p| match p {
                                super::Piece::Text(t) => quote(t),
                                super::Piece::Glob(t) => t.clone(),
                                _ => unreachable!(),
                            })
                            .collect::<String>(),
                    );
                }
                None
            })
            .collect::<Option<Vec<_>>>();
        if let Some(values) = values {
            return values.join(" ");
        }
    }
    unresolved.push(unresolved_word(word));
    word.raw.clone()
}

#[allow(clippy::too_many_lines)] // One command's expansions precede its state effects.
#[allow(clippy::too_many_arguments)] // Pipeline input is supplied after the command's words resolve.
fn simple(
    tokens: &[Tok],
    docs: &[Heredoc],
    mut position: Position,
    dialect: Dialect,
    output: &mut Vec<Command>,
    depth: usize,
    reparses: &mut Reparses,
    piped: bool,
) -> Position {
    position
        .state
        .vars
        .retain(|name, _| !name.starts_with("__directory_stack_"));
    for (index, path) in position
        .state
        .directory
        .iter()
        .chain(position.stack.iter().rev())
        .enumerate()
    {
        position
            .state
            .vars
            .insert(format!("__directory_stack_{index}"), path_value(path));
        position
            .state
            .vars
            .insert(format!("__directory_stack_+{index}"), path_value(path));
    }
    let tokens = &tokens[tokens
        .iter()
        .take_while(|t| {
            matches!(
                plain(t),
                Some("then" | "do" | "else" | "elif" | "if" | "while" | "until" | "!")
            )
        })
        .count()..];
    let mut events = Vec::new();
    let mut run = new_run(dialect, position.state.clone(), &mut events, 0, 0);
    let mut unresolved = Vec::new();
    let mut text = Vec::new();
    let mut bodies = Vec::new();
    for token in tokens {
        match token {
            Tok::Word(word) => text.push(resolve(&mut run, word, Ctx::Fields, &mut unresolved)),
            Tok::Redir {
                op,
                target: Some(word),
                ..
            } => {
                text.push(format!(
                    "{op} {}",
                    resolve(&mut run, word, Ctx::Fields, &mut unresolved)
                ));
            }
            Tok::Redir {
                op,
                heredoc: Some(index),
                ..
            } => {
                if let Some(doc) = docs.get(*index) {
                    let body = run
                        .word(&doc.body, Ctx::Assign)
                        .fields
                        .ok()
                        .and_then(|fields| fields.first().and_then(Value::exact));
                    let body = body.unwrap_or_else(|| {
                        unresolved.push(doc.body.raw.clone());
                        doc.body.raw.clone()
                    });
                    text.push(format!("{op}'{}'", doc.delimiter));
                    bodies.push(format!("{body}\n{}", doc.delimiter));
                }
            }
            Tok::Array { .. } | Tok::ArithCmd(_) => {
                unresolved.push("shell assignment".into());
            }
            _ => {}
        }
    }
    let mut text = text.join(" ");
    for body in bodies {
        text.push('\n');
        text.push_str(&body);
    }
    let words = crate::hooks::git_guard::command_argv(&text);
    let launched = crate::hooks::git_guard::strip_launchers(&words);
    let stdout = launched
        .filter(|(program, _)| matches!(*program, "echo" | "printf" | "print"))
        .and_then(|_| {
            run.approx.set(false);
            let captured = run.capture(&text).0;
            if run.approx.get() {
                None
            } else {
                captured.ok().and_then(|value| value.exact())
            }
        });
    let unread_script_word = unresolved
        .iter()
        .find(|word| word.as_str() != "process substitution")
        .cloned();
    output.push(Command {
        text: text.clone(),
        cwd: position.state.directory.clone(),
        unresolved,
        stdout,
        input_code: None,
        links: Vec::new(),
    });
    // The existing evaluator owns assignments, exports, invalidation and static
    // substitutions. PWD is ordinary assignable state only for this consumer.
    if words
        .first()
        .is_some_and(|word| word == "__unresolved_shell_code")
    {
        output.push(unread_code("shell code or eval options cannot be read"));
        return position;
    }
    let reader = crate::hooks::git_guard::script_reader(&words);
    let script = if let Some(("eval", args)) = reader {
        Some(eval_body(args))
    } else if let Some((name, args)) = reader {
        crate::hooks::git_guard::shell_inline_code(name, args)
            .or_else(|| crate::security::outward::here_string(&text))
            .or_else(|| {
                tokens.iter().find_map(|token| match token {
                    Tok::Word(word)
                    | Tok::Redir {
                        target: Some(word), ..
                    } if !matches!(token, Tok::Word(_)) || !stdin_option(args) => {
                        word.parts.iter().find_map(|part| match part {
                            Part::ProcSubst(body) => {
                                run.approx.set(false);
                                let captured =
                                    run.capture(body).0.ok().and_then(|value| value.exact());
                                (!run.approx.get()).then_some(captured).flatten()
                            }
                            _ => None,
                        })
                    }
                    Tok::Redir {
                        heredoc: Some(index),
                        ..
                    } => docs.get(*index).and_then(|doc| {
                        run.word(&doc.body, Ctx::Assign)
                            .fields
                            .ok()
                            .and_then(|fields| fields.first().and_then(Value::exact))
                    }),
                    _ => None,
                })
            })
    } else {
        None
    };
    // Only supplied inline code needs a complete reading. File operands,
    // file redirects and unread external producers keep best-effort handling.
    let requires_code = reader.is_some_and(|(_, args)| {
        crate::hooks::git_guard::has_shell_code_option(args)
            || tokens
                .iter()
                .any(|token| matches!(token, Tok::Redir { op, .. } if op.contains("<<")))
    });
    if requires_code && script.is_none() && !piped {
        output.push(unread_code("inline shell code cannot be read"));
    }
    if reader.is_some() && script.is_some() {
        if let Some(word) = unread_script_word {
            output.push(unread_code(&format!("cannot read script word `{word}`")));
        }
    }
    if let Some(script) = script {
        let current_shell =
            launched.is_some_and(|(name, _)| matches!(name, "eval" | "source" | "."));
        if reader.is_some() {
            // The synthetic filename is unread, but its supplied code is known.
            if let Some(command) = output.last_mut() {
                command
                    .unresolved
                    .retain(|word| word != "process substitution");
            }
        }
        let script = crate::hooks::git_guard::launched_script(&words, &script);
        let after = reparse(&script, position.clone(), dialect, output, depth, reparses);
        return if current_shell { after } else { position };
    }
    let directory_values =
        ["PWD", "OLDPWD"].map(|name| (name, position.state.vars.get(name).cloned()));
    run.simple(tokens, docs, false, ";");
    position.state = run.state;
    if launched.is_some_and(|(program, _)| matches!(program, "cd" | "chdir" | "pushd" | "popd")) {
        // The text evaluator invalidates directory variables; this consumer
        // owns their command-position state and applies the directory effect.
        for (name, value) in directory_values {
            if let Some(value) = value {
                position.state.vars.insert(name.into(), value);
            } else {
                position.state.vars.remove(name);
            }
        }
    }
    if let Some((program, args)) = launched {
        match program {
            "cd" | "chdir" | "pushd" => change_directory(&mut position, program, args),
            "popd" if !args.is_empty() => {
                // Indexed removals and -n differ from an ordinary pop.
                // Keep them unresolved until both shell readings are modeled.
                position.state.directory = None;
                position.stack.clear();
                set_oldpwd(&mut position.state);
                set_pwd(&mut position.state);
            }
            "popd" => {
                set_oldpwd(&mut position.state);
                position.state.directory = position.stack.pop().and_then(|next| {
                    position
                        .state
                        .directory
                        .as_ref()
                        .map(|dir| lexical_join(dir, &next))
                });
                set_pwd(&mut position.state);
            }
            "source" | "." | "eval" => {
                position.state.directory = None;
                position.state.world = Some("code may change shell state");
                position.stack.clear();
                set_pwd(&mut position.state);
            }
            _ => {}
        }
    }
    position
}

fn reparse(
    script: &str,
    mut position: Position,
    dialect: Dialect,
    output: &mut Vec<Command>,
    depth: usize,
    reparses: &mut Reparses,
) -> Position {
    if let Some((_, _, after, commands)) = reparses
        .entries
        .iter()
        .find(|(before, body, _, _)| before == &position && body == script)
    {
        output.extend(commands.iter().cloned());
        return after.clone();
    }
    let before = position.clone();
    if depth < 16 && reparses.count < MAX_SCRIPT_READINGS {
        reparses.count += 1;
        if let Ok((nested, docs)) = lex(script) {
            let first = output.len();
            let mut states = walk(
                &nested,
                &docs,
                vec![position.clone()],
                dialect,
                output,
                depth + 1,
                reparses,
            );
            bound(&mut states);
            let after = if states.len() == 1 {
                states.remove(0)
            } else {
                let mut unknown = position.clone();
                unknown.state.directory = None;
                unknown.state.world = Some("shell code has several reachable states");
                unknown.stack.clear();
                set_pwd(&mut unknown.state);
                unknown
            };
            reparses.entries.push((
                before,
                script.into(),
                after.clone(),
                output[first..].to_vec(),
            ));
            return after;
        }
    }
    position.state.directory = None;
    position.state.world = Some("shell code exceeds the bounded reading");
    position.stack.clear();
    set_pwd(&mut position.state);
    output.push(unread_code("shell code exceeds the bounded reading"));
    position
}

fn set_oldpwd(state: &mut State) {
    let value =
        state
            .directory
            .as_ref()
            .map_or(Var::Unknown("unknown previous directory"), |path| {
                let actual = path_value(path);
                if state.vars.get("PWD") == Some(&actual) {
                    actual
                } else {
                    // Shells need not agree when PWD was assigned independently.
                    // Do not prove OLDPWD from a directory parameter that differs.
                    Var::Unknown("PWD differs from the working directory")
                }
            });
    state.vars.insert("OLDPWD".into(), value);
}

fn set_pwd(state: &mut State) {
    let value = state
        .directory
        .as_ref()
        .map_or(Var::Unknown("unknown working directory"), |path| {
            path_value(path)
        });
    state.vars.insert("PWD".into(), value);
}

fn path_value(path: &Path) -> Var {
    let name = crate::git::GitName::from_os_str(path.as_os_str());
    match name.rule_text() {
        Ok(text) => Var::Scalar(Value::text(text)),
        Err(_) => Var::Unknown("shell path is not UTF-8"),
    }
}

fn change_directory(position: &mut Position, program: &str, args: &[String]) {
    if program == "pushd" && args.iter().any(|arg| arg == "-n") {
        // Bash keeps this entry relative until a later rotation uses it.
        if let Some(target) = args.iter().find(|arg| !arg.starts_with('-')) {
            position.stack.push(PathBuf::from(target));
        }
        return;
    }
    if program == "pushd"
        && args.iter().any(|arg| {
            arg.starts_with('+')
                || (arg.starts_with('-') && arg[1..].chars().all(|c| c.is_ascii_digit()))
        })
    {
        set_oldpwd(&mut position.state);
        position.state.directory = None;
        position.stack.clear();
        set_pwd(&mut position.state);
        return;
    }
    if program == "cd" && args == ["-"] {
        let old = position.state.vars.get("OLDPWD").and_then(|v| match v {
            Var::Scalar(v) => v.exact(),
            _ => None,
        });
        set_oldpwd(&mut position.state);
        position.state.directory = old.map(PathBuf::from);
        set_pwd(&mut position.state);
        return;
    }
    set_oldpwd(&mut position.state);
    let changes = crate::security::outward::dir_changes(
        &std::iter::once(program.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>(),
        position
            .state
            .vars
            .get("HOME")
            .and_then(|value| match value {
                Var::Scalar(value) => value.exact(),
                _ => None,
            })
            .as_deref()
            .map(Path::new),
    );
    if program == "pushd" {
        if let Some(dir) = &position.state.directory {
            position.stack.push(dir.clone());
        }
    }
    for change in changes {
        position.state.directory = match (&position.state.directory, change) {
            (Some(dir), crate::security::outward::DirChange::To(target)) => {
                if position.state.vars.contains_key("CDPATH")
                    && !target.is_absolute()
                    && !target.starts_with(".")
                    && !target.starts_with("..")
                {
                    None
                } else {
                    Some(lexical_join(dir, &target))
                }
            }
            _ => None,
        };
    }
    if position.state.directory.is_none() {
        position.stack.clear();
    }
    set_pwd(&mut position.state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes(line: &str) -> Vec<Command> {
        commands(
            line,
            Path::new("/project"),
            Some(Path::new("/home/example")),
        )
        .into_iter()
        .filter(|step| step.text.starts_with("probe "))
        .collect()
    }

    fn values(line: &str) -> Vec<Vec<String>> {
        probes(line)
            .iter()
            .map(|step| crate::hooks::git_guard::command_argv(&step.text)[1..].to_vec())
            .collect()
    }

    #[test]
    fn reparsed_code_preserves_scope_and_has_a_per_line_budget() {
        assert!(values("eval -- 'D=src'; probe $D")
            .iter()
            .all(|v| v == &["src"]));
        assert!(values("bash <<< 'PWD=/elsewhere'; probe $PWD")
            .iter()
            .all(|v| v == &["/project"]));
        assert!(values("source /dev/stdin <<< 'D=src'; probe $D")
            .iter()
            .all(|v| v == &["src"]));
        let repeated = std::iter::repeat_n("eval 'probe src'", 80)
            .collect::<Vec<_>>()
            .join("; ");
        let steps = commands(&repeated, Path::new("/project"), None);
        assert!(!steps
            .iter()
            .any(|step| step.text == "__unresolved_shell_code"));
        let distinct = (0..80)
            .map(|i| format!("eval 'probe file{i}'"))
            .collect::<Vec<_>>()
            .join("; ");
        assert!(commands(&distinct, Path::new("/project"), None)
            .iter()
            .any(|step| step.text == "__unresolved_shell_code"));
    }

    #[test]
    fn directory_alternatives_are_unique_and_rejoin_and_or_lists() {
        let chain = (0..10)
            .map(|i| format!("cd d{i}"))
            .collect::<Vec<_>>()
            .join(" || ");
        let steps = probes(&format!("{chain}; probe $PWD"));
        assert_eq!(steps.len(), 11);
        assert!(values("W=.worktrees/w || W=src; probe $W").contains(&vec![".worktrees/w".into()]));
        for i in 0..10 {
            assert!(steps
                .iter()
                .any(|step| step.text.contains(&format!("/project/d{i}'"))));
        }
        assert!(values("cd a || cd b && probe $PWD").contains(&vec!["/project/a".into()]));
        assert!(values("cd a && cd b || probe $PWD").contains(&vec!["/project".into()]));
        let long = (0..80)
            .map(|i| format!("cd d{i}"))
            .collect::<Vec<_>>()
            .join(" || ");
        let bounded = probes(&format!("{long}; probe $PWD"));
        assert_eq!(bounded.len(), 1);
        assert!(!bounded[0].unresolved.is_empty());
        let repeated = (0..80).map(|_| "true").collect::<Vec<_>>().join(" || ");
        assert_eq!(probes(&format!("{repeated}; probe $PWD")).len(), 1);
    }

    #[test]
    fn directory_names_use_command_position_state() {
        for line in [
            "cd docs && probe $OLDPWD ~- ~1",
            "pushd docs && probe $OLDPWD ~- ~1",
            "pushd docs && popd && probe $PWD ~+ ~0",
            "cd docs && cd - && probe $PWD ~+ ~0",
        ] {
            let steps = probes(line);
            assert!(!steps.is_empty(), "{line}");
            for step in steps {
                // ~1 after cd alone is not a portable, known stack entry.
                if line.starts_with("cd docs && probe") {
                    assert!(step.unresolved.iter().any(|word| word == "~1"));
                } else {
                    assert!(step.unresolved.is_empty(), "{line}: {step:?}");
                    assert_eq!(
                        crate::hooks::git_guard::command_argv(&step.text)[1..],
                        ["/project", "/project", "/project"]
                    );
                }
            }
        }
        for word in [
            "~-",
            "~99",
            "~-1",
            "~someone",
            "$DIRSTACK",
            "$dirstack[1]",
            "$PWD:h",
            "$PWD:a",
        ] {
            assert!(
                probes(&format!("probe {word}"))
                    .iter()
                    .any(|step| !step.unresolved.is_empty()),
                "{word}"
            );
        }
    }

    #[test]
    fn resolved_control_values_keep_glob_provenance() {
        for line in [
            "D=src; if true; then probe $D/app.py; fi",
            "for f in src/*.py; do probe \"$f\"; done",
        ] {
            let steps = probes(line);
            assert!(!steps.is_empty(), "{line}");
            assert!(
                steps.iter().all(|s| s.unresolved.is_empty()),
                "{line}: {steps:?}"
            );
        }
        assert!(probes("while read f; do probe $f; done")
            .iter()
            .any(|s| !s.unresolved.is_empty()));
        for list in ["src/app.py $EXTERNAL", "src/app.py $(cat paths.txt)"] {
            assert!(
                probes(&format!("for f in {list}; do probe $f; done"))
                    .iter()
                    .any(|s| !s.unresolved.is_empty()),
                "unknown loop alternatives must survive: {list}"
            );
        }
        assert!(probes("probe <(echo x)")
            .iter()
            .all(|s| s.unresolved.iter().all(|w| !w.is_empty())));
    }

    #[test]
    fn working_directory_and_pwd_parameter_are_independent() {
        let line = "PWD=/elsewhere; probe $PWD $(pwd) `pwd` relative";
        assert!(values(line)
            .iter()
            .all(|args| args == &["/elsewhere", "/project", "/project", "relative"]));
        assert!(probes(line)
            .iter()
            .all(|step| step.cwd.as_deref() == Some(Path::new("/project"))));
        assert!(values("PWD=/elsewhere; cd docs && probe ${PWD}")
            .iter()
            .all(|args| args == &["/project/docs"]));
    }

    #[test]
    fn assignments_and_static_substitutions_share_the_text_evaluator() {
        assert!(
            values("D=src; export F=app.py; probe $D/$F $HOME/notes $(printf %s literal)")
                .iter()
                .all(|args| args == &["src/app.py", "/home/example/notes", "literal"])
        );
        assert!(values("probe '$PWD' \"$(printf %s '$PWD')\"")
            .iter()
            .all(|args| args == &["$PWD", "$PWD"]));
        assert!(probes("probe \"$EXTERNAL\" $(cat path.txt)")
            .iter()
            .all(|step| step.unresolved.len() == 2));
        for line in [
            "D=src; source external; probe $D",
            "printf() { cat external; }; probe $(printf %s ordinary)",
            "alias printf='cat external'; probe $(printf %s ordinary)",
        ] {
            assert!(
                probes(line).iter().all(|step| !step.unresolved.is_empty()),
                "{line}"
            );
        }
    }

    #[test]
    fn groups_children_and_pipeline_tails_keep_their_scope() {
        for line in [
            "(cd docs); probe $PWD",
            "cd docs & probe $PWD",
            "cd docs | cat; probe $PWD",
        ] {
            assert!(
                values(line).iter().all(|args| args == &["/project"]),
                "{line}: {:?}",
                values(line)
            );
        }
        for line in [
            "{ cd docs; }; probe $PWD",
            "cd docs || true; probe $PWD",
            "printf x | cd docs; probe $PWD",
        ] {
            let values = values(line);
            assert!(
                values.contains(&vec!["/project/docs".into()]),
                "{line}: {values:?}"
            );
            assert!(
                values.contains(&vec!["/project".into()]),
                "{line}: {values:?}"
            );
        }
        assert!(values("D=outer; (D=inner); probe $D")
            .iter()
            .all(|args| args == &["outer"]));
        assert!(values("{ D=inner; }; probe $D")
            .iter()
            .all(|args| args == &["inner"]));
    }

    #[test]
    fn unresolved_directory_and_cdpath_do_not_invent_a_pwd() {
        for line in [
            "cd $UNSET && probe $PWD",
            "CDPATH=/elsewhere; cd docs && probe $PWD",
        ] {
            assert!(
                probes(line)
                    .iter()
                    .all(|step| step.cwd.is_none() && !step.unresolved.is_empty()),
                "{line}"
            );
        }
        assert!(values("pushd docs && popd && probe $PWD")
            .iter()
            .all(|args| args == &["/project"]));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_shell_paths_remain_unresolved() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/project-\xff".to_vec()));
        for step in commands("probe $PWD $HOME $(pwd)", &path, Some(&path)) {
            assert_eq!(step.cwd.as_ref(), Some(&path));
            assert_eq!(step.unresolved.len(), 3);
        }
    }

    /// Only pwd and assignments execute. The native shell's directory must be
    /// one of the static readings, including Bash lastpipe and zsh pipelines.
    #[cfg(unix)]
    #[test]
    fn directory_state_contains_native_bash_and_zsh_results() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        for shell in ["bash", "zsh"] {
            let shell_command = || match shell {
                "bash" => std::process::Command::new("bash"),
                "zsh" => std::process::Command::new("zsh"),
                _ => unreachable!(),
            };
            if shell_command().args(["-c", "exit 0"]).status().is_err() {
                eprintln!("not installed: {shell}");
                continue;
            }
            for prefix in [
                "cd docs",
                "{ cd docs; }",
                "(cd docs)",
                "cd docs || true",
                "cd missing || true",
                "cd docs & wait",
                "printf x | cd docs",
                "PWD=/elsewhere",
                "pushd docs >/dev/null && popd >/dev/null",
            ] {
                let output = shell_command()
                    .args(["-c", &format!("{prefix}; pwd")])
                    .current_dir(&root)
                    .env_remove("CDPATH")
                    .output()
                    .unwrap();
                let printed = String::from_utf8(output.stdout).unwrap();
                let actual = printed.lines().last().unwrap();
                let steps = commands(&format!("{prefix}; probe $PWD"), &root, None);
                assert!(
                    steps
                        .iter()
                        .filter(|step| step.text.starts_with("probe "))
                        .any(|step| step.cwd.as_ref().is_none_or(|dir| dir == Path::new(actual))),
                    "{shell}: {prefix}: {actual}: {steps:?}"
                );
            }
        }
    }
}
