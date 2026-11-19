//! What a recursive copy or sync reaches under its destination (TSK-408).
//!
//! Both shell guards read a `cp -R`, `scp -r`, `rsync` or `ditto` call by
//! the content it moves, not only by its destination word. For each source
//! the reader works out where the source's contents land (its landing
//! place), lists the enforcement paths at or below that place
//! ([`crate::hooks::edit_guard::enforcement_reached`]), and checks whether
//! the source holds an entry on the way to one of them. A deleting sync
//! refuses whenever its landing place holds one, and a source-removing sync
//! whenever its source does. A landing place that holds none keeps the
//! verdict it had, and its source is never read.
//!
//! The probe reads one directory listing per source, never a whole tree:
//! any entry of the source whose name is the first part of an enforcement
//! path's place below the landing place (`.claude` for
//! `.claude/settings.json`, `.git` for a checkout pointer) would create or
//! replace something on that path. Names are compared ignoring case, so a
//! case-insensitive destination is covered.
//!
//! A source the guard cannot read (missing, unreadable, filled in at run
//! time, a pattern, a remote spelling) refuses when its landing place holds
//! an enforcement path, and so does an option the guard does not read. A
//! destination filled in at run time refuses when the source holds an
//! enforcement name at any depth, read through its symbolic links within a
//! bound of 4096 directories, or cannot be read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::unresolved::CopyRead;
use crate::hooks::edit_guard::{enforcement_reached, holds_enforcement_files, Reached};

/// How many directories the walk of a source under a run-time destination
/// reads before it gives up, the bound the repository reader uses.
const WALK_LIMIT: usize = 4096;

/// The names that start an enforcement path inside a checkout, compared
/// ignoring case: the checkout pointer and each enforcement directory.
const ENFORCEMENT_NAMES: &[&str] = &[".git", ".codeflow", ".claude", ".github", ".codex", ".grok"];

/// Whether `flag` is one of rsync's source-removing options or an
/// abbreviation of one (`--remove-source-files`, `--remove-sent-files`).
pub(crate) fn rsync_removes_source(flag: &str) -> bool {
    flag.starts_with("--")
        && flag.len() > 2
        && ["--remove-source-files", "--remove-sent-files"]
            .iter()
            .any(|full| full.starts_with(flag))
}

/// Whether `flag` makes rsync delete in its destination: `--del`, any
/// option whose name starts with `--delete`, and an abbreviation of
/// `--delete` (`--dele`).
pub(crate) fn rsync_deletes(flag: &str) -> bool {
    flag.len() >= 5 && (flag.starts_with("--delete") || "--delete".starts_with(flag))
}

/// Whether an rsync call only reports what it would do. An option the
/// reader does not know may take a value (`-e` and `-M` take the rest of
/// their cluster or the next word), so after one a later `-n` may be that
/// value, and the call is never read as a dry run.
pub(crate) fn dry_run(copy: &CopyRead) -> bool {
    copy.unknown.is_none() && copy.flags.iter().any(|f| f == "-n" || f == "--dry-run")
}

/// How a host's filesystem reads the names a copy or link makes. Every
/// entry or landing name the shell guards compare against an enforcement
/// name goes through [`Host::opened`], the reading the path word gets from
/// `portable_path`: the host's separators, then on Windows the
/// alternate-stream refusal of `namespace_spelling`, the stream strip of
/// `without_streams`, the trailing dot or space `filesystem_spelling`
/// drops, and the short-name refusal. The Windows reading is a value, so
/// every host tests it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Host {
    windows: bool,
}

impl Host {
    /// The host the guard runs on.
    pub(crate) const THIS: Host = Host {
        windows: cfg!(windows),
    };
    /// Windows, for tests on every host.
    #[cfg(test)]
    pub(crate) const WINDOWS: Host = Host { windows: true };
    /// A host where `\`, `:` and a trailing dot are parts of a name.
    #[cfg(test)]
    pub(crate) const POSIX: Host = Host { windows: false };

