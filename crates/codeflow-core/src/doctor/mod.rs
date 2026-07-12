//! Infrastructure health checks.
//!
//! The doctor core: a small registry of named checks that run concurrently
//! and report pass/warn/fail with timing. The CLI layer renders results and
//! adds the enforcement-matrix view (charter section 3.1). Checks verify the
//! v2 surface only: the binary's hook subcommands, harness availability,
//! `.codeflow/` config validity and writability, and network reachability.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::DoctorError;
use crate::scaffold::manifest::{Ownership, RegionFormat};
use crate::scaffold::region;
use crate::scaffold::sha256_hex;
use crate::scaffold::state::InstalledManifest;

/// Outcome of a health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Warn,
}

/// Result of a single doctor check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub message: String,
    #[serde(with = "duration_millis")]
    pub duration: Duration,
}

/// Serialization helper for Duration as milliseconds.
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        s.serialize_u64(ms)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Callback to locate an executable by name.
type LookPathFn = fn(&str) -> Result<String, String>;

/// Callback to execute a command with arguments.
type ExecCommandFn = fn(&str, &[&str]) -> Result<String, String>;

/// Configuration for doctor checks.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Project root directory.
    pub project_dir: String,
    /// Locates executables. Returns Ok(path) or Err.
    pub look_path: Option<LookPathFn>,
    /// Runs a command and returns its output.
    pub exec_command: Option<ExecCommandFn>,
}

impl Options {
    fn do_look_path(&self, name: &str) -> Result<String, String> {
        if let Some(f) = self.look_path {
            f(name)
        } else {
            which::which(name)
                .map(|p| p.to_string_lossy().to_string())
                .map_err(|e| e.to_string())
        }
    }

    fn do_exec(&self, cmd: &str, args: &[&str]) -> Result<String, String> {
        if let Some(f) = self.exec_command {
            f(cmd, args)
        } else {
            let output = std::process::Command::new(cmd)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }
    }
}

/// Hook subcommands that must be functional (charter section 3.3): the
/// three Claude-layer hooks plus the five git-hook shims.
const HOOK_SUBCOMMANDS: &[(&str, &str)] = &[
    ("hook", "git-guard"),
    ("hook", "session-orient"),
    ("hook", "session-summary"),
    ("git-hook", "pre-commit"),
    ("git-hook", "commit-msg"),
    ("git-hook", "pre-merge-commit"),
    ("git-hook", "reference-transaction"),
    ("git-hook", "pre-push"),
];

/// Shipped-asset path (`src` in the scaffold manifest) of the CI workflow;
/// used to locate the installed workflow's dest in the installed-file record,
/// so a relocated workflow is still found.
const CI_ASSET_SRC: &str = "ci/codeflow-ci.yml";

/// Canonical dest for the CI workflow when the installed manifest is silent.
const CI_DEFAULT_DEST: &str = ".github/workflows/codeflow-ci.yml";

/// Sentinel emitted by the scaffolded CI's placeholder install step (the loud
/// `::error::` that fails the perimeter RED until the real installer is wired).
const CI_PLACEHOLDER_MARK: &str = "install step is an unwired PLACEHOLDER";

/// Ordered list of all check names.
const CHECK_NAMES: &[&str] = &[
    "hooks",
    "claude",
    "config",
    "permissions",
    "network",
    "delegates",
    "repo-integrity",
    "ci-perimeter",
    "managed-drift",
];

/// Return the ordered list of all available check names.
#[must_use]
pub fn check_names() -> Vec<&'static str> {
    CHECK_NAMES.to_vec()
}

/// Type alias for individual check functions.
type CheckFn = fn(&Options) -> CheckResult;

/// Build the check registry mapping names to functions.
fn check_registry() -> HashMap<&'static str, CheckFn> {
    let mut m: HashMap<&'static str, CheckFn> = HashMap::new();
    m.insert("hooks", check_hooks);
    m.insert("claude", check_claude);
    m.insert("config", check_config);
    m.insert("permissions", check_permissions);
    m.insert("network", check_network);
    m.insert("delegates", check_delegates);
    m.insert("repo-integrity", check_repo_integrity);
    m.insert("ci-perimeter", check_ci_perimeter);
    m.insert("managed-drift", check_managed_drift);
    m
}

