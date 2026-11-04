//! Proof that an optional filesystem input is absent.

use std::io;
use std::path::Path;

#[cfg(test)]
thread_local! {
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static NAMED: std::cell::RefCell<Option<(String, usize)>> =
        const { std::cell::RefCell::new(None) };
}

/// Entries this thread read through this module, so a test can prove a walk's
/// cost on every host whatever the host's path limit.
#[cfg(test)]
pub(crate) fn reads() -> usize {
    READS.with(std::cell::Cell::get)
}

/// Run `body` and count its reads through this module of a path holding a
/// component called `name`, so a test can see whether a target was inspected.
#[cfg(test)]
pub(crate) fn reads_naming<T>(name: &str, body: impl FnOnce() -> T) -> (T, usize) {
    NAMED.with(|named| *named.borrow_mut() = Some((name.to_owned(), 0)));
    let value = body();
    let (_, count) = NAMED
        .with(|named| named.borrow_mut().take())
        .expect("the count started above");
    (value, count)
}

#[cfg(test)]
fn note_read(path: &Path) {
    READS.with(|reads| reads.set(reads.get() + 1));
    NAMED.with(|named| {
        if let Some((name, count)) = named.borrow_mut().as_mut() {
            if path
                .components()
                .any(|part| part.as_os_str() == name.as_str())
            {
                *count += 1;
            }
        }
    });
}

fn read_entry(path: &Path) -> io::Result<std::fs::Metadata> {
    #[cfg(test)]
    note_read(path);
    std::fs::symlink_metadata(path)
}

/// A missing leaf is absent only beneath a directory that can be resolved.
/// An existing leaf, including a dangling link, must be read by the caller.
pub(crate) fn proven_absent(path: &Path) -> io::Result<bool> {
    match read_entry(path) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match missing_leaf(path)? {
        Missing::Absent => Ok(true),
        Missing::BelowNonDirectory(ancestor) => Err(io::Error::other(format!(
            "not a directory at {}",
            ancestor.display()
        ))),
        Missing::Unnameable(error) => Err(error),
    }
}

/// What the ancestors of a leaf that reads as not found say about it.
enum Missing {
    /// It lies beneath a directory that resolves.
    Absent,
    /// It lies beneath this existing entry, which is not a directory (a file,
    /// or a link to one), so nothing can exist at the leaf.
    BelowNonDirectory(std::path::PathBuf),
    /// An ancestor's name is one the platform refuses, or one of its own
    /// components is not a directory, so nothing can exist at the leaf.
    Unnameable(io::Error),
}

fn missing_leaf(path: &Path) -> io::Result<Missing> {
    let not_found = |error: &io::Error| error.kind() == io::ErrorKind::NotFound;
    let Some((ancestor, read)) = nearest_ancestor(path, not_found) else {
        // A relative path may have no existing ancestor (including its empty one).
        return if path.is_relative() {
            Ok(Missing::Absent)
        } else {
            Err(io::Error::other(format!(
                "cannot establish a directory ancestor for {}",
                path.display()
            )))
        };
    };
    let metadata = match read {
        Ok(metadata) => metadata,
        Err(error) if refuses_name(&error) => return Ok(Missing::Unnameable(error)),
        Err(error) => return Err(error),
    };
    let metadata = if metadata.file_type().is_symlink() {
        std::fs::metadata(ancestor).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::other(format!("dangling link at {}", ancestor.display()))
            } else {
                error
            }
        })?
    } else {
        metadata
    };
    if metadata.is_dir() {
        Ok(Missing::Absent)
    } else {
        Ok(Missing::BelowNonDirectory(ancestor.to_path_buf()))
    }
}

