//! Guards for heavy test work: admission by host capacity, and no cargo
//! target directory shared between worktrees.
//!
//! **Heavy runs share the host** (TSK-1145). Full and essential gates,
//! heavy quick targets and `codeflow reserve` each take one free *slot*
//! among the slot locks of a lock folder, and hold the folder's *host lock*
//! shared. No run waits for another: a run takes the first free slot, and
//! the number of busy slots tells it how many heavy runs share the host, so
//! it can size its job count ([`super::capacity`]). Only a target that
//! declares `host_exclusive` takes the host lock exclusively
//! ([`hold_host_exclusive`]); it starts when no other heavy run holds the
//! host, and no heavy run starts while it runs. Locks are OS locks (`flock`
//! on Unix, byte-range locks on Windows; the OS releases them when the
//! process exits, however it exits) in up to two folders:
//!
//! - the machine-wide folder under the `CodeFlow` home (`CODEFLOW_HOME`,
//!   else `~/.codeflow`), which spans every repository on the machine;
//! - the repository folder under the git common directory, which spans
//!   every worktree of the repository.
//!
//! A run takes one slot and the shared host lock in every configured folder
//! before targets start. Slot `0` is [`LOCK_FILE`], the file older binaries
//! lock exclusively; slot `n` is `full-gate-<n>.lock`. A lock that cannot
//! be opened or a record that cannot be read refuses, naming the path and
//! error (fail closed). The shipped Claude settings presets allow sandboxed
//! writes to `~/.codeflow/locks` and `~/.codeflow/gate-runs` (TSK-216), and
//! `codeflow doctor --check permissions` names any folder this process
//! cannot write ([`unwritable_gate_dirs`]). A record that still names a
//! holder while its slot is free was left by a process that died, and is
//! reclaimed.
//!
//! **Lock folders may be writable by a sandboxed seat** (TSK-1003), so a
//! lock file is never written: it is opened read only, without following
//! links, and only locked. Each holder record (`full-gate.record` beside
//! slot 0, `full-gate-<n>.record` beside slot `n`, `host.record` beside the
//! host lock) is replaced whole by a new file renamed into place, read
//! without following links, and shown only through validated values
//! (numbers, a known mode, and a folder only when its text names one the
//! gate already trusts; a planted folder value is never resolved). A link
//! planted in the folder can be locked or replaced, never written through,
//! and reveals nothing beyond a folder the gate already trusts. A run that
//! cannot write its record refuses instead of running uncoordinated. The
//! folder stays an advisory coordination boundary: a seat that can write it
//! can block or split coordination between runs. A record a pre-3.2 gate
//! keeps inside the lock file is still read while the record file names no
//! holder.
//!
//! **Target lifetime on Unix.** Heavy quick targets and reserved commands
//! have a watcher that ends their process group when the controller exits;
//! it keeps copies of the run's slot and host locks, so a target that
//! outlives its run keeps its slot busy and keeps a host-exclusive target
//! out. Other targets can outlive a killed gate. While a run holds a slot,
//! the runner records each running target's process group in the slot's
//! record ([`target_group_started`]). A free slot whose record names a
//! process group with a live process counts as busy.
//!
//! **On Windows they end with it** (TSK-142). Each target runs in a
//! kill-on-close job object whose only handle the gate process holds, so
//! the OS ends the target's tree when it closes that handle as the gate
//! exits. No group is recorded there.
//!
//! **A command under `codeflow reserve` shares its slot** (TSK-1128). On
//! Unix, `codeflow reserve` hands a `codeflow` command duplicates of its
//! locked lock descriptors and names them in [`RESERVED_LOCKS_ENV`]. The
//! name grants nothing: the receiving process marks them close-on-exec at
//! start ([`adopt_reserved_locks`]), and quick admission and a nested
//! reserve use them only after a slot path's file identity matches a
//! descriptor, a fresh open of that path is refused by the OS lock, and the
//! descriptor itself takes it, which only the holder's own open file
//! description can. Anything else falls back to ordinary admission, which
//! takes a slot of its own. Full and essential gates never inherit.
//!
//! **Target directory.** `CARGO_TARGET_DIR` outside the worktree lets builds
//! in parallel worktrees overwrite each other's binaries. A shared directory
//! is also a legitimate way to save disk, so [`check_cargo_target_dir`]
//! returns a warning, never a refusal.

use crate::bounded_file::FileIdentity;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// File name of slot 0 in each lock directory, the file older binaries
/// lock exclusively. Slot files are only ever opened read only and locked;
/// `CodeFlow` never writes, truncates or deletes them (TSK-1003).
pub const LOCK_FILE: &str = "full-gate.lock";

/// File name of the holder record beside slot 0. Every lock's record is
/// its file name with `.lock` replaced by `.record`. A record is replaced
/// whole: written to a new file created exclusively under a random name in
/// the same folder, then renamed over this name. An existing record is
/// never opened for writing, so a link planted at this name is replaced,
/// never written through (TSK-1003).
pub const RECORD_FILE: &str = "full-gate.record";

/// File name of the host lock in each lock directory: every heavy run
/// holds it shared, and a `host_exclusive` target holds it exclusively
/// (TSK-1145).
pub const HOST_LOCK_FILE: &str = "host.lock";

/// File name of slot `index`: [`LOCK_FILE`] for slot 0, else
/// `full-gate-<index>.lock`. A lock directory has no fixed number of slots:
/// a run that finds every existing slot busy creates the next one, so no run
/// count refuses a run (R483-06).
#[must_use]
pub fn slot_file(index: usize) -> String {
    if index == 0 {
        LOCK_FILE.to_string()
    } else {
        format!("full-gate-{index}.lock")
    }
}

/// The slot index a lock file name stands for, if it names a slot.
fn slot_index(name: &str) -> Option<usize> {
    if name == LOCK_FILE {
        return Some(0);
    }
    let digits = name.strip_prefix("full-gate-")?.strip_suffix(".lock")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&index| index > 0)
}

/// The indices of the slot files that exist in `dir`, ascending; empty when
/// the directory does not exist yet. Each slot is still opened through the
/// anchored, no-follow walk before it is trusted.
fn slot_indices(dir: &Path) -> Result<Vec<usize>, LockHeld> {
    let unavailable = |error: std::io::Error| {
        LockHeld::unavailable(
            dir.join(LOCK_FILE),
            format!("gate lock unavailable: cannot list the lock folder: {error}"),
        )
    };
    if crate::absence::proven_absent(dir).map_err(unavailable)? {
        return Ok(Vec::new());
    }
    let mut indices = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(unavailable)? {
        let entry = entry.map_err(unavailable)?;
        if let Some(index) = entry.file_name().to_str().and_then(slot_index) {
            indices.push(index);
        }
    }
    indices.sort_unstable();
    Ok(indices)
}

/// A heavy run's admission. Its locks release when this value drops (or
/// when the process exits); a clean drop also clears its slot records.
#[derive(Debug)]
pub struct GateLock {
    id: u64,
    files: Vec<AcquiredFile>,
    /// Descriptors of a slot an enclosing `codeflow reserve` holds. They
    /// are never unlocked here: unlocking would release the holder's slot.
    inherited: Vec<File>,
    /// The slot lock files held, whose records the drop clears.
    paths: Vec<PathBuf>,
    /// Heavy runs that share the host, this one included: the busy slots
    /// of the most crowded lock directory when the slot was taken.
    pub runs: usize,
    /// Degradations and reclaimed stale slots, for the caller to print.
    pub notes: Vec<String>,
}

/// A lock held by this process, whose record the runner extends with the
/// process groups of running targets.
struct Held {
    id: u64,
    /// The slot lock files held, each with its record beside it.
    paths: Vec<PathBuf>,
    record: String,
    groups: BTreeSet<u32>,
    /// Every group this lock has written, so a group another process added
    /// (a nested run's target, or the reserve's command) is kept on rewrite.
    recorded: BTreeSet<u32>,
    /// A lock an enclosing reserve holds: its record's holder lines stay the
    /// holder's, and this process only adds and removes its own groups.
    inherited: bool,
    /// The host locks this run holds shared, one per lock directory, with
    /// their paths; an inherited lock takes its own as well.
    host: Vec<(PathBuf, AcquiredFile)>,
    /// The folders a record's `dir` may name, for re-reading the record.
    trusted: Vec<String>,
    /// [`GateLock::runs`] of this lock.
    runs: usize,
    #[cfg(unix)]
    descriptors: Vec<std::os::fd::RawFd>,
}

static HELD: Mutex<Vec<Held>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn held() -> std::sync::MutexGuard<'static, Vec<Held>> {
    HELD.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The heavy runs sharing the host as this process's admission saw them,
/// when it holds one: the most any of its locks counted.
#[must_use]
pub fn held_runs() -> Option<usize> {
    held().iter().map(|lock| lock.runs).max()
}

impl Drop for GateLock {
    fn drop(&mut self) {
        // Deregister first, under the registry lock, so no target update can
        // rewrite a record after it is cleared. The entry's host locks
        // release when it drops, after the records are cleared.
        let entry = {
            let mut held = held();
            held.iter()
                .position(|h| h.id == self.id)
                .map(|index| held.remove(index))
        };
        if !self.inherited.is_empty() {
            // The record is the holder's. Groups of this process's targets
            // that still run stay on it, so the slot stays busy while they
            // live (TSK-1128); finished ones were removed as they ended.
            return;
        }
        for path in &self.paths {
            // Best effort, before the locks release: an uncleared record only
            // reads as a stale slot, which the next run reclaims. An empty
            // record names no holder, so readers fall back to a legacy record
            // inside the lock file, which a pre-3.2 gate may have left. Live
            // groups a nested run added stay, so the slot is busy while they
            // run (TSK-1128).
            let clear = || {
                let kept = entry
                    .as_ref()
                    .and_then(|entry| foreign_live_groups(path, entry).ok())
                    .unwrap_or_default();
                let mut text = String::new();
                for group in kept {
                    let _ = writeln!(text, "group={group}");
                }
                let _ = write_record(path, &text);
            };
            // Without the record lock the rewrite still runs: the record is
            // best effort here, and an unlocked rewrite is the old floor.
            if with_record_lock(path, clear).is_err() {
                clear();
            }
        }
        drop(entry);
        // AcquiredFile explicitly unlocks before its descriptor closes.
    }
}

/// The record beside `path` split into its holder lines and its groups.
fn record_parts(path: &Path, trusted: &[String]) -> std::io::Result<(String, BTreeSet<u32>)> {
    let text = read_holder(path, None, trusted)?;
    let mut base = String::new();
    let mut groups = BTreeSet::new();
    for line in text.split_terminator('\n') {
        if let Some(value) = line.strip_prefix("group=") {
            if let Ok(group) = value.parse() {
                groups.insert(group);
            }
        } else {
            let _ = writeln!(base, "{line}");
        }
    }
    Ok((base, groups))
}

/// Live groups on the record beside `path` that `entry` never wrote.
fn foreign_live_groups(path: &Path, entry: &Held) -> std::io::Result<Vec<u32>> {
    let (_, groups) = record_parts(path, &entry.trusted)?;
    Ok(groups
        .into_iter()
        .filter(|group| !entry.recorded.contains(group) && group_alive(*group))
        .collect())
}

/// Why heavy work may not start.
#[derive(Debug)]
pub struct LockHeld {
    /// The lock file that is held or unavailable.
    pub path: PathBuf,
    /// The holder record of that lock (empty when the holder has not
    /// written it yet), or why the lock is unavailable.
    pub holder: String,
    /// Process groups of targets still running after their run exited.
    pub running_groups: Vec<u32>,
    /// A `host_exclusive` target holds the host, or one is refused because
    /// other heavy runs hold it.
    pub exclusive: bool,
}

impl LockHeld {
    fn unavailable(path: PathBuf, holder: String) -> Self {
        Self {
            path,
            holder,
            running_groups: Vec::new(),
            exclusive: false,
        }
    }
}

impl std::fmt::Display for LockHeld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lock = crate::git::display_path(&self.path);
        if !self.exclusive {
            return write!(f, "{} (lock: {lock})", self.holder);
        }
        let holder = if self.holder.trim().is_empty() {
            "a process that has not recorded itself yet".to_string()
        } else {
            describe_holder(&self.holder)
        };
        write!(
            f,
            "a host-exclusive target is running on this machine: {holder}. Heavy runs \
             start when it ends (lock: {lock})"
        )
    }
}

/// The directory under the `CodeFlow` home that holds the machine-wide lock.
pub const HOME_LOCK_DIR: &str = "locks";

/// The directory under the `CodeFlow` home that keeps full-run evidence.
pub const HOME_EVIDENCE_DIR: &str = "gate-runs";

