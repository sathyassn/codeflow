//! Bounded regular-file reads shared by source verification and scaffolding.
//!
//! No-follow and identity checks cover the final component. Callers must guard
//! ancestors separately; this helper does not provide descriptor-relative path
//! confinement against concurrent hostile directory replacement.

use std::io::Read;
use std::path::Path;

pub(crate) mod confined;
pub(crate) use confined::ConfinedRoot;

/// The bytes of `path`, following a link, read only when it is a regular
/// file. A directory, fifo or device, or a link to one, is refused with
/// `InvalidInput` before it is opened, so a fifo cannot block the reader; a
/// missing path or a dangling link keeps `NotFound`.
pub(crate) fn read_regular(path: &Path) -> std::io::Result<Vec<u8>> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    std::fs::read(path)
}

/// [`read_regular`] as UTF-8 text.
pub(crate) fn read_regular_to_string(path: &Path) -> std::io::Result<String> {
    String::from_utf8(read_regular(path)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub(crate) fn read_bounded_regular(path: &Path, maximum_bytes: u64) -> std::io::Result<Vec<u8>> {
    read_bounded_regular_with_hook(path, maximum_bytes, || Ok(()))
}

pub(crate) fn read_bounded_regular_with_hook(
    path: &Path,
    maximum_bytes: u64,
    after_open: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<Vec<u8>> {
    read_bounded_regular_with_hooks(path, maximum_bytes, || Ok(()), after_open)
}

/// [`read_bounded_regular_with_hook`] with a hook between the path check and
/// the open, where a test swaps the path.
fn read_bounded_regular_with_hooks(
    path: &Path,
    maximum_bytes: u64,
    before_open: impl FnOnce() -> std::io::Result<()>,
    after_open: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is not regular or exceeds its byte limit",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // Non-blocking on Unix: a path swapped for a FIFO after the check above
    // opens at once and the descriptor's type check refuses it, instead of
    // the open waiting for a writer. A regular file reads the same either way.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    before_open()?;
    let file = options.open(path)?;
    // After the read the path is checked again without following a link.
    // On Unix that is a no-follow lstat only, never a second open by path,
    // which would block on a fifo swapped in after the first (TSK-1020
    // review 2): identity is the descriptor's device and inode against the
    // lstat's. Windows has no path fifos; there the path is reopened with
    // the reparse-point flag and no access rights, so a link swapped in
    // after the lstat is opened as itself and refused, never followed, and
    // identity is compared between handles this function opened (review 3).
    read_opened_regular(&file, maximum_bytes, after_open, || {
        reopened_identity(path, || Ok(()))
    })
}

/// The path's state after the read: its no-follow metadata and, on
/// Windows, the identity of a handle reopened on it without following a
/// reparse point. `before_reopen` runs between the two, where a test swaps
/// the path.
#[cfg_attr(not(windows), allow(clippy::unnecessary_wraps))]
fn reopened_identity(
    path: &Path,
    before_reopen: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<(std::fs::Metadata, Option<FileIdentity>)> {
    let after = std::fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        if after.file_type().is_symlink() || !after.is_file() {
            return Ok((after, None));
        }
        before_reopen()?;
        let reopened = std::fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let current = reopened.metadata()?;
        if current.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !current.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file changed while it was being read",
            ));
        }
        Ok((after, Some(FileIdentity::of(&reopened)?)))
    }
    #[cfg(not(windows))]
    {
        let _ = before_reopen;
        Ok((after, None))
    }
}

fn read_opened_regular(
    file: &std::fs::File,
    maximum_bytes: u64,
    after_open: impl FnOnce() -> std::io::Result<()>,
    after_read: impl FnOnce() -> std::io::Result<(std::fs::Metadata, Option<FileIdentity>)>,
) -> std::io::Result<Vec<u8>> {
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed identity or type while opening",
        ));
    }
    let opened_identity = FileIdentity::of(file)?;
    after_open()?;
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file grew beyond its byte limit",
        ));
    }
    // The held descriptor's own identity after the read, then the path's.
    let held = file.metadata()?;
    let held_identity = FileIdentity::of(file)?;
    let (after, linked_identity) = after_read()?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || !same_inode(&opened, &held)
        || held_identity != opened_identity
        || !same_inode(&opened, &after)
        || linked_identity.is_some_and(|linked| linked != opened_identity)
        || opened.len() != held.len()
        || opened.len() != bytes.len() as u64
        || opened.len() != after.len()
        || !stable_metadata(&opened, &after)?
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed while it was being read",
        ));
    }
    Ok(bytes)
}

