//! The one renderer for a shell command a message prints for a reader to
//! paste (TSK-416 review).
//!
//! A remedy, hint or refusal that names a command built from a ref, remote,
//! path or other value it read renders it here, from the raw value, never
//! from its display form. Each value becomes one POSIX shell word: unchanged
//! when every character is one a shell reads literally (letters, digits and
//! `_-./:@%+=,`), else in single quotes, each `'` written `'\''`. An operand
//! that starts with `-` follows `--`, where the verb ends its options there;
//! where `--` changes what the verb does, the command is not printed.
//!
//! A value that holds a character a terminal does not show as itself (a
//! control, format or separator character, see [`super::name`]), or a name
//! that is not valid UTF-8, has no printed form that pastes back as the same
//! value, so [`Pasted::render`] returns `None` and the caller gives a
//! recovery that needs no pasted command.
//!
//! Every renderer here takes the repository the command is for, and a
//! value that holds the name of a remote messages withhold
//! ([`crate::security::git::word_shown`], judged on the raw value before
//! quoting) is not printed either (TSK-416 review): quoting can split such a
//! name, so no check of the rendered text could see it. A command a message
//! prints is built here, so no caller can print one that skips this check.

use std::path::Path;

use super::name::hidden;
use super::GitName;
use crate::security::git::WithheldRemotes;

/// How the verb reads an operand that starts with `-`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dashed {
    /// The verb ends its options at `--`, printed before the operands.
    EndOptions,
    /// `--` changes what the verb does (`git reset` reads paths after it,
    /// `git worktree add` adds a detached worktree), so such an operand has
    /// no safe printed form.
    Refuse,
}

#[derive(Clone, Copy, Debug)]
enum Operand<'a> {
    /// A value read from git, a remote, a path or a flag: `None` when it is
    /// not valid UTF-8.
    Value(Option<&'a str>),
    /// Text the reader replaces, such as `<new-name>`, written by codeflow.
    Placeholder(&'static str),
}

/// A command to print for a reader to paste: the fixed head codeflow writes
/// (program, verb and flags), then option values and operands it read.
#[derive(Clone, Debug)]
#[must_use]
pub struct Pasted<'a> {
    root: &'a Path,
    head: &'static str,
    options: Vec<(&'static str, Option<&'a str>)>,
    operands: Vec<Operand<'a>>,
    trailing: Vec<&'static str>,
    dashed: Dashed,
}

impl<'a> Pasted<'a> {
    /// A command for the repository at `root` starting with `head`, fixed
    /// text such as `git fetch` or `git remote set-head --auto`, whose verb
    /// ends its options at `--`. `root` decides which remote names a value
    /// may not carry.
    pub fn new(root: &'a Path, head: &'static str) -> Self {
        Self {
            root,
            head,
            options: Vec::new(),
            operands: Vec::new(),
            trailing: Vec::new(),
            dashed: Dashed::EndOptions,
        }
    }

    /// A fixed flag printed after the operands, as a familiar form reads
    /// (`git remote set-head origin -a`), and before `--` when an operand
    /// starts with `-`, since git reads every word after `--` as an operand.
    pub fn trailing_flag(mut self, flag: &'static str) -> Self {
        self.trailing.push(flag);
        self
    }

    /// The verb reads its operands differently after `--`, so an operand
    /// that starts with `-` leaves the command unprinted.
    pub fn without_end_of_options(mut self) -> Self {
        self.dashed = Dashed::Refuse;
        self
    }

    /// An option with its value, written as [`option`] writes it.
    pub fn option(mut self, flag: &'static str, value: &'a str) -> Self {
        self.options.push((flag, Some(value)));
        self
    }

    /// An operand read as text.
    pub fn value(mut self, value: &'a str) -> Self {
        self.operands.push(Operand::Value(Some(value)));
        self
    }

    /// An operand read as a git name, from its exact bytes.
    pub fn name(mut self, name: &'a GitName) -> Self {
        self.operands.push(Operand::Value(name.rule_text().ok()));
        self
    }