/// Lock directories for a project: the machine-wide one under `home` (when
/// known) and the repository one under the git common directory (when the
/// project is in a git repository).
/// # Errors
/// Refuses repository discovery failures; only `NotFound` means no repository.
pub fn lock_dirs(project_dir: &Path, home: Option<&Path>) -> Result<Vec<PathBuf>, git2::Error> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(HOME_LOCK_DIR));
    }
    match git2::Repository::discover(project_dir) {
        Ok(repo) => dirs.push(repo.commondir().join("codeflow")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(dirs)
}

/// The directories a full gate writes outside the worktree that this
/// process cannot write, each with its error: every lock directory and the
/// evidence directory under `home`. Each lock file is opened as the gate
/// opens it, without taking the lock, so a running gate is not disturbed;
/// the evidence directory is probed with a file that is removed again.
#[must_use]
pub fn unwritable_gate_dirs(
    project_dir: &Path,
    home: Option<&Path>,
) -> Vec<(PathBuf, std::io::Error)> {
    let mut unwritable = Vec::new();
    let dirs = match lock_dirs(project_dir, home) {
        Ok(dirs) => dirs,
        Err(error) => {
            return vec![(
                project_dir.to_path_buf(),
                std::io::Error::other(error.to_string()),
            )]
        }
    };
    for dir in dirs {
        if let Err(error) = probe_lock_dir(&dir) {
            unwritable.push((dir, error));
        }
    }
    if let Some(home) = home {
        if let Err(error) = probe_evidence_dir(home) {
            unwritable.push((home.join(HOME_EVIDENCE_DIR), error));
        }
    }
    unwritable
}

/// Whether a gate could store durable evidence under `home`: the evidence
/// folder is created and a disposable file created and removed in it,
/// through the anchored walk the durable copy uses, so a link or junction
/// at the folder or at the probe's name is refused, never followed
/// (TSK-1044). The walk starts at the home's parent, so a link at the home
/// folder itself is refused too.
fn probe_evidence_dir(home: &Path) -> std::io::Result<()> {
    let (parent, leaf) = crate::contained::split_leaf(home)?;
    std::fs::create_dir_all(parent)?;
    let tree = crate::contained::Tree::open(parent)?;
    let probe = format!(
        "{leaf}/{HOME_EVIDENCE_DIR}/.write-probe-{}",
        std::process::id()
    );
    tree.create_new(&probe, b"")?;
    tree.remove(&probe)
}

/// Whether a gate could use the lock folder `dir`: it needs the lock file
/// and a holder record it can create and rename into place. Both are
/// probed, the record with a disposable name through the same anchored
/// write, removed again (TSK-1003). The lock itself is never taken.
fn probe_lock_dir(dir: &Path) -> std::io::Result<()> {
    let lock = dir.join(LOCK_FILE);
    let probe = dir.join(format!(".record-probe-{}", std::process::id()));
    open_lock_file(&lock)
        .map(drop)
        .and_then(|()| open_lock_file(&dir.join(HOST_LOCK_FILE)).map(drop))
        .and_then(|()| {
            let (anchor, relative) = lock_anchor(&probe)?;
            let tree = crate::contained::Tree::open(anchor)?;
            tree.write(&relative, b"")?;
            tree.remove(&relative)
        })
}

/// What a builder seat has to write outside its worktree (TSK-337), each
/// folder with its result: both gate lock folders (`locks` under `home` and
/// `codeflow` under the common git directory), probed as
/// [`unwritable_gate_dirs`] probes them, and the common git directory's
/// `objects` and `refs`, which a commit writes, each probed by creating and
/// removing a file whose dot-led name git never reads as an object or a
/// ref, through an anchored write that follows no link. The gate evidence
/// folder is not probed: builder seats run no full gate.
#[must_use]
pub fn builder_write_probe(
    project_dir: &Path,
    home: Option<&Path>,
) -> Vec<(PathBuf, std::io::Result<()>)> {
    let dirs = match lock_dirs(project_dir, home) {
        Ok(dirs) => dirs,
        Err(error) => {
            return vec![(
                project_dir.to_path_buf(),
                Err(std::io::Error::other(error.to_string())),
            )]
        }
    };
    let mut results: Vec<(PathBuf, std::io::Result<()>)> = dirs
        .iter()
        .map(|dir| (dir.clone(), probe_lock_dir(dir)))
        .collect();
    if home.is_none() {
        results.insert(
            0,
            (
                PathBuf::from(HOME_LOCK_DIR),
                Err(std::io::Error::other(
                    "no CodeFlow home folder, so its lock folder was not probed",
                )),
            ),
        );
    }
    let common = match git2::Repository::discover(project_dir) {
        Ok(repo) => repo.commondir().to_path_buf(),
        Err(error) => {
            results.push((
                project_dir.to_path_buf(),
                Err(std::io::Error::other(format!(
                    "no git repository, so the objects and refs folders were not probed: {error}"
                ))),
            ));
            return results;
        }
    };
    for folder in ["objects", "refs"] {
        let relative = format!("{folder}/.codeflow-write-probe-{}", std::process::id());
        let result = crate::contained::Tree::open(&common).and_then(|tree| {
            tree.create_new(&relative, b"")?;
            tree.remove(&relative)
        });
        results.push((common.join(folder), result));
    }
    results
}

/// Read holder records without taking a lock, changing a record, or creating directories:
/// the host lock's and every existing slot's. These are snapshots: admission still checks
/// the OS locks and live target groups.
/// # Errors
/// Refuses discovery failures and unreadable or oversized records.
pub fn gate_lock_status(root: &Path) -> Result<Vec<String>, String> {
    let dirs = lock_dirs(root, crate::registry::codeflow_home().as_deref())
        .map_err(|error| format!("cannot discover gate lock directories: {error}"))?;
    let trusted = trusted_dirs(root);
    let mut lines = Vec::new();
    for dir in dirs {
        lines.push(holder_status(&dir.join(HOST_LOCK_FILE), &trusted)?);
        let mut indices = slot_indices(&dir).map_err(|held| held.to_string())?;
        if indices.first() != Some(&0) {
            indices.insert(0, 0);
        }
        for index in indices {
            lines.push(holder_status(&dir.join(slot_file(index)), &trusted)?);
        }
    }
    Ok(lines)
}

/// One status line for the lock at `path`: its holder's validated values, or
/// that none is recorded.
fn holder_status(path: &Path, trusted: &[String]) -> Result<String, String> {
    let record = read_holder(path, None, trusted).map_err(|error| {
        format!(
            "cannot read gate lock {}: {error}",
            crate::git::display_path(path)
        )
    })?;
    let description = if record.trim().is_empty() {
        "no recorded holder".to_string()
    } else {
        let started = record
            .lines()
            .find_map(|line| line.strip_prefix("started="))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or_else(|| "unknown".to_string(), |value| value.to_string());
        format!(
            "recorded holder: {}; started_unix={started}",
            crate::git::display_line(&describe_holder(&record))
        )
    };
    Ok(format!("{}: {description}", crate::git::display_path(path)))
}

/// Admit a heavy run of `root` in `mode`, with the operator's repair pointer.
/// # Errors
/// Refuses unavailable directories and unreadable records, and while a
/// `host_exclusive` target holds the host. Never refuses because other heavy
/// runs are active. Quick admission and `codeflow reserve` call this; under
/// an enclosing `codeflow reserve` they first use its slot
/// ([`RESERVED_LOCKS_ENV`]). Full and essential gates admit themselves
/// through [`acquire_gate_lock`] and never inherit.
pub fn acquire_project_gate_lock(root: &Path, mode: &str) -> Result<GateLock, String> {
    let dirs = project_lock_dirs(root)?;
    #[cfg(unix)]
    {
        let adopted = adopted().clone();
        if let Some(lock) = inherit_gate_lock(&dirs, &adopted, trusted_dirs(root))
            .map_err(|held| with_repair_pointer(&held))?
        {
            return Ok(lock);
        }
    }
    acquire_gate_lock(&dirs, root, mode).map_err(|held| with_repair_pointer(&held))
}

/// A refusal's text, with the permissions pointer when a lock is unavailable.
#[must_use]
pub fn with_repair_pointer(held: &LockHeld) -> String {
    let message = held.to_string();
    if message.contains("gate lock unavailable") {
        format!("{message}; run `codeflow doctor --check permissions` to identify the directories to repair")
    } else {
        message
    }
}

/// The lock directories of `root`, with the operator's repair pointer.
/// # Errors
/// When the repository cannot be discovered.
pub fn project_lock_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    lock_dirs(root, crate::registry::codeflow_home().as_deref())
        .map_err(|error| format!("gate lock unavailable: cannot discover repository: {error}; run `codeflow doctor --check permissions`"))
}

/// Test-only interleaving points in lock acquisition, so a test can change
/// the lock folder at the moment a racing seat could (TSK-1003).
#[cfg(test)]
pub(crate) mod interleave {
    use std::cell::RefCell;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Point {
        /// The lock file is open, before any check on it.
        Opened,
        /// The OS lock is taken.
        Locked,
        /// A record rewrite has read the record and not yet written it.
        RecordRead,
    }

    type Step = Box<dyn FnMut(Point)>;

    thread_local! {
        static STEP: RefCell<Option<Step>> = const { RefCell::new(None) };
    }

    // Only the Unix interleaving tests set a step.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn set(step: impl FnMut(Point) + 'static) {
        STEP.with(|cell| *cell.borrow_mut() = Some(Box::new(step)));
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn clear() {
        STEP.with(|cell| *cell.borrow_mut() = None);
    }

    pub(super) fn at(point: Point) {
        STEP.with(|cell| {
            if let Some(step) = cell.borrow_mut().as_mut() {
                step(point);
            }
        });
    }
}

/// The outcome of trying one slot without waiting.
enum Slot {
    /// The slot is ours, locked and checked.
    Taken(File),
    /// Another run holds it.
    Busy,
}

/// Open the lock file at `path` and take its OS lock (shared or exclusive)
/// without waiting: the locked file, `Busy` when another process holds it,
/// or why the lock is unavailable.
fn try_open_and_lock(path: &Path, shared: bool) -> Result<Slot, LockHeld> {
    let path = path.to_path_buf();
    let file = match open_lock_file(&path) {
        Ok(file) => file,
        Err(error) => {
            return Err(LockHeld::unavailable(
                path,
                format!("gate lock unavailable: {error}"),
            ));
        }
    };
    let locked = if shared {
        os_lock::try_lock_shared(&file)
    } else {
        os_lock::try_lock(&file)
    };
    match locked {
        Ok(()) => {
            #[cfg(test)]
            interleave::at(interleave::Point::Locked);
            if let Err(error) = same_lock_file(&path, &file) {
                os_lock::unlock(&file);
                return Err(LockHeld::unavailable(
                    path,
                    format!("gate lock unavailable: {error}; retry"),
                ));
            }
            Ok(Slot::Taken(file))
        }
        Err(error) if os_lock::is_contended(&error) => Ok(Slot::Busy),
        Err(error) => Err(LockHeld::unavailable(
            path,
            format!("gate lock unavailable: {error}"),
        )),
    }
}

/// Admit a full gate in `dirs`; see [`acquire_gate_lock`].
///
/// # Errors
///
/// As [`acquire_gate_lock`].
pub fn acquire_full_gate_lock(dirs: &[PathBuf], project_dir: &Path) -> Result<GateLock, LockHeld> {
    acquire_gate_lock(dirs, project_dir, "full")
}

/// Admit a heavy run: in every lock directory, hold the host lock shared and
/// take the first free slot, recording `mode` beside it. Other active runs
/// never refuse it; they only raise [`GateLock::runs`].
/// # Errors
/// While a `host_exclusive` target holds the host; when a lock directory or
/// lock cannot be opened, or a slot record cannot be read or written. Locks taken in this call are released before returning.
pub fn acquire_gate_lock(
    dirs: &[PathBuf],
    project_dir: &Path,
    mode: &str,
) -> Result<GateLock, LockHeld> {
    let mut lock = GateLock {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        files: Vec::new(),
        inherited: Vec::new(),
        paths: Vec::new(),
        runs: 1,
        notes: Vec::new(),
    };
    let record = holder_record(project_dir, mode);
    let trusted = trusted_dirs(project_dir);
    if dirs.is_empty() {
        lock.notes.push(
            "no gate lock location (no CodeFlow home and no git repository); \
             this run does not see other heavy runs or a host-exclusive target"
                .to_string(),
        );
    }
    let mut host = Vec::new();
    for dir in dirs {
        let host_path = dir.join(HOST_LOCK_FILE);
        match try_open_and_lock(&host_path, true)? {
            Slot::Taken(file) => host.push((host_path, AcquiredFile(file))),
            Slot::Busy => {
                let holder = read_holder(&host_path, None, &trusted).unwrap_or_default();
                return Err(LockHeld {
                    path: host_path,
                    holder,
                    running_groups: Vec::new(),
                    exclusive: true,
                });
            }
        }
        let (acquired, path, busy) = take_slot(dir, &trusted, &mut lock.notes)?;
        // The record is what lets another run see this one and its live
        // targets, so a run that cannot write it does not run uncoordinated
        // (TSK-1003): it refuses, and the slot releases as `acquired` drops.
        if let Err(error) = write_record(&path, &record) {
            return Err(LockHeld::unavailable(
                path,
                format!("gate lock unavailable: its holder record cannot be written: {error}"),
            ));
        }
        lock.runs = lock.runs.max(busy + 1);
        lock.files.push(acquired);
        lock.paths.push(path);
    }
    if !lock.files.is_empty() {
        held().push(Held {
            id: lock.id,
            paths: lock.paths.clone(),
            record,
            groups: BTreeSet::new(),
            recorded: BTreeSet::new(),
            inherited: false,
            #[cfg(unix)]
            descriptors: {
                use std::os::fd::AsRawFd;
                lock.files
                    .iter()
                    .map(|file| file.0.as_raw_fd())
                    .chain(host.iter().map(|(_, file)| file.0.as_raw_fd()))
                    .collect()
            },
            host,
            trusted,
            runs: lock.runs,
        });
    }
    Ok(lock)
}

