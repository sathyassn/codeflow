//! Release lines judged where each change was introduced (SPC-013 R-120).
//!
//! A release branch combines several epic lines. Its range is judged commit
//! by commit along the first-parent path from the merge-base, never by
//! topology or a declaration the range carries:
//!
//! - **Scope** comes from the branch name. A branch is a release branch when
//!   its name matches `git.release_branch_pattern` in the policy at the
//!   default target's tip at the destination ([`BUILTIN_PATTERN`] when the
//!   key is absent), and a range is a release range when its head or its
//!   target is one. The default target and epic lines never are.
//! - **Imports.** A merge whose non-first parents all lie on the
//!   first-parent chain of a verified epic line, or of the default target,
//!   as the destination advertises them, is an import. A path it brings is
//!   one whose full tree entry equals the expected import's (the clean
//!   three-way result, or the second parent's entry for a conflicted path);
//!   every other path is a resolution, a direct change judged against the
//!   expected import. A brought completion binds where its block was
//!   introduced on its line (R-60); a brought criteria change is judged
//!   again at the commit that landed it on its line, which must be
//!   planning-only, unless that landing is at or before the line's
//!   release-rule cutoff ([`BASELINE_KEY`] in project config at the default
//!   target's tip), counted along the line's verified first-parent chain:
//!   such a legacy change is listed as information.
//! - **Direct work** is everything else. Every direct change freezes
//!   criteria. A direct commit that touches only planning records needs no
//!   release-integration completion; anything else (code, a resolution, a
//!   merge that is no import) belongs to the one open task with
//!   `role: release-integration`, whose completion inside the range binds
//!   to the head under R-60's shared binding rule. With no such task the finding is
//!   [`NO_OWNER`]. A completion of any other task made directly on the line
//!   binds to the head as a task pull request's does.
//!
//! The judge proves structure and binding only (see
//! [`super::acceptance::SCOPE_NOTE`]); whether a planning-only landing on a
//! line passed that line's review stays with the host's checks (R-80).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use git2::{Oid, Repository};

use super::acceptance::{
    active_block, bind_completion, blob_at, commit_of, finding, introduced_at, is_clean_remerge,
    Finding, Landing, Transport, BINDING_RULE, FROZEN_RULE,
};
use super::classify::is_planning_path;
use super::lifecycle::{Brought, Graph, RecordView};
use super::work_start::{record_kind_for_tree_path, RecordKind};

/// The `role` value of the task that owns direct release work.
pub const RELEASE_ROLE: &str = "release-integration";
/// The release branch pattern when the default target's policy sets none.
pub const BUILTIN_PATTERN: &str = "integration/release-*";
/// The finding for direct code on a release range that no task owns.
pub const NO_OWNER: &str = "no release-integration task to own it";
/// The project-config table mapping each epic line to its release-rule
/// cutoff commit (SPC-013 R-120, planning resolution 28).
pub const BASELINE_KEY: &str = "release_rule_baseline";
/// The project-config table mapping each epic line to its records cutoff:
/// a complete task brought from the line without an acceptance block, whose
/// record last changed there at or before the cutoff, is listed as
/// information (SPC-013 R-120, planning resolution 29).
pub const RECORDS_BASELINE_KEY: &str = "release_records_baseline";
/// The project-config key marking where a project adopted R-120, with its
/// one value `1` (SPC-013 R-120, planning resolution 29). It fixes only the
/// point the transition tables stop at; R-120 is enforced whatever it says
/// or whether it is there. Once the default target carries it, removing it
/// or changing its value refuses every release check.
pub const MARKER_KEY: &str = "release_rules";
/// `CodeFlow`'s approved release-rule cutoffs, one per epic line, each that
/// line's tip on 2026-09-27 (`docs/verification/release-rule-cutoffs-2026-09-27.md`).
/// The transition tables bridge only `CodeFlow`'s own 2.x to 3.0 history
/// (operator ruling, 2026-09-28: no effect on consuming projects), so an
/// entry of either table naming anything else refuses every release check.
/// A repository can meet them only by holding `CodeFlow`'s own line history,
/// and then they cover only `CodeFlow`'s own landings at or before them.
const APPROVED_CUTOFFS: [(&str, &str); 5] = [
    (
        "integration/EPC-014-public-docs",
        "d618075e229d49f706cf1c0e9ab5b10a3c9c69e3",
    ),
    (
        "integration/EPC-015-engineering-bar",
        "db55fc01c30f75d0eb1bd5f3df1de9dd8f9d1bff",
    ),
    (
        "integration/EPC-016-visual-guide",
        "51ee9374b506d9a359150ac15663c26c914b57cf",
    ),
    (
        "integration/EPC-018-autonomy-roster",
        "ccd56fa85160ac58d66828933e96c000357c4baa",
    ),
    (
        "integration/EPC-020-delivery-system",
        "2921df9f52a5e787f896146233a85201720591d3",
    ),
];
/// Every epic line's name starts with this.
const EPIC_PREFIX: &str = "integration/EPC-";

/// Why `pattern` cannot name release branches, if it cannot: it is not a
/// glob, or it could match an epic line (`integration/EPC-*`) or one of the
/// literal `protected` branches, the default target among them.
///
/// The epic test is conservative: a name the pattern matches starts with
/// the pattern's literal prefix, so when that prefix and `integration/EPC-`
/// diverge no epic line can match. When they agree the pattern is refused,
/// even if a later literal part would exclude every epic line.
#[must_use]
pub fn pattern_problem(pattern: &str, protected: &[String]) -> Option<String> {
    if pattern.trim().is_empty() {
        return Some("a release branch pattern cannot be empty".to_string());
    }
    let parsed = match glob::Pattern::new(pattern) {
        Ok(parsed) => parsed,
        Err(error) => return Some(format!("not a valid glob ({error})")),
    };
    let prefix = pattern.split(['*', '?', '[']).next().unwrap_or_default();
    if EPIC_PREFIX.starts_with(prefix) || prefix.starts_with(EPIC_PREFIX) {
        return Some(format!(
            "it could match an epic line ({EPIC_PREFIX}*), and epic lines keep their own rules"
        ));
    }
    protected
        .iter()
        .filter(|branch| !branch.contains(['*', '?', '[']))
        .find(|branch| parsed.matches(branch))
        .map(|branch| {
            format!("it matches the protected branch '{branch}'; the default target is never a release branch")
        })
}

/// What the destination holds, as it advertises it now.
#[derive(Debug, Clone, Default)]
pub struct Destination {
    /// Where it was asked: a URL or path. `None` when there is no
    /// destination (a local run with no `origin`).
    pub url: Option<String>,
    /// The default target's name and tip; `None` for a destination with no
    /// branch yet.
    pub default: Option<(String, Oid)>,
    /// Every advertised branch (name without `refs/heads/`) and its tip.
    pub heads: Vec<(String, Oid)>,
}

/// Ask `url` for its default branch and its branches. With no `url` the
/// destination is empty, and the built-in pattern applies.
///
/// # Errors
///
/// Returns why the destination cannot be read: it cannot be asked, or it
/// has branches but names no default branch that exists.
pub fn ask_destination(repo_root: &Path, url: Option<&str>) -> Result<Destination, String> {
    ask(repo_root, url).and_then(|answer| answer)
}

/// As [`ask_destination`], keeping apart a destination that did not answer
/// (the outer error) from one that answered with no usable default target
/// (the inner error).
///
/// # Errors
///
/// Returns why the destination could not be asked.
pub fn ask(repo_root: &Path, url: Option<&str>) -> Result<Result<Destination, String>, String> {
    let Some(url) = url else {
        return Ok(Ok(Destination::default()));
    };
    Ok(from_advertisement(url, &advertisement(repo_root, url)?))
}

/// What `url` advertises: its default branch, branches and tags (peeled),
/// in one bounded `git ls-remote --symref`. A caller that already holds
/// it, such as the pre-push hook, passes it on instead of asking again.
///
/// # Errors
///
/// Returns why the destination could not be asked.
pub fn advertisement(repo_root: &Path, url: &str) -> Result<String, String> {
    crate::git::remote_query::ls_remote(
        repo_root,
        &["--symref", url, "HEAD", "refs/heads/*", "refs/tags/*"],
    )
    .map_err(|why| format!("asking the destination {url} for its default branch: {why}"))
}

/// The destination `url` is, read from its [`advertisement`].
///
/// # Errors
///
/// Returns why the answer names no usable default target: it has branches
/// but its HEAD names none it has.
pub fn from_advertisement(url: &str, listed: &str) -> Result<Destination, String> {
    let mut symref = None;
    let mut heads = Vec::new();
    for line in listed.lines() {
        let Some((left, name)) = line.split_once('\t') else {
            continue;
        };
        if let Some(target) = left.strip_prefix("ref: ") {
            if name == "HEAD" {
                symref = target.strip_prefix("refs/heads/").map(str::to_string);
            }
            continue;
        }
        if let (Some(branch), Ok(oid)) = (name.strip_prefix("refs/heads/"), Oid::from_str(left)) {
            heads.push((branch.to_string(), oid));
        }
    }
    let default = match symref {
        Some(name) => match heads.iter().find(|(branch, _)| *branch == name) {
            Some((_, tip)) => Some((name, *tip)),
            None if heads.is_empty() => None,
            None => {
                return Err(format!(
                    "the destination {url} names '{name}' as its default branch, which it does not have; point its HEAD at the default branch (in a bare repository: `git symbolic-ref HEAD refs/heads/<branch>`)"
                ))
            }
        },
        None if heads.is_empty() => None,
        None => {
            return Err(format!(
                "the destination {url} does not name a default branch it has; point its HEAD at the default branch (in a bare repository: `git symbolic-ref HEAD refs/heads/<branch>`)"
            ))
        }
    };
    Ok(Destination {
        url: Some(url.to_string()),
        default,
        heads,
    })
}

/// Whether durable work tracking is on at the destination's default
/// target, read at its tip, which is fetched when this clone lacks it.
/// `false` only for a destination with no default target yet, or one
/// whose tip is readable and does not track durable work.
///
/// # Errors
///
/// Returns why the state cannot be read: the tip cannot be fetched, or
/// the project state there does not parse. A caller refuses then; an
/// unreadable state never counts as off.
pub fn default_tracks_work(repo_root: &Path, destination: &Destination) -> Result<bool, String> {
    let Some((name, tip)) = &destination.default else {
        return Ok(false);
    };
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    ensure_objects(repo_root, &repo, destination, &[(name.as_str(), *tip)])?;
    crate::workgraph::durable_work_tracking_enabled_at(repo_root, &tip.to_string())
        .map_err(|why| format!("durable work tracking at the default target {name}: {why}"))
}

