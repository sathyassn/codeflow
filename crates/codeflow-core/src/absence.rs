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
    for ancestor in path.ancestors().skip(1) {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
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
            return Err(io::Error::other(format!(
                "not a directory at {}",
                ancestor.display()
            )));
        }
        return Ok(true);
    }
    // A relative path may have no existing ancestor (including its empty one).
    if path.is_relative() {
        Ok(true)
    } else {
        Err(io::Error::other(format!(
            "cannot establish a directory ancestor for {}",
            path.display()
        )))
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
