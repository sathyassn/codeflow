//! Issues per task and lane, the operator-owned set, the lanes view, pace
//! and intake, and the evidence entries.

use std::collections::BTreeSet;

use super::compute::{count_f64, mean, pull_for, stage, time_text, Stage};
use super::inputs::{ForgeIn, Inputs, LedgerIn, RequiredCheckIn, TaskIn};
use super::model::{
    DecisionFact, EvidenceFact, IssueFact, IssuesFact, Known, LaneFact, LanesFact,
    OperatorItemFact, OperatorSet, PaceDay, PaceFact, SessionLaneFact, SourceCheck, Unblocker,
};
use super::owed::{OwedKind, OwedRead};
use super::plan::{is_open, Plan, Win};
use crate::delegate::shell_quote;

fn word(text: &str) -> Known<String> {
    Known::Word(text.into())
}

fn issue(
    subject: &str,
    kind: &'static str,
    detail: String,
    last_check: Known<String>,
    unblocker: Unblocker,
) -> IssueFact {
    IssueFact {
        subject: subject.to_string(),
        kind,
        detail,
        last_check,
        unblocker,
        clears: Known::Word("not yet estimable".into()),
        recommended_action: Known::not_recorded(),
        why: Known::not_recorded(),
        alternatives: Known::not_recorded(),
        command: word("none"),
        source: "records".into(),
    }
}

/// The source of the required-check issues and of their evidence: the
/// forge's runs and its required set, checked only when both were read.
pub(super) const REQUIRED_SOURCE: &str = "required checks";

/// The `owner/name` a `gh --repo` takes, for a GitHub repository.
fn gh_repo(inputs: &Inputs) -> Option<&str> {
    inputs.repository.strip_prefix("github.com/")
}

/// Issues per task and per lane, and the operator-owned set.
#[must_use]
pub fn issues(inputs: &Inputs, ids: &BTreeSet<String>, plan: &Plan) -> IssuesFact {
    let now = time_text(inputs.now);
    let mut tasks = Vec::new();
    for task in inputs
        .tasks
        .iter()
        .filter(|task| ids.contains(&task.id) && is_open(task))
    {
        task_issues(inputs, plan, task, &now, &mut tasks);
    }
    for (name, read) in &inputs.owed {
        let OwedRead::Items(items) = read else {
            continue;
        };
        for item in items
            .iter()
            .filter(|item| item.kind == OwedKind::DeferredResult)
        {
            let Some(task) = item.task.as_ref().filter(|task| ids.contains(*task)) else {
                continue;
            };
            let mut fact = issue(
                task,
                "deferred result owed",
                item.summary.clone(),
                Known::Value(now.clone()),
                Unblocker {
                    kind: "agent",
                    name: Known::Value(item.owner.clone()),
                },
            );
            fact.recommended_action = Known::or_word(
                item.event.clone().map(|e| format!("resolve it when {e}")),
                "not recorded",
            );
            fact.source.clone_from(name);
            tasks.push(fact);
        }
    }
    attach_session(inputs, &mut tasks);
    let lanes = lane_issues(inputs, &now);
    let operator = operator_set(inputs, ids, &now);
    // An open task whose reported head was not read may carry issues
    // there that no source here shows.
    let heads_read = !inputs
        .tasks
        .iter()
        .any(|task| ids.contains(&task.id) && task.head_unread.is_some());
    let complete =
        operator.unchecked.is_empty() && heads_read && required_unread(inputs, Some(ids)).is_none();
    IssuesFact {
        tasks,
        lanes,
        operator,
        complete,
    }
}

