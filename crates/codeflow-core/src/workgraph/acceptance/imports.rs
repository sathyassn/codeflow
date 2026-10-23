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
    let changed = merged_records(repo, base, head, graph)?;
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
            refuse_unpublished(repo, record, home, base, head)?;
        }
    }
    let mut homes = BTreeMap::from([(default.name.clone(), default_tip)]);
    for record in &changed {
        let Some(home) = record.integration_target.as_deref() else {
            continue;
        };
        let home = super::super::work_start::logical_target(home);
        if homes.contains_key(home) || !home.starts_with("integration/") {
            continue;
        }
        let Some(tip) = home_tip(repo, home)? else {
            // A first push still judges its own landings. A planning merge
            // shared by sibling lines need not have landed on the other home.
            // Neither case receives import authority from the local branch.
            if let Some(local) = reference_tip(repo, &format!("refs/heads/{home}"))? {
                if on_home_chain(repo, head, local)?
                    || matching_landing(repo, record, local, head, Some(base))?.is_none()
                {
                    continue;
                }
            }
            return Err(format!("{}: blocks come from target history, but home `{home}` is unavailable; fetch that home and its full history, then rerun the range", record.id));
        };
        if is_ancestor_or_same(repo, tip, base)? || on_home_chain(repo, head, tip)? {
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
        // A source already contained by the range base supplies no new imports.
        if is_ancestor_or_same(repo, tip, base)? || on_home_chain(repo, head, tip)? {
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
                && record
                    .integration_target
                    .as_deref()
                    .is_some_and(|declared| {
                        super::super::work_start::logical_target(declared) != home
                    })
            {
                continue;
            }
            let Some(landed) = matching_landing(repo, record, tip, head, Some(base))? else {
                continue;
            };
            if is_ancestor_or_same(repo, landed, base)? {
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
    format!("blocks come from target history, but published home `{home}` is unavailable; fetch refs/remotes/origin/{home} and its full history, then rerun")
}

/// Local branches are authority only in an offline repository with no remotes.
pub(super) fn home_tip(repo: &Repository, home: &str) -> Result<Option<Oid>, String> {
    let remote = format!("refs/remotes/origin/{home}");
    if let Some(tip) = reference_tip(repo, &remote)? {
        return Ok(Some(tip));
    }
    if !repo
        .remotes()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Ok(None);
    }
    reference_tip(repo, &format!("refs/heads/{home}"))
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
    let published = home_tip(repo, home)?;
    if published == Some(local) {
        return Ok(());
    }
    if let Some(landing) = matching_landing(repo, record, local, head, Some(base))? {
        if !published
            .map(|tip| is_ancestor_or_same(repo, landing, tip))
            .transpose()?
            .unwrap_or(false)
        {
            return Err(format!("{}: blocks come from target history on home `{home}`, but its exact record exists only on unpublished local history; land the reviewed work through the home's pull request, fetch refs/remotes/origin/{home}, then merge that published result", record.id));
        }
    }
    Ok(())
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
        let block = active_block(record).expect("active block checked");
        let introduced = introduced_at(repo, record, &block, landed)?;
        let anchor =
            source_landing_base(repo, landed, introduced)?.or(first_parent(repo, introduced)?);
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
            &run.criteria(),
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
        Some(proof)
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
                    super::super::work_start::logical_target(declared) != home
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
            Authority::Unknown(reason) => return Err(reason),
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
