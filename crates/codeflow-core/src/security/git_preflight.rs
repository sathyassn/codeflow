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

/// The environment variables whose value is a program Git runs, judged as
/// a command on a line that runs Git (Fable reviews 13 and 14). Sources:
///
/// - `git help git`, "Environment Variables": `GIT_EDITOR`, `GIT_PAGER`,
///   `GIT_EXTERNAL_DIFF`, `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_ASKPASS`, and
///   `GIT_SEQUENCE_EDITOR` (`git help rebase`);
/// - `git help var`, `GIT_EDITOR` and `GIT_PAGER`: Git falls back to
///   `VISUAL`, then `EDITOR`, for the editor and to `PAGER` for the pager;
/// - `git help credential`, "Requesting credentials": `SSH_ASKPASS` after
///   `GIT_ASKPASS` and `core.askPass`;
/// - `less(1)`, "Input preprocessor": `LESSOPEN` and `LESSCLOSE`, programs
///   Git's default pager runs.
///
/// The `-c`, `--config-env` and `git config` spellings of the same programs
/// are TSK-1065's residual, not judged here.
const PROGRAM_ENVIRONMENT: &[&str] = &[
    "EDITOR",
    "GIT_ASKPASS",
    "GIT_EDITOR",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "LESSCLOSE",
    "LESSOPEN",
    "PAGER",
    "SSH_ASKPASS",
    "VISUAL",
];

