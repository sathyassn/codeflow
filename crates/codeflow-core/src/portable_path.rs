//! Path spellings that hold on every platform.
//!
//! Windows names one file several ways: an 8.3 short name (`RUNNER~1`),
//! any letter case, `/` or `\` separators, and the verbatim `\\?\C:\` form
//! that [`std::fs::canonicalize`] returns. Git, the harnesses and users
//! write the plain `C:\` form, and messages show paths from a repository
//! root with `/`. These helpers give one spelling for each purpose; on
//! other platforms they change nothing.

use std::path::{Path, PathBuf};

/// [`std::fs::canonicalize`] without the verbatim prefix Windows adds to a
/// drive path, so the result compares equal to the path git or a harness
/// writes for the same file.
///
/// # Errors
///
/// The error [`std::fs::canonicalize`] gives.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize().map(without_verbatim)
}

/// `path` with a verbatim drive prefix (`\\?\C:\`) written as the plain
/// drive (`C:\`). Other prefixes, such as a verbatim UNC share, stay as they
/// are, and so does every path on other platforms.
#[must_use]
pub fn without_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            if let Prefix::VerbatimDisk(drive) = prefix.kind() {
                let mut plain = PathBuf::from(format!("{}:\\", char::from(drive)));
                plain.extend(components.filter(|c| !matches!(c, Component::RootDir)));
                return plain;
            }
        }
        path
    }
    #[cfg(not(windows))]
    {
        path
    }
}

/// `path` as text with `/` separators: how a repository-relative path is
/// shown and matched. A backslash is a separator only on Windows; elsewhere
/// it is part of a file name and stays.
///
/// OS text rule (issue 79): the text is a key, never a lossy spelling. A
/// path that is not valid UTF-8 keeps its exact bytes in the key
/// ([`crate::git::GitName::storage_key`]), so two different paths never
/// share one and valid text is unchanged. Show a key to a person with
/// [`crate::git::display_key`].
#[must_use]
pub fn slashed(path: &Path) -> String {
    let key = crate::git::GitName::from_os_str(path.as_os_str()).storage_key();
    if cfg!(windows) {
        key.replace('\\', "/")
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canonical_path_names_an_existing_directory_without_a_verbatim_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonicalize(dir.path()).unwrap();
        assert!(canonical.is_dir());
        assert!(!canonical.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(canonicalize(&canonical).unwrap(), canonical);
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_drive_path_reads_as_the_plain_drive() {
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\C:\Users\a\b.txt")),
            PathBuf::from(r"C:\Users\a\b.txt")
        );
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\?\UNC\server\share\x")
        );
        assert_eq!(slashed(Path::new(r"nested\AGENTS.md")), "nested/AGENTS.md");
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_is_part_of_a_unix_file_name() {
        assert_eq!(slashed(Path::new(r"odd\name")), r"odd\name");
        assert_eq!(slashed(Path::new("a/b")), "a/b");
    }
}
