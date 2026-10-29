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
/// configuration the line changes or the guard cannot read, a share or
/// namespace directory (checked before any access), a global option other
/// than `-c`, `-C` and the neutral ones, a `!` shell alias, a value Git
/// cannot split or that starts with an option, and a chain deeper than
/// [`ALIAS_DEPTH`].
pub(super) fn expand_alias(
    call: &outward::Unwrapped,
    command: &str,
    share: &dyn Fn(&std::path::Path) -> Result<(), &'static str>,
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
        // Every directory the lookup could touch is checked before any
        // filesystem access: a share or namespace path refuses first
        // (TSK-1002 Codex review 08).
        share(cwd)?;
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
                    share(&dir)?;
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

/// Commands whose operands are proven not to name a local object: a commit
/// message, a configuration value, or a repository URL and the remote refs
/// `clone`, `fetch`, `ls-remote` and `pull` match against what the remote
/// advertises (hosted run 40816623125: a URL read as `https` plus the path
/// `//host/...` is a share on Windows). `push` stays a reader: its source
/// is a local object. Every other command's operands are read as possible
/// `REV:PATH` object names.
const TEXT_OPERAND_COMMANDS: &[&str] = &[
    "clone",
    "commit",
    "config",
    "fetch",
    "ls-remote",
    "pull",
    "remote",
];

/// Commands whose stdin, when they read it, holds names they report
/// metadata for (attributes, ignore rules, mailmap entries), never content.
const METADATA_STDIN_COMMANDS: &[&str] = &["check-attr", "check-ignore", "check-mailmap"];

/// Commands whose stdin is data they process (a patch, a mailbox, a
/// message, content to hash), never a list of names: only a pathspec file
/// or `hash-object --stdin-paths` makes them read names.
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

/// Commands that read a stream of object names, paths or protocol requests
/// from stdin without a `--stdin` option: pack, tree and import streams,
/// the serving side of transports, and credential helpers.
const STREAM_STDIN_COMMANDS: &[&str] = &[
    "credential",
    "credential-cache",
    "credential-netrc",
    "credential-osxkeychain",
    "credential-store",
    "cvsserver",
    "daemon",
    "fast-import",
    "http-backend",
    "mktree",
    "pack-objects",
    "receive-pack",
    "remote-ext",
    "remote-fd",
    "remote-ftp",
    "remote-ftps",
    "remote-http",
    "remote-https",
    "shell",
    "upload-archive",
    "upload-pack",
];

/// Commands whose `--stdin` list is file names, not object names.
const PATH_LIST_COMMANDS: &[&str] = &["checkout-index", "update-index"];

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

/// Git commands that read their operands as pathspecs (Git 2.53). Any
/// other command's operands are file names, revisions or text, and get the
/// ordinary path and object readings: pathspec magic never clears them.
const PATHSPEC_COMMANDS: &[&str] = &[
    "add",
    "archive",
    "bisect",
    "checkout",
    "clean",
    "commit",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "difftool",
    "format-patch",
    "grep",
    "log",
    "ls-files",
    "range-diff",
    "reset",
    "restore",
    "rev-list",
    "rm",
    "shortlog",
    "show",
    "stash",
    "status",
    "whatchanged",
];

/// Pathspec commands that take no revision, so every non-option word is a
/// pathspec before `--` too.
const REVISION_FREE_COMMANDS: &[&str] = &["add", "clean", "ls-files", "rm", "status"];

/// What one Git pathspec selects, as far as the guard reads it.
pub(super) enum Pathspec {
    /// A pattern Git matches against the tree and history itself (`'*.rs'`,
    /// `':(glob)**/x'`): the guard can only compare it with the secret
    /// names, as these spellings to check.
    Pattern(Vec<String>),
    /// A literal path after its magic (`:(top)id_rsa`), judged as a path.
    Literal(String),
    /// An exclusion. It never narrows what the other pathspecs select, and
    /// without them Git applies it to the whole tree.
    Excluded,
}

/// Why pathspecs that are all exclusions refuse: Git applies them to the
/// whole tree, which the guard cannot clear by name.
pub(super) const EXCLUSIONS_ONLY: &str =
    "cannot establish what Git pathspecs that only exclude select: they apply to the whole tree; name the paths to read";

/// Why a pattern that a secret name could match refuses.
pub(super) const PATTERN: &str = "cannot establish which tracked paths a Git pathspec selects";

/// One pathspec read as Git reads it: `Ok(None)` for a plain path with no
/// magic and no glob character, judged as an ordinary path. Git's glob `*`
/// also crosses `/`, so a pattern is also compared as a match anywhere below
/// the root and by the text after its last `*`. A case-insensitive pathspec
/// and magic the guard does not read (`attr:`, an unknown word) refuse.
fn classify(word: &str, after_separator: bool) -> Result<Option<Pathspec>, &'static str> {
    let (magic, body) = pathspec_magic(word, after_separator)?.unwrap_or((Vec::new(), word));
    if magic
        .iter()
        .any(|word| matches!(*word, "exclude" | "!" | "^"))
    {
        return Ok(Some(Pathspec::Excluded));
    }
    if magic.contains(&"icase") {
        return Err(ICASE);
    }
    let literal = magic.contains(&"literal");
    if literal || !body.contains(['*', '?', '[']) {
        return Ok((!magic.is_empty()).then(|| Pathspec::Literal(body.to_string())));
    }
    let body = body.strip_prefix('/').unwrap_or(body);
    // With `glob` magic Git matches as a shell does: `*` stays within one
    // component and the pattern is anchored where Git runs.
    if magic.contains(&"glob") {
        return Ok(Some(Pathspec::Pattern(vec![body.to_string()])));
    }
    let mut spellings = vec![body.to_string(), format!("**/{body}")];
    if let Some((_, after)) = body.rsplit_once('*') {
        let leaf = after.rsplit_once('/').map_or(after, |(_, leaf)| leaf);
        spellings.push(format!("**/*{leaf}"));
    }
    Ok(Some(Pathspec::Pattern(spellings)))
}

