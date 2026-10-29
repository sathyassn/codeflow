//! What a Git command reads beyond its operands, for the secret-read check
//! (TSK-1002 reviews 06 to 09). A Git command is cleared only in a form the
//! guard parses completely: an alias it cannot expand, an option it cannot
//! resolve, an input it cannot decode or a stdin it cannot read refuses as
//! cannot-verify.

use crate::security::outward;

/// Git's own commands besides the builtins (Git 2.53 `git --list-cmds=main`
/// less the builtins): its shell, Perl and helper programs. Their operands
/// are judged like a builtin's. Any other name is an alias, which is
/// expanded, or an external `git-*` program, judged like any program.
const SCRIPT_COMMANDS: &[&str] = &[
    "archimport",
    "credential-netrc",
    "credential-osxkeychain",
    "cvsexportcommit",
    "cvsimport",
    "cvsserver",
    "daemon",
    "difftool--helper",
    "filter-branch",
    "http-backend",
    "http-fetch",
    "http-push",
    "imap-send",
    "instaweb",
    "merge-octopus",
    "merge-one-file",
    "merge-resolve",
    "mergetool",
    "p4",
    "quiltimport",
    "remote-ftp",
    "remote-ftps",
    "remote-http",
    "remote-https",
    "request-pull",
    "send-email",
    "sh-i18n--envsubst",
    "shell",
    "submodule",
    "subtree",
    "web--browse",
];

/// Git global options without a value that leave alias lookup unchanged.
const ALIAS_NEUTRAL_GLOBALS: &[&str] = &[
    "--no-pager",
    "-p",
    "--paginate",
    "-P",
    "--no-replace-objects",
    "--no-optional-locks",
    "--no-lazy-fetch",
    "--no-advice",
];

/// The deepest alias chain the guard follows, as the git guard does.
const ALIAS_DEPTH: usize = 8;

/// `call` with a Git alias expanded the way Git prepends its value
/// (TSK-1002 review 07): `Ok(None)` when `call` is not Git, its subcommand
/// is one of Git's own, or no configuration aliases the name, so it runs an
/// external `git-*` program the guard judges like any other program. An
/// alias the guard cannot read refuses: a directory it cannot establish,
/// configuration the line changes or the guard cannot read, a global option
/// other than `-c`, `-C` and the neutral ones, a `!` shell alias, a value Git
/// cannot split or that starts with an option, and a chain deeper than
/// [`ALIAS_DEPTH`].
pub(super) fn expand_alias(
    call: &outward::Unwrapped,
    command: &str,
) -> Result<Option<outward::Unwrapped>, &'static str> {
    let mut expanded: Option<outward::Unwrapped> = None;
    for _ in 0..ALIAS_DEPTH {
        let current = expanded.as_ref().unwrap_or(call);
        let Some((program, rest)) = current.args.split_first() else {
            return Ok(expanded);
        };
        let Some(git) = crate::hooks::git_guard::git_arguments(program, rest) else {
            return Ok(expanded);
        };
        let Some((subcommand, tail)) = crate::hooks::git_guard::git_subcommand(&git) else {
            return Ok(expanded);
        };
        let at = current.args.len() - tail.len() - 1;
        if known(subcommand) || at == 0 {
            return Ok(expanded);
        }
        let Some(cwd) = current.cwd.as_deref() else {
            return Err("cannot establish the directory a Git alias is read in");
        };
        if crate::hooks::git_guard::configuration_line_backstop(command).is_some() {
            return Err("cannot read a Git alias on a line that changes configuration");
        }
        let mut config = Vec::new();
        let mut dir = cwd.to_path_buf();
        let mut index = 1;
        while index < at {
            let value = current.args.get(index + 1);
            match current.args[index].as_str() {
                "-c" => {
                    config.extend(value.cloned());
                    index += 2;
                }
                // Each `-C` is taken from the one before, as Git does.
                "-C" => {
                    let value = value
                        .filter(|value| !value.contains('$'))
                        .ok_or("cannot establish the directory a Git alias is read in")?;
                    dir = dir.join(value);
                    index += 2;
                }
                // `--exec-path` moves where Git finds programs, not aliases.
                word if ALIAS_NEUTRAL_GLOBALS.contains(&word)
                    || word.starts_with("--exec-path=") =>
                {
                    index += 1;
                }
                _ => return Err("cannot read a Git alias under that global option"),
            }
        }
        let query = crate::hooks::git_guard::AliasQuery {
            target: None,
            config: &config,
            name: subcommand,
        };
        let value = match crate::hooks::git_guard::read_alias(&dir, &query) {
            crate::hooks::git_guard::AliasAnswer::NotAlias => return Ok(expanded),
            crate::hooks::git_guard::AliasAnswer::Unreadable(_) => {
                return Err("cannot read the Git configuration that defines an alias");
            }
            crate::hooks::git_guard::AliasAnswer::Expansion(value) => value,
        };
        if value.starts_with('!') {
            return Err("cannot establish what a Git shell alias reads");
        }
        let words = crate::hooks::git_guard::split_alias(&value)
            .filter(|words| words.first().is_some_and(|word| !word.starts_with('-')))
            .ok_or("cannot read a Git alias value")?;
        let mut args = current.args[..at].to_vec();
        args.extend(words.iter().cloned());
        args.extend_from_slice(&current.args[at + 1..]);
        let expands = current.expands.as_ref().map(|expands| {
            let mut next = expands[..at.min(expands.len())].to_vec();
            next.extend(words.iter().map(|_| false));
            next.extend(expands.iter().skip(at + 1));
            next
        });
        expanded = Some(outward::Unwrapped {
            args,
            expands,
            input: current.input.clone(),
            stdin_bound: current.stdin_bound,
            unresolved: current.unresolved.clone(),
            supplier: current.supplier.clone(),
            cwd: current.cwd.clone(),
            links: current.links.clone(),
        });
    }
    Err("cannot follow a Git alias chain that deep")
}

