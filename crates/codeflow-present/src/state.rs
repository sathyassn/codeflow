use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    config::{ProjectConfig, RetentionPolicy, UtilityTokens},
    document::{ParsedDocument, PresentationDocument, Provenance},
    error::{PresentError, Result},
    limits,
    platform::is_link_like,
};

#[cfg(not(windows))]
use crate::platform::{harden_private_file, harden_private_path};

const STATE_SCHEMA_VERSION: u32 = 1;
const UPDATE_MARKER: &str = ".updating.json";
const PROJECT_MUTATION_LOCK: &str = ".create.lock";
const CREATE_TRANSACTION_MARKER: &str = ".creating.json";
const DELETE_TRANSACTION_MARKER: &str = ".deleting";
pub(crate) const LAUNCH_RECOVERY_PREFIX: &str = "launch-recovery-";
const CONTROL_MUTATION_RESERVE_BYTES: u64 = limits::MAX_SESSION_STATE_BYTES;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateMarker {
    from_revision: u64,
    to_revision: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CreateTransactionMarker {
    schema_version: u32,
    session_id: Uuid,
    nonce: Uuid,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DeleteTransactionMarker {
    schema_version: u32,
    session_id: Uuid,
    nonce: Uuid,
}

enum ClearOutcome {
    Removed,
    Retained(&'static str),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionRecord {
    pub state_schema_version: u32,
    pub id: Uuid,
    pub project_key: String,
    pub title: String,
    pub status: SessionStatus,
    pub current_revision: u64,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_instance: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_instance: Option<Uuid>,
    #[serde(default)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevisionContent {
    Supported { document: PresentationDocument },
    Unsupported { schema_version: u32, raw: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RevisionRecord {
    pub state_schema_version: u32,
    pub revision: u64,
    pub created_at_unix: u64,
    pub content: RevisionContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionHistory {
    pub schema_version: u32,
    pub session: HistorySession,
    pub revisions: Vec<RevisionRecord>,
    pub feedback_events: Vec<FeedbackEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistorySession {
    pub id: Uuid,
    pub title: String,
    pub status: SessionStatus,
    pub current_revision: u64,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub closed_at_unix: Option<u64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackVerdict {
    Approve,
    ApproveWithNotes,
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    Comment,
    Question,
    Decision,
    Suggestion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextSelector {
    pub exact: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    pub start_utf16: u32,
    pub end_utf16: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedbackNote {
    pub id: Uuid,
    pub block_id: String,
    pub block_label: String,
    pub kind: FeedbackKind,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<TextSelector>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedbackEnvelope {
    pub event_id: Uuid,
    pub session_id: Uuid,
    pub revision: u64,
    pub actor: String,
    pub verdict: FeedbackVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default)]
    pub notes: Vec<FeedbackNote>,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackAppend {
    pub sequence: u64,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackResolution {
    Addressed,
    Dismissed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackLifecycle {
    Received,
    Delivered,
    Addressed,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FeedbackAnchor {
    Block { block_id: String },
    Anchored { start_utf16: u32, end_utf16: u32 },
    Reanchored { start_utf16: u32, end_utf16: u32 },
    Orphaned { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedbackNoteView {
    pub id: Uuid,
    pub block_label: String,
    pub kind: FeedbackKind,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    pub anchor: FeedbackAnchor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedbackView {
    pub event_id: Uuid,
    pub source_revision: u64,
    pub event_version: u64,
    pub lifecycle: FeedbackLifecycle,
    pub verdict: FeedbackVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    pub notes: Vec<FeedbackNoteView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedbackSnapshot {
    pub items: Vec<FeedbackView>,
    pub omitted_older: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeedbackEvent {
    Received {
        sequence: u64,
        envelope: FeedbackEnvelope,
    },
    Delivered {
        sequence: u64,
        event_id: Uuid,
        at_unix: u64,
    },
    Addressed {
        sequence: u64,
        event_id: Uuid,
        at_unix: u64,
    },
    Dismissed {
        sequence: u64,
        event_id: Uuid,
        at_unix: u64,
    },
}

impl FeedbackEvent {
    fn sequence(&self) -> u64 {
        match self {
            Self::Received { sequence, .. }
            | Self::Delivered { sequence, .. }
            | Self::Addressed { sequence, .. }
            | Self::Dismissed { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeedbackLedgerState {
    lifecycle: FeedbackLifecycle,
    version: u64,
    delivered_version: Option<u64>,
    resolution: Option<FeedbackResolution>,
}

#[derive(Debug, Clone, Default)]
struct FeedbackLedger {
    states: std::collections::HashMap<Uuid, FeedbackLedgerState>,
    next_sequence: u64,
}

impl FeedbackLedger {
    fn replay(events: &[FeedbackEvent]) -> Result<Self> {
        let mut ledger = Self {
            states: std::collections::HashMap::new(),
            next_sequence: 1,
        };
        for event in events {
            if event.sequence() != ledger.next_sequence {
                return Err(PresentError::CorruptState(
                    "feedback log has a non-contiguous event sequence".to_string(),
                ));
            }
            ledger.apply(event)?;
            ledger.next_sequence = ledger
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| PresentError::CorruptState("event sequence overflow".to_string()))?;
        }
        Ok(ledger)
    }

    fn apply(&mut self, event: &FeedbackEvent) -> Result<()> {
        match event {
            FeedbackEvent::Received { sequence, envelope } => {
                if self.states.contains_key(&envelope.event_id) {
                    return Err(PresentError::CorruptState(format!(
                        "feedback event {} was received more than once",
                        envelope.event_id
                    )));
                }
                self.states.insert(
                    envelope.event_id,
                    FeedbackLedgerState {
                        lifecycle: FeedbackLifecycle::Received,
                        version: *sequence,
                        delivered_version: None,
                        resolution: None,
                    },
                );
            }
            FeedbackEvent::Delivered {
                sequence, event_id, ..
            } => {
                let state = self.states.get_mut(event_id).ok_or_else(|| {
                    PresentError::CorruptState(format!(
                        "feedback event {event_id} was delivered before it was received"
                    ))
                })?;
                if state.lifecycle != FeedbackLifecycle::Received {
                    return Err(PresentError::CorruptState(format!(
                        "feedback event {event_id} has an invalid duplicate or post-terminal delivery"
                    )));
                }
                state.lifecycle = FeedbackLifecycle::Delivered;
                state.version = *sequence;
                state.delivered_version = Some(*sequence);
            }
            FeedbackEvent::Addressed {
                sequence, event_id, ..
            }
            | FeedbackEvent::Dismissed {
                sequence, event_id, ..
            } => {
                let resolution = if matches!(event, FeedbackEvent::Addressed { .. }) {
                    FeedbackResolution::Addressed
                } else {
                    FeedbackResolution::Dismissed
                };
                let state = self.states.get_mut(event_id).ok_or_else(|| {
                    PresentError::CorruptState(format!(
                        "feedback event {event_id} was resolved before it was received"
                    ))
                })?;
                if state.lifecycle != FeedbackLifecycle::Delivered {
                    return Err(PresentError::CorruptState(format!(
                        "feedback event {event_id} has an invalid resolution transition"
                    )));
                }
                state.lifecycle = match resolution {
                    FeedbackResolution::Addressed => FeedbackLifecycle::Addressed,
                    FeedbackResolution::Dismissed => FeedbackLifecycle::Dismissed,
                };
                state.version = *sequence;
                state.resolution = Some(resolution);
            }
        }
        Ok(())
    }

    fn state(&self, event_id: Uuid) -> Option<FeedbackLedgerState> {
        self.states.get(&event_id).copied()
    }
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
    runtime_root: PathBuf,
    project_key: String,
    project_root: PathBuf,
    config: ProjectConfig,
    retention: RetentionPolicy,
}

pub(crate) struct RuntimeControlLease {
    _project: File,
    _session: File,
    _runtime: File,
}

static ACTIVE_RUNTIME_LEASES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

/// Holds both the process-local reservation and the cross-process file lock for
/// one presentation runtime role.
pub struct RuntimeLease {
    file: Option<File>,
    path: PathBuf,
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        // Keep the process-local reservation until the OS handle has released
        // its cross-process lock. A contender can otherwise observe a brief
        // unlocked registry entry while the file is still locked.
        drop(self.file.take());
        if let Ok(mut active) = active_runtime_leases().lock() {
            active.remove(&self.path);
        }
    }
}

fn active_runtime_leases() -> &'static Mutex<HashSet<PathBuf>> {
    ACTIVE_RUNTIME_LEASES.get_or_init(|| Mutex::new(HashSet::new()))
}

fn reserve_runtime_lease(path: &Path) -> Result<bool> {
    active_runtime_leases()
        .lock()
        .map(|mut active| active.insert(path.to_path_buf()))
        .map_err(|_| {
            PresentError::ServiceUnavailable(
                "the process-local presentation lease registry is unavailable".to_string(),
            )
        })
}

fn release_runtime_lease(path: &Path) {
    if let Ok(mut active) = active_runtime_leases().lock() {
        active.remove(path);
    }
}

fn try_runtime_lease(path: &Path) -> Result<Option<RuntimeLease>> {
    if !reserve_runtime_lease(path)? {
        return Ok(None);
    }
    let lease = match open_private_append(path) {
        Ok(lease) => lease,
        Err(error) => {
            release_runtime_lease(path);
            return Err(error);
        }
    };
    match lease.try_lock_exclusive() {
        Ok(()) => Ok(Some(RuntimeLease {
            file: Some(lease),
            path: path.to_path_buf(),
        })),
        Err(error) => {
            release_runtime_lease(path);
            if error.kind() == std::io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(PresentError::io(path, error))
            }
        }
    }
}

impl SessionStore {
    pub fn discover(project: &Path) -> Result<Self> {
        let repository = git2::Repository::discover(project).map_err(|error| {
            PresentError::InvalidDocument(format!("not a Git repository: {error}"))
        })?;
        let common = repository
            .commondir()
            .canonicalize()
            .map_err(|error| PresentError::io(repository.commondir(), error))?;
        let project_key = hex_digest(&canonical_path_identity(&common));
        let worktree = repository.workdir().ok_or_else(|| {
            PresentError::InvalidDocument(
                "cf-present requires a non-bare Git working tree".to_string(),
            )
        })?;
        let project_root = worktree
            .canonicalize()
            .map_err(|error| PresentError::io(worktree, error))?;
        let config = ProjectConfig::load(&project_root)?;
        let state_root = platform_state_root()?;
        let root = state_root.join("projects").join(&project_key);
        let runtime_root = state_root
            .join("runtime")
            .join("projects")
            .join(&project_key);
        create_private_dir_all(&root)?;
        create_private_dir_all(&runtime_root)?;
        ensure_safe_dir(&root)?;
        ensure_safe_dir(&runtime_root)?;
        Ok(Self {
            root,
            runtime_root,
            project_key,
            project_root,
            retention: config.retention.clone(),
            config,
        })
    }

    #[cfg(test)]
    pub(crate) fn at_root(root: PathBuf, project_key: String) -> Result<Self> {
        create_private_dir_all(&root)?;
        let runtime_root = root
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{project_key}-runtime"));
        create_private_dir_all(&runtime_root)?;
        let config = ProjectConfig::default();
        Ok(Self {
            project_root: root.clone(),
            root,
            runtime_root,
            project_key,
            retention: config.retention.clone(),
            config,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn utility_tokens(&self) -> Result<Option<UtilityTokens>> {
        self.config.load_tokens(&self.project_root)
    }

    pub fn runtime_dir(&self, id: Uuid) -> Result<PathBuf> {
        let _project_lease = self.prepare_control_mutation()?;
        let _session_lock = self.lock_session(id)?;
        self.load(id)?;
        let path = self.runtime_root.join(id.to_string());
        create_private_dir_all(&path)?;
        Ok(path)
    }

    pub(crate) fn prepare_runtime_control_mutation(
        &self,
        id: Uuid,
        additional_bytes: u64,
    ) -> Result<RuntimeControlLease> {
        let project = self.prepare_control_mutation()?;
        let session = self.lock_session(id)?;
        self.load(id)?;
        let runtime = self.runtime_root.join(id.to_string());
        create_private_dir_all(&runtime)?;
        let control = runtime.join("control");
        create_private_dir_all(&control)?;
        let lock_path = control.join(".lock");
        let runtime_lock = open_private_append(&lock_path)?;
        runtime_lock
            .lock_exclusive()
            .map_err(|error| PresentError::io(&lock_path, error))?;
        let current = directory_size_bounded(&control, limits::MAX_RUNTIME_CONTROL_BYTES)?;
        if current
            .checked_add(additional_bytes)
            .is_none_or(|projected| projected > limits::MAX_RUNTIME_CONTROL_BYTES)
        {
            return Err(PresentError::ServiceUnavailable(format!(
                "presentation runtime control requires {additional_bytes} bytes of headroom under its {} byte bound",
                limits::MAX_RUNTIME_CONTROL_BYTES
            )));
        }
        Ok(RuntimeControlLease {
            _project: project,
            _session: session,
            _runtime: runtime_lock,
        })
    }

    pub fn create(&self, parsed: ParsedDocument) -> Result<SessionRecord> {
        let id = Uuid::new_v4();
        let sessions_root = self.root.join("sessions");
        create_private_dir_all(&sessions_root)?;
        let _project_lease = self.lock_project_mutation()?;
        Self::cleanup_staged_creates_unlocked(&sessions_root)?;
        Self::cleanup_interrupted_removals_unlocked(&sessions_root)?;
        Self::cleanup_atomic_temps_unlocked(&sessions_root)?;
        self.migrate_legacy_runtime_unlocked(&sessions_root)?;
        self.enforce_retention_unlocked()?;
        let final_session = self.session_dir(id);
        let create_nonce = Uuid::new_v4();
        let staging = sessions_root.join(format!(".creating-{id}-{}", create_nonce.simple()));
        let now = now_unix()?;
        let (title, provenance, content) = match parsed {
            ParsedDocument::Supported(document) => (
                document.title.clone(),
                document.provenance.clone(),
                RevisionContent::Supported { document },
            ),
            ParsedDocument::Unsupported {
                schema_version,
                raw,
            } => (
                format!("Unsupported presentation v{schema_version}"),
                Provenance::default(),
                RevisionContent::Unsupported {
                    schema_version,
                    raw,
                },
            ),
        };
        let session = SessionRecord {
            state_schema_version: STATE_SCHEMA_VERSION,
            id,
            project_key: self.project_key.clone(),
            title,
            status: SessionStatus::Active,
            current_revision: 1,
            created_at_unix: now,
            updated_at_unix: now,
            closed_at_unix: None,
            service_port: None,
            service_pid: None,
            service_instance: None,
            browser_pid: None,
            browser_instance: None,
            provenance,
        };
        let initial_revision = RevisionRecord {
            state_schema_version: STATE_SCHEMA_VERSION,
            revision: 1,
            created_at_unix: now,
            content,
        };
        let minimum_bytes = serialized_json_bytes(&initial_revision)?
            .checked_add(serialized_json_bytes(&session)?)
            .ok_or_else(|| {
                PresentError::InvalidDocument(
                    "initial presentation state size overflow".to_string(),
                )
            })?;
        self.ensure_project_capacity_unlocked(minimum_bytes, true)?;
        create_private_dir_all(&staging)?;
        write_json_atomic(
            &staging.join(CREATE_TRANSACTION_MARKER),
            &CreateTransactionMarker {
                schema_version: 1,
                session_id: id,
                nonce: create_nonce,
            },
        )?;
        create_private_dir_all(&staging.join("revisions"))?;
        let result = (|| {
            write_json_atomic(
                &staging.join("revisions/00000000000000000001.json"),
                &initial_revision,
            )?;
            write_json_atomic(&staging.join("session.json"), &session)?;
            create_private_file(&staging.join("events.jsonl"))?;
            create_private_file(&staging.join(".lock"))?;
            sync_directory(&staging.join("revisions"))?;
            sync_directory(&staging)?;
            fs::rename(&staging, &final_session)
                .map_err(|error| PresentError::io(&final_session, error))?;
            sync_directory(&sessions_root)?;
            Ok(session)
        })();
        if result.is_err() && staging.exists() {
            let _ = remove_dir_confined(&sessions_root, &staging);
        }
        if result.is_err() && final_session.exists() {
            let _ = remove_dir_confined(&sessions_root, &final_session);
            let _ = sync_directory(&sessions_root);
        }
        result
    }

    pub fn load(&self, id: Uuid) -> Result<SessionRecord> {
        self.ensure_session_layout(id)?;
        let path = self.session_path(id);
        let session: SessionRecord = read_json(&path, limits::MAX_SESSION_STATE_BYTES)?;
        self.validate_session(&session)?;
        Ok(session)
    }

    pub fn list(&self) -> Result<Vec<SessionRecord>> {
        let sessions_dir = self.root.join("sessions");
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }
        ensure_safe_dir(&sessions_dir)?;
        Self::cleanup_staged_creates(&sessions_dir)?;
        self.list_unlocked()
    }

    fn list_unlocked(&self) -> Result<Vec<SessionRecord>> {
        let sessions_dir = self.root.join("sessions");
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }
        ensure_safe_dir(&sessions_dir)?;
        let mut sessions = Vec::new();
        let mut entries = 0_usize;
        for entry in
            fs::read_dir(&sessions_dir).map_err(|error| PresentError::io(&sessions_dir, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(&sessions_dir, error))?;
            entries = entries.checked_add(1).ok_or_else(|| {
                PresentError::CorruptState("session entry count overflow".to_string())
            })?;
            if entries > limits::MAX_STATE_ENTRIES {
                return Err(PresentError::CorruptState(
                    "presentation state exceeds the session entry bound".to_string(),
                ));
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| PresentError::io(entry.path(), error))?;
            if is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(entry.path()));
            }
            if !metadata.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(&name) else {
                continue;
            };
            sessions.push(self.load(id)?);
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_unix));
        Ok(sessions)
    }

    fn cleanup_staged_creates(sessions_dir: &Path) -> Result<()> {
        let lease = open_private_append(&sessions_dir.join(PROJECT_MUTATION_LOCK))?;
        match lease.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => {
                return Err(PresentError::io(
                    sessions_dir.join(PROJECT_MUTATION_LOCK),
                    error,
                ))
            }
        }
        Self::cleanup_staged_creates_unlocked(sessions_dir)?;
        Self::cleanup_interrupted_removals_unlocked(sessions_dir)?;
        Self::cleanup_atomic_temps_unlocked(sessions_dir)
    }

    fn cleanup_staged_creates_unlocked(sessions_dir: &Path) -> Result<()> {
        for entry in
            fs::read_dir(sessions_dir).map_err(|error| PresentError::io(sessions_dir, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(sessions_dir, error))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some((session_id, nonce)) =
                parse_transaction_directory(&name, ".creating-", &entry.path())?
            else {
                continue;
            };
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| PresentError::io(entry.path(), error))?;
            if !metadata.is_dir() || is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(entry.path()));
            }
            if sessions_dir.join(session_id.to_string()).exists() {
                return Err(PresentError::CorruptState(format!(
                    "staged create {} collides with its published session",
                    entry.path().display()
                )));
            }
            let marker: CreateTransactionMarker = read_json(
                &entry.path().join(CREATE_TRANSACTION_MARKER),
                limits::MAX_SESSION_STATE_BYTES,
            )?;
            if marker
                != (CreateTransactionMarker {
                    schema_version: 1,
                    session_id,
                    nonce,
                })
            {
                return Err(PresentError::CorruptState(format!(
                    "staged create {} lacks matching transaction proof",
                    entry.path().display()
                )));
            }
            remove_dir_confined(sessions_dir, &entry.path())?;
        }
        Ok(())
    }

    fn cleanup_interrupted_removals_unlocked(sessions_dir: &Path) -> Result<()> {
        for entry in
            fs::read_dir(sessions_dir).map_err(|error| PresentError::io(sessions_dir, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(sessions_dir, error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some((session_id, nonce)) =
                parse_transaction_directory(name, ".trash-", &entry.path())?
            else {
                continue;
            };
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| PresentError::io(entry.path(), error))?;
            if !metadata.is_dir() || is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(entry.path()));
            }
            if sessions_dir.join(session_id.to_string()).exists() {
                return Err(PresentError::CorruptState(format!(
                    "interrupted removal {} collides with a live session",
                    entry.path().display()
                )));
            }
            let marker: DeleteTransactionMarker = read_json(
                &entry.path().join(DELETE_TRANSACTION_MARKER),
                limits::MAX_SESSION_STATE_BYTES,
            )?;
            if marker
                != (DeleteTransactionMarker {
                    schema_version: 1,
                    session_id,
                    nonce,
                })
            {
                return Err(PresentError::CorruptState(format!(
                    "interrupted removal {} lacks matching tombstone proof",
                    entry.path().display()
                )));
            }
            remove_dir_confined(sessions_dir, &entry.path())?;
        }
        Ok(())
    }

    fn cleanup_atomic_temps_unlocked(sessions_dir: &Path) -> Result<()> {
        for entry in
            fs::read_dir(sessions_dir).map_err(|error| PresentError::io(sessions_dir, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(sessions_dir, error))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if Uuid::parse_str(&name).is_err() {
                continue;
            }
            let session = entry.path();
            let metadata = fs::symlink_metadata(&session)
                .map_err(|error| PresentError::io(&session, error))?;
            if !metadata.is_dir() || is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(session));
            }
            cleanup_atomic_temps_in(&session, false)?;
            cleanup_atomic_temps_in(&session.join("revisions"), true)?;
        }
        Ok(())
    }

    fn migrate_legacy_runtime_unlocked(&self, sessions_dir: &Path) -> Result<()> {
        for entry in
            fs::read_dir(sessions_dir).map_err(|error| PresentError::io(sessions_dir, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(sessions_dir, error))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(&name) else {
                continue;
            };
            let legacy = entry.path().join("runtime");
            let metadata = match fs::symlink_metadata(&legacy) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(PresentError::io(&legacy, error)),
            };
            if !metadata.is_dir() || is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(legacy));
            }
            let session = self.load(id)?;
            if session.service_instance.is_some() || session.browser_instance.is_some() {
                return Err(PresentError::ServiceUnavailable(format!(
                    "presentation session {id} still owns legacy runtime resources; close it with the prior runtime before retrying"
                )));
            }
            let target = self.runtime_root.join(id.to_string());
            if target.exists() {
                return Err(PresentError::ServiceUnavailable(format!(
                    "presentation session {id} has both legacy and current runtime state; retain both for operator recovery"
                )));
            }
            fs::rename(&legacy, &target).map_err(|error| PresentError::io(&legacy, error))?;
            sync_directory(&entry.path())?;
            sync_directory(&self.runtime_root)?;
        }
        Ok(())
    }

    pub fn revision(&self, id: Uuid, revision: u64) -> Result<RevisionRecord> {
        let session = self.load(id)?;
        ensure_safe_dir(&self.session_dir(id).join("revisions"))?;
        if revision == 0 || revision > session.current_revision || revision > limits::MAX_REVISIONS
        {
            return Err(PresentError::CorruptState(format!(
                "revision {revision} is outside session {}",
                session.id
            )));
        }
        let record: RevisionRecord = read_json(
            &self.revision_path(id, revision),
            limits::MAX_REVISION_STATE_BYTES,
        )?;
        if record.state_schema_version != STATE_SCHEMA_VERSION || record.revision != revision {
            return Err(PresentError::CorruptState(format!(
                "revision {revision} has an unsupported or mismatched state schema"
            )));
        }
        Ok(record)
    }

    pub fn current_revision(&self, id: Uuid) -> Result<RevisionRecord> {
        let session = self.load(id)?;
        self.revision(id, session.current_revision)
    }

    pub fn update_document(&self, id: Uuid, parsed: ParsedDocument) -> Result<u64> {
        let _project_lease = self.prepare_growth_mutation()?;
        let _lock = self.lock_session(id)?;
        self.reconcile_update_unlocked(id)?;
        let mut session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        let revision = session.current_revision.checked_add(1).ok_or_else(|| {
            PresentError::CorruptState("presentation revision overflow".to_string())
        })?;
        if revision > limits::MAX_REVISIONS {
            return Err(PresentError::InvalidDocument(format!(
                "presentation sessions support at most {} revisions",
                limits::MAX_REVISIONS
            )));
        }
        let now = now_unix()?;
        let (title, provenance, content) = match parsed {
            ParsedDocument::Supported(document) => (
                document.title.clone(),
                document.provenance.clone(),
                RevisionContent::Supported { document },
            ),
            ParsedDocument::Unsupported {
                schema_version,
                raw,
            } => (
                format!("Unsupported presentation v{schema_version}"),
                Provenance::default(),
                RevisionContent::Unsupported {
                    schema_version,
                    raw,
                },
            ),
        };
        let marker_path = self.session_dir(id).join(UPDATE_MARKER);
        session.title = title;
        session.provenance = provenance;
        session.current_revision = revision;
        session.updated_at_unix = now;
        let marker = UpdateMarker {
            from_revision: revision - 1,
            to_revision: revision,
        };
        let revision_record = RevisionRecord {
            state_schema_version: STATE_SCHEMA_VERSION,
            revision,
            created_at_unix: now,
            content,
        };
        let session_bytes = serialized_json_bytes(&session)?;
        let peak_growth = serialized_json_bytes(&marker)?
            .checked_add(serialized_json_bytes(&revision_record)?)
            .and_then(|bytes| bytes.checked_add(session_bytes))
            .ok_or_else(|| {
                PresentError::InvalidDocument("presentation update size overflow".to_string())
            })?;
        self.ensure_project_capacity_unlocked(peak_growth, true)?;
        let result = (|| {
            write_json_atomic(&marker_path, &marker)?;
            write_json_atomic(&self.revision_path(id, revision), &revision_record)?;
            write_json_atomic(&self.session_path(id), &session)?;
            remove_file_if_regular(&marker_path)?;
            sync_directory(&self.session_dir(id))?;
            Ok(revision)
        })();
        if result.is_err() {
            let _ = self.reconcile_update_unlocked(id);
        }
        result
    }

    fn reconcile_update_unlocked(&self, id: Uuid) -> Result<()> {
        let marker_path = self.session_dir(id).join(UPDATE_MARKER);
        if fs::symlink_metadata(&marker_path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(());
        }
        let marker: UpdateMarker = read_json(&marker_path, limits::MAX_SESSION_STATE_BYTES)?;
        if marker.to_revision != marker.from_revision.saturating_add(1)
            || marker.to_revision > limits::MAX_REVISIONS
        {
            return Err(PresentError::CorruptState(format!(
                "session {id} has an invalid update marker"
            )));
        }
        let session = self.load(id)?;
        let revision_path = self.revision_path(id, marker.to_revision);
        match session.current_revision {
            current if current == marker.from_revision => {
                match fs::symlink_metadata(&revision_path) {
                    Ok(metadata) if metadata.is_file() && !is_link_like(&metadata) => {
                        let record: RevisionRecord =
                            read_json(&revision_path, limits::MAX_REVISION_STATE_BYTES)?;
                        if record.revision != marker.to_revision {
                            return Err(PresentError::CorruptState(format!(
                                "session {id} has a mismatched interrupted revision"
                            )));
                        }
                        remove_file_if_regular(&revision_path)?;
                    }
                    Ok(_) => return Err(PresentError::UnsafePath(revision_path)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(PresentError::io(&revision_path, error)),
                }
                remove_file_if_regular(&marker_path)?;
            }
            current if current == marker.to_revision => {
                let record: RevisionRecord =
                    read_json(&revision_path, limits::MAX_REVISION_STATE_BYTES)?;
                if record.revision != marker.to_revision {
                    return Err(PresentError::CorruptState(format!(
                        "session {id} has a mismatched committed revision"
                    )));
                }
                remove_file_if_regular(&marker_path)?;
            }
            _ => {
                return Err(PresentError::CorruptState(format!(
                    "session {id} update marker disagrees with current state"
                )));
            }
        }
        sync_directory(&self.session_dir(id))
    }

    pub fn history(&self, id: Uuid) -> Result<SessionHistory> {
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        let mut aggregate_bytes = 0_u64;
        for revision in 1..=session.current_revision {
            let path = self.revision_path(id, revision);
            let file = open_private_read(&path)?;
            aggregate_bytes = aggregate_bytes
                .checked_add(
                    file.metadata()
                        .map_err(|error| PresentError::io(&path, error))?
                        .len(),
                )
                .ok_or_else(|| PresentError::CorruptState("history size overflow".to_string()))?;
        }
        let events_path = self.events_path(id);
        let events = open_private_read(&events_path)?;
        aggregate_bytes = aggregate_bytes
            .checked_add(
                events
                    .metadata()
                    .map_err(|error| PresentError::io(&events_path, error))?
                    .len(),
            )
            .ok_or_else(|| PresentError::CorruptState("history size overflow".to_string()))?;
        if aggregate_bytes > limits::MAX_HISTORY_READ_BYTES {
            return Err(PresentError::ServiceUnavailable(format!(
                "presentation history is {aggregate_bytes} bytes; one-shot history output is limited to {} bytes",
                limits::MAX_HISTORY_READ_BYTES
            )));
        }
        let capacity = usize::try_from(session.current_revision).map_err(|_| {
            PresentError::CorruptState("presentation revision count is not addressable".to_string())
        })?;
        let mut revisions = Vec::with_capacity(capacity);
        for revision in 1..=session.current_revision {
            let record: RevisionRecord = read_json(
                &self.revision_path(id, revision),
                limits::MAX_REVISION_STATE_BYTES,
            )?;
            if record.state_schema_version != STATE_SCHEMA_VERSION || record.revision != revision {
                return Err(PresentError::CorruptState(format!(
                    "revision {revision} has an unsupported or mismatched state schema"
                )));
            }
            revisions.push(record);
        }
        let feedback_events = self.read_events_unlocked(id)?;
        Ok(SessionHistory {
            schema_version: STATE_SCHEMA_VERSION,
            session: HistorySession {
                id: session.id,
                title: session.title,
                status: session.status,
                current_revision: session.current_revision,
                created_at_unix: session.created_at_unix,
                updated_at_unix: session.updated_at_unix,
                closed_at_unix: session.closed_at_unix,
                provenance: session.provenance,
            },
            revisions,
            feedback_events,
        })
    }

    pub fn set_service(&self, id: Uuid, port: u16, pid: u32, instance: Uuid) -> Result<()> {
        let _project_lease = self.prepare_growth_mutation()?;
        let _lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        session.service_port = Some(port);
        session.service_pid = Some(pid);
        session.service_instance = Some(instance);
        session.updated_at_unix = now_unix()?;
        self.write_session_unlocked(id, &session, true)
    }

    pub fn clear_service(&self, id: Uuid, instance: Uuid) -> Result<bool> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.service_instance != Some(instance) {
            return Ok(false);
        }
        session.service_port = None;
        session.service_pid = None;
        session.service_instance = None;
        session.updated_at_unix = now_unix()?;
        self.write_session_unlocked(id, &session, false)?;
        Ok(true)
    }

    pub fn set_browser(&self, id: Uuid, pid: u32, instance: Uuid) -> Result<()> {
        let _project_lease = self.prepare_growth_mutation()?;
        let _lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        if session.browser_instance.is_some() {
            return Err(PresentError::BrowserUnavailable(
                "the presentation session already owns a browser process group".to_string(),
            ));
        }
        session.browser_pid = Some(pid);
        session.browser_instance = Some(instance);
        session.updated_at_unix = now_unix()?;
        self.write_session_unlocked(id, &session, true)
    }

    pub fn clear_browser(&self, id: Uuid, instance: Uuid) -> Result<bool> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.browser_instance != Some(instance) {
            return Ok(false);
        }
        session.browser_pid = None;
        session.browser_instance = None;
        session.updated_at_unix = now_unix()?;
        self.write_session_unlocked(id, &session, false)?;
        Ok(true)
    }

    pub fn close(&self, id: Uuid) -> Result<SessionRecord> {
        let _project_lease = self.prepare_control_mutation()?;
        let lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.status == SessionStatus::Closed {
            return Ok(session);
        }
        let now = now_unix()?;
        session.status = SessionStatus::Closed;
        session.closed_at_unix = Some(now);
        session.updated_at_unix = now;
        self.write_session_unlocked(id, &session, false)?;
        drop(lock);
        Ok(session)
    }

    pub fn enforce_retention(&self) -> Result<Vec<Uuid>> {
        let _project_lease = self.lock_project_mutation()?;
        Self::cleanup_staged_creates_unlocked(&self.root.join("sessions"))?;
        Self::cleanup_interrupted_removals_unlocked(&self.root.join("sessions"))?;
        Self::cleanup_atomic_temps_unlocked(&self.root.join("sessions"))?;
        self.migrate_legacy_runtime_unlocked(&self.root.join("sessions"))?;
        self.enforce_retention_unlocked()
    }

    fn enforce_retention_unlocked(&self) -> Result<Vec<Uuid>> {
        let now = now_unix()?;
        let mut closed: Vec<_> = self
            .list_unlocked()?
            .into_iter()
            .filter(|session| session.status == SessionStatus::Closed)
            .collect();
        closed.sort_by_key(|session| (session.closed_at_unix.unwrap_or(0), session.id));
        let mut closed_count = closed.len();
        let mut total_bytes = directory_size_bounded(&self.root, self.retention.max_project_bytes)?;
        let mut removed = Vec::new();
        for session in closed {
            let expired = session.closed_at_unix.is_some_and(|closed_at| {
                now.saturating_sub(closed_at)
                    >= self.retention.closed_days.saturating_mul(24 * 60 * 60)
            });
            if !expired
                && closed_count <= self.retention.max_closed_sessions
                && total_bytes <= self.retention.max_project_bytes
            {
                continue;
            }
            if matches!(
                self.clear_one(&session, Duration::ZERO, false, true)?,
                ClearOutcome::Removed
            ) {
                closed_count = closed_count.saturating_sub(1);
                total_bytes = directory_size_bounded(&self.root, self.retention.max_project_bytes)?;
                removed.push(session.id);
            }
        }
        total_bytes = directory_size_bounded(&self.root, self.retention.max_project_bytes)?;
        if total_bytes > self.retention.max_project_bytes {
            return Err(PresentError::ServiceUnavailable(format!(
                "presentation state uses {total_bytes} bytes and no unlocked closed state remains eligible for the {} byte project bound",
                self.retention.max_project_bytes
            )));
        }
        Ok(removed)
    }

    pub fn append_feedback(&self, mut envelope: FeedbackEnvelope) -> Result<FeedbackAppend> {
        let id = envelope.session_id;
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        let events = self.read_events_unlocked(id)?;
        if let Some((sequence, existing)) = events.iter().find_map(|event| match event {
            FeedbackEvent::Received {
                sequence,
                envelope: existing,
            } if existing.event_id == envelope.event_id => Some((*sequence, existing)),
            _ => None,
        }) {
            let mut normalized_existing = existing.clone();
            normalized_existing.created_at_unix = 0;
            envelope.created_at_unix = 0;
            if normalized_existing == envelope {
                return Ok(FeedbackAppend {
                    sequence,
                    created: false,
                });
            }
            return Err(PresentError::CorruptState(format!(
                "feedback event id {} was reused with different content",
                envelope.event_id
            )));
        }
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        let revision = self.revision(id, session.current_revision)?;
        validate_feedback(&envelope, &session, &revision.content)?;
        envelope.created_at_unix = now_unix()?;
        let ledger = FeedbackLedger::replay(&events)?;
        let sequence = ledger.next_sequence;
        let event = FeedbackEvent::Received { sequence, envelope };
        let mut candidate = events;
        candidate.push(event.clone());
        FeedbackLedger::replay(&candidate)?;
        self.append_events_unlocked(id, &[event], true)?;
        Ok(FeedbackAppend {
            sequence,
            created: true,
        })
    }

    pub fn events(&self, id: Uuid) -> Result<Vec<FeedbackEvent>> {
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        self.read_events_unlocked(id)
    }

    pub fn latest_event_sequence(&self, id: Uuid) -> Result<u64> {
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        self.read_events_unlocked(id)
            .map(|events| events.last().map_or(0, FeedbackEvent::sequence))
    }

    pub fn pending_feedback(&self, id: Uuid) -> Result<Vec<FeedbackEnvelope>> {
        let events = self.events(id)?;
        let ledger = FeedbackLedger::replay(&events)?;
        Ok(events
            .into_iter()
            .filter_map(|event| match event {
                FeedbackEvent::Received { envelope, .. }
                    if ledger
                        .state(envelope.event_id)
                        .is_some_and(|state| state.lifecycle == FeedbackLifecycle::Received) =>
                {
                    Some(envelope)
                }
                _ => None,
            })
            .collect())
    }

    pub fn mark_delivered(&self, id: Uuid, event_ids: &[Uuid]) -> Result<()> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        let events = self.read_events_unlocked(id)?;
        let ledger = FeedbackLedger::replay(&events)?;
        let mut sequence = ledger.next_sequence;
        let mut seen = std::collections::HashSet::new();
        let mut additions = Vec::new();
        for event_id in event_ids {
            if !seen.insert(*event_id) {
                continue;
            }
            let Some(state) = ledger.state(*event_id) else {
                return Err(PresentError::CorruptState(format!(
                    "cannot deliver unknown event {event_id}"
                )));
            };
            if state.lifecycle != FeedbackLifecycle::Received {
                continue;
            }
            additions.push(FeedbackEvent::Delivered {
                sequence,
                event_id: *event_id,
                at_unix: now_unix()?,
            });
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| PresentError::CorruptState("event sequence overflow".to_string()))?;
        }
        if additions.is_empty() {
            return Ok(());
        }
        let mut candidate = events;
        candidate.extend(additions.iter().cloned());
        FeedbackLedger::replay(&candidate)?;
        self.append_events_unlocked(id, &additions, true)?;
        Ok(())
    }

    pub fn resolve_feedback(
        &self,
        id: Uuid,
        event_id: Uuid,
        expected_version: u64,
        resolution: FeedbackResolution,
    ) -> Result<u64> {
        let _project_lease = self.prepare_control_mutation()?;
        let _lock = self.lock_session(id)?;
        self.load(id)?;
        let events = self.read_events_unlocked(id)?;
        let ledger = FeedbackLedger::replay(&events)?;
        let Some(state) = ledger.state(event_id) else {
            return Err(PresentError::InvalidDocument(format!(
                "feedback event {event_id} does not belong to session {id}"
            )));
        };
        if let Some(existing_resolution) = state.resolution {
            if existing_resolution != resolution {
                return Err(PresentError::InvalidDocument(format!(
                    "feedback event {event_id} is already resolved as {existing_resolution:?}"
                )));
            }
            if expected_version == state.version
                || state.delivered_version == Some(expected_version)
            {
                return Ok(state.version);
            }
            return Err(PresentError::InvalidDocument(format!(
                "feedback event {event_id} is at version {}, not {expected_version}",
                state.version
            )));
        }
        if state.version != expected_version {
            return Err(PresentError::InvalidDocument(format!(
                "feedback event {event_id} is at version {}, not {expected_version}",
                state.version
            )));
        }
        if state.lifecycle != FeedbackLifecycle::Delivered {
            return Err(PresentError::InvalidDocument(format!(
                "feedback event {event_id} must be delivered before it can be resolved"
            )));
        }
        let sequence = ledger.next_sequence;
        let event = match resolution {
            FeedbackResolution::Addressed => FeedbackEvent::Addressed {
                sequence,
                event_id,
                at_unix: now_unix()?,
            },
            FeedbackResolution::Dismissed => FeedbackEvent::Dismissed {
                sequence,
                event_id,
                at_unix: now_unix()?,
            },
        };
        let mut candidate = events;
        candidate.push(event.clone());
        FeedbackLedger::replay(&candidate)?;
        self.append_events_unlocked(id, &[event], true)?;
        Ok(sequence)
    }

    pub fn feedback_snapshot(&self, id: Uuid) -> Result<FeedbackSnapshot> {
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        let revision = self.revision(id, session.current_revision)?;
        let events = self.read_events_unlocked(id)?;
        build_feedback_snapshot(&events, &revision.content, session.current_revision)
    }

    pub fn clear(
        &self,
        selected: Option<Uuid>,
        older_than: Duration,
        dry_run: bool,
    ) -> Result<Vec<Uuid>> {
        let _project_lease = self.lock_project_mutation()?;
        let sessions = self.root.join("sessions");
        if let Some(id) = selected {
            let session = self.load(id)?;
            return match self.clear_one(&session, older_than, dry_run, true)? {
                ClearOutcome::Removed => Ok(vec![id]),
                ClearOutcome::Retained(reason) => Err(PresentError::PartialCleanup {
                    removed: Vec::new(),
                    failures: vec![format!("{id}: {reason}")],
                }),
            };
        }
        Self::cleanup_staged_creates_unlocked(&sessions)?;
        Self::cleanup_interrupted_removals_unlocked(&sessions)?;
        Self::cleanup_atomic_temps_unlocked(&sessions)?;
        self.migrate_legacy_runtime_unlocked(&sessions)?;
        let mut removed = Vec::new();
        let mut failures = Vec::new();
        for entry in fs::read_dir(&sessions).map_err(|error| PresentError::io(&sessions, error))? {
            let entry = entry.map_err(|error| PresentError::io(&sessions, error))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(&name) else {
                continue;
            };
            match self
                .load(id)
                .and_then(|session| self.clear_one(&session, older_than, dry_run, false))
            {
                Ok(ClearOutcome::Removed) => removed.push(id),
                Ok(ClearOutcome::Retained(_)) => {}
                Err(error) => failures.push(format!("{id}: {error}")),
            }
        }
        if !failures.is_empty() {
            return Err(PresentError::PartialCleanup {
                removed: removed.iter().map(ToString::to_string).collect(),
                failures,
            });
        }
        Ok(removed)
    }

    fn clear_one(
        &self,
        session: &SessionRecord,
        older_than: Duration,
        dry_run: bool,
        explicitly_selected: bool,
    ) -> Result<ClearOutcome> {
        let Some(lock) = self.lock_session_nonblocking_raw(session.id)? else {
            return Ok(ClearOutcome::Retained(
                "session state is currently locked by another operation",
            ));
        };
        let mut session = self.load(session.id)?;
        let Some(closed_at) = session.closed_at_unix else {
            return Ok(ClearOutcome::Retained("session is still active"));
        };
        if now_unix()?.saturating_sub(closed_at) < older_than.as_secs() && !explicitly_selected {
            return Ok(ClearOutcome::Retained(
                "closed session has not reached the requested retention age",
            ));
        }
        let service_lease = if session.service_instance.is_some() {
            let Some(lease) =
                self.try_acquire_runtime_lease(session.id, ".service.lock", "running service")?
            else {
                return Ok(ClearOutcome::Retained("session service is still running"));
            };
            Some(lease)
        } else {
            None
        };
        if session.browser_instance.is_some() {
            return Ok(ClearOutcome::Retained(
                "session browser identity has not been reconciled; retry `codeflow present close`",
            ));
        }
        let path = self.session_dir(session.id);
        ensure_confined_child(&self.root.join("sessions"), &path)?;
        if dry_run {
            return Ok(ClearOutcome::Removed);
        }
        if session.service_instance.is_some() {
            session.service_port = None;
            session.service_pid = None;
            session.service_instance = None;
            session.updated_at_unix = now_unix()?;
            self.write_session_unlocked(session.id, &session, false)?;
        }
        let runtime = self.runtime_root.join(session.id.to_string());
        if runtime.exists() && has_launch_recovery_evidence(&runtime.join("control"))? {
            return Err(PresentError::ServiceUnavailable(format!(
                "presentation runtime for {} retains launch-recovery evidence; retry `codeflow present close` before clearing durable state",
                session.id
            )));
        }
        let nonce = Uuid::new_v4();
        let tombstone = path.join(DELETE_TRANSACTION_MARKER);
        write_json_atomic(
            &tombstone,
            &DeleteTransactionMarker {
                schema_version: 1,
                session_id: session.id,
                nonce,
            },
        )?;
        sync_directory(&path)?;
        drop(service_lease);
        drop(lock);
        let trash =
            self.root
                .join("sessions")
                .join(format!(".trash-{}-{}", session.id, nonce.simple()));
        if let Err(error) = fs::rename(&path, &trash) {
            let rollback = self.lock_session_nonblocking_raw(session.id)?;
            if let Some(rollback) = rollback {
                remove_file_if_regular(&tombstone)?;
                sync_directory(&path)?;
                drop(rollback);
            }
            return Err(PresentError::io(&path, error));
        }
        sync_directory(&self.root.join("sessions"))?;
        remove_dir_confined(&self.root.join("sessions"), &trash)?;
        if runtime.exists() {
            remove_dir_confined(&self.runtime_root, &runtime)?;
        }
        Ok(ClearOutcome::Removed)
    }

    fn validate_session(&self, session: &SessionRecord) -> Result<()> {
        if session.state_schema_version != STATE_SCHEMA_VERSION {
            return Err(PresentError::CorruptState(format!(
                "session {} uses unsupported state schema {}",
                session.id, session.state_schema_version
            )));
        }
        if session.project_key != self.project_key {
            return Err(PresentError::CorruptState(format!(
                "session {} belongs to another project",
                session.id
            )));
        }
        if session.current_revision == 0 || session.current_revision > limits::MAX_REVISIONS {
            return Err(PresentError::CorruptState(format!(
                "session {} has an invalid revision count",
                session.id
            )));
        }
        if session.title.is_empty() || session.title.len() > limits::MAX_TITLE_BYTES {
            return Err(PresentError::CorruptState(format!(
                "session {} has an invalid title",
                session.id
            )));
        }
        match session.status {
            SessionStatus::Active if session.closed_at_unix.is_some() => {
                return Err(PresentError::CorruptState(format!(
                    "active session {} has a close timestamp",
                    session.id
                )));
            }
            SessionStatus::Closed if session.closed_at_unix.is_none() => {
                return Err(PresentError::CorruptState(format!(
                    "closed session {} has no close timestamp",
                    session.id
                )));
            }
            _ => {}
        }
        let service_fields = [
            session.service_port.is_some(),
            session.service_pid.is_some(),
            session.service_instance.is_some(),
        ];
        if service_fields.iter().any(|present| *present)
            && !service_fields.iter().all(|present| *present)
        {
            return Err(PresentError::CorruptState(format!(
                "session {} has a partial service identity",
                session.id
            )));
        }
        if session.browser_pid.is_some() != session.browser_instance.is_some() {
            return Err(PresentError::CorruptState(format!(
                "session {} has a partial browser identity",
                session.id
            )));
        }
        Ok(())
    }

    fn session_dir(&self, id: Uuid) -> PathBuf {
        self.root.join("sessions").join(id.to_string())
    }

    fn session_path(&self, id: Uuid) -> PathBuf {
        self.session_dir(id).join("session.json")
    }

    fn revision_path(&self, id: Uuid, revision: u64) -> PathBuf {
        self.session_dir(id)
            .join("revisions")
            .join(format!("{revision:020}.json"))
    }

    fn events_path(&self, id: Uuid) -> PathBuf {
        self.session_dir(id).join("events.jsonl")
    }

    fn lock_project_mutation(&self) -> Result<File> {
        let sessions = self.root.join("sessions");
        create_private_dir_all(&sessions)?;
        let path = sessions.join(PROJECT_MUTATION_LOCK);
        let lock = open_private_append(&path)?;
        lock.lock_exclusive()
            .map_err(|error| PresentError::io(path, error))?;
        Ok(lock)
    }

    fn prepare_growth_mutation(&self) -> Result<File> {
        let lease = self.lock_project_mutation()?;
        let sessions = self.root.join("sessions");
        Self::cleanup_staged_creates_unlocked(&sessions)?;
        Self::cleanup_interrupted_removals_unlocked(&sessions)?;
        Self::cleanup_atomic_temps_unlocked(&sessions)?;
        self.migrate_legacy_runtime_unlocked(&sessions)?;
        self.enforce_retention_unlocked()?;
        Ok(lease)
    }

    fn prepare_control_mutation(&self) -> Result<File> {
        let lease = self.lock_project_mutation()?;
        let sessions = self.root.join("sessions");
        Self::cleanup_staged_creates_unlocked(&sessions)?;
        Self::cleanup_interrupted_removals_unlocked(&sessions)?;
        Self::cleanup_atomic_temps_unlocked(&sessions)?;
        self.migrate_legacy_runtime_unlocked(&sessions)?;
        Ok(lease)
    }

    fn ensure_project_capacity_unlocked(
        &self,
        additional_bytes: u64,
        preserve_control_reserve: bool,
    ) -> Result<()> {
        let reserve = if preserve_control_reserve {
            CONTROL_MUTATION_RESERVE_BYTES
        } else {
            0
        };
        let required = additional_bytes.checked_add(reserve).ok_or_else(|| {
            PresentError::InvalidDocument("project quota size overflow".to_string())
        })?;
        if required > self.retention.max_project_bytes {
            return Err(PresentError::InvalidDocument(format!(
                "presentation mutation requires {required} bytes of project headroom but the project state limit is {} bytes",
                self.retention.max_project_bytes
            )));
        }
        let current = directory_size_bounded(&self.root, self.retention.max_project_bytes)?;
        if current
            .checked_add(required)
            .is_none_or(|projected| projected > self.retention.max_project_bytes)
        {
            return Err(PresentError::ServiceUnavailable(format!(
                "presentation state has insufficient capacity for this mutation under the {} byte project bound; close and clear unused sessions or raise the configured bound",
                self.retention.max_project_bytes
            )));
        }
        Ok(())
    }

    fn write_session_unlocked(
        &self,
        id: Uuid,
        session: &SessionRecord,
        enforce_capacity: bool,
    ) -> Result<()> {
        if enforce_capacity {
            self.ensure_project_capacity_unlocked(serialized_json_bytes(session)?, true)?;
        }
        write_json_atomic(&self.session_path(id), session)
    }

    fn lock_session(&self, id: Uuid) -> Result<File> {
        let lock = self.open_lock_raw(id)?;
        lock.lock_exclusive()
            .map_err(|error| PresentError::io(self.session_dir(id).join(".lock"), error))?;
        self.reject_tombstone(id)?;
        Ok(lock)
    }

    fn lock_session_nonblocking_raw(&self, id: Uuid) -> Result<Option<File>> {
        let lock = self.open_lock_raw(id)?;
        match lock.try_lock_exclusive() {
            Ok(()) => Ok(Some(lock)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(PresentError::io(self.session_dir(id).join(".lock"), error)),
        }
    }

    fn open_lock_raw(&self, id: Uuid) -> Result<File> {
        let dir = self.session_dir(id);
        ensure_confined_child(&self.root.join("sessions"), &dir)?;
        if !dir.exists() {
            return Err(PresentError::SessionNotFound(id.to_string()));
        }
        self.ensure_session_layout(id)?;
        open_private_append(&dir.join(".lock"))
    }

    fn ensure_session_layout(&self, id: Uuid) -> Result<()> {
        let sessions = self.root.join("sessions");
        ensure_safe_dir(&sessions)?;
        let session = self.session_dir(id);
        ensure_confined_child(&sessions, &session)?;
        match fs::symlink_metadata(&session) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PresentError::SessionNotFound(id.to_string()));
            }
            Err(error) => return Err(PresentError::io(&session, error)),
            Ok(_) => {}
        }
        ensure_safe_dir(&session)?;
        ensure_safe_dir(&session.join("revisions"))
    }

    fn reject_tombstone(&self, id: Uuid) -> Result<()> {
        if self.session_dir(id).join(".deleting").exists() {
            return Err(PresentError::SessionNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn acquire_service_lease(&self, id: Uuid) -> Result<RuntimeLease> {
        self.acquire_runtime_lease(id, ".service.lock", "running service")
    }

    pub fn acquire_startup_lease(&self, id: Uuid) -> Result<RuntimeLease> {
        self.acquire_runtime_lease(id, ".startup.lock", "service startup")
    }

    pub fn acquire_browser_launch_lease(&self, id: Uuid) -> Result<RuntimeLease> {
        self.acquire_runtime_lease(id, ".browser-launch.lock", "browser launch")
    }

    fn acquire_runtime_lease(
        &self,
        id: Uuid,
        file_name: &str,
        owner: &str,
    ) -> Result<RuntimeLease> {
        self.load(id)?;
        let path = self.session_dir(id).join(file_name);
        try_runtime_lease(&path)?.ok_or_else(|| {
            PresentError::ServiceUnavailable(format!(
                "presentation session {id} already has a {owner}"
            ))
        })
    }

    fn try_acquire_runtime_lease(
        &self,
        id: Uuid,
        file_name: &str,
        owner: &str,
    ) -> Result<Option<RuntimeLease>> {
        let path = self.session_dir(id).join(file_name);
        try_runtime_lease(&path).map_err(|error| {
            PresentError::ServiceUnavailable(format!(
                "failed to prove absence of {owner} for session {id}: {error}"
            ))
        })
    }

    fn read_events_unlocked(&self, id: Uuid) -> Result<Vec<FeedbackEvent>> {
        let path = self.events_path(id);
        self.ensure_session_layout(id)?;
        let file = open_private_read(&path)?;
        let metadata = file
            .metadata()
            .map_err(|error| PresentError::io(&path, error))?;
        if metadata.len() > limits::MAX_EVENT_LOG_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} exceeds the {} byte event-log bound",
                path.display(),
                limits::MAX_EVENT_LOG_BYTES
            )));
        }
        let capacity = usize::try_from(metadata.len())
            .map_err(|_| PresentError::CorruptState("event log is not addressable".to_string()))?;
        let mut raw = String::with_capacity(capacity);
        file.take(limits::MAX_EVENT_LOG_BYTES.saturating_add(1))
            .read_to_string(&mut raw)
            .map_err(|error| PresentError::io(&path, error))?;
        if raw.len() as u64 > limits::MAX_EVENT_LOG_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} grew beyond its event-log bound while reading",
                path.display()
            )));
        }
        parse_event_log(&path, &raw, limits::MAX_FEEDBACK_EVENTS)
    }

    fn append_events_unlocked(
        &self,
        id: Uuid,
        events: &[FeedbackEvent],
        preserve_control_reserve: bool,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let path = self.events_path(id);
        self.ensure_session_layout(id)?;
        repair_partial_tail(&path)?;
        let mut encoded = Vec::new();
        for event in events {
            let line = serde_json::to_vec(event)?;
            if line.len() as u64 > limits::MAX_EVENT_RECORD_BYTES {
                return Err(PresentError::InvalidDocument(
                    "feedback event exceeds the event-record bound".to_string(),
                ));
            }
            encoded.extend_from_slice(&line);
            encoded.push(b'\n');
        }
        let mut file = open_private_append(&path)?;
        let current = file
            .metadata()
            .map_err(|error| PresentError::io(&path, error))?
            .len();
        if current
            .checked_add(encoded.len() as u64)
            .is_none_or(|size| size > limits::MAX_EVENT_LOG_BYTES)
        {
            return Err(PresentError::InvalidDocument(
                "feedback history reached its bounded event-log capacity".to_string(),
            ));
        }
        self.enforce_retention_unlocked()?;
        self.ensure_project_capacity_unlocked(encoded.len() as u64, preserve_control_reserve)?;
        file.write_all(&encoded)
            .map_err(|error| PresentError::io(&path, error))?;
        file.sync_data()
            .map_err(|error| PresentError::io(&path, error))
    }
}

