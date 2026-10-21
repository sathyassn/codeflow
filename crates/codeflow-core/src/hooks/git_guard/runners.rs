//! Read Git command sources through one option, configuration and location grammar.
mod catalog;
use super::{
    basename, command_argv_patterns, config_only_reads, configuration_variable,
    expand_shell_commands, expands, git_subcommand, inherited_locations, integrity_shell_path,
    launcher_effects, launcher_env, may_change_discard_state, parse_options, quote_words,
    redirect_writes, run_dirs, same_path, shown_word, split_alias, strip_launchers,
    strip_reserved_words, unresolved_word, Arity, Literals, OptionSpec, ParsedOptions, Seen,
    Violation, GIT_BUILTINS, GIT_LOCATION_VARS, GIT_PUSH_OPTIONS, GIT_REBASE_OPTIONS,
    MAX_ALIAS_DEPTH, NESTING_LIMIT, NESTING_UNREAD, RUN_DIR_LIMIT,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) struct Body {
    pub command: String,
    pub directory: PathBuf,
}

#[derive(Default)]
struct Inputs {
    globals: Vec<String>,
    environment: crate::security::git_config::Environment,
}

impl Inputs {
    fn child(
        args: &[String],
        env: &crate::security::git_config::Environment,
    ) -> Result<Self, String> {
        let settings = configuration_options(args)?;
        let parsed = checked(&settings, &GLOBAL_OPTIONS)?;
        let mut globals = Vec::new();
        for (option, value) in parsed.values {
            let name = match option {
                Seen::Short('c') => "-c",
                Seen::Long("--config-env", _) => "--config-env",
                _ => continue,
            };
            globals.extend([name.to_string(), value.to_string()]);
        }
        // The body directory represents repository setup. Submodule runners
        // clear the parent's repository-location environment for their children.
        let environment = env
            .iter()
            .filter(|(name, _)| !GIT_LOCATION_VARS.contains(&name.as_str()))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Ok(Self {
            globals,
            environment,
        })
    }
}

pub(super) fn unread(reason: &str) -> Violation {
    Violation::always_blocking(
        "git.hook_integrity",
        format!("cannot read Git runner: {reason}"),
        "use explicit, readable Git runner options and a resolvable checkout",
    )
}

pub(super) fn arguments(program: &str, args: &[String]) -> Option<Vec<String>> {
    let name = basename(program);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    if name == "git" {
        Some(args.to_vec())
    } else {
        let verb = name.strip_prefix("git-")?;
        known_program(verb).then(|| {
            std::iter::once(verb.to_string())
                .chain(args.iter().cloned())
                .collect()
        })
    }
}

fn checked<'a>(words: &'a [String], spec: &OptionSpec) -> Result<ParsedOptions<'a>, String> {
    let parsed = parse_options(words, spec);
    if let Some(word) = parsed.unread.first() {
        return Err(format!(
            "unclassified or incomplete option `{}`",
            shown_word(word)
        ));
    }
    Ok(parsed)
}

// Revision suffixes can contain the shell reader's `~` and `^` markers.
// Their literal non-option prefix cannot expand into a Git option. Other
// patterns remain unread, including patterns in an exec command's value.
fn revision_operand(word: &str) -> bool {
    let Some(at) = word.find(['~', '^']) else {
        return false;
    };
    let (name, suffix) = word.split_at(at);
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./@-".contains(c))
        && suffix
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '~' | '^'))
}

fn patterns(args: &[String], words: &[String], patterns: &[bool]) -> Result<(), String> {
    let operands = match git_subcommand(args) {
        Some((verb, rest)) if option_spec(verb).is_some() => {
            checked(rest, option_spec(verb).unwrap())?.operands
        }
        Some(("branch", rest)) => parse_options(rest, &super::GIT_BRANCH_OPTIONS).operands,
        _ => Vec::new(),
    };
    if words.iter().zip(patterns).any(|(word, pattern)| {
        *pattern && !(operands.contains(&word.as_str()) && revision_operand(word))
    }) {
        return Err("a Git runner word requires brace or filename expansion".into());
    }
    Ok(())
}

