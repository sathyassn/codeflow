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

/// The environment variables whose value is a program Git runs, from the
/// runner catalog.
pub(super) fn program_environment_names() -> impl Iterator<Item = &'static str> {
    catalog::environment_names()
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
    checked_mode(words, spec, false)
}

fn checked_mode<'a>(
    words: &'a [String],
    spec: &OptionSpec,
    stop: bool,
) -> Result<ParsedOptions<'a>, String> {
    let parsed = super::parse_options_mode(words, spec, stop);
    for word in &parsed.unread {
        // Unknown flags do not carry a command. Keep scanning the remaining
        // argv, including short bundles, for the command options we do know.
        let spelling = word.split('=').next().unwrap_or(word);
        let candidates: Vec<_> = spec
            .long
            .iter()
            .filter(|(name, _)| name.starts_with(spelling))
            .map(|(name, _)| *name)
            .collect();
        let known =
            parsed.missing.contains(word) || (word.starts_with("--") && !candidates.is_empty());
        if known {
            return Err(format!(
                "incomplete or ambiguous option `{}`; use {} with an explicit readable value or command",
                shown_word(word),
                if candidates.is_empty() { shown_word(word) } else { candidates.join(" or ") }
            ));
        }
    }
    Ok(parsed)
}

// Revision suffixes can contain the shell reader's `~` and `^` markers.
// Their literal non-option prefix cannot expand into a Git option. Other
// patterns remain unread, including patterns in an exec command's value.
pub(super) fn revision_operand(word: &str) -> bool {
    if word.is_empty() || word.starts_with(['-', '~', '^', ':']) {
        return false;
    }
    // `REV^!` is the commit without its parents (`git help revisions`).
    if let Some(base) = word.strip_suffix('!').filter(|base| base.ends_with('^')) {
        return revision_operand(base);
    }
    // These are Git revision operators, not arbitrary shell patterns. Keep
    // command substitution, globbing and brace lists out of this exception.
    let mut chars = word.chars().peekable();
    let mut operator = false;
    while let Some(ch) = chars.next() {
        match ch {
            '@' if chars.peek() == Some(&'{') => {
                chars.next();
                let mut closed = false;
                for selector in chars.by_ref() {
                    if selector == '}' {
                        closed = true;
                        break;
                    }
                    if !(selector.is_alphanumeric() || "_./@- :+".contains(selector)) {
                        return false;
                    }
                }
                if !closed {
                    return false;
                }
                operator = true;
            }
            '~' | '^' | ':' => operator = true,
            ch if ch.is_alphanumeric() || "_./@-".contains(ch) => {}
            _ => return false,
        }
    }
    operator || word.contains("..")
}

// Only the current user's home has a proven expansion: the HOME the line
// assigned, else the inherited one (TSK-379). The remaining path must be
// literal; named-user homes and glob/brace suffixes stay unread.
fn home_operand(word: &str) -> bool {
    let Some(tail) = word
        .strip_prefix("~/")
        .or_else(|| (word == "~").then_some(""))
    else {
        return false;
    };
    !expands(tail)
        && !tail.chars().any(super::pattern_character)
        && super::line_home::home().is_some_and(|home| home.is_absolute())
}

