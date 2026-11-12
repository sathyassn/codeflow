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

/// Refuse a repository whose `config` or `config.worktree`, in the git
/// directory or the common directory it names, exists as anything but a
/// regular file of bounded size. libgit2 reads those files with a blocking
/// read, so a fifo would hold the caller with no answer. A file a config
/// `include` names is read by libgit2 and is not checked here.
fn config_files_readable(git_dir: &Path) -> Result<(), String> {
    const LIMIT: u64 = super::policy::SETTINGS_READ_LIMIT;
    let mut dirs = vec![git_dir.to_path_buf()];
    let common = git_dir.join("commondir");
    if !crate::absence::proven_absent(&common)
        .map_err(|error| format!("cannot inspect {}: {error}", common.display()))?
    {
        let bytes = crate::bounded_file::read_bounded_following(&common, 4096)
            .map_err(|error| format!("cannot read {}: {error}", common.display()))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("cannot read {}: not UTF-8", common.display()))?;
        let line = text.strip_suffix('\n').unwrap_or(&text);
        dirs.push(git_dir.join(line.strip_suffix('\r').unwrap_or(line)));
    }
    for dir in dirs {
        for name in ["config", "config.worktree"] {
            let path = dir.join(name);
            match std::fs::metadata(&path) {
                Ok(meta) if !meta.is_file() => {
                    return Err(format!(
                        "repository configuration {} is not a regular file",
                        path.display()
                    ))
                }
                Ok(meta) if meta.len() > LIMIT => {
                    return Err(format!(
                        "repository configuration {} is larger than {LIMIT} bytes",
                        path.display()
                    ))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
            }
        }
    }
    Ok(())
}