/// This table is also used by the branch reader. Its value arities identify
/// bundles, attached values and unambiguous long-option prefixes consistently.
pub(super) fn commands(args: &[String]) -> Result<Vec<String>, String> {
    let Some((verb, rest)) = git_subcommand(args) else {
        return Ok(Vec::new());
    };
    if catalog::SOURCES
        .iter()
        .any(|source| matches!(source, catalog::Source::EncodedCommand(owner) if *owner == verb))
    {
        return Err("a Git transport helper requires unclassified percent expansion".into());
    }
    match verb {
        "for-each-repo" => {
            let (_, body) = for_each(rest)?;
            Ok(vec![format!("git {}", quote_words(body))])
        }
        "bisect" if rest.first().is_some_and(|word| word == "run") => {
            if rest.len() == 1 {
                return Err("bisect run has no command".into());
            }
            Ok(vec![quote_words(&rest[1..])])
        }
        "submodule" | "submodule--helper" => {
            let at = rest
                .iter()
                .position(|word| !word.starts_with('-'))
                .unwrap_or(rest.len());
            if rest.get(at).is_none_or(|word| word != "foreach") {
                return Ok(Vec::new());
            }
            checked(&rest[..at], &SUBMODULE_OPTIONS)?;
            let words = &rest[at + 1..];
            let end = words
                .iter()
                .position(|word| !word.starts_with('-') || word == "--")
                .unwrap_or(words.len());
            checked(&words[..end], &SUBMODULE_OPTIONS)?;
            let end = end + usize::from(words.get(end).is_some_and(|word| word == "--"));
            if end == words.len() {
                return Err("submodule foreach has no command".into());
            }
            Ok(vec![words[end..].join(" ")])
        }
        _ => {
            let Some(spec) = option_spec(verb) else {
                return Ok(Vec::new());
            };
            let parsed = checked(rest, spec)?;
            if parsed.operands.iter().any(|word| unresolved_word(word)) {
                return Err("a Git runner operand expands at run time".into());
            }
            Ok(option_commands(verb, &parsed))
        }
    }
}

fn option_commands(verb: &str, parsed: &ParsedOptions<'_>) -> Vec<String> {
    let mut commands = Vec::new();
    for source in catalog::SOURCES {
        if let catalog::Source::Option(owner, short, long) = source {
            if *owner == verb {
                commands.extend(
                    parsed
                        .values_of(*short, long)
                        .into_iter()
                        .map(str::to_string),
                );
            }
        }
    }
    commands
}

/// Independent policy checks retain readable literal bodies even when an
/// unknown surrounding option makes the integrity reader refuse the runner.
pub(super) fn carried(args: &[String]) -> Vec<String> {
    if let Ok(commands) = commands(args) {
        return commands;
    }
    let Some((verb, rest)) = git_subcommand(args) else {
        return Vec::new();
    };
    option_spec(verb).map_or_else(Vec::new, |spec| {
        option_commands(verb, &parse_options(rest, spec))
    })
}

fn option_spec(verb: &str) -> Option<&'static OptionSpec> {
    match verb {
        "rebase" => Some(&GIT_REBASE_OPTIONS),
        "filter-branch" => Some(&catalog::FILTER_OPTIONS),
        "difftool" => Some(&catalog::DIFFTOOL_OPTIONS),
        "mergetool" => Some(&catalog::MERGETOOL_OPTIONS),
        "grep" => Some(&catalog::GREP_OPTIONS),
        "clone" => Some(&catalog::CLONE_OPTIONS),
        "archive" => Some(&catalog::ARCHIVE_OPTIONS),
        "daemon" => Some(&catalog::DAEMON_OPTIONS),
        "send-email" => Some(&catalog::EMAIL_OPTIONS),
        "instaweb" => Some(&catalog::INSTAWEB_OPTIONS),
        "push" => Some(&GIT_PUSH_OPTIONS),
        "fetch" | "pull" | "ls-remote" | "fetch-pack" | "send-pack" => {
            Some(&catalog::TRANSPORT_OPTIONS)
        }
        _ => None,
    }
}

pub(super) fn command_options(verb: &str) -> bool {
    option_spec(verb).is_some() || matches!(verb, "for-each-repo" | "submodule--helper")
}

fn known_program(verb: &str) -> bool {
    GIT_BUILTINS.contains(&verb) || command_options(verb) || matches!(verb, "submodule" | "bisect")
}