/// Take the first free slot in `dir`: the slot, its path, and how many
/// other slots in `dir` are busy. A slot is busy when another process holds
/// its lock or its record names a live target group.
fn take_slot(
    dir: &Path,
    trusted: &[String],
    notes: &mut Vec<String>,
) -> Result<(AcquiredFile, PathBuf, usize), LockHeld> {
    let mut busy = 0;
    // Every existing slot is tried in order, then the next index is created;
    // a slot another run creates first only moves this run one further.
    for index in 0.. {
        let path = dir.join(slot_file(index));
        let Slot::Taken(file) = try_open_and_lock(&path, false)? else {
            busy += 1;
            continue;
        };
        let mut acquired = AcquiredFile(file);
        let previous = read_holder(&path, Some(&mut acquired.0), trusted).map_err(|error| {
            LockHeld::unavailable(
                path.clone(),
                format!("cannot read gate lock record: {error}"),
            )
        })?;
        let running = live_groups(&previous).map_err(|error| {
            LockHeld::unavailable(
                path.clone(),
                format!("cannot reclaim gate lock: {error}; inspect and repair the lock record after verifying its processes"),
            )
        })?;
        if !running.is_empty() {
            // The run that wrote this record is gone but its targets are
            // not: the slot is still in use. Leave the record for them.
            busy += 1;
            continue;
        }
        if !previous.trim().is_empty() {
            notes.push(format!(
                "reclaimed a stale gate lock at {} left by {}",
                crate::git::display_path(&path),
                crate::git::display_line(&describe_holder(&previous))
            ));
        }
        busy += busy_after(dir, index + 1, trusted)?;
        return Ok((acquired, path, busy));
    }
    unreachable!("the slot scan ends at a slot it takes or an error")
}

/// Busy slots in `dir` from `first` on, among the slot files that exist.
/// A free slot is locked only for the check and released at once.
fn busy_after(dir: &Path, first: usize, trusted: &[String]) -> Result<usize, LockHeld> {
    let mut busy = 0;
    for index in slot_indices(dir)?
        .into_iter()
        .filter(|&index| index >= first)
    {
        if slot_busy(&dir.join(slot_file(index)), trusted)? {
            busy += 1;
        }
    }
    Ok(busy)
}

/// The host held exclusively for a `host_exclusive` target. Dropping it
/// releases the host and takes this process's shared host locks back.
#[derive(Debug)]
pub struct HostExclusive {
    /// Each host lock held exclusively: a handle opened for it, or `None`
    /// when this process's own shared handle was upgraded in place.
    files: Vec<(PathBuf, Option<AcquiredFile>)>,
}

impl Drop for HostExclusive {
    fn drop(&mut self) {
        for (path, _) in &self.files {
            let _ = write_record(path, "");
        }
        release_exclusive(std::mem::take(&mut self.files));
        retake_shared(std::time::Duration::from_secs(5));
    }
}

/// Take the host lock at `path` exclusively on the handle this process
/// already holds for it, or `None` when it holds none. Target watchers keep
/// copies of that handle (Unix shares the lock across copies of one open
/// file description), so the exclusive lease lasts while a target outlives
/// its run, as the shared one did (R483-01).
fn upgrade_held_host(path: &Path) -> Option<std::io::Result<()>> {
    let held = held();
    let (_, file) = held
        .iter()
        .flat_map(|lock| &lock.host)
        .find(|(held_path, _)| held_path == path)?;
    Some(os_lock::try_lock(&file.0))
}

/// Release exclusive host locks: opened handles close, and upgraded ones are
/// unlocked so [`retake_shared`] can take them back shared.
fn release_exclusive(files: Vec<(PathBuf, Option<AcquiredFile>)>) {
    let upgraded: Vec<PathBuf> = files
        .into_iter()
        .filter_map(|(path, file)| file.is_none().then_some(path))
        .collect();
    let held = held();
    for path in upgraded {
        if let Some((_, file)) = held
            .iter()
            .flat_map(|lock| &lock.host)
            .find(|(held_path, _)| *held_path == path)
        {
            os_lock::unlock(&file.0);
        }
    }
}

/// Take the host lock of every lock directory of `root` exclusively, so the
/// calling run's next target runs alone on the host: this process's own
/// shared host locks are released first, then the exclusive locks are tried
/// until `wait` passes, on the same handles where this process holds them,
/// so a target's watcher keeps the exclusive lease if the run dies. A run under `codeflow reserve` cannot take them,
/// since the reserve holds the host shared.
/// # Errors
/// When other heavy runs still hold the host after `wait`, the run is nested
/// under a reserve, or a lock cannot be opened. The shared locks are taken
/// back before returning.
pub fn hold_host_exclusive(
    root: &Path,
    wait: std::time::Duration,
) -> Result<HostExclusive, String> {
    hold_host_exclusive_in(&project_lock_dirs(root)?, root, wait)
}