    /// A placeholder operand the reader replaces, such as `<new-name>`.
    pub fn placeholder(mut self, text: &'static str) -> Self {
        self.operands.push(Operand::Placeholder(text));
        self
    }

    /// The raw values this command carries: option values and operands,
    /// `None` for a value that is not valid UTF-8.
    fn values(&self) -> impl Iterator<Item = Option<&'a str>> + '_ {
        self.options
            .iter()
            .map(|(_, value)| *value)
            .chain(self.operands.iter().filter_map(|operand| match operand {
                Operand::Value(value) => Some(*value),
                Operand::Placeholder(_) => None,
            }))
    }

    /// Whether a raw value holds the name of a remote messages withhold, or
    /// the remotes cannot be read, so every value is held back. The words
    /// joined as raw text and the rendered line are judged too, since a
    /// name holding a space can span two words that each look clean
    /// (TSK-416 review).
    fn withheld(&self) -> bool {
        let remotes = WithheldRemotes::read(self.root);
        if self
            .values()
            .flatten()
            .any(|value| remotes.scrub(value) != value)
        {
            return true;
        }
        let mut raw: Vec<&str> = vec![self.head];
        for (flag, value) in &self.options {
            raw.push(*flag);
            raw.extend(*value);
        }
        for operand in &self.operands {
            match operand {
                Operand::Value(value) => raw.extend(*value),
                Operand::Placeholder(text) => raw.push(*text),
            }
        }
        raw.extend(self.trailing.iter().copied());
        let raw = raw.join(" ");
        remotes.scrub(&raw) != raw || self.line().is_some_and(|line| remotes.scrub(&line) != line)
    }

    /// The command as one line a POSIX shell runs with exactly these
    /// arguments, or `None` when a value has no printed form that pastes
    /// back as itself or holds a withheld remote name.
    #[must_use]
    pub fn render(&self) -> Option<String> {
        if self.withheld() {
            return None;
        }
        self.line()
    }

    /// [`Pasted::render`] without the remote-name check.
    fn line(&self) -> Option<String> {
        let mut words = vec![self.head.to_string()];
        for (flag, value) in &self.options {
            words.push(rendered_option(flag, (*value)?)?);
        }
        let mut operands = Vec::with_capacity(self.operands.len());
        let mut dashed = false;
        for operand in &self.operands {
            match operand {
                Operand::Value(value) => {
                    let value = (*value)?;
                    dashed |= value.starts_with('-');
                    operands.push(shell_word(value)?);
                }
                Operand::Placeholder(text) => operands.push((*text).to_string()),
            }
        }
        let trailing = self.trailing.iter().map(|flag| (*flag).to_string());
        if dashed {
            match self.dashed {
                Dashed::EndOptions => {
                    words.extend(trailing);
                    words.push("--".to_string());
                    words.extend(operands);
                }
                Dashed::Refuse => return None,
            }
        } else {
            words.extend(operands);
            words.extend(trailing);
        }
        Some(words.join(" "))
    }
}

/// `value`, a word of a command for the repository at `root`, as one POSIX
/// shell word, or `None` when it holds a character a terminal does not show
/// as itself or the name of a remote messages withhold.
#[must_use]
pub fn word(root: &Path, value: &str) -> Option<String> {
    if !crate::security::git::word_shown(root, value) {
        return None;
    }
    shell_word(value)
}

/// [`word`] without the remote-name check, for the renderers here.
fn shell_word(value: &str) -> Option<String> {
    if value.chars().any(hidden) {
        return None;
    }
    let literal = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',')
        });
    Some(if literal {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    })
}

/// One option of a printed command, for text that names the option a
/// reader passes: `flag value`, or `flag=value` when the value starts with
/// `-`, so the parser keeps it as the option's value; `None` as [`word`]
/// gives it for the repository at `root`.
#[must_use]
pub fn option(root: &Path, flag: &'static str, value: &str) -> Option<String> {
    if !crate::security::git::word_shown(root, value)
        || !crate::security::git::word_shown(root, &format!("{flag} {value}"))
    {
        return None;
    }
    rendered_option(flag, value).filter(|shown| crate::security::git::word_shown(root, shown))
}