#[allow(clippy::too_many_lines)]
fn task_issues(inputs: &Inputs, plan: &Plan, task: &TaskIn, now: &str, out: &mut Vec<IssueFact>) {
    let state = task.state.as_deref();
    if task.status == "blocked" || state == Some("blocked") {
        let blocker = task.blocker.clone().unwrap_or_default();
        let owner = blocker.owner.clone();
        let kind = if owner
            .as_deref()
            .is_some_and(|o| o.to_ascii_lowercase().contains("operator"))
        {
            "operator"
        } else {
            "agent"
        };
        let mut fact = issue(
            &task.id,
            "blocked",
            blocker
                .reason
                .clone()
                .unwrap_or_else(|| task.state_reason.clone()),
            Known::Value(now.to_string()),
            Unblocker {
                kind,
                name: Known::or_word(owner, "not recorded"),
            },
        );
        fact.why = Known::or_word(blocker.reason.clone(), "not recorded");
        fact.recommended_action = Known::or_word(
            blocker.revisit.map(|r| format!("revisit: {r}")),
            "not recorded",
        );
        fact.alternatives = Known::or_word(
            task.follow_ups.clone().map(|text| vec![text]),
            "not recorded",
        );
        fact.command = Known::Value(format!(
            "cd {} && codeflow task status {} todo",
            shell_quote(&inputs.repo_path),
            task.id
        ));
        out.push(fact);
    }
    if state == Some("waiting") {
        let before: Vec<String> = task
            .depends_on
            .iter()
            .filter(|dep| inputs.tasks.iter().any(|t| &&t.id == dep && is_open(t)))
            .cloned()
            .collect();
        let mut fact = issue(
            &task.id,
            "waiting",
            format!(
                "waits on {}",
                if before.is_empty() {
                    task.state_reason.clone()
                } else {
                    before.join(", ")
                }
            ),
            Known::Value(now.to_string()),
            Unblocker {
                kind: "predecessor",
                name: Known::or_word(before.first().cloned(), "not recorded"),
            },
        );
        if let Some(window) = plan.finish(before.iter()) {
            if matches!(window, Win::Estimable { .. }) {
                fact.clears = Known::Value(plan.fact(&window));
            }
        }
        out.push(fact);
    }
    if let (Some(pull), ForgeIn::Read { read_at, .. }) = (pull_for(inputs, task), &inputs.forge) {
        // Only the required set decides these; an optional run never
        // stands in for a required one.
        let kind = match pull.required_checks.as_str() {
            "failed" => Some("required check red"),
            "missing" => Some("required check missing"),
            "pending" => Some("required check pending"),
            "skipped" => Some("required check skipped without reuse evidence"),
            "unverified" => Some("required checks not read"),
            _ => None,
        };
        if let Some(kind) = kind {
            let detail = match (&pull.required, &pull.producers_unread) {
                (Ok(checks), None) => format!(
                    "pull request #{} required checks ({}) read as {}",
                    pull.number,
                    checks
                        .iter()
                        .map(RequiredCheckIn::label)
                        .collect::<Vec<_>>()
                        .join(", "),
                    pull.required_checks
                ),
                (Ok(_), Some(reason)) => format!(
                    "pull request #{}: the runs of the apps its required checks name could not be read: {reason}",
                    pull.number
                ),
                (Err(reason), _) => format!(
                    "pull request #{}: which checks are required could not be read: {reason}",
                    pull.number
                ),
            };
            let mut fact = issue(
                &task.id,
                kind,
                detail,
                Known::Value(time_text(*read_at)),
                Unblocker {
                    kind: "ci run",
                    name: Known::Value(format!("pull request #{}", pull.number)),
                },
            );
            // Judged from the runs and the required set together, so its
            // source is checked only when both were read.
            fact.source = REQUIRED_SOURCE.into();
            fact.command = Known::or_word(
                gh_repo(inputs).map(|repo| format!("gh pr checks {} --repo {repo}", pull.number)),
                "none",
            );
            out.push(fact);
        }
    }
    if state == Some("active") {
        if let Some(branch) = &task.branch {
            if branch.commits_ahead == 0 && inputs.now - branch.head_time > inputs.sweep_seconds {
                let mut fact = issue(
                    &task.id,
                    "stale claim",
                    format!(
                        "claimed on {} with no commit of its own for {}, past the sweep interval",
                        branch.name,
                        super::compute::duration_words(inputs.now - branch.head_time)
                    ),
                    Known::Value(now.to_string()),
                    Unblocker {
                        kind: "lane",
                        name: Known::or_word(plan.lane_of(&task.id), "not recorded"),
                    },
                );
                fact.command = Known::Value(format!(
                    "git -C {} log -1 {}",
                    shell_quote(&inputs.repo_path),
                    shell_quote(&branch.name)
                ));
                out.push(fact);
            }
        }
    }
}

