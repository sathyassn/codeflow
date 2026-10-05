//! One reading layer for names that git or the operating system supplies.
//!
//! A reference, branch, remote, worktree or path name is bytes, and valid
//! UTF-8 is not promised (issue 79). [`GitName`] holds the bytes exactly and
//! gives two views of them, and nothing else that makes a `String`:
//!
//! - [`GitName::display`] is for output only: messages, reports and logs. It
//!   writes each invalid byte, each backslash and each control character as an
//!   escape. It is never a key, never compared, never given to git, a glob or
//!   a config lookup.
//! - [`GitName::rule_text`] is the name as `&str` when it is valid UTF-8, and
//!   an error that carries the display form when it is not. A rule that needs
//!   text (a glob, a config key, a task id or work prefix, a policy match)
//!   calls it and refuses, or widens conservatively with a comment at the
//!   site, on the error.
//!
//! Decisions use the bytes: maps are keyed by `GitName`, names compare as
//! bytes, and a prefix is tested with [`GitName::starts_with`]. A value that
//! is persisted uses [`GitName::storage_key`], which names the bytes exactly.
//! A lossy decode (`String::from_utf8_lossy`) gives two different names one
//! text, so it is for display only and `crates/codeflow-core/tests/
//! name_decode_scan.rs` fails on any other use.

use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

/// A name as bytes, exactly as git or the operating system supplied it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct GitName(Vec<u8>);

/// The error of [`GitName::rule_text`]: the name is not valid UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotUtf8(String);

impl NotUtf8 {
    /// The name in display form, for the refusal message.
    #[must_use]
    pub fn display(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NotUtf8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the name `{}` is not valid UTF-8", self.0)
    }
}

impl std::error::Error for NotUtf8 {}

/// The error of [`GitName::os_path`]: the platform cannot hold these bytes
/// as a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRepresentable(String);

impl fmt::Display for NotRepresentable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the name `{}` cannot be a path on this platform", self.0)
    }
}

impl std::error::Error for NotRepresentable {}

/// A name's display form: formatting only, no `Deref` to `str`, no
/// conversion back to a name.
pub struct DisplayName<'a>(&'a [u8]);

impl fmt::Display for DisplayName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.0.utf8_chunks() {
            for character in chunk.valid().chars() {
                match character {
                    '\\' => f.write_str("\\\\")?,
                    c if c.is_control() => write!(f, "\\x{:02x}", u32::from(c))?,
                    c => fmt::Write::write_char(f, c)?,
                }
            }
            for byte in chunk.invalid() {
                write!(f, "\\x{byte:02x}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for DisplayName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for GitName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GitName({})", self.display())
    }
}

impl fmt::Display for GitName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.display(), f)
    }
}

impl GitName {
    /// A name from its exact bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    /// A name from an owned byte vector.
    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// A name from text, which is always valid UTF-8.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }

    /// A name from an operating-system string, exact on every platform.
    #[must_use]
    pub fn from_os_str(value: &OsStr) -> Self {
        Self(value.as_encoded_bytes().to_vec())
    }

