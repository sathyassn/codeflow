use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    config::{ProjectConfig, RetentionPolicy},
    document::{ParsedDocument, PresentationDocument, Provenance},
    error::{PresentError, Result},
    limits,
};

const STATE_SCHEMA_VERSION: u32 = 1;

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

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
    project_key: String,
    retention: RetentionPolicy,
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
        let project_key = hex_digest(common.as_os_str().to_string_lossy().as_bytes());
        let worktree = repository.workdir().ok_or_else(|| {
            PresentError::InvalidDocument(
                "cf-present requires a non-bare Git working tree".to_string(),
            )
        })?;
        let config = ProjectConfig::load(worktree)?;
        let root = platform_state_root()?.join("projects").join(&project_key);
        create_private_dir_all(&root)?;
        ensure_safe_dir(&root)?;
        Ok(Self {
            root,
            project_key,
            retention: config.retention,
        })
    }

    #[cfg(test)]
    pub(crate) fn at_root(root: PathBuf, project_key: String) -> Result<Self> {
        create_private_dir_all(&root)?;
        Ok(Self {
            root,
            project_key,
            retention: RetentionPolicy::default(),
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn runtime_dir(&self, id: Uuid) -> Result<PathBuf> {
        self.load(id)?;
        let path = self.session_dir(id).join("runtime");
        create_private_dir_all(&path)?;
        Ok(path)
    }

    pub fn create(&self, parsed: ParsedDocument) -> Result<SessionRecord> {
        self.enforce_retention()?;
        let id = Uuid::new_v4();
        create_private_dir_all(&self.root.join("sessions"))?;
        let session_dir = self.session_dir(id);
        create_private_dir_all(&session_dir)?;
        create_private_dir_all(&session_dir.join("revisions"))?;
        let _lock = self.lock_session(id)?;
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
            provenance,
        };
        write_json_atomic(
            &self.revision_path(id, 1),
            &RevisionRecord {
                state_schema_version: STATE_SCHEMA_VERSION,
                revision: 1,
                created_at_unix: now,
                content,
            },
        )?;
        write_json_atomic(&self.session_path(id), &session)?;
        create_private_file(&self.events_path(id))?;
        Ok(session)
    }

    pub fn load(&self, id: Uuid) -> Result<SessionRecord> {
        let path = self.session_path(id);
        let session: SessionRecord = read_json(&path)?;
        self.validate_session(&session)?;
        Ok(session)
    }

    pub fn list(&self) -> Result<Vec<SessionRecord>> {
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
            let file_type = entry
                .file_type()
                .map_err(|error| PresentError::io(entry.path(), error))?;
            if !file_type.is_dir() || file_type.is_symlink() {
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

    pub fn revision(&self, id: Uuid, revision: u64) -> Result<RevisionRecord> {
        let session = self.load(id)?;
        if revision == 0 || revision > session.current_revision || revision > limits::MAX_REVISIONS
        {
            return Err(PresentError::CorruptState(format!(
                "revision {revision} is outside session {}",
                session.id
            )));
        }
        let record: RevisionRecord = read_json(&self.revision_path(id, revision))?;
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
        let _lock = self.lock_session(id)?;
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
        write_json_atomic(
            &self.revision_path(id, revision),
            &RevisionRecord {
                state_schema_version: STATE_SCHEMA_VERSION,
                revision,
                created_at_unix: now,
                content,
            },
        )?;
        session.title = title;
        session.provenance = provenance;
        session.current_revision = revision;
        session.updated_at_unix = now;
        write_json_atomic(&self.session_path(id), &session)?;
        Ok(revision)
    }

    pub fn history(&self, id: Uuid) -> Result<SessionHistory> {
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        let capacity = usize::try_from(session.current_revision).map_err(|_| {
            PresentError::CorruptState("presentation revision count is not addressable".to_string())
        })?;
        let mut revisions = Vec::with_capacity(capacity);
        for revision in 1..=session.current_revision {
            let record: RevisionRecord = read_json(&self.revision_path(id, revision))?;
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
        let _lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        session.service_port = Some(port);
        session.service_pid = Some(pid);
        session.service_instance = Some(instance);
        session.updated_at_unix = now_unix()?;
        write_json_atomic(&self.session_path(id), &session)
    }

    pub fn close(&self, id: Uuid) -> Result<()> {
        let lock = self.lock_session(id)?;
        let mut session = self.load(id)?;
        if session.status == SessionStatus::Closed {
            return Ok(());
        }
        let now = now_unix()?;
        session.status = SessionStatus::Closed;
        session.closed_at_unix = Some(now);
        session.updated_at_unix = now;
        write_json_atomic(&self.session_path(id), &session)?;
        drop(lock);
        self.enforce_retention()?;
        Ok(())
    }

    pub fn enforce_retention(&self) -> Result<Vec<Uuid>> {
        let now = now_unix()?;
        let mut closed: Vec<_> = self
            .list()?
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
            let cleared = self.clear(Some(session.id), Duration::ZERO, false)?;
            if cleared.contains(&session.id) {
                closed_count = closed_count.saturating_sub(1);
                total_bytes = directory_size_bounded(&self.root, self.retention.max_project_bytes)?;
                removed.push(session.id);
            }
        }
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
        let _lock = self.lock_session(id)?;
        let session = self.load(id)?;
        if session.status != SessionStatus::Active {
            return Err(PresentError::SessionClosed(id.to_string()));
        }
        let revision = self.revision(id, session.current_revision)?;
        validate_feedback(&envelope, &session, &revision.content)?;
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
        envelope.created_at_unix = now_unix()?;
        let sequence = next_sequence(&events)?;
        self.append_event_unlocked(id, &FeedbackEvent::Received { sequence, envelope })?;
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
        self.read_last_event_unlocked(id)
            .map(|event| event.map_or(0, |event| event.sequence()))
    }

    pub fn pending_feedback(&self, id: Uuid) -> Result<Vec<FeedbackEnvelope>> {
        let events = self.events(id)?;
        let delivered: std::collections::HashSet<_> = events
            .iter()
            .filter_map(|event| match event {
                FeedbackEvent::Delivered { event_id, .. }
                | FeedbackEvent::Addressed { event_id, .. }
                | FeedbackEvent::Dismissed { event_id, .. } => Some(*event_id),
                FeedbackEvent::Received { .. } => None,
            })
            .collect();
        Ok(events
            .into_iter()
            .filter_map(|event| match event {
                FeedbackEvent::Received { envelope, .. }
                    if !delivered.contains(&envelope.event_id) =>
                {
                    Some(envelope)
                }
                _ => None,
            })
            .collect())
    }

    pub fn mark_delivered(&self, id: Uuid, event_ids: &[Uuid]) -> Result<()> {
        let _lock = self.lock_session(id)?;
        let events = self.read_events_unlocked(id)?;
        let received: std::collections::HashSet<_> = events
            .iter()
            .filter_map(|event| match event {
                FeedbackEvent::Received { envelope, .. } => Some(envelope.event_id),
                _ => None,
            })
            .collect();
        let mut sequence = next_sequence(&events)?;
        for event_id in event_ids {
            if !received.contains(event_id) {
                return Err(PresentError::CorruptState(format!(
                    "cannot deliver unknown event {event_id}"
                )));
            }
            self.append_event_unlocked(
                id,
                &FeedbackEvent::Delivered {
                    sequence,
                    event_id: *event_id,
                    at_unix: now_unix()?,
                },
            )?;
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| PresentError::CorruptState("event sequence overflow".to_string()))?;
        }
        Ok(())
    }

    pub fn clear(
        &self,
        selected: Option<Uuid>,
        older_than: Duration,
        dry_run: bool,
    ) -> Result<Vec<Uuid>> {
        let now = now_unix()?;
        let mut removed = Vec::new();
        for session in self.list()? {
            if selected.is_some_and(|selected| selected != session.id) {
                continue;
            }
            let Some(closed_at) = session.closed_at_unix else {
                continue;
            };
            if now.saturating_sub(closed_at) < older_than.as_secs() && selected.is_none() {
                continue;
            }
            let lock = match self.lock_session_nonblocking_raw(session.id)? {
                Some(lock) => lock,
                None => continue,
            };
            let path = self.session_dir(session.id);
            ensure_confined_child(&self.root.join("sessions"), &path)?;
            if !dry_run {
                let tombstone = path.join(".deleting");
                create_private_file(&tombstone)?;
                sync_directory(&path)?;
                drop(lock);
                let trash = self.root.join("sessions").join(format!(
                    ".trash-{}-{}",
                    session.id,
                    Uuid::new_v4().simple()
                ));
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
            }
            removed.push(session.id);
        }
        Ok(removed)
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
        open_private_append(&dir.join(".lock"))
    }

    fn reject_tombstone(&self, id: Uuid) -> Result<()> {
        if self.session_dir(id).join(".deleting").exists() {
            return Err(PresentError::SessionNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn acquire_service_lease(&self, id: Uuid) -> Result<File> {
        self.load(id)?;
        let path = self.session_dir(id).join(".service.lock");
        let lease = open_private_append(&path)?;
        match lease.try_lock_exclusive() {
            Ok(()) => Ok(lease),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(PresentError::ServiceUnavailable(format!(
                    "presentation session {id} already has a running service"
                )))
            }
            Err(error) => Err(PresentError::io(path, error)),
        }
    }

    fn read_events_unlocked(&self, id: Uuid) -> Result<Vec<FeedbackEvent>> {
        let path = self.events_path(id);
        let metadata = fs::metadata(&path).map_err(|error| PresentError::io(&path, error))?;
        if metadata.len() > limits::MAX_EVENT_LOG_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} exceeds the {} byte event-log bound",
                path.display(),
                limits::MAX_EVENT_LOG_BYTES
            )));
        }
        let mut raw = String::new();
        File::open(&path)
            .map_err(|error| PresentError::io(&path, error))?
            .read_to_string(&mut raw)
            .map_err(|error| PresentError::io(&path, error))?;
        let mut events = Vec::new();
        let lines = raw.split_inclusive('\n').collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
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
                Ok(event) => events.push(event),
                Err(_) if index + 1 == lines.len() && !line.ends_with('\n') => break,
                Err(error) => {
                    return Err(PresentError::CorruptState(format!(
                        "{} line {}: {error}",
                        path.display(),
                        index + 1
                    )));
                }
            }
        }
        if events.len() > limits::MAX_FEEDBACK_EVENTS {
            return Err(PresentError::CorruptState(format!(
                "{} exceeds the {} event bound",
                path.display(),
                limits::MAX_FEEDBACK_EVENTS
            )));
        }
        for pair in events.windows(2) {
            if pair[1].sequence() != pair[0].sequence() + 1 {
                return Err(PresentError::CorruptState(format!(
                    "{} has a non-contiguous event sequence",
                    path.display()
                )));
            }
        }
        Ok(events)
    }

    fn append_event_unlocked(&self, id: Uuid, event: &FeedbackEvent) -> Result<()> {
        let path = self.events_path(id);
        repair_partial_tail(&path)?;
        let encoded = serde_json::to_vec(event)?;
        if encoded.len() as u64 > limits::MAX_EVENT_RECORD_BYTES {
            return Err(PresentError::InvalidDocument(
                "feedback event exceeds the event-record bound".to_string(),
            ));
        }
        let current = fs::metadata(&path)
            .map_err(|error| PresentError::io(&path, error))?
            .len();
        if current
            .checked_add(encoded.len() as u64 + 1)
            .is_none_or(|size| size > limits::MAX_EVENT_LOG_BYTES)
        {
            return Err(PresentError::InvalidDocument(
                "feedback history reached its bounded event-log capacity".to_string(),
            ));
        }
        let mut file = open_private_append(&path)?;
        file.write_all(&encoded)
            .and_then(|()| file.write_all(b"\n"))
            .map_err(|error| PresentError::io(&path, error))?;
        file.sync_data()
            .map_err(|error| PresentError::io(&path, error))
    }

    fn read_last_event_unlocked(&self, id: Uuid) -> Result<Option<FeedbackEvent>> {
        let path = self.events_path(id);
        let mut file = File::open(&path).map_err(|error| PresentError::io(&path, error))?;
        let length = file
            .metadata()
            .map_err(|error| PresentError::io(&path, error))?
            .len();
        if length == 0 {
            return Ok(None);
        }
        if length > limits::MAX_EVENT_LOG_BYTES {
            return Err(PresentError::CorruptState(format!(
                "{} exceeds the event-log bound",
                path.display()
            )));
        }
        let window = (limits::MAX_EVENT_RECORD_BYTES * 2).min(length);
        file.seek(SeekFrom::Start(length - window))
            .map_err(|error| PresentError::io(&path, error))?;
        let mut bytes = Vec::with_capacity(window as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| PresentError::io(&path, error))?;
        if window < length {
            let first_newline = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .ok_or_else(|| {
                    PresentError::CorruptState("event tail has an oversized record".to_string())
                })?;
            bytes.drain(..=first_newline);
        }
        let complete_end = if bytes.last() == Some(&b'\n') {
            bytes.len()
        } else {
            bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1)
        };
        let complete = &bytes[..complete_end];
        let Some(line) = complete
            .split(|byte| *byte == b'\n')
            .rev()
            .find(|line| !line.is_empty())
        else {
            return Ok(None);
        };
        if line.len() as u64 > limits::MAX_EVENT_RECORD_BYTES {
            return Err(PresentError::CorruptState(
                "event tail has an oversized record".to_string(),
            ));
        }
        serde_json::from_slice(line)
            .map(Some)
            .map_err(|error| PresentError::CorruptState(format!("invalid event tail: {error}")))
    }
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
    let mut bytes = envelope.actor.len() + envelope.instruction.as_deref().map_or(0, str::len);
    let mut ids = std::collections::HashSet::new();
    if envelope.notes.len() > 100 {
        return Err(PresentError::InvalidDocument(
            "a review may contain at most 100 notes".to_string(),
        ));
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

fn next_sequence(events: &[FeedbackEvent]) -> Result<u64> {
    events.last().map_or(Ok(1), |event| {
        event
            .sequence()
            .checked_add(1)
            .ok_or_else(|| PresentError::CorruptState("event sequence overflow".to_string()))
    })
}

fn platform_state_root() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| PresentError::UnsafePath(PathBuf::from("%LOCALAPPDATA%")))?;
        return Ok(root.join("codeflow").join("present"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        return Ok(home
            .join("Library")
            .join("Application Support")
            .join("codeflow")
            .join("present"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(root).join("codeflow").join("present"));
        }
        Ok(home_dir()?.join(".local/state/codeflow/present"))
    }
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| PresentError::UnsafePath(PathBuf::from("$HOME")))
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
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn create_private_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| PresentError::io(path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| PresentError::io(path, error))?;
    }
    Ok(())
}

