//! Dry grading of the live delivery holdout (TSK-111, SPC-013 R-105, R-106).
//!
//! The six live cases and their scripted solutions are a qualification
//! holdout kept on the private archive's `test/live-delivery-holdout` ref,
//! never in this repository. With `CODEFLOW_EVAL_HOLDOUT` naming a checkout of
//! that ref, this test runs its `tests/test_live_pack.py` against this
//! checkout's eval kit and the binary Cargo built: each case is played
//! correct, incorrect, truncated, empty and adversarial, and only the correct
//! outcome may pass. Without the holdout it says so and checks nothing; the
//! grader itself is covered by `evals/model-artifacts/test_eval_kit.py`.
//!
//! Unix only: the fixtures run the scaffold's shell git hooks and `python3`.
//! Subject code runs only under macOS `sandbox-exec`; elsewhere, or inside
//! another sandbox, the holdout test checks that those assertions fail closed.

#[cfg(unix)]
#[test]
fn the_live_holdout_grades_scripted_outcomes_with_the_built_binary() {
    use std::path::Path;
    use std::process::Command;

    let Some(holdout) = std::env::var_os("CODEFLOW_EVAL_HOLDOUT") else {
        eprintln!(
            "skipped: CODEFLOW_EVAL_HOLDOUT is not set; the live delivery holdout lives on the \
             private archive's test/live-delivery-holdout ref, outside this repository"
        );
        return;
    };
    let script = Path::new(&holdout).join("tests/test_live_pack.py");
    assert!(
        script.is_file(),
        "CODEFLOW_EVAL_HOLDOUT does not hold tests/test_live_pack.py: {}",
        script.display()
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = tempfile::tempdir().expect("home tempdir");
    let output = Command::new("python3")
        .arg("-B")
        .arg(&script)
        .current_dir(&root)
        .env("CODEFLOW_ROOT", &root)
        .env("CODEFLOW_BIN", env!("CARGO_BIN_EXE_codeflow"))
        .env("CODEFLOW_HOME", home.path().join(".codeflow"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
        .output()
        .expect("python3 runs the live holdout dry grading");
    assert!(
        output.status.success(),
        "live holdout dry grading failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
