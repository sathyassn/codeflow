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
#[derive(Debug, Default, Clone)]
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
        // OS text rule (issue 79): the repository's own folder names its
        // evidence, so it is hashed by its exact bytes. A lossy spelling would
        // let two folders share one evidence directory and one green base.
        .join(&digest(identity.as_os_str().as_encoded_bytes())[..16])
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

// Open once and inspect that descriptor before consuming local selection evidence.
#[cfg(unix)]
fn read_owned_record(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no preconditions and does not access memory.
    if !record_permissions_safe(metadata.uid(), metadata.mode(), unsafe { libc::geteuid() })
        || !metadata.is_file()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "run record must be owned by the effective user and not group- or world-writable",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(unix)]
fn record_permissions_safe(owner: u32, mode: u32, effective_user: u32) -> bool {
    owner == effective_user && mode & 0o022 == 0
}

#[cfg(not(unix))]
fn read_owned_record(_path: &Path) -> std::io::Result<Vec<u8>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "run record ownership protection unavailable",
    ))
}

/// Resolve the effective account independently of process-controlled HOME.
#[cfg(unix)]
fn account_home() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let mut storage = vec![0_u8; 16_384];
    loop {
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // SAFETY: the entry and buffer are writable for their stated sizes.
        // getpwuid_r stores its strings in this buffer, unlike getpwuid's shared storage.
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                entry.as_mut_ptr(),
                storage.as_mut_ptr().cast(),
                storage.len(),
                &raw mut result,
            )
        };
        if status == libc::ERANGE {
            storage.resize(storage.len().checked_mul(2)?, 0);
            continue;
        }
        if status != 0 || result.is_null() {
            return None;
        }
        // SAFETY: a successful lookup initialized entry; pw_dir is a terminated
        // string in storage, which remains alive until it has been copied.
        let directory = unsafe {
            let entry = entry.assume_init();
            if entry.pw_dir.is_null() {
                return None;
            }
            CStr::from_ptr(entry.pw_dir)
        };
        let path = PathBuf::from(OsStr::from_bytes(directory.to_bytes()));
        return path.is_absolute().then_some(path);
    }
}

#[cfg(not(unix))]
fn account_home() -> Option<PathBuf> {
    None
}

fn credit_home() -> Result<PathBuf, String> {
    if std::env::var_os("CODEFLOW_HOME").is_some() {
        return Err("selection credit denied: CODEFLOW_HOME is overridden".into());
    }
    let account = account_home().ok_or("selection credit denied: account home unavailable")?;
    if std::env::var_os("HOME").as_deref() != Some(account.as_os_str()) {
        return Err(
            "selection credit denied: HOME differs from the effective user's account home".into(),
        );
    }
    Ok(account.join(".codeflow"))
}

fn green_base(
    root: &Path,
    base: &str,
    config_digest: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let home = credit_home()?;
    let tree = git_output(
        root,
        &["rev-parse", "--verify", &format!("{base}^{{tree}}")],
    )?;
    let directory = durable_root(root, &home);
    let entries =
        std::fs::read_dir(directory).map_err(|_| "no qualifying run: evidence unavailable")?;
    let mut records = Vec::new();
    let mut reason = "no qualifying run for the base tree";
    for entry in entries {
        let Some(record) = entry
            .ok()
            .and_then(|entry| read_owned_record(&entry.path().join("run.json")).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        else {
            continue;
        };
        if record["git_tree"].as_str().is_none() {
            continue;
        }
        if record["git_tree"] != tree.strip_suffix('\n').unwrap_or(&tree) {
            continue;
        }
        if record["config_digest"] != config_digest {
            reason = "config digest changed";
            continue;
        }
        if record["passed"] != true {
            reason = "failed qualifying run";
            continue;
        }
        if record["complete"] != true || record["mode"] != "full" {
            reason = "partial qualifying run";
            continue;
        }
        if record["clean"] != true {
            reason = "dirty qualifying run";
            continue;
        }
        if record["generation_unchanged"] != true {
            reason = "inputs changed during qualifying run";
            continue;
        }
        records.push(record);
    }
    if records.is_empty() {
        Err(reason.into())
    } else {
        Ok(records)
    }
}

fn comparison_base(root: &Path, options: &GateOptions) -> Result<(String, String), String> {
    if let Some(base) = &options.since {
        let sha = git_output(
            root,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )?;
        return Ok((
            sha.strip_suffix('\n').unwrap_or(&sha).into(),
            format!("explicit --since {base}"),
        ));
    }
    let repo = git2::Repository::discover(root)
        .map_err(|_| "no comparison base: repository unavailable")?;
    let head = repo
        .head()
        .map_err(|_| "no comparison base: HEAD unavailable")?;
    if repo.head_detached().unwrap_or(true) {
        return Err("no comparison base: detached HEAD has no task".into());
    }
    let branch_name = crate::git::name::reference_shorthand(&head);
    let branch = branch_name
        .rule_text()
        .map_err(|_| "no comparison base: branch is not UTF-8")?;
    let task = crate::workgraph::work_start::task_id_from_branch(root, branch)
        .map_err(|error| format!("cannot resolve task: {error}"))?
        .ok_or("no comparison base: branch has no task")?;
    let target = crate::workgraph::declared_work_target(root, &task)
        .map_err(|error| format!("cannot resolve task line: {error}"))?
        .ok_or("no comparison base: task has no declared integration target")?;
    let resolved = crate::workgraph::work_start::resolve_work_target_checked(root, Some(&target))
        .map_err(|error| format!("cannot resolve task line: {error}"))?
        .ok_or("no comparison base: task line unavailable")?;
    let base = git_output(root, &["merge-base", "HEAD", &resolved.target])?;
    Ok((
        base.strip_suffix('\n').unwrap_or(&base).into(),
        format!(
            "merge base of HEAD with {task} integration target {target} ({})",
            resolved.target
        ),
    ))
}

/// Only exact, declared content evidence can remove an eligible check.
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
        excluded: BTreeSet::new(),
        reason: reason.into(),
        reasons: BTreeMap::new(),
    };
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
    if options.all {
        return all("--all: every target");
    }
    let (base, found) = match comparison_base(root, options) {
        Ok(base) => base,
        Err(reason) => return all(&reason),
    };
    let context = format!("comparison base {base}: {found}");
    let mut selection = select_base(root, targets, execution, mode, home, config_digest, &base);
    selection.reason = format!("{context}; {}", selection.reason);
    selection
}

