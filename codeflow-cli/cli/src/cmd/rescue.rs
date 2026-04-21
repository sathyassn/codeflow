//! `codeflow rescue` subcommand.
//!
//! Inspect, apply, prune, and pin rescue patch bundles written by the
//! worktree cleanup subsystem (see `codeflow-core/src/worktree/cleanup.rs`
//! and `codeflow-core/src/autorun/rescue.rs`).
//!
//! **INF-TSK-049-001 batch 2 (AC #24-#33)**: bundles live under
//! `dirs::cache_dir()/codeflow/rescue/` (XDG cache root), NEVER inside any
//! git working tree. The path resolves via
//! [`codeflow_core::autorun::rescue::xdg_rescue_root`].
//!
//! Each bundle directory contains:
//! - `working.patch`     — output of `git diff HEAD`
//! - `staged.patch`      — output of `git diff --cached`
//! - `untracked.tar.gz`  — tarball of untracked files (100 MB cap)
//! - `metadata.json`     — session id, branch, HEAD sha, timestamp, reason,
//!   skipped files, worktree path
//! - `.pinned` (optional) — when present, exempts the bundle from
//!   age-based auto-prune

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use codeflow_core::autorun::rescue as rescue_core;

use crate::helpers;

#[derive(Debug, Subcommand)]
pub enum RescueCommand {
    /// List available rescue bundles.
    List,
    /// Show metadata and a diffstat for a bundle by ID.
    Show {
        /// Bundle ID (the directory name under the XDG rescue root).
        id: String,
    },
    /// Apply a bundle's patches to a working tree.
    Apply {
        /// Bundle ID.
        id: String,
        /// Target repo to apply into. Defaults to the current project.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Ignore a HEAD sha mismatch between the bundle and the current branch.
        #[arg(long)]
        force_base: bool,
    },
    /// Delete bundles. Alias for `clean`.
    Drop {
        /// Bundle ID (omit with `--all` or `--older-than`).
        id: Option<String>,
        /// Delete every bundle.
        #[arg(long)]
        all: bool,
        /// Delete bundles older than N days (positive integer).
        #[arg(long, value_name = "N")]
        older_than: Option<u32>,
    },
    /// Delete bundles. Alias for `drop`.
    Clean {
        /// Bundle ID (omit with `--all` or `--older-than`).
        id: Option<String>,
        /// Delete every bundle.
        #[arg(long)]
        all: bool,
        /// Delete bundles older than N days (positive integer).
        #[arg(long, value_name = "N")]
        older_than: Option<u32>,
    },
    /// Pin a bundle so auto-prune never deletes it.
    Pin {
        /// Bundle ID.
        id: String,
    },
    /// Unpin a bundle so auto-prune may delete it once retention elapses.
    Unpin {
        /// Bundle ID.
        id: String,
    },
}

pub fn run(command: Option<RescueCommand>) -> Result<()> {
    match command.unwrap_or(RescueCommand::List) {
        RescueCommand::List => list(),
        RescueCommand::Show { id } => show(&id),
        RescueCommand::Apply {
            id,
            repo,
            force_base,
        } => {
            let repo = match repo {
                Some(r) => r,
                None => helpers::detect_project_dir()?,
            };
            apply(&repo, &id, force_base)
        }
        RescueCommand::Drop {
            id,
            all,
            older_than,
        }
        | RescueCommand::Clean {
            id,
            all,
            older_than,
        } => clean_bundles(id.as_deref(), all, older_than),
        RescueCommand::Pin { id } => pin_bundle(&id, true),
        RescueCommand::Unpin { id } => pin_bundle(&id, false),
    }
}

// ---------------------------------------------------------------------------
// Subcommand: list
// ---------------------------------------------------------------------------

fn list() -> Result<()> {
    let bundles = rescue_core::list_bundles().context("list rescue bundles")?;
    if bundles.is_empty() {
        println!("(no rescue bundles)");
        return Ok(());
    }
    println!(
        "{:<50} {:<6} {:<30} {:<25} {:>10} PATH",
        "ID", "PINNED", "BRANCH", "CREATED", "SIZE"
    );
    for b in &bundles {
        println!(
            "{:<50} {:<6} {:<30} {:<25} {:>10} {}",
            b.id,
            if b.pinned { "yes" } else { "no" },
            if b.branch.is_empty() { "-" } else { &b.branch },
            if b.created_at.is_empty() {
                "-"
            } else {
                &b.created_at
            },
            human_size(b.size_bytes),
            b.path.display(),
        );
    }
    Ok(())
}