/// Whether a range is a release range, and under which pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The head branch is a release branch.
    pub head: bool,
    /// The pull request's target branch is a release branch.
    pub into: bool,
    pub pattern: String,
    /// Where the pattern was read: the default target's tip, or the built-in.
    pub source: String,
}

impl Scope {
    /// Whether the range is a release range: its head or its target is a
    /// release branch.
    #[must_use]
    pub fn release(&self) -> bool {
        self.head || self.into
    }
}

/// The scope of a range whose head branch is `head` and, for a pull
/// request, whose target is `into`, under the policy at the destination's
/// default target.
///
/// # Errors
///
/// Returns why the default target's policy cannot be read: its tip is not
/// here and cannot be fetched, or the policy there is unreadable or names
/// an invalid pattern. The check fails closed; it never falls back to the
/// ordinary rules.
pub fn scope(
    repo_root: &Path,
    destination: &Destination,
    head: &str,
    into: Option<&str>,
) -> Result<Scope, String> {
    let (pattern, source) = match &destination.default {
        None => (
            BUILTIN_PATTERN.to_string(),
            "built in (the destination has no default target yet)".to_string(),
        ),
        Some((name, tip)) => {
            let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
            ensure_objects(repo_root, &repo, destination, &[(name.as_str(), *tip)])?;
            let pattern = pattern_at(&repo, *tip, name)
                .map_err(|why| format!("the policy at the default target {name} ({tip}): {why}"))?;
            let short = &tip.to_string()[..9];
            match pattern {
                Some(pattern) => (pattern, format!("{name} at {short}")),
                None => (
                    BUILTIN_PATTERN.to_string(),
                    format!("built in (no release_branch_pattern at {name} {short})"),
                ),
            }
        }
    };
    let glob = glob::Pattern::new(&pattern).map_err(|error| error.to_string())?;
    let default = destination.default.as_ref().map(|(name, _)| name.as_str());
    let matches = |name: &str| {
        !name.is_empty()
            && !name.starts_with(EPIC_PREFIX)
            && Some(name) != default
            && glob.matches(name)
    };
    Ok(Scope {
        head: matches(head),
        into: into.is_some_and(matches),
        pattern,
        source,
    })
}

/// The scope of the checked-out branch, whose pull request goes `into` a
/// target (for a completion, the task's declared integration target), under
/// the policy at `origin`'s default target: what `task status complete`
/// judges a completion made here under, with the context CI has.
///
/// # Errors
///
/// As [`scope`], and when `origin` cannot be asked.
pub fn checkout_scope(repo_root: &Path, into: Option<&str>) -> Result<Scope, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let branch = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().ok().map(str::to_string))
        .unwrap_or_default();
    let url = repo
        .find_remote("origin")
        .ok()
        .and_then(|remote| remote.url().ok().map(str::to_string));
    let destination = ask_destination(repo_root, url.as_deref())?;
    scope(repo_root, &destination, &branch, into)
}

/// The merge a pull request from `head` into `base` would create, written
/// to the object store only (no ref moves): a pull request into a release
/// branch is judged as that merge (SPC-013 R-120).
///
/// # Errors
///
/// Returns why it cannot be made: a revision does not resolve, or the merge
/// conflicts (naming the paths), which the branch resolves first.
pub fn pull_request_merge(repo_root: &Path, base: &str, head: &str) -> Result<Oid, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let (ours, theirs) = (commit(base)?, commit(head)?);
    let error = |error: git2::Error| error.message().to_string();
    let mut index = repo.merge_commits(&ours, &theirs, None).map_err(error)?;
    if index.has_conflicts() {
        let paths: BTreeSet<String> = index
            .conflicts()
            .map_err(error)?
            .filter_map(Result::ok)
            .filter_map(|conflict| conflict.our.or(conflict.their))
            .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
            .collect();
        return Err(format!(
            "the merge this pull request would create conflicts in {}; bring the target into the branch first",
            paths.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let tree = index.write_tree_to(&repo).map_err(error)?;
    let tree = repo.find_tree(tree).map_err(error)?;
    let signature = git2::Signature::new("codeflow", "codeflow@localhost", &git2::Time::new(0, 0))
        .map_err(error)?;
    repo.commit(
        None,
        &signature,
        &signature,
        "the merge this pull request would create",
        &tree,
        &[&ours, &theirs],
    )
    .map_err(error)
}

/// The release pattern the policy at `tip` sets: `None` when the policy
/// file has no such key. A missing policy file is an error, since an
/// existing default target without one cannot name its release branches.
fn pattern_at(repo: &Repository, tip: Oid, default: &str) -> Result<Option<String>, String> {
    let tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.message().to_string())?;
    // Only a destination with no default target yet may use the built-in
    // pattern without a policy (R-120); an existing target whose policy
    // file is missing cannot say what its release branches are, so that is
    // an error. A policy file without the key selects the built-in pattern.
    let entry = tree
        .get_path(Path::new(".codeflow/policy.json"))
        .map_err(|_| ".codeflow/policy.json is missing".to_string())?;
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| format!(".codeflow/policy.json cannot be read: {}", error.message()))?;
    let value: serde_json::Value = serde_json::from_slice(blob.content())
        .map_err(|error| format!(".codeflow/policy.json is not valid JSON: {error}"))?;
    let git = value.get("git");
    let Some(raw) = git.and_then(|git| git.get("release_branch_pattern")) else {
        return Ok(None);
    };
    let pattern = raw
        .as_str()
        .ok_or("git.release_branch_pattern is not a string")?
        .to_string();
    let mut protected: Vec<String> = git
        .and_then(|git| git.get("protected_branches"))
        .and_then(serde_json::Value::as_array)
        .map_or_else(
            || {
                crate::hooks::policy::Policy::default()
                    .git
                    .protected_branches
            },
            |items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            },
        );
    // The actual default target is never a release branch, protected or not.
    protected.push(default.to_string());
    match pattern_problem(&pattern, &protected) {
        Some(problem) => Err(format!(
            "git.release_branch_pattern '{pattern}' is refused: {problem}"
        )),
        None => Ok(Some(pattern)),
    }
}

/// The tip the destination advertises for `branch`, made available here:
/// fetched into the object store when this clone lacks it, no ref moved.
///
/// # Errors
///
/// Returns why the tip is not here: the destination does not advertise
/// `branch`, or its tip is missing and cannot be fetched.
pub fn advertised_tip_here(
    repo_root: &Path,
    destination: &Destination,
    branch: &str,
) -> Result<Oid, String> {
    let tip = destination
        .heads
        .iter()
        .chain(destination.default.iter())
        .find(|(name, _)| name == branch)
        .map(|(_, tip)| *tip)
        .ok_or_else(|| format!("the destination advertises no branch '{branch}'"))?;
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    ensure_objects(repo_root, &repo, destination, &[(branch, tip)])?;
    Ok(tip)
}

/// Make sure every `(branch, tip)` commit is here, fetching the missing
/// ones from the destination into the object store.
fn ensure_objects(
    repo_root: &Path,
    repo: &Repository,
    destination: &Destination,
    wanted: &[(&str, Oid)],
) -> Result<(), String> {
    let missing = |wanted: &[(&str, Oid)]| -> Vec<(String, Oid)> {
        wanted
            .iter()
            .filter(|(_, tip)| repo.find_commit(*tip).is_err())
            .map(|(name, tip)| ((*name).to_string(), *tip))
            .collect()
    };
    let absent = missing(wanted);
    if absent.is_empty() {
        return Ok(());
    }
    let refs: Vec<String> = absent
        .iter()
        .map(|(name, _)| format!("refs/heads/{name}"))
        .collect();
    let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
    let fetched = destination.url.as_deref().map_or_else(
        || Err("no destination to fetch from".to_string()),
        |url| crate::git::remote_query::fetch_objects(repo_root, url, &refs),
    );
    let still = missing(wanted);
    if still.is_empty() {
        return Ok(());
    }
    let named: Vec<String> = still
        .iter()
        .map(|(name, tip)| format!("{name} at {tip}"))
        .collect();
    Err(format!(
        "the judge needs {}, which is not in this clone and could not be fetched{}",
        named.join(", "),
        fetched
            .err()
            .map(|why| format!(" ({why})"))
            .unwrap_or_default()
    ))
}

/// One path's full tree entry: object id and mode (the mode carries the
/// entry type).
type Entry = (Oid, u32);

/// Every entry of a tree by path, recursively.
fn tree_entries(tree: &git2::Tree<'_>) -> Result<BTreeMap<String, Entry>, String> {
    let mut entries = BTreeMap::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(git2::ObjectType::Tree) {
            return git2::TreeWalkResult::Ok;
        }
        let name = String::from_utf8_lossy(entry.name_bytes());
        #[allow(clippy::cast_sign_loss)] // git modes are small positive octal values
        let mode = entry.filemode() as u32;
        entries.insert(format!("{root}{name}"), (entry.id(), mode));
        git2::TreeWalkResult::Ok
    })
    .map_err(|error| format!("tree {}: {}", tree.id(), error.message()))?;
    Ok(entries)
}

fn commit_entries(repo: &Repository, commit: Oid) -> Result<BTreeMap<String, Entry>, String> {
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("{commit}: {}", error.message()))?;
    tree_entries(&tree)
}

/// Each path a commit changes against `from` (none for a root commit),
/// with its entry before and after; a type change is one path.
type Changes = BTreeMap<String, (Option<Entry>, Option<Entry>)>;

fn changes(repo: &Repository, from: Option<Oid>, to: Oid) -> Result<Changes, String> {
    let error = |error: git2::Error| format!("{to}: {}", error.message());
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let after = tree(to).map_err(error)?;
    let before = from.map(tree).transpose().map_err(error)?;
    let mut options = git2::DiffOptions::new();
    options.include_typechange(true);
    let diff = repo
        .diff_tree_to_tree(before.as_ref(), Some(&after), Some(&mut options))
        .map_err(error)?;
    let side = |file: git2::DiffFile<'_>| {
        (!file.id().is_zero()).then(|| (file.id(), u32::from(file.mode())))
    };
    Ok(diff
        .deltas()
        .filter_map(|delta| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())?;
            Some((
                path.to_string_lossy().replace('\\', "/"),
                (side(delta.old_file()), side(delta.new_file())),
            ))
        })
        .collect())
}

