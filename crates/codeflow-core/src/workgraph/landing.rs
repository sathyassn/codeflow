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
//!   readable history with no overlay, records that parse and one identity.
//!   A branch or remote-tracking ref outside the range that adds a
//!   completing version only makes the answer unknown.
//! - **Criteria** are authority. They come from the newest version of the
//!   record at the judged points themselves, which a later reviewed
//!   planning amendment may have changed; when the points no longer hold
//!   the record, from the newest version in their history. A landing
//!   witness never supplies criteria.
//!
//! Readiness (whether a predecessor is complete now) is not decided here:
//! it reads the current status at the execution base (D3a).

use std::collections::{HashMap, HashSet};

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
}

impl TaskJudgement {
    /// A judgement that cannot be made: both answers unknown, for `reason`.
    pub(super) fn unreadable(reason: String) -> Self {
        Self {
            landing: Landing::Unreadable(reason.clone()),
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
    /// Each record that is the task, as its blob and parsed record.
    records: Vec<(Oid, RecordView)>,
    /// Why the tree's answer cannot be trusted, when it cannot: a record
    /// that may be the task does not parse, or the identity is ambiguous.
    doubt: Option<String>,
}

/// One run's reading of record history: trees, task entries and parsed
/// records are read once per object and shared by every task judged.
pub(super) struct RecordStore<'r> {
    repo: &'r Repository,
    /// Task entries (path, blob) per `project-management` tree.
    entries: HashMap<Oid, Vec<(String, Oid)>>,
    /// What each `project-management` tree holds of each task id.
    held: HashMap<(Oid, String), Held>,
    /// Why no history read here is complete, when it is not.
    overlay: Option<String>,
    /// The commits a shallow clone's history is cut at.
    boundary: HashSet<Oid>,
}

impl<'r> RecordStore<'r> {
    /// A store over `repo`. A shallow boundary that cannot be read, a graft
    /// file or a replace ref makes every proof of absence unknown.
    pub(super) fn new(repo: &'r Repository) -> Self {
        let (boundary, mut overlay) = match super::release_line::shallow_boundary(repo) {
            Ok(boundary) => (boundary, None),
            Err(reason) => (HashSet::new(), Some(reason)),
        };
        if repo.path().join("info/grafts").exists() {
            overlay.get_or_insert_with(|| {
                "this clone has a graft file (`.git/info/grafts`), so its history may not be the history the target holds".to_string()
            });
        }
        let replaced = repo
            .references_glob("refs/replace/*")
            .map(|mut refs| refs.next().is_some())
            .unwrap_or(true);
        if replaced {
            overlay.get_or_insert_with(|| {
                "this clone has replace refs (`refs/replace/`), so its history may not be the history the target holds".to_string()
            });
        }
        Self {
            repo,
            entries: HashMap::new(),
            held: HashMap::new(),
            overlay,
            boundary,
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
        let key = (records, task.id.clone());
        if let Some(held) = self.held.get(&key) {
            return Ok(held.clone());
        }
        if !self.entries.contains_key(&records) {
            let tree = self.repo.find_tree(records).map_err(|error| {
                format!("cannot read the records at {commit}: {}", error.message())
            })?;
            let found = super::acceptance::task_entries_in(self.repo, &tree);
            self.entries.insert(records, found);
        }
        let entries = self.entries.get(&records).cloned().unwrap_or_default();
        let mut held = Held::default();
        for (path, blob) in entries {
            let own = path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(&task.own_file));
            let Ok(object) = self.repo.find_blob(blob) else {
                if own {
                    held.doubt
                        .get_or_insert(format!("cannot read {path} at {commit}"));
                }
                continue;
            };
            let content = String::from_utf8_lossy(object.content());
            let literal = content.contains(task.id.as_str())
                || task.uid.as_deref().is_some_and(|uid| content.contains(uid));
            // A double-quoted YAML value can spell the id or uid with `\x`
            // escapes, so such a file is parsed too.
            if !(own || literal || content.contains('\\')) {
                continue;
            }
            match RecordView::parse(RecordKind::Task, &path, &content) {
                Ok(record)
                    if super::acceptance::is_same_task(&record, &task.id, task.uid.as_deref()) =>
                {
                    let uid = super::acceptance::record_uid(&record.content);
                    if record.id == task.id
                        && uid.is_some()
                        && task.uid.is_some()
                        && uid != task.uid
                    {
                        held.doubt.get_or_insert(format!(
                            "{path} at {commit} carries the id {} with another uid, so which task it is cannot be told",
                            task.id
                        ));
                    }
                    if !held.records.iter().any(|(known, _)| *known == blob) {
                        held.records.push((blob, record));
                    }
                }
                Err(_) if own || literal => {
                    held.doubt
                        .get_or_insert(format!("{path} at {commit} does not parse"));
                }
                Ok(_) | Err(_) => {}
            }
        }
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
    /// the newest version of the task's record, the first commit whose
    /// version shows a completion (the walk stops there), and why the walk
    /// proves no absence, when it does not.
    fn walk(&mut self, task: &Identity, push: &[Oid], hide: &[Oid]) -> Walk {
        let mut found = Walk::default();
        if push.is_empty() {
            found.doubt = Some("no judged point to read".to_string());
            return found;
        }
        found.doubt.clone_from(&self.overlay);
        let unreadable =
            |error: git2::Error| format!("cannot read the history: {}", error.message());
        let walk = match self
            .repo
            .revwalk()
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
            if found.newest.is_none() {
                found.newest = held.records.first().map(|(_, record)| record.clone());
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
        // The criteria at the judged points: the newest point's version.
        let mut points = Vec::new();
        for point in judged {
            match self.held_at(*point, task) {
                Ok(held) => points.push((*point, held)),
                Err(reason) => return TaskJudgement::unreadable(reason),
            }
        }
        let newest = points.iter().find(|(point, _)| {
            points.iter().all(|(other, _)| {
                other == point
                    || self
                        .repo
                        .graph_descendant_of(*point, *other)
                        .unwrap_or(false)
            })
        });
        let at_points = if let Some((_, held)) = newest {
            match (&held.doubt, held.records.first()) {
                (Some(doubt), _) => Err(doubt.clone()),
                (None, Some((_, record))) => Ok(Some(record.clone())),
                (None, None) => Ok(None),
            }
        } else {
            let mut versions = points.iter().flat_map(|(_, held)| held.records.iter());
            let first = versions.next().map(|(_, record)| record.clone());
            let agree = points.iter().all(|(_, held)| {
                held.doubt.is_none()
                    && held
                        .records
                        .first()
                        .map(|(_, record)| record.criteria.signature())
                        == first.as_ref().map(|record| record.criteria.signature())
            });
            if agree {
                Ok(first)
            } else {
                Err("the judged points hold different versions of this task, and none of them is newer than the others".to_string())
            }
        };
        let history = self.walk(task, judged, &[]);
        let criteria = match at_points {
            Ok(Some(record)) => Authority::Held(Box::new(record)),
            Err(reason) => Authority::Unknown(reason),
            Ok(None) => match (&history.newest, &history.doubt) {
                (Some(record), _) if history.witness.is_some() || history.doubt.is_none() => {
                    Authority::Held(Box::new(record.clone()))
                }
                (_, Some(doubt)) => Authority::Unknown(doubt.clone()),
                (Some(record), None) => Authority::Held(Box::new(record.clone())),
                (None, None) => Authority::Absent,
            },
        };
        if let Some(witness) = history.witness {
            return TaskJudgement {
                landing: Landing::Landed { witness },
                criteria,
            };
        }
        let landing = match history.doubt {
            Some(doubt) => Landing::Unreadable(doubt),
            None => self.outside(task, judged, outside),
        };
        TaskJudgement { landing, criteria }
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
            let reaches = outside.at.is_some_and(|at| {
                at == *tip || self.repo.graph_descendant_of(*tip, at).unwrap_or(false)
            });
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
                let reference = outside
                    .tips
                    .iter()
                    .find(|(_, tip)| {
                        *tip == commit
                            || self.repo.graph_descendant_of(*tip, commit).unwrap_or(false)
                    })
                    .map_or_else(|| commit.to_string(), |(name, _)| name.clone());
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
            let Ok(name) = reference.name() else { continue };
            let tracked = name.starts_with("refs/heads/") || name.starts_with("refs/remotes/");
            if !tracked || reference.kind() == Some(git2::ReferenceType::Symbolic) {
                continue;
            }
            if let Ok(commit) = reference.peel_to_commit() {
                let short = reference.shorthand().unwrap_or(name).to_string();
                refs.push((name.to_string(), short, commit.id()));
            }
        }
        refs.sort();
        let mut walk = self.repo.revwalk().map_err(unreadable)?;
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
            let parents: Vec<Oid> = self
                .repo
                .find_commit(commit)
                .map_err(unreadable)?
                .parent_ids()
                .collect();
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
            let holder = refs
                .iter()
                .find(|(_, _, tip)| {
                    *tip == commit || self.repo.graph_descendant_of(*tip, commit).unwrap_or(false)
                })
                .map_or_else(|| commit.to_string(), |(_, short, _)| short.clone());
            return Ok(Some((holder, commit)));
        }
        Ok(None)
    }
}

/// What a walk of one task's history found ([`RecordStore::walk`]).
#[derive(Default)]
struct Walk {
    /// The newest version of the record the walk reached.
    newest: Option<RecordView>,
    /// The first commit, newest first, whose version shows a completion.
    witness: Option<Oid>,
    /// Why the walk proves no absence, when it does not.
    doubt: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
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
        git(clone.path(), &["clone", "-q", "--depth", "2", &url, "copy"]);
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
}
