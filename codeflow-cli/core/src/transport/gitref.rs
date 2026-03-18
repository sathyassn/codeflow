//! Git ref-based transport for Loro CRDT delta synchronization.
//!
//! Each peer pushes its Loro deltas to a per-peer git ref at
//! `refs/coordination/loro/{peer-id}`. The sync daemon fetches all peer refs,
//! imports their deltas, and merges into the local `LoroDoc`.
//!
//! Git refs are transport-only -- the actual coordination is via Loro CRDT
//! merge, not git merge.

use std::path::Path;

use git2::Repository;

use crate::coordination::PeerId;
use crate::error::SyncError;

/// The ref namespace used for Loro coordination data.
const REF_PREFIX: &str = "refs/coordination/loro/";

/// Build the full ref name for a peer.
#[must_use]
pub fn peer_ref(peer_id: &PeerId) -> String {
    format!("{REF_PREFIX}{}", peer_id.as_str())
}

/// Push local Loro delta bytes to the peer's coordination ref.
///
/// Creates or updates the ref at `refs/coordination/loro/{peer_id}` by writing
/// the delta bytes as a git blob and pointing the ref at a tree containing it.
///
/// # Errors
///
/// Returns `SyncError::GitError` if git operations fail.
pub fn push_delta(repo_path: &Path, peer_id: &PeerId, delta_bytes: &[u8]) -> Result<(), SyncError> {
    let repo = Repository::open(repo_path)?;
    let blob_oid = repo.blob(delta_bytes)?;

    // Create a tree with a single "delta" entry pointing to the blob.
    let mut tree_builder = repo.treebuilder(None)?;
    tree_builder.insert("delta", blob_oid, 0o100_644)?;
    let tree_oid = tree_builder.write()?;

    // Point the ref at the tree.
    let refname = peer_ref(peer_id);
    repo.reference(&refname, tree_oid, true, "sync daemon: push delta")?;

    Ok(())
}

/// Fetch all peer delta refs from the remote.
///
/// Returns a list of `(PeerId, Vec<u8>)` pairs for each peer that has
/// a coordination ref. Skips the local peer to avoid self-import.
///
/// # Errors
///
/// Returns `SyncError::GitError` if git operations fail.
/// Returns `SyncError::ConnectionError` if the remote is unreachable.
pub fn fetch_peer_deltas(
    repo_path: &Path,
    local_peer: &PeerId,
) -> Result<Vec<(PeerId, Vec<u8>)>, SyncError> {
    let repo = Repository::open(repo_path)?;
    let mut results = Vec::new();

    // Iterate over all refs matching our coordination prefix.
    let refs = repo
        .references_glob(&format!("{REF_PREFIX}*"))
        .map_err(|e| SyncError::ConnectionError(format!("failed to list refs: {e}")))?;

    for reference in refs {
        let reference = reference?;
        let refname = match reference.name() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Extract peer ID from ref name.
        let peer_id_str = match refname.strip_prefix(REF_PREFIX) {
            Some(id) => id,
            None => continue,
        };

        let peer_id = PeerId::new(peer_id_str);

        // Skip local peer's own ref.
        if peer_id == *local_peer {
            continue;
        }

        // Resolve ref to tree, then read the "delta" blob.
        if let Some(target_oid) = reference.target() {
            if let Ok(bytes) = read_delta_from_tree(&repo, target_oid) {
                results.push((peer_id, bytes));
            }
        }
    }

    Ok(results)
}

/// Check whether the repository has any configured remotes.
///
/// Multi-machine mode is enabled when at least one remote exists.
///
/// # Errors
///
/// Returns `SyncError::GitError` if the repository cannot be opened.
pub fn has_remote(repo_path: &Path) -> Result<bool, SyncError> {
    let repo = Repository::open(repo_path)?;
    let remotes = repo.remotes()?;
    Ok(!remotes.is_empty())
}

