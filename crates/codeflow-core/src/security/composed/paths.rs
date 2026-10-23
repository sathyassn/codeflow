//! Command-position shell values for filesystem consumers. The text lexer and
//! value evaluator are shared with the composed-text check; no shell is run.
use super::{
    lex, new_run, Ctx, Dialect, Heredoc, Part, Run, State, Tok, Value, Var, Word, GRAMMAR,
};
use crate::security::outward::lexical_join;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub(crate) text: String,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) unresolved: Vec<String>,
}

#[derive(Clone)]
struct Position {
    state: State,
    stack: Vec<PathBuf>,
}

/// Resolve both supported shell readings. Pipeline tails may run in the caller
/// (zsh and Bash lastpipe), so the walk retains both the child and caller state.
pub(crate) fn commands(line: &str, cwd: &Path, home: Option<&Path>) -> Vec<Command> {
    let Ok((tokens, docs)) = lex(line) else {
        return vec![Command {
            text: line.into(),
            cwd: None,
            unresolved: vec![line.into()],
        }];
    };
    let mut result = Vec::new();
    for dialect in [Dialect::Bash, Dialect::Zsh] {
        let mut state = State {
            directory: Some(cwd.to_path_buf()),
            ..State::default()
        };
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
        );
    }
    result.dedup_by(|a, b| a.text == b.text && a.cwd == b.cwd && a.unresolved == b.unresolved);
    result
}

fn plain(tok: &Tok) -> Option<&str> {
    match tok {
        Tok::Word(word) => word.plain(),
        Tok::Op(op) => Some(op),
        _ => None,
    }
}

fn bound(states: &mut Vec<Position>) {
    if states.len() > 64 {
        let mut state = State {
            world: Some("too many reachable shell states"),
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
    while at < tokens.len() {
        if (matches!(tokens.get(at), Some(Tok::Word(_)))
            && matches!(tokens.get(at + 1), Some(Tok::Op("(")))
            && matches!(tokens.get(at + 2), Some(Tok::Op(")"))))
            || matches!(
                plain(&tokens[at]),
                Some("if" | "for" | "while" | "until" | "case" | "function")
            )
        {
            for state in &mut states {
                state.state.directory = None;
                state.state.world = Some(GRAMMAR);
                set_pwd(&mut state.state);
            }
        }
        let before = states.clone();
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
                .map(|state| simple(&tokens[start..at], docs, state, dialect, output))
                .collect();
        }
        let term = tokens.get(at).and_then(plain).unwrap_or(";");
        match term {
            "|" | "|&" => {
                pipeline.get_or_insert_with(|| before.clone());
                states = before;
            }
            "&" => {
                states.clone_from(&list_start);
                pipeline = None;
            }
            "||" => {
                states.extend(before);
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

fn resolve(run: &mut Run<'_>, word: &Word, ctx: Ctx, unresolved: &mut Vec<String>) -> String {
    // Keep literal syntax and glob provenance; only expansions need rendering.
    if word
        .parts
        .iter()
        .all(|part| matches!(part, Part::Lit { .. }))
    {
        return word.raw.clone();
    }
    if run.state.world.is_some() {
        unresolved.push(word.raw.clone());
        return word.raw.clone();
    }
    run.approx.set(false);
    let eval = run.word(word, ctx);
    if run.approx.get() {
        unresolved.push(word.raw.clone());
        return word.raw.clone();
    }
    if let Ok(fields) = eval.fields {
        if let Some(values) = fields.iter().map(Value::exact).collect::<Option<Vec<_>>>() {
            return values
                .iter()
                .map(|value| quote(value))
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    unresolved.push(word.raw.clone());
    word.raw.clone()
}

#[allow(clippy::too_many_lines)] // One command's expansions precede its state effects.
fn simple(
    tokens: &[Tok],
    docs: &[Heredoc],
    mut position: Position,
    dialect: Dialect,
    output: &mut Vec<Command>,
) -> Position {
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
    output.push(Command {
        text: text.clone(),
        cwd: position.state.directory.clone(),
        unresolved,
    });
    let words = crate::hooks::git_guard::command_argv(&text);
    let launched = crate::hooks::git_guard::strip_launchers(&words);
    // The existing evaluator owns assignments, exports, invalidation and static
    // substitutions. PWD is ordinary assignable state only for this consumer.
    run.simple(tokens, docs, false, ";");
    position.state = run.state;
    if let Some((program, args)) = launched {
        match program {
            "cd" | "chdir" | "pushd" => change_directory(&mut position, program, args),
            "popd" => {
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
                set_pwd(&mut position.state);
            }
            _ => {}
        }
    }
    position
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
        position.state.directory = None;
        set_pwd(&mut position.state);
        return;
    }
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
