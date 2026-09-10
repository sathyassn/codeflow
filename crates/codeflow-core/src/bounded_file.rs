//! Bounded regular-file reads shared by source verification and scaffolding.
//!
//! No-follow and identity checks cover the final component. Callers must guard
//! ancestors separately; this helper does not provide descriptor-relative path
//! confinement against concurrent hostile directory replacement.

use std::io::Read;
use std::path::Path;

mod confined;
pub(crate) use confined::ConfinedRoot;

pub(crate) fn read_bounded_regular(path: &Path, maximum_bytes: u64) -> std::io::Result<Vec<u8>> {
    read_bounded_regular_with_hook(path, maximum_bytes, || Ok(()))
}

pub(crate) fn read_bounded_regular_with_hook(
    path: &Path,
    maximum_bytes: u64,
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
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
