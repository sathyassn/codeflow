//! The record inventory on every ref (SPC-013 R-7, R-21, R-24): which ids
//! each local and remote-tracking branch holds, with their `uid`s and the
//! commits that introduced them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use super::entry::{frontmatter_value, record_id_from_path, Kind, RegId, RECORD_ROOTS};
use super::git::{z_fields, z_records, Git};
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
    matches!(name, "main" | "master")
        || name.starts_with(crate::hooks::policy::INTEGRATION_BRANCH_PREFIX)
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
                "-z",
                "--name-only",
                "--full-tree",
                sha.as_str(),
                "--",
            ];
            args.extend_from_slice(&RECORD_ROOTS);
            git.run_bytes(&args)?
        };
        max = max.max(max_seq_in(
            z_fields(&listing).iter().map(String::as_str),
            kind,
        ));
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
                out.push(crate::portable_path::slashed(relative));
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

/// Each id added in `rev`'s history: its adding commits, oldest first, and
/// the record paths it was added at. A merge result counts as an add where
/// it holds a record path that none of its parents holds.
type AddLog = BTreeMap<RegId, (Vec<String>, BTreeSet<String>)>;

fn add_log(git: &Git, rev: &str) -> Result<AddLog, IdsError> {
    let mut args = vec![
        "log",
        "--reverse",
        "--no-renames",
        "--cc",
        "--diff-filter=A",
        "--raw",
        "-z",
        "--format=%x1e%H",
        rev,
        "--",
    ];
    args.extend_from_slice(&RECORD_ROOTS);
    let log = git.run_bytes(&args)?;
    let mut out: AddLog = BTreeMap::new();
    for record in z_records(&log) {
        let mut fields = record.iter().map(String::as_str);
        let sha = fields.next().unwrap_or_default().to_string();
        for change in raw_fields(fields) {
            if change.status != 'A' {
                continue;
            }
            if let Some(id) = record_id_from_path(&change.path) {
                let (shas, paths) = out.entry(id).or_default();
                if !shas.contains(&sha) {
                    shas.push(sha.clone());
                }
                paths.insert(change.path);
            }
        }
    }
    Ok(out)
}

/// For each id in `rev`'s history, the commit that introduced it (R-111
/// `introduced`): for the copy `rev` holds, the start of that copy's
/// lifetime (see `lifetime_start`); for an id `rev` no longer holds, its
/// first add.
///
/// # Errors
///
/// Returns [`IdsError::Shallow`] on a shallow clone, and an error when git
/// fails.
pub fn introductions(git: &Git, rev: &str) -> Result<BTreeMap<RegId, String>, IdsError> {
    complete_history(git)?;
    let mut held = None;
    let mut out = BTreeMap::new();
    for (id, (shas, paths)) in add_log(git, rev)? {
        let intro = resolve(git, rev, &id, &shas, &paths, &mut held)?;
        out.insert(id, intro);
    }
    Ok(out)
}

/// The introduction of `id` in `rev`'s history, as [`introductions`]
/// gives it, or `None` when `rev`'s history never added it.
///
/// # Errors
///
/// Returns [`IdsError::Shallow`] on a shallow clone, and an error when git
/// fails.
pub fn introduction(git: &Git, rev: &str, id: &RegId) -> Result<Option<String>, IdsError> {
    Introductions::default().of(git, rev, id)
}

/// [`introduction`] for many ids and revisions: each revision's add log
/// and held ids are read once, however many ids are asked about it.
#[derive(Default)]
pub struct Introductions {
    checked: bool,
    logs: HashMap<String, AddLog>,
    held: HashMap<String, Option<BTreeSet<RegId>>>,
}

impl Introductions {
    /// The introduction of `id` in `rev`'s history, as [`introduction`]
    /// gives it.
    ///
    /// # Errors
    ///
    /// Returns [`IdsError::Shallow`] on a shallow clone, and an error when
    /// git fails.
    pub fn of(&mut self, git: &Git, rev: &str, id: &RegId) -> Result<Option<String>, IdsError> {
        if !self.checked {
            complete_history(git)?;
            self.checked = true;
        }
        if !self.logs.contains_key(rev) {
            self.logs.insert(rev.to_string(), add_log(git, rev)?);
        }
        let Some((shas, paths)) = self.logs[rev].get(id) else {
            return Ok(None);
        };
        let held = self.held.entry(rev.to_string()).or_default();
        resolve(git, rev, id, shas, paths, held).map(Some)
    }
}