fn parse_transaction_directory(
    name: &str,
    prefix: &str,
    path: &Path,
) -> Result<Option<(Uuid, Uuid)>> {
    let Some(rest) = name.strip_prefix(prefix) else {
        return Ok(None);
    };
    let Some((session_text, nonce_text)) = rest.rsplit_once('-') else {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    };
    let session_id =
        Uuid::parse_str(session_text).map_err(|_| PresentError::UnsafePath(path.to_path_buf()))?;
    let nonce =
        Uuid::parse_str(nonce_text).map_err(|_| PresentError::UnsafePath(path.to_path_buf()))?;
    if session_id.to_string() != session_text
        || nonce.simple().to_string() != nonce_text
        || name != format!("{prefix}{session_id}-{}", nonce.simple())
    {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(Some((session_id, nonce)))
}

fn has_launch_recovery_evidence(control: &Path) -> Result<bool> {
    let entries = match fs::read_dir(control) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(PresentError::io(control, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| PresentError::io(control, error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(_) = parse_launch_recovery_name(&name, &entry.path())? else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| PresentError::io(entry.path(), error))?;
        if !metadata.is_file() || is_link_like(&metadata) {
            return Err(PresentError::UnsafePath(entry.path()));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn parse_launch_recovery_name(name: &str, path: &Path) -> Result<Option<Uuid>> {
    let Some(instance) = name
        .strip_prefix(LAUNCH_RECOVERY_PREFIX)
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return Ok(None);
    };
    let instance =
        Uuid::parse_str(instance).map_err(|_| PresentError::UnsafePath(path.to_path_buf()))?;
    if name != format!("{LAUNCH_RECOVERY_PREFIX}{}.json", instance.simple()) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(Some(instance))
}

fn parse_event_log(path: &Path, raw: &str, max_events: usize) -> Result<Vec<FeedbackEvent>> {
    let mut events = Vec::new();
    for (index, line) in raw.split_inclusive('\n').enumerate() {
        let value = line.strip_suffix('\n').unwrap_or(line);
        if value.is_empty() {
            continue;
        }
        if value.len() as u64 > limits::MAX_EVENT_RECORD_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} line {} exceeds the event-record bound",
                path.display(),
                index + 1
            )));
        }
        match serde_json::from_str::<FeedbackEvent>(value) {
            Ok(event) => {
                if events.len() == max_events {
                    return Err(PresentError::CorruptState(format!(
                        "{} exceeds the {} event bound",
                        path.display(),
                        max_events
                    )));
                }
                events.push(event);
            }
            Err(_) if !line.ends_with('\n') => break,
            Err(error) => {
                return Err(PresentError::CorruptState(format!(
                    "{} line {}: {error}",
                    path.display(),
                    index + 1
                )));
            }
        }
    }
    FeedbackLedger::replay(&events).map_err(|error| match error {
        PresentError::CorruptState(message) => {
            PresentError::CorruptState(format!("{}: {message}", path.display()))
        }
        other => other,
    })?;
    Ok(events)
}

