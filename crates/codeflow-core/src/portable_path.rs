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
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        // Strip before parsing components: a mixed-separator verbatim path
        // treats `/` as part of a name, and pushing onto a verbatim path
        // collapses `..` before a guard can resolve links or expand globs.
        if units
            .get(..4)
            .is_some_and(|prefix| matches!(prefix, [47 | 92, 47 | 92, 63, 47 | 92]))
            && units
                .get(4)
                .is_some_and(|c| matches!(c, 65..=90 | 97..=122))
            && units.get(5) == Some(&58)
            && units.get(6).is_some_and(|c| matches!(c, 47 | 92))
        {
            return PathBuf::from(std::ffi::OsString::from_wide(&units[4..]));
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

/// Win32 accepts backslash separators and ignores trailing dots and spaces
/// in ordinary path components. Verbatim drive prefixes retain the same
/// protected-name judgment. Other hosts retain those literal bytes.
#[must_use]
pub(crate) fn filesystem_spelling(text: &str) -> String {
    if !cfg!(windows) {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let text = if bytes
        .get(..4)
        .is_some_and(|prefix| matches!(prefix, [b'/' | b'\\', b'/' | b'\\', b'?', b'/' | b'\\']))
        && bytes.get(4).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(5) == Some(&b':')
        && bytes.get(6).is_some_and(|c| matches!(c, b'/' | b'\\'))
    {
        &text[4..]
    } else {
        text
    };
    text.replace('\\', "/")
        .split('/')
        .map(|part| {
            if matches!(part, "." | "..") {
                part
            } else {
                part.trim_end_matches(['.', ' '])
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Namespaces and streams need a mapping before a guard can compare names.
/// Existing 8.3 short-name components resolve through the kernel, whose final
/// path names each entry's long name. A short-looking component that does not
/// exist refuses while an entry could still be created there, because a
/// command earlier on the line could create the entry it aliases. It stays
/// literal only when nothing can exist at that path. Unreadable components
/// refuse.
pub(crate) fn guard_path(path: &Path) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Ok(path.to_path_buf());
    }
    let text = path
        .to_str()
        .ok_or("cannot inspect a Windows path as text")?;
    let spelling = text.replace('\\', "/").to_ascii_lowercase();
    if unmapped_namespace(&spelling) {
        return Err("cannot map a Windows device, UNC or alternate-stream path".into());
    }
    let plain = without_verbatim(path.to_path_buf());
    if !has_short_component(&spelling) {
        return Ok(plain);
    }
    map_short_components(&plain)
}

/// Windows spellings a guard cannot map to a drive path: the device and
/// verbatim UNC namespaces, and alternate stream syntax. A stream type such
/// as `::$DATA` or `:$I30:$INDEX_ALLOCATION` opens the file or directory
/// itself. `spelling` is lowercase text with `/` separators.
fn unmapped_namespace(spelling: &str) -> bool {
    spelling.starts_with("//./")
        || spelling.starts_with("//?/unc/")
        || spelling.contains("::")
        || spelling.contains(":$")
}

/// Whether a component of `text` (with `/` separators) has the 8.3 short
/// name shape: a `~` followed by digits, before any extension.
pub(crate) fn has_short_component(text: &str) -> bool {
    text.split('/').any(|part| {
        part.split_once('~').is_some_and(|(_, tail)| {
            let number = tail.split('.').next().unwrap_or_default();
            !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())
        })
    })
}

/// [`guard_path`]'s short-name mapping, without the platform check so every
/// host can test it.
fn map_short_components(path: &Path) -> Result<PathBuf, String> {
    let mut real = PathBuf::new();
    // Set once a component can hold no entry: nothing below it exists or
    // can be created, so the rest of the path stays as written.
    let mut impossible = false;
    for part in path.components() {
        if matches!(part, std::path::Component::Normal(_))
            && matches!(
                Path::new(part.as_os_str()).components().next(),
                Some(std::path::Component::Prefix(_))
            )
        {
            // `quoted/C:/tail`: a later component that reads alone as a
            // drive must not replace the path before it.
            let mut relative = std::ffi::OsString::from(".");
            relative.push(std::path::MAIN_SEPARATOR_STR);
            relative.push(part.as_os_str());
            real.push(relative);
        } else {
            real.push(part.as_os_str());
        }
        if impossible {
            continue;
        }
        let part = part
            .as_os_str()
            .to_str()
            .ok_or("cannot inspect a Windows path component as text")?;
        if !has_short_component(&part.replace('\\', "/")) {
            continue;
        }
        match crate::absence::entry(&real).map_err(|_| "cannot inspect a Windows short filename")? {
            // The kernel's final path may still contain `~1`: an entry can
            // have that literal long name rather than an 8.3 alias.
            crate::absence::Entry::Exists => {
                real = canonicalize(&real).map_err(|_| "cannot map a Windows short filename")?;
            }
            crate::absence::Entry::Impossible => impossible = true,
            crate::absence::Entry::Creatable => {
                return Err(
                    "cannot map a Windows short filename that a command could still create".into(),
                );
            }
        }
    }
    Ok(real)
}

/// The user's home directory as the guards and doctor read it: the first
/// of [`user_homes`].
#[must_use]
pub fn user_home() -> Option<PathBuf> {
    user_homes().into_iter().next()
}

/// Every home a shell of this user may read: `HOME`, then `USERPROFILE`
/// when it names another directory, each kept only as an absolute path. On
/// Windows a Git Bash `HOME` spelled `/c/Users/u` reads as `C:\Users\u`,
/// and where it differs from `USERPROFILE` both are homes: bash reads
/// `HOME`, native programs the profile (TSK-242 review round 13). With
/// neither set, the home is the account's own, as bash reads it from the
/// password database for `~` (TSK-238 review round 22).
#[must_use]
pub fn user_homes() -> Vec<PathBuf> {
    let homes = homes_from(
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
        cfg!(windows),
    );
    if homes.is_empty() {
        return std::env::home_dir()
            .filter(|home| home.is_absolute())
            .into_iter()
            .collect();
    }
    homes
}

/// The first of [`homes_from`] for this platform, so the order is testable
/// without changing this process's environment.
#[must_use]
pub fn home_from(
    home: Option<std::ffi::OsString>,
    profile: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    homes_from(home, profile, cfg!(windows)).into_iter().next()
}

/// [`user_homes`] from given values, with `windows` choosing whether a
/// `/<letter>/...` value is a Git Bash drive path.
#[must_use]
pub fn homes_from(
    home: Option<std::ffi::OsString>,
    profile: Option<std::ffi::OsString>,
    windows: bool,
) -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = Vec::new();
    // OS text rule (issue 79): a value is read as text only when it is
    // text. One that is not is never a Git Bash drive path, and it equals
    // another home only byte for byte, so two homes never fold into one.
    let fold = |text: &str| {
        let text = text.replace('\\', "/");
        let text = text.trim_end_matches('/');
        if windows {
            text.to_lowercase()
        } else {
            text.to_string()
        }
    };
    for value in [home, profile].into_iter().flatten() {
        let drive = value
            .to_str()
            .and_then(git_bash_drive_path)
            .filter(|_| windows);
        let path = match drive {
            Some(path) => path,
            None if Path::new(&value).is_absolute() => PathBuf::from(&value),
            None => continue,
        };
        let same = |other: &PathBuf| match (other.to_str(), path.to_str()) {
            (Some(other), Some(path)) => fold(other) == fold(path),
            _ => other.as_os_str() == path.as_os_str(),
        };
        if !homes.iter().any(same) {
            homes.push(path);
        }
    }
    homes
}

/// The Windows path a Git Bash drive path names: `/c/Users/u` is
/// `C:\Users\u` and `/c` is `C:\`. `None` for any other text.
#[must_use]
pub fn git_bash_drive_path(text: &str) -> Option<PathBuf> {
    git_bash_drive_text(text).map(PathBuf::from)
}

/// [`git_bash_drive_path`] as text: `/c/Users/u` is `C:\Users\u`.
#[must_use]
pub fn git_bash_drive_text(text: &str) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let tail = chars.as_str();
    if !(tail.is_empty() || tail.starts_with('/')) {
        return None;
    }
    let below = tail.trim_start_matches('/').replace('/', "\\");
    Some(format!("{}:\\{below}", letter.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TSK-1002 Windows repair (hosted run 40602309629): a literal
    /// `RUNNER~1` directory is its own long name, and a missing name beneath
    /// a file can never be created; both stay as written.
    #[test]
    fn short_mapping_keeps_literal_long_names_and_impossible_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonicalize(temp.path()).unwrap();
        let literal = root.join("RUNNER~1");
        std::fs::create_dir(&literal).unwrap();
        assert_eq!(map_short_components(&literal).unwrap(), literal);
        let prospective = literal.join("new-file");
        assert_eq!(map_short_components(&prospective).unwrap(), prospective);
        std::fs::write(root.join("file"), "x").unwrap();
        let below_file = root.join("file/HEAD~1/leaf");
        assert_eq!(map_short_components(&below_file).unwrap(), below_file);
    }

    /// A short name that does not exist yet could alias an entry a command
    /// earlier on the line creates (`mkdir .claude && ... CLAUDE~1/x`), so
    /// the mapper refuses it rather than judging the literal spelling.
    #[test]
    fn short_mapping_refuses_names_a_command_could_create() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonicalize(temp.path()).unwrap();
        for word in ["HEAD~1", "CLAUDE~1/settings.json", "missing/RUNNER~1/leaf"] {
            assert!(map_short_components(&root.join(word)).is_err(), "{word}");
        }
    }

    /// The kernel maps an existing alias; a symlink models it on Unix. A
    /// dangling alias cannot be mapped and refuses.
    #[cfg(unix)]
    #[test]
    fn short_mapping_resolves_aliases_and_refuses_dangling_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonicalize(temp.path()).unwrap();
        let real = root.join("long directory");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, root.join("ALIAS~1")).unwrap();
        assert_eq!(
            map_short_components(&root.join("ALIAS~1/new-file")).unwrap(),
            real.join("new-file")
        );
        std::os::unix::fs::symlink(root.join("missing"), root.join("BROKEN~1")).unwrap();
        assert!(map_short_components(&root.join("BROKEN~1/new-file")).is_err());
    }

    #[test]
    fn unmapped_windows_namespaces_include_every_stream_type_spelling() {
        for spelling in [
            "//./c:/users/u/.codex/auth.json",
            "//?/unc/server/share/.codex/auth.json",
            "c:/users/u/.codex/auth.json::$data",
            "c:/users/u/.codex::$index_allocation/auth.json",
            "c:/users/u/.codex:$i30:$index_allocation/auth.json",
        ] {
            assert!(unmapped_namespace(spelling), "{spelling}");
        }
        for spelling in ["c:/users/u/notes.txt", "//?/c:/users/u/notes.txt", "head~1"] {
            assert!(!unmapped_namespace(spelling), "{spelling}");
        }
    }

    #[test]
    fn short_component_shape_needs_digits_after_the_tilde() {
        for text in ["HEAD~1", "c:/users/RUNNER~1/x", "AUTH~1.JSO", "a/b~12"] {
            assert!(has_short_component(text), "{text}");
        }
        for text in ["~", "~/x", "a~b", "a~1b", "RUN~1*", "notes.txt~"] {
            assert!(!has_short_component(text), "{text}");
        }
    }

    /// Issue 79: two homes whose names differ only in bytes that are not
    /// UTF-8 share one lossy spelling, but they are two homes.
    #[cfg(unix)]
    #[test]
    fn homes_that_are_not_text_compare_by_their_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let a = std::ffi::OsString::from_vec(b"/home/caf\xe9".to_vec());
        let b = std::ffi::OsString::from_vec(b"/home/caf\xff".to_vec());
        for windows in [false, true] {
            assert_eq!(
                homes_from(Some(a.clone()), Some(b.clone()), windows),
                [PathBuf::from(&a), PathBuf::from(&b)]
            );
            assert_eq!(
                homes_from(Some(a.clone()), Some(a.clone()), windows),
                [PathBuf::from(&a)]
            );
        }
    }

    #[test]
    fn the_home_is_home_else_userprofile_when_absolute() {
        let a = std::env::temp_dir().join("a");
        let b = std::env::temp_dir().join("b");
        let os = |p: &Path| Some(p.as_os_str().to_os_string());
        assert_eq!(home_from(os(&a), os(&b)), Some(a.clone()));
        assert_eq!(home_from(None, os(&b)), Some(b.clone()));
        assert_eq!(home_from(Some("".into()), os(&b)), Some(b.clone()));
        assert_eq!(home_from(Some("relative".into()), os(&b)), Some(b.clone()));
        assert_eq!(home_from(Some("relative".into()), None), None);
        // Two names for two directories are both homes; one name twice is one.
        for windows in [false, true] {
            assert_eq!(homes_from(os(&a), os(&b), windows), [a.clone(), b.clone()]);
            assert_eq!(
                homes_from(os(&a), os(&a), windows),
                std::slice::from_ref(&a)
            );
        }
    }

    /// A Git Bash `HOME` is a Windows drive path, read the same way on
    /// every platform when `windows` says the values came from Windows.
    #[test]
    fn a_git_bash_home_reads_as_its_drive_path() {
        assert_eq!(
            git_bash_drive_path("/c/Users/u"),
            Some(PathBuf::from("C:\\Users\\u"))
        );
        assert_eq!(git_bash_drive_path("/d"), Some(PathBuf::from("D:\\")));
        for other in ["/cd/x", "/", "c/Users", "/1/x", "C:\\Users\\u"] {
            assert_eq!(git_bash_drive_path(other), None, "{other}");
        }
        let profile = std::env::temp_dir().join("profile");
        let msys = || Some(std::ffi::OsString::from("/c/Users/u"));
        let os = Some(profile.as_os_str().to_os_string());
        assert_eq!(
            homes_from(msys(), os.clone(), true),
            [PathBuf::from("C:\\Users\\u"), profile.clone()]
        );
        // Elsewhere `/c/Users/u` is a plain absolute path. A Windows host
        // never reads with `windows` false (`home_from` passes
        // `cfg!(windows)`), and there the path has no drive, so it is not
        // absolute and the assertion would test an unreachable case.
        if !cfg!(windows) {
            assert_eq!(
                homes_from(msys(), os, false),
                [PathBuf::from("/c/Users/u"), profile]
            );
        }
        // On Windows one directory in two letter cases is one home.
        let upper = std::env::temp_dir().join("Home");
        let lower = std::env::temp_dir().join("home");
        let os = |p: &Path| Some(p.as_os_str().to_os_string());
        assert_eq!(homes_from(os(&upper), os(&lower), true), [upper]);
    }

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
        for path in [
            r"\\?\C:\Users\a\b.txt",
            r"\\?\C:/Users/a/b.txt",
            "//?/C:/Users/a/b.txt",
        ] {
            assert_eq!(
                without_verbatim(PathBuf::from(path)),
                PathBuf::from(r"C:\Users\a\b.txt")
            );
            assert_eq!(filesystem_spelling(path), "C:/Users/a/b.txt");
        }
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\?\UNC\server\share\x")
        );
        assert_eq!(slashed(Path::new(r"nested\AGENTS.md")), "nested/AGENTS.md");
        assert_eq!(filesystem_spelling(r".git\config. "), ".git/config");
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_is_part_of_a_unix_file_name() {
        assert_eq!(slashed(Path::new(r"odd\name")), r"odd\name");
        assert_eq!(slashed(Path::new("a/b")), "a/b");
    }
}
