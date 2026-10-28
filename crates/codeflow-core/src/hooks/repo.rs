//! Repository discovery for the hook plane.
//!
//! Hooks fire from arbitrary working directories (worktrees included); this
//! resolves the project root, the *common* git dir (runtime state under
//! `.git/codeflow/` is shared across worktrees, charter §4.1), and the
//! current branch.

use std::path::{Path, PathBuf};

use git2::Repository;

use crate::git::GitName;
use crate::hooks::policy::NON_UTF8_BRANCH;

/// Whether the git directory `dir` is on a case-insensitive filesystem, so
/// ref names that differ there may name one loose ref (TSK-381 SF-3). The
/// probe reads `HEAD` and a lower-case spelling of it: the same file means
/// case-insensitive, a missing one case-sensitive. A probe that cannot decide
/// counts as case-insensitive, failing closed.
#[must_use]
pub fn case_insensitive_git_dir(dir: &Path) -> bool {
    let Ok(upper) = std::fs::symlink_metadata(dir.join("HEAD")) else {
        return true;
    };
    match crate::absence::symlink_metadata_optional(&dir.join("head")) {
        Ok(None) => false,
        Err(_) => true,
        Ok(Some(lower)) => same_file(&upper, &lower),
    }
}

/// The name a filesystem that folds case may store as the same entry: an
/// approximation of APFS name comparison, which ignores both case and
/// Unicode normalization. The name is decomposed, case folded (upper case,
/// then lower case: `ſ` and `K` fold to `s` and `k`) and recomposed, so an
/// NFD `café` and an NFC `café` fold alike (TSK-381 SF-8).
#[must_use]
pub fn folded_ref_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    let decomposed: String = name.nfd().collect();
    decomposed.to_uppercase().to_lowercase().nfc().collect()
}

/// The commit `name` points to, read only when git lists a ref by exactly
/// that spelling and no other listed spelling folds to it (TSK-381 SF-4). On
/// a case-insensitive filesystem a loose `refs/remotes/origin/MAIN` also
/// opens as `refs/remotes/origin/main`, so a lookup by name can return a
/// ref fetched under another spelling. Every ref is listed from `refs/` down
/// and compared byte for byte: a listing glob that named a directory along
/// `name` would open it under the spelling asked for and report the stored
/// `INTEGRATION/x` as `integration/x` (SF-7), so no glob is used. This catches
/// an alias while the genuine ref is packed or absent; once a loose alias has
/// been written over a loose genuine ref, the stored spelling is the genuine
/// one, and the reference-transaction hook's collision refusal is the control.
///
/// # Errors
///
/// Returns the git error when the references cannot be listed or peeled.
pub fn listed_ref_commit(repo: &Repository, name: &str) -> Result<Option<git2::Oid>, git2::Error> {
    let folded = folded_ref_name(name);
    let (mut exact, mut alias) = (None, false);
    for reference in repo.references()? {
        let reference = reference?;
        let listed = reference.name_bytes();
        if listed == name.as_bytes() {
            exact = Some(reference.peel_to_commit()?.id());
            continue;
        }
        // A name that is not valid UTF-8 is compared byte for byte: packed-refs
        // matches names exactly, ext4 casefold treats such a name as opaque
        // bytes and does not fold it, and APFS refuses to store it.
        if let Ok(text) = GitName::from_bytes(listed).rule_text() {
            alias |= folded_ref_name(text) == folded;
        }
    }
    Ok(exact.filter(|_| !alias))
}

#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Without a file identity to compare, a `head` beside `HEAD` is taken as
/// the same entry, failing closed.
#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    true
}

/// Resolved repository context for a hook invocation.
pub struct RepoInfo {
    /// Working-tree root (the worktree's own root when in a worktree).
    pub root: PathBuf,
    /// The common git directory (`.git` of the main checkout).
    pub common_dir: PathBuf,
    /// Current branch name as the hook plane matches it: empty when detached
    /// only; [`NON_UTF8_BRANCH`] when the name is not valid UTF-8.
    pub branch: String,
    /// The same branch, exact, for output.
    pub branch_name: Option<GitName>,
    /// `true` when this checkout is a linked worktree.
    pub is_worktree: bool,
}

impl RepoInfo {
    /// Discover the repository containing `start`, walking parents.
    /// Returns `None` when `start` is not inside a git repository.
    ///
    /// # Errors
    /// Repository discovery or HEAD cannot be read, or the repository has no worktree.
    pub fn discover(start: &Path) -> Result<Option<Self>, String> {
        let Some(repo) = open(start)? else {
            return Ok(None);
        };
        // Hook context must validate every protected checkout before policy
        // selection. `open` checks the current checkout's identity separately,
        // so read-only inventory tools can still diagnose missing siblings.
        for admin in registrations(&repo)? {
            checked_registration(&admin)?;
        }
        let root = repo
            .workdir()
            .ok_or("repository has no worktree")?
            .to_path_buf();
        let common_dir = repo.commondir().to_path_buf();
        let branch_name = current_branch_name(&repo)?;
        let branch = branch_rule_text(branch_name.as_ref());
        let is_worktree = repo.is_worktree();
        Ok(Some(Self {
            root,
            common_dir,
            branch,
            branch_name,
            is_worktree,
        }))
    }

    /// Runtime state directory, shared across worktrees: `<common>/codeflow`.
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.common_dir.join("codeflow")
    }

    /// Ledger directory under the state dir.
    #[must_use]
    pub fn ledger_dir(&self) -> PathBuf {
        self.state_dir().join("ledger")
    }
}

/// The checked-out branch exactly, or `None` only for a proven detached HEAD.
/// Unborn branches keep their symbolic name so protection applies before a commit.
///
/// # Errors
/// HEAD or its target cannot be read or is malformed.
pub fn current_branch_name(repo: &Repository) -> Result<Option<GitName>, String> {
    match crate::root_checkout::head(repo)? {
        crate::root_checkout::Head::Branch(name) => Ok(Some(name)),
        crate::root_checkout::Head::Detached(_) => Ok(None),
    }
}

/// Current branch rule text, empty only for a proven detached HEAD.
/// A non-UTF8 branch remains [`NON_UTF8_BRANCH`], which every branch rule
/// treats as protected; [`current_branch_name`] preserves its exact bytes.
///
/// # Errors
/// HEAD or its target cannot be read or is malformed.
pub fn current_branch(repo: &Repository) -> Result<String, String> {
    Ok(branch_rule_text(current_branch_name(repo)?.as_ref()))
}

fn branch_rule_text(name: Option<&GitName>) -> String {
    name.map_or_else(String::new, |name| {
        name.rule_text()
            .map_or_else(|_| NON_UTF8_BRANCH.to_string(), str::to_string)
    })
}

