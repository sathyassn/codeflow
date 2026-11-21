//! Read-only registry judgements: `ids check` (R-8, R-9, R-21), the merge
//! rule (R-2, R-14, R-111) and the uniqueness scan over every ref, the
//! safety net that holds even without a registry (ADR-0072 option 1b).
//! Nothing here fetches or writes (R-20: read commands never fetch).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::entry::{frontmatter_value, record_id_from_path, Kind, RegId, RECORD_ROOTS};
use super::git::{z_fields, Git};
use super::inventory::{self, branch_name, is_landing_branch};
use super::ledger::{short, Ledger};
use super::{tracking_ref, IdsError, AUTHORITY, REGISTRY_REF};

/// A judgement: what blocks, what warns, and what was read.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Informational lines.
    pub info: Vec<String>,
    /// Findings that fail the check.
    pub blocks: Vec<String>,
    /// Findings reported without failing.
    pub warns: Vec<String>,
}

impl Report {
    /// Whether the check passes.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// The registry ref to read: the fetched authority copy, else the local
/// branch (a repository with no remote).
///
/// # Errors
/// Returns an error when Git cannot inspect either registry ref.
pub fn registry_ref(git: &Git) -> Result<Option<String>, IdsError> {
    let tracking = tracking_ref(AUTHORITY);
    if git.rev(&tracking)?.is_some() {
        return Ok(Some(tracking));
    }
    Ok(git.rev(REGISTRY_REF)?.map(|_| REGISTRY_REF.to_string()))
}

/// What the standalone `codeflow ids check` found.
#[derive(Debug)]
pub enum Standalone {
    /// Durable work tracking is off, so no registry rule applies.
    TrackingOff,
    /// The registry check, and with a range the merge rule, judged.
    Judged(Report),
}

/// Why the standalone `codeflow ids check` could not finish.
#[derive(Debug)]
pub enum StandaloneError {
    /// Whether durable work tracking is on could not be read; the message
    /// is the full explanation.
    Tracking(String),
    /// The registry check could not run.
    Check(IdsError),
    /// The merge rule could not run.
    MergeRule(IdsError),
}

/// The standalone `codeflow ids check`: [`check`], then with a range
/// `(base, head)` the [`merge_rule`], its findings appended to the check's.
/// A blocking check report still runs the merge rule.
///
/// Both run in one read-only history judgment, as `codeflow ci` runs its
/// id-registry check, so the overlay and graph snapshot is read and verified
/// once per repository for the run instead of before every history query
/// (TSK-1520). Each spawned query still reads the replace refs, graft files
/// and shallow file for itself (`history::check_query`), and the graph
/// identity check still runs on every query.
///
/// # Errors
///
/// Returns which step could not run.
pub fn standalone(
    root: &Path,
    registry: Option<&str>,
    range: Option<(&str, &str)>,
) -> Result<Standalone, StandaloneError> {
    match crate::workgraph::durable_work_tracking_enabled(root) {
        Ok(true) => {}
        Ok(false) => return Ok(Standalone::TrackingOff),
        Err(error) => {
            return Err(StandaloneError::Tracking(
                crate::workgraph::work_start::tracking_state_message(error),
            ))
        }
    }
    let git = Git::new(root);
    // Read only: neither step writes the registry or any ref.
    let _judgment = crate::workgraph::history::judgment();
    let mut report = check(&git, registry).map_err(StandaloneError::Check)?;
    if let Some((base, head)) = range {
        let range = merge_rule(&git, base, head).map_err(StandaloneError::MergeRule)?;
        report.info.extend(range.info);
        report.warns.extend(range.warns);
        report.blocks.extend(range.blocks);
    }
    Ok(Standalone::Judged(report))
}

/// `ids check`: the registry's growth and damage rules, reconciled with the
/// record inventory on every ref.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn check(git: &Git, registry: Option<&str>) -> Result<Report, IdsError> {
    let mut report = Report::default();
    // Reconciling needs every introduction, and a shallow clone would
    // invent them at its boundary: fail closed instead (R-111).
    if git.is_shallow()? {
        report.blocks.push(IdsError::Shallow.to_string());
        return Ok(report);
    }
    let Some(registry) = (match registry {
        Some(value) => Some(value.to_string()),
        None => registry_ref(git)?,
    }) else {
        report.blocks.push(
            "no `codeflow/registry` found: CI fetches it explicitly; a maintainer seeds it once with `codeflow ids seed`"
                .to_string(),
        );
        return Ok(report);
    };
    let ledger = Ledger::read(git, &registry)?;
    let Some(tip) = &ledger.tip else {
        report
            .blocks
            .push(format!("registry ref {registry} does not resolve"));
        return Ok(report);
    };
    let count = ledger.ids().count();
    report.info.push(format!(
        "registry: {registry} tip {}, {} commits, {count} ids ever added",
        short(tip),
        ledger.commits
    ));
    if ledger.is_damaged() {
        report.blocks.extend(ledger.damage.iter().cloned());
        for finding in &ledger.violations {
            report.blocks.push(format!(
                "current damage: {}: {}",
                short(&finding.commit),
                finding.message
            ));
        }
    } else {
        for finding in &ledger.violations {
            report.warns.push(format!(
                "history (repaired): {}: {}",
                short(&finding.commit),
                finding.message
            ));
        }
    }
    for kind in Kind::ALL {
        let max = ledger.max_seq(kind)?;
        let held = ledger
            .ids()
            .filter(|id| id.kind() == kind && id.seq().is_some())
            .count();
        let gaps = usize::try_from(max)
            .unwrap_or(usize::MAX)
            .saturating_sub(held);
        if max > 0 {
            report.info.push(format!(
                "{kind}: highest {max}, {gaps} gap(s) never issued or abandoned (a gap is not damage)"
            ));
        }
    }
    reconcile(git, &ledger, &mut report)?;
    Ok(report)
}

