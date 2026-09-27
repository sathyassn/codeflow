//! The record inventory on every ref (SPC-013 R-7, R-21, R-24): which ids
//! each local and remote-tracking branch holds, with their `uid`s and the
//! commits that introduced them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::entry::{frontmatter_value, record_id_from_path, Kind, RegId, RECORD_ROOTS};
use super::git::Git;
use super::{IdsError, REGISTRY_BRANCH};

/// One copy of a record in one tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub id: RegId,
    pub path: String,
    pub blob: String,
    pub uid: Option<String>,
}

/// The records one ref holds.
#[derive(Debug, Clone)]
pub struct RefRecords {
    pub refname: String,
    pub sha: String,
    pub copies: Vec<Copy>,
}

/// The branch name a ref stands for: `main` for `refs/heads/main` and for
/// `refs/remotes/origin/main`.
#[must_use]
pub fn branch_name(refname: &str) -> &str {
    if let Some(name) = refname.strip_prefix("refs/heads/") {
        return name;
    }
    refname
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.split_once('/'))
        .map_or(refname, |(_, name)| name)
}

/// Whether a branch is a landing line: `main`, `master` or `integration/*`.
#[must_use]
pub fn is_landing_branch(name: &str) -> bool {
    matches!(name, "main" | "master") || name.starts_with("integration/")
}

/// Every local and remote-tracking code branch: the registry is excluded.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn code_refs(git: &Git) -> Result<Vec<(String, String)>, IdsError> {
    Ok(git
        .branch_refs()?
        .into_iter()
        .filter(|(name, _)| branch_name(name) != REGISTRY_BRANCH)
        .collect())
}

/// The record copies in `rev`'s tree, with their `uid`s.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn copies_at(git: &Git, rev: &str) -> Result<Vec<Copy>, IdsError> {
    let files = git.tree(rev, &RECORD_ROOTS)?;
    let mut copies: Vec<Copy> = files
        .into_iter()
        .filter_map(|(_, blob, path)| {
            record_id_from_path(&path).map(|id| Copy {
                id,
                path,
                blob,
                uid: None,
            })
        })
        .collect();
    let blobs: Vec<String> = copies.iter().map(|copy| copy.blob.clone()).collect();
    let texts = git.blobs(&blobs)?;
    for copy in &mut copies {
        copy.uid = texts
            .get(&copy.blob)
            .and_then(|bytes| frontmatter_value(&String::from_utf8_lossy(bytes), "uid"));
    }
    Ok(copies)
}

/// The records on every code ref, reading each distinct tip once.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn on_every_ref(git: &Git) -> Result<Vec<RefRecords>, IdsError> {
    let mut cache: HashMap<String, Vec<Copy>> = HashMap::new();
    let mut out = Vec::new();
    for (refname, sha) in code_refs(git)? {
        let copies = if let Some(copies) = cache.get(&sha) {
            copies.clone()
        } else {
            let copies = copies_at(git, &sha)?;
            cache.insert(sha.clone(), copies.clone());
            copies
        };
        out.push(RefRecords {
            refname,
            sha,
            copies,
        });
    }
    Ok(out)
}

/// The highest canonical sequence of `kind` on any code ref (R-7).
///
/// # Errors
///
/// Returns an error when git fails.
pub fn max_seq_on_refs(git: &Git, kind: Kind) -> Result<u64, IdsError> {
    let mut seen = std::collections::HashSet::new();
    let mut max = 0;
    for (_, sha) in code_refs(git)? {
        if !seen.insert(sha.clone()) {
            continue;
        }
        let listing = {
            let mut args = vec![
                "ls-tree",
                "-r",
                "--name-only",
                "--full-tree",
                sha.as_str(),
                "--",
            ];
            args.extend_from_slice(&RECORD_ROOTS);
            git.run(&args)?
        };
        max = max.max(max_seq_in(listing.lines(), kind));
    }
    Ok(max)
}

/// The highest canonical sequence of `kind` in the working tree, so an
/// uncommitted record also counts.
#[must_use]
pub fn max_seq_in_worktree(root: &Path, kind: Kind) -> u64 {
    let mut paths = Vec::new();
    for base in RECORD_ROOTS {
        collect_files(root, &root.join(base), &mut paths, 0);
    }
    max_seq_in(paths.iter().map(String::as_str), kind)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_files(root, &path, out, depth + 1);
        } else if kind.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn max_seq_in<'a>(paths: impl Iterator<Item = &'a str>, kind: Kind) -> u64 {
    paths
        .filter_map(record_id_from_path)
        .filter(|id| id.kind() == kind)
        .filter_map(|id| id.seq())
        .max()
        .unwrap_or(0)
}