fn validate_feedback(
    envelope: &FeedbackEnvelope,
    session: &SessionRecord,
    content: &RevisionContent,
) -> Result<()> {
    if envelope.session_id != session.id || envelope.revision != session.current_revision {
        return Err(PresentError::InvalidDocument(
            "feedback session or revision is stale".to_string(),
        ));
    }
    if envelope.actor.trim().is_empty() || envelope.actor.len() > 256 {
        return Err(PresentError::InvalidDocument(
            "feedback actor is empty or too long".to_string(),
        ));
    }
    if matches!(envelope.verdict, FeedbackVerdict::RequestChanges)
        && envelope
            .instruction
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(PresentError::InvalidDocument(
            "request_changes requires a nonempty instruction".to_string(),
        ));
    }
    if envelope.instruction.as_deref().is_some_and(|instruction| {
        instruction.encode_utf16().count() > limits::MAX_FEEDBACK_TEXT_UTF16
    }) {
        return Err(PresentError::InvalidDocument(format!(
            "feedback instruction exceeds {} UTF-16 units",
            limits::MAX_FEEDBACK_TEXT_UTF16
        )));
    }
    let mut bytes = envelope.actor.len() + envelope.instruction.as_deref().map_or(0, str::len);
    let mut ids = std::collections::HashSet::new();
    if envelope.notes.len() > limits::MAX_FEEDBACK_NOTES {
        return Err(PresentError::InvalidDocument(format!(
            "a review may contain at most {} notes",
            limits::MAX_FEEDBACK_NOTES
        )));
    }
    for note in &envelope.notes {
        if !ids.insert(note.id) {
            return Err(PresentError::InvalidDocument(
                "feedback note ids must be unique".to_string(),
            ));
        }
        if note.body.trim().is_empty()
            || note.block_id.trim().is_empty()
            || note.block_label.trim().is_empty()
        {
            return Err(PresentError::InvalidDocument(
                "feedback note body, block id, and block label are required".to_string(),
            ));
        }
        if note.body.encode_utf16().count() > limits::MAX_FEEDBACK_TEXT_UTF16 {
            return Err(PresentError::InvalidDocument(format!(
                "feedback note exceeds {} UTF-16 units",
                limits::MAX_FEEDBACK_TEXT_UTF16
            )));
        }
        let block = content_block(content, &note.block_id).ok_or_else(|| {
            PresentError::InvalidDocument(format!(
                "feedback block {} is not present in the current revision",
                note.block_id
            ))
        })?;
        if note.block_label != block.review_label() {
            return Err(PresentError::InvalidDocument(
                "feedback block label does not match the current revision".to_string(),
            ));
        }
        bytes += note.body.len() + note.block_id.len() + note.block_label.len();
        if let Some(selector) = &note.selector {
            let selected_units = selector.exact.encode_utf16().count();
            let range_units = selector.end_utf16.saturating_sub(selector.start_utf16) as usize;
            if selector.end_utf16 <= selector.start_utf16
                || selector.exact.trim().is_empty()
                || selected_units > limits::MAX_SELECTOR_EXACT_UTF16
                || selected_units != range_units
                || selector.prefix.encode_utf16().count() > 32
                || selector.suffix.encode_utf16().count() > 32
            {
                return Err(PresentError::InvalidDocument(
                    "feedback selector range or exact quote is invalid".to_string(),
                ));
            }
            validate_selector_anchor(selector, &block.canonical_review_text())?;
            bytes += selector.exact.len() + selector.prefix.len() + selector.suffix.len();
        }
    }
    if bytes > limits::MAX_FEEDBACK_BYTES {
        return Err(PresentError::InvalidDocument(format!(
            "feedback exceeds {} bytes",
            limits::MAX_FEEDBACK_BYTES
        )));
    }
    Ok(())
}

