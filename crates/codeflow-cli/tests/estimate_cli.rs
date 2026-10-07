//! Black-box forecast schema, diagnostics, and no-write contract.
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const REGISTRY: &str = "{\"schema_version\":1,\"repos\":[]}\n";
const LIMITATION:&str="Checks supplied allocations and local evidence links only; does not establish source truth, authorization, quality completeness, or predictive accuracy.";
const HOSTILE_STATE: &str = "schema_version=1\ntier='full'\nscaffold_version='0.0.0-PRIVATE_STATE_SENTINEL'\nstack=''\nareas=[]\npolicy_armed=false\ngit_hooks='unwired'\npermission_preset='standard'\n";

#[test]
fn estimate_never_loads_unrelated_version_advisory_state() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("absent-home");
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    let state = dir.path().join(".codeflow/project.toml");
    for bytes in [
        HOSTILE_STATE.to_owned(),
        format!("{HOSTILE_STATE}#{}", "x".repeat(2 * 1024 * 1024)),
    ] {
        std::fs::write(&state, &bytes).unwrap();
        let output = run(
            dir.path(),
            &home,
            &["estimate", "check", "missing.json", "--json"],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(
            output.stderr.is_empty(),
            "unrelated advisory must not read or echo project state"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_STATE_SENTINEL"));
        assert_eq!(std::fs::read_to_string(&state).unwrap(), bytes);
    }
    assert!(!home.exists());
}

fn command(root: &Path, home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    command
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("HOME", home)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    command
}

fn run(root: &Path, home: &Path, args: &[&str]) -> Output {
    command(root, home, args).output().unwrap()
}

#[cfg(unix)]
#[test]
fn unrelated_project_state_symlink_and_fifo_are_never_opened() {
    use std::os::unix::ffi::OsStrExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let home = dir.path().join("absent-home");
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    let state = dir.path().join(".codeflow/project.toml");
    let target = outside.path().join("sensitive.toml");
    std::fs::write(&target, HOSTILE_STATE).unwrap();
    std::os::unix::fs::symlink(&target, &state).unwrap();
    let args = ["estimate", "check", "missing.json", "--json"];
    let output = run(dir.path(), &home, &args);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), HOSTILE_STATE);
    std::fs::remove_file(&state).unwrap();
    let c_path = std::ffi::CString::new(state.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path is a NUL-terminated temporary pathname.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let mut child = command(dir.path(), &home, &args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Drain both pipes while waiting: a host under pipe memory pressure
    // gives a new pipe a 512-byte buffer, which the report outgrows, so an
    // undrained pipe would stall the child and read as the FIFO block.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                std::io::Read::read_to_end(&mut pipe, &mut bytes).unwrap();
            }
            bytes
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
    );
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(5) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("estimate blocked opening unrelated project-state FIFO");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(!stdout.join().unwrap().is_empty());
    assert!(stderr.join().unwrap().is_empty());
    assert!(!home.exists());
}

fn forecast() -> Value {
    let pin = |path: &str| json!({"path":path,"sha256":DIGEST});
    let scenarios=["favorable","planning","adverse"].iter().map(|name|json!({"name":name,"assumptions":["Fictional scenario"],"activities":[{"id":"a","package_id":"p","stage":"implement","start_seconds":0,"duration_seconds":10,"demands":[{"resource_id":"worker","units":2}],"after":[],"after_boundaries":[],"basis":"Fictional explicit duration"}],"milestones":[{"id":"done","after":["a"]}]})).collect::<Vec<_>>();
    json!({"schema_version":1,"id":"fixture","anchor_epoch_seconds":0,"horizon_seconds":100,
        "profile":pin("profile.md"),"rubric":pin("rubric.md"),"packages":[{"id":"p","source":{"kind":"declared","pin":pin("brief.md"),"reference":"https://example.invalid/opaque-not-fetched"},"context_pins":[],"grade":"easy","grade_evidence":["Fictional scope"],"duration_basis":"judgment","stage_exclusions":[{"stage":"review","reason":"Fixture exclusion"},{"stage":"verify","reason":"Fixture exclusion"},{"stage":"rework","reason":"Fixture exclusion"}]}],
        "boundaries":[],"resources":[{"id":"worker","capacity":2,"windows":[{"start_seconds":0,"end_seconds":100}]}],"scenarios":scenarios})
}

#[test]
fn unreadable_report_is_golden_and_creates_no_user_or_project_state() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("absent-home");
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::write(dir.path().join(".codeflow/policy.json"), "{}").unwrap();
    let output = run(
        dir.path(),
        &home,
        &["estimate", "check", "missing.json", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    let expected = json!({"schema_version":1,"checked_package_count":0,"source_assurance":"unverified","input_sha256":null,"source_digests":[],"exclusions":[],"scenarios":[],"findings":[{"code":"input_read","location":"input","message":"Forecast is unreadable, changed, non-regular, or exceeds 1 MiB."}],"limitation":LIMITATION});
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        expected
    );
    assert!(!home.exists());
    assert!(!dir.path().join("project-management").exists());
}

#[test]
fn valid_json_report_and_existing_registry_are_stable() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join("registry.json"), REGISTRY).unwrap();
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::write(dir.path().join(".codeflow/policy.json"), "{}").unwrap();
    for path in ["brief.md", "profile.md", "rubric.md"] {
        std::fs::write(dir.path().join(path), "abc").unwrap();
    }
    let bytes = serde_json::to_vec(&forecast()).unwrap();
    std::fs::write(dir.path().join("forecast.json"), &bytes).unwrap();
    let output = run(
        dir.path(),
        &home,
        &["estimate", "check", "forecast.json", "--json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let mut value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let digest = value["input_sha256"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
    value["input_sha256"] = json!("INPUT_DIGEST");
    let scenarios=["favorable","planning","adverse"].iter().map(|name|json!({"name":name,"valid":true,"elapsed_seconds":10,"resources":[{"resource_id":"worker","consumption_seconds":20}],"milestones":[{"id":"done","at_seconds":10}]})).collect::<Vec<_>>();
    let expected = json!({"schema_version":1,"checked_package_count":1,"source_assurance":"limited","input_sha256":"INPUT_DIGEST","source_digests":[{"path":"brief.md","sha256":DIGEST},{"path":"profile.md","sha256":DIGEST},{"path":"rubric.md","sha256":DIGEST}],"exclusions":[{"package_id":"p","stage":"review","reason":"Fixture exclusion"},{"package_id":"p","stage":"verify","reason":"Fixture exclusion"},{"package_id":"p","stage":"rework","reason":"Fixture exclusion"}],"scenarios":scenarios,"findings":[],"limitation":LIMITATION});
    assert_eq!(value, expected);
    assert_eq!(
        std::fs::read(dir.path().join("forecast.json")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read_to_string(home.join("registry.json")).unwrap(),
        REGISTRY
    );
    assert!(!dir.path().join("project-management").exists());
    assert_eq!(
        std::fs::read_dir(&home).unwrap().count(),
        1,
        "registry lock/state must not be created"
    );
}

#[test]
fn malformed_input_is_redacted_and_no_other_verbs_exist() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::write(
        dir.path().join("bad.json"),
        "{\"PRIVATE_CONTENT_SENTINEL\":42}",
    )
    .unwrap();
    let output = run(
        dir.path(),
        &home,
        &["estimate", "check", "bad.json", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_CONTENT_SENTINEL"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_CONTENT_SENTINEL"));
    assert!(!home.exists());
    assert!(
        !run(dir.path(), &home, &["estimate", "schedule", "bad.json"])
            .status
            .success()
    );
}