/// [`hold_host_exclusive`] over the given lock directories.
/// # Errors
/// As [`hold_host_exclusive`].
pub fn hold_host_exclusive_in(
    dirs: &[PathBuf],
    root: &Path,
    wait: std::time::Duration,
) -> Result<HostExclusive, String> {
    if held().iter().any(|lock| {
        lock.inherited
            && lock.paths.iter().any(|path| {
                path.parent()
                    .is_some_and(|parent| dirs.iter().any(|dir| dir == parent))
            })
    }) {
        return Err(
            "a host-exclusive target cannot run under `codeflow reserve`, which holds the host \
             for its command; run it outside the reserve"
                .to_string(),
        );
    }
    let trusted = trusted_dirs(root);
    for lock in held().iter() {
        for (_, file) in &lock.host {
            os_lock::unlock(&file.0);
        }
    }
    let start = std::time::Instant::now();
    let mut taken: Vec<(PathBuf, Option<AcquiredFile>)> = Vec::new();
    let result = loop {
        let mut busy = None;
        let mut failed = None;
        for dir in dirs.iter().skip(taken.len()) {
            let path = dir.join(HOST_LOCK_FILE);
            let attempt = match upgrade_held_host(&path) {
                Some(Ok(())) => Ok(None),
                Some(Err(error)) if os_lock::is_contended(&error) => Err(None),
                Some(Err(error)) => Err(Some(LockHeld::unavailable(
                    path.clone(),
                    format!("gate lock unavailable: {error}"),
                ))),
                None => match try_open_and_lock(&path, false) {
                    Ok(Slot::Taken(file)) => Ok(Some(AcquiredFile(file))),
                    Ok(Slot::Busy) => Err(None),
                    Err(held) => Err(Some(held)),
                },
            };
            match attempt {
                Ok(file) => taken.push((path, file)),
                Err(None) => {
                    busy = Some(path);
                    break;
                }
                Err(Some(held)) => {
                    failed = Some(with_repair_pointer(&held));
                    break;
                }
            }
        }
        if let Some(message) = failed {
            break Err(message);
        }
        if busy.is_none() && taken.len() == dirs.len() {
            // The host lock is ours, but a target that outlived its run can
            // still be working: a slot other than this run's own that is
            // busy keeps the target waiting.
            match orphaned_slot(dirs, &trusted) {
                Ok(None) => break Ok(()),
                Ok(Some(path)) => busy = Some(path),
                Err(held) => break Err(with_repair_pointer(&held)),
            }
        }
        let Some(path) = busy else {
            break Ok(());
        };
        if start.elapsed() >= wait {
            break Err(format!(
                "a host-exclusive target waits for the other heavy runs on this machine to \
                 end, and they still ran after {}s (lock: {})",
                wait.as_secs(),
                crate::git::display_path(&path)
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    if let Err(message) = result {
        release_exclusive(taken);
        retake_shared(wait.max(std::time::Duration::from_secs(5)));
        return Err(message);
    }
    let record = holder_record(root, "exclusive");
    let unwritten = taken.iter().find_map(|(path, _)| {
        write_record(path, &record)
            .err()
            .map(|error| (path.clone(), error))
    });
    if let Some((path, error)) = unwritten {
        release_exclusive(taken);
        retake_shared(std::time::Duration::from_secs(5));
        return Err(format!(
            "gate lock unavailable: the host-exclusive record beside {} cannot be written: {error}",
            crate::git::display_path(&path)
        ));
    }
    Ok(HostExclusive { files: taken })
}

/// A busy slot in `dirs` that this process does not hold, if any.
fn orphaned_slot(dirs: &[PathBuf], trusted: &[String]) -> Result<Option<PathBuf>, LockHeld> {
    let own: Vec<PathBuf> = held()
        .iter()
        .flat_map(|lock| lock.paths.iter().cloned())
        .collect();
    for dir in dirs {
        for index in slot_indices(dir)? {
            let path = dir.join(slot_file(index));
            if own.contains(&path) {
                continue;
            }
            if slot_busy(&path, trusted)? {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

/// Whether the existing slot at `path` is busy: held by another process, or
/// free with a record naming a live target group.
fn slot_busy(path: &Path, trusted: &[String]) -> Result<bool, LockHeld> {
    match try_open_and_lock(path, false)? {
        Slot::Busy => Ok(true),
        Slot::Taken(file) => {
            let mut acquired = AcquiredFile(file);
            let record = read_holder(path, Some(&mut acquired.0), trusted).map_err(|error| {
                LockHeld::unavailable(
                    path.to_path_buf(),
                    format!("cannot read gate lock record: {error}"),
                )
            })?;
            let running = live_groups(&record).map_err(|error| {
                LockHeld::unavailable(
                    path.to_path_buf(),
                    format!("cannot reclaim gate lock: {error}"),
                )
            })?;
            Ok(!running.is_empty())
        }
    }
}

/// Take back this process's shared host locks, waiting up to `wait` for a
/// host-exclusive target of another run to end.
fn retake_shared(wait: std::time::Duration) {
    let start = std::time::Instant::now();
    let held = held();
    for lock in held.iter() {
        for (_, file) in &lock.host {
            while os_lock::try_lock_shared(&file.0).is_err() && start.elapsed() < wait {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
}

/// Names the lock descriptors `codeflow reserve` hands to a `codeflow`
/// command, as decimal descriptor numbers separated by commas (TSK-1128).
/// The value is a hint, never authority: `inherit_gate_lock` trusts a
/// descriptor only after the OS lock shows it is the holder's own.
pub const RESERVED_LOCKS_ENV: &str = "CODEFLOW_RESERVED_LOCK_FDS";

/// The most descriptors one hand-over names: one per lock directory.
const MOST_RESERVED_LOCKS: usize = 4;

#[cfg(unix)]
static ADOPTED: Mutex<Vec<std::os::fd::RawFd>> = Mutex::new(Vec::new());

#[cfg(unix)]
fn adopted() -> std::sync::MutexGuard<'static, Vec<std::os::fd::RawFd>> {
    ADOPTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Take the descriptors an enclosing `codeflow reserve` handed over, before
/// this process starts any thread or child. The variable is removed, so no
/// child sees it, and each named descriptor that is an open regular file is
/// made close-on-exec, so no target, tool or daemon this process starts can
/// keep the reserved lock after the run. A malformed value is ignored
/// whole. The descriptors grant nothing until `inherit_gate_lock` checks
/// them.
pub fn adopt_reserved_locks() {
    let value = std::env::var_os(RESERVED_LOCKS_ENV);
    std::env::remove_var(RESERVED_LOCKS_ENV);
    #[cfg(unix)]
    if let Some(descriptors) = value
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(parse_descriptors)
    {
        let mut kept = Vec::new();
        for fd in descriptors {
            // SAFETY: fstat and fcntl only read or set this descriptor's
            // flags; an unopened number fails with EBADF and is skipped.
            unsafe {
                let mut stat: libc::stat = std::mem::zeroed();
                if libc::fstat(fd, &raw mut stat) != 0
                    || stat.st_mode & libc::S_IFMT != libc::S_IFREG
                {
                    continue;
                }
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) != 0 {
                    continue;
                }
            }
            kept.push(fd);
        }
        *adopted() = kept;
    }
    #[cfg(not(unix))]
    let _ = value;
}

/// Descriptor numbers from a hand-over value: distinct, above the standard
/// streams, at most [`MOST_RESERVED_LOCKS`]; `None` when any part is not.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_descriptors(value: &str) -> Option<Vec<i32>> {
    let mut descriptors = Vec::new();
    for part in value.split(',') {
        let fd = part.parse::<i32>().ok().filter(|fd| *fd > 2)?;
        if descriptors.contains(&fd) || descriptors.len() == MOST_RESERVED_LOCKS {
            return None;
        }
        descriptors.push(fd);
    }
    Some(descriptors)
}

/// Use the slots in `dirs` that an enclosing holder handed over as
/// `descriptors`, or `None` to fall back to ordinary admission. Every lock
/// directory must match: one of its existing slot files has the identity of
/// one descriptor, a fresh open of that path is refused by the OS lock
/// (someone holds it), and that descriptor takes the lock (so the holder is
/// its open file description, which only the holder's process tree can
/// carry). A descriptor this process opened itself, or one naming any other
/// file, fails a check. Nothing is unlocked on any path: the descriptors
/// share the holder's slot, and the holder record stays the holder's.
///
/// A slot alone is not the whole lease: this process also takes the host
/// lock of every directory shared, as ordinary admission does, so a hand-over
/// of self-held slots cannot start heavy work while a `host_exclusive` target
/// holds the host (R483-02). A slot record that cannot be read refuses, as it
/// does in ordinary admission.
/// # Errors
/// While a `host_exclusive` target holds the host, or when a host lock or a
/// slot record of `dirs` cannot be opened or read.
#[cfg(unix)]
fn inherit_gate_lock(
    dirs: &[PathBuf],
    descriptors: &[std::os::fd::RawFd],
    trusted: Vec<String>,
) -> Result<Option<GateLock>, LockHeld> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if dirs.is_empty() || descriptors.is_empty() {
        return Ok(None);
    }
    let candidates: Vec<(FileIdentity, File)> = descriptors
        .iter()
        .filter_map(|&fd| {
            // SAFETY: F_DUPFD_CLOEXEC returns a new descriptor this process
            // owns, or -1 when `fd` is not open.
            let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
            if copy < 0 {
                return None;
            }
            // SAFETY: `copy` is a fresh descriptor owned by nothing else.
            let file = unsafe { File::from_raw_fd(copy) };
            Some((FileIdentity::of(&file).ok()?, file))
        })
        .collect();
    let mut inherited = Vec::new();
    let mut paths = Vec::new();
    for dir in dirs {
        let Some((path, file)) = slot_indices(dir)?.into_iter().find_map(|index| {
            let path = dir.join(slot_file(index));
            let (anchor, relative) = lock_anchor(&path).ok()?;
            let current = crate::contained::Tree::open(anchor)
                .ok()?
                .open_regular(&relative)
                .ok()?;
            let identity = FileIdentity::of(&current).ok()?;
            let file = candidates
                .iter()
                .find(|(candidate, _)| *candidate == identity)
                .and_then(|(_, file)| file.try_clone().ok())?;
            Some((path, file))
        }) else {
            return Ok(None);
        };
        let Ok(fresh) = open_lock_file(&path) else {
            return Ok(None);
        };
        match os_lock::try_lock(&fresh) {
            Err(error) if os_lock::is_contended(&error) => {}
            Ok(()) => {
                // Nobody holds it: there is nothing to inherit.
                os_lock::unlock(&fresh);
                return Ok(None);
            }
            Err(_) => return Ok(None),
        }
        if os_lock::try_lock(&file).is_err() {
            return Ok(None);
        }
        inherited.push(file);
        paths.push(path);
    }
    // The host half of the lease, taken as ordinary admission takes it.
    let mut host = Vec::new();
    for dir in dirs {
        let host_path = dir.join(HOST_LOCK_FILE);
        match try_open_and_lock(&host_path, true)? {
            Slot::Taken(file) => host.push((host_path, AcquiredFile(file))),
            Slot::Busy => {
                let holder = read_holder(&host_path, None, &trusted).unwrap_or_default();
                return Err(LockHeld {
                    path: host_path,
                    holder,
                    running_groups: Vec::new(),
                    exclusive: true,
                });
            }
        }
    }
    // The run under the reserve is the reserve's run: its busy slot counts
    // once, so `runs` is the busy slots of the most crowded folder. An
    // unreadable slot record refuses here as in ordinary admission.
    let mut runs = 1;
    for dir in dirs {
        runs = runs.max(busy_after(dir, 0, &trusted)?);
    }
    let lock = GateLock {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        files: Vec::new(),
        inherited,
        paths: paths.clone(),
        runs,
        notes: vec![
            "heavy work runs under the slot held by the enclosing `codeflow reserve`".to_string(),
        ],
    };
    // Targets' watchers keep copies of the slot, and this process adds its
    // targets' groups to the holder's record without changing its holder
    // lines, so the slot stays busy while they run, even after this process
    // and the reserve end (TSK-1128).
    held().push(Held {
        id: lock.id,
        paths,
        record: String::new(),
        groups: BTreeSet::new(),
        recorded: BTreeSet::new(),
        inherited: true,
        trusted,
        runs: lock.runs,
        descriptors: lock
            .inherited
            .iter()
            .map(AsRawFd::as_raw_fd)
            .chain(host.iter().map(|(_, file)| file.0.as_raw_fd()))
            .collect(),
        host,
    });
    Ok(Some(lock))
}

/// Duplicate held locks for a target's lifetime watcher before spawning it.
/// The registry guard prevents `GateLock::drop` from closing a borrowed descriptor.
#[cfg(unix)]
pub(crate) fn inherited_gate_locks() -> std::io::Result<Vec<File>> {
    use std::os::fd::BorrowedFd;
    let locks = held();
    locks
        .iter()
        .flat_map(|lock| &lock.descriptors)
        .map(|&descriptor| {
            // SAFETY: the registry guard keeps the owning GateLock alive during this clone.
            unsafe { BorrowedFd::borrow_raw(descriptor) }
                .try_clone_to_owned()
                .map(File::from)
        })
        .collect()
}

/// Record that a target started in process group `pgid`, in every full-gate
/// lock this process holds. A no-op when it holds none (light-only quick work, or a
/// runner used outside `codeflow test`).
///
/// # Errors
///
/// When a holder record could not be rewritten; the target then runs
/// without its group recorded, which the caller reports.
pub fn target_group_started(pgid: u32) -> Result<(), String> {
    update_groups(|groups| {
        groups.insert(pgid);
    })
}

/// Record that the target in process group `pgid` finished.
///
/// # Errors
///
/// When a holder record could not be rewritten.
pub fn target_group_finished(pgid: u32) -> Result<(), String> {
    update_groups(|groups| {
        groups.remove(&pgid);
    })
}

fn update_groups(change: impl Fn(&mut BTreeSet<u32>)) -> Result<(), String> {
    let mut failures = Vec::new();
    let mut held = held();
    for lock in held.iter_mut() {
        change(&mut lock.groups);
        lock.recorded.extend(lock.groups.iter().copied());
        for path in &lock.paths {
            failures.extend(rewrite_serialized(lock, path));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Rewrite the record beside `path` for `lock`: its holder lines (the
/// holder's own for an inherited lock), every live group another process
/// added, then this lock's groups. The caller holds the record lock.
fn rewrite_record(lock: &Held, path: &Path) -> Result<(), String> {
    // Keep the holder lines of an inherited record and every live
    // group another process added (TSK-1128); a record that cannot
    // be read keeps only this lock's own lines.
    let read = record_parts(path, &lock.trusted);
    #[cfg(test)]
    interleave::at(interleave::Point::RecordRead);
    let (base, foreign) = match read {
        Ok((base, groups)) => (
            if lock.inherited {
                base
            } else {
                lock.record.clone()
            },
            groups
                .into_iter()
                .filter(|group| !lock.recorded.contains(group) && group_alive(*group))
                .collect::<Vec<_>>(),
        ),
        Err(error) if lock.inherited => {
            return Err(format!(
                "the holder record beside {} could not be read to add target process groups: {error}",
                crate::git::display_path(path)
            ));
        }
        Err(_) => (lock.record.clone(), Vec::new()),
    };
    let mut text = base;
    for group in foreign.iter().chain(&lock.groups) {
        // Writing to a String cannot fail.
        let _ = writeln!(text, "group={group}");
    }
    // A new record renamed into place. A missed write weakens the
    // killed-gate check, so it is reported, never ignored (TSK-1003).
    write_record(path, &text).map_err(|error| {
        format!(
            "the gate lock record beside {} could not be updated with target process groups: {error}",
            crate::git::display_path(path)
        )
    })
}

/// Rewrite the record beside `path` for `lock` under the record lock, so a
/// late rewrite still keeps the groups another process added. Without the
/// record lock the rewrite runs unlocked, as before, and the missing
/// serialization is reported. Returns the failures to report.
fn rewrite_serialized(lock: &Held, path: &Path) -> Vec<String> {
    match with_record_lock(path, || rewrite_record(lock, path)) {
        Ok(Ok(())) => Vec::new(),
        Ok(Err(failure)) => vec![failure],
        Err(error) => {
            let mut failures = vec![format!(
                "the record lock beside {} could not be taken, so the record was rewritten without it: {error}",
                crate::git::display_path(path)
            )];
            failures.extend(rewrite_record(lock, path).err());
            failures
        }
    }
}

/// Run `rewrite` while holding the OS lock of the record lock file beside
/// the gate lock at `lock`, waiting up to five seconds for another rewrite
/// to finish. A holder keeps it only for one read, merge and rename, and
/// the OS releases it if that process dies.
fn with_record_lock<T>(lock: &Path, rewrite: impl FnOnce() -> T) -> std::io::Result<T> {
    let mut record_lock = record_path(lock).into_os_string();
    record_lock.push(".lock");
    let record_lock = PathBuf::from(record_lock);
    let (anchor, relative) = lock_anchor(&record_lock)?;
    let guard = crate::contained::Tree::open(anchor)?.create_read_only(&relative)?;
    let start = std::time::Instant::now();
    loop {
        match os_lock::try_lock(&guard) {
            Ok(()) => break,
            Err(error)
                if os_lock::is_contended(&error)
                    && start.elapsed() < std::time::Duration::from_secs(5) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    let result = rewrite();
    os_lock::unlock(&guard);
    Ok(result)
}

/// Process groups named in a holder record that still have a live process.
fn live_groups(record: &str) -> Result<Vec<u32>, String> {
    let mut groups = Vec::new();
    for line in record.split_terminator('\n') {
        if let Some(value) = line.strip_prefix("group=") {
            let pgid = value
                .parse::<u32>()
                .map_err(|_| "a group value is not a target process group".to_string())?;
            if pgid <= 1 {
                return Err("a group value is not a target process group".to_string());
            }
            if group_alive(pgid) {
                groups.push(pgid);
            }
        }
    }
    Ok(groups)
}

#[cfg(unix)]
fn group_alive(pgid: u32) -> bool {
    // 0 and 1 are not target groups, and as negative pids they address this
    // process's own group or every process.
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return false;
    };
    if pgid <= 1 {
        return false;
    }
    // SAFETY: signal 0 only checks that the group exists and is signalable;
    // nothing is delivered.
    if unsafe { libc::kill(-pgid, 0) } == 0 {
        return true;
    }
    // EPERM: the group exists but belongs to another user.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn group_alive(_pgid: u32) -> bool {
    // No group is recorded here: on Windows each target's tree ends with
    // its gate through the target's job object (TSK-142), so nothing can
    // outlive the gate.
    false
}

#[derive(Debug)]
struct AcquiredFile(File);

impl Drop for AcquiredFile {
    fn drop(&mut self) {
        os_lock::unlock(&self.0);
    }
}

/// The OS lock behind a gate lock. On Unix it is `flock` on the whole file.
/// Windows locks are mandatory, and a locked range cannot be read through
/// another handle, so there the lock covers one byte far past any holder
/// record: every process can still read who holds the gate.
#[cfg(not(windows))]
mod os_lock {
    use fs2::FileExt;
    use std::fs::File;

    pub(super) fn try_lock(file: &File) -> std::io::Result<()> {
        file.try_lock_exclusive()
    }

    pub(super) fn try_lock_shared(file: &File) -> std::io::Result<()> {
        FileExt::try_lock_shared(file)
    }

    pub(super) fn unlock(file: &File) {
        let _ = FileExt::unlock(file);
    }

    pub(super) fn is_contended(error: &std::io::Error) -> bool {
        error.kind() == fs2::lock_contended_error().kind()
    }
}

#[cfg(windows)]
mod os_lock {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    /// The locked byte: far past any record, which stays a few hundred bytes.
    const LOCK_OFFSET: u64 = 1 << 40;

    fn at_lock_offset() -> OVERLAPPED {
        // SAFETY: OVERLAPPED is plain data; all zeroes is its documented
        // initial state.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.Anonymous.Anonymous.Offset = (LOCK_OFFSET & 0xffff_ffff) as u32;
        overlapped.Anonymous.Anonymous.OffsetHigh = (LOCK_OFFSET >> 32) as u32;
        overlapped
    }

    fn lock_range(file: &File, flags: u32) -> std::io::Result<()> {
        let mut overlapped = at_lock_offset();
        // SAFETY: the handle is open for the call, and the OVERLAPPED lives
        // across it; a synchronous handle completes before returning.
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                flags | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &raw mut overlapped,
            )
        };
        if locked == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn try_lock(file: &File) -> std::io::Result<()> {
        lock_range(file, LOCKFILE_EXCLUSIVE_LOCK)
    }

    /// A shared lock on the same byte: any number of shared holders, and
    /// no exclusive holder beside them.
    pub(super) fn try_lock_shared(file: &File) -> std::io::Result<()> {
        lock_range(file, 0)
    }

    pub(super) fn unlock(file: &File) {
        let mut overlapped = at_lock_offset();
        // SAFETY: as in `try_lock`; closing the handle also releases it.
        unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &raw mut overlapped) };
    }

    pub(super) fn is_contended(error: &std::io::Error) -> bool {
        error.raw_os_error() == i32::try_from(ERROR_LOCK_VIOLATION).ok()
    }
}

/// The trusted folder a lock file is opened beneath, and the lock's path
/// relative to it: `<anchor>/<home or .git>/<lock folder>/full-gate.lock`.
/// The lock folder may be writable by a sandboxed seat (`~/.codeflow/locks`
/// for a Codex builder, the common `.git` for any seat that commits), so it
/// and the lock file are opened without following links (TSK-1003). The
/// walk starts one folder higher, so a link at the `CodeFlow` home or the
/// common `.git` folder itself is refused too, never trusted as the root
/// (TSK-1044).
fn lock_anchor(path: &Path) -> std::io::Result<(&Path, String)> {
    let unsupported = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "gate lock path has no parent folder or is not valid UTF-8",
        )
    };
    let name = |path: &Path| {
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_string)
            .ok_or_else(unsupported)
    };
    let dir = path.parent().ok_or_else(unsupported)?;
    let owner = dir.parent().ok_or_else(unsupported)?;
    let anchor = owner
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(unsupported)?;
    Ok((
        anchor,
        format!("{}/{}/{}", name(owner)?, name(dir)?, name(path)?),
    ))
}

/// Open (creating if needed) the lock file at `path`, read only. The lock
/// folder and the file are refused when either is a link, and the file when
/// it is not a regular file, checked on the opened descriptor. Nothing is
/// ever written through this descriptor: it only carries the OS lock, so a
/// lock entry that names an outside file, by a hard link planted before or
/// during the open, lets a seat lock that file but never change it.
fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let (anchor, relative) = lock_anchor(path)?;
    std::fs::create_dir_all(anchor)?;
    let file = crate::contained::Tree::open(anchor)?.create_read_only(&relative)?;
    #[cfg(test)]
    interleave::at(interleave::Point::Opened);
    Ok(file)
}

/// Whether the lock path still names the file `file` holds open. A lock
/// file swapped for another regular file splits coordination: a later gate
/// would lock the new file. The lock folder is an advisory coordination
/// boundary, not a security boundary, so a swap after this check still
/// splits it; the check closes the window while the lock is taken. Both
/// identities are read from handles this module opened, the held one and the
/// anchored no-follow reopen, and an identity that cannot be read refuses.
fn same_lock_file(path: &Path, file: &File) -> std::io::Result<()> {
    let (anchor, relative) = lock_anchor(path)?;
    let current = crate::contained::Tree::open(anchor)?.open_regular(&relative)?;
    if FileIdentity::of(&current)? == FileIdentity::of(file)? {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "the lock path names another file than the one locked",
        ))
    }
}

/// The record path beside the lock file at `lock`: its name with `.lock`
/// replaced by `.record` ([`RECORD_FILE`] beside [`LOCK_FILE`]).
fn record_path(lock: &Path) -> PathBuf {
    let name = lock
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| name.strip_suffix(".lock"))
        .map_or_else(|| RECORD_FILE.to_string(), |stem| format!("{stem}.record"));
    lock.with_file_name(name)
}

/// The holder record for the lock at `lock`, parsed and rendered again from
/// validated values only ([`parse_record`]), so an outside file reached
/// through a planted link is never printed. The record file is read through an anchored read-only
/// open that refuses links. While it is absent or empty, the legacy record
/// a pre-3.2 gate keeps inside the lock file is read instead, so a gate of
/// an older binary on the same machine is still seen: through `held` when
/// the caller holds the lock file open, else through the same read-only
/// open. Empty when neither names a holder. Nothing is created.
fn read_holder(
    lock: &Path,
    held: Option<&mut File>,
    trusted: &[String],
) -> std::io::Result<String> {
    let unreadable = |error: String| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unreadable gate lock record: {error}"),
        )
    };
    let record = record_path(lock);
    if !crate::absence::proven_absent(&record)? {
        let (anchor, relative) = lock_anchor(&record)?;
        let text =
            read_record(&mut crate::contained::Tree::open(anchor)?.open_regular(&relative)?)?;
        let parsed = parse_record(&text, trusted).map_err(unreadable)?;
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }
    let legacy = match held {
        Some(file) => read_record(file)?,
        None if crate::absence::proven_absent(lock)? => String::new(),
        None => {
            let (anchor, relative) = lock_anchor(lock)?;
            read_record(&mut crate::contained::Tree::open(anchor)?.open_regular(&relative)?)?
        }
    };
    parse_record(&legacy, trusted).map_err(unreadable)
}

/// At most 4096 bytes of `file`, from its start.
fn read_record(file: &mut File) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(std::io::Error::other("gate lock record exceeds 4096 bytes"));
    }
    String::from_utf8(bytes).map_err(|_| std::io::Error::other("gate lock record is not UTF-8"))
}

