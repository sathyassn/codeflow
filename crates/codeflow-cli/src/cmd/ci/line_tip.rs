//! A landing candidate judged as an epic line's next tip (TSK-1155, issue
//! #489). The pre-push hook refuses a push that adds a commit made directly
//! on an `integration/EPC-*` line (`work.epic_line`); this runs the same
//! `line_adoption::check_range` in `codeflow ci`, so a candidate holding
//! such a commit is refused before its gate instead of at push.
//!
//! Which runs it judges comes from the run's context, never from the head's
//! branch name. A pull request head lands by a merge commit whose first
//! parent is the line tip, so its own commits never sit on the line's
//! first-parent history: a pull request body or event, or `--into`, means
//! the head lands by merge. A push is judged by the hook itself.
//!
//! `--line-tip <LINE>` claims the head is that line's next tip: the landing
//! path's explicit claim. Without the claim, a run whose base is the fetched
//! tip of exactly one epic line is judged the same way: such a run cannot
//! tell a landing candidate from a change that lands by merge, so a head
//! that would add a direct commit refuses with a remedy naming each route.
//! A base that is also the default target's fetched tip, as a new line's
//! is, is no less ambiguous: no fact the requester cannot forge says which
//! branch the head lands on, so the remedy also names `--into` for the
//! target. Every finding blocks, and a line whose authority cannot be
//! established refuses (charter D19). Authority comes only from the line's
//! fetched tip, resolved once.
use std::fmt::Write as _;
use std::path::Path;

use codeflow_core::hooks::Violation;
use codeflow_core::workgraph::{
    default_work_target_ref, durable_work_tracking_enabled_at, epic_line_bound_at, line_adoption,
    EpicLineBinding,
};

const RULE: &str = "work.epic_line";

/// What the run says about how its head reaches a line.
pub(super) struct Run<'a> {
    /// `--line-tip`: the head is this line's next tip.
    pub line_tip: Option<&'a str>,
    /// The resolved base commit.
    pub base: Option<&'a str>,
    pub head: &'a str,
    /// The head lands by a merge commit: a pull request body or event, or
    /// `--into` outside a push.
    pub merge_landing: bool,
    /// A push hand-off from the pre-push hook.
    pub pushed: bool,
    /// The push of a branch named by the judging policy's
    /// `git.candidate_prefix` (TSK-1154 AC-15). The name only selects this
    /// judgment, which grants a member nothing its own pull request would
    /// not get; the line and its tip still come from the fetched line ref.
    pub candidate_push: bool,
}

/// A landing candidate's line, once its authority is established: the bound
/// epic line and its fetched tip, resolved once. The acceptance check judges
/// the candidate's members against this tip (TSK-1154), never against a
/// supplied base, a local branch or anything the head names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Landing {
    pub line: String,
    pub tip: String,
}

/// Where the judged line came from, for the output and the remedy. A line
/// found from the base carries the default target's branch name when the
/// base is that target's fetched tip too.
#[derive(Clone, PartialEq, Eq)]
enum Source {
    Flag,
    Base {
        target: Option<String>,
    },
    /// A pushed landing candidate whose first-parent history reaches the
    /// line's fetched tip.
    Pushed,
}

/// Judge the head as a line's next tip when the run's context says it is
/// one, or cannot say it is not; push a blocking `work.epic_line` finding
/// for each refusal. Returns the line and its fetched tip when the line's
/// authority was established, so the acceptance check can judge the
/// candidate member by member; `None` otherwise.
pub(super) fn check(
    root: &Path,
    run: &Run<'_>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) -> Option<Landing> {
    let (line, source) = match destination(root, run) {
        Ok(Some(found)) => found,
        Ok(None) => return None,
        Err((error, source)) => {
            ran.push("line-tip");
            refuse(tagged, &error, None, &source);
            return None;
        }
    };
    ran.push("line-tip");
    let how = match &source {
        Source::Flag => "--line-tip".to_string(),
        Source::Pushed => {
            "a pushed landing candidate whose first-parent history reaches its fetched tip"
                .to_string()
        }
        Source::Base { target: None } => "the base is its fetched tip".to_string(),
        Source::Base {
            target: Some(target),
        } => format!(
            "the base is its fetched tip, and the default target '{}' is there too",
            codeflow_core::git::display_text(target)
        ),
    };
    println!(
        "codeflow ci: line tip: the head is judged as the next tip of '{}' ({how})",
        codeflow_core::git::display_text(&line)
    );
    match judge(root, &line, &source, run.head, tagged) {
        Ok(tip) => tip.map(|tip| Landing { line, tip }),
        Err(error) => {
            refuse(tagged, &error, Some(&line), &source);
            None
        }
    }
}

