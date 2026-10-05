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
    /// The adoption marker of the release rule (SPC-013 R-120): `1`, written
    /// by `init` and `update` of the release that brings the rule and never
    /// rewritten after. It fixes where the transition tables stop; `init`
    /// and `update` never write a table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_rules: Option<i64>,
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
        let ours = toml::Table::try_from(self).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        // Read without following a link, as the write below refuses one.
        let merged = match read_beneath_root(root, PROJECT_TOML)? {
            Some(existing) => {
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
            None => ours,
        };
        let text = toml::to_string_pretty(&merged).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        write_record(root, PROJECT_TOML, text.as_bytes())
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

/// The `[feedback]` section of `project.toml`: the topics an operator
/// feedback item may carry (`codeflow feedback`). User-owned like
/// `[scaffold]`: [`ProjectState::store`] round-trips it as a foreign key, and
/// only [`FeedbackConfig::write_defaults`] ever adds it, once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedbackConfig {
    /// The project's topic list; `None` when the section or key is absent.
    pub topics: Option<Vec<String>>,
}

impl FeedbackConfig {
    /// The list written on the first `codeflow feedback new` when the project
    /// has none.
    pub const DEFAULT_TOPICS: [&'static str; 7] = [
        "process",
        "design",
        "architecture",
        "writing",
        "tooling",
        "security",
        "scope",
    ];

    /// Reads `[feedback]` from `project.toml`. A missing file, section or key
    /// yields no list.
    ///
    /// # Errors
    ///
    /// IO failures other than not-found, invalid TOML, or a section that is
    /// not a table of non-empty topic strings.
    pub fn load(root: &Path) -> Result<Self, ScaffoldError> {
        let Some(text) = read_beneath_root(root, PROJECT_TOML)? else {
            return Ok(Self::default());
        };
        let table: toml::Table =
            toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
                what: PROJECT_TOML.to_string(),
                detail: e.to_string(),
            })?;
        let invalid = |detail: String| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail,
        };
        let Some(section) = table.get("feedback") else {
            return Ok(Self::default());
        };
        let section = section
            .as_table()
            .ok_or_else(|| invalid("feedback: expected a table".to_string()))?;
        let Some(topics) = section.get("topics") else {
            return Ok(Self::default());
        };
        let topics = topics
            .as_array()
            .ok_or_else(|| invalid("feedback.topics: expected an array of strings".to_string()))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|topic| !topic.is_empty() && !topic.contains(char::is_whitespace))
                    .map(ToString::to_string)
                    .ok_or_else(|| {
                        invalid(format!(
                            "feedback.topics[{index}]: expected a one-word topic string"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            topics: Some(topics),
        })
    }

    /// The project's topics, or the default list when it has none.
    #[must_use]
    pub fn topics_or_default(&self) -> Vec<String> {
        self.topics.clone().unwrap_or_else(|| {
            Self::DEFAULT_TOPICS
                .iter()
                .map(ToString::to_string)
                .collect()
        })
    }

    /// Write the default list into an existing `project.toml` that has no
    /// `feedback.topics`: under its `[feedback]` header, or as a new section
    /// appended; the rest of the file keeps its bytes. Returns the topics in force afterwards and
    /// whether the file was written.
    ///
    /// # Errors
    ///
    /// [`ScaffoldError::NotInitialized`] without a `project.toml`, IO
    /// failures, invalid TOML, or a `[feedback]` value that is not a table.
    pub fn write_defaults(root: &Path) -> Result<(Vec<String>, bool), ScaffoldError> {
        let Some(text) = read_beneath_root(root, PROJECT_TOML)? else {
            return Err(ScaffoldError::NotInitialized);
        };
        let current = Self::load(root)?;
        if let Some(topics) = current.topics {
            return Ok((topics, false));
        }
        let table: toml::Table =
            toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
                what: PROJECT_TOML.to_string(),
                detail: e.to_string(),
            })?;
        let mut topics_line = String::from("topics = [\n");
        for topic in Self::DEFAULT_TOPICS {
            topics_line.push_str("    \"");
            topics_line.push_str(topic);
            topics_line.push_str("\",\n");
        }
        topics_line.push_str("]\n");
        // Either way every other byte of the file stays as the project wrote
        // it: the key goes right under an existing `[feedback]` header, or a
        // new section is appended.
        let updated = if table.contains_key("feedback") {
            let header = text.split_inclusive('\n').position(|line| {
                let code = line.split('#').next().unwrap_or_default().trim();
                code == "[feedback]"
            });
            let Some(header) = header else {
                return Err(ScaffoldError::InvalidState {
                    what: PROJECT_TOML.to_string(),
                    detail: "`feedback` is set without a `[feedback]` header line; add `topics` to it by hand".to_string(),
                });
            };
            let mut updated = String::with_capacity(text.len() + topics_line.len() + 1);
            for (index, line) in text.split_inclusive('\n').enumerate() {
                updated.push_str(line);
                if index == header {
                    if !line.ends_with('\n') {
                        updated.push('\n');
                    }
                    updated.push_str(&topics_line);
                }
            }
            updated
        } else {
            let mut updated = text.clone();
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str("\n[feedback]\n");
            updated.push_str(&topics_line);
            updated
        };
        let check: toml::Table =
            toml::from_str(&updated).map_err(|e| ScaffoldError::InvalidState {
                what: PROJECT_TOML.to_string(),
                detail: format!("adding the feedback topics would break the file: {e}"),
            })?;
        if check
            .get("feedback")
            .and_then(|v| v.get("topics"))
            .is_none()
        {
            return Err(ScaffoldError::InvalidState {
                what: PROJECT_TOML.to_string(),
                detail: "the feedback topics could not be added".to_string(),
            });
        }
        // The state file keeps its permissions (a private one stays
        // private) and the scaffold writer's durability.
        write_record_keeping_mode(root, PROJECT_TOML, updated.as_bytes())?;
        Ok((Self::load(root)?.topics_or_default(), true))
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
        write_record(root, INSTALLED_MANIFEST, text.as_bytes())
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
        write_record(root, &Self::rel(dest), content.as_bytes())
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
/// content and the parent directory are fully flushed at once, as for any
/// single write. Inside a batch the content sync is the cheapest call that
/// still puts the data ahead of the rename on the disk, and the directory
/// sync waits until a record write ([`write_record`]) or
/// [`SyncBatch::finish`] needs it (see the `sync` module).
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ScaffoldError> {
    write_file_with_mode(path, bytes, None)
}