/// Open the repository at or above `start`; only a proven non-repository is absent.
///
/// # Errors
/// Discovery fails, including an existing repository marker with malformed metadata.
pub fn open(start: &Path) -> Result<Option<Repository>, String> {
    match Repository::discover(crate::portable_path::without_verbatim(start.to_path_buf())) {
        Ok(repo) => {
            // Derive repository claims from the physical metadata. On Windows,
            // opening a directory symlink can otherwise report its alias as
            // the workdir and hide a conflicting checkout at the destination.
            let directory = canonical(repo.path(), "discovered git directory")?;
            let repo = Repository::open(crate::portable_path::without_verbatim(directory))
                .map_err(|error| {
                    format!("cannot open discovered git directory: {}", error.message())
                })?;
            let (discovered, repo) = check_discovery_boundaries(start, repo)?;
            if repo.is_bare()
                && repo
                    .commondir()
                    .file_name()
                    .is_some_and(|name| name == ".git")
            {
                return Err(format!("repository at {} reports no work tree; its checkout enforcement cannot be verified", repo.commondir().display()));
            }
            if repo.is_worktree() {
                // Git discovers the checkout at its .git pointer, not at the
                // location claimed by the administrative registration. Read
                // that registration with native semantics only to cross-check
                // discovery (libgit2 can report `./` for Windows backslashes).
                let registered = checked_registration(repo.path())?.ok_or_else(|| {
                    registration_error("discovered checkout has an absent registration target")
                })?;
                if registered != discovered {
                    return Err(registration_error(
                        "worktree registration disagrees with the discovered checkout",
                    ));
                }
            } else if !repo.is_bare()
                && discovered != canonical(repo.path(), "git directory")?
                && claims_work_directory(&repo)?
                && repo
                    .workdir()
                    .map(|path| canonical(path, "work directory"))
                    .transpose()?
                    .as_ref()
                    != Some(&discovered)
            {
                return Err(
                    "repository work directory disagrees with the discovered checkout".into(),
                );
            }
            if !repo.is_bare() && discovered != canonical(repo.path(), "git directory")? {
                // `false` changes this handle only, never either pointer file.
                repo.set_workdir(&discovered, false).map_err(|error| {
                    format!("cannot resolve worktree directory: {}", error.message())
                })?;
            }
            Ok(Some(repo))
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            // libgit2 also reports NotFound for an existing .git with an invalid
            // HEAD. Only an empty directory is proven to contain no repository;
            // gitfiles, partial metadata and unreadable directories must refuse.
            for ancestor in start.ancestors() {
                let marker = ancestor.join(".git");
                if !crate::absence::proven_absent(&marker)
                    .map_err(|failure| format!("cannot inspect repository marker: {failure}"))?
                {
                    match std::fs::symlink_metadata(&marker) {
                        Ok(meta) if meta.is_dir() => {
                            let mut entries =
                                std::fs::read_dir(ancestor.join(".git")).map_err(|failure| {
                                    format!("cannot inspect repository directory: {failure}")
                                })?;
                            if entries
                                .next()
                                .transpose()
                                .map_err(|failure| {
                                    format!("cannot inspect repository entry: {failure}")
                                })?
                                .is_some()
                            {
                                return Err(discovery_error(start, &error));
                            }
                        }
                        Ok(_) => return Err(discovery_error(start, &error)),
                        Err(failure) => {
                            return Err(format!("cannot inspect repository marker: {failure}"))
                        }
                    }
                }
                if !crate::absence::proven_absent(&ancestor.join("objects"))
                    .map_err(|failure| failure.to_string())?
                    && ["HEAD", "config", "refs"]
                        .iter()
                        .try_fold(false, |found, name| {
                            crate::absence::proven_absent(&ancestor.join(name))
                                .map(|absent| found || !absent)
                        })
                        .map_err(|failure| failure.to_string())?
                {
                    return Err(format!(
                        "Cannot discover existing bare repository: {}.",
                        error.message().trim_end_matches('.')
                    ));
                }
            }
            Ok(None)
        }
        Err(error) => Err(discovery_error(start, &error)),
    }
}

fn discovery_error(start: &Path, error: &git2::Error) -> String {
    let marker = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|marker| std::fs::symlink_metadata(marker).is_ok());
    let Some(marker) = marker else {
        return format!(
            "Cannot discover repository: {}.",
            error.message().trim_end_matches('.')
        );
    };
    let message = format!(
        "Cannot read repository marker {}: {}.",
        crate::git::display_key(&crate::portable_path::slashed(&marker)),
        error.message().trim_end_matches('.')
    );
    let registration = std::fs::read(&marker)
        .ok()
        .and_then(|bytes| {
            let named = crate::git::name::gitfile_dir(&bytes)?;
            let target = super::edit_guard::normalized(&marker.parent()?.join(named), true).ok()?;
            let worktrees = target.parent()?;
            if worktrees.file_name()? != "worktrees" {
                return None;
            }
            let common = worktrees.parent()?;
            let repo = Repository::open(common).ok()?;
            (super::edit_guard::normalized(repo.commondir(), true).ok()? == common).then_some(())
        })
        .is_some();
    if registration {
        registration_error(&message)
    } else {
        message
    }
}

pub(super) fn registration_error(error: &str) -> String {
    format!("{error}; the operator runs `git worktree repair` in their own terminal outside the harness")
}

// Only an owner's enforcement paths require interpreting an unreadable
// marker. A broken outer marker must not lock ordinary nested project files.
fn could_enforce(target: &Path, root: &Path) -> bool {
    [false, true].into_iter().any(|resolve| {
        let (Ok(target), Ok(root)) = (
            super::edit_guard::normalized(target, resolve),
            super::edit_guard::normalized(root, resolve),
        ) else {
            return true;
        };
        let Ok(relative) = target.strip_prefix(&root) else {
            return false;
        };
        let Some(first) = relative.components().next() else {
            return true;
        };
        let first = first.as_os_str();
        // The name test comes first: it reads nothing, and every lookup
        // now asks this for each owner (TSK-1050).
        first.eq_ignore_ascii_case(".git")
            || ([
                "config",
                "config.worktree",
                "hooks",
                "HEAD",
                "packed-refs",
                "refs",
            ]
            .iter()
            .any(|name| first.eq_ignore_ascii_case(name))
                && git_directory(&root).unwrap_or(true))
            || crate::security::actions::table()
                .claude_edit_denies
                .iter()
                .any(|rule| {
                    rule.strip_prefix("Edit(/")
                        .and_then(|text| text.split('/').next())
                        .is_some_and(|name| first.eq_ignore_ascii_case(name.trim_end_matches(')')))
                })
    })
}

fn git_directory(path: &Path) -> Result<bool, String> {
    for name in ["HEAD", "objects", "refs"] {
        if crate::absence::cannot_exist(&path.join(name)).map_err(|error| error.to_string())? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Removal of a container also removes a separate Git directory inside it.
/// Checkout `.git` boundaries are handled by checkout discovery instead.
pub(super) fn contains_git_directory(target: &Path) -> Result<bool, String> {
    if crate::absence::cannot_exist(target).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    if !std::fs::metadata(target)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Ok(false);
    }
    let mut pending = vec![target.to_path_buf()];
    let mut read = 0;
    while let Some(dir) = pending.pop() {
        if git_directory(&dir)? {
            return Ok(true);
        }
        // Registered checkout removals have their own lifecycle refusal.
        if same_checkout_root(&dir) {
            continue;
        }
        read += 1;
        if read > 4096 {
            return Err("cannot inspect contained Git directories beyond 4096 directories; narrow the cleanup target".into());
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name() != ".git" && entry.file_type().map_err(|e| e.to_string())?.is_dir()
            {
                pending.push(entry.path());
            }
        }
    }
    Ok(false)
}

fn same_checkout_root(dir: &Path) -> bool {
    Repository::open(dir)
        .ok()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf))
        .is_some_and(|root| root == dir)
}

// libgit2 guesses the parent of a separate git directory when Git has not
// stored core.worktree. That guess is not a conflicting checkout claim.
fn claims_work_directory(repo: &Repository) -> Result<bool, String> {
    match repo
        .config()
        .and_then(|config| config.get_entry("core.worktree").map(|_| ()))
    {
        Ok(()) => Ok(true),
        Err(error) if error.code() == git2::ErrorCode::NotFound => repo
            .workdir()
            .map(|root| {
                crate::absence::proven_absent(&root.join(".git"))
                    .map(|absent| !absent)
                    .map_err(|error| error.to_string())
            })
            .transpose()
            .map(|present| present.unwrap_or(false)),
        Err(error) => Err(format!(
            "cannot inspect repository work directory: {}",
            error.message()
        )),
    }
}

fn canonical(path: &Path, what: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("cannot resolve {what}: {error}"))
}

type TargetRootsCache = std::collections::HashMap<(PathBuf, PathBuf), Result<Vec<PathBuf>, String>>;

thread_local! {
    pub(super) static TARGET_ROOTS: std::cell::RefCell<Option<TargetRootsCache>> =
        const { std::cell::RefCell::new(None) };
}

/// Every physical repository enclosing a target, including outer repositories
/// hidden by a nested checkout or submodule. Missing suffixes are legitimate
/// create targets; an unreadable marker remains an error at the caller's open.
pub(super) fn target_roots(target: &Path, session: &Path) -> Result<Vec<PathBuf>, String> {
    let key = (target.to_path_buf(), session.to_path_buf());
    if let Some(cached) = TARGET_ROOTS.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|map| map.get(&key))
            .cloned()
    }) {
        return cached;
    }
    let value = read_target_roots(target, session);
    TARGET_ROOTS.with(|cache| {
        if let Some(map) = cache.borrow_mut().as_mut() {
            map.insert(key, value.clone());
        }
    });
    value
}

