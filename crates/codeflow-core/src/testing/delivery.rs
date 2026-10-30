//! Candidate selection and retained evidence for the test gate.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::config::TargetConfig;
use super::error::TestingError;

/// Public selection controls. `since` and `all` never remove an owed check;
/// `only` limits a run to named targets, and such a run is never complete.
#[derive(Debug, Default, Clone)]
pub struct GateOptions {
    pub since: Option<String>,
    pub all: bool,
    /// Named targets to run with their prerequisites; empty means no limit.
    pub only: Vec<String>,
    /// A test config to read instead of the repository's
    /// `.codeflow/test-config.json`, with no fallback to it or to stack
    /// detection (`codeflow test --config`, TSK-1020).
    pub config: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Selection {
    pub selected: BTreeSet<String>,
    pub skipped: BTreeSet<String>,
    /// Targets outside an explicit caller selection, never evidence skips.
    pub excluded: BTreeSet<String>,
    pub reason: String,
    pub reasons: BTreeMap<String, String>,
}

pub fn invalid(root: &Path, message: impl Into<String>) -> TestingError {
    TestingError::ConfigInvalid {
        path: root.join(super::gate::TEST_CONFIG_PATH),
        message: message.into(),
    }
}

/// A refused gate evidence read or write, naming the full path. When the
/// link is the build-output folder or the `CodeFlow` home itself, the
/// message names the recovery.
pub(super) fn evidence_refused(
    root: &Path,
    relative: &str,
    error: &std::io::Error,
) -> TestingError {
    let recovery = if configured_folder_is_link(root, relative) {
        " The build-output folder or the CodeFlow home is itself a link: set \
         CARGO_TARGET_DIR or CODEFLOW_HOME to the real folder it names."
    } else {
        ""
    };
    TestingError::Io(std::io::Error::new(
        error.kind(),
        format!(
            "gate evidence refused at {}: {error}; nothing was written through it.{recovery}",
            root.join(relative).display()
        ),
    ))
}

/// Whether the link that refused `relative` lies in the folder the operator
/// configured (the build-output folder or the home): the part of `relative`
/// before the gate's own `codeflow-gate` or `gate-runs` folder.
fn configured_folder_is_link(root: &Path, relative: &str) -> bool {
    let Some(link) = crate::contained::linked_component(root, relative) else {
        return false;
    };
    let configured: Vec<&str> = relative
        .split('/')
        .take_while(|part| !matches!(*part, "codeflow-gate" | "gate-runs"))
        .collect();
    link.split('/').count() <= configured.len()
}

/// Where a path a target names lies, for the anchored walk: beneath the
/// build-output folder `target` of the evidence anchor `anchor`, else
/// beneath the project `root`, as the tree to walk and the `/`-separated
/// path inside it (TSK-1044). Every read, presence check or removal of such
/// a path goes through the returned tree, so a link or junction at any
/// component is refused. A path outside both folders is refused.
///
/// # Errors
///
/// When the path lies outside both folders or a root cannot be opened.
pub(super) fn locate(
    root: &Path,
    anchor: &Path,
    target: &str,
    path: &Path,
) -> std::io::Result<(crate::contained::Tree, String)> {
    let path: PathBuf = path
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .collect();
    let beneath = |base: &Path| {
        crate::contained::relative_to(base, &path)
            .ok()
            .filter(|relative| {
                relative == target
                    || relative
                        .strip_prefix(target)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
    };
    if let Some(relative) = beneath(anchor) {
        return Ok((crate::contained::Tree::open(anchor)?, relative));
    }
    if let Ok(output) = crate::contained::Tree::open(anchor) {
        if let Some(relative) = beneath(output.path()) {
            return Ok((output, relative));
        }
    }
    let project = crate::contained::Tree::open(root)?;
    let relative = crate::contained::relative_to(root, &path)
        .or_else(|_| crate::contained::relative_to(project.path(), &path))
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} lies outside the project and its build-output folder",
                    path.display()
                ),
            )
        })?;
    Ok((project, relative))
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

