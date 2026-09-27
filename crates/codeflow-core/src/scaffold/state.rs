//! Consumer-project state: `.codeflow/project.toml` (user-owned,
//! schema-versioned) and `.codeflow/manifest.json` (the installed-file record
//! that makes `codeflow update` possible), plus the `.codeflow/.baseline/`
//! pristine-copy store used as the 3-way merge base.
//!
//! `project.toml` is user-owned: load/store round-trips through a
//! `toml::Table` so keys codeflow does not know about (e.g. a future
//! `orient.enabled = false`) survive every rewrite.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{Ownership, Tier};
use super::ScaffoldError;

pub const CODEFLOW_DIR: &str = ".codeflow";
pub const PROJECT_TOML: &str = ".codeflow/project.toml";
pub const INSTALLED_MANIFEST: &str = ".codeflow/manifest.json";
pub const BASELINE_DIR: &str = ".codeflow/.baseline";

/// How git hooks ended up wired at init.
pub const GIT_HOOKS_WIRED: &str = "wired";
pub const GIT_HOOKS_UNWIRED: &str = "unwired";

/// `.codeflow/project.toml` — the fields codeflow owns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub schema_version: u32,
    pub tier: Tier,
    pub scaffold_version: String,
    pub stack: String,
    pub areas: Vec<String>,
    /// Branch policy arming flag. Init writes `false`, makes the scaffold
    /// commit, then flips to `true` — the bootstrap grace that gives a brand
    /// new project its first hour with zero policy walls (charter AC #1).
    /// Git hooks and git-guard read this flag.
    pub policy_armed: bool,
    /// `"wired"` when codeflow set `core.hooksPath`; `"unwired"` when an
    /// existing hook manager (husky/lefthook/custom hooksPath) was detected
    /// and left alone — doctor surfaces this.
    pub git_hooks: String,
    pub permission_preset: String,
    #[serde(default)]
    pub product_one_liner: String,
}

impl ProjectState {
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(PROJECT_TOML)
    }

    #[must_use]
    pub fn exists(root: &Path) -> bool {
        Self::path(root).exists()
    }

    /// # Errors
    ///
    /// [`ScaffoldError::NotInitialized`] when the file is absent; otherwise
    /// IO/parse failures.
    pub fn load(root: &Path) -> Result<Self, ScaffoldError> {
        let path = Self::path(root);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ScaffoldError::NotInitialized
            } else {
                ScaffoldError::io(&path, e)
            }
        })?;
        toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })
    }

    /// Writes the state, preserving any keys in the existing file that this
    /// struct does not model (project.toml is user-owned).
    ///
    /// # Errors
    ///
    /// IO failures, or an existing file that is not valid TOML.
    pub fn store(&self, root: &Path) -> Result<(), ScaffoldError> {
        let path = Self::path(root);
        let ours = toml::Table::try_from(self).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        let merged = match std::fs::read_to_string(&path) {
            Ok(existing) => {
                let mut table: toml::Table =
                    toml::from_str(&existing).map_err(|e| ScaffoldError::InvalidState {
                        what: PROJECT_TOML.to_string(),
                        detail: e.to_string(),
                    })?;
                for (k, v) in ours {
                    table.insert(k, v);
                }
                table
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ours,
            Err(e) => return Err(ScaffoldError::io(&path, e)),
        };
        let text = toml::to_string_pretty(&merged).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        write_file(&path, text.as_bytes())
    }
}

/// The `[scaffold]` section of `project.toml` — user-owned knobs that steer
/// `codeflow update`.
///
/// Deliberately NOT a field of [`ProjectState`]: update reads it read-only via
/// [`ScaffoldConfig::load`], so `ProjectState::store` treats the whole
/// `[scaffold]` table as an unmodeled foreign key and round-trips it (plus any
/// future keys in it) verbatim — the same user-owned guarantee this module
/// gives every other unknown section.
#[derive(Debug, Clone, Default)]
pub struct ScaffoldConfig {
    /// Repo-relative globs of managed files `codeflow update` must leave
    /// entirely alone: never rewrite, never resurrect if the user deleted them,
    /// never prune as orphaned. The per-artifact opt-out for a team that does
    /// not want a shipped artifact (a non-Codex team's `.codex/**`, a GitLab
    /// team's `.github/**`). `*` and `?` match within one path segment; `**`
    /// spans segments.
    pub ignore: Vec<String>,
}