/// The nearest ancestor of `path` whose read `passes` does not accept, with
/// that read. `None` when every ancestor passes.
///
/// A guard asks about each name of a path, so one read per ancestor is
/// quadratic in depth, and only the host's path limit hides it: Windows reads
/// a path of 32 KiB, Unix stops near 1 or 4 KiB (TSK-1258). Nothing lies below
/// a missing name, so the reads step out in doubling strides and then bisect.
/// The nearest ancestor is read first, so an ordinary missing leaf costs what
/// it always did.
///
/// A name Windows refuses can lie below a missing name, where its read differs
/// from its neighbours'. The bisection may then settle on the missing name's
/// parent instead. Nothing exists at the leaf on either reading.
fn nearest_ancestor(
    path: &Path,
    passes: impl Fn(&io::Error) -> bool,
) -> Option<(&Path, io::Result<std::fs::Metadata>)> {
    let ancestors: Vec<&Path> = path.ancestors().skip(1).collect();
    let read = |at: usize| match read_entry(ancestors[at]) {
        Err(error) if passes(&error) => None,
        read => Some(read),
    };
    // Every ancestor before `near` passed.
    let mut near = 0;
    let mut stride = 1;
    let (mut far, mut found) = loop {
        if near == ancestors.len() {
            return None;
        }
        let at = (near + stride - 1).min(ancestors.len() - 1);
        if let Some(found) = read(at) {
            break (at, found);
        }
        near = at + 1;
        stride *= 2;
    };
    while near < far {
        let middle = near + (far - near) / 2;
        match read(middle) {
            Some(closer) => (far, found) = (middle, closer),
            None => near = middle + 1,
        }
    }
    Some((ancestors[far], found))
}

/// The deepest of `path` and its ancestors with an entry on disk, so a walk
/// over ancestors can skip the ones nothing can lie below. A relative path
/// with no such ancestor gives its empty last ancestor, the working directory.
/// A read failure that does not prove absence is returned, so a caller fails
/// closed as it would on reading a name below it.
pub(crate) fn deepest_entry(path: &Path) -> io::Result<&Path> {
    // `InvalidInput` is the NUL byte no platform can hold in a name.
    let no_entry = |error: &io::Error| {
        error.kind() == io::ErrorKind::NotFound
            || error.kind() == io::ErrorKind::InvalidInput
            || refuses_name(error)
    };
    match read_entry(path) {
        Ok(_) => return Ok(path),
        Err(error) if no_entry(&error) => {}
        Err(error) => return Err(error),
    }
    match nearest_ancestor(path, no_entry) {
        Some((ancestor, Ok(_))) => Ok(ancestor),
        Some((_, Err(error))) => Err(error),
        None => Ok(path.ancestors().last().unwrap_or(path)),
    }
}

/// Whether no entry can exist at `path`, for a guard deciding whether a word
/// it reads as a path names something on disk. That holds for a path that is
/// [`proven_absent`], for a name the platform cannot hold (a NUL anywhere; on
/// Windows `*`, `?`, `"`, `:` or a control character in a name, refused as
/// `ERROR_INVALID_NAME`, or a malformed path, `ERROR_BAD_PATHNAME`; on Unix a
/// name the kernel refuses as too long), and for a path beneath an existing
/// non-directory. Windows `ERROR_FILENAME_EXCED_RANGE` is not such a name: it
/// is the legacy `MAX_PATH` limit, and a long-path-aware process (Git for
/// Windows with `core.longpaths`) can still open that path. It and any other
/// read failure are returned, so a path the guard cannot read fails closed
/// (OS text rule, issue 79).
pub(crate) fn cannot_exist(path: &Path) -> io::Result<bool> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Ok(true);
    }
    match read_entry(path) {
        Ok(_) => Ok(false),
        Err(error) if failure_proves_no_entry(path, &error)? => Ok(true),
        Err(error) => Err(error),
    }
}

/// Leaf metadata for a guard: `None` for a path that [`cannot_exist`], the
/// error for a read failure on a path that could.
pub(crate) fn existing_metadata(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Ok(None);
    }
    match read_entry(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if failure_proves_no_entry(path, &error)? => Ok(None),
        Err(error) => Err(error),
    }
}