fn human_size(n: u64) -> String {
    const K: u64 = 1024;
    if n >= K * K {
        format!("{:.1}MB", n as f64 / (K * K) as f64)
    } else if n >= K {
        format!("{:.1}KB", n as f64 / K as f64)
    } else {
        format!("{n}B")
    }
}

// ---------------------------------------------------------------------------
// Subcommand: show
// ---------------------------------------------------------------------------

fn show(id: &str) -> Result<()> {
    let bundle = resolve_bundle(id)?;
    let metadata = read_metadata(&bundle)?;
    println!("Metadata:");
    println!("{}", serde_json::to_string_pretty(&metadata)?);

    println!("\nDiffstat (working + staged):");
    let mut args: Vec<&str> = vec!["apply", "--stat"];
    let working = bundle.join("working.patch");
    let staged = bundle.join("staged.patch");
    let working_str = working.to_string_lossy();
    let staged_str = staged.to_string_lossy();
    if working.exists() && file_non_empty(&working) {
        args.push(working_str.as_ref());
    }
    if staged.exists() && file_non_empty(&staged) {
        args.push(staged_str.as_ref());
    }
    if args.len() == 2 {
        println!("(no diff content)");
    } else {
        let status = Command::new("git").args(&args).status()?;
        if !status.success() {
            eprintln!("(diffstat exited with non-zero status — patches may have errors)");
        }
    }

    // Spec: also print the head ~80 lines of working.patch.
    if working.exists() && file_non_empty(&working) {
        println!("\nworking.patch (first 80 lines):");
        let s = fs::read_to_string(&working).unwrap_or_default();
        for (i, line) in s.lines().enumerate() {
            if i >= 80 {
                println!("... (truncated)");
                break;
            }
            println!("{line}");
        }
    }
    Ok(())
}

fn file_non_empty(p: &Path) -> bool {
    fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Subcommand: apply
// ---------------------------------------------------------------------------

fn apply(project_dir: &Path, id: &str, force_base: bool) -> Result<()> {
    let bundle = resolve_bundle(id)?;

    // Working tree must be clean.
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_dir)
        .output()?;
    if !status.stdout.is_empty() {
        bail!(
            "working tree is not clean — commit or stash first before applying a rescue bundle\n\
             (dirty files reported by `git status --porcelain`)"
        );
    }

    // HEAD sha check.
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()?;
    let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

    let meta = read_metadata(&bundle)?;
    let bundle_head = meta
        .get("head_sha")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !bundle_head.is_empty() && bundle_head != head_sha && !force_base {
        bail!(
            "HEAD sha differs from bundle's recorded base\n  bundle: {bundle_head}\n  HEAD:   {head_sha}\n\
             Re-run with --force-base to apply anyway."
        );
    }

    // Apply staged, then working. `--3way` performs a 3-way merge on conflict
    // but does NOT auto-resolve; conflicts produce a non-zero exit code.
    for (label, name) in [("staged", "staged.patch"), ("working", "working.patch")] {
        let p = bundle.join(name);
        if !p.exists() || !file_non_empty(&p) {
            continue;
        }
        let out = Command::new("git")
            .args(["apply", "--3way", p.to_string_lossy().as_ref()])
            .current_dir(project_dir)
            .output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!(
                "`git apply --3way` failed on {label} patch — conflicts likely. \
                 Resolve manually or --force-base onto a matching HEAD.\nstderr: {stderr}"
            );
        }
    }

    // Restore untracked files from the tarball, but only if the target path
    // does not already exist in the working tree.
    let tar_path = bundle.join("untracked.tar.gz");
    if tar_path.exists() && file_non_empty(&tar_path) {
        restore_untracked(&tar_path, project_dir)?;
    }

    println!("Applied rescue bundle: {}", bundle.display());
    Ok(())
}

/// Return true when any component of `path` is unsafe for use as a
/// relative destination under a project root.
fn has_traversal_component(path: &Path) -> bool {
    use std::path::Component;
    for comp in path.components() {
        match comp {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return true,
            Component::Normal(s) => {
                if s.to_string_lossy().contains('\0') {
                    return true;
                }
            }
            Component::CurDir => {}
        }
    }
    false
}