/// Reconcile every ref's records with the registry (R-21).
fn reconcile(git: &Git, ledger: &Ledger, report: &mut Report) -> Result<(), IdsError> {
    let mut intro_cache: HashMap<String, BTreeMap<RegId, String>> = HashMap::new();
    let mut landing_cache: HashMap<(RegId, String), Option<String>> = HashMap::new();
    let tips = inventory::landing_tips(git)?;
    let mut unplaced: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for records in inventory::on_every_ref(git)? {
        let name = branch_name(&records.refname).to_string();
        let landing = is_landing_branch(&name);
        for copy in &records.copies {
            let entry = ledger.entry(&copy.id);
            let verdict = match (&copy.uid, entry) {
                (_, None) if ledger.holds(&copy.id) => format!(
                    "{} on {name} is held by an invalid registry entry; repair the registry before landing it (R-8)",
                    copy.id
                ),
                (_, None) => {
                    unplaced
                        .entry(copy.id.to_string())
                        .or_default()
                        .push(name.clone());
                    continue;
                }
                (Some(uid), Some(entry)) if *uid == entry.uid => continue,
                (Some(uid), Some(entry)) => format!(
                    "{} on {name} carries uid {uid}, but the registry binds {} to {}",
                    copy.id, copy.id, entry.uid
                ),
                (None, Some(entry)) => {
                    if !intro_cache.contains_key(&records.sha) {
                        intro_cache.insert(
                            records.sha.clone(),
                            inventory::introductions(git, &records.sha)?,
                        );
                    }
                    let intro = intro_cache[&records.sha].get(&copy.id).cloned();
                    let known = intro.as_ref().is_some_and(|intro| {
                        entry.introduced_sha() == Some(intro.as_str())
                            || entry.mapped.iter().any(|sha| sha == intro)
                            || entry.landed_sha() == Some(intro.as_str())
                    });
                    if known {
                        continue;
                    }
                    // R-111: a copy whose own nonempty landing equals the
                    // entry's nonempty `landed` is a replica, the same join
                    // seed made (a cherry-pick or rewrite of the landed file).
                    if let (Some(intro), Some(landed)) = (&intro, entry.landed_sha()) {
                        let key = (copy.id.clone(), intro.clone());
                        if !landing_cache.contains_key(&key) {
                            let found = inventory::landed_among(
                                git,
                                &tips,
                                &copy.id,
                                intro,
                                &mut intro_cache,
                            )?;
                            landing_cache.insert(key.clone(), found);
                        }
                        if landing_cache[&key].as_deref() == Some(landed) {
                            continue;
                        }
                    }
                    format!(
                        "{} on {name} has no uid and its introduction {} matches neither `introduced`, `mapped` nor `landed` of the registry entry",
                        copy.id,
                        intro.as_deref().map_or("unknown", short)
                    )
                }
            };
            if landing {
                report.blocks.push(format!("collision: {verdict}"));
            } else {
                report.warns.push(format!("collision: {verdict}"));
            }
        }
    }
    for (id, refs) in unplaced {
        report.warns.push(format!(
            "cannot place {id} (on {}): not in the registry; `codeflow ids admit` or `codeflow ids seed` registers it",
            refs.join(", ")
        ));
    }
    Ok(())
}