/// What the filesystem says about the entry at a path a guard must map.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Something exists there, possibly a dangling link.
    Exists,
    /// Nothing exists there now, but its nearest existing ancestor is a
    /// directory, so a command could still create an entry there.
    Creatable,
    /// Nothing can exist there: see [`cannot_exist`].
    Impossible,
}

/// Classify `path` as [`Entry`]. A read failure that settles none of the
/// three is returned, so a caller that cannot classify a path fails closed.
pub(crate) fn entry(path: &Path) -> io::Result<Entry> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Ok(Entry::Impossible);
    }
    match read_entry(path) {
        Ok(_) => Ok(Entry::Exists),
        Err(error) if refuses_name(&error) => Ok(Entry::Impossible),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(match missing_leaf(path)? {
            Missing::Absent => Entry::Creatable,
            Missing::BelowNonDirectory(_) | Missing::Unnameable(_) => Entry::Impossible,
        }),
        Err(error) => Err(error),
    }
}

/// Whether reading `path` failing with `error` proves nothing can exist
/// there: the platform refused the name ([`refuses_name`]), a component is
/// not a directory, or the leaf is not found and its ancestors settle it
/// ([`missing_leaf`]).
fn failure_proves_no_entry(path: &Path, error: &io::Error) -> io::Result<bool> {
    if refuses_name(error) {
        return Ok(true);
    }
    if error.kind() == io::ErrorKind::NotFound {
        return missing_leaf(path).map(|_| true);
    }
    Ok(false)
}

/// Whether the platform refused the path itself: a name it cannot hold, or a
/// component that is not a directory. Rust reports Windows
/// `ERROR_FILENAME_EXCED_RANGE` with the same `InvalidFilename` kind as a
/// refused name, so on Windows the raw code decides
/// ([`windows_error_refuses_name`]). On Unix that kind is `ENAMETOOLONG`, the
/// kernel's refusal, which every process on the host shares.
fn refuses_name(error: &io::Error) -> bool {
    match error.kind() {
        io::ErrorKind::InvalidFilename if cfg!(windows) => {
            error.raw_os_error().is_some_and(windows_error_refuses_name)
        }
        io::ErrorKind::InvalidFilename | io::ErrorKind::NotADirectory => true,
        _ => false,
    }
}

/// Windows `ERROR_INVALID_NAME`: a name the filesystem cannot store.
const ERROR_INVALID_NAME: i32 = 123;
/// Windows `ERROR_BAD_PATHNAME`: a path that cannot be parsed.
const ERROR_BAD_PATHNAME: i32 = 161;

/// Whether a Windows raw OS error that Rust reports as `InvalidFilename`
/// proves the name cannot exist. `ERROR_FILENAME_EXCED_RANGE` (206) does
/// not: it says this call hit the legacy `MAX_PATH` limit, not that the path
/// names nothing. Kept apart from `cfg(windows)` so every host tests it.
fn windows_error_refuses_name(code: i32) -> bool {
    matches!(code, ERROR_INVALID_NAME | ERROR_BAD_PATHNAME)
}

/// Obtain leaf metadata, returning None only for proven absence through its ancestors.
pub(crate) fn symlink_metadata_optional(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match read_entry(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && proven_absent(path)? => Ok(None),
        Err(error) => Err(error),
    }
}