/// The Git subcommand of `call` and the index of its first operand.
fn git_command(call: &outward::Unwrapped) -> Option<(String, usize)> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let (subcommand, tail) = crate::hooks::git_guard::git_subcommand(&git)?;
    Some((subcommand.to_string(), call.args.len() - tail.len()))
}

/// Whether a pathspec operand of `call` selects anything itself: a word
/// after `--`, or any non-option word of a command that takes no revision,
/// that is not an exclusion.
fn has_positive_pathspec(call: &outward::Unwrapped, subcommand: &str, start: usize) -> bool {
    let mut after_separator = false;
    for word in &call.args[start..] {
        if word == "--" && !after_separator {
            after_separator = true;
            continue;
        }
        let revision_free = REVISION_FREE_COMMANDS.contains(&subcommand);
        let candidate = after_separator || revision_free && !word.starts_with('-');
        let spec = classify(word, after_separator || revision_free);
        if candidate && !matches!(spec, Ok(Some(Pathspec::Excluded))) {
            return true;
        }
    }
    false
}

/// The pathspec reading of the operand at `at`, or `None` when the command
/// does not read it as a pathspec or it is a plain path. Exclusions never
/// narrow a read; pathspecs that are all exclusions refuse.
pub(super) fn pathspec(
    call: &outward::Unwrapped,
    at: usize,
) -> Option<Result<Pathspec, &'static str>> {
    let (subcommand, start) = git_command(call)?;
    let subcommand = subcommand.as_str();
    if !PATHSPEC_COMMANDS.contains(&subcommand) || at < start {
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
    // A command that takes no revision reads short magic (`:!x`) before
    // `--` too.
    let magic_position = after_separator || REVISION_FREE_COMMANDS.contains(&subcommand);
    match classify(word, magic_position) {
        Err(why) => Some(Err(why)),
        Ok(None) => None,
        Ok(Some(Pathspec::Excluded)) if !has_positive_pathspec(call, subcommand, start) => {
            Some(Err(EXCLUSIONS_ONLY))
        }
        Ok(Some(spec)) => Some(Ok(spec)),
    }
}