/// The expected import of `merge`: the clean merge of its parents, taking
/// the incoming parent's entry for a path that conflicts. As git merges an
/// octopus, each later parent merges into the result so far from its merge
/// base with every parent merged before it, so a parent that an earlier one
/// already contains adds nothing and never makes an older snapshot the
/// expected import.
fn expected_import(
    repo: &Repository,
    merge: &git2::Commit<'_>,
) -> Result<BTreeMap<String, Entry>, String> {
    let error =
        |error: git2::Error| history_error(repo, &format!("{}: {}", merge.id(), error.message()));
    let all: Vec<git2::Commit<'_>> = merge.parents().collect();
    let first = &all[0];
    let parents: Vec<&git2::Commit<'_>> = all.iter().skip(1).collect();
    let mut ours = first.tree().map_err(error)?;
    let mut merged = vec![first.id()];
    let mut result = BTreeMap::new();
    for (position, incoming) in parents.iter().enumerate() {
        let mut bases = vec![incoming.id()];
        bases.extend(merged.iter().copied());
        let ancestor = repo
            .merge_base_many(&bases)
            .and_then(|base| repo.find_commit(base))
            .and_then(|base| base.tree())
            .map_err(error)?;
        merged.push(incoming.id());
        let theirs = incoming.tree().map_err(error)?;
        let index = repo
            .merge_trees(&ancestor, &ours, &theirs, None)
            .map_err(error)?;
        let incoming_entries = tree_entries(&theirs)?;
        result = BTreeMap::new();
        let mut conflicted = BTreeSet::new();
        for entry in index.iter() {
            let path = String::from_utf8_lossy(&entry.path).into_owned();
            let stage = (entry.flags >> 12) & 0x3;
            if stage == 0 {
                result.insert(path, (entry.id, entry.mode));
            } else {
                conflicted.insert(path);
            }
        }
        for path in conflicted {
            result.remove(&path);
            if let Some(entry) = incoming_entries.get(&path) {
                result.insert(path, *entry);
            }
        }
        if position + 1 < parents.len() {
            // An octopus: the next parent merges into this result.
            let mut index = git2::Index::new().map_err(error)?;
            for (path, (id, mode)) in &result {
                let entry = git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: *mode,
                    uid: 0,
                    gid: 0,
                    file_size: 0,
                    id: *id,
                    flags: 0,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                };
                index.add(&entry).map_err(error)?;
            }
            let tree = index.write_tree_to(repo).map_err(error)?;
            ours = repo.find_tree(tree).map_err(error)?;
        }
    }
    Ok(result)
}

/// The first-parent chains of the verified epic lines and the default
/// target, as the destination advertises them, built on first use.
struct Lines<'a> {
    repo_root: &'a Path,
    repo: &'a Repository,
    destination: &'a Destination,
    /// Advertised epic lines and their tips.
    candidates: Vec<(String, Oid)>,
    chains: HashMap<Oid, HashSet<Oid>>,
    /// First-parent chains in order, oldest first.
    ordered: HashMap<Oid, Vec<Oid>>,
    /// Answers of `position`, by line tip and commit.
    positions: HashMap<(Oid, Oid), Option<usize>>,
    verified: HashMap<String, bool>,
    fetched: bool,
}

impl<'a> Lines<'a> {
    fn new(repo_root: &'a Path, repo: &'a Repository, destination: &'a Destination) -> Self {
        let candidates = destination
            .heads
            .iter()
            .filter(|(name, _)| name.starts_with(EPIC_PREFIX))
            .cloned()
            .collect();
        Self {
            repo_root,
            repo,
            destination,
            candidates,
            chains: HashMap::new(),
            ordered: HashMap::new(),
            positions: HashMap::new(),
            verified: HashMap::new(),
            fetched: false,
        }
    }

    /// Fetch the advertised tips this clone lacks, once.
    fn fetch(&mut self) -> Result<(), String> {
        if self.fetched {
            return Ok(());
        }
        self.fetched = true;
        let mut wanted: Vec<(&str, Oid)> = self
            .candidates
            .iter()
            .map(|(name, tip)| (name.as_str(), *tip))
            .collect();
        if let Some((name, tip)) = &self.destination.default {
            wanted.push((name.as_str(), *tip));
        }
        ensure_objects(self.repo_root, self.repo, self.destination, &wanted)
    }

    fn chain(&mut self, tip: Oid) -> &HashSet<Oid> {
        let repo = self.repo;
        self.chains.entry(tip).or_insert_with(|| {
            let mut chain = HashSet::new();
            let mut at = Some(tip);
            while let Some(oid) = at {
                if !chain.insert(oid) {
                    break;
                }
                at = repo
                    .find_commit(oid)
                    .ok()
                    .and_then(|commit| commit.parent_id(0).ok());
            }
            chain
        })
    }

    fn verified(&mut self, name: &str, tip: Oid) -> bool {
        if let Some(known) = self.verified.get(name) {
            return *known;
        }
        let ok = self
            .destination
            .default
            .as_ref()
            .is_some_and(|(default, default_tip)| {
                super::work_start::check_epic_line(
                    self.repo_root,
                    name,
                    default,
                    &default_tip.to_string(),
                    &tip.to_string(),
                )
                .is_ok()
            });
        self.verified.insert(name.to_string(), ok);
        ok
    }

    /// Why `cutoff` is not on the verified epic line `line`'s first-parent
    /// chain as advertised now, if it is not.
    fn off_line(&mut self, line: &str, cutoff: Oid) -> Option<String> {
        let Some(tip) = self
            .candidates
            .iter()
            .find(|(name, _)| name == line)
            .map(|(_, tip)| *tip)
        else {
            return Some(format!(
                "{line} is not an epic line the destination advertises"
            ));
        };
        if !self.verified(line, tip) {
            return Some(format!("{line} fails the epic-line check"));
        }
        if !self.chain(tip).contains(&cutoff) {
            return Some(format!(
                "{line}'s cutoff {} is not on its first-parent chain",
                short(cutoff)
            ));
        }
        None
    }

    /// Why `landing` is not `cutoff` or before it on the verified epic
    /// line `line`'s first-parent chain as advertised now, if it is not.
    /// Dates and plain ancestry never count: a topic landed after the
    /// cutoff is after it, a cutoff rewritten away covers nothing, and a
    /// landing on another line's chain is not this line's.
    fn uncovered(&mut self, line: &str, cutoff: Oid, landing: Oid) -> Option<String> {
        if let Some(why) = self.off_line(line, cutoff) {
            return Some(why);
        }
        if !self.chain(cutoff).contains(&landing) {
            return Some(format!(
                "the landing is not on {line}'s first-parent chain at or before its release-rule cutoff {}",
                short(cutoff)
            ));
        }
        None
    }

    /// Where `line`'s first-parent chain, as advertised now, first holds
    /// `commit`: the index from the oldest end of the chain of the first
    /// chain commit that is `commit` or descends from it. `None` when the
    /// line is not advertised or never holds it.
    fn position(&mut self, line: &str, commit: Oid) -> Option<usize> {
        let tip = self
            .candidates
            .iter()
            .chain(self.destination.default.iter())
            .find(|(name, _)| name == line)
            .map(|(_, tip)| *tip)?;
        if let Some(known) = self.positions.get(&(tip, commit)) {
            return *known;
        }
        let repo = self.repo;
        let order = self.ordered.entry(tip).or_insert_with(|| {
            let mut order = Vec::new();
            let mut at = Some(tip);
            while let Some(oid) = at {
                order.push(oid);
                at = repo
                    .find_commit(oid)
                    .ok()
                    .and_then(|found| found.parent_id(0).ok());
            }
            order.reverse();
            order
        });
        // A commit on the chain holds itself first; otherwise holding
        // `commit` is monotone along the chain. A walk that fails gives no
        // position, so nothing can supersede on it.
        let mut failed = false;
        let at = order
            .iter()
            .position(|chain| *chain == commit)
            .unwrap_or_else(|| {
                order.partition_point(|chain| {
                    if let Ok(descends) = repo.graph_descendant_of(*chain, commit) {
                        !descends
                    } else {
                        failed = true;
                        true
                    }
                })
            });
        let found = (!failed && at < order.len()).then_some(at);
        self.positions.insert((tip, commit), found);
        found
    }

    /// The line whose first-parent chain holds `commit`: the default
    /// target, or a verified epic line. Never the judged branch's own ref.
    fn line_of(&mut self, commit: Oid) -> Result<Option<String>, String> {
        self.fetch()?;
        if let Some((name, tip)) = self.destination.default.clone() {
            if self.chain(tip).contains(&commit) {
                return Ok(Some(name));
            }
        }
        for (name, tip) in self.candidates.clone() {
            if self.chain(tip).contains(&commit) && self.verified(&name, tip) {
                return Ok(Some(name));
            }
        }
        Ok(None)
    }
}

/// A task's brought completions in a release range. Only a completion
/// brought from the task's own line that binds where it was introduced
/// there, and that the line landed after every completion of the task
/// brought before, becomes the completion in force; an earlier completion
/// is never accepted, its findings only stop standing. Order is the
/// position on that line's first-parent chain, never parent or import
/// order.
#[derive(Default)]
struct Held {
    /// The findings of brought completions that still stand.
    findings: Vec<Finding>,
    /// The latest position of a completion brought from the task's line.
    latest: Option<usize>,
}

/// The position on the task's line `target` of the completion an import
/// `merge` brings at `path`, when it comes from that line as the line
/// stands at the newest of the merge's parents on it: the record must be
/// the one that parent holds, so an older parent of the same line cannot
/// stand in for a newer one. `None` when no parent is on the line or the
/// record is not the newest parent's.
fn own_line_position(
    repo: &Repository,
    lines: &mut Lines<'_>,
    merge: &git2::Commit<'_>,
    target: &str,
    path: &str,
    now: &RecordView,
    introduced: Oid,
) -> Result<Option<usize>, String> {
    let mut newest: Option<(usize, Oid)> = None;
    for parent in merge.parent_ids().skip(1) {
        if lines.line_of(parent)?.as_deref() != Some(target) {
            continue;
        }
        if let Some(at) = lines.position(target, parent) {
            if newest.is_none_or(|(was, _)| at > was) {
                newest = Some((at, parent));
            }
        }
    }
    let Some((_, parent)) = newest else {
        return Ok(None);
    };
    let same = record_at(repo, parent, path).is_some_and(|there| {
        there.criteria.signature() == now.criteria.signature()
            && active_block(&there) == active_block(now)
            && there.superseded_blocks() == now.superseded_blocks()
    });
    Ok(if same {
        lines.position(target, introduced)
    } else {
        None
    })
}

