//! Git operations command: merge conflict detection and branch analysis.
//!
//! Subcommands:
//! - `check-conflicts --target <branch>` — detect merge conflicts with target branch

use std::path::Path;
use std::process;

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Git operations subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum GitCommand {
    /// Check for merge conflicts with a target branch
    #[command(name = "check-conflicts")]
    CheckConflicts {
        /// Target branch to check against
        #[arg(long, default_value = "main")]
        target: String,
    },
}

pub fn run(cmd: Option<GitCommand>) -> Result<()> {
    let Some(subcmd) = cmd else {
        println!("Git operations");
        println!();
        println!("Usage:");
        println!("  codeflow git <command>");
        println!();
        println!("Available Commands:");
        println!("  check-conflicts    Check for merge conflicts with a target branch");
        return Ok(());
    };

    match subcmd {
        GitCommand::CheckConflicts { target } => run_check_conflicts(&target),
    }
}

fn run_check_conflicts(target: &str) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_check_conflicts_with_dir(&project_dir, target)
}

fn run_check_conflicts_with_dir(project_dir: &Path, target: &str) -> Result<()> {
    match codeflow_core::git::check_merge_conflicts(project_dir, target) {
        Ok(result) => {
            if result.has_conflicts {
                let json = serde_json::json!({
                    "has_conflicts": true,
                    "target_branch": result.target_branch,
                    "conflicting_files": result.conflicting_files,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
                process::exit(1);
            }
            // Clean merge — exit silently with code 0.
            Ok(())
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a git repo with an initial commit containing a file.
    fn init_repo_with_file(dir: &Path, filename: &str, content: &str) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();

        fs::write(dir.join(filename), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(filename)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();

        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn test_run_no_subcommand_shows_help() {
        let result = run(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_conflicts_clean_merge() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        // Create target branch.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &head, false).unwrap();
        }

        // Add a non-conflicting file on main.
        {
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            fs::write(dir.path().join("new.txt"), "new\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("new.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "add new file", &tree, &[&head])
                .unwrap();
        }

        let result = run_check_conflicts_with_dir(dir.path(), "target");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_conflicts_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        // Not a git repo — run_check_conflicts_with_dir calls process::exit(2),
        // which we can't test directly. Instead, test the core function.
        let result = codeflow_core::git::check_merge_conflicts(dir.path(), "main");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_conflicts_target_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_file(dir.path(), "file.txt", "content\n");
        let result = codeflow_core::git::check_merge_conflicts(dir.path(), "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_conflicts_with_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "base\n");

        // Create target branch from initial commit.
        {
            let initial = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &initial, false).unwrap();
        }

        // Switch to target and modify the file.
        repo.set_head("refs/heads/target").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        {
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            fs::write(dir.path().join("file.txt"), "target version\n").unwrap();
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new("file.txt")).unwrap();
            idx.write().unwrap();
            let tid = idx.write_tree().unwrap();
            let tree = repo.find_tree(tid).unwrap();
            let tc = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "target edit", &tree, &[&tc])
                .unwrap();
        }

        // Go back to main and make a conflicting change.
        repo.set_head("refs/heads/main").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        {
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            fs::write(dir.path().join("file.txt"), "main version\n").unwrap();
            let mut idx2 = repo.index().unwrap();
            idx2.add_path(Path::new("file.txt")).unwrap();
            idx2.write().unwrap();
            let tid2 = idx2.write_tree().unwrap();
            let tree2 = repo.find_tree(tid2).unwrap();
            let mc = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "main edit", &tree2, &[&mc])
                .unwrap();
        }

        // Verify the core function detects conflict.
        let result = codeflow_core::git::check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(result.has_conflicts);
        assert!(result.conflicting_files.contains(&"file.txt".to_string()));
    }

    #[test]
    fn test_git_command_enum_variants() {
        // Exercise the Debug impl.
        let cmd = GitCommand::CheckConflicts {
            target: "main".to_string(),
        };
        let debug = format!("{cmd:?}");
        assert!(debug.contains("CheckConflicts"));
        assert!(debug.contains("main"));
    }

    #[test]
    fn test_git_command_clone() {
        let cmd = GitCommand::CheckConflicts {
            target: "develop".to_string(),
        };
        let cloned = cmd.clone();
        match cloned {
            GitCommand::CheckConflicts { target } => {
                assert_eq!(target, "develop");
            }
        }
    }
}
