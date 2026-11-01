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
    AcceptanceIn, BlockerIn, BranchIn, EpicIn, ForecastIn, ForgeIn, Inputs, LaneIn, LedgerIn,
    OperatorItemIn, PullIn, RefIn, ScopeSpec, TaskIn, UnreadableIn,
};
use super::owed::OwedSource;
use crate::workgraph::readiness;
use crate::workgraph::record_text;
use crate::workgraph::work_start::{self, Record, RecordKind};

/// The default sweep interval when the project records none: 30 minutes.
const DEFAULT_SWEEP_MINUTES: i64 = 30;
/// The most first-parent commits the history walk reads per tip.
const HISTORY_LIMIT: usize = 5_000;
/// How far back the history walk reads.
const HISTORY_DAYS: i64 = 60;

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
    let worktrees = worktree_branches(&repo);
    let history = landings(&repo, &read.tips, now);
    let default_records = default_tip_records(&repo, &default_branch);
    let mut tasks: Vec<TaskIn> = read
        .tasks
        .values()
        .map(|found| {
            task_in(
                &repo,
                found,
                backlog.as_ref(),
                &worktrees,
                &read.tips,
                default_records.as_ref(),
                history.as_ref(),
                &default_branch,
            )
        })
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
    let forge = forge(&root, options, now, &ids);
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
            .map(|source| (source.name().to_string(), source.read()))
            .collect(),
        forecast: None,
        forecast_note: None,
        release_step: root.join("docs/releasing.md").is_file(),
        history_read: history.is_some(),
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
}

fn task_id_of(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    stem.starts_with("TSK-").then(|| stem.to_string())
}

/// Every task on the tip of its own declared target; epics from any tip,
/// the default target's first.
fn read_records(root: &Path, repo: &Repository) -> Read {
    let mut out = Read::default();
    let default = work_start::default_work_target(root).ok().flatten();
    let mut targets: Vec<String> = readiness::declared_targets(root)
        .map(|set| set.into_iter().collect())
        .unwrap_or_default();
    if let Some(default) = &default {
        targets.retain(|target| target != default);
        targets.insert(0, default.clone());
    }
    let mut seen_unreadable = BTreeSet::new();
    for declared in &targets {
        let Ok(Some((_, tip))) = readiness::resolve_target(root, repo, declared) else {
            continue;
        };
        let name = bare(declared);
        out.tips.insert(name.clone(), tip);
        let Ok(tree) = repo.find_commit(tip).and_then(|commit| commit.tree()) else {
            continue;
        };
        let Ok(records) = work_start::records_from_tree_lenient(repo, &tree) else {
            continue;
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
                        if home == name && !out.tasks.contains_key(&record.id) {
                            out.tasks.insert(
                                record.id.clone(),
                                Found {
                                    record,
                                    text,
                                    path: entry.path,
                                    target: home,
                                },
                            );
                        }
                    }
                    RecordKind::Spec => {}
                },
            }
        }
    }
    // A task whose own target is not here is read from the first tip that
    // holds it, so it is never dropped.
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

/// The task statuses the trusted remote's default branch holds; `None`
/// when that ref is not here.
fn default_tip_records(repo: &Repository, default: &str) -> Option<BTreeMap<String, String>> {
    let commit = repo
        .find_reference(&format!("refs/remotes/origin/{default}"))
        .and_then(|reference| reference.peel_to_commit())
        .ok()?;
    let tree = commit.tree().ok()?;
    let records = work_start::records_from_tree_lenient(repo, &tree).ok()?;
    Some(
        records
            .into_iter()
            .filter_map(|entry| entry.read.ok())
            .filter(|(record, _)| record.kind == RecordKind::Task)
            .map(|(record, _)| (record.id, record.status))
            .collect(),
    )
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
        if let Some(branch) = opened
            .head()
            .ok()
            .and_then(|head| head.shorthand().ok().map(str::to_string))
        {
            out.insert(branch, worktree.path().display().to_string());
        }
    }
    out
}

/// A frontmatter value (`key: value`) of a record's text.
fn frontmatter(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim() != "---")
        .find_map(|line| line.strip_prefix(&format!("{key}:")).map(str::trim))
        .map(|value| value.trim_matches(['"', '\'']).to_string())
        .filter(|value| !value.is_empty())
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

/// The active acceptance block of a record's text, with its binding.
fn acceptance(
    repo: &Repository,
    text: &str,
    path: &str,
    head: Option<Oid>,
    bound_on_tip: bool,
) -> Option<AcceptanceIn> {
    let block = record_text::acceptance_blocks(text)
        .into_iter()
        .filter_map(|fenced| fenced.parsed.ok())
        .rfind(|block| !block.superseded)?;
    let total = record_text::parse_criteria(text).items.len();
    let verified = block
        .criteria
        .iter()
        .filter(|(_, result)| result.outcome == "verified")
        .count();
    let bound = bound_on_tip
        || head.is_some_and(|head| only_record_since(repo, &block.reviewed, head, path));
    Some(AcceptanceIn {
        verified,
        total,
        reviewed: block.reviewed.clone(),
        approved: block.verdict == "approved",
        bound,
    })
}