/// The first-parent path of a release range, oldest first, from `anchor`
/// to `head`. History the default target holds was judged there.
fn first_parent_path(
    repo: &Repository,
    head: Oid,
    anchor: Oid,
    default_tip: Option<Oid>,
) -> Result<Vec<Oid>, String> {
    let mut walk = repo.revwalk().map_err(|error| error.to_string())?;
    walk.push(head)
        .and_then(|()| walk.hide(anchor))
        .and_then(|()| default_tip.map_or(Ok(()), |tip| walk.hide(tip)))
        .and_then(|()| walk.simplify_first_parent())
        .map_err(|error| error.to_string())?;
    let mut path: Vec<Oid> = walk
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    path.reverse();
    Ok(path)
}

/// The tasks in `graph` (the head) that may own direct release work: each
/// task with `role: release-integration` that is open, or complete with a
/// completion this range makes (compared with `start`, the commit before
/// the path). A cancelled holder and a completion from before the range
/// own nothing.
fn owners<'g>(repo: &Repository, graph: &'g Graph, start: Option<Oid>) -> Vec<&'g RecordView> {
    graph
        .records
        .values()
        .filter(|task| task.kind == RecordKind::Task && task.role.as_deref() == Some(RELEASE_ROLE))
        .filter(|task| match task.status.as_str() {
            "complete" => completion_changed(
                start
                    .and_then(|start| record_at(repo, start, &task.path))
                    .as_ref(),
                task,
            ),
            "cancelled" => false,
            _ => true,
        })
        .collect()
}

/// The one task the release checks select as the owner of direct release
/// work on `base..head` toward `destination`, or `None` when no task or
/// several tasks are eligible, or the range is empty.
///
/// # Errors
///
/// Returns a message when a revision or the records at `head` cannot be
/// read.
pub fn release_owner(
    repo_root: &Path,
    destination: &Destination,
    base: &str,
    head: &str,
) -> Result<Option<String>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let head_oid = oid(head)?;
    let anchor = repo
        .merge_base(oid(base)?, head_oid)
        .map_err(|error| history_error(&repo, error.message()))?;
    let default_tip = destination.default.as_ref().map(|(_, tip)| *tip);
    let path = first_parent_path(&repo, head_oid, anchor, default_tip)?;
    let Some(oldest) = path.first() else {
        return Ok(None);
    };
    let start = repo
        .find_commit(*oldest)
        .ok()
        .and_then(|commit| commit.parent_id(0).ok());
    let graph = Graph::from_revision(&repo, &head_oid.to_string())?;
    Ok(match owners(&repo, &graph, start).as_slice() {
        [owner] => Some(owner.id.clone()),
        _ => None,
    })
}

/// A task record at `commit`, when the path holds one that parses.
fn record_at(repo: &Repository, commit: Oid, path: &str) -> Option<RecordView> {
    blob_at(repo, commit, path)
        .and_then(|content| RecordView::parse(RecordKind::Task, path, &content).ok())
}

/// A task record from a tree entry.
fn record_of(repo: &Repository, entry: Option<&Entry>, path: &str) -> Option<RecordView> {
    let (id, _) = entry?;
    let blob = repo.find_blob(*id).ok()?;
    RecordView::parse(
        RecordKind::Task,
        path,
        &String::from_utf8_lossy(blob.content()),
    )
    .ok()
}

/// Whether `after` completes a task or changes its active block, compared
/// with `before`.
fn completion_changed(before: Option<&RecordView>, after: &RecordView) -> bool {
    super::lifecycle::is_recompletion(before, after)
        || after.status == "complete"
            && !before.is_some_and(|before| {
                before.status == "complete" && active_block(before) == active_block(after)
            })
}

/// A direct change of a task record's criteria, keyed on the record's
/// identity: removing a record, creating one, or putting another identity
/// (`uid`) at its path changes criteria as much as editing them, so a
/// delete and a later re-create cannot reset the freeze. A record without
/// a `uid` that gains one, as `ids backfill` writes it, keeps its identity.
fn direct_criteria_changed(before: Option<&RecordView>, after: Option<&RecordView>) -> bool {
    match (before, after) {
        (None, None) => false,
        (Some(before), Some(after)) => {
            let uid = uid_of(before);
            before.criteria.signature() != after.criteria.signature()
                || (uid.is_some() && uid != uid_of(after))
        }
        _ => true,
    }
}

/// A record's `uid` frontmatter value, when it has one.
fn uid_of(record: &RecordView) -> Option<String> {
    let (data, _) = crate::validate::parse_frontmatter(record.content.as_bytes()).ok()?;
    let uid = crate::validate::get_string_field(&data, "uid");
    (!uid.trim().is_empty()).then_some(uid)
}

fn criteria_changed(before: Option<&RecordView>, after: Option<&RecordView>) -> bool {
    matches!((before, after), (Some(before), Some(after))
        if before.criteria.signature() != after.criteria.signature())
}

fn short(oid: Oid) -> String {
    oid.to_string()[..9].to_string()
}

/// Direct work found on the range, for the release-integration rule.
struct Work {
    commit: Oid,
    what: String,
}

/// What the judge decided about a release range.
#[derive(Debug, Default)]
pub struct Judgement {
    /// The findings, deduplicated.
    pub findings: Vec<Finding>,
    /// One line per commit of the first-parent path, oldest first: how it
    /// was judged (an import, an import with resolutions, or direct work).
    pub path: Vec<String>,
    /// Information that refuses nothing: each legacy criteria change, landed
    /// on its line at or before the release-rule baseline.
    pub notes: Vec<String>,
    /// The records the range brings from verified lines, for the records
    /// rule to judge where each was introduced.
    pub brought: Brought,
}

/// The judgement of a release range from `base` to `head`: every commit of
/// its first-parent path from the merge-base judged as an import, a
/// resolution or direct work.
///
/// # Errors
///
/// Returns a message when a revision, a tree or an object the judge needs
/// cannot be read or fetched.
pub fn release_findings(
    repo_root: &Path,
    destination: &Destination,
    base: &str,
    head: &str,
) -> Result<Judgement, String> {
    judge(repo_root, destination, base, head, &APPROVED_CUTOFFS)
}

