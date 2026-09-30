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
    let repository_state = match git2::Repository::open(root) {
        Ok(repository) => match repository.head() {
            Ok(head) => {
                let commit = head
                    .peel_to_commit()
                    .map_err(|e| PresentError::InvalidRequest(e.to_string()))?
                    .id()
                    .to_string();
                let mut options = git2::StatusOptions::new();
                options
                    .include_untracked(true)
                    .recurse_untracked_dirs(false);
                let dirty = !repository
                    .statuses(Some(&mut options))
                    .map_err(|e| PresentError::InvalidRequest(e.to_string()))?
                    .is_empty();
                Some(RepositoryContext { commit, dirty })
            }
            Err(e)
                if matches!(
                    e.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(e) => return Err(PresentError::InvalidRequest(e.to_string())),
        },
        Err(e) if e.code() == git2::ErrorCode::NotFound => None,
        Err(e) => return Err(PresentError::InvalidRequest(e.to_string())),
    };
    let commit = repository_state.as_ref().map(|c| c.commit.clone());
    let mut snapshots = Vec::new();
    if let RevisionContent::Supported { document } = content {
        for block in document.walk() {
            let (Block::Code { source, .. } | Block::Diff { source, .. }) = block else {
                continue;
            };
            let Some(source) = source else { continue };
            let mut path = root.to_path_buf();
            for component in Path::new(&source.path).components() {
                let Component::Normal(part) = component else {
                    return Err(PresentError::UnsafePath(path));
                };
                path.push(part);
                match std::fs::symlink_metadata(&path) {
                    Ok(meta) if crate::platform::is_link_like(&meta) => {
                        return Err(PresentError::UnsafePath(path))
                    }
                    Ok(_) => {}
                    // A diff may name a deleted file. Path and commit are provenance,
                    // not a claim that the snippet matches a current file's bytes.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(PresentError::io(&path, e)),
                }
            }
            if path.is_dir() {
                return Err(PresentError::UnsafePath(path));
            }
            snapshots.push(SourceSnapshot {
                block_id: block.id().into(),
                path: source.path.clone(),
                commit: commit.clone(),
            });
        }
    }
    Ok((repository_state, snapshots))
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
    fn legacy_revision_round_trips_without_new_fields() {
        let bytes = r#"{"state_schema_version":1,"revision":1,"created_at_unix":0,"content":{"kind":"unsupported","schema_version":99,"raw":"{}"}}"#;
        let record: crate::state::RevisionRecord = serde_json::from_str(bytes).unwrap();
        assert!(record.context.is_none());
        assert!(record.snapshots.is_empty());
        assert_eq!(serde_json::to_string(&record).unwrap(), bytes);
    }
}
