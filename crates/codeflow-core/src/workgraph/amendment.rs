//! What a planning amendment changes, reported per epic (ADR-0078,
//! SPC-013 R-52, R-70).
//!
//! One reviewed plan change may span several epics and carry its
//! instruction text, so its pull request names every epic it changes on one
//! `Task:` line. The amendment may change any task's criteria; the reviewer
//! sees each change here: per named epic, each task whose criteria change
//! with its delta, the records added or removed, the status transitions,
//! and the instruction and doc files the range touches. A record of an epic
//! the line does not name is refused; a standalone task or a spec belongs
//! to no epic and is listed, never refused. A criteria change to a task
//! complete on both sides is flagged, since its acceptance block was
//! reviewed against the earlier criteria (R-119). Before any of that, [`range_problem`] keeps
//! product, instruction and enforcement paths out of the range. The
//! amendment lands on
//! the target, and a line takes it by merging the target; a record that
//! changed on its line since the line last merged the target is flagged.

use std::collections::{BTreeMap, BTreeSet};

use git2::{Oid, Repository};

use super::acceptance::{blob_at, criteria_delta, finding, target_tips, Finding};
use super::classify::{amendment_path, instructions_block_unchanged, AmendmentPath, ProjectPaths};
use super::lifecycle::{Graph, RecordView};
use super::work_start::RecordKind;

/// The planning amendment's report (notes) and its epic scope (a
/// refusal). Always blocking when it refuses: the scope is never
/// adjustable.
pub const AMENDMENT_RULE: &str = "work.planning_amendment";

/// The group a change is reported under: an epic, or none.
const NO_EPIC: &str = "no epic";

