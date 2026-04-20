//! `codeflow rescue` subcommand.
//!
//! Inspect, apply, and drop rescue patch bundles written by the worktree
//! cleanup subsystem (see `codeflow-core/src/worktree/cleanup.rs`).
//!
//! A rescue bundle lives at
//! `<project_root>/.state/rescue/{sid}-{YYYYMMDD-HHMMSS}-{pid}/`
//! and contains:
//! - `working.patch`  — output of `git diff HEAD`
//! - `staged.patch`   — output of `git diff --cached`
//! - `untracked.tar.gz` — tarball of untracked files (100 MB cap)
//! - `metadata.json`  — session id, branch, HEAD sha, timestamp, reason,
//!   skipped files, worktree path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;

use crate::helpers;

#[derive(Debug, Subcommand)]
pub enum RescueCommand {
    /// List available rescue bundles.
    List,
    /// Show metadata and a diffstat for a bundle by ID.
    Show {
        /// Bundle ID (the directory name under `.state/rescue/`).
        id: String,
    },
    /// Apply a bundle's patches to the current working tree.
    Apply {
        /// Bundle ID.
        id: String,
        /// Ignore a HEAD sha mismatch between the bundle and the current branch.
        #[arg(long)]
        force_base: bool,
    },
    /// Delete a bundle (or all bundles).
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
}

pub fn run(command: Option<RescueCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match command.unwrap_or(RescueCommand::List) {
        RescueCommand::List => list(&project_dir),
        RescueCommand::Show { id } => show(&project_dir, &id),
        RescueCommand::Apply { id, force_base } => apply(&project_dir, &id, force_base),
        RescueCommand::Drop {
            id,
            all,
            older_than,
        } => drop_bundles(&project_dir, id.as_deref(), all, older_than),
    }
}

// ---------------------------------------------------------------------------
// Subcommand: list
// ---------------------------------------------------------------------------

fn list(project_dir: &Path) -> Result<()> {
    let dir = project_dir.join(".state").join("rescue");
    if !dir.is_dir() {
        println!("(no rescue bundles)");
        return Ok(());
    }
    let mut rows = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let meta = read_metadata(&path).ok();
        let size = dir_size_bytes(&path);
        rows.push(Row {
            id: id.to_string(),
            session: meta
                .as_ref()
                .and_then(|m| {
                    m.get("session_id")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| "-".into()),
            branch: meta
                .as_ref()
                .and_then(|m| m.get("branch").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| "-".into()),
            created: meta
                .as_ref()
                .and_then(|m| {
                    m.get("timestamp_rfc3339")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| "-".into()),
            size_bytes: size,
            path,
        });
    }
    // Sort newest first by created; fall back to id.
    rows.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)));

    if rows.is_empty() {
        println!("(no rescue bundles)");
        return Ok(());
    }

    println!(
        "{:<50} {:<20} {:<30} {:<25} {:>10} PATH",
        "ID", "SESSION", "BRANCH", "CREATED", "SIZE"
    );
    for r in &rows {
        println!(
            "{:<50} {:<20} {:<30} {:<25} {:>10} {}",
            r.id,
            r.session,
            r.branch,
            r.created,
            human_size(r.size_bytes),
            r.path.display(),
        );
    }
    Ok(())
}