fn restore_untracked(tar_path: &Path, project_dir: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    let f = fs::File::open(tar_path).with_context(|| format!("open {}", tar_path.display()))?;
    let dec = GzDecoder::new(f);
    let mut ar = tar::Archive::new(dec);

    let project_root = fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());

    for entry in ar.entries()? {
        let mut entry = entry?;
        let path_in_tar = entry.path()?.to_path_buf();

        if path_in_tar.is_absolute() {
            eprintln!(
                "skip {}: absolute path rejected (tar entry must be relative)",
                path_in_tar.display()
            );
            continue;
        }

        if has_traversal_component(&path_in_tar) {
            eprintln!(
                "skip {}: path traversal rejected (contains '..' or unsafe component)",
                path_in_tar.display()
            );
            continue;
        }

        let target = project_root.join(&path_in_tar);

        if !target.starts_with(&project_root) {
            eprintln!(
                "skip {}: path traversal rejected (would escape project_dir)",
                path_in_tar.display()
            );
            continue;
        }

        if target.exists() {
            eprintln!(
                "skip {}: already exists in working tree (not overwriting)",
                path_in_tar.display()
            );
            continue;
        }

        if let Some(parent) = target.parent() {
            if fs::create_dir_all(parent).is_err() {
                eprintln!("skip {}: cannot create parent dir", path_in_tar.display());
                continue;
            }
            match fs::canonicalize(parent) {
                Ok(canonical_parent) => {
                    if !canonical_parent.starts_with(&project_root) {
                        eprintln!(
                            "skip {}: symlinked parent escapes project_root",
                            path_in_tar.display()
                        );
                        continue;
                    }
                }
                Err(_) => {
                    eprintln!(
                        "skip {}: cannot canonicalize parent dir",
                        path_in_tar.display()
                    );
                    continue;
                }
            }
        }
        entry.unpack(&target)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: drop / clean
// ---------------------------------------------------------------------------

fn clean_bundles(id: Option<&str>, all: bool, older_than: Option<u32>) -> Result<()> {
    let chosen_count = [id.is_some(), all, older_than.is_some()]
        .iter()
        .filter(|b| **b)
        .count();
    if chosen_count != 1 {
        bail!("clean requires exactly one of <id>, --all, --older-than N");
    }

    let root = rescue_core::xdg_rescue_root().context("resolve xdg rescue root")?;
    if !root.is_dir() {
        println!("(no rescue bundles)");
        return Ok(());
    }

    if let Some(id) = id {
        let bundle = resolve_bundle(id)?;
        fs::remove_dir_all(&bundle).with_context(|| format!("remove {}", bundle.display()))?;
        println!("dropped: {}", bundle.display());
        return Ok(());
    }

    if all {
        let mut dropped = 0;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Skip dotfiles like .migration-v1-complete and .last-scan
            // (those are files; defensive against future directory markers).
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with('.'))
            {
                continue;
            }
            // Pinned bundles are exempt from --all.
            if path.join(".pinned").exists() {
                continue;
            }
            fs::remove_dir_all(&path)?;
            dropped += 1;
        }
        println!("dropped {dropped} bundle(s) (pinned bundles preserved)");
        return Ok(());
    }

    if let Some(days) = older_than {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
        let mut dropped = 0;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with('.'))
            {
                continue;
            }
            if path.join(".pinned").exists() {
                continue;
            }
            let Ok(meta) = read_metadata(&path) else {
                continue;
            };
            let Some(ts) = meta.get("timestamp_rfc3339").and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts) else {
                continue;
            };
            if ts.with_timezone(&chrono::Utc) < cutoff {
                fs::remove_dir_all(&path)?;
                dropped += 1;
            }
        }
        println!("dropped {dropped} bundle(s) older than {days}d (pinned preserved)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: pin / unpin
// ---------------------------------------------------------------------------

fn pin_bundle(id: &str, pin: bool) -> Result<()> {
    let bundle = resolve_bundle(id)?;
    let marker = bundle.join(".pinned");
    if pin {
        if !marker.exists() {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&marker)
                .with_context(|| format!("create {}", marker.display()))?;
        }
        println!("pinned: {}", bundle.display());
    } else {
        if marker.exists() {
            fs::remove_file(&marker).with_context(|| format!("remove {}", marker.display()))?;
        }
        println!("unpinned: {}", bundle.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_bundle(id: &str) -> Result<PathBuf> {
    if id.is_empty() {
        bail!("bundle id cannot be empty");
    }
    if id.contains('/') || id.contains("..") || id.contains('\\') || id.contains('\0') {
        bail!("invalid bundle id: {id}");
    }
    let root = rescue_core::xdg_rescue_root().context("resolve xdg rescue root")?;
    let p = root.join(id);
    if !p.is_dir() {
        return Err(anyhow!("bundle not found: {}", p.display()));
    }
    Ok(p)
}

fn read_metadata(bundle: &Path) -> Result<serde_json::Value> {
    let p = bundle.join("metadata.json");
    let s = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    Ok(serde_json::from_str(&s)?)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that mutate `HOME` / `XDG_CACHE_HOME`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn redirect_xdg_cache(td: &tempfile::TempDir) {
        // SAFETY: ENV_LOCK serialises mutators.
        unsafe {
            std::env::set_var("HOME", td.path());
            std::env::set_var("XDG_CACHE_HOME", td.path().join(".cache"));
        }
    }

    fn make_bundle(
        root: &Path,
        id: &str,
        timestamp: &str,
        head_sha: &str,
        working: &str,
        staged: &str,
        untracked: Option<(&str, &str)>,
    ) -> PathBuf {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("working.patch"), working).unwrap();
        fs::write(dir.join("staged.patch"), staged).unwrap();
        use flate2::Compression;
        use flate2::write::GzEncoder;
        let f = fs::File::create(dir.join("untracked.tar.gz")).unwrap();
        let enc = GzEncoder::new(f, Compression::default());
        let mut tb = tar::Builder::new(enc);
        if let Some((name, contents)) = untracked {
            let tmp_src = tempfile::tempdir().unwrap();
            let src = tmp_src.path().join(name);
            fs::write(&src, contents).unwrap();
            tb.append_path_with_name(&src, name).unwrap();
        }
        tb.finish().unwrap();
        let metadata = serde_json::json!({
            "session_id": id,
            "branch": "feat/x",
            "head_sha": head_sha,
            "timestamp_rfc3339": timestamp,
            "reason": "unit-test",
            "skipped_large_files": [],
            "worktree_path": "/tmp/fake-wt",
        });
        fs::write(
            dir.join("metadata.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn test_list_empty() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(list().is_ok());
    }

    #[test]
    fn test_list_multiple() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(
            &root,
            "ses-a-20260101-000000",
            "2026-01-01T00:00:00Z",
            "deadbeef",
            "",
            "",
            None,
        );
        make_bundle(
            &root,
            "ses-b-20260102-000000",
            "2026-01-02T00:00:00Z",
            "cafef00d",
            "",
            "",
            None,
        );
        assert!(list().is_ok());
    }

    #[test]
    fn test_show_missing_bundle() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let res = show("does-not-exist");
        assert!(res.is_err());
    }

    #[test]
    fn test_show_valid_bundle() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        let id = "ses-x-20260101-000000";
        make_bundle(&root, id, "2026-01-01T00:00:00Z", "abc123", "", "", None);
        let res = show(id);
        assert!(res.is_ok(), "show failed: {res:?}");
    }

    #[test]
    fn test_resolve_bundle_rejects_traversal() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(resolve_bundle("../../etc/passwd").is_err());
        assert!(resolve_bundle("ses/../evil").is_err());
        assert!(resolve_bundle("a\\b").is_err());
        assert!(resolve_bundle("").is_err());
    }

    #[test]
    fn test_clean_all() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "a", "t1", "s1", "", "", None);
        make_bundle(&root, "b", "t2", "s2", "", "", None);
        clean_bundles(None, true, None).unwrap();
        let count = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_clean_all_preserves_pinned() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "a", "t1", "s1", "", "", None);
        let pinned = make_bundle(&root, "p", "t2", "s2", "", "", None);
        fs::write(pinned.join(".pinned"), "").unwrap();
        clean_bundles(None, true, None).unwrap();
        assert!(!root.join("a").exists());
        assert!(root.join("p").exists(), "pinned bundle must survive --all");
    }

    #[test]
    fn test_clean_by_id() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "ses-one", "t", "s", "", "", None);
        make_bundle(&root, "ses-two", "t", "s", "", "", None);
        clean_bundles(Some("ses-one"), false, None).unwrap();
        assert!(!root.join("ses-one").exists());
        assert!(root.join("ses-two").exists());
    }

    #[test]
    fn test_clean_older_than() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "old", "2020-01-01T00:00:00Z", "h", "", "", None);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_bundle(&root, "recent", &now, "h", "", "", None);
        clean_bundles(None, false, Some(30)).unwrap();
        assert!(!root.join("old").exists());
        assert!(root.join("recent").exists());
    }

    #[test]
    fn test_clean_older_than_preserves_pinned() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        let old_pinned = make_bundle(&root, "old-p", "2020-01-01T00:00:00Z", "h", "", "", None);
        fs::write(old_pinned.join(".pinned"), "").unwrap();
        clean_bundles(None, false, Some(30)).unwrap();
        assert!(
            root.join("old-p").exists(),
            "pinned bundle must survive --older-than"
        );
    }

    #[test]
    fn test_clean_requires_exactly_one_flag() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        // 0 flags.
        let r = clean_bundles(None, false, None);
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("exactly one"));
        // id + all.
        assert!(clean_bundles(Some("x"), true, None).is_err());
        // id + --older-than.
        assert!(clean_bundles(Some("x"), false, Some(7)).is_err());
        // --all + --older-than.
        assert!(clean_bundles(None, true, Some(7)).is_err());
        // all three.
        assert!(clean_bundles(Some("x"), true, Some(7)).is_err());
    }

    #[test]
    fn test_pin_and_unpin_bundle() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "ses-pin", "t", "h", "", "", None);
        // Initially unpinned.
        assert!(!root.join("ses-pin/.pinned").exists());
        pin_bundle("ses-pin", true).unwrap();
        assert!(root.join("ses-pin/.pinned").exists());
        // Idempotent.
        pin_bundle("ses-pin", true).unwrap();
        assert!(root.join("ses-pin/.pinned").exists());
        // Unpin.
        pin_bundle("ses-pin", false).unwrap();
        assert!(!root.join("ses-pin/.pinned").exists());
        // Idempotent.
        pin_bundle("ses-pin", false).unwrap();
        assert!(!root.join("ses-pin/.pinned").exists());
    }

    #[test]
    fn test_pin_missing_bundle_errors() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(pin_bundle("does-not-exist", true).is_err());
    }

    #[test]
    fn test_apply_requires_clean_tree() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        let project = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(project.path())
            .status()
            .unwrap();
        fs::write(project.path().join("init.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(project.path())
            .status()
            .unwrap();

        make_bundle(&root, "ses-app1", "t", "deadbeef", "", "", None);
        // Dirty it.
        fs::write(project.path().join("init.txt"), "b").unwrap();
        let res = apply(project.path(), "ses-app1", false);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("working tree"));
    }

    #[test]
    fn test_has_traversal_component_detects_parent() {
        assert!(has_traversal_component(Path::new("../evil")));
        assert!(has_traversal_component(Path::new("a/../b")));
        assert!(has_traversal_component(Path::new("a/b/..")));
    }

    #[test]
    fn test_has_traversal_component_accepts_clean() {
        assert!(!has_traversal_component(Path::new("a.txt")));
        assert!(!has_traversal_component(Path::new("sub/dir/file")));
        assert!(!has_traversal_component(Path::new("./a")));
    }

    #[test]
    fn test_has_traversal_component_rejects_null_byte() {
        let p = std::path::PathBuf::from("a\0b");
        assert!(has_traversal_component(&p));
    }

    // -- run() dispatch tests --

    #[test]
    fn test_run_defaults_to_list_when_no_command() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(run(None).is_ok());
    }

    #[test]
    fn test_run_list_command() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(run(Some(RescueCommand::List)).is_ok());
    }

    #[test]
    fn test_run_show_command_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        assert!(run(Some(RescueCommand::Show { id: "nope".into() })).is_err());
    }

    #[test]
    fn test_run_drop_delegates_to_clean() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "drop-me", "t", "h", "", "", None);
        assert!(
            run(Some(RescueCommand::Drop {
                id: Some("drop-me".into()),
                all: false,
                older_than: None,
            }))
            .is_ok()
        );
        assert!(!root.join("drop-me").exists());
    }

    #[test]
    fn test_run_clean_delegates_to_clean() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "clean-me", "t", "h", "", "", None);
        assert!(
            run(Some(RescueCommand::Clean {
                id: Some("clean-me".into()),
                all: false,
                older_than: None,
            }))
            .is_ok()
        );
        assert!(!root.join("clean-me").exists());
    }

    #[test]
    fn test_run_pin_command() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        make_bundle(&root, "pinit", "t", "h", "", "", None);
        assert!(run(Some(RescueCommand::Pin { id: "pinit".into() })).is_ok());
        assert!(root.join("pinit/.pinned").exists());
    }

    #[test]
    fn test_run_unpin_command() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        let b = make_bundle(&root, "unpinit", "t", "h", "", "", None);
        fs::write(b.join(".pinned"), "").unwrap();
        assert!(
            run(Some(RescueCommand::Unpin {
                id: "unpinit".into()
            }))
            .is_ok()
        );
        assert!(!root.join("unpinit/.pinned").exists());
    }

    // -- human_size tests --

    #[test]
    fn test_human_size_bytes() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1023), "1023B");
    }

    #[test]
    fn test_human_size_kilobytes() {
        assert_eq!(human_size(1024), "1.0KB");
        assert_eq!(human_size(1500), "1.5KB");
        assert_eq!(human_size(1024 * 1023), "1023.0KB");
    }

    #[test]
    fn test_human_size_megabytes() {
        assert_eq!(human_size(1024 * 1024), "1.0MB");
        assert_eq!(human_size(1024 * 1024 * 5), "5.0MB");
    }

    // -- apply happy path (clean tree, matching HEAD) --

    /// INF-TSK-049-001 batch 2: `apply` full happy path — clean tree + HEAD
    /// matches bundle + staged patch applied cleanly. Verifies the file
    /// ends up modified as the patch describes.
    #[test]
    fn test_apply_happy_path_with_matching_head() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();

        let project = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", "--initial-branch=main"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(project.path())
            .status()
            .unwrap();
        fs::write(project.path().join("hello.txt"), "hello\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(project.path())
            .status()
            .unwrap();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(project.path())
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

        // Build a valid git patch that turns "hello\n" into "hello\nworld\n".
        // Generate it by dirtying the tree, running git diff, then cleaning.
        fs::write(project.path().join("hello.txt"), "hello\nworld\n").unwrap();
        let diff_out = Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(project.path())
            .output()
            .unwrap();
        let patch = String::from_utf8_lossy(&diff_out.stdout).into_owned();
        // Restore clean tree.
        fs::write(project.path().join("hello.txt"), "hello\n").unwrap();

        make_bundle(&root, "ses-happy", "t", &head_sha, &patch, "", None);

        let res = apply(project.path(), "ses-happy", false);
        assert!(res.is_ok(), "apply failed: {res:?}");

        let final_contents = fs::read_to_string(project.path().join("hello.txt")).unwrap();
        assert!(
            final_contents.contains("world"),
            "working patch did not apply; got: {final_contents:?}"
        );
    }

    /// Edge: apply with an untracked tarball entry should restore the file
    /// (was not present in working tree).
    #[test]
    fn test_apply_restores_untracked_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();

        let project = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", "--initial-branch=main"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(project.path())
            .status()
            .unwrap();
        fs::write(project.path().join("seed.txt"), "seed\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(project.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(project.path())
            .status()
            .unwrap();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(project.path())
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

        make_bundle(
            &root,
            "ses-restore",
            "t",
            &head_sha,
            "",
            "",
            Some(("new.txt", "restored-content")),
        );

        let res = apply(project.path(), "ses-restore", false);
        assert!(res.is_ok(), "apply failed: {res:?}");
        assert_eq!(
            fs::read_to_string(project.path().join("new.txt")).unwrap(),
            "restored-content"
        );
    }

    #[test]
    fn test_show_with_populated_patches() {
        let _g = ENV_LOCK.lock().unwrap();
        let td = tempfile::tempdir().unwrap();
        redirect_xdg_cache(&td);
        let root = rescue_core::xdg_rescue_root().unwrap();
        // Build a valid unified-diff patch so the "git apply --stat" path
        // runs successfully and exercises the non-empty args branch.
        let patch = "--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-old line\n+new line\n";
        make_bundle(
            &root,
            "ses-shown",
            "2026-04-21T00:00:00Z",
            "h",
            patch,
            "",
            None,
        );
        assert!(show("ses-shown").is_ok());
    }
}
