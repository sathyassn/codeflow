//! The one read of everything `codeflow status` reports: records on each
//! declared target's tip, the readiness core's states, task branches and
//! worktrees, the fetched refs, the forge through the bounded observer, the
//! coordination ledger, an adopted forecast, a session file and the
//! owed-item sources. Nothing here writes or fetches.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use git2::{Oid, Repository};

use super::inputs::{
    AcceptanceIn, BlockerIn, BranchIn, CheckRunIn, EpicIn, ForecastIn, ForgeIn, Inputs, LaneIn,
    LedgerIn, OperatorItemIn, PullIn, RefIn, ScopeSpec, TaskIn, UnreadableIn,
};
use super::owed::OwedSource;
use crate::workgraph::history;
use crate::workgraph::readiness;
use crate::workgraph::record_text;
use crate::workgraph::work_start::{self, Record, RecordKind};

/// The default sweep interval when the project records none: 30 minutes.
const DEFAULT_SWEEP_MINUTES: i64 = 30;
/// The most first-parent commits the history walk reads per tip.
const HISTORY_LIMIT: usize = 5_000;

/// How `status` reads: the scope, a fixed snapshot time for tests and
/// reproducible output, the session file and the owed-item sources.
pub struct Options {
    pub scope: ScopeSpec,
    /// The snapshot time in seconds since the epoch; the clock when `None`.
    pub now: Option<i64>,
    pub session: Option<PathBuf>,
    pub owed: Vec<Box<dyn OwedSource>>,
    /// The forge client program (`gh`).
    pub gh_program: OsString,
    /// How long the forge read may take.
    pub forge_limit: Duration,
}

impl Options {
    /// Options for `scope` with the clock, no session file, no owed-item
    /// source, `gh` and the observer's network bound.
    #[must_use]
    pub fn new(scope: ScopeSpec) -> Self {
        Self {
            scope,
            now: None,
            session: None,
            owed: Vec::new(),
            gh_program: OsString::from("gh"),
            forge_limit: crate::coord::observe::NETWORK_TIMEOUT,
        }
    }
}

/// A target name as a bare branch (`origin/main` and `refs/heads/main` are
/// `main`).
fn bare(target: &str) -> String {
    target
        .strip_prefix("refs/remotes/origin/")
        .or_else(|| target.strip_prefix("refs/heads/"))
        .or_else(|| target.strip_prefix("origin/"))
        .unwrap_or(target)
        .to_string()
}

fn seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn parse_time(text: &str) -> Option<i64> {
    crate::coord::parse_time(text)
        .ok()
        .and_then(|secs| i64::try_from(secs).ok())
}

/// Read everything the report needs, once.
///
/// # Errors
///
/// The reason when the repository cannot be opened or the session file is
/// refused; every other unreadable source is reported inside the inputs.
pub fn collect(start: &Path, options: &Options) -> Result<Inputs, String> {
    let repo = Repository::discover(start).map_err(|error| error.message().to_string())?;
    // One overlay and shallow-boundary snapshot for every history read.
    let _judgment = history::judgment();
    let root = repo
        .workdir()
        .ok_or_else(|| "the repository has no worktree".to_string())?
        .to_path_buf();
    let session = match &options.session {
        Some(path) => Some(super::session::read(path)?),
        None => None,
    };
    let now = options.now.unwrap_or_else(seconds_now);
    let default_branch = work_start::default_work_target(&root)
        .ok()
        .flatten()
        .map_or_else(|| "main".to_string(), |target| bare(&target));
    let read = read_records(&root, &repo);
    let (backlog, readiness_problem) = match readiness::backlog(&root) {
        Ok(backlog) => (Some(backlog), None),
        Err(error) => (None, Some(error)),
    };
    let ids: BTreeSet<String> = read.tasks.keys().cloned().collect();
    // The forge is read before the tasks: an open task's acceptance binds
    // to the head its pull request reports.
    let forge = forge(&root, options, now, &ids);
    let history = landings(&repo, &read.tips, now);
    let context = TaskContext {
        repo: &repo,
        backlog: backlog.as_ref(),
        landed: readiness::target_tasks(&root),
        worktrees: worktree_branches(&repo),
        tips: &read.tips,
        history: history.as_ref().map(|(landed, _)| landed),
        pulls: match &forge {
            ForgeIn::Read { pulls, .. } => Some(pulls.as_slice()),
            ForgeIn::Unverified { .. } => None,
        },
        default_tip: remote_tip(&repo, &default_branch),
        home_unread: &read.home_unread,
    };
    let mut tasks: Vec<TaskIn> = read
        .tasks
        .values()
        .map(|found| task_in(&context, found))
        .collect();
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    let epics = read
        .epics
        .values()
        .map(|(record, text)| epic_in(&root, record, text))
        .collect();
    let line_refs = read
        .tips
        .keys()
        .filter(|target| **target != default_branch)
        .map(|target| (target.clone(), remote_ref(&repo, target)))
        .collect();
    let current_target = current_target(&repo, &options.scope, &tasks, &ids, &default_branch);
    let mut inputs = Inputs {
        now,
        repository: crate::coord::reference::repository_identity(&root),
        repo_path: root.display().to_string(),
        scope: options.scope.clone(),
        current_target,
        default_ref: Some(remote_ref(&repo, &default_branch)),
        default_branch,
        line_refs,
        fetched_at: readiness::fetched_at(&repo).as_deref().and_then(parse_time),
        sweep_seconds: sweep_seconds(&root),
        tasks,
        epics,
        unreadable: read.unreadable,
        readiness_problem,
        forge,
        ledger: ledger(&root),
        session,
        owed: options
            .owed
            .iter()
            .map(|source| (source.label().to_string(), source.read()))
            .collect(),
        forecast: None,
        forecast_note: None,
        release_step: root.join("docs/releasing.md").is_file(),
        history_read: history.as_ref().is_some_and(|(_, reached)| *reached),
        record_losses: read.losses,
    };
    let (forecast, note) = forecast(&root, &inputs);
    inputs.forecast = forecast;
    inputs.forecast_note = note;
    Ok(inputs)
}