fn config_environment(
    env: &crate::security::git_config::Environment,
) -> crate::security::git_config::Environment {
    env.iter()
        .filter(|(name, _)| !catalog::environment(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn configured_commands(
    args: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
    earlier: &[PathBuf],
) -> Result<Vec<String>, String> {
    let Some((verb, rest)) = git_subcommand(args) else {
        return Ok(Vec::new());
    };
    let settings = configuration_options(args)?;
    let config = crate::security::git_config::read(cwd, &settings, &config_environment(env))?;
    transport(args, &config)?;
    for path in config
        .sources
        .iter()
        .chain(config.includes.iter().map(|include| &include.path))
    {
        unchanged(path, earlier)?;
    }
    let mut commands = Vec::new();
    let pagination = globals(args)?
        .iter()
        .fold(None, |choice, word| match word.as_str() {
            "-p" | "--paginate" => Some(true),
            "-P" | "--no-pager" => Some(false),
            _ => choice,
        });
    let selected_pager = format!("pager.{verb}");
    let paging = pagination == Some(true)
        || config
            .get(&selected_pager)?
            .is_some_and(|value| crate::security::git::config_boolean(value) != Some(false));
    for key in config.keys()? {
        if key == "core.editor" && !uses_editor(verb, rest) {
            continue;
        }
        let pager = key == "core.pager" || key.starts_with("pager.");
        if pager
            && (pagination == Some(false) || (key.starts_with("pager.") && key != selected_pager))
        {
            continue;
        }
        if !catalog::SOURCES.iter().any(|source| {
            matches!(source,
            catalog::Source::Config(pattern, verbs) | catalog::Source::ConfigList(pattern, verbs)
                if catalog::config_matches(key, pattern)
                && (catalog::active(verbs, verb) || (pager && paging)))
        }) {
            continue;
        }
        for value in config_values(&config, key)? {
            if value.is_empty()
                || crate::security::git::config_kind(key, value)
                    == crate::security::git::ConfigKind::Safe
            {
                continue;
            }
            // Helpers and custom submodule updates use a leading shell marker.
            commands.push(value.strip_prefix('!').unwrap_or(value).to_string());
        }
    }
    for source in catalog::SOURCES {
        if let catalog::Source::Environment(name, verbs) = source {
            if matches!(*name, "GIT_EDITOR" | "EDITOR" | "VISUAL") && !uses_editor(verb, rest) {
                continue;
            }
            let pager = matches!(*name, "PAGER" | "GIT_PAGER");
            if (pager && pagination == Some(false))
                || !(catalog::active(verbs, verb) || (pager && paging))
            {
                continue;
            }
            let value = match env.get(*name) {
                Some(value) => Some(value.clone()),
                None => match std::env::var(name) {
                    Ok(value) => Some(value),
                    Err(std::env::VarError::NotPresent) => None,
                    Err(std::env::VarError::NotUnicode(_)) => {
                        return Err(format!("Git command environment {name} is not UTF-8"));
                    }
                },
            };
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                commands.push(value);
            }
        }
    }
    Ok(commands)
}

fn configuration_options(args: &[String]) -> Result<Vec<String>, String> {
    let mut settings = globals(args)?.to_vec();
    let (verb, rest) = git_subcommand(args).ok_or("no Git subcommand")?;
    for source in catalog::SOURCES {
        if let catalog::Source::ConfigOption(owner, short, long) = source {
            if *owner == verb {
                let spec = option_spec(verb).ok_or("unclassified configuration option")?;
                for value in checked(rest, spec)?.values_of(*short, long) {
                    settings.extend(["-c".to_string(), value.to_string()]);
                }
            }
        }
    }
    Ok(settings)
}

fn uses_editor(verb: &str, rest: &[String]) -> bool {
    match verb {
        "config" => {
            let options = parse_options(rest, &super::GIT_CONFIG_OPTIONS);
            options.has_short(&['e'])
                || options.has_long("--edit")
                || options.operands.first() == Some(&"edit")
        }
        "branch" => parse_options(rest, &super::GIT_BRANCH_OPTIONS).has_long("--edit-description"),
        _ => true,
    }
}

fn config_values<'a>(
    config: &'a crate::security::git_config::EffectiveConfig,
    key: &str,
) -> Result<Vec<&'a str>, String> {
    let list = catalog::SOURCES.iter().any(|source| {
        matches!(source, catalog::Source::ConfigList(pattern, _)
            if catalog::config_matches(key, pattern))
    });
    if !list {
        return Ok(config.get(key)?.into_iter().collect());
    }
    let mut values = Vec::new();
    for (_, _, bytes) in config.get_all(key) {
        let value = std::str::from_utf8(bytes)
            .map_err(|_| format!("cannot read Git command setting {key}: not UTF-8"))?;
        if value.is_empty() {
            values.clear();
        } else {
            values.push(value);
        }
    }
    Ok(values)
}

