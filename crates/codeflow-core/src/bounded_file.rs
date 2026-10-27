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
    read_opened_regular(file, maximum_bytes, after_open, || {
        Ok((
            std::fs::symlink_metadata(path)?,
            same_file::Handle::from_path(path)?,
        ))
    })
}

fn read_opened_regular(
    file: std::fs::File,
    maximum_bytes: u64,
    after_open: impl FnOnce() -> std::io::Result<()>,
    after_read: impl FnOnce() -> std::io::Result<(std::fs::Metadata, same_file::Handle)>,
) -> std::io::Result<Vec<u8>> {
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed identity or type while opening",
        ));
    }
    let opened_identity = same_file::Handle::from_file(file.try_clone()?)?;
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
    let (after, linked_identity) = after_read()?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || opened_identity != linked_identity
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
