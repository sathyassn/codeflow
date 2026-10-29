//! Git objects named without a path (TSK-1002, object reads by id).
//!
//! The secret-read model judges a Git object by the path it names
//! (`HEAD:id_rsa`). An object id, a tag or any revision that does not end in
//! `REV:PATH` names content without a path, so the guard does not map it
//! back to one. A command that may print, copy or relabel object content
//! accepts such a name only when it is proven to peel to a commit, whose
//! readings the rest of the model already covers. Anything else refuses as
//! cannot-verify.

use crate::security::outward;

/// Why an object named without a path refuses.
pub(super) const BY_ID: &str =
    "cannot establish what a Git object named by id, tag or revision holds; name it as REV:PATH";

/// Commands that may print an object's content, copy it into a file, the
/// index or a tree, point a ref at it, or send it to a remote (Git 2.53).
/// History commands (`log`, `rev-list`) and name readers (`ls-tree`,
/// `rev-parse`) are absent: they print no blob content.
const CONTENT_COMMANDS: &[&str] = &[
    "archive",
    "blame",
    "bundle",
    "cat-file",
    "checkout",
    "commit-tree",
    "diff",
    "diff-tree",
    "difftool",
    "grep",
    "merge-tree",
    "notes",
    "push",
    "read-tree",
    "replace",
    "reset",
    "restore",
    "show",
    "switch",
    "tag",
    "unpack-file",
    "update-ref",
    "worktree",
];

/// Git commands that change no ref, so a lookup made before the line runs
/// stays true after them.
const READ_ONLY: &[&str] = &[
    "blame",
    "cat-file",
    "describe",
    "diff",
    "grep",
    "log",
    "ls-files",
    "ls-tree",
    "merge-base",
    "name-rev",
    "rev-list",
    "rev-parse",
    "shortlog",
    "show",
    "show-ref",
    "status",
    "whatchanged",
];

/// For each call of the line, whether a Git call before it could move a
/// ref, so a lookup made now may not hold when it runs. A Git call the
/// guard does not list as read-only, an alias included, counts. A line that
/// may set or clear `GIT_DIR` moves the repository a lookup would read, so
/// every call counts.
pub(super) fn earlier_writers(calls: &[outward::Unwrapped], command: &str) -> Vec<bool> {
    let moves_dir = command.contains("GIT_DIR=")
        || command.contains("GIT_DIR")
            && ["unset", "env", "export", "declare", "typeset"]
                .iter()
                .any(|word| command.contains(word));
    let mut seen = moves_dir;
    calls
        .iter()
        .map(|call| {
            let before = seen;
            if let Some((subcommand, _)) = git_command(call) {
                seen = seen || !READ_ONLY.contains(&subcommand.as_str());
            }
            before
        })
        .collect()
}

/// The Git subcommand of `call`, the index of its first operand and the
/// global options before the subcommand.
fn git_command(call: &outward::Unwrapped) -> Option<(String, usize)> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let (subcommand, tail) = crate::hooks::git_guard::git_subcommand(&git)?;
    Some((subcommand.to_string(), call.args.len() - tail.len()))
}

