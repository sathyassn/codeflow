//! The portal's transaction I/O: the shared contained tree
//! ([`crate::contained`]) with errors in the scaffold's terms.

use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io;
use std::ops::Deref;
use std::path::Path;

use crate::contained::Tree;
use crate::scaffold::ScaffoldError;

pub(super) struct PortalIo(Tree);

impl Deref for PortalIo {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl AsRef<Path> for PortalIo {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}

impl PortalIo {
    pub(super) fn open(root: &Path) -> Result<Self, ScaffoldError> {
        Tree::open(root)
            .map(Self)
            .map_err(|e| ScaffoldError::io(root, e))
    }

    fn fail(&self, relative: &str) -> impl FnOnce(io::Error) -> ScaffoldError + '_ {
        let path = self.0.path().join(relative);
        move |e| ScaffoldError::io(path, e)
    }

    pub(super) fn read(&self, relative: &str, maximum: u64) -> io::Result<Vec<u8>> {
        self.0.read(relative, maximum)
    }

    pub(super) fn metadata(&self, relative: &str) -> io::Result<Metadata> {
        self.0.metadata(relative)
    }

    pub(super) fn inspect(&self, relative: &str) -> Result<Option<Metadata>, ScaffoldError> {
        match self.0.metadata(relative) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(self.fail(relative)(error)),
        }
    }

    pub(super) fn write(&self, relative: &str, bytes: &[u8]) -> Result<(), ScaffoldError> {
        self.0.write(relative, bytes).map_err(self.fail(relative))
    }

    pub(super) fn remove(&self, relative: &str) -> Result<(), ScaffoldError> {
        self.0.remove(relative).map_err(self.fail(relative))
    }

    pub(super) fn remove_empty(&self, relative: &str) -> io::Result<()> {
        self.0.remove_empty(relative)
    }

    pub(super) fn mkdir(&self, relative: &str) -> Result<(), ScaffoldError> {
        self.0.mkdir(relative).map_err(self.fail(relative))
    }

    pub(super) fn rename(&self, from: &str, to: &str) -> Result<(), ScaffoldError> {
        self.0.rename(from, to).map_err(self.fail(from))
    }

    pub(super) fn entries(&self, relative: &str, maximum: usize) -> io::Result<Vec<OsString>> {
        self.0.entries(relative, maximum)
    }

    pub(super) fn lock_file(&self, relative: &str) -> io::Result<File> {
        self.0.lock_file(relative)
    }
}
