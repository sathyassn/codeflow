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
//! before targets start. Slot `n` is `full-gate-<n>.lock`, from 1 up.
//! A lock that cannot be opened or a record that cannot be read refuses,
//! naming the path and error (fail closed).
//!
//! **Older binaries are fenced out** (TSK-1256). 3.1.x and the line builds
//! before TSK-1256 know only [`LOCK_FILE`], which they lock exclusively, and
//! run alone. Every new run, inherited ones included, holds [`LOCK_FILE`]
//! shared in every folder for its whole life, before it takes any other
//! lock, so an older gate refuses while any new run is active. While an
//! older gate holds it, or targets an older gate started outlive it, a new
//! run refuses at once, naming the last recorded holder, and takes nothing
//! else; it does not wait. A shared take that fails for any other reason
//! refuses (fail closed). Each new run writes [`RECORD_FILE`] at admission,
//! in the one-holder format older readers parse, so an older gate's refusal
//! names a holder: the last admitted run, which may have ended, since no
//! release rewrites it. `codeflow test --lock-status` and the slot records
//! show the recorded slot and target group information. Builds between the landed TSK-1145 change and
//! TSK-1256 fall back to a later slot when `full-gate.lock` is busy, so they
//! are not fenced out; they are drained or rebuilt before a new build runs
//! on the same machine. The shipped Claude settings presets allow sandboxed
//! writes to `~/.codeflow/locks` only (TSK-216); no preset grants
//! `~/.codeflow/gate-runs`, so a full gate that keeps its evidence runs
//! outside the sandbox (issue 254). `codeflow doctor --check permissions` names any folder this process
//! cannot write ([`unwritable_gate_dirs`]). A record that still names a
//! holder while its slot is free was left by a process that died, and is
//! reclaimed.
//!
//! **Lock folders may be writable by a sandboxed seat** (TSK-1003), so a
//! lock file is never written: it is opened read only, without following
//! links, and only locked. Each holder record (`full-gate.record` beside
//! the fence, `full-gate-<n>.record` beside slot `n`, `host.record` beside
//! the host lock) is replaced whole by a new file renamed into place, read
//! without following links, and shown only through validated values
//! (numbers, a known mode, and a folder only when its text names one the
//! gate already trusts; a planted folder value is never resolved). A link
//! planted in the folder can be locked or replaced, never written through,
//! and reveals nothing beyond a folder the gate already trusts. A run that
//! cannot write its record refuses instead of running uncoordinated. The
//! folder stays an advisory coordination boundary: a seat that can write it
//! can block or split coordination between runs. A record a pre-3.2 gate
//! keeps inside the lock file is still shown while the record file names no
//! holder; for target groups both are always checked (TSK-1256).
//!
//! **Target lifetime on Unix.** Heavy quick targets and reserved commands
//! have a watcher that ends their process group when the controller exits;
//! it keeps copies of the run's slot, host and fence locks, so a target that
//! outlives its run keeps its slot busy and keeps a host-exclusive target
//! and older binaries out. Other targets can outlive a killed gate. Every
//! target of an admitted run inherits the run's fences, and a flock belongs
//! to the open file description, so the fence lasts while any process of
//! the target that kept the descriptor lives, grandchildren included; the
//! run only ever closes its own copy of a fence, never unlocks it
//! (TSK-1256). A descendant that closes its inherited descriptors, as some
//! daemons do, does not keep the fence. While a run holds a slot,
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

/// File name of the fence in each lock directory: the one lock older
/// binaries take, exclusively. Every new run holds it shared and takes no
/// slot here (TSK-1256). Lock files are only ever opened read only and
/// locked; `CodeFlow` never writes, truncates or deletes them (TSK-1003).
pub const LOCK_FILE: &str = "full-gate.lock";

/// File name of the holder record beside the fence: the last admitted run,
/// which may have ended, one holder, as older readers parse it (TSK-1256). Every lock's record is
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

/// The first slot index. Slot files start at 1, so no slot is the fence
/// ([`LOCK_FILE`]) and the files of slots 1 and up keep the names the
/// TSK-1145 version gave them (TSK-1256).
pub const FIRST_SLOT: usize = 1;

/// File name of slot `index` (from [`FIRST_SLOT`]): `full-gate-<index>.lock`.
/// A lock directory has no fixed number of slots: a run that finds every
/// existing slot busy creates the next one, so no run count refuses a run
/// (R483-06).
#[must_use]
pub fn slot_file(index: usize) -> String {
    debug_assert!(index >= FIRST_SLOT, "slot files start at {FIRST_SLOT}");
    format!("full-gate-{index}.lock")
}

/// The slot index a lock file name stands for, if it names a slot. The
/// fence ([`LOCK_FILE`]) is not a slot.
fn slot_index(name: &str) -> Option<usize> {
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
            dir.to_path_buf(),
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
    /// The fence held shared in each lock directory, with its path. It is
    /// closed after the slots and host locks, never unlocked, so targets
    /// that inherited it keep it (TSK-1256).
    fences: Vec<(PathBuf, FenceFile)>,
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
    /// The fence descriptors of this lock, which every target of the run inherits
    /// ([`fence_copies`]).
    #[cfg(unix)]
    fences: Vec<std::os::fd::RawFd>,
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
            // Before the locks release, under the record lock, so a target
            // update of another process cannot interleave: an empty record
            // names no holder, so readers fall back to a legacy record inside
            // the lock file, which a pre-3.2 gate may have left. Live groups
            // a nested run added stay, so the slot is busy while they run
            // (TSK-1128). A clear that cannot be done safely is reported and
            // skipped; the record then reads as a stale slot, which the next
            // run reclaims once its groups have ended.
            let clear = || -> Result<(), String> {
                let kept = match entry.as_ref() {
                    Some(entry) => foreign_live_groups(path, entry)
                        .map_err(|error| format!("its record could not be read: {error}"))?,
                    None => Vec::new(),
                };
                let mut text = String::new();
                for group in kept {
                    let _ = writeln!(text, "group={group}");
                }
                write_record(path, &text)
                    .map_err(|error| format!("its record could not be written: {error}"))
            };
            let failure = match with_record_lock(path, clear) {
                Ok(Ok(())) => None,
                Ok(Err(failure)) => Some(failure),
                Err(error) => Some(format!("its record lock could not be taken: {error}")),
            };
            if let Some(failure) = failure {
                eprintln!(
                    "warning: the gate lock {} was released without clearing its holder \
                     record: {failure}; the next run reclaims it as a stale slot",
                    crate::git::display_path(path)
                );
            }
        }
        // The fence record is left as written at admission: it names the
        // last admitted run, which may have ended (TSK-1256). AcquiredFile
        // explicitly unlocks before its descriptor closes: the slots
        // release, then the host locks with the entry. The fences close
        // last and are never unlocked: targets inherited them, and an
        // unlock would release them for those targets too.
        self.files.clear();
        drop(entry);
        self.fences.clear();
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
    /// An older binary's gate holds the fence ([`LOCK_FILE`]) exclusively,
    /// or targets such a gate started still run after it ended (TSK-1256).
    pub older_binary: bool,
}

impl LockHeld {
    fn unavailable(path: PathBuf, holder: String) -> Self {
        Self {
            path,
            holder,
            running_groups: Vec::new(),
            exclusive: false,
            older_binary: false,
        }
    }

    /// An older binary's gate holds the fence at `path`, or targets it
    /// started still run (`running_groups`); `holder` is the last recorded
    /// holder ([`fence_holder`]).
    fn older_binary(path: PathBuf, holder: String, running_groups: Vec<u32>) -> Self {
        Self {
            path,
            holder,
            running_groups,
            exclusive: false,
            older_binary: true,
        }
    }
}