/// One record a range adds.
#[derive(Debug, Clone)]
struct Added {
    id: RegId,
    path: String,
    uid: Option<String>,
}

/// The record texts a range's merge rule reads, fetched in one
/// `git cat-file --batch` rather than a process per record.
struct Texts(HashMap<String, String>);

impl Texts {
    fn read(git: &Git, wanted: &[(&str, &str)]) -> Result<Texts, IdsError> {
        let specs: Vec<String> = wanted
            .iter()
            .map(|(rev, path)| format!("{rev}:{path}"))
            .collect();
        git.blobs(&specs)?
            .into_iter()
            .map(|(key, bytes)| {
                let text = String::from_utf8(bytes).map_err(|error| {
                    IdsError::Git(format!("record {key} is not valid UTF-8: {error}"))
                })?;
                Ok((key, text))
            })
            .collect::<Result<HashMap<_, _>, _>>()
            .map(Texts)
    }

    /// The `uid` of the record at `path` in `rev`, if it has one.
    fn uid_at(&self, rev: &str, path: &str) -> Option<String> {
        let text = self.0.get(&format!("{rev}:{path}"))?;
        frontmatter_value(text, "uid")
    }
}

/// The merge rule (R-2, R-14) and the uniqueness scan for the range
/// `base...head`: every record the range adds must be bound to its `uid`
/// in the registry (or, before a line's backfill, be a replica by
/// provenance), no existing record's `uid` may change, and no other ref may
/// hold a different record under the same id.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn merge_rule(git: &Git, base: &str, head: &str) -> Result<Report, IdsError> {
    let mut report = Report::default();
    let merge_base = git
        .run(&["merge-base", base, head])?
        .strip_suffix('\n')
        .unwrap_or("")
        .to_string();
    let mut args = vec![
        "diff",
        "--name-status",
        "-z",
        "--no-renames",
        merge_base.as_str(),
        head,
        "--",
    ];
    args.extend_from_slice(&RECORD_ROOTS);
    let diff = git.run_bytes(&args)?;
    let mut added_paths: Vec<(RegId, String)> = Vec::new();
    let mut removed: HashMap<RegId, String> = HashMap::new();
    let mut modified: Vec<(RegId, String)> = Vec::new();
    let fields = z_fields(&diff);
    let mut fields = fields.iter().map(String::as_str);
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let Some(id) = record_id_from_path(path) else {
            continue;
        };
        match status {
            "A" => added_paths.push((id, path.to_string())),
            "D" => {
                removed.insert(id, path.to_string());
            }
            _ => modified.push((id, path.to_string())),
        }
    }
    let mut wanted: Vec<(&str, &str)> = Vec::new();
    for (id, path) in &added_paths {
        wanted.push((head, path));
        if let Some(old) = removed.get(id) {
            wanted.push((&merge_base, old));
        }
    }
    for (_, path) in &modified {
        wanted.extend([(merge_base.as_str(), path.as_str()), (head, path.as_str())]);
    }
    let texts = Texts::read(git, &wanted)?;
    let mut added = Vec::new();
    let mut backfilled = Vec::new();
    for (id, path) in added_paths {
        if let Some(old) = removed.get(&id) {
            // A moved record keeps its identity: judge it as an edit.
            backfilled.extend(check_uid_edit(
                &texts,
                &merge_base,
                old,
                head,
                &path,
                &id,
                &mut report,
            ));
            continue;
        }
        let uid = texts.uid_at(head, &path);
        added.push(Added { id, path, uid });
    }
    for (id, path) in &modified {
        backfilled.extend(check_uid_edit(
            &texts,
            &merge_base,
            path,
            head,
            path,
            id,
            &mut report,
        ));
    }
    report.info.push(format!(
        "merge rule: {} record(s) added, {} uid(s) backfilled in {}..{}",
        added.len(),
        backfilled.len(),
        short(&merge_base),
        short(head)
    ));
    let mut intros = inventory::Introductions::default();
    if !added.is_empty() || !backfilled.is_empty() {
        let judged: Vec<Added> = added.iter().chain(&backfilled).cloned().collect();
        bind(git, &mut intros, head, &judged, &mut report)?;
    }
    if !added.is_empty() {
        scan(git, &mut intros, head, &added, &mut report)?;
    }
    Ok(report)
}

