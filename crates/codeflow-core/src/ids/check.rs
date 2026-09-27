//! Read-only registry judgements: `ids check` (R-8, R-9, R-21), the merge
//! rule (R-2, R-14, R-111) and the uniqueness scan over every ref, the
//! safety net that holds even without a registry (ADR-0072 option 1b).
//! Nothing here fetches or writes (R-20: read commands never fetch).

use std::collections::{BTreeMap, HashMap};

use super::entry::{frontmatter_value, record_id_from_path, Kind, RegId, RECORD_ROOTS};
use super::git::Git;
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
#[must_use]
pub fn registry_ref(git: &Git) -> Option<String> {
    let tracking = tracking_ref(AUTHORITY);
    if git.rev(&tracking).is_some() {
        return Some(tracking);
    }
    git.rev(REGISTRY_REF).map(|_| REGISTRY_REF.to_string())
}

/// `ids check`: the registry's growth and damage rules, reconciled with the
/// record inventory on every ref.
///
/// # Errors
///
/// Returns an error when git fails.
pub fn check(git: &Git, registry: Option<&str>) -> Result<Report, IdsError> {
    let mut report = Report::default();
    let Some(registry) = registry.map(str::to_string).or_else(|| registry_ref(git)) else {
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
        let max = ledger.max_seq(kind);
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
    let mut unplaced: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for records in inventory::on_every_ref(git)? {
        let name = branch_name(&records.refname).to_string();
        let landing = is_landing_branch(&name);
        for copy in &records.copies {
            let entry = ledger.entry(&copy.id);
            let verdict = match (&copy.uid, entry) {
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
                    let intro = intro_cache[&records.sha].get(&copy.id);
                    let known = intro.is_some_and(|intro| {
                        entry.introduced_sha() == Some(intro.as_str())
                            || entry.mapped.iter().any(|sha| sha == intro)
                            || entry.landed_sha() == Some(intro.as_str())
                    });
                    if known {
                        continue;
                    }
                    format!(
                        "{} on {name} has no uid and its introduction {} matches neither `introduced`, `mapped` nor `landed` of the registry entry",
                        copy.id,
                        intro.map_or("unknown", |sha| short(sha))
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

fn uid_at(git: &Git, rev: &str, path: &str) -> Option<String> {
    let text = git.run(&["show", &format!("{rev}:{path}")]).ok()?;
    frontmatter_value(&text, "uid")
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
    let merge_base = git.run(&["merge-base", base, head])?.trim().to_string();
    let mut args = vec![
        "diff",
        "--name-status",
        "--no-renames",
        merge_base.as_str(),
        head,
        "--",
    ];
    args.extend_from_slice(&RECORD_ROOTS);
    let diff = git.run(&args)?;
    let mut added_paths: Vec<(RegId, String)> = Vec::new();
    let mut removed: HashMap<RegId, String> = HashMap::new();
    let mut modified: Vec<(RegId, String)> = Vec::new();
    for line in diff.lines() {
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
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
    let mut added = Vec::new();
    for (id, path) in added_paths {
        if let Some(old) = removed.get(&id) {
            // A moved record keeps its identity: judge it as an edit.
            check_uid_edit(git, &merge_base, old, head, &path, &id, &mut report);
            continue;
        }
        let uid = uid_at(git, head, &path);
        added.push(Added { id, path, uid });
    }
    for (id, path) in &modified {
        check_uid_edit(git, &merge_base, path, head, path, id, &mut report);
    }
    report.info.push(format!(
        "merge rule: {} record(s) added in {}..{}",
        added.len(),
        short(&merge_base),
        short(head)
    ));
    if !added.is_empty() {
        bind(git, head, &added, &mut report)?;
        scan(git, head, &added, &mut report)?;
    }
    Ok(report)
}

fn check_uid_edit(
    git: &Git,
    base: &str,
    old_path: &str,
    head: &str,
    new_path: &str,
    id: &RegId,
    report: &mut Report,
) {
    let Some(old) = uid_at(git, base, old_path) else {
        return;
    };
    match uid_at(git, head, new_path) {
        Some(new) if new == old => {}
        Some(new) => report.blocks.push(format!(
            "{id}: the range edits the uid of an existing record ({old} -> {new}); a uid is written once (R-2)"
        )),
        None => report.blocks.push(format!(
            "{id}: the range removes the uid of an existing record ({old}) (R-2)"
        )),
    }
}

fn bind(git: &Git, head: &str, added: &[Added], report: &mut Report) -> Result<(), IdsError> {
    let Some(registry) = registry_ref(git) else {
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
                if inventory::is_replica(git, entry, head, id)? {
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
fn scan(git: &Git, head: &str, added: &[Added], report: &mut Report) -> Result<(), IdsError> {
    let head_sha = git.rev(head).unwrap_or_default();
    let ledger = match registry_ref(git) {
        Some(registry) => Ledger::read(git, &registry)?,
        None => Ledger::default(),
    };
    let mut intro_cache: HashMap<String, BTreeMap<RegId, String>> = HashMap::new();
    let mut identity =
        |rev: &str, id: &RegId, uid: Option<&String>| -> Result<Option<String>, IdsError> {
            if let Some(uid) = uid {
                return Ok(Some(uid.clone()));
            }
            if let Some(entry) = ledger.entry(id) {
                if inventory::is_replica(git, entry, rev, id)? {
                    return Ok(Some(entry.uid.clone()));
                }
            }
            if !intro_cache.contains_key(rev) {
                intro_cache.insert(rev.to_string(), inventory::introductions(git, rev)?);
            }
            Ok(intro_cache[rev]
                .get(id)
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