    /// The separators of names: `/` and `\` on Windows, `/` elsewhere.
    pub(crate) fn separators(self) -> &'static [char] {
        if self.windows {
            WINDOWS_SEPARATORS
        } else {
            &['/']
        }
    }

    /// The last name of `source`, as written: the name a copy of it gets
    /// inside a directory, `source` itself when it has no separator, and
    /// on Windows without the drive of a drive-relative word (`C:name`).
    pub(crate) fn last_name(self, source: &str) -> &str {
        let name = last_name_on(source, self.separators());
        let bytes = name.as_bytes();
        let first = name.len() == source.trim_end_matches(self.separators()).len();
        if self.windows
            && first
            && bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
        {
            &name[2..]
        } else {
            name
        }
    }

    /// The name the filesystem opens for the entry name `name`, with its
    /// case kept. On Windows a named stream (`config:notes`) opens the file
    /// before its first `:`, and a trailing dot or space is dropped. `Err`
    /// when the guard cannot map the name: stream type syntax (`::`, `:$`),
    /// which `namespace_spelling` refuses, a name that is only a stream or
    /// only dots, or a name Windows may read as the short name of another
    /// entry.
    pub(crate) fn opened(self, name: &str) -> Result<String, String> {
        if !self.windows || matches!(name, "." | "..") {
            return Ok(name.to_string());
        }
        let lower = name.to_ascii_lowercase();
        if lower.contains("::") || lower.contains(":$") {
            return Err(format!(
                "`{name}` uses Windows alternate-stream syntax, which the guard cannot map"
            ));
        }
        let file = name.split(':').next().unwrap_or(name);
        let file = file.trim_end_matches(['.', ' ']);
        if file.is_empty() {
            return Err(format!("`{name}` names no file the guard can map"));
        }
        if crate::portable_path::has_short_component(file) {
            return Err(format!(
                "`{name}` may be read by Windows as the short name of another entry"
            ));
        }
        Ok(file.to_string())
    }

    /// [`Host::opened`] with case ignored, as names are compared.
    pub(crate) fn compared(self, name: &str) -> Result<String, String> {
        self.opened(name).map(|name| name.to_lowercase())
    }

    /// The entry a copy or link of `source` makes in a directory, named as
    /// the filesystem opens it; `None` for `.`, `..` and a root, whose
    /// contents are copied.
    pub(crate) fn entry_name(self, source: &str) -> Option<Result<String, String>> {
        let name = self.last_name(source);
        (!name.is_empty() && name != "." && name != "..").then(|| self.opened(name))
    }
}

/// A name as the guard compares it on this host ([`Host::compared`]).
fn compared(name: &str) -> Result<String, String> {
    Host::THIS.compared(name)
}

/// Whether a source's own name is an enforcement name, compared as the
/// filesystem opens it; `Err` when the guard cannot map the name.
fn enforcement_name(name: Option<&str>) -> Result<bool, String> {
    let Some(name) = name else {
        return Ok(false);
    };
    Ok(ENFORCEMENT_NAMES.contains(&compared(name)?.as_str()))
}

/// Whether a long option `flag` is `full` or an abbreviation of it.
fn long_is(flag: &str, full: &str) -> bool {
    flag.len() > 3 && flag.starts_with("--") && full.starts_with(flag)
}

/// Whether a `cp` or `scp` call copies directories.
fn recursive(cmd: &str, flags: &[String]) -> bool {
    match cmd {
        "rsync" | "ditto" => true,
        "cp" => flags.iter().any(|f| {
            matches!(f.as_str(), "-R" | "-r" | "-a")
                || long_is(f, "--recursive")
                || long_is(f, "--archive")
        }),
        "scp" => flags.iter().any(|f| f == "-r"),
        _ => false,
    }
}

/// Where the guard reads paths: how a word resolves (`None` when the
/// shell fills it in at run time or the directory is unknown) and the
/// session roots whose repositories hold the enforcement paths.
pub(crate) struct Place<'a> {
    pub(crate) resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub(crate) roots: &'a [&'a Path],
}

/// A source as the guard can read it.
enum Source {
    /// A directory: its entries' names lowercased, and as they are.
    Tree(HashSet<String>, Vec<std::ffi::OsString>),
    /// A file or another non-directory entry.
    Entry,
    /// Why the guard cannot read it.
    Unread(String),
}

/// Whether `word` names a remote place for rsync or scp.
fn remote(word: &str) -> bool {
    word.starts_with("rsync://")
        || word
            .split('/')
            .next()
            .is_some_and(|first| first.contains(':') && !first.is_empty())
}

/// Whether the shell may expand `word` as a pathname pattern.
fn pattern(word: &str) -> bool {
    word.contains(['*', '?', '['])
}

