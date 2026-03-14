//! Conformance test harness for binary-level contract verification.
//!
//! Post-cutover: This harness now tests the Rust binary only (Go has been removed).
//! The harness validates that the Rust binary at `codeflow-cli/target/debug/codeflow`
//! conforms to the hook handler contract.
//!
//! # Binary Path Resolution
//!
//! The binary is resolved relative to the workspace root:
//! - Rust: `{workspace_root}/codeflow-cli/target/debug/codeflow` (built with `cd codeflow-cli && cargo build`)
//!
//! # Graceful Degradation Contract
//!
//! The binary implements the graceful degradation contract:
//! - Empty or invalid stdin → exit 0 (never blocks on infrastructure errors)
//! - Handler errors → exit 0 (never blocks)
//! - Block output → exit 2 (stderr: reason message)
//! - Allow/Warn output → exit 0 (stderr: warning message for Warn)
//!
//! # Non-Deterministic Outputs
//!
//! Some handlers produce non-deterministic output (e.g., `session-start init`
//! generates a unique session ID per invocation). For these, use
//! `assert_rust_json_env` which checks JSON structure and static values
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

/// Resolved path to the Rust binary.
pub struct BinaryPaths {
    pub rust_binary: PathBuf,
}

/// Find the workspace root by walking up from this file's location.
///
/// The workspace root is the directory that contains `codeflow-cli/`
/// (the Rust workspace).
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR points to codeflow-cli/cli
    // workspace root = codeflow-cli/cli/../.. = project root
    manifest_dir
        .parent() // codeflow-cli/
        .expect("cli parent")
        .parent() // project root
        .expect("codeflow-cli parent (project root)")
        .to_path_buf()
}

/// Resolve path to the Rust binary, failing with a clear message if missing.
///
/// # Panics
///
/// Panics with build instructions if the binary is missing.
pub fn resolve_binaries() -> BinaryPaths {
    let root = workspace_root();

    let rust_binary = root
        .join("codeflow-cli")
        .join("target")
        .join("debug")
        .join("codeflow");

    assert!(
        rust_binary.exists(),
        "Rust binary missing at {}: build with `cd codeflow-cli && cargo build`",
        rust_binary.display()
    );

    BinaryPaths { rust_binary }
}

