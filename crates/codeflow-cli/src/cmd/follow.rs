//! `codeflow follow`: follow-ups in the clone's coordination ledger
//! (ADR-0086). Every verb but `list` and `snapshot` writes one event under
//! the ledger lock as the bound actor; `list` and `snapshot` read only.
//!
//! Exit status: 0 done, 1 refused, 2 no repository or unusable input,
//! 3 busy, 4 read-only (a damaged log), 5 unwritten (a denied write),
//! 6 unavailable (a denied read), 7 key conflict (an idempotency key
//! reused for another operation).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use codeflow_core::coord::actor::{Actor, Unbound, INTAKE};
use codeflow_core::coord::due::{self, Observer, RepoObserver};
use codeflow_core::coord::reference::{Link, Resolver};
use codeflow_core::coord::schema::{
    Data, Drop, Evidence, EvidenceKind, FollowUp, HandOver, ItemCheck, ItemUpdate, Reason,
    Rejected, Source,
};
use codeflow_core::coord::store::{self, Draft, Paths, ReadOutcome, SubjectId, COMMAND_WAIT};
use codeflow_core::coord::text;
use codeflow_core::coord::transition::{Item, ItemState, Ledger};
use codeflow_core::coord::{self as coord, CoordError};
use codeflow_core::git::TextBlock;

#[derive(Debug, Args)]
pub struct FollowArgs {
    #[command(subcommand)]
    pub command: FollowCommand,
}

#[derive(Debug, Subcommand)]
pub enum FollowCommand {
    /// Add a follow-up: reserve the next FU number and record the full item.
    /// Similar items, open tasks, epic criteria and feedback are suggested,
    /// never refused; only an exact duplicate is.
    Add(AddArgs),
    /// Change an open follow-up's text, links, schedule or waits (never its
    /// source). The holder or the operator.
    Update(UpdateArgs),
    /// Record what was observed and the next check. With no id, check every
    /// open item you hold: close those whose fulfils links are done on their
    /// target, reopen those whose proof was reverted, and list the due ones.
    Check(CheckArgs),
    /// Give an open follow-up to another holder or stream.
    HandOver(HandOverArgs),
    /// Close an open follow-up you hold, with evidence a fresh session can
    /// open (commit:SHA, PR#N, TSK-NNN or an https URL).
    Close(CloseArgs),
    /// Drop an open follow-up with a reason and what happens to what it
    /// leaves. The holder, the stream's coordinator or the operator.
    Drop(DropArgs),
    /// Reopen a closed or dropped follow-up; the old proof stays in history.
    Reopen(ReopenArgs),
    /// Show the folded ledger: counts, the urgent subset and a pointer to the
    /// rest. Reads only: never writes, claims a role or takes a lock.
    List(ListArgs),
    /// Print a redacted Markdown export of every follow-up. Never committed;
    /// write it where you choose.
    Snapshot(SnapshotArgs),
    /// Replace a follow-up's free text with `[purged]` in the live log,
    /// keeping its ids, links and history. The holder or the operator.
    Purge(PurgeArgs),
    /// Move a damaged suffix of the log to quarantine so the ledger is
    /// writable again. The operator only; nothing is deleted here (a later
    /// purge removes only the purged item's lines from quarantine).
    Repair,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// One line a cold reader understands.
    #[arg(long)]
    pub title: String,
    /// What this is about, for someone without the conversation.
    #[arg(long)]
    pub description: String,
    /// The observable condition that finishes it.
    #[arg(long, value_name = "CONDITION")]
    pub done_when: String,
    /// Where it comes from: operator, coordinator, lane, review, issue or outside.
    #[arg(long)]
    pub source: String,
    /// The source's own reference (a review id, an issue URL, a message).
    #[arg(long, value_name = "REFERENCE")]
    pub source_reference: Option<String>,
    /// The stream it belongs to (WS-NNN), or `intake`. Default: your own.
    #[arg(long, value_name = "WS-NNN|intake")]
    pub stream: Option<String>,
    /// `coordinator` or a lane id. Default: yourself.
    #[arg(long, value_name = "coordinator|LN-NNN")]
    pub holder: Option<String>,
    /// A typed link, repeat: fulfils:TSK-NNN, prerequisite:PR#N, context:EPC-NNN.
    #[arg(long = "link", value_name = "KIND:TARGET")]
    pub links: Vec<String>,
    /// Why the item links to nothing.
    #[arg(long, value_name = "REASON")]
    pub no_link_reason: Option<String>,
    /// When to check next: a duration (30m, 2h, 1d) or a UTC time. Default 10m.
    #[arg(long, value_name = "WHEN")]
    pub next_check: Option<String>,
    /// A follow-up or lane, possibly another stream's, this waits on. Repeat.
    #[arg(long = "waits-on", value_name = "FU-NNN|LN-NNN")]
    pub waits_on: Vec<String>,
    /// A condition the operator must settle (never a holder).
    #[arg(long, value_name = "CONDITION")]
    pub awaiting_operator: Option<String>,
    /// A suggestion considered and rejected: ID=REASON. Repeat.
    #[arg(long = "reject", value_name = "ID=REASON")]
    pub rejects: Vec<String>,
    /// A key that makes a retried add record once.
    #[arg(long, value_name = "KEY")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
    /// A new title.
    #[arg(long)]
    pub title: Option<String>,
    /// A new description.
    #[arg(long)]
    pub description: Option<String>,
    /// A new finishing condition.
    #[arg(long, value_name = "CONDITION")]
    pub done_when: Option<String>,
    /// Replace the links, repeat.
    #[arg(long = "link", value_name = "KIND:TARGET")]
    pub links: Vec<String>,
    /// Replace the links with a reason for having none.
    #[arg(long, value_name = "REASON")]
    pub no_link_reason: Option<String>,
    /// When to check next: a duration (30m, 2h, 1d) or a UTC time.
    #[arg(long, value_name = "WHEN")]
    pub next_check: Option<String>,
    /// Replace the waits, repeat.
    #[arg(long = "waits-on", value_name = "FU-NNN|LN-NNN")]
    pub waits_on: Vec<String>,
    /// Set the operator condition; an empty value clears it.
    #[arg(long, value_name = "CONDITION")]
    pub awaiting_operator: Option<String>,
    /// The revision you read; refused when the item changed since.
    #[arg(long, value_name = "N")]
    pub expect_revision: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// One follow-up; omit to check every open item you hold.
    pub id: Option<String>,
    /// What you observed, in words. Default: the observed link states.
    #[arg(long, value_name = "TEXT")]
    pub observed: Option<String>,
    /// When to check next: a duration or a UTC time. Default 10m; an item
    /// waiting on a prerequisite is rechecked at least daily.
    #[arg(long, value_name = "WHEN")]
    pub next_check: Option<String>,
    /// Whose answer this records when it is not yours (operator, a review).
    #[arg(long, value_name = "SOURCE")]
    pub answer_source: Option<String>,
    /// Make no network observation (pull requests, remote branches).
    #[arg(long)]
    pub offline: bool,
    /// The revision you read; refused when the item changed since.
    #[arg(long, value_name = "N")]
    pub expect_revision: Option<u64>,
}

