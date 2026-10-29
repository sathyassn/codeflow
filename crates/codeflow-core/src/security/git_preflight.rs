//! Spellings refused in a line that runs Git before any guard reads a
//! repository or its configuration (TSK-1002 Codex review 10).
//!
//! A share, verbatim or device spelling anywhere in such a line refuses
//! before any walker, alias lookup or configuration read can touch it. A
//! `GIT_*` environment variable the guard has not classified refuses as
//! cannot-verify: unlisted means unknown. A variable whose value is a
//! program Git runs is judged by running that value through the guards as
//! a command of its own (review 11).

/// The `GIT_*` variables a line may set beside a Git call. Each is listed
/// from Git's own documentation (`git help git`, "Environment Variables",
/// Git 2.53) with why it leaves path, pathspec, configuration and alias
/// resolution as the guard reads it:
///
/// - identity (`GIT_AUTHOR_*`, `GIT_COMMITTER_*`): commit metadata only;
/// - output and interaction (`GIT_TERMINAL_PROMPT`, `GIT_FLUSH`,
///   `GIT_PROGRESS_DELAY`, `GIT_MERGE_VERBOSITY`, `GIT_MERGE_AUTOEDIT`,
///   `GIT_ADVICE`, `GIT_OPTIONAL_LOCKS`, `GIT_DIFF_OPTS`,
///   `GIT_HTTP_USER_AGENT`, `GIT_SSH_VARIANT`, `GIT_LFS_SKIP_SMUDGE`);
/// - pathspec forms that only narrow a pattern (`GIT_GLOB_PATHSPECS`) or
///   make it literal (`GIT_NOGLOB_PATHSPECS`, `GIT_LITERAL_PATHSPECS`),
///   which the pattern rule over-refuses at worst;
/// - `GIT_DIR`, accepted only where it names the repository Git would use
///   without it (`interpreter::git_objects`);
/// - the programs Git runs, [`PROGRAM_ENVIRONMENT`], whose values are
///   judged as commands.
///
/// Every other `GIT_*` variable refuses on a line that runs Git, among them
/// the ones that move resolution: `GIT_WORK_TREE`, `GIT_COMMON_DIR`,
/// `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
/// `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_NAMESPACE`,
/// `GIT_CEILING_DIRECTORIES`, `GIT_DISCOVERY_ACROSS_FILESYSTEM`, every
/// `GIT_CONFIG*`, `GIT_TEMPLATE_DIR`, `GIT_ATTR_SOURCE`, `GIT_ATTR_NOSYSTEM`,
/// `GIT_ICASE_PATHSPECS`, `GIT_REPLACE_REF_BASE`, `GIT_NO_REPLACE_OBJECTS`,
/// `GIT_SHALLOW_FILE`, `GIT_REDIRECT_STDIN` and its siblings, `GIT_EXEC_PATH`
/// (where Git finds helper programs the guard never sees), and the
/// `GIT_TRACE*` family, whose value may name a file Git writes.
const CLASSIFIED_ENVIRONMENT: &[&str] = &[
    "GIT_ADVICE",
    "GIT_AUTHOR_DATE",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_NAME",
    "GIT_COMMITTER_DATE",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_DIFF_OPTS",
    "GIT_DIR",
    "GIT_FLUSH",
    "GIT_GLOB_PATHSPECS",
    "GIT_HTTP_USER_AGENT",
    "GIT_LFS_SKIP_SMUDGE",
    "GIT_LITERAL_PATHSPECS",
    "GIT_MERGE_AUTOEDIT",
    "GIT_MERGE_VERBOSITY",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_OPTIONAL_LOCKS",
    "GIT_PROGRESS_DELAY",
    "GIT_SSH_VARIANT",
    "GIT_TERMINAL_PROMPT",
];

/// The `GIT_*` variables whose value is a program Git runs, the environment
/// spellings of `core.pager`, `core.editor`, `sequence.editor`,
/// `core.sshCommand`, `core.askPass` and `diff.external`. Their value is
/// judged as a command on the line (Fable review 13).
const PROGRAM_ENVIRONMENT: &[&str] = &[
    "GIT_ASKPASS",
    "GIT_EDITOR",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
];

/// Shell builtins whose operands set, export or clear variables.
const DECLARERS: &[&str] = &[
    "declare", "env", "export", "local", "readonly", "typeset", "unset",
];

/// Spellings that are verbatim or device paths wherever they appear.
const NAMESPACE_MARKERS: &[&str] = &["\\\\?\\", "\\\\.\\", "\\??\\", "//?/", "//./", "/??/"];

