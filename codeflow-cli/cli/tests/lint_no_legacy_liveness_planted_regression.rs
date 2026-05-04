//! INF-TSK-050-003 AC-15 / WS-REV MAJOR-7: planted-regression test for
//! the legacy-liveness lint guard.
//!
//! AC-15 contract: the script must exit 1 when production Rust code
//! references the deleted `is_session_stale` or `check_heartbeat_alive`
//! symbols, AND must NOT flag doc-comment references that merely name
//! the symbols in prose. WS-REV MAJOR-7 fix: previously this was
//! validated manually; now the contract is enforced by automation.
//!
//! Test structure: plant offending content under a tempdir, point the
//! script at it via `--target`, assert exit 1 + offending file:line
//! reported. Companion test plants a doc-comment reference and asserts
//! exit 0 (allowlist works).

use std::path::{Path, PathBuf};
use std::process::Command;

fn lint_script_path() -> PathBuf {
    // Walk up from the cli crate dir to the repo root, then descend
    // into the script's known location.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent() // codeflow-cli/
        .and_then(Path::parent) // repo root
        .expect("repo root parent")
        .join(".codeflow/testing/scripts/lint/test-no-legacy-liveness.sh")
}

#[test]
fn lint_guard_fires_on_planted_is_session_stale_call() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path();
    // Mimic the production source layout the script scans:
    // <target>/cli/src/main.rs (or any *.rs under <target> excluding
    // /target/ and /tests/). Use a path that matches the script's
    // find expression.
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");
    let planted = src_dir.join("planted.rs");
    std::fs::write(
        &planted,
        r"
fn main() {
    let _ = is_session_stale(true, None, None);
}
",
    )
    .expect("write planted source");

    let output = Command::new("bash")
        .arg(lint_script_path())
        .arg("--target")
        .arg(target)
        .output()
        .expect("invoke lint script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "AC-15: planted is_session_stale must exit non-zero; stdout={stdout}, stderr={stderr}"
    );
    // The script reports findings to stderr in text format.
    assert!(
        stderr.contains("is_session_stale"),
        "stderr must name the offending symbol; got: {stderr}"
    );
    assert!(
        stderr.contains("planted.rs"),
        "stderr must name the offending file; got: {stderr}"
    );
}

#[test]
fn lint_guard_fires_on_planted_check_heartbeat_alive_call() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path();
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");
    let planted = src_dir.join("planted_heartbeat.rs");
    std::fs::write(
        &planted,
        r#"
fn main() {
    let _ = check_heartbeat_alive("dir", "sid", 90);
}
"#,
    )
    .expect("write planted source");

    let output = Command::new("bash")
        .arg(lint_script_path())
        .arg("--target")
        .arg(target)
        .output()
        .expect("invoke lint script");

    assert!(
        !output.status.success(),
        "AC-15: planted check_heartbeat_alive must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check_heartbeat_alive"),
        "stderr must name the offending symbol; got: {stderr}"
    );
}

#[test]
fn lint_guard_allows_doc_comment_references() {
    // Doc-comment lines (`///`, `//!`, `//`) that name the deleted
    // symbols in prose are allowed — the lint script's allowlist
    // strips comment-only lines before applying the regex. This test
    // verifies the allowlist works.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path();
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");
    let comment_only = src_dir.join("comment_only.rs");
    std::fs::write(
        &comment_only,
        r"
//! This module previously called is_session_stale and check_heartbeat_alive.
/// The deleted is_session_stale predicate is documented here for history.
// is_session_stale was removed by AC-09 — see INF-TSK-050-003.
fn nothing() {}
",
    )
    .expect("write comment-only source");

    let output = Command::new("bash")
        .arg(lint_script_path())
        .arg("--target")
        .arg(target)
        .output()
        .expect("invoke lint script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "AC-15: comment-only references must NOT trigger the guard. \
         exit={:?}, stdout={stdout}, stderr={stderr}",
        output.status.code()
    );
}

#[test]
fn lint_guard_passes_on_clean_target() {
    // Empty target with no .rs files at all → the script reports
    // "no rust files found" and exits 2 (sanity, not a guard failure).
    // We verify it does NOT exit 0 with success in this edge case to
    // catch regressions that silently skip the scan.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path();
    // Create the directory structure but NO .rs files.
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");

    let output = Command::new("bash")
        .arg(lint_script_path())
        .arg("--target")
        .arg(target)
        .output()
        .expect("invoke lint script");

    // Exit 2 is the "no files found" case — explicit sanity exit, not
    // a silent pass.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "no .rs files must exit 2 (sanity), not 0; stderr={stderr}"
    );
}

#[test]
fn lint_guard_format_json_emits_machine_readable_output() {
    // Verify --format json works as documented (operator-facing
    // contract for CI consumers).
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path();
    let src_dir = target.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir src");
    let planted = src_dir.join("planted_json.rs");
    std::fs::write(
        &planted,
        r"fn main() { let _ = is_session_stale(true, None, None); }",
    )
    .expect("write");

    let output = Command::new("bash")
        .arg(lint_script_path())
        .arg("--format")
        .arg("json")
        .arg("--target")
        .arg(target)
        .output()
        .expect("invoke lint script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Exit 1 (findings present) — AC-15.
    assert_eq!(output.status.code(), Some(1));
    // JSON shape: { "count": 1, "findings": [...] }.
    assert!(
        stdout.contains("\"count\""),
        "json output must have count field; got: {stdout}"
    );
    assert!(
        stdout.contains("\"findings\""),
        "json output must have findings array; got: {stdout}"
    );
    assert!(
        stdout.contains("is_session_stale"),
        "json output must include the offending symbol; got: {stdout}"
    );
}
