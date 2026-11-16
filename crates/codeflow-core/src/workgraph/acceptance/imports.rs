//! Source attribution for unchanged records imported by an ordinary merge.
use std::collections::{BTreeMap, BTreeSet};

use super::mains_bytes::MainsBytes;
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
/// home (TSK-1378 AC-11): records and plans only, until the operator
/// decides otherwise. The line's own planning move and the walk of the
/// default branch's moves use it; a line's adoption anchor and a shared
/// planning introduction always admit records only.
pub const PLANNING_LANDING_PATHS: &[AmendmentPath] = &[AmendmentPath::Record];

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
type ImportKey = (
    std::path::PathBuf,
    Oid,
    Oid,
    String,
    Option<MainsBytes>,
    Option<String>,
);
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
/// With `mains`, a record that arrived by merge and that main's bytes
/// admits ([`super::mains_bytes::judge`], TSK-1185) is imported from the
/// verified default tip without re-proof; its completion is the one the
/// default branch admitted.
///
/// `receiving` is the epic line the range lands on, as the run names it
/// (`--into`, or the judged branch when that is the line), never read from
/// a record or a ref. A task whose line copy names it, or whose copy this
/// check cannot place because the run names no line, takes a home move on
/// the default branch only from a verified tip (TSK-1185 AC-16).
///
/// # Errors
/// Refuses unreadable or overlaid history and an invalid source binding.
pub fn imported_records(
    repo: &Repository,
    base: Oid,
    head: Oid,
    graph: &Graph,
    mains: Option<&MainsBytes>,
    receiving: Option<&str>,
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
    let key = (
        repo.path().to_path_buf(),
        base,
        head,
        digest,
        mains.cloned(),
        receiving.map(str::to_string),
    );
    if let Some(result) = IMPORTS.with(|memo| memo.borrow().get(&key).cloned()) {
        return result;
    }
    let result = compute_imports(repo, (base, head), graph, mains, receiving);
    IMPORTS.with(|memo| memo.borrow_mut().insert(key, result.clone()));
    result
}

/// Main's bytes (TSK-1185 AC-11, AC-13): each record in `changed` the rule
/// admits is the default branch's, imported from the verified tip without
/// re-proof and removed from `changed`. A judgment that cannot be read
/// admits nothing, so every record keeps the full checks.
fn take_mains_bytes(
    repo: &Repository,
    mains: &MainsBytes,
    (base, head): (Oid, Oid),
    graph: &Graph,
    changed: &mut Vec<&RecordView>,
) -> Imports {
    let admitted = super::mains_bytes::judge(repo, mains, base, head, graph)
        .map(|judged| judged.admitted)
        .unwrap_or_default();
    let mut imported = BTreeMap::new();
    changed.retain(|record| {
        if !admitted.contains(&record.path) {
            return true;
        }
        imported.insert(
            record.id.clone(),
            ImportedRecord {
                path: record.path.clone(),
                home: record.integration_target.as_deref().map_or_else(
                    || "the default branch".to_string(),
                    |declared| super::super::work_start::logical_target(declared).to_string(),
                ),
                landing: mains.tip,
                completion: None,
                home_anchor: None,
            },
        );
        false
    });
    imported
}