/// One task record as found on its home tip.
struct Found {
    record: Record,
    text: String,
    path: String,
    target: String,
}

#[derive(Default)]
struct Read {
    tasks: BTreeMap<String, Found>,
    epics: BTreeMap<String, (Record, String)>,
    unreadable: Vec<UnreadableIn>,
    /// Each declared target's bare name to the tip read.
    tips: BTreeMap<String, Oid>,
    /// Record coverage lost: the target (none when the list failed), why.
    losses: Vec<(Option<String>, String)>,
    /// Tasks whose home target could not be read, to why.
    home_unread: BTreeMap<String, String>,
}

fn task_id_of(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    stem.starts_with("TSK-").then(|| stem.to_string())
}

/// Every task on the tip of its own declared target; epics from any tip,
/// the default target's first. A target that cannot be listed, resolved or
/// read is recorded as lost coverage, and a task whose home target was lost
/// is kept from the first other tip that holds it, with its stage unknown.
#[allow(clippy::too_many_lines)]
fn read_records(root: &Path, repo: &Repository) -> Read {
    let mut out = Read::default();
    let default = work_start::default_work_target(root).ok().flatten();
    let mut targets: Vec<String> = match readiness::declared_targets(root) {
        Ok(set) => set.into_iter().collect(),
        Err(error) => {
            out.losses.push((
                None,
                format!("the declared targets could not be listed: {error}"),
            ));
            Vec::new()
        }
    };
    if let Some(default) = &default {
        targets.retain(|target| target != default);
        targets.insert(0, default.clone());
    }
    let mut lost: BTreeMap<String, String> = BTreeMap::new();
    let mut elsewhere: BTreeMap<String, Found> = BTreeMap::new();
    let mut seen_unreadable = BTreeSet::new();
    for declared in &targets {
        let name = bare(declared);
        let tip = match readiness::resolve_target(root, repo, declared) {
            Ok(Some((_, tip))) => tip,
            Ok(None) => {
                lost.insert(name, format!("the target {declared} is not here"));
                continue;
            }
            Err(error) => {
                lost.insert(
                    name,
                    format!("the target {declared} cannot be resolved: {error}"),
                );
                continue;
            }
        };
        out.tips.insert(name.clone(), tip);
        let records = repo
            .find_commit(tip)
            .and_then(|commit| commit.tree())
            .map_err(|error| error.message().to_string())
            .and_then(|tree| {
                work_start::records_from_tree_lenient(repo, &tree)
                    .map_err(|error| error.to_string())
            });
        let records = match records {
            Ok(records) => records,
            Err(error) => {
                lost.insert(
                    name,
                    format!("the records on {declared} cannot be read: {error}"),
                );
                continue;
            }
        };
        for entry in records {
            match entry.read {
                Err(reason) => {
                    if seen_unreadable.insert(entry.path.clone()) {
                        out.unreadable.push(UnreadableIn {
                            task_id: task_id_of(&entry.path),
                            path: entry.path,
                            reason,
                        });
                    }
                }
                Ok((record, text)) => match record.kind {
                    RecordKind::Epic => {
                        out.epics.entry(record.id.clone()).or_insert((record, text));
                    }
                    RecordKind::Task => {
                        let home = record
                            .integration_target
                            .clone()
                            .or_else(|| default.clone())
                            .map_or_else(|| name.clone(), |target| bare(&target));
                        let found = Found {
                            path: entry.path,
                            target: home.clone(),
                            record,
                            text,
                        };
                        if home == name {
                            out.tasks.entry(found.record.id.clone()).or_insert(found);
                        } else {
                            elsewhere.entry(found.record.id.clone()).or_insert(found);
                        }
                    }
                    RecordKind::Spec => {}
                },
            }
        }
    }
    for (id, found) in elsewhere {
        if out.tasks.contains_key(&id) {
            continue;
        }
        if let Some(reason) = lost.get(&found.target) {
            out.home_unread.insert(id.clone(), reason.clone());
            out.tasks.insert(id, found);
        }
    }
    out.losses.extend(
        lost.into_iter()
            .map(|(target, reason)| (Some(target), reason)),
    );
    out.unreadable.retain(|record| {
        record
            .task_id
            .as_ref()
            .is_none_or(|id| !out.tasks.contains_key(id))
    });
    out
}