fn read_target_roots(target: &Path, session: &Path) -> Result<Vec<PathBuf>, String> {
    // Roots are listed, compared and kept in the spelling `normalized` gives
    // the ancestors below: the plain `C:\` form, never the verbatim `\\?\C:\`
    // form `std::fs::canonicalize` returns on Windows. A session spelled the
    // verbatim way was listed beside its own plain ancestor, so one owner was
    // opened twice for every lookup and kept under two keys (TSK-1050).
    let mut roots = vec![crate::portable_path::without_verbatim(
        session.to_path_buf(),
    )];
    for resolve in [false, true] {
        let target =
            super::edit_guard::normalized(target, resolve).map_err(|error| error.to_string())?;
        for ancestor in target.ancestors() {
            let marker = ancestor.join(".git");
            if (!crate::absence::cannot_exist(&marker).map_err(|error| {
                format!(
                    "cannot inspect target repository {}: {error}",
                    marker.display()
                )
            })? || git_directory(ancestor)?)
                && !roots.iter().any(|root| root == ancestor)
            {
                roots.push(ancestor.to_path_buf());
            }
        }
    }
    // A reading kept from earlier in the scope may remove an owner as a
    // duplicate only for a pair two fresh readings proved to share one common
    // directory; any other duplicate a kept reading finds has every owner
    // read again (TSK-1050).
    match distinct_owners(target, &roots, true)? {
        Some(distinct) => Ok(distinct),
        None => Ok(distinct_owners(target, &roots, false)?.unwrap_or_default()),
    }
}

/// The owners in `roots` that `target` must be judged against, one per
/// common directory. With `reuse`, an owner `target` could not enforce
/// through may take the reading kept earlier in this scope. Two owners that
/// share a common directory keep the first; when either reading of that pair
/// was kept, the pair must have been proven to share by two fresh readings
/// earlier in this scope ([`SHARED`]), else the result is `None` and the
/// caller reads every owner afresh. Without `reuse`, every owner is opened
/// now and the result is always `Some`.
fn distinct_owners(
    target: &Path,
    roots: &[PathBuf],
    reuse: bool,
) -> Result<Option<Vec<PathBuf>>, String> {
    // Each common directory, with the first owner that had it and whether
    // that owner's reading was kept.
    let mut common: std::collections::BTreeMap<PathBuf, (PathBuf, bool)> =
        std::collections::BTreeMap::new();
    let mut distinct = Vec::new();
    for root in roots {
        // Main and linked checkouts share the complete protection set. Open
        // each discovered owner first, so deduplication cannot hide a broken
        // pointer or a disagreement with its registration. An owner whose
        // enforcement paths `target` may name is always opened now, so a
        // kept reading never stands in for the refusal a broken owner gives.
        let enforced = could_enforce(target, root);
        let (opened, kept) = owner_common(root, reuse && !enforced);
        let opened = match opened {
            Ok(opened) => opened,
            Err(error) if enforced => {
                if error.starts_with("Cannot read repository marker ") {
                    return Err(error);
                }
                let marker = if git_directory(root).unwrap_or(false) {
                    root.clone()
                } else {
                    root.join(".git")
                };
                return Err(format!(
                    "cannot judge repository marker {}: {error}",
                    crate::git::display_key(&crate::portable_path::slashed(&marker))
                ));
            }
            Err(_) => continue,
        };
        if let Some(directory) = opened {
            let directory = directory?;
            if let Some((first, first_kept)) = common.get(&directory) {
                if !kept && !first_kept {
                    note_shared(first, root);
                } else if !proven_shared(first, root) {
                    return Ok(None);
                }
                continue;
            }
            common.insert(directory, (root.clone(), kept));
        }
        distinct.push(root.clone());
    }
    Ok(Some(distinct))
}

thread_local! {
    /// The pairs of owners two fresh readings proved to share one common
    /// directory in one guard evaluation (TSK-1050), present only while a
    /// [`super::edit_guard::RepoFactsScope`] is open: a linked worktree and
    /// its main checkout. Kept readings may fold such a pair into one owner.
    pub(super) static SHARED: std::cell::RefCell<Option<std::collections::HashSet<(PathBuf, PathBuf)>>> =
        const { std::cell::RefCell::new(None) };
}

fn shared_key(a: &Path, b: &Path) -> (PathBuf, PathBuf) {
    if a <= b {
        (a.to_path_buf(), b.to_path_buf())
    } else {
        (b.to_path_buf(), a.to_path_buf())
    }
}

fn note_shared(a: &Path, b: &Path) {
    SHARED.with(|pairs| {
        if let Some(pairs) = pairs.borrow_mut().as_mut() {
            pairs.insert(shared_key(a, b));
        }
    });
}

fn proven_shared(a: &Path, b: &Path) -> bool {
    SHARED.with(|pairs| {
        pairs
            .borrow()
            .as_ref()
            .is_some_and(|pairs| pairs.contains(&shared_key(a, b)))
    })
}

/// What opening one discovered owner gave: the error [`open`] reported, or
/// for what it opened the canonical common directory (`None` for a proven
/// non-repository, an error when that directory cannot be resolved).
type OwnerCommon = Result<Option<Result<PathBuf, String>>, String>;

/// The metadata of the files an owner's reading starts from: `.git` (a
/// directory, or a file naming a linked worktree's Git directory) and
/// `HEAD` (where the root is itself a Git directory). A reading is kept only
/// while these are unchanged, so an owner replaced, broken or mended during
/// the scope is opened again.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Marker(Vec<Option<MarkerFile>>);

#[derive(Clone, PartialEq, Eq)]
struct MarkerFile {
    dir: bool,
    len: u64,
    modified: Option<std::time::SystemTime>,
    id: u64,
}

fn marker(root: &Path) -> Marker {
    Marker(
        [".git", "HEAD"]
            .iter()
            .map(|name| {
                std::fs::symlink_metadata(root.join(name))
                    .ok()
                    .map(|meta| MarkerFile {
                        dir: meta.is_dir(),
                        len: meta.len(),
                        modified: meta.modified().ok(),
                        id: file_id(&meta),
                    })
            })
            .collect(),
    )
}