    /// The exact bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The display form, for output only.
    #[must_use]
    pub fn display(&self) -> DisplayName<'_> {
        DisplayName(&self.0)
    }

    /// The name as text when it is valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`NotUtf8`], carrying the display form, when it is not. The
    /// caller refuses, or widens conservatively and says why at the site.
    pub fn rule_text(&self) -> Result<&str, NotUtf8> {
        std::str::from_utf8(&self.0).map_err(|_| NotUtf8(self.display().to_string()))
    }

    /// Whether the name starts with `prefix`, byte for byte.
    #[must_use]
    pub fn starts_with(&self, prefix: &[u8]) -> bool {
        self.0.starts_with(prefix)
    }

    /// The name after `prefix`, when it starts with it.
    #[must_use]
    pub fn strip_prefix(&self, prefix: &[u8]) -> Option<Self> {
        self.0.strip_prefix(prefix).map(Self::from_bytes)
    }

    /// The name with `suffix` appended.
    #[must_use]
    pub fn joined(&self, suffix: &[u8]) -> Self {
        let mut bytes = self.0.clone();
        bytes.extend_from_slice(suffix);
        Self(bytes)
    }

    /// The name as a path: exact on Unix, and only when it is valid UTF-8
    /// elsewhere, because native Windows paths are UTF-16 and git bytes are
    /// not.
    ///
    /// # Errors
    ///
    /// Returns [`NotRepresentable`] when the platform cannot hold the bytes.
    pub fn os_path(&self) -> Result<PathBuf, NotRepresentable> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            Ok(PathBuf::from(OsStr::from_bytes(&self.0)))
        }
        #[cfg(not(unix))]
        {
            std::str::from_utf8(&self.0)
                .map(PathBuf::from)
                .map_err(|_| NotRepresentable(self.display().to_string()))
        }
    }

    /// The persisted form: the name itself when it is valid UTF-8, else the
    /// lossy text, a NUL and the hex of the bytes. A name never holds a NUL
    /// (a path, a reference and a remote name cannot), so no valid name equals
    /// an invalid one's key, and two different invalid names differ in their
    /// hex. [`GitName::from_storage_key`] reverses it.
    #[must_use]
    pub fn storage_key(&self) -> String {
        use std::fmt::Write as _;
        if let Ok(text) = std::str::from_utf8(&self.0) {
            return text.to_string();
        }
        // display: the readable half of a key whose identity is the hex.
        let mut key = String::from_utf8_lossy(&self.0).into_owned();
        key.push('\0');
        for byte in &self.0 {
            let _ = write!(key, "{byte:02x}");
        }
        key
    }

    /// The name a [`GitName::storage_key`] names.
    #[must_use]
    pub fn from_storage_key(key: &str) -> Self {
        let Some((_, hex)) = key.split_once('\0') else {
            return Self::from_text(key);
        };
        Self(
            hex.as_bytes()
                .chunks(2)
                .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
                .collect(),
        )
    }
}

impl From<&str> for GitName {
    fn from(text: &str) -> Self {
        Self::from_text(text)
    }
}

impl From<String> for GitName {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Readers: the only code that turns git2's name accessors into names.
// ---------------------------------------------------------------------------

/// The full name of a reference.
#[must_use]
pub fn reference_name(reference: &git2::Reference<'_>) -> GitName {
    GitName::from_bytes(reference.name_bytes())
}

/// The short name of a reference.
#[must_use]
pub fn reference_shorthand(reference: &git2::Reference<'_>) -> GitName {
    GitName::from_bytes(reference.shorthand_bytes())
}

/// The full name a symbolic reference points at, when it is symbolic.
#[must_use]
pub fn symbolic_target(reference: &git2::Reference<'_>) -> Option<GitName> {
    reference.symbolic_target_bytes().map(GitName::from_bytes)
}

/// The name of a branch, with its remote when it is remote-tracking.
///
/// # Errors
///
/// Returns git's error when the name cannot be read.
pub fn branch_name(branch: &git2::Branch<'_>) -> Result<GitName, git2::Error> {
    branch.name_bytes().map(GitName::from_bytes)
}

/// Every name in a git2 string array, none dropped.
#[must_use]
pub fn names_of(array: &git2::string_array::StringArray) -> Vec<GitName> {
    array.iter_bytes().map(GitName::from_bytes).collect()
}

/// Every remote name of the repository, none dropped.
///
/// # Errors
///
/// Returns git's error when the remotes cannot be listed.
pub fn remote_names(repo: &git2::Repository) -> Result<Vec<GitName>, git2::Error> {
    Ok(names_of(&repo.remotes()?))
}

/// The path of every file a diff touches (the old and the new path of each
/// delta, one when they are the same), as exact bytes.
#[must_use]
pub fn diff_paths(diff: &git2::Diff<'_>) -> Vec<GitName> {
    let mut paths = Vec::new();
    for delta in diff.deltas() {
        let old = delta.old_file().path_bytes();
        let new = delta.new_file().path_bytes();
        for path in old.into_iter().chain(new.filter(|new| Some(*new) != old)) {
            paths.push(GitName::from_bytes(path));
        }
    }
    paths
}

/// The display form of a [`GitName::storage_key`], for a message.
#[must_use]
pub fn display_key(key: &str) -> String {
    GitName::from_storage_key(key).display().to_string()
}

/// What a tree walk does after an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    /// Go on, into a tree entry too.
    Continue,
    /// Do not go into this tree entry (no effect on other entries).
    SkipTree,
    /// Stop the walk; it ends without an error.
    Stop,
}

