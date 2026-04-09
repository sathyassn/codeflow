//! Worktree command: git worktree lifecycle management.
//!
//! Supports subcommands: `list`, `cleanup`, `prune`, `repair`.
//! Default (no subcommand) behaves like `list`.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeflow_core::worktree::{
    CleanupAction, CleanupOpts, EntryClassification, WorktreeManager, WorktreeState,
};

use crate::helpers;

/// Worktree subcommands.
#[derive(Debug, Subcommand)]
pub enum WorktreeCommand {
    /// List worktrees and their state (includes liveness status)
    List,
    /// Remove stale or inactive worktrees with liveness and branch safety checks
    Cleanup {
        /// Force removal even of orphaned/unpushed worktrees
        #[arg(long)]
        force: bool,
        /// Force removal of a specific worktree by name substring, or "all"
        #[arg(long, value_name = "ID")]
        force_id: Option<String>,
        /// Report what would be removed without acting
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Show detailed liveness and safety information
        #[arg(short, long)]
        verbose: bool,
        /// Show interactive prompts with branch/PR status before removal
        #[arg(short, long)]
        interactive: bool,
        /// Maximum number of old removed entries to keep (default: 5)
        #[arg(long)]
        keep: Option<usize>,
    },
    /// Remove orphaned worktree references from git internals
    Prune {
        /// Report what would be pruned without acting
        #[arg(long)]
        dry_run: bool,
    },
    /// Repair shared state symlinks in a worktree
    Repair {
        /// Worktree path to repair (defaults to CODEFLOW_WORKTREE_PATH)
        #[arg(long)]
        path: Option<String>,
    },
}

#[allow(clippy::needless_pass_by_value)] // clap passes enum by value
pub fn run(command: Option<WorktreeCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    match command.as_ref() {
        Some(WorktreeCommand::List) | None => run_list(&project_dir),
        Some(WorktreeCommand::Cleanup {
            force,
            force_id,
            dry_run,
            yes,
            verbose,
            interactive,
            keep,
        }) => run_cleanup(
            &project_dir,
            *force,
            force_id.as_deref(),
            *dry_run,
            *yes,
            *verbose,
            *interactive,
            *keep,
        ),
        Some(WorktreeCommand::Prune { dry_run }) => run_prune(&project_dir, *dry_run),
        Some(WorktreeCommand::Repair { path }) => run_repair(&project_dir, path.as_deref()),
    }
}

