//! Read code-ref records with one blob batch and in-process tree listings.

use std::collections::{HashMap, HashSet};

use super::entry::{frontmatter_value, record_id_from_path, RECORD_ROOTS};
use super::git::Git;
use super::inventory::{Copy, RefRecords};
use super::IdsError;

/// Resolve range paths without reading their contents. Different paths may
/// name the same blob, and its bytes must only enter the batch once.
pub(super) fn blob_ids(git: &Git, specs: &[String]) -> Result<Vec<String>, IdsError> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let request = format!("{}\n", specs.join("\n"));
    let headers = git.run_input(&["cat-file", "--batch-check"], request.as_bytes())?;
    let headers: Vec<_> = headers.split_terminator('\n').collect();
    if headers.len() != specs.len() {
        return Err(IdsError::Git("git cat-file: truncated output".into()));
    }
    specs
        .iter()
        .zip(headers)
        .map(|(spec, header)| {
            if header.ends_with(" missing") {
                return Err(IdsError::Git(format!(
                    "git cat-file: requested blob {spec} is missing; repair the repository objects, then retry"
                )));
            }
            let fields: Vec<_> = header.split(' ').collect();
            let [oid, _kind, size] = fields.as_slice() else {
                return Err(IdsError::Git(format!("git cat-file: bad header {header}")));
            };
            if !matches!(oid.len(), 40 | 64)
                || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
                || size.parse::<usize>().is_err()
            {
                return Err(IdsError::Git(format!("git cat-file: bad header {header}")));
            }
            Ok((*oid).to_string())
        })
        .collect()
}

/// Keep ref order and Git's recursive tree order while sharing object reads.
/// `texts` includes blobs the merge rule has already read for its range.
pub(super) fn read(
    git: &Git,
    refs: Vec<(String, String)>,
    texts: &mut HashMap<String, Vec<u8>>,
) -> Result<Vec<RefRecords>, IdsError> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    let repo = git2::Repository::open(git.root()).map_err(|error| tree_error(&error))?;
    let mut trees = HashMap::new();
    let mut unread = Vec::new();
    let mut asked = HashSet::new();
    for (_, sha) in &refs {
        if !trees.contains_key(sha) {
            let copies = list(&repo, sha)?;
            for copy in &copies {
                if !texts.contains_key(&copy.blob) && asked.insert(copy.blob.clone()) {
                    unread.push(copy.blob.clone());
                }
            }
            trees.insert(sha.clone(), copies);
        }
    }
    texts.extend(git.blobs(&unread)?);

    let mut uids = HashMap::new();
    let mut out = Vec::new();
    for (refname, sha) in refs {
        let mut copies = trees[&sha].clone();
        for copy in &mut copies {
            if !uids.contains_key(&copy.blob) {
                let text = std::str::from_utf8(&texts[&copy.blob]).map_err(|error| {
                    IdsError::Git(format!("record {} is not valid UTF-8: {error}", copy.path))
                })?;
                uids.insert(copy.blob.clone(), frontmatter_value(text, "uid"));
            }
            copy.uid = uids[&copy.blob].clone();
        }
        out.push(RefRecords {
            refname,
            sha,
            copies,
        });
    }
    Ok(out)
}

fn list(repo: &git2::Repository, sha: &str) -> Result<Vec<Copy>, IdsError> {
    let oid = git2::Oid::from_str(sha).map_err(|error| tree_error(&error))?;
    let tree = repo
        .find_commit(oid)
        .and_then(|commit| commit.tree())
        .map_err(|error| tree_error(&error))?;
    let mut copies = Vec::new();
    crate::git::walk_tree(repo, &tree, &mut |name, entry| {
        let path = name.bytes();
        // Skip unrelated subtrees, including any that cannot be read. Only
        // the same two roots as `ls-tree -- RECORD_ROOTS` belong to this scan.
        let inside = RECORD_ROOTS.iter().any(|root| {
            path == root.as_bytes()
                || path
                    .strip_prefix(root.as_bytes())
                    .is_some_and(|rest| rest.starts_with(b"/"))
        });
        let ancestor = RECORD_ROOTS.iter().any(|root| {
            root.as_bytes()
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with(b"/"))
        });
        if !inside && !ancestor {
            return crate::git::Walk::SkipTree;
        }
        if entry.kind() != Some(git2::ObjectType::Tree) && inside {
            let path = name.storage_key();
            if let Some(id) = record_id_from_path(&path) {
                copies.push(Copy {
                    id,
                    path,
                    blob: entry.id().to_string(),
                    uid: None,
                });
            }
        }
        crate::git::Walk::Continue
    })
    .map_err(|error| tree_error(&error))?;
    Ok(copies)
}

fn tree_error(error: &git2::Error) -> IdsError {
    IdsError::Git(format!("cannot list code-ref records: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_keep_git_order_and_exact_path_names() {
        let (dir, repo, _) = super::super::r22_fixture::repository(b"README.md", b"base");
        let sha = crate::git::add_commit(
            &repo,
            &[
                (b"docs/decisions/ADR-001.md", b"first"),
                (
                    b"project-management/epics/EPC-001/tasks/TSK-001.md",
                    b"nested",
                ),
                (b"project-management/tasks/TSK-002.md", b"second"),
                (
                    b"project-management/odd\xff/TSK-003.md",
                    b"invalid directory",
                ),
                (b"unrelated/tasks/TSK-004.md", b"outside"),
            ],
        );
        let expected: Vec<_> = Git::new(dir.path())
            .tree(&sha.to_string(), &RECORD_ROOTS)
            .unwrap()
            .into_iter()
            .filter(|(_, _, path)| record_id_from_path(path).is_some())
            .map(|(_, blob, path)| (blob, path))
            .collect();
        let actual: Vec<_> = list(&repo, &sha.to_string())
            .unwrap()
            .into_iter()
            .map(|copy| (copy.blob, copy.path))
            .collect();
        assert_eq!(actual, expected);
    }
}