impl std::fmt::Display for LockHeld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lock = crate::git::display_path(&self.path);
        if !self.exclusive && !self.older_binary {
            return write!(f, "{} (lock: {lock})", self.holder);
        }
        if self.older_binary {
            let last = if self.holder.is_empty() {
                "no recorded holder".to_string()
            } else {
                format!(
                    "last recorded holder: {}, which may have ended",
                    describe_holder(&self.holder)
                )
            };
            let what = if self.running_groups.is_empty() {
                "an older CodeFlow binary's gate is running on this machine".to_string()
            } else {
                let groups: Vec<String> = self
                    .running_groups
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                format!(
                    "targets an older CodeFlow binary's gate started still run on this \
                     machine after that gate ended (process groups {})",
                    groups.join(", ")
                )
            };
            return write!(
                f,
                "{what} ({last}; `codeflow test --lock-status` shows the recorded slots and target groups). \
                 Gates of CodeFlow 3.1 and other older binaries run alone, so this run \
                 started nothing; run it again when they end (lock: {lock})"
            );
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

/// Whether this process can take the steps git starts with to rewrite the
/// shared git config file of the repository holding `project_dir`
/// (TSK-1156), and the path probed. Git rewrites that file for any
/// `git config` change, `git push -u` and `git branch -m` of a branch with
/// tracking settings; a profile that cannot write it refuses those with
/// `could not write config file`.
///
/// The probe never replaces or changes the file, because the file controls
/// remotes and hooks and a replacement is a new file that could lose its
/// ACL, owner or security attributes. It creates `config.lock` exclusively
/// (the first step of git's own write, so no other git process writes
/// meanwhile, and proof the folder accepts a new file), opens the config
/// file for appending without writing a byte, and removes the lock. A held
/// lock is reported, never removed. The final rename of the lock onto the
/// config file is not tried, so `Ok` means the rewrite is not refused at
/// either step it can observe, not that the rename will succeed.
pub fn probe_git_config_write(project_dir: &Path) -> (PathBuf, std::io::Result<()>) {
    // The shared root comes from a validated registration, found without
    // opening the repository (which would read the config file, and block on
    // a pipe in its place). The probe works through the handle validation
    // opened and never resolves the path again.
    let tree = match crate::hooks::repo::shared_git_tree(project_dir) {
        Ok(tree) => tree,
        Err(error) => {
            return (
                project_dir.to_path_buf(),
                Err(std::io::Error::other(format!(
                "no usable git repository, so the shared git config file was not probed: {error}"
            ))),
            )
        }
    };
    (tree.path().join("config"), probe_config_in(&tree))
}

/// The probe steps on the validated shared root `tree`.
fn probe_config_in(tree: &crate::contained::Tree) -> std::io::Result<()> {
    tree.create_new("config.lock", b"")?;
    // An anchored append open of the existing regular file: it neither
    // creates, truncates nor writes, follows no link, and never blocks on a
    // special file. It asks the system whether this process may write the
    // file, and changes nothing.
    let opened = tree.open_regular_for_append("config").map(drop);
    let released = tree.remove("config.lock");
    match (opened, released) {
        (Ok(()), released) => released,
        (Err(opened), Ok(())) => Err(opened),
        (Err(opened), Err(released)) => Err(std::io::Error::new(
            opened.kind(),
            format!("{opened}; the lock file was also not removed: {released}"),
        )),
    }
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
/// the host lock's, the fence's and every existing slot's. These are snapshots: admission still checks
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
        lines.push(fence_status(&dir.join(LOCK_FILE), &trusted)?);
        for index in slot_indices(&dir).map_err(|held| held.to_string())? {
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

/// The status line for the fence at `path`: its record names the last
/// admitted run, which may have ended, so the line says so and points to
/// the slot lines, which show the recorded slots and target groups
/// (TSK-1256).
fn fence_status(path: &Path, trusted: &[String]) -> Result<String, String> {
    let line = holder_status(path, trusted)?;
    Ok(line.replacen(
        ": recorded holder: ",
        ": last recorded holder (may have ended; the slot lines show the recorded slots and target groups): ",
        1,
    ))
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

/// Admit a heavy run: hold the fence ([`LOCK_FILE`]) shared in every lock
/// directory first, then in each one hold the host lock shared and take the
/// first free slot, recording `mode` beside the slot and the fence. Other
/// active new runs never refuse it; they only raise [`GateLock::runs`].
/// # Errors
/// At once, before any slot is taken, while an older binary's gate holds a
/// fence (TSK-1256); while a `host_exclusive` target holds the host; when a
/// lock directory or lock cannot be opened, or a slot or fence record cannot
/// be read or written. Locks taken in this call are released before returning.
pub fn acquire_gate_lock(
    dirs: &[PathBuf],
    project_dir: &Path,
    mode: &str,
) -> Result<GateLock, LockHeld> {
    let mut lock = GateLock {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        files: Vec::new(),
        fences: Vec::new(),
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
    // Every fence first: an older binary's gate refuses this run before it
    // takes a slot or the host in any folder (TSK-1256).
    lock.fences = take_fences(dirs, &trusted)?;
    let mut host = Vec::new();
    for dir in dirs {
        host.push(take_host_shared(dir, &trusted)?);
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
    // Older readers name the holder from the fence record, so it names this
    // run, the last admitted one; nothing rewrites it on release, so it may
    // name a run that has ended (TSK-1256). A run that cannot write it
    // refuses, as for its slot record.
    for (fence, _) in &lock.fences {
        let written =
            with_record_lock(fence, || write_record(fence, &record)).and_then(|written| written);
        if let Err(error) = written {
            return Err(LockHeld::unavailable(
                fence.clone(),
                format!("gate lock unavailable: its holder record cannot be written: {error}"),
            ));
        }
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
                    .chain(lock.fences.iter().map(|(_, fence)| fence.file.as_raw_fd()))
                    .collect()
            },
            #[cfg(unix)]
            fences: {
                use std::os::fd::AsRawFd;
                lock.fences
                    .iter()
                    .map(|(_, fence)| fence.file.as_raw_fd())
                    .collect()
            },
            host,
            trusted,
            runs: lock.runs,
        });
    }
    // Admitted: from here on the fences only close (FenceFile).
    for (_, fence) in &mut lock.fences {
        fence.armed = false;
    }
    Ok(lock)
}

/// Hold the host lock of `dir` shared, with its path.
/// # Errors
/// While a `host_exclusive` target holds it, naming that target, or when it
/// cannot be opened or locked.
fn take_host_shared(dir: &Path, trusted: &[String]) -> Result<(PathBuf, AcquiredFile), LockHeld> {
    let path = dir.join(HOST_LOCK_FILE);
    match try_open_and_lock(&path, true)? {
        Slot::Taken(file) => Ok((path, AcquiredFile(file))),
        Slot::Busy => {
            let holder = read_holder(&path, None, trusted).unwrap_or_default();
            Err(LockHeld {
                path,
                holder,
                running_groups: Vec::new(),
                exclusive: true,
                older_binary: false,
            })
        }
    }
}

/// Hold the fence of every directory in `dirs` shared, with its path, in
/// order; the fences taken release if a later one refuses.
/// # Errors
/// As [`take_fence`].
fn take_fences(
    dirs: &[PathBuf],
    trusted: &[String],
) -> Result<Vec<(PathBuf, FenceFile)>, LockHeld> {
    dirs.iter()
        .map(|dir| Ok((dir.join(LOCK_FILE), take_fence(dir, trusted)?)))
        .collect()
}

/// Hold the fence ([`LOCK_FILE`]) in `dir` shared.
/// # Errors
/// At once while an older binary's gate holds the fence exclusively, or
/// while the fence record names a live target group, which only an older
/// binary's gate that ended while its targets ran leaves there, naming the
/// last recorded holder; when the fence cannot be opened or locked, or its
/// record cannot be read (fail closed).
fn take_fence(dir: &Path, trusted: &[String]) -> Result<FenceFile, LockHeld> {
    let path = dir.join(LOCK_FILE);
    let Slot::Taken(file) = try_open_and_lock(&path, true)? else {
        let holder = fence_holder(&path, trusted);
        return Err(LockHeld::older_binary(path, holder, Vec::new()));
    };
    // Armed until admission completes: a refusal before any target starts
    // unlocks it explicitly.
    let mut fence = FenceFile { file, armed: true };
    let (holder, running) = fence_live_groups(&path, Some(&mut fence.file), trusted)?;
    if !running.is_empty() {
        // The older gate is gone but its targets are not, and they run as
        // that gate did, alone; the shared hold releases as `fence` drops.
        return Err(LockHeld::older_binary(path, holder, running));
    }
    Ok(fence)
}

/// The live target groups an older binary's gate left on the fence at
/// `fence`, with the holder lines of the record that names them. Both
/// locations are read and checked independently, since either can carry an
/// orphan whatever the other says: the record inside the lock file, which a
/// 3.1 gate writes and whose targets never inherit the lock, and
/// [`RECORD_FILE`], which line builds before TSK-1256 write. The caller
/// holds the fence shared, so no older gate writes either meanwhile. The
/// lock file is read through `held` when given.
/// # Errors
/// When either record cannot be read or parsed, or names a value that is
/// not a target process group (fail closed).
fn fence_live_groups(
    fence: &Path,
    held: Option<&mut File>,
    trusted: &[String],
) -> Result<(String, Vec<u32>), LockHeld> {
    let unreadable = |error: std::io::Error| {
        LockHeld::unavailable(
            fence.to_path_buf(),
            format!("cannot read gate lock record: {error}"),
        )
    };
    let read_file = |file: &Path| -> std::io::Result<String> {
        if crate::absence::proven_absent(file)? {
            return Ok(String::new());
        }
        let (anchor, relative) = lock_anchor(file)?;
        read_record(&mut crate::contained::Tree::open(anchor)?.open_regular(&relative)?)
    };
    let inline = match held {
        Some(file) => read_record(file),
        None => read_file(fence),
    }
    .map_err(unreadable)?;
    let sidecar = read_file(&record_path(fence)).map_err(unreadable)?;
    let mut holder = String::new();
    let mut running: Vec<u32> = Vec::new();
    for text in [inline, sidecar] {
        let parsed = parse_record(&text, trusted).map_err(|error| {
            unreadable(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unreadable gate lock record: {error}"),
            ))
        })?;
        let live = live_groups(&parsed).map_err(|error| {
            LockHeld::unavailable(
                fence.to_path_buf(),
                format!("cannot reclaim gate lock: {error}; inspect and repair the lock record after verifying its processes"),
            )
        })?;
        if !live.is_empty() && holder.is_empty() {
            for line in parsed
                .split_terminator('\n')
                .filter(|line| !line.starts_with("group="))
            {
                let _ = writeln!(holder, "{line}");
            }
        }
        for group in live {
            if !running.contains(&group) {
                running.push(group);
            }
        }
    }
    Ok((holder, running))
}

/// The last holder recorded for the fence at `path`, for display only.
/// The record file (which line builds before TSK-1256 write) is shown only
/// while the process it names lives. Otherwise the record a 3.1 gate keeps
/// inside the lock file is shown as it stands, with no liveness check, so
/// it can name a 3.1 gate that has ended. Empty when neither is readable.
fn fence_holder(path: &Path, trusted: &[String]) -> String {
    let read = |file: &Path| -> Option<String> {
        if crate::absence::proven_absent(file).ok()? {
            return None;
        }
        let (anchor, relative) = lock_anchor(file).ok()?;
        let mut opened = crate::contained::Tree::open(anchor)
            .ok()?
            .open_regular(&relative)
            .ok()?;
        let text = read_record(&mut opened).ok()?;
        parse_record(&text, trusted)
            .ok()
            .filter(|record| !record.is_empty())
    };
    read(&record_path(path))
        .filter(|record| holder_alive(record))
        .or_else(|| read(path))
        .unwrap_or_default()
}

/// Whether the process a holder record names by `pid` still runs.
#[cfg(unix)]
fn holder_alive(record: &str) -> bool {
    let Some(pid) = record
        .split_terminator('\n')
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| libc::pid_t::try_from(value.parse::<u32>().ok()?).ok())
        .filter(|pid| *pid > 1)
    else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists; nothing is sent.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether the process a holder record names still runs on Windows.
/// This selects diagnostic text only; the fence has already refused admission.
#[cfg(windows)]
fn holder_alive(record: &str) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    let Some(pid) = record
        .split_terminator('\n')
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 1)
    else {
        return false;
    };
    // SAFETY: request only synchronization access to the named process,
    // with handle inheritance disabled. This does not change the process.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        // Like Unix EPERM, an inaccessible process may still be the holder.
        return std::io::Error::last_os_error().raw_os_error()
            == i32::try_from(ERROR_ACCESS_DENIED).ok();
    }
    // SAFETY: OpenProcess returned a valid handle, owned here exactly once.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: the owned process handle stays open throughout the zero-time
    // probe. A terminated process is signaled even while its handle exists.
    unsafe { WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT }
}