fn create_private_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let file = open_private_create_new(path)?;
    file.sync_all()
        .map_err(|error| PresentError::io(path, error))
}

fn open_private_create_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| PresentError::io(path, error))
}

fn open_private_append(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    #[cfg(unix)]
    validate_private_file(path, &file)?;
    Ok(file)
}

fn repair_partial_tail(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    let mut file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    #[cfg(unix)]
    validate_private_file(path, &file)?;
    let length = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?
        .len();
    if length == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))
        .map_err(|error| PresentError::io(path, error))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|error| PresentError::io(path, error))?;
    if final_byte[0] == b'\n' {
        return Ok(());
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|error| PresentError::io(path, error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    let repaired_length = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    file.set_len(repaired_length as u64)
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

fn ensure_safe_dir(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
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
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => PresentError::SessionNotFound(path.display().to_string()),
        _ => PresentError::io(path, error),
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let file = File::open(path).map_err(|error| PresentError::io(path, error))?;
    #[cfg(unix)]
    validate_private_file(path, &file)?;
    serde_json::from_reader(file).map_err(PresentError::from)
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
    let mut file = open_private_create_new(&temporary)?;
    if let Err(error) = serde_json::to_writer_pretty(&mut file, value)
        .and_then(|()| file.write_all(b"\n").map_err(serde_json::Error::io))
    {
        let _ = fs::remove_file(&temporary);
        return Err(PresentError::Json(error));
    }
    file.sync_all()
        .map_err(|error| PresentError::io(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| PresentError::io(path, error))?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| PresentError::io(path, error))?;
    }
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
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    fs::remove_dir_all(path).map_err(|error| PresentError::io(path, error))
}

fn remove_file_if_regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    fs::remove_file(path).map_err(|error| PresentError::io(path, error))
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
            if metadata.file_type().is_symlink() {
                return Err(PresentError::UnsafePath(path));
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
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
        for private_name in ["service_port", "service_pid", "service_instance"] {
            assert!(!serialized.contains(private_name));
        }
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
    fn exact_feedback_retry_is_idempotent_but_conflicting_reuse_is_loud() {
        let (_temp, store) = store();
        let session = store.create(parsed()).unwrap();
        let event_id = Uuid::new_v4();
        let envelope = feedback(session.id, event_id);

        assert!(store.append_feedback(envelope.clone()).unwrap().created);
        assert!(!store.append_feedback(envelope.clone()).unwrap().created);
        assert_eq!(store.events(session.id).unwrap().len(), 1);

        let mut conflicting = envelope;
        conflicting.actor = "another operator".to_string();
        assert!(store.append_feedback(conflicting).is_err());
        assert_eq!(store.events(session.id).unwrap().len(), 1);
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
    fn automatic_retention_evicts_oldest_closed_state_and_preserves_active_state() {
        let (_temp, mut store) = store();
        store.retention.max_closed_sessions = 1;
        let first = store.create(parsed()).unwrap();
        let second = store.create(parsed()).unwrap();
        let active = store.create(parsed()).unwrap();
        store.close(first.id).unwrap();
        store.close(second.id).unwrap();

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
