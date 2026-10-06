//! The issue protocol and the writes that follow it (SPC-013 R-10 to R-19,
//! R-108): reserve, sync, admit, restore, resume and the pending-state
//! repair that retarget needs.

use std::path::Path;

use super::entry::{Entry, Kind, RegId};
use super::git::{z_fields, Git};
use super::inventory::{self, branch_name, is_landing_branch};
use super::ledger::{short, Ledger};
use super::state::{self, Unwritten};
use super::{tracking_ref, IdsError, Standing, AUTHORITY, PUSH_ATTEMPTS, REGISTRY_REF};

/// What to reserve.
#[derive(Debug, Clone)]
pub struct Request {
    pub kind: Kind,
    pub title: String,
    /// The intended integration target, informational after issue.
    pub target: String,
    /// Keep this `uid` (retarget, admit); a fresh one otherwise.
    pub uid: Option<String>,
    /// Reserve exactly this id when it is free (admit).
    pub wanted: Option<RegId>,
    /// Issuer override (admit writes `admit:<maintainer>`).
    pub issuer: Option<String>,
    /// The commit subject verb: `issue` or `admit`.
    pub verb: &'static str,
    /// The creation request kept for `--resume` until the record is written.
    pub resume: Option<serde_json::Value>,
}

impl Request {
    /// A plain issue of the next `kind` id.
    #[must_use]
    pub fn issue(kind: Kind, title: &str, target: &str) -> Request {
        Request {
            kind,
            title: title.to_string(),
            target: target.to_string(),
            uid: None,
            wanted: None,
            issuer: None,
            verb: "issue",
            resume: None,
        }
    }
}

/// A reservation made.
#[derive(Debug, Clone)]
pub struct Reservation {
    pub id: RegId,
    pub uid: String,
    pub standing: Standing,
    /// Why a reservation is pending, when it is.
    pub note: Option<String>,
}

pub(super) enum Fetched {
    Ok,
    Absent,
    Offline(String),
}

pub(super) enum Pushed {
    Accepted,
    Moved(String),
    Failed(IdsError),
    Unclear(String),
}

/// Reserve an id by the issue protocol (R-12, R-13).
///
/// # Errors
///
/// Returns the classified failure: damage, an unseeded registry, a rewrite,
/// a permission, unsupported or transport error, or contention past the
/// retry bound.
pub fn reserve(root: &Path, request: &Request) -> Result<Reservation, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    reserve_locked(&git, request)
}

fn reserve_locked(git: &Git, request: &Request) -> Result<Reservation, IdsError> {
    let uid = request.uid.clone().unwrap_or_else(super::new_uid);
    let issuer = match request.issuer.clone() {
        Some(issuer) => issuer,
        None => git.user_email()?,
    };
    if !git.has_remote(AUTHORITY)? {
        return commit_local(git, request, &uid, &issuer, Standing::Local, None);
    }
    if let Fetched::Offline(reason) = fetch(git, AUTHORITY)? {
        return commit_local(git, request, &uid, &issuer, Standing::Pending, Some(reason));
    }
    verify_descent(git, AUTHORITY)?;
    publish_pending(git, AUTHORITY)?;
    let tracking = tracking_ref(AUTHORITY);
    let mut reason = String::new();
    for attempt in 0..PUSH_ATTEMPTS {
        if attempt > 0 {
            backoff(attempt);
            if let Fetched::Offline(reason) = fetch(git, AUTHORITY)? {
                return commit_local(git, request, &uid, &issuer, Standing::Pending, Some(reason));
            }
            verify_descent(git, AUTHORITY)?;
        }
        let base = git.rev(&tracking)?;
        let ledger = Ledger::read(git, &tracking)?;
        healthy(&ledger)?;
        if base.is_none() {
            fresh_repository(git)?;
        }
        let id = choose(git, &[&ledger], request)?;
        let entry = Entry::issued(
            id.clone(),
            uid.clone(),
            &request.title,
            &issuer,
            &request.target,
        );
        let commit = commit_entries(git, base.as_deref(), &[&entry], request.verb)?;
        remember(git, &entry, request)?;
        match push(git, AUTHORITY, &commit)? {
            Pushed::Accepted => {
                adopt(git, AUTHORITY, &commit)?;
                return Ok(Reservation {
                    id,
                    uid,
                    standing: Standing::Reserved,
                    note: None,
                });
            }
            Pushed::Moved(why) => {
                forget(git, &id)?;
                reason = why;
            }
            Pushed::Failed(error) => {
                forget(git, &id)?;
                return Err(error);
            }
            Pushed::Unclear(why) => return read_back(git, &entry, &commit, &why),
        }
    }
    Err(IdsError::Contended(PUSH_ATTEMPTS, reason))
}

/// A jittered pause before retrying a lost race, so issuers that keep
/// colliding spread out instead of starving one another.
fn backoff(attempt: usize) {
    let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]) % 100;
    let base = 40 * u64::try_from(attempt).unwrap_or(1);
    std::thread::sleep(std::time::Duration::from_millis(base + jitter));
}