/// The entry a copy of `source` makes in a directory, named as this host's
/// filesystem opens it ([`Host::entry_name`]); `None` for `.`, `..` and a
/// root, whose contents are copied; `Err` when the guard cannot map it.
fn base_name(source: &str) -> Option<Result<String, String>> {
    Host::THIS.entry_name(source)
}

/// [`last_name`] on `separators`, so each host's reading is tested on
/// every host.
pub(crate) fn last_name_on<'a>(source: &'a str, separators: &[char]) -> &'a str {
    source
        .trim_end_matches(separators)
        .rsplit(separators)
        .next()
        .unwrap_or(source)
}

/// The path separators Windows reads, as `portable_path::filesystem_spelling`
/// does: a backslash separates names there.
pub(crate) const WINDOWS_SEPARATORS: &[char] = &['/', '\\'];

/// The path separators of this host: a backslash separates names only on
/// Windows, and elsewhere it is part of a name.
pub(crate) const SEPARATORS: &[char] = if cfg!(windows) {
    WINDOWS_SEPARATORS
} else {
    &['/']
};

/// Whether `source` names its contents rather than itself: it ends in a
/// separator or a separator and `.`, or is `.`.
fn names_contents(source: &str) -> bool {
    names_contents_on(source, SEPARATORS)
}

/// [`names_contents`] on `separators`.
fn names_contents_on(source: &str, separators: &[char]) -> bool {
    source == "."
        || source.ends_with(separators)
        || source
            .strip_suffix('.')
            .is_some_and(|rest| rest.ends_with(separators))
}

/// `name` inside the directory `dest`, whose trailing separators are read
/// on this host's separators.
fn joined(dest: &str, name: &str) -> String {
    format!("{}/{name}", dest.trim_end_matches(SEPARATORS))
}

