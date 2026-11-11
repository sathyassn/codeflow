//! `session-summary` — the `SessionEnd` hook (charter §3.3).
//!
//! Appends a session record to the sessions ledger: branch, changed-files
//! count vs the merge-base with the default (protected) branch, commits made
//! on the branch, and the end reason. Zero ceremony: this is recall's
//! automatically captured corpus. The hook **never fails the session** —
//! callers map every error to a warning + exit 0.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use git2::Repository;

use crate::ledger::{
    files, session_identity, Event, JsonlWriter, LedgerError, LedgerWriter, SessionIdentity,
};

use super::policy::Policy;
use super::repo::RepoInfo;
use super::rfc3339_utc_now;

/// The data captured for one session record.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub session_id: Option<String>,
    pub branch: String,
    pub base_branch: Option<String>,
    pub changed_files: usize,
    pub commits_on_branch: usize,
    pub reason: Option<String>,
    pub timestamp: String,
}

/// A ledger (the session or the refusals ledger) could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerUnwritten {
    /// What keeps the ledger from being written, from the path the write
    /// failed on: a file in the way of a directory, a directory in the way
    /// of a file, or the nearest existing part of that path this user
    /// cannot write.
    pub path: PathBuf,
    /// The error the write gave.
    pub cause: String,
    /// What clears it.
    pub repair: Repair,
}

/// The repair a ledger path needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repair {
    /// Obtaining the repository or policy inputs failed.
    RestoreInputs,
    /// A file stands where the ledger needs a directory.
    RemoveFile,
    /// A directory stands where the ledger writes a file.
    RemoveDirectory,
    /// The path exists but this user cannot write it.
    MakeWritable,
    /// Another process holds the ledger file's lock (the refusal record,
    /// which waits a bounded time).
    EndLockHolder,
}

impl Repair {
    /// The remedy's words for this repair.
    #[must_use]
    pub fn words(self) -> &'static str {
        match self {
            Self::RestoreInputs => "restore readable repository and policy inputs",
            Self::RemoveFile => {
                "remove or rename the file that stands where the ledger needs a directory"
            }
            Self::RemoveDirectory => {
                "remove the directory that stands where the ledger writes a file"
            }
            Self::MakeWritable => "give this user write access",
            Self::EndLockHolder => {
                "end the process that holds the lock on the ledger file, or wait for it"
            }
        }
    }
}

/// Build and append the session record for the repo containing `root`.
///
/// `payload_json` is the `SessionEnd` hook payload from stdin; unknown or
/// missing fields degrade gracefully. Returns the ledger file written.
///
/// # Errors
///
/// Returns [`LedgerUnwritten`] when the ledger append fails; callers treat
/// it as a warning (exit 0). Outside a git repository there is no session
/// to record: `Ok(None)`.
pub fn record(root: &Path, payload_json: &str) -> Result<Option<PathBuf>, LedgerUnwritten> {
    let payload: serde_json::Value =
        serde_json::from_str(payload_json.trim()).unwrap_or(serde_json::Value::Null);
    let Some(info) = RepoInfo::discover(root).map_err(|cause| LedgerUnwritten {
        path: root.to_path_buf(),
        cause,
        repair: Repair::RestoreInputs,
    })?
    else {
        return Ok(None);
    };
    let summary = build(&info, &payload)?;
    append(&info, &summary).map(Some)
}

/// Gather the session facts from the repository state.
///
/// # Errors
/// The policy needed to build the session summary cannot be read.
pub fn build(
    info: &RepoInfo,
    payload: &serde_json::Value,
) -> Result<SessionRecord, LedgerUnwritten> {
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
    };

    let policy = Policy::load(&info.root).map_err(|cause| LedgerUnwritten {
        path: info.root.join(".codeflow/policy.json"),
        cause,
        repair: Repair::RestoreInputs,
    })?;
    let (base_branch, changed_files, commits_on_branch) =
        diff_stats(&info.root, policy.git.default_base_branch());

    Ok(SessionRecord {
        session_id: str_field("session_id"),
        branch: info
            .branch_name
            .as_ref()
            .map_or_else(String::new, |name| name.display().to_string()),
        base_branch,
        changed_files,
        commits_on_branch,
        reason: str_field("reason"),
        timestamp: str_field("timestamp").unwrap_or_else(rfc3339_utc_now),
    })
}