/// Resolve an unclear push by fetching and reading back (R-13): reserved
/// only when the fetched tip binds the number to our `uid`.
fn read_back(git: &Git, entry: &Entry, commit: &str, why: &str) -> Result<Reservation, IdsError> {
    let tracking = tracking_ref(AUTHORITY);
    match fetch(git, AUTHORITY)? {
        Fetched::Offline(reason) => {
            // Nothing can be known: keep the commit as a pending reservation
            // so `ids sync` resolves it by uid later.
            let old = git.rev(REGISTRY_REF)?;
            set_ref(git, REGISTRY_REF, commit, old.as_deref())?;
            Ok(Reservation {
                id: entry.id.clone(),
                uid: entry.uid.clone(),
                standing: Standing::Pending,
                note: Some(format!(
                    "push result unclear ({why}); read-back failed ({reason})"
                )),
            })
        }
        Fetched::Ok | Fetched::Absent => {
            let ledger = Ledger::read(git, &tracking)?;
            if ledger
                .entry(&entry.id)
                .is_some_and(|bound| bound.uid == entry.uid)
            {
                let tip = ledger.tip.clone().unwrap_or_default();
                adopt(git, AUTHORITY, &tip)?;
                Ok(Reservation {
                    id: entry.id.clone(),
                    uid: entry.uid.clone(),
                    standing: Standing::Reserved,
                    note: Some(format!(
                        "push result unclear ({why}); read back as reserved"
                    )),
                })
            } else {
                forget(git, &entry.id)?;
                Err(IdsError::Transport(format!(
                    "{why}; read back: {} is not bound to this reservation, so it was not taken",
                    entry.id
                )))
            }
        }
    }
}

/// Commit a reservation on the local registry (offline, or no authority).
fn commit_local(
    git: &Git,
    request: &Request,
    uid: &str,
    issuer: &str,
    standing: Standing,
    note: Option<String>,
) -> Result<Reservation, IdsError> {
    let tracking = tracking_ref(AUTHORITY);
    if git.rev(REGISTRY_REF)?.is_none() && git.rev(&tracking)?.is_none() {
        match fresh_repository(git) {
            Ok(()) => {}
            // With no remote this clone is the authority: seeding it is
            // local and safe, so the first `new` seeds instead of refusing.
            Err(IdsError::NotSeeded) if standing == Standing::Local => {
                super::seed::seed_locked(git, None)?;
            }
            Err(error) => return Err(error),
        }
    }
    let local = Ledger::read(git, REGISTRY_REF)?;
    let remote = Ledger::read(git, &tracking)?;
    healthy(&local)?;
    healthy(&remote)?;
    let base = match git.rev(REGISTRY_REF)? {
        Some(local) => Some(local),
        None => git.rev(&tracking)?,
    };
    let id = choose(git, &[&local, &remote], request)?;
    let entry = Entry::issued(
        id.clone(),
        uid.to_string(),
        &request.title,
        issuer,
        &request.target,
    );
    let commit = commit_entries(git, base.as_deref(), &[&entry], request.verb)?;
    remember(git, &entry, request)?;
    set_ref(
        git,
        REGISTRY_REF,
        &commit,
        git.rev(REGISTRY_REF)?.as_deref(),
    )?;
    Ok(Reservation {
        id,
        uid: uid.to_string(),
        standing,
        note,
    })
}

fn choose(git: &Git, ledgers: &[&Ledger], request: &Request) -> Result<RegId, IdsError> {
    if let Some(wanted) = &request.wanted {
        if !ledgers.iter().any(|ledger| ledger.holds(wanted)) {
            return Ok(wanted.clone());
        }
    }
    let kind = request.kind;
    let history = ledgers
        .iter()
        .map(|ledger| ledger.max_seq(kind))
        .max()
        .unwrap_or(0);
    let refs = inventory::max_seq_on_refs(git, kind)?;
    let worktree = inventory::max_seq_in_worktree(git.root(), kind).map_err(|error| {
        IdsError::Invalid(format!("cannot inventory work records before issuing an ID: {error}; restore readable record directories, then retry"))
    })?;
    let next = history.max(refs).max(worktree) + 1;
    Ok(RegId::canonical(kind, next))
}

fn healthy(ledger: &Ledger) -> Result<(), IdsError> {
    if ledger.is_damaged() {
        return Err(IdsError::Damaged(ledger.damage.clone()));
    }
    Ok(())
}

/// With no registry anywhere, only a repository with no records may create
/// it by its first issue; one with records is seeded first (R-24).
fn fresh_repository(git: &Git) -> Result<(), IdsError> {
    for (_, sha) in inventory::code_refs(git)? {
        let mut args = vec![
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "--full-tree",
            sha.as_str(),
            "--",
        ];
        args.extend_from_slice(&super::entry::RECORD_ROOTS);
        if z_fields(&git.run_bytes(&args)?)
            .iter()
            .any(|path| super::record_id_from_path(path).is_some())
        {
            return Err(IdsError::NotSeeded);
        }
    }
    Ok(())
}