/// Commands whose operands are proven not to name an object: a commit
/// message, a remote URL, a configuration value. Every other command's
/// operands are read as possible `REV:PATH` object names.
const TEXT_OPERAND_COMMANDS: &[&str] = &["commit", "remote", "config"];

/// Commands whose stdin, when they read it, holds names they report
/// metadata for (attributes, ignore rules, mailmap entries), never content.
const METADATA_STDIN_COMMANDS: &[&str] = &["check-attr", "check-ignore", "check-mailmap"];

/// Commands whose stdin, when they read it, is data they process (a patch,
/// a mailbox, a message, content to hash), never a list of names to read,
/// unless a names option below is given.
const DATA_STDIN_COMMANDS: &[&str] = &[
    "am",
    "apply",
    "check-ref-format",
    "column",
    "commit",
    "fmt-merge-msg",
    "hash-object",
    "interpret-trailers",
    "mailinfo",
    "mailsplit",
    "mktag",
    "notes",
    "patch-id",
    "stripspace",
    "tag",
];

/// `git hash-object`'s complete long-option list (Git 2.53).
const HASH_OBJECT_OPTIONS: &[&str] = &["stdin", "stdin-paths", "filters", "literally", "path"];

/// `git cat-file`'s complete long-option list (Git 2.53,
/// `git cat-file --git-completion-helper-all`), for Git's own
/// unique-prefix rule.
const CAT_FILE_OPTIONS: &[&str] = &[
    "allow-unknown-type",
    "use-mailmap",
    "mailmap",
    "batch",
    "batch-check",
    "batch-command",
    "batch-all-objects",
    "buffer",
    "follow-symlinks",
    "unordered",
    "textconv",
    "filters",
    "path",
    "filter",
];

/// Commands whose glob operands are ref or refspec patterns
/// (`refs/heads/*:refs/remotes/o/*`, `tag -l 'v*'`), not pathspecs.
const REF_PATTERN_COMMANDS: &[&str] = &[
    "branch",
    "clone",
    "config",
    "describe",
    "fetch",
    "for-each-ref",
    "ls-remote",
    "name-rev",
    "pack-refs",
    "pull",
    "push",
    "remote",
    "show-ref",
    "tag",
];