/// Whether the process a holder record names still runs. No check here: a
/// named holder is shown on platforms without a process-liveness probe.
#[cfg(not(any(unix, windows)))]
fn holder_alive(record: &str) -> bool {
    record
        .split_terminator('\n')
        .any(|line| line.starts_with("pid="))
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
    for index in FIRST_SLOT.. {
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
    /// The fence held shared in each lock directory, taken before the host,
    /// so an older binary's gate stays out while the target runs whatever
    /// called this (TSK-1256). The target inherits it ([`fence_copies`]);
    /// it closes after the host, never unlocked.
    fences: Vec<(PathBuf, FenceFile)>,
    /// This hold's entry in the registry [`fence_copies`] reads; Unix only,
    /// since targets inherit fences only there.
    #[cfg(unix)]
    id: u64,
}

impl Drop for HostExclusive {
    fn drop(&mut self) {
        #[cfg(unix)]
        host_fences().retain(|(id, _)| *id != self.id);
        for (path, _) in &self.files {
            if let Err(error) = write_record(path, "") {
                eprintln!(
                    "warning: the host-exclusive record beside {} could not be cleared: \
                     {error}; it names a hold that has ended",
                    crate::git::display_path(path)
                );
            }
        }
        release_exclusive(std::mem::take(&mut self.files));
        retake_shared(codeflow_limits::GATE_LOCK_RETAKE_SECONDS.wait());
        self.fences.clear();
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
/// since the reserve holds the host shared. The fence of every directory is
/// held shared first, for the life of the hold, so older binaries' gates
/// stay out whether or not the caller holds an admission (TSK-1256).
/// # Errors
/// At once while an older binary's gate or its surviving targets hold a
/// fence; when other heavy runs still hold the host after `wait`, the run is
/// nested under a reserve, or a lock cannot be opened. The shared locks are
/// taken back before returning.
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
    let fences = take_fences(dirs, &trusted).map_err(|held| with_repair_pointer(&held))?;
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
        std::thread::sleep(codeflow_limits::POLL_WAIT_MILLIS.wait());
    };
    if let Err(message) = result {
        release_exclusive(taken);
        retake_shared(wait.max(codeflow_limits::GATE_LOCK_RETAKE_SECONDS.wait()));
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
        retake_shared(codeflow_limits::GATE_LOCK_RETAKE_SECONDS.wait());
        return Err(format!(
            "gate lock unavailable: the host-exclusive record beside {} cannot be written: {error}",
            crate::git::display_path(&path)
        ));
    }
    Ok(held_exclusively(taken, fences))
}

/// The host-exclusive hold over the host locks `taken` and the `fences`
/// held for it. From here on the fences only close, and the target
/// inherits them as it inherits an admission's ([`fence_copies`]).
fn held_exclusively(
    taken: Vec<(PathBuf, Option<AcquiredFile>)>,
    fences: Vec<(PathBuf, FenceFile)>,
) -> HostExclusive {
    let mut hold = HostExclusive {
        files: taken,
        fences,
        #[cfg(unix)]
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
    };
    for (_, fence) in &mut hold.fences {
        fence.armed = false;
        #[cfg(unix)]
        host_fences().push((hold.id, std::os::fd::AsRawFd::as_raw_fd(&fence.file)));
    }
    hold
}

/// A busy slot in `dirs` that this process does not hold, or a fence whose
/// record names a live target group of an older binary's killed gate, if
/// any. The fence lock itself is not checked: this run holds it shared.
fn orphaned_slot(dirs: &[PathBuf], trusted: &[String]) -> Result<Option<PathBuf>, LockHeld> {
    let own: Vec<PathBuf> = held()
        .iter()
        .flat_map(|lock| lock.paths.iter().cloned())
        .collect();
    for dir in dirs {
        let fence = dir.join(LOCK_FILE);
        let (_, running) = fence_live_groups(&fence, None, trusted)?;
        if !running.is_empty() {
            return Ok(Some(fence));
        }
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
                std::thread::sleep(codeflow_limits::POLL_LOCK_MILLIS.wait());
            }
        }
    }
}

/// Names the lock descriptors `codeflow reserve` hands to a `codeflow`
/// command, as decimal descriptor numbers separated by commas (TSK-1128).
/// The value is a hint, never authority: `inherit_gate_lock` trusts a
/// descriptor only after the OS lock shows it is the holder's own.
pub const RESERVED_LOCKS_ENV: &str = "CODEFLOW_RESERVED_LOCK_FDS";

/// The most descriptors one hand-over names: a slot, the host lock and the
/// fence in each of the two lock directories.
const MOST_RESERVED_LOCKS: usize = codeflow_limits::GATE_MOST_RESERVED_LOCKS.default_count();

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
    // The fence, held shared by this process too, so a target that outlives
    // the reserve still keeps older binaries out (TSK-1256).
    let fences = take_fences(dirs, &trusted)?;
    // The host half of the lease, taken as ordinary admission takes it.
    let mut host = Vec::new();
    for dir in dirs {
        host.push(take_host_shared(dir, &trusted)?);
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
        fences,
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
            .chain(lock.fences.iter().map(|(_, fence)| fence.file.as_raw_fd()))
            .collect(),
        fences: lock
            .fences
            .iter()
            .map(|(_, fence)| fence.file.as_raw_fd())
            .collect(),
        host,
    });
    let mut lock = lock;
    for (_, fence) in &mut lock.fences {
        fence.armed = false;
    }
    Ok(Some(lock))
}

/// The fences of active host-exclusive holds, by hold, which every target
/// inherits as it inherits an admission's ([`fence_copies`]).
#[cfg(unix)]
static HOST_FENCES: Mutex<Vec<(u64, std::os::fd::RawFd)>> = Mutex::new(Vec::new());

#[cfg(unix)]
fn host_fences() -> std::sync::MutexGuard<'static, Vec<(u64, std::os::fd::RawFd)>> {
    HOST_FENCES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Copies of every fence this process holds, for a target to inherit
/// before it is spawned (TSK-1256). A copy shares the fence's open file
/// description, which owns the shared lock, so the fence lasts while any
/// process that kept a copy lives: a target, its grandchildren, and those
/// that outlive a killed gate. The copies are close-on-exec here; the
/// spawn clears that in the child only. The registry guards keep the
/// owners alive during the copy.
/// # Errors
/// When a descriptor cannot be duplicated; the target then does not start.
#[cfg(unix)]
pub(crate) fn fence_copies() -> std::io::Result<Vec<File>> {
    use std::os::fd::BorrowedFd;
    let held = held();
    let hosts = host_fences();
    held.iter()
        .flat_map(|lock| lock.fences.iter().copied())
        .chain(hosts.iter().map(|(_, fd)| *fd))
        .map(|descriptor| {
            // SAFETY: the registry guards keep the owning fence open.
            unsafe { BorrowedFd::borrow_raw(descriptor) }
                .try_clone_to_owned()
                .map(File::from)
        })
        .collect()
}

/// A fence ([`LOCK_FILE`]) held shared. Targets inherit copies of it, and
/// a flock belongs to the open file description, so an explicit unlock
/// through any copy releases it for all of them. Once a run is admitted
/// the fence is only ever closed; only a refusal before any target starts
/// (`armed`) unlocks it explicitly, so a sibling thread's fork that holds a
/// copy until its exec cannot keep a refused run's fence.
#[derive(Debug)]
struct FenceFile {
    file: File,
    armed: bool,
}