fn commit_entries(
    git: &Git,
    base: Option<&str>,
    entries: &[&Entry],
    verb: &str,
) -> Result<String, IdsError> {
    let mut files = Vec::new();
    for entry in entries {
        let blob = git.write_blob(entry.render().as_bytes())?;
        files.push((entry.id.registry_path(), blob));
    }
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.to_string()).collect();
    git.commit_files(base, &files, &format!("{verb}: {}", ids.join(" ")))
}

fn remember(git: &Git, entry: &Entry, request: &Request) -> Result<(), IdsError> {
    let Some(resume) = &request.resume else {
        return Ok(());
    };
    let unwritten = Unwritten {
        id: entry.id.to_string(),
        uid: entry.uid.clone(),
        issuer: entry.issuer.clone(),
        request: resume.clone(),
    };
    state::update(git, |state| {
        state.unwritten.retain(|item| item.id != unwritten.id);
        state.unwritten.push(unwritten);
    })
}

fn forget(git: &Git, id: &RegId) -> Result<(), IdsError> {
    let id = id.to_string();
    state::update(git, |state| state.unwritten.retain(|item| item.id != id))
}

/// Mark a reservation's record as written.
///
/// # Errors
///
/// Returns an error when the state cannot be saved.
pub fn written(root: &Path, id: &RegId) -> Result<(), IdsError> {
    forget(&Git::new(root), id)
}

fn set_ref(git: &Git, name: &str, new: &str, old: Option<&str>) -> Result<(), IdsError> {
    let mut args = vec!["update-ref", "-m", "codeflow ids", name, new];
    if let Some(old) = old {
        args.push(old);
    }
    git.run(&args).map(|_| ())
}

/// After an accepted push or a verified read-back: the last verified tip,
/// the tracking ref and the local registry all move to `tip`, which must
/// descend from the saved checkpoint.
pub(super) fn adopt(git: &Git, remote: &str, tip: &str) -> Result<(), IdsError> {
    checkpoint(git, remote, tip)?;
    git.run(&[
        "update-ref",
        "-m",
        "codeflow ids",
        &tracking_ref(remote),
        tip,
    ])?;
    git.run(&["update-ref", "-m", "codeflow ids", REGISTRY_REF, tip])
        .map(|_| ())
}

pub(super) fn fetch(git: &Git, remote: &str) -> Result<Fetched, IdsError> {
    let tracking = tracking_ref(remote);
    let refspec = format!("refs/heads/{}:{tracking}", super::REGISTRY_BRANCH);
    let output = git.output(&["fetch", "--no-tags", remote, &refspec])?;
    if output.status.success() {
        return Ok(Fetched::Ok);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return Err(IdsError::Git(format!(
            "fetching the registry from {remote} failed without a message"
        )));
    }
    let lower = stderr.to_lowercase();
    if lower.contains("couldn't find remote ref") || lower.contains("could not find remote ref") {
        let name = format!("refs/heads/{}", super::REGISTRY_BRANCH);
        match crate::git::remote_query::ls_remote(git.root(), &["--exit-code", remote, &name]) {
            Ok(answer) if answer.is_empty() => {}
            Ok(_) => return Ok(Fetched::Offline(stderr)),
            Err(error) => {
                return Ok(Fetched::Offline(format!(
                    "{stderr}; cannot prove remote absence: {error}"
                )))
            }
        }
        if git.rev(&tracking)?.is_some() {
            return Err(IdsError::Rewritten(format!(
                "`{}` is gone from {remote} although it was fetched before",
                super::REGISTRY_BRANCH
            )));
        }
        return Ok(Fetched::Absent);
    }
    if lower.contains("non-fast-forward") || lower.contains("[rejected]") {
        return Err(IdsError::Rewritten(format!(
            "a non-forced fetch refused the update: {stderr}"
        )));
    }
    Ok(Fetched::Offline(stderr))
}

/// R-10: refuse when the fetched tip does not descend from the last tip
/// this clone verified.
/// A registry absent from the authority is new only while this clone has
/// never verified one; after a checkpoint it is a deletion, refused before
/// any push so nothing can replace the lost history.
pub(super) fn verify_descent(git: &Git, remote: &str) -> Result<(), IdsError> {
    let Some(tip) = git.rev(&tracking_ref(remote))? else {
        if let Some(last) = state::load(git)?.last_verified.get(remote) {
            return Err(IdsError::Rewritten(format!(
                "`{branch}` is absent from {remote} although this clone verified {} there; nothing was pushed. A maintainer republishes the verified history (`git push {remote} {last}:refs/heads/{branch}`), then repairs any file it lacks with `codeflow ids restore <id>`",
                short(last),
                branch = super::REGISTRY_BRANCH,
            )));
        }
        return Ok(());
    };
    checkpoint(git, remote, &tip)
}