/// Changed-file count (work tree + index vs merge-base) and commit count
/// (merge-base..HEAD) against the default base branch. All best-effort:
/// anything unresolvable degrades to zero counts.
fn diff_stats(root: &Path, base: Option<&str>) -> (Option<String>, usize, usize) {
    let Ok(repo) = Repository::discover(root) else {
        return (None, 0, 0);
    };
    let Some(base_name) = base else {
        return (None, 0, 0);
    };
    let Ok(head) = repo.head().and_then(|h| h.peel_to_commit()) else {
        return (None, 0, 0); // unborn HEAD: nothing to count yet
    };

    let base_commit = repo
        .revparse_single(base_name)
        .or_else(|_| repo.revparse_single(&format!("origin/{base_name}")))
        .ok()
        .and_then(|obj| obj.peel_to_commit().ok());
    let Some(base_commit) = base_commit else {
        return (None, 0, 0);
    };

    let Ok(merge_base) = repo.merge_base(head.id(), base_commit.id()) else {
        return (Some(base_name.to_string()), 0, 0);
    };

    let changed = repo
        .find_commit(merge_base)
        .and_then(|c| c.tree())
        .and_then(|tree| repo.diff_tree_to_workdir_with_index(Some(&tree), None))
        .map_or(0, |diff| diff.deltas().len());

    let commits = repo
        .revwalk()
        .and_then(|mut walk| {
            walk.push(head.id())?;
            walk.hide(merge_base)?;
            Ok(walk.count())
        })
        .unwrap_or(0);

    (Some(base_name.to_string()), changed, commits)
}

/// Append the record to the sessions ledger under the shared state dir.
fn append(info: &RepoInfo, summary: &SessionRecord) -> Result<PathBuf, LedgerUnwritten> {
    let ledger_dir = info.ledger_dir();
    let unwritten = |error: LedgerError| unwritten(&ledger_dir, error);
    let identity = identity_of(&ledger_dir, summary.session_id.as_deref())?;
    let session_id = identity.as_ref().map(|i| i.record_id.clone());
    let file_key = identity.map(|i| i.file_key);

    let mut data = HashMap::new();
    data.insert("branch".to_string(), serde_json::json!(summary.branch));
    data.insert(
        "changed_files".to_string(),
        serde_json::json!(summary.changed_files),
    );
    data.insert(
        "commits_on_branch".to_string(),
        serde_json::json!(summary.commits_on_branch),
    );
    if let Some(base) = &summary.base_branch {
        data.insert("base_branch".to_string(), serde_json::json!(base));
    }
    if let Some(reason) = &summary.reason {
        data.insert("reason".to_string(), serde_json::json!(reason));
    }

    let event = Event {
        event_type: "session_end".to_string(),
        timestamp: summary.timestamp.clone(),
        session_id,
        // Shown in the ledger (OS text rule, issue 79).
        worktree: Some(
            crate::git::GitName::from_os_str(info.root.as_os_str())
                .display()
                .to_string(),
        ),
        data,
    };

    let writer = JsonlWriter::new_with_session(&ledger_dir, file_key.clone()).map_err(unwritten)?;
    writer.append_event(event).map_err(unwritten)?;

    let file = match &file_key {
        Some(key) => format!("{t}/{t}-{key}.jsonl", t = files::SESSIONS),
        None => format!("{t}/{t}.jsonl", t = files::SESSIONS),
    };
    Ok(ledger_dir.join(file))
}

/// The sessions-ledger event for the model and effort a session runs at.
pub const EFFECTIVE_MODEL: &str = "effective_model";

/// The model and effort the user's harness started a session with. They are
/// recorded as given: the harness choice applies over the catalog's
/// suggestion, and nothing re-launches the session to change them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSession {
    pub model: String,
    pub effort: String,
    /// The host harness id the session runs in.
    pub host: String,
    /// The harness's own session id, the one its `SessionEnd` hook payload
    /// carries, so the record lands beside that session's summary.
    pub session_id: Option<String>,
    pub timestamp: String,
}

