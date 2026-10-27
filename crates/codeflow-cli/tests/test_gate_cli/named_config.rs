//! `codeflow test --config <path>` (TSK-1020): the run reads the named file
//! and only that file, and refuses a path it cannot trust before any target
//! starts.

use std::path::Path;

use super::{repo, run, stderr};

/// The repository's own config: one target that fails if it ever runs.
const OWN: &str = r#"{"schema_version":"1.0","targets":[{"name":"own","runner":"custom","modes":{"quick":{"command":"exit 7"}}}]}"#;
/// The named config: one target that passes.
const NAMED: &str = r#"{"schema_version":"1.0","targets":[{"name":"named","runner":"custom","modes":{"quick":{"command":"echo named ran"}}}]}"#;

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn named_config(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("base-test-config.json");
    std::fs::write(&path, NAMED).unwrap();
    path
}

#[test]
fn the_named_config_is_the_only_config_read() {
    let dir = repo(OWN);
    let home = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let named = named_config(side.path());
    let config = named.to_str().unwrap();
    let out = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--strict", "--config", config],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(out.status.success(), "{text}");
    assert!(text.contains("target named: ok"), "{text}");
    assert!(!text.contains("target own"), "{text}");
    // Without the option the repository's own config still drives the run.
    let own = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--strict"],
    );
    let text = format!("{}{}", stdout(&own), stderr(&own));
    assert!(!own.status.success(), "{text}");
    assert!(text.contains("target own: FAILED"), "{text}");
    // With no repository config, and a stack detection would find, the named
    // config is still the only input.
    std::fs::remove_file(dir.path().join(".codeflow/test-config.json")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let out = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--strict", "--config", config],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(out.status.success(), "{text}");
    assert!(text.contains("target named: ok"), "{text}");
    assert!(!text.contains("cargo"), "{text}");
}

/// Runs `codeflow test --config <path>` and asserts it refused before any
/// target started, naming `needle`.
fn assert_refused(dir: &Path, home: &Path, path: &Path, needle: &str) {
    let out = run(
        dir,
        home,
        &[
            "test",
            "--mode",
            "quick",
            "--strict",
            "--config",
            path.to_str().unwrap(),
        ],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!out.status.success(), "{text}");
    assert!(!text.contains("target "), "a target ran: {text}");
    assert!(!text.contains("selected "), "a target was selected: {text}");
    assert!(text.contains(needle), "{needle}: {text}");
}

#[test]
fn a_named_path_it_cannot_trust_is_refused_before_any_target() {
    let dir = repo(OWN);
    let home = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    assert_refused(
        dir.path(),
        home.path(),
        &side.path().join("absent.json"),
        "not found",
    );
    assert_refused(dir.path(), home.path(), side.path(), "not a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let real = named_config(side.path());
        let link = side.path().join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_refused(dir.path(), home.path(), &link, "symbolic link");
        let dangling = side.path().join("dangling.json");
        std::os::unix::fs::symlink(side.path().join("gone.json"), &dangling).unwrap();
        assert_refused(dir.path(), home.path(), &dangling, "symbolic link");
        // Unreadable: skipped where the process can read anything (root).
        let locked = side.path().join("locked.json");
        std::fs::write(&locked, NAMED).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&locked).is_err() {
            assert_refused(dir.path(), home.path(), &locked, "cannot be read");
        }
    }
    let invalid = side.path().join("invalid.json");
    std::fs::write(&invalid, "{ not json").unwrap();
    assert_refused(dir.path(), home.path(), &invalid, "invalid test config");
}

#[test]
fn setup_refuses_a_named_config() {
    let dir = repo(OWN);
    let home = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let named = named_config(side.path());
    let out = run(
        dir.path(),
        home.path(),
        &[
            "test",
            "--config",
            named.to_str().unwrap(),
            "setup",
            "--list-templates",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains(
        "--config (setup writes .codeflow/test-config.json) cannot be combined with setup"
    ));
}