/// Obtain metadata through symbolic links, returning None only for proven
/// absence. A dangling link exists, so it is an error, never absence.
pub(crate) fn metadata_optional(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    #[cfg(test)]
    note_read(path);
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && proven_absent(path)? => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `path` is a directory (or, with `dir` false, a regular file),
/// through symbolic links: `Some(false)` when it is proven absent or of
/// another type, `None` when its metadata cannot be read.
pub(crate) fn is_kind(path: &Path, dir: bool) -> Option<bool> {
    match metadata_optional(path) {
        Ok(Some(metadata)) => Some(if dir {
            metadata.is_dir()
        } else {
            metadata.is_file()
        }),
        Ok(None) => Some(false),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::proven_absent;

    #[test]
    fn r18_absence_requires_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(proven_absent(&dir.path().join("missing/deeper/file")).unwrap());
        assert!(!proven_absent(dir.path()).unwrap());
        std::fs::write(dir.path().join("file"), "x").unwrap();
        assert!(!proven_absent(&dir.path().join("file")).unwrap());
        assert!(proven_absent(&dir.path().join("file/child")).is_err());
        assert!(proven_absent(std::path::Path::new("r18-absent-relative-parent/child")).unwrap());
    }

    /// TSK-1002 Windows repair: a short-name mapper refuses a name a command
    /// could still create, but not one nothing can ever hold.
    #[test]
    fn entries_separate_creatable_from_impossible_names() {
        use super::{entry, Entry};
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(entry(dir.path()).unwrap(), Entry::Exists);
        assert_eq!(
            entry(&dir.path().join("missing/HEAD~1")).unwrap(),
            Entry::Creatable
        );
        std::fs::write(dir.path().join("file"), "x").unwrap();
        assert_eq!(
            entry(&dir.path().join("file/RUNNER~1")).unwrap(),
            Entry::Impossible
        );
        assert_eq!(
            entry(&dir.path().join("nul\0name")).unwrap(),
            Entry::Impossible
        );
    }

    #[cfg(unix)]
    #[test]
    fn r18_absence_distinguishes_links() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        symlink(dir.path().join("missing"), &link).unwrap();
        assert!(!proven_absent(&link).unwrap());
        assert!(proven_absent(&link.join("child/deeper"))
            .unwrap_err()
            .to_string()
            .contains("dangling link"));
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let resolved = dir.path().join("resolved");
        symlink(&real, &resolved).unwrap();
        assert!(proven_absent(&resolved.join("missing")).unwrap());
        std::fs::write(dir.path().join("file"), "x").unwrap();
        symlink(dir.path().join("file"), dir.path().join("file-link")).unwrap();
        assert!(proven_absent(&dir.path().join("file-link/child")).is_err());
    }
}

#[cfg(test)]
mod optional_metadata_tests {
    #[cfg(unix)]
    #[test]
    fn r22_optional_metadata_preserves_errors_and_absence() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing/child");
        assert!(super::symlink_metadata_optional(&missing)
            .unwrap()
            .is_none());
        let link = dir.path().join("link");
        std::os::unix::fs::symlink("missing", &link).unwrap();
        assert!(super::symlink_metadata_optional(&link).unwrap().is_some());
        assert!(super::symlink_metadata_optional(&link.join("child")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn followed_kind_tells_absence_from_an_unread_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        std::fs::write(dir.path().join("f"), "x").unwrap();
        assert_eq!(super::is_kind(&dir.path().join("d"), true), Some(true));
        assert_eq!(super::is_kind(&dir.path().join("f"), true), Some(false));
        assert_eq!(super::is_kind(&dir.path().join("f"), false), Some(true));
        assert_eq!(
            super::is_kind(&dir.path().join("missing"), true),
            Some(false)
        );
        // A dangling link exists: what it names cannot be read, so it is not
        // proven to be absent.
        let link = dir.path().join("link");
        std::os::unix::fs::symlink("missing", &link).unwrap();
        assert_eq!(super::is_kind(&link, true), None);
        assert!(super::metadata_optional(&link).is_err());
    }
}

#[cfg(test)]
mod cannot_exist_tests {
    use super::{
        cannot_exist, deepest_entry, existing_metadata, proven_absent, reads,
        windows_error_refuses_name,
    };
    use std::path::Path;