/// [`release_findings`] with the transition tables' entries limited to
/// `approved`.
#[allow(clippy::too_many_lines)] // One pass over the path keeps the rules in the order R-120 states them.
fn judge(
    repo_root: &Path,
    destination: &Destination,
    base: &str,
    head: &str,
    approved: &[(&str, &str)],
) -> Result<Judgement, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    if let Some(overlay) = history_overlay(&repo)? {
        return Err(format!(
            "this clone overlays its recorded history with {overlay}, so the commits a release check walks are not the ones the destination holds; remove it, or judge from a clone without it, then retry (SPC-013 R-120)"
        ));
    }
    let head_oid = oid(head)?;
    let anchor = repo
        .merge_base(oid(base)?, head_oid)
        .map_err(|error| history_error(&repo, error.message()))?;
    let default_tip = destination.default.as_ref().map(|(_, tip)| *tip);
    let path = first_parent_path(&repo, head_oid, anchor, default_tip)?;
    let mut lines = Lines::new(repo_root, &repo, destination);
    // Read at the default target's tip, never from the range.
    let bridge = match &destination.default {
        Some((name, tip)) => bridge(&repo, name, *tip, &path, &mut lines, approved)?,
        None => Bridge::default(),
    };
    let cutoffs = &bridge.rules;
    let Some(oldest) = path.first() else {
        return Ok(Judgement::default());
    };
    let start = repo
        .find_commit(*oldest)
        .ok()
        .and_then(|commit| commit.parent_id(0).ok());
    let graph = Graph::from_revision(&repo, &head_oid.to_string())?;
    let policy_at = destination
        .default
        .as_ref()
        .map(|(name, tip)| format!("{name} at {}", short(*tip)))
        .unwrap_or_default();
    let mut notes = Vec::new();
    let mut findings = Vec::new();
    let mut work: Vec<Work> = Vec::new();
    // Each task completed directly: the commit that made it and the record
    // as it made it. The finding is the one that record earns at the head,
    // whatever a later import writes, until the task's own line supersedes
    // it.
    let mut direct_completions: BTreeMap<String, (Oid, RecordView)> = BTreeMap::new();
    // Each task's brought completions: see [`Held`].
    let mut brought_completions: BTreeMap<String, Held> = BTreeMap::new();
    let mut report = Vec::new();
    // Each record path an import brought, with the parent it came from, and
    // each record path a direct change or a resolution touched.
    let mut record_sources: BTreeMap<String, Oid> = BTreeMap::new();
    let mut direct_records: BTreeSet<String> = BTreeSet::new();

    for commit_oid in &path {
        let commit = repo
            .find_commit(*commit_oid)
            .map_err(|error| error.message().to_string())?;
        let at = short(*commit_oid);
        let first = commit.parent_id(0).ok();
        let changed = changes(&repo, first, *commit_oid)?;
        let mut unqualified = None;
        if commit.parent_count() > 1 {
            for parent in commit.parent_ids().skip(1) {
                if lines.line_of(parent)?.is_none() {
                    unqualified = Some(parent);
                    break;
                }
            }
        }
        if commit.parent_count() > 1 && unqualified.is_none() {
            // An import: each path is brought or a resolution.
            let expected = expected_import(&repo, &commit)?;
            let after = commit_entries(&repo, *commit_oid)?;
            let paths: BTreeSet<&String> = expected.keys().chain(after.keys()).collect();
            let resolutions: Vec<&String> = paths
                .iter()
                .copied()
                .filter(|path| expected.get(*path) != after.get(*path))
                .collect();
            report.push(match resolutions.len() {
                0 => format!("{at}: import"),
                count => {
                    format!("{at}: import with {count} resolved path(s), judged as direct work")
                }
            });
            if let Some(first) = resolutions.first() {
                let more = match resolutions.len() {
                    1 => String::new(),
                    count => format!(" and {} other path(s)", count - 1),
                };
                work.push(Work {
                    commit: *commit_oid,
                    what: format!(
                        "merge {at} resolves {first}{more} differently from the import it brings"
                    ),
                });
            }
            // A resolution is a direct change, even with no first-parent
            // diff.
            direct_records.extend(
                resolutions
                    .iter()
                    .filter(|path| record_kind_for_tree_path(path).is_some())
                    .map(|path| (*path).clone()),
            );
            for path in changed.keys() {
                if record_kind_for_tree_path(path).is_none() || resolutions.contains(&path) {
                    continue;
                }
                if let Some(source) = commit
                    .parent_ids()
                    .skip(1)
                    .find(|parent| entry_at(&repo, *parent, path) == after.get(path).copied())
                {
                    record_sources.insert(path.clone(), source);
                }
            }
            for path in &resolutions {
                if record_kind_for_tree_path(path) != Some(RecordKind::Task) {
                    continue;
                }
                let then = record_of(&repo, expected.get(*path), path);
                let now = record_of(&repo, after.get(*path), path);
                // The resolution may keep a `uid` one side holds (one line's
                // backfill), but never drop the release side's `uid` nor
                // bring one that neither side held.
                let ours = commit
                    .parent_id(0)
                    .ok()
                    .and_then(|parent| record_at(&repo, parent, path))
                    .as_ref()
                    .and_then(uid_of);
                let theirs = then.as_ref().and_then(uid_of);
                let kept = now.as_ref().and_then(uid_of);
                let replaced = match &kept {
                    Some(kept) => ours.as_ref() != Some(kept) && theirs.as_ref() != Some(kept),
                    None => ours.is_some(),
                };
                if replaced || direct_criteria_changed(then.as_ref(), now.as_ref()) {
                    findings.push(frozen(
                        now.as_ref().or(then.as_ref()),
                        path,
                        &format!(
                            "the resolution of merge {at}, compared with what the import brings"
                        ),
                    ));
                }
                if let Some(now) = &now {
                    if completion_changed(then.as_ref(), now) {
                        direct_completions.insert(now.id.clone(), (*commit_oid, now.clone()));
                    }
                }
            }
            let brought: Vec<&String> = paths
                .iter()
                .copied()
                .filter(|path| {
                    record_kind_for_tree_path(path) == Some(RecordKind::Task)
                        && !resolutions.contains(path)
                        && changed.contains_key(*path)
                })
                .collect();
            for path in brought {
                let then = record_of(&repo, changed[path].0.as_ref(), path);
                let Some(now) = record_of(&repo, after.get(path), path) else {
                    continue;
                };
                let source = commit.parent_ids().skip(1).find(|parent| {
                    record_at(&repo, *parent, path).is_some_and(|there| {
                        there.criteria.signature() == now.criteria.signature()
                            && active_block(&there) == active_block(&now)
                    })
                });
                if criteria_changed(then.as_ref(), Some(&now)) {
                    match source {
                        Some(source) => match landed_criteria(&repo, &now, path, source) {
                            Landed::Planning => {}
                            Landed::OwnTask { landing } => notes.push(format!(
                                "own-task amendment: {} changed its criteria in its own reviewed pull request, landed on its line at {}",
                                now.id,
                                short(landing)
                            )),
                            Landed::Unreadable(found) | Landed::Frozen(found) => {
                                findings.push(found);
                            }
                            Landed::WithCode { landing, changed } => {
                                // The cutoff is the one of the line the task
                                // itself targets, never whichever advertised
                                // line happens to hold the landing.
                                let target = now
                                    .integration_target
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|line| !line.is_empty());
                                let verdict = match (target, cutoffs.is_empty()) {
                                    (_, true) => Err(String::new()),
                                    (None, false) => Err(format!(
                                        " ({} names no integration target, so no line's release-rule cutoff can be chosen for it)",
                                        now.id
                                    )),
                                    (Some(line), false) => match cutoffs.get(line) {
                                        None => Err(format!(
                                            " ({line}, the line {} targets, records no release-rule cutoff)",
                                            now.id
                                        )),
                                        Some(cutoff) => match lines.uncovered(line, *cutoff, landing) {
                                            None => Ok((line, *cutoff)),
                                            Some(why) => Err(format!(" ({why})")),
                                        },
                                    },
                                };
                                if let Ok((line, cutoff)) = verdict {
                                    notes.push(format!(
                                            "legacy criteria change, landed before the release rule: {} on {line}, landing {}, cutoff {}, policy {policy_at}",
                                            now.id,
                                            short(landing),
                                            short(cutoff)
                                        ));
                                } else {
                                    let why = verdict.err().unwrap_or_default();
                                    findings.push(finding(
                                            FROZEN_RULE,
                                            format!(
                                                "{} changes its criteria on its line at {}, which also changes {changed}{why}; a criteria change lands on a line by a planning pull request or the task's own reviewed pull request; carry any other change in the epic's batched amendment",
                                                now.id,
                                                short(landing)
                                            ),
                                        ));
                                }
                            }
                        },
                        None => findings.push(frozen(
                            Some(&now),
                            path,
                            &format!("merge {at}, which no imported parent carries as it lands"),
                        )),
                    }
                }
                if completion_changed(then.as_ref(), &now) {
                    match (source, active_block(&now)) {
                        (Some(source), Some(block)) => {
                            let introduced = introduced_at(&repo, &now, &block, source);
                            let bound = bind_completion(
                                &repo,
                                &now,
                                &graph,
                                Landing::Commit(introduced),
                                default_tip,
                                Transport::TaskLanding,
                                super::acceptance::source_landing_base(&repo, source, introduced),
                            );
                            // Where the task's own line landed it, when the
                            // import brings it from that line.
                            let target = now
                                .integration_target
                                .as_deref()
                                .map(str::trim)
                                .filter(|line| !line.is_empty());
                            let position = match target {
                                Some(target) => own_line_position(
                                    &repo, &mut lines, &commit, target, path, &now, introduced,
                                )?,
                                None => None,
                            };
                            let task_held = brought_completions.entry(now.id.clone()).or_default();
                            let newer = position
                                .is_some_and(|at| task_held.latest.is_none_or(|was| at > was));
                            if newer && bound.is_empty() {
                                // It becomes the completion in force: the
                                // findings of every earlier one, brought or
                                // made on the release line, no longer stand.
                                task_held.findings.clear();
                                direct_completions.remove(&now.id);
                            } else {
                                task_held.findings.extend(bound);
                            }
                            if let Some(at) = position {
                                task_held.latest =
                                    Some(task_held.latest.map_or(at, |was| was.max(at)));
                            }
                        }
                        _ => {
                            direct_completions.insert(now.id.clone(), (*commit_oid, now.clone()));
                        }
                    }
                }
            }
            continue;
        }
        // Direct: a commit, or a merge with a parent that is no import.
        direct_records.extend(
            changed
                .keys()
                .filter(|path| record_kind_for_tree_path(path).is_some())
                .cloned(),
        );
        let planning_only = changed.keys().all(|path| is_planning_path(path));
        report.push(format!(
            "{at}: {}{}",
            match unqualified {
                Some(parent) => format!(
                    "merge that is no import (parent {} is on no verified line)",
                    short(parent)
                ),
                None => "direct commit".to_string(),
            },
            match (changed.len(), planning_only) {
                (0, _) => ", no path changed",
                (_, true) => ", planning records only",
                _ => ", direct work",
            }
        ));
        for (path, (then, now)) in &changed {
            if record_kind_for_tree_path(path) != Some(RecordKind::Task) {
                continue;
            }
            let then = record_of(&repo, then.as_ref(), path);
            let now = record_of(&repo, now.as_ref(), path);
            if direct_criteria_changed(then.as_ref(), now.as_ref()) {
                findings.push(frozen(now.as_ref().or(then.as_ref()), path, &at));
            }
            if let Some(now) = &now {
                if completion_changed(then.as_ref(), now) {
                    direct_completions.insert(now.id.clone(), (*commit_oid, now.clone()));
                }
            }
        }
        // A direct change of planning records alone needs no owner; anything
        // else is release-integration work.
        if let Some(path) = changed.keys().find(|path| !is_planning_path(path)) {
            let what = match unqualified {
                Some(parent) => format!(
                    "merge {at} is no import (its parent {} is on no verified epic line's or the default target's first-parent chain) and brings {path}",
                    short(parent)
                ),
                None => format!("{at} changes {path} directly"),
            };
            work.push(Work {
                commit: *commit_oid,
                what,
            });
        }
    }

    // Direct completions on the line have no task-landing transport.
    let at_head = |id: &str| {
        graph
            .records
            .get(id)
            .filter(|task| task.kind == RecordKind::Task)
    };
    for (id, (made, task)) in &direct_completions {
        // A task the head no longer holds complete carries no completion.
        if at_head(id).is_some_and(|task| task.status == "complete") {
            let prefix = format!("{id}: ");
            findings.extend(
                bind_completion(
                    &repo,
                    task,
                    &graph,
                    Landing::Commit(head_oid),
                    default_tip,
                    Transport::Direct,
                    Some(anchor),
                )
                .into_iter()
                .map(|mut found| {
                    if let Some(rest) = found.message.strip_prefix(&prefix) {
                        found.message = format!(
                            "{id} (completed directly on the release line at {}): {rest}",
                            short(*made)
                        );
                    }
                    found
                }),
            );
        }
    }

    // Direct work other than planning records belongs to the one task that
    // owns release integration, completed inside the range at the head.
    if !work.is_empty() {
        match owners(&repo, &graph, start).as_slice() {
            [] => {
                let closed: Vec<&str> = graph
                    .records
                    .values()
                    .filter(|task| {
                        task.kind == RecordKind::Task && task.role.as_deref() == Some(RELEASE_ROLE)
                    })
                    .map(|task| task.id.as_str())
                    .collect();
                let remedy = if closed.is_empty() {
                    format!("one open task gets `role: {RELEASE_ROLE}` by a planning pull request, and completes inside the release pull request at its head")
                } else {
                    format!(
                        "{} carries the role but was closed before this range; reopening it (R-119) makes it the owner again",
                        closed.join(", ")
                    )
                };
                for item in &work {
                    findings.push(finding(
                        BINDING_RULE,
                        format!(
                            "{}: {NO_OWNER}; the release rules still apply ({remedy})",
                            item.what
                        ),
                    ));
                }
            }
            [owner] if owner.status == "complete" => {
                if !direct_completions.contains_key(&owner.id) {
                    findings.extend(bind_completion(
                        &repo,
                        owner,
                        &graph,
                        Landing::Commit(head_oid),
                        default_tip,
                        Transport::Direct,
                        Some(anchor),
                    ));
                }
            }
            [owner] => {
                let commits: Vec<String> = work.iter().map(|item| short(item.commit)).collect();
                findings.push(finding(
                    BINDING_RULE,
                    format!(
                        "{} is direct release work ({}), which {} (`role: {RELEASE_ROLE}`) owns: its completion lies inside this range and binds to the head both seats reviewed",
                        work[0].what,
                        commits.join(", "),
                        owner.id
                    ),
                ));
            }
            several => {
                let ids: Vec<&str> = several.iter().map(|task| task.id.as_str()).collect();
                findings.push(finding(
                    BINDING_RULE,
                    format!(
                        "{} are all open with `role: {RELEASE_ROLE}`; one task owns direct release work",
                        ids.join(", ")
                    ),
                ));
            }
        }
    }
    findings.extend(
        brought_completions
            .into_values()
            .flat_map(|held| held.findings),
    );
    // An epic closed on its line or on the release line binds where its
    // own block was introduced, as on any line (R-33, R-60).
    let at_anchor = Graph::from_revision(&repo, &anchor.to_string())?;
    findings.extend(super::acceptance::epic_completions_in_range(
        &repo, &at_anchor, &graph, head_oid, true,
    ));
    let mut seen = HashSet::new();
    findings.retain(|found| seen.insert((found.rule, found.message.clone())));
    record_sources.retain(|path, _| !direct_records.contains(path));
    let brought = brought_records(
        &repo,
        &mut lines,
        &bridge.records,
        &policy_at,
        anchor,
        head_oid,
        &record_sources,
    );
    Ok(Judgement {
        findings,
        path: report,
        notes,
        brought,
    })
}

