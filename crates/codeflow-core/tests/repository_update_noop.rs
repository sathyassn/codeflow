//! This repository is its own first consumer: `codeflow update` on it
//! must be a no-op. When a shipped asset changes without a resync, update
//! rewrites the committed `.codeflow/.baseline/` copy and the manifest hash
//! (and, for a managed file, the live copy), and nothing else notices.
//!
//! This test replays update against a scratch copy of the files update reads
//! and writes (`.codeflow/` plus every manifest destination), using the
//! shipped `assets/` tree and this build's version, and fails naming every
//! file update would change. It never touches the repository itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{update, DirSource, ScaffoldManifest, UpdateOptions};

/// The command a landing task runs to bring the repository back in step.
const RESYNC: &str = "cargo run -p codeflow-cli -- update";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Tracked paths under `.codeflow/`, so local runtime state never enters the
/// replay (the same view a clean clone has).
fn tracked_codeflow_paths(root: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", ".codeflow"])
        .output()
        .expect("git is available to list tracked .codeflow files");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .collect()
}

/// Every regular file under `dir`, keyed by forward-slash path relative to
/// `base`, with its bytes.
fn snapshot(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).expect("scratch dir is readable") {
        let path = entry.expect("scratch entry is readable").path();
        if path.is_dir() {
            snapshot(base, &path, out);
        } else {
            let rel = path
                .strip_prefix(base)
                .expect("path under scratch root")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).expect("scratch file is readable");
            out.insert(rel, bytes);
        }
    }
}

#[test]
fn codeflow_update_is_a_no_op_on_this_repository() {
    let root = repo_root();
    let assets = DirSource::new(root.join("assets"));

    let installed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".codeflow/manifest.json")).expect("installed manifest reads"),
    )
    .expect("installed manifest is JSON");
    let shipped = ScaffoldManifest::load(&assets).expect("shipped manifest loads");

    let mut paths: BTreeSet<String> = tracked_codeflow_paths(&root).into_iter().collect();
    paths.extend(
        installed["files"]
            .as_object()
            .expect("installed manifest has a files map")
            .keys()
            .cloned(),
    );
    paths.extend(shipped.entries.iter().map(|entry| entry.dest.clone()));

    // The project name comes from the root directory name, so the scratch
    // root carries the repository's name, as a clean clone would.
    let scratch = tempfile::tempdir().expect("tempdir");
    let copy = scratch
        .path()
        .canonicalize()
        .expect("tempdir resolves")
        .join("codeflow");
    for rel in &paths {
        let from = root.join(rel);
        if !from.is_file() {
            continue; // update adds it; the diff below reports that
        }
        let to = copy.join(rel);
        std::fs::create_dir_all(to.parent().expect("dest has a parent")).expect("mkdir");
        std::fs::copy(&from, &to).expect("copy into scratch");
    }

    let mut before = BTreeMap::new();
    snapshot(&copy, &copy, &mut before);

    let report = update(
        &assets,
        &copy,
        &UpdateOptions {
            force: false,
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            diff_out: None,
        },
    )
    .expect("codeflow update runs on the scratch copy");

    let mut after = BTreeMap::new();
    snapshot(&copy, &copy, &mut after);

    let mut drift: Vec<String> = Vec::new();
    for (rel, bytes) in &after {
        match before.get(rel) {
            None => drift.push(format!("added    {rel}")),
            Some(old) if old != bytes => drift.push(format!("changed  {rel}")),
            Some(_) => {}
        }
    }
    drift.extend(
        before
            .keys()
            .filter(|rel| !after.contains_key(*rel))
            .map(|rel| format!("removed  {rel}")),
    );

    assert!(
        drift.is_empty(),
        "`codeflow update` is not a no-op on this repository: a shipped asset \
         changed without resyncing CodeFlow's own install. Run `{RESYNC}` at \
         the repository (or worktree) root, review what it changed and commit \
         it with the asset change. Files update would change:\n  {}\n\n{report}",
        drift.join("\n  ")
    );
}