#[cfg(unix)]
fn file_id(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn file_id(_meta: &std::fs::Metadata) -> u64 {
    0
}

/// An owner reading kept for one guard evaluation: a proven non-repository
/// or a resolved common directory, with the [`Marker`] it was read from.
type KeptOwner = (Option<PathBuf>, Marker);

thread_local! {
    /// The successful owner readings of one guard evaluation (TSK-1050),
    /// present only while a [`super::edit_guard::RepoFactsScope`] is open. A
    /// failure is never kept.
    pub(super) static OWNERS: std::cell::RefCell<Option<std::collections::HashMap<PathBuf, KeptOwner>>> =
        const { std::cell::RefCell::new(None) };
}

/// The reading of the owner at `root`, and whether it was kept from earlier
/// in this [`super::edit_guard::RepoFactsScope`]. With `reuse`, a kept
/// successful reading answers while the owner's [`Marker`] is unchanged;
/// otherwise the owner is opened now and a successful reading is kept for
/// later lookups. A failure is never kept, so the next lookup opens the
/// owner again.
///
/// Callers never reuse for a target the owner could enforce, and a kept
/// reading drops a duplicate only for a pair proven to share (see
/// [`distinct_owners`]). One evaluation asks for the owners of many ordinary
/// targets in the same repository, so those lookups share one open.
fn owner_common(root: &Path, reuse: bool) -> (OwnerCommon, bool) {
    let now = marker(root);
    if reuse {
        if let Some((kept, _)) = OWNERS.with(|cache| {
            cache
                .borrow()
                .as_ref()
                .and_then(|map| map.get(root))
                .filter(|(_, read_from)| *read_from == now)
                .cloned()
        }) {
            return (Ok(kept.map(Ok)), true);
        }
    }
    #[cfg(test)]
    OWNER_OPENS.with(|opens| opens.set(opens.get() + 1));
    let value = open(root)
        .map(|repo| repo.map(|repo| canonical(repo.commondir(), "target common directory")));
    let success = match &value {
        Ok(None) => Some(None),
        Ok(Some(Ok(directory))) => Some(Some(directory.clone())),
        Ok(Some(Err(_))) | Err(_) => None,
    };
    OWNERS.with(|cache| match (cache.borrow_mut().as_mut(), success) {
        (Some(map), Some(reading)) => {
            map.insert(root.to_path_buf(), (reading, now));
        }
        (Some(map), None) => {
            map.remove(root);
        }
        (None, _) => {}
    });
    (value, false)
}

/// A checkout hidden from Git's registration list still names its common
/// directory in its gitfile. Only inspect configured locations reached by
/// this target, after registered checkouts have already been considered.
pub(super) fn hidden_checkout_under(target: &Path, root: &Path) -> Result<Option<PathBuf>, String> {
    let normalize =
        |path: &Path| super::edit_guard::normalized(path, true).map_err(|e| e.to_string());
    let target = normalize(target)?;
    if crate::absence::cannot_exist(&target).map_err(|e| e.to_string())? {
        return Ok(None);
    }
    if !std::fs::metadata(&target)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Ok(None);
    }
    let Some(repo) = open(root)? else {
        return Ok(None);
    };
    let common = normalize(repo.commondir())?;
    let main = Repository::open(&common).map_err(|e| e.message().to_string())?;
    let Some(main_root) = main.workdir() else {
        return Ok(None);
    };
    let policy = super::landed_policy::load(main_root)?.policy;
    let mut pending = Vec::new();
    for entry in &policy.git.worktree_locations {
        let location = crate::root_checkout::expand_location(
            entry,
            main_root,
            &crate::root_checkout::process_env,
        )
        .ok_or_else(|| format!("cannot resolve configured worktree location `{entry}`"))?;
        let location = normalize(&location)?;
        let start = if target.starts_with(&location) {
            for parent in target
                .ancestors()
                .skip(1)
                .take_while(|parent| parent.starts_with(&location))
            {
                if same_checkout(parent, &common)? {
                    return Ok(None);
                }
            }
            target.clone()
        } else if location.starts_with(&target) {
            location
        } else {
            continue;
        };
        if !crate::absence::cannot_exist(&start).map_err(|e| e.to_string())? {
            pending.push(start);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut directories_read = 0;
    while let Some(dir) = pending.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let marker = dir.join(".git");
        if !crate::absence::cannot_exist(&marker).map_err(|e| e.to_string())?
            && std::fs::symlink_metadata(&marker)
                .map_err(|e| e.to_string())?
                .is_file()
        {
            let bytes = std::fs::read(&marker).map_err(|e| e.to_string())?;
            if let Some(named) = crate::git::name::gitfile_dir(&bytes) {
                if normalize(&dir.join(named))?.starts_with(common.join("worktrees")) {
                    return Ok(Some(dir));
                }
            }
        }
        // Foreign, empty and malformed markers do not establish a boundary
        // for this repository. A hidden checkout can still sit below them.
        if same_checkout(&dir, &common)? {
            continue;
        }
        directories_read += 1;
        if directories_read > 4096 {
            return Err("cannot inspect hidden checkouts beyond 4096 directories; narrow the cleanup target".into());
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            // Git metadata is not another checkout. Directory symlinks are
            // not traversed by recursive removal; a named alias was resolved
            // above before deciding which configured locations it reaches.
            if entry.file_name() != ".git" && entry.file_type().map_err(|e| e.to_string())?.is_dir()
            {
                pending.push(entry.path());
            }
        }
    }
    Ok(None)
}

/// Only a checkout of this common directory bounds its content search.
fn same_checkout(dir: &Path, common: &Path) -> Result<bool, String> {
    let marker = dir.join(".git");
    if crate::absence::cannot_exist(&marker).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let Ok(Some(repo)) = open(dir) else {
        return Ok(false);
    };
    let checkout = repo
        .workdir()
        .map(|root| super::edit_guard::normalized(root, true))
        .transpose()
        .map_err(|error| error.to_string())?;
    Ok(!repo.is_bare()
        && checkout.as_deref() == Some(dir)
        && super::edit_guard::normalized(repo.commondir(), true)
            .map_err(|error| error.to_string())?
            == common)
}

/// Creation must not plant a discovery boundary inside a configured location.
pub(super) fn configured_worktree_target(target: &Path, root: &Path) -> Result<bool, String> {
    let Some(repo) = open(root)? else {
        return Ok(false);
    };
    let common = Repository::open(repo.commondir()).map_err(|e| e.message().to_string())?;
    let Some(main) = common.workdir() else {
        return Ok(false);
    };
    let policy = super::landed_policy::load(main)?.policy;
    for entry in &policy.git.worktree_locations {
        let location =
            crate::root_checkout::expand_location(entry, main, &crate::root_checkout::process_env)
                .ok_or_else(|| format!("cannot resolve configured worktree location `{entry}`"))?;
        for resolve in [false, true] {
            let target =
                super::edit_guard::normalized(target, resolve).map_err(|e| e.to_string())?;
            let location =
                super::edit_guard::normalized(&location, resolve).map_err(|e| e.to_string())?;
            if target.starts_with(location) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Administrative directories belong to the common git directory that
/// physically contains `worktrees/<name>`, regardless of commondir's text.
fn check_common_directory(repo: &Repository) -> Result<(), String> {
    let admin = canonical(repo.path(), "worktree administrative directory")?;
    let registrations = admin
        .parent()
        .ok_or("worktree has no registration directory")?;
    let expected = registrations
        .parent()
        .ok_or("worktree registration has no common directory")?;
    if canonical(
        &expected.join("worktrees"),
        "worktree registration directory",
    )? != registrations
        || canonical(repo.commondir(), "worktree common directory")? != expected
    {
        return Err("worktree common directory disagrees with its registration directory".into());
    }
    Ok(())
}

// Inventory the administrative names before interpreting their pointers. A
// relative missing leaf is prunable too; the generic worktree reader requires
// a checkout parent in the raw text before it can represent that checkout.
pub(super) fn registrations(repo: &Repository) -> Result<Vec<PathBuf>, String> {
    let names = repo
        .worktrees()
        .map_err(|error| error.message().to_string())?;
    crate::git::name::names_of(&names)
        .into_iter()
        .map(|name| {
            name.os_path()
                .map(|folder| repo.commondir().join("worktrees").join(folder))
                .map_err(|error| error.to_string())
        })
        .collect()
}

/// The normalized marker named by a regular registration file.
pub(super) fn registered_marker(admin: &Path) -> Result<PathBuf, String> {
    let file = admin.join("gitdir");
    let metadata = std::fs::symlink_metadata(&file)
        .map_err(|error| format!("cannot inspect worktree registration: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("worktree registration must be a regular file".into());
    }
    let text = std::fs::read(&file)
        .map_err(|error| format!("cannot read worktree registration: {error}"))?;
    let text = text.trim_ascii_end();
    let root = text
        .strip_suffix(b"/.git")
        .or_else(|| {
            cfg!(windows)
                .then(|| text.strip_suffix(b"\\.git"))
                .flatten()
        })
        .filter(|root| !root.is_empty())
        .ok_or("worktree registration must end with /.git")?;
    let root = GitName::from_bytes(root)
        .os_path()
        .map_err(|error| error.to_string())?;
    let root = crate::portable_path::without_verbatim(admin.to_path_buf())
        .join(crate::portable_path::without_verbatim(root));
    // All absence proofs use this same normalized reading. A missing
    // component followed by .. must not certify an existing checkout absent.
    let root = super::edit_guard::normalized(&root, true).map_err(|error| error.to_string())?;
    Ok(root.join(".git"))
}

#[cfg(test)]
thread_local! {
    static VALIDATION_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times [`owner_common`] opened a discovered owner.
    static OWNER_OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Both pointers must agree: the registration names a checkout containing
/// a .git marker that opens this administrative directory, with its original
/// common directory. Resolving only the registration's parent would let a
/// symlink to a marker in another directory hide the actual checkout.
///
/// Absent targets are Git's prunable state, not live checkout roots. An
/// unreadable target or an existing target with conflicting identity refuses.
pub(super) fn checked_registration(admin: &Path) -> Result<Option<PathBuf>, String> {
    super::edit_guard::registration_at(admin)
        .map(|registration| registration.checkout)
        .map_err(|error| registration_error(&error))
}

#[derive(Clone)]
pub(super) struct CheckedRegistration {
    pub marker: PathBuf,
    pub checkout: Option<PathBuf>,
}

pub(super) fn read_registration(admin: &Path) -> Result<CheckedRegistration, String> {
    #[cfg(test)]
    VALIDATION_READS.with(|reads| reads.set(reads.get() + 1));
    let registered = registered_marker(admin)?;
    if crate::absence::proven_absent(&registered)
        .map_err(|error| format!("cannot inspect registered target: {error}"))?
    {
        return Ok(CheckedRegistration {
            marker: registered,
            checkout: None,
        });
    }
    let root = canonical(
        registered
            .parent()
            .ok_or("worktree registration has no checkout parent")?,
        "worktree registration",
    )?;
    let marker = root.join(".git");
    let registered_marker = canonical(&registered, "registered checkout marker")?;
    if registered_marker != canonical(&marker, "checkout marker")? {
        return Err("worktree registration names a different checkout marker".into());
    }
    let metadata = std::fs::metadata(&marker)
        .map_err(|error| format!("cannot inspect registered checkout marker: {error}"))?;
    // An alias named .git must not move the directory containing a gitfile.
    // Directory markers may legitimately link straight to the admin directory;
    // their identity is checked by opening that directory below.
    if metadata.is_file() && registered_marker.parent() != Some(root.as_path()) {
        return Err("worktree registration disagrees with its marker's directory".into());
    }
    let checkout = open_checkout(&root)
        .map_err(|error| format!("cannot open registered checkout: {error}"))?;
    if !checkout.is_worktree()
        || canonical(checkout.path(), "registered checkout git directory")?
            != canonical(admin, "worktree administrative directory")?
    {
        return Err("worktree registration disagrees with its checkout marker".into());
    }
    check_common_directory(&checkout)?;
    Ok(CheckedRegistration {
        marker: registered,
        checkout: Some(root),
    })
}

// A directory marker can be a symlink to either ordinary or linked
// metadata. Resolve it before asking libgit2 to open it, and use plain native
// paths at that boundary: verbatim Windows spellings do not reliably follow
// directory markers in libgit2. Gitfiles still resolve from their checkout.
fn open_checkout(root: &Path) -> Result<Repository, String> {
    let marker = root.join(".git");
    let metadata = std::fs::metadata(&marker)
        .map_err(|error| format!("cannot inspect checkout marker: {error}"))?;
    let directory = if metadata.is_dir() {
        canonical(&marker, "checkout metadata directory")?
    } else {
        root.to_path_buf()
    };
    Repository::open(crate::portable_path::without_verbatim(directory))
        .map_err(|error| discovery_error(root, &error))
}

/// libgit2 may ignore a broken inner marker and successfully discover an outer
/// repository. Each marker below the reported boundary must resolve to that
/// repository, identify a nearer linked checkout, or be empty. `open` then
/// validates the selected checkout's registration and common directory.
fn check_discovery_boundaries(
    start: &Path,
    repo: Repository,
) -> Result<(PathBuf, Repository), String> {
    let start = start
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository search directory: {error}"))?;
    let boundary = (!repo.is_worktree())
        .then(|| {
            repo.workdir()
                .unwrap_or_else(|| repo.path())
                .canonicalize()
                .map_err(|error| format!("cannot resolve discovered repository directory: {error}"))
        })
        .transpose()?;
    let git_boundary = repo
        .path()
        .canonicalize()
        .map_err(|error| format!("cannot resolve discovered git directory: {error}"))?;
    for ancestor in start.ancestors() {
        if boundary.as_deref() == Some(ancestor)
            || (!repo.is_worktree() && ancestor == git_boundary)
        {
            return Ok((ancestor.to_path_buf(), repo));
        }
        let marker = ancestor.join(".git");
        if crate::absence::proven_absent(&marker)
            .map_err(|error| format!("cannot inspect repository marker: {error}"))?
        {
            continue;
        }
        let open_error = match open_checkout(ancestor) {
            Ok(local) => {
                let local_directory = local.path().canonicalize().map_err(|error| {
                    format!(
                        "cannot resolve repository marker {}: {error}",
                        marker.display()
                    )
                })?;
                if local_directory == git_boundary {
                    return Ok((ancestor.to_path_buf(), repo));
                }
                // Windows discovery can skip a directory symlink to linked
                // metadata. Select that nearer checkout and let `open` check
                // both pointers, rather than treating the outer result as final.
                if local.is_worktree() {
                    return Ok((ancestor.to_path_buf(), local));
                }
                return Err(format!(
                    "repository discovery skipped another repository at {}",
                    marker.display()
                ));
            }
            Err(error) => error,
        };
        let mut entries = std::fs::read_dir(&marker).map_err(|error| {
            format!(
                "cannot open repository marker {}: {open_error}; cannot inspect it: {error}",
                marker.display()
            )
        })?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("cannot inspect repository entry: {error}"))?
            .is_some()
        {
            return Err(format!(
                "cannot open repository marker {}: {open_error}",
                marker.display()
            ));
        }
    }
    Err("discovered repository is outside the searched ancestry".to_string())
}

// Recovery admits only literal words. Validate the small shell subset before
// using the shared quote-removing tokenizer: operators and expansions outside
// single quotes cannot acquire permission by decoding to a registered path.
pub(super) fn literal_recovery_command(command: &str) -> bool {
    let mut quote = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\0')
            | (
                None,
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '{' | '}' | '*' | '?' | '[' | ']' | '~'
                | '#' | '^' | '!' | '\n' | '\r',
            ) => return false,
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (_, '\\') => {
                let Some(next) = chars.next() else {
                    return false;
                };
                if next == '\0' || next == '\n' {
                    return false;
                }
                // Quote removal belongs to shell_tokens. An escaped character
                // cannot introduce an operator, expansion or closing quote.
            }
            (_, '$' | '`') => return false,
            _ => {}
        }
    }
    quote.is_none()
}

/// The bounded recovery available even when checkout identity cannot be read.
/// Only one literal Git command qualifies; shell operators, substitutions,
/// environment changes and Git global options never inherit this exception.
#[must_use]
pub fn worktree_recovery(command: &str, cwd: &Path) -> bool {
    if !literal_recovery_command(command) {
        return false;
    }
    let words = super::git_guard::shell_tokens(command);
    let args: Vec<_> = words.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["git", "worktree", "prune" | "list"] => true,
        ["git", "worktree", "remove", "--force", target] => {
            open(cwd).ok().flatten().is_some_and(|repo| {
                absent_registered_checkout(&repo, Path::new(target)).unwrap_or(false)
            })
        }
        _ => false,
    }
}

/// The precise absent path Git records, never its relative suffix lookup.
/// Used by recovery and by the normal discard check after policy loading.
pub(super) fn absent_registered_checkout(repo: &Repository, target: &Path) -> Result<bool, String> {
    let target = crate::portable_path::without_verbatim(target.to_path_buf());
    if !target.is_absolute() {
        return Ok(false);
    }
    let prefix = match target.components().next() {
        Some(std::path::Component::Prefix(prefix)) => prefix.as_os_str().as_encoded_bytes().len(),
        _ => 0,
    };
    let bytes = &target.as_os_str().as_encoded_bytes()[prefix..];
    let separator = |byte: &u8| *byte == b'/' || (cfg!(windows) && *byte == b'\\');
    // A Windows UNC prefix can itself be an absolute filesystem root.
    let Some(tail) = bytes.get(1..) else {
        return Ok(false);
    };
    if tail
        .split(separator)
        .any(|part| part == b"." || part == b"..")
        || bytes
            .windows(2)
            .any(|pair| separator(&pair[0]) && separator(&pair[1]))
    {
        return Ok(false);
    }
    let target = super::edit_guard::normalized(&target, true).map_err(|error| error.to_string())?;
    if !crate::absence::proven_absent(&target).map_err(|error| error.to_string())? {
        return Ok(false);
    }
    for admin in registrations(repo)? {
        let marker = registered_marker(&admin)?;
        let root = marker
            .parent()
            .ok_or("registration has no checkout parent")?;
        if root == target
            && crate::absence::proven_absent(root).map_err(|error| error.to_string())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    /// TSK-381 SF-3: the probe reports the filesystem the git directory is
    /// on, and one that cannot decide counts as case-insensitive.
    /// TSK-381 SF-4: a ref is read only under the spelling git lists, and
    /// not at all when another listed spelling folds to it.
    #[test]
    fn a_ref_is_read_only_under_its_listed_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let tree = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        let commit = repo.commit(None, &sig, &sig, "one", &tree, &[]).unwrap();
        let remotes = dir.path().join(".git/refs/remotes/origin");
        std::fs::create_dir_all(&remotes).unwrap();
        std::fs::write(remotes.join("MAIN"), format!("{commit}\n")).unwrap();
        let listed = |name: &str| super::listed_ref_commit(&repo, name).unwrap();
        assert_eq!(listed("refs/remotes/origin/MAIN"), Some(commit));
        assert_eq!(listed("refs/remotes/origin/main"), None);
        if !dir.path().join(".git/head").exists() {
            // A case-sensitive filesystem holds both spellings: neither counts.
            std::fs::write(remotes.join("main"), format!("{commit}\n")).unwrap();
            assert_eq!(listed("refs/remotes/origin/main"), None);
            assert_eq!(listed("refs/remotes/origin/MAIN"), None);
        }
    }

    /// TSK-381 SF-7: a tracking ref stored under `INTEGRATION/` is judged by
    /// that stored spelling, not as `integration/` because the listing asked
    /// for that directory. SF-8: NFD and NFC spellings fold alike.
    #[test]
    fn a_ref_under_a_differently_spelled_directory_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let tree = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        let commit = repo.commit(None, &sig, &sig, "one", &tree, &[]).unwrap();
        let stored = dir.path().join(".git/refs/remotes/origin/INTEGRATION");
        std::fs::create_dir_all(&stored).unwrap();
        std::fs::write(stored.join("producer-line"), format!("{commit}\n")).unwrap();
        let listed = |name: &str| super::listed_ref_commit(&repo, name).unwrap();
        assert_eq!(
            listed("refs/remotes/origin/integration/producer-line"),
            None
        );
        assert_eq!(
            listed("refs/remotes/origin/INTEGRATION/producer-line"),
            Some(commit)
        );
        assert_eq!(
            super::folded_ref_name("integration/cafe\u{301}"),
            super::folded_ref_name("integration/caf\u{e9}")
        );
    }

    #[test]
    fn the_case_probe_reads_the_git_directory_and_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let insensitive = dir.path().join("head").exists();
        assert_eq!(super::case_insensitive_git_dir(dir.path()), insensitive);
        if !insensitive {
            std::fs::write(dir.path().join("head"), "other\n").unwrap();
            assert!(!super::case_insensitive_git_dir(dir.path()));
        }
        let empty = tempfile::tempdir().unwrap();
        assert!(super::case_insensitive_git_dir(empty.path()));
    }

    #[cfg(unix)]
    #[test]
    fn r22_broken_nested_marker_cannot_select_outer_repository() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::os::unix::fs::symlink(nested.join("missing"), nested.join(".git")).unwrap();
        assert!(super::open(&nested).is_err());
        std::fs::remove_file(nested.join(".git")).unwrap();
        std::fs::write(nested.join(".git"), "gitdir: missing\n").unwrap();
        assert!(super::open(&nested).is_err());
    }

    #[test]
    fn r22_absent_and_empty_nested_markers_keep_parent_discovery() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        for empty_marker in [false, true] {
            if empty_marker {
                std::fs::create_dir(nested.join(".git")).unwrap();
            }
            let repo = super::open(&nested).unwrap().unwrap();
            assert_eq!(
                repo.workdir().unwrap().canonicalize().unwrap(),
                dir.path().canonicalize().unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn r22_nested_bare_repository_stops_before_broken_outer_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join(".git")).unwrap();
        let nested = dir.path().join("nested.git");
        Repository::init_bare(&nested).unwrap();
        let repo = super::open(&nested).unwrap().unwrap();
        assert!(repo.is_bare());
        assert_eq!(
            repo.path().canonicalize().unwrap(),
            nested.canonicalize().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn r22_repository_discovery_from_symlinked_subdirectory_keeps_real_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        Repository::init(&root).unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&nested, &link).unwrap();
        let repo = super::open(&link).unwrap().unwrap();
        assert_eq!(
            repo.workdir().unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn r22_repository_discovery_accepts_separate_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("checkout");
        let metadata = dir.path().join("metadata");
        git(
            dir.path(),
            &[
                "init",
                "--separate-git-dir",
                metadata.to_str().unwrap(),
                root.to_str().unwrap(),
            ],
        );
        for start in [&root, &metadata, &metadata.join("objects")] {
            let repo = super::open(start).unwrap().unwrap();
            assert_eq!(
                repo.path().canonicalize().unwrap(),
                metadata.canonicalize().unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn r18_repository_beneath_dangling_ancestor_is_not_absent() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("broken");
        std::os::unix::fs::symlink(dir.path().join("missing"), &link).unwrap();
        assert!(super::open(&link.join("child")).is_err());
    }

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

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "chore: init"]);
    }

    #[test]
    fn registrations_are_validated_once_per_guard_invocation() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        for name in ["one", "two"] {
            git(dir.path(), &["worktree", "add", "-b", name, name]);
        }
        for _ in 0..2 {
            let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
            super::VALIDATION_READS.with(|reads| reads.set(0));
            for cwd in [dir.path().to_path_buf(), dir.path().join("one")] {
                super::RepoInfo::discover(&cwd).unwrap().unwrap();
                let repo = super::open(&cwd).unwrap().unwrap();
                crate::hooks::edit_guard::checkout_roots(&repo).unwrap();
                for target in ["build", "out"] {
                    crate::hooks::edit_guard::repository_enforcement_target(
                        &cwd.join(target),
                        &cwd,
                        true,
                    )
                    .unwrap();
                    crate::hooks::edit_guard::holds_enforcement_files(&cwd.join(target), &cwd)
                        .unwrap();
                }
            }
            assert_eq!(super::VALIDATION_READS.with(std::cell::Cell::get), 2);
        }
    }

    #[test]
    fn discovery_keeps_the_nearest_registered_checkout() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let linked = dir.path().join("linked");
        git(
            dir.path(),
            &["worktree", "add", "-b", "task/linked", "linked"],
        );
        // libgit2 can discover the outer repository when a Windows directory
        // symlink names linked metadata. The boundary walk must retain the
        // nearer checkout; registration validation still belongs to `open`.
        let outer = Repository::open(dir.path()).unwrap();
        let (root, repository) = super::check_discovery_boundaries(&linked, outer).unwrap();
        assert_eq!(root, linked.canonicalize().unwrap());
        assert!(repository.is_worktree());
        assert_eq!(
            super::checked_registration(repository.path()).unwrap(),
            Some(root)
        );
    }

    #[test]
    fn test_discover_none_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(RepoInfo::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_discover_basic_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let info = RepoInfo::discover(dir.path()).unwrap().unwrap();
        assert_eq!(info.branch, "main");
        assert!(!info.is_worktree);
        assert!(info.state_dir().ends_with("codeflow"));
        assert!(info.common_dir.exists());
    }

    #[test]
    fn test_discover_from_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let sub = dir.path().join("src/deep");
        std::fs::create_dir_all(&sub).unwrap();
        let info = RepoInfo::discover(&sub).unwrap().unwrap();
        assert_eq!(
            info.root.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_discover_unborn_head_reports_branch() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let info = RepoInfo::discover(dir.path()).unwrap().unwrap();
        assert_eq!(info.branch, "main");
    }

    /// Review finding on issue 79: a branch whose name is not valid UTF-8 read
    /// as detached, so a protected pattern never saw it and the refusal was
    /// dropped. The hook plane now reads it as one sentinel that every branch
    /// rule treats as protected, and the exact name is kept for display.
    /// Unix only: Windows cannot hold the name as a ref path, so libgit2
    /// cannot read the branch there at all.
    #[cfg(unix)]
    #[test]
    fn a_branch_that_is_not_utf8_is_not_detached_and_stays_protected() {
        let dir = tempfile::tempdir().unwrap();
        let git_repo = Repository::init(dir.path()).unwrap();
        let tree = git_repo
            .find_tree(git_repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let commit = git_repo
            .commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
            .unwrap();
        drop(tree);
        let git_dir = dir.path().join(".git");
        let mut packed = b"# pack-refs with: peeled fully-peeled sorted \n".to_vec();
        packed.extend_from_slice(format!("{commit} refs/heads/release/").as_bytes());
        packed.extend_from_slice(b"caf\xe9\n");
        std::fs::write(git_dir.join("packed-refs"), packed).unwrap();
        std::fs::write(git_dir.join("HEAD"), b"ref: refs/heads/release/caf\xe9\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let name = current_branch(&repo).unwrap();
        assert_eq!(name, NON_UTF8_BRANCH);
        let policy = crate::hooks::policy::GitPolicy {
            protected_branches: vec!["main".to_string()],
            ..crate::hooks::policy::GitPolicy::default()
        };
        assert!(policy.branch_is_protected(&name));
        assert_eq!(
            current_branch_name(&repo)
                .unwrap()
                .unwrap()
                .display()
                .to_string(),
            "release/caf\\xe9"
        );
        // The unborn case reads the symbolic target the same way.
        std::fs::write(git_dir.join("packed-refs"), b"").unwrap();
        assert_eq!(
            current_branch(&Repository::open(dir.path()).unwrap()).unwrap(),
            name
        );
    }

    #[test]
    fn test_worktree_shares_common_dir() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        init_repo(&main);
        let wt = dir.path().join("wt");
        git(
            &main,
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat/x"],
        );
        let info = RepoInfo::discover(&wt).unwrap().unwrap();
        assert_eq!(info.branch, "feat/x");
        assert!(info.is_worktree);
        let main_info = RepoInfo::discover(&main).unwrap().unwrap();
        // State dirs resolve to the same place: shared across worktrees.
        assert_eq!(
            info.state_dir().parent().unwrap().canonicalize().unwrap(),
            main_info
                .state_dir()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap(),
        );
        // A canonical native registration uses backslashes and a verbatim
        // prefix on Windows. Discovery must preserve the checkout identity
        // without rewriting either pointer file.
        let native = wt.canonicalize().unwrap();
        let admin = Repository::open(&wt).unwrap().path().to_path_buf();
        let pointer = std::fs::read(native.join(".git")).unwrap();
        std::fs::create_dir(native.join("src")).unwrap();
        for registration in [
            format!("{}\n", native.join(".git").display()),
            "../../../../wt/.git\n".to_string(),
        ] {
            std::fs::write(admin.join("gitdir"), &registration).unwrap();
            for start in [&native, &native.join("src")] {
                let discovered = RepoInfo::discover(start).unwrap().unwrap();
                assert_eq!(discovered.root.canonicalize().unwrap(), native);
                assert!(discovered.is_worktree);
                assert!(crate::hooks::git_discard::inspect(
                    start,
                    None,
                    &crate::hooks::git_discard::Intent::HardReset,
                )
                .unwrap()
                .is_none());
                assert_eq!(
                    crate::hooks::edit_guard::checkout_root_of(start)
                        .unwrap()
                        .canonicalize()
                        .unwrap(),
                    native
                );
            }
            assert_eq!(
                std::fs::read_to_string(admin.join("gitdir")).unwrap(),
                registration
            );
            assert_eq!(std::fs::read(native.join(".git")).unwrap(), pointer);
        }
        std::fs::write(admin.join("gitdir"), "\n").unwrap();
        assert!(crate::git::gitdir_checkout(&admin).is_err());
    }

    #[test]
    fn worktree_registration_cannot_move_the_discovered_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        init_repo(&main);
        let wt = dir.path().join("wt");
        git(
            &main,
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat/x"],
        );
        let admin = Repository::open(&wt).unwrap().path().to_path_buf();
        let other = dir.path().join("other");
        Repository::init(&other).unwrap();
        for registration in [
            format!("{}\n", other.join(".git").display()),
            "../../../../other/.git\n".to_string(),
        ] {
            std::fs::write(admin.join("gitdir"), registration).unwrap();
            assert!(
                super::open(&wt).is_err(),
                "registration selected another checkout"
            );
            assert!(crate::hooks::git_discard::inspect(
                &wt,
                None,
                &crate::hooks::git_discard::Intent::HardReset,
            )
            .is_err());
        }
        // A claimed root inside the real checkout can contain the caller.
        // A raw discovery then inspects the wrong tree instead of failing
        // its outside-worktree check. Discard must use the same cross-check.
        let nested = wt.join("src");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", nested.join(".git").display()),
        )
        .unwrap();
        assert!(crate::hooks::git_discard::inspect(
            &nested,
            None,
            &crate::hooks::git_discard::Intent::HardReset,
        )
        .is_err());
    }
}

