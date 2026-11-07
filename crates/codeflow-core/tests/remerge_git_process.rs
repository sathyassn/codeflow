//! TSK-1185 AC-9: the `git` process that judges a merge, seen from outside.
//! A fake `git` first on `PATH` acts as a Git older than 2.41, and then
//! records the environment a real remerge receives. `PATH` and `GIT_` values
//! are process-wide, so this file holds one test and runs in its own process.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use codeflow_core::hooks::path_policy::{self, PathPolicy};
use codeflow_core::hooks::policy::GitPolicy;
use codeflow_core::workgraph::history;
use git2::{Oid, Repository};

fn git(root: &Path, args: &[&str]) -> String {
    let output = codeflow_core::git::judging_command()
        .current_dir(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn commit_file(root: &Path, name: &str) {
    std::fs::write(root.join(name), "text\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", name]);
}

/// A repository whose `HEAD` is a clean merge, and the commit before it.
fn merged_repository(root: &Path) -> String {
    git(root, &["init", "-q", "-b", "main"]);
    commit_file(root, "base.txt");
    let base = git(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "task"]);
    commit_file(root, "task.txt");
    git(root, &["checkout", "-q", "main"]);
    commit_file(root, "main.txt");
    git(root, &["merge", "-q", "--no-ff", "-m", "merge", "task"]);
    base
}

fn real_git() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|folder| folder.join("git"))
        .find(|path| path.is_file())
        .unwrap()
}

/// Put `script` first on `PATH` as `git`.
fn fake_git(folder: &Path, script: &str) {
    let path = folder.join("git");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var_os("PATH").unwrap();
    let mut folders = vec![folder.to_path_buf()];
    folders.extend(std::env::split_paths(&path));
    std::env::set_var("PATH", std::env::join_paths(folders).unwrap());
}

fn remerge(root: &Path) -> Result<Vec<Vec<u8>>, git2::Error> {
    let repo = Repository::open(root).unwrap();
    let merge = repo.head().unwrap().peel_to_commit().unwrap();
    let parents: Vec<_> = merge.parents().collect();
    history::remerge_differences(&repo, &parents[0], &parents[1], merge.tree_id())
}

#[test]
fn tsk1185_the_judging_git_process() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path();
    let base = merged_repository(root);
    Oid::from_str(&base).unwrap();
    let real = real_git();
    let (old, recording) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let original_path = std::env::var_os("PATH").unwrap();

    // A Git older than 2.41 stops at `--attr-source` with exit 129.
    fake_git(
        old.path(),
        &format!(
            "#!/bin/sh\n\
             [ \"$1\" = version ] && {{ echo 'git version 2.40.1'; exit 0; }}\n\
             for argument; do case \"$argument\" in --attr-source=*)\n\
             echo \"unknown option: $argument\" >&2; exit 129;; esac; done\n\
             exec '{}' \"$@\"\n",
            real.display()
        ),
    );
    let refusal = remerge(root).unwrap_err().message().to_string();
    assert!(refusal.contains("Git 2.40.1 is too old"), "{refusal}");
    assert!(refusal.contains("Git 2.41 or later"), "{refusal}");
    assert!(refusal.contains("not a bad merge"), "{refusal}");
    assert!(refusal.contains("upgrade Git"), "{refusal}");

    // A caller sees that refusal, not a merge resolution finding.
    let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
    let refusal = path_policy::range_with(root, Some(&base), "HEAD", &paths, None).unwrap_err();
    assert!(refusal.contains("Git 2.40.1 is too old"), "{refusal}");
    std::env::set_var("PATH", &original_path);

    // A real remerge receives only the `GIT_` values the code sets.
    let log = recording.path().join("environment");
    fake_git(
        recording.path(),
        &format!(
            "#!/bin/sh\nenv >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            real.display()
        ),
    );
    for name in [
        "GIT_GRAFT_FILE",
        "GIT_SHALLOW_FILE",
        "GIT_EXEC_PATH",
        "GIT_REPLACE_REF_BASE",
        "GIT_ATTR_SOURCE",
    ] {
        std::env::set_var(name, "/nonexistent");
    }
    assert_eq!(remerge(root).unwrap(), Vec::<Vec<u8>>::new());
    let log = std::fs::read_to_string(log).unwrap();
    let mut seen: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("GIT_"))
        .filter_map(|line| line.split('=').next())
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen,
        [
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_ATTR_NOSYSTEM",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_NO_LAZY_FETCH",
        ]
    );
}