/// What a Git pathspec operand selects, as far as the guard reads it.
pub(super) enum Pathspec {
    /// A pattern Git matches against the tree and history itself (`'*.rs'`,
    /// `':(glob)**/x'`): the guard can only compare it with the secret
    /// names, as these spellings to check.
    Pattern(Vec<String>),
    /// A literal path after its magic (`:(top)id_rsa`), judged as a path.
    Literal(String),
    /// An exclusion, which selects nothing by itself.
    Excluded,
}

/// The pathspec reading of the operand at `at`, or `None` when it is not
/// a pathspec with glob characters or magic. Git's glob `*` also crosses
/// `/`, so a pattern is also compared as a match anywhere below the root
/// and by the text after its last `*`. Magic the guard does not read
/// (`attr:`, an unknown word) refuses.
pub(super) fn pathspec(
    call: &outward::Unwrapped,
    at: usize,
) -> Option<Result<Pathspec, &'static str>> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let (subcommand, tail) = crate::hooks::git_guard::git_subcommand(&git)?;
    let start = call.args.len() - tail.len();
    if REF_PATTERN_COMMANDS.contains(&subcommand) || at < start {
        return None;
    }
    if call.args[..start]
        .iter()
        .any(|word| word == "--icase-pathspecs")
    {
        return Some(Err(ICASE));
    }
    let word = call.args.get(at)?.as_str();
    let after_separator = call.args[start..at].iter().any(|word| word == "--");
    if !after_separator && word.starts_with('-') {
        return None;
    }
    let (magic, body) = match pathspec_magic(word, after_separator) {
        Ok(Some(split)) => split,
        Ok(None) => (Vec::new(), word),
        Err(why) => return Some(Err(why)),
    };
    if magic
        .iter()
        .any(|word| matches!(*word, "exclude" | "!" | "^"))
    {
        return Some(Ok(Pathspec::Excluded));
    }
    if magic.contains(&"icase") {
        return Some(Err(ICASE));
    }
    let literal = magic.contains(&"literal");
    if literal || !body.contains(['*', '?', '[']) {
        return (!magic.is_empty()).then(|| Ok(Pathspec::Literal(body.to_string())));
    }
    let body = body.strip_prefix('/').unwrap_or(body);
    // With `glob` magic Git matches as a shell does: `*` stays within one
    // component and the pattern is anchored where Git runs.
    if magic.contains(&"glob") {
        return Some(Ok(Pathspec::Pattern(vec![body.to_string()])));
    }
    let mut spellings = vec![body.to_string(), format!("**/{body}")];
    if let Some((_, after)) = body.rsplit_once('*') {
        let leaf = after.rsplit_once('/').map_or(after, |(_, leaf)| leaf);
        spellings.push(format!("**/*{leaf}"));
    }
    Some(Ok(Pathspec::Pattern(spellings)))
}

/// Git matches a case-insensitive pathspec (`:(icase)`,
/// `--icase-pathspecs`) in any case, which the guard's pattern comparison
/// does not model.
pub(super) const ICASE: &str = "cannot establish what a case-insensitive Git pathspec selects";

/// The magic words of a pathspec (`:(top,glob)x`, `:!x`, `:/x`) and the
/// path after them; `None` when the word has no magic. Before `--` a
/// short form may be a revision (`:/message`, `:0:path`), which the
/// object-path reading judges, so only the long form is read there.
fn pathspec_magic(
    word: &str,
    after_separator: bool,
) -> Result<Option<(Vec<&str>, &str)>, &'static str> {
    const UNREAD: &str = "cannot establish what Git pathspec magic selects";
    let Some(rest) = word.strip_prefix(':') else {
        return Ok(None);
    };
    if let Some(long) = rest.strip_prefix('(') {
        let (words, body) = long.split_once(')').ok_or(UNREAD)?;
        let magic: Vec<&str> = words.split(',').collect();
        if magic
            .iter()
            .any(|word| !matches!(*word, "top" | "literal" | "glob" | "icase" | "exclude"))
        {
            return Err(UNREAD);
        }
        return Ok(Some((magic, body)));
    }
    if !after_separator {
        return Ok(None);
    }
    let short_end = rest
        .char_indices()
        .find(|(_, ch)| !matches!(ch, '/' | '!' | '^'))
        .map_or(rest.len(), |(at, _)| at);
    let magic: Vec<&str> = rest[..short_end]
        .char_indices()
        .map(|(at, ch)| &rest[at..at + ch.len_utf8()])
        .collect();
    let body = rest[short_end..]
        .strip_prefix(':')
        .unwrap_or(&rest[short_end..]);
    Ok(Some((magic, body)))
}

