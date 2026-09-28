//! Every `git` process codeflow starts is built by `codeflow_core::git::command`
//! (TSK-141 AC-4, SPC-013 R-85 as amended), so a hook git fires during a
//! codeflow command runs that same binary. A new spawn written as
//! `Command::new("git")` would dispatch its hooks by PATH again.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_git_process_is_built_by_the_one_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let constructor = crates.join("codeflow-core/src/git/mod.rs");
    let mut files = Vec::new();
    for krate in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        rust_files(&crates.join(krate).join("src"), &mut files);
    }
    assert!(files.contains(&constructor), "the constructor moved");
    let spawn = concat!("Command::new(", "\"git\")");
    let mut offenders = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let count = text.matches(spawn).count();
        let allowed = usize::from(*file == constructor);
        if count != allowed {
            offenders.push(format!("{}: {count}", file.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "spawn git through codeflow_core::git::command():\n{}",
        offenders.join("\n")
    );
}
