//! Pin reads anchored by directory handles, without changing portal semantics.
//!
//! The root must already be canonical and absolute. Traversal rejects symlinks
//! and Windows reparse points (including mounted-folder reparse points), and
//! held directory handles prevent ancestor redirection. This is path confinement,
//! not byte-origin isolation against hard links or Unix mount arrangements.
//! Windows handles temporarily deny deletion/rename sharing during each read.

use std::fs::File;
use std::io;
use std::path::{Component, Path};

pub(crate) struct ConfinedRoot {
    #[cfg(unix)]
    directory: File,
    #[cfg(windows)]
    root: std::path::PathBuf,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "source path is not confined")
}

impl ConfinedRoot {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut directory = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open("/")?;
            for component in root.components() {
                match component {
                    Component::RootDir => {}
                    Component::Normal(name) => directory = open_at(&directory, name, true)?,
                    _ => return Err(invalid()),
                }
            }
            Ok(Self { directory })
        }
        #[cfg(windows)]
        {
            let mut path = std::path::PathBuf::new();
            let mut ancestors = Vec::new();
            for component in root.components() {
                path.push(component);
                if matches!(component, Component::RootDir | Component::Normal(_)) {
                    ancestors.push(open_locked(&path, true)?);
                }
            }
            Ok(Self {
                root: root.to_path_buf(),
                _ancestors: ancestors,
            })
        }
        #[cfg(not(any(unix, windows)))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "confined reads unsupported",
        ))
    }

    pub(crate) fn read(&self, relative: &Path, maximum_bytes: u64) -> io::Result<Vec<u8>> {
        self.read_with_hook(relative, maximum_bytes, || Ok(()))
    }

    fn read_with_hook(
        &self,
        relative: &Path,
        maximum_bytes: u64,
        after_open: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<Vec<u8>> {
        let components = relative.components().collect::<Vec<_>>();
        if components.is_empty()
            || components
                .iter()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            let mut parent = self.directory.try_clone()?;
            for component in &components[..components.len() - 1] {
                parent = open_at(&parent, component.as_os_str(), true)?;
            }
            let leaf = components.last().ok_or_else(invalid)?.as_os_str();
            let file = open_at(&parent, leaf, false)?;
            super::read_opened_regular(file, maximum_bytes, after_open, || {
                let linked = open_at(&parent, leaf, false)?;
                Ok((linked.metadata()?, same_file::Handle::from_file(linked)?))
            })
        }
        #[cfg(windows)]
        {
            let mut path = self.root.clone();
            let mut ancestors = Vec::new();
            for component in &components[..components.len() - 1] {
                path.push(component);
                ancestors.push(open_locked(&path, true)?);
            }
            path.push(components.last().ok_or_else(invalid)?);
            let file = open_locked(&path, false)?;
            let result = super::read_opened_regular(file, maximum_bytes, after_open, || {
                let linked = open_locked(&path, false)?;
                Ok((linked.metadata()?, same_file::Handle::from_file(linked)?))
            });
            drop(ancestors);
            result
        }
        #[cfg(not(any(unix, windows)))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "confined reads unsupported",
        ))
    }
}

#[cfg(unix)]
fn open_at(parent: &File, name: &std::ffi::OsStr, directory: bool) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| invalid())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is a live directory descriptor; name is NUL-terminated.
    // The returned owned descriptor is adopted exactly once after checking -1.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn open_locked(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(invalid());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::ConfinedRoot;
    use std::path::Path;

    #[test]
    fn confined_reader_is_bounded_and_rejects_nonregular_or_nonrelative_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("brief.md"), "inside").unwrap();
        std::fs::create_dir(root.path().join("directory")).unwrap();
        let reader = ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        assert!(ConfinedRoot::open(Path::new("relative-root")).is_err());
        assert!(ConfinedRoot::open(&root.path().join("..")).is_err());
        assert_eq!(reader.read(Path::new("brief.md"), 6).unwrap(), b"inside");
        assert!(reader.read(Path::new("brief.md"), 5).is_err());
        assert!(reader.read(Path::new("directory"), 100).is_err());
        assert!(reader.read(Path::new("../brief.md"), 100).is_err());
        assert!(reader.read(&root.path().join("brief.md"), 100).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_swap_after_open_cannot_redirect_confined_read() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("evidence");
        let saved = root.path().join("saved");
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::write(ancestor.join("brief.md"), "inside").unwrap();
        std::fs::write(outside.path().join("brief.md"), "outside").unwrap();
        let reader = ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        let bytes = reader
            .read_with_hook(Path::new("evidence/brief.md"), 100, || {
                std::fs::rename(&ancestor, &saved)?;
                std::os::unix::fs::symlink(outside.path(), &ancestor)
            })
            .unwrap();
        assert_eq!(bytes, b"inside");
        assert!(reader.read(Path::new("evidence/brief.md"), 100).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn confined_leaf_replacement_and_growth_fail() {
        let root = tempfile::tempdir().unwrap();
        let leaf = root.path().join("brief.md");
        std::fs::write(&leaf, "inside").unwrap();
        let reader = ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        assert!(reader
            .read_with_hook(Path::new("brief.md"), 100, || {
                std::fs::rename(&leaf, root.path().join("saved.md"))?;
                std::fs::write(&leaf, "inside")
            })
            .is_err());
        assert!(reader
            .read_with_hook(Path::new("brief.md"), 100, || {
                std::fs::write(&leaf, "longer replacement")
            })
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn confined_fifo_is_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let path = std::ffi::CString::new(root.path().join("pipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: path is a valid NUL-terminated temporary pathname.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let reader = ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        assert!(reader.read(Path::new("pipe"), 100).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn held_windows_ancestors_and_leaf_deny_replacement() {
        let root = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("evidence");
        std::fs::create_dir(&ancestor).unwrap();
        let leaf = ancestor.join("brief.md");
        std::fs::write(&leaf, "inside").unwrap();
        let reader = ConfinedRoot::open(&root.path().canonicalize().unwrap()).unwrap();
        let bytes = reader
            .read_with_hook(Path::new("evidence/brief.md"), 100, || {
                assert!(std::fs::rename(&ancestor, root.path().join("saved")).is_err());
                assert!(std::fs::rename(&leaf, ancestor.join("saved.md")).is_err());
                Ok(())
            })
            .unwrap();
        assert_eq!(bytes, b"inside");
        drop(reader);
    }
}
