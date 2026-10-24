//! Immutable fixture seeds shared by a nextest run or one cargo test process.
//! Each consumer gets separate files, Git objects and a relocated local remote.
//! Nextest seeds stay under Cargo's target directory and never cross run IDs.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn seeds() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Ok(run) = std::env::var("NEXTEST_RUN_ID") {
            let run = uuid::Uuid::parse_str(&run).expect("nextest run UUID");
            // The executable is in target/[llvm-cov-target/]debug/deps.
            let exe = std::env::current_exe().unwrap();
            exe.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("fixture-seeds")
                .join(run.to_string())
        } else {
            super::process_folders::until_exit(tempfile::tempdir().unwrap())
        }
    })
}

/// Build `key` once, then return an independent copy of the complete fixture.
/// Keys include every input which changes the seed (tier, task count, policy).
pub fn copy(key: &str, construct: impl FnOnce() -> tempfile::TempDir) -> tempfile::TempDir {
    assert!(!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let root = seeds();
    let seed = root.join(key);
    let lock = codeflow_core::file_lock::lock_path_exclusive(&root.join(format!("{key}.lock")))
        .expect("lock fixture seed");
    let ready = root.join(format!("{key}.ready"));
    if !ready.exists() {
        // An interrupted builder never publishes a partial fixture.
        if seed.exists() {
            std::fs::remove_dir_all(&seed).unwrap();
        }
        record(&root.join(format!("{key}.builds")));
        let built = construct();
        copy_tree(built.path(), &seed, built.path(), &seed);
        std::fs::write(&ready, b"complete\n").unwrap();
    }
    // Each copy replaces one original constructor invocation. Keep both counts
    // with the run's seeds so the setup reduction can be measured directly.
    record(&root.join(format!("{key}.copies")));
    drop(lock);
    let copy = tempfile::tempdir().unwrap();
    copy_tree(&seed, copy.path(), &seed, copy.path());
    copy
}

fn record(path: &Path) {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"1\n")
        .unwrap();
}

fn copy_tree(source: &Path, dest: &Path, old: &Path, new: &Path) {
    std::fs::create_dir_all(dest).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = dest.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&path, &target, old, new);
        } else {
            assert!(
                kind.is_file(),
                "fixture seed contains a link: {}",
                path.display()
            );
            std::fs::copy(&path, &target).unwrap();
            // Git configs may name the fixture's bare origin by absolute path.
            // Never rewrite object bytes or committed content.
            if entry.file_name() == "config"
                && (source.file_name().is_some_and(|n| n == ".git")
                    || source.join("HEAD").is_file() && source.join("objects").is_dir())
            {
                relocate_config(&target, old, new);
            }
        }
    }
}

fn relocate_config(path: &Path, old: &Path, new: &Path) {
    // Decode Git's quoting before replacing paths (notably Windows backslashes).
    let mut config = git2::Config::open(path).unwrap();
    let mut changes = Vec::new();
    {
        let mut entries = config.entries(None).unwrap();
        while let Some(entry) = entries.next() {
            let entry = entry.unwrap();
            let value = entry.value().expect("UTF-8 fixture configuration");
            let relocated = value.replace(
                old.to_string_lossy().as_ref(),
                new.to_string_lossy().as_ref(),
            );
            if relocated != value {
                changes.push((entry.name().unwrap().to_owned(), relocated));
            }
        }
    }
    for (name, value) in changes {
        config.set_str(&name, &value).unwrap();
    }
}
