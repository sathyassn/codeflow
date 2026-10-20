//! Temporary folders that the tests of one process share, removed when the
//! process exits (TSK-386).
//!
//! A `static` is never dropped, so a `tempfile::TempDir` kept in one stays on
//! disk after the process ends: every run of the suite left another shared
//! home in the user's temporary directory. A folder registered here is
//! removed by a handler that the C runtime calls when the process exits
//! normally, which the test harness does whether its tests passed or failed.
//! A process killed by a signal still leaves its folders.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

extern "C" {
    fn atexit(callback: extern "C" fn()) -> std::os::raw::c_int;
}

/// Remove every registered folder. It never panics, because a panic cannot
/// cross the C runtime that calls it.
extern "C" fn remove_folders() {
    let mut folders = match FOLDERS.lock() {
        Ok(folders) => folders,
        Err(poisoned) => poisoned.into_inner(),
    };
    for folder in folders.drain(..) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// Keep `dir` until the process exits, then remove it.
pub fn until_exit(dir: tempfile::TempDir) -> PathBuf {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        // SAFETY: `remove_folders` takes no arguments, never unwinds, and
        // reads only this module's statics, which live for the whole process.
        let status = unsafe { atexit(remove_folders) };
        assert_eq!(status, 0, "register the exit handler");
    });
    let path = dir.keep();
    FOLDERS.lock().unwrap().push(path.clone());
    path
}

/// The `CODEFLOW_HOME` the tests of this process share, so the registry
/// touch of every command stays out of the developer's real home.
pub fn isolated_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| until_exit(tempfile::tempdir().expect("home tempdir")))
}