struct Row {
    id: String,
    session: String,
    branch: String,
    created: String,
    size_bytes: u64,
    path: PathBuf,
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

fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        if let Ok(m) = entry.metadata() {
            if m.is_file() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Subcommand: show
// ---------------------------------------------------------------------------

fn show(project_dir: &Path, id: &str) -> Result<()> {
    let bundle = resolve_bundle(project_dir, id)?;
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
        return Ok(());
    }
    let status = Command::new("git").args(&args).status()?;
    if !status.success() {
        eprintln!("(diffstat exited with non-zero status — patches may have errors)");
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
    let bundle = resolve_bundle(project_dir, id)?;

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
///
/// Rejects:
/// - `..` parent-escape components
/// - Root (`/`) or Windows-prefix components (drive letters, UNC prefixes)
/// - Null bytes embedded in component strings
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

    // Canonicalize the project dir once so containment checks are robust
    // against symlinks and mixed-case paths. Fall back to the raw path when
    // canonicalize fails (e.g., in tempdirs that don't resolve further).
    let project_root = fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());

    for entry in ar.entries()? {
        let mut entry = entry?;
        let path_in_tar = entry.path()?.to_path_buf();

        // SEC-F1 guard (1/2): reject absolute paths. These would bypass the
        // `starts_with` containment check below by writing to an absolute
        // destination that happens to not begin with project_dir.
        if path_in_tar.is_absolute() {
            eprintln!(
                "skip {}: absolute path rejected (tar entry must be relative)",
                path_in_tar.display()
            );
            continue;
        }

        // SEC-F1 guard (2/2): reject any component that would escape the
        // project root — `..`, null byte, or Windows-style drive roots.
        if has_traversal_component(&path_in_tar) {
            eprintln!(
                "skip {}: path traversal rejected (contains '..' or unsafe component)",
                path_in_tar.display()
            );
            continue;
        }

        let target = project_root.join(&path_in_tar);

        // Defence-in-depth: verify the final target still lives under the
        // canonicalized project root. Catches edge cases not covered by the
        // component walk (symlinked parents planted earlier in this run).
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

        // REV-MINOR-2: the `starts_with` check above operates on the
        // un-resolved target path. If a parent dir already exists as a
        // symlink pointing outside the project root, `entry.unpack` will
        // follow it and write outside. Close the gap by creating the
        // target's parent then canonicalizing it — the canonical path
        // must still live under project_root.
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
// Subcommand: drop
// ---------------------------------------------------------------------------

fn drop_bundles(
    project_dir: &Path,
    id: Option<&str>,
    all: bool,
    older_than: Option<u32>,
) -> Result<()> {
    // Validate flag combination BEFORE looking at the filesystem so callers
    // get a consistent error whether or not the rescue dir exists.
    let chosen_count = [id.is_some(), all, older_than.is_some()]
        .iter()
        .filter(|b| **b)
        .count();
    if chosen_count != 1 {
        bail!("drop requires exactly one of <id>, --all, --older-than N");
    }

    let dir = project_dir.join(".state").join("rescue");
    if !dir.is_dir() {
        println!("(no rescue bundles)");
        return Ok(());
    }

    if let Some(id) = id {
        let bundle = resolve_bundle(project_dir, id)?;
        fs::remove_dir_all(&bundle).with_context(|| format!("remove {}", bundle.display()))?;
        println!("dropped: {}", bundle.display());
        return Ok(());
    }

    if all {
        let mut dropped = 0;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                fs::remove_dir_all(entry.path())?;
                dropped += 1;
            }
        }
        println!("dropped {dropped} bundle(s)");
        return Ok(());
    }

    if let Some(days) = older_than {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
        let mut dropped = 0;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let Ok(meta) = read_metadata(&entry.path()) else {
                continue;
            };
            let Some(ts) = meta.get("timestamp_rfc3339").and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts) else {
                continue;
            };
            if ts.with_timezone(&chrono::Utc) < cutoff {
                fs::remove_dir_all(entry.path())?;
                dropped += 1;
            }
        }
        println!("dropped {dropped} bundle(s) older than {days}d");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_bundle(project_dir: &Path, id: &str) -> Result<PathBuf> {
    if id.contains('/') || id.contains("..") || id.contains('\\') {
        bail!("invalid bundle id: {id}");
    }
    let p = project_dir.join(".state").join("rescue").join(id);
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

    fn make_bundle(
        project: &Path,
        id: &str,
        timestamp: &str,
        head_sha: &str,
        working: &str,
        staged: &str,
        untracked: Option<(&str, &str)>,
    ) -> PathBuf {
        let dir = project.join(".state").join("rescue").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("working.patch"), working).unwrap();
        fs::write(dir.join("staged.patch"), staged).unwrap();
        // untracked tar (empty unless provided)
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
            "session_id": "ses-test",
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
        let dir = tempfile::tempdir().unwrap();
        assert!(list(dir.path()).is_ok());
    }

    #[test]
    fn test_list_multiple() {
        let dir = tempfile::tempdir().unwrap();
        make_bundle(
            dir.path(),
            "ses-a-20260101-000000-1",
            "2026-01-01T00:00:00Z",
            "deadbeef",
            "",
            "",
            None,
        );
        make_bundle(
            dir.path(),
            "ses-b-20260102-000000-2",
            "2026-01-02T00:00:00Z",
            "cafef00d",
            "",
            "",
            None,
        );
        assert!(list(dir.path()).is_ok());
    }

