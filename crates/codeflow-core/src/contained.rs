//! Repository-contained file I/O anchored by held directory handles.
//!
//! [`Tree`] opens a trusted root and resolves every repository-relative path
//! one component at a time from it, never following a symbolic link, a
//! junction or another reparse point. A link, or a non-directory where a
//! directory is needed, at any component refuses the operation, and missing
//! folders are created through the same walk. Record writers under
//! `project-management/` and `docs/` (issue 94) and the docs portal's
//! transaction I/O share this one helper. The root itself is trusted and
//! canonicalized; it may sit under a link, as a macOS `/var` tempdir does.
//!
//! Limits. Every operation is relative to held parent handles, so an
//! ancestor swapped for a link after the walk cannot redirect it. A held
//! directory that is moved stays the anchor (an object anchor, not lexical
//! containment): a same-user process that renames it outside the tree during
//! the call carries the written file with it. Unix leaf replacement and
//! deletion are name-relative after the type check, so a link raced in at
//! the leaf can be replaced or removed, never followed. Hard links and Unix
//! mounts are not isolated; git never checks out a hard link. On Windows
//! every reparse point is refused, held directory handles deny delete
//! sharing but not content changes, and a directory sync is a no-op. A link
//! that git checks out as a plain file (`core.symlinks=false`) fails the walk
//! as a non-directory above the leaf and is an ordinary file at the leaf. A
//! refusal names the linked component from a lexical walk made after the
//! failure; under a race that wording is best effort, the refusal is not.

use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::bounded_file::confined::ConfinedDirectory;
use crate::bounded_file::ConfinedRoot;

/// A trusted root and its held handle.
pub(crate) struct Tree {
    path: PathBuf,
    anchor: ConfinedRoot,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unsafe repository path or file type",
    )
}

fn checked(relative: &str) -> io::Result<&Path> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 4096
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(path)
}

/// `path` relative to `root` as the `/`-separated form [`Tree`] takes.
///
/// # Errors
///
/// When `path` is not beneath `root` or has a component that is not a name.
pub(crate) fn relative_to(root: &Path, path: &Path) -> io::Result<String> {
    let rest = path.strip_prefix(root).map_err(|_| invalid())?;
    let mut parts = Vec::new();
    for component in rest.components() {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        parts.push(name.to_str().ok_or_else(invalid)?);
    }
    Ok(parts.join("/"))
}

/// Create `relative` under `root`, as [`Tree::create_new`] does.
///
/// # Errors
///
/// As [`Tree::open`] and [`Tree::create_new`].
pub(crate) fn create_new(root: &Path, relative: &str, bytes: &[u8]) -> io::Result<()> {
    Tree::open(root)?.create_new(relative, bytes)
}

