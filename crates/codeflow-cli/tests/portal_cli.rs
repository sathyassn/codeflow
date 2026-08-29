use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn codeflow(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("codeflow binary runs")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn initialized_fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let initialized = codeflow(&root, &home, &["init", "--yes", "--standard"]);
    assert!(
        initialized.status.success(),
        "{}",
        output_text(&initialized)
    );
    Fixture {
        _temp: temp,
        root,
        home,
    }
}

#[test]
fn portal_cli_adopts_reconciles_and_preserves_user_configuration() {
    let fixture = initialized_fixture();

    let first = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"],
    );
    assert!(first.status.success(), "{}", output_text(&first));
    let first_report = String::from_utf8(first.stdout).unwrap();
    assert!(first_report.contains("guide/package-lock.json"));
    assert!(fixture.root.join("guide/portal.config.json").is_file());
    assert!(fixture.root.join(".codeflow/docs-portal.json").is_file());

    let user_config = "{\"project_owned\":true}\n";
    std::fs::write(fixture.root.join("guide/portal.config.json"), user_config).unwrap();
    let second = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"],
    );
    assert!(second.status.success(), "{}", output_text(&second));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("guide/portal.config.json")).unwrap(),
        user_config
    );

    let updated = codeflow(&fixture.root, &fixture.home, &["update"]);
    assert!(updated.status.success(), "{}", output_text(&updated));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("guide/portal.config.json")).unwrap(),
        user_config
    );
}

#[test]
fn portal_cli_fails_closed_before_mutating_unsafe_or_uninitialized_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    let uninitialized = codeflow(&root, &home, &["portal", "setup", "--path", "guide"]);
    assert!(!uninitialized.status.success());
    assert!(
        String::from_utf8_lossy(&uninitialized.stderr).contains("not a codeflow project"),
        "{}",
        output_text(&uninitialized)
    );
    assert!(!root.join("guide").exists());

    let fixture = initialized_fixture();
    let unsafe_path = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "../escape"],
    );
    assert!(!unsafe_path.status.success());
    assert!(
        String::from_utf8_lossy(&unsafe_path.stderr).contains("invalid portal path"),
        "{}",
        output_text(&unsafe_path)
    );
    assert!(!fixture.root.parent().unwrap().join("escape").exists());
}

#[test]
fn portal_validation_cli_reports_missing_evidence_without_running_project_code() {
    let fixture = initialized_fixture();
    let setup = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"],
    );
    assert!(setup.status.success(), "{}", output_text(&setup));

    let marker = fixture.root.join("project-code-ran");
    std::fs::write(
        fixture.root.join("guide/package.json"),
        format!(
            "{{\"scripts\":{{\"prevalidate\":\"touch {}\"}}}}\n",
            marker.display()
        ),
    )
    .unwrap();
    let validated = codeflow(
        &fixture.root,
        &fixture.home,
        &["validate", "--portal", "guide"],
    );
    assert!(!validated.status.success());
    assert!(
        String::from_utf8_lossy(&validated.stderr).contains("evidence.json"),
        "{}",
        output_text(&validated)
    );
    assert!(!marker.exists(), "portal validation executed project code");
}
