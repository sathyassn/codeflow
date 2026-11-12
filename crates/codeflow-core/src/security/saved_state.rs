//! The saved shell state a harness or terminal tool runs before its later
//! commands (TSK-1461). This commit holds the tests and the interface only;
//! the inventory and the pass follow.
#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};

use super::{Class, StartupEnv};
use crate::hooks::Violation;

#[cfg(test)]
thread_local! {
    static OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the saved-state inventory and pass run.
#[cfg(not(test))]
pub(crate) fn enabled() -> bool {
    true
}

/// Whether the saved-state inventory and pass run.
#[cfg(test)]
pub(crate) fn enabled() -> bool {
    !OFF.with(std::cell::Cell::get)
}

/// Run `f` with the saved-state inventory and pass turned off.
#[cfg(test)]
pub(crate) fn without<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            OFF.with(|off| off.set(self.0));
        }
    }
    let _restore = Restore(OFF.with(|off| off.replace(true)));
    f()
}

/// Whether `reading` has the folder names of `name` as consecutive
/// components.
pub(crate) fn dir_named(reading: &str, name: &str) -> bool {
    let want: Vec<&str> = name.split('/').collect();
    let parts: Vec<&str> = reading.split('/').collect();
    parts
        .windows(want.len())
        .any(|window| window == want.as_slice())
}

/// Add the saved shell state to the startup class.
pub(super) fn extend_class(_class: &mut Class, _env: &StartupEnv) {}

/// Judge `command` for removals, moves, copies and extractions over saved
/// shell state.
#[must_use]
pub(crate) fn evaluate(_command: &str, _cwd: &Path, _env: &StartupEnv) -> Vec<Violation> {
    Vec::new()
}

#[cfg(test)]
#[path = "saved_state_tests.rs"]
mod tests;