/// A path's full tree entry at `commit`, when it has one.
fn entry_at(repo: &Repository, commit: Oid, path: &str) -> Option<Entry> {
    let entry = repo
        .find_commit(commit)
        .ok()?
        .tree()
        .ok()?
        .get_path(Path::new(path))
        .ok()?;
    Some((entry.id(), entry.filemode().cast_unsigned()))
}

/// Where a brought change landed on its line: walking `source`'s
/// first-parent chain, the first commit whose first parent no longer
/// `holds` it.
fn landing_on_line(repo: &Repository, source: Oid, holds: &dyn Fn(Oid) -> bool) -> Oid {
    let mut at = source;
    while let Some(parent) = repo
        .find_commit(at)
        .ok()
        .and_then(|commit| commit.parent_id(0).ok())
        .filter(|parent| holds(*parent))
    {
        at = parent;
    }
    at
}

/// The records the range brings wholly from verified lines (every change
/// to them an import brought), each judged where it was introduced there:
/// a `uid` backfill, a spec's approval or supersession at the landing that
/// made it, and a complete task without an acceptance block against its
/// line's records cutoff (SPC-013 R-120, TSK-140 AC-11 to AC-13).
fn brought_records(
    repo: &Repository,
    lines: &mut Lines<'_>,
    cutoffs: &BTreeMap<String, Oid>,
    policy_at: &str,
    anchor: Oid,
    head: Oid,
    sources: &BTreeMap<String, Oid>,
) -> Brought {
    let mut brought = Brought::default();
    for (path, source) in sources {
        let Some(kind) = record_kind_for_tree_path(path) else {
            continue;
        };
        let parse = |content: &str| RecordView::parse(kind, path, content).ok();
        let Some(now) = blob_at(repo, head, path).as_deref().and_then(parse) else {
            continue;
        };
        let then = blob_at(repo, anchor, path).as_deref().and_then(parse);
        if then.as_ref().is_some_and(|then| {
            super::lifecycle::without_backfilled_uid(&now.content).as_deref()
                == Some(then.content.as_str())
        }) {
            brought.backfills.insert(now.id.clone());
            continue;
        }
        let status_at = |oid: Oid| {
            blob_at(repo, oid, path)
                .as_deref()
                .and_then(parse)
                .map(|record| record.status)
        };
        if kind == RecordKind::Spec
            && matches!(now.status.as_str(), "approved" | "superseded")
            && then.as_ref().is_none_or(|then| then.status != now.status)
        {
            let landing = landing_on_line(repo, *source, &|oid| {
                status_at(oid).as_deref() == Some(now.status.as_str())
            });
            let problem = match super::acceptance::non_planning_change(
                repo,
                repo.find_commit(landing)
                    .ok()
                    .and_then(|commit| commit.parent_id(0).ok()),
                landing,
            ) {
                Ok(None) => None,
                Ok(Some(changed)) => Some(format!(
                    "a spec becomes {} only in a planning-only change (project-management/ and docs/plan/); {} became {} on its line at {}, which also changes {changed}",
                    now.status,
                    now.id,
                    now.status,
                    short(landing)
                )),
                Err(error) => Some(format!(
                    "the landing {} that made {} {} on its line cannot be read: {}",
                    short(landing),
                    now.id,
                    now.status,
                    error.message()
                )),
            };
            brought.approvals.insert(now.id.clone(), problem);
        }
        if kind == RecordKind::Task && now.status == "complete" && now.active_blocks().is_empty() {
            let Some(line) = now
                .integration_target
                .as_deref()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            else {
                continue;
            };
            let Some(cutoff) = cutoffs.get(line) else {
                continue;
            };
            let blob = blob_at(repo, head, path);
            let landing = landing_on_line(repo, *source, &|oid| blob_at(repo, oid, path) == blob);
            if lines.uncovered(line, *cutoff, landing).is_none() {
                brought.legacy.insert(
                    now.id.clone(),
                    format!(
                        "legacy record, completed before the release rule: {} on {line}, landing {}, cutoff {}, policy {policy_at}",
                        now.id,
                        short(landing),
                        short(*cutoff)
                    ),
                );
            }
        }
    }
    brought
}

/// A criteria change made directly on the release line.
fn frozen(record: Option<&RecordView>, path: &str, where_: &str) -> Finding {
    let id = record.map_or(path, |record| record.id.as_str());
    finding(
        FROZEN_RULE,
        format!(
            "{id} changes its criteria directly on the release line ({where_}); a criteria change lands on its line by a planning pull request and is brought from there"
        ),
    )
}

/// How the landing of a brought criteria change on its line is judged.
enum Landed {
    /// The landing changed planning records only.
    Planning,
    /// The landing also changed `changed` and is not the task's own
    /// reviewed pull request; refused unless the line's release-rule cutoff
    /// covers it.
    WithCode { landing: Oid, changed: String },
    /// The landing is the task's own reviewed and completed pull request
    /// (SPC-013 R-120, resolution 41): accepted.
    OwnTask { landing: Oid },
    /// The landing cannot be read.
    Unreadable(Finding),
    /// The landing reopened the task as it changed its criteria: frozen
    /// before the planning-only exemption is considered.
    Frozen(Finding),
}

