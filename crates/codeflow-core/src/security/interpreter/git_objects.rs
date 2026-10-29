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
/// of `worktree`, `notes` and `bundle`, the path or file `worktree add`
/// and `bundle` take after it, and `grep`'s search patterns (Fable review
/// 13: `git grep 2026` searches text).
fn candidates(call: &outward::Unwrapped, subcommand: &str, start: usize) -> Vec<String> {
    let operand = |word: &String| word == "-" || !word.starts_with('-');
    let words_before: Vec<&String> = call.args[start..]
        .iter()
        .take_while(|word| *word != "--")
        .collect();
    let verb = words_before
        .iter()
        .find(|word| operand(word))
        .map(|word| word.as_str());
    let (patterns, explicit) = if subcommand == "grep" {
        grep_patterns(&words_before)
    } else {
        (Vec::new(), false)
    };
    let mut skip = match (subcommand, verb) {
        ("worktree", Some("add")) | ("bundle", Some(_)) => 2,
        ("push" | "notes" | "worktree", _) => 1,
        ("grep", _) if !explicit => 1,
        _ => 0,
    };
    let mut words = Vec::new();
    for (at, word) in words_before.into_iter().enumerate() {
        if patterns.contains(&at) {
            continue;
        }
        if skip > 0 && operand(word) {
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

/// The positions of `git grep`'s pattern words among `words` (an `-e` or
/// `-f` option with its value, attached or separate), and whether any is
/// given that way; without one, the first operand is the pattern.
fn grep_patterns(words: &[&String]) -> (Vec<usize>, bool) {
    let mut positions = Vec::new();
    let mut explicit = false;
    let mut next = false;
    for (at, word) in words.iter().enumerate() {
        if next {
            positions.push(at);
            next = false;
            continue;
        }
        if matches!(word.as_str(), "--regexp" | "--file") {
            positions.push(at);
            (explicit, next) = (true, true);
        } else if word.starts_with("--regexp=") || word.starts_with("--file=") {
            positions.push(at);
            explicit = true;
        } else if let Some(cluster) = word.strip_prefix('-').filter(|c| !c.starts_with('-')) {
            // A short cluster: flags until a letter that takes a value.
            for (index, letter) in cluster.char_indices() {
                if matches!(letter, 'e' | 'f') {
                    positions.push(at);
                    explicit = true;
                    next = index + 1 == cluster.len();
                    break;
                }
                if matches!(letter, 'A' | 'B' | 'C' | 'm' | 'O') || !letter.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    (positions, explicit)
}

/// Why a work tree or Git directory set on the line refuses.
pub(super) const MOVED: &str = "cannot establish that a Git directory or work tree the line sets is the repository Git would use without it; run Git in the repository without --git-dir, --work-tree or GIT_DIR";

/// Why `call` refuses for a Git directory or work tree it sets
/// (`--git-dir`, `--work-tree`, `GIT_DIR` on the line), or `None` when it
/// sets neither. Git itself reports the Git directory and work tree with
/// and without them, from the directory the call runs in; they must be the
/// same, so the setting changes nothing. With a Git directory alone, Git
/// takes the current directory as the work tree, and a `.git` file is read
/// as a gitfile, as Git does. Anything the guard cannot establish refuses
/// (TSK-1002 Fable review 13): `git --work-tree=~/.ssh add -A` stages a
/// store that `git diff --cached` would print.
fn moved_repository(
    call: &outward::Unwrapped,
    start: usize,
    command: &str,
) -> Option<&'static str> {
    let globals = if start >= 2 {
        &call.args[1..start - 1]
    } else {
        &[][..]
    };
    let mut kept = Vec::new();
    let mut moved: Vec<Option<String>> = crate::security::git_preflight::git_dir_values(command)
        .into_iter()
        .map(|value| value.map(|value| format!("--git-dir={value}")))
        .collect();
    let mut at = 0;
    while at < globals.len() {
        let word = globals[at].as_str();
        let next = globals.get(at + 1);
        match word {
            "-C" | "-c" => {
                kept.push(word.to_string());
                kept.extend(next.cloned());
                at += 2;
            }
            "--git-dir" | "--work-tree" => {
                moved.push(next.map(|value| format!("{word}={value}")));
                at += 2;
            }
            _ => {
                if word.starts_with("--git-dir=") || word.starts_with("--work-tree=") {
                    moved.push(Some(word.to_string()));
                }
                at += 1;
            }
        }
    }
    if moved.is_empty() {
        return None;
    }
    let Some(cwd) = call.cwd.as_deref() else {
        return Some(MOVED);
    };
    let Some(moved) = moved
        .into_iter()
        .map(|value| value.filter(|value| !value.contains(['$', '`', '~'])))
        .collect::<Option<Vec<String>>>()
    else {
        return Some(MOVED);
    };
    let mut with = kept.clone();
    with.extend(moved);
    let found = |globals: &[String]| match crate::security::git_config::repository_of(cwd, globals)
    {
        Ok(Some((dir, top))) => crate::portable_path::canonicalize(&dir)
            .ok()
            .zip(crate::portable_path::canonicalize(&top).ok()),
        _ => None,
    };
    match (found(&kept), found(&with)) {
        (Some(without), Some(set)) if without == set => None,
        _ => Some(MOVED),
    }
}

/// Why `call` refuses for an object it names without a path, or `None`.
/// `earlier_writer` says a Git call before it on the line may move a ref,
/// so names are not looked up: each must be a `HEAD` form or an existing
/// path, which Git reads as a path.
pub(super) fn unproven(
    call: &outward::Unwrapped,
    earlier_writer: bool,
    command: &str,
) -> Option<&'static str> {
    let (subcommand, start) = git_command(call)?;
    if let Some(why) = moved_repository(call, start, command) {
        return Some(why);
    }
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
            .any(|word| super::git_reads::update_index_option(word) == Some("cacheinfo"))
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
