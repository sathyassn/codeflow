//! Candidate selection and retained evidence for the test gate.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::config::{ExecutionConfig, TargetConfig};
use super::error::TestingError;

/// Public selection controls. Neither control can remove an owed check.
#[derive(Debug, Default)]
pub struct GateOptions {
    pub since: Option<String>,
    pub all: bool,
}

#[derive(Debug, Serialize)]
pub struct Selection {
    pub selected: BTreeSet<String>,
    pub skipped: BTreeSet<String>,
    pub reason: String,
}

pub fn invalid(root: &Path, message: impl Into<String>) -> TestingError {
    TestingError::ConfigInvalid {
        path: root.join(super::gate::TEST_CONFIG_PATH),
        message: message.into(),
    }
}

pub fn digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

pub fn target_dir(root: &Path) -> PathBuf {
    root.join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
}

pub fn durable_root(root: &Path, home: &Path) -> PathBuf {
    let identity = git2::Repository::discover(root)
        .ok()
        .and_then(|r| r.commondir().canonicalize().ok())
        .unwrap_or_else(|| root.to_path_buf());
    home.join("gate-runs")
        .join(&digest(identity.to_string_lossy().as_bytes())[..16])
}

pub fn revision(root: &Path) -> Option<String> {
    git2::Repository::discover(root)
        .ok()?
        .head()
        .ok()?
        .target()
        .map(|id| id.to_string())
}

/// Snapshot the bytes and modes of every tracked path, including dirty edits.
pub fn tracked(root: &Path) -> Result<BTreeMap<String, String>, TestingError> {
    let Ok(repo) = git2::Repository::discover(root) else {
        return Ok(BTreeMap::new());
    };
    let mut result = BTreeMap::new();
    for entry in repo
        .index()
        .map_err(|e| invalid(root, e.to_string()))?
        .iter()
    {
        let name = std::str::from_utf8(&entry.path).map_err(|e| invalid(root, e.to_string()))?;
        let path = root.join(name);
        let bytes = if path.is_symlink() {
            std::fs::read_link(&path)?
                .to_string_lossy()
                .as_bytes()
                .to_vec()
        } else if path.exists() {
            std::fs::read(&path)?
        } else {
            b"<deleted>".to_vec()
        };
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::symlink_metadata(&path).map_or(0, |m| m.permissions().mode())
        };
        #[cfg(not(unix))]
        let mode = entry.mode;
        result.insert(
            name.to_string(),
            digest(&[mode.to_le_bytes().as_slice(), &bytes].concat()),
        );
    }
    Ok(result)
}

#[must_use]
pub fn matches(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| {
        glob::Pattern::new(pattern).is_ok_and(|p| {
            p.matches_with(
                path,
                glob::MatchOptions {
                    require_literal_separator: true,
                    ..glob::MatchOptions::new()
                },
            )
        })
    })
}

fn green_base(root: &Path, home: Option<&Path>, base: &str, config_digest: &str) -> bool {
    let Some(home) = home else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(durable_root(root, home)) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let artifact = std::fs::read(entry.path().join("run.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        artifact.is_some_and(|a| {
            a["passed"] == true
                && a["complete"] == true
                && a["clean"] == true
                && a["revision"] == base
                && a["config_digest"] == config_digest
        })
    })
}

/// Select conservatively from the whole delta, then include producer closure.
/// Unknown evidence, rename, deletion, untracked or unmatched input means all.
pub fn select(
    root: &Path,
    targets: &[TargetConfig],
    execution: &ExecutionConfig,
    mode: &str,
    options: &GateOptions,
    home: Option<&Path>,
    config_digest: &str,
) -> Selection {
    let eligible: BTreeSet<String> = targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key(mode))
        .map(|t| t.name.clone())
        .collect();
    let all = |reason: &str| Selection {
        selected: eligible.clone(),
        skipped: BTreeSet::new(),
        reason: reason.into(),
    };
    if options.all {
        return all("--all: every target");
    }
    let Some(base) = &options.since else {
        return all("no proven comparison base");
    };
    let Some(base_sha) = git_output(
        root,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    ) else {
        return all("unproven base");
    };
    let base_sha = base_sha.trim();
    if !green_base(root, home, base_sha, config_digest) {
        return all("base has no green run with the same config digest");
    }
    if git_output(root, &["merge-base", "--is-ancestor", base_sha, "HEAD"]).is_none() {
        return all("base is not a candidate ancestor");
    }
    // Include staged and unstaged edits as well as the committed range.
    let Some(delta) = git_output(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            base_sha,
            "--",
        ],
    ) else {
        return all("delta unavailable");
    };
    let Some(untracked) = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])
    else {
        return all("untracked inputs unavailable");
    };
    let mut paths = Vec::new();
    let mut fields = delta.split('\0').filter(|f| !f.is_empty());
    while let Some(status) = fields.next() {
        if status.starts_with(['R', 'D', 'C']) {
            return all("rename or deletion: every target");
        }
        let Some(path) = fields.next() else {
            return all("unrecognized delta");
        };
        paths.push(path);
    }
    paths.extend(untracked.split('\0').filter(|p| !p.is_empty()));
    if paths.iter().any(|p| matches(&execution.run_everything, p)) {
        return all("shared infrastructure changed");
    }
    if paths
        .iter()
        .any(|p| !targets.iter().any(|t| matches(&t.narrow, p)))
    {
        return all("unmatched input: every target");
    }
    let mut selected: BTreeSet<String> = targets
        .iter()
        .filter(|t| {
            eligible.contains(&t.name)
                && (t.narrow.is_empty() || paths.iter().any(|p| matches(&t.narrow, p)))
        })
        .map(|t| t.name.clone())
        .collect();
    loop {
        let before = selected.len();
        for target in targets {
            if selected.contains(&target.name) {
                selected.extend(target.requires.iter().cloned());
            }
        }
        if selected.len() == before {
            break;
        }
    }
    Selection {
        skipped: eligible.difference(&selected).cloned().collect(),
        selected,
        reason: format!("unchanged declared inputs against green base {base_sha}"),
    }
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = crate::git::command()
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