impl ScaffoldConfig {
    /// Reads `[scaffold]` from `project.toml`. A missing file or absent section
    /// yields an empty config (no opt-outs).
    ///
    /// # Errors
    ///
    /// IO failures other than not-found, or invalid TOML.
    pub fn load(root: &Path) -> Result<Self, ScaffoldError> {
        let path = ProjectState::path(root);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(ScaffoldError::io(&path, e)),
        };
        let table: toml::Table =
            toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
                what: PROJECT_TOML.to_string(),
                detail: e.to_string(),
            })?;
        let ignore = match table.get("scaffold") {
            None => Vec::new(),
            Some(value) => {
                let section = value
                    .as_table()
                    .ok_or_else(|| ScaffoldError::InvalidState {
                        what: PROJECT_TOML.to_string(),
                        detail: "scaffold: expected a table".to_string(),
                    })?;
                match section.get("ignore") {
                    None => Vec::new(),
                    Some(value) => {
                        let entries =
                            value
                                .as_array()
                                .ok_or_else(|| ScaffoldError::InvalidState {
                                    what: PROJECT_TOML.to_string(),
                                    detail: "scaffold.ignore: expected an array of strings"
                                        .to_string(),
                                })?;
                        entries
                            .iter()
                            .enumerate()
                            .map(|(index, value)| {
                                value.as_str().map(ToString::to_string).ok_or_else(|| {
                                    ScaffoldError::InvalidState {
                                        what: PROJECT_TOML.to_string(),
                                        detail: format!(
                                            "scaffold.ignore[{index}]: expected a string"
                                        ),
                                    }
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    }
                }
            }
        };
        Ok(Self { ignore })
    }

    /// Whether `dest` (a repo-relative managed-file path) matches any ignore
    /// glob — i.e. `codeflow update` must skip it entirely.
    #[must_use]
    pub fn is_ignored(&self, dest: &str) -> bool {
        self.ignore
            .iter()
            .any(|glob| crate::security::pattern::matches_extended_glob(dest, glob))
    }
}

/// One record in `.codeflow/manifest.json`.
///
/// `sha256` semantics by ownership class:
/// - `managed`: hash of the pristine shipped version (== the `.baseline/`
///   copy), NOT of the on-disk file when it carries user edits from a 3-way
///   merge — "file hash == recorded hash" means the user has not modified it;
/// - `managed-region` (markdown/hash): hash of the codeflow block only;
/// - `managed-region` (json): hash of the rendered shipped preset;
/// - `user-owned`: hash of the rendered shipped default (the user file is
///   never compared — only the defaults are diffed for new keys).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFile {
    pub src: String,
    pub ownership: Ownership,
    pub sha256: String,
    #[serde(default)]
    pub exec: bool,
}

/// `.codeflow/manifest.json` — what codeflow installed and at which version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledManifest {
    pub schema_version: u32,
    pub scaffold_version: String,
    /// dest path (project-root relative) → record.
    pub files: std::collections::BTreeMap<String, InstalledFile>,
}

impl InstalledManifest {
    #[must_use]
    pub fn new(scaffold_version: &str) -> Self {
        Self {
            schema_version: 1,
            scaffold_version: scaffold_version.to_string(),
            files: std::collections::BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(INSTALLED_MANIFEST)
    }

    /// Loads the record, or an empty one when absent.
    ///
    /// # Errors
    ///
    /// IO failures other than not-found, or invalid JSON.
    pub fn load_or_default(root: &Path, scaffold_version: &str) -> Result<Self, ScaffoldError> {
        let path = Self::path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(ScaffoldError::from),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new(scaffold_version)),
            Err(e) => Err(ScaffoldError::io(&path, e)),
        }
    }

    /// # Errors
    ///
    /// IO or serialization failures.
    pub fn store(&self, root: &Path) -> Result<(), ScaffoldError> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_file(&Self::path(root), text.as_bytes())
    }
}

/// Baseline (pristine shipped copy) storage under `.codeflow/.baseline/<dest>`.
pub struct Baseline;

impl Baseline {
    #[must_use]
    pub fn path(root: &Path, dest: &str) -> PathBuf {
        root.join(BASELINE_DIR).join(dest)
    }

    /// Repo-relative baseline path, for the symlink guard.
    fn rel(dest: &str) -> String {
        format!("{BASELINE_DIR}/{dest}")
    }

    #[must_use]
    pub fn read(root: &Path, dest: &str) -> Option<String> {
        // A symlinked baseline path resolves to `None` (treated as absent),
        // exactly like a missing baseline — never a read that follows the link.
        read_beneath_root(root, &Self::rel(dest)).ok().flatten()
    }

    /// # Errors
    ///
    /// IO failures, or a symlinked baseline path (refused, not followed).
    pub fn write(root: &Path, dest: &str, content: &str) -> Result<(), ScaffoldError> {
        write_beneath_root(root, &Self::rel(dest), content.as_bytes())
    }

    /// Removes the baseline copy for `dest`. A missing baseline is not an error.
    ///
    /// # Errors
    ///
    /// IO failures other than not-found, or a symlinked baseline path.
    pub fn remove(root: &Path, dest: &str) -> Result<(), ScaffoldError> {
        remove_beneath_root(root, &Self::rel(dest))
    }
}

