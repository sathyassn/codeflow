//! The store: `<common git dir>/codeflow/coord/`.
//!
//! ```text
//! codeflow/coord/
//!   events.jsonl    the one log: every follow-up, lane, stream and
//!                   coordinator event, one sequence, append-only
//!   status/         liveness, one overwritten file per actor instance
//!   counters.json   reserved FU and LN numbers; gaps allowed, never lowered
//!   coord.lock      the stable sidecar lock; never removed or replaced
//!   instance.json   this ledger's UUID; FU and LN ids are local to it
//!   quarantine/     torn tails and damaged suffixes moved aside, kept
//! ```
//!
//! Writers take the lock with a bounded wait, repair a torn tail, fold and
//! validate the whole log, judge the new events by the same rules, append
//! them in one write and sync before they report success. Readers take no
//! lock and create nothing: they fold the committed prefix (every complete
//! line). The paths come from Git's common-dir discovery, so every worktree
//! of a clone shares one ledger, and the directory and files are private.

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::actor::{format_id, RecordedBy};
use super::schema::{Data, Envelope, SCHEMA_VERSION};
use super::transition::{fold, Folded, Ledger};
use super::CoordError;

/// How long a command waits for the lock.
pub const COMMAND_WAIT: Duration = Duration::from_secs(10);
/// How long a hook waits for the lock before it reports busy.
pub const HOOK_WAIT: Duration = Duration::from_secs(2);

/// The ledger's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The common git directory the ledger lives in.
    pub common_dir: PathBuf,
    pub dir: PathBuf,
    pub events: PathBuf,
    pub status: PathBuf,
    pub counters: PathBuf,
    pub lock: PathBuf,
    pub instance: PathBuf,
    pub quarantine: PathBuf,
}

impl Paths {
    /// The ledger's files under a common git directory.
    #[must_use]
    pub fn under(common_dir: &Path) -> Self {
        let dir = common_dir.join("codeflow").join("coord");
        Self {
            common_dir: common_dir.to_path_buf(),
            events: dir.join("events.jsonl"),
            status: dir.join("status"),
            counters: dir.join("counters.json"),
            lock: dir.join("coord.lock"),
            instance: dir.join("instance.json"),
            quarantine: dir.join("quarantine"),
            dir,
        }
    }

    /// Find the clone from `start` through Git's own discovery: the ledger
    /// is in the common directory, shared by every worktree.
    ///
    /// # Errors
    ///
    /// The reason no repository can be found.
    pub fn discover(start: &Path) -> Result<(Self, PathBuf), CoordError> {
        let repo = git2::Repository::discover(start)
            .map_err(|error| CoordError::Repository(error.message().to_string()))?;
        let root = repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| CoordError::Repository("the repository has no worktree".into()))?;
        Ok((Self::under(repo.commondir()), root))
    }
}

/// This ledger's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub instance_id: String,
    pub schema_version: u32,
    pub created_at: String,
}

/// The reserved numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    #[serde(default)]
    pub follow_up: u64,
    #[serde(default)]
    pub lane: u64,
}

/// One actor's liveness, overwritten on every bound mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Liveness {
    pub recorded_by: RecordedBy,
    pub last_seen: String,
    pub sequence: u64,
}

/// What a read found. A read never writes, takes no lock and creates
/// nothing.
#[derive(Debug)]
pub enum ReadOutcome {
    /// No ledger exists in this clone.
    Empty,
    /// The ledger exists but cannot be read.
    Unavailable(String),
    /// The folded ledger; `folded.damage` set means read-only.
    Ledger(Box<Snapshot>),
}

/// A read ledger.
#[derive(Debug)]
pub struct Snapshot {
    pub folded: Folded,
    pub instance: Option<Instance>,
}

/// Read and fold the committed prefix.
#[must_use]
pub fn read(paths: &Paths) -> ReadOutcome {
    for path in [&paths.dir, &paths.events] {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return ReadOutcome::Unavailable(format!(
                    "{} is a symbolic link; the ledger reads only its own files",
                    path.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ReadOutcome::Empty
            }
            Err(error) => return ReadOutcome::Unavailable(format!("{}: {error}", path.display())),
        }
    }
    let bytes = match fs::read(&paths.events) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ReadOutcome::Empty,
        Err(error) => {
            return ReadOutcome::Unavailable(format!("{}: {error}", paths.events.display()))
        }
    };
    let instance = fs::read_to_string(&paths.instance)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    ReadOutcome::Ledger(Box::new(Snapshot {
        folded: fold(&bytes),
        instance,
    }))
}