/// Open the repository at or above `start`; only a proven non-repository is absent.
///
/// # Errors
/// Discovery fails, including an existing repository marker with malformed metadata.
pub fn open(start: &Path) -> Result<Option<Repository>, String> {
    let start_path = crate::portable_path::without_verbatim(start.to_path_buf());
    // libgit2 opens the repository's configuration with a blocking read, so
    // a fifo there would hold the hook: check the files first (TSK-397
    // security review SEC01-3).
    if let Ok(git_dir) = Repository::discover_path(&start_path, std::iter::empty::<&Path>()) {
        config_files_readable(&git_dir)?;
    }
    match Repository::discover(start_path) {
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
        // A name that holds nothing holds no marker, and probing one per
        // missing ancestor made this walk quadratic in depth (TSK-1258).
        let present = crate::absence::deepest_entry(&target).map_err(|error| {
            format!(
                "cannot inspect target repository {}: {error}",
                target.display()
            )
        })?;
        for ancestor in present.ancestors() {
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
    distinct_owners(target, &roots)
}

/// The owners in `roots` that `target` must be judged against. An owner
/// `target` could not enforce through may take the reading kept earlier in
/// this scope. Two owners opened now that share a common directory keep the
/// first. An owner whose reading was kept is always listed: its metadata
/// shows that it changed, never that it is still the same repository, so a
/// kept reading never removes an owner as a duplicate (review 03 of PR
/// 386). Every consumer refuses when any owner protects the target, so an
/// owner listed twice over one common directory changes no verdict.
fn distinct_owners(target: &Path, roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    // The common directories of the owners opened now.
    let mut common = std::collections::BTreeSet::new();
    let mut distinct = Vec::new();
    for root in roots {
        // Main and linked checkouts share the complete protection set. Open
        // each discovered owner first, so deduplication cannot hide a broken
        // pointer or a disagreement with its registration. An owner whose
        // enforcement paths `target` may name is always opened now, so a
        // kept reading never stands in for the refusal a broken owner gives.
        let enforced = could_enforce(target, root);
        let (opened, kept) = owner_common(root, !enforced);
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
        if let (Some(directory), false) = (opened, kept) {
            if !common.insert(directory?) {
                continue;
            }
        }
        distinct.push(root.clone());
    }
    Ok(distinct)
}

/// What opening one discovered owner gave: the error [`open`] reported, or
/// for what it opened the canonical common directory (`None` for a proven
/// non-repository, an error when that directory cannot be resolved).
type OwnerCommon = Result<Option<Result<PathBuf, String>>, String>;

/// The metadata of the files an owner's reading starts from: `.git` (a
/// directory, or a file naming a linked worktree's Git directory) and
/// `HEAD` (where the root is itself a Git directory), each `None` when
/// proven absent. A reading is reused only while these are unchanged, so an
/// owner replaced, broken or mended during the scope is opened again. This
/// detects a change; it does not prove the repository is the same, so a
/// reused reading only ever adds an owner (see [`distinct_owners`]).
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Marker(Vec<Option<MarkerFile>>);

#[derive(Clone, PartialEq, Eq)]
struct MarkerFile {
    dir: bool,
    len: u64,
    modified: std::time::SystemTime,
    id: u64,
}

/// The [`Marker`] of the owner at `root`, or `None` when any of its
/// metadata cannot be read: such an owner is opened again on every lookup
/// and its reading is not kept.
fn marker(root: &Path) -> Option<Marker> {
    [".git", "HEAD"]
        .iter()
        .map(
            |name| match crate::absence::symlink_metadata_optional(&root.join(name)).ok()? {
                None => Some(None),
                Some(meta) => Some(Some(MarkerFile {
                    dir: meta.is_dir(),
                    len: meta.len(),
                    modified: meta.modified().ok()?,
                    id: file_id(&meta),
                })),
            },
        )
        .collect::<Option<Vec<_>>>()
        .map(Marker)
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
/// successful reading answers while the owner's [`Marker`] reads the same;
/// otherwise the owner is opened now, and a successful reading is kept for
/// later lookups when its marker could be read. A failure is never kept, so
/// the next lookup opens the owner again.
///
/// Callers never reuse for a target the owner could enforce, and a kept
/// reading never removes an owner (see [`distinct_owners`]). One evaluation
/// asks for the owners of many ordinary targets in the same repository, so
/// those lookups share one open.
fn owner_common(root: &Path, reuse: bool) -> (OwnerCommon, bool) {
    let now = marker(root);
    if let (true, Some(now)) = (reuse, &now) {
        if let Some((kept, _)) = OWNERS.with(|cache| {
            cache
                .borrow()
                .as_ref()
                .and_then(|map| map.get(root))
                .filter(|(_, read_from)| read_from == now)
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
    OWNERS.with(|cache| match (cache.borrow_mut().as_mut(), success, now) {
        (Some(map), Some(reading), Some(now)) => {
            map.insert(root.to_path_buf(), (reading, now));
        }
        (Some(map), _, _) => {
            map.remove(root);
        }
        (None, _, _) => {}
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
    check_common_paths(&admin, repo.commondir())
}

/// The comparison behind [`check_common_directory`], from the canonical
/// administrative directory and the common directory the checkout names.
fn check_common_paths(admin: &Path, commondir: &Path) -> Result<(), String> {
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
        || canonical(commondir, "worktree common directory")? != expected
    {
        return Err("worktree common directory disagrees with its registration directory".into());
    }
    Ok(())
}

/// The text of a small marker file in `tree`, read through the held handle:
/// at most 4 KiB, never through a link, never blocking on a special file, and
/// the type checked on the opened descriptor.
fn small_marker(tree: &crate::contained::Tree, name: &str, what: &str) -> Result<String, String> {
    let bytes = tree
        .read(name, 4096)
        .map_err(|error| format!("cannot read {what}: {error}"))?;
    String::from_utf8(bytes.trim_ascii_end().to_vec())
        .map_err(|_| format!("{what} is not valid text"))
}

/// Whether the held directory is a git directory: a `HEAD` file and
/// `objects` and `refs` folders, checked on the handle that is returned.
fn require_repository(tree: &crate::contained::Tree) -> Result<(), String> {
    if !tree.metadata("HEAD").is_ok_and(|m| m.is_file()) {
        return Err("git directory has no `HEAD` file".into());
    }
    for folder in ["objects", "refs"] {
        if !tree.metadata(folder).is_ok_and(|m| m.is_dir()) {
            return Err(format!("git directory has no `{folder}` folder"));
        }
    }
    Ok(())
}

#[cfg(test)]
type Interleave = Box<dyn FnMut(&str)>;

#[cfg(test)]
thread_local! {
    /// Runs at the named points inside [`shared_git_tree`] and the sibling
    /// gate of [`read_registration`] so a test can swap a folder or a file
    /// exactly there.
    pub(crate) static INTERLEAVE: std::cell::RefCell<Option<Interleave>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn interleave(stage: &str) {
    if let Some(mut hook) = INTERLEAVE.with(|slot| slot.borrow_mut().take()) {
        hook(stage);
        INTERLEAVE.with(|slot| *slot.borrow_mut() = Some(hook));
    }
}

/// The tree that holds the shared config for the checkout holding
/// `project_dir`, found without opening the repository (opening reads its
/// config, which can block on a pipe in its place).
///
/// An ordinary git directory is its own shared root. A linked worktree's
/// administrative directory is accepted only when its registration is real:
///
/// - its `commondir` file names a common directory that is physically the
///   grandparent of the administrative directory (`<common>/worktrees/<name>`
///   with the middle folder named `worktrees`), so an alias cannot stand in;
/// - its `gitdir` file names the checkout's `.git` marker;
/// - that checkout's marker names this administrative directory.
///
/// The returned tree is the handle opened here: the caller works through it
/// and never re-resolves a path, so a rename or link swapped in afterwards
/// cannot move the work to another directory. Every marker is read through a
/// held handle, bounded, without following a link or blocking on a pipe.
/// Any disagreement refuses before a caller creates a file.
///
/// # Errors
///
/// When no git directory is found, a marker is not a small regular file, or
/// the registration, common directory and checkout disagree.
pub(crate) fn shared_git_tree(project_dir: &Path) -> Result<crate::contained::Tree, String> {
    use crate::contained::Tree;
    let found = Repository::discover_path(project_dir, [] as [&Path; 0])
        .map_err(|error| error.message().to_string())?;
    #[cfg(test)]
    interleave("before-admin");
    let admin =
        Tree::open(&found).map_err(|error| format!("cannot open git directory: {error}"))?;
    match admin.metadata("commondir") {
        Ok(_) => {}
        // No commondir in the held directory: an ordinary git directory,
        // which is accepted only when this handle itself holds a repository.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            require_repository(&admin)?;
            return Ok(admin);
        }
        Err(error) => return Err(format!("cannot inspect commondir: {error}")),
    }
    let named = small_marker(&admin, "commondir", "commondir")?;
    if named.is_empty() {
        return Err("commondir is empty".into());
    }
    let registrations = admin
        .path()
        .parent()
        .filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees"))
        .ok_or("worktree administrative directory is not under `worktrees`")?;
    let physical = registrations
        .parent()
        .ok_or("worktree registration has no common directory")?;
    let admin_name = admin
        .path()
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or("worktree administrative directory has no usable name")?;
    #[cfg(test)]
    interleave("before-common");
    let common = Tree::open(&admin.path().join(&named))
        .map_err(|error| format!("cannot open common directory: {error}"))?;
    if common.path() != physical {
        return Err("worktree common directory disagrees with its registration".into());
    }
    // The registration, found again beneath the common directory that was
    // just opened, must be the very folder held as `admin`: a common
    // directory swapped in after `admin` was opened holds a different one.
    let registered_here = common
        .subtree(&format!("worktrees/{admin_name}"))
        .map_err(|error| format!("common directory does not hold the registration: {error}"))?;
    if !registered_here
        .same_directory(&admin)
        .map_err(|error| format!("cannot compare the registration: {error}"))?
    {
        return Err("common directory holds a different registration than the one read".into());
    }
    require_repository(&common)?;
    // The registration names the checkout's marker, and that marker names
    // this administrative directory.
    let registered = registered_marker_from(
        admin.path(),
        small_marker(&admin, "gitdir", "worktree registration")?.as_bytes(),
    )?;
    let mut checkout = None;
    for dir in project_dir.ancestors() {
        if std::fs::symlink_metadata(dir.join(".git")).is_ok() {
            checkout = Some(
                Tree::open(dir).map_err(|error| format!("cannot open the checkout: {error}"))?,
            );
            break;
        }
    }
    let checkout = checkout.ok_or("no checkout marker above the project")?;
    if registered != checkout.path().join(".git") {
        return Err("worktree registration names a different checkout".into());
    }
    let text = small_marker(&checkout, ".git", "checkout marker")?;
    let target = text
        .strip_prefix("gitdir:")
        .map(str::trim_ascii)
        .filter(|target| !target.is_empty())
        .ok_or("checkout marker does not name a git directory")?;
    let named = checkout.path().join(target);
    #[cfg(test)]
    interleave("before-checkout-target");
    let named = Tree::open(&named)
        .map_err(|error| format!("cannot open checkout git directory: {error}"))?;
    if named.path() != admin.path()
        || !named
            .same_directory(&admin)
            .map_err(|error| format!("cannot compare the checkout marker's target: {error}"))?
    {
        return Err("checkout marker names a different git directory".into());
    }
    // The same objects must still be what the held common directory lists.
    require_repository(&common)?;
    Ok(common)
}

type RegistrationListings = std::collections::HashMap<PathBuf, Result<Vec<PathBuf>, String>>;

thread_local! {
    /// The registration listing of each common directory, read once per guard
    /// evaluation (TSK-1275), present only while a
    /// [`super::edit_guard::RepoFactsScope`] is open and dropped with it. One
    /// call lists the registrations for discovery, for the checkout roots and
    /// for the protected paths; libgit2 reads the `worktrees` directory and
    /// three files of every entry each time. A failed listing is kept for the
    /// evaluation too, so every reader refuses alike. Nothing is kept between
    /// evaluations.
    pub(super) static LISTINGS: std::cell::RefCell<Option<RegistrationListings>> =
        const { std::cell::RefCell::new(None) };
}

// Inventory the administrative names before interpreting their pointers. A
// relative missing leaf is prunable too; the generic worktree reader requires
// a checkout parent in the raw text before it can represent that checkout.
pub(super) fn registrations(repo: &Repository) -> Result<Vec<PathBuf>, String> {
    let key = repo.commondir().to_path_buf();
    if let Some(kept) = LISTINGS.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|listings| listings.get(&key).cloned())
    }) {
        return kept;
    }
    let listed = list_registrations(repo);
    LISTINGS.with(|cache| {
        if let Some(listings) = cache.borrow_mut().as_mut() {
            listings.insert(key, listed.clone());
        }
    });
    listed
}

fn list_registrations(repo: &Repository) -> Result<Vec<PathBuf>, String> {
    #[cfg(test)]
    LISTING_READS.with(|reads| reads.set(reads.get() + 1));
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
    // One bounded read: no link followed, no wait on a pipe, the type checked
    // on the opened descriptor (a path inspected and then reopened could be
    // swapped in between).
    let text = crate::bounded_file::read_bounded_regular(&admin.join("gitdir"), 4096)
        .map_err(|error| format!("cannot read worktree registration: {error}"))?;
    registered_marker_from(admin, &text)
}

/// The normalized marker a registration's text names, relative to `admin`
/// when the text is relative.
fn registered_marker_from(admin: &Path, text: &[u8]) -> Result<PathBuf, String> {
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
    /// How many times [`list_registrations`] asked libgit2 for the listing.
    static LISTING_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    // The registered spelling is usually the checkout's own: one resolution
    // then answers both (TSK-1275).
    let checkout_marker = if registered == marker {
        registered_marker.clone()
    } else {
        canonical(&marker, "checkout marker")?
    };
    if registered_marker != checkout_marker {
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
    // TSK-1275: a sibling skips its full open only when its own facts hold,
    // an earlier full open of another sibling of the same physical common
    // directory, in this evaluation, accepted the files they share, and
    // those files read now as they did then.
    let judged = if let Some(common) = gated_sibling_facts(admin, &root) {
        match gate_step(admin, &common) {
            GateStep::Skip => {
                #[cfg(test)]
                SKIPPED_OPENS.with(|skips| skips.set(skips.get() + 1));
                return Ok(CheckedRegistration {
                    marker: registered,
                    checkout: Some(root),
                });
            }
            GateStep::Judge(before) => Some((common, before)),
            GateStep::Open => None,
        }
    } else {
        None
    };
    let checkout = open_checkout(&root)
        .map_err(|error| format!("cannot open registered checkout: {error}"))?;
    if !checkout.is_worktree()
        || canonical(checkout.path(), "registered checkout git directory")?
            != canonical(admin, "worktree administrative directory")?
    {
        return Err("worktree registration disagrees with its checkout marker".into());
    }
    check_common_directory(&checkout)?;
    if let Some((common, before)) = judged {
        #[cfg(test)]
        interleave("after-gate-open");
        judge_shared_files(admin, common, before);
    }
    Ok(CheckedRegistration {
        marker: registered,
        checkout: Some(root),
    })
}

/// What one guard evaluation has learned about the files linked checkouts
/// share (TSK-1275), present only while a
/// [`super::edit_guard::RepoFactsScope`] is open and dropped with it.
#[derive(Default)]
pub(super) struct SiblingGate {
    /// Physical common directories one full sibling open accepted, whose
    /// shared configuration is the same for every sibling, each with the
    /// stamp of the shared files that open read. A skip relies on them only
    /// while they still read the same.
    proven: std::collections::HashMap<PathBuf, SharedStamp>,
    /// Common directories, spelled as their registrations are listed, whose
    /// shared configuration can differ between siblings, so every sibling
    /// keeps its full open and no facts are read for them.
    doubtful: std::collections::HashSet<PathBuf>,
}

thread_local! {
    pub(super) static SIBLING_GATE: std::cell::RefCell<Option<SiblingGate>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// How many sibling opens [`read_registration`] skipped.
    static SKIPPED_OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, unix))]
thread_local! {
    /// Whether [`shared_stamp`] refuses shared files changed within
    /// [`SETTLED_MARGIN_SECS`]. Tests build their fixtures moments before
    /// they validate them, so only the test of the margin turns it on.
    static REQUIRE_SETTLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A shared file is relied on only when it last changed more than this many
/// seconds before it was stamped. A later change then always moves its
/// change time, at every timestamp granularity the guard meets (1 s on
/// HFS+ and network volumes, 2 s on FAT); a change inside one tick could
/// otherwise leave the stamp as it was.
#[cfg(unix)]
const SETTLED_MARGIN_SECS: i64 = 3;

/// One shared file as it read at one moment: its device, inode and size,
/// then its modification and status-change times in seconds and
/// nanoseconds. A write moves the status-change time, which no caller can
/// set back, and a replacement moves the inode.
type FileStamp = (u64, u64, u64, (i64, i64), (i64, i64));

/// The files every sibling of one common directory reads on a full open,
/// as they read at one moment (TSK-1275): the common `config` and
/// `info/grafts`, and the global, XDG and system configuration files libgit2
/// finds then. `None` beside a path records it proven absent.
type SharedStamp = Vec<(PathBuf, Option<FileStamp>)>;

/// The change time, in seconds, a stamped file must be older than: no limit
/// when the margin is off (tests only), and nothing settles when the clock
/// cannot be read.
#[cfg(unix)]
fn settled_before() -> i64 {
    #[cfg(test)]
    if !REQUIRE_SETTLED.with(std::cell::Cell::get) {
        return i64::MAX;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .map_or(i64::MIN, |now| now.saturating_sub(SETTLED_MARGIN_SECS))
}

/// The shared files of `common` as they read now. `None`, which no stamp
/// equals, when a file cannot be read or proven absent, or changed within
/// the settling margin. No sibling skips on a host that is not Unix, so
/// there no stamp is taken.
fn shared_stamp(common: &Path) -> Option<SharedStamp> {
    #[cfg(unix)]
    {
        shared_stamp_unix(common)
    }
    #[cfg(not(unix))]
    {
        let _ = common;
        None
    }
}

#[cfg(unix)]
fn shared_stamp_unix(common: &Path) -> Option<SharedStamp> {
    use std::os::unix::fs::MetadataExt;
    let limit = settled_before();
    // The common config must be a regular file to qualify at all, so it is
    // stamped without following a link; libgit2 follows links for the rest.
    let mut files = vec![
        (common.join("config"), false),
        (common.join("info").join("grafts"), true),
    ];
    files.extend(
        [
            git2::Config::find_global(),
            git2::Config::find_xdg(),
            git2::Config::find_system(),
        ]
        .into_iter()
        .flatten()
        .map(|path| (path, true)),
    );
    let mut stamps = Vec::with_capacity(files.len());
    for (path, follow) in files {
        let read = if follow {
            std::fs::metadata(&path)
        } else {
            std::fs::symlink_metadata(&path)
        };
        let stamp = match read {
            Ok(meta) => Some((
                meta.dev(),
                meta.ino(),
                meta.size(),
                (meta.mtime(), meta.mtime_nsec()),
                (meta.ctime(), meta.ctime_nsec()),
            )),
            Err(_) if matches!(crate::absence::proven_absent(&path), Ok(true)) => None,
            Err(_) => return None,
        };
        if let Some((_, _, _, (modified, _), (changed, _))) = stamp {
            if modified.max(changed) >= limit {
                return None;
            }
        }
        stamps.push((path, stamp));
    }
    Some(stamps)
}

/// What a sibling whose own facts hold does next.
enum GateStep {
    /// The common directory is proven and its shared files read as they
    /// did when it was proven: skip the full open.
    Skip,
    /// Nothing is proven yet: take the full open, then judge the shared
    /// files against this stamp, taken before the open.
    Judge(Option<SharedStamp>),
    /// The shared files changed since the proof: take the full open, and
    /// every later sibling of this common directory takes it too.
    Open,
}

fn gate_step(admin: &Path, common: &Path) -> GateStep {
    let proven = SIBLING_GATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|gate| gate.proven.contains_key(common))
    });
    let now = shared_stamp(common);
    if !proven {
        return GateStep::Judge(now);
    }
    SIBLING_GATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(gate) = slot.as_mut() else {
            return GateStep::Open;
        };
        if now.is_some() && gate.proven.get(common) == now.as_ref() {
            return GateStep::Skip;
        }
        gate.proven.remove(common);
        if let Some(listed) = listed_common(admin) {
            gate.doubtful.insert(listed.to_path_buf());
        }
        GateStep::Open
    })
}

/// The registration's common directory as the listing spelled it.
fn listed_common(admin: &Path) -> Option<&Path> {
    admin.parent().and_then(Path::parent)
}

/// The sibling's facts, read only inside a guard evaluation and only for a
/// common directory not already known to be doubtful.
fn gated_sibling_facts(admin: &Path, root: &Path) -> Option<PathBuf> {
    let wanted = SIBLING_GATE.with(|gate| {
        gate.borrow().as_ref().is_some_and(|gate| {
            listed_common(admin).is_some_and(|common| !gate.doubtful.contains(common))
        })
    });
    if !wanted {
        return None;
    }
    sibling_facts(admin, root)
}

/// After a full open of a sibling whose own facts held succeeded, judge
/// once per evaluation whether the files siblings share read the same for
/// every sibling, and record the answer for the rest of this evaluation.
/// The judged files must be the ones that open read: their stamp from
/// before the open must still hold after the judgement, or the common
/// directory stays doubtful.
fn judge_shared_files(admin: &Path, common: PathBuf, before: Option<SharedStamp>) {
    let proven = before.filter(|before| {
        shared_config_is_sibling_independent(&common)
            && shared_stamp(&common).as_ref() == Some(before)
    });
    SIBLING_GATE.with(|gate| {
        if let Some(gate) = gate.borrow_mut().as_mut() {
            if let Some(stamp) = proven {
                gate.proven.insert(common, stamp);
            } else if let Some(listed) = listed_common(admin) {
                gate.doubtful.insert(listed.to_path_buf());
            }
        }
    });
    #[cfg(test)]
    interleave("after-gate-judged");
}

/// TSK-1275: the per-sibling facts that, with an earlier full open of
/// another sibling of the same physical common directory in this
/// evaluation, prove this sibling's `Repository::open` would succeed (the
/// equivalence table in the task record, against libgit2 1.9.6
/// `git_repository_open_ext`). Returns that canonical common directory.
/// Any doubt, error or unsupported host returns `None`, and the caller keeps
/// the full open, so a rejection keeps its cause. Computed for this
/// registration in this call and never stored.
fn sibling_facts(admin: &Path, root: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        sibling_facts_unix(admin, root)
    }
    #[cfg(not(unix))]
    {
        let _ = (admin, root);
        None
    }
}