fn select_base(
    root: &Path,
    targets: &[TargetConfig],
    execution: &ExecutionConfig,
    mode: &str,
    home: Option<&Path>,
    config_digest: &str,
    base: &str,
) -> Selection {
    let eligible: BTreeSet<String> = targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key(mode))
        .map(|t| t.name.clone())
        .collect();
    let all = |reason: &str| Selection {
        selected: eligible.clone(),
        skipped: BTreeSet::new(),
        excluded: BTreeSet::new(),
        reason: reason.into(),
        reasons: BTreeMap::new(),
    };
    if git_output(root, &["merge-base", "--is-ancestor", base, "HEAD"]).is_err() {
        return all("unproven ancestry");
    }
    let records = match green_base(root, base, config_digest) {
        Ok(records) => records,
        Err(reason) => return all(&reason),
    };
    let delta = match git_output(
        root,
        &["diff", "--name-status", "-z", "--find-renames", base, "--"],
    ) {
        Ok(delta) => delta,
        Err(reason) => return all(&reason),
    };
    let untracked = match git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"]) {
        Ok(paths) => paths,
        Err(reason) => return all(&reason),
    };
    if !untracked.is_empty() {
        return all("untracked input: every target");
    }
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
    if paths
        .iter()
        .any(|path| matches(&execution.run_everything, path))
    {
        return all("shared infrastructure changed");
    }
    if paths
        .iter()
        .any(|path| !targets.iter().any(|t| matches(&t.narrow, path)))
    {
        return all("unmatched input: every target");
    }
    let mut selected = BTreeSet::new();
    let mut reasons = BTreeMap::new();
    for target in targets.iter().filter(|t| eligible.contains(&t.name)) {
        if target.input_class != Some(super::config::InputClass::Content) {
            selected.insert(target.name.clone());
            reasons.insert(
                target.name.clone(),
                format!("input class {:?} requires execution", target.input_class),
            );
            continue;
        }
        let now = super::evidence::observe(root, target, targets, home);
        let mut reason = now
            .unavailable
            .clone()
            .unwrap_or_else(|| "missing target evidence".into());
        let mut proven = false;
        if let Some(now) = now.identity {
            for record in &records {
                match target_covered(record, target, targets, mode, &now) {
                    Ok(()) => {
                        proven = true;
                        break;
                    }
                    Err(missing) => reason = missing,
                }
            }
        }
        if proven {
            reason = "unchanged declared check covered by full candidate evidence".into();
        } else {
            selected.insert(target.name.clone());
        }
        reasons.insert(target.name.clone(), reason);
    }
    include_affected_graph(targets, &eligible, &mut selected, &mut reasons);
    Selection {
        excluded: BTreeSet::new(),
        skipped: eligible.difference(&selected).cloned().collect(),
        selected,
        reason: "per-target evidence against the base tree".into(),
        reasons,
    }
}