/// A file's identity, read from a handle this module opened. On Unix it is
/// the device and inode. On Windows it is the volume serial number and the
/// full 128-bit file id from `GetFileInformationByHandleEx(FileIdInfo)`:
/// the 64-bit file index is not unique on `ReFS` (TSK-1020 review 4). A call
/// that fails, or an all-zero id, refuses: no identity is never a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    volume: u64,
    id: u128,
}

impl FileIdentity {
    /// The identity of an open file.
    pub(crate) fn of(file: &std::fs::File) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            Ok(Self {
                volume: metadata.dev(),
                id: u128::from(metadata.ino()),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
            };
            let mut info = FILE_ID_INFO::default();
            // SAFETY: the handle is open for the duration of the call, and
            // `info` is a writable FILE_ID_INFO whose exact size is passed.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle().cast(),
                    FileIdInfo,
                    (&raw mut info).cast(),
                    u32::try_from(std::mem::size_of::<FILE_ID_INFO>()).unwrap_or(u32::MAX),
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Self::from_file_id_info(info.VolumeSerialNumber, info.FileId.Identifier)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = file;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no file identity on this platform",
            ))
        }
    }

    /// The identity a `FileIdInfo` query returned, refused when the id is all
    /// zero, which names no file reliably.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    fn from_file_id_info(volume: u64, identifier: [u8; 16]) -> std::io::Result<Self> {
        if identifier == [0; 16] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the file system gave no reliable file identity",
            ));
        }
        Ok(Self {
            volume,
            id: u128::from_le_bytes(identifier),
        })
    }
}

/// Whether two metadata name the same file: device and inode on Unix. Other
/// platforms compare handles instead, so this holds there.
fn same_inode(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        true
    }
}