fn content_block<'a>(content: &'a RevisionContent, id: &str) -> Option<&'a crate::document::Block> {
    match content {
        RevisionContent::Unsupported { .. } => None,
        RevisionContent::Supported { document } => find_block(&document.blocks, id),
    }
}

fn find_block<'a>(
    blocks: &'a [crate::document::Block],
    id: &str,
) -> Option<&'a crate::document::Block> {
    blocks.iter().find_map(|block| {
        if block.id() == id {
            return Some(block);
        }
        match block {
            crate::document::Block::Disclosure { blocks, .. } => find_block(blocks, id),
            crate::document::Block::Tabs { tabs, .. } => {
                tabs.iter().find_map(|tab| find_block(&tab.blocks, id))
            }
            _ => None,
        }
    })
}

fn build_feedback_snapshot(
    events: &[FeedbackEvent],
    current: &RevisionContent,
    current_revision: u64,
) -> Result<FeedbackSnapshot> {
    let mut lifecycle = std::collections::HashMap::new();
    let mut received = Vec::new();
    for event in events {
        match event {
            FeedbackEvent::Received { sequence, envelope } => {
                lifecycle.insert(envelope.event_id, (FeedbackLifecycle::Received, *sequence));
                received.push(envelope);
            }
            FeedbackEvent::Delivered {
                sequence, event_id, ..
            } => {
                lifecycle.insert(*event_id, (FeedbackLifecycle::Delivered, *sequence));
            }
            FeedbackEvent::Addressed {
                sequence, event_id, ..
            } => {
                lifecycle.insert(*event_id, (FeedbackLifecycle::Addressed, *sequence));
            }
            FeedbackEvent::Dismissed {
                sequence, event_id, ..
            } => {
                lifecycle.insert(*event_id, (FeedbackLifecycle::Dismissed, *sequence));
            }
        }
    }
    let omitted_older = received.len().saturating_sub(limits::MAX_VISIBLE_FEEDBACK);
    let visible = received.into_iter().skip(omitted_older);
    let mut items = Vec::new();
    for envelope in visible {
        let (state, event_version) =
            lifecycle.get(&envelope.event_id).copied().ok_or_else(|| {
                PresentError::CorruptState("feedback lifecycle is missing".to_string())
            })?;
        let notes = envelope
            .notes
            .iter()
            .map(|note| FeedbackNoteView {
                id: note.id,
                block_label: note.block_label.clone(),
                kind: note.kind.clone(),
                body: note.body.clone(),
                quote: note
                    .selector
                    .as_ref()
                    .map(|selector| selector.exact.clone()),
                anchor: reanchor_note(note, envelope.revision, current, current_revision),
            })
            .collect();
        items.push(FeedbackView {
            event_id: envelope.event_id,
            source_revision: envelope.revision,
            event_version,
            lifecycle: state,
            verdict: envelope.verdict.clone(),
            instruction: envelope.instruction.clone(),
            notes,
        });
    }
    Ok(FeedbackSnapshot {
        items,
        omitted_older,
    })
}

