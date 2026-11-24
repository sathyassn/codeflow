//! Source attribution for unchanged records imported by an ordinary merge.
use std::collections::{BTreeMap, BTreeSet};

use super::{
    active_block, bind_completion_with_amendment, bind_epic_completion, commit_of,
    default_target_name, first_parent, history, introduced_at, is_ancestor_or_same,
    is_clean_remerge, own_file_name, owned_paths, presence_at, record_at, record_uid,
    source_landing_base, CompletionProof, Graph, Landing, Oid, Presence, RecordKind, RecordView,
    Repository, RunBases, Transport,
};
use crate::workgraph::classify::AmendmentPath;

mod home;
pub use home::HomeAnchor;
use home::{MOVED, NO_SOURCE};

/// A record whose exact contents arrived from a verified home landing.
#[derive(Debug, Clone)]
pub struct ImportedRecord {
    /// The exact imported record path.
    pub path: String,
    /// The home line, resolved independently of range spelling.
    pub home: String,
    /// The first home-line commit holding these contents.
    pub landing: Oid,
    /// The binding of a completed task; absent for other record states.
    pub completion: Option<CompletionProof>,
    /// The receiving line's own adoption that the walk of this task's home
    /// moves started from (TSK-1378 AC-13); absent when it started at the
    /// merge base of the line and the default branch.
    pub home_anchor: Option<HomeAnchor>,
}

/// The path classes a planning landing may carry when it moves a task's
/// home or introduces a task on `main` targeting a line (TSK-1378 AC-11,
/// TSK-1605): records and docs. A line's adoption anchor and a shared
/// planning introduction stay records-only.
pub const PLANNING_LANDING_PATHS: &[AmendmentPath] = &[AmendmentPath::Record, AmendmentPath::Doc];

/// Why the import check refuses, before it can name each record.
const UNENUMERATED: &str = "the imported records could not be enumerated, so this refusal may not name every refused record";

/// The routes that clear a refused home move or arrival (TSK-1378 AC-9).
fn home_repair(default: &str) -> String {
    format!("move the home through the receiving line's own reviewed planning merge where the task's state permits it, or reopen, replan or correct the source record through its pull request; a later change to the record on `{default}` does not clear the move")
}

const SOURCE_REPAIR: &str = "fetch the home and repair or reopen the work there through its task pull request, then merge the reviewed result";

