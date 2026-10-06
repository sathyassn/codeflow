//! Seeding, backfill and retarget (SPC-013 R-16, R-24, R-25, R-111).
//!
//! `ids seed` registers every id found on every ref and in retained history,
//! once, with its provenance; `ids backfill` copies the registered `uid` into
//! the records of the current line; `ids retarget` renumbers an unmerged
//! record whose number is held by another record.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use super::entry::{frontmatter_value, new_uid, record_id_from_path, Entry, RegId, RECORD_ROOTS};
use super::git::{z_fields, Git};
use super::inventory::{self, branch_name, is_landing_branch};
use super::issue::{self, Fetched, Pushed, Request};
use super::ledger::{short, Ledger};
use super::{state, tracking_ref, IdsError, Standing, AUTHORITY, PUSH_ATTEMPTS, REGISTRY_REF};

/// A maintainer's explicit mapping: for each id, the introducing shas of
/// copies that are one record (`ids seed --map`).
///
/// ```toml
/// [ids."TSK-050"]
/// copies = ["<introducing sha>", "<introducing sha>"]
/// ```
#[derive(Debug, Default, Clone)]
pub struct SeedMap {
    copies: BTreeMap<RegId, BTreeSet<String>>,
    /// A digest of the map file, cited in the seed commit.
    pub digest: String,
}

impl SeedMap {
    /// Parse a map file.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid TOML or an unknown id.
    pub fn parse(text: &str) -> Result<SeedMap, IdsError> {
        let table: toml::Table = text
            .parse()
            .map_err(|error| IdsError::Invalid(format!("seed map: {error}")))?;
        let mut copies = BTreeMap::new();
        if let Some(ids) = table.get("ids").and_then(toml::Value::as_table) {
            for (key, value) in ids {
                let id = RegId::parse(key).ok_or_else(|| {
                    IdsError::Invalid(format!("seed map: {key} is not a record id"))
                })?;
                let shas: BTreeSet<String> = value
                    .get("copies")
                    .and_then(toml::Value::as_array)
                    .ok_or_else(|| IdsError::Invalid(format!("seed map: {key} lacks copies")))?
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect();
                copies.insert(id, shas);
            }
        }
        let digest = crate::scaffold::sha256_hex(text.as_bytes());
        Ok(SeedMap { copies, digest })
    }
}

/// One copy of an id found while seeding: where it was read and how it
/// was introduced.
#[derive(Debug, Clone)]
struct Found {
    refname: String,
    intro: String,
    landing: bool,
    /// Whether the ref's tip still holds the id; `false` when the record
    /// was renamed or deleted on that line and survives only in history.
    at_tip: bool,
    uid: Option<String>,
}

/// What a seed did.
#[derive(Debug, Default)]
pub struct SeedReport {
    pub registered: Vec<RegId>,
    pub already: usize,
    pub standing: Option<Standing>,
}

/// `ids seed` (R-24, R-111).
///
/// # Errors
///
/// Returns [`IdsError::Clash`] naming every id whose copies provenance
/// cannot decide, and [`IdsError::Shallow`] on a shallow clone, without
/// writing anything; or a push failure.
pub fn seed(root: &Path, map: Option<&SeedMap>) -> Result<SeedReport, IdsError> {
    let git = Git::new(root);
    let _lock = state::lock(&git)?;
    seed_locked(&git, map)
}

