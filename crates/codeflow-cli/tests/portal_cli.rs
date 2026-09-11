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
    initialized_fixture_for("--standard")
}

fn initialized_fixture_for(tier: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let initialized = codeflow(&root, &home, &["init", "--yes", tier]);
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
fn portal_distribution_is_opt_in_and_hash_only_across_fresh_tiers() {
    for tier in ["--minimal", "--standard", "--full"] {
        let fixture = initialized_fixture_for(tier);
        assert!(!fixture.root.join("guide").exists());
        assert!(!fixture.root.join(".codeflow/docs-portal.json").exists());
        let setup = codeflow(
            &fixture.root,
            &fixture.home,
            &["portal", "setup", "--path", "guide"],
        );
        assert!(setup.status.success(), "{tier}: {}", output_text(&setup));
        let state_path = fixture.root.join(".codeflow/docs-portal.json");
        let state_bytes = std::fs::read(&state_path).unwrap();
        let state: serde_json::Value = serde_json::from_slice(&state_bytes).unwrap();
        assert_eq!(state["schema_version"], 2);
        assert_eq!(state["runtime_ownership"], "managed");
        assert_eq!(state["starter_version"], "2.0.0");
        assert!(state["files"]["scripts/generator.mjs"].is_object());
        assert!(!fixture
            .root
            .join(".codeflow/.docs-portal-baseline")
            .exists());
        let project = std::fs::read(fixture.root.join(".codeflow/project.toml")).unwrap();
        let config_path = fixture.root.join("guide/portal.config.json");
        let config = std::fs::read(&config_path).unwrap();
        for _ in 0..2 {
            let update = codeflow(&fixture.root, &fixture.home, &["update"]);
            assert!(update.status.success(), "{tier}: {}", output_text(&update));
            assert_eq!(std::fs::read(&state_path).unwrap(), state_bytes);
            assert_eq!(std::fs::read(&config_path).unwrap(), config);
            assert_eq!(
                std::fs::read(fixture.root.join(".codeflow/project.toml")).unwrap(),
                project
            );
            assert!(!fixture
                .root
                .join(".codeflow/.docs-portal-baseline")
                .exists());
        }
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

#[test]
fn portal_transfer_requires_confirmation_and_preserves_runtime_after_updates() {
    let fixture = initialized_fixture();
    let setup = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"],
    );
    assert!(setup.status.success(), "{}", output_text(&setup));
    let state = fixture.root.join(".codeflow/docs-portal.json");
    let before = std::fs::read(&state).unwrap();
    let unconfirmed = codeflow(&fixture.root, &fixture.home, &["portal", "transfer"]);
    assert!(!unconfirmed.status.success());
    assert_eq!(std::fs::read(&state).unwrap(), before);
    std::fs::write(
        fixture.root.join("guide/astro.config.mjs"),
        "// project fork\n",
    )
    .unwrap();
    std::fs::remove_file(fixture.root.join("guide/package-lock.json")).unwrap();
    std::fs::remove_file(fixture.root.join("guide/portal.config.json")).unwrap();
    let transferred = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "transfer", "--confirm"],
    );
    assert!(
        transferred.status.success(),
        "{}",
        output_text(&transferred)
    );
    let frozen = std::fs::read(&state).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&frozen).unwrap();
    assert_eq!(json["runtime_ownership"], "transferred");
    assert_eq!(json["schema_version"], 2);
    for args in [
        vec!["portal", "transfer", "--confirm"],
        vec!["portal", "setup", "--path", "guide"],
        vec!["update"],
    ] {
        let output = codeflow(&fixture.root, &fixture.home, &args);
        assert!(output.status.success(), "{}", output_text(&output));
        assert!(output_text(&output).contains("project-owned"));
        assert_eq!(std::fs::read(&state).unwrap(), frozen);
        assert_eq!(
            std::fs::read(fixture.root.join("guide/astro.config.mjs")).unwrap(),
            b"// project fork\n"
        );
        assert!(!fixture.root.join("guide/package-lock.json").exists());
        assert!(!fixture.root.join("guide/portal.config.json").exists());
    }
}

#[test]
fn portal_conflict_is_nonzero_without_state_advance_or_repair() {
    let fixture = initialized_fixture();
    assert!(codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"]
    )
    .status
    .success());
    std::fs::write(
        fixture.root.join("guide/package-lock.json"),
        "project lockfile\n",
    )
    .unwrap();
    std::fs::remove_file(fixture.root.join("guide/.node-version")).unwrap();
    let before = std::fs::read(fixture.root.join(".codeflow/docs-portal.json")).unwrap();
    let conflict = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "setup", "--path", "guide"],
    );
    assert_eq!(
        conflict.status.code(),
        Some(2),
        "{}",
        output_text(&conflict)
    );
    assert!(output_text(&conflict).contains("transfer --confirm"));
    assert_eq!(
        std::fs::read(fixture.root.join(".codeflow/docs-portal.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(fixture.root.join("guide/package-lock.json")).unwrap(),
        b"project lockfile\n"
    );
    assert!(!fixture.root.join("guide/.node-version").exists());
}

#[test]
fn portal_transfer_has_no_root_selection_or_implicit_adoption() {
    let fixture = initialized_fixture();
    let missing = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "transfer", "--confirm"],
    );
    assert!(!missing.status.success());
    assert!(!fixture.root.join(".codeflow/docs-portal.json").exists());
    let extra = codeflow(
        &fixture.root,
        &fixture.home,
        &["portal", "transfer", "--confirm", "--path", "guide"],
    );
    assert!(!extra.status.success());
    assert!(!fixture.root.join("guide").exists());
}