fn reanchor_note(
    note: &FeedbackNote,
    source_revision: u64,
    current: &RevisionContent,
    current_revision: u64,
) -> FeedbackAnchor {
    let RevisionContent::Supported { document } = current else {
        return FeedbackAnchor::Orphaned {
            reason: "current document schema is unsupported".to_string(),
        };
    };
    let Some(block) = find_block(&document.blocks, &note.block_id) else {
        return FeedbackAnchor::Orphaned {
            reason: "the referenced block is absent from the current revision".to_string(),
        };
    };
    let Some(selector) = &note.selector else {
        return FeedbackAnchor::Block {
            block_id: note.block_id.clone(),
        };
    };
    if source_revision == current_revision {
        return FeedbackAnchor::Anchored {
            start_utf16: selector.start_utf16,
            end_utf16: selector.end_utf16,
        };
    }
    let canonical = block.canonical_review_text();
    let matches = canonical
        .match_indices(&selector.exact)
        .filter(|(start, exact)| {
            canonical[..*start].ends_with(&selector.prefix)
                && canonical[start + exact.len()..].starts_with(&selector.suffix)
        })
        .map(|(start, exact)| {
            let start_utf16 = canonical[..start].encode_utf16().count();
            let end_utf16 = start_utf16 + exact.encode_utf16().count();
            (start_utf16, end_utf16)
        })
        .take(2)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(start, end)] => {
            let Ok(start_utf16) = u32::try_from(*start) else {
                return FeedbackAnchor::Orphaned {
                    reason: "re-anchored selection exceeds the offset bound".to_string(),
                };
            };
            let Ok(end_utf16) = u32::try_from(*end) else {
                return FeedbackAnchor::Orphaned {
                    reason: "re-anchored selection exceeds the offset bound".to_string(),
                };
            };
            FeedbackAnchor::Reanchored {
                start_utf16,
                end_utf16,
            }
        }
        [] => FeedbackAnchor::Orphaned {
            reason: "the exact quote and context no longer match".to_string(),
        },
        _ => FeedbackAnchor::Orphaned {
            reason: "the exact quote and context match more than once".to_string(),
        },
    }
}

fn validate_selector_anchor(selector: &TextSelector, canonical: &str) -> Result<()> {
    let units: Vec<u16> = canonical.encode_utf16().collect();
    let start = selector.start_utf16 as usize;
    let end = selector.end_utf16 as usize;
    if end > units.len() {
        return Err(PresentError::InvalidDocument(
            "feedback selector is outside canonical review text".to_string(),
        ));
    }
    let exact = String::from_utf16(&units[start..end]).map_err(|_| {
        PresentError::InvalidDocument("feedback selector splits a UTF-16 character".to_string())
    })?;
    let prefix_start = start.saturating_sub(32);
    let prefix = String::from_utf16(&units[prefix_start..start]).map_err(|_| {
        PresentError::InvalidDocument("feedback selector prefix is not valid UTF-16".to_string())
    })?;
    let suffix_end = end.saturating_add(32).min(units.len());
    let suffix = String::from_utf16(&units[end..suffix_end]).map_err(|_| {
        PresentError::InvalidDocument("feedback selector suffix is not valid UTF-16".to_string())
    })?;
    if selector.exact != exact || selector.prefix != prefix || selector.suffix != suffix {
        return Err(PresentError::InvalidDocument(
            "feedback selector does not match canonical review text".to_string(),
        ));
    }
    Ok(())
}

fn platform_state_root() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData;

        let root = crate::platform::known_folder(&FOLDERID_LocalAppData)?;
        return Ok(root.join("codeflow").join("present"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("codeflow")
            .join("present"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = absolute_xdg_state_home(std::env::var_os("XDG_STATE_HOME")) {
            return Ok(root.join("codeflow").join("present"));
        }
        Ok(home_dir()?.join(".local/state/codeflow/present"))
    }
}

#[cfg(all(unix, any(not(target_os = "macos"), test)))]
fn absolute_xdg_state_home(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|root| root.is_absolute())
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf> {
    validated_home(std::env::var_os("HOME"))
}

#[cfg(unix)]
fn validated_home(home: Option<std::ffi::OsString>) -> Result<PathBuf> {
    let home = home
        .map(PathBuf::from)
        .ok_or_else(|| PresentError::UnsafePath(PathBuf::from("$HOME")))?;
    if !home.is_absolute() {
        return Err(PresentError::UnsafePath(home));
    }
    Ok(home)
}

fn now_unix() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| {
            PresentError::CorruptState(format!("system clock precedes Unix epoch: {error}"))
        })
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn serialized_json_bytes<T: Serialize>(value: &T) -> Result<u64> {
    let bytes = serde_json::to_vec_pretty(value)?
        .len()
        .checked_add(1)
        .ok_or_else(|| {
            PresentError::InvalidDocument("serialized state size overflow".to_string())
        })?;
    u64::try_from(bytes).map_err(|_| {
        PresentError::InvalidDocument("serialized state is not addressable".to_string())
    })
}

fn canonical_path_identity(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;

        let mut identity = b"unix\0".to_vec();
        identity.extend_from_slice(path.as_os_str().as_bytes());
        identity
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;

        let mut identity = b"windows-utf16le\0".to_vec();
        for unit in path.as_os_str().encode_wide() {
            identity.extend_from_slice(&unit.to_le_bytes());
        }
        identity
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut identity = b"fallback\0".to_vec();
        identity.extend_from_slice(path.as_os_str().to_string_lossy().as_bytes());
        identity
    }
}