/// [`seed`] under a lock the caller holds.
pub(super) fn seed_locked(git: &Git, map: Option<&SeedMap>) -> Result<SeedReport, IdsError> {
    // A seed is permanent, so it never reads introductions from a shallow
    // clone, whose boundary commits look like adds (R-111).
    if git.is_shallow()? {
        return Err(IdsError::Shallow);
    }
    let online = git.has_remote(AUTHORITY);
    let tracking = tracking_ref(AUTHORITY);
    let mut reason = String::new();
    for _ in 0..PUSH_ATTEMPTS {
        if online {
            if let Fetched::Offline(reason) = issue::fetch(git, AUTHORITY)? {
                return Err(IdsError::Offline(reason));
            }
            issue::verify_descent(git, AUTHORITY)?;
        }
        let registry = if online {
            tracking.clone()
        } else {
            REGISTRY_REF.to_string()
        };
        let ledger = Ledger::read(git, &registry)?;
        if ledger.is_damaged() {
            return Err(IdsError::Damaged(ledger.damage.clone()));
        }
        let entries = plan(git, &ledger, map)?;
        let mut report = SeedReport {
            already: ledger.ids().count(),
            ..SeedReport::default()
        };
        if entries.is_empty() {
            return Ok(report);
        }
        let mut files = Vec::new();
        for entry in &entries {
            files.push((
                entry.id.registry_path(),
                git.write_blob(entry.render().as_bytes())?,
            ));
        }
        let message = match map {
            Some(map) => format!("seed: {} ids (map sha256:{})", entries.len(), map.digest),
            None => format!("seed: {} ids", entries.len()),
        };
        let base = ledger.tip.clone();
        let commit = git.commit_files(base.as_deref(), &files, &message)?;
        report.registered = entries.iter().map(|entry| entry.id.clone()).collect();
        if !online {
            git.run(&[
                "update-ref",
                "-m",
                "codeflow ids seed",
                REGISTRY_REF,
                &commit,
            ])?;
            report.standing = Some(Standing::Local);
            return Ok(report);
        }
        match issue::push(git, AUTHORITY, &commit)? {
            Pushed::Accepted => {
                issue::adopt(git, AUTHORITY, &commit)?;
                report.standing = Some(Standing::Reserved);
                return Ok(report);
            }
            Pushed::Moved(why) => reason = why,
            Pushed::Failed(error) => return Err(error),
            // Nothing is assumed: seeding again is idempotent once the
            // authority answers.
            Pushed::Unclear(why) => return Err(IdsError::Transport(why)),
        }
    }
    Err(IdsError::Contended(PUSH_ATTEMPTS, reason))
}

/// The entries a seed would add: every id on every ref or in retained
/// history that the registry does not hold yet.
fn plan(git: &Git, ledger: &Ledger, map: Option<&SeedMap>) -> Result<Vec<Entry>, IdsError> {
    let mapped_by = format!("{} {}", git.user_email(), super::today());
    let mut entries = Vec::new();
    let mut undecided = Vec::new();
    for (id, copies) in copies_on_refs(git)? {
        if ledger.holds(&id) {
            continue;
        }
        match decide(git, &id, &live(copies), map, &mapped_by)? {
            Ok(entry) => entries.push(entry),
            Err(reason) => undecided.push(reason),
        }
    }
    if !undecided.is_empty() {
        return Err(IdsError::Clash(format!(
            "seed stopped; nothing was written. These ids have copies that are not provably one record:\n  {}\nretarget one of two unrelated records, or map copies of one record with `codeflow ids seed --map <file>`",
            undecided.join("\n  ")
        )));
    }
    Ok(entries)
}

/// The copies that decide an id: a copy no tip holds any more (its record
/// was renamed or deleted on that line) is history, not a live copy, while
/// some tip holds the id. With no tip holding it, every copy counts.
fn live(copies: Vec<Found>) -> Vec<Found> {
    if copies.iter().any(|copy| copy.at_tip) {
        copies.into_iter().filter(|copy| copy.at_tip).collect()
    } else {
        copies
    }
}

/// Every copy of every id on every code ref, with its introduction.
fn copies_on_refs(git: &Git) -> Result<BTreeMap<RegId, Vec<Found>>, IdsError> {
    let mut found: BTreeMap<RegId, Vec<Found>> = BTreeMap::new();
    let mut intro_cache: HashMap<String, BTreeMap<RegId, String>> = HashMap::new();
    let mut uid_cache: HashMap<String, Vec<inventory::Copy>> = HashMap::new();
    let mut refs = inventory::code_refs(git)?;
    // Landing lines first, so a record's introduction is read from the line
    // it landed on when several refs hold it.
    refs.sort_by_key(|(name, _)| (!is_landing_branch(branch_name(name)), name.clone()));
    for (refname, sha) in &refs {
        if !intro_cache.contains_key(sha) {
            intro_cache.insert(sha.clone(), inventory::introductions(git, sha)?);
            uid_cache.insert(sha.clone(), inventory::copies_at(git, sha)?);
        }
        let landing = is_landing_branch(branch_name(refname));
        for (id, intro) in &intro_cache[sha] {
            let copy = uid_cache[sha].iter().find(|copy| &copy.id == id);
            found.entry(id.clone()).or_default().push(Found {
                refname: branch_name(refname).to_string(),
                intro: intro.clone(),
                landing,
                at_tip: copy.is_some(),
                uid: copy.and_then(|copy| copy.uid.clone()),
            });
        }
    }
    Ok(found)
}

