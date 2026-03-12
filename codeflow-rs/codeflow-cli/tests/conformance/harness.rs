//! Conformance test harness for binary-level contract verification.
//!
//! This harness runs both the Go (`codeflow-cli/bin/codeflow`) and Rust
//! (`codeflow-rs/target/debug/codeflow`) binaries against identical stdin inputs
//! and diffs the results (stdout, stderr, exit code).
//!
//! # Binary Path Resolution
//!
//! Both binaries are resolved relative to the workspace root:
//! - Go:   `{workspace_root}/codeflow-cli/bin/codeflow` (built with `cd codeflow-cli && make build`)
//! - Rust: `{workspace_root}/codeflow-rs/target/debug/codeflow` (built with `cd codeflow-rs && cargo build`)
//!
//! # Graceful Degradation Contract
//!
//! Both binaries implement the graceful degradation contract:
//! - Empty or invalid stdin → exit 0 (never blocks on infrastructure errors)
//! - Handler errors → exit 0 (never blocks)
//! - Block output → exit 2 (stderr: reason message)
//! - Allow/Warn output → exit 0 (stderr: warning message for Warn)
//!
//! # Known Gaps
//!
//! `user-prompt-submit validate` is a stub in Rust (PromptValidateStub — always
//! returns Allow, produces no stderr output). Go's implementation outputs context
//! reminders to stderr. Tests for this handler are marked `#[ignore]` with a
//! TODO comment. See tests/conformance/hooks.rs for the ignored tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Output captured from one binary invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl BinaryOutput {
    /// Returns true if the process exited with code 0.
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    /// Returns true if the process exited with code 2 (hook block).
    pub fn blocked(&self) -> bool {
        self.exit_code == 2
    }
}

/// Resolved paths to both binaries.
pub struct BinaryPaths {
    pub go_binary: PathBuf,
    pub rust_binary: PathBuf,
}

/// Find the workspace root by walking up from this file's location.
///
/// The workspace root is the directory that contains both `codeflow-cli/`
/// and `codeflow-rs/` subdirectories.
pub fn workspace_root() -> PathBuf {
    // This test file is at: codeflow-rs/codeflow-cli/tests/conformance/mod.rs
    // Workspace root is 4 levels up.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR points to codeflow-rs/codeflow-cli
    // workspace root = codeflow-rs/codeflow-cli/../.. = workspace
    manifest_dir
        .parent() // codeflow-rs/
        .expect("codeflow-cli parent")
        .parent() // workspace root
        .expect("codeflow-rs parent (workspace root)")
        .to_path_buf()
}

/// Resolve paths to both binaries, failing with a clear message if either is missing.
///
/// # Panics
///
/// Panics with build instructions if either binary is missing.
pub fn resolve_binaries() -> BinaryPaths {
    let root = workspace_root();

    let go_binary = root.join("codeflow-cli").join("bin").join("codeflow");
    let rust_binary = root
        .join("codeflow-rs")
        .join("target")
        .join("debug")
        .join("codeflow");

    assert!(
        go_binary.exists(),
        "Go binary missing at {}: build with `cd codeflow-cli && make build`",
        go_binary.display()
    );
    assert!(
        rust_binary.exists(),
        "Rust binary missing at {}: build with `cd codeflow-rs && cargo build`",
        rust_binary.display()
    );

    BinaryPaths {
        go_binary,
        rust_binary,
    }
}

/// Run a single binary with the given CLI arguments and stdin input.
///
/// Returns `BinaryOutput` with stdout, stderr, and exit code.
pub fn run_binary(binary: &Path, args: &[&str], stdin: &str) -> BinaryOutput {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", binary.display()));

    if let Some(mut stdin_pipe) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin_pipe.write_all(stdin.as_bytes());
        // Drop closes the pipe, signaling EOF to the child.
    }

    let output = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("failed to wait on {}: {e}", binary.display()));

    BinaryOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    }
}

/// Run both binaries with the same args and stdin, returning `(go_output, rust_output)`.
pub fn run_conformance(args: &[&str], stdin: &str) -> (BinaryOutput, BinaryOutput) {
    let paths = resolve_binaries();
    let go_out = run_binary(&paths.go_binary, args, stdin);
    let rust_out = run_binary(&paths.rust_binary, args, stdin);
    (go_out, rust_out)
}

/// Assert that Go and Rust outputs match on exit code and stdout.
///
/// stderr is excluded from the primary assertion because:
/// - Logging handlers write timestamps to stderr (non-deterministic).
/// - Some handlers write environment-dependent paths.
/// - The known gap for user-prompt-submit validate produces intentional stderr diff.
///
/// Use `assert_conformance_strict` when stderr must also match.
pub fn assert_conformance(args: &[&str], stdin: &str) {
    let (go, rust) = run_conformance(args, stdin);

    assert_eq!(
        go.exit_code,
        rust.exit_code,
        "exit code mismatch for `{}`:\n  Go:   {}\n  Rust: {}\n  Go stderr:   {}\n  Rust stderr: {}",
        args.join(" "),
        go.exit_code,
        rust.exit_code,
        go.stderr.trim(),
        rust.stderr.trim()
    );

    assert_eq!(
        go.stdout,
        rust.stdout,
        "stdout mismatch for `{}`:\n  Go:   {:?}\n  Rust: {:?}",
        args.join(" "),
        go.stdout,
        rust.stdout
    );
}

