//! Temporary folders that the tests of one process share, removed when the
//! process exits (TSK-386).
//!
//! A `static` is never dropped, so a `tempfile::TempDir` kept in one stays on
//! disk after the process ends: every run of the suite left another shared
//! home in the user's temporary directory. A folder registered here is
//! removed when the process exits normally, which the test harness does
//! whether its tests passed or failed:
//!
//! - On Unix, the C runtime's `atexit` handler runs from `exit`, which both
//!   a return from `main` and `std::process::exit` reach.
//! - On Windows, `std::process::exit` calls `ExitProcess` directly, which
//!   skips the C runtime's `atexit` table, and the harness exits that way
//!   when a test fails. The loader calls the executable's TLS callbacks with
//!   `DLL_PROCESS_DETACH` inside `ExitProcess` on every exit path, so the
//!   folders are removed from one, as the standard library runs its own
//!   thread-local destructors.
//!
//! A process killed outright still leaves its folders.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, TryLockError};

static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Remove every registered folder. It never panics, because a panic cannot
/// cross the runtime that calls it, and never waits for the lock: at exit
/// the thread that held it may be gone.
extern "C" fn remove_folders() {
    let mut folders = match FOLDERS.try_lock() {
        Ok(folders) => folders,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    for folder in folders.drain(..) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[cfg(unix)]
fn remove_folders_at_exit() {
    extern "C" {
        fn atexit(callback: extern "C" fn()) -> std::os::raw::c_int;
    }
    // SAFETY: `remove_folders` takes no arguments, never unwinds, and reads
    // only this module's statics, which live for the whole process.
    let status = unsafe { atexit(remove_folders) };
    assert_eq!(status, 0, "register the exit handler");
}

#[cfg(windows)]
fn remove_folders_at_exit() {
    windows_exit::keep_linked();
}

#[cfg(windows)]
mod windows_exit {
    use std::ffi::c_void;

    const DLL_PROCESS_DETACH: u32 = 0;

    extern "C" {
        #[link_name = "_tls_used"]
        static TLS_USED: u8;
    }

    /// The loader calls every pointer in the `.CRT$XL*` sections, the
    /// executable's TLS callback array, at thread and process detach.
    #[used]
    #[link_section = ".CRT$XLB"]
    static ON_DETACH: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) = on_detach;

    unsafe extern "system" fn on_detach(_module: *mut c_void, reason: u32, _reserved: *mut c_void) {
        if reason == DLL_PROCESS_DETACH {
            super::remove_folders();
        }
    }

    /// Keep the TLS directory and this callback from being discarded by
    /// the linker, as the standard library does for its own callback.
    pub fn keep_linked() {
        // SAFETY: both are statics that live for the whole process; a
        // one-byte read of `_tls_used` and a read of the pointer change
        // nothing.
        unsafe {
            std::ptr::from_ref(&TLS_USED).read_volatile();
            std::ptr::from_ref(&ON_DETACH).read_volatile();
        }
    }
}

/// Keep `dir` until the process exits, then remove it.
pub fn until_exit(dir: tempfile::TempDir) -> PathBuf {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(remove_folders_at_exit);
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