/// [`write_file`], and when `mode_from` names an existing regular file, the
/// result takes its permissions: the temporary file is private (0600 on
/// unix) until they are applied, so a private file never reads wider.
fn write_file_with_mode(
    path: &Path,
    bytes: &[u8],
    mode_from: Option<&Path>,
) -> Result<(), ScaffoldError> {
    let permissions = mode_from
        .and_then(|from| std::fs::symlink_metadata(from).ok())
        .filter(std::fs::Metadata::is_file)
        .map(|meta| meta.permissions());
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
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if permissions.is_some() {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        }
        let mut temp = options
            .open(&temp_path)
            .map_err(|e| ScaffoldError::io(&temp_path, e))?;
        temp.write_all(bytes)
            .map_err(|e| ScaffoldError::io(&temp_path, e))?;
        if let Some(permissions) = &permissions {
            temp.set_permissions(permissions.clone())
                .map_err(|e| ScaffoldError::io(&temp_path, e))?;
        }
        sync::content(&temp, !batched).map_err(|e| ScaffoldError::io(&temp_path, e))?;
        #[cfg(test)]
        interruption::before_rename(path, bytes)?;
        std::fs::rename(&temp_path, path).map_err(|e| ScaffoldError::io(path, e))?;
        sync::written();
        #[cfg(test)]
        interruption::renamed(path, bytes);
        if batched {
            sync::defer_directory(parent);
            Ok(())
        } else {
            sync::directory_now(parent)
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// Writes a state record (a baseline copy, the manifest, `project.toml`)
/// at repo-relative `rel`, refusing a symlink on the way (leaf or ancestor),
/// that says earlier writes are in place.
///
/// Inside a batch, every directory an earlier write renamed into is synced
/// first, and any other device holding those files is flushed, so the
/// record never reaches the disk ahead of the files it describes: after a
/// crash, a record that survived means those files did.
pub(crate) fn write_record(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(rel))?;
    sync::settle_before(&path)?;
    write_file(&path, bytes)
}

/// [`write_record`] that keeps the record's permissions, for a write that
/// must not widen a file the project made private (the feedback topics).
fn write_record_keeping_mode(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(rel))?;
    sync::settle_before(&path)?;
    write_file_with_mode(&path, bytes, Some(&path))
}

/// One scaffold run's deferred directory syncs and device flushes (TSK-153).
///
/// A run (`init`, `update`, a pull request template decision) opens a batch
/// before its writes and calls [`SyncBatch::finish`] after them. Directory
/// syncs wait for the next record write or the end of the run, and the full
/// device flush happens once per device the run touched instead of for every
/// file. A batch opened while another is active on the same thread joins it:
/// only the outermost one flushes. A batch dropped without `finish` (an error
/// path) still flushes, ignoring failures.
#[must_use = "call `finish` so the run's directory syncs and device flushes happen"]
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

    /// Syncs every directory the batch still has pending, then flushes the
    /// cache of each device the batch touched.
    ///
    /// # Errors
    ///
    /// A directory that cannot be opened or synced, or a failed flush.
    pub fn finish(mut self) -> Result<(), ScaffoldError> {
        self.finished = true;
        if self.outermost {
            sync::finish_batch()
        } else {
            Ok(())
        }
    }
}