/// The id an event will take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectId {
    Given(String),
    /// Reserve the next follow-up number.
    NextFollowUp,
    /// Reserve the next lane number.
    NextLane,
}

/// An event to write, before the store gives it a sequence.
#[derive(Debug, Clone)]
pub struct Draft {
    pub subject: SubjectId,
    pub data: Data,
    pub comment: Option<String>,
    pub idempotency_key: Option<String>,
    pub expected_revision: Option<u64>,
}

impl Draft {
    /// A draft with no comment, key or expected revision.
    #[must_use]
    pub fn new(subject: SubjectId, data: Data) -> Self {
        Self {
            subject,
            data,
            comment: None,
            idempotency_key: None,
            expected_revision: None,
        }
    }
}

/// What a commit wrote.
#[derive(Debug)]
pub struct Committed {
    /// The events appended, in order; empty when nothing was needed.
    pub envelopes: Vec<Envelope>,
    /// The idempotency key was already recorded at this sequence; nothing
    /// was written.
    pub duplicate_of: Option<u64>,
    /// Why the liveness file could not be refreshed, when it could not.
    pub heartbeat_error: Option<String>,
    /// A torn tail that was moved to quarantine before the append.
    pub repaired_tail: Option<PathBuf>,
}

/// The held lock and the open log.
struct Locked {
    _lock: fs::File,
    events: fs::File,
    repaired_tail: Option<PathBuf>,
}

fn unwritten(path: &Path, error: &std::io::Error) -> CoordError {
    CoordError::Unwritten(format!("{}: {error}", path.display()))
}

/// Create a private directory, refusing a symbolic link in its place.
fn private_dir(path: &Path) -> Result<(), CoordError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(CoordError::Refused(format!(
                "{} is a symbolic link; the ledger writes only its own directory",
                path.display()
            )))
        }
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(CoordError::Refused(format!(
                "{} is not a directory",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(unwritten(path, &error)),
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => private_dir(path),
        Err(error) => Err(unwritten(path, &error)),
    }
}

/// Open one of the ledger's files for writing, private, never through a
/// symbolic link.
fn private_file(path: &Path, append: bool) -> Result<fs::File, CoordError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(CoordError::Refused(format!(
            "{} is a symbolic link; the ledger writes only its own files",
            path.display()
        )));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).create(true);
    if append {
        options.append(true);
    } else {
        options.write(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|error| unwritten(path, &error))
}

/// Write a whole file atomically and durably: a private temporary file in
/// the same directory, synced, renamed over the target, the directory synced.
fn durable_write(path: &Path, bytes: &[u8]) -> Result<(), CoordError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(CoordError::Refused(format!(
            "{} is a symbolic link; the ledger writes only its own files",
            path.display()
        )));
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(".coord-").suffix(".tmp");
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600));
    let mut temporary = builder
        .tempfile_in(dir)
        .map_err(|error| unwritten(dir, &error))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| unwritten(path, &error))?;
    temporary
        .persist(path)
        .map_err(|error| unwritten(path, &error.error))?;
    sync_dir(dir);
    Ok(())
}

/// Sync a directory so a new or renamed entry survives a crash. Windows
/// cannot open a directory for this; NTFS journals the rename.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn lock(paths: &Paths, wait: Duration) -> Result<Locked, CoordError> {
    private_dir(&paths.common_dir.join("codeflow"))?;
    private_dir(&paths.dir)?;
    let lock = private_file(&paths.lock, false)?;
    let deadline = Instant::now() + wait;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == fs2::lock_contended_error().kind() => {
                if Instant::now() >= deadline {
                    return Err(CoordError::Busy);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(unwritten(&paths.lock, &error)),
        }
    }
    let created = !paths.events.exists();
    let mut events = private_file(&paths.events, true)?;
    if created {
        sync_dir(&paths.dir);
    }
    ensure_instance(paths)?;
    let repaired_tail = repair_tail(paths, &mut events)?;
    Ok(Locked {
        _lock: lock,
        events,
        repaired_tail,
    })
}