fn run_list(project_dir: &Path) -> Result<()> {
    let mgr = WorktreeManager::new(project_dir);

    let entries = mgr.list(None).context("listing worktrees")?;

    if entries.is_empty() {
        println!("no worktrees");
        return Ok(());
    }

    for entry in &entries {
        let state = mgr.detect_state(entry);
        let liveness = mgr.liveness_status(entry);
        let branch = entry.branch.as_deref().unwrap_or("(detached)");
        println!(
            "{} ({}) [{}] {} [{}]",
            entry.name,
            branch,
            state,
            liveness.label(),
            entry.status
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn run_cleanup(
    project_dir: &Path,
    force: bool,
    force_id: Option<&str>,
    dry_run: bool,
    yes: bool,
    verbose: bool,
    interactive: bool,
    keep: Option<usize>,
) -> Result<()> {
    let force_flag = force || force_id.is_some();
    let force_target = force_id.and_then(|v| {
        if v == "all" {
            None
        } else {
            Some(v.to_string())
        }
    });
    let mgr = WorktreeManager::new(project_dir);

    // Phase 1: Analyze — gather all entries and classify using core logic.
    let entries = mgr.list(None).context("listing worktrees")?;
    let threshold = codeflow_core::worktree::unknown_liveness_threshold(project_dir);

    // (name, branch, state, classification, liveness_label, branch_safety_msg)
    let mut clean_entries: Vec<(
        String,
        String,
        WorktreeState,
        EntryClassification,
        String,
        String,
    )> = Vec::new();
    let mut skip_entries: Vec<(
        String,
        String,
        WorktreeState,
        EntryClassification,
        String,
        String,
    )> = Vec::new();

    for entry in &entries {
        let state = mgr.detect_state(entry);
        let classification = codeflow_core::worktree::classify_entry(&mgr, entry, threshold);
        let branch = entry.branch.as_deref().unwrap_or("(detached)").to_string();
        let pid_liveness = mgr.pid_liveness(entry);
        let liveness_label = pid_liveness.label().to_string();

        // Check branch safety for entries with existing directories.
        let branch_safety = if Path::new(&entry.path).exists() {
            Some(codeflow_core::worktree::check_branch_safety(Path::new(
                &entry.path,
            )))
        } else {
            None
        };

        // Check branch safety for Clean entries to potentially upgrade to ForceRequired.
        let effective_classification = if classification.action == CleanupAction::Clean {
            if let Some(ref safety) = branch_safety {
                if safety.risk >= codeflow_core::worktree::BranchRisk::High {
                    EntryClassification {
                        action: CleanupAction::ForceRequired,
                        reason: format!("high risk: {} (use --force)", safety.message),
                    }
                } else {
                    if safety.risk == codeflow_core::worktree::BranchRisk::Medium {
                        codeflow_core::diagnostics::warn(
                            "worktree",
                            &format!("{}: {}", entry.name, safety.message),
                        );
                    }
                    classification
                }
            } else {
                classification
            }
        } else {
            classification
        };

        let branch_safety_msg = branch_safety
            .as_ref()
            .map(|s| format!("branch: {}", s.message))
            .unwrap_or_default();

        let info = (
            entry.name.clone(),
            branch,
            state,
            effective_classification,
            liveness_label,
            branch_safety_msg,
        );

        match info.3.action {
            CleanupAction::Clean => clean_entries.push(info),
            CleanupAction::Skip | CleanupAction::ForceRequired => skip_entries.push(info),
        }
    }

    // Phase 1b: Scan for orphaned worktree directories (no registry entry).
    let orphans = scan_orphaned_directories(mgr.base_dir(), &entries);

    // Phase 2: Display analysis.
    if !clean_entries.is_empty() {
        println!("CLEAN ({}):", clean_entries.len());
        for (name, branch, state, classification, liveness_label, _) in &clean_entries {
            println!(
                "  {} ({}) [{}] {} -- {}",
                name, branch, state, liveness_label, classification.reason
            );
        }
    }

    if !skip_entries.is_empty() {
        // Separate ForceRequired from Skip for clearer display.
        let force_required: Vec<_> = skip_entries
            .iter()
            .filter(|(_, _, _, c, _, _)| c.action == CleanupAction::ForceRequired)
            .collect();
        let skipped: Vec<_> = skip_entries
            .iter()
            .filter(|(_, _, _, c, _, _)| c.action == CleanupAction::Skip)
            .collect();

        if !force_required.is_empty() {
            println!("FORCE REQUIRED ({}):", force_required.len());
            for (name, branch, state, classification, liveness_label, branch_safety) in
                &force_required
            {
                println!(
                    "  {} ({}) [{}] PID:{} -- {}",
                    name, branch, state, liveness_label, classification.reason
                );
                if verbose && !branch_safety.is_empty() {
                    println!("    {branch_safety}");
                }
                if verbose {
                    println!("    --force-id {name}");
                }
            }
        }

        if !skipped.is_empty() {
            println!("SKIP ({}):", skipped.len());
            for (name, branch, state, classification, liveness_label, branch_safety) in &skipped {
                println!(
                    "  {} ({}) [{}] PID:{} -- {}",
                    name, branch, state, liveness_label, classification.reason
                );
                if verbose && !branch_safety.is_empty() {
                    println!("    {branch_safety}");
                }
                if verbose {
                    println!("    --force-id {name}");
                }
            }
        }
    }

    if !orphans.is_empty() {
        println!("ORPHANED ({}):", orphans.len());
        for (name, path) in &orphans {
            println!("  {name} (orphaned -- no registry entry)");
            if verbose {
                println!("    path: {}", path.display());
            }
        }
    }

    if clean_entries.is_empty() && orphans.is_empty() && !force_flag {
        println!("no worktrees to clean up");
        return Ok(());
    }

    // Apply --force-id filtering: resolve substring to exact names.
    let mut force_names: Option<Vec<String>> = None;
    let mut force_additions: Vec<String> = Vec::new();
    if force_flag {
        if let Some(ref target) = force_target {
            // Substring match on worktree name in skip list.
            let matches: Vec<_> = skip_entries
                .iter()
                .filter(|(name, ..)| name.contains(target.as_str()))
                .collect();
            if matches.is_empty() {
                // Check if it matches an orphan name.
                let orphan_match = orphans
                    .iter()
                    .any(|(name, _)| name.contains(target.as_str()));
                if orphan_match {
                    println!(
                        "'{target}' is an orphan and will be cleaned automatically (no --force needed)"
                    );
                } else if !skip_entries.is_empty() {
                    anyhow::bail!("no worktree matching '{target}' found in SKIP list");
                }
            } else if matches.len() > 1 {
                anyhow::bail!(
                    "ambiguous --force-id target '{target}': matches {} entries ({})",
                    matches.len(),
                    matches
                        .iter()
                        .map(|(name, ..)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            } else {
                let matched_name = matches[0].0.clone();
                force_additions.push(matched_name.clone());
                force_names = Some(vec![matched_name]);
            }
        }
        // --force all: force_names stays None (applies to all).
    }

    let clean_names: Vec<&str> = clean_entries.iter().map(|(n, ..)| n.as_str()).collect();
    let total_to_clean = clean_names.len() + force_additions.len() + orphans.len();
    if total_to_clean == 0 {
        println!("no worktrees to clean up");
        return Ok(());
    }

    // Phase 3: Dry-run output includes orphans.
    if dry_run {
        println!("\nwould remove {total_to_clean} worktree(s):");
        for name in &clean_names {
            println!("  {name}");
        }
        for name in &force_additions {
            println!("  {name}");
        }
        for (name, _) in &orphans {
            println!("  {name} (orphan)");
        }
        return Ok(());
    }

    // Phase 3b: Confirm (unless --yes).
    if !yes {
        // Count how many force-removed entries have alive PIDs.
        let alive_force_count = if force_flag {
            force_additions
                .iter()
                .filter(|name| {
                    skip_entries
                        .iter()
                        .any(|(n, _, _, _, liveness, _)| n == *name && liveness == "ALIVE")
                })
                .count()
        } else {
            0
        };

        if alive_force_count > 0 {
            eprintln!(
                "\nWARNING: {alive_force_count} worktree(s) have ALIVE processes. \
                 Force-removing may interrupt active sessions."
            );
            eprint!("{total_to_clean} worktree(s) will be FORCE-REMOVED. Proceed? [y/N] ");
        } else {
            eprint!("\n{total_to_clean} worktree(s) will be removed. Proceed? [y/N] ");
        }
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .context("reading confirmation")?;
        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("aborted");
            return Ok(());
        }
    }

    // Phase 4: Execute registered worktree cleanup.
    let opts = CleanupOpts {
        force: force_flag,
        dry_run,
        prune: false,
        interactive,
        keep,
        force_names,
    };

    let removed = mgr
        .cleanup_stale(&opts)
        .context("cleaning up stale worktrees")?;

    // Phase 5: Remove orphaned directories.
    let mut orphan_removed_names: Vec<String> = Vec::new();
    for (name, path) in &orphans {
        match codeflow_core::worktree::cleanup_orphan(project_dir, path, dry_run) {
            Ok(()) => {
                orphan_removed_names.push(name.clone());
            }
            Err(e) => {
                eprintln!("  failed to remove orphan '{name}': {e}");
            }
        }
    }

    // Phase 6: Unified summary output.
    let total_removed = removed.len() + orphan_removed_names.len();
    if total_removed > 0 {
        println!("removed {total_removed} worktree(s):");
        for name in &removed {
            println!("  {name}");
        }
        for name in &orphan_removed_names {
            println!("  {name} (orphan)");
        }
    } else {
        println!("no worktrees removed");
    }

    Ok(())
}

/// Scan `.git-worktrees/` for directories starting with `worktree-` that
/// have no matching registry entry. Skips directories younger than 5 minutes
/// to avoid interfering with concurrent worktree setup.
fn scan_orphaned_directories(
    base_dir: &Path,
    registry_entries: &[codeflow_core::worktree::WorktreeEntry],
) -> Vec<(String, std::path::PathBuf)> {
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return Vec::new();
    };

    let registered_names: HashSet<&str> =
        registry_entries.iter().map(|e| e.name.as_str()).collect();

    let age_threshold = std::time::Duration::from_secs(5 * 60);
    let now = std::time::SystemTime::now();

    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("worktree-") {
            continue;
        }
        if registered_names.contains(name.as_str()) {
            continue;
        }
        // Age guard: skip directories younger than 5 minutes.
        if let Ok(meta) = entry.metadata() {
            if let Ok(created) = meta.created().or_else(|_| meta.modified()) {
                if let Ok(age) = now.duration_since(created) {
                    if age < age_threshold {
                        continue;
                    }
                }
            }
        }
        orphans.push((name, entry.path()));
    }
    orphans
}

fn run_prune(project_dir: &Path, dry_run: bool) -> Result<()> {
    let mut args = vec!["worktree", "prune"];
    if dry_run {
        args.push("--dry-run");
    }

    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(project_dir)
        .output()
        .context("running git worktree prune")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            println!("no stale worktree references to prune");
        }
        Ok(())
    } else {
        anyhow::bail!("git worktree prune failed (exit {})", output.status)
    }
}