/// The one writer of the saved checkpoint (R-10). `tip` becomes the last
/// verified tip only when it descends from the one saved before, so no
/// command path (issue, sync, read-back, seed, restore) can move the
/// checkpoint across a rewrite and make a lost number issuable again.
fn checkpoint(git: &Git, remote: &str, tip: &str) -> Result<(), IdsError> {
    let state = state::load(git)?;
    if let Some(last) = state.last_verified.get(remote) {
        if git.rev(last)?.is_none() || !git.is_ancestor(last, tip)? {
            return Err(IdsError::Rewritten(format!(
                "the registry tip {} does not descend from the last verified tip {}",
                short(tip),
                short(last)
            )));
        }
    }
    state::update(git, |state| {
        state
            .last_verified
            .insert(remote.to_string(), tip.to_string());
    })
}

pub(super) fn push(git: &Git, remote: &str, commit: &str) -> Result<Pushed, IdsError> {
    let refspec = format!("{commit}:{REGISTRY_REF}");
    let output = git.output(&["push", "--porcelain", remote, &refspec])?;
    if output.status.success() {
        return Ok(Pushed::Accepted);
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(classify_push(&text))
}

/// Classify a failed push (R-13). A refusal that names permission, an
/// unsupported operation or a transport failure is reported as such even
/// when it also mentions a ref lock, so it is never retried as a race. Only
/// the diagnostics of a lost compare-and-swap count as a moved tip; a hook
/// refusal is reported; anything else is unclear and read back.
fn classify_push(text: &str) -> Pushed {
    let lower = diagnostics(text);
    let reason = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "Done")
        .collect::<Vec<_>>()
        .join("; ");
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    if has(&["codeflow pre-push"]) {
        return Pushed::Failed(IdsError::Refused(reason));
    }
    if has(&[
        "permission",
        "denied",
        "gh013",
        "protected branch",
        "not allowed",
        "authentication failed",
        "read-only",
    ]) || has_word(&lower, "403")
    {
        return Pushed::Failed(IdsError::Permission(reason));
    }
    if has(&["not supported", "unsupported", "does not support"]) {
        return Pushed::Failed(IdsError::Unsupported(reason));
    }
    if has(&[
        "could not resolve host",
        "connection refused",
        "does not appear to be a git repository",
        "no such file or directory",
    ]) {
        return Pushed::Failed(IdsError::Transport(reason));
    }
    if is_lost_race(&lower) {
        return Pushed::Moved(reason);
    }
    if has(&["hook declined", "pre-receive hook"]) {
        return Pushed::Failed(IdsError::Refused(reason));
    }
    Pushed::Unclear(reason)
}

/// The lower-cased diagnostic text of a push, without what names the
/// objects involved: the `To <url>` line, the closing `failed to push some
/// refs to '<url>'` line and the `<src>:<dst>` field of a porcelain status
/// line. A sha, a path or a URL can contain `403` or
/// `denied` by chance and must never decide the outcome.
fn diagnostics(text: &str) -> String {
    text.lines()
        .filter(|line| {
            !line.starts_with("To ") && !line.starts_with("error: failed to push some refs to ")
        })
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                [flag, _refs, summary] if flag.trim_matches(' ').len() <= 1 => {
                    (*summary).to_string()
                }
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
}

/// Whether `word` occurs in `text` with no letter or digit on either side.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The diagnostics of a lost compare-and-swap: the client saw a newer tip
/// (`fetch first`, `non-fast-forward`, `stale info`), or the host's ref
/// transaction found another old value (`is at X but expected Y`,
/// `incorrect old value`, a creation that met an existing ref).
fn is_lost_race(lower: &str) -> bool {
    [
        "(fetch first)",
        "(non-fast-forward)",
        "(stale info)",
        "incorrect old value",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || (lower.contains("cannot lock ref")
            && (lower.contains("but expected") || lower.contains("reference already exists")))
}

/// What a sync did.
#[derive(Debug, Default)]
pub struct SyncReport {
    /// Pending ids published now.
    pub published: Vec<RegId>,
    /// Pending ids the authority already binds to the same `uid`.
    pub already: Vec<RegId>,
    /// Why nothing was published, when that is the outcome.
    pub note: Option<String>,
}

/// `ids sync` (R-15, R-19): publish pending reservations on the authority.
///
/// # Errors
///
/// Returns a clash naming `ids retarget` (or the authority migration for a
/// landed record), or the classified push failure. Offline is
/// [`IdsError::Offline`].
pub fn sync(root: &Path) -> Result<SyncReport, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    if !git.has_remote(AUTHORITY)? {
        return Ok(SyncReport {
            note: Some("no authority remote: the local registry is authoritative".to_string()),
            ..SyncReport::default()
        });
    }
    if let Fetched::Offline(reason) = fetch(&git, AUTHORITY)? {
        return Err(IdsError::Offline(reason));
    }
    verify_descent(&git, AUTHORITY)?;
    publish_pending(&git, AUTHORITY)
}

/// Whether the local registry holds reservations the authority lacks.
///
/// # Errors
/// Returns an error when Git cannot inspect the local or authority history.
pub fn has_pending(root: &Path) -> Result<bool, IdsError> {
    let git = Git::new(root);
    let Some(local) = git.rev(REGISTRY_REF)? else {
        return Ok(false);
    };
    match git.rev(&tracking_ref(AUTHORITY))? {
        Some(tracking) => Ok(!git.is_ancestor(&local, &tracking)?),
        None => git.has_remote(AUTHORITY),
    }
}