impl Tree {
    /// Anchor the canonical form of `root`.
    ///
    /// # Errors
    ///
    /// When `root` cannot be canonicalized or opened.
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        let path = root.canonicalize()?;
        let anchor = ConfinedRoot::open(&path)?;
        Ok(Self { path, anchor })
    }

    /// The canonical root.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Run `operation`; when it fails and a component of `relative` is a
    /// link, say which one, keeping the error's kind.
    fn explained<T>(
        &self,
        relative: &str,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        operation().map_err(|error| match self.linked_component(relative) {
            Some(link) => io::Error::new(
                error.kind(),
                format!(
                    "{link} is a symbolic link or junction; CodeFlow never reads or writes \
                     repository files through one"
                ),
            ),
            None => error,
        })
    }

    /// The first component of `relative` that is a link or reparse point,
    /// found without following. It only words an error.
    fn linked_component(&self, relative: &str) -> Option<String> {
        let mut path = self.path.clone();
        let mut shown: Vec<&str> = Vec::new();
        for part in relative.split('/').filter(|part| !part.is_empty()) {
            path.push(part);
            shown.push(part);
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if is_link(&metadata) {
                return Some(shown.join("/"));
            }
        }
        None
    }

    /// Enter each component of `path`, creating a missing folder when
    /// `create` is set.
    fn walk(&self, path: &Path, create: bool) -> io::Result<ConfinedDirectory> {
        let mut directory = self.anchor.directory(Path::new(""))?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                return Err(invalid());
            };
            match directory.enter(name) {
                Ok(()) => {}
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    match platform::mkdir(&directory, name) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                    platform::sync(&directory)?;
                    directory.enter(name)?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(directory)
    }

    fn parent(&self, relative: &str, create: bool) -> io::Result<(ConfinedDirectory, OsString)> {
        let path = checked(relative)?;
        let directory = self.walk(path.parent().ok_or_else(invalid)?, create)?;
        Ok((
            directory,
            path.file_name().ok_or_else(invalid)?.to_os_string(),
        ))
    }

    /// Read a regular file of at most `maximum` bytes.
    ///
    /// # Errors
    ///
    /// When a component is a link or the leaf is not a regular file.
    pub(crate) fn read(&self, relative: &str, maximum: u64) -> io::Result<Vec<u8>> {
        self.explained(relative, || self.anchor.read(checked(relative)?, maximum))
    }

    /// The leaf's metadata; a link at the leaf is refused.
    ///
    /// # Errors
    ///
    /// When a component is a link or the leaf is missing or special.
    pub(crate) fn metadata(&self, relative: &str) -> io::Result<Metadata> {
        self.explained(relative, || {
            let (parent, name) = self.parent(relative, false)?;
            platform::metadata(&parent, &name)
        })
    }

    /// Replace the leaf atomically, creating missing folders: the bytes go
    /// to a new temporary file beside it, which takes an existing leaf's
    /// Unix permission bits before any byte is written, is synced, then
    /// renamed over the leaf, and the folder is synced. A failure before the
    /// rename leaves the old content and removes the temporary file when it
    /// can (a failed removal leaves it behind); one after the rename leaves
    /// the new content and still returns the error, so a caller that must
    /// undo it writes the old bytes back.
    ///
    /// # Errors
    ///
    /// When a component is a link, the leaf is not a regular file, or I/O
    /// fails.
    pub(crate) fn write(&self, relative: &str, bytes: &[u8]) -> io::Result<()> {
        self.explained(relative, || {
            self.write_with_hook(relative, bytes, || Ok(()))
        })
    }

    fn write_with_hook(
        &self,
        relative: &str,
        bytes: &[u8],
        hook: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let (parent, name) = self.parent(relative, true)?;
        let existing = match platform::metadata(&parent, &name) {
            Ok(metadata) if !metadata.is_file() => return Err(invalid()),
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let temporary = OsString::from(format!(".codeflow-{}.tmp", ulid::Ulid::new()));
        let mut file = platform::create_file(&parent, &temporary, true)?;
        let result = (|| {
            #[cfg(unix)]
            if let Some(permissions) = existing {
                file.set_permissions(permissions)?;
            }
            #[cfg(not(unix))]
            let _ = existing;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            hook()?;
            platform::rename(&parent, &temporary, &parent, &name)?;
            #[cfg(test)]
            fault::check(fault::Point::AfterRename, relative)?;
            platform::sync(&parent)
        })();
        if result.is_err() {
            let _ = platform::remove(&parent, &temporary, false);
        }
        result
    }

    /// Create the leaf exclusively, creating missing folders. An existing
    /// leaf of any kind, a link included, is refused; a failed write or
    /// sync removes the new file when it can (a failed removal leaves it
    /// behind).
    ///
    /// # Errors
    ///
    /// When a component is a link, the leaf exists, or I/O fails.
    pub(crate) fn create_new(&self, relative: &str, bytes: &[u8]) -> io::Result<()> {
        self.explained(relative, || {
            let (parent, name) = self.parent(relative, true)?;
            let mut file = platform::create_file(&parent, &name, true)?;
            let written = file.write_all(bytes).and_then(|()| file.sync_all());
            #[cfg(test)]
            let written = written.and_then(|()| fault::check(fault::Point::AfterCreate, relative));
            drop(file);
            let result = written.and_then(|()| platform::sync(&parent));
            if result.is_err() {
                let _ = platform::remove(&parent, &name, false);
            }
            result
        })
    }

    /// Create every missing folder of `relative` through the same walk.
    ///
    /// # Errors
    ///
    /// When a component is a link or not a folder, or I/O fails.
    pub(crate) fn create_dir_all(&self, relative: &str) -> io::Result<()> {
        self.explained(relative, || self.walk(checked(relative)?, true).map(drop))
    }

    /// Remove a regular file; a missing one is not an error.
    ///
    /// # Errors
    ///
    /// When a component is a link, the leaf is not a regular file, or I/O
    /// fails.
    pub(crate) fn remove(&self, relative: &str) -> io::Result<()> {
        self.explained(relative, || {
            self.remove_with_hook(relative, false, || Ok(()))
        })
    }

    /// Remove an empty folder; a missing one is not an error.
    ///
    /// # Errors
    ///
    /// As [`Tree::remove`], and when the folder is not empty.
    pub(crate) fn remove_empty(&self, relative: &str) -> io::Result<()> {
        self.explained(relative, || {
            self.remove_with_hook(relative, true, || Ok(()))
        })
    }

    fn remove_with_hook(
        &self,
        relative: &str,
        directory: bool,
        hook: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let (parent, name) = match self.parent(relative, false) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        match platform::metadata(&parent, &name) {
            Ok(metadata) if metadata.is_dir() == directory && (directory || metadata.is_file()) => {
            }
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
        hook()?;
        platform::remove(&parent, &name, directory)?;
        platform::sync(&parent)
    }

    /// Create one folder whose parents exist or are created.
    ///
    /// # Errors
    ///
    /// When a component is a link, the folder exists, or I/O fails.
    pub(crate) fn mkdir(&self, relative: &str) -> io::Result<()> {
        self.explained(relative, || {
            let (parent, name) = self.parent(relative, true)?;
            platform::mkdir(&parent, &name)?;
            platform::sync(&parent)
        })
    }

    /// Rename `from` to `to`, which must not exist.
    ///
    /// # Errors
    ///
    /// When a component of either path is a link, `to` exists, or I/O
    /// fails.
    pub(crate) fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.explained(from, || {
            self.explained(to, || self.rename_with_hook(from, to, || Ok(())))
        })
    }

    fn rename_with_hook(
        &self,
        from: &str,
        to: &str,
        hook: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let (old_parent, old_name) = self.parent(from, false)?;
        let (new_parent, new_name) = self.parent(to, false)?;
        platform::metadata(&old_parent, &old_name)?;
        match platform::metadata(&new_parent, &new_name) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "rename destination already exists",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        hook()?;
        platform::rename(&old_parent, &old_name, &new_parent, &new_name)?;
        platform::sync(&old_parent)?;
        platform::sync(&new_parent)
    }

    /// The sorted names in a folder, at most `maximum` of them.
    ///
    /// # Errors
    ///
    /// When a component is a link, or the folder holds more names.
    pub(crate) fn entries(&self, relative: &str, maximum: usize) -> io::Result<Vec<OsString>> {
        self.explained(relative, || {
            let directory = self.anchor.directory(checked(relative)?)?;
            platform::entries(&directory, maximum)
        })
    }

    /// Open (creating if needed) a regular file to hold a lock on.
    ///
    /// # Errors
    ///
    /// When a component is a link or the leaf is not a regular file.
    pub(crate) fn lock_file(&self, relative: &str) -> io::Result<File> {
        self.explained(relative, || {
            let (parent, name) = self.parent(relative, true)?;
            platform::create_file(&parent, &name, false)
        })
    }
}

