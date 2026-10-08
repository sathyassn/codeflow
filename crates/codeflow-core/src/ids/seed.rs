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
use crate::scaffold::state::guard_beneath_root;

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

/// The record files in the working tree, by id. A symbolic link, to a
/// file or a directory, is never followed: a renumbering must not read or
/// write outside the checkout. A linked record root is left out here;
/// `backfill` refuses it by name (issue 94).
fn worktree_records(root: &Path) -> BTreeMap<RegId, Vec<PathBuf>> {
    let mut out: BTreeMap<RegId, Vec<PathBuf>> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = RECORD_ROOTS
        .iter()
        .filter_map(|base| guard_beneath_root(root, Path::new(base)).ok())
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
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

/// A frontmatter fence, read as the record parser reads it: `---` with
/// any trailing spaces, tabs or carriage return.
fn is_fence(line: &str) -> bool {
    line.trim_end_matches(['\n', '\r', ' ', '\t']) == "---"
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
            in_front = is_fence(bare);
            continue;
        }
        if in_front && !inserted && bare.starts_with(&format!("{after}:")) {
            out.push_str(line);
            out.push('\n');
            inserted = true;
        }
        if in_front && is_fence(bare) {
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
    // A linked record root is refused by name, never passed over in silence.
    for base in RECORD_ROOTS {
        match tree.metadata(base) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
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
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    guard_beneath_root(root, Path::new(&rel))
        .map_err(|error| IdsError::Invalid(format!("{rel} cannot be renumbered: {error}")))?;
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
    let (new_path, rewritten) = renumber_files(root, &git, path, &rel, from, &to)?;
    Ok(Retarget {
        from: from.clone(),
        to,
        path: new_path,
        standing,
        rewritten,
    })
}

/// Write a renumbering: the record under its new name with `former_ids`,
/// and every tracked link. Every change is computed and checked first, so
/// a refusal (a linked destination, a changed Verbatim section) writes
/// nothing. Every read, write and delete goes beneath the root without
/// following a link (issue 94). The renumbered record is placed first and
/// exclusively, so a file already at the new number refuses the
/// renumbering before any other file changes; a later failure restores
/// every rewritten link and the old record. Returns the record's new path
/// and the rewritten files.
fn renumber_files(
    root: &Path,
    git: &Git,
    path: &Path,
    rel: &str,
    from: &RegId,
    to: &RegId,
) -> Result<(PathBuf, Vec<PathBuf>), IdsError> {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .replacen(&from.to_string(), &to.to_string(), 1);
    let new_path = path.with_file_name(file);
    let new = crate::contained::relative_to(root, &new_path)?;
    guard_beneath_root(root, Path::new(&new))
        .map_err(|error| IdsError::Invalid(format!("{new} cannot be written: {error}")))?;
    let tree = crate::contained::Tree::open(root)?;
    let original = String::from_utf8(tree.read(rel, u64::MAX)?)
        .map_err(|error| IdsError::Invalid(format!("{rel}: {error}")))?;
    // Every change is computed and checked before any file is written, so
    // a refusal leaves the record and its links as they were.
    let planned = plan_link_rewrites(git, &tree, from, to, &[rel, &new])?;
    let base = replace_id(&original, from, to, kept_range(rel, &original).as_ref())
        .unwrap_or_else(|| original.clone());
    let record = Rewrite {
        rel: rel.to_string(),
        path: new_path.clone(),
        after: add_former_id(&base, from),
        before: original,
    };
    for change in planned.iter().chain(std::iter::once(&record)) {
        if !verbatim_kept(&change.rel, &change.before, &change.after) {
            return Err(IdsError::Invalid(format!(
                "{} would change the operator's words in its Verbatim section; nothing was renumbered",
                change.rel
            )));
        }
    }
    let moving = new != rel;
    // The renamed record keeps the permissions of the file it is renamed
    // from, and they are held until the end so a restore after the old name
    // was unlinked gives the record the same bits back.
    let permissions = if moving {
        Some(tree.metadata(rel)?.permissions())
    } else {
        None
    };
    if let Some(permissions) = &permissions {
        tree.create_new_keeping(&new, record.after.as_bytes(), Some(permissions))?;
    }
    if let Err(error) = rewrite_links(&tree, &planned) {
        return Err(retarget_rollback_error(
            &tree,
            moving.then_some(new.as_str()),
            error,
            Vec::new(),
        ));
    }
    let finish = if moving {
        tree.remove(rel)
    } else {
        tree.write(&new, record.after.as_bytes())
    };
    if let Err(error) = finish {
        // Removal can fail after unlinking, and replacement after renaming.
        // Restore the old record as well as every rewritten link in either case.
        let mut failed = restore_links(&tree, &planned);
        let restored = tree.write_keeping(rel, record.before.as_bytes(), permissions.as_ref());
        if let Err(error) = &restored {
            failed.push(format!("restoring {rel} failed: {error}"));
            if moving {
                failed.push(format!("recovery record retained at {new}"));
            }
        }
        // The new record may be the only surviving copy if restoration failed.
        return Err(retarget_rollback_error(
            &tree,
            (moving && restored.is_ok()).then_some(new.as_str()),
            IdsError::Invalid(format!("{rel}: {error}")),
            failed,
        ));
    }
    let rewritten = planned.into_iter().map(|change| change.path).collect();
    Ok((new_path, rewritten))
}

/// Finish rollback without hiding a failure to remove the newly created record.
fn retarget_rollback_error(
    tree: &crate::contained::Tree,
    new: Option<&str>,
    error: IdsError,
    mut failed: Vec<String>,
) -> IdsError {
    if let Some(new) = new {
        if let Err(error) = tree.remove(new) {
            failed.push(format!("removing new record {new} failed: {error}"));
        }
    }
    if failed.is_empty() {
        error
    } else {
        IdsError::Invalid(format!("{error}; {}", failed.join("; ")))
    }
}

fn add_former_id(text: &str, from: &RegId) -> String {
    if let Some(current) = frontmatter_value(text, "former_ids") {
        let inner = current.trim_start_matches('[').trim_end_matches(']').trim();
        let list = if inner.is_empty() {
            format!("[{from}]")
        } else {
            format!("[{inner}, {from}]")
        };
        // Only the frontmatter's line changes; the body keeps every byte,
        // a `former_ids:` line quoted in it included.
        let mut out = String::with_capacity(text.len() + 16);
        let mut fences = 0;
        for line in text.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            if is_fence(bare) {
                fences += 1;
            }
            if fences == 1 && bare.starts_with("former_ids:") {
                out.push_str("former_ids: ");
                out.push_str(&list);
                out.push_str(&line[bare.len()..]);
            } else {
                out.push_str(line);
            }
        }
        return out;
    }
    insert_after(text, "uid", &format!("former_ids: [{from}]")).unwrap_or_else(|| text.to_string())
}

/// The part of a record that a renumbering never rewrites: a feedback
/// item's Verbatim section, which quotes the operator exactly.
fn kept_range(rel: &str, text: &str) -> Option<std::ops::Range<usize>> {
    rel.starts_with(&format!("{}/", crate::feedback::FEEDBACK_DIR))
        .then(|| crate::feedback::verbatim_range(text))
        .flatten()
}

/// Replace whole-word `from` with `to`, outside `kept`. A longer id that
/// starts with `from` (`TSK-005-001`) is kept.
fn replace_id(
    text: &str,
    from: &RegId,
    to: &RegId,
    kept: Option<&std::ops::Range<usize>>,
) -> Option<String> {
    let pattern = regex::Regex::new(&format!(r"\b{}\b", regex::escape(&from.to_string()))).ok()?;
    let replacement = to.to_string();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut any = false;
    for found in pattern.find_iter(text) {
        if kept.is_some_and(|kept| kept.contains(&found.start())) {
            continue;
        }
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

/// One planned file change: its repository path, where it is written, and
/// its text before and after.
struct Rewrite {
    rel: String,
    path: PathBuf,
    before: String,
    after: String,
}

/// Whether a change keeps a feedback item's Verbatim section byte for
/// byte: the operator's words are never renumbered, whatever rewrote them.
fn verbatim_kept(rel: &str, before: &str, after: &str) -> bool {
    let quoted = |text: &str| {
        kept_range(rel, text)
            .and_then(|range| text.get(range))
            .map(str::to_string)
    };
    quoted(before) == quoted(after)
}

/// The rewrites of links to `from` in tracked text under the record roots
/// and docs, computed and not yet written, except `skip` (the record being
/// renumbered, at its old and new paths, since a tracked path can be absent
/// on disk). The originals are kept until the whole renumbering, including
/// removal of the old record, succeeds.
fn plan_link_rewrites(
    git: &Git,
    tree: &crate::contained::Tree,
    from: &RegId,
    to: &RegId,
    skip: &[&str],
) -> Result<Vec<Rewrite>, IdsError> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend_from_slice(&RECORD_ROOTS);
    args.push("docs");
    let files: BTreeSet<String> = z_fields(&git.run(&args)?).map(str::to_string).collect();
    // A tracked link, or a file under one, is skipped like an unreadable
    // file: it is read and written only beneath the root (issue 94), so a
    // rewrite never changes a file outside the checkout.
    let mut planned = Vec::new();
    for file in files
        .into_iter()
        .filter(|file| !skip.contains(&file.as_str()))
    {
        let Some(text) = tree
            .read(&file, u64::MAX)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            continue;
        };
        if let Some(updated) = replace_id(&text, from, to, kept_range(&file, &text).as_ref()) {
            planned.push(Rewrite {
                path: git.root().join(&file),
                rel: file,
                before: text,
                after: updated,
            });
        }
    }
    Ok(planned)
}

/// A failed write restores every attempted link, including one whose rename
/// already landed, and reports each restoration failure.
fn rewrite_links(tree: &crate::contained::Tree, planned: &[Rewrite]) -> Result<(), IdsError> {
    for (index, change) in planned.iter().enumerate() {
        if let Err(error) = tree.write(&change.rel, change.after.as_bytes()) {
            let failed = restore_links(tree, &planned[..=index]);
            if failed.is_empty() {
                return Err(IdsError::Invalid(format!("{}: {error}", change.rel)));
            }
            return Err(IdsError::Invalid(format!(
                "{}: {error}; {}",
                change.rel,
                failed.join("; ")
            )));
        }
    }
    Ok(())
}

fn restore_links(tree: &crate::contained::Tree, planned: &[Rewrite]) -> Vec<String> {
    planned
        .iter()
        .filter_map(|change| {
            tree.write(&change.rel, change.before.as_bytes())
                .err()
                .map(|error| format!("restoring {} failed: {error}", change.rel))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contained::fault::{self, Point};

    const OLD: &str = "project-management/tasks/TSK-005.md";
    const NEW: &str = "project-management/tasks/TSK-006.md";
    const LINKS: [&str; 2] = ["docs/a.md", "docs/b.md"];

    fn retarget_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q", "-b", "task/retarget"]).unwrap();
        git.run(&["config", "user.name", "Test"]).unwrap();
        git.run(&["config", "user.email", "test@example.test"])
            .unwrap();
        let tree = crate::contained::Tree::open(dir.path()).unwrap();
        tree.write(
            OLD,
            format!("---\nid: TSK-005\nuid: {}\ntitle: Move\n---\n", new_uid()).as_bytes(),
        )
        .unwrap();
        for link in LINKS {
            tree.write(link, b"see TSK-005\n").unwrap();
        }
        // Tracking is enough for link discovery; no fixture commit is needed.
        git.run(&["add", "docs"]).unwrap();
        dir
    }

    #[test]
    fn retarget_remove_failure_restores_records_and_links() {
        for point in [Point::BeforeRemove, Point::AfterRemove] {
            let dir = retarget_fixture();
            let root = dir.path();
            let before = std::fs::read(root.join(OLD)).unwrap();
            // A private record: a restore that recreates the name from
            // scratch would give it the umask's bits instead.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(root.join(OLD), std::fs::Permissions::from_mode(0o600))
                    .unwrap();
            }
            fault::arm(point, OLD);
            let error = retarget(root, &RegId::parse("TSK-005").unwrap()).unwrap_err();
            assert!(error.to_string().contains("injected failure"), "{error}");
            assert_eq!(std::fs::read(root.join(OLD)).unwrap(), before);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(root.join(OLD))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777;
                assert_eq!(mode, 0o600, "{point:?} restored the record's mode");
            }
            for link in LINKS {
                assert_eq!(std::fs::read(root.join(link)).unwrap(), b"see TSK-005\n");
            }
            assert!(!root.join(NEW).exists(), "the new record is removed");
        }
    }

    #[test]
    fn retarget_keeps_recovery_record_when_restoring_old_fails() {
        for point in [Point::BeforeRename, Point::AfterRename] {
            let dir = retarget_fixture();
            let root = dir.path();
            let original = std::fs::read_to_string(root.join(OLD)).unwrap();
            let expected = format!(
                "---\nid: TSK-006\nuid: {}\nformer_ids: [TSK-005]\ntitle: Move\n---\n",
                frontmatter_value(&original, "uid").unwrap()
            );
            fault::arm_sequence(&[(Point::AfterRemove, OLD), (point, OLD)]);
            let error = retarget(root, &RegId::parse("TSK-005").unwrap())
                .unwrap_err()
                .to_string();
            if point == Point::BeforeRename {
                assert!(!root.join(OLD).exists(), "restoration was not published");
            } else {
                assert_eq!(std::fs::read_to_string(root.join(OLD)).unwrap(), original);
            }
            assert!(
                root.join(NEW).is_file(),
                "the recovery record must survive: {error}"
            );
            assert_eq!(std::fs::read_to_string(root.join(NEW)).unwrap(), expected);
            for expected_error in [OLD, "AfterRemove", &format!("{point:?}"), NEW, "recovery"] {
                assert!(
                    error.contains(expected_error),
                    "missing {expected_error}: {error}"
                );
            }
            for link in LINKS {
                assert_eq!(std::fs::read(root.join(link)).unwrap(), b"see TSK-005\n");
            }
        }
    }

    #[test]
    fn retarget_link_restore_failure_preserves_record_and_link_contents() {
        for failure in [(Point::AfterRemove, OLD), (Point::AfterRename, LINKS[1])] {
            let dir = retarget_fixture();
            let root = dir.path();
            let original = std::fs::read(root.join(OLD)).unwrap();
            fault::arm_sequence(&[failure, (Point::BeforeRename, LINKS[0])]);
            let error = retarget(root, &RegId::parse("TSK-005").unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains("restoring docs/a.md failed"), "{error}");
            assert!(error.contains("BeforeRename"), "{error}");
            // A failed atomic restoration keeps the complete rewritten file.
            assert_eq!(
                std::fs::read(root.join(LINKS[0])).unwrap(),
                b"see TSK-006\n"
            );
            assert_eq!(
                std::fs::read(root.join(LINKS[1])).unwrap(),
                b"see TSK-005\n"
            );
            assert_eq!(std::fs::read(root.join(OLD)).unwrap(), original);
            assert!(!root.join(NEW).exists(), "the old record is safely present");
        }
    }

    #[test]
    fn retarget_reports_failed_new_record_cleanup() {
        let dir = retarget_fixture();
        let root = dir.path();
        let before = std::fs::read(root.join(OLD)).unwrap();
        fault::arm_sequence(&[(Point::AfterRename, LINKS[1]), (Point::BeforeRemove, NEW)]);
        let error = retarget(root, &RegId::parse("TSK-005").unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("AfterRename"), "{error}");
        assert!(
            error.contains(NEW) && error.contains("BeforeRemove"),
            "{error}"
        );
        assert_eq!(std::fs::read(root.join(OLD)).unwrap(), before);
        for link in LINKS {
            assert_eq!(std::fs::read(root.join(link)).unwrap(), b"see TSK-005\n");
        }
        assert!(root.join(NEW).is_file(), "the reported new record remains");
    }

    #[test]
    fn retarget_reports_each_rollback_failure_and_continues_restoring() {
        let dir = retarget_fixture();
        let root = dir.path();
        let before = std::fs::read(root.join(OLD)).unwrap();
        fault::arm_sequence(&[
            (Point::AfterRemove, OLD),
            (Point::AfterRename, LINKS[0]),
            (Point::BeforeRemove, NEW),
        ]);
        let error = retarget(root, &RegId::parse("TSK-005").unwrap())
            .unwrap_err()
            .to_string();
        for expected in [
            OLD,
            "AfterRemove",
            LINKS[0],
            "AfterRename",
            NEW,
            "BeforeRemove",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
        assert_eq!(std::fs::read(root.join(OLD)).unwrap(), before);
        for link in LINKS {
            assert_eq!(std::fs::read(root.join(link)).unwrap(), b"see TSK-005\n");
        }
        assert!(root.join(NEW).is_file(), "the reported new record remains");
    }

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

    /// Renumbering rewrites a feedback item's frontmatter and Placement but
    /// never its Verbatim quote; other records are rewritten throughout.
    #[test]
    fn renumbering_keeps_the_operators_words() {
        let from = RegId::parse("TSK-005").unwrap();
        let to = RegId::parse("TSK-009").unwrap();
        let item = "---\nid: FB-001\nplaced_in: [\"TSK-005\"]\n---\n\n# FB-001: x\n\n## Verbatim\n\nkeep TSK-005 first\n\n## Reading\n\nTSK-005\n";
        let rewritten = replace_id(
            item,
            &from,
            &to,
            kept_range("project-management/feedback/FB-001.md", item).as_ref(),
        )
        .unwrap();
        assert_eq!(
            rewritten,
            "---\nid: FB-001\nplaced_in: [\"TSK-009\"]\n---\n\n# FB-001: x\n\n## Verbatim\n\nkeep TSK-005 first\n\n## Reading\n\nTSK-009\n"
        );
        let task = "## Verbatim\n\nTSK-005\n";
        assert_eq!(
            replace_id(
                task,
                &from,
                &to,
                kept_range("project-management/tasks/TSK-001.md", task).as_ref()
            )
            .unwrap(),
            "## Verbatim\n\nTSK-009\n"
        );
    }

    /// `former_ids` changes only in the frontmatter; the same line quoted in
    /// the body keeps its bytes.
    #[test]
    fn former_ids_change_only_in_the_frontmatter() {
        let id = RegId::parse("FB-002").unwrap();
        let text =
            "---\nid: FB-003\nformer_ids: [FB-001]\n---\n\n## Verbatim\n\nformer_ids: [keep]\n";
        assert_eq!(
            add_former_id(text, &id),
            "---\nid: FB-003\nformer_ids: [FB-001, FB-002]\n---\n\n## Verbatim\n\nformer_ids: [keep]\n"
        );
    }

    /// Renumbering never reads or writes through a symbolic link: a linked
    /// record or directory is not listed, and a tracked link is not
    /// rewritten, so a file outside the checkout keeps its bytes.
    #[cfg(unix)]
    #[test]
    fn renumbering_never_follows_a_symbolic_link() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let feedback = root.join("project-management/feedback");
        std::fs::create_dir_all(&feedback).unwrap();
        std::fs::write(
            feedback.join("FB-001.md"),
            "---\nid: FB-001\n---\nTSK-005\n",
        )
        .unwrap();
        let target = outside.path().join("elsewhere.md");
        std::fs::write(&target, "---\nid: FB-002\n---\nTSK-005\n").unwrap();
        symlink(&target, feedback.join("FB-002.md")).unwrap();
        symlink(outside.path(), root.join("project-management/linked")).unwrap();
        std::fs::write(outside.path().join("TSK-009.md"), "---\nid: TSK-009\n---\n").unwrap();

        let records = worktree_records(root);
        assert!(records.contains_key(&RegId::parse("FB-001").unwrap()));
        assert!(!records.contains_key(&RegId::parse("FB-002").unwrap()));
        assert!(!records.contains_key(&RegId::parse("TSK-009").unwrap()));

        let git = |args: &[&str]| {
            let out = crate::git::command()
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        let planned = plan_link_rewrites(
            &Git::new(root),
            &crate::contained::Tree::open(root).unwrap(),
            &RegId::parse("TSK-005").unwrap(),
            &RegId::parse("TSK-006").unwrap(),
            &[],
        )
        .unwrap();
        let rels: Vec<&str> = planned.iter().map(|change| change.rel.as_str()).collect();
        assert_eq!(rels, ["project-management/feedback/FB-001.md"]);
        assert!(planned[0].after.contains("TSK-006"));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "---\nid: FB-002\n---\nTSK-005\n"
        );
    }

    /// A closing fence with trailing spaces still ends the frontmatter, as
    /// the record parser reads it; the body keeps every byte.
    #[test]
    fn a_fence_with_trailing_space_still_ends_the_frontmatter() {
        let id = RegId::parse("FB-001").unwrap();
        let text = "---\nid: FB-002\nuid: u\nformer_ids: [FB-000]\n---  \n\n## Verbatim\n\nformer_ids: [keep]\nuid: body\n";
        assert_eq!(
            add_former_id(text, &id),
            "---\nid: FB-002\nuid: u\nformer_ids: [FB-000, FB-001]\n---  \n\n## Verbatim\n\nformer_ids: [keep]\nuid: body\n"
        );
        let bare = "---\nid: FB-002\n---\t\nuid: body\n";
        assert!(insert_after(bare, "uid", "former_ids: [FB-001]").is_none());
    }

    /// The last check before any write: a change to a feedback item's
    /// Verbatim section refuses the renumbering, whatever made it.
    #[test]
    fn a_change_to_the_verbatim_section_is_caught() {
        let rel = "project-management/feedback/FB-001.md";
        let before =
            "---\nid: FB-001\n---\n\n## Verbatim\n\nkeep TSK-005\n\n## Reading\n\nTSK-005\n";
        let reading = before.replace("\nTSK-005\n", "\nTSK-006\n");
        assert!(verbatim_kept(rel, before, &reading));
        let quoted = before.replace("keep TSK-005", "keep TSK-006");
        assert!(!verbatim_kept(rel, before, &quoted));
        assert!(verbatim_kept(
            "project-management/tasks/TSK-001.md",
            before,
            &quoted
        ));
    }

    /// A destination that is a symbolic link refuses the renumbering before
    /// anything is written: the record and the links to it keep their bytes.
    #[cfg(unix)]
    #[test]
    fn a_refused_destination_writes_nothing() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let feedback = root.join("project-management/feedback");
        let tasks = root.join("project-management/tasks");
        std::fs::create_dir_all(&feedback).unwrap();
        std::fs::create_dir_all(&tasks).unwrap();
        let record = "---\nid: FB-001\nuid: u\n---\n\n# FB-001: x\n";
        let link = "---\nid: TSK-001\n---\nsee FB-001\n";
        std::fs::write(feedback.join("FB-001.md"), record).unwrap();
        std::fs::write(tasks.join("TSK-001.md"), link).unwrap();
        let out = crate::git::command()
            .args(["init", "-q"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success());
        let out = crate::git::command()
            .args(["add", "-A"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success());
        let elsewhere = outside.path().join("elsewhere.md");
        std::fs::write(&elsewhere, "outside\n").unwrap();
        symlink(&elsewhere, feedback.join("FB-002.md")).unwrap();

        let error = renumber_files(
            root,
            &Git::new(root),
            &feedback.join("FB-001.md"),
            "project-management/feedback/FB-001.md",
            &RegId::parse("FB-001").unwrap(),
            &RegId::parse("FB-002").unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cannot be written"), "{error}");
        assert_eq!(
            std::fs::read_to_string(feedback.join("FB-001.md")).unwrap(),
            record
        );
        assert_eq!(
            std::fs::read_to_string(tasks.join("TSK-001.md")).unwrap(),
            link
        );
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "outside\n");
    }

    /// A write that fails part way leaves every file whole, restores each
    /// file already written and the old record, and names the file that
    /// failed.
    #[cfg(unix)]
    #[test]
    fn a_renumbering_that_stops_part_way_restores_what_landed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let feedback = root.join("project-management/feedback");
        let tasks = root.join("project-management/tasks");
        let docs = root.join("docs");
        for folder in [&feedback, &tasks, &docs] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let record = "---\nid: FB-001\nuid: u\n---\n\n# FB-001: x\n";
        let task = "---\nid: TSK-001\n---\nsee FB-001\n";
        std::fs::write(feedback.join("FB-001.md"), record).unwrap();
        std::fs::write(tasks.join("TSK-001.md"), task).unwrap();
        std::fs::write(docs.join("guide.md"), "see FB-001\n").unwrap();
        for args in [&["init", "-q"][..], &["add", "-A"][..]] {
            let out = crate::git::command()
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(tasks.join("probe"), "x").is_ok() {
            // Permissions do not bind here (a privileged user): nothing to show.
            std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let error = renumber_files(
            root,
            &Git::new(root),
            &feedback.join("FB-001.md"),
            "project-management/feedback/FB-001.md",
            &RegId::parse("FB-001").unwrap(),
            &RegId::parse("FB-002").unwrap(),
        )
        .unwrap_err()
        .to_string();
        std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            error.contains("project-management/tasks/TSK-001.md"),
            "{error}"
        );
        assert!(!error.contains("restoring docs/guide.md"), "{error}");
        assert_eq!(
            std::fs::read_to_string(docs.join("guide.md")).unwrap(),
            "see FB-001\n"
        );
        assert_eq!(
            std::fs::read_to_string(tasks.join("TSK-001.md")).unwrap(),
            task
        );
        assert_eq!(
            std::fs::read_to_string(feedback.join("FB-001.md")).unwrap(),
            record
        );
        assert!(!feedback.join("FB-002.md").exists());
        let leftovers: Vec<_> = std::fs::read_dir(&tasks)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// A renumbering keeps each file's permissions: an executable link
    /// stays executable, a private one stays private, and the renamed
    /// record keeps the mode of the file it was renamed from.
    #[cfg(unix)]
    #[test]
    fn a_renumbering_keeps_each_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let feedback = root.join("project-management/feedback");
        let docs = root.join("docs");
        std::fs::create_dir_all(&feedback).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        let files = [
            (docs.join("verify.sh"), "#!/bin/sh\necho FB-001\n", 0o755),
            (docs.join("private.md"), "see FB-001\n", 0o600),
            (
                feedback.join("FB-001.md"),
                "---\nid: FB-001\nuid: u\n---\n\n# FB-001: x\n",
                0o640,
            ),
        ];
        for (path, text, mode) in &files {
            std::fs::write(path, text).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode)).unwrap();
        }
        for args in [&["init", "-q"][..], &["add", "-A"][..]] {
            let out = crate::git::command()
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        renumber_files(
            root,
            &Git::new(root),
            &feedback.join("FB-001.md"),
            "project-management/feedback/FB-001.md",
            &RegId::parse("FB-001").unwrap(),
            &RegId::parse("FB-002").unwrap(),
        )
        .unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&docs.join("verify.sh")), 0o755);
        assert_eq!(mode(&docs.join("private.md")), 0o600);
        assert_eq!(mode(&feedback.join("FB-002.md")), 0o640);
        assert!(std::fs::read_to_string(docs.join("verify.sh"))
            .unwrap()
            .contains("FB-002"));
        assert!(!feedback.join("FB-001.md").exists());
    }

    #[test]
    fn seed_maps_parse() {
        let map = SeedMap::parse("[ids.\"TSK-050\"]\ncopies = [\"a\", \"b\"]\n").unwrap();
        assert_eq!(map.copies[&RegId::parse("TSK-050").unwrap()].len(), 2);
        assert!(SeedMap::parse("[ids.\"nope\"]\ncopies = []\n").is_err());
    }
}