/// A brought criteria change is judged again at the commit that landed it
/// on its line: walking `source`'s first-parent chain, the first commit
/// whose first parent does not carry the brought criteria. That landing
/// must change planning records only, or be the task's own reviewed pull
/// request ([`own_task_landing`]).
fn landed_criteria(repo: &Repository, now: &RecordView, path: &str, source: Oid) -> Landed {
    let signature = now.criteria.signature();
    let carries = |oid: Oid| {
        record_at(repo, oid, path).is_some_and(|there| there.criteria.signature() == signature)
    };
    let mut at = source;
    let landing = loop {
        let Ok(commit) = repo.find_commit(at) else {
            return Landed::Unreadable(finding(
                FROZEN_RULE,
                format!(
                    "{}: the history of its criteria change cannot be read at {}",
                    now.id,
                    short(at)
                ),
            ));
        };
        match commit.parent_id(0).ok() {
            Some(parent) if carries(parent) => at = parent,
            _ => break commit,
        }
    };
    // Resolution 43: a reopened task keeps its criteria. Judged first, as
    // the task pull request rule judges it before the class exemptions.
    if let Ok(parent) = landing.parent_id(0) {
        let base = parent.to_string();
        let head = landing.id().to_string();
        let reopened = Graph::from_revision(repo, &head)
            .and_then(|after| super::acceptance::reopened_ids(repo, &base, &head, &after));
        match reopened {
            Ok(ids) if ids.contains(&now.id) => {
                return Landed::Frozen(finding(
                    FROZEN_RULE,
                    format!(
                        "{}, landed on its line at {}",
                        super::acceptance::reopened_message(&now.id),
                        short(landing.id())
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) => {
                return Landed::Unreadable(finding(
                    FROZEN_RULE,
                    format!(
                        "{}: the landing {} of its criteria change cannot be read: {error}",
                        now.id,
                        short(landing.id())
                    ),
                ));
            }
        }
    }
    match super::acceptance::non_planning_change(repo, landing.parent_id(0).ok(), landing.id()) {
        Ok(None) => Landed::Planning,
        Ok(Some(_)) if own_task_landing(repo, path, &landing) => Landed::OwnTask {
            landing: landing.id(),
        },
        Ok(Some(changed)) => Landed::WithCode {
            landing: landing.id(),
            changed,
        },
        Err(error) => Landed::Unreadable(finding(
            FROZEN_RULE,
            format!(
                "{}: the landing {} of its criteria change cannot be read: {}",
                now.id,
                short(landing.id()),
                error.message()
            ),
        )),
    }
}

/// Whether `landing` is the task's own reviewed pull request landing its
/// criteria change with its completion (SPC-013 R-120 as reconciled by
/// resolution 41): a two-parent merge whose range changes the criteria of
/// exactly the record at `path`, not complete at the merge's first parent,
/// complete at the merge with an acceptance block whose reviewed commit
/// lies in the second-parent range and already carries the criteria, the
/// merge being the clean re-merge of its parents. Any other landing that
/// changed a criterion with code stays frozen.
fn own_task_landing(repo: &Repository, path: &str, landing: &git2::Commit<'_>) -> bool {
    if landing.parent_count() != 2 {
        return false;
    }
    let (Ok(first), Ok(second)) = (landing.parent_id(0), landing.parent_id(1)) else {
        return false;
    };
    let Ok(changed) = changes(repo, Some(first), landing.id()) else {
        return false;
    };
    let mut amended = changed
        .iter()
        .filter_map(|(changed_path, (before, after))| {
            (record_kind_for_tree_path(changed_path) == Some(RecordKind::Task)
                && direct_criteria_changed(
                    record_of(repo, before.as_ref(), changed_path).as_ref(),
                    record_of(repo, after.as_ref(), changed_path).as_ref(),
                ))
            .then_some(changed_path.as_str())
        });
    if amended.next() != Some(path) || amended.next().is_some() {
        return false;
    }
    if record_at(repo, first, path).is_some_and(|before| before.status == "complete") {
        return false;
    }
    let Some(at_merge) = record_at(repo, landing.id(), path) else {
        return false;
    };
    if at_merge.status != "complete" {
        return false;
    }
    let Some(block) = active_block(&at_merge) else {
        return false;
    };
    let Some(reviewed) = commit_of(repo, &block.reviewed) else {
        return false;
    };
    // The review saw these criteria: a change after the reviewed commit is
    // unreviewed, whatever the binding rule says of it.
    if record_at(repo, reviewed, path)
        .is_none_or(|at_review| at_review.criteria.signature() != at_merge.criteria.signature())
    {
        return false;
    }
    let reaches =
        |from: Oid| from == reviewed || repo.graph_descendant_of(from, reviewed).unwrap_or(false);
    if !reaches(second) || reaches(first) {
        return false;
    }
    is_clean_remerge(repo, landing).unwrap_or(false)
}

/// What a commit's project config says of [`MARKER_KEY`]. Only config
/// that was read counts: an object this clone lacks is an error, never
/// [`Marker::Absent`].
#[derive(Clone, PartialEq, Eq)]
enum Marker {
    /// No project config, or config without the key.
    Absent,
    Adopted,
    Changed(String),
    /// Config that was read but is not valid TOML, so it carries no key.
    Invalid(String),
}

impl Marker {
    fn describe(&self) -> String {
        match self {
            Self::Absent => "removed".to_string(),
            Self::Adopted => format!("{MARKER_KEY} = 1"),
            Self::Changed(value) => format!("changed to `{MARKER_KEY} = {value}`"),
            Self::Invalid(why) => format!("unreadable ({why})"),
        }
    }
}

/// A commit's project config as the release checks read it.
#[derive(Clone, PartialEq)]
struct Config {
    /// The commit carries `.codeflow/project.toml`.
    present: bool,
    marker: Marker,
    /// Each transition table the config holds, by key.
    tables: BTreeMap<&'static str, toml::Value>,
}

/// The transition tables (SPC-013 R-120, planning resolution 29).
const TABLES: [&str; 2] = [BASELINE_KEY, RECORDS_BASELINE_KEY];

/// The project config a commit carries, memoized by blob.
///
/// # Errors
///
/// Returns a message when the commit, a tree on the config's path or the
/// config blob is not in this clone: whether the marker was there cannot
/// be read, so it is never taken as absent.
fn config_at(
    repo: &Repository,
    commit: Oid,
    blobs: &mut HashMap<Oid, Config>,
) -> Result<Config, String> {
    let absent = Config {
        present: false,
        marker: Marker::Absent,
        tables: BTreeMap::new(),
    };
    let unavailable = |what: &str, id: Oid, error: &git2::Error| {
        format!(
            "{what} at {} (object {id}) is not in this clone ({}), so whether it carries {MARKER_KEY} cannot be read; fetch it (`git fetch --unshallow` for a shallow clone, `git cat-file -p {id}` for a partial one), then retry (SPC-013 R-120)",
            short(commit),
            error.message()
        )
    };
    let found = repo
        .find_commit(commit)
        .map_err(|error| unavailable("the commit", commit, &error))?;
    let tree = found
        .tree()
        .map_err(|error| unavailable("the commit's tree", found.tree_id(), &error))?;
    let Some(folder) = tree.get_name(".codeflow") else {
        return Ok(absent);
    };
    if folder.kind() != Some(git2::ObjectType::Tree) {
        return Ok(absent);
    }
    let folder = repo
        .find_tree(folder.id())
        .map_err(|error| unavailable(".codeflow", folder.id(), &error))?;
    let Some(entry) = folder.get_name("project.toml") else {
        return Ok(absent);
    };
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return Ok(absent);
    }
    if let Some(known) = blobs.get(&entry.id()) {
        return Ok(known.clone());
    }
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| unavailable(".codeflow/project.toml", entry.id(), &error))?;
    let config = match String::from_utf8_lossy(blob.content()).parse::<toml::Value>() {
        Err(error) => Config {
            present: true,
            marker: Marker::Invalid(format!("not valid TOML: {error}")),
            tables: BTreeMap::new(),
        },
        Ok(config) => Config {
            present: true,
            marker: match config.get(MARKER_KEY) {
                None => Marker::Absent,
                Some(toml::Value::Integer(1)) => Marker::Adopted,
                Some(value) => Marker::Changed(value.to_string()),
            },
            tables: TABLES
                .iter()
                .filter_map(|key| config.get(*key).map(|value| (*key, value.clone())))
                .collect(),
        },
    };
    blobs.insert(entry.id(), config.clone());
    Ok(config)
}

/// `why` a walk of the release range failed, saying so when this clone's
/// history is shallow: the range may reach past its boundary.
fn history_error(repo: &Repository, why: &str) -> String {
    if repo.is_shallow() {
        format!(
            "this clone's history is shallow, so the release range's history cannot be read ({why}); run `git fetch --unshallow`, then retry (SPC-013 R-120)"
        )
    } else {
        why.to_string()
    }
}

/// A commit's first parent as its object records it, read from the object
/// database: grafts, replace refs and shallow boundaries never apply.
///
/// # Errors
///
/// Returns a message when the object is missing or is not a commit.
fn recorded_first_parent(odb: &git2::Odb<'_>, commit: Oid) -> Result<Option<Oid>, String> {
    let object = odb.read(commit).map_err(|error| {
        format!(
            "commit {commit} is not in this clone ({}); fetch the default target's full history, then retry (SPC-013 R-120)",
            error.message()
        )
    })?;
    if object.kind() != git2::ObjectType::Commit {
        return Err(format!("{commit} is not a commit"));
    }
    for line in object.data().split(|byte| *byte == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(parent) = line.strip_prefix(b"parent ") {
            let parent = std::str::from_utf8(parent)
                .ok()
                .and_then(|hex| Oid::from_str(hex.trim()).ok())
                .ok_or_else(|| format!("commit {commit} records a malformed parent"))?;
            return Ok(Some(parent));
        }
    }
    Ok(None)
}

/// The history overlay of the repository at `repo_root`: a non-empty
/// graft file or a replace ref, as the release judge finds it. The push
/// set treats a clone with one as unable to prove two histories unrelated.
///
/// # Errors
///
/// Returns a message when the repository, a graft file or the refs cannot
/// be read.
pub fn history_overlay_at(repo_root: &Path) -> Result<Option<String>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    history_overlay(&repo)
}

/// A local overlay that makes git or libgit2 read commits' parents or
/// objects as something other than what they record: a non-empty graft
/// file, or a replace ref: a ref named by the literal prefix
/// `refs/replace/` or `GIT_REPLACE_REF_BASE` followed by an object id.
/// The release walks go through libgit2, which follows grafts, and the
/// push set through git, which follows both.
///
/// # Errors
///
/// Returns a message when a graft file or the refs cannot be read.
pub(super) fn history_overlay(repo: &Repository) -> Result<Option<String>, String> {
    let mut grafts = vec![
        repo.commondir().join("info").join("grafts"),
        repo.path().join("info").join("grafts"),
    ];
    if let Some(file) = std::env::var_os("GIT_GRAFT_FILE") {
        grafts.push(std::path::PathBuf::from(file));
    }
    for file in grafts {
        match std::fs::read_to_string(&file) {
            Ok(text) if text.lines().any(|line| !line.trim().is_empty()) => {
                return Ok(Some(format!("the graft file {}", file.display())));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "the graft file {} cannot be read: {error}",
                    file.display()
                ))
            }
        }
    }
    // As git reads them: the base is a literal prefix, `refs/replace/`
    // unless `GIT_REPLACE_REF_BASE` names another (`refs/custom` selects
    // `refs/custom<id>`), and a ref replaces the object its remainder names.
    // The built-in base is checked either way.
    let mut bases = vec![b"refs/replace/".to_vec()];
    if let Some(base) = std::env::var_os("GIT_REPLACE_REF_BASE") {
        bases.push(base.as_encoded_bytes().to_vec());
    }
    let object_id =
        |rest: &[u8]| matches!(rest.len(), 40 | 64) && rest.iter().all(u8::is_ascii_hexdigit);
    let references = repo
        .references()
        .map_err(|error| error.message().to_string())?;
    for reference in references {
        let reference = reference.map_err(|error| error.message().to_string())?;
        let name = reference.name_bytes();
        let replaces = bases
            .iter()
            .any(|base| name.strip_prefix(base.as_slice()).is_some_and(&object_id));
        if replaces {
            return Ok(Some(format!(
                "the replace ref {} (`git replace -d` removes it)",
                String::from_utf8_lossy(name)
            )));
        }
    }
    Ok(None)
}

/// The commits this clone's history is cut at (`.git/shallow`): each looks
/// parentless though it has parents, so a walk reaching one is truncated.
///
/// # Errors
///
/// Returns a message when the clone is shallow but its boundary list
/// cannot be read.
pub(super) fn shallow_boundary(repo: &Repository) -> Result<HashSet<Oid>, String> {
    if !repo.is_shallow() {
        return Ok(HashSet::new());
    }
    let listed = std::fs::read_to_string(repo.path().join("shallow")).map_err(|error| {
        format!("this clone is shallow and its boundary cannot be read: {error}")
    })?;
    listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| Oid::from_str(line).map_err(|error| error.message().to_string()))
        .collect()
}

/// The default target's history, read from the parents each commit
/// records, so no graft, replacement or shallow boundary stands in for the
/// chain the destination holds.
struct History<'r> {
    repo: &'r Repository,
    odb: git2::Odb<'r>,
    boundary: HashSet<Oid>,
    default: &'r str,
    blobs: HashMap<Oid, Config>,
}

impl<'r> History<'r> {
    fn new(repo: &'r Repository, default: &'r str) -> Result<Self, String> {
        Ok(Self {
            repo,
            odb: repo.odb().map_err(|error| error.message().to_string())?,
            // A walk that stops at a shallow boundary is not the start of
            // history: adoption before the cut cannot be seen.
            boundary: shallow_boundary(repo)?,
            default,
            blobs: HashMap::new(),
        })
    }