type Imports = BTreeMap<String, ImportedRecord>;
// The outer history judgment and an object fetch clear this cache. A graph's
// contents distinguish callers that have removed records from the same head.
type ImportKey = (std::path::PathBuf, Oid, Oid, String);
thread_local! {
    static IMPORTS: std::cell::RefCell<BTreeMap<ImportKey, Result<Imports, String>>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

pub(crate) fn clear_cache() {
    IMPORTS.with(|memo| memo.borrow_mut().clear());
}

/// Prove imports without granting authority to the receiving head. Each
/// source must be a default target or verified epic line, contain the exact
/// record on its first-parent chain, and share that landing with the head.
/// The receiver may not introduce any change to that record outside the
/// source's history. Product changes remain in the receiver's own range.
///
/// # Errors
/// Refuses unreadable or overlaid history and an invalid source binding.
pub fn imported_records(
    repo: &Repository,
    base: Oid,
    head: Oid,
    graph: &Graph,
) -> Result<BTreeMap<String, ImportedRecord>, String> {
    let _judgment = history::judgment();
    history::check(repo).map_err(|error| error.to_string())?;
    let contents: Vec<_> = graph
        .records
        .values()
        .map(|record| (&record.path, &record.content))
        .collect();
    let digest = crate::scaffold::sha256_hex(
        &serde_json::to_vec(&contents).map_err(|error| error.to_string())?,
    );
    let key = (repo.path().to_path_buf(), base, head, digest);
    if let Some(result) = IMPORTS.with(|memo| memo.borrow().get(&key).cloned()) {
        return result;
    }
    let result = compute_imports(repo, base, head, graph);
    IMPORTS.with(|memo| memo.borrow_mut().insert(key, result.clone()));
    result
}

#[allow(clippy::too_many_lines)] // Keep enumeration, home and record refusals in one judgment order.
fn compute_imports(
    repo: &Repository,
    base: Oid,
    head: Oid,
    graph: &Graph,
) -> Result<Imports, String> {
    #[cfg(test)]
    COMPUTATIONS.with(|count| count.set(count.get() + 1));
    // A target may independently have synced the same source after the
    // receiver forked. Search imports since the shared anchor, while the
    // actual target base still judges competing criteria below.
    let unenumerated = |error: String| format!("{UNENUMERATED}: {error}");
    let anchor =
        history::merge_base(repo, base, head).map_err(|error| unenumerated(error.to_string()))?;
    let changed = merged_records(repo, anchor, head, graph).map_err(unenumerated)?;
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    // A range that changes no record by a merge reads the default target
    // only to judge the records it leaves unchanged; without one, nothing
    // is imported from it.
    let default = match default_target_name(repo) {
        Ok(default) => default,
        Err(_) if changed.is_empty() => return Ok(BTreeMap::new()),
        Err(error) => return Err(unenumerated(error)),
    };
    let default_tip = match home_tip(repo, &default.name) {
        Ok(Some(tip)) => tip,
        Ok(None) | Err(_) if changed.is_empty() => return Ok(BTreeMap::new()),
        Ok(None) => return Err(unenumerated(unavailable_home(&default.name))),
        Err(error) => return Err(unenumerated(error)),
    };
    let syncs = home::takes_default(repo, anchor, head, default_tip).map_err(unenumerated)?;
    let cutover = Cutover::new(repo, default_tip);
    // Each refusal names its record; the check never stops at the first.
    let mut refusals = Vec::new();
    let refuse = |id: &str, place: &str, reason: &str| refusal(&default.name, id, place, reason);
    if syncs {
        refusals.extend(unchanged_refusals(
            repo,
            graph,
            &changed,
            &default.name,
            default_tip,
            anchor,
            head,
        ));
    }
    if changed.is_empty() {
        return finish(BTreeMap::new(), &refusals);
    }
    for record in &changed {
        let home = record.integration_target.as_deref().map_or(
            default.name.as_str(),
            super::super::work_start::logical_target,
        );
        if home == default.name || home.starts_with("integration/") {
            if let Err(reason) = refuse_unpublished(repo, record, home, default_tip, anchor, head) {
                refusals.push(reason);
            }
        }
    }
    let mut homes = BTreeMap::from([(default.name.clone(), default_tip)]);
    let mut unverified: BTreeMap<String, String> = BTreeMap::new();
    let mut retired = BTreeMap::new();
    for record in &changed {
        let Some(home) = record.integration_target.as_deref() else {
            continue;
        };
        let home = super::super::work_start::logical_target(home);
        if homes.contains_key(home) || !home.starts_with("integration/") {
            continue;
        }
        // One unverified home names every record that uses it.
        if let Some(reason) = unverified.get(home) {
            refusals.push(format!("{}: blocks come from target history on unverified home `{home}`: {reason}; repair the home through its classified pull requests, then merge its reviewed result", record.id));
            continue;
        }
        match home_source(
            repo,
            record,
            home,
            &default.name,
            default_tip,
            anchor,
            head,
            syncs,
        ) {
            Ok(HomeSource::Line(tip)) => {
                homes.insert(home.to_string(), tip);
            }
            Ok(HomeSource::Retired(landing)) => {
                retired.insert(record.id.clone(), landing);
            }
            Ok(HomeSource::None) => {}
            Err(HomeRefusal::Line(reason)) => {
                refusals.push(format!("{}: blocks come from target history on unverified home `{home}`: {reason}; repair the home through its classified pull requests, then merge its reviewed result", record.id));
                unverified.insert(home.to_string(), reason);
            }
            Err(HomeRefusal::Arrival(reason)) => {
                refusals.push(refuse(&record.id, &format!("`{home}`"), &reason));
            }
            Err(HomeRefusal::Record(reason)) => refusals.push(reason),
        }
    }
    let mut imported = BTreeMap::new();
    for (home, tip) in homes {
        // A source already contained by the shared anchor supplies no new imports.
        let contained = match is_ancestor_or_same(repo, tip, anchor) {
            Ok(false) => on_home_chain(repo, head, tip),
            other => other,
        };
        match contained {
            Ok(true) => continue,
            Ok(false) => {}
            Err(reason) => {
                refusals.push(unenumerated(reason));
                continue;
            }
        }
        let owned = match owned_paths(
            root,
            &head.to_string(),
            &[&base.to_string(), &tip.to_string()],
        ) {
            Ok(owned) => owned,
            Err(reason) => {
                refusals.push(unenumerated(reason));
                continue;
            }
        };
        for record in &changed {
            if owned.contains(&record.path) {
                continue;
            }
            if record.kind == RecordKind::Task
                && !(home == default.name && retired.contains_key(&record.id))
                && record
                    .integration_target
                    .as_deref()
                    .is_some_and(|declared| {
                        super::super::work_start::logical_target(declared) != home
                    })
            {
                continue;
            }
            let landed = match retired.get(&record.id).filter(|_| home == default.name) {
                Some(landing) => Ok(Some(*landing)),
                None => matching_landing(repo, record, tip, head, Some(anchor)),
            };
            let landed = match landed {
                Ok(Some(landed)) => landed,
                Ok(None) => continue,
                Err(reason) => {
                    refusals.push(refuse(&record.id, &format!("`{home}`"), &reason));
                    continue;
                }
            };
            match is_new_import(repo, record, landed, anchor, head) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(reason) => {
                    refusals.push(refuse(
                        &record.id,
                        &format!("`{home}` at {landed}"),
                        &reason,
                    ));
                    continue;
                }
            }
            // The default branch's home moves are walked from the line's
            // merge base with it, for a task homed there (TSK-1378).
            let walk =
                (home == default.name && tip == default_tip && !retired.contains_key(&record.id))
                    .then_some(Walk {
                        receiver: anchor,
                        default_tip,
                    });
            match prove_record(
                repo,
                record,
                &home,
                tip,
                default_tip,
                landed,
                base,
                head,
                walk,
                Some(&cutover),
            ) {
                Ok(result) => {
                    imported.insert(record.id.clone(), result);
                }
                Err(reason) => {
                    refusals.push(refuse(
                        &record.id,
                        &format!("`{home}` at {landed}"),
                        &reason,
                    ));
                }
            }
        }
    }
    finish(imported, &refusals)
}