/// Test-only failure injection at the points a disk error is otherwise hard
/// to provoke: after a replacement's rename and after a new file's write.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::RefCell;
    use std::io;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Point {
        AfterRename,
        AfterCreate,
    }

    thread_local! {
        static ARMED: RefCell<Option<(Point, String)>> = const { RefCell::new(None) };
    }

    /// Fail the next operation on `relative` at `point` on this thread.
    pub(crate) fn arm(point: Point, relative: &str) {
        ARMED.with(|armed| *armed.borrow_mut() = Some((point, relative.to_string())));
    }

    pub(super) fn check(point: Point, relative: &str) -> io::Result<()> {
        ARMED.with(|armed| {
            let mut armed = armed.borrow_mut();
            if armed
                .as_ref()
                .is_some_and(|(at, path)| *at == point && path == relative)
            {
                *armed = None;
                return Err(io::Error::other(format!("injected failure at {point:?}")));
            }
            Ok(())
        })
    }
}

#[cfg(unix)]
fn is_link(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(unix)]
mod platform {
    use super::{invalid, io, ConfinedDirectory, File, Metadata, OsStr, OsString};
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    fn name(name: &OsStr) -> io::Result<std::ffi::CString> {
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 255
            || matches!(bytes, b"." | b"..")
            || bytes.contains(&b'/')
        {
            return Err(invalid());
        }
        std::ffi::CString::new(bytes).map_err(|_| invalid())
    }

    #[test]
    fn basename_rejects_nonlocal_and_unbounded_names() {
        for bad in ["", ".", "..", "/absolute", "nested/leaf", "nul\0byte"] {
            assert!(name(OsStr::new(bad)).is_err(), "accepted {bad:?}");
        }
        assert!(name(OsStr::new(&"a".repeat(256))).is_err());
        for valid in ["leaf", ".hidden", "a\\b:c", &"a".repeat(255)] {
            assert_eq!(
                name(OsStr::new(valid)).unwrap().as_bytes(),
                valid.as_bytes()
            );
        }
    }