/// Probe writable temporary storage and the tools actually named by selected commands.
pub fn preflight(
    root: &Path,
    targets: &[TargetConfig],
    mode: &str,
) -> Result<BTreeMap<String, String>, TestingError> {
    let probe = std::env::temp_dir().join(format!("codeflow-gate-probe-{}", uuid::Uuid::new_v4()));
    std::fs::write(&probe, b"probe").map_err(|e| {
        invalid(
            root,
            format!("temporary directory is not writable in this sandbox: {e}"),
        )
    })?;
    std::fs::remove_file(probe)?;
    let mut tools = BTreeSet::new();
    let mut versions = BTreeMap::new();
    for t in targets {
        let command = &t.modes[mode].command;
        for tool in ["cargo", "python3", "node", "npm", "git"] {
            if matches!(tool, "node" | "npm")
                && command.starts_with("python3 -B scripts/with-node.py ")
            {
                continue;
            }
            if command
                .split(|c: char| c.is_whitespace() || "'\";&|".contains(c))
                .any(|word| word == tool)
            {
                tools.insert(tool);
            }
        }
        for sub in ["nextest", "llvm-cov"] {
            if command.contains(&format!("cargo {sub}"))
                || (sub == "nextest" && command.contains("llvm-cov nextest"))
            {
                let output = Command::new("cargo")
                    .args([sub, "--version"])
                    .output()
                    .map_err(|e| {
                        invalid(root, format!("missing cargo {sub} in this sandbox: {e}"))
                    })?;
                if !output.status.success() {
                    return Err(invalid(
                        root,
                        format!("missing cargo {sub} in this sandbox"),
                    ));
                }
                versions.insert(format!("cargo {sub}"), observation(&output));
            }
        }
    }
    for tool in tools {
        let path = which::which(tool)
            .map_err(|e| invalid(root, format!("missing tool '{tool}' in this sandbox: {e}")))?;
        let output = crate::git::process(&path).arg("--version").output()?;
        if !output.status.success() {
            return Err(invalid(
                root,
                format!("tool '{tool}' version probe failed in this sandbox"),
            ));
        }
        versions.insert(
            tool.into(),
            format!("{}: {}", path.display(), observation(&output)),
        );
    }
    // Use each target's pinned Node launcher for readiness. A browser is needed
    // only for browser targets; this probe neither installs nor launches it.
    for t in targets {
        let command = &t.modes[mode].command;
        if let Some(rest) = command.strip_prefix("python3 -B scripts/with-node.py ") {
            let pin = rest.split_whitespace().next().unwrap_or("");
            let mut probe = String::from("node --version");
            if command.contains("npm ") {
                probe.push_str(" && npm --version");
            }
            if command.contains("check:browser") || command.contains("check:real-browser") {
                probe.push_str(" && node scripts/check-gate-browser.mjs");
            }
            let mut cmd = Command::new("python3");
            cmd.args(["-B", "scripts/with-node.py", pin, &probe])
                .current_dir(root);
            let output = cmd
                .output()
                .map_err(|e| invalid(root, format!("Node preflight: {e}")))?;
            if !output.status.success() {
                return Err(invalid(
                    root,
                    format!(
                        "Node/browser unavailable in this sandbox for '{}': {}",
                        t.name,
                        String::from_utf8_lossy(&output.stderr)
                    ),
                ));
            }
            versions.insert(format!("node:{pin}"), observation(&output));
        }
    }
    Ok(versions)
}

fn observation(output: &std::process::Output) -> String {
    format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .trim()
    .to_string()
}