/// The pending files of the local registry: every addition in commits the
/// tracking ref does not reach.
fn pending_entries(git: &Git, local: &str, tracking: Option<&str>) -> Result<Vec<Entry>, IdsError> {
    let ledger = Ledger::read(git, local)?;
    let violations = ledger.range_violations(git, tracking)?;
    if let Some(finding) = violations.first() {
        return Err(IdsError::Invalid(format!(
            "the local registry breaks the append-only rule at {}: {}",
            short(&finding.commit),
            finding.message
        )));
    }
    let mut args = vec!["rev-list", "--reverse", local];
    let exclude = tracking.map(|tip| format!("^{tip}"));
    if let Some(exclude) = &exclude {
        args.push(exclude);
    }
    let commits = git.run(&args)?;
    let mut entries = Vec::new();
    for commit in commits.split_terminator('\n') {
        let changes = git.run_bytes(&[
            "diff-tree",
            "-r",
            "--root",
            "--no-renames",
            "--no-commit-id",
            "-z",
            commit,
        ])?;
        for change in inventory::raw_changes(&changes)? {
            if change.status != 'A' {
                continue;
            }
            if let Some(id) = RegId::from_registry_path(&change.path) {
                if let Some(entry) = ledger.entry(&id) {
                    entries.push(entry.clone());
                }
            }
        }
    }
    Ok(entries)
}

fn publish_pending(git: &Git, remote: &str) -> Result<SyncReport, IdsError> {
    let tracking = tracking_ref(remote);
    let mut report = SyncReport::default();
    let Some(local) = git.rev(REGISTRY_REF)? else {
        return Ok(report);
    };
    let mut reason = String::new();
    for attempt in 0..PUSH_ATTEMPTS {
        if attempt > 0 {
            backoff(attempt);
            if let Fetched::Offline(reason) = fetch(git, remote)? {
                return Err(IdsError::Offline(reason));
            }
            verify_descent(git, remote)?;
        }
        let tip = git.rev(&tracking)?;
        if let Some(tip) = &tip {
            if git.is_ancestor(&local, tip)? {
                set_ref(git, REGISTRY_REF, tip, Some(&local))?;
                return Ok(report);
            }
        }
        let pending = pending_entries(git, &local, tip.as_deref())?;
        let Some(tip) = tip else {
            // The authority has no registry: publish the local history as
            // it is; its root commit is judged by the same rule (R-109).
            match push(git, remote, &local)? {
                Pushed::Accepted => {
                    adopt(git, remote, &local)?;
                    report.published = pending.into_iter().map(|entry| entry.id).collect();
                    return Ok(report);
                }
                Pushed::Moved(why) => {
                    reason = why;
                    continue;
                }
                Pushed::Failed(error) => return Err(error),
                Pushed::Unclear(why) => return Err(IdsError::Transport(why)),
            }
        };
        let authority = Ledger::read(git, &tracking)?;
        healthy(&authority)?;
        let mut remaining = Vec::new();
        let mut clashes = Vec::new();
        report.already.clear();
        for entry in pending {
            match authority.entry(&entry.id) {
                Some(bound) if bound.uid == entry.uid => report.already.push(entry.id.clone()),
                Some(_) => clashes.push(entry),
                None if authority.holds(&entry.id) => clashes.push(entry),
                None => remaining.push(entry),
            }
        }
        if !clashes.is_empty() {
            return Err(IdsError::Clash(clash_message(git, &clashes)?));
        }
        if remaining.is_empty() {
            checkpoint(git, remote, &tip)?;
            set_ref(git, REGISTRY_REF, &tip, Some(&local))?;
            return Ok(report);
        }
        let refs: Vec<&Entry> = remaining.iter().collect();
        let commit = commit_entries(git, Some(&tip), &refs, "sync")?;
        match push(git, remote, &commit)? {
            Pushed::Accepted => {
                adopt(git, remote, &commit)?;
                report.published = remaining.into_iter().map(|entry| entry.id).collect();
                return Ok(report);
            }
            Pushed::Moved(why) => reason = why,
            Pushed::Failed(error) => return Err(error),
            Pushed::Unclear(why) => {
                if let Fetched::Offline(reason) = fetch(git, remote)? {
                    return Err(IdsError::Transport(format!(
                        "{why}; read-back failed: {reason}"
                    )));
                }
                let authority = Ledger::read(git, &tracking)?;
                if remaining.iter().all(|entry| {
                    authority
                        .entry(&entry.id)
                        .is_some_and(|bound| bound.uid == entry.uid)
                }) {
                    let tip = authority.tip.clone().unwrap_or_default();
                    adopt(git, remote, &tip)?;
                    report.published = remaining.into_iter().map(|entry| entry.id).collect();
                    return Ok(report);
                }
                return Err(IdsError::Transport(why));
            }
        }
    }
    Err(IdsError::Contended(PUSH_ATTEMPTS, reason))
}