// remote-ext has its own percent expansion and service substitutions. It is
// not a shell body, so fail closed instead of guessing its resulting argv.
fn transport(
    args: &[String],
    config: &crate::security::git_config::EffectiveConfig,
) -> Result<(), String> {
    let Some((verb, rest)) = git_subcommand(args) else {
        return Ok(());
    };
    if !catalog::active(catalog::REMOTE, verb) {
        return Ok(());
    }
    let ext = |value: &str| value.starts_with("ext::");
    let direct = rest
        .iter()
        .any(|word| ext(word) || word.split_once('=').is_some_and(|(_, value)| ext(value)));
    let mut configured = false;
    for key in config.keys()? {
        if catalog::SOURCES.iter().any(|source| {
            matches!(source, catalog::Source::Transport(pattern)
                if catalog::config_matches(key, pattern))
        }) {
            // URL rewrites put the replacement URL in the subsection name.
            configured |= key.starts_with("url.ext::");
            for (_, _, bytes) in config.get_all(key) {
                let value = std::str::from_utf8(bytes)
                    .map_err(|_| format!("Git transport setting {key} is not UTF-8"))?;
                configured |= ext(value);
            }
        }
    }
    if direct || configured {
        return Err("an ext:: transport command requires unclassified percent expansion".into());
    }
    Ok(())
}

const FOREACH_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[("--config", Arity::Value), ("--keep-going", Arity::Flag)],
    git_style: true,
};

fn for_each(rest: &[String]) -> Result<(&str, &[String]), String> {
    let mut end = rest.len();
    for length in 1..=rest.len() {
        if !parse_options(&rest[..length], &FOREACH_OPTIONS)
            .operands
            .is_empty()
        {
            end = length - 1;
            break;
        }
    }
    let parsed = checked(&rest[..end], &FOREACH_OPTIONS)?;
    let keys = parsed.values_of('\0', "--config");
    let key = keys
        .last()
        .copied()
        .ok_or("for-each-repo has no configuration key")?;
    if expands(key) || end == rest.len() {
        return Err("for-each-repo has an unread key or no command".into());
    }
    Ok((key, &rest[end..]))
}

fn saved_commands(
    args: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
    earlier: &[PathBuf],
) -> Result<Vec<String>, String> {
    let Some(("rebase", rest)) = git_subcommand(args) else {
        return Ok(Vec::new());
    };
    let parsed = checked(rest, &GIT_REBASE_OPTIONS)?;
    if !["--continue", "--skip", "--edit-todo"]
        .iter()
        .any(|flag| parsed.has_long(flag))
    {
        return Ok(Vec::new());
    }
    let repo = checkout(args, cwd, env)?;
    let path = repo.path().join("rebase-merge/git-rebase-todo");
    unchanged(&path, earlier)?;
    if crate::absence::cannot_exist(&path).map_err(|error| error.to_string())? {
        return Ok(Vec::new());
    }
    let bytes = crate::bounded_file::read_bounded_regular(&path, 1024 * 1024)
        .map_err(|error| format!("cannot read {} within 1 MiB: {error}", path.display()))?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let mut commands = Vec::new();
    for line in text
        .split('\n')
        .map(str::trim_ascii)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (verb, value) = line
            .split_once(|c: char| c.is_ascii_whitespace())
            .unwrap_or((line, ""));
        match verb {
            "exec" | "x" if !value.trim_ascii().is_empty() => {
                commands.push(value.trim_ascii().to_string());
            }
            "pick" | "p" | "reword" | "r" | "edit" | "e" | "squash" | "s" | "fixup" | "f"
            | "break" | "b" | "drop" | "d" | "label" | "l" | "reset" | "t" | "merge" | "m"
            | "update-ref" | "u" | "noop" => {}
            _ => return Err(format!("unclassified instruction in {}", path.display())),
        }
    }
    Ok(commands)
}

