//! End-to-end coverage for the public `codeflow test setup` options.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .output()
        .expect("codeflow runs")
}

fn codeflow_with_input(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codeflow spawns");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn config_path(dir: &Path) -> std::path::PathBuf {
    dir.join(".codeflow/test-config.json")
}

#[cfg(unix)]
#[test]
fn test_command_does_not_follow_a_committed_schema_temporary_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();
    std::fs::create_dir(root.path().join(".codeflow")).unwrap();
    let planted = root.path().join(".codeflow/test-config.schema.tmp");
    std::os::unix::fs::symlink(&sentinel, &planted).unwrap();
    std::fs::write(
        config_path(root.path()),
        r#"{
        "schema_version":"1.0",
        "targets":[{"name":"smoke","runner":"custom","modes":{
            "quick":{"command":"printf safe"},
            "essential":{"command":"printf safe"},
            "full":{"command":"printf safe"}
        }}]
    }"#,
    )
    .unwrap();
    // Each Git command uses only this disposable repository's configuration.
    for args in [
        vec!["init", "-q"],
        vec!["add", ".codeflow"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(root.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["test", "--mode", "quick"])
        .current_dir(root.path())
        .env("CODEFLOW_HOME", isolated_home())
        .env("CARGO_TARGET_DIR", root.path().join("target"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
    // The test runner consumes a populated config without writing the schema.
    // Setup exercises the schema writer with the same committed link present.
    let setup = codeflow(
        root.path(),
        &["test", "setup", "--template", "minimal.json", "--replace"],
    );
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
    assert!(std::fs::symlink_metadata(planted).unwrap().is_symlink());
    assert!(root
        .path()
        .join(".codeflow/test-config.schema.json")
        .is_file());
}

#[test]
fn list_uses_templates_embedded_in_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let output = codeflow(dir.path(), &["test", "setup", "--list-templates"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in [
        "minimal.json",
        "example-rust.json",
        "monorepo-multi-target.json",
    ] {
        assert!(stdout.contains(name), "missing {name}: {stdout}");
    }
}

#[test]
fn setup_actions_conflict_and_replace_requires_template() {
    let dir = tempfile::tempdir().unwrap();
    let conflict = codeflow(
        dir.path(),
        &[
            "test",
            "setup",
            "--list-templates",
            "--template",
            "minimal.json",
        ],
    );
    assert_eq!(conflict.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("cannot be used with"));

    let orphan_replace = codeflow(dir.path(), &["test", "setup", "--replace"]);
    assert_eq!(orphan_replace.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&orphan_replace.stderr);
    assert!(stderr.contains("required arguments") && stderr.contains("--template"));
}

#[test]
fn embedded_template_is_applied_and_requires_explicit_replace() {
    let dir = tempfile::tempdir().unwrap();
    let initial = codeflow(
        dir.path(),
        &["test", "setup", "--template", "monorepo-multi-target.json"],
    );
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(config_path(dir.path())).unwrap()).unwrap();
    assert_eq!(parsed["targets"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["targets"][0]["cwd"], "services/api");

    let before = std::fs::read(config_path(dir.path())).unwrap();
    let rejected = codeflow(dir.path(), &["test", "setup", "--template", "minimal.json"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--replace"));
    assert_eq!(std::fs::read(config_path(dir.path())).unwrap(), before);

    let replaced = codeflow(
        dir.path(),
        &["test", "setup", "--template", "minimal.json", "--replace"],
    );
    assert!(replaced.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(config_path(dir.path())).unwrap()).unwrap();
    assert!(parsed["targets"].as_array().unwrap().is_empty());
}

#[test]
fn safe_auto_setup_preserves_malformed_and_populated_configs() {
    let malformed_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(malformed_dir.path().join(".codeflow")).unwrap();
    let malformed = b"{ populated but malformed\n";
    std::fs::write(config_path(malformed_dir.path()), malformed).unwrap();
    let output = codeflow(malformed_dir.path(), &["test", "setup"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid test config"));
    assert_eq!(
        std::fs::read(config_path(malformed_dir.path())).unwrap(),
        malformed
    );

    let populated_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(populated_dir.path().join(".codeflow")).unwrap();
    let populated = br#"{"schema_version":"1.0","targets":[{"name":"kept","runner":"custom","modes":{"full":{"command":"true"}}}]}"#;
    std::fs::write(config_path(populated_dir.path()), populated).unwrap();
    let output = codeflow(populated_dir.path(), &["test", "setup"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("leaving it unchanged"));
    assert_eq!(
        std::fs::read(config_path(populated_dir.path())).unwrap(),
        populated
    );
}

#[test]
fn add_target_appends_through_the_existing_wizard() {
    let dir = tempfile::tempdir().unwrap();
    let initial = codeflow(dir.path(), &["test", "setup", "--template", "minimal.json"]);
    assert!(initial.status.success());

    let output = codeflow_with_input(
        dir.path(),
        &["test", "setup", "--add-target"],
        "api\ncustom\nservices/api\ntrue\ntrue\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(config_path(dir.path())).unwrap()).unwrap();
    assert_eq!(parsed["targets"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["targets"][0]["name"], "api");
    assert_eq!(parsed["targets"][0]["cwd"], "services/api");
}

#[test]
fn unsafe_or_unknown_template_name_cannot_read_arbitrary_files() {
    let dir = tempfile::tempdir().unwrap();
    let output = codeflow(
        dir.path(),
        &["test", "setup", "--template", "../Cargo.toml"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(!config_path(dir.path()).exists());
}