/// Decide one id: the entry to register, or why provenance cannot decide.
/// Copies are one record when they share an introducing commit, or when one
/// copy's landing is another's (R-111); a map joins the rest explicitly.
fn decide(
    git: &Git,
    id: &RegId,
    copies: &[Found],
    map: Option<&SeedMap>,
    mapped_by: &str,
) -> Result<Result<Entry, String>, IdsError> {
    let intros: BTreeSet<&String> = copies.iter().map(|copy| &copy.intro).collect();
    let mut landed: BTreeMap<String, Option<String>> = BTreeMap::new();
    for intro in &intros {
        landed.insert((*intro).clone(), inventory::landed_for(git, id, intro)?);
    }
    let mut records: Vec<BTreeSet<String>> = Vec::new();
    for intro in intros {
        let key = landed[intro].clone();
        let joins = |other: &String| {
            other == intro
                || key.is_some() && landed[other] == key
                || key.as_deref() == Some(other.as_str())
        };
        if let Some(record) = records.iter_mut().find(|record| record.iter().any(joins)) {
            record.insert(intro.clone());
        } else {
            records.push(BTreeSet::from([intro.clone()]));
        }
    }
    let mut by = "none".to_string();
    if records.len() > 1 {
        let covered = map
            .and_then(|map| map.copies.get(id))
            .is_some_and(|shas| records.iter().flatten().all(|intro| shas.contains(intro)));
        if !covered {
            let groups: Vec<String> = records
                .iter()
                .map(|record| {
                    record
                        .iter()
                        .map(|sha| short(sha))
                        .collect::<Vec<_>>()
                        .join("+")
                })
                .collect();
            return Ok(Err(format!(
                "{id}: {} copies that provenance cannot decide ({})",
                records.len(),
                groups.join(", ")
            )));
        }
        by = mapped_by.to_string();
        records = vec![records.into_iter().flatten().collect()];
    }
    let record = &records[0];
    let members: Vec<&Found> = copies
        .iter()
        .filter(|copy| record.contains(&copy.intro))
        .collect();
    // The introduction to register: one that landed, else the first copy
    // read (landing lines are read first).
    let Some(chosen) = members
        .iter()
        .min_by_key(|copy| (!copy.landing, landed[&copy.intro].is_none()))
    else {
        return Ok(Err(format!("{id}: no copy to register")));
    };
    let uids: BTreeSet<&String> = members
        .iter()
        .filter_map(|copy| copy.uid.as_ref())
        .collect();
    if uids.len() > 1 {
        return Ok(Err(format!(
            "{id}: its copies carry {} different uids",
            uids.len()
        )));
    }
    let mapped: Vec<String> = if by == "none" {
        Vec::new()
    } else {
        record
            .iter()
            .filter(|sha| **sha != chosen.intro)
            .cloned()
            .collect()
    };
    Ok(Ok(Entry {
        id: id.clone(),
        uid: uids.into_iter().next().cloned().unwrap_or_else(new_uid),
        title: title_of(git, &chosen.intro, id).unwrap_or_default(),
        issuer: "seed".to_string(),
        created: crate::workgraph::now_rfc3339(),
        target: chosen.refname.clone(),
        introduced: format!("{}@{}", chosen.intro, chosen.refname),
        landed: landed[&chosen.intro]
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        mapped,
        mapped_by: by,
    }))
}