/// The trusted folder the gate's evidence writes walk from, and the
/// build-output folder's `/`-separated path beneath it (TSK-1044). For
/// `target` or a relative `CARGO_TARGET_DIR` made only of names, the folder
/// is the project and every component of the build-output path is entered
/// without following a link; otherwise it is the build-output folder's
/// parent, and its last component is entered that way. Either way a link or
/// junction planted at the build-output folder is refused, never trusted.
///
/// # Errors
///
/// When the build-output path has no parent or a component that is not a
/// UTF-8 name.
pub fn target_anchor(root: &Path) -> Result<(PathBuf, String), TestingError> {
    let configured = std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into());
    let path = Path::new(&configured);
    if path.is_relative() {
        if let Ok(relative) = crate::contained::relative_to(Path::new(""), path) {
            if !relative.is_empty() {
                return Ok((root.to_path_buf(), relative));
            }
        }
    }
    let full = root.join(path);
    let (parent, leaf) = crate::contained::split_leaf(&full).map_err(|_| {
        invalid(
            root,
            format!(
                "build-output folder {} has no parent or is not a UTF-8 name",
                full.display()
            ),
        )
    })?;
    Ok((parent.to_path_buf(), leaf))
}

/// The durable evidence folder of the repository at `root`, relative to the
/// `CodeFlow` home: `gate-runs/<digest of the repository's common folder>`.
pub fn durable_relative(root: &Path) -> String {
    let identity = git2::Repository::discover(root)
        .ok()
        .and_then(|r| r.commondir().canonicalize().ok())
        .unwrap_or_else(|| root.to_path_buf());
    format!(
        "{}/{}",
        super::gate_guard::HOME_EVIDENCE_DIR,
        // OS text rule (issue 79): the repository's own folder names its
        // evidence, so it is hashed by its exact bytes. A lossy spelling would
        // let two folders share one evidence directory.
        &digest(identity.as_os_str().as_encoded_bytes())[..16]
    )
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
/// keeps the exact bytes where the platform allows it. The key is the name
/// itself when it is valid UTF-8. Otherwise it is the lossy text, a NUL and the
/// hex of the bytes: a path never holds a NUL, so no valid name can equal that
/// key, and two different invalid names differ in their hex, so one name never
/// hides a change of another in the snapshot.
fn tracked_entry(root: &Path, raw: &[u8]) -> (String, Option<PathBuf>) {
    let name = crate::git::GitName::from_bytes(raw);
    // Exact bytes where the platform holds them. Where it cannot (a name that
    // is not UTF-8 on native Windows) no file of that name can exist, so
    // there is no path and the key alone stands for the entry, never a
    // lossy lookalike of another name.
    let path = name.os_path().ok().map(|relative| root.join(relative));
    (name.storage_key(), path)
}

/// The bytes of a link target, exact on every platform (`as_encoded_bytes`
/// keeps a Windows target's unpaired surrogates, which a lossy spelling would
/// merge).
fn link_target_bytes(target: &Path) -> Vec<u8> {
    target.as_os_str().as_encoded_bytes().to_vec()
}

/// Snapshot the bytes and modes of every tracked path, including dirty edits.
pub fn tracked(root: &Path) -> Result<BTreeMap<String, String>, TestingError> {
    let repo = match git2::Repository::discover(root) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(invalid(root, format!("cannot discover repository: {error}; repair repository metadata before rerunning the gate"))),
    };
    tracked_repository(root, &repo, &mut BTreeSet::new())
}

