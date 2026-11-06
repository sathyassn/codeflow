//! TSK-362 AC-4: with the built binary, a fabricated `verified_by` tag makes
//! `validate --docs` fail, and restoring the tag makes it pass.
use std::path::Path;
use std::process::{Command, Output};

fn validate_docs(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["validate", "--docs"])
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn registry(tag: &str) -> String {
    format!(
        "# Capabilities\n\n```yaml\nid: CAP-001\nname: flow\narea: engine\nstatus: shipped\nverified_by: [{tag:?}]\n```\n\nProse.\n"
    )
}

#[test]
fn a_fabricated_tag_fails_and_restoring_it_passes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "tests/flow.rs", "#[test]\nfn real_case() {}\n");
    let real = "test: tests/flow.rs::real_case";
    let fabricated = "test: tests/flow.rs::fabricated_case";

    write(root, "docs/capabilities.md", &registry(real));
    let before = validate_docs(root);
    assert!(before.status.success(), "{before:?}");

    write(root, "docs/capabilities.md", &registry(fabricated));
    let refused = validate_docs(root);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(stderr.contains("CAP-001"), "{stderr}");
    assert!(stderr.contains(fabricated), "{stderr}");

    write(root, "docs/capabilities.md", &registry(real));
    let restored = validate_docs(root);
    assert!(restored.status.success(), "{restored:?}");
}