fn title_of(git: &Git, commit: &str, id: &RegId) -> Option<String> {
    let (_, blob) = inventory::added_records(git, commit)
        .ok()?
        .into_iter()
        .find(|(path, _)| record_id_from_path(path).as_ref() == Some(id))?;
    let text = git.run(&["cat-file", "blob", &blob]).ok()?;
    frontmatter_value(&text, "title")
}

/// The record files in the working tree, by id.
fn worktree_records(root: &Path) -> BTreeMap<RegId, Vec<PathBuf>> {
    let mut out: BTreeMap<RegId, Vec<PathBuf>> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = RECORD_ROOTS.iter().map(|base| root.join(base)).collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Never follow a link below a record root. A linked root itself
            // is read here; its writes are refused (issue 94).
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if let Some(id) = record_id_from_path(&relative) {
                out.entry(id).or_default().push(path);
            }
        }
    }
    out
}

/// Insert `key: value` after the frontmatter's `after` line.
fn insert_after(text: &str, after: &str, line: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() + line.len() + 1);
    let mut inserted = false;
    let mut in_front = false;
    for (index, current) in text.split_inclusive('\n').enumerate() {
        out.push_str(current);
        let bare = current.trim_end_matches(['\r', '\n']);
        if index == 0 {
            in_front = bare == "---";
            continue;
        }
        if in_front && !inserted && bare.starts_with(&format!("{after}:")) {
            out.push_str(line);
            out.push('\n');
            inserted = true;
        }
        if in_front && bare == "---" {
            in_front = false;
        }
    }
    inserted.then_some(out)
}

/// What a backfill did.
#[derive(Debug, Default)]
pub struct BackfillReport {
    pub written: Vec<RegId>,
    pub refused: Vec<String>,
}

/// `ids backfill` (R-25): copy the registered `uid` into every record on the
/// current line that lacks one. It never generates or changes a `uid`.
///
/// # Errors
///
/// Returns an error when git fails or no registry is available.
pub fn backfill(root: &Path) -> Result<BackfillReport, IdsError> {
    let git = Git::new(root);
    let Some(registry) = super::check::registry_ref(&git) else {
        return Err(IdsError::Invalid(
            "no `codeflow/registry` to backfill from; fetch it or seed it first".to_string(),
        ));
    };
    let ledger = Ledger::read(&git, &registry)?;
    let mut report = BackfillReport::default();
    let mut intros = inventory::Introductions::default();
    // Records are read and written beneath the root without following a
    // link (issue 94).
    let tree = crate::contained::Tree::open(root)?;
    for (id, paths) in worktree_records(root) {
        for path in paths {
            let relative = crate::contained::relative_to(root, &path)?;
            let text = String::from_utf8(tree.read(&relative, u64::MAX)?)
                .map_err(|error| IdsError::Invalid(format!("{relative}: {error}")))?;
            if frontmatter_value(&text, "uid").is_some() {
                continue;
            }
            let Some(entry) = ledger.entry(&id) else {
                report
                    .refused
                    .push(format!("{id}: no registry entry; seed or admit it first"));
                continue;
            };
            if !inventory::is_replica(&git, &mut intros, entry, "HEAD", &id)? {
                report.refused.push(format!(
                    "{id}: this line's copy is not the registered record (provenance differs); retarget it"
                ));
                continue;
            }
            let Some(updated) = insert_after(&text, "id", &format!("uid: {}", entry.uid)) else {
                report.refused.push(format!(
                    "{id}: {} has no `id:` frontmatter line",
                    path.display()
                ));
                continue;
            };
            tree.write(&relative, updated.as_bytes())?;
            report.written.push(id.clone());
        }
    }
    Ok(report)
}

/// What a retarget did.
#[derive(Debug)]
pub struct Retarget {
    pub from: RegId,
    pub to: RegId,
    pub path: PathBuf,
    pub standing: Standing,
    pub rewritten: Vec<PathBuf>,
}

