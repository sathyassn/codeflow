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

/// On a write, an existing directory or file of the ledger must belong to
/// this user; group and other access is removed. Unix only: Windows keeps
/// the ACL the directory inherits from the git directory.
// The result carries the unix ownership refusal; elsewhere it is always Ok.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn make_private(path: &Path, meta: &fs::Metadata, mode: u32) -> Result<(), CoordError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        if meta.uid() != me {
            return Err(CoordError::Refused(format!(
                "{} belongs to another user; the ledger writes only its own files",
                path.display()
            )));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
                .map_err(|error| unwritten(path, &error))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, meta, mode);
    Ok(())
}

/// Create a private directory, refusing a symbolic link in its place and
/// making an existing one private. A new directory's parent is synced so
/// the entry survives a crash.
fn private_dir(path: &Path) -> Result<(), CoordError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(CoordError::Refused(format!(
                "{} is a symbolic link; the ledger writes only its own directory",
                path.display()
            )))
        }
        Ok(meta) if meta.is_dir() => return make_private(path, &meta, 0o700),
        Ok(_) => {
            return Err(CoordError::Refused(format!(
                "{} is not a directory",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(unwritten(path, &error)),
    }
    #[cfg(unix)]
    let builder = {
        let mut builder = fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    match builder.create(path) {
        Ok(()) => sync_dir(path.parent().unwrap_or(Path::new("."))),
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
    // The log is appended on unix with O_APPEND. Windows grants an append
    // handle no right to cut the file, so a torn-tail or damage repair
    // (`set_len`) would be denied there: the log opens for writing and every
    // append seeks to the end first, under the lock.
    #[cfg(unix)]
    if append {
        options.append(true);
    } else {
        options.write(true).truncate(false);
    }
    #[cfg(not(unix))]
    {
        let _ = append;
        options.write(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| unwritten(path, &error))?;
    let meta = file.metadata().map_err(|error| unwritten(path, &error))?;
    make_private(path, &meta, 0o600)?;
    Ok(file)
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
    sync_dir(dir)
}

/// Sync a directory so a new or renamed entry survives a crash; a failure is
/// reported, never ignored. Windows cannot open a directory for this; NTFS
/// journals the rename, so there it is a no-op.
// The result carries the unix sync failure; elsewhere it is always Ok.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn sync_dir(dir: &Path) -> Result<(), CoordError> {
    #[cfg(unix)]
    {
        fs::File::open(dir)
            .and_then(|handle| handle.sync_all())
            .map_err(|error| unwritten(dir, &error))?;
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
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
        sync_dir(&paths.dir)?;
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
/// Idempotency is per event. When the first draft carries a key, every event
/// of the operation gets its own (`key#0`, `key#1`, ... for several), and
/// each is judged alone: an event whose key is recorded with the same
/// content is skipped, one recorded with other content or by another actor
/// is a [`CoordError::KeyConflict`], and the rest are written. A retry must
/// therefore build the same events in the same order; a retry after an
/// interrupted operation writes exactly the missing ones. A key recorded
/// before its item was purged is acknowledged without writing anything, so
/// a stale retry cannot restore purged text. When nothing is left to write,
/// [`Committed::duplicate_of`] names the first recorded sequence.
///
/// # Errors
///
/// [`CoordError::Busy`] past the wait, [`CoordError::ReadOnly`] for a
/// damaged log, [`CoordError::Unwritten`] for a denied write, and
/// [`CoordError::Refused`] for an event that breaks a rule.
#[allow(clippy::too_many_lines)]
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
    let mut drafts = keyed(build(&ledger)?);
    // Each keyed event of the operation is judged alone: one already
    // recorded with the same payload is skipped, so a retry after an
    // interrupted operation writes exactly the events that are missing; the
    // same key with another payload or actor is refused.
    let mut duplicate_of = None;
    let mut kept = Vec::with_capacity(drafts.len());
    for draft in drafts.drain(..) {
        let Some(key) = &draft.idempotency_key else {
            kept.push(draft);
            continue;
        };
        if let Some(sequence) = ledger.purged_keys.get(&super::transition::purged_key(key)) {
            duplicate_of.get_or_insert(*sequence);
            continue;
        }
        match ledger.keys.get(key) {
            None => kept.push(draft),
            Some((sequence, digest)) => {
                let subject = match &draft.subject {
                    SubjectId::Given(id) => Some(id.as_str()),
                    _ => None,
                };
                let wanted = super::transition::operation_digest(
                    draft.data.kind(),
                    subject,
                    &actor.actor_instance,
                    &draft.data.to_value(),
                );
                if *digest != wanted {
                    return Err(CoordError::KeyConflict(
                        "the idempotency key was already used for a different operation or actor; nothing was written".into(),
                    ));
                }
                duplicate_of.get_or_insert(*sequence);
            }
        }
    }
    let drafts = kept;
    if drafts.is_empty() && duplicate_of.is_some() {
        return Ok(Committed {
            envelopes: Vec::new(),
            duplicate_of,
            heartbeat_error: None,
            repaired_tail: locked.repaired_tail,
        });
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
            purged: false,
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
        .seek(std::io::SeekFrom::End(0))
        .and_then(|_| locked.events.write_all(text.as_bytes()))
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

/// Give every event of a keyed operation its own key: the operation's key
/// with the event's place (`key#0`, `key#1`, ...) when it has several
/// events, so each can be found again after an interruption.
fn keyed(mut drafts: Vec<Draft>) -> Vec<Draft> {
    let Some(key) = drafts
        .first()
        .and_then(|draft| draft.idempotency_key.clone())
    else {
        return drafts;
    };
    if drafts.len() > 1 {
        for (index, draft) in drafts.iter_mut().enumerate() {
            if index == 0 || draft.idempotency_key.is_none() {
                draft.idempotency_key = Some(format!("{key}#{index}"));
            }
        }
    }
    drafts
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

/// Overwrite the actor's liveness file through the bounded observer, within
/// the heartbeat limit. Only a bound mutation calls this; a read never does.
fn heartbeat(paths: &Paths, actor: &RecordedBy, sequence: u64) -> Result<(), CoordError> {
    heartbeat_within(paths, actor, sequence, super::observe::HEARTBEAT_TIMEOUT)
}

fn heartbeat_within(
    paths: &Paths,
    actor: &RecordedBy,
    sequence: u64,
    limit: Duration,
) -> Result<(), CoordError> {
    bounded_heartbeat(paths, actor, sequence, limit, false)
}

/// The heartbeat within `limit`: the ledger's directories (when
/// `prepare`) and the liveness file are all made inside the one bound.
fn bounded_heartbeat(
    paths: &Paths,
    actor: &RecordedBy,
    sequence: u64,
    limit: Duration,
    prepare: bool,
) -> Result<(), CoordError> {
    let (paths, actor) = (paths.clone(), actor.clone());
    let status = paths.status.clone();
    let work = move || {
        #[cfg(test)]
        if actor.actor_instance == STALLED_INSTANCE {
            std::thread::sleep(Duration::from_secs(30));
        }
        if prepare {
            private_dir(&paths.common_dir.join("codeflow"))?;
            private_dir(&paths.dir)?;
        }
        write_heartbeat(&paths, &actor, sequence)
    };
    super::observe::within(limit, work).unwrap_or_else(|| {
        Err(CoordError::Unwritten(format!(
            "{}: the liveness write did not finish within {} ms",
            status.display(),
            limit.as_millis()
        )))
    })
}

/// A test actor instance whose heartbeat stalls before its first
/// directory is made, to prove the whole refresh is bounded.
#[cfg(test)]
const STALLED_INSTANCE: &str = "test-stalled-heartbeat";

fn write_heartbeat(paths: &Paths, actor: &RecordedBy, sequence: u64) -> Result<(), CoordError> {
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

/// Refresh the liveness file of a bound actor whose operation recorded no
/// event (a check that found nothing to record). Reads never call this.
///
/// # Errors
///
/// The reason the file could not be written.
pub fn refresh_liveness(
    paths: &Paths,
    actor: &RecordedBy,
    sequence: u64,
) -> Result<(), CoordError> {
    refresh_liveness_within(paths, actor, sequence, super::observe::HEARTBEAT_TIMEOUT)
}

/// [`refresh_liveness`] within `limit`: a write that does not finish in
/// time is reported unwritten and the caller goes on.
///
/// # Errors
///
/// The reason the file could not be written, or that it did not finish.
pub fn refresh_liveness_within(
    paths: &Paths,
    actor: &RecordedBy,
    sequence: u64,
    limit: Duration,
) -> Result<(), CoordError> {
    bounded_heartbeat(paths, actor, sequence, limit, true)
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
    /// Events whose free text was replaced.
    pub lines: usize,
    /// Quarantined lines of the item that were removed; other items' lines
    /// stay.
    pub quarantine_removed: usize,
    /// Quarantine files that still hold the item's id in a line whose
    /// subject could not be established; reported, never claimed purged.
    pub retained: Vec<PathBuf>,
    /// The purge event's sequence; `None` when the item was not in the
    /// ledger and only its quarantined lines were purged, with no event.
    pub sequence: Option<u64>,
}

/// Fields that hold structure, not free text: ids, kinds, times, holders,
/// streams and link identities. Every other string of an item's events is
/// free text and is purged.
const STRUCTURAL: [&str; 21] = [
    "stream",
    "holder",
    "to_holder",
    "to_stream",
    "waits_on",
    "next_check",
    "observed_at",
    "kind",
    "type",
    "id",
    "uid",
    "repository",
    "number",
    "sha",
    "name",
    "remote",
    "at",
    "generation",
    "expected_generation",
    "outcome",
    "successor",
];

/// Replace every free-text string of `value` with `[purged]`, keeping the
/// structural fields. `source` is structure (an enum) only in `item_added`.
fn purge_value(value: &mut serde_json::Value, keep_source: bool) {
    match value {
        serde_json::Value::String(text) if !text.is_empty() => *text = "[purged]".into(),
        serde_json::Value::Array(items) => {
            for item in items {
                purge_value(item, keep_source);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, field) in object.iter_mut() {
                // A link holds only its kind and target identity.
                let structural = STRUCTURAL.contains(&key.as_str())
                    || key == "links"
                    || (key == "source" && keep_source);
                if !structural {
                    purge_value(field, keep_source);
                }
            }
        }
        _ => {}
    }
}

/// Remove an item's free text from the live log and from quarantine, as
/// `actor`, and record who purged it and when (no content) as an
/// `item_purged` event. Under the lock the actor is bound and must hold the
/// item, as for any mutation. Ids, links, sequences and order are kept; an
/// exact-duplicate identity becomes `[purged FU-NNN]`, so purged items stay
/// distinct, and an idempotency key is replaced by an opaque one.
///
/// # Errors
///
/// As [`commit`], and a refusal when the subject is unknown or the actor
/// does not hold it.
#[allow(clippy::too_many_lines)]
pub fn purge(
    paths: &Paths,
    actor: &RecordedBy,
    subject: &str,
    wait: Duration,
) -> Result<Purged, CoordError> {
    let mut locked = lock(paths, wait)?;
    let bytes = read_log(paths, &mut locked.events)?;
    let before = fold(&bytes);
    if let Some(damage) = before.damage {
        return Err(CoordError::ReadOnly(damage));
    }
    let ledger = &before.ledger;
    let bound = ledger.bind(actor).map_err(CoordError::Refused)?;
    let Some(item) = ledger.items.get(subject) else {
        // An item that survives only in quarantine (its events fell into a
        // damaged suffix): the operator may purge its quarantined lines.
        if bound != super::transition::Bound::Operator {
            return Err(CoordError::Refused(format!(
                "{subject} is not in this ledger; only the operator purges an item that survives only in quarantine"
            )));
        }
        let (quarantine_removed, retained) = purge_quarantine(paths, subject)?;
        if quarantine_removed == 0 && retained.is_empty() {
            return Err(CoordError::Refused(format!(
                "{subject} is neither in this ledger nor in quarantine"
            )));
        }
        return Ok(Purged {
            lines: 0,
            quarantine_removed,
            retained,
            sequence: None,
        });
    };
    if !super::transition::holds(&bound, item) {
        return Err(CoordError::Refused(
            "only the holder or the operator purges a follow-up".into(),
        ));
    }
    let mut out = String::with_capacity(bytes.len());
    let mut lines = 0_u64;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| CoordError::ReadOnly("the log is not UTF-8".into()))?;
    for raw in text.lines() {
        let mut value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| CoordError::ReadOnly(error.to_string()))?;
        if value.get("subject_id").and_then(serde_json::Value::as_str) == Some(subject) {
            let added =
                value.get("event").and_then(serde_json::Value::as_str) == Some("item_added");
            // The envelope's own flag, never a text prefix, says an earlier
            // purge already made this event's identity digests.
            let already = value.get("purged") == Some(&serde_json::Value::Bool(true));
            if let Some(object) = value.as_object_mut() {
                if object.contains_key("comment") {
                    object.insert("comment".into(), "[purged]".into());
                }
                if !already {
                    // Identity stays, as a digest: a retry of the purged
                    // operation is still recognised, and two purged items
                    // stay distinct, without any of their text.
                    if let Some(key) = object
                        .get("idempotency_key")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                    {
                        object.insert(
                            "idempotency_key".into(),
                            super::transition::purged_key(&key).into(),
                        );
                    }
                }
                if let Some(data) = object.get_mut("data") {
                    let reference = data
                        .get("source_reference")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    purge_value(data, added);
                    if let Some(reference) = reference {
                        data["source_reference"] = if already {
                            reference
                        } else {
                            super::transition::purged_reference(&reference)
                        }
                        .into();
                    }
                }
                object.insert("purged".into(), true.into());
            }
            lines += 1;
        }
        out.push_str(&serde_json::to_string(&value).unwrap_or_default());
        out.push('\n');
    }
    // The purge event, judged by the same rules as any other.
    let mut state = fold(out.as_bytes());
    if state.damage.is_some() || state.ledger.last_sequence != ledger.last_sequence {
        return Err(CoordError::Refused(
            "the purged log would not fold cleanly; nothing was changed".into(),
        ));
    }
    let envelope = Envelope {
        schema_version: SCHEMA_VERSION,
        event: super::schema::EventKind::ItemPurged,
        subject_id: subject.to_string(),
        sequence: ledger.last_sequence + 1,
        recorded_at: super::format_time(super::now_secs()),
        recorded_by: actor.clone(),
        comment: None,
        idempotency_key: None,
        expected_revision: None,
        purged: false,
        data: Data::ItemPurged(super::schema::Purge { events: lines }).to_value(),
    };
    let data = super::schema::validate(&envelope).map_err(CoordError::Refused)?;
    state
        .ledger
        .apply(&envelope, &data)
        .map_err(CoordError::Refused)?;
    out.push_str(&serde_json::to_string(&envelope).unwrap_or_default());
    out.push('\n');
    // Windows refuses to rename over a file that is open: close the log
    // before replacing it. The lock file stays held until the purge ends.
    let Locked {
        _lock: _held,
        events,
        repaired_tail: _,
    } = locked;
    drop(events);
    durable_write(&paths.events, out.as_bytes())?;
    let (quarantine_removed, retained) = purge_quarantine(paths, subject)?;
    let _ = heartbeat(paths, actor, envelope.sequence);
    Ok(Purged {
        lines: usize::try_from(lines).unwrap_or(usize::MAX),
        quarantine_removed,
        retained,
        sequence: Some(envelope.sequence),
    })
}

/// Whether one quarantined line belongs to `subject`. A line that parses
/// is judged by its `subject_id`; any other line by a `subject_id` field in
/// its bytes, in any JSON spacing, without decoding the rest (a damaged
/// byte elsewhere does not hide it). `None` when the line names the
/// subject's id but no `subject_id` field can be read: ambiguous.
fn names_subject(line: &[u8], subject: &str) -> Option<bool> {
    if let Some(value) = std::str::from_utf8(line)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
    {
        return Some(value.get("subject_id").and_then(serde_json::Value::as_str) == Some(subject));
    }
    // A line that does not parse may hold one event or, after a lost line
    // break, several. Every readable `subject_id` value is collected: the
    // line is the subject's only when each one names it, and it belongs to
    // another item when none does and the id is not mentioned.
    let field = b"\"subject_id\"";
    let skip = |bytes: &[u8]| -> usize {
        bytes
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            .count()
    };
    let mut values: Vec<&[u8]> = Vec::new();
    let mut unreadable = false;
    let mut start = 0;
    while let Some(found) = line[start..]
        .windows(field.len())
        .position(|window| window == field)
    {
        let rest = &line[start + found + field.len()..];
        let rest = &rest[skip(rest)..];
        let value = rest.strip_prefix(b":").and_then(|after| {
            let after = &after[skip(after)..];
            let value = after.strip_prefix(b"\"")?;
            let end = value.iter().position(|byte| *byte == b'"')?;
            Some(&value[..end])
        });
        match value {
            Some(value) => values.push(value),
            None => unreadable = true,
        }
        start += found + field.len();
    }
    let mentions = line
        .windows(subject.len())
        .any(|window| window == subject.as_bytes());
    let all_subject = !values.is_empty() && values.iter().all(|value| *value == subject.as_bytes());
    if all_subject && !unreadable {
        Some(true)
    } else if mentions {
        // Another subject beside this one, a field that cannot be read, or
        // the id named outside any field: ambiguous, kept and reported.
        None
    } else {
        Some(false)
    }
}

/// Remove the subject's lines from every quarantined fragment, keeping every
/// other item's lines byte for byte; a fragment left empty is removed.
/// Returns how many lines were removed and the fragments that still name
/// the subject in a line whose subject could not be read.
fn purge_quarantine(paths: &Paths, subject: &str) -> Result<(usize, Vec<PathBuf>), CoordError> {
    if crate::absence::proven_absent(&paths.quarantine)
        .map_err(|error| unwritten(&paths.quarantine, &error))?
    {
        return Ok((0, Vec::new()));
    }
    let entries =
        fs::read_dir(&paths.quarantine).map_err(|error| unwritten(&paths.quarantine, &error))?;
    let mut removed = 0;
    let mut retained = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| unwritten(&paths.quarantine, &error))?
            .path();
        let bytes = fs::read(&path).map_err(|error| unwritten(&path, &error))?;
        let mut kept: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut dropped = 0;
        let mut ambiguous = false;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            let body = line.strip_suffix(b"\n").unwrap_or(line);
            match names_subject(body, subject) {
                Some(true) => dropped += 1,
                Some(false) => kept.extend_from_slice(line),
                None => {
                    ambiguous = true;
                    kept.extend_from_slice(line);
                }
            }
        }
        if ambiguous {
            retained.push(path.clone());
        }
        if dropped == 0 {
            continue;
        }
        removed += dropped;
        if kept.iter().all(u8::is_ascii_whitespace) {
            fs::remove_file(&path).map_err(|error| unwritten(&path, &error))?;
        } else {
            durable_write(&path, &kept)?;
        }
    }
    if removed > 0 {
        sync_dir(&paths.quarantine)?;
    }
    Ok((removed, retained))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(instance: &str) -> RecordedBy {
        RecordedBy {
            role: "coordinator".into(),
            actor_instance: instance.into(),
            generation: Some(1),
            stream: None,
            harness: "unknown".into(),
            model: "unknown".into(),
            session: "unknown".into(),
        }
    }

    #[test]
    fn a_stalled_heartbeat_refresh_returns_within_its_bound() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let limit = Duration::from_millis(300);
        let start = std::time::Instant::now();
        let out = refresh_liveness_within(&paths, &actor(STALLED_INSTANCE), 1, limit);
        let elapsed = start.elapsed();
        assert!(
            matches!(&out, Err(CoordError::Unwritten(why)) if why.contains("did not finish")),
            "{out:?}"
        );
        assert!(elapsed < limit + Duration::from_secs(2), "{elapsed:?}");
    }

    #[test]
    fn a_prompt_heartbeat_refresh_writes_the_liveness_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        refresh_liveness(&paths, &actor("lane-one"), 4).unwrap();
        let live = liveness(&paths);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].sequence, 4);
    }
}