impl Drop for FenceFile {
    fn drop(&mut self) {
        if self.armed {
            os_lock::unlock(&self.file);
        }
    }
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

/// Remove a confirmed target group using one deadline for the registry and
/// every record lock. Callers defer this function: filesystem I/O itself is
/// not interruptible. A missed write leaves the on-disk record conservative.
#[cfg(unix)]
pub(crate) fn target_group_finished_before(
    pgid: u32,
    finish_by: std::time::Instant,
) -> Result<(), String> {
    update_groups_before(pgid, false, finish_by)
}

/// Bound initial target records as well as final cleanup records. Filesystem
/// I/O is deferred by the caller, under the same absolute wait budget.
#[cfg(unix)]
pub(crate) fn target_group_started_before(
    pgid: u32,
    finish_by: std::time::Instant,
) -> Result<(), String> {
    update_groups_before(pgid, true, finish_by)
}

#[cfg(unix)]
fn update_groups_before(
    pgid: u32,
    running: bool,
    finish_by: std::time::Instant,
) -> Result<(), String> {
    let mut locks = loop {
        if std::time::Instant::now() >= finish_by {
            return Err(
                "target group bookkeeping reached the shared deadline waiting for held records"
                    .into(),
            );
        }
        match HELD.try_lock() {
            Ok(locks) => break locks,
            Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(
                    codeflow_limits::POLL_GATE_RECORD_MILLIS
                        .wait()
                        .min(finish_by.saturating_duration_since(std::time::Instant::now())),
                );
            }
        }
    };
    let mut failures = Vec::new();
    for lock in locks.iter_mut() {
        if running {
            lock.groups.insert(pgid);
        } else {
            lock.groups.remove(&pgid);
        }
        lock.recorded.extend(lock.groups.iter().copied());
        for path in &lock.paths {
            if std::time::Instant::now() >= finish_by {
                failures.push("target group bookkeeping reached the shared deadline; remaining records retained".into());
                return Err(failures.join("; "));
            }
            match with_record_lock_before(path, finish_by, || rewrite_record(lock, path)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => failures.push(error),
                Err(error) => failures.push(format!(
                    "target group record beside {} was retained: {error}",
                    crate::git::display_path(path),
                )),
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
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
    // be read is not rewritten, so no group another process added is lost.
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
        Err(error) => {
            return Err(format!(
                "the holder record beside {} could not be read to update target process groups: {error}",
                crate::git::display_path(path)
            ));
        }
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
/// record lock nothing is rewritten, since an unserialized rewrite can drop
/// a group another process added; the failure is returned to report.
fn rewrite_serialized(lock: &Held, path: &Path) -> Vec<String> {
    match with_record_lock(path, || rewrite_record(lock, path)) {
        Ok(Ok(())) => Vec::new(),
        Ok(Err(failure)) => vec![failure],
        Err(error) => vec![format!(
            "the record lock beside {} could not be taken, so its record was not updated with \
             target process groups: {error}",
            crate::git::display_path(path)
        )],
    }
}

/// Run `rewrite` while holding the OS lock of the record lock file beside
/// the gate lock at `lock`, waiting up to five seconds for another rewrite
/// to finish. A holder keeps it only for one read, merge and rename, and
/// the OS releases it if that process dies.
fn with_record_lock<T>(lock: &Path, rewrite: impl FnOnce() -> T) -> std::io::Result<T> {
    with_record_lock_before(
        lock,
        std::time::Instant::now() + codeflow_limits::GATE_LOCK_RETAKE_SECONDS.wait(),
        rewrite,
    )
}

fn with_record_lock_before<T>(
    lock: &Path,
    finish_by: std::time::Instant,
    rewrite: impl FnOnce() -> T,
) -> std::io::Result<T> {
    let expired = || {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "shared record bookkeeping deadline reached",
        )
    };
    if std::time::Instant::now() >= finish_by {
        return Err(expired());
    }
    let mut record_lock = record_path(lock).into_os_string();
    record_lock.push(".lock");
    let record_lock = PathBuf::from(record_lock);
    let (anchor, relative) = lock_anchor(&record_lock)?;
    let guard = crate::contained::Tree::open(anchor)?.create_read_only(&relative)?;
    loop {
        if std::time::Instant::now() >= finish_by {
            return Err(expired());
        }
        match os_lock::try_lock(&guard) {
            Ok(()) => break,
            Err(error) if os_lock::is_contended(&error) => {
                std::thread::sleep(
                    codeflow_limits::POLL_GATE_RECORD_MILLIS
                        .wait()
                        .min(finish_by.saturating_duration_since(std::time::Instant::now())),
                );
            }
            Err(error) => return Err(error),
        }
    }
    if std::time::Instant::now() >= finish_by {
        os_lock::unlock(&guard);
        return Err(expired());
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

/// The record a run writes beside its slot and fence. A project path that
/// holds a control character, such as a line break, is left out: written
/// raw it would split the record into lines that every reader, this
/// binary's included, refuses as unreadable, and the slot could not be
/// cleared or taken again. Readers show a missing folder as unknown.
fn holder_record(project_dir: &Path, mode: &str) -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let dir = project_dir.display().to_string();
    let dir = if dir.is_empty() || dir.chars().any(char::is_control) {
        String::new()
    } else {
        format!("dir={dir}\n")
    };
    format!(
        "pid={}\nstarted={started}\n{dir}mode={mode}\n",
        std::process::id()
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
                std::thread::sleep(codeflow_limits::POLL_PROCESS_MILLIS.wait());
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
        assert!(first.paths.iter().all(|path| path.ends_with(slot_file(1))));

        let second = acquire_full_gate_lock(&dirs(tmp.path()), &work).unwrap();
        assert_eq!(second.runs, 2);
        assert!(second.paths.iter().all(|path| path.ends_with(slot_file(2))));
        for dir in dirs(tmp.path()) {
            let record = std::fs::read_to_string(dir.join("full-gate-2.record")).unwrap();
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
                    .filter(|lock| lock.paths.iter().all(|path| path.ends_with(slot_file(1))))
                    .or_else(|| {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        None
                    })
            })
            .expect("slot 1 is free once its holder drops it");
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
                let file = open_lock_file(&dir.join(slot_file(1))).unwrap();
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
        let sibling = dirs[0].join(slot_file(2));
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
        let held: Vec<File> = (1..=64)
            .map(|index| {
                let file = open_lock_file(&dir.join(slot_file(index))).unwrap();
                os_lock::try_lock(&file).unwrap();
                file
            })
            .collect();
        let lock = acquire_full_gate_lock(&dirs, &work).expect("a 65th run is admitted");
        assert_eq!(lock.runs, 65);
        assert!(lock.paths[0].ends_with(slot_file(65)));
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
            .map(|dir| open_lock_file(&dir.join(slot_file(1))).unwrap())
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
        assert!(beside.paths.iter().all(|path| path.ends_with(slot_file(2))));
        drop(beside);
        drop(inherited);
        let fresh = open_lock_file(&dirs[0].join(slot_file(1))).unwrap();
        assert!(
            os_lock::try_lock(&fresh).is_err(),
            "the inherited lock never releases the holder's slot"
        );
        let record = std::fs::read_to_string(record_path(&dirs[0].join(slot_file(1)))).unwrap();
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
            fences: Vec::new(),
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
                    fences: Vec::new(),
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
        assert_eq!(
            parse_descriptors("3,4,5,6,7,8"),
            Some(vec![3, 4, 5, 6, 7, 8])
        );
        for bad in ["", "2", "7,7", "x", "7,", "-1", "3,4,5,6,7,8,9"] {
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
            dir.join(slot_file(1)),
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
        let record = std::fs::read_to_string(record_path(&dir.join(slot_file(1)))).unwrap();
        assert!(record.contains(&format!("pid={}", std::process::id())));
    }

    /// A project path with a line break must not leave a record that the
    /// next run, which reads every record and fails closed, cannot parse:
    /// the run's own release and the next admission both read it.
    #[test]
    fn a_project_path_with_a_line_break_leaves_a_record_the_next_run_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let hostile = Path::new("/w/ev\ncodeflow test: passed");
        let record = holder_record(hostile, "full");
        assert!(parse_record(&record, &[]).is_ok(), "{record:?}");
        let first = acquire_full_gate_lock(std::slice::from_ref(&dir), hostile).unwrap();
        drop(first);
        let next = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"));
        assert!(next.is_ok(), "{:?}", next.err());
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
        // An older binary's gate ran alone, so its surviving target does
        // too: a new run refuses at once and starts nothing (TSK-1256).
        let refused = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a run is refused while an older gate's target survives");
        assert!(refused.older_binary);
        assert_eq!(refused.running_groups, vec![group]);
        assert_eq!(refused.path, dir.join(LOCK_FILE));
        let message = refused.to_string();
        assert!(
            message.contains(&format!("process groups {group}")),
            "{message}"
        );
        assert!(message.contains("last recorded holder"), "{message}");
        assert!(!dir.join(slot_file(1)).exists(), "no slot is taken");
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
        assert!(refused.contains("older CodeFlow binary"), "{refused}");

        target.kill().unwrap();
        target.wait().unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert_eq!(lock.runs, 1, "the ended target no longer counts");
        assert!(lock.paths[0].ends_with(slot_file(1)));
    }

    #[test]
    fn a_held_lock_records_running_target_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        // A group number no other test uses: records are shared by every
        // lock this process holds.
        target_group_started(4_000_001).unwrap();
        let slot = record_path(&dir.join(slot_file(1)));
        let record = std::fs::read_to_string(&slot).unwrap();
        assert!(record.contains("group=4000001"), "{record}");
        assert!(record.contains(&format!("pid={}", std::process::id())));
        target_group_finished(4_000_001).unwrap();
        let record = std::fs::read_to_string(&slot).unwrap();
        assert!(!record.contains("group=4000001"), "{record}");
        drop(lock);
        assert_eq!(std::fs::read_to_string(&slot).unwrap(), "");
        // The fence record names the last admitted run, which has ended.
        assert!(std::fs::read_to_string(dir.join(RECORD_FILE))
            .unwrap()
            .contains(&format!("pid={}", std::process::id())));
        // The lock file itself is never written (TSK-1003).
        assert_eq!(std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap(), "");
    }

    #[cfg(unix)]
    #[test]
    fn target_cleanup_shares_one_deadline_across_several_held_records() {
        use std::time::{Duration, Instant};
        let tmp = tempfile::tempdir().unwrap();
        let dirs = (0..3)
            .map(|n| tmp.path().join(format!("locks-{n}")))
            .collect::<Vec<_>>();
        let lock = acquire_full_gate_lock(&dirs, Path::new("/w")).unwrap();
        let group = 4_000_013;
        target_group_started(group).unwrap();
        let blockers = lock
            .paths
            .iter()
            .map(|path| {
                let mut name = record_path(path).into_os_string();
                name.push(".lock");
                let file = File::open(PathBuf::from(name)).unwrap();
                os_lock::try_lock(&file).unwrap();
                file
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        let finish_by = started + Duration::from_millis(250);
        let result = crate::testing::runner::finish_cleanup_before(finish_by, move |until| {
            target_group_finished_before(group, until)
        });
        let elapsed = started.elapsed();
        // Release every fixture lock before assertions or GateLock::drop.
        for blocker in blockers {
            os_lock::unlock(&blocker);
        }
        let records = lock
            .paths
            .iter()
            .map(|path| std::fs::read_to_string(record_path(path)).unwrap())
            .collect::<Vec<_>>();
        drop(lock);
        assert!(
            result.is_err(),
            "contended bookkeeping must not be called complete"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "one budget, not five seconds per record: {elapsed:?}"
        );
        assert!(
            records
                .iter()
                .all(|record| record.contains("group=4000013")),
            "unwritten records remain conservative"
        );
    }

    #[cfg(unix)]
    #[test]
    fn target_cleanup_does_not_wait_out_the_held_record_mutex() {
        use std::time::{Duration, Instant};
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _held = held();
            ready_tx.send(()).unwrap();
            // A finite release also lets the old implementation finish and
            // fail the regression rather than deadlocking the test process.
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        });
        ready_rx.recv().unwrap();
        let started = Instant::now();
        let result = crate::testing::runner::finish_cleanup_before(
            started + Duration::from_millis(250),
            move |until| target_group_finished_before(4_000_015, until),
        );
        let elapsed = started.elapsed();
        let _ = release_tx.send(());
        holder.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(1),
            "registry mutex is inside the cleanup budget: {elapsed:?}"
        );
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

    /// A repository whose config the probe is pointed at, and its git folder.
    #[cfg(unix)]
    fn config_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let git_dir = tmp.path().join(".git");
        (tmp, git_dir)
    }

    /// The probe, run on its own thread so a regression that blocks on a
    /// special file fails the test instead of hanging the run.
    #[cfg(unix)]
    fn probe_path_with_deadline(
        project: &std::path::Path,
    ) -> (std::path::PathBuf, std::io::Result<()>) {
        let project = project.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(probe_git_config_write(&project));
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the git config probe blocked")
    }

    #[cfg(unix)]
    fn probe_with_deadline(project: &std::path::Path) -> std::io::Result<()> {
        probe_path_with_deadline(project).1
    }

    /// A config that is a link is never followed: the probe refuses it, the
    /// file it points at is not opened for writing, and no lock is left.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_link_and_leaves_no_lock() {
        let (tmp, git_dir) = config_fixture();
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        std::fs::remove_file(git_dir.join("config")).unwrap();
        std::os::unix::fs::symlink(&outside, git_dir.join("config")).unwrap();
        assert!(
            probe_with_deadline(tmp.path()).is_err(),
            "a link was followed"
        );
        assert!(!git_dir.join("config.lock").exists());
        assert!(git_dir.join("config").is_symlink());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    }

