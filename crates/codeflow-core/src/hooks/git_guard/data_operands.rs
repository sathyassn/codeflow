//! Git operands, programs and environment a line supplies as data (TSK-326).
//!
//! The guards judge a git command by the words it receives. A word the
//! shell fills in from data the guard cannot read has no text to judge: a
//! variable assigned from a command substitution or `read`, an inherited
//! variable, or the input `xargs` or `find -exec` puts in place of `{}` or
//! appends. Such a word in the program position, in a git environment
//! assignment, or in an operand a guard's decisions read is refused as
//! unresolved, before the push, alias, transport and release decisions read
//! it. A variable the line assigned a literal before the word is filled in
//! with that literal and judged as written. Both guards read the line with
//! [`command_data`] and [`git_args`], each with its own set of decisions
//! ([`Reader`]).

use std::collections::HashMap;

use super::super::git_target::{expand_word, has_substitution, Cwd, ShellState, Val};
use super::{
    basename, cd_target, command_argv, command_argv_braces, command_argv_live, configuration_input,
    configuration_launch, expand_shell_commands, expands, flat_top_level, git_program_environment,
    is_identifier, is_shell, launcher_name, map_top_level, message_values, parallel_template,
    plain_pushd, quote_words, shell_tokens, shown_word, strip_reserved_words, xargs_command,
    GIT_BUILTINS, GLOBAL_VALUE_OPTIONS, JUDGED_SUBCOMMANDS, NESTING_UNREAD,
};

/// Stands in a word for text that `xargs` or `find` supplies from its
/// input when it runs the command.
const SUPPLIED: char = '\u{3}';

/// Why a word written as a command substitution cannot be read.
const SUBSTITUTION: &str = "a command substitution";

/// Stand in a word for a `$` and a backtick the shell passes as text
/// (quoted or escaped), so that only a live expansion is read as one.
const TEXT_DOLLAR: char = '\u{4}';
const TEXT_BACKTICK: char = '\u{5}';

/// Ends a word that holds a brace list or sequence outside quotes, which the
/// shell expands into several words.
const BRACES: char = '\u{6}';

/// Why a word with a brace list or sequence cannot be read as one word.
const BRACE_EXPANSION: &str = "brace expansion, which can make it several words";

/// `words`, the argument vector after its leading reserved words, with each
/// `$` and backtick of a word the shell does not expand marked as text, and
/// each word with a brace list or sequence outside quotes marked as one.
/// `live` and `braces` hold, for the whole argument vector, whether each
/// word expands and whether it holds brace syntax; `words` is their tail. A
/// word with no flag is read as expanding.
pub(crate) fn mark_text(words: &[String], live: &[bool], braces: &[bool]) -> Vec<String> {
    let offset = live.len().saturating_sub(words.len());
    let brace_offset = braces.len().saturating_sub(words.len());
    words
        .iter()
        .enumerate()
        .map(|(at, word)| {
            let mut word = if live.get(offset + at).copied().unwrap_or(true) {
                word.clone()
            } else {
                word.replace('$', &TEXT_DOLLAR.to_string())
                    .replace('`', &TEXT_BACKTICK.to_string())
            };
            if braces.get(brace_offset + at).copied().unwrap_or(false) {
                word.push(BRACES);
            }
            word
        })
        .collect()
}

/// `word` with the marks of [`mark_text`] turned back into the text written.
pub(crate) fn unmark(word: &str) -> String {
    word.replace(TEXT_DOLLAR, "$")
        .replace(TEXT_BACKTICK, "`")
        .replace(BRACES, "")
}

/// Which guard reads the line, and so which git subcommands' operands its
/// decisions judge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reader {
    /// git-guard: every subcommand it judges for protected refs.
    GitGuard,
    /// exec-guard: the subcommands its push, alias, transport and release
    /// decisions read.
    ExecGuard,
}

impl Reader {
    /// Whether this guard's decisions read the operands of `sub`. A name
    /// that is not a builtin may be an alias, which can expand to any
    /// command.
    fn judges(self, sub: &str) -> bool {
        if !GIT_BUILTINS.contains(&sub) {
            return true;
        }
        match self {
            Self::GitGuard => JUDGED_SUBCOMMANDS.contains(&sub),
            Self::ExecGuard => matches!(
                sub,
                "push" | "tag" | "config" | "update-ref" | "remote" | "credential"
            ),
        }
    }
}

/// A form the guard cannot resolve, for a refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Form {
    /// What is filled in from data and where it lands, in words.
    pub(crate) what: String,
    /// The git subcommand the form reaches, when it is known.
    pub(crate) sub: Option<String>,
    /// `true` for an operand of a git subcommand, which git-guard judges
    /// with that subcommand's own rules.
    pub(crate) operand: bool,
    /// `true` when the word is a command substitution written on the git
    /// command itself, in its program, subcommand, option or operand
    /// position. git-guard's own reader judges those already (TSK-112), so
    /// it leaves them to that reader; exec-guard refuses them here.
    pub(crate) substitution: bool,
}

