//! Git commands that write work-tree files from stored content (issue 633,
//! TSK-1390).
//!
//! A checkout or restore of paths, `checkout-index`, `read-tree -u`, `stash
//! apply`, `stash pop`, `stash branch`, `apply`, `cherry-pick -n` and
//! `revert -n` leave HEAD where it is and write files in the work tree.
//! When such a write puts other content on an enforcement path, the file sits
//! there as an uncommitted change and rides into the next commit. `git am`
//! and `git reset --hard`, `--keep` and `--merge` to a revision are judged
//! the same way.
//!
//! The judgment reads the content each command would write from the object
//! store (a revision, the index, the stash commits, the picked commits or the
//! patch paths), never from the work tree, and compares it with HEAD's entry
//! for every enforcement path the command's selection can reach. Equal
//! content passes, so restoring HEAD's files and restoring ordinary files
//! from any revision keep working. Anything the guard cannot read refuses:
//! an unknown option, a revision or stash it cannot resolve, a patch on
//! stdin, an unmerged enforcement entry, or a setting that moves the
//! repository, index or object store. A selection it cannot read is taken as
//! the whole tree. Branch switches, merge, rebase, pull and committing
//! cherry-pick and revert move HEAD with the files and keep their verdict.

use super::{expands, integrity_shell_path, parse_options, Arity, OptionSpec, ParsedOptions};
use crate::hooks::policy::PolicyLevel;
use crate::hooks::Violation;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

/// One tree, index or commit entry: its object and file mode.
type Entry = (git2::Oid, u32);

/// The entries that can land on an enforcement path, keyed by the path as
/// stored (`/`-separated).
type Entries = BTreeMap<String, Entry>;

/// How many tree entries one judgment reads before it stops and refuses.
const ENTRY_LIMIT: usize = 200_000;
/// How many commits a picked range may hold before the guard refuses it.
const COMMIT_LIMIT: usize = 10_000;
/// The largest patch or mailbox file the guard reads.
const PATCH_LIMIT: u64 = 64 * 1024 * 1024;

/// Global options that change where Git reads the repository, its index,
/// its objects, its configuration or its attributes. A path writer behind one
/// cannot be judged from the target directory alone.
const MOVING_GLOBALS: &[&str] = &[
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--attr-source",
    "--shallow-file",
    "--super-prefix",
    "--exec-path",
    "--bare",
];

/// Global options that change nothing the judgment reads.
const PLAIN_GLOBALS: &[&str] = &[
    "--no-pager",
    "-P",
    "-p",
    "--paginate",
    "--no-optional-locks",
    "--no-advice",
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--no-replace-objects",
    "--no-lazy-fetch",
];

/// Environment names that move the repository, index, object store,
/// configuration, replacement refs or attributes Git reads.
const MOVING_ENVIRONMENT: &[&str] = &[
    "GIT_DIR",
    "GIT_COMMON_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_REPLACE_REF_BASE",
    "GIT_ATTR_SOURCE",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// The verdict of one judgment: the enforcement path a write would change,
/// or the reason the guard cannot tell.
enum Verdict {
    Writes { path: String, source: String },
    Unknown(String),
}

/// The finding for a Git path writer that could put content other than
/// HEAD's on an enforcement path, or that the guard cannot judge.
pub(super) fn violation(
    verb: &str,
    filled: &[String],
    rest: &[String],
    level: PolicyLevel,
    dirs: (&Path, &Path),
    line: &str,
) -> Option<Violation> {
    if !level.is_active() || !candidate(verb, rest) {
        return None;
    }
    let verdict = match judge_line(verb, filled, rest, dirs, line) {
        Ok(None) => return None,
        Ok(Some(verdict)) => verdict,
        Err(why) => Verdict::Unknown(why),
    };
    let message = match verdict {
        Verdict::Writes { path, source } => format!(
            "`git {verb}` would put content other than HEAD's on the enforcement path `{path}` (from {source}); restore HEAD's content instead, or have a person make this change"
        ),
        Verdict::Unknown(why) => {
            format!("cannot tell what `git {verb}` writes on enforcement paths: {why}")
        }
    };
    Some(super::hook_integrity_violation(level, message))
}

/// Whether `verb` is one of the commands this module judges. Cheap: it
/// reads no repository.
fn candidate(verb: &str, rest: &[String]) -> bool {
    match verb {
        "checkout" | "restore" | "checkout-index" | "read-tree" | "apply" | "am"
        | "cherry-pick" | "revert" | "reset" => true,
        "stash" => rest
            .first()
            .is_some_and(|sub| matches!(sub.as_str(), "apply" | "pop" | "branch")),
        _ => false,
    }
}

/// Resolve where the command runs and judge it there. `Ok(None)` when it
/// writes nothing on an enforcement path that differs from HEAD's. `dirs`
/// holds the directory the command runs in and the session's directory.
fn judge_line(
    verb: &str,
    filled: &[String],
    rest: &[String],
    (cwd, session): (&Path, &Path),
    line: &str,
) -> Result<Option<Verdict>, String> {
    let (directory, mut unreadable) = globals_read(filled, rest, cwd);
    if unreadable.is_none() {
        unreadable = MOVING_ENVIRONMENT
            .iter()
            .find(|name| line.contains(*name))
            .map(|name| {
                format!(
                    "the line sets or names `{name}`, which changes which repository, index or objects Git reads"
                )
            });
    }
    let opened = match crate::hooks::repo::open(&directory) {
        Ok(opened) => opened,
        Err(why) if unreadable.is_none() => return Err(why),
        Err(_) => None,
    };
    let workdir = opened
        .as_ref()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf));
    let Some(workdir) = workdir.or_else(|| unreadable.is_some().then(|| directory.clone())) else {
        // Outside a work tree, and with nothing moving where Git looks,
        // Git writes no work-tree file.
        return Ok(None);
    };
    if let (None, Some(repo)) = (&unreadable, &opened) {
        // Replacement refs and grafts change what a stored revision names.
        if let Err(e) = crate::workgraph::history::check(repo) {
            unreadable = Some(format!(
                "stored objects may not be what Git writes: {}",
                e.message()
            ));
        }
    }
    let prefix = if unreadable.is_none() {
        prefix_in(&directory, &workdir)?
    } else {
        Vec::new()
    };
    let place = Place {
        opened: opened.as_ref(),
        unreadable,
        prefix,
        workdir,
        directory,
        session: session.to_path_buf(),
    };
    judge(&place, verb, rest)
}

/// The directory the global options move the command to, and why the guard
/// cannot trust what it would read there, when a global option changes the
/// repository, index or configuration Git reads, or is one it does not read.
fn globals_read(filled: &[String], rest: &[String], cwd: &Path) -> (PathBuf, Option<String>) {
    let globals = &filled[..filled.len() - rest.len() - 1];
    let mut directory = cwd.to_path_buf();
    let mut words = globals.iter();
    while let Some(word) = words.next() {
        let name = word.split('=').next().unwrap_or(word);
        let why = if word == "-c" {
            match words.next() {
                None => Some("`-c` has no setting".to_string()),
                Some(setting) if inert_setting(setting) => None,
                Some(setting) => Some(format!(
                    "the setting `-c {}` can change which work tree, files or content Git writes",
                    setting.split('=').next().unwrap_or(setting)
                )),
            }
        } else if word == "-C" {
            match words.next() {
                None => Some("`-C` has no directory".to_string()),
                Some(value) if expands(value) => Some(format!(
                    "the `-C` directory `{value}` is not a literal path"
                )),
                Some(value) => {
                    directory = integrity_shell_path(value, &directory);
                    None
                }
            }
        } else if MOVING_GLOBALS.contains(&name) || name.starts_with("-c") {
            Some(format!(
                "the global option `{name}` changes which repository, index or configuration Git reads"
            ))
        } else if word == "--icase-pathspecs" || PLAIN_GLOBALS.contains(&word.as_str()) {
            // Pathspecs are always matched without ASCII case here.
            None
        } else {
            Some(format!(
                "the global option `{name}` is not one the guard reads"
            ))
        };
        if why.is_some() {
            return (directory, why);
        }
    }
    (directory, None)
}