fn remote_ref(repo: &Repository, branch: &str) -> RefIn {
    let name = format!("origin/{branch}");
    let sha = repo
        .find_reference(&format!("refs/remotes/{name}"))
        .and_then(|reference| reference.peel_to_commit())
        .ok()
        .map(|commit| commit.id().to_string());
    RefIn { name, sha }
}

/// Each linked worktree's checked-out branch to its path.
fn worktree_branches(repo: &Repository) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(names) = repo.worktrees() else {
        return out;
    };
    for name in names.iter().flatten().flatten() {
        let Ok(worktree) = repo.find_worktree(name) else {
            continue;
        };
        let Ok(opened) = Repository::open_from_worktree(&worktree) else {
            continue;
        };
        if let Some(branch) = opened.head().ok().map(|head| {
            crate::git::name::reference_shorthand(&head)
                .display()
                .to_string()
        }) {
            out.insert(branch, worktree.path().display().to_string());
        }
    }
    out
}

/// A frontmatter string field of a record's text, read with the record
/// parser (YAML, so a trailing comment is not part of the value).
fn frontmatter(text: &str, key: &str) -> Option<String> {
    let (data, _) = crate::validate::parse_frontmatter(text.as_bytes()).ok()?;
    let value = crate::validate::get_string_field(&data, key);
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn section(text: &str, heading: &str) -> Option<String> {
    record_text::section_markdown(text, heading)
        .map(|body| {
            body.lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .filter(|body| !body.is_empty())
}

/// Outcome and Context, or Description where there is no Context.
fn about(text: &str) -> Option<String> {
    let parts: Vec<String> = [
        section(text, "## Outcome"),
        section(text, "## Context").or_else(|| section(text, "## Description")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn blocker(text: &str) -> Option<BlockerIn> {
    let found = record_text::parse_blocker(text)?;
    let word = |value: String| (!value.trim().is_empty()).then(|| value.trim().to_string());
    Some(BlockerIn {
        reason: word(found.reason),
        owner: word(found.owner),
        revisit: word(found.revisit),
    })
}

/// How a block's binding is judged.
enum Binding {
    /// A closed task: its completion bound on the home target as the
    /// landing gate binds it (`readiness::target_tasks`), or why not.
    Landed(Result<bool, String>),
    /// An open task: bound to this head, the one its pull request or
    /// branch reports.
    Head(Oid),
    /// An open task whose reported head is not here.
    Missing(String),
}

/// The record's acceptance block: one active block that passes the
/// structural acceptance rules against the record's current criteria
/// (`record_text::check_acceptance`, the rule CI and `work_start` use),
/// counted against those criteria only, and its binding.
fn acceptance(repo: &Repository, text: &str, path: &str, binding: Binding) -> Option<AcceptanceIn> {
    let blocks = record_text::acceptance_blocks(text);
    let active: Vec<_> = blocks
        .iter()
        .filter(|block| !block.is_superseded())
        .collect();
    if active.is_empty() {
        return None;
    }
    let criteria = record_text::parse_criteria(text).items;
    let total = criteria.len();
    let parsed = match active.as_slice() {
        [only] => only
            .parsed
            .as_ref()
            .map_err(|_| "the active acceptance block cannot be parsed"),
        _ => Err("the record has more than one active acceptance block"),
    };
    let block = match parsed {
        Ok(block) => block,
        Err(reason) => {
            return Some(AcceptanceIn {
                verified: 0,
                total,
                reviewed: "none".into(),
                approved: false,
                valid: false,
                bound: false,
                note: Some(reason.into()),
            })
        }
    };
    let verified = criteria
        .iter()
        .filter(|criterion| {
            block
                .criteria
                .iter()
                .any(|(id, result)| *id == criterion.id && result.outcome == "verified")
        })
        .count();
    let problems = record_text::check_acceptance(block, &criteria);
    let (bound, note) = if problems.is_empty() {
        match binding {
            Binding::Landed(Ok(true)) => (true, None),
            Binding::Landed(Ok(false)) => (
                false,
                Some(
                    "the completion is not bound on its home target as the landing gate binds it"
                        .into(),
                ),
            ),
            Binding::Landed(Err(reason)) => (
                false,
                Some(format!("the binding could not be read: {reason}")),
            ),
            Binding::Head(head) => match head_binding(repo, &block.reviewed, head, path, &criteria)
            {
                Ok(()) => (true, None),
                Err(reason) => (false, Some(reason)),
            },
            Binding::Missing(reason) => (false, Some(reason)),
        }
    } else {
        (false, Some(problems.join("; ")))
    };
    Some(AcceptanceIn {
        verified,
        total,
        reviewed: block.reviewed.clone(),
        approved: block.verdict == "approved",
        valid: problems.is_empty(),
        bound,
        note,
    })
}

/// An open task's block binds to `head` when nothing but the record changed
/// since the reviewed commit and the record's criteria are the ones the
/// reviewed commit held.
fn head_binding(
    repo: &Repository,
    reviewed: &str,
    head: Oid,
    path: &str,
    criteria: &[record_text::Criterion],
) -> Result<(), String> {
    let review = Oid::from_str(reviewed)
        .and_then(|oid| repo.find_commit(oid))
        .map_err(|_| format!("the reviewed commit {reviewed} is not here"))?;
    let (old, new) = (
        review.tree().map_err(|error| error.message().to_string())?,
        repo.find_commit(head)
            .and_then(|commit| commit.tree())
            .map_err(|error| error.message().to_string())?,
    );
    let diff = repo
        .diff_tree_to_tree(Some(&old), Some(&new), None)
        .map_err(|error| error.message().to_string())?;
    let only_record = diff.deltas().all(|delta| {
        delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .is_some_and(|changed| changed == Path::new(path))
    });
    if !only_record {
        return Err(format!(
            "the head {head} changed more than the record since the reviewed commit"
        ));
    }
    let seen = text_at(repo, review.id(), path)
        .map(|text| record_text::parse_criteria(&text).items)
        .unwrap_or_default();
    let pairs = |items: &[record_text::Criterion]| -> Vec<(String, String)> {
        items
            .iter()
            .map(|c| (c.id.clone(), c.text.clone()))
            .collect()
    };
    if pairs(&seen) != pairs(criteria) {
        return Err("the criteria changed since the reviewed commit".into());
    }
    Ok(())
}

fn find_branch<'r>(repo: &'r Repository, name: &str) -> Option<git2::Commit<'r>> {
    [format!("refs/heads/{name}"), format!("refs/remotes/{name}")]
        .iter()
        .find_map(|reference| repo.find_reference(reference).ok())
        .and_then(|reference| reference.peel_to_commit().ok())
}

fn branch_in(
    repo: &Repository,
    name: &str,
    tip: Option<Oid>,
    worktrees: &BTreeMap<String, String>,
) -> Option<(BranchIn, Oid)> {
    let commit = find_branch(repo, name)?;
    // The guarded reader refuses grafts, replace refs and a shallow cut.
    let mut walk = history::revwalk(repo).ok()?;
    walk.push(commit.id()).ok()?;
    if let Some(tip) = tip {
        walk.hide(tip).ok()?;
    }
    let commits_ahead = walk
        .take(HISTORY_LIMIT)
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .len();
    let local = name.split_once('/').map_or(name, |(remote, rest)| {
        if repo.find_remote(remote).is_ok() {
            rest
        } else {
            name
        }
    });
    Some((
        BranchIn {
            name: name.to_string(),
            head: commit.id().to_string(),
            commits_ahead,
            head_time: commit.time().seconds(),
            worktree: worktrees
                .get(local)
                .or_else(|| worktrees.get(name))
                .cloned(),
        },
        commit.id(),
    ))
}

/// The record's text at `path` on `commit`.
fn text_at(repo: &Repository, commit: Oid, path: &str) -> Option<String> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let entry = tree.get_path(Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    String::from_utf8(blob.content().to_vec()).ok()
}

/// Each home target's tasks to their newest landing: time and commit.
type Landings = BTreeMap<String, BTreeMap<String, (i64, Oid)>>;

/// What every task's facts are read against, once.
struct TaskContext<'a> {
    repo: &'a Repository,
    backlog: Option<&'a readiness::Backlog>,
    /// The landing gate's view of each home target's records.
    landed: Result<BTreeMap<String, readiness::TargetTask>, String>,
    worktrees: BTreeMap<String, String>,
    tips: &'a BTreeMap<String, Oid>,
    /// Each home target's landing transitions, when the walk succeeded.
    history: Option<&'a Landings>,
    /// The open pull requests, when the forge was read.
    pulls: Option<&'a [PullIn]>,
    /// The trusted remote's default branch tip.
    default_tip: Option<Oid>,
    home_unread: &'a BTreeMap<String, String>,
}

/// The statuses the stage ladder knows.
const KNOWN_STATUSES: [&str; 4] = ["todo", "blocked", "complete", "cancelled"];

/// The remote-tracking tip of `branch` on `origin`.
fn remote_tip(repo: &Repository, branch: &str) -> Option<Oid> {
    repo.find_reference(&format!("refs/remotes/origin/{branch}"))
        .and_then(|reference| reference.peel_to_commit())
        .ok()
        .map(|commit| commit.id())
}

/// Whether `commit` is `tip` or an ancestor of it.
/// `None` when the guarded reader cannot prove the ancestry either way.
fn reaches(repo: &Repository, tip: Oid, commit: Oid) -> Option<bool> {
    if tip == commit {
        return Some(true);
    }
    history::descendant_of(repo, tip, commit).ok()
}

#[allow(clippy::too_many_lines)]
fn task_in(context: &TaskContext<'_>, found: &Found) -> TaskIn {
    let repo = context.repo;
    let record = &found.record;
    let entry = context
        .backlog
        .and_then(|b| b.entries.iter().find(|entry| entry.task_id == record.id));
    let tip = context.tips.get(&found.target).copied();
    let branch = entry
        .and_then(|entry| entry.branch_names.first())
        .and_then(|name| branch_in(repo, name, tip, &context.worktrees));
    let closed = matches!(record.status.as_str(), "complete" | "cancelled");
    let pull = context.pulls.and_then(|pulls| {
        pulls
            .iter()
            .find(|pull| pull.task.as_deref() == Some(record.id.as_str()))
    });
    // The most advanced copy: the home tip once closed, else the head the
    // pull request reports, else the branch head.
    let (acceptance, follow_from) = if closed {
        let binding = Binding::Landed(match &context.landed {
            Ok(tasks) => tasks.get(&record.id).map_or_else(
                || Err("the task is not on its home target's remote ref".into()),
                |task| Ok(task.accepted),
            ),
            Err(reason) => Err(reason.clone()),
        });
        (
            acceptance(repo, &found.text, &found.path, binding),
            found.text.clone(),
        )
    } else {
        let head = match (pull, &branch) {
            (Some(pull), _) => match Oid::from_str(&pull.head_sha)
                .ok()
                .filter(|oid| repo.find_commit(*oid).is_ok())
            {
                Some(oid) => Some(Ok(oid)),
                None => Some(Err(format!(
                    "the head {} pull request #{} reports is not here; fetch it",
                    pull.head_sha, pull.number
                ))),
            },
            (None, Some((_, head))) => Some(Ok(*head)),
            (None, None) => None,
        };
        match head {
            Some(Ok(head)) => match text_at(repo, head, &found.path) {
                Some(text) => (
                    acceptance(repo, &text, &found.path, Binding::Head(head)),
                    text,
                ),
                None => (
                    acceptance(
                        repo,
                        &found.text,
                        &found.path,
                        Binding::Missing(format!("the record is not readable at the head {head}")),
                    ),
                    found.text.clone(),
                ),
            },
            Some(Err(reason)) => (
                acceptance(repo, &found.text, &found.path, Binding::Missing(reason)),
                found.text.clone(),
            ),
            None => (None, found.text.clone()),
        }
    };
    let follow_ups = section(&follow_from, "## Follow-ups").or_else(|| {
        record_text::acceptance_blocks(&follow_from)
            .into_iter()
            .filter_map(|fenced| fenced.parsed.ok())
            .rfind(|block| !block.superseded)
            .map(|block| block.follow_ups)
            .filter(|text| !text.trim().is_empty() && text.trim() != "none")
    });
    let transition = context
        .history
        .and_then(|history| history.get(&found.target))
        .and_then(|landed| landed.get(&record.id))
        .copied();
    // Landed on a ref: the completion's transition commit is reachable
    // from it. Not complete is never landed; unknown when the ref or the
    // transition cannot be read.
    let landed_on = |tip: Option<Oid>| -> Option<bool> {
        if record.status != "complete" {
            return tip.map(|_| false);
        }
        let (tip, (_, commit)) = (tip?, transition?);
        reaches(repo, tip, commit)
    };
    let unknown = if let Some(reason) = context.home_unread.get(&record.id) {
        Some(reason.clone())
    } else if !KNOWN_STATUSES.contains(&record.status.as_str()) {
        Some(format!(
            "the status `{}` is not one the stage ladder knows",
            record.status
        ))
    } else if closed {
        None
    } else {
        match (context.backlog, entry) {
            (None, _) => Some("the readiness core could not judge the targets".into()),
            (Some(_), None) => Some("the readiness core holds no state for the task".into()),
            (Some(_), Some(_)) => None,
        }
    };
    TaskIn {
        id: record.id.clone(),
        title: record.title.clone(),
        epic: record.epic_id.clone(),
        status: record.status.clone(),
        target: found.target.clone(),
        depends_on: record.depends_on.iter().map(|dep| dep.id.clone()).collect(),
        created: record.created.clone(),
        state: if closed {
            None
        } else {
            entry.map(|entry| entry.state.as_str().to_string())
        },
        state_reason: entry.map(|entry| entry.reason.clone()).unwrap_or_default(),
        blocker: blocker(&found.text),
        theme: frontmatter(&found.text, "theme"),
        outcome: section(&found.text, "## Outcome"),
        about: about(&found.text),
        follow_ups,
        acceptance,
        branch: branch.map(|(branch, _)| branch),
        on_main: landed_on(context.default_tip),
        on_home_ref: landed_on(remote_tip(repo, &found.target)),
        unknown,
        record_on_line: !context.home_unread.contains_key(&record.id),
        landed_at: transition.map(|(time, _)| time),
    }
}

/// Each home target's newest `complete` transition per task on the
/// first-parent history of its tip, with the commit that made it, and
/// whether every walk reached back past the seven-day pace window (or the
/// root). `None` when a walk fails.
fn landings(repo: &Repository, tips: &BTreeMap<String, Oid>, now: i64) -> Option<(Landings, bool)> {
    let mut out = BTreeMap::new();
    let mut reached = true;
    let floor = now - super::compute::PACE_WINDOW_SECONDS;
    let mut spec = git2::DiffOptions::new();
    spec.pathspec("project-management/tasks");
    let status = |blob: Oid| {
        repo.find_blob(blob)
            .ok()
            .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
            .and_then(|text| frontmatter(&text, "status"))
    };
    for (target, tip) in tips {
        let landed: &mut BTreeMap<String, (i64, Oid)> = out.entry(target.clone()).or_default();
        let mut commit = repo.find_commit(*tip).ok()?;
        let mut walked_past = false;
        for _ in 0..HISTORY_LIMIT {
            if commit.time().seconds() < floor {
                walked_past = true;
            }
            // The guarded reader: a commit with no parent is the root; an
            // unreadable or unverified parent fails the walk.
            let parent = match history::parents(repo, &commit).ok()?.first() {
                Some(oid) => Some(repo.find_commit(*oid).ok()?),
                None => None,
            };
            let old = match &parent {
                Some(parent) => Some(parent.tree().ok()?),
                None => None,
            };
            let diff = repo
                .diff_tree_to_tree(old.as_ref(), Some(&commit.tree().ok()?), Some(&mut spec))
                .ok()?;
            for delta in diff.deltas() {
                let Some(id) = delta
                    .new_file()
                    .path()
                    .and_then(|p| p.to_str())
                    .and_then(task_id_of)
                else {
                    continue;
                };
                let now_complete = status(delta.new_file().id()).as_deref() == Some("complete");
                let was_complete = !delta.old_file().id().is_zero()
                    && status(delta.old_file().id()).as_deref() == Some("complete");
                if now_complete && !was_complete {
                    landed
                        .entry(id)
                        .or_insert((commit.time().seconds(), commit.id()));
                }
            }
            let Some(parent) = parent else {
                walked_past = true;
                break;
            };
            commit = parent;
        }
        reached &= walked_past;
    }
    Some((out, reached))
}

/// Epic text helpers: the first `##` heading whose section names each task.
fn epic_sections(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut heading: Option<String> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            heading = Some(name.trim().to_string());
            continue;
        }
        let Some(current) = &heading else { continue };
        let mut rest = line;
        while let Some(at) = rest.find("TSK-") {
            let digits: String = rest[at + 4..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !digits.is_empty() {
                out.entry(format!("TSK-{digits}"))
                    .or_insert_with(|| current.clone());
            }
            rest = &rest[at + 4..];
        }
    }
    out
}

/// The capability entries that list `epic`, with the prose after each
/// entry's block.
fn capabilities(root: &Path, epic: &str) -> Vec<(String, String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("docs/capabilities.md")) else {
        return Vec::new();
    };
    let (entries, _) = crate::capability::parse_capabilities(&text);
    let lines: Vec<&str> = text.lines().collect();
    entries
        .iter()
        .filter(|entry| {
            entry
                .epics
                .iter()
                .chain(&entry.epic_id)
                .any(|id| id == epic)
        })
        .map(|entry| {
            let start = lines
                .iter()
                .position(|line| line.trim() == format!("id: {}", entry.id))
                .and_then(|at| {
                    lines[at..]
                        .iter()
                        .position(|line| line.trim_start().starts_with("```"))
                        .map(|end| at + end + 1)
                });
            let prose = start.map_or_else(String::new, |start| {
                lines[start..]
                    .iter()
                    .take_while(|line| !line.starts_with('#'))
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string()
            });
            (entry.id.clone(), entry.name.clone(), prose)
        })
        .collect()
}

fn epic_in(root: &Path, record: &Record, text: &str) -> EpicIn {
    EpicIn {
        id: record.id.clone(),
        title: record.title.clone(),
        status: record.status.clone(),
        outcome: section(text, "## Outcome"),
        about: about(text),
        sections: epic_sections(text),
        section_text: text
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .filter_map(|heading| {
                section(text, &format!("## {}", heading.trim()))
                    .map(|body| (heading.trim().to_string(), body))
            })
            .collect(),
        capabilities: capabilities(root, &record.id),
    }
}

/// The line or branch the scope is read against.
fn current_target(
    repo: &Repository,
    scope: &ScopeSpec,
    tasks: &[TaskIn],
    ids: &BTreeSet<String>,
    default: &str,
) -> String {
    if let ScopeSpec::Line(line) = scope {
        return line.clone();
    }
    if let ScopeSpec::Task(id) = scope {
        if let Some(task) = tasks.iter().find(|task| &task.id == id) {
            return task.target.clone();
        }
    }
    let head = repo
        .head()
        .ok()
        .map(|head| {
            crate::git::name::reference_shorthand(&head)
                .display()
                .to_string()
        })
        .unwrap_or_default();
    if readiness::is_landing_target(&head) {
        return head;
    }
    let suffix = head.rsplit('/').next().unwrap_or("");
    ids.iter()
        .filter(|id| suffix == id.as_str() || suffix.starts_with(&format!("{id}-")))
        .max_by_key(|id| id.len())
        .and_then(|id| tasks.iter().find(|task| &task.id == id))
        .map_or_else(|| default.to_string(), |task| task.target.clone())
}

fn sweep_seconds(root: &Path) -> i64 {
    std::fs::read_to_string(root.join(".codeflow/project.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("sweep")?.get("interval_minutes")?.as_integer())
        .filter(|minutes| *minutes > 0)
        .unwrap_or(DEFAULT_SWEEP_MINUTES)
        * 60
}

/// A check run's state as read, lower case: the conclusion once it
/// completed, else its status or state.
fn run_state(check: &serde_json::Value) -> String {
    check
        .get("conclusion")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .or_else(|| check.get("state").and_then(serde_json::Value::as_str))
        .or_else(|| check.get("status").and_then(serde_json::Value::as_str))
        .unwrap_or("pending")
        .to_ascii_lowercase()
}

/// The check runs as read, and their summary: `failed` when any failed,
/// was cancelled or timed out, else `pending` when any has not finished,
/// `missing` when none ran, `skipped` when any skipped (a skip is not a
/// pass without reuse evidence), else `passed`.
fn checks(rollup: Option<&serde_json::Value>) -> (String, Vec<CheckRunIn>) {
    let text = |check: &serde_json::Value, key: &str| {
        check
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let runs: Vec<CheckRunIn> = rollup
        .and_then(serde_json::Value::as_array)
        .map(|checks| {
            checks
                .iter()
                .map(|check| CheckRunIn {
                    name: text(check, "name")
                        .or_else(|| text(check, "context"))
                        .unwrap_or_else(|| "unnamed".into()),
                    state: run_state(check),
                    completed_at: text(check, "completedAt"),
                })
                .collect()
        })
        .unwrap_or_default();
    let any = |states: &[&str]| runs.iter().any(|run| states.contains(&run.state.as_str()));
    let summary = if runs.is_empty() {
        "missing"
    } else if runs.iter().any(|run| {
        !matches!(
            run.state.as_str(),
            "success"
                | "neutral"
                | "skipped"
                | "pending"
                | "queued"
                | "in_progress"
                | "expected"
                | "waiting"
                | "requested"
        )
    }) {
        "failed"
    } else if any(&[
        "pending",
        "queued",
        "in_progress",
        "expected",
        "waiting",
        "requested",
    ]) {
        "pending"
    } else if any(&["skipped"]) {
        "skipped"
    } else {
        "passed"
    };
    (summary.into(), runs)
}

/// The most open pull requests one forge read lists; a full page may hide
/// more, so it is reported unverified rather than as the whole set.
const FORGE_LIMIT: usize = 200;

/// The open pull requests, read once through the bounded observer.
fn forge(root: &Path, options: &Options, now: i64, ids: &BTreeSet<String>) -> ForgeIn {
    let identity = crate::coord::reference::repository_identity(root);
    let Some(repository) = identity.strip_prefix("github.com/") else {
        return ForgeIn::Unverified {
            reason: format!("the forge for `{identity}` is not one status reads"),
        };
    };
    let mut command = crate::git::process(&options.gh_program);
    command
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1")
        .args([
            "pr",
            "list",
            "--repo",
            repository,
            "--state",
            "open",
            "--limit",
            &FORGE_LIMIT.to_string(),
            "--json",
            "number,title,headRefName,headRefOid,baseRefName,isDraft,statusCheckRollup",
        ]);
    let Some(output) = crate::coord::observe::command(command, options.forge_limit) else {
        return ForgeIn::Unverified {
            reason: "the forge read did not finish within its bound or could not start".into(),
        };
    };
    if !output.status.success() {
        return ForgeIn::Unverified {
            reason: "the forge read failed (offline or not signed in)".into(),
        };
    }
    let Ok(serde_json::Value::Array(rows)) = serde_json::from_slice(&output.stdout) else {
        return ForgeIn::Unverified {
            reason: "the forge answer was not the expected JSON".into(),
        };
    };
    if rows.len() >= FORGE_LIMIT {
        return ForgeIn::Unverified {
            reason: format!(
                "the forge listed {} open pull requests, its read limit, so more may exist",
                rows.len()
            ),
        };
    }
    let text = |row: &serde_json::Value, key: &str| {
        row.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut pulls: Vec<PullIn> = rows
        .iter()
        .filter_map(|row| {
            let head_ref = text(row, "headRefName");
            let suffix = head_ref.rsplit('/').next().unwrap_or("").to_string();
            let (checks, runs) = checks(row.get("statusCheckRollup"));
            Some(PullIn {
                number: row.get("number")?.as_u64()?,
                title: text(row, "title"),
                head_sha: text(row, "headRefOid"),
                base: text(row, "baseRefName"),
                draft: row
                    .get("isDraft")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                checks,
                runs,
                task: ids
                    .iter()
                    .filter(|id| suffix == id.as_str() || suffix.starts_with(&format!("{id}-")))
                    .max_by_key(|id| id.len())
                    .cloned(),
                head_ref,
            })
        })
        .collect();
    pulls.sort_by_key(|pull| pull.number);
    ForgeIn::Read {
        read_at: now,
        pulls,
    }
}

fn ledger(root: &Path) -> LedgerIn {
    use crate::coord::store::{read, Paths, ReadOutcome};
    let Ok((paths, _)) = Paths::discover(root) else {
        return LedgerIn::Absent;
    };
    match read(&paths) {
        ReadOutcome::Empty => LedgerIn::Absent,
        ReadOutcome::Unavailable(reason) => LedgerIn::Unreadable(reason),
        ReadOutcome::Ledger(snapshot) => {
            let ledger = &snapshot.folded.ledger;
            let lanes = ledger
                .lanes
                .values()
                .filter(|lane| lane.closed.is_none())
                .map(|lane| LaneIn {
                    id: lane.id.clone(),
                    stream: lane.stream.clone(),
                    scope: lane.scope.clone(),
                    attempt: lane.generation,
                    seat: lane.seat.clone(),
                    reported_at: lane.reported_at.as_deref().and_then(parse_time),
                })
                .collect();
            let operator_items = ledger
                .items
                .values()
                .filter(|item| item.state.as_str() == "open")
                .filter_map(|item| {
                    Some(OperatorItemIn {
                        id: item.id.clone(),
                        title: item.record.title.clone(),
                        awaiting: item.record.awaiting_operator.clone()?,
                        done_when: item.record.done_when.clone(),
                        added_at: parse_time(&item.added_at),
                    })
                })
                .collect();
            LedgerIn::Read {
                lanes,
                operator_items,
            }
        }
    }
}

/// The adopted forecast that covers the most of the scope, when one does.
fn forecast(root: &Path, inputs: &Inputs) -> (Option<ForecastIn>, Option<String>) {
    use crate::estimate::adoption::{read, Adoption, Home};
    let Adoption::Adopted {
        home: Home::Present { forecasts },
        ..
    } = read(root)
    else {
        return (None, None);
    };
    let scope = super::compute::scope_ids(inputs);
    let mut best: Option<(usize, ForecastIn)> = None;
    let mut note = None;
    for path in &forecasts {
        match crate::estimate::task_ends(root, Path::new(path)) {
            Err(reason) => note = Some(format!("{path}: {reason}")),
            Ok(ends) => {
                let covered = ends.ends.keys().filter(|id| scope.contains(*id)).count();
                if covered == 0 || best.as_ref().is_some_and(|(n, _)| *n > covered) {
                    continue;
                }
                let anchor = i64::try_from(ends.anchor_epoch_seconds).unwrap_or(i64::MAX);
                let offset = |end: u64| {
                    (anchor.saturating_add(i64::try_from(end).unwrap_or(i64::MAX)) - inputs.now)
                        .max(0)
                };
                best = Some((
                    covered,
                    ForecastIn {
                        path: path.clone(),
                        stale: ends.stale,
                        ends: ends
                            .ends
                            .iter()
                            .map(|(id, [a, b, c])| {
                                (id.clone(), (offset(*a), offset(*b), offset(*c)))
                            })
                            .collect(),
                    },
                ));
            }
        }
    }
    let note = if best.is_some() { None } else { note };
    (best.map(|(_, forecast)| forecast), note)
}