fn stable_metadata(left: &std::fs::Metadata, right: &std::fs::Metadata) -> std::io::Result<bool> {
    if left.modified()? != right.modified()? {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(left.ctime() == right.ctime() && left.ctime_nsec() == right.ctime_nsec())
    }
    #[cfg(not(unix))]
    Ok(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{read_bounded_regular, read_bounded_regular_with_hooks, read_regular};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;

    /// The longest a read of a fifo may take. An open of a fifo with no
    /// writer blocks until one appears, so a reader that opens it never
    /// returns; a refusal before the open takes microseconds.
    const PROMPT: Duration = Duration::from_secs(5);

    fn make_fifo(path: &Path) {
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated pathname that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    }

    /// Runs `read` on its own thread and returns what it returned, or panics
    /// after [`PROMPT`]. A blocked reader is released first by opening the
    /// fifo for writing, so the test process does not keep a stuck thread.
    fn within_the_bound(
        fifo: &Path,
        read: impl FnOnce() -> std::io::Result<Vec<u8>> + Send + 'static,
    ) -> std::io::Result<Vec<u8>> {
        let (done, result) = channel();
        std::thread::spawn(move || {
            let _ = done.send(read());
        });
        match result.recv_timeout(PROMPT) {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                let _release = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(fifo);
                panic!("the read of a fifo did not return within {PROMPT:?}: it opened the fifo");
            }
        }
    }

    #[test]
    fn read_bounded_regular_refuses_a_fifo_without_opening_it() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        make_fifo(&fifo);
        let started = std::time::Instant::now();
        let path = fifo.clone();
        let error = within_the_bound(&fifo, move || read_bounded_regular(&path, 1024))
            .expect_err("a fifo is not a regular file");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
        assert!(started.elapsed() < PROMPT, "{:?}", started.elapsed());
    }

    #[test]
    fn a_regular_file_swapped_for_a_fifo_before_the_open_is_refused_promptly() {
        // TSK-1020 review 1: the path passes the regular-file check, then is
        // replaced by a fifo with no writer before the open. The open must
        // not wait for a writer; the descriptor's type check refuses it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        let fifo = path.clone();
        let swapped = path.clone();
        let started = std::time::Instant::now();
        let error = within_the_bound(&fifo, move || {
            read_bounded_regular_with_hooks(
                &swapped,
                1024,
                || {
                    std::fs::remove_file(&swapped)?;
                    make_fifo(&swapped);
                    Ok(())
                },
                || Ok(()),
            )
        })
        .expect_err("a fifo is not a regular file");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
        assert!(started.elapsed() < PROMPT, "{:?}", started.elapsed());
    }

    /// Reads a regular file and, after the first open, replaces the path
    /// with `swap`'s result. The read must refuse at once (TSK-1020 review 2).
    fn refuses_a_swap_after_the_open(swap: fn(&Path)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        let fifo = dir.path().join("pipe");
        let swapped = path.clone();
        let started = std::time::Instant::now();
        let error = within_the_bound(&fifo, move || {
            read_bounded_regular_with_hooks(
                &swapped,
                1024,
                || Ok(()),
                || {
                    std::fs::remove_file(&swapped)?;
                    swap(&swapped);
                    Ok(())
                },
            )
        })
        .expect_err("the path no longer names the opened file");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
        assert!(started.elapsed() < PROMPT, "{:?}", started.elapsed());
    }

    #[test]
    fn a_fifo_swapped_in_after_the_open_is_refused_promptly() {
        refuses_a_swap_after_the_open(|path| {
            make_fifo(&path.with_file_name("pipe"));
            std::fs::rename(path.with_file_name("pipe"), path).unwrap();
        });
    }

    #[test]
    fn a_link_to_a_fifo_swapped_in_after_the_open_is_refused_promptly() {
        refuses_a_swap_after_the_open(|path| {
            let fifo = path.with_file_name("pipe");
            make_fifo(&fifo);
            std::os::unix::fs::symlink(&fifo, path).unwrap();
        });
    }

    #[test]
    fn read_bounded_regular_refuses_a_link_to_a_fifo_without_opening_it() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        make_fifo(&fifo);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&fifo, &link).unwrap();
        let error = within_the_bound(&fifo, move || read_bounded_regular(&link, 1024))
            .expect_err("a link is not a regular file");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
    }

    #[test]
    fn read_regular_refuses_a_fifo_without_opening_it() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        make_fifo(&fifo);
        let path = fifo.clone();
        let error = within_the_bound(&fifo, move || read_regular(&path))
            .expect_err("a fifo is not a regular file");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{error}");
    }

    #[test]
    fn read_bounded_regular_still_reads_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"bytes").unwrap();
        assert_eq!(read_bounded_regular(&file, 1024).unwrap(), b"bytes");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::reopened_identity;

    /// TSK-1020 review 3: a link swapped in between the no-follow metadata
    /// read and the reopen is opened as itself and refused, never followed
    /// to the file it names.
    #[test]
    fn a_link_swapped_in_before_the_reopen_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let other = dir.path().join("other.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::write(&other, b"{}").unwrap();
        let (_, identity) = reopened_identity(&path, || Ok(())).unwrap();
        assert_eq!(
            identity.unwrap(),
            super::FileIdentity::of(&std::fs::File::open(&path).unwrap()).unwrap(),
            "a regular file reopens as itself"
        );
        let swapped = path.clone();
        let error = reopened_identity(&path, || {
            std::fs::remove_file(&swapped)?;
            std::os::windows::fs::symlink_file(&other, &swapped)
        })
        .expect_err("a link is refused, not followed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
    }
}

#[cfg(test)]
mod identity_tests {
    use super::FileIdentity;

    /// TSK-1020 review 4: the whole 128-bit id decides. Two ids that share
    /// their low 64 bits, as `ReFS` ids can, are different files.
    #[test]
    fn identities_compare_the_volume_and_the_full_128_bit_id() {
        let low = [7u8, 0, 0, 0, 0, 0, 0, 0];
        let mut first = [0u8; 16];
        let mut second = [0u8; 16];
        first[..8].copy_from_slice(&low);
        second[..8].copy_from_slice(&low);
        first[15] = 1;
        second[15] = 2;
        let a = FileIdentity::from_file_id_info(9, first).unwrap();
        let b = FileIdentity::from_file_id_info(9, second).unwrap();
        assert_ne!(a, b, "a shared low 64 bits is not the same file");
        assert_eq!(a, FileIdentity::from_file_id_info(9, first).unwrap());
        assert_ne!(
            a,
            FileIdentity::from_file_id_info(10, first).unwrap(),
            "another volume"
        );
        let error = FileIdentity::from_file_id_info(9, [0; 16]).expect_err("no reliable identity");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn an_open_file_has_its_own_identity() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (dir.path().join("one"), dir.path().join("two"));
        std::fs::write(&one, b"1").unwrap();
        std::fs::write(&two, b"2").unwrap();
        let first = FileIdentity::of(&std::fs::File::open(&one).unwrap()).unwrap();
        assert_eq!(
            first,
            FileIdentity::of(&std::fs::File::open(&one).unwrap()).unwrap()
        );
        assert_ne!(
            first,
            FileIdentity::of(&std::fs::File::open(&two).unwrap()).unwrap()
        );
    }
}