    /// A missing config is reported, and the lock the probe took is removed.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_reports_a_missing_file_and_removes_its_lock() {
        let (tmp, git_dir) = config_fixture();
        std::fs::remove_file(git_dir.join("config")).unwrap();
        let error = probe_with_deadline(tmp.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
        assert!(!git_dir.join("config.lock").exists());
        assert!(
            !git_dir.join("config").exists(),
            "the probe created the file"
        );
    }

    /// The system refuses the append open after the lock was created: the
    /// error is returned and the lock is removed. (As the superuser the
    /// system allows the open, so there is nothing to refuse.)
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_removes_its_lock_when_the_append_open_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (tmp, git_dir) = config_fixture();
        let config = git_dir.join("config");
        let before = std::fs::read(&config).unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o400)).unwrap();
        let error = probe_with_deadline(tmp.path()).unwrap_err();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "{error}"
        );
        assert!(!git_dir.join("config.lock").exists());
        assert_eq!(std::fs::read(&config).unwrap(), before);
    }

    /// From a linked worktree the probe reaches the shared config of the main
    /// repository, through the worktree folder's place under `worktrees`.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_finds_the_shared_config_from_a_linked_worktree() {
        let (tmp, git_dir) = config_fixture();
        let repo = git2::Repository::open(tmp.path()).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let linked = tmp.path().join("linked");
        repo.worktree("linked", &linked, None).unwrap();
        let (path, result) = probe_git_config_write(&linked);
        result.unwrap();
        assert_eq!(
            path.canonicalize().unwrap(),
            git_dir.join("config").canonicalize().unwrap()
        );
    }

    /// Every path under `root`, for proving a refused probe created nothing.
    #[cfg(unix)]
    fn listing(root: &std::path::Path) -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                out.push(path.display().to_string());
                if path.is_dir() && !path.is_symlink() {
                    walk(&path, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, &mut out);
        out.sort();
        out
    }

    /// A repository with one commit and a linked worktree `linked` beside it.
    #[cfg(unix)]
    fn linked_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let (tmp, _git_dir) = config_fixture();
        let repo = git2::Repository::open(tmp.path()).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let linked = tmp.path().join("linked");
        repo.worktree("linked", &linked, None).unwrap();
        (tmp, linked)
    }

    /// A git folder stored under a folder named `worktrees` is not a linked
    /// worktree: the probe measures that folder's own config and creates
    /// nothing beside it, for a bare folder and for a checkout pointing at it.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_does_not_take_a_folder_name_for_a_registration() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("x");
        let separate = outer.join("worktrees").join("repo.git");
        std::fs::create_dir_all(&separate).unwrap();
        git2::Repository::init_bare(&separate).unwrap();
        let checkout = tmp.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(
            checkout.join(".git"),
            format!("gitdir: {}\n", separate.display()),
        )
        .unwrap();
        let before = listing(tmp.path());
        let (path, result) = probe_git_config_write(&checkout);
        result.unwrap();
        assert_eq!(
            path.canonicalize().unwrap(),
            separate.join("config").canonicalize().unwrap()
        );
        assert_eq!(listing(tmp.path()), before, "the probe left a file behind");
        assert!(!outer.join("config.lock").exists());
        let (path, result) = probe_git_config_write(&separate);
        result.unwrap();
        assert_eq!(
            path.canonicalize().unwrap(),
            separate.join("config").canonicalize().unwrap()
        );
        assert_eq!(listing(tmp.path()), before);
    }

    /// A checkout marker steered at an administrative folder under an
    /// unrelated `worktrees` folder, with a `commondir` naming a real
    /// repository, is refused before any lock is created.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_forged_checkout_marker() {
        let (real, _linked) = linked_fixture();
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("x").join("worktrees").join("fake");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            fake.join("commondir"),
            format!("{}\n", real.path().join(".git").display()),
        )
        .unwrap();
        std::fs::write(
            fake.join("gitdir"),
            format!("{}\n", real.path().join("linked").join(".git").display()),
        )
        .unwrap();
        let checkout = tmp.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(
            checkout.join(".git"),
            format!("gitdir: {}\n", fake.display()),
        )
        .unwrap();
        let before = listing(tmp.path());
        let real_before = listing(real.path());
        let (_, result) = probe_git_config_write(&checkout);
        assert!(result.is_err(), "a forged marker was accepted");
        assert_eq!(listing(tmp.path()), before);
        assert_eq!(listing(real.path()), real_before);
        assert!(!tmp.path().join("x").join("config.lock").exists());
    }

    /// A registration whose `commondir` or `gitdir` backlink disagrees with
    /// the checkout is refused, and nothing is created.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_registration_that_disagrees() {
        let other = tempfile::tempdir().unwrap();
        git2::Repository::init(other.path()).unwrap();
        for forged in ["commondir", "gitdir"] {
            let (tmp, linked) = linked_fixture();
            let admin = tmp.path().join(".git").join("worktrees").join("linked");
            let wrong = other.path().join(".git");
            std::fs::write(admin.join(forged), format!("{}\n", wrong.display())).unwrap();
            let before = listing(tmp.path());
            let other_before = listing(other.path());
            let (_, result) = probe_git_config_write(&linked);
            assert!(result.is_err(), "{forged} mismatch was accepted");
            assert_eq!(listing(tmp.path()), before, "{forged}");
            assert_eq!(listing(other.path()), other_before, "{forged}");
        }
    }

    /// A common directory reached through an alias of the registrations
    /// folder is not the physical parent of the registration: refused with
    /// the disagreement named, and no lock is left in either place.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_common_directory_reached_through_an_alias() {
        let (real, linked) = linked_fixture();
        let alias_root = tempfile::tempdir().unwrap();
        let common = alias_root.path().join("b");
        std::fs::create_dir_all(common.join("objects")).unwrap();
        std::fs::create_dir_all(common.join("refs")).unwrap();
        std::os::unix::fs::symlink(
            real.path().join(".git").join("worktrees"),
            common.join("worktrees"),
        )
        .unwrap();
        let admin = real.path().join(".git").join("worktrees").join("linked");
        std::fs::write(admin.join("commondir"), format!("{}\n", common.display())).unwrap();
        let before = listing(alias_root.path());
        let real_before = listing(real.path());
        let (_, result) = probe_git_config_write(&linked);
        let error = result.expect_err("an alias was accepted as the common directory");
        assert!(
            error
                .to_string()
                .contains("disagrees with its registration"),
            "{error}"
        );
        assert_eq!(listing(alias_root.path()), before);
        assert_eq!(listing(real.path()), real_before);
    }

    /// The probe works through the directory validation opened: a path
    /// swapped for a link after validation moves nothing.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_keeps_the_directory_it_validated() {
        let (tmp, git_dir) = config_fixture();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("config"), "other").unwrap();
        let tree = crate::hooks::repo::shared_git_tree(tmp.path()).unwrap();
        // Between validation and the probe the git folder is renamed away
        // and its old name becomes a link to an unrelated folder.
        let moved = tmp.path().join("moved-git");
        std::fs::rename(&git_dir, &moved).unwrap();
        std::os::unix::fs::symlink(other.path(), &git_dir).unwrap();
        // A fresh resolution of the validated path lands in the other folder:
        // the race is real for any caller that re-resolves it.
        assert!(crate::contained::Tree::open(tree.path())
            .unwrap()
            .metadata("config")
            .is_ok_and(|m| m.len() == 5));
        let before = listing(other.path());
        probe_config_in(&tree).unwrap();
        assert_eq!(
            listing(other.path()),
            before,
            "the probe left the validated folder"
        );
        assert_eq!(
            std::fs::read_to_string(other.path().join("config")).unwrap(),
            "other"
        );
        assert!(!moved.join("config.lock").exists());
        assert!(moved.join("config").is_file());
    }

    /// Replace the linked fixture's common directory with a look-alike that
    /// has its own `worktrees/linked` registration for the same checkout.
    #[cfg(unix)]
    fn swap_common_directory(tmp: &std::path::Path) {
        let common = tmp.join(".git");
        let aside = tmp.join("git-original");
        std::fs::rename(&common, &aside).unwrap();
        let admin = common.join("worktrees").join("linked");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir(common.join("objects")).unwrap();
        std::fs::create_dir(common.join("refs")).unwrap();
        std::fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::copy(
            aside.join("worktrees").join("linked").join("gitdir"),
            admin.join("gitdir"),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn interleave_at(stage: &'static str, action: impl Fn() + 'static) {
        crate::hooks::repo::INTERLEAVE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move |at: &str| {
                if at == stage {
                    action();
                }
            }));
        });
    }

    #[cfg(unix)]
    fn no_interleave() {
        crate::hooks::repo::INTERLEAVE.with(|slot| *slot.borrow_mut() = None);
    }

    /// The common directory is replaced after the administrative directory
    /// was opened and before the common directory is. The replacement's path
    /// equals the stored one and it lists its own registration, but that is
    /// not the folder held: refused, and nothing is created in it.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_common_directory_swapped_after_the_admin_opens() {
        let (tmp, linked) = linked_fixture();
        let root = tmp.path().to_path_buf();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = seen.clone();
        interleave_at("before-common", move || {
            swap_common_directory(&root);
            *record.borrow_mut() = listing(&root.join(".git"));
        });
        let (_, result) = probe_git_config_write(&linked);
        no_interleave();
        let error = result.expect_err("a swapped common directory was accepted");
        assert!(
            error.to_string().contains("different registration"),
            "{error}"
        );
        assert!(!seen.borrow().is_empty(), "the swap did not run");
        assert_eq!(listing(&tmp.path().join(".git")), *seen.borrow());
        assert!(!tmp.path().join(".git").join("config.lock").exists());
    }

    /// The same swap after the common directory was validated and before the
    /// checkout marker's target is opened: that target is the replacement's
    /// folder, not the held one.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_checkout_target_swapped_late() {
        let (tmp, linked) = linked_fixture();
        let root = tmp.path().to_path_buf();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = seen.clone();
        interleave_at("before-checkout-target", move || {
            swap_common_directory(&root);
            *record.borrow_mut() = listing(&root.join(".git"));
        });
        let (_, result) = probe_git_config_write(&linked);
        no_interleave();
        let error = result.expect_err("a swapped checkout target was accepted");
        assert!(
            error.to_string().contains("different git directory"),
            "{error}"
        );
        assert!(!seen.borrow().is_empty(), "the swap did not run");
        assert_eq!(listing(&tmp.path().join(".git")), *seen.borrow());
        assert!(!tmp.path().join(".git").join("config.lock").exists());
    }

    /// The discovered folder is replaced by one that is no repository, before
    /// the first handle is taken: a linked worktree's administrative folder
    /// and an ordinary git folder are both refused, with nothing created.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_folder_swapped_before_the_first_open() {
        let (tmp, linked) = linked_fixture();
        let admin = tmp.path().join(".git").join("worktrees").join("linked");
        let root = tmp.path().to_path_buf();
        let at = admin.clone();
        interleave_at("before-admin", move || {
            std::fs::rename(&at, root.join("admin-original")).unwrap();
            std::fs::create_dir(&at).unwrap();
            // A config file, so only the repository check can refuse it.
            std::fs::write(at.join("config"), "").unwrap();
        });
        let (_, result) = probe_git_config_write(&linked);
        no_interleave();
        assert!(
            result.is_err(),
            "a swapped administrative folder was accepted"
        );
        assert_eq!(listing(&admin).len(), 1, "something was created in it");

        let (tmp, git_dir) = config_fixture();
        let root = tmp.path().to_path_buf();
        let at = git_dir.clone();
        interleave_at("before-admin", move || {
            std::fs::rename(&at, root.join("git-original")).unwrap();
            std::fs::create_dir(&at).unwrap();
            // A config file, so only the repository check can refuse it.
            std::fs::write(at.join("config"), "").unwrap();
        });
        let (_, result) = probe_git_config_write(tmp.path());
        no_interleave();
        assert!(result.is_err(), "a swapped git folder was accepted");
        assert_eq!(listing(&git_dir).len(), 1, "something was created in it");
    }

    /// A registration file (`commondir`, `gitdir`) or the checkout's `.git`
    /// file that is a link, and a registration file that is a pipe, is
    /// refused without blocking and leaves no file. A pipe standing where
    /// the checkout's `.git` should be is a different case: git discovery
    /// steps over it and finds the enclosing repository, so the probe
    /// succeeds on that repository's own config (asserted below).
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_registration_pipe_or_any_marker_link() {
        for kind in ["pipe", "link"] {
            // The two registration files and the checkout's own `.git` marker.
            for marker in ["commondir", "gitdir", "checkout .git"] {
                let (tmp, linked) = linked_fixture();
                let admin = tmp.path().join(".git").join("worktrees").join("linked");
                let file = if marker == "checkout .git" {
                    linked.join(".git")
                } else {
                    admin.join(marker)
                };
                let original = std::fs::read(&file).unwrap();
                std::fs::remove_file(&file).unwrap();
                if kind == "pipe" {
                    let path = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
                    // SAFETY: a valid NUL-terminated path.
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                } else {
                    let target = tmp.path().join("real-marker");
                    std::fs::write(&target, original).unwrap();
                    std::os::unix::fs::symlink(&target, &file).unwrap();
                }
                let before = listing(tmp.path());
                let (path, result) = probe_path_with_deadline(&linked);
                if kind == "pipe" && marker == "checkout .git" {
                    // The enclosing repository is measured, nothing else.
                    result.unwrap();
                    assert_eq!(
                        path.canonicalize().unwrap(),
                        tmp.path()
                            .join(".git")
                            .join("config")
                            .canonicalize()
                            .unwrap()
                    );
                } else {
                    assert!(result.is_err(), "{kind} {marker} was accepted");
                }
                assert_eq!(listing(tmp.path()), before, "{kind} {marker}");
            }
        }
    }

    /// A special file or a folder in the config's place is refused without
    /// blocking, and the lock is removed.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_refuses_a_special_file_or_folder_without_blocking() {
        let (tmp, git_dir) = config_fixture();
        let config = git_dir.join("config");
        std::fs::remove_file(&config).unwrap();
        let path = std::ffi::CString::new(config.to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(
            probe_with_deadline(tmp.path()).is_err(),
            "a pipe was accepted"
        );
        assert!(!git_dir.join("config.lock").exists());
        std::fs::remove_file(&config).unwrap();
        std::fs::create_dir(&config).unwrap();
        assert!(
            probe_with_deadline(tmp.path()).is_err(),
            "a folder was accepted"
        );
        assert!(!git_dir.join("config.lock").exists());
    }

    /// TSK-1156: the config probe changes nothing and leaves no lock, and a
    /// held lock (how git sees a config file it cannot write) is reported
    /// and left alone.
    #[test]
    fn the_git_config_probe_changes_nothing_and_reports_a_held_lock() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let git_dir = tmp.path().join(".git");
        let config = git_dir.join("config");
        let lock = git_dir.join("config.lock");
        let before = std::fs::read(&config).unwrap();
        let (path, result) = probe_git_config_write(tmp.path());
        assert!(path.ends_with(".git/config"), "{path:?}");
        result.unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), before);
        assert!(!lock.exists());
        std::fs::write(&lock, "held").unwrap();
        let (_, result) = probe_git_config_write(tmp.path());
        assert!(result.is_err());
        assert_eq!(std::fs::read(&config).unwrap(), before);
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "held");
        let plain = tempfile::tempdir().unwrap();
        assert!(probe_git_config_write(plain.path()).1.is_err());
    }

    /// The config file controls remotes and hooks, so measuring access to it
    /// must leave the very same file in place: the same inode, which carries
    /// its owner, mode, ACL and security attributes, and the same bytes and
    /// modification time, after a probe that succeeds and after one that is
    /// refused (a held lock). A replaced file would be a new inode that
    /// could have lost any attribute the replacement does not copy.
    #[cfg(unix)]
    #[test]
    fn the_git_config_probe_leaves_the_same_file_with_its_access_restrictions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let config = tmp.path().join(".git").join("config");
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
        let identity = || {
            let meta = std::fs::metadata(&config).unwrap();
            (
                meta.ino(),
                meta.dev(),
                meta.mode(),
                meta.uid(),
                meta.gid(),
                meta.mtime(),
                meta.mtime_nsec(),
                std::fs::read(&config).unwrap(),
            )
        };
        let before = identity();
        probe_git_config_write(tmp.path()).1.unwrap();
        assert_eq!(identity(), before, "a successful probe changed the file");
        std::fs::write(tmp.path().join(".git").join("config.lock"), "held").unwrap();
        assert!(probe_git_config_write(tmp.path()).1.is_err());
        assert_eq!(identity(), before, "a refused probe changed the file");
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
    /// still read, so its killed gate's live targets keep a new run out
    /// (TSK-1256: they run alone, as that gate did).
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
        let refused =
            acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap_err();
        assert!(refused.older_binary);
        assert_eq!(
            refused.running_groups,
            vec![group],
            "the legacy record's live group is read"
        );
        target.kill().unwrap();
        target.wait().unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert!(lock.paths[0].ends_with(slot_file(1)));
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
        std::fs::write(dir.join(slot_file(1)), "").unwrap();
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