#[cfg(test)]
mod r16_obtaining_regressions {
    #[test]
    fn r17_empty_git_directory_is_not_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(super::open(dir.path()).unwrap().is_none());
        assert!(super::RepoInfo::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn r17_bare_repository_without_head_is_not_absent() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        std::fs::remove_file(dir.path().join("HEAD")).unwrap();
        assert!(super::open(dir.path()).is_err());
    }

    #[test]
    fn r16_invalid_existing_head_is_not_detached_or_nonrepo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), b"invalid HEAD\n").unwrap();
        assert!(super::current_branch_name(&repo).is_err());
        assert!(super::current_branch(&repo).is_err());
        assert!(super::RepoInfo::discover(dir.path()).is_err());
        // libgit2 may still open a repository handle with malformed HEAD.
        // It must never become absent; the head consumer must still refuse.
        match super::open(dir.path()) {
            Ok(Some(reopened)) => assert!(super::current_branch_name(&reopened).is_err()),
            Err(_) => {}
            Ok(None) => panic!("an existing repository was discarded as absent"),
        }
        std::fs::remove_file(dir.path().join(".git/HEAD")).unwrap();
        assert!(super::open(dir.path()).is_err());
    }
}

/// TSK-1050: one guard evaluation opens each repository once for ordinary
/// targets, and a kept reading never turns a refusal into an allow.
#[cfg(test)]
mod owner_opens {
    use super::OWNER_OPENS;
    use crate::hooks::edit_guard::RepoFactsScope;
    use std::path::{Path, PathBuf};