/// The findings of a planning amendment from the target graph `target` (at
/// `target_tip`) to `head`, over the paths the range changes. `named` holds
/// the epics the pull request's `Task:` line names, or `None` for a push,
/// which has no body yet and is judged on its pull request.
#[must_use]
pub fn findings(
    repo: &Repository,
    target: &Graph,
    head: &Graph,
    changed_paths: &[String],
    named: Option<&[String]>,
    target_tip: Oid,
) -> Vec<Finding> {
    let changed: BTreeSet<&str> = changed_paths.iter().map(String::as_str).collect();
    let ids: BTreeSet<&str> = head
        .records
        .values()
        .chain(target.records.values())
        .filter(|record| changed.contains(record.path.as_str()))
        .map(|record| record.id.as_str())
        .collect();
    let mut refused = Vec::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in ids {
        let before = target.records.get(id);
        let after = head.records.get(id);
        let owners = owners(before, after);
        if let Some(named) = named {
            for owner in owners.iter().filter(|owner| !named.contains(owner)) {
                let which = if owner == id {
                    format!("{id} is an epic")
                } else {
                    format!("{id} belongs to {owner}")
                };
                refused.push(finding(
                    AMENDMENT_RULE,
                    format!(
                        "{which}, which the `Task:` line does not name; a planning amendment names every epic whose records it changes"
                    ),
                ));
            }
        }
        let mut lines = changes(id, before, after);
        if let Some(line) = before.and_then(|record| newer_on_line(repo, target_tip, record)) {
            lines.push(line);
        }
        let keys: Vec<String> = if owners.is_empty() {
            vec![NO_EPIC.to_string()]
        } else {
            owners.into_iter().collect()
        };
        for key in keys {
            groups.entry(key).or_default().extend(lines.iter().cloned());
        }
    }
    let files: Vec<&str> = changed_paths
        .iter()
        .map(String::as_str)
        .filter(|path| *path == "AGENTS.md" || path.starts_with("docs/"))
        .collect();
    let mut found = refused;
    // Named epics first, in the order the line names them, then the rest.
    let order: Vec<String> = named
        .unwrap_or_default()
        .iter()
        .cloned()
        .chain(groups.keys().cloned())
        .collect();
    let mut seen = BTreeSet::new();
    for key in order {
        if !seen.insert(key.clone()) {
            continue;
        }
        for line in groups.get(&key).into_iter().flatten() {
            found.push(note(format!("{key}: {line}")));
        }
    }
    if !files.is_empty() {
        found.push(note(format!(
            "instruction and doc files: {}",
            files
                .iter()
                .map(|path| if *path == "AGENTS.md" {
                    "AGENTS.md (outside its managed block)".to_string()
                } else {
                    (*path).to_string()
                })
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    found
}

fn note(message: String) -> Finding {
    Finding {
        rule: AMENDMENT_RULE,
        message,
        note: true,
        epic_record: None,
    }
}

/// The epics a changed record belongs to: an epic record is its own; a task
/// belongs to its epic on either side, so moving a task between epics
/// changes both; a standalone task and a spec belong to none.
fn owners(before: Option<&RecordView>, after: Option<&RecordView>) -> BTreeSet<String> {
    let mut owners = BTreeSet::new();
    for record in before.into_iter().chain(after) {
        match record.kind {
            RecordKind::Epic => {
                owners.insert(record.id.clone());
            }
            RecordKind::Task => {
                if let Some(epic) = record
                    .epic_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|epic| !epic.is_empty() && *epic != "null")
                {
                    owners.insert(epic.to_string());
                }
            }
            RecordKind::Spec => {}
        }
    }
    owners
}

fn kind_name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Epic => "epic",
        RecordKind::Spec => "spec",
        RecordKind::Task => "task",
    }
}

/// What changed in one record, one line per change.
fn changes(id: &str, before: Option<&RecordView>, after: Option<&RecordView>) -> Vec<String> {
    let (before, after) = match (before, after) {
        (None, Some(after)) => {
            return vec![format!(
                "{id} added ({} record, {})",
                kind_name(after.kind),
                after.status
            )]
        }
        (Some(before), None) => {
            return vec![format!("{id} removed ({} record)", kind_name(before.kind))]
        }
        (Some(before), Some(after)) => (before, after),
        (None, None) => return Vec::new(),
    };
    let mut lines = Vec::new();
    if before.status != after.status {
        lines.push(format!("{id} status {} -> {}", before.status, after.status));
    }
    if before.epic_id != after.epic_id {
        let name = |epic: &Option<String>| epic.clone().unwrap_or_else(|| "none".to_string());
        lines.push(format!(
            "{id} moves from epic {} to {}",
            name(&before.epic_id),
            name(&after.epic_id)
        ));
    }
    if after.kind == RecordKind::Task && before.criteria.signature() != after.criteria.signature() {
        let delta = criteria_delta(&before.criteria.items, &after.criteria.items);
        if !delta.is_empty() {
            lines.push(format!("{id} criteria delta: {}", delta.join("; ")));
        }
        if before.status == "complete" && after.status == "complete" {
            lines.push(format!(
                "{id} is complete, and its acceptance block was reviewed against the earlier criteria: confirm that completion still holds, or reopen the task in its own pull request (SPC-013 R-119)"
            ));
        }
    }
    if lines.is_empty() {
        lines.push(format!("{id} changed, with no status or criteria change"));
    }
    lines
}

/// When `record` is a task bound for an integration line and its record
/// changed on that line since the line last merged the target, the note
/// that flags it: the amendment lands on the target and the line takes it
/// by merging the target, so the record there needs checking.
fn newer_on_line(repo: &Repository, target_tip: Oid, record: &RecordView) -> Option<String> {
    if record.kind != RecordKind::Task {
        return None;
    }
    let line = record.integration_target.as_deref()?.trim();
    if !line.starts_with("integration/") {
        return None;
    }
    target_tips(repo, line).into_iter().find_map(|(name, tip)| {
        let shared = repo.merge_base(tip, target_tip).ok()?;
        (blob_at(repo, tip, &record.path) != blob_at(repo, shared, &record.path)).then(|| {
            format!(
                "{} is newer on {name} than on the target; this amendment lands on the target, so merge the target into {line} and check the record there",
                record.id
            )
        })
    })
}

/// Why a planning amendment judged at `head` on the target `base` cannot
/// name `epic`: it has no epic record at the head, or it was cancelled
/// before this range. Cancelling it in this range is a planning change of
/// its own, so the target's status decides.
#[must_use]
pub fn epic_problem(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    epic: &str,
) -> Option<String> {
    let status = |revision: &str| -> Result<Option<String>, String> {
        let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
        let graph = Graph::from_revision(&repo, revision)?;
        Ok(graph
            .records
            .get(epic)
            .filter(|record| record.kind == RecordKind::Epic)
            .map(|record| record.status.clone()))
    };
    match (status(base), status(head)) {
        (Err(error), _) | (_, Err(error)) => Some(format!(
            "cannot read the epic records to check {epic}: {error}"
        )),
        (_, Ok(None)) => Some(format!("`Task:` names {epic}, which has no epic record")),
        (Ok(Some(status)), _) if status == "cancelled" => {
            Some(format!("`Task:` names {epic}, which is cancelled"))
        }
        _ => None,
    }
}

/// Why a planning amendment that changes `AGENTS.md`'s managed block is
/// refused.
pub const MANAGED_BLOCK_CHANGED: &str = "a planning-only pull request changes the managed block of AGENTS.md (codeflow:managed:begin to codeflow:managed:end), which must stay byte-identical to the target's; `codeflow update` owns that block, and a planning amendment edits only the project section";

/// A Git tree entry's mode for a symbolic link.
const LINK_MODE: i32 = 0o120_000;

/// [`range_problem`] for revisions named as text, as `codeflow ci` holds
/// them: the target (`base`) and the head of a range whose changed paths
/// are `paths`.
#[must_use]
pub fn range_problem_at(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    paths: &[String],
) -> Option<String> {
    let resolved = Repository::discover(repo_root)
        .map_err(|error| error.message().to_string())
        .and_then(|repo| {
            let oid = |revision: &str| {
                repo.revparse_single(revision)
                    .and_then(|object| object.peel_to_commit())
                    .map(|commit| commit.id())
                    .map_err(|error| format!("{revision}: {}", error.message()))
            };
            Ok(range_problem(&repo, oid(base)?, oid(head)?, paths))
        });
    resolved.unwrap_or_else(|error| Some(format!("cannot read the range to classify it: {error}")))
}

/// Why the change of `paths` from the target at `base` to `head` cannot
/// ride in a planning amendment (ADR-0078), or `None` when it can. The
/// project's product and watched paths come from the policy at `base`, the
/// target's, never from a working copy. Every path must be one
/// [`amendment_path`] admits; no changed path may be a symbolic link on
/// either side, or be one a link may reach in any letter case or through an
/// absolute target, so a link cannot carry instruction text in through a
/// document; and `AGENTS.md` keeps the target's managed block byte for
/// byte.
#[must_use]
pub fn range_problem(repo: &Repository, base: Oid, head: Oid, paths: &[String]) -> Option<String> {
    let project = match project_at(repo, base) {
        Ok(project) => project,
        Err(error) => {
            return Some(format!(
                "cannot read the target's policy to classify the range: {error}"
            ))
        }
    };
    let mut instructions = false;
    for path in paths {
        match amendment_path(path, &project) {
            None => {
                return Some(format!(
                    "a planning-only pull request touches a product path: {path}; a planning amendment carries records, plans, docs outside the adopter-facing set and AGENTS.md outside its managed block"
                ))
            }
            Some(AmendmentPath::Instructions) => instructions = true,
            Some(_) => {}
        }
        if [base, head]
            .iter()
            .any(|at| entry_at(repo, *at, path).is_some_and(|(mode, _)| mode == LINK_MODE))
        {
            return Some(format!(
                "a planning-only pull request changes the symbolic link {path}; a planning amendment carries regular files only"
            ));
        }
    }
    for at in [base, head] {
        for (link, reach) in links(repo, at, &project) {
            if let Some(path) = paths.iter().find(|path| reach.covers(path)) {
                return Some(format!(
                    "a planning-only pull request changes {path}, which the symbolic link {link} reaches; a planning amendment does not change what a link carries"
                ));
            }
        }
    }
    let bytes = |at: Oid| entry_at(repo, at, "AGENTS.md").map(|(_, bytes)| bytes);
    if instructions && !instructions_block_unchanged(bytes(base).as_deref(), bytes(head).as_deref())
    {
        return Some(MANAGED_BLOCK_CHANGED.to_string());
    }
    None
}

/// The mode and bytes of the entry at `path` in the tree of `commit`.
fn entry_at(repo: &Repository, commit: Oid, path: &str) -> Option<(i32, Vec<u8>)> {
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .ok()?;
    let entry = tree.get_path(std::path::Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    Some((entry.filemode(), blob.content().to_vec()))
}

/// What a symbolic link may reach once every link on its way is followed,
/// judged without the case of its letters, since a checkout on a
/// case-insensitive file system resolves `DOCS/` and `docs/` alike.
#[derive(Debug, PartialEq)]
enum Reach {
    /// The repository path the link resolves to, in lower case; empty for
    /// the repository root, which covers every path.
    Inside(String),
    /// A link on the way is absolute, holds a backslash (a separator on
    /// Windows, a letter elsewhere), leaves the repository or loops, so
    /// what it reaches is not known here and it covers every path.
    Unknown,
}

impl Reach {
    /// Whether the link may reach `path` or a folder holding it.
    fn covers(&self, path: &str) -> bool {
        let path = path.to_lowercase();
        match self {
            Self::Inside(target) => {
                target.is_empty() || path == *target || path.starts_with(&format!("{target}/"))
            }
            Self::Unknown => true,
        }
    }
}

/// How many links one resolution follows before it counts as a loop, as a
/// POSIX system does.
const MAX_HOPS: usize = 40;

/// Every symbolic link in the tree of `commit` that sits where a planning
/// amendment may not write, or at `AGENTS.md`, with what it may reach. A
/// link that resolves to the root `AGENTS.md` is left out: the managed
/// block of that file is compared on its own, so `CLAUDE.md -> AGENTS.md`
/// carries only what `AGENTS.md` may.
fn links(repo: &Repository, commit: Oid, project: &ProjectPaths) -> Vec<(String, Reach)> {
    let Ok(tree) = repo.find_commit(commit).and_then(|commit| commit.tree()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let _ = tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.filemode() == LINK_MODE {
            if let Ok(name) = entry.name() {
                let path = format!("{dir}{name}");
                if matches!(
                    amendment_path(&path, project),
                    None | Some(AmendmentPath::Instructions)
                ) {
                    let reach = resolve(repo, &tree, &path);
                    if reach != Reach::Inside("agents.md".to_string()) {
                        found.push((path, reach));
                    }
                }
            }
        }
        git2::TreeWalkResult::Ok
    });
    found
}

/// What the link at `link` in `tree` resolves to: each component is looked
/// up in the tree without regard to case, and a link met on the way is
/// followed before any `..` after it, as the file system does.
fn resolve(repo: &Repository, tree: &git2::Tree<'_>, link: &str) -> Reach {
    let mut resolved: Vec<String> = link.split('/').map(str::to_string).collect();
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    // Start from the link itself, so its own target is the first hop.
    if let Some(last) = resolved.pop() {
        pending.push_back(last);
    }
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        match part.as_str() {
            "" | "." => {}
            ".." => {
                if resolved.pop().is_none() {
                    return Reach::Unknown;
                }
            }
            name => match child(repo, tree, &resolved, name) {
                Some((_, LINK_MODE, target)) => {
                    hops += 1;
                    if hops > MAX_HOPS
                        || target.contains('\\')
                        || target.starts_with('/')
                        || target.as_bytes().get(1) == Some(&b':')
                    {
                        return Reach::Unknown;
                    }
                    for (at, piece) in target.split('/').enumerate() {
                        pending.insert(at, piece.to_string());
                    }
                }
                Some((actual, _, _)) => resolved.push(actual),
                None => resolved.push(name.to_string()),
            },
        }
    }
    Reach::Inside(resolved.join("/").to_lowercase())
}