/// A session fact's recommendation fills an issue's action, labelled
/// `session`; it never replaces one the record holds.
fn attach_session(inputs: &Inputs, issues: &mut [IssueFact]) {
    let Some(session) = &inputs.session else {
        return;
    };
    for fact in &session.facts {
        let (Some(task), Some(rec)) = (&fact.task, &fact.recommendation) else {
            continue;
        };
        for issue in issues.iter_mut().filter(|issue| &issue.subject == task) {
            if issue.recommended_action.value().is_none() {
                issue.recommended_action = Known::Value(rec.action.clone());
                issue.why = Known::Value(rec.why.clone());
                issue.alternatives = Known::Value(rec.alternatives.clone());
                issue.source = "session".into();
            }
        }
    }
}

fn lane_issues(inputs: &Inputs, now: &str) -> Vec<IssueFact> {
    let mut out = Vec::new();
    if let LedgerIn::Read { lanes, .. } = &inputs.ledger {
        for lane in lanes {
            let quiet = lane
                .reported_at
                .is_none_or(|at| inputs.now - at > inputs.sweep_seconds);
            if quiet {
                let mut fact = issue(
                    &lane.id,
                    "seat with no progress",
                    lane.reported_at.map_or_else(
                        || "the lane has never reported".into(),
                        |at| format!("last report {}, past the sweep interval", time_text(at)),
                    ),
                    Known::Value(now.to_string()),
                    Unblocker {
                        kind: "lane",
                        name: Known::Value(lane.id.clone()),
                    },
                );
                fact.source = "ledger".into();
                out.push(fact);
            }
        }
    }
    if let Some(session) = &inputs.session {
        for fact in session.facts.iter().filter(|fact| fact.no_progress) {
            let subject = fact
                .lane
                .clone()
                .or_else(|| fact.task.clone())
                .unwrap_or_default();
            let mut item = issue(
                &subject,
                "seat with no progress",
                format!("the session saw no progress at {}", fact.observed_at),
                Known::Value(fact.observed_at.clone()),
                Unblocker {
                    kind: "lane",
                    name: Known::or_word(fact.seat.clone(), "not recorded"),
                },
            );
            item.source = "session".into();
            out.push(item);
        }
    }
    out
}

fn decision(
    context: Option<String>,
    options: Option<Vec<String>>,
    recommendation: Option<String>,
    reasoning: Option<String>,
) -> DecisionFact {
    DecisionFact {
        context: Known::or_word(context, "not recorded"),
        options: Known::or_word(options.filter(|o| !o.is_empty()), "not recorded"),
        recommendation: Known::or_word(recommendation, "not recorded"),
        reasoning: Known::or_word(reasoning, "not recorded"),
    }
}

