//! Immutable repository context and block-level revision comparisons.
use crate::{
    document::Block,
    state::{block_digest, FeedbackEvent, RevisionContent, SessionStore},
    PresentError, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryContext {
    pub commit: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    pub block_id: String,
    pub path: String,
    pub commit: Option<String>,
}

pub(crate) fn capture(
    root: &Path,
    content: &RevisionContent,
) -> Result<(Option<RepositoryContext>, Vec<SourceSnapshot>)> {
    let mut failed = false;
    let repository_state = repository_context(root).unwrap_or_else(|error| {
        warn_capture(error);
        failed = true;
        None
    });
    let commit = repository_state.as_ref().map(|c| c.commit.clone());
    let mut snapshots = Vec::new();
    if let RevisionContent::Supported { document } = content {
        for block in document.walk() {
            let (Block::Code { source, .. } | Block::Diff { source, .. }) = block else {
                continue;
            };
            let Some(source) = source else { continue };
            match check_source_path(root, &source.path) {
                Ok(()) => snapshots.push(SourceSnapshot {
                    block_id: block.id().into(),
                    path: source.path.clone(),
                    commit: commit.clone(),
                }),
                Err(error @ PresentError::Io { .. }) => {
                    warn_capture(error);
                    failed = true;
                }
                // A capture error does not excuse an unsafe path in another block.
                Err(error) => return Err(error),
            }
        }
    }
    if failed {
        Ok((None, Vec::new()))
    } else {
        Ok((repository_state, snapshots))
    }
}

fn warn_capture(error: impl std::fmt::Display) {
    eprintln!("present: warning: revision metadata omitted: {error}");
}

fn repository_context(root: &Path) -> std::result::Result<Option<RepositoryContext>, git2::Error> {
    let repository = match git2::Repository::open(root) {
        Ok(repository) => repository,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let head = match repository.head() {
        Ok(head) => head,
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let commit = head.peel_to_commit()?;
    // Staged changes already prove dirty without touching the working tree.
    let staged = repository.diff_tree_to_index(Some(&commit.tree()?), None, None)?;
    let dirty = if staged.deltas().len() > 0 {
        true
    } else {
        // A clean index still needs a workdir scan to detect tracked and
        // untracked changes. Do not repeat the HEAD/index comparison or
        // enumerate the contents of untracked directories.
        let mut options = git2::StatusOptions::new();
        options
            .show(git2::StatusShow::Workdir)
            .include_untracked(true)
            .recurse_untracked_dirs(false);
        !repository.statuses(Some(&mut options))?.is_empty()
    };
    Ok(Some(RepositoryContext {
        commit: commit.id().to_string(),
        dirty,
    }))
}

fn check_source_path(root: &Path, source: &str) -> Result<()> {
    let mut path = root.to_path_buf();
    let mut directory = false;
    for component in Path::new(source).components() {
        let Component::Normal(part) = component else {
            return Err(PresentError::UnsafePath(path));
        };
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if crate::platform::is_link_like(&meta) => {
                return Err(PresentError::UnsafePath(path))
            }
            Ok(meta) => directory = meta.is_dir(),
            // Diffs may name deleted files. Provenance does not certify bytes.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => directory = false,
            Err(error) => return Err(PresentError::io(&path, error)),
        }
    }
    if directory {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(())
}

impl SessionStore {
    /// Compare immutable block bodies, carrying only feedback known by the target revision.
    pub fn diff(&self, id: Uuid, from: u64, to: u64) -> Result<Value> {
        let history = self.history(id)?;
        let revision = |n| {
            history
                .revisions
                .iter()
                .find(|r| r.revision == n)
                .ok_or_else(|| PresentError::InvalidRequest(format!("unknown revision {n}")))
        };
        let source = revision(from)?;
        let target = revision(to)?;
        let document = |r: &crate::state::RevisionRecord| match &r.content {
            RevisionContent::Supported { document } => Ok(document.clone()),
            _ => Err(PresentError::InvalidRequest(
                "diff needs supported documents".into(),
            )),
        };
        let before = document(source)?;
        let after = document(target)?;
        let mut events = history.feedback_events;
        events.retain(
            |e| !matches!(e, FeedbackEvent::Received {envelope,..} if envelope.revision > to),
        );
        let sources = self.source_revisions(id, &events, target)?;
        let feedback = crate::state::build_feedback_snapshot(
            id,
            &events,
            &target.content,
            to,
            &sources,
            &std::collections::HashSet::new(),
        )?;
        let old = before.walk();
        let new = after.walk();
        let mut order = new.clone();
        order.extend(
            old.iter()
                .filter(|b| !new.iter().any(|n| n.id() == b.id()))
                .copied(),
        );
        let blocks: Vec<_> = order.iter().map(|b| {
            let previous = old.iter().find(|v| v.id() == b.id());
            let next = new.iter().find(|v| v.id() == b.id());
            let status = match (previous,next) {
                (None,_) => "added", (_,None) => "removed",
                (Some(a),Some(b)) if block_digest(a) == block_digest(b) => "unchanged", _ => "changed",
            };
            let notes: Vec<_> = feedback.items.iter().flat_map(|review| review.notes.iter().filter_map(|note| {
                let belongs = events.iter().any(|e| matches!(e, FeedbackEvent::Received {envelope,..}
                    if envelope.event_id == review.event_id && envelope.notes.iter().any(|n| n.id == note.id && n.block_id == b.id())));
                belongs.then(|| json!({"event_id":review.event_id,"source_revision":review.source_revision,
                    "carried":review.source_revision < to,"note":note}))
            })).collect();
            let answers: Vec<_> = history.response_events.iter().filter_map(crate::responses::ResponseEvent::answer)
                .filter(|a| a.revision <= to && a.form_id == b.id())
                .map(|a| json!({"carried":a.revision < to,"answer":a})).collect();
            json!({"id":b.id(),"status":status,"before":previous,"after":next,"notes":notes,"answers":answers})
        }).collect();
        Ok(json!({"session_id":id,"from":from,"to":to,"blocks":blocks}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::parse_document;

    #[test]
    fn context_captures_clean_dirty_and_source_paths_without_reading_file_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let repository = git2::Repository::init(root).unwrap();
        std::fs::write(root.join("sample.rs"), "fn main() {}\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("sample.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let head = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let crate::document::ParsedDocument::Supported(document) = parse_document(br#"{"schema_version":2,"title":"Source","blocks":[{"type":"code","id":"code","language":"rust","code":"fn main() {}","source":{"path":"sample.rs"}},{"type":"diff","id":"diff","diff":"+fn main() {}","source":{"path":"sample.rs"}}]}"#).unwrap() else { panic!() };
        let content = RevisionContent::Supported { document };
        let (repository_state, snapshots) = capture(root, &content).unwrap();
        assert_eq!(
            repository_state,
            Some(RepositoryContext {
                commit: head.to_string(),
                dirty: false
            })
        );
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].path, "sample.rs");
        assert_eq!(snapshots[0].commit, Some(head.to_string()));
        std::fs::write(root.join("sample.rs"), "changed").unwrap();
        assert!(capture(root, &content).unwrap().0.unwrap().dirty);
        index.add_path(Path::new("sample.rs")).unwrap();
        index.write().unwrap();
        assert!(capture(root, &content).unwrap().0.unwrap().dirty);
        std::fs::write(root.join("sample.rs"), "fn main() {}\n").unwrap();
        index.add_path(Path::new("sample.rs")).unwrap();
        index.write().unwrap();
        assert!(!capture(root, &content).unwrap().0.unwrap().dirty);
        std::fs::write(root.join("untracked.txt"), "untracked").unwrap();
        assert!(capture(root, &content).unwrap().0.unwrap().dirty);
        std::fs::remove_file(root.join("untracked.txt")).unwrap();
        #[cfg(unix)]
        {
            std::fs::remove_file(root.join("sample.rs")).unwrap();
            std::os::unix::fs::symlink("outside", root.join("sample.rs")).unwrap();
            assert!(matches!(
                capture(root, &content),
                Err(PresentError::UnsafePath(_))
            ));
        }
    }

    #[test]
    fn corrupt_git_metadata_is_optional() {
        let tmp = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init(tmp.path()).unwrap();
        let tree_id = repository.index().unwrap().write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        std::fs::write(repository.path().join("index"), "broken index").unwrap();
        let content = RevisionContent::Unsupported {
            schema_version: 99,
            raw: "{}".into(),
        };
        let (repository_state, snapshots) = capture(tmp.path(), &content).unwrap();
        assert!(repository_state.is_none());
        assert!(snapshots.is_empty());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("outside", tmp.path().join("unsafe.rs")).unwrap();
            let crate::document::ParsedDocument::Supported(document) = parse_document(br#"{"schema_version":2,"title":"Source","blocks":[{"type":"code","id":"code","language":"rust","code":"fn main() {}","source":{"path":"unsafe.rs"}}]}"#).unwrap() else { panic!() };
            assert!(matches!(
                capture(tmp.path(), &RevisionContent::Supported { document }),
                Err(PresentError::UnsafePath(_))
            ));
        }
    }

    #[test]
    fn legacy_revision_round_trips_without_new_fields() {
        let bytes = r#"{"state_schema_version":1,"revision":1,"created_at_unix":0,"content":{"kind":"unsupported","schema_version":99,"raw":"{}"}}"#;
        let record: crate::state::RevisionRecord = serde_json::from_str(bytes).unwrap();
        assert!(record.context.is_none());
        assert!(record.snapshots.is_empty());
        assert_eq!(serde_json::to_string(&record).unwrap(), bytes);
    }
}