/// Whether `word` is an object id or its abbreviation (four or more hex
/// digits).
fn hex(word: &str) -> bool {
    word.len() >= 4 && word.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `HEAD` or `@` with commit steps (`HEAD~2`, `@^`): always a commit, so no
/// lookup is needed.
fn head_form(word: &str) -> bool {
    let rest = word
        .strip_prefix("HEAD")
        .or_else(|| word.strip_prefix('@'))
        .unwrap_or("x");
    rest.bytes()
        .all(|byte| byte == b'~' || byte == b'^' || byte.is_ascii_digit())
}

/// The words of `call` that may name an object: operands and separate
/// option values before `--`, a long option's `=value` and a short
/// option's attached value at any letter. A word with `:` names a path the
/// rest of the model judges, except a `push` refspec, whose source is an
/// object. Each side of a range counts, without a leading `^`. The leading
/// operands that name no object are skipped: `push`'s repository, the verb
/// of `worktree`, `notes` and `bundle`, and the path or file `worktree add`
/// and `bundle` take after it.
fn candidates(call: &outward::Unwrapped, subcommand: &str, start: usize) -> Vec<String> {
    let operands = call.args[start..]
        .iter()
        .take_while(|word| *word != "--")
        .filter(|word| !word.starts_with('-'));
    let verb = operands.clone().next().map(String::as_str);
    let mut skip = match (subcommand, verb) {
        ("worktree", Some("add")) | ("bundle", Some(_)) => 2,
        ("push" | "notes" | "worktree", _) => 1,
        _ => 0,
    };
    let mut words = Vec::new();
    for word in call.args[start..].iter().take_while(|word| *word != "--") {
        if skip > 0 && !word.starts_with('-') {
            skip -= 1;
            continue;
        }
        let values: Vec<&str> = if let Some(long) = word.strip_prefix("--") {
            long.split_once('=')
                .map(|(_, value)| value)
                .into_iter()
                .collect()
        } else if let Some(short) = word.strip_prefix('-') {
            (1..short.len())
                .filter(|at| short.is_char_boundary(*at))
                .map(|at| &short[at..])
                .collect()
        } else {
            vec![word.as_str()]
        };
        for value in values {
            let value = if subcommand == "push" {
                value
                    .trim_start_matches('+')
                    .split(':')
                    .next()
                    .unwrap_or("")
            } else if value.contains(':') {
                continue;
            } else {
                value
            };
            for side in value.split("..") {
                let side = side.trim_start_matches(['^', '.']);
                if !side.is_empty() && !head_form(side) {
                    words.push(side.to_string());
                }
            }
        }
    }
    words.sort();
    words.dedup();
    words
}

/// Why `call` refuses for an object it names without a path, or `None`.
/// `earlier_writer` says a Git call before it on the line may move a ref,
/// so names are not looked up: each must be a `HEAD` form or an existing
/// path, which Git reads as a path.
pub(super) fn unproven(call: &outward::Unwrapped, earlier_writer: bool) -> Option<&'static str> {
    let (subcommand, start) = git_command(call)?;
    let subcommand = subcommand.as_str();
    // A format atom that prints raw object content, and fetching an object
    // by id into a named ref, name content without a path in any command.
    let words = &call.args[start..];
    if words.iter().any(|word| word.contains("%(raw")) {
        return Some(BY_ID);
    }
    if matches!(subcommand, "fetch" | "pull")
        && words.iter().any(|word| {
            word.split_once(':')
                .is_some_and(|(source, _)| hex(source.trim_start_matches('+')))
        })
    {
        return Some(BY_ID);
    }
    // Index entries given by object id relabel content under any name.
    if subcommand == "update-index"
        && words
            .iter()
            .any(|word| word.len() > 4 && "--cacheinfo".starts_with(word.as_str()))
    {
        return Some(BY_ID);
    }
    if !CONTENT_COMMANDS.contains(&subcommand) {
        return None;
    }
    // `cat-file -t`, `-s` and `-e` print an object's type or size, or test
    // that it exists.
    if subcommand == "cat-file"
        && words
            .iter()
            .any(|word| ["-t", "-s", "-e"].contains(&word.as_str()))
    {
        return None;
    }
    let names = candidates(call, subcommand, start);
    if names.is_empty() {
        return None;
    }
    let Some(cwd) = call.cwd.as_deref() else {
        return Some(BY_ID);
    };
    if earlier_writer {
        let path =
            |name: &String| matches!(crate::absence::proven_absent(&cwd.join(name)), Ok(false));
        return (!names.iter().all(path)).then_some(BY_ID);
    }
    let globals = if start >= 2 {
        &call.args[1..start - 1]
    } else {
        &[][..]
    };
    match crate::security::git_config::object_kinds(cwd, globals, &names) {
        Ok(kinds) => kinds
            .iter()
            .any(|kind| kind.as_ref().is_some_and(|(_, commit)| !commit))
            .then_some(BY_ID),
        Err(_) => Some(BY_ID),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_forms_need_no_lookup() {
        for word in ["HEAD", "HEAD~2", "HEAD^", "@", "@~1^2"] {
            assert!(head_form(word), "{word}");
        }
        for word in ["HEADS", "main", "HEAD^{blob}", "@{-1}", "abcd"] {
            assert!(!head_form(word), "{word}");
        }
    }

    #[test]
    fn object_ids_are_hex_words() {
        assert!(hex("78981922"));
        assert!(hex("abcd"));
        assert!(!hex("abc"));
        assert!(!hex("main"));
    }
}
