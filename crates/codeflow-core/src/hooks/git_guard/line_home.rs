//! The home a line's `~` and `$HOME` words name (TSK-379).
//!
//! The shell expands `~` and `$HOME` with the HOME the line assigned before
//! the word, not with the HOME of the guard process. Every resolver both
//! shell guards use reads the home through [`home`], so a line that
//! assigns one literal absolute HOME in statement or export form is read
//! with that home for the whole line. Any other assignment before a use
//! (an unresolved or relative value, an assignment in front of a command,
//! `read`, `unset` or a loop variable) leaves those words unread, and the
//! line refuses ([`unread_violation`]).
use super::{
    builtin_words, command_argv, expand_shell_commands, expands, hook_integrity_violation,
    integrity_write_in_run, redirect_writes, run_dirs, shell_tokens, strip_reserved_words,
    Literals, PolicyLevel, Violation,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

/// How a line's `~` and `$HOME` words resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LineHome {
    /// No word that uses the home follows an assignment of HOME.
    Inherited,
    /// One literal absolute value, assigned in statement or export form
    /// before any word that uses the home.
    Literal(PathBuf),
    /// An assignment the guard cannot read precedes a word that uses the
    /// home; the text says which.
    Unread(String),
}

/// Read how `command` assigns HOME before the words that use it.
pub(crate) fn read(command: &str) -> LineHome {
    let segments = expand_shell_commands(command).segments;
    let mut literals = Literals::default();
    let mut assigned = false;
    let mut unread: Option<&'static str> = None;
    let mut before = false;
    let mut after = false;
    for segment in &segments {
        let uses = uses_home(segment);
        if uses {
            if assigned {
                after = true;
            } else {
                before = true;
            }
        }
        let raw = shell_tokens(segment);
        let looping = raw
            .first()
            .is_some_and(|word| matches!(word.as_str(), "for" | "select"))
            && raw.get(1).is_some_and(|word| word == "HOME");
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        if looping || reads_into_home(&words) || unproven_change(&words) {
            assigned = true;
            unread.get_or_insert(
                "a command on the line can change HOME to a value the guard does not read",
            );
        }
        let words = builtin_words(&words);
        if prefix_home(words) {
            // The command's own words expand with the earlier home, but
            // what it runs (a shell body, a Git callback) sees the new one.
            assigned = true;
            after |= uses;
            unread.get_or_insert(
                "HOME is assigned in front of a command, so that command and its own words use different homes",
            );
        }
        literals.note(words);
        assigned |= literals.0.contains_key("HOME");
    }
    if !assigned || !after {
        return LineHome::Inherited;
    }
    if let Some(why) = unread {
        return LineHome::Unread(why.to_string());
    }
    match literals.0.get("HOME") {
        Some(Some(_)) if before => LineHome::Unread(
            "the line uses `~` or `$HOME` both before and after it assigns HOME".to_string(),
        ),
        Some(Some(value)) if Path::new(value).is_absolute() => {
            LineHome::Literal(PathBuf::from(value))
        }
        Some(Some(_)) => LineHome::Unread("HOME is assigned a relative path".to_string()),
        _ => LineHome::Unread("HOME is assigned a value the guard does not read".to_string()),
    }
}

