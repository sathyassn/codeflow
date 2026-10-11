//! Proof that an optional filesystem input is absent.

use std::io;
use std::path::Path;

/// A missing leaf is absent only beneath a directory that can be resolved.
/// An existing leaf, including a dangling link, must be read by the caller.
pub(crate) fn proven_absent(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
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
    for ancestor in path.ancestors().skip(1) {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
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
        if !metadata.is_dir() {
            return Ok(Missing::BelowNonDirectory(ancestor.to_path_buf()));
        }
        return Ok(Missing::Absent);
    }
    // A relative path may have no existing ancestor (including its empty one).
    if path.is_relative() {
        Ok(Missing::Absent)
    } else {
        Err(io::Error::other(format!(
            "cannot establish a directory ancestor for {}",
            path.display()
        )))
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
    match std::fs::symlink_metadata(path) {
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
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if failure_proves_no_entry(path, &error)? => Ok(None),
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
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && proven_absent(path)? => Ok(None),
        Err(error) => Err(error),
    }
}

/// Obtain metadata through symbolic links, returning None only for proven
/// absence. A dangling link exists, so it is an error, never absence.
pub(crate) fn metadata_optional(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
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
    use super::{cannot_exist, existing_metadata, proven_absent, windows_error_refuses_name};

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
}