/// Shell builtins whose operands set, export or clear variables.
const DECLARERS: &[&str] = &["declare", "export", "local", "readonly", "typeset", "unset"];

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
            // A program word the shell fills in may be Git (`$G commit`).
            crate::hooks::git_guard::strip_launchers(&words).is_some_and(|(program, rest)| {
                crate::hooks::git_guard::unresolved_word(program)
                    || crate::hooks::git_guard::git_arguments(program, rest).is_some()
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

/// One variable a line sets or declares: its name, the value it assigns if
/// the word shows one, and whether `unset` clears it.
struct Assignment {
    name: String,
    value: Option<String>,
    unset: bool,
}

/// What a line that runs Git sets: every assignment the shell makes, and
/// the first word that may set a program variable some other way.
#[derive(Default)]
struct Scan {
    assignments: Vec<Assignment>,
    loose: Option<String>,
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Whether `word` names a program variable other than by reading it
/// (`$NAME`, `${NAME}`, `${NAME:-x}`): a bare name such as a `read` or
/// `printf -v` target or a `declare -n` value, or an assigning expansion
/// (`${NAME:=x}`, `${NAME=x}`).
fn names_program_variable(word: &str) -> bool {
    let bytes = word.as_bytes();
    let boundary = |at: usize| {
        bytes
            .get(at)
            .is_none_or(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
    };
    PROGRAM_ENVIRONMENT.iter().any(|name| {
        word.match_indices(name).any(|(at, _)| {
            let end = at + name.len();
            if (at > 0 && !boundary(at - 1)) || !boundary(end) {
                return false;
            }
            let rest = &word[end..];
            match at.checked_sub(1).map(|before| bytes[before]) {
                Some(b'$') => false,
                Some(b'{') if at >= 2 && bytes[at - 2] == b'$' => {
                    rest.starts_with('=') || rest.starts_with(":=")
                }
                _ => true,
            }
        })
    })
}

/// The assignments and declarations of a line that runs Git, by their
/// shell grammar (TSK-1002 Codex review 12): the `NAME=value` words before
/// a command's program word, those among its launchers (`env`, `sudo`),
/// and the operands of a declaring builtin. A word in any other place,
/// such as a commit message or a search text, assigns nothing.
fn scan(command: &str) -> Scan {
    let mut found = Vec::new();
    let mut loose = None;
    if !runs_git(command) {
        return Scan::default();
    }
    for segment in crate::hooks::git_guard::expand_commands(command) {
        let words = crate::hooks::git_guard::command_argv(&segment);
        let leading = words
            .iter()
            .take_while(|word| {
                word.split_once('=')
                    .is_some_and(|(name, _)| identifier(name.trim_end_matches('+')))
            })
            .count();
        // `env -S` packs its command into one word, read as a segment of
        // its own: only the leading assignments belong to this one.
        let (program_at, program) = match crate::hooks::git_guard::strip_launchers(&words) {
            Some((program, rest)) => (words.len() - rest.len() - 1, Some(program)),
            None => (leading, None),
        };
        let git = program.is_some_and(|program| {
            let rest = &words[program_at + 1..];
            crate::hooks::git_guard::git_arguments(program, rest).is_some()
        });
        let builtin = program.filter(|program| DECLARERS.contains(program));
        let mut set = |name: &str, value: Option<&str>, unset: bool| {
            found.push(Assignment {
                name: name.to_string(),
                value: value.map(str::to_string),
                unset,
            });
            PROGRAM_ENVIRONMENT.contains(&name)
        };
        let mut unset_operand = false;
        for (at, word) in words.iter().enumerate() {
            let skipped = std::mem::take(&mut unset_operand);
            let assigned = if at < program_at {
                // An `env -u NAME` operand clears a variable.
                unset_operand = matches!(word.as_str(), "-u" | "--unset");
                match word.split_once('=') {
                    Some((name, value)) if identifier(name.trim_end_matches('+')) => {
                        set(name.trim_end_matches('+'), Some(value), false)
                    }
                    _ => skipped || word.starts_with("--unset="),
                }
            } else if at > program_at && !word.starts_with('-') {
                match builtin {
                    Some(builtin) => match word.split_once('=') {
                        // A declaring builtin's operand may build its name
                        // (`export "$N=x"`).
                        Some((name, value)) => set(name.trim_end_matches('+'), Some(value), false),
                        None => set(word, None, builtin == "unset"),
                    },
                    None => false,
                }
            } else {
                false
            };
            // Git's own arguments are text; anywhere else, a word naming a
            // program variable may set it in a way the guard does not read.
            let own = git && at >= program_at;
            if !assigned && !own && loose.is_none() && names_program_variable(word) {
                loose = Some(word.clone());
            }
        }
    }
    Scan {
        assignments: found,
        loose,
    }
}

/// The `(name, value)` pairs [`scan`] reads.
fn assignments(command: &str) -> Vec<(String, Option<String>)> {
    scan(command)
        .assignments
        .into_iter()
        .map(|assignment| (assignment.name, assignment.value))
        .collect()
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

/// The commands a line that runs Git gives the variables naming a program
/// Git runs ([`PROGRAM_ENVIRONMENT`]), to be judged as commands, or why one
/// cannot be established (TSK-1002 Codex review 12, Fable review 14).
///
/// One rule covers a program value and a command word: its text must
/// resolve to literal text, as [`crate::security::composed`] resolves the
/// line's own assignments; anything else refuses as cannot-verify. A value
/// built from the line's variables is judged by what it holds; one that
/// holds text from outside the line, a construct the resolver does not
/// model, an export with no value the line shows, or a name set some other
/// way (`read`, `printf -v`, `${NAME:=x}`, a `declare -n` reference) refuses.
///
/// # Errors
///
/// Why a program value cannot be established.
pub(crate) fn program_values(command: &str) -> Result<Vec<String>, String> {
    let scan = scan(command);
    if let Some(word) = scan.loose {
        return Err(format!(
            "`{word}` may set a program Git runs in a way the guard does not read"
        ));
    }
    let program = |name: &str| PROGRAM_ENVIRONMENT.contains(&name);
    // `LESSOPEN` and `LESSCLOSE` start with `|`, `||` or `-` markers.
    let command_text = |name: &str, value: &str| {
        if name.starts_with("LESS") {
            value.trim_start_matches(['|', '-']).to_string()
        } else {
            value.to_string()
        }
    };
    let bound: Vec<(String, super::composed::Composed)> = super::composed::bindings(command)
        .into_iter()
        .filter(|(name, _)| program(name))
        .collect();
    let mut out = Vec::new();
    for assignment in scan.assignments.iter().filter(|a| program(&a.name)) {
        match &assignment.value {
            None if assignment.unset => {}
            None => {
                return Err(format!(
                    "`{}` is exported with a value the line does not show",
                    assignment.name
                ))
            }
            Some(value) if !crate::hooks::git_guard::unresolved_word(value) => {
                out.push(command_text(&assignment.name, value));
            }
            Some(_) if bound.iter().any(|(name, _)| *name == assignment.name) => {}
            Some(value) => {
                return Err(format!(
                    "`{}` takes its value from `{value}`, which the guard cannot resolve",
                    assignment.name
                ))
            }
        }
    }
    for (name, value) in bound {
        match value {
            super::composed::Composed::Resolved(value) => match value.exact() {
                Some(text) => out.push(command_text(&name, &text)),
                None => {
                    return Err(format!(
                        "`{name}` holds text from outside the line, which the guard cannot read"
                    ))
                }
            },
            super::composed::Composed::Unresolved(why) => {
                return Err(format!("`{name}` holds {why}"));
            }
        }
    }
    out.retain(|text| !text.is_empty());
    out.sort();
    out.dedup();
    Ok(out)
}

/// Whether a Git call on the line sets `--exec-path=DIR`, where Git finds
/// the helper programs it runs, which the guard never sees: refused as the
/// `GIT_EXEC_PATH` spelling is (Fable review 14). `git --exec-path` alone
/// prints the path and runs nothing.
#[must_use]
pub(crate) fn exec_path_option(command: &str) -> bool {
    crate::hooks::git_guard::expand_commands(command)
        .iter()
        .any(|segment| {
            let words = crate::hooks::git_guard::command_argv(segment);
            crate::hooks::git_guard::strip_launchers(&words).is_some_and(|(program, rest)| {
                crate::hooks::git_guard::git_arguments(program, rest).is_some_and(|args| {
                    let global = crate::hooks::git_guard::git_subcommand(&args)
                        .map_or(args.len(), |(_, after)| args.len() - after.len() - 1);
                    args[..global]
                        .iter()
                        .any(|word| word.starts_with("--exec-path="))
                })
            })
        })
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
            Ok(vec!["cat id_rsa".to_string()])
        );
        assert_eq!(program_values("git commit -m 'GIT_PAGER=cat'"), Ok(vec![]));
    }

    #[test]
    fn only_assignment_positions_assign() {
        for line in [
            "git commit -m 'GIT_EXEC_PATH=fixture'",
            "git grep 'GIT_WORK_TREE=fixture'",
            "git log --grep=GIT_DIR=x",
        ] {
            assert_eq!(unclassified_environment(line), None, "{line}");
            assert!(git_dir_values(line).is_empty(), "{line}");
        }
        assert_eq!(
            unclassified_environment("env -i GIT_WORK_TREE=/x git add y").as_deref(),
            Some("GIT_WORK_TREE")
        );
        assert_eq!(
            unclassified_environment("nohup env GIT_WORK_TREE=/x git add y").as_deref(),
            Some("GIT_WORK_TREE")
        );
    }

    #[test]
    fn program_values_resolve_or_refuse() {
        let ok = |line: &str| program_values(line).unwrap_or_else(|why| panic!("{line}: {why}"));
        assert_eq!(ok("E='cat x'; GIT_EDITOR=\"$E\" git commit"), ["cat x"]);
        assert_eq!(ok("E='cat x'; export GIT_PAGER=\"$E\"; git log"), ["cat x"]);
        assert_eq!(ok("P=cat; GIT_PAGER=\"$P\" git log"), ["cat"]);
        assert_eq!(ok("EDITOR='cat x' git commit"), ["cat x"]);
        assert_eq!(ok("LESSOPEN='|cat x %s' git log"), ["cat x %s"]);
        assert_eq!(ok("unset PAGER; git log"), Vec::<String>::new());
        assert_eq!(ok("env -u GIT_PAGER git log"), Vec::<String>::new());
        assert_eq!(ok("echo \"$GIT_PAGER\"; git log"), Vec::<String>::new());
        for line in [
            "GIT_PAGER=\"$OUTSIDE\" git log",
            "E='cat x'; env GIT_PAGER=\"$E\" git log",
            "export GIT_PAGER; git log",
            "printf -v GIT_PAGER %s 'cat x'; export GIT_PAGER; git log",
            "read -r GIT_PAGER <<< 'cat x'; git log",
            ": \"${GIT_PAGER:=cat x}\"; git log",
            "declare -n r=GIT_PAGER; r='cat x'; git log",
            "GIT_PAGER=\"$(cat /tmp/x)\" git log",
            "G=git; GIT_PAGER=\"$OUTSIDE\" $G log",
        ] {
            assert!(program_values(line).is_err(), "{line}");
        }
    }

    #[test]
    fn exec_path_option_is_a_global_setting() {
        assert!(exec_path_option("git --exec-path=/tmp/x log -1"));
        assert!(exec_path_option("git -C . --exec-path=/tmp/x status"));
        assert!(!exec_path_option("git --exec-path"));
        assert!(!exec_path_option("git commit -m --exec-path=/tmp/x"));
        assert!(!exec_path_option("git log -1"));
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
