//! Source attribution for unchanged records imported by an ordinary merge.
use std::collections::BTreeMap;

use super::{
    active_block, bind_completion_with_amendment, bind_epic_completion, commit_of,
    default_target_name, first_parent, history, introduced_at, is_ancestor_or_same,
    is_clean_remerge, own_file_name, owned_paths, presence_at, record_at, record_uid,
    source_landing_base, CompletionProof, Graph, Landing, Oid, Presence, RecordKind, RecordView,
    Repository, RunBases, Transport,
};

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
}

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
    let anchor = history::merge_base(repo, base, head).map_err(|error| error.to_string())?;
    let changed = merged_records(repo, anchor, head, graph)?;
    if changed.is_empty() {
        return Ok(BTreeMap::new());
    }
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    let default = default_target_name(repo)?;
    let default_tip =
        home_tip(repo, &default.name)?.ok_or_else(|| unavailable_home(&default.name))?;
    for record in &changed {
        let home = record.integration_target.as_deref().map_or(
            default.name.as_str(),
            super::super::work_start::logical_target,
        );
        if home == default.name || home.starts_with("integration/") {
            refuse_unpublished(repo, record, home, default_tip, anchor, head)?;
        }
    }
    let mut homes = BTreeMap::from([(default.name.clone(), default_tip)]);
    let mut retired = BTreeMap::new();
    for record in &changed {
        let Some(home) = record.integration_target.as_deref() else {
            continue;
        };
        let home = super::super::work_start::logical_target(home);
        if homes.contains_key(home) || !home.starts_with("integration/") {
            continue;
        }
        let Some(tip) = home_tip(repo, home)? else {
            // A retired line's exact record can still be proved at its
            // published default-target landing. The proof below judges the
            // landing merge and the source span, not record equality alone.
            if let Some(landing) = matching_landing(repo, record, default_tip, head, Some(anchor))?
            {
                retired.insert(record.id.clone(), landing);
                continue;
            }
            // Unpublished local landings were refused above. A remaining
            // local branch supplies no import authority (for example a first
            // push of this very line, or a shared planning merge).
            if reference_tip(repo, &format!("refs/heads/{home}"))?.is_some() {
                continue;
            }
            return Err(format!("{}: blocks come from target history, but home `{home}` is unavailable and no exact record landing is proven on published default `{}`; fetch the home or default target and its full history, then rerun the range", record.id, default.name));
        };
        if is_ancestor_or_same(repo, tip, anchor)? || on_home_chain(repo, head, tip)? {
            continue;
        }
        match super::super::work_start::check_epic_line(
            root,
            home,
            &default.name,
            &default_tip.to_string(),
            &tip.to_string(),
        ) {
            Ok(_) => {
                homes.insert(home.to_string(), tip);
            }
            Err(reason) => {
                return Err(format!("{}: blocks come from target history on unverified home `{home}`: {reason}; repair the home through its classified pull requests, then merge its reviewed result", record.id));
            }
        }
    }
    let mut imported = BTreeMap::new();
    for (home, tip) in homes {
        // A source already contained by the shared anchor supplies no new imports.
        if is_ancestor_or_same(repo, tip, anchor)? || on_home_chain(repo, head, tip)? {
            continue;
        }
        let owned = owned_paths(
            root,
            &head.to_string(),
            &[&base.to_string(), &tip.to_string()],
        )?;
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
                Some(landing) => Some(*landing),
                None => matching_landing(repo, record, tip, head, Some(anchor))?,
            };
            let Some(landed) = landed else {
                continue;
            };
            if !is_new_import(repo, record, landed, anchor, head)? {
                continue;
            }
            let result = prove_record(repo, record, &home, tip, default_tip, landed, base, head)
                .map_err(|reason| format!("{}: blocks come from target history on `{home}` at {landed}: {reason}; fetch the home and repair or reopen the work there through its task pull request, then merge the reviewed result", record.id))?;
            imported.insert(record.id.clone(), result);
        }
    }
    Ok(imported)
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
/// (TSK-1004): the merge that introduced this task record, carried on the
/// receiving head's own first-parent chain with the record unchanged at
/// every step since, and a version that holds no completion or acceptance
/// evidence. Two lines that branch from one planning merge both hold it,
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
    if record.kind != RecordKind::Task
        || !matches!(record.status.as_str(), "todo" | "blocked")
        || !record.active_blocks().is_empty()
        || !record.superseded_blocks().is_empty()
    {
        return Ok(false);
    }
    let commit = repo
        .find_commit(landing)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    let [first, _, ..] = parents.as_slice() else {
        return Ok(false);
    };
    let uid = record_uid(&record.content);
    if !matches!(
        presence_at(
            repo,
            *first,
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
    )?;
    Ok(Some(ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion: proof.completion,
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
) -> Result<ImportedRecord, String> {
    let commit = repo
        .find_commit(landed)
        .map_err(|error| error.to_string())?;
    let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
    if parents.len() > 1 && !is_clean_remerge(repo, &commit).map_err(|error| error.to_string())? {
        return Err("the home landing contains an unreviewed merge resolution".into());
    }
    verify_record_home(repo, record, home, landed, &parents, base, head)?;
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
    Ok(ImportedRecord {
        path: record.path.clone(),
        home: home.to_string(),
        landing: landed,
        completion,
    })
}

fn verify_record_home(
    repo: &Repository,
    record: &RecordView,
    home: &str,
    landed: Oid,
    parents: &[Oid],
    base: Oid,
    head: Oid,
) -> Result<(), String> {
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
        if let Some(parent) = parents.first() {
            if record_at(repo, *parent, record)?.is_some_and(|old| {
                old.integration_target.as_deref().is_some_and(|declared| {
                    super::super::work_start::logical_target(declared)
                        != record
                            .integration_target
                            .as_deref()
                            .map_or(home, super::super::work_start::logical_target)
                })
            }) {
                return Err("the landing changes the task's declared home".into());
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
    Ok(())
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