    fn parent_of(&self, commit: Oid) -> Result<Option<Oid>, String> {
        let Some(parent) = recorded_first_parent(&self.odb, commit)? else {
            return Ok(None);
        };
        if self.odb.exists(parent) {
            return Ok(Some(parent));
        }
        let default = self.default;
        Err(if self.boundary.contains(&commit) {
            format!(
                "this clone's history is shallow at {}, so whether {default} adopted {MARKER_KEY} before it cannot be read; run `git fetch --unshallow`, then retry (SPC-013 R-120)",
                short(commit)
            )
        } else {
            format!(
                "{} records the parent {parent}, which is not in this clone, so whether {default} adopted {MARKER_KEY} cannot be read; fetch the default target's full history, then retry (SPC-013 R-120)",
                short(commit)
            )
        })
    }

    /// The first-parent chain from the root to `tip`, oldest first.
    fn chain(&self, tip: Oid) -> Result<Vec<Oid>, String> {
        let mut chain = Vec::new();
        let mut next = Some(tip);
        while let Some(commit) = next {
            chain.push(commit);
            next = self.parent_of(commit)?;
        }
        chain.reverse();
        Ok(chain)
    }

    fn config(&mut self, commit: Oid) -> Result<Config, String> {
        config_at(self.repo, commit, &mut self.blobs)
    }
}

/// The transition tables the default target's history honours.
#[derive(Debug, Default)]
struct Bridge {
    /// Release-rule cutoffs ([`BASELINE_KEY`]), by line.
    rules: BTreeMap<String, Oid>,
    /// Records cutoffs ([`RECORDS_BASELINE_KEY`]), by line.
    records: BTreeMap<String, Oid>,
}

/// Read the default target's first-parent history at `tip` once: the
/// adoption marker and the transition tables.
///
/// [`MARKER_KEY`] is written once: from the first commit on the chain whose
/// project config names it, every later commit there carries it as `1`,
/// and so does every commit of the judged `path` whose first parent does.
/// Its first appearance must be `1` as well. The marker never decides
/// whether R-120 applies; it fixes the adoption point the tables stop at.
///
/// Each table is honoured only as a one-time bridge: added in one commit
/// and never changed after it, at or before the adoption commit, with a
/// commit carrying project config without the marker before adoption; each
/// cutoff's tree carrying project config without the marker, neither the
/// adoption commit nor a descendant of it, and on its line's verified
/// first-parent chain as the destination advertises it, whether or not a
/// finding uses the entry; and, once every entry meets those, each entry
/// one of the `approved` cutoffs, which [`release_findings`] fixes to
/// [`APPROVED_CUTOFFS`]. `lines` fetches the advertised lines, whose
/// history holds the cutoffs, before they are read.
///
/// # Errors
///
/// Returns a message naming the condition and the commit that broke it, so
/// every release check refuses.
fn bridge(
    repo: &Repository,
    default: &str,
    tip: Oid,
    path: &[Oid],
    lines: &mut Lines<'_>,
    approved: &[(&str, &str)],
) -> Result<Bridge, String> {
    let mut history = History::new(repo, default)?;
    let chain = history.chain(tip)?;
    let mut configs = Vec::with_capacity(chain.len());
    for commit in &chain {
        configs.push(history.config(*commit)?);
    }
    let refuse = |adopted: Oid, at: Oid, marker: &Marker, place: &str| {
        format!(
            "the adoption marker `{MARKER_KEY} = 1` set on {default} at {} is {} at {} {place}; it is written once and never changed, so every release check refuses (SPC-013 R-120)",
            short(adopted),
            marker.describe(),
            short(at)
        )
    };
    let mut adopted = None;
    for (at, config) in configs.iter().enumerate() {
        match (adopted, &config.marker) {
            (None, Marker::Adopted) => adopted = Some(at),
            (None, Marker::Changed(value)) => {
                return Err(format!(
                    "the adoption marker on {default} first appears at {} as `{MARKER_KEY} = {value}`; its one value is 1, so every release check refuses (SPC-013 R-120)",
                    short(chain[at])
                ))
            }
            (Some(first), Marker::Absent | Marker::Changed(_) | Marker::Invalid(_)) => {
                return Err(refuse(
                    chain[first],
                    chain[at],
                    &config.marker,
                    &format!("on {default}"),
                ))
            }
            // Before adoption, config without the key or that is not TOML
            // carries no marker; after it, the marker kept is the rule.
            (None, Marker::Absent | Marker::Invalid(_)) | (Some(_), Marker::Adopted) => {}
        }
    }
    if let Some(first) = adopted {
        for commit in path {
            let Some(parent) = history.parent_of(*commit)? else {
                continue;
            };
            if history.config(parent)?.marker != Marker::Adopted {
                continue;
            }
            let marker = history.config(*commit)?.marker;
            if marker != Marker::Adopted {
                return Err(refuse(
                    chain[first],
                    *commit,
                    &marker,
                    "in the judged range",
                ));
            }
        }
    }
    let mut tables = Vec::new();
    for key in TABLES {
        tables.push(table_on_chain(&chain, &configs, adopted, key, default)?);
    }
    if tables.iter().any(|table| !table.is_empty()) {
        lines.fetch()?;
    }
    for (key, table) in TABLES.iter().zip(&tables) {
        for (line, cutoff) in table {
            let why = match cutoff_problem(repo, &mut history, &chain, adopted, *cutoff)? {
                Some(why) => Some(why),
                None => lines
                    .off_line(line, *cutoff)
                    .map(|why| format!("but {why}")),
            };
            if let Some(why) = why {
                return Err(bridge_refusal(
                    key,
                    default,
                    &format!("names {} as the cutoff of {line}, {why}", short(*cutoff)),
                ));
            }
        }
    }
    for (key, table) in TABLES.iter().zip(&tables) {
        for (line, cutoff) in table {
            let cutoff = cutoff.to_string();
            if !approved.contains(&(line.as_str(), cutoff.as_str())) {
                return Err(bridge_refusal(
                    key,
                    default,
                    &format!(
                        "names {} as the cutoff of {line}, which is not one of CodeFlow's approved cutoffs: the tables bridge only CodeFlow's own 2.x to 3.0 history, and no consuming project can use one",
                        &cutoff[..9]
                    ),
                ));
            }
        }
    }
    let records = tables.pop().unwrap_or_default();
    let rules = tables.pop().unwrap_or_default();
    Ok(Bridge { rules, records })
}

fn bridge_refusal(key: &str, default: &str, condition: &str) -> String {
    format!(
        "the transition table `{key}` on {default} {condition}; it is a one-time bridge for history from before the project adopted `{MARKER_KEY} = 1`, so every release check refuses (SPC-013 R-120)"
    )
}

/// One table as the chain's configs hold it: written once, at or before
/// adoption, with project config without the marker before adoption.
fn table_on_chain(
    chain: &[Oid],
    configs: &[Config],
    adopted: Option<usize>,
    key: &str,
    default: &str,
) -> Result<BTreeMap<String, Oid>, String> {
    let refuse = |condition: String| bridge_refusal(key, default, &condition);
    let mut added: Option<(usize, &toml::Value)> = None;
    for (at, config) in configs.iter().enumerate() {
        let value = config.tables.get(key);
        match (added, value) {
            (None, Some(value)) => added = Some((at, value)),
            (Some((first, was)), now) if now != Some(was) => {
                return Err(refuse(format!(
                    "added at {} is {} at {}: it is written once and never edited, extended, removed or added again",
                    short(chain[first]),
                    if now.is_none() { "removed" } else { "changed" },
                    short(chain[at])
                )))
            }
            _ => {}
        }
    }
    let Some((first, value)) = added else {
        return Ok(BTreeMap::new());
    };
    let table = parse_table(key, value)?;
    let Some(adoption) = adopted else {
        return Err(refuse(format!(
            "is added at {}, but {default} never adopted `{MARKER_KEY} = 1`, so the table has no adoption point to stop at",
            short(chain[first])
        )));
    };
    if first > adoption {
        return Err(refuse(format!(
            "is added at {}, after the adoption commit {}; a table is added at or before adoption",
            short(chain[first]),
            short(chain[adoption])
        )));
    }
    if !configs[..adoption]
        .iter()
        .any(|config| config.present && config.marker == Marker::Absent)
    {
        return Err(refuse(format!(
            "is added at {}, but no commit with project config without the marker precedes the adoption commit {}, so the project started under the release rule and has no earlier history to carry",
            short(chain[first]),
            short(chain[adoption])
        )));
    }
    Ok(table)
}

/// Why `cutoff` cannot stop a table: its tree carries no project config or
/// carries the marker, or it is the adoption commit or descends from it.
fn cutoff_problem(
    repo: &Repository,
    history: &mut History<'_>,
    chain: &[Oid],
    adopted: Option<usize>,
    cutoff: Oid,
) -> Result<Option<String>, String> {
    let config = history.config(cutoff)?;
    if !config.present {
        return Ok(Some("whose tree carries no project config".to_string()));
    }
    if config.marker != Marker::Absent {
        return Ok(Some(format!("whose project config carries `{MARKER_KEY}`")));
    }
    let Some(adoption) = adopted.map(|at| chain[at]) else {
        return Ok(None);
    };
    let after = cutoff == adoption
        || repo
            .graph_descendant_of(cutoff, adoption)
            .map_err(|error| error.message().to_string())?;
    Ok(after.then(|| {
        format!(
            "which is the adoption commit {} or descends from it",
            short(adoption)
        )
    }))
}

/// A transition table's entries: a table from each line's branch name to
/// one full commit id.
///
/// # Errors
///
/// Returns a message when the value is not a table or an entry is not a
/// full 40-character lowercase commit id.
fn parse_table(key: &str, value: &toml::Value) -> Result<BTreeMap<String, Oid>, String> {
    let toml::Value::Table(table) = value else {
        return Err(format!(
            "{key} at the default target is not a table of line names to commit ids"
        ));
    };
    let mut cutoffs = BTreeMap::new();
    for (line, value) in table {
        let entry = value.as_str().unwrap_or_default().trim();
        let full = entry.len() == 40
            && entry
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        let oid = full
            .then(|| Oid::from_str(entry).ok())
            .flatten()
            .ok_or_else(|| {
                format!(
                    "{key} entry for {line} at the default target (`{entry}`) is not a full 40-character lowercase commit id"
                )
            })?;
        cutoffs.insert(line.clone(), oid);
    }
    Ok(cutoffs)
}

#[cfg(test)]
#[path = "release_line_tests.rs"]
mod tests;
