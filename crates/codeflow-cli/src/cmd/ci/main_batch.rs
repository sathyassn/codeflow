//! The filing members of a batch into the default target (TSK-1154
//! AC-16). The acceptance check admits a filing member, one that adds no
//! task completion and only adds new record files; here each is judged
//! over its own three-dot range, from the merge base of its member merge's
//! two parents to its head, by the checks a one-pull-request filing into
//! that target gets, with the run's policy and binary (the base's): the
//! workflow change notice, path policy with the size and ignored-file
//! rules, the record transitions and the id registry's merge rule. Each
//! finding names the member. Classification admission
//! (`amendment::range_problem`) is part of the admission itself, and the
//! workgraph validation runs once on the batch head, which holds every
//! filing's records.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};

use super::judging::Authority;

/// What judges a filing, as the run resolved it: its policy, its
/// authority commit and the commit whose baseline list governs records.
pub(super) struct Judged<'a> {
    pub configured: &'a GitPolicy,
    pub authority: Option<&'a Authority>,
    pub baseline: Option<&'a str>,
}

/// Judge each filing member of the batch `landing` at `head`.
pub(super) fn filings(
    root: &Path,
    landing: &super::line_tip::Landing,
    head: &str,
    judged: &Judged<'_>,
    target_tip: &Result<String, super::NoTarget>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let members = match codeflow_core::workgraph::acceptance::candidate::filings(
        root,
        &landing.tip,
        head,
        &landing.line,
    ) {
        Ok(members) => members,
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: Violation::always_blocking(
                    codeflow_core::workgraph::acceptance::BINDING_RULE,
                    format!("cannot list the batch's filing members: {error}"),
                    "fetch the full history of the batch and its default target, then rerun `codeflow ci`",
                ),
            });
            return;
        }
    };
    ran.push("main-batch-filings");
    for member in members {
        let (base, tip) = (member.onto.to_string(), member.head.to_string());
        println!(
            "codeflow ci: main batch: filing member {tip:.9}, merged by {:.9}: judged over {base:.9}...{tip:.9} as a filing pull request into '{}'",
            member.merge,
            codeflow_core::git::display_text(&landing.line)
        );
        let mut found = Vec::new();
        let mut checks = Vec::new();
        super::workflow_change::dispatch(root, Some(&base), &tip, &mut found, &mut checks);
        paths(root, judged, &base, &tip, &mut found);
        let bases = [base.clone()];
        super::work_records::dispatch(
            root,
            &bases,
            &tip,
            judged.baseline,
            false,
            None,
            target_tip,
            &mut found,
            &mut checks,
        );
        super::id_registry::dispatch(root, &bases, &tip, &mut found, &mut checks);
        let named = format!(
            "member {tip:.9} (a filing of new records), merged by {:.9}",
            member.merge
        );
        tagged.extend(found.into_iter().map(|mut each| {
            each.violation.message = format!("{named}: {}", each.violation.message);
            each
        }));
    }
}

/// Path policy with the size and ignored-file rules over one filing's
/// range, as the run applies them to the whole range.
fn paths(
    root: &Path,
    judged: &Judged<'_>,
    base: &str,
    tip: &str,
    found: &mut Vec<super::TaggedViolation>,
) {
    let report = codeflow_core::hooks::path_policy::PathPolicy::from_git(judged.configured)
        .and_then(|policy| {
            let content = super::judging::content_rules(root, judged.authority)?;
            codeflow_core::hooks::path_policy::range_with(
                root,
                Some(base),
                tip,
                &policy,
                Some(&content),
            )
        });
    match report {
        Ok(report) => {
            found.extend(
                report
                    .violations
                    .into_iter()
                    .map(|violation| super::TaggedViolation {
                        sha: None,
                        violation,
                    }),
            );
        }
        Err(error) => found.push(super::TaggedViolation {
            sha: None,
            violation: Violation::always_blocking(
                "git.path_policy",
                format!("cannot verify introduced paths: {error}"),
                "repair the named policy source or Git object, then retry",
            ),
        }),
    }
}