    type Roots = Result<Vec<PathBuf>, String>;

    fn opens() -> usize {
        OWNER_OPENS.with(std::cell::Cell::get)
    }

    fn reset() {
        OWNER_OPENS.with(|count| count.set(0));
    }

    /// A repository with one commit at the physical path of a new
    /// directory, spelled as git writes it (no Windows verbatim prefix), so
    /// every spelling of a target names one owner.
    fn repository() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(dir.path()).unwrap();
        let repo = git2::Repository::init(&root).unwrap();
        let tree = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "one", &tree, &[])
            .unwrap();
        (dir, root)
    }

    fn roots(root: &Path, word: &str) -> Roots {
        super::target_roots(&root.join(word), root)
    }

    /// The owners a target gets with no scope, so nothing is kept.
    fn fresh(root: &Path, word: &str) -> Roots {
        roots(root, word)
    }

    fn break_head(root: &Path) -> Vec<u8> {
        let head = root.join(".git/HEAD");
        let bytes = std::fs::read(&head).unwrap();
        std::fs::remove_file(&head).unwrap();
        bytes
    }

    fn mend_head(root: &Path, bytes: &[u8]) {
        std::fs::write(root.join(".git/HEAD"), bytes).unwrap();
    }

    /// Ordinary targets in one repository share one open per scope, a new
    /// scope opens again, and no scope keeps nothing.
    #[test]
    fn each_owner_is_opened_once_per_guard_evaluation() {
        let (_dir, root) = repository();
        for _ in 0..2 {
            let _scope = RepoFactsScope::enter();
            reset();
            assert_eq!(roots(&root, "d0/x"), Ok(vec![root.clone()]));
            assert_eq!(opens(), 1, "the first lookup in a scope opens its owner");
            for word in 1..16 {
                assert_eq!(roots(&root, &format!("d{word}/x")), Ok(vec![root.clone()]));
            }
            assert_eq!(opens(), 1, "later ordinary targets reuse the opened owner");
        }
        reset();
        assert!(roots(&root, "d0/x").is_ok());
        assert!(roots(&root, "d0/x").is_ok());
        assert_eq!(opens(), 2, "outside a scope every lookup opens again");
    }

    /// A session root in the verbatim spelling Windows `canonicalize` gives
    /// (`\\?\C:\`) and a target's plain ancestors name one owner, which is
    /// listed, opened and kept once, in the plain spelling. On other
    /// platforms both spellings are the same path, so only the hosted
    /// Windows lane exercises two of them.
    #[test]
    fn a_session_in_another_spelling_names_one_owner() {
        let (_dir, root) = repository();
        let verbatim = root.canonicalize().unwrap();
        let _scope = RepoFactsScope::enter();
        reset();
        for word in ["a/x", ".claude/settings.json", "b/y"] {
            assert_eq!(
                super::target_roots(&root.join(word), &verbatim),
                Ok(vec![root.clone()]),
                "{word}"
            );
        }
        assert_eq!(
            opens(),
            2,
            "one open for the ordinary targets, one for the protected target"
        );
    }

    /// A linked worktree and its main checkout share one common directory.
    /// Once a lookup has read both and found them sharing, later ordinary
    /// targets reuse both readings, so 16 targets in the worktree open the
    /// two owners once each, whether the session is the main checkout or
    /// the worktree (Codex review 02 of PR 386).
    #[test]
    fn a_linked_worktree_and_its_main_checkout_are_opened_once() {
        let (_dir, root) = repository();
        let repo = git2::Repository::open(&root).unwrap();
        let linked = root.join("w");
        repo.worktree("w", &linked, None).unwrap();
        for session in [root.clone(), linked.clone()] {
            let _scope = RepoFactsScope::enter();
            reset();
            for word in 0..16 {
                let target = linked.join(format!("d{word}/x"));
                assert_eq!(
                    super::target_roots(&target, &session),
                    Ok(vec![session.clone()]),
                    "{word}"
                );
            }
            assert_eq!(opens(), 2, "each owner is opened once: {session:?}");
        }
    }

    /// A duplicate that a kept reading finds, for a pair not yet proven to
    /// share, has every owner read again, so the pair is folded only on
    /// fresh readings; once proven, kept readings fold it.
    #[test]
    fn an_unproven_pair_is_read_again_before_it_is_folded() {
        let (_dir, root) = repository();
        let repo = git2::Repository::open(&root).unwrap();
        let linked = root.join("w");
        repo.worktree("w", &linked, None).unwrap();
        let _scope = RepoFactsScope::enter();
        reset();
        let main = super::target_roots(&root.join("a/x"), &root);
        assert_eq!(main, Ok(vec![root.clone()]));
        assert_eq!(opens(), 1, "the main checkout alone");
        let first = super::target_roots(&linked.join("b/x"), &root);
        assert_eq!(first, Ok(vec![root.clone()]));
        assert_eq!(opens(), 4, "the worktree, then both owners afresh");
        let later = super::target_roots(&linked.join("c/x"), &root);
        assert_eq!(later, Ok(vec![root.clone()]));
        assert_eq!(opens(), 4, "a proven pair folds on kept readings");
    }

    /// Two owners, an outer repository and an independent one nested in
    /// it, keep their own readings: each target gets exactly its owners,
    /// in any order, and each owner is opened once.
    #[test]
    fn two_owners_keep_their_own_roots() {
        let (_dir, root) = repository();
        let nested = root.join("nested");
        git2::Repository::init(&nested).unwrap();
        let both = vec![root.clone(), nested.clone()];
        let _scope = RepoFactsScope::enter();
        reset();
        for word in 0..6 {
            let (outer, inner) = (format!("o{word}/x"), format!("nested/i{word}/x"));
            let pairs = if word % 2 == 0 {
                [(outer, vec![root.clone()]), (inner, both.clone())]
            } else {
                [(inner, both.clone()), (outer, vec![root.clone()])]
            };
            for (target, owners) in pairs {
                assert_eq!(roots(&root, &target), Ok(owners), "{target}");
            }
        }
        assert_eq!(opens(), 2, "each owner is opened once");
    }

    /// An owner that breaks during the scope refuses an unseen protected
    /// target as it does with no scope, whichever kind of target opened it
    /// first (Codex review 01 of PR 386).
    #[test]
    fn a_protected_target_sees_an_owner_that_breaks_mid_scope() {
        for ordinary_first in [true, false] {
            let (_dir, root) = repository();
            let scope = RepoFactsScope::enter();
            let first = if ordinary_first {
                "ordinary.txt"
            } else {
                ".claude/a.json"
            };
            assert_eq!(roots(&root, first), Ok(vec![root.clone()]));
            let head = break_head(&root);
            let after = roots(&root, ".claude/notes.txt");
            let ordinary = roots(&root, "plain.txt");
            drop(scope);
            let expected = fresh(&root, ".claude/notes.txt");
            assert!(expected.is_err(), "{expected:?}");
            assert_eq!(after, expected, "ordinary first: {ordinary_first}");
            // An ordinary target may keep an owner the fresh reading skips;
            // it never loses one the fresh reading keeps.
            let (Ok(kept), Ok(fresh_owners)) = (ordinary, fresh(&root, "plain.txt")) else {
                panic!("an ordinary target never fails on a broken owner");
            };
            assert!(fresh_owners.iter().all(|owner| kept.contains(owner)));
            mend_head(&root, &head);
        }
    }

    /// An owner mended during the scope is opened again, so an unseen
    /// target of either kind gets the owner a fresh reading gives.
    #[test]
    fn a_mended_owner_is_read_again_in_the_same_scope() {
        for ordinary_first in [true, false] {
            let (_dir, root) = repository();
            let head = break_head(&root);
            let _scope = RepoFactsScope::enter();
            if ordinary_first {
                assert_eq!(roots(&root, "src/a.rs"), Ok(Vec::new()));
            } else {
                assert!(roots(&root, ".claude/a.json").is_err());
            }
            mend_head(&root, &head);
            for word in ["src/b.rs", ".claude/b.json"] {
                assert_eq!(roots(&root, word), Ok(vec![root.clone()]), "{word}");
            }
        }
    }

    /// A broken owner fails every protected target and skips every
    /// ordinary one, exactly as with no scope. Each unseen target opens it
    /// again: a failure is never kept.
    #[test]
    fn a_broken_owner_fails_closed_for_every_protected_target_in_a_scope() {
        let (_dir, root) = repository();
        break_head(&root);
        let passes = [
            [
                ".claude/settings.json",
                "src/a.rs",
                ".git/config",
                "notes.txt",
            ],
            [
                "src/b.rs",
                ".claude/hooks/x",
                "docs/c.md",
                ".git/hooks/pre-push",
            ],
        ];
        let expected: Vec<Vec<Roots>> = passes
            .iter()
            .map(|pass| pass.iter().map(|word| fresh(&root, word)).collect())
            .collect();
        let _scope = RepoFactsScope::enter();
        reset();
        for (pass, results) in passes.iter().zip(&expected) {
            for (word, result) in pass.iter().zip(results) {
                assert_eq!(result.is_err(), word.starts_with('.'), "{word}: {result:?}");
                assert_eq!(&roots(&root, word), result, "{word}");
            }
        }
        assert_eq!(
            opens(),
            8,
            "every unseen target opens the broken owner again"
        );
    }

    /// A kept reading never removes an owner as a duplicate. A linked
    /// worktree shares its main checkout's common directory, so it is
    /// folded into it; once the worktree is replaced by an independent
    /// repository, an unseen target inside it gets both owners, as a fresh
    /// reading does.
    #[test]
    fn a_kept_reading_never_removes_an_owner() {
        let (_dir, root) = repository();
        let repo = git2::Repository::open(&root).unwrap();
        let linked = root.join("w");
        repo.worktree("w", &linked, None).unwrap();
        let scope = RepoFactsScope::enter();
        assert_eq!(roots(&root, "w/a.txt"), Ok(vec![root.clone()]));
        std::fs::remove_file(linked.join(".git")).unwrap();
        git2::Repository::init(&linked).unwrap();
        let after = roots(&root, "w/b.txt");
        drop(scope);
        assert_eq!(
            fresh(&root, "w/b.txt"),
            Ok(vec![root.clone(), linked.clone()])
        );
        assert_eq!(after, Ok(vec![root, linked]));
    }
}
