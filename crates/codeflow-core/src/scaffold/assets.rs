//! Asset resolution abstraction.
//!
//! The engine never assumes where assets come from. The CLI supplies a
//! rust-embed-backed source over `assets/` (disk in debug builds for instant
//! scaffold iteration, embedded bytes in release — charter §10); tests supply
//! [`DirSource`] over fixture trees.

use std::path::PathBuf;

/// A read-only view over the shipped scaffold assets.
///
/// Paths are relative to the assets root, e.g. `base/git-hooks/pre-commit`
/// or `base/scaffold-manifest.toml`.
pub trait AssetSource {
    /// Returns the raw bytes of the asset at `path`, or `None` if it does not
    /// exist. A missing asset is not an engine error: manifest entries whose
    /// asset is absent are skipped with a warning so the asset-authoring
    /// workstream and this engine can land independently.
    ///
    /// # Errors
    ///
    /// Returns an error when obtaining an asset fails; only genuine source absence is `Ok(None)`.
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>>;
}

/// Filesystem-backed asset source (tests, and any "scaffold from a directory"
/// use case).
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl AssetSource for DirSource {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        // Only a proven missing asset is absent: a dangling link is one
        // that cannot be read.
        let path = self.root.join(path);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && crate::absence::proven_absent(&path)? =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

/// Convenience: read an asset as UTF-8 text.
pub(crate) fn read_text(
    source: &dyn AssetSource,
    path: &str,
) -> Result<Option<String>, super::ScaffoldError> {
    source
        .read(path)
        .map_err(|error| super::ScaffoldError::InvalidState {
            what: format!("asset {path}"),
            detail: error.to_string(),
        })?
        .map(|bytes| {
            String::from_utf8(bytes).map_err(|_| super::ScaffoldError::InvalidState {
                what: format!("asset {path}"),
                detail: "not valid UTF-8".into(),
            })
        })
        .transpose()
}

#[cfg(test)]
mod r15_text_regressions {
    #[test]
    fn r15_invalid_asset_text_is_an_error_not_missing() {
        struct Bad;
        impl super::AssetSource for Bad {
            fn read(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
                Ok(Some(vec![0xff]))
            }
        }
        assert!(super::read_text(&Bad, "base/bad").is_err());
    }
}

#[cfg(test)]
mod r16_core_regressions {
    #[test]
    fn r16_asset_read_error_is_not_absence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("asset")).unwrap();
        let source = super::DirSource::new(root.path());
        assert!(super::read_text(&source, "asset").is_err());
    }

    /// A dangling link is an asset that cannot be read, never a missing one;
    /// an asset that is truly missing still reads as absent.
    #[cfg(unix)]
    #[test]
    fn r23_a_dangling_asset_link_is_not_absence() {
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(root.path().join("gone"), root.path().join("asset")).unwrap();
        let source = super::DirSource::new(root.path());
        assert!(super::read_text(&source, "asset").is_err());
        assert!(super::read_text(&source, "missing").unwrap().is_none());
    }
}
