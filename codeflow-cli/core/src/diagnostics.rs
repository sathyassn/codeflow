//! Shared warning and diagnostic utilities.
//!
//! Provides consistent stderr warning emission across all codeflow-core
//! components. All warning output follows the format:
//!   `[{component}] WARNING: {message}`

use std::path::Path;

/// Emit a warning to stderr with a component tag.
///
/// Format: `[{component}] WARNING: {message}`
pub fn warn(component: &str, message: &str) {
    eprintln!("[{component}] WARNING: {message}");
}

/// Emit a warning to stderr that includes a filesystem path.
///
/// Format: `[{component}] WARNING: {message}: {path}`
pub fn warn_with_path(component: &str, message: &str, path: &Path) {
    eprintln!("[{component}] WARNING: {message}: {}", path.display());
}

/// Emit a warning about a fallback from one mechanism to another.
///
/// Format: `[{component}] WARNING: {from} unavailable, falling back to {to}`
pub fn warn_fallback(component: &str, from: &str, to: &str) {
    eprintln!("[{component}] WARNING: {from} unavailable, falling back to {to}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_warn_does_not_panic() {
        warn("test-component", "something happened");
    }

    #[test]
    fn test_warn_with_path_does_not_panic() {
        let path = PathBuf::from("/tmp/test/file.txt");
        warn_with_path("test-component", "file missing", &path);
    }

    #[test]
    fn test_warn_fallback_does_not_panic() {
        warn_fallback(
            "test-component",
            "CODEFLOW_WORKTREE_PATH",
            "CF_PROJECT_ROOT",
        );
    }
}
