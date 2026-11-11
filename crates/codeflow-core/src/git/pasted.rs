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

use super::name::hidden;
use super::GitName;

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
    head: &'static str,
    options: Vec<(&'static str, Option<&'a str>)>,
    operands: Vec<Operand<'a>>,
    trailing: Vec<&'static str>,
    dashed: Dashed,
}

impl<'a> Pasted<'a> {
    /// A command starting with `head`, fixed text such as `git fetch` or
    /// `git remote set-head --auto`, whose verb ends its options at `--`.
    pub fn new(head: &'static str) -> Self {
        Self {
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

    /// The command as one line a POSIX shell runs with exactly these
    /// arguments, or `None` when a value has no printed form that pastes
    /// back as itself.
    #[must_use]
    pub fn render(&self) -> Option<String> {
        let mut words = vec![self.head.to_string()];
        for (flag, value) in &self.options {
            words.push(option(*flag, (*value)?)?);
        }
        let mut operands = Vec::with_capacity(self.operands.len());
        let mut dashed = false;
        for operand in &self.operands {
            match operand {
                Operand::Value(value) => {
                    let value = (*value)?;
                    dashed |= value.starts_with('-');
                    operands.push(word(value)?);
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

/// `value` as one POSIX shell word, or `None` when it holds a character a
/// terminal does not show as itself.
#[must_use]
pub fn word(value: &str) -> Option<String> {
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
/// gives it.
#[must_use]
pub fn option(flag: &'static str, value: &str) -> Option<String> {
    let shown = word(value)?;
    Some(if value.starts_with('-') {
        format!("{flag}={shown}")
    } else {
        format!("{flag} {shown}")
    })
}

/// A whole argument vector built by codeflow (`program` then `args`, each
/// already one argument as the command receives it), as one line a POSIX
/// shell runs with exactly those arguments, or `None` as [`word`] gives it
/// for any of them. For a rerun line that repeats an invocation as made.
#[must_use]
pub fn argv(program: &'static str, args: &[&str]) -> Option<String> {
    let mut words = vec![program.to_string()];
    for arg in args {
        words.push(word(arg)?);
    }
    Some(words.join(" "))
}

/// What a step says in place of a command with no printed form: the reader
/// runs it with a tool that takes the name as it is, never a pasted line.
pub const NOT_PRINTED: &str = "with a git client: a name it needs holds a character a terminal does not show as itself, so no command is printed to paste";

/// `command` rendered in backticks, or [`NOT_PRINTED`] when it has no
/// printed form that pastes back as the values it names.
#[must_use]
pub fn step(command: &Pasted<'_>) -> String {
    command
        .render()
        .map_or_else(|| NOT_PRINTED.to_string(), |line| format!("`{line}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            word("refs/remotes/origin/main").as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(word("origin").as_deref(), Some("origin"));
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
            let printed = Pasted::new("git fetch").value(&value).render().unwrap();
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
        assert_eq!(word(r"a\b").as_deref(), Some(r"'a\b'"));
        for hostile in ["a\u{200b}b", "a\u{202e}b", "a\u{1b}[31mb", "a\nb", "a\tb"] {
            assert_eq!(word(hostile), None, "{hostile:?}");
            assert_eq!(Pasted::new("git fetch").value(hostile).render(), None);
            assert_eq!(option("--into", hostile), None);
        }
        let invalid = GitName::from_bytes(b"caf\xe9");
        assert_eq!(Pasted::new("git switch").name(&invalid).render(), None);
        let valid = GitName::from_text("feat/x");
        assert_eq!(
            Pasted::new("git switch").name(&valid).render().as_deref(),
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
            let mut command = Pasted::new("git fetch --prune");
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
            ["fetch", "--prune", "--", "--upload-pack=touch_pwned", "main"]
        );
        assert_eq!(
            shell_argv(&fetch(&["origin", "-o$(id)"])),
            ["fetch", "--prune", "--", "origin", "-o$(id)"]
        );
        let renamed = Pasted::new("git branch -m")
            .value("-y")
            .placeholder("<new-name>")
            .render();
        assert_eq!(renamed.as_deref(), Some("git branch -m -- -y <new-name>"));
        let set_head = |remote: &str| {
            Pasted::new("git remote set-head")
                .value(remote)
                .trailing_flag("-a")
                .render()
                .unwrap()
        };
        assert_eq!(set_head("origin"), "git remote set-head origin -a");
        assert_eq!(set_head("-x"), "git remote set-head -a -- -x");
        assert_eq!(
            Pasted::new("git reset --keep")
                .without_end_of_options()
                .value("-x")
                .render(),
            None
        );
        assert_eq!(
            Pasted::new("git reset --keep")
                .without_end_of_options()
                .value("refs/remotes/origin/x$(id)")
                .render()
                .as_deref(),
            Some("git reset --keep 'refs/remotes/origin/x$(id)'")
        );
        let into = Pasted::new("codeflow work start")
            .option("--into", "-x")
            .value("TSK-1")
            .render()
            .unwrap();
        assert_eq!(into, "codeflow work start --into=-x TSK-1");
        assert_eq!(
            shell_argv(&into),
            ["work", "start", "--into=-x", "TSK-1"]
        );
        let rerun = argv(
            "codeflow",
            &["ci", "--branch", "feat/x$(id)", "--into=-x", "--pushed-ref", "refs/heads/a b"],
        )
        .unwrap();
        assert_eq!(
            shell_argv(&rerun),
            ["ci", "--branch", "feat/x$(id)", "--into=-x", "--pushed-ref", "refs/heads/a b"]
        );
        assert_eq!(argv("codeflow", &["ci", "--branch", "a\u{202e}b"]), None);
        assert_eq!(
            option("--line-tip", "a b").as_deref(),
            Some("--line-tip 'a b'")
        );
        assert_eq!(
            shell_argv(&format!("codeflow ci {}", option("--into", "o$(id)").unwrap())),
            ["ci", "--into", "o$(id)"]
        );
    }
}
