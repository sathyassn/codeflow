//! Offline inventory of promised file contents needed by range judgments.
//!
//! Only partial clones need the inventory. libgit2 reads local object headers;
//! neither classification nor diagnostics starts a Git process or a transport.

use std::collections::{BTreeMap, BTreeSet};

use git2::{ErrorCode, ObjectType, Oid, Repository};

use super::GitName;
use crate::workgraph::history;

fn partial(repo: &Repository) -> Result<bool, String> {
    // Config errors can quote a remote name. Keep classification errors
    // static so neither that name nor URL userinfo reaches a diagnostic.
    let unreadable = || "cannot read partial-clone promisor configuration".to_string();
    let config = repo.config().map_err(|_| unreadable())?;
    match config.get_string("extensions.partialclone") {
        Ok(_) => return Ok(true),
        Err(error) if error.code() == ErrorCode::NotFound => {}
        Err(_) => return Err(unreadable()),
    }
    let mut entries = config
        .entries(Some(r"^remote\..*\.promisor$"))
        .map_err(|_| unreadable())?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|_| unreadable())?;
        let name = entry.name().map_err(|_| unreadable())?;
        if config.get_bool(name).map_err(|_| unreadable())? {
            return Ok(true);
        }
    }
    Ok(false)
}

type Missing = BTreeSet<(Vec<u8>, Oid)>;

fn fetch_commands(objects: &BTreeSet<Oid>, windows: bool) -> String {
    if windows {
        let steps = objects
            .iter()
            .map(|id| format!("git cat-file -e {id}"))
            .collect::<Vec<_>>()
            .join(" && ");
        format!("cmd /C \"set GIT_NO_LAZY_FETCH=0&& {steps}\"")
    } else {
        objects
            .iter()
            .map(|id| format!("GIT_NO_LAZY_FETCH=0 git cat-file -e {id}"))
            .collect::<Vec<_>>()
            .join(" && ")
    }
}

fn guidance(missing: &Missing) -> String {
    let paths = missing
        .iter()
        .map(|(path, id)| format!("{} (object {id})", GitName::from_bytes(path).display()))
        .collect::<Vec<_>>()
        .join(", ");
    let objects: BTreeSet<_> = missing.iter().map(|(_, id)| *id).collect();
    // Only typed object IDs enter commands. A promisor's name or URL is never
    // printed, so withheld remote names and URL credentials cannot leak.
    let commands = fetch_commands(&objects, cfg!(windows));
    format!(
        "partial clone is missing file contents: {paths}; no contents were fetched. Fetch them outside the check, then retry: {commands}. If the promisor is unavailable or does not hold an object, it remains unreadable and the check still refuses"
    )
}

/// Preserve the original object error and explain a missing promised blob.
/// Full clones and errors other than `NotFound` keep their existing text.
#[must_use]
pub fn object_error(repo: &Repository, path: &[u8], id: Oid, error: &git2::Error) -> String {
    explain_object(repo, path, id, error, error.to_string())
}

/// Add missing-content guidance while preserving the caller's original
/// unreadable-object refusal verbatim at its start.
#[must_use]
pub fn explain_object(
    repo: &Repository,
    path: &[u8],
    id: Oid,
    error: &git2::Error,
    original: String,
) -> String {
    if error.code() != ErrorCode::NotFound || !partial(repo).unwrap_or(false) {
        return original;
    }
    let missing = BTreeSet::from([(path.to_vec(), id)]);
    format!("{original}; {}", guidance(&missing))
}

#[derive(Default)]
struct Inventory {
    /// Missing paths relative to each tree. Unchanged subtrees are read once,
    /// including across the commits of a long range.
    trees: BTreeMap<Oid, Vec<(Vec<u8>, Oid)>>,
    headers: BTreeMap<Oid, Option<String>>,
    missing: Missing,
}

impl Inventory {
    fn tree(&mut self, repo: &Repository, id: Oid) -> Result<Vec<(Vec<u8>, Oid)>, String> {
        if let Some(known) = self.trees.get(&id) {
            return Ok(known.clone());
        }
        let tree = repo.find_tree(id).map_err(|error| error.to_string())?;
        let mut missing = Vec::new();
        for entry in &tree {
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    for (relative, blob) in self.tree(repo, entry.id())? {
                        let mut path = entry.name_bytes().to_vec();
                        path.push(b'/');
                        path.extend(relative);
                        missing.push((path, blob));
                    }
                }
                Some(ObjectType::Blob) => {
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        self.headers.entry(entry.id())
                    {
                        let error = match repo.odb().and_then(|odb| odb.read_header(entry.id())) {
                            Ok(_) => None,
                            Err(error) if error.code() == ErrorCode::NotFound => {
                                Some(error.to_string())
                            }
                            Err(error) => return Err(error.to_string()),
                        };
                        slot.insert(error);
                    }
                    if self.headers[&entry.id()].is_some() {
                        missing.push((entry.name_bytes().to_vec(), entry.id()));
                    }
                }
                // A gitlink names a commit in another repository, not a blob
                // promised by this one. Never ask this object store for it.
                _ => {}
            }
        }
        self.trees.insert(id, missing.clone());
        Ok(missing)
    }

    fn commit(&mut self, repo: &Repository, id: Oid) -> Result<(), String> {
        let commit = repo.find_commit(id).map_err(|error| error.to_string())?;
        let missing = self.tree(repo, commit.tree_id())?;
        self.missing.extend(missing);
        Ok(())
    }

    fn finish(&self) -> Result<(), String> {
        if self.missing.is_empty() {
            return Ok(());
        }
        let errors: BTreeSet<_> = self.headers.values().filter_map(Option::as_deref).collect();
        Err(format!(
            "cannot read file contents: {}; {}",
            errors.into_iter().collect::<Vec<_>>().join("; "),
            guidance(&self.missing)
        ))
    }
}

