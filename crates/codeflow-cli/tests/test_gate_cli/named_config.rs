//! `codeflow test --config <path>` (TSK-1020): the run reads the named file
//! and only that file, and refuses a path it cannot trust before any target
//! starts.

use std::path::Path;

use super::{repo, run, stderr};

/// The repository's own config: one target that leaves `own-ran.txt` and
/// fails, so a run that falls back to it is seen on disk.
const OWN: &str = r#"{"schema_version":"1.0","targets":[{"name":"own","runner":"custom","modes":{"quick":{"command":"echo own> own-ran.txt && exit 7"}}}]}"#;
/// The named config: one target that leaves `named-ran.txt` and passes.
const NAMED: &str = r#"{"schema_version":"1.0","targets":[{"name":"named","runner":"custom","modes":{"quick":{"command":"echo named> named-ran.txt"}}}]}"#;

/// No target of either config ran: neither sentinel exists.
fn assert_no_target_ran(dir: &Path, text: &str) {
    for sentinel in ["own-ran.txt", "named-ran.txt"] {
        assert!(
            !dir.join(sentinel).exists(),
            "{sentinel} exists: a target ran: {text}"
        );
    }
}

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
    assert!(dir.path().join("named-ran.txt").exists(), "{text}");
    assert!(!dir.path().join("own-ran.txt").exists(), "{text}");
    std::fs::remove_file(dir.path().join("named-ran.txt")).unwrap();
    // Without the option the repository's own config still drives the run.
    let own = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--strict"],
    );
    let text = format!("{}{}", stdout(&own), stderr(&own));
    assert!(!own.status.success(), "{text}");
    assert!(text.contains("target own: FAILED"), "{text}");
    assert!(dir.path().join("own-ran.txt").exists(), "{text}");
    std::fs::remove_file(dir.path().join("own-ran.txt")).unwrap();
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
    assert_no_target_ran(dir, &text);
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
fn diagnostics_name_the_config_the_run_read() {
    // TSK-1020 review 1: with --config, the remedy and the error name that
    // file, never the repository config the run ignores.
    let dir = repo(OWN);
    let home = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let empty = side.path().join("empty.json");
    std::fs::write(&empty, r#"{"schema_version":"1.0","targets":[]}"#).unwrap();
    let out = run(
        dir.path(),
        home.path(),
        &[
            "test",
            "--mode",
            "quick",
            "--strict",
            "--config",
            empty.to_str().unwrap(),
        ],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains(&format!("{} defines no targets", empty.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "Add a target for this mode to {}",
            empty.display()
        )),
        "{text}"
    );
    assert!(!text.contains(".codeflow/test-config.json"), "{text}");
    assert_no_target_ran(dir.path(), &text);
    let named = named_config(side.path());
    let out = run(
        dir.path(),
        home.path(),
        &[
            "test",
            "--mode",
            "quick",
            "--strict",
            "--only",
            "absent",
            "--config",
            named.to_str().unwrap(),
        ],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("absent"), "{text}");
    assert!(text.contains(&named.display().to_string()), "{text}");
    assert!(!text.contains(".codeflow/test-config.json"), "{text}");
    assert_no_target_ran(dir.path(), &text);
}

/// A fifo planted at the named path is refused promptly, with no target
/// started: an open of a fifo with no writer would otherwise wait forever.
#[cfg(unix)]
#[test]
fn a_fifo_at_the_named_path_is_refused_promptly() {
    let dir = repo(OWN);
    let home = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let fifo = side.path().join("pipe.json");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    // Output goes to files, not pipes, so waiting on the child cannot block
    // on an undrained pipe.
    let log = side.path().join("out.log");
    let mut child = super::command(
        dir.path(),
        home.path(),
        &[
            "test",
            "--mode",
            "quick",
            "--strict",
            "--config",
            fifo.to_str().unwrap(),
        ],
    )
    .stdout(std::fs::File::create(&log).unwrap())
    .stderr(std::fs::File::options().append(true).open(&log).unwrap())
    .spawn()
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("codeflow test --config <fifo> did not return within 20 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(!status.success(), "{text}");
    assert!(text.contains("not a regular file"), "{text}");
    assert_no_target_ran(dir.path(), &text);
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