#[allow(clippy::too_many_lines)]
fn operator_set(inputs: &Inputs, ids: &BTreeSet<String>, now: &str) -> OperatorSet {
    let mut items = Vec::new();
    let mut sources = Vec::new();
    let check = |source: &str, problem: Option<String>| SourceCheck {
        source: source.into(),
        checked: problem.is_none(),
        reason: Known::or_word(problem, "none"),
    };
    sources.push(check("records", records_problem(inputs)));
    for task in inputs
        .tasks
        .iter()
        .filter(|task| ids.contains(&task.id) && is_open(task))
    {
        let Some(blocker) = &task.blocker else {
            continue;
        };
        if blocker
            .owner
            .as_deref()
            .is_some_and(|o| o.to_ascii_lowercase().contains("operator"))
        {
            items.push(OperatorItemFact {
                id: task.id.clone(),
                source: "records".into(),
                summary: task.title.clone(),
                awaiting: Known::or_word(blocker.reason.clone(), "not recorded"),
                decision: decision(blocker.reason.clone(), None, blocker.revisit.clone(), None),
                last_check: now.to_string(),
            });
        }
    }
    match &inputs.ledger {
        LedgerIn::Absent => sources.push(check("ledger", None)),
        LedgerIn::Unreadable(reason) => sources.push(check("ledger", Some(reason.clone()))),
        LedgerIn::Read { operator_items, .. } => {
            sources.push(check("ledger", None));
            for item in operator_items {
                items.push(OperatorItemFact {
                    id: item.id.clone(),
                    source: "ledger".into(),
                    summary: item.title.clone(),
                    awaiting: Known::Value(item.awaiting.clone()),
                    decision: decision(
                        Some(item.awaiting.clone()),
                        None,
                        None,
                        Some(item.done_when.clone()),
                    ),
                    last_check: now.to_string(),
                });
            }
        }
    }
    for (name, read) in &inputs.owed {
        match read {
            OwedRead::Absent => sources.push(check(name, None)),
            OwedRead::Unreadable(reason) => sources.push(check(name, Some(reason.clone()))),
            OwedRead::Items(owed) => {
                sources.push(check(name, None));
                for item in owed
                    .iter()
                    .filter(|item| item.kind == OwedKind::OperatorAction)
                {
                    let d = item.decision.clone().unwrap_or_default();
                    items.push(OperatorItemFact {
                        id: item.id.clone(),
                        source: name.clone(),
                        summary: item.summary.clone(),
                        awaiting: Known::or_word(item.event.clone(), "not recorded"),
                        decision: decision(
                            d.context,
                            Some(d.options),
                            d.recommendation,
                            d.reasoning,
                        ),
                        last_check: now.to_string(),
                    });
                }
            }
        }
    }
    sources.push(check(
        "forge",
        match &inputs.forge {
            ForgeIn::Read { .. } => None,
            ForgeIn::Unverified { reason } => Some(reason.clone()),
        },
    ));
    if let Some(session) = &inputs.session {
        sources.push(check("session", None));
        for (index, fact) in session.facts.iter().enumerate() {
            let Some(d) = &fact.decision else { continue };
            items.push(OperatorItemFact {
                id: format!("session-{}", index + 1),
                source: "session".into(),
                summary: d.context.clone(),
                awaiting: Known::Value("a decision".into()),
                decision: decision(
                    Some(d.context.clone()),
                    Some(d.options.clone()),
                    Some(d.recommendation.clone()),
                    Some(d.reasoning.clone()),
                ),
                last_check: fact.observed_at.clone(),
            });
        }
    }
    let unchecked: Vec<String> = sources
        .iter()
        .filter(|s| !s.checked)
        .map(|s| s.source.clone())
        .collect();
    let state = if !items.is_empty() {
        "owed"
    } else if unchecked.is_empty() {
        "none owed"
    } else {
        "not established"
    };
    OperatorSet {
        state,
        items,
        sources,
        unchecked,
    }
}