/// Refuse before a range reader can meet a missing promised blob. Include the
/// base (old-side diff contents) and any authority/baseline commits a caller
/// reads. No file contents are fetched or read, and no remote name enters
/// output. Full clones skip this inventory and keep their readers' existing
/// object-error refusals.
///
/// # Errors
/// Unreadable history, trees, object headers, or missing promised contents.
pub fn check_range(
    repo: &Repository,
    base: Option<&str>,
    head: &str,
    sources: &[&str],
) -> Result<(), String> {
    if !partial(repo)? {
        return Ok(());
    }
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| error.to_string())
    };
    let mut inventory = Inventory::default();
    let mut walk = history::revwalk(repo).map_err(|error| error.to_string())?;
    walk.push(commit(head)?)
        .map_err(|error| error.to_string())?;
    if let Some(base) = base {
        let base = commit(base)?;
        walk.hide(base).map_err(|error| error.to_string())?;
        inventory.commit(repo, base)?;
    }
    for id in walk {
        inventory.commit(repo, id.map_err(|error| error.to_string())?)?;
    }
    for source in sources {
        inventory.commit(repo, commit(source)?)?;
    }
    inventory.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_contents_commands_enable_retrieval_on_both_platforms() {
        let first = Oid::from_str(&"a".repeat(40)).unwrap();
        let second = Oid::from_str(&"b".repeat(40)).unwrap();
        let objects = BTreeSet::from([second, first]);
        assert_eq!(fetch_commands(&objects, false), format!("GIT_NO_LAZY_FETCH=0 git cat-file -e {first} && GIT_NO_LAZY_FETCH=0 git cat-file -e {second}"));
        assert_eq!(fetch_commands(&objects, true), format!("cmd /C \"set GIT_NO_LAZY_FETCH=0&& git cat-file -e {first} && git cat-file -e {second}\""));
    }

    #[test]
    fn missing_contents_commands_hold_only_ids_and_paths_use_git_display() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.config()
            .unwrap()
            .set_bool("remote.withheld-marker.promisor", true)
            .unwrap();
        let id = Oid::from_str(&"a".repeat(40)).unwrap();
        let error = git2::Error::new(
            ErrorCode::NotFound,
            git2::ErrorClass::Odb,
            "unreadable object",
        );
        let raw = b"odd\n\xff'$(entry)";
        let text = object_error(&repo, raw, id, &error);
        assert!(
            text.contains(&GitName::from_bytes(raw).display().to_string()),
            "{text}"
        );
        assert!(!text.contains('\n') && !text.contains('\u{fffd}'), "{text}");
        assert!(!text.contains("withheld-marker"), "{text}");
        assert!(text.contains(&format!("git cat-file -e {id}")), "{text}");
        assert!(text.starts_with(&error.to_string()), "{text}");
    }

    #[test]
    fn full_clone_and_nonmissing_errors_keep_original_text() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let id = Oid::from_str(&"a".repeat(40)).unwrap();
        let missing = git2::Error::new(
            ErrorCode::NotFound,
            git2::ErrorClass::Odb,
            "unreadable object",
        );
        assert_eq!(
            object_error(&repo, b"ordinary.txt", id, &missing),
            missing.to_string()
        );
        repo.config()
            .unwrap()
            .set_bool("remote.origin.promisor", false)
            .unwrap();
        assert_eq!(
            object_error(&repo, b"ordinary.txt", id, &missing),
            missing.to_string()
        );
        repo.config()
            .unwrap()
            .set_bool("remote.origin.promisor", true)
            .unwrap();
        let other = git2::Error::from_str("unreadable object of another kind");
        assert_eq!(
            object_error(&repo, b"ordinary.txt", id, &other),
            other.to_string()
        );
    }

    #[test]
    fn malformed_promisor_config_refuses_without_naming_the_remote() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.config()
            .unwrap()
            .set_str("remote.withheld-marker.promisor", "invalid-value")
            .unwrap();
        let error = check_range(&repo, None, "HEAD", &[]).unwrap_err();
        assert_eq!(error, "cannot read partial-clone promisor configuration");
        assert!(!error.contains("withheld-marker") && !error.contains("invalid-value"));
    }
}