fn run_repair(project_dir: &Path, path: Option<&str>) -> Result<()> {
    let wt_path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let env_path = std::env::var("CODEFLOW_WORKTREE_PATH")
                .context("no --path provided and CODEFLOW_WORKTREE_PATH not set")?;
            std::path::PathBuf::from(env_path)
        }
    };

    if !wt_path.exists() {
        anyhow::bail!("worktree path does not exist: {}", wt_path.display());
    }

    codeflow_core::worktree::repair_symlinks(project_dir, &wt_path)
        .context("repairing symlinks")?;

    println!("repaired symlinks for {}", wt_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::worktree::{WorktreeEntry, WorktreeRegistry, write_registry};

    #[test]
    fn test_worktree_command_exists() {
        let _: fn(Option<WorktreeCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_worktree_manager_construction() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let _ = mgr;
    }

    #[test]
    fn test_worktree_list_no_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let result = mgr.list(None);
        if let Ok(entries) = result {
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn test_worktree_detect_state_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/nonexistent/path".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::PendingCleanup,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let state = mgr.detect_state(&entry);
        let state_str = state.to_string();
        assert!(
            state_str == "stale" || state_str == "orphaned",
            "expected stale or orphaned, got {state_str}"
        );
    }

    #[test]
    fn test_worktree_entry_display_format() {
        let entry = WorktreeEntry {
            name: "my-worktree".to_string(),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/something".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let state = mgr.detect_state(&entry);
        let branch = entry.branch.as_deref().unwrap_or("(detached)");
        let output = format!("{} ({}) [{}]", entry.name, branch, state);
        assert!(output.contains("my-worktree"));
        assert!(output.contains("feat/something"));
    }

    #[test]
    fn test_worktree_empty_entries_message() {
        let entries: Vec<WorktreeEntry> = vec![];
        assert!(entries.is_empty());
    }

    #[test]
    fn test_worktree_run_list_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_list(dir.path());
        let _ = result;
    }

    #[test]
    fn test_worktree_run_list_git_init() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let result = run_list(dir.path());
        let _ = result;
    }

    // -- cleanup subcommand tests --

    #[test]
    fn test_cleanup_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_stale_entry_removed() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "stale-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees/stale-wt")
                .to_string_lossy()
                .to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Directory does not exist, so state is Stale -- cleanup should handle it.
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_dry_run_does_not_modify() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "stale-wt".to_string(),
            path: "/nonexistent/stale-wt".to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok());

        // Registry should not have changed (dry run).
        let reg_after = codeflow_core::worktree::read_registry(&reg_path).unwrap();
        assert_eq!(
            reg_after.worktrees[0].status,
            codeflow_core::worktree::WorktreeStatus::Active
        );
    }

    #[test]
    fn test_cleanup_force_includes_orphaned() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");

        // Create a directory that's an orphaned worktree (exists but no .git).
        let wt_dir = dir.path().join(".git-worktrees/orphan-wt");
        std::fs::create_dir_all(&wt_dir).unwrap();

        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "orphan-wt".to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: Some("feat/orphan".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Without force, orphaned should not be removed (dry run to check count).
        let mgr = WorktreeManager::new(dir.path());
        let opts_no_force = CleanupOpts {
            force: false,
            dry_run: true,
            ..CleanupOpts::default()
        };
        let removed_no_force = mgr.cleanup_stale(&opts_no_force).unwrap();
        assert!(
            removed_no_force.is_empty(),
            "orphaned not removed without force"
        );

        // With force, orphaned should be removed.
        let opts_force = CleanupOpts {
            force: true,
            dry_run: true,
            ..CleanupOpts::default()
        };
        let removed_force = mgr.cleanup_stale(&opts_force).unwrap();
        assert_eq!(removed_force.len(), 1);
        assert_eq!(removed_force[0], "orphan-wt");
    }

    // -- CLI subcommand dispatch tests --

    #[test]
    fn test_dispatch_list_default() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        // No subcommand -> None -> defaults to list.
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_dispatch_list_explicit() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "list"]).unwrap();
        assert!(matches!(cli.command, Some(WorktreeCommand::List)));
    }

    #[test]
    fn test_dispatch_cleanup() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                force_id: None,
                dry_run: false,
                yes: false,
                verbose: false,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_with_flags() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "--force", "--dry-run"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: true,
                force_id: None,
                dry_run: true,
                yes: false,
                verbose: false,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_interactive() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "-i"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                force_id: None,
                dry_run: false,
                yes: false,
                verbose: false,
                interactive: true,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_keep_param() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "--keep", "3"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                force_id: None,
                dry_run: false,
                yes: false,
                verbose: false,
                interactive: false,
                keep: Some(3),
            })
        ));
    }

    #[test]
    fn test_dispatch_repair() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "repair"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Repair { path: None })
        ));
    }

    #[test]
    fn test_dispatch_repair_with_path() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "repair", "--path", "/tmp/worktree"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Repair { path: Some(_) })
        ));
    }

    // -- run_list with entries tests --

    #[test]
    fn test_list_shows_branch_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "list-wt".to_string(),
            path: "/nonexistent/list-wt".to_string(),
            branch: Some("feat/listed".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // run_list should succeed and print the entry.
        let result = run_list(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_detached_branch_shows_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "detached-wt".to_string(),
            path: "/nonexistent/detached-wt".to_string(),
            branch: None,
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_list(dir.path());
        assert!(result.is_ok());
    }

    // -- interactive cleanup tests --

    #[test]
    fn test_cleanup_interactive_no_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // Interactive with no entries should say "no worktrees to clean up".
        let result = run_cleanup(dir.path(), false, None, false, true, false, true, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_interactive_shows_stale_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        // Add a stale entry (directory doesn't exist).
        reg.worktrees.push(WorktreeEntry {
            name: "stale-interactive".to_string(),
            path: "/nonexistent/stale-interactive".to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, None, false, true, false, true, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_pending_entries_shown_in_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "pending-cleanup".to_string(),
            path: "/gone/pending".to_string(),
            branch: Some("feat/old".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::PendingCleanup,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // PendingCleanup entries are cleanup candidates (not skipped).
        let result = run_cleanup(dir.path(), false, None, false, true, false, true, None);
        assert!(result.is_ok());
    }

    // -- cleanup output path tests --

    #[test]
    fn test_cleanup_non_interactive_empty_says_no_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_dry_run_stale_shows_would_remove() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "dry-run-stale".to_string(),
            path: dir
                .path()
                .join(".git-worktrees/dry-run-stale")
                .to_string_lossy()
                .to_string(),
            branch: Some("feat/dry".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Dry run should report "would remove" without actually removing.
        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok());
    }

    // -- new 4-phase cleanup tests --

    #[test]
    fn test_cleanup_verbose_shows_details() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "verbose-wt".to_string(),
            path: "/nonexistent/verbose-wt".to_string(),
            branch: Some("feat/verbose".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Verbose + dry run exercises the verbose display path.
        let result = run_cleanup(dir.path(), false, None, true, true, true, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_yes_flag_skips_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "yes-wt".to_string(),
            path: "/nonexistent/yes-wt".to_string(),
            branch: Some("feat/yes".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // --yes should proceed without prompting.
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_force_id_nonexistent_errors() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // --force-id with nonexistent target should still succeed (no skip entries).
        let result = run_cleanup(
            dir.path(),
            false,
            Some("nonexistent"),
            false,
            true,
            false,
            false,
            None,
        );
        // No entries at all -> "no worktrees" message, ok.
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_force_all_with_stale() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        // Add an orphaned entry (directory exists but no .git).
        let wt_dir = dir.path().join(".git-worktrees/orphan-force");
        std::fs::create_dir_all(&wt_dir).unwrap();
        reg.worktrees.push(WorktreeEntry {
            name: "orphan-force".to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: Some("feat/orphan".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // --force-id all with dry run should include orphaned in force list.
        let result = run_cleanup(
            dir.path(),
            true,
            Some("all"),
            true,
            true,
            false,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_force_id_specific_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        // Orphaned worktree (skip candidate).
        let wt_dir = dir.path().join(".git-worktrees/target-wt");
        std::fs::create_dir_all(&wt_dir).unwrap();
        reg.worktrees.push(WorktreeEntry {
            name: "target-wt".to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: Some("feat/target".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // --force-id target-wt with dry run.
        let result = run_cleanup(
            dir.path(),
            true,
            Some("target-wt"),
            true,
            true,
            false,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_no_entries_prints_message() {
        let dir = tempfile::tempdir().unwrap();
        // No registry at all.
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_with_dead_session_entry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        // Entry with dead lead_pid and directory exists with .git.
        let wt_dir = dir.path().join(".git-worktrees/dead-session");
        std::fs::create_dir_all(wt_dir.join(".git")).unwrap();
        reg.worktrees.push(WorktreeEntry {
            name: "dead-session".to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: Some("feat/dead".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-dead00000000000000000000".to_string()),
            task_id: None,
            source: None,
            lead_pid: Some(999_999_999),
        });
        write_registry(&reg_path, &reg).unwrap();

        // Dead session should be in CLEAN list, dry run.
        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_pending_cleanup_in_clean_list() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "pending-entry".to_string(),
            path: "/gone/pending".to_string(),
            branch: Some("feat/pending".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::PendingCleanup,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // PendingCleanup entries should go to CLEAN list.
        let result = run_cleanup(dir.path(), false, None, true, true, true, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dispatch_cleanup_yes_flag() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "-y"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                force_id: None,
                dry_run: false,
                yes: true,
                verbose: false,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_verbose_flag() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "-v"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                force_id: None,
                dry_run: false,
                yes: false,
                verbose: true,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_force_id() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli =
            TestCli::try_parse_from(["test", "cleanup", "--force-id", "my-worktree"]).unwrap();
        match &cli.command {
            Some(WorktreeCommand::Cleanup { force_id, .. }) => {
                assert_eq!(force_id.as_deref(), Some("my-worktree"));
            }
            other => panic!("expected Cleanup, got {other:?}"),
        }
    }

    // -- Orphan scanning tests --

    #[test]
    fn test_scan_orphaned_directories_finds_unregistered() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create an orphaned worktree dir (old enough).
        let orphan = base.join("worktree-orphan123");
        std::fs::create_dir_all(&orphan).unwrap();
        // Touch with old mtime via filetime.
        let old_time = filetime::FileTime::from_unix_time(1_000_000_000, 0);
        let _ = filetime::set_file_mtime(&orphan, old_time);

        // Create a registered worktree dir.
        let registered = base.join("worktree-registered");
        std::fs::create_dir_all(&registered).unwrap();
        let _ = filetime::set_file_mtime(&registered, old_time);

        // Non-worktree dir (doesn't start with "worktree-").
        let other = base.join("some-other-dir");
        std::fs::create_dir_all(&other).unwrap();

        let registry_entries = vec![WorktreeEntry {
            name: "worktree-registered".to_string(),
            path: registered.to_string_lossy().to_string(),
            branch: None,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        }];

        let orphans = scan_orphaned_directories(&base, &registry_entries);
        assert_eq!(orphans.len(), 1, "should find exactly one orphan");
        assert_eq!(orphans[0].0, "worktree-orphan123");
    }

    #[test]
    fn test_scan_orphaned_directories_age_guard_skips_new() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create a freshly-created orphan dir (just created = < 5 min old).
        let orphan = base.join("worktree-fresh");
        std::fs::create_dir_all(&orphan).unwrap();
        // Don't set mtime — it was just created, so it's < 5 min old.

        let orphans = scan_orphaned_directories(&base, &[]);
        assert!(
            orphans.is_empty(),
            "freshly created directory should be skipped by age guard"
        );
    }

    #[test]
    fn test_scan_orphaned_no_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Don't create .git-worktrees — scan should return empty, not error.
        let nonexistent = dir.path().join(".git-worktrees");
        let orphans = scan_orphaned_directories(&nonexistent, &[]);
        assert!(orphans.is_empty());
    }

    // -- orphan scan independence and force override tests --

    #[test]
    fn test_cleanup_finds_orphans_with_only_alive_entries() {
        // Registry has 1 active (alive) entry + 1 orphan dir (>5min old).
        // Verify orphan found and alive entry untouched.
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create an active worktree directory (with .git so detect_state=Active).
        let alive_dir = base.join("worktree-alive");
        std::fs::create_dir_all(&alive_dir).unwrap();
        std::fs::write(alive_dir.join(".git"), "gitdir: /somewhere").unwrap();

        // Create an orphan directory (>5min old, no registry entry).
        let orphan_dir = base.join("worktree-orphan-old");
        std::fs::create_dir_all(&orphan_dir).unwrap();
        // Set mtime to 10 minutes ago to pass the 5-minute age guard.
        let ten_min_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 60);
        filetime::set_file_mtime(
            &orphan_dir,
            filetime::FileTime::from_system_time(ten_min_ago),
        )
        .unwrap();

        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-alive".to_string(),
            path: alive_dir.to_string_lossy().to_string(),
            branch: Some("feat/alive".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // dry_run so we can check detection without actual deletion.
        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok(), "cleanup should succeed");

        // The alive entry should NOT be cleaned, and the orphan SHOULD be detected.
        // Verify the orphan directory still exists (dry run).
        assert!(
            orphan_dir.exists(),
            "orphan dir should still exist (dry run)"
        );
        // Verify the alive entry's directory is untouched.
        assert!(alive_dir.exists(), "alive dir should be untouched");
    }

    #[test]
    fn test_cleanup_finds_orphans_with_empty_registry() {
        // Registry empty, 2 orphan dirs (>5min old). Verify both found as orphans.
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create 2 orphan directories with old mtimes.
        let orphan1 = base.join("worktree-orphan1");
        let orphan2 = base.join("worktree-orphan2");
        std::fs::create_dir_all(&orphan1).unwrap();
        std::fs::create_dir_all(&orphan2).unwrap();
        let ten_min_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 60);
        filetime::set_file_mtime(&orphan1, filetime::FileTime::from_system_time(ten_min_ago))
            .unwrap();
        filetime::set_file_mtime(&orphan2, filetime::FileTime::from_system_time(ten_min_ago))
            .unwrap();

        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // dry_run: should detect both orphans despite empty registry.
        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok(), "cleanup should succeed with empty registry");

        // Both orphans should still exist (dry run).
        assert!(orphan1.exists(), "orphan1 should still exist (dry run)");
        assert!(orphan2.exists(), "orphan2 should still exist (dry run)");
    }

    #[test]
    fn test_cleanup_force_all_cleans_alive_and_orphans() {
        // 1 active (alive) entry + 1 orphan. --force all --yes.
        // Verify both cleaned (or marked for cleaning in dry_run).
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create alive entry (Active state, session alive is unknown/not checked).
        let alive_dir = base.join("worktree-alive-force");
        std::fs::create_dir_all(&alive_dir).unwrap();
        std::fs::write(alive_dir.join(".git"), "gitdir: /somewhere").unwrap();

        // Create orphan directory.
        let orphan_dir = base.join("worktree-orphan-force");
        std::fs::create_dir_all(&orphan_dir).unwrap();
        let ten_min_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 60);
        filetime::set_file_mtime(
            &orphan_dir,
            filetime::FileTime::from_system_time(ten_min_ago),
        )
        .unwrap();

        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-alive-force".to_string(),
            path: alive_dir.to_string_lossy().to_string(),
            branch: Some("feat/alive".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // --force (all) with dry_run to verify both would be cleaned.
        let result = run_cleanup(dir.path(), true, None, true, true, false, false, None);
        assert!(result.is_ok(), "force-all cleanup should succeed");
    }

    #[test]
    fn test_cleanup_dry_run_shows_orphans() {
        // 1 active (alive) entry + 1 orphan. --dry-run.
        // Verify output includes orphan, nothing deleted.
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create alive entry directory.
        let alive_dir = base.join("worktree-dryrun-alive");
        std::fs::create_dir_all(&alive_dir).unwrap();
        std::fs::write(alive_dir.join(".git"), "gitdir: /somewhere").unwrap();

        // Create orphan directory (old enough).
        let orphan_dir = base.join("worktree-dryrun-orphan");
        std::fs::create_dir_all(&orphan_dir).unwrap();
        let ten_min_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 60);
        filetime::set_file_mtime(
            &orphan_dir,
            filetime::FileTime::from_system_time(ten_min_ago),
        )
        .unwrap();

        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-dryrun-alive".to_string(),
            path: alive_dir.to_string_lossy().to_string(),
            branch: Some("feat/dryrun".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // dry_run should show orphan without deleting.
        let result = run_cleanup(dir.path(), false, None, true, true, false, false, None);
        assert!(result.is_ok(), "dry_run cleanup should succeed");

        // Orphan directory should still exist (dry run).
        assert!(
            orphan_dir.exists(),
            "orphan dir should not be deleted in dry_run"
        );
        // Alive directory should still exist.
        assert!(
            alive_dir.exists(),
            "alive dir should not be deleted in dry_run"
        );
    }

    #[test]
    fn test_cleanup_no_orphans_no_cleanable_returns_clean() {
        // 1 active (alive) entry, no orphans. Verify "no worktrees to clean up".
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create alive entry directory.
        let alive_dir = base.join("worktree-clean-alive");
        std::fs::create_dir_all(&alive_dir).unwrap();
        std::fs::write(alive_dir.join(".git"), "gitdir: /somewhere").unwrap();

        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-clean-alive".to_string(),
            path: alive_dir.to_string_lossy().to_string(),
            branch: Some("feat/clean".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // No orphans, no cleanable entries. Should return Ok with
        // "no worktrees to clean up".
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(
            result.is_ok(),
            "cleanup should succeed with alive entry and no orphans"
        );
    }

    #[test]
    fn test_cleanup_orphan_age_guard_skips_young_dirs() {
        // 1 young dir (<5min), no registry. Verify NOT treated as orphan.
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let base = dir.path().join(".git-worktrees");
        std::fs::create_dir_all(&base).unwrap();

        // Create a freshly-created orphan dir (< 5 min old, should be skipped).
        let young_dir = base.join("worktree-young-orphan");
        std::fs::create_dir_all(&young_dir).unwrap();
        // Don't set mtime — it was just created so age < 5 min.

        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // Should return "no worktrees to clean up" because young dir is
        // skipped by age guard.
        let result = run_cleanup(dir.path(), false, None, false, true, false, false, None);
        assert!(result.is_ok(), "cleanup should succeed");

        // The young directory should still exist.
        assert!(young_dir.exists(), "young dir should not be touched");
    }
}