/// `ids retarget <id>` (R-16): renumber an unmerged record to the next free
/// number, rewrite its in-tree links and record the old id in `former_ids`.
///
/// # Errors
///
/// Refuses a record already landed on a landing line, a record without a
/// `uid`, and a record the registry already binds to its number.
pub fn retarget(root: &Path, from: &RegId) -> Result<Retarget, IdsError> {
    let git = Git::new(root);
    let records = worktree_records(root);
    let paths = records
        .get(from)
        .ok_or_else(|| IdsError::Invalid(format!("{from} is not a record in this working tree")))?;
    let [path] = paths.as_slice() else {
        return Err(IdsError::Invalid(format!(
            "{from} has several files in this working tree"
        )));
    };
    let text = std::fs::read_to_string(path)?;
    let uid = frontmatter_value(&text, "uid").ok_or_else(|| {
        IdsError::Invalid(format!(
            "{from} has no uid; run `codeflow ids backfill` first"
        ))
    })?;
    for (refname, sha) in inventory::code_refs(&git)? {
        if !is_landing_branch(branch_name(&refname)) {
            continue;
        }
        if inventory::copies_at(&git, &sha)?
            .iter()
            .any(|copy| &copy.id == from && copy.uid.as_deref() == Some(uid.as_str()))
        {
            return Err(IdsError::Invalid(format!(
                "{from} has landed on {}; a landed record is never renumbered",
                branch_name(&refname)
            )));
        }
    }
    let registry = super::check::registry_ref(&git);
    let ledger = match &registry {
        Some(registry) => Ledger::read(&git, registry)?,
        None => Ledger::default(),
    };
    if ledger.entry(from).is_some_and(|entry| entry.uid == uid) {
        return Err(IdsError::Invalid(format!(
            "the registry already binds {from} to this record; there is nothing to retarget"
        )));
    }
    issue::drop_pending(root, from)?;
    let bound = ledger
        .ids_for_uid(&uid)
        .into_iter()
        .find(|id| id != from && id.kind() == from.kind());
    let (to, standing) = if let Some(id) = bound {
        (id, Standing::Reserved)
    } else {
        let title = frontmatter_value(&text, "title").unwrap_or_default();
        let target =
            frontmatter_value(&text, "integration_target").unwrap_or_else(|| "none".to_string());
        let mut request = Request::issue(from.kind(), &title, &target);
        request.uid = Some(uid.clone());
        let reservation = issue::reserve(root, &request)?;
        (reservation.id, reservation.standing)
    };
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .replacen(&from.to_string(), &to.to_string(), 1);
    let new_path = path.with_file_name(file);
    // Read, write and delete beneath the root without following a link
    // (issue 94). The renumbered record is placed first and exclusively, so
    // a file already at the new number refuses the retarget before any
    // other file in the tree changes.
    let tree = crate::contained::Tree::open(root)?;
    let old = crate::contained::relative_to(root, path)?;
    let new = crate::contained::relative_to(root, &new_path)?;
    let content = String::from_utf8(tree.read(&old, u64::MAX)?)
        .map_err(|error| IdsError::Invalid(format!("{old}: {error}")))?;
    let content = replace_id(&content, from, &to).unwrap_or(content);
    let content = add_former_id(&content, from);
    let moving = new != old;
    if moving {
        tree.create_new(&new, content.as_bytes())?;
    }
    let rewritten = match rewrite_links(&git, &tree, from, &to, &old) {
        Ok(rewritten) => rewritten,
        Err(error) => {
            if moving {
                let _ = tree.remove(&new);
            }
            return Err(error);
        }
    };
    if moving {
        tree.remove(&old)?;
    } else {
        tree.write(&new, content.as_bytes())?;
    }
    Ok(Retarget {
        from: from.clone(),
        to,
        path: new_path,
        standing,
        rewritten,
    })
}