#[derive(Debug, Args)]
pub struct HandOverArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
    /// The new holder: `coordinator` or a lane id.
    #[arg(long, value_name = "coordinator|LN-NNN")]
    pub to: String,
    /// Move it to another stream (WS-NNN) or `intake`.
    #[arg(long, value_name = "WS-NNN|intake")]
    pub stream: Option<String>,
    /// A note recorded with the event.
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Debug, Args)]
pub struct CloseArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
    /// commit:SHA, PR#N, TSK-NNN or an https URL.
    #[arg(long, value_name = "REFERENCE")]
    pub evidence: String,
    /// A note recorded with the event.
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Debug, Args)]
pub struct DropArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
    /// Why it is dropped.
    #[arg(long)]
    pub reason: String,
    /// What happens to what it leaves (relinked to, filed as, nothing left).
    #[arg(long)]
    pub disposition: String,
}

#[derive(Debug, Args)]
pub struct ReopenArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
    /// Why it is open again.
    #[arg(long)]
    pub reason: String,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Show one follow-up in full.
    pub id: Option<String>,
    /// Markdown instead of a table.
    #[arg(long)]
    pub md: bool,
    /// Every item, closed and dropped included.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// Write here instead of standard output; an existing file is refused.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PurgeArgs {
    /// The follow-up, FU-NNN.
    pub id: String,
}

/// The most urgent items `list` shows before it points at the rest.
const URGENT_SHOWN: usize = 20;