#[allow(clippy::too_many_lines)] // Keep enumeration, home and record refusals in one judgment order.
fn compute_imports(
    repo: &Repository,
    (base, head): (Oid, Oid),
    graph: &Graph,
    mains: Option<&MainsBytes>,
    receiving: Option<&str>,
) -> Result<Imports, String> {
    #[cfg(test)]
    COMPUTATIONS.with(|count| count.set(count.get() + 1));
    // A target may independently have synced the same source after the
    // receiver forked. Search imports since the shared anchor, while the
    // actual target base still judges competing criteria below.
    let unenumerated = |error: String| format!("{UNENUMERATED}: {error}");
    let anchor =
        history::merge_base(repo, base, head).map_err(|error| unenumerated(error.to_string()))?;
    let merged = merged_records(repo, anchor, head, graph).map_err(unenumerated)?;
    let mut changed = merged.clone();
    let trusted = mains.map(|mains| mains.tip);
    let mut imported = match mains {
        Some(mains) => take_mains_bytes(repo, mains, (base, head), graph, &mut changed),
        None => BTreeMap::new(),
    };
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    // Only a range that merges no record may skip the default target.
    // Records admitted from main's bytes still arrived by merge, so an
    // unreadable default refuses them too, and the walk over the records
    // the range leaves unchanged always runs.
    let default = match default_target_name(repo) {
        Ok(default) => default,
        Err(_) if merged.is_empty() => return Ok(BTreeMap::new()),
        Err(error) => return Err(unenumerated(error)),
    };
    let default_tip = match home_tip(repo, &default.name) {
        Ok(Some(tip)) => tip,
        Ok(None) | Err(_) if merged.is_empty() => return Ok(BTreeMap::new()),
        Ok(None) => return Err(unenumerated(unavailable_home(&default.name))),
        Err(error) => return Err(unenumerated(error)),
    };
    let syncs = home::takes_default(repo, anchor, head, default_tip).map_err(unenumerated)?;
    // Each refusal names its record; the check never stops at the first.
    let mut refusals = Vec::new();
    let refuse = |id: &str, place: &str, reason: &str| refusal(&default.name, id, place, reason);
    if syncs {
        refusals.extend(unchanged_refusals(
            repo,
            graph,
            &merged,
            &default.name,
            default_tip,
            anchor,
            head,
        ));
    }
    if changed.is_empty() {
        return finish(imported, &refusals);
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
                        receiving,
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
                trusted,
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
/// TSK-1185 AC-16 adds one exception to TSK-1004 AC-8: the landing may
/// instead be on the first-parent chain of the default tip this run
/// verified (`trusted`), never of a home a record or a ref names.
///
/// # Errors
/// Unreadable history is an error, never a planning move.
fn is_planning_move(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    parent: Oid,
    head: Oid,
    trusted: Option<Oid>,
) -> Result<bool, String> {
    let on_chain = on_home_chain(repo, landed, head)?
        || match trusted {
            Some(tip) => on_home_chain(repo, landed, tip)?,
            None => false,
        };
    Ok(on_chain && is_trusted_planning_move(repo, record, landed, parent)?)
}

/// Whether `landed` is an admissible planning landing over `parent` with
/// the task in a permitted state at both (TSK-1004 AC-6, AC-7). The caller
/// supplies the chain `landed` is on.
pub(super) fn is_trusted_planning_move(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    parent: Oid,
) -> Result<bool, String> {
    Ok(
        admissible_planning_landing_with(repo, landed, PLANNING_LANDING_PATHS)? == Some(parent)
            && permitted_at(repo, landed, record)
            && permitted_at(repo, parent, record),
    )
}

/// The record as it stood before `landed`, when `landed` corrected a
/// completed task's declared home to the home that already holds its work
/// (TSK-1185 AC-10). A task that lands on a home other than the one it
/// declared is left naming the wrong home, and a planning landing can only
/// correct it once the task is complete, which [`is_planning_move`] refuses.
///
/// All of these must hold on the home's own history: `landed` is on its
/// first-parent chain, it is an admissible planning landing, the new target
/// is `home`, the record's only changed line is `integration_target`, and the
/// record's reviewed commit is already in the landing's first parent. So no
/// other edit rides along and the claim rests on commit identity, and
/// [`prove_record`] then proves the completion at its original landing.
///
/// # Errors
/// Unreadable history is an error, never a correction.
fn home_correction(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    tip: Oid,
    landed: Oid,
    parent: Oid,
) -> Result<Option<RecordView>, String> {
    let names_home = record
        .integration_target
        .as_deref()
        .is_some_and(|declared| super::super::work_start::logical_target(declared) == home);
    if record.kind != RecordKind::Task || !names_home {
        return Ok(None);
    }
    let Some(before) = record_at(repo, parent, record)? else {
        return Ok(None);
    };
    let reviewed = active_block(record).and_then(|block| commit_of(repo, &block.reviewed));
    let Some(reviewed) = reviewed else {
        return Ok(None);
    };
    let admitted = before.integration_target != record.integration_target
        && changes_only_the_target(&before.content, &record.content)
        && on_home_chain(repo, landed, tip)?
        && is_ancestor_or_same(repo, reviewed, parent)?
        && admissible_planning_landing_with(repo, landed, PLANNING_LANDING_PATHS)? == Some(parent);
    Ok(admitted.then_some(before))
}

/// The task's declared home in `view`, by its logical name, or `None` when
/// the record declares none.
fn declared_home(view: &RecordView) -> Option<&str> {
    view.integration_target
        .as_deref()
        .map(super::super::work_start::logical_target)
}

/// The refusal of a task whose record before `at`, on its home's
/// first-parent history, is not exactly one record with its id and uid at
/// its path: its home history cannot be followed by identity, so the walk
/// never reads another record, or the task's absence, in its place.
fn path_changed(at: Oid) -> String {
    format!(
        "the task's record cannot be identified before {at}: it is not one record with the task's id and uid at the same path (another path, another task's record, two copies, or an unreadable task record), so its earlier home moves cannot be judged; keep the record at one path while another line imports it"
    )
}

/// The refusal of a home change where one side declares no home.
fn home_undeclared(at: Oid) -> String {
    format!(
        "{MOVED}: the record declares no home on one side of the change at {at}, so the change cannot be judged; declare the home on both sides"
    )
}

/// The task's record at `parent`, the first parent of `at`, found by
/// identity, never by path alone: `None` when no task record there carries
/// its id or uid, so `at` introduced it. Otherwise exactly one record there
/// must carry it, with the same id and uid and at the path of `current`,
/// the task's record at `at`. Another path, another task's record at the
/// path, two copies, or an unreadable task record is refused.
fn version_before(
    repo: &Repository,
    current: &RecordView,
    record: &RecordView,
    at: Oid,
    parent: Oid,
) -> Result<Option<RecordView>, String> {
    let who = home::Identity::of(record);
    match home::held_at(repo, parent, &who) {
        home::Held::Absent => Ok(None),
        home::Held::One(old)
            if old.path == current.path
                && old.id == record.id
                && record_uid(&old.content) == record_uid(&record.content) =>
        {
            Ok(Some(*old))
        }
        _ => Err(path_changed(at)),
    }
}

/// Refuse a home change an edit landed after it would hide (TSK-1185).
/// [`matching_landing`] finds only the landing of the record's current text,
/// whose parent already names the new home, so every commit that moved the
/// task is found here, on the home's first-parent history past the receiver,
/// and must be a planning move or a home correction in its own right. A
/// planning move is judged as [`is_planning_move`] judges one at its own
/// landing: on the first-parent history of the head under check or of the
/// default tip this run verified (`trusted`), so a later landing never
/// lowers the bar (TSK-1004 AC-8, TSK-1185 AC-16). An accepted move or
/// correction clears only itself: the walk goes on to the receiver. It
/// stops early only where the record was introduced, proved by identity; a
/// changed path or a side that declares no home is refused.
fn verify_earlier_home_move(
    repo: &Repository,
    record: &RecordView,
    (home, tip): (&str, Oid),
    landed: Oid,
    (base, head): (Oid, Oid),
    trusted: Option<Oid>,
) -> Result<(), String> {
    if record.kind != RecordKind::Task {
        return Ok(());
    }
    let mut current = record.clone();
    let mut at = landed;
    loop {
        // A commit the receiver holds is past the walk; its child is not, so
        // a move made right at the fork is still judged.
        if is_ancestor_or_same(repo, at, base)? {
            return Ok(());
        }
        let Some(parent) = first_parent(repo, at)? else {
            return Ok(());
        };
        let Some(old) = version_before(repo, &current, record, at, parent)? else {
            return Ok(());
        };
        // `verify_record_home` judges the landing's own change.
        if at != landed && declared_home(&old) != declared_home(&current) {
            judge_earlier_move(
                repo,
                (&current, &old, record),
                (home, tip),
                (at, parent),
                (head, trusted),
            )?;
        }
        current = old;
        at = parent;
    }
}

/// One earlier home move at `at` over `parent`, where `current` is the
/// record at `at` and `old` at `parent`: a proved home correction (TSK-1185
/// AC-10), else a planning move in a permitted state on the head's or the
/// verified tip's first-parent history. A side that declares no home is
/// refused unless the move is a proved correction.
fn judge_earlier_move(
    repo: &Repository,
    (current, old, record): (&RecordView, &RecordView, &RecordView),
    (home, tip): (&str, Oid),
    (at, parent): (Oid, Oid),
    (head, trusted): (Oid, Option<Oid>),
) -> Result<(), String> {
    if home_correction(repo, current, home, tip, at, parent)?.is_some() {
        return Ok(());
    }
    if declared_home(current).is_none() || declared_home(old).is_none() {
        return Err(home_undeclared(at));
    }
    if !is_trusted_planning_move(repo, record, at, parent)? {
        return Err(MOVED.into());
    }
    if is_planning_move(repo, record, at, parent, head, trusted)? {
        return Ok(());
    }
    Err(format!(
        "{MOVED}: the earlier move at {at} is a planning landing on the first-parent history of neither the head under check nor a default-branch tip this run verified; the move is taken when it lands on the checked line itself, or on a run that verifies the default branch's tip, as the hosted check does with --policy-from, for a move made there"
    ))
}

/// Whether two record texts differ in exactly one line, the
/// `integration_target` line.
fn changes_only_the_target(before: &str, after: &str) -> bool {
    let (before, after): (Vec<_>, Vec<_>) =
        (before.split('\n').collect(), after.split('\n').collect());
    let target = |line: &str| line.starts_with("integration_target:");
    let mut changed = before.iter().zip(&after).filter(|(old, new)| old != new);
    before.len() == after.len()
        && changed
            .next()
            .is_some_and(|(old, new)| target(old) && target(new))
        && changed.next().is_none()
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
    trusted: Option<Oid>,
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
        trusted,
    )?;
    Ok(Some(ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion: proof.completion,
        home_anchor: None,
    }))
}