/// Whether the line runs Git: a simple command, nested ones included, whose
/// program is `git` or a `git-*` program.
fn runs_git(command: &str) -> bool {
    crate::hooks::git_guard::expand_commands(command)
        .iter()
        .any(|segment| {
            let words = crate::hooks::git_guard::command_argv(segment);
            let words: Vec<String> = words
                .iter()
                .skip_while(|word| word.contains('=') && !word.starts_with('-'))
                .cloned()
                .collect();
            crate::hooks::git_guard::strip_launchers(&words).is_some_and(|(program, rest)| {
                crate::hooks::git_guard::git_arguments(program, rest).is_some()
            })
        })
}

/// The values a word can carry: the word, the text after its first `=`
/// (`--git-dir=X`, `include.path=X`, `GIT_X=X`), and for a short option its
/// attached value (`-CX`). A short option is a `-` and an ASCII letter or
/// digit, so a message such as `-é` is cut at a character boundary.
fn values(word: &str) -> Vec<&str> {
    let mut out = vec![word];
    if let Some((_, value)) = word.split_once('=') {
        out.push(value);
    }
    let bytes = word.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'-' && bytes[1].is_ascii_alphanumeric() {
        out.push(&word[2..]);
    }
    out
}

/// The variable assignments and declarations of a line that runs Git: for
/// each word that assigns (`NAME=value`) or that a declaring builtin takes
/// as an operand (`export NAME`, `unset NAME`), the name and the value it
/// assigns, if any.
fn assignments(command: &str) -> Vec<(String, Option<String>)> {
    if !runs_git(command) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for segment in crate::hooks::git_guard::expand_commands(command) {
        let words = crate::hooks::git_guard::command_argv(&segment);
        let declares = words
            .first()
            .is_some_and(|word| DECLARERS.contains(&word.as_str()));
        for (at, word) in words.iter().enumerate() {
            if word.starts_with('-') || declares && at == 0 {
                continue;
            }
            // Outside a declaring builtin only a plain name assigns; a
            // declaring builtin's operand may build its name (`"$N=x"`).
            let plain = |name: &str| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            };
            match word.split_once('=') {
                Some((name, value)) if declares || plain(name) => {
                    out.push((name.to_string(), Some(value.to_string())));
                }
                None if declares => out.push((word.clone(), None)),
                _ => {}
            }
        }
    }
    out
}

/// Why a line that runs Git refuses before any lookup: a share, verbatim or
/// device spelling in any word or value, a `-c` setting included.
#[must_use]
pub(crate) fn namespace(command: &str) -> Option<&'static str> {
    if !runs_git(command) {
        return None;
    }
    let marked = NAMESPACE_MARKERS
        .iter()
        .any(|marker| command.contains(marker));
    let shared = || {
        crate::hooks::git_guard::expand_commands(command)
            .iter()
            .flat_map(|segment| crate::hooks::git_guard::command_argv(segment))
            .any(|word| {
                values(&word)
                    .iter()
                    .any(|value| super::outward::remote_or_namespace_operand(value))
            })
    };
    (marked || shared()).then_some(
        "cannot read a Git repository, path or configuration through a share, verbatim or device spelling; use its drive-letter path",
    )
}

/// The refusal [`namespace`] makes, at a fixed level: it protects the
/// guard's own reads, so it comes before every lookup whatever the policy.
#[must_use]
pub(crate) fn namespace_violation(command: &str) -> Option<crate::hooks::Violation> {
    namespace(command).map(|why| {
        crate::hooks::Violation::always_blocking(
            "security.secret_reads",
            format!("cannot verify Git input: {why}"),
            "name the repository or file by its drive-letter path",
        )
    })
}

/// The first variable a line that runs Git sets or declares that the guard
/// cannot classify: a `GIT_*` name [`CLASSIFIED_ENVIRONMENT`] and
/// [`PROGRAM_ENVIRONMENT`] do not list, or a declared name the shell builds
/// (`export "$N=x"`). A name in text, such as a commit message, is not an
/// assignment (Fable review 13).
#[must_use]
pub(crate) fn unclassified_environment(command: &str) -> Option<String> {
    let identifier = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    };
    assignments(command)
        .into_iter()
        .map(|(name, _)| name)
        .find(|name| {
            if !identifier(name) {
                return name.contains(['$', '`']);
            }
            name.starts_with("GIT_")
                && !CLASSIFIED_ENVIRONMENT.contains(&name.as_str())
                && !PROGRAM_ENVIRONMENT.contains(&name.as_str())
        })
}