/// Creates parent directories and atomically writes `bytes` to `path`.
///
/// The bytes go to a temp file that is synced before it is renamed over
/// `path`, so a crash leaves the old content or the new, never a torn or
/// empty file, including after a power cut. Outside a [`SyncBatch`] the
/// content sync is a full flush and the parent directory is flushed at once,
/// as for any single write. Inside a batch the content sync is the cheapest
/// call that still puts the data ahead of the rename on the disk (see the
/// `sync` module) and the directory flush waits for [`SyncBatch::finish`],
/// which flushes each touched directory once and the device cache once per
/// run.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ScaffoldError> {
    let parent = path.parent().ok_or_else(|| {
        ScaffoldError::io(
            path,
            std::io::Error::other("destination has no parent directory"),
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|e| ScaffoldError::io(parent, e))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", ulid::Ulid::new()));
    let batched = sync::batch_active();

    let result = (|| {
        use std::io::Write;
        let mut temp = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| ScaffoldError::io(&temp_path, e))?;
        temp.write_all(bytes)
            .map_err(|e| ScaffoldError::io(&temp_path, e))?;
        sync::content(&temp, !batched).map_err(|e| ScaffoldError::io(&temp_path, e))?;
        #[cfg(test)]
        interruption::before_rename(path)?;
        std::fs::rename(&temp_path, path).map_err(|e| ScaffoldError::io(path, e))?;
        sync::written();
        #[cfg(test)]
        interruption::renamed(path, bytes);
        if batched {
            sync::defer_directory(parent);
            Ok(())
        } else {
            sync::flush_directories(&[parent.to_path_buf()])
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// One scaffold run's deferred directory flushes (TSK-153).
///
/// A run (`init`, `update`, a pull request template decision) opens a batch
/// before its writes and calls [`SyncBatch::finish`] after them, so the
/// expensive flush happens once per touched directory and once per run
/// instead of for every file. A batch opened while another is active on the
/// same thread joins it: only the outermost one flushes. A batch dropped
/// without `finish` (an error path) still flushes, ignoring failures.
#[must_use = "call `finish` so the run's directory flushes happen and report errors"]
pub struct SyncBatch {
    outermost: bool,
    finished: bool,
}

impl SyncBatch {
    /// Opens a batch on this thread, or joins the one already open.
    pub fn begin() -> Self {
        Self {
            outermost: sync::open_batch(),
            finished: false,
        }
    }

    /// Flushes every directory the batch touched, then the device cache.
    ///
    /// # Errors
    ///
    /// A directory that cannot be opened or synced.
    pub fn finish(mut self) -> Result<(), ScaffoldError> {
        self.finished = true;
        if self.outermost {
            sync::flush_directories(&sync::close_batch())
        } else {
            Ok(())
        }
    }
}

impl Drop for SyncBatch {
    fn drop(&mut self) {
        if self.outermost && !self.finished {
            let _ = sync::flush_directories(&sync::close_batch());
        }
    }
}

/// Counts of the sync calls made on this thread, for tests and timing notes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncCounts {
    /// Managed files written and renamed into place.
    pub files_written: usize,
    /// Ordered content syncs inside a batch: `F_BARRIERFSYNC` on macOS,
    /// `fsync` elsewhere.
    pub content_syncs: usize,
    /// Plain `fsync` of a directory after renames into it.
    pub directory_fsyncs: usize,
    /// Device cache flushes: `sync_all` for a single write outside a batch,
    /// and on macOS the run's final `F_FULLFSYNC` and any refused barrier's
    /// fallback.
    pub full_flushes: usize,
}

/// The sync calls this thread has made since it started.
#[must_use]
pub fn sync_counts() -> SyncCounts {
    sync::counts()
}

mod sync {
    //! The platform sync calls behind [`super::write_file`] and
    //! [`super::SyncBatch`], with per-thread counts.
    //!
    //! A renamed file must never be torn or empty, even after a power cut,
    //! so its data has to reach the disk before the rename does.
    //!
    //! - macOS: `fsync(2)` only hands the data to the drive, which may write
    //!   its cache in any order, so after a power cut the rename can land
    //!   without the data. `File::sync_all` is `fcntl(F_FULLFSYNC)`, which
    //!   empties the whole drive cache (about 13 ms a call on an internal
    //!   SSD). `fcntl(F_BARRIERFSYNC)` writes the file's data and makes the
    //!   drive finish it before any later write, the rename included (about
    //!   5.5 ms), so each file in a run gets the barrier and the run ends with
    //!   one `F_FULLFSYNC`. A file system that refuses the barrier gets
    //!   `F_FULLFSYNC` instead, never a plain `fsync`.
    //! - Linux: `fsync(2)` writes the file's data and flushes the device
    //!   cache before it returns, so it already orders the data ahead of the
    //!   rename; each directory sync is a full one and no separate device
    //!   flush is issued.

    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;
    use std::fs::File;
    use std::path::{Path, PathBuf};

    use super::{ScaffoldError, SyncCounts};

    thread_local! {
        static PENDING: RefCell<Option<BTreeSet<PathBuf>>> = const { RefCell::new(None) };
        static COUNTS: Cell<SyncCounts> = const {
            Cell::new(SyncCounts {
                files_written: 0,
                content_syncs: 0,
                directory_fsyncs: 0,
                full_flushes: 0,
            })
        };
    }

    fn bump(update: impl FnOnce(&mut SyncCounts)) {
        COUNTS.with(|cell| {
            let mut counts = cell.get();
            update(&mut counts);
            cell.set(counts);
        });
    }

    pub(super) fn counts() -> SyncCounts {
        COUNTS.with(Cell::get)
    }

    pub(super) fn written() {
        bump(|c| c.files_written += 1);
    }

    pub(super) fn batch_active() -> bool {
        PENDING.with(|pending| pending.borrow().is_some())
    }

    /// Opens a batch; `false` when one is already open (the caller joins it).
    pub(super) fn open_batch() -> bool {
        PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            if pending.is_some() {
                false
            } else {
                *pending = Some(BTreeSet::new());
                true
            }
        })
    }

    pub(super) fn close_batch() -> Vec<PathBuf> {
        PENDING.with(|pending| {
            pending
                .borrow_mut()
                .take()
                .map(|set| set.into_iter().collect())
                .unwrap_or_default()
        })
    }

    pub(super) fn defer_directory(dir: &Path) {
        PENDING.with(|pending| {
            if let Some(set) = pending.borrow_mut().as_mut() {
                set.insert(dir.to_path_buf());
            }
        });
    }

    /// Makes a written file's content durable, or at least ordered ahead of
    /// its rename: a full flush outside a batch, an ordered sync inside one.
    pub(super) fn content(file: &File, full: bool) -> std::io::Result<()> {
        if full {
            file.sync_all()?;
            bump(|c| c.full_flushes += 1);
        } else {
            ordered(file)?;
            bump(|c| c.content_syncs += 1);
        }
        Ok(())
    }

    /// `F_BARRIERFSYNC`, or `F_FULLFSYNC` where the file system refuses it;
    /// the fallback counts as a full flush, so the tests see it.
    #[cfg(target_vendor = "apple")]
    fn ordered(file: &File) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        // SAFETY: `fcntl` with `F_BARRIERFSYNC` takes no argument and only
        // uses the descriptor, which `file` keeps open for the call.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_BARRIERFSYNC) } == -1 {
            file.sync_all()?;
            bump(|c| c.full_flushes += 1);
        }
        Ok(())
    }

    /// `fsync` already flushes the device where the target is not Apple.
    #[cfg(not(target_vendor = "apple"))]
    fn ordered(file: &File) -> std::io::Result<()> {
        fsync(file)
    }

    /// Syncs each directory once, then flushes the device cache once where
    /// `fsync` does not already do it.
    // Directory sync is a Unix call; other targets keep the fallible contract.
    #[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
    pub(super) fn flush_directories(dirs: &[PathBuf]) -> Result<(), ScaffoldError> {
        #[cfg(unix)]
        {
            let mut last = None;
            for dir in dirs {
                let handle = File::open(dir).map_err(|e| ScaffoldError::io(dir, e))?;
                fsync(&handle).map_err(|e| ScaffoldError::io(dir, e))?;
                bump(|c| c.directory_fsyncs += 1);
                last = Some((dir, handle));
            }
            if let Some((dir, handle)) = last {
                device_flush(&handle).map_err(|e| ScaffoldError::io(dir, e))?;
            }
        }
        #[cfg(not(unix))]
        let _ = dirs;
        Ok(())
    }

    #[cfg(unix)]
    fn fsync(file: &File) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        // SAFETY: `fsync` only reads the descriptor, which `file` keeps open
        // for the duration of the call.
        if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    #[cfg(not(unix))]
    fn fsync(file: &File) -> std::io::Result<()> {
        file.sync_all()
    }

    /// One device cache flush on macOS; nothing where `fsync` already
    /// flushes the device.
    #[cfg(unix)]
    fn device_flush(file: &File) -> std::io::Result<()> {
        if cfg!(target_vendor = "apple") {
            file.sync_all()?;
            bump(|c| c.full_flushes += 1);
        }
        Ok(())
    }
}