/// TSK-1256: older binaries (3.1.x, and line builds before this change)
/// lock `full-gate.lock` exclusively; every new run holds it shared.
#[cfg(test)]
mod old_binary_fence_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A temp root, a trusted work folder in it, and two lock folders.
    fn setup() -> (tempfile::TempDir, PathBuf, Vec<PathBuf>) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        let dirs = vec![root.join("home-locks"), root.join("repo-locks")];
        (tmp, work, dirs)
    }

    /// An older binary's take, as 3.1.0 and the pre-TSK-1256 line builds
    /// make it: `full-gate.lock` opened and locked exclusively, no wait.
    fn old_take(dir: &Path) -> std::io::Result<AcquiredFile> {
        let file = open_lock_file(&dir.join(LOCK_FILE))?;
        os_lock::try_lock(&file)?;
        Ok(AcquiredFile(file))
    }

    /// [`old_take`], retried briefly: a sibling test that forks a child can
    /// hold a duplicate of a released descriptor until its exec.
    fn old_take_eventually(dir: &Path) -> AcquiredFile {
        for _ in 0..40 {
            if let Ok(file) = old_take(dir) {
                return file;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        old_take(dir).expect("the old binary's take succeeds once no new run is active")
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// AC-1: while new runs hold only slots 1 and up, an older binary's
    /// exclusive take of `full-gate.lock` is refused.
    #[test]
    fn an_old_take_is_refused_while_new_runs_hold_only_later_slots() {
        let (_tmp, work, dirs) = setup();
        let first = acquire_full_gate_lock(&dirs, &work).unwrap();
        let second = acquire_full_gate_lock(&dirs, &work).unwrap();
        drop(first);
        for dir in &dirs {
            match old_take(dir) {
                Err(error) if os_lock::is_contended(&error) => {}
                other => panic!(
                    "an old binary's take of {} must be refused while a new run is active: {:?}",
                    dir.join(LOCK_FILE).display(),
                    other.map(drop)
                ),
            }
        }
        drop(second);
        for dir in &dirs {
            drop(old_take_eventually(dir));
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn holder_liveness_uses_exact_lf_separators() {
        let pid = std::process::id();
        assert!(holder_alive(&format!("pid={pid}\n")));
        assert!(holder_alive(&format!("mode=full\npid={pid}")));
        for suffix in ["\r\n", "\u{0085}", "\u{2028}", "\u{00a0}"] {
            assert!(!holder_alive(&format!("pid={pid}{suffix}")));
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_exited_windows_holder_is_not_live_with_its_handle_open() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit", "/B", "0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        // Retain Child's process handle so this also covers a terminated
        // process object that OpenProcess can still open.
        assert!(!holder_alive(&format!("pid={pid}\n")));
        drop(child);
    }

    /// AC-2: while a 3.1 gate holds `full-gate.lock` exclusively, a new run
    /// refuses at once, takes no slot, and names the holder from the record
    /// the 3.1 gate keeps inside the lock file.
    #[test]
    fn a_new_run_refuses_at_once_naming_a_3_1_holder() {
        let (_tmp, work, dirs) = setup();
        let pid = std::process::id();
        std::fs::create_dir_all(&dirs[0]).unwrap();
        // A crashed new run's record names a dead process; the live 3.1
        // holder's record is the one inside the lock file.
        std::fs::write(
            dirs[0].join(RECORD_FILE),
            "pid=999999\nstarted=1\ndir=/gone\nmode=full\n",
        )
        .unwrap();
        std::fs::write(
            dirs[0].join(LOCK_FILE),
            format!("pid={pid}\nstarted={}\ndir={}\n", now(), work.display()),
        )
        .unwrap();
        let old = old_take(&dirs[0]).unwrap();
        let start = Instant::now();
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(2), "no wait");
        assert_eq!(refused.path, dirs[0].join(LOCK_FILE));
        let message = refused.to_string();
        let expected_dir = crate::git::display_line(&trimmed_dir(&work.display().to_string()));
        assert!(
            message.contains(&format!("pid {pid} in {expected_dir}")),
            "{message}"
        );
        assert!(!message.contains("999999"), "{message}");
        assert!(message.contains("older CodeFlow binary"), "{message}");
        for dir in &dirs {
            assert!(!dir.join(slot_file(1)).exists(), "no slot is taken");
            assert!(!dir.join(HOST_LOCK_FILE).exists(), "nothing starts");
        }
        drop(old);
        let run = acquire_full_gate_lock(&dirs, &work).expect("admitted once the old gate ends");
        drop(run);
    }

    /// AC-2: a line build from before this change keeps its record in
    /// `full-gate.record`; holding only the repository folder's lock, it
    /// still refuses a new run before any slot in either folder is taken.
    #[test]
    fn a_new_run_refuses_at_once_naming_a_line_build_holder() {
        let (_tmp, work, dirs) = setup();
        let pid = std::process::id();
        std::fs::create_dir_all(&dirs[1]).unwrap();
        std::fs::write(
            dirs[1].join(RECORD_FILE),
            format!(
                "pid={pid}\nstarted={}\ndir={}\nmode=essential\n",
                now(),
                work.display()
            ),
        )
        .unwrap();
        let old = old_take(&dirs[1]).unwrap();
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert_eq!(refused.path, dirs[1].join(LOCK_FILE));
        let message = refused.to_string();
        assert!(message.contains(&format!("pid {pid}")), "{message}");
        assert!(message.contains("essential mode"), "{message}");
        for dir in &dirs {
            assert!(!dir.join(slot_file(1)).exists(), "no slot is taken");
        }
        drop(old_take_eventually(&dirs[0]));
        drop(old);
    }

    /// AC-3: a fence that cannot be opened refuses, naming the file, and
    /// releases the fence already taken in another folder.
    #[test]
    fn a_fence_that_cannot_be_opened_fails_closed_naming_it() {
        let (_tmp, work, dirs) = setup();
        std::fs::create_dir_all(dirs[1].join(LOCK_FILE)).unwrap();
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert_eq!(refused.path, dirs[1].join(LOCK_FILE));
        let message = with_repair_pointer(&refused);
        assert!(message.contains("gate lock unavailable"), "{message}");
        assert!(message.contains(LOCK_FILE), "{message}");
        for dir in &dirs {
            assert!(!dir.join(slot_file(1)).exists(), "no slot is taken");
        }
        drop(old_take_eventually(&dirs[0]));
    }

    /// AC-3: a permission error on the fence fails closed the same way.
    #[cfg(unix)]
    #[test]
    fn a_fence_without_permission_fails_closed_naming_it() {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: geteuid only reads the process's user id.
        if unsafe { libc::geteuid() } == 0 {
            return; // root opens any file; the fixture cannot deny it
        }
        let (_tmp, work, dirs) = setup();
        std::fs::create_dir_all(&dirs[0]).unwrap();
        let fence = dirs[0].join(LOCK_FILE);
        std::fs::write(&fence, "").unwrap();
        std::fs::set_permissions(&fence, std::fs::Permissions::from_mode(0o000)).unwrap();
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        std::fs::set_permissions(&fence, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(refused.path, fence);
        let message = refused.to_string();
        assert!(message.contains("gate lock unavailable"), "{message}");
        assert!(message.contains(LOCK_FILE), "{message}");
        for dir in &dirs {
            assert!(!dir.join(slot_file(1)).exists(), "no slot is taken");
        }
    }

    /// AC-4: `full-gate.record` names the last admitted run, in the
    /// one-holder format older readers parse; it may have ended, and no
    /// release rewrites it. The lock file a 3.1 reader shows stays empty.
    #[test]
    fn old_readers_see_the_last_admitted_holder() {
        let (_tmp, root_work, dirs) = setup();
        let root = root_work.parent().unwrap().to_path_buf();
        let works: Vec<PathBuf> = ["work-a", "work-b", "work-c"]
            .iter()
            .map(|name| root.join(name))
            .collect();
        for work in &works {
            std::fs::create_dir(work).unwrap();
        }
        // What a line build before this change reads: the record file,
        // one holder, keys and modes its strict parser accepts.
        let line_reader = |dir: &Path| {
            let text = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
            parse_record(&text, &[]).expect("an old parser reads the record");
            assert!(
                text.lines().filter(|line| line.starts_with("pid=")).count() <= 1,
                "{text}"
            );
            assert!(
                text.lines()
                    .filter_map(|line| line.strip_prefix("mode="))
                    .all(|mode| ["full", "quick", "essential", "reserve"].contains(&mode)),
                "{text}"
            );
            text
        };
        // What 3.1 reads: the lock file itself, which new runs never write.
        let release_reader = |dir: &Path| std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
        let names = |text: &str, work: &Path| text.contains(&format!("dir={}\n", work.display()));

        let a = acquire_full_gate_lock(&dirs, &works[0]).unwrap();
        let b = acquire_full_gate_lock(&dirs, &works[1]).unwrap();
        let c = acquire_full_gate_lock(&dirs, &works[2]).unwrap();
        for dir in &dirs {
            let text = line_reader(dir);
            assert!(names(&text, &works[2]), "{text}");
            assert_eq!(
                release_reader(dir),
                "",
                "3.1 shows no holder, never a false one"
            );
        }
        // A, the first admitted, ends: the record still names C.
        drop(a);
        for dir in &dirs {
            let text = line_reader(dir);
            assert!(
                names(&text, &works[2]),
                "the last admitted run is named: {text}"
            );
        }
        drop(c);
        drop(b);
        for dir in &dirs {
            let text = line_reader(dir);
            assert!(
                names(&text, &works[2]),
                "no release rewrites it, so it names the last admitted run: {text}"
            );
            assert_eq!(release_reader(dir), "");
        }
    }

    /// R3: a fence record lock that cannot be taken refuses the admission,
    /// so no run is active under a record that names another; the fence
    /// record is not written without it.
    #[test]
    fn a_fence_record_lock_that_cannot_be_taken_refuses_the_run() {
        let (_tmp, work, dirs) = setup();
        std::fs::create_dir_all(dirs[0].join("full-gate.record.lock")).unwrap();
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert_eq!(refused.path, dirs[0].join(LOCK_FILE));
        assert!(
            refused
                .to_string()
                .contains("holder record cannot be written"),
            "{refused}"
        );
        assert!(
            !dirs[0].join(RECORD_FILE).exists(),
            "nothing is written unlocked"
        );
        // Every lock it took is released.
        for dir in &dirs {
            drop(old_take_eventually(dir));
        }
    }

    /// R3: without a slot's record lock, a target update and the release
    /// leave the record as it is and report it, never rewriting unlocked.
    #[cfg(unix)]
    #[test]
    fn a_slot_record_lock_that_cannot_be_taken_is_reported_never_bypassed() {
        let (_tmp, work, dirs) = setup();
        let lock = acquire_full_gate_lock(&dirs[..1], &work).unwrap();
        // Out of the registry, so a sibling test's target update never
        // meets the planted record lock; the release still clears its slot.
        held().retain(|entry| entry.id != lock.id);
        let path = lock.paths[0].clone();
        let record = record_path(&path);
        let before = std::fs::read_to_string(&record).unwrap();
        let mut guard = record.clone().into_os_string();
        guard.push(".lock");
        let guard = PathBuf::from(guard);
        if guard.exists() {
            std::fs::remove_file(&guard).unwrap();
        }
        std::fs::create_dir(&guard).unwrap();
        let own = Held {
            id: u64::MAX - 2,
            paths: vec![path.clone()],
            record: before.clone(),
            groups: BTreeSet::from([4_000_003]),
            recorded: BTreeSet::from([4_000_003]),
            inherited: false,
            host: Vec::new(),
            trusted: trusted_dirs(&work),
            runs: 1,
            descriptors: Vec::new(),
            fences: Vec::new(),
        };
        let failures = rewrite_serialized(&own, &path);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("record lock"), "{failures:?}");
        assert!(failures[0].contains("was not updated"), "{failures:?}");
        assert_eq!(std::fs::read_to_string(&record).unwrap(), before);
        drop(lock);
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            before,
            "the release does not clear the record without its record lock"
        );
    }

    /// R4: a host-exclusive hold outside any admission still holds the
    /// fence, so an older binary's gate is refused while it lasts.
    #[test]
    fn a_standalone_host_exclusive_hold_keeps_older_binaries_out() {
        let (_tmp, work, dirs) = setup();
        let hold = hold_host_exclusive_in(&dirs, &work, Duration::ZERO).expect("an idle host");
        for dir in &dirs {
            match old_take(dir) {
                Err(error) if os_lock::is_contended(&error) => {}
                other => panic!("an old take must be refused: {:?}", other.map(drop)),
            }
        }
        drop(hold);
        for dir in &dirs {
            drop(old_take_eventually(dir));
        }
    }

    /// R4: while an older binary's gate holds the fence, a standalone
    /// host-exclusive hold refuses at once and takes no host lock.
    #[test]
    fn a_standalone_host_exclusive_hold_refuses_beside_an_older_gate() {
        let (_tmp, work, dirs) = setup();
        std::fs::create_dir_all(&dirs[1]).unwrap();
        let old = old_take(&dirs[1]).unwrap();
        let start = Instant::now();
        let refused = hold_host_exclusive_in(&dirs, &work, Duration::from_secs(5)).unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(2), "no wait");
        assert!(refused.contains("older CodeFlow binary"), "{refused}");
        for dir in &dirs {
            assert!(!dir.join(HOST_LOCK_FILE).exists(), "no host lock is taken");
        }
        drop(old);
        drop(old_take_eventually(&dirs[0]));
    }

    /// R2, an upgrade restriction: a build between the landed TSK-1145
    /// change and this one takes the host shared, finds `full-gate.lock`
    /// busy, and falls back to a later slot, so it is admitted beside a new
    /// run and is not fenced out. Such builds are drained or rebuilt before
    /// a new build runs on the same machine.
    #[test]
    fn a_c1481_era_build_falls_back_to_a_slot_beside_a_new_run() {
        let (_tmp, work, dirs) = setup();
        let new = acquire_full_gate_lock(&dirs[..1], &work).unwrap();
        let dir = &dirs[0];
        // The c1481a51a admission, step by step.
        let host = open_lock_file(&dir.join(HOST_LOCK_FILE)).unwrap();
        os_lock::try_lock_shared(&host).expect("the host is shared");
        let zero = open_lock_file(&dir.join(LOCK_FILE)).unwrap();
        let error = os_lock::try_lock(&zero).unwrap_err();
        assert!(os_lock::is_contended(&error), "slot 0 is busy: {error}");
        let one = open_lock_file(&dir.join(slot_file(1))).unwrap();
        assert!(os_lock::try_lock(&one).is_err(), "slot 1 is the new run's");
        let two = open_lock_file(&dir.join(slot_file(2))).unwrap();
        os_lock::try_lock(&two).expect("the fallback slot admits it beside the new run");
        drop(AcquiredFile(two));
        drop(AcquiredFile(host));
        drop(new);
    }

    /// AC-9: a host that has the slot-0 and fence files the landed TSK-1145
    /// version left, with stale records, admits a run correctly; a landed
    /// run still on slot 1 counts, and one on slot 0 is an older binary.
    #[test]
    fn a_host_with_the_landed_slot_files_still_admits_correctly() {
        let (_tmp, work, dirs) = setup();
        for dir in &dirs {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(LOCK_FILE), "").unwrap();
            std::fs::write(
                dir.join(RECORD_FILE),
                "pid=999999\nstarted=1\ndir=/gone\nmode=full\n",
            )
            .unwrap();
            std::fs::write(dir.join("full-gate.record.lock"), "").unwrap();
            std::fs::write(dir.join(slot_file(1)), "").unwrap();
            std::fs::write(
                record_path(&dir.join(slot_file(1))),
                "pid=999998\nstarted=1\ndir=/gone\nmode=quick\n",
            )
            .unwrap();
            std::fs::write(dir.join(HOST_LOCK_FILE), "").unwrap();
        }
        let lock = acquire_full_gate_lock(&dirs, &work).expect("stale landed files never refuse");
        assert_eq!(lock.runs, 1, "stale records count no run");
        assert!(
            lock.paths.iter().all(|path| path.ends_with(slot_file(1))),
            "{:?}",
            lock.paths
        );
        assert!(
            lock.notes.iter().any(|note| note.contains("pid 999998")),
            "{:?}",
            lock.notes
        );
        for dir in &dirs {
            let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
            assert!(
                record.contains(&format!("pid={}\n", std::process::id())),
                "{record}"
            );
        }
        drop(lock);

        // A landed-version run still holding slot 1 counts as a run.
        let landed: Vec<File> = dirs
            .iter()
            .map(|dir| {
                let file = open_lock_file(&dir.join(slot_file(1))).unwrap();
                for _ in 0..40 {
                    if os_lock::try_lock(&file).is_ok() {
                        return file;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                panic!("slot 1 stays held");
            })
            .collect();
        let beside = acquire_full_gate_lock(&dirs, &work).unwrap();
        assert_eq!(beside.runs, 2);
        assert!(beside.paths.iter().all(|path| path.ends_with(slot_file(2))));
        drop(beside);
        drop(landed);

        // A landed-version run on slot 0 locks `full-gate.lock`
        // exclusively, as an older binary does, and is refused as one.
        let slot_zero = old_take_eventually(&dirs[0]);
        let refused = acquire_full_gate_lock(&dirs, &work).unwrap_err();
        assert_eq!(refused.path, dirs[0].join(LOCK_FILE));
        drop(slot_zero);
    }

    /// TSK-1256 R1-B: a released run's fence is only closed, never
    /// unlocked, so a target's inherited copy keeps older binaries out
    /// after the run ends; once the copy closes, the fence frees.
    #[cfg(unix)]
    #[test]
    fn a_released_runs_fence_stays_while_a_target_copy_lives() {
        let (_tmp, work, dirs) = setup();
        let lock = acquire_full_gate_lock(&dirs[..1], &work).unwrap();
        let copies = fence_copies().unwrap();
        let ours = std::fs::File::open(dirs[0].join(LOCK_FILE)).unwrap();
        let own_identity = FileIdentity::of(&ours).unwrap();
        // Keep only the copy of this test's fence; sibling tests' fences
        // close with their own copies.
        let copy: Vec<File> = copies
            .into_iter()
            .filter(|file| FileIdentity::of(file).ok().as_ref() == Some(&own_identity))
            .collect();
        assert_eq!(copy.len(), 1, "one copy of this run's fence");
        drop(lock);
        match old_take(&dirs[0]) {
            Err(error) if os_lock::is_contended(&error) => {}
            other => panic!(
                "the target's copy must keep the fence: {:?}",
                other.map(drop)
            ),
        }
        drop(copy);
        drop(old_take_eventually(&dirs[0]));
    }

    /// TSK-1256 R1-A: a completed new run leaves `full-gate.record`, and a
    /// 3.1 gate killed afterwards leaves its target's group only inside
    /// `full-gate.lock`. A reserve's admission and a standalone
    /// host-exclusive hold both refuse before taking a slot or the host.
    #[cfg(unix)]
    #[test]
    fn a_3_1_orphan_behind_a_completed_runs_record_is_refused() {
        use std::os::unix::process::CommandExt;
        let (_tmp, work, dirs) = setup();
        drop(acquire_full_gate_lock(&dirs, &work).unwrap());
        for dir in &dirs {
            assert!(std::fs::read_to_string(dir.join(RECORD_FILE))
                .unwrap()
                .contains("pid="));
        }
        let mut survivor = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = survivor.id();
        let old = old_take_eventually(&dirs[1]);
        std::fs::write(
            dirs[1].join(LOCK_FILE),
            format!("pid=999999\nstarted=1\ngroup={group}\n"),
        )
        .unwrap();
        drop(old);
        let slot_records: Vec<String> = dirs
            .iter()
            .map(|dir| std::fs::read_to_string(record_path(&dir.join(slot_file(1)))).unwrap())
            .collect();
        let refused = acquire_gate_lock(&dirs, &work, "reserve").unwrap_err();
        assert!(refused.older_binary);
        assert_eq!(refused.path, dirs[1].join(LOCK_FILE));
        assert_eq!(refused.running_groups, vec![group]);
        let refused = hold_host_exclusive_in(&dirs, &work, Duration::ZERO).unwrap_err();
        assert!(refused.contains("older CodeFlow binary"), "{refused}");
        assert!(
            refused.contains(&format!("process groups {group}")),
            "{refused}"
        );
        for (dir, before) in dirs.iter().zip(&slot_records) {
            assert_eq!(
                &std::fs::read_to_string(record_path(&dir.join(slot_file(1)))).unwrap(),
                before,
                "no slot was taken"
            );
            assert!(!dir.join(slot_file(2)).exists(), "no slot was taken");
            assert!(
                !std::fs::read_to_string(record_path(&dir.join(HOST_LOCK_FILE)))
                    .unwrap_or_default()
                    .contains("mode=exclusive"),
                "the host was not taken"
            );
        }
        survivor.kill().unwrap();
        survivor.wait().unwrap();
        drop(acquire_gate_lock(&dirs, &work, "reserve").expect("admitted once it ends"));
    }

    /// TSK-1256: a host-exclusive hold outside any admission passes its
    /// fence to the target it guards, as an admission does, and stops when
    /// the hold ends.
    #[cfg(unix)]
    #[test]
    fn a_host_exclusive_holds_fence_reaches_its_target() {
        let (_tmp, work, dirs) = setup();
        let ours = |copies: Vec<File>| {
            let identity = FileIdentity::of(&File::open(dirs[0].join(LOCK_FILE)).unwrap()).unwrap();
            copies
                .into_iter()
                .filter(|file| FileIdentity::of(file).ok().as_ref() == Some(&identity))
                .count()
        };
        let hold = hold_host_exclusive_in(&dirs[..1], &work, Duration::ZERO).unwrap();
        assert_eq!(ours(fence_copies().unwrap()), 1, "the target inherits it");
        drop(hold);
        assert_eq!(
            ours(fence_copies().unwrap()),
            0,
            "the ended hold passes none"
        );
        drop(old_take_eventually(&dirs[0]));
    }
}
