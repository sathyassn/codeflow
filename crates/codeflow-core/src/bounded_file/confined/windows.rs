//! Basename-relative Windows opens shared by confined reads and portal writes.
//!
//! `NtCreateFile` uses the held directory as `OBJECT_ATTRIBUTES.RootDirectory`;
//! no reconstructed ancestor path is parsed. No-delete sharing is supplementary:
//! it does not prevent in-place reparse metadata mutation. The no-reparse options
//! and handle-relative lookup, plus type/reparse checks, provide the boundary.

use super::invalid;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE,
    FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows_sys::Win32::Foundation::{
    RtlNtStatusToDosError, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE,
    SYNCHRONIZE,
};
use windows_sys::Win32::System::WindowsProgramming::FILE_CREATED;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

pub(crate) fn check(file: &File, directory: Option<bool>) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (!metadata.is_file() && !metadata.is_dir())
        || directory.is_some_and(|expected| metadata.is_dir() != expected)
    {
        return Err(invalid());
    }
    Ok(())
}

/// The same bounded name validation applies to relative opens and both rename
/// endpoints. Colons are forbidden so a basename cannot select an NTFS stream.
pub(crate) fn basename(name: &OsStr) -> io::Result<Vec<u16>> {
    let name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty()
        || name.len() > 255
        || name.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
        || name == [46]
        || name == [46, 46]
    {
        return Err(invalid());
    }
    Ok(name)
}

/// Opens exactly one basename relative to a held, regular directory. The caller
/// supplies only an NT open/create disposition and required access/type; this
/// helper never accepts a full path or a fallback path-based operation.
pub(crate) fn open_relative(
    parent: &File,
    name: &OsStr,
    access: u32,
    disposition: u32,
    directory: Option<bool>,
) -> io::Result<File> {
    open_relative_with_hook(parent, name, access, disposition, directory, || Ok(()))
}

fn open_relative_with_hook(
    parent: &File,
    name: &OsStr,
    access: u32,
    disposition: u32,
    directory: Option<bool>,
    after_parent_check: impl FnOnce() -> io::Result<()>,
) -> io::Result<File> {
    let mut name = basename(name)?;
    check(parent, Some(true))?;
    after_parent_check()?;
    let length = u16::try_from(name.len() * 2).map_err(|_| invalid())?;
    let unicode = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(std::mem::size_of::<OBJECT_ATTRIBUTES>()).map_err(|_| invalid())?,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const unicode,
        // Portal state/manifest/journal portable keys already reject aliases;
        // keep Windows lookup explicitly case-insensitive across these paths.
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..Default::default()
    };
    let mut status_block = IO_STATUS_BLOCK::default();
    let mut handle = std::ptr::null_mut();
    let kind = match directory {
        Some(true) => FILE_DIRECTORY_FILE,
        Some(false) => FILE_NON_DIRECTORY_FILE,
        None => 0,
    };
    // SAFETY: all input/output storage and the held parent remain alive through
    // this synchronous call. UNICODE_STRING is a bounded, counted basename, not
    // NUL-terminated input. A successful returned handle is adopted exactly once.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            access
                | SYNCHRONIZE
                | if disposition == FILE_CREATE {
                    DELETE
                } else {
                    0
                },
            &raw const attributes,
            &raw mut status_block,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            disposition,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT | kind,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: pure NTSTATUS-to-Win32 conversion, preserving NotFound etc.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) }.cast_signed(),
        ));
    }
    if handle.is_null() {
        return Err(invalid());
    }
    // SAFETY: successful NtCreateFile supplied this newly owned file handle.
    let file = unsafe { File::from_raw_handle(handle) };
    if let Err(error) = check(&file, directory).and_then(|()| check(parent, Some(true))) {
        discard_rejected_creation(&file, disposition, status_block.Information);
        return Err(error);
    }
    Ok(file)
}