/// Run all checks concurrently and return results in canonical order.
///
/// Uses `tokio::task::spawn_blocking` for concurrent execution. Individual
/// check failures are captured in the returned `CheckResult` items.
pub async fn run_all(opts: &Options) -> Vec<CheckResult> {
    let registry = check_registry();
    let mut handles = Vec::with_capacity(CHECK_NAMES.len());

    for &name in CHECK_NAMES {
        let f = registry[name];
        let opts_clone = opts.clone();
        handles.push(tokio::task::spawn_blocking(move || f(&opts_clone)));
    }

    let mut results = Vec::with_capacity(CHECK_NAMES.len());
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => results.push(CheckResult {
                name: "unknown".to_string(),
                status: Status::Fail,
                message: format!("task join error: {e}"),
                duration: Duration::ZERO,
            }),
        }
    }
    results
}

/// Run a single named check.
///
/// # Errors
///
/// Returns `DoctorError::CheckNotFound` if the name is not in the registry.
pub fn run_check(name: &str, opts: &Options) -> Result<CheckResult, DoctorError> {
    let registry = check_registry();
    let f = registry
        .get(name)
        .ok_or_else(|| DoctorError::CheckNotFound(name.to_string()))?;
    Ok(f(opts))
}

// ---------------------------------------------------------------------------
// Individual check functions
// ---------------------------------------------------------------------------

fn check_hooks(opts: &Options) -> CheckResult {
    let start = Instant::now();

    let Ok(codeflow_bin) = opts.do_look_path("codeflow") else {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: "codeflow binary not found in PATH".into(),
            duration: start.elapsed(),
        };
    };

    let mut failing = Vec::new();
    for &(group, subcommand) in HOOK_SUBCOMMANDS {
        if opts
            .do_exec(&codeflow_bin, &[group, subcommand, "--help"])
            .is_err()
        {
            failing.push(format!("{group} {subcommand}"));
        }
    }

    if !failing.is_empty() {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: format!(
                "{} hook subcommand(s) not responding: {}",
                failing.len(),
                failing.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "hooks".into(),
        status: Status::Pass,
        message: format!("all {} hook subcommands functional", HOOK_SUBCOMMANDS.len()),
        duration: start.elapsed(),
    }
}

fn check_claude(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_look_path("claude") {
        Ok(_) => CheckResult {
            name: "claude".into(),
            status: Status::Pass,
            message: "claude CLI found".into(),
            duration: start.elapsed(),
        },
        // Warn, not fail: git hooks + CI are the canonical enforcement
        // plane (charter section 9); codeflow works without a harness.
        Err(_) => CheckResult {
            name: "claude".into(),
            status: Status::Warn,
            message: "claude CLI not found in PATH (Claude-layer hooks inactive)".into(),
            duration: start.elapsed(),
        },
    }
}

fn check_config(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = Path::new(&opts.project_dir).join(".codeflow");

    if !config_dir.is_dir() {
        return CheckResult {
            name: "config".into(),
            status: Status::Warn,
            message: ".codeflow/ directory not found (run codeflow init)".into(),
            duration: start.elapsed(),
        };
    }

    let mut invalid_files = Vec::new();
    if let Err(e) = walk_json_files(&config_dir, &mut invalid_files) {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("error scanning .codeflow/: {e}"),
            duration: start.elapsed(),
        };
    }

    if !invalid_files.is_empty() {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("invalid JSON config files: {}", invalid_files.join(", ")),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "config".into(),
        status: Status::Pass,
        message: "all .codeflow/ JSON files are valid".into(),
        duration: start.elapsed(),
    }
}

/// Walk a directory tree and validate JSON files, recording paths relative
/// to `dir` for any that fail to parse.
fn walk_json_files(dir: &Path, invalid: &mut Vec<String>) -> Result<(), std::io::Error> {
    walk_json_files_inner(dir, dir, invalid)
}

