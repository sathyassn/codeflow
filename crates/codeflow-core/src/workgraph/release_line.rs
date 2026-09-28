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
//!   to the head under R-60's first rule. With no such task the finding is
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
    active_block, bind_completion, bind_completion_at_head, blob_at, finding, introduced_at,
    Finding, Landing, BINDING_RULE, FROZEN_RULE,
};
use super::classify::is_planning_path;
use super::lifecycle::{Graph, RecordView};
use super::work_start::{record_kind_for_tree_path, RecordKind};

/// The `role` value of the task that owns direct release work.
pub const RELEASE_ROLE: &str = "release-integration";
/// The release branch pattern when the default target's policy sets none.
pub const BUILTIN_PATTERN: &str = "integration/release-*";
/// The finding for direct code on a release range that no task owns.
pub const NO_OWNER: &str = "no release-integration task to own it";
/// The project-config table mapping each epic line to its release-rule
/// cutoff commit (SPC-013 R-120, planning resolution 22).
pub const BASELINE_KEY: &str = "release_rule_baseline";
/// The project-config key marking where a project adopted R-120, with its
/// one value `1` (SPC-013 R-120, planning resolution 23). It fixes only the
/// point the transition tables stop at; R-120 is enforced whatever it says
/// or whether it is there. Once the default target carries it, removing it
/// or changing its value refuses every release check.
pub const MARKER_KEY: &str = "release_rules";
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

    /// Why `landing` is not `cutoff` or before it on the verified epic
    /// line `line`'s first-parent chain as advertised now, if it is not.
    /// Dates and plain ancestry never count: a topic landed after the
    /// cutoff is after it, a cutoff rewritten away covers nothing, and a
    /// landing on another line's chain is not this line's.
    fn uncovered(&mut self, line: &str, cutoff: Oid, landing: Oid) -> Option<String> {
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
                "{line}'s release-rule cutoff {} is not on its first-parent chain",
                short(cutoff)
            ));
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
    });
    Ok(if same {
        lines.position(target, introduced)
    } else {
        None
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
    after.status == "complete"
        && !before.is_some_and(|before| {
            before.status == "complete" && active_block(before) == active_block(after)
        })
}

/// A direct change of a task record's criteria, keyed on the record's
/// identity: removing a record, creating one, or putting another identity
/// (`uid`) at its path changes criteria as much as editing them, so a
/// delete and a later re-create cannot reset the freeze.
fn direct_criteria_changed(before: Option<&RecordView>, after: Option<&RecordView>) -> bool {
    match (before, after) {
        (None, None) => false,
        (Some(before), Some(after)) => {
            before.criteria.signature() != after.criteria.signature()
                || uid_of(before) != uid_of(after)
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
}

/// The judgement of a release range from `base` to `head`: every commit of
/// its first-parent path from the merge-base judged as an import, a
/// resolution or direct work.
///
/// # Errors
///
/// Returns a message when a revision, a tree or an object the judge needs
/// cannot be read or fetched.
#[allow(clippy::too_many_lines)] // One pass over the path keeps the rules in the order R-120 states them.
pub fn release_findings(
    repo_root: &Path,
    destination: &Destination,
    base: &str,
    head: &str,
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
    // The first-parent path, oldest first. History the default target
    // holds was judged there.
    let mut walk = repo.revwalk().map_err(|error| error.to_string())?;
    walk.push(head_oid)
        .and_then(|()| walk.hide(anchor))
        .and_then(|()| default_tip.map_or(Ok(()), |tip| walk.hide(tip)))
        .and_then(|()| walk.simplify_first_parent())
        .map_err(|error| error.to_string())?;
    let mut path: Vec<Oid> = walk
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    path.reverse();
    if let Some((name, tip)) = &destination.default {
        marker_holds(&repo, name, *tip, &path)?;
    }
    let Some(oldest) = path.first() else {
        return Ok(Judgement::default());
    };
    let start = repo
        .find_commit(*oldest)
        .ok()
        .and_then(|commit| commit.parent_id(0).ok());
    let graph = Graph::from_revision(&repo, &head_oid.to_string())?;
    // Read at the default target's tip, never from the range.
    let cutoffs = match default_tip {
        Some(tip) => cutoffs_at(&repo, tip)?,
        None => BTreeMap::new(),
    };
    let policy_at = destination
        .default
        .as_ref()
        .map(|(name, tip)| format!("{name} at {}", short(*tip)))
        .unwrap_or_default();
    let mut notes = Vec::new();
    let mut lines = Lines::new(repo_root, &repo, destination);
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
            for path in &resolutions {
                if record_kind_for_tree_path(path) != Some(RecordKind::Task) {
                    continue;
                }
                let then = record_of(&repo, expected.get(*path), path);
                let now = record_of(&repo, after.get(*path), path);
                if direct_criteria_changed(then.as_ref(), now.as_ref()) {
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
                            Landed::Unreadable(found) => findings.push(found),
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
                                                "{} changes its criteria on its line at {}, which also changes {changed}{why}; a criteria change lands on a line by a planning pull request",
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

    // Completions made directly on the line bind to the head (R-60 rule 1).
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
                bind_completion_at_head(
                    &repo,
                    task,
                    &graph,
                    Landing::Commit(head_oid),
                    default_tip,
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
        let owners: Vec<&RecordView> = graph
            .records
            .values()
            .filter(|task| {
                task.kind == RecordKind::Task && task.role.as_deref() == Some(RELEASE_ROLE)
            })
            .filter(|task| match task.status.as_str() {
                "complete" => completion_changed(
                    start
                        .and_then(|start| record_at(&repo, start, &task.path))
                        .as_ref(),
                    task,
                ),
                "cancelled" => false,
                _ => true,
            })
            .collect();
        match owners.as_slice() {
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
                    findings.extend(bind_completion_at_head(
                        &repo,
                        owner,
                        &graph,
                        Landing::Commit(head_oid),
                        default_tip,
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
    let mut seen = HashSet::new();
    findings.retain(|found| seen.insert((found.rule, found.message.clone())));
    Ok(Judgement {
        findings,
        path: report,
        notes,
    })
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
    /// The landing also changed `changed`; refused unless the line's
    /// release-rule cutoff covers it.
    WithCode { landing: Oid, changed: String },
    /// The landing cannot be read.
    Unreadable(Finding),
}

/// A brought criteria change is judged again at the commit that landed it
/// on its line: walking `source`'s first-parent chain, the first commit
/// whose first parent does not carry the brought criteria. That landing
/// must change planning records only.
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
    match super::acceptance::non_planning_change(repo, landing.parent_id(0).ok(), landing.id()) {
        Ok(None) => Landed::Planning,
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

/// The marker a commit's project config carries, memoized by blob.
///
/// # Errors
///
/// Returns a message when the commit, a tree on the config's path or the
/// config blob is not in this clone: whether the marker was there cannot
/// be read, so it is never taken as absent.
fn marker_at(
    repo: &Repository,
    commit: Oid,
    blobs: &mut HashMap<Oid, Marker>,
) -> Result<Marker, String> {
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
        return Ok(Marker::Absent);
    };
    if folder.kind() != Some(git2::ObjectType::Tree) {
        return Ok(Marker::Absent);
    }
    let folder = repo
        .find_tree(folder.id())
        .map_err(|error| unavailable(".codeflow", folder.id(), &error))?;
    let Some(entry) = folder.get_name("project.toml") else {
        return Ok(Marker::Absent);
    };
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return Ok(Marker::Absent);
    }
    if let Some(known) = blobs.get(&entry.id()) {
        return Ok(known.clone());
    }
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| unavailable(".codeflow/project.toml", entry.id(), &error))?;
    let marker = match String::from_utf8_lossy(blob.content()).parse::<toml::Value>() {
        Err(error) => Marker::Invalid(format!("not valid TOML: {error}")),
        Ok(config) => match config.get(MARKER_KEY) {
            None => Marker::Absent,
            Some(toml::Value::Integer(1)) => Marker::Adopted,
            Some(value) => Marker::Changed(value.to_string()),
        },
    };
    blobs.insert(entry.id(), marker.clone());
    Ok(marker)
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

/// A local overlay that makes git or libgit2 read commits' parents or
/// objects as something other than what they record: a non-empty graft
/// file, or a replace ref under `refs/replace/` or `GIT_REPLACE_REF_BASE`.
/// The release walks go through libgit2, which follows grafts, and the
/// push set through git, which follows both.
///
/// # Errors
///
/// Returns a message when a graft file or the refs cannot be read.
fn history_overlay(repo: &Repository) -> Result<Option<String>, String> {
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
    let mut bases = vec!["refs/replace/".to_string()];
    if let Ok(base) = std::env::var("GIT_REPLACE_REF_BASE") {
        if !base.trim().is_empty() {
            let base = base.trim().to_string();
            bases.push(if base.ends_with('/') {
                base
            } else {
                format!("{base}/")
            });
        }
    }
    let references = repo
        .references()
        .map_err(|error| error.message().to_string())?;
    for reference in references {
        let reference = reference.map_err(|error| error.message().to_string())?;
        let name = reference.name_bytes();
        if bases.iter().any(|base| name.starts_with(base.as_bytes())) {
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
fn shallow_boundary(repo: &Repository) -> Result<HashSet<Oid>, String> {
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

/// [`MARKER_KEY`] is written once: from the first commit on the default
/// target's first-parent chain whose project config names it, every later
/// commit on that chain carries it as `1`, and so does every commit of the
/// judged `path` whose first parent does. The key's first appearance must
/// be `1` as well. The marker never decides whether R-120 applies; this
/// only keeps the adoption point it fixes from being moved.
///
/// # Errors
///
/// Returns a message naming the commit that removed, changed or broke the
/// marker, so every release check refuses.
fn marker_holds(repo: &Repository, default: &str, tip: Oid, path: &[Oid]) -> Result<(), String> {
    // A walk that stops at a shallow boundary is not the start of history:
    // adoption before the cut cannot be seen, so nothing is inferred.
    let boundary = shallow_boundary(repo)?;
    let truncated = |commit: Oid| {
        boundary.contains(&commit).then(|| {
            format!(
                "this clone's history is shallow at {}, so whether {default} adopted {MARKER_KEY} before it cannot be read; run `git fetch --unshallow`, then retry (SPC-013 R-120)",
                short(commit)
            )
        })
    };
    // Parents as each commit records them, so no graft, replacement or
    // shallow boundary stands in for the chain the destination holds.
    let odb = repo.odb().map_err(|error| error.message().to_string())?;
    let parent_of = |commit: Oid| -> Result<Option<Oid>, String> {
        let Some(parent) = recorded_first_parent(&odb, commit)? else {
            return Ok(None);
        };
        if odb.exists(parent) {
            return Ok(Some(parent));
        }
        Err(truncated(commit).unwrap_or_else(|| {
            format!(
                "{} records the parent {parent}, which is not in this clone, so whether {default} adopted {MARKER_KEY} cannot be read; fetch the default target's full history, then retry (SPC-013 R-120)",
                short(commit)
            )
        }))
    };
    let mut chain = Vec::new();
    let mut next = Some(tip);
    while let Some(commit) = next {
        chain.push(commit);
        next = parent_of(commit)?;
    }
    chain.reverse();
    let mut blobs = HashMap::new();
    let refuse = |adopted: Oid, at: Oid, marker: &Marker, place: &str| {
        format!(
            "the adoption marker `{MARKER_KEY} = 1` set on {default} at {} is {} at {} {place}; it is written once and never changed, so every release check refuses (SPC-013 R-120)",
            short(adopted),
            marker.describe(),
            short(at)
        )
    };
    let mut adopted = None;
    for commit in chain {
        let marker = marker_at(repo, commit, &mut blobs)?;
        match (adopted, &marker) {
            (None, Marker::Adopted) => adopted = Some(commit),
            (None, Marker::Changed(value)) => {
                return Err(format!(
                    "the adoption marker on {default} first appears at {} as `{MARKER_KEY} = {value}`; its one value is 1, so every release check refuses (SPC-013 R-120)",
                    short(commit)
                ))
            }
            (Some(first), Marker::Absent | Marker::Changed(_) | Marker::Invalid(_)) => {
                return Err(refuse(first, commit, &marker, &format!("on {default}")))
            }
            // Before adoption, config without the key or that is not TOML
            // carries no marker; after it, the marker kept is the rule.
            (None, Marker::Absent | Marker::Invalid(_)) | (Some(_), Marker::Adopted) => {}
        }
    }
    let Some(first) = adopted else {
        return Ok(());
    };
    for commit in path {
        let Some(parent) = parent_of(*commit)? else {
            continue;
        };
        if marker_at(repo, parent, &mut blobs)? != Marker::Adopted {
            continue;
        }
        let marker = marker_at(repo, *commit, &mut blobs)?;
        if marker != Marker::Adopted {
            return Err(refuse(first, *commit, &marker, "in the judged range"));
        }
    }
    Ok(())
}

/// The release-rule cutoffs recorded in `.codeflow/project.toml` at `tip`,
/// the default target's advertised tip: a table from each line's branch
/// name to one full commit id. Absent file or key: none.
///
/// # Errors
///
/// Returns a message when the file cannot be read or parsed, the key is not
/// a table, or a value is not a full 40-character lowercase commit id.
fn cutoffs_at(repo: &Repository, tip: Oid) -> Result<BTreeMap<String, Oid>, String> {
    let tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.message().to_string())?;
    let Ok(entry) = tree.get_path(Path::new(".codeflow/project.toml")) else {
        return Ok(BTreeMap::new());
    };
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| format!(".codeflow/project.toml cannot be read: {}", error.message()))?;
    let config: toml::Value = String::from_utf8_lossy(blob.content())
        .parse()
        .map_err(|error| format!(".codeflow/project.toml is not valid TOML: {error}"))?;
    let table = match config.get(BASELINE_KEY) {
        None => return Ok(BTreeMap::new()),
        Some(toml::Value::Table(table)) => table,
        Some(_) => {
            return Err(format!(
                "{BASELINE_KEY} at the default target is not a table of line names to commit ids"
            ))
        }
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
                    "{BASELINE_KEY} entry for {line} at the default target (`{entry}`) is not a full 40-character lowercase commit id"
                )
            })?;
        cutoffs.insert(line.clone(), oid);
    }
    Ok(cutoffs)
}
