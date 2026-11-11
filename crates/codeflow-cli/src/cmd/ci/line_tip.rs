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
}

/// Where the judged line came from, for the output and the remedy. A line
/// found from the base carries the default target's branch name when the
/// base is that target's fetched tip too.
#[derive(Clone, PartialEq, Eq)]
enum Source {
    Flag,
    Base { target: Option<String> },
}

/// Judge the head as a line's next tip when the run's context says it is
/// one, or cannot say it is not; push a blocking `work.epic_line` finding
/// for each refusal.
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
            return refuse(tagged, &error, None, &source);
        }
    };
    ran.push("line-tip");
    let how = match &source {
        Source::Flag => "--line-tip".to_string(),
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
    if let Err(error) = judge(root, &line, &source, run.head, tagged) {
        refuse(tagged, &error, Some(&line), &source);
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

/// Judge `head` as `line`'s next tip against the line's fetched tip.
fn judge(
    root: &Path,
    line: &str,
    source: &Source,
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
    if let Source::Base { target } = source {
        let _ = write!(remedy, "; {}", routes(Some(line), target.as_deref()));
    }
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(RULE, message, &remedy),
    });
    Ok(())
}

/// The remedy for a run that found the line from its base: it names the
/// role flag for each way the head can land.
fn routes(line: Option<&str>, target: Option<&str>) -> String {
    let (line_tip, into) = (flag("--line-tip", line), flag("--into", line));
    let mut text = format!(
        "this run found the line from its base and cannot tell how the head reaches it: pass {line_tip} on a landing candidate, or, for a pull request head that lands on the line by merge, {into} or the pull request's body (--pr-body-file or CODEFLOW_PR_BODY)"
    );
    if let Some(target) = target {
        let into = flag("--into", Some(target));
        let _ = write!(
            text,
            "; the base is also the fetched tip of the default target, so for a change into it pass {into}"
        );
    }
    text
}

/// A flag with its value as a reader passes it, rendered from the raw
/// value so it pastes back as that value (TSK-416 review round 2): `<LINE>`
/// when there is no value, and the flag with the value typed by hand when
/// the value holds a character a terminal does not show as itself.
fn flag(name: &'static str, value: Option<&str>) -> String {
    match value {
        None => format!("{name} <LINE>"),
        Some(value) => codeflow_core::git::pasted::option(name, value).unwrap_or_else(|| {
            format!(
                "{name} with the line's name typed by hand, since it holds a character a terminal does not show as itself"
            )
        }),
    }
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
    let remedy = match source {
        Source::Flag => format!(
            "fetch the line (`git fetch origin`) and pass {} for a landing candidate cut from its fetched tip, or {} for a pull request head that lands by merge",
            flag("--line-tip", line),
            flag("--into", line)
        ),
        Source::Base { target } => format!(
            "fetch the lines (`git fetch origin`); {}",
            routes(line, target.as_deref())
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

#[cfg(test)]
mod tests {
    use super::flag;

    /// TSK-416 review round 2: a line name reaches the printed role flag
    /// as one shell word from its raw value; a name starting with `-`
    /// stays the flag's value; a name a terminal cannot show as itself is
    /// not printed to paste.
    #[test]
    fn a_role_flag_renders_the_raw_line_name() {
        assert_eq!(
            flag("--line-tip", Some("integration/EPC-001-x")),
            "--line-tip integration/EPC-001-x"
        );
        assert_eq!(flag("--into", Some("o$(id)")), "--into 'o$(id)'");
        assert_eq!(flag("--into", Some("-x")), "--into=-x");
        assert_eq!(flag("--into", None), "--into <LINE>");
        let hidden = flag("--into", Some("a\u{200b}b"));
        assert!(!hidden.contains('\u{200b}') && !hidden.contains("200b"), "{hidden}");
    }
}