/// Whether the guard models `subcommand`.
pub(super) fn known(subcommand: &str) -> bool {
    crate::hooks::git_guard::GIT_BUILTINS.contains(&subcommand)
        || SCRIPT_COMMANDS.contains(&subcommand)
}

/// Whether the operands of `subcommand` may name an object.
pub(super) fn object_reader(subcommand: &str) -> bool {
    !TEXT_OPERAND_COMMANDS.contains(&subcommand)
}

/// What `git cat-file` reads from its input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Batch {
    /// No batch mode: stdin is not read.
    None,
    /// `--batch-check`: object metadata only.
    Check,
    /// `--batch`: the contents of each object named on a line.
    Objects,
    /// `--batch-command`: `contents`, `info` and `flush` commands.
    Commands,
}

/// The long option `name` resolves to by Git's rule: an exact name, else
/// the one option it is a prefix of. `None` when it names no option or
/// more than one; Git refuses those too.
fn resolve_long<'a>(name: &str, options: &[&'a str]) -> Option<(&'a str, bool)> {
    let forms = options
        .iter()
        .flat_map(|option| [(*option, false), (*option, true)]);
    let spelled = |(option, negated): &(&str, bool)| {
        if *negated {
            format!("no-{option}")
        } else {
            (*option).to_string()
        }
    };
    if let Some(exact) = forms.clone().find(|form| spelled(form) == name) {
        return Some(exact);
    }
    let mut prefixed = forms.filter(|form| spelled(form).starts_with(name));
    let first = prefixed.next()?;
    prefixed.next().is_none().then_some(first)
}

/// `git cat-file`'s batch mode from its options, read completely. An
/// option the complete list does not resolve, an unknown short option, or
/// a content batch over every object or through in-tree links refuses.
fn cat_file(options: &[&str]) -> Result<Batch, &'static str> {
    const UNREAD: &str = "cannot establish a git cat-file option";
    let mut mode = Batch::None;
    let mut every_object = false;
    let mut links = false;
    let mut words = options.iter();
    while let Some(word) = words.next() {
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            let (option, negated) = resolve_long(name, CAT_FILE_OPTIONS).ok_or(UNREAD)?;
            match (option, negated) {
                ("batch", false) => mode = Batch::Objects,
                ("batch-command", false) => mode = Batch::Commands,
                ("batch-check", false) if mode == Batch::None => mode = Batch::Check,
                ("batch-all-objects", false) => every_object = true,
                ("follow-symlinks", false) => links = true,
                ("path" | "filter", false) if !attached => {
                    words.next();
                }
                _ => {}
            }
        } else if let Some(short) = word.strip_prefix('-').filter(|short| !short.is_empty()) {
            if !short.chars().all(|letter| "eptszZh".contains(letter)) {
                return Err(UNREAD);
            }
        }
    }
    if matches!(mode, Batch::Objects | Batch::Commands) && (every_object || links) {
        return Err("cannot establish which objects a content batch reads");
    }
    Ok(mode)
}

/// Whether `git hash-object` reads paths from stdin (`--stdin-paths` in
/// any spelling Git accepts). An option the complete list does not
/// resolve refuses.
fn hash_object_paths(options: &[&str]) -> Result<bool, &'static str> {
    let mut paths = false;
    for word in options {
        if let Some(long) = word.strip_prefix("--") {
            let name = long.split_once('=').map_or(long, |(name, _)| name);
            let (option, negated) = resolve_long(name, HASH_OBJECT_OPTIONS)
                .ok_or("cannot establish a git hash-object option")?;
            if option == "stdin-paths" {
                paths = !negated;
            }
        }
    }
    Ok(paths)
}