/// The line the head would become the tip of, or `None` when the run's
/// context says it is not a line's next tip.
fn destination(root: &Path, run: &Run<'_>) -> Result<Option<(String, Source)>, (String, Source)> {
    if let Some(named) = run.line_tip {
        let line = named
            .strip_prefix("refs/heads/")
            .or_else(|| named.strip_prefix("refs/remotes/origin/"))
            .or_else(|| named.strip_prefix("origin/"))
            .unwrap_or(named);
        line_adoption::epic_id(line)
            .map_err(|reason| (format!("--line-tip {reason}"), Source::Flag))?;
        return Ok(Some((line.to_string(), Source::Flag)));
    }
    if run.pushed && run.candidate_push && !run.merge_landing {
        return pushed_line(root, run.head).map_err(|error| (error, Source::Pushed));
    }
    if run.pushed || run.merge_landing {
        return Ok(None);
    }
    let Some(base) = run.base else {
        return Ok(None);
    };
    // A head that is the base adds no commit, so however it lands no line
    // receives one; the two commit ids decide this, not a name.
    if run.head == base {
        println!("codeflow ci: line tip: none (the head is the base and adds no commit)");
        return Ok(None);
    }
    let unknown = Source::Base { target: None };
    let target = default_target_at(root, base).map_err(|error| (error, unknown.clone()))?;
    let source = Source::Base { target };
    let mut lines =
        line_adoption::fetched_lines_at(root, base).map_err(|error| (error, source.clone()))?;
    match lines.len() {
        0 => {
            println!("codeflow ci: line tip: none (the base is no epic line's fetched tip)");
            Ok(None)
        }
        1 => Ok(lines.pop().map(|line| (line, source))),
        _ => Err((
            format!(
                "the base is the fetched tip of several epic lines ({}), so the line the head would become the tip of is ambiguous",
                lines.join(", ")
            ),
            source,
        )),
    }
}

/// How many first-parent commits of a pushed candidate are searched for an
/// epic line's fetched tip. A candidate is that tip plus one merge per
/// member; a tip not found within this depth leaves the push judged as one
/// range, as before (the strict side).
const PUSHED_DEPTH: usize = 500;

/// The epic line whose fetched tip is on the pushed head's first-parent
/// history, nearest the head first; `None` when none is, and a refusal when
/// several lines share that tip.
fn pushed_line(root: &Path, head: &str) -> Result<Option<(String, Source)>, String> {
    let walked = line_adoption::first_parent_history(root, head, PUSHED_DEPTH)
        .map_err(|error| format!("cannot read the pushed candidate's history: {error}"))?;
    for commit in &walked {
        let mut lines = line_adoption::fetched_lines_at(root, commit)?;
        match lines.len() {
            0 => {}
            1 => return Ok(lines.pop().map(|line| (line, Source::Pushed))),
            _ => {
                return Err(format!(
                    "the pushed candidate's first-parent history reaches {}, the fetched tip of several epic lines ({}), so its line is ambiguous",
                    &commit[..commit.len().min(9)],
                    lines.join(", ")
                ))
            }
        }
    }
    println!("codeflow ci: line tip: none (the pushed candidate's first-parent history reaches no epic line's fetched tip)");
    Ok(None)
}

/// The default target's branch name when its fetched tip is `base`. Only
/// the fetched ref counts, never a local branch.
fn default_target_at(root: &Path, base: &str) -> Result<Option<String>, String> {
    let Some(target) = default_work_target_ref(root)
        .map_err(|error| format!("the default target cannot be read: {error}"))?
    else {
        return Ok(None);
    };
    let Some(name) = target.strip_prefix("refs/remotes/origin/") else {
        return Ok(None);
    };
    let tip = line_adoption::reference_tip(root, &target)?;
    Ok((tip.as_deref() == Some(base)).then(|| name.to_string()))
}