fn prove_task_completion(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    tip: Oid,
    default_tip: Oid,
    landed: Oid,
    parents: &[Oid],
) -> Result<CompletionProof, String> {
    let block = active_block(record).expect("active block checked");
    let introduced = introduced_at(repo, record, &block, landed)?;
    let anchor = source_landing_base(repo, landed, introduced)?.or(first_parent(repo, introduced)?);
    // Judge the entire task head that landed, including changes made
    // after its completion block was written. The block's introduction
    // alone cannot prove that the home accepted the reviewed result.
    let span = if parents.len() == 2 {
        let second = parents[1];
        if record_at(repo, second, record)?.is_none_or(|old| old.content != record.content) {
            return Err("the home merge does not carry the exact source record".into());
        }
        second
    } else {
        landed
    };
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
    if parents.len() == 2 {
        proof.landing_merge = Some(landed);
    }
    Ok(proof)
}

/// A correction landed no reviewed work, so the completion is proved where it
/// did land. This calls the proof directly: `prove_record` on the original
/// would read its old target as a retired home.
fn prove_corrected_completion(
    repo: &Repository,
    original: &RecordView,
    home: &str,
    tip: Oid,
    default_tip: Oid,
    before_correction: Oid,
) -> Result<CompletionProof, String> {
    let origin = matching_landing(repo, original, tip, before_correction, None)?
        .ok_or("the corrected record has no landing on the home")?;
    let commit = repo
        .find_commit(origin)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    if parents.len() > 1 && !is_clean_remerge(repo, &commit).map_err(|error| error.to_string())? {
        return Err("the home landing contains an unreviewed merge resolution".into());
    }
    prove_task_completion(repo, original, home, tip, default_tip, origin, &parents)
}