/// One record's refusal, with the repair its cause takes, on one line.
fn refusal(default: &str, id: &str, place: &str, reason: &str) -> String {
    let repair = if reason.starts_with(MOVED) || reason.starts_with(NO_SOURCE) {
        home_repair(default)
    } else {
        SOURCE_REPAIR.to_string()
    };
    format!("{id}: blocks come from target history on {place}: {reason}; {repair}")
        .replace('\n', " ")
}

/// The refusals for the tasks the range leaves as the receiving line holds
/// them, whose home the line re-homed or introduced since its merge base
/// with the default tip (TSK-1378 AC-13): each still answers for the moves
/// the default branch made in that span.
fn unchanged_refusals(
    repo: &Repository,
    graph: &Graph,
    changed: &[&RecordView],
    default: &str,
    default_tip: Oid,
    anchor: Oid,
    head: Oid,
) -> Vec<String> {
    let paths = match home::start_point(repo, anchor, default_tip)
        .and_then(|start| home::line_paths(repo, anchor, start))
    {
        Ok(paths) => paths,
        Err(reason) => return vec![format!("{UNENUMERATED}: {reason}")],
    };
    let mut refusals = Vec::new();
    for record in graph.records.values() {
        if record.kind != RecordKind::Task || changed.iter().any(|old| old.id == record.id) {
            continue;
        }
        let judged = match unchanged_line_candidate(repo, record, anchor, head, &paths) {
            Ok(false) => Ok(()),
            Ok(true) => home::judge_unchanged(repo, record, default_tip, anchor, head),
            Err(reason) => Err(reason),
        };
        if let Err(reason) = judged {
            refusals.push(refusal(
                default,
                &record.id,
                &format!("`{default}`"),
                &reason,
            ));
        }
    }
    refusals
}