/// The places the contents of `source` land, each as spelled (for the
/// message) and as a path, for a copy to `dest`.
fn landings(
    cmd: &str,
    copy: &CopyRead,
    source: &str,
    dest: &str,
    place: &Place<'_>,
) -> Vec<String> {
    let join = |name: &str| joined(dest, name);
    let contents = names_contents(source);
    let named = base_name(source);
    let mut out = Vec::new();
    match cmd {
        "rsync" => {
            // A name the guard cannot map lands at the destination itself,
            // which holds every place below it.
            match &named {
                Some(Ok(name)) if !contents => out.push(join(name)),
                _ => out.push(dest.to_string()),
            }
            // `-R` keeps the source's path below the destination.
            if copy
                .flags
                .iter()
                .any(|f| f == "-R" || long_is(f, "--relative"))
            {
                let marked = source
                    .split_once("/./")
                    .or_else(|| cfg!(windows).then(|| source.split_once("\\.\\")).flatten())
                    .map_or(source, |(_, rest)| rest);
                let kept = marked
                    .trim_start_matches(SEPARATORS)
                    .trim_end_matches(SEPARATORS);
                if !kept.is_empty() {
                    // The kept path's last name is read as the filesystem
                    // opens it; one the guard cannot map keeps the
                    // destination itself.
                    let last = last_name_on(kept, SEPARATORS);
                    match Host::THIS.opened(last) {
                        Ok(opened) => {
                            let parent = &kept[..kept.len() - last.len()];
                            out.push(join(&format!("{parent}{opened}")));
                        }
                        Err(_) => out.push(dest.to_string()),
                    }
                }
            }
        }
        "ditto" => {
            out.push(dest.to_string());
            if let Some(Ok(name)) = &named {
                out.push(join(name));
            }
        }
        _ => {
            let no_target = copy
                .flags
                .iter()
                .any(|f| f == "-T" || long_is(f, "--no-target-directory"));
            match &named {
                None | Some(Err(_)) => out.push(dest.to_string()),
                Some(Ok(name)) if copy.target => out.push(join(name)),
                Some(_) if no_target => out.push(dest.to_string()),
                Some(Ok(name)) => {
                    // Into an existing directory a source lands at its name
                    // there, and BSD cp copies the contents of `src/`; an
                    // absent destination takes the contents. A destination
                    // whose kind cannot be read gets both.
                    let kind =
                        (place.resolve)(dest).map(|path| crate::absence::is_kind(&path, true));
                    let unknown = copy.sources.len() == 1 && !matches!(kind, Some(Some(_)));
                    let directory = copy.sources.len() > 1 || kind == Some(Some(true));
                    if directory || unknown {
                        out.push(join(name));
                    }
                    if contents || !directory || unknown {
                        out.push(dest.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Read `source` once: the names of its entries when it is a directory.
fn read_source(cmd: &str, source: &str, place: &Place<'_>) -> (Source, Option<PathBuf>) {
    if matches!(cmd, "rsync" | "scp") && remote(source) {
        return (Source::Unread("a remote place".into()), None);
    }
    if pattern(source) {
        return (Source::Unread("a pattern the shell expands".into()), None);
    }
    let Some(path) = (place.resolve)(source) else {
        return (
            Source::Unread("filled in at run time or relative to an unknown directory".into()),
            None,
        );
    };
    let read = match crate::absence::metadata_optional(&path) {
        Ok(Some(metadata)) if metadata.is_dir() => match std::fs::read_dir(&path) {
            Ok(entries) => {
                let mut lower = HashSet::new();
                let mut names = Vec::new();
                let mut failed = None;
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let name = entry.file_name();
                            let text = name.to_string_lossy();
                            match compared(&text) {
                                Ok(compared) => {
                                    lower.insert(compared);
                                }
                                Err(why) => failed = Some(format!("it holds {why}")),
                            }
                            names.push(name);
                        }
                        Err(error) => failed = Some(format!("cannot list it: {error}")),
                    }
                }
                failed.map_or(Source::Tree(lower, names), Source::Unread)
            }
            Err(error) => Source::Unread(format!("cannot list it: {error}")),
        },
        Ok(Some(_)) => Source::Entry,
        Ok(None) => Source::Unread("it does not exist yet".into()),
        Err(error) => Source::Unread(format!("cannot read it: {error}")),
    };
    (read, Some(path))
}

/// What every root's repositories hold at or below `base`.
fn reached(base: &Path, place: &Place<'_>) -> Result<Reached, String> {
    let mut all = Reached::default();
    for root in place.roots {
        let found = enforcement_reached(base, root)?;
        if all.inside.is_none() {
            all.inside = found.inside;
        }
        for relative in found.below {
            if !all.below.contains(&relative) {
                all.below.push(relative);
            }
        }
    }
    Ok(all)
}

/// The enforcement name `dir` holds at any depth, read within
/// [`WALK_LIMIT`] directories. Symbolic links are followed, since a copy
/// that follows them writes their targets; a link that cannot be read
/// refuses.
fn holds_enforcement_name(dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut pending = vec![dir.to_path_buf()];
    let mut read = 0;
    while let Some(next) = pending.pop() {
        read += 1;
        if read > WALK_LIMIT {
            return Err(format!("cannot read it beyond {WALK_LIMIT} directories"));
        }
        let entries = std::fs::read_dir(&next).map_err(|e| format!("cannot list it: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot list it: {e}"))?;
            let text = entry.file_name().to_string_lossy().into_owned();
            let name = compared(&text).map_err(|why| format!("it holds {why}"))?;
            if ENFORCEMENT_NAMES.contains(&name.as_str()) {
                return Ok(Some(entry.path()));
            }
            let kind = entry
                .file_type()
                .map_err(|e| format!("cannot read it: {e}"))?;
            // A copy that follows links (`cp -RL`, `rsync -L`, `scp -r`)
            // writes a linked directory's contents, so the walk reads
            // through every link. A link that cannot be read refuses, and
            // WALK_LIMIT ends a cycle with a refusal.
            let directory = if kind.is_symlink() {
                std::fs::metadata(entry.path())
                    .map_err(|e| {
                        format!("cannot read the link {}: {e}", entry.path().display())
                    })?
                    .is_dir()
            } else {
                kind.is_dir()
            };
            if directory {
                pending.push(entry.path());
            }
        }
    }
    Ok(None)
}

/// A symbolic link at the landing place `base` that an entry of the source
/// would be written through, when it leads to or above an enforcement
/// path: `cp -R` follows a link to a directory it copies into. Only the
/// landing place's own entries are read; a link deeper in the destination
/// is the residual the policy reference lists.
fn through_link(
    cmd: &str,
    source: &str,
    base: &Path,
    landing: &str,
    names: &[std::ffi::OsString],
    place: &Place<'_>,
) -> Option<String> {
    for name in names {
        let at = base.join(name);
        let Ok(Some(metadata)) = crate::absence::symlink_metadata_optional(&at) else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let shown = format!(
            "{}/{}",
            landing.trim_end_matches(SEPARATORS),
            name.to_string_lossy()
        );
        let target = match std::fs::canonicalize(&at) {
            Ok(target) => target,
            Err(error) => {
                return Some(format!(
                    "`{cmd}` writes the source `{source}` through the link `{shown}`, which the guard cannot resolve: {error}"
                ))
            }
        };
        match reached(&target, place) {
            Ok(reach) if reach.is_empty() => {}
            Ok(_) => {
                return Some(format!(
                    "`{cmd}` writes the source `{source}` through the link `{shown}`, which leads to enforcement files"
                ))
            }
            Err(error) => {
                return Some(format!(
                    "`{cmd}` writes the source `{source}` through the link `{shown}`, which the guard cannot verify: {error}"
                ))
            }
        }
    }
    None
}

/// The first rsync option in `flags` that `test` matches.
fn flag_of(flags: &[String], test: fn(&str) -> bool) -> Option<&str> {
    flags.iter().map(String::as_str).find(|f| test(f))
}

/// Why a source-removing sync given `flag` removes enforcement files, or
/// cannot be shown not to, for any of its local `sources`.
fn removed_source(cmd: &str, flag: &str, sources: &[String], place: &Place<'_>) -> Option<String> {
    for source in sources {
        if remote(source) {
            continue;
        }
        let Some(path) = (place.resolve)(source) else {
            return Some(format!(
                "`{cmd} {flag}` removes its source `{source}`, which the guard cannot resolve"
            ));
        };
        for root in place.roots {
            match holds_enforcement_files(&path, root).and_then(|holds| {
                Ok(holds
                    || crate::hooks::edit_guard::repository_enforcement_target(&path, root, false)?)
            }) {
                Ok(false) => {}
                Ok(true) => {
                    return Some(format!(
                        "`{cmd} {flag}` removes its source `{source}`, which is or holds enforcement files"
                    ))
                }
                Err(error) => {
                    return Some(format!(
                        "`{cmd} {flag}` removes its source `{source}`, which the guard cannot read: {error}"
                    ))
                }
            }
        }
    }
    None
}

/// A landing place under a destination filled in at run time: refuse a
/// deleting sync, a source that holds an enforcement name, or one the guard
/// cannot read. `None` when the source holds none.
fn run_time_landing(
    cmd: &str,
    dest: &str,
    source: &str,
    deletes: Option<&str>,
    read: &mut Option<(Source, Option<PathBuf>)>,
    place: &Place<'_>,
) -> Option<String> {
    if let Some(flag) = deletes {
        return Some(format!(
            "`{cmd} {flag}` deletes under its destination `{dest}`, which the guard cannot resolve"
        ));
    }
    let (state, path) = read.get_or_insert_with(|| read_source(cmd, source, place));
    let held = match (state, path) {
        (Source::Unread(why), _) => Err(why.clone()),
        (Source::Entry, Some(path)) => {
            let own = path.file_name().map(|n| n.to_string_lossy().into_owned());
            enforcement_name(own.as_deref()).map(|hit| hit.then(|| path.clone()))
        }
        (Source::Tree(..), Some(path)) => {
            holds_enforcement_name(path).and_then(|found| match found {
                Some(found) => Ok(Some(found)),
                None => match base_name(source) {
                    None => Ok(None),
                    Some(name) => name
                        .and_then(|name| enforcement_name(Some(&name)))
                        .map(|hit| hit.then(|| path.clone())),
                },
            })
        }
        (_, None) => Err("it cannot be resolved".into()),
    };
    match held {
        Ok(None) => None,
        Ok(Some(found)) => Some(format!(
            "`{cmd}` writes to `{dest}`, which the guard cannot resolve, from the source `{source}`, which holds the enforcement entry `{}`",
            found.display()
        )),
        Err(why) => Some(format!(
            "`{cmd}` writes to `{dest}`, which the guard cannot resolve, from the source `{source}`, which the guard cannot verify: {why}"
        )),
    }
}

/// Why a recursive copy or sync of `copy` by `cmd` reaches an enforcement
/// file or a checkout's `.git`, or cannot be shown not to: the refusal
/// message, naming the file and the operand it comes from. `None` keeps
/// the verdict the destination word gets.
pub(crate) fn judge(cmd: &str, copy: &CopyRead, place: &Place<'_>) -> Option<String> {
    if !recursive(cmd, &copy.flags) || copy.directories {
        return None;
    }
    if cmd == "rsync" && dry_run(copy) {
        return None;
    }
    let current = |word: &String| {
        if word.is_empty() {
            ".".to_string()
        } else {
            word.clone()
        }
    };
    let dest = current(copy.dest.as_ref()?);
    let sources: Vec<String> = copy.sources.iter().map(current).collect();
    let rsync = cmd == "rsync";
    let deletes = flag_of(&copy.flags, rsync_deletes).filter(|_| rsync);
    let removes = flag_of(&copy.flags, rsync_removes_source).filter(|_| rsync);

    if let Some(flag) = removes {
        if let Some(message) = removed_source(cmd, flag, &sources, place) {
            return Some(message);
        }
    }

    for source in &sources {
        let mut read: Option<(Source, Option<PathBuf>)> = None;
        for landing in landings(cmd, copy, source, &dest, place) {
            let Some(base) = (place.resolve)(&landing) else {
                let message = run_time_landing(cmd, &dest, source, deletes, &mut read, place);
                if message.is_some() {
                    return message;
                }
                continue;
            };
            let reach = match reached(&base, place) {
                Ok(reach) => reach,
                Err(error) => {
                    return Some(format!(
                        "`{cmd}` writes under its destination `{landing}`, which the guard cannot verify: {error}"
                    ))
                }
            };
            if reach.is_empty() {
                continue;
            }
            let shown = |relative: &Path| {
                format!(
                    "{}/{}",
                    landing.trim_end_matches(SEPARATORS),
                    crate::portable_path::slashed(relative)
                )
            };
            if let Some(flag) = deletes {
                let what = reach.inside.as_ref().map_or_else(
                    || shown(&reach.below[0]),
                    |inside| inside.display().to_string(),
                );
                return Some(format!(
                    "`{cmd} {flag}` would delete or replace the enforcement path `{what}` under its destination `{landing}`"
                ));
            }
            if copy.unknown.is_some() {
                let option = copy.unknown.as_deref().unwrap_or_default();
                return Some(format!(
                    "`{cmd}` is given `{option}`, an option the guard does not read, and its destination `{landing}` holds enforcement files"
                ));
            }
            if let Some(Err(why)) = base_name(source) {
                return Some(format!(
                    "`{cmd}` copies the source `{source}` to `{landing}`, which holds enforcement files, and the guard cannot map the name it lands under: {why}"
                ));
            }
            let (state, _) = read.get_or_insert_with(|| read_source(cmd, source, place));
            match state {
                Source::Unread(why) => {
                    return Some(format!(
                        "`{cmd}` copies the source `{source}`, which the guard cannot verify ({why}), to `{landing}`, which holds enforcement files"
                    ))
                }
                Source::Entry => {
                    if let Some(inside) = &reach.inside {
                        return Some(format!(
                            "`{cmd}` writes the source `{source}` at the enforcement path `{}`",
                            inside.display()
                        ));
                    }
                }
                Source::Tree(lower, names) => {
                    let found = tree_reaches(cmd, source, &base, &landing, (lower, names.as_slice()), &reach, place);
                    if found.is_some() {
                        return found;
                    }
                }
            }
        }
    }
    None
}

/// Why a directory source whose top-level names are `lower` (compared) and
/// `names` (as listed) writes into or onto an enforcement path at the
/// resolved landing place `base`, or through a link there.
fn tree_reaches(
    cmd: &str,
    source: &str,
    base: &Path,
    landing: &str,
    (lower, names): (&HashSet<String>, &[std::ffi::OsString]),
    reach: &Reached,
    place: &Place<'_>,
) -> Option<String> {
    if let Some(inside) = &reach.inside {
        return Some(format!(
            "`{cmd}` writes the contents of the source `{source}` into the enforcement path `{}`",
            inside.display()
        ));
    }
    for relative in &reach.below {
        let first = relative
            .components()
            .next()
            .map(|c| compared(&c.as_os_str().to_string_lossy()));
        // A name the guard cannot map may be any of the source's names.
        if first.is_some_and(|first| match first {
            Ok(first) => lower.contains(&first),
            Err(_) => true,
        }) {
            return Some(format!(
                "`{cmd}` would write the enforcement path `{}/{}` from the source `{source}`",
                landing.trim_end_matches(SEPARATORS),
                crate::portable_path::slashed(relative)
            ));
        }
    }
    // A link at the landing place that the copy writes through, such as
    // `x -> .claude` for a source `x/`.
    through_link(cmd, source, base, landing, names, place)
}

#[cfg(test)]
#[path = "copy_reach_tests.rs"]
mod tests;