/// Configuration keys a `-c` may set without changing which work tree,
/// files or content Git writes: programs run for a person and how output
/// looks. Any other key, an include or a filter among them, is read as
/// moving what Git writes. Programs Git runs are judged by the runner rules.
const INERT_SETTINGS: &[&str] = &[
    "core.editor",
    "core.pager",
    "core.quotepath",
    "sequence.editor",
];

/// Sections whose every key is inert in the same way.
const INERT_SECTIONS: &[&str] = &["advice.", "color.", "pager.", "user."];

/// Whether a `-c` setting is literal and sets an inert key.
fn inert_setting(setting: &str) -> bool {
    if expands(setting) {
        return false;
    }
    let key = setting
        .split('=')
        .next()
        .unwrap_or(setting)
        .to_ascii_lowercase();
    INERT_SETTINGS.contains(&key.as_str())
        || INERT_SECTIONS
            .iter()
            .any(|section| key.starts_with(section))
}

/// The repository and the directory a command runs in.
struct Place<'r> {
    /// The repository found where the command runs, if any.
    opened: Option<&'r git2::Repository>,
    /// Why the guard cannot trust what it would read, when it cannot: a
    /// global option, an environment name or a history overlay. A command
    /// that writes no work-tree file is still judged; one that does refuses
    /// when it reads the repository ([`Place::repo`]) or a patch.
    unreadable: Option<String>,
    workdir: PathBuf,
    /// The directory the command runs in.
    directory: PathBuf,
    /// The session's directory, which the shared path checks read from.
    session: PathBuf,
    /// That directory relative to the work tree, as path components.
    prefix: Vec<String>,
}

impl<'r> Place<'r> {
    /// The repository whose content the command writes, or why the guard
    /// cannot read it.
    fn repo(&self) -> Result<&'r git2::Repository, String> {
        if let Some(why) = &self.unreadable {
            return Err(why.clone());
        }
        self.opened
            .ok_or_else(|| "cannot find the repository the command writes".to_string())
    }
}

fn prefix_in(directory: &Path, workdir: &Path) -> Result<Vec<String>, String> {
    let real = |path: &Path| {
        crate::portable_path::canonicalize(path)
            .map_err(|e| format!("cannot resolve `{}`: {e}", path.display()))
    };
    let (directory, workdir) = (real(directory)?, real(workdir)?);
    let Ok(inside) = directory.strip_prefix(&workdir) else {
        return Ok(Vec::new());
    };
    let mut prefix = Vec::new();
    for component in inside.components() {
        match component.as_os_str().to_str() {
            Some(text) => prefix.push(text.to_string()),
            None => {
                return Err(format!(
                    "the directory `{}` is not UTF-8, so the guard cannot read the pathspecs given there",
                    directory.display()
                ))
            }
        }
    }
    Ok(prefix)
}