fn resolve(
    git: &Git,
    rev: &str,
    id: &RegId,
    shas: &[String],
    paths: &BTreeSet<String>,
    held: &mut Option<BTreeSet<RegId>>,
) -> Result<String, IdsError> {
    let first = shas[0].clone();
    // One add is the introduction. This is sound only because the add log
    // holds merge additions too (`--cc` in `add_log`): every lifetime
    // starts with a commit that added the id against all its parents, so a
    // path added once was never re-added and the walk has nothing to pick.
    if shas.len() == 1 {
        return Ok(first);
    }
    if held.is_none() {
        let ids = git
            .tree(rev, &RECORD_ROOTS)?
            .into_iter()
            .filter_map(|(_, _, path)| record_id_from_path(&path))
            .collect();
        *held = Some(ids);
    }
    if !held.as_ref().is_some_and(|ids| ids.contains(id)) {
        return Ok(first);
    }
    Ok(lifetime_start(git, rev, id, paths, shas)?.unwrap_or(first))
}

/// The blobs `commit` holds for `id` at any of `paths`, cached.
fn held_blobs(
    git: &Git,
    commit: &str,
    id: &RegId,
    paths: &[&str],
    cache: &mut HashMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<String>, IdsError> {
    if let Some(blobs) = cache.get(commit) {
        return Ok(blobs.clone());
    }
    let blobs: BTreeSet<String> = git
        .tree(commit, paths)?
        .into_iter()
        .filter(|(_, _, path)| record_id_from_path(path).as_ref() == Some(id))
        .map(|(_, blob, _)| blob)
        .collect();
    cache.insert(commit.to_string(), blobs.clone());
    Ok(blobs)
}

/// The first commit of the lifetime of the copy of `id` that `rev` holds.
///
/// It walks git's simplified history of the id's record paths, so a merge
/// follows the parent its result came from and a merge that removes the
/// record counts as a removal. The lifetime runs back while some record
/// path holds the id, so a move under the same id continues it. Across a
/// stretch where no record path holds the id, it continues only when the
/// content that returns is identical to the content before (a revert, or a
/// round trip through a path that is not a record path); otherwise the
/// commit after the stretch starts a new record. With several starts, as
/// when two lines added the id and a merge kept both histories, the oldest
/// add wins.
fn lifetime_start(
    git: &Git,
    rev: &str,
    id: &RegId,
    paths: &BTreeSet<String>,
    adds: &[String],
) -> Result<Option<String>, IdsError> {
    let path_list: Vec<&str> = paths.iter().map(String::as_str).collect();
    let mut args = vec!["rev-list", "--parents", rev, "--"];
    args.extend_from_slice(&path_list);
    let listing = git.run(&args)?;
    let mut parents: HashMap<String, Vec<String>> = HashMap::new();
    let mut order = Vec::new();
    for line in listing.split_terminator('\n') {
        let mut shas = line
            .split(' ')
            .filter(|part| !part.is_empty())
            .map(str::to_string);
        let Some(commit) = shas.next() else {
            continue;
        };
        parents.insert(commit.clone(), shas.collect());
        order.push(commit);
    }
    let referenced: HashSet<&String> = parents.values().flatten().collect();
    let mut cache = HashMap::new();
    let mut stack = Vec::new();
    for head in order.iter().filter(|commit| !referenced.contains(commit)) {
        if !held_blobs(git, head, id, &path_list, &mut cache)?.is_empty() {
            stack.push(head.clone());
        }
    }
    let mut seen = HashSet::new();
    let mut starts = HashSet::new();
    while let Some(commit) = stack.pop() {
        if !seen.insert(commit.clone()) {
            continue;
        }
        let here = held_blobs(git, &commit, id, &path_list, &mut cache)?;
        let mut continues = false;
        for parent in parents.get(&commit).into_iter().flatten() {
            if !held_blobs(git, parent, id, &path_list, &mut cache)?.is_empty() {
                continues = true;
                stack.push(parent.clone());
                continue;
            }
            // A stretch without the id: find where it was last held.
            let mut gap = vec![parent.clone()];
            let mut crossed = HashSet::new();
            while let Some(absent) = gap.pop() {
                if !crossed.insert(absent.clone()) {
                    continue;
                }
                for before in parents.get(&absent).into_iter().flatten() {
                    let blobs = held_blobs(git, before, id, &path_list, &mut cache)?;
                    if blobs.is_empty() {
                        gap.push(before.clone());
                    } else if !blobs.is_disjoint(&here) {
                        continues = true;
                        stack.push(before.clone());
                    }
                }
            }
        }
        if !continues {
            starts.insert(commit);
        }
    }
    Ok(adds
        .iter()
        .find(|add| starts.contains(*add))
        .or_else(|| order.iter().rev().find(|commit| starts.contains(*commit)))
        .cloned())
}

/// The record files `commit` added, as `(path, blob)`: for a merge, the
/// record paths its result holds that none of its parents holds.
///
/// # Errors
///
/// Returns an error when git fails.
pub(crate) fn added_records(git: &Git, commit: &str) -> Result<Vec<(String, String)>, IdsError> {
    let changes = git.run_bytes(&[
        "diff-tree",
        "-r",
        "--root",
        "--no-renames",
        "--no-commit-id",
        "--cc",
        "--raw",
        "-z",
        commit,
    ])?;
    Ok(raw_changes(&changes)
        .into_iter()
        .filter(|change| change.status == 'A' && record_id_from_path(&change.path).is_some())
        .map(|change| (change.path, change.blob))
        .collect())
}

/// The blob a commit added for `id`'s record file.
fn added_blob(git: &Git, commit: &str, id: &RegId) -> Result<Option<String>, IdsError> {
    Ok(added_records(git, commit)?
        .into_iter()
        .find(|(path, _)| record_id_from_path(path).as_ref() == Some(id))
        .map(|(_, blob)| blob))
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
    landed_among(git, &landing_tips(git)?, id, intro, &mut HashMap::new())
}

/// The tips of every landing line (`main`, `master`, `integration/*`),
/// local and remote-tracking, in ref order.
///
/// # Errors
///
/// Returns an error when git fails.
pub(crate) fn landing_tips(git: &Git) -> Result<Vec<String>, IdsError> {
    let mut tips: Vec<String> = Vec::new();
    for (name, sha) in code_refs(git)? {
        if is_landing_branch(branch_name(&name)) && !tips.contains(&sha) {
            tips.push(sha);
        }
    }
    Ok(tips)
}

/// [`landed_for`] against given landing tips, reading each tip's
/// introductions once into `intros` (keyed by tip) so a caller judging
/// many copies walks each landing history only once.
///
/// # Errors
///
/// Returns an error when git fails.
pub(crate) fn landed_among(
    git: &Git,
    tips: &[String],
    id: &RegId,
    intro: &str,
    intros: &mut HashMap<String, BTreeMap<RegId, String>>,
) -> Result<Option<String>, IdsError> {
    for sha in tips {
        if git.is_ancestor(intro, sha)? {
            return Ok(Some(intro.to_string()));
        }
    }
    let Some(blob) = added_blob(git, intro, id)? else {
        return Ok(None);
    };
    for sha in tips {
        if !intros.contains_key(sha) {
            intros.insert(sha.clone(), introductions(git, sha)?);
        }
        if let Some(theirs) = intros[sha].get(id) {
            if added_blob(git, theirs, id)?.as_deref() == Some(blob.as_str()) {
                return Ok(Some(theirs.clone()));
            }
        }
    }
    Ok(None)
}

/// Whether the copy of `id` that `rev` holds is the record `entry`
/// registers, judged from the registry entry alone (R-111): its
/// introduction ([`introduction`]) equal to `introduced` or to an entry of
/// `mapped`, or its nonempty landing equal to a nonempty `landed`. An older
/// add of the id that another record's lifetime owns never counts, and a
/// matching number alone never counts.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn is_replica(
    git: &Git,
    intros: &mut Introductions,
    entry: &super::Entry,
    rev: &str,
    id: &RegId,
) -> Result<bool, IdsError> {
    let Some(intro) = intros.of(git, rev, id)? else {
        return Ok(false);
    };
    let known = entry.introduced_sha() == Some(intro.as_str()) || entry.mapped.contains(&intro);
    if known {
        return Ok(true);
    }
    let Some(landed) = entry.landed_sha() else {
        return Ok(false);
    };
    Ok(intro == landed || landed_for(git, id, &intro)?.as_deref() == Some(landed))
}