#[cfg(unix)]
fn sibling_facts_unix(admin: &Path, root: &Path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    // Only regular files and directories are read here, and no link is
    // followed; anything else keeps the full open.
    let regular = |path: &Path| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| meta.file_type().is_file())
    };
    let directory = |path: &Path| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| meta.file_type().is_dir())
    };
    // Proven absent, never merely unreadable: an entry of any kind, a read
    // error or a doubtful ancestor keeps the full open.
    let absent = |path: &Path| matches!(crate::absence::proven_absent(path), Ok(true));
    // The checkout marker is a gitfile of exactly `gitdir: <absolute path>`
    // naming this administrative directory.
    let marker = root.join(".git");
    let text = crate::bounded_file::read_bounded_regular(&marker, 4096).ok()?;
    let target = text.strip_prefix(b"gitdir: ")?.trim_ascii_end();
    if target.is_empty() || target.contains(&0) || target.contains(&b'\n') {
        return None;
    }
    let target = Path::new(std::ffi::OsStr::from_bytes(target));
    if !target.is_absolute() {
        return None;
    }
    // A target spelled as the canonical directory resolves to it unchanged.
    let admin = admin.canonicalize().ok()?;
    if target != admin && target.canonicalize().ok()? != admin {
        return None;
    }
    // A linked worktree's registration: its `gitdir` and `HEAD` files, no
    // configuration or shallow file of its own (libgit2 reads both from the
    // sibling's own git directory), and a `commondir` naming the physical
    // parent of the registration directory.
    regular(&admin.join("gitdir"))?;
    regular(&admin.join("HEAD"))?;
    if !absent(&admin.join("config.worktree")) || !absent(&admin.join("shallow")) {
        return None;
    }
    let named = crate::bounded_file::read_bounded_regular(&admin.join("commondir"), 4096).ok()?;
    let named = named.trim_ascii_end();
    if named.is_empty() || named.contains(&0) || named.contains(&b'\n') {
        return None;
    }
    // `../..`, which git writes, names the canonical directory's grandparent:
    // in a path without links `..` is the parent the path spells. Any other
    // spelling is resolved.
    let common = if named == b"../.." {
        admin.parent()?.parent()?.to_path_buf()
    } else {
        admin
            .join(Path::new(std::ffi::OsStr::from_bytes(named)))
            .canonicalize()
            .ok()?
    };
    // Either way the full open's physical check applies: the registration
    // directory must be `<common>/worktrees`. A listed registration can link
    // to an administrative directory anywhere under the common directory,
    // and the full open refuses that.
    check_common_paths(&admin, &common).ok()?;
    regular(&common.join("HEAD"))?;
    directory(&common.join("objects"))?;
    directory(&common.join("refs"))?;
    // Ownership: only a checkout whose work directory, gitfile and git
    // directory all belong to the effective user qualifies; any other owner
    // keeps libgit2's `safe.directory` handling.
    // SAFETY: geteuid has no preconditions and cannot fail.
    let user = unsafe { libc::geteuid() };
    for path in [root, marker.as_path(), admin.as_path()] {
        if std::fs::symlink_metadata(path).ok()?.uid() != user {
            return None;
        }
    }
    Some(common)
}