fn clash_message(git: &Git, clashes: &[Entry]) -> Result<String, IdsError> {
    let mut lines =
        vec!["sync stopped: the authority binds these numbers to another record:".to_string()];
    let landing: Vec<(String, String)> = git
        .branch_refs()?
        .into_iter()
        .filter(|(name, _)| name.starts_with("refs/heads/") && is_landing_branch(branch_name(name)))
        .collect();
    for entry in clashes {
        let mut landed_on = None;
        for (name, sha) in &landing {
            if inventory::copies_at(git, sha)?
                .iter()
                .any(|copy| copy.id == entry.id && copy.uid.as_deref() == Some(entry.uid.as_str()))
            {
                landed_on = Some(branch_name(name).to_string());
                break;
            }
        }
        lines.push(match landed_on {
            Some(branch) => format!(
                "  {}: already landed on {branch}; an explicit authority migration must record its disposition (R-19). It is never renamed silently.",
                entry.id
            ),
            None => format!(
                "  {}: run `codeflow ids retarget {}` to renumber this unmerged record",
                entry.id, entry.id
            ),
        });
    }
    Ok(lines.join("\n"))
}

/// Remove the pending local reservation of `id` (retarget replaces it).
///
/// # Errors
///
/// Returns an error when git fails.
pub fn drop_pending(root: &Path, id: &RegId) -> Result<bool, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    let tracking = git.rev(&tracking_ref(AUTHORITY))?;
    let Some(local) = git.rev(REGISTRY_REF)? else {
        return Ok(false);
    };
    if match tracking.as_deref() {
        Some(tip) => git.is_ancestor(&local, tip)?,
        None => false,
    } {
        return Ok(false);
    }
    let pending = pending_entries(&git, &local, tracking.as_deref())?;
    if !pending.iter().any(|entry| &entry.id == id) {
        return Ok(false);
    }
    let keep: Vec<&Entry> = pending.iter().filter(|entry| &entry.id != id).collect();
    match (&tracking, keep.is_empty()) {
        (Some(tip), true) => set_ref(&git, REGISTRY_REF, tip, Some(&local))?,
        (base, false) => {
            let commit = commit_entries(&git, base.as_deref(), &keep, "issue")?;
            set_ref(&git, REGISTRY_REF, &commit, Some(&local))?;
        }
        (None, true) => {
            git.run(&["update-ref", "-d", REGISTRY_REF, &local])?;
        }
    }
    Ok(true)
}

/// What an admission did.
#[derive(Debug, Clone)]
pub struct Admission {
    pub requested: RegId,
    pub reserved: RegId,
    pub standing: Standing,
    /// The registry already bound this `uid`; nothing was written.
    pub already: bool,
}

/// `ids admit` (R-17): reserve the number for a fork or hand-written
/// record's `uid`, or the next free number when it is held.
///
/// # Errors
///
/// Returns an error when the authority is unreachable or the push fails.
pub fn admit(
    root: &Path,
    id: &RegId,
    uid: &str,
    title: &str,
    target: &str,
) -> Result<Admission, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    let registry = if git.has_remote(AUTHORITY)? {
        if let Fetched::Offline(reason) = fetch(&git, AUTHORITY)? {
            return Err(IdsError::Offline(reason));
        }
        verify_descent(&git, AUTHORITY)?;
        publish_pending(&git, AUTHORITY)?;
        tracking_ref(AUTHORITY)
    } else {
        REGISTRY_REF.to_string()
    };
    let ledger = Ledger::read(&git, &registry)?;
    if let Some(bound) = ledger.ids_for_uid(uid).into_iter().next() {
        return Ok(Admission {
            requested: id.clone(),
            reserved: bound,
            standing: Standing::Reserved,
            already: true,
        });
    }
    let request = Request {
        kind: id.kind(),
        title: title.to_string(),
        target: target.to_string(),
        uid: Some(uid.to_string()),
        wanted: Some(id.clone()),
        issuer: Some(format!("admit:{}", git.user_email()?)),
        verb: "admit",
        resume: None,
    };
    let reservation = reserve_locked(&git, &request)?;
    Ok(Admission {
        requested: id.clone(),
        reserved: reservation.id,
        standing: reservation.standing,
        already: false,
    })
}