fn walk_json_files_inner(
    root: &Path,
    dir: &Path,
    invalid: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_json_files_inner(root, &path, invalid)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let rel = path.strip_prefix(root).map_or_else(
                |_| path.to_string_lossy().to_string(),
                |p| p.to_string_lossy().to_string(),
            );
            match std::fs::read(&path) {
                Ok(data) => {
                    if serde_json::from_slice::<serde_json::Value>(&data).is_err() {
                        invalid.push(rel);
                    }
                }
                Err(e) => {
                    invalid.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    Ok(())
}

fn check_permissions(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = PathBuf::from(&opts.project_dir).join(".codeflow");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&config_dir) {
            if meta.permissions().mode() & 0o200 == 0 {
                return CheckResult {
                    name: "permissions".into(),
                    status: Status::Fail,
                    message: ".codeflow/ is not writable".into(),
                    duration: start.elapsed(),
                };
            }
        }
    }

    let _ = config_dir; // suppress unused warning on non-unix

    CheckResult {
        name: "permissions".into(),
        status: Status::Pass,
        message: "file permissions correct".into(),
        duration: start.elapsed(),
    }
}

fn check_network(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_exec("host", &["-W", "2", "github.com"]) {
        Ok(_) => CheckResult {
            name: "network".into(),
            status: Status::Pass,
            message: "network connectivity OK".into(),
            duration: start.elapsed(),
        },
        Err(_) => CheckResult {
            name: "network".into(),
            status: Status::Warn,
            message: "network connectivity check failed (offline?)".into(),
            duration: start.elapsed(),
        },
    }
}

/// Cross-vendor delegation readiness (ADR-0005). Optional by design, so a
/// missing or unauthenticated delegate warns — never fails. `codex` is the
/// primary tier: authenticated under the user's own subscription. `agy`
/// (Antigravity) is a degraded, opt-in consult tier reported informationally.
fn check_delegates(opts: &Options) -> CheckResult {
    let start = Instant::now();

    // `agy` presence is informational; codex authentication decides pass/warn.
    let agy_note = if opts.do_look_path("agy").is_ok() {
        "; agy present (degraded read-only consult tier, opt-in)"
    } else {
        ""
    };

    let Ok(codex_bin) = opts.do_look_path("codex") else {
        return CheckResult {
            name: "delegates".into(),
            status: Status::Warn,
            message: format!(
                "codex not found in PATH — cross-vendor delegation unavailable (optional){agy_note}"
            ),
            duration: start.elapsed(),
        };
    };

    // `codex login status` exits 0 only when subscription-authenticated.
    match opts.do_exec(&codex_bin, &["login", "status"]) {
        Ok(_) => CheckResult {
            name: "delegates".into(),
            status: Status::Pass,
            message: format!("codex: subscription-authenticated{agy_note}"),
            duration: start.elapsed(),
        },
        Err(_) => CheckResult {
            name: "delegates".into(),
            status: Status::Warn,
            message: format!(
                "codex present but not authenticated — run `codex login` to enable delegation{agy_note}"
            ),
            duration: start.elapsed(),
        },
    }
}