impl EffectiveSession {
    /// Parse `<model>@<effort>` as the harness reports them, for the
    /// harness session `session`.
    ///
    /// # Errors
    /// Either part is empty or holds a character outside printable ASCII
    /// (a space, a control or a non-ASCII character), or the `@` is missing
    /// or doubled; or the session id is empty, longer than
    /// [`MAX_SESSION_ID`] characters, or holds a character other than an
    /// ASCII letter, digit, `-` or `_`.
    pub fn parse(value: &str, host: &str, session: &str) -> Result<Self, String> {
        if session.is_empty()
            || session.len() > MAX_SESSION_ID
            || !session
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!(
                "session must be the harness session id: 1 to {MAX_SESSION_ID} ASCII letters, \
                 digits, `-` or `_`"
            ));
        }
        let (model, effort) = value
            .split_once('@')
            .ok_or("effective must be <model>@<effort>")?;
        for (part, label) in [(model, "model"), (effort, "effort")] {
            if part.is_empty() || part.contains('@') || !part.chars().all(|c| c.is_ascii_graphic())
            {
                return Err(format!(
                    "effective {label} must be one nonempty word of printable ASCII: \
                     <model>@<effort>"
                ));
            }
        }
        Ok(Self {
            model: model.into(),
            effort: effort.into(),
            host: host.into(),
            session_id: Some(session.into()),
            timestamp: rfc3339_utc_now(),
        })
    }
}

/// Append the session's effective model and effort to the sessions ledger
/// of the repository containing `root`. Outside a repository nothing is
/// recorded: `Ok(None)`.
///
/// # Errors
/// Returns [`LedgerUnwritten`] when the ledger cannot be written.
pub fn record_effective(
    root: &Path,
    session: &EffectiveSession,
) -> Result<Option<PathBuf>, LedgerUnwritten> {
    let Some(info) = RepoInfo::discover(root).map_err(|cause| LedgerUnwritten {
        path: root.to_path_buf(),
        cause,
        repair: Repair::RestoreInputs,
    })?
    else {
        return Ok(None);
    };
    let ledger_dir = info.ledger_dir();
    let identity = identity_of(&ledger_dir, session.session_id.as_deref())?;
    let session_id = identity.as_ref().map(|i| i.record_id.clone());
    let file_key = identity.map(|i| i.file_key);
    let mut data = HashMap::new();
    data.insert("model".to_string(), serde_json::json!(session.model));
    data.insert("effort".to_string(), serde_json::json!(session.effort));
    data.insert("host".to_string(), serde_json::json!(session.host));
    data.insert("source".to_string(), serde_json::json!("harness choice"));
    let event = Event {
        event_type: EFFECTIVE_MODEL.to_string(),
        timestamp: session.timestamp.clone(),
        session_id,
        worktree: Some(
            crate::git::GitName::from_os_str(info.root.as_os_str())
                .display()
                .to_string(),
        ),
        data,
    };
    let writer = JsonlWriter::new_with_session(&ledger_dir, file_key.clone())
        .map_err(|error| unwritten(&ledger_dir, error))?;
    writer
        .append_event(event)
        .map_err(|error| unwritten(&ledger_dir, error))?;
    let file = match &file_key {
        Some(key) => format!("{t}/{t}-{key}.jsonl", t = files::SESSIONS),
        None => format!("{t}/{t}.jsonl", t = files::SESSIONS),
    };
    Ok(Some(ledger_dir.join(file)))
}