/// [`option`] without the remote-name check, for the renderers here.
fn rendered_option(flag: &'static str, value: &str) -> Option<String> {
    let shown = shell_word(value)?;
    Some(if value.starts_with('-') {
        format!("{flag}={shown}")
    } else {
        format!("{flag} {shown}")
    })
}

/// A whole argument vector built by codeflow (`program` then `args`, each
/// already one argument as the command receives it), as one line a POSIX
/// shell runs with exactly those arguments, or `None` as [`word`] gives it
/// for any of them in the repository at `root`. For a rerun line that
/// repeats an invocation as made.
#[must_use]
pub fn argv(root: &Path, program: &'static str, args: &[&str]) -> Option<String> {
    let mut words = vec![program.to_string()];
    for arg in args {
        words.push(word(root, arg)?);
    }
    // A withheld name holding a space can span two clean words, in the raw
    // arguments or in the line (TSK-416 review).
    let raw = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let line = words.join(" ");
    (crate::security::git::word_shown(root, &raw) && crate::security::git::word_shown(root, &line))
        .then_some(line)
}

/// What a step says in place of a command with no printed form because a
/// value holds a character a terminal does not show as itself: the reader
/// runs it with a tool that takes the name as it is, never a pasted line.
pub const NOT_PRINTED: &str = "with a git client: a name it needs holds a character a terminal does not show as itself, so no command is printed to paste";

/// What a step says in place of a command with no printed form because an
/// operand starts with `-` and the verb reads its operands differently
/// after `--` (TSK-416 review round 3).
pub const NOT_PRINTED_DASHED: &str = "with a git client: a name it needs starts with `-`, which this command reads as an option, and after `--` as something other than that name, so no command is printed to paste";

/// What a step says in place of a command with no printed form because a
/// value holds the name of a remote messages withhold, before
/// [`crate::security::git::WITHHELD_REMOTE_STEP`] (TSK-416 review).
pub const NOT_PRINTED_WITHHELD: &str =
    "with no command printed, since a name it needs holds the name of a remote codeflow withholds";

impl Pasted<'_> {
    /// Why [`Pasted::render`] gives `None`, as the text a step prints in
    /// the command's place: [`NOT_PRINTED_WITHHELD`] for a value holding a
    /// withheld remote name, first, since no form of such a command may be
    /// printed; [`NOT_PRINTED`] for a value with a hidden character or
    /// invalid UTF-8, which has no printed form under any verb; else
    /// [`NOT_PRINTED_DASHED`] for a dashed operand the verb cannot take
    /// after `--`. `None` when the command renders.
    #[must_use]
    pub fn unprinted(&self) -> Option<&'static str> {
        if self.withheld() {
            return Some(NOT_PRINTED_WITHHELD);
        }
        if self
            .values()
            .any(|value| value.is_none_or(|value| value.chars().any(hidden)))
        {
            return Some(NOT_PRINTED);
        }
        let dashed = self.operands.iter().any(
            |operand| matches!(operand, Operand::Value(Some(value)) if value.starts_with('-')),
        );
        (dashed && self.dashed == Dashed::Refuse).then_some(NOT_PRINTED_DASHED)
    }
}