/// Repository structural integrity (ADR-0007). Two failure signatures, both
/// reproduced from a `gh pr merge --delete-branch` / worktree mishap:
///
/// 1. `core.bare = true` on a repo that still has a working tree — the root
///    checkout got flipped to bare; remedy `git config core.bare false`.
/// 2. A protected branch checked out in a non-root (linked) worktree —
///    remedy: switch that worktree to its feature branch.
///
/// Passes otherwise, and stays quiet where it cannot determine repo state
/// (not a git repo, git unavailable): this check flags, it never guesses.
fn check_repo_integrity(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let pass = |msg: &str| CheckResult {
        name: "repo-integrity".into(),
        status: Status::Pass,
        message: msg.into(),
        duration: start.elapsed(),
    };
    let fail = |msg: String| CheckResult {
        name: "repo-integrity".into(),
        status: Status::Fail,
        message: msg,
        duration: start.elapsed(),
    };

    // Signal 1: core.bare on a repo that still holds a working tree. A working
    // checkout keeps its git dir under `.git`; a genuinely bare repo has none.
    let is_bare = opts
        .do_exec(
            "git",
            &["-C", opts.project_dir.as_str(), "rev-parse", "--is-bare-repository"],
        )
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if is_bare && root.join(".git").exists() {
        return fail(
            "core.bare=true on a repo with a working tree — a merge/worktree mishap flipped it; run `git config core.bare false`".into(),
        );
    }

    // Signal 2: a protected branch checked out in a non-root worktree.
    let policy = crate::hooks::policy::Policy::load(&root).git;
    if let Ok(list) = opts.do_exec(
        "git",
        &["-C", opts.project_dir.as_str(), "worktree", "list", "--porcelain"],
    ) {
        let worktrees = parse_worktree_list(&list);
        // The main (root) worktree is listed first and may hold a protected
        // branch; only linked worktrees (the rest) must not.
        for wt in worktrees.iter().skip(1) {
            if let Some(branch) = &wt.branch {
                if policy.branch_is_protected(branch) {
                    return fail(format!(
                        "protected branch '{branch}' is checked out in a non-root worktree ({}) — switch that worktree to its feature branch",
                        wt.path
                    ));
                }
            }
        }
    }

    pass("repo layout healthy: not bare, no protected branch in a linked worktree")
}

/// One entry parsed from `git worktree list --porcelain`.
struct WorktreeEntry {
    path: String,
    /// Checked-out branch (short name), `None` when detached.
    branch: Option<String>,
}

fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            cur = Some(WorktreeEntry {
                path: path.to_string(),
                branch: None,
            });
        } else if let Some(refname) = line.strip_prefix("branch ") {
            if let Some(w) = cur.as_mut() {
                w.branch = Some(refname.strip_prefix("refs/heads/").unwrap_or(refname).to_string());
            }
        }
    }
    if let Some(w) = cur.take() {
        out.push(w);
    }
    out
}