/// Whether `text` uses the home: a `~` that starts a word, an assignment
/// value or a path list entry, or a `$HOME` or `${HOME...}` expansion.
/// Quoted text counts, because a body a command runs later (a `bash -c`
/// string, a Git callback) expands it with the home it inherits.
fn uses_home(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (at, ch) in chars.iter().enumerate() {
        match ch {
            '~' => {
                if at == 0 || " \t\n=:'\"(`;|&<>{,".contains(chars[at - 1]) {
                    return true;
                }
            }
            '$' => {
                let rest: String = chars[at + 1..].iter().take(6).collect();
                if rest.starts_with("{HOME") {
                    return true;
                }
                if let Some(tail) = rest.strip_prefix("HOME") {
                    if !tail
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// `HOME=...` in front of a command, directly or through `env`.
fn prefix_home(words: &[String]) -> bool {
    let assignment = |word: &str| {
        word.split_once('=')
            .is_some_and(|(name, _)| super::is_identifier(name))
    };
    let leading = words.iter().take_while(|word| assignment(word)).count();
    if leading == words.len() {
        return false;
    }
    if words[..leading]
        .iter()
        .any(|word| word.starts_with("HOME="))
    {
        return true;
    }
    words[leading..]
        .iter()
        .position(|word| super::basename(word) == "env")
        .is_some_and(|at| {
            words[leading + at + 1..]
                .iter()
                .any(|word| word.starts_with("HOME="))
        })
}

/// A HOME change the reader cannot prove to be one literal value: an
/// append, a sourced file or an `eval` of text it does not read (either can
/// run any assignment), a nameref declaration, or a `HOME=` word in any form
/// other than a plain statement, a declaring builtin or a prefix, such as
/// one behind a wrapper the reader does not unwrap.
fn unproven_change(words: &[String]) -> bool {
    if words.iter().any(|word| word.starts_with("HOME+=")) {
        return true;
    }
    let unwrapped = builtin_words(words);
    let Some((first, rest)) = unwrapped.split_first() else {
        return words.iter().any(|word| word.starts_with("HOME="));
    };
    match first.as_str() {
        "source" | "." => true,
        "eval" => rest.iter().any(|word| expands(word)),
        "declare" | "typeset" | "local" => rest
            .iter()
            .take_while(|word| word.starts_with('-'))
            .any(|flag| flag.contains('n')),
        "export" | "readonly" => false,
        _ => {
            let assignment = |word: &String| {
                word.split_once('=')
                    .is_some_and(|(name, _)| super::is_identifier(name))
            };
            let leading = unwrapped.iter().take_while(|word| assignment(word)).count();
            let statement = leading == unwrapped.len();
            // A plain statement or a prefix is read elsewhere; a `HOME=`
            // word anywhere else is an assignment through a form the
            // reader does not unwrap, or data it cannot tell from one.
            !statement
                && unwrapped[leading..]
                    .iter()
                    .skip(1)
                    .any(|word| word.starts_with("HOME="))
                && !matches!(first.as_str(), "env" | "echo" | "printf")
                && (expands(first)
                    || unwrapped.len() != words.len()
                    || matches!(first.as_str(), "command" | "builtin"))
        }
    }
}

/// A builtin that stores a value it reads into HOME.
fn reads_into_home(words: &[String]) -> bool {
    let words = builtin_words(words);
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    match first.as_str() {
        "read" | "unset" | "getopts" | "mapfile" | "readarray" => {
            rest.iter().any(|word| word == "HOME")
        }
        "printf" => {
            rest.windows(2)
                .any(|pair| pair[0] == "-v" && pair[1] == "HOME")
                || rest.iter().any(|word| word == "-vHOME")
        }
        _ => false,
    }
}

thread_local! {
    static ASSIGNED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static TILDE: Cell<bool> = const { Cell::new(false) };
}

/// While alive, [`home`] answers with the line's literal home on this
/// thread. A nested body that assigns no HOME keeps the enclosing line's,
/// because it runs with the environment that line gave it.
pub(crate) struct Scope {
    previous: Option<PathBuf>,
    tilde: bool,
}

impl Scope {
    #[must_use]
    pub(crate) fn enter(command: &str, line: &LineHome) -> Self {
        let previous = ASSIGNED.with(|home| home.borrow().clone());
        if let LineHome::Literal(path) = line {
            ASSIGNED.with(|home| *home.borrow_mut() = Some(path.clone()));
        }
        let tilde = TILDE.with(|tilde| tilde.replace(!quoted_tilde(command)));
        Self { previous, tilde }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ASSIGNED.with(|home| *home.borrow_mut() = previous);
        TILDE.with(|tilde| tilde.set(self.tilde));
    }
}

/// Whether a quoted or escaped `~` stands where a word could start. Quote
/// removal makes `'~/x'` read like `~/x`, but the shell keeps the first
/// as a directory named `~`.
fn quoted_tilde(command: &str) -> bool {
    ["'~", "\"~", "\\~"]
        .iter()
        .any(|form| command.contains(form))
}

/// Whether a quote-stripped word's leading `~` is the home: inside a
/// [`Scope`] whose line has no quoted or escaped `~`. Readers that cannot
/// tell a quoted `~` from the home keep such a word unread otherwise.
pub(crate) fn tilde_resolves() -> bool {
    TILDE.with(Cell::get)
}

/// The home `~` and `$HOME` name for the line being judged: its literal
/// assignment inside a [`Scope`], else the guard's own.
pub(crate) fn home() -> Option<PathBuf> {
    ASSIGNED
        .with(|home| home.borrow().clone())
        .or_else(crate::portable_path::user_home)
}

/// The line's literal home inside a [`Scope`], when it assigns one.
pub(crate) fn assigned() -> Option<PathBuf> {
    ASSIGNED.with(|home| home.borrow().clone())
}

/// The refusal for a line whose home the guard cannot read.
pub(crate) fn unread_violation(line: &LineHome, level: PolicyLevel) -> Option<Violation> {
    let LineHome::Unread(why) = line else {
        return None;
    };
    level.is_active().then(|| {
        hook_integrity_violation(
            level,
            format!(
                "{why}, and the line then uses `~` or `$HOME`, so the guard cannot tell which paths those words name; assign HOME one literal absolute path in its own statement, or spell the path out"
            ),
        )
    })
}

/// Exec-guard reads no shell writes of its own, so a line that reassigns
/// HOME refuses there when a command that uses the home writes an
/// enforcement path under that home, as git-guard's writer checks judge it.
/// A read-only use, such as a Git pathspec, passes.
pub(crate) fn named_enforcement(
    command: &str,
    cwd: &Path,
    level: PolicyLevel,
) -> Option<Violation> {
    if !level.is_active() {
        return None;
    }
    let home = assigned()?;
    let segments = expand_shell_commands(command).segments;
    let run = run_dirs(&segments, cwd);
    for segment in &segments {
        if !uses_home(segment) {
            continue;
        }
        let mut words = shell_tokens(segment);
        strip_reserved_words(&mut words);
        if let Some(found) =
            integrity_write_in_run(&words, &redirect_writes(segment), level, &run, cwd, command)
        {
            return Some(hook_integrity_violation(
                level,
                format!(
                    "the line sets HOME to `{}`, and {}",
                    home.display(),
                    found.message
                ),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on this platform, spelled as the tests write `/r`.
    fn abs(line: &str) -> String {
        if cfg!(windows) {
            line.replace("=/r", "=C:/r")
                .replace("=/a", "=C:/a")
                .replace("=/b", "=C:/b")
                .replace("=/zz", "=C:/zz")
        } else {
            line.to_string()
        }
    }

    #[test]
    fn literal_statement_and_export_forms_name_the_line_home() {
        for line in [
            "HOME=/r; git clone u ~/.claude/x",
            "export HOME=/r; git clone u ~/.claude/x",
            "HOME=/r && rm -rf ~/.claude",
            "HOME=/r; printf x > \"$HOME/.claude/settings.json\"",
            "command export HOME=/r; printf x > ~/.claude/settings.json",
            "builtin export HOME=/r; git clone u ~/.claude/x",
            "command -p declare -x HOME=/r; git init ~/.claude/x",
            "builtin command export HOME=/r; rm -rf ~/.claude",
        ] {
            let line = abs(line);
            let root = if cfg!(windows) { "C:/r" } else { "/r" };
            assert_eq!(
                read(&line),
                LineHome::Literal(PathBuf::from(root)),
                "{line}"
            );
        }
    }

    #[test]
    fn ordinary_lines_inherit_the_home() {
        for line in [
            "git clone u ~/scratch/y",
            "PATH=\"$HOME/bin:$PATH\" git status",
            "git log HEAD~1 --format='%h~%s'",
            "HOME=/r; git status",
            "cd ~ && HOME=/r",
            "echo HOME=x; ls ~",
            "export -n HOME; ls ~",
            "command -v git && git clone u ~/scratch/y",
        ] {
            let line = abs(line);
            assert_eq!(read(&line), LineHome::Inherited, "{line}");
        }
    }

    #[test]
    fn unread_assignments_before_a_use_are_unread() {
        for line in [
            "HOME=$X; git clone u ~/.claude/x",
            "HOME=r; rm -rf ~/.claude",
            "HOME=/zz PATH=\"$HOME/bin:$PATH\" git status",
            "env HOME=/zz sh -c 'rm -rf ~/.claude'",
            "read HOME; rm -rf ~/.claude",
            "unset HOME; rm -rf ~/.claude",
            "for HOME in /a /b; do rm -rf ~/.claude; done",
            "ls ~; HOME=/r; rm -rf ~/.claude",
            "HOME=/a; HOME=/b; rm -rf ~/.claude",
            "HOME+=/x; rm -rf ~/.claude",
            "HOME=/r; HOME+=/x; rm -rf ~/.claude",
            ". ./env.sh; rm -rf ~/.claude",
            "source env.sh && rm -rf ~/.claude",
            "eval \"$SETUP\"; rm -rf ~/.claude",
            "declare -n H=HOME; H=/r; rm -rf ~/.claude",
            "command -x export HOME=/r; rm -rf ~/.claude",
            "$WRAP export HOME=/r; rm -rf ~/.claude",
        ] {
            let line = abs(line);
            assert!(matches!(read(&line), LineHome::Unread(_)), "{line}");
        }
    }

    #[test]
    fn a_scope_restores_the_enclosing_home() {
        let outer = Scope::enter("ls", &LineHome::Literal(PathBuf::from("/outer")));
        assert!(tilde_resolves());
        {
            let _inner = Scope::enter("ls '~/x'", &LineHome::Inherited);
            assert_eq!(assigned(), Some(PathBuf::from("/outer")));
            assert!(!tilde_resolves());
            let _literal = Scope::enter("ls", &LineHome::Literal(PathBuf::from("/inner")));
            assert_eq!(assigned(), Some(PathBuf::from("/inner")));
        }
        assert_eq!(assigned(), Some(PathBuf::from("/outer")));
        assert!(tilde_resolves());
        drop(outer);
        assert_eq!(assigned(), None);
        assert!(!tilde_resolves());
    }
}