/// A test-only interruption point between a file's synced temp copy and its
/// rename, standing in for a crash at that moment (TSK-153 AC-4).
#[cfg(test)]
pub(crate) mod interruption {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use super::ScaffoldError;

    thread_local! {
        static STOP_AT: Cell<Option<usize>> = const { Cell::new(None) };
        static COMPLETED: RefCell<Vec<(PathBuf, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }

    /// Interrupts the write after `completed` writes have landed on this
    /// thread; `None` lets every write through. Clears the completed log.
    pub(crate) fn arm(completed: Option<usize>) {
        STOP_AT.with(|stop| stop.set(completed));
        COMPLETED.with(|log| log.borrow_mut().clear());
    }

    /// The writes that landed since [`arm`], in order.
    pub(crate) fn completed() -> Vec<(PathBuf, Vec<u8>)> {
        COMPLETED.with(|log| log.borrow().clone())
    }

    pub(super) fn before_rename(path: &Path) -> Result<(), ScaffoldError> {
        let landed = COMPLETED.with(|log| log.borrow().len());
        // Sticky, like a crash: once reached, no later write lands either.
        if STOP_AT.with(Cell::get).is_some_and(|stop| landed >= stop) {
            return Err(ScaffoldError::io(
                path,
                std::io::Error::other("test interruption before rename"),
            ));
        }
        Ok(())
    }

    pub(super) fn renamed(path: &Path, bytes: &[u8]) {
        COMPLETED.with(|log| log.borrow_mut().push((path.to_path_buf(), bytes.to_vec())));
    }
}

/// Resolves `root.join(rel)` while refusing to traverse a symlink.
///
/// The manifest-text guard (`is_safe_relative_dest`) blocks an absolute or
/// `..`-escaping dest, but `root.join(rel)` still *follows* any symlink already
/// on disk: a leaf symlink, or a symlinked ancestor directory, pre-placed in
/// the target repo (e.g. one cloned from a hostile repo) would let a scaffold
/// read / write / prune land OUTSIDE the tree. `root` is trusted (it may itself
/// sit under a symlink — a macOS `/var` tempdir does); only the components
/// BENEATH it are checked, each with `symlink_metadata` (which never follows),
/// so the first symlinked component is refused before any IO touches it.
///
/// This is a check-then-act guard: a live attacker swapping a component for a
/// symlink between this check and the following IO is a residual TOCTOU window
/// no `std::fs` primitive closes portably. It is sized to the real threat — a
/// symlink pre-planted before `codeflow init`/`update` runs — not to a process
/// racing the scaffold on the same tree.
pub(crate) fn guard_beneath_root(root: &Path, rel: &Path) -> Result<PathBuf, ScaffoldError> {
    use std::path::Component;
    let mut cur = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::CurDir => continue,
            Component::Normal(seg) => cur.push(seg),
            // Absolute / `..` / prefix components can only escape; `root.join`
            // would resolve them away from the tree. Refuse rather than trust.
            _ => {
                return Err(ScaffoldError::UnsafeSymlink {
                    path: rel.to_path_buf(),
                })
            }
        }
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(ScaffoldError::UnsafeSymlink { path: cur });
            }
            // Absent (not yet created) or a real file/dir: safe to descend.
            _ => {}
        }
    }
    Ok(cur)
}