const SUBMODULE_OPTIONS: OptionSpec = OptionSpec {
    short: &[('q', Arity::Flag)],
    long: &[("--quiet", Arity::Flag), ("--recursive", Arity::Flag)],
    git_style: true,
};

const GLOBAL_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('C', Arity::Value),
        ('c', Arity::Value),
        ('p', Arity::Flag),
        ('P', Arity::Flag),
        ('h', Arity::Flag),
        ('v', Arity::Flag),
    ],
    long: &[
        ("--git-dir", Arity::Value),
        ("--work-tree", Arity::Value),
        ("--namespace", Arity::Value),
        ("--attr-source", Arity::Value),
        ("--shallow-file", Arity::Value),
        ("--config-env", Arity::Value),
        ("--exec-path", Arity::AttachedValue),
        ("--bare", Arity::Flag),
        ("--paginate", Arity::Flag),
        ("--no-pager", Arity::Flag),
        ("--no-replace-objects", Arity::Flag),
        ("--no-lazy-fetch", Arity::Flag),
        ("--no-optional-locks", Arity::Flag),
        ("--no-advice", Arity::Flag),
        ("--literal-pathspecs", Arity::Flag),
        ("--glob-pathspecs", Arity::Flag),
        ("--noglob-pathspecs", Arity::Flag),
        ("--icase-pathspecs", Arity::Flag),
    ],
    git_style: false,
};

fn globals(args: &[String]) -> Result<&[String], String> {
    let (_, rest) = git_subcommand(args).ok_or("no Git subcommand")?;
    let words = &args[..args.len() - rest.len() - 1];
    checked(words, &GLOBAL_OPTIONS)?;
    if words.iter().any(|word| expands(word)) {
        return Err("a Git global option expands at run time".into());
    }
    Ok(words)
}