/// For each id in `rev`'s history, the first commit that added a record
/// file for it (R-111 `introduced`).
///
/// # Errors
///
/// Returns an error when git fails.
pub fn introductions(git: &Git, rev: &str) -> Result<BTreeMap<RegId, String>, IdsError> {
    let mut args = vec![
        "log",
        "--reverse",
        "--no-renames",
        "--diff-filter=A",
        "--name-only",
        "--format=%x1e%H",
        rev,
        "--",
    ];
    args.extend_from_slice(&RECORD_ROOTS);
    let log = git.run(&args)?;
    let mut out = BTreeMap::new();
    for record in log.split('\x1e').filter(|record| !record.trim().is_empty()) {
        let mut lines = record.lines();
        let sha = lines.next().unwrap_or_default().trim().to_string();
        for path in lines.filter(|line| !line.trim().is_empty()) {
            if let Some(id) = record_id_from_path(path.trim()) {
                out.entry(id).or_insert_with(|| sha.clone());
            }
        }
    }
    Ok(out)
}

/// Every commit in `rev`'s history that added a record file for `id`.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn adding_commits(git: &Git, rev: &str, id: &RegId) -> Result<Vec<String>, IdsError> {
    let mut args = vec![
        "log",
        "--no-renames",
        "--diff-filter=A",
        "--name-only",
        "--format=%x1e%H",
        rev,
        "--",
    ];
    args.extend_from_slice(&RECORD_ROOTS);
    let log = git.run(&args)?;
    let mut out = Vec::new();
    for record in log.split('\x1e').filter(|record| !record.trim().is_empty()) {
        let mut lines = record.lines();
        let sha = lines.next().unwrap_or_default().trim().to_string();
        if lines.any(|path| record_id_from_path(path.trim()).as_ref() == Some(id)) {
            out.push(sha);
        }
    }
    Ok(out)
}

/// The blob a commit added for `id`'s record file.
fn added_blob(git: &Git, commit: &str, id: &RegId) -> Option<String> {
    let changes = git
        .run(&[
            "diff-tree",
            "-r",
            "--root",
            "--no-renames",
            "--no-commit-id",
            "--diff-filter=A",
            commit,
        ])
        .ok()?;
    changes.lines().find_map(|line| {
        let (meta, path) = line.split_once('\t')?;
        (record_id_from_path(path).as_ref() == Some(id))
            .then(|| meta.split_whitespace().nth(3).map(str::to_string))
            .flatten()
    })
}

/// The landing of a copy introduced by `intro` (R-27, R-111): the commit
/// itself when a landing line holds it, else the landing line's own
/// introduction of `id` when it added the identical file (a cherry-pick).
/// `None` when the copy has not landed.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn landed_for(git: &Git, id: &RegId, intro: &str) -> Result<Option<String>, IdsError> {
    let landing: Vec<(String, String)> = code_refs(git)?
        .into_iter()
        .filter(|(name, _)| is_landing_branch(branch_name(name)))
        .collect();
    if landing.iter().any(|(_, sha)| git.is_ancestor(intro, sha)) {
        return Ok(Some(intro.to_string()));
    }
    let Some(blob) = added_blob(git, intro, id) else {
        return Ok(None);
    };
    for (_, sha) in &landing {
        if let Some(theirs) = introductions(git, sha)?.get(id) {
            if added_blob(git, theirs, id).as_deref() == Some(blob.as_str()) {
                return Ok(Some(theirs.clone()));
            }
        }
    }
    Ok(None)
}

/// Whether the copy of `id` in `rev`'s history is the record `entry`
/// registers, judged from the registry entry alone (R-111): an introducing
/// commit equal to `introduced` or to an entry of `mapped`, or a nonempty
/// landing equal to a nonempty `landed`. A matching number alone never
/// counts.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn is_replica(
    git: &Git,
    entry: &super::Entry,
    rev: &str,
    id: &RegId,
) -> Result<bool, IdsError> {
    let intros = adding_commits(git, rev, id)?;
    let known: Vec<&str> = entry
        .introduced_sha()
        .into_iter()
        .chain(entry.mapped.iter().map(String::as_str))
        .collect();
    if intros.iter().any(|intro| known.contains(&intro.as_str())) {
        return Ok(true);
    }
    let Some(landed) = entry.landed_sha() else {
        return Ok(false);
    };
    for intro in &intros {
        if intro == landed || landed_for(git, id, intro)?.as_deref() == Some(landed) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names_strip_heads_and_remotes() {
        assert_eq!(branch_name("refs/heads/main"), "main");
        assert_eq!(
            branch_name("refs/remotes/origin/codeflow/registry"),
            "codeflow/registry"
        );
        assert!(is_landing_branch("integration/EPC-020-delivery-system"));
        assert!(!is_landing_branch("task/TSK-101-id-registry"));
    }
}