fn ensure_instance(paths: &Paths) -> Result<Instance, CoordError> {
    match fs::read_to_string(&paths.instance) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            CoordError::ReadOnly(format!(
                "{} is malformed: {error}",
                paths.instance.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let instance = Instance {
                instance_id: uuid::Uuid::new_v4().to_string(),
                schema_version: SCHEMA_VERSION,
                created_at: super::format_time(super::now_secs()),
            };
            let text = serde_json::to_string_pretty(&instance).unwrap_or_default();
            durable_write(&paths.instance, text.as_bytes())?;
            Ok(instance)
        }
        Err(error) => Err(unwritten(&paths.instance, &error)),
    }
}

/// Move bytes after the last line break (a torn write) to quarantine and
/// cut the log back to its committed prefix. Runs under the lock, before
/// any append.
fn repair_tail(paths: &Paths, events: &mut fs::File) -> Result<Option<PathBuf>, CoordError> {
    let mut bytes = Vec::new();
    events
        .seek(std::io::SeekFrom::Start(0))
        .and_then(|_| events.read_to_end(&mut bytes))
        .map_err(|error| unwritten(&paths.events, &error))?;
    let committed = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if committed == bytes.len() {
        return Ok(None);
    }
    let kept = quarantine(paths, "torn", &bytes[committed..])?;
    events
        .set_len(committed as u64)
        .and_then(|()| events.sync_all())
        .map_err(|error| unwritten(&paths.events, &error))?;
    Ok(Some(kept))
}

fn quarantine(paths: &Paths, kind: &str, bytes: &[u8]) -> Result<PathBuf, CoordError> {
    private_dir(&paths.quarantine)?;
    let name = format!(
        "{kind}-{}-{}.bin",
        super::now_secs(),
        uuid::Uuid::new_v4().simple()
    );
    let path = paths.quarantine.join(name);
    durable_write(&path, bytes)?;
    Ok(path)
}

fn read_log(paths: &Paths, events: &mut fs::File) -> Result<Vec<u8>, CoordError> {
    let mut bytes = Vec::new();
    events
        .seek(std::io::SeekFrom::Start(0))
        .and_then(|_| events.read_to_end(&mut bytes))
        .map_err(|error| unwritten(&paths.events, &error))?;
    Ok(bytes)
}

fn fold_clean(paths: &Paths, events: &mut fs::File) -> Result<Ledger, CoordError> {
    let bytes = read_log(paths, events)?;
    let folded = fold(&bytes);
    if let Some(damage) = folded.damage {
        return Err(CoordError::ReadOnly(format!(
            "{damage}; nothing was written. Run `codeflow follow repair` as the operator to move the damaged suffix to quarantine"
        )));
    }
    Ok(folded.ledger)
}

