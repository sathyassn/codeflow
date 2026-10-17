//! Run the managed workflow's real secret-scan journey with local tool builds.
//! The shared harness also runs in CI's scaffold secret scan. Its download is served
//! from the local gitleaks archive; the release checksum boundary is mocked.
//!
//! ```sh
//! CODEFLOW_GITLEAKS_BIN=/absolute/path/to/gitleaks \
//! CODEFLOW_TEST_BINARY=/absolute/path/to/codeflow \
//! cargo test -p codeflow-core --test managed_secret_scan -- --ignored --nocapture
//! ```

#[cfg(unix)]
#[test]
#[ignore = "requires real gitleaks and a current codeflow build at the documented environment paths"]
fn managed_secret_scan_enforces_trusted_policy_and_unambiguous_ranges() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gitleaks = std::env::var_os("CODEFLOW_GITLEAKS_BIN")
        .expect("set CODEFLOW_GITLEAKS_BIN to the gitleaks binary");
    let codeflow = std::env::var_os("CODEFLOW_TEST_BINARY")
        .expect("set CODEFLOW_TEST_BINARY to this worktree's codeflow build");
    let output = std::process::Command::new("sh")
        .arg(root.join("scripts/check-scaffold-gitleaks.sh"))
        .arg(gitleaks)
        .arg(codeflow)
        .current_dir(root)
        .output()
        .expect("run the managed secret-scan harness");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "managed secret-scan journey failed:\n{stdout}\n{stderr}"
    );
    print!("{stdout}");
}