/// No change beyond the record between the reviewed commit and `head`.
fn only_record_since(repo: &Repository, reviewed: &str, head: Oid, path: &str) -> bool {
    let Ok(reviewed) = Oid::from_str(reviewed).and_then(|oid| repo.find_commit(oid)) else {
        return false;
    };
    let (Ok(old), Ok(new)) = (
        reviewed.tree(),
        repo.find_commit(head).and_then(|commit| commit.tree()),
    ) else {
        return false;
    };
    let Ok(diff) = repo.diff_tree_to_tree(Some(&old), Some(&new), None) else {
        return false;
    };
    diff.deltas().all(|delta| {
        delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .is_some_and(|changed| changed == Path::new(path))
    })
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
    let mut walk = repo.revwalk().ok()?;
    walk.push(commit.id()).ok()?;
    if let Some(tip) = tip {
        walk.hide(tip).ok()?;
    }
    let commits_ahead = walk.take(HISTORY_LIMIT).count();
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

#[allow(clippy::too_many_arguments)]
fn task_in(
    repo: &Repository,
    found: &Found,
    backlog: Option<&readiness::Backlog>,
    worktrees: &BTreeMap<String, String>,
    tips: &BTreeMap<String, Oid>,
    default_records: Option<&BTreeMap<String, String>>,
    history: Option<&BTreeMap<String, i64>>,
    default_branch: &str,
) -> TaskIn {
    let record = &found.record;
    let entry = backlog.and_then(|b| b.entries.iter().find(|entry| entry.task_id == record.id));
    let tip = tips.get(&found.target).copied();
    let branch = entry
        .and_then(|entry| entry.branch_names.first())
        .and_then(|name| branch_in(repo, name, tip, worktrees));
    let closed = matches!(record.status.as_str(), "complete" | "cancelled");
    // The most advanced copy: the home tip once complete, else the branch.
    let (acceptance, follow_from) = if closed {
        (
            acceptance(
                repo,
                &found.text,
                &found.path,
                None,
                record.reviewed.is_some(),
            ),
            found.text.clone(),
        )
    } else if let Some((_, head)) = &branch {
        let text = text_at(repo, *head, &found.path).unwrap_or_else(|| found.text.clone());
        (
            acceptance(repo, &text, &found.path, Some(*head), false),
            text,
        )
    } else {
        (None, found.text.clone())
    };
    let follow_ups = section(&follow_from, "## Follow-ups").or_else(|| {
        record_text::acceptance_blocks(&follow_from)
            .into_iter()
            .filter_map(|fenced| fenced.parsed.ok())
            .rfind(|block| !block.superseded)
            .map(|block| block.follow_ups)
            .filter(|text| !text.trim().is_empty() && text.trim() != "none")
    });
    let on_main = default_records.map(|records| {
        found.target == default_branch
            || records
                .get(&record.id)
                .is_some_and(|status| status == "complete")
    });
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
        on_main,
        record_on_line: true,
        landed_at: history.and_then(|landed| landed.get(&record.id).copied()),
    }
}

/// When each task's record became `complete` on the first-parent history
/// of its home tip, newest transition first; `None` when a walk fails.
fn landings(
    repo: &Repository,
    tips: &BTreeMap<String, Oid>,
    now: i64,
) -> Option<BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    let floor = now - HISTORY_DAYS * 86_400;
    let mut spec = git2::DiffOptions::new();
    spec.pathspec("project-management/tasks");
    let status = |blob: Oid| {
        repo.find_blob(blob)
            .ok()
            .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
            .and_then(|text| frontmatter(&text, "status"))
    };
    for tip in tips.values().collect::<BTreeSet<_>>() {
        let mut commit = repo.find_commit(*tip).ok()?;
        for _ in 0..HISTORY_LIMIT {
            if commit.time().seconds() < floor {
                break;
            }
            let Ok(parent) = commit.parent(0) else { break };
            let diff = repo
                .diff_tree_to_tree(
                    Some(&parent.tree().ok()?),
                    Some(&commit.tree().ok()?),
                    Some(&mut spec),
                )
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
                    out.entry(id).or_insert(commit.time().seconds());
                }
            }
            commit = parent;
        }
    }
    Some(out)
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
        .and_then(|head| head.shorthand().ok().map(str::to_string))
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

/// The required checks as read: `missing` when none ran, `skipped` when
/// every one skipped, never rewritten.
fn checks(rollup: Option<&serde_json::Value>) -> String {
    let Some(checks) = rollup.and_then(serde_json::Value::as_array) else {
        return "missing".into();
    };
    let conclusions: Vec<String> = checks
        .iter()
        .map(|check| {
            check
                .get("conclusion")
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
                .or_else(|| check.get("state").and_then(serde_json::Value::as_str))
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .collect();
    if conclusions.is_empty() {
        "missing".into()
    } else if conclusions.iter().any(|c| {
        c.is_empty()
            || matches!(
                c.as_str(),
                "pending" | "queued" | "in_progress" | "expected"
            )
    }) {
        "pending".into()
    } else if conclusions.iter().all(|c| c == "skipped") {
        "skipped".into()
    } else if conclusions
        .iter()
        .all(|c| matches!(c.as_str(), "success" | "skipped" | "neutral"))
    {
        "passed".into()
    } else {
        "failed".into()
    }
}

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
            "200",
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
            Some(PullIn {
                number: row.get("number")?.as_u64()?,
                title: text(row, "title"),
                head_sha: text(row, "headRefOid"),
                base: text(row, "baseRefName"),
                draft: row
                    .get("isDraft")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                checks: checks(row.get("statusCheckRollup")),
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
