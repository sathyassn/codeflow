//! Candidate selection and retained evidence for the test gate.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::config::{ExecutionConfig, TargetConfig};
use super::error::TestingError;

/// Public selection controls. `since` and `all` never remove an owed check;
/// `only` limits a run to named targets, and such a run is never complete.
#[derive(Debug, Default)]
pub struct GateOptions {
    pub since: Option<String>,
    pub all: bool,
    /// Named targets to run with their prerequisites; empty means no limit.
    pub only: Vec<String>,
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
    home.join(super::gate_guard::HOME_EVIDENCE_DIR)
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

/// The snapshot key and the file system path of one index entry.
///
/// OS text rule (issue 79, `docs/architecture.md`): a path in the index is
/// bytes, and the gate only reads it and hashes what it names, so a name that
/// is not valid UTF-8 must not stop `codeflow test`. The file system path
/// keeps the exact bytes where the platform allows it. The key is the lossy
/// text and, for a name that is not valid UTF-8, the hex of its bytes, so two
/// different names never share a key and hide a change of one of them.
fn tracked_entry(root: &Path, raw: &[u8]) -> (String, PathBuf) {
    let key = if let Ok(name) = std::str::from_utf8(raw) {
        name.to_string()
    } else {
        let mut hex = String::with_capacity(raw.len() * 2);
        for byte in raw {
            let _ = write!(hex, "{byte:02x}");
        }
        format!("{} [bytes {hex}]", String::from_utf8_lossy(raw))
    };
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        root.join(std::ffi::OsStr::from_bytes(raw))
    };
    #[cfg(not(unix))]
    let path = root.join(String::from_utf8_lossy(raw).as_ref());
    (key, path)
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
        let (name, path) = tracked_entry(root, &entry.path);
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
            name,
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
    if !options.only.is_empty() {
        let selected = with_prerequisites(targets, options.only.iter().cloned().collect());
        return Selection {
            skipped: eligible.difference(&selected).cloned().collect(),
            selected,
            reason: "--only: the named targets and their prerequisites".into(),
        };
    }
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
    let selected = with_prerequisites(
        targets,
        targets
            .iter()
            .filter(|t| {
                eligible.contains(&t.name)
                    && (t.narrow.is_empty() || paths.iter().any(|p| matches(&t.narrow, p)))
            })
            .map(|t| t.name.clone())
            .collect(),
    );
    Selection {
        skipped: eligible.difference(&selected).cloned().collect(),
        selected,
        reason: format!("unchanged declared inputs against green base {base_sha}"),
    }
}

/// Add every target the selection requires, transitively.
fn with_prerequisites(
    targets: &[TargetConfig],
    mut selected: BTreeSet<String>,
) -> BTreeSet<String> {
    loop {
        let before = selected.len();
        for target in targets {
            if selected.contains(&target.name) {
                selected.extend(target.requires.iter().cloned());
            }
        }
        if selected.len() == before {
            return selected;
        }
    }
}

/// Refuse an `--only` name that is not an enabled target with this mode, so
/// a mistyped or retired name cannot shrink a run without a trace.
///
/// # Errors
/// Names each unknown, disabled or mode-less target.
pub fn check_only(
    root: &Path,
    targets: &[TargetConfig],
    mode: &str,
    only: &[String],
) -> Result<(), TestingError> {
    let unknown: Vec<&str> = only
        .iter()
        .filter(|name| {
            !targets
                .iter()
                .any(|t| &t.name == *name && t.enabled && t.modes.contains_key(mode))
        })
        .map(String::as_str)
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(invalid(
            root,
            format!(
                "--only names {}, which {} no enabled target with a {mode} mode",
                unknown.join(", "),
                if unknown.len() == 1 { "is" } else { "are" }
            ),
        ))
    }
}

/// Git's text output, or `None` when it failed or is not valid UTF-8.
///
/// OS text rule (issue 79, `docs/architecture.md`): kept strict on purpose.
/// The output decides which targets a run may skip, and `None` makes every
/// caller select every target, so a path that is not valid UTF-8 widens the
/// run and never narrows it. A lossy path could match a `narrow` pattern the
/// real bytes do not and drop a check the change owes.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository whose index and first commit hold `caf\xe9.txt`, a name
    /// that is not valid UTF-8, beside `plain.txt`. The file is never written
    /// to the work tree, so the repository builds on file systems that refuse
    /// such a name.
    fn repository_with_a_non_utf8_path() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        for name in [b"plain.txt".as_slice(), b"caf\xe9.txt"] {
            let id = repo.blob(b"data").unwrap();
            index
                .add(&git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100_644,
                    uid: 0,
                    gid: 0,
                    file_size: 4,
                    id,
                    flags: 0,
                    flags_extended: 0,
                    path: name.to_vec(),
                })
                .unwrap();
        }
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("test", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "test: seed",
            &tree,
            &[],
        )
        .unwrap();
        dir
    }

    /// Issue 79: the snapshot of tracked files used to fail on the first
    /// index path that is not valid UTF-8, which stopped `codeflow test` in
    /// any project with such a file name.
    #[test]
    fn tracked_snapshots_a_path_that_is_not_utf8() {
        let dir = repository_with_a_non_utf8_path();
        let snapshot = tracked(dir.path()).unwrap();
        assert_eq!(snapshot.len(), 2, "{snapshot:?}");
        assert!(snapshot.contains_key("plain.txt"));
        let key = snapshot
            .keys()
            .find(|key| key.starts_with("caf"))
            .expect("the non-UTF-8 path is in the snapshot");
        assert!(key.ends_with("[bytes 636166e92e747874]"), "{key}");
        // A second snapshot of the same tree is equal, so the gate sees no
        // generation change for a name it cannot spell.
        assert_eq!(snapshot, tracked(dir.path()).unwrap());
    }

    #[test]
    fn two_names_that_differ_only_in_invalid_bytes_keep_distinct_keys() {
        let root = Path::new("/repo");
        let (first, _) = tracked_entry(root, b"a\xe9");
        let (second, _) = tracked_entry(root, b"a\xff");
        assert_ne!(first, second);
        assert_eq!(tracked_entry(root, b"plain.txt").0, "plain.txt");
    }

    /// Kept strict on purpose: git output that is not valid UTF-8 is `None`,
    /// which every caller reads as "select every target".
    #[test]
    fn git_output_that_is_not_utf8_selects_every_target() {
        let dir = repository_with_a_non_utf8_path();
        assert!(git_output(dir.path(), &["ls-tree", "-r", "--name-only", "-z", "HEAD"]).is_none());
        assert!(git_output(dir.path(), &["rev-parse", "HEAD"]).is_some());
    }
}