/// Why the records source was not fully read, when it was not: an
/// unreadable record, or a target whose records could not be listed or
/// read.
fn records_problem(inputs: &Inputs) -> Option<String> {
    let mut problems: Vec<String> = Vec::new();
    let unreadable: Vec<&str> = inputs.unreadable.iter().map(|r| r.path.as_str()).collect();
    if !unreadable.is_empty() {
        problems.push(format!("unreadable: {}", unreadable.join(", ")));
    }
    problems.extend(
        inputs
            .record_losses
            .iter()
            .map(|(_, reason)| reason.clone()),
    );
    // A task the records or the readiness core could not judge.
    problems.extend(inputs.tasks.iter().filter_map(|task| {
        task.unknown
            .as_deref()
            .filter(|reason| {
                !reason.starts_with(super::gather::FORGE_UNREAD)
                    && !reason.starts_with(super::gather::HEAD_UNREAD)
            })
            .map(|reason| format!("{}: {reason}", task.id))
    }));
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// The lanes view.
#[must_use]
pub fn lanes(inputs: &Inputs, ids: &BTreeSet<String>, _plan: &Plan) -> LanesFact {
    let percent_of = |scope: &[String]| {
        let stages: Vec<Stage> = inputs
            .tasks
            .iter()
            .filter(|task| scope.contains(&task.id))
            .map(|task| stage(inputs, task))
            .collect();
        if stages.contains(&Stage::Unknown) {
            return None;
        }
        let percents: Vec<u32> = stages.into_iter().filter_map(Stage::percent).collect();
        mean(&percents)
    };
    let mut lanes = Vec::new();
    let mut source = "observed";
    if let LedgerIn::Read { lanes: ledger, .. } = &inputs.ledger {
        if !ledger.is_empty() {
            source = "ledger";
            for lane in ledger {
                lanes.push(LaneFact {
                    id: lane.id.clone(),
                    label: "ledger",
                    stream: Known::Value(lane.stream.clone()),
                    tasks: lane.scope.clone(),
                    attempt: Known::Value(lane.attempt),
                    seat: Known::or_word(lane.seat.clone(), "not recorded"),
                    worktree: Known::not_recorded(),
                    branch: Known::not_recorded(),
                    pull_request: Known::not_recorded(),
                    head_age_seconds: Known::not_recorded(),
                    percent: Known::or_word(percent_of(&lane.scope), "unknown"),
                });
            }
        }
    }
    if source == "observed" {
        for task in inputs
            .tasks
            .iter()
            .filter(|task| task.state.as_deref() == Some("active"))
        {
            let scope = vec![task.id.clone()];
            lanes.push(LaneFact {
                id: format!("observed {}", task.id),
                label: "observed",
                stream: Known::not_recorded(),
                tasks: scope.clone(),
                attempt: Known::not_recorded(),
                seat: Known::not_recorded(),
                worktree: Known::or_word(
                    task.branch.as_ref().and_then(|b| b.worktree.clone()),
                    "none",
                ),
                branch: Known::or_word(task.branch.as_ref().map(|b| b.name.clone()), "none"),
                pull_request: Known::or_word(super::epics::open_pull(inputs, task), "none"),
                head_age_seconds: Known::or_word(
                    task.branch.as_ref().map(|b| inputs.now - b.head_time),
                    "unknown",
                ),
                percent: Known::or_word(percent_of(&scope), "unknown"),
            });
        }
    }
    let carried: Vec<String> = lanes
        .iter()
        .flat_map(|lane| lane.tasks.clone())
        .filter(|id| ids.contains(id) || source == "observed")
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let session = inputs.session.as_ref().map(|session| {
        session
            .facts
            .iter()
            .filter(|fact| fact.lane.is_some() || fact.task.is_some())
            .map(|fact| SessionLaneFact {
                label: "session",
                lane: Known::or_word(fact.lane.clone(), "none"),
                task: Known::or_word(fact.task.clone(), "none"),
                builder: Known::or_word(fact.builder.clone(), "not recorded"),
                reviewer: Known::or_word(fact.reviewer.clone(), "not recorded"),
                seat: Known::or_word(fact.seat.clone(), "not recorded"),
                verdict_in_flight: Known::or_word(fact.verdict_in_flight.clone(), "none"),
                blocker: Known::or_word(fact.blocker.clone(), "none"),
                observed_at: fact.observed_at.clone(),
            })
            .collect()
    });
    let note = if source == "observed" && inputs.session.is_none() {
        Known::Value("lanes: observed only".into())
    } else {
        word("none")
    };
    LanesFact {
        source,
        note,
        lanes,
        session: Known::or_word(session, "no session file given"),
        overall_percent: Known::or_word(percent_of(&carried), "unknown"),
    }
}

/// The last seven UTC dates ending at `now`, oldest first.
pub(super) fn last_days(now: i64) -> Vec<String> {
    (0..7)
        .rev()
        .map(|back| time_text(now - back * 86_400)[..10].to_string())
        .collect()
}

/// Pace and intake over the last seven days.
#[must_use]
pub fn pace(inputs: &Inputs, ids: &BTreeSet<String>, plan: &Plan) -> PaceFact {
    let scope: Vec<&TaskIn> = inputs
        .tasks
        .iter()
        .filter(|task| ids.contains(&task.id) && task.status != "cancelled")
        .collect();
    let lanes = plan.lanes_used.max(1);
    let planned = count_f64(lanes) * 24.0;
    // Each measure needs its own coverage: landings the history walk of
    // every home; additions every task's creation date; open counts both,
    // and every task's openness.
    let known = inputs.history_read;
    let dated = scope.iter().all(|task| task.created.is_some());
    let added_known = known && dated;
    let open_known = added_known && super::plan::membership_unknown(inputs, ids).is_none();
    let days = last_days(inputs.now);
    let day_of = |at: i64| time_text(at)[..10].to_string();
    let landed_on = |date: &str| -> Vec<String> {
        scope
            .iter()
            .filter(|t| t.landed_at.is_some_and(|at| day_of(at) == date))
            .map(|t| t.id.clone())
            .collect()
    };
    let added_on = |date: &str| -> Vec<String> {
        scope
            .iter()
            .filter(|t| t.created.as_deref() == Some(date))
            .map(|t| t.id.clone())
            .collect()
    };
    let open_at = |date: &str| {
        scope
            .iter()
            .filter(|t| t.created.as_deref().is_none_or(|c| c <= date))
            .filter(|t| {
                let landed = t.status == "complete" || t.state.as_deref() == Some("landed");
                !landed || t.landed_at.is_some_and(|at| day_of(at).as_str() > date)
            })
            .count()
    };
    let per_day: Vec<PaceDay> = days
        .iter()
        .map(|date| PaceDay {
            date: date.clone(),
            landed: if known {
                Known::Value(landed_on(date))
            } else {
                Known::unknown()
            },
            added: if added_known {
                Known::Value(added_on(date))
            } else {
                Known::unknown()
            },
            open_at_end: if open_known {
                Known::Value(open_at(date))
            } else {
                Known::unknown()
            },
        })
        .collect();
    let landed_week: usize = days.iter().map(|d| landed_on(d).len()).sum();
    let added_week: usize = days.iter().map(|d| added_on(d).len()).sum();
    PaceFact {
        days: per_day,
        landed_total: if known { Known::Value(landed_week) } else { Known::unknown() },
        added_total: if added_known { Known::Value(added_week) } else { Known::unknown() },
        lanes_observed: plan.lanes_used,
        observed_landing_rate_per_day: if known { Known::Value(count_f64(landed_week) / 7.0) } else { Known::unknown() },
        planned_capacity_rate_per_day: planned,
        intake_rate_per_day: if added_known { Known::Value(count_f64(added_week) / 7.0) } else { Known::unknown() },
        basis: "landings from the home target's first-parent history, additions from record creation dates; the planned capacity is one task per lane-hour over the lanes used and is never lowered to the observed pace".into(),
    }
}

/// Why the required checks of an open pull request in scope (`None`: any)
/// could not be read; while one is unread, a missing required check cannot
/// be found and the issues view is not complete.
fn required_unread(inputs: &Inputs, ids: Option<&BTreeSet<String>>) -> Option<String> {
    if let ForgeIn::Unverified { reason } = &inputs.forge {
        return Some(format!("{}: {reason}", super::gather::FORGE_UNREAD));
    }
    let unread: Vec<String> = inputs
        .tasks
        .iter()
        .filter(|task| ids.is_none_or(|ids| ids.contains(&task.id)) && is_open(task))
        .filter_map(|task| {
            let pull = pull_for(inputs, task)?;
            pull.required
                .as_ref()
                .err()
                .or(pull.producers_unread.as_ref())
                .map(|reason| format!("pull request #{}: {reason}", pull.number))
        })
        .collect();
    (!unread.is_empty()).then(|| unread.join("; "))
}

fn evidence_entry(
    measure: &'static str,
    source: &str,
    checked: bool,
    at: &str,
    note: Option<String>,
) -> EvidenceFact {
    EvidenceFact {
        measure,
        source: source.into(),
        checked,
        checked_at: if checked {
            Known::Value(at.into())
        } else {
            Known::Word("not checked".into())
        },
        note: Known::or_word(note, "none"),
    }
}

/// One evidence entry per measure and source.
#[must_use]
pub fn evidence(inputs: &Inputs, plan: &Plan) -> Vec<EvidenceFact> {
    let at = time_text(inputs.now);
    let records = records_problem(inputs);
    let records_ok = records.is_none();
    let readiness_ok = inputs.readiness_problem.is_none();
    let forge = match &inputs.forge {
        ForgeIn::Read { read_at, .. } => (true, time_text(*read_at), None),
        ForgeIn::Unverified { reason } => (false, at.clone(), Some(reason.clone())),
    };
    let refs_ok = inputs
        .fetched_at
        .is_some_and(|f| inputs.now - f <= inputs.sweep_seconds);
    let ledger = match &inputs.ledger {
        LedgerIn::Absent => (true, Some("no ledger in this clone".to_string())),
        LedgerIn::Unreadable(reason) => (false, Some(reason.clone())),
        LedgerIn::Read { .. } => (true, None),
    };
    let mut out = vec![
        evidence_entry("percents", "records", records_ok, &at, records),
        {
            let heads: Vec<String> = inputs
                .tasks
                .iter()
                .filter_map(|task| {
                    task.head_unread
                        .as_ref()
                        .map(|reason| format!("{}: {reason}", task.id))
                })
                .collect();
            let note = (!heads.is_empty()).then(|| heads.join("; "));
            evidence_entry("percents", "git", note.is_none(), &at, note)
        },
        evidence_entry(
            "locations",
            "git refs",
            refs_ok,
            &at,
            (!refs_ok).then(|| "the refs are unfetched or older than the sweep interval".into()),
        ),
        evidence_entry("locations", "forge", forge.0, &forge.1, forge.2.clone()),
        evidence_entry(
            "order",
            "records through the readiness core",
            readiness_ok,
            &at,
            inputs.readiness_problem.clone(),
        ),
        evidence_entry("lanes", "ledger", ledger.0, &at, ledger.1.clone()),
        evidence_entry(
            "windows",
            if plan.basis_kind == "forecast" {
                "forecast record"
            } else {
                "planned throughput rule"
            },
            true,
            &at,
            inputs.forecast_note.clone(),
        ),
        evidence_entry("issues", "forge", forge.0, &forge.1, forge.2),
        {
            let unread = required_unread(inputs, None);
            evidence_entry("issues", REQUIRED_SOURCE, unread.is_none(), &at, unread)
        },
        evidence_entry("issues", "ledger", ledger.0, &at, ledger.1),
        evidence_entry(
            "pace",
            "git history",
            inputs.history_read,
            &at,
            inputs.history_note.clone(),
        ),
        {
            // Additions and open counts need every task's creation date.
            let undated: Vec<&str> = inputs
                .tasks
                .iter()
                .filter(|task| task.created.is_none() && task.status != "cancelled")
                .map(|task| task.id.as_str())
                .collect();
            let note = (!undated.is_empty())
                .then(|| format!("no creation date on {}", undated.join(", ")));
            evidence_entry("pace", "record creation dates", note.is_none(), &at, note)
        },
    ];
    for (name, read) in &inputs.owed {
        let problem = match read {
            OwedRead::Unreadable(reason) => Some(reason.clone()),
            _ => None,
        };
        out.push(evidence_entry(
            "issues",
            name,
            problem.is_none(),
            &at,
            problem,
        ));
    }
    if inputs.session.is_some() {
        out.push(evidence_entry("lanes", "session file", true, &at, None));
    }
    out
}