/// Only this call's proven exclusive creation may be disposed. Cleanup is
/// best-effort through that same handle, never a reconstructed name: preserve
/// the original rejection and do not claim zero side effects if disposal fails.
fn discard_rejected_creation(file: &File, disposition: u32, information: usize) {
    if disposition != FILE_CREATE || information != FILE_CREATED as usize {
        return;
    }
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let Ok(size) = u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO>()) else {
        return;
    };
    // SAFETY: this exclusive create requested DELETE; the original owned handle
    // and correctly sized disposition structure remain live. No path is parsed.
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const info).cast(),
            size,
        );
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::Path;
    use windows_sys::Wdk::Storage::FileSystem::{FILE_OPEN, FILE_OPEN_IF};
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Ioctl::{FSCTL_DELETE_REPARSE_POINT, FSCTL_SET_REPARSE_POINT};
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    #[test]
    fn rejected_create_cleanup_requires_both_exclusive_disposition_and_created_status() {
        for (disposition, information, removed) in [
            (FILE_CREATE, FILE_CREATED as usize, true),
            (FILE_CREATE, 1, false), // FILE_OPENED is not a creation claim.
            (FILE_OPEN, FILE_CREATED as usize, false),
            (FILE_OPEN_IF, FILE_CREATED as usize, false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("owned");
            let file = std::fs::OpenOptions::new()
                .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .create_new(true)
                .open(&path)
                .unwrap();
            // Model the post-open rejection branch with explicit kernel outcome
            // claims. A false creation claim/disposition must never delete data.
            discard_rejected_creation(&file, disposition, information);
            drop(file);
            assert_eq!(!path.exists(), removed);
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("existing"), b"preserve").unwrap();
        let anchor =
            super::super::ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        let parent = anchor.directory(Path::new("")).unwrap();
        let file = open_relative(
            parent.ancestors.last().unwrap(),
            OsStr::new("existing"),
            FILE_GENERIC_READ,
            FILE_OPEN,
            Some(false),
        )
        .unwrap();
        discard_rejected_creation(&file, FILE_OPEN, 1);
        drop(file);
        assert_eq!(
            std::fs::read(root.path().join("existing")).unwrap(),
            b"preserve"
        );
    }

    pub(crate) struct Junction(File);

    impl Drop for Junction {
        fn drop(&mut self) {
            let mut bytes = Vec::from(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
            bytes.extend_from_slice(&[0; 4]);
            control(&self.0, FSCTL_DELETE_REPARSE_POINT, &bytes).unwrap();
        }
    }

    fn control(file: &File, code: u32, bytes: &[u8]) -> io::Result<()> {
        let mut returned = 0;
        // SAFETY: live synchronous handle, valid counted input and output size.
        let success = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                code,
                bytes.as_ptr().cast(),
                u32::try_from(bytes.len()).unwrap(),
                std::ptr::null_mut(),
                0,
                &raw mut returned,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Mutates an existing empty directory in place, not a rename/symlink swap.
    /// No DELETE access is requested; the fixture exercises the distinct seam
    /// that holding no-delete-sharing handles alone does not establish safe.
    pub(crate) fn junction(directory: &Path, target: &Path) -> io::Result<Junction> {
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_DATA | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)?;
        let absolute = target.canonicalize()?;
        let target = absolute.to_str().ok_or_else(invalid)?;
        let substitute = format!("\\??\\{}", target.strip_prefix("\\\\?\\").unwrap_or(target));
        let name: Vec<u16> = substitute.encode_utf16().collect();
        let length = u16::try_from(name.len() * 2).map_err(|_| invalid())?;
        let mut bytes = Vec::from(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        // Eight-byte mount-point header, counted substitute + NUL, empty print
        // name + NUL. Offsets are relative to PathBuffer, per REPARSE_DATA_BUFFER.
        bytes.extend_from_slice(&(length + 12).to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        for value in [0_u16, length, length + 2, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for unit in name {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 4]);
        control(&file, FSCTL_SET_REPARSE_POINT, &bytes)?;
        Ok(Junction(file))
    }

    #[test]
    fn in_place_parent_reparse_after_check_never_redirects_relative_opens_or_creates() {
        for (disposition, directory) in [
            (FILE_OPEN, false),
            (FILE_CREATE, false),
            (FILE_CREATE, true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("empty")).unwrap();
            std::fs::write(outside.path().join("sentinel"), b"outside").unwrap();
            let anchor =
                super::super::ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
            let parent = anchor.directory(Path::new("empty")).unwrap();
            let mut redirect = None;
            let name = if disposition == FILE_OPEN {
                "sentinel"
            } else {
                "created"
            };
            let result = open_relative_with_hook(
                parent.ancestors.last().unwrap(),
                OsStr::new(name),
                if directory {
                    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES
                } else {
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE
                },
                disposition,
                Some(directory),
                || {
                    redirect = Some(junction(&root.path().join("empty"), outside.path())?);
                    Ok(())
                },
            );
            assert!(result.is_err(), "changed parent must be refused");
            assert_eq!(
                std::fs::read(outside.path().join("sentinel")).unwrap(),
                b"outside"
            );
            assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
            drop(redirect);
        }
    }
}