fn patterns(
    args: &[String],
    words: &[String],
    patterns: &[bool],
    segment: &str,
) -> Result<(), String> {
    if let Some(("grep", rest)) = git_subcommand(args) {
        return grep_syntax(rest, segment);
    }
    let command_values = commands(args)?;
    let expanded = words.iter().zip(patterns).find(|(word, pattern)| {
        **pattern
            && ((!revision_operand(word) && !home_operand(word)) || command_values.contains(word))
    });
    // A leading `^` is a zsh negation glob that may expand to any name in
    // the directory, an option-like name included, so it stays refused;
    // the message says to quote a `^REV` exclusion (TSK-1002 Fable review
    // 17).
    match expanded {
        Some((word, _)) if word.starts_with('^') => Err(format!(
            "a Git runner word requires filename expansion: zsh may read `{word}` as a \
             negation glob; quote it when it is a revision (`'{word}'`)"
        )),
        Some(_) => Err("a Git runner word requires brace or filename expansion".into()),
        None => Ok(()),
    }
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
            let body = &words[end..];
            Ok(vec![if body.len() == 1 {
                body[0].clone()
            } else {
                quote_words(body)
            }])
        }
        _ => {
            let Some(spec) = option_spec(verb) else {
                return Ok(Vec::new());
            };
            let parsed = checked_mode(rest, spec, verb == "grep")?;
            if verb != "grep" && parsed.operands.iter().any(|word| unresolved_word(word)) {
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
/// incomplete command option makes the integrity reader refuse the runner.
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

/// The files and directories a Git command writes through its own output
/// options and operands (TSK-379 review rounds 5 and 9), read with the same
/// option tables, arities and unique-prefix resolution as every other Git
/// verb here: `--output` on any verb that takes diff options, `archive -o`,
/// `format-patch -o`, `bundle create FILE`, `merge-file` without `-p`,
/// `checkout-index --prefix`, `bugreport` and `diagnose -o`, `index-pack
/// -o`, `mailsplit -o`, fast-export and fast-import marks, and the file a
/// `config --file` write changes. An option the table does not know, where
/// its value could be the output, refuses.
#[allow(clippy::too_many_lines)] // One reading per Git verb that writes a named file.
pub(super) fn output_paths(verb: &str, rest: &[String]) -> Result<Vec<String>, String> {
    let mut paths: Vec<String> = parse_options(rest, &catalog::DIFF_OUTPUT_OPTIONS)
        .values_of('\0', "--output")
        .into_iter()
        .map(str::to_string)
        .collect();
    let read = |spec: &OptionSpec, stop: bool, outputs: &[(char, &str)]| {
        let parsed = checked_mode(rest, spec, stop)?;
        let mut found = Vec::new();
        for (short, long) in outputs {
            found.extend(
                parsed
                    .values_of(*short, long)
                    .into_iter()
                    .map(str::to_string),
            );
            // An unknown short bundle could hold the output letter, and an
            // unknown long word could be an abbreviation Git accepts.
            if parsed.unread.iter().any(|word| {
                (!word.starts_with("--") && *short != '\0' && word.contains(*short))
                    || (word.starts_with("--")
                        && !long.is_empty()
                        && long.starts_with(word.split('=').next().unwrap_or(word)))
            }) {
                return Err(format!(
                    "an option the guard does not read could name the `git {verb}` output"
                ));
            }
        }
        Ok::<_, String>((parsed, found))
    };
    match verb {
        "archive" => paths.extend(read(&catalog::ARCHIVE_OPTIONS, false, &[('o', "--output")])?.1),
        "format-patch" => paths.extend(
            read(
                &catalog::FORMAT_PATCH_OPTIONS,
                false,
                &[('o', "--output-directory"), ('\0', "--output")],
            )?
            .1,
        ),
        "bugreport" | "diagnose" => paths.extend(
            read(
                &catalog::REPORT_OPTIONS,
                false,
                &[('o', "--output-directory")],
            )?
            .1,
        ),
        "index-pack" => paths.extend(read(&catalog::INDEX_PACK_OPTIONS, false, &[('o', "")])?.1),
        "mailsplit" => paths.extend(read(&catalog::MAILSPLIT_OPTIONS, false, &[('o', "")])?.1),
        "checkout-index" => {
            let (parsed, prefix) = read(
                &catalog::CHECKOUT_INDEX_OPTIONS,
                false,
                &[('\0', "--prefix")],
            )?;
            paths.extend(prefix);
            // The files it checks out are written in the work tree
            // (TSK-1002 Fable review 15); `-a` writes every one.
            paths.extend(parsed.operands.iter().map(|word| (*word).to_string()));
            if parsed.has_short(&['a']) || parsed.has_long("--all") {
                paths.push(".".to_string());
            }
        }
        // The work-tree files a checkout or restore of paths overwrites,
        // and the directory `git apply --directory` writes under (TSK-1002
        // Fable review 15). `git worktree remove` without `--force` removes
        // only a clean worktree, whose files are all committed: it is the
        // sanctioned cleanup after a landing and stays allowed.
        "checkout" | "restore" => paths.extend(restored_paths(verb, rest)?),
        "apply" => {
            let mut words = rest.iter();
            while let Some(word) = words.next() {
                if word == "--directory" {
                    paths.extend(words.next().cloned());
                } else if let Some(directory) = word.strip_prefix("--directory=") {
                    paths.push(directory.to_string());
                }
            }
        }
        "fast-export" | "fast-import" => {
            paths.extend(read(&catalog::MARKS_OPTIONS, false, &[('\0', "--export-marks")])?.1);
        }
        "config" if !super::config_only_reads(rest) => {
            paths.extend(read(&super::GIT_CONFIG_OPTIONS, false, &[('f', "--file")])?.1);
        }
        "merge-file" => {
            // Git reads merge-file options anywhere before `--`; the first
            // operand is the file it rewrites.
            let (parsed, _) = read(&catalog::MERGE_FILE_OPTIONS, false, &[])?;
            if !parsed.unread.is_empty() {
                return Err(
                    "an option the guard does not read could name the `git merge-file` output"
                        .into(),
                );
            }
            if !(parsed.has_short(&['p']) || parsed.has_long("--stdout")) {
                paths.extend(parsed.operands.first().map(|word| (*word).to_string()));
            }
        }
        "bundle" if rest.first().is_some_and(|word| word == "create") => {
            let parsed = checked_mode(&rest[1..], &catalog::BUNDLE_CREATE_OPTIONS, true)?;
            if !parsed.unread.is_empty() {
                return Err(
                    "an option the guard does not read could name the `git bundle` output".into(),
                );
            }
            paths.extend(
                parsed
                    .operands
                    .first()
                    .filter(|file| **file != "-")
                    .map(|file| (*file).to_string()),
            );
        }
        _ => {}
    }
    Ok(paths)
}

/// The paths `git checkout` or `git restore` writes in the work tree: every
/// operand after `--`, and every operand before it that is not an option
/// value (a branch name too, judged as a path, which names no enforcement
/// file). A restore of the index alone (`--staged` without `--worktree`)
/// writes none. Paths read from a file cannot be established.
fn restored_paths(verb: &str, rest: &[String]) -> Result<Vec<String>, String> {
    let valued: &[&str] = if verb == "checkout" {
        &["-b", "-B", "--orphan", "--conflict"]
    } else {
        &["-s", "--source", "--conflict"]
    };
    let mut paths = Vec::new();
    let mut staged = false;
    let mut worktree = false;
    let mut branch = false;
    let mut separated = false;
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            // Before `--`, `git checkout` takes only the tree-ish it
            // restores from.
            if verb == "checkout" {
                paths.clear();
            }
            paths.extend(words.by_ref().cloned());
            separated = true;
            break;
        }
        if word.starts_with("--pathspec-from-file") {
            return Err(format!(
                "cannot establish the paths `git {verb}` writes from a file"
            ));
        }
        if valued.contains(&word.as_str()) {
            branch |= verb == "checkout" && word != "--conflict";
            words.next();
            continue;
        }
        if let Some(letters) = word.strip_prefix('-') {
            match word.as_str() {
                "--staged" => staged = true,
                "--worktree" => worktree = true,
                _ if verb == "restore" && !letters.starts_with('-') => {
                    // A short bundle ends at `s`, whose value follows.
                    let letters = letters.split('s').next().unwrap_or("");
                    staged |= letters.contains('S');
                    worktree |= letters.contains('W');
                }
                _ => {}
            }
            continue;
        }
        paths.push(word.clone());
    }
    if verb == "restore" && staged && !worktree {
        return Ok(Vec::new());
    }
    if verb == "checkout" {
        // `git checkout -b`, `-B` and `--orphan` switch branches and take
        // no paths (Git refuses both at once).
        if branch {
            return Ok(Vec::new());
        }
        // Without `--`, Git reads a first operand that names a commit as
        // the commit to check out. One with a revision suffix (`HEAD~3`,
        // `main^`) is that commit: as a pathspec it could match only a
        // tracked file literally so named, never an enforcement file.
        if !separated && paths.first().is_some_and(|word| revision_suffixed(word)) {
            paths.remove(0);
        }
    }
    Ok(paths)
}

/// A word spelled as a ref name followed by `~N` or `^N` suffixes.
fn revision_suffixed(word: &str) -> bool {
    let Some(at) = word.find(['~', '^']) else {
        return false;
    };
    let (base, suffix) = word.split_at(at);
    base.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
        && !base.contains("..")
        && base
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_./-".contains(ch))
        && suffix
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '~' || ch == '^')
}

pub(super) fn command_options(verb: &str) -> bool {
    option_spec(verb).is_some() || matches!(verb, "for-each-repo" | "submodule--helper")
}

// Git grep stops parsing options at its first non-option or `--`.
// After that boundary, xargs cannot append a pager option from stdin.
pub(super) fn closed_grep_options(rest: &[String]) -> bool {
    checked_mode(rest, &catalog::GREP_OPTIONS, true)
        .is_ok_and(|parsed| parsed.ended || !parsed.operands.is_empty())
}

fn grep_syntax(rest: &[String], segment: &str) -> Result<(), String> {
    let parsed = checked_mode(rest, &catalog::GREP_OPTIONS, true)?;
    for word in super::argv_words(segment) {
        if !word.expands && !word.pattern {
            continue;
        }
        if revision_operand(&word.text) && parsed.operands.contains(&word.text.as_str()) {
            continue;
        }
        if parsed
            .operands
            .first()
            .is_some_and(|first| *first == word.text)
            && !parsed.ended
        {
            let home = (word.text.starts_with("$HOME/") || word.text.starts_with("${HOME}/"))
                && word.literal()
                && super::line_home::home().is_some_and(|p| p.is_absolute());
            let prefix = word.text.split(['$', '`']).next().unwrap_or("");
            if !word.pattern
                && ((!prefix.is_empty() && !prefix.starts_with(['-', '~', '='])) || home)
            {
                continue;
            }
        } else if parsed.operands.contains(&word.text.as_str())
            || (parsed.values_of('e', "").contains(&word.text.as_str()) && word.literal())
        {
            continue;
        }
        return Err(
            "a Git grep option or its first operand requires unclassified expansion".into(),
        );
    }
    Ok(())
}