/// What a `--pathspec-from-file` option in any spelling Git accepts, an
/// abbreviation included, reads its paths from.
enum PathspecFile {
    /// The word is not that option.
    NotTheOption,
    /// The paths come from stdin (`-`).
    Stdin,
    /// The paths come from a file, or the value is missing.
    Elsewhere,
}

fn pathspec_file(word: &str, next: Option<&str>) -> PathspecFile {
    let Some(long) = word.strip_prefix("--") else {
        return PathspecFile::NotTheOption;
    };
    let (name, value) = long
        .split_once('=')
        .map_or((long, None), |(name, value)| (name, Some(value)));
    if name.is_empty() || !"pathspec-from-file".starts_with(name) {
        return PathspecFile::NotTheOption;
    }
    if value.or(next) == Some("-") {
        PathspecFile::Stdin
    } else {
        PathspecFile::Elsewhere
    }
}

/// A line in Git's C-style quoting (`"a\tb"`, `"\303\251"`), decoded as Git
/// decodes a quoted path. `None` when the quoting is not complete.
fn unquote(line: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = line.strip_prefix('"')?.chars();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => {
                let escaped = chars.next()?;
                let byte = match escaped {
                    'a' => 7,
                    'b' => 8,
                    't' => b'\t',
                    'n' => b'\n',
                    'v' => 11,
                    'f' => 12,
                    'r' => b'\r',
                    '"' => b'"',
                    '\\' => b'\\',
                    '0'..='3' => {
                        let digits = [escaped, chars.next()?, chars.next()?];
                        let text: String = digits.iter().collect();
                        u8::from_str_radix(&text, 8).ok()?
                    }
                    _ => return None,
                };
                bytes.push(byte);
            }
            other => {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    if !chars.as_str().is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// What a Git command reads beyond its operands: `None` when it reads
/// nothing more the guard needs to judge. An external `git-*` program
/// reads its stdin as any program does. Given stdin by the line, a command that may read names
/// from it has a literal input (here-string, heredoc, `echo` or `printf`
/// pipe) judged record by record, in Git's quoting too, as file paths and
/// object names, and refuses any other input, a file or a program's
/// output. Only commands proven to read data or metadata names from stdin
/// are exempt, and only without a names option: `--pathspec-from-file=-`,
/// `hash-object --stdin-paths`. A `cat-file --batch-command` record is
/// judged by its command: `contents` names an object, `info` and `flush`
/// read no content, any other refuses. A pathspec file other than stdin
/// refuses.
pub(super) fn reads(
    call: &outward::Unwrapped,
    judge: &dyn Fn(&str) -> Result<bool, &'static str>,
) -> Option<Result<bool, &'static str>> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let (subcommand, tail) = crate::hooks::git_guard::git_subcommand(&git)?;
    if !known(subcommand) {
        return None;
    }
    let options: Vec<&str> = tail
        .iter()
        .map(String::as_str)
        .take_while(|word| *word != "--")
        .collect();
    let mut names = false;
    for (at, word) in options.iter().enumerate() {
        match pathspec_file(word, options.get(at + 1).copied()) {
            PathspecFile::NotTheOption => {}
            PathspecFile::Stdin => names = true,
            PathspecFile::Elsewhere => {
                return Some(Err("cannot establish the paths a pathspec file names"));
            }
        }
    }
    if subcommand == "hash-object" {
        match hash_object_paths(&options) {
            Ok(paths) => names |= paths,
            Err(why) => return Some(Err(why)),
        }
    }
    let batch = if subcommand == "cat-file" {
        match cat_file(&options) {
            Ok(batch) => Some(batch),
            Err(why) => return Some(Err(why)),
        }
    } else {
        None
    };
    // xargs gives the command it runs no stdin of its own.
    let stdin = call.stdin_bound && !matches!(call.supplier, Some(outward::Supplier::Xargs));
    let exempt = METADATA_STDIN_COMMANDS.contains(&subcommand)
        || DATA_STDIN_COMMANDS.contains(&subcommand) && !names;
    if !stdin || exempt || matches!(batch, Some(Batch::None | Batch::Check)) {
        return None;
    }
    let Some(input) = call.input.as_deref() else {
        return Some(Err("cannot establish the Git input on stdin"));
    };
    Some(judge_input(input, batch, judge))
}

/// Every record of a literal Git input, by the reading its mode gives it.
fn judge_input(
    input: &str,
    batch: Option<Batch>,
    judge: &dyn Fn(&str) -> Result<bool, &'static str>,
) -> Result<bool, &'static str> {
    // A colon suffix of the record or of a word in it may be an object
    // path (`HEAD:x`, `contents HEAD:x`); Git splits words at a space or tab.
    let objects = |text: &str| -> Result<bool, &'static str> {
        let words = std::iter::once(text).chain(text.split([' ', '\t']).filter(|w| !w.is_empty()));
        for word in words {
            for (colon, _) in word.match_indices(':') {
                let path = &word[colon + 1..];
                if !path.is_empty() && judge(path)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };
    for record in input.split(['\n', '\0']) {
        let record = record.trim_end_matches('\r');
        if record.is_empty() {
            continue;
        }
        let secret = match batch {
            Some(Batch::Objects) => objects(record)?,
            Some(Batch::Commands) => match record.split_once([' ', '\t']) {
                Some(("contents", object)) => objects(object)?,
                Some(("info", _)) => false,
                None if record == "flush" => false,
                _ => return Err("cannot establish a git cat-file batch command"),
            },
            _ => {
                let decoded = if record.starts_with('"') {
                    Some(unquote(record).ok_or("cannot decode a quoted Git path on stdin")?)
                } else {
                    None
                };
                let mut secret = false;
                for form in std::iter::once(record).chain(decoded.as_deref()) {
                    secret = secret || judge(form)? || objects(form)?;
                }
                secret
            }
        };
        if secret {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_options_resolve_by_git_unique_prefix_rule() {
        assert_eq!(
            resolve_long("batch", CAT_FILE_OPTIONS),
            Some(("batch", false))
        );
        assert_eq!(
            resolve_long("batch-com", CAT_FILE_OPTIONS),
            Some(("batch-command", false))
        );
        assert_eq!(resolve_long("batch-c", CAT_FILE_OPTIONS), None);
        assert_eq!(
            resolve_long("no-fol", CAT_FILE_OPTIONS),
            Some(("follow-symlinks", true))
        );
        assert_eq!(resolve_long("unknown", CAT_FILE_OPTIONS), None);
    }

    #[test]
    fn quoted_paths_decode_as_git_decodes_them() {
        assert_eq!(unquote(r#""id_rsa""#).as_deref(), Some("id_rsa"));
        assert_eq!(unquote(r#""id\137rsa""#).as_deref(), Some("id_rsa"));
        assert_eq!(unquote(r#""a\tb""#).as_deref(), Some("a\tb"));
        assert_eq!(unquote(r#""open"#), None);
        assert_eq!(unquote(r#""bad\q""#), None);
        assert_eq!(unquote(r#""a"b"#), None);
    }

    #[test]
    fn cat_file_modes_are_read_completely() {
        assert!(cat_file(&["--batch-com"]) == Ok(Batch::Commands));
        assert!(cat_file(&["--batch-check"]) == Ok(Batch::Check));
        assert!(cat_file(&["--batch-c"]).is_err());
        assert!(cat_file(&["--batch", "--follow-sym"]).is_err());
        assert!(cat_file(&["--batch-all", "--batch"]).is_err());
        assert!(cat_file(&["--batch-check", "--batch-all-objects"]) == Ok(Batch::Check));
        assert!(cat_file(&["-q"]).is_err());
        assert!(cat_file(&["-p"]) == Ok(Batch::None));
    }
}
