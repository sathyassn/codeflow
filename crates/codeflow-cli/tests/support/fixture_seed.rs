//! Fixture seeds built once per nextest run (or once per `cargo test`
//! process) and copied for each test (TSK-356).
//!
//! A slow fixture, such as a planned project with a bare origin, is built
//! by the first test that asks for its key. Every later test gets its own
//! copy of the complete tree, with its own files, Git objects and a
//! relocated local remote, so no test sees another's changes.
//!
//! Under nextest each test is its own process, so the seed lives in a
//! folder named for the run ID under Cargo's target directory and a file
//! lock lets one process build it. A run removes seed folders that earlier
//! runs left more than a day ago.

// Each test file uses the part it needs.
#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// Seed folders older than this belong to finished runs.
const STALE_RUN: Duration = Duration::from_secs(24 * 60 * 60);

fn seeds() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Ok(run) = std::env::var("NEXTEST_RUN_ID") {
            let run = uuid::Uuid::parse_str(&run).expect("nextest run UUID");
            // The executable is in target/[llvm-cov-target/]debug/deps.
            let exe = std::env::current_exe().unwrap();
            let parent = exe
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("fixture-seeds");
            prepare_run_directory(&parent, run, STALE_RUN)
        } else {
            super::process_folders::until_exit(tempfile::tempdir().unwrap())
        }
    })
}

/// Create the seed folder for one nextest run and remove the folders of
/// runs last changed more than `stale` ago. A folder another live run is
/// using is newer than that, so it stays.
pub fn prepare_run_directory(parent: &Path, run: uuid::Uuid, stale: Duration) -> PathBuf {
    let current = parent.join(run.to_string());
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|changed| SystemTime::now().duration_since(changed).ok())
                .is_some_and(|age| age > stale);
            if entry.path() != current && entry.file_type().is_ok_and(|kind| kind.is_dir()) && old {
                // A sibling process removing the same folder is harmless.
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    std::fs::create_dir_all(&current).unwrap();
    current
}

/// Build `key` once, then return an independent copy of the whole fixture.
/// The key names every input that changes the seed (tier, task count,
/// policy).
pub fn copy(key: &str, construct: impl FnOnce() -> tempfile::TempDir) -> tempfile::TempDir {
    copy_in(seeds(), key, construct)
}

/// [`copy`] with the seed folder named, so a test can count builds.
pub fn copy_in(
    root: &Path,
    key: &str,
    construct: impl FnOnce() -> tempfile::TempDir,
) -> tempfile::TempDir {
    assert!(
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "fixture seed key {key:?}"
    );
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
    // Each copy stands for one build the test made before; both counts stay
    // with the run's seeds so the saving can be read after a run.
    record(&root.join(format!("{key}.copies")));
    drop(lock);
    let copy = tempfile::tempdir().unwrap();
    copy_tree(&seed, copy.path(), &seed, copy.path());
    copy
}

/// How many times `key` was built and copied under `root`.
pub fn counts(root: &Path, key: &str) -> (usize, usize) {
    let lines = |suffix: &str| {
        std::fs::read_to_string(root.join(format!("{key}.{suffix}")))
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    (lines("builds"), lines("copies"))
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
                "fixture seed holds a link: {}",
                path.display()
            );
            std::fs::copy(&path, &target).unwrap();
            // A Git config may name the fixture's bare origin by absolute
            // path. Object bytes and committed content are never rewritten.
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
    // Read through Git's quoting before replacing paths (Windows
    // backslashes in particular).
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