fn check_uid_edit(
    texts: &Texts,
    base: &str,
    old_path: &str,
    head: &str,
    new_path: &str,
    id: &RegId,
    report: &mut Report,
) -> Option<Added> {
    let Some(old) = texts.uid_at(base, old_path) else {
        // A record without a uid keeps its legacy allowance, but its first
        // uid (the backfill) must be the one the registry binds (R-2).
        return texts.uid_at(head, new_path).map(|uid| Added {
            id: id.clone(),
            path: new_path.to_string(),
            uid: Some(uid),
        });
    };
    match texts.uid_at(head, new_path) {
        Some(new) if new == old => {}
        Some(new) => report.blocks.push(format!(
            "{id}: the range edits the uid of an existing record ({old} -> {new}); a uid is written once (R-2)"
        )),
        None => report.blocks.push(format!(
            "{id}: the range removes the uid of an existing record ({old}) (R-2)"
        )),
    }
    None
}

fn bind(
    git: &Git,
    intros: &mut inventory::Introductions,
    head: &str,
    added: &[Added],
    report: &mut Report,
) -> Result<(), IdsError> {
    let Some(registry) = registry_ref(git)? else {
        report.blocks.push(format!(
            "{} record(s) added but no `codeflow/registry` was fetched: CI must fetch it explicitly with full history (R-20)",
            added.len()
        ));
        return Ok(());
    };
    let ledger = Ledger::read(git, &registry)?;
    for record in added {
        let id = &record.id;
        let entry = ledger.entry(id);
        match (&record.uid, entry) {
            (Some(uid), Some(entry)) if *uid == entry.uid => {
                report.info.push(format!("bound: {id} -> {uid}"));
            }
            (Some(uid), Some(entry)) => report.blocks.push(format!(
                "{id}: the registry binds it to uid {}, but {} carries {uid}; renumber with `codeflow ids retarget {id}` (R-14)",
                entry.uid, record.path
            )),
            (Some(uid), None) if ledger.holds(id) => report.blocks.push(format!(
                "{id}: the registry holds {id} under an invalid entry; {} (uid {uid}) cannot bind to it; retarget (R-14)",
                record.path
            )),
            (Some(uid), None) => report.blocks.push(format!(
                "{id}: not reserved in the registry; a maintainer admits uid {uid} with `codeflow ids admit {}` (R-14, R-17)",
                record.path
            )),
            (None, Some(entry)) => {
                if inventory::is_replica(git, intros, entry, head, id)? {
                    report
                        .info
                        .push(format!("bound by provenance: {id} (no uid before backfill)"));
                } else {
                    report.blocks.push(format!(
                        "{id}: {} has no uid and its provenance does not match the registry entry; it is a different record (R-111)",
                        record.path
                    ));
                }
            }
            (None, None) => report.blocks.push(format!(
                "{id}: {} has no uid and no registry entry; new records carry the uid `new` writes (R-1, R-14)",
                record.path
            )),
        }
    }
    Ok(())
}

