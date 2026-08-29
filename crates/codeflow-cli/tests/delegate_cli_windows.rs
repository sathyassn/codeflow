//! Native-Windows coverage for the schema-v2 delegate lifecycle boundary.
#![cfg(windows)]

use std::process::Command;

#[test]
fn init_fails_closed_and_directs_the_operator_to_wsl2() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["delegate", "init", "--run-id", "run-1", "--state-dir"])
        .arg(&state)
        .output()
        .expect("codeflow runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("native Windows is unsupported for delegate state; use WSL2"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!state.exists());
}