/// What keeps the ledger from being written, from the path the writer
/// failed on: a file in the way of a directory it creates, a directory in
/// the way of a file it opens, or else the nearest existing part of that
/// path, which this user cannot write. The refusal record reads it too.
#[must_use]
pub fn unwritten(ledger_dir: &Path, error: LedgerError) -> LedgerUnwritten {
    let cause = error.to_string();
    if let LedgerError::LockTimeout { path, .. } = error {
        return LedgerUnwritten {
            path,
            cause,
            repair: Repair::EndLockHolder,
        };
    }
    let LedgerError::IoAt { path, source } = error else {
        return LedgerUnwritten {
            path: ledger_dir.to_path_buf(),
            cause,
            repair: Repair::MakeWritable,
        };
    };
    let existing = |path: &Path| {
        path.ancestors()
            .find(|part| part.exists())
            .map_or_else(|| path.to_path_buf(), Path::to_path_buf)
    };
    let (path, repair) = match source.kind() {
        ErrorKind::NotADirectory | ErrorKind::AlreadyExists => (
            path.ancestors()
                .find(|part| part.exists() && !part.is_dir())
                .map_or_else(|| existing(&path), Path::to_path_buf),
            Repair::RemoveFile,
        ),
        ErrorKind::IsADirectory => (path, Repair::RemoveDirectory),
        // Windows refuses to open a directory as a file with "access
        // denied" rather than naming it a directory.
        ErrorKind::PermissionDenied if path.is_dir() => (path, Repair::RemoveDirectory),
        _ => (existing(&path), Repair::MakeWritable),
    };
    LedgerUnwritten {
        path,
        cause,
        repair,
    }
}

/// The longest `--session` id accepted. The id no longer reaches a file
/// name (the file is named by a fixed-length hash), so this bounds only
/// the value written into each record.
pub const MAX_SESSION_ID: usize = 100;