/// Run a single binary with the given CLI arguments and stdin input.
///
/// **Always runs in isolation:** sets `CF_PROJECT_ROOT` to a temporary directory
/// with `.state/` subdirectories, `GIT_CEILING_DIRECTORIES` to prevent git from
/// walking up to the real repo, and removes `CODEFLOW_SESSION_ID` to prevent env
/// leakage. This ensures conformance tests NEVER pollute production state files.
///
/// Returns `BinaryOutput` with stdout, stderr, and exit code.
pub fn run_binary(binary: &Path, args: &[&str], stdin: &str) -> BinaryOutput {
    let tmp = make_isolated_project_dir();
    run_binary_isolated(binary, args, stdin, tmp.path())
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

    // Seed a minimal codeflow-env.sh with a dummy session ID.
    // Handlers like session-start init with source=compact/resume expect an
    // existing session. Without this file, they error (Go exits 1) while
    // the Rust stub always exits 0, causing false conformance mismatches.
    let env_file = state.join("runtime").join("codeflow-env.sh");
    std::fs::write(
        &env_file,
        "export CODEFLOW_SESSION_ID='ses-test-isolation-00000000000'\n",
    )
    .unwrap_or_else(|e| panic!("failed to write codeflow-env.sh: {e}"));

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

/// Run the Rust binary in an isolated temp directory.
///
/// Returns `(rust_output, temp_dir)`. The caller must keep
/// `temp_dir` alive until all assertions are complete.
#[allow(dead_code)]
pub fn run_rust_isolated(args: &[&str], stdin: &str) -> (BinaryOutput, tempfile::TempDir) {
    let tmp = make_isolated_project_dir();
    let paths = resolve_binaries();
    let rust_out = run_binary_isolated(&paths.rust_binary, args, stdin, tmp.path());
    (rust_out, tmp)
}

/// Run the Rust binary with the given args and stdin.
///
/// The binary invocation is automatically isolated (see `run_binary`).
pub fn run_rust(args: &[&str], stdin: &str) -> BinaryOutput {
    let paths = resolve_binaries();
    run_binary(&paths.rust_binary, args, stdin)
}

/// Assert that the Rust binary exits with code 0 (allow).
pub fn assert_rust_allow(args: &[&str], stdin: &str) {
    let rust = run_rust(args, stdin);
    assert_eq!(
        rust.exit_code,
        0,
        "Rust should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );
}

/// Assert that the Rust binary exits with code 2 (block).
#[allow(dead_code)]
pub fn assert_rust_block(args: &[&str], stdin: &str) {
    let rust = run_rust(args, stdin);
    assert_eq!(
        rust.exit_code,
        2,
        "Rust should exit 2 (block) for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );
}

/// Assert Rust binary produces valid JSON env output with expected structure.
///
/// Checks:
/// - Exit code 0
/// - Valid JSON with an `env` object
/// - Dynamic keys have non-empty values
#[allow(dead_code)]
pub fn assert_rust_json_env(args: &[&str], stdin: &str, dynamic_keys: &[&str]) {
    let rust = run_rust(args, stdin);

    assert_eq!(
        rust.exit_code,
        0,
        "Rust should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );

    let rust_json: serde_json::Value =
        serde_json::from_str(rust.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "Rust stdout is not valid JSON: {e}\n  stdout: {:?}",
                rust.stdout
            )
        });

    let rust_env = rust_json
        .get("env")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("Rust JSON missing 'env' object: {rust_json:?}"));

    for key in dynamic_keys {
        let val = rust_env.get(*key).and_then(|v| v.as_str()).unwrap_or("");
        assert!(!val.is_empty(), "Rust env[{key}] should not be empty");
    }
}

/// Run the Rust binary with a fixture, using an isolated project dir.
///
/// Loads the fixture and replaces its `project_dir` with a temp dir.
///
/// Returns `BinaryOutput`. The temp dir lives until the return value is consumed.
pub fn run_rust_with_fixture(args: &[&str], event_name: &str) -> BinaryOutput {
    let tmp = make_isolated_project_dir();
    let fixture = load_fixture_with_project_dir(event_name, tmp.path());
    let paths = resolve_binaries();
    run_binary_isolated(&paths.rust_binary, args, &fixture, tmp.path())
}

/// Assert that the Rust binary exits with code 0 for a fixture.
pub fn assert_rust_allow_fixture(args: &[&str], event_name: &str) {
    let rust = run_rust_with_fixture(args, event_name);
    assert_eq!(
        rust.exit_code,
        0,
        "Rust should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );
}

/// Assert Rust binary produces valid JSON env output from fixture.
#[allow(dead_code)]
pub fn assert_rust_json_env_fixture(args: &[&str], event_name: &str, dynamic_keys: &[&str]) {
    let rust = run_rust_with_fixture(args, event_name);

    assert_eq!(
        rust.exit_code,
        0,
        "Rust should exit 0 for `{}`, got {}\n  stderr: {}",
        args.join(" "),
        rust.exit_code,
        rust.stderr.trim()
    );

    let rust_json: serde_json::Value =
        serde_json::from_str(rust.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "Rust stdout is not valid JSON: {e}\n  stdout: {:?}",
                rust.stdout
            )
        });

    let rust_env = rust_json
        .get("env")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("Rust JSON missing 'env' object: {rust_json:?}"));

    for key in dynamic_keys {
        let val = rust_env.get(*key).and_then(|v| v.as_str()).unwrap_or("");
        assert!(!val.is_empty(), "Rust env[{key}] should not be empty");
    }
}

/// Load a fixture file from `tests/fixtures/` by event name.
///
/// Panics if the fixture file cannot be read.
pub fn load_fixture(event_name: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent() // go up from cli to codeflow-cli
        .expect("parent of cli")
        .join("cli")
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