/// The walk of the default branch's home moves for a task homed there
/// (TSK-1378): the receiving line's state and the trusted default tip, and
/// the receiving line's name as the run gives it.
#[derive(Clone, Copy)]
struct Walk<'a> {
    receiver: Oid,
    default_tip: Oid,
    receiving: Option<&'a str>,
}

/// The home checks of one landing: a home correction (TSK-1185 AC-10), the
/// record's identity and home at the landing with the default branch's walk
/// (TSK-1378) or the trusted planning move (TSK-1185 AC-16), and, without a
/// walk, a home move an earlier commit made. With a walk, every home move of
/// the task on the default branch since the line's merge base with it is
/// already judged. Returns the walk's anchor and the record before a
/// correction.
#[allow(clippy::too_many_arguments)] // The landing, the range, the walk and the trusted tip.
fn verify_landing_home(
    repo: &Repository,
    record: &RecordView,
    (home, tip): (&str, Oid),
    landed: Oid,
    parents: &[Oid],
    (base, head): (Oid, Oid),
    walk: Option<Walk<'_>>,
    trusted: Option<Oid>,
) -> Result<(Option<HomeAnchor>, Option<RecordView>), String> {
    let correction = match parents.first() {
        Some(parent) => home_correction(repo, record, home, tip, landed, *parent)?,
        None => None,
    };
    let home_anchor = verify_record_home(
        repo,
        record,
        landed,
        parents,
        base,
        head,
        walk,
        (correction.is_some(), trusted),
    )?;
    if walk.is_none() {
        verify_earlier_home_move(repo, record, (home, tip), landed, (base, head), trusted)?;
    }
    Ok((home_anchor, correction))
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
    walk: Option<Walk<'_>>,
    trusted: Option<Oid>,
) -> Result<ImportedRecord, String> {
    let commit = repo
        .find_commit(landed)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    if parents.len() > 1 && !is_clean_remerge(repo, &commit).map_err(|error| error.to_string())? {
        return Err("the home landing contains an unreviewed merge resolution".into());
    }
    let (home_anchor, correction) = verify_landing_home(
        repo,
        record,
        (home, tip),
        landed,
        &parents,
        (base, head),
        walk,
        trusted,
    )?;
    let imported = |completion| ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion,
        home_anchor,
    };
    if let Some(original) = correction {
        let completion =
            prove_corrected_completion(repo, &original, home, tip, default_tip, parents[0])?;
        return Ok(imported(Some(completion)));
    }
    if let Some(proof) = prove_retired_record(
        repo,
        record,
        home,
        default_tip,
        landed,
        base,
        head,
        &parents,
        trusted,
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
            trusted,
        )?
        .completion
    } else if record.kind == RecordKind::Task && active_block(record).is_some() {
        Some(prove_task_completion(
            repo,
            record,
            home,
            tip,
            default_tip,
            landed,
            &parents,
        )?)
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
    Ok(imported(completion))
}