/// How the secret-read check judges a path and a pattern spelling.
pub(super) struct Judges<'a> {
    /// Whether a path names a secret, or why that cannot be established.
    pub(super) path: &'a dyn Fn(&str) -> Result<bool, &'static str>,
    /// Whether a secret name could match a pattern spelling.
    pub(super) pattern: &'a dyn Fn(&str) -> bool,
}

/// The verdict on one pathspec that selects something.
pub(super) fn judge_pathspec(spec: &Pathspec, judges: &Judges<'_>) -> Result<bool, &'static str> {
    match spec {
        Pathspec::Pattern(spellings) if spellings.iter().any(|x| (judges.pattern)(x)) => {
            Err(PATTERN)
        }
        Pathspec::Pattern(_) | Pathspec::Excluded => Ok(false),
        Pathspec::Literal(path) => (judges.path)(path),
    }
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

/// Whether the options end with NUL-separated pathspec records
/// (`--pathspec-file-nul` in any unique spelling; the last form wins).
fn pathspec_file_nul(options: &[&str]) -> bool {
    let mut nul = false;
    for word in options {
        let Some(name) = word.strip_prefix("--") else {
            continue;
        };
        let (negated, name) = name
            .strip_prefix("no-")
            .map_or((false, name), |name| (true, name));
        if name.len() >= "pathspec-fi".len() && "pathspec-file-nul".starts_with(name) {
            nul = !negated;
        }
    }
    nul
}

/// Whether `word` is a `--stdin` option in a spelling Git may accept: the
/// name, a unique abbreviation of it, or a `--stdin-*` form.
fn stdin_option(word: &str) -> bool {
    let Some(long) = word.strip_prefix("--") else {
        return false;
    };
    let name = long.split_once('=').map_or(long, |(name, _)| name);
    name.starts_with("stdin") || name.len() >= 3 && "stdin".starts_with(name)
}

/// Whether a short-option word asks for NUL-separated records (`-z`, or a
/// cluster holding `z`).
fn short_nul(word: &str) -> bool {
    word.strip_prefix('-')
        .is_some_and(|short| !short.starts_with('-') && short.contains('z'))
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

/// How Git reads the records of a list on stdin.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Records {
    /// Object names, one per line (`cat-file --batch`).
    Objects,
    /// `cat-file --batch-command` commands.
    Commands,
    /// File names (`hash-object --stdin-paths`, `update-index --stdin`),
    /// C-quoted per line, or verbatim when NUL-separated.
    Paths { nul: bool },
    /// Pathspecs (`--pathspec-from-file=-`), decoded as paths are, then read
    /// with the argv pathspec rules.
    Pathspecs { nul: bool },
    /// Revisions, object names or requests a `--stdin` command or a stream
    /// reader takes: every record and word is judged as a path and as an
    /// object name.
    Stream,
}

/// What a Git command reads from stdin: `None` unless the command is in a
/// form that reads names from it. Those forms are a `cat-file` content
/// batch, `hash-object --stdin-paths`, `--pathspec-from-file=-`, a `--stdin`
/// option, and the stream readers. Each needs a literal input (here-string,
/// heredoc, `echo` or `printf` pipe) the guard reads record by record; any
/// other stdin, a file, a program's output, a descriptor an earlier `exec`
/// rebinds or one the line does not give, refuses. Every other command is
/// judged by its operands alone, whatever its stdin holds. A pathspec file
/// other than stdin refuses.
pub(super) fn reads(
    call: &outward::Unwrapped,
    judges: &Judges<'_>,
) -> Option<Result<bool, &'static str>> {
    let (subcommand, start) = git_command(call)?;
    let subcommand = subcommand.as_str();
    if !known(subcommand) {
        return None;
    }
    let options: Vec<&str> = call.args[start..]
        .iter()
        .map(String::as_str)
        .take_while(|word| *word != "--")
        .collect();
    let records = match records(subcommand, &options) {
        Ok(Some(records)) => records,
        Ok(None) => return None,
        Err(why) => return Some(Err(why)),
    };
    let Some(input) = call.input.as_deref() else {
        return Some(Err(if call.stdin_bound {
            "cannot establish the Git input on stdin"
        } else {
            "cannot establish what a Git list reads from stdin the line does not give"
        }));
    };
    Some(judge_input(input, records, judges))
}

/// The records `subcommand` reads from stdin with `options`, or `None`
/// when it reads no names there.
fn records(subcommand: &str, options: &[&str]) -> Result<Option<Records>, &'static str> {
    for (at, word) in options.iter().enumerate() {
        match pathspec_file(word, options.get(at + 1).copied()) {
            PathspecFile::NotTheOption => {}
            PathspecFile::Stdin => {
                return Ok(Some(Records::Pathspecs {
                    nul: pathspec_file_nul(options),
                }));
            }
            PathspecFile::Elsewhere => {
                return Err("cannot establish the paths a pathspec file names");
            }
        }
    }
    if subcommand == "hash-object" {
        return Ok(hash_object_paths(options)?.then_some(Records::Paths { nul: false }));
    }
    if subcommand == "cat-file" {
        return Ok(match cat_file(options)? {
            Batch::Objects => Some(Records::Objects),
            Batch::Commands => Some(Records::Commands),
            Batch::None | Batch::Check => None,
        });
    }
    if METADATA_STDIN_COMMANDS.contains(&subcommand) || DATA_STDIN_COMMANDS.contains(&subcommand) {
        return Ok(None);
    }
    let stdin = options.iter().any(|word| stdin_option(word));
    // `update-index --index-info` reads index entries naming objects.
    let index_info = options.contains(&"--index-info");
    if stdin && !index_info && PATH_LIST_COMMANDS.contains(&subcommand) {
        let nul = options.iter().any(|word| short_nul(word));
        return Ok(Some(Records::Paths { nul }));
    }
    let stream = stdin || index_info || STREAM_STDIN_COMMANDS.contains(&subcommand);
    Ok(stream.then_some(Records::Stream))
}