/// The folders a holder record's `dir` may name and still be shown: the
/// project folder this gate runs for, its canonical form, and the main
/// checkout and linked worktrees git lists for its repository, as strings
/// with trailing separators removed. Only the project folder, which this
/// process was given, is resolved; the git-listed paths are taken as text
/// and never touched, since a seat that writes `.git` can change them.
fn trusted_dirs(project_dir: &Path) -> Vec<String> {
    let mut dirs = vec![project_dir.to_path_buf()];
    if let Ok(canonical) = std::fs::canonicalize(project_dir) {
        dirs.push(canonical);
    }
    if let Ok(repo) = git2::Repository::discover(project_dir) {
        if let Some(workdir) = repo.workdir() {
            dirs.push(workdir.to_path_buf());
        }
        let common = repo.commondir();
        if common.file_name().is_some_and(|name| name == ".git") {
            if let Some(main) = common.parent() {
                dirs.push(main.to_path_buf());
            }
        }
        if let Ok(names) = repo.worktrees() {
            for name in names.iter().filter_map(|name| name.ok().flatten()) {
                if let Ok(worktree) = repo.find_worktree(name) {
                    dirs.push(worktree.path().to_path_buf());
                }
            }
        }
    }
    let mut out: Vec<String> = dirs
        .iter()
        .map(|dir| trimmed_dir(&dir.display().to_string()).into_owned())
        .filter(|dir| !dir.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `dir` without trailing path separators, compared as text only. On
/// Windows, where git lists worktrees with `/` and paths display with `\\`,
/// both are compared as `/`.
fn trimmed_dir(dir: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    if cfg!(windows) {
        std::borrow::Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(trimmed)
    }
}

/// The modes a holder record names.
const LOCK_MODES: [&str; 5] = ["full", "quick", "essential", "reserve", "exclusive"];

/// Parse a holder record strictly and render it again from validated
/// values only: `pid` as a process number, `started` as a Unix time,
/// `mode` from [`LOCK_MODES`] (at most once each) and `group` lines as
/// process group numbers. `dir` is kept only when its text equals one of
/// `trusted` ([`trusted_dirs`]); otherwise it is left out and shown as an
/// unknown directory. The `dir` value is never resolved, opened or passed
/// to any filesystem call, so a planted value cannot reach a link, a
/// network share or an outside folder (TSK-1003, SEC-4). Any other line
/// refuses the whole record. Errors are fixed texts that never include the
/// record's text, so a file reached through a planted link reveals nothing
/// beyond a folder the gate already trusts.
fn parse_record(text: &str, trusted: &[String]) -> Result<String, String> {
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for line in text.split_terminator('\n') {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "a line is not key=value".to_string())?;
        let shown = match key {
            "pid" => value
                .parse::<u32>()
                .map(|pid| pid.to_string())
                .map_err(|_| "a pid value is not a process number".to_string())?,
            "started" => value
                .parse::<u64>()
                .map(|started| started.to_string())
                .map_err(|_| "a started value is not a Unix time".to_string())?,
            "mode" => LOCK_MODES
                .iter()
                .find(|mode| **mode == value)
                .map(ToString::to_string)
                .ok_or_else(|| "a mode value is not a known mode".to_string())?,
            "group" => match value.parse::<u32>() {
                Ok(group) if group > 1 => group.to_string(),
                _ => return Err("a group value is not a target process group".to_string()),
            },
            "dir" => {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err("a dir value is empty or holds a control character".to_string());
                }
                let value = trimmed_dir(value);
                if let Some(dir) = trusted.iter().find(|dir| dir.as_str() == value.as_ref()) {
                    dir.clone()
                } else {
                    // Not a folder the gate trusts: shown as unknown, never
                    // as the record's text, and never looked up.
                    if !seen.insert("dir") {
                        return Err("a repeated dir".to_string());
                    }
                    continue;
                }
            }
            _ => return Err("an unknown key".to_string()),
        };
        if key != "group" && !seen.insert(key) {
            return Err(format!("a repeated {key}"));
        }
        // Writing to a String cannot fail.
        let _ = writeln!(out, "{key}={shown}");
    }
    Ok(out)
}

fn holder_record(project_dir: &Path, mode: &str) -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "pid={}\nstarted={started}\ndir={}\nmode={mode}\n",
        std::process::id(),
        project_dir.display()
    )
}

/// Replace the record beside the lock at `lock` with `text`: a new file
/// created exclusively under a random name in the lock folder, written
/// through its own descriptor, then renamed over the record name. An
/// existing record is never opened for writing; a link at the record name
/// is refused, never followed, and a hard link there is replaced by the
/// rename.
fn write_record(lock: &Path, text: &str) -> std::io::Result<()> {
    let record = record_path(lock);
    let (anchor, relative) = lock_anchor(&record)?;
    let tree = crate::contained::Tree::open(anchor)?;
    // On Windows a rename over a record another process is reading fails
    // with a sharing violation until that brief read ends; retry a few
    // times before reporting it.
    let mut attempts = 0;
    loop {
        match tree.write(&relative, text.as_bytes()) {
            Err(error) if cfg!(windows) && attempts < 20 && is_sharing_violation(&error) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

/// Windows `ERROR_SHARING_VIOLATION` (32) or `ERROR_ACCESS_DENIED` (5),
/// which a rename over an open file reports.
fn is_sharing_violation(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
}

/// Render a holder record as `pid N in DIR, started S s ago`.
fn describe_holder(record: &str) -> String {
    let field = |key: &str| {
        record
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    let pid = field("pid").unwrap_or("unknown");
    let dir = field("dir").map_or_else(
        || " in an unknown directory".to_string(),
        |d| format!(" in {}", crate::git::display_line(d)),
    );
    let age = field("started")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|started| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            format!(", started {} s ago", now.saturating_sub(started))
        })
        .unwrap_or_default();
    format!(
        "pid {pid}{dir}{age}, {} mode",
        field("mode").unwrap_or("full")
    )
}

/// Warn about a `CARGO_TARGET_DIR` outside the worktree when the gate runs
/// cargo.
///
/// `value` is the variable as set (relative paths resolve against `cwd`, as
/// cargo resolves them). `None` or an empty value passes.
///
/// Returns the warning naming the shared directory, or `None`.
#[must_use]
pub fn check_cargo_target_dir(
    worktree: &Path,
    cwd: &Path,
    value: Option<&std::ffi::OsStr>,
) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;
    let raw = Path::new(value);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };
    let target = resolve(&joined);
    let root = resolve(worktree);
    if target.starts_with(&root) {
        return None;
    }
    Some(format!(
        "CARGO_TARGET_DIR={} is shared outside this worktree ({}). When worktrees build \
         in parallel, a shared target directory lets one build overwrite another's \
         binaries mid-run; for parallel work, point it inside each worktree (for \
         example {}).",
        crate::git::display_path(&target),
        crate::git::display_path(&root),
        crate::git::display_path(&root.join("target"))
    ))
}

