//! The `overrides` check (TSK-1302): which project overrides of managed
//! instructions apply, the shipped text each one replaces, and whether
//! `codeflow update` has applied them.
//!
//! The check runs update's own override check, so a file update would
//! refuse fails here with the same refusal. Every name and text it prints
//! comes from the shipped assets: the targets were checked against the
//! shipped kernel and skills, and the replaced text is the shipped unit.
//! The project's replacement text is never printed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{input_failure, CheckResult, Options, Status};
use crate::remedy;
use crate::scaffold::overrides::{record, AppliedUnit, DestPlan, Target};

const NAME: &str = "overrides";

/// The longest replaced text printed for one unit, in characters.
const SHOWN_CHARS: usize = 120;

/// Run the check.
pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let result = |status: Status, message: String| CheckResult {
        name: NAME.into(),
        status,
        message,
        duration: start.elapsed(),
    };
    let Some(plan_of) = opts.override_plan else {
        return result(
            Status::Pass,
            "not checked: this build supplies no shipped instructions to check overrides against"
                .into(),
        );
    };
    let plan = match plan_of(&root) {
        Ok(plan) => plan,
        Err(error) => {
            return result(
                Status::Fail,
                format!("`codeflow update` would refuse the project overrides: {error}"),
            )
        }
    };
    if plan.dests.is_empty() {
        return result(Status::Pass, "no project overrides".into());
    }
    let mut applied = Vec::new();
    let mut pending = Vec::new();
    for (dest, dest_plan) in &plan.dests {
        let state = match applied_state(&root, dest, dest_plan) {
            Ok(state) => state,
            Err(error) => {
                let path = root.join(record::record_path(dest));
                return input_failure(NAME, start, &path, error);
            }
        };
        if !state {
            pending.push(dest.clone());
        }
        for unit in &dest_plan.applied {
            applied.push(describe(dest, unit));
        }
    }
    let listing = if applied.is_empty() {
        "no override applies; the shipped text is restored".to_string()
    } else {
        format!(
            "{} override(s) of managed instructions: {}",
            applied.len(),
            applied.join("; ")
        )
    };
    if pending.is_empty() {
        return result(Status::Pass, format!("{listing}; all applied"));
    }
    result(
        Status::Warn(remedy::DOCTOR_OVERRIDES_PENDING.remedy()),
        format!("{listing}; not yet applied to {}", pending.join(", ")),
    )
}

/// Whether update last applied exactly this plan to `dest`: its record
/// holds the same units with the same shipped and replacement text, and a
/// file whose overrides were all removed has no record left.
fn applied_state(
    root: &Path,
    dest: &str,
    plan: &DestPlan,
) -> Result<bool, crate::scaffold::ScaffoldError> {
    Ok(match record::Record::read(root, dest)? {
        Some(record) => record.applied == plan.applied,
        None => plan.applied.is_empty(),
    })
}

/// One applied unit as doctor prints it: the target, the file and the
/// shipped text it replaces.
fn describe(dest: &str, unit: &AppliedUnit) -> String {
    let replaced = match Target::parse(&unit.entry.target) {
        Some(Target::Section { .. }) => {
            let lines = unit.replaced.lines().count();
            let first = unit
                .replaced
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default();
            format!(
                "the section body ({lines} lines) starting \"{}\"",
                clip(first)
            )
        }
        _ => format!("\"{}\"", clip(&unit.replaced)),
    };
    format!("{} in {dest} replaces {replaced}", unit.entry.target)
}

/// `text` on one line, cut to [`SHOWN_CHARS`] characters.
fn clip(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= SHOWN_CHARS {
        return flat.to_string();
    }
    let cut: String = flat.chars().take(SHOWN_CHARS).collect();
    format!("{}...", cut.trim_end())
}
