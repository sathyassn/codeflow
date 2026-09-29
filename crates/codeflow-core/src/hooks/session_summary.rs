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

use crate::ledger::{files, Event, JsonlWriter, LedgerError, LedgerWriter};

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
    /// A file stands where the ledger needs a directory.
    RemoveFile,
    /// A directory stands where the ledger writes a file.
    RemoveDirectory,
    /// The path exists but this user cannot write it.
    MakeWritable,
}

impl Repair {
    /// The remedy's words for this repair.
    #[must_use]
    pub fn words(self) -> &'static str {
        match self {
            Self::RemoveFile => {
                "remove or rename the file that stands where the ledger needs a directory"
            }
            Self::RemoveDirectory => {
                "remove the directory that stands where the ledger writes a file"
            }
            Self::MakeWritable => "give this user write access",
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
    let Some(info) = RepoInfo::discover(root) else {
        return Ok(None);
    };
    let summary = build(&info, &payload);
    append(&info, &summary).map(Some)
}

/// Gather the session facts from the repository state.
#[must_use]
pub fn build(info: &RepoInfo, payload: &serde_json::Value) -> SessionRecord {
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
    };

    let policy = Policy::load(&info.root);
    let (base_branch, changed_files, commits_on_branch) =
        diff_stats(&info.root, policy.git.default_base_branch());

    SessionRecord {
        session_id: str_field("session_id"),
        branch: info.branch.clone(),
        base_branch,
        changed_files,
        commits_on_branch,
        reason: str_field("reason"),
        timestamp: str_field("timestamp").unwrap_or_else(rfc3339_utc_now),
    }
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
    let session_id = summary
        .session_id
        .as_ref()
        .map(|id| normalize_session_id(id));

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
        session_id: session_id.clone(),
        worktree: Some(info.root.to_string_lossy().to_string()),
        data,
    };

    let writer =
        JsonlWriter::new_with_session(&ledger_dir, session_id.clone()).map_err(unwritten)?;
    writer.append_event(event).map_err(unwritten)?;

    let file = match &session_id {
        Some(sid) => format!("{t}/{t}-{sid}.jsonl", t = files::SESSIONS),
        None => format!("{t}/{t}.jsonl", t = files::SESSIONS),
    };
    Ok(ledger_dir.join(file))
}

/// What keeps the ledger from being written, from the path the writer
/// failed on: a file in the way of a directory it creates, a directory in
/// the way of a file it opens, or else the nearest existing part of that
/// path, which this user cannot write. The refusal record reads it too.
#[must_use]
pub fn unwritten(ledger_dir: &Path, error: LedgerError) -> LedgerUnwritten {
    let cause = error.to_string();
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
        _ => (existing(&path), Repair::MakeWritable),
    };
    LedgerUnwritten {
        path,
        cause,
        repair,
    }
}

/// Session fragment files follow the `…-ses-{id}.jsonl` convention the
/// ledger compactor expects.
fn normalize_session_id(id: &str) -> String {
    if id.starts_with("ses-") {
        id.to_string()
    } else {
        format!("ses-{id}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
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
            path.ends_with("sessions/sessions-ses-abc123.jsonl"),
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
        let dir = tempfile::tempdir().unwrap();
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
            (r#"{"session_id":"s1"}"#, "sessions-ses-s1.jsonl"),
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

    #[test]
    fn test_normalize_session_id() {
        assert_eq!(normalize_session_id("abc"), "ses-abc");
        assert_eq!(normalize_session_id("ses-abc"), "ses-abc");
    }
}
