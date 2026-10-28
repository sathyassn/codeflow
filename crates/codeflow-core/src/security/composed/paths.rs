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
    /// Literal stdin data established for this command position.
    input_code: Option<String>,
    /// An inner pipe or stdin redirect owns the input, including unknown input.
    input_bound: bool,
    pub(crate) links: Vec<Link>,
    pub(crate) checkouts: Vec<PathBuf>,
}

impl Command {
    /// Inherit known stdin only when no inner pipe or redirect owns it.
    pub(crate) fn input<'a>(&'a self, inherited: Option<&'a str>) -> Option<&'a str> {
        if self.input_bound {
            self.input_code.as_deref()
        } else {
            self.input_code.as_deref().or(inherited)
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Position {
    state: State,
    stack: Vec<PathBuf>,
    scripts: Vec<(PathBuf, String)>,
    /// Set once the line runs `set -P` or `set -o physical`: every later
    /// `cd` and `pushd` resolves links as `cd -P` does.
    physical: bool,
}

fn script_states(states: &[Position]) -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    for (path, body) in states.iter().flat_map(|state| &state.scripts) {
        if paths.iter().any(|(prior, _)| prior == path) {
            continue;
        }
        let body = if states.iter().all(|state| {
            state
                .scripts
                .iter()
                .any(|(other, value)| other == path && value == body)
        }) {
            body.clone()
        } else {
            "__unresolved_shell_code".into()
        };
        paths.push((path.clone(), body));
    }
    paths
}

/// Resolve both supported shell readings. Pipeline tails may run in the caller
/// (zsh and Bash lastpipe), so the walk retains both the child and caller state.
pub(crate) fn commands(line: &str, cwd: &Path, home: Option<&Path>) -> Vec<Command> {
    read_commands(line, cwd, home, false)
}

pub(crate) fn writer_commands(line: &str, cwd: &Path, home: Option<&Path>) -> Vec<Command> {
    read_commands(line, cwd, home, true)
}

/// Executable text after command-position resolution, excluding heredoc data.
pub(crate) fn execution_text(line: &str, cwd: &Path, home: Option<&Path>) -> String {
    commands(line, cwd, home)
        .into_iter()
        .map(|step| step.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_commands(line: &str, cwd: &Path, home: Option<&Path>, environment: bool) -> Vec<Command> {
    let Ok((tokens, docs)) = lex(line) else {
        return vec![Command {
            text: line.into(),
            cwd: None,
            unresolved: vec![line.into()],
            stdout: None,
            input_code: None,
            input_bound: true,
            links: Vec::new(),
            checkouts: Vec::new(),
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
        let mut reading = Vec::new();
        walk(
            &tokens,
            &docs,
            vec![Position {
                state,
                stack: Vec::new(),
                scripts: Vec::new(),
                physical: false,
            }],
            dialect,
            &mut reading,
            0,
            &mut Reparses::default(),
        );
        result.extend(position_effects(reading));
    }
    // Deduplicate only after each shell reading has its own ordered effects.
    // The same text before and after a declaration has different authority.
    let mut indices = HashMap::new();
    let mut unique: Vec<Command> = Vec::new();
    for command in result {
        let key = (
            command.cwd.clone(),
            command.text.clone(),
            command.input_code.clone(),
            command.input_bound,
            command.links.clone(),
            command.checkouts.clone(),
        );
        if let Some(&index) = indices.get(&key) {
            let prior: &mut Command = &mut unique[index];
            for word in command.unresolved {
                if !prior.unresolved.contains(&word) {
                    prior.unresolved.push(word);
                }
            }
        } else {
            indices.insert(key, unique.len());
            unique.push(command);
        }
    }
    unique
}

fn position_effects(commands: Vec<Command>) -> Vec<Command> {
    let mut links = Vec::new();
    let mut next_group = 0;
    let mut checkouts = Vec::new();
    commands
        .into_iter()
        .map(|mut command| {
            command.links.clone_from(&links);
            command.checkouts.clone_from(&checkouts);
            if let Some(dir) = &command.cwd {
                let words = pipeline_tail_words(&command.text);
                for path in crate::hooks::git_guard::planned_worktrees(&words, dir) {
                    if let Some(word) = path.to_str() {
                        match linked_paths(word, dir, &links) {
                            Ok(paths) => checkouts.extend(paths),
                            Err(why) => command.unresolved.push(why),
                        }
                    }
                    checkouts.push(path);
                }
                let mut created = created_links(&command.text, dir, command.input_code.as_deref());
                for link in &mut created {
                    if let Some(group) = &mut link.alternatives {
                        *group += next_group;
                    }
                }
                next_group += created.len();
                links.extend(created);
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
    /// Mutually exclusive destinations for the same link creation.
    alternatives: Option<usize>,
}

impl Link {
    /// The path at which this link may be created.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.from
    }
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
    let plain_cwd = crate::portable_path::without_verbatim(cwd.to_path_buf());
    let cwd = plain_cwd.as_path();
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
    let dest = cwd.join(&dest);
    let mut links = Vec::new();
    for (group, source) in copy.sources.into_iter().enumerate() {
        // The destination can be an existing directory, or one made earlier
        // on the line. Retain both spellings without consulting the hook's cwd.
        for from in [
            Some(dest.clone()),
            Path::new(&source).file_name().map(|base| dest.join(base)),
        ]
        .into_iter()
        .flatten()
        {
            let to = if symbolic {
                from.parent().unwrap_or(cwd)
            } else {
                cwd
            }
            .join(&source);
            links.push(Link {
                from,
                to,
                alternatives: Some(group),
            });
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
            alternatives: None,
            from: dest,
            to: cwd.join("$__unread_link_source"),
        }];
    }
    operands
        .into_iter()
        .map(|source| Link {
            alternatives: None,
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
            alternatives: None,
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
    let cwd = crate::portable_path::without_verbatim(cwd.to_path_buf());
    let word = crate::portable_path::without_verbatim(PathBuf::from(word));
    let original = cwd.join(word);
    let mut pending = vec![(original.clone(), vec![original.clone()], Vec::new())];
    let mut seen = std::collections::HashSet::new();
    let mut mapped = Vec::new();
    while let Some((path, ancestors, choices)) = pending.pop() {
        if !seen.insert((path.clone(), choices.clone())) {
            continue;
        }
        if seen.len() > 64 || ancestors.len() > 16 {
            return Err("cannot verify paths through too many same-line links".into());
        }
        // Follow each link before consuming a following parent component,
        // just as the kernel does. Whole-path lexical normalization loses it.
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        let mut replaced = false;
        while let Some(component) = components.next() {
            // A directory destination also admits a link below this prefix.
            // Retain that alternative, but never consume a parent component
            // on the original spelling after following a planned link.
            if replaced && matches!(component, std::path::Component::ParentDir) {
                break;
            }
            prefix.push(component.as_os_str());
            let prefix_path = crate::hooks::git_guard::prospective_path(&prefix)?;
            for (index, link) in links.iter().enumerate() {
                if link.alternatives.is_some_and(|group| {
                    choices
                        .iter()
                        .any(|&(prior, chosen)| prior == group && chosen != index)
                }) {
                    continue;
                }
                let from = crate::hooks::git_guard::prospective_path(&link.from)?;
                if prefix_path != from {
                    continue;
                }
                if link
                    .to
                    .to_str()
                    .is_none_or(crate::hooks::git_guard::unresolved_word)
                {
                    return Err("cannot verify a link target created earlier on this line".into());
                }
                let target = crate::portable_path::without_verbatim(link.to.clone())
                    .join(components.clone().as_path());
                if ancestors.contains(&target) {
                    return Err(
                        "cannot verify a cycle of links created earlier on this line".into(),
                    );
                }
                let mut next_ancestors = ancestors.clone();
                next_ancestors.push(target.clone());
                let mut next_choices = choices.clone();
                if let Some(group) = link.alternatives {
                    if !next_choices.contains(&(group, index)) {
                        next_choices.push((group, index));
                    }
                }
                pending.push((target, next_ancestors, next_choices));
                replaced = true;
            }
        }
        if !replaced && path != original {
            // prospective_path is a folded comparison key, not a filesystem
            // path. Return the case-preserving physical spelling instead.
            let resolved = crate::hooks::edit_guard::normalized(&path, true)
                .map_err(|error| error.to_string())?;
            if !mapped.contains(&resolved) {
                mapped.push(resolved);
            }
        }
    }
    Ok(mapped)
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
                        let value = run.capture_stdout(body).ok().and_then(|v| v.exact());
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
    let value = run
        .capture_stdout(line)
        .ok()
        .and_then(|value| value.exact());
    (!run.approx.get()).then_some(value).flatten()
}

fn stdin_option(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('s'))
}

fn stdin_redirect(token: &Tok) -> bool {
    let Tok::Redir { op, .. } = token else {
        return false;
    };
    let digits = op.bytes().take_while(u8::is_ascii_digit).count();
    let (fd, rest) = op.split_at(digits);
    // Bare input redirects and fd 0 (including leading zeros) bind stdin.
    fd.bytes().all(|byte| byte == b'0') && rest.starts_with('<')
}

fn unread_code(reason: &str) -> Command {
    Command {
        text: "__unresolved_shell_code".into(),
        cwd: None,
        unresolved: vec![reason.into()],
        stdout: None,
        input_code: None,
        input_bound: true,
        links: Vec::new(),
        checkouts: Vec::new(),
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
        let scripts = script_states(states)
            .into_iter()
            .map(|(path, _)| (path, "__unresolved_shell_code".into()))
            .collect();
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
            scripts,
            // A shell option set in code the guard did not read may be on.
            physical: true,
        }];
    }
}

/// Only unredirected, unshadowed literal builtins have a proven status here.
/// Other commands retain both success and failure paths.
fn literal_status(tokens: &[Tok], states: &[Position]) -> Option<bool> {
    let [Tok::Word(word)] = tokens else {
        return None;
    };
    let name = word.plain()?;
    let success = match name {
        "true" | ":" => true,
        "false" => false,
        _ => return None,
    };
    (!states.is_empty()
        && states.iter().all(|position| {
            position.state.world.is_none()
                && position.state.changed.is_empty()
                && !position.state.functions.contains_key(name)
                && !position.state.aliases.contains_key(name)
        }))
    .then_some(success)
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
        let defines_function = (matches!(tokens.get(at), Some(Tok::Word(_)))
            && matches!(tokens.get(at + 1), Some(Tok::Op("(")))
            && matches!(tokens.get(at + 2), Some(Tok::Op(")"))))
            || keyword == Some("function");
        if defines_function || matches!(keyword, Some("if" | "for" | "while" | "until" | "case")) {
            let moves = tokens[at..]
                .iter()
                .any(|token| matches!(plain(token), Some("cd" | "chdir" | "pushd" | "popd")));
            for state in &mut states {
                // Control flow alone cannot change cwd. Keep it exact for
                // ordinary loops; declarations and possible moves retain the
                // existing conservative directory reading.
                if moves || defines_function {
                    state.state.directory = None;
                }
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
        let mut exit_success = None;
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
            } else {
                let scripts = script_states(&nested);
                for state in &mut states {
                    state.scripts.clone_from(&scripts);
                }
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
            if pipeline.is_none() {
                exit_success = literal_status(&tokens[start..at], &states);
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
                if consumer.input_bound {
                    output.push(consumer);
                    continue;
                }
                let before = output.len();
                for producer in producers.iter().filter(|producer| {
                    producer.cwd == consumer.cwd || producer.cwd.is_none() || consumer.cwd.is_none()
                }) {
                    let mut command = consumer.clone();
                    command.text = format!("{} | {}", producer.text, command.text);
                    command.input_code.clone_from(&producer.stdout);
                    command.input_bound = true;
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
                        input_bound: true,
                        links: Vec::new(),
                        checkouts: Vec::new(),
                    }];
                }
                pipeline.get_or_insert_with(|| before.clone());
                let scripts = script_states(&states);
                states = before;
                for state in &mut states {
                    state.scripts.clone_from(&scripts);
                }
            }
            "&" => {
                states.clone_from(&list_start);
                pipeline = None;
            }
            "||" => {
                // Success skips the next alternative even when no variable
                // changed. A literal false has no successful branch.
                match exit_success {
                    Some(false) => {}
                    Some(true) => alternatives.append(&mut states),
                    None => {
                        alternatives.extend(states);
                        states = before;
                    }
                }
                states.append(&mut failures);
                bound(&mut alternatives);
            }
            "&&" => {
                match exit_success {
                    Some(false) => failures.append(&mut states),
                    Some(true) => {}
                    None => failures.extend(before.clone()),
                }
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

/// A NUL-delimited stream can carry bytes that a shell argument cannot.
/// Keep this narrow printf form out of command-substitution decoding.
fn nul_stdout(program: &str, args: &[String]) -> Option<String> {
    if program != "printf" || !matches!(args.first().map(String::as_str), Some("%s\\0" | "%s\\000"))
    {
        return None;
    }
    let mut output = String::new();
    if args.len() == 1 {
        output.push('\0');
    }
    for arg in &args[1..] {
        output.push_str(arg);
        output.push('\0');
    }
    Some(output)
}

/// Whether `word` is a `NAME=value` shell assignment.
fn is_assignment_word(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|ch: char| ch.is_ascii_digit())
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

/// The body recorded for a directory a recursive copy wrote: every file
/// under it may have been written on this line, with contents unknown.
const WRITTEN_TREE: &str = "__unresolved_shell_code # written tree";

fn names_written_file(position: &Position, dir: &Path, file: &str, links: &[Link]) -> bool {
    let candidate = lexical_join(dir, Path::new(file));
    if position
        .scripts
        .iter()
        .any(|(path, body)| body == WRITTEN_TREE && candidate.starts_with(path))
    {
        return true;
    }
    if crate::hooks::git_guard::has_glob(file) {
        // New files are absent from the expansion snapshot. A selector that
        // may name one cannot certify the code the interpreter will read.
        let Some(name) = candidate.file_name().and_then(|name| name.to_str()) else {
            return true;
        };
        let Ok(pattern) = glob::Pattern::new(name) else {
            return true;
        };
        return position
            .scripts
            .iter()
            .map(|(path, _)| path)
            .chain(links.iter().map(|link| &link.from))
            .any(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| {
                        pattern.matches_with(
                            name,
                            glob::MatchOptions {
                                case_sensitive: !cfg!(windows),
                                require_literal_separator: true,
                                require_literal_leading_dot: false,
                            },
                        )
                    })
            });
    }
    if !position
        .scripts
        .iter()
        .any(|(path, _)| path.file_name() == candidate.file_name())
        && !links
            .iter()
            .any(|link| candidate.starts_with(&link.from) || candidate.starts_with(&link.to))
        && !std::fs::symlink_metadata(&candidate)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return false;
    }
    let readings = |path: &Path| -> Result<Vec<PathBuf>, String> {
        let mut paths = vec![crate::hooks::git_guard::prospective_path(path)?];
        let mapped = linked_paths(
            path.to_str().ok_or("cannot read a script path")?,
            dir,
            links,
        )?;
        for path in mapped {
            paths.push(crate::hooks::git_guard::prospective_path(&path)?);
        }
        Ok(paths)
    };
    let Ok(candidates) = readings(&candidate) else {
        return true;
    };
    for (path, _) in &position.scripts {
        let Ok(written) = readings(path) else {
            return true;
        };
        if written.iter().any(|path| candidates.contains(path)) {
            return true;
        }
    }
    false
}

fn unresolved_word(word: &Word) -> String {
    if word.raw.is_empty() {
        "process substitution".into()
    } else {
        word.raw.clone()
    }
}

fn substitution_bodies(word: &Word, depth: usize) -> Vec<String> {
    if depth >= 8 {
        return vec!["__unresolved_shell_code".into()];
    }
    let mut bodies = Vec::new();
    for part in &word.parts {
        match part {
            Part::Subst { body, .. } | Part::ProcSubst(body) => bodies.push(body.clone()),
            Part::Param {
                body, braced: true, ..
            } => {
                if let Ok((tokens, _)) = lex(body) {
                    for token in tokens {
                        if let Tok::Word(word)
                        | Tok::Redir {
                            target: Some(word), ..
                        } = token
                        {
                            bodies.extend(substitution_bodies(&word, depth + 1));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    bodies
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
    // Expansions execute even when their enclosing word is only data. A
    // subshell's assignments and directory changes do not escape its scope.
    for token in tokens {
        let word = match token {
            Tok::Word(word)
            | Tok::Redir {
                target: Some(word), ..
            } => Some(word),
            Tok::Redir {
                heredoc: Some(index),
                ..
            } => docs
                .get(*index)
                .filter(|doc| !doc.quoted)
                .map(|doc| &doc.body),
            _ => None,
        };
        if let Some(word) = word {
            for body in substitution_bodies(word, 0) {
                let after = reparse(&body, position.clone(), dialect, output, depth, reparses);
                position.scripts = after.scripts;
            }
        }
    }
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
    let mut doc_input = None;
    let mut here_input = None;
    let stdin = tokens.iter().rev().find(|token| stdin_redirect(token));
    let mut doc_headers = Vec::new();
    for token in tokens {
        match token {
            Tok::Word(word) => text.push(resolve(&mut run, word, Ctx::Fields, &mut unresolved)),
            Tok::Redir {
                op,
                target: Some(word),
                ..
            } => {
                let before = unresolved.len();
                let redirect = format!(
                    "{op} {}",
                    resolve(&mut run, word, Ctx::Fields, &mut unresolved)
                );
                if op.ends_with("<<<")
                    && stdin.is_some_and(|last| std::ptr::eq(token, last))
                    && unresolved.len() == before
                {
                    here_input =
                        crate::security::outward::here_string(&redirect).map(|mut body| {
                            body.push('\n');
                            body
                        });
                }
                text.push(redirect);
            }
            Tok::Redir {
                op,
                heredoc: Some(index),
                ..
            } => {
                if let Some(doc) = docs.get(*index) {
                    let known_body = run
                        .word(&doc.body, Ctx::Assign)
                        .fields
                        .ok()
                        .and_then(|fields| fields.first().and_then(Value::exact));
                    let body = known_body.clone().unwrap_or_else(|| {
                        unresolved.push(doc.body.raw.clone());
                        doc.body.raw.clone()
                    });
                    let header = format!("{op}'{}'", doc.delimiter);
                    if stdin.is_some_and(|last| std::ptr::eq(token, last)) {
                        doc_input = known_body;
                    }
                    doc_headers.push(header.clone());
                    text.push(header);
                    bodies.push(format!("{body}\n{}", doc.delimiter));
                }
            }
            Tok::Array { .. } | Tok::ArithCmd(_) => {
                unresolved.push("shell assignment".into());
            }
            _ => {}
        }
    }
    // A here-document given to an ordinary program is data, even when its
    // contents look like interpreter commands. Keep bodies only for code
    // readers; their reparse below still sees the original tokens and docs.
    let header_words = crate::hooks::git_guard::command_argv(&text.join(" "));
    let reads_code = crate::hooks::git_guard::script_reader(&header_words).is_some()
        || crate::security::outward::reads_stdin_code(&text.join(" "));
    if !reads_code {
        text.retain(|part| !doc_headers.contains(part));
    }
    let mut text = text.join(" ");
    for body in bodies {
        if reads_code {
            text.push('\n');
            text.push_str(&body);
        }
    }
    if cfg!(windows)
        && crate::hooks::git_guard::strip_launchers(&header_words)
            .is_some_and(|(program, _)| crate::security::unresolved::writer_name(program) == "cmd")
    {
        // cmd's native body uses backslashes as separators, not shell escapes.
        text = text.replace('\\', "/");
    }
    let words = crate::hooks::git_guard::command_argv(&text);
    let launched = crate::hooks::git_guard::strip_launchers(&words);
    if launched.is_some_and(|(program, _)| position.state.aliases.contains_key(program)) {
        // Alias syntax can replace the command and introduce redirections.
        // The path reader does not expand it, so never certify its operands.
        text = "__unresolved_shell_code".into();
        unresolved.push("an alias defined earlier on this line".into());
    }
    let stdout = if launched.is_some_and(|(program, args)| {
        program == "cat" && args.iter().all(|arg| matches!(arg.as_str(), "-" | "--"))
    }) {
        doc_input.clone()
    } else {
        launched
            .filter(|(program, _)| matches!(*program, "echo" | "printf" | "print"))
            .and_then(|_| {
                run.approx.set(false);
                let captured = run.capture_stdout(&text);
                if run.approx.get() {
                    None
                } else {
                    captured.ok().and_then(|value| value.exact()).or_else(|| {
                        launched
                            .filter(|(program, _)| {
                                unresolved.is_empty()
                                    && run.state.world.is_none()
                                    && !run.state.functions.contains_key(*program)
                                    && !run.state.aliases.contains_key(*program)
                            })
                            .and_then(|(program, args)| nul_stdout(program, args))
                    })
                }
            })
    };
    let input_code = doc_input.clone().or(here_input);
    if let Some(dir) = &position.state.directory {
        let mut targets = crate::hooks::git_guard::redirect_writes(&text).targets;
        let mut trees: Vec<String> = Vec::new();
        if let Some((program, args)) = launched {
            if let Some(effects) = crate::security::unresolved::writer_effects(program, args, &[]) {
                targets.extend(effects.writes);
                // A link names an existing file rather than writing a body;
                // the line's links are judged as links (`linked_script`).
                if let Some(copy) = effects.copy.filter(|_| {
                    !matches!(
                        crate::security::unresolved::writer_name(program),
                        "ln" | "link"
                    )
                }) {
                    if let Some(dest) = copy.dest {
                        // A copy into a directory writes `dest/<source name>`
                        // (review 02, B3). A recursive copy, or one whose
                        // options the table cannot read, writes a tree.
                        let recursive = copy.unknown.is_some()
                            || copy.flags.iter().any(|flag| {
                                matches!(flag.as_str(), "--recursive" | "--archive")
                                    || !flag.starts_with("--")
                                        && flag.starts_with('-')
                                        && flag.contains(['r', 'R', 'a'])
                            });
                        for source in &copy.sources {
                            if let Some(name) =
                                Path::new(source).file_name().and_then(|n| n.to_str())
                            {
                                let child = format!("{}/{name}", dest.trim_end_matches('/'));
                                if recursive {
                                    trees.push(child);
                                } else {
                                    targets.push(child);
                                }
                            }
                        }
                        if recursive {
                            trees.push(dest.clone());
                        }
                        targets.push(dest);
                    }
                }
            }
            // The writers that write their operands, as the git guard
            // judges them: every operand may be a file run later.
            if matches!(
                crate::security::unresolved::writer_name(program),
                "tee" | "touch" | "truncate" | "ed" | "ex" | "vi" | "vim" | "nvim"
            ) {
                targets.extend(args.iter().filter(|arg| !arg.starts_with('-')).cloned());
            }
        }
        let literal_cat = launched.is_some_and(|(program, _)| program == "cat")
            && tokens
                .iter()
                .filter(|token| matches!(token, Tok::Word(_)))
                .count()
                == 1
            && !tokens
                .iter()
                .any(|token| matches!(token, Tok::Redir { op, .. } if op.contains(">>")));
        // Every file a modeled write produces may be executed later on the
        // line, whatever wrote it: a heredoc, `printf` or `echo`, an append,
        // `tee` or a copy (TSK-1002 review 01, B3). Only a body the guard
        // reads exactly is kept: a heredoc through a plain `cat`, or the
        // exact output of `echo` or `printf` with no append. Any other body
        // is unknown, and running the file refuses.
        let appends = tokens
            .iter()
            .any(|token| matches!(token, Tok::Redir { op, .. } if op.contains(">>")));
        let printed = launched
            .filter(|(program, _)| matches!(*program, "echo" | "printf" | "print"))
            .and(stdout.as_ref())
            .filter(|_| !appends && unresolved.is_empty());
        let redirected = crate::hooks::git_guard::redirect_writes(&text).targets;
        for target in targets {
            let path = lexical_join(dir, Path::new(&target));
            position.scripts.retain(|(prior, _)| prior != &path);
            let body = doc_input
                .as_ref()
                .filter(|_| literal_cat)
                .or(printed.filter(|_| redirected.contains(&target)))
                .cloned()
                .unwrap_or_else(|| "__unresolved_shell_code".into());
            position.scripts.push((path, body));
        }
        for tree in trees {
            position
                .scripts
                .push((lexical_join(dir, Path::new(&tree)), WRITTEN_TREE.into()));
        }
    }
    let unread_script_word = unresolved
        .iter()
        .find(|word| word.as_str() != "process substitution")
        .cloned();
    output.push(Command {
        text: text.clone(),
        cwd: position.state.directory.clone(),
        unresolved,
        stdout,
        input_code,
        input_bound: stdin.is_some(),
        links: Vec::new(),
        checkouts: Vec::new(),
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
    // Inline code after a directory change the guard cannot establish
    // refuses whole: `eval`, a shell's `-c` body, and code read from stdin
    // (TSK-1002 review 03, ruling 4). The guard does not read names out of
    // code it cannot place.
    if position.state.directory.is_none()
        && (matches!(reader, Some(("eval", _)))
            || reader.is_some_and(|(name, args)| {
                crate::hooks::git_guard::shell_inline_code(name, args).is_some()
            })
            || crate::security::outward::reads_stdin_code(&text))
    {
        output.push(unread_code(
            "inline code runs after a directory change the guard cannot establish",
        ));
        return position;
    }
    if !position.scripts.is_empty() {
        let links = position_effects(output.clone())
            .last()
            .map(|command| command.links.clone())
            .unwrap_or_default();
        let linked_script = links.iter().any(|link| {
            position
                .scripts
                .iter()
                .any(|(path, _)| path.starts_with(&link.from) || path.starts_with(&link.to))
        });
        // `env -S` packs the command, its PATH and its directory into one
        // string the launcher walk does not open: after a write on the line
        // it refuses (TSK-1002 review 03, ruling 3).
        if launched.is_none() && crate::hooks::git_guard::env_split_command(&words).is_some() {
            let mut unread =
                unread_code("a file written earlier on this line may run from an env -S string");
            unread.input_code = Some("__unresolved_shell_code".into());
            output.push(unread);
        }
        if let Some((program, args)) = launched {
            let family = crate::security::outward::interpreter_family(
                crate::hooks::git_guard::basename(program),
            );
            let interpreter = family.is_some();
            let make = matches!(
                crate::security::unresolved::writer_name(program),
                "make" | "gmake"
            );
            let mut candidates = Vec::new();
            let file_arguments = family
                .filter(|family| matches!(*family, "python" | "node" | "nodejs" | "perl" | "ruby"))
                .map(|family| crate::security::outward::interpreter_program_files(family, args));
            if program.contains(['/', '\\']) {
                candidates.push(program);
            }
            if let Some(files) = &file_arguments {
                candidates.extend(files.iter().map(String::as_str));
            } else if interpreter
                || (reader.is_none_or(|(name, args)| {
                    crate::hooks::git_guard::shell_inline_code(name, args).is_none()
                }) && !crate::hooks::git_guard::reads_shell_stdin(&words)
                    && (make || reader.is_some()))
            {
                candidates.extend(args.iter().map(String::as_str));
            }
            let explicit_makefile = make
                && args.iter().any(|arg| {
                    matches!(arg.as_str(), "-f" | "--file" | "--makefile")
                        || arg.starts_with("-f")
                        || arg.starts_with("--file=")
                        || arg.starts_with("--makefile=")
                });
            if make && !explicit_makefile {
                candidates.extend(["GNUmakefile", "makefile", "Makefile"]);
            }
            let mut directory = position.state.directory.clone();
            // A launcher's directory (`env -C sub python3 script`) is where
            // the script operand is opened (review 02, B3); one the guard
            // cannot read leaves the directory unknown.
            let (moves, uncertain) = crate::hooks::git_guard::launcher_effects(&words);
            let launcher_moved = uncertain.is_some() || !moves.is_empty();
            if uncertain.is_some()
                || moves.iter().any(|dir| {
                    crate::hooks::git_guard::unresolved_word(dir)
                        || crate::hooks::git_guard::has_glob(dir)
                })
            {
                directory = None;
            } else {
                for target in moves {
                    directory = directory.and_then(|dir| {
                        crate::security::outward::physical_join(&dir, Path::new(target))
                    });
                }
            }
            // The words in front of the program: assignments and launchers.
            // Unknown placement reads as all of them (fail closed).
            // An env whose options the launcher walk stops at (`env -S`)
            // is itself the program: all of its words count.
            let prefix = words
                .iter()
                .position(|word| std::ptr::eq(word.as_str(), program))
                .filter(|_| crate::hooks::git_guard::launcher_name(program) != "env")
                .map_or(&words[..], |at| &words[..at]);
            // An execution context the guard does not fully model: a
            // launcher option that splits a command or sets the search path
            // (`env -S`, `env -P`), or an interpreter's own directory option
            // (`ruby -C`, `pwsh -wd`). After a write on the line, a
            // relative or bare-name run there refuses (TSK-1002 review 03,
            // ruling 3).
            let unmodeled = prefix.iter().any(|word| {
                word.starts_with('-') && !word.starts_with("--") && word.contains(['S', 'P'])
                    || word.starts_with("--s")
            }) || match family {
                Some("ruby") => args
                    .iter()
                    .any(|arg| arg.starts_with("-C") || arg.starts_with("--chdir")),
                Some("powershell") => {
                    crate::hooks::git_guard::powershell_working_directory(args).is_some()
                }
                _ => false,
            };
            if unmodeled {
                directory = None;
            }
            // A bare name runs from PATH; once the line changes PATH, in the
            // shell or in any launcher word (`env PATH=. run`, `env -S
            // 'PATH=. run'`), it may name a file the line wrote.
            let path_changed = unmodeled
                || position.state.world.is_some()
                || position.state.vars.contains_key("PATH")
                || position.state.changed.contains("PATH")
                || prefix.iter().any(|word| word.contains("PATH="));
            let path_names_written = path_changed
                && !program.contains(['/', '\\'])
                && (unmodeled
                    || position.scripts.iter().any(|(path, body)| {
                        body == WRITTEN_TREE
                            || path.file_name() == Some(std::ffi::OsStr::new(program))
                    }));
            // A recursive copy writes files the guard cannot list: any
            // interpreter, shell, Make or launcher after it on the line may
            // load one, by import, module search or PATH (ruling 2).
            let tree_written = position
                .scripts
                .iter()
                .any(|(_, body)| body == WRITTEN_TREE);
            let runs_after_tree = tree_written
                && (interpreter
                    || make
                    || reader.is_some()
                    || prefix.iter().any(|word| !is_assignment_word(word)));
            if make {
                let mut at = 0;
                while let Some(arg) = args.get(at) {
                    let target = if matches!(arg.as_str(), "-C" | "--directory") {
                        at += 1;
                        args.get(at).map(String::as_str)
                    } else {
                        arg.strip_prefix("--directory=")
                            .or_else(|| arg.strip_prefix("-C"))
                    };
                    if let Some(target) = target {
                        directory = directory.and_then(|dir| {
                            crate::security::outward::physical_join(&dir, Path::new(target))
                        });
                    }
                    at += 1;
                }
            }
            let executes_written = directory.as_ref().map_or(
                !position.scripts.is_empty() && !candidates.is_empty(),
                |dir| {
                    candidates.iter().any(|file| {
                        let file = file
                            .strip_prefix("--file=")
                            .or_else(|| file.strip_prefix("--makefile="))
                            .or_else(|| file.strip_prefix("-f").filter(|value| !value.is_empty()))
                            .unwrap_or(file);
                        names_written_file(&position, dir, file, &links)
                    })
                },
            );
            // An interpreter also loads the modules it imports, so a file
            // written on this line with a source extension it loads counts
            // as executed, wherever it was written.
            let loads: &[&str] = match family {
                Some("python") => &["py", "pyc", "pth"],
                Some("node" | "nodejs") => &["js", "mjs", "cjs", "node"],
                Some("ruby") => &["rb"],
                Some("perl") => &["pl", "pm"],
                Some("powershell") => &["ps1", "psm1"],
                Some("deno") => &["ts", "tsx", "js", "jsx", "mjs"],
                _ => &[],
            };
            let executes_written = executes_written
                || path_names_written
                || runs_after_tree
                || position.scripts.iter().any(|(path, _)| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| {
                            loads.contains(&extension.to_ascii_lowercase().as_str())
                        })
                });
            let unverified_selection = candidates
                .iter()
                .any(|file| crate::hooks::git_guard::has_glob(file));
            // The body below is read from the line's directory, so a script
            // a launcher runs from another directory is not read raw.
            let raw_body = !launcher_moved
                && directory.as_ref().is_some_and(|dir| {
                    reader.is_some_and(|(_, args)| {
                        args.iter()
                            .find(|arg| !arg.starts_with('-'))
                            .is_some_and(|file| {
                                let file = lexical_join(dir, Path::new(file));
                                position.scripts.iter().any(|(path, _)| path == &file)
                            })
                    })
                });
            // A shell started with a changed PATH (`env PATH=. sh -c run`)
            // runs its inline code's bare names from that PATH, which the
            // nested reading does not carry.
            let shell_code_on_path = path_changed
                && reader.is_some_and(|(name, args)| {
                    crate::hooks::git_guard::shell_inline_code(name, args).is_some()
                });
            if shell_code_on_path
                || executes_written
                    && (reader.is_none()
                        || interpreter
                        || linked_script
                        || unverified_selection
                        || !raw_body)
            {
                let mut unread = unread_code("a file written earlier on this line is executed without a verified code reading");
                unread.input_code = Some("__unresolved_shell_code".into());
                output.push(unread);
            }
        }
    }
    let script = if let Some(("eval", args)) = reader {
        Some(eval_body(args))
    } else if let Some((name, args)) = reader {
        crate::hooks::git_guard::shell_inline_code(name, args)
            .or_else(|| {
                let file = args.iter().find(|arg| !arg.starts_with('-'))?;
                let dir = position.state.directory.as_ref()?;
                let path = lexical_join(dir, Path::new(file));
                position
                    .scripts
                    .iter()
                    .find(|(prior, _)| prior == &path)
                    .map(|(_, code)| code.clone())
            })
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
                                let captured = run
                                    .capture_stdout(body)
                                    .ok()
                                    .and_then(|value| value.exact());
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
        position.scripts.clone_from(&after.scripts);
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
        if program == "set"
            && (args
                .iter()
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('P'))
                || args
                    .windows(2)
                    .any(|pair| pair[0] == "-o" && pair[1] == "physical")
                || args.iter().any(|arg| arg == "-ophysical"))
        {
            position.physical = true;
        }
        let physical = matches!(program, "cd" | "chdir" | "pushd")
            && (position.physical
                || position.state.world.is_some()
                || args
                    .iter()
                    .any(|arg| arg.starts_with('-') && arg.contains('P')));
        if physical {
            // Planned links are not on disk yet. Retain their kernel reading
            // before a physical cd consumes parent components.
            let links = position_effects(output.clone())
                .last()
                .map(|step| step.links.clone())
                .unwrap_or_default();
            if let (Some(dir), Some(target)) = (
                position.state.directory.as_ref(),
                args.iter().find(|arg| !arg.starts_with('-')),
            ) {
                match linked_paths(target, dir, &links) {
                    Ok(paths) if paths.len() == 1 => {
                        position.state.directory = paths.into_iter().next();
                        set_pwd(&mut position.state);
                        return position;
                    }
                    Ok(paths) if paths.is_empty() => {}
                    _ => {
                        position.state.directory = None;
                        set_pwd(&mut position.state);
                        output.push(unread_code("cannot establish one physical directory through links declared on this line"));
                        return position;
                    }
                }
            }
            // A physical change through `..` or an existing link resolves
            // where the guard does not follow it: the directory becomes
            // unknown, so the unknown-directory rules apply (TSK-1002 review
            // 03, ruling 5). A plain physical change keeps its spelling.
            if let (Some(dir), Some(target)) = (
                position.state.directory.clone(),
                args.iter().find(|arg| !arg.starts_with('-')),
            ) {
                let lexical = lexical_join(&dir, Path::new(target));
                let through_parent = Path::new(target)
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir));
                let through_link =
                    crate::portable_path::canonicalize(&lexical).is_ok_and(|real| real != lexical);
                if through_parent || through_link {
                    set_oldpwd(&mut position.state);
                    position.state.directory = None;
                    position.stack.clear();
                    set_pwd(&mut position.state);
                    return position;
                }
            }
        }
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
                unknown.scripts = script_states(&states);
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

    #[cfg(windows)]
    #[test]
    fn verbatim_cwd_keeps_parent_components_until_planned_links_resolve() {
        let temp = tempfile::tempdir().unwrap();
        let verbatim = temp.path().canonicalize().unwrap();
        let root = crate::portable_path::without_verbatim(verbatim.clone());
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let links = created_links("ln -s .git/hooks l", &verbatim, None);
        let paths = linked_paths("l/../config", &verbatim, &links).unwrap();
        assert!(paths.contains(&root.join(".git/config")), "{paths:?}");
    }

    #[test]
    fn r26_unused_heredoc_does_not_certify_the_written_script() {
        let temp = tempfile::tempdir().unwrap();
        let line = "cat 3<<'E' > x.py; python3 x.py\nprint(1)\nE";
        let readings = commands(line, temp.path(), None);
        assert!(
            readings.iter().any(|step| step
                .unresolved
                .iter()
                .any(|why| { why.contains("written earlier on this line") })),
            "{readings:?}"
        );
    }

    /// Review 01, B3: a file written on the line by any modeled producer and
    /// then run, or imported by an interpreter, is unread code. A shell
    /// script whose exact body is known is read instead; a data file passed
    /// to a program as its argument is not run.
    #[test]
    fn every_written_file_that_runs_is_read_or_refused() {
        let temp = tempfile::tempdir().unwrap();
        let unread = |line: &str| {
            commands(line, temp.path(), None).iter().any(|step| {
                step.unresolved.iter().any(|why| {
                    why.contains("written earlier on this line") || why.contains("cannot be read")
                })
            })
        };
        for line in [
            "printf 'print(1)' > run.py; python3 run.py",
            "echo 'print(1)' > run.py && python3 run.py",
            "echo 'print(1)' >> run.py; python3 run.py",
            "echo x | tee run.py; python3 run.py",
            "cp other.py run.py; python3 run.py",
            "printf 'echo x' > run.sh; ./run.sh",
            "printf 'x:\\n\\techo x' > Makefile; make",
            "printf 'x=1' > helper.py; python3 main.py",
            "printf 'x' > lib.js; node --require ./lib.js app.js",
            "echo \"$X\" > run.sh; bash -x run.sh",
            // Review 02, B3: a copy's child in its directory, a recursive
            // copy's tree, a launcher's directory and a changed PATH.
            "cp other.py out/; python3 out/other.py",
            "cp other.py out; python3 out/other.py",
            "cp -t out other.py; python3 out/other.py",
            "cp -r src out; sh out/src/run.sh",
            "cp -a src out; ./out/src/run",
            "cp -R src out; ./out/run",
            "printf x > sub/script; env -C sub python3 script",
            "printf x > sub/script; env --chdir=sub sh script",
            "printf x > sub/Makefile; make -C sub",
            "printf x > run; chmod +x run; PATH=.:$PATH run",
            "printf x > run; chmod +x run; export PATH=.:$PATH; run",
            "printf x > run; chmod +x run; PATH=$PWD:$PATH; run",
            // Review 03, ruling 2: any interpreter, shell, Make or launcher
            // after a recursive copy, imports and module search included.
            "cp -r src out; python3 -c 'import out.src.helper'",
            "cp -r src out; python3 -m out.src.tool",
            "cp -r src out; PYTHONPATH=out python3 main.py",
            "cp -a src out; node -e 'require(\"./out/src/lib\")'",
            "mkdir L && rsync -a src/ L/ && python3 tool.py L/app.py",
            "cp -R src out; nice ./tool",
            // Ruling 3: an execution context the guard does not model.
            "printf x > sub/script; ruby -Csub script",
            "cp -r src out; env PATH=out/src:$PATH run",
            "printf x > run; env PATH=.:$PATH run",
            "printf x > run; env -i PATH=. run",
            "printf x > run; env -P . run",
            "printf x > run; env -S 'PATH=. run'",
            "printf x > run; nice env PATH=. run",
            "printf x > run; env PATH=. sh -c run",
        ] {
            assert!(unread(line), "allowed: {line}");
        }
        for line in [
            "python3 gen.py > out.json; python3 use.py out.json",
            "printf 'x' > notes.txt; cat notes.txt",
            "echo x > log.txt; node app.js log.txt",
            "cp other.py out/; cat out/other.py",
            "printf x > run; ls",
            "cp -r src out; ls out",
            "printf x > run; env PATH=. ls",
        ] {
            assert!(!unread(line), "refused: {line}");
        }
        // A shell script with an exact body is read as that body.
        let text: Vec<_> = commands("printf 'cat notes.txt' > r.sh; sh r.sh", temp.path(), None)
            .into_iter()
            .map(|step| step.text)
            .collect();
        assert!(
            text.iter().any(|step| step.contains("cat notes.txt")),
            "{text:?}"
        );
    }

    #[test]
    fn r25_stdout_retains_newlines_while_substitution_removes_them() {
        for (line, expected) in [
            ("printf '%s\\n' public", "public\n"),
            ("printf '%s\\n\\n' public", "public\n\n"),
            ("echo public", "public\n"),
            ("printf '%s' public", "public"),
        ] {
            assert_eq!(literal_output(line).as_deref(), Some(expected), "{line}");
        }
        let commands = probes("P=$(printf '%s\\n\\n' public); probe \"$P\"");
        assert!(!commands.is_empty());
        for command in commands {
            assert_eq!(
                crate::hooks::git_guard::command_argv(&command.text),
                ["probe", "public"]
            );
        }
    }

    #[test]
    fn review3_physical_link_child_parent_has_one_result() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(temp.path())
            .unwrap()
            .join("MiXeD");
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        let links = [Link {
            alternatives: None,
            from: root.join("l"),
            to: root.join(".git"),
        }];
        for word in ["l/hooks/..", "./l/hooks/..", "l/hooks/../config"] {
            let expected = if word.ends_with("config") {
                root.join(".git/config")
            } else {
                root.join(".git")
            };
            assert_eq!(
                linked_paths(word, &root, &links).unwrap(),
                [expected],
                "{word}"
            );
        }
    }

    #[test]
    fn r23_directory_link_alternatives_keep_the_descendant_destination() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(temp.path())
            .unwrap()
            .join("MiXeD");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join(".worktrees/w/.claude")).unwrap();
        let links = created_links("ln -s ../.worktrees/w docs", &root, None);
        let paths = linked_paths("docs/w/.claude", &root, &links).unwrap();
        assert!(
            paths.contains(&root.join(".worktrees/w/.claude")),
            "{paths:?}"
        );
    }

    #[test]
    fn r24_written_and_executed_link_paths_share_comparison_keys() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(temp.path())
            .unwrap()
            .join("MiXeD");
        std::fs::create_dir_all(root.join("clean")).unwrap();
        for (written, executed) in [("l/x.py", "clean/x.py"), ("clean/x.py", "l/x.py")] {
            let line =
                format!("ln -s clean l; cat <<'E' > {written}; python3 {executed}\nprint(1)\nE");
            let readings = commands(&line, &root, None);
            assert!(
                readings.iter().any(|step| step
                    .unresolved
                    .iter()
                    .any(|why| { why.contains("written earlier on this line") })),
                "{line}: {readings:?}"
            );
        }
    }

    #[test]
    fn r24_one_link_choice_can_be_followed_more_than_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(temp.path()).unwrap();
        let links = created_links("ln -s . l", &root, None);
        let paths = linked_paths("l/l/notes", &root, &links).unwrap();
        assert_eq!(paths, [root.join("notes")]);
    }

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

    /// Directory values are native paths; compare them with `/` on Windows too.
    fn slashes(text: &str) -> String {
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text.to_owned()
        }
    }

    fn values(line: &str) -> Vec<Vec<String>> {
        probes(line)
            .iter()
            .map(|step| {
                crate::hooks::git_guard::command_argv(&step.text)[1..]
                    .iter()
                    .map(|arg| slashes(arg))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn tsk1002_alias_calls_and_data_heredocs_have_distinct_meanings() {
        for line in [
            "alias wipe='rm -rf'; wipe .git",
            "alias wipe='rm -rf'\nwipe .git",
            "alias w='printf x >'\nw .codeflow/policy.json",
        ] {
            let steps = commands(line, Path::new("/fixture"), None);
            assert!(
                steps
                    .iter()
                    .any(|step| step.text == "__unresolved_shell_code"
                        && step.unresolved.iter().any(|why| why.contains("alias"))),
                "{line}: {steps:?}"
            );
        }
        let line = "cat > notes.txt <<'EOF'\npython3 tool.py .claude/settings.json\nEOF";
        assert!(!execution_text(line, Path::new("/fixture"), None).contains("python3"));
        assert!(execution_text(
            "python3 <<'EOF'\nprint(1)\nEOF",
            Path::new("/fixture"),
            None
        )
        .contains("print(1)"));
    }

    #[cfg(unix)]
    #[test]
    fn tsk1002_same_line_links_resolve_before_parent_components() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".codeflow/rules")).unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "fixture").unwrap();
        let links = vec![Link {
            alternatives: None,
            from: root.join("l"),
            to: root.join(".codeflow/rules"),
        }];
        let modeled = linked_paths("l/../policy.json", &root, &links).unwrap();
        std::os::unix::fs::symlink(".codeflow/rules", root.join("l")).unwrap();
        let native = root.join("l/../policy.json").canonicalize().unwrap();
        assert!(modeled.contains(&native), "{modeled:?}: {native:?}");
    }

    #[test]
    fn tsk1002_prospective_checkouts_follow_command_positions() {
        let steps = commands("python3 before.py /tmp/new-checkout; git worktree add -b task/new /tmp/new-checkout; python3 after.py /tmp/new-checkout", Path::new("/fixture"), None);
        assert!(steps
            .iter()
            .find(|s| s.text.contains("before.py"))
            .unwrap()
            .checkouts
            .is_empty());
        assert!(steps
            .iter()
            .find(|s| s.text.contains("after.py"))
            .unwrap()
            .checkouts
            .contains(&PathBuf::from("/tmp/new-checkout")));
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
    fn literal_statuses_rejoin_only_reachable_and_or_paths() {
        for line in [
            "false || D=build && probe $D",
            "false && D=private || D=build; probe $D",
            "D=build; true || D=private; probe $D",
            "D=build; : || D=private; probe $D",
            "D=build; false && D=private; probe $D",
            "false || true || D=private; D=build; probe $D",
        ] {
            let steps = probes(line);
            assert_eq!(steps.len(), 1, "{line}: {steps:?}");
            assert!(steps[0].unresolved.is_empty(), "{line}: {steps:?}");
            assert_eq!(values(line), [vec!["build".to_string()]], "{line}");
        }
        assert!(probes("false && probe private").is_empty());
        assert!(probes("true || probe private").is_empty());
        for line in [
            "D=private; unknown || D=public && probe $D",
            "D=private; true || D=public && probe $D",
            "D=private; unknown && false || probe $D",
            "D=private; unknown || false && probe $D",
            "D=private; alias false=true; false || D=public && probe $D",
            "D=private; false() { true; }; false || D=public && probe $D",
            "D=private; enable -n false; false || D=public && probe $D",
            "D=private; disable $name; false || D=public && probe $D",
            "D=private; alias $definition; false || D=public && probe $D",
            "D=private; false 2>/dev/null || D=public && probe $D",
            "D=private; unknown | false || D=public && probe $D",
        ] {
            assert!(
                values(line).contains(&vec!["private".into()])
                    || probes(line).iter().any(|step| !step.unresolved.is_empty()),
                "success must remain reachable or unverified: {line}: {:?}",
                probes(line)
            );
        }
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
                .any(|step| slashes(&step.text).contains(&format!("/project/d{i}'"))));
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
                        crate::hooks::git_guard::command_argv(&slashes(&step.text))[1..],
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
        let steps = commands("probe $PWD $HOME $(pwd)", &path, Some(&path));
        assert!(steps.iter().any(|step| step.text == "pwd"));
        let parents: Vec<_> = steps
            .into_iter()
            .filter(|step| step.text.starts_with("probe "))
            .collect();
        assert_eq!(parents.len(), 1);
        for step in parents {
            assert_eq!(step.cwd.as_ref(), Some(&path));
            assert_eq!(step.unresolved.len(), 3);
        }
    }

    /// Only builtins execute. The native shell's directory must be
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
                "false || cd docs && true",
                "false && cd missing || cd docs",
                "cd docs && true || cd missing",
                "cd docs && false || true",
                "true || cd missing && cd docs",
                "false || true || cd missing",
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