/// One entry of NUL-delimited raw diff output (`diff-tree -z`).
pub(crate) struct RawChange {
    pub blob: String,
    pub status: char,
    pub path: String,
}

/// Parse `diff-tree --raw -z` output: a `:meta` field, then its path.
pub(crate) fn raw_changes(output: &[u8]) -> Vec<RawChange> {
    raw_fields(z_fields(output).iter().map(String::as_str))
}

/// Parse raw diff fields. A combined entry (`--cc`, a merge) starts with
/// one colon per parent and carries one status letter per parent; it
/// takes a letter only when every parent agrees on it, else `M`, so `A`
/// means that no parent held the path. The blob is the result's.
fn raw_fields<'a>(mut fields: impl Iterator<Item = &'a str>) -> Vec<RawChange> {
    let mut changes = Vec::new();
    while let Some(meta) = fields.next() {
        let body = meta.trim_start_matches(':');
        let parents = meta.len() - body.len();
        if parents == 0 {
            continue;
        }
        let Some(path) = fields.next() else {
            break;
        };
        let parts: Vec<&str> = body.split(' ').filter(|part| !part.is_empty()).collect();
        if parts.len() != 2 * parents + 3 {
            continue;
        }
        let letters = parts[2 * parents + 2];
        let status = if parents == 1 {
            letters.chars().next().unwrap_or('?')
        } else {
            let mut chars = letters.chars();
            let first = chars.next().unwrap_or('?');
            if chars.all(|letter| letter == first) {
                first
            } else {
                'M'
            }
        };
        changes.push(RawChange {
            blob: parts[2 * parents + 1].to_string(),
            status,
            path: path.to_string(),
        });
    }
    changes
}

