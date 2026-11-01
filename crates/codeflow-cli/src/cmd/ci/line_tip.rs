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
//! path's explicit claim. Every finding then blocks, and a line whose
//! authority cannot be established refuses (charter D19). Without the claim,
//! a run whose base is the fetched tip of exactly one epic line is judged
//! the same way, but its findings print as warnings: such a run cannot tell
//! a landing candidate from a change that lands by merge, and the generic CI
//! template (`ci-generic.sh`) checks a task change that way, with the line
//! tip as its base and no pull request signal. Authority comes only from the
//! line's fetched tip, resolved once.
use std::fmt::Write as _;
use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
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
}

/// Where the judged line came from, for the output and the remedy.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Flag,
    Base,
}

/// Judge the head as a line's next tip when the run's context says it is
/// one; push a `work.epic_line` finding for each refusal, blocking for
/// `--line-tip` and a warning for a line found from the base.
pub(super) fn check(
    root: &Path,
    run: &Run<'_>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let (line, source) = match destination(root, run) {
        Ok(Some(found)) => found,
        Ok(None) => return,
        Err((error, source)) => {
            ran.push("line-tip");
            return refuse(tagged, &error, None, source);
        }
    };
    ran.push("line-tip");
    let how = match source {
        Source::Flag => "--line-tip",
        Source::Base => "the base is its fetched tip",
    };
    println!(
        "codeflow ci: line tip: the head is judged as the next tip of '{}' ({how})",
        codeflow_core::git::display_text(&line)
    );
    if let Err(error) = judge(root, &line, source, run.head, tagged) {
        refuse(tagged, &error, Some(&line), source);
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
    if run.pushed || run.merge_landing {
        return Ok(None);
    }
    let Some(base) = run.base else {
        return Ok(None);
    };
    let mut lines =
        line_adoption::fetched_lines_at(root, base).map_err(|error| (error, Source::Base))?;
    match lines.len() {
        0 => {
            println!("codeflow ci: line tip: none (the base is no epic line's fetched tip)");
            Ok(None)
        }
        1 => Ok(lines.pop().map(|line| (line, Source::Base))),
        _ => Err((
            format!(
                "the base is the fetched tip of several epic lines ({}), so the line the head would become the tip of is ambiguous",
                lines.join(", ")
            ),
            Source::Base,
        )),
    }
}

/// Judge `head` as `line`'s next tip against the line's fetched tip.
fn judge(
    root: &Path,
    line: &str,
    source: Source,
    head: &str,
    tagged: &mut Vec<super::TaggedViolation>,
) -> Result<(), String> {
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
    let tracking = [head, tip_text.as_str(), target.as_str()]
        .map(|revision| durable_work_tracking_enabled_at(root, revision));
    if !tracking.iter().any(|state| state == &Ok(true)) {
        if let Some(Err(error)) = tracking.iter().find(|state| state.is_err()) {
            return Err(format!("durable work tracking cannot be read: {error}"));
        }
        println!("codeflow ci: line tip: durable work tracking is off, so no epic line is judged");
        return Ok(());
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
        return Ok(());
    }
    let (message, mut remedy) =
        line_adoption::direct_commit_refusal(line, &report.unadopted, &remote);
    if source == Source::Base {
        let shown = codeflow_core::git::display_text(line);
        let _ = write!(
            remedy,
            "; this run found the line from its base, so this is a warning: pass --line-tip {shown} on a landing candidate to block, or --into {shown} for a pull request head that lands by merge"
        );
    }
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: finding(message, &remedy, source),
    });
    Ok(())
}

/// The finding at the level its source gives: `--line-tip` blocks, a line
/// found from the base warns. The level is fixed either way.
fn finding(message: String, remedy: &str, source: Source) -> Violation {
    let mut violation = Violation::always_blocking(RULE, message, remedy);
    if source == Source::Base {
        violation.level = PolicyLevel::Warn;
    }
    violation
}

/// A run that cannot judge the line it was given refuses: CI is the hard
/// line (charter D19), so an unreadable or unbound line is never skipped.
/// A line found from the base reports the same cause as a warning.
fn refuse(
    tagged: &mut Vec<super::TaggedViolation>,
    message: &str,
    line: Option<&str>,
    source: Source,
) {
    let shown = line.map_or_else(|| "<LINE>".to_string(), codeflow_core::git::display_text);
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: finding(
            format!("cannot judge the head as an epic line's next tip: {message}"),
            &format!(
                "fetch the line (`git fetch origin`) and pass --line-tip {shown} for a landing candidate cut from its fetched tip, or --into {shown} for a pull request head that lands by merge"
            ),
            source,
        ),
    });
}