/// The judgment for one verb.
fn judge(place: &Place<'_>, verb: &str, rest: &[String]) -> Result<Option<Verdict>, String> {
    match verb {
        "checkout" => checkout(place, rest),
        "restore" => restore(place, rest),
        "checkout-index" => checkout_index(place, rest),
        "read-tree" => read_tree(place, rest),
        "stash" => stash(place, rest),
        "apply" => apply(place, rest),
        "am" => am(place, rest),
        "cherry-pick" | "revert" => pick(place, verb, rest),
        "reset" => reset(place, rest),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Enforcement paths and the entries that can land on them.

/// One repository-relative enforcement path from the action table, and
/// whether it protects a whole directory.
struct Target {
    parts: Vec<&'static str>,
    directory: bool,
}

fn targets() -> Vec<Target> {
    crate::security::actions::table()
        .claude_edit_denies
        .iter()
        .filter_map(|rule| rule.strip_prefix("Edit(/")?.strip_suffix(')'))
        // Git never writes work-tree files under `.git`.
        .filter(|relative| !relative.starts_with(".git/"))
        .map(|relative| {
            let (path, directory) = relative
                .strip_suffix("/**")
                .map_or((relative, false), |prefix| (prefix, true));
            Target {
                parts: path.split('/').collect(),
                directory,
            }
        })
        .collect()
}

/// A stored name component lands on the target component `want` when they
/// match without ASCII case, as a case-insensitive file system writes them.
/// A component with any other character may fold onto it there (Unicode case
/// folding, ignorable code points), so it is taken as a match.
fn lands_on(have: &str, want: &str) -> bool {
    !have.is_ascii() || have.eq_ignore_ascii_case(want)
}

/// Whether a stored path is an ancestor of, equal to, or (for a directory
/// target) inside an enforcement path.
fn relevant(parts: &[&str], targets: &[Target]) -> bool {
    targets.iter().any(|target| {
        let shared = parts.len().min(target.parts.len());
        (0..shared).all(|i| lands_on(parts[i], target.parts[i]))
            && (parts.len() <= target.parts.len() || target.directory)
    })
}

/// Stands in for a stored name component that is not UTF-8 when deciding
/// where it can land. It is not ASCII, so it lands on every target
/// component, as such a name may.
const NOT_UTF8: &str = "\u{fffd}";

/// Why a stored name refuses: a lossy spelling could make two stored names
/// one and hide a change between them.
const NOT_UTF8_NAME: &str = "a stored name that can land on an enforcement path is not UTF-8";

/// The components of a stored path, each read strictly as UTF-8; one that is
/// not reads as [`NOT_UTF8`], and the flag says whether any did.
fn stored_components(bytes: &[u8]) -> (Vec<&str>, bool) {
    let mut strict = true;
    let parts = bytes
        .split(|b| *b == b'/')
        .map(|part| {
            if let Ok(text) = std::str::from_utf8(part) {
                text
            } else {
                strict = false;
                NOT_UTF8
            }
        })
        .collect();
    (parts, !strict)
}

struct Reader<'r> {
    repo: &'r git2::Repository,
    targets: Vec<Target>,
    budget: std::cell::Cell<usize>,
}

impl<'r> Reader<'r> {
    fn new(repo: &'r git2::Repository) -> Self {
        Self {
            repo,
            targets: targets(),
            budget: std::cell::Cell::new(ENTRY_LIMIT),
        }
    }

    fn spend(&self) -> Result<(), String> {
        let left = self.budget.get();
        if left == 0 {
            return Err("the trees hold more entries than the guard reads".into());
        }
        self.budget.set(left - 1);
        Ok(())
    }

    /// The entries of `tree` that can land on an enforcement path.
    fn tree(&self, tree: &git2::Tree<'_>) -> Result<Entries, String> {
        let mut out = Entries::new();
        self.walk(tree, &mut Vec::new(), &mut out)?;
        Ok(out)
    }

    fn walk(
        &self,
        tree: &git2::Tree<'_>,
        prefix: &mut Vec<String>,
        out: &mut Entries,
    ) -> Result<(), String> {
        for entry in tree {
            self.spend()?;
            let (name, not_utf8) = match std::str::from_utf8(entry.name_bytes()) {
                Ok(name) => (name, false),
                Err(_) => (NOT_UTF8, true),
            };
            prefix.push(name.to_string());
            let parts: Vec<&str> = prefix.iter().map(String::as_str).collect();
            if relevant(&parts, &self.targets) {
                if not_utf8 {
                    return Err(NOT_UTF8_NAME.into());
                }
                if entry.kind() == Some(git2::ObjectType::Tree) {
                    let child = self
                        .repo
                        .find_tree(entry.id())
                        .map_err(|e| format!("cannot read a stored tree: {}", e.message()))?;
                    self.walk(&child, prefix, out)?;
                } else {
                    let mode = u32::try_from(entry.filemode()).unwrap_or(u32::MAX);
                    out.insert(parts.join("/"), (entry.id(), mode));
                }
            }
            prefix.pop();
        }
        Ok(())
    }

    /// The tree a revision names, read from the object store. `HEAD` itself
    /// reads as [`Self::head`] does, so on an unborn branch it names the
    /// same empty content HEAD is compared as.
    fn revision(&self, rev: &str) -> Result<Entries, String> {
        if matches!(rev, "HEAD" | "@") {
            return self.head();
        }
        let tree = self.revision_tree(rev)?;
        self.tree(&tree)
    }

    fn revision_tree(&self, rev: &str) -> Result<git2::Tree<'r>, String> {
        if expands(rev) {
            return Err(format!("the revision `{rev}` is not literal"));
        }
        self.repo
            .revparse_single(rev)
            .and_then(|object| object.peel_to_tree())
            .map_err(|e| format!("cannot read the revision `{rev}`: {}", e.message()))
    }

    fn commit(&self, rev: &str) -> Result<git2::Commit<'r>, String> {
        if expands(rev) {
            return Err(format!("the revision `{rev}` is not literal"));
        }
        self.repo
            .revparse_single(rev)
            .and_then(|object| object.peel_to_commit())
            .map_err(|e| format!("cannot read the commit `{rev}`: {}", e.message()))
    }

    /// A commit's parents, read through the guarded history reader, which
    /// refuses grafts, replacement refs and a shallow cut at the commit.
    fn parent_commits(&self, commit: &git2::Commit<'r>) -> Result<Vec<git2::Commit<'r>>, String> {
        let id = commit.id();
        crate::workgraph::history::parents(self.repo, commit)
            .map_err(|e| format!("cannot read the parents of {id}: {}", e.message()))?
            .into_iter()
            .map(|parent| {
                self.repo
                    .find_commit(parent)
                    .map_err(|e| format!("cannot read a parent of {id}: {}", e.message()))
            })
            .collect()
    }

    /// HEAD's entries; none on an unborn branch.
    fn head(&self) -> Result<Entries, String> {
        match self.repo.head() {
            Ok(head) => {
                let tree = head
                    .peel_to_tree()
                    .map_err(|e| format!("cannot read HEAD's tree: {}", e.message()))?;
                self.tree(&tree)
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(Entries::new()),
            Err(e) => Err(format!("cannot read HEAD: {}", e.message())),
        }
    }

    /// The index's entries. An unmerged entry on an enforcement path refuses:
    /// its stages hold content the guard cannot pick between.
    fn index(&self) -> Result<Entries, String> {
        let index = self
            .repo
            .index()
            .map_err(|e| format!("cannot read the index: {}", e.message()))?;
        let mut out = Entries::new();
        for entry in index.iter() {
            self.spend()?;
            let (parts, not_utf8) = stored_components(&entry.path);
            if !relevant(&parts, &self.targets) {
                continue;
            }
            if not_utf8 {
                return Err(NOT_UTF8_NAME.into());
            }
            let path = parts.join("/");
            if entry.flags & 0x3000 != 0 {
                return Err(format!("the index holds an unmerged entry for `{path}`"));
            }
            out.insert(path, (entry.id, entry.mode));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Selections.

/// The paths a command selects.
enum Selection {
    /// Every path.
    Whole,
    /// Pathspecs, each as stored-path components from the work-tree root.
    Specs(Vec<Spec>),
}

struct Spec {
    parts: Vec<String>,
    /// The whole pathspec when it holds a wildcard (`*`, `?`, `[`).
    pattern: Option<String>,
}

impl Selection {
    /// Read pathspecs as Git does: relative to the directory the command
    /// runs in, `.` and directories selecting what lies below them, and
    /// wildcards matching across `/` and leading dots. Anything else (magic,
    /// an escape, an expansion, a path outside the work tree) is the whole
    /// tree.
    fn pathspecs(place: &Place<'_>, words: &[&str]) -> Self {
        let mut specs = Vec::new();
        for word in words {
            match Spec::read(place, word) {
                Some(spec) => specs.push(spec),
                None => return Selection::Whole,
            }
        }
        Selection::Specs(specs)
    }

    fn selects(&self, path: &str) -> bool {
        match self {
            Selection::Whole => true,
            Selection::Specs(specs) => specs.iter().any(|spec| spec.selects(path)),
        }
    }
}

impl Spec {
    fn read(place: &Place<'_>, word: &str) -> Option<Self> {
        if word.is_empty()
            || expands(word)
            || word.starts_with(':')
            || word.starts_with('~')
            || word.contains('\\')
        {
            return None;
        }
        let path = Path::new(word);
        let (mut parts, relative) = if path.is_absolute() {
            let real = crate::portable_path::canonicalize(&place.workdir).ok()?;
            let inside = path
                .strip_prefix(&real)
                .or_else(|_| path.strip_prefix(&place.workdir))
                .ok()?;
            (Vec::new(), inside.to_path_buf())
        } else {
            (place.prefix.clone(), path.to_path_buf())
        };
        for component in relative.components() {
            match component {
                Component::Normal(name) => match name.to_str() {
                    Some(name) => parts.push(name.to_string()),
                    // A name that is not UTF-8 is read as the whole tree.
                    None => return None,
                },
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop()?;
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        let joined = parts.join("/");
        let pattern = joined
            .contains(['*', '?', '['])
            .then(|| joined.to_ascii_lowercase());
        Some(Spec { parts, pattern })
    }

    fn selects(&self, path: &str) -> bool {
        let stored: Vec<&str> = path.split('/').collect();
        if let Some(pattern) = &self.pattern {
            // A wildcard matches the path or any directory above it; a name
            // the guard cannot fold matches.
            if !path.is_ascii() {
                return true;
            }
            let lower = path.to_ascii_lowercase();
            let parts: Vec<&str> = lower.split('/').collect();
            return (1..=parts.len())
                .any(|n| wildmatch(pattern.as_bytes(), parts[..n].join("/").as_bytes()));
        }
        // A literal selects itself and everything below it.
        self.parts.len() <= stored.len()
            && self
                .parts
                .iter()
                .zip(&stored)
                .all(|(want, have)| lands_on(have, want))
    }
}

/// Git's default pathspec wildcard match: `*` matches any run of characters,
/// `/` and leading dots included; `?` and a bracket expression match one
/// character. A bracket expression is read as any one character, which can
/// only select more.
fn wildmatch(pattern: &[u8], text: &[u8]) -> bool {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            b'*' => tokens.push(None),
            b'?' => tokens.push(Some(None)),
            b'[' => {
                // Skip to the closing bracket; an unclosed one is literal.
                let close = pattern[i + 1..]
                    .iter()
                    .skip(1)
                    .position(|&c| c == b']')
                    .map(|at| i + 2 + at);
                if let Some(close) = close {
                    tokens.push(Some(None));
                    i = close;
                } else {
                    tokens.push(Some(Some(b'[')));
                }
            }
            c => tokens.push(Some(Some(c))),
        }
        i += 1;
    }
    // Classic dynamic programming over tokens and text.
    let mut reach = vec![false; text.len() + 1];
    reach[0] = true;
    for token in &tokens {
        let mut next = vec![false; text.len() + 1];
        match token {
            None => {
                let mut on = false;
                for (at, slot) in next.iter_mut().enumerate() {
                    on |= reach[at];
                    *slot = on;
                }
            }
            Some(want) => {
                for at in 0..text.len() {
                    if reach[at] && want.is_none_or(|c| c == text[at]) {
                        next[at + 1] = true;
                    }
                }
            }
        }
        reach = next;
    }
    reach[text.len()]
}

// ---------------------------------------------------------------------------
// Comparisons.

/// The first selected enforcement path whose entry in `source` differs from
/// `head`. In overlay mode a path the source lacks is left alone.
fn differs(
    source: &Entries,
    head: &Entries,
    selection: &Selection,
    overlay: bool,
) -> Option<String> {
    keys(source, head).find_map(|path| {
        let (have, base) = (source.get(path), head.get(path));
        (have != base && !(overlay && have.is_none()) && selection.selects(path))
            .then(|| path.to_string())
    })
}

/// The first enforcement path where `change` differs from `base` and puts
/// something other than `head` there: what applying the change from `base`
/// to `change` on top of HEAD can write.
fn applies(change: &Entries, base: &Entries, head: &Entries) -> Option<String> {
    keys(change, base).find_map(|path| {
        let new = change.get(path);
        (new != base.get(path) && new != head.get(path)).then(|| path.to_string())
    })
}

fn keys<'a>(a: &'a Entries, b: &'a Entries) -> impl Iterator<Item = &'a str> {
    a.keys()
        .chain(b.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
}

/// The finding that the command writes other content on `path`.
fn writes(path: String, source: impl Into<String>) -> Verdict {
    Verdict::Writes {
        path,
        source: source.into(),
    }
}

/// Parse with `spec`, refusing an option the guard does not read.
fn options<'a>(
    verb: &str,
    rest: &'a [String],
    spec: &OptionSpec,
) -> Result<ParsedOptions<'a>, String> {
    let parsed = parse_options(rest, spec);
    if let Some(word) = parsed.unread.first() {
        return Err(format!(
            "`git {verb}` has the option `{word}`, which the guard does not read"
        ));
    }
    Ok(parsed)
}

/// Operands before and after the pathspec separator. After
/// `--end-of-options`, a later `--` is still the separator.
fn split<'a>(parsed: &ParsedOptions<'a>) -> (Vec<&'a str>, Option<Vec<&'a str>>) {
    let at = parsed.separated.map(|at| (at, at)).or_else(|| {
        parsed
            .operands
            .iter()
            .position(|word| *word == "--")
            .map(|at| (at, at + 1))
    });
    match at {
        Some((end, start)) => (
            parsed.operands[..end].to_vec(),
            Some(parsed.operands[start..].to_vec()),
        ),
        None => (parsed.operands.clone(), None),
    }
}

// ---------------------------------------------------------------------------
// The commands.

const CHECKOUT: OptionSpec = OptionSpec {
    short: &[
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('q', Arity::Flag),
        ('f', Arity::Flag),
        ('m', Arity::Flag),
        ('p', Arity::Flag),
        ('t', Arity::Flag),
        ('l', Arity::Flag),
        ('2', Arity::Flag),
        ('3', Arity::Flag),
    ],
    long: &[
        ("--orphan", Arity::Value),
        ("--conflict", Arity::Value),
        ("--pathspec-from-file", Arity::Value),
        ("--pathspec-file-nul", Arity::Flag),
        ("--track", Arity::AttachedValue),
        ("--no-track", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--guess", Arity::Flag),
        ("--no-guess", Arity::Flag),
        ("--force", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--patch", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--ours", Arity::Flag),
        ("--theirs", Arity::Flag),
        ("--overlay", Arity::Flag),
        ("--no-overlay", Arity::Flag),
        ("--ignore-skip-worktree-bits", Arity::Flag),
        ("--ignore-other-worktrees", Arity::Flag),
        ("--overwrite-ignore", Arity::Flag),
        ("--no-overwrite-ignore", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// The last `--overlay` or `--no-overlay` wins; `default` otherwise.
fn overlay(parsed: &ParsedOptions<'_>, default: bool) -> bool {
    parsed
        .long
        .iter()
        .rev()
        .find_map(|(name, _)| match *name {
            "--overlay" => Some(true),
            "--no-overlay" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

/// `git checkout` of paths. A branch switch (`-b`, `-B`, `--orphan`,
/// `--detach`, or one commit and no paths) moves HEAD and is not judged
/// here (TSK-325 AC-7).
fn checkout(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("checkout", rest, &CHECKOUT)?;
    if parsed.has_short(&['b', 'B']) || parsed.has_long("--orphan") || parsed.has_long("--detach") {
        return Ok(None);
    }
    let patch = parsed.has_short(&['p']) || parsed.has_long("--patch");
    let from_file = parsed.has_long("--pathspec-from-file");
    let overlay = overlay(&parsed, true);
    let reader = Reader::new(place.repo()?);
    let head = reader.head()?;
    let whole = patch || from_file;
    let (before, after) = split(&parsed);
    // Each reading Git may take: the source (a revision, else the index)
    // and the words it selects.
    let mut readings: Vec<(Option<&str>, Vec<&str>)> = Vec::new();
    match after {
        Some(paths) => match before.as_slice() {
            [] => readings.push((None, paths)),
            [rev] => {
                if paths.is_empty() && !whole {
                    // `git checkout <commit> --` switches.
                    return Ok(None);
                }
                readings.push((Some(*rev), paths));
            }
            _ => return Ok(None),
        },
        None => match before.split_first() {
            None => {
                if whole {
                    readings.push((None, Vec::new()));
                }
            }
            Some((first, others)) => match reader.revision_tree(first) {
                Ok(_) => {
                    if !others.is_empty() || whole {
                        readings.push((Some(*first), others.to_vec()));
                    }
                    // A word that names a commit and a path reads as the
                    // path from the index too; judging both refuses more.
                    readings.push((None, before.clone()));
                }
                // A revision spelling the guard cannot read: alone it is a
                // branch switch at most; with paths or `-p` it is a source.
                Err(why)
                    if first.contains(['@', '{', '}', '^', '~', ':'])
                        && (!others.is_empty() || whole) =>
                {
                    return Err(why)
                }
                Err(_) => readings.push((None, before.clone())),
            },
        },
    }
    judge_readings(place, &reader, &head, &readings, whole, overlay)
}

fn judge_readings(
    place: &Place<'_>,
    reader: &Reader<'_>,
    head: &Entries,
    readings: &[(Option<&str>, Vec<&str>)],
    whole_when_empty: bool,
    overlay: bool,
) -> Result<Option<Verdict>, String> {
    for (source, words) in readings {
        let selection = if words.is_empty() && whole_when_empty {
            Selection::Whole
        } else if words.is_empty() {
            continue;
        } else {
            Selection::pathspecs(place, words)
        };
        let (entries, name) = match source {
            Some(rev) => (reader.revision(rev)?, format!("the revision `{rev}`")),
            None => (reader.index()?, "the index".to_string()),
        };
        if let Some(path) = differs(&entries, head, &selection, overlay) {
            return Ok(Some(writes(path, name)));
        }
    }
    Ok(None)
}

const RESTORE: OptionSpec = OptionSpec {
    short: &[
        ('s', Arity::Value),
        ('p', Arity::Flag),
        ('W', Arity::Flag),
        ('S', Arity::Flag),
        ('q', Arity::Flag),
        ('m', Arity::Flag),
    ],
    long: &[
        ("--source", Arity::Value),
        ("--patch", Arity::Flag),
        ("--worktree", Arity::Flag),
        ("--staged", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--ours", Arity::Flag),
        ("--theirs", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--conflict", Arity::Value),
        ("--ignore-unmerged", Arity::Flag),
        ("--ignore-skip-worktree-bits", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--overlay", Arity::Flag),
        ("--no-overlay", Arity::Flag),
        ("--pathspec-from-file", Arity::Value),
        ("--pathspec-file-nul", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git restore`: the work tree unless only `--staged` is given; from
/// `--source`, else HEAD when the index is restored too, else the index.
fn restore(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("restore", rest, &RESTORE)?;
    let staged = parsed.has_short(&['S']) || parsed.has_long("--staged");
    let worktree = parsed.has_short(&['W']) || parsed.has_long("--worktree") || !staged;
    if !worktree {
        return Ok(None);
    }
    let patch = parsed.has_short(&['p']) || parsed.has_long("--patch");
    let from_file = parsed.has_long("--pathspec-from-file");
    let source = parsed.values_of('s', "--source").last().copied();
    let source = source.or(staged.then_some("HEAD"));
    let reader = Reader::new(place.repo()?);
    let head = reader.head()?;
    let words: Vec<&str> = parsed.operands.clone();
    judge_readings(
        place,
        &reader,
        &head,
        &[(source, words)],
        patch || from_file,
        overlay(&parsed, false),
    )
}

const CHECKOUT_INDEX: OptionSpec = OptionSpec {
    short: &[
        ('u', Arity::Flag),
        ('q', Arity::Flag),
        ('f', Arity::Flag),
        ('a', Arity::Flag),
        ('n', Arity::Flag),
        ('z', Arity::Flag),
    ],
    long: &[
        ("--index", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--force", Arity::Flag),
        ("--all", Arity::Flag),
        ("--no-create", Arity::Flag),
        ("--prefix", Arity::Value),
        ("--stage", Arity::Value),
        ("--temp", Arity::Flag),
        ("--ignore-skip-worktree-bits", Arity::Flag),
        ("--stdin", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git checkout-index` writes index entries: every one with `-a`, those
/// read from stdin with `--stdin`, otherwise the files named. A `--prefix`
/// is joined to each path as text.
fn checkout_index(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("checkout-index", rest, &CHECKOUT_INDEX)?;
    if parsed.has_long("--temp") || parsed.has_short(&['n']) || parsed.has_long("--no-create") {
        return Ok(None);
    }
    let reader = Reader::new(place.repo()?);
    let index = reader.index()?;
    let all = parsed.has_short(&['a']) || parsed.has_long("--all") || parsed.has_long("--stdin");
    let prefix = parsed
        .values_of('\0', "--prefix")
        .last()
        .copied()
        .unwrap_or("");
    if !prefix.is_empty() {
        return prefixed_index(place, &reader, prefix, all, &parsed.operands);
    }
    let head = reader.head()?;
    let selection = if all {
        Selection::Whole
    } else {
        Selection::pathspecs(place, &parsed.operands)
    };
    Ok(differs(&index, &head, &selection, true).map(|path| writes(path, "the index")))
}

/// With `--prefix`, each index path is written at the prefix joined to it as
/// text, which may be another checkout. Refuse when a joined path is an
/// enforcement path of any checkout, by the shared path check, whatever the
/// content.
fn prefixed_index(
    place: &Place<'_>,
    reader: &Reader<'_>,
    prefix: &str,
    all: bool,
    named: &[&str],
) -> Result<Option<Verdict>, String> {
    if expands(prefix) {
        return Err(format!("the prefix `{prefix}` is not literal"));
    }
    let index = place
        .repo()?
        .index()
        .map_err(|e| format!("cannot read the index: {}", e.message()))?;
    let selection = if all {
        Selection::Whole
    } else {
        Selection::pathspecs(place, named)
    };
    let roots: BTreeSet<String> = reader
        .targets
        .iter()
        .map(|target| target.parts[0].to_ascii_lowercase())
        .collect();
    for entry in index.iter() {
        reader.spend()?;
        let Ok(path) = std::str::from_utf8(&entry.path) else {
            return Err(
                "an index path is not UTF-8, so the guard cannot tell where `--prefix` writes it"
                    .into(),
            );
        };
        if !selection.selects(path) {
            continue;
        }
        let joined = format!("{prefix}{path}");
        // Only a joined path with an enforcement directory name, or a name
        // the guard cannot fold, can be one; the shared check decides.
        let may_land = joined
            .split(['/', '\\'])
            .any(|part| !part.is_ascii() || roots.contains(&part.to_ascii_lowercase()));
        if may_land
            && super::token_integrity_path(&joined, &place.directory, &place.session).is_some()
        {
            return Ok(Some(writes(
                joined,
                format!("the index under the prefix `{prefix}`"),
            )));
        }
    }
    Ok(None)
}

const READ_TREE: OptionSpec = OptionSpec {
    short: &[
        ('m', Arity::Flag),
        ('u', Arity::Flag),
        ('i', Arity::Flag),
        ('n', Arity::Flag),
        ('v', Arity::Flag),
        ('q', Arity::Flag),
    ],
    long: &[
        ("--reset", Arity::Flag),
        ("--trivial", Arity::Flag),
        ("--aggressive", Arity::Flag),
        ("--prefix", Arity::Value),
        ("--index-output", Arity::Value),
        ("--empty", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--no-sparse-checkout", Arity::Flag),
        ("--sparse-checkout", Arity::Flag),
        ("--exclude-per-directory", Arity::Value),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git read-tree -u` writes the work tree from the trees it reads (and, in
/// a merge, the index it keeps). Without `-u` it writes only the index, which
/// a later work-tree write is judged by.
fn read_tree(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("read-tree", rest, &READ_TREE)?;
    if !parsed.has_short(&['u']) || parsed.has_short(&['n']) || parsed.has_long("--dry-run") {
        return Ok(None);
    }
    if parsed.has_long("--prefix") || parsed.has_long("--index-output") {
        return Err("`git read-tree -u` with `--prefix` or `--index-output` writes paths the guard does not model".into());
    }
    let reader = Reader::new(place.repo()?);
    let head = reader.head()?;
    let mut sources: Vec<(Entries, String)> = Vec::new();
    if parsed.has_long("--empty") {
        sources.push((Entries::new(), "an empty tree".into()));
    }
    for rev in &parsed.operands {
        sources.push((reader.revision(rev)?, format!("the revision `{rev}`")));
    }
    if parsed.has_short(&['m']) && !parsed.has_long("--reset") {
        sources.push((reader.index()?, "the index".into()));
    }
    for (entries, name) in sources {
        if let Some(path) = differs(&entries, &head, &Selection::Whole, false) {
            return Ok(Some(writes(path, name)));
        }
    }
    Ok(None)
}

const STASH_APPLY: OptionSpec = OptionSpec {
    short: &[('q', Arity::Flag)],
    long: &[
        ("--index", Arity::Flag),
        ("--quiet", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git stash apply`, `pop` and `branch` write what the stash changed
/// against its base, its index with `--index` (always for `branch`), and its
/// untracked files. `branch` first moves HEAD to the stash's base.
fn stash(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let (sub, args) = rest.split_first().ok_or("no stash subcommand")?;
    let parsed = options("stash", args, &STASH_APPLY)?;
    let mut operands = parsed.operands.clone();
    let branch = sub == "branch";
    if branch {
        if operands.is_empty() {
            return Ok(None);
        }
        operands.remove(0);
    }
    let name = match operands.as_slice() {
        [] => {
            match place.repo()?.find_reference("refs/stash") {
                Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
                Err(e) => return Err(format!("cannot read the stash: {}", e.message())),
                Ok(_) => {}
            }
            "stash@{0}".to_string()
        }
        [one] if one.chars().all(|c| c.is_ascii_digit()) => format!("stash@{{{one}}}"),
        [one] => (*one).to_string(),
        _ => return Ok(None),
    };
    let reader = Reader::new(place.repo()?);
    let stash = reader.commit(&name)?;
    let parents = reader.parent_commits(&stash)?;
    let tree = |commit: &git2::Commit<'_>| -> Result<Entries, String> {
        commit
            .tree()
            .map_err(|e| format!("cannot read a stash tree: {}", e.message()))
            .and_then(|tree| reader.tree(&tree))
    };
    let base_commit = parents
        .first()
        .ok_or_else(|| format!("`{name}` is not a stash: it has no base commit"))?;
    let base = tree(base_commit)?;
    let head = if branch { base.clone() } else { reader.head()? };
    let mut changes = vec![(tree(&stash)?, base.clone(), format!("the stash `{name}`"))];
    if branch || parsed.has_long("--index") {
        let index = parents
            .get(1)
            .ok_or_else(|| format!("`{name}` has no index commit"))?;
        changes.push((tree(index)?, base.clone(), format!("the index of `{name}`")));
    }
    if let Some(untracked) = parents.get(2) {
        changes.push((
            tree(untracked)?,
            Entries::new(),
            format!("the untracked files of `{name}`"),
        ));
    }
    for (change, from, source) in changes {
        if let Some(path) = applies(&change, &from, &head) {
            return Ok(Some(writes(path, source)));
        }
    }
    Ok(None)
}

const APPLY: OptionSpec = OptionSpec {
    short: &[
        ('p', Arity::Value),
        ('C', Arity::Value),
        ('R', Arity::Flag),
        ('v', Arity::Flag),
        ('q', Arity::Flag),
        ('z', Arity::Flag),
        ('N', Arity::Flag),
        ('3', Arity::Flag),
    ],
    long: &[
        ("--stat", Arity::Flag),
        ("--numstat", Arity::Flag),
        ("--summary", Arity::Flag),
        ("--check", Arity::Flag),
        ("--index", Arity::Flag),
        ("--intent-to-add", Arity::Flag),
        ("--cached", Arity::Flag),
        ("--3way", Arity::Flag),
        ("--no-3way", Arity::Flag),
        ("--ours", Arity::Flag),
        ("--theirs", Arity::Flag),
        ("--union", Arity::Flag),
        ("--build-fake-ancestor", Arity::Value),
        ("--reverse", Arity::Flag),
        ("--reject", Arity::Flag),
        ("--unidiff-zero", Arity::Flag),
        ("--apply", Arity::Flag),
        ("--no-add", Arity::Flag),
        ("--allow-binary-replacement", Arity::Flag),
        ("--binary", Arity::Flag),
        ("--exclude", Arity::Value),
        ("--include", Arity::Value),
        ("--ignore-space-change", Arity::Flag),
        ("--ignore-whitespace", Arity::Flag),
        ("--whitespace", Arity::Value),
        ("--inaccurate-eof", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--recount", Arity::Flag),
        ("--directory", Arity::Value),
        ("--unsafe-paths", Arity::Flag),
        ("--allow-empty", Arity::Flag),
        ("--allow-overlap", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git apply` writes the files its patches name unless it only reports
/// (`--stat`, `--numstat`, `--summary`, `--check` without `--apply`) or
/// writes only the index (`--cached`). Any patch that names an enforcement
/// path changes it, so the paths alone decide.
fn apply(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("apply", rest, &APPLY)?;
    let reports = ["--stat", "--numstat", "--summary", "--check"]
        .iter()
        .any(|name| parsed.has_long(name));
    if parsed.has_long("--cached") || (reports && !parsed.has_long("--apply")) {
        return Ok(None);
    }
    let strip = strip_count(parsed.values_of('p', "").last().copied())?;
    let directory = parsed.values_of('\0', "--directory").last().copied();
    patch_files(place, "apply", &parsed.operands, strip, directory)
}

fn strip_count(value: Option<&str>) -> Result<usize, String> {
    value.map_or(Ok(1), |value| {
        value
            .parse()
            .map_err(|_| format!("cannot read the strip count `{value}`"))
    })
}

/// Judge the paths every patch or mailbox file names. Stdin, a directory, a
/// file too large or unreadable, or an encoded mail refuses.
fn patch_files(
    place: &Place<'_>,
    verb: &str,
    files: &[&str],
    strip: usize,
    directory: Option<&str>,
) -> Result<Option<Verdict>, String> {
    if files.is_empty() || files.contains(&"-") {
        return Err(format!(
            "`git {verb}` reads its patch from stdin, which the guard cannot read"
        ));
    }
    let mut texts = Vec::new();
    for file in files {
        if expands(file) {
            return Err(format!("the patch file `{file}` is not a literal path"));
        }
        let path = integrity_shell_path(file, &place.directory);
        texts.push((read_patch(&path)?, (*file).to_string()));
    }
    judge_patches(place, verb, &texts, strip, directory)
}

fn read_patch(path: &Path) -> Result<String, String> {
    let shown = path.display();
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("cannot read the patch `{shown}`: {e}"))?;
    if !metadata.is_file() {
        return Err(format!("the patch `{shown}` is not a regular file"));
    }
    if metadata.len() > PATCH_LIMIT {
        return Err(format!(
            "the patch `{shown}` is larger than the guard reads"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read the patch `{shown}`: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn judge_patches(
    place: &Place<'_>,
    verb: &str,
    texts: &[(String, String)],
    strip: usize,
    directory: Option<&str>,
) -> Result<Option<Verdict>, String> {
    // Patch paths are read from the work tree, which must be the one found.
    place.repo()?;
    let targets = targets();
    for (text, name) in texts {
        for path in patch_paths(text, strip)? {
            let mut parts: Vec<String> = Vec::new();
            if let Some(directory) = directory {
                parts.extend(
                    directory
                        .split('/')
                        .filter(|p| !p.is_empty())
                        .map(str::to_string),
                );
            }
            parts.extend(
                path.split('/')
                    .filter(|p| !p.is_empty() && *p != ".")
                    .map(str::to_string),
            );
            if path.starts_with('/') || parts.iter().any(|part| part == "..") {
                return Err(format!(
                    "the patch `{name}` names the path `{path}` outside the work tree"
                ));
            }
            // The patch path is read from the work-tree root and, to be sure,
            // from the directory the command runs in.
            let mut prefixed = place.prefix.clone();
            prefixed.extend(parts.iter().cloned());
            for candidate in [parts.clone(), prefixed] {
                let refs: Vec<&str> = candidate.iter().map(String::as_str).collect();
                if relevant(&refs, &targets) {
                    return Ok(Some(writes(
                        candidate.join("/"),
                        format!("the patch `{name}` of `git {verb}`"),
                    )));
                }
            }
        }
    }
    Ok(None)
}

/// The paths a unified or Git diff names: both sides of `diff --git`, the
/// `---` and `+++` lines (after `strip` leading components), and renames
/// and copies. A mail body in base64 or quoted-printable refuses, since its
/// diff is not readable as text.
fn patch_paths(text: &str, strip: usize) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let stripped = |path: &str| -> Result<Option<String>, String> {
        // Git ends the name at a tab; trailing spaces are not part of it.
        let path = path
            .split('\t')
            .next()
            .unwrap_or(path)
            .trim_end_matches(' ');
        if path == "/dev/null" {
            return Ok(None);
        }
        let path = unquote(path).ok_or("cannot read a quoted patch path")?;
        let parts: Vec<&str> = path.split('/').collect();
        Ok(Some(
            parts
                .get(strip..)
                .map_or_else(String::new, |rest| rest.join("/")),
        ))
    };
    // Patch lines end at a newline; a mail may also carry a carriage return.
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-transfer-encoding:")
            && (lower.contains("base64") || lower.contains("quoted-printable"))
        {
            return Err("a mail in the patch is encoded, so its paths cannot be read".into());
        }
        if let Some(both) = line.strip_prefix("diff --git ") {
            // Split at " b/" when unquoted; otherwise read quoted names.
            for name in diff_git_names(both)? {
                paths.extend(stripped(&name)?);
            }
        } else if let Some(path) = line
            .strip_prefix("--- ")
            .or_else(|| line.strip_prefix("+++ "))
        {
            paths.extend(stripped(path)?);
        } else if let Some(path) = ["rename from ", "rename to ", "copy from ", "copy to "]
            .iter()
            .find_map(|head| line.strip_prefix(head))
        {
            paths.push(unquote(path).ok_or("cannot read a quoted rename path")?);
        }
    }
    paths.retain(|path| !path.is_empty());
    Ok(paths)
}

fn diff_git_names(both: &str) -> Result<Vec<String>, String> {
    if both.starts_with('"') || both.contains(" \"") {
        // Quoted names: read each quoted or bare word.
        let mut names = Vec::new();
        let mut rest = both.trim_matches(' ');
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('"') {
                let end = closing_quote(after).ok_or("cannot read a quoted diff path")?;
                names.push(format!("\"{}\"", &after[..end]));
                rest = after[end + 1..].trim_start_matches(' ');
            } else {
                let end = rest.find(' ').unwrap_or(rest.len());
                names.push(rest[..end].to_string());
                rest = rest[end..].trim_start_matches(' ');
            }
        }
        return Ok(names);
    }
    // `a/x b/y`: both halves are judged; any split point reads both names.
    Ok(both
        .match_indices(' ')
        .map(|(at, _)| both[..at].to_string())
        .chain(
            both.match_indices(' ')
                .map(|(at, _)| both[at + 1..].to_string()),
        )
        .collect())
}

fn closing_quote(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// A Git C-quoted path, or the path as written.
fn unquote(path: &str) -> Option<String> {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return Some(path.to_string());
    };
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = *bytes.get(i + 1)?;
        i += 2;
        out.push(match next {
            b'n' => b'\n',
            b't' => b'\t',
            b'"' => b'"',
            b'\\' => b'\\',
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'r' => b'\r',
            b'v' => 11,
            b'0'..=b'3' => {
                let digits = inner.get(i - 1..i + 2)?;
                i += 2;
                u8::from_str_radix(digits, 8).ok()?
            }
            _ => return None,
        });
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

const AM: OptionSpec = OptionSpec {
    short: &[
        ('s', Arity::Flag),
        ('k', Arity::Flag),
        ('c', Arity::Flag),
        ('m', Arity::Flag),
        ('q', Arity::Flag),
        ('u', Arity::Flag),
        ('3', Arity::Flag),
        ('i', Arity::Flag),
        ('n', Arity::Flag),
        ('r', Arity::Flag),
        ('C', Arity::Value),
        ('p', Arity::Value),
        ('S', Arity::AttachedValue),
    ],
    long: &[
        ("--signoff", Arity::Flag),
        ("--keep", Arity::Flag),
        ("--keep-non-patch", Arity::Flag),
        ("--keep-cr", Arity::Flag),
        ("--no-keep-cr", Arity::Flag),
        ("--scissors", Arity::Flag),
        ("--no-scissors", Arity::Flag),
        ("--message-id", Arity::Flag),
        ("--no-message-id", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--utf8", Arity::Flag),
        ("--no-utf8", Arity::Flag),
        ("--3way", Arity::Flag),
        ("--no-3way", Arity::Flag),
        ("--rerere-autoupdate", Arity::Flag),
        ("--no-rerere-autoupdate", Arity::Flag),
        ("--ignore-space-change", Arity::Flag),
        ("--ignore-whitespace", Arity::Flag),
        ("--whitespace", Arity::Value),
        ("--directory", Arity::Value),
        ("--exclude", Arity::Value),
        ("--include", Arity::Value),
        ("--reject", Arity::Flag),
        ("--patch-format", Arity::Value),
        ("--interactive", Arity::Flag),
        ("--committer-date-is-author-date", Arity::Flag),
        ("--ignore-date", Arity::Flag),
        ("--skip", Arity::Flag),
        ("--gpg-sign", Arity::AttachedValue),
        ("--no-gpg-sign", Arity::Flag),
        ("--continue", Arity::Flag),
        ("--resolved", Arity::Flag),
        ("--resolvemsg", Arity::Value),
        ("--abort", Arity::Flag),
        ("--quit", Arity::Flag),
        ("--retry", Arity::Flag),
        ("--show-current-patch", Arity::AttachedValue),
        ("--allow-empty", Arity::Flag),
        ("--empty", Arity::Value),
        ("--quoted-cr", Arity::Value),
        ("--no-verify", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git am` commits each mail's patch. It is judged by the paths its patches
/// name: the mailbox files, or for `--continue`, `--skip`, `--resolved` and
/// `--retry` the session in progress.
fn am(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = options("am", rest, &AM)?;
    if ["--abort", "--quit", "--show-current-patch"]
        .iter()
        .any(|name| parsed.has_long(name))
    {
        return Ok(None);
    }
    let strip = strip_count(parsed.values_of('p', "").last().copied())?;
    let directory = parsed.values_of('\0', "--directory").last().copied();
    let resumes = parsed.has_short(&['r'])
        || ["--continue", "--skip", "--resolved", "--retry"]
            .iter()
            .any(|name| parsed.has_long(name));
    if resumes {
        let session = place.repo()?.path().join("rebase-apply");
        let unread = |e: std::io::Error| format!("cannot read the am session: {e}");
        // Only a proven absent session means there is nothing to resume.
        if crate::absence::symlink_metadata_optional(&session)
            .map_err(unread)?
            .is_none()
        {
            return Ok(None);
        }
        let mut texts = Vec::new();
        for entry in std::fs::read_dir(&session).map_err(unread)? {
            let entry = entry.map_err(unread)?;
            let file_name = entry.file_name();
            // Git names its patch files `0001` and so on, and `patch`; a
            // name that is not UTF-8 is none of them.
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.bytes().all(|b| b.is_ascii_digit()) || name == "patch" {
                texts.push((read_patch(&entry.path())?, name.to_string()));
            }
        }
        return judge_patches(place, "am", &texts, strip, directory);
    }
    patch_files(place, "am", &parsed.operands, strip, directory)
}

const PICK: OptionSpec = OptionSpec {
    short: &[
        ('e', Arity::Flag),
        ('x', Arity::Flag),
        ('n', Arity::Flag),
        ('s', Arity::Flag),
        ('m', Arity::Value),
        ('S', Arity::AttachedValue),
        ('X', Arity::Value),
    ],
    long: &[
        ("--edit", Arity::Flag),
        ("--no-edit", Arity::Flag),
        ("--cleanup", Arity::Value),
        ("--mainline", Arity::Value),
        ("--no-commit", Arity::Flag),
        ("--signoff", Arity::Flag),
        ("--gpg-sign", Arity::AttachedValue),
        ("--no-gpg-sign", Arity::Flag),
        ("--ff", Arity::Flag),
        ("--allow-empty", Arity::Flag),
        ("--allow-empty-message", Arity::Flag),
        ("--keep-redundant-commits", Arity::Flag),
        ("--empty", Arity::Value),
        ("--strategy", Arity::Value),
        ("--strategy-option", Arity::Value),
        ("--rerere-autoupdate", Arity::Flag),
        ("--no-rerere-autoupdate", Arity::Flag),
        ("--reference", Arity::Flag),
        ("--continue", Arity::Flag),
        ("--skip", Arity::Flag),
        ("--abort", Arity::Flag),
        ("--quit", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git cherry-pick -n` and `git revert -n` apply commits to the work tree
/// and index without committing. A committing pick or revert moves HEAD and
/// keeps its verdict (TSK-325 AC-7).
fn pick(place: &Place<'_>, verb: &str, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = parse_options(rest, &PICK);
    let no_commit = parsed.has_short(&['n']) || parsed.has_long("--no-commit");
    let resumes = parsed.has_long("--continue") || parsed.has_long("--skip");
    if resumes {
        // A sequence started with `-n` continues without committing; its
        // remaining commits sit in the sequencer, which the guard does not
        // replay.
        let opts = place.repo()?.path().join("sequencer/opts");
        let unread = |e: std::io::Error| format!("cannot read the sequencer state: {e}");
        if crate::absence::symlink_metadata_optional(&opts)
            .map_err(unread)?
            .is_none()
        {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&opts).map_err(unread)?;
        return if text.contains("no-commit") {
            Err(format!(
                "`git {verb}` would continue a no-commit sequence the guard cannot replay; use `git {verb} --abort`"
            ))
        } else {
            Ok(None)
        };
    }
    if !no_commit {
        return Ok(None);
    }
    if let Some(word) = parsed.unread.first() {
        return Err(format!(
            "`git {verb}` has the option `{word}`, which the guard does not read"
        ));
    }
    let reader = Reader::new(place.repo()?);
    let head = reader.head()?;
    for commit in picked(place.repo()?, &parsed.operands)? {
        let id = commit.id().to_string();
        let changed = commit
            .tree()
            .map_err(|e| format!("cannot read the commit {id}: {}", e.message()))
            .and_then(|tree| reader.tree(&tree))?;
        let mut parents = Vec::new();
        for parent in reader.parent_commits(&commit)? {
            let tree = parent
                .tree()
                .map_err(|e| format!("cannot read a parent of {id}: {}", e.message()))?;
            parents.push(reader.tree(&tree)?);
        }
        if parents.is_empty() {
            parents.push(Entries::new());
        }
        for parent in &parents {
            let found = if verb == "revert" {
                applies(parent, &changed, &head)
            } else {
                applies(&changed, parent, &head)
            };
            if let Some(path) = found {
                return Ok(Some(writes(path, format!("the commit {id}"))));
            }
        }
    }
    Ok(None)
}

/// The commits a pick names: each operand alone, or a walk when any operand
/// uses range notation (`A..B`, `^A`).
fn picked<'r>(
    repo: &'r git2::Repository,
    operands: &[&str],
) -> Result<Vec<git2::Commit<'r>>, String> {
    if operands.is_empty() {
        return Err("no commit to pick".into());
    }
    for word in operands {
        if expands(word) || word.contains("...") || *word == "-" || word.starts_with('-') {
            return Err(format!("cannot read the commit range `{word}`"));
        }
    }
    let ranged = operands
        .iter()
        .any(|word| word.contains("..") || word.starts_with('^'));
    let read = |rev: &str| {
        repo.revparse_single(rev)
            .and_then(|object| object.peel_to_commit())
            .map_err(|e| format!("cannot read the commit `{rev}`: {}", e.message()))
    };
    if !ranged {
        return operands.iter().map(|word| read(word)).collect();
    }
    // The guarded walk refuses grafts, replacement refs and a shallow cut.
    let mut walk = crate::workgraph::history::revwalk(repo)
        .map_err(|e| format!("cannot walk commits: {}", e.message()))?;
    let side = |rev: &str| read(if rev.is_empty() { "HEAD" } else { rev });
    for word in operands {
        let result = if let Some(hidden) = word.strip_prefix('^') {
            walk.hide(read(hidden)?.id())
        } else if let Some((from, to)) = word.split_once("..") {
            let (from, to) = (side(from)?.id(), side(to)?.id());
            walk.hide(from).and_then(|()| walk.push(to))
        } else {
            walk.push(read(word)?.id())
        };
        result.map_err(|e| format!("cannot read the commit range `{word}`: {}", e.message()))?;
    }
    let mut commits = Vec::new();
    for id in walk {
        let id = id.map_err(|e| format!("cannot walk commits: {}", e.message()))?;
        if commits.len() == COMMIT_LIMIT {
            return Err("the range holds more commits than the guard reads".into());
        }
        commits.push(
            repo.find_commit(id)
                .map_err(|e| format!("cannot read the commit {id}: {}", e.message()))?,
        );
    }
    Ok(commits)
}

const RESET: OptionSpec = OptionSpec {
    short: &[('q', Arity::Flag), ('N', Arity::Flag), ('p', Arity::Flag)],
    long: &[
        ("--hard", Arity::Flag),
        ("--soft", Arity::Flag),
        ("--mixed", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--keep", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--no-quiet", Arity::Flag),
        ("--refresh", Arity::Flag),
        ("--no-refresh", Arity::Flag),
        ("--intent-to-add", Arity::Flag),
        ("--patch", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--pathspec-from-file", Arity::Value),
        ("--pathspec-file-nul", Arity::Flag),
        (super::END_OF_OPTIONS, Arity::Flag),
    ],
    git_style: true,
};

/// `git reset --hard`, `--keep` and `--merge` to a revision write that
/// revision's files in the work tree. Without a revision they restore
/// HEAD's.
fn reset(place: &Place<'_>, rest: &[String]) -> Result<Option<Verdict>, String> {
    let parsed = parse_options(rest, &RESET);
    let moves = ["--hard", "--keep", "--merge"]
        .iter()
        .any(|name| parsed.has_long(name));
    if !moves {
        return Ok(None);
    }
    if let Some(word) = parsed.unread.first() {
        return Err(format!(
            "`git reset` has the option `{word}`, which the guard does not read"
        ));
    }
    let (before, _) = split(&parsed);
    let Some(rev) = before.first() else {
        return Ok(None);
    };
    let reader = Reader::new(place.repo()?);
    let head = reader.head()?;
    let target = reader.revision(rev)?;
    Ok(differs(&target, &head, &Selection::Whole, false)
        .map(|path| writes(path, format!("the revision `{rev}`"))))
}

#[cfg(test)]
mod tests;