/// Commit events as `actor`: take the lock (waiting at most `wait`),
/// repair a torn tail, fold and validate the log, ask `build` for the
/// events against the fresh state, judge each by the schema and the
/// transition rules, append them in one write and sync before returning.
///
/// When the first draft's idempotency key is already recorded, nothing is
/// written and [`Committed::duplicate_of`] names the earlier sequence.
///
/// # Errors
///
/// [`CoordError::Busy`] past the wait, [`CoordError::ReadOnly`] for a
/// damaged log, [`CoordError::Unwritten`] for a denied write, and
/// [`CoordError::Refused`] for an event that breaks a rule.
pub fn commit<F>(
    paths: &Paths,
    actor: &RecordedBy,
    wait: Duration,
    build: F,
) -> Result<Committed, CoordError>
where
    F: FnOnce(&Ledger) -> Result<Vec<Draft>, CoordError>,
{
    let mut locked = lock(paths, wait)?;
    let ledger = fold_clean(paths, &mut locked.events)?;
    let drafts = build(&ledger)?;
    if let Some(key) = drafts
        .first()
        .and_then(|draft| draft.idempotency_key.as_ref())
    {
        if let Some(sequence) = ledger.keys.get(key) {
            return Ok(Committed {
                envelopes: Vec::new(),
                duplicate_of: Some(*sequence),
                heartbeat_error: None,
                repaired_tail: locked.repaired_tail,
            });
        }
    }
    if drafts.is_empty() {
        return Ok(Committed {
            envelopes: Vec::new(),
            duplicate_of: None,
            heartbeat_error: None,
            repaired_tail: locked.repaired_tail,
        });
    }
    let mut state = ledger;
    let mut counters = read_counters(paths)?;
    let counters_before = counters.clone();
    let mut envelopes = Vec::new();
    let mut text = String::new();
    let now = super::format_time(super::now_secs());
    for draft in drafts {
        let subject_id = match draft.subject {
            SubjectId::Given(id) => id,
            SubjectId::NextFollowUp => {
                counters.follow_up = counters.follow_up.max(state.max_follow_up) + 1;
                format_id("FU", counters.follow_up)
            }
            SubjectId::NextLane => {
                counters.lane = counters.lane.max(state.max_lane) + 1;
                format_id("LN", counters.lane)
            }
        };
        let envelope = Envelope {
            schema_version: SCHEMA_VERSION,
            event: draft.data.kind(),
            subject_id,
            sequence: state.last_sequence + 1,
            recorded_at: now.clone(),
            recorded_by: actor.clone(),
            comment: draft.comment,
            idempotency_key: draft.idempotency_key,
            expected_revision: draft.expected_revision,
            data: draft.data.to_value(),
        };
        let data = super::schema::validate(&envelope).map_err(CoordError::Refused)?;
        state.apply(&envelope, &data).map_err(CoordError::Refused)?;
        let line = serde_json::to_string(&envelope)
            .map_err(|error| CoordError::Refused(error.to_string()))?;
        text.push_str(&line);
        text.push('\n');
        envelopes.push(envelope);
    }
    // Numbers are reserved before the events that use them are written: a
    // failed append leaves a gap, never a reused number.
    if counters != counters_before {
        let body = serde_json::to_string_pretty(&counters).unwrap_or_default();
        durable_write(&paths.counters, body.as_bytes())?;
    }
    locked
        .events
        .write_all(text.as_bytes())
        .and_then(|()| locked.events.sync_all())
        .map_err(|error| unwritten(&paths.events, &error))?;
    let last = envelopes.last().map_or(0, |envelope| envelope.sequence);
    let heartbeat_error = heartbeat(paths, actor, last)
        .err()
        .map(|error| error.to_string());
    Ok(Committed {
        envelopes,
        duplicate_of: None,
        heartbeat_error,
        repaired_tail: locked.repaired_tail,
    })
}

fn read_counters(paths: &Paths) -> Result<Counters, CoordError> {
    match fs::read_to_string(&paths.counters) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            CoordError::ReadOnly(format!(
                "{} is malformed: {error}",
                paths.counters.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Counters::default()),
        Err(error) => Err(unwritten(&paths.counters, &error)),
    }
}

/// The file name of an actor's liveness file: the instance id when it is
/// a plain name, otherwise its digest.
fn status_name(instance: &str) -> String {
    let plain = !instance.is_empty()
        && instance.len() <= 80
        && instance
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !instance.starts_with('.');
    if plain {
        format!("{instance}.json")
    } else {
        format!(
            "{}.json",
            &crate::scaffold::sha256_hex(instance.as_bytes())[..32]
        )
    }
}

/// Overwrite the actor's liveness file. Only a bound mutation calls this;
/// a read never does.
fn heartbeat(paths: &Paths, actor: &RecordedBy, sequence: u64) -> Result<(), CoordError> {
    private_dir(&paths.status)?;
    let liveness = Liveness {
        recorded_by: actor.clone(),
        last_seen: super::format_time(super::now_secs()),
        sequence,
    };
    let body = serde_json::to_string_pretty(&liveness).unwrap_or_default();
    durable_write(
        &paths.status.join(status_name(&actor.actor_instance)),
        body.as_bytes(),
    )
}

/// Read every liveness file; unreadable ones are skipped.
#[must_use]
pub fn liveness(paths: &Paths) -> Vec<Liveness> {
    let Ok(entries) = fs::read_dir(&paths.status) else {
        return Vec::new();
    };
    let mut out: Vec<Liveness> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    out.sort_by(|a, b| {
        a.recorded_by
            .actor_instance
            .cmp(&b.recorded_by.actor_instance)
    });
    out
}