/// Judge `head` as `line`'s next tip against the line's fetched tip, and
/// return that tip once the line is bound and the head is cut from it
/// (`None` when durable work tracking is off).
fn judge(
    root: &Path,
    line: &str,
    source: &Source,
    head: &str,
    tagged: &mut Vec<super::TaggedViolation>,
) -> Result<Option<String>, String> {
    let remote = format!("refs/remotes/origin/{line}");
    // The one trusted tip, resolved once: never a local branch, a supplied
    // base or anything the head names.
    let tip_text = line_adoption::fetched_tip(root, line)?.ok_or_else(|| {
        format!(
            "'{}' has no fetched tip ('origin/{}'), so the line's authority cannot be established",
            codeflow_core::git::display_text(line),
            codeflow_core::git::display_text(line)
        )
    })?;
    let target = default_work_target_ref(root)
        .map_err(|error| format!("the default target cannot be read: {error}"))?
        .ok_or("no main or master branch to judge the line against")?;
    // Tracking as the hook judges it, at the head or the target, plus the
    // line's own tip: deleting the records in the head cannot switch it off.
    // Any unreadable state refuses before the readable ones are combined, so
    // one tracked revision never hides another's error.
    let mut tracked = false;
    for revision in [head, tip_text.as_str(), target.as_str()] {
        tracked |= durable_work_tracking_enabled_at(root, revision).map_err(|error| {
            format!(
                "durable work tracking cannot be read at {}: {error}",
                &revision[..revision.len().min(9)]
            )
        })?;
    }
    if !tracked {
        println!("codeflow ci: line tip: durable work tracking is off, so no epic line is judged");
        return Ok(None);
    }
    let place = format!("'origin/{line}'");
    match epic_line_bound_at(root, line, &tip_text, &place).map_err(|error| error.to_string())? {
        EpicLineBinding::Bound => {}
        EpicLineBinding::Refused(reason) | EpicLineBinding::Unbound(reason) => {
            return Err(format!(
                "'{}' {reason}, so it is not a bound epic line",
                codeflow_core::git::display_text(line)
            ));
        }
    }
    // The judged range always starts at the fetched tip: a base placed
    // after a direct commit cannot hide it.
    if !line_adoption::descends_from(root, &tip_text, head)? {
        return Err(format!(
            "the head is not cut from the line's fetched tip {}, so it cannot become the line's next tip",
            &tip_text[..9]
        ));
    }
    let report = line_adoption::check_range(root, line, &target, &tip_text, head)?;
    if let Some(error) = &report.record_error {
        return Err(format!(
            "{}'s line_adoptions cannot be read at the head: {error}",
            line_adoption::epic_id(line).unwrap_or("the epic")
        ));
    }
    if report.unadopted.is_empty() {
        println!(
            "codeflow ci: line tip: no unadopted direct commit from {}",
            &tip_text[..9]
        );
        return Ok(Some(tip_text));
    }
    let (message, mut remedy) =
        line_adoption::direct_commit_refusal(line, &report.unadopted, &remote);
    if let Source::Base { target } = source {
        let _ = write!(remedy, "; {}", routes(line, target.as_deref()));
    }
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(RULE, message, &remedy),
    });
    Ok(Some(tip_text))
}

/// The remedy for a run that found the line from its base: it names the
/// role flag for each way the head can land.
fn routes(line: &str, target: Option<&str>) -> String {
    let shown = codeflow_core::git::display_text(line);
    let mut text = format!(
        "this run found the line from its base and cannot tell how the head reaches it: pass --line-tip {shown} on a landing candidate, or, for a pull request head that lands on the line by merge, --into {shown} or the pull request's body (--pr-body-file or CODEFLOW_PR_BODY)"
    );
    if let Some(target) = target {
        let target = codeflow_core::git::display_text(target);
        let _ = write!(
            text,
            "; the base is also the fetched tip of the default target, so for a change into it pass --into {target}"
        );
    }
    text
}

/// A run that cannot judge the line it was given or found refuses: CI is
/// the hard line (charter D19), so an unreadable, unbound or ambiguous line
/// is never skipped.
fn refuse(
    tagged: &mut Vec<super::TaggedViolation>,
    message: &str,
    line: Option<&str>,
    source: &Source,
) {
    let shown = line.map_or_else(|| "<LINE>".to_string(), codeflow_core::git::display_text);
    let remedy = match source {
        Source::Flag => format!(
            "fetch the line (`git fetch origin`) and pass --line-tip {shown} for a landing candidate cut from its fetched tip, or --into {shown} for a pull request head that lands by merge"
        ),
        Source::Pushed => format!(
            "fetch the line (`git fetch origin`) and rebuild the candidate from the fetched tip of {shown}, with only no-ff merges of reviewed heads"
        ),
        Source::Base { target } => format!(
            "fetch the lines (`git fetch origin`); {}",
            routes(line.unwrap_or("<LINE>"), target.as_deref())
        ),
    };
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(
            RULE,
            format!("cannot judge the head as an epic line's next tip: {message}"),
            &remedy,
        ),
    });
}