fn add_former_id(text: &str, from: &RegId) -> String {
    if let Some(current) = frontmatter_value(text, "former_ids") {
        let inner = current.trim_start_matches('[').trim_end_matches(']').trim();
        let list = if inner.is_empty() {
            format!("[{from}]")
        } else {
            format!("[{inner}, {from}]")
        };
        return text
            .lines()
            .map(|line| {
                if line.starts_with("former_ids:") {
                    format!("former_ids: {list}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + if text.ends_with('\n') { "\n" } else { "" };
    }
    insert_after(text, "uid", &format!("former_ids: [{from}]")).unwrap_or_else(|| text.to_string())
}

/// Replace whole-word `from` with `to`. A longer id that starts with
/// `from` (`TSK-005-001`) is kept.
fn replace_id(text: &str, from: &RegId, to: &RegId) -> Option<String> {
    let pattern = regex::Regex::new(&format!(r"\b{}\b", regex::escape(&from.to_string()))).ok()?;
    let replacement = to.to_string();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut any = false;
    for found in pattern.find_iter(text) {
        let after = &text[found.end()..];
        if after.starts_with('-') && after[1..].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        out.push_str(&text[last..found.start()]);
        out.push_str(&replacement);
        last = found.end();
        any = true;
    }
    any.then(|| {
        out.push_str(&text[last..]);
        out
    })
}

/// Rewrite links to `from` in tracked text under the record roots and docs,
/// except `skip` (the record being renumbered). Every change is planned
/// before any is written; a failed write puts back the files this call
/// changed, the failing one included, and names any that could not be.
fn rewrite_links(
    git: &Git,
    tree: &crate::contained::Tree,
    from: &RegId,
    to: &RegId,
    skip: &str,
) -> Result<Vec<PathBuf>, IdsError> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend_from_slice(&RECORD_ROOTS);
    args.push("docs");
    let files: BTreeSet<String> = z_fields(&git.run(&args)?).map(str::to_string).collect();
    // A tracked link, or a file under one, is skipped like an unreadable
    // file: it is read and written only beneath the root (issue 94).
    let mut planned = Vec::new();
    for file in files.into_iter().filter(|file| file != skip) {
        let Some(text) = tree
            .read(&file, u64::MAX)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            continue;
        };
        if let Some(updated) = replace_id(&text, from, to) {
            planned.push((file, text, updated));
        }
    }
    for (index, (file, _, updated)) in planned.iter().enumerate() {
        if let Err(error) = tree.write(file, updated.as_bytes()) {
            let failed: Vec<String> = planned[..=index]
                .iter()
                .filter(|(file, original, _)| tree.write(file, original.as_bytes()).is_err())
                .map(|(file, _, _)| file.clone())
                .collect();
            if failed.is_empty() {
                return Err(error.into());
            }
            return Err(IdsError::Invalid(format!(
                "{file}: {error}; restoring {} also failed",
                failed.join(", ")
            )));
        }
    }
    Ok(planned
        .into_iter()
        .map(|(file, _, _)| git.root().join(file))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_lines_go_after_id_inside_frontmatter_only() {
        let text = "---\nid: TSK-001\ntitle: x\n---\nid: body\n";
        assert_eq!(
            insert_after(text, "id", "uid: u").unwrap(),
            "---\nid: TSK-001\nuid: u\ntitle: x\n---\nid: body\n"
        );
        assert!(insert_after("no frontmatter\nid: x\n", "id", "uid: u").is_none());
    }

    #[test]
    fn former_ids_append() {
        let id = RegId::parse("TSK-005").unwrap();
        let text = "---\nid: TSK-009\nuid: u\n---\n";
        assert_eq!(
            add_former_id(text, &id),
            "---\nid: TSK-009\nuid: u\nformer_ids: [TSK-005]\n---\n"
        );
        let again = add_former_id(
            "---\nid: TSK-010\nuid: u\nformer_ids: [TSK-005]\n---\n",
            &RegId::parse("TSK-009").unwrap(),
        );
        assert!(again.contains("former_ids: [TSK-005, TSK-009]"), "{again}");
    }

    #[test]
    fn seed_maps_parse() {
        let map = SeedMap::parse("[ids.\"TSK-050\"]\ncopies = [\"a\", \"b\"]\n").unwrap();
        assert_eq!(map.copies[&RegId::parse("TSK-050").unwrap()].len(), 2);
        assert!(SeedMap::parse("[ids.\"nope\"]\ncopies = []\n").is_err());
    }
}