/// `ids restore` (R-108): one commit returning each named file to the bytes
/// of its first addition, subject `restore: <ids>`.
///
/// # Errors
///
/// Returns an error for an id never added, nothing to restore, or a failed
/// push.
pub fn restore(root: &Path, ids: &[RegId]) -> Result<Vec<RegId>, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    let online = git.has_remote(AUTHORITY)?;
    let registry = if online {
        if let Fetched::Offline(reason) = fetch(&git, AUTHORITY)? {
            return Err(IdsError::Offline(reason));
        }
        verify_descent(&git, AUTHORITY)?;
        tracking_ref(AUTHORITY)
    } else {
        REGISTRY_REF.to_string()
    };
    let mut reason = String::new();
    for attempt in 0..PUSH_ATTEMPTS {
        if attempt > 0 {
            if let Fetched::Offline(reason) = fetch(&git, AUTHORITY)? {
                return Err(IdsError::Offline(reason));
            }
            verify_descent(&git, AUTHORITY)?;
        }
        let ledger = Ledger::read(&git, &registry)?;
        let Some(tip) = ledger.tip.clone() else {
            return Err(IdsError::Invalid(
                "there is no registry to restore".to_string(),
            ));
        };
        let tree: std::collections::HashMap<String, String> = git
            .tree(&tip, &["ids"])?
            .into_iter()
            .map(|(_, blob, path)| (path, blob))
            .collect();
        let mut files = Vec::new();
        let mut restored = Vec::new();
        for id in ids {
            let Some(first) = ledger.first_add(id) else {
                return Err(IdsError::Invalid(format!(
                    "{id} was never added to the registry"
                )));
            };
            let path = id.registry_path();
            if tree.get(&path) != Some(&first.blob) {
                files.push((path, first.blob.clone()));
                restored.push(id.clone());
            }
        }
        if files.is_empty() {
            return Err(IdsError::Invalid(
                "nothing to restore: every named file matches its first addition".to_string(),
            ));
        }
        let names: Vec<String> = restored.iter().map(ToString::to_string).collect();
        let commit =
            git.commit_files(Some(&tip), &files, &format!("restore: {}", names.join(" ")))?;
        let check = Ledger::read(&git, &commit)?;
        if let Some(finding) = check.range_violations(&git, Some(&tip))?.first() {
            return Err(IdsError::Invalid(format!(
                "restore refused: {}",
                finding.message
            )));
        }
        if !online {
            set_ref(&git, REGISTRY_REF, &commit, Some(&tip))?;
            return Ok(restored);
        }
        match push(&git, AUTHORITY, &commit)? {
            Pushed::Accepted => {
                adopt(&git, AUTHORITY, &commit)?;
                return Ok(restored);
            }
            Pushed::Moved(why) => reason = why,
            Pushed::Failed(error) => return Err(error),
            Pushed::Unclear(why) => return Err(IdsError::Transport(why)),
        }
    }
    Err(IdsError::Contended(PUSH_ATTEMPTS, reason))
}

