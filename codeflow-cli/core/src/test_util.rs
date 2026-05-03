//! Test-only utilities shared across the crate.
//!
//! This module is gated on `#[cfg(test)]` so it does not contribute to
//! production binary size or surface. Callers reach in via
//! `crate::test_util::xdg_env_lock()`.
//!
//! ## Why a shared lock?
//!
//! Tests in `worktree::cleanup` and `autorun::rescue` both mutate the
//! process-global `HOME` and `XDG_CACHE_HOME` env vars to redirect
//! `dirs::cache_dir()` into a per-test tempdir. Each module previously
//! defined its OWN `Mutex` — meaning two tests in different modules
//! could enter the critical section concurrently, racing the env mutation
//! and producing intermittent failures (`test_rescue_bad_config_does_not_panic`
//! and friends would fail under parallel execution but pass in isolation).
//!
//! Consolidating into one mutex serialises ALL such tests across the
//! crate. Tests acquire the lock once at the top of the test body
//! (`let _g = test_util::xdg_env_lock();`) before calling
//! `redirect_xdg_cache`. The mutex tolerates poison: a previous test
//! panicking inside the critical section poisons the mutex, but the env
//! mutation is idempotent (each test overwrites with its own tempdir
//! before reading), so we just want serialisation, not poison
//! propagation.
//!
//! INF-TSK-024-051 Phase 2.5: addresses the parallel-test race that
//! Phase 1 surfaced in `cargo test -p codeflow-core --lib` (works under
//! `--test-threads=1`, intermittent failures otherwise).
//!
//! ## Why both Mutex AND `#[serial_test::serial(env_vars)]`?
//!
//! The `serial_test` crate already serialises tests with the
//! `env_vars` key in `crate::session::liveness::tests`. Tests that
//! mutate XDG env vars MUST use the SAME `env_vars` key so they
//! serialise against the existing liveness tests too — otherwise a
//! `liveness` test could mutate `CODEFLOW_WORKTREE_PATH` while an XDG
//! test holds `xdg_env_lock`, racing on the broader env-var space.
//!
//! Convention going forward:
//!   - Add `#[serial_test::serial(env_vars)]` to any test that mutates
//!     ANY process-global env var (HOME, XDG_*, CODEFLOW_*).
//!   - Acquire `xdg_env_lock()` ALSO when mutating HOME/XDG specifically
//!     for filesystem redirection — defence-in-depth against env-mutation
//!     ordering bugs that the `serial_test` macro alone cannot detect
//!     (it serialises tests at the macro level but doesn't lock helper
//!     functions that read env between calls).

use std::sync::Mutex;

/// Process-global mutex serialising tests that mutate `HOME` /
/// `XDG_CACHE_HOME`.
///
/// Defined here (not in any specific module's tests) so the same mutex
/// is observed by every caller crate-wide. Without consolidation, each
/// module had its own `Mutex<()>` and they raced against each other.
static XDG_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the shared XDG env-var lock for the duration of a test.
///
/// Returns a `MutexGuard` that releases on drop. Tolerates a poisoned
/// mutex (a previous test panicking inside the critical section)
/// because the env mutation is idempotent — every caller overwrites
/// `HOME` / `XDG_CACHE_HOME` with its own tempdir before reading, so
/// stale values from a poisoned guard cannot leak.
///
/// # Usage
///
/// ```ignore
/// #[test]
/// fn my_test() {
///     let _g = crate::test_util::xdg_env_lock();
///     let td = tempfile::tempdir().unwrap();
///     // SAFETY: serialised via XDG_ENV_LOCK
///     unsafe {
///         std::env::set_var("HOME", td.path());
///         std::env::set_var("XDG_CACHE_HOME", td.path().join(".cache"));
///     }
///     // ... rest of test
/// }
/// ```
pub fn xdg_env_lock() -> std::sync::MutexGuard<'static, ()> {
    XDG_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_can_be_acquired() {
        let _g = xdg_env_lock();
        // Holding the lock; nothing to assert beyond not panicking.
    }

    #[test]
    fn lock_is_reentrant_across_calls_after_drop() {
        {
            let _g = xdg_env_lock();
        }
        let _g = xdg_env_lock();
    }
}