/// What [`purge`] did.
#[derive(Debug)]
pub struct Purged {
    pub lines: usize,
}

/// Replace the free text of every event about `subject` with `[purged]`,
/// keeping ids, links, sequences and order. The rewritten log must fold as
/// cleanly as before. Runs under the lock.
///
/// # Errors
///
/// As [`commit`], and a refusal when the subject is unknown.
pub fn purge(paths: &Paths, subject: &str, wait: Duration) -> Result<Purged, CoordError> {
    let mut locked = lock(paths, wait)?;
    let bytes = read_log(paths, &mut locked.events)?;
    let before = fold(&bytes);
    if let Some(damage) = before.damage {
        return Err(CoordError::ReadOnly(damage));
    }
    if !before.ledger.items.contains_key(subject) {
        return Err(CoordError::Refused(format!(
            "{subject} is not in this ledger"
        )));
    }
    let mut out = String::with_capacity(bytes.len());
    let mut lines = 0;
    for raw in String::from_utf8_lossy(&bytes).lines() {
        let mut value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| CoordError::ReadOnly(error.to_string()))?;
        if value.get("subject_id").and_then(serde_json::Value::as_str) == Some(subject) {
            if let Some(object) = value.as_object_mut() {
                if object.contains_key("comment") {
                    object.insert("comment".into(), "[purged]".into());
                }
                if let Some(data) = object.get_mut("data") {
                    purge_text(data);
                }
            }
            lines += 1;
        }
        out.push_str(&serde_json::to_string(&value).unwrap_or_default());
        out.push('\n');
    }
    let after = fold(out.as_bytes());
    if after.damage.is_some() || after.ledger.last_sequence != before.ledger.last_sequence {
        return Err(CoordError::Refused(
            "the purged log would not fold cleanly; nothing was changed".into(),
        ));
    }
    durable_write(&paths.events, out.as_bytes())?;
    Ok(Purged { lines })
}

/// The free-text fields of follow-up data.
const PURGED_FIELDS: [&str; 10] = [
    "title",
    "description",
    "done_when",
    "source_reference",
    "no_link_reason",
    "awaiting_operator",
    "observed",
    "reason",
    "disposition",
    "summary",
];

fn purge_text(data: &mut serde_json::Value) {
    let Some(object) = data.as_object_mut() else {
        return;
    };
    for field in PURGED_FIELDS {
        if let Some(value) = object.get_mut(field) {
            if value.is_string() && value.as_str() != Some("") {
                *value = "[purged]".into();
            }
        }
    }
    if let Some(serde_json::Value::Array(rejected)) = object.get_mut("rejected_suggestions") {
        for entry in rejected {
            if let Some(reason) = entry.get_mut("reason") {
                *reason = "[purged]".into();
            }
        }
    }
}

/// What [`repair`] did.
#[derive(Debug)]
pub struct Repaired {
    /// Where the damaged suffix went, when there was one.
    pub quarantined: Option<PathBuf>,
    /// The events kept.
    pub kept: usize,
}

/// Move the damaged suffix of the log (from the first line that does not
/// fold) to quarantine and keep the valid prefix. The operator's repair
/// path; nothing is deleted.
///
/// # Errors
///
/// As [`commit`].
pub fn repair(paths: &Paths, wait: Duration) -> Result<Repaired, CoordError> {
    let mut locked = lock(paths, wait)?;
    let bytes = read_log(paths, &mut locked.events)?;
    let folded = fold(&bytes);
    if folded.damage.is_none() {
        return Ok(Repaired {
            quarantined: locked.repaired_tail,
            kept: folded.lines,
        });
    }
    let kept = quarantine(paths, "damaged", &bytes[folded.valid_bytes..])?;
    locked
        .events
        .set_len(folded.valid_bytes as u64)
        .and_then(|()| locked.events.sync_all())
        .map_err(|error| unwritten(&paths.events, &error))?;
    Ok(Repaired {
        quarantined: Some(kept),
        kept: folded.lines,
    })
}