    /// Windows maps three codes to `InvalidFilename`. Only a name it cannot
    /// store (123) or a malformed path (161) names nothing; a path past the
    /// legacy `MAX_PATH` limit (206) can still exist for a long-path-aware
    /// process, so it must stay a read failure.
    #[test]
    fn only_a_windows_name_refusal_proves_no_entry() {
        assert!(windows_error_refuses_name(123), "ERROR_INVALID_NAME");
        assert!(windows_error_refuses_name(161), "ERROR_BAD_PATHNAME");
        assert!(
            !windows_error_refuses_name(206),
            "ERROR_FILENAME_EXCED_RANGE"
        );
        assert!(!windows_error_refuses_name(2), "ERROR_FILE_NOT_FOUND");
        assert!(!windows_error_refuses_name(5), "ERROR_ACCESS_DENIED");
    }

    /// A name the platform refuses names nothing: Windows refuses `"$d"`,
    /// `a*b` or a newline with `ERROR_INVALID_NAME` (os error 123), and Unix
    /// refuses a name longer than its limit with the same error kind, so the
    /// guard's reading is proven on every host.
    fn refused_names() -> Vec<String> {
        if cfg!(windows) {
            ["\"$d\"", "a*b", "line\nbreak"].map(String::from).to_vec()
        } else {
            vec!["x".repeat(4096)]
        }
    }

    #[test]
    fn a_name_the_platform_refuses_cannot_exist() {
        let dir = tempfile::tempdir().unwrap();
        for name in refused_names() {
            let path = dir.path().join(&name);
            assert!(proven_absent(&path).is_err(), "absence proof stays strict");
            assert!(cannot_exist(&path).unwrap(), "{name:.20}");
            assert!(cannot_exist(&path.join("child")).unwrap(), "{name:.20}");
            assert!(existing_metadata(&path).unwrap().is_none(), "{name:.20}");
        }
        let nul = dir.path().join("a\0b");
        assert!(cannot_exist(&nul).unwrap());
        assert!(existing_metadata(&nul).unwrap().is_none());
    }

    /// A path beneath an existing file names nothing: Windows reports it as
    /// not found and the file as its ancestor, Unix as not a directory.
    #[test]
    fn a_path_beneath_a_file_cannot_exist() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("README.md");
        std::fs::write(&file, "x").unwrap();
        assert!(
            proven_absent(&file.join("n")).is_err(),
            "absence proof stays strict"
        );
        assert!(cannot_exist(&file.join("n")).unwrap());
        assert!(cannot_exist(&file.join("n/deeper")).unwrap());
        assert!(existing_metadata(&file.join("n")).unwrap().is_none());
        assert!(!cannot_exist(&file).unwrap());
        assert!(existing_metadata(&file).unwrap().is_some());
        assert!(cannot_exist(&dir.path().join("missing/child")).unwrap());
    }

    /// A path past the legacy `MAX_PATH` limit that exists is read, and one
    /// that is missing is settled by its ancestors; neither length is read
    /// as a name Windows refuses.
    #[cfg(windows)]
    #[test]
    fn a_windows_path_past_max_path_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        while deep.as_os_str().len() <= 300 {
            deep.push("a-long-directory-name-for-max-path");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let policy = deep.join("policy.json");
        std::fs::write(&policy, "{}").unwrap();
        assert!(policy.as_os_str().len() > 260);
        assert!(!cannot_exist(&policy).unwrap());
        assert!(existing_metadata(&policy).unwrap().is_some());
        assert!(cannot_exist(&deep.join("missing/child")).unwrap());
        assert!(existing_metadata(&deep.join("missing")).unwrap().is_none());
    }