/// Walk a tree in pre-order with every entry's full path as exact bytes.
///
/// `git2`'s own `Tree::walk` gives the parent path as `&str` and aborts the
/// whole walk, with an error, at the first directory whose name is not valid
/// UTF-8 (issue 79). This walk reads names as bytes, so one such directory
/// neither stops a record or release check nor hides what is under it. A path
/// has no trailing slash, also for a tree entry.
///
/// # Errors
///
/// Returns git's error when a tree object cannot be read.
pub fn walk_tree(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    visit: &mut dyn FnMut(&GitName, &git2::TreeEntry<'_>) -> Walk,
) -> Result<(), git2::Error> {
    fn go(
        repo: &git2::Repository,
        tree: &git2::Tree<'_>,
        prefix: &GitName,
        visit: &mut dyn FnMut(&GitName, &git2::TreeEntry<'_>) -> Walk,
    ) -> Result<bool, git2::Error> {
        for entry in tree {
            let path = if prefix.is_empty() {
                GitName::from_bytes(entry.name_bytes())
            } else {
                prefix.joined(b"/").joined(entry.name_bytes())
            };
            match visit(&path, &entry) {
                Walk::Stop => return Ok(false),
                Walk::SkipTree => continue,
                Walk::Continue => {}
            }
            if entry.kind() == Some(git2::ObjectType::Tree) {
                let child = repo.find_tree(entry.id())?;
                if !go(repo, &child, &path, visit)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    go(repo, tree, &GitName::default(), visit).map(|_| ())
}

/// The names in a NUL-separated list (`git ... -z`), none dropped. A trailing
/// terminator does not make an empty last name.
#[must_use]
pub fn parse_nul_list(bytes: &[u8]) -> Vec<GitName> {
    let mut names: Vec<GitName> = bytes
        .split(|byte| *byte == 0)
        .map(GitName::from_bytes)
        .collect();
    if names.last().is_some_and(GitName::is_empty) {
        names.pop();
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_names_that_differ_only_in_an_invalid_byte_stay_two_names() {
        let invalid = GitName::from_bytes(b"caf\xe9");
        let lookalike = GitName::from_text("caf\u{fffd}");
        assert_ne!(invalid, lookalike);
        assert_ne!(invalid, GitName::from_bytes(b"caf\xff"));
        assert_ne!(invalid.storage_key(), lookalike.storage_key());
    }

    #[test]
    fn display_escapes_invalid_bytes_backslashes_and_controls() {
        assert_eq!(
            GitName::from_text("release/main").display().to_string(),
            "release/main"
        );
        assert_eq!(
            GitName::from_text("caf\u{e9}").display().to_string(),
            "caf\u{e9}"
        );
        assert_eq!(
            GitName::from_bytes(b"caf\xe9").display().to_string(),
            "caf\\xe9"
        );
        assert_eq!(GitName::from_text("a\\b").display().to_string(), "a\\\\b");
        assert_eq!(GitName::from_text("a\nb").display().to_string(), "a\\x0ab");
    }

    #[test]
    fn rule_text_is_the_name_or_a_refusal_that_names_it() {
        assert_eq!(GitName::from_text("main").rule_text().unwrap(), "main");
        let error = GitName::from_bytes(b"caf\xe9").rule_text().unwrap_err();
        assert_eq!(error.display(), "caf\\xe9");
        assert!(error.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn a_storage_key_names_its_bytes_and_goes_back_to_them() {
        assert_eq!(
            GitName::from_text("dir/plain.txt").storage_key(),
            "dir/plain.txt"
        );
        for raw in [
            &b"caf\xe9"[..],
            b"a/b\xff\xfe/c",
            b"plain",
            "caf\u{e9}".as_bytes(),
        ] {
            let name = GitName::from_bytes(raw);
            assert_eq!(GitName::from_storage_key(&name.storage_key()), name);
        }
    }

    #[test]
    fn prefixes_and_joins_work_on_bytes() {
        let name = GitName::from_bytes(b"task/TSK-1-caf\xe9");
        assert!(name.starts_with(b"task/TSK-1-"));
        assert_eq!(
            name.strip_prefix(b"task/").unwrap().bytes(),
            b"TSK-1-caf\xe9"
        );
        assert_eq!(GitName::from_text("a/").joined(b"\xff").bytes(), b"a/\xff");
    }

    #[test]
    fn a_tree_walk_goes_through_a_directory_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let blob = repo.blob(b"x").unwrap();
        let mut inner = repo.treebuilder(None).unwrap();
        inner.insert(&b"f"[..], blob, 0o100_644).unwrap();
        let inner = inner.write().unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        root.insert(&b"dir\xff"[..], inner, 0o040_000).unwrap();
        root.insert(&b"top"[..], blob, 0o100_644).unwrap();
        let tree = repo.find_tree(root.write().unwrap()).unwrap();
        let mut seen = Vec::new();
        walk_tree(&repo, &tree, &mut |path, _| {
            seen.push(path.bytes().to_vec());
            Walk::Continue
        })
        .unwrap();
        assert_eq!(
            seen,
            [b"dir\xff".to_vec(), b"dir\xff/f".to_vec(), b"top".to_vec()]
        );
        let mut skipped = Vec::new();
        walk_tree(&repo, &tree, &mut |path, _| {
            skipped.push(path.bytes().to_vec());
            Walk::SkipTree
        })
        .unwrap();
        assert_eq!(skipped, [b"dir\xff".to_vec(), b"top".to_vec()]);
    }

    /// Why `walk_tree` exists: git2's own walk stops with an error at the
    /// first directory whose name is not valid UTF-8, before it reaches the
    /// entries after it (issue 79).
    #[test]
    fn git2_own_walk_stops_at_a_directory_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, commit) =
            crate::git::repo_with_tree(dir.path(), &[(b"dir\xff/f", b"x"), (b"top", b"y")]);
        let tree = repo.find_commit(commit).unwrap().tree().unwrap();
        let mut seen = 0;
        let result = tree.walk(git2::TreeWalkMode::PreOrder, |_, _| {
            seen += 1;
            git2::TreeWalkResult::Ok
        });
        assert!(
            result.is_err() || seen < 3,
            "git2 walk read all {seen} entries"
        );
    }

    #[test]
    fn a_nul_list_keeps_every_name() {
        let names = parse_nul_list(b"a\0caf\xe9\0\0b\0");
        let bytes: Vec<&[u8]> = names.iter().map(GitName::bytes).collect();
        assert_eq!(bytes, [&b"a"[..], b"caf\xe9", b"", b"b"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_name_is_an_exact_path_on_unix() {
        let path = GitName::from_bytes(b"d/caf\xe9").os_path().unwrap();
        assert_eq!(GitName::from_os_str(path.as_os_str()).bytes(), b"d/caf\xe9");
    }
}