/// `new --resume` (R-18): the pending write for `id`, when the reservation's
/// issuer and `uid` match this clone's state and the registry binds them.
///
/// # Errors
///
/// Returns an error naming why the write is refused.
pub fn resume(root: &Path, id: &RegId) -> Result<Unwritten, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    let state = state::load(&git)?;
    let key = id.to_string();
    let Some(unwritten) = state.unwritten.iter().find(|item| item.id == key).cloned() else {
        return Err(IdsError::Invalid(format!(
            "no reservation of {id} is waiting for its record in this clone"
        )));
    };
    let me = git.user_email()?;
    if unwritten.issuer != me {
        return Err(IdsError::Invalid(format!(
            "{id} was reserved by {}, not by {me}",
            unwritten.issuer
        )));
    }
    if git.has_remote(AUTHORITY)? {
        if let Fetched::Offline(_) = fetch(&git, AUTHORITY)? {
            // Offline: the local registry must hold the pending binding.
        }
    }
    let bound = [tracking_ref(AUTHORITY), REGISTRY_REF.to_string()]
        .iter()
        .map(|registry| Ledger::read(&git, registry))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .find_map(|ledger| ledger.entry(id).cloned());
    match bound {
        Some(entry) if entry.uid == unwritten.uid && entry.issuer == unwritten.issuer => {
            Ok(unwritten)
        }
        Some(_) => Err(IdsError::Invalid(format!(
            "the registry binds {id} to another reservation; it is not yours to write"
        ))),
        None => Err(IdsError::Invalid(format!(
            "{id} is not reserved in the registry; the reservation never landed, so run `new` again"
        ))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn r20_pending_registry_query_failure_refuses() {
        let root = tempfile::tempdir().unwrap();
        assert!(super::has_pending(&root.path().join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn r20_registry_remote_failure_does_not_select_local_authority() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_R20_REMOTE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let root = tempfile::tempdir().unwrap();
            let repo = git2::Repository::init(root.path()).unwrap();
            repo.remote("origin", "https://example.invalid/repo")
                .unwrap();
            assert!(super::sync(root.path()).is_err());
            return;
        }
        let programs = tempfile::tempdir().unwrap();
        let stub = programs.path().join("git");
        std::fs::write(&stub, "#!/bin/sh\ncase \"$3\" in remote|config) exit 128;; *) exec /usr/bin/git \"$@\";; esac\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ids::issue::tests::r20_registry_remote_failure_does_not_select_local_authority",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", programs.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn r20_registry_missing_ref_diagnostic_needs_remote_proof() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_R20_FETCH_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let root = tempfile::tempdir().unwrap();
            git2::Repository::init(root.path()).unwrap();
            let git = crate::ids::Git::new(root.path());
            assert!(matches!(
                super::fetch(&git, "origin"),
                Ok(super::Fetched::Offline(_)) | Err(_)
            ));
            return;
        }
        let programs = tempfile::tempdir().unwrap();
        let stub = programs.path().join("git");
        std::fs::write(&stub, "#!/bin/sh\ncase \"$3\" in fetch) printf \"fatal: couldn't find remote ref refs/heads/codeflow/registry\\n\" >&2; exit 128;; rev-parse) exit 1;; *) exit 128;; esac\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ids::issue::tests::r20_registry_missing_ref_diagnostic_needs_remote_proof",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", programs.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    use super::*;

    #[test]
    fn push_outcomes_are_classified_never_collapsed() {
        let moved = [
            " ! refs/heads/codeflow/registry [rejected] (fetch first)",
            "! a:refs/heads/codeflow/registry [remote rejected] (cannot lock ref 'refs/heads/codeflow/registry': is at x but expected y)",
            "[rejected] (non-fast-forward)",
        ];
        for text in moved {
            assert!(matches!(classify_push(text), Pushed::Moved(_)), "{text}");
        }
        assert!(matches!(
            classify_push("remote: error: GH013: Repository rule violations found"),
            Pushed::Failed(IdsError::Permission(_))
        ));
        assert!(matches!(
            classify_push("ERROR: Permission to x denied to y. 403"),
            Pushed::Failed(IdsError::Permission(_))
        ));
        assert!(matches!(
            classify_push("fatal: the remote end does not support atomic pushes"),
            Pushed::Failed(IdsError::Unsupported(_))
        ));
        assert!(matches!(
            classify_push("fatal: unable to access: Could not resolve host: example.invalid"),
            Pushed::Failed(IdsError::Transport(_))
        ));
        assert!(matches!(
            classify_push("codeflow pre-push: [block] registry.append_only"),
            Pushed::Failed(IdsError::Refused(_))
        ));
        assert!(matches!(
            classify_push("fatal: the remote end hung up unexpectedly"),
            Pushed::Unclear(_)
        ));
    }

    #[test]
    fn overlapping_refusals_are_never_retried_as_a_race() {
        let moved = [
            "! x:refs/heads/codeflow/registry [remote rejected] (cannot lock ref 'refs/heads/codeflow/registry': reference already exists)",
            "error: cannot update ref 'refs/heads/codeflow/registry': incorrect old value provided",
            " ! [rejected] x -> codeflow/registry (stale info)",
        ];
        for text in moved {
            assert!(matches!(classify_push(text), Pushed::Moved(_)), "{text}");
        }
        let permission = [
            "! [remote rejected] (cannot lock ref 'refs/heads/codeflow/registry': Unable to create '/srv/x.git/refs/heads/codeflow/registry.lock': Permission denied)",
            "remote: error: cannot lock ref 'refs/heads/codeflow/registry': is at a but expected b\nremote: error: GH013: Repository rule violations found",
        ];
        for text in permission {
            assert!(
                matches!(classify_push(text), Pushed::Failed(IdsError::Permission(_))),
                "{text}"
            );
        }
        assert!(matches!(
            classify_push("cannot lock ref 'refs/heads/codeflow/registry': unable to resolve reference: No such file or directory"),
            Pushed::Failed(IdsError::Transport(_))
        ));
        assert!(matches!(
            classify_push(
                "fatal: cannot lock ref: the receiving end does not support this operation"
            ),
            Pushed::Failed(IdsError::Unsupported(_))
        ));
        assert!(
            matches!(
                classify_push("error: failed to update ref 'refs/heads/codeflow/registry'"),
                Pushed::Unclear(_)
            ),
            "a bare update failure is read back, not retried"
        );
        // Object names never decide: a sha or path holding "403" or
        // "denied" is not a permission error.
        let named = [
            "To /tmp/permission-denied/remote.git\n!\tc7ccd4df81bce53a100957c13116e4d2bf794038:refs/heads/codeflow/registry\t[remote rejected] (incorrect old value provided)\nerror: failed to push some refs to '/tmp/permission-denied/remote.git'",
            "To /srv/403/remote.git\n!\tabc4030:refs/heads/codeflow/registry\t[rejected] (fetch first)",
        ];
        for text in named {
            assert!(matches!(classify_push(text), Pushed::Moved(_)), "{text}");
        }
        assert!(matches!(
            classify_push("remote: HTTP 403 Forbidden"),
            Pushed::Failed(IdsError::Permission(_))
        ));
    }
}

#[cfg(test)]
mod r22_tests {
    #[test]
    fn r22_choose_refuses_unreadable_inventory_and_keeps_empty_root() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let git = super::Git::new(dir.path());
        let request = super::Request::issue(crate::ids::Kind::Tsk, "task", "main");
        assert_eq!(
            super::choose(&git, &[], &request).unwrap().to_string(),
            "TSK-001"
        );
        std::fs::write(dir.path().join("project-management"), b"not a directory").unwrap();
        assert!(super::choose(&git, &[], &request).is_err());
    }
}
