//! The merge rule and the uniqueness scan (TSK-101, SPC-013 R-2, R-14,
//! R-111): every record the range adds must be bound to its `uid` in the
//! fetched `codeflow/registry`, no existing `uid` may change, and no other
//! ref may hold a different record under the same id. Only projects with
//! durable work tracking run it. The enforcing copy runs from the target
//! branch's `codeflow-policy` workflow; this row gives the same verdict
//! in the ordinary PR job.

use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::ids::{check, Git};
use codeflow_core::workgraph::durable_work_tracking_enabled;

const RULE: &str = "work.id_registry";

/// Run the check for `codeflow ci` and record its findings and whether it ran.
pub(super) fn dispatch(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    match durable_work_tracking_enabled(root) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: super::tracking_state_violation(error),
            });
            ran.push("id-registry");
            return;
        }
    }
    let resolved_base = match super::resolve_base(root, base_candidates) {
        Ok(value) => value,
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: super::tracking_state_violation(error),
            });
            return;
        }
    };
    let Some(base) = resolved_base else {
        return;
    };
    ran.push("id-registry");
    match check::merge_rule(&Git::new(root), &base, head) {
        Ok(report) => {
            for line in &report.info {
                println!("codeflow ci: {line}");
            }
            for warn in report.warns {
                push(tagged, PolicyLevel::Warn, warn);
            }
            for block in report.blocks {
                push(tagged, PolicyLevel::Block, block);
            }
        }
        Err(error) => push(
            tagged,
            PolicyLevel::Block,
            format!("cannot judge the record ids of the range: {error}"),
        ),
    }
}

fn push(tagged: &mut Vec<super::TaggedViolation>, level: PolicyLevel, message: String) {
    let remedy = clearing(&message).remedy();
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::new(RULE, level, message, remedy),
    });
}

/// The step that clears a merge-rule message: each names its own cause.
fn clearing(message: &str) -> &'static codeflow_core::remedy::Clearing {
    use codeflow_core::remedy::{
        ID_REGISTRY, ID_REGISTRY_RETARGET, ID_REGISTRY_UID, ID_REGISTRY_UNFETCHED,
    };
    if message.contains("no `codeflow/registry` was fetched") {
        &ID_REGISTRY_UNFETCHED
    } else if message.contains("the uid of an existing record") {
        &ID_REGISTRY_UID
    } else if message.starts_with("collision:")
        || message.contains("retarget")
        || message.contains("it is a different record")
    {
        &ID_REGISTRY_RETARGET
    } else {
        &ID_REGISTRY
    }
}