fn tracked_repository(
    root: &Path,
    repo: &git2::Repository,
    visiting: &mut BTreeSet<PathBuf>,
) -> Result<BTreeMap<String, String>, TestingError> {
    let identity = repo.path().canonicalize().map_err(TestingError::Io)?;
    if !visiting.insert(identity.clone()) {
        return Err(invalid(
            root,
            "submodule metadata forms a cycle; repair it before rerunning the gate",
        ));
    }
    let mut result = BTreeMap::new();
    let mut folders = BTreeMap::new();
    for entry in repo
        .index()
        .map_err(|e| invalid(root, e.to_string()))?
        .iter()
    {
        let (name, path) = tracked_entry(root, &entry.path);
        let Some(path) = path else {
            result.insert(
                name,
                digest(&[entry.mode.to_le_bytes().as_slice(), b"<unrepresentable>"].concat()),
            );
            continue;
        };
        if entry.mode == 0o160_000 {
            result.insert(name, submodule_snapshot(&path, entry.id, visiting)?);
            continue;
        }
        let unreadable = |error: std::io::Error| {
            TestingError::Io(std::io::Error::new(error.kind(), format!("cannot snapshot {}: {error}; restore a readable tracked path before rerunning the gate", path.display())))
        };
        // A link or junction that replaced a tracked folder is a change,
        // never read through to the same bytes elsewhere (TSK-1044).
        if beneath_link(root, &path, &mut folders).map_err(unreadable)? {
            result.insert(name, digest(b"<beneath a link>"));
            continue;
        }
        let metadata = if crate::absence::proven_absent(&path).map_err(unreadable)? {
            None
        } else {
            Some(std::fs::symlink_metadata(&path).map_err(unreadable)?)
        };
        let bytes = if metadata
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink())
        {
            // The exact target bytes: a lossy spelling would give two
            // different targets one hash and hide a change of the link.
            link_target_bytes(&std::fs::read_link(&path).map_err(unreadable)?)
        } else if metadata.is_some() {
            std::fs::read(&path).map_err(unreadable)?
        } else {
            b"<deleted>".to_vec()
        };
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.map_or(0, |m| m.permissions().mode())
        };
        #[cfg(not(unix))]
        let mode = entry.mode;
        result.insert(
            name,
            digest(&[mode.to_le_bytes().as_slice(), &bytes].concat()),
        );
    }
    visiting.remove(&identity);
    Ok(result)
}

/// Whether a folder between `root` and the tracked `path` is a link or
/// junction, read without following; `folders` caches each folder's answer.
fn beneath_link(
    root: &Path,
    path: &Path,
    folders: &mut BTreeMap<PathBuf, bool>,
) -> std::io::Result<bool> {
    let mut folder = path.parent();
    while let Some(current) = folder {
        if current == root || !current.starts_with(root) {
            return Ok(false);
        }
        let linked = if let Some(linked) = folders.get(current) {
            *linked
        } else {
            let linked = crate::contained::is_link_at(current)?;
            folders.insert(current.to_path_buf(), linked);
            linked
        };
        if linked {
            return Ok(true);
        }
        folder = current.parent();
    }
    Ok(false)
}

/// A gitlink is a repository, not a file. Include both its checked-out
/// revision and recursively tracked bytes, so edits during a target run
/// cannot hide behind an unchanged gitlink or an already-dirty status.
fn submodule_snapshot(
    path: &Path,
    recorded: git2::Oid,
    visiting: &mut BTreeSet<PathBuf>,
) -> Result<String, TestingError> {
    let repo = match git2::Repository::open_ext(
        path,
        git2::RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&Path>(),
    ) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Ok(digest(format!("{recorded}:<uninitialized>").as_bytes()));
        }
        Err(error) => {
            return Err(invalid(
                path,
                format!(
                "cannot snapshot submodule metadata: {error}; repair it before rerunning the gate"
            ),
            ))
        }
    };
    let head = repo
        .head()
        .map_err(|error| invalid(path, format!("cannot snapshot submodule HEAD: {error}")))?;
    let files = tracked_repository(path, &repo, visiting)?;
    let bytes = serde_json::to_vec(&(
        recorded.to_string(),
        head.target().map(|id| id.to_string()),
        files,
    ))
    .map_err(|error| invalid(path, format!("cannot encode submodule snapshot: {error}")))?;
    Ok(digest(&bytes))
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