pub(crate) fn create_private_dir_all(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !is_link_like(&metadata) => {}
            #[cfg(unix)]
            Ok(metadata) if trusted_system_symlink(&current, &metadata) => {
                let target = fs::canonicalize(&current)
                    .map_err(|error| PresentError::io(&current, error))?;
                let target_metadata = fs::symlink_metadata(&target)
                    .map_err(|error| PresentError::io(&target, error))?;
                if !target_metadata.is_dir() || is_link_like(&target_metadata) {
                    return Err(PresentError::UnsafePath(current));
                }
            }
            Ok(_) => return Err(PresentError::UnsafePath(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(windows)]
                crate::platform::create_private_directory(&current)?;
                #[cfg(not(windows))]
                {
                    let builder = fs::DirBuilder::new();
                    #[cfg(unix)]
                    let builder = {
                        use std::os::unix::fs::DirBuilderExt;
                        let mut builder = builder;
                        builder.mode(0o700);
                        builder
                    };
                    #[cfg(not(unix))]
                    let builder = builder;
                    builder
                        .create(&current)
                        .map_err(|error| PresentError::io(&current, error))?;
                    let metadata = fs::symlink_metadata(&current)
                        .map_err(|error| PresentError::io(&current, error))?;
                    if !metadata.is_dir() || is_link_like(&metadata) {
                        return Err(PresentError::UnsafePath(current));
                    }
                    harden_private_path(&current, true)?;
                }
            }
            Err(error) => return Err(PresentError::io(&current, error)),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| PresentError::io(path, error))?;
    }
    #[cfg(windows)]
    crate::platform::verify_private_directory(path)?;
    Ok(())
}

#[cfg(unix)]
fn trusted_system_symlink(path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    // macOS ships immutable compatibility links such as /var -> /private/var.
    // User-owned or group/world-writable links remain a hard failure.
    metadata.file_type().is_symlink()
        && metadata.uid() == 0
        && metadata.permissions().mode() & 0o022 == 0
        && path.parent().is_some()
}

fn create_private_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link_like(&metadata) => {
            open_private_read(path)?;
            return Ok(());
        }
        Ok(_) => return Err(PresentError::UnsafePath(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(PresentError::io(path, error)),
    }
    let file = open_private_create_new(path)?;
    file.sync_all()
        .map_err(|error| PresentError::io(path, error))
}

pub(crate) fn open_private_create_new(path: &Path) -> Result<File> {
    #[cfg(windows)]
    {
        return crate::platform::open_private_create_new(path);
    }
    #[cfg(not(windows))]
    {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        add_no_follow(&mut options);
        let file = options
            .open(path)
            .map_err(|error| PresentError::io(path, error))?;
        if let Err(error) = harden_private_file(path, &file) {
            discard_new_file(&file, path);
            return Err(error);
        }
        #[cfg(any(unix, windows))]
        if let Err(error) = validate_private_file(path, &file) {
            discard_new_file(&file, path);
            return Err(error);
        }
        Ok(file)
    }
}

pub(crate) fn discard_new_file(file: &File, path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };

        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileDispositionInfo,
                (&raw const disposition).cast(),
                u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO>())
                    .expect("file disposition size fits u32"),
            )
        };
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        let _ = fs::remove_file(path);
    }
    #[cfg(windows)]
    let _ = path;
}

#[cfg(unix)]
fn add_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
}

#[cfg(windows)]
fn add_no_follow(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(not(any(unix, windows)))]
fn add_no_follow(_options: &mut OpenOptions) {}

fn open_private_append(path: &Path) -> Result<File> {
    #[cfg(windows)]
    {
        let file = match crate::platform::open_private_create_new(path) {
            Ok(mut file) => {
                file.seek(SeekFrom::End(0))
                    .map_err(|error| PresentError::io(path, error))?;
                file
            }
            Err(error)
                if matches!(
                    &error,
                    PresentError::Io { source, .. }
                        if source.kind() == std::io::ErrorKind::AlreadyExists
                ) =>
            {
                let mut existing = OpenOptions::new();
                existing.read(true).write(true);
                add_no_follow(&mut existing);
                let mut file = existing
                    .open(path)
                    .map_err(|error| PresentError::io(path, error))?;
                file.seek(SeekFrom::End(0))
                    .map_err(|error| PresentError::io(path, error))?;
                file
            }
            Err(error) => return Err(error),
        };
        validate_private_file(path, &file)?;
        return Ok(file);
    }

    #[cfg(not(windows))]
    {
        let mut create = OpenOptions::new();
        create.create_new(true).append(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            create.mode(0o600);
        }
        add_no_follow(&mut create);
        let file = match create.open(path) {
            Ok(file) => {
                harden_private_path(path, false)?;
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut existing = OpenOptions::new();
                existing.append(true).read(true);
                add_no_follow(&mut existing);
                existing
                    .open(path)
                    .map_err(|error| PresentError::io(path, error))?
            }
            Err(error) => return Err(PresentError::io(path, error)),
        };
        #[cfg(any(unix, windows))]
        validate_private_file(path, &file)?;
        Ok(file)
    }
}

fn open_private_read(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    add_no_follow(&mut options);
    let file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    #[cfg(any(unix, windows))]
    validate_private_file(path, &file)?;
    Ok(file)
}