/// The values a line that runs Git gives the variables naming a program Git
/// runs ([`PROGRAM_ENVIRONMENT`]), to be judged as commands.
#[must_use]
pub(crate) fn program_values(command: &str) -> Vec<String> {
    assignments(command)
        .into_iter()
        .filter(|(name, _)| PROGRAM_ENVIRONMENT.contains(&name.as_str()))
        .filter_map(|(_, value)| value)
        .filter(|value| !value.is_empty())
        .collect()
}

/// The values a line that runs Git gives `GIT_DIR`, or `None` in place of
/// one it declares without a value the line shows (`export GIT_DIR`).
#[must_use]
pub(crate) fn git_dir_values(command: &str) -> Vec<Option<String>> {
    assignments(command)
        .into_iter()
        .filter(|(name, _)| name == "GIT_DIR")
        .map(|(_, value)| value)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_spellings_refuse_on_git_lines_only() {
        for line in [
            "git -C '//fixture-host/fixture-share' fixture-alias",
            "git -c include.path=//fixture-host/fixture-share/config fixture-alias",
            "git -cinclude.path=//fixture-host/share/config status",
            "GIT_CONFIG_VALUE_0=//fixture-host/share git status",
            "git -C '\\\\?\\C:\\fixture' status",
            "git --git-dir='\\\\.\\C:\\fixture\\.git' status",
            "cd x && git -C//fixture-host/share status",
        ] {
            assert!(namespace(line).is_some(), "{line}");
        }
        for line in [
            "git status",
            "git clone https://example.test/x/y.git",
            "cat README.md",
        ] {
            assert!(namespace(line).is_none(), "{line}");
        }
    }

    #[test]
    fn a_multibyte_message_word_does_not_panic() {
        for line in [
            "git commit -m '-é'",
            "git commit -m -é",
            "git log --grep=-ü -é",
        ] {
            assert!(namespace(line).is_none(), "{line}");
            let _ = unclassified_environment(line);
        }
    }

    #[test]
    fn unlisted_git_environment_refuses() {
        assert_eq!(
            unclassified_environment("GIT_WORK_TREE=/x git add y").as_deref(),
            Some("GIT_WORK_TREE")
        );
        assert_eq!(
            unclassified_environment("export GIT_INDEX_FILE=/tmp/i; git show :x").as_deref(),
            Some("GIT_INDEX_FILE")
        );
        assert_eq!(
            unclassified_environment("env GIT_CONFIG_COUNT=1 git status").as_deref(),
            Some("GIT_CONFIG_COUNT")
        );
        assert_eq!(
            unclassified_environment("GIT_TRACE=/tmp/t git status").as_deref(),
            Some("GIT_TRACE")
        );
        assert_eq!(unclassified_environment("GIT_PAGER=cat git log"), None);
        assert_eq!(unclassified_environment("GIT_DIR=.git git status"), None);
        assert_eq!(unclassified_environment("echo GIT_WORK_TREE"), None);
        assert_eq!(unclassified_environment("MY_GIT_X=1 git status"), None);
        assert_eq!(
            unclassified_environment("git commit -m 'docs: note GIT_WORK_TREE'"),
            None
        );
        assert_eq!(unclassified_environment("git grep GIT_WORK_TREE"), None);
        assert_eq!(
            unclassified_environment("GIT_LFS_SKIP_SMUDGE=1 git clone https://x.test/y"),
            None
        );
        assert_eq!(
            unclassified_environment("unset GIT_EXEC_PATH; git status").as_deref(),
            Some("GIT_EXEC_PATH")
        );
        assert_eq!(
            unclassified_environment("N=GIT_WORK_TREE; export \"$N=/x\"; git add y").as_deref(),
            Some("$N")
        );
        assert_eq!(
            program_values("GIT_EXTERNAL_DIFF='cat id_rsa' git diff"),
            vec!["cat id_rsa".to_string()]
        );
        assert!(program_values("git commit -m 'GIT_PAGER=cat'").len() <= 1);
    }

    #[test]
    fn a_namespace_line_is_refused_before_any_configuration_read() {
        let cwd = std::env::temp_dir();
        for line in [
            "git -C '//fixture-host/fixture-share' fixture-alias",
            "git -c include.path=//fixture-host/fixture-share/config fixture-alias",
        ] {
            super::super::git_config::READS.with(|reads| reads.set(0));
            let violations = crate::hooks::exec_guard::evaluate_in(
                line,
                true,
                &crate::hooks::policy::SecuritySection::default(),
                crate::hooks::policy::PolicyLevel::Block,
                &cwd,
                &cwd,
            );
            assert!(
                violations
                    .iter()
                    .any(|v| v.message.contains("share, verbatim or device")),
                "{line}: {violations:?}"
            );
            assert_eq!(
                super::super::git_config::READS.with(std::cell::Cell::get),
                0,
                "{line}"
            );
        }
    }
}
