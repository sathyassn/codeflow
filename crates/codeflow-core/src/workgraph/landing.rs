//! Whether a completion of a task has landed on the judged target, and
//! which criteria the target holds for it (SPC-013 R-52, R-119; TSK-234).
//!
//! One judgement answers both questions for every check that asks them:
//! the reopen freeze, the criteria freeze of a task pull request, and the
//! planning amendment's report. Two answers come out of it, and they are
//! kept apart:
//!
//! - **Landing** is evidence. A completion has landed when some commit
//!   reachable from a judged point (the range anchor or a run's criteria
//!   base) holds a version of the task's record that shows a completion: it
//!   is complete, keeps an acceptance block, active or superseded, or
//!   records a reopen. The whole history of the judged points is read,
//!   never only their trees, so a later edit or deletion on the target
//!   never hides a landing. `Planned` needs proof of absence: a complete,
//!   readable history with no overlay, records and trees that read and
//!   parse, and one identity (one id never carried with two uids, and one
//!   record of the task per commit).
//!   A branch or remote-tracking ref outside the range that adds a
//!   completing version only makes the answer unknown.
//! - **Criteria** are authority. They come from the newest version of the
//!   record at the judged points themselves, which a later reviewed
//!   planning amendment may have changed; when the points no longer hold
//!   the record, from the newest version on every line of their history,
//!   which must agree. A landing witness never supplies criteria.
//!
//! Readiness (whether a predecessor is complete now) is not decided here:
//! it reads the current status at the execution base (D3a).

use crate::workgraph::history;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::rc::Rc;

use git2::{Oid, Repository};

use super::lifecycle::RecordView;
use super::work_start::RecordKind;

/// A task's identity: its parsed `id` and `uid`, and the file name of its
/// own record, which counts as the task's even when it does not parse.
pub(super) struct Identity {
    id: String,
    uid: Option<String>,
    own_file: String,
}

impl Identity {
    /// An identity from its parts.
    #[cfg(test)]
    pub(super) fn named(id: &str, uid: Option<String>, own_file: &str) -> Self {
        Self {
            id: id.to_string(),
            uid,
            own_file: own_file.to_string(),
        }
    }

    /// The identity `task` carries.
    pub(super) fn of(task: &RecordView) -> Self {
        Self {
            id: task.id.clone(),
            uid: super::acceptance::record_uid(&task.content),
            own_file: super::acceptance::own_file_name(task),
        }
    }
}

/// Whether a completion of the task has landed on the judged target.
pub(super) enum Landing {
    /// A judged point's history holds a version that shows a completion,
    /// at `witness`. Evidence only: it never supplies criteria.
    Landed { witness: Oid },
    /// The whole readable history of the judged points shows no
    /// completion, and no ref outside the range adds one.
    Planned,
    /// The history cannot prove either answer: the reason.
    Unreadable(String),
    /// No judged point shows a landing, but a branch or remote-tracking ref
    /// outside the range adds a version that shows a completion: `commit`,
    /// which `holder` reaches and the range's head does not.
    Elsewhere { holder: String, commit: Oid },
    /// No judged point shows a landing, but a tip of one of the task's
    /// targets that the run is not judged against does: `reference` at
    /// `commit`. Such a tip can be moved here, so it only refuses.
    Unjudged { reference: String, commit: Oid },
}

/// The criteria the judged target holds for the task.
pub(super) enum Authority {
    /// The newest version at the judged points, or in their history when
    /// the points no longer hold the record.
    Held(Box<RecordView>),
    /// No version anywhere in a complete, readable history: a new task.
    Absent,
    /// The versions cannot be told apart or read: the reason.
    Unknown(String),
}

/// One task's judgement ([`RecordStore::judge`]).
pub(super) struct TaskJudgement {
    pub(super) landing: Landing,
    pub(super) criteria: Authority,
    /// The source of the criteria, kept apart from the run's policy authority.
    pub(super) criteria_source: String,
}

impl TaskJudgement {
    /// A judgement that cannot be made: both answers unknown, for `reason`.
    pub(super) fn unreadable(reason: String) -> Self {
        Self {
            landing: Landing::Unreadable(reason.clone()),
            criteria_source: format!("criteria authority unreadable: {reason}"),
            criteria: Authority::Unknown(reason),
        }
    }

    /// Whether the landing answer is unknown (anything but landed or
    /// planned): a criteria change is refused, and nothing else.
    pub(super) fn landing_unknown(&self) -> Option<String> {
        match &self.landing {
            Landing::Landed { .. } | Landing::Planned => None,
            Landing::Unreadable(reason) => Some(reason.clone()),
            Landing::Elsewhere { holder, commit } => Some(format!(
                "`{holder}` records a completion of this task outside this range (commit {commit:.9} adds or changes its record)"
            )),
            Landing::Unjudged { reference, commit } => Some(format!(
                "`{reference}` (commit {commit:.9}) records a completion of this task, but it is not the target this run is judged against"
            )),
        }
    }
}

/// What the run reads outside its judged points: refs that can only refuse.
pub(super) struct Outside<'a> {
    /// The range's head, whose own history never counts as outside.
    pub(super) head: Oid,
    /// Tips of the task's targets that the run is not judged against.
    pub(super) tips: &'a [(String, Oid)],
    /// The completion this range recovered, when it recovered one: a tip
    /// that reaches it holds this range's completion beyond the judged
    /// target.
    pub(super) at: Option<Oid>,
    /// Bases the range reaches beyond its anchor (its own history): read
    /// only for a record that does not parse.
    pub(super) own: &'a [Oid],
}

/// What one tree holds of one task.
#[derive(Clone, Default)]
struct Held {
    /// Each record that is the task, as its blob and parsed record: one
    /// per path, so two copies of one record count as two.
    records: Vec<(Oid, RecordView)>,
    /// The uids the records carrying the task's id hold.
    uids: BTreeSet<String>,
    /// Why the tree's answer cannot be trusted, when it cannot: a record
    /// that may be the task does not parse, or the identity is ambiguous.
    doubt: Option<String>,
}

/// One task file as the store parsed it, shared by every tree and task
/// that holds it, so a lookup copies a pointer, never the record.
#[derive(Clone)]
enum Read {
    /// It parses as a task record, with the uid its frontmatter carries.
    Parsed(Rc<RecordView>, Option<Rc<str>>),
    /// It does not parse.
    Broken,
}

/// One run's reading of record history: trees, task entries and parsed
/// records are read once per object and shared by every task judged.
pub(super) struct RecordStore<'r> {
    repo: &'r Repository,
    /// Task entries (path, blob) per `project-management` tree.
    entries: HashMap<Oid, Vec<(String, Oid)>>,
    /// Each task file's text, by blob (`None` when it cannot be read).
    texts: HashMap<Oid, Option<Rc<str>>>,
    /// Each task file parsed, by path and blob.
    read: HashMap<(String, Oid), Read>,
    /// What each `project-management` tree holds of each task identity.
    held: HashMap<(Oid, String, Option<String>), Held>,
    /// Per task identity, whether each blob may spell it ([`may_spell`]).
    spells: HashMap<(String, Option<String>), HashMap<Oid, bool>>,
    /// Why no history read here is complete, when it is not.
    overlay: Option<String>,
    /// The commits a shallow clone's history is cut at.
    boundary: HashSet<Oid>,
    /// Every introduction of a version in a holder's history for one task
    /// ([`RecordStore::introductions`]), or why it cannot be read, so each
    /// task's history is walked once per holder and version in a run. One
    /// store judges every task of a run, so the key carries the task's
    /// whole identity: another task's proof never answers (SEC-306-03).
    introduced: HashMap<Traced, Result<Vec<Oid>, String>>,
}

impl<'r> RecordStore<'r> {
    /// A store over `repo`. A shallow boundary that cannot be read, or a
    /// history overlay (a graft file or a replace ref, wherever git reads
    /// them), makes every proof of absence unknown.
    pub(super) fn new(repo: &'r Repository) -> Self {
        let (boundary, boundary_problem) = match super::release_line::shallow_boundary(repo) {
            Ok(boundary) => (boundary, None),
            Err(reason) => (
                HashSet::new(),
                Some(format!("cannot verify history: {reason}")),
            ),
        };
        let overlay = history::check(repo)
            .err()
            .map(|error| error.to_string())
            .or(boundary_problem);
        Self {
            repo,
            entries: HashMap::new(),
            texts: HashMap::new(),
            read: HashMap::new(),
            held: HashMap::new(),
            spells: HashMap::new(),
            overlay,
            boundary,
            introduced: HashMap::new(),
        }
    }