/// The imports, or every refusal on its own line.
fn finish(imported: Imports, refusals: &[String]) -> Result<Imports, String> {
    if refusals.is_empty() {
        Ok(imported)
    } else {
        Err(refusals
            .iter()
            .map(|refusal| refusal.replace('\n', " "))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Whether `record` is a task the range leaves as the receiving line holds
/// it, and whose file some commit of the line's first-parent history since
/// its merge base with the default tip changed (`line_paths`), even when a
/// later commit restored it: the candidates whose home the line may have
/// re-homed or introduced (TSK-1378 AC-13). The identity walk in
/// `judge_unchanged` then decides by the task's home history.
fn unchanged_line_candidate(
    repo: &Repository,
    record: &RecordView,
    receiver: Oid,
    head: Oid,
    line_paths: &BTreeSet<Vec<u8>>,
) -> Result<bool, String> {
    let line_entry = home::entry_at(repo, receiver, &record.path)?;
    if line_entry.is_none() || line_entry != home::entry_at(repo, head, &record.path)? {
        return Ok(false);
    }
    Ok(line_paths.contains(record.path.as_bytes()))
}

/// Where a changed record's declared integration line supplies imports.
enum HomeSource {
    /// A published, verified line tip.
    Line(Oid),
    /// A retired line's exact record, proved at its default landing.
    Retired(Oid),
    /// No import authority, and nothing to refuse.
    None,
}

enum HomeRefusal {
    /// The line itself is unverified; every record using it is refused.
    Line(String),
    /// A record arriving on the receiving line without proof (AC-12).
    Arrival(String),
    /// A complete refusal of this record.
    Record(String),
}

#[allow(clippy::too_many_arguments)] // The range and the default target it is judged by.
fn home_source(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    default: &str,
    default_tip: Oid,
    anchor: Oid,
    head: Oid,
    syncs: bool,
) -> Result<HomeSource, HomeRefusal> {
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    let unread =
        |reason: String| HomeRefusal::Record(format!("{}: {UNENUMERATED}: {reason}", record.id));
    let Some(tip) = home_tip(repo, home).map_err(unread)? else {
        // A retired line's exact record can still be proved at its
        // published default-target landing. The proof below judges the
        // landing merge and the source span, not record equality alone.
        if let Some(landing) =
            matching_landing(repo, record, default_tip, head, Some(anchor)).map_err(unread)?
        {
            return Ok(HomeSource::Retired(landing));
        }
        // Unpublished local landings were refused above. A remaining
        // local branch supplies no import authority (for example a first
        // push of this very line, or a shared planning merge).
        if reference_tip(repo, &format!("refs/heads/{home}"))
            .map_err(unread)?
            .is_some()
        {
            return Ok(HomeSource::None);
        }
        return Err(HomeRefusal::Record(format!("{}: blocks come from target history, but home `{home}` is unavailable and no exact record landing is proven on published default `{default}`; fetch the home or default target and its full history, then rerun the range", record.id)));
    };
    let receiving = is_ancestor_or_same(repo, tip, anchor).map_err(unread)?
        || on_home_chain(repo, head, tip).map_err(unread)?;
    if receiving {
        // The receiving line supplies no import to itself. A record the
        // range takes from the default branch that newly names it as its
        // home still proves where it came from (TSK-1378 AC-12).
        if syncs && record.kind == RecordKind::Task {
            home::judge_arrival(repo, record, default_tip, anchor, head)
                .map_err(HomeRefusal::Arrival)?;
        }
        return Ok(HomeSource::None);
    }
    super::super::work_start::check_epic_line(
        root,
        home,
        default,
        &default_tip.to_string(),
        &tip.to_string(),
    )
    .map(|_| HomeSource::Line(tip))
    .map_err(HomeRefusal::Line)
}

fn merged_records<'a>(
    repo: &Repository,
    base: Oid,
    head: Oid,
    graph: &'a Graph,
) -> Result<Vec<&'a RecordView>, String> {
    let before = Graph::from_revision(repo, &base.to_string())?;
    let changed: Vec<_> = graph
        .records
        .values()
        .filter(|record| {
            !before
                .records
                .get(&record.id)
                .is_some_and(|old| old.path == record.path && old.content == record.content)
        })
        .collect();
    let mut merged = Vec::new();
    for record in changed {
        if arrived_by_merge(repo, record, head, base)? {
            merged.push(record);
        }
    }
    Ok(merged)
}

fn unavailable_home(home: &str) -> String {
    format!("blocks come from target history, but published home `{home}` is unavailable; fetch the configured remote for `{home}` and its full history, then rerun")
}

/// Local branches are authority only in an offline repository with no remotes.
pub(super) fn home_tip(repo: &Repository, home: &str) -> Result<Option<Oid>, String> {
    let remotes = repo.remotes().map_err(|error| error.to_string())?;
    if remotes.is_empty() {
        return reference_tip(repo, &format!("refs/heads/{home}"));
    }
    let configured = remotes
        .iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let known = |remote: &String| configured.contains(&Some(remote.as_str()));
    let mut remote = super::super::readiness::target_fetch_remote(repo, home)?.filter(known);
    if remote.is_none() {
        // A retired line may use the default target's fetch remote, but
        // branch config cannot turn a pushed branch path into a remote.
        let default = default_target_name(repo)?;
        remote = super::super::readiness::target_fetch_remote(repo, &default.name)?.filter(known);
    }
    let Some(remote) =
        remote.or_else(|| known(&"origin".to_string()).then(|| "origin".to_string()))
    else {
        return Ok(None);
    };
    reference_tip(repo, &format!("refs/remotes/{remote}/{home}"))
}

fn reference_tip(repo: &Repository, name: &str) -> Result<Option<Oid>, String> {
    match repo.find_reference(name) {
        Ok(reference) => reference
            .peel_to_commit()
            .map(|commit| Some(commit.id()))
            .map_err(|error| error.to_string()),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn refuse_unpublished(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    default_tip: Oid,
    base: Oid,
    head: Oid,
) -> Result<(), String> {
    if repo
        .remotes()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Ok(());
    }
    let Ok(reference) = repo.find_reference(&format!("refs/heads/{home}")) else {
        return Ok(());
    };
    let local = reference
        .peel_to_commit()
        .map_err(|error| error.to_string())?
        .id();
    if on_home_chain(repo, head, local)? {
        return Ok(());
    }
    // A deleted remote home may already be contained by the published
    // default target. A retained local ref must not veto that proof.
    let published = home_tip(repo, home)?.unwrap_or(default_tip);
    if published == local {
        return Ok(());
    }
    if let Some(landing) = matching_landing(repo, record, local, head, Some(base))? {
        if !is_ancestor_or_same(repo, landing, published)?
            && !is_shared_planning(repo, record, landing, head)?
        {
            return Err(format!("{}: blocks come from target history on home `{home}`, but its exact record exists only on unpublished local history; land the reviewed work through the home's pull request, fetch the configured remote for `{home}`, then merge that published result", record.id));
        }
    }
    Ok(())
}

/// A landing the shared anchor already holds, or shared planning
/// provenance, supplies no new import.
fn is_new_import(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    anchor: Oid,
    head: Oid,
) -> Result<bool, String> {
    Ok(!is_ancestor_or_same(repo, landed, anchor)?
        && !is_shared_planning(repo, record, landed, head)?)
}

/// Whether `landing` is shared planning provenance rather than an import
/// (TSK-1004): an admissible planning landing that introduced this task
/// record in a permitted state, carried on the receiving head's own
/// first-parent chain with the record unchanged at every step since. Two lines that branch from one planning merge both hold it,
/// and each line's own range answers for it, as it would for a planning
/// merge of its own.
///
/// Shared ancestry alone never qualifies. A version that changes an
/// existing record (a completion, evidence or a criteria change) is not an
/// introduction, so it still needs published home authority. Any record
/// state it cannot read, a renamed copy included, answers no.
fn is_shared_planning(
    repo: &Repository,
    record: &RecordView,
    landing: Oid,
    head: Oid,
) -> Result<bool, String> {
    // The state is judged once, at the landing, by identity: the head holds
    // the same version, which the walk below proves.
    if record.kind != RecordKind::Task {
        return Ok(false);
    }
    let Some(first) = admissible_planning_landing_with(repo, landing, &[AmendmentPath::Record])?
    else {
        return Ok(false);
    };
    if !permitted_at(repo, landing, record) {
        return Ok(false);
    }
    let uid = record_uid(&record.content);
    if !matches!(
        presence_at(
            repo,
            first,
            &record.id,
            uid.as_deref(),
            &own_file_name(record)
        ),
        Presence::Absent
    ) {
        return Ok(false);
    }
    // Bind to the receiver's own first-parent chain, by commit identity:
    // the record must be this exact version at every commit from the head
    // back to the landing, so the receiving line has not changed it since.
    let mut at = head;
    loop {
        if record_at(repo, at, record)?.is_none_or(|own| own.content != record.content) {
            return Ok(false);
        }
        if at == landing {
            return Ok(true);
        }
        match first_parent(repo, at)? {
            Some(parent) if is_ancestor_or_same(repo, landing, parent)? => at = parent,
            _ => return Ok(false),
        }
    }
}

/// The first parent of `landing` when it is an admissible planning landing
/// (TSK-1004): a merge whose tree is the clean merge of its parents, and
/// whose change from its first parent passes the planning-amendment
/// admissibility check, touches only paths of the `admitted` classes (both
/// names of a rename) and holds only regular, non-executable files on each
/// side. A wider set keeps every other condition; the admissibility check
/// already requires trees with no symbolic link or submodule when a doc is
/// in the change (TSK-1378 AC-11).
///
/// # Errors
/// A commit, tree, diff or policy that cannot be read is an error, which
/// the caller reports as a refusal: unread history never admits a landing.
fn admissible_planning_landing_with(
    repo: &Repository,
    landing: Oid,
    admitted: &[AmendmentPath],
) -> Result<Option<Oid>, String> {
    use super::super::amendment::{landing_problem, project_at};
    use super::super::classify::amendment_path;
    let commit = repo
        .find_commit(landing)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    let [first, _] = parents.as_slice() else {
        return Ok(None);
    };
    if !is_clean_remerge(repo, &commit).map_err(|error| error.to_string())?
        || landing_problem(repo, Some(*first), landing)
            .map_err(|error| error.to_string())?
            .is_some()
    {
        return Ok(None);
    }
    let project = project_at(repo, *first)?;
    let tree = |oid: Oid| {
        repo.find_commit(oid)
            .and_then(|commit| commit.tree())
            .map_err(|error| error.to_string())
    };
    let diff = repo
        .diff_tree_to_tree(Some(&tree(*first)?), Some(&tree(landing)?), None)
        .map_err(|error| error.to_string())?;
    for delta in diff.deltas() {
        // The side a delta adds or deletes holds no entry; every other side does.
        let sides = match delta.status() {
            git2::Delta::Added => vec![delta.new_file()],
            git2::Delta::Deleted => vec![delta.old_file()],
            _ => vec![delta.old_file(), delta.new_file()],
        };
        for file in sides {
            let name = crate::git::GitName::from_bytes(file.path_bytes().unwrap_or_default());
            let path = name.rule_text().map_err(|odd| odd.display().to_string())?;
            let regular = i32::from(file.mode()) == 0o100_644;
            let admits =
                amendment_path(path, &project).is_some_and(|kind| admitted.contains(&kind));
            if !regular || !admits {
                return Ok(None);
            }
        }
    }
    Ok(Some(*first))
}

/// Whether `landed` moved the task's declared home as reviewed planning
/// (TSK-1004 AC-5): the head under check made the move itself, on its own
/// first-parent history, through an admissible planning landing, and the
/// task is in a permitted state at the landing and at its first parent.
/// It answers only the home-change question; every other proof still runs.
///
/// # Errors
/// Unreadable history is an error, never a planning move.
fn is_planning_move(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    parent: Oid,
    head: Oid,
) -> Result<bool, String> {
    Ok(on_home_chain(repo, landed, head)?
        && admissible_planning_landing_with(repo, landed, PLANNING_LANDING_PATHS)? == Some(parent)
        && permitted_at(repo, landed, record)
        && permitted_at(repo, parent, record))
}

/// Whether the task `record` names is in a permitted state at `at`: exactly
/// one task record there carries its id or uid, every task record there
/// parses, and that record states `todo` or `blocked` and holds no
/// acceptance block, active, superseded or malformed.
fn permitted_at(repo: &Repository, at: Oid, record: &RecordView) -> bool {
    let Ok(entries) = super::task_entries(repo, at) else {
        return false;
    };
    let uid = record_uid(&record.content);
    let mut found = Vec::new();
    for (path, blob) in entries {
        let Ok(blob) = repo.find_blob(blob) else {
            return false;
        };
        let Ok(content) = std::str::from_utf8(blob.content()) else {
            return false;
        };
        let Ok(task) = RecordView::parse(RecordKind::Task, &path, content) else {
            return false;
        };
        if super::is_same_task(&task, &record.id, uid.as_deref()) {
            found.push(task);
        }
    }
    let [task] = found.as_slice() else {
        return false;
    };
    matches!(task.status.as_str(), "todo" | "blocked")
        && task.active_blocks().is_empty()
        && task.superseded_blocks().is_empty()
}

/// A line cannot supply import authority to its own current or historical head.
fn on_home_chain(repo: &Repository, head: Oid, mut tip: Oid) -> Result<bool, String> {
    if !is_ancestor_or_same(repo, head, tip)? {
        return Ok(false);
    }
    loop {
        if tip == head {
            return Ok(true);
        }
        let Some(parent) = first_parent(repo, tip)? else {
            return Ok(false);
        };
        tip = parent;
    }
}

/// Whether the current record was introduced by a merge's other parent,
/// rather than written on the receiver's first-parent path.
fn arrived_by_merge(
    repo: &Repository,
    record: &RecordView,
    mut at: Oid,
    base: Oid,
) -> Result<bool, String> {
    loop {
        if is_ancestor_or_same(repo, at, base)? {
            return Ok(false);
        }
        let commit = repo.find_commit(at).map_err(|error| error.to_string())?;
        let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
        let Some(first) = parents.first() else {
            return Ok(false);
        };
        if record_at(repo, *first, record)?.is_some_and(|old| old.content == record.content) {
            at = *first;
            continue;
        }
        for parent in parents.iter().skip(1) {
            if record_at(repo, *parent, record)?.is_some_and(|old| old.content == record.content) {
                return Ok(true);
            }
        }
        return Ok(false);
    }
}

/// Find an exact source version, then its introduction on the home chain.
fn matching_landing(
    repo: &Repository,
    record: &RecordView,
    mut at: Oid,
    head: Oid,
    base: Option<Oid>,
) -> Result<Option<Oid>, String> {
    let mut found = None;
    loop {
        if base
            .map(|base| is_ancestor_or_same(repo, at, base))
            .transpose()?
            .unwrap_or(false)
        {
            // A contiguous matching version that began before the range is not a new import.
            return if found.is_some()
                && record_at(repo, at, record)?.is_some_and(|old| old.content == record.content)
            {
                Ok(None)
            } else {
                Ok(found)
            };
        }
        let same =
            record_at(repo, at, record)?.is_some_and(|other| other.content == record.content);
        if same && is_ancestor_or_same(repo, at, head)? {
            found = Some(at);
        } else if found.is_some() {
            break;
        }
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        at = parent;
    }
    Ok(found)
}

#[allow(clippy::too_many_arguments)] // The same guarded landing inputs as prove_record.
fn prove_retired_record(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    default_tip: Oid,
    landed: Oid,
    base: Oid,
    head: Oid,
    parents: &[Oid],
) -> Result<Option<ImportedRecord>, String> {
    let Some(declared) = record
        .integration_target
        .as_deref()
        .map(super::super::work_start::logical_target)
        .filter(|declared| record.kind == RecordKind::Task && *declared != home)
    else {
        return Ok(None);
    };
    let [before_landing, source] = parents else {
        return Err("the retired home's record has no default-target merge landing".into());
    };
    if record_at(repo, *source, record)?.is_none_or(|old| old.content != record.content) {
        return Err("the default-target merge does not carry the exact retired-home record".into());
    }
    // Judge the line before main accepted it. The current default tip
    // already contains source and would leave an empty history to check.
    super::super::work_start::check_epic_line(
        repo.workdir().unwrap_or_else(|| repo.path()),
        declared,
        home,
        &before_landing.to_string(),
        &source.to_string(),
    )?;
    // The published default landing supplies authority for this historical
    // source chain. Bind the task at its own landing on that chain, so
    // later line work and closure do not become this task's completion.
    let anchor = history::merge_base(repo, base, head).map_err(|error| error.to_string())?;
    let origin = matching_landing(repo, record, *source, head, Some(anchor))?
        .ok_or("the retired home's source chain has no exact record landing")?;
    let proof = prove_record(
        repo,
        record,
        declared,
        *source,
        default_tip,
        origin,
        base,
        head,
        None,
        None,
    )?;
    Ok(Some(ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion: proof.completion,
        home_anchor: None,
    }))
}

/// The head a completed task's home landing took: the second parent of a
/// two-parent merge, which must hold the exact record, else the landing
/// itself. A clean merge can combine two edits into a record neither parent
/// holds, so no reviewed source holds that result.
///
/// # Errors
/// The second parent does not hold the exact record, or cannot be read.
fn exact_source(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    parents: &[Oid],
) -> Result<Oid, String> {
    let [_, second] = parents else {
        return Ok(landed);
    };
    if record_at(repo, *second, record)?.is_none_or(|old| old.content != record.content) {
        return Err("the home merge does not carry the exact source record".into());
    }
    Ok(*second)
}

/// Prove the completion of a task that landed at `landed` from the head
/// `span` ([`exact_source`]).
fn prove_task_completion(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    tip: Oid,
    default_tip: Oid,
    landed: Oid,
    span: Oid,
) -> Result<CompletionProof, String> {
    let block = active_block(record).expect("active block checked");
    let introduced = introduced_at(repo, record, &block, landed)?;
    let anchor = source_landing_base(repo, landed, introduced)?.or(first_parent(repo, introduced)?);
    // Judge the entire task head that landed, including changes made
    // after its completion block was written. The block's introduction
    // alone cannot prove that the home accepted the reviewed result.
    let run = RunBases::new([landed]);
    let source_graph = Graph::from_revision(repo, &landed.to_string())?;
    let bound = bind_completion_with_amendment(
        repo,
        record,
        &source_graph,
        Landing::Commit(span),
        Some(default_tip),
        Transport::TaskLanding,
        anchor,
        anchor,
        &run.stacking().with_line_head(tip),
        &run.criteria().with_imported_home(home, tip),
    );
    if !bound.findings.is_empty() {
        return Err(bound
            .findings
            .iter()
            .map(|found| found.message.as_str())
            .collect::<Vec<_>>()
            .join("; "));
    }
    let mut proof = bound
        .proof
        .ok_or("the home completion has no binding proof")?;
    if span != landed {
        proof.landing_merge = Some(landed);
    }
    Ok(proof)
}

/// The walk of the default branch's home moves for a task homed there
/// (TSK-1378): the receiving line's state and the trusted default tip.
#[derive(Clone, Copy)]
struct Walk {
    receiver: Oid,
    default_tip: Oid,
}

/// Read and validate `git.import_authority_settled_before` from the trusted
/// default tip's policy (TSK-1605). Returns `Ok(None)` when absent. Fails
/// closed if the policy is unreadable, the SHA is not 40 lowercase hex, or
/// the commit is not on `default_tip`'s first-parent history.
fn settled_before_cutover(repo: &Repository, default_tip: Oid) -> Result<Option<Oid>, String> {
    if home::entry_at(repo, default_tip, ".codeflow/policy.json")?.is_none() {
        return Ok(None);
    }
    let policy = crate::hooks::landed_policy::checked_policy_at(repo, default_tip)?;
    let Some(raw) = policy.git.import_authority_settled_before.as_deref() else {
        return Ok(None);
    };
    let valid_hex = raw.len() == 40 && raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !valid_hex {
        return Err(format!(
            "git.import_authority_settled_before '{raw}' is not 40 lowercase hex digits"
        ));
    }
    let cutover = Oid::from_str(raw).map_err(|error| {
        format!("git.import_authority_settled_before '{raw}' is invalid: {error}")
    })?;
    let mut at = default_tip;
    loop {
        if at == cutover {
            return Ok(Some(cutover));
        }
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        at = parent;
    }
    Err(format!(
        "git.import_authority_settled_before {cutover} is not on the default target's first-parent history at {default_tip}"
    ))
}

/// The settled-history cutover of one import run (TSK-1605), read only when
/// a completion could use it and at most once, its error included.
struct Cutover<'r> {
    repo: &'r Repository,
    default_tip: Oid,
    read: std::cell::OnceCell<Result<Option<Oid>, String>>,
}