/// Assert that Go and Rust outputs match on exit code, stdout, AND stderr.
///
/// Only use this for handlers where stderr is stable and deterministic
/// (e.g., empty, or a fixed message not containing timestamps/paths).
pub fn assert_conformance_strict(args: &[&str], stdin: &str) {
    let (go, rust) = run_conformance(args, stdin);

    assert_eq!(
        go.exit_code,
        rust.exit_code,
        "exit code mismatch for `{}`:\n  Go:   {}\n  Rust: {}",
        args.join(" "),
        go.exit_code,
        rust.exit_code
    );

    assert_eq!(
        go.stdout,
        rust.stdout,
        "stdout mismatch for `{}`:\n  Go:   {:?}\n  Rust: {:?}",
        args.join(" "),
        go.stdout,
        rust.stdout
    );

    assert_eq!(
        go.stderr,
        rust.stderr,
        "stderr mismatch for `{}`:\n  Go:   {:?}\n  Rust: {:?}",
        args.join(" "),
        go.stderr,
        rust.stderr
    );
}

/// Assert that both binaries exit with code 0 (allow).
pub fn assert_both_allow(args: &[&str], stdin: &str) {
    let (go, rust) = run_conformance(args, stdin);
    assert_eq!(
        go.exit_code,
        0,
        "Go should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        go.exit_code,
        go.stderr.trim()
    );
    assert_eq!(
        rust.exit_code,
        0,
        "Rust should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );
    // Also assert they match each other.
    assert_eq!(go.exit_code, rust.exit_code);
}

/// Assert that both binaries exit with code 2 (block).
#[allow(dead_code)]
pub fn assert_both_block(args: &[&str], stdin: &str) {
    let (go, rust) = run_conformance(args, stdin);
    assert_eq!(
        go.exit_code,
        2,
        "Go should exit 2 (block) for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        go.exit_code,
        go.stderr.trim()
    );
    assert_eq!(
        rust.exit_code,
        2,
        "Rust should exit 2 (block) for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );
}

/// Load a fixture file from `tests/fixtures/` by event name.
///
/// Panics if the fixture file cannot be read.
pub fn load_fixture(event_name: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent() // go up from codeflow-cli to codeflow-rs
        .expect("parent of codeflow-cli")
        .join("codeflow-cli")
        .join("tests")
        .join("fixtures")
        .join(format!("{event_name}.json"));

    std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("fixture {}: {e}", fixture_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_root_exists() {
        let root = workspace_root();
        assert!(
            root.exists(),
            "workspace root should exist: {}",
            root.display()
        );
    }

    #[test]
    fn test_workspace_root_contains_expected_directories() {
        let root = workspace_root();
        assert!(
            root.join("codeflow-cli").is_dir(),
            "workspace root should contain codeflow-cli/"
        );
        assert!(
            root.join("codeflow-rs").is_dir(),
            "workspace root should contain codeflow-rs/"
        );
    }

    #[test]
    fn test_load_fixture_pre_tool_use() {
        let fixture = load_fixture("pre_tool_use");
        assert!(
            fixture.contains("pre_tool_use"),
            "fixture should contain event type"
        );
        assert!(fixture.contains("Bash"), "fixture should contain tool name");
    }

    #[test]
    fn test_load_fixture_session_start() {
        let fixture = load_fixture("session_start");
        assert!(
            fixture.contains("session_start"),
            "fixture should contain event type"
        );
        assert!(fixture.contains("startup"), "fixture should contain source");
    }

    #[test]
    fn test_load_fixture_all_event_types() {
        let events = [
            "pre_tool_use",
            "post_tool_use",
            "task_completed",
            "session_start",
            "session_end",
            "stop",
            "user_prompt_submit",
        ];
        for event in &events {
            let fixture = load_fixture(event);
            assert!(
                !fixture.is_empty(),
                "fixture for {event} should not be empty"
            );
            assert!(
                fixture.contains(event),
                "fixture for {event} should contain the event name"
            );
        }
    }

    #[test]
    fn test_binary_output_succeeded() {
        let out = BinaryOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert!(out.succeeded());
        assert!(!out.blocked());
    }

    #[test]
    fn test_binary_output_blocked() {
        let out = BinaryOutput {
            stdout: String::new(),
            stderr: "block reason".to_string(),
            exit_code: 2,
        };
        assert!(!out.succeeded());
        assert!(out.blocked());
    }

    #[test]
    fn test_binary_output_general_error() {
        let out = BinaryOutput {
            stdout: String::new(),
            stderr: "error".to_string(),
            exit_code: 1,
        };
        assert!(!out.succeeded());
        assert!(!out.blocked());
    }

    #[test]
    fn test_resolve_binaries_go_exists() {
        let paths = resolve_binaries();
        assert!(paths.go_binary.exists(), "Go binary should exist");
    }

    #[test]
    fn test_resolve_binaries_rust_exists() {
        let paths = resolve_binaries();
        assert!(paths.rust_binary.exists(), "Rust binary should exist");
    }
}