    /// What the tree at `commit` holds of the task.
    ///
    /// # Errors
    ///
    /// Returns a message when the commit or its tree cannot be read.
    fn held_at(&mut self, commit: Oid, task: &Identity) -> Result<Held, String> {
        let tree = self
            .repo
            .find_commit(commit)
            .and_then(|commit| commit.tree())
            .map_err(|error| format!("cannot read the tree of {commit}: {}", error.message()))?;
        let Some(records) = tree
            .get_name("project-management")
            .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
            .map(|entry| entry.id())
        else {
            return Ok(Held::default());
        };
        let key = (records, task.id.clone(), task.uid.clone());
        if let Some(held) = self.held.get(&key) {
            return Ok(held.clone());
        }
        if !self.entries.contains_key(&records) {
            let tree = self.repo.find_tree(records).map_err(|error| {
                format!("cannot read the records at {commit}: {}", error.message())
            })?;
            let found = super::acceptance::task_entries_in(self.repo, &tree)
                .map_err(|reason| format!("cannot read the records at {commit}: {reason}"))?;
            self.entries.insert(records, found);
        }
        let entries = self.entries.get(&records).cloned().unwrap_or_default();
        let identity = (task.id.clone(), task.uid.clone());
        let mut spells = self.spells.remove(&identity).unwrap_or_default();
        let mut held = Held::default();
        for (path, blob) in entries {
            let own = path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(&task.own_file));
            let Some(text) = self.text(blob) else {
                // Any task file may be this task under another name.
                held.doubt.get_or_insert(format!(
                    "{path} at {commit} cannot be read, so whether it is this task cannot be told"
                ));
                continue;
            };
            // Only a file that may spell the task's id or uid can parse as
            // the task, so no other file is parsed for it. Each blob is
            // searched once per task, however many trees hold it.
            let spelled = *spells.entry(blob).or_insert_with(|| may_spell(&text, task));
            if !(own || spelled) {
                continue;
            }
            match self.read(&path, blob, &text) {
                // The same task by parsed identity (`is_same_task`), read
                // with the uid parsed once per file.
                Read::Parsed(record, uid)
                    if record.id == task.id
                        || (task.uid.is_some() && uid.as_deref() == task.uid.as_deref()) =>
                {
                    if record.id == task.id {
                        if uid.is_some()
                            && task.uid.is_some()
                            && uid.as_deref() != task.uid.as_deref()
                        {
                            held.doubt.get_or_insert(format!(
                                "{path} at {commit} carries the id {} with another uid, so which task it is cannot be told",
                                task.id
                            ));
                        }
                        held.uids.extend(uid.as_deref().map(str::to_string));
                    }
                    held.records.push((blob, record.as_ref().clone()));
                }
                Read::Parsed(..) => {}
                Read::Broken => {
                    held.doubt.get_or_insert(format!(
                        "{path} at {commit} does not parse, so whether it is this task cannot be told"
                    ));
                }
            }
        }
        self.spells.insert(identity, spells);
        if held.records.len() > 1 {
            held.doubt.get_or_insert(format!(
                "{commit} holds {} records of {}, so which one is the task cannot be told",
                held.records.len(),
                task.id
            ));
        }
        self.held.insert(key, held.clone());
        Ok(held)
    }

    /// The text of `blob`, read once per run; `None` when the object
    /// cannot be read or is not UTF-8. A lossy rendering could parse as a
    /// record whose values differ from its bytes (issue 79), so such a
    /// file is doubt, never a record.
    fn text(&mut self, blob: Oid) -> Option<Rc<str>> {
        if let Some(text) = self.texts.get(&blob) {
            return text.clone();
        }
        let text = self
            .repo
            .find_blob(blob)
            .ok()
            .and_then(|object| std::str::from_utf8(object.content()).ok().map(Rc::from));
        self.texts.insert(blob, text.clone());
        text
    }

    /// The task file at `path` with `blob` and `text`, parsed once per run.
    fn read(&mut self, path: &str, blob: Oid, text: &str) -> Read {
        let key = (path.to_string(), blob);
        if let Some(read) = self.read.get(&key) {
            return read.clone();
        }
        let read = match RecordView::parse(RecordKind::Task, path, text) {
            Ok(record) => {
                let uid = super::acceptance::record_uid(&record.content).map(Rc::from);
                Read::Parsed(Rc::new(record), uid)
            }
            Err(_) => Read::Broken,
        };
        self.read.insert(key, read.clone());
        read
    }

    /// The records of the task the tree at `commit` holds.
    ///
    /// # Errors
    ///
    /// Returns a message when the commit or its tree cannot be read.
    pub(super) fn records_at(
        &mut self,
        commit: Oid,
        task: &Identity,
    ) -> Result<Vec<RecordView>, String> {
        Ok(self
            .held_at(commit, task)?
            .records
            .into_iter()
            .map(|(_, record)| record)
            .collect())
    }

    /// Walk the history `push` reaches and `hide` does not, newest first:
    /// the first commit whose version shows a completion (the walk stops
    /// there), and why the walk proves no absence, when it does not.
    fn walk(&mut self, task: &Identity, push: &[Oid], hide: &[Oid]) -> Walk {
        let mut found = Walk::default();
        if push.is_empty() {
            found.doubt = Some("no judged point to read".to_string());
            return found;
        }
        found.doubt.clone_from(&self.overlay);
        let unreadable =
            |error: git2::Error| format!("cannot read the history: {}", error.message());
        let walk = match history::revwalk(self.repo)
            .map_err(unreadable)
            .and_then(|mut walk| {
                walk.set_sorting(git2::Sort::TOPOLOGICAL)
                    .map_err(unreadable)?;
                for point in push {
                    walk.push(*point).map_err(unreadable)?;
                }
                for point in hide {
                    walk.hide(*point).map_err(unreadable)?;
                }
                Ok(walk)
            }) {
            Ok(walk) => walk,
            Err(reason) => {
                found.doubt.get_or_insert(reason);
                return found;
            }
        };
        let mut uids: BTreeSet<String> = task.uid.iter().cloned().collect();
        for commit in walk {
            let commit = match commit {
                Ok(commit) => commit,
                Err(error) => {
                    found.doubt.get_or_insert(unreadable(error));
                    return found;
                }
            };
            if self.boundary.contains(&commit) {
                found.doubt.get_or_insert(format!(
                    "this clone is shallow at {commit}, so the history before it cannot be read; fetch it in full (`git fetch --unshallow`)"
                ));
            }
            let held = match self.held_at(commit, task) {
                Ok(held) => held,
                Err(reason) => {
                    found.doubt.get_or_insert(reason);
                    continue;
                }
            };
            if let Some(doubt) = held.doubt {
                found.doubt.get_or_insert(doubt);
            }
            uids.extend(held.uids);
            if let Some(doubt) = two_uids(task, &uids) {
                found.doubt.get_or_insert(doubt);
            }
            if held
                .records
                .iter()
                .any(|(_, record)| super::acceptance::shows_completion(record))
            {
                found.witness = Some(commit);
                return found;
            }
        }
        found
    }

    /// Judge the task against `judged` (the range anchor and the run's
    /// criteria bases outside the range), reading `outside` only to refuse.
    pub(super) fn judge(
        &mut self,
        task: &Identity,
        judged: &[Oid],
        outside: &Outside<'_>,
    ) -> TaskJudgement {
        if let Some(reason) = &self.overlay {
            return TaskJudgement::unreadable(reason.clone());
        }
        // The criteria at the judged points: the newest point's version.
        let mut points = Vec::new();
        for point in judged {
            match self.held_at(*point, task) {
                Ok(held) => points.push((*point, held)),
                Err(reason) => return TaskJudgement::unreadable(reason),
            }
        }
        // An ancestry that cannot be read refuses (issue 79): read as "not
        // newer", it would let the points' agreement decide alone.
        let mut newest = None;
        for (point, held) in &points {
            let mut all = true;
            for (other, _) in &points {
                match super::acceptance::is_ancestor_or_same(self.repo, *other, *point) {
                    Ok(true) => {}
                    Ok(false) => {
                        all = false;
                        break;
                    }
                    Err(reason) => return TaskJudgement::unreadable(reason),
                }
            }
            if all {
                newest = Some((point, held));
                break;
            }
        }
        let at_points = if let Some((_, held)) = newest {
            match (&held.doubt, held.records.first()) {
                (Some(doubt), _) => Err(doubt.clone()),
                (None, Some((_, record))) => Ok(Some(record.clone())),
                (None, None) => Ok(None),
            }
        } else {
            self.agreed(task, &points)
        };
        let criteria = match at_points {
            Ok(Some(record)) => Authority::Held(Box::new(record)),
            Err(reason) => Authority::Unknown(reason),
            Ok(None) => match self.recovered(task, judged) {
                Ok(Some(record)) => Authority::Held(Box::new(record)),
                Ok(None) => Authority::Absent,
                Err(reason) => Authority::Unknown(reason),
            },
        };
        let criteria_source = criteria_source(&points, &criteria, newest.is_none());
        let history = self.walk(task, judged, &[]);
        if let Some(witness) = history.witness {
            return TaskJudgement {
                landing: Landing::Landed { witness },
                criteria,
                criteria_source,
            };
        }
        let landing = match history.doubt {
            Some(doubt) => Landing::Unreadable(doubt),
            None => self.outside(task, judged, outside),
        };
        TaskJudgement {
            landing,
            criteria,
            criteria_source,
        }
    }

    /// The version the judged `points` agree on when none of them is newer
    /// than the others. A point that holds no record of the task and
    /// carries no doubt holds no version, so it neither agrees nor
    /// disagrees (issue 153): a default branch that never held a task
    /// planned on a diverged integration line leaves the line's version as
    /// the authority. Such a point still disagrees when its own history
    /// holds a version with other criteria, which it no longer holds.
    /// Held versions inherited unchanged from a point that replaced them
    /// do not compete. The remaining held criteria must agree.
    ///
    /// # Errors
    ///
    /// Returns why the criteria cannot be told: a point's doubt, held
    /// versions with different criteria, or an absent point whose history
    /// holds a version with other criteria or cannot prove which version
    /// it keeps ([`Self::recovered`]).
    fn agreed(
        &mut self,
        task: &Identity,
        points: &[(Oid, Held)],
    ) -> Result<Option<RecordView>, String> {
        let context = held_context(points);
        let differ = || {
            format!("the judged points hold different versions of this task, and none of them is newer than the others: {context}")
        };
        let mut versions: Vec<(Oid, Oid, &RecordView)> = Vec::new();
        let mut absent: Vec<Oid> = Vec::new();
        for (point, held) in points {
            if let Some(doubt) = &held.doubt {
                return Err(format!("{doubt}; judged points: {context}"));
            }
            match held.records.first() {
                Some((blob, record)) => versions.push((*point, *blob, record)),
                None => absent.push(*point),
            }
        }
        let Some((first, rest)) = versions.split_first() else {
            return Ok(None);
        };
        let signature = first.2.criteria.signature();
        if rest
            .iter()
            .any(|(_, _, record)| record.criteria.signature() != signature)
        {
            // Held versions compete by their criteria, so each introduction
            // is traced by criteria signature (SEC-306-01).
            let mut remaining = Vec::new();
            for &(point, blob, record) in &versions {
                let mut older = false;
                let held = Holding::new(point, blob, Version::criteria(record));
                for &(other, other_blob, version) in &versions {
                    let newer = Holding::new(other, other_blob, Version::criteria(version));
                    if held.version != newer.version
                        && self
                            .yields(task, &held, &newer)
                            .map_err(|reason| format!("{reason}; judged points: {context}"))?
                    {
                        older = true;
                        break;
                    }
                }
                if !older {
                    remaining.push((point, blob, record));
                }
            }
            versions = remaining;
        }
        let Some((_, _, first)) = versions.first() else {
            return Err(differ());
        };
        let signature = first.criteria.signature();
        if versions
            .iter()
            .any(|(_, _, record)| record.criteria.signature() != signature)
        {
            return Err(differ());
        }
        if !absent.is_empty() {
            if let Some(earlier) = self
                .recovered(task, &absent)
                .map_err(|reason| format!("{reason}; judged points: {context}"))?
            {
                if earlier.criteria.signature() != signature {
                    return Err(differ());
                }
            }
        }
        Ok(Some((*first).clone()))
    }

    /// The newest version of the task's record in the history of `judged`,
    /// for a task the judged points no longer hold: the newest version on
    /// every line of that history. A line's version is older than another
    /// line's only when that line inherited it unchanged: every commit that
    /// introduced it is a proper ancestor of every introduction of the other
    /// line's version. Lines that hold different versions otherwise give no answer.
    ///
    /// # Errors
    ///
    /// Returns the reason when the history cannot prove which version is
    /// the newest: an overlay, a shallow cut, an unreadable or ambiguous
    /// record, or lines that disagree.
    pub(super) fn recovered(
        &mut self,
        task: &Identity,
        judged: &[Oid],
    ) -> Result<Option<RecordView>, String> {
        if let Some(overlay) = &self.overlay {
            return Err(overlay.clone());
        }
        // Each walk hides the newest holders found so far with their
        // history, so the next holder it meets is on another line.
        let mut hidden: Vec<Oid> = Vec::new();
        let mut holders: Vec<(Oid, Oid, RecordView)> = Vec::new();
        while let Some((commit, blob, record)) = self.first_holder(task, judged, &hidden)? {
            hidden.push(commit);
            holders.push((commit, blob, record));
        }
        // A holder's version is older when its line inherited it unchanged
        // from a line that holds another version. The rest must agree,
        // whatever order the walks met them in.
        // Lines here compete by criteria, status and epic ([`same_version`]).
        // Lines with other criteria are ordered by criteria introductions
        // alone, as held points are, so a status or epic change never erases
        // an earlier decision for competing criteria (SEC-306-04). Only
        // lines whose criteria agree are ordered by the whole tuple.
        let mut newest: Vec<usize> = Vec::new();
        for (index, (commit, blob, record)) in holders.iter().enumerate() {
            let mut older = false;
            for (other, other_blob, version) in &holders {
                if other == commit || same_version(record, version) {
                    continue;
                }
                let key = if record.criteria.signature() == version.criteria.signature() {
                    Version::standing
                } else {
                    Version::criteria
                };
                let held = Holding::new(*commit, *blob, key(record));
                let newer = Holding::new(*other, *other_blob, key(version));
                if self.yields(task, &held, &newer)? {
                    older = true;
                    break;
                }
            }
            if !older {
                newest.push(index);
            }
        }
        let Some(&first) = newest.first() else {
            return if holders.is_empty() {
                Ok(None)
            } else {
                Err("the target no longer holds this task, and each line of its history holds a version another line replaced, so which one it keeps cannot be told".to_string())
            };
        };
        let kept = &holders[first].2;
        if let Some(&other) = newest
            .iter()
            .find(|&&index| !same_version(kept, &holders[index].2))
        {
            return Err(format!(
                "the target no longer holds this task, and lines of its history hold different versions of it (one at {:.9}), so which one it keeps cannot be told",
                holders[other].0
            ));
        }
        Ok(Some(kept.clone()))
    }

    /// Whether every introduction of `older`'s version precedes every
    /// introduction of `newer`'s. Comparing decisions, rather than tips,
    /// keeps a restored version competing even after its line merged the
    /// other version.
    fn yields(
        &mut self,
        task: &Identity,
        older: &Holding,
        newer: &Holding,
    ) -> Result<bool, String> {
        let earlier = self
            .introductions(task, older.commit, &older.version)
            .map_err(|reason| older.context(&reason))?;
        let later = self
            .introductions(task, newer.commit, &newer.version)
            .map_err(|reason| newer.context(&reason))?;
        for before in earlier {
            for &after in &later {
                if before == after
                    || !super::acceptance::is_ancestor_or_same(self.repo, before, after)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Every introduction of `version` in the complete, unambiguous history
    /// of `holder`, walked once per holder and version in a run.
    fn introductions(
        &mut self,
        task: &Identity,
        holder: Oid,
        version: &Version,
    ) -> Result<Vec<Oid>, String> {
        let key = Traced {
            id: task.id.clone(),
            uid: task.uid.clone(),
            own_file: task.own_file.clone(),
            holder,
            version: version.clone(),
        };
        if let Some(found) = self.introduced.get(&key) {
            return found.clone();
        }
        let found = self.trace(task, holder, version);
        self.introduced.insert(key, found.clone());
        found
    }

    /// Walk `holder`'s history for the commits that introduce `version`: a
    /// commit holds it while some parent holds no record of the task or
    /// another version, or it has no parent. A merge that resolves the
    /// record to one parent's version against the other's therefore
    /// introduces the version it keeps, whatever its bytes; a version is
    /// what its caller compares, never the blob, so a resolution that keeps
    /// one side's criteria with new text still introduces them (SEC-306-01).
    fn trace(
        &mut self,
        task: &Identity,
        holder: Oid,
        version: &Version,
    ) -> Result<Vec<Oid>, String> {
        if let Some(overlay) = &self.overlay {
            return Err(overlay.clone());
        }
        let unreadable =
            |error: git2::Error| format!("cannot read the history: {}", error.message());
        let mut walk = history::revwalk(self.repo).map_err(unreadable)?;
        walk.push(holder).map_err(unreadable)?;
        let mut introduced = Vec::new();
        let mut uids: BTreeSet<String> = task.uid.iter().cloned().collect();
        // Whether each blob holds the version, read once per walk.
        let mut holds: HashMap<Oid, bool> = HashMap::new();
        for commit in walk {
            let commit = commit.map_err(unreadable)?;
            let held = self.history_at(task, commit, &mut uids)?;
            if !version.held(&held, &mut holds) {
                continue;
            }
            let parents = history::parents(
                self.repo,
                &self.repo.find_commit(commit).map_err(unreadable)?,
            )
            .map_err(unreadable)?;
            let mut decided = parents.is_empty();
            for parent in &parents {
                let held = self.history_at(task, *parent, &mut uids)?;
                if !version.held(&held, &mut holds) {
                    decided = true;
                    break;
                }
            }
            if decided {
                introduced.push(commit);
            }
        }
        if introduced.is_empty() {
            return Err("no introduction of this version could be proved".to_string());
        }
        Ok(introduced)
    }

    /// A historical tree must be readable before it can prove inheritance.
    fn history_at(
        &mut self,
        task: &Identity,
        commit: Oid,
        uids: &mut BTreeSet<String>,
    ) -> Result<Held, String> {
        if self.boundary.contains(&commit) {
            return Err(format!(
                "this clone is shallow at {commit}, so the history before it cannot be read; fetch it in full (`git fetch --unshallow`)"
            ));
        }
        let held = self.held_at(commit, task)?;
        if let Some(doubt) = &held.doubt {
            return Err(doubt.clone());
        }
        uids.extend(held.uids.iter().cloned());
        if let Some(doubt) = two_uids(task, uids) {
            return Err(doubt);
        }
        Ok(held)
    }

    /// The first commit, newest first, that `judged` reaches and `hide`
    /// does not and that holds a record of the task, with that record's
    /// blob and parsed record.
    fn first_holder(
        &mut self,
        task: &Identity,
        judged: &[Oid],
        hide: &[Oid],
    ) -> Result<Option<(Oid, Oid, RecordView)>, String> {
        let unreadable =
            |error: git2::Error| format!("cannot read the history: {}", error.message());
        let mut walk = history::revwalk(self.repo).map_err(unreadable)?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)
            .map_err(unreadable)?;
        for point in judged {
            walk.push(*point).map_err(unreadable)?;
        }
        for point in hide {
            walk.hide(*point).map_err(unreadable)?;
        }
        let mut uids: BTreeSet<String> = task.uid.iter().cloned().collect();
        for commit in walk {
            let commit = commit.map_err(unreadable)?;
            let held = self.history_at(task, commit, &mut uids)?;
            if let Some((blob, record)) = held.records.into_iter().next() {
                return Ok(Some((commit, blob, record)));
            }
        }
        Ok(None)
    }

    /// The refs outside the judged points that make the answer unknown: a
    /// target tip that shows a completion, then a branch or remote-tracking
    /// ref that adds one; `Planned` when none does.
    fn outside(&mut self, task: &Identity, judged: &[Oid], outside: &Outside<'_>) -> Landing {
        for point in outside.own {
            match self.held_at(*point, task) {
                Ok(held) => {
                    if let Some(doubt) = held.doubt {
                        return Landing::Unreadable(doubt);
                    }
                }
                Err(reason) => return Landing::Unreadable(reason),
            }
        }
        for (reference, tip) in outside.tips {
            let held = match self.held_at(*tip, task) {
                Ok(held) => held,
                Err(reason) => return Landing::Unreadable(reason),
            };
            if let Some(doubt) = held.doubt {
                return Landing::Unreadable(doubt);
            }
            let reaches = match outside.at {
                Some(at) => match super::acceptance::is_ancestor_or_same(self.repo, at, *tip) {
                    Ok(reaches) => reaches,
                    Err(reason) => return Landing::Unreadable(reason),
                },
                None => false,
            };
            let shows = held
                .records
                .iter()
                .any(|(_, record)| super::acceptance::shows_completion(record));
            if (shows || reaches) && !held.records.is_empty() {
                return Landing::Unjudged {
                    reference: reference.clone(),
                    commit: *tip,
                };
            }
        }
        let tips: Vec<Oid> = outside.tips.iter().map(|(_, tip)| *tip).collect();
        if !tips.is_empty() {
            let history = self.walk(task, &tips, judged);
            if let Some(commit) = history.witness {
                let mut reference = commit.to_string();
                for (name, tip) in outside.tips {
                    match super::acceptance::is_ancestor_or_same(self.repo, commit, *tip) {
                        Ok(true) => {
                            reference.clone_from(name);
                            break;
                        }
                        Ok(false) => {}
                        Err(reason) => return Landing::Unreadable(reason),
                    }
                }
                return Landing::Unjudged { reference, commit };
            }
            if let Some(doubt) = history.doubt {
                return Landing::Unreadable(doubt);
            }
        }
        match self.holder_outside(task, outside.head) {
            Ok(Some((holder, commit))) => Landing::Elsewhere { holder, commit },
            Ok(None) => Landing::Planned,
            Err(reason) => Landing::Unreadable(reason),
        }
    }

    /// A branch or remote-tracking ref that records a completion of the
    /// task outside the range ending at `head`: some commit it reaches and
    /// `head` does not adds the task's record, or changes it from every
    /// parent's version, to a version that shows a completion. A planned
    /// record held elsewhere is no landing (issue #67), and a copy inherited
    /// from the range's own commits (a branch stacked on this one, or this
    /// branch's remote copy with unrelated commits on top) adds nothing.
    ///
    /// # Errors
    ///
    /// Returns a message when the refs or their history cannot be read, so
    /// the caller fails closed.
    fn holder_outside(
        &mut self,
        task: &Identity,
        head: Oid,
    ) -> Result<Option<(String, Oid)>, String> {
        let unreadable =
            |error: git2::Error| format!("cannot read the branches of this clone: {error}");
        let mut refs = Vec::new();
        for reference in self.repo.references().map_err(unreadable)? {
            let reference = reference.map_err(unreadable)?;
            // OS text rule (issue 79): a ref that holds the task outside the
            // range counts when its name is not UTF-8, and one that cannot be
            // peeled refuses; only the holder named is the display form.
            let name = crate::git::name::reference_name(&reference);
            let tracked = name.starts_with(b"refs/heads/") || name.starts_with(b"refs/remotes/");
            if !tracked || reference.kind() == Some(git2::ReferenceType::Symbolic) {
                continue;
            }
            let commit = reference.peel_to_commit().map_err(unreadable)?;
            let short = crate::git::name::reference_shorthand(&reference)
                .display()
                .to_string();
            refs.push((name, short, commit.id()));
        }
        refs.sort();
        let mut walk = history::revwalk(self.repo).map_err(unreadable)?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)
            .map_err(unreadable)?;
        for (_, _, tip) in &refs {
            walk.push(*tip).map_err(unreadable)?;
        }
        walk.hide(head).map_err(unreadable)?;
        let commits = walk.collect::<Result<Vec<_>, _>>().map_err(unreadable)?;
        for commit in commits {
            let found = self.held_at(commit, task)?;
            if found.records.is_empty() {
                continue;
            }
            let parents = history::parents(
                self.repo,
                &self.repo.find_commit(commit).map_err(unreadable)?,
            )
            .map_err(unreadable)?;
            let mut inherited = HashSet::new();
            for parent in parents {
                inherited.extend(
                    self.held_at(parent, task)?
                        .records
                        .into_iter()
                        .map(|(blob, _)| blob),
                );
            }
            let adds = found.records.iter().any(|(blob, record)| {
                !inherited.contains(blob) && super::acceptance::shows_completion(record)
            });
            if !adds {
                continue;
            }
            let mut holder = commit.to_string();
            for (_, short, tip) in &refs {
                if super::acceptance::is_ancestor_or_same(self.repo, commit, *tip)? {
                    holder.clone_from(short);
                    break;
                }
            }
            return Ok(Some((holder, commit)));
        }
        Ok(None)
    }
}