fn include_affected_graph(
    targets: &[TargetConfig],
    eligible: &BTreeSet<String>,
    selected: &mut BTreeSet<String>,
    reasons: &mut BTreeMap<String, String>,
) {
    // A producer that will run may change its outputs. Its consumers cannot
    // take credit from the outputs that happen to exist before that execution.
    loop {
        let previous = selected.clone();
        for target in targets.iter().filter(|t| eligible.contains(&t.name)) {
            if target.requires.iter().any(|name| selected.contains(name))
                && selected.insert(target.name.clone())
            {
                reasons.insert(target.name.clone(), "producer requires execution".into());
            }
        }
        *selected = with_prerequisites(targets, selected.clone());
        if *selected == previous {
            break;
        }
    }
    for name in selected.iter() {
        if reasons
            .get(name)
            .is_some_and(|reason| reason.starts_with("unchanged"))
        {
            reasons.insert(name.clone(), "required prerequisite".into());
        }
    }
}

fn target_covered(
    record: &serde_json::Value,
    target: &TargetConfig,
    targets: &[TargetConfig],
    mode: &str,
    now: &super::evidence::Identity,
) -> Result<(), String> {
    for skipped in [&record["selection"]["skipped"], &record["ci_skipped"]] {
        if !skipped.as_array().is_some_and(Vec::is_empty) {
            return Err("partial qualifying run: skipped evidence".into());
        }
    }
    let full: BTreeSet<_> = targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key("full"))
        .map(|t| t.name.as_str())
        .collect();
    let results = record["results"]
        .as_array()
        .ok_or("missing execution results")?;
    let executed: BTreeSet<_> = results.iter().filter_map(|r| r["name"].as_str()).collect();
    if full != executed || results.len() != full.len() {
        return Err("partial qualifying run: execution membership differs".into());
    }
    let executions = record["target_evidence"]
        .as_array()
        .ok_or("missing target evidence")?;
    if executions
        .iter()
        .any(|e| !targets.iter().any(|t| e["name"] == t.name))
    {
        return Err("unknown declared target in evidence".into());
    }
    let value = executions
        .iter()
        .find(|e| e["name"] == target.name)
        .ok_or("missing target evidence")?;
    let evidence: super::evidence::Execution =
        serde_json::from_value(value.clone()).map_err(|_| "missing or corrupt target evidence")?;
    if evidence.git_tree.as_deref() != record["git_tree"].as_str() {
        return Err("target Git tree evidence differs".into());
    }
    if !record["results"].as_array().is_some_and(|results| {
        results
            .iter()
            .any(|r| r["name"] == target.name && r["exit_code"] == 0 && r["error"].is_null())
    }) {
        return Err("missing passed target execution".into());
    }
    if evidence.command != target.modes[mode].command {
        return Err("mode-specific command differs".into());
    }
    if evidence.mode != mode
        && !(evidence.mode == "full"
            && target
                .evidence
                .as_ref()
                .is_some_and(|e| e.covers_modes.iter().any(|m| m == mode)))
    {
        return Err("mode is not covered by the qualifying execution".into());
    }
    let before = evidence.before.identity.ok_or_else(|| {
        evidence
            .before
            .unavailable
            .unwrap_or_else(|| "missing before evidence".into())
    })?;
    let after = evidence.after.identity.ok_or_else(|| {
        evidence
            .after
            .unavailable
            .unwrap_or_else(|| "missing after evidence".into())
    })?;
    if before != after {
        return Err("inputs or dependencies changed during qualifying run".into());
    }
    if let Some(reason) = super::evidence::mismatch(&before, now) {
        return Err(reason.into());
    }
    Ok(())
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
/// The output decides which targets a run may skip, and an error makes every
/// caller select every target, so a path that is not valid UTF-8 widens the
/// run and never narrows it. A lossy path could match a `narrow` pattern the
/// real bytes do not and drop a check the change owes.
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

    #[cfg(unix)]
    #[test]
    fn green_base_uses_git_tree_identity_across_commits_and_rejects_legacy_evidence() {
        let root = tempfile::tempdir().unwrap();
        let home = account_home().expect("account home").join(".codeflow");
        let repo = git2::Repository::init(root.path()).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let signature = git2::Signature::now("Test", "test@example.com").unwrap();
        let candidate = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "candidate",
                &tree,
                &[],
            )
            .unwrap();
        let merged = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "merge",
                &tree,
                &[&repo.find_commit(candidate).unwrap()],
            )
            .unwrap();
        assert_ne!(candidate, merged);
        let directory = durable_root(root.path(), home.as_path()).join("candidate");
        std::fs::create_dir_all(&directory).unwrap();
        let mut record = serde_json::json!({"git_tree":tree.id().to_string(),"revision":candidate.to_string(),
            "passed":true,"complete":true,"clean":true,"mode":"full","generation_unchanged":true,"config_digest":"config"});
        std::fs::write(
            directory.join("run.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(
            green_base(root.path(), &merged.to_string(), "config")
                .map_or(0, |records| records.len()),
            usize::from(cfg!(unix))
        );
        record.as_object_mut().unwrap().remove("git_tree");
        std::fs::write(
            directory.join("run.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(green_base(root.path(), &merged.to_string(), "config")
            .unwrap_err()
            .contains("no qualifying run for the base tree"));
        std::fs::remove_dir_all(directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn selection_finds_the_declared_task_line_without_evidence_and_explains_the_fallback() {
        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("project-management/tasks")).unwrap();
        std::fs::write(
            root.path().join("project-management/tasks/TSK-001.md"),
            "---\nid: TSK-001\nintegration_target: integration/checks\n---\n",
        )
        .unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["."], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Test", "test@example.com").unwrap();
        let id = repo
            .commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
            .unwrap();
        let commit = repo.find_commit(id).unwrap();
        repo.branch("integration/checks", &commit, false).unwrap();
        repo.branch("task/TSK-001-selection", &commit, false)
            .unwrap();
        repo.set_head("refs/heads/task/TSK-001-selection").unwrap();
        let target: TargetConfig = serde_json::from_value(serde_json::json!({"name":"check","runner":"custom","modes":{"quick":{"command":"echo check"}}})).unwrap();
        let selected = select(
            root.path(),
            &[target],
            &ExecutionConfig::default(),
            "quick",
            &GateOptions::default(),
            None,
            "config",
        );
        assert_eq!(selected.selected, BTreeSet::from(["check".into()]));
        assert!(selected
            .reason
            .contains("TSK-001 integration target integration/checks"));
        assert!(selected.reason.contains(&id.to_string()));
        let reason = if cfg!(unix) {
            "no qualifying run"
        } else {
            "selection credit denied: account home unavailable"
        };
        assert!(selected.reason.contains(reason), "{}", selected.reason);
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

    /// Kept strict on purpose: git output that is not valid UTF-8 is `None`,
    /// which every caller reads as "select every target".
    #[test]
    fn git_output_that_is_not_utf8_selects_every_target() {
        let dir = repository_with_a_non_utf8_path();
        assert!(git_output(dir.path(), &["ls-tree", "-r", "--name-only", "-z", "HEAD"]).is_err());
        assert!(git_output(dir.path(), &["rev-parse", "HEAD"]).is_ok());
    }

    /// Review finding: two link targets that differ only in invalid bytes must
    /// hash differently, so a producer that retargets a tracked link is a
    /// generation change. Runs where the file system accepts such a target.
    /// Review finding on issue 79: the evidence directory was named by a lossy
    /// spelling of the folder, so `caf` plus an invalid byte and `caf` plus a
    /// real U+FFFD shared one directory and one green base.
    #[cfg(unix)]
    #[test]
    fn two_folders_that_differ_only_in_an_invalid_byte_keep_their_own_evidence() {
        use std::os::unix::ffi::OsStrExt as _;
        let home = Path::new("/home/example");
        let invalid = Path::new(std::ffi::OsStr::from_bytes(b"/no/such/caf\xff"));
        let replacement = Path::new("/no/such/caf\u{fffd}");
        assert_ne!(
            durable_root(invalid, home),
            durable_root(replacement, home),
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
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/file"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("sub/file")).unwrap();
        index.write().unwrap();
        std::fs::remove_file(dir.path().join("sub/file")).unwrap();
        assert_eq!(tracked(dir.path()).unwrap().len(), 1);
        std::fs::remove_dir(dir.path().join("sub")).unwrap();
        std::os::unix::fs::symlink("sub", dir.path().join("sub")).unwrap();
        assert!(tracked(dir.path()).is_err());
    }
}

#[cfg(all(test, unix))]
mod record_protection_tests {
    #[test]
    fn repair_record_owner_and_write_permissions_are_required() {
        assert!(super::record_permissions_safe(1000, 0o600, 1000));
        assert!(super::record_permissions_safe(1000, 0o644, 1000));
        assert!(!super::record_permissions_safe(1001, 0o600, 1000));
        assert!(!super::record_permissions_safe(1000, 0o620, 1000));
        assert!(!super::record_permissions_safe(1000, 0o602, 1000));
    }
}
