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
//! # Non-Deterministic Outputs
//!
//! Some handlers produce non-deterministic output (e.g., `session-start init`
//! generates a unique session ID per invocation). For these, use
//! `assert_conformance_json_env` which compares JSON structure and static values
//! while allowing dynamic values to differ.

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

/// Create an isolated project directory with `.state/` subdirectory tree.
///
/// Returns a `TempDir` whose path can be passed as `CF_PROJECT_ROOT`.
/// The caller must keep the `TempDir` alive (not drop it) until assertions
/// are complete -- dropping it deletes the directory.
pub fn make_isolated_project_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp dir for test isolation");
    let state = tmp.path().join(".state");
    for subdir in &[
        "ledger",
        "db",
        "runtime",
        "logs/sessions",
        "session",
        "sentinels/pathflow",
    ] {
        std::fs::create_dir_all(state.join(subdir))
            .unwrap_or_else(|e| panic!("failed to create {subdir}: {e}"));
    }
    tmp
}

/// Run a single binary with isolation: sets `CF_PROJECT_ROOT` and
/// `GIT_CEILING_DIRECTORIES` to the given project dir, and removes
/// `CODEFLOW_SESSION_ID` to prevent env leakage.
pub fn run_binary_isolated(
    binary: &Path,
    args: &[&str],
    stdin: &str,
    project_dir: &Path,
) -> BinaryOutput {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CF_PROJECT_ROOT", project_dir)
        .env(
            "GIT_CEILING_DIRECTORIES",
            project_dir.parent().unwrap_or(project_dir),
        )
        .env_remove("CODEFLOW_SESSION_ID")
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

/// Run both binaries in isolated temp directories with the same args and stdin.
///
/// Returns `(go_output, rust_output, temp_dir)`. The caller must keep
/// `temp_dir` alive until all assertions are complete.
pub fn run_conformance_isolated(
    args: &[&str],
    stdin: &str,
) -> (BinaryOutput, BinaryOutput, tempfile::TempDir) {
    let tmp = make_isolated_project_dir();
    let paths = resolve_binaries();
    let go_out = run_binary_isolated(&paths.go_binary, args, stdin, tmp.path());
    let rust_out = run_binary_isolated(&paths.rust_binary, args, stdin, tmp.path());
    (go_out, rust_out, tmp)
}

/// Run both binaries with the same args and stdin, returning `(go_output, rust_output)`.
///
/// Uses test isolation internally: each invocation gets a fresh temp directory
/// with `CF_PROJECT_ROOT` set, preventing writes to production state files.
pub fn run_conformance(args: &[&str], stdin: &str) -> (BinaryOutput, BinaryOutput) {
    let (go, rust, _tmp) = run_conformance_isolated(args, stdin);
    (go, rust)
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

/// Assert structural conformance for JSON env output (e.g., session-start init).
///
/// Both binaries produce `{"env":{"KEY":"value",...}}` but some values are
/// non-deterministic (e.g., `CODEFLOW_SESSION_ID` contains a ULID).
///
/// This asserts:
/// - Both exit with code 0
/// - Both produce valid JSON with an `env` object
/// - Both have the same set of keys in `env`
/// - All values match EXCEPT keys listed in `dynamic_keys`, which are checked
///   only for non-emptiness (both must have non-empty values)
#[allow(dead_code)]
pub fn assert_conformance_json_env(args: &[&str], stdin: &str, dynamic_keys: &[&str]) {
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

    let go_json: serde_json::Value = serde_json::from_str(go.stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "Go stdout is not valid JSON: {e}\n  stdout: {:?}",
            go.stdout
        )
    });
    let rust_json: serde_json::Value =
        serde_json::from_str(rust.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "Rust stdout is not valid JSON: {e}\n  stdout: {:?}",
                rust.stdout
            )
        });

    let go_env = go_json
        .get("env")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("Go JSON missing 'env' object: {go_json:?}"));
    let rust_env = rust_json
        .get("env")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("Rust JSON missing 'env' object: {rust_json:?}"));

    let go_keys: std::collections::BTreeSet<&String> = go_env.keys().collect();
    let rust_keys: std::collections::BTreeSet<&String> = rust_env.keys().collect();
    assert_eq!(
        go_keys, rust_keys,
        "env keys differ:\n  Go:   {go_keys:?}\n  Rust: {rust_keys:?}"
    );

    for key in &go_keys {
        let go_val = go_env[*key].as_str().unwrap_or("");
        let rust_val = rust_env[*key].as_str().unwrap_or("");

        if dynamic_keys.contains(&key.as_str()) {
            assert!(!go_val.is_empty(), "Go env[{key}] should not be empty");
            assert!(!rust_val.is_empty(), "Rust env[{key}] should not be empty");
        } else {
            assert_eq!(
                go_val, rust_val,
                "env[{key}] mismatch:\n  Go:   {go_val:?}\n  Rust: {rust_val:?}"
            );
        }
    }
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

/// Load a fixture and replace its `project_dir` field with the given path.
///
/// This enables dynamic injection of isolated temp directories into fixture
/// JSON, so the binary under test writes state to the temp dir instead of
/// the real project.
pub fn load_fixture_with_project_dir(event_name: &str, project_dir: &Path) -> String {
    let template = load_fixture(event_name);
    let mut json: serde_json::Value = serde_json::from_str(&template)
        .unwrap_or_else(|e| panic!("fixture {event_name} is not valid JSON: {e}"));
    json["project_dir"] = serde_json::Value::String(project_dir.to_string_lossy().into_owned());
    serde_json::to_string(&json).unwrap()
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

    #[test]
    fn test_make_isolated_project_dir_creates_state_structure() {
        let tmp = make_isolated_project_dir();
        let state = tmp.path().join(".state");
        assert!(state.join("ledger").is_dir(), "ledger dir should exist");
        assert!(state.join("db").is_dir(), "db dir should exist");
        assert!(state.join("runtime").is_dir(), "runtime dir should exist");
        assert!(
            state.join("logs/sessions").is_dir(),
            "logs/sessions dir should exist"
        );
        assert!(state.join("session").is_dir(), "session dir should exist");
        assert!(
            state.join("sentinels/pathflow").is_dir(),
            "sentinels/pathflow dir should exist"
        );
    }

    #[test]
    fn test_load_fixture_with_project_dir_replaces_path() {
        let tmp = make_isolated_project_dir();
        let fixture = load_fixture_with_project_dir("session_start", tmp.path());
        let json: serde_json::Value = serde_json::from_str(&fixture).unwrap();
        assert_eq!(
            json["project_dir"].as_str().unwrap(),
            tmp.path().to_string_lossy().as_ref(),
            "project_dir should be replaced with temp dir path"
        );
    }

    #[test]
    fn test_load_fixture_with_project_dir_preserves_other_fields() {
        let tmp = make_isolated_project_dir();
        let fixture = load_fixture_with_project_dir("session_start", tmp.path());
        let json: serde_json::Value = serde_json::from_str(&fixture).unwrap();
        assert_eq!(
            json["event"].as_str().unwrap(),
            "session_start",
            "event field should be preserved"
        );
        assert_eq!(
            json["source"].as_str().unwrap(),
            "startup",
            "source field should be preserved"
        );
    }
}