/// The uniqueness scan: another ref holding the same id as a different
/// record blocks the range. A copy's identity is its `uid`; a copy without
/// one takes the `uid` of the registry entry it is a replica of, else its
/// introducing commit.
fn scan(
    git: &Git,
    intros: &mut inventory::Introductions,
    head: &str,
    added: &[Added],
    report: &mut Report,
) -> Result<(), IdsError> {
    let head_sha = git.rev(head)?.unwrap_or_default();
    let ledger = match registry_ref(git)? {
        Some(registry) => Ledger::read(git, &registry)?,
        None => Ledger::default(),
    };
    let mut identity =
        |rev: &str, id: &RegId, uid: Option<&String>| -> Result<Option<String>, IdsError> {
            if let Some(uid) = uid {
                return Ok(Some(uid.clone()));
            }
            if let Some(entry) = ledger.entry(id) {
                if inventory::is_replica(git, intros, entry, rev, id)? {
                    return Ok(Some(entry.uid.clone()));
                }
            }
            Ok(intros
                .of(git, rev, id)?
                .map(|sha| format!("introduced by {sha}")))
        };
    let mut ours = Vec::new();
    for record in added {
        ours.push(identity(&head_sha, &record.id, record.uid.as_ref())?);
    }
    let mut cache: HashMap<String, Vec<inventory::Copy>> = HashMap::new();
    for (refname, sha) in inventory::code_refs(git)? {
        if sha == head_sha {
            continue;
        }
        if !cache.contains_key(&sha) {
            cache.insert(sha.clone(), inventory::copies_at(git, &sha)?);
        }
        let copies = cache[&sha].clone();
        for (record, our) in added.iter().zip(&ours) {
            for copy in copies.iter().filter(|copy| copy.id == record.id) {
                let theirs = identity(&sha, &copy.id, copy.uid.as_ref())?;
                if theirs.is_none() || theirs != *our {
                    report.blocks.push(format!(
                        "uniqueness: {} also exists on {} as a different record ({}); renumber this one with `codeflow ids retarget {}`",
                        record.id,
                        branch_name(&refname),
                        copy.path,
                        record.id
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod r21_tests {
    #[test]
    fn r21_record_uid_cache_refuses_undecodable_text() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        crate::git::add_commit(
            &repo,
            &[(
                b"project-management/tasks/TSK-001.md",
                b"---\nid: TSK-001\nuid: caf\xff\n---\n",
            )],
        );
        assert!(super::Texts::read(
            &super::Git::new(dir.path()),
            &[("HEAD", "project-management/tasks/TSK-001.md")]
        )
        .is_err());
    }
}

#[cfg(test)]
mod r22_tests {
    use super::*;
    use crate::ids::r22_fixture::*;

    #[test]
    fn r22_uid_reader_missing_base_refuses_but_absent_uid_is_legacy() {
        let (dir, repo, _) = repository(TASK, b"---\nid: TSK-001\n---\n");
        let git = Git::new(dir.path());
        let path = std::str::from_utf8(TASK).unwrap();
        let texts = Texts::read(&git, &[("HEAD", path)]).unwrap();
        assert!(texts.uid_at("HEAD", path).is_none());
        remove_blob(&repo, b"---\nid: TSK-001\n---\n");
        assert!(Texts::read(&git, &[("HEAD", path)]).is_err());
    }

    #[test]
    fn r22_uid_edit_missing_base_cannot_be_backfill() {
        let (dir, repo, base) = repository(TASK, b"---\nid: TSK-001\n---\n");
        let head = crate::git::add_commit(&repo, &[(TASK, TEXT)]);
        remove_blob(&repo, b"---\nid: TSK-001\n---\n");
        assert!(matches!(
            merge_rule(&Git::new(dir.path()), &base.to_string(), &head.to_string()),
            Err(IdsError::Git(_))
        ));
    }

    #[test]
    fn r22_invalid_first_registry_entry_blocks_landing_copy() {
        let (dir, repo, _) = repository(b"ids/TSK/001.toml", b"broken = [");
        let git = Git::new(dir.path());
        let ledger = Ledger::read(&git, "HEAD").unwrap();
        crate::git::add_commit(&repo, &[(TASK, TEXT)]);
        let mut report = Report::default();
        reconcile(&git, &ledger, &mut report).unwrap();
        assert!(!report.blocks.is_empty(), "{report:?}");
        let mut absent = Report::default();
        reconcile(&git, &Ledger::default(), &mut absent).unwrap();
        assert!(absent.blocks.is_empty());
        assert!(!absent.warns.is_empty());
    }
}

/// TSK-1520: the standalone check verifies the commit graph once per run.
#[cfg(test)]
mod judgment_tests {
    use super::*;
    use crate::ids::judgment_fixture::{seeded, Repo};
    use crate::workgraph::history::{reset_snapshot_reads, snapshot_reads};

    fn judged(outcome: Result<Standalone, StandaloneError>) -> Report {
        match outcome.unwrap() {
            Standalone::Judged(report) => report,
            Standalone::TrackingOff => panic!("tracking is on in the fixture"),
        }
    }

    /// The calls the arm made before TSK-1520, with no judgment scope.
    fn unscoped(root: &Path, range: Option<(&str, &str)>) -> Report {
        let git = Git::new(root);
        let mut report = check(&git, None).unwrap();
        if let Some((base, head)) = range {
            let range = merge_rule(&git, base, head).unwrap();
            report.info.extend(range.info);
            report.warns.extend(range.warns);
            report.blocks.extend(range.blocks);
        }
        report
    }

    fn same(left: &Report, right: &Report) {
        assert_eq!(left.info, right.info);
        assert_eq!(left.warns, right.warns);
        assert_eq!(left.blocks, right.blocks);
    }

    #[test]
    fn the_range_entry_reads_the_snapshot_once() {
        let (repo, _) = seeded();
        let range = Some(("main", "feat/one"));
        // The fixture exercises history: without one scope the same calls
        // read the snapshot before every guarded query.
        reset_snapshot_reads();
        let before = unscoped(repo.root(), range);
        let unscoped_reads = snapshot_reads();
        assert!(unscoped_reads > 1, "{unscoped_reads}");
        reset_snapshot_reads();
        let report = judged(standalone(repo.root(), None, range));
        assert_eq!(snapshot_reads(), 1, "unscoped: {unscoped_reads}");
        // AC-2: the same findings in the same order.
        same(&report, &before);
        assert!(report.passed(), "{:?}", report.blocks);
        assert!(
            report
                .warns
                .iter()
                .any(|warn| warn.contains("collision: TSK-002 on feat/other")),
            "{:?}",
            report.warns
        );
        assert!(
            report
                .info
                .iter()
                .any(|line| line.starts_with("merge rule:")),
            "{:?}",
            report.info
        );
    }

    #[test]
    fn the_no_range_entry_reads_the_snapshot_once() {
        let (repo, _) = seeded();
        reset_snapshot_reads();
        let before = unscoped(repo.root(), None);
        let unscoped_reads = snapshot_reads();
        assert!(unscoped_reads > 1, "{unscoped_reads}");
        reset_snapshot_reads();
        let report = judged(standalone(repo.root(), None, None));
        assert_eq!(snapshot_reads(), 1, "unscoped: {unscoped_reads}");
        same(&report, &before);
    }

    #[test]
    fn a_blocking_range_still_runs_the_merge_rule_in_the_same_read() {
        let (repo, _) = seeded();
        // A different record on a landing line blocks the check.
        repo.git(&["branch", "integration/EPC-002-other", "feat/other"]);
        let range = Some(("main", "feat/other"));
        let before = unscoped(repo.root(), range);
        reset_snapshot_reads();
        let report = judged(standalone(repo.root(), None, range));
        assert_eq!(snapshot_reads(), 1);
        same(&report, &before);
        assert!(!report.passed());
        assert!(
            report
                .info
                .iter()
                .any(|line| line.starts_with("merge rule:")),
            "{:?}",
            report.info
        );
    }

    #[test]
    fn two_runs_each_read_the_snapshot_once() {
        let (repo, _) = seeded();
        for _ in 0..2 {
            reset_snapshot_reads();
            judged(standalone(repo.root(), None, Some(("main", "feat/two"))));
            assert_eq!(snapshot_reads(), 1);
        }
    }

    #[test]
    fn tracking_off_reads_no_snapshot() {
        let repo = Repo::new();
        reset_snapshot_reads();
        assert!(matches!(
            standalone(repo.root(), None, Some(("main", "main"))),
            Ok(Standalone::TrackingOff)
        ));
        assert_eq!(snapshot_reads(), 0);
    }

    #[test]
    fn no_registry_reads_no_snapshot_without_a_range() {
        let repo = Repo::new();
        repo.record("TSK-001", "first");
        repo.commit("a record");
        let before = unscoped(repo.root(), None);
        reset_snapshot_reads();
        let report = judged(standalone(repo.root(), None, None));
        assert_eq!(snapshot_reads(), 0);
        same(&report, &before);
        assert!(report.blocks[0].starts_with("no `codeflow/registry` found"));
    }

    #[test]
    fn a_shallow_clone_reads_no_snapshot_without_a_range_and_one_with_it() {
        let (repo, _) = seeded();
        repo.make_shallow();
        reset_snapshot_reads();
        let report = judged(standalone(repo.root(), None, None));
        assert_eq!(snapshot_reads(), 0);
        assert_eq!(report.blocks, vec![IdsError::Shallow.to_string()]);
        // The merge rule still runs and refuses as before, reading once.
        let git = Git::new(repo.root());
        let before = merge_rule(&git, "main", "feat/one")
            .unwrap_err()
            .to_string();
        reset_snapshot_reads();
        let Err(StandaloneError::MergeRule(error)) =
            standalone(repo.root(), None, Some(("main", "feat/one")))
        else {
            panic!("the merge rule refuses a shallow clone");
        };
        assert!(snapshot_reads() <= 1, "{}", snapshot_reads());
        assert_eq!(error.to_string(), before);
        assert!(error.to_string().contains("shallow"), "{error}");
    }
}

/// TSK-1520: inside one judgment the graph check is shared, but the overlay
/// state is read again for every spawned history query.
#[cfg(test)]
mod overlay_state_tests {
    use super::*;
    use crate::ids::judgment_fixture::{seeded, Repo};

    /// The seeded fixture plus `feat/new`, which adds a record whose uid
    /// the registry does not hold, so the range's merge rule blocks it.
    fn fixture() -> (Repo, String, String) {
        let (repo, base) = seeded();
        repo.git(&["checkout", "-q", "-b", "feat/new", "main"]);
        repo.write(
            "project-management/tasks/TSK-003.md",
            "---\nid: TSK-003\nuid: 00000000-0000-4000-8000-000000000003\ntitle: \"third\"\nstatus: todo\n---\n\n# TSK-003\n",
        );
        let head = repo.commit("feat/new: TSK-003");
        repo.git(&["checkout", "-q", "main"]);
        (repo, base, head)
    }

    /// Run the standalone steps in one judgment, changing the overlay
    /// state between the check and the merge rule.
    fn judged_with_change(repo: &Repo, change: impl FnOnce(&Repo)) -> Result<Report, IdsError> {
        let git = Git::new(repo.root());
        let _judgment = crate::workgraph::history::judgment();
        check(&git, None)?;
        change(repo);
        merge_rule(&git, "main", "feat/new")
    }

    #[test]
    fn the_merge_rule_blocks_the_unreserved_record_without_a_change() {
        let (repo, _, _) = fixture();
        let report = judged_with_change(&repo, |_| {}).unwrap();
        assert!(
            report.blocks.iter().any(|block| block.contains("TSK-003")),
            "{report:?}"
        );
    }

    #[test]
    fn a_replace_ref_added_during_a_run_refuses_the_next_query() {
        let (repo, _, head) = fixture();
        let tree = repo.git(&["rev-parse", "main^{tree}"]);
        let main = repo.git(&["rev-parse", "main"]);
        let stand_in = repo.git(&["commit-tree", &tree, "-p", &main, "-m", "stand-in"]);
        let error = judged_with_change(&repo, |repo| {
            repo.git(&["replace", &head, &stand_in]);
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("replace ref"), "{error}");
    }

    /// A replace ref added after the last guard is not read by a child:
    /// the unguarded `diff` the merge rule runs after `merge-base` still
    /// sees the recorded head.
    #[test]
    fn a_spawned_query_reads_the_recorded_objects() {
        let (repo, _, head) = fixture();
        let recorded = repo.git(&["rev-parse", &format!("{head}^{{tree}}")]);
        let tree = repo.git(&["rev-parse", "main^{tree}"]);
        let main = repo.git(&["rev-parse", "main"]);
        let stand_in = repo.git(&["commit-tree", &tree, "-p", &main, "-m", "stand-in"]);
        repo.git(&["replace", &head, &stand_in]);
        assert_eq!(
            repo.git(&["rev-parse", &format!("{head}^{{tree}}")]),
            tree,
            "the fixture's own Git reads the replacement"
        );
        let seen = Git::new(repo.root())
            .run(&["rev-parse", &format!("{head}^{{tree}}")])
            .unwrap();
        assert_eq!(seen.trim(), recorded);
    }

    #[test]
    fn a_graft_file_added_during_a_run_refuses_the_next_query() {
        let (repo, base, head) = fixture();
        let error = judged_with_change(&repo, |repo| {
            repo.write(".git/info/grafts", &format!("{head} {base}\n"));
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("graft file"), "{error}");
    }

    #[test]
    fn a_shallow_file_added_during_a_run_refuses_the_next_query() {
        let (repo, base, _) = fixture();
        let error = judged_with_change(&repo, |repo| {
            repo.write(".git/shallow", &format!("{base}\n"));
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("shallow"), "{error}");
    }
}