/// Canonicalize the longest existing ancestor and append the rest lexically,
/// so a target directory that does not exist yet still resolves symlinks in
/// its existing part (`/tmp` versus `/private/tmp` on macOS).
fn resolve(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    let mut existing = normal.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut out = canonical;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(tmp: &Path) -> Vec<PathBuf> {
        vec![tmp.join("home-locks"), tmp.join("repo-locks")]
    }

    /// TSK-1145 AC-1: a second heavy run is admitted beside the first, in
    /// the next slot of every folder, and both count as sharing the host.
    #[test]
    fn a_second_run_takes_the_next_slot_and_both_are_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let work = std::fs::canonicalize(tmp.path()).unwrap().join("work-a");
        std::fs::create_dir(&work).unwrap();
        let first = acquire_full_gate_lock(&dirs(tmp.path()), &work).unwrap();
        assert!(first.notes.is_empty(), "{:?}", first.notes);
        assert_eq!(first.runs, 1);
        assert!(first.paths.iter().all(|path| path.ends_with(LOCK_FILE)));

        let second = acquire_full_gate_lock(&dirs(tmp.path()), &work).unwrap();
        assert_eq!(second.runs, 2);
        assert!(second.paths.iter().all(|path| path.ends_with(slot_file(1))));
        for dir in dirs(tmp.path()) {
            let record = std::fs::read_to_string(dir.join("full-gate-1.record")).unwrap();
            assert!(
                record.contains(&format!("pid={}", std::process::id())),
                "{record}"
            );
        }
        assert_eq!(held_runs(), Some(2).max(held_runs()));

        drop(first);
        drop(second);
        // A sibling test forking a child can hold a duplicate of the lock
        // descriptor until that child's exec closes it; allow that window.
        let again = (0..40)
            .find_map(|_| {
                acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/work/b"))
                    .ok()
                    .filter(|lock| lock.paths.iter().all(|path| path.ends_with(LOCK_FILE)))
                    .or_else(|| {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        None
                    })
            })
            .expect("slot 0 is free once its holder drops it");
        assert!(
            again.notes.is_empty(),
            "a clean release leaves no stale record: {:?}",
            again.notes
        );
    }

    /// TSK-1145 AC-4: a host-exclusive hold refuses new heavy runs, naming
    /// the hold, and a heavy run keeps a host-exclusive hold out.
    #[test]
    fn a_host_exclusive_hold_and_heavy_runs_exclude_each_other() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = dirs(tmp.path());
        let none = std::time::Duration::ZERO;
        let hold = hold_host_exclusive_in(&dirs, &work, none).expect("an idle host");
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert!(refused.exclusive);
        let message = refused.to_string();
        assert!(
            message.contains("host-exclusive target is running"),
            "{message}"
        );
        assert!(message.contains("exclusive mode"), "{message}");
        drop(hold);
        let run = acquire_full_gate_lock(&dirs, &work).expect("the host is free again");
        drop(run);
        // A run in another process holds the host shared; a second open
        // file description stands for it here.
        let other = open_lock_file(&dirs[0].join(HOST_LOCK_FILE)).unwrap();
        os_lock::try_lock_shared(&other).unwrap();
        let waited = hold_host_exclusive_in(&dirs, &work, none).unwrap_err();
        assert!(waited.contains("host-exclusive target waits"), "{waited}");
        os_lock::unlock(&other);
        drop(hold_host_exclusive_in(&dirs, &work, none).expect("the other run ended"));
    }

    /// R483-02: a hand-over of slots this process locked itself, with no
    /// host lease, is refused while a `host_exclusive` target holds the host.
    #[cfg(unix)]
    #[test]
    fn a_handed_over_slot_without_the_host_lease_is_refused_under_an_exclusive_target() {
        use std::os::fd::AsRawFd;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = dirs(tmp.path());
        let own: Vec<File> = dirs
            .iter()
            .map(|dir| {
                let file = open_lock_file(&dir.join(LOCK_FILE)).unwrap();
                os_lock::try_lock(&file).unwrap();
                file
            })
            .collect();
        let exclusive = open_lock_file(&dirs[0].join(HOST_LOCK_FILE)).unwrap();
        os_lock::try_lock(&exclusive).unwrap();
        let handed: Vec<_> = own.iter().map(AsRawFd::as_raw_fd).collect();
        let refused = inherit_gate_lock(&dirs, &handed, Vec::new())
            .expect_err("a slot without the host lease must not admit heavy work");
        assert!(refused.exclusive, "{refused}");
        os_lock::unlock(&exclusive);
        let inherited = inherit_gate_lock(&dirs, &handed, Vec::new())
            .unwrap()
            .expect("with the host free the hand-over inherits");
        drop(inherited);
    }

    /// R483-02: an unreadable record of another slot refuses an inherited
    /// admission, as it refuses ordinary admission.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_sibling_slot_record_refuses_an_inherited_admission() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = dirs(tmp.path());
        let holder = acquire_full_gate_lock(&dirs, &work).unwrap();
        let sibling = dirs[0].join(slot_file(1));
        std::fs::write(&sibling, "").unwrap();
        std::fs::write(tmp.path().join("elsewhere"), "").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("elsewhere"), record_path(&sibling)).unwrap();
        let handed: Vec<_> = holder
            .files
            .iter()
            .map(|file| std::os::fd::AsRawFd::as_raw_fd(&file.0))
            .collect();
        let refused = inherit_gate_lock(&dirs, &handed, Vec::new())
            .expect_err("an unreadable slot record must refuse");
        assert!(
            refused.to_string().contains("cannot read gate lock record"),
            "{refused}"
        );
        drop(holder);
    }

    /// R483-06: no run count refuses a run. With 64 slots held, a 65th run
    /// is admitted in a new slot and counts every holder.
    #[test]
    fn a_run_beyond_sixty_four_holders_is_admitted() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dir = tmp.path().join("one");
        std::fs::create_dir(&dir).unwrap();
        let dirs = vec![dir.clone()];
        let held: Vec<File> = (0..64)
            .map(|index| {
                let file = open_lock_file(&dir.join(slot_file(index))).unwrap();
                os_lock::try_lock(&file).unwrap();
                file
            })
            .collect();
        let lock = acquire_full_gate_lock(&dirs, &work).expect("a 65th run is admitted");
        assert_eq!(lock.runs, 65);
        assert!(lock.paths[0].ends_with(slot_file(64)));
        drop(lock);
        drop(held);
    }

    /// R483-01: the exclusive lease is taken on the handle target watchers
    /// copy, so a watcher that outlives a dead run still holds it.
    #[cfg(unix)]
    #[test]
    fn a_watcher_copy_carries_the_exclusive_host_lease() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = dirs(tmp.path());
        let lock = acquire_full_gate_lock(&dirs, &work).unwrap();
        let exclusive =
            hold_host_exclusive_in(&dirs, &work, std::time::Duration::from_secs(2)).unwrap();
        let copies = inherited_gate_locks().unwrap();
        let host_copies: Vec<&File> = copies
            .iter()
            .filter(|file| {
                FileIdentity::of(file).ok()
                    == FileIdentity::of(&open_lock_file(&dirs[0].join(HOST_LOCK_FILE)).unwrap())
                        .ok()
            })
            .collect();
        assert!(!host_copies.is_empty(), "watchers receive the host lock");
        for copy in host_copies {
            os_lock::try_lock(copy)
                .expect("the watcher's copy is the exclusive holder, not a released handle");
        }
        drop(copies);
        drop(exclusive);
        drop(lock);
    }

    /// TSK-1128 AC-2, AC-5: only descriptors of the holder's own locked
    /// open file description inherit; descriptors this process opened
    /// itself, or naming another file, fall back to ordinary admission, and
    /// a second gate still refuses while the inherited lock is in use.
    #[cfg(unix)]
    #[test]
    fn only_the_holders_own_descriptors_inherit_its_lock() {
        use std::os::fd::AsRawFd;
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = dirs(tmp.path());
        let numbers = |files: &[File]| files.iter().map(AsRawFd::as_raw_fd).collect::<Vec<_>>();
        let own: Vec<File> = dirs
            .iter()
            .map(|dir| open_lock_file(&dir.join(LOCK_FILE)).unwrap())
            .collect();
        assert!(
            inherit_gate_lock(&dirs, &numbers(&own), Vec::new())
                .unwrap()
                .is_none(),
            "a free lock has no holder to inherit from"
        );
        let holder = acquire_full_gate_lock(&dirs, &work).expect("the probe left the lock free");
        assert!(
            inherit_gate_lock(&dirs, &numbers(&own), Vec::new())
                .unwrap()
                .is_none(),
            "descriptors opened outside the holder are refused"
        );
        std::fs::write(tmp.path().join("other"), "").unwrap();
        let other = vec![File::open(tmp.path().join("other")).unwrap()];
        assert!(inherit_gate_lock(&dirs, &numbers(&other), Vec::new())
            .unwrap()
            .is_none());
        let first = holder.files.first().map(|file| file.0.as_raw_fd()).unwrap();
        assert!(
            inherit_gate_lock(&dirs, &[first], Vec::new())
                .unwrap()
                .is_none(),
            "every lock must be handed over"
        );

        let handed: Vec<_> = holder.files.iter().map(|file| file.0.as_raw_fd()).collect();
        let inherited = inherit_gate_lock(&dirs, &handed, Vec::new())
            .unwrap()
            .expect("the holder's descriptors inherit");
        assert!(inherited.notes[0].contains("enclosing `codeflow reserve`"));
        assert_eq!(inherited.runs, 1, "the nested run is the holder's run");
        let beside = acquire_full_gate_lock(&dirs, &work).expect("another run is admitted");
        assert_eq!(beside.runs, 2);
        assert!(beside.paths.iter().all(|path| path.ends_with(slot_file(1))));
        drop(beside);
        drop(inherited);
        let fresh = open_lock_file(&dirs[0].join(LOCK_FILE)).unwrap();
        assert!(
            os_lock::try_lock(&fresh).is_err(),
            "the inherited lock never releases the holder's slot"
        );
        let record = std::fs::read_to_string(dirs[0].join(RECORD_FILE)).unwrap();
        assert!(
            record.contains(&format!("pid={}", std::process::id())),
            "the inherited lock never rewrites the holder's record: {record}"
        );
        drop(holder);
    }

    /// TSK-1128 (Grok round 02, N6): a rewrite that read the record before
    /// another process added a group line still keeps that line, because
    /// both rewrites hold the record lock from read to rename.
    #[cfg(unix)]
    #[test]
    fn a_late_record_rewrite_keeps_a_group_added_meanwhile() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = dirs(tmp.path());
        let lock = acquire_gate_lock(&dirs, &work, "reserve").unwrap();
        let path = lock.paths[0].clone();
        let trusted = trusted_dirs(&work);
        // Live process groups stand for the reserve's command and the
        // nested run's target; a rewrite drops only groups that ended.
        let spawn = || {
            std::process::Command::new("sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .unwrap()
        };
        let (mut command, mut target) = (spawn(), spawn());
        let (mine, foreign) = (command.id(), target.id());
        let own = Held {
            id: u64::MAX,
            paths: vec![path.clone()],
            record: "pid=1\nmode=reserve\n".to_string(),
            groups: BTreeSet::from([mine]),
            recorded: BTreeSet::from([mine]),
            inherited: false,
            host: Vec::new(),
            trusted: trusted.clone(),
            runs: 1,
            descriptors: Vec::new(),
        };
        // At the rewrite's read, a nested run (an inherited lock) adds the
        // target's group. Give it a moment: unserialized, it
        // finishes before the late rewrite renames its stale merge.
        let adder = std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot = adder.clone();
        let (other_path, other_trusted) = (path.clone(), trusted.clone());
        interleave::set(move |point| {
            if point != interleave::Point::RecordRead || slot.borrow().is_some() {
                return;
            }
            let (path, trusted) = (other_path.clone(), other_trusted.clone());
            let (done, finished) = std::sync::mpsc::channel();
            *slot.borrow_mut() = Some(std::thread::spawn(move || {
                let nested = Held {
                    id: u64::MAX - 1,
                    paths: vec![path.clone()],
                    record: String::new(),
                    groups: BTreeSet::from([foreign]),
                    recorded: BTreeSet::from([foreign]),
                    inherited: true,
                    host: Vec::new(),
                    trusted,
                    runs: 1,
                    descriptors: Vec::new(),
                };
                assert_eq!(rewrite_serialized(&nested, &path), Vec::<String>::new());
                let _ = done.send(());
            }));
            let _ = finished.recv_timeout(std::time::Duration::from_millis(300));
        });
        let failures = rewrite_serialized(&own, &path);
        interleave::clear();
        assert_eq!(failures, Vec::<String>::new());
        let other = adder.borrow_mut().take().expect("the other rewrite ran");
        other.join().unwrap();
        let record = std::fs::read_to_string(record_path(&path)).unwrap();
        for child in [&mut command, &mut target] {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(
            record
                .lines()
                .any(|line| line == format!("group={foreign}")),
            "the group added meanwhile was dropped: {record}"
        );
        assert!(
            record.lines().any(|line| line == format!("group={mine}")),
            "{record}"
        );
        drop(lock);
    }

    #[test]
    fn hand_over_values_parse_strictly() {
        assert_eq!(parse_descriptors("7"), Some(vec![7]));
        assert_eq!(parse_descriptors("7,9"), Some(vec![7, 9]));
        for bad in ["", "2", "7,7", "x", "7,", "-1", "3,4,5,6,7"] {
            assert_eq!(parse_descriptors(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_repository_folder_alone_still_counts_a_run() {
        // A sandboxed run may reach only the repository folder; it must
        // still see a run that holds both.
        let tmp = tempfile::tempdir().unwrap();
        let _both = acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/w")).unwrap();
        let repo_only = vec![tmp.path().join("repo-locks")];
        let alone = acquire_full_gate_lock(&repo_only, Path::new("/w")).unwrap();
        assert_eq!(alone.runs, 2);
    }

    #[test]
    fn a_stale_lock_from_a_dead_process_is_reclaimed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        // A record with no flock behind it: its process died without cleanup.
        std::fs::write(
            dir.join(LOCK_FILE),
            "pid=999999\nstarted=1\ndir=/gone/worktree\n",
        )
        .unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert!(
            lock.notes
                .iter()
                .any(|n| n.contains("reclaimed a stale gate lock")
                    && n.contains("pid 999999 in an unknown directory")),
            "{:?}",
            lock.notes
        );
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
        assert!(record.contains(&format!("pid={}", std::process::id())));
    }

    #[cfg(unix)]
    #[test]
    fn a_free_slot_whose_target_group_runs_counts_as_busy() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let mut target = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = target.id();
        // The record a killed gate leaves: its pid is gone, its target is not.
        std::fs::write(
            dir.join(LOCK_FILE),
            format!("pid=999999\nstarted=1\ndir=/gone\ngroup={group}\n"),
        )
        .unwrap();
        let beside = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect("a run is admitted beside a surviving target");
        assert_eq!(beside.runs, 2, "the surviving target counts as a run");
        assert!(beside.paths[0].ends_with(slot_file(1)));
        assert!(
            std::fs::read_to_string(dir.join(LOCK_FILE))
                .unwrap()
                .contains(&format!("group={group}")),
            "the record stays for the surviving target"
        );
        let refused = hold_host_exclusive_in(
            std::slice::from_ref(&dir),
            Path::new("/w"),
            std::time::Duration::ZERO,
        )
        .unwrap_err();
        assert!(refused.contains("host-exclusive target waits"), "{refused}");
        drop(beside);

        target.kill().unwrap();
        target.wait().unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert!(lock.paths[0].ends_with(LOCK_FILE));
        assert!(
            lock.notes
                .iter()
                .any(|n| n.contains("reclaimed a stale gate lock")),
            "{:?}",
            lock.notes
        );
    }

    #[test]
    fn a_held_lock_records_running_target_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        // A group number no other test uses: records are shared by every
        // lock this process holds.
        target_group_started(4_000_001).unwrap();
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
        assert!(record.contains("group=4000001"), "{record}");
        assert!(record.contains(&format!("pid={}", std::process::id())));
        target_group_finished(4_000_001).unwrap();
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
        assert!(!record.contains("group=4000001"), "{record}");
        drop(lock);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap(), "");
        // The lock file itself is never written (TSK-1003).
        assert_eq!(std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap(), "");
    }

    #[test]
    fn an_unopenable_lock_refuses_and_releases_other_locks() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        let usable = tmp.path().join("repo-locks");
        let unusable = vec![usable.clone(), blocker.join("locks")];
        let error = acquire_full_gate_lock(&unusable, Path::new("/w")).unwrap_err();
        assert!(error.to_string().contains("gate lock unavailable"));
        let file = open_lock_file(&usable.join(LOCK_FILE)).unwrap();
        os_lock::try_lock(&file).expect("partial acquisition released its OS lock");
    }

    #[test]
    fn the_builder_write_probe_covers_locks_objects_and_refs() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let found = builder_write_probe(tmp.path(), Some(&home));
        let dirs: Vec<&PathBuf> = found.iter().map(|(dir, _)| dir).collect();
        assert_eq!(dirs.len(), 4, "{dirs:?}");
        assert_eq!(dirs[0], &home.join("locks"));
        assert!(dirs[1].ends_with(".git/codeflow"), "{dirs:?}");
        assert!(dirs[2].ends_with(".git/objects"), "{dirs:?}");
        assert!(dirs[3].ends_with(".git/refs"), "{dirs:?}");
        assert!(found.iter().all(|(_, result)| result.is_ok()), "{found:?}");
        for dir in &dirs[2..] {
            let left: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("write-probe"))
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }
        let without_home = builder_write_probe(tmp.path(), None);
        assert_eq!(without_home.len(), 4);
        assert!(without_home[0].1.is_err());
        let plain = tempfile::tempdir().unwrap();
        let outside = builder_write_probe(plain.path(), Some(&home));
        assert!(
            outside.iter().any(|(_, result)| result.is_err()),
            "{outside:?}"
        );
    }

    #[test]
    fn lock_dirs_cover_home_and_the_git_common_dir() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let found = lock_dirs(tmp.path(), Some(&home)).unwrap();
        assert_eq!(found[0], home.join("locks"));
        assert!(found[1].ends_with(".git/codeflow"), "{found:?}");
    }

    /// TSK-216 AC-3: doctor's probe names each gate directory this process
    /// cannot write. Where it can write, it creates the directories and the
    /// empty lock files a gate would, and keeps them: a lock file is shared
    /// with any running gate and is never unlinked. Only its evidence probe
    /// file is removed.
    #[test]
    fn unwritable_gate_dirs_names_each_directory_the_gate_cannot_write() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        assert!(unwritable_gate_dirs(tmp.path(), Some(&home)).is_empty());
        let evidence: Vec<_> = std::fs::read_dir(home.join(HOME_EVIDENCE_DIR))
            .unwrap()
            .collect();
        assert!(evidence.is_empty(), "the probe file is removed");
        let lock = home.join(HOME_LOCK_DIR).join(LOCK_FILE);
        assert_eq!(
            std::fs::metadata(&lock).map(|m| m.len()).ok(),
            Some(0),
            "{lock:?}"
        );

        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        let found: Vec<PathBuf> = unwritable_gate_dirs(tmp.path(), Some(&blocker))
            .into_iter()
            .map(|(dir, _)| dir)
            .collect();
        assert_eq!(
            found,
            [blocker.join(HOME_LOCK_DIR), blocker.join(HOME_EVIDENCE_DIR)]
        );
    }

    /// TSK-1044 review 1: a link at the home folder itself is refused, not
    /// trusted as the walk's root.
    #[cfg(unix)]
    #[test]
    fn the_evidence_probe_refuses_a_linked_home() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::os::unix::fs::symlink(outside.path(), &home).unwrap();
        let error = probe_evidence_dir(&home).unwrap_err();
        assert!(
            error.to_string().contains("home is a symbolic link"),
            "{error}"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    /// TSK-1044 AC-2: a link planted at the evidence folder, or at the
    /// probe's own name, is refused; the probe writes and removes nothing
    /// where it points.
    #[cfg(unix)]
    #[test]
    fn the_evidence_probe_never_follows_a_planted_link() {
        for at_probe in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("victim"), "victim").unwrap();
            let home = tmp.path().join("home");
            std::fs::create_dir_all(&home).unwrap();
            if at_probe {
                std::fs::create_dir(home.join(HOME_EVIDENCE_DIR)).unwrap();
                std::os::unix::fs::symlink(
                    outside.path().join("victim"),
                    home.join(HOME_EVIDENCE_DIR)
                        .join(format!(".write-probe-{}", std::process::id())),
                )
                .unwrap();
            } else {
                std::os::unix::fs::symlink(outside.path(), home.join(HOME_EVIDENCE_DIR)).unwrap();
            }
            let error = probe_evidence_dir(&home).unwrap_err();
            if !at_probe {
                assert!(error.to_string().contains("is a symbolic link"), "{error}");
            }
            let names: Vec<_> = std::fs::read_dir(outside.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, ["victim"]);
            assert_eq!(
                std::fs::read(outside.path().join("victim")).unwrap(),
                b"victim"
            );
        }
    }

    /// TSK-1003: a sandboxed seat that may write a lock folder must not be
    /// able to redirect a later, less restricted gate's writes. A lock file
    /// planted as a link is refused and its target is left unchanged.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_planted_as_a_link_is_refused_and_its_target_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let target = tmp.path().join("sentinel");
        std::fs::write(&target, "keep me\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(LOCK_FILE)).unwrap();
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a linked lock file is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
        assert!(read_holder(&dir.join(LOCK_FILE), None, &[]).is_err());
    }

    /// TSK-1003: a lock folder replaced by a link is refused as well.
    #[cfg(unix)]
    #[test]
    fn a_lock_folder_planted_as_a_link_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = elsewhere.join(LOCK_FILE);
        std::fs::write(&target, "keep me\n").unwrap();
        let dir = tmp.path().join("locks");
        std::os::unix::fs::symlink(&elsewhere, &dir).unwrap();
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a linked lock folder is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    }

    /// TSK-1003: a special file in place of the lock is refused, without
    /// blocking on it.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_that_is_a_fifo_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo =
            std::ffi::CString::new(dir.join(LOCK_FILE).as_os_str().as_encoded_bytes().to_vec())
                .unwrap();
        // SAFETY: a NUL-terminated path and a plain mode.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a FIFO lock is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
    }

    /// TSK-1003: once the lock is held, its record is updated through the
    /// acquired descriptor. A path swapped for a link afterwards is never
    /// written through.
    #[cfg(unix)]
    #[test]
    fn a_lock_path_swapped_for_a_link_after_acquisition_is_not_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        let target = tmp.path().join("sentinel");
        std::fs::write(&target, "keep me\n").unwrap();
        std::fs::remove_file(dir.join(LOCK_FILE)).unwrap();
        std::os::unix::fs::symlink(&target, dir.join(LOCK_FILE)).unwrap();
        target_group_started(4_000_003).unwrap();
        target_group_finished(4_000_003).unwrap();
        drop(lock);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    }

    /// TSK-1003 round three: a lock file planted as a hard link to an outside
    /// file is refused at both lock locations, by the gate and by the status
    /// read, and the outside file is neither read nor changed.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_planted_as_a_hard_link_is_never_written_or_printed() {
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let target = tmp.path().join(format!("sentinel-{index}"));
            std::fs::write(&target, "pid=1\nSECRET keep me\n").unwrap();
            std::fs::hard_link(&target, dir.join(LOCK_FILE)).unwrap();
            // Its text is no holder record, so the gate refuses; it says so
            // without printing the text.
            let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                .expect_err("an unreadable legacy record is refused");
            assert!(
                error.to_string().contains("unreadable gate lock record")
                    && !error.to_string().contains("SECRET"),
                "{error}"
            );
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(status.is_err(), "{status:?}");
            assert!(!format!("{status:?}").contains("SECRET"), "{status:?}");
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "pid=1\nSECRET keep me\n"
            );
        }
    }

    /// TSK-1003 round three: a lock file swapped for another regular file
    /// while it is being taken is not trusted: the path and the locked
    /// descriptor must name the same file.
    #[cfg(unix)]
    #[test]
    fn a_lock_whose_path_names_another_file_after_locking_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let path = dir.join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "").unwrap();
        assert!(same_lock_file(&path, &file).is_err());
        let fresh = open_lock_file(&path).unwrap();
        assert!(same_lock_file(&path, &fresh).is_ok());
    }

    /// TSK-1021 AC-1: the check compares the identity of the held handle
    /// with the anchored reopen on every platform: the lock path's own file
    /// passes, a handle on another file is refused.
    #[test]
    fn a_lock_check_compares_file_identities() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("locks").join(LOCK_FILE);
        let other = tmp.path().join("other").join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        let elsewhere = open_lock_file(&other).unwrap();
        assert!(same_lock_file(&path, &file).is_ok());
        let error = same_lock_file(&path, &elsewhere).expect_err("another file");
        assert!(error.to_string().contains("another file"), "{error}");
    }

    /// TSK-1003 round four, the review's interleaving: the lock entry is a
    /// hard link to an outside file when it is opened, the entry is removed
    /// before any check (the outside file now has one name), and the entry
    /// is linked to the outside file again once the lock is taken. Nothing
    /// in the outside file may change, at either lock location.
    #[cfg(unix)]
    #[test]
    fn unlink_and_relink_around_the_checks_never_writes_the_outside_file() {
        use std::rc::Rc;
        let tmp = tempfile::tempdir().unwrap();
        // The first text is no record, so acquisition refuses; the second
        // parses as a stale record, so acquisition succeeds and the group
        // update and the drop run under the same interleaving.
        let cases = [
            ("SECRET-OUTSIDE\n", false),
            ("pid=999999\nstarted=1\n", true),
        ];
        let locations = ["home/locks", "repo/.git/codeflow"];
        for (index, (location, (text, acquires))) in locations
            .iter()
            .flat_map(|location| cases.iter().map(move |case| (*location, *case)))
            .enumerate()
        {
            let dir = tmp.path().join(format!("case-{index}")).join(location);
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = Rc::new(tmp.path().join(format!("outside-{index}")));
            std::fs::write(&*sentinel, text).unwrap();
            let entry = Rc::new(dir.join(LOCK_FILE));
            std::fs::hard_link(&*sentinel, &*entry).unwrap();
            let (s, e) = (Rc::clone(&sentinel), Rc::clone(&entry));
            interleave::set(move |point| match point {
                interleave::Point::Opened => std::fs::remove_file(&*e).unwrap(),
                interleave::Point::Locked => std::fs::hard_link(&*s, &*e).unwrap(),
                interleave::Point::RecordRead => {}
            });
            let result = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"));
            interleave::clear();
            assert_eq!(result.is_ok(), acquires, "{dir:?}: {result:?}");
            if let Ok(lock) = result {
                target_group_started(4_000_005).unwrap();
                target_group_finished(4_000_005).unwrap();
                drop(lock);
            }
            assert_eq!(
                std::fs::read_to_string(&*sentinel).unwrap(),
                text,
                "outside file changed at {dir:?}"
            );
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(
                !status.as_deref().unwrap_or_default().contains("SECRET"),
                "{status:?}"
            );
        }
    }

    /// TSK-1003 round four: a holder record planted as a hard link to an
    /// outside file is replaced by rename, never written through, and the
    /// status read never prints the outside file's text.
    #[cfg(unix)]
    #[test]
    fn a_planted_record_link_is_replaced_never_written_or_printed() {
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = tmp.path().join(format!("record-outside-{index}"));
            std::fs::write(&sentinel, "SECRET-OUTSIDE\n").unwrap();
            std::fs::hard_link(&sentinel, dir.join(RECORD_FILE)).unwrap();
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(
                !status.as_deref().unwrap_or_default().contains("SECRET"),
                "{status:?}"
            );
            let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                .expect_err("an unreadable record is refused");
            assert!(!error.to_string().contains("SECRET"), "{error}");
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n"
            );
            // A planted record that parses is reclaimed, and the new record
            // replaces the planted name by rename: the outside file keeps
            // its text.
            let stale = tmp.path().join(format!("stale-outside-{index}"));
            std::fs::write(&stale, "pid=999999\n").unwrap();
            std::fs::remove_file(dir.join(RECORD_FILE)).unwrap();
            std::fs::hard_link(&stale, dir.join(RECORD_FILE)).unwrap();
            let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
            target_group_started(4_000_007).unwrap();
            target_group_finished(4_000_007).unwrap();
            drop(lock);
            assert_eq!(std::fs::read_to_string(&stale).unwrap(), "pid=999999\n");
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n"
            );
        }
    }

    /// TSK-1003 round four (SEC-1): an attacker thread that keeps removing
    /// the lock entry and linking it to an outside file again, while gates
    /// take and release the lock and record groups, never changes the
    /// outside file, at either lock location.
    #[cfg(unix)]
    #[test]
    fn a_racing_relinker_never_changes_the_outside_file() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = tmp.path().join(format!("raced-{index}"));
            std::fs::write(&sentinel, "SECRET-OUTSIDE\n").unwrap();
            let entry = dir.join(LOCK_FILE);
            std::fs::hard_link(&sentinel, &entry).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let attacker = {
                let (stop, sentinel, entry) = (Arc::clone(&stop), sentinel.clone(), entry.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = std::fs::remove_file(&entry);
                        let _ = std::fs::hard_link(&sentinel, &entry);
                    }
                })
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(lock) =
                    acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                {
                    target_group_started(4_000_009).unwrap();
                    target_group_finished(4_000_009).unwrap();
                    drop(lock);
                }
                if std::fs::read_to_string(&sentinel).unwrap() != "SECRET-OUTSIDE\n" {
                    break;
                }
            }
            stop.store(true, Ordering::Relaxed);
            attacker.join().unwrap();
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n",
                "outside file changed at {dir:?}"
            );
        }
    }

    /// TSK-1003: a gate of a pre-3.2 binary keeps its record inside the lock
    /// file. While the record file names no holder, that legacy record is
    /// still read, so its killed gate's live targets keep the slot busy.
    #[cfg(unix)]
    #[test]
    fn a_legacy_record_is_read_while_the_record_file_is_empty() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let mut target = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = target.id();
        std::fs::write(dir.join(RECORD_FILE), "").unwrap();
        std::fs::write(
            dir.join(LOCK_FILE),
            format!("pid=999999\nstarted=1\ndir=/old\ngroup={group}\n"),
        )
        .unwrap();
        let beside = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert_eq!(
            beside.runs, 2,
            "the legacy record's live group keeps slot 0 busy"
        );
        assert!(beside.paths[0].ends_with(slot_file(1)));
        drop(beside);
        target.kill().unwrap();
        target.wait().unwrap();
    }

    /// TSK-1003 round five (SEC-3): text planted in either lock file name,
    /// at either lock location, never reaches any output: not a refusal,
    /// not a note and not the status line. A `group` value that is not a
    /// process group refuses with a fixed message; a `dir` that is not an
    /// existing directory is shown as unknown.
    #[test]
    fn planted_record_text_never_reaches_any_output() {
        let tmp = tempfile::tempdir().unwrap();
        // The review's case: a link to a folder whose name holds record
        // lines, which a resolved `dir` would have injected (SEC-4).
        #[cfg(unix)]
        let injected = {
            let base = std::fs::canonicalize(tmp.path()).unwrap();
            let target = base.join("x\npid=SECRET_PID\nmode=SECRET_MODE");
            std::fs::create_dir(&target).unwrap();
            let link = base.join("injecting-link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            format!("pid=999999\ndir={}\n", link.display())
        };
        #[cfg(not(unix))]
        let injected = "pid=999999\ndir=/SECRET_PID\n".to_string();
        let mut case = 0;
        for location in ["home/locks", "repo/.git/codeflow"] {
            for name in [LOCK_FILE, RECORD_FILE] {
                for planted in [
                    "pid=999999\ngroup=SECRET\n",
                    "pid=999999\ndir=/SECRET-dir\n",
                    "pid=999999\nmode=SECRET\n",
                    injected.as_str(),
                ] {
                    case += 1;
                    let dir = tmp.path().join(format!("case-{case}")).join(location);
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(name), planted).unwrap();
                    let mut output = String::new();
                    let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                    match acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")) {
                        Ok(lock) => {
                            output.push_str(&lock.notes.join("\n"));
                            drop(lock);
                        }
                        Err(held) => {
                            let _ = write!(output, "{held}\n{held:?}");
                        }
                    }
                    let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                    assert!(
                        !output.contains("SECRET"),
                        "{location}/{name} {planted:?}:\n{output}"
                    );
                }
            }
        }
    }

    /// TSK-1003 round six (SEC-4, SEC-5): a planted `dir` value is never
    /// resolved. Only a value equal to a folder the gate already trusts is
    /// shown; a link to a folder whose name holds a newline cannot inject
    /// record lines, and an outside or relative existing folder, or a UNC
    /// path, shows as an unknown directory.
    #[cfg(unix)]
    #[test]
    fn a_planted_dir_value_is_never_resolved_or_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let injected = base.join("x\nmode=PLANTED");
        std::fs::create_dir(&injected).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&injected, &link).unwrap();
        let outside = base.join("outside-existing");
        std::fs::create_dir(&outside).unwrap();
        let mut case = 0;
        for (value, secret) in [
            (link.display().to_string(), "PLANTED"),
            (outside.display().to_string(), "outside-existing"),
            ("src".to_string(), "codeflow-core"),
            ("\\\\host\\share".to_string(), "host"),
        ] {
            for name in [LOCK_FILE, RECORD_FILE] {
                case += 1;
                let dir = base.join(format!("case-{case}")).join("locks");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(name), format!("pid=999999\ndir={value}\n")).unwrap();
                let mut output = String::new();
                let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                match acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")) {
                    Ok(lock) => {
                        output.push_str(&lock.notes.join("\n"));
                        drop(lock);
                    }
                    Err(held) => {
                        let _ = write!(output, "{held}\n{held:?}");
                    }
                }
                assert!(
                    !output.contains(secret) && output.contains("unknown directory"),
                    "{name} dir={value:?}:\n{output}"
                );
            }
        }
    }

    /// TSK-1003: with an existing lock file in a folder the process cannot
    /// write, the lock opens read only but the holder record cannot be
    /// written, so the gate refuses instead of running uncoordinated, and
    /// the permissions probe names the folder.
    #[cfg(unix)]
    #[test]
    fn a_read_only_lock_folder_refuses_and_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let dir = home.join(HOME_LOCK_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOCK_FILE), "").unwrap();
        std::fs::write(dir.join(HOST_LOCK_FILE), "").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let refused = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"));
        let reported = unwritable_gate_dirs(tmp.path(), Some(&home));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = refused.expect_err("a gate that cannot write its record refuses");
        assert!(
            error
                .to_string()
                .contains("holder record cannot be written"),
            "{error}"
        );
        assert!(
            reported.iter().any(|(found, _)| *found == dir),
            "{reported:?}"
        );
    }

    #[test]
    fn target_dir_inside_the_worktree_gives_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        for value in ["target", "./build/cargo", ""] {
            assert!(
                check_cargo_target_dir(&root, &root, Some(std::ffi::OsStr::new(value))).is_none(),
                "{value}"
            );
        }
        let abs = root.join("target");
        assert!(check_cargo_target_dir(&root, &root, Some(abs.as_os_str())).is_none());
        assert!(check_cargo_target_dir(&root, &root, None).is_none());
    }

    #[test]
    fn target_dir_outside_the_worktree_warns_naming_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        let shared = tmp.path().join("shared-target");
        let warning = check_cargo_target_dir(&root, &root, Some(shared.as_os_str())).unwrap();
        assert!(warning.contains("shared-target"), "{warning}");
        assert!(warning.contains("in parallel"), "{warning}");
        // A relative escape resolves against the working directory.
        let escape = check_cargo_target_dir(&root, &root, Some(std::ffi::OsStr::new("../x")));
        assert!(escape.is_some());
    }
}

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_lock_dirs_refuse_corrupt_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(lock_dirs(dir.path(), None).unwrap().is_empty());
        std::fs::write(dir.path().join(".git"), "broken gitdir").unwrap();
        assert!(lock_dirs(dir.path(), None).is_err());
    }
    #[test]
    fn r22_invalid_group_record_cannot_be_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        for group in ["oops", "4294967296", "0", "1"] {
            let path = dir.path().join(LOCK_FILE);
            std::fs::write(&path, format!("pid=999999\ngroup={group}\n")).unwrap();
            let result = acquire_full_gate_lock(&[dir.path().to_path_buf()], dir.path());
            assert!(result.is_err(), "{group}");
            assert!(std::fs::read_to_string(path).unwrap().contains(group));
        }
        std::fs::write(dir.path().join(LOCK_FILE), "pid=999999\n").unwrap();
        assert!(acquire_full_gate_lock(&[dir.path().to_path_buf()], dir.path()).is_ok());
    }
}

