//! The judging binary identifies its compiled source inputs.
use std::process::Command;

#[test]
fn version_and_hook_report_compiled_identity() {
    let binary = env!("CARGO_BIN_EXE_codeflow");
    let version = Command::new(binary).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&version.stdout);
    for field in ["source=", "dirty=", "inputs="] {
        assert!(text.contains(field), "{text}");
    }
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(binary)
        .args(["git-hook", "pre-commit"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let first = text.lines().next().unwrap_or_default();
    for field in [
        "binary=", "version=", "source=", "dirty=", "inputs=", "sha256=",
    ] {
        assert!(first.contains(field), "{text}");
    }
}

#[test]
fn stale_sources_warn_twice_and_matching_inputs_clear_the_warning() {
    use codeflow_core::hooks::{git_hook::judging_identity, source_identity};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for input in source_identity::INPUT_ROOTS {
        let path = root.join(input);
        if path.extension().is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "base").unwrap();
        } else {
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("fixture.rs"), "base").unwrap();
        }
    }
    let original = source_identity::input_digest(root).unwrap();
    assert_eq!(
        judging_identity(root, "3", "unavailable", "true", &original).len(),
        1
    );
    for content in ["first edit", "second edit"] {
        std::fs::write(
            root.join("crates/codeflow-core/src/hooks/fixture.rs"),
            content,
        )
        .unwrap();
        let stale = judging_identity(root, "3", "unavailable", "true", &original);
        assert!(stale[1].contains("built from different hook or policy sources"));
        let rebuilt = source_identity::input_digest(root).unwrap();
        assert_eq!(
            judging_identity(root, "3", "unavailable", "true", &rebuilt).len(),
            1
        );
    }
    // The guards read paths through `portable_path`, so an edit there alone
    // is a stale binary too.
    let before = source_identity::input_digest(root).unwrap();
    std::fs::write(
        root.join("crates/codeflow-core/src/portable_path.rs"),
        "edit",
    )
    .unwrap();
    let stale = judging_identity(root, "3", "unavailable", "true", &before);
    assert!(stale[1].contains("built from different hook or policy sources"));
}