/// Perimeter honesty (charter §9): the scaffolded CI workflow is the
/// authoritative, server-enforced perimeter — branch protection can require
/// it. `codeflow init` writes `codeflow-ci.yml` with a PLACEHOLDER install
/// step that fails RED by design (a missing binary is an unarmed perimeter,
/// not a pass). This WARNS while that placeholder stands — the gate compiles
/// and runs but enforces nothing real — and skips cleanly (Pass) when no CI
/// workflow was scaffolded (e.g. a minimal-tier project). WARN only, never a
/// block.
fn check_ci_perimeter(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let dest = ci_workflow_dest(&root);

    let Ok(content) = std::fs::read_to_string(root.join(&dest)) else {
        return CheckResult {
            name: "ci-perimeter".into(),
            status: Status::Pass,
            message: "no codeflow CI workflow found (nothing to arm)".into(),
            duration: start.elapsed(),
        };
    };

    if content.contains(CI_PLACEHOLDER_MARK) {
        return CheckResult {
            name: "ci-perimeter".into(),
            status: Status::Warn,
            message: format!(
                "{dest} install step is still the PLACEHOLDER — the CI perimeter is not armed; wire the release installer so the test/validate gates enforce"
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "ci-perimeter".into(),
        status: Status::Pass,
        message: format!("{dest} install step is wired (CI perimeter armed)"),
        duration: start.elapsed(),
    }
}

/// Where init placed the CI workflow: the installed-file record for the shipped
/// CI asset (robust to a relocated workflow), else the canonical dest.
fn ci_workflow_dest(root: &Path) -> String {
    InstalledManifest::load_or_default(root, "0")
        .ok()
        .and_then(|m| {
            m.files
                .iter()
                .find(|(_, f)| f.src == CI_ASSET_SRC)
                .map(|(dest, _)| dest.clone())
        })
        .unwrap_or_else(|| CI_DEFAULT_DEST.to_string())
}

/// Managed-region drift (charter §4.3.2): WARN when a marker-delimited managed
/// region has been hand-edited inside its `codeflow:managed` markers.
/// `codeflow update` regenerates that block from the shipped asset, so an
/// in-marker edit is silently lost on the next update — surfacing it here is
/// the honest signal. Reuses the installed-file record: the manifest stores a
/// sha256 of each managed region (the pristine shipped block); this recomputes
/// the current block's hash and flags any that no longer match. Content OUTSIDE
/// the markers is project-owned and never compared, and JSON settings merges
/// (no text markers; the record holds the shipped preset's hash, not the
/// on-disk block) are skipped. WARN only; stays quiet where it cannot read the
/// record or a file — it flags, it never guesses.
fn check_managed_drift(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let pass = |msg: String| CheckResult {
        name: "managed-drift".into(),
        status: Status::Pass,
        message: msg,
        duration: start.elapsed(),
    };

    let Ok(installed) = InstalledManifest::load_or_default(&root, "0") else {
        return pass("no readable installed manifest; drift check skipped".into());
    };

    let mut drifted: Vec<String> = Vec::new();
    for (dest, file) in &installed.files {
        if file.ownership != Ownership::ManagedRegion {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(root.join(dest)) else {
            continue; // file absent: presence is another check's concern
        };
        // Marker-delimited regions only (markdown/hash). A JSON settings merge
        // has no text markers, so extraction returns None and it is skipped —
        // its recorded hash is the shipped preset, not the on-disk block.
        let Some(block) = region::extract_block(&content, RegionFormat::Markdown)
            .or_else(|| region::extract_block(&content, RegionFormat::Hash))
        else {
            continue;
        };
        if sha256_hex(block.as_bytes()) != file.sha256 {
            drifted.push(dest.clone());
        }
    }

    if drifted.is_empty() {
        return pass("no managed-region drift (codeflow blocks match the record)".into());
    }
    CheckResult {
        name: "managed-drift".into(),
        status: Status::Warn,
        message: format!(
            "{} managed region(s) hand-edited inside codeflow markers — `codeflow update` will regenerate and lose these edits: {}",
            drifted.len(),
            drifted.join(", ")
        ),
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_opts() -> Options {
        Options {
            look_path: Some(|_| Err("not found".into())),
            exec_command: Some(|_, _| Err("not available".into())),
            ..Options::default()
        }
    }

    #[test]
    fn test_check_names_count() {
        assert_eq!(check_names().len(), 9);
    }

    #[test]
    fn test_check_registry_has_all_entries() {
        let registry = check_registry();
        for &name in CHECK_NAMES {
            assert!(registry.contains_key(name), "missing check: {name}");
        }
    }

    #[test]
    fn test_run_check_unknown() {
        let opts = test_opts();
        let result = run_check("nonexistent", &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_check_claude_not_found_warns() {
        let opts = test_opts();
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Warn);
    }

    #[test]
    fn test_check_claude_found() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "claude" {
                Ok("/usr/local/bin/claude".into())
            } else {
                Err("not found".into())
            }
        });
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_missing_dir_warns() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("policy.json"), "{}").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("bad.json"), "not json!!").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("invalid"));
        assert!(result.message.contains("bad.json"));
    }

    #[test]
    fn test_check_config_nested_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join(".codeflow").join(".baseline");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("settings.json"), "{ nope").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
    }

    #[test]
    fn test_check_hooks_no_binary() {
        let opts = test_opts();
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_hooks_all_pass() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, _args| Ok("help output".to_string()));
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("functional"));
        assert!(result.message.contains('8'));
    }

    #[test]
    fn test_check_hooks_some_failing() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, args| {
            // Fail for git-guard, pass for others.
            if args.len() >= 2 && args[1] == "git-guard" {
                Err("unknown subcommand".into())
            } else {
                Ok("ok".into())
            }
        });
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not responding"));
        assert!(result.message.contains("hook git-guard"));
    }

    #[test]
    fn test_check_network_pass() {
        let mut opts = test_opts();
        opts.exec_command = Some(|_cmd, _args| Ok("github.com has address".into()));
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_network_fail_warns() {
        let opts = test_opts(); // exec_command returns Err
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("failed"));
    }

    #[test]
    fn test_check_permissions_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[cfg(unix)]
    #[test]
    fn test_check_permissions_readonly_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);

        // Restore so tempdir cleanup succeeds.
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not writable"));
    }

    #[test]
    fn test_check_delegates_codex_authenticated_passes() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codex" {
                Ok("/usr/local/bin/codex".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_, _| Ok("Logged in using ChatGPT".into()));
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("subscription-authenticated"), "got: {}", result.message);
    }

    #[test]
    fn test_check_delegates_codex_missing_warns_not_fails() {
        // test_opts look_path always errs → codex absent. Optional, so warn.
        let opts = test_opts();
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("optional"), "got: {}", result.message);
    }

    #[test]
    fn test_check_delegates_codex_present_unauthenticated_warns_with_remedy() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codex" {
                Ok("/usr/local/bin/codex".into())
            } else {
                Err("not found".into())
            }
        });
        // exec_command errs → `codex login status` nonzero → unauthenticated.
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("codex login"), "remedy line: {}", result.message);
    }

    #[test]
    fn test_check_delegates_agy_presence_is_informational() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "agy" => Ok("/usr/local/bin/agy".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, _| Ok("Logged in".into()));
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("agy present"), "got: {}", result.message);
    }

    #[test]
    fn test_repo_integrity_bare_with_working_tree_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("true\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Fail);
        assert!(r.message.contains("core.bare"), "got: {}", r.message);
        assert!(r.message.contains("git config core.bare false"), "remedy: {}", r.message);
    }

    #[test]
    fn test_repo_integrity_protected_in_linked_worktree_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("false\n".into())
            } else if args.contains(&"worktree") {
                // Root on a feature branch; a LINKED worktree holds main.
                Ok("worktree /repo/root\nHEAD aaa\nbranch refs/heads/feat/x\n\nworktree /repo/wt\nHEAD bbb\nbranch refs/heads/main\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Fail);
        assert!(r.message.contains("main"), "got: {}", r.message);
        assert!(r.message.contains("worktree"), "got: {}", r.message);
    }

    #[test]
    fn test_repo_integrity_healthy_passes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("false\n".into())
            } else if args.contains(&"worktree") {
                // Root holds main (allowed); the linked worktree is a feature.
                Ok("worktree /repo/root\nHEAD aaa\nbranch refs/heads/main\n\nworktree /repo/wt\nHEAD bbb\nbranch refs/heads/feat/x\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_repo_integrity_non_repo_passes_quietly() {
        // git unavailable / not a repo → no signal → pass, never guess.
        let opts = test_opts(); // exec_command returns Err
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Pass);
    }

    // --- ci-perimeter -------------------------------------------------------

    fn write_ci(root: &Path, body: &str) {
        let ci = root.join(CI_DEFAULT_DEST);
        std::fs::create_dir_all(ci.parent().unwrap()).unwrap();
        std::fs::write(&ci, body).unwrap();
    }

    #[test]
    fn test_ci_perimeter_placeholder_warns() {
        let dir = tempfile::tempdir().unwrap();
        write_ci(
            dir.path(),
            "steps:\n  - name: Install codeflow\n    run: echo \"::error::codeflow install step is an unwired PLACEHOLDER\"\n",
        );
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert_eq!(r.status, Status::Warn);
        assert!(r.message.contains("not armed"), "got: {}", r.message);
        assert!(r.message.contains("codeflow-ci.yml"), "names the file: {}", r.message);
    }

    #[test]
    fn test_ci_perimeter_wired_passes() {
        let dir = tempfile::tempdir().unwrap();
        write_ci(
            dir.path(),
            "steps:\n  - name: Install codeflow\n    run: curl -fsSL https://example/installer.sh | sh\n",
        );
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
        assert!(r.message.contains("armed"), "got: {}", r.message);
    }

    #[test]
    fn test_ci_perimeter_missing_file_skips_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert_eq!(r.status, Status::Pass);
        assert!(r.message.contains("no codeflow CI workflow"), "got: {}", r.message);
    }

    // --- managed-drift ------------------------------------------------------

    /// A single managed-region record whose sha256 is the hash of `block`,
    /// exactly as `codeflow update` records it.
    fn record_region(root: &Path, dest: &str, src: &str, sha_of: &str) {
        use crate::scaffold::state::InstalledFile;
        let mut m = InstalledManifest::new("2.0.0");
        m.files.insert(
            dest.to_string(),
            InstalledFile {
                src: src.to_string(),
                ownership: Ownership::ManagedRegion,
                sha256: sha256_hex(sha_of.as_bytes()),
                exec: false,
            },
        );
        m.store(root).unwrap();
    }

    const REGION_BLOCK: &str =
        "<!-- codeflow:managed:begin scaffold=2.0.0 -->\nrules\n<!-- codeflow:managed:end -->";

    #[test]
    fn test_managed_drift_clean_when_block_matches_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("AGENTS.md"), format!("# Mine\n\n{REGION_BLOCK}\n\ntail\n")).unwrap();
        record_region(root, "AGENTS.md", "AGENTS.md.tmpl", REGION_BLOCK);

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_managed_drift_flags_in_marker_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // On disk the region has been hand-edited INSIDE the markers.
        let edited =
            "<!-- codeflow:managed:begin scaffold=2.0.0 -->\nrules HAND EDITED\n<!-- codeflow:managed:end -->";
        std::fs::write(root.join("AGENTS.md"), format!("# Mine\n\n{edited}\n")).unwrap();
        // The record still holds the pristine block's hash.
        record_region(root, "AGENTS.md", "AGENTS.md.tmpl", REGION_BLOCK);

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Warn);
        assert!(r.message.contains("AGENTS.md"), "names the drifted file: {}", r.message);
        assert!(r.message.contains("codeflow update"), "explains the risk: {}", r.message);
    }

    #[test]
    fn test_managed_drift_ignores_outside_marker_edits_and_json_regions() {
        use crate::scaffold::state::InstalledFile;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Edits OUTSIDE the markers are project-owned and must NOT flag.
        std::fs::write(
            root.join("AGENTS.md"),
            format!("# Heavily customized intro\n\n{REGION_BLOCK}\n\nlots of my own notes\n"),
        )
        .unwrap();
        // A JSON managed-region (settings): no text markers, and its recorded
        // hash is the shipped preset, never the on-disk block — must be skipped
        // even though the sha deliberately does not match.
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{\"hooks\": {}}\n").unwrap();

        let mut m = InstalledManifest::new("2.0.0");
        m.files.insert(
            "AGENTS.md".into(),
            InstalledFile {
                src: "AGENTS.md.tmpl".into(),
                ownership: Ownership::ManagedRegion,
                sha256: sha256_hex(REGION_BLOCK.as_bytes()),
                exec: false,
            },
        );
        m.files.insert(
            ".claude/settings.json".into(),
            InstalledFile {
                src: "settings/default.json".into(),
                ownership: Ownership::ManagedRegion,
                sha256: "deadbeef".into(),
                exec: false,
            },
        );
        m.store(root).unwrap();

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Pass, "outside-marker/JSON must not flag: {}", r.message);
    }

    #[test]
    fn test_managed_drift_no_manifest_passes_quietly() {
        // No installed manifest → nothing to compare → pass, never guess.
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Pass);
    }

    #[test]
    fn test_status_serde() {
        let json = serde_json::to_string(&Status::Pass).unwrap();
        assert_eq!(json, "\"pass\"");
        let parsed: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Status::Pass);
    }

    #[test]
    fn test_check_result_serialization() {
        let result = CheckResult {
            name: "test".into(),
            status: Status::Pass,
            message: "ok".into(),
            duration: Duration::from_millis(42),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"duration\":42"));
    }

    #[tokio::test]
    async fn test_run_all_returns_all_results() {
        let opts = test_opts();
        let results = run_all(&opts).await;
        assert_eq!(results.len(), CHECK_NAMES.len());
    }

    #[test]
    fn test_run_check_valid_name() {
        let opts = test_opts();
        let result = run_check("claude", &opts).unwrap();
        assert_eq!(result.name, "claude");
    }
}