fn aliases(
    mut args: Vec<String>,
    cwd: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Vec<String>, String> {
    for _ in 0..MAX_ALIAS_DEPTH {
        let Some((verb, rest)) = git_subcommand(&args) else {
            return Ok(args);
        };
        if known_program(verb) {
            return Ok(args);
        }
        let global = globals(&args)?;
        let config = crate::security::git_config::read(cwd, global, &config_environment(env))?;
        let Some(value) = config.get(&format!("alias.{verb}"))? else {
            return Err(format!(
                "cannot classify Git command `{}`",
                shown_word(verb)
            ));
        };
        if value.starts_with('!') {
            return Err("a shell Git alias has no provable runner argv".into());
        }
        let words = split_alias(value).ok_or("an alias Git cannot split")?;
        if words.first().is_none_or(|word| word.starts_with('-')) {
            return Err("an alias without a literal subcommand".into());
        }
        args = global
            .iter()
            .cloned()
            .chain(words)
            .chain(rest.iter().cloned())
            .collect();
    }
    Err("an alias chain deeper than the guard follows".into())
}

fn environment(
    tokens: &[String],
    literals: &Literals,
) -> Result<crate::security::git_config::Environment, String> {
    let mut env = BTreeMap::new();
    for (name, value) in &literals.0 {
        if GIT_LOCATION_VARS.contains(&name.as_str())
            || catalog::environment(name)
            || configuration_variable(name)
        {
            env.insert(
                name.clone(),
                value
                    .clone()
                    .ok_or("an unresolved Git location assignment")?,
            );
        }
    }
    for (name, value) in launcher_env(tokens)? {
        if GIT_LOCATION_VARS.contains(&name.as_str())
            || configuration_variable(&name)
            || catalog::environment(&name)
        {
            if expands(&value) && !catalog::environment(&name) {
                return Err(format!("`{name}` expands at run time"));
            }
            env.insert(name, value);
        }
    }
    Ok(env)
}

fn checkout(
    args: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<git2::Repository, String> {
    let parsed = checked(globals(args)?, &GLOBAL_OPTIONS)?;
    let mut at = cwd.to_path_buf();
    for change in parsed.values_of('C', "-C") {
        at = integrity_shell_path(change, &at);
    }
    let inherited = inherited_locations(|name| std::env::var(name))?;
    let location = |name: &str, option: &str| {
        parsed
            .values_of('\0', option)
            .last()
            .map(|value| (*value).to_string())
            .or_else(|| env.get(name).cloned())
            .or_else(|| {
                inherited
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.clone())
            })
    };
    if env.contains_key("GIT_COMMON_DIR")
        || inherited.iter().any(|(name, _)| *name == "GIT_COMMON_DIR")
        || parsed.has_long("--namespace")
    {
        return Err("the Git runner's common directory or namespace is unresolved".into());
    }
    let repo = if let Some(dir) = location("GIT_DIR", "--git-dir") {
        git2::Repository::open_ext(
            integrity_shell_path(&dir, &at),
            git2::RepositoryOpenFlags::NO_SEARCH,
            std::iter::empty::<&std::ffi::OsStr>(),
        )
        .map_err(|e| format!("cannot open Git runner directory: {}", e.message()))?
    } else {
        super::super::repo::open(&at)?.ok_or("the Git runner has no discoverable checkout")?
    };
    let root = repo
        .workdir()
        .ok_or("the Git runner has no resolvable work tree")?;
    if parsed.has_short(&['c'])
        || parsed.has_long("--config-env")
        || env.keys().any(|name| configuration_variable(name))
    {
        let config =
            crate::security::git_config::read(cwd, globals(args)?, &config_environment(env))?;
        if config.boolean("core.bare")? {
            return Err("command-local Git configuration selects a bare runner repository".into());
        }
        if let Some(tree) = config.get("core.worktree")? {
            let configured = integrity_shell_path(tree, repo.path());
            if !same_path(root, &configured).map_err(|e| e.to_string())? {
                return Err("command-local Git configuration changes the runner work tree".into());
            }
        }
    }
    if let Some(tree) = location("GIT_WORK_TREE", "--work-tree") {
        if !same_path(root, &integrity_shell_path(&tree, &at)).map_err(|e| e.to_string())? {
            return Err("the explicit Git runner work tree does not match its checkout".into());
        }
    }
    Ok(repo)
}

fn directories(
    args: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Vec<PathBuf>, String> {
    let (verb, rest) = git_subcommand(args).ok_or("no Git subcommand")?;
    if verb == "for-each-repo" {
        let (key, _) = for_each(rest)?;
        let config =
            crate::security::git_config::read(cwd, globals(args)?, &config_environment(env))?;
        let mut dirs = Vec::new();
        let global = checked(globals(args)?, &GLOBAL_OPTIONS)?;
        let origin = global
            .values_of('C', "-C")
            .iter()
            .fold(cwd.to_path_buf(), |at, change| {
                integrity_shell_path(change, &at)
            });
        for (_, _, bytes) in config.get_all(key) {
            let value =
                std::str::from_utf8(bytes).map_err(|_| "a for-each-repo directory is not UTF-8")?;
            let at = integrity_shell_path(value, &origin);
            let repo =
                super::super::repo::open(&at)?.ok_or("an unreadable for-each-repo checkout")?;
            dirs.push(
                repo.workdir()
                    .ok_or("a for-each-repo entry has no work tree")?
                    .to_path_buf(),
            );
            if dirs.len() > RUN_DIR_LIMIT {
                return Err("too many for-each-repo directories".into());
            }
        }
        return Ok(dirs);
    }
    let repo = checkout(args, cwd, env)?;
    if verb == "filter-branch" {
        let root = repo.workdir().ok_or("no filter work tree")?;
        let parsed = checked(rest, &catalog::FILTER_OPTIONS)?;
        let temporary = parsed
            .values_of('d', "")
            .last()
            .copied()
            .unwrap_or(".git-rewrite");
        if expands(temporary) {
            return Err("an unread filter-branch temporary directory".into());
        }
        return Ok(vec![
            root.to_path_buf(),
            integrity_shell_path(temporary, root).join("t"),
        ]);
    }
    if !matches!(verb, "submodule" | "submodule--helper") {
        return Ok(vec![repo
            .workdir()
            .ok_or("no runner work tree")?
            .to_path_buf()]);
    }
    let recursive = parse_options(rest, &SUBMODULE_OPTIONS).has_long("--recursive");
    let mut pending = vec![repo];
    let mut dirs = Vec::new();
    while let Some(repo) = pending.pop() {
        for module in repo.submodules().map_err(|e| e.message().to_string())? {
            let path = repo
                .workdir()
                .ok_or("no submodule work tree")?
                .join(module.path());
            if crate::absence::cannot_exist(&path.join(".git")).map_err(|e| e.to_string())? {
                continue;
            }
            let child =
                super::super::repo::open(&path)?.ok_or("an unreadable submodule checkout")?;
            dirs.push(
                child
                    .workdir()
                    .ok_or("no submodule work tree")?
                    .to_path_buf(),
            );
            if dirs.len() > RUN_DIR_LIMIT {
                return Err("too many Git runner directories to inspect".into());
            }
            if recursive {
                pending.push(child);
            }
        }
    }
    Ok(dirs)
}

pub(super) fn read(command: &str, cwd: &Path) -> Result<Vec<Body>, String> {
    let mut bodies = Vec::new();
    collect(command, cwd, 0, &Inputs::default(), &mut bodies)?;
    Ok(bodies)
}

fn unchanged(source: &Path, earlier: &[PathBuf]) -> Result<(), String> {
    let source =
        super::super::edit_guard::normalized(source, true).map_err(|error| error.to_string())?;
    for target in earlier {
        let target = super::super::edit_guard::normalized(target, true)
            .map_err(|error| error.to_string())?;
        if source.starts_with(&target) {
            return Err(format!(
                "an earlier command may rewrite runner source {}; run the commands separately",
                source.display()
            ));
        }
    }
    Ok(())
}

fn note_writes(segment: &str, args: &[String], dirs: &[PathBuf], earlier: &mut Vec<PathBuf>) {
    if !may_change_discard_state(segment) {
        return;
    }
    let redirects = redirect_writes(segment);
    let redirected = super::words_of(&redirects.targets);
    let targets = if redirected.is_empty() {
        args
    } else {
        &redirected
    };
    for target in targets
        .iter()
        .filter(|target| !target.starts_with('-') && !expands(target))
    {
        for dir in dirs {
            earlier.push(integrity_shell_path(target, dir));
        }
    }
}

fn callback_directories(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Vec<PathBuf>, String> {
    let repo = checkout(args, at, env)?;
    // Callbacks can run before setup, in the checkout, or from metadata.
    let mut dirs = vec![
        at.to_path_buf(),
        repo.workdir().ok_or("no callback work tree")?.to_path_buf(),
        repo.path().to_path_buf(),
        repo.commondir().to_path_buf(),
    ];
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

fn alias_write(rest: &[String]) -> bool {
    let parsed = parse_options(rest, &super::GIT_CONFIG_OPTIONS);
    if !parsed.unread.is_empty()
        || parsed.has_short(&['e'])
        || ["--edit", "--rename-section", "--remove-section"]
            .iter()
            .any(|flag| parsed.has_long(flag))
    {
        return false;
    }
    let operands = &parsed.operands;
    let key = match operands.first() {
        Some(&"set" | &"unset") => operands.get(1),
        _ => operands.first(),
    };
    key.is_some_and(|key| key.starts_with("alias.") && !expands(key))
}

fn config_env(
    args: &[String],
    literals: &Literals,
    tokens: &[String],
) -> Result<Vec<String>, String> {
    let (_, rest) = git_subcommand(args).ok_or("no Git subcommand")?;
    let end = args.len() - rest.len() - 1;
    let assigned = launcher_env(tokens)?;
    let mut output = Vec::new();
    let mut at = 0;
    while at < end {
        let word = &args[at];
        let setting = if word == "--config-env" {
            at += 1;
            Some(args.get(at).ok_or("missing --config-env value")?.as_str())
        } else {
            word.strip_prefix("--config-env=")
        };
        if let Some(setting) = setting {
            let (key, name) = setting
                .split_once('=')
                .ok_or("unread --config-env setting")?;
            let value = if let Some((_, value)) = assigned.iter().rev().find(|(var, _)| var == name)
            {
                value.clone()
            } else if let Some(value) = literals.0.get(name) {
                value
                    .clone()
                    .ok_or_else(|| format!("unread --config-env variable {name}"))?
            } else {
                std::env::var(name).map_err(|_| format!("unread --config-env variable {name}"))?
            };
            output.extend(["-c".to_string(), format!("{key}={value}")]);
        } else {
            output.push(word.clone());
        }
        at += 1;
    }
    output.extend(args[end..].iter().cloned());
    Ok(output)
}

fn note_config_write(args: &[String], settings: &mut bool, aliases: &mut bool) {
    if let Some(("config", rest)) = git_subcommand(args) {
        if !config_only_reads(rest) {
            if alias_write(rest) {
                *aliases = true;
            } else {
                *settings = true;
            }
        }
    }
}

fn literal_segments(segments: &[String]) -> Vec<String> {
    let mut bindings = Literals::default();
    segments
        .iter()
        .map(|segment| {
            let filled = bindings.fill(segment);
            let mut tokens = super::command_argv(&filled);
            strip_reserved_words(&mut tokens);
            bindings.note(&tokens);
            filled
        })
        .collect()
}

fn normalized_command(args: &[String], env: &crate::security::git_config::Environment) -> String {
    let mut words = Vec::new();
    if !env.is_empty() {
        words.push("env".to_string());
        words.extend(env.iter().map(|(name, value)| format!("{name}={value}")));
    }
    words.push("git".to_string());
    words.extend(args.iter().cloned());
    quote_words(&words)
}

fn collect(
    command: &str,
    cwd: &Path,
    depth: usize,
    inputs: &Inputs,
    bodies: &mut Vec<Body>,
) -> Result<(), String> {
    if depth > NESTING_LIMIT {
        return Err("Git runners nest deeper than the guard reads".into());
    }
    let expanded = expand_shell_commands(command);
    if depth > 0
        && (!expanded.unread_commands.is_empty()
            || expanded
                .segments
                .iter()
                .any(|segment| segment == NESTING_UNREAD))
    {
        return Err("a Git runner body contains an unread command word".into());
    }
    let segments = literal_segments(&expanded.segments);
    let run = run_dirs(&segments, cwd);
    let mut literals = Literals::default();
    let mut changed_sources = false;
    let mut changed_aliases = false;
    let mut earlier = Vec::new();
    for segment in segments {
        let filled = literals.fill(&segment);
        let (mut tokens, mut patterns) = command_argv_patterns(&filled);
        let before = tokens.len();
        strip_reserved_words(&mut tokens);
        patterns.drain(..before - tokens.len());
        literals.note(&tokens);
        let Some((program, args)) = strip_launchers(&tokens) else {
            continue;
        };
        let Some(args) = arguments(program, args) else {
            note_writes(&filled, args, &run.dirs, &mut earlier);
            continue;
        };
        if matches!(args.as_slice(), [flag] if flag == "--version" || flag == "-v") {
            continue;
        }
        if changed_sources
            || (changed_aliases
                && git_subcommand(&args).is_none_or(|(verb, _)| !known_program(verb)))
        {
            return Err("an earlier command may rewrite Git runner configuration or saved instructions; run the commands separately".into());
        }
        let original = args;
        let args: Vec<String> = inputs.globals.iter().chain(&original).cloned().collect();
        let args = config_env(&args, &literals, &tokens)?;
        let mut env = inputs.environment.clone();
        env.extend(environment(&tokens, &literals)?);
        for start in &run.dirs {
            let (changes, uncertain) = launcher_effects(&tokens);
            let at = changes.iter().fold(start.clone(), |at, change| {
                integrity_shell_path(change, &at)
            });
            let args = aliases(args.clone(), &at, &env)?;
            if depth > 0 && args != original {
                bodies.push(Body {
                    command: normalized_command(&args, &env),
                    directory: at.clone(),
                });
            }
            note_config_write(&args, &mut changed_sources, &mut changed_aliases);
            let mut commands = commands(&args)?;
            let configured = configured_commands(&args, &at, &env, &earlier)?;
            commands.extend(saved_commands(&args, &at, &env, &earlier)?);
            if option_spec(git_subcommand(&args).map_or("", |(verb, _)| verb)).is_some()
                || !commands.is_empty()
                || !configured.is_empty()
            {
                self::patterns(&args, &tokens, &patterns)?;
            }
            if commands.is_empty() && configured.is_empty() {
                continue;
            }
            if let Some(reason) = run.unknown.as_ref().or(uncertain.as_ref()) {
                return Err(reason.clone());
            }
            let child = Inputs::child(&args, &env)?;
            for (commands, callbacks) in [(&commands, false), (&configured, true)] {
                if commands.is_empty() {
                    continue;
                }
                let dirs = if callbacks {
                    callback_directories(&args, &at, &env)?
                } else {
                    directories(&args, &at, &env)?
                };
                for dir in dirs {
                    for command in commands {
                        collect(command, &dir, depth + 1, &child, bodies)?;
                        bodies.push(Body {
                            command: command.clone(),
                            directory: dir.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