/// Repo-relative read that refuses to follow a symlink. `Ok(None)` when the
/// file is absent; a symlinked path (leaf or ancestor) is a hard error.
pub(crate) fn read_beneath_root(root: &Path, rel: &str) -> Result<Option<String>, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(rel))?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ScaffoldError::io(&path, e)),
    }
}

/// Repo-relative write that refuses to follow a symlink (leaf or ancestor),
/// creating parent directories as needed.
pub(crate) fn write_beneath_root(
    root: &Path,
    rel: &str,
    bytes: &[u8],
) -> Result<(), ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(rel))?;
    write_file(&path, bytes)
}

/// Repo-relative delete that refuses to follow a symlink (leaf or ancestor).
/// A missing file is not an error.
pub(crate) fn remove_beneath_root(root: &Path, rel: &str) -> Result<(), ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(rel))?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ScaffoldError::io(&path, e)),
    }
}

/// Sets (or clears) the executable bit.
// Windows has no Unix executable bit, but callers retain one cross-platform
// fallible contract.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
pub(crate) fn set_exec(path: &Path, exec: bool) -> Result<(), ScaffoldError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| ScaffoldError::io(path, e))?;
        let mut perms = meta.permissions();
        let mode = if exec {
            perms.mode() | 0o111
        } else {
            perms.mode() & !0o111
        };
        perms.set_mode(mode);
        std::fs::set_permissions(path, perms).map_err(|e| ScaffoldError::io(path, e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, exec);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_state_round_trip_preserves_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state = ProjectState {
            schema_version: 1,
            tier: Tier::Standard,
            scaffold_version: "2.0.0".to_string(),
            stack: "rust".to_string(),
            areas: vec!["core".to_string()],
            policy_armed: true,
            git_hooks: GIT_HOOKS_WIRED.to_string(),
            permission_preset: "default".to_string(),
            product_one_liner: "demo".to_string(),
        };
        state.store(root).unwrap();

        // User adds a key codeflow does not model.
        let path = ProjectState::path(root);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\n[orient]\nenabled = false\n");
        std::fs::write(&path, text).unwrap();

        let mut loaded = ProjectState::load(root).unwrap();
        loaded.policy_armed = false;
        loaded.store(root).unwrap();

        let final_text = std::fs::read_to_string(&path).unwrap();
        assert!(final_text.contains("[orient]"), "foreign key survived");
        assert!(final_text.contains("policy_armed = false"));
    }

    #[test]
    fn scaffold_config_ignore_glob_matching() {
        let cfg = ScaffoldConfig {
            ignore: vec![
                ".codex/**".to_string(),
                ".github/workflows/*.yml".to_string(),
                ".claude/settings.json".to_string(),
            ],
        };
        assert!(cfg.is_ignored(".codex/config.toml"));
        assert!(
            cfg.is_ignored(".codex/agents/foo.md"),
            "** spans path segments"
        );
        assert!(cfg.is_ignored(".github/workflows/ci.yml"));
        assert!(
            cfg.is_ignored(".claude/settings.json"),
            "exact path matches"
        );
        assert!(
            !cfg.is_ignored(".github/workflows/nested/ci.yml"),
            "* stays within a single path segment"
        );
        assert!(
            !cfg.is_ignored(".claude/workflows/develop.md"),
            "unrelated path kept"
        );

        assert!(
            !ScaffoldConfig::default().is_ignored(".codex/config.toml"),
            "no globs ignores nothing"
        );
    }

    #[test]
    fn scaffold_config_load_reads_ignore_section() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(
            &ProjectState::path(root),
            b"schema_version = 1\ntier = \"full\"\n\n[scaffold]\nignore = [\".codex/**\", \".github/**\"]\n",
        )
        .unwrap();
        let cfg = ScaffoldConfig::load(root).unwrap();
        assert_eq!(
            cfg.ignore,
            vec![".codex/**".to_string(), ".github/**".to_string()]
        );

        // Missing file and absent section both yield an empty (no-op) config.
        let empty = tempfile::tempdir().unwrap();
        assert!(ScaffoldConfig::load(empty.path())
            .unwrap()
            .ignore
            .is_empty());
        write_file(&ProjectState::path(empty.path()), b"schema_version = 1\n").unwrap();
        assert!(ScaffoldConfig::load(empty.path())
            .unwrap()
            .ignore
            .is_empty());
    }

    #[test]
    fn scaffold_config_rejects_wrong_ignore_types_with_location() {
        for (body, location) in [
            ("[scaffold]\nignore = true\n", "scaffold.ignore"),
            (
                "[scaffold]\nignore = [\".codex/**\", 7]\n",
                "scaffold.ignore[1]",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            write_file(&ProjectState::path(dir.path()), body.as_bytes()).unwrap();
            let error = ScaffoldConfig::load(dir.path()).unwrap_err().to_string();
            assert!(error.contains(location), "{error}");
        }
    }

    #[test]
    fn project_state_store_propagates_non_not_found_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ProjectState::path(dir.path())).unwrap();
        let state = ProjectState {
            schema_version: 1,
            tier: Tier::Standard,
            scaffold_version: "2.0.0".to_string(),
            stack: "rust".to_string(),
            areas: Vec::new(),
            policy_armed: false,
            git_hooks: GIT_HOOKS_UNWIRED.to_string(),
            permission_preset: "default".to_string(),
            product_one_liner: String::new(),
        };
        let error = state.store(dir.path()).unwrap_err().to_string();
        assert!(error.contains(PROJECT_TOML), "{error}");
        assert!(ProjectState::path(dir.path()).is_dir());
    }

    #[test]
    fn write_file_atomically_replaces_content_and_syncs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("managed.txt");
        write_file(&path, b"old").unwrap();
        write_file(&path, b"new content").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new content");
        let leftovers = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    // codex round-2: scaffold IO must not follow a pre-planted symlink out of
    // the repo. `guard_beneath_root` checks each component beneath root no-follow.
    #[test]
    fn guard_beneath_root_allows_normal_and_absent_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // An absent nested dest is safe to descend (nothing on disk to follow).
        assert_eq!(
            guard_beneath_root(root, Path::new("a/b/c.txt")).unwrap(),
            root.join("a/b/c.txt")
        );
        // A real file under a real dir is fine.
        std::fs::create_dir_all(root.join("real")).unwrap();
        write_file(&root.join("real/f.txt"), b"x").unwrap();
        assert!(guard_beneath_root(root, Path::new("real/f.txt")).is_ok());
    }

    #[test]
    fn guard_beneath_root_refuses_parent_and_absolute_components() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(guard_beneath_root(root, Path::new("../escape")).is_err());
        assert!(guard_beneath_root(root, Path::new("/etc/passwd")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn guard_beneath_root_refuses_leaf_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        symlink(outside.path().join("target.txt"), root.join("link.txt")).unwrap();
        let err = guard_beneath_root(root, Path::new("link.txt")).unwrap_err();
        assert!(matches!(err, ScaffoldError::UnsafeSymlink { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn guard_beneath_root_refuses_ancestor_symlink_and_writes_nothing_outside() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A directory ancestor is a symlink pointing outside the tree.
        symlink(outside.path(), root.join("sub")).unwrap();
        assert!(matches!(
            guard_beneath_root(root, Path::new("sub/evil.txt")).unwrap_err(),
            ScaffoldError::UnsafeSymlink { .. }
        ));
        // A write through the ancestor link is refused and lands nothing outside.
        assert!(write_beneath_root(root, "sub/evil.txt", b"pwned").is_err());
        assert!(
            !outside.path().join("evil.txt").exists(),
            "write must not escape the repo via the ancestor symlink"
        );
    }

    // TSK-153: sync calls per file and per run.

    #[test]
    fn a_single_write_outside_a_batch_is_fully_flushed_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let before = sync_counts();
        write_file(&dir.path().join("one.txt"), b"one").unwrap();
        let after = sync_counts();
        assert_eq!(after.files_written - before.files_written, 1);
        assert_eq!(
            after.content_syncs, before.content_syncs,
            "content is fully flushed"
        );
        assert_eq!(after.directory_fsyncs - before.directory_fsyncs, 1);
        let device = usize::from(cfg!(target_vendor = "apple"));
        assert_eq!(after.full_flushes - before.full_flushes, 1 + device);
    }

    #[test]
    fn a_batch_syncs_each_file_in_order_and_flushes_once_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let before = sync_counts();
        let batch = SyncBatch::begin();
        for name in ["a/1", "a/2", "a/3", "b/1", "b/2"] {
            write_file(&dir.path().join(name), name.as_bytes()).unwrap();
        }
        let inner = SyncBatch::begin(); // joins the open batch
        write_file(&dir.path().join("a/4"), b"4").unwrap();
        inner.finish().unwrap();
        let mid = sync_counts();
        assert_eq!(mid.files_written - before.files_written, 6);
        assert_eq!(
            mid.content_syncs - before.content_syncs,
            6,
            "one ordered sync per file"
        );
        assert_eq!(
            mid.directory_fsyncs, before.directory_fsyncs,
            "deferred to the end"
        );
        assert_eq!(
            mid.full_flushes, before.full_flushes,
            "no full flush per file"
        );
        batch.finish().unwrap();
        let after = sync_counts();
        assert_eq!(
            after.directory_fsyncs - before.directory_fsyncs,
            2,
            "once per directory"
        );
        let device = usize::from(cfg!(target_vendor = "apple"));
        assert_eq!(
            after.full_flushes - before.full_flushes,
            device,
            "once per run"
        );
        assert_eq!(std::fs::read(dir.path().join("b/2")).unwrap(), b"b/2");
    }

    #[test]
    fn a_batch_dropped_on_an_error_path_still_flushes() {
        let dir = tempfile::tempdir().unwrap();
        let before = sync_counts();
        {
            let _batch = SyncBatch::begin();
            write_file(&dir.path().join("x/1"), b"1").unwrap();
        }
        let after = sync_counts();
        assert_eq!(after.directory_fsyncs - before.directory_fsyncs, 1);
        assert!(!sync::batch_active(), "the dropped batch closed");
    }

    fn shipped_assets() -> crate::scaffold::DirSource {
        crate::scaffold::DirSource::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
    }

    fn init_opts(tier: Tier) -> crate::scaffold::InitOptions {
        crate::scaffold::InitOptions {
            tier: Some(tier),
            force: false,
            binary_version: "9.9.9".to_string(),
            answers: crate::scaffold::InitAnswers::default(),
        }
    }

    fn force_update() -> crate::scaffold::UpdateOptions {
        crate::scaffold::UpdateOptions {
            force: true,
            binary_version: "9.9.9".to_string(),
            diff_out: None,
        }
    }

    /// Every file under `root` outside `.git`, keyed by its relative path.
    fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut files = std::collections::BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().is_some_and(|name| name == ".git") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(root).unwrap().to_path_buf();
                    files.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        files
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    /// Hands every managed skill file a local edit, so a forced update
    /// replaces existing content rather than only adding files.
    fn edit_managed_skills(root: &Path) -> usize {
        let mut edited = 0;
        for (rel, bytes) in snapshot(root) {
            let text = rel.to_string_lossy();
            if text.starts_with(".agents/skills/") && text.ends_with("SKILL.md") {
                let mut bytes = bytes;
                bytes.extend_from_slice(b"\nlocal edit\n");
                std::fs::write(root.join(&rel), bytes).unwrap();
                edited += 1;
            }
        }
        edited
    }

    #[test]
    fn init_and_update_flush_the_device_once_per_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let device = usize::from(cfg!(target_vendor = "apple"));

        let before = sync_counts();
        crate::scaffold::init(&shipped_assets(), &root, &init_opts(Tier::Standard)).unwrap();
        let after = sync_counts();
        let written = after.files_written - before.files_written;
        let directories = after.directory_fsyncs - before.directory_fsyncs;
        assert!(
            written > 200,
            "a standard init writes the scaffold: {written}"
        );
        assert_eq!(
            after.content_syncs - before.content_syncs,
            written,
            "one per file"
        );
        assert!(
            directories > 0 && directories < written / 2,
            "{directories} of {written}"
        );
        assert_eq!(
            after.full_flushes - before.full_flushes,
            device,
            "one per run"
        );
        println!("init: {written} files, {directories} directories, {device} device flush");

        assert!(edit_managed_skills(&root) > 5);
        let before = sync_counts();
        crate::scaffold::update(&shipped_assets(), &root, &force_update()).unwrap();
        let after = sync_counts();
        let written = after.files_written - before.files_written;
        assert!(
            written > 5,
            "the forced update rewrites the edited skills: {written}"
        );
        assert_eq!(after.content_syncs - before.content_syncs, written);
        assert_eq!(after.full_flushes - before.full_flushes, device);
        println!(
            "update: {written} files, {} directories",
            after.directory_fsyncs - before.directory_fsyncs
        );
    }

    /// After a run stops at an arbitrary write, each file holds its content
    /// from before the run or the content of a write that completed: never
    /// torn, never empty.
    fn assert_whole_files(
        root: &Path,
        before: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
        stop: usize,
    ) {
        let completed = interruption::completed();
        for (rel, bytes) in snapshot(root) {
            let hidden = rel.file_name().unwrap().to_string_lossy().starts_with('.');
            if hidden && rel.extension().is_some_and(|ext| ext == "tmp") {
                continue; // a temp copy, never a managed path
            }
            let landed = completed
                .iter()
                .filter(|(path, _)| path.strip_prefix(root).is_ok_and(|r| r == rel))
                .any(|(_, content)| *content == bytes);
            let kept = before.get(&rel).is_some_and(|old| *old == bytes);
            assert!(
                landed || kept,
                "stop {stop}: {} is neither its earlier content nor a completed write ({} bytes)",
                rel.display(),
                bytes.len()
            );
        }
    }

    #[test]
    fn an_interrupted_init_or_update_leaves_no_torn_or_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        // A fresh init, stopped at writes spread across the run.
        interruption::arm(None);
        crate::scaffold::init(
            &shipped_assets(),
            &dir.path().join("whole"),
            &init_opts(Tier::Standard),
        )
        .unwrap();
        let total = interruption::completed().len();
        let base = dir.path().join("base");
        crate::scaffold::init(&shipped_assets(), &base, &init_opts(Tier::Standard)).unwrap();
        assert!(edit_managed_skills(&base) > 5);
        let edited = snapshot(&base);
        interruption::arm(None);
        copy_tree(&base, &dir.path().join("probe"));
        crate::scaffold::update(
            &shipped_assets(),
            &dir.path().join("probe"),
            &force_update(),
        )
        .unwrap();
        let update_total = interruption::completed().len();

        for stop in [0, total / 3, 2 * total / 3, total - 1] {
            let root = dir.path().join(format!("init-{stop}"));
            interruption::arm(Some(stop));
            assert!(
                crate::scaffold::init(&shipped_assets(), &root, &init_opts(Tier::Standard))
                    .is_err(),
                "stop {stop} of {total} interrupts the init"
            );
            assert_eq!(interruption::completed().len(), stop);
            assert_whole_files(&root, &std::collections::BTreeMap::new(), stop);
        }
        for stop in [0, update_total / 2, update_total - 1] {
            let root = dir.path().join(format!("update-{stop}"));
            copy_tree(&base, &root);
            interruption::arm(Some(stop));
            assert!(
                crate::scaffold::update(&shipped_assets(), &root, &force_update()).is_err(),
                "stop {stop} of {update_total} interrupts the update"
            );
            assert_whole_files(&root, &edited, stop);
        }
        interruption::arm(None);
    }
}
