//! Exit code constants and error types for the CLI binary.
//!
//! Maps Go's `cliutil.Exit*` constants to Rust process exit codes.
//! All constants are public API; some are used only as more commands
//! are implemented, so `dead_code` is expected during incremental build-out.

use std::process;

/// Successful execution (hook: allow).
pub const EXIT_SUCCESS: i32 = 0;

/// General runtime error.
#[allow(dead_code)]
pub const EXIT_GENERAL_ERROR: i32 = 1;

/// Configuration or setup error. Also used for hook block (exit 2).
#[allow(dead_code)]
pub const EXIT_CONFIG_ERROR: i32 = 2;

/// Runtime failure during command execution.
#[allow(dead_code)]
pub const EXIT_RUNTIME_ERROR: i32 = 3;

/// Failure in an external dependency.
#[allow(dead_code)]
pub const EXIT_EXTERNAL_ERROR: i32 = 4;

/// Internal/unexpected error.
#[allow(dead_code)]
pub const EXIT_INTERNAL_ERROR: i32 = 5;

/// Hook block exit code (alias for `EXIT_CONFIG_ERROR`).
pub const EXIT_HOOK_BLOCK: i32 = 2;

/// An error that carries a specific exit code.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ExitError {
    pub code: i32,
    pub message: String,
}

impl ExitError {
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Exit the process with this error's code, printing the message to stderr.
    #[allow(dead_code)]
    pub fn exit(&self) -> ! {
        if !self.message.is_empty() {
            eprintln!("{}", self.message);
        }
        process::exit(self.code)
    }
}

impl std::fmt::Display for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ExitError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_values_match_go() {
        assert_eq!(EXIT_SUCCESS, 0);
        assert_eq!(EXIT_GENERAL_ERROR, 1);
        assert_eq!(EXIT_CONFIG_ERROR, 2);
        assert_eq!(EXIT_RUNTIME_ERROR, 3);
        assert_eq!(EXIT_EXTERNAL_ERROR, 4);
        assert_eq!(EXIT_INTERNAL_ERROR, 5);
    }

    #[test]
    fn test_exit_hook_block_equals_config_error() {
        assert_eq!(EXIT_HOOK_BLOCK, EXIT_CONFIG_ERROR);
        assert_eq!(EXIT_HOOK_BLOCK, 2);
    }

    #[test]
    fn test_exit_error_new() {
        let err = ExitError::new(EXIT_GENERAL_ERROR, "something failed");
        assert_eq!(err.code, 1);
        assert_eq!(err.message, "something failed");
    }

    #[test]
    fn test_exit_error_display() {
        let err = ExitError::new(EXIT_CONFIG_ERROR, "bad config");
        assert_eq!(err.to_string(), "bad config");
    }

    #[test]
    fn test_exit_error_empty_message() {
        let err = ExitError::new(EXIT_SUCCESS, "");
        assert_eq!(err.code, 0);
        assert!(err.message.is_empty());
    }

    #[test]
    fn test_exit_error_is_std_error() {
        let err = ExitError::new(EXIT_GENERAL_ERROR, "test");
        let _: &dyn std::error::Error = &err;
    }
}