fn known_program(verb: &str) -> bool {
    GIT_BUILTINS.contains(&verb) || command_options(verb) || matches!(verb, "submodule" | "bisect")
}

fn config_environment(
    env: &crate::security::git_config::Environment,
) -> crate::security::git_config::Environment {
    env.iter()
        .filter(|(name, _)| !catalog::environment(name) && !program_environment(name))
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
    crate::security::git_config::inherited_location_is_own(
        cwd,
        &settings,
        &config_environment(env),
        &crate::security::git_config::process_location()?,
    )?;
    let config = crate::security::git_config::read(cwd, &settings, &config_environment(env))?;
    transport(args, &config)?;
    repository_owned_hooks_path(&config)?;
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
    let paging = catalog::pages_by_default(verb, rest)
        || pagination == Some(true)
        || config
            .get(&selected_pager)?
            .is_some_and(|value| crate::security::git::config_boolean(value) != Some(false));
    for key in config.keys()? {
        if key == "core.editor" && !uses_editor(verb, rest) {
            continue;
        }
        let pager = key == "core.pager" || key.starts_with("pager.");
        if pager
            && (pagination == Some(false)
                || !paging
                || (key.starts_with("pager.") && key != selected_pager))
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
            let active = if pager {
                pagination != Some(false) && paging
            } else {
                catalog::active(verbs, verb)
            };
            if !active {
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
        "add" | "reset" | "checkout" | "restore" | "stash" | "replace" => {
            let spec = if matches!(verb, "checkout" | "restore") {
                &catalog::CHECKOUT_EDITOR_OPTIONS
            } else {
                &catalog::EDITOR_MODE_OPTIONS
            };
            let options = parse_options(rest, spec);
            !options.unread.is_empty()
                || options.has_short(&['p', 'e', 'i'])
                || ["--patch", "--edit", "--interactive"]
                    .iter()
                    .any(|name| options.has_long(name))
        }
        "send-email" => {
            let options = parse_options(rest, &catalog::EMAIL_OPTIONS);
            !options.unread.is_empty()
                || options.has_long("--annotate")
                || options.has_long("--compose")
        }
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

/// Refuse a `core.hooksPath` that does not come from the repository's own
/// configuration (TSK-1130). Git runs hooks from the directory the
/// effective value names, and the enforcement plane installs its hooks in
/// the repository. A value from the command scope (`-c`, `GIT_CONFIG_COUNT`,
/// `GIT_CONFIG_PARAMETERS`) replaces them for the call. A value from the
/// global or system scope can replace them too, and the process
/// environment selects those files (`GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`,
/// `HOME`, `XDG_CONFIG_HOME`, and any include in them), so Git's scope label
/// does not show who chose the file. Only an absent value (Git's default
/// hooks directory) or a value from the `local` or `worktree` scope, files
/// inside the repository, passes. The directory it names is not read: a
/// path the caller chose can gain a hook between this check and the run.
fn repository_owned_hooks_path(
    config: &crate::security::git_config::EffectiveConfig,
) -> Result<(), String> {
    // Git lists values in precedence order, so the last one is in force.
    match config.get_all("core.hooksPath").last() {
        None | Some(("local" | "worktree", _, _)) => Ok(()),
        Some((scope, _, _)) => Err(format!(
            "core.hooksPath comes from the {scope} configuration, which the environment or the command can select and which replaces the repository hooks; set it in the repository's own configuration or unset it"
        )),
    }
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
        if external_program(verb, global, &starting_directory(&args, cwd)?, env)?.is_some() {
            return Ok(args);
        }
        let config = crate::security::git_config::read(cwd, global, &config_environment(env))?;
        let Some(value) = config.get(&format!("alias.{verb}"))? else {
            return Err(format!(
                "cannot resolve external Git program `git-{}` on PATH or git --exec-path; install that program or use a builtin or configured alias",
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

fn external_body(
    args: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Option<Body>, String> {
    let Some((verb, rest)) = git_subcommand(args) else {
        return Ok(None);
    };
    if known_program(verb) {
        return Ok(None);
    }
    let global = globals(args)?;
    let at = starting_directory(args, cwd)?;
    let path = external_program(verb, global, &at, env)?
        .ok_or("external Git program disappeared during resolution")?;
    let name = crate::git::GitName::from_os_str(path.as_os_str());
    let path = name
        .rule_text()
        .map_err(|_| "external Git program path is not UTF-8")?;
    let words: Vec<_> = std::iter::once(path.to_string())
        .chain(rest.iter().cloned())
        .collect();
    Ok(Some(Body {
        command: quote_words(&words),
        directory: at,
    }))
}

fn starting_directory(args: &[String], cwd: &Path) -> Result<PathBuf, String> {
    let parsed = checked(globals(args)?, &GLOBAL_OPTIONS)?;
    Ok(parsed
        .values_of('C', "-C")
        .iter()
        .fold(cwd.to_path_buf(), |at, change| {
            integrity_shell_path(change, &at)
        }))
}

fn program_environment(name: &str) -> bool {
    matches!(name, "PATH" | "GIT_EXEC_PATH")
}

// Resolve the same external spelling Git dispatches. Merely locating a
// program does not execute or read its script; it gets ordinary argv checks.
fn external_program(
    verb: &str,
    global: &[String],
    cwd: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Option<PathBuf>, String> {
    if verb.is_empty()
        || verb.starts_with('-')
        || verb.contains(['/', '\\'])
        || unresolved_word(verb)
    {
        return Err("an external Git command needs a literal program name".into());
    }
    let program = format!("git-{verb}");
    let exec = global
        .iter()
        .rev()
        .find_map(|word| word.strip_prefix("--exec-path="))
        .map(std::ffi::OsString::from)
        .or_else(|| env.get("GIT_EXEC_PATH").map(std::ffi::OsString::from))
        .or_else(|| std::env::var_os("GIT_EXEC_PATH"));
    let exec = if let Some(exec) = exec {
        exec
    } else {
        let mut command = crate::git::command();
        command.current_dir(cwd).arg("--exec-path");
        let bytes = crate::security::git_config::bounded_output(
            &mut command,
            std::time::Instant::now() + std::time::Duration::from_secs(3),
        )?;
        let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
        crate::git::GitName::from_bytes(bytes)
            .os_path()
            .map_err(|error| error.to_string())?
            .into_os_string()
    };
    let path = env
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let paths =
        std::env::join_paths(std::iter::once(cwd.join(exec)).chain(std::env::split_paths(&path)))
            .map_err(|_| "cannot read Git program search path")?;
    Ok(which::which_in(program, Some(paths), cwd).ok())
}

// PATH extensions use the inherited value when this line has not replaced
// it. Substitute only inside assignment tokens, never back into shell syntax.
fn note_literals(literals: &mut Literals, tokens: &mut [String]) -> Result<(), String> {
    let mut inherited = Vec::new();
    if tokens.iter().any(|word| {
        word.split_once('=')
            .is_some_and(|(name, value)| program_environment(name) && expands(value))
    }) {
        for name in ["HOME", "PATH", "GIT_EXEC_PATH"] {
            if literals.0.contains_key(name) {
                continue;
            }
            match std::env::var(name) {
                Ok(value) => inherited.push((name.to_string(), value)),
                Err(std::env::VarError::NotPresent) => {}
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err(format!("Git program environment {name} is not UTF-8"));
                }
            }
        }
    }
    for word in tokens.iter_mut() {
        if let Some((name, value)) = word.split_once('=') {
            if program_environment(name) {
                *word = format!(
                    "{name}={}",
                    crate::security::startup::substitute_with(value, &inherited)
                );
            }
        }
    }
    literals.note(tokens);
    Ok(())
}

fn environment(
    tokens: &[String],
    literals: &Literals,
) -> Result<crate::security::git_config::Environment, String> {
    let mut env = BTreeMap::new();
    for (name, value) in &literals.0 {
        if GIT_LOCATION_VARS.contains(&name.as_str())
            || program_environment(name)
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
            || program_environment(&name)
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
    if parsed.has_long("--bare") {
        return Err("a bare Git runner has no proven work tree".into());
    }
    let at = starting_directory(args, cwd)?;
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
            starting_directory(args, cwd)?,
            root.to_path_buf(),
            integrity_shell_path(temporary, root).join("t"),
        ]);
    }
    if !matches!(verb, "submodule" | "submodule--helper") {
        let mut dirs = vec![
            starting_directory(args, cwd)?,
            repo.workdir().ok_or("no runner work tree")?.to_path_buf(),
        ];
        dirs.sort();
        dirs.dedup();
        return Ok(dirs);
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

/// A path an earlier command on the line writes (TSK-379 security round 8).
/// `places` marks a destination the command fills with entries it does not
/// name: a recursive copy, move or sync, a link, an archive extraction or a
/// clone. `None` is a placement the guard cannot resolve.
struct Landing {
    /// A worktree creation inherits this repository's configuration.
    worktree_source: Option<PathBuf>,
    path: Option<PathBuf>,
    places: bool,
    /// Repository metadata a Git command creates (a new worktree's
    /// administrative directory): it changes only what a command reading
    /// that Git directory sees.
    metadata: bool,
}

fn land(target: &str, places: bool, dirs: &[PathBuf], landings: &mut Vec<Landing>) {
    if target.starts_with('-') && target != "-" {
        return;
    }
    if expands(target) {
        if places {
            landings.push(Landing {
                worktree_source: None,
                path: None,
                places,
                metadata: false,
            });
        }
        return;
    }
    for dir in dirs {
        landings.push(Landing {
            worktree_source: None,
            path: Some(integrity_shell_path(target, dir)),
            places,
            metadata: false,
        });
    }
}

fn named_git(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
}

/// What a non-Git command writes, for the runner-state check: its
/// redirections and operands, and where it places entries it does not name.
fn note_landings(
    segment: &str,
    program: &str,
    args: &[String],
    dirs: &[PathBuf],
    landings: &mut Vec<Landing>,
) {
    if !may_change_discard_state(segment) {
        return;
    }
    for target in &redirect_writes(segment).targets {
        land(target, false, dirs, landings);
    }
    let name = basename(program);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    // Tests and metadata readers name paths without writing them.
    if !matches!(
        name,
        "test" | "[" | "stat" | "wc" | "head" | "tail" | "file" | "du" | "readlink" | "realpath"
    ) {
        for arg in args {
            land(arg, false, dirs, landings);
        }
    }
    let Some(effects) = crate::security::unresolved::writer_effects(name, args, &[]) else {
        // Archivers the writer table does not list extract into the
        // directory they run in.
        if matches!(
            name,
            "cpio" | "bsdcpio" | "pax" | "7z" | "7za" | "7zz" | "7zr" | "unar" | "ar" | "jar"
        ) {
            land(".", true, dirs, landings);
            for arg in args {
                land(arg, true, dirs, landings);
            }
        }
        return;
    };
    if effects.read.unknown.is_some() {
        for arg in args {
            land(arg, true, dirs, landings);
        }
    }
    for path in &effects.writes {
        land(path, false, dirs, landings);
    }
    if let Some(copy) = &effects.copy {
        if let Some(dest) = &copy.dest {
            // A plain copy or move of named regular files lands each file
            // under its own name; nothing else can appear at the destination.
            let recursive = copy.flags.iter().any(|flag| {
                matches!(
                    flag.as_str(),
                    "-R" | "-r" | "-a" | "--recursive" | "--archive"
                ) || (flag.starts_with('-')
                    && !flag.starts_with("--")
                    && flag.contains(['R', 'r', 'a']))
            });
            let files = matches!(name, "cp" | "mv")
                && !recursive
                && !copy.sources.is_empty()
                && copy.sources.iter().all(|source| {
                    !expands(source)
                        && dirs.iter().all(|dir| {
                            let path = integrity_shell_path(source, dir);
                            !named_git(&path)
                                && std::fs::symlink_metadata(&path)
                                    .is_ok_and(|meta| meta.file_type().is_file())
                        })
                });
            land(
                if dest.is_empty() { "." } else { dest },
                !files,
                dirs,
                landings,
            );
        }
    }
    if let Some(places) = &effects.places {
        if places.dirs.is_empty() {
            land(".", true, dirs, landings);
        }
        let mut joined = String::new();
        for dir in &places.dirs {
            land(dir, true, dirs, landings);
            joined = if joined.is_empty() || dir.starts_with(['/', '~']) {
                dir.clone()
            } else {
                format!("{}/{dir}", joined.trim_end_matches('/'))
            };
            land(&joined, true, dirs, landings);
        }
    }
}

/// What a Git command writes, for the runner-state check: its redirections
/// and output paths, a clone's checkout, and the Git directory an init
/// creates from a template.
fn note_git_landings(
    segment: &str,
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
    landings: &mut Vec<Landing>,
) -> Result<(), String> {
    let dirs = [at.to_path_buf()];
    for target in &redirect_writes(segment).targets {
        land(target, false, &dirs, landings);
    }
    let Some((verb, rest)) = git_subcommand(args) else {
        return Ok(());
    };
    let start = [starting_directory(args, at)?];
    for path in output_paths(verb, rest)? {
        land(&path, false, &start, landings);
    }
    match verb {
        "worktree" if rest.first().is_some_and(|word| word == "add") => {
            let parsed = checked_mode(&rest[1..], &catalog::WORKTREE_ADD_OPTIONS, true)?;
            match parsed.operands.first() {
                Some(path) if parsed.unread.is_empty() => {
                    land(path, true, &start, landings);
                    if let Some(landing) = landings.last_mut() {
                        landing.worktree_source = Some(start[0].clone());
                    }
                }
                _ => {
                    for word in &rest[1..] {
                        land(word, true, &start, landings);
                    }
                }
            }
            // The new worktree's administrative directory in the common
            // Git directory is created too.
            if let Some(repo) = super::super::repo::open(&start[0])? {
                landings.push(Landing {
                    worktree_source: None,
                    path: Some(repo.commondir().join("worktrees")),
                    places: true,
                    metadata: true,
                });
            }
        }
        "clone" => {
            let parsed = checked(rest, &catalog::CLONE_OPTIONS)?;
            let directory = parsed
                .operands
                .get(1)
                .map(|dir| (*dir).to_string())
                .or_else(|| parsed.operands.first().and_then(|url| humanish(url)));
            land(directory.as_deref().unwrap_or("."), true, &start, landings);
        }
        "init" => {
            let template = rest
                .iter()
                .any(|word| word == "--template" || word.starts_with("--template="))
                || env.contains_key("GIT_TEMPLATE_DIR")
                || std::env::var_os("GIT_TEMPLATE_DIR").is_some()
                || crate::security::git_config::read(at, globals(args)?, &config_environment(env))?
                    .get("init.templatedir")?
                    .is_some();
            if template {
                for path in super::git_init_paths(rest) {
                    land(
                        &format!("{}/.git", path.trim_end_matches('/')),
                        false,
                        &start,
                        landings,
                    );
                    land(&path, false, &start, landings);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// The directory `git clone URL` creates: the URL's last path part without
/// a trailing `.git` or `/.git`.
fn humanish(url: &str) -> Option<String> {
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix("/.git").unwrap_or(url);
    let name = url.rsplit(['/', ':']).next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (!name.is_empty() && name != "." && name != ".." && !expands(name)).then(|| name.to_string())
}

/// The first `.git` component of `path`, as the directory that holds it.
fn git_holder(path: &Path) -> Option<PathBuf> {
    let mut holder = PathBuf::new();
    for component in path.components() {
        if component.as_os_str().eq_ignore_ascii_case(".git") {
            return Some(holder);
        }
        holder.push(component);
    }
    None
}

/// Refuse when an earlier write on the line could create, replace or move
/// repository state the next Git command discovers or reads: a `.git` at
/// or above its starting directory or work tree, an entry placed at or
/// above them, a write in or around its explicit Git directory, or one at
/// a configuration file its environment names. The guard read that state
/// before the line ran, so the read would be stale.
fn fresh_runner(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
    landings: &[Landing],
) -> Result<(), String> {
    const STALE: &str = "an earlier command may rewrite Git runner configuration or saved instructions; run the commands separately";
    if landings.is_empty() {
        return Ok(());
    }
    if landings.iter().any(|landing| landing.path.is_none()) {
        return Err(STALE.into());
    }
    let start = starting_directory(args, at)?;
    let parsed = checked(globals(args)?, &GLOBAL_OPTIONS)?;
    let mut trees = vec![start.clone()];
    let mut git_dirs = Vec::new();
    let mut files = Vec::new();
    for value in parsed.values_of('\0', "--work-tree") {
        trees.push(integrity_shell_path(value, &start));
    }
    for value in parsed.values_of('\0', "--git-dir") {
        git_dirs.push(integrity_shell_path(value, &start));
    }
    let inherited = inherited_locations(|name| std::env::var(name))?;
    let variables = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .chain(inherited.iter().map(|(name, value)| (*name, value.clone())));
    for (name, value) in variables {
        let path = integrity_shell_path(&value, &start);
        match name {
            "GIT_DIR" | "GIT_COMMON_DIR" => git_dirs.push(path),
            "GIT_WORK_TREE" => trees.push(path),
            "GIT_CONFIG_GLOBAL" | "GIT_CONFIG_SYSTEM" | "GIT_CONFIG" => {
                // Read from either side of a `-C` change.
                files.push(integrity_shell_path(&value, at));
                files.push(path);
            }
            "HOME" => {
                files.push(path.join(".gitconfig"));
                files.push(path.join(".config/git/config"));
            }
            "XDG_CONFIG_HOME" => files.push(path.join("git/config")),
            _ => {}
        }
    }
    if let Some(home) = super::line_home::home() {
        files.push(home.join(".gitconfig"));
        files.push(home.join(".config/git/config"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        files.push(PathBuf::from(xdg).join("git/config"));
    }
    let normal = |path: &Path| -> Result<PathBuf, String> {
        super::super::edit_guard::normalized(path, true).map_err(|error| error.to_string())
    };
    let trees = trees
        .iter()
        .map(|p| normal(p))
        .collect::<Result<Vec<_>, _>>()?;
    let git_dirs = git_dirs
        .iter()
        .map(|p| normal(p))
        .collect::<Result<Vec<_>, _>>()?;
    let files = files
        .iter()
        .map(|p| normal(p))
        .collect::<Result<Vec<_>, _>>()?;
    for landing in landings {
        let Some(path) = &landing.path else {
            continue;
        };
        let path = normal(path)?;
        if landing.metadata {
            if git_dirs.iter().any(|dir| dir.starts_with(&path)) {
                return Err(STALE.into());
            }
            continue;
        }
        let holder = git_holder(&path);
        let stale = trees.iter().any(|tree| {
            (landing.places && tree.starts_with(&path))
                || holder
                    .as_ref()
                    .is_some_and(|holder| tree.starts_with(holder))
        }) || git_dirs
            .iter()
            .any(|dir| path.starts_with(dir) || (landing.places && dir.starts_with(&path)))
            || files
                .iter()
                .any(|file| *file == path || (landing.places && file.starts_with(&path)));
        if stale {
            return Err(STALE.into());
        }
    }
    Ok(())
}

/// `git worktree add` runs callbacks in the checkout it creates, with that
/// worktree's configuration. A conditional include can apply there although
/// it does not apply where the guard reads. Which ones do depends on the
/// administrative directory Git creates, its name, its spelling through any
/// alias and the new branch, so the guard does not decide it (TSK-379 review
/// rounds 9 to 13): every conditional include, of any kind, is read as a
/// file by the path Git reads it by, and a setting Git can run during the
/// add, or a hooks path, refuses. Includes they name are read the same way.
fn new_worktree_configuration(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<(), String> {
    let Some(("worktree", rest)) = git_subcommand(args) else {
        return Ok(());
    };
    if rest.first().is_none_or(|word| word != "add") {
        return Ok(());
    }
    future_worktree_configuration(args, at, env, "worktree")
}

fn future_worktree_configuration(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
    verb: &str,
) -> Result<(), String> {
    let environment = config_environment(env);
    let config = crate::security::git_config::read(at, globals(args)?, &environment)?;
    // Each file is read by the spelling Git reads it by, so a relative
    // include inside a linked file resolves as Git resolves it (TSK-379
    // review round 12); the canonical path only identifies a file once.
    let mut pending: Vec<(PathBuf, PathBuf)> = config
        .includes
        .iter()
        .filter(|include| !include.missing)
        .filter(|include| include.condition.is_some())
        .map(|include| (include.logical.clone(), include.path.clone()))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut read = 0;
    while let Some((path, canonical)) = pending.pop() {
        if !seen.insert((path.clone(), canonical)) {
            continue;
        }
        read += 1;
        if read > 128 {
            return Err("more than 128 conditional configuration files to read".into());
        }
        let file = crate::security::git_config::read_file(&path, &environment)?;
        for key in file.keys()? {
            let runs = key.eq_ignore_ascii_case("core.hookspath")
                || catalog::SOURCES.iter().any(|source| {
                    matches!(source,
                        catalog::Source::Config(pattern, verbs) | catalog::Source::ConfigList(pattern, verbs)
                            if catalog::config_matches(key, pattern) && catalog::active(verbs, verb))
                });
            if !runs {
                continue;
            }
            let hooks = key.eq_ignore_ascii_case("core.hookspath");
            for value in config_values(&file, key)? {
                // An empty command runs nothing, but an empty hooks path is
                // still a hooks location Git uses (TSK-379 review round 13).
                if !hooks
                    && (value.is_empty()
                        || crate::security::git::config_kind(key, value)
                            == crate::security::git::ConfigKind::Safe)
                {
                    continue;
                }
                return Err(format!(
                    "the conditional include {} can apply to the new worktree and sets `{key}`, which the guard cannot judge before the worktree exists; add the worktree without that configuration or ask the operator",
                    path.display()
                ));
            }
        }
        pending.extend(
            file.includes
                .iter()
                .filter(|include| !include.missing)
                .map(|include| (include.logical.clone(), include.path.clone())),
        );
    }
    Ok(())
}

/// What a non-Git segment writes, from each directory it can run in.
fn note_segment(
    filled: &str,
    tokens: &[String],
    program: &str,
    args: &[String],
    locations: &[PathBuf],
    earlier: &mut Vec<PathBuf>,
    landings: &mut Vec<Landing>,
) {
    // `export NAME=value` and the other declaration builtins assign shell
    // variables; their operands name no file, so only their redirections
    // can write. Read as paths, `R=C:/Users/RUNNER~1/...` named a missing
    // short-name component the Windows mapper must refuse (hosted run
    // 40602309629).
    let words: Vec<String> = std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect();
    let declaring = matches!(
        super::builtin_words(&words).first().map(String::as_str),
        Some("export" | "declare" | "typeset" | "local" | "readonly")
    );
    let args: &[String] = if declaring { &[] } else { args };
    note_writes(filled, args, locations, earlier);
    let (changes, _) = launcher_effects(tokens);
    let dirs: Vec<PathBuf> = locations
        .iter()
        .map(|start| {
            changes.iter().fold(start.clone(), |at, change| {
                integrity_shell_path(change, &at)
            })
        })
        .collect();
    note_landings(filled, program, args, &dirs, landings);
}

/// Read the command bodies and callbacks a runner starts, each from the
/// directories it runs in.
fn descend(
    runner: &Runner<'_>,
    args: &[String],
    [commands, configured]: [&Vec<String>; 2],
    bodies: &mut Vec<Body>,
) -> Result<(), String> {
    let child = Inputs::child(args, runner.env)?;
    for (commands, callbacks) in [(commands, false), (configured, true)] {
        if commands.is_empty() {
            continue;
        }
        let dirs = if callbacks {
            callback_directories(args, runner.at, runner.env)?
        } else {
            directories(args, runner.at, runner.env)?
        };
        for dir in dirs {
            for command in commands {
                collect(command, &dir, runner.depth + 1, &child, bodies)?;
                bodies.push(Body {
                    command: command.clone(),
                    directory: dir.clone(),
                });
            }
        }
    }
    Ok(())
}

/// One Git segment at one starting directory.
struct Runner<'a> {
    segment: &'a str,
    at: &'a Path,
    env: &'a crate::security::git_config::Environment,
    depth: usize,
}

/// Status in a checkout created earlier on the line can read the source
/// configuration. Do not infer a future callback's cwd or conditional config.
fn future_status(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
    landings: &[Landing],
) -> Result<Option<Vec<String>>, String> {
    if git_subcommand(args).is_none_or(|(verb, _)| verb != "status") {
        return Ok(None);
    }
    let target = starting_directory(args, at)?;
    let target = crate::security::outward::lexical_join(at, &target);
    let source = landings.iter().find_map(|landing| {
        (landing
            .path
            .as_ref()
            .map(|path| crate::security::outward::lexical_join(at, path))
            .as_ref()
            == Some(&target))
        .then_some(landing.worktree_source.as_ref())
        .flatten()
    });
    let Some(source) = source else {
        return Ok(None);
    };
    let source_name = crate::git::GitName::from_os_str(source.as_os_str());
    let source_text = source_name.rule_text().map_err(|error| error.to_string())?;
    let mut relocated = vec!["-C".to_string(), source_text.to_owned()];
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "-C" {
            words.next();
        } else if word.starts_with("--git-dir") || word.starts_with("--work-tree") {
            return Err(
                "cannot verify a new worktree status with an explicit Git directory".into(),
            );
        } else {
            relocated.push(word.clone());
        }
    }
    fresh_runner(&relocated, at, env, landings)?;
    future_worktree_configuration(&relocated, at, env, "status")?;
    if !configured_commands(&relocated, at, env, &[])?.is_empty() {
        return Err(
            "cannot verify status callbacks before the new worktree exists; run status separately"
                .into(),
        );
    }
    Ok(Some(relocated))
}

impl Runner<'_> {
    /// Check the runner state against earlier writes, note this segment's
    /// own writes, and resolve its aliases.
    fn resolve(
        &self,
        args: &[String],
        landings: &[Landing],
        pending: &mut Vec<Landing>,
    ) -> Result<Vec<String>, String> {
        if let Some(status) = future_status(args, self.at, self.env, landings)? {
            return Ok(status);
        }
        fresh_runner(args, self.at, self.env, landings)?;
        new_worktree_configuration(args, self.at, self.env)?;
        note_git_landings(self.segment, args, self.at, self.env, pending)?;
        let expanded = aliases(args.to_vec(), self.at, self.env)?;
        // An alias can expand to a creation or an output path; judge the
        // expansion as the written command is judged. A nested body's
        // expansion is itself judged as a body.
        if self.depth == 0 && expanded != args {
            if let Some(found) = super::git_command_write_violation(
                "git",
                &expanded,
                super::PolicyLevel::Block,
                self.at,
                self.at,
                self.segment,
            ) {
                return Err(found.message);
            }
            note_git_landings(self.segment, &expanded, self.at, self.env, pending)?;
        }
        Ok(expanded)
    }
}

fn note_writes(segment: &str, args: &[String], dirs: &[PathBuf], earlier: &mut Vec<PathBuf>) {
    if !may_change_discard_state(segment) {
        return;
    }
    let redirects = redirect_writes(segment);
    // redirect_writes already removes shell quoting. These are filesystem
    // paths, not case-insensitive enforcement names or whitespace-delimited
    // words: folding either loses writes on case-sensitive filesystems.
    let targets = if redirects.targets.is_empty() {
        args
    } else {
        &redirects.targets
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

// Git does not expand `~` in its own location options; the shell already
// did. Hand the configuration reader the directory the shell gives Git.
fn home_globals(mut args: Vec<String>) -> Vec<String> {
    if !super::line_home::tilde_resolves() {
        return args;
    }
    let Some((_, rest)) = git_subcommand(&args) else {
        return args;
    };
    let end = args.len() - rest.len() - 1;
    for at in 1..end {
        if matches!(args[at - 1].as_str(), "-C" | "--git-dir" | "--work-tree")
            && home_operand(&args[at])
        {
            if let Some(path) = integrity_shell_path(&args[at], Path::new("/")).to_str() {
                args[at] = path.to_string();
            }
        }
    }
    args
}

// A known directory with no enclosing repository is distinct from unreadable
// discovery or an explicit unresolved Git location. Global callbacks still
// get checked from that directory; their mere presence does not require a
// checkout (for example an installed LFS filter during a config write).
fn callbacks_without_checkout(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<bool, String> {
    let parsed = checked(globals(args)?, &GLOBAL_OPTIONS)?;
    if ["--bare", "--git-dir", "--work-tree", "--namespace"]
        .iter()
        .any(|name| parsed.has_long(name))
        || env
            .keys()
            .any(|name| GIT_LOCATION_VARS.contains(&name.as_str()))
        || !inherited_locations(|name| std::env::var(name))?.is_empty()
    {
        return Ok(false);
    }
    Ok(super::super::repo::open(at)?.is_none())
}

fn callback_directories(
    args: &[String],
    at: &Path,
    env: &crate::security::git_config::Environment,
) -> Result<Vec<PathBuf>, String> {
    let start = starting_directory(args, at)?;
    if callbacks_without_checkout(args, &start, env)? {
        return Ok(vec![start]);
    }
    let repo = checkout(args, at, env)?;
    // Callbacks can run before setup, in the checkout, or from metadata.
    let mut dirs = vec![
        start,
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

fn segment_directories(
    command: &str,
    segments: &[String],
    cwd: &Path,
    fallback: &[PathBuf],
) -> Vec<Vec<PathBuf>> {
    let top = super::flat_top_level(command);
    let roles = top
        .as_ref()
        .and_then(|top| super::map_top_level(segments, top));
    let facts = super::LineFacts::read(segments, roles.as_deref(), command);
    let mut shell = super::ShellState::new(top.is_some());
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let tracked = if let Some(Some(join)) = roles.as_ref().map(|roles| roles[index]) {
                shell.begin(join);
                true
            } else {
                false
            };
            let mut tokens = super::shell_tokens(segment);
            strip_reserved_words(&mut tokens);
            let dirs = match facts.moves_for(&shell, tracked, &tokens).cwd {
                super::Cwd::Paths(paths) => paths
                    .iter()
                    .map(|path| integrity_shell_path(path, cwd))
                    .collect(),
                super::Cwd::Unknown(_) => fallback.to_vec(),
            };
            if tracked {
                if let Some(dir) = super::cd_target(&tokens) {
                    if tokens[0] == "cd" || super::plain_pushd(segment) {
                        shell.cd(&dir);
                    } else {
                        shell.observe(&tokens[0], &tokens[1..]);
                    }
                } else {
                    let words = super::command_argv(segment);
                    let launched = super::configuration_launch(&words);
                    shell.assign(
                        &tokens,
                        launched.map(|(program, _)| program),
                        words.len() != tokens.len(),
                    );
                    if let Some((program, args)) = launched {
                        shell.observe(program, args);
                    }
                }
            }
            dirs
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // One ordered walk of the line's segments shares the earlier-command state.
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
    let locations = segment_directories(command, &expanded.segments, cwd, &run.dirs);
    let mut literals = Literals::default();
    let mut changed_sources = false;
    let mut changed_aliases = false;
    // Whether an earlier command may have changed which checkouts a
    // `submodule foreach` or `for-each-repo` runs in.
    let mut changed_checkouts = false;
    let mut earlier = Vec::new();
    let mut landings = Vec::new();
    for (index, segment) in segments.into_iter().enumerate() {
        let filled = literals.fill(&segment);
        let (mut tokens, mut patterns) = command_argv_patterns(&filled);
        let before = tokens.len();
        strip_reserved_words(&mut tokens);
        patterns.drain(..before - tokens.len());
        note_literals(&mut literals, &mut tokens)?;
        let Some((program, args)) = strip_launchers(&tokens) else {
            continue;
        };
        let Some(args) = arguments(program, args) else {
            let (writes, lands) = (earlier.len(), landings.len());
            note_segment(
                &filled,
                &tokens,
                program,
                args,
                &locations[index],
                &mut earlier,
                &mut landings,
            );
            changed_checkouts |= earlier.len() != writes
                || landings.len() != lands
                || !keeps_checkouts(program, args);
            continue;
        };
        let args = home_globals(args);
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
        let mut pending = Vec::new();
        for start in &locations[index] {
            let (changes, uncertain) = launcher_effects(&tokens);
            let at = changes.iter().fold(start.clone(), |at, change| {
                integrity_shell_path(change, &at)
            });
            let runner = Runner {
                segment: &filled,
                at: &at,
                env: &env,
                depth,
            };
            let args = runner.resolve(&args, &landings, &mut pending)?;
            if depth > 0 && args != original {
                bodies.push(Body {
                    command: normalized_command(&args, &env),
                    directory: at.clone(),
                });
            }
            bodies.extend(external_body(&args, &at, &env)?);
            note_config_write(&args, &mut changed_sources, &mut changed_aliases);
            let mut commands = commands(&args)?;
            let configured = configured_commands(&args, &at, &env, &earlier)?;
            commands.extend(saved_commands(&args, &at, &env, &earlier)?);
            if option_spec(git_subcommand(&args).map_or("", |(verb, _)| verb)).is_some()
                || !commands.is_empty()
                || !configured.is_empty()
            {
                self::patterns(&args, &tokens, &patterns, &filled)?;
            }
            if commands.is_empty() && configured.is_empty() {
                continue;
            }
            if let Some(reason) = run.unknown.as_ref().or(uncertain.as_ref()) {
                return Err(reason.clone());
            }
            if changed_checkouts && !commands.is_empty() && runs_in_checkouts(&args) {
                return Err("an earlier command may change the checkouts a Git runner runs its command in; run the commands separately".into());
            }
            descend(&runner, &args, [&commands, &configured], bodies)?;
        }
        landings.extend(pending);
        changed_checkouts = true;
    }
    Ok(())
}

/// Whether a Git call runs its command in a set of checkouts it reads when
/// it starts: each submodule (`submodule foreach`) or each configured
/// repository (`for-each-repo`).
fn runs_in_checkouts(args: &[String]) -> bool {
    matches!(
        git_subcommand(args),
        Some(("submodule" | "submodule--helper" | "for-each-repo", _))
    )
}

/// Whether a command other than Git is known to leave every checkout as it
/// was: a directory change, an assignment, or a builtin that only prints or
/// tests. Its writes are counted separately.
fn keeps_checkouts(program: &str, args: &[String]) -> bool {
    let words: Vec<String> = std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect();
    matches!(
        super::builtin_words(&words).first().map(String::as_str),
        Some(
            "cd" | "pushd"
                | "popd"
                | "echo"
                | "printf"
                | "true"
                | "false"
                | ":"
                | "test"
                | "["
                | "export"
                | "declare"
                | "typeset"
                | "local"
                | "readonly"
                | "pwd"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(verb: &str, line: &str) -> Vec<String> {
        let rest: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        restored_paths(verb, &rest).unwrap()
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let init = crate::git::command()
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap();
        assert!(init.success());
        root
    }

    fn call_with(
        root: &Path,
        words: &[&str],
        environment: &[(&str, &str)],
    ) -> Result<Vec<String>, String> {
        let args: Vec<String> = words.iter().map(ToString::to_string).collect();
        let environment: crate::security::git_config::Environment = environment
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        configured_commands(&args, root, &environment, &[])
    }

    fn call(root: &Path, words: &[&str]) -> Result<Vec<String>, String> {
        call_with(root, words, &[])
    }

    /// A configuration file that sets the hooks path, outside the repository.
    fn hostile_file(dir: &Path, name: &str) -> String {
        let file = dir.join(name);
        std::fs::write(&file, "[core]\n\thooksPath = /elsewhere\n").unwrap();
        file.to_string_lossy().into_owned()
    }

    fn set_local_hooks_path(root: &Path) {
        let set = crate::git::command()
            .args(["config", "core.hooksPath", "/repository-owned"])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(set.success());
    }

    #[test]
    fn a_hooks_path_in_the_command_scope_refuses() {
        let root = repository();
        let refused = call(root.path(), &["-c", "core.hooksPath=/elsewhere", "status"])
            .expect_err("a command-scope hooks path replaces the repository hooks");
        assert!(refused.contains("command"), "{refused}");
    }

    #[test]
    fn a_hooks_path_chosen_by_the_environment_refuses_in_every_file_scope() {
        let root = repository();
        let outside = tempfile::tempdir().unwrap();
        let file = hostile_file(outside.path(), "hostile");
        // An explicit global or system file.
        assert!(call_with(root.path(), &["status"], &[("GIT_CONFIG_GLOBAL", &file)]).is_err());
        assert!(call_with(root.path(), &["status"], &[("GIT_CONFIG_SYSTEM", &file)]).is_err());
        // A home directory the caller picked.
        let home = outside.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[core]\n\thooksPath = /elsewhere\n",
        )
        .unwrap();
        let home_text = home.to_string_lossy().into_owned();
        assert!(call_with(root.path(), &["status"], &[("HOME", &home_text)]).is_err());
        // An XDG directory the caller picked, with no file in the home.
        let xdg = outside.path().join("xdg");
        std::fs::create_dir_all(xdg.join("git")).unwrap();
        std::fs::write(xdg.join("git/config"), "[core]\n\thooksPath = /elsewhere\n").unwrap();
        let xdg_text = xdg.to_string_lossy().into_owned();
        let bare = outside.path().join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        let bare_text = bare.to_string_lossy().into_owned();
        assert!(call_with(
            root.path(),
            &["status"],
            &[("HOME", &bare_text), ("XDG_CONFIG_HOME", &xdg_text)]
        )
        .is_err());
    }

    #[test]
    fn an_included_hooks_path_keeps_the_scope_of_the_file_that_includes_it() {
        let root = repository();
        let outside = tempfile::tempdir().unwrap();
        let target = hostile_file(outside.path(), "included");
        // Direct include from a global file the caller selected.
        let direct = outside.path().join("direct");
        std::fs::write(&direct, format!("[include]\n\tpath = {target}\n")).unwrap();
        let direct_text = direct.to_string_lossy().into_owned();
        assert!(call_with(
            root.path(),
            &["status"],
            &[("GIT_CONFIG_GLOBAL", &direct_text)]
        )
        .is_err());
        // Conditional include whose condition matches this repository.
        let conditional = outside.path().join("conditional");
        std::fs::write(
            &conditional,
            format!(
                "[includeIf \"gitdir:{}/\"]\n\tpath = {target}\n",
                root.path().canonicalize().unwrap().to_string_lossy()
            ),
        )
        .unwrap();
        let conditional_text = conditional.to_string_lossy().into_owned();
        assert!(call_with(
            root.path(),
            &["status"],
            &[("GIT_CONFIG_GLOBAL", &conditional_text)]
        )
        .is_err());
        // An include named on the command line keeps the command scope.
        let include = format!("include.path={target}");
        assert!(call(root.path(), &["-c", &include, "status"]).is_err());
    }

    #[test]
    fn a_local_hooks_path_overrides_an_injected_one() {
        let root = repository();
        let outside = tempfile::tempdir().unwrap();
        let file = hostile_file(outside.path(), "hostile");
        set_local_hooks_path(root.path());
        // The repository's own value is the last one Git reads, so the
        // injected global value is ineffective and the call is not refused.
        assert!(call_with(root.path(), &["status"], &[("GIT_CONFIG_GLOBAL", &file)]).is_ok());
        assert!(call(root.path(), &["status"]).is_ok());
        assert!(call(root.path(), &["-c", "user.name=x", "status"]).is_ok());
    }

    #[test]
    fn a_repository_location_the_caller_chose_refuses() {
        let root = repository();
        let other = repository();
        let own_git = root.path().join(".git").to_string_lossy().into_owned();
        let other_git = other.path().join(".git").to_string_lossy().into_owned();
        let other_tree = other.path().to_string_lossy().into_owned();
        for (name, value) in [
            ("GIT_DIR", other_git.as_str()),
            ("GIT_COMMON_DIR", other_git.as_str()),
            ("GIT_WORK_TREE", other_tree.as_str()),
        ] {
            let inherited = [(name.to_string(), value.to_string())]
                .into_iter()
                .collect();
            let refused = crate::security::git_config::inherited_location_is_own(
                root.path(),
                &[],
                &crate::security::git_config::Environment::new(),
                &inherited,
            )
            .expect_err(name);
            assert!(refused.contains(name), "{refused}");
        }
        // The variable naming the repository the guard found itself is not
        // a move, and neither is no variable.
        let own: crate::security::git_config::Environment =
            [("GIT_DIR".to_string(), own_git)].into_iter().collect();
        assert!(crate::security::git_config::inherited_location_is_own(
            root.path(),
            &[],
            &crate::security::git_config::Environment::new(),
            &own
        )
        .is_ok());
        assert!(call(root.path(), &["status"]).is_ok());
    }

    #[test]
    fn no_hooks_path_is_not_judged() {
        let root = repository();
        assert!(call(root.path(), &["status"]).is_ok());
    }

    #[test]
    fn an_unreadable_configuration_refuses() {
        let root = repository();
        std::fs::write(root.path().join(".git/config"), b"[broken\n").unwrap();
        assert!(call(root.path(), &["status"]).is_err());
    }

    #[test]
    fn checkout_writes_its_paths_and_not_its_commit() {
        let none = Vec::<String>::new();
        assert_eq!(written("checkout", "-B main HEAD~3"), none);
        assert_eq!(written("checkout", "-b x origin/main"), none);
        assert_eq!(written("checkout", "HEAD~3"), none);
        assert_eq!(written("checkout", "main^"), none);
        assert_eq!(written("checkout", "HEAD~1 -- a.txt"), vec!["a.txt"]);
        assert_eq!(written("checkout", "-- .claude/x"), vec![".claude/x"]);
        assert_eq!(
            written("checkout", "main .claude/x"),
            vec!["main", ".claude/x"]
        );
        assert_eq!(
            written("checkout", "../.codeflow~1"),
            vec!["../.codeflow~1"]
        );
        assert_eq!(written("restore", "--source HEAD~1 a.txt"), vec!["a.txt"]);
        assert_eq!(written("restore", "--staged a.txt"), none);
    }
}