/// The entry named `name`, in any case, in the folder `dir` of `tree`: its
/// actual name, its mode and, for a link, its target.
fn child(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    dir: &[String],
    name: &str,
) -> Option<(String, i32, String)> {
    let folder = if dir.is_empty() {
        tree.clone()
    } else {
        tree.get_path(std::path::Path::new(&dir.join("/")))
            .ok()?
            .to_object(repo)
            .ok()?
            .into_tree()
            .ok()?
    };
    let wanted = name.to_lowercase();
    let entry = folder.iter().find(|entry| {
        entry
            .name()
            .is_ok_and(|actual| actual.to_lowercase() == wanted)
    })?;
    let target = if entry.filemode() == LINK_MODE {
        let blob = repo.find_blob(entry.id()).ok()?;
        String::from_utf8_lossy(blob.content()).into_owned()
    } else {
        String::new()
    };
    Some((entry.name().ok()?.to_string(), entry.filemode(), target))
}

/// The project's product and watched paths from the policy at `commit`: its
/// `git.product_paths`, else the default for the stack its project file
/// records, and its `git.breaking_watch_paths`.
fn project_at(repo: &Repository, commit: Oid) -> Result<ProjectPaths, String> {
    let list = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let policy: serde_json::Value = match entry_at(repo, commit, ".codeflow/policy.json") {
        Some((_, bytes)) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
        None => serde_json::Value::Null,
    };
    let git = &policy["git"];
    let product = if git["product_paths"].is_array() {
        list(&git["product_paths"])
    } else {
        let stack = entry_at(repo, commit, ".codeflow/project.toml")
            .and_then(|(_, bytes)| String::from_utf8(bytes).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|table| {
                table
                    .get("stack")
                    .and_then(|stack| stack.as_str().map(str::to_string))
            })
            .unwrap_or_default();
        super::classify::stack_product_paths(&stack)
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    Ok(ProjectPaths {
        product,
        watched: list(&git["breaking_watch_paths"]),
    })
}

/// The path that keeps a criteria change landed by `landing` (on top of
/// `parent`) from being a planning change, or `None` when it is one: a
/// range that [`range_problem`] admits as a planning amendment (ADR-0078),
/// records-only ranges included, so a release imports nothing its
/// planning pull request would have refused.
///
/// # Errors
///
/// Returns the Git error when a tree cannot be read.
pub(super) fn landing_problem(
    repo: &Repository,
    parent: Option<Oid>,
    landing: Oid,
) -> Result<Option<String>, git2::Error> {
    let Some(parent) = parent else {
        return super::acceptance::non_planning_change(repo, None, landing);
    };
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let diff = repo.diff_tree_to_tree(Some(&tree(parent)?), Some(&tree(landing)?), None)?;
    let paths: Vec<String> = diff
        .deltas()
        .flat_map(|delta| [delta.old_file().path(), delta.new_file().path()])
        .flatten()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if range_problem(repo, parent, landing, &paths).is_none() {
        return Ok(None);
    }
    // Name the path at fault; a problem no single path shows (the policy
    // cannot be read) names the first.
    let single = |path: &String| range_problem(repo, parent, landing, std::slice::from_ref(path));
    Ok(paths
        .iter()
        .find(|path| single(path).is_some())
        .or_else(|| paths.first())
        .cloned())
}