impl<'r> Cutover<'r> {
    fn new(repo: &'r Repository, default_tip: Oid) -> Self {
        Self {
            repo,
            default_tip,
            read: std::cell::OnceCell::new(),
        }
    }

    /// Whether `landed` is at or before the cutover; never, when none is set.
    ///
    /// # Errors
    /// The cutover or the history cannot be read ([`settled_before_cutover`]).
    fn settles(&self, landed: Oid) -> Result<bool, String> {
        let read = self
            .read
            .get_or_init(|| settled_before_cutover(self.repo, self.default_tip));
        match read {
            Ok(Some(cutover)) => is_ancestor_or_same(self.repo, landed, *cutover),
            Ok(None) => Ok(false),
            Err(reason) => Err(reason.clone()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn prove_record(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    tip: Oid,
    default_tip: Oid,
    landed: Oid,
    base: Oid,
    head: Oid,
    walk: Option<Walk>,
    cutover: Option<&Cutover<'_>>,
) -> Result<ImportedRecord, String> {
    let commit = repo
        .find_commit(landed)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    if parents.len() > 1 && !is_clean_remerge(repo, &commit).map_err(|error| error.to_string())? {
        return Err("the home landing contains an unreviewed merge resolution".into());
    }
    let home_anchor = verify_record_home(repo, record, home, landed, &parents, base, head, walk)?;
    if let Some(proof) = prove_retired_record(
        repo,
        record,
        home,
        default_tip,
        landed,
        base,
        head,
        &parents,
    )? {
        return Ok(proof);
    }
    let completion = if record.kind == RecordKind::Task
        && super::super::redaction::check_landed(repo, landed, record)? == Some(true)
    {
        let note = super::super::redaction::annotation(record)?
            .ok_or("the redacted home record has no source annotation")?;
        let source = commit_of(repo, &note.source).ok_or("the redaction source is unavailable")?;
        if source == landed || !is_ancestor_or_same(repo, source, landed)? {
            return Err("the redaction source must precede its home landing".into());
        }
        let original = record_at(repo, source, record)?
            .ok_or("the redaction source has no original record")?;
        let origin = matching_landing(repo, &original, tip, source, None)?
            .ok_or("the original record has no landing on the home")?;
        prove_record(
            repo,
            &original,
            home,
            tip,
            default_tip,
            origin,
            origin,
            source,
            None,
            None,
        )?
        .completion
    } else if record.kind == RecordKind::Task && active_block(record).is_some() {
        // The cutover settles the completion's proof only: the landing must
        // still carry its source's exact record. A cutover that cannot be
        // read refuses the record and never counts as no cutover.
        let span = exact_source(repo, record, landed, &parents)?;
        let settled_default_landing = match (walk, cutover) {
            (Some(_), Some(cutover)) => cutover.settles(landed)?,
            _ => false,
        };
        if settled_default_landing {
            None
        } else {
            Some(prove_task_completion(
                repo,
                record,
                home,
                tip,
                default_tip,
                landed,
                span,
            )?)
        }
    } else {
        if record.kind == RecordKind::Epic {
            let origin = active_block(record)
                .map(|block| introduced_at(repo, record, &block, landed))
                .transpose()?
                .unwrap_or(landed);
            let findings = bind_epic_completion(repo, record, Landing::Commit(origin));
            if !findings.is_empty() {
                return Err(findings
                    .iter()
                    .map(|found| found.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "));
            }
        }
        None
    };
    Ok(ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion,
        home_anchor,
    })
}

/// Prove the record's identity and home at its landing, and the accepted
/// criteria. With a walk, every home move of the task on the default
/// branch since the line's merge base with it is judged ([`home`]);
/// without one, the landing's own change from its first parent is.
#[allow(clippy::too_many_arguments)] // The landing, the range and the walk.
fn verify_record_home(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    landed: Oid,
    parents: &[Oid],
    base: Oid,
    head: Oid,
    walk: Option<Walk>,
) -> Result<Option<HomeAnchor>, String> {
    let mut anchor = None;
    if record.kind == RecordKind::Task {
        let uid = record_uid(&record.content);
        match presence_at(
            repo,
            landed,
            &record.id,
            uid.as_deref(),
            &own_file_name(record),
        ) {
            Presence::Present(source) if source.content == record.content => {}
            _ => return Err("the home record identity is ambiguous or changed".into()),
        }
    }
    if let (RecordKind::Task, Some(walk)) = (record.kind, walk) {
        anchor = home::judge_home(repo, record, walk.default_tip, walk.receiver, head)?;
    } else if record.kind == RecordKind::Task {
        if let Some(parent) = parents.first() {
            if record_at(repo, *parent, record)?.is_some_and(|old| {
                old.integration_target.as_deref().is_some_and(|declared| {
                    super::super::work_start::logical_target(declared)
                        != record
                            .integration_target
                            .as_deref()
                            .map_or(home, super::super::work_start::logical_target)
                })
            }) && !is_planning_move(repo, record, landed, *parent, head)?
            {
                return Err(MOVED.into());
            }
        }
    }
    // Divergent, independently introduced criteria do not become stale just
    // because the source has the same record as the receiving head.
    // The receiving base is set aside only when the full judgement finds
    // competing versions with no doubt in either history, and the base is
    // proved to inherit its criteria (TSK-1605): a merge of a branch that
    // lacked the record is no decision of the base. Any other unknown, an
    // ambiguous or unreadable record in either history included, refuses,
    // and so does a base that amended and restored the criteria.
    if record.kind == RecordKind::Task {
        use super::super::landing::{Authority, Identity, Outside, RecordStore};
        let identity = Identity::of(record);
        let outside = Outside {
            head,
            tips: &[],
            at: None,
            own: &[],
            replaced: None,
        };
        let mut store = RecordStore::new(repo);
        let full = store.judge(&identity, &[base, landed], &outside);
        let mut criteria = full.criteria;
        if full.diverged
            && store
                .inherits_criteria(&identity, base, landed)
                .unwrap_or(false)
        {
            criteria = store.judge(&identity, &[landed], &outside).criteria;
        }
        match criteria {
            Authority::Held(source)
                if source.criteria.signature() == record.criteria.signature() => {}
            Authority::Unknown(reason) => {
                return Err(format!("accepted criteria cannot be resolved: {reason}"))
            }
            _ => {
                return Err(
                    "the receiving base and home do not agree on the accepted criteria".into(),
                )
            }
        }
    }
    Ok(anchor)
}

/// The paths whose imports have the same source proof used by acceptance.
///
/// # Errors
/// Refuses unreadable revisions, records, or unproven source history.
pub fn imported_record_paths(
    root: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<Vec<String>, String> {
    let repo = Repository::discover(root).map_err(|error| error.to_string())?;
    let resolve = |name: &str| {
        repo.revparse_single(name)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| error.to_string())
    };
    let graph = Graph::from_revision(&repo, head)?;
    Ok(
        imported_records(&repo, resolve(base)?, resolve(head)?, &graph)?
            .into_values()
            .map(|record| record.path)
            .collect(),
    )
}

#[cfg(test)]
mod home_set_tests;
#[cfg(test)]
mod home_tests;

#[cfg(test)]
thread_local! { static COMPUTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_are_computed_once_per_judgment_and_recomputed_for_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"fixture")]);
        let graph = Graph::from_revision(&repo, &head.to_string()).unwrap();
        COMPUTATIONS.with(|count| count.set(0));
        {
            let _run = history::judgment();
            for _ in 0..3 {
                assert!(imported_records(&repo, head, head, &graph)
                    .unwrap()
                    .is_empty());
            }
            assert_eq!(COMPUTATIONS.with(std::cell::Cell::get), 1);
        }
        assert!(imported_records(&repo, head, head, &graph)
            .unwrap()
            .is_empty());
        assert_eq!(COMPUTATIONS.with(std::cell::Cell::get), 2);
    }
}