/// Whether a task file may name `task`, whether it parses or not: its
/// text holds the task's id or uid, or the text the frontmatter parser
/// reads as YAML holds a backslash, with which a double-quoted value can
/// spell either by escapes. YAML has no other way to build a string from
/// text that does not hold it, so a file with none of these is another
/// task's (TSK-234 review rounds 6 and 7). That holds for a supported id
/// and a canonical uid, which are strings; a uid in any other form may be
/// a boolean or number another spelling also reads as (`TRUE` and `true`),
/// so every file may name a task that carries one (round 13).
fn may_spell(content: &str, task: &Identity) -> bool {
    if task
        .uid
        .as_deref()
        .is_some_and(|uid| !crate::ids::is_uid(uid))
    {
        return true;
    }
    let literal = content.contains(task.id.as_str())
        || task.uid.as_deref().is_some_and(|uid| content.contains(uid));
    literal || crate::validate::frontmatter_scope(content).contains('\\')
}

/// Describe the versions read at the fixed points, including missing or
/// ambiguous records, without substituting a landing witness for authority.
fn held_context(points: &[(Oid, Held)]) -> String {
    points
        .iter()
        .map(|(point, held)| {
            if held.records.is_empty() {
                format!(
                    "{point:.9} holds {}",
                    if held.doubt.is_some() {
                        "an unreadable version"
                    } else {
                        "no record"
                    }
                )
            } else {
                let blobs = held
                    .records
                    .iter()
                    .map(|(blob, _)| format!("{blob:.9}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{point:.9} holds {blobs}")
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn criteria_source(points: &[(Oid, Held)], authority: &Authority, unordered: bool) -> String {
    let Authority::Held(kept) = authority else {
        let state = if matches!(authority, Authority::Absent) {
            "absent"
        } else {
            "unknown"
        };
        return format!(
            "criteria authority {state}; judged points: {}",
            held_context(points)
        );
    };
    for (point, held) in points {
        if let Some((blob, _)) = held
            .records
            .iter()
            .find(|(_, record)| record.content == kept.content)
        {
            let mut source = format!("criteria authority: {point:.9} holds {blob:.9}");
            if unordered {
                for (other, version) in points {
                    if let Some((old_blob, old)) = version.records.first() {
                        if old.criteria.signature() != kept.criteria.signature() {
                            let _ =
                                write!(source, "; {other:.9} holds older version {old_blob:.9}");
                        }
                    }
                }
            }
            return source;
        }
    }
    format!(
        "criteria authority recovered from the judged history; judged points: {}",
        held_context(points)
    )
}

/// What makes two records of a task one version for a caller: the criteria
/// signature the freeze compares, for the versions judged points hold
/// (`agreed`) and for lines of a history the points no longer hold whose
/// criteria differ (`recovered`); and with it the status and epic
/// ([`same_version`]) only for such lines whose criteria agree.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Version {
    criteria: Vec<(String, String)>,
    standing: Option<(String, Option<String>)>,
}

impl Version {
    /// The version `record` holds, by its criteria alone.
    fn criteria(record: &RecordView) -> Self {
        Self {
            criteria: record.criteria.signature(),
            standing: None,
        }
    }

    /// The version `record` holds, by its criteria, status and epic.
    fn standing(record: &RecordView) -> Self {
        Self {
            criteria: record.criteria.signature(),
            standing: Some((record.status.clone(), record.epic_id.clone())),
        }
    }

    /// Whether `record` is this version.
    fn is(&self, record: &RecordView) -> bool {
        record.criteria.signature() == self.criteria
            && self
                .standing
                .as_ref()
                .is_none_or(|(status, epic)| *status == record.status && *epic == record.epic_id)
    }

    /// Whether `held` holds this version, reading each blob once through
    /// `seen`.
    fn held(&self, held: &Held, seen: &mut HashMap<Oid, bool>) -> bool {
        held.records
            .iter()
            .any(|(blob, record)| *seen.entry(*blob).or_insert_with(|| self.is(record)))
    }
}

/// What one walk of [`RecordStore::trace`] answers for: one task, by its
/// whole identity, one holder and one version.
#[derive(PartialEq, Eq, Hash)]
struct Traced {
    id: String,
    uid: Option<String>,
    own_file: String,
    holder: Oid,
    version: Version,
}

/// A version held at a commit, with the blob that names it in messages.
struct Holding {
    commit: Oid,
    blob: Oid,
    version: Version,
}

impl Holding {
    fn new(commit: Oid, blob: Oid, version: Version) -> Self {
        Self {
            commit,
            blob,
            version,
        }
    }

    /// `reason`, prefixed with the history it was read in.
    fn context(&self, reason: &str) -> String {
        format!(
            "reading the history of {:.9} ({:.9}): {reason}",
            self.commit, self.blob
        )
    }
}

/// Whether two versions of a task's record keep the same authority: the
/// same criteria, status and epic.
fn same_version(one: &RecordView, other: &RecordView) -> bool {
    one.criteria.signature() == other.criteria.signature()
        && one.status == other.status
        && one.epic_id == other.epic_id
}

/// Why the uids `seen` with the task's id make its identity ambiguous:
/// one id carried with two uids is reuse, so no version can be told to be
/// this task's. Successive versions and records without a uid are one task.
fn two_uids(task: &Identity, seen: &BTreeSet<String>) -> Option<String> {
    (seen.len() > 1).then(|| {
        let uids: Vec<&str> = seen.iter().map(String::as_str).collect();
        format!(
            "the history carries the id {} with another uid ({}), so which task each version is cannot be told",
            task.id,
            uids.join(" and ")
        )
    })
}

/// What a walk of one task's history found ([`RecordStore::walk`]).
#[derive(Default)]
struct Walk {
    /// The first commit, newest first, whose version shows a completion.
    witness: Option<Oid>,
    /// Why the walk proves no absence, when it does not.
    doubt: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = crate::git::command()
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn record(uid: &str) -> String {
        format!("---\nid: TSK-001\nuid: {uid}\nstatus: todo\nintegration_target: main\n---\n\n# Work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n")
    }

    /// A repository whose `main` holds a planned TSK-001 written with
    /// `first`, then rewritten with `second` uid, then two more commits.
    fn history(first: &str, second: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let path = root.join("project-management/tasks/TSK-001.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (step, text) in [record(first), record(second), String::new(), String::new()]
            .into_iter()
            .enumerate()
        {
            if text.is_empty() {
                std::fs::write(root.join(format!("note-{step}.txt")), "note\n").unwrap();
            } else {
                std::fs::write(&path, text).unwrap();
            }
            git(root, &["add", "-A"]);
            git(
                root,
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    &format!("step {step}"),
                ],
            );
        }
        dir
    }

    fn judge(root: &std::path::Path, uid: &str) -> TaskJudgement {
        let repo = Repository::open(root).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap().id();
        let identity = Identity::named("TSK-001", Some(uid.to_string()), "TSK-001.md");
        RecordStore::new(&repo).judge(
            &identity,
            &[head],
            &Outside {
                head,
                tips: &[],
                at: None,
                own: &[],
            },
        )
    }

    /// What the store reads of TSK-001 in the tree of `head`.
    fn held(repo: &Repository, head: Oid) -> Result<Held, String> {
        RecordStore::new(repo).held_at(head, &Identity::named("TSK-001", None, "TSK-001.md"))
    }

    fn remove_object(repo: &Repository, oid: Oid) {
        let hex = oid.to_string();
        std::fs::remove_file(repo.path().join("objects").join(&hex[..2]).join(&hex[2..])).unwrap();
    }

    /// Issue 79: a task file that does not parse may be this task, and a
    /// file that is not UTF-8 is never parsed from a lossy rendering.
    #[test]
    fn r22_a_malformed_or_undecodable_task_file_is_doubt() {
        for (name, text) in [
            (
                b"project-management/tasks/TSK-999.md".as_slice(),
                b"---\nid: TSK-001\nstatus: [unclosed\n---\n".as_slice(),
            ),
            (
                b"project-management/tasks/TSK-001.md".as_slice(),
                b"---\nid: TSK-001\nstatus: todo\nintegration_target: caf\xff\n---\n".as_slice(),
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (repo, head) = crate::git::repo_with_tree(dir.path(), &[(name, text)]);
            let found = held(&repo, head).unwrap();
            assert!(found.doubt.is_some(), "{}", String::from_utf8_lossy(name));
            assert!(found.records.is_empty());
        }
    }

    /// Issue 79: a tree a task directory names that cannot be read, and an
    /// epic directory whose name is not UTF-8, refuse instead of reading
    /// as no record.
    #[test]
    fn r22_unreadable_task_directories_refuse() {
        for path in [
            "project-management",
            "project-management/tasks",
            "project-management/epics",
            "project-management/epics/EPC-001",
            "project-management/epics/EPC-001/tasks",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (repo, head) = crate::git::repo_with_tree(
                dir.path(),
                &[
                    (
                        b"project-management/tasks/TSK-001.md",
                        b"---\nid: TSK-001\nstatus: todo\n---\n",
                    ),
                    (
                        b"project-management/epics/EPC-001/tasks/TSK-002.md",
                        b"---\nid: TSK-002\nstatus: todo\n---\n",
                    ),
                ],
            );
            let tree = repo
                .find_commit(head)
                .unwrap()
                .tree()
                .unwrap()
                .get_path(std::path::Path::new(path))
                .unwrap()
                .id();
            remove_object(&repo, tree);
            drop(repo);
            let repo = Repository::open(dir.path()).unwrap();
            assert!(held(&repo, head).is_err(), "{path}");
        }
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(
            dir.path(),
            &[(
                b"project-management/epics/EPC-001\xff/tasks/TSK-001.md",
                b"---\nid: TSK-001\nstatus: todo\n---\n",
            )],
        );
        assert!(held(&repo, head).is_err());
    }

    /// The control: absent directories, an entry of another kind, a file
    /// that is no task record and another task's record hold nothing.
    #[test]
    fn r22_absent_directories_and_other_tasks_hold_nothing() {
        for files in [
            vec![],
            vec![(
                b"project-management".as_slice(),
                b"ordinary file".as_slice(),
            )],
            vec![(
                b"project-management/epics/EPC-001\xff/tasks/note.md".as_slice(),
                b"not a task record".as_slice(),
            )],
            vec![(
                b"project-management/tasks/TSK-002.md".as_slice(),
                b"---\nid: TSK-002\nstatus: todo\n---\n".as_slice(),
            )],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (repo, head) = crate::git::repo_with_tree(dir.path(), &files);
            let found = held(&repo, head).unwrap();
            assert!(found.records.is_empty() && found.doubt.is_none());
        }
    }

    /// Issue 79: a branch that cannot be peeled to a commit refuses the
    /// search for a holder outside the range instead of being skipped.
    #[test]
    fn r22_holder_refuses_unpeelable_branch() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(dir.path(), &[]);
        let task = Identity::named("TSK-001", None, "TSK-001.md");
        let blob = repo.blob(b"not a commit").unwrap();
        std::fs::write(repo.path().join("refs/heads/broken"), format!("{blob}\n")).unwrap();
        assert!(RecordStore::new(&repo).holder_outside(&task, head).is_err());
        std::fs::remove_file(repo.path().join("refs/heads/broken")).unwrap();
        assert!(RecordStore::new(&repo)
            .holder_outside(&task, head)
            .unwrap()
            .is_none());
    }

    const UID: &str = "6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d";
    const OTHER: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    /// A complete history with one identity proves a planned task.
    #[test]
    fn a_complete_history_proves_a_planned_task() {
        let dir = history(UID, UID);
        let judgement = judge(dir.path(), UID);
        assert!(matches!(judgement.landing, Landing::Planned));
        assert!(matches!(judgement.criteria, Authority::Held(_)));
    }

    /// A shallow clone cuts the history, so it never proves a task planned
    /// (TSK-234 design D2, proof of absence).
    #[test]
    fn a_shallow_clone_gives_no_planned_answer() {
        let dir = history(UID, UID);
        let clone = tempfile::tempdir().unwrap();
        let url = format!("file://{}", dir.path().display());
        codeflow_fixture::clone(clone.path(), &url, "copy")
            .depth(2)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .run();
        let judgement = judge(&clone.path().join("copy"), UID);
        match judgement.landing {
            Landing::Unreadable(reason) => assert!(reason.contains("shallow"), "{reason}"),
            _ => panic!("a shallow clone proved an answer"),
        }
    }

    /// One id carried with two uids in the judged history is ambiguous
    /// reuse: the landing is unknown, never planned (TSK-234 design D2).
    #[test]
    fn one_id_with_two_uids_gives_no_planned_answer() {
        let dir = history(OTHER, UID);
        let judgement = judge(dir.path(), UID);
        match judgement.landing {
            Landing::Unreadable(reason) => assert!(reason.contains("another uid"), "{reason}"),
            _ => panic!("an ambiguous identity proved an answer"),
        }
    }

    /// A line forked before the target completed and deleted the task, and
    /// merged after, still holds the older planned version. That version is
    /// older on the target's own line, so the completed version is the
    /// authority (TSK-234 review round 5).
    #[test]
    fn a_stale_line_does_not_compete_with_the_newest_version() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let path = root.join("project-management/tasks/TSK-001.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, record(UID)).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "plan"]);
        git(root, &["switch", "-q", "-c", "stale"]);
        std::fs::write(root.join("note.txt"), "note\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "stale work"]);
        git(root, &["switch", "-q", "main"]);
        std::fs::write(
            &path,
            record(UID).replace("status: todo", "status: complete"),
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "complete"]);
        std::fs::remove_file(&path).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "delete"]);
        git(root, &["merge", "-q", "--no-ff", "--no-edit", "stale"]);
        assert!(!path.exists(), "the merge keeps the deletion");
        let judgement = judge(root, UID);
        assert!(matches!(judgement.landing, Landing::Landed { .. }));
        match judgement.criteria {
            Authority::Held(record) => assert_eq!(record.status, "complete"),
            Authority::Absent => panic!("absent"),
            Authority::Unknown(reason) => panic!("unknown: {reason}"),
        }
    }

    const LINE: &str = "integration/EPC-001-line";
    const RECORD: &str = "project-management/tasks/TSK-001.md";

    /// One commit's change to a fixture tree.
    enum Step {
        /// A commit that changes no record.
        Note,
        /// Write `text` at `path`.
        File(&'static str, String),
        /// Delete the task's record.
        Delete,
    }

    /// A repository whose `main` commits `base`, then forks [`LINE`]; each
    /// branch then commits its own steps, so neither tip is an ancestor of
    /// the other, as when a line has diverged from `main`.
    fn diverged(base: Step, main: Vec<Step>, line: Vec<Step>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let mut count = 0;
        let mut apply = |step: Step| {
            count += 1;
            match step {
                Step::Note => {
                    std::fs::write(root.join(format!("note-{count}.txt")), "note\n").unwrap();
                }
                Step::File(path, text) => {
                    let path = root.join(path);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, text).unwrap();
                }
                Step::Delete => std::fs::remove_file(root.join(RECORD)).unwrap(),
            }
            git(root, &["add", "-A"]);
            git(root, &["commit", "-q", "-m", &format!("step {count}")]);
        };
        apply(base);
        git(root, &["branch", LINE]);
        for step in main {
            apply(step);
        }
        git(root, &["switch", "-q", LINE]);
        for step in line {
            apply(step);
        }
        dir
    }

    /// Judge TSK-001 at the tips `points` name, in that order, as the
    /// pre-push does with the line's tip as the range anchor and `main`'s
    /// tip as the candidate authority.
    fn judge_at(root: &std::path::Path, points: &[&str]) -> TaskJudgement {
        let repo = Repository::open(root).unwrap();
        let judged: Vec<Oid> = points
            .iter()
            .map(|name| {
                repo.revparse_single(name)
                    .unwrap()
                    .peel_to_commit()
                    .unwrap()
                    .id()
            })
            .collect();
        let identity = Identity::named("TSK-001", Some(UID.to_string()), "TSK-001.md");
        RecordStore::new(&repo).judge(
            &identity,
            &judged,
            &Outside {
                head: judged[0],
                tips: &[],
                at: None,
                own: &[],
            },
        )
    }

    /// TSK-001's record with its one criterion reading `criterion`.
    fn with_criterion(criterion: &str) -> String {
        record(UID).replace("When run, the system shall work.", criterion)
    }

    /// Issue 153: a line that has diverged from `main` holds a task `main`
    /// never held. `main` holds no version, so the line's version is the
    /// authority, in either order of the judged points.
    #[test]
    fn a_point_that_never_held_the_task_holds_no_version() {
        let dir = diverged(
            Step::Note,
            vec![Step::Note],
            vec![Step::File(RECORD, record(UID))],
        );
        for points in [[LINE, "main"], ["main", LINE]] {
            let judgement = judge_at(dir.path(), &points);
            assert!(matches!(judgement.landing, Landing::Planned), "{points:?}");
            match judgement.criteria {
                Authority::Held(held) => assert_eq!(
                    held.criteria.signature(),
                    RecordView::parse(RecordKind::Task, RECORD, &record(UID))
                        .unwrap()
                        .criteria
                        .signature(),
                    "{points:?}"
                ),
                Authority::Absent => panic!("{points:?}: absent"),
                Authority::Unknown(reason) => panic!("{points:?}: unknown: {reason}"),
            }
        }
    }

    /// Issue 176: main inherited v1, and the diverged line replaced it.
    /// This was the refusing shape before held inheritance was considered.
    #[test]
    fn a_version_the_line_replaced_does_not_compete() {
        let newer = with_criterion("When run, the system shall work fast.");
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::Note],
            vec![Step::File(RECORD, newer.clone())],
        );
        let signature = RecordView::parse(RecordKind::Task, RECORD, &newer)
            .unwrap()
            .criteria
            .signature();
        for points in [[LINE, "main"], ["main", LINE]] {
            match judge_at(dir.path(), &points).criteria {
                Authority::Held(held) => assert_eq!(held.criteria.signature(), signature),
                Authority::Unknown(reason) => panic!("{points:?}: {reason}"),
                Authority::Absent => panic!("{points:?}: absent"),
            }
        }
    }

    /// Concurrent amendments still compete. The unchanged-main shape now
    /// belongs to `a_version_the_line_replaced_does_not_compete`.
    #[test]
    fn held_versions_that_differ_still_give_no_answer() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall stop."),
            )],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        for points in [[LINE, "main"], ["main", LINE]] {
            match judge_at(dir.path(), &points).criteria {
                Authority::Unknown(reason) => {
                    assert!(reason.contains("different versions"), "{reason}");
                }
                _ => panic!("two different held versions gave an answer"),
            }
        }
    }

    fn positive_shape() -> tempfile::TempDir {
        diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::Note],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        )
    }

    fn refuses_both_orders(root: &std::path::Path, reason: &str) {
        for points in [[LINE, "main"], ["main", LINE]] {
            match judge_at(root, &points).criteria {
                Authority::Unknown(why) => assert!(why.contains(reason), "{points:?}: {why}"),
                _ => panic!("{points:?}: uncertain history selected authority"),
            }
        }
    }

    fn save(root: &std::path::Path, path: &str, text: &str) {
        std::fs::write(root.join(path), text).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "fixture change"]);
    }

    /// `text` with a new description line and the same criteria.
    fn reworded(text: &str) -> String {
        text.replace("# Work\n", "# Work\n\nReworded after a merge.\n")
    }

    /// Merge the amended line into main, then deliberately choose v1's
    /// criteria, written as `v1`. Advancing the line afterwards keeps the
    /// judged tips unordered.
    fn main_chooses_old_version(in_merge: bool, v1: &str) -> tempfile::TempDir {
        let dir = positive_shape();
        let root = dir.path();
        git(root, &["switch", "main"]);
        if in_merge {
            git(root, &["merge", "--no-ff", "--no-commit", LINE]);
            save(root, RECORD, v1);
        } else {
            git(root, &["merge", "--no-ff", "-m", "take v2", LINE]);
            save(root, RECORD, v1);
        }
        git(root, &["switch", LINE]);
        save(root, "later.txt", "the line advanced\n");
        let repo = Repository::open(root).unwrap();
        let main = repo.revparse_single("main").unwrap().id();
        let line = repo.revparse_single(LINE).unwrap().id();
        assert!(!repo.graph_descendant_of(main, line).unwrap());
        assert!(!repo.graph_descendant_of(line, main).unwrap());
        dir
    }

    #[test]
    fn a_revert_to_the_inherited_bytes_still_competes() {
        let dir = main_chooses_old_version(false, &record(UID));
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn a_merge_resolution_to_the_inherited_bytes_still_competes() {
        let dir = main_chooses_old_version(true, &record(UID));
        refuses_both_orders(dir.path(), "different versions");
    }

    /// SEC-306-01: a revert to v1's criteria with new text is still a
    /// decision for v1, so main does not inherit the line's v2 by bytes.
    #[test]
    fn a_revert_to_the_inherited_criteria_with_new_text_still_competes() {
        let dir = main_chooses_old_version(false, &reworded(&record(UID)));
        refuses_both_orders(dir.path(), "different versions");
    }

    /// SEC-306-01: the same for a merge resolution on main.
    #[test]
    fn a_merge_resolution_to_the_inherited_criteria_with_new_text_still_competes() {
        let dir = main_chooses_old_version(true, &reworded(&record(UID)));
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn a_fresh_main_version_replaces_the_lines_inherited_version() {
        let dir = positive_shape();
        let root = dir.path();
        git(root, &["switch", "main"]);
        git(root, &["merge", "--no-ff", "-m", "take v2", LINE]);
        save(
            root,
            RECORD,
            &with_criterion("When run, the system shall work safely."),
        );
        git(root, &["switch", LINE]);
        save(root, "later.txt", "the line advanced\n");
        for points in [[LINE, "main"], ["main", LINE]] {
            let Authority::Held(held) = judge_at(root, &points).criteria else {
                panic!("{points:?}: inherited v2 competed with fresh v3");
            };
            assert!(held.content.contains("work safely"));
        }
    }

    #[test]
    fn a_merge_resolution_on_the_line_still_competes() {
        let dir = line_resolves_concurrent_amendments(&with_criterion(
            "When run, the system shall work fast.",
        ));
        refuses_both_orders(dir.path(), "different versions");
    }

    /// SEC-306-01: a line that resolves a conflict with main's concurrent
    /// amendment to its own criteria, with new text, keeps its earlier
    /// decision, so main's amendment still competes in both point orders.
    #[test]
    fn a_merge_resolution_on_the_line_with_new_text_still_competes() {
        let dir = line_resolves_concurrent_amendments(&reworded(&with_criterion(
            "When run, the system shall work fast.",
        )));
        refuses_both_orders(dir.path(), "different versions");
    }

    /// Main and the line amend the criteria concurrently; the line merges
    /// main, resolving the record's conflict to `resolved`; main then
    /// advances so the tips stay unordered.
    fn line_resolves_concurrent_amendments(resolved: &str) -> tempfile::TempDir {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall stop."),
            )],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        let root = dir.path();
        let merge = crate::git::command()
            .args(["merge", "--no-ff", "--no-commit", "main"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(!merge.status.success(), "the record must conflict");
        assert!(Repository::open(root)
            .unwrap()
            .path()
            .join("MERGE_HEAD")
            .exists());
        save(root, RECORD, resolved);
        git(root, &["switch", "main"]);
        save(root, "later.txt", "main advanced\n");
        dir
    }

    #[test]
    fn an_independent_reintroduction_of_the_same_bytes_still_competes() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::Delete, Step::File(RECORD, record(UID))],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        refuses_both_orders(dir.path(), "different versions");
    }

    /// Introductions are traced by criteria signature (SEC-306-01), so an
    /// edit on main that keeps v1's criteria decides nothing about them:
    /// main still holds the criteria the line replaced.
    #[test]
    fn a_noncriteria_record_edit_on_main_does_not_compete() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                RECORD,
                format!("{}\nA main-side note.\n", record(UID)),
            )],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        for points in [[LINE, "main"], ["main", LINE]] {
            let Authority::Held(held) = judge_at(dir.path(), &points).criteria else {
                panic!("{points:?}: a text edit on main competed with the line's criteria");
            };
            assert!(held.content.contains("work fast"), "{points:?}");
        }
    }

    #[test]
    fn shallow_history_cannot_prove_held_inheritance() {
        let dir = positive_shape();
        let clone = tempfile::tempdir().unwrap();
        codeflow_fixture::clone(
            clone.path(),
            format!("file://{}", dir.path().display()),
            "copy",
        )
        .depth(1)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
        let root = clone.path().join("copy");
        git(
            &root,
            &["fetch", "--depth=1", "origin", "main:refs/heads/main"],
        );
        refuses_both_orders(&root, "shallow");
    }

    #[test]
    fn grafted_history_cannot_prove_held_inheritance() {
        let dir = positive_shape();
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo.revparse_single("main").unwrap().id();
        std::fs::create_dir_all(repo.path().join("info")).unwrap();
        std::fs::write(repo.path().join("info/grafts"), format!("{main}\n")).unwrap();
        refuses_both_orders(dir.path(), "graft");
    }

    #[test]
    fn replaced_history_cannot_prove_held_inheritance() {
        let dir = positive_shape();
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo
            .revparse_single("main")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        let parent = main.parent_id(0).unwrap();
        repo.reference(
            &format!("refs/replace/{}", main.id()),
            parent,
            false,
            "fixture",
        )
        .unwrap();
        refuses_both_orders(dir.path(), "replace");
    }

    #[test]
    fn a_duplicate_identity_at_a_point_refuses_held_inheritance() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                "project-management/tasks/TSK-999.md",
                record(OTHER),
            )],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        refuses_both_orders(dir.path(), "another uid");
    }

    #[test]
    fn a_deleted_duplicate_identity_refuses_held_inheritance() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                "project-management/tasks/TSK-999.md",
                record(OTHER),
            )],
            vec![Step::File(
                RECORD,
                with_criterion("When run, the system shall work fast."),
            )],
        );
        git(dir.path(), &["switch", "main"]);
        git(dir.path(), &["rm", "project-management/tasks/TSK-999.md"]);
        git(dir.path(), &["commit", "-m", "delete duplicate"]);
        refuses_both_orders(dir.path(), "another uid");
    }

    #[test]
    fn recovered_inheritance_refuses_doubt_below_the_first_holder() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![
                Step::File(RECORD, "---\nid: TSK-001\nstatus: [broken\n---\n".into()),
                Step::File(RECORD, record(UID)),
                Step::Delete,
            ],
            vec![
                Step::File(
                    RECORD,
                    with_criterion("When run, the system shall work fast."),
                ),
                Step::Delete,
            ],
        );
        refuses_both_orders(dir.path(), "does not parse");
    }

    #[test]
    fn equal_held_criteria_still_agree_across_status_and_epic_changes() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::File(
                RECORD,
                record(UID).replace("status: todo", "status: complete"),
            )],
            vec![Step::File(
                RECORD,
                record(UID).replace("status: todo", "epic_id: EPC-001\nstatus: todo"),
            )],
        );
        for points in [[LINE, "main"], ["main", LINE]] {
            assert!(matches!(
                judge_at(dir.path(), &points).criteria,
                Authority::Held(_)
            ));
        }
    }

    #[test]
    fn held_inheritance_handles_more_than_two_points() {
        let dir = positive_shape();
        git(dir.path(), &["switch", "-c", "another", "main"]);
        save(dir.path(), "another.txt", "another unrelated commit\n");
        for points in [[LINE, "main", "another"], ["another", "main", LINE]] {
            let Authority::Held(held) = judge_at(dir.path(), &points).criteria else {
                panic!("{points:?}: inherited versions competed");
            };
            assert!(held.content.contains("work fast"));
        }
    }

    /// A point that no longer holds the task but held a version with other
    /// criteria disagrees; one that only deleted the version the other
    /// point holds does not.
    #[test]
    fn an_absent_point_disagrees_only_through_another_version_in_its_history() {
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![
                Step::File(RECORD, with_criterion("When run, the system shall stop.")),
                Step::Delete,
            ],
            vec![Step::Note],
        );
        match judge_at(dir.path(), &[LINE, "main"]).criteria {
            Authority::Unknown(reason) => {
                assert!(reason.contains("different versions"), "{reason}");
            }
            _ => panic!("a deleted version with other criteria was passed over"),
        }
        let dir = diverged(
            Step::File(RECORD, record(UID)),
            vec![Step::Delete],
            vec![Step::Note],
        );
        assert!(matches!(
            judge_at(dir.path(), &[LINE, "main"]).criteria,
            Authority::Held(_)
        ));
    }

    /// A point that holds no record of the task but a file that may be it
    /// and does not parse carries a doubt, which still refuses.
    #[test]
    fn a_doubt_at_a_point_without_the_record_still_gives_no_answer() {
        let dir = diverged(
            Step::Note,
            vec![Step::File(
                "project-management/tasks/TSK-999.md",
                "---\nid: TSK-001\nstatus: [unclosed\n---\n".to_string(),
            )],
            vec![Step::File(RECORD, record(UID))],
        );
        match judge_at(dir.path(), &[LINE, "main"]).criteria {
            Authority::Unknown(reason) => assert!(reason.contains("does not parse"), "{reason}"),
            _ => panic!("a doubt gave an answer"),
        }
    }

    const SECOND: &str = "project-management/tasks/TSK-002.md";
    const THIRD: &str = "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d";
    const SHARED: &str = "When run, the system shall work.";
    const LINE_V: &str = "When run, the system shall work fast.";

    /// Task `id`'s record, carrying `uid`, with its one criterion `criterion`.
    fn task_with(id: &str, uid: &str, criterion: &str) -> String {
        record(uid)
            .replace("TSK-001", id)
            .replace("When run, the system shall work.", criterion)
    }

    fn uid_of(id: &str) -> &'static str {
        if id == "TSK-001" {
            UID
        } else {
            OTHER
        }
    }

    /// Judge task `id` at the tips `points` in the run's shared `store`.
    fn judge_in(
        store: &mut RecordStore<'_>,
        repo: &Repository,
        id: &str,
        points: &[&str],
    ) -> TaskJudgement {
        let judged: Vec<Oid> = points
            .iter()
            .map(|name| {
                repo.revparse_single(name)
                    .unwrap()
                    .peel_to_commit()
                    .unwrap()
                    .id()
            })
            .collect();
        let identity = Identity::named(id, Some(uid_of(id).to_string()), &format!("{id}.md"));
        store.judge(
            &identity,
            &judged,
            &Outside {
                head: judged[0],
                tips: &[],
                at: None,
                own: &[],
            },
        )
    }

    /// TSK-001 and TSK-002 hold the same criteria at the fork, and the line
    /// amends both to the same new criteria. `main` runs `second` on
    /// TSK-002 only, and leaves TSK-001 as the line inherited it.
    fn two_tasks(second: &dyn Fn(&std::path::Path)) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
        std::fs::write(root.join(RECORD), task_with("TSK-001", UID, SHARED)).unwrap();
        std::fs::write(root.join(SECOND), task_with("TSK-002", OTHER, SHARED)).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "plan both"]);
        git(root, &["branch", LINE]);
        second(root);
        git(root, &["switch", "-q", LINE]);
        std::fs::write(root.join(RECORD), task_with("TSK-001", UID, LINE_V)).unwrap();
        std::fs::write(root.join(SECOND), task_with("TSK-002", OTHER, LINE_V)).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "amend both on the line"]);
        let repo = Repository::open(root).unwrap();
        let main = repo.revparse_single("main").unwrap().id();
        let line = repo.revparse_single(LINE).unwrap().id();
        assert!(!repo.graph_descendant_of(main, line).unwrap());
        assert!(!repo.graph_descendant_of(line, main).unwrap());
        dir
    }

    /// Main amends TSK-002, then restores its first criteria.
    fn main_restores_second(root: &std::path::Path) {
        save(
            root,
            SECOND,
            &task_with("TSK-002", OTHER, "When run, the system shall stop."),
        );
        save(root, SECOND, &task_with("TSK-002", OTHER, SHARED));
    }

    /// Main deletes TSK-002, then adds its first criteria again.
    fn main_reintroduces_second(root: &std::path::Path) {
        git(root, &["rm", "-q", SECOND]);
        git(root, &["commit", "-q", "-m", "delete TSK-002"]);
        save(root, SECOND, &task_with("TSK-002", OTHER, SHARED));
    }

    /// Main holds TSK-002's id with another uid for a while.
    fn main_doubts_second(root: &std::path::Path) {
        let path = "project-management/tasks/TSK-999.md";
        save(root, path, &task_with("TSK-002", THIRD, SHARED));
        git(root, &["rm", "-q", path]);
        git(root, &["commit", "-q", "-m", "delete the duplicate"]);
    }

    /// Delete both records on `main` and on the line, so neither judged
    /// point holds them and `recovered` reads their history.
    fn delete_both(root: &std::path::Path) {
        for branch in ["main", LINE] {
            git(root, &["switch", "-q", branch]);
            git(root, &["rm", "-q", RECORD, SECOND]);
            git(root, &["commit", "-q", "-m", "retire both"]);
        }
    }

    /// SEC-306-03: one store judges both tasks in both task orders and both
    /// point orders. TSK-001 inherited, so the line's criteria win; TSK-002's
    /// answer is refused with `reason`, whatever was judged before it.
    fn tasks_do_not_share_introductions(root: &std::path::Path, reason: &str) {
        let repo = Repository::open(root).unwrap();
        for ids in [["TSK-001", "TSK-002"], ["TSK-002", "TSK-001"]] {
            for points in [[LINE, "main"], ["main", LINE]] {
                let mut store = RecordStore::new(&repo);
                for id in ids {
                    match (id, judge_in(&mut store, &repo, id, &points).criteria) {
                        ("TSK-001", Authority::Held(held)) => {
                            assert!(held.content.contains("work fast"), "{ids:?} {points:?}");
                        }
                        ("TSK-002", Authority::Unknown(why)) => {
                            assert!(why.contains(reason), "{ids:?} {points:?}: {why}");
                        }
                        (id, _) => panic!("{ids:?} {points:?}: {id} took another task's answer"),
                    }
                }
            }
        }
    }

    #[test]
    fn one_tasks_proof_does_not_settle_another_tasks_restore() {
        let dir = two_tasks(&main_restores_second);
        tasks_do_not_share_introductions(dir.path(), "different versions");
    }

    #[test]
    fn one_tasks_proof_does_not_settle_another_tasks_reintroduction() {
        let dir = two_tasks(&main_reintroduces_second);
        tasks_do_not_share_introductions(dir.path(), "different versions");
    }

    #[test]
    fn one_tasks_refusal_does_not_refuse_another_task() {
        let dir = two_tasks(&main_doubts_second);
        tasks_do_not_share_introductions(dir.path(), "another uid");
    }

    #[test]
    fn recovered_tasks_do_not_share_introductions() {
        let dir = two_tasks(&main_restores_second);
        delete_both(dir.path());
        tasks_do_not_share_introductions(dir.path(), "different versions");
    }

    /// SEC-306-04: main merges the line's V, then restores the first
    /// criteria S with `change` (another epic or status), in the merge's
    /// resolution when `in_merge`; each line then deletes the record, so
    /// `recovered` decides. S was decided at the fork as well, so the line's
    /// V does not yield to it, and neither wins.
    fn recovered_restore(in_merge: bool, change: fn(&str) -> String) -> tempfile::TempDir {
        let first = record(UID).replace("status: todo", "epic_id: EPC-001\nstatus: todo");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
        save(root, RECORD, &first);
        git(root, &["branch", LINE]);
        git(root, &["switch", "-q", LINE]);
        save(
            root,
            RECORD,
            &first.replace("When run, the system shall work.", LINE_V),
        );
        git(root, &["switch", "-q", "main"]);
        if in_merge {
            git(root, &["merge", "--no-ff", "--no-commit", LINE]);
        } else {
            git(root, &["merge", "--no-ff", "-m", "take v2", LINE]);
        }
        save(root, RECORD, &change(&first));
        git(root, &["rm", "-q", RECORD]);
        git(root, &["commit", "-q", "-m", "retire on main"]);
        git(root, &["switch", "-q", LINE]);
        save(root, "later.txt", "the line advanced\n");
        git(root, &["rm", "-q", RECORD]);
        git(root, &["commit", "-q", "-m", "retire on the line"]);
        let repo = Repository::open(root).unwrap();
        let main = repo.revparse_single("main").unwrap().id();
        let line = repo.revparse_single(LINE).unwrap().id();
        assert!(!repo.graph_descendant_of(main, line).unwrap());
        assert!(!repo.graph_descendant_of(line, main).unwrap());
        dir
    }

    fn other_epic(text: &str) -> String {
        text.replace("epic_id: EPC-001", "epic_id: EPC-002")
    }

    fn other_status(text: &str) -> String {
        text.replace("status: todo", "status: blocked")
    }

    #[test]
    fn a_recovered_restore_with_another_epic_still_competes() {
        let dir = recovered_restore(false, other_epic);
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn a_recovered_restore_with_another_status_still_competes() {
        let dir = recovered_restore(false, other_status);
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn a_recovered_merge_resolution_with_another_epic_still_competes() {
        let dir = recovered_restore(true, other_epic);
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn a_recovered_merge_resolution_with_another_status_still_competes() {
        let dir = recovered_restore(true, other_status);
        refuses_both_orders(dir.path(), "different versions");
    }

    #[test]
    fn landing_refuses_grafts_and_replace_refs() {
        for replace in [false, true] {
            let dir = history(UID, UID);
            assert!(matches!(judge(dir.path(), UID).landing, Landing::Planned));
            let repo = Repository::open(dir.path()).unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            let base = head.parent_id(0).unwrap();
            if replace {
                repo.reference(&format!("refs/replace/{}", head.id()), base, false, "test")
                    .unwrap();
            } else {
                std::fs::create_dir_all(repo.path().join("info")).unwrap();
                std::fs::write(
                    repo.path().join("info/grafts"),
                    format!("{} {base}\n", head.id()),
                )
                .unwrap();
            }
            match judge(dir.path(), UID).landing {
                Landing::Unreadable(reason) => {
                    assert!(reason.contains("cannot verify"), "{reason}");
                }
                _ => panic!("overlaid history proved a landing answer"),
            }
        }
    }
    #[test]
    fn landed_evidence_refuses_grafts_and_replace_refs() {
        for replace in [false, true] {
            let dir = history(UID, UID);
            let repo = Repository::open(dir.path()).unwrap();
            let base = repo.head().unwrap().peel_to_commit().unwrap().id();
            let completed = record(UID).replace("status: todo", "status: complete");
            let head = crate::git::add_commit(
                &repo,
                &[(b"project-management/tasks/TSK-001.md", completed.as_bytes())],
            );
            assert!(matches!(
                judge(dir.path(), UID).landing,
                Landing::Landed { .. }
            ));
            if replace {
                repo.reference(&format!("refs/replace/{head}"), base, false, "test")
                    .unwrap();
            } else {
                std::fs::create_dir_all(repo.path().join("info")).unwrap();
                std::fs::write(repo.path().join("info/grafts"), format!("{head} {base}\n"))
                    .unwrap();
            }
            match judge(dir.path(), UID).landing {
                Landing::Unreadable(reason) => {
                    assert!(reason.contains("cannot verify"), "{reason}");
                }
                _ => panic!("overlaid history proved a landing answer"),
            }
        }
    }
}