/// Select the targets a run executes. `--only` limits a run to the named
/// targets and their prerequisites. Otherwise every eligible target runs:
/// local selection credit is withdrawn (ADR-0089, TSK-1044), because every
/// run record is written by a process an agent session or a candidate's
/// tests can run as the same user, so no record can prove that a check
/// passed. `--since` and the task line no longer narrow a run.
#[must_use]
pub fn select(targets: &[TargetConfig], mode: &str, options: &GateOptions) -> Selection {
    let eligible: BTreeSet<String> = targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key(mode))
        .map(|t| t.name.clone())
        .collect();
    if !options.only.is_empty() {
        let selected = with_prerequisites(targets, options.only.iter().cloned().collect());
        return Selection {
            excluded: eligible.difference(&selected).cloned().collect(),
            skipped: BTreeSet::new(),
            selected,
            reason: "--only: the named targets and their prerequisites".into(),
            reasons: BTreeMap::new(),
        };
    }
    let reason = if options.all {
        "--all: every target".to_string()
    } else if let Some(base) = &options.since {
        format!("every target: --since {base} narrows nothing, since no local run record earns selection credit (ADR-0089)")
    } else {
        "every target: no local run record earns selection credit (ADR-0089)".to_string()
    };
    Selection {
        selected: eligible,
        skipped: BTreeSet::new(),
        excluded: BTreeSet::new(),
        reason,
        reasons: BTreeMap::new(),
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

/// Git's text output, or an error when it failed or is not valid UTF-8.
///
/// OS text rule (issue 79, `docs/architecture.md`): kept strict on purpose.
/// Its one caller, the untracked-input scan of a target's recorded evidence,
/// reads an error as "evidence unavailable", so a path that is not valid
/// UTF-8 is never matched by its lossy spelling. No run is narrowed by it:
/// no record earns selection credit (ADR-0089).
pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::git::command()
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|error| format!("cannot read Git comparison: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot read Git comparison: Git exited {}",
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("cannot read Git comparison as UTF-8: {error}"))
}