    #[test]
    fn test_show_missing_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let res = show(dir.path(), "does-not-exist");
        assert!(res.is_err());
    }

    #[test]
    fn test_show_valid_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let id = "ses-x-20260101-000000-1";
        make_bundle(
            dir.path(),
            id,
            "2026-01-01T00:00:00Z",
            "abc123",
            "",
            "",
            None,
        );
        // Note: `show` invokes `git apply --stat`. With empty patches that's
        // a no-op printed branch, so we run only when `git` is on PATH.
        let res = show(dir.path(), id);
        // Even with empty patches, metadata must print without error.
        assert!(res.is_ok(), "show failed: {res:?}");
    }

    #[test]
    fn test_resolve_bundle_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_bundle(dir.path(), "../../etc/passwd").is_err());
        assert!(resolve_bundle(dir.path(), "ses/../evil").is_err());
        assert!(resolve_bundle(dir.path(), "a\\b").is_err());
    }

    #[test]
    fn test_drop_all() {
        let dir = tempfile::tempdir().unwrap();
        make_bundle(dir.path(), "a", "t1", "s1", "", "", None);
        make_bundle(dir.path(), "b", "t2", "s2", "", "", None);
        drop_bundles(dir.path(), None, true, None).unwrap();
        let rescue = dir.path().join(".state/rescue");
        let count = fs::read_dir(&rescue).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_drop_by_id() {
        let dir = tempfile::tempdir().unwrap();
        make_bundle(dir.path(), "ses-one", "t", "s", "", "", None);
        make_bundle(dir.path(), "ses-two", "t", "s", "", "", None);
        drop_bundles(dir.path(), Some("ses-one"), false, None).unwrap();
        assert!(!dir.path().join(".state/rescue/ses-one").exists());
        assert!(dir.path().join(".state/rescue/ses-two").exists());
    }

    #[test]
    fn test_drop_older_than() {
        let dir = tempfile::tempdir().unwrap();
        make_bundle(dir.path(), "old", "2020-01-01T00:00:00Z", "h", "", "", None);
        // Recent bundle — should survive `--older-than 30`.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        make_bundle(dir.path(), "recent", &now, "h", "", "", None);
        drop_bundles(dir.path(), None, false, Some(30)).unwrap();
        assert!(!dir.path().join(".state/rescue/old").exists());
        assert!(dir.path().join(".state/rescue/recent").exists());
    }

    #[test]
    fn test_drop_requires_one_flag() {
        // REV-MINOR-4: every multi-flag permutation must be rejected, not
        // just id+all. Covers all 5 illegal combinations: none, id+all,
        // id+older_than, all+older_than, and all three together.
        let dir = tempfile::tempdir().unwrap();

        // 0 flags.
        let r = drop_bundles(dir.path(), None, false, None);
        assert!(r.is_err(), "no flags must be rejected");
        assert!(
            r.unwrap_err().to_string().contains("exactly one"),
            "error must name the invariant"
        );

        // id + all.
        assert!(
            drop_bundles(dir.path(), Some("x"), true, None).is_err(),
            "id + --all must be rejected"
        );

        // id + --older-than.
        assert!(
            drop_bundles(dir.path(), Some("x"), false, Some(7)).is_err(),
            "id + --older-than must be rejected"
        );

        // --all + --older-than.
        assert!(
            drop_bundles(dir.path(), None, true, Some(7)).is_err(),
            "--all + --older-than must be rejected"
        );

        // id + --all + --older-than.
        assert!(
            drop_bundles(dir.path(), Some("x"), true, Some(7)).is_err(),
            "all three flags must be rejected"
        );
    }

    /// `apply` refuses on dirty working tree.
    #[test]
    fn test_apply_requires_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        // Ignore `.state/` so bundle fixtures don't make the tree dirty.
        fs::create_dir_all(dir.path().join(".git/info")).ok();
        fs::write(dir.path().join(".git/info/exclude"), ".state/\n").unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        fs::write(dir.path().join("init.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        make_bundle(dir.path(), "ses-app1", "t", "deadbeef", "", "", None);
        // Dirty it.
        fs::write(dir.path().join("init.txt"), "b").unwrap();
        let res = apply(dir.path(), "ses-app1", false);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("working tree"),
            "error must reference working tree"
        );
    }

    /// `apply` without --force-base errors on HEAD mismatch.
    #[test]
    fn test_apply_head_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        // Ignore `.state/` so bundle fixtures don't make the tree dirty.
        fs::create_dir_all(dir.path().join(".git/info")).ok();
        fs::write(dir.path().join(".git/info/exclude"), ".state/\n").unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        fs::write(dir.path().join("init.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        make_bundle(
            dir.path(),
            "ses-mm1",
            "2026-01-01T00:00:00Z",
            "deadbeef_not_real_sha",
            "",
            "",
            None,
        );
        let res = apply(dir.path(), "ses-mm1", false);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("HEAD sha"),
            "error must reference HEAD sha; got: {err_msg}"
        );
    }

    /// Untracked restore skips already-present files.
    #[test]
    fn test_apply_restore_skips_existing() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        // Ignore `.state/` so bundle fixtures don't make the tree dirty.
        fs::create_dir_all(dir.path().join(".git/info")).ok();
        fs::write(dir.path().join(".git/info/exclude"), ".state/\n").unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        fs::write(dir.path().join("init.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

        make_bundle(
            dir.path(),
            "ses-rest1",
            "2026-01-01T00:00:00Z",
            &head_sha,
            "",
            "",
            Some(("u.txt", "from-bundle")),
        );

        // Pre-existing file with different content.
        fs::write(dir.path().join("u.txt"), "pre-existing").unwrap();
        // Not tracking it — so the working tree is dirty. Add + commit.
        Command::new("git")
            .args(["add", "u.txt"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "u", "--no-verify"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        // HEAD sha now differs — use --force-base.
        apply(dir.path(), "ses-rest1", true).unwrap();

        // Existing file must NOT be overwritten.
        let contents = fs::read_to_string(dir.path().join("u.txt")).unwrap();
        assert_eq!(contents, "pre-existing");
    }

    // -----------------------------------------------------------------
    // SEC-F1: path traversal guard in `restore_untracked`
    // -----------------------------------------------------------------

    fn make_bundle_with_raw_tar(
        project: &Path,
        id: &str,
        head_sha: &str,
        tar_bytes: &[u8],
    ) -> PathBuf {
        let dir = project.join(".state").join("rescue").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("working.patch"), "").unwrap();
        fs::write(dir.join("staged.patch"), "").unwrap();
        fs::write(dir.join("untracked.tar.gz"), tar_bytes).unwrap();
        let metadata = serde_json::json!({
            "session_id": "ses-sec",
            "branch": "feat/x",
            "head_sha": head_sha,
            "timestamp_rfc3339": "2026-04-20T00:00:00Z",
            "reason": "sec-unit-test",
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

    /// Build a gzipped tar in memory where each entry's name is controlled
    /// by the caller — including names that the `tar` crate's high-level
    /// helpers would reject (`/…`, `../…`, null bytes). Writes a minimal
    /// ustar header by hand so the resulting archive mirrors what a
    /// malicious bundle author could produce.
    fn build_evil_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        fn write_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) {
            let mut header = [0u8; 512];
            // Name: first 100 bytes of header.
            let name_bytes = name.as_bytes();
            let n = std::cmp::min(100, name_bytes.len());
            header[..n].copy_from_slice(&name_bytes[..n]);
            // Mode (0o644) as octal string with trailing NUL.
            let mode = b"0000644\0";
            header[100..108].copy_from_slice(mode);
            // uid / gid / size / mtime / typeflag as zeroed octal strings.
            header[108..116].copy_from_slice(b"0000000\0");
            header[116..124].copy_from_slice(b"0000000\0");
            // Size: octal, 11 digits + NUL.
            let size_str = format!("{:011o}", data.len());
            header[124..135].copy_from_slice(size_str.as_bytes());
            header[135] = 0;
            header[136..148].copy_from_slice(b"00000000000\0");
            header[148..156].copy_from_slice(b"        "); // placeholder chksum
            header[156] = b'0'; // typeflag = regular file
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");

            // Compute checksum: unsigned byte sum of all 512 bytes with the
            // chksum field treated as 8 spaces.
            let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
            let chksum_str = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(chksum_str.as_bytes());

            out.extend_from_slice(&header);
            out.extend_from_slice(data);
            // Pad to 512-byte boundary.
            let pad = (512 - (data.len() % 512)) % 512;
            out.extend(std::iter::repeat_n(0u8, pad));
        }

        let mut raw = Vec::new();
        for (name, data) in entries {
            write_entry(&mut raw, name, data);
        }
        // Two 512-byte zero blocks terminate the archive.
        raw.extend(std::iter::repeat_n(0u8, 1024));

        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    fn init_git_repo_at(dir: &Path) -> String {
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .unwrap();
        // Gitignore `.state/` so bundle fixtures written by tests don't
        // show up as untracked and trip `apply`'s clean-tree guard.
        fs::create_dir_all(dir.join(".git/info")).ok();
        fs::write(dir.join(".git/info/exclude"), ".state/\n").unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir)
            .status()
            .unwrap();
        fs::write(dir.join("init.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "i", "--no-verify"])
            .current_dir(dir)
            .status()
            .unwrap();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&head.stdout).trim().to_string()
    }

    #[test]
    fn test_apply_rejects_traversal_tar_entry() {
        // Tar entry named `../evil.txt` must be skipped — no file written
        // outside project_dir.
        let dir = tempfile::tempdir().unwrap();
        let head_sha = init_git_repo_at(dir.path());

        let tar_bytes = build_evil_tar(&[("../evil.txt", b"pwned")]);
        make_bundle_with_raw_tar(dir.path(), "ses-trav1", &head_sha, &tar_bytes);

        apply(dir.path(), "ses-trav1", false).unwrap();

        // Escape target (project_dir parent) must not contain evil.txt.
        let escape_path = dir.path().parent().unwrap().join("evil.txt");
        assert!(
            !escape_path.exists(),
            "SEC-F1: '../evil.txt' must NOT be extracted; found at {escape_path:?}"
        );
        // Also verify no evil.txt inside project dir (shouldn't happen either).
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn test_apply_rejects_absolute_path_tar_entry() {
        let dir = tempfile::tempdir().unwrap();
        let head_sha = init_git_repo_at(dir.path());

        // Absolute path in tar must be rejected.
        let tar_bytes = build_evil_tar(&[("/tmp/codeflow_sec_f1_evil.txt", b"pwned")]);
        make_bundle_with_raw_tar(dir.path(), "ses-abs1", &head_sha, &tar_bytes);

        apply(dir.path(), "ses-abs1", false).unwrap();

        assert!(
            !Path::new("/tmp/codeflow_sec_f1_evil.txt").exists(),
            "SEC-F1: absolute tar entry must NOT be extracted"
        );
    }

    #[test]
    fn test_apply_accepts_normal_tar_entries_regression() {
        // Regression: valid relative entries (both top-level and nested)
        // still extract correctly after the SEC-F1 guard is in place.
        let dir = tempfile::tempdir().unwrap();
        let head_sha = init_git_repo_at(dir.path());

        let tar_bytes = build_evil_tar(&[
            ("good.txt", b"top-level"),
            ("sub/dir/nested.txt", b"nested"),
        ]);
        make_bundle_with_raw_tar(dir.path(), "ses-ok1", &head_sha, &tar_bytes);

        apply(dir.path(), "ses-ok1", false).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("good.txt")).unwrap(),
            "top-level"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("sub/dir/nested.txt")).unwrap(),
            "nested"
        );
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

    /// REV-MINOR-2: when a project-local directory name is actually a
    /// symlink pointing outside the project root, `restore_untracked` must
    /// refuse to extract entries inside it. The component walk + raw
    /// `starts_with` guard only prevent syntactic escape; the symlink
    /// resolves at `unpack` time and bypasses them.
    ///
    /// Unix-only: Windows symlink creation requires elevated privileges
    /// and different API shapes.
    #[cfg(unix)]
    #[test]
    fn test_apply_rejects_symlinked_parent_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let _initial_head = init_git_repo_at(dir.path());

        // Create an external dir and a symlink inside project pointing to
        // it. Commit the symlink so the working tree stays clean for apply.
        let external = tempfile::tempdir().unwrap();
        let link_path = dir.path().join("linked");
        symlink(external.path(), &link_path).unwrap();
        Command::new("git")
            .args(["add", "linked"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add symlink", "--no-verify"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

        // Tar entry names `linked/evil.txt` — syntactically clean but its
        // parent is a symlink that escapes the project root.
        let tar_bytes = build_evil_tar(&[("linked/evil.txt", b"pwned")]);
        make_bundle_with_raw_tar(dir.path(), "ses-symlink1", &head_sha, &tar_bytes);

        apply(dir.path(), "ses-symlink1", false).unwrap();

        // The external target must NOT contain the evil file.
        assert!(
            !external.path().join("evil.txt").exists(),
            "REV-MINOR-2: symlinked parent must not allow extraction outside project_root"
        );
    }
}