/// `command` rendered in backticks, or the reason it has no printed form
/// that pastes back as the values it names ([`Pasted::unprinted`]); for a
/// withheld remote name, that reason and the steps for such a remote.
#[must_use]
pub fn step(command: &Pasted<'_>) -> String {
    if let Some(line) = command.render() {
        return format!("`{line}`");
    }
    match command.unprinted() {
        Some(NOT_PRINTED_WITHHELD) => format!(
            "{NOT_PRINTED_WITHHELD}; {}",
            crate::security::git::WITHHELD_REMOTE_STEP
        ),
        reason => reason.unwrap_or(NOT_PRINTED).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with `remotes` configured, each with a plain URL.
    fn repo_with(remotes: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = crate::git::command()
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        git(&["init", "-q"]);
        for remote in remotes {
            git(&["config", &format!("remote.{remote}.url"), "/nowhere"]);
        }
        dir
    }

    /// A repository with no remotes, shared by the tests here that judge
    /// how a value is quoted.
    fn root() -> &'static Path {
        static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        ROOT.get_or_init(|| repo_with(&[])).path()
    }

    /// TSK-416 review: the renderer judges each raw value against the
    /// repository's withheld remote names before quoting, so a name holding
    /// `'`, which quoting splits, is caught. Every renderer refuses it, the
    /// step gives the steps for a withheld remote, a shown remote still
    /// prints, and a root whose remotes cannot be read prints nothing.
    #[test]
    fn a_value_holding_a_withheld_remote_name_is_never_printed() {
        let withheld = "user:p'ass@host";
        let repo = repo_with(&["origin", withheld, "p'ass"]);
        let root = repo.path();
        let in_branch = format!("refs/remotes/origin/{withheld}");
        for value in [withheld, in_branch.as_str(), "origin/p'ass"] {
            let fetch = Pasted::new(root, "git fetch").value("origin").value(value);
            assert_eq!(fetch.render(), None, "{value}");
            assert_eq!(fetch.unprinted(), Some(NOT_PRINTED_WITHHELD), "{value}");
            let printed = step(&fetch);
            assert!(!printed.contains("git fetch"), "{printed}");
            assert!(!printed.contains("p'"), "{printed}");
            assert!(
                printed.contains(crate::security::git::WITHHELD_REMOTE_STEP),
                "{printed}"
            );
            let into = Pasted::new(root, "codeflow ci").option("--into", value);
            assert_eq!(into.render(), None, "{value}");
            assert_eq!(word(root, value), None, "{value}");
            assert_eq!(option(root, "--into", value), None, "{value}");
            assert_eq!(argv(root, "codeflow", &["ci", "--into", value]), None);
        }
        assert_eq!(
            Pasted::new(root, "git fetch")
                .value("origin")
                .value("main")
                .render()
                .as_deref(),
            Some("git fetch origin main")
        );
        assert_eq!(word(root, "origin").as_deref(), Some("origin"));
        let unread = tempfile::tempdir().unwrap();
        assert_eq!(
            Pasted::new(unread.path(), "git fetch")
                .value("origin")
                .render(),
            None
        );
        assert_eq!(word(unread.path(), "main"), None);
    }

    /// TSK-416 review: a withheld name holding a space can span two words
    /// that each hold none of it, such as `origin` and `user:s3cret@host`
    /// for the remote `origin user:s3cret@host`. Every renderer judges the
    /// joined words and the line, so no form of such a command is printed;
    /// the words apart, or in another order, still print.
    #[test]
    fn a_withheld_name_spanning_two_words_is_never_printed() {
        let spanning = "origin user:s3cret@host";
        let repo = repo_with(&["origin", spanning]);
        let root = repo.path();
        let fetch = Pasted::new(root, "git fetch")
            .value("origin")
            .value("user:s3cret@host");
        assert_eq!(fetch.render(), None);
        assert_eq!(fetch.unprinted(), Some(NOT_PRINTED_WITHHELD));
        let printed = step(&fetch);
        assert!(!printed.contains("s3cret"), "{printed}");
        assert!(
            printed.contains(crate::security::git::WITHHELD_REMOTE_STEP),
            "{printed}"
        );
        // The fetch hint `task status`, acceptance and `ci` print.
        let hint = crate::workgraph::work_start::fetch_origin(root, "user:s3cret@host");
        assert!(!hint.contains("s3cret"), "{hint}");
        assert!(hint.contains(NOT_PRINTED_WITHHELD), "{hint}");
        // A span across a flag and its value, or two arguments.
        let into = Pasted::new(root, "codeflow ci").option("origin", "user:s3cret@host");
        assert_eq!(into.render(), None);
        assert_eq!(option(root, "origin", "user:s3cret@host"), None);
        assert_eq!(
            argv(root, "codeflow", &["origin", "user:s3cret@host"]),
            None
        );
        // Each word alone, or the words apart, still prints.
        assert_eq!(
            word(root, "user:s3cret@host").as_deref(),
            Some("user:s3cret@host")
        );
        assert_eq!(
            Pasted::new(root, "git fetch")
                .value("user:s3cret@host")
                .value("origin")
                .render()
                .as_deref(),
            Some("git fetch user:s3cret@host origin")
        );
    }

    /// The arguments `sh` hands a program named `git` when it runs
    /// `command`: a shell function stands in for git and prints each one.
    #[cfg(unix)]
    fn shell_argv(command: &str) -> Vec<String> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "git() {{ printf '%s\\0' \"$@\"; }}; codeflow() {{ git \"$@\"; }}; {command}"
            ))
            .output()
            .unwrap();
        assert!(out.status.success(), "{command}");
        let text = String::from_utf8(out.stdout).unwrap();
        text.split_terminator('\0').map(str::to_string).collect()
    }

    /// TSK-416 review: each value reaches a shell as one literal argument,
    /// whatever it holds, and an ordinary name prints unchanged.
    #[test]
    #[cfg(unix)]
    fn a_value_reaches_the_shell_as_one_literal_argument() {
        assert_eq!(
            word(root(), "refs/remotes/origin/main").as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(word(root(), "origin").as_deref(), Some("origin"));
        let scratch = tempfile::tempdir().unwrap();
        let marker = scratch.path().join("ran");
        let marker = marker.to_str().unwrap();
        for value in [
            format!("o$(touch {marker})"),
            format!("main;touch {marker}"),
            format!("x`touch {marker}`"),
            "/srv/a repo/x.git".to_string(),
            "it's".to_string(),
            "~/x".to_string(),
            r"C:\work\a\\b".to_string(),
            "caf\u{e9}".to_string(),
            String::new(),
        ] {
            let printed = Pasted::new(root(), "git fetch")
                .value(&value)
                .render()
                .unwrap();
            assert_eq!(shell_argv(&printed), ["fetch", value.as_str()], "{printed}");
            assert!(!std::path::Path::new(marker).exists(), "{value} ran");
        }
    }

    /// TSK-416 review round 2, finding 3: the printed form is the value
    /// itself, never its display form, so a backslash is not doubled; a
    /// value a terminal cannot show as itself is not printed at all, since
    /// its escaped form would paste as a different name.
    #[test]
    fn the_printed_value_is_the_raw_value_or_nothing() {
        assert_eq!(word(root(), r"a\b").as_deref(), Some(r"'a\b'"));
        for hostile in ["a\u{200b}b", "a\u{202e}b", "a\u{1b}[31mb", "a\nb", "a\tb"] {
            assert_eq!(word(root(), hostile), None, "{hostile:?}");
            assert_eq!(
                Pasted::new(root(), "git fetch").value(hostile).render(),
                None
            );
            assert_eq!(option(root(), "--into", hostile), None);
        }
        let invalid = GitName::from_bytes(b"caf\xe9");
        assert_eq!(
            Pasted::new(root(), "git switch").name(&invalid).render(),
            None
        );
        let valid = GitName::from_text("feat/x");
        assert_eq!(
            Pasted::new(root(), "git switch")
                .name(&valid)
                .render()
                .as_deref(),
            Some("git switch feat/x")
        );
    }

    /// TSK-416 review: an operand that starts with `-` follows `--` where
    /// the verb ends its options there, and is never printed where `--`
    /// changes the verb; an option value keeps its `-` after `=`.
    #[test]
    #[cfg(unix)]
    fn a_dashed_operand_never_reaches_an_option_position() {
        let fetch = |values: &[&str]| {
            let mut command = Pasted::new(root(), "git fetch --prune");
            for value in values {
                command = command.value(value);
            }
            command.render().unwrap()
        };
        assert_eq!(fetch(&["origin"]), "git fetch --prune origin");
        assert_eq!(fetch(&["origin", "main"]), "git fetch --prune origin main");
        assert_eq!(fetch(&["-x"]), "git fetch --prune -- -x");
        assert_eq!(
            shell_argv(&fetch(&["--upload-pack=touch_pwned", "main"])),
            [
                "fetch",
                "--prune",
                "--",
                "--upload-pack=touch_pwned",
                "main"
            ]
        );
        assert_eq!(
            shell_argv(&fetch(&["origin", "-o$(id)"])),
            ["fetch", "--prune", "--", "origin", "-o$(id)"]
        );
        let renamed = Pasted::new(root(), "git branch -m")
            .value("-y")
            .placeholder("<new-name>")
            .render();
        assert_eq!(renamed.as_deref(), Some("git branch -m -- -y <new-name>"));
        let set_head = |remote: &str| {
            Pasted::new(root(), "git remote set-head")
                .value(remote)
                .trailing_flag("-a")
                .render()
                .unwrap()
        };
        assert_eq!(set_head("origin"), "git remote set-head origin -a");
        assert_eq!(set_head("-x"), "git remote set-head -a -- -x");
        assert_eq!(
            Pasted::new(root(), "git reset --keep")
                .without_end_of_options()
                .value("-x")
                .render(),
            None
        );
        assert_eq!(
            Pasted::new(root(), "git reset --keep")
                .without_end_of_options()
                .value("refs/remotes/origin/x$(id)")
                .render()
                .as_deref(),
            Some("git reset --keep 'refs/remotes/origin/x$(id)'")
        );
        let into = Pasted::new(root(), "codeflow work start")
            .option("--into", "-x")
            .value("TSK-1")
            .render()
            .unwrap();
        assert_eq!(into, "codeflow work start --into=-x TSK-1");
        assert_eq!(shell_argv(&into), ["work", "start", "--into=-x", "TSK-1"]);
        let rerun = argv(
            root(),
            "codeflow",
            &[
                "ci",
                "--branch",
                "feat/x$(id)",
                "--into=-x",
                "--pushed-ref",
                "refs/heads/a b",
            ],
        )
        .unwrap();
        assert_eq!(
            shell_argv(&rerun),
            [
                "ci",
                "--branch",
                "feat/x$(id)",
                "--into=-x",
                "--pushed-ref",
                "refs/heads/a b"
            ]
        );
        assert_eq!(
            argv(root(), "codeflow", &["ci", "--branch", "a\u{202e}b"]),
            None
        );
        assert_eq!(
            option(root(), "--line-tip", "a b").as_deref(),
            Some("--line-tip 'a b'")
        );
        assert_eq!(
            shell_argv(&format!(
                "codeflow ci {}",
                option(root(), "--into", "o$(id)").unwrap()
            )),
            ["ci", "--into", "o$(id)"]
        );
    }

    /// TSK-416 review round 3, minor finding: a step with no printed form
    /// says why. A dashed operand the verb cannot take after `--` names the
    /// dash; a hidden character names that, and wins when both hold, since
    /// such a value has no printed form under any verb.
    #[test]
    fn a_step_with_no_printed_form_names_its_own_cause() {
        fn reset(value: &str) -> Pasted<'_> {
            Pasted::new(root(), "git reset --keep")
                .value(value)
                .without_end_of_options()
        }
        assert_eq!(step(&reset("-x")), NOT_PRINTED_DASHED);
        assert_eq!(reset("-x").unprinted(), Some(NOT_PRINTED_DASHED));
        assert_eq!(step(&reset("x\u{200b}y")), NOT_PRINTED);
        assert_eq!(step(&reset("-x\u{200b}$(id)")), NOT_PRINTED);
        assert_eq!(reset("x").unprinted(), None);
        // A verb that ends its options at `--` renders a dashed operand.
        let fetch = Pasted::new(root(), "git fetch").value("-x");
        assert_eq!(fetch.unprinted(), None);
        assert_eq!(step(&fetch), "`git fetch -- -x`");
        // A hidden character in an option value, and invalid UTF-8.
        let into = Pasted::new(root(), "codeflow ci").option("--into", "o\u{202e}$(id)");
        assert_eq!(into.render(), None);
        assert_eq!(step(&into), NOT_PRINTED);
        let invalid = GitName::from_bytes(b"a\xffb");
        assert_eq!(
            step(&Pasted::new(root(), "git switch").name(&invalid)),
            NOT_PRINTED
        );
        // Neither reason claims the other cause.
        assert!(!NOT_PRINTED_DASHED.contains("does not show"));
        assert!(!NOT_PRINTED.contains("starts with"));
    }
}