#[must_use]
pub fn run(args: &FollowArgs) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => return fail(&CoordError::Repository(error.to_string())),
    };
    let (paths, root) = match Paths::discover(&cwd) {
        Ok(found) => found,
        Err(error) => return fail(&error),
    };
    let result = match &args.command {
        FollowCommand::List(list) => return list_cmd(&paths, &root, list),
        FollowCommand::Snapshot(snapshot) => return snapshot_cmd(&paths, snapshot),
        command => mutate(&paths, &root, command),
    };
    match result {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

fn fail(error: &CoordError) -> i32 {
    eprintln!("follow: {error}");
    match error {
        CoordError::Refused(_) => 1,
        CoordError::Repository(_) => 2,
        CoordError::Busy => 3,
        CoordError::ReadOnly(_) => 4,
        CoordError::Unwritten(_) => 5,
        CoordError::Unavailable(_) => 6,
        CoordError::KeyConflict(_) => 7,
    }
}

fn refused(message: impl Into<String>) -> CoordError {
    CoordError::Refused(message.into())
}

fn bound_actor() -> Result<Actor, CoordError> {
    use std::io::IsTerminal as _;
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    Actor::from_env(&codeflow_core::root_checkout::process_env, terminal).map_err(|unbound| {
        match unbound {
            Unbound::Observer => refused(
                "no role is bound: an unbound watcher may only read (`codeflow follow list`)",
            ),
            Unbound::Refused(reason) => refused(reason),
        }
    })
}

fn schedule(text: Option<&str>, now: u64) -> Result<u64, CoordError> {
    match text {
        Some(text) => coord::parse_schedule(text, now).map_err(refused),
        None => Ok(now + coord::CHECK_INTERVAL_SECS),
    }
}

fn links(resolver: &Resolver<'_>, written: &[String]) -> Result<Vec<Link>, CoordError> {
    written
        .iter()
        .map(|text| resolver.link(text).map_err(refused))
        .collect()
}

fn mutate(paths: &Paths, root: &Path, command: &FollowCommand) -> Result<(), CoordError> {
    if let FollowCommand::Repair = command {
        return repair(paths);
    }
    let actor = bound_actor()?;
    match command {
        FollowCommand::Add(add) => add_cmd(paths, root, &actor, add),
        FollowCommand::Update(update) => update_cmd(paths, root, &actor, update),
        FollowCommand::Check(check) => check_cmd(paths, root, &actor, check),
        FollowCommand::HandOver(hand) => one(
            paths,
            &actor,
            &hand.id,
            None,
            hand.comment.clone(),
            Data::ItemHandedOver(HandOver {
                to_holder: hand.to.clone(),
                to_stream: hand.stream.clone(),
            }),
            "handed over",
        ),
        FollowCommand::Close(close) => {
            let evidence = asserted_evidence(root, &close.evidence)?;
            one(
                paths,
                &actor,
                &close.id,
                None,
                close.comment.clone(),
                Data::ItemClosed(evidence),
                "closed",
            )
        }
        FollowCommand::Drop(drop) => one(
            paths,
            &actor,
            &drop.id,
            None,
            None,
            Data::ItemDropped(Drop {
                reason: drop.reason.clone(),
                disposition: drop.disposition.clone(),
            }),
            "dropped",
        ),
        FollowCommand::Reopen(reopen) => one(
            paths,
            &actor,
            &reopen.id,
            None,
            None,
            Data::ItemReopened(Reason {
                reason: reopen.reason.clone(),
            }),
            "reopened",
        ),
        FollowCommand::Purge(purge) => purge_cmd(paths, &actor, &purge.id),
        FollowCommand::List(_) | FollowCommand::Snapshot(_) | FollowCommand::Repair => Ok(()),
    }
}

fn report(committed: &store::Committed, verb: &str) {
    if let Some(path) = &committed.repaired_tail {
        eprintln!(
            "follow: a torn write at the end of the log was moved to {}",
            path.display()
        );
    }
    if let Some(sequence) = committed.duplicate_of {
        println!("already recorded at sequence {sequence}; nothing was written");
    }
    for envelope in &committed.envelopes {
        println!(
            "{} {verb} (sequence {})",
            envelope.subject_id, envelope.sequence
        );
    }
    if let Some(error) = &committed.heartbeat_error {
        eprintln!("follow: warning: the liveness file was not refreshed: {error}");
    }
}

#[allow(clippy::too_many_arguments)]
fn one(
    paths: &Paths,
    actor: &Actor,
    id: &str,
    expected: Option<u64>,
    comment: Option<String>,
    data: Data,
    verb: &str,
) -> Result<(), CoordError> {
    let committed = store::commit(paths, &actor.recorded_by, COMMAND_WAIT, |_| {
        Ok(vec![Draft {
            subject: SubjectId::Given(id.to_string()),
            data,
            comment,
            idempotency_key: None,
            expected_revision: expected,
        }])
    })?;
    report(&committed, verb);
    Ok(())
}

fn asserted_evidence(root: &Path, written: &str) -> Result<Evidence, CoordError> {
    use codeflow_core::coord::reference::Target;
    let resolver = Resolver::new(root);
    let reference = if let Some(url) = written.strip_prefix("https://") {
        if url.is_empty() || url.contains(char::is_whitespace) {
            return Err(refused("evidence must be a URL a fresh session can open"));
        }
        written.to_string()
    } else {
        // Resolved, so the reference can be opened: a commit must be in
        // this repository and a record must exist.
        let link = resolver
            .link(&format!("context:{written}"))
            .map_err(refused)?;
        match link.target {
            Target::Commit { sha, .. } => format!("commit:{sha}"),
            Target::Record { .. } | Target::PullRequest { .. } | Target::Branch { .. } => {
                link.target.display()
            }
            Target::FollowUp { .. } | Target::Lane { .. } | Target::Time { .. } => {
                return Err(refused(
                    "evidence is a commit, a pull request, a record or an https URL",
                ))
            }
        }
    };
    Ok(Evidence {
        kind: EvidenceKind::Asserted,
        reference,
        source: "asserted by the closer".into(),
        repository: Some(resolver.repository.clone()),
        target: None,
        revision: None,
        observed_at: coord::format_time(coord::now_secs()),
    })
}

#[allow(clippy::too_many_lines)]
fn add_cmd(paths: &Paths, root: &Path, actor: &Actor, add: &AddArgs) -> Result<(), CoordError> {
    use codeflow_core::coord::actor::Role;
    let now = coord::now_secs();
    let resolver = Resolver::new(root);
    let links = links(&resolver, &add.links)?;
    let source = Source::parse(&add.source).map_err(refused)?;
    let mut rejected = Vec::new();
    for entry in &add.rejects {
        let (reference, reason) = entry
            .split_once('=')
            .ok_or_else(|| refused("--reject takes ID=REASON"))?;
        rejected.push(Rejected {
            reference: reference.to_string(),
            reason: reason.to_string(),
        });
    }
    // Network and record reads run before the lock; the store re-judges
    // every rule against the state it reads under the lock.
    let mut watcher = RepoObserver::new(root, false);
    let snapshot = match store::read(paths) {
        ReadOutcome::Ledger(snapshot) => Some(snapshot),
        _ => None,
    };
    let empty = Ledger::default();
    let ledger_now = snapshot.as_ref().map_or(&empty, |s| &s.folded.ledger);
    let liveness = store::liveness(paths);
    let (default_stream, default_holder) = match &actor.role {
        Role::Coordinator => (actor.recorded_by.stream.clone(), "coordinator".to_string()),
        Role::Lane(id) => (
            ledger_now.lanes.get(id).map(|lane| lane.stream.clone()),
            id.clone(),
        ),
        Role::Operator => (None, "coordinator".to_string()),
    };
    let probe = Item {
        id: "FU-000".into(),
        record: FollowUp {
            title: add.title.clone(),
            description: add.description.clone(),
            done_when: add.done_when.clone(),
            source,
            source_reference: add.source_reference.clone(),
            stream: INTAKE.into(),
            holder: "coordinator".into(),
            links: links.clone(),
            no_link_reason: add.no_link_reason.clone(),
            next_check: String::new(),
            schedule: add.next_check.clone(),
            waits_on: add.waits_on.clone(),
            awaiting_operator: None,
            rejected_suggestions: Vec::new(),
            observed: Vec::new(),
        },
        state: ItemState::Open,
        revision: 0,
        added_at: String::new(),
        added_by: actor.recorded_by.clone(),
        last_check: None,
        proofs: Vec::new(),
        reopened: Vec::new(),
        purged: Vec::new(),
        baseline: Vec::new(),
        identity_purged: false,
        last_sequence: 0,
    };
    let observed = due::observe_item(&probe, ledger_now, &liveness, &mut watcher);
    let requested = schedule(add.next_check.as_deref(), now)?;
    let next_check = due::capped_next_check(&probe, requested, now);
    let stream = add
        .stream
        .clone()
        .or(default_stream)
        .unwrap_or_else(|| INTAKE.to_string());
    let holder = add.holder.clone().unwrap_or(default_holder);
    let record = FollowUp {
        stream,
        holder,
        next_check: coord::format_time(next_check),
        awaiting_operator: add.awaiting_operator.clone(),
        rejected_suggestions: rejected.clone(),
        observed,
        ..probe.record
    };
    let mut considered = rejected.clone();
    // A record already linked is no suggestion.
    considered.extend(record.links.iter().map(|link| Rejected {
        reference: link.target.display(),
        reason: "linked".into(),
    }));
    let suggestions = suggest(root, ledger_now, &record.title, &considered);
    let committed = store::commit(paths, &actor.recorded_by, COMMAND_WAIT, |_| {
        Ok(vec![Draft {
            subject: SubjectId::NextFollowUp,
            data: Data::ItemAdded(record),
            comment: None,
            idempotency_key: add.idempotency_key.clone(),
            expected_revision: None,
        }])
    })?;
    report(&committed, "added");
    for (reference, title) in suggestions {
        println!(
            "similar: {reference} \"{}\": link it with --link context:{reference}, or record --reject {reference}=<reason>",
            text::terminal(&title)
        );
    }
    Ok(())
}

/// Words of a title, for the similarity suggestion.
fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

fn similar(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let shared = a.intersection(b).count();
    let union = a.union(b).count();
    shared * 2 >= union
}

/// Similar open follow-ups, open tasks, epic criteria and open feedback,
/// each named so the adder can link or reject it. Never a refusal.
fn suggest(
    root: &Path,
    ledger: &Ledger,
    title: &str,
    rejected: &[Rejected],
) -> Vec<(String, String)> {
    let mine = words(title);
    let skip: BTreeSet<&str> = rejected.iter().map(|r| r.reference.as_str()).collect();
    let mut out = Vec::new();
    for item in ledger.items.values() {
        if item.state == ItemState::Open && similar(&mine, &words(&item.record.title)) {
            out.push((item.id.clone(), item.record.title.clone()));
        }
    }
    let pm = root.join("project-management");
    for (dir, open_only) in [("tasks", true), ("epics", false)] {
        let Ok(entries) = std::fs::read_dir(pm.join(dir)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let value = |key| codeflow_core::ids::entry::frontmatter_value(&content, key);
            let (Some(id), Some(record_title)) = (value("id"), value("title")) else {
                continue;
            };
            let record_title = record_title.trim_matches('"').to_string();
            let status = value("status").unwrap_or_default();
            let open = !matches!(
                status.split('#').next().unwrap_or_default().trim(),
                "complete" | "cancelled"
            );
            if open_only && open && similar(&mine, &words(&record_title)) {
                out.push((id.clone(), record_title.clone()));
            }
            if !open_only {
                for line in content.lines().filter(|line| line.starts_with("- AC-")) {
                    if similar(&mine, &words(line)) {
                        let criterion = line
                            .trim_start_matches("- ")
                            .split_whitespace()
                            .next()
                            .unwrap_or("AC");
                        out.push((
                            format!("{id} {criterion}"),
                            line.chars().take(120).collect(),
                        ));
                    }
                }
            }
        }
    }
    for item in codeflow_core::feedback::load(root).items {
        if !matches!(item.status.as_str(), "placed" | "superseded" | "declined")
            && similar(&mine, &words(&item.title))
        {
            out.push((item.id.clone(), item.title.clone()));
        }
    }
    out.retain(|(reference, _)| !skip.contains(reference.as_str()));
    out.truncate(10);
    out
}

fn update_cmd(
    paths: &Paths,
    root: &Path,
    actor: &Actor,
    update: &UpdateArgs,
) -> Result<(), CoordError> {
    let resolver = Resolver::new(root);
    let now = coord::now_secs();
    let new_links = if update.links.is_empty() {
        None
    } else {
        Some(links(&resolver, &update.links)?)
    };
    let waits = (!update.waits_on.is_empty()).then(|| update.waits_on.clone());
    let snapshot = match store::read(paths) {
        ReadOutcome::Ledger(snapshot) => snapshot,
        ReadOutcome::Empty => return Err(refused("the ledger is empty")),
        ReadOutcome::Unavailable(reason) => return Err(CoordError::Unavailable(reason)),
    };
    let ledger = &snapshot.folded.ledger;
    let item = ledger
        .items
        .get(&update.id)
        .ok_or_else(|| refused(format!("{} is not in this ledger", update.id)))?;
    // The item as it will stand, so new dependencies are observed (outside
    // the lock) and the schedule rule applies to them.
    let mut probe = item.clone();
    if let Some(links) = &new_links {
        probe.record.links.clone_from(links);
        probe.record.no_link_reason = None;
    }
    if update.no_link_reason.is_some() && new_links.is_none() {
        probe.record.links.clear();
    }
    if let Some(waits) = &waits {
        probe.record.waits_on.clone_from(waits);
    }
    let dependencies_changed =
        new_links.is_some() || update.no_link_reason.is_some() || waits.is_some();
    let observed = if dependencies_changed {
        let mut watcher = RepoObserver::new(root, false);
        Some(due::observe_item(
            &probe,
            ledger,
            &store::liveness(paths),
            &mut watcher,
        ))
    } else {
        None
    };
    let requested = match update.next_check.as_deref() {
        Some(text) => coord::parse_schedule(text, now).map_err(refused)?,
        None => coord::parse_time(&item.record.next_check).unwrap_or(now),
    };
    let effective = due::capped_next_check(&probe, requested, now);
    let next_check = (update.next_check.is_some()
        || coord::format_time(effective) != item.record.next_check)
        .then(|| coord::format_time(effective));
    let change = ItemUpdate {
        title: update.title.clone(),
        description: update.description.clone(),
        done_when: update.done_when.clone(),
        links: new_links,
        no_link_reason: update.no_link_reason.clone(),
        next_check,
        waits_on: waits,
        awaiting_operator: update.awaiting_operator.clone(),
        observed,
    };
    one(
        paths,
        actor,
        &update.id,
        Some(update.expect_revision.unwrap_or(item.revision)),
        None,
        Data::ItemUpdated(change),
        "updated",
    )
}

#[allow(clippy::too_many_lines)]
fn check_cmd(
    paths: &Paths,
    root: &Path,
    actor: &Actor,
    check: &CheckArgs,
) -> Result<(), CoordError> {
    let now = coord::now_secs();
    let snapshot = match store::read(paths) {
        ReadOutcome::Ledger(snapshot) => snapshot,
        ReadOutcome::Empty => return Err(refused("the ledger is empty; nothing to check")),
        ReadOutcome::Unavailable(reason) => return Err(CoordError::Unavailable(reason)),
    };
    if let Some(damage) = &snapshot.folded.damage {
        return Err(CoordError::ReadOnly(damage.clone()));
    }
    let ledger = &snapshot.folded.ledger;
    let bound = ledger.bind(&actor.recorded_by).map_err(refused)?;
    let liveness = store::liveness(paths);
    // Observations, network included, run outside the lock; each event
    // carries the revision it was judged against and is refused if the
    // item moved meanwhile.
    let mut watcher = RepoObserver::new(root, !check.offline);
    let mut drafts = Vec::new();
    let mut due_lines = Vec::new();
    let selected: Vec<&Item> = match &check.id {
        Some(id) => vec![ledger
            .items
            .get(id)
            .ok_or_else(|| refused(format!("{id} is not in this ledger")))?],
        None => ledger
            .items
            .values()
            .filter(|item| codeflow_core::coord::transition::holds(&bound, item))
            .collect(),
    };
    for item in selected {
        let expected = Some(check.expect_revision.unwrap_or(item.revision));
        if item.state != ItemState::Open {
            if let Some(reason) = due::reverted(item, ledger, &mut watcher) {
                drafts.push(Draft {
                    subject: SubjectId::Given(item.id.clone()),
                    data: Data::ItemReopened(Reason { reason }),
                    comment: None,
                    idempotency_key: None,
                    expected_revision: expected,
                });
            } else if check.id.is_some() {
                return Err(refused(format!(
                    "{} is {}; reopen it to check it",
                    item.id,
                    item.state.as_str()
                )));
            }
            continue;
        }
        if let Some(evidence) = due::fulfilment(item, ledger, &mut watcher) {
            drafts.push(Draft {
                subject: SubjectId::Given(item.id.clone()),
                data: Data::ItemClosed(evidence),
                comment: None,
                idempotency_key: None,
                expected_revision: expected,
            });
            continue;
        }
        let observations = due::observe_item(item, ledger, &liveness, &mut watcher);
        let reasons = due::due_reasons(item, now, &observations);
        if check.id.is_none() {
            for reason in reasons {
                due_lines.push(format!("{} due: {reason}", item.id));
            }
            continue;
        }
        let requested = schedule(check.next_check.as_deref(), now)?;
        let observed = check.observed.clone().unwrap_or_else(|| {
            let states: Vec<String> = observations
                .iter()
                .map(|seen| format!("{} {}", seen.link, seen.state))
                .collect();
            if states.is_empty() {
                "checked; nothing linked to observe".into()
            } else {
                format!("observed {}", states.join(", "))
            }
        });
        drafts.push(Draft {
            subject: SubjectId::Given(item.id.clone()),
            data: Data::ItemChecked(ItemCheck {
                observed,
                next_check: coord::format_time(due::capped_next_check(item, requested, now)),
                observations,
                answer_source: check.answer_source.clone(),
            }),
            comment: None,
            idempotency_key: None,
            expected_revision: expected,
        });
    }
    for line in &due_lines {
        println!("{line}");
    }
    if drafts.is_empty() {
        if check.id.is_none() && due_lines.is_empty() {
            println!("nothing due among the items you hold");
        }
        // A bound check is a mutating operation even when it records no
        // event: it refreshes the actor's liveness (never a read's job).
        if let Err(error) = store::refresh_liveness(paths, &actor.recorded_by, ledger.last_sequence)
        {
            eprintln!("follow: warning: the liveness file was not refreshed: {error}");
        }
        return Ok(());
    }
    let committed = store::commit(paths, &actor.recorded_by, COMMAND_WAIT, |_| Ok(drafts))?;
    for envelope in &committed.envelopes {
        let verb = match envelope.event.as_str() {
            "item_closed" => "closed on machine-verified evidence",
            "item_reopened" => "reopened: its proof no longer holds",
            _ => "checked",
        };
        println!(
            "{} {verb} (sequence {})",
            envelope.subject_id, envelope.sequence
        );
    }
    if let Some(error) = &committed.heartbeat_error {
        eprintln!("follow: warning: the liveness file was not refreshed: {error}");
    }
    Ok(())
}

fn purge_cmd(paths: &Paths, actor: &Actor, id: &str) -> Result<(), CoordError> {
    // Authorization is judged under the lock, inside the store.
    let purged = store::purge(paths, &actor.recorded_by, id, COMMAND_WAIT)?;
    match purged.sequence {
        Some(sequence) => println!(
            "{id}: every free-text field of {} events replaced with [purged]; ids, links, times and order kept; {} of its quarantined lines removed, other items' kept; recorded as sequence {sequence}",
            purged.lines, purged.quarantine_removed
        ),
        None => println!(
            "{id}: not in the ledger; {} of its quarantined lines removed, other items' kept; no event was recorded",
            purged.quarantine_removed
        ),
    }
    for path in &purged.retained {
        eprintln!(
            "follow: warning: {} still names {id} in a line whose subject cannot be read; it was kept, not purged: inspect it and remove it by hand if it is {id}'s",
            path.display()
        );
    }
    Ok(())
}

fn repair(paths: &Paths) -> Result<(), CoordError> {
    let actor = bound_actor()?;
    if actor.recorded_by.role != "operator" {
        return Err(refused("only the operator repairs the ledger"));
    }
    let repaired = store::repair(paths, COMMAND_WAIT)?;
    match repaired.quarantined {
        Some(path) => println!(
            "kept {} events; the damaged suffix is in {}",
            repaired.kept,
            path.display()
        ),
        None => println!("the log is sound; {} events, nothing moved", repaired.kept),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct View<'a> {
    item: &'a Item,
    due: Vec<String>,
}

fn list_cmd(paths: &Paths, root: &Path, list: &ListArgs) -> i32 {
    let snapshot = match store::read(paths) {
        ReadOutcome::Empty => {
            println!("coordination ledger: empty (no ledger in this clone)");
            return 0;
        }
        ReadOutcome::Unavailable(reason) => return fail(&CoordError::Unavailable(reason)),
        ReadOutcome::Ledger(snapshot) => snapshot,
    };
    if let Some(damage) = &snapshot.folded.damage {
        eprintln!(
            "follow: warning: the ledger is read-only ({}); showing the valid prefix",
            text::scrub(damage)
        );
    }
    if snapshot.folded.tail > 0 {
        eprintln!("follow: note: a write is in progress or torn at the end of the log; showing the committed prefix");
    }
    let ledger = &snapshot.folded.ledger;
    let liveness = store::liveness(paths);
    let mut watcher = RepoObserver::new(root, false);
    let now = watcher.now();
    if let Some(id) = &list.id {
        let Some(item) = ledger.items.get(id) else {
            return fail(&refused(format!("{id} is not in this ledger")));
        };
        let observed = due::observe_item(item, ledger, &liveness, &mut watcher);
        let due = due::due_reasons(item, now, &observed);
        print_block!(full_record(item, &due, list.md, false));
        return 0;
    }
    let views: Vec<View<'_>> = ledger
        .items
        .values()
        .map(|item| {
            let observed = due::observe_item(item, ledger, &liveness, &mut watcher);
            View {
                due: due::due_reasons(item, now, &observed),
                item,
            }
        })
        .collect();
    let instance = snapshot
        .instance
        .as_ref()
        .map_or("unknown", |instance| instance.instance_id.as_str());
    print_block!(render_list(&views, instance, list.md, list.all));
    0
}

fn counts(views: &[View<'_>]) -> String {
    let open = views
        .iter()
        .filter(|view| view.item.state == ItemState::Open)
        .count();
    let due = views.iter().filter(|view| !view.due.is_empty()).count();
    let awaiting = views
        .iter()
        .filter(|view| {
            view.item.state == ItemState::Open && view.item.record.awaiting_operator.is_some()
        })
        .count();
    let closed = views
        .iter()
        .filter(|view| matches!(view.item.state, ItemState::Closed(_)))
        .count();
    let dropped = views.len() - open - closed;
    format!("{open} open ({due} due, {awaiting} awaiting the operator), {closed} closed, {dropped} dropped")
}

fn holder_label(item: &Item) -> String {
    format!("{} / {}", item.record.stream, item.record.holder)
}

fn render_list(views: &[View<'_>], instance: &str, md: bool, all: bool) -> TextBlock {
    let mut block = TextBlock::new();
    let selected: Vec<&View<'_>> = views
        .iter()
        .filter(|view| {
            all || (view.item.state == ItemState::Open
                && (!view.due.is_empty() || view.item.record.awaiting_operator.is_some()))
        })
        .collect();
    let mut groups: BTreeMap<String, Vec<&View<'_>>> = BTreeMap::new();
    for view in selected
        .iter()
        .take(if all { usize::MAX } else { URGENT_SHOWN })
    {
        groups
            .entry(holder_label(view.item))
            .or_default()
            .push(view);
    }
    let heading = if all {
        "every item"
    } else {
        "urgent: due or awaiting the operator"
    };
    if md {
        let _ = writeln!(block, "# Coordination ledger");
        let _ = writeln!(block);
        let _ = writeln!(block, "Ledger `{instance}`: {}.", counts(views));
        let _ = writeln!(block);
        let _ = writeln!(block, "## {heading}");
        for (group, items) in &groups {
            let _ = writeln!(block);
            let _ = writeln!(block, "### {}", text::markdown(group));
            let _ = writeln!(block);
            let _ = writeln!(block, "| Id | State | Title | Next check | Due |");
            let _ = writeln!(block, "|---|---|---|---|---|");
            for view in items {
                let _ = writeln!(
                    block,
                    "| {} | {} | {} | {} | {} |",
                    view.item.id,
                    view.item.state.as_str(),
                    text::markdown(&view.item.record.title),
                    view.item.record.next_check,
                    text::markdown(&due_cell(view))
                );
            }
        }
    } else {
        let _ = writeln!(block, "coordination ledger {instance}: {}", counts(views));
        let _ = writeln!(block, "{heading}:");
        for (group, items) in &groups {
            let _ = writeln!(block, "  {}", text::terminal(group));
            for view in items {
                let _ = writeln!(
                    block,
                    "    {:<8} {:<8} {}  (next check {})",
                    view.item.id,
                    view.item.state.as_str(),
                    text::terminal(&view.item.record.title),
                    view.item.record.next_check
                );
                let cell = due_cell(view);
                if !cell.is_empty() {
                    let _ = writeln!(block, "      {}", text::terminal(&cell));
                }
            }
        }
    }
    if groups.is_empty() {
        if md {
            let _ = writeln!(block);
            let _ = writeln!(block, "Nothing here.");
        } else {
            let _ = writeln!(block, "  nothing");
        }
    }
    if !all && selected.len() > URGENT_SHOWN {
        if md {
            let _ = writeln!(block);
        }
        let _ = writeln!(
            block,
            "showing {URGENT_SHOWN} of {} urgent items",
            selected.len()
        );
    }
    if !all {
        if md {
            let _ = writeln!(block);
        }
        let _ = writeln!(
            block,
            "`codeflow follow list --all` lists every item; `codeflow follow list FU-NNN` shows one in full"
        );
    }
    block
}

fn due_cell(view: &View<'_>) -> String {
    let mut parts = view.due.clone();
    if let Some(condition) = &view.item.record.awaiting_operator {
        if view.item.state == ItemState::Open {
            parts.push(format!("awaiting the operator: {condition}"));
        }
    }
    parts.join("; ")
}

fn render_text(value: &str, md: bool, redacted: bool) -> String {
    let value = if redacted {
        text::redact(value)
    } else {
        value.to_string()
    };
    if md {
        text::markdown(&value)
    } else {
        text::terminal(&value)
    }
}

/// One follow-up in full, readable without its conversation.
#[allow(clippy::too_many_lines)]
fn full_record(item: &Item, due: &[String], md: bool, redacted: bool) -> TextBlock {
    let mut block = TextBlock::new();
    let record = &item.record;
    let t = |value: &str| render_text(value, md, redacted);
    let field = |block: &mut TextBlock, name: &str, value: &str| {
        if md {
            let _ = writeln!(block, "- **{name}:** {value}");
        } else {
            let _ = writeln!(block, "  {name}: {value}");
        }
    };
    if md {
        let _ = writeln!(block, "## {}: {}", item.id, t(&record.title));
        let _ = writeln!(block);
    } else {
        let _ = writeln!(block, "{}: {}", item.id, t(&record.title));
    }
    field(&mut block, "state", item.state.as_str());
    field(&mut block, "stream", &t(&record.stream));
    field(&mut block, "holder", &t(&record.holder));
    field(&mut block, "description", &t(&record.description));
    field(&mut block, "done when", &t(&record.done_when));
    let source = match &record.source_reference {
        Some(reference) => format!("{} ({})", record.source.as_str(), t(reference)),
        None => record.source.as_str().to_string(),
    };
    field(&mut block, "source", &source);
    if record.links.is_empty() {
        field(
            &mut block,
            "links",
            &format!(
                "none: {}",
                t(record.no_link_reason.as_deref().unwrap_or(""))
            ),
        );
    } else {
        let links: Vec<String> = record.links.iter().map(|link| t(&link.display())).collect();
        field(&mut block, "links", &links.join(", "));
    }
    if !record.waits_on.is_empty() {
        field(&mut block, "waits on", &record.waits_on.join(", "));
    }
    if let Some(condition) = &record.awaiting_operator {
        field(&mut block, "awaiting the operator", &t(condition));
    }
    field(&mut block, "next check", &record.next_check);
    field(
        &mut block,
        "added",
        &format!("{} by {}", item.added_at, item.added_by.role),
    );
    if let Some((at, check)) = &item.last_check {
        let source = check
            .answer_source
            .as_ref()
            .map(|source| format!(" (answer from {})", t(source)))
            .unwrap_or_default();
        field(
            &mut block,
            "last check",
            &format!("{at}: {}{source}", t(&check.observed)),
        );
    }
    if !due.is_empty() {
        field(&mut block, "due", &t(&due.join("; ")));
    }
    for proof in &item.proofs {
        let kind = match proof.kind {
            EvidenceKind::Asserted => "asserted",
            EvidenceKind::MachineVerified => "machine-verified",
        };
        field(
            &mut block,
            "proof",
            &format!(
                "{kind}: {} ({}, observed {})",
                t(&proof.reference),
                t(&proof.source),
                proof.observed_at
            ),
        );
    }
    for (at, role) in &item.purged {
        field(&mut block, "purged", &format!("{at} by {role}"));
    }
    for reason in &item.reopened {
        field(&mut block, "reopened", &t(reason));
    }
    if let ItemState::Dropped {
        reason,
        disposition,
    } = &item.state
    {
        field(
            &mut block,
            "dropped",
            &format!("{}; left: {}", t(reason), t(disposition)),
        );
    }
    for rejected in &record.rejected_suggestions {
        field(
            &mut block,
            "not a duplicate of",
            &format!("{} ({})", t(&rejected.reference), t(&rejected.reason)),
        );
    }
    block
}

fn snapshot_cmd(paths: &Paths, snapshot: &SnapshotArgs) -> i32 {
    let read = match store::read(paths) {
        ReadOutcome::Empty => {
            println!("coordination ledger: empty (no ledger in this clone)");
            return 0;
        }
        ReadOutcome::Unavailable(reason) => return fail(&CoordError::Unavailable(reason)),
        ReadOutcome::Ledger(read) => read,
    };
    if let Some(damage) = &read.folded.damage {
        eprintln!(
            "follow: warning: the ledger is read-only ({}); exporting the valid prefix",
            text::scrub(damage)
        );
    }
    let mut block = TextBlock::new();
    let instance = read
        .instance
        .as_ref()
        .map_or("unknown", |instance| instance.instance_id.as_str());
    let _ = writeln!(block, "# Coordination ledger snapshot");
    let _ = writeln!(block);
    let _ = writeln!(
        block,
        "Ledger `{instance}`, through sequence {}, exported {}. Redacted: secret-shaped lines are replaced and session details are left block. Ids are local to this ledger; cite them as `{instance}/FU-NNN`.",
        read.folded.ledger.last_sequence,
        coord::format_time(coord::now_secs())
    );
    for item in read.folded.ledger.items.values() {
        let _ = writeln!(block);
        block.push_str(full_record(item, &[], true, true).as_str());
    }
    match &snapshot.output {
        None => {
            print_block!(block);
            0
        }
        Some(path) => {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let written = options.open(path).and_then(|mut file| {
                use std::io::Write as _;
                file.write_all(block.as_str().as_bytes())
            });
            match written {
                Ok(()) => {
                    println!(
                        "wrote {}; it is not committed, and nothing here commits it",
                        path.display()
                    );
                    0
                }
                Err(error) => fail(&CoordError::Unwritten(format!(
                    "{}: {error}",
                    path.display()
                ))),
            }
        }
    }
}