impl Drop for SyncBatch {
    fn drop(&mut self) {
        if self.outermost && !self.finished {
            let _ = sync::finish_batch();
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
    /// Directory syncs after renames into them.
    pub directory_syncs: usize,
    /// Full flushes: a single write outside a batch (its content and its
    /// directory), and on macOS the `F_FULLFSYNC` for each device a run
    /// touched and any refused barrier's fallback.
    pub full_flushes: usize,
}

/// The sync calls this thread has made since it started.
#[must_use]
pub fn sync_counts() -> SyncCounts {
    sync::counts()
}

mod sync {
    //! The platform sync calls behind [`super::write_file`],
    //! [`super::write_record`] and [`super::SyncBatch`], with per-thread
    //! counts.
    //!
    //! Two orderings must hold across a crash or a power cut: a renamed file's
    //! data reaches the disk before its rename, and a state record reaches the
    //! disk only after the renames it describes.
    //!
    //! - macOS: `fsync(2)` only hands data to the drive, which may write its
    //!   cache in any order, so after a power cut a rename can land without
    //!   its data. `fcntl(F_FULLFSYNC)` empties the whole drive cache (about
    //!   13 ms a call on an internal SSD). `fcntl(F_BARRIERFSYNC)` writes the
    //!   file or directory and makes the drive finish it before any later
    //!   write (about 5.5 ms). So each file in a run gets the barrier before
    //!   its rename, each directory renamed into gets the barrier before the
    //!   next record write and at the end, and the run ends with one
    //!   `F_FULLFSYNC` for each device it touched. A file system that refuses
    //!   the barrier gets `F_FULLFSYNC` instead, never a plain `fsync`.
    //! - Linux: `fsync(2)` writes the file or directory and flushes the device
    //!   cache before it returns, so it gives both orderings; no separate
    //!   device flush is issued.

    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::File;
    use std::path::{Path, PathBuf};

    #[cfg(test)]
    use super::interruption::{self, Op};
    use super::{ScaffoldError, SyncCounts};

    /// The open batch: directories renamed into since the last settle, and
    /// one directory per device a settle has synced since that device's
    /// last flush.
    struct Batch {
        pending: BTreeSet<PathBuf>,
        devices: BTreeMap<u64, PathBuf>,
    }

    thread_local! {
        static BATCH: RefCell<Option<Batch>> = const { RefCell::new(None) };
        static COUNTS: Cell<SyncCounts> = const {
            Cell::new(SyncCounts {
                files_written: 0,
                content_syncs: 0,
                directory_syncs: 0,
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
        BATCH.with(|batch| batch.borrow().is_some())
    }

    /// Opens a batch; `false` when one is already open (the caller joins it).
    pub(super) fn open_batch() -> bool {
        BATCH.with(|batch| {
            let mut batch = batch.borrow_mut();
            if batch.is_some() {
                false
            } else {
                *batch = Some(Batch {
                    pending: BTreeSet::new(),
                    devices: BTreeMap::new(),
                });
                true
            }
        })
    }

    pub(super) fn defer_directory(dir: &Path) {
        BATCH.with(|batch| {
            if let Some(batch) = batch.borrow_mut().as_mut() {
                batch.pending.insert(dir.to_path_buf());
            }
        });
    }

    /// Makes a written file's content durable, or at least ordered ahead of
    /// its rename: a full flush outside a batch, an ordered sync inside one.
    pub(super) fn content(file: &File, full: bool) -> std::io::Result<()> {
        if full {
            full_flush(file)
        } else {
            ordered(file)?;
            bump(|c| c.content_syncs += 1);
            Ok(())
        }
    }

    /// Inside a batch, syncs every directory renamed into since the last
    /// settle, with the ordered sync, and notes its device. Outside a batch
    /// nothing is pending.
    // Directory sync is a Unix call; other targets keep the fallible contract.
    #[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
    pub(super) fn settle() -> Result<(), ScaffoldError> {
        let pending = BATCH.with(|batch| {
            batch
                .borrow_mut()
                .as_mut()
                .map(|batch| std::mem::take(&mut batch.pending))
                .unwrap_or_default()
        });
        #[cfg(unix)]
        for dir in pending {
            let handle = File::open(&dir).map_err(|e| ScaffoldError::io(&dir, e))?;
            ordered(&handle).map_err(|e| ScaffoldError::io(&dir, e))?;
            bump(|c| c.directory_syncs += 1);
            #[cfg(test)]
            interruption::synced(&dir);
            let device = device_of(&dir, &handle).map_err(|e| ScaffoldError::io(&dir, e))?;
            BATCH.with(|batch| {
                if let Some(batch) = batch.borrow_mut().as_mut() {
                    batch.devices.entry(device).or_insert(dir);
                }
            });
        }
        #[cfg(not(unix))]
        drop(pending);
        Ok(())
    }

    /// Settles before a state record at `record`. On macOS it then flushes
    /// each other device with syncs since its last flush: a barrier orders
    /// writes on its own device only, so it cannot keep a record on another
    /// device from reaching the disk first. The record's own device keeps
    /// the barrier alone.
    pub(super) fn settle_before(record: &Path) -> Result<(), ScaffoldError> {
        settle()?;
        #[cfg(target_vendor = "apple")]
        {
            let touched: Vec<(u64, PathBuf)> = BATCH.with(|batch| {
                batch.borrow().as_ref().map_or_else(Vec::new, |batch| {
                    batch
                        .devices
                        .iter()
                        .map(|(device, dir)| (*device, dir.clone()))
                        .collect()
                })
            });
            if touched.is_empty() {
                return Ok(());
            }
            let own = device_at(record).map_err(|e| ScaffoldError::io(record, e))?;
            for (device, dir) in touched {
                if device == own {
                    continue;
                }
                device_flush(&dir)?;
                BATCH.with(|batch| {
                    if let Some(batch) = batch.borrow_mut().as_mut() {
                        batch.devices.remove(&device);
                    }
                });
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        let _ = record;
        Ok(())
    }

    /// Closes the batch: settles what is pending, then flushes the cache of
    /// each device with syncs since its last flush.
    pub(super) fn finish_batch() -> Result<(), ScaffoldError> {
        let settled = settle();
        let devices = BATCH
            .with(|batch| batch.borrow_mut().take())
            .map(|batch| batch.devices)
            .unwrap_or_default();
        settled?;
        for dir in devices.values() {
            device_flush(dir)?;
        }
        Ok(())
    }

    /// A single write's directory, fully flushed at once.
    // Directory sync is a Unix call; other targets keep the fallible contract.
    #[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
    pub(super) fn directory_now(dir: &Path) -> Result<(), ScaffoldError> {
        #[cfg(unix)]
        {
            let handle = File::open(dir).map_err(|e| ScaffoldError::io(dir, e))?;
            full_flush(&handle).map_err(|e| ScaffoldError::io(dir, e))?;
            bump(|c| c.directory_syncs += 1);
            #[cfg(test)]
            {
                interruption::synced(dir);
                interruption::flushed(dir);
            }
        }
        #[cfg(not(unix))]
        let _ = dir;
        Ok(())
    }

    #[cfg(unix)]
    fn device_of(dir: &Path, handle: &File) -> std::io::Result<u64> {
        use std::os::unix::fs::MetadataExt;
        #[cfg(test)]
        if let Some(device) = interruption::device(dir) {
            return Ok(device);
        }
        let _ = dir;
        Ok(handle.metadata()?.dev())
    }

    /// The device a write to `path` lands on: that of its nearest existing
    /// parent directory. The write renames a temp file beside `path`,
    /// replacing whatever is there, so a link at `path` never decides it.
    #[cfg(target_vendor = "apple")]
    fn device_at(path: &Path) -> std::io::Result<u64> {
        use std::os::unix::fs::MetadataExt;
        let parent = path.parent().unwrap_or(path);
        let existing = parent
            .ancestors()
            .find(|dir| dir.exists())
            .unwrap_or(parent);
        #[cfg(test)]
        if let Some(device) = interruption::device_stat(existing) {
            return Ok(device);
        }
        Ok(std::fs::metadata(existing)?.dev())
    }

    /// The ordered sync: `F_BARRIERFSYNC`, or `F_FULLFSYNC` where the file
    /// system refuses the barrier.
    #[cfg(target_vendor = "apple")]
    fn ordered(file: &File) -> std::io::Result<()> {
        if fcntl(file, libc::F_BARRIERFSYNC).is_err() {
            full_flush(file)?;
        }
        Ok(())
    }

    /// The ordered sync: `fsync`, which already flushes the device here.
    #[cfg(not(target_vendor = "apple"))]
    fn ordered(file: &File) -> std::io::Result<()> {
        fsync(file)
    }

    /// A full flush: `F_FULLFSYNC` on macOS, `sync_all` elsewhere.
    fn full_flush(file: &File) -> std::io::Result<()> {
        #[cfg(target_vendor = "apple")]
        fcntl(file, libc::F_FULLFSYNC)?;
        #[cfg(not(target_vendor = "apple"))]
        {
            #[cfg(test)]
            interruption::op(Op::SyncAll);
            file.sync_all()?;
        }
        bump(|c| c.full_flushes += 1);
        Ok(())
    }

    /// Flushes the cache of the device holding `dir` on macOS; nothing
    /// where `fsync` already flushes the device.
    #[cfg(target_vendor = "apple")]
    fn device_flush(dir: &Path) -> Result<(), ScaffoldError> {
        let handle = File::open(dir).map_err(|e| ScaffoldError::io(dir, e))?;
        full_flush(&handle).map_err(|e| ScaffoldError::io(dir, e))?;
        #[cfg(test)]
        interruption::flushed(dir);
        Ok(())
    }

    #[cfg(not(target_vendor = "apple"))]
    #[allow(clippy::unnecessary_wraps)]
    fn device_flush(_dir: &Path) -> Result<(), ScaffoldError> {
        Ok(())
    }

    /// The one place a sync `fcntl` reaches the kernel, so the tests can see
    /// which command ran.
    #[cfg(target_vendor = "apple")]
    fn fcntl(file: &File, command: libc::c_int) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        #[cfg(test)]
        {
            interruption::op(Op::Fcntl(command));
            if interruption::refused(command) {
                return Err(std::io::Error::from_raw_os_error(libc::ENOTSUP));
            }
        }
        // SAFETY: `F_BARRIERFSYNC` and `F_FULLFSYNC` take no argument and only
        // use the descriptor, which `file` keeps open for the call.
        if unsafe { libc::fcntl(file.as_raw_fd(), command) } == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(all(unix, not(target_vendor = "apple")))]
    fn fsync(file: &File) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        #[cfg(test)]
        interruption::op(Op::Fsync);
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
        #[cfg(test)]
        interruption::op(Op::Fsync);
        file.sync_all()
    }
}

/// Test-only observation and interruption points for the scaffold writes
/// (TSK-153): the sync calls that reached the kernel, the order of renames
/// and directory syncs, a stop before a rename (as an error, or as an abrupt
/// process abort), a refused barrier, and fake device identities.
#[cfg(test)]
pub(crate) mod interruption {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use super::ScaffoldError;

    /// A sync call as it reached the kernel. Each platform makes only its
    /// own calls, so the others are never constructed there.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[allow(dead_code)]
    pub(crate) enum Op {
        Fcntl(i32),
        Fsync,
        SyncAll,
    }

    /// A rename, a directory sync or a full flush of the device holding a
    /// directory, in the order they happened.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Event {
        Renamed(PathBuf, Vec<u8>),
        Synced(PathBuf),
        Flushed(PathBuf),
    }

    thread_local! {
        static STOP_AT: Cell<Option<usize>> = const { Cell::new(None) };
        static ABORT: Cell<bool> = const { Cell::new(false) };
        static INTENTS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
        static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
        static OPS: RefCell<Vec<Op>> = const { RefCell::new(Vec::new()) };
        static REFUSE: Cell<Option<i32>> = const { Cell::new(None) };
        static DEVICES: RefCell<Vec<(PathBuf, u64)>> = const { RefCell::new(Vec::new()) };
    }

    /// Stops the write after `completed` renames on this thread with an
    /// error; `None` lets every write through. Clears the logs.
    pub(crate) fn arm(completed: Option<usize>) {
        STOP_AT.with(|stop| stop.set(completed));
        ABORT.with(|abort| abort.set(false));
        INTENTS.with(|intents| *intents.borrow_mut() = None);
        EVENTS.with(|log| log.borrow_mut().clear());
        OPS.with(|log| log.borrow_mut().clear());
    }

    /// Aborts the process before the rename after `completed` renames,
    /// skipping every destructor and flush, as a killed process would.
    /// Each write's path and content digest is appended to `intents` before
    /// its rename.
    pub(crate) fn arm_abort(completed: usize, intents: &Path) {
        arm(Some(completed));
        ABORT.with(|abort| abort.set(true));
        INTENTS.with(|log| *log.borrow_mut() = Some(intents.to_path_buf()));
    }

    /// The renames since [`arm`], in order.
    pub(crate) fn completed() -> Vec<(PathBuf, Vec<u8>)> {
        events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Renamed(path, bytes) => Some((path, bytes)),
                Event::Synced(_) | Event::Flushed(_) => None,
            })
            .collect()
    }

    pub(crate) fn events() -> Vec<Event> {
        EVENTS.with(|log| log.borrow().clone())
    }

    pub(crate) fn ops() -> Vec<Op> {
        OPS.with(|log| log.borrow().clone())
    }

    /// Makes the kernel refuse `command` (as an unsupported file system
    /// would), or stops refusing with `None`.
    #[cfg(target_vendor = "apple")]
    pub(crate) fn refuse(command: Option<i32>) {
        REFUSE.with(|refuse| refuse.set(command));
    }

    /// Reports each directory under a listed prefix as on that device.
    pub(crate) fn fake_devices(devices: Vec<(PathBuf, u64)>) {
        DEVICES.with(|list| *list.borrow_mut() = devices);
    }

    pub(super) fn before_rename(path: &Path, bytes: &[u8]) -> Result<(), ScaffoldError> {
        if let Some(intents) = INTENTS.with(|log| log.borrow().clone()) {
            use std::io::Write;
            let line = format!(
                "{}\t{}\n",
                path.display(),
                crate::scaffold::hash::sha256_hex(bytes)
            );
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&intents)
                .map_err(|e| ScaffoldError::io(&intents, e))?;
            file.write_all(line.as_bytes())
                .map_err(|e| ScaffoldError::io(&intents, e))?;
        }
        let landed = completed().len();
        // Sticky, like a crash: once reached, no later write lands either.
        if STOP_AT.with(Cell::get).is_some_and(|stop| landed >= stop) {
            if ABORT.with(Cell::get) {
                std::process::abort();
            }
            return Err(ScaffoldError::io(
                path,
                std::io::Error::other("test interruption before rename"),
            ));
        }
        Ok(())
    }

    pub(super) fn renamed(path: &Path, bytes: &[u8]) {
        EVENTS.with(|log| {
            log.borrow_mut()
                .push(Event::Renamed(path.to_path_buf(), bytes.to_vec()));
        });
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(super) fn synced(dir: &Path) {
        EVENTS.with(|log| log.borrow_mut().push(Event::Synced(dir.to_path_buf())));
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(super) fn flushed(dir: &Path) {
        EVENTS.with(|log| log.borrow_mut().push(Event::Flushed(dir.to_path_buf())));
    }

    pub(super) fn op(op: Op) {
        OPS.with(|log| log.borrow_mut().push(op));
    }

    #[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
    pub(super) fn refused(command: i32) -> bool {
        REFUSE.with(Cell::get) == Some(command)
    }

    /// The fake device a `stat` of `path` would report: that of the path it
    /// resolves to, following links as `stat` does.
    #[cfg(target_vendor = "apple")]
    pub(super) fn device_stat(path: &Path) -> Option<u64> {
        let resolved = std::fs::canonicalize(path).ok()?;
        DEVICES.with(|list| {
            list.borrow()
                .iter()
                .map(|(prefix, device)| {
                    (
                        std::fs::canonicalize(prefix).unwrap_or_else(|_| prefix.clone()),
                        *device,
                    )
                })
                .filter(|(prefix, _)| resolved.starts_with(prefix))
                .max_by_key(|(prefix, _)| prefix.as_os_str().len())
                .map(|(_, device)| device)
        })
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(super) fn device(dir: &Path) -> Option<u64> {
        DEVICES.with(|list| {
            list.borrow()
                .iter()
                .filter(|(prefix, _)| dir.starts_with(prefix))
                .max_by_key(|(prefix, _)| prefix.as_os_str().len())
                .map(|(_, device)| *device)
        })
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
            release_rules: None,
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
    fn feedback_topics_default_once_and_keep_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(matches!(
            FeedbackConfig::write_defaults(root),
            Err(ScaffoldError::NotInitialized)
        ));
        let original =
            "# kept\nschema_version = 1\ntier = \"full\"\n\n[scaffold]\nignore = [\".codex/**\"]\n";
        write_file(&ProjectState::path(root), original.as_bytes()).unwrap();
        assert_eq!(FeedbackConfig::load(root).unwrap().topics, None);
        let (topics, written) = FeedbackConfig::write_defaults(root).unwrap();
        assert!(written);
        assert_eq!(topics, FeedbackConfig::DEFAULT_TOPICS.to_vec());
        let text = std::fs::read_to_string(ProjectState::path(root)).unwrap();
        assert!(text.starts_with(original), "{text}");
        assert_eq!(ScaffoldConfig::load(root).unwrap().ignore, [".codex/**"]);

        // A project's own list is kept and never rewritten.
        let own = format!("{original}\n[feedback]\ntopics = [\"ux\", \"process\"]\n");
        write_file(&ProjectState::path(root), own.as_bytes()).unwrap();
        let (topics, written) = FeedbackConfig::write_defaults(root).unwrap();
        assert!(!written);
        assert_eq!(topics, ["ux", "process"]);
        assert_eq!(
            std::fs::read_to_string(ProjectState::path(root)).unwrap(),
            own
        );

        // A `[feedback]` table without topics gains the key under its
        // header; comments and every other line keep their bytes.
        let partial = "# top\nschema_version = 1 # kept\n\n[feedback] # mine\nnote = \"x\"\n\n[scaffold]\n# why\nignore = []\n";
        write_file(&ProjectState::path(root), partial.as_bytes()).unwrap();
        let (topics, written) = FeedbackConfig::write_defaults(root).unwrap();
        assert!(written);
        assert_eq!(topics, FeedbackConfig::DEFAULT_TOPICS.to_vec());
        let text = std::fs::read_to_string(ProjectState::path(root)).unwrap();
        let (head, tail) = partial.split_at(partial.find("note").unwrap());
        assert!(text.starts_with(head), "{text}");
        assert!(text.ends_with(tail), "{text}");
        assert!(
            text[head.len()..].starts_with("topics = [\n    \"process\",\n"),
            "{text}"
        );

        // `feedback` set without a header line is refused, the file untouched.
        let dotted = "schema_version = 1\nfeedback.note = \"x\"\n";
        write_file(&ProjectState::path(root), dotted.as_bytes()).unwrap();
        assert!(FeedbackConfig::write_defaults(root).is_err());
        assert_eq!(
            std::fs::read_to_string(ProjectState::path(root)).unwrap(),
            dotted
        );

        write_file(
            &ProjectState::path(root),
            b"schema_version = 1\n[feedback]\ntopics = [\"two words\"]\n",
        )
        .unwrap();
        let error = FeedbackConfig::load(root).unwrap_err().to_string();
        assert!(error.contains("feedback.topics[0]"), "{error}");
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
            release_rules: None,
        };
        let error = state.store(dir.path()).unwrap_err().to_string();
        // The error names the file by its native path.
        assert!(error.contains("project.toml"), "{error}");
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

    /// Adding the default feedback topics is a scaffold record write: it is
    /// flushed like any single write (content and directory, in full) and
    /// the state file keeps its permissions.
    #[test]
    fn feedback_topics_are_written_durably_and_keep_the_file_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(&ProjectState::path(root), b"schema_version = 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                ProjectState::path(root),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        let before = sync_counts();
        let (_, written) = FeedbackConfig::write_defaults(root).unwrap();
        let after = sync_counts();
        assert!(written);
        assert_eq!(after.files_written - before.files_written, 1);
        let directory = usize::from(cfg!(unix));
        assert_eq!(after.directory_syncs - before.directory_syncs, directory);
        assert_eq!(after.full_flushes - before.full_flushes, 1 + directory);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(ProjectState::path(root))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

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
        // Directory sync is a Unix call; Windows syncs the content only.
        let directory = usize::from(cfg!(unix));
        assert_eq!(after.directory_syncs - before.directory_syncs, directory);
        assert_eq!(
            after.full_flushes - before.full_flushes,
            1 + directory,
            "the content and the directory, each in full"
        );
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
            mid.directory_syncs, before.directory_syncs,
            "deferred to the end"
        );
        assert_eq!(
            mid.full_flushes, before.full_flushes,
            "no full flush per file"
        );
        batch.finish().unwrap();
        let after = sync_counts();
        assert_eq!(
            after.directory_syncs - before.directory_syncs,
            2 * usize::from(cfg!(unix)),
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
        assert_eq!(
            after.directory_syncs - before.directory_syncs,
            usize::from(cfg!(unix))
        );
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
        let skills = Path::new(".agents").join("skills");
        for (rel, bytes) in snapshot(root) {
            // By component, so Windows `\` separators match too.
            if rel.starts_with(&skills) && rel.file_name() == Some("SKILL.md".as_ref()) {
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
        let directories = after.directory_syncs - before.directory_syncs;
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
            directories <= written && (directories > 0 || cfg!(not(unix))),
            "{directories} of {written}"
        );
        assert_eq!(
            after.full_flushes - before.full_flushes,
            device,
            "one per device, and the run touched one"
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
            after.directory_syncs - before.directory_syncs
        );
    }

    /// After a run stops at an arbitrary write, every file that existed
    /// before is still there, and each file holds its content from before the
    /// run or the content of a write that completed: never torn, never empty.
    fn assert_whole_files(
        root: &Path,
        before: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
        stop: usize,
    ) {
        let completed = interruption::completed();
        let now = snapshot(root);
        for rel in before.keys() {
            assert!(
                now.contains_key(rel),
                "stop {stop}: {} existed before the run and is gone",
                rel.display()
            );
        }
        for (rel, bytes) in now {
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
    fn an_init_or_update_stopped_by_an_error_leaves_no_torn_or_empty_file() {
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

    // TSK-153 review round 1: the sync calls that reach the kernel, record
    // ordering, every device, and an abrupt process abort.

    /// The ordered sync as it reaches the kernel on this platform.
    fn ordered_op() -> interruption::Op {
        #[cfg(target_vendor = "apple")]
        return interruption::Op::Fcntl(libc::F_BARRIERFSYNC);
        #[cfg(not(target_vendor = "apple"))]
        return interruption::Op::Fsync;
    }

    /// A device flush as it reaches the kernel, where the platform needs one.
    fn device_flush_ops() -> Vec<interruption::Op> {
        #[cfg(target_vendor = "apple")]
        return vec![interruption::Op::Fcntl(libc::F_FULLFSYNC)];
        #[cfg(not(target_vendor = "apple"))]
        return Vec::new();
    }

    #[test]
    fn a_run_reaches_the_kernel_only_with_ordered_syncs_and_one_flush() {
        // A plain `fsync` on macOS, or a full flush per file, fails here.
        let dir = tempfile::tempdir().unwrap();
        interruption::arm(None);
        let batch = SyncBatch::begin();
        write_file(&dir.path().join("a/1"), b"1").unwrap();
        write_file(&dir.path().join("a/2"), b"2").unwrap();
        Baseline::write(dir.path(), "a/1", "1").unwrap();
        batch.finish().unwrap();
        let ops = interruption::ops();
        let (syncs, last) = ops.split_at(ops.len() - device_flush_ops().len());
        assert_eq!(last, device_flush_ops().as_slice());
        // 3 file contents, then on Unix `a` before the record and
        // `.codeflow/.baseline/a` at the end.
        let directories = 2 * usize::from(cfg!(unix));
        assert_eq!(
            syncs,
            vec![ordered_op(); 3 + directories].as_slice(),
            "{ops:?}"
        );
        interruption::arm(None);
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_refused_barrier_falls_back_to_a_full_flush_never_fsync() {
        let dir = tempfile::tempdir().unwrap();
        interruption::arm(None);
        interruption::refuse(Some(libc::F_BARRIERFSYNC));
        let batch = SyncBatch::begin();
        write_file(&dir.path().join("one.txt"), b"one").unwrap();
        let result = batch.finish();
        interruption::refuse(None);
        result.unwrap();
        let barrier = interruption::Op::Fcntl(libc::F_BARRIERFSYNC);
        let full = interruption::Op::Fcntl(libc::F_FULLFSYNC);
        assert_eq!(
            interruption::ops(),
            vec![barrier, full, barrier, full, full],
            "content and directory each fall back; then the device flush"
        );
        interruption::arm(None);
    }

    #[test]
    fn each_device_a_run_touched_is_flushed() {
        let dir = tempfile::tempdir().unwrap();
        interruption::arm(None);
        interruption::fake_devices(vec![
            (dir.path().join("project"), 1),
            (dir.path().join("report"), 2),
        ]);
        let before = sync_counts();
        let batch = SyncBatch::begin();
        write_file(&dir.path().join("project/a.txt"), b"a").unwrap();
        write_file(&dir.path().join("project/b/c.txt"), b"c").unwrap();
        write_file(&dir.path().join("report/diff.txt"), b"d").unwrap();
        let result = batch.finish();
        interruption::fake_devices(Vec::new());
        result.unwrap();
        let flushes = interruption::ops()
            .into_iter()
            .filter(|op| device_flush_ops().contains(op))
            .count();
        assert_eq!(flushes, 2 * device_flush_ops().len(), "one per device");
        assert_eq!(
            sync_counts().full_flushes - before.full_flushes,
            2 * device_flush_ops().len()
        );
        interruption::arm(None);
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_record_is_on_its_directory_device_whatever_its_leaf() {
        // T153-R3-1: the write replaces a link beside it, so the record lands
        // on its directory's device, never on the old link target's.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let other = dir.path().join("other");
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(root.join(".codeflow/present.json"), "{}").unwrap();
        std::fs::write(other.join("old.json"), "{}").unwrap();
        std::os::unix::fs::symlink(other.join("old.json"), root.join(".codeflow/link.json"))
            .unwrap();
        interruption::fake_devices(vec![(root.clone(), 1), (other.clone(), 2)]);
        let flushes = || {
            interruption::events()
                .iter()
                .filter(|event| matches!(event, interruption::Event::Flushed(p) if *p == other))
                .count()
        };
        for (case, rel) in [
            ("an existing record", ".codeflow/present.json"),
            ("a missing record", ".codeflow/new/record.json"),
            ("a leaf link to the other device", ".codeflow/link.json"),
        ] {
            interruption::arm(None);
            let batch = SyncBatch::begin();
            write_file(&other.join("payload"), case.as_bytes()).unwrap();
            sync::settle_before(&root.join(rel)).unwrap();
            assert_eq!(flushes(), 1, "{case}: the other device is flushed first");
            batch.finish().unwrap();
        }
        // Flushed, a device stays clean until it is written again.
        interruption::arm(None);
        let batch = SyncBatch::begin();
        write_file(&other.join("payload"), b"one").unwrap();
        write_record(&root, ".codeflow/present.json", b"1").unwrap();
        write_record(&root, ".codeflow/present.json", b"2").unwrap();
        assert_eq!(
            flushes(),
            1,
            "a same-device record after the flush adds none"
        );
        write_file(&other.join("payload"), b"two").unwrap();
        write_record(&root, ".codeflow/present.json", b"3").unwrap();
        assert_eq!(
            flushes(),
            2,
            "a new write on the other device needs a new flush"
        );
        batch.finish().unwrap();
        interruption::fake_devices(Vec::new());
        interruption::arm(None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_state_record_is_refused_and_left_alone() {
        // T153-R3-1: every state record refuses a link, as baselines did.
        type Store<'a> = Box<dyn Fn(&Path) -> Result<(), ScaffoldError> + 'a>;
        let state = ProjectState {
            schema_version: 1,
            tier: Tier::Standard,
            scaffold_version: "2.0.0".to_string(),
            stack: "rust".to_string(),
            areas: Vec::new(),
            policy_armed: true,
            git_hooks: GIT_HOOKS_WIRED.to_string(),
            permission_preset: "default".to_string(),
            product_one_liner: "demo".to_string(),
            release_rules: None,
        };
        let records: [(&str, Store); 3] = [
            (
                INSTALLED_MANIFEST,
                Box::new(|root| InstalledManifest::new("3.0.0").store(root)),
            ),
            (PROJECT_TOML, Box::new(|root| state.store(root))),
            (
                ".codeflow/.baseline/a.txt",
                Box::new(|root| Baseline::write(root, "a.txt", "x")),
            ),
        ];
        for (rel, store) in records {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("project");
            let leaf = root.join(rel);
            std::fs::create_dir_all(leaf.parent().unwrap()).unwrap();
            let outside = dir.path().join("outside");
            std::fs::write(&outside, "outside\n").unwrap();
            std::os::unix::fs::symlink(&outside, &leaf).unwrap();
            let refused = store(&root);
            assert!(
                matches!(refused, Err(ScaffoldError::UnsafeSymlink { .. })),
                "{rel}: {refused:?}"
            );
            assert!(
                leaf.symlink_metadata().unwrap().file_type().is_symlink(),
                "{rel}"
            );
            assert_eq!(
                std::fs::read_to_string(&outside).unwrap(),
                "outside\n",
                "{rel}"
            );
        }
    }

    #[cfg(unix)]
    /// A state record: a baseline copy, the manifest or `project.toml`.
    fn is_record(root: &Path, path: &Path) -> bool {
        path.starts_with(root.join(BASELINE_DIR))
            || path == InstalledManifest::path(root)
            || path == ProjectState::path(root)
    }

    #[cfg(unix)]
    /// Whether a device keeps its own write cache, so an ordered sync
    /// orders writes on that device only: true for the macOS barrier; a
    /// Linux `fsync` is durable when it returns.
    const INDEPENDENT_CACHES: bool = cfg!(target_vendor = "apple");

    #[cfg(unix)]
    /// Every rename a state record follows has its directory synced before
    /// the record's rename, and, where devices cache independently, a
    /// rename on another device than the record's also has its device
    /// flushed first.
    fn assert_records_follow_durable_files(root: &Path, device: &dyn Fn(&Path) -> u64) {
        // Renames whose directory has not been synced since, in order.
        let mut unsynced: Vec<PathBuf> = Vec::new();
        // Per device, a synced rename whose device has not been flushed since.
        let mut unflushed = std::collections::BTreeMap::new();
        for event in interruption::events() {
            match event {
                interruption::Event::Synced(dir) => {
                    let (now, rest) = std::mem::take(&mut unsynced)
                        .into_iter()
                        .partition::<Vec<_>, _>(|file| file.parent() == Some(dir.as_path()));
                    unsynced = rest;
                    if INDEPENDENT_CACHES {
                        for file in now {
                            unflushed.entry(device(&file)).or_insert(file);
                        }
                    }
                }
                interruption::Event::Flushed(dir) => {
                    unflushed.remove(&device(&dir));
                }
                interruption::Event::Renamed(file, _) => {
                    if is_record(root, &file) {
                        assert!(
                            unsynced.is_empty(),
                            "{} reached the disk ahead of {}'s directory",
                            file.display(),
                            unsynced[0].display()
                        );
                        let own = device(&file);
                        if let Some((_, ahead)) = unflushed.iter().find(|(d, _)| **d != own) {
                            panic!(
                                "{} reached its device ahead of {}'s device flush",
                                file.display(),
                                ahead.display()
                            );
                        }
                    }
                    unsynced.push(file);
                }
            }
        }
    }

    #[cfg(unix)]
    /// A device map with `second`, when given, on device 2 and all else on 1.
    fn two_devices(second: Option<PathBuf>) -> impl Fn(&Path) -> u64 {
        move |path| match &second {
            Some(second) if path.starts_with(second) => 2,
            _ => 1,
        }
    }

    // Directory sync is a Unix call, so the ordering is a Unix guarantee.
    #[cfg(unix)]
    #[test]
    fn a_record_never_reaches_the_disk_ahead_of_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        interruption::arm(None);
        crate::scaffold::init(&shipped_assets(), &root, &init_opts(Tier::Standard)).unwrap();
        assert_records_follow_durable_files(&root, &two_devices(None));
        assert!(edit_managed_skills(&root) > 5);
        interruption::arm(None);
        crate::scaffold::update(&shipped_assets(), &root, &force_update()).unwrap();
        assert_records_follow_durable_files(&root, &two_devices(None));
        interruption::arm(None);
    }

    #[cfg(unix)]
    #[test]
    fn a_record_never_reaches_its_drive_ahead_of_files_on_another() {
        // T153-R2-1: a barrier on one drive orders nothing on another.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let skills = root.join(".agents");
        let device = two_devices(Some(skills.clone()));
        interruption::fake_devices(vec![(root.clone(), 1), (skills, 2)]);
        interruption::arm(None);
        crate::scaffold::init(&shipped_assets(), &root, &init_opts(Tier::Standard)).unwrap();
        assert_records_follow_durable_files(&root, &device);
        assert!(edit_managed_skills(&root) > 5);
        interruption::arm(None);
        crate::scaffold::update(&shipped_assets(), &root, &force_update()).unwrap();
        assert_records_follow_durable_files(&root, &device);
        interruption::fake_devices(Vec::new());
        interruption::arm(None);
    }

    #[cfg(unix)]
    /// The shipped assets plus one managed file, `zzz/payload.txt`, whose
    /// shipped content is `body`.
    fn assets_with_payload(dir: &Path, body: &str) -> crate::scaffold::DirSource {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        copy_tree(&assets, dir);
        let manifest = dir.join("base/scaffold-manifest.toml");
        let mut text = std::fs::read_to_string(&manifest).unwrap();
        text.push_str(
            "\n[[entry]]\nsrc = \"payload\"\ndest = \"zzz/payload.txt\"\n\
             ownership = \"managed\"\ntiers = [\"standard\"]\n",
        );
        std::fs::write(&manifest, text).unwrap();
        std::fs::write(dir.join("base/payload"), body).unwrap();
        crate::scaffold::DirSource::new(dir.to_path_buf())
    }

    #[cfg(unix)]
    /// The worst state a crash after `events` may leave: every state record,
    /// every rename whose directory was synced durably, and no other
    /// rename. Where devices cache independently, a sync is durable only once
    /// its device is flushed or a kept record on the same device follows it,
    /// since the barrier orders that device's writes.
    fn crash_state(
        root: &Path,
        before: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
        events: &[interruption::Event],
        device: &dyn Fn(&Path) -> u64,
    ) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        // Scanning backwards: what happens after the current event.
        let mut durable_dirs = std::collections::BTreeSet::new();
        let mut flushed_later = std::collections::BTreeSet::new();
        let mut record_later = std::collections::BTreeSet::new();
        let mut kept = Vec::new();
        for event in events.iter().rev() {
            match event {
                interruption::Event::Flushed(dir) => {
                    flushed_later.insert(device(dir));
                }
                interruption::Event::Synced(dir) => {
                    let on = device(dir);
                    if !INDEPENDENT_CACHES
                        || flushed_later.contains(&on)
                        || record_later.contains(&on)
                    {
                        durable_dirs.insert(dir.clone());
                    }
                }
                interruption::Event::Renamed(path, bytes) => {
                    if is_record(root, path) {
                        record_later.insert(device(path));
                        kept.push((path, bytes));
                    } else if durable_dirs.contains(path.parent().unwrap()) {
                        kept.push((path, bytes));
                    }
                }
            }
        }
        let mut state = before.clone();
        for (path, bytes) in kept.into_iter().rev() {
            state.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                bytes.clone(),
            );
        }
        state
    }

    #[cfg(unix)]
    #[test]
    fn an_update_after_a_crash_at_any_point_installs_the_new_version() {
        // T153-1: a record ahead of its file made the next update keep the
        // old shipped file as a user edit.
        assert_update_recovers_from_any_crash(false);
    }

    #[cfg(unix)]
    #[test]
    fn a_crash_with_the_file_on_another_drive_still_installs_the_new_version() {
        // T153-R2-1: the payload's drive caches apart from its records'.
        assert_update_recovers_from_any_crash(true);
    }

    #[cfg(unix)]
    /// Crashes an upgrade just before and after each write that concerns
    /// `zzz/payload.txt`, and each device flush, and checks that the next
    /// update installs the new version. With `second_drive`, `zzz/` is on a
    /// device of its own.
    fn assert_update_recovers_from_any_crash(second_drive: bool) {
        let dir = tempfile::tempdir().unwrap();
        let old = assets_with_payload(&dir.path().join("old"), "old shipped data\n");
        let new = assets_with_payload(&dir.path().join("new"), "new shipped data\n");
        let upgrade = crate::scaffold::UpdateOptions {
            force: false,
            binary_version: "9.9.9".to_string(),
            diff_out: None,
        };
        let base = dir.path().join("base");
        crate::scaffold::init(&old, &base, &init_opts(Tier::Standard)).unwrap();
        let before = snapshot(&base);
        let probe = dir.path().join("probe");
        copy_tree(&base, &probe);
        if second_drive {
            interruption::fake_devices(vec![(probe.clone(), 1), (probe.join("zzz"), 2)]);
        }
        interruption::arm(None);
        crate::scaffold::update(&new, &probe, &upgrade).unwrap();
        interruption::fake_devices(Vec::new());
        let rebase = |path: PathBuf| base.join(path.strip_prefix(&probe).unwrap());
        let events: Vec<_> = interruption::events()
            .into_iter()
            .map(|event| match event {
                interruption::Event::Renamed(path, bytes) => {
                    interruption::Event::Renamed(rebase(path), bytes)
                }
                interruption::Event::Synced(path) => interruption::Event::Synced(rebase(path)),
                interruption::Event::Flushed(path) => interruption::Event::Flushed(rebase(path)),
            })
            .collect();
        interruption::arm(None);
        let device = two_devices(second_drive.then(|| base.join("zzz")));
        let expected = snapshot(&probe);
        assert_eq!(
            expected[Path::new("zzz/payload.txt")],
            b"new shipped data\n".to_vec()
        );
        // The update rewrites every managed file; crash just before and just
        // after each write that concerns the payload (the file, its baseline,
        // the manifest and the project state) and each device flush.
        let watched = [
            base.join("zzz/payload.txt"),
            Baseline::path(&base, "zzz/payload.txt"),
            InstalledManifest::path(&base),
            ProjectState::path(&base),
        ];
        let mut points = std::collections::BTreeSet::from([0, events.len()]);
        for (at, event) in events.iter().enumerate() {
            let concerns = match event {
                interruption::Event::Renamed(path, _) => watched.contains(path),
                interruption::Event::Flushed(_) => true,
                interruption::Event::Synced(_) => false,
            };
            if concerns {
                points.extend([at, at + 1]);
            }
        }
        assert!(points.len() >= 8, "{points:?}");
        for point in points {
            let root = dir.path().join(format!("crash-{point}"));
            copy_tree(&base, &root);
            for (rel, bytes) in crash_state(&base, &before, &events[..point], &device) {
                std::fs::write(root.join(rel), bytes).unwrap();
            }
            crate::scaffold::update(&new, &root, &upgrade).unwrap();
            assert_eq!(
                snapshot(&root)[Path::new("zzz/payload.txt")],
                b"new shipped data\n".to_vec(),
                "a crash after event {point} of {} keeps the old file",
                events.len()
            );
        }
    }

    const CHILD: &str = "CODEFLOW_TSK153_ABORT_CHILD";

    #[test]
    fn an_update_killed_mid_run_leaves_every_file_whole() {
        // An abrupt abort skips every destructor and deferred flush.
        if let Ok(spec) = std::env::var(CHILD) {
            let mut parts = spec.split('\n');
            let root = PathBuf::from(parts.next().unwrap());
            let stop: usize = parts.next().unwrap().parse().unwrap();
            let intents = PathBuf::from(parts.next().unwrap());
            interruption::arm_abort(stop, &intents);
            let _ = crate::scaffold::update(&shipped_assets(), &root, &force_update());
            std::process::exit(3); // not reached when the abort fires
        }
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        crate::scaffold::init(&shipped_assets(), &base, &init_opts(Tier::Standard)).unwrap();
        assert!(edit_managed_skills(&base) > 5);
        let before = snapshot(&base);
        copy_tree(&base, &dir.path().join("probe"));
        interruption::arm(None);
        crate::scaffold::update(
            &shipped_assets(),
            &dir.path().join("probe"),
            &force_update(),
        )
        .unwrap();
        let total = interruption::completed().len();
        interruption::arm(None);
        for stop in [0, total / 2, total - 1] {
            let root = dir.path().join(format!("killed-{stop}"));
            copy_tree(&base, &root);
            let intents = dir.path().join(format!("intents-{stop}"));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "scaffold::state::tests::an_update_killed_mid_run_leaves_every_file_whole",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(
                    CHILD,
                    format!("{}\n{stop}\n{}", root.display(), intents.display()),
                )
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                assert_eq!(
                    status.signal(),
                    Some(libc::SIGABRT),
                    "stop {stop}: {status}"
                );
            }
            #[cfg(not(unix))]
            assert!(!status.success(), "stop {stop}: {status}");
            let intended: Vec<(PathBuf, String)> = std::fs::read_to_string(&intents)
                .unwrap()
                .lines()
                .map(|line| {
                    let (path, digest) = line.split_once('\t').unwrap();
                    (PathBuf::from(path), digest.to_string())
                })
                .collect();
            assert_eq!(
                intended.len(),
                stop + 1,
                "the child stopped at write {stop}"
            );
            let now = snapshot(&root);
            for rel in before.keys() {
                assert!(
                    now.contains_key(rel),
                    "stop {stop}: {} is gone",
                    rel.display()
                );
            }
            for (rel, bytes) in now {
                let hidden = rel.file_name().unwrap().to_string_lossy().starts_with('.');
                if hidden && rel.extension().is_some_and(|ext| ext == "tmp") {
                    continue; // the stopped write's temp copy, never a managed path
                }
                let digest = crate::scaffold::hash::sha256_hex(&bytes);
                let landed = intended.iter().any(|(path, sum)| {
                    path.strip_prefix(&root).is_ok_and(|r| r == rel) && *sum == digest
                });
                let kept = before.get(&rel).is_some_and(|old| *old == bytes);
                assert!(
                    landed || kept,
                    "stop {stop}: {} is neither its earlier content nor an intended write",
                    rel.display()
                );
            }
        }
    }
}