    /// A nameable path the guard cannot read still fails closed.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_nameable_path_is_not_proven_missing() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        symlink(dir.path().join("missing"), &link).unwrap();
        assert!(cannot_exist(&link.join("child")).is_err(), "dangling link");
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&locked).is_ok();
        let result = cannot_exist(&locked.join("child"));
        let metadata = existing_metadata(&locked.join("child"));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !readable {
            assert!(result.is_err(), "{result:?}");
            assert!(metadata.is_err(), "{metadata:?}");
        }
    }

    /// A missing leaf is settled in reads that grow with the logarithm of its
    /// missing depth. One read per ancestor made a guard that asks this for
    /// every component quadratic in reads, which only the host's path limit
    /// hid: Windows reads a path of about 32 KiB, Unix stops near 1 or 4 KiB
    /// (TSK-1258, PR 552's Windows hang).
    #[test]
    fn a_deep_missing_leaf_is_settled_without_reading_each_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let leaf = dir.path().join("a/".repeat(400));
        let before = reads();
        assert!(cannot_exist(&leaf).unwrap());
        let spent = reads() - before;
        assert!(spent <= 20, "{spent} reads for a leaf 400 names deep");
    }

    /// However far out the nearest entry lies, the walk settles the leaf the
    /// way one read per ancestor did: a file, a dangling link or a directory.
    #[test]
    fn the_stepped_walk_settles_a_leaf_at_every_distance() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        for depth in 1..40 {
            let below = "n/".repeat(depth);
            assert!(cannot_exist(&dir.path().join(&below)).unwrap(), "{depth}");
            assert!(proven_absent(&dir.path().join(&below)).unwrap(), "{depth}");
            assert!(cannot_exist(&file.join(&below)).unwrap(), "{depth}");
            assert!(proven_absent(&file.join(&below)).is_err(), "{depth}");
        }
        assert!(proven_absent(Path::new("codeflow-no-such-name/n/n/n")).unwrap());
    }

    /// A link with no target, or a directory the guard cannot read, stays a
    /// failure at every distance, so a guard fails closed on it.
    #[cfg(unix)]
    #[test]
    fn the_stepped_walk_fails_closed_at_every_distance() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        symlink(dir.path().join("missing"), &link).unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&locked).is_ok();
        let results: Vec<_> = (1..40)
            .map(|depth| {
                let below = "n/".repeat(depth);
                (
                    cannot_exist(&link.join(&below)),
                    cannot_exist(&locked.join(&below)),
                )
            })
            .collect();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        for (at_link, at_locked) in results {
            assert!(at_link.is_err(), "{at_link:?}");
            assert!(readable || at_locked.is_err(), "{at_locked:?}");
        }
    }

    /// What each host does with a path of 32,767 characters, which a guard
    /// can reach only through a link or short name that expands past its
    /// bound. Windows reports `os error 206`, which proves nothing about the
    /// path, so the read fails closed. Unix refuses the name, so nothing can
    /// exist there and the search moves outward.
    #[test]
    fn a_path_at_the_windows_limit_fails_closed_there() {
        let dir = tempfile::tempdir().unwrap();
        let mut path = dir.path().to_path_buf();
        while path.as_os_str().len() < 32_700 {
            path.push("a");
        }
        path.push("a".repeat(32_767 - path.as_os_str().len() - 1));
        assert_eq!(path.as_os_str().len(), 32_767);
        if cfg!(windows) {
            let error = deepest_entry(&path).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(206), "{error}");
            assert!(cannot_exist(&path).is_err());
        } else {
            assert_eq!(deepest_entry(&path).unwrap(), dir.path());
            assert!(cannot_exist(&path).unwrap());
        }
    }

    #[test]
    fn the_deepest_entry_is_the_last_name_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("file");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(deepest_entry(root).unwrap(), root);
        assert_eq!(deepest_entry(&file).unwrap(), file);
        assert_eq!(deepest_entry(&root.join("n/n/n/n/n")).unwrap(), root);
        assert_eq!(deepest_entry(&file.join("n/n/n/n")).unwrap(), file);
        assert_eq!(deepest_entry(&root.join("a\0b/n")).unwrap(), root);
        assert_eq!(
            deepest_entry(Path::new("codeflow-no-such-name/n/n")).unwrap(),
            Path::new("")
        );
    }
}
