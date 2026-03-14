//! Validate command: validate task and epic markdown frontmatter.
//!
//! Subcommands:
//! - `codeflow validate epic <file>` -- validate a single epic file
//! - `codeflow validate task <file>` -- validate a single task file
//! - `codeflow validate all` -- recursively validate all epics/tasks
//!
//! Exits with code 0 on success, 1 on validation errors.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::helpers;

/// Validate subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ValidateCommand {
    /// Validate a single epic markdown file
    Epic {
        /// Path to the epic markdown file
        file: PathBuf,
    },
    /// Validate a single task markdown file
    Task {
        /// Path to the task markdown file
        file: PathBuf,
    },
    /// Recursively validate all epics and tasks in project-management/
    All,
}

pub fn run(cmd: ValidateCommand) -> Result<()> {
    let opts = codeflow_core::validate::ValidateOptions::default();

    match cmd {
        ValidateCommand::Epic { file } => {
            let (errs, warns) = codeflow_core::validate::validate_epic(&file, &opts)
                .with_context(|| format!("validating epic {}", file.display()))?;
            print_results(&file, &errs, &warns);
            if errs.is_empty() {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        ValidateCommand::Task { file } => {
            let (errs, warns) = codeflow_core::validate::validate_task(&file, &opts)
                .with_context(|| format!("validating task {}", file.display()))?;
            print_results(&file, &errs, &warns);
            if errs.is_empty() {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        ValidateCommand::All => run_all(&opts),
    }
}

fn print_results(
    path: &Path,
    errs: &[codeflow_core::validate::ValidationError],
    warns: &[codeflow_core::validate::ValidationWarning],
) {
    for e in errs {
        eprintln!("ERROR {}: {e}", path.display());
    }
    for w in warns {
        eprintln!("WARN  {}: {w}", path.display());
    }
    if errs.is_empty() && warns.is_empty() {
        println!("{}: ok", path.display());
    }
}

fn run_all(opts: &codeflow_core::validate::ValidateOptions) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let pm_dir = project_dir.join("project-management");

    let mut total_errors = 0;
    let mut total_warnings = 0;
    let mut files_checked = 0;

    let epics_dir = pm_dir.join("epics");
    if epics_dir.is_dir() {
        validate_dir_recursive(
            &epics_dir,
            opts,
            &mut total_errors,
            &mut total_warnings,
            &mut files_checked,
        )?;
    }

    println!("validated {files_checked} files: {total_errors} errors, {total_warnings} warnings");

    if total_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn validate_dir_recursive(
    dir: &Path,
    opts: &codeflow_core::validate::ValidateOptions,
    total_errors: &mut usize,
    total_warnings: &mut usize,
    files_checked: &mut usize,
) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            validate_dir_recursive(&path, opts, total_errors, total_warnings, files_checked)?;
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }

        *files_checked += 1;
        let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        let (errs, warns) = if filename.contains("-TSK-") {
            codeflow_core::validate::validate_task(&path, opts)
                .with_context(|| format!("validating task {}", path.display()))?
        } else if filename.contains("-EPC-") {
            codeflow_core::validate::validate_epic(&path, opts)
                .with_context(|| format!("validating epic {}", path.display()))?
        } else {
            continue;
        };

        for e in &errs {
            eprintln!("ERROR {}: {e}", path.display());
        }
        for w in &warns {
            eprintln!("WARN  {}: {w}", path.display());
        }
        *total_errors += errs.len();
        *total_warnings += warns.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: ValidateCommand,
    }

    #[test]
    fn test_validate_epic_subcommand_parses() {
        let cli = TestCli::try_parse_from(["test", "epic", "/tmp/epic.md"]);
        assert!(cli.is_ok(), "should parse validate epic <file>");
    }

    #[test]
    fn test_validate_task_subcommand_parses() {
        let cli = TestCli::try_parse_from(["test", "task", "/tmp/task.md"]);
        assert!(cli.is_ok(), "should parse validate task <file>");
    }

    #[test]
    fn test_validate_all_subcommand_parses() {
        let cli = TestCli::try_parse_from(["test", "all"]);
        assert!(cli.is_ok(), "should parse validate all");
    }

    #[test]
    fn test_validate_requires_subcommand() {
        let cli = TestCli::try_parse_from(["test"]);
        assert!(cli.is_err(), "validate without subcommand should fail");
    }

    #[test]
    fn test_print_results_no_errors_no_warnings() {
        let path = Path::new("/tmp/test.md");
        // Should print "ok" without panicking.
        print_results(path, &[], &[]);
    }

    #[test]
    fn test_validate_epic_nonexistent_file() {
        let result = run(ValidateCommand::Epic {
            file: PathBuf::from("/nonexistent/epic.md"),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_task_nonexistent_file() {
        let result = run(ValidateCommand::Task {
            file: PathBuf::from("/nonexistent/task.md"),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_dir_recursive_empty() {
        let dir = tempfile::tempdir().unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 0);
        assert_eq!(errors, 0);
    }

    #[test]
    fn test_validate_dir_recursive_skips_non_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("data.json"), "{}").unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 0);
    }

    #[test]
    fn test_validate_dir_recursive_skips_non_matching_md() {
        let dir = tempfile::tempdir().unwrap();
        // MD file without -TSK- or -EPC- in name is counted but not validated.
        std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        // files_checked increments for all .md files before pattern check
        assert_eq!(files, 1);
        assert_eq!(errors, 0);
    }

    #[test]
    fn test_validate_dir_recursive_processes_task_file() {
        let dir = tempfile::tempdir().unwrap();
        // Task file with minimal frontmatter (will produce validation errors but not Err).
        std::fs::write(
            dir.path().join("INF-TSK-001-001.md"),
            "---\ntitle: Test Task\n---\n# Task\n",
        )
        .unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 1);
    }

    #[test]
    fn test_validate_dir_recursive_processes_epic_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("INF-EPC-001.md"),
            "---\ntitle: Test Epic\n---\n# Epic\n",
        )
        .unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 1);
    }

    #[test]
    fn test_validate_dir_recursive_recurses_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("INF-TSK-002-001.md"),
            "---\ntitle: Test Task\n---\n# Task\n",
        )
        .unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 1);
    }

    #[test]
    fn test_validate_dir_nonexistent_returns_error() {
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result = validate_dir_recursive(
            std::path::Path::new("/nonexistent/validate/dir"),
            &opts,
            &mut errors,
            &mut warnings,
            &mut files,
        );
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("reading directory"),
            "expected 'reading directory' context, got: {msg}"
        );
    }

    #[test]
    fn test_print_results_with_errors() {
        let path = Path::new("/tmp/test.md");
        let errs = vec![codeflow_core::validate::ValidationError {
            field: "status".into(),
            message: "missing required field".into(),
        }];
        print_results(path, &errs, &[]);
    }

    #[test]
    fn test_print_results_with_warnings() {
        let path = Path::new("/tmp/test.md");
        let warns = vec![codeflow_core::validate::ValidationWarning {
            field: "priority".into(),
            message: "recommended field missing".into(),
        }];
        print_results(path, &[], &warns);
    }

    #[test]
    fn test_validate_dir_recursive_counts_errors() {
        let dir = tempfile::tempdir().unwrap();
        // Task file with frontmatter but missing required fields => validation errors.
        std::fs::write(
            dir.path().join("INF-TSK-003-001.md"),
            "---\ntitle: Incomplete\n---\n# Task\n",
        )
        .unwrap();
        let opts = codeflow_core::validate::ValidateOptions::default();
        let mut errors = 0;
        let mut warnings = 0;
        let mut files = 0;
        let result =
            validate_dir_recursive(dir.path(), &opts, &mut errors, &mut warnings, &mut files);
        assert!(result.is_ok());
        assert_eq!(files, 1);
        // Should have some validation errors from missing required fields.
        assert!(errors > 0, "expected validation errors for incomplete task");
    }
}