/// The value the shell gives `word`, from the literals the line assigned
/// (`vars`), or why the guard cannot know it.
fn fill(word: &str, vars: &HashMap<String, Val>, supplier: Option<&str>) -> Result<String, String> {
    if word.contains(SUPPLIED) {
        return Err(format!(
            "the input {} supplies",
            supplier.unwrap_or("a command")
        ));
    }
    if word.contains(BRACES) {
        return Err(BRACE_EXPANSION.to_string());
    }
    if !expands(word) {
        return Ok(word.to_string());
    }
    if has_substitution(word) {
        return Err(SUBSTITUTION.to_string());
    }
    expand_word(word, vars)
}

/// How a word reads in a refusal.
fn shown(word: &str) -> String {
    let word = shown_word(&unmark(word));
    if word.contains(SUPPLIED) {
        word.replace(SUPPLIED, "{}")
    } else {
        word
    }
}

fn unresolved(word: &str, place: &str, why: &str, sub: Option<&str>, operand: bool) -> Form {
    // `expand_word` names a variable with no value it can read by the
    // variable alone; say what that means.
    let shown = shown(word);
    let what = if why.starts_with("`$") && why.ends_with('`') {
        if why.trim_matches('`') == shown {
            format!("`{shown}` {place} is filled in at run time, since the line does not set it to a literal")
        } else {
            format!("`{shown}` {place} is filled in at run time from {why}, which the line does not set to a literal")
        }
    } else {
        format!("`{shown}` {place} is filled in at run time from {why}")
    };
    Form {
        what,
        sub: sub.map(unmark),
        operand,
        substitution: why == SUBSTITUTION && !place.starts_with("sets `"),
    }
}

/// Whether a variable assigned for git can change what it reads or runs.
fn git_environment(name: &str) -> bool {
    name.starts_with("GIT_")
        || configuration_input(name)
        || git_program_environment().any(|known| known == name)
}

/// Whether `word` is a git option whose value is a message, which no
/// decision reads; `Some(true)` when the value is the next word. Only a
/// subcommand whose `-m` takes a message has one ([`message_values`]).
fn message_option(word: &str, messages: bool) -> Option<bool> {
    if !messages {
        return None;
    }
    if word == "-m" || word == "--message" {
        return Some(true);
    }
    if word.starts_with("--message=") {
        return Some(false);
    }
    if word.starts_with('-') && !word.starts_with("--") {
        if word.ends_with('m') {
            return Some(true);
        }
        if word.len() > 2 && word[1..].starts_with('m') {
            return Some(false);
        }
    }
    None
}

/// Whether a git global option's value is configuration: `-c` and
/// `--config-env`, attached or as the next word.
fn configuration_option(word: &str) -> bool {
    word == "-c"
        || word == "--config-env"
        || word.starts_with("--config-env=")
        || (word.starts_with("-c") && !word.starts_with("--"))
}

/// The arguments git receives (`args`, after the program), with every word
/// a decision of `reader` reads filled in from the line's literals, and
/// whether any word changed. A word in the subcommand position, a
/// configuration value of a judged subcommand, or an operand of a judged
/// subcommand that the guard cannot fill in is the refused form. Other
/// words that expand are kept as written: the target and branch readers
/// judge them on their own.
pub(super) fn git_args(
    args: &[String],
    vars: &HashMap<String, Val>,
    reader: Reader,
) -> Result<(Vec<String>, bool), Form> {
    let (filled, changed) = git_args_supplied(args, vars, reader, None)?;
    Ok((filled.iter().map(|word| unmark(word)).collect(), changed))
}

fn git_args_supplied(
    args: &[String],
    vars: &HashMap<String, Val>,
    reader: Reader,
    supplier: Option<&str>,
) -> Result<(Vec<String>, bool), Form> {
    // Find the subcommand first: whether a global option's value matters
    // depends on what it configures.
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if GLOBAL_VALUE_OPTIONS.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    let sub_at = at.min(args.len());
    let sub = match args.get(sub_at) {
        Some(word) => match fill(word, vars, supplier) {
            Ok(sub) => Some(sub),
            Err(why) => {
                return Err(unresolved(
                    word,
                    "in the git subcommand position",
                    &why,
                    None,
                    false,
                ))
            }
        },
        // `xargs git` takes its subcommand from its input.
        None => None,
    };
    let judged = sub.as_deref().is_none_or(|sub| reader.judges(sub));
    let mut filled = Filled::default();
    let data = Data {
        vars,
        supplier,
        judged,
    };
    data.globals(&args[..sub_at], sub.as_deref(), &mut filled)?;
    let Some(sub) = sub else {
        // No subcommand on the line: `xargs` supplies it.
        if supplier.is_some() {
            return Err(Form {
                what: format!(
                    "the git subcommand and its operands are filled in at run time from the input {} supplies",
                    supplier.unwrap_or("a command")
                ),
                sub: None,
                operand: false,
                substitution: false,
            });
        }
        return Ok((filled.words, filled.changed));
    };
    filled.keep(&args[sub_at], Some(sub.clone()));
    data.operands(&args[sub_at + 1..], &sub, &mut filled)?;
    Ok((filled.words, filled.changed))
}

/// Git's arguments as they are filled in, and whether any word changed.
#[derive(Default)]
struct Filled {
    words: Vec<String>,
    changed: bool,
}