    fn result(value: libc::c_int) -> io::Result<()> {
        if value == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn create_file(
        parent: &ConfinedDirectory,
        leaf: &OsStr,
        exclusive: bool,
    ) -> io::Result<File> {
        if !exclusive {
            match metadata(parent, leaf) {
                Ok(metadata) if !metadata.is_file() => return Err(invalid()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let name = name(leaf)?;
        let flags = libc::O_RDWR
            | libc::O_CREAT
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if exclusive { libc::O_EXCL } else { 0 };
        // SAFETY: live directory descriptor and NUL-terminated basename; mode is
        // supplied because O_CREAT is set. A successful descriptor is owned once.
        let descriptor =
            unsafe { libc::openat(parent.file.as_raw_fd(), name.as_ptr(), flags, 0o666) };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned this owned descriptor.
        let file = unsafe { File::from_raw_fd(descriptor) };
        if !file.metadata()?.is_file() {
            return Err(invalid());
        }
        Ok(file)
    }

    pub(super) fn metadata(parent: &ConfinedDirectory, leaf: &OsStr) -> io::Result<Metadata> {
        let name = name(leaf)?;
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid directory/name and writable stat storage. Refuse special
        // files before open so a pre-planted device is not opened for metadata.
        result(unsafe {
            libc::fstatat(
                parent.file.as_raw_fd(),
                name.as_ptr(),
                info.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })?;
        // SAFETY: fstatat succeeded and initialized info.
        let kind = unsafe { info.assume_init() }.st_mode & libc::S_IFMT;
        if !matches!(kind, libc::S_IFREG | libc::S_IFDIR) {
            return Err(invalid());
        }
        // SAFETY: directory and basename are valid; O_NONBLOCK prevents FIFO waits.
        let descriptor = unsafe {
            libc::openat(
                parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful openat returned an owned descriptor.
        unsafe { File::from_raw_fd(descriptor) }.metadata()
    }

    pub(super) fn mkdir(parent: &ConfinedDirectory, leaf: &OsStr) -> io::Result<()> {
        let name = name(leaf)?;
        // SAFETY: valid held parent and basename.
        result(unsafe { libc::mkdirat(parent.file.as_raw_fd(), name.as_ptr(), 0o777) })
    }

    pub(super) fn rename(
        old: &ConfinedDirectory,
        from: &OsStr,
        new: &ConfinedDirectory,
        to: &OsStr,
    ) -> io::Result<()> {
        let from = name(from)?;
        let to = name(to)?;
        // SAFETY: both directory descriptors and basenames remain live.
        result(unsafe {
            libc::renameat(
                old.file.as_raw_fd(),
                from.as_ptr(),
                new.file.as_raw_fd(),
                to.as_ptr(),
            )
        })
    }

    pub(super) fn remove(
        parent: &ConfinedDirectory,
        leaf: &OsStr,
        directory: bool,
    ) -> io::Result<()> {
        let name = name(leaf)?;
        // SAFETY: valid held parent and basename. This is name-relative deletion,
        // deliberately not represented as authenticated-inode compare-and-swap.
        result(unsafe {
            libc::unlinkat(
                parent.file.as_raw_fd(),
                name.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        })
    }

    pub(super) fn sync(parent: &ConfinedDirectory) -> io::Result<()> {
        parent.file.sync_all()
    }

    pub(super) fn entries(parent: &ConfinedDirectory, maximum: usize) -> io::Result<Vec<OsString>> {
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                // SAFETY: this stream uniquely owns the successful fdopendir.
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let descriptor = parent.file.try_clone()?.into_raw_fd();
        // SAFETY: fdopendir takes ownership only on success.
        let pointer = unsafe { libc::fdopendir(descriptor) };
        if pointer.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: fdopendir failed; this descriptor is still ours.
            drop(unsafe { File::from_raw_fd(descriptor) });
            return Err(error);
        }
        let stream = Stream(pointer);
        // SAFETY: rewind the owned directory stream before enumeration.
        unsafe {
            libc::rewinddir(stream.0);
        }
        let mut entries = Vec::new();
        loop {
            clear_errno()?;
            // SAFETY: live uniquely owned stream; d_name is copied before readdir.
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error);
                }
                break;
            }
            // SAFETY: successful readdir provides a NUL-terminated d_name.
            let bytes = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            if entries.len() >= maximum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory entry limit exceeded",
                ));
            }
            entries.push(OsString::from_vec(bytes.to_vec()));
        }
        entries.sort();
        Ok(entries)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn clear_errno() -> io::Result<()> {
        #[cfg(target_os = "macos")]
        // SAFETY: libc returns this thread's writable errno location.
        unsafe {
            *libc::__error() = 0;
        }
        #[cfg(target_os = "linux")]
        // SAFETY: libc returns this thread's writable errno location.
        unsafe {
            *libc::__errno_location() = 0;
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Ok(())
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "directory enumeration is unsupported on this platform",
            ))
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::{invalid, io, ConfinedDirectory, File, Metadata, OsStr, OsString};
    use crate::bounded_file::confined::windows::{basename, check, nt_result, open_relative};
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FileRenameInformation, NtSetInformationFile, FILE_CREATE, FILE_OPEN,
        FILE_RENAME_INFORMATION,
    };
    use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandleEx, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_TRAVERSE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    fn open(
        parent: &ConfinedDirectory,
        leaf: &OsStr,
        access: u32,
        create: bool,
    ) -> io::Result<File> {
        open_relative(
            parent.ancestors.last().ok_or_else(invalid)?,
            leaf,
            access,
            if create { FILE_CREATE } else { FILE_OPEN },
            create.then_some(false),
        )
    }

    pub(super) fn create_file(
        parent: &ConfinedDirectory,
        leaf: &OsStr,
        exclusive: bool,
    ) -> io::Result<File> {
        let created = open(parent, leaf, FILE_GENERIC_READ | FILE_GENERIC_WRITE, true);
        let file = match created {
            Err(error) if !exclusive && error.kind() == io::ErrorKind::AlreadyExists => {
                // One bounded fallback, no retry on a racing disappearance.
                // Existing lease files are opened without DELETE access.
                open(parent, leaf, FILE_GENERIC_READ | FILE_GENERIC_WRITE, false)?
            }
            result => result?,
        };
        if !file.metadata()?.is_file() {
            return Err(invalid());
        }
        Ok(file)
    }

    pub(super) fn metadata(parent: &ConfinedDirectory, leaf: &OsStr) -> io::Result<Metadata> {
        open(parent, leaf, FILE_READ_ATTRIBUTES, false)?.metadata()
    }

    pub(super) fn mkdir(parent: &ConfinedDirectory, leaf: &OsStr) -> io::Result<()> {
        open_relative(
            parent.ancestors.last().ok_or_else(invalid)?,
            leaf,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_CREATE,
            Some(true),
        )?;
        Ok(())
    }

    pub(super) fn rename(
        old: &ConfinedDirectory,
        from: &OsStr,
        new: &ConfinedDirectory,
        to: &OsStr,
    ) -> io::Result<()> {
        let name = basename(to)?;
        let source = open(old, from, DELETE | FILE_READ_ATTRIBUTES, false)?;
        // Kernel FILE_RENAME_INFORMATION, not Win32 FILE_RENAME_INFO: the
        // Win32 wrapper reads FileName as a NUL-terminated path relative to the
        // process current directory, while the kernel resolves a simple name
        // under RootDirectory. The kernel requires at least the header size
        // plus the name bytes, even for a one-character name.
        let name_bytes = name.len() * std::mem::size_of::<u16>();
        let size = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name_bytes;
        let mut storage = vec![0_usize; size.div_ceil(std::mem::size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        let parent = new.ancestors.last().ok_or_else(invalid)?;
        check(parent, Some(true))?;
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: zeroed storage is pointer-aligned and sized for the header
        // plus the counted UTF-16 basename, written through a pointer derived
        // from the whole allocation. Source and destination parent stay held.
        let status = unsafe {
            (*info).Anonymous.ReplaceIfExists = true;
            (*info).RootDirectory = parent.as_raw_handle();
            (*info).FileNameLength = u32::try_from(name_bytes).map_err(|_| invalid())?;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                (&raw mut (*info).FileName).cast::<u16>(),
                name.len(),
            );
            NtSetInformationFile(
                source.as_raw_handle(),
                &raw mut status_block,
                info.cast(),
                u32::try_from(size).map_err(|_| invalid())?,
                FileRenameInformation,
            )
        };
        nt_result(status)?;
        check(parent, Some(true))?;
        Ok(())
    }

    pub(super) fn remove(
        parent: &ConfinedDirectory,
        leaf: &OsStr,
        directory: bool,
    ) -> io::Result<()> {
        let file = open(parent, leaf, DELETE | FILE_READ_ATTRIBUTES, false)?;
        let metadata = file.metadata()?;
        if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
            return Err(invalid());
        }
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: live opened handle and a correctly sized disposition structure.
        let success = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&raw const info).cast(),
                u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO>())
                    .map_err(|_| invalid())?,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    // Windows has no directory-fsync parity here. File contents are flushed
    // before publication; this no-op must not imply Unix crash-durability proof.
    #[allow(clippy::unnecessary_wraps)]
    pub(super) fn sync(_: &ConfinedDirectory) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn entries(parent: &ConfinedDirectory, maximum: usize) -> io::Result<Vec<OsString>> {
        let handle = parent.ancestors.last().ok_or_else(invalid)?;
        check(handle, Some(true))?;
        let mut entries = Vec::new();
        // FILE_ID_BOTH_DIR_INFO requires 8-byte alignment. The Restart class
        // resets a previously used directory handle; subsequent calls resume.
        let mut buffer = vec![0_u64; 8192];
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            buffer.fill(0);
            // SAFETY: live directory handle, writable 64-KiB aligned buffer.
            let success = unsafe {
                GetFileInformationByHandleEx(
                    handle.as_raw_handle(),
                    class,
                    buffer.as_mut_ptr().cast(),
                    65_536,
                )
            };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES.cast_signed()) {
                    break;
                }
                return Err(error);
            }
            class = FileIdBothDirectoryInfo;
            // SAFETY: u64 storage can be viewed as bytes for bounded parsing.
            let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), 65_536) };
            let name_offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            let length_offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
            let mut offset = 0_usize;
            loop {
                let remaining = bytes.get(offset..).ok_or_else(invalid)?;
                let integer = |at: usize| -> io::Result<u32> {
                    let value = remaining.get(at..at + 4).ok_or_else(invalid)?;
                    Ok(u32::from_ne_bytes(value.try_into().map_err(|_| invalid())?))
                };
                let next = integer(0)? as usize;
                let length = integer(length_offset)? as usize;
                if length == 0 || !length.is_multiple_of(2) || length > 510 {
                    return Err(invalid());
                }
                let end = name_offset.checked_add(length).ok_or_else(invalid)?;
                let name_bytes = remaining.get(name_offset..end).ok_or_else(invalid)?;
                let name: Vec<u16> = name_bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
                    .collect();
                if name != [46] && name != [46, 46] {
                    if entries.len() >= maximum {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "directory entry limit exceeded",
                        ));
                    }
                    entries.push(OsString::from_wide(&name));
                }
                if next == 0 {
                    break;
                }
                if next < end || !next.is_multiple_of(8) {
                    return Err(invalid());
                }
                offset = offset.checked_add(next).ok_or_else(invalid)?;
            }
        }
        check(handle, Some(true))?;
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_io_creates_replaces_lists_and_removes_without_traversal() {
        let root = tempfile::tempdir().unwrap();
        let io = Tree::open(root.path()).unwrap();
        io.write("guide/nested/value", b"first").unwrap();
        io.write("guide/nested/value", b"next").unwrap();
        assert_eq!(io.read("guide/nested/value", 4).unwrap(), b"next");
        assert!(io.read("guide/nested/value", 3).is_err());
        assert_eq!(
            io.entries("guide/nested", 1).unwrap(),
            [OsString::from("value")]
        );
        assert!(io.entries("guide/nested", 0).is_err());
        io.remove("guide/nested/value").unwrap();
        io.remove("guide/nested/value").unwrap();
        io.remove_empty("guide/nested").unwrap();
        // A one-character name is shorter than the rename header's padding, and
        // a cross-directory rename resolves only under the held destination.
        io.write("guide/a", b"short").unwrap();
        io.mkdir("moved").unwrap();
        io.rename("guide/a", "moved/b").unwrap();
        assert_eq!(io.read("moved/b", 5).unwrap(), b"short");
        assert!(io.read("guide/a", 5).is_err());
        assert_eq!(io.entries("guide", 1).unwrap(), Vec::<OsString>::new());
        for bad in [
            "",
            "../outside",
            "/outside",
            "guide/../outside",
            "guide/./file",
            "guide//file",
        ] {
            assert!(io.write(bad, b"bad").is_err());
            assert!(io.remove(bad).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_swaps_cannot_redirect_runtime_state_or_baseline_writes_and_deletes() {
        for directory in ["guide", ".codeflow", ".codeflow/.docs-portal-baseline"] {
            for deleting in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let outside = tempfile::tempdir().unwrap();
                std::fs::create_dir_all(root.path().join(directory)).unwrap();
                std::fs::write(root.path().join(directory).join("value"), b"inside").unwrap();
                std::fs::write(outside.path().join("value"), b"outside sentinel").unwrap();
                let io = Tree::open(root.path()).unwrap();
                let path = format!("{directory}/value");
                let saved = root.path().join("held-directory");
                let hook = || {
                    std::fs::rename(root.path().join(directory), &saved)?;
                    std::os::unix::fs::symlink(outside.path(), root.path().join(directory))
                };
                if deleting {
                    io.remove_with_hook(&path, false, hook).unwrap();
                    assert!(!saved.join("value").exists());
                } else {
                    io.write_with_hook(&path, b"next", hook).unwrap();
                    assert_eq!(std::fs::read(saved.join("value")).unwrap(), b"next");
                }
                assert_eq!(
                    std::fs::read(outside.path().join("value")).unwrap(),
                    b"outside sentinel"
                );
                assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
                assert!(io.read(&path, 100).is_err());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn journal_rename_remains_relative_to_held_parent_during_ancestor_swap() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".codeflow/stage")).unwrap();
        std::fs::write(root.path().join(".codeflow/stage/manifest"), "inside").unwrap();
        std::fs::create_dir(outside.path().join("transaction")).unwrap();
        std::fs::write(outside.path().join("transaction/sentinel"), "outside").unwrap();
        let io = Tree::open(root.path()).unwrap();
        io.rename_with_hook(".codeflow/stage", ".codeflow/transaction", || {
            std::fs::rename(root.path().join(".codeflow"), root.path().join("held"))?;
            std::os::unix::fs::symlink(outside.path(), root.path().join(".codeflow"))
        })
        .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("held/transaction/manifest")).unwrap(),
            b"inside"
        );
        assert_eq!(
            std::fs::read(outside.path().join("transaction/sentinel")).unwrap(),
            b"outside"
        );
        assert_eq!(
            std::fs::read_dir(outside.path().join("transaction"))
                .unwrap()
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn preplanted_symlinks_and_special_files_are_refused_without_external_changes() {
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("sentinel"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("redirect")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("sentinel"), root.path().join("leaf"))
            .unwrap();
        let io = Tree::open(root.path()).unwrap();
        for path in ["redirect/sentinel", "leaf"] {
            assert!(io.write(path, b"bad").is_err());
            assert!(io.remove(path).is_err());
            assert!(io.lock_file(path).is_err());
        }
        let pipe = std::ffi::CString::new(root.path().join("pipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: valid NUL-terminated fixture pathname.
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        assert!(io.write("pipe", b"bad").is_err());
        assert!(io.remove("pipe").is_err());
        assert!(io.lock_file("pipe").is_err());
        assert_eq!(
            std::fs::read(outside.path().join("sentinel")).unwrap(),
            b"outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn moved_directory_is_an_object_anchor_not_lexical_root_isolation() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("guide")).unwrap();
        let io = Tree::open(root.path()).unwrap();
        io.write_with_hook("guide/value", b"anchored object", || {
            std::fs::rename(root.path().join("guide"), outside.path().join("moved"))
        })
        .unwrap();
        assert_eq!(
            std::fs::read(outside.path().join("moved/value")).unwrap(),
            b"anchored object"
        );
    }

    #[cfg(windows)]
    #[test]
    fn in_place_reparse_on_held_directory_refuses_operations() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("empty")).unwrap();
        std::fs::write(outside.path().join("sentinel"), b"outside").unwrap();
        let io = Tree::open(root.path()).unwrap();
        let parent = io.anchor.directory(Path::new("empty")).unwrap();
        let redirect = crate::bounded_file::confined::windows::tests::junction(
            &root.path().join("empty"),
            outside.path(),
        )
        .unwrap();
        assert!(platform::entries(&parent, 10).is_err());
        assert!(platform::metadata(&parent, OsStr::new("sentinel")).is_err());
        assert!(platform::create_file(&parent, OsStr::new("created"), true).is_err());
        assert!(platform::mkdir(&parent, OsStr::new("created-directory")).is_err());
        assert!(platform::remove(&parent, OsStr::new("sentinel"), false).is_err());
        assert_eq!(
            std::fs::read(outside.path().join("sentinel")).unwrap(),
            b"outside"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
        drop(redirect);
    }

    #[cfg(windows)]
    #[test]
    fn windows_held_parents_deny_rename_until_operation_releases_them() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("guide")).unwrap();
        let io = Tree::open(root.path()).unwrap();
        io.write_with_hook("guide/value", b"inside", || {
            assert!(std::fs::rename(root.path().join("guide"), root.path().join("moved")).is_err());
            Ok(())
        })
        .unwrap();
        std::fs::rename(root.path().join("guide"), root.path().join("moved")).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("moved/value")).unwrap(),
            b"inside"
        );
    }

    /// Issue 94: a link at any component, the leaf included and a dangling
    /// one, refuses every operation, names the component and changes
    /// nothing where it points.
    #[cfg(unix)]
    #[test]
    fn a_link_at_any_component_is_refused_and_named() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("tasks")).unwrap();
        std::fs::write(outside.path().join("tasks/TSK-001.md"), "outside").unwrap();
        std::fs::write(outside.path().join("sentinel"), "outside").unwrap();
        let cases = [
            (
                "project-management",
                outside.path().to_path_buf(),
                "project-management",
            ),
            (
                "project-management/tasks",
                outside.path().join("tasks"),
                "project-management/tasks",
            ),
            (
                "project-management/tasks/TSK-001.md",
                outside.path().join("sentinel"),
                "project-management/tasks/TSK-001.md",
            ),
            (
                "project-management/tasks",
                outside.path().join("missing"),
                "project-management/tasks",
            ),
        ];
        for (linked, target, named) in cases {
            let root = tempfile::tempdir().unwrap();
            let link = root.path().join(linked);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&target, &link).unwrap();
            let before = snapshot(outside.path());
            let tree = Tree::open(root.path()).unwrap();
            let record = "project-management/tasks/TSK-001.md";
            let mut errors = vec![
                tree.write(record, b"bad").unwrap_err(),
                tree.create_new(record, b"bad").unwrap_err(),
                tree.read(record, 100).unwrap_err(),
                tree.metadata(record).unwrap_err(),
            ];
            if Path::new(linked).extension().is_none() {
                errors.push(
                    tree.create_new("project-management/tasks/TSK-002.md", b"bad")
                        .unwrap_err(),
                );
                errors.push(
                    tree.create_dir_all("project-management/tasks/nested")
                        .unwrap_err(),
                );
            }
            for error in &errors {
                assert!(
                    error
                        .to_string()
                        .starts_with(&format!("{named} is a symbolic link")),
                    "{linked}: {error}"
                );
            }
            assert!(tree.remove(record).is_err(), "{linked}: nothing is removed");
            assert_eq!(
                snapshot(outside.path()),
                before,
                "{linked}: nothing outside"
            );
            assert!(std::fs::symlink_metadata(&link).is_ok(), "the link stays");
        }
    }

    #[test]
    fn create_new_makes_folders_and_never_replaces() {
        let root = tempfile::tempdir().unwrap();
        let tree = Tree::open(root.path()).unwrap();
        tree.create_new("docs/decisions/ADR-0001-a.md", b"first")
            .unwrap();
        let error = tree
            .create_new("docs/decisions/ADR-0001-a.md", b"second")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(root.path().join("docs/decisions/ADR-0001-a.md")).unwrap(),
            b"first"
        );
        tree.create_dir_all("project-management/epics").unwrap();
        tree.create_dir_all("project-management/epics").unwrap();
        assert!(root.path().join("project-management/epics").is_dir());
        create_new(root.path(), "project-management/epics/EPC-001.md", b"epic").unwrap();
        tree.write("project-management/epics/EPC-001.md", b"next")
            .unwrap();
        tree.remove("project-management/epics/EPC-001.md").unwrap();
        tree.remove("project-management/epics/EPC-001.md").unwrap();
        assert_eq!(
            std::fs::read_dir(root.path().join("project-management/epics"))
                .unwrap()
                .count(),
            0,
            "no temporary file is left behind"
        );
    }

    #[test]
    fn a_failure_after_the_write_or_rename_is_reported_and_cleaned_up() {
        let root = tempfile::tempdir().unwrap();
        let tree = Tree::open(root.path()).unwrap();
        fault::arm(fault::Point::AfterCreate, "docs/new.md");
        assert!(tree.create_new("docs/new.md", b"new").is_err());
        assert!(
            !root.path().join("docs/new.md").exists(),
            "the new file is removed"
        );
        tree.create_new("docs/new.md", b"old").unwrap();
        fault::arm(fault::Point::AfterRename, "docs/new.md");
        assert!(tree.write("docs/new.md", b"next").is_err());
        assert_eq!(
            std::fs::read(root.path().join("docs/new.md")).unwrap(),
            b"next",
            "after the rename the new bytes are in place and the error still returns"
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("docs")).unwrap().count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn replacement_keeps_the_unix_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let tree = Tree::open(root.path()).unwrap();
        for mode in [0o600, 0o755] {
            let path = root.path().join("record.md");
            std::fs::write(&path, "old").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            tree.write("record.md", b"new").unwrap();
            let after = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
            assert_eq!(after, mode);
            assert_eq!(std::fs::read(&path).unwrap(), b"new");
        }
    }

    #[test]
    fn relative_paths_stay_beneath_the_root() {
        let root = Path::new("/repo");
        assert_eq!(
            relative_to(root, Path::new("/repo/project-management/tasks/TSK-001.md")).unwrap(),
            "project-management/tasks/TSK-001.md"
        );
        assert!(relative_to(root, Path::new("/elsewhere/TSK-001.md")).is_err());
        assert!(relative_to(root, Path::new("/repo/../escape")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_at_any_folder_is_refused_and_named() {
        for linked in ["project-management", "project-management/tasks"] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(outside.path().join("tasks")).unwrap();
            let link = root.path().join(linked);
            std::fs::create_dir_all(&link).unwrap();
            let target = if linked.ends_with("tasks") {
                outside.path().join("tasks")
            } else {
                outside.path().to_path_buf()
            };
            let redirect =
                crate::bounded_file::confined::windows::tests::junction(&link, &target).unwrap();
            let tree = Tree::open(root.path()).unwrap();
            let errors = [
                tree.write("project-management/tasks/TSK-001.md", b"bad")
                    .unwrap_err(),
                tree.create_new("project-management/tasks/TSK-001.md", b"bad")
                    .unwrap_err(),
                tree.create_dir_all("project-management/tasks/nested")
                    .unwrap_err(),
            ];
            for error in &errors {
                assert!(
                    error
                        .to_string()
                        .starts_with(&format!("{linked} is a symbolic link")),
                    "{linked}: {error}"
                );
            }
            assert_eq!(
                std::fs::read_dir(outside.path().join("tasks"))
                    .unwrap()
                    .count(),
                0,
                "{linked}: nothing outside"
            );
            drop(redirect);
        }
    }

    /// Every file and folder beneath `dir`, read without following.
    #[cfg(unix)]
    fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current).unwrap().flatten() {
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    stack.push(path.clone());
                }
                let bytes = if kind.is_file() {
                    std::fs::read(&path).unwrap()
                } else {
                    Vec::new()
                };
                out.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
        out
    }
}