fn repair_partial_tail(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    add_no_follow(&mut options);
    let mut file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    #[cfg(any(unix, windows))]
    validate_private_file(path, &file)?;
    let length = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?
        .len();
    if length == 0 {
        return Ok(());
    }
    if length > limits::MAX_EVENT_LOG_BYTES {
        return Err(PresentError::CorruptState(format!(
            "{} exceeds the {} byte event-log bound",
            path.display(),
            limits::MAX_EVENT_LOG_BYTES
        )));
    }
    file.seek(SeekFrom::End(-1))
        .map_err(|error| PresentError::io(path, error))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|error| PresentError::io(path, error))?;
    if final_byte[0] == b'\n' {
        return Ok(());
    }

    let window = (limits::MAX_EVENT_RECORD_BYTES * 2).min(length);
    let start = length - window;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| PresentError::io(path, error))?;
    let capacity = usize::try_from(window).map_err(|_| {
        PresentError::CorruptState("event repair tail is not addressable".to_string())
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    std::io::Read::by_ref(&mut file)
        .take(window.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    if bytes.len() as u64 > window {
        return Err(PresentError::CorruptState(
            "event log grew while its partial tail was being repaired".to_string(),
        ));
    }
    let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        if start != 0 {
            return Err(PresentError::CorruptState(
                "event tail has an oversized record".to_string(),
            ));
        }
        return file
            .set_len(0)
            .and_then(|()| file.sync_data())
            .map_err(|error| PresentError::io(path, error));
    };
    let partial_bytes = bytes.len().saturating_sub(last_newline + 1) as u64;
    if partial_bytes > limits::MAX_EVENT_RECORD_BYTES {
        return Err(PresentError::CorruptState(
            "event partial tail exceeds the event-record bound".to_string(),
        ));
    }
    let repaired_length = start
        + u64::try_from(last_newline + 1).map_err(|_| {
            PresentError::CorruptState("event repair offset is not addressable".to_string())
        })?;
    file.set_len(repaired_length)
        .and_then(|()| file.sync_data())
        .map_err(|error| PresentError::io(path, error))
}

#[cfg(unix)]
fn validate_private_file(path: &Path, file: &File) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_private_file(path: &Path, file: &File) -> Result<()> {
    crate::platform::verify_private_file(path, file)
}

fn ensure_safe_dir(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(PresentError::UnsafePath(path.to_path_buf()));
        }
    }
    #[cfg(windows)]
    crate::platform::verify_private_directory(path)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, max_bytes: u64) -> Result<T> {
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => PresentError::SessionNotFound(path.display().to_string()),
        _ => PresentError::io(path, error),
    })?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    add_no_follow(&mut options);
    let file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    #[cfg(any(unix, windows))]
    validate_private_file(path, &file)?;
    let opened = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?;
    if opened.len() > max_bytes {
        return Err(PresentError::CorruptState(format!(
            "{} exceeds its {} byte state bound",
            path.display(),
            max_bytes
        )));
    }
    let capacity = usize::try_from(opened.len()).map_err(|_| {
        PresentError::CorruptState(format!("{} is not addressable", path.display()))
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(PresentError::CorruptState(format!(
            "{} grew beyond its state bound while reading",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes).map_err(PresentError::from)
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(path.to_path_buf()))?;
    ensure_safe_dir(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = open_private_create_new(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")
            .map_err(|error| PresentError::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| PresentError::io(&temporary, error))?;
        fs::rename(&temporary, path).map_err(|error| PresentError::io(path, error))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        if let Ok(file) = open_private_read(&temporary) {
            discard_new_file(&file, &temporary);
        }
    }
    result
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| PresentError::io(path, error))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn ensure_confined_child(root: &Path, child: &Path) -> Result<()> {
    if child.parent() != Some(root) {
        return Err(PresentError::UnsafePath(child.to_path_buf()));
    }
    ensure_safe_dir(root)
}

fn remove_dir_confined(root: &Path, path: &Path) -> Result<()> {
    ensure_confined_child(root, path)?;
    ensure_safe_dir(path)?;
    fs::remove_dir_all(path).map_err(|error| PresentError::io(path, error))
}

fn remove_file_if_regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    open_private_read(path)?;
    fs::remove_file(path).map_err(|error| PresentError::io(path, error))
}

fn cleanup_atomic_temps_in(directory: &Path, revisions: bool) -> Result<()> {
    ensure_safe_dir(directory)?;
    for entry in fs::read_dir(directory).map_err(|error| PresentError::io(directory, error))? {
        let entry = entry.map_err(|error| PresentError::io(directory, error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(body) = name
            .strip_prefix('.')
            .and_then(|name| name.strip_suffix(".tmp"))
        else {
            continue;
        };
        let Some((destination, nonce)) = body.rsplit_once('.') else {
            continue;
        };
        if Uuid::parse_str(nonce).is_err() {
            continue;
        }
        let expected_destination = if revisions {
            let bytes = destination.as_bytes();
            bytes.len() == 25
                && &bytes[20..] == b".json"
                && bytes[..20].iter().all(u8::is_ascii_digit)
        } else {
            matches!(destination, "session.json" | ".updating.json")
        };
        if expected_destination {
            remove_file_if_regular(&entry.path())?;
        }
    }
    sync_directory(directory)
}

fn directory_size_bounded(root: &Path, byte_limit: u64) -> Result<u64> {
    if !root.exists() {
        return Ok(0);
    }
    ensure_safe_dir(root)?;
    let mut total = 0_u64;
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        ensure_safe_dir(&directory)?;
        if depth > limits::MAX_STATE_DEPTH {
            return Err(PresentError::CorruptState(
                "presentation state exceeds the directory-depth bound".to_string(),
            ));
        }
        for entry in
            fs::read_dir(&directory).map_err(|error| PresentError::io(&directory, error))?
        {
            let entry = entry.map_err(|error| PresentError::io(&directory, error))?;
            entries = entries.checked_add(1).ok_or_else(|| {
                PresentError::CorruptState("presentation state entry count overflow".to_string())
            })?;
            if entries > limits::MAX_STATE_ENTRIES {
                return Err(PresentError::CorruptState(
                    "presentation state exceeds the entry-count bound".to_string(),
                ));
            }
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
            if is_link_like(&metadata) {
                return Err(PresentError::UnsafePath(path));
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
                open_private_read(&path)?;
                total = total.checked_add(metadata.len()).ok_or_else(|| {
                    PresentError::CorruptState("presentation state size overflow".to_string())
                })?;
                if total > byte_limit {
                    return Ok(byte_limit.saturating_add(1));
                }
            } else {
                return Err(PresentError::UnsafePath(path));
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn project_identity_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStrExt as _;

        let first = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/repo-\x80"));
        let second = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/repo-\x81"));
        assert_ne!(
            canonical_path_identity(first),
            canonical_path_identity(second)
        );
        assert_ne!(
            hex_digest(&canonical_path_identity(first)),
            hex_digest(&canonical_path_identity(second))
        );
    }
    use crate::document::{Block, ParsedDocument};

    fn store() -> (tempfile::TempDir, SessionStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::at_root(temp.path().join("project"), "key".to_string()).unwrap();
        (temp, store)
    }

    fn parsed() -> ParsedDocument {
        ParsedDocument::Supported(PresentationDocument {
            schema_version: 1,
            title: "Review".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks: vec![Block::Narrative {
                id: "intro".to_string(),
                markdown: "Hello".to_string(),
            }],
        })
    }

    fn feedback(session_id: Uuid, event_id: Uuid) -> FeedbackEnvelope {
        FeedbackEnvelope {
            event_id,
            session_id,
            revision: 1,
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::Approve,
            instruction: None,
            notes: Vec::new(),
            created_at_unix: 0,
        }
    }

    fn parsed_code(bytes: usize) -> ParsedDocument {
        ParsedDocument::Supported(PresentationDocument {
            schema_version: 1,
            title: "Large review".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks: vec![Block::Code {
                id: "code".to_string(),
                language: "text".to_string(),
                code: "x".repeat(bytes),
                caption: None,
            }],
        })
    }

    #[test]
    fn worktree_store_round_trip_and_close_are_idempotent() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        assert_eq!(store.current_revision(session.id).unwrap().revision, 1);
        store.close(session.id).unwrap();
        store.close(session.id).unwrap();
        assert_eq!(
            store.load(session.id).unwrap().status,
            SessionStatus::Closed
        );
    }

    #[test]
    fn close_returns_the_locked_final_identity_across_a_browser_registration_race() {
        use std::sync::{Arc, Barrier};

        for _ in 0..32 {
            let (_temp, store) = store();
            let session = store.create(parsed()).unwrap();
            let instance = Uuid::new_v4();
            let barrier = Arc::new(Barrier::new(3));

            let set_store = store.clone();
            let set_barrier = Arc::clone(&barrier);
            let set = std::thread::spawn(move || {
                set_barrier.wait();
                set_store.set_browser(session.id, 42, instance)
            });
            let close_store = store.clone();
            let close_barrier = Arc::clone(&barrier);
            let close = std::thread::spawn(move || {
                close_barrier.wait();
                close_store.close(session.id)
            });
            barrier.wait();

            let set_result = set.join().unwrap();
            let closed = close.join().unwrap().unwrap();
            let persisted = store.load(session.id).unwrap();
            assert_eq!(closed, persisted);
            assert_eq!(persisted.status, SessionStatus::Closed);
            if set_result.is_ok() {
                assert_eq!(persisted.browser_pid, Some(42));
                assert_eq!(persisted.browser_instance, Some(instance));
            } else {
                assert_eq!(persisted.browser_pid, None);
                assert_eq!(persisted.browser_instance, None);
            }
        }
    }

    #[test]
    fn project_quota_serializes_creates_and_rolls_back_the_losing_session() {
        use std::sync::{Arc, Barrier};

        let (_temp, mut store) = store();
        store.retention.max_project_bytes = 1024 * 1024;
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.create(parsed_code(600 * 1024))
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(
            directory_size_bounded(store.root(), store.retention.max_project_bytes).unwrap()
                <= store.retention.max_project_bytes
        );
    }

    #[test]
    fn project_quota_serializes_updates_before_publication() {
        use std::sync::{Arc, Barrier};

        let (_temp, mut store) = store();
        let first = store.create(parsed()).unwrap();
        let second = store.create(parsed()).unwrap();
        store.retention.max_project_bytes = 1024 * 1024;
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for session in [first.id, second.id] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.update_document(session, parsed_code(600 * 1024))
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
        assert!(
            directory_size_bounded(store.root(), store.retention.max_project_bytes).unwrap()
                <= store.retention.max_project_bytes
        );
        assert!(!store.session_dir(first.id).join(UPDATE_MARKER).exists());
        assert!(!store.session_dir(second.id).join(UPDATE_MARKER).exists());
    }

    #[test]
    fn overquota_control_paths_can_close_release_runtime_and_clear() {
        let (_temp, mut store) = store();
        let session = store.create(parsed()).unwrap();
        let instance = Uuid::new_v4();
        store.set_browser(session.id, 42, instance).unwrap();
        let current = directory_size_bounded(store.root(), u64::MAX - 1).unwrap();
        store.retention.max_project_bytes = current.saturating_sub(1);

        assert!(store.runtime_dir(session.id).is_ok());
        assert_eq!(
            store.close(session.id).unwrap().status,
            SessionStatus::Closed
        );
        assert!(store.clear_browser(session.id, instance).unwrap());
        assert_eq!(
            store
                .clear(Some(session.id), Duration::ZERO, false)
                .unwrap(),
            vec![session.id]
        );
    }

    #[test]
    fn impossible_single_session_quota_fails_before_publication() {
        let (_temp, mut store) = store();
        store.retention.max_project_bytes = 1024 * 1024;
        assert!(store.create(parsed_code(1024 * 1024)).is_err());
        assert!(fs::read_dir(store.root().join("sessions"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".creating-")));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn update_appends_an_immutable_revision_and_history_excludes_runtime_identity() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let mut update = parsed();
        if let ParsedDocument::Supported(document) = &mut update {
            document.title = "Updated".to_string();
        }
        assert_eq!(store.update_document(session.id, update).unwrap(), 2);
        assert_eq!(store.revision(session.id, 1).unwrap().revision, 1);
        assert_eq!(store.current_revision(session.id).unwrap().revision, 2);

        let history = store.history(session.id).unwrap();
        assert_eq!(history.revisions.len(), 2);
        let serialized = serde_json::to_string(&history).unwrap();
        for private_name in [
            "service_port",
            "service_pid",
            "service_instance",
            "browser_pid",
            "browser_instance",
        ] {
            assert!(!serialized.contains(private_name));
        }
    }

    #[test]
    fn interrupted_update_markers_roll_back_or_finalize_deterministically() {
        let (_temp, store) = store();

        let marker_only = store.create(parsed()).unwrap();
        write_json_atomic(
            &store.session_dir(marker_only.id).join(UPDATE_MARKER),
            &UpdateMarker {
                from_revision: 1,
                to_revision: 2,
            },
        )
        .unwrap();
        assert_eq!(store.update_document(marker_only.id, parsed()).unwrap(), 2);

        let orphan = store.create(parsed()).unwrap();
        write_json_atomic(
            &store.session_dir(orphan.id).join(UPDATE_MARKER),
            &UpdateMarker {
                from_revision: 1,
                to_revision: 2,
            },
        )
        .unwrap();
        write_json_atomic(
            &store.revision_path(orphan.id, 2),
            &RevisionRecord {
                state_schema_version: STATE_SCHEMA_VERSION,
                revision: 2,
                created_at_unix: now_unix().unwrap(),
                content: RevisionContent::Supported {
                    document: match parsed() {
                        ParsedDocument::Supported(document) => document,
                        ParsedDocument::Unsupported { .. } => unreachable!(),
                    },
                },
            },
        )
        .unwrap();
        assert_eq!(store.update_document(orphan.id, parsed()).unwrap(), 2);

        let committed = store.create(parsed()).unwrap();
        let revision = RevisionRecord {
            state_schema_version: STATE_SCHEMA_VERSION,
            revision: 2,
            created_at_unix: now_unix().unwrap(),
            content: RevisionContent::Supported {
                document: match parsed() {
                    ParsedDocument::Supported(document) => document,
                    ParsedDocument::Unsupported { .. } => unreachable!(),
                },
            },
        };
        write_json_atomic(&store.revision_path(committed.id, 2), &revision).unwrap();
        let mut session = store.load(committed.id).unwrap();
        session.current_revision = 2;
        write_json_atomic(&store.session_path(committed.id), &session).unwrap();
        write_json_atomic(
            &store.session_dir(committed.id).join(UPDATE_MARKER),
            &UpdateMarker {
                from_revision: 1,
                to_revision: 2,
            },
        )
        .unwrap();
        assert_eq!(store.update_document(committed.id, parsed()).unwrap(), 3);
    }

    #[test]
    fn stale_unpublished_create_is_removed_without_affecting_sessions() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let staged_id = Uuid::new_v4();
        let nonce = Uuid::new_v4();
        let staged = store
            .root
            .join("sessions")
            .join(format!(".creating-{staged_id}-{}", nonce.simple()));
        create_private_dir_all(&staged).unwrap();
        write_json_atomic(
            &staged.join(CREATE_TRANSACTION_MARKER),
            &CreateTransactionMarker {
                schema_version: 1,
                session_id: staged_id,
                nonce,
            },
        )
        .unwrap();
        fs::write(staged.join("partial"), b"partial").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(!staged.exists());
        assert!(store.load(session.id).is_ok());
    }

    #[test]
    fn interrupted_trash_and_atomic_temps_are_recovered_under_project_lock() {
        let (_temp, store) = store();
        let kept = store.create(parsed()).unwrap();
        let removed = store.create(parsed()).unwrap();
        let sessions = store.root.join("sessions");
        let nonce = Uuid::new_v4();
        write_json_atomic(
            &store
                .session_dir(removed.id)
                .join(DELETE_TRANSACTION_MARKER),
            &DeleteTransactionMarker {
                schema_version: 1,
                session_id: removed.id,
                nonce,
            },
        )
        .unwrap();
        let trash = sessions.join(format!(".trash-{}-{}", removed.id, nonce.simple()));
        fs::rename(store.session_dir(removed.id), &trash).unwrap();

        let session_temp = store
            .session_dir(kept.id)
            .join(format!(".session.json.{}.tmp", Uuid::new_v4()));
        create_private_file(&session_temp).unwrap();
        let revision_temp = store
            .session_dir(kept.id)
            .join("revisions")
            .join(format!(".00000000000000000002.json.{}.tmp", Uuid::new_v4()));
        create_private_file(&revision_temp).unwrap();

        assert_eq!(store.list().unwrap().len(), 1);
        assert!(!trash.exists());
        assert!(!session_temp.exists());
        assert!(!revision_temp.exists());
        assert!(store.load(kept.id).is_ok());
    }

    #[test]
    fn recovery_rejects_exact_transaction_names_without_matching_proof() {
        let (_temp, store) = store();
        let sessions = store.root.join("sessions");
        let staged_id = Uuid::new_v4();
        let staged_nonce = Uuid::new_v4();
        let staged = sessions.join(format!(".creating-{staged_id}-{}", staged_nonce.simple()));
        create_private_dir_all(&staged).unwrap();
        assert!(store.list().is_err());
        assert!(staged.exists());
        fs::remove_dir_all(&staged).unwrap();

        let session = store.create(parsed()).unwrap();
        let trash_nonce = Uuid::new_v4();
        let trash = sessions.join(format!(".trash-{}-{}", session.id, trash_nonce.simple()));
        fs::rename(store.session_dir(session.id), &trash).unwrap();
        assert!(store.list().is_err());
        assert!(trash.exists());
    }

    #[test]
    fn unowned_legacy_runtime_is_migrated_out_of_durable_state() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let legacy = store.session_dir(session.id).join("runtime");
        create_private_dir_all(&legacy).unwrap();
        create_private_file(&legacy.join("legacy-control")).unwrap();

        store.close(session.id).unwrap();
        assert!(!legacy.exists());
        assert!(store
            .runtime_root()
            .join(session.id.to_string())
            .join("legacy-control")
            .exists());
    }

    #[test]
    fn one_shot_history_refuses_an_unsafe_aggregate_envelope() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let events = open_private_append(&store.events_path(session.id)).unwrap();
        events.set_len(limits::MAX_HISTORY_READ_BYTES + 1).unwrap();
        assert!(matches!(
            store.history(session.id),
            Err(PresentError::ServiceUnavailable(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn session_directory_symlink_is_rejected_before_lock_or_read() {
        use std::os::unix::fs::symlink;

        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let path = store.session_dir(session.id);
        let target = store.root.join("relocated-session");
        fs::rename(&path, &target).unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(
            store.load(session.id),
            Err(PresentError::UnsafePath(_))
        ));
        assert!(matches!(
            store.acquire_service_lease(session.id),
            Err(PresentError::UnsafePath(_))
        ));
    }

    #[test]
    fn corrupted_or_exhausted_revision_metadata_fails_before_allocation_or_iteration() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let mut corrupted = session.clone();
        corrupted.current_revision = limits::MAX_REVISIONS + 1;
        write_json_atomic(&store.session_path(session.id), &corrupted).unwrap();
        assert!(matches!(
            store.load(session.id),
            Err(PresentError::CorruptState(_))
        ));
        assert!(store.list().is_err());

        corrupted.current_revision = limits::MAX_REVISIONS;
        write_json_atomic(&store.session_path(session.id), &corrupted).unwrap();
        assert!(matches!(
            store.update_document(session.id, parsed()),
            Err(PresentError::InvalidDocument(_))
        ));
    }

    #[test]
    fn request_changes_requires_instruction() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let envelope = FeedbackEnvelope {
            event_id: Uuid::new_v4(),
            session_id: session.id,
            revision: 1,
            actor: "operator".to_string(),
            verdict: FeedbackVerdict::RequestChanges,
            instruction: None,
            notes: Vec::new(),
            created_at_unix: 0,
        };
        assert!(store.append_feedback(envelope).is_err());
    }

    #[test]
    fn feedback_resolution_requires_current_version_and_reanchoring_never_guesses() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        let mut envelope = feedback(session.id, event_id);
        envelope.notes.push(FeedbackNote {
            id: Uuid::new_v4(),
            block_id: "intro".to_string(),
            block_label: "intro".to_string(),
            kind: FeedbackKind::Comment,
            body: "Keep this wording.".to_string(),
            selector: Some(TextSelector {
                exact: "Hello".to_string(),
                prefix: String::new(),
                suffix: String::new(),
                start_utf16: 0,
                end_utf16: 5,
            }),
        });
        assert_eq!(store.append_feedback(envelope).unwrap().sequence, 1);
        store.mark_delivered(session.id, &[event_id]).unwrap();
        assert!(store
            .resolve_feedback(session.id, event_id, 1, FeedbackResolution::Addressed)
            .is_err());
        assert_eq!(
            store
                .resolve_feedback(session.id, event_id, 2, FeedbackResolution::Addressed)
                .unwrap(),
            3
        );

        let mut moved = parsed();
        let ParsedDocument::Supported(document) = &mut moved else {
            unreachable!()
        };
        let Block::Narrative { markdown, .. } = &mut document.blocks[0] else {
            unreachable!()
        };
        *markdown = "Before Hello after".to_string();
        store.update_document(session.id, moved).unwrap();
        let snapshot = store.feedback_snapshot(session.id).unwrap();
        assert_eq!(snapshot.items[0].lifecycle, FeedbackLifecycle::Addressed);
        assert!(matches!(
            snapshot.items[0].notes[0].anchor,
            FeedbackAnchor::Reanchored {
                start_utf16: 7,
                end_utf16: 12
            }
        ));

        let mut ambiguous = parsed();
        let ParsedDocument::Supported(document) = &mut ambiguous else {
            unreachable!()
        };
        let Block::Narrative { markdown, .. } = &mut document.blocks[0] else {
            unreachable!()
        };
        *markdown = "Hello and Hello".to_string();
        store.update_document(session.id, ambiguous).unwrap();
        assert!(matches!(
            store.feedback_snapshot(session.id).unwrap().items[0].notes[0].anchor,
            FeedbackAnchor::Orphaned { .. }
        ));
    }

    #[test]
    fn interrupted_final_event_is_ignored_but_middle_corruption_is_loud() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let path = store.events_path(session.id);
        fs::write(&path, b"{\"event\":\"received\"").unwrap();
        assert!(store.events(session.id).unwrap().is_empty());
        fs::write(&path, b"not-json\n{}\n").unwrap();
        assert!(store.events(session.id).is_err());
    }

    #[test]
    fn newline_dense_event_log_is_parsed_without_an_intermediate_line_index() {
        let path = Path::new("events.jsonl");
        let event_id = Uuid::new_v4();
        let event = FeedbackEvent::Received {
            sequence: 1,
            envelope: feedback(Uuid::new_v4(), event_id),
        };
        let mut raw = "\n".repeat(2 * 1024 * 1024);
        raw.push_str(&serde_json::to_string(&event).unwrap());
        raw.push('\n');
        raw.push_str("{\"event\":");

        assert_eq!(parse_event_log(path, &raw, 2).unwrap(), vec![event]);
    }

    #[test]
    fn event_log_fails_as_soon_as_count_or_sequence_exceeds_its_contract() {
        let event_id = Uuid::new_v4();
        let first = FeedbackEvent::Received {
            sequence: 1,
            envelope: feedback(Uuid::new_v4(), event_id),
        };
        let second = FeedbackEvent::Delivered {
            sequence: 2,
            event_id,
            at_unix: 0,
        };
        let raw = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
        assert!(parse_event_log(Path::new("events.jsonl"), &raw, 1).is_err());

        let gap = FeedbackEvent::Delivered {
            sequence: 3,
            event_id,
            at_unix: 0,
        };
        let raw = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&gap).unwrap()
        );
        assert!(parse_event_log(Path::new("events.jsonl"), &raw, 2).is_err());
    }

    #[test]
    fn append_repairs_an_interrupted_tail_before_writing_the_next_event() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let path = store.events_path(session.id);
        fs::write(&path, b"{\"event\":\"received\"").unwrap();

        assert_eq!(
            store
                .append_feedback(feedback(session.id, Uuid::new_v4()))
                .unwrap(),
            FeedbackAppend {
                sequence: 1,
                created: true
            }
        );
        assert_eq!(store.events(session.id).unwrap().len(), 1);
        assert!(fs::read_to_string(path).unwrap().ends_with('\n'));
    }

    #[test]
    fn append_refuses_oversized_logs_and_unrecoverable_partial_records() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let path = store.events_path(session.id);
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(limits::MAX_EVENT_LOG_BYTES + 1).unwrap();
        assert!(matches!(
            store.append_feedback(feedback(session.id, Uuid::new_v4())),
            Err(PresentError::CorruptState(_))
        ));
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            limits::MAX_EVENT_LOG_BYTES + 1
        );

        file.set_len(limits::MAX_EVENT_RECORD_BYTES * 2 + 1)
            .unwrap();
        assert!(matches!(
            store.append_feedback(feedback(session.id, Uuid::new_v4())),
            Err(PresentError::CorruptState(_))
        ));
    }

    #[test]
    fn exact_feedback_retry_is_idempotent_but_conflicting_reuse_is_loud() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        let envelope = feedback(session.id, event_id);

        assert!(store.append_feedback(envelope.clone()).unwrap().created);
        assert!(!store.append_feedback(envelope.clone()).unwrap().created);
        assert_eq!(store.events(session.id).unwrap().len(), 1);

        store.update_document(session.id, parsed()).unwrap();
        store.close(session.id).unwrap();
        assert!(!store.append_feedback(envelope.clone()).unwrap().created);
        assert_eq!(store.events(session.id).unwrap().len(), 1);

        let mut conflicting = envelope;
        conflicting.actor = "another operator".to_string();
        assert!(store.append_feedback(conflicting).is_err());
        assert_eq!(store.events(session.id).unwrap().len(), 1);
    }

    #[test]
    fn terminal_resolution_retries_are_idempotent_and_conflicts_are_loud() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        store
            .append_feedback(feedback(session.id, event_id))
            .unwrap();
        store.mark_delivered(session.id, &[event_id]).unwrap();

        assert_eq!(
            store
                .resolve_feedback(session.id, event_id, 2, FeedbackResolution::Addressed)
                .unwrap(),
            3
        );
        assert_eq!(
            store
                .resolve_feedback(session.id, event_id, 2, FeedbackResolution::Addressed)
                .unwrap(),
            3
        );
        assert_eq!(
            store
                .resolve_feedback(session.id, event_id, 3, FeedbackResolution::Addressed)
                .unwrap(),
            3
        );
        assert!(store
            .resolve_feedback(session.id, event_id, 3, FeedbackResolution::Dismissed)
            .is_err());
        assert!(store
            .resolve_feedback(session.id, event_id, 1, FeedbackResolution::Addressed)
            .is_err());
        assert_eq!(store.events(session.id).unwrap().len(), 3);
    }

    #[test]
    fn concurrent_terminal_retries_converge_and_conflicts_never_overwrite() {
        use std::sync::{Arc, Barrier};

        let (_temp, store) = store();
        let store = Arc::new(store);
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        store
            .append_feedback(feedback(session.id, event_id))
            .unwrap();
        store.mark_delivered(session.id, &[event_id]).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.resolve_feedback(session.id, event_id, 2, FeedbackResolution::Addressed)
            }));
        }
        barrier.wait();
        for worker in workers {
            assert_eq!(worker.join().unwrap().unwrap(), 3);
        }
        assert_eq!(store.events(session.id).unwrap().len(), 3);

        let conflicting_id = Uuid::new_v4();
        store
            .append_feedback(feedback(session.id, conflicting_id))
            .unwrap();
        store.mark_delivered(session.id, &[conflicting_id]).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let left_store = Arc::clone(&store);
        let left_barrier = Arc::clone(&barrier);
        let left = std::thread::spawn(move || {
            left_barrier.wait();
            left_store.resolve_feedback(
                session.id,
                conflicting_id,
                5,
                FeedbackResolution::Addressed,
            )
        });
        let right_store = Arc::clone(&store);
        let right_barrier = Arc::clone(&barrier);
        let right = std::thread::spawn(move || {
            right_barrier.wait();
            right_store.resolve_feedback(
                session.id,
                conflicting_id,
                5,
                FeedbackResolution::Dismissed,
            )
        });
        barrier.wait();
        let outcomes = [left.join().unwrap(), right.join().unwrap()];
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|result| result.is_err()).count(), 1);
    }

    #[test]
    fn overquota_feedback_retries_are_zero_growth_idempotent_operations() {
        let (_temp, mut store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        let envelope = feedback(session.id, event_id);
        store.append_feedback(envelope.clone()).unwrap();
        store.mark_delivered(session.id, &[event_id]).unwrap();
        let current = directory_size_bounded(store.root(), u64::MAX - 1).unwrap();
        store.retention.max_project_bytes = current.saturating_sub(1);

        assert_eq!(
            store.append_feedback(envelope).unwrap(),
            FeedbackAppend {
                sequence: 1,
                created: false
            }
        );
        store.mark_delivered(session.id, &[event_id]).unwrap();
        assert_eq!(store.events(session.id).unwrap().len(), 2);
    }

    #[test]
    fn concurrent_delivery_is_one_durable_transition() {
        use std::sync::{Arc, Barrier};

        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        store
            .append_feedback(feedback(session.id, event_id))
            .unwrap();
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.mark_delivered(session.id, &[event_id])
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(store.events(session.id).unwrap().len(), 2);
    }

    #[test]
    fn malformed_feedback_lifecycle_logs_fail_closed() {
        let path = Path::new("events.jsonl");
        let session_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let received = FeedbackEvent::Received {
            sequence: 1,
            envelope: feedback(session_id, event_id),
        };
        let delivered = FeedbackEvent::Delivered {
            sequence: 2,
            event_id,
            at_unix: 0,
        };
        let addressed = FeedbackEvent::Addressed {
            sequence: 3,
            event_id,
            at_unix: 0,
        };
        let post_terminal = FeedbackEvent::Delivered {
            sequence: 4,
            event_id,
            at_unix: 0,
        };
        let invalid = format!(
            "{}\n{}\n{}\n{}\n",
            serde_json::to_string(&received).unwrap(),
            serde_json::to_string(&delivered).unwrap(),
            serde_json::to_string(&addressed).unwrap(),
            serde_json::to_string(&post_terminal).unwrap()
        );
        assert!(matches!(
            parse_event_log(path, &invalid, 8),
            Err(PresentError::CorruptState(_))
        ));

        let duplicate = FeedbackEvent::Received {
            sequence: 2,
            envelope: feedback(session_id, event_id),
        };
        let invalid = format!(
            "{}\n{}\n",
            serde_json::to_string(&received).unwrap(),
            serde_json::to_string(&duplicate).unwrap()
        );
        assert!(parse_event_log(path, &invalid, 8).is_err());
    }

    #[test]
    fn feedback_anchor_and_label_must_match_the_exact_revision_text() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let mut envelope = feedback(session.id, Uuid::new_v4());
        envelope.notes.push(FeedbackNote {
            id: Uuid::new_v4(),
            block_id: "intro".to_string(),
            block_label: "intro".to_string(),
            kind: FeedbackKind::Comment,
            body: "Explain this.".to_string(),
            selector: Some(TextSelector {
                exact: "ell".to_string(),
                prefix: "H".to_string(),
                suffix: "o".to_string(),
                start_utf16: 1,
                end_utf16: 4,
            }),
        });
        assert!(store.append_feedback(envelope.clone()).is_ok());

        envelope.event_id = Uuid::new_v4();
        envelope.notes[0].selector.as_mut().unwrap().exact = "fake".to_string();
        assert!(store.append_feedback(envelope.clone()).is_err());
        envelope.notes[0].selector.as_mut().unwrap().exact = "ell".to_string();
        envelope.notes[0].block_label = "fabricated".to_string();
        assert!(store.append_feedback(envelope).is_err());
    }

    #[test]
    fn service_lease_prevents_duplicate_owners_and_releases_cleanly() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let lease = store.acquire_service_lease(session.id).unwrap();
        assert!(matches!(
            store.acquire_service_lease(session.id),
            Err(PresentError::ServiceUnavailable(_))
        ));
        drop(lease);
        assert!(store.acquire_service_lease(session.id).is_ok());

        let startup = store.acquire_startup_lease(session.id).unwrap();
        assert!(matches!(
            store.acquire_startup_lease(session.id),
            Err(PresentError::ServiceUnavailable(_))
        ));
        drop(startup);
        assert!(store.acquire_startup_lease(session.id).is_ok());
    }

    #[test]
    fn clear_removes_only_closed_selected_session_via_tombstone_rename() {
        let (_temp, store) = store();
        let closed = store.create(parsed()).unwrap();
        let active = store.create(parsed()).unwrap();
        store.close(closed.id).unwrap();

        assert_eq!(
            store.clear(Some(closed.id), Duration::ZERO, false).unwrap(),
            vec![closed.id]
        );
        assert!(matches!(
            store.load(closed.id),
            Err(PresentError::SessionNotFound(_))
        ));
        assert_eq!(store.load(active.id).unwrap().status, SessionStatus::Active);
    }

    #[test]
    fn selected_clear_isolated_from_unrelated_corruption_and_bulk_reports_partial_progress() {
        let (_temp, store) = store();
        let selected = store.create(parsed()).unwrap();
        let corrupt = store.create(parsed()).unwrap();
        store.close(selected.id).unwrap();
        store.close(corrupt.id).unwrap();
        fs::write(store.session_path(corrupt.id), b"not-json").unwrap();
        let unrelated_staging = store.root.join("sessions/.creating-unrelated");
        create_private_dir_all(&unrelated_staging).unwrap();

        assert_eq!(
            store
                .clear(Some(selected.id), Duration::ZERO, false)
                .unwrap(),
            vec![selected.id]
        );
        assert!(unrelated_staging.exists());
        fs::remove_dir_all(&unrelated_staging).unwrap();
        let error = store.clear(None, Duration::ZERO, false).unwrap_err();
        assert!(matches!(error, PresentError::PartialCleanup { .. }));
        assert!(store.session_dir(corrupt.id).exists());
    }

    #[cfg(unix)]
    #[test]
    fn unix_home_root_must_be_absolute() {
        assert!(absolute_xdg_state_home(Some("relative/state".into())).is_none());
        assert_eq!(
            absolute_xdg_state_home(Some("/private/state".into())),
            Some(PathBuf::from("/private/state"))
        );
        assert!(validated_home(Some("relative/home".into())).is_err());
        assert!(validated_home(None).is_err());
        assert_eq!(
            validated_home(Some("/private/home".into())).unwrap(),
            PathBuf::from("/private/home")
        );
    }

    #[test]
    fn selected_clear_reports_live_runtime_and_stale_service_state_converges() {
        let (_temp, store) = store();
        let browser = store.create(parsed()).unwrap();
        let live_service = store.create(parsed()).unwrap();
        let crashed_service = store.create(parsed()).unwrap();
        let browser_instance = Uuid::new_v4();
        let live_service_instance = Uuid::new_v4();
        let crashed_service_instance = Uuid::new_v4();
        store.set_browser(browser.id, 42, browser_instance).unwrap();
        let live_lease = store.acquire_service_lease(live_service.id).unwrap();
        store
            .set_service(live_service.id, 43123, 43, live_service_instance)
            .unwrap();
        store
            .set_service(crashed_service.id, 43124, 44, crashed_service_instance)
            .unwrap();
        store.close(browser.id).unwrap();
        store.close(live_service.id).unwrap();
        store.close(crashed_service.id).unwrap();

        assert!(matches!(
            store.clear(Some(browser.id), Duration::ZERO, false),
            Err(PresentError::PartialCleanup { .. })
        ));
        assert!(matches!(
            store.clear(Some(live_service.id), Duration::ZERO, false),
            Err(PresentError::PartialCleanup { .. })
        ));
        assert!(store.load(browser.id).is_ok());
        assert!(store.load(live_service.id).is_ok());

        assert_eq!(
            store
                .clear(Some(crashed_service.id), Duration::ZERO, false)
                .unwrap(),
            vec![crashed_service.id]
        );
        assert!(matches!(
            store.load(crashed_service.id),
            Err(PresentError::SessionNotFound(_))
        ));
        drop(live_lease);
    }

    #[test]
    fn automatic_retention_evicts_oldest_closed_state_and_preserves_active_state() {
        let (_temp, mut store) = store();
        store.retention.max_closed_sessions = 1;
        let first = store.create(parsed()).unwrap();
        let second = store.create(parsed()).unwrap();
        let active = store.create(parsed()).unwrap();
        store.close(first.id).unwrap();
        store.close(second.id).unwrap();
        store.enforce_retention().unwrap();

        let sessions = store.list().unwrap();
        assert_eq!(
            sessions
                .iter()
                .filter(|session| session.status == SessionStatus::Closed)
                .count(),
            1
        );
        assert_eq!(store.load(active.id).unwrap().status, SessionStatus::Active);
    }

    #[test]
    fn size_driven_retention_recomputes_until_multiple_evictions_fit() {
        let (_temp, mut store) = store();
        store.retention.max_closed_sessions = usize::MAX;
        let sessions = (0..3)
            .map(|_| {
                let session = store.create(parsed()).unwrap();
                store.close(session.id).unwrap();
                session
            })
            .collect::<Vec<_>>();
        let current = directory_size_bounded(store.root(), u64::MAX - 1).unwrap();
        let one = directory_size_bounded(&store.session_dir(sessions[0].id), u64::MAX - 1).unwrap();
        store.retention.max_project_bytes = current.saturating_sub(one.saturating_mul(2));

        let removed = store.enforce_retention().unwrap();
        assert!(removed.len() >= 2);
        assert!(
            directory_size_bounded(store.root(), store.retention.max_project_bytes).unwrap()
                <= store.retention.max_project_bytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_is_owner_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        assert_eq!(
            fs::metadata(store.session_dir(session.id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.session_path(session.id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
