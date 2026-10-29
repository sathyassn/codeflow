//! Spellings refused in a line that runs Git before any guard reads a
//! repository or its configuration (TSK-1002 Codex review 10).
//!
//! A share, verbatim or device spelling anywhere in such a line refuses
//! before any walker, alias lookup or configuration read can touch it. A
//! `GIT_*` environment variable the guard has not classified refuses as
//! cannot-verify: unlisted means unknown.

/// The `GIT_*` variables a line may name beside a Git call. Each is listed
/// from Git's own documentation (`git help git`, "Environment Variables",
/// Git 2.53) with why it leaves path, pathspec, configuration and alias
/// resolution as the guard reads it:
///
/// - identity (`GIT_AUTHOR_*`, `GIT_COMMITTER_*`): commit metadata only;
/// - programs Git runs (`GIT_PAGER`, `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`,
///   `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, `GIT_ASKPASS`,
///   `GIT_EXTERNAL_DIFF`, and `GIT_EXEC_PATH`, where Git finds its helper
///   and external `git-*` programs): what those programs do once they run
///   is the option D residual tracked by TSK-1065;
/// - output and interaction (`GIT_TERMINAL_PROMPT`, `GIT_FLUSH`,
///   `GIT_PROGRESS_DELAY`, `GIT_MERGE_VERBOSITY`, `GIT_ADVICE`,
///   `GIT_OPTIONAL_LOCKS`, `GIT_DIFF_OPTS`, `GIT_HTTP_USER_AGENT`);
/// - pathspec forms that only narrow a pattern (`GIT_GLOB_PATHSPECS`) or
///   make it literal (`GIT_NOGLOB_PATHSPECS`, `GIT_LITERAL_PATHSPECS`),
///   which the pattern rule over-refuses at worst;
/// - `GIT_DIR`, whose work tree stays the directory Git runs in.
///
/// Every other `GIT_*` variable refuses on a line that runs Git, among them
/// the ones that move resolution: `GIT_WORK_TREE`, `GIT_COMMON_DIR`,
/// `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
/// `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_NAMESPACE`,
/// `GIT_CEILING_DIRECTORIES`, `GIT_DISCOVERY_ACROSS_FILESYSTEM`, every
/// `GIT_CONFIG*`, `GIT_TEMPLATE_DIR`, `GIT_ATTR_SOURCE`, `GIT_ATTR_NOSYSTEM`,
/// `GIT_ICASE_PATHSPECS`, `GIT_REPLACE_REF_BASE`, `GIT_NO_REPLACE_OBJECTS`,
/// `GIT_SHALLOW_FILE`, `GIT_REDIRECT_STDIN` and its siblings, and the
/// `GIT_TRACE*` family, whose value may name a file Git writes.
const CLASSIFIED_ENVIRONMENT: &[&str] = &[
    "GIT_ADVICE",
    "GIT_ASKPASS",
    "GIT_AUTHOR_DATE",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_NAME",
    "GIT_COMMITTER_DATE",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_DIFF_OPTS",
    "GIT_DIR",
    "GIT_EDITOR",
    "GIT_EXEC_PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_FLUSH",
    "GIT_GLOB_PATHSPECS",
    "GIT_HTTP_USER_AGENT",
    "GIT_LITERAL_PATHSPECS",
    "GIT_MERGE_VERBOSITY",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_OPTIONAL_LOCKS",
    "GIT_PAGER",
    "GIT_PROGRESS_DELAY",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_SSH_VARIANT",
    "GIT_TERMINAL_PROMPT",
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
/// attached value (`-CX`).
fn values(word: &str) -> Vec<&str> {
    let mut out = vec![word];
    if let Some((_, value)) = word.split_once('=') {
        out.push(value);
    }
    if word.starts_with('-') && !word.starts_with("--") && word.len() > 2 {
        out.push(&word[2..]);
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

/// The first `GIT_*` name the line holds that [`CLASSIFIED_ENVIRONMENT`]
/// does not list, when the line runs Git.
#[must_use]
pub(crate) fn unclassified_environment(command: &str) -> Option<String> {
    if !runs_git(command) {
        return None;
    }
    let bytes = command.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut at = 0;
    while let Some(found) = command[at..].find("GIT_") {
        let start = at + found;
        let mut end = start + "GIT_".len();
        while end < bytes.len() && identifier(bytes[end]) {
            end += 1;
        }
        let name = &command[start..end];
        let bounded = start == 0 || !identifier(bytes[start - 1]);
        if bounded && !CLASSIFIED_ENVIRONMENT.contains(&name) {
            return Some(name.to_string());
        }
        at = end;
    }
    None
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