impl Filled {
    /// Adds `word`, as `value` when it was filled in to that.
    fn keep(&mut self, word: &str, value: Option<String>) {
        match value {
            Some(value) if value != word => {
                self.changed = true;
                self.words.push(value);
            }
            _ => self.words.push(word.to_string()),
        }
    }
}

/// What filling in one git call's words reads: the line's literals, the
/// command supplying input, and whether the subcommand's words are judged.
struct Data<'a> {
    vars: &'a HashMap<String, Val>,
    supplier: Option<&'a str>,
    judged: bool,
}

impl Data<'_> {
    fn fill(&self, word: &str) -> Result<String, String> {
        fill(word, self.vars, self.supplier)
    }

    /// Global options: a configuration value is judged with the
    /// subcommand; any other value is filled in when the guard can, else
    /// kept.
    fn globals(
        &self,
        globals: &[String],
        sub: Option<&str>,
        filled: &mut Filled,
    ) -> Result<(), Form> {
        let refused = |word: &str, why: &str| {
            unresolved(word, "in a git configuration option", why, sub, true)
        };
        let mut i = 0;
        while let Some(word) = globals.get(i) {
            let configures = self.judged && configuration_option(word);
            // A brace list makes one option several words, which can change
            // the subcommand git runs.
            if word.contains(BRACES) {
                return Err(refused(word, BRACE_EXPANSION));
            }
            filled.keep(word, None);
            if GLOBAL_VALUE_OPTIONS.contains(&word.as_str()) {
                if let Some(value) = globals.get(i + 1) {
                    match self.fill(value) {
                        Ok(v) => filled.keep(value, Some(v)),
                        Err(why) if configures || value.contains(BRACES) => {
                            return Err(refused(value, &why))
                        }
                        Err(_) => filled.keep(value, None),
                    }
                }
                i += 2;
                continue;
            }
            if configures && (expands(word) || word.contains(SUPPLIED)) {
                if let Err(why) = self.fill(word) {
                    return Err(refused(word, &why));
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// The operands of `sub`: each is filled in when it is judged, except a
    /// message value. A substitution is reported only when no other operand
    /// is data, so git-guard's deferral to its own reader never hides a data
    /// variable after it.
    fn operands(&self, rest: &[String], sub: &str, filled: &mut Filled) -> Result<(), Form> {
        let mut deferred: Option<Form> = None;
        let messages = message_values(sub, rest);
        let mut j = 0;
        while let Some(word) = rest.get(j) {
            j += 1;
            if !self.judged {
                filled.keep(word, None);
                continue;
            }
            if let Some(next) = message_option(word, messages) {
                filled.keep(word, None);
                if let Some(value) = rest.get(j).filter(|_| next) {
                    // A brace list in a message makes more words after it,
                    // which git reads as operands.
                    if value.contains(BRACES) {
                        return Err(unresolved(
                            value,
                            &format!("in a `git {sub}` message"),
                            BRACE_EXPANSION,
                            Some(sub),
                            true,
                        ));
                    }
                    filled.keep(value, None);
                    j += 1;
                }
                continue;
            }
            match self.fill(word) {
                Ok(value) => filled.keep(word, Some(value)),
                Err(why) => {
                    let form = unresolved(
                        word,
                        &format!("in a `git {sub}` operand"),
                        &why,
                        Some(sub),
                        true,
                    );
                    if !form.substitution {
                        return Err(form);
                    }
                    deferred.get_or_insert(form);
                    filled.keep(word, None);
                }
            }
        }
        deferred.map_or(Ok(()), Err)
    }
}

/// What one simple command (`words`, quote-stripped) gives git: `None` when
/// it runs no git, else git's arguments filled in ([`git_args`]) and
/// whether its program word was filled in from a literal. A program word,
/// a git environment assignment, or a command `xargs` or `find` runs with
/// words from its input is the refused form.
pub(super) fn command_data(
    words: &[String],
    vars: &HashMap<String, Val>,
    reader: Reader,
) -> Result<Option<GitCall>, Form> {
    command_data_supplied(words, vars, reader, None)
}

/// A git invocation as [`command_data`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GitCall {
    /// Git's arguments, filled in from the line's literals.
    pub(crate) args: Vec<String>,
    /// Whether a word changed when it was filled in.
    pub(crate) changed: bool,
    /// Whether the program word was filled in (`PROG=git; $PROG push`).
    pub(crate) program_filled: bool,
}

fn command_data_supplied(
    words: &[String],
    vars: &HashMap<String, Val>,
    reader: Reader,
    supplier: Option<&str>,
) -> Result<Option<GitCall>, Form> {
    let Some((program, args)) = configuration_launch(words) else {
        return Ok(None);
    };
    let program_at = words.len() - args.len() - 1;
    let filled_program =
        if expands(program) || program.contains(SUPPLIED) || program.contains(BRACES) {
            match fill(program, vars, supplier) {
                Ok(value) => value,
                // A program with no sign of git is left to exec-guard's own
                // reader, which refuses a program it cannot read where it
                // decides one; this refusal is for a program that may be git.
                Err(_) if !git_evidence(args) => return Ok(None),
                Err(why) => {
                    return Err(unresolved(
                        program,
                        "in the program position",
                        &why,
                        None,
                        false,
                    ))
                }
            }
        } else {
            program.to_string()
        };
    let program_filled = filled_program != program;
    match launcher_name(&filled_program) {
        "xargs" => {
            if let Some(child) = xargs_command(args) {
                let options = &args[..args.len() - child.len()];
                let child: Vec<String> = match xargs_replacement(options) {
                    Some(token) => child.iter().map(|w| w.replace(&token, "\u{3}")).collect(),
                    None => child
                        .iter()
                        .cloned()
                        .chain(std::iter::once(SUPPLIED.to_string()))
                        .collect(),
                };
                command_data_supplied(&child, vars, reader, Some("`xargs`")).map_err(supplied)?;
            }
            return Ok(None);
        }
        "find" => {
            for child in find_commands(args) {
                let child: Vec<String> = child.iter().map(|w| w.replace("{}", "\u{3}")).collect();
                command_data_supplied(&child, vars, reader, Some("`find`")).map_err(supplied)?;
            }
            return Ok(None);
        }
        // `parallel` and `fd -x` put their input in place of a replacement
        // string, or append it, as `xargs` does.
        "parallel" => {
            if let Some(child) = parallel_data_command(args) {
                let child = supply(child);
                command_data_supplied(&child, vars, reader, Some("`parallel`"))
                    .map_err(supplied)?;
            }
            for child in parallel_shell_children(args) {
                let child = supply(&child);
                command_data_supplied(&child, vars, reader, Some("`parallel`"))
                    .map_err(supplied)?;
            }
            return Ok(None);
        }
        "fd" | "fdfind" => {
            if let Some(child) = fd_command(args) {
                let child = supply(child);
                command_data_supplied(&child, vars, reader, Some("`fd`")).map_err(supplied)?;
            }
            return Ok(None);
        }
        _ => {}
    }
    let name = basename(&filled_program);
    if let Some(supplier) = supplier.filter(|_| is_shell(name) || name == "eval") {
        shell_script_data(name, args, vars, reader, supplier)?;
        return Ok(None);
    }
    if name.strip_suffix(".exe").unwrap_or(name) != "git" {
        return Ok(None);
    }
    for word in &words[..program_at] {
        let Some((name, value)) = word.split_once('=') else {
            continue;
        };
        if is_identifier(name) && git_environment(name) {
            if let Err(why) = fill(value, vars, supplier) {
                return Err(unresolved(
                    word,
                    &format!("sets `{name}` for git and"),
                    &why,
                    None,
                    false,
                ));
            }
        }
    }
    let (args, changed) = git_args_supplied(args, vars, reader, supplier)?;
    Ok(Some(GitCall {
        args: args.iter().map(|word| unmark(word)).collect(),
        changed,
        program_filled,
    }))
}

/// Whether the arguments of a program the guard cannot read show that it
/// may be git: a git subcommand git-guard judges or a git global option
/// first, or a word naming git (`env "$V" git push`).
fn git_evidence(args: &[String]) -> bool {
    let named_git = args.iter().any(|word| {
        let name = basename(word);
        name.strip_suffix(".exe").unwrap_or(name) == "git"
    });
    named_git
        || args.first().is_some_and(|first| {
            JUDGED_SUBCOMMANDS.contains(&first.as_str())
                || GLOBAL_VALUE_OPTIONS.contains(&first.as_str())
                || first.starts_with("--git-dir")
                || first.starts_with("--work-tree")
                || first.starts_with("--exec-path")
                || first.starts_with("--config-env")
        })
}

/// A shell (`sh -c`, `bash -c` and the like) or `eval` that `supplier` runs
/// with its input. The script is read as a command line in which the
/// replacement strings and the positional parameters (`$1`, `"$@"`, `$0`)
/// are supplied input, so a git command that takes one as an operand
/// refuses as unresolved. Input spliced into the script text is code from
/// data, and a script the guard cannot read (a file, standard input, a
/// variable, or code it cannot parse) refuses as well (TSK-326).
fn shell_script_data(
    name: &str,
    args: &[String],
    vars: &HashMap<String, Val>,
    reader: Reader,
    supplier: &str,
) -> Result<(), Form> {
    let runs = if name == "eval" {
        "`eval`".to_string()
    } else {
        format!("`{name}`")
    };
    let place = format!("as the script {runs} runs");
    let script = shell_script(name, args).map_err(|why| Form {
        what: format!(
            "the code {runs} runs with the input {supplier} supplies is filled in at run time from {why}, which the guard does not read"
        ),
        sub: None,
        operand: false,
        substitution: false,
    })?;
    let text = fill(&script, vars, Some(supplier))
        .map_err(|why| unresolved(&script, &place, &why, None, false))?;
    let text = positional_supplied(&unmark(&text));
    let empty = HashMap::new();
    for segment in &expand_shell_commands(&text).segments {
        if segment == NESTING_UNREAD || segment == "__unresolved_shell_code" {
            return Err(unresolved(
                &script,
                &place,
                "shell code the guard cannot read",
                None,
                false,
            ));
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        if words.is_empty() {
            continue;
        }
        let marked = mark_text(
            &words,
            &command_argv_live(segment),
            &command_argv_braces(segment),
        );
        command_data_supplied(&marked, &empty, reader, Some(supplier))?;
    }
    Ok(())
}

/// The script a shell runs from `args` with `-c` (fish's `--command` too),
/// or for `eval` its words joined; otherwise what it runs instead, in words.
fn shell_script(name: &str, args: &[String]) -> Result<String, &'static str> {
    if name == "eval" {
        return Ok(args.join(" "));
    }
    let plain = name.to_ascii_lowercase();
    let plain = plain.trim_end_matches(".exe");
    if matches!(plain, "pwsh" | "powershell" | "cmd") {
        return Err("a PowerShell or cmd command");
    }
    let mut command = false;
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if word == "--" || word == "-" {
            at += 1;
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            if let Some(script) = long.strip_prefix("command=") {
                return Ok(script.to_string());
            }
            command |= long == "command";
            at += 1;
            continue;
        }
        let Some(flags) = word
            .strip_prefix('-')
            .or_else(|| word.strip_prefix('+'))
            .filter(|flags| !flags.is_empty())
        else {
            break;
        };
        command |= flags.contains('c');
        // `-o NAME` and `-O NAME` take the next word.
        if flags.ends_with(['o', 'O']) {
            at += 1;
        }
        at += 1;
    }
    match args.get(at) {
        Some(script) if command => Ok(script.clone()),
        _ if command => Err("`-c` with no script"),
        _ => Err("a script file or standard input"),
    }
}

/// `text` with each positional parameter (`$0` to `$9`, `${N}`, `$@`, `$*`,
/// `${@}`, `${*}`) marked as supplied input.
fn positional_supplied(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '$' {
            match chars.get(i + 1) {
                Some(next) if next.is_ascii_digit() || matches!(next, '@' | '*') => {
                    out.push(SUPPLIED);
                    i += 2;
                    continue;
                }
                Some('{') => {
                    let inner: String =
                        chars[i + 2..].iter().take_while(|&&ch| ch != '}').collect();
                    let closed = chars.get(i + 2 + inner.chars().count()) == Some(&'}');
                    if closed
                        && (matches!(inner.as_str(), "@" | "*")
                            || (!inner.is_empty() && inner.chars().all(|ch| ch.is_ascii_digit())))
                    {
                        out.push(SUPPLIED);
                        i += 3 + inner.chars().count();
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A form inside a command `xargs` or `find` runs: no git invocation on
/// the line carries it, so the line itself refuses.
fn supplied(form: Form) -> Form {
    Form {
        operand: false,
        substitution: false,
        ..form
    }
}

/// The replacement string of an `xargs` call (`-I {}`, `-J %`, `-i`,
/// `--replace`), read from its options.
fn xargs_replacement(options: &[String]) -> Option<String> {
    let mut at = 0;
    while let Some(word) = options.get(at) {
        at += 1;
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            if name.len() >= 3 && "replace".starts_with(name) {
                return Some(value.unwrap_or("{}").to_string());
            }
            continue;
        }
        let Some(cluster) = word.strip_prefix('-') else {
            continue;
        };
        for (offset, letter) in cluster.char_indices() {
            let attached = &cluster[offset + letter.len_utf8()..];
            match letter {
                'I' | 'J' => {
                    return if attached.is_empty() {
                        options.get(at).cloned()
                    } else {
                        Some(attached.to_string())
                    };
                }
                'i' => {
                    return Some(if attached.is_empty() { "{}" } else { attached }.to_string());
                }
                'a' | 'd' | 'E' | 'L' | 'n' | 'P' | 's' | 'R' | 'S' => {
                    if attached.is_empty() {
                        at += 1;
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    None
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` and `-okdir`.
/// The command GNU `parallel` runs with words from data: its input, an
/// `--arg-file` or a `::::` file. `None` when it has no command or its
/// only inputs are `:::` words on the line, which the launcher reader
/// judges as written.
fn parallel_data_command(args: &[String]) -> Option<&[String]> {
    let (mut start, _) = parallel_template(args);
    let from_file = args[..start].iter().any(|word| {
        word == "--arg-file"
            || word.starts_with("--arg-file=")
            || (word.starts_with("-a") && !word.starts_with("--"))
    });
    if args.get(start).is_some_and(|word| word == "--") {
        start += 1;
    }
    let rest = &args[start..];
    let end = rest
        .iter()
        .position(|word| matches!(word.as_str(), ":::" | ":::+" | "::::" | "::::+"))
        .unwrap_or(rest.len());
    let inputs = &rest[end..];
    let literal_inputs = !inputs.is_empty()
        && !inputs
            .iter()
            .any(|word| matches!(word.as_str(), "::::" | "::::+"));
    (end > 0 && (from_file || !literal_inputs)).then(|| &rest[..end])
}

/// The shells and `eval`s GNU `parallel` runs when its only inputs are
/// `:::` words on the line, as words: its template as the shell line it
/// builds (joined, or each word quoted under `-q`). Parallel's own quoting
/// of an input spliced into a script is not modelled, so the input counts
/// as supplied there and a replacement string inside the script refuses,
/// as it does under `find` and `xargs` (TSK-326). Empty when the inputs
/// come from data, which [`parallel_data_command`] reads.
fn parallel_shell_children(args: &[String]) -> Vec<Vec<String>> {
    if parallel_data_command(args).is_some() {
        return Vec::new();
    }
    let (mut start, quoted) = parallel_template(args);
    if args.get(start).is_some_and(|word| word == "--") {
        start += 1;
    }
    let rest = args.get(start..).unwrap_or_default();
    let end = rest
        .iter()
        .position(|word| matches!(word.as_str(), ":::" | ":::+" | "::::" | "::::+"))
        .unwrap_or(rest.len());
    if end == 0 {
        return Vec::new();
    }
    let line = if quoted {
        quote_words(&rest[..end])
    } else {
        rest[..end].join(" ")
    };
    let mut children = Vec::new();
    for segment in &expand_shell_commands(&line).segments {
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let runs_shell = configuration_launch(&words).is_some_and(|(program, _)| {
            let name = basename(program);
            is_shell(name) || name == "eval"
        });
        if runs_shell {
            children.push(mark_text(
                &words,
                &command_argv_live(segment),
                &command_argv_braces(segment),
            ));
        }
    }
    children
}

/// The command `fd` runs for each result (`-x`, `--exec`) or for all of
/// them (`-X`, `--exec-batch`): the words after the option, up to a `;`.
pub(super) fn fd_command(args: &[String]) -> Option<&[String]> {
    let at = args
        .iter()
        .position(|word| matches!(word.as_str(), "-x" | "--exec" | "-X" | "--exec-batch"))?;
    let tail = &args[at + 1..];
    let end = tail
        .iter()
        .position(|word| word == ";")
        .unwrap_or(tail.len());
    (end > 0).then(|| &tail[..end])
}

/// `child` with each replacement string `parallel` and `fd` fill in (`{}`,
/// `{.}`, `{/}`, `{//}`, `{/.}`, `{#}`, `{%}`, `{1}` and the like) marked as
/// supplied input; with none, the input is appended, as both do.
fn supply(child: &[String]) -> Vec<String> {
    let mut found = false;
    let mut out: Vec<String> = child
        .iter()
        .map(|word| {
            let mut text = String::new();
            let mut rest = word.as_str();
            while let Some(open) = rest.find('{') {
                text.push_str(&rest[..open]);
                let after = &rest[open + 1..];
                match after.find('}') {
                    Some(close)
                        if after[..close]
                            .chars()
                            .all(|c| matches!(c, '.' | '/' | '#' | '%' | '0'..='9')) =>
                    {
                        text.push(SUPPLIED);
                        found = true;
                        rest = &after[close + 1..];
                    }
                    _ => {
                        text.push('{');
                        rest = after;
                    }
                }
            }
            text.push_str(rest);
            text
        })
        .collect();
    if !found {
        out.push(SUPPLIED.to_string());
    }
    out
}

fn find_commands(args: &[String]) -> Vec<&[String]> {
    args.iter()
        .enumerate()
        .filter(|(_, word)| matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
        .map(|(at, _)| {
            let tail = &args[at + 1..];
            let end = tail
                .iter()
                .position(|w| w == ";" || w == "+")
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .collect()
}

/// What a whole line gives git, read as git-guard reads it: the first
/// form it supplies as data, and each git invocation whose words the line's
/// literals filled in, with a `-C` for a directory a `cd` moved it to. A
/// filled-in invocation whose directory the guard cannot follow is the
/// refused form.
#[derive(Debug, Default)]
pub(crate) struct LineData {
    pub(crate) unresolved: Option<Form>,
    pub(crate) filled: Vec<Vec<String>>,
}

/// Read `command` for [`LineData`] with `reader`'s decisions.
pub(crate) fn line_data(command: &str, reader: Reader) -> LineData {
    let expanded = expand_shell_commands(command);
    let segments = &expanded.segments;
    let roles = flat_top_level(command).and_then(|top| map_top_level(segments, &top));
    let mut shell = ShellState::new(roles.is_some());
    let empty = HashMap::new();
    let mut out = LineData::default();
    for (idx, segment) in segments.iter().enumerate() {
        let top_level = match roles.as_ref().map(|r| r[idx]) {
            Some(Some(join)) => {
                shell.begin(join);
                true
            }
            _ => false,
        };
        if segment == NESTING_UNREAD || segment == "__unresolved_shell_code" {
            continue;
        }
        let mut tokens = shell_tokens(segment);
        strip_reserved_words(&mut tokens);
        if tokens.is_empty() {
            continue;
        }
        if let Some(dir) = cd_target(&tokens) {
            if top_level {
                if tokens[0] == "cd" || plain_pushd(segment) {
                    shell.cd(&dir);
                } else {
                    shell.observe(&tokens[0], &tokens[1..]);
                }
            }
            continue;
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let marked = mark_text(
            &words,
            &command_argv_live(segment),
            &command_argv_braces(segment),
        );
        let launched = configuration_launch(&words);
        if top_level {
            let redirected = words.len() != tokens.len();
            shell.assign(&tokens, launched.map(|(program, _)| program), redirected);
            if let Some((program, args)) = launched {
                shell.observe(program, args);
            }
        }
        let vars = if top_level && shell.flat() {
            &shell.vars
        } else {
            &empty
        };
        match command_data(&marked, vars, reader) {
            Err(form) => {
                out.unresolved = Some(form);
                return out;
            }
            Ok(Some(call)) if call.changed || call.program_filled => {
                let mut argv = vec!["git".to_string()];
                if top_level {
                    match &shell.cwd {
                        Cwd::Paths(dirs) if dirs.len() == 1 => {
                            if !dirs[0].is_empty() {
                                argv.extend(["-C".to_string(), dirs[0].clone()]);
                            }
                        }
                        _ => {
                            out.unresolved = Some(Form {
                                what: "a git command whose words the line fills in runs in a directory the guard cannot follow".to_string(),
                                sub: None,
                                operand: false,
                                substitution: false,
                            });
                            return out;
                        }
                    }
                }
                argv.extend(call.args);
                out.filled.push(argv);
            }
            Ok(_) => {}
        }
    }
    out
}

/// The line with each bracket arithmetic expansion (`$[2]`) that holds
/// only integer arithmetic replaced by its value, as bash and zsh expand
/// it; `None` when the line has none the guard can replace. Only a line
/// whose quoting the scan reads exactly is rewritten: one with a command
/// substitution, a backtick, a heredoc or `$'…'` / `$"…"` quoting keeps
/// its text, so its expansions stay unresolved. Single-quoted text,
/// escaped dollars and comments are left as written; an expression the
/// evaluator does not hold, or one that overflows or divides by zero, is
/// left too, and the guard refuses it as unresolved.
pub(crate) fn resolve_bracket_arithmetic(command: &str) -> Option<String> {
    if !command.contains("$[")
        || ["$(", "`", "<<", "$'", "$\""]
            .iter()
            .any(|form| command.contains(form))
    {
        return None;
    }
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut changed = false;
    let mut double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                out.push(c);
                if let Some(&next) = chars.get(i + 1) {
                    out.push(next);
                }
                i += 2;
                continue;
            }
            '\'' if !double => {
                let close = chars[i + 1..].iter().position(|&ch| ch == '\'')? + i + 1;
                out.extend(&chars[i..=close]);
                i = close + 1;
                continue;
            }
            '"' => double = !double,
            '#' if !double
                && (i == 0
                    || matches!(chars[i - 1], ' ' | '\t' | '\n')
                    || ";|&(".contains(chars[i - 1])) =>
            {
                let end = chars[i..]
                    .iter()
                    .position(|&ch| ch == '\n')
                    .map_or(chars.len(), |p| p + i);
                out.extend(&chars[i..end]);
                i = end;
                continue;
            }
            '$' if chars.get(i + 1) == Some(&'[') => {
                if let Some(close) = chars[i + 2..].iter().position(|&ch| ch == ']') {
                    let close = close + i + 2;
                    let expression: String = chars[i + 2..close].iter().collect();
                    if let Some(value) = arithmetic_value(&expression) {
                        out.push_str(&value.to_string());
                        changed = true;
                        i = close + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    changed.then_some(out)
}

/// The value of an integer arithmetic expression of decimal literals, `+`,
/// `-`, `*`, `/`, `%` and parentheses, as the shell computes it, or `None`
/// for anything else: a name, an assignment, a base or octal literal, an
/// overflow or a division by zero.
fn arithmetic_value(expression: &str) -> Option<i64> {
    let tokens: Vec<char> = expression
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n'))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut at = 0;
    let value = sum(&tokens, &mut at, 0)?;
    (at == tokens.len()).then_some(value)
}

/// Nesting deeper than this is not read.
const ARITHMETIC_DEPTH: usize = 16;

fn sum(tokens: &[char], at: &mut usize, depth: usize) -> Option<i64> {
    let mut value = product(tokens, at, depth)?;
    while let Some(&op) = tokens.get(*at) {
        if !matches!(op, '+' | '-') {
            break;
        }
        *at += 1;
        let right = product(tokens, at, depth)?;
        value = if op == '+' {
            value.checked_add(right)?
        } else {
            value.checked_sub(right)?
        };
    }
    Some(value)
}

fn product(tokens: &[char], at: &mut usize, depth: usize) -> Option<i64> {
    let mut value = unary(tokens, at, depth)?;
    while let Some(&op) = tokens.get(*at) {
        if !matches!(op, '*' | '/' | '%') {
            break;
        }
        *at += 1;
        let right = unary(tokens, at, depth)?;
        value = match op {
            '*' => value.checked_mul(right)?,
            '/' => value.checked_div(right)?,
            _ => value.checked_rem(right)?,
        };
    }
    Some(value)
}

fn unary(tokens: &[char], at: &mut usize, depth: usize) -> Option<i64> {
    if depth > ARITHMETIC_DEPTH {
        return None;
    }
    match tokens.get(*at)? {
        '-' => {
            *at += 1;
            unary(tokens, at, depth + 1)?.checked_neg()
        }
        '+' => {
            *at += 1;
            unary(tokens, at, depth + 1)
        }
        '(' => {
            *at += 1;
            let value = sum(tokens, at, depth + 1)?;
            (tokens.get(*at) == Some(&')')).then(|| *at += 1)?;
            Some(value)
        }
        c if c.is_ascii_digit() => {
            let start = *at;
            while tokens.get(*at).is_some_and(char::is_ascii_digit) {
                *at += 1;
            }
            let digits: String = tokens[start..*at].iter().collect();
            // A leading zero is an octal literal; not read.
            if digits.len() > 1 && digits.starts_with('0') {
                return None;
            }
            digits.parse().ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_arithmetic_resolves_integer_expressions_only() {
        let cases = [
            (
                "git push origin HEAD:v$[2]",
                Some("git push origin HEAD:v2"),
            ),
            (
                "git push origin \"HEAD:v$[1+2]\"",
                Some("git push origin \"HEAD:v3\""),
            ),
            (
                "git push origin HEAD:v$[ (7 - 1) / 2 ]",
                Some("git push origin HEAD:v3"),
            ),
            ("git push origin HEAD:v$[x]", None),
            ("git push origin HEAD:v$[HOME=1]", None),
            ("git push origin HEAD:v$[010]", None),
            ("git push origin HEAD:v$[1/0]", None),
            ("git push origin HEAD:v$[9223372036854775807+1]", None),
            ("git push origin 'HEAD:v$[2]'", None),
            ("git push origin HEAD:v\\$[2]", None),
            ("git push origin HEAD # v$[2]", None),
            ("git push origin HEAD:v$[2] $(true)", None),
            ("cat <<EOF\n$[2]\nEOF", None),
        ];
        for (line, want) in cases {
            assert_eq!(resolve_bracket_arithmetic(line).as_deref(), want, "{line}");
        }
    }

    fn words(text: &str) -> Vec<String> {
        command_argv(text)
    }

    #[test]
    fn data_supplied_program_environment_and_xargs_input_are_forms() {
        let vars = HashMap::from([
            (
                "V".to_string(),
                Val::Unknown("a command substitution".to_string()),
            ),
            ("PROG".to_string(), Val::Known("git".to_string())),
            ("B".to_string(), Val::Known("feat/x".to_string())),
        ]);
        for (line, reader) in [
            ("$V push origin HEAD:main", Reader::GitGuard),
            ("env \"$V\" git push origin", Reader::ExecGuard),
            ("GIT_DIR=\"$V\" git push origin", Reader::GitGuard),
            ("xargs git push origin", Reader::ExecGuard),
            ("xargs -I{} env {} git push origin", Reader::GitGuard),
            ("xargs -I % git push origin HEAD:%", Reader::GitGuard),
            ("xargs git", Reader::GitGuard),
            (
                "find . -name x -exec git push origin {} ;",
                Reader::ExecGuard,
            ),
            ("git push origin \"$V\"", Reader::ExecGuard),
            ("git -c \"$V\" push origin", Reader::GitGuard),
            ("git \"$V\" origin main", Reader::ExecGuard),
            ("git push origin \"$UNSET\"", Reader::GitGuard),
        ] {
            assert!(
                command_data(&words(line), &vars, reader).is_err(),
                "{line} is refused"
            );
        }
        for (line, reader) in [
            ("git push origin \"$B\"", Reader::GitGuard),
            ("$PROG push origin HEAD", Reader::ExecGuard),
            ("git log \"$V\"", Reader::GitGuard),
            ("git commit -m \"$V\"", Reader::GitGuard),
            ("xargs git add", Reader::GitGuard),
            ("xargs echo", Reader::ExecGuard),
            ("echo \"$V\"", Reader::ExecGuard),
            ("git -C \"$V\" status", Reader::GitGuard),
        ] {
            assert!(
                command_data(&words(line), &vars, reader).is_ok(),
                "{line} is read"
            );
        }
        let call = command_data(&words("$PROG push origin \"$B\""), &vars, Reader::GitGuard)
            .unwrap()
            .unwrap();
        assert_eq!(call.args, ["push", "origin", "feat/x"]);
        assert!(call.changed && call.program_filled);
    }
    /// TSK-326 review N3: `expand_word` reads a `$` with no name after it
    /// (`v$`, `$$`, `$@`, `$?`, `$[`) as unresolved; the empty name slice is
    /// an empty range, not a panic.
    #[test]
    fn a_dollar_with_no_name_is_unresolved() {
        let vars = HashMap::new();
        for word in ["v$", "HEAD:v$[2]", "$$", "$@", "a$?b", "$*", "$!", "$#"] {
            assert!(expand_word(word, &vars).is_err(), "{word}");
        }
    }

    #[test]
    fn positional_parameters_are_supplied_input() {
        assert_eq!(
            positional_supplied("git push origin \"$1\" $@ ${2} ${*} $0 ${10} ${1:-x} $V"),
            "git push origin \"\u{3}\" \u{3} \u{3} \u{3} \u{3} \u{3} ${1:-x} $V"
        );
    }

    #[test]
    fn shell_scripts_are_read_from_their_command_option() {
        let args = |line: &str| line.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(
            shell_script("sh", &args("-c git_log _")),
            Ok("git_log".into())
        );
        assert_eq!(
            shell_script("bash", &args("-ec git_log")),
            Ok("git_log".into())
        );
        assert_eq!(
            shell_script("bash", &args("-o pipefail -c git_log")),
            Ok("git_log".into())
        );
        assert_eq!(
            shell_script("fish", &args("--command=git_log")),
            Ok("git_log".into())
        );
        assert_eq!(shell_script("eval", &args("git log")), Ok("git log".into()));
        assert!(shell_script("sh", &args("script.sh x")).is_err());
        assert!(shell_script("bash", &args("-c")).is_err());
        assert!(shell_script("pwsh", &args("-c git_log")).is_err());
    }
}