/// Every record of a literal Git input, by the reading its mode gives it.
fn judge_input(input: &str, records: Records, judges: &Judges<'_>) -> Result<bool, &'static str> {
    // A colon suffix of the record or of a word in it may be an object
    // path (`HEAD:x`, `contents HEAD:x`); Git splits words at a space or tab.
    let objects = |text: &str| -> Result<bool, &'static str> {
        let words = std::iter::once(text).chain(text.split([' ', '\t']).filter(|w| !w.is_empty()));
        for word in words {
            for (colon, _) in word.match_indices(':') {
                let path = &word[colon + 1..];
                if !path.is_empty() && (judges.path)(path)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };
    // A line-mode record that starts with a quote is read in Git's C
    // quoting; a NUL-separated one is taken verbatim.
    let decoded = |record: &str, nul: bool| -> Result<String, &'static str> {
        if !nul && record.starts_with('"') {
            unquote(record).ok_or("cannot decode a quoted Git path on stdin")
        } else {
            Ok(record.to_string())
        }
    };
    let (mut excluded, mut selecting) = (false, false);
    // Splitting at both separators in either mode only adds readings.
    for record in input.split(['\n', '\0']) {
        let record = record.trim_end_matches('\r');
        if record.is_empty() {
            continue;
        }
        let secret = match records {
            Records::Objects => objects(record)?,
            Records::Commands => match record.split_once([' ', '\t']) {
                Some(("contents", object)) => objects(object)?,
                Some(("info", _)) => false,
                None if record == "flush" => false,
                _ => return Err("cannot establish a git cat-file batch command"),
            },
            Records::Paths { nul } => (judges.path)(&decoded(record, nul)?)?,
            Records::Pathspecs { nul } => {
                let path = decoded(record, nul)?;
                match classify(&path, true)? {
                    Some(Pathspec::Excluded) => {
                        excluded = true;
                        false
                    }
                    Some(spec) => {
                        selecting = true;
                        judge_pathspec(&spec, judges)?
                    }
                    None => {
                        selecting = true;
                        (judges.path)(&path)?
                    }
                }
            }
            Records::Stream => {
                let unquoted = record.starts_with('"').then(|| unquote(record)).flatten();
                let mut secret = false;
                for form in std::iter::once(record).chain(unquoted.as_deref()) {
                    secret = secret || (judges.path)(form)? || objects(form)?;
                }
                secret
            }
        };
        if secret {
            return Ok(true);
        }
    }
    if excluded && !selecting {
        return Err(EXCLUSIONS_ONLY);
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
    fn stdin_records_are_read_only_from_the_forms_that_read_them() {
        assert!(records("status", &[]) == Ok(None));
        assert!(records("add", &["-p"]) == Ok(None));
        assert!(records("log", &["--oneline"]) == Ok(None));
        assert!(records("rev-list", &["--stdin"]) == Ok(Some(Records::Stream)));
        assert!(records("update-index", &["--std"]) == Ok(Some(Records::Paths { nul: false })));
        assert!(
            records("update-index", &["-z", "--stdin"]) == Ok(Some(Records::Paths { nul: true }))
        );
        assert!(records("pack-objects", &["--stdout"]) == Ok(Some(Records::Stream)));
        assert!(records("hash-object", &["--stdin"]) == Ok(None));
        assert!(records("hash-object", &["--stdin-p"]) == Ok(Some(Records::Paths { nul: false })));
        assert!(records("check-attr", &["--stdin"]) == Ok(None));
        assert!(
            records("add", &["--pathspec-from-file=-", "--pathspec-file-nul"])
                == Ok(Some(Records::Pathspecs { nul: true }))
        );
        assert!(
            records(
                "add",
                &[
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                    "--no-pathspec-file-nul"
                ]
            ) == Ok(Some(Records::Pathspecs { nul: false }))
        );
        assert!(records("add", &["--pathspec-from-file=list"]).is_err());
    }

    #[test]
    fn exclusions_never_narrow_a_selection() {
        let path = |path: &str| Ok(path.contains("id_rsa"));
        let pattern = |spelling: &str| spelling.contains("id_r");
        let judges = Judges {
            path: &path,
            pattern: &pattern,
        };
        let pathspecs = Records::Pathspecs { nul: false };
        assert_eq!(
            judge_input(":!README.md\n", pathspecs, &judges),
            Err(EXCLUSIONS_ONLY)
        );
        assert_eq!(
            judge_input(":!README.md\nsrc\n", pathspecs, &judges),
            Ok(false)
        );
        assert_eq!(
            judge_input(":(icase)ID_RSA\n", pathspecs, &judges),
            Err(ICASE)
        );
        assert_eq!(
            judge_input(":(glob)**/id_r*\n", pathspecs, &judges),
            Err(PATTERN)
        );
        assert_eq!(judge_input(":(top)id_rsa\n", pathspecs, &judges), Ok(true));
        assert_eq!(judge_input("\"id_rsa\"\n", pathspecs, &judges), Ok(true));
        let nul = Records::Pathspecs { nul: true };
        assert_eq!(judge_input("\"x\"\0", nul, &judges), Ok(false));
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