/// Read the "delta" blob from a tree object.
fn read_delta_from_tree(repo: &Repository, tree_oid: git2::Oid) -> Result<Vec<u8>, SyncError> {
    let tree = repo.find_tree(tree_oid)?;
    let entry = tree
        .get_name("delta")
        .ok_or_else(|| SyncError::SchemaError("tree missing 'delta' entry".to_string()))?;

    let blob = repo.find_blob(entry.id())?;
    Ok(blob.content().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initialize a bare git repo for testing.
    fn init_test_repo(dir: &Path) -> Repository {
        Repository::init(dir).expect("init test repo")
    }

    // -- Positive tests --

    #[test]
    fn test_peer_ref_format() {
        let peer = PeerId::new("alice-laptop");
        assert_eq!(peer_ref(&peer), "refs/coordination/loro/alice-laptop");
    }

    #[test]
    fn test_peer_ref_with_hyphen() {
        let peer = PeerId::new("bob-desktop-home");
        assert_eq!(peer_ref(&peer), "refs/coordination/loro/bob-desktop-home");
    }

    #[test]
    fn test_push_delta_creates_ref() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let peer = PeerId::new("test-peer");
        let delta = b"test-delta-bytes";

        push_delta(dir.path(), &peer, delta).unwrap();

        // Verify ref exists.
        let repo = Repository::open(dir.path()).unwrap();
        let reference = repo
            .find_reference("refs/coordination/loro/test-peer")
            .unwrap();
        assert!(reference.target().is_some());
    }

    #[test]
    fn test_push_delta_updates_existing_ref() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let peer = PeerId::new("test-peer");

        push_delta(dir.path(), &peer, b"first").unwrap();
        push_delta(dir.path(), &peer, b"second").unwrap();

        // Should not error on update.
        let repo = Repository::open(dir.path()).unwrap();
        let reference = repo
            .find_reference("refs/coordination/loro/test-peer")
            .unwrap();
        assert!(reference.target().is_some());
    }

    #[test]
    fn test_push_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let peer = PeerId::new("peer-a");
        let delta = b"hello-crdt-world";

        push_delta(dir.path(), &peer, delta).unwrap();

        // Read back using fetch_peer_deltas with a different local peer.
        let local = PeerId::new("peer-local");
        let results = fetch_peer_deltas(dir.path(), &local).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.as_str(), "peer-a");
        assert_eq!(results[0].1, delta);
    }

    #[test]
    fn test_fetch_skips_local_peer() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        let local = PeerId::new("local-peer");
        let remote = PeerId::new("remote-peer");

        push_delta(dir.path(), &local, b"local-data").unwrap();
        push_delta(dir.path(), &remote, b"remote-data").unwrap();

        let results = fetch_peer_deltas(dir.path(), &local).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.as_str(), "remote-peer");
        assert_eq!(results[0].1, b"remote-data");
    }

    #[test]
    fn test_fetch_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let local = PeerId::new("local");

        let results = fetch_peer_deltas(dir.path(), &local).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fetch_multiple_peers() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        push_delta(dir.path(), &PeerId::new("peer-a"), b"data-a").unwrap();
        push_delta(dir.path(), &PeerId::new("peer-b"), b"data-b").unwrap();
        push_delta(dir.path(), &PeerId::new("peer-c"), b"data-c").unwrap();

        let local = PeerId::new("peer-local");
        let results = fetch_peer_deltas(dir.path(), &local).unwrap();
        assert_eq!(results.len(), 3);

        let peer_ids: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
        assert!(peer_ids.contains(&"peer-a"));
        assert!(peer_ids.contains(&"peer-b"));
        assert!(peer_ids.contains(&"peer-c"));
    }

    // -- has_remote tests --

    #[test]
    fn test_has_remote_no_remotes() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        assert!(!has_remote(dir.path()).unwrap());
    }

    #[test]
    fn test_has_remote_with_remote() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_test_repo(dir.path());
        repo.remote("origin", "https://example.com/repo.git")
            .unwrap();
        assert!(has_remote(dir.path()).unwrap());
    }

    // -- Error tests --

    #[test]
    fn test_push_delta_invalid_repo_path() {
        let peer = PeerId::new("test");
        let result = push_delta(Path::new("/nonexistent/repo"), &peer, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_fetch_peer_deltas_invalid_repo_path() {
        let local = PeerId::new("local");
        let result = fetch_peer_deltas(Path::new("/nonexistent/repo"), &local);
        assert!(result.is_err());
    }

    #[test]
    fn test_has_remote_invalid_repo_path() {
        let result = has_remote(Path::new("/nonexistent/repo"));
        assert!(result.is_err());
    }

    // -- Edge cases --

    #[test]
    fn test_push_empty_delta() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let peer = PeerId::new("peer");

        // Empty delta should succeed.
        push_delta(dir.path(), &peer, b"").unwrap();

        let local = PeerId::new("other");
        let results = fetch_peer_deltas(dir.path(), &local).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_empty());
    }

    #[test]
    fn test_push_large_delta() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let peer = PeerId::new("peer");

        // 1 MB delta.
        let large_data = vec![0xAB_u8; 1_000_000];
        push_delta(dir.path(), &peer, &large_data).unwrap();

        let local = PeerId::new("other");
        let results = fetch_peer_deltas(dir.path(), &local).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.len(), 1_000_000);
    }
}