/// The ledger identity for an optional harness session id, from the one
/// shared [`session_identity`]: `None` writes the base file. An empty id
/// is refused before anything touches the filesystem.
fn identity_of(
    ledger_dir: &Path,
    id: Option<&str>,
) -> Result<Option<SessionIdentity>, LedgerUnwritten> {
    id.map(session_identity)
        .transpose()
        .map_err(|cause| LedgerUnwritten {
            path: ledger_dir.to_path_buf(),
            cause,
            repair: Repair::RestoreInputs,
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo_with_branch_work(dir: &Path) {
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "chore: init"]);
        git(dir, &["checkout", "-b", "feat/x"]);
        std::fs::write(dir.join("one.txt"), "one\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "feat: one"]);
        // Uncommitted modification to a tracked file (untracked files are
        // not part of a git diff and are deliberately not counted).
        std::fs::write(dir.join("base.txt"), "base changed\n").unwrap();
    }

    #[test]
    fn test_record_appends_session_end_event() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let payload = r#"{"session_id":"abc123","reason":"clear","hook_event_name":"SessionEnd"}"#;

        let path = record(dir.path(), payload).unwrap().unwrap();
        assert!(
            // `sid-` and the first 128 bits of SHA-256("ses-abc123").
            path.ends_with("sessions/sessions-sid-63799fbfc9f9dab70ac4c347bb24a5a3.jsonl"),
            "{path:?}"
        );
        // Lands under the repo's shared state dir (canonicalize: macOS /var symlink).
        assert!(
            path.canonicalize()
                .unwrap()
                .starts_with(dir.path().canonicalize().unwrap().join(".git")),
            "{path:?}"
        );

        let line = std::fs::read_to_string(&path).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event["event"], "session_end");
        assert_eq!(event["session_id"], "ses-abc123");
        assert_eq!(event["branch"], "feat/x");
        assert_eq!(event["base_branch"], "main");
        // one committed file + one uncommitted tracked change vs merge-base.
        assert_eq!(event["changed_files"], 2);
        assert_eq!(event["commits_on_branch"], 1);
        assert_eq!(event["reason"], "clear");
        assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn test_record_without_session_id_uses_base_file() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let path = record(dir.path(), "{}").unwrap().unwrap();
        assert!(path.ends_with("sessions/sessions.jsonl"), "{path:?}");
        assert!(path.exists());
    }

    #[test]
    fn test_record_garbage_payload_still_records() {
        // Zero ceremony: malformed payloads must not lose the record.
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let path = record(dir.path(), "not json at all").unwrap().unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_record_outside_repo_records_nothing() {
        let Some(dir) = crate::fixture_tests::outside_repository() else {
            return;
        };
        assert_eq!(record(dir.path(), "{}"), Ok(None));
    }

    #[test]
    fn a_ledger_blocked_by_a_file_names_that_file() {
        // TSK-147 review F5: the warning names what keeps the ledger from
        // being written, under git's common directory, not `.codeflow/`.
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let blocker = dir.path().join(".git/codeflow");
        std::fs::write(&blocker, "not a directory").unwrap();
        let err = record(dir.path(), "{}").unwrap_err();
        assert_eq!(
            err.path.canonicalize().unwrap(),
            blocker.canonicalize().unwrap()
        );
        assert_eq!(err.repair, Repair::RemoveFile);
    }

    /// TSK-147 round 3 F5: the failed path itself, not an ancestor, with
    /// the repair its kind needs: a directory where the data file or its
    /// lock goes is removed.
    #[test]
    fn a_ledger_file_that_is_a_directory_names_that_directory() {
        for (payload, name) in [
            ("{}", "sessions.jsonl"),
            (
                r#"{"session_id":"s1"}"#,
                "sessions-sid-67f0646baf898daa03f0c8d0bd47c2ec.jsonl",
            ),
            ("{}", "sessions.jsonl.lock"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            repo_with_branch_work(dir.path());
            let sessions = dir.path().join(".git/codeflow/ledger/sessions");
            let blocker = sessions.join(name);
            std::fs::create_dir_all(&blocker).unwrap();
            let err = record(dir.path(), payload).unwrap_err();
            assert_eq!(
                err.path.canonicalize().unwrap(),
                blocker.canonicalize().unwrap(),
                "{name}: {err:?}"
            );
            assert_eq!(err.repair, Repair::RemoveDirectory, "{name}");
            assert!(!err.cause.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_ledger_directory_without_write_access_names_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let sessions = dir.path().join(".git/codeflow/ledger/sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o555)).unwrap();
        let err = record(dir.path(), "{}");
        std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = err.unwrap_err();
        assert_eq!(
            err.path.canonicalize().unwrap(),
            sessions.canonicalize().unwrap(),
            "{err:?}"
        );
        assert_eq!(err.repair, Repair::MakeWritable);
    }

    #[test]
    fn test_record_on_base_branch_counts_zero_commits() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "t@example.com"]);
        git(dir.path(), &["config", "user.name", "t"]);
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "chore: init"]);

        let path = record(dir.path(), r#"{"session_id":"s1"}"#)
            .unwrap()
            .unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event["commits_on_branch"], 0);
        assert_eq!(event["changed_files"], 0);
    }

    #[test]
    fn test_record_unborn_head_degrades_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let path = record(dir.path(), r#"{"session_id":"s2"}"#)
            .unwrap()
            .unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event["changed_files"], 0);
        assert_eq!(event["branch"], "main");
    }

    #[test]
    fn test_payload_timestamp_wins() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let payload = r#"{"session_id":"s3","timestamp":"2026-06-12T10:00:00Z"}"#;
        let path = record(dir.path(), payload).unwrap().unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.contains("2026-06-12T10:00:00Z"));
    }

    #[test]
    fn test_worktree_records_into_shared_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        repo_with_branch_work(&main);
        let wt = dir.path().join("wt");
        git(
            &main,
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat/wt"],
        );

        let path = record(&wt, r#"{"session_id":"wt1"}"#).unwrap().unwrap();
        // The record lands in the MAIN repo's .git/codeflow, not the worktree's.
        assert!(
            path.canonicalize()
                .unwrap()
                .starts_with(main.canonicalize().unwrap().join(".git")),
            "expected shared state dir, got {path:?}"
        );
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.contains("feat/wt"));
    }

    /// Distinct ids get distinct fixed-length keys of lowercase hex, which a
    /// filesystem cannot fold, and no key is a name an earlier release
    /// wrote (`ses-…`).
    #[test]
    fn session_keys_are_fixed_length_lowercase_hex_distinct_and_never_legacy() {
        let long = ["é".repeat(40), "x".repeat(4096)];
        let ids = [
            "alpha",
            "Alpha",
            "ALPHA",
            "_alpha",
            "__alpha",
            "ses-alpha",
            "a_b",
            "a__b",
            "../x",
            "é",
            "É",
            long[0].as_str(),
            long[1].as_str(),
        ];
        let mut folded = std::collections::HashSet::new();
        for id in ids {
            let identity = session_identity(id).unwrap();
            assert_eq!(identity.record_id, format!("ses-{id}"));
            let hex = identity.file_key.strip_prefix("sid-").unwrap();
            assert_eq!(hex.len(), 32, "{id}");
            assert!(
                hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{id}"
            );
            assert!(folded.insert(identity.file_key.to_lowercase()), "{id}");
        }
        // A legacy name and a new id that once shared a file: `_alpha` was
        // `sessions-ses-_alpha`, the name the 71282a421 encoding gave
        // `Alpha`; `Alpha` was `sessions-ses-Alpha`, which folds onto
        // `alpha`'s legacy name.
        for id in ["Alpha", "alpha", "_alpha"] {
            let key = session_identity(id).unwrap().file_key;
            for legacy in ["ses-_alpha", "ses-Alpha", "ses-alpha"] {
                assert_ne!(key.to_lowercase(), legacy.to_lowercase(), "{id}");
            }
        }
        assert!(session_identity("").is_err());
    }

    #[test]
    fn the_session_flag_is_bounded_and_the_key_length_is_not_the_ids() {
        for c in ["_", "Z", "a"] {
            let id = c.repeat(MAX_SESSION_ID);
            assert!(EffectiveSession::parse("m@high", "codex-cli", &id).is_ok());
            assert!(EffectiveSession::parse("m@high", "codex-cli", &format!("{id}a")).is_err());
        }
    }

    /// A long or non-ASCII id from the `SessionEnd` payload still writes:
    /// the file name has a fixed length, and the record keeps the id.
    #[test]
    fn a_long_or_non_ascii_session_end_id_still_records() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        for id in ["é".repeat(40), "Ω".repeat(300)] {
            let path = record(dir.path(), &format!(r#"{{"session_id":"{id}"}}"#))
                .unwrap()
                .unwrap();
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(name.len(), "sessions-sid-.jsonl".len() + 32, "{name}");
            let line = std::fs::read_to_string(&path).unwrap();
            let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(event["session_id"], format!("ses-{id}"));
        }
        let err = record(dir.path(), r#"{"session_id":""}"#).unwrap_err();
        assert!(err.cause.contains("empty"), "{err:?}");
    }

    #[test]
    fn ids_that_differ_by_case_prefix_or_underscore_never_share_a_file() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let ids = [
            "alpha",
            "Alpha",
            "ALPHA",
            "_alpha",
            "__alpha",
            "ses-alpha",
            "a_b",
            "a__b",
        ];
        let mut files = std::collections::HashSet::new();
        for id in ids {
            let end = record(dir.path(), &format!(r#"{{"session_id":"{id}"}}"#))
                .unwrap()
                .unwrap();
            let effective = record_effective(
                dir.path(),
                &EffectiveSession::parse("gpt-6.1-sol@medium", "codex-cli", id).unwrap(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(end, effective, "{id}: both records share its file");
            let key = session_identity(id).unwrap().file_key;
            assert!(end.ends_with(format!("sessions/sessions-{key}.jsonl")));
            files.insert(end);
        }
        for path in &files {
            let lines = std::fs::read_to_string(path).unwrap();
            let events: Vec<serde_json::Value> = lines
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            // On a case-insensitive volume a shared file would hold four.
            assert_eq!(
                events.len(),
                2,
                "{path:?}: only its own records: {events:?}"
            );
            let stored = events[0]["session_id"].as_str().unwrap();
            assert!(
                path.ends_with(format!(
                    "sessions-{}.jsonl",
                    crate::ledger::session_file_key(stored)
                )),
                "{path:?}"
            );
            assert_eq!(events[1]["session_id"], stored);
        }
        assert_eq!(files.len(), ids.len());
    }

    /// Ledger files an earlier release wrote keep their names and compact by
    /// the old rule, `sid-` files by their key, side by side. A session
    /// running across the upgrade has its records in two files: both
    /// compact once it ends, unless its id started `ses-`, which earlier
    /// releases recorded without the added prefix.
    #[test]
    fn legacy_and_new_fragments_compact_together_across_the_upgrade() {
        use crate::ledger::compact::compact_ledger_type;

        let dir = tempfile::tempdir().unwrap();
        repo_with_branch_work(dir.path());
        let ledger_dir = RepoInfo::discover(dir.path())
            .unwrap()
            .unwrap()
            .ledger_dir();
        let line = |event: &str, sid: &str| {
            format!(
                r#"{{"event":"{event}","timestamp":"2026-01-01T00:00:00Z","session_id":"{sid}"}}"#
            ) + "\n"
        };
        let sessions = ledger_dir.join("sessions");
        let graph = ledger_dir.join("work-graph");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&graph).unwrap();
        // An earlier release ran session `Old_1` to its end: it kept the
        // case and the `_` (`ses-Old_1`).
        std::fs::write(
            sessions.join("sessions-ses-Old_1.jsonl"),
            line("session_end", "ses-Old_1"),
        )
        .unwrap();
        let historical = graph.join("work-graph-ses-Old_1.jsonl");
        std::fs::write(&historical, line("task_created", "ses-Old_1")).unwrap();
        // Session `Run` is still running under the earlier release.
        let running = graph.join("work-graph-ses-Run.jsonl");
        std::fs::write(&running, line("task_created", "ses-Run")).unwrap();
        // Session `Split_A` began under that release and ends under this
        // one: its `session_end` goes to the `sid-` file, recorded as
        // `ses-Split_A` as before.
        let split = graph.join("work-graph-ses-Split_A.jsonl");
        std::fs::write(&split, line("task_created", "ses-Split_A")).unwrap();
        let end = record(dir.path(), r#"{"session_id":"Split_A"}"#)
            .unwrap()
            .unwrap();
        let key = crate::ledger::session_file_key("ses-Split_A");
        assert!(end.ends_with(format!("sessions/sessions-{key}.jsonl")));
        assert!(!sessions.join("sessions-ses-Split_A.jsonl").exists());
        // Session `ses-Gamma` began under that release, which recorded it
        // as `ses-Gamma`; this one records `ses-ses-Gamma`.
        let gamma = graph.join("work-graph-ses-Gamma.jsonl");
        std::fs::write(&gamma, line("task_created", "ses-Gamma")).unwrap();
        record(dir.path(), r#"{"session_id":"ses-Gamma"}"#)
            .unwrap()
            .unwrap();
        // A fragment under a new `sid-` key, for session `Delta`.
        let delta_key = crate::ledger::session_file_key("ses-Delta");
        let delta = graph.join(format!("work-graph-{delta_key}.jsonl"));
        std::fs::write(&delta, line("task_created", "ses-Delta")).unwrap();
        record(dir.path(), r#"{"session_id":"Delta"}"#)
            .unwrap()
            .unwrap();
        // The current session's `sid-` fragment is never merged.
        let current_key = crate::ledger::session_file_key("ses-Now");
        let current = graph.join(format!("work-graph-{current_key}.jsonl"));
        std::fs::write(&current, line("task_created", "ses-Now")).unwrap();
        record(dir.path(), r#"{"session_id":"Now"}"#)
            .unwrap()
            .unwrap();

        let result = compact_ledger_type(&ledger_dir, "work-graph", Some("ses-Now")).unwrap();
        assert!(!historical.exists(), "a completed legacy fragment merges");
        assert!(
            !split.exists(),
            "the split session's legacy fragment merges"
        );
        assert!(!delta.exists(), "a completed sid- fragment merges");
        assert!(running.exists(), "a running legacy session stays");
        assert!(gamma.exists(), "a ses- id recorded differently stays");
        assert!(current.exists(), "the current session stays");
        assert_eq!(result.merged_count, 3, "{result:?}");
        assert_eq!(result.skipped_active, 3, "{result:?}");
        let base = std::fs::read_to_string(graph.join("work-graph.jsonl")).unwrap();
        for sid in ["ses-Old_1", "ses-Split_A", "ses-Delta"] {
            assert!(base.contains(sid), "{sid}: {base}");
        }
        // The legacy files were never renamed or rewritten.
        assert_eq!(
            std::fs::read_to_string(sessions.join("sessions-ses-Old_1.jsonl")).unwrap(),
            line("session_end", "ses-Old_1")
        );
    }
}
