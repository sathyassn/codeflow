//! Run the managed workflow's real secret-scan journey with local tool builds.
//! The shared harness also runs in CI's scaffold secret scan. Its download is served
//! from the local gitleaks archive; the release checksum boundary is mocked.
//! The optional previous build exercises an older project's update; without
//! it the update route replays this build's init. One Git stub delays a
//! diagnostic probe to exercise its timeout; finding scans use real Git.
//!
//! ```sh
//! CODEFLOW_GITLEAKS_BIN=/absolute/path/to/gitleaks \
//! CODEFLOW_TEST_BINARY=/absolute/path/to/codeflow \
//! CODEFLOW_PREVIOUS_BINARY=/absolute/path/to/older/codeflow \
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
    let previous = std::env::var_os("CODEFLOW_PREVIOUS_BINARY").unwrap_or_else(|| codeflow.clone());
    let output = std::process::Command::new("sh")
        .arg(root.join("scripts/check-scaffold-gitleaks.sh"))
        .arg(gitleaks)
        .arg(codeflow)
        .arg(previous)
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