/// Probe writable temporary storage and the tools actually named by selected commands.
pub fn preflight(
    root: &Path,
    targets: &[TargetConfig],
    mode: &str,
) -> Result<BTreeMap<String, String>, TestingError> {
    use std::io::Write;

    // An unnamed, exclusively created file cannot follow a planted temp name.
    let mut probe = tempfile::tempfile().map_err(|e| {
        invalid(
            root,
            format!("temporary directory is not writable in this sandbox: {e}"),
        )
    })?;
    probe.write_all(b"probe").map_err(|e| {
        invalid(
            root,
            format!("temporary directory is not writable in this sandbox: {e}"),
        )
    })?;
    drop(probe);
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
                .split(|c: char| matches!(c, ' ' | '\t' | '\n') || "'\";&|".contains(c))
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
            let pin = rest
                .split([' ', '\t', '\n'])
                .find(|word| !word.is_empty())
                .unwrap_or("");
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

    /// TSK-1044: no run record earns selection credit, so every eligible
    /// target of a quick, essential or full run is selected, `--since`
    /// included, and nothing is skipped; `--only` still limits a run.
    #[test]
    fn selection_runs_every_eligible_target_and_never_skips() {
        let targets: Vec<TargetConfig> = serde_json::from_value(serde_json::json!([
            {"name":"content","runner":"custom","input_class":"content","narrow":["docs/**"],
             "modes":{"quick":{"command":"echo q"},"essential":{"command":"echo e"},"full":{"command":"echo f"}}},
            {"name":"other","runner":"custom","requires":["content"],
             "modes":{"quick":{"command":"echo q"},"essential":{"command":"echo e"},"full":{"command":"echo f"}}},
            {"name":"off","runner":"custom","enabled":false,"modes":{"quick":{"command":"echo q"}}}
        ]))
        .unwrap();
        let every = BTreeSet::from(["content".to_string(), "other".to_string()]);
        for mode in ["quick", "essential", "full"] {
            for since in [None, Some("HEAD~1".to_string())] {
                let options = GateOptions {
                    since: since.clone(),
                    ..GateOptions::default()
                };
                let selection = select(&targets, mode, &options);
                assert_eq!(selection.selected, every, "{mode} {since:?}");
                assert!(selection.skipped.is_empty(), "{mode} {since:?}");
                assert!(
                    selection.reason.contains("ADR-0089"),
                    "{}",
                    selection.reason
                );
                if let Some(base) = &since {
                    assert!(selection
                        .reason
                        .contains(&format!("--since {base} narrows nothing")));
                }
            }
        }
        let only = select(
            &targets,
            "quick",
            &GateOptions {
                only: vec!["other".into()],
                ..GateOptions::default()
            },
        );
        assert_eq!(only.selected, every, "--only keeps the prerequisite");
        assert!(only.skipped.is_empty());
    }

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
        assert!(key.ends_with("\x00636166e92e747874"), "{key:?}");
        // A second snapshot of the same tree is equal, so the gate sees no
        // generation change for a name it cannot spell.
        assert_eq!(snapshot, tracked(dir.path()).unwrap());
    }

    /// Grok review 2: a path a target names resolves beneath the
    /// build-output folder or the project only. A path beside the
    /// build-output folder under an absolute anchor, or outside both, is
    /// refused.
    #[test]
    fn locate_keeps_a_path_beneath_the_build_output_folder_or_the_project() {
        let project = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(parent.path().join("out")).unwrap();
        let (tree, relative) = locate(
            project.path(),
            parent.path(),
            "out",
            &parent.path().join("out/r.xml"),
        )
        .unwrap();
        assert_eq!(relative, "out/r.xml");
        assert_eq!(tree.path(), parent.path().canonicalize().unwrap());
        let (tree, relative) = locate(
            project.path(),
            parent.path(),
            "out",
            &project.path().join("./reports/r.xml"),
        )
        .unwrap();
        assert_eq!(relative, "reports/r.xml");
        assert_eq!(tree.path(), project.path().canonicalize().unwrap());
        for beside in ["outer/r.xml", "other/r.xml"] {
            let error = locate(
                project.path(),
                parent.path(),
                "out",
                &parent.path().join(beside),
            )
            .err()
            .unwrap();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{beside}");
        }
    }

    /// Grok review 2: the refusal names `CARGO_TARGET_DIR` and
    /// `CODEFLOW_HOME` only when the link is the folder they configure.
    #[cfg(unix)]
    #[test]
    fn the_refusal_names_the_recovery_for_a_linked_configured_folder() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("target")).unwrap();
        std::fs::create_dir_all(root.path().join("home/gate-runs/run")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("home/gate-runs/run/x"))
            .unwrap();
        let error = std::io::Error::other("refused");
        let text = evidence_refused(root.path(), "target/codeflow-gate/run", &error).to_string();
        assert!(text.contains("CARGO_TARGET_DIR or CODEFLOW_HOME"), "{text}");
        let text = evidence_refused(root.path(), "home/gate-runs/run/x/y", &error).to_string();
        assert!(!text.contains("CARGO_TARGET_DIR"), "{text}");
    }

    /// Grok review 2 sweep: a tracked folder replaced by a link is a change
    /// in the snapshot, never read through to the same bytes elsewhere.
    #[cfg(unix)]
    #[test]
    fn a_tracked_folder_replaced_by_a_link_changes_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "same").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let before = tracked(dir.path()).unwrap();
        let copy = tempfile::tempdir().unwrap();
        std::fs::write(copy.path().join("lib.rs"), "same").unwrap();
        std::fs::remove_dir_all(dir.path().join("src")).unwrap();
        std::os::unix::fs::symlink(copy.path(), dir.path().join("src")).unwrap();
        let after = tracked(dir.path()).unwrap();
        assert_ne!(before.get("src/lib.rs"), after.get("src/lib.rs"));
    }

    #[test]
    fn two_names_that_differ_only_in_invalid_bytes_keep_distinct_keys() {
        let root = Path::new("/repo");
        let (first, _) = tracked_entry(root, b"a\xe9");
        let (second, _) = tracked_entry(root, b"a\xff");
        assert_ne!(first, second);
        assert_eq!(tracked_entry(root, b"plain.txt").0, "plain.txt");
        // Review finding: a valid name that spells the lossy text and the
        // hex of an invalid one must not share its key.
        let (valid, _) = tracked_entry(root, "a\u{fffd} [bytes 61ff]".as_bytes());
        assert_ne!(second, valid);
    }

    /// Review finding: both names in one index must stay visible in the
    /// snapshot, so a change to one of them is a generation change.
    #[test]
    fn a_valid_name_that_spells_an_invalid_ones_key_keeps_its_own_entry() {
        let dir = repository_with_a_non_utf8_path();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        let id = repo.blob(b"other").unwrap();
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100_644,
                uid: 0,
                gid: 0,
                file_size: 5,
                id,
                flags: 0,
                flags_extended: 0,
                path: "caf\u{fffd} [bytes 636166e92e747874]".as_bytes().to_vec(),
            })
            .unwrap();
        index.write().unwrap();
        assert_eq!(tracked(dir.path()).unwrap().len(), 3);
    }

    /// Kept strict on purpose: git output that is not valid UTF-8 is an
    /// error, never a lossy spelling.
    #[test]
    fn git_output_that_is_not_utf8_is_an_error() {
        let dir = repository_with_a_non_utf8_path();
        assert!(git_output(dir.path(), &["ls-tree", "-r", "--name-only", "-z", "HEAD"]).is_err());
        assert!(git_output(dir.path(), &["rev-parse", "HEAD"]).is_ok());
    }

    /// Review finding: two link targets that differ only in invalid bytes must
    /// hash differently, so a producer that retargets a tracked link is a
    /// generation change. Runs where the file system accepts such a target.
    /// Review finding on issue 79: the evidence directory was named by a lossy
    /// spelling of the folder, so `caf` plus an invalid byte and `caf` plus a
    /// real U+FFFD shared one directory.
    #[cfg(unix)]
    #[test]
    fn two_folders_that_differ_only_in_an_invalid_byte_keep_their_own_evidence() {
        use std::os::unix::ffi::OsStrExt as _;
        let invalid = Path::new(std::ffi::OsStr::from_bytes(b"/no/such/caf\xff"));
        let replacement = Path::new("/no/such/caf\u{fffd}");
        assert_ne!(
            durable_relative(invalid),
            durable_relative(replacement),
            "one evidence directory for two folders"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_target_that_is_not_utf8_is_hashed_exactly() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o120_000,
                uid: 0,
                gid: 0,
                file_size: 4,
                id: repo.blob(b"base").unwrap(),
                flags: 0,
                flags_extended: 0,
                path: b"link".to_vec(),
            })
            .unwrap();
        index.write().unwrap();
        let link = dir.path().join("link");
        let target = |bytes: &[u8]| {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(bytes), &link)
        };
        if target(b"caf\xff").is_err() {
            return;
        }
        let first = tracked(dir.path()).unwrap();
        target(b"caf\xfe").unwrap();
        assert_ne!(first, tracked(dir.path()).unwrap());
    }
}

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_tracked_refuses_corrupt_repository_but_allows_no_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(tracked(dir.path()).unwrap().is_empty());
        std::fs::write(dir.path().join(".git"), "not a gitdir\n").unwrap();
        assert!(tracked(dir.path()).is_err());
    }
    #[test]
    fn r22_tracked_refuses_stat_failure_but_records_deletion() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/file"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("sub/file")).unwrap();
        index.write().unwrap();
        std::fs::remove_file(dir.path().join("sub/file")).unwrap();
        let deleted = tracked(dir.path()).unwrap();
        assert_eq!(deleted.len(), 1);
        // A folder that cannot be read is refused, never read as deleted.
        let sub = dir.path().join("sub");
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = tracked(dir.path());
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(unreadable.is_err());
        // A looping link in place of the folder is a change, never a
        // deletion (TSK-1044).
        std::fs::remove_dir(&sub).unwrap();
        std::os::unix::fs::symlink("sub", &sub).unwrap();
        let linked = tracked(dir.path()).unwrap();
        assert_ne!(linked, deleted);
    }
}