/// Whether one configuration file, as libgit2 itself parses it, reads the
/// same for every sibling: no include of any kind (a conditional include
/// reads the sibling's own git directory or branch), no `core.worktree`,
/// and `core.bare` only false. `extensions.worktreeConfig` is allowed: it
/// adds a sibling's own `config.worktree`, and a sibling with one never
/// qualifies. A file that cannot be parsed is doubtful.
fn config_is_sibling_independent(path: &Path) -> bool {
    let Ok(config) = git2::Config::open(path) else {
        return false;
    };
    let Ok(mut entries) = config.entries(None) else {
        return false;
    };
    while let Some(entry) = entries.next() {
        let Ok(entry) = entry else {
            return false;
        };
        // Names are compared as bytes, folded as ASCII as git folds section
        // and key names, so a name that is not UTF-8 is still judged.
        let name = entry.name_bytes().to_ascii_lowercase();
        let bare_not_false = name == b"core.bare"
            && !entry
                .value()
                .is_ok_and(|value| matches!(git2::Config::parse_bool(value), Ok(false)));
        if name.starts_with(b"include.")
            || name.starts_with(b"includeif.")
            || name == b"core.worktree"
            || bare_not_false
        {
            return false;
        }
    }
    true
}

/// The shared configuration of one physical common directory: its local
/// file and the global, XDG and system files every sibling also reads must
/// each be sibling independent. Judged once per evaluation.
fn shared_config_is_sibling_independent(common: &Path) -> bool {
    let local = common.join("config");
    std::fs::symlink_metadata(&local).is_ok_and(|meta| meta.file_type().is_file())
        && config_is_sibling_independent(&local)
        && [
            git2::Config::find_global(),
            git2::Config::find_xdg(),
            git2::Config::find_system(),
        ]
        .into_iter()
        .flatten()
        .all(|path| config_is_sibling_independent(&path))
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

    /// TSK-1275: discovery, the checkout roots and the protected paths all list
    /// the registrations; one guard evaluation asks libgit2 once per common
    /// directory, and the next evaluation asks again.
    #[test]
    fn registrations_are_listed_once_per_guard_invocation() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        for name in ["one", "two"] {
            git(dir.path(), &["worktree", "add", "-b", name, name]);
        }
        let listings = || super::LISTING_READS.with(std::cell::Cell::get);
        for _ in 0..2 {
            let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
            super::LISTING_READS.with(|reads| reads.set(0));
            for cwd in [dir.path().to_path_buf(), dir.path().join("one")] {
                super::RepoInfo::discover(&cwd).unwrap().unwrap();
                let repo = super::open(&cwd).unwrap().unwrap();
                crate::hooks::edit_guard::checkout_roots(&repo).unwrap();
                crate::hooks::edit_guard::holds_enforcement_files(&cwd.join("build"), &cwd)
                    .unwrap();
            }
            assert_eq!(listings(), 1);
        }
        // Outside a scope nothing is kept.
        super::LISTING_READS.with(|reads| reads.set(0));
        let repo = super::open(dir.path()).unwrap().unwrap();
        super::registrations(&repo).unwrap();
        super::registrations(&repo).unwrap();
        assert_eq!(listings(), 2);
    }

    /// The canonical administrative directory and checkout root of a new
    /// linked worktree `name`.
    fn sibling(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
        git(dir, &["worktree", "add", "-b", name, name]);
        let root = dir.join(name).canonicalize().unwrap();
        let admin = dir
            .join(".git")
            .join("worktrees")
            .join(name)
            .canonicalize()
            .unwrap();
        (admin, root)
    }

    fn skipped() -> usize {
        super::SKIPPED_OPENS.with(std::cell::Cell::get)
    }

    /// Validate `admins` in one guard evaluation; returns the skipped opens.
    fn validate_in_one_scope(admins: &[&Path]) -> usize {
        let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
        let before = skipped();
        for admin in admins {
            super::read_registration(admin).unwrap();
        }
        skipped() - before
    }

    /// TSK-1275: after one full sibling open in an evaluation, a sibling of
    /// the same common directory whose own facts hold skips its open; the
    /// first sibling never skips, nothing is skipped outside an evaluation,
    /// and the next evaluation starts again (AC-6).
    #[cfg(unix)]
    #[test]
    fn a_sibling_skips_its_open_only_after_a_full_open_in_the_same_evaluation() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, _) = sibling(dir.path(), "two");
        let (three, _) = sibling(dir.path(), "three");
        let common = dir.path().join(".git").canonicalize().unwrap();
        // The host's own global and system files decide whether any skip is
        // allowed; a host whose files could differ per sibling skips none.
        let expected = if super::shared_config_is_sibling_independent(&common) {
            2
        } else {
            0
        };
        let before = skipped();
        for admin in [&one, &two, &three] {
            super::read_registration(admin).unwrap();
        }
        assert_eq!(
            skipped(),
            before,
            "outside an evaluation every sibling opens"
        );
        assert_eq!(validate_in_one_scope(&[&one, &two, &three]), expected);
        assert_eq!(validate_in_one_scope(&[&two, &three]), expected / 2);
        assert!(super::SIBLING_GATE.with(|gate| gate.borrow().is_none()));
    }

    /// A named condition of a skip test: how to break it and how to mend it.
    #[cfg(unix)]
    type Case<'a> = (&'a str, Box<dyn Fn() + 'a>, Box<dyn Fn() + 'a>);

    /// TSK-1275: a commondir naming any other directory keeps the full open,
    /// and other spellings of the same directories are resolved before they
    /// prove anything: an absolute commondir, and a gitfile naming the
    /// registration through a link.
    #[cfg(unix)]
    #[test]
    fn commondir_and_gitfile_spellings_are_resolved_before_a_skip() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        let marker = two_root.join(".git");
        let original = std::fs::read(&marker).unwrap();
        let commondir = two.join("commondir");
        let written = std::fs::read(&commondir).unwrap();
        assert_eq!(written.trim_ascii_end(), b"../..");
        let foreign = dir.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        init_repo(&foreign);
        let unproven: [Case; 2] = [
            (
                "commondir naming another git directory",
                Box::new(|| {
                    let other = foreign.join(".git").canonicalize().unwrap();
                    std::fs::write(&commondir, format!("{}\n", other.display())).unwrap();
                }),
                Box::new(|| std::fs::write(&commondir, &written).unwrap()),
            ),
            (
                "commondir that does not resolve",
                Box::new(|| std::fs::write(&commondir, "../../missing\n").unwrap()),
                Box::new(|| std::fs::write(&commondir, &written).unwrap()),
            ),
        ];
        for (case, break_it, mend) in &unproven {
            break_it();
            assert!(super::sibling_facts(&two, &two_root).is_none(), "{case}");
            let skips = {
                let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                let before = skipped();
                super::read_registration(&one).unwrap();
                let _ = super::read_registration(&two);
                skipped() - before
            };
            assert_eq!(skips, 0, "{case}");
            mend();
            assert_eq!(validate_in_one_scope(&[&one, &two]), 1, "{case} mended");
        }
        std::fs::write(&commondir, format!("{}\n", common.display())).unwrap();
        assert_eq!(super::sibling_facts(&two, &two_root), Some(common.clone()));
        std::fs::write(&commondir, &written).unwrap();
        let alias = dir.path().join("admin-alias");
        std::os::unix::fs::symlink(&two, &alias).unwrap();
        std::fs::write(&marker, format!("gitdir: {}\n", alias.display())).unwrap();
        assert_eq!(super::sibling_facts(&two, &two_root), Some(common.clone()));
        std::fs::write(&marker, &original).unwrap();
    }

    /// TSK-1275: each condition of the equivalence table keeps the full open
    /// for the sibling it concerns, and a shared file that could differ per
    /// sibling keeps it for all of them. Every case is judged as the files
    /// are now; no answer carries over from the evaluation before (AC-6).
    #[cfg(unix)]
    #[test]
    fn every_unproven_condition_keeps_the_full_open() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        assert_eq!(validate_in_one_scope(&[&one, &two]), 1);
        let marker = two_root.join(".git");
        let original = std::fs::read(&marker).unwrap();
        let per_sibling: [Case; 6] = [
            (
                "own config.worktree",
                Box::new(|| std::fs::write(two.join("config.worktree"), "[core]\n").unwrap()),
                Box::new(|| std::fs::remove_file(two.join("config.worktree")).unwrap()),
            ),
            (
                "own shallow file",
                Box::new(|| std::fs::write(two.join("shallow"), "not an object id\n").unwrap()),
                Box::new(|| std::fs::remove_file(two.join("shallow")).unwrap()),
            ),
            (
                "relative gitfile",
                Box::new(|| std::fs::write(&marker, "gitdir: ../.git/worktrees/two\n").unwrap()),
                Box::new(|| std::fs::write(&marker, &original).unwrap()),
            ),
            (
                "gitfile without the space",
                Box::new(|| {
                    std::fs::write(&marker, format!("gitdir:{}\n", two.display())).unwrap();
                }),
                Box::new(|| std::fs::write(&marker, &original).unwrap()),
            ),
            (
                "symlinked HEAD",
                Box::new(|| {
                    std::fs::rename(two.join("HEAD"), two.join("HEAD.real")).unwrap();
                    std::os::unix::fs::symlink(two.join("HEAD.real"), two.join("HEAD")).unwrap();
                }),
                Box::new(|| {
                    std::fs::remove_file(two.join("HEAD")).unwrap();
                    std::fs::rename(two.join("HEAD.real"), two.join("HEAD")).unwrap();
                }),
            ),
            (
                "symlinked gitfile",
                Box::new(|| {
                    std::fs::rename(&marker, two_root.join("marker.real")).unwrap();
                    std::os::unix::fs::symlink(two_root.join("marker.real"), &marker).unwrap();
                }),
                Box::new(|| {
                    std::fs::remove_file(&marker).unwrap();
                    std::fs::rename(two_root.join("marker.real"), &marker).unwrap();
                }),
            ),
        ];
        for (case, break_it, mend) in &per_sibling {
            break_it();
            assert!(super::sibling_facts(&two, &two_root).is_none(), "{case}");
            // The full open decides: a sibling libgit2 accepts still passes,
            // and one it refuses keeps today's refusal.
            let skips = {
                let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                let before = skipped();
                super::read_registration(&one).unwrap();
                let _ = super::read_registration(&two);
                skipped() - before
            };
            assert_eq!(skips, 0, "{case}");
            mend();
            assert_eq!(validate_in_one_scope(&[&one, &two]), 1, "{case} mended");
        }
        // Shared files: an include, a conditional include, a core.worktree
        // or a bare repository flag in the common configuration keeps every
        // full open; worktreeConfig alone does not.
        let config = common.join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        for (added, independent) in [
            ("[include]\n\tpath = other\n", false),
            ("[includeIf \"gitdir:/nowhere/\"]\n\tpath = other\n", false),
            ("[core]\n\tworktree = /elsewhere\n", false),
            ("[core]\n\tbare = true\n", false),
            ("[extensions]\n\tworktreeConfig = true\n", true),
        ] {
            std::fs::write(&config, format!("{text}{added}")).unwrap();
            assert_eq!(
                super::shared_config_is_sibling_independent(&common),
                independent,
                "{added}"
            );
            std::fs::write(&config, &text).unwrap();
        }
    }

    /// TSK-1275: a sibling's own `config.worktree` or `shallow` keeps the
    /// full open unless it is proven absent: present but unreadable, a link
    /// (dangling or not), a directory, or a case variant of its name on a
    /// volume that folds case. Configuration names are judged as bytes, so
    /// an include, `core.worktree` or `core.bare` in any letter case, or
    /// under a section name that is not UTF-8, keeps every full open.
    #[cfg(unix)]
    #[test]
    fn a_sibling_file_not_proven_absent_keeps_the_full_open() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        assert_eq!(validate_in_one_scope(&[&one, &two]), 1);
        // Whether this volume folds case decides what a case variant names.
        let probe = two.join("CASE-PROBE");
        std::fs::write(&probe, "").unwrap();
        let folds_case = std::fs::symlink_metadata(two.join("case-probe")).is_ok();
        std::fs::remove_file(&probe).unwrap();
        let target = two.join("link-target");
        std::fs::write(&target, "[core]\n").unwrap();
        for name in ["config.worktree", "shallow"] {
            let path = two.join(name);
            let variant = two.join(name.to_ascii_uppercase());
            let mut cases: Vec<Case> = vec![
                (
                    "present and unreadable",
                    Box::new(|| {
                        std::fs::write(&path, "[core]\n").unwrap();
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
                            .unwrap();
                    }),
                    Box::new(|| std::fs::remove_file(&path).unwrap()),
                ),
                (
                    "a dangling link",
                    Box::new(|| {
                        std::os::unix::fs::symlink(two.join("missing"), &path).unwrap();
                    }),
                    Box::new(|| std::fs::remove_file(&path).unwrap()),
                ),
                (
                    "a link to a file",
                    Box::new(|| std::os::unix::fs::symlink(&target, &path).unwrap()),
                    Box::new(|| std::fs::remove_file(&path).unwrap()),
                ),
                (
                    "a directory",
                    Box::new(|| std::fs::create_dir(&path).unwrap()),
                    Box::new(|| std::fs::remove_dir(&path).unwrap()),
                ),
            ];
            if folds_case {
                cases.push((
                    "a case variant of its name",
                    Box::new(|| std::fs::write(&variant, "[core]\n").unwrap()),
                    Box::new(|| std::fs::remove_file(&variant).unwrap()),
                ));
            }
            for (case, break_it, mend) in &cases {
                break_it();
                assert!(
                    super::sibling_facts(&two, &two_root).is_none(),
                    "{name}: {case}"
                );
                let skips = {
                    let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                    let before = skipped();
                    super::read_registration(&one).unwrap();
                    let _ = super::read_registration(&two);
                    skipped() - before
                };
                assert_eq!(skips, 0, "{name}: {case}");
                mend();
                assert_eq!(
                    validate_in_one_scope(&[&one, &two]),
                    1,
                    "{name}: {case} mended"
                );
            }
        }
        let config = common.join("config");
        let text = std::fs::read(&config).unwrap();
        for added in [
            &b"[Include]\n\tPath = other\n"[..],
            b"[INCLUDEIF \"gitdir:/nowhere/\"]\n\tpath = other\n",
            b"[includeIf \"gitdir:/no\xffwhere/\"]\n\tpath = other\n",
            b"[Core]\n\tWorkTree = /elsewhere\n",
            b"[CORE]\n\tBARE = TRUE\n",
        ] {
            let mut edited = text.clone();
            edited.extend_from_slice(added);
            std::fs::write(&config, &edited).unwrap();
            assert!(
                !super::shared_config_is_sibling_independent(&common),
                "{}",
                added.escape_ascii()
            );
            std::fs::write(&config, &text).unwrap();
        }
        assert!(super::shared_config_is_sibling_independent(&common));
    }

    /// TSK-1275 AC-6: a change between two guard evaluations is judged by
    /// the second one even when a skip was allowed in the first: a
    /// same-length config edit with its time restored, a sibling-specific
    /// `config.worktree`, and a broken registration.
    #[cfg(unix)]
    #[test]
    fn a_skip_never_carries_a_verdict_into_the_next_evaluation() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return;
        }
        assert_eq!(validate_in_one_scope(&[&one, &two]), 1);
        // A same-length edit that adds an include, with the time restored.
        let config = common.join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        let stamp = std::fs::metadata(&config).unwrap().modified().unwrap();
        let head = "[core]\n\trepositoryformatversion = 0";
        assert!(text.starts_with(head), "{text}");
        let mut swap = String::from("[include]\n\tpath = ");
        while swap.len() < head.len() {
            swap.push('z');
        }
        let edited = text.replacen(head, &swap, 1);
        assert_eq!(edited.len(), text.len());
        std::fs::write(&config, &edited).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&config)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
        assert_eq!(validate_in_one_scope(&[&one, &two]), 0);
        std::fs::write(&config, &text).unwrap();
        assert_eq!(validate_in_one_scope(&[&one, &two]), 1);
        // A registration whose checkout now names another administrative
        // directory is refused by the next evaluation, skip or not.
        std::fs::write(
            two_root.join(".git"),
            format!("gitdir: {}\n", one.display()),
        )
        .unwrap();
        let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
        super::read_registration(&one).unwrap();
        assert!(super::read_registration(&two).is_err());
    }

    /// TSK-1275 AC-6: an included file and a sibling's own `config.worktree`
    /// changed between two guard evaluations are judged by the second one.
    /// An unsupported repository format version is a change libgit2 refuses.
    #[cfg(unix)]
    #[test]
    fn included_and_worktree_config_changes_are_judged_by_the_next_evaluation() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, _) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        let config = common.join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        let refused = "[core]\n\trepositoryformatversion = 99\n";
        // An included file: the include keeps every full open, and the next
        // evaluation reads the included file as it is then.
        let included = common.join("included");
        std::fs::write(&included, "[user]\n\tname = x\n").unwrap();
        std::fs::write(&config, format!("{text}[include]\n\tpath = included\n")).unwrap();
        assert_eq!(validate_in_one_scope(&[&one, &two]), 0);
        std::fs::write(&included, refused).unwrap();
        {
            let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
            assert!(super::read_registration(&two).is_err());
        }
        std::fs::write(&config, &text).unwrap();
        // A sibling's own config.worktree, read with worktreeConfig enabled.
        std::fs::write(
            &config,
            format!("{text}[extensions]\n\tworktreeConfig = true\n"),
        )
        .unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        assert_eq!(validate_in_one_scope(&[&one, &two]), 1);
        std::fs::write(two.join("config.worktree"), refused).unwrap();
        let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
        super::read_registration(&one).unwrap();
        assert!(super::read_registration(&two).is_err());
    }

    /// TSK-1275 AC-6: a registration added or broken between two guard
    /// evaluations is judged by the second one; no listing crosses calls.
    #[test]
    fn a_registration_changed_between_guard_calls_is_judged_by_the_next() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(dir.path(), &["worktree", "add", "-b", "one", "one"]);
        {
            let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
            let repo = super::open(dir.path()).unwrap().unwrap();
            assert_eq!(super::registrations(&repo).unwrap().len(), 1);
            super::RepoInfo::discover(dir.path()).unwrap().unwrap();
        }
        git(dir.path(), &["worktree", "add", "-b", "two", "two"]);
        {
            let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
            let repo = super::open(dir.path()).unwrap().unwrap();
            assert_eq!(super::registrations(&repo).unwrap().len(), 2);
        }
        // A third registration whose pointer names another checkout's marker
        // is refused by the next evaluation's discovery.
        let admin = dir.path().join(".git").join("worktrees").join("rogue");
        std::fs::create_dir(&admin).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", dir.path().join("one").join(".git").display()),
        )
        .unwrap();
        let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
        let Err(refused) = super::RepoInfo::discover(dir.path()) else {
            panic!("a registration naming another checkout's marker is refused");
        };
        assert!(
            refused.contains("worktree registration"),
            "unexpected refusal: {refused}"
        );
    }

    /// TSK-1275: the registration listing is one reading per guard
    /// evaluation. A registration added after it was read is not listed again
    /// in that evaluation, which never reads the new registration's files,
    /// and the next evaluation lists it.
    #[test]
    fn a_registration_added_inside_one_evaluation_is_listed_by_the_next() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(dir.path(), &["worktree", "add", "-b", "one", "one"]);
        let scope = crate::hooks::edit_guard::RepoFactsScope::enter();
        let repo = super::open(dir.path()).unwrap().unwrap();
        assert_eq!(super::registrations(&repo).unwrap().len(), 1);
        git(dir.path(), &["worktree", "add", "-b", "two", "two"]);
        assert_eq!(super::registrations(&repo).unwrap().len(), 1);
        drop(scope);
        let _next = crate::hooks::edit_guard::RepoFactsScope::enter();
        assert_eq!(super::registrations(&repo).unwrap().len(), 2);
    }

    /// Runs `action` once, the first time the sibling gate reaches `stage`
    /// on this thread; the returned flag says whether it ran.
    #[cfg(unix)]
    fn interleave_once(
        stage: &'static str,
        action: impl FnOnce() + 'static,
    ) -> std::rc::Rc<std::cell::Cell<bool>> {
        let fired = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = fired.clone();
        let mut action = Some(action);
        super::INTERLEAVE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move |at: &str| {
                if at != stage {
                    return;
                }
                if let Some(action) = action.take() {
                    action();
                    seen.set(true);
                }
            }));
        });
        fired
    }

    /// TSK-1275: a listed registration that links to an administrative
    /// directory elsewhere under the common directory is refused by the full
    /// open, because its registration directory is not `<common>/worktrees`.
    /// After another sibling has proven the common directory in the same
    /// evaluation it is still refused and never skips, whichever way its
    /// gitfile names the administrative directory.
    #[cfg(unix)]
    #[test]
    fn a_registration_linked_elsewhere_under_the_common_directory_stays_refused() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (listed, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        assert_eq!(validate_in_one_scope(&[&one, &listed]), 1);
        // Git's own `commondir` text, `../..`, still names the common
        // directory from the moved administrative directory.
        let moved = common.join("other").join("two");
        std::fs::create_dir(common.join("other")).unwrap();
        std::fs::rename(&listed, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &listed).unwrap();
        assert_eq!(
            std::fs::read(moved.join("commondir"))
                .unwrap()
                .trim_ascii_end(),
            b"../.."
        );
        for named in [&listed, &moved] {
            std::fs::write(
                two_root.join(".git"),
                format!("gitdir: {}\n", named.display()),
            )
            .unwrap();
            assert!(
                super::sibling_facts(&listed, &two_root).is_none(),
                "{}",
                named.display()
            );
            // Outside an evaluation every sibling is opened, as on main.
            let Err(refused) = super::read_registration(&listed) else {
                panic!("the full open refuses a registration outside <common>/worktrees");
            };
            assert!(
                refused.contains("common directory disagrees"),
                "unexpected refusal: {refused}"
            );
            let (skips, inside) = {
                let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                let before = skipped();
                super::read_registration(&one).unwrap();
                let inside = super::read_registration(&listed);
                (skipped() - before, inside)
            };
            assert_eq!(skips, 0, "{}", named.display());
            assert_eq!(inside.err(), Some(refused), "{}", named.display());
        }
    }

    /// TSK-1275 (AC-3, AC-6): the shared configuration replaced inside one
    /// guard evaluation, right after the gate's full open or right after the
    /// gate proved it, is read by every sibling validated afterwards, as
    /// main's own open of each sibling reads it. A format version libgit2
    /// refuses is refused by discovery at either point.
    #[cfg(unix)]
    #[test]
    fn a_shared_config_replaced_inside_one_evaluation_is_read_by_later_siblings() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, _) = sibling(dir.path(), "two");
        let (three, _) = sibling(dir.path(), "three");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        // The skip is live here: two of three siblings skip.
        assert_eq!(validate_in_one_scope(&[&one, &two, &three]), 2);
        let config = common.join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        let refused = text.replacen(
            "repositoryformatversion = 0",
            "repositoryformatversion = 99",
            1,
        );
        assert_ne!(refused, text);
        // Main's answer once the change is in place: each sibling refuses.
        std::fs::write(&config, &refused).unwrap();
        assert!(super::read_registration(&two).is_err());
        for stage in ["after-gate-open", "after-gate-judged"] {
            std::fs::write(&config, &text).unwrap();
            let (path, replacement) = (config.clone(), refused.clone());
            let fired =
                interleave_once(stage, move || std::fs::write(&path, &replacement).unwrap());
            let discovered = {
                let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                super::RepoInfo::discover(dir.path())
            };
            super::INTERLEAVE.with(|slot| *slot.borrow_mut() = None);
            assert!(fired.get(), "{stage}: the gate's full open ran");
            assert!(
                discovered.is_err(),
                "{stage}: a sibling validated after the change refuses it"
            );
        }
        std::fs::write(&config, &text).unwrap();
    }

    /// TSK-1275 (AC-3, AC-6): a sibling's registration files replaced after
    /// another sibling proved the common directory, inside the same
    /// evaluation, are read afresh. The replaced sibling never skips, and its
    /// full open refuses it with main's error.
    #[cfg(unix)]
    #[test]
    fn a_registration_replaced_after_the_gate_is_proven_is_read_afresh() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, two_root) = sibling(dir.path(), "two");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        let foreign = dir.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        init_repo(&foreign);
        let foreign_git = foreign.join(".git").canonicalize().unwrap();
        let marker = two_root.join(".git");
        let gitfile = std::fs::read(&marker).unwrap();
        let commondir = two.join("commondir");
        let written = std::fs::read(&commondir).unwrap();
        let replaced: [Case; 2] = [
            (
                "commondir naming another repository",
                Box::new(|| {
                    std::fs::write(&commondir, format!("{}\n", foreign_git.display())).unwrap();
                }),
                Box::new(|| std::fs::write(&commondir, &written).unwrap()),
            ),
            (
                "gitfile naming another sibling's registration",
                Box::new(|| {
                    std::fs::write(&marker, format!("gitdir: {}\n", one.display())).unwrap();
                }),
                Box::new(|| std::fs::write(&marker, &gitfile).unwrap()),
            ),
        ];
        for (case, replace, mend) in &replaced {
            replace();
            let main = super::read_registration(&two);
            mend();
            let (skips, inside) = {
                let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
                let before = skipped();
                super::read_registration(&one).unwrap();
                replace();
                let inside = super::read_registration(&two);
                (skipped() - before, inside)
            };
            mend();
            assert!(main.is_err(), "{case}");
            assert_eq!(skips, 0, "{case}");
            assert_eq!(inside.err(), main.err(), "{case}");
            assert_eq!(validate_in_one_scope(&[&one, &two]), 1, "{case} mended");
        }
    }

    /// TSK-1275: shared files changed within the settling margin are not
    /// relied on, so every sibling keeps its full open; once they have
    /// settled, the next evaluation skips again. A benign change between the
    /// gate's open and its judgement also keeps every full open.
    #[cfg(unix)]
    #[test]
    fn shared_files_are_relied_on_only_once_settled_and_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let (one, _) = sibling(dir.path(), "one");
        let (two, _) = sibling(dir.path(), "two");
        let (three, _) = sibling(dir.path(), "three");
        let common = dir.path().join(".git").canonicalize().unwrap();
        if !super::shared_config_is_sibling_independent(&common) {
            return; // this host's global or system configuration already decides
        }
        let config = common.join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        let (path, replacement) = (config.clone(), format!("{text}[user]\n\tname = x\n"));
        let fired = interleave_once("after-gate-open", move || {
            std::fs::write(&path, &replacement).unwrap();
        });
        assert_eq!(validate_in_one_scope(&[&one, &two, &three]), 0);
        super::INTERLEAVE.with(|slot| *slot.borrow_mut() = None);
        assert!(fired.get());
        assert_eq!(validate_in_one_scope(&[&one, &two, &three]), 2);
        super::REQUIRE_SETTLED.with(|require| require.set(true));
        std::fs::write(&config, &text).unwrap();
        assert_eq!(validate_in_one_scope(&[&one, &two, &three]), 0);
        std::thread::sleep(std::time::Duration::from_secs(
            u64::try_from(super::SETTLED_MARGIN_SECS).unwrap() + 1,
        ));
        let settled = validate_in_one_scope(&[&one, &two, &three]);
        super::REQUIRE_SETTLED.with(|require| require.set(false));
        assert_eq!(settled, 2);
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
    /// The first lookup opens both and lists the session's owner once; later
    /// ordinary targets reuse both readings and list both owners, since a
    /// kept reading never removes one. 16 targets in the worktree open the
    /// two owners once each, whether the session is the main checkout or the
    /// worktree (reviews 02 and 03 of PR 386).
    #[test]
    fn a_linked_worktree_and_its_main_checkout_are_opened_once() {
        let (_dir, root) = repository();
        let repo = git2::Repository::open(&root).unwrap();
        let linked = root.join("w");
        repo.worktree("w", &linked, None).unwrap();
        for (session, other) in [
            (root.clone(), linked.clone()),
            (linked.clone(), root.clone()),
        ] {
            let _scope = RepoFactsScope::enter();
            reset();
            for word in 0..16 {
                let target = linked.join(format!("d{word}/x"));
                let expected = if word == 0 {
                    vec![session.clone()]
                } else {
                    vec![session.clone(), other.clone()]
                };
                assert_eq!(
                    super::target_roots(&target, &session),
                    Ok(expected),
                    "{word}"
                );
            }
            assert_eq!(opens(), 2, "each owner is opened once: {session:?}");
        }
    }

    /// Whichever owner a scope opens first, each is opened once: a lookup
    /// that finds a kept reading and a fresh one sharing a common directory
    /// lists both rather than reading them again (review 03 of PR 386).
    #[test]
    fn a_kept_reading_and_a_fresh_one_list_both_owners() {
        let (_dir, root) = repository();
        let repo = git2::Repository::open(&root).unwrap();
        let linked = root.join("w");
        repo.worktree("w", &linked, None).unwrap();
        let both = vec![root.clone(), linked.clone()];
        let _scope = RepoFactsScope::enter();
        reset();
        let main = super::target_roots(&root.join("a/x"), &root);
        assert_eq!(main, Ok(vec![root.clone()]));
        assert_eq!(opens(), 1, "the main checkout alone");
        let first = super::target_roots(&linked.join("b/x"), &root);
        assert_eq!(first, Ok(both.clone()));
        assert_eq!(opens(), 2, "the worktree once more");
        let later = super::target_roots(&linked.join("c/x"), &root);
        assert_eq!(later, Ok(both));
        assert_eq!(opens(), 2, "both readings are kept");
    }

    /// An owner whose metadata cannot be read has no marker, so its reading
    /// is never reused; one whose `.git` and `HEAD` are proven absent has
    /// one (review 03 of PR 386).
    #[cfg(unix)]
    #[test]
    fn an_owner_whose_metadata_cannot_be_read_has_no_marker() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // A superuser reads through the mode, so there is nothing to test.
        let enforced = std::fs::read_dir(&locked).is_err();
        let unreadable = super::marker(&locked.join("r"));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if enforced {
            assert!(unreadable.is_none());
        }
        assert!(super::marker(dir.path()).is_some());
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

    /// A repository with one commit at `path`.
    fn committed(path: &Path) -> git2::Repository {
        let repo = git2::Repository::init(path).unwrap();
        {
            let tree = repo.treebuilder(None).unwrap().write().unwrap();
            let tree = repo.find_tree(tree).unwrap();
            let sig = git2::Signature::now("t", "t@example.test").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "one", &tree, &[])
                .unwrap();
        }
        repo
    }

    /// A main checkout `r` with a linked worktree `r/w`, and an independent
    /// repository `c` beside it. `c` has a linked worktree `d` inside
    /// `r/w/b`, so its enforcement files lie there, and a registration `x`
    /// that names `r/w` as its checkout, so `r/w/.git` can be pointed at
    /// `c` with text of the same length.
    fn retarget_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = crate::portable_path::canonicalize(dir.path()).unwrap();
        let (root, other) = (base.join("r"), base.join("c"));
        let main = committed(&root);
        let independent = committed(&other);
        let linked = root.join("w");
        main.worktree("w", &linked, None).unwrap();
        std::fs::create_dir_all(linked.join("b")).unwrap();
        independent
            .worktree("d", &linked.join("b/d"), None)
            .unwrap();
        independent.worktree("x", &base.join("x"), None).unwrap();
        let registration = std::fs::read(root.join(".git/worktrees/w/gitdir")).unwrap();
        std::fs::write(other.join(".git/worktrees/x/gitdir"), registration).unwrap();
        (dir, root, linked)
    }

    /// Point `linked/.git` at `c`'s registration `x` in place, with text of
    /// the same length and the old modification time, so its inode, length,
    /// type and time are all unchanged, and drop the main checkout's own
    /// registration of it, as moving a worktree between repositories does.
    /// The main checkout's `.git` entries are unchanged.
    fn retarget(linked: &Path) {
        use std::io::Write;
        let gitfile = linked.join(".git");
        let before = std::fs::read_to_string(&gitfile).unwrap();
        let after = before
            .replace("/r/.git/worktrees/w", "/c/.git/worktrees/x")
            .replace("\\r\\.git\\worktrees\\w", "\\c\\.git\\worktrees\\x");
        assert_ne!(before, after, "{before}");
        assert_eq!(before.len(), after.len());
        let modified = std::fs::metadata(&gitfile).unwrap().modified().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&gitfile)
            .unwrap();
        file.write_all(after.as_bytes()).unwrap();
        file.set_modified(modified).unwrap();
        let root = linked.parent().unwrap();
        std::fs::remove_dir_all(root.join(".git/worktrees/w")).unwrap();
    }

    type Admission = (
        Result<bool, String>,
        Result<bool, String>,
        Result<Option<(bool, PathBuf)>, String>,
    );

    /// What the three edit-guard readers that walk a target's owners say
    /// about `target`: whether it is an enforcement target, whether it holds
    /// enforcement files, and the checkout a recursive delete would remove.
    fn admission(target: &Path, root: &Path) -> Admission {
        use crate::hooks::edit_guard::{self, Checkout};
        (
            edit_guard::repository_enforcement_target(target, root, true),
            edit_guard::holds_enforcement_files(target, root),
            edit_guard::registered_checkout_under(target, root, None).map(|found| {
                found.map(|checkout| match checkout {
                    Checkout::Registered(path) => (true, path),
                    Checkout::Hidden(path) => (false, path),
                })
            }),
        )
    }

    /// A worktree pointed at an independent repository during the scope,
    /// with every metadata field the owner fingerprint compares unchanged,
    /// keeps its owner: an unseen target inside it gets the owners and the
    /// admission results a fresh reading gives (Codex review 03 of PR 386).
    #[test]
    fn a_worktree_retargeted_with_unchanged_metadata_keeps_its_owner() {
        for session_is_main in [true, false] {
            let (_dir, root, linked) = retarget_fixture();
            let session = if session_is_main {
                root.clone()
            } else {
                linked.clone()
            };
            let target = linked.join("b");
            let scope = RepoFactsScope::enter();
            assert_eq!(
                super::target_roots(&linked.join("a/x"), &session),
                Ok(vec![session.clone()]),
                "both owners read afresh share one common directory"
            );
            let fingerprint = super::marker(&linked);
            assert!(fingerprint.is_some());
            retarget(&linked);
            assert!(
                super::marker(&linked) == fingerprint,
                "the retarget keeps every compared metadata field"
            );
            let owners = super::target_roots(&target.join("y"), &session);
            let admitted = admission(&target, &session);
            drop(scope);
            let fresh_owners = super::target_roots(&target.join("y"), &session);
            let mut both = vec![root.clone(), linked.clone()];
            if !session_is_main {
                both.reverse();
            }
            assert_eq!(fresh_owners, Ok(both), "two repositories now");
            assert_eq!(owners, fresh_owners, "session is main: {session_is_main}");
            let fresh_admitted = admission(&target, &session);
            assert_eq!(fresh_admitted.1, Ok(true), "c's worktree d lies in w/b");
            assert_eq!(
                admitted, fresh_admitted,
                "session is main: {session_is_main}"
            );
        }
    }
}