#[cfg(test)]
mod heavy_admission_tests {
    use super::*;

    #[test]
    fn essential_record_adds_only_mode_and_legacy_reader_keeps_holder_fields() {
        let record = holder_record(Path::new("/work/essential"), "essential");
        let keys: BTreeSet<_> = record
            .lines()
            .map(|line| line.split_once('=').unwrap().0)
            .collect();
        assert_eq!(keys, BTreeSet::from(["pid", "started", "dir", "mode"]));
        // 3.1 looks up known key prefixes and ignores additional keys.
        let old_field = |key: &str| {
            record
                .lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .map(str::trim)
        };
        assert_eq!(
            old_field("pid"),
            Some(std::process::id().to_string().as_str())
        );
        assert_eq!(old_field("dir"), Some("/work/essential"));
        assert!(old_field("started").unwrap().parse::<u64>().is_ok());
        assert!(describe_holder(&record).contains("essential mode"));
        assert!(describe_holder("pid=9\ndir=/old\n").contains("full mode"));
    }

    #[cfg(unix)]
    #[test]
    fn refusal_unlocks_even_when_a_fork_duplicate_keeps_the_descriptor_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        os_lock::try_lock(&file).unwrap();
        // dup and fork share the open file description behind flock.
        let inherited = file.try_clone().unwrap();
        let refused = AcquiredFile(file);
        drop(refused);
        let next = open_lock_file(&path).unwrap();
        os_lock::try_lock(&next).expect("explicit unlock also releases inherited descriptors");
        os_lock::unlock(&next);
        drop(inherited);
    }
}