/// Prove the record's identity and home at its landing, and the accepted
/// criteria. With a walk, every home move of the task on the default
/// branch since the line's merge base with it is judged ([`home`]);
/// without one, the landing's own change from its first parent is.
#[allow(clippy::too_many_arguments)] // The landing, the range and the walk.
fn verify_record_home(
    repo: &Repository,
    record: &RecordView,
    landed: Oid,
    parents: &[Oid],
    base: Oid,
    head: Oid,
    walk: Option<Walk<'_>>,
    (corrected, trusted): (bool, Option<Oid>),
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
        // A home correction proved by `verify_landing_home` (AC-10) is
        // exempt from the last move's rules; every earlier move is judged.
        // A task the receiving line holds as its own takes the walk's moves
        // only from the verified tip (TSK-1185 AC-16).
        anchor = home::judge_home(
            repo,
            record,
            walk.default_tip,
            (walk.receiver, head),
            corrected.then_some(landed),
            (walk.receiving, trusted),
        )?;
    } else if record.kind == RecordKind::Task {
        if let Some(parent) = parents.first() {
            // The landing's own home change. Absent at its path and by
            // identity is an introduction; another path, or a side that
            // declares no home, is refused unless the landing is a proved
            // correction.
            if let Some(old) = version_before(repo, record, record, landed, *parent)? {
                let (was, now) = (declared_home(&old), declared_home(record));
                if was != now && !corrected {
                    if was.is_none() || now.is_none() {
                        return Err(home_undeclared(landed));
                    }
                    if !is_planning_move(repo, record, landed, *parent, head, trusted)? {
                        return Err(MOVED.into());
                    }
                }
            }
        }
    }
    // Divergent, independently introduced criteria do not become stale just
    // because the source has the same record as the receiving head.
    if record.kind == RecordKind::Task {
        use super::super::landing::{Authority, Identity, Outside, RecordStore};
        let mut store = RecordStore::new(repo);
        let judged = store.judge(
            &Identity::of(record),
            &[base, landed],
            &Outside {
                head,
                tips: &[],
                at: None,
                own: &[],
                replaced: None,
            },
        );
        match judged.criteria {
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
    mains: Option<&MainsBytes>,
    receiving: Option<&str>,
) -> Result<Vec<String>, String> {
    let repo = Repository::discover(root).map_err(|error| error.to_string())?;
    let resolve = |name: &str| {
        repo.revparse_single(name)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| error.to_string())
    };
    let graph = Graph::from_revision(&repo, head)?;
    Ok(imported_records(
        &repo,
        resolve(base)?,
        resolve(head)?,
        &graph,
        mains,
        receiving,
    )?
    .into_values()
    .map(|record| record.path)
    .collect())
}

#[cfg(test)]
mod earlier_tests;
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
                assert!(imported_records(&repo, head, head, &graph, None, None)
                    .unwrap()
                    .is_empty());
            }
            assert_eq!(COMPUTATIONS.with(std::cell::Cell::get), 1);
        }
        assert!(imported_records(&repo, head, head, &graph, None, None)
            .unwrap()
            .is_empty());
        assert_eq!(COMPUTATIONS.with(std::cell::Cell::get), 2);
    }
}