/// Fail on a shallow clone: its boundary commits look like adds, so an
/// introduction read from it would be invented (R-111).
fn complete_history(git: &Git) -> Result<(), IdsError> {
    if git.is_shallow()? {
        return Err(IdsError::Shallow);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn r20_registry_failed_added_blob_is_not_unlanded() {
        let root = tempfile::tempdir().unwrap();
        let git = super::Git::new(&root.path().join("missing"));
        let id = crate::ids::RegId::parse("TSK-001").unwrap();
        assert!(super::landed_among(
            &git,
            &[],
            &id,
            "HEAD",
            &mut std::collections::HashMap::new()
        )
        .is_err());
    }

    #[test]
    fn r20_registry_failed_ancestry_is_not_unlanded() {
        let root = tempfile::tempdir().unwrap();
        let git = super::Git::new(&root.path().join("missing"));
        let id = crate::ids::RegId::parse("TSK-001").unwrap();
        assert!(super::landed_among(
            &git,
            &["main".into()],
            &id,
            "HEAD",
            &mut std::collections::HashMap::new()
        )
        .unwrap_err()
        .to_string()
        .contains("merge-base"));
    }

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

    /// A hermetic scratch repository for the lifetime fixtures.
    struct Repo {
        dir: tempfile::TempDir,
    }

    impl Repo {
        fn new() -> Repo {
            let repo = Repo {
                dir: tempfile::tempdir().unwrap(),
            };
            repo.git(&["init", "-q", "-b", "main"]);
            repo.write("README.md", "fixture\n");
            repo.commit("root");
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let out = crate::git::command()
                .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
                .args(args)
                .current_dir(self.dir.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        fn write(&self, path: &str, text: &str) {
            let file = self.dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }

        fn remove(&self, path: &str) {
            std::fs::remove_file(self.dir.path().join(path)).unwrap();
        }

        fn commit(&self, message: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }

        /// Fork a side line, add unrelated work on both, and start their
        /// merge without committing it.
        fn start_merge(&self) {
            self.git(&["checkout", "-q", "-b", "side"]);
            self.write("side.txt", "side\n");
            self.commit("side");
            self.git(&["checkout", "-q", "main"]);
            self.write("main.txt", "main\n");
            self.commit("main");
            self.git(&["merge", "-q", "--no-ff", "--no-commit", "side"]);
        }

        fn handle(&self) -> Git {
            Git::new(self.dir.path())
        }
    }

    const TASK: &str = "project-management/tasks/TSK-001.md";
    const NESTED: &str = "project-management/epics/EPC-001/tasks/TSK-001.md";

    fn record(title: &str) -> String {
        format!("---\nid: TSK-001\ntitle: \"{title}\"\n---\n")
    }

    /// SL-4: the record is deleted, then a merge result adds a different
    /// record under its id at `path`. Returns the merge.
    fn merge_born(repo: &Repo, path: &str) -> String {
        repo.write(TASK, &record("original"));
        repo.commit("original");
        repo.remove(TASK);
        repo.commit("delete");
        repo.start_merge();
        repo.write(path, &record("replacement"));
        repo.commit("replacement in the merge")
    }

    /// SL-4: the record is renamed to another id, then a merge result
    /// renames it back, unchanged or one byte longer. Returns the
    /// original add and the merge.
    fn rename_back(repo: &Repo, changed: bool) -> (String, String) {
        repo.write(TASK, &record("original"));
        let original = repo.commit("original");
        repo.git(&["mv", TASK, "project-management/tasks/TSK-002.md"]);
        repo.commit("rename away");
        repo.start_merge();
        repo.git(&["mv", "project-management/tasks/TSK-002.md", TASK]);
        if changed {
            repo.write(TASK, &format!("{} ", record("original")));
        }
        (original, repo.commit("rename back in the merge"))
    }

    fn id() -> RegId {
        RegId::parse("TSK-001").unwrap()
    }

    #[test]
    fn the_add_log_holds_merge_additions_and_their_paths() {
        for path in [TASK, NESTED] {
            let repo = Repo::new();
            let merge = merge_born(&repo, path);
            let log = add_log(&repo.handle(), "HEAD").unwrap();
            let (shas, paths) = &log[&id()];
            assert_eq!(shas.last(), Some(&merge), "{path}: {shas:?}");
            assert!(paths.contains(path), "{path}: {paths:?}");
        }
        let repo = Repo::new();
        let (_, merge) = rename_back(&repo, true);
        let log = add_log(&repo.handle(), "HEAD").unwrap();
        assert_eq!(log[&id()].0.last(), Some(&merge), "{:?}", log[&id()]);
    }

    #[test]
    fn the_one_add_shortcut_agrees_with_the_forced_lifetime_walk() {
        let mut cases: Vec<(String, Repo, String)> = Vec::new();
        for path in [TASK, NESTED] {
            let repo = Repo::new();
            let merge = merge_born(&repo, path);
            cases.push((format!("merge-born at {path}"), repo, merge));
        }
        for changed in [false, true] {
            let repo = Repo::new();
            let (original, merge) = rename_back(&repo, changed);
            let expected = if changed { merge } else { original };
            cases.push((format!("rename back, changed={changed}"), repo, expected));
        }
        let repo = Repo::new();
        repo.write(TASK, &record("only"));
        let only = repo.commit("only add");
        repo.write(TASK, &record("edited"));
        repo.commit("edit");
        cases.push(("one add".to_string(), repo, only));
        for (name, repo, expected) in cases {
            let git = repo.handle();
            for (id, (shas, paths)) in add_log(&git, "HEAD").unwrap() {
                let shortcut = resolve(&git, "HEAD", &id, &shas, &paths, &mut None).unwrap();
                let walked = lifetime_start(&git, "HEAD", &id, &paths, &shas)
                    .unwrap()
                    .unwrap_or_else(|| shas[0].clone());
                let held = git
                    .tree("HEAD", &RECORD_ROOTS)
                    .unwrap()
                    .iter()
                    .any(|(_, _, path)| record_id_from_path(path).as_ref() == Some(&id));
                if held {
                    assert_eq!(shortcut, walked, "{name}: {id}");
                }
            }
            let intro = introduction(&git, "HEAD", &id()).unwrap();
            assert_eq!(intro.as_ref(), Some(&expected), "{name}");
        }
    }

    #[test]
    fn combined_entries_are_adds_only_when_no_parent_held_the_path() {
        let z = "0000000";
        let output = format!(
            ":000000 100644 {z} aaa A\0tasks/TSK-001.md\0\
             ::000000 000000 100644 {z} {z} bbb AA\0tasks/TSK-002.md\0\
             ::000000 100644 100644 {z} ccc ddd AM\0tasks/TSK-003.md\0\
             ::100644 100644 000000 eee fff {z} DD\0tasks/TSK-004.md\0"
        );
        let changes: Vec<(char, String, String)> = raw_changes(output.as_bytes())
            .into_iter()
            .map(|change| (change.status, change.blob, change.path))
            .collect();
        assert_eq!(
            changes,
            vec![
                ('A', "aaa".to_string(), "tasks/TSK-001.md".to_string()),
                ('A', "bbb".to_string(), "tasks/TSK-002.md".to_string()),
                ('M', "ddd".to_string(), "tasks/TSK-003.md".to_string()),
                ('D', z.to_string(), "tasks/TSK-004.md".to_string()),
            ]
        );
    }
}
